----------------------------- MODULE mesh_tree -----------------------------
(***************************************************************************)
(* I7 --- DIRECTORY ANCESTRY REMAINS ACYCLIC.                               *)
(*                                                                          *)
(* `docs/consistency.md` §6 states I7 as two claims, and they are different  *)
(* claims with different evidence:                                          *)
(*                                                                          *)
(*   (a) every materialized state is a tree; and                            *)
(*   (b) concurrent cyclic directory moves resolve deterministically and     *)
(*       identically on every peer.                                         *)
(*                                                                          *)
(* Neither can be stated in models/mesh.tla: a ChangeSet there is an         *)
(* identifier, an author, a causal parent set and two claimed heads, with no *)
(* materialized tree and no operation vocabulary.  models/README.md said     *)
(* closing I7 needed a tree in that model or a second model.  This is the    *)
(* second model, and it is separate because the two state spaces multiply    *)
(* and neither property needs the other's actions.                          *)
(*                                                                          *)
(* WHAT THIS MODELS IS WHAT SHIPPED.  Every rule below is a rule             *)
(* `crates/mesh-materializer` implements today, cited to the file:           *)
(*                                                                          *)
(*   * an object is bound under at most one name in at most one directory,   *)
(*     held in the `parent_of` index --- `state.rs`, module comment.  Hard   *)
(*     links are `Rejection::AlreadyLinked`, so the parent relation is a     *)
(*     partial FUNCTION here and not a relation;                             *)
(*   * a move whose destination is the moved object or one of its            *)
(*     descendants is `Rejection::WouldCycle` --- `apply.rs::move_entry`,    *)
(*     via `WorkspaceState::is_self_or_ancestor`;                            *)
(*   * that ancestor walk is bounded by the object count and returns TRUE    *)
(*     when the bound is exceeded, so a state that somehow held a cycle      *)
(*     terminates rather than hangs --- `state.rs::is_self_or_ancestor`;      *)
(*   * the applied order is causal depth, then ChangeSet identifier, and it  *)
(*     is a function of the SET and never of the arrival order ---           *)
(*     `order.rs`, module comment, SG-1;                                     *)
(*   * materialization is total: a rejected operation yields a rejection and *)
(*     the fold continues, it does not abort the state --- `lib.rs`,         *)
(*     "Materialization is total".                                          *)
(*                                                                          *)
(* WHAT THIS DELIBERATELY DOES NOT MODEL is in models/README.md under        *)
(* "Assumptions and boundary".  The short form: every object here is a       *)
(* directory, there are no names, and `MoveObject` is the only verb.  Name   *)
(* collision, deletion, versions, manifests and the other seventeen verbs    *)
(* are out, and so is `mesh-conflicts`, which is a twenty-one-line           *)
(* placeholder --- the conflict-rule table of protocol §7.2 does not exist   *)
(* to be modelled.  Half (b) is therefore checked as the property the        *)
(* CURRENT rule has, which is that a concurrent cyclic move is REJECTED on   *)
(* every peer identically, not that it is repaired by a conflict rule.       *)
(***************************************************************************)
EXTENDS Integers, FiniteSets, Sequences

CONSTANTS
    Peers,       \* the peers that author and receive moves
    Objects,     \* directory objects other than the root
    Root,        \* the root object.  It is never moved: `Rejection::RootObject`
    MaxOps,      \* how many moves one behaviour may author

    (* Mutation switches.  Each disables exactly one conjunct of exactly one *)
    (* rule.  Each must produce a counterexample; models/check.sh runs them  *)
    (* and fails if one passes.                                             *)
    MUT_MOVE_WITHOUT_CYCLE_CHECK,      \* drop `is_self_or_ancestor` on a move
    MUT_MATERIALIZE_IN_ARRIVAL_ORDER   \* fold in the order received, not the
                                       \* order derived from the set

Nodes    == Objects \cup {Root}
NoParent == "detached"
Ops      == 1..MaxOps

(***************************************************************************)
(* State.                                                                   *)
(***************************************************************************)
VARIABLES
    op,        \* [Ops -> [obj, dest, depth]]  the authored moves
    authored,  \* how many have been authored
    held,      \* [Peers -> SUBSET Ops]  the moves this peer holds
    arrivals   \* [Peers -> Seq(Ops)]  the order they arrived in.  ALWAYS
               \* EMPTY unless MUT_MATERIALIZE_IN_ARRIVAL_ORDER is on, so
               \* that the honest configurations pay nothing for it --- the
               \* honest fold does not read it and must not.

vars == << op, authored, held, arrivals >>

NoOp == [obj |-> Root, dest |-> Root, depth |-> 0]

(***************************************************************************)
(* The initial tree: every object directly under the root.  A deeper start   *)
(* would add states without adding cases --- every ancestry this model needs *)
(* is built by the moves themselves.                                        *)
(***************************************************************************)
InitialParent == [n \in Nodes |-> IF n = Root THEN NoParent ELSE Root]

(***************************************************************************)
(* `WorkspaceState::is_self_or_ancestor`, including its bound.  The crate    *)
(* counts steps and returns TRUE once the count exceeds the object count;    *)
(* that is not a modelling convenience, it is the crate's own statement that *)
(* totality is a property of the function and not of its input.             *)
(***************************************************************************)
RECURSIVE UpWalk(_, _, _, _)
UpWalk(par, cand, at, fuel) ==
    IF at = cand           THEN TRUE
    ELSE IF fuel <= 0      THEN TRUE
    ELSE IF par[at] = NoParent THEN FALSE
    ELSE UpWalk(par, cand, par[at], fuel - 1)

IsSelfOrAncestor(par, cand, object) ==
    UpWalk(par, cand, object, Cardinality(Nodes))

(***************************************************************************)
(* One move against one state.  Two rejections are modelled --- the root is  *)
(* not movable, and a move into the moved object's own subtree would cycle.  *)
(* Every other rejection in `apply.rs` is about names, versions and deleted  *)
(* objects, which this model does not carry.                                *)
(***************************************************************************)
WouldCycle(par, o) ==
    /\ ~MUT_MOVE_WITHOUT_CYCLE_CHECK
    /\ IsSelfOrAncestor(par, o.obj, o.dest)

Rejects(par, o) ==
    \/ o.obj = Root
    \/ o.obj = o.dest
    \/ par[o.obj] = o.dest        \* `Effect::AlreadyInEffect`: no state change
    \/ WouldCycle(par, o)

ApplyMove(par, o) ==
    IF Rejects(par, o) THEN par ELSE [par EXCEPT ![o.obj] = o.dest]

(***************************************************************************)
(* The applied order --- `order.rs`.  Causal depth, then identifier.  Both   *)
(* are functions of the set, which is the whole reason SG-1 is reachable:    *)
(* an order that is observed rather than derived makes two peers that        *)
(* received the same moves in different orders hold different states, and    *)
(* neither would know.                                                      *)
(***************************************************************************)
Rank(c) == << op[c].depth, c >>

Precedes(x, y) == x[1] < y[1] \/ (x[1] = y[1] /\ x[2] < y[2])

RECURSIVE SortIn(_)
SortIn(S) ==
    IF S = {}
    THEN << >>
    ELSE LET m == CHOOSE c \in S : \A d \in S \ {c} : Precedes(Rank(c), Rank(d))
         IN  << m >> \o SortIn(S \ {m})

(***************************************************************************)
(* The fold.  It returns the tree AND the set of moves that were rejected,   *)
(* because "resolve identically on every peer" is a claim about both: two    *)
(* peers that agree on the tree while disagreeing about which move was       *)
(* refused have not resolved the conflict identically, they have agreed by   *)
(* accident.                                                                *)
(***************************************************************************)
RECURSIVE FoldFrom(_, _, _)
FoldFrom(par, rej, seq) ==
    IF seq = << >>
    THEN [par |-> par, rejected |-> rej]
    ELSE LET c == Head(seq)
             o == op[c]
         IN  FoldFrom(ApplyMove(par, o),
                      IF o.obj # Root /\ WouldCycle(par, o)
                      THEN rej \cup {c} ELSE rej,
                      Tail(seq))

AppliedSequence(p) ==
    IF MUT_MATERIALIZE_IN_ARRIVAL_ORDER THEN arrivals[p] ELSE SortIn(held[p])

Materialize(p) == FoldFrom(InitialParent, {}, AppliedSequence(p))

TreeOf(p) == Materialize(p).par

(***************************************************************************)
(* ACTIONS                                                                  *)
(***************************************************************************)

(* Authoring is local.  The author applies the move to its OWN state first,  *)
(* so a move that would cycle against what its author can see is never       *)
(* sealed --- which is what makes every cycle this model finds a CONCURRENCY *)
(* failure rather than a peer authoring nonsense.                           *)
(*                                                                          *)
(* The causal depth is fixed at authoring time from the set authored         *)
(* against: zero with nothing behind it, one more than the deepest move it   *)
(* follows otherwise.  It is a field rather than a computation because a     *)
(* move's causal parents never change after it is sealed.                    *)
Author(p, o, d) ==
    /\ authored < MaxOps
    /\ o \in Objects
    /\ d \in Nodes
    /\ o # d
    /\ LET par == TreeOf(p)
           c   == authored + 1
           dep == IF held[p] = {} THEN 0
                  ELSE 1 + (CHOOSE m \in { op[q].depth : q \in held[p] } :
                              \A n \in { op[q].depth : q \in held[p] } : n <= m)
           new == [obj |-> o, dest |-> d, depth |-> dep]
       IN  /\ par[o] # d                       \* not already in effect
           /\ ~IsSelfOrAncestor(par, o, d)      \* locally valid, always
           /\ op'       = [op EXCEPT ![c] = new]
           /\ authored' = c
           /\ held'     = [held EXCEPT ![p] = @ \cup {c}]
           /\ arrivals' = IF MUT_MATERIALIZE_IN_ARRIVAL_ORDER
                          THEN [arrivals EXCEPT ![p] = Append(@, c)]
                          ELSE arrivals

(* Delivery.  Metadata only, may reorder freely, and an identifier already   *)
(* held is not a step --- so this model cannot say anything about duplicate  *)
(* delivery and does not try to.  I5 is models/mesh.tla's.                   *)
Deliver(s, d, c) ==
    /\ s # d
    /\ c \in held[s]
    /\ c \notin held[d]
    /\ held'     = [held EXCEPT ![d] = @ \cup {c}]
    /\ arrivals' = IF MUT_MATERIALIZE_IN_ARRIVAL_ORDER
                   THEN [arrivals EXCEPT ![d] = Append(@, c)]
                   ELSE arrivals
    /\ UNCHANGED << op, authored >>

Init ==
    /\ op       = [c \in Ops |-> NoOp]
    /\ authored = 0
    /\ held     = [p \in Peers |-> {}]
    /\ arrivals = [p \in Peers |-> << >>]

Next ==
    \/ \E p \in Peers : \E o \in Objects : \E d \in Nodes : Author(p, o, d)
    \/ \E s, d \in Peers : \E c \in Ops : Deliver(s, d, c)

Spec == Init /\ [][Next]_vars

(***************************************************************************)
(* INVARIANTS                                                               *)
(***************************************************************************)

TypeOK ==
    /\ authored \in 0..MaxOps
    /\ \A p \in Peers : held[p] \subseteq 1..authored
    /\ \A c \in 1..authored : op[c].obj \in Objects /\ op[c].dest \in Nodes

(***************************************************************************)
(* I7(a) and SG-5.  Every materialized state is a tree: from every object    *)
(* the upward walk reaches the root, so no object is its own ancestor.       *)
(*                                                                          *)
(* This is NOT true by construction.  Each move carries a check made against *)
(* a state that already reflects every move ordered before it, and the moves *)
(* were authored concurrently against states that do not contain each other. *)
(* That a per-step local check composes into a global tree over an           *)
(* arbitrary interleaving of concurrent moves is the property, and           *)
(* MUT_MOVE_WITHOUT_CYCLE_CHECK is a two-move counterexample to it.          *)
(***************************************************************************)
RECURSIVE ReachesRoot(_, _, _)
ReachesRoot(par, at, fuel) ==
    IF at = Root       THEN TRUE
    ELSE IF fuel <= 0  THEN FALSE
    ELSE IF par[at] = NoParent THEN TRUE   \* detached, not cyclic
    ELSE ReachesRoot(par, par[at], fuel - 1)

Acyclic(par) == \A n \in Nodes : ReachesRoot(par, n, Cardinality(Nodes))

DirectoryAncestryIsAcyclic == \A p \in Peers : Acyclic(TreeOf(p))

(***************************************************************************)
(* I7(b) and SG-1.  Two peers holding the same move set materialize the same *)
(* tree AND refuse the same moves.                                          *)
(*                                                                          *)
(* Say plainly what this is worth: under the honest fold it is TRUE BY       *)
(* CONSTRUCTION, because `Materialize` reads `held[p]` and nothing else.     *)
(* The model is not evidence that a peer computes it; it is evidence about   *)
(* what the rule has to be, and MUT_MATERIALIZE_IN_ARRIVAL_ORDER is the      *)
(* implementation bug it exists to catch --- the fold that applies what      *)
(* arrived in the order it arrived. `order.rs` names exactly that bug in its *)
(* own module comment, and this is that sentence with a trace attached.      *)
(***************************************************************************)
ConcurrentMovesResolveIdenticallyOnEveryPeer ==
    \A p, q \in Peers :
        held[p] = held[q] =>
            /\ Materialize(p).par      = Materialize(q).par
            /\ Materialize(p).rejected = Materialize(q).rejected

(***************************************************************************)
(* SG-6 and charter P6: object identity is not a function of path.  A move   *)
(* changes where an object is and never which object it is, so the set of    *)
(* objects is invariant under materialization.  Stated because a resolution  *)
(* rule that "resolved" a cyclic move by deleting one side would satisfy     *)
(* both invariants above and be a charter P5 violation.                     *)
(***************************************************************************)
NoObjectIsLostToAMove ==
    \A p \in Peers : DOMAIN TreeOf(p) = Nodes

(***************************************************************************)
(* REJECTION IS A CONCURRENCY OUTCOME, NEVER A SOLO ONE.  A peer holding one *)
(* move applies it: every move was valid against the state its author could  *)
(* see, and one move on its own cannot close a cycle from a flat tree.  So a *)
(* rejection in this model always means two moves that were each fine apart. *)
(*                                                                          *)
(* Stated because the alternative reading of a passing acyclicity run is     *)
(* that the fold rejects generously --- a materializer that refused          *)
(* everything would keep the tree a tree and be useless, and                 *)
(* DirectoryAncestryIsAcyclic alone cannot tell the two apart.               *)
(***************************************************************************)
ASoloMoveIsNeverRejected ==
    \A p \in Peers :
        Cardinality(held[p]) <= 1 => Materialize(p).rejected = {}

=============================================================================
