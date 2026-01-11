use serde::Deserialize;
use serde_json::from_str;
use std::{
    fs,
    path::{Path, PathBuf},
};

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

pub fn parse_manifest(path: PathBuf, root: &Path) -> ViteManifest {
    let absolute_path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    let config_content = fs::read_to_string(&absolute_path).expect("Vite manifest not found");
    let parsed_config: ViteManifest = from_str(&config_content).expect("Could not parse config");

    println!("{:?}", parsed_config);

    parsed_config
}
