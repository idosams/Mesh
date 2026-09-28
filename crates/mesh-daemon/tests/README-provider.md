# Local provider verification

The dated results below are preserved source-branch evidence, not canonical Mesh validation.
The migration PR records new verification against its exact canonical revision.

The ordinary `fleet-agent` integration suite uses a fixture executable. It checks native working
directory selection, stdin task delivery, credential omission from arguments and observations, and
refusal to launch an already claimed run. Runtime tests also replay launch ownership across restart.

The ignored `actual_codex_edits_checkpoints_and_submits_a_private_review` test exercises an installed,
authenticated Codex CLI and the built Mesh MCP bridge. It consumes provider usage and is deliberately
opt-in. It uses disposable data, never the repository as an agent working folder. Provide absolute
executable paths; use the provider's normal login without exporting or copying authentication files.

```sh
cargo build -p mesh-mcp
MESH_TEST_CODEX=/absolute/path/to/codex \
MESH_TEST_MCP=/absolute/path/to/target/debug/mesh-mcp \
cargo test -p mesh-daemon --test fleet-agent actual_codex -- --ignored --nocapture
```

The native test host delegates one child lane, grants a test-only signing identity, starts the real
provider and binds the real local IPC service. The model must edit `note.txt`, save through
`mesh_fleet_checkpoint` and submit through `mesh_fleet_submit_review`. Assertions inspect durable
checkpoint and review records, final bytes and unchanged selected-source state. Raw provider output
and credentials are not retained. The printed evidence directory is preserved even on failure;
the 240-second deadline requests direct-child termination but cannot prove all descendants exited.
Do not remove an uncertain run's workspace until process ownership has been reconciled.

On 2026-09-27 this test passed with `codex-cli 0.155.0-alpha.9.2` in 29.81 seconds on the development
Mac. That is evidence for one local provider-to-MCP-to-native capture/review connection. It does not
prove fleet scheduling, human approval, shared-state advancement, provider restart reconciliation,
process-tree cancellation, a packaged desktop journey or externally launched harness onboarding.

## Coordinator and automatic child dispatch

The separate opt-in test `actual_coordinator_delegates_two_workers_and_host_saves_both_reviews` starts
a real coordinator with authority to delegate two private lanes. A `CodexFleetHost` discovers the
allocated children, issues native sessions and starts actual Codex workers within the configured
concurrency budget. No test code creates the child lanes or manually dispatches their runs.

```sh
MESH_TEST_CODEX=/absolute/path/to/codex \
MESH_TEST_MCP=/absolute/path/to/target/debug/mesh-mcp \
cargo test -p mesh-daemon --test fleet-agent actual_coordinator -- --ignored --nocapture
```

On 2026-09-27 the strengthened test passed with the same Codex version in 48.82 seconds. Assertions verified exactly
two delegated child lanes, one successful run per child, distinct expected file contents, complete
signed checkpoints and immutable review submissions, and unchanged selected-source state. Evidence
directories remain available after the run. The fixture host test separately exercises queue capacity,
refusal to adopt another host's active processes, and retention of cancelled slots.

The strengthened test also opens each child's saved records through a separate native reader and
reconstructs the exact `/note.txt` artifact from its immutable review. It compares those bytes with
the expected worker output. Retained evidence can be checked again without consuming provider usage:

```sh
MESH_TEST_REVIEW_EVIDENCE=/absolute/path/to/retained/fleet-proof \
cargo test -p mesh-daemon --test fleet-agent retained_actual -- --ignored --nocapture
```

The real test drives ticks from native test code. It does not prove the desktop's scheduling loop,
live fleet UI, parallel human approval, persisted process recovery or remote workers. Agent completion
and recorded review do not advance the main version.

## Canonical migration verification

On 2026-09-27, canonical Mesh revision `bc85196b5c2f2c447cdcb93e40c8e8e58307ca4f`
([PR #8](https://github.com/idosams/Mesh/pull/8)) passed the real coordinator/two-worker test with
`codex-cli 0.155.0-alpha.16` in 48.90 seconds. The native host scheduled both delegated lanes; each
produced its expected distinct file, complete signed checkpoint and immutable review. The source
workspace projection remained unchanged. Independent saved-review readers reconstructed both artifacts
in the test, and the retained-evidence test passed again after the provider run ended.

The ordinary canonical gate also passed: 3,085 Rust tests (13 skipped), 544 desktop tests and 44 real
daemon checks. The skipped provider checks are separate from the successful explicit coordinator
proof. This verifies native scheduling and provider/MCP integration on disposable data, not packaged
UI, human approval, process-tree reconciliation, a second provider or remote execution. The PR remains
unmerged until separately authorized; local evidence does not replace hosted checks or human review.


## Packaged fleet bridge

The desktop executable now includes the scoped fleet MCP mode. A native desktop host can construct
`CodexAdapter::with_desktop_bridge(provider, desktop_executable)` without requiring a separately
installed development `mesh-mcp` binary. Native desktop scheduling is now connected; live fleet UI
and packaged scheduling proof remain unfinished.

After building a clean revision-bound local app, verify its bundle using
`apps/desktop/scripts/verify-local-app.mjs --app <app-path> --revision <exact-revision>` before and
after the journey, retaining both results and the executable hash. The bridge test checks runtime
revision and behavior; it does not itself verify the bundle seal. Then exercise its stdio bridge
through native IPC:

```sh
MESH_TEST_DESKTOP=/absolute/path/to/Mesh.app/Contents/MacOS/mesh-desktop \
MESH_TEST_DESKTOP_REVISION=<exact-40-character-build-revision> \
cargo test -p mesh-mcp --test fleet packaged_desktop_bridge -- --ignored --nocapture
```

This opt-in test uses disposable attached input and a test-only native signing identity. It verifies
that the packaged bridge reports the expected exact revision, delegates two independent children,
checkpoints a change, submits an immutable review, replays exact requests, and refuses revoked
credentials. Original source edits and the desktop-selected workspace remain unchanged. Invalid
fleet arguments and incomplete environment credentials exit before graphical startup and do not
print the test credential. No real model is launched, no provider usage is consumed, and no shared
main is approved. This is packaged non-graphical bridge evidence, not an end-to-end desktop fleet.


## Desktop native scheduling fixture

`cargo test -p mesh-desktop fleet_host::tests` runs the actual application-owned scheduling loop with
an inert local provider fixture and native software session signers. It verifies duplicate start,
redacted live observation, stop-before-start, direct-child cancellation with reserved custody,
owner-drop cancellation, preserved source/selection and suspended dispatch after launch failure.
The daemon host test additionally proves observation-only ticks do not dispatch queued children even
when a slot opens. Catalogue tests refuse execution lookup for reconstructed services; IPC registration
tests permit only exact-instance retries. These tests do not consume model usage, exercise Tauri's
renderer commands, establish process-tree shutdown, or prove graphical interaction.
