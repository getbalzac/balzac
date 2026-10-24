#!/usr/bin/env bash
# Publishes the packages filled by prepare.mjs. The platform packages go first:
# once balzac is public, installs resolve its optional dependencies, and any
# that are missing would be skipped silently.
# Versions that are already on npm are skipped, so a failed run can be retried.
set -euo pipefail

npm_dir="$(cd "$(dirname "$0")/.." && pwd)"

publish() {
  local dir="$1" name version
  name="$(node -p "require('$dir/package.json').name")"
  version="$(node -p "require('$dir/package.json').version")"
  if npm view "$name@$version" version >/dev/null 2>&1; then
    echo "$name@$version is already published, skipping"
    return
  fi
  # npm refuses to publish a prerelease without a dist-tag; keep it off "latest"
  if [[ "$version" == *-* ]]; then
    npm publish "$dir" --tag next
  else
    npm publish "$dir"
  fi
}

for dir in "$npm_dir"/platforms/*/; do
  publish "${dir%/}"
done
publish "$npm_dir/balzac"
