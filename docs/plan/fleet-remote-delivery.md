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
