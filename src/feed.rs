use std::fmt::Write as _;

use jiff::{Timestamp, civil, fmt::rfc2822, tz::Offset};
use serde_json::Value;

use crate::config::{FeedConfig, FeedFormat};
use crate::sitemap::{PageEntry, PageType, SitePages, escape_xml};

/// Feed-relevant metadata read from a collection entry's frontmatter.
///
/// Recognized keys:
/// - `title`: item title (falls back to the URL path)
/// - `date`: publication date; entries without a valid `date` are left out of feeds
/// - `updated` (or `lastmod`): last modification date, defaults to `date`
/// - `summary` (or `description`): short plain-text summary
/// - `author`: item author name
#[derive(Debug, Clone, PartialEq)]
pub struct FeedMeta {
    pub title: Option<String>,
    pub published: Timestamp,
    pub updated: Timestamp,
    pub summary: Option<String>,
    pub author: Option<String>,
}

impl FeedMeta {
    /// Returns `None` when the frontmatter has no parseable `date`
    #[must_use]
    pub fn from_frontmatter(fm: &Value) -> Option<Self> {
        let get_str = |key: &str| fm.get(key).and_then(Value::as_str);

        let published = parse_date(get_str("date")?)?;
        let updated = get_str("updated")
            .or_else(|| get_str("lastmod"))
            .and_then(parse_date)
            .unwrap_or(published);

        Some(Self {
            title: get_str("title").map(str::to_string),
            published,
            updated,
            summary: get_str("summary")
                .or_else(|| get_str("description"))
                .map(str::to_string),
            author: get_str("author").map(str::to_string),
        })
    }
}

/// A collection entry selected for a feed, with its parsed metadata
#[derive(Debug, Clone)]
pub struct FeedEntry<'a> {
    pub page: &'a PageEntry,
    pub meta: FeedMeta,
}

/// Selects the entries of `collections` that belong in feeds, newest first.
///
/// This is the single place deciding which entries are published in feeds.
#[must_use]
pub fn select_feed_entries<'a>(
    site_pages: &'a SitePages,
    collections: &[String],
) -> Vec<FeedEntry<'a>> {
    let mut entries: Vec<FeedEntry<'a>> = collections
        .iter()
        .flat_map(|collection| site_pages.collection(collection))
        .filter_map(|page| {
            let meta = page
                .frontmatter
                .as_ref()
                .and_then(FeedMeta::from_frontmatter);
            if meta.is_none() {
                log::warn!(
                    "{} has no valid `date` in its frontmatter, leaving it out of feeds",
                    page.source_path.display()
                );
            }
            meta.map(|meta| FeedEntry { page, meta })
        })
        .collect();

    entries.sort_by(|a, b| {
        b.meta
            .published
            .cmp(&a.meta.published)
            .then_with(|| a.page.url_path.cmp(&b.page.url_path))
    });
    entries
}

impl FeedEntry<'_> {
    #[must_use]
    pub fn in_collection(&self, collection: &str) -> bool {
        matches!(&self.page.page_type, PageType::Collection { name } if name == collection)
    }
}

/// Feed-level metadata shared by the Atom and RSS writers
#[derive(Debug, Clone)]
pub struct FeedChannel<'a> {
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub author: Option<&'a str>,
    /// Site root URL, without a trailing slash
    pub site_url: &'a str,
    /// Absolute URL of the feed file itself
    pub feed_url: &'a str,
}

/// An item ready to be written: an entry plus its optional rendered HTML body
#[derive(Debug, Clone)]
pub struct FeedItem<'a> {
    pub entry: FeedEntry<'a>,
    pub content_html: Option<String>,
}

impl FeedItem<'_> {
    fn title(&self) -> &str {
        self.entry
            .meta
            .title
            .as_deref()
            .unwrap_or(&self.entry.page.url_path)
    }

    fn link(&self, site_url: &str) -> String {
        format!("{site_url}{}", self.entry.page.url_path)
    }
}

/// Collection names that get their own feed, as configured or every discovered collection
#[must_use]
pub fn feed_collections(site_pages: &SitePages, config: &FeedConfig) -> Vec<String> {
    resolve_collections(site_pages, config.collections.as_deref())
}

/// The configured collection names, or every discovered collection when unset
#[must_use]
pub fn resolve_collections(site_pages: &SitePages, configured: Option<&[String]>) -> Vec<String> {
    if let Some(collections) = configured {
        return collections.to_vec();
    }

    let mut names: Vec<String> = site_pages
        .collection_items()
        .into_iter()
        .filter_map(|p| match &p.page_type {
            PageType::Collection { name } => Some(name.clone()),
            PageType::Static | PageType::Listing(_) => None,
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// A concrete feed document type written for a collection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedKind {
    Atom,
    Rss,
}

impl FeedKind {
    #[must_use]
    pub fn for_format(format: FeedFormat) -> &'static [FeedKind] {
        match format {
            FeedFormat::Atom => &[FeedKind::Atom],
            FeedFormat::Rss => &[FeedKind::Rss],
            FeedFormat::Both => &[FeedKind::Atom, FeedKind::Rss],
        }
    }

    /// File name written inside the collection's output directory
    #[must_use]
    pub fn filename(self) -> &'static str {
        match self {
            FeedKind::Atom => "atom.xml",
            FeedKind::Rss => "feed.xml",
        }
    }

    #[must_use]
    pub fn generate(self, channel: &FeedChannel<'_>, items: &[FeedItem<'_>]) -> String {
        match self {
            FeedKind::Atom => generate_atom(channel, items),
            FeedKind::Rss => generate_rss(channel, items),
        }
    }
}

/// Generate an Atom 1.0 feed (RFC 4287)
#[must_use]
pub fn generate_atom(channel: &FeedChannel<'_>, items: &[FeedItem<'_>]) -> String {
    let mut xml = String::new();
    xml.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    xml.push('\n');
    xml.push_str(r#"<feed xmlns="http://www.w3.org/2005/Atom">"#);
    xml.push('\n');

    let _ = writeln!(xml, "  <id>{}</id>", escape_xml(channel.feed_url));
    let _ = writeln!(xml, "  <title>{}</title>", escape_xml(channel.title));
    if let Some(description) = channel.description {
        let _ = writeln!(xml, "  <subtitle>{}</subtitle>", escape_xml(description));
    }
    let updated = items
        .iter()
        .map(|i| i.entry.meta.updated)
        .max()
        .unwrap_or(Timestamp::UNIX_EPOCH);
    let _ = writeln!(xml, "  <updated>{}</updated>", format_rfc3339(updated));
    let _ = writeln!(
        xml,
        r#"  <link rel="self" type="application/atom+xml" href="{}"/>"#,
        escape_xml(channel.feed_url)
    );
    let _ = writeln!(
        xml,
        r#"  <link rel="alternate" type="text/html" href="{}/"/>"#,
        escape_xml(channel.site_url)
    );
    // Atom requires a feed author unless every entry has one; only then fall
    // back to the feed title so the document stays valid
    let feed_author = channel.author.or_else(|| {
        items
            .iter()
            .any(|i| i.entry.meta.author.is_none())
            .then_some(channel.title)
    });
    if let Some(author) = feed_author {
        let _ = writeln!(
            xml,
            "  <author><name>{}</name></author>",
            escape_xml(author)
        );
    }

    for item in items {
        let meta = &item.entry.meta;
        let link = item.link(channel.site_url);
        xml.push_str("  <entry>\n");
        let _ = writeln!(xml, "    <id>{}</id>", escape_xml(&link));
        let _ = writeln!(xml, "    <title>{}</title>", escape_xml(item.title()));
        let _ = writeln!(
            xml,
            r#"    <link rel="alternate" type="text/html" href="{}"/>"#,
            escape_xml(&link)
        );
        let _ = writeln!(
            xml,
            "    <published>{}</published>",
            format_rfc3339(meta.published)
        );
        let _ = writeln!(
            xml,
            "    <updated>{}</updated>",
            format_rfc3339(meta.updated)
        );
        if let Some(author) = &meta.author {
            let _ = writeln!(
                xml,
                "    <author><name>{}</name></author>",
                escape_xml(author)
            );
        }
        if let Some(summary) = &meta.summary {
            let _ = writeln!(
                xml,
                r#"    <summary type="text">{}</summary>"#,
                escape_xml(summary)
            );
        }
        if let Some(content) = &item.content_html {
            let _ = writeln!(
                xml,
                r#"    <content type="html" xml:base="{}">{}</content>"#,
                escape_xml(&link),
                escape_xml(content)
            );
        }
        xml.push_str("  </entry>\n");
    }

    xml.push_str("</feed>\n");
    xml
}

/// Generate an RSS 2.0 feed
#[must_use]
pub fn generate_rss(channel: &FeedChannel<'_>, items: &[FeedItem<'_>]) -> String {
    let mut xml = String::new();
    xml.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    xml.push('\n');
    xml.push_str(r#"<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom" xmlns:content="http://purl.org/rss/1.0/modules/content/" xmlns:dc="http://purl.org/dc/elements/1.1/">"#);
    xml.push('\n');
    xml.push_str("  <channel>\n");

    let _ = writeln!(xml, "    <title>{}</title>", escape_xml(channel.title));
    let _ = writeln!(xml, "    <link>{}/</link>", escape_xml(channel.site_url));
    let _ = writeln!(
        xml,
        "    <description>{}</description>",
        escape_xml(channel.description.unwrap_or(channel.title))
    );
    let _ = writeln!(
        xml,
        r#"    <atom:link href="{}" rel="self" type="application/rss+xml"/>"#,
        escape_xml(channel.feed_url)
    );
    if let Some(last_build) = items.iter().map(|i| i.entry.meta.updated).max() {
        let _ = writeln!(
            xml,
            "    <lastBuildDate>{}</lastBuildDate>",
            format_rfc822(last_build)
        );
    }

    for item in items {
        let meta = &item.entry.meta;
        let link = item.link(channel.site_url);
        xml.push_str("    <item>\n");
        let _ = writeln!(xml, "      <title>{}</title>", escape_xml(item.title()));
        let _ = writeln!(xml, "      <link>{}</link>", escape_xml(&link));
        let _ = writeln!(
            xml,
            r#"      <guid isPermaLink="true">{}</guid>"#,
            escape_xml(&link)
        );
        let _ = writeln!(
            xml,
            "      <pubDate>{}</pubDate>",
            format_rfc822(meta.published)
        );
        // RSS <author> must be an email address; dc:creator takes a plain name
        if let Some(author) = &meta.author {
            let _ = writeln!(xml, "      <dc:creator>{}</dc:creator>", escape_xml(author));
        }
        if let Some(summary) = &meta.summary {
            // Readers parse <description> as HTML, so the plain-text summary is
            // HTML-escaped before being XML-escaped
            let _ = writeln!(
                xml,
                "      <description>{}</description>",
                escape_xml(&escape_xml(summary))
            );
        }
        if let Some(content) = &item.content_html {
            let _ = writeln!(
                xml,
                "      <content:encoded>{}</content:encoded>",
                escape_xml(content)
            );
        }
        xml.push_str("    </item>\n");
    }

    xml.push_str("  </channel>\n");
    xml.push_str("</rss>\n");
    xml
}

/// Parses a frontmatter date: RFC 3339 (`2024-01-15T10:00:00+02:00`),
/// a datetime without offset (`2024-01-15T10:00:00` or `2024-01-15 10:00`), or
/// a plain date (`2024-01-15`). Values without an offset are taken as UTC.
#[must_use]
pub fn parse_date(value: &str) -> Option<Timestamp> {
    let value = value.trim();
    if let Ok(ts) = value.parse::<Timestamp>() {
        return Some(ts);
    }
    if let Ok(dt) = value.parse::<civil::DateTime>() {
        return Offset::UTC.to_timestamp(dt).ok();
    }
    None
}

/// RFC 3339 timestamp in UTC, as required by Atom
#[must_use]
pub fn format_rfc3339(ts: Timestamp) -> String {
    ts.to_string()
}

/// RFC 822 (via RFC 2822 / RFC 9110) timestamp in GMT, as required by RSS 2.0
#[must_use]
pub fn format_rfc822(ts: Timestamp) -> String {
    rfc2822::DateTimePrinter::new()
        .timestamp_to_rfc9110_string(&ts)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::sitemap::SitemapMeta;

    fn entry(collection: &str, slug: &str, fm: &Value) -> PageEntry {
        PageEntry {
            url_path: format!("/{collection}/{slug}"),
            source_path: PathBuf::from(format!("content/{collection}/{slug}.md")),
            output_path: PathBuf::from(format!("dist/{collection}/{slug}.html")),
            page_type: PageType::Collection {
                name: collection.to_string(),
            },
            sitemap_meta: SitemapMeta::default(),
            frontmatter: Some(fm.clone()),
            content: Some("Body".to_string()),
        }
    }

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn channel<'a>() -> FeedChannel<'a> {
        FeedChannel {
            title: "Tom & Jerry's <blog>",
            description: Some("News \"daily\""),
            author: None,
            site_url: "https://example.com",
            feed_url: "https://example.com/blog/atom.xml",
        }
    }

    #[test]
    fn test_parse_date_formats() {
        assert_eq!(parse_date("2024-01-15"), Some(ts("2024-01-15T00:00:00Z")));
        assert_eq!(
            parse_date("2024-01-15T10:30:00+02:00"),
            Some(ts("2024-01-15T08:30:00Z"))
        );
        assert_eq!(
            parse_date("2024-01-15T10:30:00Z"),
            Some(ts("2024-01-15T10:30:00Z"))
        );
        assert_eq!(
            parse_date("2024-01-15 10:30"),
            Some(ts("2024-01-15T10:30:00Z"))
        );
        assert_eq!(
            parse_date(" 2024-01-15T10:30:00 "),
            Some(ts("2024-01-15T10:30:00Z"))
        );
    }

    #[test]
    fn test_parse_date_rejects_invalid() {
        assert_eq!(parse_date("2024-02-30"), None);
        assert_eq!(parse_date("January 15, 2024"), None);
        assert_eq!(parse_date(""), None);
    }

    #[test]
    fn test_date_formatting() {
        let date = ts("2024-01-15T08:30:00Z");
        assert_eq!(format_rfc3339(date), "2024-01-15T08:30:00Z");
        assert_eq!(format_rfc822(date), "Mon, 15 Jan 2024 08:30:00 GMT");
    }

    #[test]
    fn test_feed_meta_from_frontmatter() {
        let fm = serde_json::json!({
            "title": "Hello",
            "date": "2024-01-15",
            "lastmod": "2024-02-01",
            "description": "A post",
            "author": "Ada"
        });
        let meta = FeedMeta::from_frontmatter(&fm).unwrap();
        assert_eq!(meta.title.as_deref(), Some("Hello"));
        assert_eq!(meta.published, ts("2024-01-15T00:00:00Z"));
        assert_eq!(meta.updated, ts("2024-02-01T00:00:00Z"));
        assert_eq!(meta.summary.as_deref(), Some("A post"));
        assert_eq!(meta.author.as_deref(), Some("Ada"));
    }

    #[test]
    fn test_feed_meta_prefers_summary_and_updated() {
        let fm = serde_json::json!({
            "date": "2024-01-15",
            "updated": "2024-03-01",
            "lastmod": "2024-02-01",
            "summary": "Short",
            "description": "Long"
        });
        let meta = FeedMeta::from_frontmatter(&fm).unwrap();
        assert_eq!(meta.updated, ts("2024-03-01T00:00:00Z"));
        assert_eq!(meta.summary.as_deref(), Some("Short"));
    }

    #[test]
    fn test_feed_meta_requires_date() {
        assert!(FeedMeta::from_frontmatter(&serde_json::json!({"title": "x"})).is_none());
        assert!(FeedMeta::from_frontmatter(&serde_json::json!({"date": "soon"})).is_none());
        assert!(FeedMeta::from_frontmatter(&Value::Null).is_none());
    }

    #[test]
    fn test_select_feed_entries_sorts_and_filters() {
        let mut site_pages = SitePages::new();
        site_pages.add_pages(vec![
            entry("blog", "old", &serde_json::json!({"date": "2023-01-01"})),
            entry("blog", "new", &serde_json::json!({"date": "2024-06-01"})),
            entry("blog", "mid", &serde_json::json!({"date": "2024-01-01"})),
            entry("blog", "undated", &serde_json::json!({"title": "x"})),
            entry("notes", "other", &serde_json::json!({"date": "2024-03-01"})),
            entry("pages", "skip", &serde_json::json!({"date": "2025-01-01"})),
        ]);

        let urls = |collections: &[&str]| {
            let collections: Vec<String> = collections.iter().map(ToString::to_string).collect();
            select_feed_entries(&site_pages, &collections)
                .into_iter()
                .map(|e| e.page.url_path.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(urls(&["blog"]), ["/blog/new", "/blog/mid", "/blog/old"]);
        assert_eq!(
            urls(&["blog", "notes"]),
            ["/blog/new", "/notes/other", "/blog/mid", "/blog/old"]
        );
    }

    #[test]
    fn test_feed_collections_defaults_to_all() {
        let mut site_pages = SitePages::new();
        site_pages.add_pages(vec![
            entry("notes", "a", &Value::Null),
            entry("blog", "b", &Value::Null),
            entry("blog", "c", &Value::Null),
        ]);
        let mut config = FeedConfig::default();
        assert_eq!(feed_collections(&site_pages, &config), ["blog", "notes"]);

        config.collections = Some(vec!["notes".to_string()]);
        assert_eq!(feed_collections(&site_pages, &config), ["notes"]);
    }

    fn sample_items(site_pages: &SitePages) -> Vec<FeedItem<'_>> {
        select_feed_entries(site_pages, &["blog".to_string()])
            .into_iter()
            .map(|entry| FeedItem {
                entry,
                content_html: Some("<p>Body & more</p>".to_string()),
            })
            .collect()
    }

    fn sample_pages() -> SitePages {
        let mut site_pages = SitePages::new();
        site_pages.add_pages(vec![entry(
            "blog",
            "first",
            &serde_json::json!({
                "title": "R&D <notes>",
                "date": "2024-01-15T10:00:00+02:00",
                "summary": "Fish & chips",
                "author": "Ada"
            }),
        )]);
        site_pages
    }

    #[test]
    fn test_generate_atom() {
        let site_pages = sample_pages();
        let xml = generate_atom(&channel(), &sample_items(&site_pages));

        assert!(xml.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
        assert!(xml.contains(r#"<feed xmlns="http://www.w3.org/2005/Atom">"#));
        assert!(xml.contains("<title>Tom &amp; Jerry&apos;s &lt;blog&gt;</title>"));
        assert!(xml.contains("<subtitle>News &quot;daily&quot;</subtitle>"));
        assert!(xml.contains("<updated>2024-01-15T08:00:00Z</updated>"));
        assert!(xml.contains(
            r#"<link rel="self" type="application/atom+xml" href="https://example.com/blog/atom.xml"/>"#
        ));
        assert!(
            !xml.split("<entry>").next().unwrap().contains("<author>"),
            "every entry has an author, so the feed needs none"
        );
        assert!(xml.contains("<id>https://example.com/blog/first</id>"));
        assert!(xml.contains("<title>R&amp;D &lt;notes&gt;</title>"));
        assert!(xml.contains("<published>2024-01-15T08:00:00Z</published>"));
        assert!(xml.contains("<author><name>Ada</name></author>"));
        assert!(xml.contains(r#"<summary type="text">Fish &amp; chips</summary>"#));
        assert!(
            xml.contains(r#"<content type="html" xml:base="https://example.com/blog/first">&lt;p&gt;Body &amp; more&lt;/p&gt;</content>"#)
        );
    }

    #[test]
    fn test_generate_rss() {
        let site_pages = sample_pages();
        let mut channel = channel();
        channel.feed_url = "https://example.com/blog/feed.xml";
        let xml = generate_rss(&channel, &sample_items(&site_pages));

        assert!(xml.contains(r#"<rss version="2.0""#));
        assert!(xml.contains("<link>https://example.com/</link>"));
        assert!(xml.contains("<description>News &quot;daily&quot;</description>"));
        assert!(xml.contains(
            r#"<atom:link href="https://example.com/blog/feed.xml" rel="self" type="application/rss+xml"/>"#
        ));
        assert!(xml.contains("<lastBuildDate>Mon, 15 Jan 2024 08:00:00 GMT</lastBuildDate>"));
        assert!(xml.contains("<link>https://example.com/blog/first</link>"));
        assert!(xml.contains(r#"<guid isPermaLink="true">https://example.com/blog/first</guid>"#));
        assert!(xml.contains("<pubDate>Mon, 15 Jan 2024 08:00:00 GMT</pubDate>"));
        assert!(xml.contains("<dc:creator>Ada</dc:creator>"));
        assert!(xml.contains("<description>Fish &amp;amp; chips</description>"));
        assert!(
            xml.contains("<content:encoded>&lt;p&gt;Body &amp; more&lt;/p&gt;</content:encoded>")
        );
    }

    #[test]
    fn test_rss_summary_is_html_escaped_text() {
        let mut site_pages = SitePages::new();
        site_pages.add_pages(vec![entry(
            "blog",
            "tags",
            &serde_json::json!({"date": "2024-01-15", "summary": "The <first> post"}),
        )]);
        let xml = generate_rss(&channel(), &sample_items(&site_pages));
        // XML-decodes to `The &lt;first&gt; post`, which HTML renders as the literal text
        assert!(xml.contains("<description>The &amp;lt;first&amp;gt; post</description>"));
    }

    #[test]
    fn test_atom_feed_author() {
        let pages = |author: Option<&str>| {
            let mut fm = serde_json::json!({"date": "2024-01-15"});
            if let Some(author) = author {
                fm["author"] = serde_json::json!(author);
            }
            let mut site_pages = SitePages::new();
            site_pages.add_pages(vec![entry("blog", "post", &fm)]);
            site_pages
        };
        let feed_author = |xml: &str| xml.split("<entry>").next().unwrap().contains("<author>");

        // Every entry has an author: no feed-level fallback needed
        let site_pages = pages(Some("Ada"));
        assert!(!feed_author(&generate_atom(
            &channel(),
            &sample_items(&site_pages)
        )));

        // An entry without an author: fall back to the title to stay valid
        let site_pages = pages(None);
        let xml = generate_atom(&channel(), &sample_items(&site_pages));
        assert!(
            xml.contains("  <author><name>Tom &amp; Jerry&apos;s &lt;blog&gt;</name></author>")
        );

        // A configured author always wins
        let mut configured = channel();
        configured.author = Some("Jane");
        let xml = generate_atom(&configured, &sample_items(&site_pages));
        assert!(xml.contains("  <author><name>Jane</name></author>"));
    }

    #[test]
    fn test_untitled_item_falls_back_to_url_path() {
        let mut site_pages = SitePages::new();
        site_pages.add_pages(vec![entry(
            "blog",
            "untitled",
            &serde_json::json!({"date": "2024-01-15"}),
        )]);
        let xml = generate_rss(&channel(), &sample_items(&site_pages));
        assert!(xml.contains("<title>/blog/untitled</title>"));
    }
}
