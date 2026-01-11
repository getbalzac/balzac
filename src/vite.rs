use serde::Deserialize;
use serde_json::from_str;
use std::{fs, path::PathBuf};

#[derive(Debug, Deserialize, Default)]
pub struct ViteChunk {
    pub file: Option<String>,
    pub src: Option<String>,
    pub is_entry: Option<bool>,
    pub imports: Option<Vec<String>>,
    pub css: Option<Vec<String>>,
    pub dynamic_imports: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ViteManifest {
    #[serde(flatten)]
    pub chunks: std::collections::HashMap<String, ViteChunk>,
}

pub fn parse_manifest(path: PathBuf) -> ViteManifest {
    let config_content = fs::read_to_string(path).expect("Vite manifest not found");
    let parsed_config: ViteManifest = from_str(&config_content).expect("Could not parse config");

    println!("{:?}", parsed_config);

    parsed_config
}
