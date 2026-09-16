------------------------------- MODULE mesh -------------------------------
(***************************************************************************)
(* The CWP formal model: actor heads, causal delivery, chunk availability,   *)
(* policy epochs, offline reconnection, and the one thing this product is    *)
(* actually about --- only an exact human-reviewed state advances the        *)
(* protected shared version.                                                 *)
(*                                                                          *)
(* HALF OF THIS MODELS WHAT SHIPPED AND HALF DOES NOT, and the difference    *)
(* decides how much each half is worth.  The head-and-delivery half tracks    *)
(* `crates/mesh-state`, which is real code, and every rule below is a rule    *)
(* that crate implements rather than one somebody wished it implemented:      *)
(*                                                                          *)
(*   * the head is a function of the applied causal set and of nothing else  *)
(*     --- `advance.rs`, module comment;                                     *)
(*   * the causal order is depth, then identifier --- this protocol's        *)
(*     spelling of lamport, then content hash.  Wall-clock time is not a     *)
(*     tiebreak and is not read (OG-6);                                      *)
(*   * a ChangeSet's causal parents are its author's own tips, never the     *)
(*     author's choice --- `HeadAdvancement::author`;                        *)
(*   * a receiver rederives both heads a ChangeSet claims and refuses a      *)
(*     mismatch rather than believing it (OG-9) --- `HeadAdvancement::apply`;*)
(*   * a ChangeSet whose causal parent has not arrived is buffered           *)
(*     indefinitely, never dropped (OG-3) --- `Reception::Buffered`.         *)
(*                                                                          *)
(* The publication-and-approval half --- Approve, Publish, the epoch and the *)
(* canonical head --- tracks the SPECIFICATION.  `crates/mesh-approval` is a *)
(* placeholder: twenty-one lines, one constant, no envelope and no           *)
(* compare-and-swap.  Those invariants say what an implementation would have *)
(* to satisfy.  They cannot say that anything satisfies it.                  *)
(*                                                                          *)
(* THE EIGHT BEHAVIOURS OF PLAN SECTION 13.1, AND WHERE EACH ONE IS.         *)
(* This table is the acceptance criterion "every one of the eight modelled   *)
(* behaviours is represented", made checkable by reading.                    *)
(*                                                                          *)
(*   actor-head advancement     Author                                       *)
(*                              AppliedIsExactlyWhatTheCausalRuleAdmits      *)
(*   causal operation delivery  Deliver                                      *)
(*                              NoDeliveredChangeSetIsDropped, Convergence   *)
(*   canonical compare-and-swap Publish        CompareAndSwapHeld            *)
(*   human approval             OfferForReview, Approve                      *)
(*                              OnlyAnExactHumanReviewedStateAdvances        *)
(*   conflict preservation      Collect                                      *)
(*                              AcknowledgedWorkIsNeverDiscarded,            *)
(*                              ConcurrentWorkIsPreserved                    *)
(*   chunk availability         FetchContent   CanonicalContentIsAvailable   *)
(*   policy epochs              RotateEpoch    CanonicalAdmittedInItsOwnEpoch*)
(*   offline peer reconnection  GoOffline, GoOnline                          *)
(*                              EventuallyEveryReconnectedPeerConverges      *)
(*   duplicate delivery         Redeliver                                    *)
(*                              DuplicateDeliveryIsIdempotent                *)
(*                                                                          *)
(* THE SEVEN CORE INVARIANTS OF docs/consistency.md SECTION 6 are I1 and I2  *)
(* in OnlyAnExactHumanReviewedStateAdvances, I3 in CanonicalContentIs-       *)
(* Available, I4 in AcknowledgedWorkIsNeverDiscarded, I5 in DuplicateDelivery*)
(* IsIdempotent, I6 in Convergence with its condition ConvergenceUnder-      *)
(* IdentifierBinding --- and I7 IN NEITHER OF THEM.  I7 needs a materialized *)
(* tree, which this module does not have and models/mesh_tree.tla does.      *)
(*                                                                          *)
(* WHAT THIS MODEL DELIBERATELY DOES NOT MODEL is in models/README.md under  *)
(* "Assumptions and boundary".  Read it before treating a passing run as     *)
(* evidence: a model's boundary is part of its result, and an unstated       *)
(* boundary gets read as a guarantee.                                        *)
(***************************************************************************)
EXTENDS Integers, FiniteSets, Sequences

CONSTANTS
    Humans,          \* human actors --- the only principals that may approve
    Agents,          \* agent actors --- may author and deliver, never approve
    MaxChangeSets,   \* how many ChangeSets one behaviour may author
    MaxEpoch,        \* how far the policy epoch may rotate
    MaxApprovals,    \* how many approval envelopes may be produced
    MaxPublications, \* how many admissions one behaviour may attempt
    MaxOutages,      \* how many times a peer may go offline

    (* The identifier-integrity assumption.  TRUE says every record a peer    *)
    (* receives really is the record its identifier is the digest of.         *)
    (* `mesh-state` cannot check this --- it keys on `ChangeSetId` and states *)
    (* the ceiling in its own crate documentation --- so the assumption is a  *)
    (* CONSTANT here, and models/mesh-divergence.cfg turns it off.            *)
    IdentifiersAreVerified,

    (* Mutation switches.  Each disables exactly one conjunct of exactly one  *)
    (* guard.  Each must produce a counterexample; models/check.sh runs them  *)
    (* and fails if any of them passes.  A guard whose removal changes        *)
    (* nothing was not a guard.                                               *)
    MUT_APPLY_WITHOUT_PARENTS,   \* apply a ChangeSet before its causal parents
    MUT_DROP_ORPHAN,             \* drop an early ChangeSet instead of buffering it
    MUT_ORDER_BY_CLOCK,          \* order by a clock reading instead of causal depth
    MUT_NO_COMPARE_AND_SWAP,     \* admit against a stale expected canonical head
    MUT_AGENT_MAY_APPROVE,       \* let an agent-scoped key produce an approval envelope
    MUT_SILENT_REBASE,           \* admit reviewed changes onto the current head
    MUT_PUBLISH_WITHOUT_CONTENT, \* admit a transition whose content is not retrievable
    MUT_COLLECT_UNPUBLISHED,     \* collect anything the canonical head does not reference
    MUT_REPLAY_APPROVAL,         \* admit one approval envelope twice
    MUT_EPOCH_IGNORED,           \* admit an envelope issued under a prior policy epoch
    MUT_ORDER_BY_ARRIVAL,        \* order by the order records arrived here
    MUT_REDELIVER_REORDERS       \* let a second arrival of a held identifier re-rank it

Actors     == Humans \cup Agents
ChangeSets == 1..MaxChangeSets
NoActor    == "no-actor"

(***************************************************************************)
(* A ChangeSet record.  `id = 0` is the absence of a record --- a peer that  *)
(* has never received this identifier.                                      *)
(*                                                                          *)
(* `base` and `result` are what the AUTHOR claims: the head it authored      *)
(* against and the head applying it produces.  They are carried rather than  *)
(* trusted; every receiver rederives both and refuses a mismatch.  Without   *)
(* them the model would be checking a protocol in which a peer can assert a  *)
(* state it never computed, which is exactly OG-9's failure.                 *)
(***************************************************************************)
(* `arr` is the LOCAL arrival index --- see the `arrival` variable.  It is   *)
(* zero in every honest record and in every configuration that switches on   *)
(* no arrival mutation, and no honest expression reads it.                   *)
NoRecord == [id |-> 0, author |-> NoActor, parents |-> {},
             base |-> << >>, result |-> << >>, epoch |-> 0, arr |-> 0]

(* A record with the local arrival index stripped, so that "is this the      *)
(* record its author sealed?" stays a question about the record and not      *)
(* about who received it when.                                              *)
Bare(r) == [r EXCEPT !.arr = 0]

(***************************************************************************)
(* State.                                                                   *)
(***************************************************************************)
VARIABLES
    rec,           \* [ChangeSets -> record]  the honest, authored records
    fake,          \* [Actors -> [ChangeSets -> record]]  records a peer holds
                   \* that disagree with the honest table.  All NoRecord unless
                   \* the identifier-integrity assumption is switched off.
    known,         \* [Actors -> SUBSET ChangeSets]  records this peer holds
    applied,       \* [Actors -> SUBSET ChangeSets]  the applied causal set
    refused,       \* [Actors -> SUBSET ChangeSets]  refused, reported, never applied
    delivered,     \* [Actors -> SUBSET ChangeSets]  everything ever handed to this peer
    collected,     \* [Actors -> SUBSET ChangeSets]  removed by an explicit retention step
    acked,         \* [Actors -> SUBSET ChangeSets]  everything ever past the
                   \* acknowledgement boundary here.  Monotone: it never shrinks,
                   \* which is what makes "acknowledged work is never discarded"
                   \* a statement a checker can refute.
    content,       \* [Actors -> SUBSET ChangeSets]  chunk content retrievable here
    online,        \* [Actors -> BOOLEAN]
    outages,       \* how many times a peer has gone offline (a budget, not a fact
                   \* about the protocol --- it exists to bound the search)
    offer,         \* [Actors -> [live: BOOLEAN, set: SUBSET ChangeSets]]
    epoch,         \* the current policy epoch
    envelopes,     \* the approval envelopes that exist
    envCount,      \* how many have been issued (their identifiers)
    spent,         \* Seq of envelope identifiers admitted, in admission order
    canon,         \* the canonical head and the envelope that produced it
    canonPrev,     \* the canonical head immediately before the last admission
    canonContent,  \* chunk content the canonical head is entitled to reference
    authored,      \* how many ChangeSets have been authored
    forgeUsed,     \* how many forged records have been introduced
    arrival        \* [Actors -> [ChangeSets -> Nat]]  the order records arrived
                   \* HERE.  ALWAYS ALL ZERO unless an arrival mutation is on,
                   \* so an honest configuration pays nothing for it: the
                   \* honest order is a function of the causal set and never of
                   \* the schedule, and a variable that recorded the schedule
                   \* would multiply the state space by every delivery order.

vars == << rec, fake, known, applied, refused, delivered, collected, acked,
           content, online, outages, offer, epoch, envelopes, envCount, spent,
           canon, canonPrev, canonContent, authored, forgeUsed, arrival >>

(* Whether any expression in this run reads the arrival index at all.  Off in *)
(* every honest configuration, which is the property MUT_ORDER_BY_ARRIVAL     *)
(* exists to refute.                                                          *)
ReadsArrival == MUT_ORDER_BY_ARRIVAL \/ MUT_REDELIVER_REORDERS

(***************************************************************************)
(* One peer's view of the records.  Identical to the honest table unless a   *)
(* forged record has reached this peer.                                      *)
(***************************************************************************)
(* `ViewAt` takes the arrival index as an argument rather than reading the   *)
(* variable, because `Redeliver` has to ask what this peer would derive under *)
(* an arrival index it has not committed to yet.                             *)
ViewAt(a, ar)     == [c \in ChangeSets |->
                        LET r == IF fake[a][c].id # 0
                                 THEN fake[a][c] ELSE rec[c]
                        IN  IF ReadsArrival THEN [r EXCEPT !.arr = ar[c]]
                                            ELSE r]
View(a)           == ViewAt(a, arrival[a])
ViewWith(a, c, r) == [View(a) EXCEPT ![c] = r]

(***************************************************************************)
(* Causality.                                                               *)
(*                                                                          *)
(* `Closure` is the ancestor closure inside a set: the members of F and      *)
(* every causal parent reachable from them that the set contains.  It        *)
(* terminates because F only grows and the set is finite.                    *)
(***************************************************************************)
RECURSIVE Closure(_, _, _)
Closure(v, S, F) ==
    LET next == F \cup UNION { v[c].parents \cap S : c \in F }
    IN  IF next = F THEN F ELSE Closure(v, S, next)

Ancestry(v, S, c) == Closure(v, S, v[c].parents \cap S)

Concurrent(v, S, c, d) ==
    /\ c # d
    /\ c \notin Ancestry(v, S, d)
    /\ d \notin Ancestry(v, S, c)

Tips(v, S) == { c \in S : ~\E d \in S : c \in v[d].parents }

SetMax(S) == CHOOSE x \in S : \A y \in S : y <= x

(***************************************************************************)
(* Causal depth: zero for a ChangeSet with no causal parent in the set, one  *)
(* more than its deepest parent otherwise.  This is the lamport half of      *)
(* lamport, then content hash.                                               *)
(*                                                                          *)
(* The fuel argument is not a modelling nicety.  A forged record can name    *)
(* causal parents the honest table does not, and a checker that met a cycle  *)
(* without fuel would not report a violation --- it would hang.  Fuel turns  *)
(* a hang into a wrong answer, and a wrong answer is what                    *)
(* CausalOrderIsRespected exists to catch.                                   *)
(***************************************************************************)
RECURSIVE DepthFuel(_, _, _, _)
DepthFuel(v, S, c, fuel) ==
    LET P == v[c].parents \cap S
    IN  IF P = {} \/ fuel <= 0
        THEN 0
        ELSE 1 + SetMax({ DepthFuel(v, S, p, fuel - 1) : p \in P })

Depth(v, S, c) == DepthFuel(v, S, c, MaxChangeSets)

(***************************************************************************)
(* A clock reading.  Deliberately WRONG: it runs backwards over the          *)
(* authoring order, which is what a badly set machine produces and what      *)
(* `crates/mesh-state/tests/heads.rs` feeds the real fold.  Nothing in the   *)
(* unmutated model reads it.  MUT_ORDER_BY_CLOCK is the only expression that *)
(* does, and it exists so that "order never consults wall-clock time" is a   *)
(* claim a checker can refute rather than a sentence in a document.          *)
(***************************************************************************)
Stamp(c) == MaxChangeSets + 1 - c

(* The order.  Depth, then identifier --- both pure functions of the causal   *)
(* set, which is the whole reason two peers holding one set agree without     *)
(* coordinating (ADR-0015, decision clause 1).  Two mutations replace that    *)
(* function with something the SCHEDULE decides: a clock reading, and the     *)
(* local arrival index.  Each is an implementation that a reader of the code  *)
(* would have to look twice at, and each is refuted here.                     *)
Rank(v, S, c) ==
    CASE MUT_ORDER_BY_CLOCK -> << Stamp(c), c >>
      [] ReadsArrival       -> << v[c].arr, c >>
      [] OTHER              -> << Depth(v, S, c), c >>

Precedes(x, y) == x[1] < y[1] \/ (x[1] = y[1] /\ x[2] < y[2])

RECURSIVE OrderIn(_, _, _)
OrderIn(v, Full, S) ==
    IF S = {}
    THEN << >>
    ELSE LET m == CHOOSE c \in S :
                    \A d \in S \ {c} : Precedes(Rank(v, Full, c), Rank(v, Full, d))
         IN  << m >> \o OrderIn(v, Full, S \ {m})

CausalOrder(v, S) == OrderIn(v, S, S)

(***************************************************************************)
(* THE HEAD.                                                                *)
(*                                                                          *)
(* The head IS the ordered applied set.  The protocol names a head by        *)
(* digesting that sequence under a domain label (`crates/mesh-state`,        *)
(* `digest.rs`), and this model treats the digest as injective --- two       *)
(* distinct ordered sequences never share a head.  That is an ASSUMPTION,    *)
(* not a result: nothing here says anything about BLAKE3, and a collision is *)
(* outside this model entirely.                                              *)
(***************************************************************************)
HeadOver(v, S) == CausalOrder(v, S)

HeadOf(a) == HeadOver(View(a), applied[a])

(***************************************************************************)
(* The receive rule, exactly as `HeadAdvancement::apply` states it.          *)
(***************************************************************************)
DerivedBase(v, S, P)      == HeadOver(v, Closure(v, S, P))
DerivedResult(v, S, c, P) == HeadOver(v, Closure(v, S, P) \cup {c})

Admissible(v, App, c) ==
    LET r == v[c]
        P == r.parents
    IN  /\ r.id # 0                                     \* a record actually arrived
        /\ c \notin P                                   \* SelfParent
        /\ ~\E p, q \in P :                             \* ParentImplied
              p # q /\ p \in Ancestry(v, App, q)
        /\ r.base   = DerivedBase(v, App, P)            \* OG-9, the base half
        /\ r.result = DerivedResult(v, App, c, P)       \* OG-9, the resulting half

(***************************************************************************)
(* Integration to a fixpoint --- `drain_buffered`.  Everything whose causal  *)
(* parents have arrived is applied or refused; the rest stays held.  A batch *)
(* is applied at once rather than one at a time, which is the same result    *)
(* because admissibility depends only on a ChangeSet's own ancestors and     *)
(* never on the rest of the applied set.                                     *)
(*                                                                          *)
(* Settle is the ACTION's rule and carries the mutation switch.              *)
(* HonestSettle is the same rule with no switch, and it is what the          *)
(* invariant rederives with --- an invariant that calls the function it is   *)
(* checking proves only that the function is deterministic.                  *)
(***************************************************************************)
ReadyIn(v, K, App, Ref) ==
    IF MUT_APPLY_WITHOUT_PARENTS
    THEN K \ (App \cup Ref)
    ELSE { c \in K \ (App \cup Ref) : v[c].parents \subseteq App }

RECURSIVE Settle(_, _, _, _)
Settle(v, K, App, Ref) ==
    LET ready == ReadyIn(v, K, App, Ref)
        ok    == { c \in ready : Admissible(v, App, c) }
    IN  IF ready = {}
        THEN << App, Ref >>
        ELSE Settle(v, K, App \cup ok, Ref \cup (ready \ ok))

RECURSIVE HonestSettle(_, _, _, _)
HonestSettle(v, K, App, Ref) ==
    LET ready == { c \in K \ (App \cup Ref) : v[c].parents \subseteq App }
        ok    == { c \in ready : Admissible(v, App, c) }
    IN  IF ready = {}
        THEN << App, Ref >>
        ELSE HonestSettle(v, K, App \cup ok, Ref \cup (ready \ ok))

Buffered(a) == known[a] \ (applied[a] \cup refused[a])

(***************************************************************************)
(* ACTIONS                                                                  *)
(***************************************************************************)

(* Actor-head advancement.  Authoring is local: it never waits for the       *)
(* network, which is charter P4 and is why `online` is not a guard here.     *)
(* The causal parents are the author's own tips and are not chosen: an       *)
(* author that could choose them could drop one, and a dropped causal parent *)
(* is precisely the fault that makes two histories impossible to relate.     *)
Author(a) ==
    /\ authored < MaxChangeSets
    /\ LET c     == authored + 1
           v     == View(a)
           P     == Tips(v, applied[a])
           ar    == IF ReadsArrival
                    THEN [arrival[a] EXCEPT ![c] = Cardinality(known[a]) + 1]
                    ELSE arrival[a]
           draft == [id |-> c, author |-> a, parents |-> P,
                     base |-> HeadOver(v, applied[a]), result |-> << >>,
                     epoch |-> epoch, arr |-> ar[c]]
           vc    == [v EXCEPT ![c] = draft]
           r     == [draft EXCEPT !.result = HeadOver(vc, applied[a] \cup {c})]
       IN  /\ rec'       = [rec       EXCEPT ![c] = Bare(r)]
           /\ arrival'   = [arrival   EXCEPT ![a] = ar]
           /\ known'     = [known     EXCEPT ![a] = @ \cup {c}]
           /\ applied'   = [applied   EXCEPT ![a] = @ \cup {c}]
           /\ delivered' = [delivered EXCEPT ![a] = @ \cup {c}]
           /\ acked'     = [acked     EXCEPT ![a] = @ \cup {c}]
           /\ content'   = [content   EXCEPT ![a] = @ \cup {c}]
           /\ authored'  = c
    /\ UNCHANGED << fake, refused, collected, online, outages, offer, epoch,
                    envelopes, envCount, spent, canon, canonPrev, canonContent,
                    forgeUsed >>

(* Causal delivery.  Metadata only --- content moves separately, which is    *)
(* what makes "metadata replicated" and "content available" two facts and    *)
(* not one.  Delivery may repeat and reorder freely: an identifier already   *)
(* known is not a step at all, so a redelivery cannot move a head.           *)
Deliver(s, d, c) ==
    /\ s # d
    /\ online[s] /\ online[d]
    /\ c \in known[s]
    /\ c \notin known[d]
    /\ LET r      == Bare(View(s)[c])
           ar     == IF ReadsArrival
                     THEN [arrival[d] EXCEPT ![c] = Cardinality(known[d]) + 1]
                     ELSE arrival[d]
           vd     == [ViewAt(d, ar) EXCEPT ![c] =
                        IF ReadsArrival THEN [r EXCEPT !.arr = ar[c]] ELSE r]
           orphan == ~(r.parents \subseteq applied[d])
       IN  /\ fake' = IF r = rec[c] THEN fake ELSE [fake EXCEPT ![d][c] = r]
           /\ arrival' = [arrival EXCEPT ![d] = ar]
           /\ IF MUT_DROP_ORPHAN /\ orphan
              THEN UNCHANGED << known, applied, refused, acked >>
              ELSE LET settled == Settle(vd, known[d] \cup {c},
                                         applied[d], refused[d])
                   IN  /\ known'   = [known   EXCEPT ![d] = @ \cup {c}]
                       /\ applied' = [applied EXCEPT ![d] = settled[1]]
                       /\ refused' = [refused EXCEPT ![d] = settled[2]]
                       /\ acked'   = [acked   EXCEPT ![d] = @ \cup settled[1]]
           /\ delivered' = [delivered EXCEPT ![d] = @ \cup {c}]
           (* Work that comes back is no longer collected work.  Without this *)
           (* the retention record would say a ChangeSet is gone while the    *)
           (* peer is holding it.                                             *)
           /\ collected' = [collected EXCEPT ![d] = @ \ {c}]
    /\ UNCHANGED << rec, content, online, outages, offer, epoch, envelopes,
                    envCount, spent, canon, canonPrev, canonContent, authored,
                    forgeUsed >>

(***************************************************************************)
(* I5 --- DUPLICATE DELIVERY.                                               *)
(*                                                                          *)
(* Handing a peer an identifier it already holds.  `Deliver` cannot express  *)
(* this: it is guarded on `c \notin known[d]`, so in models/mesh.tla as it   *)
(* shipped a duplicate was not a step and idempotence was not a claim the    *)
(* model made --- models/README.md said so under "Assumptions and boundary". *)
(*                                                                          *)
(* This action makes it a step, and the honest rule does the work rather     *)
(* than short-circuiting: it refolds the peer's held set and requires the    *)
(* answer to be the one it already had.  `DuplicateDeliveryIsIdempotent`     *)
(* then says what `docs/consistency.md` §6 says of I5 --- applying an        *)
(* already-applied ChangeSet leaves the state hash unchanged --- as a        *)
(* property over the transition rather than a remark about set insertion.    *)
(*                                                                          *)
(* MUT_REDELIVER_REORDERS is the implementation this refutes: a fold that    *)
(* lets a second arrival re-rank a record it already holds.  The applied set *)
(* is unchanged there too, which is exactly why it is worth checking --- the *)
(* head moves and no set the peer can inspect says anything moved.           *)
Redeliver(s, d, c) ==
    /\ s # d
    /\ online[s] /\ online[d]
    /\ c \in known[s]
    /\ c \in known[d]
    /\ LET ar == IF MUT_REDELIVER_REORDERS
                 THEN [arrival[d] EXCEPT ![c] = Cardinality(known[d])]
                 ELSE arrival[d]
           settled == Settle(ViewAt(d, ar), known[d], applied[d], refused[d])
       IN  /\ arrival'  = [arrival EXCEPT ![d] = ar]
           /\ applied'  = [applied EXCEPT ![d] = settled[1]]
           /\ refused'  = [refused EXCEPT ![d] = settled[2]]
           /\ acked'    = [acked   EXCEPT ![d] = @ \cup settled[1]]
    /\ UNCHANGED << rec, fake, known, delivered, collected, content, online,
                    outages, offer, epoch, envelopes, envCount, spent, canon,
                    canonPrev, canonContent, authored, forgeUsed >>

(* An identifier that is not the digest of the record delivered under it.    *)
(*                                                                          *)
(* This is not a hypothetical.  `crates/mesh-state` states the ceiling in    *)
(* its own crate documentation: it keys on `ChangeSetId`, treats two records *)
(* carrying one identifier as one ChangeSet, and says verification belongs   *)
(* to the boundary that has the bytes.  The forger picks causal parents the  *)
(* victim has already applied and fills in both claimed heads with what the  *)
(* victim will itself derive, so the record is admissible by construction    *)
(* and NOTHING IS REFUSED.  That is the whole point: the failure is silent.  *)
Forge(d, c, P) ==
    /\ ~IdentifiersAreVerified
    /\ forgeUsed = 0
    /\ c \in 1..authored
    /\ c \notin known[d]
    /\ P \subseteq applied[d]
    /\ LET v == View(d)
       IN  /\ ~\E p, q \in P : p # q /\ p \in Ancestry(v, applied[d], q)
           /\ P # rec[c].parents
           /\ LET draft == [id |-> c, author |-> rec[c].author, parents |-> P,
                            base |-> DerivedBase(v, applied[d], P),
                            result |-> << >>, epoch |-> rec[c].epoch,
                            arr |-> arrival[d][c]]
                  vc    == [v EXCEPT ![c] = draft]
                  r     == [draft EXCEPT !.result =
                              DerivedResult(vc, applied[d], c, P)]
                  (* The victim settles against the FINISHED record.  An     *)
                  (* earlier revision of this action settled against `draft`,*)
                  (* whose resulting head is still the placeholder, so every *)
                  (* forgery was refused and the divergence configuration    *)
                  (* passed --- a model that reported the guard working      *)
                  (* because the attack was malformed.  That is the failure  *)
                  (* mode a mutation campaign exists to catch, and it is     *)
                  (* recorded here rather than quietly fixed.                *)
                  vr    == [v EXCEPT ![c] = r]
                  settled == Settle(vr, known[d] \cup {c},
                                    applied[d], refused[d])
              IN  /\ fake'      = [fake      EXCEPT ![d][c] = r]
                  /\ known'     = [known     EXCEPT ![d] = @ \cup {c}]
                  /\ applied'   = [applied   EXCEPT ![d] = settled[1]]
                  /\ refused'   = [refused   EXCEPT ![d] = settled[2]]
                  /\ acked'     = [acked     EXCEPT ![d] = @ \cup settled[1]]
                  /\ delivered' = [delivered EXCEPT ![d] = @ \cup {c}]
                  /\ collected' = [collected EXCEPT ![d] = @ \ {c}]
    /\ forgeUsed' = 1
    /\ UNCHANGED << rec, content, online, outages, offer, epoch, envelopes,
                    envCount, spent, canon, canonPrev, canonContent, authored,
                    arrival >>

(* Chunk availability.  Holding the record is not holding the content.       *)
FetchContent(s, d, c) ==
    /\ s # d
    /\ online[s] /\ online[d]
    /\ c \in content[s]
    /\ c \in known[d]
    /\ c \notin content[d]
    /\ content' = [content EXCEPT ![d] = @ \cup {c}]
    /\ UNCHANGED << rec, fake, known, applied, refused, delivered, collected,
                    acked, online, outages, offer, epoch, envelopes, envCount,
                    spent, canon, canonPrev, canonContent, authored, forgeUsed, arrival >>

(* Offline peer reconnection.  Nothing else changes when a peer goes away:   *)
(* no head moves and nothing is discarded.  MaxOutages is a search budget,   *)
(* not a protocol rule --- the protocol tolerates unboundedly many outages   *)
(* and this model checks boundedly many.                                     *)
GoOffline(a) ==
    /\ online[a]
    /\ outages < MaxOutages
    /\ online'  = [online EXCEPT ![a] = FALSE]
    /\ outages' = outages + 1
    /\ UNCHANGED << rec, fake, known, applied, refused, delivered, collected,
                    acked, content, offer, epoch, envelopes, envCount, spent,
                    canon, canonPrev, canonContent, authored, forgeUsed, arrival >>

GoOnline(a) ==
    /\ ~online[a]
    /\ online' = [online EXCEPT ![a] = TRUE]
    /\ UNCHANGED << rec, fake, known, applied, refused, delivered, collected,
                    acked, content, outages, offer, epoch, envelopes, envCount,
                    spent, canon, canonPrev, canonContent, authored, forgeUsed, arrival >>

(* Retention.  A collector may remove content unreachable from every         *)
(* retained root and nothing else.  The retained roots are this peer's own   *)
(* acknowledged work and the canonical head.                                 *)
(*                                                                          *)
(* With the guard in place this action is NEVER ENABLED, and that is the     *)
(* property rather than an oversight: every applied ChangeSet is reachable   *)
(* from the actor head that applied it, so a conservative collector has      *)
(* nothing to take.  MUT_COLLECT_UNPUBLISHED replaces the retained-root test *)
(* with "the canonical head does not reference it" --- the resolution rule   *)
(* that throws the losing side of a conflict away --- and the counterexample *)
(* is what charter P5 costs when it is not enforced.                         *)
Collect(a, c) ==
    /\ c \in applied[a]
    /\ c \in Tips(View(a), applied[a])
    /\ c \notin canon.set
    /\ MUT_COLLECT_UNPUBLISHED \/ c \notin acked[a]
    /\ applied'   = [applied   EXCEPT ![a] = @ \ {c}]
    /\ known'     = [known     EXCEPT ![a] = @ \ {c}]
    /\ content'   = [content   EXCEPT ![a] = @ \ {c}]
    /\ collected' = [collected EXCEPT ![a] = @ \cup {c}]
    /\ UNCHANGED << rec, fake, refused, delivered, acked, online, outages,
                    offer, epoch, envelopes, envCount, spent, canon, canonPrev,
                    canonContent, authored, forgeUsed, arrival >>

(* Human approval, part one: the actor offers THIS head.  The head is copied *)
(* into the offer as a value, so later work advances the working head and    *)
(* cannot move the offer --- which is what makes "an approval refers to      *)
(* exact bytes" structural rather than a rule somebody has to remember.      *)
OfferForReview(a) ==
    /\ applied[a] # {}
    /\ envCount < MaxApprovals
    /\ ~(offer[a].live /\ offer[a].set = applied[a])
    /\ offer' = [offer EXCEPT ![a] = [live |-> TRUE, set |-> applied[a]]]
    /\ UNCHANGED << rec, fake, known, applied, refused, delivered, collected,
                    acked, content, online, outages, epoch, envelopes, envCount,
                    spent, canon, canonPrev, canonContent, authored, forgeUsed, arrival >>

(* Human approval, part two: a human signs an envelope binding the exact     *)
(* reviewed head, the exact expected canonical head and the policy epoch.    *)
(* An agent cannot reach this action.  TG-3 says the capability is           *)
(* unrepresentable rather than denied, and an unreachable action is this     *)
(* model's spelling of unrepresentable.                                      *)
Approve(h, a) ==
    /\ MUT_AGENT_MAY_APPROVE \/ h \in Humans
    /\ envCount < MaxApprovals
    /\ offer[a].live
    /\ offer[a].set # {}
    /\ offer[a].set \subseteq applied[h]
    /\ LET e == [id           |-> envCount + 1,
                 approver     |-> h,
                 subject      |-> a,
                 reviewedSet  |-> offer[a].set,
                 reviewedHead |-> HeadOver(View(h), offer[a].set),
                 expected     |-> canon.head,
                 epoch        |-> epoch]
       IN  /\ envelopes' = envelopes \cup {e}
           /\ envCount'  = envCount + 1
    /\ UNCHANGED << rec, fake, known, applied, refused, delivered, collected,
                    acked, content, online, outages, offer, epoch, spent, canon,
                    canonPrev, canonContent, authored, forgeUsed, arrival >>

(* The canonical compare-and-swap.  Five guards, each one a row of the trust *)
(* graph, each one switchable so that its removal can be watched to produce  *)
(* a counterexample.                                                         *)
Publish(e) ==
    /\ e \in envelopes
    /\ Len(spent) < MaxPublications
    /\ MUT_REPLAY_APPROVAL         \/ ~\E i \in 1..Len(spent) : spent[i] = e.id
    /\ MUT_NO_COMPARE_AND_SWAP     \/ e.expected = canon.head
    /\ MUT_EPOCH_IGNORED           \/ e.epoch = epoch
    /\ MUT_PUBLISH_WITHOUT_CONTENT \/ e.reviewedSet \subseteq content[e.approver]
    /\ LET admitted == IF MUT_SILENT_REBASE
                       THEN canon.set \cup e.reviewedSet
                       ELSE e.reviewedSet
       IN  /\ canon' = [set     |-> admitted,
                        head    |-> IF MUT_SILENT_REBASE
                                    THEN HeadOver(View(e.approver), admitted)
                                    ELSE e.reviewedHead,
                        by      |-> e.approver,
                        env     |-> e.id,
                        epochAt |-> epoch]
           /\ canonContent' = canonContent
                                \cup (admitted \cap content[e.approver])
    /\ canonPrev' = canon.head
    /\ spent'     = Append(spent, e.id)
    /\ UNCHANGED << rec, fake, known, applied, refused, delivered, collected,
                    acked, content, online, outages, offer, epoch, envelopes,
                    envCount, authored, forgeUsed, arrival >>

(* Policy epochs.  Rotation requires no peer to be online --- TG-7.          *)
RotateEpoch ==
    /\ epoch < MaxEpoch
    /\ epoch' = epoch + 1
    /\ UNCHANGED << rec, fake, known, applied, refused, delivered, collected,
                    acked, content, online, outages, offer, envelopes, envCount,
                    spent, canon, canonPrev, canonContent, authored, forgeUsed, arrival >>

Init ==
    /\ rec          = [c \in ChangeSets |-> NoRecord]
    /\ fake         = [a \in Actors |-> [c \in ChangeSets |-> NoRecord]]
    /\ known        = [a \in Actors |-> {}]
    /\ applied      = [a \in Actors |-> {}]
    /\ refused      = [a \in Actors |-> {}]
    /\ delivered    = [a \in Actors |-> {}]
    /\ collected    = [a \in Actors |-> {}]
    /\ acked        = [a \in Actors |-> {}]
    /\ content      = [a \in Actors |-> {}]
    /\ online       = [a \in Actors |-> TRUE]
    /\ outages      = 0
    /\ offer        = [a \in Actors |-> [live |-> FALSE, set |-> {}]]
    /\ epoch        = 1
    /\ envelopes    = {}
    /\ envCount     = 0
    /\ spent        = << >>
    /\ canon        = [set |-> {}, head |-> << >>, by |-> NoActor,
                       env |-> 0, epochAt |-> 0]
    /\ canonPrev    = << >>
    /\ canonContent = {}
    /\ authored     = 0
    /\ forgeUsed    = 0
    /\ arrival      = [a \in Actors |-> [c \in ChangeSets |-> 0]]

Next ==
    \/ \E a \in Actors : Author(a)
    \/ \E s, d \in Actors : \E c \in ChangeSets : Deliver(s, d, c)
    \/ \E s, d \in Actors : \E c \in ChangeSets : Redeliver(s, d, c)
    \/ \E d \in Actors : \E c \in ChangeSets : \E P \in SUBSET ChangeSets :
          Forge(d, c, P)
    \/ \E s, d \in Actors : \E c \in ChangeSets : FetchContent(s, d, c)
    \/ \E a \in Actors : GoOffline(a) \/ GoOnline(a)
    \/ \E a \in Actors : \E c \in ChangeSets : Collect(a, c)
    \/ \E a \in Actors : OfferForReview(a)
    \/ \E h, a \in Actors : Approve(h, a)
    \/ \E e \in envelopes : Publish(e)
    \/ RotateEpoch

Spec == Init /\ [][Next]_vars

(* Fairness, for the reconnection property only.  Checked in its own         *)
(* configuration, because liveness costs more than the safety run does.      *)
(*                                                                          *)
(* Delivery is STRONGLY fair and reconnection is only weakly fair, and the   *)
(* asymmetry is the whole content of the property.  Delivery between two     *)
(* peers is repeatedly disabled --- every time either of them goes away ---  *)
(* so weak fairness would not oblige it to happen at all, and the property   *)
(* would hold for the uninteresting reason that nothing had to be delivered. *)
Fairness ==
    /\ \A s, d \in Actors : \A c \in ChangeSets : SF_vars(Deliver(s, d, c))
    /\ \A s, d \in Actors : \A c \in ChangeSets : SF_vars(FetchContent(s, d, c))
    /\ \A a \in Actors : WF_vars(GoOnline(a))
    /\ \A a \in Actors : WF_vars(Author(a))

LiveSpec == Spec /\ Fairness

(***************************************************************************)
(* INVARIANTS                                                               *)
(*                                                                          *)
(* Each names the plan or protocol invariant it is, and each has a mutation  *)
(* in models/check.sh that breaks it.  An invariant no mutation can break is *)
(* not being checked, and a check that cannot fail is not evidence.          *)
(***************************************************************************)

TypeOK ==
    /\ epoch \in 1..MaxEpoch
    /\ envCount \in 0..MaxApprovals
    /\ authored \in 0..MaxChangeSets
    /\ Len(spent) <= MaxPublications
    /\ \A a \in Actors :
          /\ applied[a] \subseteq known[a]
          /\ refused[a] \subseteq known[a]
          /\ content[a] \subseteq ChangeSets
          /\ acked[a] \subseteq ChangeSets
          /\ \A c \in ChangeSets : arrival[a][c] \in 0..MaxChangeSets

(* OG-3 and OG-5.  The applied set is exactly what the causal rule admits    *)
(* from the records this peer holds --- whatever order they arrived in and   *)
(* however often.  Rederived here with the unmutated rule, from scratch.     *)
AppliedIsExactlyWhatTheCausalRuleAdmits ==
    \A a \in Actors :
        HonestSettle(View(a), known[a], {}, {}) = << applied[a], refused[a] >>

(* OG-3, the half about loss.  Everything ever delivered is applied, held as *)
(* a known-missing dependency, refused and reported, or removed by an        *)
(* explicit retention step.  There is no fifth outcome, and in particular    *)
(* "dropped because it was early" is not one.                                *)
NoDeliveredChangeSetIsDropped ==
    \A a \in Actors :
        delivered[a] \subseteq
            (applied[a] \cup Buffered(a) \cup refused[a] \cup collected[a])

(* The other half of OG-3, and the one a buffering bug hides behind: honest  *)
(* work is never refused.  A receiver that refuses a ChangeSet because its   *)
(* causal parent has not arrived yet has not "validated" anything --- it has *)
(* discarded work for being early, and it will report a clean run while      *)
(* doing it.  Refusal is for records that cannot be derived, and in a        *)
(* history where every record was honestly authored there are none.          *)
NoHonestRefusal == \A a \in Actors : refused[a] = {}

(* OG-6.  A causal parent is never ordered after a child.  This is what      *)
(* "the order is lamport, then content hash, never wall-clock" means when it *)
(* is stated as something a checker can refute.                              *)
CausalOrderIsRespected ==
    \A a \in Actors :
        LET v == View(a)
            o == CausalOrder(v, applied[a])
        IN  \A i, j \in 1..Len(o) : o[i] \in v[o[j]].parents => i < j

(* OG-5 and I6.  Two peers holding the same causal set hold the same head.   *)
Convergence ==
    \A a1, a2 \in Actors :
        applied[a1] = applied[a2] => HeadOf(a1) = HeadOf(a2)

(* ADR-0015's condition, written down.  The decision says actor heads         *)
(* converge without a lock *if* a ChangeSet's identifier binds its causal     *)
(* parent set and every receiver verifies the binding.  Here that condition   *)
(* is a predicate: every record every peer holds under an identifier names    *)
(* the causal parents the record sealed under that identifier really names.   *)
(*                                                                            *)
(* `mesh-types` enforces the first half --- `impl Absorb for ChangeSet`       *)
(* absorbs `causal_parents` --- and NOTHING ENFORCES THE SECOND HALF today:   *)
(* no crate recomputes the identifier from the bytes before head advancement  *)
(* sees it (ADR-0015, decision clause 2, and the obligation filed as          *)
(* 01KZE3NHTZQJ8DQYXBVJ24A0WT).                                               *)
IdentifierBindsCausalParents ==
    \A a \in Actors : \A c \in known[a] : View(a)[c].parents = rec[c].parents

(* I6, stated with its condition attached rather than assumed.               *)
(*                                                                          *)
(* This is the invariant to read next to `Convergence` and                   *)
(* `NoSilentDivergence`, and the three together are what makes ADR-0015's    *)
(* "conditionally yes" checkable rather than argued:                         *)
(*                                                                          *)
(*   * the condition is SUFFICIENT --- this invariant holds in every         *)
(*     configuration, including models/mesh-divergence.cfg, where records    *)
(*     may be forged and bare `Convergence` fails;                           *)
(*   * the condition is NECESSARY --- `Convergence` fails there, so no       *)
(*     weaker premise carries it;                                            *)
(*   * the condition is not vacuous --- it is true throughout every honest   *)
(*     configuration, where `Convergence` is checked and holds.              *)
(*                                                                          *)
(* At the checked sizes, and only there.  See models/README.md, "Bounded,    *)
(* not proved".                                                             *)
ConvergenceUnderIdentifierBinding ==
    IdentifierBindsCausalParents => Convergence

(* The same property with the failure MODE named: if two peers holding one   *)
(* identifier set disagree about the head, at least one of them refused      *)
(* something.  A violation of this is the sentence "two peers, the same      *)
(* identifier set, two different heads, and no refusal" --- a divergence     *)
(* nothing in the system reports and no surface can see.                     *)
NoSilentDivergence ==
    Convergence \/ \E a \in Actors : refused[a] # {}

(* I4 and charter P5.  Past the acknowledgement boundary, work stays.  The   *)
(* losing side of a conflict is work like any other.                         *)
AcknowledgedWorkIsNeverDiscarded ==
    \A a \in Actors : acked[a] \subseteq applied[a]

(* Conflict preservation, stated over the concurrent pairs themselves so     *)
(* that the property is visible rather than merely implied.  A peer holding  *)
(* both sides of a conflict keeps both sides.                                *)
ConcurrentWorkIsPreserved ==
    \A a \in Actors :
        \A c, d \in acked[a] :
            Concurrent(View(a), acked[a], c, d) => {c, d} \subseteq applied[a]

(* THE PRODUCT THESIS.  I1 and I2 in one sentence: the protected shared      *)
(* version is either genesis, or exactly the state a human reviewed, named   *)
(* by the envelope that human signed.  Not a superset of it, not a rebase of *)
(* it, and not an agent's.                                                   *)
OnlyAnExactHumanReviewedStateAdvances ==
    \/ canon.env = 0
    \/ \E e \in envelopes :
          /\ e.id           = canon.env
          /\ e.approver     \in Humans
          /\ e.reviewedHead = canon.head
          /\ e.reviewedSet  = canon.set

(* TG-9.  A transition is admitted only against the canonical head it named, *)
(* so the canonical sequence is a line and not a fork.                       *)
CompareAndSwapHeld ==
    \/ canon.env = 0
    \/ \E e \in envelopes : e.id = canon.env /\ e.expected = canonPrev

(* TG-10.  An approval envelope is single-use.                              *)
ApprovalIsSingleUse ==
    \A i, j \in 1..Len(spent) : spent[i] = spent[j] => i = j

(* I3.  No canonical state references content that is not retrievable.       *)
CanonicalContentIsAvailable == canon.set \subseteq canonContent

(* TG-7.  An envelope is admitted in the epoch it was issued under.          *)
CanonicalAdmittedInItsOwnEpoch ==
    \/ canon.env = 0
    \/ \E e \in envelopes : e.id = canon.env /\ e.epoch = canon.epochAt

(***************************************************************************)
(* I5.  DUPLICATE DELIVERY IS IDEMPOTENT, stated the way                     *)
(* `docs/consistency.md` §6 states it: applying an already-applied ChangeSet *)
(* leaves the state hash unchanged.                                         *)
(*                                                                          *)
(* It is an ACTION property and not an invariant, because idempotence is a   *)
(* claim about a TRANSITION and there is no state in which it is false ---   *)
(* an invariant over `applied` would have been the weaker claim this model   *)
(* previously declined to make.  Checked in its own configuration            *)
(* (models/mesh-idempotence.cfg), because a temporal property costs more     *)
(* than the safety run does.                                                *)
(*                                                                          *)
(* WHAT IT IS WORTH.  The applied and refused sets in the consequent are     *)
(* COMPUTED by `Redeliver` --- it refolds the held set --- so their being    *)
(* unchanged is a result.  The head being unchanged additionally needs the   *)
(* order to be a function of the set, which is what MUT_REDELIVER_REORDERS   *)
(* removes.  What the model still does not establish is I5 against the real  *)
(* fold; `crates/mesh-state/tests/delivery.rs` does that, delivering every   *)
(* ChangeSet of a generated history up to four times in a shuffled stream.   *)
(***************************************************************************)
Heads == [a \in Actors |-> HeadOf(a)]

DuplicateDeliveryIsIdempotent ==
    [][ \A s, d \in Actors : \A c \in ChangeSets :
          Redeliver(s, d, c) => /\ Heads'   = Heads
                                /\ applied' = applied
                                /\ refused' = refused ]_vars

(***************************************************************************)
(* Offline peer reconnection, as the liveness property it actually is: every *)
(* peer ends up holding every ChangeSet that was authored.                   *)
(*                                                                          *)
(* The "if no peer stays offline forever" half of that sentence is NOT in    *)
(* the formula --- it is `Fairness`, and it is only true of LiveSpec.  Read  *)
(* on Spec alone this property is false, and rightly so: a peer that never   *)
(* comes back never converges, and no protocol rule can change that.         *)
(*                                                                          *)
(* Anti-entropy is modelled as nothing more than Deliver being repeatedly    *)
(* enabled --- there is no summary exchange here, and models/README.md says  *)
(* so under "Assumptions and boundary".                                      *)
(***************************************************************************)
Converged ==
    /\ authored = MaxChangeSets
    /\ \A a \in Actors : applied[a] = ChangeSets

EventuallyEveryReconnectedPeerConverges == <>[]Converged

=============================================================================
