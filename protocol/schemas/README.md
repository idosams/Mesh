# protocol/schemas

**Maturity: normative draft artifacts for the published subset.** Repository implementations may
be ahead of this surface. See [Protocol status](../README.md) and
[Project status](../../docs/project-status.md).

Published CWP schemas.

## `canonical-encoding-v0.json`

The machine-readable schema of the **canonical encoding** — the `mesh-cbor/0` profile, and every
signed record type's domain tag and field order.

It is the companion to [`../test-vectors/`](../test-vectors/README.md): the schema says what the
fields are and in what order, the vectors say what the resulting bytes are. An external
implementer needs both and needs no Rust. Read
[`../test-vectors/README.md`](../test-vectors/README.md) first — it explains the profile in prose;
this file is the same thing in a form a code generator or a conformance harness can consume.

Structure:

- `encoding_profile` — the five admitted CBOR shapes, everything excluded, and the rules that close
  the encoding.
- `records[]` — one entry per record type: its domain tag, whether it is signed, and its schema as
  an ordered field list.
- `digest` — which digest is which, including the standing difference between
  `canonical_encoding_digest_hex` and `record_id_hex`.

**Generated, not hand-written.** It is produced by `mesh_types::published_documents()` and compared
to this file byte for byte on every test run, so it cannot drift from the encoder. Editing it by
hand breaks the build rather than changing anything. Regenerate after a deliberate encoding change
with:

```console
cargo test -p mesh-types --test serialization-compat -- --ignored write_published_documents
```

An encoding change is a compatibility event: the vectors move to a new format-version directory and
the old one is retained, because signatures were made over the bytes it holds.

## Not yet published

The rest of the CWP schema surface — the wire message set, the knowledge model, the validation
profile — is filled under **E16 open protocol** and by the tasks that own
`docs/protocol.md` §7. Find them with:

```bash
See the public GitHub issue tracker
```
