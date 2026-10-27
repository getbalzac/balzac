//! `balzac check`: finds links and asset references in the output directory
//! that point to files the site does not publish.
//!
//! Every `.html` and `.css` file of the output directory is scanned. URLs are
//! resolved the way a static host serves the output (see [`Site::locate`]),
//! against the list of output files rather than the file system, so case
//! mismatches are caught on case-insensitive file systems too. External URLs
//! are never fetched.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use rayon::prelude::*;

use crate::config::{ResolvedConfig, UrlStyle};
use crate::url::{percent_decode, split_base_url};

#[derive(Debug, Clone, Copy)]
pub struct CheckOptions {
    /// Check that `#fragment`s of links to HTML pages match an element id.
    pub fragments: bool,
}

/// Outcome of a check.
#[derive(Debug, Default)]
pub struct Report {
    /// Number of HTML and CSS files scanned.
    pub files: usize,
    /// Number of internal URLs checked.
    pub links: usize,
    /// Number of external URLs skipped.
    pub external: usize,
    /// Broken URLs, ordered by file and line.
    pub broken: Vec<BrokenLink>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokenLink {
    /// File containing the URL, relative to the output directory, `/`-separated.
    pub file: String,
    /// 1-based line of the URL in `file`.
    pub line: usize,
    /// The URL as written, with HTML entities decoded.
    pub url: String,
    pub reason: Reason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// No output file is served at `path`, a URL path including the `base_url` path.
    NotFound { path: String },
    /// A root-relative or same-origin URL not under the `base_url` path.
    OutsideBasePath { base_path: String },
    /// A relative URL with more `..` than the page is deep.
    AboveRoot,
    /// The target page has no element with this id.
    MissingFragment { fragment: String, file: String },
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { path } => write!(f, "{path} not found"),
            Self::OutsideBasePath { base_path } => {
                write!(f, "outside the base_url path {base_path}/")
            }
            Self::AboveRoot => write!(f, "goes above the site root"),
            Self::MissingFragment { fragment, file } => {
                write!(f, "no id \"{fragment}\" in {file}")
            }
        }
    }
}

/// Scans the output directory of `config` for broken links.
pub fn check(config: &ResolvedConfig, options: CheckOptions) -> io::Result<Report> {
    let mut files = Vec::new();
    collect_files(&config.output_directory, "", &mut files)?;

    let documents = files
        .par_iter()
        .filter_map(|file| {
            let kind = DocumentKind::of(file)?;
            Some(
                fs::read(config.output_directory.join(file)).map(|contents| {
                    let contents = String::from_utf8_lossy(&contents);
                    let document = match kind {
                        DocumentKind::Html => scan_html(&contents),
                        DocumentKind::Css => scan_css(&contents, 0, &LineIndex::new(&contents)),
                    };
                    (file.as_str(), kind, document)
                }),
            )
        })
        .collect::<io::Result<Vec<_>>>()?;

    let (origin, base_path) = config.base_url.as_deref().map_or(("", ""), split_base_url);
    let site = Site {
        origin,
        base_path,
        url_style: config.url_style,
        files: files.iter().map(String::as_str).collect(),
        ids: documents
            .iter()
            .filter(|(_, kind, _)| *kind == DocumentKind::Html)
            .map(|(file, _, document)| (*file, &document.ids))
            .collect(),
        fragments: options.fragments,
    };

    let mut report = Report {
        files: documents.len(),
        ..Report::default()
    };
    for (file, kind, document) in &documents {
        site.check_document(file, *kind, document, &mut report);
    }
    Ok(report)
}

/// Appends the files below `directory` to `files`, as `/`-separated paths
/// prefixed with `prefix`, in a stable order.
fn collect_files(directory: &Path, prefix: &str, files: &mut Vec<String>) -> io::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<io::Result<Vec<_>>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = format!("{prefix}{}", entry.file_name().to_string_lossy());
        if entry.path().is_dir() {
            collect_files(&entry.path(), &format!("{path}/"), files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocumentKind {
    Html,
    Css,
}

impl DocumentKind {
    /// The kind of `file` from its extension, `None` for files not scanned.
    fn of(file: &str) -> Option<Self> {
        let extension = Path::new(file).extension()?;
        if extension.eq_ignore_ascii_case("html") {
            Some(Self::Html)
        } else if extension.eq_ignore_ascii_case("css") {
            Some(Self::Css)
        } else {
            None
        }
    }
}

/// URLs and element ids found in an HTML or CSS file.
#[derive(Debug, Default)]
struct Document {
    references: Vec<Reference>,
    ids: HashSet<String>,
    /// `href` of the first `<base>` element.
    base_href: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct Reference {
    line: usize,
    url: String,
}

/// The published site, as needed to resolve URLs.
struct Site<'a> {
    /// `https://example.com` for `base_url = "https://example.com/blog/"`, or empty.
    origin: &'a str,
    /// `/blog` for `base_url = "https://example.com/blog/"`, or empty.
    base_path: &'a str,
    url_style: UrlStyle,
    files: HashSet<&'a str>,
    ids: HashMap<&'a str, &'a HashSet<String>>,
    fragments: bool,
}

/// What URLs without origin in a document resolve against.
enum Base {
    /// The document itself.
    Document,
    /// The `<base href>` of the document, a path below the site root as
    /// returned by [`Site::resolve`], e.g. `docs/` or `docs/intro`.
    Path(String),
    /// A `<base href>` outside the site: only absolute URLs are checked.
    External,
}

/// Where a URL points to, before looking for the file.
#[derive(Debug, PartialEq, Eq)]
enum Target<'u> {
    /// Another site (or a `mailto:`, `data:`, ... URL), not checked.
    External,
    /// The page itself (`#intro`, `?page=2`, or an empty URL).
    SamePage { fragment: Option<&'u str> },
    /// A path below the site root, without leading slash, e.g. `posts/hello`
    /// or `posts/` (a directory URL).
    Path {
        path: String,
        fragment: Option<&'u str>,
    },
}

impl Site<'_> {
    fn check_document(
        &self,
        file: &str,
        kind: DocumentKind,
        document: &Document,
        report: &mut Report,
    ) {
        let base = match &document.base_href {
            None => Base::Document,
            Some(href) => match self.resolve(href, directory_of(file)) {
                Ok(Target::Path { path, .. }) => Base::Path(path),
                Ok(Target::SamePage { .. }) => Base::Document,
                Ok(Target::External) | Err(_) => Base::External,
            },
        };

        for reference in &document.references {
            let Some(result) = self.check_url(file, kind, &base, &reference.url) else {
                report.external += 1;
                continue;
            };
            report.links += 1;
            if let Err(reason) = result {
                report.broken.push(BrokenLink {
                    file: file.to_string(),
                    line: reference.line,
                    url: reference.url.clone(),
                    reason,
                });
            }
        }
    }

    /// Checks `url`, found in `file`, whose URLs without origin resolve
    /// against `base`. `None` for external URLs.
    fn check_url(
        &self,
        file: &str,
        kind: DocumentKind,
        base: &Base,
        url: &str,
    ) -> Option<Result<(), Reason>> {
        let resolved = match base {
            Base::Document => self.resolve(url, directory_of(file)),
            Base::External if !is_absolute(url) => return None,
            Base::External => self.resolve(url, ""),
            Base::Path(base) => self
                .resolve(url, directory_of(base))
                .map(|target| match target {
                    // `#id`, `?query` and empty URLs point to the base document.
                    Target::SamePage { fragment } => Target::Path {
                        path: base.clone(),
                        fragment,
                    },
                    target => target,
                }),
        };
        let (target_file, fragment) = match resolved {
            Err(reason) => return Some(Err(reason)),
            Ok(Target::External) => return None,
            // In a stylesheet, `url(#id)` refers to the document using it.
            Ok(Target::SamePage { .. }) if kind == DocumentKind::Css => return Some(Ok(())),
            Ok(Target::SamePage { fragment }) => (file, fragment),
            Ok(Target::Path { path, fragment }) => {
                let Some(target_file) = self.locate(&path) else {
                    let path = format!("{}/{path}", self.base_path);
                    return Some(Err(Reason::NotFound { path }));
                };
                (target_file, fragment)
            }
        };
        Some(
            fragment
                .and_then(|fragment| self.check_fragment(target_file, fragment))
                .map_or(Ok(()), Err),
        )
    }

    /// Resolves `url`, found in a page whose URL is in `directory` (a path
    /// below the site root ending with `/`, or empty for the root).
    fn resolve<'u>(&self, url: &'u str, directory: &str) -> Result<Target<'u>, Reason> {
        let url = url.trim_matches(|c: char| c.is_ascii_whitespace());
        let (url, fragment) = match url.split_once('#') {
            Some((url, fragment)) => (url, Some(fragment)),
            None => (url, None),
        };
        let url = url.split_once('?').map_or(url, |(path, _)| path);

        if url.starts_with("//") || crate::helpers::is_external(url) {
            return match self.strip_origin(url) {
                Some(path) => self.resolve_root_relative(path, fragment),
                None => Ok(Target::External),
            };
        }
        if url.is_empty() {
            return Ok(Target::SamePage { fragment });
        }
        if url.starts_with('/') {
            return self.resolve_root_relative(url, fragment);
        }
        Ok(Target::Path {
            path: normalize(&format!("{directory}{url}")).ok_or(Reason::AboveRoot)?,
            fragment,
        })
    }

    /// `/blog/posts/../about/` -> `about/` with `base_url` path `/blog`.
    fn resolve_root_relative<'u>(
        &self,
        url: &str,
        fragment: Option<&'u str>,
    ) -> Result<Target<'u>, Reason> {
        let below_base = if self.base_path.is_empty() {
            Some(url)
        } else {
            url.strip_prefix(self.base_path)
                .filter(|rest| rest.is_empty() || rest.starts_with('/'))
        };
        let outside = || Reason::OutsideBasePath {
            base_path: self.base_path.to_string(),
        };
        let path = below_base.ok_or_else(outside)?;
        Ok(Target::Path {
            // `..` above the base path leaves the site.
            path: normalize(path.trim_start_matches('/')).ok_or_else(outside)?,
            fragment,
        })
    }

    /// The path of `url` if it is on the origin of `base_url`, e.g. `/blog/`
    /// for `https://example.com/blog/` or `//example.com/blog/`.
    fn strip_origin<'u>(&self, url: &'u str) -> Option<&'u str> {
        if self.origin.is_empty() {
            return None;
        }
        let scheme_relative = self
            .origin
            .find("//")
            .map_or(self.origin, |start| &self.origin[start..]);
        [self.origin, scheme_relative]
            .into_iter()
            .find_map(|origin| {
                let prefix = url.get(..origin.len())?;
                let rest = &url[origin.len()..];
                (prefix.eq_ignore_ascii_case(origin) && (rest.is_empty() || rest.starts_with('/')))
                    .then_some(if rest.is_empty() { "/" } else { rest })
            })
    }

    /// The output file a static host serves for `path`, a path below the
    /// site root as returned by [`Self::resolve`]:
    ///
    /// - `posts/` (and the root) are served from `posts/index.html` only;
    /// - `posts` from the file `posts`, or from `posts/index.html` (hosts
    ///   redirect to `posts/`), or from `posts.html` with `url_style = "extensionless"`.
    ///
    /// The percent-decoded path is tried first, then the raw one, like in `balzac dev`.
    fn locate(&self, path: &str) -> Option<&str> {
        let decoded = percent_decode(path).filter(|decoded| decoded != path);
        decoded
            .iter()
            .map(String::as_str)
            .chain([path])
            .find_map(|path| {
                let mut candidates = Vec::with_capacity(3);
                if path.is_empty() || path.ends_with('/') {
                    candidates.push(format!("{path}index.html"));
                } else {
                    candidates.push(path.to_string());
                    candidates.push(format!("{path}/index.html"));
                    if self.url_style == UrlStyle::Extensionless {
                        candidates.push(format!("{path}.html"));
                    }
                }
                candidates
                    .iter()
                    .find_map(|candidate| self.files.get(candidate.as_str()).copied())
            })
    }

    /// Checks that `fragment` matches an element id of `file` if it is a page.
    fn check_fragment(&self, file: &str, fragment: &str) -> Option<Reason> {
        // `#` and `#top` scroll to the top of any page, `#:~:text=` is a text fragment.
        if !self.fragments
            || fragment.is_empty()
            || fragment.eq_ignore_ascii_case("top")
            || fragment.starts_with(":~:")
        {
            return None;
        }
        let ids = self.ids.get(file)?;
        let found = ids.contains(fragment)
            || percent_decode(fragment).is_some_and(|decoded| ids.contains(&decoded));
        (!found).then(|| Reason::MissingFragment {
            fragment: fragment.to_string(),
            file: file.to_string(),
        })
    }
}

/// Whether `url` has an origin (`https://...`, `//host/...`, `mailto:...`),
/// so does not depend on `<base href>`.
fn is_absolute(url: &str) -> bool {
    let url = url.trim_matches(|c: char| c.is_ascii_whitespace());
    url.starts_with("//") || crate::helpers::is_external(url)
}

/// Directory of the URL of the page at `path`, a path below the site root:
/// `posts/hello` -> `posts/`. Relative URLs resolve against it, and it is
/// also the directory of the page's output file in every `url_style`.
fn directory_of(path: &str) -> &str {
    path.rfind('/').map_or("", |slash| &path[..=slash])
}

/// Removes `.` and `..` segments from a `/`-separated path below the site
/// root, keeping a trailing slash; `None` when it goes above the root.
fn normalize(path: &str) -> Option<String> {
    let mut segments: Vec<&str> = Vec::new();
    let mut directory = path.is_empty();
    for segment in path.split('/') {
        directory = matches!(segment, "" | "." | "..");
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            segment => segments.push(segment),
        }
    }
    let mut normalized = segments.join("/");
    if directory && !normalized.is_empty() {
        normalized.push('/');
    }
    Some(normalized)
}

/// Line numbers of byte offsets.
struct LineIndex {
    newlines: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        Self {
            newlines: text.match_indices('\n').map(|(index, _)| index).collect(),
        }
    }

    fn line(&self, offset: usize) -> usize {
        self.newlines.partition_point(|&newline| newline < offset) + 1
    }
}

/// Elements whose contents are text rather than markup.
const RAW_TEXT_ELEMENTS: [&str; 5] = ["script", "style", "textarea", "title", "xmp"];

/// Collects the URLs of `href`, `src`, `srcset` and `poster` attributes on
/// any element, `data` on `<object>`, `url(...)` in `style` attributes and
/// `<style>` elements, and the `id`s (and `<a name>`s) of all elements.
fn scan_html(html: &str) -> Document {
    let lines = LineIndex::new(html);
    let mut document = Document::default();
    let mut position = 0;
    while let Some(start) = html[position..].find('<').map(|offset| position + offset) {
        let rest = &html[start..];
        position = if let Some(comment) = rest.strip_prefix("<!--") {
            comment
                .find("-->")
                .map_or(html.len(), |end| start + "<!--".len() + end + "-->".len())
        } else if rest.starts_with("<!") || rest.starts_with("<?") || rest.starts_with("</") {
            rest.find('>').map_or(html.len(), |end| start + end + 1)
        } else if rest[1..].starts_with(|c: char| c.is_ascii_alphabetic()) {
            let (tag, end) = parse_tag(html, start + 1);
            collect_tag(&tag, &lines, &mut document);
            match RAW_TEXT_ELEMENTS
                .iter()
                .find(|name| tag.name.eq_ignore_ascii_case(name))
            {
                Some(name) => {
                    let close = find_ignore_ascii_case(&html[end..], &format!("</{name}"))
                        .map_or(html.len(), |offset| end + offset);
                    if *name == "style" {
                        let css = scan_css(&html[end..close], end, &lines);
                        document.references.extend(css.references);
                    }
                    close
                }
                None => end,
            }
        } else {
            start + 1
        };
    }
    document
}

fn collect_tag(tag: &Tag<'_>, lines: &LineIndex, document: &mut Document) {
    let is = |name: &str| tag.name.eq_ignore_ascii_case(name);
    for attribute in &tag.attributes {
        let line = lines.line(attribute.offset);
        let name = attribute.name;
        let value = &attribute.value;
        let mut push = |url: &str| {
            document.references.push(Reference {
                line,
                url: url.to_string(),
            });
        };
        if name.eq_ignore_ascii_case("id") || (is("a") && name.eq_ignore_ascii_case("name")) {
            document.ids.insert(value.to_string());
        } else if is("base") && name.eq_ignore_ascii_case("href") {
            document.base_href.get_or_insert_with(|| value.to_string());
        } else if ["href", "xlink:href", "src", "poster"]
            .iter()
            .any(|url_attribute| name.eq_ignore_ascii_case(url_attribute))
            || (is("object") && name.eq_ignore_ascii_case("data"))
        {
            push(value);
        } else if name.eq_ignore_ascii_case("srcset") {
            srcset_urls(value).into_iter().for_each(push);
        } else if name.eq_ignore_ascii_case("style") {
            for (_, url) in crate::fingerprint::url_references(value) {
                push(url);
            }
        }
    }
}

/// The URLs of a `srcset`: `a.png 1x, b.png 2x` -> `a.png`, `b.png`.
fn srcset_urls(srcset: &str) -> Vec<&str> {
    let mut urls = Vec::new();
    let mut rest = srcset;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == ',');
        if rest.is_empty() {
            return urls;
        }
        let end = rest
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(rest.len());
        let candidate = &rest[..end];
        let url = candidate.trim_end_matches(',');
        urls.push(url);
        // Skip the descriptors (`2x`, `640w`) unless the URL ended the candidate.
        rest = if url.len() < candidate.len() {
            &rest[end..]
        } else {
            rest[end..]
                .find(',')
                .map_or("", |comma| &rest[end + comma + 1..])
        };
    }
}

/// `url(...)` references of `css`, which starts at byte `offset` of the file
/// `lines` indexes. Comments are skipped.
fn scan_css(css: &str, offset: usize, lines: &LineIndex) -> Document {
    // Blank out comments, keeping offsets.
    let mut uncommented = String::new();
    let css = if css.contains("/*") {
        let mut rest = css;
        while let Some(start) = rest.find("/*") {
            uncommented.push_str(&rest[..start]);
            let end = rest[start + 2..]
                .find("*/")
                .map_or(rest.len(), |end| start + 2 + end + 2);
            uncommented.extend(
                rest[start..end]
                    .bytes()
                    .map(|byte| if byte == b'\n' { '\n' } else { ' ' }),
            );
            rest = &rest[end..];
        }
        uncommented.push_str(rest);
        uncommented.as_str()
    } else {
        css
    };
    Document {
        references: crate::fingerprint::url_references(css)
            .into_iter()
            .map(|(position, url)| Reference {
                line: lines.line(offset + position),
                url: url.to_string(),
            })
            .collect(),
        ..Document::default()
    }
}

struct Tag<'a> {
    name: &'a str,
    attributes: Vec<Attribute<'a>>,
}

struct Attribute<'a> {
    name: &'a str,
    /// Value with character references decoded.
    value: Cow<'a, str>,
    /// Byte offset of the value (or of the name, without value) in the document.
    offset: usize,
}

/// Parses the start tag whose name begins at byte `position` (after `<`),
/// returning it and the offset just after its `>`.
fn parse_tag(html: &str, mut position: usize) -> (Tag<'_>, usize) {
    // Only ASCII bytes are compared, so every slice falls on a char boundary.
    let bytes = html.as_bytes();
    let is_space = |byte: u8| byte.is_ascii_whitespace();
    let ends_name = |byte: u8| is_space(byte) || matches!(byte, b'/' | b'>');
    let skip = |position: &mut usize, condition: &dyn Fn(u8) -> bool| {
        while *position < bytes.len() && condition(bytes[*position]) {
            *position += 1;
        }
    };

    let name_start = position;
    skip(&mut position, &|byte| !ends_name(byte));
    let mut tag = Tag {
        name: &html[name_start..position],
        attributes: Vec::new(),
    };
    loop {
        skip(&mut position, &|byte| is_space(byte) || byte == b'/');
        match bytes.get(position) {
            None => return (tag, html.len()),
            Some(b'>') => return (tag, position + 1),
            Some(_) => {}
        }
        let name_start = position;
        // A leading `=` is part of the name.
        position += 1;
        skip(&mut position, &|byte| !ends_name(byte) && byte != b'=');
        let name = &html[name_start..position];
        skip(&mut position, &is_space);
        if bytes.get(position) != Some(&b'=') {
            tag.attributes.push(Attribute {
                name,
                value: Cow::Borrowed(""),
                offset: name_start,
            });
            continue;
        }
        position += 1;
        skip(&mut position, &is_space);
        let (value_start, value_end) = if let Some(&quote @ (b'"' | b'\'')) = bytes.get(position) {
            let start = position + 1;
            let end = html[start..]
                .find(char::from(quote))
                .map_or(html.len(), |end| start + end);
            position = (end + 1).min(html.len());
            (start, end)
        } else {
            let start = position;
            skip(&mut position, &|byte| !is_space(byte) && byte != b'>');
            (start, position)
        };
        tag.attributes.push(Attribute {
            name,
            value: decode_character_references(&html[value_start..value_end]),
            offset: value_start,
        });
    }
}

fn find_ignore_ascii_case(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Decodes numeric character references and the named ones templates
/// produce (`&amp;`, `&quot;`, ...); others are kept as written.
fn decode_character_references(value: &str) -> Cow<'_, str> {
    if !value.contains('&') {
        return Cow::Borrowed(value);
    }
    let mut decoded = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(ampersand) = rest.find('&') {
        decoded.push_str(&rest[..ampersand]);
        rest = &rest[ampersand..];
        if let Some((character, length)) = character_reference(rest) {
            decoded.push(character);
            rest = &rest[length..];
        } else {
            decoded.push('&');
            rest = &rest[1..];
        }
    }
    decoded.push_str(rest);
    Cow::Owned(decoded)
}

/// The character `reference` (`&amp;...`) starts with, and its length.
fn character_reference(reference: &str) -> Option<(char, usize)> {
    let end = reference.bytes().take(12).position(|byte| byte == b';')?;
    let name = &reference[1..end];
    let character = if let Some(number) = name.strip_prefix('#') {
        let (digits, radix) = number
            .strip_prefix(['x', 'X'])
            .map_or((number, 10), |hex| (hex, 16));
        char::from_u32(u32::from_str_radix(digits, radix).ok()?)?
    } else {
        match name {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            "nbsp" => '\u{a0}',
            _ => return None,
        }
    };
    Some((character, end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(document: &Document) -> Vec<(usize, &str)> {
        document
            .references
            .iter()
            .map(|reference| (reference.line, reference.url.as_str()))
            .collect()
    }

    fn site<'a>(files: &[&'a str], url_style: UrlStyle, base_url: &'a str) -> Site<'a> {
        let (origin, base_path) = split_base_url(base_url);
        Site {
            origin,
            base_path,
            url_style,
            files: files.iter().copied().collect(),
            ids: HashMap::new(),
            fragments: true,
        }
    }

    fn path(path: &str, fragment: Option<&'static str>) -> Target<'static> {
        Target::Path {
            path: path.to_string(),
            fragment,
        }
    }

    #[test]
    fn test_scan_html_collects_url_attributes() {
        let html = r#"<!DOCTYPE html>
<html><head>
<link rel="stylesheet" href="/assets/site.css">
<script src='/app.js'>if (a<b) { x = "<a href='/no'>" }</script>
<style>body { background: url("/bg.png") }</style>
<title><a href="/no"></title>
</head><body>
<!-- <a href="/commented"> -->
<a HREF=/about class=x>About</a>
<img src="a.png" srcset="b.png 2x, c.png 3x" alt="">
<video poster="poster.jpg"><source src="v.mp4"></video>
<object data="doc.pdf"></object>
<div data="not-a-url" style="background-image: url(d.png)"></div>
<svg><use xlink:href="sprite.svg#icon"></use></svg>
<a href="/search?q=a&amp;b=c&#x3D;">Search</a>
</body></html>"#;
        let document = scan_html(html);
        assert_eq!(
            urls(&document),
            [
                (3, "/assets/site.css"),
                (4, "/app.js"),
                (5, "/bg.png"),
                (9, "/about"),
                (10, "a.png"),
                (10, "b.png"),
                (10, "c.png"),
                (11, "poster.jpg"),
                (11, "v.mp4"),
                (12, "doc.pdf"),
                (13, "d.png"),
                (14, "sprite.svg#icon"),
                (15, "/search?q=a&b=c="),
            ]
        );
    }

    #[test]
    fn test_scan_html_collects_ids_and_base() {
        let document = scan_html(
            r#"<base href="/docs/"><base href="/other/"><h2 id="intro">Intro</h2><a name="old"></a><p name="no">"#,
        );
        let mut ids: Vec<_> = document.ids.iter().map(String::as_str).collect();
        ids.sort_unstable();
        assert_eq!(ids, ["intro", "old"]);
        assert_eq!(document.base_href.as_deref(), Some("/docs/"));
        assert_eq!(document.references, []);
    }

    #[test]
    fn test_scan_html_tolerates_malformed_markup() {
        for html in [
            "<a href=\"/x",
            "<a href=",
            "<a",
            "<",
            "<!--",
            "<é href=/x>",
            "<a é=\"/x\" href=/y>",
        ] {
            scan_html(html);
        }
        assert_eq!(urls(&scan_html("<a é=\"/x\" href=/y>")), [(1, "/y")]);
    }

    #[test]
    fn test_scan_css_skips_comments() {
        let css = "/* url(/old.png)\n*/\na { background: url('/new.png') }";
        assert_eq!(
            urls(&scan_css(css, 0, &LineIndex::new(css))),
            [(3, "/new.png")]
        );
    }

    #[test]
    fn test_srcset_urls() {
        assert_eq!(srcset_urls("a.png"), ["a.png"]);
        assert_eq!(
            srcset_urls(" a.png 1x,b.png 2x , c.png"),
            ["a.png", "b.png", "c.png"]
        );
        assert_eq!(srcset_urls("a.png, b.png 2x"), ["a.png", "b.png"]);
        // URLs end at whitespace, so browsers read this as one URL.
        assert_eq!(srcset_urls("a.png,b.png 2x"), ["a.png,b.png"]);
        assert_eq!(
            srcset_urls("data:image/png;base64,AAA= 1x, b.png 2x"),
            ["data:image/png;base64,AAA=", "b.png"]
        );
    }

    #[test]
    fn test_decode_character_references() {
        assert_eq!(
            decode_character_references("a&amp;b&#x3D;&#61;&lt;"),
            "a&b==<"
        );
        assert_eq!(decode_character_references("&copy; & &;"), "&copy; & &;");
    }

    #[test]
    fn test_normalize() {
        assert_eq!(normalize("").as_deref(), Some(""));
        assert_eq!(normalize("a/b").as_deref(), Some("a/b"));
        assert_eq!(normalize("a/b/").as_deref(), Some("a/b/"));
        assert_eq!(normalize("a/./b/../c").as_deref(), Some("a/c"));
        assert_eq!(normalize("a/..").as_deref(), Some(""));
        assert_eq!(normalize("a/b/..").as_deref(), Some("a/"));
        assert_eq!(normalize("a//b").as_deref(), Some("a/b"));
        assert_eq!(normalize(".."), None);
    }

    #[test]
    fn test_resolve() {
        let site = site(&[], UrlStyle::Extensionless, "");
        assert_eq!(
            site.resolve("https://example.com/", ""),
            Ok(Target::External)
        );
        assert_eq!(
            site.resolve("//cdn.example.com/a.js", ""),
            Ok(Target::External)
        );
        assert_eq!(
            site.resolve("mailto:a@example.com", ""),
            Ok(Target::External)
        );
        assert_eq!(
            site.resolve("", "posts/"),
            Ok(Target::SamePage { fragment: None })
        );
        assert_eq!(
            site.resolve("#intro", "posts/"),
            Ok(Target::SamePage {
                fragment: Some("intro")
            })
        );
        assert_eq!(
            site.resolve("/about?x=1#team", "posts/"),
            Ok(path("about", Some("team")))
        );
        assert_eq!(
            site.resolve(" hello ", "posts/"),
            Ok(path("posts/hello", None))
        );
        assert_eq!(
            site.resolve("../img/a.png", "posts/"),
            Ok(path("img/a.png", None))
        );
        assert_eq!(site.resolve("../..", "posts/"), Err(Reason::AboveRoot));
        assert_eq!(site.resolve("/", "posts/"), Ok(path("", None)));
    }

    #[test]
    fn test_resolve_under_base_url_path() {
        let site = site(&[], UrlStyle::Directory, "https://Example.com/blog/");
        assert_eq!(site.resolve("/blog/about/", ""), Ok(path("about/", None)));
        assert_eq!(site.resolve("/blog", ""), Ok(path("", None)));
        assert_eq!(
            site.resolve("https://example.com/blog/a", ""),
            Ok(path("a", None))
        );
        assert_eq!(
            site.resolve("//example.com/blog/a", ""),
            Ok(path("a", None))
        );
        assert_eq!(
            site.resolve("https://example.com.evil/blog/a", ""),
            Ok(Target::External)
        );
        assert_eq!(
            site.resolve("http://example.com/blog/a", ""),
            Ok(Target::External)
        );
        let outside = Err(Reason::OutsideBasePath {
            base_path: "/blog".to_string(),
        });
        assert_eq!(site.resolve("/about/", ""), outside);
        assert_eq!(site.resolve("/blogroll/", ""), outside);
        assert_eq!(site.resolve("/blog/../about/", ""), outside);
        assert_eq!(site.resolve("https://example.com/", ""), outside);
        assert_eq!(site.resolve("../a", "posts/"), Ok(path("a", None)));
    }

    #[test]
    fn test_locate_extensionless() {
        let files = [
            "index.html",
            "about.html",
            "posts/index.html",
            "a b.pdf",
            "100%.html",
        ];
        let site = site(&files, UrlStyle::Extensionless, "");
        assert_eq!(site.locate(""), Some("index.html"));
        assert_eq!(site.locate("about"), Some("about.html"));
        assert_eq!(site.locate("about.html"), Some("about.html"));
        assert_eq!(site.locate("about/"), None);
        assert_eq!(site.locate("About"), None);
        assert_eq!(site.locate("posts"), Some("posts/index.html"));
        assert_eq!(site.locate("posts/"), Some("posts/index.html"));
        assert_eq!(site.locate("a%20b.pdf"), Some("a b.pdf"));
        assert_eq!(site.locate("100%"), Some("100%.html"));
    }

    #[test]
    fn test_locate_directory() {
        let files = ["index.html", "about/index.html", "404.html"];
        let site = site(&files, UrlStyle::Directory, "");
        assert_eq!(site.locate("about/"), Some("about/index.html"));
        assert_eq!(site.locate("about"), Some("about/index.html"));
        assert_eq!(site.locate("404.html"), Some("404.html"));
        assert_eq!(site.locate("404"), None);
    }

    #[test]
    fn test_base_href() {
        let docs_ids: HashSet<String> = ["setup".to_string()].into();
        let mut site = site(
            &["index.html", "docs/index.html", "docs/a.html"],
            UrlStyle::Extensionless,
            "https://example.com",
        );
        site.ids.insert("docs/index.html", &docs_ids);
        let check = |html: &str| {
            let mut report = Report::default();
            site.check_document(
                "index.html",
                DocumentKind::Html,
                &scan_html(html),
                &mut report,
            );
            let broken: Vec<_> = report
                .broken
                .iter()
                .map(|link| (link.url.clone(), link.reason.to_string()))
                .collect();
            (report.links, report.external, broken)
        };

        assert_eq!(
            check(
                r##"<base href="/docs/"><a href="#setup"><a href="?x"><a href=""><a href="a"><a href="#nope">"##
            ),
            (
                5,
                0,
                vec![(
                    "#nope".to_string(),
                    "no id \"nope\" in docs/index.html".to_string()
                )]
            )
        );
        assert_eq!(
            check(
                r##"<base href="https://other.example/"><a href="#x"><a href="/y"><a href="z"><a href="https://example.com/docs/a"><a href="https://example.com/missing">"##
            ),
            (
                2,
                3,
                vec![(
                    "https://example.com/missing".to_string(),
                    "/missing not found".to_string()
                )]
            )
        );
    }

    #[test]
    fn test_check_fragment() {
        let ids: HashSet<String> = ["intro".to_string(), "café".to_string()].into();
        let mut site = site(&["a.html", "doc.pdf"], UrlStyle::Extensionless, "");
        site.ids.insert("a.html", &ids);
        for fragment in ["intro", "caf%C3%A9", "", "top", "TOP", ":~:text=hello"] {
            assert_eq!(site.check_fragment("a.html", fragment), None, "{fragment}");
        }
        assert_eq!(
            site.check_fragment("a.html", "outro"),
            Some(Reason::MissingFragment {
                fragment: "outro".to_string(),
                file: "a.html".to_string()
            })
        );
        assert_eq!(site.check_fragment("doc.pdf", "page=2"), None);
        site.fragments = false;
        assert_eq!(site.check_fragment("a.html", "outro"), None);
    }
}
