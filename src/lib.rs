pub mod config;
pub mod renderer;

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::renderer::Renderer;

pub fn make_dist_folder(parsed_config: &config::Config) -> std::io::Result<()> {
    let dir_exists = fs::exists(&parsed_config.output_directory)?;
    if !dir_exists {
        log::info!(
            "Creating output directory {}",
            parsed_config.output_directory
        );
        fs::create_dir(&parsed_config.output_directory)?;
    } else {
        log::info!(
            "Output directory {} already exists, recreating",
            parsed_config.output_directory
        );
        fs::remove_dir_all(&parsed_config.output_directory)?;
        fs::create_dir(&parsed_config.output_directory)?;
    }
    Ok(())
}

pub fn render_static_pages(parsed_config: &config::Config) -> std::io::Result<()> {
    let render = renderer::HandlebarsRenderer {};
    for entry in fs::read_dir(&parsed_config.pages_directory)? {
        let dir = entry?;
        log::info!(
            "Rendering page {}",
            dir.file_name()
                .into_string()
                .expect("Could not get filename as a string")
        );
        let entry_path = dir.path();
        let content = fs::read_to_string(&entry_path)?;
        let rendered = render.render(content, serde_json::json!(&parsed_config.global));
        let file_path = &entry_path.file_stem().expect("Could not get file stem");
        fs::write(
            Path::new(&parsed_config.output_directory)
                .join(PathBuf::from(file_path).with_extension("html")),
            rendered,
        )?;
    }
    Ok(())
}
