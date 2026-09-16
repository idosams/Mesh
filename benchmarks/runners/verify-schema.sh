#!/bin/sh
# Proves this build still refuses an incomplete result row.
#
# Runs the required-field contract from the compiled artefact rather than from
# the test suite, so it means something on a machine that is about to publish a
# number. No repository, no host probe, no timing — safe anywhere, and fast
# enough to be a gate.
#
# Exit codes: 0 the contract holds, 1 a required field stopped being required.
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repo_root"

# rustup's default location, used only when cargo is not already on PATH (a
# non-interactive shell frequently is not the shell the toolchain was installed in).
if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
    export PATH
fi

exec cargo bench -q -p mesh-bench --bench smoke -- --verify-schema
