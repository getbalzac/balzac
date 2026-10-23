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
- direct changes to markdown files in `content/<collection>/` re-render all pages (any page can list collection items through `collections`) and trigger a full-page reload, without recreating the output directory
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
- url_style (optional): how page URLs map to output files, see [URL Style](#url-style)

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
- With `directory`, a top-level `pages/404.hbs` stays at `dist/404.html` so hosts can find the error page.
- With `directory`, `balzac dev` redirects `/about` to `/about/`, as static hosts do.

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

## Development

All required tooling can be installed using [mise](https://mise.jdx.dev/) with `mise install`
