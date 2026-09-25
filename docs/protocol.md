# CWP protocol: the four core graphs and the terminology register

This document defines **what the words mean** and **what the four graphs are**. It is the
vocabulary every later protocol document, crate, schema and test is written against. A term used
in this repository that is not defined here is a defect in one of the two — either the term gets
a definition here, or the code gets a different word.

Working protocol name: **CWP** — the Causal Workspace Protocol.

Normative source: the Mesh execution plan, §4.1 (four core graphs), §4.2 (entity model), §4.4
(actor heads) and §5.1 (consistency model). Product rules are **cited, never restated**: where a
rule already exists in [`docs/charter.md`](charter.md), this document links to it rather than
paraphrasing it, because a paraphrase is a second definition and second definitions are the
failure this document exists to prevent.

The consistency model itself — which state is authoritative, which is allowed to be lossy, and
what each promise means — lives in [`docs/consistency.md`](consistency.md).

The **published protocol** — the draft an outside party builds a client from, with its schemas, its
test vectors, its message set with exact bytes, and its versioning and compatibility policy — is
[`protocol/README.md`](../protocol/README.md). That directory cites this document for every
definition and restates none of them; what it adds is a coverage map, an implementation-status
table, the compatibility policy and the open questions an external implementer meets.

---

## 1. How to read this document

### 1.1 The one-definition rule

**Every term in the register below has exactly one definition. No term has two.** Where two
concepts are genuinely different, they get two terms; where two words mean the same thing, one is
the term and the other is an **alias** carrying no definition of its own (§4).

The rule has five clauses:

- The register (§3) is the only place in the repository where a protocol term is defined.
- No word in these documents is used as a term without a definition.
- No enumerated value belongs to two terms, because a value with two meanings is the conflation
  the register exists to prevent.
- A crate, a public type, a wire message, a schema field or a test name uses register terms.
- Introducing a term in a later document without adding it here is a specification gap, and
  [`docs/protocol.md`](protocol.md) is the thing that has to change first.

**Four of the five clauses are checked by a machine; one is not.** §6 specifies the lint, pastes its
real output, and names precisely what it does not reach. Clauses one, two and three are enforced by
TL-7, TL-8 and TL-9; clause four is enforced by TL-4, TL-5, TL-6 and TL-10 for crate names and
public types — TL-5 catching an item with no row and TL-10 a row with no item — and by nothing for
wire messages, schema fields and test names, which no crate yet declares.
Clause five is a rule about what an author must do next and is not machine-checkable at all. Read
§6's *What the checks do not cover* before relying on any sentence here that says a property
"holds": the lint makes uniqueness mechanical for quoted usage and headings, and leaves unquoted
prose to review permanently.

### 1.2 Words that are correct here and forbidden elsewhere

[charter §5](charter.md#5-what-the-words-mean-to-a-user) names nine words that are never exposed
to a user: `DAG`, `frontier`, `vector clock`, `branch`, `commit`, `rebase`, `staging`, `ref` and
`operation log`. **That is charter §5's list, quoted in full and not restated with different
members here.** Three of the nine — `DAG`, `frontier` and `vector clock` — are precise and
required in this document and in [`docs/consistency.md`](consistency.md). The other six have no
register row and none of them names a concept here: `commit` appears only as ordinary English —
chiefly inside `durable commit sequence` — and once naming an imported Git commit under
`external provenance anchor`; `rebase` appears only as the ordinary verb for the thing
`reclassification` refuses to do silently; and `branch`, `staging`, `ref` and `operation log`
appear nowhere in either document except this paragraph and §4.1's declaration table.

The prohibition on internal vocabulary is wider than those nine words, and the wider rule is §5 of
this document rather than a longer list: **no register term appears in a product surface.**
`causal parent`, `compare-and-swap`, `policy epoch`, `file manifest` and `chunk` are as unsuitable
for a product surface as the nine are, and §5 already covers them — enumerating them a second time
here would be a second list to keep in step with the first. The two vocabularies are deliberately
separate; §5 is the only sanctioned bridge between them.

### 1.3 What this document does not own

| Section | Content | Owned by |
|---|---|---|
| §7.1 | The eighteen-operation reference with fields and preconditions | T122 `01KZC29CAJ32VSXP9T9RZNKVN1` |
| §7.2 | The eleven-row conflict rules table | T126 `01KZC2D8Q6CAPS2CBS32QQTF5P` |
| §7.3 | The wire message set and per-peer knowledge model | T148 `01KZC2EGC8E02DHP5BD6CQN7QE` |

Those sections are reserved here so that the vocabulary they need already exists when they land.
They extend this document; they never redefine a term in it.

---

## 2. The four core graphs

The four graphs are **four questions about one history**, not four data structures maintained in
parallel. There is a single durable history — the operation graph — and the other three are
answers derived from it plus the records that authorize and observe it.

| Graph | The question it answers | Node | Edge |
|---|---|---|---|
| State | *What did the workspace look like?* | Workspace state | State transition |
| Operation | *Who caused this transition, and what did it follow?* | ChangeSet | Causal parent |
| Dependency | *What exact inputs went into this computation, and what came out?* | Version, derivation | Read, production |
| Trust | *Who was allowed to do this, and what evidence backs it?* | Principal, capability, bundle, approval | Issuance, binding, authorization |

Conflating them is the most likely source of subtle protocol confusion. Two rules keep them
apart:

- **Direction of derivation.** The state graph is a pure function of the operation graph. The
  dependency graph *references* state-graph versions and never mutates them. The trust graph
  *gates* which operation-graph nodes may become a canonical transition in the state graph. No
  arrow runs the other way.
- **No shared node identity.** A node in one graph is never silently reused as a node in another.
  A version appears in the state graph as content and in the dependency graph as an input; those
  are two roles of one identifier, never two definitions of one node.

### 2.1 The state graph — reproducible workspace states

**Nodes.** A *workspace state* is the complete, immutable materialization of one workspace at one
point in its history: the root directory version and, transitively, every directory version, file
version, file manifest and chunk reference reachable from it. A state is named by its *state
hash*, which is the `record ID` of its root directory version — the BLAKE3 digest of that version's
`canonical encoding`, and of nothing else.

**One rule names everything, and it is stated here once.** A content-derived name is the BLAKE3
digest of exactly one byte string, and which byte string it is follows from whether the thing being
named has a schema. An immutable canonical record has one — its `canonical encoding` — and is named
by the digest of those bytes. A byte sequence with no schema, such as a `chunk`, is named by the
digest of those bytes directly, which is what lets any BLAKE3 tool verify a chunk without knowing
this protocol exists. There is no third rule and no second framing, and an external implementation
built from [`protocol/`](../protocol/README.md) can therefore recompute every name it is given.

That replaces a contradiction which stood here until
[ADR-0033](design-decisions.md#adr-0033) ruled on
it: §3.10's `DigestWriter` row described a second framing that `derive_id` used, so this section and
that row could not both be true, and the visible cost was that an outside implementer could
recompute a record's `canonical_digest` and not its `record ID`. The canonical encoding won. **The
rule above is what the protocol says; `crates/mesh-types` has not finished moving onto it.**
`derive_id` still frames a record separately, and the function deriving an `ActorId` from a
`PublicKey` still joins a domain tag to a key without an encoding, until
`01KZFMZC4MTHTT3BW4Y0BW6NYA` lands — a separate change because plan §14.3
rule 3 forbids one run from moving a signed record's definition and its implementation together.
Every §3.10 row that lags says so in its own cell, rather than leaving a reader to compare two
documents.

Three roles single out particular nodes: the *canonical head* (one per workspace), an *actor head*
(one per actor), and a *checkpoint* (a durable actor state that survived the acknowledgement
boundary).

**Two axes, never one enumeration.** An actor head carries two independent facts, and they are
enumerated separately because merging them gives one word two meanings. Its *head state* is where
it sits on the review axis; its *availability state* is what a peer can actually obtain of it. Plan
§4.4 lists the two axes as one six-member list — *local only, metadata replicated, fully content
available remotely, ready for review, superseded, archived* — and that list is split here
deliberately, because `local only` describes retrievability while `ready for review` describes
lifecycle, and a surface that reads one as the other reports a replication fact as a review fact.
The two axes vary freely: a head that is `ready for review` may be `local only`, and a `superseded`
head may still be `content available`. The split has one consequence worth naming: the review axis
needs a member for a head that has not been offered for review, which the plan's six-member list
has no room for, so `working` is added here and is the one member of either axis with no
counterpart in plan §4.4. TL-9 (§6) rejects any future row re-merging the two axes, and it runs.

**Edges.** A *state transition* is a directed edge from a base state to a resulting state,
labelled with the ChangeSet that produced it. The edge set is the applied subset of the operation
graph; a state transition never exists without an operation-graph node justifying it.

**Invariants.**

| ID | Invariant | Consequence if violated |
|---|---|---|
| SG-1 | A state node is a pure function of the causal set reachable from it. Materializing the same causal set on any peer, in any delivery order, produces the same state hash. | Two peers disagree about the same history. |
| SG-2 | A state node is immutable. Its content never changes after it is named. | An approval no longer refers to exact bytes. |
| SG-3 | A state reachable from any retained root stays reachable until an explicit retention policy permits deletion — see [charter P5](charter.md#p5--no-valid-work-is-silently-discarded). | Valid work is silently discarded; a collector that removes reachable content is a P0. |
| SG-4 | The state transition relation is acyclic. No state is its own ancestor. | History becomes untraversable. |
| SG-5 | Within a state node, directory ancestry is acyclic: the materialized tree is a tree. | Concurrent directory moves produce an unmaterializable state. |
| SG-6 | Object identity is not a function of path. A rename or move yields the same object — see [charter P6](charter.md#p6--paths-are-not-file-identities). | A rename severs history or degrades into delete-and-create. |
| SG-7 | The canonical head sequence is totally ordered: each canonical state has exactly one canonical predecessor. | Two canonical heads exist; "the shared version" stops meaning one thing. |
| SG-8 | A state transition is observable in full or not at all. No observer sees a partial advance. | A reader sees a workspace that never existed. |
| SG-9 | No canonical state references content that is not verified and retrievable. | The shared version cannot be materialized. |

### 2.2 The operation graph — causality and attribution

**Nodes.** A *ChangeSet* is the atomic, signed unit of change: exactly one actor, an actor
sequence number, a set of causal parents, a base head, a resulting head, an ordered set of
operations, the policy epoch it was produced under, a hybrid logical time, and a signature. A
ChangeSet cannot be constructed without its causal parents, base head and policy epoch.

An *operation* is a member of the closed operation vocabulary — the distributable verbs of the
protocol. Operations are the **content** of a node, not nodes themselves: causality is tracked at
ChangeSet granularity.

A *raw write* — a `write(2)` against a mounted workspace — is explicitly **not** an operation. Raw
writes are coalesced locally into durable file versions, so the vocabulary describes meaningful
transitions rather than syscalls.

**Edges.** A *causal parent* edge points from a ChangeSet to each ChangeSet its author had already
applied when it was produced. The edge set is a DAG. Two ChangeSets are *concurrent* exactly when
neither is an ancestor of the other; concurrency is a structural fact, never a timing observation.

**Invariants.**

| ID | Invariant | Consequence if violated |
|---|---|---|
| OG-1 | Exactly one actor signs a ChangeSet. Attribution is a signed field, never inferred. | "Which agent did this?" stops being answerable. |
| OG-2 | Per actor, sequence numbers are strictly increasing and never reused; a gap is detectable by any peer. | Silent loss of an actor's work becomes undetectable. |
| OG-3 | A ChangeSet is applied only after every causal parent is applied. A child that arrives first is buffered as a known-missing dependency and is never dropped or garbage-collected as invalid. | Delivery order changes the resulting state. |
| OG-4 | Applying a known ChangeSet again has no additional effect, verified by state hash. | Duplicate delivery corrupts state. |
| OG-5 | Any two delivery orders that respect causality reach the same head. | Convergence fails; peers fork permanently. |
| OG-6 | Neither causality nor head advancement ever consults wall-clock time. Hybrid logical time is metadata and a total-order tiebreak, never evidence of causality. | A wrong system clock changes history. |
| OG-7 | A raw write is never a graph node and never crosses the wire as an operation. | The vocabulary becomes a syscall trace and stops being reviewable. |
| OG-8 | The graph is append-only. A ChangeSet is never edited or deleted; a correction is a new ChangeSet. | Signed records stop being permanent, and every downstream signature is voidable. |
| OG-9 | A ChangeSet's resulting head must follow from its base head and its operations, and this is checked on construction and on receipt — never asserted by its author. | A peer can fabricate a state it never computed. |
| OG-10 | A conflict is a property of a concurrent pair under a conflict rule, resolved identically on every peer from the operation set alone. | Two peers resolve the same conflict differently. |

### 2.3 The dependency graph — exact inputs and derived outputs

This graph is what makes the context claim defensible. It records, per computation, the **exact
versions** that were exposed to it and the outputs that were derived from them.

**Nodes.**

- A *version* — the same identifier the state graph names as content, here in its role as an input.
- A *derivation* — one computation: its kind, its tool or model identity, its configuration digest,
  its determinism flag, and its outputs. Parsing, indexing, diffing, embedding and summarising are
  all derivations.

**Edges.**

- A *read observation*: from a session or derivation to a version, labelled with the *region* it
  covered, the *attribution confidence* of the observation, and the read's declared purpose.
- A *production* edge: from a derivation to each output it produced.
- A *derived-from* edge: from a derivation to another derivation whose output it consumed. Chains
  are traversed transitively for invalidation.

**Invariants.**

| ID | Invariant | Consequence if violated |
|---|---|---|
| DG-1 | A read observation names an exact version, never a path alone. | Staleness cannot be computed. |
| DG-2 | Attribution confidence is set by the observing surface and never upgraded afterwards. An inferred read never records exact-integrated confidence — see [charter P10](charter.md#p10--reliability-claims-must-be-measurable) and [charter §6](charter.md#6-poc-non-goals). | The system overstates what it knows, which is the whole product's credibility. |
| DG-3 | A derivation records every input version and its configuration digest, or it is flagged non-deterministic. | Reuse becomes unsound. |
| DG-4 | When an input version is superseded, every transitively dependent derivation moves to a non-valid validity state. Validity never silently remains valid. | Agents act on stale derived context. |
| DG-5 | Only a derivation flagged deterministic, with identical input versions and an identical configuration digest, may be reused instead of recomputed. The flag is set by the producer, never inferred. | Non-deterministic output is cached and served as fact. |
| DG-6 | The derived-from relation is acyclic: a derivation is never its own transitive input. | Invalidation does not terminate. |
| DG-7 | Context supplied to a model call is bounded and versioned. An oversized request is refused or explicitly chunked — never silently truncated — see [charter P8](charter.md#p8--the-complete-workspace-never-enters-model-context-automatically). | Confidently wrong answers, with no signal that anything was dropped. |
| DG-8 | Speculative read-ahead is recorded distinguishably from a real read. | The ledger inflates and every downstream measurement is wrong. |

### 2.4 The trust graph — authority, evidence and canonical transitions

**Nodes.**

- A *principal*: an actor, identified by its actor key, together with the device it runs on. The
  *device* here is the machine (§3.1) — never an actor kind. The actor kind plan §4.2 spells
  `Device` is the register's `device actor`, and the rename is what keeps `principal` from
  composing a thing with itself.
- A *capability*: a scoped, expiring grant of authority, held by a principal.
- A *review bundle*: the immutable, deterministically computed unit of review.
- A *validation result*: the outcome of one independent validation run against a bundle.
- An *approval envelope*: a human's signed statement binding exactly what was reviewed.
- A *publication receipt*: the verifiable record that a canonical transition occurred.
- A *canonical transition*: the admitted advance of the canonical head.

**Edges.**

| Edge | From | To | Meaning |
|---|---|---|---|
| Sponsors | Human actor | Agent actor | The human accountable for the agent's authority. |
| Issues | Principal | Capability | Delegation, always narrowing. |
| Scopes | Capability | Workspace, action set, policy epoch | The exact bound of the grant. |
| Attests | Validation result | Review bundle | Independent evidence about reviewed bytes. |
| Binds | Approval envelope | Workspace, expected canonical head, reviewed actor head, review bundle, selected changes, conflict resolutions, validation digest, policy epoch, approving actor | The nine fields the signature covers — plan §4.7. See *The bound field count* below. |
| Authorizes | Approval envelope | Canonical transition | The only path by which canonical state advances. |
| Witnesses | Publication receipt | Canonical transition | Independently verifiable proof it happened. |
| Revokes | Policy epoch | Capability | Authority withdrawn without a global online step. |

**The bound field count.** What an approval cryptographically binds is the most security-critical
statement in this document, and three sources state it with three different numbers. They are
reconciled here, once, and nowhere else:

| Source | What it says | Count |
|---|---|---|
| Plan §4.7 `ApprovalEnvelope` | `workspace_id`, `expected_canonical_head`, `reviewed_actor_head`, `review_bundle_id`, `selected_changes`, `conflict_resolutions`, `validation_digest`, `policy_epoch`, `approved_by`, `signature` | 10 struct members |
| This document | The nine the signature covers — every member above except `signature`, which is the signature *over* them | **9 bound fields** |
| [charter P7](charter.md#p7--approval-refers-to-exact-bytes) | The exact actor state reviewed, the exact canonical base, the exact selected changes, the exact conflict resolutions, the validation evidence shown to the approver | 5 clauses |

**Nine is the number of bound fields**, and it is the only one this document uses. Ten is the same
set plus the signature, so nine and ten are one fact stated about two things. Charter P7's five are
a strict subset of the nine — it omits the workspace, the review bundle, the policy epoch and the
approving actor because none of them is product-visible, and it contradicts nothing about the five
it does state. Five, nine and ten are three views of one binding set. A source disagreeing with this
table is a defect in that source, and changing the bound set is a protocol change under §8, not an
edit.

**Invariants.**

| ID | Invariant | Consequence if violated |
|---|---|---|
| TG-1 | Every authority-bearing action requires an unexpired capability valid in the current policy epoch, checked on every path. | Authority becomes ambient. |
| TG-2 | A capability cannot be widened after issuance. Widening is unrepresentable in the type, not merely rejected at runtime. | Delegation becomes privilege escalation. |
| TG-3 | No agent-scoped key can produce a valid approval envelope. The capability is unrepresentable, not merely denied — see [charter P1](charter.md#p1--work-is-automatic-publication-is-explicit) and [charter P2](charter.md#p2--every-actor-works-privately). | An agent publishes to the shared version; the product's central promise fails. |
| TG-4 | Modifying any bound field of an approval envelope invalidates its signature. | An attacker substitutes bytes the human never saw. |
| TG-5 | A result produced by the actor that produced the change is agent-reported evidence and never satisfies a validation requirement on its own. A requirement with no independent result is reported as unvalidated, never as passing. | A system optimising to look correct grades its own work. |
| TG-6 | A publication receipt is verifiable from the public key and the encoding specification alone, without Mesh's own code path. | The trust claim is unauditable. |
| TG-7 | Policy epochs are strictly increasing; authority granted under a prior epoch is not valid in a later one, and rotation requires no peer to be online. | Revocation depends on connectivity. |
| TG-8 | A hard guard blocks publication unconditionally and cannot be disabled by configuration. A warning is overridable only by an explicit human action recorded with its reason. | The override affordance becomes a way to publish corrupt state. |
| TG-9 | A canonical transition is admitted only if its expected canonical head equals the current canonical head. Of N concurrent attempts against the same expected head, exactly one succeeds and the rest fail as stale, naming the current head. | A publication silently rebases bytes the human did not see. |
| TG-10 | An approval envelope is single-use; a replayed envelope is rejected. | Approval becomes a reusable capability. |

### 2.5 How the graphs compose

```text
      trust graph                operation graph              state graph
  approval envelope  --gates-->  ChangeSet (canonical) --applies--> canonical head
  capability         --gates-->  ChangeSet (actor)     --applies--> actor head
                                       |
                                       | produces versions
                                       v
                                 dependency graph
                     read observations --> versions --> derivations
```

Read the composition as three one-way rules:

1. **State is derived, never authored.** No surface writes a workspace state directly; states
   exist only as the materialization of an applied causal set (SG-1).
2. **Trust gates, it does not compute.** The trust graph decides *whether* a ChangeSet becomes a
   canonical transition. It never changes what that ChangeSet says (TG-4).
3. **Dependency observes, it does not participate.** Read observations and derivations never
   change a state, a head, or an authority. Losing the dependency graph loses knowledge, never
   work — see [`docs/consistency.md`](consistency.md).

---

## 3. The terminology register

One row, one term, one definition. Rows are grouped for reading only; the term column is unique
across the **whole** register.

Column meanings: **Graph** names which of the four graphs the term belongs to (`—` for
cross-cutting terms). **Home** names the crate, document or surface that owns the term's
implementation.

<!-- terminology:begin -->

### 3.1 Substrate and identity

| Term | Definition | Graph | Home |
|---|---|---|---|
| `Mesh` | The product: a local-first workspace system in which every human and agent has a continuously durable private state, private states synchronize in real time, and only an exact human-reviewed state can advance the protected shared version. | — | — |
| `CWP` | The Causal Workspace Protocol: the open wire and record protocol Mesh implements, specified so an external party can build an interoperating client. | — | `protocol/` |
| `workspace` | The unit of collaboration: one shared project with one canonical head, a set of actors and a policy epoch. Not a directory — a directory is one materialization of one workspace state. | — | `mesh-types` |
| `actor` | An authenticated participant that can hold a private state and author ChangeSets, identified by an actor key. | — | `mesh-types` |
| `actor kind` | The classification of an actor as exactly one of: `human`, `device actor`, `agent`, `agent-run`, `automation`, `validator`, `service`. Kind constrains which capabilities the actor may hold. Every member has its own row below; a member with no row is a specification gap, not a self-explanatory word. | trust | `mesh-types` |
| `human` | An actor kind whose key is held by a person under OS key isolation, and the only kind that can hold the approval capability. | trust | `mesh-types` |
| `device actor` | An actor kind whose authority derives from a device key rather than from a person or a model, so that a change originating from a machine itself — rather than from any human or agent running on it — is attributable to that machine. Plan §4.2 spells this kind `Device`; it is renamed here because `device` already names the machine and one word may not carry two definitions. | trust | `mesh-types` |
| `agent` | An actor kind representing a long-running model-driven participant with a sponsor human and no approval capability. | trust | `mesh-types` |
| `agent-run` | An actor kind representing one bounded execution of an agent, so work can be attributed to a single run rather than to the agent as a whole. | trust | `mesh-types` |
| `automation` | An actor kind representing a rule-driven, non-model participant — a scheduled job, a hook or a trigger — acting under a capability delegated by a sponsor human. It authors ChangeSets under its own identity so its work is not attributed to the person who configured it, and it holds no approval capability. | trust | `mesh-types` |
| `validator` | An actor kind that executes validation runs and can attest to a review bundle but can never author a canonical transition. | trust | `mesh-types` |
| `service` | An actor kind representing a first-party Mesh component participating as an actor in its own right, such as a relay's workspace-scoped identity. It holds only the capabilities its function requires and never the approval capability, which is what keeps a component of the system from being an authority over canonical state. | trust | `mesh-types` |
| `device` | The physical or virtual machine an actor runs on, holding a device key distinct from any actor key. Never an actor kind — an actor that *is* a machine is a `device actor`. | trust | `mesh-crypto` |
| `session` | A bounded interval of one actor's activity, used to group read observations and ChangeSets for attribution. | operation | `mesh-types` |
| `object` | The stable identity of a file or directory, independent of any path it has ever had. | state | `mesh-state` |
| `version` | An immutable, content-derived snapshot of one object's content and metadata at one point in its history. | state | `mesh-types` |
| `file version` | A version of a file object: its file manifest, its portable metadata and its provenance. | state | `mesh-types` |
| `directory version` | A version of a directory object: its set of directory entries, each naming a child object and its version. | state | `mesh-types` |
| `directory entry` | The binding of a name to a child object inside a directory version. Renames and moves operate on entries, never on objects. | state | `mesh-state` |
| `file manifest` | The ordered list of chunk references that reconstructs a file version's bytes exactly. | state | `mesh-cas` |
| `chunk` | A content-addressed, compressed byte range — the unit of storage, transfer and deduplication. | state | `mesh-chunking` |
| `chunk reference` | A manifest entry naming a chunk by content digest together with its position and length. | state | `mesh-cas` |
| `entity ID` | A UUIDv7 naming a mutable entity — workspace, session, object, capability. Never derived from content, because the content changes. **Never an `actor`**, though this row listed one until the two normative sources were reconciled: plan §4.2's `ActorId` won, because an actor's identity is its `actor key`, and a name not derived from that key would have to be bound to it by a record somebody could later rewrite — after which past authorship would verify against a key its signer never held. The cost is that a key cannot be rotated in place: rotating one names a new participant. Ruled in [ADR-0003](design-decisions.md#adr-0003). | — | `mesh-types` |
| `record ID` | The `content digest` of an immutable canonical record's `canonical encoding`, which is that record's name — version, file manifest, ChangeSet, head, bundle, approval, receipt, and an actor, which is named by the immutable record that is its `actor key` and never by the mutable actor record around it. A `chunk` is not on that list: it has no schema, so it is named by a `content digest` of its own bytes instead. Recomputes identically from content, which is what makes such records self-verifying, and recomputes from a published document rather than from an implementation, which is what makes them externally verifiable. Ruled in [ADR-0033](design-decisions.md#adr-0033); `crates/mesh-types` lags the rule until `01KZFMZC4MTHTT3BW4Y0BW6NYA`. | — | `mesh-types` |
| `content digest` | The BLAKE3 hash of a byte sequence, used to name chunks and to verify content on receipt, and — applied to a record's `canonical encoding` — to produce that record's `record ID`. | — | `mesh-crypto` |
| `canonical encoding` | The deterministic serialization every signed record is encoded with — CBOR for signed records, protobuf on the wire — such that two independent implementations produce byte-identical output. | — | `mesh-types` |
| `test vector` | A published input/output pair that pins the canonical encoding, so an external implementation can prove byte-identical agreement. | — | `protocol/test-vectors` |
| `actor sequence number` | A strictly increasing per-actor counter carried by every ChangeSet, making omission detectable by any peer. | operation | `mesh-operations` |
| `hybrid logical time` | A monotonic timestamp carried as ChangeSet metadata for display and total-order tiebreaking. It never decides causality and never affects head advancement (OG-6). | operation | `mesh-types` |

### 3.2 State graph terms

| Term | Definition | Graph | Home |
|---|---|---|---|
| `state graph` | The graph whose nodes are workspace states and whose edges are state transitions; the answer to *what did the workspace look like*. | state | `mesh-state` |
| `workspace state` | The complete immutable materialization of one workspace at one point in its history, named by its state hash. | state | `mesh-materializer` |
| `state hash` | The content-derived name of a workspace state: the `record ID` of its root directory version, and therefore the `content digest` of that version's `canonical encoding`. | state | `mesh-materializer` |
| `state transition` | A directed edge from a base state to a resulting state, labelled with the ChangeSet that produced it. | state | `mesh-state` |
| `materialization` | The deterministic computation of a workspace state from an applied causal set. | state | `mesh-materializer` |
| `head` | A named workspace state that some party currently treats as current. | state | `mesh-state` |
| `actor head` | The head of one actor's private state — that actor's current tip, advanced only by that actor's own ChangeSets and by incorporated canonical advances. | state | `mesh-state` |
| `canonical head` | The single protected shared head of a workspace, advanced only by a canonical transition. | state | `mesh-approval` |
| `head state` | Where an actor head sits on the review axis, exactly one of: `working`, `ready for review`, `superseded`, `archived`. Independent of `availability state`, which is the retrievability axis; the two are never merged into one enumeration. | state | `mesh-state` |
| `checkpoint` | An actor head that has crossed the acknowledgement boundary and is therefore durable. Created automatically; never created by a user. | state | `mesh-store` |
| `acknowledgement boundary` | The exact point in the `durable commit sequence` after which a checkpoint is promised to survive a crash, and before which nothing is promised. | state | `mesh-store` |
| `durable commit sequence` | The fixed order of steps by which a local write becomes durable — chunk write, hash verification, atomic promotion, index transaction, outbox write, commit — such that a crash at any step leaves no durable head referencing unpromoted content. | state | `mesh-store` |
| `retained root` | A member of the set the collector must treat as live: canonical heads, active actor heads, review bundles, unresolved conflicts, named restore points, retention-policy states and offline-peer watermarks. | state | `mesh-store` |
| `reachable` | The property of being referenced, transitively, from at least one retained root. Reachable content is never collected. Seven of the eight retained roots close over the **whole causal ancestry** of what they name; only the manifest root closes over just the chunks it lists, because a manifest has no causal parent. Decided in `01KZG94AFZP999E8F8ZF2X574E`, which also settles that nothing inside a root's closure is prunable, so retention retires roots rather than filtering content. | state | `mesh-cas` |
| `restore point` | A named workspace state a human can return the workspace to, held as a retained root for as long as the name exists. | state | `mesh-store` |
| `tombstone` | The record that an object was deleted, retained so that a concurrent edit to the same object preserves both facts rather than resolving to silence. | state | `mesh-conflicts` |
| `portable metadata` | The subset of file metadata Mesh carries across platforms without loss, and the declared rules for what is dropped elsewhere. | state | `mesh-materializer` |

### 3.3 Operation graph terms

| Term | Definition | Graph | Home |
|---|---|---|---|
| `operation graph` | The append-only `DAG` whose nodes are ChangeSets and whose edges are causal parents; the answer to *who caused this transition, and what did it follow*. | operation | `mesh-operations` |
| `DAG` | A directed acyclic graph. Used of the causal-parent edge set, whose acyclicity is what makes the causal set of a head finite and its materialization terminating. | operation | `mesh-operations` |
| `operation` | One verb of the closed operation vocabulary — a distributable, reviewable transition such as creating, editing, renaming, moving or deleting an object. Enumerated in §7.1. | operation | `mesh-operations` |
| `operation vocabulary` | The closed, versioned set of eighteen operations. Adding a member is a protocol change requiring a test vector, a compatibility test and protocol review in the same change. | operation | `mesh-operations` |
| `ChangeSet` | The atomic signed unit of change by exactly one actor: actor sequence number, causal parents, base head, resulting head, operations, policy epoch, hybrid logical time and signature. | operation | `mesh-operations` |
| `causal parent` | An edge from a ChangeSet to a ChangeSet its author had already applied when producing it. | operation | `mesh-operations` |
| `base head` | The head a ChangeSet's operations were computed against. | operation | `mesh-operations` |
| `resulting head` | The head a ChangeSet's operations produce when applied to its base head; checked on construction and on receipt, never trusted from its author (OG-9). | operation | `mesh-operations` |
| `causal set` | The transitive closure of causal parents of a head — everything that head depends on. | operation | `mesh-state` |
| `concurrent` | The relation between two ChangeSets when neither is an ancestor of the other. A structural fact, never a timing observation. | operation | `mesh-state` |
| `causal delivery` | The delivery discipline under which a ChangeSet is applied only after every causal parent has been applied. | operation | `mesh-sync-engine` |
| `known-missing dependency` | A received ChangeSet buffered because a causal parent has not arrived, held visibly and indefinitely rather than dropped or collected. | operation | `mesh-state` |
| `convergence` | The property that any two peers holding the same causal set reach the same head, regardless of delivery order. | operation | `mesh-state` |
| `raw write` | A syscall-level write against a mounted workspace. Coalesced locally into a file version; never a ChangeSet, never distributed (OG-7). | operation | `mesh-fuse` |
| `coalescing` | The local aggregation of raw writes and save events into one durable file version, so the operation graph records meaningful transitions rather than syscalls. | operation | `mesh-store` |
| `conflict` | A concurrent pair that a conflict rule declares cannot be merged silently. Every outcome is reproducible from the operation set alone. | operation | `mesh-conflicts` |
| `conflict rule` | One row of the conflict table: a named concurrent pattern and its mandated outcome — merge, preserve both, tombstone plus edited version, dual identity, or deterministic cycle resolution. Enumerated in §7.2. | operation | `mesh-conflicts` |
| `preserve both` | The mandated outcome for a conflict where no automatic winner is safe: every durable version stays reachable and the case surfaces for human attention. | operation | `mesh-conflicts` |
| `attribution` | The signed identification of the actor, session and run that authored a ChangeSet. Never inferred, never reconstructed after the fact. | operation | `mesh-operations` |

### 3.4 Dependency graph terms

| Term | Definition | Graph | Home |
|---|---|---|---|
| `dependency graph` | The graph of exact inputs exposed to computations and outputs derived from them; the answer to *what went into this, and what came out*. | dependency | `mesh-derivations` |
| `read observation` | The record that a session or derivation read a specific version: workspace, actor, session, object, version, region, attribution confidence and purpose. | dependency | `mesh-context-ledger` |
| `region` | The extent of a read within a version, expressed as exactly one of: `whole file`, `byte range`, `text section`, `PDF page`, `spreadsheet cell range`, `code symbol`, `unknown extent`. | dependency | `mesh-context-ledger` |
| `attribution confidence` | The declared reliability of a read observation, exactly one of: `exact integrated read`, `exact filesystem range`, `filesystem read-ahead`, `process inferred`, `recovery detected`, `unknown provenance`. Set by the observing surface and never upgraded (DG-2). | dependency | `mesh-context-ledger` |
| `derivation` | One recorded computation over exact versioned inputs: its kind, tool or model identity, configuration digest, determinism flag and outputs. | dependency | `mesh-derivations` |
| `configuration digest` | The content-derived summary of every parameter, prompt, tool version and setting a derivation ran under, so that "same inputs" is a checkable claim. | dependency | `mesh-derivations` |
| `determinism flag` | The producer-declared assertion that a derivation returns identical outputs for identical inputs and configuration digest. Never inferred (DG-5). | dependency | `mesh-derivations` |
| `validity state` | The current standing of a derivation's outputs, exactly one of: `valid`, `possibly stale`, `definitely stale`, `revalidated`. | dependency | `mesh-derivations` |
| `invalidation` | The propagation of a superseded input version to every transitively dependent derivation's validity state. | dependency | `mesh-context-ledger` |
| `stale read` | A read observation whose observed version is no longer the current version of that object for the reading actor. | dependency | `mesh-context-ledger` |
| `compiled context` | The bounded, versioned set of content assembled for one model call, with every element traceable to an exact version (DG-7). | dependency | `mesh-context-compiler` |
| `run memory` | The compact, versioned summary of an agent run, retained so a later run can resume without re-reading the workspace. | dependency | `mesh-context-compiler` |
| `token accounting` | The per-actor, per-run ledger of tokens supplied to and returned by model calls, attributed to the compiled context that caused them. | dependency | `mesh-daemon` |

### 3.5 Trust graph terms

| Term | Definition | Graph | Home |
|---|---|---|---|
| `trust graph` | The graph of principals, capabilities, evidence and authorizations; the answer to *who was allowed to do this, and what backs it*. | trust | `mesh-policy` |
| `principal` | An actor together with the `device` it acts from, as the holder of capabilities. `device` here is always the machine, never an actor kind; for a `device actor` the machine and the actor are one entity in two roles. | trust | `mesh-policy` |
| `actor key` | The Ed25519 key pair that signs an actor's ChangeSets and, for a human actor, approval envelopes. Isolated in OS key storage. | trust | `mesh-crypto` |
| `device key` | The Ed25519 key pair identifying a device to a relay, distinct from every actor key so that device compromise and actor compromise are separable. | trust | `mesh-crypto` |
| `capability` | A scoped, expiring grant of authority to a principal. Narrowable by delegation and never widenable after issuance (TG-2). | trust | `mesh-policy` |
| `capability token` | The signed, verifiable encoding of a capability that a peer can check without contacting its issuer. | trust | `mesh-crypto` |
| `policy epoch` | The monotonically increasing generation of a workspace's authority configuration. Rotation invalidates the prior epoch's grants without requiring any peer to be online (TG-7). | trust | `mesh-policy` |
| `revocation` | The withdrawal of a capability before its expiry, taking effect through epoch rotation on every peer, online or not. | trust | `mesh-policy` |
| `review bundle` | The immutable, deterministically computed unit of review: the exact base and actor state, the object-level operations, content representations, conflicts, dependency impact, validation results and the actor's optional explanation. Later actor work can never enter an existing bundle. | trust | `mesh-approval` |
| `selection` | The human's chosen subset of a review bundle's changes, expanded only explicitly when a dependency requires it, and never expanded silently. | trust | `mesh-approval` |
| `validation plan` | The deterministic set of `validation run`s selected for a review bundle from its changed file types, paths, operation types, change volume, detected tooling, policy, conflicts and input staleness. | trust | `mesh-validator` |
| `validation profile` | The one-time, human-approved set of project commands Mesh is permitted to execute. Nothing detected runs before the profile is approved. | trust | `mesh-validator` |
| `validation run` | One execution of one command from the `validation profile` against a `review snapshot`, in a recorded environment. Independent exactly when its executing actor is not the actor that produced the change (TG-5). | trust | `mesh-validator` |
| `review snapshot` | The immutable materialization of the exact actor state a review bundle names, which every `validation run` for that bundle executes against. Later actor work never changes it. | trust | `mesh-validator` |
| `validation result` | The authoritative outcome of one `validation run` against a `review snapshot`, recording command, environment digest, exit state and artifacts. | trust | `mesh-validator` |
| `agent-reported evidence` | A result asserted by the actor that produced the change. Displayed as evidence, distinguished in data and in the review surface, and never sufficient to satisfy a validation requirement (TG-5). | trust | `mesh-validator` |
| `approval envelope` | The human-signed record whose signature covers exactly nine bound fields: the workspace, the expected canonical head, the reviewed actor head, the review bundle, the selected changes, the conflict resolutions, the validation digest, the policy epoch and the approving actor. Modifying any one of the nine invalidates the signature (TG-4). The count is reconciled against plan §4.7 and charter P7 in §2.4. | trust | `mesh-approval` |
| `publication` | The act of admitting an approved selection into canonical state. | trust | `mesh-approval` |
| `canonical transition` | The admitted advance of the canonical head, authorized by exactly one approval envelope and serialized by compare-and-swap. | trust | `mesh-approval` |
| `compare-and-swap` | The admission rule for a canonical transition: it succeeds only if the expected canonical head equals the current canonical head (TG-9). | trust | `mesh-approval` |
| `stale base` | The failed-swap condition where the expected canonical head no longer matches. Reported with the current head named, and routed to `reclassification` — never silently rebased. | trust | `mesh-approval` |
| `reclassification` | The decision procedure a `stale base` is routed to: it either rebases the selection onto the current canonical head under an explicit rule that provably touches no reviewed byte, or it returns the work for re-review. It never resolves by rebasing silently. | trust | `mesh-approval` |
| `publication receipt` | The independently verifiable record of a canonical transition, checkable from the public key and the encoding specification alone. | trust | `mesh-approval` |
| `hard guard` | A condition under which publication is refused regardless of human intent. Cannot be overridden or disabled by configuration; names the exact condition and object that triggered it. | trust | `mesh-policy` |
| `warning` | A publication condition a human may override by an explicit action that is recorded with its reason. | trust | `mesh-policy` |

### 3.6 Distribution, availability and presence

| Term | Definition | Graph | Home |
|---|---|---|---|
| `peer` | Another device holding some part of the same workspace, reached directly or through a relay. | — | `mesh-sync-engine` |
| `relay` | The service that authenticates devices and actors and forwards metadata and content between peers. It is a transport participant and never an authority over canonical state. | — | `services/relay` |
| `metadata plane` | The message plane carrying heads, ChangeSets, manifests and availability — everything except chunk bytes. | — | `mesh-sync-engine` |
| `content plane` | The message plane carrying chunk bytes, with resumable transfer and integrity verification on receipt. | — | `mesh-sync-engine` |
| `availability state` | What a peer can actually obtain of an actor head, exactly one of: `local only`, `metadata replicated`, `content available`. The retrievability axis, independent of `head state` and never conflated with `presence` on any surface. | — | `mesh-sync-engine` |
| `presence` | The ephemeral, TTL-bounded signal that an actor or device is currently reachable. Never durable, never a durability signal, and never part of canonical history. | — | `mesh-daemon` |
| `knowledge set` | What one peer currently believes another peer holds: contiguous sequence, sparse ChangeSets, head set, manifest and chunk availability, policy epoch and canonical head. | — | `mesh-sync-protocol` |
| `outbox` | The durable per-peer queue written inside the same transaction as the checkpoint it describes, making delivery survive a crash and tolerate duplication and reordering. | — | `mesh-store` |
| `watermark` | The recorded position up to which a peer is known to have received, used both to resume delivery and to keep required content alive for an offline peer. | — | `mesh-store` |
| `anti-entropy` | The periodic reconciliation that compares summaries between peers and repairs gaps that incremental delivery missed. | — | `mesh-sync-engine` |
| `replication` | The transfer of an actor's private state to another device. Replication is not publication — see [charter P3](charter.md#p3--private-state-is-durable-and-remotely-visible). | — | `mesh-sync-engine` |

### 3.7 Consistency classes

Every fact the system holds belongs to exactly one of these four classes, and the class decides its
authority, its ordering and whether it may be lost. The classes are register terms because
[`docs/consistency.md`](consistency.md) is built on them: that document elaborates their properties
and their failure behaviour, and it must not define them a second time. TL-7 (§6) catches a breach
of that rule for any section heading in either document, and it runs.

| Term | Definition | Graph | Home |
|---|---|---|---|
| `private actor state` | One actor's own ChangeSets, actor head and checkpoints. Authoritative for that actor, ordered causally with no global total order, and never lost after the acknowledgement boundary. | state | `mesh-state` |
| `canonical state` | The canonical head together with the totally ordered sequence of canonical transitions that produced it. Distinct from `canonical head`, which is the single state at the tip of that sequence. Linearizable, single-writer through compare-and-swap, never lost. | state | `mesh-approval` |
| `derived state` | Any fact that is a function of `private actor state`, `canonical state` and `presence` — indexes, caches, materialized views, availability computations, read observations and derivations. Rebuildable, never an authority, and lossy without loss of work. | — | `mesh-derivations` |

### 3.8 Adapters, views and interoperability

| Term | Definition | Graph | Home |
|---|---|---|---|
| `workspace adapter` | The trait every filesystem surface implements, together with the conformance suite that defines correct behaviour for all of them. | — | `mesh-fuse` |
| `actor mount` | The filesystem view presenting one actor's own head as a writable directory tree. | — | `mesh-fuse` |
| `shadow view` | A strictly read-only materialization of another actor's head, which alters neither that actor's work nor the canonical head. | — | `mesh-fuse` |
| `external provenance anchor` | The recorded external identity — such as an imported Git commit — that a workspace state was derived from or exported to. | — | `mesh-git-bridge` |
| `event ledger` | The structured, local, append-only record of daemon-observable events, used for diagnostics and support bundles. Never an authority over protocol state. | — | `mesh-daemon` |

### 3.9 Crate names

Each crate name is a term. A crate's public surface implements the terms whose **Home** column
names it.

| Term | Definition | Graph | Home |
|---|---|---|---|
| `mesh-types` | The crate defining entity IDs, record IDs and the entity model. No storage and no network dependency, asserted by the architecture check. | — | `crates/mesh-types` |
| `mesh-crypto` | The crate implementing actor keys, device keys, signing, verification, content digests and capability tokens. | trust | `crates/mesh-crypto` |
| `mesh-operations` | The crate implementing the `operation vocabulary`, the published schemas and `canonical encoding` of its members, `actor sequence number` issuing and observation, write coalescing, and ChangeSet construction whose resulting `head` is derived rather than supplied. No platform-adapter dependency — no dependency of any kind, asserted at compile time against its own manifest. | operation | `crates/mesh-operations` |
| `mesh-state` | The crate implementing actor heads, object identity, causal parent resolution and head advancement. | state | `crates/mesh-state` |
| `mesh-materializer` | The crate implementing materialization of an applied causal set into a workspace state. | state | `crates/mesh-materializer` |
| `mesh-conflicts` | The crate implementing the conflict rules table and its preservation outcomes. | operation | `crates/mesh-conflicts` |
| `mesh-cas` | The crate implementing content-addressed storage with atomic, hash-verified chunk promotion. | state | `crates/mesh-cas` |
| `mesh-chunking` | The crate implementing whole-file and content-defined chunking with content digests and compression. | state | `crates/mesh-chunking` |
| `mesh-store` | The crate implementing the local durable index: heads, manifests, outbox, watermarks and review state. | state | `crates/mesh-store` |
| `mesh-sync-protocol` | The crate defining the CWP wire message set and the knowledge set. | — | `crates/mesh-sync-protocol` |
| `mesh-sync-engine` | The crate implementing the metadata plane, content plane, outbox delivery, reconciliation and anti-entropy. | — | `crates/mesh-sync-engine` |
| `mesh-policy` | The crate implementing capabilities, policy epochs, revocation and the publication hard guards. | trust | `crates/mesh-policy` |
| `mesh-approval` | The crate implementing review bundles, selection, the approval envelope, compare-and-swap publication and receipts. | trust | `crates/mesh-approval` |
| `mesh-context-ledger` | The crate implementing read observations, regions, attribution confidence and invalidation. | dependency | `crates/mesh-context-ledger` |
| `mesh-context-compiler` | The crate implementing bounded, versioned context retrieval and run memory. | dependency | `crates/mesh-context-compiler` |
| `mesh-derivations` | The crate implementing derivations, validity states and deterministic reuse. | dependency | `crates/mesh-derivations` |
| `mesh-git-bridge` | The crate implementing Git import, export and external provenance anchors. Optional by design; no core crate depends on it — see [charter P9](charter.md#p9--git-is-an-adapter). | — | `crates/mesh-git-bridge` |
| `mesh-validator` | The crate implementing the validation profile, validation plan and independent validation runs. | trust | `crates/mesh-validator` |
| `mesh-daemon` | The crate implementing the local daemon: its IPC surface, event ledger, counters, telemetry, presence and crash diagnostics. | — | `crates/mesh-daemon` |
| `mesh-fuse` | The crate implementing the Linux FUSE workspace adapter, which is the filesystem conformance reference. | — | `crates/mesh-fuse` |
| `mesh-fskit-ffi` | The crate exposing the stable C ABI the macOS FSKit extension calls into. | — | `crates/mesh-fskit-ffi` |
| `mesh-mcp` | The crate implementing the Mesh MCP server, giving an agent inspection, search, read, change and checkpoint surfaces with exact attribution. | dependency | `crates/mesh-mcp` |
| `mesh-agent-sdk` | The crate implementing the integrated-agent SDK and the generic CLI wrapper, through which reads become exact read observations. | dependency | `crates/mesh-agent-sdk` |
| `mesh-simulator` | The crate implementing the deterministic simulator with replaceable clock, disk, network, scheduler and key store. | — | `crates/mesh-simulator` |
| `mesh-bench` | The crate implementing the reproducible benchmark harness and its result schema. | — | `crates/mesh-bench` |

### 3.10 Public items

Every public item exported by a crate in `crates/` resolves to a term. Most of the twenty-five
crates are still scaffolds exporting one item each; `mesh-types`, `mesh-store`,
`mesh-sync-protocol`, `mesh-cas`, `mesh-crypto`, `mesh-state` and `mesh-operations` are implemented
and their non-`CRATE_NAME` items are the rows below, in that order, save `ChangeSet`, whose row is
§3.3's because the concept is older than the type.

A name published by two crates has one row, not two, because a term is unique across the whole
register and TL-1 is what says so. `mesh-operations` mirrors part of `mesh-types`' surface rather
than importing it — `ObjectId`, `CborWriter`, `encode_canonical` and the rest of that list — and
each of those names keeps the single row it already had, homed at the crate that defined it. TL-10
reads the home cell, so one row satisfies both crates and neither publication is hidden.

**Every implemented crate is in this table, and the gate is what says so.** An earlier revision
carried a paragraph here naming three implemented crates whose items had no row; those items are
rows now, and the paragraph is gone rather than reworded, because a standing caveat about an
incomplete table is a thing a reader learns to skip.

**TL-5 checks these rows in one direction and TL-10 in the other.** TL-5 reads what a crate root
declares — including behind an `async`, `unsafe`, `const` or `extern` qualifier — and what it
re-exports by name from a private module, resolving each re-exported name to the module that
declares it, so deleting any one row below turns the gate red and names the item that lost its
definition. TL-10 walks the rows instead: a row here homed at one crate that publishes no item of
that name is a finding, so a renamed or deleted item cannot leave its row behind. Until
`01KZCV213VGTK670DPYC1X2M9Y` the first direction stopped at declarations, the implemented crates
declared nothing but `CRATE_NAME` in `src/lib.rs`, and the table was complete only because its
authors made it complete. The first run of the widened check found `MAX_NESTING` missing, which is
what an unchecked register does.

What is still not reached is an item made public through a public module rather than re-exported
from the crate root — §6's TL-5 ceiling bullet, unchanged in that respect. That bullet is the second
of the four, not the third; the sentence replaced here called it the third, and was already wrong
about that before this change.

| Term | Definition | Graph | Home |
|---|---|---|---|
| `CRATE_NAME` | The public constant every crate exports carrying its own crate name, so that a scaffolded crate still has one verifiable behaviour under test. | — | `crates/*` |
| `Uuid` | The 128-bit RFC 9562 value an `entity ID` is carried in. A container, not a promise: it holds any sixteen bytes, and the version-7 requirement is enforced one level up. | — | `crates/mesh-types` |
| `UuidParseError` | Why a hyphenated `Uuid` failed to parse: wrong length, a missing hyphen, or a character outside the hex alphabet. | — | `crates/mesh-types` |
| `EntityIdError` | Why a `Uuid` was refused as an `entity ID`: wrong version, wrong variant, or not a UUID at all. | — | `crates/mesh-types` |
| `WorkspaceId` | The `entity ID` of a `workspace`. | — | `crates/mesh-types` |
| `SessionId` | The `entity ID` of a `session`. | operation | `crates/mesh-types` |
| `ObjectId` | The `entity ID` of an `object`. | state | `crates/mesh-types` |
| `CapabilityId` | The `entity ID` of a `capability`. | trust | `crates/mesh-types` |
| `Digest32` | The opaque thirty-two-byte value a `content digest` produces, rendered as sixty-four lowercase hex characters. Every `record ID` wraps one. | — | `crates/mesh-types` |
| `DigestParseError` | Why a hex `Digest32` failed to parse: wrong length, or a character outside the hex alphabet. | — | `crates/mesh-types` |
| `DigestHasher` | Incremental digest state: the half of the digest seam that absorbs bytes and produces a `Digest32`. | — | `crates/mesh-types` |
| `ContentDigest` | A thirty-two-byte digest algorithm: the half of the digest seam that names one. Replacing the algorithm is a new implementation of this trait, never an edit to a call site. | — | `crates/mesh-types` |
| `Blake3` | The `ContentDigest` implementation Mesh uses, and the only one today. | — | `crates/mesh-types` |
| `Blake3Hasher` | The `DigestHasher` state `Blake3` drives: the BLAKE3 tree hasher, implemented in-crate so that `mesh-types` carries no dependency. | — | `crates/mesh-types` |
| `DomainTag` | The versioned label naming a record type, encoded as the first element of that record's `canonical encoding`, so that two records with identical field values in different domains never share an identifier. | — | `crates/mesh-types` |
| `DigestWriter` | The retired identity framing: a `DomainTag`, then every variable-length field preceded by its length. It produced a second byte string for every record, and no published document ever described it, which is why [ADR-0033](design-decisions.md#adr-0033) retired it rather than publishing it. Present in `crates/mesh-types` until `01KZFMZC4MTHTT3BW4Y0BW6NYA` deletes it; it derives no name this document defines. | — | `crates/mesh-types` |
| `Absorb` | The ability to contribute fields to a `DigestWriter` in a fixed order. Retired with it under [ADR-0033](design-decisions.md#adr-0033); `CanonicalEncode` is the one remaining way a record produces bytes. | — | `crates/mesh-types` |
| `CanonicalRecord` | An immutable record whose identity is a digest of its own content: it names the `record ID` type it produces and the `DomainTag` it is encoded under. Its name is the `content digest` of its `canonical encoding`, so a record's schema is the only thing that decides what it is called, and no `entity ID` can be named by it. | — | `crates/mesh-types` |
| `derive_id` | The function that turns a `CanonicalRecord` into its `record ID`: the `content digest` of that record's `canonical encoding`, and nothing else. `crates/mesh-types` still computes it through the retired framing until `01KZFMZC4MTHTT3BW4Y0BW6NYA`. | — | `crates/mesh-types` |
| `ActorId` | The `record ID` of an `actor`, derived from that actor's `actor key` and from nothing else, so that renaming or disabling an actor cannot rename it. The key is a canonical record in its own right — one thirty-two-byte field under its own `DomainTag` — so an `ActorId` comes from the one rule and not from a second one, which is also what makes it computable outside this repository. `crates/mesh-types` joins the tag to the key without an encoding until `01KZFMZC4MTHTT3BW4Y0BW6NYA`. | trust | `crates/mesh-types` |
| `VersionId` | The `record ID` of a `version` — a `file version` or a `directory version`, each derived under its own `DomainTag`. | state | `crates/mesh-types` |
| `ManifestId` | The `record ID` of a `file manifest`. | state | `crates/mesh-types` |
| `ContentHash` | The `content digest` of a byte sequence: the name a `chunk` is stored and verified under. Not a `record ID` — a `chunk` has no schema and no `canonical encoding`, and hashing its bytes bare is what lets any BLAKE3 tool verify one. Reclassified in [ADR-0033](design-decisions.md#adr-0033); nothing about how a chunk is hashed changed. | state | `crates/mesh-types` |
| `ChangeSetId` | The `record ID` of a `ChangeSet`, binding every field except the signature over them. | operation | `crates/mesh-types` |
| `HeadId` | The `record ID` of a `head` — an `actor head` or the `canonical head`. | state | `crates/mesh-types` |
| `ReviewBundleId` | The `record ID` of a `review bundle`. | trust | `crates/mesh-types` |
| `ApprovalId` | The `record ID` of an `approval envelope`. | trust | `crates/mesh-types` |
| `PublicKey` | The thirty-two Ed25519 bytes an `actor key` or a `device key` is carried as. Opaque here; signing and verification belong to `mesh-crypto`. | trust | `crates/mesh-types` |
| `Signature` | The sixty-four Ed25519 bytes an actor signs a canonical record with. | trust | `crates/mesh-types` |
| `Timestamp` | A Unix-epoch millisecond, carried for display and for ordering that decides nothing. Never causality. | — | `crates/mesh-types` |
| `Hlc` | The `hybrid logical time` a `ChangeSet` carries: a physical millisecond and a logical counter that breaks ties within it. | operation | `crates/mesh-types` |
| `ActorKind` | The `actor kind` enumeration, with all seven members representable and only `human` able to hold the approval `capability`. | trust | `crates/mesh-types` |
| `Actor` | An `actor` record: its `ActorKind`, its `PublicKey`, its sponsor human, its display name and its lifecycle timestamps. The key is the identity; the rest is mutable state about it. | trust | `crates/mesh-types` |
| `ActivitySession` | A `session` record: the workspace, the actor, the base head, the optional intent, the capability and the two timestamps. Created automatically, never user-managed. | operation | `crates/mesh-types` |
| `ObjectKind` | What an `object` is: a file, a directory or a symbolic link. | state | `crates/mesh-types` |
| `Object` | An `object` record: its `ObjectId`, its `ObjectKind` and the `ChangeSet` that created it. | state | `crates/mesh-types` |
| `NormalizedName` | A `directory entry` name that is structurally usable as one. Rejects the empty name, the two relative names, a path separator and a NUL byte; the Unicode normalization form is `mesh-materializer`'s to specify and this is where it is enforced. | state | `crates/mesh-types` |
| `NameError` | Why a `NormalizedName` was refused. | state | `crates/mesh-types` |
| `DirectoryEntry` | A `directory entry`: the binding of a `NormalizedName` to a child `object` and its `version`. | state | `crates/mesh-types` |
| `DirectoryVersion` | A `directory version` record: the object and its entries, held in sorted name order so that its `VersionId` is reproducible without a separate sorting step. | state | `crates/mesh-types` |
| `PortableMetadata` | The `portable metadata` a `file version` binds. One field today, and widening it moves every `VersionId` that binds it. | state | `crates/mesh-types` |
| `FileVersion` | A `file version` record: the object, the versions it supersedes, its `ManifestId`, its `PortableMetadata` and the `ChangeSet` that produced it. | state | `crates/mesh-types` |
| `ChunkRef` | A `chunk reference`: a `Digest32` naming a `chunk`, with its offset and length in the reconstructed file. | state | `crates/mesh-types` |
| `FileManifest` | A `file manifest` record: the byte length, the `content digest` of the reconstructed bytes, and the ordered chunk references. | state | `crates/mesh-types` |
| `ActorSequence` | The `actor sequence number` a `ChangeSet` carries. Saturating rather than wrapping, because a wrapped counter makes the omission it exists to expose undetectable. | operation | `crates/mesh-types` |
| `PolicyEpoch` | The `policy epoch` a `ChangeSet` was authored under. | trust | `crates/mesh-types` |
| `CausalParents` | The ChangeSets a `ChangeSet` causally follows. Genesis and non-genesis are separate constructors, so an empty parent list is a statement rather than an omission. | operation | `crates/mesh-types` |
| `Unset` | The zero-sized marker standing where a `ChangeSetDraft` field has not been supplied yet. Once supplied, the field's own type replaces it. | operation | `crates/mesh-types` |
| `ChangeSetDraft` | A `ChangeSet` under construction, carrying its causal parents, base head and policy epoch as its own type parameters. Sealing it into a `ChangeSet` is offered only on the draft that has all three, which makes the causal context a compile-time requirement rather than a check. | operation | `crates/mesh-types` |
| `CBOR_PROFILE` | The name of the restricted CBOR profile the `canonical encoding` is written in. Versioned, because a change to any of its rules is a new profile and never an edit to this one. | — | `crates/mesh-types` |
| `CborWriter` | The writer that can only produce that profile: unsigned integers, byte strings, text strings, arrays and booleans, each in the shortest head that holds it. It has no method that emits a map, a tag, a float or an indefinite length, so no caller can emit one. | — | `crates/mesh-types` |
| `CborReader` | The reader that accepts exactly what `CborWriter` produces and refuses every other spelling of the same value — a wider head than necessary, an indefinite length, an excluded major type, a simple value other than true and false. What makes byte-identical agreement a property of the format rather than of one implementation. | — | `crates/mesh-types` |
| `CborError` | Why some bytes are not an item of that profile. | — | `crates/mesh-types` |
| `MAX_NESTING` | The nesting depth `CborReader` refuses to read past, so that a remote peer cannot abort a decoder with a few hundred kilobytes of opening brackets. Far above anything the schemas produce. | — | `crates/mesh-types` |
| `CanonicalType` | What one field of a record holds, in enough detail to decode it: an unsigned integer, a boolean, a byte string, a text string, a homogeneous sequence, a fixed-arity group of named fields, or a complete nested record. There is no variant for an absent value, which is what makes optional-field ambiguity unrepresentable rather than forbidden. | — | `crates/mesh-types` |
| `FieldSchema` | One field's published name and its `CanonicalType`. The name is descriptive; the bytes carry position. | — | `crates/mesh-types` |
| `RecordSchema` | One record type's published schema: its `DomainTag` and its fields in encoding order. The field order is the format, which is why changing it moves every `test vector` of that type. | — | `crates/mesh-types` |
| `CanonicalValue` | One field's value, held as data before it is written, so that a reordered field is something a test can construct rather than something a reviewer has to imagine. | — | `crates/mesh-types` |
| `CanonicalEncode` | A record that has a `canonical encoding`: it names its `RecordSchema` and produces its field values in that order. Implementing it is what makes a record signable. | — | `crates/mesh-types` |
| `encode_canonical` | The function producing a record's `canonical encoding`: an array whose first element is its `DomainTag` and whose remaining elements are its fields in schema order. | — | `crates/mesh-types` |
| `decode_canonical` | The inverse of `encode_canonical` under a known `RecordSchema`, refusing another record type's `DomainTag`, a field of the wrong shape, and any byte after the record ends. | — | `crates/mesh-types` |
| `DecodeError` | Why some bytes are not the `canonical encoding` of a record under a given `RecordSchema`. | — | `crates/mesh-types` |
| `canonical_digest` | The `content digest` of a record's `canonical encoding` — the digest a signature over that record's bytes covers, and, under [ADR-0033](design-decisions.md#adr-0033), that record's `record ID`. The two were separate values until that ruling and are one value afterwards. | — | `crates/mesh-types` |
| `schema_violations` | Every way a record's field values disagree with its published `RecordSchema`; empty when they agree. What keeps a published schema describing the bytes it claims to. | — | `crates/mesh-types` |
| `SchemaViolation` | One such disagreement: a wrong field count, a wrong shape, or a fixed-width byte field of the wrong width, each naming the field it is about. | — | `crates/mesh-types` |
| `SCHEMA_FORMAT` | The version of the schema vocabulary published under `protocol/schemas`. | — | `crates/mesh-types` |
| `VECTOR_FORMAT` | The version of the `test vector` file format published under `protocol/test-vectors`. | — | `crates/mesh-types` |
| `PublishedDocument` | One published file: its path under `protocol/` and its complete text. | — | `crates/mesh-types` |
| `published_documents` | Every published schema and `test vector` file, as exact text. Compared to the working tree in both directions on every test run, which is what makes an encoding change without a vector update fail. | — | `crates/mesh-types` |

| `RecordDigest` | The storage form of a `record ID`: thirty-two bytes, ordered by memcmp so that an in-memory sort and a SQL sort agree. | state | `crates/mesh-store` |
| `EntityUuid` | The storage form of an `entity ID`: sixteen bytes, with no conversion to or from `RecordDigest`, so the two identity families stay apart at the storage boundary. | state | `crates/mesh-store` |
| `IdError` | Why a hex `RecordDigest` or `EntityUuid` failed to parse: wrong length, or a character outside the hex alphabet. | — | `crates/mesh-store` |
| `RecordKind` | Which immutable record a `StoredRecord` is. Every local table declares the kinds it folds, which is how reconstructability stops being a claim and becomes something a test reads. | operation | `crates/mesh-store` |
| `StoredRecord` | One immutable record as the local index receives it. The only input a rebuild is given, so anything the index holds that this cannot produce is a fact in the wrong place. | operation | `crates/mesh-store` |
| `OperationRecord` | An `operation` as the local index holds it: its author, its `actor sequence number`, its `hybrid logical time`, its `policy epoch`, its `session` and its causal parents. | operation | `crates/mesh-store` |
| `ManifestRecord` | A `file manifest` as the local index holds it. The `chunk` bytes it names live in the content-addressed store and never here. | state | `crates/mesh-store` |
| `ChunkSlice` | One `chunk reference` as the local index holds it: its `content digest`, its offset and its length in the reconstructed file. | state | `crates/mesh-store` |
| `PeerRecord` | A `peer` joining this workspace's replication set, at a named `operation`. It exists so the `outbox` has a peer list a rebuild can reproduce. | dependency | `crates/mesh-store` |
| `AckRecord` | A `peer`'s immutable receipt that it holds an `actor`'s operations up to a sequence number. The `watermark` is the fold of these, never a counter somebody increments. | dependency | `crates/mesh-store` |
| `ReviewRecord` | A `review bundle` as the local index holds it: the bundle, the `operation` under review, and who opened it. | trust | `crates/mesh-store` |
| `ApprovalRecord` | An `approval envelope` as the local index holds it: the envelope, its bundle, its approver and its verdict. | trust | `crates/mesh-store` |
| `ReviewVerdict` | What an approver decided about a `review bundle`. Stored as an integer code under a database constraint rather than as text. | trust | `crates/mesh-store` |
| `ContextRecord` | One entry recording that a `session` touched an `operation`'s content, how, and over how many bytes. | dependency | `crates/mesh-store` |
| `ContextAccess` | How a `session` touched content: reading it, writing it, or referencing it without reading its bytes. | dependency | `crates/mesh-store` |
| `ColumnType` | The storage class a column holds. There are two, and no record-derived column holds free text, which is what lets the writer render SQL without quoting a string. | — | `crates/mesh-store` |
| `ColumnDomain` | What a column means as opposed to what it stores, including the `mesh-types` item it mirrors when it mirrors one. | — | `crates/mesh-store` |
| `Column` | One column of one local table: its name, its `ColumnType` and its `ColumnDomain`. | — | `crates/mesh-store` |
| `Provenance` | Where a local table's rows can be rebuilt from. It has two shapes and no third, so a fact that is neither a fold over records nor the migration ledger has nowhere to be declared. | operation | `crates/mesh-store` |
| `Table` | One local table: its name, the schema version that introduced it, its columns, its `Provenance` and its purpose. | state | `crates/mesh-store` |
| `TABLES` | Every local table, in the order the reconstruction digest absorbs them. Appended to rather than sorted, because the order is part of the digest. | state | `crates/mesh-store` |
| `table` | The local table with a given name, if the schema declares one. | state | `crates/mesh-store` |
| `table_names_in_ddl` | The table names a block of SQL creates. A small reader rather than a parser, stated as the lint it is. | — | `crates/mesh-store` |
| `UNINDEXED_MESH_TYPES_IDS` | The `mesh-types` record identifiers the local index deliberately does not hold, each with its reason, so that not indexing one is a decision on the record. | state | `crates/mesh-store` |
| `Value` | A value in a local column: an integer or a byte string. There is no null and no text, and both absences are constraints the schema keeps. | — | `crates/mesh-store` |
| `Row` | One row of one local table, its values in column order. One shape serves the fold, the SQL writer and the reader, because a separate shape for each is how the three drift. | — | `crates/mesh-store` |
| `Migration` | One forward step of the local schema. It carries SQL and no reverse, which is what makes forward-only a property of the type rather than a promise. | — | `crates/mesh-store` |
| `MIGRATIONS` | Every `Migration`, in version order. Append only. | — | `crates/mesh-store` |
| `CURRENT_VERSION` | The schema version a freshly opened local database ends up at. | — | `crates/mesh-store` |
| `AppliedMigration` | One migration as the local version table records it: the version, and the fingerprint of the SQL that ran. | — | `crates/mesh-store` |
| `MigrationPlan` | The migrations still to run against a local database, and the version-table rows recording them. | — | `crates/mesh-store` |
| `MigrationError` | Why planning or verifying local migrations failed: a database from a newer build, a gap in the version table, a migration whose SQL changed after it ran, or a malformed migration set. | — | `crates/mesh-store` |
| `plan_migrations` | The migrations to run against a local database at a given version. Refuses a database newer than this build. | — | `crates/mesh-store` |
| `verify_applied` | Holds the version table a local database reports against the migrations this build carries, refusing a gap or a rewritten migration. | — | `crates/mesh-store` |
| `full_schema_sql` | Every migration's SQL, concatenated in version order: the schema a fresh local database ends up with, as text. | — | `crates/mesh-store` |
| `FoldError` | Why an immutable record could not be folded into the local index. Every variant is a statement about the record stream, never about the index. | operation | `crates/mesh-store` |
| `Index` | The whole local index in memory: the result of folding immutable records, and the reference every rebuild is compared against. | state | `crates/mesh-store` |
| `no_session` | The `session` identifier that names no session, for records that predate sessions. | operation | `crates/mesh-store` |
| `IndexDigest` | The seam an index-digest algorithm implements. A drift detector between two copies of an index, never a security primitive. | — | `crates/mesh-store` |
| `Fnv1a128` | The default `IndexDigest`, named rather than disguised: sufficient for detecting an accidental difference, and not collision-resistant against a chosen input. | — | `crates/mesh-store` |
| `Digest16` | A sixteen-byte index-digest value. | — | `crates/mesh-store` |
| `rebuild` | Replay immutable records into a fresh `Index`. The recovery for a corrupt local index, and therefore also the thing every reconstruction test runs. | operation | `crates/mesh-store` |
| `RebuildReport` | What a rebuild did: how many records were replayed, how many rows each table ended up with, and the digest of the result. | operation | `crates/mesh-store` |
| `indexed_record_kinds` | Every `RecordKind` some local table folds. | operation | `crates/mesh-store` |
| `tables_fed_by` | The local tables a given `RecordKind` contributes to. | operation | `crates/mesh-store` |
| `tables_outside_the_fold` | The local tables that are not a fold over immutable records at all. | — | `crates/mesh-store` |
| `DATABASE_FILE_NAME` | The file name of the local metadata database inside a workspace directory. | — | `crates/mesh-store` |
| `MOUNT_DIRECTORY_NAME` | The directory name materialized workspace content is written under. | state | `crates/mesh-store` |
| `PathError` | Why a workspace-relative path was refused: empty, absolute, climbing out, carrying a prefix or a NUL byte, or spelling a legal path a second way. | state | `crates/mesh-store` |
| `WorkspaceRelativePath` | A path that names workspace content relative to a `MountRoot`. Constructing one is the only way to name materializable content, which is what excludes the local database from `materialization` by construction rather than by a rule. | state | `crates/mesh-store` |
| `WorkspaceRoot` | A workspace's own directory. It hands out a `MountRoot` and a `DatabasePath` and nothing that is both. | state | `crates/mesh-store` |
| `MountRoot` | The root materialized workspace content is written under. A distinct type from `WorkspaceRoot`, so the local database's parent is not reachable from what `materialization` holds. | state | `crates/mesh-store` |
| `DatabasePath` | The local metadata database, and the two sidecar files write-ahead logging creates beside it. All three, never just the first. | — | `crates/mesh-store` |
| `WORKSPACE_EXCLUSION_FILE_NAME` | The file at a workspace root a user edits to say what Mesh must not version. | state | `crates/mesh-store` |
| `ExclusionSource` | Where an exclusion rule came from, ordered by precedence with the lowest first: the repository's own ignore rules, the workspace configuration, then the Mesh-native file. The order *is* the precedence rule, so a new source cannot be added without being placed. | state | `crates/mesh-store` |
| `ExclusionRule` | One declared rule with the `ExclusionSource` that declared it, and whether it takes a path out or puts one back. | state | `crates/mesh-store` |
| `ExclusionError` | Why an exclusion rule was refused: empty, naming something outside the workspace, or using a wildcard the predicate does not implement. A rule is refused rather than skipped, because a skipped rule admits every path it would have kept out. | state | `crates/mesh-store` |
| `ExclusionSet` | A workspace's declared exclusion set. Its verdict is a pure function of the path and the set — no filesystem, no clock and no adapter — which is why every adapter gives the same answer for the same path. | state | `crates/mesh-store` |
| `Exclusion` | What the predicate says about one path: that it may produce a durable version, or which source and rule stopped it. | state | `crates/mesh-store` |
| `Admission` | What an `ExclusionSet` admits from a set of observed paths and lengths, and what it refuses, in bytes. The refused bytes are the ones no storage budget ever has to bound. | state | `crates/mesh-store` |
| `CommitStep` | Which step of the plan's commit sequence a `Statement` belongs to. Ordered, so a plan's steps can be checked never to go backwards. | operation | `crates/mesh-store` |
| `Statement` | One SQL statement and the `CommitStep` it belongs to, with every value already a literal. | operation | `crates/mesh-store` |
| `Checkpoint` | What one local `checkpoint` contributes to the index. `chunk` bytes are absent on purpose, because they are already durable before this is built. | operation | `crates/mesh-store` |
| `CommitPlan` | An ordered SQL plan for one local transaction, whether a `Checkpoint` or a whole rebuild. | operation | `crates/mesh-store` |
| `Pragma` | One local database connection setting, with the reason it is set, because a setting nobody can justify is a setting nobody dares change. | — | `crates/mesh-store` |
| `PRAGMAS` | The settings every connection to a local database is opened with, write-ahead logging and foreign keys among them. | — | `crates/mesh-store` |
| `SqlExecutor` | The SQL execution seam a local database driver implements. Three methods, none of which names a database engine. | — | `crates/mesh-store` |
| `StoreError` | Why opening or writing a local store failed: the driver, the migrations, the fold, or a version table that is not the shape this build declares. | — | `crates/mesh-store` |
| `Store` | The single writer over a local database. It owns one connection and one `Index`, and its write method takes an exclusive borrow, so the compiler serializes writes through it. | state | `crates/mesh-store` |
| `connection_pragmas_sql` | The settings a local database driver must re-apply on every connection, excluding the ones the database file itself remembers. | — | `crates/mesh-store` |
| `read_all_tables` | Read every local table out of a database, skipping the ones a migration has not created yet. | state | `crates/mesh-store` |
| `RECORD_ENCODING_PROFILE` | The name of the `canonical encoding` every record body carried by a CWP message is in, mirrored as a string because no dependency edge is permitted. | — | `crates/mesh-sync-protocol` |
| `PROTOCOL_VERSION` | The version of the CWP message set this build speaks. A `HELLO` naming another version is refused rather than negotiated. | — | `crates/mesh-sync-protocol` |
| `WIRE_FORMAT` | The versioned name of the CWP message framing, distinct from the `canonical encoding` profile the framing is built on. | — | `crates/mesh-sync-protocol` |
| `MessagePlane` | Which plane carries a CWP message, exactly one of: `handshake`, `metadata`, `content`. The plan names two; the third is session establishment, which belongs to neither and would otherwise be counted inside the `metadata plane`'s latency. | — | `crates/mesh-sync-protocol` |
| `MessageKind` | The name of one CWP message without its fields, exactly one of: `HELLO`, `AUTHENTICATE`, `ADVERTISE_FRONTIER`, `REQUEST_OPERATIONS`, `OPERATIONS_BATCH`, `ACK_OPERATIONS`, `ADVERTISE_MANIFESTS`, `REQUEST_CHUNKS`, `CHUNK_BATCH`, `ACK_CHUNKS`, `UPDATE_ACTOR_HEAD`, `UPDATE_CANONICAL_HEAD`, `PRESENCE`, `REVIEW_BUNDLE`, `VALIDATION_RECEIPT`, `APPROVAL_ENVELOPE`, `ANTI_ENTROPY_SUMMARY`, `ERROR`. Enumerated in §7.3. | — | `crates/mesh-sync-protocol` |
| `MESSAGE_KINDS` | Every `MessageKind`, in plan §5.3's order, so a conformance harness enumerates the set rather than hard-coding it. | — | `crates/mesh-sync-protocol` |
| `SyncMessage` | One CWP message with its fields. The eighteen variants are §7.3's table. | — | `crates/mesh-sync-protocol` |
| `HeadAdvertisement` | What one `peer` advertises about one `actor`: the `actor head` it holds, how much of that actor's sequence it holds contiguously, and what it holds beyond. | state | `crates/mesh-sync-protocol` |
| `SparseChangeSet` | One ChangeSet a `peer` holds beyond its contiguous run, carried with the `actor sequence number` it sits at so a receiver can tell which hole it is beyond. | operation | `crates/mesh-sync-protocol` |
| `CarriedChangeSet` | One ChangeSet as it travels the `metadata plane`: its `canonical encoding` bytes, plus the fields a receiver needs before it has decoded them. | operation | `crates/mesh-sync-protocol` |
| `ChunkRequest` | A request for one `chunk`'s bytes, resumable from an offset and bounded in size. | state | `crates/mesh-sync-protocol` |
| `ChunkPart` | A slice of one `chunk`'s bytes, with its offset and whether it completes the chunk — which is when the receiver verifies the `ContentHash`. | state | `crates/mesh-sync-protocol` |
| `PresenceState` | What a `PRESENCE` message reports, exactly one of: `active`, `idle`, `away`. | — | `crates/mesh-sync-protocol` |
| `MAX_OPERATIONS_PER_BATCH` | The largest number of ChangeSets one `OPERATIONS_BATCH` may carry, enforced by the receiver so a claimed batch size cannot drive an unbounded allocation. | — | `crates/mesh-sync-protocol` |
| `MAX_CHUNK_PART_BYTES` | The largest number of `chunk` bytes one `ChunkPart` may carry, which bounds a receiver's buffer without bounding the chunk. | — | `crates/mesh-sync-protocol` |
| `ErrorCode` | Why a CWP message was refused, exactly one of: `unsupported version`, `unknown message`, `unauthenticated session`, `unverified peer`, `malformed message`, `unknown actor`, `sequence gap`, `unknown ChangeSet`, `unknown chunk`, `offset past end`, `integrity failure`, `unknown policy epoch`, `batch too large`, `busy`. A closed set; adding a member is a protocol change. | — | `crates/mesh-sync-protocol` |
| `ERROR_CODES` | Every `ErrorCode`, so a conformance harness enumerates the set rather than hard-coding it. | — | `crates/mesh-sync-protocol` |
| `ProtocolError` | A refusal: the `ErrorCode`, the `MessageKind` it is about, and a diagnostic detail that no `peer` parses. The same value a locally checked precondition produces and an `ERROR` message carries. | — | `crates/mesh-sync-protocol` |
| `WireError` | Why some bytes are not a CWP message: a shape failure, distinct from a well-formed message that must be refused. | — | `crates/mesh-sync-protocol` |
| `encode_message` | The `canonical encoding` bytes of a `SyncMessage`. | — | `crates/mesh-sync-protocol` |
| `decode_message` | The `SyncMessage` some bytes encode, or the `WireError` explaining why they encode none. | — | `crates/mesh-sync-protocol` |
| `ActorKnowledge` | What a `peer` holds of one `actor`'s history: the maximum contiguous `actor sequence number`, what is held beyond it, and the `actor head` it has announced. | state | `crates/mesh-sync-protocol` |
| `KnowledgeSet` | The `knowledge set` as a value. Exact about the holder, a belief about a `peer`, and the same type either way. | state | `crates/mesh-sync-protocol` |
| `ActorGap` | What one side lacks of one `actor`'s history: where to resume, how far the other side reaches, and the ChangeSets beyond the contiguous run it does not hold. | operation | `crates/mesh-sync-protocol` |
| `ReplicationGap` | The difference between two knowledge sets, and the requests that close it. Run in one direction it answers what to ask for; run in the other, what to send. | operation | `crates/mesh-sync-protocol` |
| `MerkleSummary` | A summary of one `actor`'s history for `anti-entropy`: leaves over contiguous sequence runs, and a root over the leaves. | operation | `crates/mesh-sync-protocol` |
| `SummaryNode` | One leaf of a `MerkleSummary`: a contiguous run of one `actor`'s sequence and the digest of what it holds. | operation | `crates/mesh-sync-protocol` |
| `SummaryDigest` | The digest seam a `MerkleSummary` is built under. `mesh-sync-protocol` ships no implementation, so a composition root cannot pick the wrong one by accident. | — | `crates/mesh-sync-protocol` |
| `SUMMARY_DOMAIN` | The domain a `MerkleSummary` absorbs first, so a summary digest can never collide with a digest of the same bytes taken for another purpose. | — | `crates/mesh-sync-protocol` |
| `PeerAuthenticator` | The seam a real signature verifier plugs into. `mesh-sync-protocol` has no dependency and therefore no cryptography of its own. | trust | `crates/mesh-sync-protocol` |
| `NoAuthenticator` | The only `PeerAuthenticator` shipped today: it checks nothing and answers `unverified` for every `peer`, never `verified`. A truthful non-verifier, not a stub that pretends. | trust | `crates/mesh-sync-protocol` |
| `AuthenticationOutcome` | What a verifier concluded about a `peer`, exactly one of: `verified`, `unverified`, `refused`. The last two are different facts — no evidence checked, versus evidence checked and wrong. | trust | `crates/mesh-sync-protocol` |
| `AuthenticationPolicy` | Whether a session admits a `peer` whose identity is unchecked, exactly one of: `require verified peers`, `admit unverified peers`. The first admits nothing on the current tree. | trust | `crates/mesh-sync-protocol` |
| `Session` | One side of one CWP session: the `actor` a `peer` claims to be, what was concluded about that claim, and whether the handshake has completed. | trust | `crates/mesh-sync-protocol` |
| `Cas` | The content-addressed store: staging, verified promotion, read and reclamation over one `workspace`'s chunk store. Generic over its filesystem and its `ContentDigest`, so neither is named on the promotion or read path. | state | `crates/mesh-cas` |
| `CasError` | Why a content-addressed store operation failed. | state | `crates/mesh-cas` |
| `DurableFs` | The filesystem seam the store's durability argument rests on. An implementation must not fuse a write with its sync, because a test that cannot interrupt between the two cannot prove what a crash leaves behind. | state | `crates/mesh-cas` |
| `StdFs` | The only `DurableFs` shipped: the standard library's filesystem, which the store runs on outside tests. | state | `crates/mesh-cas` |
| `StoreLayout` | The paths of one `workspace`'s content-addressed store. Purely a naming calculation — constructing one touches no filesystem, so a caller can ask where a `chunk` would live without creating anything. | state | `crates/mesh-cas` |
| `CHUNKS_DIRECTORY_NAME` | The directory promoted, immutable, content-named `chunk` files live in. | state | `crates/mesh-cas` |
| `SCRATCH_DIRECTORY_NAME` | The directory in-flight staging lives in. Everything under it is discardable by definition. | state | `crates/mesh-cas` |
| `QUARANTINE_DIRECTORY_NAME` | The directory bytes that failed verification are kept in for diagnosis rather than deleted. | state | `crates/mesh-cas` |
| `LOGS_DIRECTORY_NAME` | The directory the store's append-only operational records live in. | state | `crates/mesh-cas` |
| `ARRIVAL_JOURNAL_FILE_NAME` | The file name of the `ArrivalJournal` inside the store's log directory. | state | `crates/mesh-cas` |
| `ARRIVAL_JOURNAL_REWRITE_FILE_NAME` | The `ArrivalJournal`'s rewrite target during compaction, placed beside it so the rename that replaces it is atomic. | state | `crates/mesh-cas` |
| `ArrivalJournal` | The append-only record of which chunks have arrived and which are spoken for, held as a view over the store's filesystem and its `StoreLayout` so the two cannot disagree about where the file is. | state | `crates/mesh-cas` |
| `Promotion` | One staged `chunk`'s passage to promoted, immutable content, carrying which `PromotionStep` it has reached so an interruption names a known position rather than an ambiguous one. | state | `crates/mesh-cas` |
| `PromotionStep` | One interruptible step of a `Promotion`, ordered, so a crash between two steps is recoverable by the step it stopped after. | state | `crates/mesh-cas` |
| `ReferenceOracle` | The seam reclamation asks what still points at a `chunk`. The store never answers that itself: a content-addressed store that believed it knew what referenced its content would be a second, disagreeing source of truth. | state | `crates/mesh-cas` |
| `KeyPurpose` | The sealed marker distinguishing the two key families this crate admits, so an `actor key` and a `device key` are different types rather than one type used two ways. | trust | `crates/mesh-crypto` |
| `ForActor` | The `KeyPurpose` marker for an `actor key`. | trust | `crates/mesh-crypto` |
| `ForDevice` | The `KeyPurpose` marker for a `device key`. | trust | `crates/mesh-crypto` |
| `KeyPair` | An Ed25519 key pair, named by its public half and typed by its `KeyPurpose`. The secret half never exists as a value, so no function can return one. | trust | `crates/mesh-crypto` |
| `ActorKey` | The `KeyPair` that signs an `actor`'s ChangeSets and, for a human actor, approval envelopes. | trust | `crates/mesh-crypto` |
| `DeviceKey` | The `KeyPair` identifying one device to a relay, a distinct type from every `actor key` with no conversion between them. | trust | `crates/mesh-crypto` |
| `KeyRole` | Which key family a `KeyPair` belongs to, as a value rather than a type, for messages and wire formats a type parameter cannot travel through. | trust | `crates/mesh-crypto` |
| `KeyParseError` | Why a hexadecimal public key failed to parse. | trust | `crates/mesh-crypto` |
| `PUBLIC_KEY_BYTES` | The length in bytes of an Ed25519 public key. | trust | `crates/mesh-crypto` |
| `SIGNATURE_BYTES` | The length in bytes of an Ed25519 signature. | trust | `crates/mesh-crypto` |
| `SignatureScheme` | The signature seam: the one trait a signature backend implements, so replacing the algorithm is a new implementation of it and never an edit at a call site. | trust | `crates/mesh-crypto` |
| `VerifyError` | Why a signature was not accepted. Deliberately coarse at the boundary a `peer` can observe: a caller learns that verification failed, not which internal check failed first. | trust | `crates/mesh-crypto` |
| `Ed25519` | The only `SignatureScheme` implementation in the workspace: an adapter over an exactly pinned, audited Ed25519 library, with the malleability and non-canonical-scalar rejections checked in this crate rather than left to a feature flag. | trust | `crates/mesh-crypto` |
| `DomainSeparator` | The versioned label naming the protocol position a signature was made in, so a signature made in one position is not valid in another. | trust | `crates/mesh-crypto` |
| `SigningPayload` | The framed bytes a signature is made and checked over, built once from a `DomainSeparator` and handed to both sides, so signing and verification cannot frame differently. | trust | `crates/mesh-crypto` |
| `KeyCustody` | The custody seam: what holds a secret half and signs with it. It has no accessor for the secret half, which is how no private key can leave this crate. | trust | `crates/mesh-crypto` |
| `KeyGenerator` | A custody that can mint new key material. Separate from `KeyCustody` so that a read-only holder, provisioned elsewhere, is expressible. | trust | `crates/mesh-crypto` |
| `CustodyBackend` | Where a secret half lives, named so an operator can tell isolated custody from the absence of it. | trust | `crates/mesh-crypto` |
| `CustodyError` | Why custody could not act. No variant carries key material, and none carries an operating-system error string. | trust | `crates/mesh-crypto` |
| `HumanKeyCustody` | The custody a person's own `actor key` is held under, and the only custody a `HumanKeyAttestation` can be made from. | trust | `crates/mesh-crypto` |
| `HumanKeyAttestation` | Evidence that an `actor key` is held by a person under operating-system key isolation. It carries the attested key, so a human-held `Capability` cannot be minted for a different one. | trust | `crates/mesh-crypto` |
| `AuthorityTier` | One rung of the authority lattice a `Capability` is typed by. Sealed, with exactly two rungs and no way up. | trust | `crates/mesh-crypto` |
| `Delegated` | The `AuthorityTier` every non-human `actor` holds. | trust | `crates/mesh-crypto` |
| `HumanHeld` | The `AuthorityTier` only a person's own key holds. | trust | `crates/mesh-crypto` |
| `DelegatedAction` | The action vocabulary a `Delegated` `Capability` draws from. Advancing the `canonical head` is absent from it, so the argument granting it cannot be written — the claim that an agent cannot advance canonical state, enforced by the type system rather than by a check. | trust | `crates/mesh-crypto` |
| `HumanAction` | The action vocabulary a `HumanHeld` `Capability` draws from: everything delegable, plus advancing the `canonical head`. | trust | `crates/mesh-crypto` |
| `Capability` | A `capability` as a value, immutable and typed by its `AuthorityTier`. Delegation is the only derivation on it, and there is no method, field or constructor that widens one. | trust | `crates/mesh-crypto` |
| `Delegation` | What a delegation asks for, checked field by field against the parent before a `Capability` exists, so there is no moment at which an over-broad capability is a value. | trust | `crates/mesh-crypto` |
| `DelegationBudget` | How many further delegations a `Capability` permits. Strictly decreasing, so a delegation chain is finite. | trust | `crates/mesh-crypto` |
| `WorkspaceScope` | The `workspace` a `Capability` is scoped to, carried as sixteen opaque bytes rather than as a second `WorkspaceId` type. | trust | `crates/mesh-crypto` |
| `Expiry` | The moment a `Capability` stops being valid. There is no ambient clock in this crate: the current time is always an argument. | trust | `crates/mesh-crypto` |
| `CapabilityError` | Why a `Capability` is not usable right now. | trust | `crates/mesh-crypto` |
| `DelegationError` | Why a `Delegation` was refused. Every variant is a way the request was not strictly narrower than its parent. | trust | `crates/mesh-crypto` |
| `CapabilityToken` | The signed, verifiable encoding of a `Capability` a `peer` can check without contacting its issuer. It holds bytes and a `Signature`, never a `Capability`. | trust | `crates/mesh-crypto` |
| `CapabilityCodec` | The encoding seam a `CapabilityToken`'s payload is written and read through. Decoding yields `CapabilityParts`, never a `Capability`. | trust | `crates/mesh-crypto` |
| `CapabilityParts` | The inert field values a `CapabilityCodec` decodes. Constructing one grants nothing; only a verified signature turns them into a `Capability`. | trust | `crates/mesh-crypto` |
| `PartsError` | Why decoded `CapabilityParts` are not a `Capability` at the requested `AuthorityTier`. | trust | `crates/mesh-crypto` |
| `CodecError` | Why a `CapabilityToken` payload did not decode. | trust | `crates/mesh-crypto` |
| `TokenError` | Why a `CapabilityToken` was not accepted. | trust | `crates/mesh-crypto` |
| `TOKEN_MAGIC` | The magic bytes a `CapabilityToken` envelope begins with. | trust | `crates/mesh-crypto` |
| `TOKEN_VERSION` | The `CapabilityToken` envelope version this build reads and writes. | trust | `crates/mesh-crypto` |
| `MAX_PAYLOAD_BYTES` | The largest `CapabilityToken` payload the envelope accepts, so a hostile length cannot ask for an unbounded allocation. | trust | `crates/mesh-crypto` |
| `KeyRing` | One `actor`'s or one device's key succession: the current key and every key it succeeded, so past signatures stay verifiable while only the current key may sign. | trust | `crates/mesh-crypto` |
| `KeyStanding` | Where a key stands in a `KeyRing`. | trust | `crates/mesh-crypto` |
| `RetiredKey` | A key a `KeyRing` has succeeded, and the moment it stopped being current. | trust | `crates/mesh-crypto` |
| `RotationError` | Why a `KeyRing` rotation was refused. | trust | `crates/mesh-crypto` |
| `ActorHead` | One `actor head` as a value: whose it is, which state it names, and where that state sits on the review axis. Immutable — every transition returns a new value. | state | `crates/mesh-state` |
| `HeadState` | Where an `actor head` sits on the review axis, as a value: the `head state` members and no others, independent of `availability state`, which is the retrievability axis and is never merged into it. | state | `crates/mesh-state` |
| `HeadAdvancement` | The receiver that advances one `actor head` from delivered ChangeSets: applying what it can derive, buffering what it cannot yet, and refusing what it never can. | state | `crates/mesh-state` |
| `DeliveredChangeSet` | One authored transition as a `peer` delivers it. Immutable, so the statement that a head is a function of what was delivered stays falsifiable. | state | `crates/mesh-state` |
| `HeadDigest` | The digest seam an `actor head` is named under. This crate ships no implementation and names no algorithm, so nothing here can pick the wrong one. | state | `crates/mesh-state` |
| `HEAD_DOMAIN` | The versioned label every `actor head` digest is derived under, absorbed first so that a head identifier cannot collide with a digest of the same bytes taken in another domain. | state | `crates/mesh-state` |
| `Reception` | What one delivery of a `DeliveredChangeSet` did. | state | `crates/mesh-state` |
| `Refusal` | Why a ChangeSet is not a transition this receiver can derive. Every variant is a statement about the delivered record and never about the receiver's own state, so a refusal means the same thing on every `peer` holding the same `causal set`. | state | `crates/mesh-state` |
| `KnownMissing` | A ChangeSet held because a causal parent has not arrived: buffered, held visibly and indefinitely, with no expiry and no eviction. | state | `crates/mesh-state` |
| `Operation` | One meaningful transition an `actor` performed, as a value: which member of the `operation vocabulary` it is, and every field that member binds. No variant carries file bytes — content reaches a record as a `content digest` — so an applier that needs a fact this type does not hold is being asked to guess. | operation | `crates/mesh-operations` |
| `OperationKind` | Which member of the `operation vocabulary` an `Operation` is. The eighteen members are plan §4.3's, in plan §4.3's order, and the crate's own test reads that section rather than restating it, so a nineteenth member is a plan change before it is a code change. | operation | `crates/mesh-operations` |
| `ReadRegion` | How much of an `object` an `actor` read, at the granularity a `read observation` records. Which bytes, cells or symbols were read is the dependency graph's to carry; what the vocabulary binds is the granularity claim, because that is what stops a reviewer overstating what the system knows. | dependency | `crates/mesh-operations` |
| `AttributionConfidence` | The `attribution confidence` of a `read observation` as a value. Enumerated so that an inference is never recorded as an observation: three of the six members are inferences and each says so. | dependency | `crates/mesh-operations` |
| `DerivationKind` | What kind of computation produced a `derivation`. | dependency | `crates/mesh-operations` |
| `ValidationOutcome` | What a `validator` concluded about a `head`: a check that passed, one that failed, one that was not run, and one that could not run. Four values, because that is the smallest set the hard guards and the warnings both need; widening it is a protocol change. | trust | `crates/mesh-operations` |
| `PreservedEntry` | One entry a `conflict rule` preserves: the `object` and the `NormalizedName` it keeps. Every contender keeps a name, so a resolution naming only a winner would have no way to say what the other one is now called. | operation | `crates/mesh-operations` |
| `DerivationId` | The `record ID` of a `derivation`. | dependency | `crates/mesh-operations` |
| `CHANGESET_DOMAIN` | The `DomainTag` every `ChangeSet` is encoded under. | operation | `crates/mesh-operations` |
| `CHANGESET_SCHEMA` | The published `RecordSchema` of a `ChangeSet`: ten bound fields, and never the signing bytes, because a record that bound its own signature could never be signed. | operation | `crates/mesh-operations` |
| `ReceivedChangeSet` | A `ChangeSet` as a `peer` delivered it, before the `head` its author claims has been checked. It carries every field a `ChangeSet` carries and none of its meaning: the one thing to do with it is to derive the head again and compare. | operation | `crates/mesh-operations` |
| `HeadRefused` | A resulting `head` a `ReceivedChangeSet` claims that the transition it describes does not produce. | operation | `crates/mesh-operations` |
| `TransitionCommitment` | Everything one authored transition is except what it produces: its `causal parent` set, the `head` it followed, its `policy epoch` and its operations. It has no public constructor, so no caller can ask for a head over a transition that never happened. | operation | `crates/mesh-operations` |
| `TRANSITION_DOMAIN` | The `DomainTag` every `TransitionCommitment` is encoded under. | operation | `crates/mesh-operations` |
| `TRANSITION_SCHEMA` | The published `RecordSchema` of a `TransitionCommitment`: `CHANGESET_SCHEMA` minus the resulting `head`, field for field and in the same order, so the two can be read side by side and the one difference is the point. | operation | `crates/mesh-operations` |
| `HeadDerivation` | The seam that says which `head` a `TransitionCommitment` produces. One method, no default body and no implementation in this crate, so the vocabulary names no digest algorithm and cannot name the wrong one. | state | `crates/mesh-operations` |
| `RawWrite` | One raw write an adapter observed, accumulated locally and never published. It has no `canonical encoding`, which is a compile-time fact rather than a convention: it cannot be nested in a `TransitionCommitment` and so cannot reach a `ChangeSet`. | operation | `crates/mesh-operations` |
| `WriteCoalescer` | The local accumulator between one durable `version` and the next, one per `object` being written: raw writes go in, and one `Operation` naming content that is already content-addressed comes out. Consumed and returned by every method, so one accumulator cannot be checkpointed twice. | operation | `crates/mesh-operations` |
| `CheckpointTrigger` | Why a `checkpoint` is being taken. Enumerated rather than free text, so that why a `version` exists is answerable for every version and an adapter cannot invent a reason nothing downstream can read. | operation | `crates/mesh-operations` |
| `NotCheckpointed` | Why a `checkpoint` of a `WriteCoalescer` produced no `Operation`. | operation | `crates/mesh-operations` |
| `SequenceLedger` | The issuing half of the `actor sequence number`: one `actor`'s own counter, consumed and returned advanced, so a number cannot be issued twice by keeping the earlier value. | operation | `crates/mesh-operations` |
| `SequenceExhausted` | Why an `actor sequence number` could not be issued. | operation | `crates/mesh-operations` |
| `SequenceWitness` | The observing half: which `actor sequence number` has arrived from each author and which has not. Keyed by author, because a sequence is per `actor` and two actors standing at the same number are unrelated facts. | operation | `crates/mesh-operations` |
| `SequenceObservation` | What a `SequenceWitness` made of one arriving `actor sequence number`. A gap is a statement about this receiver's knowledge and not an accusation — the ChangeSets not yet seen may be in flight — and it becomes a fault only when it persists. | operation | `crates/mesh-operations` |
| `OPERATION_DOMAIN_PREFIX` | The prefix every `Operation`'s `DomainTag` carries, so a member of the `operation vocabulary` is recognisable as one before anything else about it is decoded. | operation | `crates/mesh-operations` |
| `operation_schemas` | Every member's `RecordSchema`, in `OperationKind` order: the list published beside the `test vector` corpus. A function rather than a constant, so the eighteen schemas keep exactly one home and a nineteenth cannot reach a published list without joining the vocabulary first. | operation | `crates/mesh-operations` |
| `decode_operation` | The inverse of `encode_canonical` for an `Operation`. Refuses bytes that are not a well-formed record, a `DomainTag` naming no member, an arity or a fixed width the schema disagrees with, and a field carrying a value the vocabulary does not admit. | operation | `crates/mesh-operations` |
| `encode_operations` | The `canonical encoding` of each `Operation` in a sequence, in that sequence's order. | operation | `crates/mesh-operations` |
| `peek_domain` | The `DomainTag` some canonical bytes lead with, read without decoding anything else, which is the dispatch step for a sum type: read the tag, then decode against the member it names. | — | `crates/mesh-operations` |
| `one_of_every_operation` | One `Operation` per member of the `operation vocabulary`, in `OperationKind` order. This is the corpus every round-trip and `test vector` claim in this crate is measured over, so a member added without a corpus entry is caught rather than merely untested. | operation | `crates/mesh-operations` |
| `variable_length_operations` | The members whose shape has a variable part, at the empty and the several ends of it. A corpus of one value per member misses the two encodings a sequence field gets wrong: the empty sequence, and one whose length crosses an encoding head boundary. | operation | `crates/mesh-operations` |
| `corpus_covers_the_vocabulary` | Whether the corpus covers every member of the `operation vocabulary` exactly once. Published so that a conformance suite can state the same coverage claim without reimplementing the check and arriving at a different answer. | operation | `crates/mesh-operations` |
<!-- terminology:end -->

---

## 4. Aliases

An alias is a word that appears in prose or in an external source and resolves to exactly one
register term. **An alias carries no definition of its own** — that is what keeps it from becoming
a second definition of the same concept.

<!-- aliases:begin -->

| Alias | Resolves to | Where the alias comes from |
|---|---|---|
| `actor working head` | `actor head` | Plan §4.4 and [charter §5](charter.md#5-what-the-words-mean-to-a-user). |
| `durable actor checkpoint` | `checkpoint` | [charter §5](charter.md#5-what-the-words-mean-to-a-user). |
| `shared version` | `canonical head` | User-facing wording, [charter §5](charter.md#5-what-the-words-mean-to-a-user). |
| `frontier` | `head` | Distributed-systems literature. |
| `causal history` | `causal set` | Distributed-systems literature. |
| `stable object` | `object` | Plan §4.2. |
| `publication authority` | `capability` | Plan §4.7. |
| `envelope` | `approval envelope` | Shorthand used in task contracts. |
| `bundle` | `review bundle` | Shorthand used in task contracts. |
| `receipt` | `publication receipt` | Shorthand used in task contracts. |
| `epoch` | `policy epoch` | Shorthand used in task contracts. |
| `manifest` | `file manifest` | Shorthand used in task contracts. |

<!-- aliases:end -->

**Banned abbreviation.** `CAS` is ambiguous in this repository — it reads as both
*compare-and-swap* and *content-addressed storage*, which are terms in two different graphs. It is
therefore not an alias and must not appear in prose, in an identifier or in a schema field: write
`compare-and-swap` or name the `mesh-cas` crate. The crate name itself is exempt, because a crate
name is a term with one definition.

### 4.1 Declared non-terms

TL-8 (§6) requires every backticked word in this document and in
[`docs/consistency.md`](consistency.md) to resolve to a register term, an alias, an enumerated
member value or a row of the table below. Repository paths, command-line flags, HTML markers and
entity IDs are recognised by shape and are never asked to resolve. Everything else that is quoted
but is *not* a protocol term is declared here, so that "this word has no definition" is a deliberate
statement rather than an oversight.

**The table is the declaration; this prose is not.** An earlier revision exempted the plan's
`ApprovalEnvelope` members, plan §4.2's `Device` and task-document section names in this paragraph
alone, and left them out of the table — which read as a complete declaration to a human and as
thirteen undeclared words to TL-8. Running the check found all thirteen. They are rows now. A
sentence saying a word is exempt is not a declaration; only a row is.

<!-- non-terms:begin -->

| Literal | Why it is not a term |
|---|---|
| `CAS` | The banned abbreviation above. Quoted only to ban it. |
| `Home` | A column name in the register, not a concept in the protocol. |
| `graph` | A column name in the register. The concept is `state graph`, `operation graph`, `dependency graph` or `trust graph`. |
| `state` | A value of the register's graph column. |
| `dependency` | A value of the register's graph column. |
| `trust` | A value of the register's graph column. |
| `—` | The register's graph value for a cross-cutting term. |
| `pub` | A Rust keyword, quoted as source text. |
| `async` | A Rust keyword, quoted in §3.10 and §6 as one of the qualifiers TL-5 reads past to reach an item's name. |
| `unsafe` | A Rust keyword, quoted in §3.10 and §6 as one of the qualifiers TL-5 reads past to reach an item's name. |
| `const` | A Rust keyword, quoted in §3.10 and §6 as one of the qualifiers TL-5 reads past to reach an item's name. |
| `extern` | A Rust keyword, quoted in §3.10 and §6 as one of the qualifiers TL-5 reads past to reach an item's name. |
| `test` | A `package.json` script name, quoted in §6 where the gate's wiring is described. |
| `verify` | A `package.json` script name, quoted in §6 where the gate's wiring is described. |
| `verify:terminology` | The `package.json` script that runs this gate, quoted in §6 as a script name. |
| `write(2)` | A POSIX syscall, quoted as an external name. |
| `vector clock` | A literature construct Mesh does not use — causality is carried by causal parents and ordered by `hybrid logical time`. Quoted in §1.2 only to name a word banned from product surfaces. |
| `branch` | One of [charter §5](charter.md#5-what-the-words-mean-to-a-user)'s nine never-exposed words. Quoted in §1.2 only to name it; Mesh has no branch concept. |
| `commit` | One of charter §5's nine never-exposed words, and a Git concept Mesh does not have. It appears in these documents only as ordinary English, chiefly inside `durable commit sequence`. |
| `rebase` | One of charter §5's nine never-exposed words. It appears here only as the ordinary verb for the thing `reclassification` refuses to do silently; the term for the procedure is `reclassification`. |
| `staging` | One of charter §5's nine never-exposed words. Quoted in §1.2 only to name it; Mesh has no staging area. |
| `ref` | One of charter §5's nine never-exposed words. Quoted in §1.2 only to name it; the register's naming terms are `entity ID` and `record ID`. |
| `operation log` | One of charter §5's nine never-exposed words. Quoted in §1.2 only to name it; the concept is `operation graph`. |
| `## Allowed paths` | A section name in a task document, not a protocol concept. |
| `npm test` | A repository build command, quoted in §6 where the gate's wiring is described. |
| `package.json` | A repository build file, quoted in §6 as a filename. |
| `ApprovalEnvelope` | The Rust type name in plan §4.7, quoted verbatim in §2.4's reconciliation table. The term is `approval envelope`. |
| `workspace_id` | A member name of plan §4.7's `ApprovalEnvelope`, quoted as source text. The term is `workspace`. |
| `expected_canonical_head` | A member name of plan §4.7's `ApprovalEnvelope`, quoted as source text. The term is `canonical head`. |
| `reviewed_actor_head` | A member name of plan §4.7's `ApprovalEnvelope`, quoted as source text. The term is `actor head`. |
| `review_bundle_id` | A member name of plan §4.7's `ApprovalEnvelope`, quoted as source text. The term is `review bundle`. |
| `selected_changes` | A member name of plan §4.7's `ApprovalEnvelope`, quoted as source text. The term is `selection`. |
| `conflict_resolutions` | A member name of plan §4.7's `ApprovalEnvelope`, quoted as source text. The concept is owned by §7.2, reserved for T126. |
| `validation_digest` | A member name of plan §4.7's `ApprovalEnvelope`, quoted as source text. The evidence it digests is a `validation result`. |
| `policy_epoch` | A member name of plan §4.7's `ApprovalEnvelope`, quoted as source text. The term is `policy epoch`. |
| `approved_by` | A member name of plan §4.7's `ApprovalEnvelope`, quoted as source text. The term is `actor`. |
| `signature` | A member name of plan §4.7's `ApprovalEnvelope`, quoted as source text. It is the tenth struct member and the one the other nine are bound by — §2.4. |
| `Device` | Plan §4.2's spelling of an actor kind, quoted only where §2.4 and §3.1 record the rename. The term is `device actor`; `device` is the machine. |

<!-- non-terms:end -->

---

## 5. The bridge to user-facing wording

The internal model is a graph; the user model is six words. The mapping is
[charter §5](charter.md#5-what-the-words-mean-to-a-user) and it is not restated here, because a
copy would be a second definition. Two rules apply to every surface:

- A register term never appears in a product surface. Translation happens at the surface boundary,
  in exactly one place per surface.
- A user-facing word never acquires a protocol meaning. If a concept cannot be said in the six
  words, that is a product design question for the product owner — never a seventh word added
  locally.

---

## 6. The terminology lint

> **This section describes a check that runs.** Its automated validation exits zero against this revision:
>
> ```console
> $ node tools/program/vocab-lint/lint.mjs --terminology; echo "exit=$?"
>   ok   baseline  clean fixture produces no findings
>   ok   TL-1      the same term gets a second row
>   ok   TL-2      a row carries a graph value outside the vocabulary
>   ok   TL-3      an alias points at a term that does not exist
>   ok   TL-4      a crate directory exists with no register row
>   ok   TL-5      a crate declares a public item with no register row
>   ok   TL-5      a private module’s item is re-exported with no register row
>   ok   TL-5      a re-export renames an item to a word with no register row
>   ok   TL-5      a whole module is re-exported with a glob
>   ok   TL-5      a crate root declares a public `async fn`
>   ok   TL-5      a crate root declares a public `unsafe fn`
>   ok   TL-5      a crate root declares a public `extern "C" fn`
>   ok   TL-5      a crate root declares a public `unsafe trait`
>   ok   TL-5      a crate root declares a public `const fn`
>   ok   TL-5      a crate root declares a public `unsafe extern "C" fn`
>   ok   TL-5      an item is re-exported from the same module a bare re-export republishes whole
>   ok   TL-6      a crate term names somebody else’s directory
>   ok   TL-7      a companion document defines a concept the register does not own
>   ok   TL-8      prose uses a backticked word that resolves to nothing
>   ok   TL-9      two enumerated terms share a member value
>   ok   TL-10      a register row is homed at a real crate that publishes no such item
>   ok   TL-10      a register row is homed at the wrong crate
> self-test: pass (21 mutations over 10 checks)
>
> terminology: clean
>   384 terms · 12 aliases · 74 member values · 25 crates · 10 checks
> exit=0
> ```
>
> Every statement in this document of the form "TL-*n* rejects…" now describes something that
> happens. The two lines that matter are the last two: *clean* is the register passing, and
> *self-test: pass* is each of the ten checks having been broken on purpose and having fired. One
> without the other is not evidence — a check that cannot fail proves nothing by passing. The same
> command is the `verify:terminology` script, which `npm test` reaches, so the transcript above is
> reproduced on every merge rather than on request.

That block is the transcript recorded when the terminology gate landed, not a claim about this
revision. The later internal audit recorded 423 terms, 393 findings, and exit 1; those historical
counts have not been refreshed for this public tree. The terminology diagnostic is not on the
public `npm test` path. Run it explicitly before relying on its current findings. The executable
user-facing vocabulary checks remain on the desktop test path.

**Where it lives.** Behind `--terminology` on the same entrypoint as the existing `--user-facing`
mode, dispatched whole to a separate module. Two modes, two word lists, one entrypoint: §1.2 is why
the two vocabularies must never be merged into one list, and the dispatch is what keeps them from
merging by accident.

**Parsing.**

1. The register is the Markdown tables between `<!-- terminology:begin -->` and
   `<!-- terminology:end -->` in this file, the alias table is between `<!-- aliases:begin -->` and
   `<!-- aliases:end -->`, and the declared non-terms are between `<!-- non-terms:begin -->` and
   `<!-- non-terms:end -->` (§4.1).
2. A register row is a four-cell pipe row — term, definition, graph, home. The term cell holds
   exactly one backticked token; the definition cell is non-empty.
3. The graph cell is one of `state`, `operation`, `dependency`, `trust`, `—`.
4. A definition containing "exactly one of:" declares the backticked words that follow as that
   term's enumerated member values.
5. Fenced code is blanked before any of the above, including a fence nested inside a blockquote —
   as the console transcript above is. Examples inside a fence are never read as terminology usage.

**Checks — each failure names the offending term, its file and its line.**

| ID | Check |
|---|---|
| TL-1 | Every register term is unique across the whole register. A term appearing in two rows fails. |
| TL-2 | Every register row has a non-empty definition, a valid graph value and a non-empty home. |
| TL-3 | Every alias resolves to an existing register term, and no alias is itself a register term. |
| TL-4 | Every directory name under `crates/` appears as a register term. |
| TL-5 | Every public type or value a crate publishes from its `src/lib.rs` — declared there, or declared in a private module and re-exported from there — appears as a register term. A re-export of a whole module is rejected by name rather than expanded. |
| TL-6 | Every crate whose name is a register term has a home cell naming its own directory. |
| TL-7 | **No definition outside the register.** A numbered level-three heading whose title is followed by an em dash and a gloss, in this document or in [`docs/consistency.md`](consistency.md), introduces a concept — so that concept must be a register term or an alias. This is what stops a companion document from growing its own definitions. |
| TL-8 | **No undefined term in prose.** Every backticked word in either document resolves to a register term, an alias, an enumerated member value or a declared non-term (§4.1). Repository paths, flags, HTML markers and entity IDs are structural and exempt. |
| TL-9 | **No member value carries two meanings.** No enumerated member value is declared by two different register terms. |
| TL-10 | **No register row for an item that does not exist.** Every §3.10 row whose home cell names exactly one crate directory names a public item that crate actually publishes. This is TL-5 run backwards: TL-5 catches an item with no row, TL-10 catches a row with no item. A crate-name row is exempt — TL-4 and TL-6 own that column — and so is a row homed `crates/*` or at a document. |

**Every check ships with a violating mutation.** The self-test builds a synthetic register and a
synthetic crate, breaks one rule, and asserts the matching check fires; it fails if the clean
fixture produces a finding, if any mutation fails to fire, or if any check has no mutation at all.
That last clause is what stops a tenth check being added with no way to prove it works. A check may
carry several mutations — one reach each — and a mutation may also state what its finding has to
say, because firing on the wrong item is not the check working. This is a requirement of
[`docs/consistency.md`](consistency.md) §6 applied to the tool that enforces §6.

**What the checks do not cover, and this is the honest ceiling.** Four limits, all deliberate. The
count said three for as long as TL-10's direction was missing entirely and unlisted, which is the
failure mode a ceiling paragraph is supposed to prevent:

- TL-8 sees quoted words. A concept written in bare prose, never quoted and never given a heading,
  is outside every check here and is caught only by review. The register's own definitions are held
  to the quoted form precisely so that TL-8 can reach them.
- TL-5 reads what a crate root publishes — an item declared in `src/lib.rs`, whatever `async`,
  `unsafe`, `const` or `extern` qualifier stands between `pub` and the item keyword, and an item
  declared in a private module and re-exported from `src/lib.rs` by name, resolved to the module
  that declares it. It does not read module declarations, and a re-export that republishes a whole
  module by name resolves to that module's file and is skipped rather than demanded as a term.
  `mesh-bench` publishes twelve modules whose names are internal benchmark plumbing — ordinary
  English words that would each acquire a protocol meaning the moment they became register rows,
  which is the exact failure this document exists to prevent. Two costs follow. An item made public
  *through* such a module, rather than re-exported from the crate root, is not reached: the register
  covers the flat surface, and a crate that wants its item covered re-exports it. And a re-export of
  a whole module's contents with a glob publishes names `src/lib.rs` does not list, so TL-5 rejects
  it by name instead of expanding it — expanding it would let a crate's public surface grow without
  `src/lib.rs` changing, which is the silent drift the check exists to catch. The full argument, the
  alternatives it beat and the measurement behind it are
  [`docs/adr/0019-reject-a-glob-re-export-rather-than-expand-it.md`](design-decisions.md#adr-0019).
  This bullet cited `0027` until `01KZD0D413BE4GMX5RA9H3V8Z8`; `0027` was one of three copies of that
  one decision, and the two later copies are gone.
- TL-10 reads the home cell, so it only asks a row to name a published item when that row is homed
  at exactly one crate directory. A row homed `crates/*`, at a document, or at a bare crate name is
  a concept rather than a published item and is never asked; a fabricated row can therefore still
  hide behind a home cell that claims no single crate. Moving a row to such a home to silence TL-10
  is the manoeuvre this sentence exists to make visible.
- The checks read this document and [`docs/consistency.md`](consistency.md). A third document
  introducing a term is caught when it is added to the list, not before.

**Consequence of failure.** A term used in code with no definition here blocks that code's change
until the term is defined or the code is renamed. This is the intended friction: the register is
cheap to extend and expensive to bypass.

**Current public gate wiring.** The terminology command and historical transcript above are
retained as diagnostic evidence. The public package does not define `verify:terminology`, and
`npm test` does not run that diagnostic. It does run the user-facing vocabulary gate through the
desktop suite. See [the developer guide](developer-guide.md) for the complete current validation
commands. The earlier internal wiring described in the historical transcript is not a current
public-repository guarantee.

---

## 7. Reserved sections

These sections are reserved so that later work extends this document instead of starting a
parallel vocabulary. Each names its owner; none may redefine a term in §3.

### 7.1 Operation reference

*Reserved for T122 `01KZC29CAJ32VSXP9T9RZNKVN1`.* The eighteen operations of the operation
vocabulary, each with its fields, preconditions and the state-graph effect it produces.

### 7.2 Conflict rules table

*Reserved for T126 `01KZC2D8Q6CAPS2CBS32QQTF5P`.* The eleven conflict rules, each with its
concurrent pattern, its mandated outcome, and the test that fails if the rule is removed.

### 7.3 Wire message set and knowledge model

*Owned by T148 `01KZC2EGC8E02DHP5BD6CQN7QE`.* Plan §5.2–5.4's message set, the plane separation and
the per-peer knowledge model. Implemented in `crates/mesh-sync-protocol`; the transport that
carries these messages is `mesh-sync-engine`, which does not exist yet. `crates/mesh-sync-protocol/tests/protocol-vectors.rs`
pins the bytes of every message below.

**An external implementer reads [`protocol/wire/`](../protocol/wire/README.md), not that table.**
The message set is published there in a language-neutral form — each message's fields in encoding
order and one complete example with its exact bytes — because a table of bytes inside a Rust test is
not a publication. The published bytes are a mirror of the pinned table rather than a generated
corpus, and `protocol/verify-published.mjs` fails if the two ever disagree; why it is a mirror, and
what would replace it, is [`protocol/README.md`](../protocol/README.md) §6.4.

**The protobuf projection of this table is
[`protocol/proto/mesh/v0/sync.proto`](../protocol/proto/mesh/v0/sync.proto)** — plan §8.1's network
half: one message per message below, plus one per compound element they carry, held to this table
by `crates/mesh-sync-protocol/tests/proto_projection.rs`. It declares no service and no envelope —
the alternatives are in its header comment and the ADR recording them is owed by task
`01KZGA1KHXE7D9W6DC9MK0D01K` — and **it is not the encoding a CWP message is defined in**: a peer
re-encodes to `mesh-cbor/0` before comparing any bytes, exactly as it does for a signed record.

#### Three planes

The plan names two message planes. There are three values here, because `HELLO`, `AUTHENTICATE`
and `ERROR` belong to neither: filing them under the `metadata plane` would put a handshake inside
the measurement the metadata-plane latency budget is about. `MessagePlane` is therefore exactly one
of `handshake`, `metadata` and `content`.

The separation is a type-level fact rather than a convention, and the property it buys is plan
§5.2's: **a peer learns that an object changed before the bytes of that object arrive.** Only
`REQUEST_CHUNKS`, `CHUNK_BATCH` and `ACK_CHUNKS` are on the `content plane`, so nothing that
carries visibility can be queued behind chunk bytes.

#### The message set

Eighteen messages, in plan §5.3's order. A wire tag is assigned once and is never reused or
renumbered; a tag this version does not define is answered `unknown message` rather than guessed.

| Tag | Message | Plane | Fields | Preconditions, checked by both sides |
|---|---|---|---|---|
| 1 | `HELLO` | `handshake` | protocol version, record encoding profile, claimed `actor`, challenge | the version is this one and the profile is `mesh-cbor/0`, else `unsupported version` |
| 2 | `AUTHENTICATE` | `handshake` | `actor`, echoed challenge, signature | a non-empty signature; `HELLO` first, and the same `actor` it claimed |
| 3 | `ADVERTISE_FRONTIER` | `metadata` | one `HeadAdvertisement` per `actor`, optional `canonical head`, `policy epoch` | entries ascend by `actor`; each sparse set ascends by `actor sequence number` and lies strictly beyond the contiguous run |
| 4 | `REQUEST_OPERATIONS` | `metadata` | `actor`, sequence to resume after, specific ChangeSets, maximum batch | a non-zero maximum; the identifier list ascends and holds no duplicate |
| 5 | `OPERATIONS_BATCH` | `metadata` | ChangeSets, each as `CarriedChangeSet` | non-empty; at most `MAX_OPERATIONS_PER_BATCH`, else `batch too large`; no sequence zero; every body non-empty |
| 6 | `ACK_OPERATIONS` | `metadata` | `actor`, contiguous sequence, sparse set | as `ADVERTISE_FRONTIER`'s sparse rule |
| 7 | `ADVERTISE_MANIFESTS` | `metadata` | file manifests held | ascending, no duplicate |
| 8 | `REQUEST_CHUNKS` | `content` | one `ChunkRequest` per chunk | non-empty; ascending by `ContentHash`; each byte bound non-zero |
| 9 | `CHUNK_BATCH` | `content` | one `ChunkPart` per slice of bytes | non-empty; every part non-empty and at most `MAX_CHUNK_PART_BYTES`; no part ends past the largest representable offset |
| 10 | `ACK_CHUNKS` | `content` | chunks verified on receipt | ascending, no duplicate |
| 11 | `UPDATE_ACTOR_HEAD` | `metadata` | `actor`, `actor head`, sequence | the sequence is not zero: an actor with no ChangeSets has no head |
| 12 | `UPDATE_CANONICAL_HEAD` | `metadata` | `canonical head`, `policy epoch`, `publication receipt` identifier | none beyond decoding; the receipt is verified where the bytes are |
| 13 | `PRESENCE` | `metadata` | `actor`, `PresenceState`, time to live | a non-zero time to live |
| 14 | `REVIEW_BUNDLE` | `metadata` | `review bundle` identifier, canonical bytes | a non-empty body |
| 15 | `VALIDATION_RECEIPT` | `metadata` | the ChangeSet it is about, canonical bytes | a non-empty body |
| 16 | `APPROVAL_ENVELOPE` | `metadata` | `approval envelope` identifier, canonical bytes | a non-empty body |
| 17 | `ANTI_ENTROPY_SUMMARY` | `metadata` | `actor`, `MerkleSummary` | the summary's runs are non-empty, ascending and contiguous |
| 18 | `ERROR` | `handshake` | `ErrorCode`, the message refused, a diagnostic detail | none: a refusal must always be sendable |

**Records this protocol does not own travel opaque.** A ChangeSet, a `review bundle`, a validation
result and an `approval envelope` are carried as their `canonical encoding` bytes plus their
`record ID`, never as a second set of fields. Their signatures were made over those bytes; a second
description of a signed record is a description that will one day disagree with what was signed.
The fields `CarriedChangeSet` does spell out — `actor sequence number`, `causal parent` set,
`base head`, `resulting head`, `policy epoch` — are the ones a receiver needs to plan its next
request before it has decoded anything, and every one is redundant with the body beside it.

#### Error cases

`ErrorCode` is closed: `unsupported version`, `unknown message`, `unauthenticated session`,
`unverified peer`, `malformed message`, `unknown actor`, `sequence gap`, `unknown ChangeSet`,
`unknown chunk`, `offset past end`, `integrity failure`, `unknown policy epoch`, `batch too large`,
`busy`. A sender decides what to do from the code alone; the detail string is diagnostic and is
never parsed. Only `busy` and `unknown policy epoch` are retryable — every other code says the
message itself is wrong, and resending it is a loop.

#### What each peer tracks about every other peer

The `knowledge set` is plan §5.4's list and nothing else: per `actor`, the maximum contiguous
`actor sequence number` and the ChangeSets held beyond it; the head set; `file manifest`
availability; `chunk` availability; the `policy epoch`; the `canonical head`.

Four properties make it usable rather than merely present.

- **One shape, two uses.** Held about yourself it is exact; held about a `peer` it is a belief.
  They are the same type, so `ReplicationGap` — the difference between two of them — computes both
  "what must I ask you for" and "what must I send you" with no second implementation to disagree.
- **Monotone under observation.** Nothing a `peer` says lowers what you believe it holds. A stale
  advertisement therefore costs a retransmission, never a lost delivery.
- **Sparse entries carry their sequence.** Without it a receiver knows a ChangeSet is beyond some
  hole but not which, and so cannot ask for what is between. A sparse entry at or below the
  contiguous sequence is refused `sequence gap`: no store can be in that state.
- **`presence` is not knowledge.** It is ephemeral and expires; a `knowledge set` has no expiry, so
  folding presence into one would make a permanent record of a signal that is allowed to be lost.

#### Merkle summaries for large histories

`ANTI_ENTROPY_SUMMARY` carries a `MerkleSummary`: a leaf per fixed-width run of one `actor`'s
sequence holding a digest of the identifiers in that run, and a root over the leaves. Two peers
compare roots in one round trip and, when they differ, compare leaves to learn exactly which ranges
to ask about — which is what keeps `anti-entropy` cheaper than the history it repairs. The depth is
not fixed by the protocol, because the `peer` that builds the summary chooses the run width and
sends the leaves it built. A summary with a hole in it is refused, because a missing range would
otherwise read as a range that agrees.

#### Encoding

A message is a two-element array — the wire tag, then the fields in the order the table above lists
them — in `mesh-cbor/0`, the same `canonical encoding` a signed record uses. CWP messages carry no
signature, so canonicity here is a conformance requirement rather than a security one: one message
must have exactly one encoding, or the published vectors would pin one encoder's habits instead of
the format. A decoder refuses a head longer than its value needs, a trailing byte after a complete
message and a length that exceeds the bytes that follow it.

#### Authentication: what is true today, stated rather than implied

**Nothing in Mesh can produce a signature in production yet.** `mesh-crypto` verifies real Ed25519
and holds no secret half; key custody is a platform backend nobody has built. A `peer` therefore
cannot present evidence of who it is, and `AUTHENTICATE` has nothing to carry.

The protocol does not paper over that. `PeerAuthenticator` is the seam a real verifier plugs into;
`NoAuthenticator` is the only implementation shipped and answers `unverified` for every `peer`,
never `verified`. `AuthenticationPolicy` is exactly one of `require verified peers` and
`admit unverified peers`, and on the current tree the first admits **nothing at all** — which is
the correct behaviour for a posture nothing can satisfy, and is asserted as a test rather than
described here. Replication today runs under the second, between peers whose identity is asserted
and not checked.

---

## 8. Changing this document

- **Adding a term** is a normal change: add one register row, use it.
- **Changing a definition** is a protocol change. It requires protocol review and, if any signed
  record's meaning moves, a test vector and a compatibility test in the same change.
- **Removing a term** requires that nothing still uses it. Until the lint of §6 exists, "nothing
  still uses it" is a claim the author checks by hand and the reviewer re-checks; TL-4, TL-5 and
  TL-8 name the part a machine would be able to prove. An alias row is the compatible way to retire
  a word.
- **A term with two candidate meanings** is two terms or a design defect. It is never resolved by
  writing a second definition, and never by softening the first.

**What a change costs outside this document** — which changes are breaking for someone who has
already built against the published material, what a compatibility event carries, and how a record
type, a message or an error code is retired — is
[`protocol/VERSIONING.md`](../protocol/VERSIONING.md). This section governs the register; that one
governs the bytes.
