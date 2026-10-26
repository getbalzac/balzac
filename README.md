# Balzac

> an experiment in building a static site generator using rust

## Install

With npm (or any Node package manager), globally or as a project dependency:

```sh
npm install --global balzac
# or run it without installing
npx balzac init
```

With Homebrew:

```sh
brew install getbalzac/tap/balzac
```

With Cargo:

```sh
cargo install balzac
```

With the installer scripts from the [latest release](https://github.com/getbalzac/balzac/releases/latest):

```sh
# macOS and Linux
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/getbalzac/balzac/releases/latest/download/balzac-installer.sh | sh
```

```powershell
# Windows
powershell -ExecutionPolicy Bypass -c "irm https://github.com/getbalzac/balzac/releases/latest/download/balzac-installer.ps1 | iex"
```

Prebuilt binaries are available for macOS (arm64, x64), Linux (arm64, x64; glibc and musl) and Windows (arm64, x64).

## Usage

- Install Balzac
- Initialize a project
- Build or run it in dev mode

### Commands

Create a new project in the current directory:

```sh
balzac init
```

Create a new project in a specific directory:

```sh
balzac init --path ./my-site
```

Create a new project with sitemap support enabled:

```sh
balzac init --sitemap
```

Create a new project with RSS/Atom feed support enabled:

```sh
balzac init --feed
```

Build the site:

```sh
balzac build
```

When a template, frontmatter or config file is invalid, the build stops with an error naming the file (and the line and column for template errors) and exits with status 1.

Pages are rendered in parallel, one thread per CPU core. Set `RAYON_NUM_THREADS` to use fewer threads, e.g. `RAYON_NUM_THREADS=1 balzac build` for a sequential build. The output is the same whatever the number of threads. When several pages fail, the build reports the first one in build order: static pages, then collection items, then listings.

Build a site from a specific root:

```sh
balzac build --root ./my-site
```

Run the dev server:

```sh
balzac dev
```

Run the dev server on a custom host and port:

```sh
balzac dev --host 127.0.0.1 --port 4000
```

Run the dev server for a specific project root:

```sh
balzac dev --root ./my-site
```

### Dev Mode

`balzac dev` performs an initial build, serves the output directory locally, watches Balzac source files, and reloads the browser when pages change.

Current behavior:

- direct changes to page files in `pages/` (including nested ones like `pages/docs/intro.hbs`) trigger a targeted rebuild and reload only for that page, after checking the site for output conflicts; deleting a page removes its output and any output folders left empty. A full rebuild runs instead for the 404 page, a page that [shadows a collection item](#collections), and a deleted page whose output another page still writes (e.g. deleting `pages/docs/intro.hbs` while `pages/docs/intro.html` remains)
- direct changes to markdown files in `content/<collection>/` (including nested folders) re-render all pages (any page can list collection items through `collections`), including listing and taxonomy pages, and trigger a full-page reload, without recreating the output directory; generated pages that are no longer produced (deleted items, changed `slug`s, a tag with no posts left) are removed
- changes to shared files such as `partials/`, `layouts/`, `assets/`, `helpers/`, `balzac.toml`, or collection templates (`details.hbs` by default) trigger a full rebuild and full-page reload
- draft collection items (`draft: true`) are rendered
- hooks are skipped unless you opt in with `dev = true` in `[hooks]` (see [Hooks in dev mode](#hooks-in-dev-mode))
- a failed build (template, frontmatter or config error) does not stop the server: the error is logged and shown in an overlay in the browser until the next successful build, and any change made while the build is failing rebuilds the whole site. Only a `balzac.toml` that cannot be read or parsed when the server starts stops it, since the config says which directories to serve and watch
- missing paths are answered with a `404` status and your `404.html` page if you have one (see [404 Page](#404-page)), otherwise plain text

The dev server defaults to `http://127.0.0.1:4000`.


## Directory Structure

Balzac supports different directories that you are free to create or skip:

- pages_directory (required): directory that stores all the handlebars templates that will be used to create pages
- partials_directory (optional): houses all handlebars partials
- layouts_directory (optional): houses all handlebars layouts
- helpers_directory (optional): houses custom template helpers written in Rhai, see [Custom helpers](#custom-helpers)

### Static Pages

Every `.hbs`, `.handlebars` or `.html` file in the pages directory becomes a page. Subdirectories are walked recursively and mirrored in the URL, and `index` files map to their folder's URL. Where each page is written depends on [`url_style`](#url-style):

| Source                           | URL                     | `extensionless` output           | `directory` output                     |
| -------------------------------- | ----------------------- | -------------------------------- | -------------------------------------- |
| `pages/index.hbs`                | `/`                     | `dist/index.html`                | `dist/index.html`                      |
| `pages/about.hbs`                | `/about`                | `dist/about.html`                | `dist/about/index.html`                |
| `pages/docs/index.hbs`           | `/docs`                 | `dist/docs.html`                 | `dist/docs/index.html`                 |
| `pages/docs/getting-started.hbs` | `/docs/getting-started` | `dist/docs/getting-started.html` | `dist/docs/getting-started/index.html` |

With `directory`, URLs end with a slash (`/docs/`).

Some files in a subfolder are templates rather than pages, but only when the feature that uses them is set up. Otherwise they're rendered as regular pages (e.g. `pages/archive/list.hbs` at `/archive/list`):

- `pages/<collection>/details.hbs` (or the collection's configured [`template`](#collection-config)) when a matching `content/<collection>` directory exists (see [Collections](#collections))
- `pages/<collection>/list.hbs` when `[listings.<collection>]` is configured
- `pages/<taxonomy>/term.hbs` and `pages/<taxonomy>/taxonomy.hbs` when `[taxonomies.<taxonomy>]` is configured

The build fails if two page files would write the same file, e.g. `pages/docs/intro.hbs` and `pages/docs/intro.html`, or `pages/docs.hbs` and `pages/docs/index.hbs` (both are `/docs`). The same goes for a static page and a generated listing or taxonomy page, e.g. `pages/posts.hbs` and the first page of `[listings.posts]` with `url_style = "directory"`. Only the page at `/404`, `pages/404.hbs` (or `.html`/`.handlebars`) or `pages/404/index.hbs`, is the [404 page](#404-page) and left out of the sitemap; `pages/docs/404.hbs` is a regular page.



## Config Reference

- output_directory : the directory where the generated website will be sent to
- pages_directory (optional): the directory where static pages will reside (like your index)
- partials_directory (optional): directory where partial templates will reside
- layouts_directory (optional): directory where layout templates will reside
- assets_directory (optional): directory where static assets will reside
- content_directory (optional): directory where content (markdown) will reside
- tags_directory (optional): directory where markdown tag templates will reside, see [Tag templates](#tag-templates)
- helpers_directory (optional): directory where custom helper scripts will reside (default: `./helpers`)
- global: fill this array if you want to have global data available in all the templates and files
- base_url (optional): the public URL of the site, used for the sitemap, robots.txt and the [`url_for` and `asset` helpers](#url_for)
- sitemap (optional): see [Sitemap](#sitemap)
- robots (optional): see [robots.txt](#robotstxt)
- listings (optional): paginated collection listings, see [Listings and pagination](#listings-and-pagination)
- taxonomies (optional): tag/category pages, see [Taxonomies](#taxonomies)
- markdown (optional): markdown rendering options, see [Markdown](#markdown)
- url_style (optional): how page URLs map to output files, see [URL Style](#url-style)
- feed (optional): RSS and Atom feeds for collections, see [Feeds](#feeds)
- hooks (optional): shell commands run during the build, see [Hooks](#hooks)
- bundler (optional): asset bundler integration, see [Vite](#vite)
- collections (optional): per-collection settings, see [Collections](#collections)

Unknown keys are rejected, so a typo like `ouput_directory` or `[sitemp]` fails the build (and `balzac dev`) with the offending line and a suggestion:

```txt
TOML parse error at line 3, column 2
  |
3 | [sitemp]
  |  ^^^^^^
unknown field `sitemp`, expected one of `output_directory`, ...
help: did you mean `sitemap`?
```

`global` is the exception: it is free-form, any key goes.

### URL Style

`url_style` controls where pages are written and which URLs are used for them (including in the sitemap and the dev server).

| `url_style`               | `pages/about.hbs`        | `content/posts/foo.md`       |
| ------------------------- | ------------------------ | ---------------------------- |
| `"extensionless"` (default) | `dist/about.html` → `/about` | `dist/posts/foo.html` → `/posts/foo` |
| `"directory"`             | `dist/about/index.html` → `/about/` | `dist/posts/foo/index.html` → `/posts/foo/` |

```toml
url_style = "directory"
```

- `extensionless` needs a host that serves `about.html` for `/about` (Netlify, Vercel, Cloudflare Pages).
- `directory` works on any static host (S3, GitHub Pages, nginx defaults).
- `pages/index.hbs` is always written to `dist/index.html` and served at `/`.
- With `directory`, the 404 page stays at `dist/404.html` so hosts can find the error page.
- With `directory`, `balzac dev` redirects `/about` to `/about/`, as static hosts do.
- [Listing](#listings-and-pagination) and [taxonomy](#taxonomies) pages (`/posts/`, `/posts/page/2/`, `/tags/rust/`) are always directory-style, written to `<url>/index.html`, whatever the `url_style`. Only the item URLs they link to (`pagination.items`, `collections`) follow `url_style`.
- With `directory`, a top-level page named like a listing (`pages/posts.hbs` with `[listings.posts]`, or `pages/tags.hbs` with a `tags/taxonomy.hbs` index) would write the same `index.html`, so the build fails.

### Vite

Balzac supports Vite through `[bundler.vite]`.

```toml
[bundler.vite]
enabled = true
manifest_path = "dist/.vite/manifest.json"
dev_origin = "http://127.0.0.1:5173"
```

- `manifest_path` is used by `balzac build`
- `dev_origin` is used by `balzac dev`

When using Vite in development, run Vite separately from Balzac. A typical workflow looks like this:

```sh
pnpm vite
balzac dev
```

Templates using `{{vite_url "main.js"}}` will resolve to the Vite dev server in `balzac dev` and to the built manifest in `balzac build`.

## Template Helpers

Besides the helpers that come with [handlebars-rust](https://docs.rs/handlebars) (`if`, `unless`, `each`, `with`, `lookup`, `log`, `eq`, `ne`, `gt`, `gte`, `lt`, `lte`, `and`, `or`, `not`, `len`), Balzac registers these helpers in every template, including [tag templates](#tag-templates). [`vite_url`](#vite) and [`term_url`](#taxonomies) are documented with their features.

Helpers that return lists (`sort`, `limit`, `where`) are meant to be used as subexpressions and can be nested:

```hbs
{{#each (limit (sort (where collections.posts "fm.tags" "rust") "fm.title") 5)}}
  <a href="{{url_for url}}">{{fm.title}}</a>
{{/each}}
```

### url_for

Turns a site path into a URL, taking the path of `base_url` into account, so a site can be deployed under a sub-path:

```hbs
<a href="{{url_for "/about/"}}">About</a>     {{!-- base_url = "https://example.com/blog/" -> /blog/about/ --}}
<a href="{{url_for url}}">{{fm.title}}</a>   {{!-- an item URL -> /blog/posts/hello --}}
<link rel="canonical" href="{{url_for url absolute=true}}"> {{!-- https://example.com/blog/posts/hello --}}
```

- URLs are root-relative (`/blog/about/`), so they work on any host. Without `base_url`, or when the path of `base_url` is `/`, the path is returned with a leading `/`.
- `absolute=true` prepends the whole `base_url`, for canonical links, feeds and social tags. It fails when `base_url` is not set.
- External URLs (`https://...`, `//cdn...`, `mailto:...`) and fragments (`#top`) are returned unchanged.
- In `balzac dev` the site is served at `/`, so the path of `base_url` is not prepended (`absolute=true` still uses `base_url`).

The helper is called `url_for` rather than `url` because a helper named `url` would replace the `url` field of collection items in `{{url}}`.

### asset

Like `url_for`, for files of the assets directory, which are published under `/assets/`. Accepts `absolute=true` too:

```hbs
<link rel="stylesheet" href="{{asset "css/site.css"}}"> {{!-- /assets/css/site.css --}}
```

### date

Formats a date with [strftime](https://docs.rs/jiff/latest/jiff/fmt/strtime/index.html#conversion-specifications) syntax. The format defaults to `%Y-%m-%d`:

```hbs
<time datetime="{{date fm.date}}">{{date fm.date "%B %-d, %Y"}}</time> {{!-- January 31, 2025 --}}
&copy; {{date "now" "%Y"}}
```

- The date can be an ISO 8601 date or date-time (`2025-01-31`, `2025-01-31T10:00:00`, `2025-01-31 10:00:00+01:00`, `2025-01-31T09:00:00Z`), a Unix timestamp in seconds, or `"now"` (the build time, in UTC).
- Dates are formatted as written: there is no time zone conversion. Offsets (`%z`, `%:z`) can only be formatted for dates that have one; Unix timestamps and `"now"` are in UTC.
- A missing or `null` date renders nothing. A date that can't be parsed, or an invalid format, fails the build.

### markdown

Renders a markdown string, such as a frontmatter field, with the [markdown options](#markdown) and tag templates of the site. The HTML is not escaped, so double braces are enough. `inline=true` drops the `<p>` around input made of a single paragraph, for use in headings or links:

```hbs
<div class="summary">{{markdown fm.summary}}</div>
<h1>{{markdown fm.title inline=true}}</h1>
```

A missing or `null` value renders nothing.

### slugify

Turns text into a URL slug, the same way as [taxonomy terms](#taxonomies):

```hbs
<section id="{{slugify fm.title}}"> {{!-- "Rust & WebAssembly" -> rust-webassembly --}}
```

### json

Serializes a value as JSON, with `pretty=true` for indented output. `<`, `>` and `&` are escaped as `\u003c`, `\u003e` and `\u0026`, so the output can be put in a `<script>` element with triple braces:

```toml
[global.organization]
"@type" = "Organization"
name = "Balzac"
```

```hbs
<script type="application/ld+json">{{{json organization}}}</script>
<div data-tags="{{json fm.tags}}"></div>
```

### sort

Sorts a list by a field, given as a dotted path (`fm.title`), or by value when no field is given. `reverse=true` sorts in descending order. Numbers are compared numerically and text alphabetically; items without the field come last in both orders. The sort is stable:

```hbs
{{#each (sort collections.docs "fm.weight")}}...{{/each}}
{{#each (sort fm.tags)}}...{{/each}}
{{#each (sort collections.posts "fm.title" reverse=true)}}...{{/each}}
```

### limit

Keeps the first items of a list, after skipping `offset` items:

```hbs
{{#each (limit collections.posts 3)}}...{{/each}}            {{!-- latest three posts --}}
{{#each (limit collections.posts 10 offset=3)}}...{{/each}}  {{!-- the next ten --}}
```

### where

Keeps the items of a list whose field equals a value. When the field is a list, such as tags, items that contain the value are kept. Without a value, items whose field is truthy (not missing, `null`, `false`, `0`, `""` or `[]`) are kept:

```hbs
{{#each (where collections.posts "fm.tags" "rust")}}...{{/each}}
{{#each (where collections.posts "fm.featured")}}...{{/each}}
{{len (where collections.posts "fm.tags" "rust")}} posts about Rust
```

`sort`, `limit` and `where` treat a missing list as empty.

## 404 Page

Create `pages/404.hbs` (or `.html`/`.handlebars`, or `pages/404/index.hbs`) and it is rendered to `dist/404.html` like any other page. Most static hosts (GitHub Pages, Netlify, Cloudflare Pages, ...) serve this file automatically for missing paths. The 404 page is always left out of the sitemap, and `balzac dev` serves it with a `404` status for any path that doesn't exist.

## Sitemap

When `base_url` is set, Balzac writes a sitemap of all pages, collection items, listings and taxonomy pages. `balzac init --sitemap` sets it up. Optional settings:

```toml
base_url = "https://example.com"

[sitemap]
enabled = true              # set to false to skip generating the sitemap
filename = "sitemap.xml"    # default: sitemap.xml
default_priority = 0.5      # default: none
default_changefreq = "weekly" # default: none
```

Collection items can override these with `priority`, `changefreq` and `lastmod` in their frontmatter, or leave the sitemap with `sitemap_exclude: true`.

## robots.txt

Balzac writes a `dist/robots.txt` that allows all crawlers. When the sitemap is generated (`base_url` set and sitemap not disabled), a `Sitemap:` line pointing to it is added:

```txt
User-agent: *
Allow: /

Sitemap: https://example.com/sitemap.xml
```

Customize or disable it in `balzac.toml`:

```toml
[robots]
enabled = true            # set to false to skip generating robots.txt
disallow = ["/drafts/"]   # replaces `Allow: /` with these `Disallow:` rules
```

To write your own, put a `robots.txt` in your pages directory (`pages/robots.txt`). It is copied verbatim to `dist/robots.txt` and replaces the generated file, even when `enabled = false`. The pages directory is used because it maps to the root of your site, which is the only place crawlers look for `robots.txt`; files in `assets/` are published under `/assets/`.

## Feeds

Balzac can generate an Atom and/or RSS 2.0 feed for each collection. Feeds are opt-in through a `[feed]` section and require `base_url`, since feed links must be absolute.

```toml
base_url = "https://example.com"

[feed]
enabled = true          # default: true
format = "atom"         # "atom" (default), "rss" or "both"
collections = ["blog"]  # default: every collection
limit = 20              # items per feed, newest first; 0 = unlimited (default: 20)
full_content = false    # include the rendered markdown body (default: false)
title = "My Site"       # default: the collection name
description = "Latest posts"
author = "Jane Doe"     # feed-level Atom author; without it, the feed title is used when some entries lack an `author`

[feed.overrides.blog]   # optional per-collection title/description
title = "My Blog"
description = "Posts about things"
```

Each collection gets its own feed, written next to its pages:

- Atom: `<output_directory>/<collection>/atom.xml`, served at `/<collection>/atom.xml`
- RSS: `<output_directory>/<collection>/feed.xml`, served at `/<collection>/feed.xml`

Set `collections = []` to skip per-collection feeds entirely (useful with a combined feed only).

### Combined Feed

A site-wide feed merging several collections (newest first) can be written at the output root as `/atom.xml` and/or `/feed.xml`. It is off by default and accepts three forms:

```toml
[feed]
combined = true                  # every collection that has its own feed
```

```toml
[feed]
combined = ["blog", "notes"]     # these collections, whether or not they have their own feed
```

```toml
[feed.combined]
collections = ["blog", "notes"]  # default: the collections that have their own feed
title = "Everything"             # default: [feed] title, then base_url
description = "All updates"      # default: [feed] description
limit = 50                       # default: [feed] limit; 0 = unlimited
```

Link them from your templates for autodiscovery, e.g. `<link rel="alternate" type="application/atom+xml" href="/blog/atom.xml">`.

### Feed Frontmatter

Feed items are built from these frontmatter keys:

- `date` (required): publication date. Entries without a valid `date` are left out of the feed, with a warning
- `title`: item title, defaults to the item's URL path
- `updated` (or `lastmod`): last modification date, defaults to `date`
- `summary` (or `description`): short plain-text summary
- `author`: author name (Atom `<author>`, RSS `<dc:creator>`)

Dates can be `2024-01-15`, `2024-01-15T10:30:00`, `2024-01-15 10:30` or full RFC 3339 like `2024-01-15T10:30:00+02:00`. Values without an offset are treated as UTC. Atom output uses RFC 3339 dates and RSS output uses RFC 822 dates, both normalized to UTC.

With `full_content = true`, relative links in the body are resolved against the item URL in Atom (via `xml:base`). RSS has no equivalent, so prefer absolute links if you publish full-content RSS.

## Hooks

Balzac supports hooks that allow you to run shell commands at various phases of the build process. All hooks are optional and configured in the `[hooks]` section of your `balzac.toml` file.

### Available Hooks

Hooks are executed in the following order during a build:

1. **render_init_before**: Runs before the renderer is initialized
2. **render_init_after**: Runs after the renderer is initialized
3. **build_before**: Runs before the dist folder is created
4. **render_before**: Runs before rendering static pages and collections
5. **render_after**: Runs after rendering is complete, before assets are copied
6. **build_after**: Runs after all build steps are complete

### Hook Configuration

Example `balzac.toml` configuration:

```toml
[hooks]
render_init_before = "echo 'Preparing renderer'"
render_init_after = "echo 'Renderer ready'"
build_before = "pnpm build"
render_before = "echo 'Starting page generation'"
render_after = "echo 'Pages generated'"
build_after = "rsync -av dist/ production/"
```

### Notes

- Hooks are executed in the project root directory
- If a hook fails (exits with non-zero status), the entire build process will terminate
- Hook execution time is logged for each hook
- All hooks support full shell command syntax with arguments
- Hooks run during `balzac build`, and during `balzac dev` only when `dev = true` (see below)

### Hooks in dev mode

Hooks are skipped by `balzac dev` unless you opt in, so a `build_after` that deploys does not run on every save:

```toml
[hooks]
dev = true
render_init_before = "pnpm build"
```

With `dev = true`:

- the initial dev build and every full rebuild (shared files such as `partials/`, `layouts/`, `assets/` or `balzac.toml` changed) run all hooks, in the same order as `balzac build`
- targeted rebuilds (a page in `pages/` or markdown in `content/` changed) run only `render_before` and `render_after`
- a failing hook aborts the build it runs in, like any other build error: the error is logged and shown in the browser overlay, the server keeps running (also when the initial build fails), and the next change rebuilds the whole site, hooks included. The previous output is left untouched only if the hook failed before anything was written (`render_init_*` and `build_before`, or `render_before` in a targeted rebuild); a later failure can leave changed or partial output (a full rebuild recreates the output directory before `render_before`) until the next successful build
- hooks run synchronously and block the rebuild. Files they write into watched directories (e.g. `assets/`) are applied right after it in one extra pass with hooks paused, which updates the output in place: it re-renders the site and copies only the changed assets, keeping files hooks such as `build_after` wrote there (if a hook edits `balzac.toml`, that pass is a regular full rebuild instead), so they reach the output without triggering the hooks again
- long-running commands such as `vite` or `tailwindcss --watch` should not be hooks: run them alongside `balzac dev` instead

## Collections

To use a collection, create a subfolder in the content_directory folder with the name of your collection (i.e. posts) and put your markdown files in there.

The next step is to create a file in the pages_directory called <name_of_your_collection>/details.hbs (i.e. posts/details.hbs).

Each markdown file is rendered to `/<collection>/<file name>`, e.g. `content/posts/hello.md` becomes `/posts/hello`. Markdown files can be organised in nested folders; the folders are kept in the URL, so `content/posts/2024/hello.md` becomes `/posts/2024/hello`.

Other pages in that folder are still rendered as static pages, so `pages/posts/index.hbs` can serve as a listing page at `/posts`. If a static page and a collection item would render to the same file (e.g. `pages/posts/hello.hbs` and `content/posts/hello.md`), the static page wins and Balzac logs a warning.

### Frontmatter

All frontmatter present in the collection documents will be available in the template under the fm namespace.

```md
---
title: "Test"
---
```

will be available under `fm.title`

Some frontmatter keys change how an item is built:

- `slug`: replaces the file name in the URL. `content/posts/2024-01-01-hello.md` with `slug: hello-world` is rendered to `/posts/hello-world`. Nested folders are kept. The slug must be a non-empty, single URL segment: no `/`, `\`, `#`, `?`, `%` or whitespace, and it must not end with `.html`. Collection, folder and file names must not contain `#`, `?`, `%` or `\` either, and nested folder names must not end with `.html`. Two items resolving to the same URL, or to URLs differing only in letter case, fail the build.
- `draft: true`: the item is skipped by `balzac build` but rendered by `balzac dev` (and kept out of the sitemap).

### Collection Config

Collections work without any configuration. To change how a collection is rendered, add a `[collections.<name>]` section to `balzac.toml`:

```toml
[collections.posts]
template = "post.hbs"
```

- `template`: template used to render each item, relative to `<pages_directory>/<name>/` (here `pages/posts/post.hbs`). Defaults to `details.hbs`. It must stay inside that folder (subfolders like `templates/post.hbs` are fine); a template elsewhere would also be built as a static page.

### Listing collection items

Every page (static pages and collection item pages) has access to all collections under the `collections` namespace. `collections.<name>` is an array of items with:

- `url`: the item's URL (e.g. `/posts/hello`, or `/posts/hello/` with `url_style = "directory"`)
- `slug`: the last segment of the item's URL: its `slug` frontmatter, or the markdown file name without extension (e.g. `hello`)
- `fm`: the item's frontmatter
- `content`: the item's rendered HTML

Items are sorted by `fm.date` (compared as text, so use ISO dates like `2025-01-31`) from newest to oldest. Items without a date come after dated items, and ties are sorted by slug.

```hbs
<ul>
  {{#each collections.posts}}
    <li><a href="{{url}}">{{fm.title}}</a> {{fm.date}}</li>
  {{/each}}
</ul>
```

Only collections that have a template (`details.hbs` by default) are included. Drafts are listed in `balzac dev` only.

### Listings and pagination

To generate a listing of a collection, add a `[listings.<collection>]` table to `balzac.toml` and create `pages/<collection>/list.hbs`:

```toml
[listings.posts]
per_page = 10 # optional, all items go on a single page when omitted
```

This renders `/posts/`, `/posts/page/2/`, `/posts/page/3/`, ... (written to `posts/index.html`, `posts/page/2/index.html`, ...). Items are in the same order as `collections.<name>`.

Listings are configured in `balzac.toml` rather than in a page's frontmatter because their URLs come from the collection, not from a page file, and because taxonomy pages (below) have no page of their own to hold frontmatter, so both are configured the same way.

`list.hbs` receives `collection` (the collection name), `collections`, the `global` data and a `pagination` object:

- `items`: the items on this page, shaped like `collections.<name>` items (`url`, `slug`, `fm`, `content`)
- `current_page`, `total_pages` (1-based), `per_page`, `total_items`
- `prev_url`, `next_url`: URL of the previous/next page, `null` on the first/last page
- `first_url`, `last_url`
- `pages`: every page as `{ number, url, current }`, for numbered navigation

```hbs
{{#each pagination.items}}
  <a href="{{url}}">{{fm.title}}</a>
{{/each}}
<nav>
  {{#if pagination.prev_url}}<a href="{{pagination.prev_url}}">Newer</a>{{/if}}
  {{#each pagination.pages}}
    {{#if current}}<span>{{number}}</span>{{else}}<a href="{{url}}">{{number}}</a>{{/if}}
  {{/each}}
  {{#if pagination.next_url}}<a href="{{pagination.next_url}}">Older</a>{{/if}}
</nav>
```

An empty collection still renders its first page, with no items.

### Taxonomies

A taxonomy groups collection items by a frontmatter key. Add a `[taxonomies.<key>]` table and create `pages/<key>/term.hbs`:

```toml
[taxonomies.tags]
per_page = 10            # optional, all items go on a single page when omitted
collections = ["posts"]  # optional, every collection when omitted

[taxonomies.categories]
```

```md
---
title: "Hello"
tags: [Rust, Web Dev]
categories: tutorials # a single term works too
---
```

Each term gets a paginated page at `/<key>/<term>/`, `/<key>/<term>/page/2/`, ... The term is slugified in URLs: it is lowercased and each run of characters that are not letters or digits becomes `-`, so `Web Dev` becomes `/tags/web-dev/`. Terms with the same slug are merged. `term.hbs` receives `pagination` (as above), `collections`, and `taxonomy`:

- `taxonomy.name`: the taxonomy (e.g. `tags`)
- `taxonomy.term`: `{ name, slug, url, count }`, where `name` is the term as first written in frontmatter

If `pages/<key>/taxonomy.hbs` exists, an index of all terms is rendered at `/<key>/`. It receives `taxonomy.name` and `taxonomy.terms`, a list of `{ name, slug, url, count, items }` sorted by slug.

Link to a term page from any template with the `term_url` helper:

```hbs
{{#each fm.tags}}<a href="{{term_url "tags" this}}">{{this}}</a>{{/each}}
```

A value that can never be a term (missing, `null`, or no letters or digits) renders an empty URL and logs a warning. Otherwise `term_url` only builds the URL and does not check that the page exists: a term used only on items outside the taxonomy's `collections`, or a taxonomy without `term.hbs`, gets a link with no page behind it.

The build fails if two pages would be written to the same file, e.g. a `content/posts/index.md` item and the `/posts/` listing (with the default `url_style`; with `"directory"` the item is at `/posts/index/` instead), a `[listings.posts]` listing and a `[taxonomies.posts]` index, or (with `url_style = "directory"`) a `pages/posts.hbs` page and the `/posts/` listing.

Listing and taxonomy pages are added to the sitemap.

### Details page context

Besides `fm`, the details template receives:

- `content`: the rendered HTML of the document
- `excerpt`: the HTML of everything before a `<!-- more -->` marker, or of the first paragraph when there is no marker (`null` if the document has no paragraph)
- `toc`: the document headings, as a list of `{ level, id, text }`
- `toc_html`: the same headings rendered as nested `<ul>` lists linking to each heading

```hbs
<nav>{{{toc_html}}}</nav>

{{#each toc}}
  <a class="toc-level-{{level}}" href="#{{id}}">{{text}}</a>
{{/each}}
```

TOC links point to heading ids, so enable `heading_ids` (see below) or render ids in your own heading tag templates with `props.id`.

## Custom helpers

Define your own template helpers without recompiling Balzac by writing them in [Rhai](https://rhai.rs/book/), a small scripting language. Every `<name>.rhai` file in the helpers directory (`./helpers` by default, set `helpers_directory` to change it) registers a helper called `<name>`; other files are ignored.

A script receives the helper arguments as the `params` array and the named arguments as the `hash` map, and the value of its last expression is the helper's result:

```rhai
// helpers/truncate.rhai
let text = params[0];
let length = if "length" in hash { hash.length } else { 80 };
if text.len() <= length { text } else { text.sub_string(0, length) + "…" }
```

```hbs
<p>{{truncate fm.description length=120}}</p>
```

A script can return any value, not just strings, so helpers also work as subexpressions, e.g. `{{#each (my_helper ...)}}` with a script returning an array. A script named like a built-in helper (`term_url`, `vite_url`, ...) replaces it.

If a script doesn't compile, the build fails with an error naming the file and the position of the problem. Errors raised while a helper runs name both the template calling it and the script:

```txt
invalid helper script helpers/truncate.rhai: Expecting ';' to terminate this statement (line 4, position 1)
template error in pages/index.hbs:1:4: helper `truncate` (helpers/truncate.rhai) failed: Function not found: nope (&str | ImmutableString | String) (line 1, position 11)
```

`balzac dev` rebuilds the whole site when a helper changes.

Script helpers are part of the default `script-helpers` cargo feature, which adds about 2 MiB to the binary. To build Balzac without them, use `cargo install balzac --no-default-features`; the helpers directory is then ignored with a warning.

## Markdown

Markdown is rendered with [comrak](https://github.com/kivikakk/comrak), with GitHub-flavored extensions (tables, strikethrough, autolinks, task lists). Additional features are configured in the `[markdown]` section of `balzac.toml`; all keys are optional:

```toml
[markdown]
syntax_highlighting = true            # default: true
highlight_theme = "base16-ocean.dark" # default: none, no stylesheet is written
heading_ids = true                    # default: false
heading_anchors = true                # default: false
footnotes = true                      # default: false
```

### Syntax highlighting

Fenced code blocks with a known language (` ```rust `) are highlighted at build time with [syntect](https://github.com/trishume/syntect). Highlighting uses CSS classes instead of inline styles, so it can be themed from your stylesheets: the `<pre>` gets the `hl-code` class and tokens are wrapped in spans with `hl-` prefixed TextMate scope classes (`hl-keyword`, `hl-string`, `hl-comment`, ...). Code blocks without a language, or with an unknown one, are left unhighlighted.

To get started with a theme, set `highlight_theme` and Balzac writes a matching stylesheet to `<output_directory>/highlight.css`:

```html
<link rel="stylesheet" href="/highlight.css">
```

Available themes: `base16-eighties.dark`, `base16-mocha.dark`, `base16-ocean.dark`, `base16-ocean.light`, `InspiredGitHub`, `Solarized (dark)`, `Solarized (light)`. You can also copy the generated file into your assets and adapt it.

### Heading ids and anchors

With `heading_ids`, headings get an `id` generated from their text, GitHub-style (`## Getting Started` becomes `id="getting-started"`, duplicates get a `-1`, `-2`, ... suffix). `heading_anchors` also enables ids and adds a link to each heading:

```html
<h2 id="getting-started">Getting Started<a class="heading-anchor" href="#getting-started" aria-hidden="true" tabindex="-1">#</a></h2>
```

### Footnotes

`footnotes` enables footnote references (`Text[^1]`) and definitions (`[^1]: The note.`), rendered as a `<section class="footnotes">` at the end of the document.

### Tag templates

Handlebars files in the tags directory (`./tags` by default) replace the HTML of markdown elements, e.g. `tags/a.hbs` for links. Templates receive the rendered children as `content` and element data as `props`. Markdown features are exposed there too:

- headings (`h1.hbs` to `h6.hbs`): `props.id` is the heading id (also when `heading_ids` is off; `null` for headings without text, like `## [^1]`) and `props.level` its level
- code blocks (`pre.hbs`): `props.lang` is the code block language, and `props.highlighted` the highlighted HTML (the escaped code when highlighting is disabled or the language is unknown)

```hbs
{{!-- tags/pre.hbs --}}
<figure class="code" data-lang="{{props.lang}}">
  <pre class="hl-code"><code>{{{props.highlighted}}}</code></pre>
</figure>
```

## Static Page Frontmatter

Static pages (`pages/*.hbs`, `*.handlebars`, `*.html`) can also start with an optional YAML frontmatter block. It is stripped from the output and exposed to the template under `fm`, alongside your `global` data and `collections`:

```hbs
---
title: "About us"
---
<h1>{{fm.title}}</h1>
```

Invalid YAML in a page's frontmatter fails the build with the page path in the error.

## Sitemap Frontmatter

Both static pages and collection items can set sitemap metadata in their frontmatter:

- `lastmod`: string, e.g. `"2024-01-20"`
- `priority`: number, e.g. `0.8` (falls back to `[sitemap].default_priority`)
- `changefreq`: string, e.g. `weekly` (falls back to `[sitemap].default_changefreq`)
- `sitemap_exclude`: `true` to leave the page out of the sitemap (it is still rendered)

The `404` page is always left out of the sitemap, even if its frontmatter sets these keys.

Sitemap metadata is read when pages are discovered, before the `render_before` hook runs. A `render_before` hook that rewrites a static page's frontmatter changes the rendered page but not its sitemap entry. Collection items use the frontmatter saved at discovery for both rendering and the sitemap. To change frontmatter from a hook, use `build_before`, which runs before discovery.

## Benchmarks

The [Benchmark workflow](.github/workflows/benchmark.yml) times the latest balzac, [Zola](https://www.getzola.org/) and [Cobalt](https://cobalt-org.github.io/) releases building the same generated blog. It runs after each release and updates the results below; it can also be run from the Actions tab. Adding the `bench` label to a pull request benchmarks its build against them too: the results are posted in a comment (updated on later runs) and the label is removed, so it can be added again.

<!-- benchmark:start -->
Results are added here after the next release.
<!-- benchmark:end -->

To run it locally with `balzac`, `zola`, `cobalt` and [hyperfine](https://github.com/sharkdp/hyperfine) 2.x on your `PATH`:

```sh
bench/run.sh 100 1000   # number of posts per site; results go to ./results
```

## Development

All required tooling can be installed using [mise](https://mise.jdx.dev/) with `mise install`
