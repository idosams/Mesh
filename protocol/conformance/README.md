# protocol/conformance

**Maturity: executable draft evidence.** This suite checks the published `v0` artifacts; it does
not prove a supported live peer session. See [Protocol status](../README.md) and
[Project status](../../docs/project-status.md).

The conformance suite for **CWP `v0`**. Point it at your client and it tells you, case by case,
which published rules you satisfy, which you break, and which you have not implemented — and it
says which of those three each one is, because a suite that cannot tell "wrong" from "not yet"
pushes implementers towards stubs.

```console
$ node protocol/conformance/run.mjs --client reference
$ node protocol/conformance/run.mjs --client second   # the R14 second client, in Python
$ node protocol/conformance/run.mjs --client "python3 my_adapter.py" --report out.json
$ node protocol/conformance/run.mjs --list        # every case and the rule it is about
$ node protocol/conformance/run.mjs --self-test   # the suite checked against itself
```

Zero dependencies, zero network, no build step. Node 20 or later. On the current tree the suite is
**202 cases**, the reference client passes **179**, fails **0** and reports **23 unsupported** — and
all 23 are specification gaps rather than client gaps, which §6 is about.

**Two independent clients now answer it identically, case for case.**
[`clients/second/`](clients/second/README.md) is a second implementation in Python, by a different
author, in a process that cannot open `crates/**` *or* `protocol/test-vectors/**` — so its `ENC`
answers are computed from the schema rather than recognised from the answer key. It reaches the same
179 / 0 / 23 with zero per-case disagreements, and it needs a `python3` on `PATH`
(`MESH_CONFORMANCE_PYTHON` names another). What that established, and the eleven places where the
published material stops deciding, are in
[`reports/second-client-r14.md`](reports/second-client-r14.md) — task `01KZC2TBX5BGTQPXXX2DATM3TY`,
research item R14.

---

## 1. What it runs against, and what "over the wire" means here

The suite **never imports a client**. It spawns one as a subprocess and speaks a documented
line-protocol to it (§3). Everything it learns about your implementation, it learns from bytes
crossing a process boundary.

That is the strongest public boundary available today, and it is weaker than it will be. Mesh has
deterministic synchronization and local signature implementations, but no supported live CWP peer
or relay-backed user session — so there is no public session to run over. The adapter
protocol is deliberately shaped so the answer does not change when one arrives: every request is
about bytes or about a decision, never about your internal types. When a transport lands, the
session-scoped cases (§5, `HEAD-*`, `DELIV-*`, `PUB-*`) become answerable and the byte-level cases
are unchanged.

Stating that plainly is the point. A suite that called itself a wire-conformance suite while calling
functions would be making the claim this repository exists to stop being made without evidence.

## 2. The three results

| Result | Meaning | Affects the exit code |
|---|---|---|
| `pass` | The client answered, and the answer matches the published material. | — |
| `fail` | The client answered, and the answer breaks a published rule. Or the adapter crashed, timed out, or wrote something that is not JSON. | **yes** |
| `unsupported` | The client said it does not implement this, or did not declare the capability the case needs. | no |

`unsupported` is not a soft `fail`, and it is not a `pass`. A partial implementation reporting
honestly is not a broken one, and the suite is designed so that saying "I have not built this" is
always cheaper than faking an answer — a stub that returns something plausible gets graded, and
usually fails.

Exit code: **0** no case failed · **1** at least one failed · **2** usage or adapter error.

An adapter that dies mid-conversation is a `fail`, not an `unsupported`. "I cannot answer" is a
sentence a client has to be able to say out loud.

## 3. The adapter contract — `cwp-conformance-adapter/0`

Your adapter is any executable that reads **one JSON object per line on stdin** and writes **one
JSON object per line on stdout**, in order, one response per request. Nothing else about it is
constrained: any language, any runtime.

Every response carries `ok`. Three shapes, and only three:

```json
{"ok": true, ...}                                            // answered
{"ok": false, "unsupported": true, "reason": "…"}            // not implemented — graded `unsupported`
{"ok": false, "unsupported": false, "reason": "…"}           // an error — graded `fail`
```

A response may carry `"tracking": "<id>"` alongside `unsupported` when the reason is a known gap in
the specification rather than in the client.

### 3.1 `hello` — always first

```json
→ {"op": "hello"}
← {"ok": true, "adapter": "cwp-conformance-adapter/0", "client": "my-client",
   "description": "…", "capabilities": ["record.encode", "record.decode", …]}
```

`capabilities` is what makes honest partial reporting possible: a case whose `requires` you did not
declare is `unsupported` without ever being asked. Declaring a capability you cannot honour does not
help — you are graded on the answer, not the claim.

### 3.2 The operations

| `op` | Request fields | Response fields | Capability |
|---|---|---|---|
| `record.encode` | `domain_tag`, `record` | `hex` | `record.encode` |
| `record.decode` | `domain_tag`, `hex` | `domain_tag`, `record` | `record.decode` |
| `record.digest` | `hex` | `digest_hex` — BLAKE3 of those bytes | `record.digest` |
| `record.id` | `domain_tag`, `record` | `record_id_hex` | `record.id` |
| `bytes.reject` | `hex`, `as` (`record` or `message`) | `rejected` (boolean), `reason` | `bytes.reject` |
| `message.encode` | `name`, `value` | `hex` | `message.encode` |
| `message.decode` | `hex` | `tag`, `name`, `value` | `message.decode` |
| `message.plane` | `name` | `plane` | `message.plane` |
| `message.admit` | `name`, `value` | `admitted` (boolean), `error_code` (or `null`), `reason` | `message.admit` |
| `error.code` | `name` | `tag`, `retryable` | `error.code` |
| `session.*`, `publication.attempt` | `ask` | case-specific | `session`, `publication` |

Three conventions the whole protocol rests on:

- **Byte strings are lowercase hex**, everywhere, in both directions. A field the published schema
  types `bytes` is a hex string in `record` and `value` objects.
- **An unsigned value is a JSON number.** No case in the `v0` catalogue needs a value above
  `2^53 - 1`; if one ever does, it will be carried as a decimal string and this table will say so.
  Silently relying on a language's number type across a JSON boundary is how two implementations
  come to disagree about a value neither of them printed.
- **`bytes.reject` is not "did it throw".** It asks whether your *decoder* refuses the bytes. A
  conformant decoder refuses more than a permissive one accepts, so the suite also feeds it the
  unmodified published bytes: `PROF-control-accepts-the-published-bytes` and
  `MSG-control-accepts-the-published-frame` fail a client that refuses everything, which is
  otherwise a perfect score on every refusal case.

`clients/published/client.mjs` is a complete worked example in about 150 lines, written from
`protocol/**` with no Rust read. Copy its structure.

## 4. Running it

```console
$ node protocol/conformance/run.mjs --client <name|command> [options]

  --client <name|command>  reference (default) · second · broken · or a command to spawn
  --break <mutation>       with --client broken, which violation to inject
  --only <substring>       run only cases whose id contains this, or one family
  --report <file>          write the JSON report here
  --json                   the JSON report on stdout instead of the text one
  --verbose                print passing and unsupported cases too
  --list                   print the catalogue and exit
  --self-test              check the suite against itself and exit
```

A failure prints the case, the rule, where the rule is written down, and the difference:

```text
FAIL  ENC-mesh.v0.directory-version/three-entries-sorted-by-utf8-bytes-key-order
      mesh.v0.directory-version/three-entries-sorted-by-utf8-bytes sorts entries by the key's UTF-8 bytes
      rule:  a sequence modelling a keyed collection is in ascending byte-lexicographic order of
             the key's UTF-8 bytes — not by locale, not by code point after normalization
      cited: protocol/test-vectors/README.md §Ordering inside a sequence
      the encoding of entries supplied in reverse order differs at byte 47: expected 0x69, got 0x67
              expected …abcdef027020a020202020202020838369524541444d452e6d6450000000000031…
              got      …abcdef027020a020202020202020838367e6b0b42e747874500000000000337333…
```

## 5. The case families

Nothing in the catalogue is a hand-written list. Every `ENC`, `ID`, `MSG` and `ERR` case is generated
from `test-vectors/v0/**`, `schemas/canonical-encoding-v0.json` and `wire/v0/messages.json`, so a
newly published record type, message or error code gets its cases the moment it is published —
which is exactly when a hand-maintained list would fall behind.

| Family | What it decides | Built from |
|---|---|---|
| `ENC` | The bytes a signed record is: encode, byte count, BLAKE3 digest, decode back, refuse a widened head, and the keyed-sequence ordering rule. | the vectors |
| `PROF` | The `mesh-cbor/0` exclusions as refusals: maps, negative integers, floats, `null`, other simple values, tags, indefinite lengths, non-shortest heads, trailing bytes, truncation, wrong fixed widths, wrong arity. | the profile's `excluded` list |
| `ID` | Recomputing `record_id_hex`. **Nothing can pass this today** — §6. | the vectors |
| `MSG` | The frame, every message's bytes and field values, its plane, its published example's admissibility, unknown tags, trailing bytes, identifier width, and the two non-negotiated version fields. | `wire/v0/messages.json` |
| `ERR` | Every error code's tag and whether it is retryable. | `wire/v0/messages.json` |
| `HEAD` | Head advancement: sequence zero, frontier ordering, the sparse-set rule, absence as an empty sequence — and SG-7, OG-9 and OG-6, which need a session. | `docs/protocol.md` §2.1–2.2, §7.3 |
| `DELIV` | Delivery semantics: the content plane's exact membership, batch bounds, acknowledgement ordering, chunk-part bounds, Merkle-summary contiguity — and OG-3, OG-4 and OG-5, which need a session. | `docs/protocol.md` §2.2, §7.3 |
| `PUB` | Publication authority: TG-3 … TG-10. **Nothing can pass these today** — §6. | `docs/protocol.md` §2.4 |

The `HEAD`, `DELIV` and `PUB` cases that need a session are in the catalogue even though nothing can
answer them. A suite whose coverage grows only as the implementation grows is a suite that always
reports the implementation as complete.

## 6. Specification gaps — the part the suite refuses to hide

Some cases cannot be passed by *any* client built from the published material, because the published
material does not say how. Those are not client defects and the report does not file them as such:
they are printed at the end of every run, under `SPECIFICATION GAPS`, whether or not anything failed,
and counted separately in the JSON report's `specification_gaps`.

Two today.

**`record_id_hex` cannot be recomputed — 9 cases, tracked as `01KZCZDTVD0D36W5YRGX8CNE17`.**
[`../README.md`](../README.md) §6.1 has it in full. In short: `docs/protocol.md` §2.1 says a
content-derived name is computed from content *under the canonical encoding*, and §3.10's
`DigestWriter` row says the identity framing is *not* the canonical encoding — and the derivation
uses the framing, which is not published as a schema. So a client built from `protocol/**` can verify
`canonical_encoding_digest_hex` and cannot recompute `record_id_hex`. The suite keeps the `ID-*`
cases and reports them as a specification gap. **Deleting them would make the report look complete
for a protocol nobody can fully implement**, and that is the failure mode a conformance suite exists
to prevent. They come back as ordinary cases when the question is resolved — either the framing is
published with its own vectors, or identity moves onto the canonical encoding and every record
identifier moves with it. Both are breaking changes; see [`../VERSIONING.md`](../VERSIONING.md) §2.1.

**Publication, review bundles, envelopes and sessions are incomplete in the published surface — 14 cases.**
[`../README.md`](../README.md) §4. Local signing and approval implementations now exist, but the
approval shapes lack complete public record schemas and vectors, and no supported peer session is
available. A client built only from the published material cannot pass these cases, so the report
says so rather than omitting the rows.

If a client *does* answer a specification-gap case correctly, the report says that too:

```text
NOTE  9 case(s) marked as specification gaps were answered correctly. That means this client knows
      something the published material does not say — worth publishing, or worth doubting.
```

## 7. The report format — `cwp-conformance-report/0`

`--report <file>` writes JSON; [`report-schema.json`](report-schema.json) is its shape. The top
level carries `client`, `summary` (totals, per-family counts, and `specification_gaps`), `verdict`,
a `specification_gaps` array, and `cases` — every case with its `rule`, `citation`, `requires`,
`result` and, when it failed, the `detail` that names the difference.

`verdict` is `conformant-so-far` or `non-conformant`. The hedge in the first is deliberate: passing
this suite means no case failed, and 23 of the 202 cases cannot be run by anyone. It is not a
certification and does not claim to be.

## 8. `--self-test`, and why the numbers above are checkable

A conformance case only ever observed to pass is not evidence that it can fail. `--self-test` does
three things and exits non-zero if any of them does not hold:

1. The reference client passes every case it declares support for, passes at least one, and reports
   at least one `unsupported` — so the third result is reachable rather than theoretical.
2. At least one specification gap is reported, and `01KZCZDTVD0D36W5YRGX8CNE17` is among them, and
   no `ID-*` case is graded `fail`. A run that silently stopped reporting the contradiction fails
   here.
3. **Every one of the thirteen injected protocol violations in `clients/broken/client-mutations.mjs`
   is caught by the case that names its rule** — not merely by *some* case, which a suite that fails
   everything would also satisfy. Each one is a real violation rather than a crash: a widened head,
   a decoder that accepts one, an unsorted keyed sequence, a wrong digest, a message on the wrong
   plane, an oversized batch admitted, a guessed unknown tag, accepted trailing bytes, an actor head
   at sequence zero, a fabricated `record_id_hex`, a decoder that refuses everything, a wrong error
   tag, and an adapter that dies mid-conversation.

`fabricate-record-id` is worth singling out: it fails, while the reference client's `unsupported` for
the same case does not. Answering "I do not know" is free; inventing an answer is not.

## 9. What this suite does not establish

- **It cannot detect a client that answers from the published files.** Nothing the *suite* does can:
  an adapter that reads `test-vectors/v0/*.json` and echoes `canonical_encoding_hex` passes every
  `ENC` case. The reference client deletes every `vector` from the documents it loads (`loadSpec`),
  which is a discipline and not an enforcement. A *client* can do better about itself —
  [`clients/second/sandbox.py`](clients/second/sandbox.py) makes the vectors unopenable for the life
  of the process — but that is the client vouching for itself, and conformance evidence is still
  worth what the party producing it is worth.
- **It does not run a live peer session.** The repository has synchronization-engine code, but no
  supported public peer or relay-backed session contract to grade. §1.
- **It is not wired into `npm test`.** Wiring it means editing `package.json`, which is outside the
  allowed paths of the task that wrote this directory — the same limit
  [`../README.md`](../README.md) §5 states for `verify-published.mjs`. Until a task that owns
  `package.json` wires both, this suite is trustworthy exactly as often as somebody runs it. The
  command to add is `node protocol/conformance/run.mjs --self-test`, which subsumes a plain run.
- **It does not grade the repository's newer local operation and approval implementations.** The
  published schemas and vectors have not caught up with those shapes, so the suite cannot turn
  internal code into an external compatibility promise.

## 10. Reporting a gap

A case you believe is wrong is a specification bug or a suite bug, and both are worth more than a
workaround. File it — in this repository that is a `kind: bug` task. **A conformance case that the
reference implementation fails is either a spec bug or an implementation bug: decide which through
protocol review, never by removing the case.**
