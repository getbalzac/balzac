pub mod cli;
pub mod collection;
pub mod config;
pub mod context;
pub mod dev;
pub mod hooks;
pub mod listing;
pub mod markdown;
pub mod renderer;
pub mod robots;
pub mod sitemap;
pub mod vite;

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    context::{build_collections_context, merge_contexts},
    renderer::{HandlebarsRenderer, Renderer},
    sitemap::{PageEntry, PageType, SitePages, SitemapMeta},
};

/// Output file name of the not-found page, which static hosts serve for missing paths.
pub const NOT_FOUND_PAGE: &str = "404.html";
/// Name of the robots file, both in the output root and as a user override in the pages directory.
pub const ROBOTS_FILE: &str = "robots.txt";

pub fn make_dist_folder(parsed_config: &config::ResolvedConfig) -> std::io::Result<()> {
    let dir_exists = fs::exists(&parsed_config.output_directory)?;
    if dir_exists {
        log::debug!(
            "Output directory {} already exists, recreating",
            parsed_config.output_directory.display()
        );
        fs::remove_dir_all(&parsed_config.output_directory)?;
        fs::create_dir(&parsed_config.output_directory)?;
    } else {
        log::debug!(
            "Creating output directory {}",
            parsed_config.output_directory.display()
        );
        fs::create_dir(&parsed_config.output_directory)?;
    }
    Ok(())
}

pub fn add_assets(parsed_config: &config::ResolvedConfig) -> std::io::Result<()> {
    let dir_exists = fs::exists(&parsed_config.assets_directory)?;

    if dir_exists {
        // The output directory may live inside the assets directory; never copy it into itself.
        // Create it first so it can always be canonicalized and matched during the walk.
        let destination = parsed_config.output_directory.join("assets");
        fs::create_dir_all(&destination)?;
        let output_directory = fs::canonicalize(&parsed_config.output_directory)?;
        copy_dir_recursive(
            &parsed_config.assets_directory,
            &destination,
            &output_directory,
        )?;
    } else {
        log::debug!("Assets directory does not exist, skipping");
    }

    Ok(())
}

fn copy_dir_recursive(source: &Path, destination: &Path, excluded: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());

        if entry.file_type()?.is_dir() {
            if fs::canonicalize(entry.path())? == excluded {
                log::debug!("Skipping output directory {}", entry.path().display());
                continue;
            }
            copy_dir_recursive(&entry.path(), &target, excluded)?;
            continue;
        }

        // `is_file` follows symlinks: links to regular files are copied, while directory and
        // dangling links, FIFOs, sockets and devices are skipped so they can't fail or hang the copy.
        if !entry.path().is_file() {
            log::warn!(
                "Skipping asset {} (not a regular file)",
                entry.path().display()
            );
            continue;
        }

        log::debug!("Copying {} to {}", entry.path().display(), target.display());
        fs::copy(entry.path(), target)?;
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
        let is_page_file = matches!(extension.as_deref(), Some("hbs" | "handlebars" | "html"));
        if !is_page_file {
            log::debug!(
                "Skipping non-page file {} during discovery",
                dir.file_name().to_string_lossy()
            );
            continue;
        }

        let Some(page) = static_page_entry_from_source(parsed_config, &entry_path) else {
            continue;
        };

        log::debug!(
            "Discovered static page: {} -> {}",
            page.url_path,
            page.output_path.display()
        );

        pages.push(page);
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
        log::debug!("Discovering collection: {collection_name}");

        let details_page_path = parsed_config
            .pages_directory
            .join(&collection_name)
            .join("details.hbs");
        let has_details_page = fs::exists(&details_page_path)?;

        if !has_details_page {
            log::warn!("Collection {collection_name} has no details page, skipping");
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

            let Some(page) = collection_entry_from_source(parsed_config, &content_file_path)?
            else {
                continue;
            };

            log::debug!(
                "Discovered collection item: {} -> {}",
                page.url_path,
                page.output_path.display()
            );

            pages.push(page);
        }
    }

    Ok(pages)
}

/// Discovers every page of the site: static pages, collection items and the
/// generated listing pages.
pub fn discover_site_pages(parsed_config: &config::ResolvedConfig) -> std::io::Result<SitePages> {
    let static_pages = discover_static_pages(parsed_config)?;
    let collection_pages = discover_collections(parsed_config)?;
    let listing_pages = listing::discover_listings(parsed_config, &collection_pages)?;

    let mut site_pages = SitePages::new();
    site_pages.add_pages(static_pages);
    site_pages.add_pages(collection_pages);
    site_pages.add_pages(listing_pages);
    Ok(site_pages)
}

pub fn render_pages(
    parsed_config: &config::ResolvedConfig,
    pages: &[PageEntry],
    render: &HandlebarsRenderer,
) -> std::io::Result<()> {
    let collections = build_collections_context(pages, render)?;

    for page in pages {
        if !matches!(page.page_type, PageType::Static) {
            continue;
        }

        render_static_page(parsed_config, page, render, &collections)?;
    }

    Ok(())
}

pub fn render_collection_items(
    parsed_config: &config::ResolvedConfig,
    pages: &[PageEntry],
    render: &HandlebarsRenderer,
) -> std::io::Result<()> {
    let collections = build_collections_context(pages, render)?;
    let mut created_dirs: std::collections::HashSet<String> = std::collections::HashSet::new();

    for page in pages {
        let PageType::Collection {
            name: collection_name,
        } = &page.page_type
        else {
            continue;
        };

        if !created_dirs.contains(collection_name) {
            let collection_output_dir = parsed_config.output_directory.join(collection_name);
            if !fs::exists(&collection_output_dir)? {
                fs::create_dir(&collection_output_dir)?;
            }
            created_dirs.insert(collection_name.clone());
            log::info!("Rendering collection {collection_name}");
        }

        render_collection_page(parsed_config, page, render, &collections)?;
    }

    Ok(())
}

pub fn write_sitemap(
    parsed_config: &config::ResolvedConfig,
    site_pages: &SitePages,
) -> std::io::Result<()> {
    let Some(base_url) = &parsed_config.base_url else {
        log::debug!("No base_url configured, skipping sitemap generation");
        return Ok(());
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

/// Absolute URL of the generated sitemap, if one is generated.
fn sitemap_url(parsed_config: &config::ResolvedConfig) -> Option<String> {
    let base_url = parsed_config.base_url.as_ref()?;
    let sitemap_config = parsed_config.sitemap.clone().unwrap_or_default();
    sitemap_config.enabled.then(|| {
        format!(
            "{}/{}",
            base_url.trim_end_matches('/'),
            sitemap_config.filename.trim_start_matches('/')
        )
    })
}

/// Write `robots.txt` to the output root. A user-provided `robots.txt` in the
/// pages directory is copied verbatim and always takes precedence.
pub fn write_robots(parsed_config: &config::ResolvedConfig) -> std::io::Result<()> {
    let robots_path = parsed_config.output_directory.join(ROBOTS_FILE);

    let user_robots = parsed_config.pages_directory.join(ROBOTS_FILE);
    if fs::exists(&user_robots)? {
        log::info!(
            "Copying {} to {}",
            user_robots.display(),
            robots_path.display()
        );
        fs::copy(&user_robots, &robots_path)?;
        return Ok(());
    }

    if fs::exists(parsed_config.assets_directory.join(ROBOTS_FILE))? {
        log::warn!(
            "{ROBOTS_FILE} in the assets directory is served under /assets/ and ignored by crawlers; move it to the pages directory to replace the generated one"
        );
    }

    let robots_config = parsed_config.robots.clone().unwrap_or_default();
    if !robots_config.enabled {
        log::debug!("robots.txt generation is disabled");
        return Ok(());
    }

    let content = robots::generate_robots(&robots_config, sitemap_url(parsed_config).as_deref());
    log::info!("Writing robots.txt to {}", robots_path.display());
    fs::write(&robots_path, content)
}

#[must_use]
pub fn is_supported_page_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("hbs" | "handlebars" | "html")
    )
}

#[must_use]
pub fn static_page_entry_from_source(
    parsed_config: &config::ResolvedConfig,
    source_path: &Path,
) -> Option<PageEntry> {
    let relative_path = source_path
        .strip_prefix(&parsed_config.pages_directory)
        .ok()?;
    if relative_path.components().count() != 1 || !is_supported_page_file(source_path) {
        return None;
    }

    let file_stem = source_path.file_stem()?.to_string_lossy().to_string();
    let url_path = if file_stem == "index" {
        "/".to_string()
    } else {
        format!("/{file_stem}")
    };
    let output_path = parsed_config
        .output_directory
        .join(PathBuf::from(&file_stem).with_extension("html"));
    let sitemap_meta = SitemapMeta {
        // the not-found page is not real content
        exclude: file_stem == "404",
        ..SitemapMeta::default()
    };

    Some(PageEntry {
        url_path,
        source_path: source_path.to_path_buf(),
        output_path,
        page_type: PageType::Static,
        sitemap_meta,
        frontmatter: None,
        content: None,
    })
}

pub fn collection_entry_from_source(
    parsed_config: &config::ResolvedConfig,
    source_path: &Path,
) -> std::io::Result<Option<PageEntry>> {
    let Ok(relative_path) = source_path.strip_prefix(&parsed_config.content_directory) else {
        return Ok(None);
    };
    let mut components = relative_path.components();
    let Some(collection_name) = components
        .next()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
    else {
        return Ok(None);
    };
    let Some(file_name) = components.next() else {
        return Ok(None);
    };
    if components.next().is_some()
        || source_path.extension().and_then(|ext| ext.to_str()) != Some("md")
    {
        return Ok(None);
    }

    let details_page_path = parsed_config
        .pages_directory
        .join(&collection_name)
        .join("details.hbs");
    if !fs::exists(&details_page_path)? {
        return Ok(None);
    }

    let file_stem = Path::new(file_name.as_os_str())
        .file_stem()
        .expect("Could not get collection entry file stem")
        .to_string_lossy()
        .to_string();
    let file_content = fs::read_to_string(source_path)?;
    let parsed_content = collection::parse_markdown(&file_content)?;
    let sitemap_meta = SitemapMeta::from_frontmatter(&parsed_content.fm);
    let url_path = format!("/{collection_name}/{file_stem}");
    let output_path = parsed_config
        .output_directory
        .join(&collection_name)
        .join(&file_stem)
        .with_extension("html");

    Ok(Some(PageEntry {
        url_path,
        source_path: source_path.to_path_buf(),
        output_path,
        page_type: PageType::Collection {
            name: collection_name,
        },
        sitemap_meta,
        frontmatter: Some(parsed_content.fm),
        content: Some(parsed_content.content),
    }))
}

pub fn render_static_page(
    parsed_config: &config::ResolvedConfig,
    page: &PageEntry,
    render: &HandlebarsRenderer,
    collections: &serde_json::Value,
) -> std::io::Result<()> {
    log::info!(
        "Rendering page {}",
        page.source_path.file_name().map_or_else(
            || "unknown".to_string(),
            |f| f.to_string_lossy().to_string()
        )
    );

    let content = fs::read_to_string(&page.source_path)?;
    let rendered = render.render(
        content,
        merge_contexts(
            parsed_config,
            &serde_json::json!({"collections": collections}),
        ),
    );
    fs::write(&page.output_path, rendered)?;
    Ok(())
}

pub fn render_collection_page(
    parsed_config: &config::ResolvedConfig,
    page: &PageEntry,
    render: &HandlebarsRenderer,
    collections: &serde_json::Value,
) -> std::io::Result<()> {
    let collection_name = match &page.page_type {
        PageType::Collection { name } => name,
        PageType::Static | PageType::Listing(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "render_collection_page expects a collection entry",
            ));
        }
    };

    let collection_output_dir = parsed_config.output_directory.join(collection_name);
    if !fs::exists(&collection_output_dir)? {
        fs::create_dir_all(&collection_output_dir)?;
    }

    let details_page_path = parsed_config
        .pages_directory
        .join(collection_name)
        .join("details.hbs");
    let raw_content = page
        .content
        .as_ref()
        .expect("Collection item should have parsed content");
    let frontmatter = page
        .frontmatter
        .as_ref()
        .expect("Collection item should have frontmatter");

    let document = render
        .render_document(raw_content)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let rendered_result = render.render(
        fs::read_to_string(&details_page_path)?,
        merge_contexts(
            parsed_config,
            &serde_json::json!({
                "content": document.html,
                "fm": frontmatter,
                "toc": document.toc,
                "toc_html": document.toc_html,
                "excerpt": document.excerpt,
                "collections": collections,
            }),
        ),
    );

    fs::write(&page.output_path, &rendered_result)?;
    Ok(())
}

pub fn remove_output_for_source(
    parsed_config: &config::ResolvedConfig,
    source_path: &Path,
) -> std::io::Result<Option<PathBuf>> {
    let output_path = if let Some(page) = static_page_entry_from_source(parsed_config, source_path)
    {
        Some(page.output_path)
    } else {
        collection_output_path_from_source(parsed_config, source_path)
    };

    let Some(output_path) = output_path else {
        return Ok(None);
    };

    if fs::exists(&output_path)? {
        fs::remove_file(&output_path)?;
    }

    Ok(Some(output_path))
}

#[must_use]
pub fn collection_output_path_from_source(
    parsed_config: &config::ResolvedConfig,
    source_path: &Path,
) -> Option<PathBuf> {
    let relative_path = source_path
        .strip_prefix(&parsed_config.content_directory)
        .ok()?;
    let mut components = relative_path.components();
    let collection_name = components.next()?.as_os_str().to_string_lossy().to_string();
    let file_name = components.next()?;
    if components.next().is_some()
        || source_path.extension().and_then(|ext| ext.to_str()) != Some("md")
    {
        return None;
    }

    let file_stem = Path::new(file_name.as_os_str())
        .file_stem()?
        .to_string_lossy();
    Some(
        parsed_config
            .output_directory
            .join(collection_name)
            .join(file_stem.as_ref())
            .with_extension("html"),
    )
}
