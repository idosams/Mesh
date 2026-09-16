#!/bin/sh
# One benchmark run, release profile, appended to a results file.
#
# Thin on purpose: the metadata capture, the correctness gate and the rejection
# rules live in crates/mesh-bench, and a runner that reimplemented any of them
# would be a second, weaker instrument. This script only pins the two things a
# human gets wrong — building unoptimised, and forgetting where rows go.
#
# Usage:
#   benchmarks/runners/run.sh --workload blob-scan [--iterations 200] [...]
#
# Every flag is passed through to `mesh-bench run`; see `mesh-bench --help`.
#
# Where rows go, and why not into the repository:
#   the publishing policy refuses a row measured from a dirty worktree, and
#   `git status --porcelain` does not care who made the tree dirty. A default
#   --out inside the checkout therefore refuses every run after the first — the
#   harness poisoning itself with its own output, and blaming the commit for it.
#   So the default is out of tree: $MESH_BENCH_REPORTS, else
#   $XDG_STATE_HOME/mesh-bench/reports, else $HOME/.local/state/mesh-bench/reports.
#   Publishing a row into benchmarks/reports/ is a separate, deliberate commit;
#   see benchmarks/runners/README.md.
#
# Exit codes: 0 accepted, 1 refused (incomplete, unverified or not publishable),
#             2 the invocation was wrong.
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repo_root"

# Absolute, symlink-resolved, with a trailing slash so the prefix test below
# cannot match a sibling directory whose name merely starts with the repo's.
canonical_dir() {
    CDPATH='' cd -- "$1" 2>/dev/null && pwd -P
}

# True when $1 (a directory that may not exist yet) resolves inside the checkout.
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

# rustup's default location, used only when cargo is not already on PATH (a
# non-interactive shell frequently is not the shell the toolchain was installed in).
if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
    export PATH
fi

workload=""
out=""
has_out=0
has_iterations=0
previous=""
for argument in "$@"; do
    case "$previous" in
        --workload) workload="$argument" ;;
        --out) out="$argument" ;;
    esac
    case "$argument" in
        --out) has_out=1 ;;
        --iterations) has_iterations=1 ;;
    esac
    previous="$argument"
done

if [ -z "$workload" ]; then
    echo "run.sh: --workload NAME is required" >&2
    cargo run -q --release -p mesh-bench --bin mesh-bench -- workloads >&2 || true
    exit 2
fi

set -- "$@"
if [ "$has_iterations" -eq 0 ]; then
    # p99 over fewer than 200 samples is a handful of samples wearing a
    # percentile's name, and it will breach the stated band.
    set -- "$@" --iterations 200
fi

if [ "$has_out" -eq 0 ]; then
    if [ -n "${MESH_BENCH_REPORTS:-}" ]; then
        reports_dir=$MESH_BENCH_REPORTS
    elif [ -n "${XDG_STATE_HOME:-}" ]; then
        reports_dir="$XDG_STATE_HOME/mesh-bench/reports"
    else
        reports_dir="$HOME/.local/state/mesh-bench/reports"
    fi
    # The claim "the default output is out of tree" is checked, not asserted:
    # $HOME or $MESH_BENCH_REPORTS can point anywhere, including in here.
    if is_inside_repo "$reports_dir"; then
        echo "run.sh: the default results directory resolves inside the checkout:" >&2
        echo "  $reports_dir" >&2
        echo "  Rows written there make the worktree dirty, and the next run is refused." >&2
        echo "  Set MESH_BENCH_REPORTS to a directory outside $repo_root." >&2
        exit 2
    fi
    mkdir -p "$reports_dir"
    out="$reports_dir/${workload}.jsonl"
    set -- "$@" --out "$out"
    echo "run.sh: rows go to $out (override with --out or MESH_BENCH_REPORTS)" >&2
elif is_inside_repo "$(dirname -- "$out")"; then
    # Not fatal: publishing a row into benchmarks/reports/ is a real thing to
    # want. But the next run is refused until that row is committed, and being
    # told so now beats being told "measured from a dirty worktree" later.
    echo "run.sh: --out $out is inside the checkout — this run will leave the" >&2
    echo "  worktree dirty, and the next publishable run is refused until the" >&2
    echo "  row is committed or removed." >&2
fi

echo "run.sh: building --release (a debug profile is not a benchmark)" >&2
cargo build -q --release -p mesh-bench --bin mesh-bench

exec ./target/release/mesh-bench run "$@"
