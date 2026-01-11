use std::collections::HashMap;

use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct Hooks {
    #[serde(default)]
    pub build_before: Option<String>,
    #[serde(default)]
    pub build_after: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct ViteBundler {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_vite_manifest_path")]
    pub manifest_path: String,
}

#[derive(Deserialize, Clone)]
pub struct Bundler {
    #[serde(default)]
    pub vite: Option<ViteBundler>,
}

#[derive(Deserialize)]
pub struct Config {
    #[serde(default = "default_output_directory")]
    pub output_directory: String,
    #[serde(default = "default_pages_directory")]
    pub pages_directory: String,
    #[serde(default = "default_layouts_directory")]
    pub layouts_directory: String,
    #[serde(default = "default_partials_directory")]
    pub partials_directory: String,
    #[serde(default = "default_assets_directory")]
    pub assets_directory: String,
    #[serde(default = "default_content_directory")]
    pub content_directory: String,
    #[serde(default)]
    pub global: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub hooks: Option<Hooks>,
    #[serde(default)]
    pub bundler: Option<Bundler>,
}

impl Config {
    pub fn resolve(&self, root: &std::path::Path) -> ResolvedConfig {
        ResolvedConfig {
            output_directory: self.resolve_path(&self.output_directory, root),
            pages_directory: self.resolve_path(&self.pages_directory, root),
            layouts_directory: self.resolve_path(&self.layouts_directory, root),
            partials_directory: self.resolve_path(&self.partials_directory, root),
            assets_directory: self.resolve_path(&self.assets_directory, root),
            content_directory: self.resolve_path(&self.content_directory, root),
            global: self.global.clone(),
            hooks: self.hooks.clone(),
            bundler: self.bundler.clone(),
        }
    }

    fn resolve_path(&self, path: &str, root: &std::path::Path) -> std::path::PathBuf {
        let p = std::path::PathBuf::from(path);
        if p.is_absolute() { p } else { root.join(p) }
    }
}

pub struct ResolvedConfig {
    pub output_directory: std::path::PathBuf,
    pub pages_directory: std::path::PathBuf,
    pub layouts_directory: std::path::PathBuf,
    pub partials_directory: std::path::PathBuf,
    pub assets_directory: std::path::PathBuf,
    pub content_directory: std::path::PathBuf,
    pub global: Option<std::collections::HashMap<String, serde_json::Value>>,
    pub hooks: Option<Hooks>,
    pub bundler: Option<Bundler>,
}

fn default_vite_manifest_path() -> String {
    "dist/.vite/manifest.json".to_string()
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
            global: None,
            hooks: None,
            bundler: None,
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
            global: None,
            hooks: None,
            bundler: None,
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
            global: Some(global),
            hooks: None,
            bundler: None,
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
            global: None,
            hooks: None,
            bundler: None,
        };

        assert_eq!(config.output_directory, "./build");
        assert_eq!(config.pages_directory, "./src/pages");
        assert_eq!(config.layouts_directory, "./custom/layouts");
        assert_eq!(config.partials_directory, "./custom/partials");
    }
}
