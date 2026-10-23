use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SitemapConfig {
    #[serde(default = "default_sitemap_enabled")]
    pub enabled: bool,
    #[serde(
        default = "default_sitemap_filename",
        skip_serializing_if = "is_default_sitemap_filename"
    )]
    pub filename: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_priority: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_changefreq: Option<String>,
}

impl Default for SitemapConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            filename: default_sitemap_filename(),
            default_priority: None,
            default_changefreq: None,
        }
    }
}

fn default_sitemap_enabled() -> bool {
    true
}

fn default_sitemap_filename() -> String {
    "sitemap.xml".to_string()
}

fn is_default_sitemap_filename(s: &String) -> bool {
    s == &default_sitemap_filename()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RobotsConfig {
    #[serde(default = "default_robots_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disallow: Vec<String>,
}

impl Default for RobotsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            disallow: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[allow(clippy::struct_excessive_bools)] // independent on/off switches mirroring balzac.toml keys
#[serde(deny_unknown_fields)]
pub struct MarkdownConfig {
    /// Highlight fenced code blocks with CSS classes (`hl-*`)
    #[serde(default = "default_true")]
    pub syntax_highlighting: bool,
    /// Bundled syntect theme written to `<output>/highlight.css`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight_theme: Option<String>,
    /// Render `id` attributes on headings
    #[serde(default)]
    pub heading_ids: bool,
    /// Render a `#` link inside headings (implies `heading_ids`)
    #[serde(default)]
    pub heading_anchors: bool,
    #[serde(default)]
    pub footnotes: bool,
}

impl Default for MarkdownConfig {
    fn default() -> Self {
        Self {
            syntax_highlighting: true,
            highlight_theme: None,
            heading_ids: false,
            heading_anchors: false,
            footnotes: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FeedFormat {
    #[default]
    Atom,
    Rss,
    Both,
}

/// Per-collection overrides for feed metadata
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FeedOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FeedConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub format: FeedFormat,
    /// Collections to generate feeds for; `None` means every collection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collections: Option<Vec<String>>,
    /// Maximum number of items per feed; 0 means unlimited
    #[serde(default = "default_feed_limit")]
    pub limit: usize,
    /// Include the rendered markdown body in each item
    #[serde(default)]
    pub full_content: bool,
    /// Site-wide feed at the output root merging several collections
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combined: Option<CombinedFeedSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, FeedOverride>,
}

impl Default for FeedConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            format: FeedFormat::default(),
            collections: None,
            limit: default_feed_limit(),
            full_content: false,
            combined: None,
            title: None,
            description: None,
            author: None,
            overrides: BTreeMap::new(),
        }
    }
}

fn default_robots_enabled() -> bool {
    true
}

/// How page URLs map to files in the output directory.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UrlStyle {
    /// `about` is written to `about.html` and linked as `/about`.
    /// Requires a host that rewrites extensionless URLs (Netlify, Vercel, Cloudflare).
    #[default]
    Extensionless,
    /// `about` is written to `about/index.html` and linked as `/about/`.
    /// Works on any static host (S3, GitHub Pages, nginx defaults).
    Directory,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // signature required by serde's skip_serializing_if
fn is_default_url_style(style: &UrlStyle) -> bool {
    *style == UrlStyle::default()
}

fn default_true() -> bool {
    true
}

/// `combined = true`, `combined = ["blog", "notes"]` or a `[feed.combined]` table
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum CombinedFeedSetting {
    Enabled(bool),
    Collections(Vec<String>),
    Detailed(CombinedFeedConfig),
}

// Dispatches on the value type instead of `#[serde(untagged)]`, which would
// replace a typo in `[feed.combined]` with "did not match any variant".
impl<'de> Deserialize<'de> for CombinedFeedSetting {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SettingVisitor;

        impl<'de> serde::de::Visitor<'de> for SettingVisitor {
            type Value = CombinedFeedSetting;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a boolean, a list of collections or a table")
            }

            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(CombinedFeedSetting::Enabled(value))
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                seq: A,
            ) -> Result<Self::Value, A::Error> {
                Deserialize::deserialize(serde::de::value::SeqAccessDeserializer::new(seq))
                    .map(CombinedFeedSetting::Collections)
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Self::Value, A::Error> {
                Deserialize::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(CombinedFeedSetting::Detailed)
            }
        }

        deserializer.deserialize_any(SettingVisitor)
    }
}

impl CombinedFeedSetting {
    /// The combined feed settings, or `None` when it is turned off
    #[must_use]
    pub fn resolve(&self) -> Option<CombinedFeedConfig> {
        match self {
            CombinedFeedSetting::Enabled(false) => None,
            CombinedFeedSetting::Enabled(true) => Some(CombinedFeedConfig::default()),
            CombinedFeedSetting::Collections(collections) => Some(CombinedFeedConfig {
                collections: Some(collections.clone()),
                ..Default::default()
            }),
            CombinedFeedSetting::Detailed(config) => Some(config.clone()),
        }
    }
}

/// Unset fields fall back to the `[feed]` values; `collections` defaults to
/// the collections that get their own feed
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CombinedFeedConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collections: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Maximum number of items; 0 means unlimited
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

fn default_feed_limit() -> usize {
    20
}

#[derive(Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct Hooks {
    /// Also run the hooks in `balzac dev` (off by default)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dev: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_init_before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_init_after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_after: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ViteBundler {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_vite_manifest_path")]
    pub manifest_path: String,
    #[serde(
        default = "default_vite_dev_origin",
        skip_serializing_if = "is_default_vite_dev_origin"
    )]
    pub dev_origin: String,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Bundler {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vite: Option<ViteBundler>,
}

/// Paginated listing of a collection, configured under `[listings.<collection>]`.
/// Rendered with `<pages_directory>/<collection>/list.hbs`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ListingConfig {
    /// Items per page; all items go on a single page when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_page: Option<NonZeroUsize>,
}

/// Taxonomy built from the frontmatter key `<name>`, configured under
/// `[taxonomies.<name>]`. Term pages are rendered with
/// `<pages_directory>/<name>/term.hbs`, the optional index of all terms with
/// `<pages_directory>/<name>/taxonomy.hbs`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaxonomyConfig {
    /// Items per term page; all items go on a single page when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_page: Option<NonZeroUsize>,
    /// Collections whose items are classified; every collection when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collections: Option<Vec<String>>,
}

/// Every config struct denies unknown keys so that typos fail loudly; only
/// `global` (and map keys such as collection names) is free-form. New config
/// structs need `#[serde(deny_unknown_fields)]` too (serde does not support it
/// together with `#[serde(flatten)]`).
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(
        default = "default_output_directory",
        skip_serializing_if = "is_default_output_directory"
    )]
    pub output_directory: String,
    #[serde(
        default = "default_pages_directory",
        skip_serializing_if = "is_default_pages_directory"
    )]
    pub pages_directory: String,
    #[serde(
        default = "default_layouts_directory",
        skip_serializing_if = "is_default_layouts_directory"
    )]
    pub layouts_directory: String,
    #[serde(
        default = "default_partials_directory",
        skip_serializing_if = "is_default_partials_directory"
    )]
    pub partials_directory: String,
    #[serde(
        default = "default_assets_directory",
        skip_serializing_if = "is_default_assets_directory"
    )]
    pub assets_directory: String,
    #[serde(
        default = "default_content_directory",
        skip_serializing_if = "is_default_content_directory"
    )]
    pub content_directory: String,
    #[serde(
        default = "default_tags_directory",
        skip_serializing_if = "is_default_tags_directory"
    )]
    pub tags_directory: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown: Option<MarkdownConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global: Option<HashMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<Hooks>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundler: Option<Bundler>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sitemap: Option<SitemapConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub robots: Option<RobotsConfig>,
    #[serde(default, skip_serializing_if = "is_default_url_style")]
    pub url_style: UrlStyle,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub listings: HashMap<String, ListingConfig>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub taxonomies: HashMap<String, TaxonomyConfig>,
    pub feed: Option<FeedConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            output_directory: default_output_directory(),
            pages_directory: default_pages_directory(),
            layouts_directory: default_layouts_directory(),
            partials_directory: default_partials_directory(),
            assets_directory: default_assets_directory(),
            content_directory: default_content_directory(),
            tags_directory: default_tags_directory(),
            markdown: None,
            global: None,
            hooks: None,
            bundler: None,
            base_url: None,
            sitemap: None,
            robots: None,
            url_style: UrlStyle::default(),
            listings: HashMap::new(),
            taxonomies: HashMap::new(),
            feed: None,
        }
    }
}

impl Config {
    /// Reads and parses the `balzac.toml` of the project at `root`.
    pub fn load(root: &Path) -> crate::error::Result<Self> {
        let path = root.join("balzac.toml");
        let content = crate::error::read_to_string(&path)?;
        Self::from_toml(&content).map_err(|e| crate::error::Error::Config {
            path,
            message: e.to_string(),
        })
    }

    #[must_use]
    pub fn resolve(&self, root: &std::path::Path) -> ResolvedConfig {
        ResolvedConfig {
            root_directory: root.to_path_buf(),
            output_directory: Self::resolve_path(&self.output_directory, root),
            pages_directory: Self::resolve_path(&self.pages_directory, root),
            layouts_directory: Self::resolve_path(&self.layouts_directory, root),
            partials_directory: Self::resolve_path(&self.partials_directory, root),
            assets_directory: Self::resolve_path(&self.assets_directory, root),
            content_directory: Self::resolve_path(&self.content_directory, root),
            tags_directory: Self::resolve_path(&self.tags_directory, root),
            markdown: self.markdown.clone().unwrap_or_default(),
            global: self.global.clone(),
            hooks: self.hooks.clone(),
            bundler: self.bundler.clone(),
            base_url: self.base_url.clone(),
            sitemap: self.sitemap.clone(),
            robots: self.robots.clone(),
            url_style: self.url_style,
            listings: self.listings.clone(),
            taxonomies: self.taxonomies.clone(),
            feed: self.feed.clone(),
            dev_mode: false,
        }
    }

    fn resolve_path(path: &str, root: &std::path::Path) -> std::path::PathBuf {
        let p = std::path::PathBuf::from(path);
        if p.is_absolute() { p } else { root.join(p) }
    }
}

#[derive(Clone)]
pub struct ResolvedConfig {
    pub root_directory: std::path::PathBuf,
    pub output_directory: std::path::PathBuf,
    pub pages_directory: std::path::PathBuf,
    pub layouts_directory: std::path::PathBuf,
    pub partials_directory: std::path::PathBuf,
    pub assets_directory: std::path::PathBuf,
    pub content_directory: std::path::PathBuf,
    pub tags_directory: std::path::PathBuf,
    pub markdown: MarkdownConfig,
    pub global: Option<std::collections::HashMap<String, serde_json::Value>>,
    pub hooks: Option<Hooks>,
    pub bundler: Option<Bundler>,
    pub base_url: Option<String>,
    pub sitemap: Option<SitemapConfig>,
    pub robots: Option<RobotsConfig>,
    pub url_style: UrlStyle,
    pub listings: HashMap<String, ListingConfig>,
    pub taxonomies: HashMap<String, TaxonomyConfig>,
    pub feed: Option<FeedConfig>,
    pub dev_mode: bool,
}

fn default_vite_manifest_path() -> String {
    "dist/.vite/manifest.json".to_string()
}

fn default_vite_dev_origin() -> String {
    "http://127.0.0.1:5173".to_string()
}

fn is_default_output_directory(s: &String) -> bool {
    s == &default_output_directory()
}

fn is_default_pages_directory(s: &String) -> bool {
    s == &default_pages_directory()
}

fn is_default_layouts_directory(s: &String) -> bool {
    s == &default_layouts_directory()
}

fn is_default_partials_directory(s: &String) -> bool {
    s == &default_partials_directory()
}

fn is_default_assets_directory(s: &String) -> bool {
    s == &default_assets_directory()
}

fn is_default_content_directory(s: &String) -> bool {
    s == &default_content_directory()
}

fn default_tags_directory() -> String {
    "./tags".to_string()
}

fn is_default_tags_directory(s: &String) -> bool {
    s == &default_tags_directory()
}

fn is_default_vite_dev_origin(s: &String) -> bool {
    s == &default_vite_dev_origin()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitFeature {
    Sitemap,
    Feed,
}

#[derive(Debug)]
pub enum CreateConfigError {
    AlreadyExists,
    Io(io::Error),
    Serialize(toml::ser::Error),
}

impl std::fmt::Display for CreateConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CreateConfigError::AlreadyExists => write!(f, "balzac.toml already exists"),
            CreateConfigError::Io(e) => write!(f, "IO error: {e}"),
            CreateConfigError::Serialize(e) => write!(f, "Serialization error: {e}"),
        }
    }
}

impl std::error::Error for CreateConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CreateConfigError::Io(e) => Some(e),
            CreateConfigError::Serialize(e) => Some(e),
            CreateConfigError::AlreadyExists => None,
        }
    }
}

impl From<io::Error> for CreateConfigError {
    fn from(err: io::Error) -> Self {
        CreateConfigError::Io(err)
    }
}

impl From<toml::ser::Error> for CreateConfigError {
    fn from(err: toml::ser::Error) -> Self {
        CreateConfigError::Serialize(err)
    }
}

impl Config {
    pub fn create(path: &Path, features: Option<&[InitFeature]>) -> Result<(), CreateConfigError> {
        let config_path = path.join("balzac.toml");

        if config_path.exists() {
            return Err(CreateConfigError::AlreadyExists);
        }

        let features = features.unwrap_or(&[]);

        let mut config = Config::default();

        if features.contains(&InitFeature::Sitemap) {
            config.base_url = Some("https://example.com".to_string());
            config.sitemap = Some(SitemapConfig::default());
        }

        if features.contains(&InitFeature::Feed) {
            config.base_url = Some("https://example.com".to_string());
            config.feed = Some(FeedConfig::default());
        }

        let config_content = toml::to_string_pretty(&config)?;
        fs::write(&config_path, config_content)?;

        Ok(())
    }
}

/// A `balzac.toml` that could not be parsed. Displays the TOML error with the
/// offending line, followed by a "did you mean" hint for misspelled keys.
#[derive(Debug)]
pub struct ConfigError {
    inner: Box<toml::de::Error>,
    suggestion: Option<String>,
}

impl ConfigError {
    /// Closest known key to the unknown key, if there is a plausible one.
    #[must_use]
    pub fn suggestion(&self) -> Option<&str> {
        self.suggestion.as_deref()
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner.to_string().trim_end())?;
        if let Some(suggestion) = &self.suggestion {
            write!(f, "\nhelp: did you mean `{suggestion}`?")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.inner.as_ref())
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(inner: toml::de::Error) -> Self {
        let suggestion = unknown_field(inner.message()).and_then(|(field, expected)| {
            expected
                .into_iter()
                .map(|candidate| (strsim::jaro_winkler(field, candidate), candidate))
                .filter(|(score, _)| *score > 0.8)
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, candidate)| candidate.to_string())
        });
        Self {
            inner: Box::new(inner),
            suggestion,
        }
    }
}

/// Splits serde's `unknown field` message into the unknown key and the keys
/// allowed at that position.
fn unknown_field(message: &str) -> Option<(&str, Vec<&str>)> {
    let rest = message.strip_prefix("unknown field `")?;
    let (field, expected) = rest.split_once('`')?;
    let expected = expected.split('`').skip(1).step_by(2).collect();
    Some((field, expected))
}

impl Config {
    /// Parses the contents of a `balzac.toml`, rejecting unknown keys.
    pub fn from_toml(content: &str) -> Result<Self, ConfigError> {
        Ok(toml::from_str(content)?)
    }
}

fn default_output_directory() -> String {
    "./dist".to_string()
}

fn default_pages_directory() -> String {
    "./pages".to_string()
}

fn default_layouts_directory() -> String {
    "./layouts".to_string()
}

fn default_partials_directory() -> String {
    "./partials".to_string()
}

fn default_assets_directory() -> String {
    "./assets".to_string()
}

fn default_content_directory() -> String {
    "./content".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default_output_directory() {
        let config = Config {
            output_directory: "./dist".to_string(),
            pages_directory: "./pages".to_string(),
            layouts_directory: "./layouts".to_string(),
            partials_directory: "./partials".to_string(),
            assets_directory: "./assets".to_string(),
            content_directory: "./content".to_string(),
            tags_directory: "./tags".to_string(),
            markdown: None,
            global: None,
            hooks: None,
            bundler: None,
            base_url: None,
            sitemap: None,
            robots: None,
            url_style: UrlStyle::default(),
            listings: HashMap::new(),
            taxonomies: HashMap::new(),
            feed: None,
        };
        assert_eq!(config.output_directory, "./dist");
    }

    #[test]
    fn test_config_default_pages_directory() {
        let config = Config {
            output_directory: "./dist".to_string(),
            pages_directory: "./pages".to_string(),
            layouts_directory: "./layouts".to_string(),
            partials_directory: "./partials".to_string(),
            assets_directory: "./assets".to_string(),
            content_directory: "./content".to_string(),
            tags_directory: "./tags".to_string(),
            markdown: None,
            global: None,
            hooks: None,
            bundler: None,
            base_url: None,
            sitemap: None,
            robots: None,
            url_style: UrlStyle::default(),
            listings: HashMap::new(),
            taxonomies: HashMap::new(),
            feed: None,
        };
        assert_eq!(config.pages_directory, "./pages");
    }

    #[test]
    fn test_config_with_global_data() {
        let mut global = HashMap::new();
        global.insert("site_name".to_string(), serde_json::json!("My Site"));
        global.insert("author".to_string(), serde_json::json!("John Doe"));

        let config = Config {
            output_directory: "./dist".to_string(),
            pages_directory: "./pages".to_string(),
            layouts_directory: "./layouts".to_string(),
            partials_directory: "./partials".to_string(),
            assets_directory: "./assets".to_string(),
            content_directory: "./content".to_string(),
            tags_directory: "./tags".to_string(),
            markdown: None,
            global: Some(global),
            hooks: None,
            bundler: None,
            base_url: None,
            sitemap: None,
            robots: None,
            url_style: UrlStyle::default(),
            listings: HashMap::new(),
            taxonomies: HashMap::new(),
            feed: None,
        };

        assert!(config.global.is_some());
        let global_data = config.global.unwrap();
        assert_eq!(
            global_data.get("site_name"),
            Some(&serde_json::json!("My Site"))
        );
        assert_eq!(
            global_data.get("author"),
            Some(&serde_json::json!("John Doe"))
        );
    }

    #[test]
    fn test_config_custom_directories() {
        let config = Config {
            output_directory: "./build".to_string(),
            pages_directory: "./src/pages".to_string(),
            layouts_directory: "./custom/layouts".to_string(),
            partials_directory: "./custom/partials".to_string(),
            assets_directory: "./assets".to_string(),
            content_directory: "./content".to_string(),
            tags_directory: "./tags".to_string(),
            markdown: None,
            global: None,
            hooks: None,
            bundler: None,
            base_url: None,
            sitemap: None,
            robots: None,
            url_style: UrlStyle::default(),
            listings: HashMap::new(),
            taxonomies: HashMap::new(),
            feed: None,
        };

        assert_eq!(config.output_directory, "./build");
        assert_eq!(config.pages_directory, "./src/pages");
        assert_eq!(config.layouts_directory, "./custom/layouts");
        assert_eq!(config.partials_directory, "./custom/partials");
    }

    #[test]
    fn test_config_with_sitemap() {
        let config = Config {
            output_directory: "./dist".to_string(),
            pages_directory: "./pages".to_string(),
            layouts_directory: "./layouts".to_string(),
            partials_directory: "./partials".to_string(),
            assets_directory: "./assets".to_string(),
            content_directory: "./content".to_string(),
            tags_directory: "./tags".to_string(),
            markdown: None,
            global: None,
            hooks: None,
            bundler: None,
            base_url: Some("https://example.com".to_string()),
            sitemap: Some(SitemapConfig {
                enabled: true,
                filename: "sitemap.xml".to_string(),
                default_priority: Some(0.5),
                default_changefreq: Some("weekly".to_string()),
            }),
            robots: None,
            url_style: UrlStyle::default(),
            listings: HashMap::new(),
            taxonomies: HashMap::new(),
            feed: None,
        };

        assert_eq!(config.base_url, Some("https://example.com".to_string()));
        assert!(config.sitemap.is_some());
        let sitemap = config.sitemap.unwrap();
        assert!(sitemap.enabled);
        assert_eq!(sitemap.default_priority, Some(0.5));
    }

    #[test]
    fn test_config_create_without_features() {
        let temp_dir = std::env::temp_dir().join("balzac_test_create_no_features");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let result = Config::create(&temp_dir, None);
        assert!(result.is_ok());

        let config_content = fs::read_to_string(temp_dir.join("balzac.toml")).unwrap();
        let parsed: Config = toml::from_str(&config_content).unwrap();

        assert_eq!(parsed.output_directory, "./dist");
        assert!(parsed.sitemap.is_none());
        assert!(parsed.base_url.is_none());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_config_create_with_sitemap_feature() {
        let temp_dir = std::env::temp_dir().join("balzac_test_create_sitemap");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let result = Config::create(&temp_dir, Some(&[InitFeature::Sitemap]));
        assert!(result.is_ok());

        let config_content = fs::read_to_string(temp_dir.join("balzac.toml")).unwrap();
        let parsed: Config = toml::from_str(&config_content).unwrap();

        assert_eq!(parsed.base_url, Some("https://example.com".to_string()));
        assert!(parsed.sitemap.is_some());
        let sitemap = parsed.sitemap.unwrap();
        assert!(sitemap.enabled);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_config_create_with_feed_feature() {
        let temp_dir = std::env::temp_dir().join("balzac_test_create_feed");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        Config::create(&temp_dir, Some(&[InitFeature::Feed])).unwrap();

        let config_content = fs::read_to_string(temp_dir.join("balzac.toml")).unwrap();
        let parsed: Config = toml::from_str(&config_content).unwrap();

        assert_eq!(parsed.base_url, Some("https://example.com".to_string()));
        assert!(parsed.sitemap.is_none());
        let feed = parsed.feed.unwrap();
        assert!(feed.enabled);
        assert_eq!(feed.format, FeedFormat::Atom);
        assert_eq!(feed.limit, 20);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_feed_config_parsing() {
        let parsed: Config = toml::from_str(
            r#"
            base_url = "https://example.com"

            [feed]
            format = "both"
            collections = ["blog"]
            limit = 5
            full_content = true
            combined = true
            title = "My Site"

            [feed.overrides.blog]
            title = "My Blog"
            "#,
        )
        .unwrap();

        let feed = parsed.feed.unwrap();
        assert!(feed.enabled);
        assert_eq!(feed.format, FeedFormat::Both);
        assert_eq!(feed.collections, Some(vec!["blog".to_string()]));
        assert_eq!(feed.limit, 5);
        assert!(feed.full_content);
        assert_eq!(
            feed.combined.and_then(|c| c.resolve()),
            Some(CombinedFeedConfig::default())
        );
        assert_eq!(feed.title.as_deref(), Some("My Site"));
        assert_eq!(feed.overrides["blog"].title.as_deref(), Some("My Blog"));
        assert!(feed.overrides["blog"].description.is_none());
    }

    #[test]
    fn test_combined_feed_setting_forms() {
        let combined = |toml_str: &str| {
            toml::from_str::<Config>(toml_str)
                .unwrap()
                .feed
                .unwrap()
                .combined
                .and_then(|c| c.resolve())
        };

        assert_eq!(combined("[feed]\n"), None);
        assert_eq!(combined("[feed]\ncombined = false\n"), None);
        assert_eq!(
            combined("[feed]\ncombined = [\"blog\", \"notes\"]\n"),
            Some(CombinedFeedConfig {
                collections: Some(vec!["blog".to_string(), "notes".to_string()]),
                ..Default::default()
            })
        );
        assert_eq!(
            combined(
                "[feed]\n[feed.combined]\ncollections = [\"blog\"]\ntitle = \"All\"\ndescription = \"Everything\"\nlimit = 50\n"
            ),
            Some(CombinedFeedConfig {
                collections: Some(vec!["blog".to_string()]),
                title: Some("All".to_string()),
                description: Some("Everything".to_string()),
                limit: Some(50),
            })
        );
    }

    #[test]
    fn test_combined_feed_rejects_unknown_keys() {
        let parsed: Result<Config, _> =
            toml::from_str("[feed]\n[feed.combined]\ncolections = [\"blog\"]\n");
        assert!(parsed.is_err());
    }

    #[test]
    fn test_feed_config_rejects_unknown_format() {
        let parsed: Result<Config, _> = toml::from_str("[feed]\nformat = \"json\"\n");
        assert!(parsed.is_err());
    }

    #[test]
    fn test_config_create_already_exists() {
        let temp_dir = std::env::temp_dir().join("balzac_test_create_exists");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        fs::write(temp_dir.join("balzac.toml"), "existing config").unwrap();

        let result = Config::create(&temp_dir, None);
        assert!(matches!(result, Err(CreateConfigError::AlreadyExists)));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = Config {
            output_directory: "./build".to_string(),
            pages_directory: "./src/pages".to_string(),
            base_url: Some("https://example.com".to_string()),
            sitemap: Some(SitemapConfig {
                enabled: true,
                filename: "sitemap.xml".to_string(),
                default_priority: Some(0.8),
                default_changefreq: None,
            }),
            ..Default::default()
        };

        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.output_directory, "./build");
        assert_eq!(deserialized.pages_directory, "./src/pages");
        assert_eq!(
            deserialized.base_url,
            Some("https://example.com".to_string())
        );
        assert!(deserialized.sitemap.is_some());
        let sitemap = deserialized.sitemap.unwrap();
        assert!(sitemap.enabled);
        assert_eq!(sitemap.default_priority, Some(0.8));
    }

    #[test]
    fn test_markdown_config_defaults() {
        let parsed: Config = toml::from_str("").unwrap();
        assert!(parsed.markdown.is_none());

        let resolved = parsed.resolve(Path::new("."));
        assert!(resolved.markdown.syntax_highlighting);
        assert!(resolved.markdown.highlight_theme.is_none());
        assert!(!resolved.markdown.heading_ids);
        assert!(!resolved.markdown.heading_anchors);
        assert!(!resolved.markdown.footnotes);
    }

    #[test]
    fn test_markdown_config_partial_section() {
        let parsed: Config =
            toml::from_str("[markdown]\nfootnotes = true\nsyntax_highlighting = false\n").unwrap();
        let markdown = parsed.markdown.unwrap();
        assert!(markdown.footnotes);
        assert!(!markdown.syntax_highlighting);
        assert!(!markdown.heading_ids);
    }

    fn parse_error(content: &str) -> ConfigError {
        match Config::from_toml(content) {
            Ok(_) => panic!("expected {content:?} to be rejected"),
            Err(e) => e,
        }
    }

    #[test]
    fn test_unknown_top_level_key_is_rejected() {
        let error = parse_error("ouput_directory = \"./build\"\n");
        assert_eq!(error.suggestion(), Some("output_directory"));

        let message = error.to_string();
        assert!(message.contains("line 1"), "{message}");
        assert!(
            message.contains("unknown field `ouput_directory`"),
            "{message}"
        );
        assert!(
            message.ends_with("help: did you mean `output_directory`?"),
            "{message}"
        );
    }

    #[test]
    fn test_unknown_table_is_rejected() {
        let error = parse_error("base_url = \"https://example.com\"\n\n[sitemp]\nenabled = true\n");
        assert_eq!(error.suggestion(), Some("sitemap"));
        assert!(error.to_string().contains("3 | [sitemp]"), "{error}");
    }

    #[test]
    fn test_unknown_nested_keys_are_rejected() {
        for (content, suggestion) in [
            ("[hooks]\nbuild_befor = \"true\"\n", Some("build_before")),
            ("[bundler]\nvit = {}\n", Some("vite")),
            ("[bundler.vite]\nenable = true\n", Some("enabled")),
            ("[sitemap]\nfile_name = \"map.xml\"\n", Some("filename")),
            ("[robots]\ndisalow = []\n", Some("disallow")),
            ("[markdown]\nfootnote = true\n", Some("footnotes")),
            ("[listings.posts]\nper_pag = 3\n", Some("per_page")),
            ("[taxonomies.tags]\ncollection = []\n", Some("collections")),
            ("[feed]\nlimt = 3\n", Some("limit")),
            ("[feed.overrides.blog]\ntitel = \"Blog\"\n", Some("title")),
            ("[feed.combined]\ncolections = []\n", Some("collections")),
            ("[hooks]\nzzz = \"true\"\n", None),
        ] {
            let error = parse_error(content);
            assert_eq!(error.suggestion(), suggestion, "{content:?}: {error}");
            assert!(error.to_string().contains("line 2"), "{error}");
        }
    }

    #[test]
    fn test_load_rejects_unknown_keys_with_a_suggestion() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("balzac.toml"), "[sitemp]\n").unwrap();
        let error = Config::load(dir.path()).err().unwrap();
        let crate::error::Error::Config { path, message } = &error else {
            panic!("expected a config error, got {error}");
        };
        assert_eq!(path, &dir.path().join("balzac.toml"));
        assert!(message.contains("1 | [sitemp]"), "{message}");
        assert!(
            message.ends_with("help: did you mean `sitemap`?"),
            "{message}"
        );
    }

    #[test]
    fn test_global_stays_free_form() {
        let config =
            Config::from_toml("[global]\nsite_name = \"Site\"\n[global.anything]\ngoes = [1, 2]\n")
                .unwrap();
        let global = config.global.unwrap();
        assert_eq!(global["anything"], serde_json::json!({ "goes": [1, 2] }));
    }

    const PROBE: &str = "__balzac_probe__";

    /// Configs putting an unknown probe key in the table at dotted `path`
    /// (`*` stands for any map key), as a table and as an array of tables.
    fn probe_documents(path: &str) -> Vec<String> {
        let header = path.replace('*', "any");
        if path.is_empty() {
            vec![format!("{PROBE} = 0")]
        } else {
            vec![
                format!("[{header}]\n{PROBE} = 0"),
                format!("[[{header}]]\n{PROBE} = 0"),
            ]
        }
    }

    /// Keys accepted by the table at dotted `path`, found by feeding it an
    /// unknown key. `None` when the value there is not a strict table.
    fn probe_table(path: &str) -> Option<Vec<String>> {
        probe_documents(path).iter().find_map(|content| {
            let error = Config::from_toml(content).err()?;
            let (field, expected) = unknown_field(error.inner.message())?;
            (field == PROBE).then(|| expected.into_iter().map(String::from).collect())
        })
    }

    /// Every key path of the config schema, discovered through the
    /// `deny_unknown_fields` errors, so new nested structs are covered
    /// automatically. Map keys (collection names, ...) appear as `*`.
    fn schema_paths(path: &str, paths: &mut std::collections::BTreeSet<String>) {
        let Some(fields) = probe_table(path) else {
            return;
        };
        for field in fields {
            let child = if path.is_empty() {
                field
            } else {
                format!("{path}.{field}")
            };
            paths.insert(child.clone());
            if probe_table(&child).is_some() {
                schema_paths(&child, paths);
            } else if probe_table(&format!("{child}.*")).is_some() {
                paths.insert(format!("{child}.*"));
                schema_paths(&format!("{child}.*"), paths);
            }
        }
    }

    #[test]
    fn test_only_global_accepts_unknown_keys() {
        let mut schema = std::collections::BTreeSet::from([String::new()]);
        schema_paths("", &mut schema);
        // a lenient struct is a leaf to `schema_paths`, so also probe map values
        let candidates: std::collections::BTreeSet<String> = schema
            .iter()
            .flat_map(|path| {
                if path.is_empty() {
                    vec![String::new()]
                } else {
                    vec![path.clone(), format!("{path}.*")]
                }
            })
            .collect();
        let lenient: Vec<_> = candidates
            .iter()
            .filter(|path| *path != "global" && !path.starts_with("global."))
            .filter(|path| {
                probe_documents(path)
                    .iter()
                    .any(|content| Config::from_toml(content).is_ok())
            })
            .collect();
        assert!(
            lenient.is_empty(),
            "config tables accepting unknown keys, add #[serde(deny_unknown_fields)]: {lenient:?}"
        );
    }

    /// Key paths of a README toml example, with map keys replaced by `*`.
    /// Free-form values (`global`) are not descended into.
    fn documented_paths(
        prefix: &str,
        table: &toml::Table,
        schema: &std::collections::BTreeSet<String>,
        paths: &mut std::collections::BTreeSet<String>,
    ) {
        for (key, value) in table {
            let join = |key: &str| {
                if prefix.is_empty() {
                    key.to_string()
                } else {
                    format!("{prefix}.{key}")
                }
            };
            let path = if schema.contains(&join(key)) {
                join(key)
            } else {
                join("*")
            };
            paths.insert(path.clone());
            let nested = match value {
                toml::Value::Table(table) => vec![table],
                toml::Value::Array(items) => {
                    items.iter().filter_map(toml::Value::as_table).collect()
                }
                _ => Vec::new(),
            };
            let child_prefix = format!("{path}.");
            if schema.iter().any(|p| p.starts_with(&child_prefix)) {
                for table in nested {
                    documented_paths(&path, table, schema, paths);
                }
            }
        }
    }

    #[test]
    fn test_readme_config_reference_matches_config() {
        let readme = include_str!("../README.md");
        let mut schema = std::collections::BTreeSet::new();
        schema_paths("", &mut schema);
        assert!(schema.contains("bundler.vite.dev_origin"), "{schema:?}");
        assert!(schema.contains("listings.*.per_page"), "{schema:?}");

        // top-level keys: the bullet list under "## Config Reference"
        let reference = readme
            .split("## Config Reference")
            .nth(1)
            .expect("README has a Config Reference section");
        let reference = &reference[..reference.find("\n#").unwrap_or(reference.len())];
        let listed: std::collections::BTreeSet<String> = reference
            .lines()
            .filter_map(|line| line.strip_prefix("- "))
            .filter_map(|line| line.split([' ', ':']).next())
            .map(String::from)
            .collect();
        let top_level: std::collections::BTreeSet<String> = schema
            .iter()
            .filter(|p| !p.contains('.'))
            .cloned()
            .collect();
        assert_eq!(
            listed, top_level,
            "README Config Reference is out of sync with Config"
        );

        // nested keys: the ```toml examples, which must also be valid configs
        let mut documented = std::collections::BTreeSet::new();
        for block in readme.split("```toml\n").skip(1) {
            let block = &block[..block.find("```").expect("closed toml block")];
            if let Err(e) = Config::from_toml(block) {
                panic!("README toml example is not a valid config:\n{block}\n{e}");
            }
            documented_paths(
                "",
                &toml::from_str(block).unwrap(),
                &schema,
                &mut documented,
            );
        }
        let undocumented: Vec<_> = schema
            .iter()
            .filter(|p| p.contains('.') && !documented.contains(*p))
            .collect();
        assert!(
            undocumented.is_empty(),
            "config keys missing from the README toml examples: {undocumented:?}"
        );
    }
}
