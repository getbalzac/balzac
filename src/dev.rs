use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::thread;
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::config::{Config, ResolvedConfig};
use crate::renderer::{HandlebarsRenderer, Renderer};
use crate::sitemap::SitePages;
use crate::{
    add_assets, collection_entry_from_source, collection_output_path_from_source,
    discover_collections, discover_static_pages, is_supported_page_file, make_dist_folder,
    remove_output_for_source, render_collection_items, render_collection_page, render_pages,
    render_static_page, static_page_entry_from_source, write_sitemap,
};

const SSE_ENDPOINT: &str = "/__balzac/events";
const WATCH_INTERVAL: Duration = Duration::from_millis(300);
const WATCH_DEBOUNCE: Duration = Duration::from_millis(200);

pub fn run(root: &Path, host: &str, port: u16) -> std::io::Result<()> {
    let mut session = DevSession::new(root)?;

    let shared_config = Arc::new(RwLock::new(session.resolved_config.clone()));
    let events = EventBroker::default();
    start_http_server(
        host.to_string(),
        port,
        shared_config.clone(),
        events.clone(),
    )?;

    log::info!("Balzac dev server running at http://{}:{}", host, port);

    let mut snapshot = capture_snapshot(&session.resolved_config)?;

    loop {
        thread::sleep(WATCH_INTERVAL);
        let latest_snapshot = match capture_snapshot(&session.resolved_config) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                log::warn!("Error refreshing watched files, retrying: {}", error);
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
                log::warn!(
                    "Error refreshing watched files after debounce, retrying: {}",
                    error
                );
                continue;
            }
        };
        let debounced_changes = diff_snapshots(&snapshot, &debounced_snapshot);
        if debounced_changes.is_empty() {
            snapshot = debounced_snapshot;
            continue;
        }

        let reload_messages = match session.process_changes(&debounced_changes) {
            Ok(messages) => messages,
            Err(error) => {
                log::warn!("Error processing file change, skipping reload: {}", error);
                if let Ok(updated_snapshot) = capture_snapshot(&session.resolved_config) {
                    snapshot = updated_snapshot;
                } else {
                    log::warn!(
                        "Error refreshing watched files after failed reload, keeping previous snapshot"
                    );
                }
                continue;
            }
        };
        {
            let mut config = shared_config
                .write()
                .expect("dev config lock should not be poisoned");
            *config = session.resolved_config.clone();
        }
        for message in reload_messages {
            events.broadcast(message);
        }
        match capture_snapshot(&session.resolved_config) {
            Ok(updated_snapshot) => snapshot = updated_snapshot,
            Err(error) => {
                log::warn!("Error refreshing watched files after reload: {}", error);
            }
        }
    }
}

struct DevSession {
    root: PathBuf,
    resolved_config: ResolvedConfig,
}

impl DevSession {
    fn new(root: &Path) -> std::io::Result<Self> {
        let (parsed_config, resolved_config) = load_config(root)?;
        let session = Self {
            root: root.to_path_buf(),
            resolved_config,
        };
        session.log_ignored_hooks(&parsed_config);
        session.build_current_config()?;
        Ok(session)
    }

    fn process_changes(&mut self, changes: &[SourceChange]) -> std::io::Result<Vec<ReloadMessage>> {
        match classify_changes(&self.resolved_config, changes) {
            ChangePlan::None => Ok(Vec::new()),
            ChangePlan::FullReload => {
                self.full_rebuild()?;
                Ok(vec![ReloadMessage::full()])
            }
            ChangePlan::Targeted(targets) => {
                let mut messages = Vec::new();
                for target in targets {
                    if let Some(message) = self.handle_targeted_change(&target)? {
                        messages.push(message);
                    }
                }
                if !messages.is_empty() {
                    self.refresh_sitemap()?;
                }
                Ok(messages)
            }
        }
    }

    fn handle_targeted_change(
        &self,
        target: &TargetedChange,
    ) -> std::io::Result<Option<ReloadMessage>> {
        match target.change_type {
            ChangeType::Deleted => {
                remove_output_for_source(&self.resolved_config, &target.source_path)?;
                Ok(Some(ReloadMessage::targeted(&target.url_path)))
            }
            ChangeType::Created | ChangeType::Modified => {
                let renderer = self.create_renderer();
                match target.kind {
                    TargetKind::StaticPage => {
                        if let Some(page) = static_page_entry_from_source(
                            &self.resolved_config,
                            &target.source_path,
                        ) {
                            render_static_page(&self.resolved_config, &page, &renderer)?;
                            Ok(Some(ReloadMessage::targeted(&target.url_path)))
                        } else {
                            Ok(None)
                        }
                    }
                    TargetKind::CollectionItem => {
                        let Some(page) = collection_entry_from_source(
                            &self.resolved_config,
                            &target.source_path,
                        )?
                        else {
                            return Ok(None);
                        };
                        render_collection_page(&self.resolved_config, &page, &renderer)?;
                        Ok(Some(ReloadMessage::targeted(&target.url_path)))
                    }
                }
            }
        }
    }

    fn full_rebuild(&mut self) -> std::io::Result<()> {
        let (parsed_config, resolved_config) = load_config(&self.root)?;
        self.log_ignored_hooks(&parsed_config);
        self.resolved_config = resolved_config;
        self.build_current_config()
    }

    fn build_current_config(&self) -> std::io::Result<()> {
        make_dist_folder(&self.resolved_config)?;
        let renderer = self.create_renderer();
        let static_pages = discover_static_pages(&self.resolved_config)?;
        let collection_pages = discover_collections(&self.resolved_config)?;

        let mut site_pages = SitePages::new();
        site_pages.add_pages(static_pages);
        site_pages.add_pages(collection_pages);

        render_pages(&self.resolved_config, site_pages.all(), &renderer)?;
        render_collection_items(&self.resolved_config, site_pages.all(), &renderer)?;
        write_sitemap(&self.resolved_config, &site_pages)?;
        add_assets(&self.resolved_config)?;
        Ok(())
    }

    fn refresh_sitemap(&self) -> std::io::Result<()> {
        let static_pages = discover_static_pages(&self.resolved_config)?;
        let collection_pages = discover_collections(&self.resolved_config)?;
        let mut site_pages = SitePages::new();
        site_pages.add_pages(static_pages);
        site_pages.add_pages(collection_pages);
        write_sitemap(&self.resolved_config, &site_pages)
    }

    fn create_renderer(&self) -> HandlebarsRenderer<'static> {
        let mut renderer = HandlebarsRenderer::new(&self.resolved_config);
        renderer.init(&self.resolved_config);
        renderer
    }

    fn log_ignored_hooks(&self, parsed_config: &Config) {
        if parsed_config.hooks.is_some() {
            log::info!("Hooks are configured but ignored in dev mode");
        }
    }
}

fn load_config(root: &Path) -> std::io::Result<(Config, ResolvedConfig)> {
    let config_path = root.join("balzac.toml");
    let config_content = fs::read_to_string(&config_path)?;
    let parsed_config = toml::from_str::<Config>(&config_content).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Could not parse config: {}", error),
        )
    })?;
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
                    targets.insert(target.url_path.clone(), target);
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
    if is_under(&change.path, &config.partials_directory)
        || is_under(&change.path, &config.layouts_directory)
        || is_under(&change.path, &config.assets_directory)
    {
        return ChangePlan::FullReload;
    }

    if let Ok(relative_path) = change.path.strip_prefix(&config.pages_directory) {
        if relative_path.components().count() == 1 && is_supported_page_file(&change.path) {
            let Some(page) = static_page_entry_from_source(config, &change.path) else {
                return ChangePlan::None;
            };
            return ChangePlan::Targeted(vec![TargetedChange {
                kind: TargetKind::StaticPage,
                source_path: change.path.clone(),
                output_path: page.output_path,
                url_path: page.url_path,
                change_type: change.change_type.clone(),
            }]);
        }

        let mut components = relative_path.components();
        let is_details_template = components.next().is_some()
            && matches!(
                components.next().map(|component| component.as_os_str().to_string_lossy()),
                Some(name) if name == "details.hbs"
            )
            && components.next().is_none();
        if is_details_template || relative_path.components().count() > 1 {
            return ChangePlan::FullReload;
        }
    }

    if let Ok(relative_path) = change.path.strip_prefix(&config.content_directory) {
        if relative_path.components().count() == 2
            && change.path.extension().and_then(|ext| ext.to_str()) == Some("md")
        {
            if let Some((url_path, output_path)) = collection_target_paths(config, &change.path) {
                return ChangePlan::Targeted(vec![TargetedChange {
                    kind: TargetKind::CollectionItem,
                    source_path: change.path.clone(),
                    output_path,
                    url_path,
                    change_type: change.change_type.clone(),
                }]);
            }
            return ChangePlan::None;
        }

        return ChangePlan::FullReload;
    }

    ChangePlan::None
}

fn collection_target_paths(
    config: &ResolvedConfig,
    source_path: &Path,
) -> Option<(String, PathBuf)> {
    let relative_path = source_path.strip_prefix(&config.content_directory).ok()?;
    let mut components = relative_path.components();
    let collection_name = components.next()?.as_os_str().to_string_lossy().to_string();
    let file_name = components.next()?.as_os_str().to_string_lossy().to_string();
    if components.next().is_some() {
        return None;
    }

    let slug = Path::new(&file_name)
        .file_stem()?
        .to_string_lossy()
        .to_string();
    let output_path = collection_output_path_from_source(config, source_path)?;
    Some((format!("/{}/{}", collection_name, slug), output_path))
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

    fn broadcast(&self, event: ReloadMessage) {
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ReloadMessage {
    kind: ReloadKind,
    url_path: Option<String>,
}

impl ReloadMessage {
    fn full() -> Self {
        Self {
            kind: ReloadKind::Full,
            url_path: None,
        }
    }

    fn targeted(url_path: &str) -> Self {
        Self {
            kind: ReloadKind::Targeted,
            url_path: Some(url_path.to_string()),
        }
    }
}

fn start_http_server(
    host: String,
    port: u16,
    config: Arc<RwLock<ResolvedConfig>>,
    events: EventBroker,
) -> std::io::Result<()> {
    let listener = TcpListener::bind((host.as_str(), port))?;
    listener.set_nonblocking(true)?;

    thread::spawn(move || {
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    let config = config.clone();
                    let events = events.clone();
                    thread::spawn(move || {
                        if let Err(error) = handle_connection(stream, config, events) {
                            log::debug!("Error handling dev request: {}", error);
                        }
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => {
                    log::error!("Dev server accept error: {}", error);
                    break;
                }
            }
        }
    });

    Ok(())
}

fn handle_connection(
    stream: TcpStream,
    config: Arc<RwLock<ResolvedConfig>>,
    events: EventBroker,
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
        return serve_sse(stream, events.subscribe());
    }

    let config_guard = config
        .read()
        .expect("dev config lock should not be poisoned");
    let Some(file_path) = resolve_output_path(&config_guard.output_directory, &path) else {
        return write_response(
            stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"Not Found",
            false,
        );
    };

    let content_type = content_type_for_path(&file_path);
    let mut body = fs::read(&file_path)?;
    if is_html_path(&file_path) {
        let html = String::from_utf8_lossy(&body);
        body = inject_dev_scripts(&html, &config_guard).into_bytes();
    }

    write_response(
        stream,
        "200 OK",
        content_type,
        &body,
        should_include_response_body(&method),
    )
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

fn serve_sse(
    mut stream: TcpStream,
    receiver: mpsc::Receiver<ReloadMessage>,
) -> std::io::Result<()> {
    stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n",
    )?;
    stream.flush()?;

    loop {
        match receiver.recv_timeout(Duration::from_secs(10)) {
            Ok(message) => {
                let payload = serde_json::to_string(&message).map_err(std::io::Error::other)?;
                stream.write_all(format!("data: {}\n\n", payload).as_bytes())?;
                stream.flush()?;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                stream.write_all(b": keep-alive\n\n")?;
                stream.flush()?;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
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

fn should_include_response_body(method: &str) -> bool {
    method != "HEAD"
}

fn resolve_output_path(output_root: &Path, request_path: &str) -> Option<PathBuf> {
    let path = request_path.split('?').next().unwrap_or("/");
    let trimmed = path.trim_start_matches('/');

    if trimmed.is_empty() {
        let index = output_root.join("index.html");
        return index.exists().then_some(index);
    }

    let relative = sanitize_relative_path(trimmed)?;
    let candidate = output_root.join(&relative);
    if candidate.is_file() {
        return Some(candidate);
    }
    if candidate.extension().is_none() {
        let html_candidate = candidate.with_extension("html");
        if html_candidate.is_file() {
            return Some(html_candidate);
        }
        let index_candidate = candidate.join("index.html");
        if index_candidate.is_file() {
            return Some(index_candidate);
        }
    }

    None
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
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn inject_dev_scripts(html: &str, config: &ResolvedConfig) -> String {
    let mut scripts = String::new();

    if let Some(bundler) = &config.bundler
        && let Some(vite) = &bundler.vite
        && vite.enabled
    {
        scripts.push_str(&format!(
            r#"<script type="module" src="{}/@vite/client"></script>"#,
            vite.dev_origin.trim_end_matches('/')
        ));
    }

    scripts.push_str(&format!(
        r#"<script>(()=>{{const normalize=(value)=>{{if(!value||value==="/")return "/";return value.endsWith("/")?value.slice(0,-1):value;}};const source=new EventSource("{}");source.onmessage=(event)=>{{const payload=JSON.parse(event.data);if(payload.kind==="full"){{window.location.reload();return;}}if(payload.kind==="targeted"&&normalize(window.location.pathname)===normalize(payload.url_path)){{window.location.reload();}}}};}})();</script>"#,
        SSE_ENDPOINT
    ));

    if let Some(index) = html.to_ascii_lowercase().rfind("</body>") {
        let mut injected = String::with_capacity(html.len() + scripts.len());
        injected.push_str(&html[..index]);
        injected.push_str(&scripts);
        injected.push_str(&html[index..]);
        injected
    } else {
        format!("{}{}", html, scripts)
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
    use tempfile::TempDir;

    fn setup_dev_project() -> (TempDir, ResolvedConfig) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let root = temp_dir.path().to_path_buf();
        let pages = root.join("pages");
        let layouts = root.join("layouts");
        let partials = root.join("partials");
        let assets = root.join("assets");
        let content = root.join("content");
        let output = root.join("dist");

        fs::create_dir_all(&pages).unwrap();
        fs::create_dir_all(&layouts).unwrap();
        fs::create_dir_all(&partials).unwrap();
        fs::create_dir_all(&assets).unwrap();
        fs::create_dir_all(&content).unwrap();
        fs::write(root.join("balzac.toml"), "").unwrap();

        let mut config = Config::default();
        config.output_directory = output.to_string_lossy().to_string();
        config.pages_directory = pages.to_string_lossy().to_string();
        config.layouts_directory = layouts.to_string_lossy().to_string();
        config.partials_directory = partials.to_string_lossy().to_string();
        config.assets_directory = assets.to_string_lossy().to_string();
        config.content_directory = content.to_string_lossy().to_string();

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
    fn test_inject_dev_scripts_includes_balzac_client() {
        let (_temp_dir, config) = setup_dev_project();
        let injected = inject_dev_scripts("<html><body><h1>Hi</h1></body></html>", &config);
        assert!(injected.contains(SSE_ENDPOINT));
        assert!(injected.contains("EventSource"));
    }

    #[test]
    fn test_inject_dev_scripts_handles_mixed_case_body_tag() {
        let (_temp_dir, config) = setup_dev_project();
        let injected = inject_dev_scripts("<html><Body><h1>Hi</h1></BoDy></html>", &config);
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

        let injected = inject_dev_scripts("<html><body></body></html>", &config);
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

        assert_eq!(messages, vec![ReloadMessage::targeted("/posts/post-1")]);
        let rendered = fs::read_to_string(root.join("dist/posts/post-1.html")).unwrap();
        assert!(rendered.contains("First Updated"));
        assert!(rendered.contains("<p>Two</p>"));
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
}
