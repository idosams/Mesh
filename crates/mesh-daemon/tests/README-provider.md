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

## Shared stream protocol boundary

`fleet::provider::protocol` decodes external provider events separately from Mesh
IPC. Both native adapters use it; Claude stream-json decoding also has
protocol fixtures for error-dominant completion. Provider fractional cost/usage
values are accepted and redacted. Duplicate keys, oversized/deep events, malformed
identity and identity replacement fail closed. An assistant error or Claude
`is_error=true` is failure even when the result subtype says `success`; a later
result cannot erase it. EOF and unknown events never imply completion.

The existing Codex process outcome still requires successful exit, complete pipe
closure, a completed turn and no protocol failure. No protocol event grants
checkpoint, review, custody release or shared-main authority. New conformance tests
live beside the shared decoder and run in the daemon unit suite. Live Claude
launch/configuration/custody tests and successful authenticated runs remain required.

## Native Claude adapter and mixed-provider hosts

`ClaudeAdapter::new` and `with_desktop_bridge` admit explicit native executable
paths. `FleetService::start_claude` and `start_provider` reuse the existing durable
claim, provider matching, workspace identity and session custody checks.
`NativeFleetHost` dispatches only lanes matching its admitted adapter; separately
configured Codex and Claude hosts share the same objective's durable concurrency
budget. `CodexFleetHost` and `CodexProcess` remain compatibility names. The desktop
provider-selection flow still requires integration before this native API is a
user-facing second-provider feature.

The fixed Claude command uses print stream-json, stdin task delivery, environment
credentials and a strict Mesh-only MCP configuration. It disables ordinary setting
sources, skills and non-managed hooks; uses `dontAsk`; exposes sandboxed Bash for
file work; and requests enabled/fail-if-unavailable sandboxing with no unsandboxed
fallback. It does not use bare mode, safe mode, or permission-bypass flags.
Existing provider authentication stays provider-owned. Managed provider policy
remains authoritative, including managed hooks/exclusions: this adapter is not an
OS sandbox for the Claude executable itself. Native admission must use an installed
CLI supporting these options. Local option discovery used Claude Code 2.1.220;
actual sandbox/tool/bridge behavior remains part of the required live acceptance.
See the official [CLI reference](https://code.claude.com/docs/en/cli-reference),
[settings reference](https://code.claude.com/docs/en/settings-reference), and
[MCP configuration](https://code.claude.com/docs/en/mcp).

The fixture suite covers both providers' exact native working directory, private
stdin goal, absence of credential bytes in argv/observations, single durable launch,
provider mismatch refusal before claim, packaged bridge selection, and Claude's
error-dominant result despite exit zero. A mixed-host case verifies provider
selection and shared concurrency. Fixture success is not a real provider result.

The opt-in live journey uses the same actual edit/checkpoint/review assertions as
Codex and retains evidence on failure:

```sh
cargo build -p mesh-mcp
MESH_TEST_CLAUDE=/absolute/path/to/claude \
MESH_TEST_MCP=/absolute/path/to/target/debug/mesh-mcp \
cargo test -p mesh-daemon --test fleet-agent actual_claude -- --ignored --nocapture
```

No successful live Claude journey is claimed. Authentication previously returned
`authentication_failed`; local sign-in refresh is pending. Do not retry that probe
as a substitute for completing this full journey once authentication is available.

## Persisted provider choices

`NativeFleetDirectory::create_attached_with_providers` binds a validated
`FleetProviderPolicy` to an allocation. The policy chooses the coordinator and
which of Codex/Claude may receive delegated lanes. The existing `create_attached`
entry point and its v1 receipt stay Codex-only. Non-default policies use a v2
receipt; allowed-provider order is canonical and duplicate/unknown entries refuse.

The request identity cannot be reused to change providers, including while the
service is already open. Restart discovery checks the retained lane providers
against the receipt and restores observation only. Old software cannot execute a
v2 allocation by treating it as a v1 Codex allocation: the schema/receipt byte
comparison refuses it. `admitted_providers` exposes immutable native configuration
for constructing all required hosts before dispatch; it is not an agent command.
These native policy APIs still need visible desktop selector integration.

## Desktop native execution boundary

The desktop Start command resolves and admits every provider in the saved policy
before handing the complete adapter set to its application-owned loop. Missing or
duplicate adapters refuse before any worker starts. The loop observes all providers,
suspends further dispatch after a provider error, and applies cancellation across
the entire objective. It neither adopts old workers nor releases uncertain custody.

The native provisioning command accepts optional `policyJson` containing exactly
`coordinator` and `providers`. Omission preserves the legacy Codex request/response.
Explicit policy gets a `mesh.desktop-attached-fleet/v2` response with canonical
`policy`; native catalogue rows also include policy facts, null when unavailable.
This backend is ready for selector/controller integration, which is still required.
Executable discovery uses native installation locations and canonicalizes Claude's
usual installation symlink. Account login and actual sandbox/bridge operation remain
part of live acceptance, not facts inferred from an executable's presence.


### Worker progress inspection

The native `worker_progress_` regressions use real allocated folders and exact scoped credentials.
They inspect modified/new/missing/unsupported entries without saving or changing the fleet ledger,
pause the inspection boundary while another thread reads fleet state, refuse cancellation or
credential rotation/revocation before returning, and preserve substituted folders. A shared-lock
mutation must fail the parallel-read deadline. This proves read-path isolation, not scheduled
background saving or a packaged provider journey.


### Local checkpoint concurrency

The `local_checkpoint_` native tests pause a real signing callback while another thread reads
fleet state, require exact retry without further signatures, revoke/cancel during signing, and
run actual credential rotation concurrently with native custody. Mutex admission refuses instead
of waiting in the reverse order. The old capture route must fail the two-second parallel-read
deadline. Received-session behavior remains covered by its existing authority tests. These tests
do not establish periodic saving, process termination or packaged fleet acceptance.


### Automatic local progress capture

The `automatic_progress_` tests exercise native journal capture using exact scoped worker grants.
They cover unchanged polls without signatures/events, separate explicit handoff accounting,
missed fleet acknowledgment recovery without another signature, missing-file preservation,
revocation/cancellation and an older completion racing a newer explicit checkpoint. The real
provider-process fixture writes while alive, waits for an observed private save, then writes its
final edit before exiting. The host must capture both phases without a checkpoint tool call.
Disabling periodic live-worker saves must fail the fixture's live-save assertion.

The host owns one capture job per local worker, waits five seconds after a completed attempt,
and forces one final attempt before terminal acknowledgment. Incomplete or failed saving remains
separate from process outcome. Final failures are visible but not automatically retried after
revocation. Cancellation and host drop revoke credentials; drop joins admitted work. Filesystem
or signer latency can delay final acknowledgment and shutdown. Received sessions keep their
existing path. Desktop save observations are additive nullable fields; older payloads still parse.
No persisted schema changes, new handoff records or automatic process adoption are introduced.
These regressions are native fixture evidence, not packaged or authenticated provider acceptance.
