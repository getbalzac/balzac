use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use comrak::nodes::{AstNode, ListType, NodeValue};
use comrak::{Anchorizer, Arena, Options, Plugins, format_html_with_plugins, parse_document};
use handlebars::{Handlebars, RenderErrorReason};

use crate::{
    config,
    error::{self, Error, Result},
    markdown::{self, ClassHighlighter, RenderedMarkdown, TocEntry},
    vite::{ViteAssetSource, parse_manifest, vite_url},
};

pub trait Renderer {
    fn new(configuration: &config::ResolvedConfig) -> Self;
    fn init(&mut self, configuration: &config::ResolvedConfig) -> Result<()>;
    /// Renders `template`, the content of the file at `path`.
    fn render(&self, path: &Path, template: String, data: serde_json::Value) -> Result<String>;
    fn render_markdown(&self, body: &str) -> Result<String>;
    fn render_document(&self, body: &str) -> Result<RenderedMarkdown>;
}

pub struct HandlebarsRenderer<'a> {
    pub registry: handlebars::Handlebars<'a>,
    pub tag_templates: HashMap<String, String>,
    pub markdown: config::MarkdownConfig,
    /// Files of the registered partials and layouts, by name, for error messages.
    template_paths: HashMap<String, PathBuf>,
    /// Files of the tag templates, by tag name, for error messages.
    tag_template_paths: HashMap<String, PathBuf>,
}

// Pages are rendered in parallel with one shared renderer (see `parallel`).
// Handlebars only accepts `Send + Sync` helpers, and Rhai script helpers are
// thanks to rhai's `sync` feature, so a helper that isn't fails to compile here.
const _: () = {
    const fn assert_sync<T: Sync>() {}
    assert_sync::<HandlebarsRenderer<'static>>();
};

/// Converts a handlebars parse error of the template at `path`.
fn template_error(path: &Path, error: &handlebars::TemplateError) -> Error {
    let (line, column) = error.pos().unzip();
    Error::Template {
        path: path.to_path_buf(),
        line,
        column,
        message: error.reason().to_string(),
        page: None,
    }
}

/// Per-document state collected while transforming the markdown AST.
#[derive(Default)]
struct DocumentState {
    anchorizer: Anchorizer,
    toc: Vec<TocEntry>,
}

impl HandlebarsRenderer<'_> {
    fn register_partials(&mut self, configuration: &config::ResolvedConfig) -> Result<()> {
        if fs::exists(&configuration.partials_directory)? {
            self.register_partial_dir(&configuration.partials_directory)?;
        } else {
            log::debug!("Could not find partial directory, skipping register step");
        }
        Ok(())
    }

    fn register_layouts(&mut self, configuration: &config::ResolvedConfig) -> Result<()> {
        if fs::exists(&configuration.layouts_directory)? {
            self.register_partial_dir(&configuration.layouts_directory)?;
        } else {
            log::debug!("Could not find layouts directory, skipping register step");
        }
        Ok(())
    }

    /// Registers every file of `directory` as a partial named after its file stem.
    fn register_partial_dir(&mut self, directory: &Path) -> Result<()> {
        for entry in fs::read_dir(directory)? {
            let dir = entry?;
            log::debug!("Parsing partial {}", dir.file_name().to_string_lossy());
            let partial_path = dir.path();
            let partial_content = error::read_to_string(&partial_path)?;
            let Some(partial_name) = partial_path.file_stem() else {
                continue;
            };
            let partial_name = partial_name.to_string_lossy().to_string();
            self.registry
                .register_partial(&partial_name, partial_content)
                .map_err(|e| template_error(&partial_path, &e))?;
            self.template_paths.insert(partial_name, partial_path);
        }
        Ok(())
    }

    fn register_tag_templates(&mut self, configuration: &config::ResolvedConfig) -> Result<()> {
        if !fs::exists(&configuration.tags_directory)? {
            log::debug!("Could not find tags directory, skipping tag templates");
            return Ok(());
        }

        for entry in fs::read_dir(&configuration.tags_directory)? {
            let dir = entry?;
            let path = dir.path();
            let extension = path.extension().map(|e| e.to_string_lossy().to_string());
            if extension.as_deref() != Some("hbs") {
                log::debug!(
                    "Skipping non-hbs file {} in tags directory",
                    dir.file_name().to_string_lossy()
                );
                continue;
            }
            let template_content = error::read_to_string(&path)?;
            // Tag templates are rendered on demand; parse them now so syntax
            // errors are reported up front with their file.
            handlebars::Template::compile(&template_content)
                .map_err(|e| template_error(&path, &e))?;
            let Some(tag_name) = path.file_stem() else {
                continue;
            };
            let tag_name = tag_name.to_string_lossy().to_string();
            log::debug!("Registering tag template for <{tag_name}>");
            self.tag_templates
                .insert(tag_name.clone(), template_content);
            self.tag_template_paths.insert(tag_name, path);
        }
        Ok(())
    }

    /// Converts a handlebars render error of the template at `path` into an
    /// error pointing at the file it happened in: `path` itself or one of the
    /// partials it uses.
    fn render_error(&self, path: &Path, error: &handlebars::RenderError) -> Error {
        if let RenderErrorReason::TemplateError(e) = error.reason() {
            return template_error(path, e);
        }
        let partial_path = error
            .template_name
            .as_ref()
            .and_then(|name| self.template_paths.get(name));
        let error = Error::Template {
            path: partial_path.map_or_else(|| path.to_path_buf(), Clone::clone),
            line: error.line_no,
            column: error.column_no,
            message: error.reason().to_string(),
            page: None,
        };
        error.in_page(path)
    }

    pub fn register_helpers(&mut self, configuration: &config::ResolvedConfig) -> Result<()> {
        if let Some(bundler) = &configuration.bundler
            && let Some(vite) = &bundler.vite
            && vite.enabled
        {
            let asset_source = if configuration.dev_mode {
                ViteAssetSource::DevOrigin(vite.dev_origin.clone())
            } else {
                let manifest_path = std::path::PathBuf::from(&vite.manifest_path);
                let manifest = parse_manifest(manifest_path, &configuration.root_directory)
                    .map_err(Error::Vite)?;
                ViteAssetSource::Manifest(manifest)
            };
            let helper = vite_url { asset_source };
            self.registry.register_helper("vite_url", Box::new(helper));
        }

        let markdown_renderer = self.markdown_renderer();
        crate::helpers::register(&mut self.registry, configuration, markdown_renderer);
        Ok(())
    }

    /// A renderer with this one's markdown options and tag templates, for the
    /// `markdown` helper. Its registry stays empty: the helper renders tag
    /// templates with the registry calling it.
    #[must_use]
    pub fn markdown_renderer(&self) -> HandlebarsRenderer<'static> {
        HandlebarsRenderer {
            registry: Handlebars::new(),
            tag_templates: self.tag_templates.clone(),
            markdown: self.markdown.clone(),
            template_paths: self.template_paths.clone(),
            tag_template_paths: self.tag_template_paths.clone(),
        }
    }

    #[cfg(feature = "script-helpers")]
    fn register_script_helpers(&mut self, configuration: &config::ResolvedConfig) -> Result<()> {
        crate::script_helpers::register_script_helpers(
            &mut self.registry,
            &configuration.helpers_directory,
        )
    }

    #[cfg(not(feature = "script-helpers"))]
    #[allow(clippy::unused_self)] // same signature as the script-helpers variant
    fn register_script_helpers(&mut self, configuration: &config::ResolvedConfig) -> Result<()> {
        if fs::exists(&configuration.helpers_directory)? {
            log::warn!(
                "Ignoring {}: this build of Balzac has no script helper support (`script-helpers` feature)",
                configuration.helpers_directory.display()
            );
        }
        Ok(())
    }

    fn get_tag_name(value: &NodeValue) -> Option<String> {
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

    fn build_props(&self, node: &AstNode, heading_id: Option<&str>) -> serde_json::Value {
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
                "id": heading_id,
            }),
            NodeValue::Code(c) => serde_json::json!({
                "num_backticks": c.num_backticks,
                "literal": Self::html_escape(&c.literal),
            }),
            NodeValue::CodeBlock(c) => serde_json::json!({
                "info": c.info,
                "lang": Self::code_block_lang(&c.info),
                "literal": Self::html_escape(&c.literal),
                "highlighted": self.highlight_code_block(&c.info, &c.literal),
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

    fn code_block_lang(info: &str) -> &str {
        info.split_whitespace().next().unwrap_or_default()
    }

    /// Highlighted HTML for a code block, falling back to the escaped literal.
    fn highlight_code_block(&self, info: &str, literal: &str) -> String {
        if self.markdown.syntax_highlighting
            && let Some(html) = markdown::highlight(Self::code_block_lang(info), literal)
        {
            return html;
        }
        Self::html_escape(literal)
    }

    fn plugins(&self) -> Plugins<'static> {
        static HIGHLIGHTER: ClassHighlighter = ClassHighlighter;
        let mut plugins = Plugins::default();
        if self.markdown.syntax_highlighting {
            plugins.render.codefence_syntax_highlighter = Some(&HIGHLIGHTER);
        }
        plugins
    }

    fn format_node<'b>(
        node: &'b AstNode<'b>,
        options: &Options,
        plugins: &Plugins,
    ) -> std::io::Result<String> {
        let mut buf = Vec::new();
        format_html_with_plugins(node, options, &mut buf, plugins)?;
        String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    fn render_children_to_html<'b>(
        node: &'b AstNode<'b>,
        arena: &'b Arena<AstNode<'b>>,
        options: &Options,
        plugins: &Plugins,
    ) -> std::io::Result<String> {
        let temp_root = arena.alloc(AstNode::from(NodeValue::Document));

        let mut child = node.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            temp_root.append(c);
        }

        Self::format_node(temp_root, options, plugins)
    }

    /// Default heading markup when heading ids are enabled and no tag template applies.
    fn render_heading(&self, level: u8, id: &str, inner_html: &str) -> String {
        let id = Self::html_escape(id);
        let anchor = if self.markdown.heading_anchors {
            format!(
                "<a class=\"heading-anchor\" href=\"#{id}\" aria-hidden=\"true\" tabindex=\"-1\">#</a>"
            )
        } else {
            String::new()
        };
        format!("<h{level} id=\"{id}\">{inner_html}{anchor}</h{level}>\n")
    }

    fn html_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#x27;")
            .replace('/', "&#x2F;")
    }

    fn apply_tag_templates<'b>(
        &self,
        registry: &Handlebars<'_>,
        node: &'b AstNode<'b>,
        arena: &'b Arena<AstNode<'b>>,
        options: &Options,
        plugins: &Plugins,
        state: &mut DocumentState,
    ) -> Result<()> {
        // Heading text is collected before children are replaced by tag templates.
        let heading_level = match &node.data.borrow().value {
            NodeValue::Heading(h) => Some(h.level),
            _ => None,
        };
        let heading_id = heading_level.and_then(|level| {
            let text = markdown::collect_text(node);
            // Headings without sluggable text (e.g. `## [^1]`) would get an empty id.
            if !text.chars().any(|c| c.is_alphanumeric() || c == '_') {
                return None;
            }
            let id = state.anchorizer.anchorize(text.clone());
            state.toc.push(TocEntry {
                level,
                id: id.clone(),
                text,
            });
            Some(id)
        });

        let mut child = node.first_child();
        while let Some(c) = child {
            self.apply_tag_templates(registry, c, arena, options, plugins, state)?;
            child = c.next_sibling();
        }

        let tag_name = {
            let data = node.data.borrow();
            Self::get_tag_name(&data.value)
        };
        let template = tag_name
            .as_ref()
            .and_then(|tag_name| self.tag_templates.get(tag_name));

        if template.is_none()
            && let (Some(level), Some(id)) = (heading_level, &heading_id)
            && (self.markdown.heading_ids || self.markdown.heading_anchors)
        {
            let inner_html = Self::render_children_to_html(node, arena, options, plugins)?;
            node.data.borrow_mut().value =
                NodeValue::HtmlInline(self.render_heading(level, id, &inner_html));
        }

        if let Some(tag_name) = tag_name
            && let Some(template) = template
        {
            let content = {
                let data = node.data.borrow();
                match &data.value {
                    NodeValue::Code(c) => Self::html_escape(&c.literal),
                    NodeValue::CodeBlock(c) => Self::html_escape(&c.literal),
                    _ => Self::render_children_to_html(node, arena, options, plugins)?,
                }
            };
            let props = self.build_props(node, heading_id.as_deref());

            let template_data = serde_json::json!({
                "props": props,
                "content": content,
            });

            let rendered = registry
                .render_template(template, &template_data)
                .map_err(|e| {
                    let path = self.tag_template_paths.get(&tag_name).map_or_else(
                        || PathBuf::from(format!("<{tag_name}> tag template")),
                        Clone::clone,
                    );
                    self.render_error(&path, &e)
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

impl HandlebarsRenderer<'_> {
    /// Renders `template`, the content of the file at `path`, with a context
    /// that can be reused across renders (see `SiteContext`).
    pub fn render_with_context(
        &self,
        path: &Path,
        template: &str,
        context: &handlebars::Context,
    ) -> Result<String> {
        self.registry
            .render_template_with_context(template, context)
            .map_err(|e| self.render_error(path, &e))
    }

    /// Renders a markdown string from a template (the `markdown` helper), with
    /// tag templates rendered by `registry`. With `inline`, a document made of
    /// a single paragraph is rendered without the `<p>` around it.
    pub fn render_fragment(
        &self,
        registry: &Handlebars<'_>,
        body: &str,
        inline: bool,
    ) -> Result<String> {
        let arena = Arena::new();
        let options = crate::collection::build_comrak_options(&self.markdown);
        let plugins = self.plugins();
        let root = parse_document(&arena, body, &options);

        if inline
            && let Some(paragraph) = root.first_child()
            && paragraph.next_sibling().is_none()
            && matches!(paragraph.data.borrow().value, NodeValue::Paragraph)
        {
            let mut child = paragraph.first_child();
            while let Some(c) = child {
                child = c.next_sibling();
                root.append(c);
            }
            paragraph.detach();
        }

        let mut state = DocumentState::default();
        self.apply_tag_templates(registry, root, &arena, &options, &plugins, &mut state)?;
        Ok(Self::format_node(root, &options, &plugins)?)
    }
}

impl<'a> Renderer for HandlebarsRenderer<'a> {
    fn init(&mut self, configuration: &config::ResolvedConfig) -> Result<()> {
        self.register_partials(configuration)?;
        self.register_layouts(configuration)?;
        self.register_tag_templates(configuration)?;
        self.register_helpers(configuration)?;
        // After the built-in helpers, so a script can replace one of them
        self.register_script_helpers(configuration)
    }
    fn new(configuration: &config::ResolvedConfig) -> HandlebarsRenderer<'a> {
        let reg = Handlebars::new();

        HandlebarsRenderer {
            registry: reg,
            tag_templates: HashMap::new(),
            markdown: configuration.markdown.clone(),
            template_paths: HashMap::new(),
            tag_template_paths: HashMap::new(),
        }
    }
    fn render(&self, path: &Path, template: String, data: serde_json::Value) -> Result<String> {
        // `render_template` would serialize `data` into a copy; wrapping moves it
        self.render_with_context(path, &template, &handlebars::Context::from(data))
    }

    fn render_markdown(&self, body: &str) -> Result<String> {
        self.render_document(body).map(|document| document.html)
    }

    fn render_document(&self, body: &str) -> Result<RenderedMarkdown> {
        let arena = Arena::new();
        let options = crate::collection::build_comrak_options(&self.markdown);
        let plugins = self.plugins();
        let root = parse_document(&arena, body, &options);

        // Picked before tag templates run, since they replace paragraphs with raw HTML.
        // The nodes themselves stay in the tree and are transformed in place.
        let excerpt_nodes = markdown::excerpt_nodes(root);

        let mut state = DocumentState::default();
        self.apply_tag_templates(&self.registry, root, &arena, &options, &plugins, &mut state)?;

        let excerpt = if excerpt_nodes.is_empty() {
            None
        } else {
            let mut excerpt = String::new();
            for node in excerpt_nodes {
                excerpt.push_str(&Self::format_node(node, &options, &plugins)?);
            }
            Some(excerpt)
        };

        Ok(RenderedMarkdown {
            html: Self::format_node(root, &options, &plugins)?,
            toc_html: markdown::toc_to_html(&state.toc),
            toc: state.toc,
            excerpt,
        })
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
            markdown: config::MarkdownConfig::default(),
            template_paths: HashMap::new(),
            tag_template_paths: HashMap::new(),
        }
    }

    #[test]
    fn test_term_url_helper() {
        let mut renderer = create_renderer();
        renderer
            .register_helpers(&config::Config::default().resolve(std::path::Path::new(".")))
            .unwrap();
        let data = serde_json::json!({"tags": ["Web Dev", 2024, "!!"]});

        let result = renderer
            .render(
                Path::new("test.hbs"),
                "{{#each tags}}[{{term_url \"tags\" this}}]{{/each}}[{{term_url \"tags\" missing}}]"
                    .to_string(),
                data,
            )
            .unwrap();
        assert_eq!(result, "[/tags/web-dev/][/tags/2024/][][]");
    }

    #[test]
    fn test_render_simple_template() {
        let renderer = create_renderer();
        let template = "Hello, World!".to_string();
        let data = serde_json::json!({});

        let result = renderer
            .render(Path::new("test.hbs"), template, data)
            .unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_render_template_with_variable() {
        let renderer = create_renderer();
        let template = "Hello, {{name}}!".to_string();
        let data = serde_json::json!({
            "name": "Alice"
        });

        let result = renderer
            .render(Path::new("test.hbs"), template, data)
            .unwrap();
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

        let result = renderer
            .render(Path::new("test.hbs"), template, data)
            .unwrap();
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

        let result = renderer
            .render(Path::new("test.hbs"), template, data)
            .unwrap();
        assert_eq!(result, "Author: Jane Doe (jane@example.com)");
    }

    #[test]
    fn test_render_template_with_conditional() {
        let renderer = create_renderer();
        let template = "{{#if is_active}}Active{{else}}Inactive{{/if}}".to_string();
        let data = serde_json::json!({
            "is_active": true
        });

        let result = renderer
            .render(Path::new("test.hbs"), template, data)
            .unwrap();
        assert_eq!(result, "Active");
    }

    #[test]
    fn test_render_template_with_loop() {
        let renderer = create_renderer();
        let template = "{{#each items}}- {{this}}\n{{/each}}".to_string();
        let data = serde_json::json!({
            "items": ["Rust", "Handlebars", "SSG"]
        });

        let result = renderer
            .render(Path::new("test.hbs"), template, data)
            .unwrap();
        assert_eq!(result, "- Rust\n- Handlebars\n- SSG\n");
    }

    #[test]
    fn test_render_empty_template_with_data() {
        let renderer = create_renderer();
        let template = String::new();
        let data = serde_json::json!({
            "unused": "data"
        });

        let result = renderer
            .render(Path::new("test.hbs"), template, data)
            .unwrap();
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
            "Expected inline code content, got: {html}"
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
            "Expected code block content, got: {html}"
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
            "Soft breaks should NOT trigger br template, got: {html}"
        );
    }

    #[test]
    fn test_code_literal_html_escaped() {
        let mut renderer = create_renderer();
        renderer
            .tag_templates
            .insert("code".to_string(), "<code>{{{content}}}</code>".to_string());

        let markdown = "`& < > \"' /`";
        let html = renderer.render_markdown(markdown).unwrap();
        assert!(
            html.contains("<code>&amp; &lt; &gt; &quot;&#x27; &#x2F;</code>"),
            "Code literal should be HTML-escaped, got: {html}"
        );
    }

    #[test]
    fn test_codeblock_literal_html_escaped() {
        let mut renderer = create_renderer();
        renderer
            .tag_templates
            .insert("pre".to_string(), "<pre>{{{content}}}</pre>".to_string());

        let markdown = "```html\n<script>alert(1)</script>\n```";
        let html = renderer.render_markdown(markdown).unwrap();
        assert!(
            html.contains("&lt;script&gt;alert(1)&lt;&#x2F;script&gt;"),
            "Code block literal should be HTML-escaped, got: {html}"
        );
        assert!(
            !html.contains("<script>alert(1)</script>"),
            "Raw script tag should NOT appear, got: {html}"
        );
    }

    #[test]
    fn test_code_template_xss_prevention() {
        let mut renderer = create_renderer();
        renderer.tag_templates.insert(
            "code".to_string(),
            "<code class='safe'>{{{content}}}</code>".to_string(),
        );

        let markdown = "`<script>alert('xss')</script>`";
        let html = renderer.render_markdown(markdown).unwrap();
        assert!(
            html.contains("&lt;script&gt;"),
            "XSS payload should be escaped, got: {html}"
        );
        assert!(
            !html.contains("<script>alert('xss')</script>"),
            "Raw script tag should NOT appear, got: {html}"
        );
    }

    #[test]
    fn test_code_props_literal_is_escaped() {
        let mut renderer = create_renderer();
        renderer.tag_templates.insert(
            "code".to_string(),
            "<code class='lang'>{{{props.literal}}}</code>".to_string(),
        );

        let markdown = "`<script>alert(1)</script>`";
        let html = renderer.render_markdown(markdown).unwrap();
        assert!(
            html.contains("&lt;script&gt;"),
            "props.literal should be HTML-escaped, got: {html}"
        );
        assert!(
            !html.contains("<script>alert(1)</script>"),
            "Raw script tag should NOT appear in props.literal, got: {html}"
        );
    }

    #[test]
    fn test_codeblock_props_literal_is_escaped() {
        let mut renderer = create_renderer();
        renderer.tag_templates.insert(
            "pre".to_string(),
            "<pre><code class='lang'>{{{props.literal}}}</code></pre>".to_string(),
        );

        let markdown = "```html\n<script>alert(1)</script>\n```";
        let html = renderer.render_markdown(markdown).unwrap();
        assert!(
            html.contains("&lt;script&gt;alert(1)&lt;&#x2F;script&gt;"),
            "props.literal should be HTML-escaped, got: {html}"
        );
        assert!(
            !html.contains("<script>alert(1)</script>"),
            "Raw script tag should NOT appear in props.literal, got: {html}"
        );
    }

    #[test]
    fn test_apostrophe_attribute_breakout_prevention() {
        let mut renderer = create_renderer();
        renderer.tag_templates.insert(
            "code".to_string(),
            "<code class='{{{props.literal}}}'>{{{content}}}</code>".to_string(),
        );

        let markdown = "`don't`";
        let html = renderer.render_markdown(markdown).unwrap();
        assert!(
            html.contains("class='don&#x27;t'"),
            "Apostrophe in attribute should be escaped, got: {html}"
        );
        assert!(
            !html.contains("class='don't'"),
            "Raw apostrophe should NOT break out of attribute, got: {html}"
        );
    }

    #[test]
    fn test_slash_attribute_breakout_prevention() {
        let mut renderer = create_renderer();
        renderer.tag_templates.insert(
            "code".to_string(),
            "<code class='{{{props.literal}}}'>{{{content}}}</code>".to_string(),
        );

        let markdown = "`x/y`";
        let html = renderer.render_markdown(markdown).unwrap();
        assert!(
            html.contains("class='x&#x2F;y'"),
            "Slash in attribute should be escaped, got: {html}"
        );
        assert!(
            !html.contains("class='x/y'"),
            "Raw slash should NOT appear in attribute, got: {html}"
        );
    }

    fn create_renderer_with(markdown: config::MarkdownConfig) -> HandlebarsRenderer<'static> {
        HandlebarsRenderer {
            markdown,
            ..create_renderer()
        }
    }

    fn with_heading_ids() -> config::MarkdownConfig {
        config::MarkdownConfig {
            heading_ids: true,
            ..config::MarkdownConfig::default()
        }
    }

    #[test]
    fn test_headings_have_no_ids_by_default() {
        let renderer = create_renderer();
        let html = renderer.render_markdown("## Hello World").unwrap();
        assert_eq!(html, "<h2>Hello World</h2>\n");
    }

    #[test]
    fn test_heading_ids() {
        let renderer = create_renderer_with(with_heading_ids());
        let html = renderer
            .render_markdown("## Hello *World*\n\n## Hello World")
            .unwrap();
        assert!(
            html.contains(r#"<h2 id="hello-world">Hello <em>World</em></h2>"#),
            "got: {html}"
        );
        assert!(
            html.contains(r#"<h2 id="hello-world-1">Hello World</h2>"#),
            "Duplicate headings should get unique ids, got: {html}"
        );
    }

    #[test]
    fn test_headings_without_text_get_no_id() {
        let renderer = create_renderer_with(config::MarkdownConfig {
            heading_anchors: true,
            ..config::MarkdownConfig::default()
        });
        let document = renderer
            .render_document("## \n\n## !!!\n\n## Real")
            .unwrap();
        assert!(!document.html.contains("id=\"\""), "got: {}", document.html);
        assert!(
            !document.html.contains("id=\"-1\""),
            "got: {}",
            document.html
        );
        assert!(
            document.html.contains("<h2></h2>"),
            "got: {}",
            document.html
        );
        assert!(
            document.html.contains("<h2>!!!</h2>"),
            "got: {}",
            document.html
        );
        assert!(
            document.html.contains(r#"<h2 id="real">Real"#),
            "got: {}",
            document.html
        );
        assert_eq!(document.toc.len(), 1);
    }

    #[test]
    fn test_heading_anchor_links_imply_ids() {
        let renderer = create_renderer_with(config::MarkdownConfig {
            heading_anchors: true,
            ..config::MarkdownConfig::default()
        });
        let html = renderer.render_markdown("# Title").unwrap();
        assert!(
            html.contains(r##"<h1 id="title">Title<a class="heading-anchor" href="#title""##),
            "got: {html}"
        );
    }

    #[test]
    fn test_heading_template_receives_id() {
        let mut renderer = create_renderer();
        renderer.tag_templates.insert(
            "h2".to_string(),
            r#"<h2 id="{{props.id}}" class="custom">{{{content}}}</h2>"#.to_string(),
        );
        let html = renderer.render_markdown("## Install `balzac`").unwrap();
        assert!(
            html.contains(
                r#"<h2 id="install-balzac" class="custom">Install <code>balzac</code></h2>"#
            ),
            "got: {html}"
        );
    }

    #[test]
    fn test_toc_collects_headings_in_order() {
        let renderer = create_renderer();
        let document = renderer
            .render_document("# Title\n\ntext\n\n## First\n\n### Nested `code`\n\n## First")
            .unwrap();
        let toc: Vec<(u8, &str, &str)> = document
            .toc
            .iter()
            .map(|e| (e.level, e.id.as_str(), e.text.as_str()))
            .collect();
        assert_eq!(
            toc,
            vec![
                (1, "title", "Title"),
                (2, "first", "First"),
                (3, "nested-code", "Nested code"),
                (2, "first-1", "First"),
            ]
        );
        assert!(
            document
                .toc_html
                .contains(r##"<a href="#nested-code">Nested code</a>"##)
        );
    }

    #[test]
    fn test_toc_text_ignores_tag_templates_inside_headings() {
        let mut renderer = create_renderer_with(with_heading_ids());
        renderer
            .tag_templates
            .insert("code".to_string(), "<kbd>{{{content}}}</kbd>".to_string());
        let document = renderer.render_document("## Press `Enter`").unwrap();
        assert_eq!(document.toc[0].text, "Press Enter");
        assert!(
            document
                .html
                .contains(r#"<h2 id="press-enter">Press <kbd>Enter</kbd></h2>"#),
            "got: {}",
            document.html
        );
    }

    #[test]
    fn test_toc_text_is_escaped_in_toc_html() {
        let renderer = create_renderer();
        let document = renderer.render_document("# a `<b>` c").unwrap();
        assert!(
            document.toc_html.contains("a &lt;b&gt; c"),
            "got: {}",
            document.toc_html
        );
    }

    #[test]
    fn test_excerpt_before_more_marker() {
        let renderer = create_renderer();
        let document = renderer
            .render_document("First.\n\nSecond.\n\n<!-- more -->\n\nRest.")
            .unwrap();
        assert_eq!(
            document.excerpt.as_deref(),
            Some("<p>First.</p>\n<p>Second.</p>\n")
        );
        assert!(document.html.contains("<p>Rest.</p>"));
    }

    #[test]
    fn test_excerpt_defaults_to_first_paragraph() {
        let renderer = create_renderer();
        let document = renderer
            .render_document("# Title\n\nFirst **para**.\n\nSecond.")
            .unwrap();
        assert_eq!(
            document.excerpt.as_deref(),
            Some("<p>First <strong>para</strong>.</p>\n")
        );
    }

    #[test]
    fn test_excerpt_finds_paragraph_in_blockquote() {
        let renderer = create_renderer();
        let document = renderer.render_document("> Opening paragraph.").unwrap();
        assert_eq!(
            document.excerpt.as_deref(),
            Some("<p>Opening paragraph.</p>\n")
        );
    }

    #[test]
    fn test_excerpt_finds_paragraph_in_list() {
        let renderer = create_renderer();
        let document = renderer
            .render_document("- first item\n\n- second item")
            .unwrap();
        assert_eq!(document.excerpt.as_deref(), Some("<p>first item</p>\n"));
    }

    #[test]
    fn test_excerpt_skips_footnote_definitions() {
        let renderer = create_renderer_with(config::MarkdownConfig {
            footnotes: true,
            ..config::MarkdownConfig::default()
        });
        let document = renderer
            .render_document("# Title[^1]\n\n[^1]: Note.")
            .unwrap();
        assert_eq!(document.excerpt, None);
    }

    #[test]
    fn test_excerpt_none_without_paragraph() {
        let renderer = create_renderer();
        let document = renderer.render_document("# Only a title").unwrap();
        assert_eq!(document.excerpt, None);
    }

    #[test]
    fn test_excerpt_ignores_marker_in_code_block() {
        let renderer = create_renderer();
        let document = renderer
            .render_document("Intro.\n\n```html\n<!-- more -->\n```\n\nRest.")
            .unwrap();
        assert_eq!(document.excerpt.as_deref(), Some("<p>Intro.</p>\n"));
    }

    #[test]
    fn test_excerpt_uses_tag_templates() {
        let mut renderer = create_renderer();
        renderer.tag_templates.insert(
            "p".to_string(),
            "<p class='lead'>{{{content}}}</p>".to_string(),
        );
        let document = renderer.render_document("Hello.\n\nWorld.").unwrap();
        assert_eq!(
            document.excerpt.as_deref(),
            Some("<p class='lead'>Hello.</p>")
        );
    }

    #[test]
    fn test_footnotes_disabled_by_default() {
        let renderer = create_renderer();
        let html = renderer.render_markdown("Hi[^1].\n\n[^1]: Note.").unwrap();
        assert!(!html.contains("footnote"), "got: {html}");
    }

    #[test]
    fn test_footnotes_enabled() {
        let renderer = create_renderer_with(config::MarkdownConfig {
            footnotes: true,
            ..config::MarkdownConfig::default()
        });
        let html = renderer.render_markdown("Hi[^1].\n\n[^1]: Note.").unwrap();
        assert!(html.contains(r##"<a href="#fn-1""##), "got: {html}");
        assert!(
            html.contains(r#"<section class="footnotes""#),
            "got: {html}"
        );
    }

    #[test]
    fn test_code_block_highlighted_by_default() {
        let renderer = create_renderer();
        let html = renderer
            .render_markdown("```rust\nfn main() {}\n```")
            .unwrap();
        assert!(
            html.starts_with(
                r#"<pre class="hl-code"><code class="language-rust"><span class="hl-"#
            ),
            "got: {html}"
        );
    }

    #[test]
    fn test_highlighted_code_block_is_escaped() {
        let renderer = create_renderer();
        let html = renderer
            .render_markdown("```html\n<script>alert(1)</script>\n```")
            .unwrap();
        assert!(!html.contains("<script>"), "got: {html}");
        assert!(html.contains("&lt;"), "got: {html}");

        let html = renderer
            .render_markdown("```unknown-lang\n<script>alert(1)</script>\n```")
            .unwrap();
        assert!(!html.contains("<script>"), "got: {html}");
    }

    #[test]
    fn test_code_block_highlighting_disabled() {
        let renderer = create_renderer_with(config::MarkdownConfig {
            syntax_highlighting: false,
            ..config::MarkdownConfig::default()
        });
        let html = renderer
            .render_markdown("```rust\nfn main() {}\n```")
            .unwrap();
        assert_eq!(
            html,
            "<pre><code class=\"language-rust\">fn main() {}\n</code></pre>\n"
        );
    }

    #[test]
    fn test_code_block_highlighted_inside_templated_parent() {
        let mut renderer = create_renderer();
        renderer.tag_templates.insert(
            "blockquote".to_string(),
            "<blockquote class='q'>{{{content}}}</blockquote>".to_string(),
        );
        let html = renderer
            .render_markdown("> ```rust\n> let x = 1;\n> ```")
            .unwrap();
        assert!(
            html.contains(r#"<blockquote class='q'><pre class="hl-code">"#),
            "got: {html}"
        );
    }

    #[test]
    fn test_pre_template_receives_highlighted_props() {
        let mut renderer = create_renderer();
        renderer.tag_templates.insert(
            "pre".to_string(),
            "<pre data-lang='{{props.lang}}'><code>{{{props.highlighted}}}</code></pre>"
                .to_string(),
        );
        let html = renderer
            .render_markdown("```html title=x\n<script>alert(1)</script>\n```")
            .unwrap();
        assert!(html.contains("<pre data-lang='html'>"), "got: {html}");
        assert!(html.contains(r#"<span class="hl-"#), "got: {html}");
        assert!(
            !html.contains("<script>"),
            "props.highlighted should be escaped, got: {html}"
        );
    }

    #[test]
    fn test_pre_template_highlighted_falls_back_to_escaped_literal() {
        let mut renderer = create_renderer_with(config::MarkdownConfig {
            syntax_highlighting: false,
            ..config::MarkdownConfig::default()
        });
        renderer.tag_templates.insert(
            "pre".to_string(),
            "<pre>{{{props.highlighted}}}</pre>".to_string(),
        );
        let html = renderer
            .render_markdown("```html\n<script>alert(1)</script>\n```")
            .unwrap();
        assert!(
            html.contains("&lt;script&gt;alert(1)&lt;&#x2F;script&gt;"),
            "got: {html}"
        );
    }
}
