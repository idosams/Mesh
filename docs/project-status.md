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

## Implemented source coverage

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
