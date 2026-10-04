# Private input authority and publication

Status: implementation contract for fleet phase 4; not implemented by this document.
Tracks [issue #289](https://github.com/idosams/Mesh/issues/289). The full
[fleet plan](../plan/fleet-orchestration.md#phase-4-private-dependencies) remains authoritative.

## User-visible outcome

A person or an authorized agent can start a separate line of work from an exact saved private
version before its author publishes it. Later source edits do not change that input. Mesh retains
and displays every transitive input in review. Rejecting or replacing an input makes dependent
proposals require explicit revalidation; an old receipt, a successful worker exit or a newer save
cannot publish them indirectly. Existing accepted main and historical views remain readable.

Manual work uses the same project/work identities and requires no provider configuration. Agent
objective/lane/run identifiers correlate execution with work; they are not the work's identity.
Existing projects, working folders, editors and Git remain in place. This contract governs Mesh's
native consumption, import and publication paths; it does not claim to infer the provenance of
arbitrary bytes copied outside those paths.

## Authority and immutable facts

Each enrolled project has one native dependency authority, bound to its existing private history
and physical installation. The initial implementation authorizes dependencies within that project;
foreign project/objective access requires a separately authorized import and never follows merely
from knowing a version identifier. The project authority is selected by native registration, not
an agent-provided filesystem path or a renderer-provided policy database.

| Fact | Required binding | Meaning |
| --- | --- | --- |
| Work identity | Native project identity, stable work identity, exact installation/custody receipt | Identifies manual or agent-assisted work independently of an execution attempt |
| Saved input | Source work, exact operation, verified history/custody receipt, transitive closure digest | Immutable bytes and ancestry; no access or approval authority |
| Access grant | Project authority, exact input, destination work, authorization generation, stable request identity | Permits that private consumption only; agents cannot issue or broaden grants |
| Consumed input | Grant, exact source and destination, materialized starting operation, full inherited closure | Records what allocation actually consumed; acknowledgement follows durable verification |
| Eligibility decision | Exact input identity, predecessor decision, monotonic decision revision, rejection/replacement or explicit revalidation | Changes current publication eligibility without changing retained content or historical grants |
| Review snapshot | Exact output, full closure, decision revision vector, native validation result | Fixes what the human considers and what publication must recheck |

An ordinary saved operation is eligible to be authorized as an input. A checkpoint or submitted
review is optional evidence, not a prerequisite. Grant expiry/revocation stops new consumption;
it does not prove the worker stopped. Execution ownership and capacity reconciliation remain
separate. A rejected input remains readable where existing retention/access permits inspection.

Existing parent delegation already consumes private saved operations. Its input, inherited limits
and authorization evidence must enter this same model. Do not enforce the model only on a new
cross-lane API while leaving ordinary delegation, manual forks or result imports outside it.
Historical parentage alone must never manufacture a new access grant.

## Durable storage and compatibility

Use explicit immutable dependency records in the owning project's existing native record journal,
with canonical, bounded native payloads retained in its content-addressed store. SQLite remains a
reconstructable index; do not make a second mutable policy sidecar the authoritative decision log.
The envelope must identify the project authority, predecessor/revision and exact payload digest.
Native record validation must distinguish grants, consumed bindings, decisions and review snapshots;
an opaque indexed payload is not itself a verified grant. Native control entry points authorize
writes. Imported historical records and actor signatures cannot enroll or replace the local authority.

Stage payloads before the durable journal append. A grant or decision is acknowledged only after
its frame is durable and replay agrees. Stable request identity recovers the identical earlier
outcome; a conflicting request fails. An allocation can crash after its grant commits: recovery
may finish the exact recorded allocation, but cannot choose a new source or launch twice. A
consumption binding becomes visible only after the destination identity and starting operation
are verified. Uncertain partial allocations remain retained for recovery.

Introduce a required journal record kind/schema before allowing dependency-bearing publication.
Current journal scanning refuses an unknown complete record kind; preserve that property so older
readers cannot silently ignore the new authority. Test with the previous reader as well as the new
one. Old empty histories keep their existing behavior. Old accepted approvals are reconstructed
with their historical evidence. Unenrolled or incomplete dependency histories remain explicitly
legacy/unknown for new dependent publication until native migration/revalidation establishes the
missing evidence. Never rewrite old accepted main or infer a grant from readable legacy content.

## Exact closure and retention

Traverse immutable work/version edges, not live folders, latest-save pointers or provider states.
Canonical ordering and deduplication produce the same closure digest independently of traversal
order. Bound nodes, edges, depth and encoded bytes; reject overflow, cycles, conflicting duplicate
bindings and missing records. An unavailable remote authority is unknown, not an empty closure or
an eligible result. Do not truncate a closure and then authorize its visible prefix.

Every binding and review retains its referenced operations, manifests and policy payloads until
its native lifecycle releases those references. Rebuilding the index must reconstruct those roots.
Collection must use the same dependency-aware retained-root computation, including after rejection,
restart and failed allocation. A rejection cannot remove the evidence needed to explain it.

## Publication and rejection serialization

Rejection and all affected publication paths must share the same native project authority barrier.
The current custody primitive permits one physical workspace at a time. It is not already a
multi-workspace barrier: an attached source policy and a managed destination may have different
roots. Add and prove an explicit bounded custody-set transaction before enabling those paths.

The transaction pins and verifies every required root, deduplicates identities, and acquires kernel
locks in one deterministic identity order before any daemon view mutex. Ordinary single-root
mutation continues to contend on those same kernel locks. Entering a set while already holding an
unrelated single-root lock refuses. Nested history opens may reuse only identities in that exact
native set; they cannot silently acquire another root. Guards must be confined to the owning thread
and release all acquired locks on every error. Do not relax ordinary custody checks globally.

Prepare content and collect the human receipt before acquiring this barrier. Under the barrier,
refresh durable project decisions and destination history, reconstruct the exact closure and review
snapshot, verify installation/custody, receipt, current main and every expected decision revision,
then append the approval. No human ceremony, provider wait or network round trip occurs under it.
The R145 managed-history fix in [PR #291](https://github.com/idosams/Mesh/pull/291) demonstrates why
locking alone is insufficient when a daemon retains a cached view; it does not implement this set.

| Commit order | Required result |
| --- | --- |
| Rejection/replacement commits before approval | Old snapshot is stale; refuse before any approval append |
| Approval commits before rejection | Keep the accepted historical main; later dependent proposals require current eligibility |
| Approval commits but acknowledgement is lost | Recover the exact durable receipt/outcome; do not append another approval |
| Decision or approval append is interrupted | Replay the actual durable boundary; never infer a committed decision from an intent |
| Root/authority is replaced or becomes unavailable | Preserve evidence and refuse current publication |

Both attached `approve_saved_review` and managed `approve_review_for_workspace` must enforce the
contract, together with local/remote candidate import, ordinary delegation/fork, grouped integration
and retained-receipt recovery. Historical approval reconstruction uses its recorded snapshot; a
new admission uses current eligibility. Do not use the workspace-wide policy epoch as a substitute
for per-input decision revisions: unrelated reviews must not acquire accidental dependencies.

Remote returned results carry exact input bindings and authenticated historical provenance. They
must rejoin the owning project authority for a fresh publication decision. A transport receipt or
R66 parent-review binding is not current eligibility, and disconnection cannot authorize an offline
publication or prove that execution ended.

## Delivery and proof sequence

1. Implement and test the native custody-set primitive against the existing single-root paths.
   Prove both race orders with separate processes, opposite requested root orders, root replacement,
   partial acquisition failure and restart. Keep it unexposed to agents and the renderer.
2. Add canonical dependency records, journal/index reconstruction, required-reader compatibility,
   grant/decision replay and crash-boundary tests. No new dependency consumption is enabled yet.
3. Integrate explicit native grants and exact allocation bindings for manual and agent work,
   ordinary saved versions, parent delegation and remote inputs. Prove retry identity, inherited
   limits, substituted-input refusal and retained roots before exposing scoped agent operations.
4. Bind closure and decision vectors into review and enforce all publication/import/recovery paths.
   Prove rejected transitive inputs cannot pass through ordinary or combined results; test both
   concurrent commit orders and exact lost-acknowledgement recovery.
5. Add native-backed dependency presentation, authorization and revalidation controls. Keep each
   review pin independent; show unknown/stale eligibility without changing its saved version.
6. Run real-provider consumption before upstream publication and eligible-signed packaged manual,
   harness and fleet journeys, including rejection, restart, review and accepted-main integration.

Each increment needs its own published PR before the next substantial increment, source provenance,
positive/refusal and failing-before evidence, the complete Mesh gate, hosted checks and authorized
merge. No sequence item is complete merely because this contract or an isolated helper is merged.
The phase exit remains: exact private consumption works, and rejected upstream work cannot reach
shared state indirectly through any supported downstream publication path.


## Native barrier foundation

R146 adds the native initialization-set primitive, bounded to 32 requested roots before
identity deduplication. It acquires the same kernel directory locks as single-root custody in
physical-identity order, verifies every admitted namespace after acquisition, refuses nested
extension, and confines lock guards to the owning thread. Single-root initialization uses this
same implementation. Borrowed history-open guards verify that their exact custody is still held.
Partial acquisition errors release all acquired locks and thread membership. This is a native
serialization primitive, not a dependency transaction, access grant or publication capability.

Twelve focused custody tests pass, including separate processes contending in both acquisition
orders, reverse requested input order, both member roots, duplicate/bounded requests, unrelated
nesting, borrowed-guard lifetime and replacement during a blocked partial acquisition. Replacing
exclusive locks with shared locks makes the separate-process regression fail; source restored
byte-exact. Full repository validation and hosted delivery are recorded in the migration ledger.
The remaining record, authorization, closure and publication integration steps are still required.

## Durable enrollment preparation fence (R149)

`DependencyEnrollmentFence::prepare` pins the expected native installation, takes its existing
physical-directory custody lock and installs a required `mesh.workspace-agent-custody/v2` marker.
It binds the exact installation, directory identity and nonzero dependency authority; its generation
is null because preparation refuses assigned workspaces. Generic v1 custody parsing remains
unchanged and refuses this record. The API is a native Rust preparation primitive only: no agent,
renderer or CLI operation invokes it, and it does not append enrollment or grant policy-aware writes.

The marker is staged with owner-only permissions, file-synced, atomically renamed and directory-synced
through the existing custody publication path. A thread-bound guard retains the directory lock and
rechecks both namespaces, continued custody and exact marker bytes. Dropping the guard releases the
lock but never clears the marker. Exact retry reuses the same fence and syncs both the existing file
and directory before returning; a lost acknowledgement after rename cannot skip that durability step.
Malformed records, changed authorities, assigned workspaces, wrong installations, changed marker
bytes and replaced namespaces refuse. Interrupted staging is disposable; a published required marker
remains explicit recovery evidence. No enrollment journal append is enabled by this increment.

Eighteen focused custody tests and a cached native-daemon regression pass. The latter first performs
a valid edit, then prepares the fence and verifies that edit, file creation, review creation and
agent-custody acquisition refuse without changing managed file/journal bytes or invoking signing.
The regression also passed against independently archived pre-change revision
`60441cb0013df707d71f8781b27964514cb32310`, with all 1,413 original tracked files verified against
Git blobs before adding the standalone probe. The old probe publishes only the exact required marker;
all production reader code is unchanged. Removing its marker lets the cached edit proceed and fails
the refusal regression; the restored probe passes. This proves those cached native paths, not a valid
human approval, every historical writer, a packaged desktop journey or complete enrollment recovery.

A controlled retry directory-sync refusal returns no guard and retains the fence. Bypassing retry
sync makes that regression fail. This is targeted fault-injection/staging evidence, not a complete
power-loss campaign. The future transaction must install the fence before its enrollment record,
retain recovery evidence across both writes, validate local control authority and cover every
consumer/publication path before private-input behavior is enabled. Preparation does not rewrite accepted main or journal content;
this foundation does not automatically migrate any existing workspace.


## Attached-history preparation (R150)

Older attached-project approval and capture paths do not consult the managed custody marker.
An independently compiled reader at `60441cb0013df707d71f8781b27964514cb32310` accepted a
valid prepared attachment approval with that marker present. The same reader refused approval
and exact retry with a required attachment-history binding, leaving journal and source bytes
unchanged. This closes neither all historical paths nor complete enrollment recovery by itself.

Native `ProvisionedAttachment::prepare_dependency_enrollment` now prepares that distinct fence
under existing store custody. The required `mesh.attachment-history/v3` binding contains the exact
previous binding and native-selected dependency authority. Existing history readers are unchanged
and refuse it. Preparation validates registration, retained history identity, bounded private regular
files and source/store namespaces. Staging is synced before rename; the required binding and directory
are synced before acknowledgement, including exact retries after a lost acknowledgement. Conflicting
stages, authorities, changed bindings and substituted namespaces refuse and preserve their evidence.
Dropping the thread-bound guard never removes the fence or edits the source project.

This is an unexposed native preparation primitive. No renderer, agent or CLI invokes it; there is
no automatic migration, enrollment append, policy-aware write permission or publication authority.
The original binding and accepted journal bytes are retained, but ordinary Mesh history reads refuse
while this preparation is present. Complete enrollment/recovery must add validated current-reader
support before enabling the transition. Independent all-path old-writer proof and packaged acceptance
remain required. Ordinary external tools can continue editing the source folder.
