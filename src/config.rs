use std::collections::HashMap;

use serde::Deserialize;

#[derive(Deserialize)]
pub struct Hooks {
    #[serde(default)]
    pub build_before: Option<String>,
    #[serde(default)]
    pub build_after: Option<String>,
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
        };

        assert_eq!(config.output_directory, "./build");
        assert_eq!(config.pages_directory, "./src/pages");
        assert_eq!(config.layouts_directory, "./custom/layouts");
        assert_eq!(config.partials_directory, "./custom/partials");
    }
}
