use std::path::PathBuf;

use crate::config::{ResolvedConfig, UrlStyle};

/// Where a page lives: its public URL and the file it is written to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageLocation {
    pub url_path: String,
    pub output_path: PathBuf,
}

/// Single source of truth for mapping a page route to its URL and output file.
///
/// `route` is the page's logical path below the site root, one entry per URL
/// segment and without extension: `["about"]`, `["posts", "hello"]`. An empty
/// route is the site root (`/` -> `index.html`) in every style.
///
/// In `UrlStyle::Directory`, a top-level `404` page stays at `404.html` because
/// static hosts (GitHub Pages, S3, nginx) look for the error page there.
///
/// Returns `None` for an empty, `.` or `..` segment (e.g. the stem of
/// `pages/...hbs`), which would point outside the page's own directory.
#[must_use]
pub fn page_location(config: &ResolvedConfig, route: &[&str]) -> Option<PageLocation> {
    let output_root = &config.output_directory;
    let Some((last, parents)) = route.split_last() else {
        return Some(PageLocation {
            url_path: "/".to_string(),
            output_path: output_root.join("index.html"),
        });
    };
    if let Some(segment) = route
        .iter()
        .find(|segment| matches!(**segment, "" | "." | ".."))
    {
        log::warn!(
            "Skipping page {}: {segment:?} is not a valid URL segment",
            route.join("/")
        );
        return None;
    }

    let joined = route.join("/");
    let mut page_dir = output_root.clone();
    page_dir.extend(parents);

    Some(match config.url_style {
        UrlStyle::Extensionless => PageLocation {
            url_path: format!("/{joined}"),
            output_path: page_dir.join(format!("{last}.html")),
        },
        UrlStyle::Directory if route == ["404"] => PageLocation {
            url_path: "/404.html".to_string(),
            output_path: output_root.join("404.html"),
        },
        UrlStyle::Directory => PageLocation {
            url_path: format!("/{joined}/"),
            output_path: page_dir.join(last).join("index.html"),
        },
    })
}

/// Decodes `%XX` escapes (e.g. non-ASCII taxonomy terms); `None` when malformed.
#[must_use]
pub fn percent_decode(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes.get(index + 1..index + 3)?;
            if !hex.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            decoded.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

/// Splits `base_url` into its origin and its path without trailing slash:
/// `https://example.com/blog/` -> (`https://example.com`, `/blog`). The origin
/// is empty when `base_url` has no scheme.
#[must_use]
pub fn split_base_url(base_url: &str) -> (&str, &str) {
    let base = base_url.trim_end_matches('/');
    let path_start = base.split_once("://").map_or(0, |(scheme, rest)| {
        scheme.len() + "://".len() + rest.find('/').unwrap_or(rest.len())
    });
    base.split_at(path_start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::path::Path;

    fn config_with(url_style: UrlStyle) -> ResolvedConfig {
        Config {
            url_style,
            ..Config::default()
        }
        .resolve(Path::new("/site"))
    }

    fn location(url_style: UrlStyle, route: &[&str]) -> (String, PathBuf) {
        let location = page_location(&config_with(url_style), route).unwrap();
        (location.url_path, location.output_path)
    }

    #[test]
    fn test_rejects_segments_leaving_the_page_directory() {
        for style in [UrlStyle::Extensionless, UrlStyle::Directory] {
            let config = config_with(style);
            for route in [&[".."][..], &["."], &[""], &["posts", ".."], &["..", "x"]] {
                assert_eq!(page_location(&config, route), None, "{style:?} {route:?}");
            }
            assert!(page_location(&config, &["...en"]).is_some());
        }
    }

    #[test]
    fn test_root_is_index_in_every_style() {
        for style in [UrlStyle::Extensionless, UrlStyle::Directory] {
            assert_eq!(
                location(style, &[]),
                ("/".to_string(), PathBuf::from("/site/dist/index.html"))
            );
        }
    }

    #[test]
    fn test_extensionless_style() {
        assert_eq!(
            location(UrlStyle::Extensionless, &["about"]),
            ("/about".to_string(), PathBuf::from("/site/dist/about.html"))
        );
        assert_eq!(
            location(UrlStyle::Extensionless, &["posts", "hello"]),
            (
                "/posts/hello".to_string(),
                PathBuf::from("/site/dist/posts/hello.html")
            )
        );
        assert_eq!(
            location(UrlStyle::Extensionless, &["404"]),
            ("/404".to_string(), PathBuf::from("/site/dist/404.html"))
        );
    }

    #[test]
    fn test_directory_style() {
        assert_eq!(
            location(UrlStyle::Directory, &["about"]),
            (
                "/about/".to_string(),
                PathBuf::from("/site/dist/about/index.html")
            )
        );
        assert_eq!(
            location(UrlStyle::Directory, &["posts", "hello"]),
            (
                "/posts/hello/".to_string(),
                PathBuf::from("/site/dist/posts/hello/index.html")
            )
        );
    }

    #[test]
    fn test_directory_style_keeps_404_page_at_root() {
        assert_eq!(
            location(UrlStyle::Directory, &["404"]),
            (
                "/404.html".to_string(),
                PathBuf::from("/site/dist/404.html")
            )
        );
        assert_eq!(
            location(UrlStyle::Directory, &["posts", "404"]),
            (
                "/posts/404/".to_string(),
                PathBuf::from("/site/dist/posts/404/index.html")
            )
        );
    }

    #[test]
    fn test_split_base_url() {
        assert_eq!(
            split_base_url("https://example.com/blog/"),
            ("https://example.com", "/blog")
        );
        assert_eq!(
            split_base_url("https://example.com"),
            ("https://example.com", "")
        );
        assert_eq!(
            split_base_url("https://example.com/"),
            ("https://example.com", "")
        );
        assert_eq!(split_base_url("/docs/"), ("", "/docs"));
    }

    #[test]
    fn test_url_style_parses_from_toml() {
        let parsed: Config = toml::from_str(r#"url_style = "directory""#).unwrap();
        assert_eq!(parsed.url_style, UrlStyle::Directory);

        let parsed: Config = toml::from_str("").unwrap();
        assert_eq!(parsed.url_style, UrlStyle::Extensionless);

        assert!(toml::from_str::<Config>(r#"url_style = "bogus""#).is_err());
    }
}
