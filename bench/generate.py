#!/usr/bin/env python3
"""Generate the same synthetic blog for every benchmarked static site generator.

Each site has `--posts` markdown posts (frontmatter, headings, lists, a table,
links and a highlighted Rust code block), a page per post and an index listing
every post. Templates are kept equivalent so the builds do comparable work.

Usage: generate.py --posts 1000 --out ./sites
Writes ./sites/balzac, ./sites/zola and ./sites/cobalt.
"""

import argparse
import random
import shutil
from datetime import date, timedelta
from pathlib import Path

TAGS = ["rust", "web", "performance", "tooling", "design", "cli", "markdown", "templates", "release", "notes"]
WORDS = (
    "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et "
    "dolore magna aliqua enim ad minim veniam quis nostrud exercitation ullamco laboris nisi aliquip ex ea "
    "commodo consequat duis aute irure in reprehenderit voluptate velit esse cillum fugiat nulla pariatur"
).split()

CODE = """fn fibonacci(n: u64) -> u64 {
    // iterative, so large inputs don't overflow the stack
    let (mut a, mut b) = (0, 1);
    for _ in 0..n {
        (a, b) = (b, a + b);
    }
    a
}

fn main() {
    let values: Vec<u64> = (0..10).map(fibonacci).collect();
    println!("{values:?}");
}"""


def sentence(rng, n=12):
    words = [rng.choice(WORDS) for _ in range(n)]
    words[rng.randrange(n)] = f"**{rng.choice(WORDS)}**"
    words[rng.randrange(n)] = f"[{rng.choice(WORDS)}](https://example.com/{rng.choice(WORDS)})"
    return " ".join(words).capitalize() + "."


def paragraph(rng):
    return " ".join(sentence(rng, rng.randint(8, 16)) for _ in range(rng.randint(3, 6)))


def body(rng):
    parts = []
    for section in range(3):
        parts.append(f"## Section {section + 1}")
        parts.append(paragraph(rng))
        parts.append(paragraph(rng))
    parts.append("\n".join(f"- {sentence(rng, 6)}" for _ in range(5)))
    parts.append("```rust\n" + CODE + "\n```")
    rows = "\n".join(f"| {rng.choice(WORDS)} | {rng.randint(1, 999)} |" for _ in range(4))
    parts.append("| Name | Value |\n| ---- | ----- |\n" + rows)
    parts.append(paragraph(rng))
    return "\n\n".join(parts) + "\n"


def posts(count):
    rng = random.Random(42)
    start = date(2020, 1, 1)
    for i in range(count):
        yield {
            "slug": f"post-{i:05}",
            "title": f"Post number {i}: {' '.join(rng.choice(WORDS) for _ in range(3))}",
            "date": (start + timedelta(days=i)).isoformat(),
            "description": sentence(rng, 10).replace("**", "").split("[")[0].strip(),
            "tags": rng.sample(TAGS, 2),
            "body": body(rng),
        }


def write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)


def balzac(root, items):
    write(root / "balzac.toml", "")
    write(
        root / "pages/index.hbs",
        """<!DOCTYPE html>
<html><head><title>Benchmark</title></head>
<body>
<h1>All posts</h1>
<ul>
{{#each collections.posts}}
<li><a href="{{url}}">{{fm.title}}</a> <time>{{fm.date}}</time><p>{{fm.description}}</p></li>
{{/each}}
</ul>
</body></html>
""",
    )
    write(
        root / "pages/posts/details.hbs",
        """<!DOCTYPE html>
<html><head><title>{{fm.title}}</title></head>
<body>
<article>
<h1>{{fm.title}}</h1>
<time>{{fm.date}}</time>
<ul>{{#each fm.tags}}<li>{{this}}</li>{{/each}}</ul>
{{{content}}}
</article>
</body></html>
""",
    )
    for p in items:
        tags = ", ".join(f'"{t}"' for t in p["tags"])
        write(
            root / f"content/posts/{p['slug']}.md",
            f'---\ntitle: "{p["title"]}"\ndate: {p["date"]}\ndescription: "{p["description"]}"\n'
            f"tags: [{tags}]\n---\n\n{p['body']}",
        )


def zola(root, items):
    write(
        root / "config.toml",
        """base_url = "https://example.com"
compile_sass = false
build_search_index = false

[markdown.highlighting]
style = "class"
theme = "github-dark"
""",
    )
    write(root / "content/_index.md", "+++\n+++\n")
    write(root / "content/posts/_index.md", '+++\nsort_by = "date"\nrender = false\n+++\n')
    write(
        root / "templates/index.html",
        """<!DOCTYPE html>
<html><head><title>Benchmark</title></head>
<body>
<h1>All posts</h1>
<ul>
{% set posts = get_section(path="posts/_index.md") %}
{% for page in posts.pages %}
<li><a href="{{ page.permalink }}">{{ page.title }}</a> <time>{{ page.date }}</time><p>{{ page.description }}</p></li>
{% endfor %}
</ul>
</body></html>
""",
    )
    write(
        root / "templates/page.html",
        """<!DOCTYPE html>
<html><head><title>{{ page.title }}</title></head>
<body>
<article>
<h1>{{ page.title }}</h1>
<time>{{ page.date }}</time>
<ul>{% for tag in page.extra.tags %}<li>{{ tag }}</li>{% endfor %}</ul>
{{ page.content | safe }}
</article>
</body></html>
""",
    )
    write(root / "templates/section.html", "{{ section.content | safe }}\n")
    for p in items:
        tags = ", ".join(f'"{t}"' for t in p["tags"])
        write(
            root / f"content/posts/{p['slug']}.md",
            f'+++\ntitle = "{p["title"]}"\ndate = {p["date"]}\ndescription = "{p["description"]}"\n\n'
            f"[extra]\ntags = [{tags}]\n+++\n\n{p['body']}",
        )


def cobalt(root, items):
    write(root / "_cobalt.yml", "destination: _site\nsyntax_highlight:\n  enabled: true\n")
    write(
        root / "index.liquid",
        """<!DOCTYPE html>
<html><head><title>Benchmark</title></head>
<body>
<h1>All posts</h1>
<ul>
{% for post in collections.posts.pages %}
<li><a href="/{{ post.permalink }}">{{ post.title }}</a> <time>{{ post.published_date }}</time><p>{{ post.description }}</p></li>
{% endfor %}
</ul>
</body></html>
""",
    )
    write(
        root / "_layouts/post.liquid",
        """<!DOCTYPE html>
<html><head><title>{{ page.title }}</title></head>
<body>
<article>
<h1>{{ page.title }}</h1>
<time>{{ page.published_date }}</time>
<ul>{% for tag in page.tags %}<li>{{ tag }}</li>{% endfor %}</ul>
{{ page.content }}
</article>
</body></html>
""",
    )
    for p in items:
        tags = ", ".join(f'"{t}"' for t in p["tags"])
        write(
            root / f"posts/{p['slug']}.md",
            f'---\ntitle: "{p["title"]}"\npublished_date: "{p["date"]} 00:00:00 +0000"\n'
            f'description: "{p["description"]}"\ntags: [{tags}]\nlayout: post.liquid\n---\n\n{p["body"]}',
        )


GENERATORS = {"balzac": balzac, "zola": zola, "cobalt": cobalt}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--posts", type=int, default=1000)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--only", choices=GENERATORS, action="append", help="generate only these sites")
    args = parser.parse_args()

    items = list(posts(args.posts))
    for name in args.only or GENERATORS:
        root = args.out / name
        shutil.rmtree(root, ignore_errors=True)
        GENERATORS[name](root, items)
        print(f"generated {root} ({args.posts} posts)")


if __name__ == "__main__":
    main()
