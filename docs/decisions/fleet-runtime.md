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
