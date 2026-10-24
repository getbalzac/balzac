#!/usr/bin/env bash
# Benchmark balzac, zola and cobalt building the same generated site.
#
# Usage: bench/run.sh [POSTS...]   (default: 100 1000 2500)
# Needs balzac, zola, cobalt, hyperfine (2.x) and python3 on PATH.
# Writes results/<posts>.{md,json} and results/summary.md.
# Set RUNS / WARMUP to change how many builds are timed.
# Set BALZAC_PR to the path of another balzac binary (e.g. a pull request build)
# to benchmark it too; timings are then compared with the released balzac.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
work="${WORK_DIR:-$(mktemp -d)}"
results="${RESULTS_DIR:-$PWD/results}"
runs="${RUNS:-10}"
warmup="${WARMUP:-2}"
sizes=("$@")
[ ${#sizes[@]} -eq 0 ] && sizes=(100 1000 2500)

# name, site directory, output directory, build command
tools=("balzac|balzac|dist|balzac build")
[ -n "${BALZAC_PR:-}" ] && tools+=("balzac (PR)|balzac|dist|'$BALZAC_PR' build")
tools+=("zola|zola|public|zola build" "cobalt|cobalt|_site|cobalt build")

mkdir -p "$results"
summary="$results/summary.md"

# max resident memory of one build, in MiB; GNU time on Linux, BSD time on macOS
peak_mem() {
    local dir="$1" command="$2"
    if /usr/bin/time -v true >/dev/null 2>&1; then
        (cd "$dir" && eval "/usr/bin/time -v $command" 2>&1 >/dev/null) | awk -F': ' '/Maximum resident/ { printf "%.1f", $2 / 1024 }'
    else
        (cd "$dir" && eval "/usr/bin/time -l $command" 2>&1 >/dev/null) | awk '/maximum resident/ { printf "%.1f", $1 / 1048576 }'
    fi
}

{
    echo "## Static site generator benchmark"
    echo
    echo "| Tool | Version |"
    echo "| ---- | ------- |"
    # balzac has no --version flag, so the workflow passes the release it installed
    echo "| balzac | ${BALZAC_VERSION:-unknown} |"
    [ -n "${BALZAC_PR:-}" ] && echo "| balzac (PR) | ${BALZAC_PR_VERSION:-unknown} |"
    echo "| zola | $(zola --version | awk '{ print $NF }') |"
    echo "| cobalt | $(cobalt --version | awk '{ print $NF }') |"
    echo
    echo "Each site has the given number of markdown posts (headings, lists, a table, a highlighted Rust code block), a page per post and an index listing them all. Output is deleted before every build. $runs timed runs after $warmup warmup runs, on \`$(uname -sm)\` with $(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu) CPUs. Changes are relative to the released balzac."
} >"$summary"

for posts in "${sizes[@]}"; do
    sites="$work/$posts"
    python3 -I "$here/generate.py" --posts "$posts" --out "$sites"

    # cobalt has no flag for the project root, so every build runs in its site directory
    args=()
    for tool in "${tools[@]}"; do
        IFS='|' read -r name site output command <<<"$tool"
        args+=(--command-name "$name" --prepare "rm -rf '$sites/$site/$output'" "cd '$sites/$site' && $command")
    done
    hyperfine \
        --shell=default \
        --warmup "$warmup" \
        --runs "$runs" \
        --export-markdown "$results/$posts.md" \
        --export-json "$results/$posts.json" \
        "${args[@]}"

    memory=()
    for tool in "${tools[@]}"; do
        IFS='|' read -r name site output command <<<"$tool"
        memory+=("$name $(peak_mem "$sites/$site" "$command") MiB")
    done

    {
        echo
        echo "### $posts posts"
        echo
        cat "$results/$posts.md"
        echo
        joined="$(printf '%s, ' "${memory[@]}")"
        echo "Peak memory: ${joined%, }"
    } >>"$summary"
done

cat "$summary"
