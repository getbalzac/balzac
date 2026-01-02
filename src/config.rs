use std::collections::HashMap;

use facet::Facet;

#[derive(Facet)]
pub struct Config {
    #[facet(default="./dist".to_string())]
    pub output_directory: String,
    #[facet(default="./pages".to_string())]
    pub pages_directory: String,
    pub global: Option<HashMap<String, String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default_output_directory() {
        let config = Config {
            output_directory: "./dist".to_string(),
            pages_directory: "./pages".to_string(),
            global: None,
        };
        assert_eq!(config.output_directory, "./dist");
    }

    #[test]
    fn test_config_default_pages_directory() {
        let config = Config {
            output_directory: "./dist".to_string(),
            pages_directory: "./pages".to_string(),
            global: None,
        };
        assert_eq!(config.pages_directory, "./pages");
    }

    #[test]
    fn test_config_with_global_data() {
        let mut global = HashMap::new();
        global.insert("site_name".to_string(), "My Site".to_string());
        global.insert("author".to_string(), "John Doe".to_string());

        let config = Config {
            output_directory: "./dist".to_string(),
            pages_directory: "./pages".to_string(),
            global: Some(global),
        };

        assert!(config.global.is_some());
        let global_data = config.global.unwrap();
        assert_eq!(global_data.get("site_name"), Some(&"My Site".to_string()));
        assert_eq!(global_data.get("author"), Some(&"John Doe".to_string()));
    }

    #[test]
    fn test_config_custom_directories() {
        let config = Config {
            output_directory: "./build".to_string(),
            pages_directory: "./src/pages".to_string(),
            global: None,
        };

        assert_eq!(config.output_directory, "./build");
        assert_eq!(config.pages_directory, "./src/pages");
    }
}
