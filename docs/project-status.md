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
