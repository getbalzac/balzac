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

    if let Some(end_pos) = after_opening.find("\n---") {
        let frontmatter = &after_opening[..end_pos];
        let rest = &after_opening[end_pos + 1..];
        let rest = rest.trim_start_matches('-');
        let rest = rest.trim_start_matches(['\r', '\n']);

        return (Some(frontmatter.trim()), rest);
    }

    (None, content)
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
