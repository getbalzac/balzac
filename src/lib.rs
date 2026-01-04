pub mod collection;
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

pub fn add_assets(parsed_config: &config::Config) -> std::io::Result<()> {
    let dir_exists = fs::exists(&parsed_config.assets_directory)?;

    if dir_exists {
        fs::create_dir(PathBuf::from(&parsed_config.output_directory).join("assets"))?;
        for entry in fs::read_dir(&parsed_config.assets_directory)? {
            let dir = entry?;

            let is_directory = dir.metadata()?.is_dir();
            if is_directory {
                log::info!(
                    "entry {} is a directory, skipping",
                    dir.file_name().to_string_lossy()
                );
                continue;
            }

            let path = PathBuf::from(&parsed_config.output_directory)
                .join("assets")
                .join(dir.file_name());
            log::info!(
                "Copying {} to {}",
                dir.file_name().to_string_lossy(),
                path.to_string_lossy()
            );
            fs::copy(dir.path(), path)?;
        }
    } else {
        log::info!("Assets directory does not exist, skipping");
    }

    Ok(())
}

pub fn render_collections(parsed_config: &config::Config) -> std::io::Result<()> {
    let dir_exists = fs::exists(&parsed_config.content_directory)
        .expect("Could not check if the directory exists");

    if dir_exists {
        for entry in fs::read_dir(&parsed_config.content_directory)? {
            let dir = entry?;

            if dir.metadata()?.is_file() {
                log::info!(
                    "Entry {} is a file; this is not allowed, skipping",
                    dir.file_name().to_string_lossy()
                );
                continue;
            }
            log::info!("Rendering collection {}", dir.file_name().to_string_lossy());
            let has_details_page = fs::exists(
                PathBuf::from(&parsed_config.pages_directory)
                    .join(dir.file_name())
                    .join("details.hbs"),
            )?;

            if !has_details_page {
                log::error!(
                    "Collection {} has no details page",
                    dir.file_name().to_string_lossy()
                );
                continue;
            }
        }
    } else {
        log::info!("Content directory does not exist, skipping");
    }

    Ok(())
}

pub fn render_static_pages(parsed_config: &config::Config) -> std::io::Result<()> {
    let mut render = renderer::HandlebarsRenderer::new(parsed_config);
    render.init(parsed_config);
    for entry in fs::read_dir(&parsed_config.pages_directory)? {
        let dir = entry?;
        if dir.metadata()?.is_dir() {
            log::info!("Skipping directory {}", dir.file_name().to_string_lossy());
            continue;
        }
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
