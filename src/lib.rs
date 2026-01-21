pub mod collection;
pub mod config;
pub mod context;
pub mod hooks;
pub mod renderer;
pub mod sitemap;
pub mod vite;

use std::{fs, path::PathBuf};

use crate::{
    context::merge_contexts,
    renderer::{HandlebarsRenderer, Renderer},
    sitemap::{PageEntry, PageType, SitePages, SitemapMeta},
};

pub fn make_dist_folder(parsed_config: &config::ResolvedConfig) -> std::io::Result<()> {
    let dir_exists = fs::exists(&parsed_config.output_directory)?;
    if !dir_exists {
        log::debug!(
            "Creating output directory {}",
            parsed_config.output_directory.display()
        );
        fs::create_dir(&parsed_config.output_directory)?;
    } else {
        log::debug!(
            "Output directory {} already exists, recreating",
            parsed_config.output_directory.display()
        );
        fs::remove_dir_all(&parsed_config.output_directory)?;
        fs::create_dir(&parsed_config.output_directory)?;
    }
    Ok(())
}

pub fn add_assets(parsed_config: &config::ResolvedConfig) -> std::io::Result<()> {
    let dir_exists = fs::exists(&parsed_config.assets_directory)?;

    if dir_exists {
        fs::create_dir(parsed_config.output_directory.join("assets"))?;
        for entry in fs::read_dir(&parsed_config.assets_directory)? {
            let dir = entry?;

            let is_directory = dir.metadata()?.is_dir();
            if is_directory {
                log::debug!(
                    "entry {} is a directory, skipping",
                    dir.file_name().to_string_lossy()
                );
                continue;
            }

            let path = parsed_config
                .output_directory
                .join("assets")
                .join(dir.file_name());
            log::debug!(
                "Copying {} to {}",
                dir.file_name().to_string_lossy(),
                path.display()
            );
            fs::copy(dir.path(), path)?;
        }
    } else {
        log::debug!("Assets directory does not exist, skipping");
    }

    Ok(())
}

pub fn discover_static_pages(
    parsed_config: &config::ResolvedConfig,
) -> std::io::Result<Vec<PageEntry>> {
    let mut pages = Vec::new();

    let dir_exists = fs::exists(&parsed_config.pages_directory)?;
    if !dir_exists {
        log::debug!("Pages directory does not exist, skipping discovery");
        return Ok(pages);
    }

    for entry in fs::read_dir(&parsed_config.pages_directory)? {
        let dir = entry?;
        if dir.metadata()?.is_dir() {
            log::debug!(
                "Skipping directory {} during discovery",
                dir.file_name().to_string_lossy()
            );
            continue;
        }

        let entry_path = dir.path();

        let extension = entry_path
            .extension()
            .map(|e| e.to_string_lossy().to_string());
        let is_page_file = matches!(
            extension.as_deref(),
            Some("hbs") | Some("handlebars") | Some("html")
        );
        if !is_page_file {
            log::debug!(
                "Skipping non-page file {} during discovery",
                dir.file_name().to_string_lossy()
            );
            continue;
        }

        let file_stem = entry_path
            .file_stem()
            .expect("Could not get file stem")
            .to_string_lossy()
            .to_string();

        let url_path = if file_stem == "index" {
            "/".to_string()
        } else {
            format!("/{}", file_stem)
        };

        let output_path = parsed_config
            .output_directory
            .join(PathBuf::from(&file_stem).with_extension("html"));

        log::debug!(
            "Discovered static page: {} -> {}",
            url_path,
            output_path.display()
        );

        pages.push(PageEntry {
            url_path,
            source_path: entry_path,
            output_path,
            page_type: PageType::Static,
            sitemap_meta: SitemapMeta::default(),
            frontmatter: None,
            content: None,
        });
    }

    Ok(pages)
}

pub fn discover_collections(
    parsed_config: &config::ResolvedConfig,
) -> std::io::Result<Vec<PageEntry>> {
    let mut pages = Vec::new();

    let dir_exists = fs::exists(&parsed_config.content_directory)?;
    if !dir_exists {
        log::debug!("Content directory does not exist, skipping discovery");
        return Ok(pages);
    }

    for entry in fs::read_dir(&parsed_config.content_directory)? {
        let dir = entry?;

        if dir.metadata()?.is_file() {
            log::warn!(
                "Entry {} is a file; this is not allowed in content directory, skipping",
                dir.file_name().to_string_lossy()
            );
            continue;
        }

        let collection_name = dir.file_name().to_string_lossy().to_string();
        log::debug!("Discovering collection: {}", collection_name);

        let details_page_path = parsed_config
            .pages_directory
            .join(&collection_name)
            .join("details.hbs");
        let has_details_page = fs::exists(&details_page_path)?;

        if !has_details_page {
            log::warn!(
                "Collection {} has no details page, skipping",
                collection_name
            );
            continue;
        }

        let content_dir_path = parsed_config.content_directory.join(&collection_name);

        for content_entry in fs::read_dir(&content_dir_path)? {
            let content_file = content_entry?;
            let content_file_path = content_file.path();

            let extension = content_file_path.extension();
            if extension.map(|e| e.to_string_lossy()) != Some("md".into()) {
                log::debug!(
                    "Skipping non-markdown file {} during discovery",
                    content_file.file_name().to_string_lossy()
                );
                continue;
            }

            let file_stem = content_file_path
                .file_stem()
                .expect("Could not get collection entry file stem")
                .to_string_lossy()
                .to_string();

            let file_content = fs::read_to_string(&content_file_path)?;
            let parsed_content = collection::parse_markdown(&file_content)?;

            let sitemap_meta = SitemapMeta::from_frontmatter(&parsed_content.fm);

            let url_path = format!("/{}/{}", collection_name, file_stem);

            let output_path = parsed_config
                .output_directory
                .join(&collection_name)
                .join(&file_stem)
                .with_extension("html");

            log::debug!(
                "Discovered collection item: {} -> {}",
                url_path,
                output_path.display()
            );

            pages.push(PageEntry {
                url_path,
                source_path: content_file_path,
                output_path,
                page_type: PageType::Collection {
                    name: collection_name.clone(),
                },
                sitemap_meta,
                frontmatter: Some(parsed_content.fm),
                content: Some(parsed_content.content),
            });
        }
    }

    Ok(pages)
}

pub fn render_pages(
    parsed_config: &config::ResolvedConfig,
    pages: &[PageEntry],
    render: &HandlebarsRenderer,
) -> std::io::Result<()> {
    for page in pages {
        if !matches!(page.page_type, PageType::Static) {
            continue;
        }

        log::info!(
            "Rendering page {}",
            page.source_path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string())
        );

        let content = fs::read_to_string(&page.source_path)?;
        let rendered = render.render(content, serde_json::json!(&parsed_config.global));
        fs::write(&page.output_path, rendered)?;
    }

    Ok(())
}

pub fn render_collection_items(
    parsed_config: &config::ResolvedConfig,
    pages: &[PageEntry],
    render: &HandlebarsRenderer,
) -> std::io::Result<()> {
    let mut created_dirs: std::collections::HashSet<String> = std::collections::HashSet::new();

    for page in pages {
        let collection_name = match &page.page_type {
            PageType::Collection { name } => name,
            _ => continue,
        };

        if !created_dirs.contains(collection_name) {
            let collection_output_dir = parsed_config.output_directory.join(collection_name);
            if !fs::exists(&collection_output_dir)? {
                fs::create_dir(&collection_output_dir)?;
            }
            created_dirs.insert(collection_name.clone());
            log::info!("Rendering collection {}", collection_name);
        }

        let details_page_path = parsed_config
            .pages_directory
            .join(collection_name)
            .join("details.hbs");

        let content = page
            .content
            .as_ref()
            .expect("Collection item should have parsed content");
        let frontmatter = page
            .frontmatter
            .as_ref()
            .expect("Collection item should have frontmatter");

        let rendered_result = render.render(
            fs::read_to_string(&details_page_path)?,
            merge_contexts(
                parsed_config,
                serde_json::json!({"content": content, "fm": frontmatter}),
            ),
        );

        fs::write(&page.output_path, &rendered_result)?;
    }

    Ok(())
}

pub fn write_sitemap(
    parsed_config: &config::ResolvedConfig,
    site_pages: &SitePages,
) -> std::io::Result<()> {
    let base_url = match &parsed_config.base_url {
        Some(url) => url,
        None => {
            log::debug!("No base_url configured, skipping sitemap generation");
            return Ok(());
        }
    };

    let sitemap_config = parsed_config.sitemap.clone().unwrap_or_default();

    if !sitemap_config.enabled {
        log::debug!("Sitemap generation is disabled");
        return Ok(());
    }

    let xml = sitemap::generate_sitemap(site_pages, base_url, &sitemap_config);
    let sitemap_path = parsed_config
        .output_directory
        .join(&sitemap_config.filename);

    log::info!("Writing sitemap to {}", sitemap_path.display());
    fs::write(&sitemap_path, xml)?;

    Ok(())
}
