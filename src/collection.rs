use std::collections::HashMap;

use gray_matter::{Matter, ParsedEntity, Pod, engine::YAML};
use serde_json::{self, Value, json};

pub fn parse_markdown(file_content: &str) -> std::io::Result<String> {
    let parsed_md = markdown::to_html_with_options(file_content, &markdown::Options::gfm())
        .expect("Could not parse markdown file");
    Ok(parsed_md)
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

pub fn get_frontmatter(content: &str) -> serde_json::Value {
    let matter = Matter::<YAML>::new();
    let parsed: ParsedEntity = matter.parse(content).expect("Could not get frontmatter");

    parsed.data.as_ref().map(pod_to_json).unwrap_or(json!(null))
}
