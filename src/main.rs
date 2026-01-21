use std::fs;
use toml::from_str;

use balzac::hooks::{HookExecutor, HookPhase};
use balzac::renderer::Renderer;
use balzac::sitemap::SitePages;
use balzac::{
    add_assets, config, discover_collections, discover_static_pages, make_dist_folder,
    render_collection_items, render_pages, renderer, write_sitemap,
};

fn main() {
    colog::init();
    let cmd = clap::Command::new("balzac")
        .bin_name("balzac")
        .subcommand_required(true)
        .subcommand(
            clap::command!("build")
                .about("Build project using balzac")
                .arg(
                    clap::arg!(--root <PATH>)
                        .value_parser(clap::value_parser!(std::path::PathBuf))
                        .required(false),
                ),
        );
    let matches = cmd.get_matches();

    let base_path = match matches.subcommand() {
        Some(("build", sub_matches)) => match sub_matches.try_get_one::<std::path::PathBuf>("root")
        {
            Ok(Some(path)) => path.clone(),
            Ok(None) => std::env::current_dir().unwrap_or_else(|e| {
                eprintln!("Error: Could not determine current directory: {}", e);
                std::process::exit(1);
            }),
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        },
        _ => unreachable!(),
    };

    let start = std::time::Instant::now();
    let config_path = base_path.join("balzac.toml");
    let config_content = fs::read_to_string(&config_path).expect("Config file not found");
    let parsed_config: config::Config = from_str(&config_content).expect("Could not parse config");
    log::info!("Parsed configuration file (took {:?})", start.elapsed());

    let resolved_config = parsed_config.resolve(&base_path);

    let hook_executor = HookExecutor::new(parsed_config.hooks.as_ref(), &base_path);
    hook_executor.execute(HookPhase::RenderInitBefore);

    let start = std::time::Instant::now();
    let mut render: renderer::HandlebarsRenderer<'_> =
        renderer::HandlebarsRenderer::new(&resolved_config);
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
