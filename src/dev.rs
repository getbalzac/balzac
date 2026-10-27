use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::thread;
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::config::{Config, Hooks, ResolvedConfig, UrlStyle};
use crate::context::SiteContext;
use crate::error::Result;
use crate::hooks::{HookExecutor, HookPhase};
use crate::listing::render_listing_pages;
use crate::markdown;
use crate::renderer::{HandlebarsRenderer, Renderer};
use crate::sitemap::{PageType, SitePages};
use crate::url::percent_decode;
use crate::{
    NOT_FOUND_PAGE, ROBOTS_FILE, add_assets, collection_item_for_static_page,
    collection_location_from_source, conflicting_static_page, discover_site_pages,
    is_page_template, is_supported_page_file, make_dist_folder, remove_empty_parents,
    remove_output_for_source, render_collection_items, render_pages, render_static_page,
    static_page_entry_from_source, write_feeds, write_robots, write_sitemap,
};

const SSE_ENDPOINT: &str = "/__balzac/events";
const WATCH_INTERVAL: Duration = Duration::from_millis(300);
const WATCH_DEBOUNCE: Duration = Duration::from_millis(200);

/// Shows the build error over the page, and reloads the page when told to by
/// the dev server, or when the error it shows has been fixed meanwhile.
const CLIENT_SCRIPT: &str = r#"<script>(()=>{const decodeSegment=(segment)=>{try{return decodeURIComponent(segment);}catch{return segment;}};const decode=(value)=>value.split("/").map(decodeSegment).join("/");const normalize=(value)=>{const path=(value||"/").replace(/\.html$/,"").replace(/\/index$/,"/");return path.length>1&&path.endsWith("/")?path.slice(0,-1):path||"/";};let currentError=null;const showError=(message)=>{let overlay=document.getElementById("__balzac-error");if(!overlay){overlay=document.createElement("div");overlay.id="__balzac-error";overlay.setAttribute("style","position:fixed;inset:0;z-index:2147483647;overflow:auto;padding:2rem;background:rgba(24,24,27,.96);color:#fecaca;font:14px/1.5 ui-monospace,SFMono-Regular,Menlo,monospace");const title=document.createElement("strong");title.textContent="Balzac build failed";title.setAttribute("style","display:block;margin-bottom:1rem;color:#fff;font-size:16px");const details=document.createElement("pre");details.setAttribute("style","margin:0;white-space:pre-wrap");overlay.append(title,details);(document.body||document.documentElement).append(overlay);}overlay.querySelector("pre").textContent=message;currentError=message;};const buildError=__BUILD_ERROR__;if(buildError)showError(buildError);const source=new EventSource("__SSE_ENDPOINT__");source.onmessage=(event)=>{const payload=JSON.parse(event.data);if(payload.kind==="error"||(payload.kind==="status"&&payload.message)){showError(payload.message);return;}if(payload.kind==="status"){if(currentError)window.location.reload();return;}if(payload.kind==="full"){window.location.reload();return;}if(payload.kind==="targeted"&&normalize(decode(window.location.pathname))===normalize(payload.url_path)){window.location.reload();}};})();</script>"#;

pub fn run(root: &Path, host: &str, port: u16) -> Result<()> {
    let mut session = DevSession::new(root)?;

    let shared_config = Arc::new(RwLock::new(session.resolved_config.clone()));
    let build_error = Arc::new(RwLock::new(session.build_error.clone()));
    let events = EventBroker::default();
    start_http_server(
        host,
        port,
        shared_config.clone(),
        build_error.clone(),
        events.clone(),
    )?;

    log::info!("Balzac dev server running at http://{host}:{port}");

    let mut snapshot = capture_snapshot(&session.resolved_config)?;

    loop {
        thread::sleep(WATCH_INTERVAL);
        let latest_snapshot = match capture_snapshot(&session.resolved_config) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                log::warn!("Error refreshing watched files, retrying: {error}");
                continue;
            }
        };
        let changes = diff_snapshots(&snapshot, &latest_snapshot);
        if changes.is_empty() {
            continue;
        }

        thread::sleep(WATCH_DEBOUNCE);

        let debounced_snapshot = match capture_snapshot(&session.resolved_config) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                log::warn!("Error refreshing watched files after debounce, retrying: {error}");
                continue;
            }
        };
        let debounced_changes = diff_snapshots(&snapshot, &debounced_snapshot);
        if debounced_changes.is_empty() {
            snapshot = debounced_snapshot;
            continue;
        }

        // A failed build keeps the server running: the error is shown in the
        // browser until a later build succeeds. Every outcome re-snapshots
        // after the rebuild, so files written by hooks (which run
        // synchronously) during it never retrigger a rebuild.
        let reload_messages = match session.process_changes(&debounced_changes) {
            Ok(messages) => messages,
            Err(error) => {
                log::error!("Build failed: {error}");
                vec![ReloadMessage::error(&error.to_string())]
            }
        };
        {
            let mut config = shared_config
                .write()
                .expect("dev config lock should not be poisoned");
            *config = session.resolved_config.clone();
        }
        build_error
            .write()
            .expect("dev build error lock should not be poisoned")
            .clone_from(&session.build_error);
        for message in reload_messages {
            events.broadcast(&message);
        }
        match capture_snapshot(&session.resolved_config) {
            Ok(updated_snapshot) => snapshot = updated_snapshot,
            Err(error) => {
                log::warn!("Error refreshing watched files after reload: {error}");
            }
        }
    }
}

struct DevSession {
    root: PathBuf,
    resolved_config: ResolvedConfig,
    /// Hooks to run, only set when `[hooks] dev = true`.
    hooks: Option<Hooks>,
    /// Set while re-applying files written by hooks, so hooks don't rerun.
    hooks_paused: bool,
    /// Baselines for the hook writes of a full build: assets are snapshotted
    /// once copied, so writes the build copied are not applied again; other
    /// sources right after `render_init_before`, before anything is read, so
    /// no write is missed, at the cost of re-applying the (rare) source writes
    /// of `render_init_after`, `build_before` and `render_before`.
    full_build_baselines: Option<(Snapshot, Snapshot)>,
    /// Output files of the generated pages from the last render: collection
    /// items and listing pages. Their outputs depend on frontmatter (`slug`,
    /// taxonomy terms, item counts), so they cannot be recomputed from source
    /// paths once files change or are deleted.
    generated_outputs: BTreeSet<PathBuf>,
    /// Feed files written by the last render, so stale ones can be removed
    feed_paths: BTreeSet<PathBuf>,
    /// Error of the last build, until a build succeeds again.
    build_error: Option<String>,
}

impl DevSession {
    /// Fails only when the config can't be loaded: a failed initial build is
    /// kept in `build_error` so the server can start and show it.
    fn new(root: &Path) -> Result<Self> {
        let (parsed_config, resolved_config) = load_config(root)?;
        let mut session = Self {
            root: root.to_path_buf(),
            resolved_config,
            hooks: dev_hooks(parsed_config),
            hooks_paused: false,
            full_build_baselines: None,
            generated_outputs: BTreeSet::new(),
            feed_paths: BTreeSet::new(),
            build_error: None,
        };
        let result = session.initial_build();
        if let Err(error) = session.record_build(result) {
            log::error!("Build failed: {error}");
        }
        Ok(session)
    }

    fn process_changes(&mut self, changes: &[SourceChange]) -> Result<Vec<ReloadMessage>> {
        let mut plan = classify_changes(&self.resolved_config, changes);
        if plan == ChangePlan::None {
            return Ok(Vec::new());
        }
        // A failed build may have left any page stale or missing, so the next
        // change rebuilds everything.
        if self.build_error.is_some() {
            plan = ChangePlan::FullReload;
        }
        let result = self.apply_with_hook_writes(plan);
        self.record_build(result)
    }

    fn initial_build(&mut self) -> Result<()> {
        let before = self.hook_snapshot()?;
        self.build_current_config()?;
        self.apply_hook_writes(before.as_ref())?;
        Ok(())
    }

    fn apply_with_hook_writes(&mut self, plan: ChangePlan) -> Result<Vec<ReloadMessage>> {
        self.full_build_baselines = None;
        let before = self.hook_snapshot()?;
        let messages = self.apply_change_plan(plan)?;
        let hook_messages = self.apply_hook_writes(before.as_ref())?;
        Ok(merge_reload_messages(messages, hook_messages))
    }

    /// Snapshot of the watched files before a (re)build, when hooks may run.
    fn hook_snapshot(&self) -> std::io::Result<Option<Snapshot>> {
        if self.hooks.is_none() || self.hooks_paused {
            return Ok(None);
        }
        capture_snapshot(&self.resolved_config).map(Some)
    }

    /// Applies, once and with hooks paused, the watched files that hooks wrote
    /// during a (re)build, e.g. `assets/` written by `render_before` in a
    /// targeted rebuild, which would otherwise not reach the output directory.
    fn apply_hook_writes(&mut self, before: Option<&Snapshot>) -> Result<Vec<ReloadMessage>> {
        // a full build records baselines even when it is the one enabling hooks
        let baselines = self.full_build_baselines.take();
        if baselines.is_none() && before.is_none() {
            return Ok(Vec::new());
        }
        let after = capture_snapshot(&self.resolved_config)?;
        let hook_changes = match (baselines, before) {
            (Some((sources, assets)), _) => {
                let assets_directory = &self.resolved_config.assets_directory;
                let mut changes: Vec<SourceChange> = diff_snapshots(&sources, &after)
                    .into_iter()
                    .filter(|change| !is_under(&change.path, assets_directory))
                    .collect();
                changes.extend(
                    diff_snapshots(&assets, &after)
                        .into_iter()
                        .filter(|change| is_under(&change.path, assets_directory)),
                );
                changes
            }
            (None, Some(before)) => diff_snapshots(before, &after),
            (None, None) => Vec::new(),
        };
        if hook_changes.is_empty() {
            return Ok(Vec::new());
        }
        let config_path = self.root.join("balzac.toml");
        let config_changed = hook_changes.iter().any(|change| change.path == config_path);
        self.hooks_paused = true;
        let result = match classify_changes(&self.resolved_config, &hook_changes) {
            // recreating the output directory would discard what hooks such as
            // build_after wrote into it, so refresh it in place instead, unless
            // the config changed and any source directory may have moved
            ChangePlan::FullReload if !config_changed => self
                .refresh_output(&hook_changes)
                .map(|()| vec![ReloadMessage::full()]),
            plan => self.apply_change_plan(plan),
        };
        self.hooks_paused = false;
        result
    }

    /// Rebuilds the whole site over the existing output directory, with an
    /// unchanged config. Only the changed assets are copied, as `add_assets`
    /// refuses to overwrite files already in the output directory.
    fn refresh_output(&mut self, changes: &[SourceChange]) -> Result<()> {
        // deletions first, children before their directory, so the render
        // below is never undone by them
        for change in changes.iter().rev() {
            if change.change_type == ChangeType::Deleted {
                remove_output_for_source(&self.resolved_config, &change.path)?;
                self.sync_asset(change)?;
            }
        }
        self.render_site()?;
        write_robots(&self.resolved_config)?;
        markdown::write_highlight_css(&self.resolved_config)?;
        for change in changes {
            if change.change_type != ChangeType::Deleted {
                self.sync_asset(change)?;
            }
        }
        Ok(())
    }

    /// Mirrors one changed path of the assets directory into the output.
    /// It never replaces a rendered page: `render_site` runs first and, like
    /// a full build, rejects pages whose output collides with an asset, and
    /// directories are only removed once empty, as pages may live in them
    /// (e.g. a directory-style `pages/assets.hbs`).
    fn sync_asset(&self, change: &SourceChange) -> std::io::Result<()> {
        let config = &self.resolved_config;
        let Ok(relative_path) = change.path.strip_prefix(&config.assets_directory) else {
            return Ok(());
        };
        // the output directory may live inside the assets directory
        if is_under(&change.path, &config.output_directory) {
            return Ok(());
        }
        let target = config.output_directory.join("assets").join(relative_path);
        let source_is_dir = change.path.is_dir();
        if change.change_type != ChangeType::Deleted && !source_is_dir && !change.path.is_file() {
            return Ok(()); // like add_assets, skip what isn't a regular file
        }
        // clear what was deleted, or replaced by an entry of another type
        if let Ok(existing) = fs::symlink_metadata(&target)
            && (change.change_type == ChangeType::Deleted || existing.is_dir() != source_is_dir)
        {
            if !existing.is_dir() {
                fs::remove_file(&target)?;
            } else if fs::read_dir(&target)?.next().is_none() {
                fs::remove_dir(&target)?;
            } else if change.change_type != ChangeType::Deleted {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "asset {} would replace the non-empty directory {}",
                        change.path.display(),
                        target.display()
                    ),
                ));
            }
        }
        match change.change_type {
            ChangeType::Deleted => Ok(()),
            _ if source_is_dir => fs::create_dir_all(target),
            _ => {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&change.path, target).map(|_| ())
            }
        }
    }

    /// Keeps the error of a failed build, or clears it after a successful one.
    fn record_build<T>(&mut self, result: Result<T>) -> Result<T> {
        self.build_error = result.as_ref().err().map(ToString::to_string);
        result
    }

    fn apply_change_plan(&mut self, plan: ChangePlan) -> Result<Vec<ReloadMessage>> {
        match plan {
            ChangePlan::None => Ok(Vec::new()),
            ChangePlan::FullReload => {
                self.full_rebuild()?;
                Ok(vec![ReloadMessage::full()])
            }
            ChangePlan::Targeted(targets) => {
                // Every page can list collection items, so a collection change
                // re-renders the whole site instead of a single page.
                if targets
                    .iter()
                    .any(|target| target.kind == TargetKind::CollectionItem)
                {
                    // Generated outputs are pruned by `render_site`; deleted
                    // static pages in the same batch are not.
                    for target in &targets {
                        if target.kind == TargetKind::StaticPage
                            && target.change_type == ChangeType::Deleted
                            && !self.generated_outputs.contains(&target.output_path)
                        {
                            remove_output_for_source(&self.resolved_config, &target.source_path)?;
                        }
                    }
                    self.render_site()?;
                    return Ok(vec![ReloadMessage::full()]);
                }

                self.run_hook(HookPhase::RenderBefore)?;
                let mut messages = Vec::new();
                for target in targets {
                    if let Some(message) = self.handle_static_page_change(&target)? {
                        messages.push(message);
                    }
                }
                self.run_hook(HookPhase::RenderAfter)?;
                if !messages.is_empty() {
                    self.refresh_sitemap()?;
                }
                Ok(messages)
            }
        }
    }

    fn handle_static_page_change(&self, target: &TargetedChange) -> Result<Option<ReloadMessage>> {
        match target.change_type {
            ChangeType::Deleted => {
                // A page rejected for colliding with a generated page (collection
                // item or listing) never replaced its file, so that file is not
                // the page's to remove.
                if !self.generated_outputs.contains(&target.output_path) {
                    remove_output_for_source(&self.resolved_config, &target.source_path)?;
                }
                Ok(Some(ReloadMessage::targeted(&target.url_path)))
            }
            ChangeType::Created | ChangeType::Modified => {
                let Some(page) =
                    static_page_entry_from_source(&self.resolved_config, &target.source_path)
                else {
                    return Ok(None);
                };
                // Discover the whole site first: it fails on output collisions (e.g. a
                // directory-style `pages/posts.hbs` and the `/posts/` listing, or a
                // copied asset) before the page can overwrite anything.
                let site_pages = discover_site_pages(&self.resolved_config)?;
                let renderer = self.create_renderer()?;
                let site = SiteContext::new(&self.resolved_config, site_pages.all(), &renderer)?;
                render_static_page(&self.resolved_config, &page, &renderer, &site)?;
                Ok(Some(ReloadMessage::targeted(&target.url_path)))
            }
        }
    }

    fn full_rebuild(&mut self) -> Result<()> {
        let (parsed_config, resolved_config) = load_config(&self.root)?;
        self.resolved_config = resolved_config;
        self.hooks = dev_hooks(parsed_config);
        self.build_current_config()
    }

    /// Builds the whole site, running every hook phase in the same order as
    /// `balzac build`.
    fn build_current_config(&mut self) -> Result<()> {
        self.run_hook(HookPhase::RenderInitBefore)?;
        let sources_baseline = self.hook_snapshot()?;
        let renderer = self.create_renderer()?;
        self.run_hook(HookPhase::RenderInitAfter)?;
        self.run_hook(HookPhase::BuildBefore)?;
        make_dist_folder(&self.resolved_config)?;
        self.generated_outputs.clear();
        self.render_site_with(&renderer)?;
        write_robots(&self.resolved_config)?;
        markdown::write_highlight_css(&self.resolved_config)?;
        add_assets(&self.resolved_config)?;
        if let Some(sources) = sources_baseline {
            let assets = capture_snapshot(&self.resolved_config)?;
            self.full_build_baselines = Some((sources, assets));
        }
        self.run_hook(HookPhase::BuildAfter)
    }

    fn render_site(&mut self) -> Result<()> {
        let renderer = self.create_renderer()?;
        self.render_site_with(&renderer)
    }

    fn render_site_with(&mut self, renderer: &HandlebarsRenderer<'_>) -> Result<()> {
        let site_pages = discover_site_pages(&self.resolved_config)?;

        self.run_hook(HookPhase::RenderBefore)?;
        let site = SiteContext::new(&self.resolved_config, site_pages.all(), renderer)?;
        render_pages(&self.resolved_config, &site, renderer)?;
        render_collection_items(&self.resolved_config, &site, renderer)?;
        render_listing_pages(&site, renderer)?;
        // frees the per-thread page contexts before feeds are written
        drop(site);
        self.run_hook(HookPhase::RenderAfter)?;
        self.prune_stale_outputs(&site_pages)?;
        write_sitemap(&self.resolved_config, &site_pages)?;
        self.refresh_feeds(&site_pages, renderer)
    }

    /// Writes feeds and removes those from the previous render that are no
    /// longer produced, e.g. after a collection's last dated entry is removed
    fn refresh_feeds(
        &mut self,
        site_pages: &SitePages,
        renderer: &HandlebarsRenderer,
    ) -> Result<()> {
        let written: BTreeSet<PathBuf> = write_feeds(&self.resolved_config, site_pages, renderer)?
            .into_iter()
            .collect();
        for stale in self.feed_paths.difference(&written) {
            if fs::exists(stale)? {
                log::info!("Removing stale feed {}", stale.display());
                fs::remove_file(stale)?;
            }
        }
        self.feed_paths = written;
        Ok(())
    }

    /// Removes generated pages rendered previously but no longer produced, e.g.
    /// a deleted collection item, an item whose `slug` changed, a term whose
    /// last item was untagged, or a page beyond the new last page.
    fn prune_stale_outputs(&mut self, site_pages: &SitePages) -> std::io::Result<()> {
        let generated_outputs: BTreeSet<PathBuf> = site_pages
            .all()
            .iter()
            .filter(|page| {
                matches!(
                    page.page_type,
                    PageType::Collection { .. } | PageType::Listing(_)
                )
            })
            .map(|page| page.output_path.clone())
            .collect();

        // With directory-style URLs another page can take over a stale generated
        // file (e.g. `content/tags/rust.md` replacing the `/tags/rust/` term page).
        let page_outputs: BTreeSet<&Path> = site_pages
            .all()
            .iter()
            .map(|page| page.output_path.as_path())
            .collect();
        for stale in self.generated_outputs.difference(&generated_outputs) {
            if page_outputs.contains(stale.as_path()) {
                continue;
            }
            if fs::exists(stale)? {
                fs::remove_file(stale)?;
            }
            remove_empty_parents(stale, &self.resolved_config.output_directory)?;
        }

        self.generated_outputs = generated_outputs;
        Ok(())
    }

    fn refresh_sitemap(&self) -> Result<()> {
        write_sitemap(
            &self.resolved_config,
            &discover_site_pages(&self.resolved_config)?,
        )?;
        Ok(())
    }

    fn create_renderer(&self) -> Result<HandlebarsRenderer<'static>> {
        let mut renderer = HandlebarsRenderer::new(&self.resolved_config);
        renderer.init(&self.resolved_config)?;
        Ok(renderer)
    }

    /// Runs a hook; a failure aborts the current (re)build like any other
    /// build error. Hooks that fail after output was written (e.g. after
    /// `make_dist_folder` in a full rebuild, or `render_after`) leave it
    /// changed or partially rebuilt.
    fn run_hook(&self, phase: HookPhase) -> Result<()> {
        if self.hooks_paused {
            return Ok(());
        }
        HookExecutor::new(self.hooks.as_ref(), &self.root).run(phase)?;
        Ok(())
    }
}

/// Combines the reload messages of a rebuild and of its hook writes; a full
/// reload covers every targeted one.
fn merge_reload_messages(
    mut messages: Vec<ReloadMessage>,
    hook_messages: Vec<ReloadMessage>,
) -> Vec<ReloadMessage> {
    if messages.contains(&ReloadMessage::full()) || hook_messages.contains(&ReloadMessage::full()) {
        return vec![ReloadMessage::full()];
    }
    for message in hook_messages {
        if !messages.contains(&message) {
            messages.push(message);
        }
    }
    messages
}

/// Keeps the configured hooks only when they are enabled for dev mode.
fn dev_hooks(parsed_config: Config) -> Option<Hooks> {
    let hooks = parsed_config.hooks?;
    if hooks.dev {
        Some(hooks)
    } else {
        log::info!("Hooks are configured but ignored in dev mode (set `dev = true` in [hooks])");
        None
    }
}

fn load_config(root: &Path) -> Result<(Config, ResolvedConfig)> {
    let parsed_config = Config::load(root)?;
    let mut resolved_config = parsed_config.resolve(root);
    resolved_config.dev_mode = true;
    Ok((parsed_config, resolved_config))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChangeType {
    Created,
    Modified,
    Deleted,
}

#[derive(Debug, Clone)]
struct SourceChange {
    path: PathBuf,
    change_type: ChangeType,
    is_dir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TargetKind {
    StaticPage,
    CollectionItem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetedChange {
    kind: TargetKind,
    source_path: PathBuf,
    output_path: PathBuf,
    url_path: String,
    change_type: ChangeType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChangePlan {
    None,
    FullReload,
    Targeted(Vec<TargetedChange>),
}

fn classify_changes(config: &ResolvedConfig, changes: &[SourceChange]) -> ChangePlan {
    let mut targets = BTreeMap::new();

    for change in changes {
        let classification = classify_change(config, change);
        match classification {
            ChangePlan::None => {}
            ChangePlan::FullReload => return ChangePlan::FullReload,
            ChangePlan::Targeted(found) => {
                for target in found {
                    // keyed by source: different files can share a URL
                    // (`pages/docs.hbs` and `pages/docs/index.hbs`)
                    targets.insert(target.source_path.clone(), target);
                }
            }
        }
    }

    if targets.is_empty() {
        ChangePlan::None
    } else {
        ChangePlan::Targeted(targets.into_values().collect())
    }
}

fn classify_change(config: &ResolvedConfig, change: &SourceChange) -> ChangePlan {
    let config_path = config.root_directory.join("balzac.toml");
    if change.path == config_path {
        return ChangePlan::FullReload;
    }
    if change.is_dir {
        return ChangePlan::FullReload;
    }
    if change.path == config.pages_directory.join(ROBOTS_FILE)
        || is_under(&change.path, &config.partials_directory)
        || is_under(&change.path, &config.layouts_directory)
        || is_under(&change.path, &config.assets_directory)
        || is_under(&change.path, &config.tags_directory)
        || is_under(&change.path, &config.helpers_directory)
    {
        return ChangePlan::FullReload;
    }

    if is_under(&change.path, &config.pages_directory) {
        if is_page_template(config, &change.path) {
            return ChangePlan::FullReload;
        }
        if !is_supported_page_file(&change.path) {
            return ChangePlan::None;
        }
        // Another page writes the same file and still needs it after this one is
        // deleted. Other changes re-run discovery, which reports the conflict.
        if change.change_type == ChangeType::Deleted
            && conflicting_static_page(config, &change.path).is_some()
        {
            return ChangePlan::FullReload;
        }
        // A static page shadowing a collection item changes which of the two
        // owns the output file, so rebuild everything to settle it.
        if collection_item_for_static_page(config, &change.path).is_some() {
            return ChangePlan::FullReload;
        }
        let Some(page) = static_page_entry_from_source(config, &change.path) else {
            return ChangePlan::None;
        };
        if page.output_path == config.output_directory.join(NOT_FOUND_PAGE) {
            // the 404 page is shown for arbitrary missing URLs, so a targeted reload can't reach it
            return ChangePlan::FullReload;
        }
        return ChangePlan::Targeted(vec![TargetedChange {
            kind: TargetKind::StaticPage,
            source_path: change.path.clone(),
            output_path: page.output_path,
            url_path: page.url_path,
            change_type: change.change_type.clone(),
        }]);
    }

    if let Ok(relative_path) = change.path.strip_prefix(&config.content_directory) {
        if relative_path.components().count() >= 2
            && change.path.extension().and_then(|ext| ext.to_str()) == Some("md")
        {
            if let Some((_, location)) = collection_location_from_source(config, &change.path) {
                return ChangePlan::Targeted(vec![TargetedChange {
                    kind: TargetKind::CollectionItem,
                    source_path: change.path.clone(),
                    output_path: location.output_path,
                    url_path: location.url_path,
                    change_type: change.change_type.clone(),
                }]);
            }
            return ChangePlan::None;
        }

        return ChangePlan::FullReload;
    }

    ChangePlan::None
}

fn is_under(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root).is_ok()
}

#[derive(Clone, Default)]
struct EventBroker {
    clients: Arc<Mutex<Vec<mpsc::Sender<ReloadMessage>>>>,
}

impl EventBroker {
    fn subscribe(&self) -> mpsc::Receiver<ReloadMessage> {
        let (sender, receiver) = mpsc::channel();
        self.clients
            .lock()
            .expect("event clients lock should not be poisoned")
            .push(sender);
        receiver
    }

    fn broadcast(&self, event: &ReloadMessage) {
        let mut clients = self
            .clients
            .lock()
            .expect("event clients lock should not be poisoned");
        clients.retain(|client| client.send(event.clone()).is_ok());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ReloadKind {
    Full,
    Targeted,
    Error,
    /// Sent when a client connects: the current build error, if any.
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ReloadMessage {
    kind: ReloadKind,
    url_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl ReloadMessage {
    fn full() -> Self {
        Self {
            kind: ReloadKind::Full,
            url_path: None,
            message: None,
        }
    }

    fn targeted(url_path: &str) -> Self {
        Self {
            kind: ReloadKind::Targeted,
            url_path: Some(url_path.to_string()),
            message: None,
        }
    }

    /// The current build state, sent to each client when it connects, so a
    /// page that missed a build (it loaded or reconnected meanwhile) catches up.
    fn status(build_error: Option<&str>) -> Self {
        Self {
            kind: ReloadKind::Status,
            url_path: None,
            message: build_error.map(ToString::to_string),
        }
    }

    /// Shows a build error in the browser.
    fn error(message: &str) -> Self {
        Self {
            kind: ReloadKind::Error,
            url_path: None,
            message: Some(message.to_string()),
        }
    }
}

fn start_http_server(
    host: &str,
    port: u16,
    config: Arc<RwLock<ResolvedConfig>>,
    build_error: Arc<RwLock<Option<String>>>,
    events: EventBroker,
) -> std::io::Result<()> {
    let listener = TcpListener::bind((host, port))?;
    listener.set_nonblocking(true)?;

    thread::spawn(move || {
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    let config = config.clone();
                    let build_error = build_error.clone();
                    let events = events.clone();
                    thread::spawn(move || {
                        if let Err(error) =
                            handle_connection(stream, &config, &build_error, &events)
                        {
                            log::debug!("Error handling dev request: {error}");
                        }
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => {
                    log::error!("Dev server accept error: {error}");
                    break;
                }
            }
        }
    });

    Ok(())
}

fn handle_connection(
    stream: TcpStream,
    config: &RwLock<ResolvedConfig>,
    build_error: &RwLock<Option<String>>,
    events: &EventBroker,
) -> std::io::Result<()> {
    let (method, path) = read_request(&stream)?;
    if method != "GET" && method != "HEAD" {
        return write_response(
            stream,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"Method Not Allowed",
            false,
        );
    }

    if path == SSE_ENDPOINT {
        // Subscribe before reading the status so no build falls in between: a
        // page that loaded before a build finished then still catches up.
        let receiver = events.subscribe();
        let status = ReloadMessage::status(
            build_error
                .read()
                .expect("dev build error lock should not be poisoned")
                .as_deref(),
        );
        return serve_sse(stream, &status, &receiver);
    }

    let config_guard = config
        .read()
        .expect("dev config lock should not be poisoned");
    if let Some(location) = directory_redirect(&config_guard, &path) {
        return write_redirect(stream, &location);
    }
    let build_error = build_error
        .read()
        .expect("dev build error lock should not be poisoned")
        .clone();
    let response = build_file_response(&config_guard, &path, build_error.as_deref())?;
    write_response(
        stream,
        response.status,
        response.content_type,
        &response.body,
        should_include_response_body(&method),
    )
}

struct FileResponse {
    status: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
}

/// Serves `request_path` from the output directory; HTML pages get the dev
/// client, which shows `build_error` when the last build failed.
fn build_file_response(
    config: &ResolvedConfig,
    request_path: &str,
    build_error: Option<&str>,
) -> std::io::Result<FileResponse> {
    if let Some(file_path) = resolve_output_path(&config.output_directory, request_path) {
        return Ok(FileResponse {
            status: "200 OK",
            content_type: content_type_for_path(&file_path),
            body: read_output_file(&file_path, config, build_error)?,
        });
    }

    // mirror static hosts: serve the site's own 404 page when there is one
    let not_found_page = config.output_directory.join(NOT_FOUND_PAGE);
    if not_found_page.is_file() {
        return Ok(FileResponse {
            status: "404 Not Found",
            content_type: content_type_for_path(&not_found_page),
            body: read_output_file(&not_found_page, config, build_error)?,
        });
    }

    // a failed build may have left no page at all: show its error anyway
    if build_error.is_some() {
        return Ok(FileResponse {
            status: "404 Not Found",
            content_type: "text/html; charset=utf-8",
            body: inject_dev_scripts(
                "<!DOCTYPE html><html><head><title>Build failed</title></head><body></body></html>",
                config,
                build_error,
            )
            .into_bytes(),
        });
    }

    Ok(FileResponse {
        status: "404 Not Found",
        content_type: "text/plain; charset=utf-8",
        body: b"Not Found".to_vec(),
    })
}

fn read_output_file(
    file_path: &Path,
    config: &ResolvedConfig,
    build_error: Option<&str>,
) -> std::io::Result<Vec<u8>> {
    let body = fs::read(file_path)?;
    if is_html_path(file_path) {
        let html = String::from_utf8_lossy(&body);
        return Ok(inject_dev_scripts(&html, config, build_error).into_bytes());
    }
    Ok(body)
}

fn read_request(stream: &TcpStream) -> std::io::Result<(String, String)> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    if request_line.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "request line missing",
        ));
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();

    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
    }

    Ok((method, path))
}

/// Streams `status`, then every message of `receiver`.
fn serve_sse(
    mut stream: TcpStream,
    status: &ReloadMessage,
    receiver: &mpsc::Receiver<ReloadMessage>,
) -> std::io::Result<()> {
    stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n",
    )?;
    write_event(&mut stream, status)?;

    loop {
        match receiver.recv_timeout(Duration::from_secs(10)) {
            Ok(message) => write_event(&mut stream, &message)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                stream.write_all(b": keep-alive\n\n")?;
                stream.flush()?;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

fn write_event(stream: &mut TcpStream, message: &ReloadMessage) -> std::io::Result<()> {
    let payload = serde_json::to_string(message).map_err(std::io::Error::other)?;
    stream.write_all(format!("data: {payload}\n\n").as_bytes())?;
    stream.flush()
}

fn write_response(
    mut stream: TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
    include_body: bool,
) -> std::io::Result<()> {
    let headers = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status,
        content_type,
        body.len()
    );
    stream.write_all(headers.as_bytes())?;
    if include_body {
        stream.write_all(body)?;
    }
    stream.flush()
}

fn write_redirect(mut stream: TcpStream, location: &str) -> std::io::Result<()> {
    stream.write_all(redirect_headers(location).as_bytes())?;
    stream.flush()
}

// Temporary and uncached: a permanent redirect would outlive a `url_style` switch
// or a page turning back into a file, since browsers cache 301s indefinitely.
fn redirect_headers(location: &str) -> String {
    format!(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nCache-Control: no-store\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
}

/// With directory-style URLs, mirror static hosts by redirecting `/about` to `/about/`
/// so relative links resolve the same way in dev as in production.
fn directory_redirect(config: &ResolvedConfig, request_path: &str) -> Option<String> {
    if config.url_style != UrlStyle::Directory {
        return None;
    }

    let (path, query) = match request_path.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (request_path, None),
    };
    if path.ends_with('/') {
        return None;
    }

    let relative = sanitize_relative_path(path.trim_start_matches('/'))?;
    if relative.as_os_str().is_empty()
        || relative
            .file_name()
            .is_some_and(|name| name == "index.html")
    {
        return None;
    }
    // Only redirect when the path is served as a directory's `index.html` (not
    // as a file or `<path>.html`), including percent-encoded paths.
    let served = resolve_output_path(&config.output_directory, path)?;
    if served.file_name().is_none_or(|name| name != "index.html") {
        return None;
    }

    // Rebuild the location from the sanitized path: echoing the request could turn
    // `//about` into a protocol-relative redirect to another host.
    let segments: Vec<_> = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect();
    let location = format!("/{}/", segments.join("/"));
    Some(match query {
        Some(query) => format!("{location}?{query}"),
        None => location,
    })
}

fn should_include_response_body(method: &str) -> bool {
    method != "HEAD"
}

/// Resolves the request path percent-decoded first (browsers encode e.g.
/// non-ASCII terms), then as-is, since collection URLs come from file names
/// that may themselves contain `%`.
fn resolve_output_path(output_root: &Path, request_path: &str) -> Option<PathBuf> {
    let path = request_path.split('?').next().unwrap_or("/");
    percent_decode(path)
        .and_then(|decoded| resolve_decoded_path(output_root, &decoded))
        .or_else(|| resolve_decoded_path(output_root, path))
}

fn resolve_decoded_path(output_root: &Path, path: &str) -> Option<PathBuf> {
    let trimmed = path.trim_start_matches('/');

    if trimmed.is_empty() {
        let index = output_root.join("index.html");
        return index.exists().then_some(index);
    }

    let relative = sanitize_relative_path(trimmed)?;
    let candidate = output_root.join(&relative);
    // `/posts/` is a directory URL (e.g. a listing), even if `posts.html` exists.
    if path.ends_with('/') {
        let index_candidate = candidate.join("index.html");
        if index_candidate.is_file() {
            return Some(index_candidate);
        }
    }
    if candidate.is_file() {
        return Some(candidate);
    }
    // Append rather than replace the extension so `/about.en` finds `about.en.html`.
    let mut html_candidate = candidate.clone().into_os_string();
    html_candidate.push(".html");
    let html_candidate = PathBuf::from(html_candidate);
    if html_candidate.is_file() {
        return Some(html_candidate);
    }
    let index_candidate = candidate.join("index.html");
    index_candidate.is_file().then_some(index_candidate)
}

fn sanitize_relative_path(path: &str) -> Option<PathBuf> {
    let mut sanitized = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(segment) => sanitized.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => return None,
        }
    }
    Some(sanitized)
}

fn is_html_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("html")
}

fn content_type_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("xml") => "application/xml; charset=utf-8",
        Some("txt") => "text/plain; charset=utf-8",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn inject_dev_scripts(html: &str, config: &ResolvedConfig, build_error: Option<&str>) -> String {
    let mut scripts = String::new();

    if let Some(bundler) = &config.bundler
        && let Some(vite) = &bundler.vite
        && vite.enabled
    {
        let _ = write!(
            scripts,
            r#"<script type="module" src="{}/@vite/client"></script>"#,
            vite.dev_origin.trim_end_matches('/')
        );
    }

    // `<` is escaped so the message can't close the script element
    let build_error = serde_json::to_string(&build_error)
        .expect("a string always serializes to JSON")
        .replace('<', "\\u003c");
    scripts.push_str(
        &CLIENT_SCRIPT
            .replace("__SSE_ENDPOINT__", SSE_ENDPOINT)
            .replace("__BUILD_ERROR__", &build_error),
    );

    if let Some(index) = html.to_ascii_lowercase().rfind("</body>") {
        let mut injected = String::with_capacity(html.len() + scripts.len());
        injected.push_str(&html[..index]);
        injected.push_str(&scripts);
        injected.push_str(&html[index..]);
        injected
    } else {
        format!("{html}{scripts}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SnapshotEntry {
    is_dir: bool,
    modified: Option<SystemTime>,
    len: u64,
}

type Snapshot = BTreeMap<PathBuf, SnapshotEntry>;

fn capture_snapshot(config: &ResolvedConfig) -> std::io::Result<Snapshot> {
    let mut snapshot = BTreeMap::new();
    let roots = [
        config.root_directory.join("balzac.toml"),
        config.pages_directory.clone(),
        config.partials_directory.clone(),
        config.layouts_directory.clone(),
        config.assets_directory.clone(),
        config.content_directory.clone(),
        config.tags_directory.clone(),
        config.helpers_directory.clone(),
    ];

    for root in roots {
        capture_path(&root, &mut snapshot)?;
    }

    Ok(snapshot)
}

fn capture_path(path: &Path, snapshot: &mut Snapshot) -> std::io::Result<()> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };

    snapshot.insert(
        path.to_path_buf(),
        SnapshotEntry {
            is_dir: metadata.is_dir(),
            modified: metadata.modified().ok(),
            len: metadata.len(),
        },
    );

    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            capture_path(&entry.path(), snapshot)?;
        }
    }

    Ok(())
}

fn diff_snapshots(previous: &Snapshot, next: &Snapshot) -> Vec<SourceChange> {
    let mut changes = Vec::new();
    let paths = previous
        .keys()
        .chain(next.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    for path in paths {
        match (previous.get(&path), next.get(&path)) {
            (None, Some(entry)) => changes.push(SourceChange {
                path,
                change_type: ChangeType::Created,
                is_dir: entry.is_dir,
            }),
            (Some(entry), None) => changes.push(SourceChange {
                path,
                change_type: ChangeType::Deleted,
                is_dir: entry.is_dir,
            }),
            (Some(before), Some(after)) => {
                if before.is_dir && after.is_dir {
                    continue;
                }
                if before.modified != after.modified || before.len != after.len {
                    changes.push(SourceChange {
                        path,
                        change_type: ChangeType::Modified,
                        is_dir: after.is_dir,
                    });
                }
            }
            (None, None) => {}
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Bundler, ViteBundler};
    use crate::error::Error;
    use tempfile::TempDir;

    fn setup_dev_project() -> (TempDir, ResolvedConfig) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let root = temp_dir.path().to_path_buf();
        let pages = root.join("pages");
        let layouts = root.join("layouts");
        let partials = root.join("partials");
        let assets = root.join("assets");
        let content = root.join("content");
        let tags = root.join("tags");
        let output = root.join("dist");

        fs::create_dir_all(&pages).unwrap();
        fs::create_dir_all(&layouts).unwrap();
        fs::create_dir_all(&partials).unwrap();
        fs::create_dir_all(&assets).unwrap();
        fs::create_dir_all(&content).unwrap();
        fs::create_dir_all(&tags).unwrap();
        fs::write(root.join("balzac.toml"), "").unwrap();

        let config = Config {
            output_directory: output.to_string_lossy().to_string(),
            pages_directory: pages.to_string_lossy().to_string(),
            layouts_directory: layouts.to_string_lossy().to_string(),
            partials_directory: partials.to_string_lossy().to_string(),
            assets_directory: assets.to_string_lossy().to_string(),
            content_directory: content.to_string_lossy().to_string(),
            tags_directory: tags.to_string_lossy().to_string(),
            ..Config::default()
        };

        let mut resolved = config.resolve(&root);
        resolved.dev_mode = true;
        (temp_dir, resolved)
    }

    #[test]
    fn test_classify_static_page_change() {
        let (_temp_dir, config) = setup_dev_project();
        let change = SourceChange {
            path: config.pages_directory.join("index.hbs"),
            change_type: ChangeType::Modified,
            is_dir: false,
        };

        let result = classify_changes(&config, &[change]);
        assert_eq!(
            result,
            ChangePlan::Targeted(vec![TargetedChange {
                kind: TargetKind::StaticPage,
                source_path: config.pages_directory.join("index.hbs"),
                output_path: config.output_directory.join("index.html"),
                url_path: "/".to_string(),
                change_type: ChangeType::Modified,
            }])
        );
    }

    #[test]
    fn test_classify_collection_change() {
        let (_temp_dir, config) = setup_dev_project();
        let change = SourceChange {
            path: config.content_directory.join("posts").join("hello.md"),
            change_type: ChangeType::Modified,
            is_dir: false,
        };

        let result = classify_changes(&config, &[change]);
        assert_eq!(
            result,
            ChangePlan::Targeted(vec![TargetedChange {
                kind: TargetKind::CollectionItem,
                source_path: config.content_directory.join("posts").join("hello.md"),
                output_path: config.output_directory.join("posts").join("hello.html"),
                url_path: "/posts/hello".to_string(),
                change_type: ChangeType::Modified,
            }])
        );
    }

    #[test]
    fn test_classify_shared_change_as_full_reload() {
        let (_temp_dir, config) = setup_dev_project();
        let change = SourceChange {
            path: config.partials_directory.join("header.hbs"),
            change_type: ChangeType::Modified,
            is_dir: false,
        };

        assert_eq!(classify_changes(&config, &[change]), ChangePlan::FullReload);
    }

    #[test]
    fn test_remove_output_for_deleted_source() {
        let (temp_dir, config) = setup_dev_project();
        fs::create_dir_all(&config.output_directory).unwrap();
        let output_path = config.output_directory.join("index.html");
        fs::write(&output_path, "<h1>Home</h1>").unwrap();

        let removed = remove_output_for_source(&config, &config.pages_directory.join("index.hbs"))
            .unwrap()
            .unwrap();
        assert_eq!(removed, output_path);
        assert!(!temp_dir.path().join("dist/index.html").exists());
    }

    #[test]
    fn test_classify_nested_static_page_change() {
        let (_temp_dir, config) = setup_dev_project();
        let source_path = config
            .pages_directory
            .join("docs")
            .join("getting-started.hbs");
        let change = SourceChange {
            path: source_path.clone(),
            change_type: ChangeType::Created,
            is_dir: false,
        };

        assert_eq!(
            classify_changes(&config, &[change]),
            ChangePlan::Targeted(vec![TargetedChange {
                kind: TargetKind::StaticPage,
                source_path,
                output_path: config
                    .output_directory
                    .join("docs")
                    .join("getting-started.html"),
                url_path: "/docs/getting-started".to_string(),
                change_type: ChangeType::Created,
            }])
        );
    }

    #[test]
    fn test_classify_nested_index_change() {
        let (_temp_dir, config) = setup_dev_project();
        let change = SourceChange {
            path: config.pages_directory.join("docs").join("index.hbs"),
            change_type: ChangeType::Modified,
            is_dir: false,
        };

        let ChangePlan::Targeted(targets) = classify_changes(&config, &[change]) else {
            panic!("expected targeted change");
        };
        assert_eq!(targets[0].url_path, "/docs");
        assert_eq!(
            targets[0].output_path,
            config.output_directory.join("docs.html")
        );
    }

    #[test]
    fn test_classify_collection_template_change_as_full_reload() {
        let (_temp_dir, config) = setup_dev_project();
        fs::create_dir_all(config.content_directory.join("posts")).unwrap();
        let change = SourceChange {
            path: config.pages_directory.join("posts").join("details.hbs"),
            change_type: ChangeType::Modified,
            is_dir: false,
        };

        assert_eq!(classify_changes(&config, &[change]), ChangePlan::FullReload);
    }

    #[test]
    fn test_classify_details_page_without_collection_as_static_page() {
        let (_temp_dir, config) = setup_dev_project();
        let change = SourceChange {
            path: config.pages_directory.join("product").join("details.hbs"),
            change_type: ChangeType::Modified,
            is_dir: false,
        };

        let ChangePlan::Targeted(targets) = classify_changes(&config, &[change]) else {
            panic!("expected targeted change");
        };
        assert_eq!(targets[0].kind, TargetKind::StaticPage);
        assert_eq!(targets[0].url_path, "/product/details");
    }

    #[test]
    fn test_remove_output_for_deleted_nested_source_prunes_empty_dirs() {
        let (_temp_dir, config) = setup_dev_project();
        let nested_output_dir = config.output_directory.join("docs").join("guides");
        fs::create_dir_all(&nested_output_dir).unwrap();
        let output_path = nested_output_dir.join("intro.html");
        fs::write(&output_path, "<h1>Intro</h1>").unwrap();
        fs::write(config.output_directory.join("index.html"), "<h1>Home</h1>").unwrap();

        let removed = remove_output_for_source(
            &config,
            &config.pages_directory.join("docs/guides/intro.hbs"),
        )
        .unwrap()
        .unwrap();

        assert_eq!(removed, output_path);
        assert!(!config.output_directory.join("docs").exists());
        assert!(config.output_directory.join("index.html").exists());
    }

    #[test]
    fn test_remove_output_for_deleted_nested_source_keeps_non_empty_dirs() {
        let (_temp_dir, config) = setup_dev_project();
        let docs_output_dir = config.output_directory.join("docs");
        fs::create_dir_all(&docs_output_dir).unwrap();
        fs::write(docs_output_dir.join("intro.html"), "<h1>Intro</h1>").unwrap();
        fs::write(docs_output_dir.join("index.html"), "<h1>Docs</h1>").unwrap();

        remove_output_for_source(&config, &config.pages_directory.join("docs/intro.hbs"))
            .unwrap()
            .unwrap();

        assert!(!docs_output_dir.join("intro.html").exists());
        assert!(docs_output_dir.join("index.html").exists());
    }

    fn setup_shadowed_collection(config: &ResolvedConfig) {
        fs::create_dir_all(config.pages_directory.join("posts")).unwrap();
        fs::create_dir_all(config.content_directory.join("posts")).unwrap();
        fs::write(
            config.pages_directory.join("posts/details.hbs"),
            "{{{content}}}",
        )
        .unwrap();
        fs::write(
            config.pages_directory.join("posts/hello.hbs"),
            "<h1>Hello</h1>",
        )
        .unwrap();
        fs::write(config.content_directory.join("posts/hello.md"), "Body").unwrap();
    }

    #[test]
    fn test_classify_static_page_shadowing_collection_item_as_full_reload() {
        let (_temp_dir, config) = setup_dev_project();
        setup_shadowed_collection(&config);
        let change = SourceChange {
            path: config.pages_directory.join("posts/hello.hbs"),
            change_type: ChangeType::Modified,
            is_dir: false,
        };

        assert_eq!(classify_changes(&config, &[change]), ChangePlan::FullReload);
    }

    #[test]
    fn test_classify_static_page_shadowing_item_by_slug_or_nested_path_as_full_reload() {
        let (_temp_dir, config) = setup_dev_project();
        setup_shadowed_collection(&config);
        fs::remove_file(config.content_directory.join("posts/hello.md")).unwrap();
        fs::write(
            config.content_directory.join("posts/other.md"),
            "---\nslug: hello\n---\n",
        )
        .unwrap();
        fs::create_dir_all(config.pages_directory.join("posts/2024")).unwrap();
        fs::create_dir_all(config.content_directory.join("posts/2024")).unwrap();
        fs::write(config.pages_directory.join("posts/2024/a.hbs"), "A").unwrap();
        fs::write(config.content_directory.join("posts/2024/a.md"), "Body").unwrap();

        for page in ["posts/hello.hbs", "posts/2024/a.hbs"] {
            let change = SourceChange {
                path: config.pages_directory.join(page),
                change_type: ChangeType::Modified,
                is_dir: false,
            };
            assert_eq!(
                classify_changes(&config, &[change]),
                ChangePlan::FullReload,
                "{page}"
            );
        }
    }

    #[test]
    fn test_shadowed_collection_item_changes_leave_static_output_alone() {
        let (_temp_dir, config) = setup_dev_project();
        setup_shadowed_collection(&config);
        let item = config.content_directory.join("posts/hello.md");
        let static_output = config.output_directory.join("posts/hello.html");
        fs::create_dir_all(static_output.parent().unwrap()).unwrap();
        fs::write(&static_output, "<h1>Hello</h1>").unwrap();

        let change = SourceChange {
            path: item.clone(),
            change_type: ChangeType::Modified,
            is_dir: false,
        };
        assert_eq!(classify_changes(&config, &[change]), ChangePlan::None);

        assert_eq!(remove_output_for_source(&config, &item).unwrap(), None);
        assert!(static_output.exists());
    }

    #[test]
    fn test_inject_dev_scripts_normalizes_html_urls() {
        let (_temp_dir, config) = setup_dev_project();
        let injected = inject_dev_scripts("<html><body></body></html>", &config, None);
        assert!(injected.contains(r#".replace(/\.html$/,"").replace(/\/index$/,"/")"#));
    }

    #[test]
    fn test_inject_dev_scripts_decodes_percent_encoded_urls() {
        let (_temp_dir, config) = setup_dev_project();
        let injected = inject_dev_scripts("<html><body></body></html>", &config, None);
        // the payload's `url_path` is already decoded; only the browser path is
        assert!(
            injected.contains(
                "normalize(decode(window.location.pathname))===normalize(payload.url_path)"
            )
        );
        // per segment, so reserved characters like `%23` (`#`) are decoded too
        assert!(injected.contains("decodeURIComponent(segment)"));
        assert!(injected.contains("catch{return segment;}"));
    }

    #[test]
    fn test_conflicting_static_page_keeps_dotted_folder_names() {
        let (_temp_dir, config) = setup_dev_project();
        let folder = config.pages_directory.join("v1.2");
        fs::create_dir_all(&folder).unwrap();
        fs::write(config.pages_directory.join("v1.2.hbs"), "page").unwrap();
        fs::write(config.pages_directory.join("v1.hbs"), "other").unwrap();

        assert_eq!(
            conflicting_static_page(&config, &folder.join("index.hbs")),
            Some(config.pages_directory.join("v1.2.hbs"))
        );
        assert_eq!(
            conflicting_static_page(&config, &config.pages_directory.join("v1.2.hbs")),
            None
        );
        fs::write(folder.join("index.html"), "index").unwrap();
        assert_eq!(
            conflicting_static_page(&config, &config.pages_directory.join("v1.2.hbs")),
            Some(folder.join("index.html"))
        );
    }

    #[test]
    fn test_classify_page_sharing_output_with_sibling_as_full_reload() {
        let (_temp_dir, config) = setup_dev_project();
        let docs = config.pages_directory.join("docs");
        fs::create_dir_all(&docs).unwrap();
        fs::write(docs.join("intro.html"), "<h1>Intro</h1>").unwrap();
        fs::write(config.pages_directory.join("docs.hbs"), "<h1>Docs</h1>").unwrap();
        // `guides/index/index.hbs` is `/guides/index`, not `/guides`
        let guides = config.pages_directory.join("guides");
        fs::create_dir_all(guides.join("index")).unwrap();
        fs::write(guides.join("index/index.hbs"), "<h1>Index</h1>").unwrap();
        // with extensionless urls, `index/index.hbs` and `index.hbs` both write `index.html`
        fs::write(config.pages_directory.join("index.hbs"), "<h1>Home</h1>").unwrap();

        for (path, change_type, expected_full) in [
            (docs.join("intro.hbs"), ChangeType::Deleted, true),
            (docs.join("index.hbs"), ChangeType::Deleted, true),
            (docs.join("intro.hbs"), ChangeType::Created, false),
            (guides.join("index.hbs"), ChangeType::Deleted, false),
            (guides.join("index/index.hbs"), ChangeType::Deleted, false),
            (
                config.pages_directory.join("index/index.hbs"),
                ChangeType::Deleted,
                true,
            ),
        ] {
            let change = SourceChange {
                path: path.clone(),
                change_type,
                is_dir: false,
            };
            let plan = classify_changes(&config, &[change]);
            assert_eq!(
                plan == ChangePlan::FullReload,
                expected_full,
                "{}",
                path.display()
            );
        }
    }

    #[test]
    fn test_inject_dev_scripts_includes_balzac_client() {
        let (_temp_dir, config) = setup_dev_project();
        let injected = inject_dev_scripts("<html><body><h1>Hi</h1></body></html>", &config, None);
        assert!(injected.contains(SSE_ENDPOINT));
        assert!(injected.contains("EventSource"));
    }

    #[test]
    fn test_inject_dev_scripts_handles_mixed_case_body_tag() {
        let (_temp_dir, config) = setup_dev_project();
        let injected = inject_dev_scripts("<html><Body><h1>Hi</h1></BoDy></html>", &config, None);
        assert!(injected.contains("</BoDy>"));
        assert!(injected.find("EventSource").unwrap() < injected.find("</BoDy>").unwrap());
    }

    #[test]
    fn test_inject_dev_scripts_includes_vite_client_when_enabled() {
        let (_temp_dir, mut config) = setup_dev_project();
        config.bundler = Some(Bundler {
            vite: Some(ViteBundler {
                enabled: true,
                manifest_path: "dist/.vite/manifest.json".to_string(),
                dev_origin: "http://127.0.0.1:5173".to_string(),
            }),
        });

        let injected = inject_dev_scripts("<html><body></body></html>", &config, None);
        assert!(injected.contains("http://127.0.0.1:5173/@vite/client"));
    }

    #[test]
    fn test_resolve_output_path_supports_extensionless_urls() {
        let (_temp_dir, config) = setup_dev_project();
        fs::create_dir_all(&config.output_directory).unwrap();
        fs::write(config.output_directory.join("about.html"), "<h1>About</h1>").unwrap();

        let path = resolve_output_path(&config.output_directory, "/about").unwrap();
        assert_eq!(path, config.output_directory.join("about.html"));
    }

    #[test]
    fn test_missing_path_serves_custom_404_page() {
        let (_temp_dir, config) = setup_dev_project();
        fs::create_dir_all(&config.output_directory).unwrap();
        fs::write(
            config.output_directory.join(NOT_FOUND_PAGE),
            "<html><body><h1>Lost</h1></body></html>",
        )
        .unwrap();

        let response = build_file_response(&config, "/does-not-exist", None).unwrap();
        assert_eq!(response.status, "404 Not Found");
        assert_eq!(response.content_type, "text/html; charset=utf-8");
        let body = String::from_utf8(response.body).unwrap();
        assert!(body.contains("<h1>Lost</h1>"));
        assert!(body.contains(SSE_ENDPOINT));
    }

    #[test]
    fn test_missing_path_without_404_page_serves_plain_text() {
        let (_temp_dir, config) = setup_dev_project();
        fs::create_dir_all(&config.output_directory).unwrap();

        let response = build_file_response(&config, "/does-not-exist", None).unwrap();
        assert_eq!(response.status, "404 Not Found");
        assert_eq!(response.content_type, "text/plain; charset=utf-8");
        assert_eq!(response.body, b"Not Found");
    }

    #[test]
    fn test_existing_path_is_served_with_ok_status() {
        let (_temp_dir, config) = setup_dev_project();
        fs::create_dir_all(&config.output_directory).unwrap();
        fs::write(
            config.output_directory.join(NOT_FOUND_PAGE),
            "<h1>Lost</h1>",
        )
        .unwrap();
        fs::write(config.output_directory.join(ROBOTS_FILE), "User-agent: *\n").unwrap();

        let response = build_file_response(&config, "/robots.txt", None).unwrap();
        assert_eq!(response.status, "200 OK");
        assert_eq!(response.content_type, "text/plain; charset=utf-8");
        assert_eq!(response.body, b"User-agent: *\n");
    }

    #[test]
    fn test_classify_404_page_change_as_full_reload() {
        let (_temp_dir, config) = setup_dev_project();
        let change = SourceChange {
            path: config.pages_directory.join("404.hbs"),
            change_type: ChangeType::Modified,
            is_dir: false,
        };

        assert_eq!(classify_changes(&config, &[change]), ChangePlan::FullReload);
    }

    #[test]
    fn test_classify_user_robots_change_as_full_reload() {
        let (_temp_dir, config) = setup_dev_project();
        let change = SourceChange {
            path: config.pages_directory.join(ROBOTS_FILE),
            change_type: ChangeType::Created,
            is_dir: false,
        };

        assert_eq!(classify_changes(&config, &[change]), ChangePlan::FullReload);
    }

    #[test]
    fn test_resolve_output_path_prefers_directory_index_for_trailing_slash() {
        let (_temp_dir, config) = setup_dev_project();
        fs::create_dir_all(config.output_directory.join("posts")).unwrap();
        fs::write(config.output_directory.join("posts.html"), "page").unwrap();
        fs::write(config.output_directory.join("posts/index.html"), "list").unwrap();

        assert_eq!(
            resolve_output_path(&config.output_directory, "/posts/").unwrap(),
            config.output_directory.join("posts/index.html")
        );
        assert_eq!(
            resolve_output_path(&config.output_directory, "/posts").unwrap(),
            config.output_directory.join("posts.html")
        );
    }

    #[test]
    fn test_resolve_output_path_decodes_percent_escapes() {
        let (_temp_dir, config) = setup_dev_project();
        let term_dir = config.output_directory.join("tags/économie");
        fs::create_dir_all(&term_dir).unwrap();
        fs::write(term_dir.join("index.html"), "term").unwrap();

        assert_eq!(
            resolve_output_path(&config.output_directory, "/tags/%C3%A9conomie/").unwrap(),
            term_dir.join("index.html")
        );
        assert_eq!(
            resolve_output_path(&config.output_directory, "/tags/%2E%2E/%2E%2E/x"),
            None
        );
        assert_eq!(resolve_output_path(&config.output_directory, "/%zz"), None);
    }

    #[test]
    fn test_resolve_output_path_falls_back_to_raw_path_with_percent_signs() {
        let (_temp_dir, config) = setup_dev_project();
        let posts = config.output_directory.join("posts");
        fs::create_dir_all(&posts).unwrap();
        fs::write(posts.join("100%.html"), "malformed escape").unwrap();
        fs::write(posts.join("part%20one.html"), "valid escape").unwrap();

        assert_eq!(
            resolve_output_path(&config.output_directory, "/posts/100%").unwrap(),
            posts.join("100%.html")
        );
        assert_eq!(
            resolve_output_path(&config.output_directory, "/posts/part%20one").unwrap(),
            posts.join("part%20one.html")
        );
    }

    #[test]
    fn test_resolve_output_path_supports_dotted_extensionless_urls() {
        let (_temp_dir, config) = setup_dev_project();
        fs::create_dir_all(&config.output_directory).unwrap();
        fs::write(config.output_directory.join("about.en.html"), "").unwrap();

        assert_eq!(
            resolve_output_path(&config.output_directory, "/about.en"),
            Some(config.output_directory.join("about.en.html"))
        );
        assert_eq!(
            resolve_output_path(&config.output_directory, "/about"),
            None
        );
    }

    #[test]
    fn test_should_include_response_body_for_head_requests() {
        assert!(!should_include_response_body("HEAD"));
        assert!(should_include_response_body("GET"));
    }

    #[test]
    fn test_process_changes_updates_only_changed_page() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::create_dir_all(root.join("pages")).unwrap();
        fs::create_dir_all(root.join("content/posts")).unwrap();
        fs::write(
            root.join("balzac.toml"),
            r#"
output_directory = "./dist"
pages_directory = "./pages"
content_directory = "./content"
"#,
        )
        .unwrap();
        fs::write(root.join("pages/index.hbs"), "<h1>Home</h1>").unwrap();
        fs::write(root.join("pages/about.hbs"), "<h1>About v1</h1>").unwrap();

        let mut session = DevSession::new(root).unwrap();
        session.full_rebuild().unwrap();

        fs::write(root.join("pages/about.hbs"), "<h1>About v2</h1>").unwrap();
        let messages = session
            .process_changes(&[SourceChange {
                path: root.join("pages/about.hbs"),
                change_type: ChangeType::Modified,
                is_dir: false,
            }])
            .unwrap();

        assert_eq!(messages, vec![ReloadMessage::targeted("/about")]);
        assert_eq!(
            fs::read_to_string(root.join("dist/index.html")).unwrap(),
            "<h1>Home</h1>"
        );
        assert_eq!(
            fs::read_to_string(root.join("dist/about.html")).unwrap(),
            "<h1>About v2</h1>"
        );
    }

    #[test]
    fn test_process_changes_renders_new_nested_page() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::create_dir_all(root.join("pages")).unwrap();
        fs::write(
            root.join("balzac.toml"),
            r#"
output_directory = "./dist"
pages_directory = "./pages"
"#,
        )
        .unwrap();
        fs::write(root.join("pages/index.hbs"), "<h1>Home</h1>").unwrap();

        let mut session = DevSession::new(root).unwrap();

        fs::create_dir_all(root.join("pages/docs/guides")).unwrap();
        fs::write(root.join("pages/docs/guides/setup.hbs"), "<h1>Setup</h1>").unwrap();
        let messages = session
            .process_changes(&[SourceChange {
                path: root.join("pages/docs/guides/setup.hbs"),
                change_type: ChangeType::Created,
                is_dir: false,
            }])
            .unwrap();

        assert_eq!(
            messages,
            vec![ReloadMessage::targeted("/docs/guides/setup")]
        );
        assert_eq!(
            fs::read_to_string(root.join("dist/docs/guides/setup.html")).unwrap(),
            "<h1>Setup</h1>"
        );

        fs::remove_file(root.join("pages/docs/guides/setup.hbs")).unwrap();
        let messages = session
            .process_changes(&[SourceChange {
                path: root.join("pages/docs/guides/setup.hbs"),
                change_type: ChangeType::Deleted,
                is_dir: false,
            }])
            .unwrap();

        assert_eq!(
            messages,
            vec![ReloadMessage::targeted("/docs/guides/setup")]
        );
        assert!(!root.join("dist/docs").exists());
        assert!(root.join("dist/index.html").exists());
    }

    #[test]
    fn test_process_changes_updates_collection_item() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::create_dir_all(root.join("pages/posts")).unwrap();
        fs::create_dir_all(root.join("content/posts")).unwrap();
        fs::write(
            root.join("balzac.toml"),
            r#"
output_directory = "./dist"
pages_directory = "./pages"
content_directory = "./content"
"#,
        )
        .unwrap();
        fs::write(
            root.join("pages/posts/details.hbs"),
            "<h1>{{fm.title}}</h1>{{{content}}}",
        )
        .unwrap();
        fs::write(
            root.join("content/posts/post-1.md"),
            "---\ntitle: First\n---\n\nOne",
        )
        .unwrap();

        let mut session = DevSession::new(root).unwrap();
        session.full_rebuild().unwrap();

        fs::write(
            root.join("content/posts/post-1.md"),
            "---\ntitle: First Updated\n---\n\nTwo",
        )
        .unwrap();

        let messages = session
            .process_changes(&[SourceChange {
                path: root.join("content/posts/post-1.md"),
                change_type: ChangeType::Modified,
                is_dir: false,
            }])
            .unwrap();

        assert_eq!(messages, vec![ReloadMessage::full()]);
        let rendered = fs::read_to_string(root.join("dist/posts/post-1.html")).unwrap();
        assert!(rendered.contains("First Updated"));
        assert!(rendered.contains("<p>Two</p>"));
    }

    #[test]
    fn test_process_changes_rerenders_pages_listing_collections() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::create_dir_all(root.join("pages/posts")).unwrap();
        fs::create_dir_all(root.join("content/posts")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(
            root.join("balzac.toml"),
            r#"
output_directory = "./dist"
pages_directory = "./pages"
content_directory = "./content"
assets_directory = "./assets"
"#,
        )
        .unwrap();
        fs::write(root.join("assets/style.css"), "body {}").unwrap();
        fs::write(
            root.join("pages/index.hbs"),
            "{{#each collections.posts}}[{{fm.title}}]{{/each}}",
        )
        .unwrap();
        fs::write(
            root.join("pages/posts/details.hbs"),
            "{{#each collections.posts}}<{{slug}}>{{/each}}",
        )
        .unwrap();
        fs::write(root.join("content/posts/a.md"), "---\ntitle: A\n---\n").unwrap();
        fs::write(root.join("content/posts/b.md"), "---\ntitle: B\n---\n").unwrap();

        let mut session = DevSession::new(root).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("dist/index.html")).unwrap(),
            "[A][B]"
        );

        fs::write(root.join("content/posts/a.md"), "---\ntitle: A2\n---\n").unwrap();
        fs::remove_file(root.join("content/posts/b.md")).unwrap();
        let messages = session
            .process_changes(&[
                SourceChange {
                    path: root.join("content/posts/a.md"),
                    change_type: ChangeType::Modified,
                    is_dir: false,
                },
                SourceChange {
                    path: root.join("content/posts/b.md"),
                    change_type: ChangeType::Deleted,
                    is_dir: false,
                },
            ])
            .unwrap();

        assert_eq!(messages, vec![ReloadMessage::full()]);
        assert_eq!(
            fs::read_to_string(root.join("dist/index.html")).unwrap(),
            "[A2]"
        );
        assert_eq!(
            fs::read_to_string(root.join("dist/posts/a.html")).unwrap(),
            "<a>"
        );
        assert!(!root.join("dist/posts/b.html").exists());
        assert!(
            root.join("dist/assets/style.css").exists(),
            "Collection changes should not wipe the output directory"
        );
    }

    #[test]
    fn test_process_changes_static_page_sees_collections() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::create_dir_all(root.join("pages/posts")).unwrap();
        fs::create_dir_all(root.join("content/posts")).unwrap();
        fs::write(
            root.join("balzac.toml"),
            r#"
output_directory = "./dist"
pages_directory = "./pages"
content_directory = "./content"
"#,
        )
        .unwrap();
        fs::write(root.join("pages/posts/details.hbs"), "{{fm.title}}").unwrap();
        fs::write(root.join("content/posts/a.md"), "---\ntitle: A\n---\n").unwrap();
        fs::write(root.join("pages/index.hbs"), "v1").unwrap();

        let mut session = DevSession::new(root).unwrap();

        fs::write(
            root.join("pages/index.hbs"),
            "{{#each collections.posts}}{{url}}{{/each}}",
        )
        .unwrap();
        let messages = session
            .process_changes(&[SourceChange {
                path: root.join("pages/index.hbs"),
                change_type: ChangeType::Modified,
                is_dir: false,
            }])
            .unwrap();

        assert_eq!(messages, vec![ReloadMessage::targeted("/")]);
        assert_eq!(
            fs::read_to_string(root.join("dist/index.html")).unwrap(),
            "/posts/a"
        );
    }

    #[test]
    fn test_process_changes_rebuilds_all_for_partial_change() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::create_dir_all(root.join("pages")).unwrap();
        fs::create_dir_all(root.join("partials")).unwrap();
        fs::write(
            root.join("balzac.toml"),
            r#"
output_directory = "./dist"
pages_directory = "./pages"
partials_directory = "./partials"
"#,
        )
        .unwrap();
        fs::write(root.join("partials/title.hbs"), "<h1>v1</h1>").unwrap();
        fs::write(root.join("pages/index.hbs"), "{{> title}}").unwrap();

        let mut session = DevSession::new(root).unwrap();
        session.full_rebuild().unwrap();

        fs::write(root.join("partials/title.hbs"), "<h1>v2</h1>").unwrap();
        let messages = session
            .process_changes(&[SourceChange {
                path: root.join("partials/title.hbs"),
                change_type: ChangeType::Modified,
                is_dir: false,
            }])
            .unwrap();

        assert_eq!(messages, vec![ReloadMessage::full()]);
        assert_eq!(
            fs::read_to_string(root.join("dist/index.html")).unwrap(),
            "<h1>v2</h1>"
        );
    }

    #[test]
    fn test_process_changes_copies_new_nested_asset() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::create_dir_all(root.join("pages")).unwrap();
        fs::create_dir_all(root.join("assets/img")).unwrap();
        fs::write(
            root.join("balzac.toml"),
            r#"
output_directory = "./dist"
pages_directory = "./pages"
assets_directory = "./assets"
"#,
        )
        .unwrap();
        fs::write(root.join("pages/index.hbs"), "<h1>home</h1>").unwrap();

        let mut session = DevSession::new(root).unwrap();
        session.full_rebuild().unwrap();
        let before = capture_snapshot(&session.resolved_config).unwrap();

        fs::create_dir_all(root.join("assets/img/icons")).unwrap();
        fs::write(root.join("assets/img/icons/logo.svg"), "<svg/>").unwrap();
        let after = capture_snapshot(&session.resolved_config).unwrap();
        let changes = diff_snapshots(&before, &after);
        assert!(
            changes
                .iter()
                .any(|change| change.path == root.join("assets/img/icons/logo.svg")),
            "Nested asset file should be detected as a change"
        );

        let messages = session.process_changes(&changes).unwrap();

        assert_eq!(messages, vec![ReloadMessage::full()]);
        assert_eq!(
            fs::read_to_string(root.join("dist/assets/img/icons/logo.svg")).unwrap(),
            "<svg/>"
        );
    }

    #[test]
    fn test_classify_nested_asset_change_as_full_reload() {
        let (_temp_dir, config) = setup_dev_project();
        let change = SourceChange {
            path: config.assets_directory.join("img").join("logo.png"),
            change_type: ChangeType::Modified,
            is_dir: false,
        };

        assert_eq!(classify_changes(&config, &[change]), ChangePlan::FullReload);
    }

    #[test]
    fn test_classify_tag_template_change_as_full_reload() {
        let (_temp_dir, config) = setup_dev_project();
        let change = SourceChange {
            path: config.tags_directory.join("a.hbs"),
            change_type: ChangeType::Modified,
            is_dir: false,
        };

        assert_eq!(classify_changes(&config, &[change]), ChangePlan::FullReload);
    }

    #[test]
    fn test_capture_snapshot_includes_tags_directory() {
        let (_temp_dir, config) = setup_dev_project();
        let tag_file = config.tags_directory.join("a.hbs");
        fs::write(&tag_file, "<a>{{{content}}}</a>").unwrap();

        let snapshot = capture_snapshot(&config).unwrap();
        assert!(
            snapshot.contains_key(&tag_file),
            "Snapshot should include tag template file"
        );
    }

    #[test]
    fn test_classify_script_helper_change_as_full_reload() {
        let (_temp_dir, config) = setup_dev_project();
        let change = SourceChange {
            path: config.helpers_directory.join("shout.rhai"),
            change_type: ChangeType::Modified,
            is_dir: false,
        };

        assert_eq!(classify_changes(&config, &[change]), ChangePlan::FullReload);
    }

    #[test]
    #[cfg(feature = "script-helpers")]
    fn test_script_helper_change_rebuilds_pages() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::create_dir_all(root.join("pages")).unwrap();
        fs::create_dir_all(root.join("helpers")).unwrap();
        fs::write(root.join("balzac.toml"), "").unwrap();
        fs::write(root.join("pages/index.hbs"), "{{shout \"hi\"}}").unwrap();
        fs::write(root.join("helpers/shout.rhai"), "params[0].to_upper()").unwrap();

        let mut session = DevSession::new(root).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("dist/index.html")).unwrap(),
            "HI"
        );

        let snapshot = capture_snapshot(&session.resolved_config).unwrap();
        fs::write(
            root.join("helpers/shout.rhai"),
            "params[0].to_upper() + \"!!!\"",
        )
        .unwrap();
        let changes = diff_snapshots(
            &snapshot,
            &capture_snapshot(&session.resolved_config).unwrap(),
        );
        let messages = session.process_changes(&changes).unwrap();

        assert_eq!(messages, vec![ReloadMessage::full()]);
        assert_eq!(
            fs::read_to_string(root.join("dist/index.html")).unwrap(),
            "HI!!!"
        );
    }

    #[test]
    fn test_process_changes_updates_and_prunes_listing_pages() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::create_dir_all(root.join("pages/posts")).unwrap();
        fs::create_dir_all(root.join("pages/tags")).unwrap();
        fs::create_dir_all(root.join("content/posts")).unwrap();
        fs::write(
            root.join("balzac.toml"),
            r#"
output_directory = "./dist"
pages_directory = "./pages"
content_directory = "./content"
base_url = "https://example.com"

[listings.posts]
per_page = 1

[taxonomies.tags]
"#,
        )
        .unwrap();
        fs::write(root.join("pages/posts/details.hbs"), "{{fm.title}}").unwrap();
        fs::write(
            root.join("pages/posts/list.hbs"),
            "{{#each pagination.items}}[{{fm.title}}]{{/each}}",
        )
        .unwrap();
        fs::write(
            root.join("pages/tags/term.hbs"),
            "{{#each pagination.items}}[{{fm.title}}]{{/each}}",
        )
        .unwrap();
        fs::write(
            root.join("content/posts/a.md"),
            "---\ntitle: A\ntags: [rust]\n---\n",
        )
        .unwrap();
        fs::write(
            root.join("content/posts/b.md"),
            "---\ntitle: B\ntags: [rust, web]\n---\n",
        )
        .unwrap();

        let mut session = DevSession::new(root).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("dist/posts/page/2/index.html")).unwrap(),
            "[B]"
        );
        assert_eq!(
            fs::read_to_string(root.join("dist/tags/web/index.html")).unwrap(),
            "[B]"
        );

        fs::write(
            root.join("content/posts/a.md"),
            "---\ntitle: A2\ntags: [rust]\n---\n",
        )
        .unwrap();
        fs::remove_file(root.join("content/posts/b.md")).unwrap();
        let messages = session
            .process_changes(&[
                SourceChange {
                    path: root.join("content/posts/a.md"),
                    change_type: ChangeType::Modified,
                    is_dir: false,
                },
                SourceChange {
                    path: root.join("content/posts/b.md"),
                    change_type: ChangeType::Deleted,
                    is_dir: false,
                },
            ])
            .unwrap();

        assert_eq!(messages, vec![ReloadMessage::full()]);
        assert_eq!(
            fs::read_to_string(root.join("dist/posts/index.html")).unwrap(),
            "[A2]"
        );
        assert_eq!(
            fs::read_to_string(root.join("dist/tags/rust/index.html")).unwrap(),
            "[A2]"
        );
        assert!(!root.join("dist/posts/page").exists());
        assert!(!root.join("dist/tags/web").exists());
        let sitemap = fs::read_to_string(root.join("dist/sitemap.xml")).unwrap();
        assert!(sitemap.contains("https://example.com/tags/rust/"));
        assert!(!sitemap.contains("/tags/web/"));
        assert!(!sitemap.contains("/posts/page/2/"));
    }

    fn setup_partial_project(partial: &str) -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("pages")).unwrap();
        fs::create_dir_all(root.join("partials")).unwrap();
        fs::write(
            root.join("balzac.toml"),
            r#"
output_directory = "./dist"
pages_directory = "./pages"
partials_directory = "./partials"
"#,
        )
        .unwrap();
        fs::write(root.join("partials/title.hbs"), partial).unwrap();
        fs::write(root.join("pages/index.hbs"), "<body>{{> title}}</body>").unwrap();
        fs::write(root.join("pages/about.hbs"), "<body>About</body>").unwrap();
        temp_dir
    }

    fn change(path: PathBuf) -> SourceChange {
        SourceChange {
            path,
            change_type: ChangeType::Modified,
            is_dir: false,
        }
    }

    #[test]
    fn test_failed_rebuild_keeps_error_until_next_successful_build() {
        let temp_dir = setup_partial_project("<h1>v1</h1>");
        let root = temp_dir.path();
        let mut session = DevSession::new(root).unwrap();
        assert_eq!(session.build_error, None);

        fs::write(root.join("partials/title.hbs"), "<h1>\n{{#if}}</h1>").unwrap();
        let error = session
            .process_changes(&[change(root.join("partials/title.hbs"))])
            .unwrap_err();
        let Error::Template { path, line, .. } = &error else {
            panic!("expected a template error, got {error:?}");
        };
        assert!(path.ends_with("partials/title.hbs"));
        assert_eq!(*line, Some(2));
        assert_eq!(session.build_error, Some(error.to_string()));

        let response = build_file_response(
            &session.resolved_config,
            "/",
            session.build_error.as_deref(),
        )
        .unwrap();
        let body = String::from_utf8(response.body).unwrap();
        assert!(body.contains("Balzac build failed"));
        assert!(body.contains("partials/title.hbs:2"));

        fs::write(root.join("partials/title.hbs"), "<h1>v2</h1>").unwrap();
        let messages = session
            .process_changes(&[change(root.join("partials/title.hbs"))])
            .unwrap();
        assert_eq!(messages, vec![ReloadMessage::full()]);
        assert_eq!(session.build_error, None);
        assert_eq!(
            fs::read_to_string(root.join("dist/index.html")).unwrap(),
            "<body><h1>v2</h1></body>"
        );
    }

    #[test]
    fn test_unrelated_change_does_not_clear_build_error() {
        let temp_dir = setup_partial_project("<h1>v1</h1>");
        let root = temp_dir.path();
        let mut session = DevSession::new(root).unwrap();

        fs::write(root.join("pages/index.hbs"), "{{#each}}").unwrap();
        assert!(
            session
                .process_changes(&[change(root.join("pages/index.hbs"))])
                .is_err()
        );

        // a targeted rebuild of another page would succeed, but the site is
        // rebuilt as a whole until the error is fixed
        fs::write(root.join("pages/about.hbs"), "<body>About v2</body>").unwrap();
        assert!(
            session
                .process_changes(&[change(root.join("pages/about.hbs"))])
                .is_err()
        );
        assert!(session.build_error.is_some());

        fs::write(root.join("pages/index.hbs"), "<body>fixed</body>").unwrap();
        let messages = session
            .process_changes(&[change(root.join("pages/index.hbs"))])
            .unwrap();
        assert_eq!(messages, vec![ReloadMessage::full()]);
        assert_eq!(session.build_error, None);
        assert_eq!(
            fs::read_to_string(root.join("dist/about.html")).unwrap(),
            "<body>About v2</body>"
        );
    }

    #[test]
    fn test_failed_initial_build_still_starts_session() {
        let temp_dir = setup_partial_project("{{/if}}");
        let root = temp_dir.path();

        let session = DevSession::new(root).unwrap();
        let error = session.build_error.clone().expect("build should fail");
        assert!(error.contains("partials/title.hbs:1:"), "{error}");

        // nothing was rendered, missing pages show the error instead
        let response =
            build_file_response(&session.resolved_config, "/", Some(error.as_str())).unwrap();
        assert_eq!(response.status, "404 Not Found");
        assert_eq!(response.content_type, "text/html; charset=utf-8");
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("Balzac build failed")
        );
    }

    #[test]
    fn test_inject_dev_scripts_escapes_build_error() {
        let (_temp_dir, config) = setup_dev_project();
        let injected = inject_dev_scripts(
            "<html><body></body></html>",
            &config,
            Some("bad </script><script>alert(1)</script>"),
        );
        assert!(!injected.contains("</script><script>alert"));
        assert!(injected.contains(r#"const buildError="bad \u003c/script>"#));

        let injected = inject_dev_scripts("<html><body></body></html>", &config, None);
        assert!(injected.contains("const buildError=null;"));
    }

    /// First event sent to a client connecting to the SSE endpoint.
    fn first_sse_event(build_error: Option<&str>) -> String {
        let (_temp_dir, config) = setup_dev_project();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        let config = RwLock::new(config);
        let build_error = RwLock::new(build_error.map(ToString::to_string));
        let events = EventBroker::default();
        thread::spawn(move || handle_connection(server, &config, &build_error, &events));

        write!(client, "GET {SSE_ENDPOINT} HTTP/1.1\r\n\r\n").unwrap();
        BufReader::new(client)
            .lines()
            .map(Result::unwrap)
            .find(|line| line.starts_with("data: "))
            .unwrap()
    }

    #[test]
    fn test_sse_client_first_receives_current_build_status() {
        assert_eq!(
            first_sse_event(Some("boom")),
            r#"data: {"kind":"status","url_path":null,"message":"boom"}"#
        );
        assert_eq!(
            first_sse_event(None),
            r#"data: {"kind":"status","url_path":null}"#
        );
    }

    #[test]
    fn test_error_message_serialization() {
        let payload = serde_json::to_string(&ReloadMessage::error("boom")).unwrap();
        assert_eq!(
            payload,
            r#"{"kind":"error","url_path":null,"message":"boom"}"#
        );
        let payload = serde_json::to_string(&ReloadMessage::full()).unwrap();
        assert_eq!(payload, r#"{"kind":"full","url_path":null}"#);
    }

    #[test]
    fn test_classify_listing_template_change_as_full_reload() {
        let (_temp_dir, mut config) = setup_dev_project();
        config
            .taxonomies
            .insert("tags".to_string(), crate::config::TaxonomyConfig::default());
        let change = SourceChange {
            path: config.pages_directory.join("tags").join("term.hbs"),
            change_type: ChangeType::Modified,
            is_dir: false,
        };

        assert_eq!(classify_changes(&config, &[change]), ChangePlan::FullReload);
    }

    fn setup_directory_style_session() -> (TempDir, DevSession) {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::create_dir_all(root.join("pages/posts")).unwrap();
        fs::create_dir_all(root.join("content/posts")).unwrap();
        fs::write(
            root.join("balzac.toml"),
            r#"
output_directory = "./dist"
pages_directory = "./pages"
content_directory = "./content"
url_style = "directory"
"#,
        )
        .unwrap();
        fs::write(root.join("pages/index.hbs"), "<h1>Home</h1>").unwrap();
        fs::write(root.join("pages/about.hbs"), "<h1>About v1</h1>").unwrap();
        fs::write(
            root.join("pages/posts/details.hbs"),
            "<h1>{{fm.title}}</h1>{{{content}}}",
        )
        .unwrap();
        fs::write(
            root.join("content/posts/post-1.md"),
            "---\ntitle: First\n---\n\nOne",
        )
        .unwrap();

        let session = DevSession::new(root).unwrap();
        (temp_dir, session)
    }

    #[test]
    fn test_directory_style_classifies_with_trailing_slash_urls() {
        let (_temp_dir, session) = setup_directory_style_session();
        let config = &session.resolved_config;
        let change = SourceChange {
            path: config.content_directory.join("posts/post-1.md"),
            change_type: ChangeType::Modified,
            is_dir: false,
        };

        assert_eq!(
            classify_changes(config, &[change]),
            ChangePlan::Targeted(vec![TargetedChange {
                kind: TargetKind::CollectionItem,
                source_path: config.content_directory.join("posts/post-1.md"),
                output_path: config.output_directory.join("posts/post-1/index.html"),
                url_path: "/posts/post-1/".to_string(),
                change_type: ChangeType::Modified,
            }])
        );
    }

    #[test]
    fn test_directory_style_targeted_rebuilds() {
        let (temp_dir, mut session) = setup_directory_style_session();
        let root = temp_dir.path();
        assert!(root.join("dist/about/index.html").is_file());
        assert!(root.join("dist/posts/post-1/index.html").is_file());
        assert!(!root.join("dist/about.html").exists());

        fs::write(root.join("pages/about.hbs"), "<h1>About v2</h1>").unwrap();
        let messages = session
            .process_changes(&[SourceChange {
                path: root.join("pages/about.hbs"),
                change_type: ChangeType::Modified,
                is_dir: false,
            }])
            .unwrap();
        assert_eq!(messages, vec![ReloadMessage::targeted("/about/")]);
        assert_eq!(
            fs::read_to_string(root.join("dist/about/index.html")).unwrap(),
            "<h1>About v2</h1>"
        );

        fs::write(
            root.join("content/posts/post-1.md"),
            "---\ntitle: First Updated\n---\n\nTwo",
        )
        .unwrap();
        session
            .process_changes(&[SourceChange {
                path: root.join("content/posts/post-1.md"),
                change_type: ChangeType::Modified,
                is_dir: false,
            }])
            .unwrap();
        assert!(
            fs::read_to_string(root.join("dist/posts/post-1/index.html"))
                .unwrap()
                .contains("First Updated")
        );
    }

    #[test]
    fn test_directory_style_new_page_and_deletion() {
        let (temp_dir, mut session) = setup_directory_style_session();
        let root = temp_dir.path();

        fs::write(root.join("pages/contact.hbs"), "<h1>Contact</h1>").unwrap();
        session
            .process_changes(&[SourceChange {
                path: root.join("pages/contact.hbs"),
                change_type: ChangeType::Created,
                is_dir: false,
            }])
            .unwrap();
        assert!(root.join("dist/contact/index.html").is_file());

        fs::remove_file(root.join("pages/contact.hbs")).unwrap();
        let messages = session
            .process_changes(&[SourceChange {
                path: root.join("pages/contact.hbs"),
                change_type: ChangeType::Deleted,
                is_dir: false,
            }])
            .unwrap();
        assert_eq!(messages, vec![ReloadMessage::targeted("/contact/")]);
        assert!(!root.join("dist/contact").exists());

        fs::remove_file(root.join("content/posts/post-1.md")).unwrap();
        session
            .process_changes(&[SourceChange {
                path: root.join("content/posts/post-1.md"),
                change_type: ChangeType::Deleted,
                is_dir: false,
            }])
            .unwrap();
        assert!(!root.join("dist/posts/post-1").exists());
        assert!(root.join("dist/about/index.html").is_file());
        assert!(root.join("dist/index.html").is_file());
    }

    #[test]
    fn test_directory_style_serving_and_redirects() {
        let (_temp_dir, session) = setup_directory_style_session();
        let config = &session.resolved_config;
        let output = &config.output_directory;

        assert_eq!(
            resolve_output_path(output, "/about/"),
            Some(output.join("about/index.html"))
        );
        assert_eq!(
            resolve_output_path(output, "/posts/post-1/?ref=x"),
            Some(output.join("posts/post-1/index.html"))
        );
        assert_eq!(
            directory_redirect(config, "/about"),
            Some("/about/".to_string())
        );
        assert_eq!(
            directory_redirect(config, "/posts/post-1?ref=x"),
            Some("/posts/post-1/?ref=x".to_string())
        );
        assert_eq!(directory_redirect(config, "/about/"), None);
        assert_eq!(directory_redirect(config, "/"), None);
        assert_eq!(directory_redirect(config, "/missing"), None);
        assert_eq!(directory_redirect(config, "/index.html"), None);
        assert_eq!(
            directory_redirect(config, "//about"),
            Some("/about/".to_string()),
            "redirect must stay on the dev server"
        );
        assert_eq!(directory_redirect(config, "/."), None);
    }

    #[test]
    fn test_directory_style_redirects_listing_urls() {
        let (_temp_dir, session) = setup_directory_style_session();
        let config = &session.resolved_config;
        let output = &config.output_directory;
        for dir in ["posts/page/2", "tags/\u{e9}conomie"] {
            fs::create_dir_all(output.join(dir)).unwrap();
            fs::write(output.join(dir).join("index.html"), "").unwrap();
        }

        assert_eq!(
            directory_redirect(config, "/posts/page/2"),
            Some("/posts/page/2/".to_string())
        );
        assert_eq!(
            directory_redirect(config, "/tags/%C3%A9conomie"),
            Some("/tags/%C3%A9conomie/".to_string()),
            "the location keeps the request's percent-encoding"
        );
        assert_eq!(directory_redirect(config, "/posts/index.html"), None);
    }

    #[test]
    fn test_directory_style_page_colliding_with_listing_keeps_listing() {
        let (temp_dir, mut session) = setup_directory_style_session();
        let root = temp_dir.path();
        fs::write(
            root.join("balzac.toml"),
            r#"
output_directory = "./dist"
pages_directory = "./pages"
content_directory = "./content"
url_style = "directory"

[listings.posts]
"#,
        )
        .unwrap();
        fs::write(root.join("pages/posts/list.hbs"), "listing").unwrap();
        session.full_rebuild().unwrap();
        let listing = root.join("dist/posts/index.html");
        assert_eq!(fs::read_to_string(&listing).unwrap(), "listing");

        fs::write(root.join("pages/posts.hbs"), "page").unwrap();
        let created = session.process_changes(&[SourceChange {
            path: root.join("pages/posts.hbs"),
            change_type: ChangeType::Created,
            is_dir: false,
        }]);
        assert!(created.is_err(), "the collision should be reported");
        assert_eq!(fs::read_to_string(&listing).unwrap(), "listing");

        fs::remove_file(root.join("pages/posts.hbs")).unwrap();
        session
            .process_changes(&[SourceChange {
                path: root.join("pages/posts.hbs"),
                change_type: ChangeType::Deleted,
                is_dir: false,
            }])
            .unwrap();
        assert_eq!(fs::read_to_string(&listing).unwrap(), "listing");
    }

    #[test]
    fn test_directory_style_page_colliding_with_asset_keeps_asset() {
        let (temp_dir, mut session) = setup_directory_style_session();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("assets/index.html"), "asset").unwrap();
        session.full_rebuild().unwrap();
        let asset = root.join("dist/assets/index.html");
        assert_eq!(fs::read_to_string(&asset).unwrap(), "asset");

        fs::write(root.join("pages/assets.hbs"), "page").unwrap();
        let created = session.process_changes(&[SourceChange {
            path: root.join("pages/assets.hbs"),
            change_type: ChangeType::Created,
            is_dir: false,
        }]);
        let error = created.expect_err("the collision should be reported");
        assert!(
            matches!(&error, Error::Io(e) if e.kind() == std::io::ErrorKind::AlreadyExists),
            "{error:?}"
        );
        assert_eq!(fs::read_to_string(&asset).unwrap(), "asset");

        fs::remove_file(root.join("pages/assets.hbs")).unwrap();
        session
            .process_changes(&[SourceChange {
                path: root.join("pages/assets.hbs"),
                change_type: ChangeType::Deleted,
                is_dir: false,
            }])
            .unwrap();
        assert_eq!(fs::read_to_string(&asset).unwrap(), "asset");
    }

    #[test]
    fn test_directory_style_item_taking_over_stale_term_page_is_kept() {
        let (temp_dir, mut session) = setup_directory_style_session();
        let root = temp_dir.path();
        fs::write(
            root.join("balzac.toml"),
            r#"
output_directory = "./dist"
pages_directory = "./pages"
content_directory = "./content"
url_style = "directory"

[taxonomies.tags]
collections = ["posts"]
"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("pages/tags")).unwrap();
        fs::create_dir_all(root.join("content/tags")).unwrap();
        fs::write(root.join("pages/tags/term.hbs"), "term").unwrap();
        fs::write(root.join("pages/tags/details.hbs"), "tag item").unwrap();
        fs::write(
            root.join("content/posts/post-1.md"),
            "---\ntitle: First\ntags: [rust]\n---\n",
        )
        .unwrap();
        session.full_rebuild().unwrap();
        let output = root.join("dist/tags/rust/index.html");
        assert_eq!(fs::read_to_string(&output).unwrap(), "term");

        // In one batch, the term page goes away and an item takes over its file.
        fs::write(
            root.join("content/posts/post-1.md"),
            "---\ntitle: First\n---\n",
        )
        .unwrap();
        fs::write(root.join("content/tags/rust.md"), "---\ntitle: Rust\n---\n").unwrap();
        session
            .process_changes(&[
                SourceChange {
                    path: root.join("content/posts/post-1.md"),
                    change_type: ChangeType::Modified,
                    is_dir: false,
                },
                SourceChange {
                    path: root.join("content/tags/rust.md"),
                    change_type: ChangeType::Created,
                    is_dir: false,
                },
            ])
            .unwrap();
        assert_eq!(fs::read_to_string(&output).unwrap(), "tag item");
    }

    #[test]
    fn test_directory_style_collection_item_colliding_with_asset_keeps_asset() {
        let (temp_dir, mut session) = setup_directory_style_session();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("assets/index")).unwrap();
        fs::write(root.join("assets/index/index.html"), "asset").unwrap();
        fs::create_dir_all(root.join("pages/assets")).unwrap();
        fs::create_dir_all(root.join("content/assets")).unwrap();
        fs::write(root.join("pages/assets/details.hbs"), "item").unwrap();
        session.full_rebuild().unwrap();
        let asset = root.join("dist/assets/index/index.html");
        assert_eq!(fs::read_to_string(&asset).unwrap(), "asset");

        // Collection changes re-render the whole site, not a single page.
        fs::write(root.join("content/assets/index.md"), "---\ntitle: I\n---\n").unwrap();
        let created = session.process_changes(&[SourceChange {
            path: root.join("content/assets/index.md"),
            change_type: ChangeType::Created,
            is_dir: false,
        }]);
        let error = created.expect_err("the collision should be reported");
        assert!(
            matches!(&error, Error::Io(e) if e.kind() == std::io::ErrorKind::AlreadyExists),
            "{error:?}"
        );
        assert_eq!(fs::read_to_string(&asset).unwrap(), "asset");

        fs::remove_file(root.join("content/assets/index.md")).unwrap();
        session
            .process_changes(&[SourceChange {
                path: root.join("content/assets/index.md"),
                change_type: ChangeType::Deleted,
                is_dir: false,
            }])
            .unwrap();
        assert_eq!(fs::read_to_string(&asset).unwrap(), "asset");
    }

    #[test]
    fn test_directory_style_ignores_pages_named_like_parent_directories() {
        let (temp_dir, mut session) = setup_directory_style_session();
        let root = temp_dir.path();
        let outside = root.join("index.html");
        fs::write(&outside, "outside").unwrap();

        // `...hbs` has the stem `..` and `..hbs` the stem `.`.
        for name in ["...hbs", "..hbs"] {
            fs::write(root.join("pages").join(name), "escaped").unwrap();
            for change_type in [ChangeType::Created, ChangeType::Deleted] {
                session
                    .process_changes(&[SourceChange {
                        path: root.join("pages").join(name),
                        change_type,
                        is_dir: false,
                    }])
                    .unwrap();
            }
        }
        session.full_rebuild().unwrap();

        assert_eq!(fs::read_to_string(&outside).unwrap(), "outside");
        assert_eq!(
            fs::read_to_string(root.join("dist/index.html")).unwrap(),
            "<h1>Home</h1>"
        );
    }

    #[test]
    fn test_redirect_is_temporary_and_uncached() {
        let headers = redirect_headers("/about/");
        assert!(headers.starts_with("HTTP/1.1 307 Temporary Redirect\r\n"));
        assert!(headers.contains("\r\nLocation: /about/\r\n"));
        assert!(headers.contains("\r\nCache-Control: no-store\r\n"));
        assert!(headers.ends_with("\r\n\r\n"));
    }

    #[test]
    fn test_directory_style_serves_dotted_page_names() {
        let (temp_dir, mut session) = setup_directory_style_session();
        let root = temp_dir.path();
        fs::write(root.join("pages/about.en.hbs"), "<h1>About EN</h1>").unwrap();
        session
            .process_changes(&[SourceChange {
                path: root.join("pages/about.en.hbs"),
                change_type: ChangeType::Created,
                is_dir: false,
            }])
            .unwrap();

        let output = &session.resolved_config.output_directory;
        assert_eq!(
            resolve_output_path(output, "/about.en/"),
            Some(output.join("about.en/index.html"))
        );
        assert_eq!(
            directory_redirect(&session.resolved_config, "/about.en"),
            Some("/about.en/".to_string())
        );
    }

    #[test]
    fn test_deleting_item_of_unrendered_collection_is_a_no_op() {
        let (temp_dir, mut session) = setup_directory_style_session();
        let root = temp_dir.path();
        // `drafts` has no details.hbs, so its items were never rendered.
        fs::create_dir_all(root.join("content/drafts")).unwrap();

        let messages = session.process_changes(&[SourceChange {
            path: root.join("content/drafts/wip.md"),
            change_type: ChangeType::Deleted,
            is_dir: false,
        }]);
        assert!(messages.is_ok(), "{messages:?}");
    }

    #[test]
    fn test_extensionless_style_never_redirects() {
        let (_temp_dir, config) = setup_dev_project();
        fs::create_dir_all(config.output_directory.join("docs")).unwrap();
        fs::write(config.output_directory.join("docs/index.html"), "").unwrap();

        assert_eq!(directory_redirect(&config, "/docs"), None);
    }

    #[test]
    fn test_process_changes_removes_stale_feeds() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::create_dir_all(root.join("pages/posts")).unwrap();
        fs::create_dir_all(root.join("content/posts")).unwrap();
        fs::write(
            root.join("balzac.toml"),
            r#"
output_directory = "./dist"
pages_directory = "./pages"
content_directory = "./content"
base_url = "https://example.com"

[feed]
combined = true
"#,
        )
        .unwrap();
        fs::write(root.join("pages/posts/details.hbs"), "{{{content}}}").unwrap();
        fs::write(
            root.join("content/posts/a.md"),
            "---\ntitle: A\ndate: 2024-01-15\n---\n",
        )
        .unwrap();

        let mut session = DevSession::new(root).unwrap();
        assert!(root.join("dist/posts/atom.xml").exists());
        assert!(root.join("dist/atom.xml").exists());

        // Clearing the only post's date leaves nothing to publish
        fs::write(root.join("content/posts/a.md"), "---\ntitle: A\n---\n").unwrap();
        session
            .process_changes(&[SourceChange {
                path: root.join("content/posts/a.md"),
                change_type: ChangeType::Modified,
                is_dir: false,
            }])
            .unwrap();

        assert!(!root.join("dist/posts/atom.xml").exists());
        assert!(!root.join("dist/atom.xml").exists());
    }

    /// Writes a project whose hooks append their phase name to `hooks.log`.
    fn setup_hooks_project(dev: bool, extra_hooks: &str) -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("pages")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        let mut hooks = String::new();
        for phase in [
            "render_init_before",
            "render_init_after",
            "build_before",
            "render_before",
            "render_after",
            "build_after",
        ] {
            if !extra_hooks.contains(&format!("{phase} =")) {
                writeln!(hooks, "{phase} = \"sh -c 'echo {phase} >> hooks.log'\"").unwrap();
            }
        }
        fs::write(
            root.join("balzac.toml"),
            format!(
                r#"
output_directory = "./dist"
pages_directory = "./pages"
assets_directory = "./assets"

[hooks]
dev = {dev}
{hooks}{extra_hooks}
"#
            ),
        )
        .unwrap();
        fs::write(root.join("pages/index.hbs"), "<h1>Home</h1>").unwrap();
        fs::write(root.join("pages/about.hbs"), "<h1>About</h1>").unwrap();
        temp_dir
    }

    fn take_hooks_log(root: &Path) -> Vec<String> {
        let log_path = root.join("hooks.log");
        let log = fs::read_to_string(&log_path).unwrap_or_default();
        let _ = fs::remove_file(log_path);
        log.lines().map(str::to_string).collect()
    }

    fn page_change(root: &Path, page: &str) -> SourceChange {
        SourceChange {
            path: root.join("pages").join(page),
            change_type: ChangeType::Modified,
            is_dir: false,
        }
    }

    #[test]
    fn test_dev_hooks_are_skipped_unless_enabled() {
        let temp_dir = setup_hooks_project(false, "");
        let root = temp_dir.path();

        let mut session = DevSession::new(root).unwrap();
        session.full_rebuild().unwrap();

        assert_eq!(take_hooks_log(root), Vec::<String>::new());
    }

    #[test]
    fn test_dev_full_build_runs_all_hooks_in_build_order() {
        let temp_dir = setup_hooks_project(true, "");
        let root = temp_dir.path();
        let all_phases = [
            "render_init_before",
            "render_init_after",
            "build_before",
            "render_before",
            "render_after",
            "build_after",
        ];

        let mut session = DevSession::new(root).unwrap();
        assert_eq!(take_hooks_log(root), all_phases);

        fs::write(root.join("assets/site.css"), "body {}").unwrap();
        let messages = session
            .process_changes(&[SourceChange {
                path: root.join("assets/site.css"),
                change_type: ChangeType::Created,
                is_dir: false,
            }])
            .unwrap();
        assert_eq!(messages, vec![ReloadMessage::full()]);
        assert_eq!(take_hooks_log(root), all_phases);
    }

    #[test]
    fn test_dev_targeted_rebuild_runs_only_render_hooks() {
        let temp_dir = setup_hooks_project(true, "");
        let root = temp_dir.path();
        let mut session = DevSession::new(root).unwrap();
        take_hooks_log(root);

        fs::write(root.join("pages/about.hbs"), "<h1>About v2</h1>").unwrap();
        let messages = session
            .process_changes(&[page_change(root, "about.hbs")])
            .unwrap();

        assert_eq!(messages, vec![ReloadMessage::targeted("/about")]);
        assert_eq!(take_hooks_log(root), ["render_before", "render_after"]);
    }

    #[test]
    fn test_dev_failing_hook_aborts_rebuild_and_keeps_previous_output() {
        // fails only while a `fail` marker exists, so the initial build succeeds
        let temp_dir = setup_hooks_project(true, "render_before = \"test ! -e fail\"");
        let root = temp_dir.path();
        let mut session = DevSession::new(root).unwrap();

        fs::write(root.join("fail"), "").unwrap();
        fs::write(root.join("pages/about.hbs"), "<h1>About v2</h1>").unwrap();
        let error = session
            .process_changes(&[page_change(root, "about.hbs")])
            .unwrap_err();
        assert!(error.to_string().contains("render_before hook failed"));
        assert_eq!(
            fs::read_to_string(root.join("dist/about.html")).unwrap(),
            "<h1>About</h1>"
        );

        fs::remove_file(root.join("fail")).unwrap();
        session
            .process_changes(&[page_change(root, "about.hbs")])
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("dist/about.html")).unwrap(),
            "<h1>About v2</h1>"
        );
    }

    #[test]
    fn test_dev_targeted_rebuild_copies_assets_written_by_render_hook() {
        let temp_dir = setup_hooks_project(
            true,
            "render_before = \"sh -c 'echo render_before >> hooks.log; echo built >> assets/generated.css'\"",
        );
        let root = temp_dir.path();
        let mut session = DevSession::new(root).unwrap();
        take_hooks_log(root);

        fs::write(root.join("pages/about.hbs"), "<h1>About v2</h1>").unwrap();
        let messages = session
            .process_changes(&[page_change(root, "about.hbs")])
            .unwrap();

        assert_eq!(messages, vec![ReloadMessage::full()]);
        assert_eq!(
            fs::read_to_string(root.join("dist/assets/generated.css")).unwrap(),
            "built\nbuilt\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("dist/about.html")).unwrap(),
            "<h1>About v2</h1>"
        );
        // the hook writes are applied with hooks paused, so hooks ran only once
        assert_eq!(take_hooks_log(root), ["render_before", "render_after"]);
    }

    #[test]
    fn test_dev_hook_writes_keep_output_written_by_build_after() {
        let temp_dir = setup_hooks_project(
            true,
            "render_init_before = \"sh -c 'echo built >> assets/generated.css'\"\n\
             build_after = \"sh -c 'echo extra > dist/extra.txt'\"",
        );
        let root = temp_dir.path();
        let mut session = DevSession::new(root).unwrap();
        assert!(root.join("dist/extra.txt").is_file());

        fs::write(root.join("assets/site.css"), "body {}").unwrap();
        session
            .process_changes(&[SourceChange {
                path: root.join("assets/site.css"),
                change_type: ChangeType::Created,
                is_dir: false,
            }])
            .unwrap();

        assert!(root.join("dist/extra.txt").is_file());
        assert_eq!(
            fs::read_to_string(root.join("dist/assets/generated.css")).unwrap(),
            "built\nbuilt\n"
        );
    }

    #[test]
    fn test_dev_full_build_does_not_reapply_assets_it_already_copied() {
        // a redundant re-render after build_after would drop its edit
        let temp_dir = setup_hooks_project(
            true,
            "render_init_before = \"sh -c 'echo built >> assets/generated.css'\"\n\
             build_after = \"sh -c 'echo post >> dist/index.html'\"",
        );
        let root = temp_dir.path();
        let mut session = DevSession::new(root).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("dist/index.html")).unwrap(),
            "<h1>Home</h1>post\n"
        );

        fs::write(root.join("assets/site.css"), "body {}").unwrap();
        session
            .process_changes(&[SourceChange {
                path: root.join("assets/site.css"),
                change_type: ChangeType::Created,
                is_dir: false,
            }])
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("dist/index.html")).unwrap(),
            "<h1>Home</h1>post\n"
        );
    }

    #[test]
    fn test_dev_full_build_applies_pages_written_after_they_were_read() {
        let temp_dir = setup_hooks_project(
            true,
            "render_after = \"sh -c 'echo generated > pages/generated.hbs'\"",
        );
        let root = temp_dir.path();
        DevSession::new(root).unwrap();

        assert_eq!(
            fs::read_to_string(root.join("dist/generated.html")).unwrap(),
            "generated\n"
        );
    }

    #[test]
    fn test_dev_hook_writes_mirror_deleted_and_updated_assets() {
        // the hook only rewrites assets once `flip` exists
        let temp_dir = setup_hooks_project(
            true,
            "render_before = \"sh -c 'test ! -e flip || { rm assets/old.css; echo v2 > assets/kept.css; }'\"",
        );
        let root = temp_dir.path();
        fs::write(root.join("assets/old.css"), "old").unwrap();
        fs::write(root.join("assets/kept.css"), "v1").unwrap();
        let mut session = DevSession::new(root).unwrap();
        assert!(root.join("dist/assets/old.css").is_file());

        fs::write(root.join("flip"), "").unwrap();
        session
            .process_changes(&[page_change(root, "about.hbs")])
            .unwrap();

        assert!(!root.join("dist/assets/old.css").exists());
        assert_eq!(
            fs::read_to_string(root.join("dist/assets/kept.css")).unwrap(),
            "v2\n"
        );
    }

    #[test]
    fn test_dev_enabling_hooks_mid_session_applies_their_writes() {
        let temp_dir = setup_hooks_project(false, "");
        let root = temp_dir.path();
        let mut session = DevSession::new(root).unwrap();

        let config = fs::read_to_string(root.join("balzac.toml"))
            .unwrap()
            .replace("dev = false", "dev = true")
            .replace(
                "render_after = \"sh -c 'echo render_after >> hooks.log'\"",
                "render_after = \"sh -c 'echo generated > pages/generated.hbs'\"",
            );
        fs::write(root.join("balzac.toml"), config).unwrap();
        session
            .process_changes(&[SourceChange {
                path: root.join("balzac.toml"),
                change_type: ChangeType::Modified,
                is_dir: false,
            }])
            .unwrap();

        assert!(root.join("dist/generated.html").is_file());
    }

    #[test]
    fn test_dev_hook_writes_remove_outputs_of_deleted_pages() {
        // once `flip` exists, the hook deletes a page and touches an asset
        let temp_dir = setup_hooks_project(
            true,
            "render_after = \"sh -c 'test ! -e flip || { rm -f pages/about.hbs; echo v2 > assets/site.css; }'\"",
        );
        let root = temp_dir.path();
        let mut session = DevSession::new(root).unwrap();
        assert!(root.join("dist/about.html").is_file());

        fs::write(root.join("flip"), "").unwrap();
        fs::write(root.join("pages/index.hbs"), "<h1>Home v2</h1>").unwrap();
        session
            .process_changes(&[page_change(root, "index.hbs")])
            .unwrap();

        assert!(!root.join("dist/about.html").exists());
        assert_eq!(
            fs::read_to_string(root.join("dist/assets/site.css")).unwrap(),
            "v2\n"
        );
    }

    #[test]
    fn test_dev_hook_writes_replace_asset_file_with_directory_and_back() {
        let temp_dir = setup_hooks_project(
            true,
            "render_before = \"sh -c 'test ! -e flip || { rm -rf assets/bundle; mkdir assets/bundle; echo js > assets/bundle/app.js; }'\"\n\
             render_after = \"sh -c 'test ! -e flop || { rm -rf assets/bundle; echo file > assets/bundle; }'\"",
        );
        let root = temp_dir.path();
        fs::write(root.join("assets/bundle"), "file").unwrap();
        let mut session = DevSession::new(root).unwrap();

        fs::write(root.join("flip"), "").unwrap();
        session
            .process_changes(&[page_change(root, "about.hbs")])
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("dist/assets/bundle/app.js")).unwrap(),
            "js\n"
        );

        fs::remove_file(root.join("flip")).unwrap();
        fs::write(root.join("flop"), "").unwrap();
        session
            .process_changes(&[page_change(root, "about.hbs")])
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("dist/assets/bundle")).unwrap(),
            "file\n"
        );
    }

    #[test]
    fn test_dev_hook_changing_config_rebuilds_from_new_directories() {
        // once `flip` exists, the hook points the config at another assets dir
        let temp_dir = setup_hooks_project(true, "render_before = \"sh switch.sh\"");
        let root = temp_dir.path();
        fs::write(
            root.join("switch.sh"),
            "test ! -e flip || printf 'output_directory = \"./dist\"\\n\
             pages_directory = \"./pages\"\\nassets_directory = \"./static\"\\n' > balzac.toml\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("static")).unwrap();
        fs::write(root.join("static/new.css"), "new").unwrap();
        let mut session = DevSession::new(root).unwrap();
        assert!(!root.join("dist/assets/new.css").exists());

        fs::write(root.join("flip"), "").unwrap();
        session
            .process_changes(&[page_change(root, "about.hbs")])
            .unwrap();

        assert_eq!(
            fs::read_to_string(root.join("dist/assets/new.css")).unwrap(),
            "new"
        );
    }

    #[test]
    fn test_dev_hook_written_asset_never_replaces_a_rendered_page() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("pages")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(
            root.join("balzac.toml"),
            r#"
output_directory = "./dist"
pages_directory = "./pages"
assets_directory = "./assets"
url_style = "directory"

[hooks]
dev = true
render_after = "sh -c 'test ! -e flip || echo asset > assets/index.html'"
"#,
        )
        .unwrap();
        fs::write(root.join("pages/index.hbs"), "<h1>Home</h1>").unwrap();
        fs::write(root.join("pages/assets.hbs"), "<h1>Assets page</h1>").unwrap();
        let mut session = DevSession::new(root).unwrap();

        fs::write(root.join("flip"), "").unwrap();
        let error = session
            .process_changes(&[page_change(root, "index.hbs")])
            .unwrap_err();

        assert!(error.to_string().contains("would overwrite"), "{error}");
        assert_eq!(
            fs::read_to_string(root.join("dist/assets/index.html")).unwrap(),
            "<h1>Assets page</h1>"
        );
    }

    #[test]
    fn test_dev_hook_deleting_assets_keeps_pages_rendered_there() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("pages")).unwrap();
        fs::create_dir_all(root.join("assets/css")).unwrap();
        fs::write(
            root.join("balzac.toml"),
            r#"
output_directory = "./dist"
pages_directory = "./pages"
assets_directory = "./assets"
url_style = "directory"

[hooks]
dev = true
render_after = "sh -c 'test ! -e flip || rm -rf assets'"
"#,
        )
        .unwrap();
        fs::write(root.join("pages/index.hbs"), "<h1>Home</h1>").unwrap();
        fs::write(root.join("pages/assets.hbs"), "<h1>Assets page</h1>").unwrap();
        fs::write(root.join("assets/css/site.css"), "body {}").unwrap();
        let mut session = DevSession::new(root).unwrap();
        assert!(root.join("dist/assets/css/site.css").is_file());

        fs::write(root.join("flip"), "").unwrap();
        session
            .process_changes(&[page_change(root, "index.hbs")])
            .unwrap();

        assert!(!root.join("dist/assets/css").exists());
        assert_eq!(
            fs::read_to_string(root.join("dist/assets/index.html")).unwrap(),
            "<h1>Assets page</h1>"
        );
    }

    #[test]
    fn test_merge_reload_messages() {
        assert_eq!(
            merge_reload_messages(
                vec![ReloadMessage::targeted("/about")],
                vec![ReloadMessage::full()]
            ),
            vec![ReloadMessage::full()]
        );
        assert_eq!(
            merge_reload_messages(
                vec![ReloadMessage::targeted("/about")],
                vec![
                    ReloadMessage::targeted("/about"),
                    ReloadMessage::targeted("/")
                ]
            ),
            vec![
                ReloadMessage::targeted("/about"),
                ReloadMessage::targeted("/")
            ]
        );
    }

    #[test]
    fn test_dev_failing_hook_in_initial_build_is_reported_not_fatal() {
        let temp_dir = setup_hooks_project(true, "build_before = \"test -e ready\"");
        let root = temp_dir.path();

        let mut session = DevSession::new(root).unwrap();
        let error = session.build_error.clone().unwrap();
        assert!(error.contains("build_before hook failed"), "{error}");

        // the next change rebuilds the whole site, hooks included
        fs::write(root.join("ready"), "").unwrap();
        let messages = session
            .process_changes(&[page_change(root, "about.hbs")])
            .unwrap();
        assert_eq!(messages, vec![ReloadMessage::full()]);
        assert!(session.build_error.is_none());
        assert!(root.join("dist/index.html").is_file());
    }

    #[test]
    fn test_dev_hook_writing_into_watched_directory_does_not_retrigger() {
        let temp_dir = setup_hooks_project(
            true,
            "render_init_before = \"sh -c 'echo built >> assets/generated.css'\"",
        );
        let root = temp_dir.path();
        let mut session = DevSession::new(root).unwrap();
        let snapshot = capture_snapshot(&session.resolved_config).unwrap();

        fs::write(root.join("assets/site.css"), "body {}").unwrap();
        let before_rebuild = capture_snapshot(&session.resolved_config).unwrap();
        session
            .process_changes(&diff_snapshots(&snapshot, &before_rebuild))
            .unwrap();
        // the watch loop re-snapshots after each rebuild, as done here
        let snapshot = capture_snapshot(&session.resolved_config).unwrap();
        assert!(
            diff_snapshots(&before_rebuild, &snapshot)
                .iter()
                .any(|change| change.path == root.join("assets/generated.css")),
            "the hook should have written into the watched assets directory"
        );

        let changes = diff_snapshots(
            &snapshot,
            &capture_snapshot(&session.resolved_config).unwrap(),
        );
        assert!(changes.is_empty(), "hook output retriggered: {changes:?}");
        assert_eq!(
            fs::read_to_string(root.join("dist/assets/generated.css")).unwrap(),
            "built\nbuilt\n"
        );
    }

    #[test]
    fn test_resolve_output_path_supports_dotted_urls() {
        let (_temp_dir, config) = setup_dev_project();
        let posts = config.output_directory.join("posts");
        fs::create_dir_all(&posts).unwrap();
        fs::write(posts.join("release-1.2.html"), "<h1>1.2</h1>").unwrap();

        let path = resolve_output_path(&config.output_directory, "/posts/release-1.2").unwrap();
        assert_eq!(path, posts.join("release-1.2.html"));
        assert!(resolve_output_path(&config.output_directory, "/posts/missing.css").is_none());
    }

    fn setup_collection_session(toml: &str, files: &[(&str, &str)]) -> (TempDir, DevSession) {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        fs::write(root.join("balzac.toml"), toml).unwrap();
        for (path, content) in files {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        let session = DevSession::new(root).unwrap();
        (temp_dir, session)
    }

    fn modify(session: &mut DevSession, path: &Path, content: &str) -> Vec<ReloadMessage> {
        fs::write(path, content).unwrap();
        session
            .process_changes(&[SourceChange {
                path: path.to_path_buf(),
                change_type: ChangeType::Modified,
                is_dir: false,
            }])
            .unwrap()
    }

    const DETAILS: (&str, &str) = ("pages/posts/details.hbs", "<h1>{{fm.title}}</h1>");

    #[test]
    fn test_process_changes_moves_output_when_slug_changes() {
        let (temp_dir, mut session) = setup_collection_session(
            "",
            &[DETAILS, ("content/posts/post.md", "---\ntitle: v1\n---\n")],
        );
        let root = temp_dir.path();
        let source = root.join("content/posts/post.md");
        assert!(root.join("dist/posts/post.html").exists());

        let messages = modify(
            &mut session,
            &source,
            "---\ntitle: v2\nslug: renamed\n---\n",
        );
        assert_eq!(messages, vec![ReloadMessage::full()]);
        assert!(!root.join("dist/posts/post.html").exists());
        assert_eq!(
            fs::read_to_string(root.join("dist/posts/renamed.html")).unwrap(),
            "<h1>v2</h1>"
        );

        // Editing without touching the slug keeps the output in place
        modify(
            &mut session,
            &source,
            "---\ntitle: v3\nslug: renamed\n---\n",
        );
        assert_eq!(
            fs::read_to_string(root.join("dist/posts/renamed.html")).unwrap(),
            "<h1>v3</h1>"
        );

        // Removing the slug goes back to the filename
        modify(&mut session, &source, "---\ntitle: v4\n---\n");
        assert!(!root.join("dist/posts/renamed.html").exists());
        assert!(root.join("dist/posts/post.html").exists());
    }

    #[test]
    fn test_process_changes_removes_slug_output_on_delete() {
        let (temp_dir, mut session) = setup_collection_session(
            "",
            &[
                DETAILS,
                ("content/posts/post.md", "---\nslug: custom\n---\n"),
            ],
        );
        let root = temp_dir.path();
        let source = root.join("content/posts/post.md");
        assert!(root.join("dist/posts/custom.html").exists());

        fs::remove_file(&source).unwrap();
        let messages = session
            .process_changes(&[SourceChange {
                path: source,
                change_type: ChangeType::Deleted,
                is_dir: false,
            }])
            .unwrap();

        assert_eq!(messages, vec![ReloadMessage::full()]);
        assert!(!root.join("dist/posts/custom.html").exists());
    }

    #[test]
    fn test_process_changes_rejects_slug_collision() {
        let (temp_dir, mut session) = setup_collection_session(
            "",
            &[
                DETAILS,
                ("content/posts/a.md", "---\ntitle: A\n---\n"),
                ("content/posts/b.md", "---\ntitle: B\n---\n"),
            ],
        );
        let root = temp_dir.path();
        let source = root.join("content/posts/b.md");
        fs::write(&source, "---\ntitle: B\nslug: a\n---\n").unwrap();

        let result = session.process_changes(&[SourceChange {
            path: source,
            change_type: ChangeType::Modified,
            is_dir: false,
        }]);

        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(root.join("dist/posts/a.html")).unwrap(),
            "<h1>A</h1>"
        );
        assert!(root.join("dist/posts/b.html").exists());
    }

    #[test]
    fn test_process_changes_handles_rename_keeping_slug() {
        let (temp_dir, mut session) = setup_collection_session(
            "",
            &[
                DETAILS,
                ("content/posts/b.md", "---\ntitle: v1\nslug: hello\n---\n"),
            ],
        );
        let root = temp_dir.path();
        let old_source = root.join("content/posts/b.md");
        let new_source = root.join("content/posts/a.md");

        fs::rename(&old_source, &new_source).unwrap();
        // `a.md` sorts before `b.md`, so the creation is classified first
        let messages = session
            .process_changes(&[
                SourceChange {
                    path: new_source.clone(),
                    change_type: ChangeType::Created,
                    is_dir: false,
                },
                SourceChange {
                    path: old_source,
                    change_type: ChangeType::Deleted,
                    is_dir: false,
                },
            ])
            .unwrap();

        assert_eq!(messages, vec![ReloadMessage::full()]);
        assert!(root.join("dist/posts/hello.html").exists());

        // Later edits to the renamed file still go through
        modify(
            &mut session,
            &new_source,
            "---\ntitle: v2\nslug: hello\n---\n",
        );
        assert_eq!(
            fs::read_to_string(root.join("dist/posts/hello.html")).unwrap(),
            "<h1>v2</h1>"
        );
    }

    #[test]
    fn test_dev_renders_drafts() {
        let (temp_dir, _session) = setup_collection_session(
            "",
            &[
                DETAILS,
                (
                    "content/posts/wip.md",
                    "---\ntitle: WIP\ndraft: true\n---\n",
                ),
            ],
        );
        assert_eq!(
            fs::read_to_string(temp_dir.path().join("dist/posts/wip.html")).unwrap(),
            "<h1>WIP</h1>"
        );
    }

    #[test]
    fn test_process_changes_updates_nested_collection_item() {
        let (temp_dir, mut session) = setup_collection_session(
            "",
            &[
                DETAILS,
                ("content/posts/2024/post.md", "---\ntitle: v1\n---\n"),
            ],
        );
        let root = temp_dir.path();

        let messages = modify(
            &mut session,
            &root.join("content/posts/2024/post.md"),
            "---\ntitle: v2\n---\n",
        );

        assert_eq!(messages, vec![ReloadMessage::full()]);
        assert_eq!(
            fs::read_to_string(root.join("dist/posts/2024/post.html")).unwrap(),
            "<h1>v2</h1>"
        );
    }

    #[test]
    fn test_classify_custom_collection_template_change_as_full_reload() {
        let (_temp_dir, mut config) = setup_dev_project();
        config.collections.insert(
            "posts".to_string(),
            crate::config::CollectionConfig {
                template: Some("post.hbs".to_string()),
            },
        );
        fs::create_dir_all(config.content_directory.join("posts")).unwrap();
        let change = SourceChange {
            path: config.pages_directory.join("posts").join("post.hbs"),
            change_type: ChangeType::Modified,
            is_dir: false,
        };

        assert!(is_page_template(&config, &change.path));
        assert_eq!(classify_changes(&config, &[change]), ChangePlan::FullReload);
    }

    #[test]
    fn test_process_changes_removes_deleted_static_page_with_collection_change() {
        let (temp_dir, mut session) = setup_collection_session(
            "",
            &[
                DETAILS,
                ("pages/about.hbs", "<h1>About</h1>"),
                ("content/posts/post.md", "---\ntitle: v1\n---\n"),
            ],
        );
        let root = temp_dir.path();
        assert!(root.join("dist/about.html").exists());

        fs::remove_file(root.join("pages/about.hbs")).unwrap();
        fs::write(root.join("content/posts/post.md"), "---\ntitle: v2\n---\n").unwrap();
        let messages = session
            .process_changes(&[
                SourceChange {
                    path: root.join("content/posts/post.md"),
                    change_type: ChangeType::Modified,
                    is_dir: false,
                },
                SourceChange {
                    path: root.join("pages/about.hbs"),
                    change_type: ChangeType::Deleted,
                    is_dir: false,
                },
            ])
            .unwrap();

        assert_eq!(messages, vec![ReloadMessage::full()]);
        assert!(!root.join("dist/about.html").exists());
        assert_eq!(
            fs::read_to_string(root.join("dist/posts/post.html")).unwrap(),
            "<h1>v2</h1>"
        );
    }

    #[test]
    fn test_process_changes_moves_directory_output_when_slug_changes() {
        let (temp_dir, mut session) = setup_collection_session(
            "url_style = \"directory\"\n",
            &[DETAILS, ("content/posts/post.md", "---\ntitle: v1\n---\n")],
        );
        let root = temp_dir.path();
        assert!(root.join("dist/posts/post/index.html").exists());

        modify(
            &mut session,
            &root.join("content/posts/post.md"),
            "---\ntitle: v2\nslug: renamed\n---\n",
        );
        // The old page's now-empty folder goes too
        assert!(!root.join("dist/posts/post").exists());
        assert_eq!(
            fs::read_to_string(root.join("dist/posts/renamed/index.html")).unwrap(),
            "<h1>v2</h1>"
        );
    }
}
