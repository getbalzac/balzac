//! Incremental `balzac build`: a manifest of what the last successful build
//! wrote, keyed on content hashes of the inputs of each output file.
//!
//! - The *global key* hashes everything every page may depend on: the
//!   config, partials, layouts, tag templates, helpers, the Vite manifest, the
//!   user `robots.txt` and the Balzac version. When it changes (or there is no
//!   usable cache, or `--force` is given) the output directory is recreated and
//!   everything is rebuilt, exactly like a build without a cache.
//! - Otherwise each page is re-rendered only when its *page key* changed: the
//!   global key, its own source (or template), and the collection items. Every
//!   template sees all items through `collections`, so any item change
//!   re-renders every page, as in `balzac dev`.
//! - A page is also re-rendered when its output file no longer has the hash
//!   recorded when it was written (deleted, or edited by a hook), so before the
//!   `render_after` and `build_after` hooks run, every file Balzac writes is
//!   byte-identical to what a full build writes.
//! - Outputs recorded by the last build that are not produced anymore
//!   (deleted sources, changed slugs, removed assets) are removed. Files Balzac
//!   did not write (e.g. by hooks) are left alone.
//!
//! The manifest is removed when a build starts and written once it succeeds,
//! so a failed or interrupted build is followed by a full rebuild.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::Xxh3;

use crate::ROBOTS_FILE;
use crate::RenderedPages;
use crate::config::{Config, ResolvedConfig};
use crate::helpers::{ClockReadings, clock_unchanged};
use crate::sitemap::{PageEntry, PageType};

/// Directory of Balzac's local state in a project, ignored by git.
pub const STATE_DIRECTORY: &str = ".balzac";
/// Manifest of the last successful build, relative to the project root.
const MANIFEST_FILE: &str = ".balzac/cache/build.json";
/// Bumped whenever the manifest format or what goes into a key changes.
const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    format: u32,
    global: String,
    /// Pages and assets written to the output directory, by path relative to it.
    outputs: BTreeMap<PathBuf, OutputRecord>,
    /// Clock readings of the `collections` context, which every page sees.
    #[serde(default, skip_serializing_if = "ClockReadings::is_empty")]
    collections_clock: ClockReadings,
    feeds: Option<FeedsRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OutputRecord {
    /// Hash of the inputs the file was written from.
    input: String,
    /// Hash of the file as written.
    output: String,
    /// `{{date "now"}}` calls made rendering the file: it is rendered again
    /// once any of them gives another result (e.g. a new year for `%Y`).
    #[serde(default, skip_serializing_if = "ClockReadings::is_empty")]
    clock: ClockReadings,
}

/// Feeds are written together, from every collection item.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FeedsRecord {
    input: String,
    /// Feed files, by path relative to the output directory, and their hashes.
    files: BTreeMap<PathBuf, String>,
    #[serde(default, skip_serializing_if = "ClockReadings::is_empty")]
    clock: ClockReadings,
}

/// Why a build renders every page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullBuildReason {
    Forced,
    NoCache,
    GlobalChange,
}

pub struct BuildCache {
    manifest_path: PathBuf,
    output_directory: PathBuf,
    /// `None` for a full build.
    previous: Option<Manifest>,
    next: Manifest,
    full_build_reason: Option<FullBuildReason>,
    collections_key: String,
    /// Output files of the pages to render, with their page keys.
    pending: Vec<(PathBuf, String)>,
    reused: usize,
    /// Stale output files removed.
    removed: usize,
}

impl BuildCache {
    /// Loads the manifest of the last build and removes it, so that a build
    /// failing from here on leaves no cache behind. Call [`Self::start`]
    /// before using it. With `force`, the manifest is ignored.
    pub fn open(config: &ResolvedConfig, force: bool) -> io::Result<Self> {
        let manifest_path = config.root_directory.join(MANIFEST_FILE);
        let loaded = load_manifest(&manifest_path);
        invalidate(&config.root_directory)?;

        let (previous, full_build_reason) = match loaded {
            _ if force => (None, Some(FullBuildReason::Forced)),
            None => (None, Some(FullBuildReason::NoCache)),
            Some(manifest) => (Some(manifest), None),
        };
        Ok(Self {
            manifest_path,
            output_directory: config.output_directory.clone(),
            previous,
            next: Manifest {
                format: FORMAT_VERSION,
                ..Manifest::default()
            },
            full_build_reason,
            collections_key: String::new(),
            pending: Vec::new(),
            reused: 0,
            removed: 0,
        })
    }

    /// Sets the key from [`global_key`]: when it differs from the last
    /// build's, nothing is reused.
    pub fn start(&mut self, global: String) {
        if self
            .previous
            .as_ref()
            .is_some_and(|previous| previous.global != global)
        {
            self.previous = None;
            self.full_build_reason = Some(FullBuildReason::GlobalChange);
        }
        self.next.global = global;
    }

    /// Why every page is rendered, or `None` for an incremental build. A full
    /// build recreates the output directory first.
    #[must_use]
    pub fn full_build_reason(&self) -> Option<FullBuildReason> {
        self.full_build_reason
    }

    /// Decides which pages to render: those whose key changed since the last
    /// build or whose output file changed. Run it right before rendering
    /// (after the `render_before` hook), as it hashes the files rendered.
    pub fn plan_pages(
        &mut self,
        config: &ResolvedConfig,
        pages: &[PageEntry],
    ) -> io::Result<BTreeSet<PathBuf>> {
        self.collections_key = collections_key(pages);
        self.prune_before_render(config, pages)?;
        // `{{date "now"}}` in a tag template used by the items' markdown
        let collections_clock_unchanged = self
            .previous
            .as_ref()
            .is_none_or(|previous| clock_unchanged(&previous.collections_clock));
        let mut selected = BTreeSet::new();
        for page in pages {
            let key = self.page_key(config, page)?;
            let relative = self.relative(&page.output_path);
            let record = if collections_clock_unchanged {
                self.reusable(&relative, &key)?
            } else {
                None
            };
            if let Some(record) = record {
                self.next.outputs.insert(relative, record);
                self.reused += 1;
            } else {
                selected.insert(page.output_path.clone());
                self.pending.push((page.output_path.clone(), key));
            }
        }
        Ok(selected)
    }

    /// Records the pages planned by [`Self::plan_pages`] once they are written.
    pub fn finish_pages(&mut self, rendered: &RenderedPages) -> io::Result<()> {
        self.next.collections_clock = match &rendered.collections {
            Some(clock) => clock.clone(),
            // not built: no page was rendered, all of them were reused
            None => self
                .previous
                .as_ref()
                .map(|previous| previous.collections_clock.clone())
                .unwrap_or_default(),
        };
        for (output_path, key) in std::mem::take(&mut self.pending) {
            let output = hash_file(&output_path)?.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("rendered page {} is missing", output_path.display()),
                )
            })?;
            let relative = self.relative(&output_path);
            let clock = rendered
                .pages
                .get(&output_path)
                .cloned()
                .unwrap_or_default();
            self.next.outputs.insert(
                relative,
                OutputRecord {
                    input: key,
                    output,
                    clock,
                },
            );
        }
        Ok(())
    }

    /// Number of pages reused from the last build by [`Self::plan_pages`].
    #[must_use]
    pub fn reused_pages(&self) -> usize {
        self.reused
    }

    /// Copies the assets directory into `<output>/assets`, skipping files
    /// already there from the last build with the same content.
    pub fn sync_assets(&mut self, config: &ResolvedConfig) -> io::Result<()> {
        crate::copy_assets(config, &mut |source, target| {
            let input = hash_file(source)?.unwrap_or_default();
            let relative = self.relative(target);
            if let Some(record) = self.reusable(&relative, &input)? {
                self.next.outputs.insert(relative, record);
                return Ok(());
            }
            // Only a page rendered by this build can be here already (e.g. a
            // directory-style `pages/assets.hbs`), and replacing it would lose it.
            if self.next.outputs.contains_key(&relative) {
                return Err(crate::asset_collision(source, target));
            }
            log::debug!("Copying {} to {}", source.display(), target.display());
            fs::copy(source, target)?;
            self.next.outputs.insert(
                relative,
                OutputRecord {
                    output: input.clone(),
                    input,
                    clock: ClockReadings::new(),
                },
            );
            Ok(())
        })
    }

    /// Whether the feeds written by the last build are still up to date, in
    /// which case they are kept.
    pub fn feeds_fresh(&mut self) -> io::Result<bool> {
        let input = self.feeds_key();
        let Some(record) = self.previous.as_ref().and_then(|p| p.feeds.as_ref()) else {
            return Ok(false);
        };
        if record.input != input || !clock_unchanged(&record.clock) {
            return Ok(false);
        }
        for (relative, output) in &record.files {
            if hash_file(&self.output_directory.join(relative))?.as_ref() != Some(output) {
                return Ok(false);
            }
        }
        self.next.feeds = Some(record.clone());
        Ok(true)
    }

    /// Records the feed files written by this build, with the clock readings
    /// of writing them.
    pub fn record_feeds(&mut self, written: &[PathBuf], clock: ClockReadings) -> io::Result<()> {
        let mut files = BTreeMap::new();
        for path in written {
            if let Some(output) = hash_file(path)? {
                files.insert(self.relative(path), output);
            }
        }
        self.next.feeds = Some(FeedsRecord {
            input: self.feeds_key(),
            files,
            clock,
        });
        Ok(())
    }

    /// Removes the files the last build wrote that this one did not, and the
    /// folders they leave empty. Returns how many files were removed, here and
    /// before rendering.
    pub fn prune_stale_outputs(&mut self) -> io::Result<usize> {
        let Some(previous) = &self.previous else {
            return Ok(0);
        };
        let current = output_paths(&self.next);
        let stale: Vec<PathBuf> = output_paths(previous)
            .difference(&current)
            .map(|path| path.to_path_buf())
            .collect();
        for path in stale {
            self.remove_output(&path)?;
        }
        Ok(self.removed)
    }

    /// Removes the pages and assets of the last build that this one won't
    /// write, before anything is written: a stale file could stand where a
    /// folder is now needed (e.g. `assets/icons` that became a folder).
    fn prune_before_render(
        &mut self,
        config: &ResolvedConfig,
        pages: &[PageEntry],
    ) -> io::Result<()> {
        let Some(previous) = &self.previous else {
            return Ok(());
        };
        let pages: BTreeSet<PathBuf> = pages
            .iter()
            .map(|page| self.relative(&page.output_path))
            .collect();
        let stale: Vec<PathBuf> = previous
            .outputs
            .keys()
            .filter(|path| !pages.contains(*path))
            .filter(|path| {
                // assets whose source is still there are checked when synced
                path.strip_prefix("assets")
                    .map_or(true, |asset| !config.assets_directory.join(asset).is_file())
            })
            .cloned()
            .collect();
        for path in stale {
            self.remove_output(&path)?;
        }
        Ok(())
    }

    fn remove_output(&mut self, relative: &Path) -> io::Result<()> {
        let path = self.output_directory.join(relative);
        if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
            log::info!("Removing stale output {}", path.display());
            fs::remove_file(&path)?;
            self.removed += 1;
        }
        crate::remove_empty_parents(&path, &self.output_directory)
    }

    /// Writes the manifest, once the build succeeded.
    pub fn save(&self) -> io::Result<()> {
        let Some(directory) = self.manifest_path.parent() else {
            return Ok(());
        };
        fs::create_dir_all(directory)?;
        // keep the cache out of version control even without `balzac init`
        let state_directory = self.manifest_path.ancestors().nth(2);
        if let Some(ignore_file) = state_directory.map(|dir| dir.join(".gitignore"))
            && !ignore_file.exists()
        {
            fs::write(ignore_file, "*\n")?;
        }
        let json = serde_json::to_vec(&self.next).map_err(io::Error::other)?;
        fs::write(&self.manifest_path, json)
    }

    fn relative(&self, output_path: &Path) -> PathBuf {
        output_path
            .strip_prefix(&self.output_directory)
            .unwrap_or(output_path)
            .to_path_buf()
    }

    /// The record of the last build for `relative`, if it was written from
    /// `input` and the file still has the content written.
    fn reusable(&self, relative: &Path, input: &str) -> io::Result<Option<OutputRecord>> {
        let Some(record) = self
            .previous
            .as_ref()
            .and_then(|previous| previous.outputs.get(relative))
        else {
            return Ok(None);
        };
        if record.input != input || !clock_unchanged(&record.clock) {
            return Ok(None);
        }
        let current = hash_file(&self.output_directory.join(relative))?;
        Ok((current.as_deref() == Some(record.output.as_str())).then(|| record.clone()))
    }

    fn page_key(&self, config: &ResolvedConfig, page: &PageEntry) -> io::Result<String> {
        let mut hasher = KeyHasher::new("page");
        hasher
            .str(&self.next.global)
            .str(&self.collections_key)
            .path(&page.source_path)
            .path(&page.output_path)
            .str(&page.url_path);
        match &page.page_type {
            // static pages and listings are rendered from their source file
            PageType::Static => hasher.str("static").file(&page.source_path)?,
            PageType::Listing(listing) => hasher
                .str("listing")
                .str(&format!("{listing:?}"))
                .file(&page.source_path)?,
            // items are rendered from the content read at discovery, which
            // the collections key covers, with their collection's template
            PageType::Collection { name } => hasher
                .str("collection")
                .str(name)
                .file(&config.collection_template_path(name))?,
        };
        Ok(hasher.finish())
    }

    fn feeds_key(&self) -> String {
        KeyHasher::new("feeds")
            .str(&self.next.global)
            .str(&self.collections_key)
            .finish()
    }
}

/// Removes the build cache, e.g. before `balzac dev` writes to the output
/// directory, so the next `balzac build` starts from scratch.
pub fn invalidate(root: &Path) -> io::Result<()> {
    match fs::remove_file(root.join(MANIFEST_FILE)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn load_manifest(path: &Path) -> Option<Manifest> {
    let bytes = fs::read(path).ok()?;
    match serde_json::from_slice::<Manifest>(&bytes) {
        Ok(manifest) if manifest.format == FORMAT_VERSION => Some(manifest),
        Ok(_) => None,
        Err(error) => {
            log::warn!(
                "Ignoring unreadable build cache {}: {error}",
                path.display()
            );
            None
        }
    }
}

fn output_paths(manifest: &Manifest) -> BTreeSet<&Path> {
    manifest
        .outputs
        .keys()
        .chain(manifest.feeds.iter().flat_map(|feeds| feeds.files.keys()))
        .map(PathBuf::as_path)
        .collect()
}

/// Hash of everything any page may depend on besides its own source and the
/// collection items. Compute it before the renderer reads these files, so a
/// hook editing them later is caught by the next build.
pub fn global_key(config: &Config, resolved: &ResolvedConfig) -> io::Result<String> {
    let mut hasher = KeyHasher::new("global");
    hasher
        .u32(FORMAT_VERSION)
        .str(env!("CARGO_PKG_VERSION"))
        .str(if cfg!(feature = "script-helpers") {
            "script-helpers"
        } else {
            ""
        })
        // through `Value`, whose maps are sorted: the config's `HashMap`s
        // serialize in a different order in every process
        .str(
            &serde_json::to_value(config)
                .map_err(io::Error::other)?
                .to_string(),
        )
        .path(&resolved.root_directory);
    for directory in [
        &resolved.partials_directory,
        &resolved.layouts_directory,
        &resolved.tags_directory,
        &resolved.helpers_directory,
    ] {
        hasher.path(directory).tree(directory)?;
    }
    hasher.file(&resolved.pages_directory.join(ROBOTS_FILE))?;
    if let Some(vite) = resolved.bundler.as_ref().and_then(|b| b.vite.as_ref())
        && vite.enabled
    {
        hasher.file(&resolved.root_directory.join(&vite.manifest_path))?;
    }

    Ok(hasher.finish())
}

/// Hash of the collection items every template sees through `collections`.
/// Items are hashed by source path: discovery lists the collection folders
/// in `read_dir` order, which is not guaranteed to be stable.
fn collections_key(pages: &[PageEntry]) -> String {
    let mut items: Vec<(&PageEntry, &String)> = pages
        .iter()
        .filter_map(|page| match &page.page_type {
            PageType::Collection { name } => Some((page, name)),
            _ => None,
        })
        .collect();
    items.sort_by(|(a, _), (b, _)| a.source_path.cmp(&b.source_path));
    let mut hasher = KeyHasher::new("collections");
    for (page, name) in items {
        let frontmatter = page
            .frontmatter
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        hasher
            .str(name)
            .path(&page.source_path)
            .path(&page.output_path)
            .str(&page.url_path)
            .str(&frontmatter)
            .str(page.content.as_deref().unwrap_or_default());
    }
    hasher.finish()
}

/// Hash of a file's content, or `None` when it does not exist.
fn hash_file(path: &Path) -> io::Result<Option<String>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(KeyHasher::new("file").bytes(&bytes).finish())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Hashes length-prefixed fields, so that adjacent fields can't run together.
struct KeyHasher(Xxh3);

impl KeyHasher {
    fn new(domain: &str) -> Self {
        let mut hasher = Self(Xxh3::new());
        hasher.str(domain);
        hasher
    }

    fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.0.update(&(bytes.len() as u64).to_le_bytes());
        self.0.update(bytes);
        self
    }

    fn str(&mut self, value: &str) -> &mut Self {
        self.bytes(value.as_bytes())
    }

    fn u32(&mut self, value: u32) -> &mut Self {
        self.bytes(&value.to_le_bytes())
    }

    fn path(&mut self, path: &Path) -> &mut Self {
        self.bytes(path.as_os_str().as_encoded_bytes())
    }

    /// Hashes a file's content, telling a missing file from an empty one.
    fn file(&mut self, path: &Path) -> io::Result<&mut Self> {
        match fs::read(path) {
            Ok(bytes) => Ok(self.str("file").bytes(&bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(self.str("missing")),
            Err(error) => Err(error),
        }
    }

    /// Hashes every file under `directory` with its relative path, in a stable
    /// order. Symlinked folders are not followed, only their target recorded.
    fn tree(&mut self, directory: &Path) -> io::Result<&mut Self> {
        let mut entries = match fs::read_dir(directory) {
            Ok(entries) => entries.collect::<io::Result<Vec<_>>>()?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(self.str("missing"));
            }
            Err(error) if error.kind() == io::ErrorKind::NotADirectory => {
                return self.file(directory);
            }
            Err(error) => return Err(error),
        };
        entries.sort_by_key(fs::DirEntry::file_name);
        self.u32(u32::try_from(entries.len()).unwrap_or(u32::MAX));
        for entry in entries {
            let path = entry.path();
            self.bytes(entry.file_name().as_encoded_bytes());
            let file_type = entry.file_type()?;
            if file_type.is_symlink() && path.is_dir() {
                self.str("link").path(&fs::read_link(&path)?);
            } else if file_type.is_dir() {
                self.str("dir").tree(&path)?;
            } else {
                self.file(&path)?;
            }
        }
        Ok(self)
    }

    fn finish(&self) -> String {
        format!("{:032x}", self.0.digest128())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sitemap::SitemapMeta;

    fn item(collection: &str, slug: &str) -> PageEntry {
        PageEntry {
            url_path: format!("/{collection}/{slug}"),
            source_path: PathBuf::from(format!("content/{collection}/{slug}.md")),
            output_path: PathBuf::from(format!("dist/{collection}/{slug}.html")),
            page_type: PageType::Collection {
                name: collection.to_string(),
            },
            sitemap_meta: SitemapMeta::default(),
            frontmatter: Some(serde_json::json!({ "title": slug })),
            content: Some(format!("# {slug}")),
        }
    }

    #[test]
    fn test_collections_key_ignores_discovery_order() {
        let pages = vec![item("posts", "a"), item("posts", "b"), item("notes", "c")];
        let mut reordered = pages.clone();
        reordered.rotate_left(2);

        assert_eq!(collections_key(&pages), collections_key(&reordered));
        assert_ne!(
            collections_key(&pages),
            collections_key(&[item("posts", "a"), item("notes", "c")])
        );
    }
}
