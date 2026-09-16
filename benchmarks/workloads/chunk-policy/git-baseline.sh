#!/usr/bin/env bash
# The Git baseline arm of the R3 chunking-policy spike (task 01KZC2E6N03KVPK93EESJ15Z4V).
#
# `benchmarks/budgets/storage.md` §6 states Mesh at 14.7x Git for a one-byte edit. That number is
# re-run here rather than quoted, on the SAME bytes the Rust harness chunks, so the ratio is a
# measurement and not a recollection. The two arms differ in what they count and the report says so:
# Git's figure is the growth of `.git` after `git gc --aggressive`; Mesh's is new chunk content plus
# `mesh-cas`' 67-byte-per-chunk arrival journal, with no compression and no collector, because
# neither exists.
#
#   benchmarks/workloads/chunk-policy/git-baseline.sh
#
# JSON Lines on stdout, human notes on stderr. Exit 0 on success.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"

work="$(mktemp -d "${TMPDIR:-/tmp}/mesh-git-baseline.XXXXXX")"
trap 'rm -rf "$work"' EXIT

git_version="$(git --version)"
echo "baseline: $git_version" >&2

# `du` counts allocated blocks; the storage.md reproduction sums stat sizes, so this does the same.
size_of() { find "$1" -type f -exec stat -f "%z" {} \; | awk '{s+=$1} END {print s+0}'; }

commit_and_gc() {
  git -C "$1" add -A >/dev/null
  git -C "$1" -c user.email=spike@mesh -c user.name=spike commit -q -m "$2" >/dev/null
  git -C "$1" gc --aggressive --quiet >/dev/null 2>&1
}

emit() {
  printf '{"baseline":"git","tool_version":"%s","arm":"%s","content_bytes":%s,"git_bytes_v1":%s,"git_bytes_v2":%s,"git_delta_bytes":%s,"files":%s,"gc":"aggressive, before both readings"}\n' \
    "$git_version" "$1" "$2" "$3" "$4" "$(( $4 - $3 ))" "$5"
}

# ---------------------------------------------------------------- arm 1: row 3
# The exact fixture the Rust harness measures: a seeded 8,895-byte source-like file.
arm1="$work/row-three"
mkdir -p "$arm1"
"$here/run.sh" --emit-row-three "$arm1" 2>/dev/null
mv "$arm1/before.txt" "$arm1/file.txt"
git -C "$arm1" init -q
commit_and_gc "$arm1" one
before_bytes="$(size_of "$arm1/.git")"
mv "$arm1/after.txt" "$arm1/file.txt"
commit_and_gc "$arm1" two
after_bytes="$(size_of "$arm1/.git")"
content="$(stat -f "%z" "$arm1/file.txt")"
emit "storage.md-6.3/one-byte-edit" "$content" "$before_bytes" "$after_bytes" 1
echo "  row 3: ${content} B file, one byte changed -> git grew $(( after_bytes - before_bytes )) B" >&2

# ------------------------------------------------- arm 2: W1 smoke, every file
cargo build --release -p mesh-bench --bin mesh-bench --manifest-path "$root/Cargo.toml" >&2
arm2="$work/w1-smoke"
"$root/target/release/mesh-bench" corpus materialize --workload W1 --scale smoke --root "$arm2" >/dev/null
content2="$(size_of "$arm2")"
files2="$(find "$arm2" -type f | wc -l | tr -d ' ')"
git -C "$arm2" init -q
commit_and_gc "$arm2" one
before2="$(size_of "$arm2/.git")"
node -e '
const { readdirSync, statSync, readFileSync, writeFileSync } = require("node:fs");
const { join } = require("node:path");
const walk = (dir) => readdirSync(dir).flatMap((name) => {
  if (name === ".git") return [];
  const path = join(dir, name);
  return statSync(path).isDirectory() ? walk(path) : [path];
});
for (const path of walk(process.argv[1])) {
  const bytes = readFileSync(path);
  if (bytes.length === 0) continue;
  bytes[bytes.length >> 1] ^= 0xff;
  writeFileSync(path, bytes);
}
' "$arm2"
commit_and_gc "$arm2" two
after2="$(size_of "$arm2/.git")"
emit "W1/smoke/one-byte-edit-every-file" "$content2" "$before2" "$after2" "$files2"
echo "  W1 smoke: ${files2} files, ${content2} B, one byte changed in each -> git grew $(( after2 - before2 )) B" >&2

echo "not run: jujutsu (jj is not installed on this host; its default backend is Git's object store, so the chunking question it would answer is the one arm 1 already answers), git worktrees and folder replication (neither changes the object store, which is the only thing a chunk policy moves)" >&2
