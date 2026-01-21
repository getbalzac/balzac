use serde_json::Value;
use std::path::PathBuf;

use crate::config::SitemapConfig;

#[derive(Debug, Clone, PartialEq)]
pub enum PageType {
    Static,
    Collection { name: String },
}

#[derive(Debug, Clone, Default)]
pub struct SitemapMeta {
    pub lastmod: Option<String>,
    pub priority: Option<f32>,
    pub changefreq: Option<String>,
    pub exclude: bool,
}

impl SitemapMeta {
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
    pub fn new() -> Self {
        Self { pages: Vec::new() }
    }

    pub fn add_pages(&mut self, pages: Vec<PageEntry>) {
        self.pages.extend(pages);
    }

    pub fn all(&self) -> &[PageEntry] {
        &self.pages
    }

    pub fn static_pages(&self) -> Vec<&PageEntry> {
        self.pages
            .iter()
            .filter(|p| matches!(p.page_type, PageType::Static))
            .collect()
    }

    pub fn collection_items(&self) -> Vec<&PageEntry> {
        self.pages
            .iter()
            .filter(|p| matches!(p.page_type, PageType::Collection { .. }))
            .collect()
    }

    pub fn collection(&self, name: &str) -> Vec<&PageEntry> {
        self.pages
            .iter()
            .filter(|p| matches!(&p.page_type, PageType::Collection { name: n } if n == name))
            .collect()
    }

    pub fn sitemap_pages(&self) -> Vec<&PageEntry> {
        self.pages
            .iter()
            .filter(|p| !p.sitemap_meta.exclude)
            .collect()
    }
}

/// Generate sitemap XML content
pub fn generate_sitemap(site_pages: &SitePages, base_url: &str, config: &SitemapConfig) -> String {
    let mut xml = String::new();
    xml.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    xml.push('\n');
    xml.push_str(r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">"#);
    xml.push('\n');

    let base_url = base_url.trim_end_matches('/');

    for page in site_pages.sitemap_pages() {
        xml.push_str("  <url>\n");

        let full_url = format!("{}{}", base_url, page.url_path);
        xml.push_str(&format!("    <loc>{}</loc>\n", escape_xml(&full_url)));

        if let Some(ref lastmod) = page.sitemap_meta.lastmod {
            xml.push_str(&format!("    <lastmod>{}</lastmod>\n", escape_xml(lastmod)));
        }

        let changefreq = page
            .sitemap_meta
            .changefreq
            .as_ref()
            .or(config.default_changefreq.as_ref());
        if let Some(freq) = changefreq {
            xml.push_str(&format!(
                "    <changefreq>{}</changefreq>\n",
                escape_xml(freq)
            ));
        }

        let priority = page.sitemap_meta.priority.or(config.default_priority);
        if let Some(p) = priority {
            xml.push_str(&format!("    <priority>{:.1}</priority>\n", p));
        }

        xml.push_str("  </url>\n");
    }

    xml.push_str("</urlset>\n");
    xml
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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
    fn test_escape_xml() {
        assert_eq!(escape_xml("Hello & World"), "Hello &amp; World");
        assert_eq!(escape_xml("<test>"), "&lt;test&gt;");
    }
}
