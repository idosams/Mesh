//! Actor sequence numbers are monotonic where they are issued and gap-detectable where they land.
//!
//! The unit tests in `src/sequence.rs` cover the single-threaded shape. This file covers the two
//! things a single-threaded test cannot: concurrent issuance, and a receiver's view of a stream
//! that a hostile or lossy relay has interfered with.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::thread;

use mesh_operations::{
    ActorId, ActorSequence, SequenceLedger, SequenceObservation, SequenceWitness,
};

const THREADS: usize = 8;
const PER_THREAD: usize = 500;

#[test]
fn concurrent_issuance_produces_each_number_exactly_once() {
    let actor = ActorId::from_bytes([1; 32]);
    let ledger = Arc::new(Mutex::new(Some(SequenceLedger::new(actor))));
    let issued = Arc::new(Mutex::new(Vec::new()));

    let mut handles = Vec::new();
    for _ in 0..THREADS {
        let ledger = Arc::clone(&ledger);
        let issued = Arc::clone(&issued);
        handles.push(thread::spawn(move || {
            for _ in 0..PER_THREAD {
                // `issue` consumes the ledger and returns the next one, so the shared slot holds
                // the only live ledger and a thread that takes it must put its successor back
                // before releasing the guard. Two threads cannot hold the same ledger, which is
                // what makes a repeated number impossible rather than unlikely.
                let number = {
                    let mut slot = ledger.lock().expect("no thread poisoned the ledger");
                    let taken = slot.take().expect("a live ledger");
                    let (next, number) = taken.issue().expect("far below the ceiling");
                    *slot = Some(next);
                    number
                };
                issued
                    .lock()
                    .expect("no thread poisoned the log")
                    .push(number);
            }
        }));
    }
    for handle in handles {
        handle.join().expect("no thread panicked");
    }

    let issued = issued.lock().unwrap().clone();
    assert_eq!(issued.len(), THREADS * PER_THREAD);

    let distinct: BTreeSet<u64> = issued.iter().map(ActorSequence::value).collect();
    assert_eq!(
        distinct.len(),
        issued.len(),
        "a number was issued more than once"
    );
    assert_eq!(*distinct.iter().next().unwrap(), 1, "zero is never issued");
    assert_eq!(
        *distinct.iter().next_back().unwrap(),
        (THREADS * PER_THREAD) as u64,
        "the issued set is not contiguous"
    );

    // A witness that receives every issued number, in the order they happened to be recorded,
    // ends with no hole. Concurrency changes the interleaving, never the set.
    let mut witness = SequenceWitness::new();
    for number in &issued {
        witness.observe(actor, *number);
    }
    assert!(witness.is_contiguous(actor));
    assert_eq!(
        witness.highest(actor),
        Some(ActorSequence::new((THREADS * PER_THREAD) as u64))
    );
}

#[test]
fn a_ledger_never_goes_backwards_however_it_is_interleaved() {
    let actor = ActorId::from_bytes([2; 32]);
    let mut ledger = SequenceLedger::new(actor);
    let mut previous = 0u64;
    for _ in 0..10_000 {
        let (next, number) = ledger.issue().unwrap();
        ledger = next;
        assert!(
            number.value() > previous,
            "{} did not exceed {previous}",
            number.value()
        );
        previous = number.value();
    }
}

/// The withholding attack the counter exists to make visible: a relay forwards ChangeSets 1 and 3
/// and drops 2. Causal parents cannot see it — 3 need not name 2 at all — and the sequence can.
#[test]
fn a_withheld_changeset_is_visible_to_the_receiver() {
    let actor = ActorId::from_bytes([3; 32]);
    let mut witness = SequenceWitness::new();
    assert_eq!(
        witness.observe(actor, ActorSequence::new(1)),
        SequenceObservation::InOrder
    );
    assert_eq!(
        witness.observe(actor, ActorSequence::new(3)),
        SequenceObservation::Gap {
            missing: vec![ActorSequence::new(2)]
        }
    );
    assert_eq!(witness.missing(actor), vec![ActorSequence::new(2)]);
    // And it stays visible until the withheld number actually arrives.
    witness.observe(actor, ActorSequence::new(4));
    assert_eq!(witness.missing(actor), vec![ActorSequence::new(2)]);
    witness.observe(actor, ActorSequence::new(2));
    assert!(witness.is_contiguous(actor));
}

/// Shuffled delivery of a complete stream must leave no gap, whatever the order. A witness that
/// tracked only "the last number I saw" would report gaps for every out-of-order arrival.
#[test]
fn shuffled_delivery_of_a_complete_stream_leaves_no_gap() {
    let actor = ActorId::from_bytes([4; 32]);
    let count = 512u64;
    // A deterministic shuffle: step through the range by a stride coprime with its length, which
    // visits every element exactly once in an order unrelated to the sequence.
    let stride = 37u64;
    let mut witness = SequenceWitness::new();
    let mut at = 0u64;
    for _ in 0..count {
        at = (at + stride) % count;
        witness.observe(actor, ActorSequence::new(at + 1));
    }
    assert!(witness.is_contiguous(actor));
    assert_eq!(witness.highest(actor), Some(ActorSequence::new(count)));
}

#[test]
fn every_prefix_of_a_reversed_stream_reports_exactly_what_is_still_missing() {
    let actor = ActorId::from_bytes([5; 32]);
    let count = 64u64;
    let mut witness = SequenceWitness::new();
    for number in (1..=count).rev() {
        witness.observe(actor, ActorSequence::new(number));
        let expected: Vec<ActorSequence> = (1..number).map(ActorSequence::new).collect();
        assert_eq!(
            witness.missing(actor),
            expected,
            "after delivering {number}"
        );
    }
    assert!(witness.is_contiguous(actor));
}

/// Two actors are two sequences. A witness that keyed on the number rather than on the author
/// would call the second actor's first ChangeSet a duplicate.
#[test]
fn sequences_from_different_actors_never_interfere() {
    let one = ActorId::from_bytes([6; 32]);
    let other = ActorId::from_bytes([7; 32]);
    let mut witness = SequenceWitness::new();
    for number in 1..=10u64 {
        assert_eq!(
            witness.observe(one, ActorSequence::new(number)),
            SequenceObservation::InOrder
        );
        assert_eq!(
            witness.observe(other, ActorSequence::new(number)),
            SequenceObservation::InOrder
        );
    }
    assert!(witness.is_contiguous(one));
    assert!(witness.is_contiguous(other));
    assert_eq!(witness.actors().len(), 2);
}
