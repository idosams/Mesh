//! The staging name is exclusive, and that exclusivity is what makes promotion safe under a race.
//!
//! # Why this file exists
//!
//! `StdFs::stage` opens with `create_new(true)`. Its own documentation calls that "load-bearing",
//! `Promotion::perform_stage` builds a sixty-four-name retry loop entirely on the
//! `ErrorKind::AlreadyExists` it produces, and `StoreLayout::staging_path` names "two threads of
//! this process staging byte-for-byte identical content at the same instant" as the collision the
//! attempt counter resolves. Until this file, none of that had an oracle: a cold verification pass
//! replaced the one call with `create(true).truncate(true)` and the whole crate's suite still
//! passed, forty-one for forty-one.
//!
//! An untested guard is not a guard. It is a comment that happens to compile, and the next refactor
//! deletes it without anything going red.
//!
//! # What the mutant actually does
//!
//! With `create_new` gone, two threads promoting identical bytes share one staging file. Both agree
//! on the digest and both run in this process, so both compute the same attempt-zero name and both
//! are granted it. One truncates while the other sits between `Verify` and `Link`, and the second
//! then renames a half-written file into the content namespace under a name that says it was
//! verified. That is precisely the present-but-partial state the crate exists to make impossible.
//!
//! # How the file is split
//!
//! The first three tests are deterministic and are the gate. They construct the collision by hand,
//! so they fail on every machine, in every scheduling order, the instant the guard is weakened.
//! [`concurrent_identical_promotions_all_land_a_whole_chunk`] is the property those three protect —
//! a real race, which by nature is evidence when it collides and silence when it does not. It is
//! not the gate and this file does not pretend otherwise; see its own documentation for what it
//! cannot establish.

mod support;

use std::io;
use std::path::PathBuf;
use std::sync::Barrier;

use mesh_cas::{Blake3, Cas, CasError, ContentDigest, DurableFs, Promoted, PromotionStep, StdFs};

use support::{Bytes, TempRoot};

/// More staging names than the retry loop tries, without repeating its private constant here.
///
/// The loop's bound is `STAGING_ATTEMPTS`, which is not public. Occupying a number comfortably above
/// it and reading the count back out of the error keeps this test correct if that constant moves,
/// and turns "somebody raised it past this ceiling" into a message that says so rather than a
/// confusing pass.
const OCCUPIED_NAME_CEILING: u32 = 256;

/// `StdFs::stage` refuses an occupied name and does not touch what is there.
///
/// This is the mutation test, stated directly. `create(true).truncate(true)` succeeds here and
/// returns `Ok(())`, so `expect_err` panics on the first line that matters; the byte assertion that
/// follows is the second, independent reason it fails, since the intruder is deliberately shorter
/// than the occupant and a truncation is visible in the file's length alone.
#[test]
fn staging_refuses_to_overwrite_an_existing_file() {
    let root = TempRoot::new("stage-refuses");
    let filesystem = StdFs;
    let path = root.path().join("occupied.chunk");

    let occupant = Bytes::seeded(701).take(4096);
    filesystem
        .stage(&path, &occupant)
        .expect("staging into a free name succeeds");

    let intruder = b"eight!!!";
    let error = filesystem.stage(&path, intruder).expect_err(
        "staging onto an occupied name must fail: the whole promotion path treats a staging name \
         it holds as exclusively its own, and an overwrite here hands one writer's file to another",
    );

    assert_eq!(
        error.kind(),
        io::ErrorKind::AlreadyExists,
        "the retry loop in Promotion::perform_stage dispatches on this exact kind; anything else \
         is reported to the caller as a failed promotion. The error was {error}"
    );
    assert_eq!(
        std::fs::read(&path).expect("the occupant is still readable"),
        occupant,
        "the refused stage must leave the occupant's bytes exactly as they were; it wrote {} bytes \
         over them",
        intruder.len()
    );
}

/// An occupied attempt zero pushes the promotion to attempt one and leaves attempt zero alone.
///
/// The squatter stands in for the two cases `StoreLayout::staging_path` names: a crashed process's
/// leftovers, and another thread of this process that got there first. Both look identical from
/// here — a taken name — which is the point of testing it this way rather than with a real race.
#[test]
fn a_taken_staging_name_makes_the_promotion_use_the_next_attempt() {
    let root = TempRoot::new("stage-next-attempt");
    let store = Cas::open(root.path()).expect("the store opens");
    let bytes = Bytes::seeded(702).take(8192);
    let digest = Blake3::digest_bytes(&bytes);
    let process = std::process::id();

    let attempt_zero = store.layout().staging_path(&digest, process, 0);
    let attempt_one = store.layout().staging_path(&digest, process, 1);
    let squatter = Bytes::seeded(703).take(17);
    StdFs
        .stage(&attempt_zero, &squatter)
        .expect("attempt zero's name is free until this test takes it");

    let mut promotion = store.begin_promotion(bytes.clone());
    promotion
        .run_through(PromotionStep::Stage)
        .expect("staging finds a free name when the first is taken");

    assert_eq!(
        promotion.staged_path(),
        Some(attempt_one.as_path()),
        "with attempt zero occupied the promotion must move to attempt one; staging at the taken \
         name would mean two writers holding one file"
    );
    assert_eq!(
        std::fs::read(&attempt_zero).expect("the squatter's file is still readable"),
        squatter,
        "the promotion overwrote the occupant of attempt zero"
    );

    let promoted = promotion.finish().expect("the promotion completes");
    assert_eq!(
        promoted.digest(),
        digest,
        "a promotion that had to retry still lands under the digest of its bytes"
    );
    assert_eq!(
        store.read(&digest).expect("the promoted chunk verifies"),
        bytes,
        "the chunk that landed is the whole payload"
    );
    assert_eq!(
        std::fs::read(&attempt_zero).expect("the squatter's file survived the whole promotion"),
        squatter,
        "nothing later in the promotion may touch a staging name it does not hold"
    );
}

/// Every staging name being taken is an error, never an overwrite.
///
/// Beyond the three tests the contract names, and here because the retry loop's failure end is the
/// other half of the same guard: exhausting the names must produce `StagingNamesExhausted` rather
/// than falling back on clobbering one. Under the mutant the first name is granted and no promotion
/// ever reaches this path, so this fails too — but it is the deterministic pair above that is the
/// gate, not this.
#[test]
fn every_staging_name_being_taken_is_an_error_rather_than_an_overwrite() {
    let root = TempRoot::new("stage-exhausted");
    let store = Cas::open(root.path()).expect("the store opens");
    let bytes = Bytes::seeded(704).take(1024);
    let digest = Blake3::digest_bytes(&bytes);
    let process = std::process::id();

    let squatter = b"squatter";
    for attempt in 0..OCCUPIED_NAME_CEILING {
        StdFs
            .stage(
                &store.layout().staging_path(&digest, process, attempt),
                squatter,
            )
            .expect("each staging name is free until this test takes it");
    }

    let outcome = store.begin_promotion(bytes.clone()).finish();
    let Err(CasError::StagingNamesExhausted { path, attempts }) = outcome else {
        panic!(
            "with the first {OCCUPIED_NAME_CEILING} staging names occupied, a promotion must give \
             up rather than overwrite one of them; it returned {outcome:?}"
        );
    };

    assert!(
        (1..=OCCUPIED_NAME_CEILING).contains(&attempts),
        "the retry loop reported {attempts} names tried, which is either zero — so it never \
         actually looked for a free name — or more than the {OCCUPIED_NAME_CEILING} this test \
         occupied, so the promotion gave up for some other reason. Either way this test no longer \
         checks what it says it checks"
    );
    assert_eq!(
        path,
        store.layout().staging_path(&digest, process, attempts - 1),
        "the error must name the last name tried, so an operator can see which files to clear"
    );
    assert!(
        !store.contains(&digest),
        "a promotion that never found a staging name must not have produced a chunk"
    );
    for attempt in 0..OCCUPIED_NAME_CEILING {
        let occupied = store.layout().staging_path(&digest, process, attempt);
        assert_eq!(
            std::fs::read(&occupied).expect("every occupied name still holds its file"),
            squatter,
            "the exhausted promotion overwrote attempt {attempt}"
        );
    }
}

/// Eight threads promoting byte-identical content concurrently all land a whole, verifying chunk.
///
/// # What this establishes
///
/// The failure the guard prevents, reproduced against the real filesystem: every thread computes
/// the same digest and runs in the same process, so every thread's attempt-zero staging name is the
/// same string, and only `create_new` keeps them from sharing the file. When the threads genuinely
/// overlap, the mutant produces either a verification failure — one thread reading back what
/// another truncated — or a rename of a file that is no longer there, and both surface here as a
/// promotion that returned an error.
///
/// # What it does not establish
///
/// It is a race, so it is evidence only when it races. A machine that serialises the eight threads
/// end to end runs eight independent promotions, each finding attempt zero free because the last
/// one renamed its file away, and passes without ever exercising the guard. The number of stagings
/// that landed on a non-zero attempt is counted and reported for exactly that reason, and it is
/// deliberately *not* asserted on: the task contract anticipates the vacuous pass and puts the
/// gating burden on the two deterministic tests above. Read a low collision count as "this run
/// proved little", not as a failure.
#[test]
fn concurrent_identical_promotions_all_land_a_whole_chunk() {
    /// Threads racing for one staging name.
    const THREADS: usize = 8;
    /// Rounds, each with a fresh payload and so a fresh staging name to race for.
    const ROUNDS: u64 = 40;
    /// Payload size: large enough that the write, sync and read-back of one promotion overlap the
    /// next thread's stage rather than completing inside its scheduling quantum.
    const PAYLOAD: usize = 512 * 1024;

    let root = TempRoot::new("stage-concurrent");
    let store = Cas::open(root.path()).expect("the store opens");
    let mut collisions = 0_usize;

    for round in 0..ROUNDS {
        let bytes = Bytes::seeded(900 + round).take(PAYLOAD);
        let digest = Blake3::digest_bytes(&bytes);
        let attempt_zero = store.layout().staging_path(&digest, std::process::id(), 0);
        let barrier = Barrier::new(THREADS);

        let outcomes: Vec<Result<(Promoted, PathBuf), CasError>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..THREADS)
                .map(|_| {
                    let (bytes, barrier, store) = (&bytes, &barrier, &store);
                    scope.spawn(move || {
                        barrier.wait();
                        let mut promotion = store.begin_promotion(bytes.clone());
                        promotion.run_through(PromotionStep::Stage)?;
                        let staged = promotion
                            .staged_path()
                            .expect("staging records the name it took")
                            .to_path_buf();
                        Ok((promotion.finish()?, staged))
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("no promotion thread panicked"))
                .collect()
        });

        for (thread, outcome) in outcomes.into_iter().enumerate() {
            match outcome {
                Ok((promoted, staged)) => {
                    assert_eq!(
                        promoted.digest(),
                        digest,
                        "round {round}, thread {thread}: a promotion under contention still lands \
                         under the digest of its bytes"
                    );
                    if staged != attempt_zero {
                        collisions += 1;
                    }
                }
                Err(error) => panic!(
                    "round {round}, thread {thread}: promoting content another thread is promoting \
                     byte-for-byte must succeed, because the two hold different staging files. It \
                     failed with: {error}"
                ),
            }
        }

        assert_eq!(
            store.read(&digest).expect(
                "the chunk eight threads promoted at once is present and hashes to its name"
            ),
            bytes,
            "round {round}: the chunk that survived the race is not the whole payload"
        );
    }

    assert_eq!(
        store.discard_scratch().expect("scratch is readable"),
        0,
        "every one of the {} promotions removed or renamed away its own staging file; anything \
         left is a file some promotion lost track of",
        ROUNDS as usize * THREADS
    );
    // Reported rather than asserted: see this test's documentation. A run that collided zero times
    // is a run in which the guard was never reached, and the reader is entitled to know that.
    eprintln!(
        "concurrent staging: {collisions} of {} stagings landed on a non-zero attempt, so that \
         many actually contended for a taken name",
        ROUNDS as usize * THREADS
    );
}
