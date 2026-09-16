#!/bin/sh
# One baseline run — native filesystem, Git, Jujutsu or folder replication —
# appended to a results file.
#
# Thin on purpose, like run.sh beside it: the version pinning, the correctness
# gate, the footprint method and the rejection rules live in
# benchmarks/baselines/**, and a runner that reimplemented any of them would be a
# second, weaker instrument. This script pins the three things a human gets
# wrong — forgetting the release binary the generated corpora come from,
# forgetting where rows go, and running a baseline that is not the pinned one.
#
# Usage:
#   benchmarks/runners/baselines.sh --workload one-byte-edit [--baseline git-worktree] [...]
#
# Every flag is passed through to `benchmarks/baselines/run.mjs --help`.
#
# Where rows go, and why not into the repository: the publishing policy refuses a
# row measured from a dirty worktree, and `git status --porcelain` does not care
# who made the tree dirty. A default --out inside the checkout therefore refuses
# every run after the first. The default is out of tree:
#   $MESH_BENCH_REPORTS, else $XDG_STATE_HOME/mesh-bench/reports,
#   else $HOME/.local/state/mesh-bench/reports.
# Publishing a row into benchmarks/baselines/reports/ is a separate, deliberate
# commit; see benchmarks/baselines/README.md.
#
# Exit codes: 0 accepted, 1 refused (unverified, unpinned or not publishable),
#             2 the invocation was wrong.
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repo_root"

canonical_dir() {
    CDPATH='' cd -- "$1" 2>/dev/null && pwd -P
}

is_inside_repo() {
    probe=$1
    while [ ! -d "$probe" ]; do
        parent=$(dirname -- "$probe")
        [ "$parent" != "$probe" ] || return 1
        probe=$parent
    done
    probe=$(canonical_dir "$probe") || return 1
    root=$(canonical_dir "$repo_root") || return 1
    [ "$probe" != "$root" ] || return 0
    case "$probe" in
        "$root"/*) return 0 ;;
        *) return 1 ;;
    esac
}

if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
    export PATH
fi

workload=""
has_out=0
out=""
previous=""
for argument in "$@"; do
    case "$previous" in
        --workload) workload="$argument" ;;
        --out) out="$argument" ;;
    esac
    case "$argument" in
        --out) has_out=1 ;;
    esac
    previous="$argument"
done

if [ -z "$workload" ]; then
    echo "baselines.sh: --workload NAME is required" >&2
    node benchmarks/baselines/run.mjs 2>&1 | sed 1d >&2 || true
    exit 2
fi

if [ "$has_out" -eq 0 ]; then
    if [ -n "${MESH_BENCH_REPORTS:-}" ]; then
        reports_dir=$MESH_BENCH_REPORTS
    elif [ -n "${XDG_STATE_HOME:-}" ]; then
        reports_dir="$XDG_STATE_HOME/mesh-bench/reports"
    else
        reports_dir="$HOME/.local/state/mesh-bench/reports"
    fi
    if is_inside_repo "$reports_dir"; then
        echo "baselines.sh: the default results directory resolves inside the checkout:" >&2
        echo "  $reports_dir" >&2
        echo "  Rows written there make the worktree dirty, and the next run is refused." >&2
        echo "  Set MESH_BENCH_REPORTS to a directory outside $repo_root." >&2
        exit 2
    fi
    mkdir -p "$reports_dir"
    out="$reports_dir/baseline-${workload}.jsonl"
    set -- "$@" --out "$out"
    echo "baselines.sh: rows go to $out (override with --out or MESH_BENCH_REPORTS)" >&2
elif is_inside_repo "$(dirname -- "$out")"; then
    echo "baselines.sh: --out $out is inside the checkout — this run will leave the" >&2
    echo "  worktree dirty, and the next publishable run is refused until the" >&2
    echo "  row is committed or removed." >&2
fi

# The generated corpora (W1-W6) come out of the same binary the Mesh runs use.
# Building it here rather than trusting a stale target/ is what makes "identical
# corpora" a fact rather than an assumption.
echo "baselines.sh: building --release mesh-bench (the corpora come from it)" >&2
cargo build -q --release -p mesh-bench --bin mesh-bench

exec node benchmarks/baselines/run.mjs "$@"
