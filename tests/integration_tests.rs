use balzac::renderer::{HandlebarsRenderer, Renderer};
use balzac::sitemap::SitePages;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

// Import from the main crate
use balzac::config::{
    CombinedFeedConfig, CombinedFeedSetting, Config, FeedConfig, FeedFormat, FeedOverride,
    ListingConfig, RobotsConfig, SitemapConfig, TaxonomyConfig, UrlStyle,
};
use balzac::listing::render_listing_pages;
use balzac::{
    add_assets, discover_collections, discover_site_pages, discover_static_pages, make_dist_folder,
    render_collection_items, render_pages, write_feeds, write_robots, write_sitemap,
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
        url_style: UrlStyle::default(),
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
        feed: None,
        collections: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);
    let mut renderer = HandlebarsRenderer::new(&resolved_config);
    renderer
        .init(&resolved_config)
        .expect("Failed to init renderer");

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
        url_style: UrlStyle::default(),
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
        feed: None,
        collections: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);
    let mut renderer = HandlebarsRenderer::new(&resolved_config);
    renderer
        .init(&resolved_config)
        .expect("Failed to init renderer");

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
        url_style: UrlStyle::default(),
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
        feed: None,
        collections: HashMap::default(),
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
        url_style: UrlStyle::default(),
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
        feed: None,
        collections: HashMap::default(),
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
        url_style: UrlStyle::default(),
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
        feed: None,
        collections: HashMap::default(),
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
        url_style: UrlStyle::default(),
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
        feed: None,
        collections: HashMap::default(),
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
        url_style: UrlStyle::default(),
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
        feed: None,
        collections: HashMap::default(),
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
        url_style: UrlStyle::default(),
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
        feed: None,
        collections: HashMap::default(),
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
        url_style: UrlStyle::default(),
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
        feed: None,
        collections: HashMap::default(),
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
        url_style: UrlStyle::default(),
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
        feed: None,
        collections: HashMap::default(),
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
        url_style: UrlStyle::default(),
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
        feed: None,
        collections: HashMap::default(),
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
        url_style: UrlStyle::default(),
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
        feed: None,
        collections: HashMap::default(),
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
        url_style: UrlStyle::default(),
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
        feed: None,
        collections: HashMap::default(),
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
        url_style: UrlStyle::default(),
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
        feed: None,
        collections: HashMap::default(),
    };

    let resolved_config = config.resolve(&temp_path);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let collection_pages =
        discover_collections(&resolved_config).expect("Failed to discover collections");

    let mut renderer = HandlebarsRenderer::new(&resolved_config);
    renderer
        .init(&resolved_config)
        .expect("Failed to init renderer");
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
        url_style: UrlStyle::default(),
        listings: HashMap::default(),
        taxonomies: HashMap::default(),
        feed: None,
        collections: HashMap::default(),
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

/// Builds a blog with a paginated `posts` listing and a `tags` taxonomy;
/// `extra_config` is prepended to `balzac.toml`.
fn build_blog_with_listings(extra_config: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let root = temp_dir.path();
    let pages_dir = root.join("pages");
    let posts_content_dir = root.join("content/posts");
    fs::create_dir_all(pages_dir.join("posts")).expect("Failed to create posts pages dir");
    fs::create_dir_all(pages_dir.join("tags")).expect("Failed to create tags pages dir");
    fs::create_dir_all(&posts_content_dir).expect("Failed to create posts content dir");

    fs::write(
        root.join("balzac.toml"),
        extra_config.to_string()
            + r#"
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
         {{#each pagination.items}}[{{fm.title}}@{{url}}]{{/each}}|\
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
    renderer
        .init(&resolved_config)
        .expect("Failed to init renderer");
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
    let temp_dir = build_blog_with_listings("");
    let read = |path: &str| read_output(&temp_dir, path);

    assert_eq!(
        read("posts/index.html"),
        "My Blog|posts|1/2|[Three@/posts/three][Two@/posts/two]|prev=|next=/posts/page/2/|*12|..."
    );
    assert_eq!(
        read("posts/page/2/index.html"),
        "My Blog|posts|2/2|[One@/posts/one]|prev=/posts/|next=|1*2|..."
    );
    assert!(!temp_dir.path().join("dist/posts/page/1").exists());
}

#[test]
fn test_taxonomy_pages_and_sitemap() {
    let temp_dir = build_blog_with_listings("");
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
fn test_listings_and_taxonomies_with_directory_url_style() {
    let temp_dir = build_blog_with_listings("url_style = \"directory\"\n");
    let read = |path: &str| read_output(&temp_dir, path);
    let dist = temp_dir.path().join("dist");

    // Listing URLs are directory-shaped in every style; only item URLs change.
    assert_eq!(
        read("posts/index.html"),
        "My Blog|posts|1/2|[Three@/posts/three/][Two@/posts/two/]|prev=|next=/posts/page/2/|*12|..."
    );
    assert_eq!(
        read("posts/page/2/index.html"),
        "My Blog|posts|2/2|[One@/posts/one/]|prev=/posts/|next=|1*2|..."
    );
    assert_eq!(
        read("tags/rust/index.html"),
        "tags:rust(2)|[Two]|next=/tags/rust/page/2/"
    );
    assert_eq!(
        read("tags/index.html"),
        "<a href=\"/tags/rust/\">rust (2)</a>[Two][One]\
         <a href=\"/tags/web-dev/\">Web Dev (1)</a>[One]"
    );

    // Items live next to the listing pages without colliding with them.
    assert_eq!(
        read("posts/one/index.html"),
        "<a href=\"/tags/rust/\">Rust</a><a href=\"/tags/web-dev/\">Web Dev</a>"
    );
    assert!(!dist.join("posts/one.html").exists());

    let sitemap = read("sitemap.xml");
    for url in [
        "/posts/",
        "/posts/page/2/",
        "/posts/one/",
        "/posts/two/",
        "/tags/",
        "/tags/rust/",
        "/tags/rust/page/2/",
    ] {
        assert!(
            sitemap.contains(&format!("<loc>https://example.com{url}</loc>")),
            "Sitemap should contain {url}: {sitemap}"
        );
    }
    assert!(
        !sitemap.contains("<loc>https://example.com/posts/one</loc>"),
        "Sitemap should only use trailing-slash item URLs: {sitemap}"
    );
}

#[test]
fn test_directory_url_style_static_page_colliding_with_listing_fails() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let root = temp_dir.path();
    fs::create_dir_all(root.join("pages/posts")).expect("Failed to create pages dir");
    fs::create_dir_all(root.join("content/posts")).expect("Failed to create content dir");
    fs::write(root.join("pages/posts.hbs"), "page").expect("Failed to write page");
    fs::write(root.join("pages/posts/details.hbs"), "item").expect("Failed to write details");
    fs::write(root.join("pages/posts/list.hbs"), "list").expect("Failed to write list");

    let config: Config = toml::from_str("url_style = \"directory\"\n[listings.posts]\n")
        .expect("Failed to parse config");
    let error = discover_site_pages(&config.resolve(root))
        .expect_err("pages/posts.hbs and the posts listing both write posts/index.html");
    assert!(
        matches!(&error, balzac::error::Error::Io(e) if e.kind() == std::io::ErrorKind::InvalidInput),
        "{error:?}"
    );
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
    renderer
        .init(&resolved_config)
        .expect("Failed to init renderer");
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

#[test]
fn test_directory_url_style_writes_index_files_and_trailing_slash_urls() {
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

    fs::create_dir_all(pages_dir.join("blog")).expect("Failed to create blog pages dir");
    fs::create_dir_all(content_dir.join("blog")).expect("Failed to create blog content dir");
    fs::write(pages_dir.join("index.hbs"), "<h1>Home</h1>").expect("Failed to write index");
    fs::write(pages_dir.join("about.hbs"), "<h1>About</h1>").expect("Failed to write about");
    fs::write(pages_dir.join("404.hbs"), "<h1>Not found</h1>").expect("Failed to write 404");
    fs::write(pages_dir.join("blog/details.hbs"), "<h1>{{fm.title}}</h1>")
        .expect("Failed to write details template");
    fs::write(
        content_dir.join("blog/post-1.md"),
        "---\ntitle: First Post\n---\n\nHello",
    )
    .expect("Failed to write post-1");

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        base_url: Some("https://example.com".to_string()),
        sitemap: Some(SitemapConfig::default()),
        url_style: UrlStyle::Directory,
        ..Config::default()
    };
    let resolved_config = config.resolve(&temp_path);

    make_dist_folder(&resolved_config).expect("Failed to make dist folder");
    let mut site_pages = SitePages::new();
    site_pages.add_pages(discover_static_pages(&resolved_config).expect("Failed to discover"));
    site_pages.add_pages(discover_collections(&resolved_config).expect("Failed to discover"));

    let renderer = HandlebarsRenderer::new(&resolved_config);
    render_pages(&resolved_config, site_pages.all(), &renderer).expect("Failed to render pages");
    render_collection_items(&resolved_config, site_pages.all(), &renderer)
        .expect("Failed to render collections");
    write_sitemap(&resolved_config, &site_pages).expect("Failed to write sitemap");

    assert!(output_dir.join("index.html").is_file());
    assert!(output_dir.join("about/index.html").is_file());
    assert!(output_dir.join("blog/post-1/index.html").is_file());
    assert!(
        output_dir.join("404.html").is_file(),
        "404 page must stay at the root for static hosts"
    );
    assert!(!output_dir.join("about.html").exists());
    assert!(!output_dir.join("blog/post-1.html").exists());

    let sitemap = fs::read_to_string(output_dir.join("sitemap.xml")).expect("Failed to read");
    assert!(sitemap.contains("<loc>https://example.com/</loc>"));
    assert!(sitemap.contains("<loc>https://example.com/about/</loc>"));
    assert!(sitemap.contains("<loc>https://example.com/blog/post-1/</loc>"));
    assert!(!sitemap.contains("<loc>https://example.com/about</loc>"));
}

#[test]
fn test_directory_url_style_page_named_like_assets_directory() {
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

    fs::create_dir(&assets_dir).expect("Failed to create assets dir");
    fs::write(assets_dir.join("style.css"), "body {}").expect("Failed to write asset");
    fs::write(pages_dir.join("assets.hbs"), "<h1>Assets</h1>").expect("Failed to write page");

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        url_style: UrlStyle::Directory,
        ..Config::default()
    };
    let resolved_config = config.resolve(&temp_path);

    make_dist_folder(&resolved_config).expect("Failed to make dist folder");
    let pages = discover_static_pages(&resolved_config).expect("Failed to discover");
    let renderer = HandlebarsRenderer::new(&resolved_config);
    render_pages(&resolved_config, &pages, &renderer).expect("Failed to render pages");
    add_assets(&resolved_config).expect("Assets should copy into the page's directory");

    assert!(output_dir.join("assets/index.html").is_file());
    assert!(output_dir.join("assets/style.css").is_file());
}

#[test]
fn test_asset_colliding_with_rendered_page_fails_instead_of_overwriting() {
    let (_temp, temp_path, pages_dir, output_dir, _, _, assets_dir, _) = setup_test_project();

    fs::create_dir(&assets_dir).expect("Failed to create assets dir");
    fs::write(assets_dir.join("index.html"), "asset").expect("Failed to write asset");
    fs::write(pages_dir.join("assets.hbs"), "page").expect("Failed to write page");

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        url_style: UrlStyle::Directory,
        ..Config::default()
    };
    let resolved_config = config.resolve(&temp_path);

    make_dist_folder(&resolved_config).expect("Failed to make dist folder");
    let pages = discover_static_pages(&resolved_config).expect("Failed to discover");
    let renderer = HandlebarsRenderer::new(&resolved_config);
    render_pages(&resolved_config, &pages, &renderer).expect("Failed to render pages");

    let error = add_assets(&resolved_config).expect_err("collision should be reported");
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(
        fs::read_to_string(output_dir.join("assets/index.html")).unwrap(),
        "page",
        "the rendered page must not be replaced"
    );
}

#[test]
fn test_add_assets_copies_nested_directories() {
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

    fs::create_dir_all(assets_dir.join("img/icons")).expect("Failed to create assets dirs");
    fs::create_dir_all(assets_dir.join("empty")).expect("Failed to create empty dir");
    fs::write(assets_dir.join("style.css"), "body {}").expect("Failed to write css");
    fs::write(assets_dir.join("img/logo.png"), [0x89, 0x50, 0x4e, 0x47])
        .expect("Failed to write logo");
    fs::write(assets_dir.join("img/icons/star.svg"), "<svg/>").expect("Failed to write icon");

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: layouts_dir.to_string_lossy().to_string(),
        partials_directory: partials_dir.to_string_lossy().to_string(),
        assets_directory: assets_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        tags_directory: "./tags".to_string(),
        ..Config::default()
    };
    let resolved_config = config.resolve(&temp_path);

    make_dist_folder(&resolved_config).expect("Failed to create dist folder");
    add_assets(&resolved_config).expect("Failed to copy assets");

    let dist_assets = output_dir.join("assets");
    assert_eq!(
        fs::read_to_string(dist_assets.join("style.css")).unwrap(),
        "body {}"
    );
    assert_eq!(
        fs::read(dist_assets.join("img/logo.png")).unwrap(),
        vec![0x89, 0x50, 0x4e, 0x47]
    );
    assert_eq!(
        fs::read_to_string(dist_assets.join("img/icons/star.svg")).unwrap(),
        "<svg/>"
    );
    assert!(dist_assets.join("empty").is_dir());
}

fn assets_config(
    temp_path: &std::path::Path,
    output_directory: &str,
) -> balzac::config::ResolvedConfig {
    Config {
        output_directory: output_directory.to_string(),
        pages_directory: "./pages".to_string(),
        layouts_directory: "./layouts".to_string(),
        partials_directory: "./partials".to_string(),
        assets_directory: "./assets".to_string(),
        content_directory: "./content".to_string(),
        tags_directory: "./tags".to_string(),
        ..Config::default()
    }
    .resolve(temp_path)
}

#[test]
fn test_add_assets_skips_output_directory_inside_assets() {
    let (_temp, temp_path, _, _, _, _, assets_dir, _) = setup_test_project();
    fs::create_dir_all(assets_dir.join("img")).expect("Failed to create assets dirs");
    fs::write(assets_dir.join("img/logo.png"), "logo").expect("Failed to write logo");

    let resolved_config = assets_config(&temp_path, "./assets/dist");
    make_dist_folder(&resolved_config).expect("Failed to create dist folder");
    add_assets(&resolved_config).expect("Failed to copy assets");

    let dist_assets = assets_dir.join("dist/assets");
    assert_eq!(
        fs::read_to_string(dist_assets.join("img/logo.png")).unwrap(),
        "logo"
    );
    assert!(
        !dist_assets.join("dist").exists(),
        "Output directory should not be copied into itself"
    );
}

#[cfg(unix)]
#[test]
fn test_add_assets_skips_entries_that_are_not_regular_files() {
    let (_temp, temp_path, _, output_dir, _, _, assets_dir, _) = setup_test_project();
    let shared_dir = temp_path.join("shared");
    fs::create_dir_all(&shared_dir).expect("Failed to create shared dir");
    fs::create_dir_all(&assets_dir).expect("Failed to create assets dir");
    fs::write(shared_dir.join("font.woff2"), "font").expect("Failed to write font");
    fs::write(assets_dir.join("style.css"), "body {}").expect("Failed to write css");
    std::os::unix::fs::symlink(&shared_dir, assets_dir.join("shared"))
        .expect("Failed to create dir symlink");
    std::os::unix::fs::symlink(assets_dir.join("style.css"), assets_dir.join("alias.css"))
        .expect("Failed to create file symlink");
    std::os::unix::fs::symlink(
        temp_path.join("missing.png"),
        assets_dir.join("dangling.png"),
    )
    .expect("Failed to create dangling symlink");
    let _socket = std::os::unix::net::UnixListener::bind(assets_dir.join("dev.sock"))
        .expect("Failed to create socket");

    let resolved_config = assets_config(&temp_path, "./dist");
    make_dist_folder(&resolved_config).expect("Failed to create dist folder");
    add_assets(&resolved_config).expect(
        "Symlinked directories, dangling links and special files should not fail the build",
    );

    let dist_assets = output_dir.join("assets");
    assert_eq!(
        fs::read_to_string(dist_assets.join("alias.css")).unwrap(),
        "body {}"
    );
    assert!(!dist_assets.join("shared").exists());
    assert!(!dist_assets.join("dangling.png").exists());
    assert!(!dist_assets.join("dev.sock").exists());
}

#[test]
fn test_add_assets_skips_output_directory_inside_assets_before_it_exists() {
    let (_temp, temp_path, _, _, _, _, assets_dir, _) = setup_test_project();
    fs::create_dir_all(&assets_dir).expect("Failed to create assets dir");
    fs::write(assets_dir.join("style.css"), "body {}").expect("Failed to write css");

    let resolved_config = assets_config(&temp_path, "./assets/dist");
    add_assets(&resolved_config).expect("Failed to copy assets");

    let dist_assets = assets_dir.join("dist/assets");
    assert_eq!(
        fs::read_to_string(dist_assets.join("style.css")).unwrap(),
        "body {}"
    );
    assert!(
        !dist_assets.join("dist").exists(),
        "Output directory should not be copied into itself"
    );
}

fn static_page_config(
    temp_path: &std::path::Path,
    pages_dir: &std::path::Path,
    output_dir: &std::path::Path,
    global: Option<std::collections::HashMap<String, serde_json::Value>>,
) -> balzac::config::ResolvedConfig {
    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        layouts_directory: temp_path.join("layouts").to_string_lossy().to_string(),
        partials_directory: temp_path.join("partials").to_string_lossy().to_string(),
        assets_directory: temp_path.join("assets").to_string_lossy().to_string(),
        content_directory: temp_path.join("content").to_string_lossy().to_string(),
        tags_directory: "./tags".to_string(),
        global,
        hooks: None,
        bundler: None,
        base_url: Some("https://example.com".to_string()),
        sitemap: Some(SitemapConfig::default()),
        ..Config::default()
    };
    config.resolve(temp_path)
}

#[test]
fn test_static_page_frontmatter_exposed_as_fm() {
    let (_temp, temp_path, pages_dir, output_dir, ..) = setup_test_project();

    fs::write(
        pages_dir.join("about.hbs"),
        "---\ntitle: About us\ntags:\n  - a\n  - b\n---\n<h1>{{fm.title}}</h1>{{#each fm.tags}}<i>{{this}}</i>{{/each}}<p>{{site_name}}</p>",
    )
    .expect("Failed to write page");

    let mut global = std::collections::HashMap::new();
    global.insert("site_name".to_string(), serde_json::json!("Balzac"));
    let resolved_config = static_page_config(&temp_path, &pages_dir, &output_dir, Some(global));

    make_dist_folder(&resolved_config).expect("Failed to make dist folder");
    let pages = discover_static_pages(&resolved_config).expect("Failed to discover pages");
    assert_eq!(pages[0].frontmatter.as_ref().unwrap()["title"], "About us");
    render_pages(
        &resolved_config,
        &pages,
        &HandlebarsRenderer::new(&resolved_config),
    )
    .expect("Failed to render static pages");

    let output = fs::read_to_string(output_dir.join("about.html")).expect("Failed to read output");
    assert_eq!(output, "<h1>About us</h1><i>a</i><i>b</i><p>Balzac</p>");
}

#[test]
fn test_static_page_without_frontmatter_is_unchanged() {
    let (_temp, temp_path, pages_dir, output_dir, ..) = setup_test_project();

    let template = "\n<hr>\n---\n<p>not frontmatter</p>\n---\n";
    fs::write(pages_dir.join("index.hbs"), template).expect("Failed to write page");

    let resolved_config = static_page_config(&temp_path, &pages_dir, &output_dir, None);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");
    let pages = discover_static_pages(&resolved_config).expect("Failed to discover pages");
    assert!(pages[0].frontmatter.is_none());
    render_pages(
        &resolved_config,
        &pages,
        &HandlebarsRenderer::new(&resolved_config),
    )
    .expect("Failed to render static pages");

    let output = fs::read_to_string(output_dir.join("index.html")).expect("Failed to read output");
    assert_eq!(output, template);
}

#[test]
fn test_static_page_invalid_frontmatter_errors() {
    let (_temp, temp_path, pages_dir, output_dir, ..) = setup_test_project();

    fs::write(
        pages_dir.join("broken.hbs"),
        "---\ntitle: [unclosed\n---\n<h1>Broken</h1>",
    )
    .expect("Failed to write page");

    let resolved_config = static_page_config(&temp_path, &pages_dir, &output_dir, None);
    let err = discover_static_pages(&resolved_config).expect_err("Invalid YAML should fail");
    assert!(err.to_string().contains("broken.hbs"), "{}", err);
}

#[test]
fn test_static_page_frontmatter_sitemap_meta() {
    let (_temp, temp_path, pages_dir, output_dir, ..) = setup_test_project();

    fs::write(
        pages_dir.join("index.hbs"),
        "---\nlastmod: \"2024-03-01\"\npriority: 1.0\nchangefreq: daily\n---\n<h1>Home</h1>",
    )
    .expect("Failed to write index");
    fs::write(
        pages_dir.join("secret.hbs"),
        "---\nsitemap_exclude: true\n---\n<h1>Secret</h1>",
    )
    .expect("Failed to write secret");

    let resolved_config = static_page_config(&temp_path, &pages_dir, &output_dir, None);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let mut site_pages = SitePages::new();
    site_pages
        .add_pages(discover_static_pages(&resolved_config).expect("Failed to discover pages"));
    render_pages(
        &resolved_config,
        site_pages.all(),
        &HandlebarsRenderer::new(&resolved_config),
    )
    .expect("Failed to render static pages");
    write_sitemap(&resolved_config, &site_pages).expect("Failed to write sitemap");

    let sitemap =
        fs::read_to_string(output_dir.join("sitemap.xml")).expect("Failed to read sitemap");
    assert!(sitemap.contains("<loc>https://example.com/</loc>"));
    assert!(sitemap.contains("<lastmod>2024-03-01</lastmod>"));
    assert!(sitemap.contains("<priority>1.0</priority>"));
    assert!(sitemap.contains("<changefreq>daily</changefreq>"));
    assert!(!sitemap.contains("/secret"));

    // Excluded pages are still rendered, with frontmatter stripped
    let secret = fs::read_to_string(output_dir.join("secret.html")).expect("Failed to read page");
    assert_eq!(secret, "<h1>Secret</h1>");
}

#[test]
fn test_render_static_page_from_path_only_entry() {
    let (_temp, temp_path, pages_dir, output_dir, ..) = setup_test_project();

    let source = pages_dir.join("about.hbs");
    fs::write(&source, "---\ntitle: About\n---\n<h1>{{fm.title}}</h1>")
        .expect("Failed to write page");

    let resolved_config = static_page_config(&temp_path, &pages_dir, &output_dir, None);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let page = balzac::static_page_entry_from_source(&resolved_config, &source)
        .expect("Page should map to an entry");
    balzac::render_static_page(
        &resolved_config,
        &page,
        &HandlebarsRenderer::new(&resolved_config),
        &serde_json::json!({}),
    )
    .expect("Failed to render page");

    let output = fs::read_to_string(output_dir.join("about.html")).expect("Failed to read output");
    assert_eq!(output, "<h1>About</h1>");
}

#[test]
fn test_static_page_edits_after_discovery_are_rendered() {
    let (_temp, temp_path, pages_dir, output_dir, ..) = setup_test_project();

    let source = pages_dir.join("index.hbs");
    fs::write(&source, "---\ntitle: Old\n---\n<h1>{{fm.title}}</h1>").expect("Failed to write");

    let resolved_config = static_page_config(&temp_path, &pages_dir, &output_dir, None);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");
    let pages = discover_static_pages(&resolved_config).expect("Failed to discover pages");

    // Simulates a render_before hook rewriting the page after discovery
    fs::write(&source, "---\ntitle: New\n---\n<h2>{{fm.title}}</h2>").expect("Failed to write");

    render_pages(
        &resolved_config,
        &pages,
        &HandlebarsRenderer::new(&resolved_config),
    )
    .expect("Failed to render static pages");

    let output = fs::read_to_string(output_dir.join("index.html")).expect("Failed to read output");
    assert_eq!(output, "<h2>New</h2>");
}

#[test]
fn test_static_page_frontmatter_alongside_collections() {
    let (_temp, temp_path, pages_dir, output_dir, ..) = setup_test_project();

    let content_dir = temp_path.join("content");
    fs::create_dir(pages_dir.join("posts")).expect("Failed to create posts pages dir");
    fs::create_dir_all(content_dir.join("posts")).expect("Failed to create posts content dir");
    fs::write(pages_dir.join("posts").join("details.hbs"), "{{fm.title}}")
        .expect("Failed to write details template");
    fs::write(
        content_dir.join("posts").join("hello.md"),
        "---\ntitle: Hello\n---\n\nBody",
    )
    .expect("Failed to write post");
    fs::write(
        pages_dir.join("index.hbs"),
        "---\nheading: Latest\n---\n<h1>{{fm.heading}}</h1>{{#each collections.posts}}<a href=\"{{url}}\">{{fm.title}}</a>{{/each}}",
    )
    .expect("Failed to write index");

    let resolved_config = static_page_config(&temp_path, &pages_dir, &output_dir, None);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let mut site_pages = SitePages::new();
    site_pages
        .add_pages(discover_static_pages(&resolved_config).expect("Failed to discover pages"));
    site_pages
        .add_pages(discover_collections(&resolved_config).expect("Failed to discover collections"));
    render_pages(
        &resolved_config,
        site_pages.all(),
        &HandlebarsRenderer::new(&resolved_config),
    )
    .expect("Failed to render static pages");

    let output = fs::read_to_string(output_dir.join("index.html")).expect("Failed to read output");
    assert_eq!(output, "<h1>Latest</h1><a href=\"/posts/hello\">Hello</a>");
}

#[test]
fn test_not_found_page_with_frontmatter_stays_out_of_sitemap() {
    let (_temp, temp_path, pages_dir, output_dir, ..) = setup_test_project();

    fs::write(pages_dir.join("index.hbs"), "<h1>Home</h1>").expect("Failed to write index");
    fs::write(
        pages_dir.join("404.hbs"),
        "---\ntitle: Not found\n---\n<h1>{{fm.title}}</h1>",
    )
    .expect("Failed to write 404");

    let resolved_config = static_page_config(&temp_path, &pages_dir, &output_dir, None);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let mut site_pages = SitePages::new();
    site_pages
        .add_pages(discover_static_pages(&resolved_config).expect("Failed to discover pages"));
    render_pages(
        &resolved_config,
        site_pages.all(),
        &HandlebarsRenderer::new(&resolved_config),
    )
    .expect("Failed to render static pages");
    write_sitemap(&resolved_config, &site_pages).expect("Failed to write sitemap");

    let sitemap =
        fs::read_to_string(output_dir.join("sitemap.xml")).expect("Failed to read sitemap");
    assert!(!sitemap.contains("404"), "{sitemap}");
    let not_found = fs::read_to_string(output_dir.join("404.html")).expect("Failed to read 404");
    assert_eq!(not_found, "<h1>Not found</h1>");
}

/// Builds a project with `blog` and `notes` collections and runs feed generation
fn build_feeds(base_url: Option<&str>, feed: FeedConfig) -> (TempDir, PathBuf) {
    let (temp, temp_path, pages_dir, output_dir, _, _, _, content_dir) = setup_test_project();

    for collection in ["blog", "notes"] {
        fs::create_dir_all(pages_dir.join(collection)).expect("Failed to create pages dir");
        fs::create_dir_all(content_dir.join(collection)).expect("Failed to create content dir");
        fs::write(
            pages_dir.join(collection).join("details.hbs"),
            "{{{content}}}",
        )
        .expect("Failed to write details template");
    }

    fs::write(
        content_dir.join("blog").join("first.md"),
        "---\ntitle: First & Foremost\ndate: 2024-01-15\nsummary: The <first> post\nauthor: Ada\n---\n\nHello **world**",
    )
    .expect("Failed to write post");
    fs::write(
        content_dir.join("blog").join("second.md"),
        "---\ntitle: Second\ndate: 2024-03-01T09:30:00+01:00\n---\n\nSecond body",
    )
    .expect("Failed to write post");
    fs::write(
        content_dir.join("blog").join("undated.md"),
        "---\ntitle: Undated\n---\n\nNo date",
    )
    .expect("Failed to write post");
    fs::write(
        content_dir.join("notes").join("note.md"),
        "---\ntitle: A note\ndate: 2024-02-01\n---\n\nNote body with [a link](/about)",
    )
    .expect("Failed to write note");

    // Custom markdown tag templates must apply to feed content too
    let tags_dir = temp_path.join("tags");
    fs::create_dir_all(&tags_dir).expect("Failed to create tags dir");
    fs::write(
        tags_dir.join("a.hbs"),
        r#"<a href="{{props.href}}" class="custom-link">{{{content}}}</a>"#,
    )
    .expect("Failed to write tag template");

    let config = Config {
        output_directory: output_dir.to_string_lossy().to_string(),
        pages_directory: pages_dir.to_string_lossy().to_string(),
        content_directory: content_dir.to_string_lossy().to_string(),
        base_url: base_url.map(str::to_string),
        feed: Some(feed),
        ..Default::default()
    };
    let resolved_config = config.resolve(&temp_path);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let mut site_pages = SitePages::new();
    site_pages
        .add_pages(discover_collections(&resolved_config).expect("Failed to discover collections"));
    let mut renderer = HandlebarsRenderer::new(&resolved_config);
    renderer
        .init(&resolved_config)
        .expect("Failed to init renderer");
    write_feeds(&resolved_config, &site_pages, &renderer).expect("Failed to write feeds");

    (temp, output_dir)
}

#[test]
fn test_feed_generation_atom_by_default() {
    let (_temp, output_dir) = build_feeds(
        Some("https://example.com/"),
        FeedConfig {
            title: Some("My Site".to_string()),
            ..Default::default()
        },
    );

    let atom = fs::read_to_string(output_dir.join("blog").join("atom.xml"))
        .expect("Atom feed should exist");
    assert!(!output_dir.join("blog").join("feed.xml").exists());
    assert!(output_dir.join("notes").join("atom.xml").exists());

    assert!(atom.contains("<title>My Site</title>"));
    assert!(atom.contains("<id>https://example.com/blog/atom.xml</id>"));
    assert!(atom.contains("<updated>2024-03-01T08:30:00Z</updated>"));
    assert!(atom.contains("<title>First &amp; Foremost</title>"));
    assert!(atom.contains(r#"href="https://example.com/blog/first""#));
    assert!(atom.contains("<published>2024-01-15T00:00:00Z</published>"));
    assert!(atom.contains(r#"<summary type="text">The &lt;first&gt; post</summary>"#));
    assert!(!atom.contains("<content"), "full_content is off by default");
    assert!(
        !atom.contains("Undated"),
        "entries without a date are skipped"
    );
    assert!(!atom.contains("A note"), "feeds are per collection");

    let second = atom.find("/blog/second").expect("second post present");
    let first = atom.find("/blog/first").expect("first post present");
    assert!(second < first, "entries are sorted newest first");
}

#[test]
fn test_feed_generation_rss_with_overrides_and_full_content() {
    let mut overrides = std::collections::BTreeMap::new();
    overrides.insert(
        "blog".to_string(),
        FeedOverride {
            title: Some("The Blog".to_string()),
            description: Some("Posts".to_string()),
        },
    );
    let (_temp, output_dir) = build_feeds(
        Some("https://example.com"),
        FeedConfig {
            format: FeedFormat::Both,
            collections: Some(vec!["blog".to_string()]),
            limit: 1,
            full_content: true,
            title: Some("My Site".to_string()),
            overrides,
            ..Default::default()
        },
    );

    assert!(output_dir.join("blog").join("atom.xml").exists());
    assert!(
        !output_dir.join("notes").exists(),
        "only listed collections get feeds"
    );

    let rss = fs::read_to_string(output_dir.join("blog").join("feed.xml"))
        .expect("RSS feed should exist");
    assert!(rss.contains("<title>The Blog</title>"));
    assert!(rss.contains("<description>Posts</description>"));
    assert!(rss.contains(r#"<atom:link href="https://example.com/blog/feed.xml""#));
    assert!(rss.contains("<pubDate>Fri, 01 Mar 2024 08:30:00 GMT</pubDate>"));
    assert!(rss.contains("<content:encoded>&lt;p&gt;Second body&lt;/p&gt;"));
    assert!(
        !rss.contains("/blog/first"),
        "limit keeps only the newest item"
    );
}

#[test]
fn test_no_feed_without_base_url() {
    let (_temp, output_dir) = build_feeds(None, FeedConfig::default());
    assert!(!output_dir.join("blog").join("atom.xml").exists());
}

#[test]
fn test_no_feed_when_disabled() {
    let (_temp, output_dir) = build_feeds(
        Some("https://example.com"),
        FeedConfig {
            enabled: false,
            ..Default::default()
        },
    );
    assert!(!output_dir.join("blog").join("atom.xml").exists());
}

#[test]
fn test_combined_feed_merges_collections() {
    let (_temp, output_dir) = build_feeds(
        Some("https://example.com"),
        FeedConfig {
            format: FeedFormat::Both,
            combined: Some(CombinedFeedSetting::Enabled(true)),
            limit: 2,
            title: Some("My Site".to_string()),
            ..Default::default()
        },
    );

    assert!(output_dir.join("blog").join("atom.xml").exists());
    assert!(output_dir.join("notes").join("atom.xml").exists());

    let atom =
        fs::read_to_string(output_dir.join("atom.xml")).expect("Combined Atom feed should exist");
    assert!(atom.contains("<id>https://example.com/atom.xml</id>"));
    assert!(atom.contains("<title>My Site</title>"));
    let second = atom.find("/blog/second").expect("newest blog post present");
    let note = atom.find("/notes/note").expect("note present");
    assert!(
        second < note,
        "entries from all collections sorted newest first"
    );
    assert!(
        !atom.contains("/blog/first"),
        "limit applies to the combined feed"
    );

    let rss =
        fs::read_to_string(output_dir.join("feed.xml")).expect("Combined RSS feed should exist");
    assert!(rss.contains(r#"<atom:link href="https://example.com/feed.xml""#));
}

#[test]
fn test_no_combined_feed_by_default() {
    let (_temp, output_dir) = build_feeds(Some("https://example.com"), FeedConfig::default());
    assert!(output_dir.join("blog").join("atom.xml").exists());
    assert!(!output_dir.join("atom.xml").exists());
}

#[test]
fn test_combined_feed_with_own_collections_and_settings() {
    let (_temp, output_dir) = build_feeds(
        Some("https://example.com"),
        FeedConfig {
            // No per-collection feeds, only the combined one
            collections: Some(Vec::new()),
            limit: 1,
            title: Some("My Site".to_string()),
            combined: Some(CombinedFeedSetting::Detailed(CombinedFeedConfig {
                collections: Some(vec!["notes".to_string()]),
                title: Some("Notes only".to_string()),
                description: None,
                limit: Some(0),
            })),
            ..Default::default()
        },
    );

    assert!(!output_dir.join("blog").exists());
    assert!(!output_dir.join("notes").exists());

    let atom =
        fs::read_to_string(output_dir.join("atom.xml")).expect("Combined Atom feed should exist");
    assert!(atom.contains("<title>Notes only</title>"));
    assert!(atom.contains("/notes/note"));
    assert!(
        !atom.contains("/blog/"),
        "only listed collections are combined"
    );
}

#[test]
fn test_combined_feed_from_collection_list() {
    let (_temp, output_dir) = build_feeds(
        Some("https://example.com"),
        FeedConfig {
            collections: Some(vec!["blog".to_string()]),
            combined: Some(CombinedFeedSetting::Collections(vec![
                "blog".to_string(),
                "notes".to_string(),
            ])),
            ..Default::default()
        },
    );

    assert!(output_dir.join("blog").join("atom.xml").exists());
    assert!(!output_dir.join("notes").exists());

    let atom =
        fs::read_to_string(output_dir.join("atom.xml")).expect("Combined Atom feed should exist");
    assert!(atom.contains("/blog/first"));
    assert!(atom.contains("/notes/note"));
    assert!(atom.contains("<title>https://example.com</title>"));
}

#[test]
fn test_feed_full_content_uses_custom_tag_templates() {
    let (_temp, output_dir) = build_feeds(
        Some("https://example.com"),
        FeedConfig {
            collections: Some(vec!["notes".to_string()]),
            full_content: true,
            ..Default::default()
        },
    );

    let atom = fs::read_to_string(output_dir.join("notes").join("atom.xml"))
        .expect("Atom feed should exist");
    assert!(
        atom.contains(
            "&lt;a href=&quot;/about&quot; class=&quot;custom-link&quot;&gt;a link&lt;/a&gt;"
        ),
        "feed content should be rendered through custom tag templates: {atom}"
    );
}

/// Builds a resolved config rooted at `temp_path` using the default directory layout
fn nested_pages_config(
    temp_path: &std::path::Path,
    base_url: Option<&str>,
) -> balzac::config::ResolvedConfig {
    Config {
        base_url: base_url.map(str::to_string),
        sitemap: base_url.map(|_| SitemapConfig::default()),
        ..Config::default()
    }
    .resolve(temp_path)
}

/// Runs discovery, rendering and sitemap generation like `balzac build`
fn build_full_site(resolved_config: &balzac::config::ResolvedConfig) -> SitePages {
    make_dist_folder(resolved_config).expect("Failed to make dist folder");
    let site_pages = discover_site_pages(resolved_config).expect("Failed to discover pages");

    let mut renderer = HandlebarsRenderer::new(resolved_config);
    renderer
        .init(resolved_config)
        .expect("Failed to init renderer");
    render_pages(resolved_config, site_pages.all(), &renderer).expect("Failed to render pages");
    render_collection_items(resolved_config, site_pages.all(), &renderer)
        .expect("Failed to render collections");
    write_sitemap(resolved_config, &site_pages).expect("Failed to write sitemap");

    site_pages
}

#[test]
fn test_nested_static_pages_are_rendered() {
    let (_temp, temp_path, pages_dir, output_dir, _, _, _, _) = setup_test_project();

    fs::create_dir_all(pages_dir.join("docs/guides")).expect("Failed to create nested dirs");
    fs::write(pages_dir.join("index.hbs"), "<h1>Home</h1>").expect("Failed to write index");
    fs::write(pages_dir.join("docs/index.hbs"), "<h1>Docs</h1>").expect("Failed to write docs");
    fs::write(
        pages_dir.join("docs/getting-started.hbs"),
        "<h1>Getting Started</h1>",
    )
    .expect("Failed to write getting started");
    fs::write(pages_dir.join("docs/guides/setup.html"), "<h1>Setup</h1>")
        .expect("Failed to write setup");
    fs::write(pages_dir.join("docs/notes.txt"), "not a page").expect("Failed to write notes");

    let resolved_config = nested_pages_config(&temp_path, None);
    let site_pages = build_full_site(&resolved_config);

    let mut url_paths: Vec<&str> = site_pages
        .static_pages()
        .iter()
        .map(|page| page.url_path.as_str())
        .collect();
    url_paths.sort_unstable();
    assert_eq!(
        url_paths,
        vec!["/", "/docs", "/docs/getting-started", "/docs/guides/setup"]
    );

    let read = |path: &str| fs::read_to_string(output_dir.join(path)).unwrap_or_default();
    assert_eq!(read("index.html"), "<h1>Home</h1>");
    assert_eq!(read("docs.html"), "<h1>Docs</h1>");
    assert_eq!(
        read("docs/getting-started.html"),
        "<h1>Getting Started</h1>"
    );
    assert_eq!(read("docs/guides/setup.html"), "<h1>Setup</h1>");
    assert!(!output_dir.join("docs/notes.html").exists());
}

#[test]
fn test_collection_template_is_not_rendered_as_static_page() {
    let (_temp, temp_path, pages_dir, output_dir, _, _, _, content_dir) = setup_test_project();

    fs::create_dir_all(pages_dir.join("blog")).expect("Failed to create blog pages dir");
    fs::create_dir_all(content_dir.join("blog")).expect("Failed to create blog content dir");
    fs::write(
        pages_dir.join("blog/details.hbs"),
        "<article>{{fm.title}}</article>",
    )
    .expect("Failed to write details template");
    fs::write(pages_dir.join("blog/index.hbs"), "<h1>Blog</h1>")
        .expect("Failed to write blog index");
    fs::write(
        content_dir.join("blog/hello.md"),
        "---\ntitle: Hello\n---\n\nHi",
    )
    .expect("Failed to write post");

    let resolved_config = nested_pages_config(&temp_path, None);
    let site_pages = build_full_site(&resolved_config);

    let static_urls: Vec<&str> = site_pages
        .static_pages()
        .iter()
        .map(|page| page.url_path.as_str())
        .collect();
    assert_eq!(static_urls, vec!["/blog"]);
    assert!(!output_dir.join("blog/details.html").exists());
    assert_eq!(
        fs::read_to_string(output_dir.join("blog.html")).unwrap(),
        "<h1>Blog</h1>"
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("blog/hello.html")).unwrap(),
        "<article>Hello</article>"
    );
}

#[test]
fn test_details_page_without_collection_is_static_page() {
    let (_temp, temp_path, pages_dir, output_dir, _, _, _, _) = setup_test_project();

    fs::create_dir_all(pages_dir.join("product")).expect("Failed to create product dir");
    fs::write(pages_dir.join("product/details.hbs"), "<h1>Specs</h1>")
        .expect("Failed to write product details");

    let resolved_config = nested_pages_config(&temp_path, None);
    build_full_site(&resolved_config);

    assert_eq!(
        fs::read_to_string(output_dir.join("product/details.html")).unwrap(),
        "<h1>Specs</h1>"
    );
}

#[test]
fn test_sitemap_includes_nested_static_pages() {
    let (_temp, temp_path, pages_dir, output_dir, _, _, _, content_dir) = setup_test_project();

    fs::create_dir_all(pages_dir.join("docs")).expect("Failed to create docs dir");
    fs::create_dir_all(pages_dir.join("blog")).expect("Failed to create blog pages dir");
    fs::create_dir_all(content_dir.join("blog")).expect("Failed to create blog content dir");
    fs::write(pages_dir.join("docs/index.hbs"), "<h1>Docs</h1>").expect("Failed to write docs");
    fs::write(pages_dir.join("docs/install.hbs"), "<h1>Install</h1>")
        .expect("Failed to write install");
    fs::write(pages_dir.join("blog/details.hbs"), "{{{content}}}")
        .expect("Failed to write details template");
    fs::write(
        content_dir.join("blog/post.md"),
        "---\ntitle: Post\n---\n\nBody",
    )
    .expect("Failed to write post");

    let resolved_config = nested_pages_config(&temp_path, Some("https://example.com"));
    build_full_site(&resolved_config);

    let sitemap = fs::read_to_string(output_dir.join("sitemap.xml")).expect("Missing sitemap");
    assert!(sitemap.contains("<loc>https://example.com/docs</loc>"));
    assert!(sitemap.contains("<loc>https://example.com/docs/install</loc>"));
    assert!(sitemap.contains("<loc>https://example.com/blog/post</loc>"));
    assert!(!sitemap.contains("/blog/details"));
}

#[test]
fn test_static_page_wins_over_collection_item_with_same_output() {
    for url_style in [UrlStyle::Extensionless, UrlStyle::Directory] {
        let (_temp, temp_path, pages_dir, _, _, _, _, content_dir) = setup_test_project();

        fs::create_dir_all(pages_dir.join("posts")).expect("Failed to create posts pages dir");
        fs::create_dir_all(content_dir.join("posts")).expect("Failed to create posts content dir");
        fs::write(
            pages_dir.join("posts/details.hbs"),
            "<article>{{fm.title}}</article>",
        )
        .expect("Failed to write details template");
        fs::write(pages_dir.join("posts/hello.hbs"), "<h1>Static hello</h1>")
            .expect("Failed to write static page");
        fs::write(
            content_dir.join("posts/hello.md"),
            "---\ntitle: Hello\n---\n\nBody",
        )
        .expect("Failed to write hello post");
        fs::write(
            content_dir.join("posts/world.md"),
            "---\ntitle: World\n---\n\nBody",
        )
        .expect("Failed to write world post");

        let resolved_config = Config {
            url_style,
            ..Config::default()
        }
        .resolve(&temp_path);
        let site_pages = build_full_site(&resolved_config);

        let collection_urls: Vec<&str> = site_pages
            .collection_items()
            .iter()
            .map(|page| page.url_path.as_str())
            .collect();
        assert_eq!(collection_urls.len(), 1, "{url_style:?}");
        assert!(collection_urls[0].starts_with("/posts/world"));

        let hello = &site_pages.static_pages()[0].output_path;
        assert_eq!(
            fs::read_to_string(hello).unwrap(),
            "<h1>Static hello</h1>",
            "{url_style:?}"
        );
        assert!(
            balzac::collection_output_path_from_source(
                &resolved_config,
                &content_dir.join("posts/hello.md")
            )
            .is_none(),
            "Shadowed collection item should not own an output path"
        );
    }
}

#[test]
fn test_nested_pages_with_directory_url_style() {
    let (_temp, temp_path, pages_dir, output_dir, _, _, _, _) = setup_test_project();

    fs::create_dir_all(pages_dir.join("docs")).expect("Failed to create docs dir");
    fs::write(pages_dir.join("docs/index.hbs"), "<h1>Docs</h1>").expect("Failed to write docs");
    fs::write(pages_dir.join("docs/install.hbs"), "<h1>Install</h1>")
        .expect("Failed to write install");
    fs::write(pages_dir.join("docs/404.hbs"), "<h1>Docs 404</h1>")
        .expect("Failed to write nested 404");

    let resolved_config = Config {
        url_style: UrlStyle::Directory,
        ..Config::default()
    }
    .resolve(&temp_path);
    let site_pages = build_full_site(&resolved_config);

    let mut url_paths: Vec<&str> = site_pages
        .static_pages()
        .iter()
        .map(|page| page.url_path.as_str())
        .collect();
    url_paths.sort_unstable();
    assert_eq!(url_paths, vec!["/docs/", "/docs/404/", "/docs/install/"]);

    let read = |path: &str| fs::read_to_string(output_dir.join(path)).unwrap_or_default();
    assert_eq!(read("docs/index.html"), "<h1>Docs</h1>");
    assert_eq!(read("docs/install/index.html"), "<h1>Install</h1>");
    // only a top-level 404 page is the site's error page
    assert_eq!(read("docs/404/index.html"), "<h1>Docs 404</h1>");
    assert!(!output_dir.join("404.html").exists());
}

#[test]
fn test_dotted_collection_item_does_not_replace_static_page() {
    let (_temp, temp_path, pages_dir, output_dir, _, _, _, content_dir) = setup_test_project();

    fs::create_dir_all(pages_dir.join("posts")).expect("Failed to create posts pages dir");
    fs::create_dir_all(content_dir.join("posts")).expect("Failed to create posts content dir");
    fs::write(
        pages_dir.join("posts/details.hbs"),
        "<article>{{fm.title}}</article>",
    )
    .expect("Failed to write details template");
    fs::write(pages_dir.join("posts/v1.hbs"), "<h1>Version 1</h1>")
        .expect("Failed to write static page");
    fs::write(
        content_dir.join("posts/v1.2.md"),
        "---\ntitle: Release 1.2\n---\n\nBody",
    )
    .expect("Failed to write dotted post");

    let resolved_config = nested_pages_config(&temp_path, None);
    let site_pages = build_full_site(&resolved_config);

    let item = site_pages.collection_items()[0];
    assert_eq!(item.url_path, "/posts/v1.2");
    assert_eq!(item.output_path, output_dir.join("posts/v1.2.html"));
    assert_eq!(
        fs::read_to_string(output_dir.join("posts/v1.html")).unwrap(),
        "<h1>Version 1</h1>"
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("posts/v1.2.html")).unwrap(),
        "<article>Release 1.2</article>"
    );
}

#[test]
fn test_listing_templates_are_not_rendered_as_static_pages() {
    let (_temp, temp_path, pages_dir, output_dir, _, _, _, content_dir) = setup_test_project();

    fs::create_dir_all(pages_dir.join("posts")).expect("Failed to create posts pages dir");
    fs::create_dir_all(pages_dir.join("tags")).expect("Failed to create tags pages dir");
    fs::create_dir_all(pages_dir.join("archive")).expect("Failed to create archive pages dir");
    fs::create_dir_all(content_dir.join("posts")).expect("Failed to create posts content dir");
    fs::write(pages_dir.join("posts/details.hbs"), "{{fm.title}}")
        .expect("Failed to write details template");
    fs::write(pages_dir.join("posts/list.hbs"), "list").expect("Failed to write list");
    fs::write(pages_dir.join("tags/term.hbs"), "term").expect("Failed to write term");
    fs::write(pages_dir.join("tags/taxonomy.hbs"), "taxonomy").expect("Failed to write taxonomy");
    // no `[listings.archive]`, so this is a regular page
    fs::write(pages_dir.join("archive/list.hbs"), "archive").expect("Failed to write archive");
    fs::write(
        content_dir.join("posts/hello.md"),
        "---\ntitle: Hello\ntags: [rust]\n---\n\nBody",
    )
    .expect("Failed to write post");

    let mut config = Config::default();
    config
        .listings
        .insert("posts".to_string(), ListingConfig::default());
    config
        .taxonomies
        .insert("tags".to_string(), TaxonomyConfig::default());
    let resolved_config = config.resolve(&temp_path);
    let site_pages = build_full_site(&resolved_config);

    let static_urls: Vec<&str> = site_pages
        .static_pages()
        .iter()
        .map(|page| page.url_path.as_str())
        .collect();
    assert_eq!(static_urls, vec!["/archive/list"]);
    assert!(!output_dir.join("posts/list.html").exists());
    assert!(!output_dir.join("tags/term.html").exists());
    assert!(!output_dir.join("tags/taxonomy.html").exists());
    assert_eq!(
        fs::read_to_string(output_dir.join("archive/list.html")).unwrap(),
        "archive"
    );
}

#[test]
fn test_static_page_conflicting_with_listing_fails_discovery() {
    let (_temp, temp_path, pages_dir, _, _, _, _, content_dir) = setup_test_project();

    fs::create_dir_all(pages_dir.join("posts")).expect("Failed to create posts pages dir");
    fs::create_dir_all(content_dir.join("posts")).expect("Failed to create posts content dir");
    fs::write(pages_dir.join("posts/details.hbs"), "{{fm.title}}")
        .expect("Failed to write details template");
    fs::write(pages_dir.join("posts/list.hbs"), "list").expect("Failed to write list");
    // in directory style both are written to `posts/index.html`
    fs::write(pages_dir.join("posts/index.hbs"), "static").expect("Failed to write index");
    fs::write(
        content_dir.join("posts/hello.md"),
        "---\ntitle: Hello\n---\n",
    )
    .expect("Failed to write post");

    let mut config = Config {
        url_style: UrlStyle::Directory,
        ..Config::default()
    };
    config
        .listings
        .insert("posts".to_string(), ListingConfig::default());
    let resolved_config = config.resolve(&temp_path);

    let error = discover_site_pages(&resolved_config).expect_err("pages should conflict");
    assert!(error.to_string().contains("posts/index.html"), "{error}");
}

#[test]
fn test_only_top_level_404_page_is_excluded_from_sitemap() {
    let (_temp, temp_path, pages_dir, output_dir, _, _, _, _) = setup_test_project();

    fs::create_dir_all(pages_dir.join("docs")).expect("Failed to create docs dir");
    fs::write(pages_dir.join("404.hbs"), "<h1>Lost</h1>").expect("Failed to write 404");
    fs::write(pages_dir.join("docs/404.hbs"), "<h1>Error codes</h1>")
        .expect("Failed to write nested 404");

    let resolved_config = nested_pages_config(&temp_path, Some("https://example.com"));
    build_full_site(&resolved_config);

    let sitemap = fs::read_to_string(output_dir.join("sitemap.xml")).expect("Missing sitemap");
    assert!(!sitemap.contains("<loc>https://example.com/404</loc>"));
    assert!(sitemap.contains("<loc>https://example.com/docs/404</loc>"));
}

#[test]
fn test_pages_writing_the_same_output_fail_discovery() {
    let (_temp, temp_path, pages_dir, _, _, _, _, _) = setup_test_project();

    fs::create_dir_all(pages_dir.join("docs")).expect("Failed to create docs dir");
    fs::write(pages_dir.join("docs/intro.hbs"), "hbs").expect("Failed to write hbs page");
    fs::write(pages_dir.join("docs/intro.html"), "html").expect("Failed to write html page");

    let resolved_config = nested_pages_config(&temp_path, None);
    let error = discover_static_pages(&resolved_config).expect_err("pages should conflict");
    let message = error.to_string();
    assert!(
        message.contains("intro.hbs") && message.contains("intro.html"),
        "{message}"
    );

    // `docs.hbs` and `docs/index.hbs` are both `/docs`
    fs::remove_file(pages_dir.join("docs/intro.html")).expect("Failed to remove html page");
    fs::write(pages_dir.join("docs.hbs"), "docs").expect("Failed to write docs page");
    fs::write(pages_dir.join("docs/index.hbs"), "index").expect("Failed to write docs index");
    let error = discover_static_pages(&resolved_config).expect_err("pages should conflict");
    let message = error.to_string();
    assert!(
        message.contains("docs.hbs") && message.contains("index.hbs"),
        "{message}"
    );
}

#[test]
fn test_sitemap_encodes_nested_page_urls() {
    let (_temp, temp_path, pages_dir, output_dir, _, _, _, _) = setup_test_project();

    fs::create_dir_all(pages_dir.join("docs/c#")).expect("Failed to create docs dir");
    fs::write(pages_dir.join("docs/c#/index.hbs"), "C#").expect("Failed to write page");
    fs::write(pages_dir.join("docs/café.hbs"), "Café").expect("Failed to write page");

    let resolved_config = nested_pages_config(&temp_path, Some("https://example.com"));
    build_full_site(&resolved_config);

    assert!(output_dir.join("docs/c#.html").exists());
    let sitemap = fs::read_to_string(output_dir.join("sitemap.xml")).expect("Missing sitemap");
    assert!(sitemap.contains("<loc>https://example.com/docs/c%23</loc>"));
    assert!(sitemap.contains("<loc>https://example.com/docs/caf%C3%A9</loc>"));
}

/// Builds a project from `balzac.toml` contents and the given files, then
/// discovers and renders its collections like `balzac build` does.
fn build_collections(
    toml: &str,
    files: &[(&str, &str)],
) -> (
    TempDir,
    balzac::error::Result<Vec<balzac::sitemap::PageEntry>>,
) {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let root = temp.path();
    for (path, content) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    let config: Config = toml::from_str(toml).expect("Failed to parse config");
    let resolved_config = config.resolve(root);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let pages = discover_collections(&resolved_config).and_then(|pages| {
        let mut renderer = HandlebarsRenderer::new(&resolved_config);
        renderer.init(&resolved_config)?;
        render_collection_items(&resolved_config, &pages, &renderer)?;
        Ok(pages)
    });
    (temp, pages)
}

const DETAILS: (&str, &str) = (
    "pages/posts/details.hbs",
    "<h1>{{fm.title}}</h1>{{{content}}}",
);

#[test]
fn test_collection_slug_overrides_filename() {
    let (temp, pages) = build_collections(
        "",
        &[
            DETAILS,
            (
                "content/posts/2024-01-01-hello.md",
                "---\ntitle: Hello\nslug: hello-world\n---\n\nBody",
            ),
            (
                "content/posts/v1.2.md",
                "---\nslug: release-1.2\n---\n\nBody",
            ),
        ],
    );
    let pages = pages.unwrap();

    let mut urls: Vec<_> = pages.iter().map(|p| p.url_path.as_str()).collect();
    urls.sort_unstable();
    assert_eq!(urls, vec!["/posts/hello-world", "/posts/release-1.2"]);
    let dist = temp.path().join("dist/posts");
    assert!(dist.join("hello-world.html").exists());
    assert!(dist.join("release-1.2.html").exists());
    assert!(!dist.join("2024-01-01-hello.html").exists());
}

#[test]
fn test_collection_rejects_invalid_slug() {
    for slug in [
        "\"\"",
        "",
        "null",
        "a/b",
        "..",
        "42",
        "a#b",
        "\"a?b\"",
        "a%20b",
        "a b",
        "\" hello \"",
    ] {
        let (_temp, pages) = build_collections(
            "",
            &[
                DETAILS,
                (
                    "content/posts/post.md",
                    &format!("---\nslug: {slug}\n---\n"),
                ),
            ],
        );
        let error = pages.expect_err(&format!("slug {slug} should be rejected"));
        assert!(error.to_string().contains("Invalid slug"), "{}", error);
    }
}

#[test]
fn test_collection_rejects_duplicate_output_paths() {
    let (_temp, pages) = build_collections(
        "",
        &[
            DETAILS,
            ("content/posts/a.md", "---\nslug: same\n---\n"),
            ("content/posts/same.md", "Body"),
        ],
    );
    let error = pages.expect_err("duplicate slugs should be rejected");
    assert!(error.to_string().contains("/posts/same"), "{}", error);
}

#[test]
fn test_collection_drafts_are_skipped_in_build() {
    let (temp, pages) = build_collections(
        "",
        &[
            DETAILS,
            ("content/posts/published.md", "---\ndraft: false\n---\n"),
            ("content/posts/wip.md", "---\ndraft: true\n---\n"),
        ],
    );
    let pages = pages.unwrap();

    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].url_path, "/posts/published");
    assert!(!temp.path().join("dist/posts/wip.html").exists());
}

#[test]
fn test_collection_drafts_are_rendered_in_dev_mode() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("pages/posts")).unwrap();
    fs::create_dir_all(root.join("content/posts")).unwrap();
    fs::write(root.join(DETAILS.0), DETAILS.1).unwrap();
    fs::write(root.join("content/posts/wip.md"), "---\ndraft: true\n---\n").unwrap();

    let mut resolved_config = Config::default().resolve(root);
    resolved_config.dev_mode = true;
    let pages = discover_collections(&resolved_config).unwrap();

    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].url_path, "/posts/wip");
    assert!(
        pages[0].sitemap_meta.exclude,
        "Drafts should stay out of the sitemap"
    );
}

#[test]
fn test_collection_custom_template() {
    let (temp, pages) = build_collections(
        "[collections.posts]\ntemplate = \"post.hbs\"\n",
        &[
            ("pages/posts/post.hbs", "<article>{{fm.title}}</article>"),
            ("content/posts/hello.md", "---\ntitle: Hello\n---\n"),
            // Collections without config keep using details.hbs
            ("pages/notes/details.hbs", "<aside>{{fm.title}}</aside>"),
            ("content/notes/note.md", "---\ntitle: Note\n---\n"),
        ],
    );
    assert_eq!(pages.unwrap().len(), 2);

    let dist = temp.path().join("dist");
    assert_eq!(
        fs::read_to_string(dist.join("posts/hello.html")).unwrap(),
        "<article>Hello</article>"
    );
    assert_eq!(
        fs::read_to_string(dist.join("notes/note.html")).unwrap(),
        "<aside>Note</aside>"
    );
}

#[test]
fn test_collection_without_configured_template_is_skipped() {
    let (_temp, pages) = build_collections(
        "[collections.posts]\ntemplate = \"post.hbs\"\n",
        &[DETAILS, ("content/posts/hello.md", "Body")],
    );
    assert!(pages.unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn test_collection_skips_symlinked_folders() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("pages/posts")).unwrap();
    fs::create_dir_all(root.join("content/posts")).unwrap();
    fs::write(root.join(DETAILS.0), DETAILS.1).unwrap();
    fs::write(root.join("content/posts/post.md"), "Body").unwrap();
    // A cycle back to the collection folder must not recurse forever
    std::os::unix::fs::symlink("..", root.join("content/posts/loop")).unwrap();

    let pages = discover_collections(&Config::default().resolve(root)).unwrap();
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].url_path, "/posts/post");
}

#[test]
fn test_collection_nested_folders() {
    let (temp, pages) = build_collections(
        "",
        &[
            DETAILS,
            ("content/posts/top.md", "---\ntitle: Top\n---\n"),
            (
                "content/posts/2024/jan/hello.md",
                "---\ntitle: Nested\n---\n",
            ),
            ("content/posts/2024/renamed.md", "---\nslug: custom\n---\n"),
        ],
    );
    let mut urls: Vec<_> = pages.unwrap().into_iter().map(|p| p.url_path).collect();
    urls.sort();
    assert_eq!(
        urls,
        vec!["/posts/2024/custom", "/posts/2024/jan/hello", "/posts/top"]
    );

    let dist = temp.path().join("dist/posts");
    assert_eq!(
        fs::read_to_string(dist.join("2024/jan/hello.html")).unwrap(),
        "<h1>Nested</h1>"
    );
    assert!(dist.join("2024/custom.html").exists());
}

#[test]
fn test_collections_context_uses_slug_and_nested_urls() {
    let (temp, pages) = build_collections(
        "",
        &[
            (
                "pages/posts/details.hbs",
                "{{#each collections.posts}}[{{slug}} {{url}}]{{/each}}",
            ),
            (
                "content/posts/2024-01-01-hello.md",
                "---\nslug: hello\n---\n",
            ),
            ("content/posts/2024/nested.md", "Body"),
        ],
    );
    pages.unwrap();

    assert_eq!(
        fs::read_to_string(temp.path().join("dist/posts/hello.html")).unwrap(),
        "[hello /posts/hello][nested /posts/2024/nested]"
    );
}

#[test]
fn test_collection_rejects_outputs_differing_only_in_case() {
    let (_temp, pages) = build_collections(
        "",
        &[
            DETAILS,
            ("content/posts/a.md", "---\nslug: Jan\n---\n"),
            ("content/posts/b.md", "---\nslug: jan\n---\n"),
        ],
    );
    let error = pages.expect_err("case-only duplicates should be rejected");
    assert!(error.to_string().contains("differ only in case"), "{error}");
}

#[test]
fn test_collection_rejects_unreachable_urls() {
    for (path, content) in [
        // `#` would make the rest of the URL a fragment
        ("content/posts/news#2024/hello.md", "Body"),
        ("content/posts/what?.md", "Body"),
        // Hosts decode `/posts/a%20b` to `a b`; browsers turn `\` into `/`
        ("content/posts/a%20b.md", "Body"),
        ("content/posts/a\\b.md", "Body"),
        // Hosts decode `/posts/a%20b/` to `a b`, missing the `a%20b` folder
        ("content/posts/a%20b/hello.md", "Body"),
        // A `jan.html/` folder can't sit next to the `jan.html` file of `jan.md`
        ("content/posts/jan.HTML/x.md", "Body"),
        // Browsers request `/posts/news/2024/hello` for `/posts/news\2024/hello`
        ("content/posts/news\\2024/hello.md", "Body"),
        // `/posts/article.html` would be served by the file of `/posts/article`
        ("content/posts/post.md", "---\nslug: article.HTML\n---\n"),
        ("content/posts/article.html.md", "Body"),
    ] {
        let (_temp, pages) = build_collections("", &[DETAILS, (path, content)]);
        let error = pages.expect_err(&format!("{path} should be rejected"));
        assert!(error.to_string().contains("Invalid URL"), "{path}: {error}");
    }
}

#[test]
fn test_collection_slug_and_nested_folders_with_directory_urls() {
    let (temp, pages) = build_collections(
        "url_style = \"directory\"\n",
        &[
            (
                "pages/posts/details.hbs",
                "{{#each collections.posts}}[{{slug}} {{url}}]{{/each}}",
            ),
            (
                "content/posts/2024-01-01-hello.md",
                "---\nslug: hello\n---\n",
            ),
            ("content/posts/2024/nested.md", "Body"),
        ],
    );
    let mut urls: Vec<_> = pages.unwrap().into_iter().map(|p| p.url_path).collect();
    urls.sort();
    assert_eq!(urls, vec!["/posts/2024/nested/", "/posts/hello/"]);

    let dist = temp.path().join("dist/posts");
    assert_eq!(
        fs::read_to_string(dist.join("hello/index.html")).unwrap(),
        "[hello /posts/hello/][nested /posts/2024/nested/]"
    );
    assert!(dist.join("2024/nested/index.html").exists());
    assert!(!dist.join("2024-01-01-hello").exists());
}

#[test]
fn test_custom_collection_template_error_names_template_and_source() {
    let (_temp, pages) = build_collections(
        "[collections.posts]\ntemplate = \"post.hbs\"",
        &[
            ("pages/posts/post.hbs", "{{#if}}"),
            ("content/posts/nested/item.md", "Body"),
        ],
    );
    let error = pages.unwrap_err();
    assert!(
        matches!(&error, balzac::error::Error::Template { path, page: Some(page), .. }
            if path.ends_with("pages/posts/post.hbs")
                && page.ends_with("content/posts/nested/item.md")),
        "{error}"
    );
}

#[test]
fn test_static_page_shadows_collection_item_by_slug_and_nested_path() {
    let (_temp, pages) = build_collections(
        "",
        &[
            DETAILS,
            ("pages/posts/hello.hbs", "<h1>Static</h1>"),
            ("pages/posts/2024/a.hbs", "<h1>Static</h1>"),
            ("content/posts/x.md", "---\nslug: hello\n---\n"),
            ("content/posts/2024/a.md", "---\ntitle: A\n---\n"),
            ("content/posts/kept.md", "---\ntitle: Kept\n---\n"),
        ],
    );
    let urls: Vec<_> = pages.unwrap().into_iter().map(|p| p.url_path).collect();
    assert_eq!(urls, vec!["/posts/kept"]);
}

#[test]
fn test_configured_collection_template_is_not_a_static_page() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let root = temp.path();
    for (path, content) in [
        ("pages/posts/templates/post.hbs", "<h1>{{fm.title}}</h1>"),
        ("pages/posts/details.hbs", "<h1>Details</h1>"),
        ("content/posts/a.md", "---\ntitle: A\n---\n"),
    ] {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    let config: Config =
        toml::from_str("[collections.posts]\ntemplate = \"templates/post.hbs\"\n").unwrap();
    let resolved_config = config.resolve(root);

    let urls: Vec<_> = discover_static_pages(&resolved_config)
        .unwrap()
        .into_iter()
        .map(|p| p.url_path)
        .collect();
    // `details.hbs` is not the template here, so it is a regular page
    assert_eq!(urls, vec!["/posts/details"]);
}

#[test]
fn test_template_helpers_in_pages_and_collections() {
    let (_temp, temp_path, pages_dir, output_dir, _, _, _, content_dir) = setup_test_project();

    let tags_dir = temp_path.join("tags");
    fs::create_dir(&tags_dir).expect("Failed to create tags dir");
    // tag templates rendered by the markdown helper can use helpers too
    fs::write(
        tags_dir.join("a.hbs"),
        r#"<a href="{{url_for props.href}}">{{{content}}}</a>"#,
    )
    .expect("Failed to write tag template");

    let blog_pages_dir = pages_dir.join("blog");
    let blog_content_dir = content_dir.join("blog");
    fs::create_dir(&blog_pages_dir).expect("Failed to create blog pages dir");
    fs::create_dir_all(&blog_content_dir).expect("Failed to create blog content dir");

    fs::write(
        blog_pages_dir.join("details.hbs"),
        r#"<time>{{date fm.date "%B %-d, %Y"}}</time><p>{{markdown fm.summary inline=true}}</p><link href="{{asset "site.css"}}">"#,
    )
    .expect("Failed to write details template");
    fs::write(
        pages_dir.join("index.hbs"),
        r#"{{#each (limit (sort collections.blog "fm.title") 1)}}<a href="{{url_for url}}">{{fm.title}}</a>{{/each}}|{{len (where collections.blog "fm.featured" true)}}|{{slugify "Hello World"}}"#,
    )
    .expect("Failed to write index page");

    fs::write(
        blog_content_dir.join("first.md"),
        "---\ntitle: Zebra\ndate: 2025-03-09\nsummary: See [the docs](/docs/)\n---\n\nBody.",
    )
    .expect("Failed to write post");
    fs::write(
        blog_content_dir.join("second.md"),
        "---\ntitle: Aardvark\ndate: 2024-12-25\nfeatured: true\n---\n\nBody.",
    )
    .expect("Failed to write post");

    let config_toml = format!(
        "output_directory = {:?}\npages_directory = {:?}\ncontent_directory = {:?}\ntags_directory = {:?}\nbase_url = \"https://example.com/site/\"\n",
        output_dir.to_string_lossy(),
        pages_dir.to_string_lossy(),
        content_dir.to_string_lossy(),
        tags_dir.to_string_lossy(),
    );
    let config: Config = toml::from_str(&config_toml).expect("Failed to parse config");
    let resolved_config = config.resolve(&temp_path);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    let site_pages = discover_site_pages(&resolved_config).expect("Failed to discover pages");
    let mut renderer = HandlebarsRenderer::new(&resolved_config);
    renderer
        .init(&resolved_config)
        .expect("Failed to init renderer");
    render_pages(&resolved_config, site_pages.all(), &renderer).expect("Failed to render pages");
    render_collection_items(&resolved_config, site_pages.all(), &renderer)
        .expect("Failed to render collections");

    let read =
        |path: &str| fs::read_to_string(output_dir.join(path)).expect("Failed to read output");
    assert_eq!(
        read("blog/first.html"),
        r#"<time>March 9, 2025</time><p>See <a href="/site/docs/">the docs</a></p><link href="/site/assets/site.css">"#
    );
    assert_eq!(
        read("index.html"),
        r#"<a href="/site/blog/second">Aardvark</a>|1|hello-world"#
    );
}
