use balzac::renderer::{HandlebarsRenderer, Renderer};
use balzac::sitemap::SitePages;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

// Import from the main crate
use balzac::config::{Config, RobotsConfig, SitemapConfig};
use balzac::listing::render_listing_pages;
use balzac::{
    discover_collections, discover_site_pages, discover_static_pages, make_dist_folder,
    render_collection_items, render_pages, write_robots, write_sitemap,
};

/// Helper function to create a temporary project structure
fn setup_test_project() -> (
    TempDir,
    PathBuf,
    PathBuf,
    PathBuf,
    PathBuf,
    PathBuf,
    PathBuf,
    PathBuf,
) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path().to_path_buf();

    let pages_dir = temp_path.join("pages");
    let output_dir = temp_path.join("dist");
    let layouts_dir = temp_path.join("layouts");
    let partials_dir = temp_path.join("partials");
    let assets_dir = temp_path.join("assets");
    let content_dir = temp_path.join("content");

    fs::create_dir(&pages_dir).expect("Failed to create pages directory");

    (
        temp_dir,
        temp_path,
        pages_dir,
        output_dir,
        layouts_dir,
        partials_dir,
        assets_dir,
        content_dir,
    )
}

#[test]
fn test_partials_registration() {
    let (
        _temp,
        temp_path,
        pages_dir,
        output_dir,
        layouts_dir,
        partials_dir,
        assets_dir,
        content_dir,
    ) = setup_test_project();
    let partial_content = "template";

    fs::create_dir(&partials_dir).expect("Failed to create partials directory");

    fs::write(partials_dir.join("alert.handlebars"), partial_content)
        .expect("Failed to write partial");

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        tags_directory: "./tags".to_string(),
        markdown: None,
        global: None,
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
        robots: None,
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);
    let mut renderer = HandlebarsRenderer::new(&resolved_config);
    renderer.init(&resolved_config);

    assert!(
        renderer.registry.get_templates().contains_key("alert"),
        "Could not register partial alert"
    );
}

#[test]
fn test_partials_registration_without_folder() {
    let (
        _temp,
        temp_path,
        pages_dir,
        output_dir,
        layouts_dir,
        partials_dir,
        assets_dir,
        content_dir,
    ) = setup_test_project();

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        tags_directory: "./tags".to_string(),
        markdown: None,
        global: None,
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
        robots: None,
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);
    let mut renderer = HandlebarsRenderer::new(&resolved_config);
    renderer.init(&resolved_config);

    assert!(
        renderer.registry.get_templates().is_empty(),
        "Could not register partial alert"
    );
}

#[test]
fn test_full_workflow_single_page() {
    let (
        _temp,
        temp_path,
        pages_dir,
        output_dir,
        layouts_dir,
        partials_dir,
        assets_dir,
        content_dir,
    ) = setup_test_project();

    // Create a simple template
    let template_content = "<h1>{{title}}</h1><p>{{content}}</p>";
    fs::write(pages_dir.join("index.handlebars"), template_content)
        .expect("Failed to write template");

    // Create config
    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        tags_directory: "./tags".to_string(),
        markdown: None,
        global: None,
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
        robots: None,
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);
    // Run the workflow
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let pages = discover_static_pages(&resolved_config).expect("Failed to discover pages");
    render_pages(
        &resolved_config,
        &pages,
        &HandlebarsRenderer::new(&resolved_config),
    )
    .expect("Failed to render static pages");

    // Verify output
    let output_file = output_dir.join("index.html");
    assert!(
        output_file.exists(),
        "Output HTML file should exist at {}",
        output_file.display()
    );

    let output_content = fs::read_to_string(&output_file).expect("Failed to read output");
    // When no global data is provided, handlebars renders undefined variables as empty strings
    assert_eq!(output_content, "<h1></h1><p></p>");
}

#[test]
fn test_full_workflow_multiple_pages() {
    let (
        _temp,
        temp_path,
        pages_dir,
        output_dir,
        layouts_dir,
        partials_dir,
        assets_dir,
        content_dir,
    ) = setup_test_project();

    // Create multiple templates
    fs::write(pages_dir.join("index.handlebars"), "<h1>Home</h1>").expect("Failed to write index");
    fs::write(pages_dir.join("about.handlebars"), "<h1>About</h1>").expect("Failed to write about");
    fs::write(pages_dir.join("contact.handlebars"), "<h1>Contact</h1>")
        .expect("Failed to write contact");

    // Create config
    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        tags_directory: "./tags".to_string(),
        markdown: None,
        global: None,
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
        robots: None,
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);
    // Run the workflow
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let pages = discover_static_pages(&resolved_config).expect("Failed to discover pages");
    render_pages(
        &resolved_config,
        &pages,
        &HandlebarsRenderer::new(&resolved_config),
    )
    .expect("Failed to render static pages");

    // Verify all outputs exist
    assert!(output_dir.join("index.html").exists());
    assert!(output_dir.join("about.html").exists());
    assert!(output_dir.join("contact.html").exists());

    // Verify contents
    assert_eq!(
        fs::read_to_string(output_dir.join("index.html")).unwrap(),
        "<h1>Home</h1>"
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("about.html")).unwrap(),
        "<h1>About</h1>"
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("contact.html")).unwrap(),
        "<h1>Contact</h1>"
    );
}

#[test]
fn test_workflow_with_global_data() {
    let (
        _temp,
        temp_path,
        pages_dir,
        output_dir,
        layouts_dir,
        partials_dir,
        assets_dir,
        content_dir,
    ) = setup_test_project();

    // Create a template that uses global data
    let template_content = "<h1>{{global_title}}</h1><p>Author: {{global_author}}</p>";
    fs::write(pages_dir.join("index.handlebars"), template_content)
        .expect("Failed to write template");

    // Create config with global data
    let mut global = std::collections::HashMap::new();
    global.insert("global_title".to_string(), serde_json::json!("My Website"));
    global.insert("global_author".to_string(), serde_json::json!("John Doe"));

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        tags_directory: "./tags".to_string(),
        markdown: None,
        global: Some(global),
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
        robots: None,
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);
    // Run the workflow
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let pages = discover_static_pages(&resolved_config).expect("Failed to discover pages");
    render_pages(
        &resolved_config,
        &pages,
        &HandlebarsRenderer::new(&resolved_config),
    )
    .expect("Failed to render static pages");

    // Verify output with rendered global data
    let output_content =
        fs::read_to_string(output_dir.join("index.html")).expect("Failed to read output");
    assert_eq!(output_content, "<h1>My Website</h1><p>Author: John Doe</p>");
}

#[test]
fn test_make_dist_folder_creates_directory() {
    let (
        _temp,
        temp_path,
        pages_dir,
        output_dir,
        layouts_dir,
        partials_dir,
        assets_dir,
        content_dir,
    ) = setup_test_project();

    // Ensure output dir doesn't exist initially
    assert!(!output_dir.exists());

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        tags_directory: "./tags".to_string(),
        markdown: None,
        global: None,
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
        robots: None,
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    assert!(output_dir.exists());
    assert!(output_dir.is_dir());
}

#[test]
fn test_make_dist_folder_recreates_existing_directory() {
    let (
        _temp,
        temp_path,
        pages_dir,
        output_dir,
        layouts_dir,
        partials_dir,
        assets_dir,
        content_dir,
    ) = setup_test_project();

    // Create output directory with a file
    fs::create_dir(&output_dir).expect("Failed to create output dir");
    fs::write(output_dir.join("old_file.txt"), "old content").expect("Failed to write old file");

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        tags_directory: "./tags".to_string(),
        markdown: None,
        global: None,
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
        robots: None,
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    // Directory should exist but be empty
    assert!(output_dir.exists());
    assert!(!output_dir.join("old_file.txt").exists());
}

#[test]
fn test_workflow_preserves_file_extensions() {
    let (
        _temp,
        temp_path,
        pages_dir,
        output_dir,
        layouts_dir,
        partials_dir,
        assets_dir,
        content_dir,
    ) = setup_test_project();

    // Create templates with different extensions
    fs::write(pages_dir.join("page.handlebars"), "<h1>Handlebars</h1>")
        .expect("Failed to write .handlebars file");
    fs::write(pages_dir.join("home.html"), "<h1>HTML</h1>").expect("Failed to write .html file");

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        tags_directory: "./tags".to_string(),
        markdown: None,
        global: None,
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
        robots: None,
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let pages = discover_static_pages(&resolved_config).expect("Failed to discover pages");
    render_pages(
        &resolved_config,
        &pages,
        &HandlebarsRenderer::new(&resolved_config),
    )
    .expect("Failed to render static pages");

    // Both should be converted to .html output
    assert!(output_dir.join("page.html").exists());
    assert!(output_dir.join("home.html").exists());
}

#[test]
fn test_template_with_conditionals() {
    let (
        _temp,
        temp_path,
        pages_dir,
        output_dir,
        layouts_dir,
        partials_dir,
        assets_dir,
        content_dir,
    ) = setup_test_project();

    // Create a template with conditionals
    let template_content = "{{#if show_message}}<p>Welcome!</p>{{/if}}";
    fs::write(pages_dir.join("index.handlebars"), template_content)
        .expect("Failed to write template");

    let mut global = std::collections::HashMap::new();
    global.insert("show_message".to_string(), serde_json::json!("true"));

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        tags_directory: "./tags".to_string(),
        markdown: None,
        global: Some(global),
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
        robots: None,
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let pages = discover_static_pages(&resolved_config).expect("Failed to discover pages");
    render_pages(
        &resolved_config,
        &pages,
        &HandlebarsRenderer::new(&resolved_config),
    )
    .expect("Failed to render static pages");

    let output_content =
        fs::read_to_string(output_dir.join("index.html")).expect("Failed to read output");
    assert_eq!(output_content, "<p>Welcome!</p>");
}

#[test]
fn test_sitemap_generation_with_static_pages() {
    let (
        _temp,
        temp_path,
        pages_dir,
        output_dir,
        layouts_dir,
        partials_dir,
        assets_dir,
        content_dir,
    ) = setup_test_project();

    // Create multiple pages
    fs::write(pages_dir.join("index.hbs"), "<h1>Home</h1>").expect("Failed to write index");
    fs::write(pages_dir.join("about.hbs"), "<h1>About</h1>").expect("Failed to write about");
    fs::write(pages_dir.join("contact.hbs"), "<h1>Contact</h1>").expect("Failed to write contact");

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        tags_directory: "./tags".to_string(),
        markdown: None,
        global: None,
        hooks: None,
        bundler: None,
        base_url: Some("https://example.com".to_string()),
        sitemap: Some(SitemapConfig {
            enabled: true,
            filename: "sitemap.xml".to_string(),
            default_priority: Some(0.5),
            default_changefreq: None,
        }),
        robots: None,
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);

    // Create dist folder
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    // Discovery phase
    let static_pages = discover_static_pages(&resolved_config).expect("Failed to discover pages");
    let collection_pages =
        discover_collections(&resolved_config).expect("Failed to discover collections");

    // Combine into SitePages
    let mut site_pages = SitePages::new();
    site_pages.add_pages(static_pages);
    site_pages.add_pages(collection_pages);

    // Render phase
    let renderer = HandlebarsRenderer::new(&resolved_config);
    render_pages(&resolved_config, site_pages.all(), &renderer).expect("Failed to render pages");
    render_collection_items(&resolved_config, site_pages.all(), &renderer)
        .expect("Failed to render collections");

    // Sitemap generation
    write_sitemap(&resolved_config, &site_pages).expect("Failed to write sitemap");

    // Verify sitemap exists
    let sitemap_path = output_dir.join("sitemap.xml");
    assert!(sitemap_path.exists(), "Sitemap should exist");

    // Verify sitemap content
    let sitemap_content = fs::read_to_string(&sitemap_path).expect("Failed to read sitemap");
    assert!(sitemap_content.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(sitemap_content.contains("https://example.com/"));
    assert!(sitemap_content.contains("https://example.com/about"));
    assert!(sitemap_content.contains("https://example.com/contact"));
    assert!(sitemap_content.contains("<priority>0.5</priority>"));
}

#[test]
fn test_sitemap_generation_with_collections() {
    let (
        _temp,
        temp_path,
        pages_dir,
        output_dir,
        layouts_dir,
        partials_dir,
        assets_dir,
        content_dir,
    ) = setup_test_project();

    // Create a collection directory structure
    let blog_pages_dir = pages_dir.join("blog");
    let blog_content_dir = content_dir.join("blog");
    fs::create_dir(&blog_pages_dir).expect("Failed to create blog pages dir");
    fs::create_dir(&content_dir).expect("Failed to create content dir");
    fs::create_dir(&blog_content_dir).expect("Failed to create blog content dir");

    // Create details template
    fs::write(
        blog_pages_dir.join("details.hbs"),
        "<h1>{{fm.title}}</h1>{{{content}}}",
    )
    .expect("Failed to write details template");

    // Create blog posts with frontmatter
    fs::write(
        blog_content_dir.join("post-1.md"),
        "---\ntitle: First Post\nlastmod: 2024-01-15\npriority: 0.8\n---\n\nContent of first post",
    )
    .expect("Failed to write post-1");

    fs::write(
        blog_content_dir.join("post-2.md"),
        "---\ntitle: Second Post\nchangefreq: weekly\n---\n\nContent of second post",
    )
    .expect("Failed to write post-2");

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        tags_directory: "./tags".to_string(),
        markdown: None,
        global: None,
        hooks: None,
        bundler: None,
        base_url: Some("https://myblog.com".to_string()),
        sitemap: Some(SitemapConfig {
            enabled: true,
            filename: "sitemap.xml".to_string(),
            default_priority: None,
            default_changefreq: None,
        }),
        robots: None,
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);

    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    // Discovery phase
    let static_pages = discover_static_pages(&resolved_config).expect("Failed to discover pages");
    let collection_pages =
        discover_collections(&resolved_config).expect("Failed to discover collections");

    // Combine into SitePages
    let mut site_pages = SitePages::new();
    site_pages.add_pages(static_pages);
    site_pages.add_pages(collection_pages);

    // Verify collection items were discovered
    assert_eq!(
        site_pages.collection_items().len(),
        2,
        "Should have 2 collection items"
    );

    // Render phase
    let renderer = HandlebarsRenderer::new(&resolved_config);
    render_pages(&resolved_config, site_pages.all(), &renderer).expect("Failed to render pages");
    render_collection_items(&resolved_config, site_pages.all(), &renderer)
        .expect("Failed to render collections");

    // Sitemap generation
    write_sitemap(&resolved_config, &site_pages).expect("Failed to write sitemap");

    // Verify sitemap content
    let sitemap_content =
        fs::read_to_string(output_dir.join("sitemap.xml")).expect("Failed to read sitemap");

    assert!(sitemap_content.contains("https://myblog.com/blog/post-1"));
    assert!(sitemap_content.contains("https://myblog.com/blog/post-2"));
    assert!(sitemap_content.contains("<lastmod>2024-01-15</lastmod>"));
    assert!(sitemap_content.contains("<priority>0.8</priority>"));
    assert!(sitemap_content.contains("<changefreq>weekly</changefreq>"));
}

#[test]
fn test_sitemap_excludes_pages_with_sitemap_exclude() {
    let (
        _temp,
        temp_path,
        pages_dir,
        output_dir,
        layouts_dir,
        partials_dir,
        assets_dir,
        content_dir,
    ) = setup_test_project();

    // Create collection structure
    let blog_pages_dir = pages_dir.join("blog");
    let blog_content_dir = content_dir.join("blog");
    fs::create_dir(&blog_pages_dir).expect("Failed to create blog pages dir");
    fs::create_dir(&content_dir).expect("Failed to create content dir");
    fs::create_dir(&blog_content_dir).expect("Failed to create blog content dir");

    fs::write(
        blog_pages_dir.join("details.hbs"),
        "<h1>{{fm.title}}</h1>{{{content}}}",
    )
    .expect("Failed to write details template");

    // Create a public post
    fs::write(
        blog_content_dir.join("public-post.md"),
        "---\ntitle: Public Post\n---\n\nPublic content",
    )
    .expect("Failed to write public post");

    // Create a private post (excluded from sitemap)
    fs::write(
        blog_content_dir.join("private-post.md"),
        "---\ntitle: Private Post\nsitemap_exclude: true\n---\n\nPrivate content",
    )
    .expect("Failed to write private post");

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        tags_directory: "./tags".to_string(),
        markdown: None,
        global: None,
        hooks: None,
        bundler: None,
        base_url: Some("https://example.com".to_string()),
        sitemap: Some(SitemapConfig::default()),
        robots: None,
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    // Discovery and rendering
    let collection_pages =
        discover_collections(&resolved_config).expect("Failed to discover collections");
    let mut site_pages = SitePages::new();
    site_pages.add_pages(collection_pages);

    let renderer = HandlebarsRenderer::new(&resolved_config);
    render_collection_items(&resolved_config, site_pages.all(), &renderer)
        .expect("Failed to render collections");

    write_sitemap(&resolved_config, &site_pages).expect("Failed to write sitemap");

    let sitemap_content =
        fs::read_to_string(output_dir.join("sitemap.xml")).expect("Failed to read sitemap");

    // Public post should be in sitemap
    assert!(sitemap_content.contains("/blog/public-post"));
    // Private post should NOT be in sitemap
    assert!(!sitemap_content.contains("/blog/private-post"));
}

#[test]
fn test_no_sitemap_without_base_url() {
    let (
        _temp,
        temp_path,
        pages_dir,
        output_dir,
        layouts_dir,
        partials_dir,
        assets_dir,
        content_dir,
    ) = setup_test_project();

    fs::write(pages_dir.join("index.hbs"), "<h1>Home</h1>").expect("Failed to write index");

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        tags_directory: "./tags".to_string(),
        markdown: None,
        global: None,
        hooks: None,
        bundler: None,
        base_url: None, // No base_url configured
        sitemap: None,
        robots: None,
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let static_pages = discover_static_pages(&resolved_config).expect("Failed to discover pages");
    let mut site_pages = SitePages::new();
    site_pages.add_pages(static_pages);

    let renderer = HandlebarsRenderer::new(&resolved_config);
    render_pages(&resolved_config, site_pages.all(), &renderer).expect("Failed to render pages");

    // This should not fail, but also not create a sitemap
    write_sitemap(&resolved_config, &site_pages).expect("write_sitemap should not fail");

    // Sitemap should NOT exist
    assert!(
        !output_dir.join("sitemap.xml").exists(),
        "Sitemap should not be created without base_url"
    );
}

#[test]
fn test_custom_tag_renderer_for_links() {
    let (
        _temp,
        temp_path,
        pages_dir,
        output_dir,
        _layouts_dir,
        _partials_dir,
        _assets_dir,
        content_dir,
    ) = setup_test_project();

    // Create a tags directory with a custom <a> tag template
    let tags_dir = temp_path.join("tags");
    fs::create_dir(&tags_dir).expect("Failed to create tags dir");
    fs::write(
        tags_dir.join("a.hbs"),
        r#"<a href="{{props.href}}" class="custom-link">{{{content}}}</a>"#,
    )
    .expect("Failed to write tag template");

    // Create a collection directory structure
    let blog_pages_dir = pages_dir.join("blog");
    let blog_content_dir = content_dir.join("blog");
    fs::create_dir(&blog_pages_dir).expect("Failed to create blog pages dir");
    fs::create_dir(&content_dir).expect("Failed to create content dir");
    fs::create_dir(&blog_content_dir).expect("Failed to create blog content dir");

    // Create details template
    fs::write(
        blog_pages_dir.join("details.hbs"),
        "<h1>{{fm.title}}</h1>{{{content}}}",
    )
    .expect("Failed to write details template");

    // Create a blog post with a markdown link
    fs::write(
        blog_content_dir.join("test-post.md"),
        "---\ntitle: Test Post\n---\n\nCheck out [this link](https://example.com).",
    )
    .expect("Failed to write test post");

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: temp_path.join("layouts").to_string_lossy().to_string(),
        partials_directory: temp_path.join("partials").to_string_lossy().to_string(),
        assets_directory: temp_path.join("assets").to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        tags_directory: tags_dir.to_string_lossy().to_string(),
        markdown: None,
        global: None,
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
        robots: None,
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let collection_pages =
        discover_collections(&resolved_config).expect("Failed to discover collections");

    let mut renderer = HandlebarsRenderer::new(&resolved_config);
    renderer.init(&resolved_config);
    render_collection_items(&resolved_config, &collection_pages, &renderer)
        .expect("Failed to render collections");

    // Verify output contains the custom rendered link
    let output_file = output_dir.join("blog").join("test-post.html");
    assert!(
        output_file.exists(),
        "Output HTML file should exist at {}",
        output_file.display()
    );

    let output_content = fs::read_to_string(&output_file).expect("Failed to read output");
    assert!(
        output_content.contains(r#"class="custom-link""#),
        "Custom link class should be present: {output_content}"
    );
    assert!(
        output_content.contains(r#"href="https://example.com""#),
        "Link href should be preserved: {output_content}"
    );
    assert!(
        output_content.contains(">this link</a>"),
        "Link text should be preserved: {output_content}"
    );
}

#[test]
fn test_collections_available_in_static_and_details_pages() {
    let (
        _temp,
        temp_path,
        pages_dir,
        output_dir,
        layouts_dir,
        partials_dir,
        assets_dir,
        content_dir,
    ) = setup_test_project();

    let posts_pages_dir = pages_dir.join("posts");
    let posts_content_dir = content_dir.join("posts");
    fs::create_dir(&posts_pages_dir).expect("Failed to create posts pages dir");
    fs::create_dir_all(&posts_content_dir).expect("Failed to create posts content dir");

    fs::write(
        pages_dir.join("index.hbs"),
        "<h1>{{site_name}}</h1><ul>{{#each collections.posts}}<li><a href=\"{{url}}\">{{fm.title}}</a>{{{content}}}</li>{{/each}}</ul>",
    )
    .expect("Failed to write index");
    fs::write(
        posts_pages_dir.join("details.hbs"),
        "<h1>{{fm.title}}</h1>{{#each collections.posts}}[{{slug}}]{{/each}}",
    )
    .expect("Failed to write details template");

    // Unquoted YAML dates must still sort newest first
    fs::write(
        posts_content_dir.join("older.md"),
        "---\ntitle: Older\ndate: 2024-01-15\n---\n\nOld body",
    )
    .expect("Failed to write older post");
    fs::write(
        posts_content_dir.join("newer.md"),
        "---\ntitle: Newer\ndate: 2025-03-02\n---\n\nNew body",
    )
    .expect("Failed to write newer post");
    fs::write(
        posts_content_dir.join("undated.md"),
        "---\ntitle: Undated\n---\n\nNo date",
    )
    .expect("Failed to write undated post");

    let mut global = std::collections::HashMap::new();
    global.insert("site_name".to_string(), serde_json::json!("My Blog"));

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        tags_directory: "./tags".to_string(),
        markdown: None,
        global: Some(global),
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
        robots: None,
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let mut site_pages = SitePages::new();
    site_pages
        .add_pages(discover_static_pages(&resolved_config).expect("Failed to discover pages"));
    site_pages
        .add_pages(discover_collections(&resolved_config).expect("Failed to discover collections"));

    let renderer = HandlebarsRenderer::new(&resolved_config);
    render_pages(&resolved_config, site_pages.all(), &renderer).expect("Failed to render pages");
    render_collection_items(&resolved_config, site_pages.all(), &renderer)
        .expect("Failed to render collections");

    let index = fs::read_to_string(output_dir.join("index.html")).expect("Failed to read index");
    assert_eq!(
        index,
        "<h1>My Blog</h1><ul>\
         <li><a href=\"/posts/newer\">Newer</a><p>New body</p>\n</li>\
         <li><a href=\"/posts/older\">Older</a><p>Old body</p>\n</li>\
         <li><a href=\"/posts/undated\">Undated</a><p>No date</p>\n</li>\
         </ul>"
    );

    let details =
        fs::read_to_string(output_dir.join("posts/older.html")).expect("Failed to read details");
    assert_eq!(details, "<h1>Older</h1>[newer][older][undated]");
}

/// Run the build steps that produce pages, sitemap and robots.txt
fn build_site(config: &Config, root: &std::path::Path) -> PathBuf {
    let resolved_config = config.resolve(root);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let mut site_pages = SitePages::new();
    site_pages.add_pages(discover_static_pages(&resolved_config).expect("Failed to discover"));

    let renderer = HandlebarsRenderer::new(&resolved_config);
    render_pages(&resolved_config, site_pages.all(), &renderer).expect("Failed to render pages");
    write_sitemap(&resolved_config, &site_pages).expect("Failed to write sitemap");
    write_robots(&resolved_config).expect("Failed to write robots.txt");

    resolved_config.output_directory
}

#[test]
fn test_404_page_is_built_and_excluded_from_sitemap() {
    let (_temp, temp_path, pages_dir, ..) = setup_test_project();
    fs::write(pages_dir.join("index.hbs"), "<h1>Home</h1>").expect("Failed to write index");
    fs::write(pages_dir.join("404.hbs"), "<h1>Not found</h1>").expect("Failed to write 404");

    let config = Config {
        base_url: Some("https://example.com".to_string()),
        ..Config::default()
    };
    let output_dir = build_site(&config, &temp_path);

    let not_found = fs::read_to_string(output_dir.join("404.html")).expect("404.html missing");
    assert_eq!(not_found, "<h1>Not found</h1>");

    let sitemap = fs::read_to_string(output_dir.join("sitemap.xml")).expect("sitemap missing");
    assert!(sitemap.contains("<loc>https://example.com/</loc>"));
    assert!(
        !sitemap.contains("404"),
        "404 page must not be in sitemap: {sitemap}"
    );
}

#[test]
fn test_robots_default_includes_sitemap() {
    let (_temp, temp_path, pages_dir, ..) = setup_test_project();
    fs::write(pages_dir.join("index.hbs"), "<h1>Home</h1>").expect("Failed to write index");

    let config = Config {
        base_url: Some("https://example.com/".to_string()),
        ..Config::default()
    };
    let output_dir = build_site(&config, &temp_path);

    let robots = fs::read_to_string(output_dir.join("robots.txt")).expect("robots.txt missing");
    assert_eq!(
        robots,
        "User-agent: *\nAllow: /\n\nSitemap: https://example.com/sitemap.xml\n"
    );
}

#[test]
fn test_robots_without_sitemap() {
    let (_temp, temp_path, pages_dir, ..) = setup_test_project();
    fs::write(pages_dir.join("index.hbs"), "<h1>Home</h1>").expect("Failed to write index");

    // no base_url: no sitemap, so no Sitemap line
    let output_dir = build_site(&Config::default(), &temp_path);
    let robots = fs::read_to_string(output_dir.join("robots.txt")).expect("robots.txt missing");
    assert_eq!(robots, "User-agent: *\nAllow: /\n");

    // sitemap explicitly disabled
    let config = Config {
        base_url: Some("https://example.com".to_string()),
        sitemap: Some(SitemapConfig {
            enabled: false,
            ..SitemapConfig::default()
        }),
        ..Config::default()
    };
    let output_dir = build_site(&config, &temp_path);
    let robots = fs::read_to_string(output_dir.join("robots.txt")).expect("robots.txt missing");
    assert!(!robots.contains("Sitemap:"));
}

#[test]
fn test_robots_custom_rules_and_sitemap_filename() {
    let (_temp, temp_path, pages_dir, ..) = setup_test_project();
    fs::write(pages_dir.join("index.hbs"), "<h1>Home</h1>").expect("Failed to write index");

    let config: Config = toml::from_str(
        r#"
base_url = "https://example.com"

[sitemap]
filename = "map.xml"

[robots]
disallow = ["/drafts/"]
"#,
    )
    .expect("Failed to parse config");
    let output_dir = build_site(&config, &temp_path);

    let robots = fs::read_to_string(output_dir.join("robots.txt")).expect("robots.txt missing");
    assert_eq!(
        robots,
        "User-agent: *\nDisallow: /drafts/\n\nSitemap: https://example.com/map.xml\n"
    );
}

#[test]
fn test_robots_disabled() {
    let (_temp, temp_path, pages_dir, ..) = setup_test_project();
    fs::write(pages_dir.join("index.hbs"), "<h1>Home</h1>").expect("Failed to write index");

    let config = Config {
        robots: Some(RobotsConfig {
            enabled: false,
            ..RobotsConfig::default()
        }),
        ..Config::default()
    };
    let output_dir = build_site(&config, &temp_path);

    assert!(!output_dir.join("robots.txt").exists());
}

#[test]
fn test_user_robots_in_pages_wins() {
    let (_temp, temp_path, pages_dir, ..) = setup_test_project();
    let user_robots = "User-agent: *\nDisallow: /\n";
    fs::write(pages_dir.join("index.hbs"), "<h1>Home</h1>").expect("Failed to write index");
    fs::write(pages_dir.join("robots.txt"), user_robots).expect("Failed to write robots.txt");

    let config = Config {
        base_url: Some("https://example.com".to_string()),
        // generation disabled must not prevent the user's own file from being published
        robots: Some(RobotsConfig {
            enabled: false,
            ..RobotsConfig::default()
        }),
        ..Config::default()
    };
    let output_dir = build_site(&config, &temp_path);

    let robots = fs::read_to_string(output_dir.join("robots.txt")).expect("robots.txt missing");
    assert_eq!(robots, user_robots);
}

/// Builds a blog with a paginated `posts` listing and a `tags` taxonomy.
fn build_blog_with_listings() -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let root = temp_dir.path();
    let pages_dir = root.join("pages");
    let posts_content_dir = root.join("content/posts");
    fs::create_dir_all(pages_dir.join("posts")).expect("Failed to create posts pages dir");
    fs::create_dir_all(pages_dir.join("tags")).expect("Failed to create tags pages dir");
    fs::create_dir_all(&posts_content_dir).expect("Failed to create posts content dir");

    fs::write(
        root.join("balzac.toml"),
        r#"
base_url = "https://example.com"

[global]
site_name = "My Blog"

[listings.posts]
per_page = 2

[taxonomies.tags]
per_page = 1
"#,
    )
    .expect("Failed to write config");
    fs::write(
        pages_dir.join("posts/details.hbs"),
        "{{#each fm.tags}}<a href=\"{{term_url \"tags\" this}}\">{{this}}</a>{{/each}}",
    )
    .expect("Failed to write details template");
    fs::write(
        pages_dir.join("posts/list.hbs"),
        "{{site_name}}|{{collection}}|{{pagination.current_page}}/{{pagination.total_pages}}|\
         {{#each pagination.items}}[{{fm.title}}]{{/each}}|\
         prev={{pagination.prev_url}}|next={{pagination.next_url}}|\
         {{#each pagination.pages}}{{#if current}}*{{/if}}{{number}}{{/each}}|\
         {{#each collections.posts}}.{{/each}}",
    )
    .expect("Failed to write list template");
    fs::write(
        pages_dir.join("tags/term.hbs"),
        "{{taxonomy.name}}:{{taxonomy.term.name}}({{taxonomy.term.count}})|\
         {{#each pagination.items}}[{{fm.title}}]{{/each}}|next={{pagination.next_url}}",
    )
    .expect("Failed to write term template");
    fs::write(
        pages_dir.join("tags/taxonomy.hbs"),
        "{{#each taxonomy.terms}}<a href=\"{{url}}\">{{name}} ({{count}})</a>{{#each items}}[{{fm.title}}]{{/each}}{{/each}}",
    )
    .expect("Failed to write taxonomy template");

    for (slug, title, date, tags) in [
        ("one", "One", "2025-01-01", "[Rust, Web Dev]"),
        ("two", "Two", "2025-02-01", "[rust]"),
        ("three", "Three", "2025-03-01", "[]"),
    ] {
        fs::write(
            posts_content_dir.join(format!("{slug}.md")),
            format!("---\ntitle: {title}\ndate: {date}\ntags: {tags}\n---\n\nBody"),
        )
        .expect("Failed to write post");
    }

    let config_content = fs::read_to_string(root.join("balzac.toml")).unwrap();
    let config: Config = toml::from_str(&config_content).expect("Failed to parse config");
    let resolved_config = config.resolve(root);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let site_pages = discover_site_pages(&resolved_config).expect("Failed to discover pages");
    let mut renderer = HandlebarsRenderer::new(&resolved_config);
    renderer.init(&resolved_config);
    render_pages(&resolved_config, site_pages.all(), &renderer).expect("Failed to render pages");
    render_collection_items(&resolved_config, site_pages.all(), &renderer)
        .expect("Failed to render collections");
    render_listing_pages(&resolved_config, site_pages.all(), &renderer)
        .expect("Failed to render listings");
    write_sitemap(&resolved_config, &site_pages).expect("Failed to write sitemap");

    temp_dir
}

fn read_output(temp_dir: &TempDir, path: &str) -> String {
    fs::read_to_string(temp_dir.path().join("dist").join(path))
        .unwrap_or_else(|e| panic!("Failed to read {path}: {e}"))
}

#[test]
fn test_paginated_collection_listing() {
    let temp_dir = build_blog_with_listings();
    let read = |path: &str| read_output(&temp_dir, path);

    assert_eq!(
        read("posts/index.html"),
        "My Blog|posts|1/2|[Three][Two]|prev=|next=/posts/page/2/|*12|..."
    );
    assert_eq!(
        read("posts/page/2/index.html"),
        "My Blog|posts|2/2|[One]|prev=/posts/|next=|1*2|..."
    );
    assert!(!temp_dir.path().join("dist/posts/page/1").exists());
}

#[test]
fn test_taxonomy_pages_and_sitemap() {
    let temp_dir = build_blog_with_listings();
    let read = |path: &str| read_output(&temp_dir, path);

    assert_eq!(
        read("tags/rust/index.html"),
        "tags:rust(2)|[Two]|next=/tags/rust/page/2/"
    );
    assert_eq!(
        read("tags/rust/page/2/index.html"),
        "tags:rust(2)|[One]|next="
    );
    assert_eq!(
        read("tags/web-dev/index.html"),
        "tags:Web Dev(1)|[One]|next="
    );
    assert_eq!(
        read("tags/index.html"),
        "<a href=\"/tags/rust/\">rust (2)</a>[Two][One]\
         <a href=\"/tags/web-dev/\">Web Dev (1)</a>[One]"
    );

    assert_eq!(
        read("posts/one.html"),
        "<a href=\"/tags/rust/\">Rust</a><a href=\"/tags/web-dev/\">Web Dev</a>"
    );

    let sitemap = read("sitemap.xml");
    for url in [
        "/posts/",
        "/posts/page/2/",
        "/tags/",
        "/tags/rust/",
        "/tags/rust/page/2/",
        "/tags/web-dev/",
    ] {
        assert!(
            sitemap.contains(&format!("<loc>https://example.com{url}</loc>")),
            "Sitemap should contain {url}: {sitemap}"
        );
    }
}

#[test]
fn test_listing_config_rejects_zero_per_page() {
    let result = toml::from_str::<Config>("[listings.posts]\nper_page = 0\n");
    assert!(result.is_err(), "per_page = 0 should be rejected");
}

#[test]
fn test_markdown_extras_in_collection_details() {
    let (
        _temp,
        temp_path,
        pages_dir,
        output_dir,
        _layouts_dir,
        _partials_dir,
        _assets_dir,
        content_dir,
    ) = setup_test_project();

    let blog_pages_dir = pages_dir.join("blog");
    let blog_content_dir = content_dir.join("blog");
    fs::create_dir(&blog_pages_dir).expect("Failed to create blog pages dir");
    fs::create_dir_all(&blog_content_dir).expect("Failed to create blog content dir");

    fs::write(
        blog_pages_dir.join("details.hbs"),
        "<aside>{{{excerpt}}}</aside>\
         <nav>{{#each toc}}[{{level}}:{{id}}:{{text}}]{{/each}}</nav>\
         <div class=\"toc\">{{{toc_html}}}</div>\
         <main>{{{content}}}</main>",
    )
    .expect("Failed to write details template");

    fs::write(
        blog_content_dir.join("post.md"),
        "---\ntitle: Post\n---\n\nTeaser[^n].\n\n<!-- more -->\n\n## Setup\n\n```rust\nlet x = 1;\n```\n\n[^n]: A note.",
    )
    .expect("Failed to write post");

    let config_toml = format!(
        "output_directory = {:?}\npages_directory = {:?}\ncontent_directory = {:?}\n\n\
         [markdown]\nheading_ids = true\nfootnotes = true\nhighlight_theme = \"InspiredGitHub\"\n",
        output_dir.to_string_lossy(),
        pages_dir.to_string_lossy(),
        content_dir.to_string_lossy(),
    );
    let config: Config = toml::from_str(&config_toml).expect("Failed to parse config");

    let resolved_config = config.resolve(&temp_path);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let collection_pages =
        discover_collections(&resolved_config).expect("Failed to discover collections");
    let mut renderer = HandlebarsRenderer::new(&resolved_config);
    renderer.init(&resolved_config);
    render_collection_items(&resolved_config, &collection_pages, &renderer)
        .expect("Failed to render collections");
    balzac::markdown::write_highlight_css(&resolved_config).expect("Failed to write theme css");

    let output = fs::read_to_string(output_dir.join("blog").join("post.html"))
        .expect("Failed to read output");

    assert!(
        output.contains(r##"<aside><p>Teaser<sup class="footnote-ref"><a href="#fn-n""##),
        "excerpt should contain the content before the marker, got: {output}"
    );
    assert!(
        output.contains("<nav>[2:setup:Setup]</nav>"),
        "got: {output}"
    );
    assert!(
        output.contains(r##"<li><a href="#setup">Setup</a></li>"##),
        "got: {output}"
    );
    assert!(
        output.contains(r#"<h2 id="setup">Setup</h2>"#),
        "got: {output}"
    );
    assert!(
        output.contains(r#"<pre class="hl-code"><code class="language-rust">"#),
        "got: {output}"
    );
    assert!(
        output.contains(r#"<section class="footnotes""#),
        "got: {output}"
    );

    let css = fs::read_to_string(output_dir.join("highlight.css")).expect("Failed to read css");
    assert!(css.contains(".hl-code {"), "got: {css}");
}

#[test]
fn test_unknown_highlight_theme_is_an_error() {
    let (_temp, temp_path, ..) = setup_test_project();
    let config: Config =
        toml::from_str("[markdown]\nhighlight_theme = \"nope\"\n").expect("Failed to parse");
    let resolved_config = config.resolve(&temp_path);

    let error = balzac::markdown::write_highlight_css(&resolved_config)
        .expect_err("Unknown theme should fail");
    assert!(error.to_string().contains("InspiredGitHub"), "got: {error}");
}
