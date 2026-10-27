use comrak::Options;

use crate::config::MarkdownConfig;
use serde_json::{Value, json};
use serde_saphyr::options::{DuplicateKeyPolicy, MergeKeyPolicy, NonFiniteFloatPolicy};

pub struct MarkdownOutput {
    pub content: String,
    pub fm: Value,
}

/// Splits an optional leading `---` YAML frontmatter block from a template.
///
/// Returns `None` and the untouched input when there is no frontmatter block.
pub fn parse_template_frontmatter(
    file_content: &str,
) -> Result<(Option<Value>, &str), serde_saphyr::Error> {
    match extract_frontmatter(file_content) {
        (Some(yaml), body) => Ok((Some(parse_yaml(yaml)?), body)),
        (None, _) => Ok((None, file_content)),
    }
}

pub fn parse_markdown(file_content: &str) -> Result<MarkdownOutput, serde_saphyr::Error> {
    let (frontmatter_yaml, markdown_content) = extract_frontmatter(file_content);

    let fm = match frontmatter_yaml {
        Some(yaml) => parse_yaml(yaml)?,
        None => json!(null),
    };

    Ok(MarkdownOutput {
        content: markdown_content.to_string(),
        fm,
    })
}

/// Parses frontmatter with the scalar rules `serde_yaml` 0.9 used.
fn parse_yaml(yaml: &str) -> Result<Value, serde_saphyr::Error> {
    serde_saphyr::from_str_with_options(
        yaml,
        serde_saphyr::options! {
            // `yes`/`no`/`on`/`off` stay strings
            strict_booleans: true,
            duplicate_keys: DuplicateKeyPolicy::LastWins,
            merge_keys: MergeKeyPolicy::AsOrdinary,
            // `.inf`/`.nan` become null instead of failing the page
            non_finite_float_policy: NonFiniteFloatPolicy::PassThrough,
            with_snippet: false,
        },
    )
}

fn extract_frontmatter(content: &str) -> (Option<&str>, &str) {
    let content = content.trim_start();

    if !content.starts_with("---") {
        return (None, content);
    }

    // Keep the newline after the opening "---" so an immediately following
    // closing "---" (an empty block) is found like any other closing line.
    let after_opening = &content[3..];

    // Find closing "---" that is on its own line (handles both LF and CRLF)
    if let Some(end_pos) = find_closing_delimiter(after_opening) {
        let frontmatter = &after_opening[..end_pos];
        let rest = &after_opening[end_pos..];
        // Skip the newline before ---, the --- itself, and any trailing newline
        let rest = rest.trim_start_matches(['\r', '\n']);
        let rest = rest.strip_prefix("---").unwrap_or(rest);
        let rest = rest.trim_start_matches(['\r', '\n']);

        return (Some(frontmatter.trim()), rest);
    }

    (None, content)
}

/// Finds the closing `---` delimiter that appears on its own line.
/// Returns the position of the newline before `---`, or None if not found.
fn find_closing_delimiter(content: &str) -> Option<usize> {
    let mut pos = 0;
    while pos < content.len() {
        // Look for newline followed by ---
        if let Some(newline_pos) = content[pos..].find('\n') {
            let abs_pos = pos + newline_pos;
            let after_newline = abs_pos + 1;

            // Skip optional \r after finding \n (handles \r\n case where we found \n)
            // Actually we need to check what comes after the \n
            let check_pos = after_newline;

            if check_pos < content.len() && content[check_pos..].starts_with("---") {
                let after_dashes = check_pos + 3;
                // Verify --- is followed by newline or end of content
                if after_dashes >= content.len()
                    || content[after_dashes..].starts_with('\n')
                    || content[after_dashes..].starts_with("\r\n")
                {
                    return Some(abs_pos);
                }
            }
            pos = after_newline;
        } else {
            break;
        }
    }
    None
}

#[must_use]
pub fn build_comrak_options(markdown: &MarkdownConfig) -> Options<'static> {
    let mut options = Options::default();

    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.footnotes = markdown.footnotes;
    options.render.unsafe_ = true;

    options
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frontmatter_with_crlf() {
        let input = "---\r\ntitle: Test\r\n---\r\n\r\nContent";
        let (fm, content) = extract_frontmatter(input);
        assert_eq!(fm, Some("title: Test"));
        assert_eq!(content, "Content");
    }

    #[test]
    fn test_frontmatter_with_lf() {
        let input = "---\ntitle: Test\n---\n\nContent";
        let (fm, content) = extract_frontmatter(input);
        assert_eq!(fm, Some("title: Test"));
        assert_eq!(content, "Content");
    }

    #[test]
    fn test_frontmatter_with_dashes_in_yaml_value() {
        // --- appearing within a YAML value should not be treated as closing delimiter
        let input = "---\ntitle: Test\ndescription: \"contains --- dashes\"\n---\n\nContent";
        let (fm, content) = extract_frontmatter(input);
        assert_eq!(
            fm,
            Some("title: Test\ndescription: \"contains --- dashes\"")
        );
        assert_eq!(content, "Content");
    }

    #[test]
    fn test_frontmatter_with_indented_dashes_in_multiline_yaml() {
        // Indented --- (with leading spaces) should not match as closing delimiter
        let input =
            "---\ntitle: Test\ncode: |\n  some code\n  ---indented\n  more code\n---\n\nContent";
        let (fm, content) = extract_frontmatter(input);
        assert!(fm.is_some());
        assert!(fm.unwrap().contains("---indented"));
        assert_eq!(content, "Content");
    }

    #[test]
    fn test_empty_frontmatter_block() {
        let (fm, content) = extract_frontmatter("---\n---\n<h1>Home</h1>");
        assert_eq!(fm, Some(""));
        assert_eq!(content, "<h1>Home</h1>");

        let (fm, content) = extract_frontmatter("---\r\n---\r\nContent");
        assert_eq!(fm, Some(""));
        assert_eq!(content, "Content");
    }

    #[test]
    fn test_parse_template_frontmatter_empty_block_is_stripped() {
        let (fm, body) = parse_template_frontmatter("---\n---\n<h1>Home</h1>").unwrap();
        assert!(fm.is_some());
        assert_eq!(body, "<h1>Home</h1>");
    }

    #[test]
    fn test_no_frontmatter() {
        let input = "# Just markdown\n\nNo frontmatter here.";
        let (fm, content) = extract_frontmatter(input);
        assert!(fm.is_none());
        assert_eq!(content, input);
    }

    #[test]
    fn test_parse_template_frontmatter() {
        let input = "---\ntitle: About\n---\n<h1>{{fm.title}}</h1>";
        let (fm, body) = parse_template_frontmatter(input).unwrap();
        assert_eq!(fm.unwrap()["title"], "About");
        assert_eq!(body, "<h1>{{fm.title}}</h1>");
    }

    #[test]
    fn test_parse_template_frontmatter_without_block_keeps_input() {
        let input = "\n  <h1>Home</h1>";
        let (fm, body) = parse_template_frontmatter(input).unwrap();
        assert!(fm.is_none());
        assert_eq!(body, input);
    }

    #[test]
    fn test_parse_template_frontmatter_invalid_yaml() {
        let input = "---\ntitle: [unclosed\n---\n<h1>Home</h1>";
        assert!(parse_template_frontmatter(input).is_err());
    }

    #[test]
    fn test_parse_markdown_preserves_frontmatter_values() {
        let input = "---\ntitle: Hello World\ncount: 42\n---\n\n# Heading";
        let result = parse_markdown(input).unwrap();
        assert_eq!(result.fm["title"], "Hello World");
        assert_eq!(result.fm["count"], 42);
        assert!(result.content.contains("# Heading"));
    }

    fn fm(yaml: &str) -> Value {
        parse_markdown(&format!("---\n{yaml}\n---\nbody"))
            .unwrap()
            .fm
    }

    #[test]
    fn test_frontmatter_yaml_1_1_booleans_stay_strings() {
        let fm = fm("a: yes\nb: no\nc: on\nd: off\ne: y\nf: NO\ng: true\nh: FALSE");
        assert_eq!(
            fm,
            json!({"a": "yes", "b": "no", "c": "on", "d": "off", "e": "y", "f": "NO", "g": true, "h": false})
        );
    }

    #[test]
    fn test_frontmatter_dates_and_times_stay_strings() {
        let fm = fm("date: 2024-01-15\nat: 2024-01-15T10:00:00Z\ntime: 12:30:45");
        assert_eq!(
            fm,
            json!({"date": "2024-01-15", "at": "2024-01-15T10:00:00Z", "time": "12:30:45"})
        );
    }

    #[test]
    fn test_frontmatter_duplicate_key_keeps_last_value() {
        assert_eq!(fm("a: 1\na: 2"), json!({"a": 2}));
    }

    #[test]
    fn test_frontmatter_merge_key_is_an_ordinary_key() {
        let fm = fm("base: &b {x: 1}\nmerged:\n  <<: *b\n  y: 3");
        assert_eq!(fm["merged"], json!({"<<": {"x": 1}, "y": 3}));
    }

    #[test]
    fn test_frontmatter_non_finite_floats_are_null() {
        assert_eq!(fm("a: .inf\nb: .nan"), json!({"a": null, "b": null}));
    }

    #[test]
    fn test_frontmatter_numbers_nulls_and_aliases() {
        let fm = fm("a: 0o17\nb: 0x1F\nc: 1.10\nd: ~\ne:\nf: &x [1]\ng: *x\n1: one");
        assert_eq!(
            fm,
            json!({"a": 15, "b": 31, "c": 1.1, "d": null, "e": null, "f": [1], "g": [1], "1": "one"})
        );
    }

    #[test]
    fn test_frontmatter_comment_only_block_is_null() {
        assert_eq!(fm("# draft"), json!(null));
    }

    #[test]
    fn test_parse_markdown_invalid_yaml_reports_location() {
        let input = "---\ntitle: Test\ntags: [unclosed\ndate: 2024\n---\nbody";
        let message = parse_markdown(input).err().unwrap().to_string();
        assert!(message.contains("line 3"), "{message}");
        assert!(!message.contains('\n'), "{message}");
    }
}
