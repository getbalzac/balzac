use shell_words::split;
use std::fs;
use std::process::Command;
use toml::from_str;

use balzac::{
    add_assets, config, make_dist_folder, render_collections, render_static_pages, renderer,
};

use balzac::renderer::Renderer;

fn main() {
    colog::init();
    let start = std::time::Instant::now();
    let config_content = fs::read_to_string("./balzac.toml").expect("Config file not found");
    let parsed_config: config::Config = from_str(&config_content).expect("Could not parse config");
    log::info!("Parsed configuration file (took {:?})", start.elapsed());
    let start = std::time::Instant::now();
    let mut render: renderer::HandlebarsRenderer<'_> =
        renderer::HandlebarsRenderer::new(&parsed_config);
    render.init(&parsed_config);
    log::info!("Renderer is initialized (took {:?})", start.elapsed());
    if parsed_config.hooks.is_some()
        && parsed_config
            .hooks
            .as_ref()
            .expect("could not get hooks")
            .build_before
            .is_some()
    {
        log::info!("Found build_before hook, running");
        let start = std::time::Instant::now();
        let hook = &parsed_config
            .hooks
            .as_ref()
            .unwrap()
            .build_before
            .as_ref()
            .unwrap();
        let parts = split(hook).expect("Invalid command syntax");
        let mut cmd = Command::new(&parts[0]);
        for arg in &parts[1..] {
            cmd.arg(arg);
        }
        if let Err(e) = cmd.status() {
            log::error!("Error running build_before hook: {}", e);
            std::process::exit(1);
        }
        log::info!("build_before hook completed (took {:?})", start.elapsed());
    }
    let start = std::time::Instant::now();
    if let Err(e) = make_dist_folder(&parsed_config) {
        log::error!("Error creating output directory: {}", e);
        std::process::exit(1);
    }
    log::info!("Created output directory (took {:?})", start.elapsed());

    let start = std::time::Instant::now();
    if let Err(e) = render_static_pages(&parsed_config, &render) {
        log::error!("Error rendering static pages: {}", e);
        std::process::exit(1);
    }
    log::info!("Rendered static pages (took {:?})", start.elapsed());

    let start = std::time::Instant::now();
    if let Err(e) = render_collections(&parsed_config, &render) {
        log::error!("Error rendering collections: {}", e);
        std::process::exit(1);
    }
    log::info!("Rendered collections (took {:?})", start.elapsed());

    if parsed_config.hooks.is_some() && parsed_config.hooks.as_ref().unwrap().build_after.is_some()
    {
        log::info!("Found build_after hook, running");
        let start = std::time::Instant::now();
        let hook = &parsed_config
            .hooks
            .as_ref()
            .unwrap()
            .build_after
            .as_ref()
            .unwrap();
        let parts = split(hook).expect("Invalid command syntax");
        let mut cmd = Command::new(&parts[0]);
        for arg in &parts[1..] {
            cmd.arg(arg);
        }
        if let Err(e) = cmd.status() {
            log::error!("Error running build_after hook: {}", e);
            std::process::exit(1);
        }
        log::info!("build_after hook completed (took {:?})", start.elapsed());
    }

    let start = std::time::Instant::now();
    if let Err(e) = add_assets(&parsed_config) {
        log::error!("Error handling assets: {}", e);
        std::process::exit(1);
    }
    log::info!("Handled assets (took {:?})", start.elapsed());
}
