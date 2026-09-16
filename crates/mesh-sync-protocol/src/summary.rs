//! Merkle summaries, so two peers can find where their histories differ without sending them.
//!
//! # The shape, and why it is this shape
//!
//! Incremental delivery is what normally moves work between peers. Anti-entropy is the periodic
//! sweep that repairs what incremental delivery missed — a batch lost while a peer was offline, a
//! request whose answer never arrived. The naive sweep sends every identifier and compares; over a
//! long history that costs more than the repair.
//!
//! A summary is therefore **two levels**: a leaf per fixed-width run of one actor's sequence,
//! holding a digest of the ChangeSet identifiers in that run, and a root over the leaves. Two
//! peers compare roots in one round trip; if the roots differ they compare leaves and learn
//! exactly which sequence ranges to ask about. A deeper tree is a refinement of the same shape and
//! the protocol does not fix the depth, because the peer that *builds* the summary chooses the
//! run width and sends the leaves it built.
//!
//! # The digest is a seam, and this crate ships no implementation
//!
//! [`SummaryDigest`] names an algorithm; it does not supply one. The protocol digest is BLAKE3 and
//! `mesh-types` implements it, which this crate cannot depend on (see [`crate::ids`]). Shipping a
//! second, weaker digest here so that the type would be usable out of the box would mean a
//! composition root could pick the wrong one without noticing — so there is nothing to pick.

use crate::ids::{ActorSequence, ChangeSetId};

/// A thirty-two-byte digest algorithm, incremental.
///
/// The composition root supplies BLAKE3. Nothing in this crate does, on purpose.
pub trait SummaryDigest {
    /// Empty digest state.
    fn start() -> Self;

    /// Absorb bytes.
    fn absorb(&mut self, bytes: &[u8]);

    /// The digest of everything absorbed.
    fn finish(self) -> [u8; 32];
}

/// The domain this crate absorbs first when summarizing, so a summary digest can never collide
/// with a digest of the same bytes taken for another purpose.
pub const SUMMARY_DOMAIN: &str = "mesh.v0.anti-entropy-summary";

/// One leaf: a contiguous run of one actor's sequence, and the digest of what it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SummaryNode {
    first: ActorSequence,
    last: ActorSequence,
    digest: [u8; 32],
}

impl SummaryNode {
    /// A leaf covering `first..=last` with `digest`.
    #[must_use]
    pub const fn new(first: ActorSequence, last: ActorSequence, digest: [u8; 32]) -> Self {
        Self {
            first,
            last,
            digest,
        }
    }

    /// The first sequence in the run.
    #[must_use]
    pub const fn first(&self) -> ActorSequence {
        self.first
    }

    /// The last sequence in the run, inclusive.
    #[must_use]
    pub const fn last(&self) -> ActorSequence {
        self.last
    }

    /// The digest of the ChangeSet identifiers in the run, in sequence order.
    #[must_use]
    pub const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Whether the run holds at least one sequence and does not run backwards.
    #[must_use]
    pub const fn is_well_formed(&self) -> bool {
        self.first.get() >= 1 && self.first.get() <= self.last.get()
    }
}

/// A summary of one actor's history: leaves over contiguous sequence runs.
///
/// Built by [`MerkleSummary::of`], carried by `ANTI_ENTROPY_SUMMARY`, and compared by
/// [`MerkleSummary::divergence`].
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MerkleSummary {
    nodes: Vec<SummaryNode>,
}

impl MerkleSummary {
    /// The summary of a history that holds nothing.
    #[must_use]
    pub const fn empty() -> Self {
        Self { nodes: Vec::new() }
    }

    /// A summary of leaves somebody else built — a decoder, or a store that holds them already.
    ///
    /// It does not check them: a decoded summary that is not well formed must reach
    /// [`MerkleSummary::is_well_formed`] as a *value*, so the receiver refuses it with a protocol
    /// error naming the sender, rather than the decoder silently rejecting bytes that were exactly
    /// what the sender meant to send.
    #[must_use]
    pub fn from_nodes(nodes: Vec<SummaryNode>) -> Self {
        Self { nodes }
    }

    /// Summarize `ids` — one actor's ChangeSets in sequence order, starting at sequence `first` —
    /// in runs of `run_width`.
    ///
    /// A `run_width` of zero would produce no leaves for a non-empty history, which reads as a
    /// peer that holds nothing, so it is treated as one.
    #[must_use]
    pub fn of<D: SummaryDigest>(
        first: ActorSequence,
        ids: &[ChangeSetId],
        run_width: usize,
    ) -> Self {
        let width = run_width.max(1);
        let mut nodes = Vec::with_capacity(ids.len().div_ceil(width));
        for (index, run) in ids.chunks(width).enumerate() {
            let offset = (index * width) as u64;
            let run_first = ActorSequence::new(first.get().saturating_add(offset));
            let run_last = ActorSequence::new(run_first.get().saturating_add(run.len() as u64 - 1));
            let mut digest = D::start();
            digest.absorb(SUMMARY_DOMAIN.as_bytes());
            digest.absorb(&run_first.get().to_be_bytes());
            for id in run {
                digest.absorb(id.as_bytes());
            }
            nodes.push(SummaryNode::new(run_first, run_last, digest.finish()));
        }
        Self { nodes }
    }

    /// The leaves, in ascending sequence order.
    #[must_use]
    pub fn nodes(&self) -> &[SummaryNode] {
        &self.nodes
    }

    /// The root over the leaves: one digest two peers can compare in a single round trip.
    #[must_use]
    pub fn root<D: SummaryDigest>(&self) -> [u8; 32] {
        let mut digest = D::start();
        digest.absorb(SUMMARY_DOMAIN.as_bytes());
        digest.absorb(&(self.nodes.len() as u64).to_be_bytes());
        for node in &self.nodes {
            digest.absorb(&node.first().get().to_be_bytes());
            digest.absorb(&node.last().get().to_be_bytes());
            digest.absorb(node.digest());
        }
        digest.finish()
    }

    /// Whether the leaves are non-empty runs, ascending and contiguous.
    ///
    /// A summary with a hole in it would let a peer conclude that a range it never received is a
    /// range that agrees, which is the one wrong answer anti-entropy must not give.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        let mut previous: Option<ActorSequence> = None;
        for node in &self.nodes {
            if !node.is_well_formed() {
                return false;
            }
            if let Some(end) = previous {
                if node.first().get() != end.get().saturating_add(1) {
                    return false;
                }
            }
            previous = Some(node.last());
        }
        true
    }

    /// The runs this summary and `other` disagree about, from this peer's point of view.
    ///
    /// A run is returned when the other peer holds the same range with a different digest, or does
    /// not hold that range at all. The result is what a peer turns into `REQUEST_OPERATIONS` — or,
    /// when the runs it returns are this peer's own, into what it must *send*.
    #[must_use]
    pub fn divergence(&self, other: &Self) -> Vec<SummaryNode> {
        self.nodes
            .iter()
            .filter(|node| !other.agrees_about(node))
            .copied()
            .collect()
    }

    /// Whether `other` holds exactly this run with this digest.
    fn agrees_about(&self, node: &SummaryNode) -> bool {
        self.nodes.iter().any(|mine| mine == node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A digest for the tests alone: FNV-1a widened to thirty-two bytes. It is not a protocol
    /// digest and never leaves this module — the seam exists so the composition root supplies
    /// BLAKE3, and a test needs only that two identical inputs agree and two different ones do not.
    struct TestDigest(u128);

    impl SummaryDigest for TestDigest {
        fn start() -> Self {
            Self(0x6c62_272e_07bb_0142_62b8_2175_6295_c58d)
        }

        fn absorb(&mut self, bytes: &[u8]) {
            for byte in bytes {
                self.0 = (self.0 ^ u128::from(*byte)).wrapping_mul(0x0100_0000_0000_0000_0000_013b);
            }
        }

        fn finish(self) -> [u8; 32] {
            let mut out = [0u8; 32];
            out[..16].copy_from_slice(&self.0.to_be_bytes());
            out[16..].copy_from_slice(&self.0.rotate_left(37).to_be_bytes());
            out
        }
    }

    fn ids(count: u8) -> Vec<ChangeSetId> {
        (1..=count)
            .map(|byte| ChangeSetId::from_bytes([byte; 32]))
            .collect()
    }

    #[test]
    fn a_summary_covers_every_sequence_it_was_given() {
        let summary = MerkleSummary::of::<TestDigest>(ActorSequence::new(1), &ids(7), 3);
        assert!(summary.is_well_formed());
        assert_eq!(summary.nodes().len(), 3);
        assert_eq!(summary.nodes()[0].first(), ActorSequence::new(1));
        assert_eq!(summary.nodes()[0].last(), ActorSequence::new(3));
        assert_eq!(summary.nodes()[2].first(), ActorSequence::new(7));
        assert_eq!(summary.nodes()[2].last(), ActorSequence::new(7));
    }

    #[test]
    fn two_peers_holding_the_same_history_agree_on_the_root() {
        let mine = MerkleSummary::of::<TestDigest>(ActorSequence::new(1), &ids(9), 4);
        let theirs = MerkleSummary::of::<TestDigest>(ActorSequence::new(1), &ids(9), 4);
        assert_eq!(mine.root::<TestDigest>(), theirs.root::<TestDigest>());
        assert!(mine.divergence(&theirs).is_empty());
    }

    #[test]
    fn a_peer_that_is_behind_diverges_exactly_on_what_it_lacks() {
        let mine = MerkleSummary::of::<TestDigest>(ActorSequence::new(1), &ids(9), 3);
        let theirs = MerkleSummary::of::<TestDigest>(ActorSequence::new(1), &ids(6), 3);
        let missing = mine.divergence(&theirs);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].first(), ActorSequence::new(7));
        assert_ne!(mine.root::<TestDigest>(), theirs.root::<TestDigest>());
    }

    #[test]
    fn one_changed_identifier_makes_exactly_its_run_diverge() {
        let mut altered = ids(9);
        altered[4] = ChangeSetId::from_bytes([0xff; 32]);
        let mine = MerkleSummary::of::<TestDigest>(ActorSequence::new(1), &ids(9), 3);
        let theirs = MerkleSummary::of::<TestDigest>(ActorSequence::new(1), &altered, 3);
        let differing = mine.divergence(&theirs);
        assert_eq!(differing.len(), 1);
        assert_eq!(differing[0].first(), ActorSequence::new(4));
    }

    #[test]
    fn a_run_width_of_zero_summarizes_every_sequence_rather_than_none() {
        let summary = MerkleSummary::of::<TestDigest>(ActorSequence::new(1), &ids(3), 0);
        assert_eq!(summary.nodes().len(), 3);
        assert!(summary.is_well_formed());
    }

    #[test]
    fn an_empty_history_summarizes_to_an_empty_well_formed_summary() {
        let summary = MerkleSummary::of::<TestDigest>(ActorSequence::new(1), &[], 4);
        assert_eq!(summary, MerkleSummary::empty());
        assert!(summary.is_well_formed());
        assert!(summary.nodes().is_empty());
    }

    #[test]
    fn a_summary_with_a_hole_is_not_well_formed() {
        let holed = MerkleSummary {
            nodes: vec![
                SummaryNode::new(ActorSequence::new(1), ActorSequence::new(2), [0; 32]),
                SummaryNode::new(ActorSequence::new(5), ActorSequence::new(6), [1; 32]),
            ],
        };
        assert!(!holed.is_well_formed());
    }

    #[test]
    fn a_run_that_starts_at_sequence_zero_is_not_well_formed() {
        let zeroed = MerkleSummary {
            nodes: vec![SummaryNode::new(
                ActorSequence::NONE,
                ActorSequence::new(2),
                [0; 32],
            )],
        };
        assert!(!zeroed.is_well_formed());
    }

    #[test]
    fn a_run_that_runs_backwards_is_not_well_formed() {
        let backwards = MerkleSummary {
            nodes: vec![SummaryNode::new(
                ActorSequence::new(4),
                ActorSequence::new(2),
                [0; 32],
            )],
        };
        assert!(!backwards.is_well_formed());
    }

    /// The domain tag is absorbed first, so summarizing the same identifiers under another purpose
    /// could not produce the same digest by accident.
    #[test]
    fn the_domain_is_absorbed_before_anything_else() {
        let mut with_domain = TestDigest::start();
        with_domain.absorb(SUMMARY_DOMAIN.as_bytes());
        with_domain.absorb(&1u64.to_be_bytes());
        with_domain.absorb(ChangeSetId::from_bytes([1; 32]).as_bytes());

        let summary = MerkleSummary::of::<TestDigest>(ActorSequence::new(1), &ids(1), 1);
        assert_eq!(summary.nodes()[0].digest(), &with_domain.finish());
    }
}
