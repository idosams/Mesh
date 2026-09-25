# The CWP formal model

**Maturity: research evidence, not a product availability claim.** Read the archived results
below with [Project status](../docs/project-status.md) and the
[architecture guide](../docs/architecture.md).

Two TLA+ modules cover actor-head advancement, causal and duplicate delivery, chunk availability,
policy epochs, offline reconnection, canonical compare-and-swap, and human approval in
[`mesh.tla`](mesh.tla), plus the materialized directory tree in [`mesh_tree.tla`](mesh_tree.tla).
Their invariants are documented in [`docs/consistency.md`](../docs/consistency.md) §6.

**The one sentence the model exists to make refutable:**

> Only an exact human-reviewed state advances the protected shared version.

Everything else here is either a property that sentence rests on, or a boundary stated so that a
passing run is not read as a promise it did not make.

**Half of this model tracks shipped code and half of it does not, and the difference decides how
much each half is worth.**

The head-and-delivery half tracks `crates/mesh-state`, which is real: the head is a function of the
applied causal set and of nothing else; the causal order is depth, then identifier; a ChangeSet's
causal parents are its author's own tips; a receiver rederives both heads a ChangeSet claims and
refuses a mismatch; a ChangeSet whose causal parent has not arrived is held indefinitely and never
dropped. Every one of those is a rule that crate implements today, and a disagreement between this
model and that crate is a defect in one of them.

`mesh_tree.tla` tracks shipped code too — `crates/mesh-materializer`, whose `parent_of` index,
bounded ancestor walk, `Rejection::WouldCycle` and depth-then-identifier applied order are the four
rules it models.

The publication-and-approval half — `Approve`, `Publish`, the epoch and the canonical head — remains
**a specification abstraction, not a model-derived verification of the current implementation**.
The tree has moved since this model was written: `mesh-approval` now implements exact review
bundles and signed approval receipts, `mesh-policy` implements human-tier publication guards and
policy epochs, and `meshd` exercises an exact local review-and-approval path. A supported relay
compare-and-swap journey still does not exist, and this model has not been revised to establish an
equivalence relation with those Rust implementations. Its invariants therefore constrain the
design but do not prove the implementation correct.

The same evidence boundary applies to chunk availability and conflict repair for a different
reason. `mesh-sync-engine` now verifies received identifiers and persists inbound operations and
delivery acknowledgements, but it does not provide the live peer transport the model abstracts.
`mesh-conflicts` now implements the eleven-row conflict table, including deterministic cycle-free
tree resolution, but `mesh_tree.tla` still models only the materializer's reject-on-cycle rule.

Where the model departs from either, the departure is in
[Assumptions and boundary](#assumptions-and-boundary) — it is never silent.

---

## Running it

TLC is **not** a repository dependency and is **not** part of `npm test`. It needs a Java runtime,
and this repository's gates are zero-network, zero-dependency and under thirty seconds
(`CLAUDE.md`, *Testing*). This check is run deliberately, out of band:

```console
$ TLA2TOOLS_JAR=/path/to/tla2tools.jar models/check.sh
```

`check.sh` runs the seven configurations that must pass and the seventeen that must fail, each on a
named invariant or property. Its exit code is the verdict: **a mutation that passes fails the
script**, because a guard whose removal changes nothing was not a guard. Run it with
`--workers 1` and it additionally asserts each counterexample's declared depth; see
[What was checked](#what-was-checked-and-what-it-cost) for why that assertion is worth having only
at one worker. `--archive models/results` writes the gate artifact — one JSON row per
configuration, every counterexample, and the SHA-256 of every file that produced them.

The task's declared validation is `tlc models/mesh.tla -config models/mesh.cfg`, which is the first
row of that campaign and can be run on its own:

```console
$ java -cp tla2tools.jar tlc2.TLC -workers 4 -config models/mesh.cfg models/mesh.tla
```

`tla2tools.jar` is released at <https://github.com/tlaplus/tlaplus/releases>. Nothing in this
directory downloads it, and nothing in this directory is on the merge path.

| File | What it is |
|---|---|
| [`mesh.tla`](mesh.tla) | the head, delivery, publication and approval model — one module, every behaviour and every invariant except I7 |
| [`mesh_tree.tla`](mesh_tree.tla) | the materialized-tree model: I7 alone. Separate because the two state spaces multiply and neither property needs the other's actions |
| [`mesh.cfg`](mesh.cfg) | the base configuration; **must pass** |
| [`mesh-publication.cfg`](mesh-publication.cfg) | two envelopes and three admissions, so compare-and-swap has a race to arbitrate; **must pass** |
| [`mesh-history.cfg`](mesh-history.cfg) | five ChangeSets, delivery only — the deepest causal history that terminates here; **must pass** |
| [`mesh-liveness.cfg`](mesh-liveness.cfg) | the reconnection property, under fairness; **must pass** |
| [`mesh-idempotence.cfg`](mesh-idempotence.cfg) | I5, as the action property duplicate delivery actually is; **must pass** |
| [`mesh-divergence-conditional.cfg`](mesh-divergence-conditional.cfg) | ADR-0015's condition, checked in the state space where convergence itself fails; **must pass** — see [The condition](#the-condition) |
| [`mesh-tree.cfg`](mesh-tree.cfg) | I7: two peers, three directories, three moves; **must pass** |
| [`mesh-divergence.cfg`](mesh-divergence.cfg) | identifier integrity switched off; **must fail** — see [The finding](#the-finding) |
| `mesh-mut-*.cfg` | thirteen mutation runs over twelve removed guards; **each must fail** |
| `mesh-tree-mut-*.cfg` | two mutation runs over the two rules the tree model has; **each must fail** |
| [`mesh-liveness-mut-collect.cfg`](mesh-liveness-mut-collect.cfg) | the mutation that refutes the reconnection property; **must fail** |
| [`check.sh`](check.sh) | the campaign, the verdict, and `--archive` |
| [`results/`](results/) | the archived run: one JSON row per configuration, every counterexample, and the SHA-256 of every file that produced them |

---

## The eight behaviours

Plan §13.1 names eight behaviours the model must represent. Each is an action, and each has at
least one invariant that would be vacuous without it.

| Plan §13.1 behaviour | Action in `mesh.tla` | Invariant that rests on it |
|---|---|---|
| actor-head advancement | `Author` | `AppliedIsExactlyWhatTheCausalRuleAdmits` |
| causal operation delivery | `Deliver` | `NoDeliveredChangeSetIsDropped`, `Convergence` |
| canonical compare-and-swap | `Publish` | `CompareAndSwapHeld` |
| human approval | `OfferForReview`, `Approve` | `OnlyAnExactHumanReviewedStateAdvances` |
| conflict preservation | `Collect` | `AcknowledgedWorkIsNeverDiscarded`, `ConcurrentWorkIsPreserved` |
| chunk availability | `FetchContent` | `CanonicalContentIsAvailable` |
| policy epochs | `RotateEpoch` | `CanonicalAdmittedInItsOwnEpoch` |
| offline peer reconnection | `GoOffline`, `GoOnline` | `EventuallyEveryReconnectedPeerConverges` |

Two actions were added after those eight, by T188, because two of the seven core invariants could
not be stated without them. Neither is a plan §13.1 behaviour and neither is claimed as one:

| Why it exists | Action | Invariant that rests on it |
|---|---|---|
| I5 — a duplicate was not a step at all, so idempotence was not a claim the model made | `Redeliver` in `mesh.tla` | `DuplicateDeliveryIsIdempotent` |
| I7 — there was no materialized tree to be acyclic | `Author`, `Deliver` in `mesh_tree.tla` | `DirectoryAncestryIsAcyclic`, `ConcurrentMovesResolveIdenticallyOnEveryPeer` |

One of those actions is **never enabled in the base configuration**, and that is the property
rather than an oversight. `Collect` may remove only content unreachable from every retained root;
every applied ChangeSet is reachable from the actor head that applied it; so a conservative
collector has nothing to take, and TLC's coverage report shows the action at zero.
`mesh-mut-collect-unpublished.cfg` removes the retained-root test, replaces it with "the canonical
head does not reference it" — the resolution rule that throws the losing side of a conflict away —
and the counterexample is what [charter P5](../docs/charter.md) costs when it is not enforced.

---

## The invariants, and the mutation that breaks each

An invariant no mutation can break is not being checked, and a check that cannot fail is not
evidence ([`docs/consistency.md`](../docs/consistency.md) §6). Every row here has been watched to
fail.

| Invariant | What it says | Protocol source | Mutation that breaks it |
|---|---|---|---|
| `AppliedIsExactlyWhatTheCausalRuleAdmits` | the applied set is exactly what the causal rule admits from the records this peer holds, whatever order they arrived in and however often | OG-3, OG-5 | `MUT_APPLY_WITHOUT_PARENTS` |
| `NoDeliveredChangeSetIsDropped` | everything delivered is applied, held, refused, or explicitly collected — there is no fifth outcome | OG-3 | `MUT_DROP_ORPHAN` |
| `NoHonestRefusal` | honestly authored work is never refused — refusal is for records that cannot be derived, not for records that arrived early | OG-3 | `MUT_APPLY_WITHOUT_PARENTS` |
| `CausalOrderIsRespected` | a causal parent is never ordered after a child | OG-6 | `MUT_ORDER_BY_CLOCK` |
| `Convergence` | two peers holding the same causal set hold the same head | OG-5, I6 | `MUT_ORDER_BY_ARRIVAL` |
| `ConvergenceUnderIdentifierBinding` | ADR-0015's condition, written down: *if* every identifier a peer holds names the causal parents its record sealed, two peers holding one set hold one head | OG-5, I6, ADR-0015 | `MUT_ORDER_BY_ARRIVAL` — and it **survives** `mesh-divergence.cfg`, which is the result; see [The condition](#the-condition) |
| `NoSilentDivergence` | if two peers disagree about the head over one identifier set, at least one of them refused something | OG-5, I6 | identifier integrity switched off |
| `DuplicateDeliveryIsIdempotent` | handing a peer an identifier it already holds moves no head — an **action** property, because idempotence is a claim about a transition | I5, OG-4 | `MUT_REDELIVER_REORDERS` |
| `AcknowledgedWorkIsNeverDiscarded` | past the acknowledgement boundary, work stays | I4, charter P5 | `MUT_COLLECT_UNPUBLISHED` |
| `ConcurrentWorkIsPreserved` | a peer holding both sides of a conflict keeps both | OG-10, charter P5 | `MUT_COLLECT_UNPUBLISHED` |
| `OnlyAnExactHumanReviewedStateAdvances` | the canonical head is genesis, or exactly the state a human reviewed, named by the envelope that human signed | I1, I2, TG-3, charter P7 | `MUT_AGENT_MAY_APPROVE`, `MUT_SILENT_REBASE` |
| `CompareAndSwapHeld` | a transition is admitted only against the canonical head it named | TG-9 | `MUT_NO_COMPARE_AND_SWAP` |
| `ApprovalIsSingleUse` | an approval envelope is admitted at most once | TG-10 | `MUT_REPLAY_APPROVAL` |
| `CanonicalContentIsAvailable` | no canonical state references content that is not retrievable | I3, SG-9 | `MUT_PUBLISH_WITHOUT_CONTENT` |
| `CanonicalAdmittedInItsOwnEpoch` | an envelope is admitted in the epoch it was issued under | TG-7 | `MUT_EPOCH_IGNORED` |
| `EventuallyEveryReconnectedPeerConverges` | if no peer stays offline forever, every peer ends up holding every ChangeSet | consistency §5 | `MUT_COLLECT_UNPUBLISHED` |

And in `mesh_tree.tla`, which has its own two rules and its own two mutations:

| Invariant | What it says | Protocol source | Mutation that breaks it |
|---|---|---|---|
| `DirectoryAncestryIsAcyclic` | every materialized state is a tree: from every object the upward walk reaches the root | I7, SG-5 | `MUT_MOVE_WITHOUT_CYCLE_CHECK` |
| `ConcurrentMovesResolveIdenticallyOnEveryPeer` | two peers holding the same move set materialize the same tree **and refuse the same moves** | I7, SG-1, OG-10 | `MUT_MATERIALIZE_IN_ARRIVAL_ORDER` |
| `NoObjectIsLostToAMove` | a move changes where an object is and never which objects exist — a resolution that "resolved" a cyclic move by deleting one side would satisfy both rows above | SG-6, charter P6 | (no mutation; it is a type-level fact about the fold, and it is here because a future resolution rule could break it) |
| `ASoloMoveIsNeverRejected` | rejection is a concurrency outcome and never a solo one — a materializer that refused everything would keep the tree a tree and be useless, and the acyclicity invariant alone cannot tell the two apart | I7, SG-5 | (no mutation; it is the anti-vacuity half of the row above it) |

Three of those rows are weaker than they look, and saying so is cheaper than letting a reader find
out. `ConcurrentWorkIsPreserved` is **implied by** `AcknowledgedWorkIsNeverDiscarded` — it names
conflict preservation so the property is visible in the model rather than only in this document,
but it adds no checking power, and the mutation that breaks it is the same one.
`ConcurrentMovesResolveIdenticallyOnEveryPeer` is true by construction in the honest fold, because
materialization there reads the move set and nothing else; the mutation is the implementation bug
it exists to catch, and `crates/mesh-materializer`'s `order.rs` names exactly that bug in its own
module comment. `NoObjectIsLostToAMove` has no mutation at all and is marked as such.

Each mutation is a constant-guarded disabling of **exactly one conjunct** of exactly one guard, so
that the campaign is reproducible from committed files rather than from a sequence of edits
somebody made once. The switch and the guard sit on the same line of the module; reading the line
is reading the diff.

---

## What was checked, and what it cost

Measured on the machine that wrote this — Apple M-series, 8 TLC workers, TLC 2.19 on Temurin 21.
**These numbers are the time budget the acceptance criterion asks for; they are not a benchmark,
and no budget has been set against them.**

<!-- results:begin -->
| Configuration | Size | Verdict | Distinct states | Wall time |
|---|---|---|---|---|
| `mesh.cfg` | 1 human, 1 agent, 3 ChangeSets, 2 epochs, 1 envelope, 1 admission, 1 outage | pass | 421,880 | 36 s |
| `mesh-publication.cfg` | 2 ChangeSets, 2 envelopes, 3 admissions | pass | 113,328 | 5 s |
| `mesh-history.cfg` | 5 ChangeSets, delivery only | pass | 214,348 | 39 s |
| `mesh-liveness.cfg` | 2 ChangeSets, fairness, one temporal property | pass | 220 | 7 s |
| `mesh-idempotence.cfg` | 3 ChangeSets, delivery only, one action property | pass | 499 | 4 s |
| `mesh-divergence-conditional.cfg` | 2 ChangeSets, identifier integrity off, the condition | pass | 99 | 2 s |
| `mesh-tree.cfg` | 2 peers, 3 directories, 3 moves | pass | 11,209 | 3 s |
| `mesh-divergence.cfg` | 2 ChangeSets, identifier integrity off | **fails on `NoSilentDivergence`** | 91 before the violation | 1 s |
| `mesh-mut-*.cfg` (thirteen runs, twelve guards) | 2 or 3 ChangeSets | **each fails on its named invariant or action property** | 39 to 9,050 before the violation | 2 s or less each |
| `mesh-tree-mut-*.cfg` (two runs, two rules) | 2 peers, 3 directories, 2 moves | **each fails on its named invariant** | 274 and 331 | 1 s or less each |
| `mesh-liveness-mut-collect.cfg` | as `mesh-liveness.cfg` | **fails on the temporal property** | 887 | 5 s |
| whole campaign, `check.sh` | twenty-four configurations | clean, exit 0 | — | about 120 s at 8 workers, about 480 s at 1 |
<!-- results:end -->

The state counts for the **passing** rows are exact: an exhaustive run visits what it visits. The
counts for the failing rows are **not reproducible** and are given only for scale — TLC stops at
the first violation, and with eight workers how much of the level it had explored by then differs
between runs.

What is reproducible about a failing row is **the invariant it names**, which `check.sh` asserts on
every run, and **the minimal violating depth**, which it asserts only at `--workers 1`. The
counterexample length is not reproducible at higher worker counts, and that is measured rather than
supposed: TLC's workers are not level synchronized, so a parallel run can report a violation one
level deeper than the shallowest one that exists. **Four of the seventeen failing configurations did
exactly that in the eight-worker run archived under [`results/`](results/)** — `no-compare-and-swap`
at 8 against a depth of 7, `collect-unpublished` at 4 against 3, `replay-approval` at 9 against 8
and `epoch-ignored` at 7 against 6 — while all seventeen were at their declared depth at one worker.
Which four they are is itself not reproducible: an earlier eight-worker run overshot on
`publish-without-content` and `liveness-mut-collect` as well, and this one did not. Those depths are
the fifth column of `check.sh`'s expectation table, so a mutation that starts being caught for a
deeper, different reason is a failure rather than a silent change:

```text
mesh-divergence 5 · apply-without-parents 4 · refuses-honest-work 5 · drop-orphan 4
order-by-clock 3 · order-by-arrival 5 · redeliver-reorders 6 (action) · no-compare-and-swap 7
agent-may-approve 5 · silent-rebase 10 · publish-without-content 6 · collect-unpublished 3
replay-approval 8 · epoch-ignored 6 · liveness-mut-collect 6 (temporal)
tree-mut-no-cycle-check 4 · tree-mut-arrival-order 5
```

**Two of those numbers were declared wrong and the script caught it**, which is the only evidence
that the fourth column is load-bearing. `order-by-arrival` was declared at 4 and measures 5;
`redeliver-reorders` was declared at 5 and measures 6. Both were reasoned about before being run,
both runs reported `UNEXPECTED`, and the declaration was corrected to the measurement rather than
the other way round.

**An earlier revision of this section claimed the counterexample length was reproducible and that
`check.sh` asserted it. Neither was true** — the script asserted only the invariant name, and the
length varies with worker count. The measurement above is what replaced the claim, and the fourth
column is the enforcer the sentence had been asserting the existence of.

TLC's own action coverage on `mesh.cfg`, which is how the "every behaviour is represented" claim
above is checked rather than asserted — every action is reached except the two this document says
are unreachable there. TLC prints `distinct-states-first-found : states-generated` per action, and
**only the second number is reproducible**: which worker first reaches a given distinct state is
scheduling, so the left-hand figures below differ run to run while the right-hand ones do not. The
claim rests on the right-hand column being non-zero.

```text
<Author>: 14222:39728        <FetchContent>: 30923:241740   <OfferForReview>: 35542:160136
<Deliver>: 32941:107540      <GoOffline>: 49042:210940      <Approve>: 117941:144700
<Redeliver>: 0:1029960       <GoOnline>: 308:210940         <RotateEpoch>: 62470:70356
<Forge>: 0:0                 <Publish, as the Next disjunct at line 645>: 78490:89388
<Collect>: 0:0
```

**`Redeliver` at `0:1029960` is the third action with a zero on the left, and it is the only one
whose zero is the *property*.** `Forge` and `Collect` are zero on both sides — they are never
enabled in this configuration, for the reasons above. `Redeliver` is enabled a million times and
produces **no distinct state at all**, which is idempotence with a number attached: the action runs,
the fold recomputes, and TLC finds nothing new to record every single time.

Coverage is not part of `check.sh` — it needs `-coverage 1`, which costs about a third again on
`mesh.cfg`:

```console
$ java -cp tla2tools.jar tlc2.TLC -workers 8 -coverage 1 -config models/mesh.cfg models/mesh.tla
```

Coverage says the actions ran. It does not say the *interesting* states were reached, so two of
them were probed directly, by asserting the opposite and watching TLC break it:

- **A child arriving before its causal parent, buffered and then applied, is reachable in the base
  configuration.** Add `NothingIsEverBuffered == \A a \in Actors : Buffered(a) = {}` as an
  invariant and TLC reports it violated. Without that probe, every claim about buffering here
  would rest on an action that is enabled and a case that never happens.
- **Nothing is ever refused in an honest history.** The same probe on `refused` finds no
  violation, which is why `NoHonestRefusal` is an invariant of the passing configurations rather
  than a remark.
- **The cyclic case is reached in `mesh-tree.cfg`, and reached by concurrency.** Add
  `NoMoveIsEverRejected == \A p \in Peers : Materialize(p).rejected = {}` as an invariant and TLC
  reports it violated in four states: `p1` moves `a` under `b`, `p2` concurrently moves `b` under
  `a`, each valid against what its author can see, and the peer that receives both applies the
  first in the derived order and refuses the second. Without that probe, `DirectoryAncestryIsAcyclic`
  passing would be consistent with the interesting case never happening.

Three passing safety configurations rather than one, because the two expensive dimensions
multiply. The
causal-history dimension (how many ChangeSets, hence how many delivery orders) and the
envelope-and-admission dimension (how many approvals may race) were measured together at three
ChangeSets and two envelopes: **over fourteen million distinct states at depth 14 with the queue
still growing after two minutes, and it was abandoned rather than reported.** Splitting them is a
documented reduction, in the sense plan §13.1's owning task requires: *never claim a check that did
not complete.*

### Reproduced independently

The campaign above was re-run from the committed files by a second run, on a second machine, from a
JRE and a `tla2tools.jar` downloaded fresh rather than inherited: Darwin 23.5.0 arm64, 10 cores,
Temurin 21.0.12+8, TLC 2.19 of 08 August 2024 (rev 5a47802). Seventeen configurations, `check.sh`
exit 0, whole campaign about 100 s at eight workers and about 330 s at one.

**The four passing rows reproduced to the state:** 421,880 · 113,328 · 214,348 · 220, each equal to
the table above. The complete state graph of `mesh.cfg` is 1,275,469 states generated at depth 17.
Wall times did not reproduce and are not expected to — 37 s, 7 s, 31 s and 6 s at eight workers on
this machine against 17 s, 3 s, 30 s and 5 s on the machine that wrote the table. **Every one of
the thirteen failing rows failed on the invariant or property it is declared to fail on**, and each
counterexample was the declared depth at one worker.

That distinction is the whole value of the row: a distinct-state count reproduces because an
exhaustive search visits what it visits, and a wall time does not because it is a property of a
machine. Neither is a benchmark and no budget has been set against either.

### Reproduced again, after T188 changed the module

T188 added a variable (`arrival`), an action (`Redeliver`), three invariants and two mutation
switches to `mesh.tla`. **The four passing safety rows reproduced to the state across that
change** — 421,880 · 113,328 · 214,348 · 220 — which is the whole reason `arrival` stays all zeros
unless a mutation reads it and `Redeliver` is a no-op unless one does. A model edit that moved those
four numbers would have been an edit to what was being checked, not an addition to it.

What did move is the number of states *generated* on `mesh.cfg`: 1,275,469 before, 2,305,429 after,
at the same 421,880 distinct states and the same depth. `Redeliver` is enabled almost everywhere and
produces a self-loop each time, so TLC generates it and discards it. That is the cost of making
idempotence a step rather than an assumption, it is paid in generated states and not in distinct
ones, and it is stated here because a reader comparing the two runs would otherwise have to guess.

---

## Assumptions and boundary

A model's boundary is part of its result. Everything in this section is something a reader might
otherwise take a passing run to have established.

**Bounded, not proved.** TLC is a finite-state checker. The module is parameterised over arbitrary
finite `Humans`, `Agents` and `ChangeSets`, so the properties are *stated* for any number of actors
and any history depth; they are *checked* at the sizes in the table above and nowhere else. No
TLAPS proof is written and none is claimed. An unbounded-actor or unbounded-depth claim would need
an inductive invariant and a proof, and neither exists here.

**The head digest is assumed injective.** A head in this model *is* the ordered applied set; the
protocol names it by digesting that sequence under a domain label. Two distinct ordered sequences
are assumed never to share a head. Nothing here says anything about BLAKE3, and a digest collision
is outside the model entirely.

**Signatures are set membership.** `e.approver \in Humans` is the whole of "no agent-scoped key can
produce a valid approval envelope". The model shows that *if* the capability is unreachable then
the thesis holds, and shows what a trace looks like when it is reachable. It says nothing about
whether the key custody that makes it unreachable is correctly implemented — that is
`crates/mesh-crypto`, its conformance oracle, and the publication attack harness (T197).

**Idempotence is checked over the transition, and that is a narrower claim than it sounds.**
`Redeliver` hands a peer an identifier it already holds and refolds the held set rather than
short-circuiting, so the applied and refused sets in `DuplicateDeliveryIsIdempotent` are computed
and not assumed; the head being unchanged additionally needs the order to be a function of the set,
which is what `MUT_REDELIVER_REORDERS` removes. What none of that establishes is I5 against the
real fold — `crates/mesh-state/tests/delivery.rs` does that, delivering every ChangeSet in a
generated history up to four times in a shuffled stream. An earlier revision of this document said
idempotence was true by construction here and therefore not checked; that was accurate of the model
as it then stood, and `Redeliver` is what changed it.

**I7 is in a second module, and it covers the materializer rule rather than the complete conflict
resolver.**
`mesh_tree.tla` models what `crates/mesh-materializer` implements: at most one parent per object,
the bounded ancestor walk, `Rejection::WouldCycle` on a move into the moved object's own subtree,
and the depth-then-identifier applied order. `crates/mesh-conflicts::resolve_tree` now implements
deterministic cycle-free resolution and is covered by its own Rust preservation and determinism
campaigns, but that implementation is not represented in this TLA+ module. Consequently this
module checks the weaker materializer property — **the losing move is rejected, deterministically
and identically on every peer** — and is not evidence for the resolver. Names, versions, deletion,
links and the other seventeen operation verbs are outside this module entirely; every object in it
is a directory.

**A repeated causal parent is unrepresentable rather than refused.** The crate takes a parent
*list* and refuses one that names the same parent twice (`Refusal::ParentRepeated`); the model
carries a parent *set*, in which the fault cannot be written down. So that refusal is out of reach
here, and the model is not evidence for it. `crates/mesh-state/tests/heads.rs` is.

**One chunk per ChangeSet.** Chunk availability is modelled as "the content of this ChangeSet is
retrievable here or it is not". Content-defined chunking, deduplication, partial availability and
manifest structure are all outside it.

**Byzantine behaviour is modelled in exactly one place.** `Forge` is a peer that sends a record
under an identifier that is not the digest of it. No other lying is modelled: no forged signature,
no equivocating author, no colluding pair. `docs/consistency.md` §7 already states that Byzantine
tolerance between mutually hostile peers is a non-goal, and this model does not widen it.

**Wall-clock time appears only to be refuted.** The model carries a clock reading that runs
backwards over the authoring order, and no unmutated expression reads it. That is deliberate: it
makes "head advancement never consults wall-clock time" a property a checker can break rather than
a sentence in a document. The same pattern is why `arrival` exists: it records the order records
were received here, no honest expression reads it, and its two mutations make "the order is a
function of the causal set and of nothing observed" refutable rather than asserted.

**Unbounded actor counts and unbounded history depth are NOT reached, by either module.** ADR-0015's
decision clause 1 is stated for any causally closed, identifier-consistent set on any number of
peers; the exhaustive search behind it went to n = 7 with 5,040 permutations and 282,240
duplicate-insertion streams. This model does not go further in that direction and does not try to:
TLC is a finite-state checker, the largest configuration here is five ChangeSets, and there is no
inductive invariant and no TLAPS proof. **What it reaches that the n = 7 search could not is the
adversarial direction** — a peer holding a record its identifier does not name — and the result
there is the pair of runs in [The condition](#the-condition), not a bigger n.

**Not modelled at all**, and named so that their absence is not read as coverage: the durable
commit sequence and the acknowledgement boundary as a *crash* boundary (T133's fault campaign owns
it — here, applying is atomic); capability delegation chains and narrowing (TG-1, TG-2); review
bundles and validation results (TG-5); publication receipts (TG-6); the dependency graph in its
entirety — read observations, derivations, invalidation (DG-1 to DG-8); presence; anti-entropy as
an actual summary exchange rather than as delivery being repeatedly enabled.

---

## The finding

**Two peers, the same identifier set, two different heads, and no refusal — reachable in five
steps against the rules `crates/mesh-state` actually implements.**

`mesh-divergence.cfg` removes no guard. It removes one *assumption*: that a record delivered under
an identifier really is the record that identifier is the digest of. `crates/mesh-state` states
this ceiling in its own crate documentation —

> a caller that hands this crate an identifier it did not verify gets a head derived from records
> it did not verify

— and this configuration is that sentence with a trace attached. TLC finds it in five steps:

| Step | What happens |
|---|---|
| 1 | `h1` authors ChangeSet 1 with no causal parent |
| 2 | `g1` authors ChangeSet 2 with no causal parent — the two are concurrent |
| 3 | 2 is delivered to `h1`; `h1` has applied `{1, 2}`, head `<<1, 2>>` |
| 4 | a record reaches `g1` under identifier 1, naming `{2}` as its causal parent, claiming base head `<<2>>` and resulting head `<<2, 1>>` |
| 5 | `g1` rederives both claimed heads from its own causal knowledge, agrees with both, and applies it |

`g1` now holds applied `{1, 2}` with head `<<2, 1>>`. `h1` holds applied `{1, 2}` with head
`<<1, 2>>`. Neither refused anything; neither has any way to notice. Every check
`HeadAdvancement::apply` performs passed, because every check it performs is *relative to the
record it was given*.

The forger does not need to reorder a chain. It only needs to turn a **concurrent pair into a chain
in one peer's view**, which changes one causal depth, which changes the order, which changes the
head. That is why two ChangeSets are enough — an earlier revision of the configuration asserted
three were needed, on reasoning that held only for honest histories.

**What this is and is not.** It is not a defect in `crates/mesh-state`: that crate has no bytes, no
signature and no digest, states as much, and declares no dependency that could give it any of them
(ADR-0008). It is a statement about **where the boundary has to be**: whatever hands a
`DeliveredChangeSet` to the fold must have verified that the identifier is the digest of the
record, and it must do that before the fold sees it, because after the fold sees it every
downstream check agrees with the forgery. The sequencing rule is what makes this worth writing
down: `mesh-state` shipped first and the verifying boundary has not shipped yet, so today the
assumption is load-bearing and unenforced.

The owner of the boundary is the party that owns protocol and correctness (plan §14.1). This model
does not choose the crate it lands in, and **must not** — that is a decision, not a modelling
result. Two candidate homes exist in the register (`mesh-sync-protocol` for the wire boundary,
`mesh-types` for identity derivation), which is precisely why the choice needs a decision doc
rather than a lane's preference.

**That decision has since been taken, and it names this exact condition.**
[ADR-0015](../docs/design-decisions.md#adr-0015)
answers plan §11's research item R1 with *conditionally yes*, fixes the obligation on the crate that
decodes a carried record into a delivered one, and states in its own words that the second half of
the condition — a receiver recomputing the identifier from the bytes before head advancement sees
it — **is enforced by nothing today**. The implementation is filed as
`01KZE3NHTZQJ8DQYXBVJ24A0WT` and is not this task's.

---

## The condition

**ADR-0015's condition is not argued here, it is checked — and the result is the *pair* of runs, not
either one alone.**

Two configurations differ in nothing but which invariant they name. Both switch identifier integrity
off, so a forged record can reach a peer; neither removes any rule:

| Configuration | Invariant | Verdict |
|---|---|---|
| `mesh-divergence.cfg` | `NoSilentDivergence` (and so `Convergence`) | **fails**, in five steps |
| `mesh-divergence-conditional.cfg` | `ConvergenceUnderIdentifierBinding` | **holds**, over the whole state space |

`ConvergenceUnderIdentifierBinding` is the implication `IdentifierBindsCausalParents => Convergence`,
where the antecedent says every record a peer holds under an identifier names the causal parents the
sealed record under that identifier really names. Read together the two rows say:

- the condition is **sufficient** at these sizes — it survives a state space in which convergence
  itself does not;
- the condition is **necessary** — drop it and there is a five-step trace;
- the condition is **not vacuous** — it is true throughout every honest configuration, which is
  where `Convergence` is checked and holds. An implication with an antecedent nothing satisfies
  would hold for no reason at all, and that is the failure mode this bullet exists to rule out.

**What it is not.** It is not a proof. TLC checked two peers, two ChangeSets and one forgery; no
inductive invariant is written and no TLAPS proof exists, so nothing here reaches the unbounded
actor counts and unbounded history depths ADR-0015's decision clause 1 is *stated* over. Nor does it
say anything about the digest: the head is the ordered applied set here, and two distinct ordered
sequences are *assumed* never to share a head.

---

## What the mutations do and do not reach

Two negative results, recorded because a campaign that only reports its successes is not a
campaign.

**`MUT_DROP_ORPHAN` does not refute the reconnection property.** A dropped ChangeSet is
redelivered, and under strongly fair delivery it eventually arrives after its causal parent and
applies. Dropping early work is a **safety** failure — the sender was told it was delivered and it
is gone — caught by `NoDeliveredChangeSetIsDropped`, not by the liveness property.

**`Convergence` had no mutation of its own, and now it has one.** The earlier revision of this
section recorded that no mutation refuted it: under the identifier-integrity assumption the head is
a pure function of the applied set in *both* the action and the invariant, so nothing the campaign
removed could separate them. That was a statement about the *rule*, and a rule is exactly what a
mutation is supposed to remove. `MUT_ORDER_BY_ARRIVAL` removes it — the order stops being derived
from the causal set and becomes the order records arrived here — and two peers holding one
identifier set, having received it in different orders, hold two heads in four steps.

Convergence under adversarial delivery schedules with the *real fold* is still not this model's
evidence; that is the simulator campaigns, T129 and T189, per `docs/consistency.md` §8. What this
model now adds beyond them is the pair of runs in [The condition](#the-condition).

---

## Extending it

`mesh.tla` is one file on purpose: its properties cross its behaviours, and a split would make
`OnlyAnExactHumanReviewedStateAdvances` reference three modules. `mesh_tree.tla` is a *second*
module for the opposite reason — nothing in I7 touches an approval, an epoch or a chunk, and the two
state spaces multiply rather than add. Three rules for anyone adding to either:

1. **Every new invariant arrives with the mutation that breaks it**, wired into `check.sh`'s
   expectation table in the same change, with the counterexample depth measured at `--workers 1`
   rather than guessed. An invariant with no mutation is not being checked, and the one row here
   that has none says so in its own cell.
2. **Every new assumption arrives in [Assumptions and boundary](#assumptions-and-boundary)** in the
   same change. The boundary is the part of the result a later reader cannot reconstruct.
3. **Every new variable that only a mutation reads stays constant when no mutation is on.**
   `arrival` is all zeros in every honest configuration, and the four passing safety runs reproduce
   to the state because of it. A variable that recorded the schedule unconditionally would multiply
   the state space by every delivery order and buy nothing.

**Where the seven core invariants of `docs/consistency.md` §6 now stand: seven for seven, at the
strength each row of that section states and not above it.** T187 left I5 unchecked and I7
unexpressed; T188 `01KZC2JC6E3D5TK6B2FKJMYANX` added `Redeliver` and
`DuplicateDeliveryIsIdempotent` for the first and `mesh_tree.tla` for the second, gave `Convergence`
a mutation of its own, and made ADR-0015's condition a checked implication. Four of the seven remain
evidence about a *specification abstraction* rather than verification of current code because no
model-to-implementation equivalence has been established; `docs/consistency.md` §6.2 is intended
to be the row-by-row boundary, but its implementation-status prose now needs the follow-up named in
the current [project status](../docs/project-status.md).
