# Fleet runtime identity and persistence

Status: accepted for phased implementation.

## Decision

Objective, lane, agent, run, version and review are distinct identities. A lane can have many run
attempts; restarting an attempt cannot replace its prior saved versions or open reviews. A provider
process is evidence about execution, not publication authority.

Store fleet commands/events in a separate namespaced SQLite database owned by mesh-store, using its
existing rusqlite dependency. This database is durable runtime truth, not the reconstructable
workspace index. Ordered immutable events reconstruct daemon lifecycle state. Records contain
bounded metadata, never credentials or file contents. Raw provider output belongs to a separately
controlled artifact store and must not enter telemetry or this control ledger.

Every accepted command names its objective stream, expected revision and idempotency key. Reusing a
key for an identical command returns its original committed record even when the stream advanced.
Reusing it for different content fails. Compare-and-swap and append run in one immediate transaction.
This prevents two schedulers from claiming the same transition. It does not by itself provide
exactly-once external process execution: dispatch intent and process reconciliation are separate
required steps.

Native provider launch commits a `claim-launch` event before starting an external process. The
claim binds the exact current run to a fresh host-instance identity and is accepted only once while
the run is launching. Neither the same host nor a restarted host may spawn again from that claim.
A crash between claim and process creation deliberately requires reconciliation; a claim is not
proof of process existence or termination. The additive event uses the existing canonical envelope;
older decoders refuse the unknown event. Legacy dispatch records replay with no launch owner.

The Unix Codex adapter accepts native-admitted executable locations, uses the lane's verified working
folder, and forwards scoped MCP credentials only through process environment. Prompts use stdin.
Activity retains bounded categories and a validated provider thread identifier, not raw messages,
commands, stderr or file contents. A successful outcome requires process exit zero, an explicit
completed turn, no protocol failure, and both output streams closed. That outcome grants no review
approval or custody release. Direct-child termination cannot establish descendant termination.
The native tick-driven host discovers allocated Codex lanes, dispatches their first attempts within
the durable concurrency limit, and polls its owned processes without changing desktop selection.
Per-host dispatch identities prevent competing hosts from sharing a run grant. The native host
supplies a signing capability for each session; neither renderer nor agent code supplies keys.
Terminal acknowledgment revokes that session and records execution state without releasing custody
or approving content. Cancellation is rechecked at acknowledgment under the service lock; direct
process termination leaves cancelled slots reserved. Failed, interrupted and unowned attempts are
never automatically relaunched. The desktop now embeds a native scheduling loop; durable process reconciliation
and process-tree cancellation remain separate work.

Use WAL with FULL synchronous durability. Version the database schema explicitly; refuse unknown
versions. Bounded reads and payloads keep a bad provider from turning fleet observation into an
unbounded allocation. Native service code must authorize and pin the private database location
before opening it; the storage constructor is not a path-authorization boundary.

## Compatibility

This adds a new database and does not modify workspace record encodings, approval statements or
existing indexes. Older clients cannot operate a fleet but retain their workspace behavior. Unknown
future database versions fail closed. Schema changes need migration tests and this decision updated.

## Authority

Existing-project attachment is a distinct authority mode. Native registration is implemented as
described in [the attachment decision](project-attachment.md); observation and capture are planned.
That mode observes and captures work
without exclusive custody, relocating the project or changing existing tool sessions. Native code
must bind the observed root and captured file identities, detect concurrent writes, reconcile event
gaps and preserve unsupported or ambiguous work. Keep attachment metadata outside project content by
default. Correlating filesystem events with a run is not authenticated authorship; absent evidence
must remain explicit. Existing exclusive custody and human approval guards remain unchanged until
the separate attachment operations and their contracts are implemented. Importing a copy or opening
an app-managed agent folder is not proof of this mode.

Agent tools carry scoped session identity. Agent-supplied lane IDs or paths never prove authority.
The daemon chooses private folders and validates workspace identity/generation. Every lane gets an
independent service context, so UI navigation cannot redirect an agent.

Explicit pinned private dependencies require a charter amendment and dependency-aware review before
exposure. Agents never acquire protected shared-state advancement authority. Ordinary folder
isolation must not be described as an OS process sandbox.

## Local agent session boundary

The native host issues random 256-bit bearer credentials for one objective/lane/run/actor/session.
Only a digest is retained in the in-memory grant registry. Raw credentials are passed to the MCP
bridge through native process configuration, never model tool arguments, command-line flags,
control events or Debug output. IPC session names remain correlation hints, not authorization.

Every call checks current run status and workspace custody generation. Accepting delegation holds
the exact native custody guard while committing the attributed command; subsequent folder creation
completes that accepted operation. Replay rechecks that the parent run was active at acceptance.
The new `delegate` command retains actor/session/run/generation attribution and no secret. It is an
additive event kind under the existing envelope; older decoders refuse it instead of ignoring it.

IPC surface 8 adds `fleet.agent.call` without changing older methods. Unscoped MCP retains its
read-only behavior. Scoped MCP refuses version downgrade before sending a credential. Tokens expire
on service restart; native process/custody reconciliation must precede future reauthorization.
Credential revocation alone does not assert process termination or release native custody.

Native agent file capture retains exact custody throughout signing, durable save and settling.
`checkpoint_agent_file` admits one inspected tracked or new regular file; it cannot approve,
publish, delete or rewrite working content. The native host retains signing keys outside the
daemon. Its signing callback receives the canonical payload with inherited mutation authority
suspended, restored on return or unwind. Nested capture is refused before acquiring custody again.
The result is a per-file receipt. The native `checkpoint_agent_workspace` operation retains custody
across a bounded inventory, parent-first directory adoption and private file saves. A final complete
inventory and settled recovery state are required before reporting completion. Missing or unsupported
entries require explicit resolution before any save, rather than guessing a rename or deletion.
Failure retains durable partial progress and reports an incomplete result. A fresh unchanged capture
does not append duplicate changes.

Scoped MCP capture requires a native signer whose public key is the session's actual actor identity.
The host commits `begin-checkpoint` with lane/run/actor/session/generation and the admitted index
fold before capture. `finish-checkpoint` stores the bounded immutable result, including incomplete
outcomes. These additive event kinds use the versioned control envelope; older decoders refuse them.
The index fold is a 128-bit drift detector, not a signature or content hash. Retained version IDs
remain 256-bit operation identities. Completed retries return the recorded outcome without capturing
later edits. Pending intents require native reconciliation; they are never blindly replayed. Session
changes cannot reuse an earlier session's request. Capture accepted before cancellation may still
record its result, preserving work without authorizing another run. Native restart reconciliation
still needs integration.

Review submission accepts only a completed checkpoint from the current authorized session. Native
code reconstructs its immutable closure, preserves pending recovery, records the exact review and
acknowledges its bundle in the control ledger. A repeated submission returns the same bundle. The
native record can be recovered across the gap before the control acknowledgment using the complete
review index, never the bounded UI projection. This saved-review entry point does not require newer
working bytes to equal the selected historical version. It does not weaken existing human approval
or publication guards. Approval/integration of pinned results while work continues and stable base
presentation across shared-head advances still require the planned parallel-review integration.

Allocation uses service-generated identities and descriptor-pinned parent creation. Both temporary
export and final import verify the admitted parent object before writing. An occupied reservation
or ambiguous allocation is preserved and reported for recovery, never deleted or silently reused.
This does not claim operating-system isolation from every other process owned by the same user.


## Attached project roots

The additive `create-attached-lane` command stores a canonical native project registration identity
alongside the existing lane goal, provider and exact source operation. Existing `create-lane` bytes
are unchanged and replay with no source project. Older runtimes refuse the new unknown command;
there is no silent downgrade to a provider-only lane. The current decoder rejects additional fields,
unknown event variants and noncanonical encodings. Root creation validates the project identity and
normal lane limits; children inherit the original project from their parent. Correlation does not
authenticate edits, convey permission to write the original project, or approve protected main.
The native entry point separately verifies actual retained attachment authority and immutable
history membership before committing creation. A replayed origin is metadata, not a new path grant.


## Packaged agent bridge

The desktop executable has a separate `--mesh-fleet-mcp --endpoint <absolute-local-socket>` mode,
handled before AppKit/Tauri starts. It requires both native fleet environment fields and refuses
missing, partial or malformed sessions. It never falls back to the selected workspace, accepts
credentials on the command line, or adds human-approval tools. The existing `--mesh-mcp` mode retains
its required workspace identity and read-only behavior.

`CodexAdapter::with_desktop_bridge` selects this fixed native argument prefix; the renderer cannot
provide generic command arguments. The original standalone bridge constructor remains compatible.
Both paths pass session credentials through the provider's private environment configuration.
Fleet context from the packaged bridge includes its embedded build revision and exact-build flag;
reserved-field collisions refuse rather than replacing native context. Other scoped results retain
their original identities and encodings.

The packaged bridge is an execution dependency for desktop fleet hosting. Its bridge test alone
does not prove the application's scheduling loop or graphical fleet controls. An opt-in native IPC test exercises the
actual packaged executable from attached saved input, delegation, signed capture, pinned review,
retry and revocation. It requires an exact expected build revision and verifies unchanged source
work and desktop selection. That test does not launch a model or prove graphical interaction.

## Native fleet discovery and storage ownership

On macOS, `NativeFleetDirectory` owns an existing private application directory through a retained
nonblocking directory lock. A second owner refuses while the catalogue or any returned service still
holds that lease. Native callers supply a retained attachment, exact saved version, bounded goal,
stable request and explicit limits. No renderer-selected database, allocation path, provider binary
or credential enters this interface. This first catalogue authorizes the existing Codex adapter only.

Each request has a deterministic objective directory containing a create-only ledger, an allocation
receipt, and an independent `lanes` directory. Canonical `mesh.native-fleet-allocation/v1` binds the
request, project/version, goal/limits, objective directory identity, lane-directory identity, and
ledger device/inode plus macOS birth time. Receipt discovery is bounded to sixteen objectives.
Unknown, incomplete or changed records are retained and reported unavailable, never deleted or
turned into new history. Existing fleet event bytes and SQLite schema remain unchanged. Older
runtimes do not discover this new directory layout.

`FleetStore::open_guarded` separates first initialization from reopening. Reopen refuses a missing
file or empty/unknown schema. SQLite opens without following symbolic aliases. Legacy callers first resolve OS parent aliases
such as macOS `/var` while leaving the final ledger entry subject to no-follow admission. The native guard
retains the admitted directories and ledger descriptor, checks canonical receipt equality, single-link
regular files, owner-private modes and database-family aliases around operations, and checks before
commit and acknowledgment. An authority change before append commit rolls the transaction back; a
change after commit produces an uncertain result requiring the same request for reconciliation.
The native directory reference anchors ledger access. Lane import uses an identity-checked ordinary
path because macOS cannot canonicalize newly created descendants through the directory reference.
These are native ownership checks, not an OS sandbox against arbitrary same-account code.

Discovery reconstructs saved facts even when the original project is offline. It marks reconstructed
services `restored-unattached`, preserves recorded uncertain runs, and neither adopts processes nor
reacquires workspace custody. A same-host completed creation retry preserves subsequent lane work.
Conflicting input refuses. A missing bound context after restart refuses new allocation. Incomplete
allocation recovery, context reattachment and process-tree reconciliation remain separate work.

The native desktop host exposes `attached_fleets` and `provision_attached_fleet`. Reading an absent
catalogue does not create storage. Provisioning reports `started: false`, preserves the original
capture session and exposes no publication authority. Native execution is connected as described
below; graphical controls remain to be connected. Tests cover guarded rollback, source preservation, restart
without source access, owner exclusion, missing/empty ledgers, replaced directories, linked databases,
receipt changes and closed desktop limits. Packaged graphical discovery is not yet verified.


## Desktop scheduling ownership

The native `start_attached_fleet` command accepts only a catalogue objective identity. The catalogue
returns services allocated by the current instance after rechecking storage authority; reconstructed
`restored-unattached` entries never grant execution authority. The app chooses Codex from its existing
native installed-application locations and uses its own executable's fixed fleet MCP mode. No renderer
path, generic command, credential, signing key or selected-workspace mutation enters this operation.

The application owns at most sixteen explicitly started objective loops. Each loop uses the existing
native concurrency/launch guards and 250 ms waits between ticks. Exact-instance IPC registration is
idempotent, while a different service with the same objective cannot replace a route. A repeated start
retains the original loop and handles; it never clears a fault or launches a second attempt. New loops
refuse cancelled objectives and any prior attempts. Thread-creation failure before dispatch permits
retry; process launch failure retains its durable intent and revokes the failed session.

Any tick error latches `needs-attention` and disables further dispatch for that loop. Observation and
cancellation continue through the retained host's `poll_owned`, including after slots become free.
No new host is substituted to recover from an error. Native `fleet_activity` returns a bounded
`mesh.desktop-fleet-activity/v1` projection with objective, status, stop request, observation times and
redacted per-worker facts. A failed poll preserves the old observations and their old times. Activity
is execution evidence, never authenticated file authorship or main approval. These timestamps are
wall-clock observations, not monotonic duration measurements.

`stop_attached_fleet` durably records a stable cancellation before waking the loop. It also works
before start. Direct-child stop requests leave process-tree slots and workspace custody reserved.
Dropping the app's loop owner requests best-effort cancellation; abrupt exit, an unavailable ledger,
blocked launch or surviving descendants still requires reconciliation. No graceful-shutdown or full
process-tree termination guarantee is claimed. The source project and selected desktop workspace stay
independent. Native fixture-process tests cover duplicate start, continued observation, stop-before-start,
owner loss, launch failure and fault-latched queued work. Graphical control wiring and packaged live
scheduling proof remain unfinished.

## Saved fleet review readers

Native `SavedReviewSelection` binds lane, checkpoint, immutable operation and review bundle. A fleet
reader matches all four against a completed durable checkpoint with a recorded review, then uses the
lane's retained native context and allocation identity. It never uses the desktop-selected workspace,
accepts a filesystem path, adopts a restored context, or falls back to an automatic review candidate.
This read authority survives session revocation and cancellation; it conveys no mutation or approval
capability. Existing human approval and selected-workspace artifact guards are unchanged.

The dedicated lane read seam checks the original root and installation under the workspace serial,
requires the exact durable review, reconstructs presentation/artifacts from verified immutable history,
and rechecks the physical root before returning. It intentionally does not compare a mutable working
fold: another private save must not retarget or invalidate an older selected result. Artifact access
still requires an object in the recomputed bundle and a closed before/after side, inherits the 32 MiB
artifact bound, and never reads current working bytes. Projection incompleteness and omitted-change
counts are retained rather than treating unavailable content as an empty change.

The fleet lock is released during the potentially expensive per-lane reconstruction so other lanes
can continue. The service then refreshes its guarded control history and verifies the same selection
and retained context before acknowledgment. Missing contexts and substituted folders refuse; this is
not restart/context reconciliation.

`mesh.fleet-saved-reviews/v1` pages at most fifty checkpoint identities in checkpoint-id order with
an exact `after`, `next_after`, observed control revision and total. It is not chronological order or
an atomic multi-page snapshot. New earlier IDs require refreshing the list. An unknown cursor
refuses; immutable selections remain stable across list refresh. `mesh.fleet-saved-review/v1` echoes
the complete selection with the bounded recorded review projection. These are additive native
presentation schemas, not new persisted events or agent IPC tools.

Desktop commands `fleet_saved_reviews` and `inspect_fleet_saved_review` use only current-host catalogue
services and spawn native reads away from the UI thread. They accept opaque identities, not paths or
signers. Fleet cards and parallel review panels still need to consume this seam, and the artifact
reader is not yet exposed as a desktop rendering command. Source tests verify older result bytes
after newer saves/unsaved edits, navigation, cancellation and revocation; selection mismatch, directory
substitution and pagination across fifty-three checkpoints are covered. Packaged graphical and
concurrent human approval proof remain separate requirements.

The returned presentation uses the review's recorded canonical head. It does not silently reinterpret
an attached-source operation as the managed lane's review base. Input-relative comparison and mapping
lane results back to the original project's main remain separate required work; the eventual UI must
identify the actual comparison base.


## Starting-version comparison

Fresh native lane allocation retains the single local import operation after verifying its content
against the requested immutable source. The source operation and local import operation are distinct
identities. Input-relative comparison reads that local operation and the exact recorded result from
immutable history under the retained lane context. It does not manufacture an approved main version
or reinterpret the publication bundle's base. Object identity preserves move/replacement distinctions;
pages are ordered by object ID, and selected text is bounded and content-verified.

This increment changes no durable events or workspace encoding. The retained base belongs to the
current native context. Restart reads use the history-only boundary described below and the durable
source-to-local-import binding; legacy records never infer it from a current or oldest visible version.
The comparison is read-only and is not an approval statement or dependency-closure proof.


## Durable fleet review navigation

Fleet review selectors have a separate native snapshot in the existing external attachment storage.
`desktop-fleet-pins.json` uses `mesh.fleet-pins/v1`, bound to the retained catalogue directory identity;
the desktop projection is `mesh.desktop-fleet-pin-selectors/v1`. Existing attachment pin records and
schemas are unchanged. This is navigation state, not a fleet lifecycle ledger or content provenance.

At most eight distinct exact objective/lane/checkpoint/version/bundle selections are retained, with
original input identity, object cursors/selections and closed layout choices. No source path, file
content, prompt, credential or approval is stored. Syntactically valid selectors may refer to offline
or unavailable work: native history must reverify them before displaying content, and restoring them
must not provision execution contexts, adopt workers or restart execution.

Native reads/writes hold the existing catalogue lock. Saves require the current revision, stage a
private create-only file, publish atomically and acknowledge only after file/directory durability and
readback. Concurrent or stale writes refuse. Unknown schema, copied-directory receipts, corrupt records,
symlinks, hardlinks and partial first writes refuse without replacement. Interrupted staging remains
for explicit reconciliation; a previous complete snapshot remains readable. This schema does not yet
provide automatic repair of pending files or recovery of underlying lane/process contexts.

## Durable source-to-local starting version

New allocations record the verified local initial import alongside the source version in the existing
native workspace binding. The command kind is `bind-workspace-v2`; both identifiers are immutable once
bound. This preserves the source-to-local relationship for subsequent history-only restart recovery
without choosing an arbitrary oldest version or consulting newer working files.

The database envelope remains unchanged. Legacy `bind-workspace` commands retain their exact encoding
and decode to a binding with no starting-version evidence. They are never backfilled by inference.
New v2 commands require the starting version; missing, malformed or unknown fields refuse. Older
binaries refuse the unknown v2 command rather than replaying it with less authority. Rollback therefore
requires retaining a compatible binary for fleets containing v2 bindings; ordinary project history and
attachment pin formats are unchanged. Wire tests cover both encodings and replay tests cover the
immutable binding. This change persists evidence only: it does not reopen lane contexts, recover
workers, acquire execution custody or establish that a starting version remains available.


## History-only restart access

Saved-result readers can reconstruct an unavailable live context through the native allocator without
inserting that context into execution state. A separate `FleetHistory` interface exposes only saved
result lists, exact review projections, starting-version comparisons and verified artifact bytes.
The catalogue discovers retained fleets lazily for pinned reads. `current_service` still refuses
restored owners; history access never grants credentials, starts workers or clears uncertain runs.

The allocator pins the admitted lane directory and validates both working and private-store physical
identities against the durable installation before opening history. Both directories must remain
inside that lane's native allocation. History opens an existing journal only, builds a transient index,
and skips managed-file reconciliation and checkpoint-runtime installation. Its payload filesystem
refuses writes, directory creation and corruption quarantine; absent directories and corrupt bytes
remain untouched. Journal access uses a read-only regular-file descriptor. Reads check the exact
recorded checkpoint/review/version, physical directories and durable binding before returning. Descriptor ancestry is rechecked after the read, so an ancestor
alias cannot conceal a directory moved outside its allocated lane. Existing
live readers retain their context and use the same immutable selection contract. No fleet-wide lock is
held while reconstructing saved content.

Tests cover fresh-host reads with the source offline, unchanged uncheckpointed files and native state,
no writes to retained lane files, missing indexes without durable rebuilding, missing/linked journals
without recreation, replaced roots, revoked old credentials, refused new grants and pending mutations
left untouched. This is history access, not process recovery, execution adoption or main integration.
The packaged graphical restart journey remains required.

## Exact review change requests

The additive `request-review-changes` event records a native-requested message against one completed
checkpoint and its exact lane, saved operation and recorded bundle. The private runtime ledger retains
it independently of run lifetime. The native service verifies retained review history before recording;
replay also requires the same completed checkpoint and bundle. Existing command bytes and database
schema are unchanged. Older binaries refuse this unknown command; rollback of a fleet with feedback
requires a compatible binary. Unknown fields and versions refuse rather than discard feedback.

Messages contain 1–8192 UTF-8 bytes of nonblank text, with control and direction characters refused
except line feed and tab. The ledger bounds requests to 32 per lane and 256 per objective; no automatic
retirement is implemented. A caller retry identity hashes to an objective-scoped request identity.
Identical retries recover the original receipt even after later events; altered content or selection
with the same identity refuses. Recording does not modify versions, review state, main, cancellation,
run state or custody. It does not dispatch a process, acknowledge delivery or establish human signing
identity. Feedback is ordinary native-requested work input, never an approval receipt.

Authenticated `context` exposes only the current lane's recorded requests, each carrying its exact
checkpoint/version/bundle. Child lanes do not inherit parent feedback. Agents cannot invoke the native
recording method through their scoped action router. Historical read access exposes feedback after
restart without adopting execution authority. The desktop uses current-host ownership for recording;
restored fleets remain read-only until recovery. The existing MCP context tool's additive response
field requires no new tool or agent mutation permission.

Desktop panels retain an uncertain request identity and message for explicit exact retry, reject
substituted receipts, and ignore late responses after the panel closes or is replaced. Stored rows
remain durable; drafts and unconfirmed desktop retry identities are session-only. After closing or
restarting during an uncertain request, read recorded requests before creating another. There is no
automatic provider interruption, resume, completion acknowledgment, or claim that the agent has read
or addressed feedback. These lifecycle extensions and packaged graphical verification remain work.

## Proposed results for review change requests

The additive `propose-review-change-result` event links an existing request to a complete recorded
checkpoint from its originating lane. The caller's actor/session/run/generation are taken from native
credentials, must match the checkpoint's capture origin, and must name the active run. Native code
verifies the retained review and holds the custody guard while recording. Agents supply only the
request and checkpoint identities; saved version and bundle are derived from verified state. The
original checkpoint or identical saved operation is refused. This is an explicit proposal, not proof
of semantic improvement, chronology, human acceptance, resolution, or approval.

Up to eight proposals per request remain in append order. The retry identity derives from the
request/checkpoint pair; identical retries recover the original record. Altered origin, duplicate
proposals under another event key, unsubmitted/incomplete checkpoints, other lanes, expired sessions
and cancelled runs refuse. The original request, checkpoint, review and execution state are unchanged.
Existing event bytes remain unchanged; older binaries refuse the new unknown command. The new state
projection is reconstructed from the ledger without changing its database schema.

Scoped MCP exposes `mesh_fleet_propose_review_change_result`. Context retains its original request
array and adds a separate `review_change_responses` array for that lane only. Read-only history can
return original requests and proposed selectors together at one runtime revision. The desktop read
projection is now `mesh.fleet-review-changes/v2` with a closed `activity` object; original mutation
receipts remain v1. Bundled frontend/native versions move together; older closed readers refuse v2
rather than silently hide proposals. No saved pin or workspace format changes.

A reviewer explicitly pins a proposed result beside the original, using only a selector returned by
verified feedback. Each result is rechecked through the ordinary saved-review reader, including after
restart; unavailable content keeps an unavailable pin. The original panel never follows the proposal
automatically. Pin limits and persistence are shared with ordinary saved results. Linking a result does
not mark the request addressed, wake a provider, or permit publication; human resolution and original
project integration remain separate work.

## Reversible native-confirmed request decisions

The additive `set-review-change-decision` event records an explicit work decision for a request:
addressed by one of its exact proposed checkpoints, or open again. A per-request revision starts at
zero and increments on every change, including reopen. Native confirmation binds that revision and
both original/proposed saved identities; a stale choice refuses even after an address/reopen cycle.
Each request permits at most 64 decisions. Existing event encodings, database schema and workspace
formats remain unchanged. Older binaries refuse the new unknown event rather than discard decisions.

`FleetService::decide_review_change` is native-only. It verifies immutable review identities, releases
the fleet lock before invoking confirmation, revalidates the retained identities afterward, and appends
through revision/idempotency checks. Other lanes continue progressing while the dialog is open.
Cancelled confirmation appends nothing. Stable operation identities recover existing exact receipts
without showing another dialog; changed parameters under a recorded operation refuse. An old receipt
returns its original decision and the fresh current decision separately, so retry cannot undo a later
reopen. Receipts can be recovered without rereading missing artifact bytes after the accepted action.

This is ordinary work status, not a signed human approval receipt or verified semantic judgment.
Neither addressing nor reopening changes main, workspace bytes, review content, run state or custody.
The native desktop dialog states that distinction and displays the exact request and version identities.
Agent credentials cannot invoke this operation. Agent context exposes the latest per-request decisions;
new proposals for addressed requests refuse until a native reviewer reopens them. Already recorded
proposal retries remain idempotent. Work decisions remain available after execution cancellation.

The closed desktop activity projection is `mesh.fleet-review-changes/v3`, adding one decision per
request. Mutation receipts remain separate, and the new decision envelope is
`mesh.fleet-review-decision/v1`. The renderer verifies original selection, operation, desired result,
receipt revision and monotonic current state, then displays current state rather than an old receipt.
A failed response retains exact retry arguments. Explicit reload clears them only after current state
verifies; a closed panel ignores late replies. Pending desktop operations remain session-only.
Tests cover cancellation, competing confirmation, stale revisions, reversible replay, root relocation
during confirmation, agent denial and old-receipt recovery. Packaged graphical confirmation and signed
main approval are separate required evidence; native callback fixtures do not prove user presence.


## Original-project correspondence preparation

`FleetService::saved_project_mapping` and its history-only wrapper verify the entire recorded input
ancestry from an attached root through the selected delegated result. Each step binds the lane,
source version, local initial import and exact output version used by the next child; the final step
binds the recorded review selection. Cycles, missing or foreign projects, inconsistent bases/depths,
unbound legacy imports and more than the authorized depth (at most 33 lanes) refuse. Every retained
history reader is verified before and after the operation, and the recorded lineage is rechecked
against fresh runtime state. Reading does not adopt worker handles or hold their daemon locks.

At every import boundary, complete immutable inventories must match by path, type, content digest,
length and executable bit. Independent object IDs and manifest IDs are not interchangeable. Original
object identities flow through local edits and remap at verified imports. The final comparison is
against the original project input, including ancestor additions, moves and deletions. A deleted
object recreated at the same path has a distinct identity. Changes made by a parent after delegation
cannot replace the exact saved parent version used by the child. Intermediate versions are explicit
in the returned lineage; they are not human-approved merely because the final result references them.

The closed native projection is now `mesh.fleet-project-mapping/v2`. It carries the original source
project/input, ordered lineage, observed verified main, and paginated correspondence in stable
`correspondence-id` order (200 rows per page). Rows use `source:<original-object>` for source changes
and `result:<final-object>` for additions. A deletion can have no final lane object, so v1's local-only
object ordering is intentionally replaced. There are no renderer consumers or persisted mapping
records to migrate. Existing fleet ledger and workspace encodings remain unchanged.

Main and the source input are read under one attachment-history lock. Main is an observation, never
a reservation. New source captures and live edits do not replace the saved input. This projection
creates no version, review, approval, source write or worker adoption. Its scope is recorded single-
parent input ancestry; separately authorized private dependency graphs, rejection propagation and
publication validation still require implementation.

This remains native review preparation, not integration. A source-history candidate with explicit
provenance, desktop presentation, exact human approval and divergence-safe grouped write-back are
still required. Unit tests cover three generations, ancestor deletion/recreation, complete input
mismatches, graph validation and pagination. The native delegation journey proves edits/additions,
exact parent-version selection, replaced-ancestor refusal and history-only restart. It explicitly
verifies that a missing managed file still produces an incomplete checkpoint which cannot be
submitted. Explicit agent deletion resolution remains unfinished; synthetic saved deletion mapping
is not proof of that capture capability. No packaged graphical or integration claim follows.


## Durable candidate staging before project review

`FleetService::stage_project_candidate` accepts an exact saved review, original native attachment,
32-character stable request and expected project main head (or genesis). It verifies recorded ancestry
and retains the selected immutable content in `fleet-candidates/candidate-<digest>` under the external
project metadata store. The original project files, capture journal, private capture head, review queue
and accepted main are not modified. The operation is native-only; no renderer command or agent tool is
exposed. This is candidate staging, not source-history operation import or project review creation.

Each create-only allocation records canonical intent, provenance, a complete content manifest and a
separate ready receipt. Provenance includes the original project/version, fleet objective, exact
review selector, all recorded input versions, expected main and the recorded checkpoint actor/session/
run/generation. These are verifiable references to existing native history, not a standalone signed
approval or an assertion that all inherited work was authored by the final agent. The manifest binds
saved object identities, paths, entry types, content digests, lengths and executable bits. File copying
uses retained historical content and verifies the entire resulting inventory; live working bytes
cannot replace the selected result.

Admission and initial intent use the existing native metadata lock. Content copying and lineage/main
revalidation run outside that lock, so source capture is not locked for the whole copy. A ready receipt
is written only after fresh native checks and copied-content revalidation. Native namespace identity
and complete content are checked again for acknowledgment. Existing requests compare exact intent and
retained content; different input refuses. A completed exact receipt can be recovered after main moves,
without restaging or treating the candidate as eligible for approval. An incomplete allocation is
retained and refused, including on retry; no automatic cleanup, repair, overwrite or duplicate copy.

The history-only `inspect_project_candidate` path can recover only an existing complete candidate.
It creates no candidate directories, starts no workers and never repairs missing or altered bytes.
Inspection still requires verifiable source and lane histories and the original project root; standalone
offline candidate verification and discovery are not implemented. Current limits are 128 retained
allocations per project (including incomplete ones), 10,000 entries, 64 MiB total and 8 MiB per file.
Exceeding a limit preserves existing data and refuses the new allocation.

New external formats are `mesh.fleet-candidate-intent/v1`, `mesh.fleet-candidate-provenance/v1`,
`mesh.fleet-candidate-content/v1`, `mesh.fleet-candidate-ready/v1` and the native receipt projection
`mesh.fleet-project-candidate/v1`. Existing fleet events, capture journals, pin selectors and workspace
formats are unchanged. Older code has no candidate reader and does not interpret these directories as
captured work or approval. Source tests cover exact saved bytes despite later edits, lost-reply retry,
stale main, conflicting requests, restart inspection, source-history preservation, interrupted
revalidation, concurrent retained edits, changed manifests, replaced files directories and capacity.
Creating an original-project review against its exact main base, importing operations with correct
provenance, private dependency validation, UI, human approval and grouped write-back remain required.


## Candidate review against fixed original-project main

`review_project_candidate` now derives a read-only original-project content review from an existing
complete staged candidate. Both current services and history-only readers support it. It first and
last revalidates the retained candidate, reopens the exact result through native history, and compares
against the candidate's recorded expected main. A non-genesis base must belong to the trusted project
approval fold, not merely to a private version or stored review. The exact base operation is resolved
from that verified head. Main advancement does not replace the original comparison.

The comparison covers the whole proposed project. In particular, a genesis review shows all proposed
files, including inherited input, rather than only changes made after lane allocation. Paths are
compared by type, content digest, length and executable bit; moves appear as path removals/additions,
not inferred identity changes. This is a content-tree presentation, not a signable operation review.
Original object correspondence and ancestry remain separate recorded evidence for eventual import.

The canonical `mesh.fleet-project-review-context/v1` binds project/candidate identity, the complete
candidate receipt digest (including provenance), fixed base head/operation, target operation and
content manifest digest. Its digest identifies the read-only review. This identity is recomputed from
persisted immutable inputs and stays identical across pages, selected files, new captures and main
advancement. No new lifecycle record or source ReviewRecord is appended. The native projection
`mesh.fleet-project-candidate-review/v1` keeps observed main and `base_is_current` outside the immutable
context. The latter is an observation, not approval eligibility or a reservation.

Pages contain at most 200 changed paths with exact total and cursor. Selecting one changed path
returns verified saved before/after text bounded to 256 KiB per side; folders, large files, binary
content and unsafe control/bidi text have explicit states. Unknown paths, mixed page/file selection,
substituted manifests, unverified bases and altered retained content refuse. File paths select only
known saved changes and never grant arbitrary filesystem access. Artifact previews for this new
comparison remain unconnected.

Native tests exercise genesis completeness, selected immutable bytes, restart identity, real advancement
of the trusted approval fold with fixture signatures, old-base readability/staleness, new-base identity,
untrusted reviewer refusal, bounded pagination/text and no source-history mutation. Fixture signatures
do not prove graphical user presence. The digest cannot be used as a source approval bundle. Source-
history operation import, signable review creation, private dependency validation, desktop presentation,
human approval and grouped integration remain required.


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
