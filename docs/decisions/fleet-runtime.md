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
