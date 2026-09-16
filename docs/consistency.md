# CWP consistency model

This document states **what is authoritative, what converges, what is allowed to be lost, and
what each promise means**. It is the companion to [`docs/protocol.md`](protocol.md), which defines
the four core graphs and owns every term used below.

**No term is defined here.** The four consistency classes this document is built on —
`private actor state`, `canonical state`, `presence` and `derived state` — are register terms
(protocol §3.6 and §3.7). What follows states their *properties*, their promises and their failure
behaviour; the definitions stay in the register. TL-7 and TL-8 (protocol §6) fail this document if a
section here introduces a concept the register does not own, or quotes a word that resolves to
nothing — **and they run**. What they cannot see is a concept written in bare prose, never quoted
and never given a heading; protocol §6 states that ceiling, and it is the reason to read §6 before
treating any claim here as machine-checked.

Normative source: the Mesh execution plan, §5.1 (consistency model), §4.4 (actor heads) and §4.7
(human publication). Product rules are cited from [`docs/charter.md`](charter.md), never restated.

The one sentence the rest of the document elaborates:

> **An actor's own state is authoritative for that actor and converges causally across peers.
> Canonical state is linearizable and advances only by human-authorized compare-and-swap.
> Presence is best-effort and may be lost freely. Derived state may be lost and rebuilt. Nothing
> else may be lost.**

---

## 1. The four classes of state

Every fact Mesh holds belongs to exactly one class. The class decides its durability, its
ordering, its consistency guarantee and — the load-bearing part — whether it is allowed to be
lossy. The row that says *what it is* is a pointer to the register, not a definition: the classes
are defined in protocol §3.6 and §3.7.

| | `private actor state` | `canonical state` | `presence` | `derived state` |
|---|---|---|---|---|
| **What it is** | Register §3.7 — one actor's ChangeSets, actor head and checkpoints | Register §3.7 — the canonical head and its canonical transition sequence | Register §3.6 — reachability and liveness of actors and devices | Register §3.7 — indexes, caches, read observations, derivations, availability views |
| **Authority** | The owning actor | The workspace, advanced only by an approval envelope | Nobody — it is an observation | Nobody — it is a function of the other three |
| **Writer** | The owning actor only | Compare-and-swap on the canonical head only | The observing daemon | The recomputing process |
| **Durability** | Durable after the acknowledgement boundary | Durable on admission | Never durable; TTL-bounded | Rebuildable, not durable |
| **Ordering** | Causal; no global total order | Total; one predecessor per state | None | Inherited from its inputs |
| **Consistency** | Read-your-writes and monotonic locally; causal and convergent across peers | Linearizable | Best-effort | Eventually correct after rebuild |
| **Lossy?** | **No**, after acknowledgement | **No** | **Yes, freely** | **Yes, recoverably** |
| **Where** | `mesh-state`, `mesh-store` | `mesh-approval` | `mesh-daemon` | `mesh-context-ledger`, `mesh-derivations`, `mesh-store` |

### 1.1 Private actor state — authoritative, causal, never lossy after acknowledgement

Every actor works privately and sees the latest canonical state plus its own changes
([charter P2](charter.md#p2--every-actor-works-privately)). An actor's head is advanced only by
that actor's ChangeSets and by incorporating canonical advances.

- **Before the acknowledgement boundary**, nothing is promised. Work in flight may be lost by a
  crash, and the system must never report it as saved.
- **After the acknowledgement boundary**, the checkpoint is durable and is never lost, never
  rewritten and never overwritten by a conflict resolution
  ([charter P5](charter.md#p5--no-valid-work-is-silently-discarded)).
- Replication makes a checkpoint visible to peers. It does not make it canonical
  ([charter P3](charter.md#p3--private-state-is-durable-and-remotely-visible)).

There is deliberately **no global total order** over private state. Two actors' concurrent
ChangeSets are concurrent, full stop; there is no fact of the matter about which came "first", and
the protocol never invents one.

### 1.2 Canonical state — linearizable, single-writer, never lossy

The canonical head is the one protected shared state. It advances by exactly one mechanism: a
canonical transition authorized by one approval envelope and admitted by compare-and-swap against
the expected canonical head.

- **Linearizable.** Of N concurrent publication attempts against the same expected head, exactly
  one succeeds; the rest fail as stale, naming the current head. No observer sees a partial
  advance.
- **Human-gated.** No agent-scoped key can produce a valid approval envelope
  ([charter P1](charter.md#p1--work-is-automatic-publication-is-explicit)).
- **Exactly the reviewed bytes.** A `stale base` never silently rebases; it routes to
  `reclassification`, which either rebases by an explicit rule or requires re-review
  ([charter P7](charter.md#p7--approval-refers-to-exact-bytes)).

Serializing canonical advancement is precisely what makes it safe to leave everything else
lock-free: the only totally ordered thing in the system is the one thing a human signs for.

### 1.3 Presence — ephemeral, best-effort, explicitly lossy

Presence answers "is this actor reachable right now". It is the only class that may be lost
without recovery, and it is allowed to be lost because losing it costs nothing.

- Presence is TTL-bounded and expires by default rather than by an explicit clear.
- Presence **never enters canonical history** and never becomes an input to a canonical transition.
- Presence is **never a durability signal**. "A peer is online" says nothing about whether that
  peer's work is saved, replicated or retrievable.
- `availability state` is *not* `presence`. Its members `local only`, `metadata replicated` and
  `content available` are facts about content retrievability; they are derived rather than
  ephemeral, and they are never conflated with reachability on any surface.
- `availability state` is also not `head state`. Retrievability and review position are two
  independent axes of one actor head (protocol §2.1) and share no member value; TL-9 rejects a
  register row merging them back into one enumeration, and it runs (protocol §6).

### 1.4 Derived state — lossy but recoverable

Indexes, caches, materialized views, availability computations, read observations and derivations
are functions of the other three classes. Deleting them costs work, not truth.

- Any derived artifact must be reconstructible from private and canonical state alone. If it is
  not, it is misclassified and belongs in a durable class.
- **Losing derived state loses knowledge, never work.** A lost read observation degrades staleness
  detection; it never loses a version.
- A derived value is never an authority. In particular, a cached validity state never overrides a
  recomputation, and a derived availability view never authorizes a canonical transition.
- Attribution honesty applies here: where a read is inferred rather than exact, it is recorded at
  reduced confidence and the reduction is never quietly upgraded
  ([charter P10](charter.md#p10--reliability-claims-must-be-measurable),
  [charter §6](charter.md#6-poc-non-goals)).

---

## 2. The lossiness contract

This is the table an implementer checks a design against. "Lossy" means the system may discard the
fact without recovering it, and must still be correct.

| Fact | Lossy? | Condition | Consequence of loss |
|---|---|---|---|
| Unacknowledged in-flight write | Yes | Crash before the acknowledgement boundary | The write did not happen and was never reported as saved. |
| Acknowledged checkpoint | **No** | — | P0 defect. Zero acknowledged-state loss is the durability claim. |
| ChangeSet after acknowledgement | **No** | — | Convergence and attribution both break. |
| Durable version losing a conflict | **No** | — | Violates [charter P5](charter.md#p5--no-valid-work-is-silently-discarded); the losing version stays reachable. |
| Content reachable from a retained root | **No** | — | Collector defect; a collector that removes reachable content is a P0. |
| Content unreachable from every retained root | Yes | Retention policy permits it — and per `01KZG94AFZP999E8F8ZF2X574E` the policy permits it only by **retiring a named root**, never by pruning inside one. The default policy retires nothing, so on a workspace whose actors are all present this row covers crash residue and nothing else. | Intended reclamation, attributable to the root that was retired. |
| Canonical head or any canonical transition | **No** | — | The shared version stops being a single well-defined thing. |
| Approval envelope or publication receipt | **No** | — | The trust claim becomes unauditable. |
| Presence | **Yes, freely** | Any time; expires by TTL | Peers appear offline until the next signal. No work is affected. |
| Availability state | Yes | Recomputed from manifests and chunk availability | A surface briefly understates what is retrievable; never overstates it. |
| Read observation | Yes | Rebuildable only going forward | Staleness detection degrades for the affected reads; detection rate is measured and published, never assumed. |
| Derivation output | Yes | Deterministic derivations recompute; non-deterministic ones are rerun | Cost, not correctness. |
| Local index and cache | Yes | Reconstructible by recovery | Recovery time, not data. |
| Event ledger entry | Yes | Diagnostics only | Diagnostic depth, not protocol state. |

Two rules follow from the table and are worth stating as rules:

- **Never report a promise earlier than the class allows.** A surface reports "saved privately"
  only after the transaction commits, and "available to team" only when content is genuinely
  retrievable by a peer.
- **A lossy class may never gate a non-lossy one.** Publication never waits on presence;
  durability never depends on a cache; a canonical transition never consults an availability view.

---

## 3. Ordering

| Question | Rule |
|---|---|
| Did A happen before B? | Only if A is a transitive causal parent of B. Otherwise they are concurrent. |
| How is a total order produced when one is needed for display or tie-breaking? | Causal order first, then hybrid logical time, then record ID. Deterministic on every peer. |
| Does wall-clock time ever decide anything? | **No.** Head advancement never consults it; a wrong system clock changes no head. Hybrid logical time is metadata and a tiebreak, never evidence of causality. |
| Is there a global total order over all changes? | **No**, and there deliberately never will be. Only the canonical transition sequence is totally ordered. |
| What orders delivery? | Nothing. Delivery may duplicate and reorder freely; causal delivery and idempotence make the outcome independent of it. |

---

## 4. What each promise means

Every user-visible promise maps to exactly one internal condition. The user wording is
[charter §5](charter.md#5-what-the-words-mean-to-a-user); the conditions are here.

| Promise | True exactly when | Never implied by |
|---|---|---|
| Work is captured | A ChangeSet exists for the change in the local durable index | A raw write completing |
| Saved privately | The durable commit transaction has committed; the acknowledgement boundary is crossed | Chunk promotion alone, or an optimistic report |
| Available to team | A peer can genuinely retrieve the content: metadata replicated **and** content available | Metadata replication alone, or peer presence |
| Ready for review | A review bundle exists, is immutable, and names an exact base and actor state | An actor asserting it is done |
| Validated | An independent validation result exists against the immutable review snapshot | Agent-reported evidence, however detailed |
| Approved | An approval envelope is signed by a human actor and its canonical transition succeeded the compare-and-swap | A signed envelope alone — a stale base fails |

---

## 5. Behaviour under failure

| Failure | Guaranteed behaviour |
|---|---|
| Network partition or relay unreachable | Local reads and writes complete at local latency; nothing blocks ([charter P4](charter.md#p4--local-work-never-waits-for-the-wan)). Delivery resumes from the outbox and watermarks. |
| Crash before the acknowledgement boundary | No durable head references unpromoted content. Orphaned chunks become collection candidates, never dangling references. Observed at every step — [§5.1](#51-the-durable-commit-sequence-observed-step-by-step). |
| Crash after the acknowledgement boundary | The checkpoint is durable and complete. The index is reconstructible from durable content. What *complete* has and has not been observed to mean is [§5.3](#53-what-recovery-has-not-been-observed-to-mean). |
| Duplicate delivery | Reapplication has no additional effect, verified by state hash. |
| Reordered delivery | A ChangeSet whose causal parent is missing is buffered as a known-missing dependency and applied when the parent arrives. It is never dropped and never collected as invalid. |
| A causal parent that never arrives | The dependent stays buffered and visible as a known-missing dependency, indefinitely. Silence is never resolved by discarding it. |
| Clock skew, including a badly wrong clock | No head, no conflict outcome and no publication decision changes. |
| Concurrent publication | Exactly one compare-and-swap succeeds; the rest fail as stale with the current head named. |
| Long-offline peer returning | Anti-entropy reconciles by summary comparison; the peer's watermark kept its required content alive while it was away. |
| Capability revoked while a peer is offline | Epoch rotation invalidates prior-epoch authority without requiring the peer to be online. |
| Derived state lost or corrupted | It is rebuilt from private and canonical state. No durable version and no canonical transition is affected. |

The two crash rows above are the only rows in this table written from **observation rather than
from intent**. What follows is that observation, and its limits. Everything in §5.1 and §5.2 was
watched to happen on a real machine; §5.3 is the part that was not watched and is therefore not
promised.

### 5.1 The durable commit sequence, observed step by step

Plan §6.3 numbers eleven steps. A real child process was killed after each of them, and the
workspace it left behind was read by its parent. The method, the counts and the reproduction
command are [`tests/crash/README.md`](../tests/crash/README.md); this section is what the
measurement established.

| Step | Killed after | On disk afterwards | Had the user been told? |
|---|---|---|---|
| 1–3 | writing, flushing and verifying temporary chunk data | temporary data only, discardable; no addressable content, no reference, empty index | no |
| 4–8 | promoting chunks, then composing the transaction | content present, whole, and hashing to its own name; its arrival recorded; **nothing references it**; empty index | no |
| 9 | the transaction returned | the whole checkpoint — operations, heads and references, all durable | **no** |
| 10–11 | reporting the save, handing over to replication | the whole checkpoint | **yes** |

Three properties, each asserted after every one of those kills and not only where it looked
interesting:

1. **An acknowledgement is never ahead of durability.** The child records the instant it is told
   the work is saved privately, durably, so the parent can read that fact off a corpse. Wherever
   that record exists, the database holds the whole checkpoint. This is the entire zero-loss claim,
   stated as one implication that a dead process can be checked against.
2. **A durable reference is never ahead of content.** Wherever the index references content, that
   content is present, whole, and verified against its own name. There is no step at which a
   reference exists and its content does not.
3. **Content that became visible is always collectable.** Its arrival is recorded before it becomes
   visible, so an interrupted save leaves a collection candidate and never a leak.

### 5.2 The acknowledgement boundary is one step wide, and it was measured

**Durability is reached at step 9. The user is told at step 10.** In that order, with one step
between them, and it has been observed in that order — not merely specified in it.

That gap is the whole design. A kill inside it loses nothing and promises nothing: the work is
already safe and nobody has been told it is, which is the only asymmetry that is safe to have. The
opposite asymmetry — telling first — is the single failure this sequence exists to prevent, and it
would show up as a corpse carrying the record from property 1 above with an empty database, at
every one of the eight steps that precede durability.

Two further failure modes were observed rather than assumed:

- **Interrupted mid-transaction, the index holds all of it or none of it.** Killed at twelve named
  statement offsets through a transaction of two thousand operations — from `BEGIN IMMEDIATE` alone
  to the whole batch including `COMMIT` — the index was found holding either zero or two thousand,
  never a number in between, and which of the two it was agreed every time with whether `COMMIT`
  had been sent. The campaign fails if its schedule stops covering both sides, so a run that tested
  nothing says so instead of passing. That schedule used to be calibrated on the machine rather
  than enumerated, and it did miss under load; the defect was filed as
  `01KZERXN1BC2FEDNNXNBKTNY7E` and repaired, and the schedule now replays —
  `tests/crash/README.md` §4.7 carries the load-ramp case and the measurement after the repair.
- **A transaction that fails is reported as an unknown outcome, not as a rollback.** The driver
  cannot know whether the failure arrived before or after the commit was durable, so nothing is
  acknowledged and the outcome is named as unknown. The recovery is to reopen the database and read
  it. Acknowledging an unknown is the one thing the sequence never does.

### 5.3 What recovery has not been observed to mean

Stated plainly, because *"the checkpoint recovers intact"* would otherwise be read as covering all
four of these, and it covers none of them.

- **Restarting into a dirty workspace is untested at the sequence layer.** Every killed child was
  given a fresh workspace. Nothing restarts a durable commit sequence in a workspace a previous
  crash left behind, so *"temporary data is discarded at startup"* is observed only at the content
  layer — where a restart does discard it, and is asserted to remove exactly what was staged and
  then to remove nothing. At the sequence layer the discard runs against an empty directory. The
  startup routine that would call it in a shipped product does not exist yet, and has no owner.
- **A recovered checkpoint is asserted non-empty, not identical.** After a kill at steps 9 to 11
  the index is checked to hold operations, heads and references — not to be byte-for-byte the index
  a clean run of the same checkpoint produces. The fingerprint that would say so exists and is not
  compared after a crash.
- **A full disk is injected at the first four steps only.** The transaction steps compose in memory
  and cannot fail for want of space; the commit was made to fail by another mechanism, which is a
  real driver failure and not a full disk. A read-only filesystem is not injected anywhere.
- **None of this is power loss.** Killing a process leaves the page cache intact; losing power does
  not. What is covered is process crashes, out-of-memory kills and forced termination. Power-loss
  durability rests on the platform honouring its own flush and rename guarantees, and on SQLite's
  guarantees under a write-ahead log. Those are assumptions this document makes and does not
  measure, and no number here is evidence for them.

### 5.4 The scope of the zero-loss claim

**Zero acknowledged-state loss** means exactly this and nothing wider: across 47 real processes
killed at every step boundary of the durable commit sequence and of chunk promotion, and inside
both, plus five injected faults, **no killed process was ever found to have reported work saved
that the workspace did not hold**. The count is zero events, not a low rate. It is a statement about
the crash rows of the fault matrix, made on one machine, at one revision, under one build profile,
all six recorded in [`tests/crash/README.md`](../tests/crash/README.md) §1. The remaining rows of
plan §13.3 are a different campaign with a different owner, and this number says nothing about them.

---

## 6. The seven core invariants

These are the protocol's promises stated as checkable claims. Each is stated here **in prose and in
the formal expression that checks it**, with the mutation that must break the check. The models are
[`models/mesh.tla`](../models/mesh.tla) and [`models/mesh_tree.tla`](../models/mesh_tree.tla) (T187
`01KZC2ETYD1F7M95Z4DH589SSG` and T188 `01KZC2JC6E3D5TK6B2FKJMYANX`); the campaign that runs them is
[`models/check.sh`](../models/check.sh); the archived run and its counterexamples are
[`models/results/`](../models/results/). What each check is worth — and the four rows where it is
worth less than it looks — is [§6.2](#62-what-each-check-is-actually-evidence-for) and
[`models/README.md`](../models/README.md).

| ID | Invariant | Prose statement | Graph invariants it rests on |
|---|---|---|---|
| I1 | No agent advances canonical state | No sequence of actions available to an agent-scoped key results in a canonical transition. The capability is unrepresentable, not merely denied. | TG-1, TG-2, TG-3 |
| I2 | An approval cannot publish a different actor state | The state admitted by a canonical transition is byte-identical to the state bound by its approval envelope, restricted to the bound selection. | TG-4, TG-9, SG-2 |
| I3 | No canonical file references unavailable verified content | Every chunk reachable from the canonical head is present and hash-verified before the transition is admitted. | SG-9, TG-8 |
| I4 | No acknowledged actor head becomes unreachable | Once acknowledged, an actor head remains reachable from a retained root until an explicit retention policy permits deletion. | SG-3 |
| I5 | Duplicate delivery is idempotent | Applying an already-applied ChangeSet leaves the state hash unchanged. | OG-4 |
| I6 | Equivalent valid operation sets converge | Two peers holding the same causal set reach the same head under every delivery order that respects causality. | OG-3, OG-5, OG-10, SG-1 |
| I7 | Directory ancestry remains acyclic | Every materialized state is a tree; concurrent cyclic directory moves resolve deterministically and identically on every peer. | SG-5, OG-10 |

Each invariant must have a mutation that violates it. An invariant no mutation can break is not
being checked, and a check that cannot fail is not evidence.

### 6.1 The formal form of each

Every row below has been watched to fail: the mutation column names a switch in the model, the
campaign runs it, and [`models/check.sh`](../models/check.sh) exits non-zero if a mutation *passes*.
The counterexample length is the fourth column of that script's expectation table and is asserted at
one worker.

| ID | Formal expression | Where it must hold | The mutation that breaks it |
|---|---|---|---|
| I1 | `models/mesh.tla:OnlyAnExactHumanReviewedStateAdvances` | `models/mesh.cfg` | `models/mesh-mut-agent-may-approve.cfg` — an agent-scoped key produces an approval envelope |
| I2 | `models/mesh.tla:OnlyAnExactHumanReviewedStateAdvances`, with `models/mesh.tla:CompareAndSwapHeld` and `models/mesh.tla:ApprovalIsSingleUse` | `models/mesh.cfg`, `models/mesh-publication.cfg` | `models/mesh-mut-silent-rebase.cfg` — the admitted state is the reviewed one merged onto the current head; also `models/mesh-mut-no-compare-and-swap.cfg` and `models/mesh-mut-replay-approval.cfg` |
| I3 | `models/mesh.tla:CanonicalContentIsAvailable` | `models/mesh.cfg` | `models/mesh-mut-publish-without-content.cfg` — a transition is admitted whose content is not retrievable |
| I4 | `models/mesh.tla:AcknowledgedWorkIsNeverDiscarded`, with `models/mesh.tla:ConcurrentWorkIsPreserved` | `models/mesh.cfg` | `models/mesh-mut-collect-unpublished.cfg` — the collector keeps only what the canonical head references |
| I5 | `models/mesh.tla:DuplicateDeliveryIsIdempotent` (an *action* property, not an invariant) | `models/mesh-idempotence.cfg` | `models/mesh-mut-redeliver-reorders.cfg` — a second arrival of a held identifier re-ranks it |
| I6 | `models/mesh.tla:Convergence`, its condition `models/mesh.tla:ConvergenceUnderIdentifierBinding`, and its failure mode `models/mesh.tla:NoSilentDivergence` | `models/mesh.cfg`; the condition additionally in `models/mesh-divergence-conditional.cfg` | `models/mesh-mut-order-by-arrival.cfg` — the order becomes the order records arrived in; and `models/mesh-divergence.cfg`, which removes no rule and one *assumption* |
| I7 | `models/mesh_tree.tla:DirectoryAncestryIsAcyclic` and `models/mesh_tree.tla:ConcurrentMovesResolveIdenticallyOnEveryPeer` | `models/mesh-tree.cfg` | `models/mesh-tree-mut-no-cycle-check.cfg` — the move rule loses its ancestor check; `models/mesh-tree-mut-arrival-order.cfg` — the fold applies moves in the order received |

### 6.2 What each check is actually evidence for

A passing run is evidence about a *model*, and only about the part of the model that tracks code
somebody wrote. Four of these seven are worth less than the row above makes them look, and the whole
value of stating so is that a reader does not have to find out.

| ID | What the check establishes, and what it does not |
|---|---|
| I1, I2 | The publication half of the model tracks **the specification, not an implementation**. `crates/mesh-approval` is a twenty-one-line placeholder: no envelope, no compare-and-swap. These invariants say what an implementation would have to satisfy; they cannot say anything satisfies it. "No agent-scoped key can produce a valid approval envelope" is set membership in the model and says nothing about key custody |
| I3 | Same standing, and additionally: content is "retrievable here or not". There are no hashes in the model, so *hash-verified* is not checked — only *present* |
| I4 | Holds against a rule the model states; the acknowledgement boundary as a **crash** boundary is not modelled at all (applying is atomic there). That is T133 `01KZC2DWNDW0QPDSEDJCC8A018`'s fault campaign, not this |
| I5 | The applied and refused sets are recomputed by the model on redelivery rather than assumed, and the head is required to be unchanged. Against the real fold it is `crates/mesh-state`, where every ChangeSet of a generated history is delivered up to four times in a shuffled stream |
| I6 | **The strongest row, and the one with a live finding behind it.** The head-and-delivery half of the model tracks `crates/mesh-state`, which is real code. Convergence is *conditional*: `models/mesh-divergence.cfg` produces two peers holding one identifier set, two heads and no refusal, in five steps; `models/mesh-divergence-conditional.cfg` shows the condition ADR-0015 names survives the same state space. **Nothing in the tree enforces the second half of that condition today** — §7, and ADR-0015 |
| I7 | The tree model tracks `crates/mesh-materializer`, which is real code — the at-most-one-parent index, the ancestor walk, the depth-then-identifier order. What it does **not** cover is the second half of the prose above: `crates/mesh-conflicts` is a twenty-one-line placeholder, so there is no conflict-rule *repair* to model. A concurrent cyclic move is checked to be **rejected identically on every peer**, which is what the shipped rule does, and is weaker than "resolved" |

**Bounded, not proved.** Every row is checked at the finite sizes in
[`models/README.md`](../models/README.md) and nowhere else. The properties are *stated* for
arbitrary actor counts and history depths and are *checked* at two or three of each. No TLAPS proof
is written and none is claimed; an unbounded claim needs an inductive invariant and a proof, and
neither exists.

---

## 7. What this model does not promise

Stating the boundary is part of the model, because an unstated boundary gets read as a guarantee.

- **No global total order** over private actor state, and no serialization point other than the
  canonical head.
- **No Byzantine tolerance** between mutually hostile peers. The trust graph assumes peers may be
  offline, buggy, slow or duplicating, and assumes actor keys are held by their owners
  ([charter §6](charter.md#6-poc-non-goals)).
- **No byte-level collaborative editing.** Concurrent overlapping edits preserve every version and
  surface for human attention; they do not merge character by character.
- **No automatic semantic merge of arbitrary binary content.** Preservation, not resolution.
- **No exact read attribution for unintegrated applications.** Inferred reads are recorded at
  reduced confidence and are never presented as exact.
- **No liveness guarantee from presence.** Presence is an observation, not a promise, and no
  correctness property depends on it.
- **No defence, yet, against a ChangeSet identifier that does not name the record delivered under
  it.** The head fold keys on the identifier and treats two records carrying one identifier as one
  ChangeSet; it holds no bytes, no digest and no signature, and says so. Verification therefore
  belongs to the boundary that has the bytes — and that boundary has not shipped. Until it does,
  two peers can hold one identifier set, reach two different heads, and refuse nothing: the model
  reaches it in five steps ([`models/mesh-divergence.cfg`](../models/mesh-divergence.cfg)), and
  [`models/README.md`](../models/README.md) under *The finding* states what it costs and who owns
  the choice of where the check lands. Convergence above is stated on the assumption that this
  boundary exists; it is the one assumption in this document that is currently unenforced.

---

## 8. How this model is verified

The model is a claim, and every claim in this repository points at the artifact that establishes
it ([charter P10](charter.md#p10--reliability-claims-must-be-measurable)).

| Property | Verified by | Owner |
|---|---|---|
| The seven invariants | Bounded model checking, each invariant with a violating mutation | T188 `01KZC2JC6E3D5TK6B2FKJMYANX` |
| Head advancement, delivery, publication, approval | The formal model — [`models/mesh.tla`](../models/mesh.tla), four passing configurations and thirteen that must fail, run by [`models/check.sh`](../models/check.sh). Needs a Java runtime, so it is deliberate rather than automatic: it is not a step of `npm test`, for the same reason the terminology lint is not (protocol §6) | T187 `01KZC2ETYD1F7M95Z4DH589SSG` |
| Convergence under adversarial schedules | The randomized operation simulator and the deterministic simulator | T129 `01KZC2GTT3XSNBREEGSZWGBMQQ`, T189 `01KZC2XGX4F03RETGAK401REZN` |
| Zero acknowledged-state loss | Process kill after every step of the durable commit sequence and of chunk promotion, and inside both — 47 killed children and 5 injected faults per run, 0 loss events, running unattended inside the workspace test run. Scope and limits: [§5.1](#51-the-durable-commit-sequence-observed-step-by-step)–[§5.4](#54-the-scope-of-the-zero-loss-claim); method and reproduction: [`tests/crash/README.md`](../tests/crash/README.md) | T133 `01KZC2DWNDW0QPDSEDJCC8A018`, T137 `01KZC2HEKXFBSR80V438GC2Q3G` |
| Conservative collection against retained roots | Reachability property tests over synthetic histories | T135 `01KZC2KZ29Y8Z6H9W93M40NPJ0` |
| Publication linearizability and bypass resistance | Concurrent-publication race tests and the publication attack harness | T169 `01KZC3DVAV6M2W92YHRY3G8CYM`, T197 `01KZC3JF712HDVFV62WN2Y5A78` |
| Staleness detection rate and false-positive rate | A controlled corpus with known ground truth, published together | T210 `01KZC392DA7VERW3AN4AWG0VE2` |

A property in §1 through §6 with no row here is an unverified claim. Adding a property means
adding its verification in the same change.
