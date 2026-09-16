#!/bin/sh
set -eu

repo=/work
demo_rust_toolchain=
for demo_rust_candidate in "$RUSTUP_HOME"/toolchains/1.97.1-*; do
    [ -d "$demo_rust_candidate" ] || continue
    if [ -n "$demo_rust_toolchain" ]; then
        printf 'the demo image contains more than one Rust 1.97.1 host toolchain\n' >&2
        exit 2
    fi
    demo_rust_toolchain=${demo_rust_candidate##*/}
done
if [ -z "$demo_rust_toolchain" ]; then
    printf 'the demo image does not contain a fully qualified Rust 1.97.1 toolchain\n' >&2
    exit 2
fi
RUSTUP_TOOLCHAIN=$demo_rust_toolchain
export RUSTUP_TOOLCHAIN
build=/tmp/mesh-demo-build
run=/tmp/mesh-mounted-demo
actor="$run/actor"
user_view="$run/user"
shadow="$run/shadow"
warm="$run/warm"
workspace="$run/workspace"
source_file="$run/first-draft.txt"
user_source="$run/user-source.txt"
captured="$run/captured.txt"
socket="$run/daemon.sock"
control="$run/daemon.control"
reviewer_key="$run/reviewer.key"
helper="$run/raw-fuse-helper"
actor_id=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
reviewer_public=66be7e332c7a453332bd9d0a7f7db055f5c5ef1a06ada66d98b39fb6810c473a
review_bundle=3333333333333333333333333333333333333333333333333333333333333333

actor_pid=
user_pid=
shadow_pid=
warm_pid=
daemon_pid=

cleanup() {
    for mountpoint in "$warm" "$shadow" "$user_view" "$actor"; do
        if mountpoint -q "$mountpoint" 2>/dev/null; then umount "$mountpoint" || true; fi
    done
    for pid in "$warm_pid" "$shadow_pid" "$user_pid" "$actor_pid" "$daemon_pid"; do
        if [ -n "$pid" ]; then kill "$pid" 2>/dev/null || true; fi
    done
}
trap cleanup EXIT INT TERM

rm -rf "$build" "$run"
mkdir -p "$build/crates" "$actor" "$user_view" "$shadow" "$warm" "$workspace"
cp "$repo/examples/offline-demo-Cargo.toml" "$build/Cargo.toml"
for crate in mesh-types mesh-operations mesh-state mesh-materializer mesh-cas mesh-chunking mesh-bench mesh-store mesh-crypto mesh-approval mesh-daemon; do
    cp -R "$repo/crates/$crate" "$build/crates/$crate"
done

printf '%s\n' 'Mesh local mounted demo' '======================='
printf '%s\n' '1. Build local crates and the pinned bundled SQLite driver with Cargo offline'
cargo build --offline --quiet --manifest-path "$build/Cargo.toml" \
    -p mesh-daemon --bins --example capture-checkpoint
cc -std=c11 -Wall -Wextra -Wpedantic -Werror -O2 "$repo/examples/raw-fuse-helper.c" -o "$helper"
printf '   PASS: Rust and raw-FUSE helper built with network disabled\n'

meshd="$build/target/debug/meshd"
meshctl="$build/target/debug/meshctl"
checkpoint="$build/target/debug/examples/capture-checkpoint"

mkfifo "$control"
exec 3<> "$control"
node -e 'require("fs").writeFileSync(process.argv[1], Buffer.alloc(32, 11), {mode: 0o600})' "$reviewer_key"
"$meshd" --endpoint "$socket" \
    --trusted-reviewer-key "$reviewer_public" \
    --checkpoint-idle-ms 8 \
    --checkpoint-maximum-bytes 16 \
    --checkpoint-maximum-interval-ms 20 \
    < "$control" > "$run/daemon.out" 2> "$run/daemon.err" &
daemon_pid=$!
for attempt in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
    [ -S "$socket" ] && break
    sleep 0.05
done
[ -S "$socket" ]
"$meshctl" --endpoint "$socket" open "$workspace" > "$run/open-before.jsonl"
"$meshctl" --endpoint "$socket" state > "$run/state-before.jsonl"
node -e '
const lines=require("fs").readFileSync(process.argv[1],"utf8").trim().split(/\n/);
const message=JSON.parse(lines.at(-1)); const state=message.value ?? message;
if (state.records !== 0 || state.operations !== 0) process.exit(1);
' "$run/state-before.jsonl"
printf '   PASS: daemon reports a fresh durable boundary (records=0, operations=0)\n'
test -f "$workspace/metadata.sqlite"
printf '   PASS: daemon created the durable metadata.sqlite index before serving\n'

printf '%s\n' '2. Mount isolated user and actor views through the Linux kernel FUSE device'
printf 'first draft\n' > "$source_file"
printf 'user workspace\n' > "$user_source"
"$helper" "$user_view" user.txt "$user_source" - ro > "$run/user-ready.json" &
user_pid=$!
"$helper" "$actor" notes.txt "$source_file" "$captured" rw > "$run/actor-ready.json" &
actor_pid=$!
for attempt in 1 2 3 4 5 6 7 8 9 10; do
    mountpoint -q "$actor" && mountpoint -q "$user_view" && break
    sleep 0.05
done
mountpoint -q "$actor"
mountpoint -q "$user_view"
test ! -e "$user_view/notes.txt"
test "$(cat "$actor/notes.txt")" = 'first draft'
printf '   PASS: /dev/fuse serves both views and the user view has no agent file\n'

printf '%s\n' '3. Run an ordinary Node process in the actor mount and capture its close boundary'
(
    cd "$actor"
    node -e 'require("fs").writeFileSync("notes.txt", "second draft\n")'
)
test "$(cat "$captured")" = 'second draft'
"$checkpoint" "$workspace" "$captured" "$actor_id" notes.txt > "$run/checkpoint.out"
grep -q 'records=2 operations=1 manifests=1' "$run/checkpoint.out"
grep -q "actor=$actor_id" "$run/checkpoint.out"
printf '   PASS: captured ChangeSet is attributed to actor %s\n' "$actor_id"
"$meshctl" --endpoint "$socket" open "$workspace" > "$run/open-after.jsonl"
"$meshctl" --endpoint "$socket" state > "$run/state-after.jsonl"
node -e '
const fs=require("fs");
const last=p=>{const lines=fs.readFileSync(p,"utf8").trim().split(/\n/); const m=JSON.parse(lines.at(-1)); return m.value ?? m};
const before=last(process.argv[1]), after=last(process.argv[2]);
if (before.records !== 0 || after.records !== 2 || after.operations !== 1 || after.manifests !== 1) process.exit(1);
if (after.actors !== 1 || (after.conditions ?? []).length !== 0) process.exit(1);
if (before.digest === after.digest) process.exit(1);
if (!Array.isArray(after.entries) || !after.entries.some(e=>e.path === "notes.txt" && e.type === "file")) process.exit(1);
if ((after.not_yet ?? []).some(item=>item.subject === "file names and folders")) process.exit(1);
process.stdout.write(`   digest moved ${before.digest} -> ${after.digest}\n`);
' "$run/state-before.jsonl" "$run/state-after.jsonl"
printf '   PASS: exact close-path capture advanced operations 0->1 and raw records 0->2\n'
printf '   PASS: daemon materialized notes.txt from the canonical ChangeSet payload\n'

target=$(sed -n 's/.* target=\([0-9a-f][0-9a-f]*\) digest=.*/\1/p' "$run/checkpoint.out")
[ "${#target}" -eq 64 ]
"$meshctl" --endpoint "$socket" review-open "$review_bundle" "$target" "$reviewer_key" > "$run/review-open.jsonl"
if "$meshctl" --endpoint "$socket" approve "$review_bundle" "$target" genesis "$reviewer_key" > "$run/review-approve.out" 2> "$run/review-approve.err"; then
    printf 'software-held approval unexpectedly succeeded\n' >&2
    exit 1
fi
grep -q 'no verified human-held signing authority' "$run/review-approve.err"
"$meshctl" --endpoint "$socket" state > "$run/state-after-review.jsonl"
node -e '
const lines=require("fs").readFileSync(process.argv[1],"utf8").trim().split(/\n/);
const message=JSON.parse(lines.at(-1)); const state=message.value ?? message;
if (state.shared_version !== null || state.reviews !== 1 || state.records !== 3) process.exit(1);
if (!(state.not_yet ?? []).some(item=>item.subject === "shared version")) process.exit(1);
' "$run/state-after-review.jsonl"
printf '   PASS: exact review persisted and software-held approval failed closed\n'

printf '%s\n' '4. Mount an immutable actor shadow and require kernel-enforced EROFS'
"$helper" "$shadow" notes.txt "$captured" - ro > "$run/shadow-ready.json" &
shadow_pid=$!
for attempt in 1 2 3 4 5 6 7 8 9 10; do
    mountpoint -q "$shadow" && break
    sleep 0.05
done
mountpoint -q "$shadow"
test "$(cat "$shadow/notes.txt")" = 'second draft'
if (printf 'forbidden\n' > "$shadow/notes.txt") 2> "$run/shadow-write-error.txt"; then
    printf 'shadow write unexpectedly succeeded\n' >&2
    exit 1
fi
grep -qi 'read-only file system' "$run/shadow-write-error.txt"
"$meshctl" --endpoint "$socket" state > "$run/state-after-shadow.jsonl"
node -e '
const fs=require("fs"); const last=p=>{const a=fs.readFileSync(p,"utf8").trim().split(/\n/); const m=JSON.parse(a.at(-1)); return m.value ?? m};
const a=last(process.argv[1]), b=last(process.argv[2]);
if (a.digest !== b.digest || a.records !== b.records || a.operations !== b.operations) process.exit(1);
' "$run/state-after-review.jsonl" "$run/state-after-shadow.jsonl"
test "$(cat "$actor/notes.txt")" = 'second draft'
printf '   PASS: shadow reads saved bytes, rejects writes with EROFS, and changes no head\n'

printf '%s\n' '5. Measure a second mount through kernel readiness, including stat(2)'
started=$(date +%s%N)
"$helper" "$warm" notes.txt "$captured" - ro > "$run/warm-ready.json" &
warm_pid=$!
for attempt in 1 2 3 4 5 6 7 8 9 10; do
    if stat "$warm/notes.txt" >/dev/null 2>&1; then break; fi
    sleep 0.01
done
stat "$warm/notes.txt" >/dev/null
finished=$(date +%s%N)
elapsed_ms=$(((finished - started) / 1000000))
[ "$elapsed_ms" -lt 250 ]
printf '   PASS: warm mount ready in %s ms (<250 ms)\n' "$elapsed_ms"

printf '%s\n' '6. Release every view, restart configured state, and stop cleanly'
umount "$warm"; wait "$warm_pid"; warm_pid=
umount "$shadow"; wait "$shadow_pid"; shadow_pid=
umount "$user_view"; wait "$user_pid"; user_pid=
umount "$actor"; wait "$actor_pid"; actor_pid=
printf 'stop\n' >&3
wait "$daemon_pid"
daemon_pid=
"$meshd" --endpoint "$socket" --workspace "$workspace" \
    --trusted-reviewer-key "$reviewer_public" \
    --checkpoint-idle-ms 8 \
    --checkpoint-maximum-bytes 16 \
    --checkpoint-maximum-interval-ms 20 \
    < "$control" > "$run/restarted-daemon.out" 2> "$run/restarted-daemon.err" &
daemon_pid=$!
for attempt in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
    [ -S "$socket" ] && break
    sleep 0.05
done
[ -S "$socket" ]
"$meshctl" --endpoint "$socket" state > "$run/restarted-state.jsonl"
node -e '
const lines=require("fs").readFileSync(process.argv[1],"utf8").trim().split(/\n/);
const message=JSON.parse(lines.at(-1)); const state=message.value ?? message;
if (state.shared_version !== null || state.reviews !== 1 || state.records !== 3) process.exit(1);
if (!(state.not_yet ?? []).some(item=>item.subject === "shared version")) process.exit(1);
' "$run/restarted-state.jsonl"
printf 'stop\n' >&3
wait "$daemon_pid"
daemon_pid=
exec 3>&-
printf '   PASS: review state survived restart while checkpoint parameters were explicitly supplied again\n'

printf '\nPASS: real local mounted demo completed with Docker network disabled.\n'
printf 'Semantics: this is exact close-path/recovery capture, not the unratified idle-settling claim.\n'
