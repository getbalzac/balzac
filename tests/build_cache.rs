//! Runs `balzac build` repeatedly to check what the build cache re-renders,
//! keeps and removes.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use tempfile::TempDir;

const BALZAC: &str = env!("CARGO_BIN_EXE_balzac");

const CONFIG: &str = r#"output_directory = "./dist"
base_url = "https://example.com"

[listings.posts]
per_page = 10

[taxonomies.tags]
collections = ["posts"]

[feed]
format = "atom"
"#;

/// A site with every kind of page: static pages (one nested), collection
/// items, a listing, taxonomy pages, a layout, a partial, a tag template, a
/// helper and assets.
fn setup_site() -> TempDir {
    let project = TempDir::new().unwrap();
    let files = [
        ("balzac.toml", CONFIG),
        (
            "layouts/base.hbs",
            "<html><body>{{> @partial-block}}</body></html>",
        ),
        ("partials/footer.hbs", "<footer>footer</footer>"),
        ("tags/h2.hbs", "<h2 class=\"post\">{{{children}}}</h2>"),
        ("helpers/shout.rhai", "params[0].to_upper()"),
        (
            "pages/index.hbs",
            "{{#> base}}<h1>{{shout \"home\"}}</h1>{{#each collections.posts}}<a>{{fm.title}}</a>{{/each}}{{> footer}}{{/base}}",
        ),
        ("pages/about.hbs", "{{#> base}}<h1>About</h1>{{/base}}"),
        ("pages/docs/intro.hbs", "<h1>Intro</h1>"),
        (
            "pages/posts/details.hbs",
            "<article>{{fm.title}}: {{{content}}}</article>",
        ),
        (
            "pages/posts/list.hbs",
            "{{#each pagination.items}}<li>{{fm.title}}</li>{{/each}}",
        ),
        ("pages/tags/term.hbs", "<h1>{{taxonomy.term.name}}</h1>"),
        (
            "content/posts/first.md",
            "---\ntitle: First\ndate: 2025-01-01\ntags: [rust]\n---\n## Hello\n",
        ),
        (
            "content/posts/second.md",
            "---\ntitle: Second\ndate: 2025-01-02\ntags: [web]\n---\nBody\n",
        ),
        ("assets/site.css", "body {}"),
        ("assets/img/logo.svg", "<svg></svg>"),
    ];
    for (path, content) in files {
        write(project.path(), path, content);
    }
    project
}

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

struct Build {
    logs: String,
}

impl Build {
    /// `(rendered, unchanged)` page counts of the summary line, "Rendered N
    /// pages, M unchanged (took ...)".
    fn pages(&self) -> (usize, usize) {
        let summary = self
            .logs
            .lines()
            .find_map(|line| Some(&line[line.find("Rendered ")?..]))
            .filter(|summary| summary.contains(" unchanged"))
            .unwrap_or_else(|| panic!("no render summary in:\n{}", self.logs));
        let counts: Vec<usize> = summary
            .split(' ')
            .filter_map(|word| word.parse().ok())
            .collect();
        (counts[0], counts[1])
    }

    fn full(&self) -> bool {
        self.logs.contains("rebuilding everything")
    }

    fn rendered_page(&self, page: &str) -> bool {
        self.logs.contains(&format!("Rendering page {page}"))
    }
}

fn build(root: &Path, args: &[&str]) -> Build {
    let output = Command::new(BALZAC)
        .arg("build")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    let logs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "build failed:\n{logs}");
    Build { logs }
}

fn read(root: &Path, path: &str) -> String {
    fs::read_to_string(root.join(path)).unwrap()
}

/// Every file of the output directory with its modification time.
fn output_files(root: &Path) -> BTreeMap<PathBuf, SystemTime> {
    fn walk(dir: &Path, root: &Path, files: &mut BTreeMap<PathBuf, SystemTime>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, files);
            } else {
                let modified = fs::metadata(&path).unwrap().modified().unwrap();
                files.insert(path.strip_prefix(root).unwrap().to_path_buf(), modified);
            }
        }
    }
    let mut files = BTreeMap::new();
    let dist = root.join("dist");
    walk(&dist, &dist, &mut files);
    files
}

/// Total pages of `setup_site`: 3 static, 2 items, 1 listing, 2 terms.
const ALL_PAGES: usize = 8;

#[test]
fn test_first_build_is_full_and_second_renders_nothing() {
    let project = setup_site();
    let root = project.path();

    let first = build(root, &[]);
    assert!(first.full(), "{}", first.logs);
    assert_eq!(first.pages(), (ALL_PAGES, 0), "{}", first.logs);
    let before = output_files(root);

    let second = build(root, &[]);
    assert!(!second.full(), "{}", second.logs);
    assert_eq!(second.pages(), (0, ALL_PAGES), "{}", second.logs);
    assert!(
        second.logs.contains("Feeds are unchanged"),
        "{}",
        second.logs
    );
    // nothing rewritten: not pages, assets, feeds, sitemap or robots.txt
    assert_eq!(output_files(root), before);
}

#[test]
fn test_config_maps_with_several_keys_keep_the_cache() {
    let project = setup_site();
    let root = project.path();
    let config = CONFIG.replace(
        "[taxonomies.tags]",
        "[taxonomies.categories]\n\n[taxonomies.tags]",
    );
    write(
        root,
        "balzac.toml",
        &format!(
            "{config}\n[global]\na = 1\nb = 2\nc = 3\nd = 4\n\n[global.nested]\nx = 1\ny = 2\n"
        ),
    );
    build(root, &[]);

    // maps are hashed in a stable order, not the per-process `HashMap` order
    for _ in 0..3 {
        let rebuild = build(root, &[]);
        assert!(!rebuild.full(), "{}", rebuild.logs);
        assert_eq!(rebuild.pages(), (0, ALL_PAGES), "{}", rebuild.logs);
    }
}

#[test]
fn test_several_collections_keep_the_cache() {
    let project = setup_site();
    let root = project.path();
    for collection in ["notes", "articles", "zines"] {
        write(
            root,
            &format!("pages/{collection}/details.hbs"),
            "<p>{{fm.title}}</p>",
        );
        write(
            root,
            &format!("content/{collection}/one.md"),
            "---\ntitle: One\n---\n",
        );
    }
    build(root, &[]);

    for _ in 0..3 {
        let rebuild = build(root, &[]);
        assert!(!rebuild.full(), "{}", rebuild.logs);
        assert_eq!(rebuild.pages(), (0, ALL_PAGES + 3), "{}", rebuild.logs);
    }
}

#[test]
fn test_static_page_edit_renders_only_that_page() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);

    write(
        root,
        "pages/about.hbs",
        "{{#> base}}<h1>About us</h1>{{/base}}",
    );
    let rebuild = build(root, &[]);

    assert_eq!(rebuild.pages(), (1, ALL_PAGES - 1), "{}", rebuild.logs);
    assert!(rebuild.rendered_page("about.hbs"), "{}", rebuild.logs);
    assert!(read(root, "dist/about.html").contains("About us"));
}

#[test]
fn test_new_static_page_is_rendered() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);

    write(root, "pages/docs/new.hbs", "<h1>New</h1>");
    let rebuild = build(root, &[]);

    assert_eq!(rebuild.pages(), (1, ALL_PAGES), "{}", rebuild.logs);
    assert_eq!(read(root, "dist/docs/new.html"), "<h1>New</h1>");
    assert!(read(root, "dist/sitemap.xml").contains("/docs/new"));
}

#[test]
fn test_deleted_static_page_output_and_empty_folder_are_removed() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);
    assert!(root.join("dist/docs/intro.html").exists());

    fs::remove_file(root.join("pages/docs/intro.hbs")).unwrap();
    let rebuild = build(root, &[]);

    assert_eq!(rebuild.pages(), (0, ALL_PAGES - 1), "{}", rebuild.logs);
    assert!(!root.join("dist/docs/intro.html").exists());
    assert!(!root.join("dist/docs").exists());
    assert!(!read(root, "dist/sitemap.xml").contains("/docs/intro"));
}

#[test]
fn test_collection_item_edit_renders_every_page() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);

    write(
        root,
        "content/posts/second.md",
        "---\ntitle: Second, edited\ndate: 2025-01-02\ntags: [web]\n---\nBody\n",
    );
    let rebuild = build(root, &[]);

    // every template sees the items through `collections`
    assert!(!rebuild.full(), "{}", rebuild.logs);
    assert_eq!(rebuild.pages(), (ALL_PAGES, 0), "{}", rebuild.logs);
    assert!(read(root, "dist/index.html").contains("Second, edited"));
    assert!(read(root, "dist/posts/second.html").contains("Second, edited"));
    assert!(read(root, "dist/posts/index.html").contains("Second, edited"));
    assert!(read(root, "dist/posts/atom.xml").contains("Second, edited"));
}

#[test]
fn test_changed_slug_removes_old_output() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);

    write(
        root,
        "content/posts/second.md",
        "---\ntitle: Second\nslug: renamed\ndate: 2025-01-02\ntags: [web]\n---\nBody\n",
    );
    build(root, &[]);

    assert!(!root.join("dist/posts/second.html").exists());
    assert!(read(root, "dist/posts/renamed.html").contains("Second"));
    assert!(read(root, "dist/posts/index.html").contains("Second"));
}

#[test]
fn test_deleted_item_removes_its_page_and_empty_term_page() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);
    assert!(root.join("dist/tags/web/index.html").exists());

    fs::remove_file(root.join("content/posts/second.md")).unwrap();
    let rebuild = build(root, &[]);

    assert_eq!(rebuild.pages(), (ALL_PAGES - 2, 0), "{}", rebuild.logs);
    assert!(!root.join("dist/posts/second.html").exists());
    assert!(!root.join("dist/tags/web").exists());
    assert!(!read(root, "dist/posts/index.html").contains("Second"));
    assert!(!read(root, "dist/sitemap.xml").contains("/posts/second"));
}

#[test]
fn test_feed_without_dated_entries_left_is_removed() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);
    assert!(root.join("dist/posts/atom.xml").exists());

    for post in ["first", "second"] {
        write(
            root,
            &format!("content/posts/{post}.md"),
            "---\ntitle: Undated\n---\nBody\n",
        );
    }
    build(root, &[]);

    assert!(!root.join("dist/posts/atom.xml").exists());
}

#[test]
fn test_shared_file_changes_rebuild_everything() {
    let changes: [(&str, &str); 7] = [
        ("partials/footer.hbs", "<footer>new footer</footer>"),
        (
            "layouts/base.hbs",
            "<html><main>{{> @partial-block}}</main></html>",
        ),
        ("tags/h2.hbs", "<h2 class=\"changed\">{{{children}}}</h2>"),
        ("helpers/shout.rhai", "params[0].to_lower()"),
        ("helpers/new.rhai", "\"new\""),
        (
            "balzac.toml",
            &format!("{CONFIG}\n[global]\nsite = \"x\"\n"),
        ),
        ("pages/robots.txt", "User-agent: *\n"),
    ];
    for (path, content) in changes {
        let project = setup_site();
        let root = project.path();
        build(root, &[]);
        // a stray file, as a hook could have left, is cleared by a full build
        write(root, "dist/stray.txt", "stray");

        write(root, path, content);
        let rebuild = build(root, &[]);

        assert!(rebuild.full(), "{path}:\n{}", rebuild.logs);
        assert_eq!(rebuild.pages(), (ALL_PAGES, 0), "{path}:\n{}", rebuild.logs);
        assert!(!root.join("dist/stray.txt").exists(), "{path}");
        assert!(root.join("dist/assets/site.css").exists(), "{path}");
    }
}

#[test]
fn test_shared_file_change_is_rendered() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);

    write(root, "partials/footer.hbs", "<footer>new footer</footer>");
    write(
        root,
        "tags/h2.hbs",
        "<h2 class=\"changed\">{{{children}}}</h2>",
    );
    build(root, &[]);

    assert!(read(root, "dist/index.html").contains("new footer"));
    assert!(read(root, "dist/posts/first.html").contains("class=\"changed\""));
}

#[test]
fn test_collection_template_edit_renders_only_its_items() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);

    write(
        root,
        "pages/posts/details.hbs",
        "<article class=\"v2\">{{fm.title}}</article>",
    );
    let rebuild = build(root, &[]);

    assert_eq!(rebuild.pages(), (2, ALL_PAGES - 2), "{}", rebuild.logs);
    assert!(read(root, "dist/posts/first.html").contains("v2"));
}

#[test]
fn test_listing_template_edit_renders_only_listings() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);

    write(
        root,
        "pages/tags/term.hbs",
        "<h1>Tag {{taxonomy.term.name}}</h1>",
    );
    let rebuild = build(root, &[]);

    assert_eq!(rebuild.pages(), (2, ALL_PAGES - 2), "{}", rebuild.logs);
    assert!(read(root, "dist/tags/rust/index.html").contains("Tag rust"));
}

#[test]
fn test_edited_or_deleted_output_is_rendered_again() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);
    let about = read(root, "dist/about.html");

    write(root, "dist/about.html", "tampered");
    fs::remove_file(root.join("dist/posts/first.html")).unwrap();
    let rebuild = build(root, &[]);

    assert_eq!(rebuild.pages(), (2, ALL_PAGES - 2), "{}", rebuild.logs);
    assert_eq!(read(root, "dist/about.html"), about);
    assert!(root.join("dist/posts/first.html").exists());
}

#[test]
fn test_assets_are_synced() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);

    write(root, "assets/site.css", "body { color: red }");
    write(root, "assets/js/app.js", "app()");
    fs::remove_file(root.join("assets/img/logo.svg")).unwrap();
    write(root, "assets/fonts/a.woff", "font");
    build(root, &[]);
    assert_eq!(read(root, "dist/assets/site.css"), "body { color: red }");
    assert_eq!(read(root, "dist/assets/js/app.js"), "app()");
    assert!(!root.join("dist/assets/img").exists());

    // an unchanged asset is not copied again
    let font_modified = fs::metadata(root.join("dist/assets/fonts/a.woff"))
        .unwrap()
        .modified()
        .unwrap();
    write(root, "assets/img/logo.svg", "<svg></svg>");
    build(root, &[]);
    let font_after = fs::metadata(root.join("dist/assets/fonts/a.woff"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(font_after, font_modified);
    assert!(root.join("dist/assets/img/logo.svg").exists());

    // a tampered copy is replaced
    write(root, "dist/assets/site.css", "tampered");
    build(root, &[]);
    assert_eq!(read(root, "dist/assets/site.css"), "body { color: red }");
}

#[test]
fn test_asset_file_replaced_by_folder() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);

    fs::remove_file(root.join("assets/site.css")).unwrap();
    write(root, "assets/site.css/main.css", "main {}");
    build(root, &[]);

    assert_eq!(read(root, "dist/assets/site.css/main.css"), "main {}");
}

#[test]
fn test_vite_manifest_change_rebuilds_everything() {
    let project = setup_site();
    let root = project.path();
    write(
        root,
        "balzac.toml",
        &format!("{CONFIG}\n[bundler.vite]\nenabled = true\nmanifest_path = \"manifest.json\"\n"),
    );
    write(
        root,
        "pages/about.hbs",
        "<script src=\"{{vite_url \"main.js\"}}\"></script>",
    );
    write(
        root,
        "manifest.json",
        r#"{"main.js": {"file": "main-1.js"}}"#,
    );
    build(root, &[]);

    write(
        root,
        "manifest.json",
        r#"{"main.js": {"file": "main-2.js"}}"#,
    );
    let rebuild = build(root, &[]);

    assert!(rebuild.full(), "{}", rebuild.logs);
    assert!(read(root, "dist/about.html").contains("main-2.js"));
}

#[test]
fn test_unreadable_cache_is_ignored() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);

    write(root, ".balzac/cache/build.json", "{not json");
    let rebuild = build(root, &[]);

    assert!(rebuild.full(), "{}", rebuild.logs);
    assert_eq!(rebuild.pages(), (ALL_PAGES, 0), "{}", rebuild.logs);
}

#[test]
fn test_new_asset_colliding_with_page_fails() {
    let project = setup_site();
    let root = project.path();
    write(
        root,
        "balzac.toml",
        &format!("url_style = \"directory\"\n{CONFIG}"),
    );
    write(root, "pages/assets.hbs", "<h1>Assets</h1>");
    fs::remove_dir_all(root.join("assets")).unwrap();
    build(root, &[]);

    write(root, "assets/index.html", "asset");
    let output = Command::new(BALZAC)
        .arg("build")
        .current_dir(root)
        .output()
        .unwrap();
    let logs = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(1), "{logs}");
    assert_eq!(read(root, "dist/assets/index.html"), "<h1>Assets</h1>");
}

#[test]
fn test_force_and_no_cache_rebuild_everything() {
    for flag in ["--force", "--no-cache"] {
        let project = setup_site();
        let root = project.path();
        build(root, &[]);

        let rebuild = build(root, &[flag]);
        assert!(rebuild.full(), "{flag}:\n{}", rebuild.logs);
        assert_eq!(rebuild.pages(), (ALL_PAGES, 0), "{flag}:\n{}", rebuild.logs);

        // and the cache is usable again afterwards
        assert_eq!(build(root, &[]).pages(), (0, ALL_PAGES), "{flag}");
    }
}

#[test]
fn test_failed_build_is_followed_by_full_rebuild() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);

    write(root, "pages/about.hbs", "{{#if}}");
    let failed = Command::new(BALZAC)
        .arg("build")
        .current_dir(root)
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(1));

    write(root, "pages/about.hbs", "<h1>About</h1>");
    let rebuild = build(root, &[]);
    assert!(rebuild.full(), "{}", rebuild.logs);
    assert!(rebuild.logs.contains("No build cache"), "{}", rebuild.logs);
}

#[test]
fn test_files_not_written_by_balzac_are_kept() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);

    write(root, "dist/search/index.json", "{}");
    fs::remove_file(root.join("pages/about.hbs")).unwrap();
    build(root, &[]);

    assert_eq!(read(root, "dist/search/index.json"), "{}");
    assert!(!root.join("dist/about.html").exists());
}

#[test]
fn test_hooks_run_on_every_build() {
    let project = setup_site();
    let root = project.path();
    write(
        root,
        "balzac.toml",
        &format!("{CONFIG}\n[hooks]\nbuild_after = \"sh -c 'echo run >> hook.log'\"\n"),
    );
    build(root, &[]);
    let rebuild = build(root, &[]);

    assert_eq!(rebuild.pages(), (0, ALL_PAGES), "{}", rebuild.logs);
    assert_eq!(read(root, "hook.log"), "run\nrun\n");
}

#[test]
fn test_page_edited_by_render_before_hook_is_rendered() {
    let project = setup_site();
    let root = project.path();
    write(
        root,
        "balzac.toml",
        &format!(
            "{CONFIG}\n[hooks]\nrender_before = \"sh -c 'cat stamp > pages/docs/intro.hbs'\"\n"
        ),
    );
    write(root, "stamp", "one");
    build(root, &[]);
    assert_eq!(read(root, "dist/docs/intro.html"), "one");

    write(root, "stamp", "two");
    let rebuild = build(root, &[]);
    assert_eq!(rebuild.pages(), (1, ALL_PAGES - 1), "{}", rebuild.logs);
    assert_eq!(read(root, "dist/docs/intro.html"), "two");
}

#[test]
fn test_static_page_shadowing_item_takes_over_and_gives_back_its_output() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);

    write(root, "pages/posts/first.hbs", "<h1>Static first</h1>");
    build(root, &[]);
    assert_eq!(read(root, "dist/posts/first.html"), "<h1>Static first</h1>");

    fs::remove_file(root.join("pages/posts/first.hbs")).unwrap();
    build(root, &[]);
    assert!(read(root, "dist/posts/first.html").contains("<article>First"));
}

#[test]
fn test_cache_directory_ignores_itself() {
    let project = setup_site();
    let root = project.path();
    build(root, &[]);

    assert!(root.join(".balzac/cache/build.json").is_file());
    assert_eq!(read(root, ".balzac/.gitignore"), "*\n");
}

#[test]
fn test_init_gitignores_cache_directory() {
    let project = TempDir::new().unwrap();
    let root = project.path();
    let init = |path: &Path| {
        let status = Command::new(BALZAC)
            .args(["init", "--path"])
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());
    };

    init(&root.join("fresh"));
    assert_eq!(read(root, "fresh/.gitignore"), "/.balzac/\n");

    write(root, "existing/.gitignore", "node_modules");
    init(&root.join("existing"));
    assert_eq!(
        read(root, "existing/.gitignore"),
        "node_modules\n/.balzac/\n"
    );

    write(root, "ignored/.gitignore", ".balzac\n");
    init(&root.join("ignored"));
    assert_eq!(read(root, "ignored/.gitignore"), ".balzac\n");
}

#[test]
fn test_page_using_current_date_is_rendered_again_once_it_changes() {
    let project = setup_site();
    let root = project.path();
    write(
        root,
        "partials/footer.hbs",
        "<footer>{{date \"now\" \"%Y\"}}</footer>",
    );
    build(root, &[]);

    // same year: reused
    assert_eq!(build(root, &[]).pages(), (0, ALL_PAGES));

    // simulate the build of last year by editing the recorded reading
    let year = footer_year(&read(root, "dist/index.html"));
    let manifest = read(root, ".balzac/cache/build.json");
    assert!(
        manifest.contains(&format!("[\"%Y\",\"{year}\"]")),
        "{manifest}"
    );
    write(
        root,
        ".balzac/cache/build.json",
        &manifest.replace(&format!("[\"%Y\",\"{year}\"]"), "[\"%Y\",\"1999\"]"),
    );
    let rebuild = build(root, &[]);
    assert_eq!(rebuild.pages(), (1, ALL_PAGES - 1), "{}", rebuild.logs);
    assert!(rebuild.rendered_page("index.hbs"), "{}", rebuild.logs);
}

#[test]
fn test_page_using_current_time_is_always_rendered() {
    let project = setup_site();
    let root = project.path();
    write(root, "pages/docs/intro.hbs", "{{date \"now\" \"%s%f\"}}");
    build(root, &[]);

    let rebuild = build(root, &[]);
    assert_eq!(rebuild.pages(), (1, ALL_PAGES - 1), "{}", rebuild.logs);
    assert!(rebuild.rendered_page("docs/intro.hbs"), "{}", rebuild.logs);
}

#[test]
fn test_tag_template_using_current_time_renders_every_page() {
    let project = setup_site();
    let root = project.path();
    write(
        root,
        "tags/h2.hbs",
        "<h2 data-t=\"{{date \"now\" \"%s%f\"}}\">{{{children}}}</h2>",
    );
    build(root, &[]);

    // every page sees the items' content, rendered with the tag template
    let rebuild = build(root, &[]);
    assert_eq!(rebuild.pages(), (ALL_PAGES, 0), "{}", rebuild.logs);
}

/// The four-digit year in `html`.
fn footer_year(html: &str) -> String {
    let start = html.find("<footer>").unwrap() + "<footer>".len();
    html[start..start + 4].to_string()
}
