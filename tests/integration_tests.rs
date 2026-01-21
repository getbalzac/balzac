use balzac::renderer::{HandlebarsRenderer, Renderer};
use balzac::sitemap::SitePages;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

// Import from the main crate
use balzac::config::{Config, SitemapConfig};
use balzac::{
    discover_collections, discover_static_pages, make_dist_folder, render_collection_items,
    render_pages, write_sitemap,
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
        global: None,
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
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
        _temp_path,
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
        global: None,
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
    };

    let resolved_config = config.resolve(&_temp_path);
    let mut renderer = HandlebarsRenderer::new(&resolved_config);
    renderer.init(&resolved_config);

    assert!(
        renderer.registry.get_templates().len() == 0,
        "Could not register partial alert"
    );
}

#[test]
fn test_full_workflow_single_page() {
    let (
        _temp,
        _temp_path,
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
        global: None,
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
    };

    let resolved_config = config.resolve(&_temp_path);
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
        _temp_path,
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
        global: None,
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
    };

    let resolved_config = config.resolve(&_temp_path);
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
        _temp_path,
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
        global: Some(global),
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
    };

    let resolved_config = config.resolve(&_temp_path);
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
        global: None,
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
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
        _temp_path,
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
        global: None,
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
    };

    let resolved_config = config.resolve(&_temp_path);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");

    // Directory should exist but be empty
    assert!(output_dir.exists());
    assert!(!output_dir.join("old_file.txt").exists());
}

#[test]
fn test_workflow_preserves_file_extensions() {
    let (
        _temp,
        _temp_path,
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
        global: None,
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
    };

    let resolved_config = config.resolve(&_temp_path);
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
        _temp_path,
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
        global: Some(global),
        hooks: None,
        bundler: None,
        base_url: None,
        sitemap: None,
    };

    let resolved_config = config.resolve(&_temp_path);
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
        _temp_path,
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
    };

    let resolved_config = config.resolve(&_temp_path);

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
        global: None,
        hooks: None,
        bundler: None,
        base_url: Some("https://example.com".to_string()),
        sitemap: Some(SitemapConfig::default()),
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
        global: None,
        hooks: None,
        bundler: None,
        base_url: None, // No base_url configured
        sitemap: None,
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
