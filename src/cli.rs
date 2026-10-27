use std::fs;
use std::path::Path;

use crate::config::{Config, CreateConfigError, InitFeature, ResolvedConfig};
use crate::context::SiteContext;
use crate::dev;
use crate::hooks::{HookExecutor, HookPhase};
use crate::listing::{render_listing_pages, slugify};
use crate::markdown;
use crate::renderer::{HandlebarsRenderer, Renderer};
use crate::sitemap::SitePages;
use crate::{
    add_assets, discover_site_pages, make_dist_folder, render_collection_items, render_pages,
    write_feeds, write_robots, write_sitemap,
};

/// Creates a project at `path`: from the git repository `template` when
/// given, otherwise from the embedded starter site.
pub fn init(path: &Path, features: &[InitFeature], template: Option<&str>) {
    log::info!("Initializing new balzac project at {}", path.display());

    if !path.exists()
        && let Err(e) = fs::create_dir_all(path)
    {
        eprintln!("Error: Could not create project directory: {e}");
        std::process::exit(1);
    }

    if let Some(template) = template {
        init_from_template(path, template);
        log::info!("Project initialized successfully!");
        return;
    }

    create_config(path, features);

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

    write_starter_files(path);

    log::info!("Project initialized successfully!");
}

fn create_config(path: &Path, features: &[InitFeature]) {
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
}

/// Clones `template` with `git` into a temporary directory and copies its
/// files (without `.git`) into `path`, keeping files that already exist. A
/// default `balzac.toml` is created when the template has none.
fn init_from_template(path: &Path, template: &str) {
    let config_path = path.join("balzac.toml");
    if config_path.exists() {
        eprintln!(
            "Error: balzac.toml already exists at {}",
            config_path.display()
        );
        std::process::exit(1);
    }

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let clone_directory =
        std::env::temp_dir().join(format!("balzac-template-{}-{nanos}", std::process::id()));

    log::info!("Cloning template {template}");
    let status = std::process::Command::new("git")
        .args(["clone", "--depth", "1", "--quiet", "--", template])
        .arg(&clone_directory)
        .status();
    let copied = match status {
        Ok(status) if status.success() => copy_template(&clone_directory, path, Path::new(""))
            .map_err(|e| format!("Could not copy template files: {e}")),
        Ok(status) => Err(format!("git clone of {template} failed ({status})")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err("git is required to use --template, but it was not found".to_string())
        }
        Err(e) => Err(format!("Could not run git: {e}")),
    };

    if clone_directory.exists()
        && let Err(e) = fs::remove_dir_all(&clone_directory)
    {
        log::warn!(
            "Could not remove temporary clone {}: {e}",
            clone_directory.display()
        );
    }

    if let Err(message) = copied {
        eprintln!("Error: {message}");
        std::process::exit(1);
    }

    if !config_path.exists() {
        create_config(path, &[]);
    }
}

/// Copies `source/relative` into `destination/relative` recursively, skipping
/// the top-level `.git` directory and files that already exist. Symlinks are
/// recreated as symlinks on Unix and skipped elsewhere.
fn copy_template(source: &Path, destination: &Path, relative: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination.join(relative))?;
    for entry in fs::read_dir(source.join(relative))? {
        let entry = entry?;
        let relative_path = relative.join(entry.file_name());
        if relative_path == Path::new(".git") {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_template(source, destination, &relative_path)?;
            continue;
        }
        let target = destination.join(&relative_path);
        // `symlink_metadata` so a dangling symlink also counts as existing.
        if fs::symlink_metadata(&target).is_ok() {
            log::info!("Skipped {}: file already exists", relative_path.display());
            continue;
        }
        if file_type.is_symlink() {
            // Recreate symlinks rather than copying their target, which fails
            // for symlinks to directories.
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(fs::read_link(entry.path())?, &target)?;
                log::info!("Created {} (symlink)", relative_path.display());
            }
            #[cfg(not(unix))]
            log::warn!(
                "Skipped {}: symlinks in templates are only supported on Unix",
                relative_path.display()
            );
            continue;
        }
        fs::copy(entry.path(), &target)?;
        log::info!("Created {}", relative_path.display());
    }
    Ok(())
}

/// Files written by `balzac init` so a new project builds to a working site,
/// relative to the project root. `{{date}}` in the sample post is replaced
/// with today's date.
const STARTER_FILES: [(&str, &str); 5] = [
    ("layouts/base.hbs", include_str!("starter/layouts/base.hbs")),
    ("pages/index.hbs", include_str!("starter/pages/index.hbs")),
    (
        "pages/posts/details.hbs",
        include_str!("starter/pages/posts/details.hbs"),
    ),
    (
        "content/posts/hello-world.md",
        include_str!("starter/content/posts/hello-world.md"),
    ),
    ("assets/style.css", include_str!("starter/assets/style.css")),
];

/// Writes the starter files, leaving any file that already exists untouched.
fn write_starter_files(root: &Path) {
    let today = today();
    for (relative_path, content) in STARTER_FILES {
        let file_path = root.join(relative_path);
        if file_path.exists() {
            log::info!("Skipped {relative_path}: file already exists");
            continue;
        }
        let content = content.replace("{{date}}", &today);
        let written = file_path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(&file_path, content));
        if let Err(e) = written {
            eprintln!("Error: Could not create {relative_path}: {e}");
            std::process::exit(1);
        }
        log::info!("Created {relative_path}");
    }
}

/// Today's date in UTC as `YYYY-MM-DD`, the format collection items are
/// sorted by.
fn today() -> String {
    jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date()
        .to_string()
}

/// Creates `<content_directory>/<collection>/<slug>.md` with a draft
/// frontmatter, the slug being derived from `title`.
pub fn new_content(root: &Path, collection: &str, title: &str) {
    let parsed_config =
        Config::load(root).unwrap_or_else(|e| fail("Error loading configuration", &e));
    let resolved_config = parsed_config.resolve(root);

    if !is_single_path_segment(collection) {
        log::error!(
            "Invalid collection name {collection:?}: it must be a single folder name, like \"posts\""
        );
        std::process::exit(1);
    }

    let slug = slugify(title);
    if slug.is_empty() {
        log::error!("Could not derive a file name from the title {title:?}");
        std::process::exit(1);
    }

    let collection_directory = resolved_config.content_directory.join(collection);
    let file_path = collection_directory.join(format!("{slug}.md"));
    if file_path.exists() {
        log::error!("{} already exists", file_path.display());
        std::process::exit(1);
    }

    // A JSON string is a valid double-quoted YAML scalar, so any title is
    // escaped correctly.
    let quoted_title =
        serde_json::to_string(title).expect("serializing a string to JSON cannot fail");
    let content = format!(
        "---\ntitle: {quoted_title}\ndate: {}\ndraft: true\n---\n\n",
        today()
    );

    if let Err(e) =
        fs::create_dir_all(&collection_directory).and_then(|()| fs::write(&file_path, content))
    {
        log::error!("Could not create {}: {e}", file_path.display());
        std::process::exit(1);
    }
    log::info!("Created {}", file_path.display());

    let template_path = resolved_config.collection_template_path(collection);
    if !template_path.exists() {
        log::warn!(
            "{} does not exist: items of the {collection} collection are not rendered until it is created",
            template_path.display()
        );
    }
}

fn is_single_path_segment(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(_)), None)
    ) && !name.contains(['/', '\\'])
}

/// Removes the output directory, refusing to when it is or contains the
/// project root or a source directory.
pub fn clean(root: &Path) {
    let parsed_config =
        Config::load(root).unwrap_or_else(|e| fail("Error loading configuration", &e));
    let resolved_config = parsed_config.resolve(root);
    let output_directory = &resolved_config.output_directory;

    let output = match fs::canonicalize(output_directory) {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log::info!(
                "Output directory {} does not exist, nothing to clean",
                output_directory.display()
            );
            return;
        }
        Err(e) => {
            log::error!(
                "Could not resolve output directory {}: {e}",
                output_directory.display()
            );
            std::process::exit(1);
        }
    };

    if !output.is_dir() {
        log::error!(
            "Output directory {} is not a directory, refusing to remove it",
            output_directory.display()
        );
        std::process::exit(1);
    }

    let protected = [
        ("project root", &resolved_config.root_directory),
        ("pages directory", &resolved_config.pages_directory),
        ("partials directory", &resolved_config.partials_directory),
        ("layouts directory", &resolved_config.layouts_directory),
        ("assets directory", &resolved_config.assets_directory),
        ("content directory", &resolved_config.content_directory),
        ("helpers directory", &resolved_config.helpers_directory),
        ("tags directory", &resolved_config.tags_directory),
    ];
    for (name, directory) in protected {
        // A directory that does not exist cannot be removed with the output.
        let Ok(directory) = fs::canonicalize(directory) else {
            continue;
        };
        if directory.starts_with(&output) {
            log::error!(
                "Output directory {} contains the {name} ({}), refusing to remove it",
                output_directory.display(),
                directory.display()
            );
            std::process::exit(1);
        }
    }

    if let Err(e) = fs::remove_dir_all(output_directory) {
        log::error!(
            "Could not remove output directory {}: {e}",
            output_directory.display()
        );
        std::process::exit(1);
    }
    log::info!("Removed output directory {}", output_directory.display());
}

pub fn build(path: &Path) {
    let start = std::time::Instant::now();
    let parsed_config =
        Config::load(path).unwrap_or_else(|e| fail("Error loading configuration", &e));
    log::info!("Parsed configuration file (took {:?})", start.elapsed());

    let resolved_config = parsed_config.resolve(path);

    let hook_executor = HookExecutor::new(parsed_config.hooks.as_ref(), path);
    hook_executor.execute(HookPhase::RenderInitBefore);

    let start = std::time::Instant::now();
    let mut render: HandlebarsRenderer<'_> = HandlebarsRenderer::new(&resolved_config);
    if let Err(e) = render.init(&resolved_config) {
        fail("Error initializing renderer", &e);
    }
    log::info!("Renderer is initialized (took {:?})", start.elapsed());

    hook_executor.execute(HookPhase::RenderInitAfter);
    hook_executor.execute(HookPhase::BuildBefore);

    let start = std::time::Instant::now();
    match make_dist_folder(&resolved_config) {
        Ok(()) => {}
        Err(e) => {
            log::error!("Error creating output directory: {e}");
            std::process::exit(1);
        }
    }
    log::info!("Created output directory (took {:?})", start.elapsed());

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

    render_site(&resolved_config, &site_pages, &render);

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

    build_feeds(&resolved_config, &site_pages, &render);

    let start = std::time::Instant::now();
    match add_assets(&resolved_config) {
        Ok(()) => {}
        Err(e) => {
            log::error!("Error handling assets: {e}");
            std::process::exit(1);
        }
    }
    log::info!("Handled assets (took {:?})", start.elapsed());

    hook_executor.execute(HookPhase::BuildAfter);
}

/// Renders every page of the site, exiting on the first error.
fn render_site(
    resolved_config: &ResolvedConfig,
    site_pages: &SitePages,
    render: &HandlebarsRenderer,
) {
    let start = std::time::Instant::now();
    let site = match SiteContext::new(resolved_config, site_pages.all(), render) {
        Ok(site) => site,
        Err(e) => {
            log::error!("Error rendering collection content: {e}");
            std::process::exit(1);
        }
    };
    log::info!("Rendered collection content (took {:?})", start.elapsed());

    let start = std::time::Instant::now();
    match render_pages(resolved_config, &site, render) {
        Ok(()) => {}
        Err(e) => {
            log::error!("Error rendering static pages: {e}");
            std::process::exit(1);
        }
    }
    log::info!("Rendered static pages (took {:?})", start.elapsed());

    let start = std::time::Instant::now();
    match render_collection_items(resolved_config, &site, render) {
        Ok(()) => {}
        Err(e) => {
            log::error!("Error rendering collections: {e}");
            std::process::exit(1);
        }
    }
    log::info!("Rendered collections (took {:?})", start.elapsed());

    let start = std::time::Instant::now();
    match render_listing_pages(&site, render) {
        Ok(()) => {}
        Err(e) => {
            log::error!("Error rendering listings: {e}");
            std::process::exit(1);
        }
    }
    log::info!("Rendered listings (took {:?})", start.elapsed());
}

fn build_feeds(
    resolved_config: &ResolvedConfig,
    site_pages: &SitePages,
    render: &HandlebarsRenderer,
) {
    if resolved_config.feed.is_none() {
        return;
    }

    let start = std::time::Instant::now();
    if let Err(e) = write_feeds(resolved_config, site_pages, render) {
        log::error!("Error writing feeds: {e}");
        std::process::exit(1);
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
