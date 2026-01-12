use balzac::renderer::{HandlebarsRenderer, Renderer};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

// Import from the main crate
use balzac::config::Config;
use balzac::{make_dist_folder, render_static_pages};

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
    };

    let resolved_config = config.resolve(&_temp_path);
    // Run the workflow
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");
    render_static_pages(&resolved_config, &HandlebarsRenderer::new(&resolved_config))
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
    };

    let resolved_config = config.resolve(&_temp_path);
    // Run the workflow
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");
    render_static_pages(&resolved_config, &HandlebarsRenderer::new(&resolved_config))
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
    };

    let resolved_config = config.resolve(&_temp_path);
    // Run the workflow
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");
    render_static_pages(&resolved_config, &HandlebarsRenderer::new(&resolved_config))
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
    };

    let resolved_config = config.resolve(&_temp_path);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");
    render_static_pages(&resolved_config, &HandlebarsRenderer::new(&resolved_config))
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
    };

    let resolved_config = config.resolve(&_temp_path);
    make_dist_folder(&resolved_config).expect("Failed to make dist folder");
    render_static_pages(&resolved_config, &HandlebarsRenderer::new(&resolved_config))
        .expect("Failed to render static pages");

    let output_content =
        fs::read_to_string(output_dir.join("index.html")).expect("Failed to read output");
    assert_eq!(output_content, "<p>Welcome!</p>");
}
