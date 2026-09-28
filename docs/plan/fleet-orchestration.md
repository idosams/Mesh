# Everyday work management and fleet orchestration plan

Status: accepted full objective; canonical delivery is being reconciled. This document specifies
requirements, not shipped capability. Issues and PRs in idosams/Mesh own active work.

## Outcome and baseline

Mesh is the underlying management layer for a user's everyday work, with or without agents.
The user gets a familiar, approachable Git-like experience: independent lines of work, saved
versions, comparisons, review, integration and recovery. These operations must remain usable
without configuring a provider, creating an agent session or launching a run.

The default entry point is an existing project in its existing location. Mesh attaches underneath
the user's current workflow and provisions file history, work identities and correlations in the
background. Existing editors, harnesses, terminals, Git workflows and running sessions continue
using the same folder. Moving or copying the project into a Mesh-managed working folder, relaunching
the harness, or transferring exclusive custody is not a prerequisite for attachment or observation.
Mesh is the underlying file and correlation system; provider execution is an optional integration.

Agents participate in that same work model. Mesh can accept an objective, let agents create private
working versions and delegate lanes, run workers within limits, show their activity live, and
support concurrent reviews pinned to immutable versions. Fleet orchestration extends everyday work
management. Measure time to accepted, verified output and human coordination time for manual,
assisted and delegated work separately.

Implementation proceeds from canonical `idosams/Mesh` main. Older fleet work is preserved and
transferred through the [migration ledger](fleet-migration.md), with fresh checks on this history.
No phase is complete merely because a source branch implemented part of it.

Completion requires every accepted increment to be merged into `idosams/Mesh` main, with the
combined main revision passing the actual repository checks and the acceptance journeys required
by this plan. Published PRs and passing branch tests are intermediate states. Record exact tested
and merged revisions, hosted CI results, packaged evidence where required, and unresolved issues.
The owner explicitly authorized this effort to merge without human review. Required checks remain
mandatory; this authorization permits neither self-approval nor bypassing checks. This repository
delivery authorization does not replace the product's native human approval for advancing a user's
protected project main version.

## Product model

- Objective: desired outcome; delegated execution additionally has human-authorized limits.
- Lane: persistent line of work with exact inputs and private workspace identity, usable by a human
  or an authorized agent. A provider, parent agent or active run is not required for manual work.
- Agent: authenticated identity and provider capabilities.
- Run: one execution attempt. Failure does not destroy the lane or its saved work.
- Version: immutable saved content with exact identity and provenance.
- Main version: the project's accepted shared state. Private versions and worker completion never
  advance it implicitly; an exact reviewed result advances it through the native approval path.
- Review: immutable version, base, dependency closure and validation evidence.
- Attachment: native-authorized observation of an existing project at its current location, without
  claiming exclusive write ownership or redirecting existing tools.
- Correlation: evidence linking project, file identity, observed changes, saved versions, existing
  worktrees or lanes, and optional harness sessions/runs. Unknown authorship remains unknown;
  filesystem timing alone cannot establish which person or agent made a change.

The primary navigation follows projects, lines of work, versions and reviews. Agent activity is
visible in that context, with a fleet view for coordinating many agents. The user can work directly,
assign an agent to a line of work, or ask a coordinator to decompose an objective. All use the same
version and review foundation. Actor changes preserve history and obey native custody rules.
Process status is distinct from work/review status: work survives a stopped agent, and an agent can
continue running while an earlier result is reviewed.

The current fleet runtime's provider-bound lanes are execution records, not a requirement that all
project work have a provider. Integrate them with the existing workspace/version foundation; do not
route manual saves, comparisons or reviews through fleet credentials. Git-like describes the user
experience, not a claim of Git storage or unrestricted Git compatibility.

Harness-led work is a core entry path: the user can spend the entire working session in their
preferred harness, then use Mesh to inspect versions, compare parallel lines, review results and
manage the main version. Starting a Mesh-managed fleet or using a Mesh conversation UI is not a
prerequisite. External harness activity retains native identity, capture and exact-review boundaries,
with non-exclusive observation for attached projects; arbitrary external edits do not count as
approval. Mesh-led orchestration is an
additional entry path into the same work history, not a separate version system.

## Required end-to-end behavior

The baseline journey requires no agent: attach Mesh to a project already open in ordinary tools,
continue editing in the same location, and inspect provisioned history in Mesh. Then save, compare,
review, integrate an exact result and recover an earlier version. Creating an additional line of
work is optional. Multiple existing or newly provisioned manual and mixed human/agent lines are
reviewable in parallel. Agent setup must not block that journey.

The harness-led acceptance journey attaches to a project with an already-running external harness,
creates and saves versions through its integration, then opens Mesh to compare and
review them and advance the main version through human approval. Verify this without launching the
agent from Mesh. The user's selected workspace and open pinned reviews remain stable during work.

For delegated work:

1. Open a project, enter an objective, configure limits once, and start.
2. A coordinator creates child lanes using authenticated tools, not human folder handoffs.
3. Mesh allocates independent native working folders from authorized immutable versions.
4. Independent workers run concurrently; dependencies and limits govern dispatch.
5. Ordinary progress is saved automatically; an explicit checkpoint provides a handoff boundary.
6. Every lane exposes truthful activity, observation age, files, versions, blockers and controls.
7. Several pinned reviews remain usable while agents continue editing newer versions.
8. Change requests reach the originating lane. Exact human approval advances Mesh main atomically.
   Applying accepted results to the attached folder is an explicit, non-atomic group with retained
   recovery and visible partial outcomes.
9. A restart or worker failure preserves acknowledged work and avoids duplicate execution.
10. Remote workers eventually support the same identity, recovery and review contract.

## Phase 0: contracts and baseline

Adopt this plan, the runtime persistence decision, and separate agent/run/lane/version identity.
Keep native authority and exact human approval. Update charter/PRD as capabilities land; distinguish
execution words from the six existing work-state words. Product runtime records are not a second
repository task tracker. Establish a baseline for four local workers before performance claims.

Exit: reviewed contracts, reproducible source baseline, persistence compatibility tests and a
requirement-to-evidence map that keeps incomplete work explicit.

## Phase 1: first complete local loop

Implement existing-project attachment before extending the provider-led launch experience. This
is a required foundation of Phase 1, not an alternative acceptance path that a fleet demo can replace.

- Register the exact existing root natively and keep Mesh metadata outside project content by
  default. Do not alter Git history, index, branch, remotes, hooks or harness configuration as an
  attachment side effect. A user-requested integration may configure its explicitly selected scope.
- Establish a bounded initial inventory and reconcile incremental filesystem observations with
  rescans after gaps or restart. Show catch-up, incomplete and unavailable states honestly.
- Capture immutable bytes and evidence without moving, locking or rewriting the user's working
  files. Detect concurrent changes and report an incomplete capture rather than claiming an atomic
  project snapshot that was not established. A saved version is never the live folder itself.
- Correlate existing worktrees and authenticated harness events when available. Observation works
  without an integration; stronger agent attribution requires actual session evidence. Existing
  Git HEAD and Mesh's reviewed main version remain explicitly distinct identities.
- Keep accepted-main updates and restore separate from observation. Writing back requires an
  explicit operation, an exact reviewed input where applicable and a fresh divergence check;
  preserve changes made by existing tools while the review was open.
- Detaching Mesh, stopping its service or a failed provisioning attempt leaves the user's ordinary
  workflow usable. Background operation must be implemented and measured, not inferred from the
  current desktop polling behavior.

Add an explicit native attachment mode rather than weakening exclusive custody guards for existing
Mesh-managed lanes. The current copy-on-import behavior remains a separate operation and does not
satisfy attachment. New isolated folders are provisioned when additional parallel work needs them;
the user's original project does not have to move into one.

Attachment exit: with an editor and harness already running on a dirty Git project, attach Mesh,
continue ordinary work, save and review versions, restart Mesh, and detach it without changing the
project location, Git state or tool session. Prove that concurrent edits are preserved, missed events
reconcile, unknown attribution is not invented, and open immutable reviews remain stable. Verify
the same journey with no agent or harness integration installed.

Expose agent tools for context, child creation, observation, checkpoint and review submission.
The daemon allocates paths; agents supply authorized version identities, never arbitrary output
paths. Bind every call to objective, lane, actor/session and generation. Retrying an identical
request returns the original result; reusing a request key with different content fails.

Use one independent workspace service context per lane. The existing desktop-selected workspace
must not become fleet routing authority. Reuse the native version fork and custody operations.
A new child must not switch the user's selected workspace or invalidate another agent's context.

Implement one real provider adapter first. Start, observe and stop through explicit provider
capabilities. Do not claim process suspension, resumption or token accounting if unsupported.
Save launch intent before spawning; reconcile uncertain launch outcomes rather than blindly retry.

Provide a work-centered overview and pinned review path in this phase, with agent activity and fleet
controls available when relevant. Verify the manual journey without provider configuration as well
as the delegated loop; retain work and review access after an agent stops or its credentials expire.

Integrate capture through a closed native operation bound to the session's workspace and custody
generation. A checkpoint saves the agent's observed private content without temporarily releasing
custody, switching desktop selection or exposing general mutation authority. Persist attribution
using the native actor's signing identity; display labels alone are not signed provenance. Reuse
existing file settling, unsupported-entry refusal and durable journal guarantees. A failed or
partial capture cannot be presented as a complete saved workspace. Review submission pins a saved
version and gains no approval or main-version advancement authority.

Exit: one coordinator creates two workers, each edits its own folder, saves a result and presents it
in Mesh without manual folder handoffs. Run with the actual provider and packaged app; fixture
providers prove contracts but cannot satisfy the user journey alone. Also demonstrate the complete
manual baseline without an agent, and a custody-safe human/agent handoff that preserves history.

Native retained-file restoration now prepares a frozen content proposal, preserves the current
file in a new transaction, and supports explicit undo without replaying an old exchange. Restored
private bytes cannot advance main. Confirmation UI, offline recovery, grouped changes and packaged
acceptance remain required.

Desktop source now connects bounded recovery inspection, exact lookup and explicit single-file
apply/restore through complete native text confirmation. Native-owned recovery allocation and
session generation checks preserve the authority boundary. Binary or large content, cross-volume
defaults, offline recovery and grouped changes remain unsupported by this increment; actual
packaged confirmation and recovery proof are still required.

Attached-version lanes now connect saved project history to additional independent ordinary folders.
Native allocation records source project/version ancestry and supports completed-request retry without
overwriting subsequent work. Desktop source shows these manual/harness lines with independent capture,
versions and comparison controls, while the original project/session stays in place. No provider is
required and no authorship is inferred. This is source-level progress on the work-centered overview;
graphical fleet controls, project-main integration, incomplete-allocation reconciliation and packaged
graphical verification remain required. Native desktop scheduling is connected below. See [the lane decision](../decisions/attachment-version-lanes.md).

## Phase 2: durable orchestration

Persist objective limits, lanes, run attempts, input versions, event cursors, pending commands and
results. Enforce concurrency, maximum lanes, delegation depth, bounded retries and cancellation.
Dependents await exact required inputs. Provider disconnect is not completion. An observation
timeout alone never triggers a replacement run.

Persist an immutable ordered runtime event ledger separately from disposable workspace indexes.
Use the existing SQLite storage dependency. A committed record is acknowledged only after the
transaction completes. Replay reconstructs scheduler state. Idempotency lookup and append occur in
one transaction; a stale revision refuses rather than overwriting another daemon's decision.

Exit: crash/restart and concurrent commands preserve state; no duplicate launch from request retry;
children cannot exceed inherited authority; cancel prevents further dispatch. Unknown live process
ownership requires reconciliation before a retry.

## Phase 3: live fleet and parallel review

A project overview includes manual and agent-assisted lines of work. Execution fields appear only
where relevant; a manual line is not shown as an idle or failed agent. It lists lane goal, parent,
provider, execution state, observed-at time, latest
saved version, changed files, validation, dependencies and available actions. Pin multiple lanes
beside one another. Stream bounded incremental events with cursor replay and resync on gaps.
Avoid scanning every workspace after every output line. Label lost connections and stale data.

Reviews pin immutable content. New changes show as a newer version without replacing open review
content or selection. Keep independent navigation for every review. Show overlap and dependencies,
route requests for changes, and preview selected results together. Revalidate at publication if
shared state changes; changed proposed content needs fresh review. Publication remains atomic.

Exit: two reviews stay stable while a third worker writes; stale approval is rejected; reconnect
restores the current fleet; responsiveness is measured at four concurrent workers. The same pinned
review behavior works for manual lines and mixed human/agent work without starting a run.

## Phase 4: private dependencies

Explicitly authorized lanes may consume another lane's pinned saved private version. This is an
amendment to automatic isolation, not permission to observe a moving writable folder. Record exact
input closure. Rejection or replacement marks dependent results stale and schedules revalidation.
Include the full dependency closure in combined review so rejected upstream work cannot slip into
shared state. Preserve versions used by active lanes or reviews during retention.

Exit: downstream work starts before upstream human publication, but a rejected upstream result
cannot be published indirectly through the downstream lane.

## Phase 5: local fleet hardening

Add a second real provider and run the same adapter conformance tests. Measure startup, event lag,
resource use, review responsiveness, time to accepted result and human coordination time. Initial
hypotheses: zero manual handoffs, 50% less human coordination, 25% less time to acceptance, p95 event
visibility under two seconds for four workers, without increased defects. Record cost and provider
versions alongside results. These are targets, not measured claims.

Run fault campaigns for crash during allocation/launch/save, duplicate requests, revoked authority,
concurrent approvals, replaced directories, cancellation races, retention and storage exhaustion.
Run all repository checks and a revision-bound packaged-app proof. Do not install over the user's
app, publish, merge or tag without authorization.

Exit: actual packaged journey launches, observes, revises, recovers, reviews and integrates the
intended output. Signed human approval requires a supported signing environment; do not replace it
with a weaker mechanism for the sake of the proof.

## Phase 6: remote workers

Extend the provider/worker boundary to authenticated remote executors. Persist assignment leases,
transfer exact immutable inputs and outputs with integrity checks, reconnect and reconcile uncertain
runs, and enforce the same authority and budgets. Never infer remote completion from disconnection.
Keep review local or authorized to its exact identity. No production hosting claim without deployed
and observed evidence.

Exit: a real second machine executes a lane, disconnects/reconnects, and yields a verifiable result
without duplicate execution or loss of acknowledged work. End-to-end human review remains exact.

## Ownership and delivery order

Native storage and daemon code own persistence, authorization, allocation, scheduling and recovery.
Provider adapters report their actual execution capabilities. MCP routes bounded typed operations.
The desktop native host owns workspace/review authority; React presents fleet and review state.

The migration ledger preserves implementation dependencies while the product priority remains
attachment beneath existing work, live versions and parallel review, then broader optional
orchestration. Open each coherent canonical PR before beginning the next substantial increment.

## Completion evidence

Each phase needs evidence at its stated scope. Unit tests cannot prove real provider execution;
source tests cannot prove a packaged graphical journey; one machine cannot prove remote recovery.
Record exact revision, commands/results, CI and merge status in PRs. Preserve all phase requirements
until a requirement-by-requirement audit proves their outcomes on canonical Mesh. The full objective
remains active while any requirement is missing, incomplete or unverified.

Attached main comparison now observes the accepted review base, accepted result and current source.
It classifies unchanged main paths as current work to retain, distinguishes already-present results
and conflicts, and blocks destructive directory previews with unobserved children. The bounded
read-only overview grants no write authority; retained integration and recovery remain required.

Native attached-file integration now prepares a single-use regular-file proposal from exact accepted
main and an unchanged approved base. Explicit trusted-native apply atomically exchanges names and
retains the displaced inode for late editor writes. Metadata copying preserves allocation identity.
No desktop, agent, MCP or CLI apply surface is added. Restart inspection, restoration, grouped
integration, actual native confirmation and packaged graphical acceptance remain required.

Read-only native restart inspection now binds recovery receipts to exact trusted history and current
file identities, distinguishes prepared/applied arrangements from changed or incomplete observations,
and retains explicit missing or contradictory outcomes. Bounded catalogues expose an overflow flag
and direct transaction lookup. No receipt grants replay, cleanup or source-write authority. Human
restoration, graphical recovery and killed-process campaigns remain required.

Native attachment-to-fleet bridge: exact saved attachment versions can now allocate managed root
lanes through `FleetService::create_root_from_attachment`. Source project correlation persists and
follows delegated children. Existing source folders and ordinary capture remain independent;
managed sessions still use exact custody grants. Native tests verify allocation, delegation,
correlation replay and preservation on retry/refusal. Native desktop hosting is connected below;
graphical controls, real-provider proof of this attached entry point, allocation reconciliation and
project-main integration remain open.


Packaged fleet execution dependency: desktop source now exposes an explicit scoped MCP mode and
`CodexAdapter::with_desktop_bridge` selects it without a development bridge installation. Exact-build
context and fail-closed session parsing have source coverage, with an opt-in packaged attached-input
journey. Native provider admission, durable discovery and app-owned scheduling are now connected
below. Process reconciliation and live lane/review presentation remain open.

Native durable discovery is now implemented on macOS: a retained catalogue lease, create-only
allocation receipts, guarded ledger reopen, and explicit `restored-unattached` state preserve fleet
facts without assuming worker ownership. Desktop native provisioning and catalogue commands are
connected; provisioning reports no worker started and keeps source capture running. Restart and
substitution tests cover the boundary. The native execution loop is connected below; live UI,
process/context reconciliation and packaged graphical verification remain open.


Native desktop execution is now connected: current-host-only catalogue admission, native installed
provider selection, exact-instance IPC routing, app-owned scheduling loops, explicit start/stop and
redacted activity snapshots. A fault suspends dispatch without replacing owned handles; cancelled and
restored objectives cannot silently relaunch. Native fixture-process tests cover lifecycle behavior.
Next: connect provisioning and activity to the real fleet presentation, add independent immutable lane
review readers, and prove the packaged graphical journey. Process-tree/context reconciliation remains
required; this increment does not complete the local fleet phase.


Initial fleet presentation is connected to the native commands in the attached-project view. Users
select an immutable input, set limits, provision, explicitly start agents, and observe all retained
lanes without changing desktop selection. Stop remains a request with retained uncertain ownership.
The coordinator validates bounded native schemas, retains uncertain provisioning input for same-session
retry, and never launches through polling. Restored fleets expose saved state without execution
controls. Result review and recovery are explicitly unavailable until their independent native readers
and context reconciliation are connected. Pending-intent persistence across renderer reload, parallel
fleet review and packaged graphical proof (previously blocked by the locked Mac) remain next work; manual attachment review
and its existing pins remain available.

Native saved fleet results are now independently readable through exact lane/checkpoint/version/bundle
selections. Reads retain existing lane context, recheck native identity and guarded control history,
and reconstruct immutable review/artifact content without consulting newer working bytes or changing
desktop selection. The catalogue-backed desktop list and review commands are connected. Pagination
and substitution/navigation/cancellation tests pass at the native seam; visible parallel panels,
artifact presentation, persistence of pin selections, restored-context reads and packaged graphical
verification are the next integration work. No approval or project-main integration is added here.

The saved-review reader preserves each bundle's recorded canonical base. An attached source version
is not automatically the managed lane's shared review base. Add explicit input-relative comparison
and original-project main mapping before claiming the complete fleet review/integration journey.


### Parallel saved-result presentation increment

Current-session fleet results now have independent bounded lists and up to eight exact pinned panels.
Verified text, incomplete-content markers, comparison-base identity and independent file/layout state
are connected without blocking fleet refresh. Source tests cover late replies after close, concurrent
loads, retained selection on failure and accessible identities. This increment does not satisfy the
full phase exit: pin persistence, input-relative comparison, artifact previews, original-main review
and integration, recovery and packaged graphical journeys remain required.


### Starting-version comparison presentation

The native source-to-local-import binding now feeds a separate comparison inside each pinned fleet
panel. Bounded object pages and selected saved before/after sides remain independent of recorded-review
reads and fleet polling. The existing text-diff viewer supports explicitly labeled saved versions and
independent layouts. Source tests verify exact identity/cursor handling and concurrent response order.
This closes the initial current-session presentation connection; persistent pins, restart reattachment,
artifact previews, changes requested from lanes and original-main integration remain phase requirements.

Renderer fleet pin persistence now saves and restores the exact review set and independent view choices.
Reopening rechecks native history and retains unavailable selections without starting workers. Regression
coverage includes closes during writes, lost acknowledgements, conflicts, late replies after replacing
the saved set, and failed initial loads. This advances parallel review continuity; full app restart
context recovery, artifacts, original-main integration and packaged graphical proof remain required.

Restart review prerequisite: new native allocations persist their verified source/local-import binding
as `bind-workspace-v2`. Existing command bytes remain valid and carry no inferred local starting point.
The history-only boundary below validates retained directory and installation identity, opens historical
content without recovering working files or checkpoint timers, and keeps restored lanes unavailable for
execution. Persistence of the binding alone does not satisfy the restart review exit criterion.


History-only restart access now reconstructs exact saved fleet reviews through native allocation and
installation checks without restoring execution contexts. The desktop admits result lists and pinned
reads for retained owners while start/stop remain unavailable. Pending work and durable lane files stay
unchanged during inspection, and missing history is not initialized. Native and renderer tests cover
these boundaries; packaged graphical restart proof, worker recovery and original-main integration remain
phase exit requirements. Legacy bindings expose recorded reviews but cannot guess a starting-version base.

Fleet saved artifact previews are now connected in desktop source. Each pinned recorded review can
request bounded native image, PDF-page and Office previews from authenticated historical bytes,
including restored fleet history. Responses bind objective, lane, checkpoint, review, object, side,
version and content digest; each panel owns independent transient request state. Saved selectors
contain no rendered content. Unsupported formats retain metadata, failures remain retryable, and
closed panels ignore late replies. Existing native rendering and presentation validation are shared
with workspace reviews. This grants no approval, export, execution or original-project integration
authority. Packaged graphical verification remains outstanding.

Review revision-loop increment: native-requested feedback is now durably bound to exact recorded
results and surfaced to the originating lane through authenticated context. Desktop source provides
record/read/exact-retry controls per pin. Feedback does not restart workers or assert receipt or
completion. Native replay, authority, bounded-message and renderer race checks cover the foundation;
provider wakeup/resume, addressed-result linkage, durable pending desktop requests and the actual
packaged change-request journey remain phase requirements.

Review result-link increment: a scoped agent can propose a different complete recorded checkpoint for
an exact change request. Proposals retain authenticated origin and exact saved identities, survive
replay and retry, and do not replace the original review or imply resolution. Desktop source can pin
these proposed results beside the request's original saved result. The existing standalone/packaged
bridge journey now includes feedback retrieval, checkpoint/review creation, proposal and retry. Actual
packaged graphical use, human resolution, provider wakeup and original-main integration remain required.

Request-decision increment: reviewers can explicitly mark a change request addressed by one exact
proposed result or reopen it through native confirmation. Per-request revision checks and durable
operation receipts prevent stale overwrites and duplicate effects, while historical reviews remain
fixed and agents retain no decision or publication authority. Native replay/race/identity checks and
desktop retry/current-state presentation cover this work-status foundation. Packaged graphical
confirmation, automatic worker wakeup, durable pending UI operations and original-main integration
remain required for the full review and orchestration journey.


### Original-project correspondence preparation

A native read-only boundary now verifies root and delegated result ancestry against the entire exact
source input at each import boundary. It returns paginated original-to-final object correspondence,
explicit saved ancestor versions and observed verified main. Original live edits and later captures
remain untouched; later parent work does not enter an already-created child. Native signed import
is described below. Private dependency closure/rejection propagation, desktop import/review
orchestration, exact approval and grouped integration remain required. Native tests also expose the existing missing-entry
checkpoint refusal: explicit agent deletion resolution must land before that capture journey can
be called complete. See the original-project correspondence decision in `docs/decisions/fleet-runtime.md`.


### Durable fleet candidate staging

Native preparation now retains exact result content and ancestry in a create-only external project
candidate with a fixed expected main. Exact retries and history-only receipt inspection preserve
source files/captures and do not adopt workers. Partial or altered candidates refuse without repair.
This staging boundary is part of the original-main loop. Native signed import is described below;
desktop import/review orchestration, dependency validation, human approval and grouped integration
remain required. Standalone offline candidate verification/discovery and partial-allocation reconciliation
are also outstanding. See the candidate staging decision for supported limits and evidence scope.


### Fixed-base candidate content review

A native reader now presents a staged result against its recorded original-project main, including
all inherited files. Its immutable review identity survives pagination, restart and later main changes;
staleness is reported separately. Verified historical approval membership is required for a non-genesis
base. This is a read-only content review, not a source-history operation bundle. Native imported
targets can enter explicit project review. Desktop import/review orchestration, artifact presentation,
dependency validation, exact human approval and integration remain phase requirements. Native fixture approval tests verify old-base stability without claiming OS user
presence or a packaged graphical journey.


Desktop candidate command boundary: the native host now exposes original-project mapping, exact
candidate preparation and fixed candidate comparison. Project selectors resolve only to registered
native history; the fleet service verifies source provenance. Completed requests can be recovered
after restart through read-only fleet history before current execution ownership is required for
new preparation. Changed inputs, unrelated projects and unavailable bases refuse; no command
imports operations, approves results, advances main, applies source files or adopts workers.
A native desktop test exercises real capture, signed agent checkpoint, review, staging, exact retry,
wrong-project refusal and restart recovery with later live edits preserved. Renderer controls,
durable pending request inputs, packaged invocation and graphical proof remain unfinished.


### Desktop fixed project comparisons and durable preparation inputs

Pinned fleet reviews now expose a whole-project content comparison against the main observed when
preparation begins. Each panel owns independent pagination, selected saved text, exact retry and
stale-main observations. The native desktop envelopes bind the request, project, objective and
saved selection to the daemon receipt or review. Presentation rejects substituted identities,
malformed lineage, changed immutable context, unsafe/oversized text and inconsistent pages.

Before dispatching preparation, the coordinator waits for native durable acknowledgement of the
project/request/expected-main selectors. A failed or uncertain save prevents allocation until the
same selectors are confirmed. Preparation retries retain those inputs; they never discover a newer
main implicitly. Restart only reads existing comparisons, and missing candidates remain visible
with an explicit preparation retry. Closed/reloaded panels ignore late replies. Closing a panel
removes its saved navigation selector; it does not cancel a dispatched native operation or delete
retained candidate content. Pending feedback and decision operations remain session-only.

Fleet pin storage and projections now write v2 with an optional three-field candidate selector.
Readers accept exact v1 records as candidate-free navigation without rewriting on load; a subsequent
changed save writes v2 atomically under the existing revision and directory identity checks. Older
binaries reject v2 rather than dropping pending inputs. No content or approval authority is stored
in selectors, and the input comparison/review mode fields remain unchanged. Existing attachment pin
formats are unchanged. Native migration/restart/refusal tests, coordinator persistence/race tests and
React static rendering cover this increment. Packaged graphical interaction remains unverified;
source-history operation import, signable candidate review, dependency validation, human approval
and original-project integration still require implementation.


### Historical authoring boundary for candidate import

`OpenWorkspace::prepare_historical_operations` now checks an explicit native operation proposal
against one saved predecessor and its causal closure. Its sole parent, derived base head and clock
come from that history, rather than the current union of journal tips. The current indexed policy
epoch still applies. A native actor whose latest recorded operation lies outside that closure is
refused, preventing sequence forks and accidental inclusion of unrelated work. Empty/oversized
plans, incomplete history, invalid operations and replaced workspace identity also refuse.

The opaque returned plan retains the exact operations and authoring context; its JSON is bounded
context metadata, not a signable review or append capability. Writers must reopen current journal
truth under custody and compare a freshly derived plan before signing/append. Ordinary managed
writes keep their existing all-tip behavior. The native regression uses actual signed journal
appends to prove that a later branch and a proposal based on an older version retain independent
saved trees (including explicit removal), while original ordinary files and protected main remain
unchanged. Reused actors outside ancestry refuse; a later policy epoch changes the plan while its
historical base and clock remain fixed.

This historical preparation API remains read-only. The native importer described below uses the
independent capture line, original-object correspondence and signed provenance receipts. Desktop
signing-identity provisioning and review orchestration, approval, dependency closure and guarded
original-folder application remain required. No desktop command or agent capability appends this plan.


### Independent observation history

Attachment saves now derive their predecessor, file identities and unchanged-content result from
an explicit native capture position, independently of other branches in the project journal.
Saved observation listings follow that exact ancestry. Signed regression fixtures prove that an
agent candidate can change a file and add a directory while subsequent ordinary captures retain
only the user's ongoing changes. Neither capture nor this separation advances protected main or
writes the original project files. Native import uses this separation; desktop import and review
orchestration remain unfinished.

The external `mesh.attachment-capture-line/v1` record binds the original history configuration,
last capture and optional exact pending signed operation. Native code persists intent before append.
After restart, a complete journal resolves a committed intent to its saved operation or clears an
absent operation without repeating it; incomplete journals or unexpected predecessors refuse.
Read-only inspection resolves valid pending state without rewriting metadata. A subsequent save
settles it. Atomic staging accepts only recognized transitions; malformed, linked, substituted or
conflicting records are preserved and refused.

On the next save, a legacy complete linear history migrates to `mesh.attachment-history/v2`, which
wraps the exact original configuration as `capture_basis`. This preserves workspace/version identity
and prevents older writers from silently resuming all-tip capture. Read-only legacy access does not
migrate. Missing position records in v2 refuse; branched legacy history cannot guess a capture tip.
Existing signed operations and approval records are unchanged. Native migration and recovery tests
cover identity preservation, lost acknowledgement, interrupted metadata staging, linked records and
corruption. This evidence is source-level; packaged graphical verification remains outstanding.


### Native candidate operation compiler

`FleetService::prepare_project_candidate_import` now verifies an existing staged candidate and
reopens its complete saved input ancestry. It compiles against the original source version using
native correspondence for every object, including unchanged entries; paginated renderer rows are
never used as authoring input. Original object identity and file-version ancestry survive moves and
edits. Proven replacements receive new identities, even at the same path. New identities and the
plan digest bind the complete candidate receipt, including its provenance. The current verified
main must match the candidate's recorded base when preparation begins.

The compiler unlinks displaced bindings before creating and linking the target tree, reads changed
file bytes from immutable lane history, and checks the resulting operations against the exact
historical predecessor. Candidate content is bounded by the staging limits. Ambiguous identity,
kind changes on retained objects, missing parents, inconsistent content, oversized input, an actor
outside the predecessor's ancestry and empty operation sets refuse. A no-op candidate does not
fabricate a saved version. Later user captures stay outside the proposal.

The opaque result exposes bounded context and a read-only historical operation plan. It does not
append, sign, create a review or authorize integration. The native writer described below rederives
it under project custody, verifies the exact candidate and expected main again, binds provenance
into the signed import and persists an exact retry receipt before appending. Signer provisioning must respect
independent actor sequences across candidate branches. Native signed-journal fixtures prove the
compiled result preserves moved originals, removes deleted entries, distinguishes replacements,
retains exact bytes/modes and leaves ordinary files/main untouched. A delegated-lineage service
test checks stable preparation and stale-input refusal. Runtime receipts and recovery are described
below; desktop import/review orchestration and packaged user proof remain outstanding.


### Durable signed candidate import

Native `FleetService::import_project_candidate` now appends an exact staged candidate as a private
original-project version. The host supplies a `CandidateImportSigner`, with separate capabilities
for the private ChangeSet and its provenance receipt. The import remains outside the observation
capture line, does not adopt workers, leaves original files untouched and cannot advance main.
A successful target can enter the existing explicit project-review API; import itself does not
open or approve a review.

Under the retained source-history lock, the writer validates the candidate allocation/content,
rederives the historical authoring context and requires the recorded main base. Legacy capture
metadata is separated before any candidate append. Signing is followed by fresh project, history,
capture-position, main, policy/context and retained-content verification. The create-only
`fleet-candidates/<candidate>/import.json` is flushed before the normal authenticated CAS/journal
append. Its `mesh.fleet-project-import-receipt/v1` envelope signs a
`mesh.fleet-project-import-statement/v1` using domain `mesh.v0.fleet-project-import`; the statement
binds the complete candidate/provenance, plan context/digest, source predecessor, actor, exact
operation and ChangeSet signature. Existing workspace operation formats do not change. Older
readers can ignore the extra receipt; the already-established v2 history wrapper prevents old
all-tip attachment writers from resuming.

Read-only inspection validates the private, single-link bounded receipt, native candidate identity,
provenance signature and complete journal. A present operation must have the exact authenticated
signature, predecessor and retained whole-project content. It reports `imported`; an absent operation
reports `pending` without signing or replay. An explicit import retry uses the retained signature
and operation only if current context and main still match. A completed retry recovers even after
main advances. Partial/corrupt/aliased receipts, partial journal tails and changed context refuse and
retain evidence. A receipt alone is neither append nor approval authority.

Native fault tests stop after durable intent and prove read-only pending inspection, exact retry
without signing, preserved user edits and later independent capture. Service tests cover delegated
import, provenance tampering, recovery with signing disabled, history-only restart, existing native
review creation, completed retry after verified main advancement and stale fresh-import refusal.
Desktop import identity, controls and fixed-review orchestration are described below. Packaged
import invocation, graphical/OS approval proof and the complete integration journey remain
outstanding. Independent candidate branches keep independent actor sequences.


### Desktop import and fixed project review

The desktop now offers explicit save-as-project-version and create-project-review actions per pinned
candidate. Its existing durable project/request/expected-main selectors are acknowledged before
mutation. Refresh and panel restoration only inspect retained status and reviews; they never stage,
sign, append, create a review or adopt a worker. Parallel panels keep independent pending requests,
errors and verified outcomes. A lost response retries the same selectors; a confirmed imported
version stays visible even when reading its review fails. Closed/replaced panels ignore late replies.

New native commands are `import_fleet_project_candidate` and
`review_imported_fleet_project_candidate`; `create: false` is inspection-only. They resolve registered
project and fleet history, with native trust, before proceeding. Retained fleet history can explicitly
author a source-project import using a supplied native signer without restoring execution ownership.
The `mesh.desktop-fleet-candidate-import/v1` and `mesh.desktop-fleet-import-review/v1` envelopes bind
all selectors; the renderer validates those bindings and never supplies a signer, private key or path.

Each first import gets a fresh in-memory `SoftwareActorCustody`. Only its signed receipt and public
identity persist. `NativeImportSigner::recorded` has no signing capability: pending retries reuse the
exact stored signatures, and completed retries read journal truth. Native public-identity discovery
returns only after candidate, receipt signature and journal outcome verification. The new private,
single-link `import-binding.json` (`mesh.fleet-project-import-binding/v1`) binds the receipt digest,
operation and public actor before append; a missing receipt beside that retained binding refuses
instead of minting a new identity. Earlier v1 receipts without this guard remain readable without
migration, and an explicit pending retry adds the guard. Partial or conflicting guards are preserved
and refused. These are retained metadata proofs, not a backup for loss of the entire candidate store.
No private-key export or new custody backend was introduced, and software actors still cannot approve.

An imported review is derived against the candidate's recorded base, restricted to genesis or native
verified main history. The native journal stores the exact review bundle; reopening recomputes that
same base rather than using the newest main. `mesh.fleet-project-import-review/v1` keeps the review,
import receipt and separate `base_is_current` observation together. A historical review can be read
or explicitly recorded after main advances, but stale main cannot be approved implicitly. If the
imported target does not descend from its recorded non-genesis base, native review creation refuses;
rebase/integration remains explicit future work. Import and review creation never advance main.

Native tests cover first import after restart, public-only identity recovery, missing/aliased receipts,
fixed review after verified main advancement and no worker adoption. Coordinator and static-render
tests cover explicit mutation, read-only restoration, exact lost-response retry, independent panels,
late replies, incomplete reviews and substituted identities. These transferred tests require execution on this canonical increment; packaged import invocation
and graphical approval are not established by source tests.

Explicit missing-file resolution now connects native deletion adoption to the fleet ledger and harness
tools. Intent and prepared operation are durable before append; exact retry can reconcile an appended
operation without signing again or touching later filesystem work. Resolution supplies no approval
and does not substitute for a fresh complete checkpoint. Directory deletion, host reconciliation after cancellation and packaged agent-deletion verification
remain required. No-change lane inspection is described below.

Deletion-only lane results use inspection-only saved reviews when they equal lane main. Native
reconstruction must prove exact equality before creating the separate inspection identity. The pinned
starting-version comparison retains actual deleted content; importing into the original project
creates a separate review against verified project main. Inspection identities never authorize
approval or publication. Current canonical runtime and packaged verification remain required.

Native accepted-review grouping now stages and preflights every regular-file replacement, preserves
already-present and unrelated source work, records each attempt and retains partial outcomes without
automatic rollback. Recovery inspection rederives full review membership and verifies each retained
member. Unsupported directory changes refuse the entire group before source writes.
This is the replacement executor foundation; the other entry executors, complete native desktop
confirmation, grouped recovery UI and packaged proof remain required.


Group preparation now reuses a verified project capture, preserving targeted file/metadata and ignore
policy checks at each member boundary. Transferred tests assert at most three full captures for preparation and
two for apply for both 2-file and 24-file groups; canonical execution remains pending. Large-project timing, the other entry executors and
the grouped desktop journey remain required.

Accepted-review groups now include regular-file removal with durable retained inodes and explicit
removal receipts. Source absence requires exact confined-parent evidence; read failures never count
as applied deletions. Transferred tests cover mixed groups, late editor writes, interrupted member
sequences, occupied recovery names and restart inspection without replay; canonical execution is pending. Single-file desktop
preparation remains replacement-only pending complete group confirmation. Addition/directory
executors, absent-path restoration, group recovery UI and packaged graphical proof remain required.

Native accepted-review groups now support regular-file additions as well as replacements/removals,
including the first approved main. Approved bytes stay in private staging until explicit native
apply, which never overwrites a concurrent file. New paths pass the captured exclusion policy at
every ancestor. Restart evidence distinguishes a prepared stage, an installed file and later edits
without replay. Directory creation/removal, absent-path restoration, grouped desktop confirmation
and recovery, and packaged graphical proof remain required.


New native addition preparations now inherit the macOS destination parent's group and file ACL,
remove staging-folder ACL inheritance and preserve the process umask. Permission policy is bound in
v2 receipts and rechecked before and after installation; changed policy requires refusal or
reconciliation. Read-only recovery retains v1 compatibility and exposes v2 policy mismatches.
Transferred tests compare inheritance with kernel-created files and cover restrictive umasks and
policy races; canonical execution remains pending. Linux default ACLs/extended attributes remain
unsupported. No grouped packaged graphical proof is claimed by this increment.

Native retained-file restoration now handles an absent leaf beneath an existing confined parent.
It restores a separate frozen copy while keeping the original retained inode available for late
editor writes, refuses concurrent destination creation and records that no current file was displaced.
Both removal and replacement origins retain verified ancestry and read-only restart evidence.
The single-file native confirmation describes creation explicitly. Directory operations, complete
group confirmation/recovery and packaged graphical proof remain required; earlier absent-path
restoration gaps above are superseded for regular files with existing parents.


Native desktop commands now support complete regular-file group confirmation, verified group/member
recovery inspection and explicit retained-member restoration. Every changed file must fit the native
preview; project/folder labels remain literal. The host rechecks the project generation after consent.
This is the native host increment only: renderer controls, current native execution and packaged
graphical proof remain pending. Canonical monitoring lifecycle behavior is preserved; the unresolved
local FSEvents registration failure is not resolved by these commands.

Group application/recovery controls now route exact native identities, retain uncertain outcomes and
permit explicit inspection without automatic retry. Existing per-file controls, saved reviews and
project imports remain available. Hebrew controls and literal file identifiers are preserved.
Directory operations, watcher reconciliation and the full packaged acceptance objective remain.


Grouped desktop source reconciliation is accounted for by canonical PRs #87–#88 and the watcher
acceptance/documentation increment. The newer canonical monitoring lifecycle is preserved; the
native-event test now establishes a fresh post-registration baseline and requires a new callback.
Historical source counts are not canonical validation. The local registration stall, current full
verification, directory support and packaged graphical fleet acceptance remain outstanding.

### Saved execution evidence follow-through

The native group inspector now projects bounded historical attempts and outcomes
after restart, independently of current file observations. Missing, inconsistent
or changing records cannot authorize a retry. Localized desktop presentation is
a separate increment; canonical runtime and packaged graphical acceptance remain
pending. See the migration ledger for source-path accounting.

The localized saved-execution view now distinguishes historical outcomes, absent
records and unreliable evidence after restart. It validates proposal/member
identity without replaying changes. This completes source transfer of that view,
not full fleet acceptance: directory integration, large/binary confirmation, real
providers, remote recovery and packaged graphical journeys remain required.

### Directory integration delivery sequence

The directory source increment is being transferred as confined directory creation,
retained subtree staging/metadata, native group execution/recovery, then localized
complete-tree confirmation and controls. Each increment has its own PR and source
accounting. The first primitive does not establish working directory integration
or packaged acceptance; those remain required outcomes of this sequence.

The native subtree executor and group v2 reader are now transferred on an unmerged
branch. New approved directory trees retain private staging, exact review coverage
and no-replace installation; uncertainty never triggers replay. Existing desktop
confirmation refuses these groups pending complete-tree presentation. Directory
removal/type replacement, large/binary confirmation and packaged graphical proof
remain unfinished; this native increment does not complete directory acceptance.

The localized directory recovery view now presents bounded source/stage trees and
parent identity/policy changes, retaining English/Hebrew and literal paths without
restoration authority for additions. Native directory source transfer is complete
across the staged PRs; native runtime, complete packaged confirmation/recovery,
directory removal/type replacement and full provider/remote acceptance remain
required. Published source increments do not establish merged delivery.

Native directory removal now retains the whole approved tree, including late writes
through open descriptors, and uses group v3 with complete removal confirmation.
This unmerged transfer still needs native verification and localized recovery
presentation. Whole-tree restoration, file/directory conversion, packaged graphical
acceptance and full fleet/provider/remote acceptance remain required.

Localized directory removal recovery now distinguishes working and retained trees,
including possible later descriptor writes, without exposing file restoration.
The removal source is fully accounted for across #98 and its UI increment, but
native runtime, whole-tree restoration, conversion and packaged graphical proof
remain required. Historical exchange experiments do not complete conversion.

The native conversion executor now transfers both file/directory orientations
using a preserving exchange and exact two-sided evidence. Group v4 expands both
sides for complete review coverage. Desktop confirmation still refuses conversion
receipts pending complete presentation; localized recovery, whole-entry restoration
and native/packaged acceptance remain required.
