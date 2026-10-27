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

/// A blog of `posts` items with a paginated listing and a tag taxonomy,
/// rendered through a layout, a tag template and (with the `script-helpers`
/// feature) a Rhai helper. `extra_frontmatter` adds lines to item `i`.
fn setup_blog_project(posts: usize, extra_frontmatter: impl Fn(usize) -> String) -> TempDir {
    let project = TempDir::new().unwrap();
    let root = project.path();
    for dir in [
        "pages/posts",
        "pages/tags",
        "layouts",
        "tags",
        "content/posts",
    ] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    fs::write(
        root.join("balzac.toml"),
        "base_url = \"https://example.com\"\n\n[listings.posts]\nper_page = 7\n\n[taxonomies.tags]\nper_page = 5\n",
    )
    .unwrap();
    fs::write(
        root.join("layouts/base.hbs"),
        "<html><body>{{> @partial-block}}</body></html>",
    )
    .unwrap();
    fs::write(
        root.join("tags/a.hbs"),
        "<a class=\"x\" href=\"{{props.href}}\">{{{content}}}</a>",
    )
    .unwrap();
    let shout = if cfg!(feature = "script-helpers") {
        fs::create_dir_all(root.join("helpers")).unwrap();
        fs::write(root.join("helpers/shout.rhai"), "params[0].to_upper()").unwrap();
        "{{shout fm.title}}"
    } else {
        "{{fm.title}}"
    };
    fs::write(
        root.join("pages/posts/details.hbs"),
        format!(
            "{{{{#> base}}}}<h1>{shout}</h1>{{{{#if fm.broken}}}}{{{{term_url}}}}{{{{/if}}}}{{{{{{content}}}}}}<p>{{{{len collections.posts}}}}</p>{{{{/base}}}}"
        ),
    )
    .unwrap();
    fs::write(
        root.join("pages/posts/list.hbs"),
        "{{#> base}}{{#each pagination.items}}<a href=\"{{url}}\">{{fm.title}}</a>{{/each}}{{/base}}",
    )
    .unwrap();
    fs::write(
        root.join("pages/tags/term.hbs"),
        "{{#> base}}{{taxonomy.term.name}}: {{#each pagination.items}}{{url}} {{/each}}{{/base}}",
    )
    .unwrap();
    fs::write(
        root.join("pages/index.hbs"),
        "{{#> base}}{{#each collections.posts}}{{url}} {{/each}}{{/base}}",
    )
    .unwrap();
    for i in 0..posts {
        fs::write(
            root.join(format!("content/posts/post-{i:03}.md")),
            format!(
                "---\ntitle: Post {i}\ndate: \"2024-01-{:02}\"\ntags: [t{}, u{}]\n{}---\n## Heading {i}\n\n[link](/posts/post-{i:03})\n\n```rust\nfn main() {{}}\n```\n",
                i % 28 + 1,
                i % 9,
                i % 4,
                extra_frontmatter(i)
            ),
        )
        .unwrap();
    }
    project
}

/// Every file under `dir`, by path relative to it.
fn read_tree(dir: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut files = std::collections::BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let contents = fs::read(&path).unwrap();
                files.insert(path.strip_prefix(dir).unwrap().to_path_buf(), contents);
            }
        }
    }
    files
}

fn build_with_threads(root: &Path, threads: usize) -> (Option<i32>, String) {
    let output = Command::new(BALZAC)
        .arg("build")
        .env("RAYON_NUM_THREADS", threads.to_string())
        .current_dir(root)
        .output()
        .unwrap();
    let logs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code(), logs)
}

#[test]
fn test_parallel_build_matches_single_threaded_build() {
    let project = setup_blog_project(120, |_| String::new());
    let root = project.path();

    let (code, logs) = build_with_threads(root, 1);
    assert_eq!(code, Some(0), "{logs}");
    let sequential = read_tree(&root.join("dist"));

    let (code, logs) = build_with_threads(root, 8);
    assert_eq!(code, Some(0), "{logs}");
    let parallel = read_tree(&root.join("dist"));

    // 120 items, 18 listing pages, 9 + 4 tags of 14 and 30 items
    assert!(sequential.len() > 120 + 18 + 13, "{:?}", sequential.keys());
    assert_eq!(
        sequential.keys().collect::<Vec<_>>(),
        parallel.keys().collect::<Vec<_>>()
    );
    for (path, contents) in &sequential {
        assert!(&parallel[path] == contents, "{} differs", path.display());
    }
    let item = String::from_utf8_lossy(&parallel[Path::new("posts/post-007.html")]).into_owned();
    let title = if cfg!(feature = "script-helpers") {
        "POST 7"
    } else {
        "Post 7"
    };
    assert!(item.contains(&format!("<h1>{title}</h1>")), "{item}");
    assert!(item.contains("<p>120</p>"), "{item}");
}

#[test]
fn test_parallel_build_reports_first_failing_page() {
    let project = setup_blog_project(200, |i| {
        if i == 57 || i == 150 {
            "broken: true\n".to_string()
        } else {
            String::new()
        }
    });

    for _ in 0..3 {
        let (code, logs) = build_with_threads(project.path(), 8);
        assert_eq!(code, Some(1), "{logs}");
        assert!(
            logs.contains("(rendering content/posts/post-057.md)"),
            "{logs}"
        );
        assert!(!logs.contains("post-150.md)"), "{logs}");
    }
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

/// Runs `balzac` with `args` in `dir`, returning its exit code and logs.
fn run(dir: &Path, args: &[&str]) -> (Option<i32>, String) {
    let output = Command::new(BALZAC)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let logs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code(), logs)
}

#[test]
fn test_init_starter_builds_working_site() {
    let project = TempDir::new().unwrap();
    let root = project.path();

    let (code, logs) = run(root, &["init", "--feed"]);
    assert_eq!(code, Some(0), "{logs}");
    let (code, logs) = build_logs(root);
    assert_eq!(code, Some(0), "{logs}");

    let index = fs::read_to_string(root.join("dist/index.html")).unwrap();
    assert!(index.contains("<a href=\"/posts/hello-world\">Hello, world</a>"));
    assert!(index.contains("href=\"/assets/style.css\""));
    let post = fs::read_to_string(root.join("dist/posts/hello-world.html")).unwrap();
    assert!(post.contains("<h1>Hello, world</h1>"), "{post}");
    assert!(post.contains("<title>Hello, world · My Balzac site</title>"));
    assert!(root.join("dist/assets/style.css").exists());
    assert!(root.join("dist/posts/atom.xml").exists());
}

#[test]
fn test_init_keeps_existing_files() {
    let project = TempDir::new().unwrap();
    let root = project.path();
    fs::create_dir_all(root.join("pages")).unwrap();
    fs::write(root.join("pages/index.hbs"), "mine").unwrap();

    let (code, logs) = run(root, &["init"]);

    assert_eq!(code, Some(0), "{logs}");
    assert!(logs.contains("Skipped pages/index.hbs"), "{logs}");
    assert_eq!(
        fs::read_to_string(root.join("pages/index.hbs")).unwrap(),
        "mine"
    );
    assert!(root.join("layouts/base.hbs").exists());
}

#[test]
fn test_clean_removes_output_directory() {
    let project = setup_project("<h1>Title</h1>");
    let root = project.path();
    let (code, logs) = build_logs(root);
    assert_eq!(code, Some(0), "{logs}");
    assert!(root.join("dist/index.html").exists());

    let elsewhere = TempDir::new().unwrap();
    let root_arg = root.to_str().unwrap();
    let (code, logs) = run(elsewhere.path(), &["clean", "--root", root_arg]);

    assert_eq!(code, Some(0), "{logs}");
    assert!(!root.join("dist").exists());
    assert!(root.join("pages/index.hbs").exists());

    // Nothing left to clean is not an error.
    let (code, logs) = run(root, &["clean"]);
    assert_eq!(code, Some(0), "{logs}");
    assert!(logs.contains("nothing to clean"), "{logs}");
}

#[test]
fn test_clean_refuses_to_remove_project_or_sources() {
    for (config, protected) in [
        ("output_directory = \".\"\n", "project root"),
        ("output_directory = \"./pages\"\n", "pages directory"),
        (
            "output_directory = \"./site\"\ncontent_directory = \"./site/content\"\n",
            "content directory",
        ),
        ("output_directory = \"..\"\n", "project root"),
    ] {
        let project = TempDir::new().unwrap();
        let root = project.path().join("project");
        fs::create_dir_all(root.join("pages")).unwrap();
        fs::create_dir_all(root.join("site/content")).unwrap();
        fs::write(root.join("pages/index.hbs"), "hi").unwrap();
        fs::write(root.join("balzac.toml"), config).unwrap();

        let (code, logs) = run(&root, &["clean"]);

        assert_eq!(code, Some(1), "{config}: {logs}");
        assert!(
            logs.contains(&format!("contains the {protected}")),
            "{logs}"
        );
        assert!(root.join("pages/index.hbs").exists(), "{config}");
        assert!(root.join("site/content").exists(), "{config}");
    }
}

#[test]
fn test_new_creates_draft_content_file() {
    let project = setup_collection_project("<h1>{{fm.title}}</h1>");
    let root = project.path();

    let (code, logs) = run(root, &["new", "posts", "My \"Next\" Post: Part 2"]);

    assert_eq!(code, Some(0), "{logs}");
    let content = fs::read_to_string(root.join("content/posts/my-next-post-part-2.md")).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines[0], "---");
    assert_eq!(lines[1], "title: \"My \\\"Next\\\" Post: Part 2\"");
    assert!(lines[2].starts_with("date: 20"), "{content}");
    assert_eq!(lines[3], "draft: true");
    assert_eq!(lines[4], "---");

    // Drafts are skipped by `balzac build`.
    let (code, logs) = build_logs(root);
    assert_eq!(code, Some(0), "{logs}");
    assert!(root.join("dist/posts/hello.html").exists());
    assert!(!root.join("dist/posts/my-next-post-part-2.html").exists());

    let (code, logs) = run(root, &["new", "posts", "My Next Post, Part 2"]);
    assert_eq!(code, Some(1), "{logs}");
    assert!(logs.contains("already exists"), "{logs}");
}

#[test]
fn test_new_rejects_invalid_collection_and_warns_without_template() {
    let project = setup_project("<h1>Title</h1>");
    let root = project.path();

    for collection in ["../posts", "a/b", ".", ""] {
        let (code, logs) = run(root, &["new", collection, "Hello"]);
        assert_eq!(code, Some(1), "{collection}: {logs}");
        assert!(logs.contains("Invalid collection name"), "{logs}");
    }

    let (code, logs) = run(root, &["new", "notes", "Hello"]);
    assert_eq!(code, Some(0), "{logs}");
    assert!(root.join("content/notes/hello.md").exists());
    assert!(logs.contains("details.hbs does not exist"), "{logs}");
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args([
            "-c",
            "user.name=balzac",
            "-c",
            "user.email=balzac@example.com",
        ])
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

#[test]
fn test_init_from_template_repository() {
    let template = TempDir::new().unwrap();
    let template_root = template.path();
    fs::create_dir_all(template_root.join("pages/docs")).unwrap();
    fs::write(
        template_root.join("pages/index.hbs"),
        "<h1>From template</h1>",
    )
    .unwrap();
    fs::write(template_root.join("pages/docs/intro.hbs"), "<p>Intro</p>").unwrap();
    git(template_root, &["init", "--quiet"]);
    git(template_root, &["add", "."]);
    git(template_root, &["commit", "--quiet", "-m", "template"]);

    let project = TempDir::new().unwrap();
    let root = project.path();
    fs::create_dir_all(root.join("pages/docs")).unwrap();
    fs::write(root.join("pages/docs/intro.hbs"), "mine").unwrap();

    let (code, logs) = run(
        root,
        &["init", "--template", template_root.to_str().unwrap()],
    );

    assert_eq!(code, Some(0), "{logs}");
    assert!(!root.join(".git").exists());
    assert!(root.join("balzac.toml").exists(), "{logs}");
    assert!(!root.join("layouts/base.hbs").exists());
    assert_eq!(
        fs::read_to_string(root.join("pages/docs/intro.hbs")).unwrap(),
        "mine"
    );
    let (code, logs) = build_logs(root);
    assert_eq!(code, Some(0), "{logs}");
    assert_eq!(
        fs::read_to_string(root.join("dist/index.html")).unwrap(),
        "<h1>From template</h1>"
    );
}

#[test]
fn test_init_from_missing_template_fails() {
    let project = TempDir::new().unwrap();
    let missing = project.path().join("missing-template");

    let (code, logs) = run(
        project.path(),
        &["init", "--template", missing.to_str().unwrap()],
    );

    assert_eq!(code, Some(1), "{logs}");
    assert!(logs.contains("git clone of"), "{logs}");
    assert!(!project.path().join("balzac.toml").exists());
}

#[cfg(unix)]
#[test]
fn test_init_from_template_keeps_symlinks() {
    use std::os::unix::fs::symlink;

    let template = TempDir::new().unwrap();
    let template_root = template.path();
    fs::create_dir_all(template_root.join("pages")).unwrap();
    fs::create_dir_all(template_root.join("shared/docs")).unwrap();
    fs::write(template_root.join("pages/index.hbs"), "<h1>Home</h1>").unwrap();
    fs::write(template_root.join("shared/docs/intro.md"), "Intro").unwrap();
    symlink("../shared/docs", template_root.join("pages/docs")).unwrap();
    symlink("shared/docs/intro.md", template_root.join("intro.md")).unwrap();
    git(template_root, &["init", "--quiet"]);
    git(template_root, &["add", "."]);
    git(template_root, &["commit", "--quiet", "-m", "template"]);

    let project = TempDir::new().unwrap();
    let root = project.path();
    let (code, logs) = run(
        root,
        &["init", "--template", template_root.to_str().unwrap()],
    );

    assert_eq!(code, Some(0), "{logs}");
    assert_eq!(
        fs::read_link(root.join("pages/docs")).unwrap(),
        Path::new("../shared/docs")
    );
    assert_eq!(
        fs::read_link(root.join("intro.md")).unwrap(),
        Path::new("shared/docs/intro.md")
    );
    assert_eq!(fs::read_to_string(root.join("intro.md")).unwrap(), "Intro");
}
