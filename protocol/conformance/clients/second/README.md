# clients/second — the R14 second client

A complete `cwp-conformance-adapter/0` client in **Python**, written from `protocol/**` alone. It
exists to answer research item R14: *is CWP implementable by somebody who does not have the Rust?*

```console
$ node protocol/conformance/run.mjs --client second        # 202 cases · 179 pass · 0 fail
$ python3 protocol/conformance/clients/second/client.py --self-audit
```

The finding, the gap register and the time accounting are in
[`../../reports/second-client-r14.md`](../../reports/second-client-r14.md). This page is about the
code.

| File | What it is |
|---|---|
| [`client.py`](client.py) | the adapter loop and the op dispatch — one JSON object per line, in order |
| [`meshcbor.py`](meshcbor.py) | the `mesh-cbor/0` codec, schema-driven, records and messages alike |
| [`blake3.py`](blake3.py) | BLAKE3 in pure Python, from the BLAKE3 specification |
| [`admit.py`](admit.py) | the per-message preconditions, transcribed by hand from the published prose |
| [`sandbox.py`](sandbox.py) | the independence constraint, as a `sys.addaudithook` the process cannot remove |

Needs Python 3.8 or later and nothing else: no dependency, no build step, no network — the same
contract the suite holds itself to. `MESH_CONFORMANCE_PYTHON` names a different interpreter.

## Why it is worth having a second one

`clients/published/` is already a client written from the published material. This one differs in
three ways that are the reason to keep both:

1. **A different language and a different author.** Two implementations sharing a runtime share its
   habits.
2. **It cannot read the answer key.** `sandbox.py` refuses every file outside `protocol/` — so no
   Rust — *and* refuses `protocol/test-vectors/**`, where the expected bytes live. The reference
   client loads the vectors and deletes them, which is a discipline; this is a property.
3. **It wrote its own BLAKE3**, so `record.digest` is an independent computation.

## The two knobs, and why a client has knobs at all

`MESH_SECOND_READING` selects the *other* defensible reading of a rule the published material does
not decide. It exists because the report's central claim needs to be a measurement:

| Value | The other reading |
|---|---|
| `key-order` | a decoder **accepts** a keyed sequence that arrives out of order (report SPEC-4) |
| `unknown-operation` | a decoder **carries opaquely** a nested record whose domain tag has no schema, instead of refusing it (report SPEC-5) |

Every combination scores 179 pass · 0 fail, and the readings give opposite answers about real bytes.
Both knobs go away the day the questions are decided; neither is a configuration option a client
should have.

## What it answers `unsupported` to

`record.id`, because `record_id_hex` cannot be recomputed from `protocol/**`
(`01KZCZDTVD0D36W5YRGX8CNE17`), and everything session-scoped, because the published artifacts do
not define a supported live peer journey. Repository code now has synchronization and local
signing implementations, but those do not give this client an interoperable public session
contract. Both answers carry their reason, per request. A fabricated identifier would be graded
`fail`; "I do not know" is free, and it is the honest answer.
