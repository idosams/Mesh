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
never automatically relaunched. The embedding application's tick loop, durable process reconciliation
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
