# Remote fleet delivery sequence

Remote workers extend the same objective, lane, attempt and immutable review model.
They do not gain approval authority, own the user's working project, or cause local
runs to be adopted after a restart. This sequence implements Phase 6; it does not
replace the required second-machine execution and disconnect/reconnect proof.

1. **Durable assignment and leases.** Before any transfer or launch, reserve the
   existing attempt for an objective-unique assignment, a natively admitted worker
   key, the exact lane input version and immutable transfer-bundle identity. Persist
   lease sequence and expiry. Advance only the same assignment and peer using an
   exact expected sequence. A disconnect, deadline expiry or renewal never frees a
   concurrency slot, establishes completion or permits another launch.
2. **Authenticated native transport.** Establish peer identity outside renderer and
   agent input. Bind authenticated messages to the objective, lane, run, assignment,
   input bundle and lease sequence. Reject substituted peers, stale messages and
   unauthorized control requests. Retain host-key/account setup outside automatic
   product edits; no secrets in command arguments, source history or public logs.
3. **Verified immutable transfer.** Transfer a bounded manifest and content addressed
   input. Verify complete bytes before acknowledging them or allocating execution.
   Stage partial transfers privately; reconnect can resume confirmed content without
   accepting changed manifests, path traversal, links or ambient remote files.
4. **Remote executor ownership.** Persist a receiving-side launch claim before
   provider spawn, use the same provider/custody checks, and acknowledge the exact
   assignment. Repeated delivery or restart cannot spawn a duplicate. Reconcile an
   uncertain attempt by its retained identity, never by assuming a process vanished.
5. **Result and reconnect reconciliation.** Persist acknowledged checkpoints and
   immutable outputs before receipt. Verify transferred bytes and provenance locally
   before exposing a review. Cancellation and expired leases remain uncertain until
   native evidence proves their disposition. Review stays local and tied to the exact
   input/output; no remote completion advances protected main.
6. **Live presentation and acceptance.** Show worker/connection/lease uncertainty as
   native facts alongside local lanes. Execute on a real second machine, disconnect
   during work and after acknowledgment, reconnect and verify exactly one attempt,
   retained acknowledged work and exact end-to-end human review. Test peer/key
   substitution, replay, partial transfer, stale leases and changed result bytes.

The first increment adds additive `claim-remote-launch` and `advance-remote-lease`
ledger commands. Existing local claim encoding is unchanged and old histories replay
with no remote assignment. Older binaries that do not understand these commands
refuse their history instead of guessing at ownership. The runtime reducer records
correlation only: its public key-shaped field and bundle digest are not authentication
or content-verification evidence. A native caller must establish those facts before
submitting commands. No renderer, agent command or network listener exposes these
new commands in this increment.

Lease expiry is stored as a native-authorized Unix millisecond deadline; the reducer
has no wall-clock authority. A renewed record remains uncertain until the native
transport reconciles it. The same request can recover its original durable receipt,
but this is never a second grant to launch. Sequence exhaustion refuses. An expired
assignment is retained and cannot be reused by another lane, worker or local host.

## Assignment worker proof

The next increment supplies a native single-use challenge for a still-unclaimed
allocated dispatch. Native configuration supplies the expected worker key separately
from the proposed assignment. An OS-random nonce and a distinct signing domain bind
that key to the objective, lane/run, immutable version and bundle, assignment/lease,
provider and goal. Native paths are retained for local context revalidation and are
not included in the task-bearing challenge. Never log the challenge body.

The challenge expires at the earlier of thirty seconds or the assignment deadline.
Backward clock observations refuse. The consumed reply must verify through Mesh's
strict Ed25519 implementation; then native code refreshes the durable context and
uses the current revision for the claim. A changed attempt, cancellation, competing
claim, substituted key, nonce, bundle or domain refuses without claiming execution.
Losing a challenge on restart requires fresh proof and does not adopt a worker.

This proves possession of the configured worker key for one pending assignment and
records ownership only. It does not authenticate the coordinator to the worker,
protect a transport, prove transfer completeness, spawn a process, authorize a lease
renewal, trust results or approve main. Native peer/key provisioning, mutual transport
authentication and the remaining delivery steps above are still required. The lower
level ledger commands remain trusted-native primitives; a peer reply cannot call them.

## Immutable input transfer contract

The transfer increment defines `mesh.remote-input/v1`: exact saved input identity,
explicit relative directories (including empty ones), and regular files with portable
executable metadata, ordered chunk lengths/hashes and a complete-file hash. A separate
BLAKE3 domain binds the canonical manifest to the assignment's bundle identity. Decoder
input must match both native expected identities; reordered/noncanonical data, unknown
fields or kinds, traversal, missing directory parents and duplicate paths refuse.
No link, device, socket or other special-file representation is admitted.

Bounds are part of this first protocol version: 1 MiB canonical manifest, 4,096 entries,
16,384 chunk references, 4 MiB per chunk, 2 GiB total reconstructed content, 64 KiB per
received part, 4,096 UTF-8 bytes per relative path and 255 per component. These are
resource limits, not measured performance or filesystem portability claims. A native
sender must report a refused input without dropping unsupported entries or altering
the user's project. Native materialization still owns volume-specific collision and
identity checks.

The receiver constructor also checks the manifest against the retained native assignment.
It composes the existing CAS's durable partial offsets, integrity checking,
atomic promotion and corruption handling. Only declared chunks and exact bounded
contiguous parts are admitted. A lost acknowledgment is recovered by reading the
verified complete status or durable offset; it never appends the same prefix twice.
Whole-file reconstruction hashes must also match before the receiver reports complete
input availability. Empty files and directories remain represented.

The caller must own and revalidate a private store, exclude competing receivers, and
bind the receiver to an authenticated assignment. No network listener or arbitrary
store-path input is exposed. Verified availability is a read-time fact, not permanent
materialization authority: native allocation must recheck store/destination identity
and the actual output. Source-version export, authenticated transport, receiving-side
allocation/execution and second-machine acceptance remain separate required steps.

## Native attached-input export

A provisioned attachment can now prepare an exact saved version as a `RemoteInputSource`.
Preparation uses the existing native history/registration checks and its inspection lock,
including normal disposable-index recovery. It derives the transfer manifest from the verified
historical tree and retained chunk metadata; current project files are not transfer inputs.
Empty directories/files and executable metadata remain explicit. Unsupported protocol bounds
refuse the input instead of omitting entries.

The returned handle owns immutable metadata and pinned directories, without an inspection lock,
live daemon, signer or execution context. A later capture can complete while that handle remains
alive. Each chunk request must occur in its declared manifest; it checks root identity before and
after a bounded read and verifies exact length and BLAKE3 before exposing bytes. A grown file is
read only to its expected size plus one byte, at most 4 MiB plus one. The returned verified chunk
is at most 4 MiB; native transport must split it into the receiver's 64 KiB parts. Missing content,
symlinks, changed roots and corruption refuse. Handle reads never repair or quarantine storage.
This does not pin retention forever: collected content becomes explicitly unavailable.

The API is native-only and conveys no dispatch or destination authority. Callers still must bind
its manifest to the exact authorized assignment and authenticated peer, recheck session/lease
state and provision receiving-side storage. Managed-lane/private-dependency export integration,
mutual transport, remote execution/results/reconnect and actual second-machine acceptance remain
required. Preparation itself is not a network transfer, and source tests are not remote proof.

## Managed saved-review export

Native fleet services and history-only catalogue readers can prepare input from an exact recorded
lane/checkpoint/version/review selection. The service verifies its durable selection and allocation
binding, reopens the recorded history through the native allocator, checks the recorded review and
complete saved tree, and revalidates the selection before returning the export handle. It does not
install a live context, checkpoint timer, credential or provider. Original-project availability is
not required for retained lane history. Working edits do not become export content.

The handle retains allocation-parent directory pins and a physical allocation boundary in addition
to its workspace/store pins. Every later chunk read rechecks these facts. A directory moved outside
its admitted lane behind an ancestor symlink refuses even when its final directory identity still
matches. An unavailable or replaced allocation cannot be repaired or recreated by export.

This is a read capability for saved bytes, not authorization to consume a private dependency or
launch it remotely. The caller must separately verify the current dependency closure, rejection
state, assignment and peer policy. Remote dispatch integration, authenticated transport, receiving
materialization/execution, result/reconnect reconciliation and real second-machine proof remain open.
No agent or renderer command exposes this native export API.

## Receiving-side working trees

The native receiver can materialize its exact manifest into a fresh private allocation. Native
configuration supplies existing store/parent directories and their retained installation identities;
peer and renderer paths confer no authority. Storage must be private, nonoverlapping and outside
all supplied protected user-project roots. The native caller must still admit the authenticated
assignment, enforce dependency policy and exclude competing receivers.

Allocation uses a native 32-character request identity and create-only directories. An existing or
partial allocation refuses; no retry deletes, adopts or overwrites it. Its canonical manifest is
retained beside the working folder for later reconciliation. Chunk reads are descriptor-confined,
bounded to declared length plus one and checked by digest before writing. Final verification checks
the complete bounded inventory, complete-file hashes, executable metadata, hard links and physical
allocation ancestry. Empty files and directories remain explicit. Unrepresentable names or volume
collisions refuse through the exact output inventory instead of silently changing the input.

The returned handle can revalidate the tree before a later executor claim. It is not a persistent
readiness or launch receipt, does not initialize a workspace or spawn a provider, and cannot be
reopened after restart without native reconciliation. Failures preserve partial work for inspection.
Authenticated transport, durable receiving ownership, workspace initialization, result transfer and
real second-machine acceptance remain required.
