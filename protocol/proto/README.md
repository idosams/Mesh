# protocol/proto

**Maturity: draft protocol projection.** These schemas do not by themselves establish a supported
network session. See [Protocol status](../README.md) and
[Project status](../../docs/project-status.md).

Protobuf schemas for the wire protocol.

Plan §8.1 splits the wire in two: **protobuf for the network, canonical CBOR for signed records.**
This directory is the network half.

## `mesh/v0/records.proto`

The protobuf projection of the canonical record types: one message per record, with the same fields
in the same order as [`../schemas/canonical-encoding-v0.json`](../schemas/canonical-encoding-v0.json).

The agreement between the two is machine-checked. `crates/mesh-types/tests/serialization-compat.rs`
parses this file and fails if any record message's field names or order drift from the canonical
schema, and fails if a message is added that no test knows about. Two descriptions of one record
that nothing compares are two descriptions that will disagree — a field added to the wire message
and not to the signed encoding is a field a peer sends, a verifier ignores, and nobody notices
until a signature covers less than it appears to.

**Protobuf is not the signed form.** Protobuf serialization is not deterministic: field ordering,
varint width and unknown-field retention are all encoder choices. A signature is made over the
`mesh-cbor/0` encoding and never over protobuf bytes. A peer that receives one of these messages
reconstructs the record and re-encodes it canonically before verifying anything.

## `mesh/v0/sync.proto`

The protobuf projection of the **CWP message set**: one message per CWP message, named after the
`SyncMessage` variant it projects, with the same fields in the same order as
[`../../docs/protocol.md`](../../docs/protocol.md) §7.3's table and as
[`../wire/v0/messages.json`](../wire/v0/messages.json). Plus one message per compound element those
messages carry — `HeadAdvertisement`, `SparseChangeSet`, `CarriedChangeSet`, `ChunkRequest`,
`ChunkPart`, `MerkleSummary`, `SummaryNode` — under the names `crates/mesh-sync-protocol` gives
them. Twenty-five messages in all.

That agreement is machine-checked too, in both directions.
`crates/mesh-sync-protocol/tests/proto_projection.rs` parses this file and fails if a projected
message's field names or order drift from the Rust definition it projects, if a message is declared
here that no expectation covers, or if a nineteenth `SyncMessage` variant is added with no
projection. It reads the Rust by source text because that crate declares no dependency; the
technique and its limits are the ones `crates/mesh-sync-protocol/tests/mesh_types_drift.rs`
describes.

**Protobuf is not the encoding CWP messages are defined in, and a peer re-encodes to `mesh-cbor/0`
before comparing bytes.** A CWP message is defined as a two-element array — the wire tag, then the
fields in order — in the same canonical encoding a signed record uses. Protobuf serialization is
not deterministic, so nothing in the protocol is decided on protobuf bytes: not a record
identifier, not a signature over a carried body, and not the equality of two encodings of one
message.

**No service, no RPC and no envelope.** The wire tag selects the message and the `mesh-cbor/0`
frame carries it; a protobuf `service` would be a second message-selection mechanism and a `oneof`
envelope a second spelling of the tag table. The three alternatives and why each was rejected are
in the file's header comment; the ADR that records the decision in the shape
`docs/adr/0000-template.md` fixes is owed by task `01KZGA1KHXE7D9W6DC9MK0D01K`, because
`docs/adr/**` is held by four in-progress tasks and `docs/adr/README.md` says to escalate rather
than write into a claimed directory.

### One identifier here cannot be recomputed from published material

`CarriedChangeSet.id` is a ChangeSet's record identifier, and
[ADR-0015](../../docs/adr/0015-actor-heads-converge-without-a-global-lock-if-an-identifier-binds-its-parent-set.md)
requires a receiver to recompute it from `body` before that identifier may reach head advancement.
An implementation built from this directory alone **cannot do that today**:
[`../../docs/protocol.md`](../../docs/protocol.md) §2.1 says a content-derived name is computed
under the canonical encoding, §3.10's `DigestWriter` row says the identity framing is *not* the
canonical encoding, and `derive_id` uses the framing. So `canonical_encoding_digest_hex` is
recomputable and `record_id_hex` is not — the same warning
[`../test-vectors/README.md`](../test-vectors/README.md) and [`../README.md`](../README.md) §6.1
carry. It is an open question tracked as `01KZCZDTVD0D36W5YRGX8CNE17`, not an omission in this
file, and it is stated here rather than left for an implementer to hit.

## Find the tasks that fill this directory

```bash
See the public GitHub issue tracker
```
