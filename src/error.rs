use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors of a build. User-caused failures name the file they come from.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("could not read {}: {source}", display_path(path))]
    Read { path: PathBuf, source: io::Error },

    #[error("invalid config {}: {message}", display_path(path))]
    Config { path: PathBuf, message: String },

    #[error("invalid frontmatter in {}: {message}", display_path(path))]
    Frontmatter { path: PathBuf, message: String },

    #[error("{}", template_message(path, *line, *column, message, page.as_deref()))]
    Template {
        /// The template file the error is in.
        path: PathBuf,
        line: Option<usize>,
        column: Option<usize>,
        message: String,
        /// The page being rendered, when the error is in a partial, layout or
        /// tag template it uses.
        page: Option<PathBuf>,
    },

    #[error("invalid helper script {}: {message}", display_path(path))]
    Script { path: PathBuf, message: String },

    #[error("{0}")]
    Vite(String),

    #[error(transparent)]
    Hook(#[from] crate::hooks::HookError),
}

impl Error {
    /// Records that a template error happened while rendering `page`. Render
    /// calls nest (a page renders markdown, whose tag template uses a partial),
    /// so the outermost call, made last, names the page.
    #[must_use]
    pub fn in_page(mut self, page_path: &Path) -> Self {
        if let Error::Template { path, page, .. } = &mut self
            && path != page_path
        {
            *page = Some(page_path.to_path_buf());
        }
        self
    }

    /// Moves the line of a template error in `template_path` down by `lines`,
    /// for templates rendered without their first lines (e.g. frontmatter).
    #[must_use]
    pub fn offset_lines(mut self, template_path: &Path, lines: usize) -> Self {
        if let Error::Template {
            path,
            line: Some(line),
            ..
        } = &mut self
            && path == template_path
        {
            *line += lines;
        }
        self
    }
}

/// `path` relative to the working directory when it is inside it, without
/// `.` components: resolved project paths look like `/site/./pages/index.hbs`.
pub(crate) fn display_path(path: &Path) -> String {
    let path: PathBuf = path.components().collect();
    std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(cwd).ok().map(Path::to_path_buf))
        .unwrap_or(path)
        .display()
        .to_string()
}

fn template_message(
    path: &Path,
    line: Option<usize>,
    column: Option<usize>,
    message: &str,
    page: Option<&Path>,
) -> String {
    let mut location = display_path(path);
    if let Some(line) = line {
        let _ = write!(location, ":{line}");
        if let Some(column) = column {
            let _ = write!(location, ":{column}");
        }
    }
    match page {
        Some(page) => format!(
            "template error in {location} (rendering {}): {message}",
            display_path(page)
        ),
        None => format!("template error in {location}: {message}"),
    }
}

/// `fs::read_to_string` with the path in the error.
pub fn read_to_string(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template_error(line: Option<usize>, column: Option<usize>) -> Error {
        Error::Template {
            path: PathBuf::from("partials/header.hbs"),
            line,
            column,
            message: "boom".to_string(),
            page: None,
        }
    }

    #[test]
    fn test_template_error_message_includes_location() {
        assert_eq!(
            template_error(Some(3), Some(7)).to_string(),
            "template error in partials/header.hbs:3:7: boom"
        );
        assert_eq!(
            template_error(None, None).to_string(),
            "template error in partials/header.hbs: boom"
        );
    }

    #[test]
    fn test_in_page_records_the_rendered_page() {
        let error = template_error(Some(1), Some(2)).in_page(Path::new("pages/index.hbs"));
        assert_eq!(
            error.to_string(),
            "template error in partials/header.hbs:1:2 (rendering pages/index.hbs): boom"
        );

        let same_file = template_error(None, None).in_page(Path::new("partials/header.hbs"));
        assert!(matches!(same_file, Error::Template { page: None, .. }));

        let outer = template_error(None, None)
            .in_page(Path::new("tags/a.hbs"))
            .in_page(Path::new("content/posts/hello.md"));
        assert_eq!(
            outer.to_string(),
            "template error in partials/header.hbs (rendering content/posts/hello.md): boom"
        );
    }

    #[test]
    fn test_offset_lines_only_moves_errors_in_that_template() {
        let error =
            template_error(Some(2), Some(1)).offset_lines(Path::new("partials/header.hbs"), 3);
        assert!(matches!(error, Error::Template { line: Some(5), .. }));

        let other = template_error(Some(2), Some(1)).offset_lines(Path::new("pages/index.hbs"), 3);
        assert!(matches!(other, Error::Template { line: Some(2), .. }));
    }

    #[test]
    fn test_display_path_is_relative_to_working_directory() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            display_path(&cwd.join("./pages/index.hbs")),
            Path::new("pages/index.hbs").display().to_string()
        );
        assert_eq!(
            display_path(Path::new("/elsewhere/./a.md")),
            "/elsewhere/a.md"
        );
    }

    #[test]
    fn test_read_to_string_names_the_file() {
        let error = read_to_string(Path::new("does/not/exist.hbs")).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("could not read does/not/exist.hbs: ")
        );
    }
}
