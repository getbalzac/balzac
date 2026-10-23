use serde_json::Value;
use std::fmt::Write as _;
use std::path::PathBuf;

use crate::config::SitemapConfig;
use crate::listing::ListingPage;

#[derive(Debug, Clone, PartialEq)]
pub enum PageType {
    Static,
    Collection {
        name: String,
    },
    /// Generated page: collection listing, taxonomy term or taxonomy index.
    Listing(Box<ListingPage>),
}

#[derive(Debug, Clone, Default)]
pub struct SitemapMeta {
    pub lastmod: Option<String>,
    pub priority: Option<f32>,
    pub changefreq: Option<String>,
    pub exclude: bool,
}

impl SitemapMeta {
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // sitemap priorities are 0.0-1.0
    pub fn from_frontmatter(fm: &Value) -> Self {
        let mut meta = SitemapMeta::default();

        if let Value::Object(obj) = fm {
            if let Some(Value::String(lastmod)) = obj.get("lastmod") {
                meta.lastmod = Some(lastmod.clone());
            }
            if let Some(priority) = obj.get("priority") {
                meta.priority = match priority {
                    Value::Number(n) => n.as_f64().map(|f| f as f32),
                    _ => None,
                };
            }
            if let Some(Value::String(changefreq)) = obj.get("changefreq") {
                meta.changefreq = Some(changefreq.clone());
            }
            if let Some(Value::Bool(exclude)) = obj.get("sitemap_exclude") {
                meta.exclude = *exclude;
            }
        }

        meta
    }
}

#[derive(Debug, Clone)]
pub struct PageEntry {
    pub url_path: String,
    pub source_path: PathBuf,
    pub output_path: PathBuf,
    pub page_type: PageType,
    pub sitemap_meta: SitemapMeta,
    pub frontmatter: Option<Value>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SitePages {
    pages: Vec<PageEntry>,
}

impl SitePages {
    #[must_use]
    pub fn new() -> Self {
        Self { pages: Vec::new() }
    }

    pub fn add_pages(&mut self, pages: Vec<PageEntry>) {
        self.pages.extend(pages);
    }

    #[must_use]
    pub fn all(&self) -> &[PageEntry] {
        &self.pages
    }

    #[must_use]
    pub fn static_pages(&self) -> Vec<&PageEntry> {
        self.pages
            .iter()
            .filter(|p| matches!(p.page_type, PageType::Static))
            .collect()
    }

    #[must_use]
    pub fn collection_items(&self) -> Vec<&PageEntry> {
        self.pages
            .iter()
            .filter(|p| matches!(p.page_type, PageType::Collection { .. }))
            .collect()
    }

    #[must_use]
    pub fn collection(&self, name: &str) -> Vec<&PageEntry> {
        self.pages
            .iter()
            .filter(|p| matches!(&p.page_type, PageType::Collection { name: n } if n == name))
            .collect()
    }

    #[must_use]
    pub fn sitemap_pages(&self) -> Vec<&PageEntry> {
        self.pages
            .iter()
            .filter(|p| !p.sitemap_meta.exclude)
            .collect()
    }
}

/// Generate sitemap XML content
#[must_use]
pub fn generate_sitemap(site_pages: &SitePages, base_url: &str, config: &SitemapConfig) -> String {
    let mut xml = String::new();
    xml.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    xml.push('\n');
    xml.push_str(r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">"#);
    xml.push('\n');

    let base_url = base_url.trim_end_matches('/');

    for page in site_pages.sitemap_pages() {
        xml.push_str("  <url>\n");

        let full_url = format!("{}{}", base_url, encode_url_path(&page.url_path));
        let _ = writeln!(xml, "    <loc>{}</loc>", escape_xml(&full_url));

        if let Some(ref lastmod) = page.sitemap_meta.lastmod {
            let _ = writeln!(xml, "    <lastmod>{}</lastmod>", escape_xml(lastmod));
        }

        let changefreq = page
            .sitemap_meta
            .changefreq
            .as_ref()
            .or(config.default_changefreq.as_ref());
        if let Some(freq) = changefreq {
            let _ = writeln!(xml, "    <changefreq>{}</changefreq>", escape_xml(freq));
        }

        let priority = page.sitemap_meta.priority.or(config.default_priority);
        if let Some(p) = priority {
            let _ = writeln!(xml, "    <priority>{p:.1}</priority>");
        }

        xml.push_str("  </url>\n");
    }

    xml.push_str("</urlset>\n");
    xml
}

/// Percent-encodes a page's `url_path` for publishing: `/docs/c#` becomes
/// `/docs/c%23` and `/tags/économie/` becomes `/tags/%C3%A9conomie/`.
/// Unreserved characters, sub-delimiters, `:`, `@` and `/` are kept as is.
fn encode_url_path(url_path: &str) -> String {
    let mut encoded = String::with_capacity(url_path.len());
    for byte in url_path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

/// Escapes XML special characters and drops characters that XML 1.0 forbids
pub(crate) fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if c < ' ' || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sitemap_meta_from_frontmatter() {
        let fm = serde_json::json!({
            "title": "Test Post",
            "lastmod": "2024-01-20",
            "priority": 0.8,
            "changefreq": "weekly",
            "sitemap_exclude": false
        });

        let meta = SitemapMeta::from_frontmatter(&fm);
        assert_eq!(meta.lastmod, Some("2024-01-20".to_string()));
        assert_eq!(meta.priority, Some(0.8));
        assert_eq!(meta.changefreq, Some("weekly".to_string()));
        assert!(!meta.exclude);
    }

    #[test]
    fn test_sitemap_meta_exclude() {
        let fm = serde_json::json!({
            "sitemap_exclude": true
        });

        let meta = SitemapMeta::from_frontmatter(&fm);
        assert!(meta.exclude);
    }

    #[test]
    fn test_site_pages_filtering() {
        let mut site_pages = SitePages::new();
        site_pages.add_pages(vec![
            PageEntry {
                url_path: "/".to_string(),
                source_path: PathBuf::from("pages/index.hbs"),
                output_path: PathBuf::from("dist/index.html"),
                page_type: PageType::Static,
                sitemap_meta: SitemapMeta::default(),
                frontmatter: None,
                content: None,
            },
            PageEntry {
                url_path: "/blog/post-1".to_string(),
                source_path: PathBuf::from("content/blog/post-1.md"),
                output_path: PathBuf::from("dist/blog/post-1.html"),
                page_type: PageType::Collection {
                    name: "blog".to_string(),
                },
                sitemap_meta: SitemapMeta::default(),
                frontmatter: Some(serde_json::json!({"title": "Post 1"})),
                content: Some("<p>Content</p>".to_string()),
            },
        ]);

        assert_eq!(site_pages.all().len(), 2);
        assert_eq!(site_pages.static_pages().len(), 1);
        assert_eq!(site_pages.collection_items().len(), 1);
        assert_eq!(site_pages.collection("blog").len(), 1);
    }

    #[test]
    fn test_generate_sitemap() {
        let mut site_pages = SitePages::new();
        site_pages.add_pages(vec![
            PageEntry {
                url_path: "/".to_string(),
                source_path: PathBuf::from("pages/index.hbs"),
                output_path: PathBuf::from("dist/index.html"),
                page_type: PageType::Static,
                sitemap_meta: SitemapMeta {
                    lastmod: Some("2024-01-20".to_string()),
                    priority: Some(1.0),
                    changefreq: Some("daily".to_string()),
                    exclude: false,
                },
                frontmatter: None,
                content: None,
            },
            PageEntry {
                url_path: "/about".to_string(),
                source_path: PathBuf::from("pages/about.hbs"),
                output_path: PathBuf::from("dist/about.html"),
                page_type: PageType::Static,
                sitemap_meta: SitemapMeta {
                    exclude: true,
                    ..Default::default()
                },
                frontmatter: None,
                content: None,
            },
        ]);

        let config = SitemapConfig::default();
        let xml = generate_sitemap(&site_pages, "https://example.com", &config);

        assert!(xml.contains("https://example.com/"));
        assert!(xml.contains("<lastmod>2024-01-20</lastmod>"));
        assert!(xml.contains("<priority>1.0</priority>"));
        assert!(xml.contains("<changefreq>daily</changefreq>"));
        // The excluded page should not be in the sitemap
        assert!(!xml.contains("/about"));
    }

    #[test]
    fn test_encode_url_path() {
        assert_eq!(
            encode_url_path("/docs/getting-started"),
            "/docs/getting-started"
        );
        assert_eq!(encode_url_path("/docs/c#"), "/docs/c%23");
        assert_eq!(encode_url_path("/posts/part one"), "/posts/part%20one");
        assert_eq!(encode_url_path("/posts/100%"), "/posts/100%25");
        assert_eq!(encode_url_path("/tags/économie/"), "/tags/%C3%A9conomie/");
        assert_eq!(encode_url_path("/a?b"), "/a%3Fb");
    }

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("Hello & World"), "Hello &amp; World");
        assert_eq!(escape_xml("<test>"), "&lt;test&gt;");
        assert_eq!(escape_xml("a\u{0}b\u{1B}c\td"), "abc\td");
    }
}
