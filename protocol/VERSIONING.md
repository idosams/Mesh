# CWP versioning and compatibility

**Maturity: normative policy for the draft `v0` artifacts.** It is not a promise that a stable
release or live peer session exists. See [Protocol status](README.md) and
[Project status](../docs/project-status.md).

What is versioned, what a change to each thing costs, and — the part that matters to anyone
building against this — **what counts as a breaking change**.

The rule the whole policy comes from: **a signature was made over exact bytes.** Anything that
changes which bytes a record encodes to, or which bytes an identifier is derived from, invalidates
every signature already made — silently, as a verification error, long after the record was
written. So the encoding is versioned in more places than a reader might expect, and none of those
versions is negotiated.

## 1. The version identifiers

There are six, and they move independently. A change to one is never an edit to another.

| Identifier | Current value | What it versions | Where it is declared |
|---|---|---|---|
| Record encoding profile | `mesh-cbor/0` | The CBOR subset every signed record and every message is encoded in: which shapes are admitted, and the shortest-head rule. | `mesh-types` (`CBOR_PROFILE`), mirrored as a string by `mesh-sync-protocol` (`RECORD_ENCODING_PROFILE`) and named in `HELLO` |
| Schema vocabulary | `mesh-record-schema/0` | The vocabulary a published schema is written in — the field-type names, not the fields. | `mesh-types` (`SCHEMA_FORMAT`), carried by `schemas/canonical-encoding-v0.json` |
| Vector file format | `mesh-canonical-vectors/0` | The shape of a published vector file: which keys each vector carries and what they mean. | `mesh-types` (`VECTOR_FORMAT`), carried by every file under `test-vectors/v0/` |
| Domain tags | `mesh.v0.…` | One per record type — the first element of every record encoding. A domain tag is what stops two record types with the same field shapes from producing the same bytes. | `schemas/canonical-encoding-v0.json`; also `mesh.v0.anti-entropy-summary`, the domain a Merkle summary absorbs first, declared by `mesh-sync-protocol` (`SUMMARY_DOMAIN`) |
| Message framing | `mesh-cwp-wire/0` | How a message is framed: the two-element array, the tag, the field order per message. Versioned separately from the profile it is built on. | `mesh-sync-protocol` (`WIRE_FORMAT`), carried by `wire/v0/messages.json` |
| Protocol version | `0` | The message set this build speaks: which tags are defined and what each one means. | `mesh-sync-protocol` (`PROTOCOL_VERSION`), carried in `HELLO` |

`verify-published.mjs` check **PV-8** fails if this table stops naming one of them.

**There is no negotiation.** A `HELLO` naming a protocol version that is not this one, or a record
encoding profile that is not this one, is answered `unsupported version` and the session ends. One
version exists; pretending to negotiate before a second one exists would be untested code on the
security boundary.

## 2. What is a breaking change

A change is **breaking** if an implementation that was conformant before it stops being conformant
after it, *or* if a record signed before it stops verifying after it. Both halves matter: the second
is the one that fails silently.

### 2.1 Breaking — the encoding

Each of these changes the bytes a record encodes to. Every one is a new version of the thing it
touches, and the old published bytes stay published.

- Adding, removing or renaming a field of an existing record type. The field order **is** the
  format; there is no absence marker in the profile, so there is no compatible way to add a field.
- Changing a field's type or its declared width.
- Changing the order of any record's fields.
- Changing what a domain tag covers. A domain tag is never edited — a new meaning is a new tag.
- Admitting a shape the profile excludes (a map, a tag, a float, a negative integer, an
  indefinite-length item, a further simple value), or excluding one it admits.
- Relaxing the shortest-head rule, or any other rule that would give one value two spellings.
- Changing the ordering rule for a sequence that models a keyed collection.
- Changing how a record identifier is derived — including resolving the open question in
  [`README.md`](README.md) §6.1, which moves every record identifier if it resolves onto the
  canonical encoding.

### 2.2 Breaking — the message set

- Adding, removing or reordering a message's fields.
- Changing a message's plane. A plane is a delivery guarantee: moving a message onto the content
  plane puts it behind chunk bytes, and moving one off it changes what a peer may assume about
  ordering.
- Reusing or renumbering a wire tag. A tag is assigned once. A tag this version does not define is
  answered `unknown message` and never guessed, which is what makes a *newer* peer's message a clean
  refusal rather than a misparse.
- Removing an error code, or changing what one means, or changing whether it is retryable — a
  sender decides what to do from the code alone.
- Changing a precondition from something a receiver accepts to something it refuses.
- Lowering `max_operations_per_batch` or `max_chunk_part_bytes`: a peer that was sending a legal
  batch starts being refused.

### 2.3 Breaking — the protocol rules

- Changing the set of fields an approval envelope binds. Nine fields are bound today
  ([`../docs/protocol.md`](../docs/protocol.md) §2.4); a tenth or an eighth is a different signature
  over a different statement.
- Weakening any invariant in [`../docs/protocol.md`](../docs/protocol.md) §2 — a peer that relied on
  it is now wrong about the history it holds.
- Changing which actor kinds may hold which capabilities.

### 2.4 Not breaking

- Adding a **new** record type with a **new** domain tag, and publishing its vectors.
- Adding a **new** message with an unused wire tag. An older peer answers `unknown message`, which
  is a defined outcome rather than a failure.
- Adding a **new** error code. A sender that does not know it treats it as non-retryable, which is
  the conservative default: it never turns a permanent refusal into a retry loop.
- Raising `max_operations_per_batch` or `max_chunk_part_bytes`.
- Any change to prose, to a `note` field, to a summary or to a vector's `description` — none of them
  is read by a decoder.
- Adding a vector to an existing record type's file. Vectors are additive within a format-version
  directory; an existing vector is never edited in place.

## 3. What a breaking change costs

A breaking change to the encoding is a **compatibility event**, and it lands as one change carrying
all of:

1. A new format-version directory. New bytes are published under `v1/`, and `v0/` stays exactly as
   it is — signatures were made over the bytes it holds, and deleting them would make already-signed
   records unverifiable.
2. New vectors for every affected record type, generated rather than written.
3. A compatibility test that reads the old directory and asserts the old bytes still decode under
   the old version.
4. The document change in the same change as the behaviour change, so a reader never meets one
   without the other.
5. Protocol review. Changing a definition in [`../docs/protocol.md`](../docs/protocol.md) is a
   protocol change under its §8, not an edit.

A breaking change to the message set adds a new `PROTOCOL_VERSION` and leaves the old one refusable
by name.

## 4. Deprecation

A record type, a message or an error code is never deleted from a published version. It is
deprecated in place: it keeps its tag, keeps its vectors, and gains a note saying what replaced it.
Its tag is never reused for anything else, in this version or in any later one — a reused tag makes
an old peer decode a new message into the wrong shape, which is worse than refusing it.

Retiring a *word* is different from retiring a *format*: the register in
[`../docs/protocol.md`](../docs/protocol.md) §4 keeps an alias row for a word that has been renamed,
so an external source using the old word still resolves.

## 5. How to tell whether your client is still conformant

Re-run the six steps in [`test-vectors/README.md`](test-vectors/README.md) against the newest
format-version directory, and `node protocol/verify-published.mjs` in a checkout at the revision you
are targeting. If the format-version directory you built against still exists, your client is still
conformant *for that version* — that is what keeping the old directory is for.
