# protocol/test-vectors

**Maturity: normative draft vectors for the published subset.** Passing them is not proof of a
supported live session. See [Protocol status](../README.md) and
[Project status](../../docs/project-status.md).

Published test vectors for the Mesh **canonical encoding** — the byte string a signed record *is*.

Everything you need to write a conformant encoder is in this directory and in
[`../schemas/canonical-encoding-v0.json`](../schemas/canonical-encoding-v0.json). You do not need
to read any Rust, and nothing here refers you to any.

## Why this exists

A signature is made over bytes. If two implementations serialize the same record differently, one
of them produces a signature the other cannot verify — and it fails silently, as a verification
error, long after the record was written. The only defence is a serialization with exactly one
possible output per record, and the only evidence that your implementation produces it is a
published input/output pair you can check yourself.

That is what these files are.

## Start here

1. Read [`v0/index.json`](v0/index.json). It lists every record type, the file holding its vectors,
   and where the schemas are.
2. Read the profile section of
   [`../schemas/canonical-encoding-v0.json`](../schemas/canonical-encoding-v0.json). It is one page
   and it is the whole format.
3. Implement the encoder against one record type, and compare your bytes to
   `canonical_encoding_hex` in that record's file.

## The encoding, complete

### The profile: `mesh-cbor/0`

A restricted subset of [CBOR (RFC 8949)](https://www.rfc-editor.org/rfc/rfc8949). If you have a
CBOR library, its *encoder* will probably produce these bytes once you make it use definite
lengths and shortest-form heads; the profile is small enough to write from scratch in an afternoon
either way.

Every item begins with a head byte: the **major type** in the top three bits, and a five-bit
**additional information** value in the low five. When the value being encoded is below 24 it rides
in those five bits; otherwise the low five bits are 24, 25, 26 or 27 and the value follows as 1, 2,
4 or 8 **big-endian** bytes.

**The head must always be the shortest one that holds the value.** `5` is `0x05`, never
`0x18 0x05`. `256` is `0x19 0x01 0x00`, never `0x1a 0x00 0x00 0x01 0x00`. This single rule is what
makes the encoding one-to-one, and a conformant decoder rejects the longer spellings.

Five shapes are admitted, and only five:

| Shape | Major type | Encoding |
|---|---|---|
| unsigned integer | 0 | The head; the value is the head's argument. Range `0 … 2^64 - 1`. |
| byte string | 2 | The head, whose argument is the **byte** length; then that many bytes. |
| text string | 3 | The head, whose argument is the **UTF-8 byte** length — not the character count; then those bytes. |
| array | 4 | The head, whose argument is the **element count**; then that many complete items. |
| boolean | 7 | One byte: `0xf4` for false, `0xf5` for true. |

Nothing else is legal in this profile. There are **no maps**, no negative integers, no tags, no
floating point, no `null`, no `undefined`, no indefinite-length items, and no other simple value. A
decoder that accepts any of them is not conformant, because accepting a second spelling of a value
is what lets two implementations disagree about what was signed.

The absence of maps is deliberate and load-bearing: map key ordering is the most common source of
canonical-CBOR disagreement, and this profile does not have to specify it because it cannot express
a map.

### A record

A record is a **definite-length array**:

* element 0 is the record's **domain tag**, a text string such as `mesh.v0.file-manifest`;
* elements 1..n are the record's fields, **in schema order**, one array element per field.

So an empty file manifest is:

```text
84                                    array(4)                 tag + 3 fields
  75 6d 65 73 68 2e 76 30 2e 66 …     text(21) "mesh.v0.file-manifest"
  00                                  unsigned(0)              byte_length
  58 20 af 13 49 b9 …                 bytes(32) af1349b9…      content_hash
  80                                  array(0)                 chunks
```

which is the first vector in [`v0/file-manifest.json`](v0/file-manifest.json). (`af1349b9f5f9…` is
BLAKE3 of the empty input, so you can check it against any BLAKE3 library.)

The domain tag leading the array is what stops two record types with the same field shapes from
producing the same bytes. It is versioned: `mesh.v0.…`. A change to what a domain covers is a new
tag, never an edit to an existing one.

### Field types

The schema gives every field one of these types. They compose.

| Type in the schema | On the wire |
|---|---|
| `unsigned` | a major-type-0 item |
| `bool` | `0xf4` or `0xf5` |
| `bytes` | a major-type-2 item. When the schema carries `byte_length`, exactly that many bytes. |
| `text` | a major-type-3 item |
| `sequence` | an array; every element has the type in the schema's `element` |
| `group` | an array of fixed arity: one element per field in the schema's `fields`, in order |
| `record` | a complete nested record encoding, spliced in whole, carrying its own domain tag |

**There are no optional fields.** The profile has no absence marker at all, which means the
question "how is an absent field encoded" has no second answer. A field that may be absent is
declared as a `sequence` holding at most one element: `[]` when absent, `[value]` when present —
an ordinary array, decoded by the ordinary rule.

### Ordering inside a sequence

A sequence that models a keyed collection is in **ascending byte-lexicographic order of the key's
UTF-8 bytes**. Today that is `mesh.v0.directory-version`'s `entries`, keyed by `name`.

Sort the raw UTF-8 bytes. Not by locale, not by code point after any Unicode normalization form,
and not by any collation. `"README.md"` (first byte `0x52`) sorts before `"café.txt"` (`0x63`),
which sorts before `"水.txt"` (`0xe6`). The second vector in
[`v0/directory-version.json`](v0/directory-version.json) is exactly this case and the generator
inserts those three names in a *different* order on purpose, so the vector fails if you skip the
sort.

### What is not bound

`mesh.v0.changeset` binds ten fields and **not** the authoring actor's signature. The signature is
made over the record, so a record that bound its own signature could never be signed. If you are
carrying a ChangeSet with its signature, the signature travels alongside the encoding, not inside
it.

## The vector files

Each file under `v0/` describes one record type.

```json
{
  "vector_format": "mesh-canonical-vectors/0",
  "encoding_profile": "mesh-cbor/0",
  "record": "mesh.v0.file-manifest",
  "signed_record": true,
  "summary": "…",
  "schema": { "domain_tag": "…", "fields": [ … ] },
  "vectors": [ … ]
}
```

Every entry in `vectors` carries:

| Key | Meaning |
|---|---|
| `name` | A stable identifier for the case, so a failure report can name it. |
| `description` | What the case is for, and what it would catch. |
| `record` | The input, as named fields. Byte strings are lowercase hex; everything else is a JSON number, string, boolean, array or object matching the schema. |
| `canonical_encoding_hex` | **The expected output.** Lowercase hex of the complete encoding. |
| `canonical_encoding_length` | Its length in bytes, so a truncation is obvious. |
| `canonical_encoding_digest_hex` | BLAKE3 of those bytes. This is the digest a signature over the record's bytes covers. |
| `record_id_hex` | The record's **name**. Present on record types that have one — see below. |

The JSON is plain: no comments, no trailing commas, UTF-8, `\n` line endings. Non-ASCII strings
appear as themselves rather than as `\u` escapes, so a directory entry name is readable in the file
that is about it.

### Two keys, one value: a record's name is the digest of its bytes

`canonical_encoding_digest_hex` is BLAKE3 of the bytes in `canonical_encoding_hex`. You can check it
with any BLAKE3 library.

`record_id_hex` is the record's **name**, and it is the same value. A content-derived name in this
protocol is BLAKE3 of exactly one byte string, and for a record that byte string is its canonical
encoding — the one in `canonical_encoding_hex`. So there is nothing extra to implement: once your
encoder agrees with the vectors, you can name every record you encode.

Both keys are published, and they must be equal, precisely so that the previous paragraph is
something you check rather than something we assert. If they ever disagree in a file here, that file
is wrong.

A byte sequence with no schema is named differently and deliberately: a **chunk** is named by BLAKE3
of its own content, with no array, no domain tag and no framing at all, so that any BLAKE3 tool
computes the same value over the same file. That is the only other rule, and there is no third one.

**This was not always true, and the history matters if you are reading an older copy.**
`record_id_hex` used to be produced by a separate identity framing that no published document
described, so an external implementation could verify `canonical_encoding_digest_hex` and could not
recompute `record_id_hex`. ADR-0033 ruled that the canonical encoding is normative and retired the
framing. **The implementation is still catching up:** until `01KZFMZC4MTHTT3BW4Y0BW6NYA` lands, the
`record_id_hex` values in the files under `v0/` are still the old framing's output and will **not**
equal `canonical_encoding_digest_hex`. Until then, treat `canonical_encoding_digest_hex` as the
record's name and ignore `record_id_hex`; the two become equal in that change, and nothing in
`canonical_encoding_hex` moves when they do.

## How to use these vectors

For each file under `v0/`, for each entry in `vectors`:

1. Build the record from `record`, decoding the hex byte strings.
2. Encode it with your implementation.
3. Assert your bytes equal `canonical_encoding_hex`, and their count equals
   `canonical_encoding_length`.
4. Assert BLAKE3 of your bytes equals `canonical_encoding_digest_hex`. That value is also the
   record's name, so step 4 is the whole of naming.
5. Decode `canonical_encoding_hex` with your decoder and assert you get `record` back.
6. Assert your decoder **rejects** a modified copy in which one integer head is widened by one byte
   (`0x05` becomes `0x18 0x05`). A decoder that accepts it will accept bytes your encoder would
   never produce.

Steps 1–4 are conformance. Steps 5 and 6 are what stop your implementation from being
one-directionally lucky. Do not assert against `record_id_hex` yet — see the note above about the
implementation catching up.

## Stability and versioning

`v0/` is a **format version directory**. If the encoding ever changes, the new bytes are published
under `v1/` and `v0/` stays exactly as it is — signatures were made over the bytes it holds, and
deleting them would make already-signed records unverifiable.

**What never moves is `canonical_encoding_hex` and its digest**, which is what that rule was always
protecting: the bytes a signature covers. `record_id_hex` carries a *name*, no signature covers it,
and ADR-0033 corrects it in place rather than publishing a `v1/` whose encoded bytes would be
byte-identical to `v0/`'s. A `v1/` means the format moved. A name changing does not move the format.

The files here are generated. In this repository they are compared to the generator on every test
run in both directions — no file that the generator does not produce, no generated file that
differs — so the encoding and this directory cannot drift apart. Hand-editing a file here does not
change the encoding; it breaks the build.

## What is not here yet

* **The public operation vocabulary.** `mesh.v0.empty-operation` is a placeholder with no fields.
  `mesh-operations` implements and internally tests the eighteen real operations, but their
  schemas and vectors are not published in this directory. The `operations` sequence and how a
  nested operation is spliced in are already pinned by
  [`v0/changeset.json`](v0/changeset.json).
* **Review bundles, approval receipts and heads.** Local implementations now exist outside
  `mesh-types`, but their complete external record schemas and vectors are not published here.
  Until they are, an outside implementation cannot reproduce the local approval path from the
  published artifacts alone.
* **The wire message set.** Requests, responses and errors are reserved by `docs/protocol.md` §7.3.
  `../proto/mesh/v0/records.proto` carries only the protobuf projection of the record types above.

## Additive workspace root operation

The independent [workspace root vector](v0/workspace-root.json) and
[operation schema](../schemas/workspace-root-v0.json) extend the operation vocabulary.
They do not replace the existing signed-record subset or its index. Older operation readers
refuse the new domain; see the [operation contract](../../docs/plan/execution-plan.md#explicit-empty-workspace-root).
