# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
## [0.1.12] - 2026-10-24

### 🐛 Bug Fixes

- *(npm)* Don't fail or publish balzac while npm is still processing platform packages
## [0.1.11] - 2026-10-24

### 🚀 Features

- Expose collections to page templates
- Add 404 page support and robots.txt generation
- Add url_style option for directory-style clean URLs
- Add paginated listings and taxonomy pages
- Syntax highlighting, heading ids, toc, footnotes and excerpts for markdown
- Support YAML frontmatter in static pages
- Generate RSS and Atom feeds for collections
- Report build errors with file context instead of panicking
- Support nested static pages
- Reject unknown keys in balzac.toml
- Run hooks in dev mode when opted in with `[hooks] dev = true`
- Flexible collections (slug, drafts, custom templates, nested folders)
- Built-in template helpers (url_for, asset, date, markdown, sort, limit, where, json, slugify)
- User-defined template helpers written in Rhai
- Publish npm binaries as per-platform optional dependencies

### 🐛 Bug Fixes

- Copy assets directory recursively
- Restore url_style, dropped when rebasing listings onto it

### 📚 Documentation

- Document markdown options, toc and excerpt

### ⚡ Performance

- Compare borrowed strings when sorting collection items

### ⚙️ Miscellaneous Tasks

- Set up janitor PR reviews
- Enforce stricter clippy lints
- Regenerate janitor workflow for v2.1.0
## [0.1.10] - 2026-06-04

### 🚀 Features

- Dev server
- Customizable HTML tag renderers for markdown content ([#23](https://github.com/getbalzac/balzac/pull/23))
## [0.1.9] - 2026-01-26

### 🚀 Features

- Migrate markdown parse to comrak

### 💼 Other

- Release optimizations
## [0.1.8] - 2026-01-25

### 🐛 Bug Fixes

- Npm release
## [0.1.7] - 2026-01-25

### 💼 Other

- Cliff & release-plz
- Dist
- Brew publish
- Npm
