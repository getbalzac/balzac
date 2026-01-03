use std::fs;

use handlebars::Handlebars;

use crate::config;

pub trait Renderer {
    fn new(configuration: &config::Config) -> Self;
    fn init(&mut self, configuration: &config::Config);
    fn render(&self, template: String, data: serde_json::Value) -> String;
}

pub struct HandlebarsRenderer<'a> {
    pub registry: handlebars::Handlebars<'a>,
}

impl<'a> HandlebarsRenderer<'a> {
    fn register_partials(&mut self, configuration: &config::Config) {
        let partial_dir_exists = fs::exists(&configuration.partials_directory)
            .expect("Could not check if partial dir exists");
        if partial_dir_exists {
            for entry in fs::read_dir(&configuration.partials_directory)
                .expect("Could not read partials directory")
            {
                let dir = entry.expect("Could not get directory handler");
                log::info!("Parsing partial {}", dir.file_name().to_string_lossy());
                let partial_path = dir.path();
                let partial_content =
                    fs::read_to_string(&partial_path).expect("Cannot read partial file content");
                let partial_name = &partial_path.file_stem().expect("Could not get file stem");
                self.registry
                    .register_partial(&partial_name.to_string_lossy(), partial_content)
                    .expect("Cannot register partial");
            }
        } else {
            log::info!("Could not find partial directory, skipping register step");
        }
    }

    fn register_layouts(&mut self, configuration: &config::Config) {
        let layouts_dir_exists = fs::exists(&configuration.layouts_directory)
            .expect("Could not check if partial layouts dir exists");

        if layouts_dir_exists {
            for entry in fs::read_dir(&configuration.layouts_directory)
                .expect("Could not read layouts directory")
            {
                let dir = entry.expect("Could not get directory handler");
                log::info!("Parsing partial {}", dir.file_name().to_string_lossy());
                let partial_path = dir.path();
                let partial_content =
                    fs::read_to_string(&partial_path).expect("Cannot read layout file content");
                let partial_name = &partial_path.file_stem().expect("Could not get file stem");
                self.registry
                    .register_partial(&partial_name.to_string_lossy(), partial_content)
                    .expect("Cannot register layout");
            }
        } else {
            log::info!("Could not find layouts directory, skipping register step");
        }
    }
}

impl<'a> Renderer for HandlebarsRenderer<'a> {
    fn init(&mut self, configuration: &config::Config) {
        self.register_partials(configuration);
        self.register_layouts(configuration);
    }
    fn new(_configuration: &config::Config) -> HandlebarsRenderer<'a> {
        let reg = Handlebars::new();

        HandlebarsRenderer { registry: reg }
    }
    fn render(&self, template: String, data: serde_json::Value) -> String {
        self.registry
            .render_template(&template, &data)
            .expect("Could not render template")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_renderer() -> HandlebarsRenderer<'static> {
        let reg = Handlebars::new();
        HandlebarsRenderer { registry: reg }
    }

    #[test]
    fn test_render_simple_template() {
        let renderer = create_renderer();
        let template = "Hello, World!".to_string();
        let data = serde_json::json!({});

        let result = renderer.render(template, data);
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_render_template_with_variable() {
        let renderer = create_renderer();
        let template = "Hello, {{name}}!".to_string();
        let data = serde_json::json!({
            "name": "Alice"
        });

        let result = renderer.render(template, data);
        assert_eq!(result, "Hello, Alice!");
    }

    #[test]
    fn test_render_template_with_multiple_variables() {
        let renderer = create_renderer();
        let template = "{{greeting}} {{name}}, welcome to {{site}}!".to_string();
        let data = serde_json::json!({
            "greeting": "Hello",
            "name": "Bob",
            "site": "Balzac"
        });

        let result = renderer.render(template, data);
        assert_eq!(result, "Hello Bob, welcome to Balzac!");
    }

    #[test]
    fn test_render_template_with_nested_data() {
        let renderer = create_renderer();
        let template = "Author: {{author.name}} ({{author.email}})".to_string();
        let data = serde_json::json!({
            "author": {
                "name": "Jane Doe",
                "email": "jane@example.com"
            }
        });

        let result = renderer.render(template, data);
        assert_eq!(result, "Author: Jane Doe (jane@example.com)");
    }

    #[test]
    fn test_render_template_with_conditional() {
        let renderer = create_renderer();
        let template = "{{#if is_active}}Active{{else}}Inactive{{/if}}".to_string();
        let data = serde_json::json!({
            "is_active": true
        });

        let result = renderer.render(template, data);
        assert_eq!(result, "Active");
    }

    #[test]
    fn test_render_template_with_loop() {
        let renderer = create_renderer();
        let template = "{{#each items}}- {{this}}\n{{/each}}".to_string();
        let data = serde_json::json!({
            "items": ["Rust", "Handlebars", "SSG"]
        });

        let result = renderer.render(template, data);
        assert_eq!(result, "- Rust\n- Handlebars\n- SSG\n");
    }

    #[test]
    fn test_render_empty_template_with_data() {
        let renderer = create_renderer();
        let template = "".to_string();
        let data = serde_json::json!({
            "unused": "data"
        });

        let result = renderer.render(template, data);
        assert_eq!(result, "");
    }
}
