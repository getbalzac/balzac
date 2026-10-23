//! Generated listing pages: paginated collection listings (`[listings.<name>]`)
//! and taxonomy term/index pages (`[taxonomies.<name>]`).
//!
//! Every generated URL is built by the helpers in this module (`section_url`,
//! `paginated_url`, `term_url`, `output_path_for_url`), so the URL scheme can
//! be changed in one place.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::config::{ResolvedConfig, TaxonomyConfig};
use crate::context::{build_collections_context, compare_collection_items, merge_contexts};
use crate::renderer::{HandlebarsRenderer, Renderer};
use crate::sitemap::{PageEntry, PageType, SitemapMeta};

/// Template of a collection listing, in `<pages_directory>/<collection>/`.
pub const LISTING_TEMPLATE: &str = "list.hbs";
/// Template of a taxonomy term page, in `<pages_directory>/<taxonomy>/`.
pub const TERM_TEMPLATE: &str = "term.hbs";
/// Optional template of a taxonomy's index of terms, in `<pages_directory>/<taxonomy>/`.
pub const TAXONOMY_TEMPLATE: &str = "taxonomy.hbs";

#[derive(Debug, Clone, PartialEq)]
pub enum ListingPage {
    Collection {
        collection: String,
        pagination: Pagination,
    },
    Term {
        taxonomy: String,
        /// Shared by every page of the term and by the taxonomy index.
        term: Arc<Term>,
        pagination: Pagination,
    },
    Taxonomy {
        taxonomy: String,
        terms: Vec<Arc<Term>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Term {
    /// Term as first written in frontmatter (in listing order).
    pub name: String,
    pub slug: String,
    pub url: String,
    /// URLs of the items classified under this term, in listing order.
    pub item_urls: Vec<String>,
}

/// One page of a paginated listing.
#[derive(Debug, Clone, PartialEq)]
pub struct Pagination {
    /// URL of the first page; later pages live under `<base_url>page/<n>/`.
    pub base_url: String,
    /// 1-based.
    pub current_page: usize,
    pub total_pages: usize,
    pub per_page: usize,
    pub total_items: usize,
    /// URLs of the items on this page.
    pub item_urls: Vec<String>,
}

impl ListingPage {
    #[must_use]
    pub fn url(&self) -> String {
        match self {
            ListingPage::Collection { pagination, .. } | ListingPage::Term { pagination, .. } => {
                pagination.url()
            }
            ListingPage::Taxonomy { taxonomy, .. } => section_url(&[taxonomy]),
        }
    }

    /// Template context for this page; `items` maps item URLs to their entry
    /// in the `collections` context.
    fn context(&self, items: &HashMap<&str, &Value>) -> Value {
        match self {
            ListingPage::Collection {
                collection,
                pagination,
            } => json!({
                "collection": collection,
                "pagination": pagination.context(items),
            }),
            ListingPage::Term {
                taxonomy,
                term,
                pagination,
            } => json!({
                "taxonomy": { "name": taxonomy, "term": term.summary() },
                "pagination": pagination.context(items),
            }),
            ListingPage::Taxonomy { taxonomy, terms } => {
                let terms: Vec<Value> = terms
                    .iter()
                    .map(|term| {
                        let mut value = term.summary();
                        value["items"] = Value::Array(lookup_items(&term.item_urls, items));
                        value
                    })
                    .collect();
                json!({ "taxonomy": { "name": taxonomy, "terms": terms } })
            }
        }
    }
}

impl Term {
    fn summary(&self) -> Value {
        json!({
            "name": self.name,
            "slug": self.slug,
            "url": self.url,
            "count": self.item_urls.len(),
        })
    }
}

impl Pagination {
    #[must_use]
    pub fn url(&self) -> String {
        paginated_url(&self.base_url, self.current_page)
    }

    fn context(&self, items: &HashMap<&str, &Value>) -> Value {
        let page_url = |page: usize| paginated_url(&self.base_url, page);
        let pages: Vec<Value> = (1..=self.total_pages)
            .map(|page| {
                json!({
                    "number": page,
                    "url": page_url(page),
                    "current": page == self.current_page,
                })
            })
            .collect();

        json!({
            "items": lookup_items(&self.item_urls, items),
            "current_page": self.current_page,
            "total_pages": self.total_pages,
            "per_page": self.per_page,
            "total_items": self.total_items,
            "prev_url": (self.current_page > 1).then(|| page_url(self.current_page - 1)),
            "next_url": (self.current_page < self.total_pages).then(|| page_url(self.current_page + 1)),
            "first_url": page_url(1),
            "last_url": page_url(self.total_pages),
            "pages": pages,
        })
    }
}

fn lookup_items(urls: &[String], items: &HashMap<&str, &Value>) -> Vec<Value> {
    urls.iter()
        .filter_map(|url| items.get(url.as_str()).map(|item| (*item).clone()))
        .collect()
}

/// URL of a directory-style section, e.g. `["tags", "rust"]` -> `/tags/rust/`.
#[must_use]
pub fn section_url(segments: &[&str]) -> String {
    format!("/{}/", segments.join("/"))
}

/// URL of page `page` (1-based) of a listing whose first page is `base_url`.
#[must_use]
pub fn paginated_url(base_url: &str, page: usize) -> String {
    if page <= 1 {
        base_url.to_string()
    } else {
        format!("{base_url}page/{page}/")
    }
}

/// URL of the page listing items classified under `term` in `taxonomy`.
#[must_use]
pub fn term_url(taxonomy: &str, term: &str) -> String {
    section_url(&[taxonomy, &slugify(term)])
}

/// Output file of a generated page: `/posts/page/2/` -> `<output>/posts/page/2/index.html`.
#[must_use]
pub fn output_path_for_url(output_directory: &Path, url: &str) -> PathBuf {
    let mut path = output_directory.to_path_buf();
    path.extend(url.split('/').filter(|segment| !segment.is_empty()));
    path.join("index.html")
}

/// Lowercases `term` and joins its alphanumeric runs with `-`:
/// `"Rust & WebAssembly"` -> `rust-webassembly`.
#[must_use]
pub fn slugify(term: &str) -> String {
    let mut slug = String::new();
    let mut needs_separator = false;
    for c in term.chars() {
        if c.is_alphanumeric() {
            if needs_separator && !slug.is_empty() {
                slug.push('-');
            }
            needs_separator = false;
            slug.extend(c.to_lowercase());
        } else {
            needs_separator = true;
        }
    }
    slug
}

/// Term name of a frontmatter value; taxonomies accept strings and numbers.
#[must_use]
pub fn term_name(value: &Value) -> Option<String> {
    match value {
        Value::String(name) => Some(name.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// Terms of `key` in `frontmatter`, which may hold a single term or a list.
fn term_names(frontmatter: Option<&Value>, key: &str) -> Vec<String> {
    match frontmatter.and_then(|fm| fm.get(key)) {
        Some(Value::Array(values)) => values.iter().filter_map(term_name).collect(),
        Some(value) => term_name(value).into_iter().collect(),
        None => Vec::new(),
    }
}

/// Splits `item_urls` into pages of `per_page` items (a single page when
/// unset). An empty listing still gets one, empty, page.
fn paginate(
    base_url: &str,
    item_urls: &[String],
    per_page: Option<NonZeroUsize>,
) -> Vec<Pagination> {
    let per_page = per_page.map_or(item_urls.len().max(1), NonZeroUsize::get);
    let total_pages = item_urls.len().div_ceil(per_page).max(1);

    (1..=total_pages)
        .map(|page| Pagination {
            base_url: base_url.to_string(),
            current_page: page,
            total_pages,
            per_page,
            total_items: item_urls.len(),
            item_urls: item_urls
                .iter()
                .skip((page - 1) * per_page)
                .take(per_page)
                .cloned()
                .collect(),
        })
        .collect()
}

fn collection_name(page: &PageEntry) -> Option<&str> {
    match &page.page_type {
        PageType::Collection { name } => Some(name),
        _ => None,
    }
}

fn listing_entry(config: &ResolvedConfig, template: &Path, listing: ListingPage) -> PageEntry {
    let url_path = listing.url();
    PageEntry {
        output_path: output_path_for_url(&config.output_directory, &url_path),
        url_path,
        source_path: template.to_path_buf(),
        page_type: PageType::Listing(Box::new(listing)),
        sitemap_meta: SitemapMeta::default(),
        frontmatter: None,
        content: None,
    }
}

/// Plans the listing and taxonomy pages configured in `balzac.toml` from the
/// discovered collection items in `pages`. Listings whose template is missing
/// are skipped with a warning.
pub fn discover_listings(
    config: &ResolvedConfig,
    pages: &[PageEntry],
) -> std::io::Result<Vec<PageEntry>> {
    let mut items: Vec<&PageEntry> = pages
        .iter()
        .filter(|page| collection_name(page).is_some())
        .collect();
    items.sort_by(|a, b| compare_collection_items(a, b));

    let mut listings = Vec::new();

    let mut collections: Vec<_> = config.listings.iter().collect();
    collections.sort_by_key(|(name, _)| *name);
    for (collection, listing_config) in collections {
        let template = config
            .pages_directory
            .join(collection)
            .join(LISTING_TEMPLATE);
        if !fs::exists(&template)? {
            log::warn!(
                "Listing for collection {collection} has no template at {}, skipping",
                template.display()
            );
            continue;
        }

        let item_urls: Vec<String> = items
            .iter()
            .filter(|item| collection_name(item) == Some(collection))
            .map(|item| item.url_path.clone())
            .collect();
        for pagination in paginate(
            &section_url(&[collection]),
            &item_urls,
            listing_config.per_page,
        ) {
            listings.push(listing_entry(
                config,
                &template,
                ListingPage::Collection {
                    collection: collection.clone(),
                    pagination,
                },
            ));
        }
    }

    let mut taxonomies: Vec<_> = config.taxonomies.iter().collect();
    taxonomies.sort_by_key(|(name, _)| *name);
    for (taxonomy, taxonomy_config) in taxonomies {
        listings.extend(discover_taxonomy(
            config,
            taxonomy,
            taxonomy_config,
            &items,
        )?);
    }

    let mut outputs: HashMap<&Path, &str> = items
        .iter()
        .map(|item| (item.output_path.as_path(), item.url_path.as_str()))
        .collect();
    for listing in &listings {
        if let Some(existing) = outputs.insert(&listing.output_path, &listing.url_path) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "{} and the listing {} would both be written to {}",
                    existing,
                    listing.url_path,
                    listing.output_path.display()
                ),
            ));
        }
    }

    Ok(listings)
}

/// Term pages (and the optional index) of one taxonomy; `items` are sorted.
fn discover_taxonomy(
    config: &ResolvedConfig,
    taxonomy: &str,
    taxonomy_config: &TaxonomyConfig,
    items: &[&PageEntry],
) -> std::io::Result<Vec<PageEntry>> {
    let term_template = config.pages_directory.join(taxonomy).join(TERM_TEMPLATE);
    if !fs::exists(&term_template)? {
        log::warn!(
            "Taxonomy {taxonomy} has no template at {}, skipping",
            term_template.display()
        );
        return Ok(Vec::new());
    }

    let terms = collect_terms(taxonomy, taxonomy_config, items);
    let mut listings = Vec::new();
    for term in &terms {
        for pagination in paginate(&term.url, &term.item_urls, taxonomy_config.per_page) {
            listings.push(listing_entry(
                config,
                &term_template,
                ListingPage::Term {
                    taxonomy: taxonomy.to_string(),
                    term: Arc::clone(term),
                    pagination,
                },
            ));
        }
    }

    let taxonomy_template = config
        .pages_directory
        .join(taxonomy)
        .join(TAXONOMY_TEMPLATE);
    if fs::exists(&taxonomy_template)? {
        listings.push(listing_entry(
            config,
            &taxonomy_template,
            ListingPage::Taxonomy {
                taxonomy: taxonomy.to_string(),
                terms,
            },
        ));
    }

    Ok(listings)
}

/// Terms of `taxonomy` sorted by slug; each keeps the order of `items`.
fn collect_terms(
    taxonomy: &str,
    taxonomy_config: &TaxonomyConfig,
    items: &[&PageEntry],
) -> Vec<Arc<Term>> {
    let mut terms: BTreeMap<String, Term> = BTreeMap::new();

    for item in items {
        let in_taxonomy = taxonomy_config.collections.as_ref().is_none_or(|names| {
            names
                .iter()
                .any(|name| collection_name(item) == Some(name.as_str()))
        });
        if !in_taxonomy {
            continue;
        }

        // Items are distinct, so only a term repeated within this item can
        // list it twice.
        let mut item_slugs = HashSet::new();
        for name in term_names(item.frontmatter.as_ref(), taxonomy) {
            let slug = slugify(&name);
            if !item_slugs.insert(slug.clone()) {
                continue;
            }
            if slug.is_empty() {
                log::warn!(
                    "Ignoring {taxonomy} term {name:?} in {}: it has no letters or digits",
                    item.source_path.display()
                );
                continue;
            }
            let term = terms.entry(slug.clone()).or_insert_with(|| Term {
                url: section_url(&[taxonomy, &slug]),
                name,
                slug,
                item_urls: Vec::new(),
            });
            term.item_urls.push(item.url_path.clone());
        }
    }

    terms.into_values().map(Arc::new).collect()
}

/// Renders every listing entry in `pages`.
pub fn render_listing_pages(
    config: &ResolvedConfig,
    pages: &[PageEntry],
    render: &HandlebarsRenderer,
) -> std::io::Result<()> {
    if !pages
        .iter()
        .any(|page| matches!(page.page_type, PageType::Listing(_)))
    {
        return Ok(());
    }

    let collections = build_collections_context(pages, render)?;
    let items: HashMap<&str, &Value> = collections
        .as_object()
        .into_iter()
        .flat_map(|collections| collections.values())
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|item| Some((item["url"].as_str()?, item)))
        .collect();
    let mut templates: HashMap<&Path, String> = HashMap::new();

    for page in pages {
        let PageType::Listing(listing) = &page.page_type else {
            continue;
        };
        log::info!("Rendering listing {}", page.url_path);

        if !templates.contains_key(page.source_path.as_path()) {
            templates.insert(&page.source_path, fs::read_to_string(&page.source_path)?);
        }
        let template = templates[page.source_path.as_path()].clone();

        let mut context = listing.context(&items);
        context["collections"] = collections.clone();
        let rendered = render.render(template, merge_contexts(config, &context));

        if let Some(parent) = page.output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&page.output_path, rendered)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ListingConfig, TaxonomyConfig};
    use tempfile::TempDir;

    fn item(collection: &str, slug: &str, fm: Value) -> PageEntry {
        PageEntry {
            url_path: format!("/{collection}/{slug}"),
            source_path: PathBuf::from(format!("content/{collection}/{slug}.md")),
            output_path: PathBuf::from(format!("dist/{collection}/{slug}.html")),
            page_type: PageType::Collection {
                name: collection.to_string(),
            },
            sitemap_meta: SitemapMeta::default(),
            frontmatter: Some(fm),
            content: Some(String::new()),
        }
    }

    fn setup(templates: &[&str]) -> (TempDir, ResolvedConfig) {
        let temp_dir = TempDir::new().unwrap();
        for template in templates {
            let path = temp_dir.path().join("pages").join(template);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        let config = Config::default().resolve(temp_dir.path());
        (temp_dir, config)
    }

    fn per_page(n: usize) -> Option<NonZeroUsize> {
        NonZeroUsize::new(n)
    }

    fn urls(entries: &[PageEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.url_path.as_str()).collect()
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Rust"), "rust");
        assert_eq!(slugify("  Rust & WebAssembly! "), "rust-webassembly");
        assert_eq!(slugify("C++"), "c");
        assert_eq!(slugify("Économie 2024"), "économie-2024");
        assert_eq!(slugify("--"), "");
    }

    #[test]
    fn test_urls_and_output_paths() {
        assert_eq!(section_url(&["posts"]), "/posts/");
        assert_eq!(paginated_url("/posts/", 1), "/posts/");
        assert_eq!(paginated_url("/posts/", 3), "/posts/page/3/");
        assert_eq!(term_url("tags", "Web Dev"), "/tags/web-dev/");
        assert_eq!(
            output_path_for_url(Path::new("dist"), "/tags/rust/page/2/"),
            PathBuf::from("dist/tags/rust/page/2/index.html")
        );
    }

    #[test]
    fn test_paginate() {
        let urls: Vec<String> = (1..=5).map(|i| format!("/p/{i}")).collect();

        let pages = paginate("/p/", &urls, per_page(2));
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[2].item_urls, vec!["/p/5"]);
        assert_eq!(pages[2].total_items, 5);
        assert_eq!(pages[2].url(), "/p/page/3/");

        let single = paginate("/p/", &urls, None);
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].per_page, 5);

        let empty = paginate("/p/", &[], per_page(2));
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0].item_urls, Vec::<String>::new());
    }

    #[test]
    fn test_pagination_context() {
        let urls: Vec<String> = (1..=5).map(|i| format!("/p/{i}")).collect();
        let pages = paginate("/p/", &urls, per_page(2));
        let item = json!({"url": "/p/3"});
        let items = HashMap::from([("/p/3", &item)]);

        let context = pages[1].context(&items);
        assert_eq!(context["current_page"], 2);
        assert_eq!(context["total_pages"], 3);
        assert_eq!(context["prev_url"], "/p/");
        assert_eq!(context["next_url"], "/p/page/3/");
        assert_eq!(context["first_url"], "/p/");
        assert_eq!(context["last_url"], "/p/page/3/");
        assert_eq!(context["items"], json!([{"url": "/p/3"}]));
        assert_eq!(
            context["pages"][1],
            json!({"number": 2, "url": "/p/page/2/", "current": true})
        );

        let first = pages[0].context(&items);
        assert_eq!(first["prev_url"], Value::Null);
        assert_eq!(pages[2].context(&items)["next_url"], Value::Null);
    }

    #[test]
    fn test_discover_collection_listing() {
        let (_temp, mut config) = setup(&["posts/list.hbs"]);
        config.listings.insert(
            "posts".to_string(),
            ListingConfig {
                per_page: per_page(2),
            },
        );
        let pages = vec![
            item("posts", "a", json!({"date": "2024-01-01"})),
            item("posts", "b", json!({"date": "2025-01-01"})),
            item("posts", "c", json!(null)),
            item("notes", "n", json!(null)),
        ];

        let listings = discover_listings(&config, &pages).unwrap();
        assert_eq!(urls(&listings), vec!["/posts/", "/posts/page/2/"]);
        assert_eq!(
            listings[1].output_path,
            config.output_directory.join("posts/page/2/index.html")
        );
        let PageType::Listing(listing) = &listings[0].page_type else {
            panic!("expected a listing");
        };
        let ListingPage::Collection { pagination, .. } = listing.as_ref() else {
            panic!("expected a collection listing");
        };
        assert_eq!(pagination.item_urls, vec!["/posts/b", "/posts/a"]);
        assert_eq!(pagination.total_items, 3);
    }

    #[test]
    fn test_discover_listing_without_template_is_skipped() {
        let (_temp, mut config) = setup(&[]);
        config
            .listings
            .insert("posts".to_string(), ListingConfig::default());
        config
            .taxonomies
            .insert("tags".to_string(), TaxonomyConfig::default());
        let pages = vec![item("posts", "a", json!({"tags": ["rust"]}))];

        assert!(discover_listings(&config, &pages).unwrap().is_empty());
    }

    #[test]
    fn test_discover_taxonomy_terms() {
        let (_temp, mut config) = setup(&["tags/term.hbs", "tags/taxonomy.hbs"]);
        config.taxonomies.insert(
            "tags".to_string(),
            TaxonomyConfig {
                per_page: per_page(1),
                collections: Some(vec!["posts".to_string()]),
            },
        );
        let pages = vec![
            item("posts", "a", json!({"tags": ["Rust", "web"]})),
            item("posts", "b", json!({"tags": "rust", "date": "2025-01-01"})),
            item("posts", "c", json!({"tags": ["rust", "Rust", "!!"]})),
            item("notes", "n", json!({"tags": ["notes-only"]})),
        ];

        let listings = discover_listings(&config, &pages).unwrap();
        assert_eq!(
            urls(&listings),
            vec![
                "/tags/rust/",
                "/tags/rust/page/2/",
                "/tags/rust/page/3/",
                "/tags/web/",
                "/tags/"
            ]
        );

        let PageType::Listing(index) = &listings[4].page_type else {
            panic!("expected a listing");
        };
        let ListingPage::Taxonomy { terms, .. } = index.as_ref() else {
            panic!("expected a taxonomy index");
        };
        // The dated item comes first, so its spelling names the term.
        assert_eq!(terms[0].name, "rust");
        assert_eq!(terms[0].item_urls, vec!["/posts/b", "/posts/a", "/posts/c"]);
        assert_eq!(terms[1].name, "web");
    }

    #[test]
    fn test_discover_listing_conflicting_with_collection_item() {
        let (_temp, mut config) = setup(&["posts/list.hbs"]);
        config
            .listings
            .insert("posts".to_string(), ListingConfig::default());
        let mut index = item("posts", "index", json!(null));
        index.output_path = config.output_directory.join("posts/index.html");

        assert!(discover_listings(&config, &[index]).is_err());
    }

    #[test]
    fn test_discover_taxonomy_index_conflicting_with_listing() {
        let (_temp, mut config) =
            setup(&["posts/list.hbs", "posts/term.hbs", "posts/taxonomy.hbs"]);
        config
            .listings
            .insert("posts".to_string(), ListingConfig::default());
        config
            .taxonomies
            .insert("posts".to_string(), TaxonomyConfig::default());

        // Both the listing and the taxonomy index would write `/posts/`.
        assert!(discover_listings(&config, &[]).is_err());
    }

    #[test]
    fn test_discover_taxonomy_sharing_name_without_overlapping_pages() {
        // No `list.hbs` and no `taxonomy.hbs`: nothing else writes term pages.
        let (_temp, mut config) = setup(&["posts/term.hbs"]);
        config
            .listings
            .insert("posts".to_string(), ListingConfig::default());
        config
            .taxonomies
            .insert("posts".to_string(), TaxonomyConfig::default());
        let pages = vec![item("posts", "a", json!({"posts": ["rust"]}))];

        let listings = discover_listings(&config, &pages).unwrap();
        assert_eq!(urls(&listings), vec!["/posts/rust/"]);
    }

    #[test]
    fn test_discover_taxonomy_ignores_content_folder_that_is_not_a_collection() {
        // `content/tags/` without `pages/tags/details.hbs` renders no items.
        let (temp, mut config) = setup(&["tags/term.hbs"]);
        fs::create_dir_all(temp.path().join("content/tags")).unwrap();
        config
            .taxonomies
            .insert("tags".to_string(), TaxonomyConfig::default());
        let pages = vec![item("posts", "a", json!({"tags": ["rust"]}))];

        let listings = discover_listings(&config, &pages).unwrap();
        assert_eq!(urls(&listings), vec!["/tags/rust/"]);
    }
}
