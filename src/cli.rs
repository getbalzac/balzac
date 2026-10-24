use std::fs;
use std::path::Path;

use crate::cache::{self, BuildCache, FullBuildReason};
use crate::config::{Config, CreateConfigError, InitFeature, ResolvedConfig};
use crate::dev;
use crate::helpers::track_clock;
use crate::hooks::{HookExecutor, HookPhase};
use crate::markdown;
use crate::renderer::{HandlebarsRenderer, Renderer};
use crate::sitemap::SitePages;
use crate::{
    discover_site_pages, make_dist_folder, render_selected_pages, write_feeds, write_robots,
    write_sitemap,
};

pub fn init(path: &Path, features: &[InitFeature]) {
    log::info!("Initializing new balzac project at {}", path.display());

    if !path.exists()
        && let Err(e) = fs::create_dir_all(path)
    {
        eprintln!("Error: Could not create project directory: {e}");
        std::process::exit(1);
    }

    let features = if features.is_empty() {
        None
    } else {
        Some(features)
    };

    match Config::create(path, features) {
        Ok(()) => log::info!("Created balzac.toml"),
        Err(CreateConfigError::AlreadyExists) => {
            eprintln!(
                "Error: balzac.toml already exists at {}",
                path.join("balzac.toml").display()
            );
            std::process::exit(1);
        }
        Err(CreateConfigError::Io(e)) => {
            eprintln!("Error: Could not create balzac.toml: {e}");
            std::process::exit(1);
        }
        Err(CreateConfigError::Serialize(e)) => {
            eprintln!("Error: Could not serialize config: {e}");
            std::process::exit(1);
        }
    }

    let directories = ["pages", "layouts", "partials", "assets", "content", "tags"];
    for dir in &directories {
        let dir_path = path.join(dir);
        if !dir_path.exists() {
            if let Err(e) = fs::create_dir_all(&dir_path) {
                eprintln!("Error: Could not create {dir} directory: {e}");
                std::process::exit(1);
            }
            log::info!("Created directory: {dir}");
        }
    }

    if let Err(e) = ignore_state_directory(path) {
        eprintln!("Error: Could not update .gitignore: {e}");
        std::process::exit(1);
    }

    log::info!("Project initialized successfully!");
}

/// Adds Balzac's state directory (the build cache) to the project's `.gitignore`.
fn ignore_state_directory(path: &Path) -> std::io::Result<()> {
    let entry = format!("/{}/", cache::STATE_DIRECTORY);
    let gitignore = path.join(".gitignore");
    let existing = match fs::read_to_string(&gitignore) {
        Ok(existing) => existing,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let ignored = existing.lines().any(|line| {
        let line = line.trim().trim_start_matches('/').trim_end_matches('/');
        line == cache::STATE_DIRECTORY
    });
    if ignored {
        return Ok(());
    }
    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    fs::write(&gitignore, format!("{existing}{separator}{entry}\n"))?;
    log::info!("Added {entry} to .gitignore");
    Ok(())
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BuildOptions {
    /// Ignore the build cache and rebuild everything.
    pub force: bool,
}

pub fn build(path: &Path, options: BuildOptions) {
    let start = std::time::Instant::now();
    let parsed_config =
        Config::load(path).unwrap_or_else(|e| fail("Error loading configuration", &e));
    log::info!("Parsed configuration file (took {:?})", start.elapsed());

    let resolved_config = parsed_config.resolve(path);

    let mut cache = BuildCache::open(&resolved_config, options.force)
        .unwrap_or_else(|e| fail("Error opening build cache", &e.into()));

    let hook_executor = HookExecutor::new(parsed_config.hooks.as_ref(), path);
    hook_executor.execute(HookPhase::RenderInitBefore);

    // Hash the shared inputs before the renderer reads them, so a hook editing
    // them later is picked up by the next build.
    let global = cache::global_key(&parsed_config, &resolved_config)
        .unwrap_or_else(|e| fail("Error hashing build inputs", &e.into()));
    cache.start(global);

    let start = std::time::Instant::now();
    let mut render: HandlebarsRenderer<'_> = HandlebarsRenderer::new(&resolved_config);
    if let Err(e) = render.init(&resolved_config) {
        fail("Error initializing renderer", &e);
    }
    log::info!("Renderer is initialized (took {:?})", start.elapsed());

    hook_executor.execute(HookPhase::RenderInitAfter);
    hook_executor.execute(HookPhase::BuildBefore);

    prepare_output_directory(&resolved_config, &cache);

    let start = std::time::Instant::now();
    let site_pages = match discover_site_pages(&resolved_config) {
        Ok(pages) => pages,
        Err(e) => {
            log::error!("Error discovering pages: {e}");
            std::process::exit(1);
        }
    };
    log::info!(
        "Discovered {} pages (took {:?})",
        site_pages.all().len(),
        start.elapsed()
    );

    hook_executor.execute(HookPhase::RenderBefore);

    render_changed_pages(&resolved_config, &site_pages, &render, &mut cache);

    hook_executor.execute(HookPhase::RenderAfter);

    let start = std::time::Instant::now();
    match write_sitemap(&resolved_config, &site_pages) {
        Ok(()) => {}
        Err(e) => {
            log::error!("Error writing sitemap: {e}");
            std::process::exit(1);
        }
    }
    if resolved_config.base_url.is_some() {
        log::info!("Generated sitemap (took {:?})", start.elapsed());
    }

    if let Err(e) = write_robots(&resolved_config) {
        log::error!("Error writing robots.txt: {e}");
        std::process::exit(1);
    }

    match markdown::write_highlight_css(&resolved_config) {
        Ok(()) => {}
        Err(e) => {
            log::error!("Error writing highlight theme: {e}");
            std::process::exit(1);
        }
    }

    build_feeds(&resolved_config, &site_pages, &render, &mut cache);

    let start = std::time::Instant::now();
    match cache.sync_assets(&resolved_config) {
        Ok(()) => {}
        Err(e) => {
            log::error!("Error handling assets: {e}");
            std::process::exit(1);
        }
    }
    log::info!("Handled assets (took {:?})", start.elapsed());

    match cache.prune_stale_outputs() {
        Ok(0) => {}
        Ok(removed) => log::info!("Removed {removed} stale output files"),
        Err(e) => {
            log::error!("Error removing stale output files: {e}");
            std::process::exit(1);
        }
    }

    hook_executor.execute(HookPhase::BuildAfter);

    if let Err(e) = cache.save() {
        // the build itself succeeded; the next one just starts from scratch
        log::warn!("Could not write build cache: {e}");
    }
}

/// Recreates the output directory for a full build, or keeps it for an
/// incremental one.
fn prepare_output_directory(resolved_config: &ResolvedConfig, cache: &BuildCache) {
    let start = std::time::Instant::now();
    let output_ready = match cache.full_build_reason() {
        Some(reason) => {
            log::info!(
                "{}, rebuilding everything",
                match reason {
                    FullBuildReason::Forced => "--force given",
                    FullBuildReason::NoCache => "No build cache",
                    FullBuildReason::GlobalChange =>
                        "Site-wide inputs changed (config, partials, layouts, tag templates, helpers, ...)",
                }
            );
            make_dist_folder(resolved_config)
        }
        None => fs::create_dir_all(&resolved_config.output_directory),
    };
    if let Err(e) = output_ready {
        log::error!("Error creating output directory: {e}");
        std::process::exit(1);
    }
    log::info!("Prepared output directory (took {:?})", start.elapsed());
}

/// Renders the pages whose inputs or output changed since the last build.
fn render_changed_pages(
    resolved_config: &ResolvedConfig,
    site_pages: &SitePages,
    render: &HandlebarsRenderer,
    cache: &mut BuildCache,
) {
    let start = std::time::Instant::now();
    let selected = cache
        .plan_pages(resolved_config, site_pages.all())
        .unwrap_or_else(|e| fail("Error checking build cache", &e.into()));
    let rendered = render_selected_pages(resolved_config, site_pages.all(), render, |page| {
        selected.contains(&page.output_path)
    })
    .unwrap_or_else(|e| fail("Error rendering pages", &e));
    if let Err(e) = cache.finish_pages(&rendered) {
        fail("Error updating build cache", &e.into());
    }
    log::info!(
        "Rendered {} pages, {} unchanged (took {:?})",
        rendered.pages.len(),
        cache.reused_pages(),
        start.elapsed()
    );
}

fn build_feeds(
    resolved_config: &ResolvedConfig,
    site_pages: &SitePages,
    render: &HandlebarsRenderer,
    cache: &mut BuildCache,
) {
    if resolved_config.feed.is_none() {
        return;
    }

    let start = std::time::Instant::now();
    match cache.feeds_fresh() {
        Ok(true) => {
            log::info!("Feeds are unchanged");
            return;
        }
        Ok(false) => {}
        Err(e) => fail("Error checking build cache", &e.into()),
    }
    let (written, clock) = track_clock(|| write_feeds(resolved_config, site_pages, render));
    let written = written.unwrap_or_else(|e| fail("Error writing feeds", &e));
    if let Err(e) = cache.record_feeds(&written, clock) {
        fail("Error updating build cache", &e.into());
    }
    log::info!("Generated feeds (took {:?})", start.elapsed());
}

fn fail(context: &str, error: &crate::error::Error) -> ! {
    log::error!("{context}: {error}");
    std::process::exit(1);
}

pub fn dev(path: &Path, host: &str, port: u16) {
    if let Err(error) = dev::run(path, host, port) {
        log::error!("Error running dev server: {error}");
        std::process::exit(1);
    }
}
