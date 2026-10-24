#!/usr/bin/env python3
"""Replace the benchmark results in the README with a summary written by run.sh.

Usage: update-readme.py results/summary.md README.md

The results go between the `<!-- benchmark:start -->` and `<!-- benchmark:end -->`
markers. The summary's own `##` title is dropped, as the README section has one.
"""

import sys
from datetime import date
from pathlib import Path

START = "<!-- benchmark:start -->"
END = "<!-- benchmark:end -->"


def main():
    summary_path, readme_path = map(Path, sys.argv[1:3])
    lines = summary_path.read_text().strip().splitlines()
    if lines and lines[0].startswith("## "):
        lines = lines[1:]
    results = "\n".join(lines).strip()

    readme = readme_path.read_text()
    if readme.count(START) != 1 or readme.count(END) != 1 or readme.index(START) > readme.index(END):
        sys.exit(f"{readme_path} needs exactly one {START} ... {END} block")
    before, rest = readme.split(START)
    _, after = rest.split(END)
    block = f"{START}\nLast run on {date.today().isoformat()}.\n\n{results}\n{END}"
    readme_path.write_text(before + block + after)


if __name__ == "__main__":
    main()
