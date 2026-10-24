//! Runs the `balzac` binary against projects with broken templates.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

const BALZAC: &str = env!("CARGO_BIN_EXE_balzac");

fn setup_project(partial: &str) -> TempDir {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    fs::create_dir_all(root.join("pages")).unwrap();
    fs::create_dir_all(root.join("partials")).unwrap();
    fs::write(
        root.join("balzac.toml"),
        "output_directory = \"./dist\"\npages_directory = \"./pages\"\npartials_directory = \"./partials\"\n",
    )
    .unwrap();
    fs::write(root.join("partials/title.hbs"), partial).unwrap();
    fs::write(
        root.join("pages/index.hbs"),
        "<html><body>{{> title}}</body></html>",
    )
    .unwrap();
    temp_dir
}

fn build(root: &Path) -> std::process::Output {
    Command::new(BALZAC)
        .arg("build")
        .current_dir(root)
        .output()
        .unwrap()
}

#[test]
fn test_build_reports_template_error_without_panicking() {
    let project = setup_project("<h1>\n  {{#each items}}\n{{/if}}</h1>");

    let output = build(project.path());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let logs = format!("{stdout}{stderr}");

    assert_eq!(output.status.code(), Some(1), "{logs}");
    assert!(!logs.contains("panicked"), "{logs}");
    assert!(
        logs.contains("template error in partials/title.hbs:3:"),
        "{logs}"
    );
}

#[test]
fn test_build_reports_invalid_config() {
    let project = setup_project("<h1>Title</h1>");
    fs::write(project.path().join("balzac.toml"), "output_directory = [").unwrap();

    let output = build(project.path());
    let logs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(output.status.code(), Some(1), "{logs}");
    assert!(!logs.contains("panicked"), "{logs}");
    assert!(logs.contains("invalid config balzac.toml"), "{logs}");
}

#[test]
fn test_build_reports_invalid_frontmatter() {
    let project = setup_project("<h1>Title</h1>");
    let root = project.path();
    fs::create_dir_all(root.join("pages/posts")).unwrap();
    fs::create_dir_all(root.join("content/posts")).unwrap();
    fs::write(root.join("pages/posts/details.hbs"), "{{{content}}}").unwrap();
    fs::write(
        root.join("content/posts/hello.md"),
        "---\ntitle: [oops\n---\nHi",
    )
    .unwrap();

    let output = build(root);
    let logs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(output.status.code(), Some(1), "{logs}");
    assert!(!logs.contains("panicked"), "{logs}");
    assert!(
        logs.contains("invalid frontmatter in content/posts/hello.md"),
        "{logs}"
    );
}

#[test]
fn test_build_reports_template_line_below_page_frontmatter() {
    let project = setup_project("<h1>Title</h1>");
    let root = project.path();
    fs::write(
        root.join("pages/index.hbs"),
        "---\ntitle: Home\n---\n<h1>{{fm.title}}</h1>\n{{#each items}}\n{{/if}}\n",
    )
    .unwrap();

    let output = build(root);
    let logs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(output.status.code(), Some(1), "{logs}");
    // the `{{/if}}` is on line 6 of the file, line 3 of the template body
    assert!(
        logs.contains("template error in pages/index.hbs:6:"),
        "{logs}"
    );
}

/// A project with one `posts` item rendered through `details.hbs`.
fn setup_collection_project(details: &str) -> TempDir {
    let project = setup_project("<h1>Title</h1>");
    let root = project.path();
    fs::create_dir_all(root.join("pages/posts")).unwrap();
    fs::create_dir_all(root.join("content/posts")).unwrap();
    fs::write(root.join("pages/posts/details.hbs"), details).unwrap();
    fs::write(
        root.join("content/posts/hello.md"),
        "---\ntitle: Hello\n---\n[a link](/x)\n",
    )
    .unwrap();
    project
}

fn build_logs(root: &Path) -> (Option<i32>, String) {
    let output = build(root);
    let logs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code(), logs)
}

#[test]
fn test_build_reports_collection_item_of_failing_details_template() {
    let project = setup_collection_project("<h1>{{fm.title}}</h1>\n{{term_url}}");

    let (code, logs) = build_logs(project.path());

    assert_eq!(code, Some(1), "{logs}");
    assert!(
        logs.contains(
            "template error in pages/posts/details.hbs:2:1 (rendering content/posts/hello.md)"
        ),
        "{logs}"
    );
}

#[test]
fn test_build_reports_collection_item_of_failing_partial_in_tag_template() {
    let project = setup_collection_project("{{{content}}}");
    let root = project.path();
    fs::create_dir_all(root.join("tags")).unwrap();
    fs::write(root.join("tags/a.hbs"), "<a>{{> link}}</a>").unwrap();
    fs::write(root.join("partials/link.hbs"), "\n{{term_url}}").unwrap();

    let (code, logs) = build_logs(root);

    assert_eq!(code, Some(1), "{logs}");
    assert!(
        logs.contains("template error in partials/link.hbs:2:1 (rendering content/posts/hello.md)"),
        "{logs}"
    );
}

/// Kills the dev server when the test ends, even on failure.
struct DevServer(Child);

impl Drop for DevServer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn get(port: u16, path: &str) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    write!(stream, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    Some(response)
}

/// Polls `path` until its response satisfies `condition`.
fn wait_for(server: &mut DevServer, port: u16, path: &str, condition: impl Fn(&str) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut last = None;
    while Instant::now() < deadline {
        assert!(server.0.try_wait().unwrap().is_none(), "dev server exited");
        last = get(port, path);
        if last.as_deref().is_some_and(&condition) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {path}, last response: {last:?}");
}

#[test]
fn test_dev_keeps_serving_after_broken_template() {
    let project = setup_project("<h1>v1</h1>");
    let root = project.path();
    let port = free_port();
    let mut server = DevServer(
        Command::new(BALZAC)
            .args(["dev", "--port", &port.to_string()])
            .current_dir(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );

    wait_for(&mut server, port, "/", |body| body.contains("<h1>v1</h1>"));

    fs::write(root.join("partials/title.hbs"), "<h1>\n{{#if broken}}</h1>").unwrap();
    wait_for(&mut server, port, "/", |body| {
        body.contains("Balzac build failed") && body.contains("partials/title.hbs:")
    });

    fs::write(root.join("partials/title.hbs"), "<h1>fixed</h1>").unwrap();
    wait_for(&mut server, port, "/", |body| {
        body.contains("<h1>fixed</h1>") && body.contains("const buildError=null;")
    });
}

/// A project with `[assets] fingerprint = true` and `manifest = true`, Vite
/// enabled, and a page linking to a stylesheet that references an image.
fn setup_fingerprinted_project() -> TempDir {
    let project = setup_project("<h1>Title</h1>");
    let root = project.path();
    fs::write(
        root.join("balzac.toml"),
        "output_directory = \"./dist\"\n\
         [assets]\nfingerprint = true\nmanifest = true\n\
         [bundler.vite]\nenabled = true\nmanifest_path = \"vite-manifest.json\"\n",
    )
    .unwrap();
    fs::write(
        root.join("vite-manifest.json"),
        r#"{"main.js": {"file": "assets/main-123.js"}}"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("assets/css")).unwrap();
    fs::create_dir_all(root.join("assets/img")).unwrap();
    fs::write(root.join("assets/img/logo.png"), "hello").unwrap();
    fs::write(
        root.join("assets/css/site.css"),
        "h1{background:url(\"../img/logo.png\")}",
    )
    .unwrap();
    fs::write(
        root.join("pages/index.hbs"),
        "<link href=\"{{asset \"css/site.css\"}}\"><script src=\"{{vite_url \"main.js\"}}\"></script>",
    )
    .unwrap();
    project
}

#[test]
fn test_build_fingerprints_assets() {
    let project = setup_fingerprinted_project();
    let root = project.path();

    let (code, logs) = build_logs(root);
    assert_eq!(code, Some(0), "{logs}");

    // sha256("hello") starts with 2cf24dba
    let css = "h1{background:url(\"../img/logo.2cf24dba.png\")}";
    let css_name = fs::read_dir(root.join("dist/assets/css"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        css_name.len(),
        1,
        "only the fingerprinted copy: {css_name:?}"
    );
    let css_name = &css_name[0];
    let hash = css_name
        .strip_prefix("site.")
        .and_then(|rest| rest.strip_suffix(".css"));
    assert!(
        hash.is_some_and(|hash| hash.len() == 8 && hash.chars().all(|c| c.is_ascii_hexdigit())),
        "{css_name}"
    );
    assert_eq!(
        fs::read_to_string(root.join("dist/assets/css").join(css_name)).unwrap(),
        css
    );
    assert!(root.join("dist/assets/img/logo.2cf24dba.png").is_file());
    assert!(!root.join("dist/assets/img/logo.png").exists());

    let index = fs::read_to_string(root.join("dist/index.html")).unwrap();
    assert_eq!(
        index,
        format!(
            "<link href=\"/assets/css/{css_name}\"><script src=\"assets/main-123.js\"></script>"
        )
    );

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("dist/assets/manifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        manifest,
        serde_json::json!({
            "css/site.css": format!("css/{css_name}"),
            "img/logo.png": "img/logo.2cf24dba.png",
        })
    );
}

#[test]
fn test_dev_serves_assets_unfingerprinted() {
    let project = setup_fingerprinted_project();
    let root = project.path();
    let port = free_port();
    let mut server = DevServer(
        Command::new(BALZAC)
            .args(["dev", "--port", &port.to_string()])
            .current_dir(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );

    wait_for(&mut server, port, "/", |body| {
        body.contains("<link href=\"/assets/css/site.css\">")
            && body.contains("http://127.0.0.1:5173/main.js")
    });
    wait_for(&mut server, port, "/assets/css/site.css", |body| {
        body.ends_with("h1{background:url(\"../img/logo.png\")}")
    });
    assert!(!root.join("dist/assets/manifest.json").exists());
}
