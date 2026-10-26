use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use serde_json::{Map, Value};

use crate::config;
use crate::error::Result;
use crate::markdown::RenderedMarkdown;
use crate::parallel;
use crate::renderer::{HandlebarsRenderer, Renderer};
use crate::sitemap::{PageEntry, PageType};

/// The `[global]` data with `local_context` merged over it. Takes the local
/// context by value: it holds `collections`, which is large, so moving it
/// avoids a copy per page.
#[must_use]
pub fn merge_contexts(
    configuration: &config::ResolvedConfig,
    local_context: serde_json::Value,
) -> serde_json::Value {
    let mut merged = configuration
        .global
        .as_ref()
        .map(|g| serde_json::to_value(g).unwrap())
        .unwrap_or(serde_json::json!({}));
    merge(&mut merged, local_context);
    merged
}

fn merge(a: &mut serde_json::Value, b: serde_json::Value) {
    if let (serde_json::Value::Object(a_map), serde_json::Value::Object(b_map)) = (a, b) {
        for (key, b_value) in b_map {
            match a_map.get_mut(&key) {
                Some(a_value) if a_value.is_object() && b_value.is_object() => {
                    merge(a_value, b_value);
                }
                _ => {
                    a_map.insert(key, b_value);
                }
            }
        }
    }
}

/// What the render passes of a build share, built once: the `collections`
/// context, the rendered markdown of every collection item, and a reusable
/// template context per render thread.
///
/// Every page's context holds `[global]` and the whole `collections` tree,
/// which includes the HTML of every item. Copying it for each page made builds
/// quadratic in the number of items, so each thread keeps one context and only
/// swaps the keys of the page it renders.
pub struct SiteContext<'a> {
    pages: &'a [PageEntry],
    collections: Value,
    documents: HashMap<&'a Path, RenderedMarkdown>,
    /// `[global]` with `collections` merged over it, the context of every page
    /// before its own keys are added.
    base: Map<String, Value>,
    /// One per thread of rayon's pool, created on first use.
    threads: Vec<Mutex<Option<ThreadContext>>>,
}

struct ThreadContext {
    context: handlebars::Context,
    /// Keys set by the last page rendered with `context`.
    page_keys: Vec<String>,
}

impl<'a> SiteContext<'a> {
    /// Renders the markdown of the collection items in `pages` and builds the
    /// `collections` context from it.
    pub fn new(
        configuration: &config::ResolvedConfig,
        pages: &'a [PageEntry],
        render: &HandlebarsRenderer,
    ) -> Result<Self> {
        let mut items: Vec<&'a PageEntry> = pages
            .iter()
            .filter(|page| matches!(page.page_type, PageType::Collection { .. }))
            .collect();
        items.sort_by(|a, b| compare_collection_items(a, b));

        let rendered = parallel::try_map(&items, |page| {
            render
                .render_document(page.content.as_deref().unwrap_or_default())
                .map_err(|e| e.in_page(&page.source_path))
        })?;
        let collections = collections_context(&items, &rendered);
        let documents = items
            .iter()
            .map(|page| page.source_path.as_path())
            .zip(rendered)
            .collect();

        let Value::Object(base) = merge_contexts(
            configuration,
            serde_json::json!({ "collections": collections.clone() }),
        ) else {
            unreachable!("merge_contexts returns an object");
        };

        Ok(Self {
            pages,
            collections,
            documents,
            base,
            threads: (0..rayon::current_num_threads())
                .map(|_| Mutex::new(None))
                .collect(),
        })
    }

    /// The pages of the site.
    #[must_use]
    pub fn pages(&self) -> &'a [PageEntry] {
        self.pages
    }

    /// The `collections` context: a map from collection name to its items
    /// (`url`, `slug`, `fm`, rendered `content`), sorted by `fm.date`
    /// descending (dated items first), then by slug ascending.
    #[must_use]
    pub fn collections(&self) -> &Value {
        &self.collections
    }

    /// The rendered markdown of the collection item at `source_path`.
    #[must_use]
    pub fn document(&self, source_path: &Path) -> Option<&RenderedMarkdown> {
        self.documents.get(source_path)
    }

    /// Calls `f` with the template context of a page: `[global]` and
    /// `collections`, with `page_context` merged over them as `merge_contexts`
    /// does.
    pub fn with_page_context<R>(
        &self,
        page_context: Map<String, Value>,
        f: impl FnOnce(&handlebars::Context) -> R,
    ) -> R {
        let slot = rayon::current_thread_index().and_then(|index| self.threads.get(index));
        // Outside the pool (e.g. dev mode re-rendering one page), or if `f`
        // ever renders another page on this thread, use a context of its own.
        let Some(mut slot) = slot.and_then(|slot| slot.try_lock().ok()) else {
            let mut context = Value::Object(self.base.clone());
            merge(&mut context, Value::Object(page_context));
            return f(&handlebars::Context::from(context));
        };
        let thread = slot.get_or_insert_with(|| ThreadContext {
            context: handlebars::Context::from(Value::Object(self.base.clone())),
            page_keys: Vec::new(),
        });

        let Value::Object(context) = thread.context.data_mut() else {
            unreachable!("page contexts are objects");
        };
        for key in thread.page_keys.drain(..) {
            match self.base.get(&key) {
                Some(value) => context.insert(key, value.clone()),
                None => context.remove(&key),
            };
        }
        for (key, value) in page_context {
            let value = match self.base.get(&key) {
                Some(base @ Value::Object(_)) if value.is_object() => {
                    let mut merged = base.clone();
                    merge(&mut merged, value);
                    merged
                }
                _ => value,
            };
            thread.page_keys.push(key.clone());
            context.insert(key, value);
        }

        f(&thread.context)
    }
}

/// The `collections` context of `items`, sorted, and their rendered markdown.
fn collections_context(items: &[&PageEntry], rendered: &[RenderedMarkdown]) -> Value {
    let mut collections = Map::new();

    for (page, document) in items.iter().zip(rendered) {
        let PageType::Collection { name } = &page.page_type else {
            continue;
        };

        let item = serde_json::json!({
            "url": page.url_path,
            "slug": item_slug(page),
            "fm": page.frontmatter.clone().unwrap_or(Value::Null),
            "content": document.html,
        });

        collections
            .entry(name.clone())
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .expect("collection entry should be an array")
            .push(item);
    }

    Value::Object(collections)
}

/// The item's last URL segment: its `slug` frontmatter when set, else the
/// markdown file name.
fn item_slug(page: &PageEntry) -> &str {
    // Directory-style URLs end with `/` (`/posts/hello/`)
    page.url_path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default()
}

/// Order of collection items everywhere they are listed: `fm.date` descending
/// (dated items first), then slug ascending.
#[must_use]
pub fn compare_collection_items(a: &PageEntry, b: &PageEntry) -> Ordering {
    fn date(page: &PageEntry) -> Option<&str> {
        page.frontmatter.as_ref()?.get("date")?.as_str()
    }

    match (date(a), date(b)) {
        (Some(a_date), Some(b_date)) => b_date.cmp(a_date),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
    .then_with(|| item_slug(a).cmp(item_slug(b)))
    // Nested items can share a slug (`/posts/2024/a`, `/posts/2025/a`)
    .then_with(|| a.url_path.cmp(&b.url_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page_context(value: serde_json::Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            _ => panic!("expected an object"),
        }
    }

    /// The context of each page rendered in turn on one thread, so they all
    /// reuse the same per-thread context.
    fn contexts_of(pages: Vec<serde_json::Value>) -> Vec<Value> {
        let config: config::Config =
            toml::from_str("[global]\nsite = \"Blog\"\nfm = { author = \"Ada\" }\n").unwrap();
        let config = config.resolve(Path::new("."));
        let renderer = create_renderer();
        let items = vec![collection_page("posts", "hello", serde_json::json!(null))];

        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap()
            .install(|| {
                let site = SiteContext::new(&config, &items, &renderer).unwrap();
                pages
                    .into_iter()
                    .map(|page| {
                        site.with_page_context(page_context(page), |context| context.data().clone())
                    })
                    .collect()
            })
    }

    #[test]
    fn test_page_contexts_merge_over_global_and_collections() {
        let contexts = contexts_of(vec![serde_json::json!({
            "fm": { "title": "Post" },
            "content": "<p>hi</p>",
        })]);

        assert_eq!(contexts[0]["site"], "Blog");
        assert_eq!(
            contexts[0]["fm"],
            serde_json::json!({ "author": "Ada", "title": "Post" })
        );
        assert_eq!(contexts[0]["content"], "<p>hi</p>");
        assert_eq!(contexts[0]["collections"]["posts"][0]["slug"], "hello");
    }

    #[test]
    fn test_page_contexts_dont_keep_keys_of_previous_pages() {
        let contexts = contexts_of(vec![
            serde_json::json!({ "fm": { "title": "First" }, "content": "<p>1</p>" }),
            serde_json::json!({ "pagination": { "current_page": 1 } }),
            serde_json::json!({ "fm": { "draft": true } }),
        ]);

        // `content` and `fm.title` of the first page are gone
        assert_eq!(contexts[1].get("content"), None);
        assert_eq!(contexts[1]["fm"], serde_json::json!({ "author": "Ada" }));
        assert_eq!(contexts[2].get("pagination"), None);
        assert_eq!(
            contexts[2]["fm"],
            serde_json::json!({ "author": "Ada", "draft": true })
        );
        assert_eq!(contexts[2]["collections"]["posts"][0]["slug"], "hello");
    }

    #[test]
    fn test_page_context_outside_the_pool_matches_merge_contexts() {
        let config = default_config();
        let renderer = create_renderer();
        let site = SiteContext::new(&config, &[], &renderer).unwrap();
        let page = serde_json::json!({ "fm": { "title": "Post" } });

        let context =
            site.with_page_context(page_context(page.clone()), |context| context.data().clone());

        let mut expected = merge_contexts(&config, page);
        expected["collections"] = serde_json::json!({});
        assert_eq!(context, expected);
    }

    #[test]
    fn test_merge_contexts_merges_local_over_global() {
        let config: config::Config = toml::from_str(
            "[global]\nsite = \"Blog\"\nfm = { author = \"Ada\", title = \"Default\" }\n",
        )
        .unwrap();
        let config = config.resolve(Path::new("."));

        let merged = merge_contexts(
            &config,
            serde_json::json!({ "fm": { "title": "Post" }, "content": "<p>hi</p>" }),
        );

        assert_eq!(
            merged,
            serde_json::json!({
                "site": "Blog",
                "fm": { "author": "Ada", "title": "Post" },
                "content": "<p>hi</p>",
            })
        );
    }
    use crate::sitemap::SitemapMeta;
    use std::path::{Path, PathBuf};

    fn collection_page(collection: &str, slug: &str, fm: serde_json::Value) -> PageEntry {
        PageEntry {
            url_path: format!("/{collection}/{slug}"),
            source_path: PathBuf::from(format!("content/{collection}/{slug}.md")),
            output_path: PathBuf::from(format!("dist/{collection}/{slug}.html")),
            page_type: PageType::Collection {
                name: collection.to_string(),
            },
            sitemap_meta: SitemapMeta::default(),
            frontmatter: Some(fm),
            content: Some(format!("# {slug}")),
        }
    }

    fn default_config() -> config::ResolvedConfig {
        config::Config::default().resolve(Path::new("."))
    }

    fn create_renderer() -> HandlebarsRenderer<'static> {
        HandlebarsRenderer::new(&default_config())
    }

    fn slugs(context: &serde_json::Value, collection: &str) -> Vec<String> {
        context[collection]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["slug"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn test_collections_context_item_shape() {
        let renderer = create_renderer();
        let pages = vec![collection_page(
            "posts",
            "hello",
            serde_json::json!({"title": "Hello"}),
        )];

        let site = SiteContext::new(&default_config(), &pages, &renderer).unwrap();
        let context = site.collections();
        let item = &context["posts"][0];
        assert_eq!(item["url"], "/posts/hello");
        assert_eq!(item["slug"], "hello");
        assert_eq!(item["fm"]["title"], "Hello");
        assert_eq!(item["content"], "<h1>hello</h1>\n");
    }

    #[test]
    fn test_collections_context_groups_by_collection_and_skips_static() {
        let renderer = create_renderer();
        let mut static_page = collection_page("posts", "index", serde_json::json!(null));
        static_page.page_type = PageType::Static;
        let pages = vec![
            collection_page("posts", "a", serde_json::json!(null)),
            collection_page("notes", "b", serde_json::json!(null)),
            static_page,
        ];

        let site = SiteContext::new(&default_config(), &pages, &renderer).unwrap();
        let context = site.collections();
        assert_eq!(slugs(context, "posts"), vec!["a"]);
        assert_eq!(slugs(context, "notes"), vec!["b"]);
        assert_eq!(context.as_object().unwrap().len(), 2);
    }

    #[test]
    fn test_collections_context_sorts_by_date_desc_then_slug() {
        let renderer = create_renderer();
        let pages = vec![
            collection_page("posts", "zeta", serde_json::json!({"title": "No date"})),
            collection_page("posts", "old", serde_json::json!({"date": "2024-01-01"})),
            collection_page("posts", "alpha", serde_json::json!(null)),
            collection_page("posts", "new", serde_json::json!({"date": "2025-06-01"})),
            collection_page("posts", "same-b", serde_json::json!({"date": "2024-03-01"})),
            collection_page("posts", "same-a", serde_json::json!({"date": "2024-03-01"})),
        ];

        let site = SiteContext::new(&default_config(), &pages, &renderer).unwrap();
        let context = site.collections();
        assert_eq!(
            slugs(context, "posts"),
            vec!["new", "same-a", "same-b", "old", "alpha", "zeta"]
        );
    }

    #[test]
    fn test_compare_collection_items_breaks_ties_by_slug_then_url() {
        // `a.md` has `slug: zeta`, `b.md` has `slug: alpha`
        let mut a = collection_page("posts", "a", serde_json::json!(null));
        a.url_path = "/posts/zeta".to_string();
        let mut b = collection_page("posts", "b", serde_json::json!(null));
        b.url_path = "/posts/alpha".to_string();
        assert_eq!(compare_collection_items(&a, &b), Ordering::Greater);

        let mut nested_2025 = collection_page("posts", "x", serde_json::json!(null));
        nested_2025.url_path = "/posts/2025/same".to_string();
        let mut nested_2024 = collection_page("posts", "y", serde_json::json!(null));
        nested_2024.url_path = "/posts/2024/same".to_string();
        assert_eq!(
            compare_collection_items(&nested_2025, &nested_2024),
            Ordering::Greater
        );
    }
}
