#!/usr/bin/env bash
# Checks the npm packaging without building balzac: fakes dist's archives with
# a shell script per target, runs prepare.mjs on a copy of npm/, packs every
# package, then installs balzac and this machine's platform package into a
# scratch project and runs the launcher. The repository is left untouched.
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/../.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
version="0.0.0-smoke.0"

node --check "$repo_dir/npm/balzac/bin/balzac.js"
node --check "$repo_dir/npm/scripts/prepare.mjs"
bash -n "$repo_dir/npm/scripts/publish.sh"

mkdir -p "$tmp/repo" "$tmp/archives" "$tmp/packs" "$tmp/project"
cp -R "$repo_dir/npm" "$repo_dir/LICENSE" "$repo_dir/README.md" "$tmp/repo/"

# same layout as dist: balzac-<target>/balzac in .tar.xz, a flat balzac.exe in .zip
fake="$tmp/fake"
printf '#!/bin/sh\necho "fake balzac $*"\nexit 3\n' >"$fake"
chmod +x "$fake"
targets="$(node -p "Object.values(require('$repo_dir/npm/scripts/targets.json')).sort().join(' ')")"
dist_targets="$(sed -n 's/^targets = \[\(.*\)\]$/\1/p' "$repo_dir/dist-workspace.toml" | tr -d '",' | tr ' ' '\n' | sort | xargs)"
if [[ "$targets" != "$dist_targets" ]]; then
  echo "npm/scripts/targets.json ($targets) doesn't match the targets in dist-workspace.toml ($dist_targets)" >&2
  exit 1
fi
for target in $targets; do
  if [[ "$target" == *windows* ]]; then
    cp "$fake" "$tmp/balzac.exe"
    (cd "$tmp" && zip -q "archives/balzac-$target.zip" balzac.exe)
  else
    mkdir -p "$tmp/balzac-$target"
    cp "$fake" "$tmp/balzac-$target/balzac"
    tar -cJf "$tmp/archives/balzac-$target.tar.xz" -C "$tmp" "balzac-$target"
  fi
done

node "$tmp/repo/npm/scripts/prepare.mjs" "$version" "$tmp/archives"
for dir in "$tmp/repo/npm/balzac" "$tmp/repo/npm/platforms"/*/; do
  (cd "$tmp/packs" && npm pack --silent "${dir%/}" >/dev/null)
done

host="$(node -p 'process.platform + "-" + process.arch + (process.platform === "linux" ? "-gnu" : "")')"
cd "$tmp/project"
npm init --yes >/dev/null
# the @balzac/* versions don't exist on the registry; npm skips those optional
# dependencies and uses the local tarball for this machine's one
npm install --silent --no-audit --no-fund "$tmp/packs/balzac-$version.tgz" "$tmp/packs/balzac-$host-$version.tgz"

status=0
output="$(./node_modules/.bin/balzac hello world)" || status=$?
if [[ "$output" != "fake balzac hello world" || "$status" != 3 ]]; then
  echo "unexpected launcher result: status $status, output: $output" >&2
  exit 1
fi
echo "npm packaging smoke test passed ($host)"
