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
revocation and newer-version races. A real Codex worker also produced an inspectable intermediate save while active and before any
explicit handoff, with exact saved bytes unchanged after its final edit. This is native-provider
evidence, not packaged acceptance. Received remote
sessions and the older c2641c6 checkpoint do not include this behavior. The separate 5052009 checkpoint includes the local implementation. See the delivery ledger for
R130 evidence and limits.

## Ordinary saved-progress inspection foundation

Native history reads can enumerate retained local lane operations in pages of 50 and compare an
exact saved operation with its original input. They remain readable after session revocation and
restart without adopting a worker or creating a handoff. Recorded operations may be intermediate
captures; listing one makes no completeness or approval claim. The latest fleet-acknowledged
version is reported separately. Read-only desktop commands and strict response validation expose this history without review
identities. The desktop now offers separate progress lists and up to four independently pinned comparisons.
Pins are fixed to exact saved versions and remain available during list refresh. Panel selectors,
layout, page cursor and selected object now persist through the separate native store. Restart
re-reads exact native history without requiring live fleet ownership. Unavailable content retains
its selector and shows a read error; failed saves expose retry/reload controls instead of claiming
persistence. Each lane now exposes its durable latest acknowledged save in the polled overview, including
restored lanes. The shortcut pins the version shown when clicked, even if later saves arrive
during loading. Missing legacy fields mean unknown, not unsaved work. Broader changed-file/live
activity summaries and packaged parallel-inspection acceptance remain unfinished. Exact pinned comparisons now separate native file and folder counts
across the entire saved comparison, independently of the displayed page or selected object. Older
replies retain an unknown split instead of inventing zero files. These counts describe retained
saved entries, not current working files, capture completeness, or the broader live lane overview. A separate
native summary command now exposes the same exact counts and bound version identities without
returning file names or content. It reuses verified retained history outside the fleet lock and
refuses unavailable or replaced custody. A strict desktop parser bounds and validates the response.
Local lane cards now refresh these summaries independently of status, commands and pinned reviews.
At most two summary reads run concurrently, with fair scheduling over the bounded catalogue,
coalescing of newer saves, cached immutable results and retries no sooner than five seconds after a
failure. Older counts retain their exact version label during newer reads or errors; unavailable
history never becomes zero files. Hidden/disposed views stop new dispatch. Restored local lanes
remain readable without worker adoption, while remote assignments retain their separate observation
path. This covers saved-change counts in the local overview; broader activity summaries and packaged
parallel-agent acceptance remain unfinished.
Lane cards also project recorded handoff completeness and submitted-review counts for the exact
latest saved version, plus unconfirmed capture intents and open native change requests across the
lane. This aggregate reads the refreshed durable state once, exposes no feedback text, and does not
claim tests, dependency eligibility or approval. Older replies remain explicitly unknown.

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

## Saved-progress test checkpoint — 4 October 2026

A separate fixed checkpoint at `5052009ee4fe57a468bca4adec43de6f93c4c6a0` includes ordinary
local worker saving, up to four exact progress comparisons, durable panel selectors and the
latest-save shortcut. Its launcher uses separate application data and retains the original Codex
configuration location without copying credentials. Earlier checkpoints and running apps remain
preserved. A sample project and six-step guide cover ordinary edits, parallel pins and reopening.

The exact package passed embedded-revision and resource-seal verification before and after the
packaged native bridge test. That test passed in 1.35s with two delegated lanes, retained review,
feedback, exact retry and revoked-session refusal. It opened no graphical interface and launched
no provider. This development checkpoint is ad-hoc signed, not notarized, and provides no protected
main approval. Its feature stack was published in [PR #274](https://github.com/idosams/Mesh/pull/274)
through [PR #278](https://github.com/idosams/Mesh/pull/278); package availability does not imply
merged delivery. The Mac remained locked when graphical acceptance was attempted. Parallel real
agent activity with packaged panels, restart recovery through that interface and the full acceptance
plan remain unverified. See R139 in the delivery ledger for exact artifact identity.

## Managed approval history freshness

Managed-workspace approval refreshes durable history under its native custody lock before checking
the displayed workspace identity or admitting a receipt. Two independently opened clients can no
longer use an older cached history to append a second conflicting approval after the first client
commits. A native regression reproduced the previous append-before-refusal failure and now verifies
that refusal leaves the journal byte-identical and the first receipt recoverable after reopening.
This uses test credentials; it does not establish eligible signed GUI acceptance or implement
private-dependency grants, closure, rejection or revalidation.

## Native dependency barrier foundation

Native history initialization can hold a bounded set of exact directory identities using the same
kernel locks as ordinary single-workspace custody. It orders acquisition deterministically,
revalidates namespaces, refuses nested expansion and releases partial acquisitions on failure.
Guards stay on their owning thread. Separate-process tests verify contention in both orders and
refusal/cleanup when a root is replaced during a wait. This groundwork does not yet enable private
dependency grants, persist eligibility decisions or enforce transitive rejection at publication;
those remain in the [native dependency contract](decisions/private-dependency-authority.md).

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


## Provider group cancellation

Providers now start in separate native process groups. Stop and launch-abort paths signal the owned
group before reaping its leader, and refuse numeric group signaling after recorded exit. A real
process regression proves prompt inherited-pipe descendant shutdown while an unrelated process
remains alive. Restoring direct-child-only cancellation fails this regression; source was restored.
Full local validation passed 3,798 native, 185 rendered and 646 desktop tests plus all 44 daemon
checks. Hosted delivery is pending. This does not prove escaped descendants are gone,
release remote capacity, resolve the intermittent warnings in issue #172, or establish packaged
cancellation and restart acceptance. Remote terminal capacity reconciliation remains unfinished.


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

## Dependency journal foundation

R147 adds required dependency envelopes, ordered replay and a reconstructible SQLite table.
Native open/refresh and cached approval refuse these histories until semantic policy validation
exists, and collection refuses unknown dependency roots. Fixed-frame, malformed-record, replay,
SQLite reconstruction, crash-boundary and native refusal tests pass. An independently compiled
pre-change storage scanner refuses the new kind without changing the fixture. This does not prove
running-old-desktop compatibility; enrollment must first fence cached old writers. No grants,
private consumption or publication eligibility are enabled yet. Full repository and hosted checks
remain pending. See the [dependency contract](decisions/private-dependency-authority.md).

## Native dependency payload history

R148 validates canonical dependency payloads against an independently supplied native project binding
and their stored envelope/digest, then reconstructs grant generations, consumption bindings,
per-input decisions and historical review vectors. Nine focused tests cover malformed and substituted
records, atomic refusal, revocation, independent decisions, replacement, retained direct references
and complete count/byte limits. Weakening grant or eligibility checks fails the regression. This is
read-only historical validation, not native control authorization or full dependency-closure proof.
Consumption and publication remain disabled for dependency-bearing histories; writer fencing and
runtime integration are still required. Full and hosted checks are pending.

## Dependency enrollment preparation

R149 adds a native-only required custody marker that blocks generic older writers before future
policy enrollment. Exact retry synchronizes the marker again; assigned or substituted workspaces
refuse and interrupted preparation preserves explicit recovery state. Eighteen focused custody tests
and a cached native-writer regression pass. An independently built pre-change reader also refuses
cached edit/create/review/agent-acquisition paths after the marker, with unchanged file/journal bytes.
This does not prove valid human approval, every writer or complete enrollment recovery. There is no
agent/renderer/CLI enrollment operation and no automatic migration. Full and hosted checks remain
pending; see the [authority contract](decisions/private-dependency-authority.md).


## Attached-project dependency preparation

R150 adds the separate required attachment-history binding needed to fence older attached approval
and capture paths. Native tests preserve a previously prepared receipt, source content and journal,
refuse approval/capture after preparation, recover exact retries and detect changed source/binding.
Injected acknowledgement-sync failure retains the fence; bypassing sync fails the regression.
This native primitive is not exposed to users or agents and does not enroll policy. Current Mesh
history readers refuse its required binding until complete enrollment/recovery support is integrated;
ordinary editors remain usable. Full local validation passed: 3,840 native, 194 rendered and 672 desktop tests plus all 44 real-daemon checks. Hosted validation is pending. The fixed checkpoint is unchanged.


## Recoverable native dependency enrollment

R151 joins registration, retained CAS intent, both required old-reader fences and an exact enrollment
journal append. Recovery finishes only its own anchored frame prefix, re-syncs exact retries and
refuses substituted or conflicting history. Five real-storage regressions cover all 146 frame-prefix
boundaries, acknowledgement/sync failure and identity refusal; exact borrowed custody and retained
accepted-history checks also pass. Removing sync fails the regression. Full local validation passed: 3,863 native, 194 rendered and
672 desktop tests plus all 44 real-daemon checks. Hosted delivery remains pending. The native method has no renderer/agent/CLI caller: R151 alone refuses enrolled history; the R152 reader work below adds validated immutable inspection. It is not an enabled
private-consumption or publication feature, and the fixed user checkpoint is unchanged.


## Validated inspection after native enrollment

R152 restores saved versions/files, entry/text inspection, comparisons, saved reviews and accepted
main after complete native enrollment. Every read verifies the native registration, both fences,
original history anchor and bounded policy replay, then uses immutable journal/CAS handles. It
refuses incomplete/substituted evidence and later legacy approvals; historical accepted main stays
bound to its original reviewer evidence. Capture, review creation and approval remain fenced.
Focused native and accepted-main/Git regressions pass, including a deliberately removed proof check
that correctly fails its test. Full local verification passed: 3,867 native, 194 rendered and 672 desktop tests plus all 44 real-daemon checks. Hosted verification is pending. This does not expose
enrollment or private consumption to agents/users and does not change the fixed testing checkpoint.


Pre-merge reader audit found and corrected shared-helper admission in manual allocation, agent
input preparation and remote export. All four now retain the legacy enrollment fence while saved
inspection remains readable. The new test fails on the previous implementation and passes on the
correction without creating lanes or destination files. Full revalidation passed on `3902d7abde439f64545cb6d5064c1af82ab1793b`: 3,868 native, 194 rendered and 672 desktop tests plus all 44 real-daemon checks. Hosted verification of this correction is pending.


## Native decisions for saved inputs

R153 adds a native-host-only writer for exact saved-input rejection, replacement and explicit
revalidation in an enrolled registered root work. Decisions retain old accepted main and immutable
history. Stable requests recover exact outcomes; pending intent and native journal identity bind
interrupted appends. All 146 frame-prefix fault cases, sync failure, lost acknowledgement, foreign or
stale inputs and changed evidence are covered by focused tests. Removing synchronization fails its
regression. Full local verification passed: 3,874 native, 194 rendered and 672 desktop tests plus all 44 real-daemon checks. Hosted verification is pending. This is not exposed through agent/renderer/CLI
operations and is not yet downstream publication enforcement or a consumption grant. The fixed
checkpoint is unchanged.


## Native correlation of root and descendant work

R154 selects enrolled project roots and existing native child work independently of an agent run.
Exact catalog, source, history and allocation-container identities are retained through bounded
ancestry verification. Five native regressions cover restart, editor changes, substituted evidence,
foreign ownership and depth overflow; removing allocation correlation fails its regression. Full local verification passed: 3,879 native, 194 rendered and 672 desktop tests plus all 44
real-daemon checks. Hosted verification is pending. These immutable facts grant no access and retain no lock.
Native grant issuance, dependency-aware allocation, full closure and publication enforcement remain
required before exposing this through the application. The fixed checkpoint is unchanged.


## Native grants for exact private inputs

R155 adds trusted-native-host grant, revoke and regrant requests for exact saved source and existing
destination work. Requests retain native correlation across replay; replaced allocation evidence
cannot reuse a historical grant. Shared transaction recovery covers every grant-frame prefix and
failed synchronization/acknowledgement. Focused positive/refusal, schema and previous semantic-reader
checks pass. Full local verification passed: 3,887 native, 194 rendered and 672 desktop tests
plus all 44 real-daemon checks. Hosted verification remains pending. This does not enable a runtime
authorization control, content copying, consumption receipts or dependency-bearing publication.
Historical v1 grants stay historical; current consumers must require verified native bindings.
The fixed user checkpoint is unchanged.


## Eligibility decisions for child work

R156 lets the native host reject, replace and revalidate exact saved input in native child work
under its owning project authority. Five focused tests cover identity separation, foreign and stale
ancestry refusal, interrupted recovery, child-history preservation and root request compatibility.
Confusing the child with its project root fails the regression. Full local verification passed:
3,892 native, 194 rendered and 672 desktop tests plus all 44 real-daemon checks. Hosted checks remain
pending. No runtime control or downstream publication enforcement is exposed yet; those paths must
still validate the full dependency closure and current decisions. The checkpoint is unchanged.


## Current grant inspection

R157 admits a native read callback only under the exact current allowed grant and matching native
source/destination bindings. Custody remains held; saved source bytes stay read-only and fixed. Six
focused native tests and a legacy-grant refusal test pass; removing the current-generation check
fails the revocation regression. Full local verification passed: 3,899 native, 194 rendered and
672 desktop tests plus all 44 real-daemon checks. Hosted validation is pending. This is not a runtime
agent operation, materialization, consumption receipt or publication gate. Exact allocation/consumption
and full closure enforcement remain outstanding. The fixed user checkpoint is unchanged.

## Native private capture after enrollment

R154 through R157 are now merged into canonical Mesh through passing exact-head and post-merge
main checks.
R158, tracked by [issue #317](https://github.com/idosams/Mesh/issues/317), merged through
[PR #318](https://github.com/idosams/Mesh/pull/318) at `027c8d2debb06cda81fd5e32be2c28c3a10f4473`.
All seven exact-head hosted checks and post-merge main verification passed.
Its dedicated native capture path signs without custody, preserves the original source folder, and
records authenticated private progress after explicit enrollment. Exact-request recovery resumes
staged journal prefixes and preserves unknown work. Historical receipt recovery does not rewind
newer saves. Ten focused native tests and six checkpoint tests pass, including source replacement.
An initial full gate passed 3,909 native, 194 rendered and 672 desktop tests plus 44 daemon checks.
A refinement lets policy-only activity during signing coexist with unchanged private authoring;
stale actor history still refuses before writing. The refined full gate on `1d0c000` passed
3,910 native tests in 275.798s (5 slow, 18 skipped), 194 rendered and 672 desktop tests,
and all 44 real-daemon checks.
The fixed checkpoint does not include this native development API.

This does not enable automatic enrollment, dependency-aware allocation, consumption, publication or
runtime controls. Full inherited closure, retained roots and packaged/provider acceptance are still
required. The user checkpoint remains fixed at `5052009`.


## Native destination reservation before consumption

R159 ([issue #319](https://github.com/idosams/Mesh/issues/319)) merged through
[PR #320](https://github.com/idosams/Mesh/pull/320) at `227fc60d7ce4f5730d74055380567daf1ef0d2f2`
after its full local gate and all seven exact-head hosted checks passed. Native reservation creates an empty destination with its own enrolled
history in staging, outside the visible catalog. Exclusive native publication makes that exact store
visible only after its required writer fences are durable. The destination can receive an exact
owning-project grant without copying any source bytes or claiming consumption or readiness to run.

Fourteen focused reservation/ancestry tests passed. They cover exact retry, interrupted setup,
occupied destinations, preserved editor work, native grant/revoke binding, manual capture in a
reserved lane, nested reservation and refusal before exceeding the ancestry limit. Replacing the
exclusive rename with ordinary rename made the collision regression fail; exact source was restored.
Initial compilation and nested custody failures are retained with corrected passing evidence.

This is a native development API, not a user-facing fleet launch path. Complete inherited closure,
retained roots, materialization plus starting-operation/consumption receipts, dependency-aware
publication/import, runtime controls and packaged/provider/remote acceptance remain required.
The user checkpoint remains fixed at `5052009`.

R159 full verification on `7ef5088b885cf86224353b55d409487a9d604335` passed: 3,918 native tests in
271.700s (5 slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon checks.
Its interruption tests use native fault injection; power-loss, packaged and provider acceptance are
not established by this result. Reservation post-merge run `37209051599` passed.


## Complete input ancestry and retained roots in progress

R160 ([issue #321](https://github.com/idosams/Mesh/issues/321)) is local work in progress.
The first native operation reader verifies bounded authenticated payloads and requires journal
parent links, actor/session, sequence, epoch and logical-clock fields to match the signed statement.
Three focused tests pass, including multiple parents and real journal/CAS reads that preserve
corrupt or oversized evidence. Removing the parent comparison makes the refusal regression fail;
the exact source was restored. The reader is now connected to bounded native graph traversal, with typed owner-consumption facts,
complete work custody and exact history revalidation. Seventeen focused ancestry/policy/graph tests
pass (2.475s), including a real two-save history reopened after newer unsaved editor bytes. That
regression first failed because traversal omitted the workspace identity domain prefix; traversal now
uses the capture writer's existing identity function. No unused-code warnings remain in this focused
build. Retained-root computation is unfinished; no full gate or PR is claimed. No runtime admission changed.

The next implementation must resolve every native work, traverse all immutable operation and
consumed-input edges under complete custody, distinguish reserved identity from actual consumption,
and derive retained roots from the same verified graph. Legacy missing provenance, cycles, missing
inputs, conflicting bindings and overflow must refuse. Actual consumption, publication/import,
runtime controls and the remaining fleet acceptance requirements stay open.

Historical content verification is now connected to graph nodes: manifests come from the immutable
journal, their logical IDs are recomputed, and every chunk plus the reconstructed content digest is
checked with streaming reads. Per-manifest chunk references are bounded at 65,536 and the entire
inspection has a 1 GiB content-read budget; exceeding a bound refuses without a partial success.
Graph nodes record chunks qualified by their existing work/installation identity. Six focused
graph/ancestry tests pass in 0.493s, including corrupt and missing content from an earlier save that
is absent from the latest snapshot; corrupt evidence remains in place. An initial missing digest
trait import was corrected. This is content verification groundwork, not a complete retained-root
or collection implementation: policy and pending transaction objects, rebuild and cross-work
consumption coverage, full validation and PR delivery remain open.

R160 now also records selected-graph content and verified policy payloads by exact work, installation,
physical store and native correlation. Owner policy stays in the owner's store, and eligibility
changes do not rewrite immutable graph identity. An early aggregate content-reference budget prevents
loading further graph nodes after overflow. Eighteen focused tests passed (2.443s); the extended
native graph suite then passed four tests (1.95s), including real three-level reserved work, signed
captures, grants and test-only durable consumption replay fixtures. Those fixtures are not a
production consumption writer. Missing intermediate work and incomplete inherited-input receipts
refuse; repeated/reordered handles agree; rejection preserves content and adds its policy record.

The initial disk-index deletion check was vacuous because attachment readers reconstruct in-memory
indexes. A strengthened assertion exposed that mistaken fixture assumption. The corrected regression
plants invalid cached indexes in all three stores, confirms journal-derived facts ignore and preserve
those bytes, removes the actual planted files, and confirms identical replay without them. Removing
policy payload roots makes the native regression fail on the missing rejection record (0.51s); the
exact passing source was restored. This remains selected-graph retention evidence, not complete-store
collection authority or durable pins. Pending/completed transaction object retention and remaining
closure fault cases, full gate and PR delivery remain open.

R160 now has read-only exact native capture recovery-root inspection. Pending and completed request
sidecars bind the physical store, enrollment, journal and exact staged frame object. Known partial
frames are reconstructed only in memory; signed local operation facts and all journal/staged manifest
content are verified with the same immutable readers as graph inspection. Returned local CAS roots
include the staged journal object, authenticated operations, chunks and policy/authority payloads;
logical manifests and required sidecar/journal identities remain distinct. This does not recover
the request, acquire a durable pin, authorize publication, or supply a whole-project collector.

The combined capture/graph/ancestry run passed 18 tests in 15.532s. After connecting an initially
unwired aggregate signed-payload budget, all 11 capture tests passed without warnings in 15.10s.
The existing every-frame-byte recovery test now inspects roots before recovery and asserts the
partial journal is unchanged. Additional cases cover completed historical receipts after later saves,
wrong requests, corrupt staged frames preserved in place, and unchanged newer editor files. Omitting
the staged-frame root makes the regression fail (0.24s); the exact passing source was restored.
The initial missing filesystem-type qualification and unused-budget build logs are preserved.
Pending control transactions, complete graph/recovery-root composition, remaining fault cases,
full validation and PR delivery remain unfinished.

Pending native control retention now verifies an exact grant/eligibility request, policy prefix,
physical journal and staged payload without applying the control change or repairing the journal.
It returns only local authority/policy recovery objects and required sidecar identity; referenced
inputs in other work are not mislabeled as local CAS objects. Capture/control ambiguity, wrong
requests, changed journal/source identities, malformed intent, foreign suffix and corrupt payload
refuse with evidence preserved. Twelve focused grant/decision tests passed in 80.930s (two slow),
including retention inspection at all 146 frame prefixes for each control type. Full Mesh validation
is next, followed by a draft R160 PR so the unfinished increment is reviewable. Complete graph and
recovery-root composition and the remaining issue #321 fault cases still block merge readiness.

R160's current implementation `9a6f4b7a240775b81757d41ced49f641caacca6f` passed the full Mesh gate:
3,926 native tests (272.003s, five slow, 18 skipped), 194 rendered tests, 672 desktop tests and
44 real-daemon checks, plus repository/docs/license/storage/fmt/clippy checks. The first full run
stopped at a Clippy map-entry finding; it was fixed without suppressing the check. This validates
the current groundwork, not completion of issue #321 or packaged/provider acceptance. A draft PR
is being published against canonical main; remaining composition/fault coverage still blocks merge.

[Draft PR #322](https://github.com/idosams/Mesh/pull/322) publishes the validated R160 groundwork at
`1cbbab33934e7d6491b0518431b850e7fedcb72a`; its initial hosted run is `37211764661`. Five checks
have passed; Linux/macOS tests were still running at this observation. The PR remains draft.

Local refinement now composes selected completed capture receipts and their exact staged frame
objects into graph retention under the graph's complete native custody set. Receipt names, physical
journal prefix, request identity and current sidecar bytes must agree; directory enumeration and
aggregate receipt/prefix/frame verification are bounded. Complete pending control frames awaiting
acknowledgement also retain their verified sidecar. Unknown recovery state refuses without repair.
Torn journals still use the separate exact recovery inspectors; these facts do not permit collection.

Five focused graph tests pass (2.31s), covering receipt/frame corruption preservation, three-work
replay, byte-identical store substitution, conflicting handles, restored original identity and
pre-enrollment provenance refusal with saved work still readable. A regression first demonstrated
that two request receipts could claim one completed operation; explicit duplicate detection now
refuses and preserves that ambiguity. This refinement has not yet updated the published head or
passed a new full gate. Remaining recovery composition and #321 proof still block merge readiness.

R160 local refinement now composes fully journaled pending capture evidence with graph retention;
torn or unappended captures require the exact recovery inspector and cannot produce a complete
graph. Signed multi-parent operation fixtures verify shared-parent deduplication and missing-parent
refusal. An actual legacy allocation regression reproduced a copied lane incorrectly being reported
as dependency-free after enrollment. Graph inspection now refuses that copied origin until explicit
migration evidence exists, preserving its journal and files. Allocation identity for an empty native
reservation remains distinct from consumption. The five focused graph tests pass (2.67s); full
validation and publication of these refinements are pending. Draft PR #322 remains unmerged, with
seven successful checks on its older published head `1cbbab33934e7d6491b0518431b850e7fedcb72a`.

R160 composition implementation `30d658add820195b9d0a43ee2c9bc94ecbc4ffe4` passed the full `npm test`
gate: 3,927 native tests in 274.864s (five slow, 18 skipped), 194 rendered tests, 672 desktop tests,
44 real-daemon checks, and repository/docs/license/storage/format/lint checks. This completes local
validation of the graph/recovery composition and legacy-copy refusal refinement; publication and
hosted validation of this revision are next. The earlier draft CI is not evidence for this revision.
Native consumption copying, publication/import/review enforcement and the full fleet acceptance
scope remain open. The fixed user checkpoint is unchanged.

R160 [PR #322](https://github.com/idosams/Mesh/pull/322) merged normally at
`5afe7757150eb4f6c079768c9473b5da3cc8b14e` after all seven checks passed on head
`0a0f2b211d295189cce2670d4cbc03c0907e7a33` (run `37213698088`). Post-merge run `37214307093`
is still active; this is not yet a post-merge pass.

R161 [issue #323](https://github.com/idosams/Mesh/issues/323) tracks native consumed starting
versions, required older-writer fencing and exact interrupted-copy recovery. Local groundwork
separates native graph/grant selection from validation under an already-held complete custody set.
The ordinary public entry points still acquire their own guards and refuse nested acquisition.
Preparation conveys no permission: validation rechecks physical bindings and current grants,
including a grant revoked after preparation. A native composition fixture verifies graph and
immutable granted-content reads under one guard, and refuses source-only custody when destination
custody is missing. Five graph tests (2.75s) and seven grant tests (1.43s) pass. Full validation and
publication are pending. No consumption copying, starting-operation transaction or new persisted
fence is enabled by this groundwork; the entire #323 scope and fleet goal remain open.

R161 custody groundwork `a3a99f8e35e86d230aa578c53baa0d742f03bf44` passed full `npm test`:
3,928 native tests in 269.544s (five slow, 18 skipped), 194 rendered tests, 672 desktop tests,
44 real-daemon checks and all repository/docs/license/storage/format/lint gates. Publication as a
draft follows so the unfinished consumption increment remains reviewable. This does not complete
#323 or enable consumption. Main's post-R160 run still has macOS verification active; its other
six jobs have passed.

Draft [PR #324](https://github.com/idosams/Mesh/pull/324) publishes the R161 custody groundwork at
`81babaa25ea86064a7e40b831f41f3d5fafa7970`; its hosted run is `37215060435`. R160 post-merge main
run `37214307093` has now passed all seven checks on `5afe7757150eb4f6c079768c9473b5da3cc8b14e`.

Further local R161 proof uses an authenticated `InitializeWorkspace` operation in a real empty
native reservation. Its saved snapshot and qualified graph reopen identically twice, with no files
or directories invented in the destination. All five focused graph tests pass in 3.20s. An initial
fixture omitted native read custody and correctly failed; it was corrected without relaxing the
reader. This is signed native journal replay, not a delivered consumption transaction. Copying,
required persistent fencing, owner consumption commit and exact recovery remain unfinished. The
published draft head and fixed user checkpoint remain unchanged while hosted checks run.

Local R161 snapshot preparation now builds deterministic initial operations and prepared manifests
from exact granted immutable content, with an explicit workspace-root declaration, file/entry/total
budgets, and an output sink that refuses growth beyond the admitted file length. It does not write
destination files or history. Eight grant tests passed (1.51s); the new stream-bound test passed,
and the granted-snapshot test passed again through that sink (0.40s). Unsaved source edits are not
used and the owner journal remains unchanged.

This internal builder is not yet wired into production consumption/signing or recovery. Full
validation and publication of this local refinement remain pending; the earlier full gate covers
only the published custody groundwork. A further transaction requirement is explicit binding of
the copied snapshot's ignore rules: the empty reservation already bound its original capture policy,
so copied rule files cannot silently change that policy or break later captures. Resolve that in the
required consumption binding together with the pending-writer fence. The full #323 scope remains
open and the published draft head remains fixed while hosted checks run.

R161 signed preparation now connects complete graph/current-grant checks, exact empty native
reservation validation and bounded snapshot construction to authentication outside custody. Its
result is an immutable candidate, not saved work: no history, policy or destination files are
written. Revalidation reacquires the complete set and refreshes the graph, current grant, physical
binding, enrollment and empty destination. Native fixtures prove that signing can independently
acquire custody, unexpected destination edits remain preserved/refused, revoked candidates cannot
be reused, and an empty source prepares a signed candidate without creating a saved version or fake
files. Five graph tests pass in 4.47s; the initial compile check passed without warnings.

The candidate computes a prospective history binding from saved ignore-rule files without changing
the reservation's current binding. Persisting and verifying that transition belongs to the upcoming
required transaction fence; dedicated rule-binding fault coverage is still needed. There is no
commit API yet. Full validation of these local refinements is pending. All seven hosted checks on
the older published draft head `81babaa25ea86064a7e40b831f41f3d5fafa7970` passed (run `37215060435`);
that result must not be attributed to the newer local signed-preparation implementation.

Dedicated R161 ignore-rule coverage now verifies that the signed workspace identity uses the saved
rule file's prospective binding, excludes ignored saved input, rejects invalid signatures, and
leaves the current reservation marker and both journals byte-identical. Later unsaved rule edits do
not alter the candidate identity. The test passed in 0.77s. A negative mutation signing with the old
reservation configuration failed the exact workspace-identity assertion in 0.54s; the unmodified
source was restored byte-for-byte. Full validation of this refinement follows. This does not yet
persist the policy transition or enable copying; #323 remains open.

R161 signed-preparation implementation `b27eb7d7175989aee6ddd022f0ae01514994d1b9` passed full
`npm test`: 3,931 native tests in 271.702s (five slow, 18 skipped), 194 rendered tests, 672 desktop
tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates. Draft PR #324
will be updated with this verified refinement; new hosted validation remains required. It remains
draft because persistent fencing, materialization, owner/destination commit ordering and recovery
are unfinished. The fixed checkpoint and full fleet goal are unchanged.

R161 required consumption storage now adds destination start/completion subtypes 5 and 6 to the
existing required dependency envelope, with migration 4 preserving populated indexed rows. Migration
3 is unchanged. Commit `9415a315e164fa72057d0e6ce1beedafe4dfd8ee` passed 342 storage tests (one timing
benchmark skipped) and all-target daemon compilation. Reopening and raw journal reconstruction are
verified separately; the in-memory index is not implicitly loaded on open.

The local destination policy projection now binds a start to an exact request, owner enrollment,
destination installation, granted source, correlation bindings, original/prospective configuration
digests, complete-closure digest, signed starting operation and staged-object digest. Start is
allowed only as the first policy record after enrollment; completion must name that exact start
and its owner receipt. Unrelated policy advancement while pending, conflicting completion,
second start, malformed identities and duplicate request reuse refuse atomically. Exact replay
retains the same historical result and all direct references. Native admission still refuses both
pending and completed-looking records until cross-store consumption verification is implemented.
A native capture/control test confirms this refusal preserves editor bytes and history; removing
the admission check makes that test fail because capture incorrectly reaches preparation.

These are staged R161 foundations within draft PR #324, not a completed consumed version. Durable
materialization, independent owner-receipt verification, recovery, actual previous-writer proof,
full current-tree validation and merged delivery remain required. The fixed user checkpoint and
all graphical/provider/remote acceptance requirements are unchanged.

R161 required-record/provenance implementation `2a39f854ac14dbc503d5367a6410c50636261d8d`
passed full `npm test`: 3,937 native tests in 273.860s (five slow, 18 skipped), 194 rendered tests,
672 desktop tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates.
The earlier full-gate lint failure and native-gate negative mutation remain preserved. This gate
validates the currently refusing foundation; it does not prove consumption commit/recovery or
actual previous-binary compatibility. Draft PR #324 remains unmerged and issue #323 remains open.

R161 previous-native compatibility is now executable evidence. The version-independent fixture
`required_consumption_prefixes_fence_native_read_prepare_commit_and_control` passed on current
source (3.12s) and exact previous implementation `220f2da76d7e1e35c8b2801db0b8dedc63d1aec8`
(2.89s). The audit verified that the entire tracked previous tree differed only by the appended
fixture; production code and schema were the previous revision. It recognized neither new subtype
and refused all 145 nonempty prefixes of each valid-checksum frame. Native read, new preparation,
commit of an already signed candidate and control mutation all refused while preserving journal
and editor bytes. A control capture after restoring the fixture's own baseline still committed.
The current tree recognizes both frame types and passes the same refusal campaign.

All overwritten current source bytes were backed up and restored with SHA-256 checks; the branch,
HEAD, published PR and fixed user checkpoint were unchanged. The compatibility fixture, overlay
manifest, exact previous revision and logs are retained with R161 evidence. This proves the required
journal boundary in the real previous native implementation, including an already prepared writer;
it is not a packaged desktop upgrade test. The production transaction must still durably append
that boundary before its first materialized byte and recover interrupted copying/owner receipts.

R161 compatibility implementation `62031d610e854ac72ae04f2e1dd1b3a0f442de46` passed full
`npm test`: 3,938 native tests in 315.123s (five slow, 18 skipped), 194 rendered tests, 672 desktop
tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates. The previous
published head `8989b942eba30e07af8cc37022a41f778f47f387` also passed all seven hosted checks in
run `37217784394`. Compatibility proof does not close issue #323: durable staging/materialization,
owner consumption, cross-store admission and exact interrupted recovery remain unfinished. Draft
PR #324 remains unmerged; the full fleet goal and fixed user checkpoint are unchanged.

R161 now extends the existing retained-tree addition helper with explicit bounded staging and
canonical recovery receipts. Existing callers retain their 64-entry/64-MiB caps; the new internal
seam accepts admitted limits up to 100,001 entries including the staged root, 1 GiB total and
64 MiB per file. Entry ordering, duplicate/traversal paths and byte budgets are checked before
creating a private tree. A receipt binds source/recovery roots, exact target path, parent identity
and metadata, bounded tree evidence and original entry identities. It is evidence only: a consuming
native transaction must authenticate and durably retain it before authorizing installation.

Recovery reconstructs either the exact private stage or the exact already-installed tree. Retry
synchronizes both parents and preserves the same installed inode. Missing/both-present stages,
substitution, changed content, extra entries, different roots/path or parent policy refuse without
cleanup. All 46 related retained-replacement tests passed, including a 100-file tree and separate
processes: one installer exits immediately after rename, then two fresh recovery processes use only
the saved receipt and retain the same tree identity. Removing installed-tree retry handling makes
that process test fail; correct source was restored byte-for-byte.

This is a materialization/recovery primitive for the unfinished R161 transaction, not consumption
commit or runnable-lane admission. It is not yet connected to the required consumption journal
fence, source closure, signed start or owner receipt. Those cross-store steps and the full fleet
acceptance scope remain required; draft PR #324 and issue #323 remain open.

R161 bounded-tree implementation `d232d2697b251a255cac71047fc35c4217e1ea0d` passed full
`npm test`: 3,942 native tests in 274.126s (five slow, 18 skipped), 194 rendered tests, 672 desktop
tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates. Previous
published head `ba88b38049f21318638eccdfbf4b744c7cdb8ef6` passed all seven hosted checks in run
`37218506637`. These results validate the staging primitive and existing behavior; full consumed
start commit, journal-before-install ordering, owner receipt and cross-store recovery remain open.
Draft PR #324 is not merged, and the fixed test checkpoint is unchanged.

R161 now provides the corresponding exact retained-file recovery seam. Canonical receipts bind
source/recovery roots, target path, parent policy, original allocation identity (including its
creation-time discriminator), file metadata, byte length and content digest. Resume requires
independently retained saved bytes within the caller's admitted limit. Wrong bytes or receipts,
same-byte replacement files, missing/both-present entries, changed roots/path or later editor edits
refuse without overwriting or cleanup. Already-installed retries synchronize both parents and
recheck identity/content; an edit during synchronization cannot be acknowledged. Empty files work
with a zero-byte budget. Receipts remain evidence only, not consumption or write authorization.

All 50 retained-replacement tests passed. A file installer exits in a separate process immediately
after rename; two fresh recovery processes reconstruct from saved receipt/content and preserve the
same allocation identity and executable state. Disabling installed-file retry handling makes that
process test fail; the correct source was restored byte-for-byte. Existing creation/restoration
callers use the same exact recovery validation without widening their admission policy.

Tree and file primitives now cover both top-level entry types. They still need native integration
with the signed starting snapshot, staged transaction/source closure, required journal-before-install
ordering and owner consumption receipt. No consumed version or runnable lane is acknowledged by
these helper changes. Draft PR #324 and issue #323 remain open; the fixed checkpoint is unchanged.

R161 retained-file implementation `c62c293303d1554c9f25ba7e729cb3f934ba37ae` passed full
`npm test`: 3,946 native tests in 314.880s (five slow, 18 skipped), 194 rendered tests, 672 desktop
tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates. The first
full attempt found a redundant import; its failed log is retained alongside the corrected passing
run. Published predecessor `1a28f903f3f0015b6c8f9f6086b704b3c30b1330` passed all seven hosted checks
in run `37219440756`. These results cover the exact retained-file recovery seam and existing behavior,
not a completed cross-store consumption transaction. Draft PR #324 and issue #323 remain open;
the fixed user checkpoint remains unchanged.

R161 preparation and revalidation now reconstruct an exact materialization plan from the authenticated
initial operation and retained checkpoint objects. The verifier binds the prospective workspace,
genesis sequence/parents/epoch/time/base and independently derived resulting head. It accepts only
the complete initial-tree operation grammar: explicit empty root, newly created directories and
files, exact links and matching file versions. Missing parents, aliases, duplicate paths/objects,
extra operations and unrelated records refuse. The plan contains paths, manifest identities and
executable flags; it does not contain a second copy of file bodies.

Content verification checks each logical manifest, chunk layout, retained chunk digest, reconstructed
file digest, per-file and whole-tree byte limits, and exact referenced-object coverage. It streams
hashing over retained chunks. Reconstructed paths have a 64-MiB aggregate bound checked before each
path allocation; retained objects have the admitted content budget plus the existing 16-MiB signed
payload ceiling. Bounds refuse explicitly without truncating the tree or writing destination files.

Eight focused tests passed, including real signed nonempty/empty sources, preserved saved ignore
rules, tampered/missing staged content, invalid bounds, nested executable plans and conflicting
paths/versions. Valid signatures over a false genesis base or resulting head are refused. Removing
the resulting-head check makes the signed refusal test fail; the exact source was restored. Full
repository validation is pending. This connects signed preparation to a verifiable materialization
plan; durable staging, journal-before-install ordering, owner consumption and cross-store recovery
remain unfinished in draft PR #324 / issue #323. The fixed user checkpoint remains unchanged.

R161 signed-plan integration `11854f4c7374da54a7c95a3e431680c3e1416d1e` passed full `npm test`:
3,948 native tests in 274.899s (five slow, 18 skipped), 194 rendered tests, 672 desktop tests,
44 real-daemon checks and all repository/docs/license/storage/format/lint gates. The exact negative
head-check mutation and successful focused/full logs are retained. Published predecessor
`59eeee06b6334372f675e413f4e29cd26fa1d630` passed all seven hosted checks in run `37220272359`.
This validates signed-plan reconstruction and existing behavior; the durable consumption transaction
and complete fleet acceptance remain unfinished. Draft PR #324 is unmerged; the checkpoint is fixed.

R161 now connects prepared signed versions to durable private staging through
`PreparedNativeConsumedStart::stage`. Staging uses the native reservation allocation on the same
filesystem as its unchanged destination root. A private attempt records request, operation, physical
identities, grant and closure before content construction. It retains signed objects, exact operation
frames, original/prospective configuration, source graph/retention evidence and recovery receipts.
Only a complete synchronized bundle is published into the stable request slot by exclusive rename.
Interrupted scratch attempts remain preserved; retries never overwrite or adopt their incomplete
contents. No destination file or journal is changed, and no lane becomes runnable.

Retry independently checks the bundle, signed objects, frames, exact root/entry identities and
source basis. Staged directory paths, kinds, content digests, lengths and executable state are also
compared with the authenticated materialization plan; matching rewritten local receipts cannot
substitute different content. Lookups are built once per verification pass, and subtree selection
uses ordered ranges instead of rescanning every entry for every directory. An exact retry returns
the same staging receipt and allocation. Empty snapshots create no placeholder destination content.

Integrated tests passed for signed nonempty and empty sources, nested executable/empty directories,
three interrupted construction boundaries, damaged frames, rewritten tree receipts and untouched
destination/history. A publisher process exits immediately after bundle publication; two fresh
processes reopen native handles and recover the same receipt and physical bundle. Bypassing the
signed-tree comparison makes the forged-receipt test fail; original source was restored exactly.
Focused process tests passed in 6.392s. The full repository gate is pending.

This is the private staging portion of the unfinished consumption transaction. A staging receipt
is evidence, not continuing permission or a consumption acknowledgement. Required start-record
synchronization before installation, owner consumption, completion, cross-store admission and the
complete interruption/revocation campaign remain required. Private-stage retries currently compare
the exact retained-source snapshot; transaction recovery must additionally handle intervening owner
history and committed outcomes without rewriting that evidence. Draft PR #324 remains unmerged,
issue #323 remains open, and the fixed checkpoint remains unchanged.

R161 private-staging implementation `a89419f3bc3bc7b7d5203124c5f25e8cfe40cdb3` passed full
`npm test`: 3,948 native tests in 277.987s (five slow, 18 skipped), 194 rendered tests, 672 desktop
tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates. The existing
native integration tests now additionally cover private staging and separate-process restart.
Published predecessor `4d176d29087916f7499849c535d27df024fa13f3` passed all seven hosted checks in
run `37221161431`. Successful and negative-mutation logs and complete Git bundles are preserved.
This validates private staging, not destination installation or consumed runtime admission. Required
start ordering, owner receipt, completion and cross-store recovery remain open in draft PR #324 /
issue #323; the full fleet objective and fixed checkpoint are unchanged.

R161 private staging now holds explicit native custody for the destination history, destination
files and reservation allocation during retained-entry preparation, verification and private bundle
publication. The retained-entry seam requires all three roots in the still-held guard; incomplete
custody refuses before creating entries. The private barrier is released before full graph/grant
revalidation, so it cannot extend a held set or accidentally retain permission for later installation.
The final commit still requires its own complete cross-store barrier.

Three focused integration tests passed in 6.719s. At each of three interrupted construction
boundaries, another native writer remains blocked until staging releases custody, then proceeds.
Incomplete root sets refuse without changing allocation entries. A real native grant revocation
after custody release prevents acknowledgement and preserves the staged bundle; a late editor file
also prevents acknowledgement and stays untouched. Separate-process publication/recovery and empty
source coverage continue passing. Taking custody on the wrong workspace makes the concurrency test
fail, and removing final revalidation makes the revoked-request test fail. Both negative logs are
retained, and exact production source was restored before the full gate.

Full repository validation is pending. Required start ordering, owner consumption, destination
completion and cross-store read/capture admission remain unfinished in draft PR #324 / issue #323.
These staging safeguards do not install files, launch agents or advance protected main. The fixed
checkpoint remains unchanged.

R161 custody implementation `6cf11cc0e474d07cafd2d498902180c31d89191f` passed full
`npm test`: 3,949 native tests in 272.512s (five slow, 18 skipped), 194 rendered tests,
672 desktop tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates.
Published predecessor `687ab1c9ed970f11635086e2b37409523745d30b` passed all seven hosted
checks in run `37222451265`. Focused concurrency/revocation and negative-mutation evidence is
preserved with the full log and Git history. This proves private staging custody and refreshed
permission checks; required start ordering, installation, owner consumption, completion and
cross-store recovery/admission remain unfinished. Draft PR #324 is not merged; issue #323 and
the full fleet objective remain open. The user checkpoint stays fixed.

R161 now retains the complete selected owner/source/destination custody set through a synchronous
validated transaction callback. Destination ancestry and all of its roots are selected before the
single lock acquisition. Signing still runs outside custody; public revalidation remains a
point-in-time read. The private callback is a transaction integration seam, not a committed version.

Both signed-source integration tests passed in 4.996s. They require catalog, owner, source,
destination and allocation roots inside the callback, block a competing native writer until an
intentional callback failure releases custody, preserve unexpected editor work without invoking
the callback, and refuse callback admission after native grant revocation. Removing both destination
emptiness checks makes the new regression fail (1.116s); exact source was restored. Full repository
validation is pending. Required journal-before-install ordering, durable owner/completion receipts,
recovery and cross-store admission remain unfinished; PR #324 stays draft and the checkpoint fixed.

R161 transaction-custody implementation `55fe74f5528355c45bb9f6e0b4797b7bb14fbdfb` passed
full `npm test`: 3,949 native tests in 316.666s (five slow, 18 skipped), 194 rendered tests,
672 desktop tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates.
Published predecessor `a3ff44a5e95f733a7c5bd391ee98fa5a8f781616` passed all seven hosted checks
in run `37223540971`. Positive and deliberately failing regression logs and complete Git history
are preserved. The validated callback retains complete custody but does not itself install files,
append consumption or acknowledge a runnable version. Draft PR #324 remains unmerged; issue #323
and the full fleet goal remain open. The fixed checkpoint is unchanged.

R161 recovery inspection now separates exact local native journal facts from workspace admission.
The shared parser still verifies registration, the enrollment fence, canonical policy payloads and
exact journal identity/bytes. Its non-admitting result can inspect pending or completed-looking
consumption policy; the private conversion to an ordinary read proof refuses either. No generic
ignore-tail flag, consumption permission or completed cross-store proof is introduced.

Six native integration tests passed in 1.426s, including exact local-policy inspection while ordinary
read/capture/control remain refused, changed bytes and torn-suffix refusal without repair, replaced
journals, missing fences and corrupt policy payloads. Removing the conversion's consumption refusal
makes the integration regression fail in 0.204s; exact source was restored. Full repository validation
is pending. Exact consumed-prefix recovery, required journal-before-install ordering, materialization,
owner/completion receipts and cross-store admission remain unfinished in draft PR #324 / issue #323.
The user checkpoint and full fleet objective are unchanged.

R161 native-facts implementation `971ed406a085a035d54d4ee259c195c6f58012a5` passed full
`npm test`: 3,949 native tests in 273.566s (five slow, 18 skipped), 194 rendered tests,
672 desktop tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates.
Six focused native tests and the deliberately failing admission-gate mutation are preserved with
complete Git history. Published predecessor `d521b18626bf4417eb9a4a5d0aeb4a0def8273d5` has
six successful hosted checks; macOS remains running in run `37224305162` at this evidence update.
The factual reader does not grant consumed workspace admission or recover a torn consumption
append yet. Required start ordering, materialization, owner/completion receipts and cross-store
recovery/admission remain unfinished; draft PR #324 and issue #323 stay open. The checkpoint is fixed.

R161 now inspects exact interrupted required-start frames through a distinct canonical consumption
intent. Recovery facts bind the original journal identity, prefix length/digest, request, payload and
destination configuration. The observed suffix must be a byte-for-byte prefix of that exact frame;
unknown bytes are neither skipped nor repaired. A pending start remains explicitly consumption even
when replay stops at the pre-start prefix, so it cannot mint an ordinary native read capability.

Six focused integration tests passed in 1.684s. They cover every start-frame prefix from zero bytes
through the full frame, changed suffixes, an otherwise canonical start for another configuration,
unchanged editor/journal bytes and continued ordinary read/capture/control refusal. Removing pending
consumption admission fencing makes the new regression fail in 0.210s; exact source was restored.
Full repository validation is pending. This is read-only prefix verification, not the durable start
writer, materialization, owner/completion transaction or cross-store admission. Those remain required
in draft PR #324 / issue #323, together with complete restart and fault proof. The checkpoint is fixed.

R161 start-prefix implementation `eef22fe60b0e2f05ff76be69b33a2854c21058fe` passed full
`npm test`: 3,949 native tests in 275.019s (five slow, 18 skipped), 194 rendered tests,
672 desktop tests, 44 real-daemon checks and all repository/docs/license/storage/format/lint gates.
Published predecessor `41923e7ae4f9d135abd262b4981815d48198fcaa` passed all seven hosted checks
in run `37225144094`; the preceding run `37224305162` also completed all seven successfully.
Focused prefix/refusal tests, deliberately failing admission mutation and complete Git history are
preserved. This validates read-only exact-start recovery facts, not a durable start writer or an
acknowledged consumption transaction. Installation, owner/completion receipts, full restart recovery
and cross-store admission remain required in draft PR #324 / issue #323. The checkpoint is unchanged.
