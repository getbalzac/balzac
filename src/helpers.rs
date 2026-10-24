//! Built-in template helpers, registered on every renderer by
//! [`register`]. Comparison and logic helpers (`eq`, `and`, `len`, ...) come
//! with handlebars-rust and are not redefined here.

use std::cmp::Ordering;

use handlebars::{
    Context, Handlebars, Helper, HelperDef, HelperResult, JsonValue as Json, Output, RenderContext,
    RenderError, RenderErrorReason, ScopedJson, handlebars_helper,
};

use crate::config::ResolvedConfig;
use crate::renderer::HandlebarsRenderer;

/// Registers every built-in helper except `vite_url`, which depends on the
/// bundler configuration. `markdown` renders through `markdown_renderer`.
pub fn register(
    registry: &mut Handlebars<'_>,
    configuration: &ResolvedConfig,
    markdown_renderer: HandlebarsRenderer<'static>,
) {
    let site_urls = SiteUrls::new(configuration.base_url.as_deref(), configuration.dev_mode);
    registry.register_helper(
        "url_for",
        Box::new(UrlHelper {
            urls: site_urls.clone(),
            asset: false,
        }),
    );
    registry.register_helper(
        "asset",
        Box::new(UrlHelper {
            urls: site_urls,
            asset: true,
        }),
    );
    registry.register_helper("term_url", Box::new(term_url_helper));
    registry.register_helper("date", Box::new(DateHelper));
    registry.register_helper(
        "markdown",
        Box::new(MarkdownHelper {
            renderer: markdown_renderer,
        }),
    );
    registry.register_helper("slugify", Box::new(slugify_helper));
    registry.register_helper("json", Box::new(json_helper));
    registry.register_helper("sort", Box::new(sort_helper));
    registry.register_helper("limit", Box::new(limit_helper));
    registry.register_helper("where", Box::new(where_helper));
}

fn render_error(message: impl Into<String>) -> RenderError {
    RenderErrorReason::Other(message.into()).into()
}

/// Positional parameter `index` of `h`; `null` when missing.
fn param<'a>(h: &'a Helper<'_>, index: usize) -> &'a Json {
    h.param(index).map_or(&Json::Null, |p| p.value())
}

// `{{term_url "tags" this}}` -> `/tags/<term slug>/`; empty, with a warning,
// for values that never get a term page (missing, null, objects, "!!", ...).
handlebars_helper!(term_url_helper: |taxonomy: str, term: Json| {
    match crate::listing::term_name(term) {
        Some(name) if !crate::listing::slugify(&name).is_empty() => {
            crate::listing::term_url(taxonomy, &name)
        }
        _ => {
            log::warn!("term_url: {term} is not a {taxonomy} term, rendering an empty URL");
            String::new()
        }
    }
});

/// Site URLs derived from `base_url`.
#[derive(Clone)]
struct SiteUrls {
    /// Path of `base_url` without trailing slash (`/blog`), prepended to
    /// root-relative URLs. Empty in dev mode, where the site is served at `/`.
    path_prefix: String,
    /// `base_url` without trailing slash, for absolute URLs.
    absolute_base: Option<String>,
}

impl SiteUrls {
    fn new(base_url: Option<&str>, dev_mode: bool) -> Self {
        let absolute_base = base_url.map(|url| url.trim_end_matches('/').to_string());
        let path_prefix = match &absolute_base {
            Some(base) if !dev_mode => {
                let path = base.split_once("://").map_or(base.as_str(), |(_, rest)| {
                    rest.find('/').map_or("", |i| &rest[i..])
                });
                path.trim_end_matches('/').to_string()
            }
            _ => String::new(),
        };
        Self {
            path_prefix,
            absolute_base,
        }
    }

    /// `/about/` -> `<prefix>/about/`, or `<base_url>/about/` when `absolute`.
    /// External URLs (`https://...`, `//cdn...`, `mailto:...`) and fragments
    /// are returned unchanged.
    fn resolve(&self, path: &str, absolute: bool) -> Result<String, String> {
        if is_external(path) || path.starts_with('#') {
            return Ok(path.to_string());
        }
        let path = format!("/{}", path.trim_start_matches('/'));
        if absolute {
            let base = self
                .absolute_base
                .as_ref()
                .ok_or("absolute=true needs base_url to be set in balzac.toml")?;
            Ok(format!("{base}{path}"))
        } else {
            Ok(format!("{}{path}", self.path_prefix))
        }
    }
}

fn is_external(url: &str) -> bool {
    if url.starts_with("//") {
        return true;
    }
    url.split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

/// `{{url_for "/about/"}}` and `{{asset "css/site.css"}}` (under `/assets/`),
/// both accepting `absolute=true`.
struct UrlHelper {
    urls: SiteUrls,
    asset: bool,
}

impl HelperDef for UrlHelper {
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'rc>,
        _: &'reg Handlebars<'reg>,
        _: &'rc Context,
        _: &mut RenderContext<'reg, 'rc>,
    ) -> Result<ScopedJson<'rc>, RenderError> {
        let name = h.name();
        let path = match param(h, 0) {
            Json::String(path) => path,
            other => {
                return Err(render_error(format!(
                    "{name}: expected a path, got {other}"
                )));
            }
        };
        let path = if self.asset {
            format!("/assets/{}", path.trim_start_matches('/'))
        } else {
            path.clone()
        };
        let absolute = h
            .hash_get("absolute")
            .is_some_and(|v| v.value().as_bool() == Some(true));
        let url = self
            .urls
            .resolve(&path, absolute)
            .map_err(|e| render_error(format!("{name}: {e}")))?;
        Ok(ScopedJson::Derived(Json::String(url)))
    }
}

const DEFAULT_DATE_FORMAT: &str = "%Y-%m-%d";

/// `{{date fm.date "%B %d, %Y"}}`: formats a date with strftime syntax.
/// Missing and `null` dates render nothing.
struct DateHelper;

impl HelperDef for DateHelper {
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'rc>,
        _: &'reg Handlebars<'reg>,
        _: &'rc Context,
        _: &mut RenderContext<'reg, 'rc>,
    ) -> Result<ScopedJson<'rc>, RenderError> {
        let format = match param(h, 1) {
            Json::Null => DEFAULT_DATE_FORMAT,
            Json::String(format) => format,
            other => {
                return Err(render_error(format!(
                    "date: expected a format string, got {other}"
                )));
            }
        };
        let formatted = match param(h, 0) {
            Json::Null => String::new(),
            value => format_date(value, format).map_err(|e| render_error(format!("date: {e}")))?,
        };
        Ok(ScopedJson::Derived(Json::String(formatted)))
    }
}

/// Formats `value`: an ISO 8601 date or date-time (`2025-01-31`,
/// `2025-01-31T10:00:00+01:00`, ...), a Unix timestamp in seconds, or `"now"`.
fn format_date(value: &Json, format: &str) -> Result<String, String> {
    use jiff::fmt::strtime::BrokenDownTime;

    let time = match value {
        Json::String(text) if text == "now" => {
            BrokenDownTime::from(&jiff::Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC))
        }
        Json::String(text) => {
            let pieces = jiff::fmt::temporal::Pieces::parse(text.trim())
                .map_err(|e| format!("cannot parse {text:?} as a date: {e}"))?;
            let datetime = pieces.date().to_datetime(pieces.time().unwrap_or_default());
            let mut time = BrokenDownTime::from(datetime);
            time.set_offset(pieces.to_numeric_offset());
            time
        }
        Json::Number(number) => {
            let timestamp = number
                .as_i64()
                .and_then(|seconds| jiff::Timestamp::from_second(seconds).ok())
                .ok_or_else(|| format!("{number} is not a Unix timestamp in seconds"))?;
            BrokenDownTime::from(&timestamp.to_zoned(jiff::tz::TimeZone::UTC))
        }
        other => return Err(format!("expected a date, got {other}")),
    };
    time.to_string(format)
        .map_err(|e| format!("cannot format {value} with {format:?}: {e}"))
}

/// `{{markdown fm.summary}}`: renders a markdown string with the site's
/// markdown options and tag templates. The HTML is written unescaped;
/// `inline=true` drops the paragraph around single-paragraph input.
struct MarkdownHelper {
    /// Provides the markdown options and tag templates; tag templates are
    /// rendered with the registry calling the helper, not this renderer's.
    renderer: HandlebarsRenderer<'static>,
}

impl MarkdownHelper {
    fn render_html(
        &self,
        h: &Helper<'_>,
        registry: &Handlebars<'_>,
    ) -> Result<String, RenderError> {
        let source = match param(h, 0) {
            Json::Null => return Ok(String::new()),
            Json::String(source) => source,
            other => {
                return Err(render_error(format!(
                    "markdown: expected a string, got {other}"
                )));
            }
        };
        let inline = h
            .hash_get("inline")
            .is_some_and(|v| v.value().as_bool() == Some(true));
        self.renderer
            .render_fragment(registry, source, inline)
            .map_err(|e| render_error(format!("markdown: {e}")))
    }
}

impl HelperDef for MarkdownHelper {
    // Used as a subexpression, e.g. `{{#if (markdown x)}}`.
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'rc>,
        registry: &'reg Handlebars<'reg>,
        _: &'rc Context,
        _: &mut RenderContext<'reg, 'rc>,
    ) -> Result<ScopedJson<'rc>, RenderError> {
        Ok(ScopedJson::Derived(Json::String(
            self.render_html(h, registry)?,
        )))
    }

    // Writes the HTML directly so `{{markdown x}}` doesn't escape it.
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'rc>,
        registry: &'reg Handlebars<'reg>,
        _: &'rc Context,
        _: &mut RenderContext<'reg, 'rc>,
        out: &mut dyn Output,
    ) -> HelperResult {
        out.write(&self.render_html(h, registry)?)?;
        Ok(())
    }
}

// `{{slugify "Hello World"}}` -> `hello-world`, the same slugs as taxonomy terms.
handlebars_helper!(slugify_helper: |value: Json| {
    crate::listing::term_name(value).map(|text| crate::listing::slugify(&text)).unwrap_or_default()
});

// `{{{json value}}}`: `<`, `>` and `&` are escaped as `\u003c`, ... so the
// output can't close a `<script>` element.
handlebars_helper!(json_helper: |value: Json, {pretty: bool = false}| {
    let text = if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
    .unwrap_or_default();
    text.replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
});

/// Value at dotted `path` (`fm.title`, `tags.0`) in `value`; `value` itself for
/// an empty path.
fn field<'a>(value: &'a Json, path: &str) -> Option<&'a Json> {
    if path.is_empty() {
        return Some(value);
    }
    path.split('.').try_fold(value, |value, key| match value {
        Json::Object(map) => map.get(key),
        Json::Array(items) => key.parse::<usize>().ok().and_then(|i| items.get(i)),
        _ => None,
    })
}

/// Total order for sorting: numbers numerically, strings and booleans
/// naturally, then other types grouped; missing values and `null` sort last.
fn compare_values(a: Option<&Json>, b: Option<&Json>) -> Ordering {
    fn rank(value: Option<&Json>) -> u8 {
        match value {
            Some(Json::Number(_)) => 0,
            Some(Json::String(_)) => 1,
            Some(Json::Bool(_)) => 2,
            Some(Json::Array(_)) => 3,
            Some(Json::Object(_)) => 4,
            Some(Json::Null) | None => 5,
        }
    }
    match (a, b) {
        (Some(Json::Number(a)), Some(Json::Number(b))) => compare_numbers(a, b),
        (Some(Json::String(a)), Some(Json::String(b))) => a.cmp(b),
        (Some(Json::Bool(a)), Some(Json::Bool(b))) => a.cmp(b),
        _ => rank(a).cmp(&rank(b)),
    }
}

/// Compares numbers exactly: converting integers above 2^53 to `f64` would
/// lose precision.
fn compare_numbers(a: &serde_json::Number, b: &serde_json::Number) -> Ordering {
    let integer = |n: &serde_json::Number| {
        n.as_i64()
            .map(i128::from)
            .or_else(|| n.as_u64().map(i128::from))
    };
    let float = |n: &serde_json::Number| n.as_f64().unwrap_or_default();
    match (integer(a), integer(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(a), None) => compare_integer_to_float(a, float(b)),
        (None, Some(b)) => compare_integer_to_float(b, float(a)).reverse(),
        (None, None) => float(a).total_cmp(&float(b)),
    }
}

/// `integer` is within the i64/u64 range and `float` is finite (JSON has no
/// NaN or infinities).
fn compare_integer_to_float(integer: i128, float: f64) -> Ordering {
    // 2^64: beyond it, `float` is out of reach of any JSON integer
    const LIMIT: f64 = 18_446_744_073_709_551_616.0;
    let floor = float.floor();
    if floor >= LIMIT {
        return Ordering::Less;
    }
    if floor <= -LIMIT {
        return Ordering::Greater;
    }
    #[allow(clippy::cast_possible_truncation)] // integral and within ±2^64
    let floor_integer = floor as i128;
    integer.cmp(&floor_integer).then(if float > floor {
        Ordering::Less
    } else {
        Ordering::Equal
    })
}

fn items(value: &Json) -> &[Json] {
    match value {
        Json::Array(items) => items,
        _ => &[],
    }
}

// `{{#each (sort collections.posts "fm.title")}}`: sorts by a dotted field (or
// by value without one); `reverse=true` sorts descending, keeping missing
// values last. The sort is stable.
handlebars_helper!(sort_helper: |list: Json, {reverse: bool = false}, *args| {
    let key = args.get(1).and_then(|key| key.as_str()).unwrap_or_default();
    let mut sorted = items(list).to_vec();
    sorted.sort_by(|a, b| {
        let (a, b) = (field(a, key), field(b, key));
        let missing = |v: Option<&Json>| v.is_none_or(Json::is_null);
        match (missing(a), missing(b)) {
            (false, false) if reverse => compare_values(b, a),
            _ => compare_values(a, b),
        }
    });
    Json::Array(sorted)
});

// `{{#each (limit collections.posts 3)}}`: the first items of a list,
// after skipping `offset=n` items.
handlebars_helper!(limit_helper: |list: Json, count: u64, {offset: u64 = 0}| {
    let skip = usize::try_from(offset).unwrap_or(usize::MAX);
    let take = usize::try_from(count).unwrap_or(usize::MAX);
    Json::Array(items(list).iter().skip(skip).take(take).cloned().collect())
});

/// Handlebars truthiness: `false`, `null`, `0`, `""` and `[]` are falsy.
fn is_truthy(value: Option<&Json>) -> bool {
    match value {
        None | Some(Json::Null) => false,
        Some(Json::Bool(b)) => *b,
        Some(Json::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0),
        Some(Json::String(s)) => !s.is_empty(),
        Some(Json::Array(a)) => !a.is_empty(),
        Some(Json::Object(_)) => true,
    }
}

// `{{#each (where collections.posts "fm.featured" true)}}`: keeps items whose
// field equals the value, or is a list containing it (e.g. tags). Without a
// value, keeps items whose field is truthy.
handlebars_helper!(where_helper: |list: Json, key: str, *args| {
    let expected = args.get(2);
    let filtered = items(list)
        .iter()
        .filter(|item| {
            let actual = field(item, key);
            match (expected, actual) {
                (None, actual) => is_truthy(actual),
                (Some(expected), Some(Json::Array(values))) => {
                    values.contains(expected) || actual == Some(expected)
                }
                (Some(expected), actual) => actual == Some(expected),
            }
        })
        .cloned()
        .collect();
    Json::Array(filtered)
});

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use serde_json::json;

    use super::*;
    use crate::config::Config;
    use crate::renderer::Renderer;

    fn registry_for(config: &Config, dev_mode: bool) -> Handlebars<'static> {
        let mut configuration = config.resolve(Path::new("."));
        configuration.dev_mode = dev_mode;
        let mut registry = Handlebars::new();
        let mut renderer = HandlebarsRenderer::new(&configuration);
        renderer.tag_templates =
            HashMap::from([("em".to_string(), "<i>{{{content}}}</i>".to_string())]);
        register(&mut registry, &configuration, renderer.markdown_renderer());
        registry
    }

    fn render(template: &str, data: &Json) -> String {
        render_with(&Config::default(), template, data)
    }

    fn render_with(config: &Config, template: &str, data: &Json) -> String {
        registry_for(config, false)
            .render_template(template, data)
            .unwrap_or_else(|e| panic!("{template} failed: {e}"))
    }

    fn render_error_of(template: &str, data: &Json) -> String {
        registry_for(&Config::default(), false)
            .render_template(template, data)
            .expect_err("render should fail")
            .to_string()
    }

    fn with_base_url(base_url: &str) -> Config {
        Config {
            base_url: Some(base_url.to_string()),
            ..Config::default()
        }
    }

    #[test]
    fn url_for_without_base_url_is_root_relative() {
        assert_eq!(
            render(
                r#"{{url_for "about/"}}|{{url_for "/"}}|{{url_for ""}}"#,
                &json!({})
            ),
            "/about/|/|/"
        );
    }

    #[test]
    fn url_for_prefixes_base_url_path() {
        let config = with_base_url("https://example.com/blog/");
        assert_eq!(
            render_with(
                &config,
                r#"{{url_for "/about/"}}|{{url_for url}}"#,
                &json!({"url": "/posts/hello"})
            ),
            "/blog/about/|/blog/posts/hello"
        );
        let config = with_base_url("https://example.com");
        assert_eq!(
            render_with(&config, r#"{{url_for "/about/"}}"#, &json!({})),
            "/about/"
        );
    }

    #[test]
    fn url_for_absolute() {
        let config = with_base_url("https://example.com/blog/");
        assert_eq!(
            render_with(
                &config,
                r#"{{url_for "/about/" absolute=true}}|{{asset "/img/a.png" absolute=true}}"#,
                &json!({})
            ),
            "https://example.com/blog/about/|https://example.com/blog/assets/img/a.png"
        );
        assert!(
            render_error_of(r#"{{url_for "/" absolute=true}}"#, &json!({})).contains("base_url"),
        );
    }

    #[test]
    fn url_for_leaves_external_urls_and_fragments_alone() {
        let config = with_base_url("https://example.com/blog/");
        assert_eq!(
            render_with(
                &config,
                r##"{{url_for "https://x.org/a"}}|{{url_for "//cdn.x.org/a"}}|{{url_for "mailto:a@b.c"}}|{{url_for "#top"}}"##,
                &json!({})
            ),
            "https://x.org/a|//cdn.x.org/a|mailto:a@b.c|#top"
        );
    }

    #[test]
    fn url_for_ignores_base_url_path_in_dev_mode() {
        let registry = registry_for(&with_base_url("https://example.com/blog/"), true);
        let rendered = registry
            .render_template(
                r#"{{url_for "/about/"}}|{{url_for "/about/" absolute=true}}"#,
                &json!({}),
            )
            .unwrap();
        assert_eq!(rendered, "/about/|https://example.com/blog/about/");
    }

    #[test]
    fn url_for_does_not_shadow_url_fields() {
        assert_eq!(
            render(
                r"{{#each items}}{{url}};{{/each}}",
                &json!({"items": [{"url": "/a"}]})
            ),
            "/a;"
        );
    }

    #[test]
    fn asset_points_into_assets_directory() {
        let config = with_base_url("https://example.com/blog");
        assert_eq!(
            render_with(
                &config,
                r#"{{asset "css/site.css"}}|{{asset "/js/app.js"}}"#,
                &json!({})
            ),
            "/blog/assets/css/site.css|/blog/assets/js/app.js"
        );
    }

    #[test]
    fn date_formats_iso_dates() {
        let data = json!({
            "day": "2025-01-31",
            "datetime": "2025-01-31T14:05:09",
            "offset": "2025-01-31 14:05:09+01:00",
            "zulu": "2025-01-31T14:05:09Z",
            "unix": 1_738_332_309,
        });
        assert_eq!(
            render(r#"{{date day "%B %d, %Y"}}"#, &data),
            "January 31, 2025"
        );
        assert_eq!(render("{{date day}}", &data), "2025-01-31");
        assert_eq!(
            render(r#"{{date datetime "%a %-d %b %H:%M"}}"#, &data),
            "Fri 31 Jan 14:05"
        );
        assert_eq!(
            render(r#"{{date offset "%H:%M %:z"}}"#, &data),
            "14:05 +01:00"
        );
        assert_eq!(render(r#"{{date zulu "%H:%M %z"}}"#, &data), "14:05 +0000");
        assert_eq!(
            render(r#"{{date unix "%Y-%m-%dT%H:%M:%S"}}"#, &data),
            "2025-01-31T14:05:09"
        );
    }

    #[test]
    fn date_now() {
        let year: i32 = render(r#"{{date "now" "%Y"}}"#, &json!({}))
            .parse()
            .unwrap();
        assert!(year >= 2025);
    }

    #[test]
    fn date_missing_renders_nothing() {
        assert_eq!(
            render(r#"[{{date fm.date "%Y"}}]"#, &json!({"fm": {}})),
            "[]"
        );
    }

    #[test]
    fn date_errors() {
        assert!(render_error_of("{{date d}}", &json!({"d": "yesterday"})).contains("yesterday"));
        assert!(render_error_of(r#"{{date d "%z"}}"#, &json!({"d": "2025-01-31"})).contains("%z"));
        assert!(render_error_of(r#"{{date d "%Q"}}"#, &json!({"d": "2025-01-31"})).contains("%Q"));
    }

    #[test]
    fn markdown_renders_unescaped_with_tag_templates() {
        let data = json!({"summary": "Hello *world* & <b>you</b>"});
        assert_eq!(
            render("{{markdown summary}}", &data),
            "<p>Hello <i>world</i> &amp; <b>you</b></p>\n"
        );
        assert_eq!(
            render("{{markdown summary inline=true}}", &data),
            "Hello <i>world</i> &amp; <b>you</b>"
        );
        assert_eq!(render("[{{markdown missing}}]", &data), "[]");
    }

    #[test]
    fn markdown_inline_keeps_blocks_of_multi_paragraph_input() {
        assert_eq!(
            render("{{markdown text inline=true}}", &json!({"text": "a\n\nb"})),
            "<p>a</p>\n<p>b</p>\n"
        );
    }

    #[test]
    fn markdown_as_subexpression() {
        assert_eq!(
            render(r"{{#if (markdown text)}}yes{{/if}}", &json!({"text": "a"})),
            "yes"
        );
    }

    #[test]
    fn slugify_strings_and_numbers() {
        assert_eq!(
            render(
                r#"{{slugify "Rust & WebAssembly!"}}|{{slugify 2024}}|{{slugify missing}}"#,
                &json!({})
            ),
            "rust-webassembly|2024|"
        );
    }

    #[test]
    fn json_is_safe_in_script_elements() {
        let data = json!({"v": {"a": "</script>&"}});
        assert_eq!(
            render("{{{json v}}}", &data),
            r#"{"a":"\u003c/script\u003e\u0026"}"#
        );
        assert_eq!(
            render("{{{json v pretty=true}}}", &data),
            "{\n  \"a\": \"\\u003c/script\\u003e\\u0026\"\n}"
        );
    }

    fn posts() -> Json {
        json!({"posts": [
            {"slug": "b", "fm": {"title": "Banana", "weight": 2, "tags": ["rust"], "featured": true}},
            {"slug": "c", "fm": {"title": "Cherry", "tags": ["web"]}},
            {"slug": "a", "fm": {"title": "Apple", "weight": 10, "tags": ["rust", "web"], "featured": false}},
        ]})
    }

    #[test]
    fn sort_by_field() {
        let data = posts();
        assert_eq!(
            render(
                r#"{{#each (sort posts "fm.title")}}{{slug}}{{/each}}"#,
                &data
            ),
            "abc"
        );
        // missing weights come last in both directions
        assert_eq!(
            render(
                r#"{{#each (sort posts "fm.weight")}}{{slug}}{{/each}}"#,
                &data
            ),
            "bac"
        );
        assert_eq!(
            render(
                r#"{{#each (sort posts "fm.weight" reverse=true)}}{{slug}}{{/each}}"#,
                &data
            ),
            "abc"
        );
    }

    #[test]
    fn sort_scalars_and_missing_lists() {
        assert_eq!(
            render(
                r"{{#each (sort list)}}{{this}}{{/each}}|{{len (sort missing)}}",
                &json!({"list": [3, 1, 2]})
            ),
            "123|0"
        );
    }

    #[test]
    fn sort_large_integers_exactly() {
        let data = json!({"list": [9_007_199_254_740_993_u64, 9_007_199_254_740_992_u64, u64::MAX, -1, 1.5]});
        assert_eq!(
            render(r"{{#each (sort list)}}{{this}},{{/each}}", &data),
            "-1,1.5,9007199254740992,9007199254740993,18446744073709551615,"
        );
    }

    #[test]
    fn sort_large_integers_against_floats() {
        // 9007199254740993.0 is not representable: it parses as 2^53
        let data = json!({"list": [9_007_199_254_740_993_u64, 9_007_199_254_740_992.0, 2.5, 2, 3, -2.5, -3]});
        assert_eq!(
            render(r"{{#each (sort list)}}{{this}},{{/each}}", &data),
            "-3,-2.5,2,2.5,3,9007199254740992.0,9007199254740993,"
        );
    }

    #[test]
    fn limit_with_offset() {
        let data = posts();
        assert_eq!(
            render(r"{{#each (limit posts 2)}}{{slug}}{{/each}}", &data),
            "bc"
        );
        assert_eq!(
            render(
                r"{{#each (limit posts 5 offset=1)}}{{slug}}{{/each}}",
                &data
            ),
            "ca"
        );
    }

    #[test]
    fn where_filters_by_field() {
        let data = posts();
        assert_eq!(
            render(
                r#"{{#each (where posts "fm.featured" true)}}{{slug}}{{/each}}"#,
                &data
            ),
            "b"
        );
        assert_eq!(
            render(
                r#"{{#each (where posts "fm.featured" false)}}{{slug}}{{/each}}"#,
                &data
            ),
            "a"
        );
        assert_eq!(
            render(
                r#"{{#each (where posts "fm.weight")}}{{slug}}{{/each}}"#,
                &data
            ),
            "ba"
        );
        assert_eq!(
            render(
                r#"{{#each (where posts "fm.tags" "web")}}{{slug}}{{/each}}"#,
                &data
            ),
            "ca"
        );
        assert_eq!(
            render(
                r#"{{#each (where posts "fm.title" "Apple")}}{{slug}}{{/each}}"#,
                &data
            ),
            "a"
        );
    }

    #[test]
    fn helpers_compose() {
        let data = posts();
        assert_eq!(
            render(
                r#"{{#each (limit (sort (where posts "fm.tags" "rust") "fm.title") 1)}}{{fm.title}}{{/each}}"#,
                &data
            ),
            "Apple"
        );
    }
}
