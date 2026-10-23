use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Deserialize, Serialize, Clone)]
pub struct Hooks {
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
pub struct Bundler {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vite: Option<ViteBundler>,
}

#[derive(Deserialize, Serialize)]
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
            global: None,
            hooks: None,
            bundler: None,
            base_url: None,
            sitemap: None,
            robots: None,
            url_style: UrlStyle::default(),
        }
    }
}

impl Config {
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
            global: self.global.clone(),
            hooks: self.hooks.clone(),
            bundler: self.bundler.clone(),
            base_url: self.base_url.clone(),
            sitemap: self.sitemap.clone(),
            robots: self.robots.clone(),
            url_style: self.url_style,
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
    pub global: Option<std::collections::HashMap<String, serde_json::Value>>,
    pub hooks: Option<Hooks>,
    pub bundler: Option<Bundler>,
    pub base_url: Option<String>,
    pub sitemap: Option<SitemapConfig>,
    pub robots: Option<RobotsConfig>,
    pub url_style: UrlStyle,
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

        let config_content = toml::to_string_pretty(&config)?;
        fs::write(&config_path, config_content)?;

        Ok(())
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
            global: None,
            hooks: None,
            bundler: None,
            base_url: None,
            sitemap: None,
            robots: None,
            url_style: UrlStyle::default(),
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
            global: None,
            hooks: None,
            bundler: None,
            base_url: None,
            sitemap: None,
            robots: None,
            url_style: UrlStyle::default(),
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
            global: Some(global),
            hooks: None,
            bundler: None,
            base_url: None,
            sitemap: None,
            robots: None,
            url_style: UrlStyle::default(),
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
            global: None,
            hooks: None,
            bundler: None,
            base_url: None,
            sitemap: None,
            robots: None,
            url_style: UrlStyle::default(),
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
            url_style: UrlStyle::default(),
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
}
