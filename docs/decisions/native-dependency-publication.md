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


## Exact historical receipt inspection (R166)

The native-only `inspect_saved_dependency_review_receipt` operation resolves the registered work,
owning authority, exact saved-review binding and complete retained inputs under the existing bounded
custody context. It reconstructs the review bundle and resulting head from native saved history,
selects a configured trusted human credential, and verifies the canonical P-256 approval receipt
against that independently reconstructed context. The receipt cannot select the work, snapshot,
base, validation digest or resulting head. Empty or oversized receipts (over 65,536 bytes), empty
challenges, untrusted credentials, rejection decisions, invalid signatures and mismatched contexts
refuse. Missing or substituted native evidence also refuses through complete context verification.

The returned receipt digest identifies the supplied bytes; inspection does not store them, append
a record, reserve a challenge, advance main or collect human presence. The response explicitly
reports `publication_committed: false` and `approval_authority: false`. Current input eligibility
and exact historical decision freshness are separate fields. A valid historical receipt remains
cryptographically verifiable after rejection, later private progress or revalidation; none of
those inspections authorizes a new publication. Exact inspection may repeat without a write.

The shared native checker will be reused by publication/replay under complete custody. That later
path must additionally verify current main, the exact publication sequence, receipt/object and
claim-field equality, challenge admission, and the durable commit/recovery boundary. The required
publication-record reader fence stays intact. No renderer, agent or CLI route is enabled here.

The real consumed-history test now creates P-256 signatures for exact and different snapshots,
checks malformed/oversized/corrupted/untrusted/rejection/empty-challenge refusal, and verifies
unchanged owner and child journals. Its first successful run passed in 27.458s. Replacing native
expected context with the receipt's carried context failed the intended wrong-snapshot assertion
in 12.946s; production source was restored byte-for-byte. These signatures use test-generated
credentials, not OS human-presence or eligible-signed packaged proof. Additional missing-root and
substituted-graph assertions are included in the full gate. Full validation and delivery remain
required before this increment is considered merged.


R166 full `npm test` passed on the tree recorded as `a93d3765a639bfc8fa428c4c6838861d66121704`: 3,963 native tests in
338.159s (six slow, 18 skipped), 194 rendered tests, 672 desktop tests and all 44 real-daemon
checks, with repository/docs/license/storage/format/lint gates. The expanded consumed receipt
journey, including missing-root and substituted-graph refusals, passed in 41.930s inside that run.
PR #346 merged at `ebee3f979ef7d6400908759de27c4c088a645d5b`; incorporating its merge changed
ancestry only. The receipt signatures are test-generated and genesis-based, not platform
human-presence or subsequent-main approval proof. Hosted validation and normal merge remain required.


The next replay increment must explicitly resolve native canonical ancestry. Current
`review_target_for_head` searches ordinary child-journal reviews, whereas native reviews are
owner-held. Replaying trusted receipts must build a sealed per-work/installation mapping from
verified main head to exact saved operation, then supply it to native review reconstruction and
new base selection. Default-trust native history opens cannot establish accepted main. Removing
the publication read fence or manufacturing an ordinary child review is not that integration.
Second-review/second-approval proof against the first native accepted main remains required.


## Native publication origin (R169)

The first native publication claim for an exact work/installation must bind a review based on
genesis. Later claims must bind the preceding native publication result. Previously the structural
policy verifier checked the latter case but accepted an unexplained nonzero base when there was
no preceding native publication. It now refuses that unsupported history before changing any
request, challenge, publication or retained-object state. A valid genesis request may still use
the same request/challenge after refusal. This does not treat absence of configured reviewer trust
as genesis: native cryptographic replay and ordinary workspace admission remain separately fenced.

This is a stricter semantic check within required kind 7 / policy v5; the byte format is unchanged.
No native publication writer is enabled. There is no implicit migration from ordinary or imported
accepted main into the native chain; such a bridge needs independently verified authority and an
explicit contract before it can be supported. Existing ordinary approval behavior is unchanged.

The new regression failed before the fix because policy replay returned success. After the fix,
all 21 dependency-policy tests passed in 2.41s, including valid genesis/later publications,
rejection/revalidation, reused challenges, atomic refusal and idempotent historical replay.
Full validation, hosted checks and delivery remain pending. Issue #345 remains open.

R169 full `npm test` passed: 3,964 native tests in 321.553s (six slow, 18 skipped),
194 rendered tests, 672 desktop tests and all 44 real-daemon checks, plus repository,
documentation, license, storage, formatting and lint gates. Hosted checks and merge remain pending.


## Private evidence and ordinary admission (R170)

Native enrollment/completed-consumption verification now produces a separate private internal
evidence type. Ordinary workspace admission is a fallible conversion that refuses both durable
publication claims and pending publication frames. Consumed history and independent history use
the same conversion. Saved operation/content verification still occurs during read-only workspace
opening; private evidence does not by itself prove human authority, verified main or complete content.

The existing early publication reader fence remains. This refactor does not yet add a private
workspace adapter, cryptographic publication replay, second-main reconstruction, a publication
writer or a new external API. It creates the explicit admission boundary those integrations need.
Eight read-boundary tests plus the real consumed-lane desktop journey passed (nine tests, 42.669s).
Removing the conversion check caused both new refusal tests to fail; the source was restored exactly.
The initial fixture omitted required custody and failed; it was corrected to hold the real lock.
Full validation and delivery remain required.

R170 full `npm test` passed: 3,966 native tests in 318.200s (six slow, 18 skipped),
194 rendered tests, 672 desktop tests and all 44 real-daemon checks, plus repository,
docs, license, storage, formatting and lint gates. Hosted checks and merged delivery remain required.
