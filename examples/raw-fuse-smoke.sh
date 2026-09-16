#!/bin/sh
set -eu

root=/tmp/mesh-raw-fuse-smoke
actor="$root/actor"
shadow="$root/shadow"
source_file="$root/source.txt"
captured="$root/captured.txt"
helper=/tmp/raw-fuse-helper
compiler=${CC:-cc}
compiler_flags=${CFLAGS:--std=c11 -Wall -Wextra -Wpedantic -Werror -O2}

mkdir -p "$actor" "$shadow"
printf 'first draft\n' > "$source_file"
# Intentional word splitting lets a caller supply a sanitizer flag vector.
# shellcheck disable=SC2086
"$compiler" $compiler_flags /work/examples/raw-fuse-helper.c -o "$helper"

"$helper" "$actor" notes.txt "$source_file" "$captured" rw > "$root/actor-ready.json" &
actor_pid=$!
for attempt in 1 2 3 4 5 6 7 8 9 10; do
    mountpoint -q "$actor" && break
    sleep 0.05
done
mountpoint -q "$actor"
test "$(cat "$actor/notes.txt")" = 'first draft'
test "$(ls -1 "$actor")" = 'notes.txt'
node - "$actor/notes.txt" <<'NODE'
const fs = require('fs');
const path = process.argv[2];
const fd = fs.openSync(path, 'r+');
for (const operation of [
  () => fs.writeSync(fd, Buffer.from('x'), 0, 1, Number.MAX_SAFE_INTEGER),
  () => fs.writeSync(fd, Buffer.from('x'), 0, 1, 32 * 1024 * 1024),
  () => fs.ftruncateSync(fd, 32 * 1024 * 1024),
]) {
  try {
    operation();
    process.exit(1);
  } catch (error) {
    if (error.code !== 'EFBIG') throw error;
  }
}
fs.closeSync(fd);
NODE
test "$(cat "$actor/notes.txt")" = 'first draft'
printf 'second draft\n' > "$actor/notes.txt"
test "$(cat "$captured")" = 'second draft'

"$helper" "$shadow" notes.txt "$captured" - ro > "$root/shadow-ready.json" &
shadow_pid=$!
for attempt in 1 2 3 4 5 6 7 8 9 10; do
    mountpoint -q "$shadow" && break
    sleep 0.05
done
mountpoint -q "$shadow"
test "$(cat "$shadow/notes.txt")" = 'second draft'
if printf 'forbidden\n' > "$shadow/notes.txt" 2> "$root/write-error.txt"; then
    echo 'read-only write unexpectedly succeeded' >&2
    exit 1
fi
test "$(cat "$actor/notes.txt")" = 'second draft'

umount "$shadow"
umount "$actor"
wait "$shadow_pid"
wait "$actor_pid"

long_name=$(printf '%0256d' 0 | tr 0 x)
if "$helper" "$root/rejected" "$long_name" "$source_file" - ro > /dev/null 2> "$root/name-error.txt"; then
    echo 'overlong entry name unexpectedly succeeded' >&2
    exit 1
fi
grep -q '1..255 bytes' "$root/name-error.txt"

for reserved_name in . ..; do
    if "$helper" "$root/rejected" "$reserved_name" "$source_file" - ro > /dev/null 2> "$root/name-error.txt"; then
        echo 'reserved entry name unexpectedly succeeded' >&2
        exit 1
    fi
    grep -q 'one path component' "$root/name-error.txt"
done

large_source="$root/too-large.txt"
truncate -s 17M "$large_source"
if "$helper" "$root/rejected" notes.txt "$large_source" - ro > /dev/null 2> "$root/source-error.txt"; then
    echo 'oversized source unexpectedly succeeded' >&2
    exit 1
fi
grep -q 'source too large' "$root/source-error.txt"
printf 'raw-fuse-smoke: PASS\n'
