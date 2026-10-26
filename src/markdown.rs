use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::{self, Write};
use std::io;
use std::sync::LazyLock;

use comrak::adapters::SyntaxHighlighterAdapter;
use comrak::nodes::{AstNode, NodeValue};
use serde::Serialize;
use syntect::highlighting::ThemeSet;
use syntect::html::{ClassStyle, ClassedHTMLGenerator, css_for_theme_with_class_style};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::config;

/// Class prefix for highlighted tokens, so scope names like `string` or `comment`
/// don't collide with the site's own CSS.
const CLASS_STYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "hl-" };

/// Class put on `<pre>` for highlighted code blocks; the theme CSS styles it with
/// the theme's foreground and background colors.
const PRE_CLASS: &str = "hl-code";

/// Name of the stylesheet written to the output directory when `highlight_theme` is set.
pub const HIGHLIGHT_CSS_FILENAME: &str = "highlight.css";

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

/// A markdown document rendered to HTML, along with data derived from it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RenderedMarkdown {
    pub html: String,
    pub toc: Vec<TocEntry>,
    /// `toc` rendered as nested `<ul>` lists
    pub toc_html: String,
    /// HTML of the content before a `<!-- more -->` marker, or of the first paragraph
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TocEntry {
    pub level: u8,
    pub id: String,
    pub text: String,
}

/// Whether a raw HTML block is the excerpt separator (`<!-- more -->`).
#[must_use]
pub fn is_excerpt_marker(html: &str) -> bool {
    html.trim()
        .strip_prefix("<!--")
        .and_then(|rest| rest.strip_suffix("-->"))
        .is_some_and(|inner| inner.trim().eq_ignore_ascii_case("more"))
}

/// Finds the nodes making up the excerpt: the top-level nodes before the
/// `<!-- more -->` marker if there is one, otherwise the first paragraph,
/// wherever it is nested (e.g. in a blockquote or list).
#[must_use]
pub fn excerpt_nodes<'a>(root: &'a AstNode<'a>) -> Vec<&'a AstNode<'a>> {
    let mut before_marker = Vec::new();
    for node in root.children() {
        if let NodeValue::HtmlBlock(block) = &node.data.borrow().value
            && is_excerpt_marker(&block.literal)
        {
            return before_marker;
        }
        before_marker.push(node);
    }

    root.descendants()
        .find(|node| {
            matches!(node.data.borrow().value, NodeValue::Paragraph)
                && !node.ancestors().any(|ancestor| {
                    matches!(
                        ancestor.data.borrow().value,
                        NodeValue::FootnoteDefinition(_)
                    )
                })
        })
        .into_iter()
        .collect()
}

/// Plain text of a node, as used for heading ids and TOC entries.
#[must_use]
pub fn collect_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut text = String::new();
    push_text(node, &mut text);
    text
}

fn push_text<'a>(node: &'a AstNode<'a>, text: &mut String) {
    match &node.data.borrow().value {
        NodeValue::Text(literal) => text.push_str(literal),
        NodeValue::Code(code) => text.push_str(&code.literal),
        NodeValue::LineBreak | NodeValue::SoftBreak => text.push(' '),
        _ => {
            for child in node.children() {
                push_text(child, text);
            }
        }
    }
}

/// Renders TOC entries as nested `<ul>` lists following heading levels.
#[must_use]
pub fn toc_to_html(toc: &[TocEntry]) -> String {
    let mut html = String::new();
    let mut levels: Vec<u8> = Vec::new();

    for entry in toc {
        match levels.last() {
            None => {
                html.push_str("<ul>\n<li>");
                levels.push(entry.level);
            }
            Some(&current) if entry.level > current => {
                html.push_str("\n<ul>\n<li>");
                levels.push(entry.level);
            }
            Some(_) => {
                while levels.len() > 1 && levels.last().is_some_and(|&l| entry.level < l) {
                    html.push_str("</li>\n</ul>\n");
                    levels.pop();
                }
                match levels.last_mut() {
                    Some(l) if entry.level > *l => {
                        html.push_str("\n<ul>\n<li>");
                        levels.push(entry.level);
                    }
                    Some(l) => {
                        html.push_str("</li>\n<li>");
                        // A shallower heading than the first one becomes the outer level,
                        // so deeper headings after it nest under it.
                        *l = entry.level;
                    }
                    None => unreachable!("levels always keeps the outer level"),
                }
            }
        }
        let _ = write!(
            html,
            "<a href=\"#{}\">{}</a>",
            escape_html(&entry.id),
            escape_html(&entry.text)
        );
    }

    for _ in levels {
        html.push_str("</li>\n</ul>\n");
    }
    html
}

/// Highlights `code` as `lang`, returning class-annotated HTML, or `None` when the
/// language is unknown.
#[must_use]
pub fn highlight(lang: &str, code: &str) -> Option<String> {
    if lang.is_empty() {
        return None;
    }
    let syntax = SYNTAX_SET.find_syntax_by_token(lang)?;
    let mut generator =
        ClassedHTMLGenerator::new_with_class_style(syntax, &SYNTAX_SET, CLASS_STYLE);
    for line in LinesWithEndings::from(code) {
        generator
            .parse_html_for_line_which_includes_newline(line)
            .ok()?;
    }
    Some(generator.finalize())
}

/// Code block highlighter for comrak, emitting CSS classes instead of inline styles.
pub struct ClassHighlighter;

impl SyntaxHighlighterAdapter for ClassHighlighter {
    fn write_highlighted(
        &self,
        output: &mut dyn Write,
        lang: Option<&str>,
        code: &str,
    ) -> fmt::Result {
        match highlight(lang.unwrap_or_default(), code) {
            Some(html) => output.write_str(&html),
            None => comrak::html::escape(output, code),
        }
    }

    fn write_pre_tag(
        &self,
        output: &mut dyn Write,
        mut attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        attributes
            .entry("class")
            .and_modify(|class| {
                let class = class.to_mut();
                class.push(' ');
                class.push_str(PRE_CLASS);
            })
            .or_insert(Cow::Borrowed(PRE_CLASS));
        comrak::html::write_opening_tag(output, "pre", attributes)
    }

    fn write_code_tag(
        &self,
        output: &mut dyn Write,
        attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        comrak::html::write_opening_tag(output, "code", attributes)
    }
}

/// Names of the themes usable as `highlight_theme`.
#[must_use]
pub fn highlight_theme_names() -> Vec<String> {
    let mut names: Vec<String> = ThemeSet::load_defaults().themes.into_keys().collect();
    names.sort();
    names
}

/// Generates the stylesheet matching the highlighter's classes for a bundled theme.
pub fn highlight_theme_css(theme_name: &str) -> io::Result<String> {
    let themes = ThemeSet::load_defaults();
    let theme = themes.themes.get(theme_name).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "Unknown highlight_theme \"{theme_name}\", available themes: {}",
                highlight_theme_names().join(", ")
            ),
        )
    })?;
    css_for_theme_with_class_style(theme, CLASS_STYLE)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Writes the highlight theme stylesheet to the output directory, if one is configured.
pub fn write_highlight_css(configuration: &config::ResolvedConfig) -> io::Result<()> {
    let Some(theme_name) = &configuration.markdown.highlight_theme else {
        return Ok(());
    };
    let css = highlight_theme_css(theme_name)?;
    let css_path = configuration.output_directory.join(HIGHLIGHT_CSS_FILENAME);
    log::info!("Writing highlight theme to {}", css_path.display());
    std::fs::write(css_path, css)
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(level: u8, id: &str) -> TocEntry {
        TocEntry {
            level,
            id: id.to_string(),
            text: id.to_string(),
        }
    }

    #[test]
    fn test_excerpt_marker_variants() {
        assert!(is_excerpt_marker("<!-- more -->"));
        assert!(is_excerpt_marker("<!--more-->\n"));
        assert!(is_excerpt_marker("  <!--  MORE -->  "));
        assert!(!is_excerpt_marker("<!-- more stuff -->"));
        assert!(!is_excerpt_marker("<div>more</div>"));
    }

    #[test]
    fn test_toc_to_html_empty() {
        assert_eq!(toc_to_html(&[]), "");
    }

    #[test]
    fn test_toc_to_html_nests_by_level() {
        let html = toc_to_html(&[entry(2, "a"), entry(3, "b"), entry(2, "c")]);
        assert_eq!(
            html,
            "<ul>\n<li><a href=\"#a\">a</a>\n<ul>\n<li><a href=\"#b\">b</a></li>\n</ul>\n</li>\n<li><a href=\"#c\">c</a></li>\n</ul>\n"
        );
    }

    #[test]
    fn test_toc_to_html_is_balanced_for_irregular_levels() {
        let html = toc_to_html(&[
            entry(3, "a"),
            entry(1, "b"),
            entry(4, "c"),
            entry(2, "d"),
            entry(6, "e"),
        ]);
        assert_eq!(html.matches("<ul>").count(), html.matches("</ul>").count());
        assert_eq!(html.matches("<li>").count(), html.matches("</li>").count());
    }

    #[test]
    fn test_toc_to_html_nests_after_shallower_heading() {
        let html = toc_to_html(&[entry(3, "a"), entry(1, "b"), entry(2, "c")]);
        assert_eq!(
            html,
            "<ul>\n<li><a href=\"#a\">a</a></li>\n<li><a href=\"#b\">b</a>\n<ul>\n<li><a href=\"#c\">c</a></li>\n</ul>\n</li>\n</ul>\n"
        );
    }

    #[test]
    fn test_toc_to_html_escapes_text() {
        let toc = [TocEntry {
            level: 1,
            id: "x".to_string(),
            text: "<script>".to_string(),
        }];
        assert!(toc_to_html(&toc).contains("&lt;script&gt;"));
    }

    #[test]
    fn test_highlight_known_language_uses_classes() {
        let html = highlight("rust", "fn main() {}\n").unwrap();
        assert!(html.contains("class=\"hl-"), "got: {html}");
        assert!(!html.contains("style="), "got: {html}");
    }

    #[test]
    fn test_highlight_unknown_language() {
        assert!(highlight("not-a-language", "x").is_none());
        assert!(highlight("", "x").is_none());
    }

    #[test]
    fn test_highlight_escapes_html() {
        let html = highlight("html", "<script>alert(1)</script>\n").unwrap();
        assert!(!html.contains("<script>"), "got: {html}");
        assert!(html.contains("&lt;"), "got: {html}");
    }

    #[test]
    fn test_highlight_theme_css() {
        let css = highlight_theme_css("InspiredGitHub").unwrap();
        assert!(css.contains(".hl-"), "got: {css}");
        assert!(highlight_theme_css("nope").is_err());
    }
}
