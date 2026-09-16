# R14 — a second CWP client, and exactly where the specification runs out

Research item **R14** asks the only question that decides whether CWP is a protocol or a program:
*can somebody else implement it from what we published?* This report is the answer produced by
building one — [`../clients/second/`](../clients/second/), in Python — and running it against the
conformance suite. Task `01KZC2TBX5BGTQPXXX2DATM3TY`.

The short version: **the encoding and the message set are implementable from `protocol/**` alone,
and the parts around them are not.** 202 cases, 179 pass, 0 fail, 23 unsupported, agreeing with the
reference client case for case. Eleven places where the published material does not decide a
question an implementer must answer are in §3, and two of them are demonstrated in §4 by two
readings of the same sentence that **score identically on the suite and give opposite answers about
the same bytes**.

---

## 1. The result

```console
$ node protocol/conformance/run.mjs --client second
202 cases · 179 pass · 0 fail · 23 unsupported (23 of them specification gaps) · 60 ms
VERDICT: no case failed. Unsupported cases are unanswered, not passed.
```

Exit code `0`. Per family: `ENC 51/51 · ID 0/0 · PROF 13/13 · MSG 80/80 · ERR 14/14 · HEAD 6/6 ·
DELIV 15/15 · PUB 0/0`.

The two clients were then compared case by case rather than summary by summary, because two runs
can reach 179 by passing different cases:

```console
$ node protocol/conformance/run.mjs --client second    --report /tmp/second.json
$ node protocol/conformance/run.mjs --client reference --report /tmp/reference.json
$ node -e 'const a=require("/tmp/second.json"),b=require("/tmp/reference.json");
  const m=new Map(b.cases.map(c=>[c.id,c.result]));
  console.log(a.cases.filter(c=>m.get(c.id)!==c.result).length+" disagreements")'
0 disagreements
```

**Nothing was changed in the suite, in the schema, in the vectors or in the message set to make this
pass.** The only edit outside `clients/second/` is the `second` entry in `run.mjs`'s client table
and the three places that list the client names. The client was green on its first complete run;
that is stated because it is unusual and because it is the actual measurement of how good the
published material is.

The 23 unsupported cases are the two gaps the suite already publishes — `record_id_hex` (9 cases,
`01KZCZDTVD0D36W5YRGX8CNE17`) and the specified-but-unimplemented publication and session families
(14 cases). This client answers `unsupported` to both **per request, with the reason**, rather than
declining to declare the capability, so the reason lands in the report verbatim.

## 2. What was read, and what the process cannot read

R14 is worth nothing if the second implementation quietly consulted the first. Two different claims
are made here and they are not equally strong.

**The strong one, which is enforced.** The client process **cannot open**:

| Refused | Why it matters |
|---|---|
| everything outside `protocol/` — so all of `crates/**` | it cannot consult the reference implementation |
| `protocol/test-vectors/**` | it cannot read the expected bytes; every `ENC-*` answer is computed from the schema |
| every socket, subprocess and `exec` | it cannot fetch an answer or shell out to something that reads for it |

[`sandbox.py`](../clients/second/sandbox.py) installs a `sys.addaudithook` (PEP 578) hook that
cannot be removed for the life of the process. This is **stricter than the reference client**, which
loads the vectors and then deletes them: a deletion is a discipline, an audit hook is a property.

```console
$ python3 protocol/conformance/clients/second/client.py --self-audit
{ "probes": [ { "path": "crates/mesh-types/src/lib.rs",  "refused": true },
              { "path": "crates/mesh-crypto/src/lib.rs", "refused": true },
              { "path": "docs/protocol.md",              "refused": true },
              { "path": "protocol/test-vectors/v0/file-manifest.json", "refused": true } ],
  "published_material_readable": true, "holds": true }
$ echo $?
0
```

**The weak one, which is disclosed rather than enforced.** A hook constrains the process, not the
author. The author's read set was: all of `protocol/**` (including `conformance/lib/**` — the suite
is published material and a client has to answer its questions); `docs/protocol.md` §2.1, §2.2,
§3.10 and §7.3 as cited from `protocol/README.md` §6.1; and `clients/published/client.mjs`, which
`conformance/README.md` §3 names as the worked example of the adapter contract. **No Rust was
opened at any point, and `clients/published/cbor.mjs`, `blake3.mjs` and `preconditions.mjs` were
deliberately not opened** — those three are where the reference client's protocol logic lives, and
the encoder, the digest and the preconditions in `clients/second/` were written from the published
prose and schema instead. That is a statement by the author. It is worth what the author is worth,
which is why the enforced claim above exists at all.

One consequence worth keeping: `blake3.py` is BLAKE3 written from the BLAKE3 specification in pure
Python, so `record.digest` is an independent computation and not a second call into one library.

## 3. Where the specification runs out

Eleven findings. **SPEC-1 is reported and deliberately not resolved** — it is a ratified open
question with a tracking id, and resolving it is a protocol change rather than an implementer's
choice. None of the rest has a decision doc, and the owner column names the decision owner from
plan §14.1 who would have to make one.

| # | The question an implementer must answer and the material does not | Bites | Owner |
|---|---|---|---|
| **SPEC-1** | `record_id_hex` cannot be recomputed. `docs/protocol.md` §2.1 says a content-derived name is computed under the canonical encoding; §3.10's `DigestWriter` row says the identity framing is **not** the canonical encoding, and `derive_id` uses the framing, which is published as a register row and not as a schema. | every signed record: an external client can verify `canonical_encoding_digest_hex` and cannot name a record | protocol and correctness — **already tracked as `01KZCZDTVD0D36W5YRGX8CNE17`** |
| **SPEC-2** | How a nested `record` field is represented in a vector's `record` object. `test-vectors/README.md` §*The vector files* says "byte strings are lowercase hex; everything else is a JSON number, string, boolean, array or object matching the schema" — and `changeset.operations` is neither: it is a **hex string of the nested record's complete encoding**. Discoverable only by reading `changeset.json`. | anyone generating a codec from the schema | protocol and correctness |
| **SPEC-3** | Which sequences are "keyed collections". The ordering rule is normative and **the schema carries no marker** — `test-vectors/README.md` names `directory-version.entries` in prose. A schema-driven encoder must hardcode a record name or guess a heuristic. `clients/second/` guesses "a sequence of groups whose first field is `text`", which is right for the whole published population of one. | the first record type published with a second keyed collection | protocol and correctness |
| **SPEC-4** | Whether a **decoder** must refuse a keyed sequence that arrives out of order. The profile says the encoding *is* ordered; nothing says what to do with bytes that are not. §4 shows both readings passing the suite. | any peer receiving a directory version from a non-conformant or hostile encoder | protocol and correctness |
| **SPEC-5** | What a receiver does with a ChangeSet carrying an operation whose domain tag it has no schema for. The profile is self-delimiting, so skipping is possible; a schema-driven decoder cannot find the item's end. **The operation vocabulary is unpublished** (`protocol/README.md` §6.3), so this is the state *every real ChangeSet* will be in. §4 shows both readings passing the suite. | the first ChangeSet carrying a real operation — i.e. all of them | protocol and correctness |
| **SPEC-6** | Which error code a refusal carries. `preconditions` is an array of English sentences, and **six of them name no code**: `heads ascend by actor`, `changesets is non-empty`, the ordering rules of `REQUEST_OPERATIONS` / `ADVERTISE_MANIFESTS` / `ACK_CHUNKS`, the `MAX_CHUNK_PART_BYTES` bound, `sequence is non-zero`, and `expires_after_millis is non-zero`. Two implementations answering `malformed message` and `sequence gap` for the same frame are both defensible, and `wire/README.md` says a sender decides what to do **from the code alone**. | every refusal path; a sender's retry logic reads the code | protocol and correctness |
| **SPEC-7** | The largest admissible frame. `MAX_OPERATIONS_PER_BATCH` and `MAX_CHUNK_PART_BYTES` exist because (`docs/protocol.md` §7.3) "a claimed batch size cannot drive an unbounded allocation" — and **no bound at all is published** for `AUTHENTICATE.signature`, `OPERATIONS_BATCH[].body`, the three `body` fields of `REVIEW_BUNDLE` / `VALIDATION_RECEIPT` / `APPROVAL_ENVELOPE`, `ERROR.detail`, or a frame as a whole. The stated goal is not reached by the published set. | a streaming receiver, which needs an admissible-size rule before it can allocate; the nearest thing to a security finding here | protocol and correctness, with runtime and platform |
| **SPEC-8** | What `max_elements` means. It appears on `ADVERTISE_FRONTIER.canonical_head` in `wire/v0/messages.json` and is **absent from the documented field vocabulary** (`wire/README.md` lists "`sequence` (with an `element`)"), and from the profile's rules. A codec generated from the documented vocabulary ignores it and accepts a two-element canonical head. | the one optional field in the protocol | protocol and correctness |
| **SPEC-9** | Whether an implementer's BLAKE3 is right above 1024 bytes. **Every published digest input is shorter than one BLAKE3 chunk**, so the published vectors pin the single-chunk path and pin nothing about the tree. A tree-mode bug is invisible to every check in this repository until a record exceeds 1 KiB. | the first record over 1 KiB — a manifest with ~30 chunks | protocol and correctness |
| **SPEC-10** | How a session-scoped precondition is checked at all. `AUTHENTICATE`'s "HELLO came first on this session, and named the same actor" and `UPDATE_CANONICAL_HEAD`'s "the receipt is verified where its bytes are" are not decidable about one message, and `cwp-conformance-adapter/0`'s `message.admit` is a question about one message. Neither is in any conformance case. | anyone building the handshake | protocol and correctness |
| **SPEC-11** | `protocol/README.md` is now stale in three places: §1 and §4 call `conformance/` "Reserved. Not implemented", and §5 says "The Python artifact is not in this tree, so the claim is a recorded result and not something you can re-run here" — which this directory makes false. **Outside this task's allowed paths** (`protocol/conformance/**`, `docs/adr/**`), so it is reported and not fixed. | the first outside reader of §4 | product and integrations, as a documentation fix |

**The gaps above remain open public follow-up work.** The register is the authoritative
list for this report. See §6.

## 4. Two probes: same suite score, opposite answers

SPEC-4 and SPEC-5 are not stylistic. `MESH_SECOND_READING` selects the other defensible reading of
each, and all four combinations score the same:

```console
$ for r in "" key-order unknown-operation key-order,unknown-operation; do
    MESH_SECOND_READING=$r node protocol/conformance/run.mjs --client second | grep '^202'; done
202 cases · 179 pass · 0 fail · 23 unsupported (23 of them specification gaps)
202 cases · 179 pass · 0 fail · 23 unsupported (23 of them specification gaps)
202 cases · 179 pass · 0 fail · 23 unsupported (23 of them specification gaps)
202 cases · 179 pass · 0 fail · 23 unsupported (23 of them specification gaps)
```

Put the same bytes to both. The first is a `mesh.v0.directory-version` whose three entries are in
**descending** key order; the second is a `mesh.v0.changeset` carrying one operation with the domain
tag `mesh.v0.create-file`, which no published schema defines:

```console
$ printf '{"op":"bytes.reject","as":"record","hex":"8378196d6573682e76302e6469726563746f72792d76657273696f6e500189abcdef027020a020202020202020838367e6b0b42e747874500000000000337333b3333333333333335820d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d38369636166c3a92e747874500000000000327232b2323232323232325820d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d28369524541444d452e6d64500000000000317131b1313131313131315820d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1"}\n{"op":"bytes.reject","as":"record","hex":"8b716d6573682e76302e6368616e6765736574500189abcdef00710181010101010101015820f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8500189abcdef037202820202020202020201805820b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b05820b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b18181736d6573682e76302e6372656174652d66696c6507821b0000018bcfe5680003"}\n' \
  | python3 protocol/conformance/clients/second/client.py
{"ok": true, "rejected": true, "reason": "entries is not in ascending order of its key's UTF-8 bytes"}
{"ok": true, "rejected": true, "reason": "no published schema for the domain tag mesh.v0.create-file"}

$ …same input… | MESH_SECOND_READING=key-order,unknown-operation python3 …/client.py
{"ok": true, "rejected": false, "reason": "the bytes decoded without complaint"}
{"ok": true, "rejected": false, "reason": "the bytes decoded without complaint"}
```

Two implementations, both `conformant-so-far`, disagreeing about whether a real ChangeSet is
admissible. **A conformance suite cannot close this and should not try**: the suite grades against
the published rules, and there is no published rule here to grade against. Adding a case would be
the suite legislating, which `conformance/README.md` §10 already refuses. The fix is a decision.

## 5. How long it took, and where the time went

Wall-clock, measured from commit and file timestamps rather than estimated. **This is one agent
lane's elapsed time and it is not a human-effort estimate** — a human implementer's number would be
dominated by the same reading phase and would be much larger in the implementation phase.

| Phase | Elapsed | What happened |
|---|---|---|
| Reading (from the claim event) | **4 min 15 s** | `protocol/README.md`, `test-vectors/README.md`, `schemas/canonical-encoding-v0.json`, `wire/README.md`, `wire/v0/messages.json`, `conformance/README.md` and `conformance/lib/**`. Four vector files were read by the *author* for their shape — SPEC-2 is discoverable nowhere else — before the sandbox forbade the *client* from reading any. |
| Implementation to first green run, then the probes | **9 min 26 s** | 1122 lines of Python: BLAKE3 (150), the codec (420), the preconditions (211), the adapter loop (215), the sandbox (126). BLAKE3 was checked against two published digests before anything else was written. The suite was green on the first complete run. |
| Report, client documentation, gates, pull request | see the pull request | — |

Where the time actually went, which is the part worth reporting:

- **Roughly half of the implementation phase was the message preconditions** — twenty-odd English
  sentences transcribed by hand into predicates, with the error code guessed six times (SPEC-6).
  Every other part of the client is generated from a machine-readable schema and took minutes.
- **BLAKE3 cost more than the entire `mesh-cbor/0` codec.** The profile is genuinely one page and
  behaves exactly as that page says; the digest is somebody else's specification and its tree mode
  is pinned by nothing here (SPEC-9).
- **Zero time was spent debugging byte mismatches.** Not one encoding case failed at any point. On
  the evidence of this run the canonical encoding, its schema and its vectors are the best-specified
  part of CWP, and the "afternoon" estimate in `test-vectors/README.md` is not marketing.

## 6. What this does not establish

- **It is not an outside implementation.** The same repository, the same program, one author who has
  read this repository's documentation. The enforced constraint in §2 is on the process; the disclosed
  one is on the author. An outsider would meet SPEC-2, SPEC-3 and SPEC-6 harder than this run did,
  because this run could recognise what the prose *meant to say*.
- **It is not a session.** No transport exists, so `HEAD`, `DELIV` and `PUB`'s session-scoped cases
  are unanswered by this client, by the reference client, and by Mesh. 14 of the 23 unsupported cases.
- **It does not run in `npm test`.** Neither does the suite (`conformance/README.md` §9): wiring it
  means editing `package.json`, which is outside this task's allowed paths as it was outside the
  suite's. Until a task that owns `package.json` wires `node protocol/conformance/run.mjs
  --self-test`, this result is true of the tree it was run on and is not defended against
  regression. The second client raises the value of wiring it: it is the only check in the
  repository that would catch a schema change that makes the published material insufficient.
- **The gaps are registered here, not filed.** §3 records SPEC-2 … SPEC-11 with an owner each and no
  tracking id, because this lane was instructed not to open tasks. A register in a report is exactly
  the failure mode `CLAUDE.md` rule 2 names — "never let a finding die in a review report" — so the
  filing is owed by whoever reads this, and the count to file is **ten**.
