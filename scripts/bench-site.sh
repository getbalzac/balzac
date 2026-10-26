#!/usr/bin/env bash
# Generates a synthetic site for benchmarking `balzac build`: POSTS markdown
# posts (default 2000) with code blocks and tags, a paginated listing, a tag
# taxonomy, a layout, a partial and a tag template.
#
#   scripts/bench-site.sh /tmp/bench-site [POSTS]
#   time balzac build --root /tmp/bench-site
set -euo pipefail

root=${1:?usage: $0 <directory> [posts]}
posts=${2:-2000}

rm -rf "$root"
mkdir -p "$root"/{pages/posts,pages/tags,layouts,partials,tags,content/posts}

cat > "$root/balzac.toml" <<'TOML'
base_url = "https://example.com"

[global]
site_name = "Bench"

[listings.posts]
per_page = 10

[taxonomies.tags]
per_page = 10
TOML

cat > "$root/layouts/base.hbs" <<'HBS'
<!doctype html>
<html><head><title>{{site_name}}</title></head>
<body>{{> header}}{{> @partial-block}}</body></html>
HBS
cat > "$root/partials/header.hbs" <<'HBS'
<header><a href="{{url_for "/"}}">{{site_name}}</a></header>
HBS
cat > "$root/tags/a.hbs" <<'HBS'
<a class="link" href="{{props.href}}">{{{content}}}</a>
HBS

cat > "$root/pages/index.hbs" <<'HBS'
{{#> base}}<ul>{{#each collections.posts}}{{#if (lt @index 20)}}<li><a href="{{url}}">{{fm.title}}</a></li>{{/if}}{{/each}}</ul>{{/base}}
HBS
cat > "$root/pages/posts/details.hbs" <<'HBS'
{{#> base}}<article><h1>{{fm.title}}</h1><time>{{date fm.date "%Y-%m-%d"}}</time>{{{toc_html}}}{{{content}}}
<ul>{{#each fm.tags}}<li><a href="{{term_url "tags" this}}">{{this}}</a></li>{{/each}}</ul></article>{{/base}}
HBS
cat > "$root/pages/posts/list.hbs" <<'HBS'
{{#> base}}<ul>{{#each pagination.items}}<li><a href="{{url}}">{{fm.title}}</a></li>{{/each}}</ul>
{{#if pagination.next_url}}<a href="{{pagination.next_url}}">next</a>{{/if}}{{/base}}
HBS
cat > "$root/pages/tags/term.hbs" <<'HBS'
{{#> base}}<h1>{{taxonomy.term.name}}</h1><ul>{{#each pagination.items}}<li><a href="{{url}}">{{fm.title}}</a></li>{{/each}}</ul>{{/base}}
HBS
cat > "$root/pages/tags/taxonomy.hbs" <<'HBS'
{{#> base}}<ul>{{#each taxonomy.terms}}<li><a href="{{url}}">{{name}} ({{count}})</a></li>{{/each}}</ul>{{/base}}
HBS

for i in $(seq 1 "$posts"); do
  day=$(( i % 28 + 1 ))
  month=$(( i % 12 + 1 ))
  cat > "$root/content/posts/post-$i.md" <<MD
---
title: Post number $i
date: "2024-$(printf %02d "$month")-$(printf %02d "$day")"
tags: [tag-$(( i % 50 )), topic-$(( i % 7 ))]
---

## Introduction

This is **post $i**. It links to [the previous post](/posts/post-$(( i - 1 ))) and
has some *emphasis*, \`inline code\` and a list:

- first item
- second item with a [link](https://example.com/$i)
- third item

<!-- more -->

## Some code

\`\`\`rust
fn fibonacci(n: u64) -> u64 {
    match n {
        0 | 1 => n,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}

fn main() {
    println!("{}", fibonacci($(( i % 30 ))));
}
\`\`\`

## Conclusion

| column | value |
| ------ | ----- |
| post   | $i    |
| month  | $month |

Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor
incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis
nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.
MD
done

echo "Generated $posts posts in $root"
