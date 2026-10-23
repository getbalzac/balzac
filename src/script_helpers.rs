//! User-defined template helpers written in [Rhai](https://rhai.rs): every
//! `<helpers_directory>/<name>.rhai` file is registered as the helper `<name>`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use handlebars::{
    Context, Handlebars, Helper, HelperDef, PathAndJson, RenderContext, RenderError,
    RenderErrorReason, ScopedJson,
};
use rhai::serde::{from_dynamic, to_dynamic};
use rhai::{AST, Dynamic, Scope};
use serde_json::Value as Json;

use crate::error::{self, Error, Result};

const SCRIPT_EXTENSION: &str = "rhai";

/// A compiled helper script. Unlike handlebars' own script helpers, it knows
/// its source file, so runtime errors point to the script that failed.
struct ScriptHelper {
    /// Script path as shown in messages, like other build errors.
    display_path: String,
    ast: AST,
}

impl ScriptHelper {
    fn error(&self, name: &str, error: &impl std::fmt::Display) -> RenderError {
        RenderErrorReason::Other(format!(
            "helper `{name}` ({}) failed: {error}",
            self.display_path
        ))
        .into()
    }
}

impl HelperDef for ScriptHelper {
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'rc>,
        r: &'reg Handlebars<'reg>,
        _: &'rc Context,
        _: &mut RenderContext<'reg, 'rc>,
    ) -> Result<ScopedJson<'rc>, RenderError> {
        let params: Vec<&Json> = h.params().iter().map(PathAndJson::value).collect();
        let hash: HashMap<&str, &Json> = h.hash().iter().map(|(k, v)| (*k, v.value())).collect();

        let mut scope = Scope::new();
        scope.push_dynamic(
            "params",
            to_dynamic(params).map_err(|e| self.error(h.name(), &e))?,
        );
        scope.push_dynamic(
            "hash",
            to_dynamic(hash).map_err(|e| self.error(h.name(), &e))?,
        );

        let result = r
            .engine()
            .eval_ast_with_scope::<Dynamic>(&mut scope, &self.ast)
            .map_err(|e| self.error(h.name(), &e))?;
        let json: Json = from_dynamic(&result).map_err(|e| self.error(h.name(), &e))?;
        Ok(ScopedJson::Derived(json))
    }
}

/// Registers every `.rhai` file of `directory` as a helper named after the
/// file stem. A missing directory registers nothing.
pub fn register_script_helpers(registry: &mut Handlebars<'_>, directory: &Path) -> Result<()> {
    if !fs::exists(directory)? {
        log::debug!("Could not find helpers directory, skipping script helpers");
        return Ok(());
    }

    let mut paths: Vec<PathBuf> = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<_>>()?;
    paths.sort();

    for path in paths {
        if path.extension().and_then(|e| e.to_str()) != Some(SCRIPT_EXTENSION) || !path.is_file() {
            log::debug!(
                "Skipping non-rhai file {} in helpers directory",
                path.display()
            );
            continue;
        }
        let Some(name) = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
        else {
            continue;
        };
        let display_path = error::display_path(&path);

        let source = error::read_to_string(&path)?;
        let ast = registry
            .engine()
            .compile(&source)
            .map_err(|e| Error::Script {
                path: path.clone(),
                message: e.to_string(),
            })?;

        log::debug!("Registering script helper `{name}` from {display_path}");
        registry.register_helper(&name, Box::new(ScriptHelper { display_path, ast }));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn registry_with(scripts: &[(&str, &str)]) -> (TempDir, Result<Handlebars<'static>>) {
        let root = TempDir::new().unwrap();
        let helpers = root.path().join("helpers");
        fs::create_dir(&helpers).unwrap();
        for (file, script) in scripts {
            fs::write(helpers.join(file), script).unwrap();
        }
        let mut registry = Handlebars::new();
        let result = register_script_helpers(&mut registry, &helpers);
        (root, result.map(|()| registry))
    }

    #[test]
    fn test_script_helper_receives_params_and_hash() {
        let (_root, registry) = registry_with(&[(
            "greet.rhai",
            r#"let greeting = if hash.formal == true { "Good day" } else { "Hi" };
               greeting + ", " + params[0] + "!""#,
        )]);
        let registry = registry.unwrap();

        let data = serde_json::json!({ "name": "Ada" });
        assert_eq!(
            registry.render_template("{{greet name}}", &data).unwrap(),
            "Hi, Ada!"
        );
        assert_eq!(
            registry
                .render_template("{{greet name formal=true}}", &data)
                .unwrap(),
            "Good day, Ada!"
        );
    }

    #[test]
    fn test_script_helper_returns_json_values() {
        let (_root, registry) = registry_with(&[("tags.rhai", r#"["a", "b"]"#)]);
        let registry = registry.unwrap();

        assert_eq!(
            registry
                .render_template("{{#each (tags)}}[{{this}}]{{/each}}", &())
                .unwrap(),
            "[a][b]"
        );
    }

    #[test]
    fn test_non_rhai_files_are_ignored() {
        let (_root, registry) =
            registry_with(&[("shout.rhai", "params[0].to_upper()"), ("notes.txt", "{{")]);
        let registry = registry.unwrap();

        assert_eq!(
            registry.render_template("{{shout \"hey\"}}", &()).unwrap(),
            "HEY"
        );
    }

    #[test]
    fn test_missing_directory_registers_nothing() {
        let root = TempDir::new().unwrap();
        let mut registry = Handlebars::new();
        register_script_helpers(&mut registry, &root.path().join("helpers")).unwrap();
    }

    #[test]
    fn test_compile_error_names_helper_file() {
        let (root, registry) = registry_with(&[("broken.rhai", "let x = ;")]);
        let error = registry.err().unwrap().to_string();
        let path = error::display_path(&root.path().join("helpers/broken.rhai"));

        assert!(
            error.starts_with(&format!("invalid helper script {path}: ")),
            "{error}"
        );
        assert!(error.contains("line 1"), "{error}");
    }

    #[test]
    fn test_runtime_error_names_helper_file() {
        let (root, registry) = registry_with(&[("boom.rhai", "params[0].no_such_fn()")]);
        let path = error::display_path(&root.path().join("helpers/boom.rhai"));
        let registry = registry.unwrap();

        let error = registry
            .render_template("{{boom 1}}", &())
            .unwrap_err()
            .to_string();
        // Same path as compile errors and the other build errors
        assert!(
            error.contains(&format!("helper `boom` ({path}) failed")),
            "{error}"
        );
        assert!(error.contains("no_such_fn"), "{error}");
    }
}
