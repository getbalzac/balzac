use std::collections::HashMap;

use gray_matter::{Matter, ParsedEntity, Pod, engine::YAML};
use serde_json::{self, Value, json};

pub struct MarkdownOutput {
    pub content: String,
    pub fm: Value,
}

pub fn parse_markdown(file_content: &str) -> std::io::Result<MarkdownOutput> {
    let matter = Matter::<YAML>::new();
    let parsed: ParsedEntity = matter
        .parse(file_content)
        .expect("Could not get frontmatter");
    let parsed_md = markdown::to_html_with_options(&parsed.content, &markdown::Options::gfm())
        .expect("Could not parse markdown file");
    let fm = get_frontmatter(&parsed);
    Ok(MarkdownOutput {
        content: parsed_md,
        fm,
    })
}

fn pod_to_json(pod: &Pod) -> Value {
    match pod {
        Pod::Null => json!(null),
        Pod::String(s) => json!(s),
        Pod::Integer(i) => json!(i),
        Pod::Float(f) => json!(f),
        Pod::Boolean(b) => json!(b),
        Pod::Array(arr) => {
            let json_array: Vec<Value> = arr.iter().map(pod_to_json).collect();
            json!(json_array)
        }
        Pod::Hash(map) => {
            let json_map: HashMap<String, Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), pod_to_json(v)))
                .collect();
            json!(json_map)
        }
    }
}

fn get_frontmatter(parsed: &ParsedEntity<Pod>) -> serde_json::Value {
    parsed.data.as_ref().map(pod_to_json).unwrap_or(json!(null))
}
