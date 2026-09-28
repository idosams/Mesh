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
The owner's 2026-09-27 instruction authorizes eventual merges after checks and required reviews
are satisfied. It does not waive human review, permit self-approval, or authorize bypassing checks.

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
desktop fleet scheduling, project-main integration, incomplete-allocation reconciliation and packaged
graphical verification remain required. See [the lane decision](../decisions/attachment-version-lanes.md).

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
correlation replay and preservation on retry/refusal. Desktop hosting/controls, real-provider proof
of this attached entry point, allocation reconciliation and project-main integration remain open.
