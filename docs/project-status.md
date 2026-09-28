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

## Empty worker workspace prerequisite

A dedicated protocol increment adds an explicit immutable root declaration for empty saved trees.
The corrected [draft #128](https://github.com/idosams/Mesh/pull/128) uses the declaration
for empty received-worker initialization; its exact-head validation is pending. Delivery is tracked in the
[delivery ledger](plan/fleet-migration.md#r12-prerequisite-explicit-workspace-root-declaration).
