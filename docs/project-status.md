# Project status

Mesh is an early functional local alpha. Its canonical product and development repository is
[idosams/Mesh](https://github.com/idosams/Mesh). Mesh-internal is retained history, not a development
destination. The [delivery ledger](plan/fleet-migration.md) records exact revisions, source mappings,
PR checks and merge status; this page describes capability and acceptance boundaries.

## Product direction

Mesh provides the underlying files, saved versions and work correlation for an existing project.
People may keep their editor, terminal, Git workflow and external agent harness. Attaching Mesh
provisions private metadata outside the project and observes work without moving the project or
requiring Mesh to launch an agent. Manual work, harness-led work and Mesh-managed fleets are all
part of the [full fleet plan](plan/fleet-orchestration.md).

The user can review versions and Mesh main in Mesh while continuing ordinary work elsewhere.
Creating independent lanes and starting a managed fleet are optional. Agents may create authorized
child lanes and submit immutable results; they cannot approve protected Mesh main. Applying an
accepted result to the original folder is a separate, explicitly confirmed native operation.

## Automatic local worker saving

The current source saves private progress for locally owned fleet workers between explicit handoffs,
with one background capture per worker and a final attempt before execution completion. Save status
is separate from provider status; incomplete final saves remain visible and do not imply review or
approval. A complete final native version with a pending fleet acknowledgment gets up to three
attempts one second apart before terminal recording; cancellation prevents further attempts.
Native fixture tests cover live and final edits, empty polls, missed acknowledgment,
revocation and newer-version races. This is not yet hosted or packaged acceptance. Received remote
sessions and the fixed user checkpoint do not include this behavior. See the delivery ledger for
R130 evidence and limits.

## Retained worker execution observations

The native worker registry can read original session setup and recorded run outcomes after a
restart, correlated to the retained launch receipt. This is read-only historical evidence; it
cannot release occupied capacity, adopt a process or authorize another launch. Fresh native v4
status queries can carry these facts over the configured authenticated transport; old versions
cannot satisfy the new query. Desktop presentation is merged, including independent fleet-wide reads. Actual signed
second-host acceptance and safe terminal reconciliation remain unfinished. Fixture
provider-process outcomes and restart/refusal cases are covered by native regression tests;
this is not signed packaged or second-host acceptance.

## Delivery and acceptance checkpoint — 4 October 2026

Canonical main `ee6fb900d752dff293e2b33bd7a051f98d124db2` includes remote observations,
installed Codex discovery, the collector crash campaign and conservative retention of buffered
history ([PR #259](https://github.com/idosams/Mesh/pull/259)). All seven PR checks and
[merged-main verification](https://github.com/idosams/Mesh/actions/runs/37165263608) passed.
The [legacy deprecation notice](https://github.com/idosams/Mesh-internal/pull/1488) is merged;
its history and the original dirty checkout remain preserved. The
[dated acceptance map](plan/fleet-acceptance.md) records unfinished phase exits.

A fixed `c2641c6` checkpoint is available for user testing with a corrected separate-data launcher;
older checkpoints remain preserved. Its package identity and seal were verified, but packaged
provider execution is still unverified. The earlier `97e263f` window journey passed attachment,
two pinned comparisons and restart/detach with original Git preserved; it launched no provider
and approved no protected main. Current native real-Codex four-worker evidence is recorded
separately and does not substitute for that desktop journey. Signed approval, live external-harness
acceptance, second provider/host, full fault/retention work and final packaged acceptance remain open.

## Test-runner reliability

Validation requires nextest 0.9.145 or newer, including the upstream fix for sibling output-pipe
inheritance on Apple platforms. An isolated 0.9.145 run on merged PR231 passed all 3,703 native
tests without leak warnings; this supports using the corrected runner but does not establish that
every historical warning had that cause. The broader [process-lifecycle investigation](https://github.com/idosams/Mesh/issues/172)
and [capture-startup investigation](https://github.com/idosams/Mesh/issues/37) remain open. No
leak threshold, test deadline, assertion or skip was changed. See the developer guide for setup.

## Historical source baseline — PR #120

The combined implementation assessed here is `b08674d4f5ec963204984d2e09e6dacda884fd66`
([PR #120](https://github.com/idosams/Mesh/pull/120)). Its source includes:

- Durable SQLite records, signed private checkpoints, exact saved versions, review bundles and
  guarded native approval/publication. Native code retains filesystem, identity and custody authority.
- Existing-project registration and private external storage; bounded observation, background capture,
  explicit stop/resume, detach/reattach and retained history. macOS filesystem events wake scans;
  periodic reconciliation remains available. An event is not proof of content or authorship.
- Exact saved-version browsing/comparison and independent manual lanes. Original files and Git stay
  unchanged by provisioning, capture and review. Unavailable source/history and unknown authorship
  remain explicit. English/Hebrew presentation preserves literal paths and saved identities.
- Durable fleet objectives, limits, lanes, attempts, idempotent commands and native launch ownership.
  Scoped MCP sessions provide delegation, checkpoint and review tools. The native desktop host
  schedules workers and exposes redacted observations; uncertain processes retain their slots.
- Codex and Claude adapters, native provider admission and explicit coordinator/allowed-provider
  choices. Pending provisioning freezes exact inputs and choices for an explicit retry. Source
  support for Claude is not evidence of a successful authenticated Claude acceptance run.
- Up to eight independent pinned saved-review panels, fixed starting-version comparisons, artifact
  previews, durable selectors and history-only reopening. New work does not replace pinned content.
  Feedback, proposed-result links and exact work decisions retain their original review identities.
- Original-project correspondence and signed candidate imports; exact main review; guarded file,
  deletion, directory, type-conversion and grouped application; retained recovery and whole-entry
  restoration controls. Group application can have partial outcomes. Inspection never authorizes
  replay or cleanup, and restoration preserves displaced work instead of erasing it.
- A real Codex four-worker comparison with exact saved results and acknowledged worker overlap:
  168.531 s serial and 57.748 s parallel in one completed native pair. The
  [measurement record](plan/fleet-native-measurements.md) retains a failed startup attempt and
  the successful later attempt. These results do not establish GUI or human-review performance.
- Remote assignment/lease records, a single-use signed worker challenge, and bounded immutable
  input manifests with resumable CAS receipt, exact attached/managed saved-input export and
  create-only private materialization with complete output verification. These foundations do not
  establish an authenticated connection or execute a remote worker.

Source behavior and automated coverage must not be confused with the packaged graphical journey.
Restored fleet history is readable without adopting a worker or recreating execution authority.
Uncertain ownership still requires native reconciliation before further execution.

## Verification and unfinished acceptance

All seven hosted PR checks and combined-main CI passed for the exact #120 revision, including Linux and macOS native suites,
desktop/docs, lint, formatting, dependency and license checks. The latest combined local gate is
separately tracked in the delivery ledger. Hosted success does not close the intermittent local
native startup/stop concern in [issue #37](https://github.com/idosams/Mesh/issues/37).

The complete plan remains open. Required evidence still includes:

- Full combined-main validation and resolution of recorded local native failures. Preserve failure
  logs; a passing focused rerun does not establish their cause or full-suite reliability.
- Packaged manual and already-running-harness journeys: continue original work, save, compare,
  review, approve exact main, integrate and recover without requiring Mesh-managed agent execution.
- Revision-bound graphical fleet journeys, parallel review during continued work, restart/recovery,
  native confirmation and human-presence approval in an eligible signed build. Source/renderer tests
  and older packaged binaries do not prove the current revision. Do not install over the user's app.
- Successful execution with a second real provider and a matched external-harness baseline. Extend
  the completed four-worker native pair with renderer/event responsiveness, resource use, cost, time
  to accepted result and human coordination measurements. Resolve startup reliability and repeat the
  experiment before making general velocity claims.
- The full fault, private-dependency and retention acceptance audit required by the plan, including
  uncertain process ownership, revocation, replaced directories and storage exhaustion.
- Integration of native input export and materialization with mutually authenticated transport,
  durable executor ownership, verified result transfer and reconnect reconciliation. A real second machine
  must execute, disconnect and reconnect without duplicate execution or lost acknowledged work.
  See the [remote sequence](plan/fleet-remote-delivery.md); loopback and mock results are insufficient.

Signed human approval requires a supported macOS signing environment; it has no weaker software
fallback. Actual second-provider authentication, a provisioned second machine and signing prerequisites
are external acceptance inputs. Their absence does not make the corresponding requirements complete.
The Mesh-internal deprecation PR and its failing CI remain tracked in the delivery ledger.

## Distribution boundaries

The strongest supported path remains one Apple-silicon Mac with a disposable or backed-up project.
Hosted synchronization, general multi-device collaboration, stable public SDK/API compatibility,
automatic updates, supported Windows distribution and production Linux packaging are not shipped.
Continuous capture while no Mesh host is running is not claimed. Git export remains explicit and
guarded; Mesh does not replace Git hosting.

## Reproduce evidence

Run `npm test` from a prepared canonical checkout for repository/docs/license/storage, Rust,
desktop/React and real daemon checks. The daemon demonstration can also run independently:

```bash
node examples/local-daemon-demo.mjs
```

The local demo should print 44 passing checks. These cover explicit local capture, restart, review and software-key
publication refusal. They do not prove signed approval, remote execution or a clean-Mac installation.
See the [user guide](user-guide.md), [architecture](architecture.md),
[public-alpha guide](launch/public-alpha.md), [phase assessment](phase-assessment.md),
[user playbooks](user-playbooks.md) and [measurement contract](plan/fleet-native-measurements.md).

## Remote input validation

Exact saved attached/managed export and private receiving materialization are merged through #120.
All five new materialization regressions passed on hosted Linux/macOS and locally in a focused run.
Hosted coverage includes the earlier export/history regressions. The combined local full gate remains
separate and unfinished. These tests do not prove authenticated remote dispatch, workspace
initialization, executor ownership, restart reconciliation or second-machine acceptance.

The next native receiving increment binds CAS transfer to an admitted directory descriptor with
exclusive receiving ownership, bounded reads and replacement/protected-root checks. Its regressions
are pending. This closes a storage-authority integration gap before network exposure; it does not
establish remote transport or execution readiness.

## Received worker history and launch ownership

[PR #129](https://github.com/idosams/Mesh/pull/129) and
[PR #128](https://github.com/idosams/Mesh/pull/128) are merged with all required exact-head checks
passing. Received trees, including empty trees, now initialize independent native history with
an explicit durable source-to-worker mapping. This does not claim a remote provider ran.

[PR #130](https://github.com/idosams/Mesh/pull/130) merged durable launch ownership at
`486b0e0bcc2889e46b33739d9d0a55fc21c102e8`, with all seven exact-head checks passing.
[PR #131](https://github.com/idosams/Mesh/pull/131) merged at
`227f3ad1e3f6bb65a4a2b40b276075157bdf0c81`, with all seven checks passing. It consumes original ownership into the existing native service and provider
launch path, retaining the same ledger and initialized workspace. It restricts the local execution
session to the assigned lane/run and checks the lease immediately before spawn.
[PR #132](https://github.com/idosams/Mesh/pull/132) merged the worker host at
`d5bc096fc7684d1849c12b7fe804b35430d28e58`; all seven exact-head checks and combined-main checks
passed. The host owns that process, native signing sessions and a local endpoint restricted to
scoped fleet calls. Polling does not dispatch another attempt; connection loss does not drop the host.
Local provider startup remains under investigation in [#133](https://github.com/idosams/Mesh/issues/133).

[PR #134](https://github.com/idosams/Mesh/pull/134) merged coordinator identity verification before
worker admission at `5184000c449b0d7284942911796a31040aaeb6ec`, with all seven exact-head checks
passing. Signing bytes must match an already-claimed native attempt. A valid signature cannot
override capacity or recreate a reservation; all 22 focused local admission/launch tests passed.

[PR #135](https://github.com/idosams/Mesh/pull/135) merged bounded control/manifest/chunk framing
at `90eed19f0cc535362d1d47060a193ffc35c349f0`, with all seven exact-head checks passing.
All nine local framing/CAS tests passed, including disconnect, durable reopen and confirmed-offset resume.

[PR #136](https://github.com/idosams/Mesh/pull/136) merged supervisor-owned receiving sessions
at `a915612ad8e32273bf0ed7585c74db6755bd8496`; all seven exact-head and combined-main checks passed.
All 15 focused local receiving/authentication tests passed. Partial input and the original reservation
survive connection loss; saved receipts alone cannot recreate a reservation after supervisor restart.

[PR #137](https://github.com/idosams/Mesh/pull/137) merged the bounded broker command loop at
`fb9a93c61fa4d36736383fed9ad8d3bc56959d04`, with all seven exact-head checks passing. It routes authentication,
manifest/chunk receipt, confirmed offsets and one materialization handoff. A failed final reply retains
the native handoff for the supervisor. Seven focused local broker tests passed. Hosted Linux passed
3,223 tests; macOS passed 3,477 tests, all four renderer cases and the 44-check daemon demo.
A deployed worker, initial worker-proof transport, configured SSH, client reply verification, signed
results/recovery and actual second-machine operation remain unfinished. Evidence is tracked in the
[ledger](plan/fleet-migration.md#r19-bounded-receiving-broker-loop) and
[remote execution contract](plan/fleet-remote-execution-contract.md).

[PR #138](https://github.com/idosams/Mesh/pull/138) merged the complete macOS `npm test` gate at
`5941f60e81b1be60599aa2b2194dc507e060ae9c`, retaining the separate renderer step and all seven jobs.
The literal full gate and all seven exact-head checks passed. Preserved local runs and failures remain
separate evidence. See [R20](plan/fleet-migration.md#r20-complete-hosted-validation-gate).

[PR #139](https://github.com/idosams/Mesh/pull/139) merged at
`ec633abe199092d468a64da4f026cd9726ce2c78` with all seven exact-head checks passing. It connects
the original broker handoff to native workspace initialization,
durable launch intent and `ReceivedWorkerHost`. Integration tests exercise authenticated stream
transfer followed by one fixture-provider process, scoped IPC reconnect, signed checkpoint and saved
review, including loss of the broker's final reply. Changed input must refuse before launch intent.
All six focused native host tests passed. This is local native composition; resident deployment, SSH, signed remote
result recovery and second-machine acceptance remain open. See
[R21](plan/fleet-migration.md#r21-broker-handoff-to-native-provider).

[PR #140](https://github.com/idosams/Mesh/pull/140) merged at
`2b27bd3b561141831133db621c6ffa1e7c171647` with all seven exact-head checks passing.
The coordinator input client connects a native saved-history export to the receiving broker,
verifies canonical reply correlation, checks native authority during transfer and resumes from
confirmed offsets. Lost final replies remain uncertain; retained-only admission sends no input.
All seven focused native transfer tests passed, including preservation of continued live project edits.
This remains an embedding API requiring
authenticated transport, not a deployed remote worker. See
[R22](plan/fleet-migration.md#r22-coordinator-immutable-input-transfer).

[PR #141](https://github.com/idosams/Mesh/pull/141) merged at
`6ce587303a0e3a82a10b65482fdd826f2878aed7` with all seven exact-head checks passing.
It authenticates a coordinator-signed dispatch before worker proof signing. Native
keys, provider and budget/lease policy constrain the request. A real-stream test connects that
handshake to saved-input transfer; all 20 focused proof/transfer cases passed. Combined-main
validation is running. No resident endpoint, key provisioning or signed remote result recovery is claimed. See
[R23](plan/fleet-migration.md#r23-authenticated-dispatch-before-worker-proof).


The input-reconnect increment adds fresh worker identity proof for an already claimed transfer,
without any ledger mutation or second launch claim. Native state supplies the retained assignment;
cancellation, changed leases, wrong peers and running work refuse. Three native regressions were
added; the local linker failed with disk-full `errno=28`, so native validation awaits hosted CI. This is an
integration API; resident transport and real second-machine recovery remain unfinished. See
[R24](plan/fleet-migration.md#r24-fresh-identity-proof-for-input-reconnect).


[PR #142](https://github.com/idosams/Mesh/pull/142) is confirmed merged at
`57b020af3d2504fd8f9073657669edd47c08bfd0`; all seven PR and combined-main checks passed.
The separate local test process remained in macOS startup after successful compilation and is not
counted as a local test pass.

The resident-provider collection now retains received owners independently of broker connections,
keeps failed/completed slots for reconciliation and polls each owner separately. It pins a native
worker key and capacity and refuses duplicate assignments. Two new integration regressions and full
hosted validation are pending after successful native compilation. This does not yet deploy a
resident endpoint or provide remote result recovery. See
[R25](plan/fleet-migration.md#r25-resident-ownership-of-received-providers).


The resident ownership increment [#143](https://github.com/idosams/Mesh/pull/143) is now merged
with all seven PR checks passing. Its separate local run outside the sandbox failed five provider
startup deadlines; [#133](https://github.com/idosams/Mesh/issues/133) remains open.
The next native loop drives retained owners independently of broker requests, with bounded control
and nonblocking observations. Connection loss does not stop it; an explicit native stop preserves
ownership. New lifecycle regressions and full validation are pending. This remains native embedding
support; an installed worker entry point, persistent signing identity and remote result recovery
are unfinished. See [R26](plan/fleet-migration.md#r26-resident-observation-loop).


The resident event loop [#145](https://github.com/idosams/Mesh/pull/145) is merged with all seven
PR and combined-main checks passing. Local provider-startup failures remain tracked separately.
The next custody increment adds source support for persistent macOS worker actor identity, with
create-only provisioning and expected-key checks on each open/sign. It requires the eligible signed
Mesh application identity, reports OS-gated rather than hardware-backed custody, and cannot approve
main. All 45 focused custody tests and crate lint passed; full hosted validation is pending.
No eligible signed-app provisioning or remote deployment is claimed.
See [R27](plan/fleet-migration.md#r27-persistent-native-worker-actor-identity).

The persistent actor backend [#147](https://github.com/idosams/Mesh/pull/147) is merged with all
seven PR and combined-main checks passing. The next increment adds guarded native installation
provisioning and explicit `--worker provision|identity` modes before graphical startup. It preserves
partial setup, binds the expected actor key, and retains parent checks through ledger handles.
Fourteen native directory tests and two command-module tests passed; full hosted validation is pending.
These modes do not start a resident listener or provider. Successful provisioning in an eligible
signed application and actual remote execution remain unverified. See
[R28](plan/fleet-migration.md#r28-guarded-worker-installation-and-native-setup-commands).

The guarded installation increment [#149](https://github.com/idosams/Mesh/pull/149) is merged
with all seven PR and combined-main checks passing. The resident endpoint implementation tracked
in [#150](https://github.com/idosams/Mesh/issues/150) now connects retained authenticated transfers
to an independent provider supervisor through explicit native `serve` and `connect` modes. Native
connection/refusal and CLI tests have passed; the complete endpoint/provider regression and final
validation are pending. This is not verified signed-app or second-machine deployment. See the
[worker configuration](developer-guide.md#resident-worker-configuration-native-implementation-under-validation)
and [R29 ledger](plan/fleet-migration.md#r29-resident-authenticated-worker-endpoint-in-progress).

The next remote increment adds a native-configured SSH subsystem transport with bounded pipe I/O,
independent diagnostic draining and owned-client cleanup. Native coordinator composition now connects it to
signed dispatch and saved-input delivery, with explicit first-claim or reconnect intent. It has no
desktop/agent exposure. Existing host/key files and a separately provisioned
worker subsystem are prerequisites; no trust or SSH settings are changed automatically. Focused
and full validation are in progress. Actual second-machine authentication, renewed leases, signed
result recovery and packaged acceptance remain open; this is not a remote-execution readiness claim.

Native worker status now has a fresh mutually authenticated read-only query path through the resident
broker and configured SSH transport. It recovers signed admission/launch-intent facts across lost
connections without granting execution, retry or lease authority. Five focused native cases passed;
full final-base validation is pending. The facts do not establish current process liveness or saved
results. Lease renewal, result transfer, live presentation and real second-machine acceptance remain
unfinished.


The remote renewal increment adds a separate signed request/acknowledgment protocol. Coordinator
intent is retained before transport; the coordinator advances its lease only after verifying the
worker's durable acknowledgment. A dropped reply can be reconciled with the exact same request.
The original admission and launch receipts remain immutable; current durable lease checks extend
only that original attempt. Expired authorization cannot be newly renewed, and neither expiration
nor renewal frees a slot or proves process liveness. The native resident route and SSH exchange are
implemented; focused tests pass, full canonical validation is pending. Read-only status v1 still
reports explicitly historical initial-lease fields. A versioned effective-lease status query, real
second-machine execution/recovery, signed results, and packaged acceptance remain required.


The effective-lease status follow-up adds an explicit read-only v2 query/reply with separate signing
domains. Existing v1 callers still receive the same historical initial-lease facts. V2 reports the
current guarded worker lease alongside those original facts, including after expiry and durable
reopen. Missing admission remains unknown, and a valid lease never proves process liveness or
permits a retry. Native resident routing and a bounded SSH inspection entry point support both
versions. Eight focused status tests pass; full validation and merged delivery remain pending.


Native received-worker sessions can now export an exact recorded saved review as immutable
content, retaining native custody checks after the execution owner is dropped. The focused
regression passed with later unsaved edits, mismatched selection and replaced-path refusal;
daemon lint passed. Full validation and PR delivery are pending. This is the first source
increment for [#163](https://github.com/idosams/Mesh/issues/163), not signed result transfer or
second-machine acceptance.


The next remote-result increment adds native worker-signed saved-result offers, exact durable
replay and read-only ledger recovery. Three focused regressions and daemon lint passed, including
actual received-session export/signing and coordinator signature/context/manifest refusal. Full
canonical validation is pending. No result transport, complete coordinator import or second-machine
acceptance is claimed.


Known-checkpoint signed offers now have a fresh authenticated query/reply path through the native
resident route and bounded SSH wrapper. Three focused regressions passed, including lost-reply
recovery, stale-query/reply and wrong-domain/key refusal, signed mismatched checkpoint refusal,
and changes to retained history during signing. Daemon lint passed after adding missing API docs.
Full validation is pending. Discovery, content transfer/import and actual second-machine proof
remain required.


The resident worker service now connects original received owners to automatic native saved-result
publication using its configured worker key. The composed regression passes for wrong-key refusal,
retry spacing, durable publication and replay without signing again; daemon lint passes. Full
workspace and hosted validation are pending. This publishes immutable offers locally, not content
to the coordinator. Discovery, transfer/recovery, candidate import and actual remote/packaged
acceptance remain unfinished.


Native result discovery now has a durable per-launch catalog with bounded pages, exact signed-offer
cross-checks and restart-safe cursors. Its focused regression passed for pagination, reopen,
interrupted-publication repair, duplicate publication and corrupted records after fixing explicit
unknown-admission handling. Full checks are pending. Legacy offers require explicit republication
to enter the catalog. Authenticated remote discovery and content transfer/import remain unfinished.


Result catalog discovery now has a fresh authenticated native resident route and bounded SSH
entry point. Three focused regressions passed, including lost-reply recovery and refusal of
signed duplicate checkpoints, wrong assignment, cursor mismatches, stale replies and wrong keys.
Full validation is pending. Catalog discovery does not transfer files, prove completion or import
a result for review; remote content/recovery, candidate import and real-machine acceptance remain.


Native saved-result reopening now works after the original worker owner is dropped, using exact
launch, mapping, installation and immutable review checks. The composed regression passed for
saved bytes despite later working edits, changed receipt refusal and replacement-root refusal.
The first test compile missed a type qualification and was corrected; its log is retained. Full
validation is pending. This is a read-only native foundation for restart-safe transfer, not remote
file delivery or candidate import.


Native signed-result receipt now resumes durable partial chunks in a separately admitted private
store without creating input admission or working files. Its focused test passed interrupted
receipt/reopen, exclusive ownership, invalid manifest before storage effects, offset refusal and
root replacement. An initial test-helper lifetime error was corrected and retained in the log.
Full validation is pending. Network result serving, durable import receipts and actual remote
acceptance remain unfinished.


Authenticated saved-result content transfer now has a distinct native resident route and bounded
SSH entry point. Three focused regressions pass, including lost-connection partial receipt, fresh
resume, incorrect-offset refusal, stale replies, wrong signing domains and keys. The transfer
fixture uses signed synthetic metadata; native exact saved-history reopening is covered separately
by the received-host regression. Full verification is in progress. Durable candidate import and
actual second-machine/packaged acceptance remain unfinished.


Remote result transfer now records a durable coordinator content receipt before sending its end
frame. Native restart recovery verifies the exact signed offer, retained private manifest, physical
store identity and all content. The expanded receipt regression passes replay/reopen and partial
manifest, content mutation and ledger-tail refusal. Full validation is pending. Candidate import,
retention policy and actual second-machine/packaged acceptance remain required.


Native saved-result reopening now also derives exact input-to-result identity correspondence. Two
focused tests pass renames, same-path replacement, deletions, deterministic ordering and malformed
identity/content refusal; the composed worker-history regression passes after correcting its
empty-input expectation. Full validation is pending. Authenticated correspondence transport and
coordinator candidate import remain unfinished; unsigned metadata grants no import authority.


A distinct native resident query and bounded SSH entry point now retrieve authenticated result
correspondence. Focused socket tests passed multi-frame metadata and replay/key/domain/size/offset/
hash refusal; strict decoding tests passed manifest, identity, canonical-form and path checks. A
scoped test-helper borrow error was corrected and its log retained. Full validation is pending.
This is protocol-fixture evidence with separate native-history tests, not actual second-machine
acceptance or durable candidate import.

Authenticated remote result correspondence can now be retained alongside verified native content
and reopened after a coordinator restart. The receipt preserves the first worker attestation and
refuses changed or missing metadata; it does not grant candidate import or main approval. This
source increment's validation and delivery state is tracked in the migration ledger. Real remote
and packaged acceptance, coordinator candidate import and content retention policy remain open.

## Remote result history integration

Remote results now have source support for authenticated immutable transfer, retained correspondence,
independent local history, exact review correlation and bounded offline discovery. The native history
interface resolves a registered result from offer/correlation identities and reads its saved review
and artifacts without renderer-supplied paths or a running worker. Missing or replaced private storage
refuses without repair. This is a received result snapshot, not yet an original-project comparison.

Original-project result import, retention guarantees and the
fully composed second-machine journey remain incomplete. Desktop routing and durable panel selectors
are covered in the next section. Local/hosted native tests do not prove
packaged graphical acceptance. The delivery ledger records exact PR validation and merge status.

### Remote review desktop source coverage

The desktop now has native commands, bounded received-result lists and independent review panels
using exact remote correlation identities. Native reads and the controller/rendered states have
automated coverage. Remote panel selectors and view choices now persist with native revision and shared-capacity checks.
Original-project import and full ingestion/packaged remote acceptance remain unfinished. This source
coverage is not evidence of a signed packaged or second-machine graphical journey.

### Remote results and original-project candidates

Native receiver APIs can stage a retained authenticated remote result privately for its original
attached project and compile operations against that exact saved input. Renames preserve original
object identity; replacements receive new identity. Staging leaves original files, capture history
and Mesh main unchanged. Missing/corrupt evidence, changed main, cancellation, wrong project and
unverified delegated lineage refuse. The signed fixture/native-storage tests cover this direct-input
path; transitive remote lineage and graphical import actions are still open. Native signed
commit/recovery coverage is described below.
See R58 in the [delivery ledger](plan/fleet-migration.md) for the verification boundary.


Native remote candidate APIs also append signed private project versions and record their exact
project reviews, with durable pending/completed recovery. Key refusal, cancellation during signing
and replaced remote custody refuse append. Exact retries reuse retained signatures. This preserves
the separate human approval and original-folder application boundaries. Source/native fixture tests
cover these paths; graphical remote import and signed packaged acceptance remain unverified.


### Composed remote receiving source coverage

A native coordinator API now composes exact selected result transfer, authenticated evidence,
private local history and review registration. Completed retries verify retained state locally without
another connection, signature or allocation. Conflicting selected offers and replaced storage refuse.
The native endpoint integration uses a fixture provider and test keys; real SSH/second-host and packaged
receiving remain unverified. Desktop receiving/import actions and transitive lineage are still open.


Native history APIs now resolve remote project actions from retained offer/correlation identities,
reconstructing the input from original-project saved history. They preserve the existing native
staging, signing/recovery and imported-review checks. The renderer does not choose receiving paths
or supply input manifests. Native fixture journeys pass; desktop action wiring remains outstanding.


The desktop native command boundary now routes exact remote project selections through staging,
recorded import inspection/recovery and imported-review actions. Completed outcomes avoid loading a
signing key; pending/new work uses the existing native signer rules. Closed-request and missing-state
refusal tests pass alongside native recovery journeys. Graphical action controls and durable retry
state are not yet wired, and positive packaged/OS-key acceptance remains unverified.


### Remote original-project workflow in review panels

Received-result panels now expose original-project preparation, private version saving, imported
review creation and exact navigation to the existing project review surface. A separate native retry
store retains fixed inputs before dispatch and survives panel closure/restart without automatic replay.
Explicit status reads and exact retries preserve uncertain outcomes; separate requests can progress
independently. English/Hebrew controls and refusal/restart tests cover this source workflow. Native
human approval and original-folder application remain separate. Packaged, real SSH/second-host and
OS-key acceptance are still unverified; transitive lineage and retention remain open.

Native delegated-input preparation now retains complete local ancestry, original-project object
correspondence and exact saved export bytes together. Reads revalidate the recorded selection and
all retained allocation identities, including after restart without worker adoption. Received remote results can now compose authenticated correspondence with this retained local
ancestry for private original-project import and review. Native checks retain the distinct original
base and refuse cancelled or replaced ancestors around signing. Earlier remote ancestors and the
complete remote dependency/revocation policy remain unsupported. This does not authorize dispatch,
guarantee retention or establish packaged/second-host acceptance.

Remote original-project import now binds its parent review at the first authenticated remote launch
claim. Event replay preserves that exact identity through later duplicate saves and retries, without
rewriting history. Missing or ambiguous evidence at admission stays refused; late review completion
cannot silently authorize import. This does not complete remote dependency/revocation or retention.

Native retained fleet history now prepares single-use remote lease-status and result-discovery
observations. SSH waits occur outside the fleet mutex; reply verification rechecks the exact native
assignment before accepting facts. This preserves access to live views and cancellation during a
network wait. Native coordinator configuration and application controls remain to be integrated;
fixture transport coverage is not real second-host or packaged proof.

The macOS native application now has explicit coordinator status and paged-result discovery
commands before GUI startup. They require an existing private native identity, configured SSH trust,
and a recorded remote assignment in an available fleet catalogue. They observe retained facts
without taking over workers or creating missing history. Source tests cover closed configuration
and unsigned refusal; signed-app custody and real SSH/second-host operation remain unverified.
Graphical controls, remote dispatch setup and composed ingestion are still separate required work.


Guarded fleet storage now supports independent connections retaining the exact native authority and
ordinary revision/idempotency checks. Missing history cannot be recreated by reopening a connection.
This is a prerequisite for moving result ingestion outside the service mutex; the service integration
and end-to-end responsiveness evidence remain outstanding.


Native retained history now composes selected remote result receiving and local review through an
independent guarded ledger connection. Network/file work holds no fleet service mutex; native
fixture readers remain available during transport, and completed results recover offline without
worker adoption. The closed native caller must still supply independently admitted configuration.
Operator receiving controls, real SSH/second-host and packaged acceptance remain outstanding.


The macOS native coordinator command now receives an exact selected remote result from a private
configuration, resolving input from an existing saved project version or managed lane review. It
protects original/history storage, reuses the unlocked ingestion service and reports local review
identities. It requires a previously admitted assignment and eligible signed application; tests
cover native saved-input selection and refusals, not real SSH/OS-custody acceptance. Graphical
receiving and coordinator dispatch/provisioning remain outstanding.


Native coordinator source now connects saved-project fleet creation to an explicit initial remote
input transfer. Stable request lookup recovers catalogue identities after lost output without
adopting execution. The service leaves live views available during network waits and refuses repeat,
cancelled or restored starts. Input acceptance does not establish provider startup. The full source gate passed, including 3,615 native tests; real signed-app/OS-key, SSH/second-host, restart reconciliation
and graphical operator acceptance remain outstanding.


Native coordinator input recovery now has an explicit command for an already claimed initial
transfer. Retained catalogue history can resume its exact saved input after coordinator restart,
without adopting the local lane or dispatching/renewing an attempt. The resident worker must retain
its receiving reservation. Full source validation passed: 3,620 native tests, 170 rendered tests,
599 desktop tests and 44 daemon-demo checks. One background-capture test reported a process-leak
warning; issue #172 remains unresolved. This is not general lost-worker recovery or real
SSH/second-host acceptance.

The graphical fleet surface includes a session-only native configuration picker and explicit remote
status/result-discovery reads for the application's retained fleet history. Public target labels and
bounded observations are displayed without exposing credential paths or adopting a worker. This is
not remote launch/receive/reconnect UI, live process proof
or packaged second-host acceptance. See the developer guide and latest migration-ledger increment.

The remote observation panel also has in-app setup: native selectors for an existing
coordinator identity and SSH files, public connection fields, and fleet/lane choices from the retained
catalogue. Using settings performs no network connection. Changed files and stale drafts refuse;
setup still requires eligible signing custody and existing worker provisioning. Named saved settings
now retain native configuration and original identity/file bindings outside the project. Load, open,
save, remove and interrupted-save recovery are explicit actions; reopening re-admits the original
files and identities without contacting or adopting a worker. The renderer receives public fields
only. Remote execution/recovery controls and signed-app/second-host acceptance remain unfinished.


The remote panel now exposes explicit initial-input recovery through the existing catalogue owner.
Native history derives an attached root version or exact reviewed parent checkpoint; renderer input
contains only the current connection ID. Transfer retains the original assignment and never renews,
adopts or creates another attempt. Lost worker reservations, missing parent/source history, complete
remote launch/receipt UI and real signed-app/second-host acceptance remain unfinished.


Remote result discovery now displays individual saved-result identities and explicit bounded previous/
next pages. The non-empty-page cursor bug is fixed: the UI validates the returned next cursor against
the requested cursor and row count. Private offer correlation stays native-only; no download, result
receipt or acceptance is inferred from the list. Graphical result receipt and signed remote acceptance
remain unfinished.


Native saved-input reconstruction now separates historical result lookup from reconnect eligibility.
It verifies an exact recorded assignment's input version and manifest against native saved history,
including after cancellation or newer attempts, without recreating execution authority. Reconnect
keeps its stricter eligibility checks. Graphical receipt storage/intents and download controls remain
unfinished; this native preparation is not evidence of a completed remote receipt journey.


## Native receiving inbox prerequisite

`RemoteResultInbox` creates private `remote-results` storage only beneath an independently admitted
native application directory. An existing or partial inbox is preserved and refused. The native
owner must durably retain the returned installation identity outside the inbox before transport;
reopening requires that identity and validates the exact store/allocation folders against the
owner-only `mesh.remote-result-inbox/v1` record. Missing, replaced, linked, malformed or newly
protected storage refuses without repair. This new record has no migration from unknown folders.
This is receiving storage infrastructure, not an exposed graphical download action: durable
receipt intents, authenticated result selection and graphical receipt/recovery remain required.


## Retained native receipt selections

On macOS, matching the existing signed-result and ingestion API, an admitted receiving inbox can
retain up to 64 immutable `mesh.remote-receipt-intent/v1` records.
Each record binds the exact signed offer, stable allocation, inbox installation and private native
configuration. Identical retention is idempotent; changed context for the same offer refuses.
Listing revalidates signatures, canonical records, physical inbox identity and bounded private
files. Partial, copied or malformed records remain preserved and require reconciliation. No
automatic deletion or migration is provided. A saved intent does not prove any content arrived:
the caller must re-admit peer/configuration/assignment/trust and query the fleet's completed review
ledger. These primitives still require desktop integration and explicit graphical recovery.


## Catalogue-owned receiving storage

`AttachmentStorage::remote_result_inbox` retains the inbox installation outside the receiving
folder in an owner-only `mesh.native-result-inbox-binding/v1` catalogue record. Native callers
supply an independently admitted application parent; saved catalogue and parent identities must
still match on reopen. Provisioning is explicit and lazy. An inbox without its catalogue binding,
a partial binding, or substituted storage refuses without adoption, deletion or repair. All
registered project identities, including detached/offline projects, are automatically protected;
renaming a source does not remove that protection. The format is additive; unknown prior inboxes
require reconciliation. This supplies native lifecycle integration, not graphical receipt or proof
that a remote result has arrived. Desktop download/recovery controls remain unfinished.

Native application-data parents may be readable/searchable by other users, as in the desktop's
ordinary startup layout. Group/other-writable parents refuse. Receiving folders stay owner-only,
and the catalogue binding and receipts remain owner-only files; no permissions are changed on
an existing parent or project.


## Graphical remote result download and saved-attempt recovery

In the macOS remote worker panel, **Find saved remote results**, then **Download for review** on
one exact result. Native code retains at most the last authenticated page's sixteen signed offers;
the renderer supplies only its current selection and an offer digest. The app-owned catalogue
creates/reopens private receiving storage, retains the exact offer/configuration/allocation before
I/O, reconstructs the original immutable input and uses the existing guarded ingestion service.
The app's fleet owner is reused; no second catalogue owner is opened for the command.

**Load saved downloads** lists exact attempts for the current native connection and bindings.
After reopening that connection, **Check or resume saved download** uses the original retained
intent and allocation, without requiring the remote result to be on the current page. Completed
replay revalidates local content/history without another transfer; incomplete stages may resume
only through existing guarded ingestion. Partial/unacknowledged allocations and changed native
bindings still refuse and remain preserved. Listing alone never transfers content. No automatic
retry, provider launch, original-project write or protected-main approval is requested.

Only a verified completed receipt enables **Show downloaded reviews**, which opens the existing
fleet received-result queue for parallel pinned review. A stale/mismatched native reply does not
show completion. Errors retain native attempts for explicit recovery. The UI is localized in
English/Hebrew and disables duplicate in-flight actions. Storage formats remain unchanged; paired
native/UI receipt envelopes are new v1 messages. Eligible native signing remains required. Actual
packaged signed-app/SSH second-host download and recovery acceptance are still unverified; the
fixed user test checkpoint is unchanged.

## Retained remote creation requests

Native coordinator start now retains its exact private configuration in the attachment catalogue
before fleet allocation, custody or transport. The immutable request shares the catalogue's
32-character creation key. Repeating identical inputs is idempotent; changing the peer, saved
version, goal, provider, limits or deadline under that key refuses. Reading retained inputs does
not allocate, connect, renew a lease or assert completion. The existing `created` inspection
remains read-only and compatible with earlier configurations.

The additive `mesh.native-remote-start-request/v1` records bind the physical catalogue and use
owner-only, single-link, bounded files. At most 64 are retained within the catalogue's existing
256-entry discovery bound. Partial, copied, substituted, oversized or conflicting records refuse
without repair, deletion or eviction. Consumers must independently re-admit all native bindings
before an explicit action. Graphical fresh-start setup and request recovery are the next integration
steps in issue #218; this foundation does not establish a signed packaged or real second-host journey.

## Desktop fresh remote creation and original-attempt inspection

The native setup draft can prepare peer configuration without a fake fleet/lane/run. The desktop
form selects an attached saved version, goal, one provider and limits; a native draft-derived
creation key, fixed fifteen-minute deadline, private configuration and original native bindings
are retained before any fleet allocation or transport. Exact repeated preparation retains the
original deadline. Public projections contain request metadata, never private key/trust paths.

Explicit send reuses the app-owned catalogue/service and existing saved-input dispatch. The shared
CLI/native transfer helper is unchanged in authority: it refuses all already-dispatched attempts
and input acceptance does not prove provider execution. Restart listing never dispatches. Explicit
inspection finds the original catalogue request and, where its exact attempt exists, selects it
for the existing observation/reconnect controls. No automatic retry, lease renewal, process
adoption, source write or protected-main approval is added. The existing request-journal format
stays v1; desktop native configuration/bindings and public UI envelopes are additive v1 schemas.

Controller/rendered tests and native admission/host-journal tests cover the flow's deterministic
boundaries. Eligible signing and actual SSH remain external acceptance requirements. A live signed
packaged fresh-start/recovery journey, successful remote provider execution, missing-assignment/lost-reservation
reconciliation and the full fleet acceptance plan remain unfinished.

The existing worker service queues execution after input materialization. The desktop send copy
explicitly describes that authorization while keeping input receipts distinct from provider liveness.

## Remote review navigation independent of live status

The desktop no longer discards a received-result navigation request because its fleet is absent
from the last status poll. A closed, bounded objective selector goes to the existing native
retained-history reader, which remains responsible for catalogue admission and exact page/content
verification. No new native authority or persistence format is introduced. Unknown/missing native
history produces a visible queue error rather than a silent no-op; no storage is initialized by the
read. Up to sixteen queues and eight combined pinned panels remain enforced.

Queues whose status card is not loaded render independently and move into the card when it appears.
Unavailable execution ownership does not suppress an explicit retained-history read. Failed reads
cannot enable pinning and do not replace existing pinned selections. This improves the deterministic
download-to-review path; actual signed packaged and second-host acceptance still remain required.

## Durable remote materialization identity

The receiving broker now commits an immutable native allocation identity before returning a
materialized handoff or acknowledging it to the coordinator. The record binds the original
admission and physical receiving parent, allocation and input directory. Exact replay is
idempotent; changed inputs, mismatched admission, malformed records and extra events refuse
without deleting retained work. Reading it grants no allocation, lease renewal or launch.

The additive `mesh.remote-materialization/v1` record lives in the existing guarded worker ledger.
Old admissions without this record remain readable but have unknown materialization identity;
absence never proves that no input or process exists. No automatic backfill or adoption occurs.
This is the first native foundation for #222, not completed worker restart recovery. Signed
packaged, real second-host and full fleet acceptance remain outstanding.

## Read-only acknowledged input inspection

Native worker recovery can now inspect an acknowledged allocation from its durable
materialization receipt after reopening the destination. Inspection checks the original parent,
allocation and files directory identities before reading a bounded private manifest, verifies
the exact assigned input and complete file inventory/bytes, and rechecks identity afterward.
A byte-identical replacement directory is refused. Changed or missing input, and linked, public or malformed
manifests, remain preserved; inspection does not repair it.

The reader does not initialize a workspace, create a handoff, recreate a reservation, renew a
lease, adopt a provider or write to the ledger. Successful input inspection is not execution
liveness or permission to retry. This extends #222's native recovery foundation; graphical
reconciliation, interrupted initialization, provider recovery and signed second-host acceptance
remain unfinished. Existing persistence formats are unchanged.

### Explicit retained-input inspection (source increment)

The native remote status protocol now has an explicit v3 inspection request. It reports whether
the original acknowledged input is unrecorded, verified at observation time, or unavailable, with
fresh coordinator/worker authentication and pre/post-signing verification. Routine v1/v2 status
queries remain ledger-only. Inspection never resumes work, adopts a process or authorizes retry.
The SSH helper and resident worker route are connected; desktop controls and actual signed
packaged/second-host acceptance remain unfinished. See the [acceptance map](plan/fleet-acceptance.md).

### Explicit desktop original-input inspection (source increment)

The worker panel now exposes **Inspect original input** separately from routine status and transfer
resume. Native service preparation signs the exact v3 request, releases the fleet lock for the
exchange and revalidates the attempt before projecting the result. English/Hebrew UI presents
verified, unavailable or unrecorded input with its observation time. Old facts remain dated on
failure; changing the selected connection clears them. No launch or retry authority is added.
Native CLI `--coordinator inspect-input` uses the same route. The full local gate passed; fresh
hosted validation and signed packaged/real-host acceptance remain required for this increment.

### Received-workspace initialization ownership (source increment)

The original received workspace now retains an independent native allocation lock before writing
initialization intent and through its session lifetime. A competing cooperating initializer refuses
without replacing input or receipts. Read-only inspection remains available. This supplies per-attempt
exclusion for upcoming restart recovery; it does not reopen an interrupted workspace, prove an old
process stopped, release capacity or authorize replacement execution.

### Received initialization failure retention

Received worker initialization now preserves its partial copy, pending markers and any written
private history when import fails or its prepared handle is dropped. A changed working copy is
retained for inspection rather than removed by ordinary import rollback. Focused fault tests and the
full local gate pass; hosted validation and canonical delivery are tracked in R96 of the migration ledger. This is required
recovery groundwork, not a resume button or permission to launch another process. Normal project
imports keep their existing behavior; the fixed user checkpoint is unchanged.

### Received initial-history completion

Received initialization can complete only the exact intended initial journal prefix after verifying
its required payloads. It preserves conflicting history and does not append an already-complete
initial history again. Byte-cut/refusal tests and the full local gate pass; the preserved process-leak
warning, fresh hosted validation and delivery are tracked in R97 of the migration ledger. This internal path does not yet let a user resume an interrupted remote
worker: input-copy and workspace/receipt reconstruction plus authenticated recovery remain unfinished.

### Received partial-file completion

Native received copying can append the verified missing suffix of its original file while retaining
the same physical file. Changed or conflicting work is preserved and refused. The streaming path
has byte-cut, permission and replacement regressions; R98 in the migration ledger tracks validation
and delivery. This does not yet expose remote resume: original-attempt authority, prepared workspace
reopening and receipt reconciliation remain required before authenticated recovery can proceed.

### Received-import finalization (source increment)

Received initialization can complete an exact import-receipt prefix and accept an already-created
empty index placeholder without replacing either file. Conflicting or populated files and changed
physical ownership refuse intact; ordinary imports remain create-only. The focused suite passed
36 tests, including every real receipt byte cut and repeated ingestion. The full local gate
also passed 3,712 native tests, 181 rendered checks, 634 desktop tests and all 44 daemon checks.
R100 in the migration ledger tracks hosted validation and delivery. This does not yet expose a resume action or reconstruct
launch authority; original ownership/admission/lease checks and the remaining recovery journey
are still required.


### Original-directory import handoff (source increment)

Confirmed saved-work and received-worker imports now retain their original working and private
storage directories while opening the live workspace. Durable index and recovery-database access
use verified native directory references, and the displayed private-store identity remains checked.
A complete replacement workspace at the same name is refused before mutable reopening. Normal
explicit reopening of a confirmed workspace remains supported. The focused regressions and full
local gate passed; R101 in the migration ledger records exact validation and hosted-delivery status.
This does not yet reconstruct interrupted imports or expose authenticated remote resume. The user
checkpoint remains a fixed earlier packaged build while this recovery work continues.


### Original worker initialization recovery (native source increment)

An authenticated native receiving session can continue an original acknowledged initialization
under the guarded worker ledger and exclusive allocation ownership. It handles pending and confirmed
imports, preserves original history and identity, and finishes matching partial receipts without
replacement. Conflicting work, replaced directories, another initializer and existing launch records
refuse intact. This does not adopt an uncertain process or release capacity. R102 records validation.
Dedicated broker recovery authentication/routing (including renewed assignments), desktop controls
and real-host fault acceptance remain unfinished; the fixed user checkpoint does not include this.

### Renewed-lease recovery authentication (in validation)

A distinct native recovery proof binds original admission and directory identities to the current
worker lease. Coordinator signing rechecks cancellation and the selected work after the signer
returns; worker recovery rechecks proof expiry and exact lease throughout initialization. This is
connected to native recovery, but broker/supervisor/desktop routing and real second-host acceptance
remain unfinished. R103 in the delivery ledger tracks tests and publication; this is not included in
the fixed user checkpoint.

### Recovered ownership and provider startup (in validation)

Authenticated original workspace recovery now has a native handoff through the existing provider
supervisor and bounded resident mailbox. Startup uses the ordinary single launch-intent check;
reply loss preserves the owner, delivery backpressure returns the held workspace, and failed starts
retain uncertain slots. Three fixture-provider regressions passed. R104 records full validation and
publication status. Recovery wire routing and user-facing controls remain unfinished.

### Original recovery wire and worker routing (in validation)

The typed coordinator exchange and opt-in native worker endpoint now connect signed original-input
recovery to the resident supervisor. Exact original assignment, policy, limits, lease and fresh
signatures are checked; a lost final receipt retains the recovered workspace and mailbox request.
Eight focused native regressions passed, including a real local fixture process after lost reply
and closed-mailbox recovery. The full local gate passed 3,742 native tests plus desktop and real-daemon checks;
hosted validation and delivery remain pending in R105 of the migration ledger. Coordinator SSH/application routing and graphical recovery controls remain unfinished;
this is not packaged, real-provider or second-host acceptance.

### Coordinator original recovery command (in validation)

An explicit native coordinator command now connects original initialization recovery to configured
SSH transport and eligible signing custody. It releases the fleet lock during transport and signing;
service cancellation during signing prevents the request. Focused native tests and the full
local gate passed (3,744 native tests plus desktop and daemon checks); hosted validation
and delivery are pending in R106. Graphical controls, successful signed second-host
execution and packaged acceptance remain unfinished.

### Desktop original workspace recovery (in validation)

The remote panel now offers explicit original-workspace recovery through the selected native
connection. Copy explains possible assigned-agent startup and distinguishes recovered setup
from running execution. Duplicate requests are suppressed and uncertain outcomes preserve the
original selection and observations. Focused controller/rendering and native refusal tests passed;
R107 full local validation passed (3,745 native tests plus desktop and daemon checks);
hosted validation and delivery are pending. Signed packaged and real second-host recovery
remain unverified.


### Recorded remote execution presentation (R113)

The coordinator and desktop now expose an explicit read of the original worker's signed
execution history, including incomplete setup and a stop request whose termination is not
confirmed. The panel retains the observation time and exact recorded revision and clears facts
when the selected connection changes. This extends R111/R112 without implying live process
proof, capacity release, another authorized attempt or accepted work. Focused validation passed (45 native and 49 controller/rendered tests), followed by the full
canonical gate (3,764 Rust, 183 rendered and 641 desktop tests plus the real daemon demo).
Hosted exact-head checks and merge remain pending; packaged signed remote acceptance and the
full fleet plan remain open. The fixed user testing checkpoint does not include this work.

### Fleet observation signing responsiveness (R114)

A focused regression reproduced the shared service lock being held during native observation
signing. Preparation now retains guarded history on an independent connection and releases the
lock before signing, allowing concurrent views and cancellation. Stale context and lost authority
still refuse the query. Eight focused tests and the full gate passed (3,764 Rust, 182 rendered,
636 desktop tests and the real daemon demo). After combining the execution panel, all 49 focused
native tests and the full gate passed (3,768 Rust, 183 rendered, 641 desktop tests and the daemon
demo). Hosted delivery remains pending; packaged responsiveness is not yet established.
This correction does not resolve terminal capacity reconciliation or the full fleet objective.


### Current real Codex four-worker measurement (R115)

On canonical main `1c0bcd1788878bcbca8936db1c436e8c772fb841`, the real native
four-worker comparison passed with `codex-cli 0.158.0-alpha.2.1`: 184.925s serial,
64.458s parallel, and four acknowledged overlapping workers in the parallel phase.
It verified one attempt per lane, exact saved reviews and unchanged original state.
The [measurement record](plan/evidence/fleet-four-worker-2026-10-04.json) binds these
results to the source revision, binary hashes and preserved raw evidence. GUI latency,
provider cost, resource usage, human coordination and accepted-main timing remain
unmeasured; this does not complete packaged, second-provider or second-host acceptance.


### Remote assignment overview (R116)

The native fleet catalogue now retains each current remote attempt's assignment, worker identity
and exact lease values. Desktop cards distinguish coordinator records from remote observations
and suppress local-activity joins for those lanes. Existing local/legacy catalogues remain readable.
Controller/rendered regressions reproduced the missing distinction; native persisted-attempt,
malformed-fact and English/Hebrew checks cover the new projection. The full gate passed 3,769 native,
184 rendered and 642 desktop tests plus the real daemon demo. Hosted validation and merge remain
pending. Independent fleet-wide remote refresh and signed packaged/second-host acceptance remain
open in [issue #250](https://github.com/idosams/Mesh/issues/250).


### Independent remote fleet observations (R117)

Visible fleet cards now request authenticated original-attempt execution records independently,
with at most four native reads, per-lane failure/age and retained last verified history. Each read
matches one saved native connection to the current assignment, revalidates original physical
bindings and refuses changed assignments/settings. The existing selected-worker panel and saved
review queues remain independent. Native and controller tests cover bounded reads, exact identity,
late replies, revision regression, hidden views and retained errors. Full validation passed 3,771
native, 185 rendered and 646 desktop tests plus the real daemon demo. Hosted checks and merge
remain pending. This is source-level functionality; signed packaged and actual second-host fleet
acceptance, terminal reconciliation and the full fleet plan remain unfinished.


### Installed Codex application layout (R118)

The packaged local fleet acceptance preparation found that the installed Codex CLI had moved
inside a nested app bundle, while desktop discovery searched only the older resource path.
Explicit-path native provider tests did not cover this desktop boundary. Discovery now supports
both layouts in the existing supported application locations, preserving regular-file checks and
native adapter admission. Two native regression tests failed with legacy-only discovery; all three focused tests now pass.
Full validation passed 3,771 native, 183 rendered and 641 desktop tests plus the real daemon demo.
Hosted delivery and actual corrected packaged provider launch remain pending.


### Collector process-crash verification (R120)

A new deterministic campaign kills an acknowledged disposable collector at eight deletion/journal
boundaries, reopens it and verifies retained bytes before completing collection. It detects removal
of the reference veto. Production cleanup behavior is unchanged. This improves component-level
process-crash evidence; fleet root selection, writer coordination, cleanup policy/scheduling and
storage-exhaustion acceptance remain unfinished in [issue #256](https://github.com/idosams/Mesh/issues/256).


### Buffered-history retention correction (R121)

Conservative collection roots now preserve every recorded actor operation, including saved work
whose causal parents have not arrived and disconnected history outside the current actor head.
Three baseline failures reproduced the omissions; all 13 focused retention and independent GC
tests pass after the correction. Unknown actor roots still refuse and explicit narrower policies
remain explicit. This changes retained-set computation, not causal readiness or persisted formats.
Native fleet cleanup, scheduling, writer coordination and storage-exhaustion acceptance remain open.


## Collection under injected storage exhaustion

The native CAS cleanup campaign now checks eight Unix ENOSPC boundaries, including a partially
written replacement journal. It verifies retained review bytes, honest error reporting, exact
partial deletion, reopen/retry and a subsequent write. A swallowed-error mutation is rejected.
The host disk is not filled. Full local validation passed 3,780 native, 185 rendered and 646 desktop
tests plus all 44 daemon checks; hosted delivery is pending. This does not complete
native cleanup scheduling, cross-process coordination or the full storage-pressure journey.


## Explicit native orphan cleanup

The daemon now has an exact-workspace cleanup operation that keeps all recorded history and removes
at most 256 unreferenced arrival candidates. It holds native writer custody, refreshes durable
history and recovery state, and refuses stale, assigned, incomplete or replaced workspaces.
Nine focused native regressions and the full gate pass: 3,788 native, 185 rendered and 646 desktop
tests plus all 44 daemon checks. Hosted delivery is pending. Reads remain
available during a test-paused deletion while a second daemon's agent acquisition waits. Automatic
scheduling, pressure policy, full cross-process faults and packaged fleet acceptance remain open.


## Cleanup preparation and review responsiveness

Cleanup prepares fresh retention facts using an independent pinned journal descriptor after
releasing the live view and checkpoint locks. Native custody still excludes coordinated writers.
A paused-preparation test keeps workspace reads available; restoring the old view lock makes that
test fail. Identical-byte journal replacement also refuses. Full local validation passed 3,790 native,
185 rendered and 646 desktop tests plus all 44 daemon checks; hosted delivery is pending. Nonblocking background admission and automatic scheduling remain unfinished.


## Cleanup admission during active work

The native maintenance entry point now defers if writer custody or any daemon admission lock is
busy, and defers assigned-agent workspaces. Tests hold each of the five admission locks, require
a response before release, preserve the orphan, then successfully retry. Stale identity and nested
mutation refuse. Replacing a try-lock with a blocking lock fails the regression. This prevents
queued admission; filesystem and admitted scan latency remain unbounded. Automatic scheduling,
pressure policy and full fleet acceptance remain open. Full local validation passed 3,793 native, 185 rendered and 646 desktop tests plus all 44
daemon checks. Hosted delivery is pending.


## Periodic cleanup ownership

Desktop and headless daemon now start one periodic cleanup owner. It waits 60 seconds between
attempts, selects the exact open workspace, defers busy/assigned workspaces, and retains all
recorded history. Sleeping workers hold only a weak daemon reference; owner drop wakes and joins
the worker. Four new native regressions prove scheduled deletion with retained history/reopen,
busy retry, torn-history refusal, unique ownership and wakeable shutdown without a reference cycle.
All 17 focused cleanup tests pass. A dry-run-only mutation fails the actual deletion assertion;
source was restored. Full local validation passed 3,797 native, 185 rendered and 646 desktop
tests plus all 44 daemon checks. Hosted delivery is pending. No packaged scheduled-cleanup
proof, closed-workspace traversal, pressure policy or bounded admitted scan latency is claimed.


## Worker progress inspection

Native progress inspection distinguishes unchanged folders, supported edits/additions and entries
requiring explicit resolution. The existing missing-file read shares this path and no longer holds
the fleet mutex during native folder scanning. It suppresses results if the run is cancelled or
the credential is revoked/rotated during inspection. Four native regressions pass, including a
paused scan with parallel fleet reads; restoring the old lock fails that test. Full local validation passed 3,801 native, 185 rendered and 646 desktop tests plus all 44 daemon
checks. Hosted checks are pending. This read never saves content or creates checkpoints. Automatic worker
saving, cancellation-safe capture, scheduling and packaged acceptance remain unfinished in
[issue #267](https://github.com/idosams/Mesh/issues/267).


## Local checkpoint signing and fleet responsiveness

Local explicit checkpoints now scan and sign outside the fleet mutex, checking the exact current
grant and run before and after signing. Native custody never waits for a contended fleet mutex;
that attempt refuses or records an incomplete result. Tests prove parallel reads while signing
is paused, exact retry, cancellation/revocation refusal and real concurrent credential rotation
without deadlock. The previous capture path fails the read deadline mutation. Full local validation
passed 3,805 native, 185 rendered and 646 desktop tests plus all 44 daemon checks. Hosted
verification is pending. Received remote sessions retain their original authority path.
This is a prerequisite for automatic saving, not a scheduler or packaged provider acceptance claim.
