use std::fs;
use std::path::Path;

use crate::config::{Config, CreateConfigError, InitFeature};
use crate::hooks::{HookExecutor, HookPhase};
use crate::renderer::{HandlebarsRenderer, Renderer};
use crate::sitemap::SitePages;
use crate::{
    add_assets, discover_collections, discover_static_pages, make_dist_folder,
    render_collection_items, render_pages, write_sitemap,
};

pub fn init(path: &Path, features: &[InitFeature]) {
    log::info!("Initializing new balzac project at {:?}", path);

    if !path.exists()
        && let Err(e) = fs::create_dir_all(path)
    {
        eprintln!("Error: Could not create project directory: {}", e);
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
                "Error: balzac.toml already exists at {:?}",
                path.join("balzac.toml")
            );
            std::process::exit(1);
        }
        Err(CreateConfigError::Io(e)) => {
            eprintln!("Error: Could not create balzac.toml: {}", e);
            std::process::exit(1);
        }
        Err(CreateConfigError::Serialize(e)) => {
            eprintln!("Error: Could not serialize config: {}", e);
            std::process::exit(1);
        }
    }

    let directories = ["pages", "layouts", "partials", "assets", "content"];
    for dir in &directories {
        let dir_path = path.join(dir);
        if !dir_path.exists() {
            if let Err(e) = fs::create_dir_all(&dir_path) {
                eprintln!("Error: Could not create {} directory: {}", dir, e);
                std::process::exit(1);
            }
            log::info!("Created directory: {}", dir);
        }
    }

    log::info!("Project initialized successfully!");
}

pub fn build(path: &Path) {
    let start = std::time::Instant::now();
    let config_path = path.join("balzac.toml");
    let config_content = fs::read_to_string(&config_path).expect("Config file not found");
    let parsed_config: Config = toml::from_str(&config_content).expect("Could not parse config");
    log::info!("Parsed configuration file (took {:?})", start.elapsed());

    let resolved_config = parsed_config.resolve(path);

    let hook_executor = HookExecutor::new(parsed_config.hooks.as_ref(), path);
    hook_executor.execute(HookPhase::RenderInitBefore);

    let start = std::time::Instant::now();
    let mut render: HandlebarsRenderer<'_> = HandlebarsRenderer::new(&resolved_config);
    render.init(&resolved_config);
    log::info!("Renderer is initialized (took {:?})", start.elapsed());

    hook_executor.execute(HookPhase::RenderInitAfter);
    hook_executor.execute(HookPhase::BuildBefore);

    let start = std::time::Instant::now();
    match make_dist_folder(&resolved_config) {
        Ok(()) => {}
        Err(e) => {
            log::error!("Error creating output directory: {}", e);
            std::process::exit(1);
        }
    }
    log::info!("Created output directory (took {:?})", start.elapsed());

    let start = std::time::Instant::now();
    let static_pages = match discover_static_pages(&resolved_config) {
        Ok(pages) => pages,
        Err(e) => {
            log::error!("Error discovering static pages: {}", e);
            std::process::exit(1);
        }
    };
    let collection_pages = match discover_collections(&resolved_config) {
        Ok(pages) => pages,
        Err(e) => {
            log::error!("Error discovering collections: {}", e);
            std::process::exit(1);
        }
    };

    let mut site_pages = SitePages::new();
    site_pages.add_pages(static_pages);
    site_pages.add_pages(collection_pages);
    log::info!(
        "Discovered {} pages (took {:?})",
        site_pages.all().len(),
        start.elapsed()
    );

    hook_executor.execute(HookPhase::RenderBefore);

    let start = std::time::Instant::now();
    match render_pages(&resolved_config, site_pages.all(), &render) {
        Ok(()) => {}
        Err(e) => {
            log::error!("Error rendering static pages: {}", e);
            std::process::exit(1);
        }
    }
    log::info!("Rendered static pages (took {:?})", start.elapsed());

    let start = std::time::Instant::now();
    match render_collection_items(&resolved_config, site_pages.all(), &render) {
        Ok(()) => {}
        Err(e) => {
            log::error!("Error rendering collections: {}", e);
            std::process::exit(1);
        }
    }
    log::info!("Rendered collections (took {:?})", start.elapsed());

    hook_executor.execute(HookPhase::RenderAfter);

    let start = std::time::Instant::now();
    match write_sitemap(&resolved_config, &site_pages) {
        Ok(()) => {}
        Err(e) => {
            log::error!("Error writing sitemap: {}", e);
            std::process::exit(1);
        }
    }
    if resolved_config.base_url.is_some() {
        log::info!("Generated sitemap (took {:?})", start.elapsed());
    }

    let start = std::time::Instant::now();
    match add_assets(&resolved_config) {
        Ok(()) => {}
        Err(e) => {
            log::error!("Error handling assets: {}", e);
            std::process::exit(1);
        }
    }
    log::info!("Handled assets (took {:?})", start.elapsed());

    hook_executor.execute(HookPhase::BuildAfter);
}
