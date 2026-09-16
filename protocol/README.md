# CWP — the Causal Workspace Protocol, draft `v0`

This directory is the **published protocol**: everything an outside party needs to build a client
that interoperates with Mesh, with no access to the Rust source. If you find something here you
cannot implement without reading Rust, that is a specification bug — file it rather than reading
the Rust.

**Draft status.** `v0` is a draft. It is complete enough to encode the published records, frame
messages, and check bytes against two conformance clients. Repository implementations have moved
ahead of the published schemas in signing, operations, review, and deterministic synchronization;
there is still no supported peer session or hosted relay journey. §4 separates those facts. See
[Project status](../docs/project-status.md) for the broader product boundary.

---

## 1. What is here

| Path | What it holds |
|---|---|
| [`schemas/canonical-encoding-v0.json`](schemas/canonical-encoding-v0.json) | The machine-readable schema of the canonical encoding: the `mesh-cbor/0` profile, and every record type's domain tag and ordered field list. Generated from the encoder. |
| [`test-vectors/`](test-vectors/README.md) | The canonical encoding in prose, and the published input/output pairs for every record type. Generated from the encoder. |
| [`wire/`](wire/README.md) | The CWP message set: the frame, the three planes, every message's fields in encoding order, the error codes, and one complete example of each message with its exact bytes. |
| [`proto/`](proto/README.md) | The protobuf projection of the record types and of the CWP message set, for network transport. Never the signed form, and never the encoding a message is defined in. |
| [`conformance/`](conformance/README.md) | Executable, process-isolated conformance suite with reference and independent Python clients. |
| [`VERSIONING.md`](VERSIONING.md) | The versioning and compatibility policy: every version identifier, and what counts as a breaking change. |
| [`verify-published.mjs`](verify-published.mjs) | The checker that holds this directory against the implementation and against itself. §5. |

The normative vocabulary — the four graphs, the terminology register, the invariants each graph
carries, and the wire message table — is [`../docs/protocol.md`](../docs/protocol.md). Terms are
defined there and **nowhere else**, including here: this file cites that register rather than
restating it, because a restatement is a second definition and second definitions are what a
protocol specification exists to prevent.

## 2. Reading order

1. [`test-vectors/README.md`](test-vectors/README.md) — the `mesh-cbor/0` profile, complete, in one
   page. Start here whatever you are building; both the records and the messages are in this
   profile.
2. [`schemas/canonical-encoding-v0.json`](schemas/canonical-encoding-v0.json) — the same profile
   plus every record's field order, in a form a code generator can consume.
3. Encode one record type and compare against `canonical_encoding_hex`. Then decode, then re-encode.
4. [`wire/README.md`](wire/README.md) and [`wire/v0/messages.json`](wire/v0/messages.json) — the
   message set, once records encode.
5. [`../docs/protocol.md`](../docs/protocol.md) §2 for the invariants your client must not break,
   and §3 for what every word means.

## 3. Coverage map

Every signed record type and every message, with where its shape is specified and where its bytes
are pinned. `verify-published.mjs` check **PV-7** fails if a record type or a message is missing
from this table, which is what stops the table from silently falling behind the protocol.

### 3.1 Record types

| Domain tag | Signed | Schema | Vectors | Implemented in |
|---|---|---|---|---|
| `mesh.v0.file-manifest` | yes | `schemas/canonical-encoding-v0.json` | `test-vectors/v0/file-manifest.json` | `mesh-types` |
| `mesh.v0.file-version` | yes | `schemas/canonical-encoding-v0.json` | `test-vectors/v0/file-version.json` | `mesh-types` |
| `mesh.v0.directory-version` | yes | `schemas/canonical-encoding-v0.json` | `test-vectors/v0/directory-version.json` | `mesh-types` |
| `mesh.v0.changeset` | yes | `schemas/canonical-encoding-v0.json` | `test-vectors/v0/changeset.json` | `mesh-types` |
| `mesh.v0.empty-operation` | no | `schemas/canonical-encoding-v0.json` | `test-vectors/v0/empty-operation.json` | `mesh-types` — the published placeholder; the repository's implemented operation vocabulary is not yet published here (§6.3) |

### 3.2 Messages

All eighteen are specified in [`../docs/protocol.md`](../docs/protocol.md) §7.3 and published, with
their fields in encoding order and one example each, in
[`wire/v0/messages.json`](wire/v0/messages.json).

| Tag | Message | Plane |
|---|---|---|
| 1 | `HELLO` | handshake |
| 2 | `AUTHENTICATE` | handshake |
| 3 | `ADVERTISE_FRONTIER` | metadata |
| 4 | `REQUEST_OPERATIONS` | metadata |
| 5 | `OPERATIONS_BATCH` | metadata |
| 6 | `ACK_OPERATIONS` | metadata |
| 7 | `ADVERTISE_MANIFESTS` | metadata |
| 8 | `REQUEST_CHUNKS` | content |
| 9 | `CHUNK_BATCH` | content |
| 10 | `ACK_CHUNKS` | content |
| 11 | `UPDATE_ACTOR_HEAD` | metadata |
| 12 | `UPDATE_CANONICAL_HEAD` | metadata |
| 13 | `PRESENCE` | metadata |
| 14 | `REVIEW_BUNDLE` | metadata |
| 15 | `VALIDATION_RECEIPT` | metadata |
| 16 | `APPROVAL_ENVELOPE` | metadata |
| 17 | `ANTI_ENTROPY_SUMMARY` | metadata |
| 18 | `ERROR` | handshake |

### 3.3 The publication rules

Publication is the one path by which the protected shared version advances, and it is the part of
CWP a client is most likely to get subtly wrong. The rules are **specified in
[`../docs/protocol.md`](../docs/protocol.md) §2.4** as invariants TG-1 … TG-10; this section is the
sequence they apply to, so an implementer meets them in the order a real publication meets them.
Each step names the invariant that governs it rather than restating it.

| Step | What happens | Governed by |
|---|---|---|
| 1 | An actor's work reaches an actor head. Every authority-bearing action along the way needed an unexpired capability valid in the current policy epoch. | TG-1, TG-2 |
| 2 | A review bundle is computed deterministically from that exact actor head. Later work by the actor cannot enter a bundle that already exists. | TG-4's binding, and the `review bundle` definition |
| 3 | Validation runs execute against the bundle's review snapshot. A result produced by the actor that produced the change is agent-reported evidence and never satisfies a requirement on its own. | TG-5 |
| 4 | A human — and only an actor of kind `human` — signs an approval envelope binding nine fields: the workspace, the expected canonical head, the reviewed actor head, the review bundle, the selected changes, the conflict resolutions, the validation digest, the policy epoch and the approving actor. | TG-3, TG-4, §2.4's *bound field count* |
| 5 | The transition is admitted only if the expected canonical head still equals the current canonical head. Of N concurrent attempts against one expected head, exactly one succeeds; the rest fail as stale and name the current head. | TG-9 |
| 6 | A failed swap is routed to reclassification, which either rebases under a rule that provably touches no reviewed byte or returns the work for re-review. It never resolves silently. | TG-9, and the `reclassification` definition |
| 7 | A publication receipt is emitted, verifiable from the public key and this specification alone — without Mesh's own code path. | TG-6 |
| 8 | The envelope is spent. A replayed envelope is rejected. | TG-10 |

Two rules bracket the whole sequence: a hard guard refuses publication regardless of human intent
and cannot be disabled by configuration (TG-8), and policy epochs are strictly increasing so that
revocation does not depend on any peer being online (TG-7).

The repository now implements a bounded local form of steps 2–5 and 7–8 through `mesh-approval`,
`meshd`, and `meshctl`. That local path is not yet fully represented by published schemas and
vectors, and reclassification in step 6 remains outside the demonstrated journey. Treat the table
as the target protocol sequence and §4 as the precise maturity boundary.

## 4. What is implemented, and what is only specified

A documented capability that does not exist is a defect, so the two are separated here rather than
blended.

| Part | Status |
|---|---|
| The `mesh-cbor/0` profile, encoder and decoder | Implemented, `mesh-types`. Vectors published and compared to the generator in both directions on every test run. |
| The five record types in §3.1 | Implemented, `mesh-types`. |
| The eighteen messages: encoding, decoding, per-message preconditions, error codes, knowledge sets, replication gaps, Merkle summaries | Implemented, `mesh-sync-protocol`. |
| Synchronization transport | `mesh-sync-engine` implements inbound identifier validation, persisted operation and receipt handling, and sender-side durable delivery acknowledgement. The crate describes itself as mostly a placeholder: live transport, peer sessions, and a relay-backed user journey are not implemented. The published message set remains broader than the demonstrated product path. |
| Producing a signature | Real Ed25519 signing exists for local authenticated changes and `meshctl` approval. The desktop demonstration uses software custody; operating-system-backed human custody and live peer authentication are not complete. |
| Review bundles and approval receipts | `mesh-approval` and `meshd` implement exact local review, signature verification, durable receipt handling, and protected-state advancement. These shapes are not all published as protocol record schemas and vectors, so outside interoperability remains incomplete. |
| The eighteen-operation vocabulary | Implemented in `mesh-operations`. `mesh.v0.empty-operation` remains the published placeholder; the operations' external field schemas and vectors are not published yet. |
| The conflict rules | `mesh-conflicts` implements and tests the eleven-row resolver, including preservation and determinism campaigns. The normative external table in `docs/protocol.md` §7.2 is still reserved and unpublished, so those internal rules are not yet a stable interoperability contract. |
| A conformance suite | Implemented in [`conformance/`](conformance/README.md), with a process boundary, mutation self-tests, and two independent clients. Unsupported cases expose gaps in the published material. |

## 5. Checking your implementation

Two things to run, and one claim about a third.

**Against the vectors.** `test-vectors/README.md` §*How to use these vectors* gives the six steps:
encode, compare bytes, compare length, compare the encoding digest, decode back, and assert your
decoder **rejects** a widened integer head. Steps 5 and 6 are what stop an implementation from
being one-directionally lucky.

**Against this directory.** From the repository root:

```console
$ node protocol/verify-published.mjs
```

It holds the published material against the implementation and against itself: every message the
implementation declares is published with the same tag and plane (PV-1); every published example's
bytes equal the bytes the implementation pins (PV-2); every published example re-encodes from its
published field values, through a second `mesh-cbor/0` encoder written in this file against the
published profile, to exactly those bytes, and decodes back to those values, and that decoder
refuses a widened head (PV-3); the error codes agree (PV-4); every record type has vectors and is
indexed (PV-5); every published vector re-encodes through the same second encoder (PV-6); this
coverage map is complete (PV-7); and the versioning policy names every version identifier in the
tree (PV-8). `node protocol/verify-published.mjs --self-test` breaks each check on purpose and
asserts it fires — a check that cannot fail proves nothing by passing.

Two limits, stated rather than left to be discovered. The checker is **not** wired into `npm test`:
wiring it there means editing `package.json`, which is outside the allowed paths of the task that
wrote it, so today it is run deliberately. And it does not verify `canonical_encoding_digest_hex`,
because that needs BLAKE3 and this checker carries no dependencies; use any BLAKE3 library for that
step.

**Evidence that this is implementable from the published material.** A second CBOR decoder *and*
encoder were written in Python from the profile page and the vectors alone — no Rust — and agreed
with all ten published canonical vectors. That is recorded in
[ADR-0007](../docs/adr/0007-encode-signed-records-as-fixed-order-cbor-arrays.md#consequences). The
Python artifact is not in this tree, so the claim is a recorded result and not something you can
re-run here; PV-3 and PV-6 are the re-runnable version of the same idea, in JavaScript, over both
the records and the messages.

## 6. Open questions

These are unresolved. They are listed because an implementer meets them anyway, and meeting them in
a document is cheaper than meeting them in a verification failure.

### 6.1 `record_id_hex` is settled in the specification and not yet in the implementation

**The rule is decided.** A content-derived name is BLAKE3 of exactly one byte string, and for a
record that byte string is its canonical encoding — the bytes in `canonical_encoding_hex`. So a
record's name is `canonical_encoding_digest_hex`, and once your encoder agrees with the vectors you
can name every record you encode, with nothing further to implement. A **chunk** has no schema and
is named by BLAKE3 of its own content directly, which is what keeps a chunk digest computable by any
BLAKE3 tool. Those are the only two rules.

This replaced a genuine contradiction:
[`../docs/protocol.md`](../docs/protocol.md) §2.1 said a name comes from the canonical encoding while
§3.10's `DigestWriter` row said the identity framing is **not** the canonical encoding, and the
derivation function used the framing.
[`../docs/adr/0033-name-an-immutable-record-by-the-digest-of-its-canonical-encoding.md`](../docs/adr/0033-name-an-immutable-record-by-the-digest-of-its-canonical-encoding.md)
ruled for the canonical encoding, retired the framing, and reclassified a chunk's name as a content
digest rather than a record identifier.

**What is still open is the implementation, and it is open in a way you can work around exactly.**
Until `01KZFMZC4MTHTT3BW4Y0BW6NYA` lands, the `record_id_hex` values published under
`test-vectors/v0/` are still the retired framing's output, so they do **not** equal
`canonical_encoding_digest_hex`. Use `canonical_encoding_digest_hex` as the name and do not assert
against `record_id_hex` until then; the two become equal in that change, and no byte of
`canonical_encoding_hex` moves when they do. Two further things move with it and are named here so
you can see them coming: an actor key gains a one-field record schema and a vector file, so an
`ActorId` becomes computable from this material for the first time, and every existing record
identifier changes value once.

### 6.2 Live peer authentication is not an interoperable user path

Mesh can produce and verify signatures for local changes and approvals. That does not make a live
CWP session available: there is no supported peer journey with published key custody,
authentication, and relay behavior. Do not build on the assumption that an identity in `HELLO` is
verified merely because local signing exists.

### 6.3 The implemented operation vocabulary is not published

`mesh.v0.changeset` pins how a nested operation encoding is spliced into the `operations` sequence,
so the container is settled. `mesh-operations` implements eighteen operations, but their external
fields, preconditions, state effects, schemas, and vectors are not published under `protocol/`.
An outside client can carry and re-encode the published ChangeSet container; it cannot derive the
meaningful operation shapes from the public artifacts alone.

### 6.4 The wire examples are a mirror, not a generated corpus

`wire/v0/messages.json` is not produced by the implementation the way `schemas/` and
`test-vectors/` are. Its bytes are copied from the table the implementation pins in
`crates/mesh-sync-protocol/tests/protocol-vectors.rs`, and PV-2 fails if the copy and the pin ever
disagree.

*Context.* The generator that owns `schemas/` and `test-vectors/` lives in `mesh-types`, and
`mesh-types` deliberately depends on nothing — extending it to emit message vectors would give the
record layer a dependency on the message layer, which is the edge the crate graph exists to
prevent. The alternative generator would live in `mesh-sync-protocol`, and adding one was outside
the allowed paths of the task that published this directory.

*Alternatives considered.* **Leave the bytes in the Rust test** — rejected: it makes the message
set unimplementable without reading Rust, which is the whole point of publishing. **Generate from
`mesh-types`** — rejected: it inverts the dependency. **Generate from `mesh-sync-protocol` into
`protocol/wire/`** — the right answer, and the one this should become; it was out of scope here.
**Publish a mirror with a drift check** — chosen, because the failure mode that matters is silent
disagreement, and PV-2 makes disagreement loud.

*Consequences.* One more place a message's bytes are written down, and a check that must be run for
the mirror to be trustworthy — and the checker is not wired into `npm test` (§5), so today the
mirror is trustworthy exactly as often as somebody runs it. That is the cost of the choice, stated
rather than hidden.

*What would change our mind.* A generator in `mesh-sync-protocol` that emits `wire/v0/messages.json`
and is compared byte-for-byte on every test run, exactly as `mesh-types` does for `schemas/` and
`test-vectors/`. At that point the mirror stops being a mirror, PV-2 becomes redundant, and this
open question closes.

---

## 7. Reporting a gap

A gap between what this directory claims and what an implementation can do is a specification bug.
File it — in this repository it is a `kind: bug` task — rather than resolving it in a conversation.
A gap explained in a support thread is a gap the next implementer meets again.
