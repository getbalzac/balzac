use handlebars::Handlebars;

pub trait Renderer {
    fn render(&self, template: String, data: serde_json::Value) -> String;
}

pub struct HandlebarsRenderer {}

impl Renderer for HandlebarsRenderer {
    fn render(&self, template: String, data: serde_json::Value) -> String {
        let reg = Handlebars::new();

        reg.render_template(&template, &data)
            .expect("Could not render template")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_simple_template() {
        let renderer = HandlebarsRenderer {};
        let template = "Hello, World!".to_string();
        let data = serde_json::json!({});

        let result = renderer.render(template, data);
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_render_template_with_variable() {
        let renderer = HandlebarsRenderer {};
        let template = "Hello, {{name}}!".to_string();
        let data = serde_json::json!({
            "name": "Alice"
        });

        let result = renderer.render(template, data);
        assert_eq!(result, "Hello, Alice!");
    }

    #[test]
    fn test_render_template_with_multiple_variables() {
        let renderer = HandlebarsRenderer {};
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
        let renderer = HandlebarsRenderer {};
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
        let renderer = HandlebarsRenderer {};
        let template = "{{#if is_active}}Active{{else}}Inactive{{/if}}".to_string();
        let data = serde_json::json!({
            "is_active": true
        });

        let result = renderer.render(template, data);
        assert_eq!(result, "Active");
    }

    #[test]
    fn test_render_template_with_loop() {
        let renderer = HandlebarsRenderer {};
        let template = "{{#each items}}- {{this}}\n{{/each}}".to_string();
        let data = serde_json::json!({
            "items": ["Rust", "Handlebars", "SSG"]
        });

        let result = renderer.render(template, data);
        assert_eq!(result, "- Rust\n- Handlebars\n- SSG\n");
    }

    #[test]
    fn test_render_empty_template_with_data() {
        let renderer = HandlebarsRenderer {};
        let template = "".to_string();
        let data = serde_json::json!({
            "unused": "data"
        });

        let result = renderer.render(template, data);
        assert_eq!(result, "");
    }
}
