pub mod cli;
pub mod collection;
pub mod config;
pub mod context;
pub mod dev;
pub mod directive;
pub mod error;
pub mod feed;
pub mod fingerprint;
pub mod helpers;
pub mod hooks;
pub mod listing;
pub mod markdown;
pub mod parallel;
pub mod renderer;
pub mod robots;
#[cfg(feature = "script-helpers")]
pub mod script_helpers;
pub mod sitemap;
pub mod url;
pub mod vite;

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use crate::{
    context::SiteContext,
    error::{Error, Result},
    renderer::{HandlebarsRenderer, Renderer},
    sitemap::{PageEntry, PageType, SitePages, SitemapMeta},
    url::{PageLocation, page_location},
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

/// Whether `output_path` is where `add_assets` copies a file from the assets
/// directory, e.g. `dist/assets/index.html` for `assets/index.html`. Fingerprinted
/// builds publish `index.<hash>.html` instead, but `balzac dev` copies assets under
/// their own names, so the plain name is kept for a project to work in both.
#[must_use]
pub fn is_asset_output(parsed_config: &config::ResolvedConfig, output_path: &Path) -> bool {
    let Ok(relative) = output_path.strip_prefix(parsed_config.output_directory.join("assets"))
    else {
        return false;
    };
    parsed_config.assets_directory.join(relative).is_file()
}

pub fn add_assets(parsed_config: &config::ResolvedConfig) -> std::io::Result<()> {
    let dir_exists = fs::exists(&parsed_config.assets_directory)?;
    // Dev serves the assets under their own names, which the `asset` helper links to there.
    let fingerprints = (parsed_config.assets.fingerprint && !parsed_config.dev_mode)
        .then(|| fingerprint::Fingerprints::new(&parsed_config.assets_directory));

    if dir_exists {
        // The output directory may live inside the assets directory; never copy it into itself.
        // Create it first so it can always be canonicalized and matched during the walk.
        // A directory-style `pages/assets.hbs` may already have created it.
        let destination = parsed_config.output_directory.join("assets");
        fs::create_dir_all(&destination)?;
        let output_directory = fs::canonicalize(&parsed_config.output_directory)?;
        copy_dir_recursive(
            &parsed_config.assets_directory,
            &destination,
            &output_directory,
            fingerprints.as_ref(),
        )?;
    } else {
        log::debug!("Assets directory does not exist, skipping");
    }

    if parsed_config.assets.manifest && !parsed_config.dev_mode {
        match &fingerprints {
            Some(fingerprints) => write_asset_manifest(parsed_config, fingerprints)?,
            None => log::warn!(
                "[assets] manifest needs fingerprint = true, no asset manifest is written"
            ),
        }
    }

    Ok(())
}

/// Writes `<output>/assets/manifest.json`: source path -> fingerprinted path,
/// both relative to the assets directory.
fn write_asset_manifest(
    parsed_config: &config::ResolvedConfig,
    fingerprints: &fingerprint::Fingerprints,
) -> std::io::Result<()> {
    let destination = parsed_config.output_directory.join("assets");
    fs::create_dir_all(&destination)?;
    let path = destination.join(fingerprint::MANIFEST_FILE);
    if fs::exists(&path)? {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "the asset manifest would overwrite the rendered page at {}",
                path.display()
            ),
        ));
    }
    let manifest = serde_json::to_string_pretty(&fingerprints.manifest())?;
    log::info!("Writing asset manifest to {}", path.display());
    fs::write(path, manifest)
}

fn copy_dir_recursive(
    source: &Path,
    destination: &Path,
    excluded: &Path,
    fingerprints: Option<&fingerprint::Fingerprints>,
) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let mut target = destination.join(entry.file_name());

        if entry.file_type()?.is_dir() {
            if fs::canonicalize(entry.path())? == excluded {
                log::debug!("Skipping output directory {}", entry.path().display());
                continue;
            }
            copy_dir_recursive(&entry.path(), &target, excluded, fingerprints)?;
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

        let mut rewritten = None;
        if let Some(fingerprints) = fingerprints
            && let Some(asset) = fingerprints.get_file(&entry.path())?
        {
            if let Some(name) = asset.path.rsplit('/').next() {
                target = destination.join(name);
            }
            rewritten = asset.rewritten;
        }

        // Only a rendered page can be here already (e.g. a directory-style
        // `pages/assets.hbs`), and silently replacing it would lose the page.
        if fs::exists(&target)? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "asset {} would overwrite the rendered page at {}",
                    entry.path().display(),
                    target.display()
                ),
            ));
        }
        log::debug!("Copying {} to {}", entry.path().display(), target.display());
        match rewritten {
            Some(contents) => fs::write(target, contents)?,
            None => {
                fs::copy(entry.path(), target)?;
            }
        }
    }
    Ok(())
}

pub fn discover_static_pages(parsed_config: &config::ResolvedConfig) -> Result<Vec<PageEntry>> {
    let mut pages = Vec::new();

    let dir_exists = fs::exists(&parsed_config.pages_directory)?;
    if !dir_exists {
        log::debug!("Pages directory does not exist, skipping discovery");
        return Ok(pages);
    }

    discover_static_pages_in(parsed_config, &parsed_config.pages_directory, &mut pages)?;

    Ok(pages)
}

fn discover_static_pages_in(
    parsed_config: &config::ResolvedConfig,
    directory: &Path,
    pages: &mut Vec<PageEntry>,
) -> Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);

    for dir in entries {
        let entry_path = dir.path();

        if dir.metadata()?.is_dir() {
            log::debug!("Descending into directory {}", entry_path.display());
            discover_static_pages_in(parsed_config, &entry_path, pages)?;
            continue;
        }

        if !is_supported_page_file(&entry_path) {
            log::debug!(
                "Skipping non-page file {} during discovery",
                entry_path.display()
            );
            continue;
        }

        if is_page_template(parsed_config, &entry_path) {
            log::debug!(
                "Skipping page template {} during discovery",
                entry_path.display()
            );
            continue;
        }

        let Some(page) = load_static_page_entry(parsed_config, &entry_path)? else {
            continue;
        };

        if let Some(existing) = pages.iter().find(|p| p.output_path == page.output_path) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Pages {} and {} would both write {}",
                    existing.source_path.display(),
                    page.source_path.display(),
                    page.output_path.display()
                ),
            )
            .into());
        }
        log::debug!(
            "Discovered static page: {} -> {}",
            page.url_path,
            page.output_path.display()
        );

        pages.push(page);
    }

    Ok(())
}

pub fn discover_collections(parsed_config: &config::ResolvedConfig) -> Result<Vec<PageEntry>> {
    let mut pages = Vec::new();
    // Lowercased output path -> item rendering it, to report duplicates. Case is
    // ignored because macOS and Windows file systems store `Jan.html` and
    // `jan.html` as one file, so the second item would overwrite the first.
    let mut items_by_output: std::collections::HashMap<String, (PathBuf, String)> =
        std::collections::HashMap::new();

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

        let template_path = parsed_config.collection_template_path(&collection_name);
        if !fs::exists(&template_path)? {
            log::warn!(
                "Collection {} has no template at {}, skipping",
                collection_name,
                template_path.display()
            );
            continue;
        }

        let content_dir_path = parsed_config.content_directory.join(&collection_name);
        let mut source_paths = Vec::new();
        collect_markdown_files(&content_dir_path, &mut source_paths)?;
        source_paths.sort();

        for content_file_path in source_paths {
            let Some(page) = collection_entry_from_source(parsed_config, &content_file_path)?
            else {
                continue;
            };

            if let Some((existing_source, existing_url)) = items_by_output.insert(
                page.output_path.to_string_lossy().to_lowercase(),
                (page.source_path.clone(), page.url_path.clone()),
            ) {
                let message = if existing_url == page.url_path {
                    format!(
                        "{} and {} both render to {}",
                        existing_source.display(),
                        page.source_path.display(),
                        page.url_path
                    )
                } else {
                    format!(
                        "{} ({existing_url}) and {} ({}) differ only in case, so they \
                         render to the same file on case-insensitive file systems",
                        existing_source.display(),
                        page.source_path.display(),
                        page.url_path
                    )
                };
                return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, message).into());
            }

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
/// generated listing pages. Fails before anything is rendered if two pages, or
/// a page and a copied asset, would be written to the same file.
pub fn discover_site_pages(parsed_config: &config::ResolvedConfig) -> Result<SitePages> {
    let mut pages = discover_static_pages(parsed_config)?;
    pages.extend(discover_collections(parsed_config)?);
    let listing_pages = listing::discover_listings(parsed_config, &pages)?;

    // `add_assets` catches this too, but dev re-renders without copying assets again.
    if let Some(page) = pages
        .iter()
        .chain(&listing_pages)
        .find(|page| is_asset_output(parsed_config, &page.output_path))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "{} would overwrite the asset copied to {}",
                page.url_path,
                page.output_path.display()
            ),
        )
        .into());
    }

    let mut site_pages = SitePages::new();
    site_pages.add_pages(pages);
    site_pages.add_pages(listing_pages);
    Ok(site_pages)
}

/// Recursively collects the markdown files under a collection directory.
fn collect_markdown_files(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        // Symlinked folders are not followed: one pointing back up the tree
        // would recurse forever.
        if file_type.is_symlink() && path.is_dir() {
            log::warn!("Skipping symlinked folder {} in collection", path.display());
        } else if file_type.is_dir() {
            collect_markdown_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(path);
        } else {
            log::debug!(
                "Skipping non-markdown file {} during discovery",
                path.display()
            );
        }
    }
    Ok(())
}

pub fn render_pages(
    parsed_config: &config::ResolvedConfig,
    site: &SiteContext,
    render: &HandlebarsRenderer,
) -> Result<()> {
    let pages: Vec<&PageEntry> = site
        .pages()
        .iter()
        .filter(|page| matches!(page.page_type, PageType::Static))
        .collect();

    parallel::create_parent_dirs(pages.iter().map(|page| page.output_path.as_path()))?;
    parallel::try_for_each(&pages, |page| {
        write_static_page(parsed_config, page, render, site)
    })
}

pub fn render_collection_items(
    parsed_config: &config::ResolvedConfig,
    site: &SiteContext,
    render: &HandlebarsRenderer,
) -> Result<()> {
    let mut seen_collections: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut items = Vec::new();

    for page in site.pages() {
        let PageType::Collection {
            name: collection_name,
        } = &page.page_type
        else {
            continue;
        };

        if seen_collections.insert(collection_name) {
            log::info!("Rendering collection {collection_name}");
        }
        items.push(page);
    }

    parallel::create_parent_dirs(items.iter().map(|page| page.output_path.as_path()))?;
    parallel::try_for_each(&items, |page| {
        write_collection_page(parsed_config, page, render, site)
    })
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

/// Writes the configured feeds and returns the paths of the files written
pub fn write_feeds(
    parsed_config: &config::ResolvedConfig,
    site_pages: &SitePages,
    render: &HandlebarsRenderer,
) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    let Some(feed_config) = &parsed_config.feed else {
        return Ok(written);
    };

    if !feed_config.enabled {
        log::debug!("Feed generation is disabled");
        return Ok(written);
    }

    let Some(base_url) = &parsed_config.base_url else {
        log::warn!("Feeds are configured but no base_url is set, skipping feed generation");
        return Ok(written);
    };
    let site_url = base_url.trim_end_matches('/');

    let collections = feed::feed_collections(site_pages, feed_config);
    let combined = feed_config
        .combined
        .as_ref()
        .and_then(config::CombinedFeedSetting::resolve);
    let combined_collections = combined.as_ref().map(|c| match &c.collections {
        Some(names) => names.clone(),
        None => collections.clone(),
    });

    // Select once across every collection that appears in any feed
    let mut selected = collections.clone();
    selected.extend(combined_collections.iter().flatten().cloned());
    selected.sort();
    selected.dedup();
    let entries = feed::select_feed_entries(site_pages, &selected);

    let limit = effective_feed_limit(feed_config.limit);
    for collection in &collections {
        let collection_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.in_collection(collection))
            .take(limit)
            .collect();
        if collection_entries.is_empty() {
            log::warn!("Collection {collection} has no dated entries, skipping its feed");
            continue;
        }

        let overrides = feed_config.overrides.get(collection);
        let title = overrides
            .and_then(|o| o.title.as_deref())
            .or(feed_config.title.as_deref())
            .unwrap_or(collection);
        let description = overrides
            .and_then(|o| o.description.as_deref())
            .or(feed_config.description.as_deref());

        written.extend(write_feed_files(
            parsed_config,
            render,
            &collection_entries,
            Some(collection),
            title,
            description,
        )?);
    }

    if let (Some(combined), Some(combined_collections)) = (combined, combined_collections) {
        let limit = effective_feed_limit(combined.limit.unwrap_or(feed_config.limit));
        let combined_entries: Vec<_> = entries
            .iter()
            .filter(|e| combined_collections.iter().any(|c| e.in_collection(c)))
            .take(limit)
            .collect();
        if combined_entries.is_empty() {
            log::warn!("Combined feed has no dated entries, skipping it");
        } else {
            written.extend(write_feed_files(
                parsed_config,
                render,
                &combined_entries,
                None,
                combined
                    .title
                    .as_deref()
                    .or(feed_config.title.as_deref())
                    .unwrap_or(site_url),
                combined
                    .description
                    .as_deref()
                    .or(feed_config.description.as_deref()),
            )?);
        }
    }

    Ok(written)
}

/// A configured item limit, where 0 means unlimited
fn effective_feed_limit(limit: usize) -> usize {
    if limit == 0 { usize::MAX } else { limit }
}

/// Writes every configured feed kind for `entries`, inside the collection's
/// output directory or at the output root when `collection` is `None`
fn write_feed_files(
    parsed_config: &config::ResolvedConfig,
    render: &HandlebarsRenderer,
    entries: &[&feed::FeedEntry],
    collection: Option<&str>,
    title: &str,
    description: Option<&str>,
) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    let (Some(feed_config), Some(base_url)) = (&parsed_config.feed, &parsed_config.base_url) else {
        return Ok(written);
    };
    let site_url = base_url.trim_end_matches('/');

    let items = entries
        .iter()
        .map(|&entry| {
            let content_html = if feed_config.full_content {
                let body = entry.page.content.as_deref().unwrap_or_default();
                Some(
                    render
                        .render_markdown(body)
                        .map_err(|e| e.in_page(&entry.page.source_path))?,
                )
            } else {
                None
            };
            Ok(feed::FeedItem {
                entry: entry.clone(),
                content_html,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let (output_dir, url_dir) = match collection {
        Some(collection) => (
            parsed_config.output_directory.join(collection),
            format!("{site_url}/{collection}"),
        ),
        None => (parsed_config.output_directory.clone(), site_url.to_string()),
    };
    fs::create_dir_all(&output_dir)?;

    for kind in feed::FeedKind::for_format(feed_config.format) {
        let feed_url = format!("{url_dir}/{}", kind.filename());
        let channel = feed::FeedChannel {
            title,
            description,
            author: feed_config.author.as_deref(),
            site_url,
            feed_url: &feed_url,
        };
        let path = output_dir.join(kind.filename());
        log::info!("Writing feed to {}", path.display());
        fs::write(&path, kind.generate(&channel, &items))?;
        written.push(path);
    }

    Ok(written)
}

#[must_use]
pub fn is_supported_page_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("hbs" | "handlebars" | "html")
    )
}

/// Returns true when `source_path` is a template rendered by Balzac for other
/// content rather than a page of its own, i.e. `pages/<name>/<template>` where:
///
/// - the collection template (`details.hbs` unless `[collections.<name>]` sets
///   `template`) has a matching `content/<name>` directory (collection items)
/// - `list.hbs` has a `[listings.<name>]` entry (collection listing)
/// - `term.hbs` or `taxonomy.hbs` has a `[taxonomies.<name>]` entry
///
/// Without the matching content or config these files are regular pages.
#[must_use]
pub fn is_page_template(parsed_config: &config::ResolvedConfig, source_path: &Path) -> bool {
    let Ok(relative_path) = source_path.strip_prefix(&parsed_config.pages_directory) else {
        return false;
    };
    let mut components = relative_path.components();
    let Some(section) = components.next() else {
        return false;
    };
    let section = section.as_os_str().to_string_lossy();
    if is_collection_template(parsed_config, source_path) {
        return parsed_config
            .content_directory
            .join(section.as_ref())
            .is_dir();
    }
    let (Some(file_name), None) = (components.next(), components.next()) else {
        return false;
    };

    match file_name.as_os_str().to_str() {
        Some(listing::LISTING_TEMPLATE) => parsed_config.listings.contains_key(section.as_ref()),
        Some(listing::TERM_TEMPLATE | listing::TAXONOMY_TEMPLATE) => {
            parsed_config.taxonomies.contains_key(section.as_ref())
        }
        _ => false,
    }
}

/// Returns another page file that renders to the same output as `source_path`:
/// the same name with a different page extension (`intro.hbs` and
/// `intro.html`), or a folder index and the page named after the folder
/// (`docs/index.hbs` and `docs.hbs`).
#[must_use]
pub fn conflicting_static_page(
    parsed_config: &config::ResolvedConfig,
    source_path: &Path,
) -> Option<PathBuf> {
    let output_path = static_page_entry_from_source(parsed_config, source_path)?.output_path;
    // `docs/index.hbs` -> `docs`, appended to below rather than replacing an
    // extension so dotted folders (`v1.2/index.hbs` -> `v1.2.hbs`) keep their name
    let folder_page = source_path
        .parent()
        .filter(|_| source_path.file_stem().is_some_and(|stem| stem == "index"))
        .map(Path::as_os_str);
    let folder_index = source_path.with_extension("").join("index");

    ["hbs", "handlebars", "html"]
        .iter()
        .flat_map(|ext| {
            [
                Some(source_path.with_extension(ext)),
                folder_page.map(|folder| {
                    let mut page = folder.to_os_string();
                    page.push(format!(".{ext}"));
                    PathBuf::from(page)
                }),
                Some(folder_index.with_extension(ext)),
            ]
        })
        .flatten()
        // a candidate's route can differ from this page's (`docs/index/index.hbs`
        // is `/docs/index`, `docs/index.hbs` is `/docs`), so compare outputs
        .find(|candidate| {
            candidate != source_path
                && candidate.is_file()
                && static_page_entry_from_source(parsed_config, candidate)
                    .is_some_and(|page| page.output_path == output_path)
        })
}

#[must_use]
pub fn static_page_entry_from_source(
    parsed_config: &config::ResolvedConfig,
    source_path: &Path,
) -> Option<PageEntry> {
    let relative_path = source_path
        .strip_prefix(&parsed_config.pages_directory)
        .ok()?;
    if !is_supported_page_file(source_path) || is_page_template(parsed_config, source_path) {
        return None;
    }

    let route = static_page_route(relative_path)?;
    let route: Vec<&str> = route.iter().map(String::as_str).collect();
    let PageLocation {
        url_path,
        output_path,
    } = page_location(parsed_config, &route)?;
    let sitemap_meta = SitemapMeta {
        // the top-level not-found page is not real content
        exclude: route == ["404"],
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

/// Reads a static page template, splitting off its optional YAML frontmatter.
fn read_static_page_template(
    source_path: &Path,
) -> Result<(Option<serde_json::Value>, String, usize)> {
    let file_content = error::read_to_string(source_path)?;
    let (frontmatter, template) =
        collection::parse_template_frontmatter(&file_content).map_err(|e| Error::Frontmatter {
            path: source_path.to_path_buf(),
            message: e.to_string(),
        })?;
    // `template` is the end of `file_content`; count the lines before it so
    // template errors point at lines of the file
    let frontmatter_lines = file_content[..file_content.len() - template.len()]
        .matches('\n')
        .count();
    Ok((frontmatter, template.to_string(), frontmatter_lines))
}

/// Builds a static page entry, parsing its frontmatter for sitemap metadata.
///
/// The template itself is not cached: `render_static_page` re-reads the source
/// so edits made after discovery (e.g. by a `render_before` hook) are rendered.
pub fn load_static_page_entry(
    parsed_config: &config::ResolvedConfig,
    source_path: &Path,
) -> Result<Option<PageEntry>> {
    let Some(mut page) = static_page_entry_from_source(parsed_config, source_path) else {
        return Ok(None);
    };

    let (frontmatter, _, _) = read_static_page_template(source_path)?;
    if let Some(fm) = &frontmatter {
        // keep exclusions decided from the path (e.g. the not-found page)
        let excluded_by_path = page.sitemap_meta.exclude;
        page.sitemap_meta = SitemapMeta::from_frontmatter(fm);
        page.sitemap_meta.exclude |= excluded_by_path;
    }
    page.frontmatter = frontmatter;

    Ok(Some(page))
}

/// Location of a collection item derived from its source path alone.
struct CollectionItemLocation {
    collection_name: String,
    /// Folders between the collection directory and the file.
    sub_directories: Vec<String>,
    file_stem: String,
}

impl CollectionItemLocation {
    fn from_source(parsed_config: &config::ResolvedConfig, source_path: &Path) -> Option<Self> {
        if source_path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            return None;
        }
        let relative_path = source_path
            .strip_prefix(&parsed_config.content_directory)
            .ok()?;
        let mut segments = Vec::new();
        for component in relative_path.components() {
            match component {
                std::path::Component::Normal(segment) => {
                    segments.push(segment.to_string_lossy().to_string());
                }
                _ => return None,
            }
        }
        if segments.len() < 2 {
            return None;
        }
        segments.pop();
        let collection_name = segments.remove(0);
        let file_stem = source_path.file_stem()?.to_string_lossy().to_string();

        Some(Self {
            collection_name,
            sub_directories: segments,
            file_stem,
        })
    }

    /// URL and output file of the item when published under `slug`, following
    /// the configured `url_style`.
    fn page_location(
        &self,
        parsed_config: &config::ResolvedConfig,
        slug: &str,
    ) -> Option<PageLocation> {
        page_location(parsed_config, &self.route(slug))
    }

    /// URL segments of the item when published under `slug`.
    fn route<'a>(&'a self, slug: &'a str) -> Vec<&'a str> {
        std::iter::once(self.collection_name.as_str())
            .chain(self.sub_directories.iter().map(String::as_str))
            .chain([slug])
            .collect()
    }
}

/// Reads the `slug` frontmatter override, which replaces the file name in the URL.
fn slug_from_frontmatter(
    fm: &serde_json::Value,
    source_path: &Path,
) -> std::io::Result<Option<String>> {
    let invalid = |reason: &str| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid slug in {}: {}", source_path.display(), reason),
        )
    };

    match fm.get("slug") {
        None => Ok(None),
        // `slug:` with no value is most likely a mistake, not a request for the default
        Some(serde_json::Value::Null) => Err(invalid("must not be empty")),
        Some(serde_json::Value::String(slug)) => {
            if slug.is_empty() || slug == "." || slug == ".." {
                return Err(invalid("must not be empty, \".\" or \"..\""));
            }
            if slug.contains(['/', '\\']) {
                return Err(invalid("must be a single path segment without slashes"));
            }
            // These would change the URL's meaning (fragment, query, escapes) or need
            // encoding, so the published URL could not reach the generated file
            if slug.contains(['#', '?', '%']) || slug.chars().any(char::is_whitespace) {
                return Err(invalid("must not contain '#', '?', '%' or whitespace"));
            }
            Ok(Some(slug.clone()))
        }
        Some(_) => Err(invalid("must be a string")),
    }
}

/// Rejects URL segments that would make the published URL miss the item's
/// file: `#` and `?` turn the rest of the URL into a fragment or query, hosts
/// decode `%` escapes into a different name, browsers turn `\` into `/`, and a
/// segment ending in `.html` clashes with the file of the item at its prefix:
/// `/posts/a.html` is served by it, and a `posts/a.html/` folder cannot sit
/// next to the `posts/a.html` file. Applies to collection, folder and file names
/// as well as `slug`s; none of these characters ever produced a reachable page.
fn check_url_segments(
    location: &CollectionItemLocation,
    slug: &str,
    source_path: &Path,
) -> std::io::Result<()> {
    let invalid = |reason: String| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid URL for {}: {}", source_path.display(), reason),
        )
    };

    let segments = std::iter::once(location.collection_name.as_str())
        .chain(location.sub_directories.iter().map(String::as_str))
        .chain([slug]);
    for segment in segments {
        if segment.contains(['#', '?', '%', '\\']) {
            return Err(invalid(format!(
                "\"{segment}\" must not contain '#', '?', '%' or '\\'"
            )));
        }
    }
    if let Some(folder) = location
        .sub_directories
        .iter()
        .find(|folder| folder.to_ascii_lowercase().ends_with(".html"))
    {
        return Err(invalid(format!(
            "folder \"{folder}\" must not end with \".html\""
        )));
    }
    if slug.to_ascii_lowercase().ends_with(".html") {
        return Err(invalid(format!("\"{slug}\" must not end with \".html\"")));
    }
    Ok(())
}

fn is_draft(fm: &serde_json::Value) -> bool {
    matches!(fm.get("draft"), Some(serde_json::Value::Bool(true)))
}

/// Builds the page entry for a collection item, reading its frontmatter.
///
/// Returns `None` when the path is not a collection item, the collection has
/// no template, the item is a draft outside of dev mode, or a static page
/// renders to the same file (static pages take precedence).
pub fn collection_entry_from_source(
    parsed_config: &config::ResolvedConfig,
    source_path: &Path,
) -> Result<Option<PageEntry>> {
    let Some((page, route)) = unshadowed_collection_entry(parsed_config, source_path)? else {
        return Ok(None);
    };
    let route: Vec<&str> = route.iter().map(String::as_str).collect();
    if let Some(static_source) = static_page_for_collection_item(parsed_config, &route) {
        log::warn!(
            "Static page {} and collection item {} render to the same file; the static page wins",
            static_source.display(),
            source_path.display(),
        );
        return Ok(None);
    }
    Ok(Some(page))
}

/// Builds a collection item's page entry and route (`[collection, folders..., slug]`)
/// without checking whether a static page renders to the same file.
fn unshadowed_collection_entry(
    parsed_config: &config::ResolvedConfig,
    source_path: &Path,
) -> Result<Option<(PageEntry, Vec<String>)>> {
    let Some(item) = CollectionItemLocation::from_source(parsed_config, source_path) else {
        return Ok(None);
    };

    let template_path = parsed_config.collection_template_path(&item.collection_name);
    if !fs::exists(&template_path)? {
        return Ok(None);
    }

    let file_content = error::read_to_string(source_path)?;
    let parsed_content =
        collection::parse_markdown(&file_content).map_err(|e| Error::Frontmatter {
            path: source_path.to_path_buf(),
            message: e.to_string(),
        })?;

    let draft = is_draft(&parsed_content.fm);
    if draft && !parsed_config.dev_mode {
        log::debug!("Skipping draft {}", source_path.display());
        return Ok(None);
    }

    let slug = slug_from_frontmatter(&parsed_content.fm, source_path)?
        .unwrap_or_else(|| item.file_stem.clone());
    check_url_segments(&item, &slug, source_path)?;
    let Some(location) = item.page_location(parsed_config, &slug) else {
        return Ok(None);
    };
    let mut sitemap_meta = SitemapMeta::from_frontmatter(&parsed_content.fm);
    if draft {
        sitemap_meta.exclude = true;
    }

    let route = item.route(&slug).into_iter().map(str::to_string).collect();
    let page = PageEntry {
        url_path: location.url_path,
        source_path: source_path.to_path_buf(),
        output_path: location.output_path,
        page_type: PageType::Collection {
            name: item.collection_name,
        },
        sitemap_meta,
        frontmatter: Some(parsed_content.fm),
        content: Some(parsed_content.content),
    };
    Ok(Some((page, route)))
}

/// Whether `path` is the template of a collection (by default `<name>/details.hbs`).
#[must_use]
pub fn is_collection_template(parsed_config: &config::ResolvedConfig, path: &Path) -> bool {
    let Ok(relative_path) = path.strip_prefix(&parsed_config.pages_directory) else {
        return false;
    };
    let Some(collection_name) = relative_path.components().next() else {
        return false;
    };
    let collection_name = collection_name.as_os_str().to_string_lossy();
    parsed_config.collection_template_path(&collection_name) == path
}

/// Renders one static page, creating its output directory.
pub fn render_static_page(
    parsed_config: &config::ResolvedConfig,
    page: &PageEntry,
    render: &HandlebarsRenderer,
    site: &SiteContext,
) -> Result<()> {
    create_parent_dir(&page.output_path)?;
    write_static_page(parsed_config, page, render, site)
}

/// Renders one static page into its existing output directory.
fn write_static_page(
    parsed_config: &config::ResolvedConfig,
    page: &PageEntry,
    render: &HandlebarsRenderer,
    site: &SiteContext,
) -> Result<()> {
    log::info!(
        "Rendering page {}",
        page.source_path
            .strip_prefix(&parsed_config.pages_directory)
            .unwrap_or(&page.source_path)
            .display()
    );

    let (frontmatter, template, frontmatter_lines) = read_static_page_template(&page.source_path)?;
    let mut page_context = serde_json::Map::new();
    if let Some(fm) = frontmatter {
        page_context.insert("fm".to_string(), fm);
    }
    let rendered = site
        .with_page_context(page_context, |context| {
            render.render_with_context(&page.source_path, &template, context)
        })
        .map_err(|e| e.offset_lines(&page.source_path, frontmatter_lines))?;
    fs::write(&page.output_path, rendered)?;
    Ok(())
}

/// Renders one collection item into its existing output directory.
fn write_collection_page(
    parsed_config: &config::ResolvedConfig,
    page: &PageEntry,
    render: &HandlebarsRenderer,
    site: &SiteContext,
) -> Result<()> {
    let collection_name = match &page.page_type {
        PageType::Collection { name } => name,
        PageType::Static | PageType::Listing(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "write_collection_page expects a collection entry",
            )
            .into());
        }
    };

    let template_path = parsed_config.collection_template_path(collection_name);
    let frontmatter = page
        .frontmatter
        .as_ref()
        .expect("Collection item should have frontmatter");
    // Rendered once for the `collections` context and reused here
    let document = site
        .document(&page.source_path)
        .expect("Collection item should be rendered with its site");

    let page_context = serde_json::json!({
        "content": document.html,
        "fm": frontmatter,
        "toc": document.toc,
        "toc_html": document.toc_html,
        "excerpt": document.excerpt,
    });
    let serde_json::Value::Object(page_context) = page_context else {
        unreachable!("json! object");
    };
    let template = error::read_to_string(&template_path)?;
    let rendered_result = site
        .with_page_context(page_context, |context| {
            render.render_with_context(&template_path, &template, context)
        })
        .map_err(|e| e.in_page(&page.source_path))?;

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

    // A page rejected for colliding with a copied asset never replaced it.
    let Some(output_path) = output_path.filter(|path| !is_asset_output(parsed_config, path)) else {
        return Ok(None);
    };

    if fs::exists(&output_path)? {
        fs::remove_file(&output_path)?;
    }
    remove_empty_parents(&output_path, &parsed_config.output_directory)?;

    Ok(Some(output_path))
}

fn create_parent_dir(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(parent) => fs::create_dir_all(parent),
        None => Ok(()),
    }
}

/// Removes the now-empty directories between `path` and `root` (exclusive), so
/// deleting the last nested or directory-style page doesn't leave stale folders.
pub(crate) fn remove_empty_parents(path: &Path, root: &Path) -> std::io::Result<()> {
    let mut directory = path.parent();
    while let Some(current) = directory {
        if current == root || !current.starts_with(root) || !current.is_dir() {
            break;
        }
        if fs::read_dir(current)?.next().is_some() {
            break;
        }
        fs::remove_dir(current)?;
        directory = current.parent();
    }
    Ok(())
}

/// Filename-based output path of a collection item, ignoring any `slug`
/// override (the source may no longer exist to read it from).
#[must_use]
pub fn collection_output_path_from_source(
    parsed_config: &config::ResolvedConfig,
    source_path: &Path,
) -> Option<PathBuf> {
    collection_location_from_source(parsed_config, source_path)
        .map(|(_, location)| location.output_path)
}

/// Derive a collection item's name and location from its path alone (no file
/// read, so any `slug` override is ignored), e.g. `content/posts/2024/hello.md`
/// -> (`posts`, `/posts/2024/hello`). Returns `None` when a static page owns
/// the same output file.
#[must_use]
pub fn collection_location_from_source(
    parsed_config: &config::ResolvedConfig,
    source_path: &Path,
) -> Option<(String, PageLocation)> {
    let item = CollectionItemLocation::from_source(parsed_config, source_path)?;
    let route = item.route(&item.file_stem);
    if static_page_for_collection_item(parsed_config, &route).is_some() {
        return None;
    }
    let location = page_location(parsed_config, &route)?;
    Some((item.collection_name, location))
}

/// Logical route of a page file relative to the pages directory, one entry per
/// URL segment: `docs/getting-started.hbs` -> `["docs", "getting-started"]`,
/// `docs/index.hbs` -> `["docs"]`, `index.hbs` -> `[]`.
fn static_page_route(relative_path: &Path) -> Option<Vec<String>> {
    let mut route = relative_path
        .parent()?
        .components()
        .map(|c| match c {
            Component::Normal(segment) => Some(segment.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let file_stem = relative_path.file_stem()?.to_string_lossy();
    if file_stem != "index" {
        route.push(file_stem.to_string());
    }
    Some(route)
}

/// Returns the static page that renders to the same output file as the
/// collection item at `route` (`[collection, folders..., slug]`), if any:
/// `pages/posts/hello.hbs` or `pages/posts/hello/index.hbs` for `/posts/hello`.
/// Static pages take precedence in that case.
#[must_use]
pub fn static_page_for_collection_item(
    parsed_config: &config::ResolvedConfig,
    route: &[&str],
) -> Option<PathBuf> {
    let (last, parents) = route.split_last()?;
    let item_output = page_location(parsed_config, route)?.output_path;
    let parent_dir = parents
        .iter()
        .fold(parsed_config.pages_directory.clone(), |dir, segment| {
            dir.join(segment)
        });
    ["hbs", "handlebars", "html"]
        .iter()
        .flat_map(|ext| {
            [
                parent_dir.join(format!("{last}.{ext}")),
                parent_dir.join(last).join(format!("index.{ext}")),
            ]
        })
        .find(|candidate| {
            candidate.is_file()
                && static_page_entry_from_source(parsed_config, candidate)
                    .is_some_and(|page| page.output_path == item_output)
        })
}

/// Returns the collection item that renders to the same output file as the
/// static page at `source_path`, if any (e.g. `pages/posts/hello.hbs` and
/// `content/posts/hello.md`, or an item with `slug: hello`). Reads the items of
/// the collection the page sits in, since `slug`s can change their URL.
#[must_use]
pub fn collection_item_for_static_page(
    parsed_config: &config::ResolvedConfig,
    source_path: &Path,
) -> Option<PathBuf> {
    let page_output = static_page_entry_from_source(parsed_config, source_path)?.output_path;
    let relative_path = source_path
        .strip_prefix(&parsed_config.pages_directory)
        .ok()?;
    let collection_name = static_page_route(relative_path)?.into_iter().next()?;
    let collection_dir = parsed_config.content_directory.join(collection_name);
    if !collection_dir.is_dir() {
        return None;
    }
    let mut source_paths = Vec::new();
    collect_markdown_files(&collection_dir, &mut source_paths).ok()?;
    source_paths.sort();
    source_paths.into_iter().find(|item_source| {
        // invalid items fail the build anyway, which a full rebuild reports
        unshadowed_collection_entry(parsed_config, item_source)
            .ok()
            .flatten()
            .is_some_and(|(page, _)| page.output_path == page_output)
    })
}
