#!/bin/sh
# Runs the README's documented sequence and asserts it survives being run twice.
#
# This exists because of a real defect, not as a formality. run.sh used to
# default --out to benchmarks/reports/<workload>.jsonl, which is inside the
# checkout and not gitignored. The first run succeeded and left an untracked
# file; `git status --porcelain` was then non-empty; env/git.rs set
# repository.dirty; and the publishing policy refused every later run with
# "measured from a dirty worktree" — blaming the commit for the harness's own
# output. The documented first step broke the documented third step.
#
# A comment saying "we fixed that" is not a check. This is the check.
#
# Usage:
#   benchmarks/runners/selftest.sh
#
# Exit codes: 0 all assertions held, 1 an assertion failed,
#             2 the preconditions for running it were not met.
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repo_root"

if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
    export PATH
fi

workload=blob-scan
failures=0
checks=0

pass() {
    checks=$((checks + 1))
    echo "selftest: ok   — $1"
}

fail() {
    checks=$((checks + 1))
    failures=$((failures + 1))
    echo "selftest: FAIL — $1" >&2
}

# The whole point is the interaction with `git status --porcelain`, so a tree
# that is already dirty cannot answer the question. Say so; do not pass anyway.
dirty_before=$(git status --porcelain | wc -l | tr -d ' ')
if [ "$dirty_before" != "0" ]; then
    echo "selftest: the worktree is not pristine ($dirty_before entries)." >&2
    echo "  This check measures whether the harness dirties the tree, so it needs" >&2
    echo "  a clean one to start from. Commit or stash, then re-run." >&2
    git status --porcelain >&2
    exit 2
fi

scratch=$(mktemp -d "${TMPDIR:-/tmp}/mesh-bench-selftest.XXXXXX")
trap 'rm -rf "$scratch"' EXIT

# Exercise the real fallback chain rather than short-circuiting it: leaving
# MESH_BENCH_REPORTS unset and pointing XDG_STATE_HOME at the scratch directory
# makes run.sh resolve the default the same way it does for a user, while
# keeping this script out of $HOME. Setting MESH_BENCH_REPORTS here would have
# made check 1 test the value this script chose, not the one run.sh derives.
unset MESH_BENCH_REPORTS
XDG_STATE_HOME="$scratch/state"
export XDG_STATE_HOME
reports_dir="$XDG_STATE_HOME/mesh-bench/reports"

echo "selftest: building --release" >&2
cargo build -q --release -p mesh-bench --bin mesh-bench

# 0. The guard that makes every fallback safe: a results directory resolving
#    inside the checkout is refused before anything is measured. $HOME and
#    $MESH_BENCH_REPORTS can point anywhere, so the fallbacks are only as safe
#    as this check, and this check is the one that must be live.
set +e
MESH_BENCH_REPORTS="$repo_root/benchmarks/reports" \
    ./benchmarks/runners/run.sh --workload "$workload" --iterations 25 \
    >"$scratch/guard.out" 2>"$scratch/guard.err"
guard_code=$?
set -e
if [ "$guard_code" != "2" ]; then
    fail "an in-checkout results directory should exit 2, exited $guard_code"
elif ! grep -q "resolves inside the checkout" "$scratch/guard.err"; then
    fail "the in-checkout guard did not explain itself:"
    cat "$scratch/guard.err" >&2
elif [ "$(git status --porcelain | wc -l | tr -d ' ')" != "0" ]; then
    fail "the refused run still dirtied the worktree"
else
    pass "a results directory inside the checkout is refused before measuring"
fi

# 1. The default results path is outside the checkout.
#
# Exit codes are captured rather than left to `set -e` throughout: a check that
# aborts the script on the first failure reports one finding and hides the rest,
# and the point of this file is to say everything that is wrong in one pass.
set +e
./benchmarks/runners/run.sh --workload "$workload" >/dev/null 2>"$scratch/run1.err"
run1_code=$?
set -e
if [ "$run1_code" != "0" ]; then
    fail "run 1 — the documented first step — exited $run1_code:"
    cat "$scratch/run1.err" >&2
else
    pass "run 1 with the documented command was accepted"
fi

default_out=$(sed -n 's/^run\.sh: rows go to \(.*\) (override.*$/\1/p' "$scratch/run1.err")
if [ -z "$default_out" ]; then
    fail "run.sh did not report where it wrote rows"
else
    case "$default_out" in
        "$repo_root"/*)
            fail "the default results path is inside the checkout: $default_out" ;;
        /*)
            pass "the default results path is outside the checkout ($default_out)" ;;
        *)
            # Relative means relative to the repo root, which run.sh cd's into.
            fail "the default results path is relative, so it lands in the checkout: $default_out" ;;
    esac
fi

# 2. The first documented run leaves the worktree pristine.
after_first=$(git status --porcelain | wc -l | tr -d ' ')
if [ "$after_first" = "0" ]; then
    pass "run 1 left the worktree pristine"
else
    fail "run 1 dirtied the worktree with $after_first entry(ies):"
    git status --porcelain >&2
fi

# 3. The same command run a second time still succeeds. This is the assertion
#    the old default failed on, and the reason this file exists.
set +e
./benchmarks/runners/run.sh --workload "$workload" >/dev/null 2>"$scratch/run2.err"
run2_code=$?
set -e
if [ "$run2_code" = "0" ]; then
    pass "run 2 with the identical command was accepted"
else
    fail "run 2 was refused (exit $run2_code) — the harness poisoned itself again:"
    cat "$scratch/run2.err" >&2
fi

# 4. Both rows landed in the file run.sh derived from the fallback chain.
if [ ! -f "$reports_dir/$workload.jsonl" ]; then
    fail "no results file at the derived default $reports_dir/$workload.jsonl"
elif [ "$(wc -l <"$reports_dir/$workload.jsonl" | tr -d ' ')" = "2" ]; then
    pass "both rows were appended to the derived out-of-tree results file"
else
    fail "expected 2 rows in $reports_dir/$workload.jsonl, found $(wc -l <"$reports_dir/$workload.jsonl" | tr -d ' ')"
fi

# 5. repeatability.sh — the documented third step — runs after the first two.
set +e
./benchmarks/runners/repeatability.sh --workload "$workload" \
    >"$scratch/repeat.out" 2>"$scratch/repeat.err"
repeat_code=$?
set -e
if [ "$repeat_code" = "0" ]; then
    pass "repeatability.sh ran and stayed inside the stated band"
elif grep -q "outside the stated band" "$scratch/repeat.err"; then
    # A breached band is a statement about this host, not about the fix under
    # test. It must not be reported as the harness refusing to run.
    pass "repeatability.sh ran (band breached on this host, which is a host verdict)"
else
    fail "repeatability.sh was refused rather than answering:"
    cat "$scratch/repeat.err" >&2
fi

# 6. A refused row is not printed. stderr says "nothing written"; stdout must
#    agree, or the percentiles of a refused run are one copy-paste from being
#    quoted. 5 samples is below the publishable minimum of 20.
set +e
./target/release/mesh-bench run --workload "$workload" --iterations 5 \
    --out "$scratch/refused.jsonl" >"$scratch/refused.out" 2>"$scratch/refused.err"
refused_code=$?
set -e
if [ "$refused_code" != "1" ]; then
    fail "a below-minimum run should exit 1, exited $refused_code"
elif [ -s "$scratch/refused.out" ]; then
    fail "a refused run printed $(wc -l <"$scratch/refused.out" | tr -d ' ') line(s) to stdout"
elif [ -e "$scratch/refused.jsonl" ]; then
    fail "a refused run created its output file"
else
    pass "a refused run printed nothing and wrote nothing"
fi

# 7. The worktree is exactly as pristine as it started.
dirty_after=$(git status --porcelain | wc -l | tr -d ' ')
if [ "$dirty_after" = "0" ]; then
    pass "the worktree is still pristine after the full sequence"
else
    fail "the full sequence left $dirty_after entry(ies) in the worktree:"
    git status --porcelain >&2
fi

echo "selftest: $((checks - failures))/$checks checks passed"
[ "$failures" -eq 0 ] || exit 1
