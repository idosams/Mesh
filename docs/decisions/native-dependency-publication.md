# Native dependency publication

This continues [private dependency authority](private-dependency-authority.md) and
[issue #345](https://github.com/idosams/Mesh/issues/345). The complete
[fleet plan](../plan/fleet-orchestration.md) remains the delivery scope.

## Durable record contract

The required dependency envelope kind `Publication` has stable code 7. Its closed canonical
payload uses `mesh.dependency-policy/v5`; no other envelope kind accepts that schema. The
existing envelope binds the native owning authority, monotonically ordered journal position,
previous payload and current payload digest. SQLite migration 5 extends the kind constraint,
copying every existing row inside the migration transaction. Earlier migrations are unchanged.

The body contains these fields in this exact order:

| Field | Meaning |
| --- | --- |
| request | Immutable nonzero retry identity |
| revision | One-based publication sequence for the reviewed work and installation |
| previous | Previous publication payload for that work, or zero for its first publication |
| review | Same-owner retained v4 saved-review binding |
| receipt | Digest of the exact human receipt stored in the owner CAS |
| result | Nonzero claimed resulting main head; not an operation ID or CAS root |
| credential | Nonzero claimed human credential identity |
| challenge | Nonzero ceremony challenge, unique across publications in this owner history |

The immutable review binding selects the output work/installation, prior canonical head,
review bundle and complete snapshot. The snapshot selects the graph, qualified inputs and
exact decision revisions. Publication replay requires those decisions to be eligible and
unchanged at the publication's journal position. An unrelated decision does not stale them.
Rejection followed by revalidation does stale them, even if the inputs are eligible again.

Each later publication must name the exact previous publication and increment its work-local
sequence; the review's prior canonical head must match that publication's result. Publishing
the same claimed head again refuses. Exact replay of an already retained payload is idempotent
even after rejection. Later rejection preserves the historical publication projection.

This projection is a structural claim, **not verified human approval**. In particular, the
first publication's prior main, the result head, credential and challenge must still be checked
against native history and the cryptographically verified exact receipt. Imported claims and
actor signatures cannot establish those facts. No consumer may treat the policy map as main.

Local retention includes publication payloads and receipt objects, along with retained review
and graph objects. Result heads, bundle identities, credentials and challenges are not CAS
objects. Existing native collection refusal remains in force without complete verified history.

## Native commit and recovery still to implement

The intended durable commit point is one complete synchronized publication frame in the owning
authority journal, ordered alongside rejection. Before that frame, an intent or promoted receipt
is preparation only. A child journal acknowledgement must not create a second approval authority.
This increment defines and tests the record; it does not implement that writer or commit point.

The native transaction must collect human presence outside custody, then reacquire every required
root in deterministic order. It must reopen owner and destination histories, reconstruct the exact
bundle and closure, verify the trusted receipt and challenge, compare current main and every input
decision, and append only under that complete barrier. Receipt verification must derive the result
from the saved output's causal history; it cannot equate an operation digest with a main head.
The initial base must come from verified existing accepted history, never from missing trust.

Recovery must distinguish a partial frame from an acknowledged frame whose reply was lost. It must
recover the exact retained receipt and outcome without another append, including after a later
rejection. A changed ceremony or request must refuse. Native historical replay must validate the
recorded receipt and context without applying today's eligibility retroactively.

Ordinary native readers currently refuse this kind in enrolled history with an explicit receipt
verification error. Pending publication claims also refuse. No renderer, agent, CLI or approval
entry point is enabled. Required kind/schema refusal and forward-only index migration preserve the
older-reader boundary; fresh-process required-kind refusal is proved below; cached-writer and complete signed-publication
compatibility remain required.

Before enabling publication, integrate and test attached and managed approval, receipt recovery,
local/remote import, delegation/fork, grouped integration and restore. Prove both commit orders in
separate processes, replaced/missing roots, stale cached views, exact retry, every interrupted
append boundary, and receipt/content retention. Eligible-signed packaged approval and the complete
manual/harness/fleet/provider/host acceptance remain required.

## Evidence for this increment

The missing required-kind regression failed before implementation. The first focused run exposed
SQLite's old kind constraint; migration 5 resolves it. The populated migration test covers all seven
prior kinds, accepts publication after upgrade, refuses the next unknown code and preserves rows
on reopen. The initial reopen assertion incorrectly expected an automatically hydrated in-memory
index; the corrected test inspects persisted rows, while the separate store test verifies rebuild.

All 41 focused tests passed in 2.496s. They cover store framing, every partial frame prefix, SQL
rebuild/reopen, atomic malformed-field refusal, exact publication chain, reused challenge, both
rejection orders, revalidation, unrelated decisions, local retention and the native read fence.
Replacing exact decision matching with eligibility alone failed the stale-review regression in
0.033s. Production source was restored exactly. These are deterministic native tests using synthetic
receipt identities, not signed publication or a separate-process approval/rejection race. Full
repository validation, exact-head hosted checks and normal merge remain required.


The full `npm test` gate passed on `266d07b1c16a9c7541225b28c9483a235cf7e95d`: 3,963 native
tests in 322.149s (six slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44
real-daemon checks, plus repository/docs/license/storage/format/lint checks. The saved-review
prerequisite #344 merged at `09389e9e3a52bcbf93e3af72f38a1a7a9c4f925d`; the reconciliation
changed ancestry only, with the entire implementation tree preserved.

A fresh fixture passed in 37.900s. The preserved pre-publication executable at
`170518353d8901561ddd7dce5e6572d1f0ffacb0`, SHA-256
`97b60cc81cb51f1586819b3d26cdfc7e25588896ff05789bbebab84b7abea370`, first read its four versions.
After appending a correctly framed required kind 7, separate old processes refused both versions
and capture (exit 1, no acknowledgement), preserving all fixture file bytes and executable bytes
in 0.101s. The frame was generated and scanned by current mesh-store; its payload identity was
synthetic. This proves required-kind refusal, not a valid signed publication, cached writer or
GUI journey. Generator source, manifest, raw result, hashes and fixture are retained locally.
