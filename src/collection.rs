use std::{fs, path::PathBuf};

pub fn parse_markdown(path: PathBuf) -> std::io::Result<String> {
    let file_content = fs::read_to_string(path)?;

    let parsed_md = markdown::to_html_with_options(&file_content, &markdown::Options::gfm())
        .expect("Could not parse markdown file");
    Ok(parsed_md)
}
