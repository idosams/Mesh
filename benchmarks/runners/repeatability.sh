#!/bin/sh
# Two runs of the same commit on this machine, held to the stated variance band.
#
# This is the question "is this host trustworthy right now" — asked before a
# number is published, not after someone disputes it. Both runs use identical
# arguments, so anything the comparison finds is the machine, not the change.
#
# Usage:
#   benchmarks/runners/repeatability.sh --workload blob-scan [--iterations 200] [...]
#
# Exit codes: 0 inside the band, 1 outside it (or a run was refused),
#             2 the invocation was wrong.
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repo_root"

workload=""
previous=""
for argument in "$@"; do
    case "$previous" in
        --workload) workload="$argument" ;;
    esac
    case "$argument" in
        --out)
            echo "repeatability.sh: --out is managed by this script" >&2
            exit 2
            ;;
    esac
    previous="$argument"
done

if [ -z "$workload" ]; then
    echo "repeatability.sh: --workload NAME is required" >&2
    exit 2
fi

output_dir=$(mktemp -d "${TMPDIR:-/tmp}/mesh-bench-repeatability.XXXXXX")
trap 'rm -rf "$output_dir"' EXIT

echo "repeatability.sh: run 1 of 2" >&2
./benchmarks/runners/run.sh "$@" --out "$output_dir/first.jsonl" >/dev/null
echo "repeatability.sh: run 2 of 2" >&2
./benchmarks/runners/run.sh "$@" --out "$output_dir/second.jsonl" >/dev/null

exec ./target/release/mesh-bench compare "$output_dir/first.jsonl" "$output_dir/second.jsonl"
