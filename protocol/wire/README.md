# protocol/wire

**Maturity: draft message contract.** The codec exists, but there is no supported live peer or
relay-backed user journey. See [Protocol status](../README.md) and
[Project status](../../docs/project-status.md).

The CWP message set, published so a client can be written against it without reading any Rust.

Records are one half of the protocol and messages are the other. Read
[`../test-vectors/README.md`](../test-vectors/README.md) first: messages are encoded in the same
`mesh-cbor/0` profile signed records are, so one decoder serves both, and that page is where the
profile is specified.

## `v0/messages.json`

Every message: its wire tag, its plane, its fields in encoding order, the preconditions both sides
check, and **one complete example with its exact bytes**.

The file also carries the three planes, the presence states with their tags, and the fourteen error
codes with their tags and whether each is retryable.

Field shapes use the same vocabulary as
[`../schemas/canonical-encoding-v0.json`](../schemas/canonical-encoding-v0.json) — `unsigned`,
`bool`, `bytes` (with an optional `byte_length`), `text`, `sequence` (with an `element`) and `group`
(with `fields`) — so one schema-driven encoder handles records and messages alike.

## The frame

A message is a **two-element array**:

* element 0 is the **wire tag**, an unsigned integer;
* element 1 is the **body**, itself an array holding that message's fields, in the order
  `messages[].fields` lists them.

So the smallest message in the set, `ACK_CHUNKS` with one verified chunk, is:

```text
82                      array(2)      the frame
  0a                    unsigned(10)  the wire tag: ACK_CHUNKS
  81                    array(1)      the body: one field
    81                  array(1)      verified: one element
      58 20 bbbb…       bytes(32)     a content hash
```

which is `ACK_CHUNKS`'s published example, byte for byte.

A decoder reads the tag, and from the tag it knows the arity the body must have. A tag this version
does not define is a **refusal** — `unknown message` — and never a guess: that is what makes a newer
peer's message something you decline cleanly rather than misparse.

## Rules that are easy to miss

- **Shortest head, everywhere.** Same rule as records. `5` is `0x05`, never `0x18 0x05`. A decoder
  that accepts the longer spelling will accept bytes no conformant encoder produces, and the
  published examples stop pinning anything.
- **No trailing bytes.** A complete message followed by anything at all is a decode failure, not a
  message plus noise.
- **Identifiers are exactly thirty-two bytes.** Any other width is a decode failure rather than a
  shorter identifier.
- **An absent value is an empty sequence.** The profile has no absence marker at all. The one
  optional field in the set — `ADVERTISE_FRONTIER`'s `canonical_head` — is a sequence of at most one
  element: `[]` when absent, `[head]` when present.
- **Messages carry no signature.** Canonicity here is a *conformance* requirement, not a security
  one: one message must have exactly one encoding, or these examples would pin one encoder's habits
  instead of the format.
- **Records this protocol does not own travel opaque.** A ChangeSet, a review bundle, a validation
  receipt and an approval envelope are carried as their `mesh-cbor/0` bytes plus their identifier,
  never as a second set of fields — their signatures were made over those bytes. The fields
  `OPERATIONS_BATCH` does spell out beside each body are the ones a receiver needs to plan its next
  request before it has decoded anything, and every one is redundant with the body it travels with.
  Re-derive them from the body before believing them.

## The three planes

`handshake`, `metadata` and `content`. The separation buys one property, and it is the reason to
keep it: **a peer learns that an object changed before the bytes of that object arrive.** Only
`REQUEST_CHUNKS`, `CHUNK_BATCH` and `ACK_CHUNKS` are on the content plane, so nothing that carries
visibility can be queued behind chunk bytes.

`HELLO`, `AUTHENTICATE` and `ERROR` are on neither of those two: they are `handshake`. Filing them
under `metadata` would put session establishment inside the measurement the metadata-plane latency
budget is about.

## Error handling

`error_codes` in `v0/messages.json` is closed. A sender decides what to do from the **code alone**;
the `detail` string is diagnostic and no peer parses it. Only `busy` and `unknown policy epoch` are
retryable — every other code says the message itself is wrong, and resending it unchanged is a loop.

## How to use the examples

For each entry in `messages`:

1. Build the message from `vector.value`, decoding the hex byte strings.
2. Encode it and assert your bytes equal `vector.bytes_hex` and their count equals
   `vector.byte_length`.
3. Decode `vector.bytes_hex` and assert you get `vector.value` back.
4. Assert your decoder **rejects** a copy in which the wire tag's head is widened by one byte
   (`0x0a` becomes `0x18 0x0a`).

Steps 3 and 4 are the ones that catch an implementation that agrees with itself and with nobody
else. `../verify-published.mjs` runs all four against every message, from a second `mesh-cbor/0`
implementation written against this page.

## Where these bytes come from, and the one caveat

Unlike [`../schemas/`](../schemas/README.md) and [`../test-vectors/`](../test-vectors/README.md),
this file is **not** generated by the implementation. Its bytes are a mirror of the table the
implementation pins in `crates/mesh-sync-protocol/tests/protocol-vectors.rs`, and
`../verify-published.mjs` check PV-2 fails if the mirror and the pin ever disagree. The reason for
the mirror, the alternatives weighed, and what would replace it are in
[`../README.md`](../README.md) §6.4.

## Not here

- **A transport.** Nothing in this directory says how these bytes reach a peer — no framing over a
  socket, no session resumption, no relay protocol. `mesh-sync-engine` exists, but no supported
  live peer or relay-backed user session is published.
- **Interoperable live authentication.** Mesh produces signatures for local changes and approvals,
  but that does not supply the key-custody and peer-session contract needed to treat
  `AUTHENTICATE` as a supported user path. [`../README.md`](../README.md) §6.2.
- **The knowledge model as a schema.** What each peer tracks about every other peer is specified in
  [`../../docs/protocol.md`](../../docs/protocol.md) §7.3; it is a local data structure rather than a
  message, so it has no wire encoding to publish.
