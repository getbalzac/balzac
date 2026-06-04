use std::collections::HashMap;
use std::fs;

use comrak::nodes::{AstNode, ListType, NodeValue};
use comrak::{Arena, Options, format_html, parse_document};
use handlebars::Handlebars;

use crate::{
    config,
    vite::{ViteAssetSource, parse_manifest, vite_url},
};

pub trait Renderer {
    fn new(configuration: &config::ResolvedConfig) -> Self;
    fn init(&mut self, configuration: &config::ResolvedConfig);
    fn render(&self, template: String, data: serde_json::Value) -> String;
    fn render_markdown(&self, body: &str) -> std::io::Result<String>;
}

pub struct HandlebarsRenderer<'a> {
    pub registry: handlebars::Handlebars<'a>,
    pub tag_templates: HashMap<String, String>,
}

impl<'a> HandlebarsRenderer<'a> {
    fn register_partials(&mut self, configuration: &config::ResolvedConfig) {
        let partial_dir_exists = fs::exists(&configuration.partials_directory)
            .expect("Could not check if partial dir exists");
        if partial_dir_exists {
            for entry in fs::read_dir(&configuration.partials_directory)
                .expect("Could not read partials directory")
            {
                let dir = entry.expect("Could not get directory handler");
                log::debug!("Parsing partial {}", dir.file_name().to_string_lossy());
                let partial_path = dir.path();
                let partial_content =
                    fs::read_to_string(&partial_path).expect("Cannot read partial file content");
                let partial_name = &partial_path.file_stem().expect("Could not get file stem");
                self.registry
                    .register_partial(&partial_name.to_string_lossy(), partial_content)
                    .expect("Cannot register partial");
            }
        } else {
            log::debug!("Could not find partial directory, skipping register step");
        }
    }

    fn register_layouts(&mut self, configuration: &config::ResolvedConfig) {
        let layouts_dir_exists = fs::exists(&configuration.layouts_directory)
            .expect("Could not check if partial layouts dir exists");

        if layouts_dir_exists {
            for entry in fs::read_dir(&configuration.layouts_directory)
                .expect("Could not read layouts directory")
            {
                let dir = entry.expect("Could not get directory handler");
                log::debug!("Parsing partial {}", dir.file_name().to_string_lossy());
                let partial_path = dir.path();
                let partial_content =
                    fs::read_to_string(&partial_path).expect("Cannot read layout file content");
                let partial_name = &partial_path.file_stem().expect("Could not get file stem");
                self.registry
                    .register_partial(&partial_name.to_string_lossy(), partial_content)
                    .expect("Cannot register layout");
            }
        } else {
            log::debug!("Could not find layouts directory, skipping register step");
        }
    }

    fn register_tag_templates(&mut self, configuration: &config::ResolvedConfig) {
        let tags_dir_exists =
            fs::exists(&configuration.tags_directory).expect("Could not check if tags dir exists");

        if tags_dir_exists {
            for entry in
                fs::read_dir(&configuration.tags_directory).expect("Could not read tags directory")
            {
                let dir = entry.expect("Could not get directory handler");
                let path = dir.path();
                let extension = path.extension().map(|e| e.to_string_lossy().to_string());
                if extension.as_deref() != Some("hbs") {
                    log::debug!(
                        "Skipping non-hbs file {} in tags directory",
                        dir.file_name().to_string_lossy()
                    );
                    continue;
                }
                let template_content =
                    fs::read_to_string(&path).expect("Cannot read tag template file content");
                let tag_name = path
                    .file_stem()
                    .expect("Could not get file stem")
                    .to_string_lossy()
                    .to_string();
                log::debug!("Registering tag template for <{}>", tag_name);
                self.tag_templates.insert(tag_name, template_content);
            }
        } else {
            log::debug!("Could not find tags directory, skipping tag templates");
        }
    }

    pub fn register_helpers(&mut self, configuration: &config::ResolvedConfig) {
        if let Some(bundler) = &configuration.bundler
            && let Some(vite) = &bundler.vite
            && vite.enabled
        {
            let asset_source = if configuration.dev_mode {
                ViteAssetSource::DevOrigin(vite.dev_origin.clone())
            } else {
                let manifest_path = std::path::PathBuf::from(&vite.manifest_path);
                let manifest = parse_manifest(manifest_path, &configuration.root_directory)
                    .unwrap_or_else(|e| {
                        log::error!("{}", e);
                        std::process::exit(1);
                    });
                ViteAssetSource::Manifest(manifest)
            };
            let helper = vite_url { asset_source };
            self.registry.register_helper("vite_url", Box::new(helper));
        }
    }

    fn get_tag_name(&self, value: &NodeValue) -> Option<String> {
        match value {
            NodeValue::Link(_) => Some("a".to_string()),
            NodeValue::Image(_) => Some("img".to_string()),
            NodeValue::Heading(h) => Some(format!("h{}", h.level)),
            NodeValue::Paragraph => Some("p".to_string()),
            NodeValue::BlockQuote => Some("blockquote".to_string()),
            NodeValue::List(l) => match l.list_type {
                ListType::Bullet => Some("ul".to_string()),
                ListType::Ordered => Some("ol".to_string()),
            },
            NodeValue::Item(_) => Some("li".to_string()),
            NodeValue::Emph => Some("em".to_string()),
            NodeValue::Strong => Some("strong".to_string()),
            NodeValue::Strikethrough => Some("del".to_string()),
            NodeValue::Superscript => Some("sup".to_string()),
            NodeValue::Underline => Some("u".to_string()),
            NodeValue::LineBreak => Some("br".to_string()),
            NodeValue::ThematicBreak => Some("hr".to_string()),
            NodeValue::Code(_) => Some("code".to_string()),
            NodeValue::CodeBlock(_) => Some("pre".to_string()),
            NodeValue::Table(_) => Some("table".to_string()),
            NodeValue::TableRow(_) => Some("tr".to_string()),
            NodeValue::TableCell => Some("td".to_string()),
            NodeValue::DescriptionList => Some("dl".to_string()),
            NodeValue::DescriptionTerm => Some("dt".to_string()),
            NodeValue::DescriptionItem(_) | NodeValue::DescriptionDetails => Some("dd".to_string()),
            _ => None,
        }
    }

    fn build_props(&self, node: &AstNode) -> serde_json::Value {
        let data = node.data.borrow();
        match &data.value {
            NodeValue::Link(link) => serde_json::json!({
                "href": link.url,
                "title": link.title,
            }),
            NodeValue::Image(img) => serde_json::json!({
                "src": img.url,
                "title": img.title,
            }),
            NodeValue::Heading(h) => serde_json::json!({
                "level": h.level,
                "setext": h.setext,
            }),
            NodeValue::Code(c) => serde_json::json!({
                "num_backticks": c.num_backticks,
                "literal": c.literal,
            }),
            NodeValue::CodeBlock(c) => serde_json::json!({
                "info": c.info,
                "literal": c.literal,
                "fenced": c.fenced,
                "fence_char": (c.fence_char as char).to_string(),
                "fence_length": c.fence_length,
            }),
            NodeValue::List(l) => serde_json::json!({
                "start": l.start,
                "tight": l.tight,
                "delimiter": format!("{:?}", l.delimiter),
            }),
            _ => serde_json::json!({}),
        }
    }

    fn render_children_to_html<'b>(
        &self,
        node: &'b AstNode<'b>,
        arena: &'b Arena<AstNode<'b>>,
        options: &Options,
    ) -> std::io::Result<String> {
        let temp_root = arena.alloc(AstNode::from(NodeValue::Document));

        let mut child = node.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            temp_root.append(c);
        }

        let mut buf = Vec::new();
        format_html(temp_root, options, &mut buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    fn apply_tag_templates<'b>(
        &self,
        node: &'b AstNode<'b>,
        arena: &'b Arena<AstNode<'b>>,
        options: &Options,
    ) -> std::io::Result<()> {
        let mut child = node.first_child();
        while let Some(c) = child {
            self.apply_tag_templates(c, arena, options)?;
            child = c.next_sibling();
        }

        let tag_name = {
            let data = node.data.borrow();
            self.get_tag_name(&data.value)
        };

        if let Some(tag_name) = tag_name
            && let Some(template) = self.tag_templates.get(&tag_name)
        {
            let content = {
                let data = node.data.borrow();
                match &data.value {
                    NodeValue::Code(c) => c.literal.clone(),
                    NodeValue::CodeBlock(c) => c.literal.clone(),
                    _ => self.render_children_to_html(node, arena, options)?,
                }
            };
            let props = self.build_props(node);

            let template_data = serde_json::json!({
                "props": props,
                "content": content,
            });

            let rendered = self
                .registry
                .render_template(template, &template_data)
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Tag template render error for <{}>: {}", tag_name, e),
                    )
                })?;

            // Detach remaining children (they were already moved to temp_root during
            // render_children_to_html, but in case there were none or the node had no
            // children, ensure it's clean).
            let mut child_to_detach = node.first_child();
            while let Some(c) = child_to_detach {
                child_to_detach = c.next_sibling();
                c.detach();
            }

            node.data.borrow_mut().value = NodeValue::HtmlInline(rendered);
        }

        Ok(())
    }
}

impl<'a> Renderer for HandlebarsRenderer<'a> {
    fn init(&mut self, configuration: &config::ResolvedConfig) {
        self.register_partials(configuration);
        self.register_layouts(configuration);
        self.register_tag_templates(configuration);
        self.register_helpers(configuration);
    }
    fn new(_configuration: &config::ResolvedConfig) -> HandlebarsRenderer<'a> {
        let reg = Handlebars::new();

        HandlebarsRenderer {
            registry: reg,
            tag_templates: HashMap::new(),
        }
    }
    fn render(&self, template: String, data: serde_json::Value) -> String {
        self.registry
            .render_template(&template, &data)
            .expect("Could not render template")
    }

    fn render_markdown(&self, body: &str) -> std::io::Result<String> {
        let arena = Arena::new();
        let options = crate::collection::build_comrak_options();
        let root = parse_document(&arena, body, &options);
        self.apply_tag_templates(root, &arena, &options)?;
        let mut buf = Vec::new();
        format_html(root, &options, &mut buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_renderer() -> HandlebarsRenderer<'static> {
        let reg = Handlebars::new();
        HandlebarsRenderer {
            registry: reg,
            tag_templates: HashMap::new(),
        }
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

    #[test]
    fn test_render_markdown_without_tag_templates() {
        let renderer = create_renderer();
        let markdown = "# Hello\n\nA [link](https://example.com).";
        let html = renderer.render_markdown(markdown).unwrap();
        assert!(html.contains("<h1>Hello</h1>"));
        assert!(html.contains(r#"<a href="https://example.com">link</a>"#));
    }

    #[test]
    fn test_render_markdown_with_link_tag_template() {
        let mut renderer = create_renderer();
        renderer.tag_templates.insert(
            "a".to_string(),
            r#"<a href="{{props.href}}" class="custom-link">{{{content}}}</a>"#.to_string(),
        );

        let markdown = "[click here](https://example.com)";
        let html = renderer.render_markdown(markdown).unwrap();
        assert!(html.contains(r#"class="custom-link""#));
        assert!(html.contains(">click here</a>"));
    }

    #[test]
    fn test_render_markdown_with_heading_tag_template() {
        let mut renderer = create_renderer();
        renderer.tag_templates.insert(
            "h1".to_string(),
            "<h1 class='title'>{{{content}}}</h1>".to_string(),
        );

        let markdown = "# Hello World";
        let html = renderer.render_markdown(markdown).unwrap();
        assert!(html.contains("<h1 class='title'>Hello World</h1>"));
    }

    #[test]
    fn test_render_markdown_with_code_tag_template() {
        let mut renderer = create_renderer();
        renderer.tag_templates.insert(
            "code".to_string(),
            "<code class='inline-code'>{{{content}}}</code>".to_string(),
        );

        let markdown = "Use `hello_world()` function";
        let html = renderer.render_markdown(markdown).unwrap();
        assert!(
            html.contains("<code class='inline-code'>hello_world()</code>"),
            "Expected inline code content, got: {}",
            html
        );
    }

    #[test]
    fn test_render_markdown_with_pre_tag_template() {
        let mut renderer = create_renderer();
        renderer.tag_templates.insert(
            "pre".to_string(),
            "<pre class='code-block'><code>{{{content}}}</code></pre>".to_string(),
        );

        let markdown = "```rust\nfn main() {}\n```";
        let html = renderer.render_markdown(markdown).unwrap();
        assert!(
            html.contains("fn main() {}"),
            "Expected code block content, got: {}",
            html
        );
    }

    #[test]
    fn test_render_markdown_soft_break_no_br_template() {
        let mut renderer = create_renderer();
        renderer
            .tag_templates
            .insert("br".to_string(), "<br class='break'>".to_string());

        // Soft breaks occur inside paragraphs with line wrapping
        let markdown = "Line one\nLine two";
        let html = renderer.render_markdown(markdown).unwrap();
        assert!(
            !html.contains("class='break'"),
            "Soft breaks should NOT trigger br template, got: {}",
            html
        );
    }
}
