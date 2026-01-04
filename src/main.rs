use facet_toml::from_str;
use std::fs;

use balzac::{
    add_assets, config, make_dist_folder, render_collections, render_static_pages, renderer,
};

use balzac::renderer::Renderer;

fn main() {
    colog::init();
    let config_content = fs::read_to_string("./balzac.toml").expect("Config file not found");
    let parsed_config: config::Config = from_str(&config_content).expect("Could not parse config");
    log::info!("Parsed configuration file");
    let mut render = renderer::HandlebarsRenderer::new(&parsed_config);
    render.init(&parsed_config);
    log::info!("Renderer is initialized");
    if let Err(e) = make_dist_folder(&parsed_config) {
        log::error!("Error creating output directory: {}", e);
        std::process::exit(1);
    }

    if let Err(e) = render_static_pages(&parsed_config, render) {
        log::error!("Error rendering static pages: {}", e);
        std::process::exit(1);
    }

    if let Err(e) = render_collections(&parsed_config) {
        log::error!("Error rendering collections: {}", e);
        std::process::exit(1);
    }

    if let Err(e) = add_assets(&parsed_config) {
        log::error!("Error handling assets: {}", e);
        std::process::exit(1);
    }
}
