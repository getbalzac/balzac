use serde::Deserialize;
use serde_json::from_str;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[allow(non_camel_case_types)]
pub struct vite_url {
    pub manifest: ViteManifest,
}

impl handlebars::HelperDef for vite_url {
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &handlebars::Helper<'rc>,
        r: &'reg handlebars::Handlebars<'reg>,
        _: &'rc handlebars::Context,
        _: &mut handlebars::RenderContext<'reg, 'rc>,
    ) -> std::result::Result<handlebars::ScopedJson<'rc>, handlebars::RenderError> {
        let param_idx = 0;
        let name = h
            .param(param_idx)
            .and_then(|x| {
                if r.strict_mode() && x.is_value_missing() {
                    None
                } else {
                    Some(x.value())
                }
            })
            .ok_or_else(|| {
                handlebars::RenderErrorReason::ParamNotFoundForName(
                    stringify!(vite_url),
                    "name".to_string(),
                )
            })
            .and_then(|x| {
                handlebars::handlebars_helper!(@as_json_value x,str).ok_or_else(|| {
                    handlebars::RenderErrorReason::ParamTypeMismatchForName(
                        stringify!(vite_url),
                        "name".to_string(),
                        "str".to_string(),
                    )
                })
            })?;
        let result = get_file(&self.manifest, name).map_err(|e| {
            handlebars::RenderError::from(handlebars::RenderErrorReason::Other(format!(
                "Asset not found in vite manifest: {}",
                e
            )))
        })?;
        Ok(handlebars::ScopedJson::Derived(
            handlebars::JsonValue::from(result),
        ))
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct ViteChunk {
    pub file: Option<String>,
    pub src: Option<String>,
    #[serde(rename = "isEntry")]
    pub is_entry: Option<bool>,
    pub imports: Option<Vec<String>>,
    pub css: Option<Vec<String>>,
    #[serde(rename = "dynamicImports")]
    pub dynamic_imports: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ViteManifest {
    #[serde(flatten)]
    pub chunks: std::collections::HashMap<String, ViteChunk>,
}

pub fn get_file(manifest: &ViteManifest, name: &str) -> std::io::Result<String> {
    if let Some(chunk) = manifest.chunks.get(name)
        && let Some(file) = &chunk.file
    {
        Ok(file.clone())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("file not found: {}", name),
        ))
    }
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
