//! Content-hashed asset names, enabled with `[assets] fingerprint = true`:
//! `css/site.css` is published as `css/site.3f2a1b9c.css`, so browsers can
//! cache assets forever and still pick up changes.
//!
//! The hash is the first 8 hex digits of the SHA-256 of the published
//! contents. Stylesheets have their relative `url(...)` references rewritten
//! to the fingerprinted names before they are hashed, so a changed image also
//! changes the name of the stylesheet using it.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use sha2::{Digest, Sha256};

/// Name of the manifest written to `<output>/assets/` with `[assets] manifest = true`.
pub const MANIFEST_FILE: &str = "manifest.json";

/// Number of bytes of the SHA-256 kept in names (two hex digits each).
const HASH_BYTES: usize = 4;

/// An asset as published with fingerprinting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fingerprinted {
    /// Path relative to the assets directory, e.g. `css/site.3f2a1b9c.css`.
    pub path: String,
    /// Contents to publish when they differ from the source file: a
    /// stylesheet with rewritten `url(...)` references.
    pub rewritten: Option<String>,
}

/// Fingerprinted names of the files in an assets directory, computed on
/// demand and cached.
pub struct Fingerprints {
    assets_directory: PathBuf,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// Normalized relative path -> published asset, `None` when no such file exists.
    resolved: HashMap<String, Option<Fingerprinted>>,
    /// Stylesheet -> assets its `url(...)` references point to.
    references: HashMap<String, Vec<String>>,
}

impl Fingerprints {
    #[must_use]
    pub fn new(assets_directory: &Path) -> Self {
        Self {
            assets_directory: assets_directory.to_path_buf(),
            state: Mutex::default(),
        }
    }

    /// The published form of `path`, relative to the assets directory
    /// (`css/site.css`, `/css/site.css`). `None`, with a warning the first
    /// time, when it is not a file of the assets directory.
    pub fn get(&self, path: &str) -> io::Result<Option<Fingerprinted>> {
        let Some(path) = normalize(path) else {
            log::warn!("Asset {path} is outside the assets directory, it is not fingerprinted");
            return Ok(None);
        };
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        self.resolve(&mut state, &path)
    }

    /// The published form of the file at `source`, a path inside the assets
    /// directory. `None` for paths that are not valid UTF-8, which are
    /// published unfingerprinted.
    pub fn get_file(&self, source: &Path) -> io::Result<Option<Fingerprinted>> {
        let Some(path) = source
            .strip_prefix(&self.assets_directory)
            .ok()
            .and_then(|relative| {
                relative
                    .components()
                    .map(|component| component.as_os_str().to_str())
                    .collect::<Option<Vec<_>>>()
            })
        else {
            log::warn!("Asset {} is not fingerprinted", source.display());
            return Ok(None);
        };
        self.get(&path.join("/"))
    }

    /// Source path -> fingerprinted path of every asset resolved so far.
    #[must_use]
    pub fn manifest(&self) -> BTreeMap<String, String> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state
            .resolved
            .iter()
            .filter_map(|(source, asset)| Some((source.clone(), asset.as_ref()?.path.clone())))
            .collect()
    }

    fn resolve(&self, state: &mut State, path: &str) -> io::Result<Option<Fingerprinted>> {
        if let Some(asset) = state.resolved.get(path) {
            return Ok(asset.clone());
        }
        let source = self.assets_directory.join(path);
        if !source.is_file() {
            log::warn!(
                "Asset {path} not found in {}, it is not fingerprinted",
                self.assets_directory.display()
            );
            state.resolved.insert(path.to_string(), None);
            return Ok(None);
        }

        let mut contents = fs::read(&source)?;
        let mut rewritten = None;
        if is_stylesheet(path)
            && let Ok(css) = std::str::from_utf8(&contents)
        {
            let css_rewritten = rewrite_urls(css, path, &mut |target| {
                // References within a cycle are kept: each name would depend
                // on the other. Skipping them whichever file is resolved first
                // keeps the names independent of the order.
                if target == path {
                    log::warn!(
                        "Asset {path} references itself, the reference is not fingerprinted"
                    );
                    return Ok(None);
                }
                if self.reaches(state, target, path)? {
                    log::warn!(
                        "Assets {path} and {target} reference each other, the reference to {target} in {path} is not fingerprinted"
                    );
                    return Ok(None);
                }
                Ok(self.resolve(state, target)?.map(|asset| asset.path))
            })?;
            if css_rewritten != css {
                contents = css_rewritten.clone().into_bytes();
                rewritten = Some(css_rewritten);
            }
        }

        let asset = Fingerprinted {
            path: fingerprinted_name(path, &content_hash(&contents)),
            rewritten,
        };
        state.resolved.insert(path.to_string(), Some(asset.clone()));
        Ok(Some(asset))
    }

    /// Whether `to` is `from` or reachable from it through stylesheet references.
    fn reaches(&self, state: &mut State, from: &str, to: &str) -> io::Result<bool> {
        let mut pending = vec![from.to_string()];
        let mut seen = std::collections::HashSet::new();
        while let Some(path) = pending.pop() {
            if path == to {
                return Ok(true);
            }
            if seen.insert(path.clone()) {
                pending.extend(self.references(state, &path)?.iter().cloned());
            }
        }
        Ok(false)
    }

    /// Assets the `url(...)` references of the stylesheet at `path` point to;
    /// empty for other files.
    fn references<'s>(&self, state: &'s mut State, path: &str) -> io::Result<&'s [String]> {
        if !state.references.contains_key(path) {
            let mut references = Vec::new();
            let source = self.assets_directory.join(path);
            if is_stylesheet(path)
                && source.is_file()
                && let Ok(css) = String::from_utf8(fs::read(&source)?)
            {
                rewrite_urls(&css, path, &mut |target| {
                    references.push(target.to_string());
                    Ok(None)
                })?;
            }
            state.references.insert(path.to_string(), references);
        }
        Ok(&state.references[path])
    }
}

/// Short hex hash of `contents`.
fn content_hash(contents: &[u8]) -> String {
    Sha256::digest(contents)[..HASH_BYTES]
        .iter()
        .fold(String::new(), |mut hash, byte| {
            let _ = write!(hash, "{byte:02x}");
            hash
        })
}

/// `css/site.css` -> `css/site.<hash>.css`; `LICENSE` -> `LICENSE.<hash>`.
fn fingerprinted_name(path: &str, hash: &str) -> String {
    let (directory, name) = path.rsplit_once('/').map_or(("", path), |(d, n)| (d, n));
    let name = match name.rfind('.') {
        Some(dot) if dot > 0 => format!("{}.{hash}{}", &name[..dot], &name[dot..]),
        _ => format!("{name}.{hash}"),
    };
    if directory.is_empty() {
        name
    } else {
        format!("{directory}/{name}")
    }
}

/// Resolves `.` and `..` in a `/`-separated relative path. `None` when it
/// leaves the root or names the root itself.
fn normalize(path: &str) -> Option<String> {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            segment => segments.push(segment),
        }
    }
    (!segments.is_empty()).then(|| segments.join("/"))
}

fn is_stylesheet(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("css"))
}

/// Rewrites the relative `url(...)` references of the stylesheet at `path`
/// with `resolve`, which maps a normalized asset path to its fingerprinted
/// path. Absolute, root-relative, `data:` and fragment URLs are left as is.
fn rewrite_urls(
    css: &str,
    path: &str,
    resolve: &mut dyn FnMut(&str) -> io::Result<Option<String>>,
) -> io::Result<String> {
    let directory = path.rsplit_once('/').map_or("", |(d, _)| d);
    let mut output = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = find_url_function(rest) {
        let (before, after) = rest.split_at(start + "url(".len());
        output.push_str(before);
        let Some((offset, length)) = url_reference(after) else {
            rest = after;
            continue;
        };
        let reference = &after[offset..offset + length];
        output.push_str(&after[..offset]);
        match rewrite_reference(reference, directory, resolve)? {
            Some(rewritten) => output.push_str(&rewritten),
            None => output.push_str(reference),
        }
        rest = &after[offset + length..];
    }
    output.push_str(rest);
    Ok(output)
}

/// Byte index of the next `url(` (any case) that is not the end of a longer name.
fn find_url_function(css: &str) -> Option<usize> {
    let bytes = css.as_bytes();
    bytes.windows(4).enumerate().find_map(|(index, window)| {
        let starts_name = index == 0 || {
            let previous = bytes[index - 1];
            !(previous.is_ascii_alphanumeric() || matches!(previous, b'-' | b'_' | b'\\'))
        };
        (starts_name && window.eq_ignore_ascii_case(b"url(")).then_some(index)
    })
}

/// Offset and length of the reference in the arguments of a `url(`, quoted
/// or not, without the quotes and surrounding whitespace.
fn url_reference(arguments: &str) -> Option<(usize, usize)> {
    let offset = arguments.len() - arguments.trim_start().len();
    let rest = &arguments[offset..];
    if let quote @ ('"' | '\'') = rest.chars().next()? {
        let length = rest[1..].find(quote)?;
        Some((offset + 1, length))
    } else {
        let length = rest.find(')')?;
        Some((offset, rest[..length].trim_end().len()))
    }
}

/// `../img/logo.png?v=2` -> `../img/logo.<hash>.png?v=2`, or `None` to keep
/// the reference.
fn rewrite_reference(
    reference: &str,
    directory: &str,
    resolve: &mut dyn FnMut(&str) -> io::Result<Option<String>>,
) -> io::Result<Option<String>> {
    if reference.is_empty()
        || reference.starts_with(['#', '/'])
        || crate::helpers::is_external(reference)
    {
        return Ok(None);
    }
    let (file, suffix) = reference.split_at(reference.find(['?', '#']).unwrap_or(reference.len()));
    let Some(target) = normalize(&format!("{directory}/{file}")) else {
        return Ok(None);
    };
    let Some(fingerprinted) = resolve(&target)? else {
        return Ok(None);
    };
    let prefix = file.rsplit_once('/').map_or("", |(prefix, _)| prefix);
    let name = fingerprinted.rsplit('/').next().unwrap_or(&fingerprinted);
    let separator = if prefix.is_empty() { "" } else { "/" };
    Ok(Some(format!("{prefix}{separator}{name}{suffix}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn assets(files: &[(&str, &str)]) -> (TempDir, Fingerprints) {
        let temp = TempDir::new().unwrap();
        for (path, contents) in files {
            let path = temp.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        let fingerprints = Fingerprints::new(temp.path());
        (temp, fingerprints)
    }

    fn published(fingerprints: &Fingerprints, path: &str) -> String {
        fingerprints.get(path).unwrap().unwrap().path
    }

    #[test]
    fn test_content_hash_is_sha256_prefix() {
        // sha256("hello") = 2cf24dba5fb0a30e...
        assert_eq!(content_hash(b"hello"), "2cf24dba");
    }

    #[test]
    fn test_fingerprinted_name() {
        assert_eq!(
            fingerprinted_name("css/site.css", "abc"),
            "css/site.abc.css"
        );
        assert_eq!(fingerprinted_name("app.min.js", "abc"), "app.min.abc.js");
        assert_eq!(fingerprinted_name("LICENSE", "abc"), "LICENSE.abc");
        assert_eq!(fingerprinted_name("a/.htaccess", "abc"), "a/.htaccess.abc");
    }

    #[test]
    fn test_normalize() {
        assert_eq!(
            normalize("/css/./site.css").as_deref(),
            Some("css/site.css")
        );
        assert_eq!(normalize("css/../img/a.png").as_deref(), Some("img/a.png"));
        assert_eq!(normalize("../secret"), None);
        assert_eq!(normalize("/"), None);
    }

    #[test]
    fn test_get_hashes_contents() {
        let (_temp, fingerprints) = assets(&[("img/logo.png", "hello")]);
        let asset = fingerprints.get("/img/logo.png").unwrap().unwrap();
        assert_eq!(asset.path, "img/logo.2cf24dba.png");
        assert_eq!(asset.rewritten, None);
        assert_eq!(fingerprints.get("img/missing.png").unwrap(), None);
    }

    #[test]
    fn test_stylesheet_references_are_fingerprinted() {
        let (_temp, fingerprints) = assets(&[
            ("img/logo.png", "hello"),
            ("fonts/a.woff2", "font"),
            (
                "css/site.css",
                "a{background:url(../img/logo.png)}\n\
                 b{background:URL( \"../img/logo.png?v=1#x\" )}\n\
                 @font-face{src:url('../fonts/a.woff2') format('woff2')}\n\
                 c{background:url(data:image/png;base64,AAAA)}\n\
                 d{background:url(/assets/img/logo.png)}\n\
                 e{background:url(https://example.com/a.png)}\n\
                 f{filter:url(#shadow)}\n\
                 g{background:url(missing.png)}\n\
                 h{background:myurl(../img/logo.png)}",
            ),
        ]);
        let font = published(&fingerprints, "fonts/a.woff2");
        let font = font.trim_start_matches("fonts/");
        let asset = fingerprints.get("css/site.css").unwrap().unwrap();
        assert_eq!(
            asset.rewritten.as_deref().unwrap(),
            format!(
                "a{{background:url(../img/logo.2cf24dba.png)}}\n\
                 b{{background:URL( \"../img/logo.2cf24dba.png?v=1#x\" )}}\n\
                 @font-face{{src:url('../fonts/{font}') format('woff2')}}\n\
                 c{{background:url(data:image/png;base64,AAAA)}}\n\
                 d{{background:url(/assets/img/logo.png)}}\n\
                 e{{background:url(https://example.com/a.png)}}\n\
                 f{{filter:url(#shadow)}}\n\
                 g{{background:url(missing.png)}}\n\
                 h{{background:myurl(../img/logo.png)}}"
            )
        );
        assert_eq!(
            asset.path,
            fingerprinted_name(
                "css/site.css",
                &content_hash(asset.rewritten.as_deref().unwrap().as_bytes())
            )
        );
    }

    #[test]
    fn test_changed_reference_changes_stylesheet_name() {
        let (temp, fingerprints) = assets(&[
            ("logo.png", "one"),
            ("site.css", "a{background:url(logo.png)}"),
        ]);
        let before = published(&fingerprints, "site.css");
        fs::write(temp.path().join("logo.png"), "two").unwrap();
        let after = published(&Fingerprints::new(temp.path()), "site.css");
        assert_ne!(before, after);
    }

    #[test]
    fn test_stylesheet_cycles_are_left_unrewritten() {
        let (_temp, fingerprints) = assets(&[
            ("a.css", "@import url(b.css);"),
            ("b.css", "@import url(a.css);"),
        ]);
        assert_eq!(fingerprints.get("a.css").unwrap().unwrap().rewritten, None);
        assert_eq!(fingerprints.get("b.css").unwrap().unwrap().rewritten, None);
    }

    #[test]
    fn test_stylesheet_cycle_names_do_not_depend_on_order() {
        let files = [
            ("a.css", "@import url(b.css);"),
            ("b.css", "@import url(a.css); x{background:url(c.css)}"),
            ("c.css", "y{background:url(c.css)}"),
        ];
        let (_temp, forward) = assets(&files);
        let (_temp2, backward) = assets(&files);
        for path in ["a.css", "b.css", "c.css"] {
            forward.get(path).unwrap();
        }
        for path in ["c.css", "b.css", "a.css"] {
            backward.get(path).unwrap();
        }
        assert_eq!(forward.manifest(), backward.manifest());
        let b = forward.get("b.css").unwrap().unwrap();
        let c = forward.get("c.css").unwrap().unwrap().path;
        assert_eq!(
            b.rewritten.as_deref(),
            Some(format!("@import url(a.css); x{{background:url({c})}}").as_str())
        );
    }

    #[test]
    fn test_manifest_lists_resolved_assets() {
        let (_temp, fingerprints) = assets(&[
            ("img/logo.png", "hello"),
            ("site.css", "a{background:url(img/logo.png)}"),
        ]);
        let css = published(&fingerprints, "site.css");
        fingerprints.get("missing.js").unwrap();
        assert_eq!(
            fingerprints.manifest(),
            BTreeMap::from([
                (
                    "img/logo.png".to_string(),
                    "img/logo.2cf24dba.png".to_string()
                ),
                ("site.css".to_string(), css),
            ])
        );
    }
}
