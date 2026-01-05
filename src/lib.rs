pub mod collection;
pub mod config;
pub mod context;
pub mod renderer;

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    context::merge_contexts,
    renderer::{HandlebarsRenderer, Renderer},
};

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

pub fn render_collections(
    parsed_config: &config::Config,
    render: &HandlebarsRenderer,
) -> std::io::Result<()> {
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
            fs::create_dir(PathBuf::from(&parsed_config.output_directory).join(dir.file_name()))?;
            let details_page_path = PathBuf::from(&parsed_config.pages_directory)
                .join(dir.file_name())
                .join("details.hbs");
            let has_details_page = fs::exists(&details_page_path)?;

            if !has_details_page {
                log::error!(
                    "Collection {} has no details page",
                    dir.file_name().to_string_lossy()
                );
                continue;
            }

            let content_dir_path =
                PathBuf::from(&parsed_config.content_directory).join(dir.file_name());

            for content_entry in fs::read_dir(&content_dir_path)? {
                let content_dir = content_entry?;
                let content_dir_path = content_dir.path();

                if content_dir_path
                    .extension()
                    .expect("Could not get extension for collection file")
                    .to_string_lossy()
                    != "md"
                {
                    log::error!(
                        "File {} is not a markdown file, skipping",
                        content_dir.file_name().to_string_lossy()
                    );
                    continue;
                }
                let content_filename = content_dir_path
                    .file_stem()
                    .expect("Could not get collection entry file stem");
                let rendered_content = collection::parse_markdown(content_dir.path())?;
                let rendered_output_path = PathBuf::from(&parsed_config.output_directory)
                    .join(dir.file_name())
                    .join(content_filename)
                    .with_extension("html");

                let rendered_result = render.render(
                    fs::read_to_string(&details_page_path)?,
                    merge_contexts(
                        parsed_config,
                        serde_json::json!({"content": rendered_content}),
                    ),
                );

                fs::write(&rendered_output_path, &rendered_result)?;
            }
        }
    } else {
        log::info!("Content directory does not exist, skipping");
    }

    Ok(())
}

pub fn render_static_pages(
    parsed_config: &config::Config,
    render: &HandlebarsRenderer,
) -> std::io::Result<()> {
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
