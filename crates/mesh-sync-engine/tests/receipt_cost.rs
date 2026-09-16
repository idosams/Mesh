//! What the receipt check costs per record, measured rather than assumed.
//!
//! ADR-0015's Consequences say the plain part out loud: *"Every peer now pays a digest over every
//! received ChangeSet's canonical bytes before its identifier may be used. That is a real cost on
//! the metadata plane's hot path and it has no budget yet — plan §12.3's synchronization metrics do
//! not have a line for it."* This file is the measurement that turns "no budget yet" into a number
//! somebody can write a budget against.
//!
//! It is `#[ignore]`d, deliberately. A timing assertion inside `npm test` is a flaky test on a
//! shared machine, and a number that varies with the machine is not a gate — it is evidence, and
//! evidence is reported with the machine it came from. Run it, and put the output in the PR body:
//!
//! ```console
//! $ cargo test -p mesh-sync-engine --test receipt_cost --release -- --ignored --nocapture
//! ```
//!
//! **What it does not establish.** It measures `admit` in isolation on one machine with a warm
//! cache and no transport under it, so it is an upper bound on the check's own cost and says
//! nothing about the end-to-end visibility latency plan §12.3 budgets. It cannot, because no
//! transport exists to measure through — the fallback ADR-0015 names is chosen against a real
//! budget or not at all, and this number is one input to that, not the decision.

use std::time::Instant;

use mesh_sync_engine::{admit, EmptyOperations};
use mesh_sync_protocol::{
    ActorId as WireActorId, ActorSequence as WireSequence, CarriedChangeSet,
    ChangeSetId as WireChangeSetId, HeadId as WireHeadId, PolicyEpoch as WirePolicyEpoch,
};
use mesh_types::{
    derive_id, encode_canonical, ActorId, ActorSequence, Blake3, CausalParents, ChangeSet,
    ChangeSetDraft, ChangeSetId, Digest32, HeadId, Hlc, PolicyEpoch, SessionId, Signature,
    WorkspaceId,
};

/// Records at three parent-set widths, because the parent set is the field whose width varies most
/// between a linear history and a merge.
const WIDTHS: [usize; 3] = [0, 1, 8];

/// Enough iterations that the timer's resolution is not the thing being reported.
const ITERATIONS: u32 = 20_000;

fn seal(parents: usize) -> ChangeSet<()> {
    let parents = match parents {
        0 => CausalParents::genesis(),
        n => CausalParents::after(
            ChangeSetId::from_digest(Digest32::from_bytes([1; 32])),
            (1..n)
                .map(|index| ChangeSetId::from_digest(Digest32::from_bytes([index as u8 + 2; 32])))
                .collect(),
        ),
    };
    ChangeSetDraft::<()>::new(
        WorkspaceId::mint(1_700_000_000_000, [1; 10]),
        ActorId::from_digest(Digest32::from_bytes([2; 32])),
        SessionId::mint(1_700_000_000_000, [3; 10]),
        ActorSequence::new(1),
        Hlc::new(1_700_000_000_000, 4),
    )
    .causal_parents(parents)
    .base_head(HeadId::from_digest(Digest32::from_bytes([5; 32])))
    .policy_epoch(PolicyEpoch::new(7))
    .seal(
        Vec::new(),
        HeadId::from_digest(Digest32::from_bytes([9; 32])),
        Signature::from_bytes([0; 64]),
    )
}

fn carry(record: &ChangeSet<()>) -> CarriedChangeSet {
    CarriedChangeSet {
        id: WireChangeSetId::from_bytes(*derive_id::<Blake3, _>(record).digest().as_bytes()),
        author: WireActorId::from_bytes(*record.actor_id().digest().as_bytes()),
        sequence: WireSequence::new(record.actor_sequence().value()),
        parents: record
            .causal_parents()
            .as_slice()
            .iter()
            .map(|parent| WireChangeSetId::from_bytes(*parent.digest().as_bytes()))
            .collect(),
        base_head: WireHeadId::from_bytes(*record.base_head().digest().as_bytes()),
        resulting_head: WireHeadId::from_bytes(*record.resulting_head().digest().as_bytes()),
        policy_epoch: WirePolicyEpoch::new(record.policy_epoch().value()),
        body: encode_canonical(record),
    }
}

#[test]
#[ignore = "a measurement, not a gate: it reports a number and asserts only that the check ran"]
fn report_the_per_record_cost_of_the_re_derivation() {
    println!("receipt check cost, {ITERATIONS} iterations per width");
    for width in WIDTHS {
        let record = seal(width);
        let carried = carry(&record);
        let body_bytes = carried.body.len();

        // One warm pass, so the first measured iteration is not paying for a cold branch predictor.
        admit(&carried, &EmptyOperations).expect("the record is honest");

        let started = Instant::now();
        for _ in 0..ITERATIONS {
            let admitted = admit(&carried, &EmptyOperations).expect("the record is honest");
            std::hint::black_box(admitted.id());
        }
        let elapsed = started.elapsed();
        let per_record = elapsed / ITERATIONS;

        println!(
            "  parents={width:<2} body={body_bytes:>3} bytes   {:>9.3} us/record   \
             {:>8} records/s",
            per_record.as_secs_f64() * 1e6,
            (1.0 / per_record.as_secs_f64()) as u64
        );
    }
}
