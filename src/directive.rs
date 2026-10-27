//! Generic markdown directives, following the syntax of
//! [micromark-extension-directive](https://github.com/micromark/micromark-extension-directive#syntax),
//! the reference implementation of the
//! [directive proposal](https://talk.commonmark.org/t/generic-directives-plugins-syntax/444):
//!
//! - text: `:name[label]{attributes}`, inline
//! - leaf: `::name[label]{attributes}`, on a line of its own
//! - container: `:::name[label]{attributes}` up to a closing `:::`, wrapping markdown
//!
//! Containers are parsed by comrak (`block_directive`); text and leaf directives
//! are picked out of the parsed AST by [`transform`]. Escaped characters (`\:`,
//! `\]`) are kept apart by comrak (`escaped_char_spans`), so they never start
//! or close a directive.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use comrak::Arena;
use comrak::nodes::{AstNode, NodeValue};
use regex::Regex;
use serde::Serialize;

/// Brackets a label can nest, as in micromark.
const MAX_LABEL_NESTING: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    Leaf,
    Container,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attributes {
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub attributes: BTreeMap<String, String>,
}

impl Attributes {
    fn is_empty(&self) -> bool {
        self.id.is_none() && self.classes.is_empty() && self.attributes.is_empty()
    }

    /// Sets an attribute: `id` replaces the id and `class` adds classes, like
    /// the `#id` and `.class` shortcuts; other keys keep their last value.
    fn set(&mut self, name: &str, value: String) {
        match name {
            "id" => self.id = Some(value),
            "class" => self
                .classes
                .extend(value.split_whitespace().map(str::to_string)),
            _ => {
                self.attributes.insert(name.to_string(), value);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    pub kind: Kind,
    pub name: String,
    /// Markdown of a container's `[label]`; the labels of text and leaf
    /// directives are parsed in place and become the node's children.
    pub label: Option<String>,
    pub has_label: bool,
    pub attributes: Attributes,
}

/// Directives of a document, by the address of their node.
pub type Directives = HashMap<*const (), Directive>;

#[must_use]
pub fn key(node: &AstNode<'_>) -> *const () {
    std::ptr::from_ref(node).cast()
}

/// Finds the directives of a parsed document. Text and leaf directives are
/// replaced by placeholder nodes holding their label; container directives
/// stay comrak's `BlockDirective` nodes.
///
/// Like Docusaurus, a text directive without label or attributes (`:name`) is
/// left as text unless `has_template` has a template for it, so that prose
/// like `10:30` or `Note:this` is untouched.
pub fn transform<'a>(
    node: &'a AstNode<'a>,
    arena: &'a Arena<'a>,
    directives: &mut Directives,
    has_template: &dyn Fn(&str) -> bool,
) {
    let container = match &node.data.borrow().value {
        NodeValue::BlockDirective(block) => parse_container_info(&block.info),
        NodeValue::Paragraph => {
            for part in split_leaf_directives(node, arena, directives) {
                transform_inlines(part, arena, directives, has_template);
            }
            return;
        }
        NodeValue::Escaped => return,
        _ => None,
    };
    if let Some(directive) = container {
        directives.insert(key(node), directive);
    }
    transform_inlines(node, arena, directives, has_template);
}

/// Walks the children of `parent`, extracting text directives from its text nodes.
fn transform_inlines<'a>(
    parent: &'a AstNode<'a>,
    arena: &'a Arena<'a>,
    directives: &mut Directives,
    has_template: &dyn Fn(&str) -> bool,
) {
    let mut child = parent.first_child();
    while let Some(node) = child {
        // Taken first: nodes a paragraph is split into are inserted after it,
        // and handled by `transform`.
        child = node.next_sibling();
        if !matches!(node.data.borrow().value, NodeValue::Text(_)) {
            transform(node, arena, directives, has_template);
        } else if let Some((directive_node, after)) =
            text_directive(node, arena, directives, has_template)
        {
            transform_inlines(directive_node, arena, directives, has_template);
            child = Some(after);
        }
    }
}

/// Whether `c` is Unicode punctuation or a symbol (`\p{P}` or `\p{S}`), which
/// end directive names.
fn is_punctuation(c: char) -> bool {
    static PUNCTUATION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[\p{P}\p{S}]$").expect("valid regex"));
    if c.is_ascii() {
        return c.is_ascii_punctuation();
    }
    PUNCTUATION.is_match(c.encode_utf8(&mut [0; 4]))
}

/// Splits a directive name off the start of `s`: characters other than
/// whitespace and punctuation, with `-` and `_` allowed but not at the end.
fn parse_name(s: &str) -> Option<(&str, &str)> {
    let first = s.chars().next()?;
    if first.is_whitespace() || is_punctuation(first) {
        return None;
    }
    let len = s
        .char_indices()
        .skip(1)
        .find(|&(_, c)| c.is_whitespace() || (is_punctuation(c) && c != '-' && c != '_'))
        .map_or(s.len(), |(i, _)| i);
    let (name, rest) = s.split_at(len);
    if name.ends_with(['-', '_']) {
        return None;
    }
    Some((name, rest))
}

/// Parses a container's info string, `name[label]{attributes}`.
fn parse_container_info(info: &str) -> Option<Directive> {
    let (name, mut rest) = parse_name(info)?;
    let label = match rest.strip_prefix('[') {
        Some(after) => {
            let Brackets::Closed(end) = scan_brackets(after, 1) else {
                return None;
            };
            rest = &after[end + 1..];
            Some(after[..end].to_string())
        }
        None => None,
    };
    let attributes = if rest.starts_with('{') {
        let (attributes, len) = parse_attributes(rest)?;
        rest = &rest[len..];
        attributes
    } else {
        Attributes::default()
    };
    if !rest.trim().is_empty() {
        return None;
    }
    Some(Directive {
        kind: Kind::Container,
        name: name.to_string(),
        has_label: label.is_some(),
        label,
        attributes,
    })
}

/// Result of scanning text for the `]` closing a label.
enum Brackets {
    /// Offset of the closing `]`
    Closed(usize),
    /// Brackets still open at the end of the text
    Open(usize),
    TooDeep,
}

/// Scans `s` for the `]` closing `depth` open brackets.
fn scan_brackets(s: &str, mut depth: usize) -> Brackets {
    for (i, c) in s.char_indices() {
        match c {
            '[' => {
                depth += 1;
                if depth > MAX_LABEL_NESTING + 1 {
                    return Brackets::TooDeep;
                }
            }
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Brackets::Closed(i);
                }
            }
            _ => {}
        }
    }
    Brackets::Open(depth)
}

/// Characters that can't appear in unquoted attribute values and shortcuts.
fn is_value_breaker(c: char) -> bool {
    matches!(c, '"' | '\'' | '<' | '=' | '>' | '`')
}

/// Parses `{#id .class key=value key="value" key}` at the start of `s`,
/// returning the attributes and the length of the block.
fn parse_attributes(s: &str) -> Option<(Attributes, usize)> {
    let mut attributes = Attributes::default();
    let mut rest = s.strip_prefix('{')?;
    loop {
        rest = rest.trim_start();
        let c = rest.chars().next()?;
        if c == '}' {
            return Some((attributes, s.len() - rest.len() + 1));
        }
        if c == '#' || c == '.' {
            let value = &rest[1..];
            let len = value
                .find(|c: char| {
                    c.is_whitespace() || matches!(c, '#' | '.' | '}') || is_value_breaker(c)
                })
                .unwrap_or(value.len());
            let stop = value[len..].chars().next()?;
            if len == 0 || is_value_breaker(stop) {
                return None;
            }
            if c == '#' {
                attributes.id = Some(value[..len].to_string());
            } else {
                attributes.classes.push(value[..len].to_string());
            }
            rest = &value[len..];
        } else if is_punctuation(c) && c != '-' && c != '_' {
            return None;
        } else {
            let len = rest
                .find(|c: char| {
                    c.is_whitespace() || (is_punctuation(c) && !matches!(c, '-' | '.' | ':' | '_'))
                })
                .unwrap_or(rest.len());
            let name = &rest[..len];
            let after = rest[len..].trim_start();
            let (value, after) = match after.strip_prefix('=') {
                Some(value) => parse_value(value.trim_start())?,
                None => (String::new(), after),
            };
            attributes.set(name, value);
            rest = after;
        }
    }
}

/// Parses an attribute value, quoted with `"` or `'`, or unquoted.
fn parse_value(s: &str) -> Option<(String, &str)> {
    let c = s.chars().next()?;
    let (value, after) = match c {
        '<' | '=' | '>' | '`' | '}' => return None,
        '"' | '\'' => {
            let inner = &s[1..];
            let end = inner.find(c)?;
            (&inner[..end], &inner[end + 1..])
        }
        _ => {
            let len = s
                .find(|c: char| c.is_whitespace() || c == '}' || is_value_breaker(c))
                .unwrap_or(s.len());
            s.split_at(len)
        }
    };
    let next = after.chars().next()?;
    (next == '}' || next.is_whitespace()).then(|| (value.to_string(), after))
}

fn text_literal<'a>(node: &'a AstNode<'a>) -> Option<String> {
    match &node.data.borrow().value {
        NodeValue::Text(text) => Some(text.to_string()),
        _ => None,
    }
}

fn set_text<'a>(node: &'a AstNode<'a>, text: String) {
    node.data.borrow_mut().value = NodeValue::Text(Cow::Owned(text));
}

fn is_line_break<'a>(node: &'a AstNode<'a>) -> bool {
    matches!(
        node.data.borrow().value,
        NodeValue::SoftBreak | NodeValue::LineBreak
    )
}

/// A directive found in inline content, starting in a text node.
struct InlineMatch<'a> {
    name: String,
    /// Offset of the `:` the directive starts with in the start node
    start: usize,
    /// Label: offset after `[` in the start node, node and offset of the closing `]`
    label: Option<(usize, &'a AstNode<'a>, usize)>,
    attributes: Option<Attributes>,
    /// Node and offset where the text after the directive starts
    end: (&'a AstNode<'a>, usize),
}

impl InlineMatch<'_> {
    /// Without label content or attributes (`:name`, `:name[]{}`).
    fn is_simple(&self, start_node: &AstNode<'_>) -> bool {
        let empty_label = self.label.is_none_or(|(label_start, close_node, close)| {
            key(close_node) == key(start_node) && close == label_start
        });
        empty_label && self.attributes.as_ref().is_none_or(Attributes::is_empty)
    }
}

/// Matches `:name[label]{attributes}` with its `:` at `start` in the text node
/// `node`. The label can span nodes (`:abbr[*HTML*]`), and lines unless
/// `single_line`; attributes must be plain text. As in micromark, an invalid
/// label or attributes block ends the directive before it.
fn match_inline<'a>(
    node: &'a AstNode<'a>,
    text: &str,
    start: usize,
    single_line: bool,
) -> Option<InlineMatch<'a>> {
    let (name, rest) = parse_name(&text[start + 1..])?;
    // `:name:` is left to emoji shortcodes
    if rest.starts_with(':') {
        return None;
    }
    let after_name = start + 1 + name.len();
    let label = if rest.starts_with('[') {
        find_label(node, text, after_name + 1, single_line)
    } else {
        None
    };
    let (end_node, mut end) = match label {
        Some((_, close_node, close)) => (close_node, close + 1),
        None => (node, after_name),
    };

    let end_text = text_literal(end_node).unwrap_or_default();
    let attributes = if end_text[end..].starts_with('{') {
        parse_attributes(&end_text[end..]).map(|(attributes, len)| {
            end += len;
            attributes
        })
    } else {
        None
    };

    Some(InlineMatch {
        name: name.to_string(),
        start,
        label,
        attributes,
        end: (end_node, end),
    })
}

/// Finds the `]` closing a label starting at `label_start` in `node`, in it or
/// in the text nodes following it.
fn find_label<'a>(
    node: &'a AstNode<'a>,
    text: &str,
    label_start: usize,
    single_line: bool,
) -> Option<(usize, &'a AstNode<'a>, usize)> {
    let mut depth = match scan_brackets(&text[label_start..], 1) {
        Brackets::Closed(offset) => return Some((label_start, node, label_start + offset)),
        Brackets::TooDeep => return None,
        Brackets::Open(depth) => depth,
    };
    let mut sibling = node.next_sibling();
    while let Some(current) = sibling {
        if single_line && is_line_break(current) {
            return None;
        }
        if let Some(literal) = text_literal(current) {
            match scan_brackets(&literal, depth) {
                Brackets::Closed(close) => return Some((label_start, current, close)),
                Brackets::TooDeep => return None,
                Brackets::Open(open) => depth = open,
            }
        }
        sibling = current.next_sibling();
    }
    None
}

/// Replaces the matched directive with a placeholder node holding its label,
/// inserted after `node`, and returns it with the text node following it.
fn extract<'a>(
    node: &'a AstNode<'a>,
    text: &str,
    found: InlineMatch<'a>,
    kind: Kind,
    arena: &'a Arena<'a>,
    directives: &mut Directives,
) -> (&'a AstNode<'a>, &'a AstNode<'a>) {
    let directive_node: &AstNode = arena.alloc(AstNode::from(NodeValue::Raw(String::new())));
    node.insert_after(directive_node);

    let (end_node, end) = found.end;
    let end_text = text_literal(end_node).unwrap_or_default();
    let after_text = end_text[end..].to_string();

    if let Some((label_start, close_node, close)) = found.label {
        if std::ptr::eq(close_node, node) {
            push_text(directive_node, &text[label_start..close], arena);
        } else {
            push_text(directive_node, &text[label_start..], arena);
            let mut sibling = directive_node.next_sibling();
            while let Some(current) = sibling {
                if std::ptr::eq(current, close_node) {
                    break;
                }
                sibling = current.next_sibling();
                directive_node.append(current);
            }
            push_text(directive_node, &end_text[..close], arena);
        }
    }

    let after = if std::ptr::eq(end_node, node) {
        let after: &AstNode = arena.alloc(AstNode::from(NodeValue::Text(Cow::Owned(after_text))));
        directive_node.insert_after(after);
        after
    } else {
        set_text(end_node, after_text);
        end_node
    };
    set_text(node, text[..found.start].to_string());

    directives.insert(
        key(directive_node),
        Directive {
            kind,
            name: found.name,
            label: None,
            has_label: found.label.is_some(),
            attributes: found.attributes.unwrap_or_default(),
        },
    );
    (directive_node, after)
}

fn push_text<'a>(parent: &'a AstNode<'a>, text: &str, arena: &'a Arena<'a>) {
    if !text.is_empty() {
        parent.append(arena.alloc(AstNode::from(NodeValue::Text(Cow::Owned(text.to_string())))));
    }
}

/// Extracts the first text directive of the text node `node`, returning its
/// placeholder node and the node to continue scanning from.
fn text_directive<'a>(
    node: &'a AstNode<'a>,
    arena: &'a Arena<'a>,
    directives: &mut Directives,
    has_template: &dyn Fn(&str) -> bool,
) -> Option<(&'a AstNode<'a>, &'a AstNode<'a>)> {
    let text = text_literal(node)?;
    // An escaped `\:` is a separate node, so it doesn't count as a `:` here
    let previous_char = node
        .previous_sibling()
        .and_then(text_literal)
        .and_then(|previous| previous.chars().next_back());
    let found = text.match_indices(':').find_map(|(start, _)| {
        let previous = match start {
            0 => previous_char,
            _ => text[..start].chars().next_back(),
        };
        if previous == Some(':') {
            return None;
        }
        let found = match_inline(node, &text, start, false)?;
        (!found.is_simple(node) || has_template(&found.name)).then_some(found)
    })?;
    Some(extract(node, &text, found, Kind::Text, arena, directives))
}

/// Turns the line starting at `first` into a leaf directive placeholder if it
/// is a single `::name[label]{attributes}`, returning it in place of the line.
fn leaf_directive<'a>(
    first: &'a AstNode<'a>,
    arena: &'a Arena<'a>,
    directives: &mut Directives,
) -> Option<&'a AstNode<'a>> {
    let text = text_literal(first)?;
    if !text.starts_with("::") {
        return None;
    }
    let found = match_inline(first, &text, 1, true)?;
    let (end_node, end) = found.end;
    let at_line_end = end_node.next_sibling().is_none_or(is_line_break)
        && text_literal(end_node)
            .is_some_and(|literal| literal[end..].trim_matches([' ', '\t']).is_empty());
    if !at_line_end {
        return None;
    }

    let (directive_node, after) = extract(first, &text, found, Kind::Leaf, arena, directives);
    first.detach();
    after.detach();
    Some(directive_node)
}

/// Splits a paragraph around its leaf directive lines, which stand on their
/// own like other blocks. Returns the paragraphs and leaf directives it was
/// split into, in order.
fn split_leaf_directives<'a>(
    paragraph: &'a AstNode<'a>,
    arena: &'a Arena<'a>,
    directives: &mut Directives,
) -> Vec<&'a AstNode<'a>> {
    let mut parts = vec![paragraph];
    let mut current = paragraph;
    let mut line = paragraph.first_child();
    while let Some(first) = line {
        let Some(leaf) = leaf_directive(first, arena, directives) else {
            let mut node = first.next_sibling();
            while let Some(n) = node.filter(|n| !is_line_break(n)) {
                node = n.next_sibling();
            }
            line = node.and_then(comrak::arena_tree::Node::next_sibling);
            continue;
        };

        if let Some(line_break) = leaf.previous_sibling() {
            line_break.detach();
        }
        let next_line = leaf.next_sibling().and_then(|line_break| {
            let next = line_break.next_sibling();
            line_break.detach();
            next
        });
        current.insert_after(leaf);
        let rest = next_line.map(|next_line| {
            let rest: &AstNode = arena.alloc(AstNode::from(NodeValue::Paragraph));
            leaf.insert_after(rest);
            let mut node = Some(next_line);
            while let Some(n) = node {
                node = n.next_sibling();
                rest.append(n);
            }
            rest
        });
        if current.first_child().is_none() {
            current.detach();
            parts.pop();
        }
        parts.push(leaf);
        parts.extend(rest);
        line = rest.and_then(comrak::arena_tree::Node::first_child);
        if let Some(rest) = rest {
            current = rest;
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_container_info() {
        let directive =
            parse_container_info("note[My *title*]{#x .big .wide key=\"a b\" flag}").unwrap();
        assert_eq!(directive.kind, Kind::Container);
        assert_eq!(directive.name, "note");
        assert_eq!(directive.label.as_deref(), Some("My *title*"));
        assert_eq!(directive.attributes.id.as_deref(), Some("x"));
        assert_eq!(directive.attributes.classes, ["big", "wide"]);
        assert_eq!(directive.attributes.attributes["key"], "a b");
        assert_eq!(directive.attributes.attributes["flag"], "");
    }

    #[test]
    fn test_parse_container_info_rejects_invalid() {
        assert!(parse_container_info("note Some title").is_none());
        assert!(parse_container_info("").is_none());
        assert!(parse_container_info("note[unclosed").is_none());
        assert!(parse_container_info("note{k=\"v\"x}").is_none());
    }

    #[test]
    fn test_parse_name() {
        assert_eq!(parse_name("a-b_c[x]"), Some(("a-b_c", "[x]")));
        assert_eq!(parse_name("1x"), Some(("1x", "")));
        assert_eq!(parse_name("über!"), Some(("über", "!")));
        assert_eq!(parse_name("a.b"), Some(("a", ".b")));
        assert_eq!(parse_name("a-"), None);
        assert_eq!(parse_name("a_[x]"), None);
        assert_eq!(parse_name("-a"), None);
        assert_eq!(parse_name("_a"), None);
        assert_eq!(parse_name("€"), None);
        assert_eq!(parse_name(" a"), None);
    }

    #[test]
    fn test_parse_attributes() {
        let (attributes, len) = parse_attributes("{id=a class='b c' k=v}rest").unwrap();
        assert_eq!(len, "{id=a class='b c' k=v}".len());
        assert_eq!(attributes.id.as_deref(), Some("a"));
        assert_eq!(attributes.classes, ["b", "c"]);
        assert_eq!(attributes.attributes["k"], "v");
    }

    #[test]
    fn test_parse_attributes_like_micromark() {
        let (attributes, _) = parse_attributes("{#a #b .x class=y .z}").unwrap();
        assert_eq!(attributes.id.as_deref(), Some("b"));
        assert_eq!(attributes.classes, ["x", "y", "z"]);

        let (attributes, _) = parse_attributes("{#x.y}").unwrap();
        assert_eq!(attributes.id.as_deref(), Some("x"));
        assert_eq!(attributes.classes, ["y"]);

        let (attributes, _) = parse_attributes("{k = v 1=a data-x:y.z}").unwrap();
        assert_eq!(attributes.attributes["k"], "v");
        assert_eq!(attributes.attributes["1"], "a");
        assert_eq!(attributes.attributes["data-x:y.z"], "");
    }

    #[test]
    fn test_parse_attributes_rejects_invalid() {
        assert!(parse_attributes("{k=\"unclosed}").is_none());
        assert!(parse_attributes("{unclosed").is_none());
        assert!(parse_attributes("{k=\"v\"w}").is_none());
        assert!(parse_attributes("{k=`v`}").is_none());
        assert!(parse_attributes("{!}").is_none());
        assert!(parse_attributes("{#}").is_none());
        assert!(parse_attributes("{.a\"}").is_none());
    }

    #[test]
    fn test_label_nesting_limit() {
        let nested = format!("{}x{}]", "[".repeat(32), "]".repeat(32));
        assert!(matches!(scan_brackets(&nested, 1), Brackets::Closed(_)));
        let too_deep = format!("{}x{}]", "[".repeat(33), "]".repeat(33));
        assert!(matches!(scan_brackets(&too_deep, 1), Brackets::TooDeep));
    }

    #[test]
    fn test_nested_label_brackets() {
        let directive = parse_container_info("tip[a [b] c]").unwrap();
        assert_eq!(directive.label.as_deref(), Some("a [b] c"));
    }
}
