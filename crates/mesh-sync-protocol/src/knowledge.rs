//! What one peer tracks about another — plan §5.4's knowledge-tracking model, as a value.
//!
//! # One type, used two ways
//!
//! A [`KnowledgeSet`] is *what some peer holds*. Held about yourself it is exact, because you can
//! see your own store. Held about a peer it is a belief, updated only by what that peer said. The
//! two are the same shape on purpose: [`crate::ReplicationGap`] is the difference between two of
//! them, and it does not care which side is which — which is what makes "what must I send you" and
//! "what must I ask you for" the same computation run in opposite directions.
//!
//! The belief is deliberately **monotone under observation**: nothing a peer says removes
//! something from what you believe it holds. A peer that has genuinely lost data says so by
//! advertising a lower contiguous sequence, and the fold takes the higher of the two, so the belief
//! stays conservative in the direction that costs a retransmission rather than in the direction
//! that loses one. Presence expiring is the one thing that goes backwards, and presence is not
//! knowledge — it never enters a knowledge set.
//!
//! # What is tracked
//!
//! Plan §5.4's list, in full: the per-actor maximum contiguous sequence, the ChangeSets held
//! beyond it, the head set, manifest availability, chunk availability, the policy epoch and the
//! canonical head.

use std::collections::{BTreeMap, BTreeSet};

use crate::ids::{
    ActorId, ActorSequence, ChangeSetId, ContentHash, HeadId, ManifestId, PolicyEpoch,
};
use crate::message::{HeadAdvertisement, SparseChangeSet, SyncMessage};

/// What a peer holds of one actor's history.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActorKnowledge {
    contiguous_through: ActorSequence,
    sparse: BTreeMap<ActorSequence, ChangeSetId>,
    head: Option<HeadId>,
}

impl ActorKnowledge {
    /// Knowing nothing about an actor.
    #[must_use]
    pub fn nothing() -> Self {
        Self::default()
    }

    /// The highest sequence for which every preceding sequence is also held.
    #[must_use]
    pub const fn contiguous_through(&self) -> ActorSequence {
        self.contiguous_through
    }

    /// What is held beyond the contiguous run, ascending by sequence.
    #[must_use]
    pub fn sparse(&self) -> Vec<SparseChangeSet> {
        self.sparse
            .iter()
            .map(|(sequence, id)| SparseChangeSet {
                sequence: *sequence,
                id: *id,
            })
            .collect()
    }

    /// The actor head this peer holds for the actor, if it has said one.
    #[must_use]
    pub const fn head(&self) -> Option<HeadId> {
        self.head
    }

    /// Whether the sequence is held, contiguously or sparsely.
    #[must_use]
    pub fn holds(&self, sequence: ActorSequence) -> bool {
        sequence != ActorSequence::NONE
            && (sequence <= self.contiguous_through || self.sparse.contains_key(&sequence))
    }

    /// This knowledge plus one ChangeSet, promoting anything the arrival made contiguous.
    ///
    /// Arriving out of order is ordinary: sequence 5 before sequence 3 leaves 5 sparse until 3 and
    /// 4 arrive, at which point all three become contiguous in one step.
    #[must_use]
    pub fn with_changeset(&self, sequence: ActorSequence, id: ChangeSetId) -> Self {
        if sequence == ActorSequence::NONE || self.holds(sequence) {
            return self.clone();
        }
        let mut sparse = self.sparse.clone();
        sparse.insert(sequence, id);
        Self {
            contiguous_through: promote(self.contiguous_through, &mut sparse),
            sparse,
            head: self.head,
        }
    }

    /// This knowledge folded with what a peer advertised, taking the more advanced of the two.
    #[must_use]
    fn merged_with(&self, advertised: &HeadAdvertisement) -> Self {
        let mut sparse = self.sparse.clone();
        for entry in &advertised.sparse {
            sparse.insert(entry.sequence, entry.id);
        }
        let contiguous = promote(
            self.contiguous_through.max(advertised.contiguous_through),
            &mut sparse,
        );
        Self {
            contiguous_through: contiguous,
            sparse,
            head: Some(advertised.head),
        }
    }

    /// This knowledge with an announced head, leaving what is held untouched.
    #[must_use]
    fn with_head(&self, head: HeadId, through: ActorSequence) -> Self {
        Self {
            contiguous_through: self.contiguous_through.max(through),
            sparse: self.sparse.clone(),
            head: Some(head),
        }
    }
}

/// Everything sequences below `contiguous` that the sparse map now makes contiguous, moved.
fn promote(
    contiguous: ActorSequence,
    sparse: &mut BTreeMap<ActorSequence, ChangeSetId>,
) -> ActorSequence {
    let mut through = contiguous;
    loop {
        sparse.retain(|sequence, _| *sequence > through);
        let next = through.next();
        if sparse.remove(&next).is_some() {
            through = next;
        } else {
            return through;
        }
    }
}

/// What one peer holds: heads, sequences, manifests, chunks, the policy epoch and the canonical
/// head. The `knowledge set` of `docs/protocol.md` §3.6, as a value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KnowledgeSet {
    actors: BTreeMap<ActorId, ActorKnowledge>,
    manifests: BTreeSet<ManifestId>,
    chunks: BTreeSet<ContentHash>,
    policy_epoch: PolicyEpoch,
    canonical_head: Option<HeadId>,
}

impl KnowledgeSet {
    /// Knowing nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What is known about one actor, or nothing.
    #[must_use]
    pub fn actor(&self, actor: &ActorId) -> Option<&ActorKnowledge> {
        self.actors.get(actor)
    }

    /// Every actor anything is known about, ascending.
    #[must_use]
    pub fn actor_ids(&self) -> Vec<ActorId> {
        self.actors.keys().copied().collect()
    }

    /// Whether a manifest is held.
    #[must_use]
    pub fn holds_manifest(&self, manifest: ManifestId) -> bool {
        self.manifests.contains(&manifest)
    }

    /// Every manifest held, ascending.
    #[must_use]
    pub fn manifests(&self) -> Vec<ManifestId> {
        self.manifests.iter().copied().collect()
    }

    /// Whether a chunk's bytes are held and verified.
    #[must_use]
    pub fn holds_chunk(&self, chunk: ContentHash) -> bool {
        self.chunks.contains(&chunk)
    }

    /// Every chunk held, ascending.
    #[must_use]
    pub fn chunks(&self) -> Vec<ContentHash> {
        self.chunks.iter().copied().collect()
    }

    /// The policy epoch this peer is operating under.
    #[must_use]
    pub const fn policy_epoch(&self) -> PolicyEpoch {
        self.policy_epoch
    }

    /// The canonical head this peer holds, if it holds one.
    #[must_use]
    pub const fn canonical_head(&self) -> Option<HeadId> {
        self.canonical_head
    }

    /// This knowledge plus one ChangeSet held for an actor, and the actor head it produces.
    #[must_use]
    pub fn with_changeset(
        &self,
        actor: ActorId,
        sequence: ActorSequence,
        id: ChangeSetId,
        resulting_head: HeadId,
    ) -> Self {
        let existing = self.actors.get(&actor).cloned().unwrap_or_default();
        let advanced = existing.with_changeset(sequence, id);
        let head = if advanced.contiguous_through() == sequence {
            Some(resulting_head)
        } else {
            advanced.head()
        };
        let mut actors = self.actors.clone();
        actors.insert(actor, ActorKnowledge { head, ..advanced });
        Self {
            actors,
            ..self.clone()
        }
    }

    /// This knowledge plus a held manifest.
    #[must_use]
    pub fn with_manifest(&self, manifest: ManifestId) -> Self {
        let mut manifests = self.manifests.clone();
        manifests.insert(manifest);
        Self {
            manifests,
            ..self.clone()
        }
    }

    /// This knowledge plus a chunk whose bytes are held and verified.
    #[must_use]
    pub fn with_chunk(&self, chunk: ContentHash) -> Self {
        let mut chunks = self.chunks.clone();
        chunks.insert(chunk);
        Self {
            chunks,
            ..self.clone()
        }
    }

    /// This knowledge with a canonical head and the epoch it advanced under.
    #[must_use]
    pub fn with_canonical_head(&self, head: HeadId, policy_epoch: PolicyEpoch) -> Self {
        Self {
            canonical_head: Some(head),
            policy_epoch: self.policy_epoch.max(policy_epoch),
            ..self.clone()
        }
    }

    /// The advertisement this knowledge produces: one entry per actor, sorted, plus the canonical
    /// head and policy epoch.
    ///
    /// The result always satisfies [`SyncMessage::check`], because the sorting the message
    /// requires is the ordering a [`std::collections::BTreeMap`] already has.
    #[must_use]
    pub fn advertisement(&self) -> SyncMessage {
        SyncMessage::AdvertiseFrontier {
            heads: self
                .actors
                .iter()
                .filter_map(|(actor, known)| {
                    known.head().map(|head| HeadAdvertisement {
                        actor: *actor,
                        head,
                        contiguous_through: known.contiguous_through(),
                        sparse: known.sparse(),
                    })
                })
                .collect(),
            canonical_head: self.canonical_head,
            policy_epoch: self.policy_epoch,
        }
    }

    /// This knowledge folded with one message received from the peer it is about.
    ///
    /// Every message that says something about what the sender holds is folded; the rest return an
    /// unchanged value rather than being an error, because a peer is free to send a message that
    /// happens to teach nothing.
    ///
    /// `PRESENCE` teaches nothing on purpose: presence is ephemeral and TTL-bounded, and a
    /// knowledge set has no expiry, so folding it in would make a permanent record out of a signal
    /// that is allowed to be lost.
    #[must_use]
    pub fn observe(&self, message: &SyncMessage) -> Self {
        match message {
            SyncMessage::AdvertiseFrontier {
                heads,
                canonical_head,
                policy_epoch,
            } => {
                let mut actors = self.actors.clone();
                for advertised in heads {
                    let existing = actors.get(&advertised.actor).cloned().unwrap_or_default();
                    actors.insert(advertised.actor, existing.merged_with(advertised));
                }
                Self {
                    actors,
                    canonical_head: canonical_head.or(self.canonical_head),
                    policy_epoch: self.policy_epoch.max(*policy_epoch),
                    ..self.clone()
                }
            }
            SyncMessage::AckOperations {
                actor,
                contiguous_through,
                sparse,
            } => {
                let existing = self.actors.get(actor).cloned().unwrap_or_default();
                let head = existing.head();
                let mut folded = existing.merged_with(&HeadAdvertisement {
                    actor: *actor,
                    head: head.unwrap_or_else(|| HeadId::from_bytes([0; 32])),
                    contiguous_through: *contiguous_through,
                    sparse: sparse.clone(),
                });
                folded.head = head;
                let mut actors = self.actors.clone();
                actors.insert(*actor, folded);
                Self {
                    actors,
                    ..self.clone()
                }
            }
            SyncMessage::RequestOperations {
                actor,
                from_sequence,
                ..
            } => {
                // A request is evidence too: a peer that asks from sequence n holds n.
                let existing = self.actors.get(actor).cloned().unwrap_or_default();
                let mut sparse = existing.sparse.clone();
                let contiguous =
                    promote(existing.contiguous_through.max(*from_sequence), &mut sparse);
                let mut actors = self.actors.clone();
                actors.insert(
                    *actor,
                    ActorKnowledge {
                        contiguous_through: contiguous,
                        sparse,
                        head: existing.head,
                    },
                );
                Self {
                    actors,
                    ..self.clone()
                }
            }
            SyncMessage::OperationsBatch { changesets } => {
                changesets.iter().fold(self.clone(), |known, carried| {
                    known.with_changeset(
                        carried.author,
                        carried.sequence,
                        carried.id,
                        carried.resulting_head,
                    )
                })
            }
            SyncMessage::UpdateActorHead {
                actor,
                head,
                sequence,
            } => {
                let existing = self.actors.get(actor).cloned().unwrap_or_default();
                let mut actors = self.actors.clone();
                actors.insert(*actor, existing.with_head(*head, *sequence));
                Self {
                    actors,
                    ..self.clone()
                }
            }
            SyncMessage::UpdateCanonicalHead {
                head, policy_epoch, ..
            } => self.with_canonical_head(*head, *policy_epoch),
            SyncMessage::AdvertiseManifests { manifests } => {
                let mut held = self.manifests.clone();
                held.extend(manifests.iter().copied());
                Self {
                    manifests: held,
                    ..self.clone()
                }
            }
            SyncMessage::AckChunks { verified } => {
                let mut held = self.chunks.clone();
                held.extend(verified.iter().copied());
                Self {
                    chunks: held,
                    ..self.clone()
                }
            }
            SyncMessage::ChunkBatch { parts } => {
                // The sender demonstrably holds what it just sent, but only a completed chunk is
                // a chunk: a part is not the bytes the content hash names.
                let mut held = self.chunks.clone();
                held.extend(
                    parts
                        .iter()
                        .filter(|part| part.is_final)
                        .map(|part| part.content),
                );
                Self {
                    chunks: held,
                    ..self.clone()
                }
            }
            SyncMessage::Hello { .. }
            | SyncMessage::Authenticate { .. }
            | SyncMessage::RequestChunks { .. }
            | SyncMessage::Presence { .. }
            | SyncMessage::ReviewBundle { .. }
            | SyncMessage::ValidationReceipt { .. }
            | SyncMessage::ApprovalEnvelope { .. }
            | SyncMessage::AntiEntropySummary { .. }
            | SyncMessage::Error(_) => self.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(byte: u8) -> ActorId {
        ActorId::from_bytes([byte; 32])
    }

    fn changeset(byte: u8) -> ChangeSetId {
        ChangeSetId::from_bytes([byte; 32])
    }

    fn head(byte: u8) -> HeadId {
        HeadId::from_bytes([byte; 32])
    }

    #[test]
    fn knowing_nothing_knows_nothing() {
        let empty = KnowledgeSet::new();
        assert!(empty.actor_ids().is_empty());
        assert_eq!(empty.canonical_head(), None);
        assert_eq!(empty.policy_epoch(), PolicyEpoch::INITIAL);
        assert!(empty.actor(&actor(1)).is_none());
    }

    #[test]
    fn a_contiguous_run_advances_one_at_a_time() {
        let known = KnowledgeSet::new()
            .with_changeset(actor(1), ActorSequence::new(1), changeset(1), head(1))
            .with_changeset(actor(1), ActorSequence::new(2), changeset(2), head(2));
        let about = known.actor(&actor(1)).expect("actor is known");
        assert_eq!(about.contiguous_through(), ActorSequence::new(2));
        assert_eq!(about.head(), Some(head(2)));
        assert!(about.sparse().is_empty());
    }

    #[test]
    fn an_out_of_order_arrival_stays_sparse_until_the_hole_closes() {
        let with_hole = KnowledgeSet::new()
            .with_changeset(actor(1), ActorSequence::new(1), changeset(1), head(1))
            .with_changeset(actor(1), ActorSequence::new(3), changeset(3), head(3));
        let about = with_hole.actor(&actor(1)).expect("actor is known");
        assert_eq!(about.contiguous_through(), ActorSequence::new(1));
        assert_eq!(about.sparse().len(), 1);
        assert_eq!(about.head(), Some(head(1)));
        assert!(about.holds(ActorSequence::new(3)));

        let closed =
            with_hole.with_changeset(actor(1), ActorSequence::new(2), changeset(2), head(2));
        let about = closed.actor(&actor(1)).expect("actor is known");
        assert_eq!(about.contiguous_through(), ActorSequence::new(3));
        assert!(about.sparse().is_empty());
    }

    #[test]
    fn recording_the_same_changeset_twice_changes_nothing() {
        let once = KnowledgeSet::new().with_changeset(
            actor(1),
            ActorSequence::new(1),
            changeset(1),
            head(1),
        );
        let twice = once.with_changeset(actor(1), ActorSequence::new(1), changeset(1), head(1));
        assert_eq!(once, twice);
    }

    #[test]
    fn sequence_zero_is_never_recorded() {
        let known = KnowledgeSet::new().with_changeset(
            actor(1),
            ActorSequence::NONE,
            changeset(1),
            head(1),
        );
        let about = known.actor(&actor(1)).expect("the actor row exists");
        assert_eq!(about.contiguous_through(), ActorSequence::NONE);
        assert!(!about.holds(ActorSequence::NONE));
    }

    #[test]
    fn an_advertisement_round_trips_into_the_same_belief() {
        let mine = KnowledgeSet::new()
            .with_changeset(actor(1), ActorSequence::new(1), changeset(1), head(1))
            .with_changeset(actor(1), ActorSequence::new(3), changeset(3), head(3))
            .with_canonical_head(head(9), PolicyEpoch::new(4));
        let belief = KnowledgeSet::new().observe(&mine.advertisement());

        let about = belief.actor(&actor(1)).expect("actor is known");
        assert_eq!(about.contiguous_through(), ActorSequence::new(1));
        assert_eq!(about.sparse().len(), 1);
        assert_eq!(belief.canonical_head(), Some(head(9)));
        assert_eq!(belief.policy_epoch(), PolicyEpoch::new(4));
    }

    #[test]
    fn an_advertisement_this_crate_produces_always_passes_its_own_precondition() {
        let mine = KnowledgeSet::new()
            .with_changeset(actor(9), ActorSequence::new(1), changeset(1), head(1))
            .with_changeset(actor(1), ActorSequence::new(1), changeset(2), head(2))
            .with_changeset(actor(1), ActorSequence::new(4), changeset(4), head(4));
        assert_eq!(mine.advertisement().check(), Ok(()));
    }

    #[test]
    fn a_belief_never_goes_backwards_under_a_stale_advertisement() {
        let believed = KnowledgeSet::new()
            .with_changeset(actor(1), ActorSequence::new(1), changeset(1), head(1))
            .with_changeset(actor(1), ActorSequence::new(2), changeset(2), head(2));
        let stale = SyncMessage::AdvertiseFrontier {
            heads: vec![HeadAdvertisement {
                actor: actor(1),
                head: head(1),
                contiguous_through: ActorSequence::new(1),
                sparse: Vec::new(),
            }],
            canonical_head: None,
            policy_epoch: PolicyEpoch::INITIAL,
        };
        let after = believed.observe(&stale);
        assert_eq!(
            after
                .actor(&actor(1))
                .expect("actor is known")
                .contiguous_through(),
            ActorSequence::new(2)
        );
    }

    #[test]
    fn a_request_teaches_what_the_requester_already_holds() {
        let belief = KnowledgeSet::new().observe(&SyncMessage::RequestOperations {
            actor: actor(1),
            from_sequence: ActorSequence::new(7),
            specific: Vec::new(),
            max_count: 10,
        });
        assert_eq!(
            belief
                .actor(&actor(1))
                .expect("actor is known")
                .contiguous_through(),
            ActorSequence::new(7)
        );
    }

    #[test]
    fn presence_never_enters_a_knowledge_set() {
        let before = KnowledgeSet::new();
        let after = before.observe(&SyncMessage::Presence {
            actor: actor(1),
            state: crate::PresenceState::Active,
            expires_after_millis: 30_000,
        });
        assert_eq!(before, after);
    }

    #[test]
    fn an_acknowledgement_does_not_invent_a_head() {
        let belief = KnowledgeSet::new().observe(&SyncMessage::AckOperations {
            actor: actor(1),
            contiguous_through: ActorSequence::new(2),
            sparse: Vec::new(),
        });
        let about = belief.actor(&actor(1)).expect("actor is known");
        assert_eq!(about.contiguous_through(), ActorSequence::new(2));
        assert_eq!(about.head(), None);
    }

    #[test]
    fn only_a_completed_chunk_counts_as_held() {
        let belief = KnowledgeSet::new().observe(&SyncMessage::ChunkBatch {
            parts: vec![
                crate::ChunkPart {
                    content: ContentHash::from_bytes([1; 32]),
                    offset: 0,
                    bytes: vec![1, 2, 3],
                    is_final: false,
                },
                crate::ChunkPart {
                    content: ContentHash::from_bytes([2; 32]),
                    offset: 0,
                    bytes: vec![4, 5, 6],
                    is_final: true,
                },
            ],
        });
        assert!(!belief.holds_chunk(ContentHash::from_bytes([1; 32])));
        assert!(belief.holds_chunk(ContentHash::from_bytes([2; 32])));
    }

    #[test]
    fn manifests_and_chunks_are_tracked_separately() {
        let known = KnowledgeSet::new()
            .with_manifest(ManifestId::from_bytes([1; 32]))
            .with_chunk(ContentHash::from_bytes([1; 32]));
        assert!(known.holds_manifest(ManifestId::from_bytes([1; 32])));
        assert!(known.holds_chunk(ContentHash::from_bytes([1; 32])));
        assert!(!known.holds_manifest(ManifestId::from_bytes([2; 32])));
        assert_eq!(known.manifests().len(), 1);
        assert_eq!(known.chunks().len(), 1);
    }

    #[test]
    fn observing_a_message_that_teaches_nothing_returns_an_equal_value() {
        let known = KnowledgeSet::new().with_manifest(ManifestId::from_bytes([1; 32]));
        for message in [
            SyncMessage::Hello {
                protocol_version: crate::PROTOCOL_VERSION,
                encoding_profile: crate::RECORD_ENCODING_PROFILE.to_owned(),
                actor: actor(1),
                challenge: [0; 32],
            },
            SyncMessage::RequestChunks {
                requests: Vec::new(),
            },
        ] {
            assert_eq!(known.observe(&message), known);
        }
    }
}
