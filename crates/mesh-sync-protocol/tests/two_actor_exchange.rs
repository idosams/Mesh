//! Two actors exchange their private state over an in-memory transport, and converge.
//!
//! # What this test is evidence of, and what it is not
//!
//! **It is evidence** that the message set, the knowledge model and the gap arithmetic in this
//! crate are sufficient to take two peers from "each holds its own work" to "each holds both",
//! with identical state on both sides, under duplication, reordering and loss.
//!
//! **It is not evidence of a network.** The transport here is a [`Vec`] of queued messages. QUIC,
//! the fallbacks, timeouts, retries and the durable outbox are `mesh-sync-engine`'s, and none of
//! them exists yet. What this shows is that when that engine is written, the protocol it carries
//! already converges.
//!
//! **It is not evidence of authenticated replication.** Both replicas run under
//! [`AuthenticationPolicy::AdmitUnverifiedPeers`], because
//! [`AuthenticationPolicy::RequireVerifiedPeers`] admits nothing on the current tree: no key
//! custody exists, so nothing in Mesh can produce a signature, so no peer can be verified. That is
//! asserted below rather than left as a footnote — `a_strict_session_replicates_nothing_today`.
//!
//! **It is not the head model.** `mesh-state` owns head advancement; this crate cannot depend on
//! it. [`Replica`] here derives a head as a digest over the applied identifier set, which is the
//! same *property* head advancement guarantees — a function of the applied causal set and of
//! nothing else — modelled locally so that convergence is checkable. Two replicas agreeing here is
//! evidence about this protocol, not a second implementation of that crate's fold.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{actor, carried, changeset, head, TestDigest};
use mesh_sync_protocol::{
    decode_message, encode_message, ActorId, ActorSequence, AuthenticationPolicy, CarriedChangeSet,
    ChangeSetId, KnowledgeSet, MerkleSummary, MessagePlane, NoAuthenticator, ReplicationGap,
    Session, SummaryDigest, SyncMessage,
};

/// One actor's replica: what it holds, what it believes its peer holds, and its session.
///
/// Immutability is deliberately *not* used here. This is the mutable edge a real engine would also
/// have — a store and a socket — and keeping it explicit keeps the pure part of the crate visibly
/// pure: every decision below is made by a function of this crate, and this struct only holds the
/// results.
struct Replica {
    identity: ActorId,
    held: BTreeMap<ChangeSetId, CarriedChangeSet>,
    pending: Vec<CarriedChangeSet>,
    knowledge: KnowledgeSet,
    belief: KnowledgeSet,
    session: Session,
    authored: u64,
}

impl Replica {
    fn new(identity: ActorId) -> Self {
        Self {
            identity,
            held: BTreeMap::new(),
            pending: Vec::new(),
            knowledge: KnowledgeSet::new(),
            belief: KnowledgeSet::new(),
            session: Session::opening(AuthenticationPolicy::AdmitUnverifiedPeers),
            authored: 0,
        }
    }

    /// The head this replica holds: a digest over the applied identifier set, in sorted order.
    ///
    /// Sorted, so it is a function of the *set* and not of arrival order — which is the property
    /// convergence rests on.
    fn head(&self) -> [u8; 32] {
        let mut digest = TestDigest::start();
        for id in self.held.keys() {
            digest.absorb(id.as_bytes());
        }
        digest.finish()
    }

    /// Which ChangeSets this replica has applied.
    fn applied(&self) -> BTreeSet<ChangeSetId> {
        self.held.keys().copied().collect()
    }

    /// Author one ChangeSet locally. Never touches the network, and never can: nothing here
    /// consults the session, the belief or a queue.
    fn author(&mut self) {
        self.authored += 1;
        let sequence = self.authored;
        let id = derive_changeset_id(self.identity, sequence);
        let mut record = carried(self.identity, sequence, id, head(0));
        record.parents = if sequence > 1 {
            vec![derive_changeset_id(self.identity, sequence - 1)]
        } else {
            Vec::new()
        };
        record.resulting_head = head(u8::try_from(sequence % 251).unwrap_or(0));
        self.apply(record);
    }

    /// Apply a ChangeSet whose parents are held, then anything the arrival unblocked.
    fn apply(&mut self, record: CarriedChangeSet) {
        if self.held.contains_key(&record.id) {
            return;
        }
        if !record
            .parents
            .iter()
            .all(|parent| self.held.contains_key(parent))
        {
            if !self.pending.iter().any(|held| held.id == record.id) {
                self.pending.push(record);
            }
            return;
        }
        self.knowledge = self.knowledge.with_changeset(
            record.author,
            record.sequence,
            record.id,
            record.resulting_head,
        );
        self.held.insert(record.id, record);
        self.drain_pending();
    }

    fn drain_pending(&mut self) {
        loop {
            let Some(index) = self.pending.iter().position(|record| {
                record
                    .parents
                    .iter()
                    .all(|parent| self.held.contains_key(parent))
            }) else {
                return;
            };
            let record = self.pending.remove(index);
            self.knowledge = self.knowledge.with_changeset(
                record.author,
                record.sequence,
                record.id,
                record.resulting_head,
            );
            self.held.insert(record.id, record);
        }
    }

    /// The messages this replica sends after receiving `message`.
    ///
    /// Every decision here is made by this crate: [`Session::admit`] decides admissibility,
    /// [`KnowledgeSet::observe`] folds the belief, [`ReplicationGap::between`] plans the requests.
    fn receive(&mut self, message: &SyncMessage) -> Vec<SyncMessage> {
        if !self.session.is_established() {
            if let Ok(next) = self.session.accept_handshake(message, &NoAuthenticator) {
                self.session = next;
                return vec![self.knowledge.advertisement()];
            }
        }
        if self.session.admit(message).is_err() {
            return Vec::new();
        }
        self.belief = self.belief.observe(message);

        match message {
            SyncMessage::AdvertiseFrontier { .. } | SyncMessage::AckOperations { .. } => {
                let gap = ReplicationGap::between(&self.knowledge, &self.belief);
                gap.requests(64, 65_536)
            }
            SyncMessage::RequestOperations {
                actor: about,
                from_sequence,
                specific,
                max_count,
            } => {
                let wanted: Vec<CarriedChangeSet> = self
                    .held
                    .values()
                    .filter(|record| {
                        (record.author == *about && record.sequence.get() > from_sequence.get())
                            || specific.contains(&record.id)
                    })
                    .take(*max_count as usize)
                    .cloned()
                    .collect();
                if wanted.is_empty() {
                    Vec::new()
                } else {
                    vec![SyncMessage::OperationsBatch {
                        changesets: sorted_by_sequence(wanted),
                    }]
                }
            }
            SyncMessage::OperationsBatch { changesets } => {
                for record in changesets {
                    self.apply(record.clone());
                }
                changesets
                    .iter()
                    .map(|record| record.author)
                    .collect::<BTreeSet<ActorId>>()
                    .into_iter()
                    .map(|about| {
                        let known = self.knowledge.actor(&about);
                        SyncMessage::AckOperations {
                            actor: about,
                            contiguous_through: known.map_or(
                                ActorSequence::NONE,
                                mesh_sync_protocol::ActorKnowledge::contiguous_through,
                            ),
                            sparse: known
                                .map(mesh_sync_protocol::ActorKnowledge::sparse)
                                .unwrap_or_default(),
                        }
                    })
                    .collect()
            }
            SyncMessage::AntiEntropySummary {
                actor: about,
                summary,
            } => {
                let mine = self.summarize(*about);
                let missing = summary.divergence(&mine);
                missing
                    .first()
                    .map(|node| SyncMessage::RequestOperations {
                        actor: *about,
                        from_sequence: ActorSequence::new(node.first().get().saturating_sub(1)),
                        specific: Vec::new(),
                        max_count: 64,
                    })
                    .into_iter()
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// This replica's summary of one actor's contiguous history.
    fn summarize(&self, about: ActorId) -> MerkleSummary {
        let ids: Vec<ChangeSetId> = self
            .held
            .values()
            .filter(|record| record.author == about)
            .map(|record| (record.sequence, record.id))
            .collect::<BTreeMap<ActorSequence, ChangeSetId>>()
            .into_values()
            .collect();
        MerkleSummary::of::<TestDigest>(ActorSequence::new(1), &ids, 2)
    }
}

/// A ChangeSet identifier that is a function of its author and sequence, so two replicas naming
/// the same ChangeSet name it identically.
fn derive_changeset_id(author: ActorId, sequence: u64) -> ChangeSetId {
    let mut digest = TestDigest::start();
    digest.absorb(author.as_bytes());
    digest.absorb(&sequence.to_be_bytes());
    ChangeSetId::from_bytes(digest.finish())
}

fn sorted_by_sequence(mut records: Vec<CarriedChangeSet>) -> Vec<CarriedChangeSet> {
    records.sort_by_key(|record| (record.author, record.sequence));
    records
}

/// One queued message, from one side to the other. The whole transport.
#[derive(Clone)]
struct Envelope {
    to_second: bool,
    message: SyncMessage,
}

/// Run the two replicas to quiescence, transforming the queue with `shape` at every step.
///
/// Every message crosses the wire as bytes: it is encoded, decoded and only then delivered, so a
/// message this crate cannot round-trip cannot make this test pass.
fn run(
    first: &mut Replica,
    second: &mut Replica,
    mut shape: impl FnMut(Vec<Envelope>) -> Vec<Envelope>,
) {
    let mut queue = vec![
        Envelope {
            to_second: true,
            message: Session::hello(first.identity, [1; 32]),
        },
        Envelope {
            to_second: false,
            message: Session::hello(second.identity, [2; 32]),
        },
    ];
    let mut steps = 0;
    while !queue.is_empty() {
        steps += 1;
        assert!(steps < 500, "the exchange did not settle");
        let shaped = shape(queue);
        queue = Vec::new();
        for envelope in shaped {
            let bytes = encode_message(&envelope.message);
            let delivered =
                decode_message(&bytes).expect("every message crosses the wire as bytes");
            let (receiver, replies_to_second) = if envelope.to_second {
                (&mut *second, false)
            } else {
                (&mut *first, true)
            };
            for reply in receiver.receive(&delivered) {
                queue.push(Envelope {
                    to_second: replies_to_second,
                    message: reply,
                });
            }
        }
    }
}

fn two_actors_with_work(first_count: u64, second_count: u64) -> (Replica, Replica) {
    let mut first = Replica::new(actor(0x11));
    let mut second = Replica::new(actor(0x22));
    for _ in 0..first_count {
        first.author();
    }
    for _ in 0..second_count {
        second.author();
    }
    (first, second)
}

#[test]
fn two_actors_converge_on_each_other_s_work() {
    let (mut ido, mut agent) = two_actors_with_work(3, 2);
    assert_ne!(ido.applied(), agent.applied());

    run(&mut ido, &mut agent, |queue| queue);

    assert_eq!(ido.applied().len(), 5);
    assert_eq!(ido.applied(), agent.applied());
    assert_eq!(ido.head(), agent.head());
}

#[test]
fn a_duplicated_stream_converges_to_the_same_state() {
    let (mut ido, mut agent) = two_actors_with_work(4, 3);
    run(&mut ido, &mut agent, |queue| {
        queue
            .iter()
            .flat_map(|one| [one.clone(), one.clone()])
            .collect()
    });

    assert_eq!(ido.applied().len(), 7);
    assert_eq!(ido.applied(), agent.applied());
    assert_eq!(ido.head(), agent.head());
}

#[test]
fn a_reordered_stream_converges_to_the_same_state() {
    let (mut clean_first, mut clean_second) = two_actors_with_work(4, 4);
    run(&mut clean_first, &mut clean_second, |queue| queue);
    let expected = clean_first.head();

    for seed in 1u64..=16 {
        let (mut ido, mut agent) = two_actors_with_work(4, 4);
        let mut state = seed;
        run(&mut ido, &mut agent, |mut queue| {
            // A deterministic shuffle: the schedule is a function of the seed alone, so a failure
            // reproduces from the seed the assertion prints.
            for index in (1..queue.len()).rev() {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                queue.swap(index, (state >> 33) as usize % (index + 1));
            }
            queue
        });
        assert_eq!(ido.applied(), agent.applied(), "seed {seed} diverged");
        assert_eq!(ido.head(), agent.head(), "seed {seed} diverged");
        assert_eq!(ido.head(), expected, "seed {seed} reached another state");
    }
}

#[test]
fn a_changeset_arriving_before_its_parent_is_held_rather_than_dropped() {
    let mut agent = Replica::new(actor(0x22));
    let ido = actor(0x11);

    let second = {
        let mut record = carried(ido, 2, derive_changeset_id(ido, 2), head(2));
        record.parents = vec![derive_changeset_id(ido, 1)];
        record
    };
    let first = carried(ido, 1, derive_changeset_id(ido, 1), head(1));

    agent.apply(second.clone());
    assert!(
        agent.applied().is_empty(),
        "the child was applied without its parent"
    );
    assert_eq!(agent.pending.len(), 1);

    agent.apply(first);
    assert_eq!(
        agent.applied().len(),
        2,
        "the buffered child was not applied"
    );
    assert!(agent.pending.is_empty());
}

#[test]
fn a_lost_batch_is_repaired_by_the_anti_entropy_summary() {
    let (mut ido, mut agent) = two_actors_with_work(5, 0);

    // Every OPERATIONS_BATCH is dropped: incremental delivery moves nothing at all.
    run(&mut ido, &mut agent, |queue| {
        queue
            .into_iter()
            .filter(|envelope| !matches!(envelope.message, SyncMessage::OperationsBatch { .. }))
            .collect()
    });
    assert!(
        agent.applied().is_empty(),
        "the loss injection did not bite"
    );

    // The sweep: Ido summarizes her history, the agent finds the divergence and asks.
    let summary = SyncMessage::AntiEntropySummary {
        actor: ido.identity,
        summary: ido.summarize(ido.identity),
    };
    let mut queue = vec![Envelope {
        to_second: true,
        message: summary,
    }];
    let mut steps = 0;
    while !queue.is_empty() {
        steps += 1;
        assert!(steps < 100, "the repair did not settle");
        let batch = std::mem::take(&mut queue);
        for envelope in batch {
            let (receiver, replies_to_second) = if envelope.to_second {
                (&mut agent, false)
            } else {
                (&mut ido, true)
            };
            for reply in receiver.receive(&envelope.message) {
                queue.push(Envelope {
                    to_second: replies_to_second,
                    message: reply,
                });
            }
        }
    }

    assert_eq!(
        agent.applied(),
        ido.applied(),
        "anti-entropy did not repair"
    );
    assert_eq!(agent.head(), ido.head());
}

#[test]
fn local_authoring_never_waits_for_a_peer() {
    let mut alone = Replica::new(actor(0x11));
    for _ in 0..64 {
        alone.author();
    }
    assert_eq!(alone.applied().len(), 64);
    assert!(
        !alone.session.is_established(),
        "no session was ever opened"
    );
    assert_eq!(
        alone.belief,
        KnowledgeSet::new(),
        "nothing was learned of a peer"
    );
}

/// The honest statement, as an assertion. Under the policy a deployment would actually want, this
/// exchange moves nothing at all today — because nothing can produce a signature.
#[test]
fn a_strict_session_replicates_nothing_today() {
    let mut ido = Replica::new(actor(0x11));
    ido.author();
    ido.session = Session::opening(AuthenticationPolicy::RequireVerifiedPeers);

    let mut agent = Replica::new(actor(0x22));
    agent.author();
    agent.session = Session::opening(AuthenticationPolicy::RequireVerifiedPeers);

    run(&mut ido, &mut agent, |queue| queue);

    assert_eq!(
        ido.applied().len(),
        1,
        "a strict session replicated something"
    );
    assert_eq!(agent.applied().len(), 1);
    assert_ne!(ido.applied(), agent.applied());
    assert!(!ido.session.is_verified());
}

#[test]
fn the_metadata_plane_carries_the_whole_exchange_and_no_chunk_bytes() {
    let (mut ido, mut agent) = two_actors_with_work(3, 3);
    let mut planes = BTreeSet::new();
    run(&mut ido, &mut agent, |queue| {
        for envelope in &queue {
            planes.insert(envelope.message.plane());
        }
        queue
    });
    assert_eq!(ido.applied(), agent.applied());
    assert!(
        !planes.contains(&MessagePlane::Content),
        "no chunk moved: {planes:?}"
    );
    assert!(planes.contains(&MessagePlane::Metadata));
    assert!(planes.contains(&MessagePlane::Handshake));
}

#[test]
fn each_side_learns_what_the_other_holds() {
    let (mut ido, mut agent) = two_actors_with_work(3, 2);
    run(&mut ido, &mut agent, |queue| queue);

    assert!(ReplicationGap::between(&ido.knowledge, &ido.belief).is_closed());
    assert!(ReplicationGap::between(&agent.knowledge, &agent.belief).is_closed());
    assert_eq!(
        ido.belief
            .actor(&agent.identity)
            .expect("the peer's own work is known")
            .contiguous_through(),
        ActorSequence::new(2)
    );
}

#[test]
fn an_unrelated_changeset_identifier_is_never_invented() {
    let (mut ido, mut agent) = two_actors_with_work(2, 2);
    run(&mut ido, &mut agent, |queue| queue);
    assert!(!ido.applied().contains(&changeset(0xff)));
    assert_eq!(ido.applied().len(), 4);
}
