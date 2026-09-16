//! The difference between two knowledge sets, and the requests that close it.
//!
//! # One computation, run in two directions
//!
//! [`ReplicationGap::between`] answers *what does the first side lack that the second side has*.
//! Run it as `between(mine, theirs)` and it tells you what to ask for. Run it as
//! `between(theirs, mine)` and it tells you what to send. There is no second implementation for
//! the sending side, which is why the two can never disagree about what "behind" means.
//!
//! # It plans; it does not deliver
//!
//! A gap names sequences, ChangeSets, manifests and chunks. It never says when to send, how often
//! to retry, or what to do when a request goes unanswered — that is the transport's, and the
//! transport is `mesh-sync-engine`. Keeping the plan pure is what lets a test drive two actors to
//! convergence over a channel that is a [`std::vec::Vec`].

use crate::ids::{ActorId, ActorSequence, ChangeSetId, ContentHash, ManifestId};
use crate::knowledge::KnowledgeSet;
use crate::message::{ChunkRequest, SyncMessage};

/// What one side lacks of one actor's history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorGap {
    /// The actor whose history is behind.
    pub actor: ActorId,
    /// The sequence already held contiguously; the answer starts after it.
    pub from_sequence: ActorSequence,
    /// How far the other side's contiguous run reaches, so a caller can size the request.
    pub through_sequence: ActorSequence,
    /// ChangeSets the other side holds beyond its contiguous run that this side does not, sorted.
    pub specific: Vec<ChangeSetId>,
}

impl ActorGap {
    /// Whether this actor's history is level between the two sides.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.from_sequence >= self.through_sequence && self.specific.is_empty()
    }
}

/// Everything one side lacks that another side has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplicationGap {
    actors: Vec<ActorGap>,
    manifests: Vec<ManifestId>,
    chunks: Vec<ContentHash>,
}

impl ReplicationGap {
    /// What `behind` lacks that `ahead` holds.
    #[must_use]
    pub fn between(behind: &KnowledgeSet, ahead: &KnowledgeSet) -> Self {
        let mut actors = Vec::new();
        for actor in ahead.actor_ids() {
            let Some(theirs) = ahead.actor(&actor) else {
                continue;
            };
            let mine = behind.actor(&actor);
            let from_sequence =
                mine.map_or(ActorSequence::NONE, |known| known.contiguous_through());
            let specific: Vec<ChangeSetId> = theirs
                .sparse()
                .into_iter()
                .filter(|entry| !mine.is_some_and(|known| known.holds(entry.sequence)))
                .map(|entry| entry.id)
                .collect();
            let gap = ActorGap {
                actor,
                from_sequence,
                through_sequence: theirs.contiguous_through(),
                specific: sorted(specific),
            };
            if !gap.is_closed() {
                actors.push(gap);
            }
        }

        Self {
            actors,
            manifests: ahead
                .manifests()
                .into_iter()
                .filter(|manifest| !behind.holds_manifest(*manifest))
                .collect(),
            chunks: ahead
                .chunks()
                .into_iter()
                .filter(|chunk| !behind.holds_chunk(*chunk))
                .collect(),
        }
    }

    /// The per-actor history gaps, ascending by actor.
    #[must_use]
    pub fn actors(&self) -> &[ActorGap] {
        &self.actors
    }

    /// The manifests the other side holds and this one does not, ascending.
    #[must_use]
    pub fn manifests(&self) -> &[ManifestId] {
        &self.manifests
    }

    /// The chunks the other side holds and this one does not, ascending.
    #[must_use]
    pub fn chunks(&self) -> &[ContentHash] {
        &self.chunks
    }

    /// Whether there is nothing left to move.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.actors.is_empty() && self.manifests.is_empty() && self.chunks.is_empty()
    }

    /// The messages that close this gap: one `REQUEST_OPERATIONS` per actor behind, then one
    /// `REQUEST_CHUNKS` for the content.
    ///
    /// Metadata requests come first and content requests last, which is the plane separation as an
    /// ordering: a peer learns *what* changed before it spends bandwidth on the bytes. Every
    /// message returned satisfies [`SyncMessage::check`].
    ///
    /// `max_count` bounds each operations request; zero is treated as one, because a request for
    /// nothing is not a request.
    #[must_use]
    pub fn requests(&self, max_count: u32, max_chunk_bytes: u64) -> Vec<SyncMessage> {
        let count = max_count.max(1);
        let bytes = max_chunk_bytes.max(1);
        let mut messages: Vec<SyncMessage> = self
            .actors
            .iter()
            .map(|gap| SyncMessage::RequestOperations {
                actor: gap.actor,
                from_sequence: gap.from_sequence,
                specific: gap.specific.clone(),
                max_count: count,
            })
            .collect();
        if !self.chunks.is_empty() {
            messages.push(SyncMessage::RequestChunks {
                requests: self
                    .chunks
                    .iter()
                    .map(|content| ChunkRequest {
                        content: *content,
                        from_offset: 0,
                        max_bytes: bytes,
                    })
                    .collect(),
            });
        }
        messages
    }
}

/// Sorted, with duplicates removed, because every identifier list on the wire is.
fn sorted<T: Ord>(mut values: Vec<T>) -> Vec<T> {
    values.sort_unstable();
    values.dedup();
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{HeadId, PolicyEpoch};

    fn actor(byte: u8) -> ActorId {
        ActorId::from_bytes([byte; 32])
    }

    fn changeset(byte: u8) -> ChangeSetId {
        ChangeSetId::from_bytes([byte; 32])
    }

    fn head(byte: u8) -> HeadId {
        HeadId::from_bytes([byte; 32])
    }

    fn history(count: u8) -> KnowledgeSet {
        (1..=count).fold(KnowledgeSet::new(), |known, step| {
            known.with_changeset(
                actor(1),
                ActorSequence::new(u64::from(step)),
                changeset(step),
                head(step),
            )
        })
    }

    #[test]
    fn two_peers_with_the_same_knowledge_have_no_gap() {
        let gap = ReplicationGap::between(&history(3), &history(3));
        assert!(gap.is_closed());
        assert!(gap.requests(8, 1024).is_empty());
    }

    #[test]
    fn a_peer_that_is_behind_asks_from_where_it_stopped() {
        let gap = ReplicationGap::between(&history(2), &history(5));
        assert_eq!(gap.actors().len(), 1);
        assert_eq!(gap.actors()[0].from_sequence, ActorSequence::new(2));
        assert_eq!(gap.actors()[0].through_sequence, ActorSequence::new(5));

        let requests = gap.requests(8, 1024);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].check(), Ok(()));
        match &requests[0] {
            SyncMessage::RequestOperations {
                from_sequence,
                max_count,
                ..
            } => {
                assert_eq!(*from_sequence, ActorSequence::new(2));
                assert_eq!(*max_count, 8);
            }
            other => panic!("expected REQUEST_OPERATIONS, got {}", other.kind()),
        }
    }

    #[test]
    fn the_direction_decides_who_is_behind() {
        let ahead = history(5);
        let behind = history(2);
        assert!(!ReplicationGap::between(&behind, &ahead).is_closed());
        assert!(ReplicationGap::between(&ahead, &behind).is_closed());
    }

    #[test]
    fn a_sparse_changeset_the_other_side_lacks_is_asked_for_by_identifier() {
        let ahead =
            history(2).with_changeset(actor(1), ActorSequence::new(9), changeset(9), head(9));
        let gap = ReplicationGap::between(&history(2), &ahead);
        assert_eq!(gap.actors().len(), 1);
        assert_eq!(gap.actors()[0].specific, vec![changeset(9)]);
        assert_eq!(gap.requests(4, 1024)[0].check(), Ok(()));
    }

    #[test]
    fn a_sparse_changeset_the_other_side_already_holds_is_not_asked_for() {
        let sparse =
            history(2).with_changeset(actor(1), ActorSequence::new(9), changeset(9), head(9));
        assert!(ReplicationGap::between(&sparse, &sparse).is_closed());
    }

    #[test]
    fn content_is_requested_after_metadata() {
        let ahead = history(3)
            .with_manifest(ManifestId::from_bytes([1; 32]))
            .with_chunk(ContentHash::from_bytes([1; 32]));
        let gap = ReplicationGap::between(&KnowledgeSet::new(), &ahead);
        assert_eq!(gap.manifests().len(), 1);
        assert_eq!(gap.chunks().len(), 1);

        let requests = gap.requests(8, 1024);
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].kind().plane(), crate::MessagePlane::Metadata);
        assert_eq!(requests[1].kind().plane(), crate::MessagePlane::Content);
        for request in &requests {
            assert_eq!(request.check(), Ok(()));
        }
    }

    #[test]
    fn a_zero_bound_still_produces_a_well_formed_request() {
        let gap = ReplicationGap::between(&KnowledgeSet::new(), &history(1));
        for request in gap.requests(0, 0) {
            assert_eq!(request.check(), Ok(()));
        }
    }

    /// The canonical head and policy epoch are knowledge, not a gap: a peer learns them from an
    /// advertisement rather than requesting them, so they never appear in the request plan.
    #[test]
    fn a_canonical_head_difference_produces_no_request() {
        let ahead = KnowledgeSet::new().with_canonical_head(head(9), PolicyEpoch::new(2));
        let gap = ReplicationGap::between(&KnowledgeSet::new(), &ahead);
        assert!(gap.is_closed());
    }
}
