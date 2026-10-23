# Balzac

> an experiment in building a static site generator using rust

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

Build the site:

```sh
balzac build
```

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

- direct changes to top-level files in `pages/` trigger a targeted rebuild and reload only for that page
- direct changes to markdown files in `content/<collection>/` re-render all pages (any page can list collection items through `collections`), including listing and taxonomy pages, and trigger a full-page reload, without recreating the output directory; listing pages that are no longer generated (e.g. a tag with no posts left) are removed
- changes to shared files such as `partials/`, `layouts/`, `assets/`, `balzac.toml`, or collection `details.hbs` trigger a full rebuild and full-page reload
- hooks are ignored in dev mode
- missing paths are answered with a `404` status and your `404.html` page if you have one (see [404 Page](#404-page)), otherwise plain text

The dev server defaults to `http://127.0.0.1:4000`.


## Directory Structure

Balzac supports different directories that you are free to create or skip:

- pages_directory (required): directory that stores all the handlebars templates that will be used to create pages
- partials_directory (optional): houses all handlebars partials
- layouts_directory (optional): houses all handlebars layouts



## Config Reference

- output_directory : the directory where the generated website will be sent to
- pages_directory (optional): the directory where static pages will reside (like your index)
- partials_directory (optional): directory where partial templates will reside
- layouts_directory (optional): directory where layout templates will reside
- assets_directory (optional): directory where static assets will reside
- content_directory (optional): directory where content (markdown) will reside
- global: fill this array if you want to have global data available in all the templates and files
- base_url (optional): the public URL of the site, used for the sitemap and robots.txt
- sitemap (optional): see `balzac init --sitemap`
- robots (optional): see [robots.txt](#robotstxt)
- listings (optional): paginated collection listings, see [Listings and pagination](#listings-and-pagination)
- taxonomies (optional): tag/category pages, see [Taxonomies](#taxonomies)
- markdown (optional): markdown rendering options, see [Markdown](#markdown)

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

## 404 Page

Create `pages/404.hbs` (or `.html`/`.handlebars`) and it is rendered to `dist/404.html` like any other page. Most static hosts (GitHub Pages, Netlify, Cloudflare Pages, ...) serve this file automatically for missing paths. The 404 page is always left out of the sitemap, and `balzac dev` serves it with a `404` status for any path that doesn't exist.

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
- Hooks run during `balzac build`
- Hooks are intentionally skipped during `balzac dev`

## Collections

To use a collection, create a subfolder in the content_directory folder with the name of your collection (i.e. posts) and put your markdown files in there.

The next step is to create a file in the pages_directory called <name_of_your_collection>/details.hbs (i.e. posts/details.hbs).

### Frontmatter

All frontmatter present in the collection documents will be available in the template under the fm namespace.

```md
---
title: "Test"
---
```

will be available under `fm.title`

### Listing collection items

Every page (static pages and collection `details.hbs` pages) has access to all collections under the `collections` namespace. `collections.<name>` is an array of items with:

- `url`: the item's URL (e.g. `/posts/hello`)
- `slug`: the markdown file name without extension (e.g. `hello`)
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

Only collections that have a `details.hbs` page are included.

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

The build fails if two pages would be written to the same file, e.g. a `content/posts/index.md` item and the `/posts/` listing, or a `[listings.posts]` listing and a `[taxonomies.posts]` index.

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

## Development

All required tooling can be installed using [mise](https://mise.jdx.dev/) with `mise install`
