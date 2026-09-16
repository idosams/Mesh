//! Actor head advancement: the fold that turns a delivered causal set into a head.
//!
//! # The one rule the rest follows from
//!
//! **The head is a function of the applied causal set and of nothing else.** Not of arrival order,
//! not of how many times something arrived, not of any clock, and not of what an author claimed.
//! Everything below is that sentence made operational:
//!
//! * *Arrival order* cannot matter, because the head is derived from a set, and the set is ordered
//!   by a rule ([`HeadAdvancement::applied`]) rather than by history.
//! * *Duplication* cannot matter, because applying is a set insertion and a second delivery of a
//!   known identifier is answered [`Reception::AlreadyApplied`] before anything is recomputed.
//! * *A clock* cannot matter, because no field of a clock is read anywhere in this crate —
//!   `src/no_ambient_input.rs` proves that at compile time.
//! * *An author's claim* cannot matter, because both heads a ChangeSet carries are recomputed on
//!   arrival and a mismatch is [`Refusal`], not belief.
//!
//! # The causal order, and why it is that order
//!
//! Applied ChangeSets are ordered by **causal depth, then identifier**: depth is zero for a genesis
//! ChangeSet and one more than its deepest causal parent otherwise; ties break on the
//! thirty-two-byte identifier, which is a content digest. That is this protocol's spelling of
//! `lamport → content-hash`. Wall-clock time is not a tiebreaker and is not consulted; the hybrid
//! logical time a ChangeSet carries is metadata for display, never an input here (`docs/protocol.md`
//! §3.3, OG-6).
//!
//! Depth strictly increases across a causal edge, so the order is always a linear extension of the
//! causal partial order: a parent is never ordered after a child. Both halves are pure functions
//! of the causal set, so two actors holding the same set produce the same sequence and therefore
//! the same head — which is the whole of `convergence` (`docs/protocol.md` §3.3).
//!
//! # What a resulting head is derived from, and why that is not circular
//!
//! [`HeadAdvancement::apply`] derives a ChangeSet's `resulting_head` as the head over
//! `ancestry(causal parents) ∪ {changeset.id()}` — a fold over a set containing the ChangeSet's own
//! identifier. That is well defined **only if the identifier does not, in turn, bind the head**, and
//! on `main` it did: `mesh-types` binds `resulting_head` as the seventh of ten fields of the
//! ChangeSet pre-image, so identifier and head each depended on the other and neither was
//! computable. Nothing here could see it, because this crate declares no dependency on that one and
//! its own tests construct identifiers directly rather than deriving them.
//!
//! `docs/adr/0036-name-a-changeset-by-its-transition-and-let-the-head-it-produces-be-derived.md`
//! rules for the derivation in this file: a ChangeSet is named by its **nine** authored fields,
//! `resulting_head` excluded, and the fold below is the definition of the head. Nothing this file
//! does changes.
//!
//! **The other side of that ruling has not landed.** Plan §14.3 rule 3 gives the schema and its
//! implementation two runs, so `crates/mesh-types/src/changeset.rs` still binds ten fields today
//! and `Refusal::ResultingHeadNotDerived` would still refuse a ChangeSet identified the old way.
//! `01KZECDSPBM2V8KVPRNZNYPKRE` is open for that.
//!
//! # Cost, stated rather than implied
//!
//! Applying one ChangeSet recomputes the head over the whole applied set: O(n log n) in the number
//! of applied ChangeSets, plus a traversal of the causal parents' ancestors to derive the claimed
//! base head. That is the shape a correctness-first implementation should have, and it is not a
//! shape any benchmark budget has been set against — this task carries no benchmark. Incremental
//! head derivation is a later optimisation, and it needs a test that the incremental and the
//! from-scratch head agree before it is worth anything.

use core::marker::PhantomData;
use std::collections::{BTreeMap, BTreeSet};

use crate::changeset::DeliveredChangeSet;
use crate::digest::{head_over, HeadDigest};
use crate::head::{ActorHead, HeadState};
use crate::ids::{ActorId, ChangeSetId, HeadId};
use crate::parents::CausalParents;
use crate::reception::{KnownMissing, Reception, Refusal};

/// What the fold remembers about one applied ChangeSet.
///
/// The parents are kept because deriving a later ChangeSet's claimed base head needs the ancestor
/// traversal; the depth is kept because recomputing it on every ordering would make the fold
/// quadratic for no gain.
#[derive(Clone, Debug, PartialEq, Eq)]
struct AppliedChangeSet {
    parents: CausalParents,
    depth: u64,
}

/// One actor's head and the causal knowledge behind it.
///
/// Immutable. Every operation returns a new value, so a caller cannot hold a head that silently
/// moved underneath it — which is the same property, at the API level, that
/// [`HeadAdvancement::offer_for_review`] gives a review offer at the protocol level.
///
/// `D` is the digest a head is named under; [`HeadDigest`] says why this crate ships no
/// implementation of it.
pub struct HeadAdvancement<D: HeadDigest> {
    actor: ActorId,
    applied: BTreeMap<ChangeSetId, AppliedChangeSet>,
    buffered: BTreeMap<ChangeSetId, DeliveredChangeSet>,
    head: HeadId,
    review: Option<ActorHead>,
    digest: PhantomData<fn() -> D>,
}

impl<D: HeadDigest> HeadAdvancement<D> {
    /// An actor that has applied nothing.
    ///
    /// Its head is the head of the empty causal set, which is a real head rather than an absence:
    /// a genesis ChangeSet names it as the head it was authored against, and that claim is checked
    /// like any other.
    #[must_use]
    pub fn new(actor: ActorId) -> Self {
        Self {
            actor,
            applied: BTreeMap::new(),
            buffered: BTreeMap::new(),
            head: head_over::<D, _>([].iter()),
            review: None,
            digest: PhantomData,
        }
    }

    /// Whose head this is.
    #[must_use]
    pub const fn actor(&self) -> ActorId {
        self.actor
    }

    /// The current head.
    #[must_use]
    pub const fn head(&self) -> HeadId {
        self.head
    }

    /// The current head as an actor head, carrying its review state.
    #[must_use]
    pub fn actor_head(&self) -> ActorHead {
        ActorHead::new(self.actor, self.head, self.head_state())
    }

    /// Where the current head sits on the review axis.
    ///
    /// `ready for review` only while the offered head *is* the current head. Once the actor works
    /// past it, the current head is `working` again and the offer keeps naming the state it named.
    #[must_use]
    pub fn head_state(&self) -> HeadState {
        match &self.review {
            Some(offer) if offer.head() == self.head => offer.state(),
            _ => HeadState::Working,
        }
    }

    /// The head offered for review, if one has been offered.
    #[must_use]
    pub const fn review_offer(&self) -> Option<&ActorHead> {
        self.review.as_ref()
    }

    /// Offer the current head for review.
    ///
    /// The head identifier is *copied* into the offer. Nothing this type can be asked to do
    /// afterwards moves it, which is what makes "an approval refers to exact bytes" structurally
    /// true here rather than a rule somebody has to remember.
    #[must_use]
    pub fn offer_for_review(&self) -> Self {
        self.with_review(Some(ActorHead::new(
            self.actor,
            self.head,
            HeadState::ReadyForReview,
        )))
    }

    /// Mark the offered head superseded. No offer, no change.
    #[must_use]
    pub fn supersede_review_offer(&self) -> Self {
        self.with_offer_state(HeadState::Superseded)
    }

    /// Mark the offered head archived. No offer, no change.
    #[must_use]
    pub fn archive_review_offer(&self) -> Self {
        self.with_offer_state(HeadState::Archived)
    }

    /// Every applied ChangeSet, in causal order.
    ///
    /// This is the sequence the head is derived from, exposed so that a disagreement between two
    /// peers can be localised to a ChangeSet instead of to a digest.
    #[must_use]
    pub fn applied(&self) -> Vec<ChangeSetId> {
        self.order(self.applied.iter().map(|(id, entry)| (entry.depth, *id)))
    }

    /// Whether this ChangeSet has been applied.
    #[must_use]
    pub fn has_applied(&self, id: &ChangeSetId) -> bool {
        self.applied.contains_key(id)
    }

    /// The ChangeSets the actor has, and the causal parents they are still waiting for.
    ///
    /// Held indefinitely. Nothing here expires, evicts or collects a buffered ChangeSet.
    #[must_use]
    pub fn known_missing(&self) -> Vec<KnownMissing> {
        self.buffered
            .iter()
            .map(|(id, changeset)| {
                let missing: Vec<ChangeSetId> = changeset
                    .parents()
                    .as_slice()
                    .iter()
                    .filter(|parent| !self.applied.contains_key(parent))
                    .copied()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                KnownMissing::new(*id, missing)
            })
            .collect()
    }

    /// The applied ChangeSets no applied ChangeSet follows — what a new ChangeSet must name as its
    /// causal parents to leave no history unrelated.
    #[must_use]
    pub fn tips(&self) -> Vec<ChangeSetId> {
        let followed: BTreeSet<ChangeSetId> = self
            .applied
            .values()
            .flat_map(|entry| entry.parents.as_slice().iter().copied())
            .collect();
        self.order(
            self.applied
                .iter()
                .filter(|(id, _)| !followed.contains(*id))
                .map(|(id, entry)| (entry.depth, *id)),
        )
    }

    /// Author a ChangeSet from this actor's current head.
    ///
    /// The causal parents are the actor's own [`HeadAdvancement::tips`] and are not a caller's to
    /// choose: an author that could choose them could drop one, and a dropped causal parent is
    /// precisely the fault that makes two actors' histories impossible to relate. The two heads
    /// are derived, never supplied.
    ///
    /// # Errors
    ///
    /// [`Refusal::AlreadyKnown`] when the identifier is one this actor already holds.
    pub fn author(&self, id: ChangeSetId) -> Result<(Self, DeliveredChangeSet), Refusal> {
        if self.applied.contains_key(&id) || self.buffered.contains_key(&id) {
            return Err(Refusal::AlreadyKnown { changeset: id });
        }
        let tips = self.tips();
        let depth = self.depth_after(&tips);
        let resulting = self.head_including(id, depth);
        let changeset = DeliveredChangeSet::new(
            id,
            self.actor,
            CausalParents::from_slice(&tips),
            self.head,
            resulting,
        );
        let (advanced, reception) = self.deliver(changeset.clone());
        match reception {
            Reception::Applied { .. } => Ok((advanced, changeset)),
            Reception::Refused(refusal) => Err(refusal),
            // Unreachable: the identifier was checked against both collections above, and the
            // causal parents are this actor's own applied tips, so nothing is missing. Answering
            // "already known" is the conservative reading if it ever becomes reachable.
            _ => Err(Refusal::AlreadyKnown { changeset: id }),
        }
    }

    /// Deliver one ChangeSet.
    ///
    /// The whole contract of this crate is in the four answers: applied, already known, buffered
    /// as a known-missing dependency, or refused. Nothing is ever discarded.
    #[must_use]
    pub fn deliver(&self, changeset: DeliveredChangeSet) -> (Self, Reception) {
        let id = changeset.id();
        if self.applied.contains_key(&id) {
            return (self.clone(), Reception::AlreadyApplied);
        }
        if self.buffered.contains_key(&id) {
            return (self.clone(), Reception::AlreadyBuffered);
        }
        if let Some(refusal) = structural_refusal(&changeset) {
            return (self.clone(), Reception::Refused(refusal));
        }

        let missing: Vec<ChangeSetId> = changeset
            .parents()
            .as_slice()
            .iter()
            .filter(|parent| !self.applied.contains_key(parent))
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if !missing.is_empty() {
            let mut buffered = self.buffered.clone();
            buffered.insert(id, changeset);
            return (
                self.with_buffered(buffered),
                Reception::Buffered { missing },
            );
        }

        match self.apply(&changeset) {
            Err(refusal) => (self.clone(), Reception::Refused(refusal)),
            Ok(advanced) => {
                let (advanced, mut unblocked, mut refused) = advanced.drain_buffered();
                unblocked.push(id);
                refused.sort_by_key(|(id, _)| *id);
                let applied = advanced.order(
                    unblocked
                        .into_iter()
                        .map(|id| (advanced.depth_of(&id), id))
                        .collect::<Vec<_>>()
                        .into_iter(),
                );
                let head = advanced.head;
                (
                    advanced,
                    Reception::Applied {
                        head,
                        applied,
                        refused,
                    },
                )
            }
        }
    }

    /// Verify a ChangeSet against this actor's causal knowledge and apply it.
    ///
    /// Every causal parent must already be applied; the caller guarantees that.
    fn apply(&self, changeset: &DeliveredChangeSet) -> Result<Self, Refusal> {
        let parents = changeset.parents().as_slice();
        if let Some((parent, implied_by)) = self.implied_parent(parents) {
            return Err(Refusal::ParentImplied {
                changeset: changeset.id(),
                parent,
                implied_by,
            });
        }
        let base = self.ancestry_of(parents);
        let derived_base = self.head_of(base.iter().copied());
        if derived_base != changeset.base_head() {
            return Err(Refusal::BaseHeadNotDerived {
                changeset: changeset.id(),
                claimed: changeset.base_head(),
                derived: derived_base,
            });
        }

        let depth = self.depth_after(parents);
        let mut own: Vec<ChangeSetId> = base.iter().copied().collect();
        own.push(changeset.id());
        let derived_resulting = self.head_with(own.into_iter(), changeset.id(), depth);
        if derived_resulting != changeset.resulting_head() {
            return Err(Refusal::ResultingHeadNotDerived {
                changeset: changeset.id(),
                claimed: changeset.resulting_head(),
                derived: derived_resulting,
            });
        }

        let mut applied = self.applied.clone();
        applied.insert(
            changeset.id(),
            AppliedChangeSet {
                parents: changeset.parents().clone(),
                depth,
            },
        );
        let mut buffered = self.buffered.clone();
        buffered.remove(&changeset.id());
        let head = head_over::<D, _>(
            self.order(applied.iter().map(|(id, entry)| (entry.depth, *id)))
                .iter(),
        );
        Ok(Self {
            actor: self.actor,
            applied,
            buffered,
            head,
            review: self.review,
            digest: PhantomData,
        })
    }

    /// Apply every buffered ChangeSet whose causal parents have all arrived, repeatedly, until no
    /// further one becomes appliable.
    ///
    /// Candidates are taken in ascending identifier order, so the sequence of applications is a
    /// function of the buffer's contents and not of the order they were buffered in.
    fn drain_buffered(self) -> (Self, Vec<ChangeSetId>, Vec<(ChangeSetId, Refusal)>) {
        let mut state = self;
        let mut applied = Vec::new();
        let mut refused = Vec::new();
        loop {
            let ready: Vec<ChangeSetId> = state
                .buffered
                .iter()
                .filter(|(_, changeset)| {
                    changeset
                        .parents()
                        .as_slice()
                        .iter()
                        .all(|parent| state.applied.contains_key(parent))
                })
                .map(|(id, _)| *id)
                .collect();
            if ready.is_empty() {
                return (state, applied, refused);
            }
            for id in ready {
                let Some(changeset) = state.buffered.get(&id).cloned() else {
                    continue;
                };
                match state.apply(&changeset) {
                    Ok(advanced) => {
                        state = advanced;
                        applied.push(id);
                    }
                    Err(refusal) => {
                        let mut buffered = state.buffered.clone();
                        buffered.remove(&id);
                        state = state.with_buffered(buffered);
                        refused.push((id, refusal));
                    }
                }
            }
        }
    }

    /// A causal parent another causal parent already follows, if there is one.
    ///
    /// Causal parents must be minimal — no parent may be an ancestor of another. A redundant
    /// parent does not move the head, because it names a causal set already covered, so nothing in
    /// the head derivation can see it. It is refused one level up instead: the parent list is
    /// bound into the ChangeSet's own identifier, so a second spelling of one transition would be
    /// a second identifier for one state.
    ///
    /// The answer depends only on the causal set, never on what else this actor has applied, so
    /// every receiver holding the parents gives the same one.
    fn implied_parent(&self, parents: &[ChangeSetId]) -> Option<(ChangeSetId, ChangeSetId)> {
        for parent in parents {
            // Every parent is applied by the time `apply` runs; skipping an absent one keeps this
            // a pure predicate rather than making the caller's precondition load-bearing here.
            let Some(entry) = self.applied.get(parent) else {
                continue;
            };
            let ancestors = self.ancestry_of(entry.parents.as_slice());
            for other in parents {
                if other != parent && ancestors.contains(other) {
                    return Some((*other, *parent));
                }
            }
        }
        None
    }

    /// Every ancestor of these causal parents, and the parents themselves.
    fn ancestry_of(&self, parents: &[ChangeSetId]) -> BTreeSet<ChangeSetId> {
        let mut seen = BTreeSet::new();
        let mut pending: Vec<ChangeSetId> = parents.to_vec();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(entry) = self.applied.get(&id) {
                pending.extend(entry.parents.as_slice().iter().copied());
            }
        }
        seen
    }

    /// The causal depth a ChangeSet following these parents would have.
    fn depth_after(&self, parents: &[ChangeSetId]) -> u64 {
        parents
            .iter()
            .map(|parent| self.depth_of(parent).saturating_add(1))
            .max()
            .unwrap_or(0)
    }

    /// The causal depth of an applied ChangeSet, or zero for one this actor has not applied.
    fn depth_of(&self, id: &ChangeSetId) -> u64 {
        self.applied.get(id).map_or(0, |entry| entry.depth)
    }

    /// The head over a set of already-applied ChangeSets.
    fn head_of<I: Iterator<Item = ChangeSetId>>(&self, ids: I) -> HeadId {
        head_over::<D, _>(self.order(ids.map(|id| (self.depth_of(&id), id))).iter())
    }

    /// The head over a set that includes one ChangeSet this actor has not applied yet.
    fn head_with<I: Iterator<Item = ChangeSetId>>(
        &self,
        ids: I,
        pending: ChangeSetId,
        pending_depth: u64,
    ) -> HeadId {
        head_over::<D, _>(
            self.order(ids.map(|id| {
                if id == pending {
                    (pending_depth, id)
                } else {
                    (self.depth_of(&id), id)
                }
            }))
            .iter(),
        )
    }

    /// The head over this actor's applied set plus one ChangeSet it is about to author.
    fn head_including(&self, pending: ChangeSetId, pending_depth: u64) -> HeadId {
        let mut ranked: Vec<(u64, ChangeSetId)> = self
            .applied
            .iter()
            .map(|(id, entry)| (entry.depth, *id))
            .collect();
        ranked.push((pending_depth, pending));
        head_over::<D, _>(self.order(ranked.into_iter()).iter())
    }

    /// Causal depth, then identifier — the crate's one ordering rule, in one place.
    fn order<I: Iterator<Item = (u64, ChangeSetId)>>(&self, ranked: I) -> Vec<ChangeSetId> {
        let mut ranked: Vec<(u64, ChangeSetId)> = ranked.collect();
        ranked.sort_unstable();
        ranked.dedup();
        ranked.into_iter().map(|(_, id)| id).collect()
    }

    /// The same advancement with a different buffer.
    fn with_buffered(&self, buffered: BTreeMap<ChangeSetId, DeliveredChangeSet>) -> Self {
        Self {
            actor: self.actor,
            applied: self.applied.clone(),
            buffered,
            head: self.head,
            review: self.review,
            digest: PhantomData,
        }
    }

    /// The same advancement with a different review offer.
    fn with_review(&self, review: Option<ActorHead>) -> Self {
        Self {
            actor: self.actor,
            applied: self.applied.clone(),
            buffered: self.buffered.clone(),
            head: self.head,
            review,
            digest: PhantomData,
        }
    }

    /// The same advancement with the offered head moved to another review state.
    fn with_offer_state(&self, state: HeadState) -> Self {
        self.with_review(self.review.map(|offer| offer.in_state(state)))
    }
}

/// Everything about a ChangeSet that can be refused without knowing anything else.
///
/// Both faults here would make the causal order ill-defined — a ChangeSet that follows itself has
/// no depth, and a repeated parent makes the parent list disagree with the parent set — so they are
/// checked before the buffer, not after it. A ChangeSet that cannot be ordered is never held.
fn structural_refusal(changeset: &DeliveredChangeSet) -> Option<Refusal> {
    let mut seen = BTreeSet::new();
    for parent in changeset.parents().as_slice() {
        if *parent == changeset.id() {
            return Some(Refusal::SelfParent {
                changeset: changeset.id(),
            });
        }
        if !seen.insert(*parent) {
            return Some(Refusal::ParentRepeated {
                changeset: changeset.id(),
                parent: *parent,
            });
        }
    }
    None
}

impl<D: HeadDigest> Clone for HeadAdvancement<D> {
    fn clone(&self) -> Self {
        Self {
            actor: self.actor,
            applied: self.applied.clone(),
            buffered: self.buffered.clone(),
            head: self.head,
            review: self.review,
            digest: PhantomData,
        }
    }
}

impl<D: HeadDigest> PartialEq for HeadAdvancement<D> {
    fn eq(&self, other: &Self) -> bool {
        self.actor == other.actor
            && self.head == other.head
            && self.applied == other.applied
            && self.buffered == other.buffered
            && self.review == other.review
    }
}

impl<D: HeadDigest> Eq for HeadAdvancement<D> {}

impl<D: HeadDigest> core::fmt::Debug for HeadAdvancement<D> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("HeadAdvancement")
            .field("actor", &self.actor)
            .field("head", &self.head)
            .field("applied", &self.applied.len())
            .field("buffered", &self.buffered.len())
            .field("review", &self.review)
            .finish()
    }
}
