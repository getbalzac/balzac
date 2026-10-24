#!/usr/bin/env bash
# Publishes the packages filled by prepare.mjs. The platform packages go first:
# once balzac is public, installs resolve its optional dependencies, and any
# that are missing would be skipped silently.
# Versions that are already on npm are skipped, so a failed run can be retried.
set -euo pipefail

npm_dir="$(cd "$(dirname "$0")/.." && pwd)"

field() {
  node -p "require('$1/package.json').$2"
}

is_public() {
  npm view "$1" version >/dev/null 2>&1
}

publish() {
  local dir="$1" name version output
  name="$(field "$dir" name)"
  version="$(field "$dir" version)"
  if is_public "$name@$version"; then
    echo "$name@$version is already published, skipping"
    return
  fi
  local args=("$dir")
  # npm refuses to publish a prerelease without a dist-tag; keep it off "latest"
  if [[ "$version" == *-* ]]; then
    args+=(--tag next)
  fi
  if ! output="$(npm publish "${args[@]}" 2>&1)"; then
    echo "$output"
    # npm processes new versions for a few minutes before `npm view` sees them;
    # a retry in that window gets a version conflict for a version that is on
    # its way ("Cannot publish over previously staged version"; npm has worded
    # it differently over time, so match the error codes)
    case "$output" in
      *"code E409"* | *EPUBLISHCONFLICT* | *"409 Conflict"*)
        echo "$name@$version is already published and still being processed, skipping"
        return
        ;;
    esac
    return 1
  fi
  echo "$output"
}

# waits until every platform package is installable, so that balzac never
# points at versions that npm is still processing
wait_until_public() {
  local deadline=$((SECONDS + 900)) dir spec
  for dir in "$npm_dir"/platforms/*/; do
    spec="$(field "$dir" name)@$(field "$dir" version)"
    until is_public "$spec"; do
      if ((SECONDS > deadline)); then
        echo "$spec is still not available on npm, not publishing balzac" >&2
        return 1
      fi
      echo "waiting for $spec to become available"
      sleep 15
    done
  done
}

for dir in "$npm_dir"/platforms/*/; do
  publish "${dir%/}"
done
wait_until_public
publish "$npm_dir/balzac"
