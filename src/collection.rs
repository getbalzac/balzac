use comrak::{Options, markdown_to_html};
use serde_json::{Value, json};

pub struct MarkdownOutput {
    pub content: String,
    pub fm: Value,
}

pub fn parse_markdown(file_content: &str) -> std::io::Result<MarkdownOutput> {
    let (frontmatter_yaml, markdown_content) = extract_frontmatter(file_content);

    let fm = match frontmatter_yaml {
        Some(yaml) => parse_yaml_to_json(yaml)?,
        None => json!(null),
    };

    let options = build_comrak_options();
    let html = markdown_to_html(markdown_content, &options);

    Ok(MarkdownOutput { content: html, fm })
}

fn extract_frontmatter(content: &str) -> (Option<&str>, &str) {
    let content = content.trim_start();

    if !content.starts_with("---") {
        return (None, content);
    }

    let after_opening = &content[3..];
    let after_opening = after_opening.trim_start_matches(['\r', '\n']);

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

fn parse_yaml_to_json(yaml: &str) -> std::io::Result<Value> {
    serde_yaml::from_str(yaml).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Could not parse frontmatter YAML: {}", e),
        )
    })
}

fn build_comrak_options() -> Options<'static> {
    let mut options = Options::default();

    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
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
    fn test_frontmatter_with_dashes_in_multiline_yaml() {
        // --- on its own line within a multiline YAML string (preceded by non-newline)
        let input = "---\ntitle: Test\ncode: |\n  some code\n  ---not-a-delimiter\n  more code\n---\n\nContent";
        let (fm, content) = extract_frontmatter(input);
        assert!(fm.is_some());
        assert!(fm.unwrap().contains("---not-a-delimiter"));
        assert_eq!(content, "Content");
    }

    #[test]
    fn test_no_frontmatter() {
        let input = "# Just markdown\n\nNo frontmatter here.";
        let (fm, content) = extract_frontmatter(input);
        assert!(fm.is_none());
        assert_eq!(content, input);
    }

    #[test]
    fn test_parse_markdown_preserves_frontmatter_values() {
        let input = "---\ntitle: Hello World\ncount: 42\n---\n\n# Heading";
        let result = parse_markdown(input).unwrap();
        assert_eq!(result.fm["title"], "Hello World");
        assert_eq!(result.fm["count"], 42);
        assert!(result.content.contains("<h1>"));
    }
}
