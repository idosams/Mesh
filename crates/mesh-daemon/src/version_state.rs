//! This replica's own version state, derived from the records on disk.
//!
//! # What this module exists to stop
//!
//! `workspace.state` used to refuse two subjects by naming a crate: "materialising a workspace's
//! files needs mesh-materializer, which is not a dependency of this crate", and "the six-state
//! model over an actor's work needs mesh-state, which is not a dependency of this crate". A person
//! reading either sentence learns something about our build and nothing about their workspace.
//! `docs/adr/0014-narrow-the-lockfile-fence-to-admit-an-audited-cryptographic-dependency.md`
//! admits an intra-workspace `path` edge without conditions, so both sentences were avoidable, and
//! this crate — plan §8.3's rank-5 service layer, the composition root — is where the edge belongs.
//!
//! # The digest arrives here because both core crates refuse to ship one
//!
//! `crates/mesh-state/src/digest.rs` states the reason plainly: a default implementation there
//! "would be a name that looks like a `HeadId` and is not the one any other implementation
//! computes, and it would be reached for exactly once by a lane in a hurry". So [`Blake3Head`] is
//! that seam, filled with `mesh_types::Blake3Hasher` — the BLAKE3 the protocol names and the one
//! `mesh-types` already derives every record identifier with. One implementation, one place.
//!
//! # What is derived here, and what is only claimed
//!
//! The head published by [`PrivateVersion::version`] is `mesh-state`'s derivation over the causal
//! set the record file holds, using each record's **own declared causal parents**. It is a pure
//! function of those records: fold them twice in different file orders and the bytes are equal.
//! An empty record set has no newest change and therefore publishes no head identifier.
//!
//! What it is **not** is a check on the author. `mesh_types::ChangeSet` carries `base_head` and
//! `resulting_head`; `mesh_store::OperationRecord` — the record actually written to disk —
//! persists neither, and `mesh_state::DeliveredChangeSet` requires both. So a receiver cannot
//! recompute an author's claim and disagree with it, because the claim is not on disk.
//! [`PrivateVersion::checked_against_author_claim`] is `false` for exactly that reason, on every
//! path, and it is a published field rather than a comment because a caller that does not know
//! this would read a derived head as a verified one.
//!
//! Deriving the two heads at all takes a small detour, and it is deliberate rather than clever.
//! `mesh-state` exposes head derivation through `HeadAdvancement::author`, which chooses the
//! causal parents itself — correct for authoring, wrong for replay, because a record delivered
//! from a peer names parents this replica does not get to pick. The one public path from "known
//! parents" to "derived heads" is the refusal channel: `Refusal::BaseHeadNotDerived` and
//! `Refusal::ResultingHeadNotDerived` both carry the value the crate derived. [`deliver_derived`]
//! reads them and re-delivers. It calls no private function and re-implements no framing, which is
//! the property that matters: there is still exactly one implementation of how a head is named.
//!
//! # Two implementations of one order, compared
//!
//! `mesh_state::HeadAdvancement::applied` and `mesh_materializer::causal_order` are independent
//! implementations of the same rule — causal depth, then identifier. This module runs both over
//! the same records and publishes whether they agree. One implementation agreeing with itself is
//! not evidence; two agreeing is. A disagreement is reported as a disagreement and never resolved
//! by preferring one of them.

use std::collections::{BTreeMap, BTreeSet};

use mesh_materializer::{causal_order, AppliedChangeSet};
use mesh_state::{
    ActorId, CausalParents, ChangeSetId, DeliveredChangeSet, HeadAdvancement, HeadDigest, HeadId,
    HeadState, Reception, Refusal,
};
use mesh_store::{Index, OperationRecord, RecordDigest};
use mesh_types::{Blake3Hasher, DigestHasher};

/// How many times [`deliver_derived`] will re-deliver while deriving the two heads.
///
/// Two derivations are needed — the base head, then the resulting head — and the third delivery
/// applies. The fourth exists so that a future `mesh-state` growing a third derived field cannot
/// turn this into an infinite loop; a run that exhausts it reports the change as underivable
/// rather than looping or panicking.
const MAX_DERIVATION_ROUNDS: usize = 4;

/// BLAKE3 as `mesh-state`'s head digest, supplied by the composition root.
///
/// The only implementation of [`HeadDigest`] in this repository. `mesh-state` ships none on
/// purpose and says why; this crate is the root that owes it one, and it takes the same
/// `mesh_types::Blake3Hasher` every record identifier in the protocol is derived with rather than
/// a second copy of BLAKE3.
pub struct Blake3Head(Blake3Hasher);

impl HeadDigest for Blake3Head {
    fn start() -> Self {
        Self(Blake3Hasher::new())
    }

    fn absorb(&mut self, bytes: &[u8]) {
        DigestHasher::update(&mut self.0, bytes);
    }

    fn finish(self) -> HeadId {
        HeadId::from_bytes(*DigestHasher::finalize(self.0).as_bytes())
    }
}

/// One change this replica holds that is waiting for a change it has not received.
///
/// Reported rather than dropped. A count that swallowed a waiting change would read as a smaller,
/// healthier workspace than the one on disk, which is the failure this whole module is shaped
/// against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaitingChange {
    /// The change that cannot be applied yet.
    pub change: String,
    /// What it is waiting for, as identifiers this replica has not received.
    pub missing: Vec<String>,
}

/// Whether the two causal orders agree over the same records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderAgreement {
    /// Both implementations produced the same sequence.
    Agree,
    /// They produced different sequences. Carries both lengths so a reader can see which one is
    /// short without asking for the sequences.
    Differ {
        /// How many changes `mesh-state` ordered.
        head_advancement: usize,
        /// How many changes `mesh-materializer` ordered.
        materializer: usize,
    },
}

impl OrderAgreement {
    /// Whether the two agreed.
    #[must_use]
    pub const fn agreed(self) -> bool {
        matches!(self, Self::Agree)
    }
}

/// This replica's own version state, folded from the records on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateVersion {
    version: Option<String>,
    state: HeadState,
    changes_applied: usize,
    concurrent_changes: usize,
    waiting: Vec<WaitingChange>,
    order: OrderAgreement,
}

impl PrivateVersion {
    /// The version identifier this replica currently holds, as 64 lowercase hexadecimal
    /// characters, or `None` when no saved change exists and therefore there is no head.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// Where that version sits on the review axis.
    ///
    /// Derived, not constant: it is `ready for review` exactly while the change under review is
    /// still the latest one this replica holds, and `working` once the workspace has moved past
    /// it. `mesh_state::HeadAdvancement::head_state` owns that rule and this crate does not
    /// restate it.
    #[must_use]
    pub const fn state(&self) -> HeadState {
        self.state
    }

    /// How many changes went into it.
    #[must_use]
    pub const fn changes_applied(&self) -> usize {
        self.changes_applied
    }

    /// How many of those changes nothing else follows — how wide the workspace is at its newest
    /// point. One means a single line of work; more means concurrent work not yet brought together.
    #[must_use]
    pub const fn concurrent_changes(&self) -> usize {
        self.concurrent_changes
    }

    /// Every waiting change held on disk.
    #[must_use]
    pub fn waiting(&self) -> &[WaitingChange] {
        &self.waiting
    }

    /// How many waiting changes were left out of the enumeration.
    ///
    /// Kept as an additive wire-compatibility field. The complete on-disk set is now enumerated,
    /// so this is always zero.
    #[must_use]
    pub const fn waiting_not_listed(&self) -> usize {
        0
    }

    /// Whether the two causal orders agreed.
    #[must_use]
    pub const fn order(&self) -> OrderAgreement {
        self.order
    }

    /// Whether the derived version was checked against what its authors claimed.
    ///
    /// Always `false`, and published rather than omitted. `mesh_store::OperationRecord` does not
    /// persist the two head fields `mesh_types::ChangeSet` carries, so there is no claim on disk to
    /// check against. A caller that assumed otherwise would read a derived version as a verified
    /// one, which is the difference between "this is what these records say" and "this is what a
    /// peer agreed to".
    #[must_use]
    pub const fn checked_against_author_claim(&self) -> bool {
        false
    }

    /// How the version identifier is named, so a client can tell two derivations apart.
    #[must_use]
    pub const fn derivation(&self) -> &'static str {
        "blake3/mesh.v0.actor-head"
    }
}

/// Fold the operation records an index holds into this replica's version state.
///
/// Deterministic: the input is a set, both orders inside are pure functions of that set, and the
/// output does not depend on the order the records were read in.
#[must_use]
pub fn fold(index: &Index) -> PrivateVersion {
    let operations = operations_in(index);
    let reviewed = reviewed_operations(index, &operations);

    let mut advancement: HeadAdvancement<Blake3Head> = HeadAdvancement::new(replaying_actor());
    for record in delivery_order(&operations) {
        let (next, reception) = deliver_derived(&advancement, record);
        advancement = next;
        // A change that was applied and is itself under review offers the head it produced. The
        // offer holds bytes rather than a pointer, so a later change leaves it naming what it
        // named and `head_state` answers `working` again — which is `mesh-state`'s rule, applied
        // here rather than re-implemented.
        if matches!(reception, Reception::Applied { .. }) && reviewed.contains(&record.id) {
            advancement = advancement.offer_for_review();
        }
    }

    let applied = advancement.applied();
    let materializer_order = causal_order(&materializer_set(&operations));
    let order = if applied
        .iter()
        .map(ChangeSetId::as_bytes)
        .eq(materializer_order
            .iter()
            .map(mesh_materializer::ChangeSetId::as_bytes))
    {
        OrderAgreement::Agree
    } else {
        OrderAgreement::Differ {
            head_advancement: applied.len(),
            materializer: materializer_order.len(),
        }
    };

    let waiting_all: Vec<WaitingChange> = advancement
        .known_missing()
        .iter()
        .map(|entry| WaitingChange {
            change: hex32(entry.waiting().as_bytes()),
            missing: entry
                .missing()
                .iter()
                .map(|id| hex32(id.as_bytes()))
                .collect(),
        })
        .collect();
    PrivateVersion {
        version: (!operations.is_empty()).then(|| hex32(advancement.head().as_bytes())),
        state: advancement.head_state(),
        changes_applied: applied.len(),
        concurrent_changes: advancement.tips().len(),
        waiting: waiting_all,
        order,
    }
}

/// The label a replay carries.
///
/// A replay of records other actors authored is not an authoring session, so there is no actor
/// whose head this is. `mesh_state::HeadAdvancement` needs a label anyway; it is used only by
/// `actor()`, `actor_head()` and `author()`, none of which this module calls, so the label never
/// reaches the derivation and never reaches the wire. `test_no_actor_identifier_is_published`
/// holds that second half.
fn replaying_actor() -> ActorId {
    ActorId::from_bytes([0; 32])
}

/// Every operation record the index holds, by identifier.
fn operations_in(index: &Index) -> BTreeMap<RecordDigest, OperationRecord> {
    let mut all = BTreeMap::new();
    for actor in index.actors() {
        for record in index.operations_of(&actor) {
            all.insert(record.id, record.clone());
        }
    }
    all
}

/// The operations a review bundle on disk names as its subject.
fn reviewed_operations(
    index: &Index,
    operations: &BTreeMap<RecordDigest, OperationRecord>,
) -> BTreeSet<RecordDigest> {
    index
        .review_bundles()
        .iter()
        .filter_map(|bundle| index.review(bundle))
        .map(|review| review.subject_operation)
        .filter(|subject| operations.contains_key(subject))
        .collect()
}

/// Deliver parents before the changes that name them, so a whole set applies in one pass.
///
/// This is a *delivery* order and not the causal order: nothing here decides how the head is
/// named. `mesh-state` buffers anything that arrives early and applies it when its parent lands,
/// so a wrong guess here costs a pass and never an answer. Sorting by how deep a record's declared
/// parents reach, then by identifier, is a pure function of the set and keeps the fold
/// reproducible.
fn delivery_order(operations: &BTreeMap<RecordDigest, OperationRecord>) -> Vec<&OperationRecord> {
    let mut depths: BTreeMap<RecordDigest, usize> = BTreeMap::new();
    // At most one level is resolved per pass, so `len` passes settle every acyclic set; a set with
    // a cycle stops with the depths it reached and `mesh-state` refuses the cycle itself.
    for _ in 0..operations.len() {
        let mut moved = false;
        for (id, record) in operations {
            let depth = record
                .parents
                .iter()
                .map(|parent| depths.get(parent).map_or(0, |value| value + 1))
                .max()
                .unwrap_or(0);
            if depths.get(id) != Some(&depth) {
                depths.insert(*id, depth);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    let mut ordered: Vec<&OperationRecord> = operations.values().collect();
    ordered.sort_by_key(|record| (depths.get(&record.id).copied().unwrap_or(0), record.id));
    ordered
}

/// The same set, in `mesh-materializer`'s vocabulary, carrying no operations.
///
/// No operation is passed because none is on disk: an `OperationRecord` carries a
/// `payload_digest`, and the payload lives in a content-addressed store this daemon does not open.
/// `causal_order` reads identifiers and causal parents and nothing else, so it is exactly the part
/// of `mesh-materializer` these records can feed. `materialize` is deliberately not called — over
/// an empty operation set it would answer an empty workspace, and an empty answer that looks like a
/// real one is worse than the refusal it would replace.
fn materializer_set(operations: &BTreeMap<RecordDigest, OperationRecord>) -> Vec<AppliedChangeSet> {
    operations
        .values()
        .map(|record| {
            AppliedChangeSet::new(
                mesh_materializer::ChangeSetId::from_bytes(*record.id.as_bytes()),
                record
                    .parents
                    .iter()
                    .map(|parent| mesh_materializer::ChangeSetId::from_bytes(*parent.as_bytes()))
                    .collect(),
                Vec::new(),
            )
        })
        .collect()
}

/// Deliver one record, deriving the two heads it does not carry.
///
/// See this module's header for why the refusal channel is the derivation path. Every other
/// reception — applied, already known, buffered, or a refusal that is not about a head — is
/// returned unchanged, so a genuine refusal is never mistaken for a derivation round.
fn deliver_derived(
    advancement: &HeadAdvancement<Blake3Head>,
    record: &OperationRecord,
) -> (HeadAdvancement<Blake3Head>, Reception) {
    let id = ChangeSetId::from_bytes(*record.id.as_bytes());
    let actor = ActorId::from_bytes(*record.actor.as_bytes());
    let parents: Vec<ChangeSetId> = record
        .parents
        .iter()
        .map(|parent| ChangeSetId::from_bytes(*parent.as_bytes()))
        .collect();

    let mut base = HeadId::from_bytes([0; 32]);
    let mut resulting = HeadId::from_bytes([0; 32]);
    for _ in 0..MAX_DERIVATION_ROUNDS {
        let delivered = DeliveredChangeSet::new(
            id,
            actor,
            CausalParents::from_slice(&parents),
            base,
            resulting,
        )
        // Carried because a record holds it, and read by nothing: `docs/protocol.md` §3.3 OG-6
        // says hybrid logical time never decides causality, and `mesh-state`'s own compile-time
        // scan proves no source file there can reach a clock. Dropping a received field would be
        // lossy; consulting it would be wrong. The counter is narrowed rather than lost — a value
        // past `u32` would be a record `mesh-store` should never have accepted, and saturating
        // keeps it distinguishable from zero.
        .with_hybrid_logical_time(
            record.hlc_millis,
            u32::try_from(record.hlc_counter).unwrap_or(u32::MAX),
        );
        let (next, reception) = advancement.deliver(delivered);
        match reception {
            Reception::Refused(Refusal::BaseHeadNotDerived { derived, .. }) => base = derived,
            Reception::Refused(Refusal::ResultingHeadNotDerived { derived, .. }) => {
                resulting = derived;
            }
            settled => return (next, settled),
        }
    }
    (
        advancement.clone(),
        Reception::Refused(Refusal::AlreadyKnown { changeset: id }),
    )
}

/// A 32-byte identifier as 64 lowercase hexadecimal characters.
fn hex32(bytes: &[u8; 32]) -> String {
    let mut text = String::with_capacity(64);
    for byte in bytes {
        text.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        text.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    use mesh_store::{
        no_session, rebuild, ApprovalRecord, ReviewRecord, ReviewVerdict, StoredRecord,
    };

    fn digest(byte: u8) -> RecordDigest {
        RecordDigest::from_bytes([byte; 32])
    }

    fn operation(id: u8, actor: u8, sequence: u64, parents: &[u8]) -> StoredRecord {
        StoredRecord::Operation(OperationRecord {
            id: digest(id),
            actor: digest(actor),
            actor_sequence: sequence,
            hlc_millis: 1_700_000_000_000 + sequence,
            hlc_counter: 0,
            policy_epoch: 1,
            session: no_session(),
            payload_digest: digest(id.wrapping_add(0x10)),
            parents: parents.iter().map(|byte| digest(*byte)).collect(),
        })
    }

    fn index_over(records: Vec<StoredRecord>) -> Index {
        rebuild(records, Vec::new()).expect("fold").0
    }

    #[test]
    fn an_empty_workspace_reports_no_head_identifier() {
        let folded = fold(&index_over(Vec::new()));
        assert_eq!(folded.changes_applied(), 0);
        assert_eq!(folded.concurrent_changes(), 0);
        assert!(
            folded.version().is_none(),
            "an empty record set has no head identifier"
        );
        assert_eq!(folded.state(), HeadState::Working);
        assert!(folded.waiting().is_empty());
    }

    #[test]
    fn the_version_is_derived_from_the_records_and_not_from_the_order_they_were_read_in() {
        let forwards = vec![
            operation(0x04, 0x02, 1, &[]),
            operation(0x05, 0x02, 2, &[0x04]),
            operation(0x07, 0x03, 1, &[]),
        ];
        let backwards: Vec<StoredRecord> = forwards.iter().rev().cloned().collect();

        let one = fold(&index_over(forwards));
        let other = fold(&index_over(backwards));

        assert_eq!(one.version(), other.version());
        assert_eq!(one.changes_applied(), 3);
        assert_eq!(one.concurrent_changes(), 2, "0x05 and 0x07 are concurrent");
    }

    #[test]
    fn a_different_set_of_records_names_a_different_version() {
        let one = fold(&index_over(vec![operation(0x04, 0x02, 1, &[])]));
        let other = fold(&index_over(vec![
            operation(0x04, 0x02, 1, &[]),
            operation(0x05, 0x02, 2, &[0x04]),
        ]));
        assert_ne!(one.version(), other.version());
    }

    #[test]
    fn a_change_whose_parent_is_not_on_disk_is_reported_waiting_and_is_not_counted_as_applied() {
        let folded = fold(&index_over(vec![
            operation(0x04, 0x02, 1, &[]),
            operation(0x09, 0x02, 2, &[0xee]),
        ]));

        assert_eq!(folded.changes_applied(), 1, "0x09 cannot be applied");
        assert_eq!(folded.waiting().len(), 1);
        assert_eq!(folded.waiting()[0].change, hex32(&[0x09; 32]));
        assert_eq!(folded.waiting()[0].missing, vec![hex32(&[0xee; 32])]);
        assert_eq!(folded.waiting_not_listed(), 0);
    }

    #[test]
    fn a_parent_that_arrives_late_still_applies_rather_than_staying_lost() {
        let out_of_order = fold(&index_over(vec![
            operation(0x05, 0x02, 2, &[0x04]),
            operation(0x04, 0x02, 1, &[]),
        ]));
        let in_order = fold(&index_over(vec![
            operation(0x04, 0x02, 1, &[]),
            operation(0x05, 0x02, 2, &[0x04]),
        ]));

        assert_eq!(out_of_order.changes_applied(), 2);
        assert!(out_of_order.waiting().is_empty());
        assert_eq!(out_of_order.version(), in_order.version());
    }

    #[test]
    fn the_two_causal_orders_agree_over_the_same_complete_record_set() {
        let folded = fold(&index_over(vec![
            operation(0x04, 0x02, 1, &[]),
            operation(0x05, 0x02, 2, &[0x04]),
            operation(0x07, 0x03, 1, &[]),
            operation(0x08, 0x03, 2, &[0x05, 0x07]),
        ]));
        assert_eq!(folded.order(), OrderAgreement::Agree);
        assert!(folded.order().agreed());
        assert_eq!(folded.changes_applied(), 4);
        assert_eq!(folded.concurrent_changes(), 1, "0x08 brings both lines in");
    }

    #[test]
    fn the_two_causal_orders_expose_disagreement_when_a_parent_is_absent() {
        let folded = fold(&index_over(vec![
            operation(0x04, 0x02, 1, &[]),
            operation(0x09, 0x02, 2, &[0xee]),
        ]));

        assert_eq!(
            folded.order(),
            OrderAgreement::Differ {
                head_advancement: 1,
                materializer: 2,
            },
            "a constant-true agreement result must not hide the incomplete record set"
        );
        assert!(!folded.order().agreed());
    }

    #[test]
    fn a_change_under_review_makes_the_latest_version_ready_for_review() {
        let folded = fold(&index_over(vec![
            operation(0x04, 0x02, 1, &[]),
            StoredRecord::Review(ReviewRecord {
                bundle: digest(0x20),
                subject_operation: digest(0x04),
                opened_by: digest(0x03),
            }),
        ]));
        assert_eq!(folded.state(), HeadState::ReadyForReview);
        assert_eq!(folded.state().as_str(), "ready for review");
    }

    #[test]
    fn working_past_the_change_under_review_leaves_the_latest_version_working() {
        let folded = fold(&index_over(vec![
            operation(0x04, 0x02, 1, &[]),
            operation(0x05, 0x02, 2, &[0x04]),
            StoredRecord::Review(ReviewRecord {
                bundle: digest(0x20),
                subject_operation: digest(0x04),
                opened_by: digest(0x03),
            }),
        ]));
        assert_eq!(
            folded.state(),
            HeadState::Working,
            "the offer keeps naming what it named; the workspace moved on"
        );
    }

    #[test]
    fn an_approval_on_disk_does_not_move_the_version_on_its_own() {
        // The shared version is refused, not answered, and nothing here may quietly answer it: an
        // approval whose signature nobody checked is not a decision this build can act on.
        let with_approval = fold(&index_over(vec![
            operation(0x04, 0x02, 1, &[]),
            StoredRecord::Review(ReviewRecord {
                bundle: digest(0x20),
                subject_operation: digest(0x04),
                opened_by: digest(0x03),
            }),
            StoredRecord::Approval(ApprovalRecord {
                approval: digest(0x21),
                bundle: digest(0x20),
                approver: digest(0x03),
                verdict: ReviewVerdict::Approved,
            }),
        ]));
        let without_approval = fold(&index_over(vec![
            operation(0x04, 0x02, 1, &[]),
            StoredRecord::Review(ReviewRecord {
                bundle: digest(0x20),
                subject_operation: digest(0x04),
                opened_by: digest(0x03),
            }),
        ]));
        assert_eq!(with_approval.version(), without_approval.version());
        assert_eq!(with_approval.state(), without_approval.state());
    }

    #[test]
    fn a_derived_version_never_claims_to_have_been_checked_against_an_author() {
        let folded = fold(&index_over(vec![operation(0x04, 0x02, 1, &[])]));
        assert!(!folded.checked_against_author_claim());
        assert_eq!(folded.derivation(), "blake3/mesh.v0.actor-head");
    }

    #[test]
    fn every_waiting_change_is_reported_when_more_than_sixty_four_are_on_disk() {
        let mut records = Vec::new();
        const LAST_WAITING_ID: u8 = 64;
        for byte in 0..=LAST_WAITING_ID {
            records.push(operation(byte, 0x02, u64::from(byte) + 1, &[0xee]));
        }
        let folded = fold(&index_over(records));
        assert_eq!(folded.waiting().len(), usize::from(LAST_WAITING_ID) + 1);
        assert_eq!(folded.waiting_not_listed(), 0);
        assert_eq!(folded.changes_applied(), 0);
        assert!(folded
            .waiting()
            .iter()
            .all(|entry| entry.missing == vec![hex32(&[0xee; 32])]));
    }

    #[test]
    fn the_replay_label_never_reaches_the_answer() {
        let folded = fold(&index_over(vec![operation(0x04, 0x02, 1, &[])]));
        let zero = hex32(&[0; 32]);
        assert_ne!(
            folded.version(),
            Some(zero.as_str()),
            "the label is not the version"
        );
        assert!(
            !folded.waiting().iter().any(|entry| entry.change == zero),
            "no answer carries the replay label"
        );
    }

    #[test]
    fn the_head_digest_is_blake3_over_the_bytes_it_was_given() {
        // The seam, checked against `mesh-types`' own BLAKE3 rather than against itself.
        let mut seam = Blake3Head::start();
        seam.absorb(b"mesh");
        let through_seam = seam.finish();

        let mut direct = Blake3Hasher::new();
        DigestHasher::update(&mut direct, b"mesh");
        let expected = HeadId::from_bytes(*DigestHasher::finalize(direct).as_bytes());

        assert_eq!(through_seam, expected);
    }
}
