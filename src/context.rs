use std::cmp::Ordering;

use crate::config;
use crate::renderer::{HandlebarsRenderer, Renderer};
use crate::sitemap::{PageEntry, PageType};

#[must_use]
pub fn merge_contexts(
    configuration: &config::ResolvedConfig,
    local_context: &serde_json::Value,
) -> serde_json::Value {
    let global_value = configuration
        .global
        .as_ref()
        .map(|g| serde_json::to_value(g).unwrap())
        .unwrap_or(serde_json::json!({}));

    let mut merged = global_value;
    merge(&mut merged, local_context);
    merged
}

fn merge(a: &mut serde_json::Value, b: &serde_json::Value) {
    if let (serde_json::Value::Object(a_map), serde_json::Value::Object(b_map)) = (a, b) {
        for (key, b_value) in b_map {
            match a_map.get_mut(key) {
                Some(a_value) if a_value.is_object() && b_value.is_object() => {
                    merge(a_value, b_value);
                }
                _ => {
                    a_map.insert(key.clone(), b_value.clone());
                }
            }
        }
    }
}

/// Builds the `collections` context exposed to templates: a map from collection
/// name to its items (`url`, `slug`, `fm`, rendered `content`), sorted by
/// `fm.date` descending (dated items first), then by slug ascending.
pub fn build_collections_context(
    pages: &[PageEntry],
    render: &HandlebarsRenderer,
) -> std::io::Result<serde_json::Value> {
    let mut collections: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();

    for page in pages {
        let PageType::Collection { name } = &page.page_type else {
            continue;
        };

        let slug = page
            .source_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let content = match &page.content {
            Some(raw_content) => render.render_markdown(raw_content)?,
            None => String::new(),
        };
        let item = serde_json::json!({
            "url": page.url_path,
            "slug": slug,
            "fm": page.frontmatter.clone().unwrap_or(serde_json::Value::Null),
            "content": content,
        });

        collections
            .entry(name.clone())
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .expect("collection entry should be an array")
            .push(item);
    }

    for items in collections.values_mut() {
        if let serde_json::Value::Array(items) = items {
            items.sort_by(compare_collection_items);
        }
    }

    Ok(serde_json::Value::Object(collections))
}

fn compare_collection_items(a: &serde_json::Value, b: &serde_json::Value) -> Ordering {
    let date = |item: &serde_json::Value| item["fm"]["date"].as_str().map(str::to_string);
    let slug = |item: &serde_json::Value| item["slug"].as_str().unwrap_or_default().to_string();

    match (date(a), date(b)) {
        (Some(a_date), Some(b_date)) => b_date.cmp(&a_date),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
    .then_with(|| slug(a).cmp(&slug(b)))
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn create_renderer() -> HandlebarsRenderer<'static> {
        HandlebarsRenderer::new(&config::Config::default().resolve(Path::new(".")))
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
    fn test_build_collections_context_item_shape() {
        let renderer = create_renderer();
        let pages = vec![collection_page(
            "posts",
            "hello",
            serde_json::json!({"title": "Hello"}),
        )];

        let context = build_collections_context(&pages, &renderer).unwrap();
        let item = &context["posts"][0];
        assert_eq!(item["url"], "/posts/hello");
        assert_eq!(item["slug"], "hello");
        assert_eq!(item["fm"]["title"], "Hello");
        assert_eq!(item["content"], "<h1>hello</h1>\n");
    }

    #[test]
    fn test_build_collections_context_groups_by_collection_and_skips_static() {
        let renderer = create_renderer();
        let mut static_page = collection_page("posts", "index", serde_json::json!(null));
        static_page.page_type = PageType::Static;
        let pages = vec![
            collection_page("posts", "a", serde_json::json!(null)),
            collection_page("notes", "b", serde_json::json!(null)),
            static_page,
        ];

        let context = build_collections_context(&pages, &renderer).unwrap();
        assert_eq!(slugs(&context, "posts"), vec!["a"]);
        assert_eq!(slugs(&context, "notes"), vec!["b"]);
        assert_eq!(context.as_object().unwrap().len(), 2);
    }

    #[test]
    fn test_build_collections_context_sorts_by_date_desc_then_slug() {
        let renderer = create_renderer();
        let pages = vec![
            collection_page("posts", "zeta", serde_json::json!({"title": "No date"})),
            collection_page("posts", "old", serde_json::json!({"date": "2024-01-01"})),
            collection_page("posts", "alpha", serde_json::json!(null)),
            collection_page("posts", "new", serde_json::json!({"date": "2025-06-01"})),
            collection_page("posts", "same-b", serde_json::json!({"date": "2024-03-01"})),
            collection_page("posts", "same-a", serde_json::json!({"date": "2024-03-01"})),
        ];

        let context = build_collections_context(&pages, &renderer).unwrap();
        assert_eq!(
            slugs(&context, "posts"),
            vec!["new", "same-a", "same-b", "old", "alpha", "zeta"]
        );
    }
}
