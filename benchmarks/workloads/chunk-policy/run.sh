#!/usr/bin/env bash
# Builds and runs the R3 chunking-policy spike harness (task 01KZC2E6N03KVPK93EESJ15Z4V).
#
# The harness is NOT a workspace member and never will be: adding one rewrites `Cargo.lock`, which
# is governance surface no lane may write (ADR-0013, ADR-0014). It is compiled directly against the
# release rlibs of the two crates that already exist, so the policies it measures are the shipping
# policies and the corpora are the shipping corpora.
#
#   benchmarks/workloads/chunk-policy/run.sh [--samples N] [--w1 SCALE] [--w4 SCALE] [--w5 SCALE]
#
# JSON Lines go to stdout; the human summary goes to stderr.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"

# `[profile.release] lto = "thin"` leaves the rlibs holding bitcode that the system linker cannot
# read, so the spike gets its own profile: same optimisation level, LTO off. Recorded in the report
# as part of the build profile, because a build profile that differs from the workspace default and
# is not stated is a number nobody can reproduce.
CARGO_PROFILE_RELEASE_LTO=off \
  cargo build --release -p mesh-bench -p mesh-chunking --manifest-path "$root/Cargo.toml" >&2

deps="$root/target/release/deps"
bench_rlib="$(ls -t "$deps"/libmesh_bench-*.rlib | head -1)"
chunking_rlib="$(ls -t "$deps"/libmesh_chunking-*.rlib | head -1)"

out="${TMPDIR:-/tmp}/mesh-chunk-policy-spike"
mkdir -p "$out"

rustc --edition 2021 -C opt-level=3 -C debug-assertions=off \
  --extern "mesh_bench=$bench_rlib" \
  --extern "mesh_chunking=$chunking_rlib" \
  -L "dependency=$deps" \
  -o "$out/harness" \
  "$here/harness.rs" >&2

exec "$out/harness" "$@"
