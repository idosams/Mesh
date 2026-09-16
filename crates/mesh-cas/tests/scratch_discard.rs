//! `discard_scratch` may not free a staging name a live promotion is still going to rename.
//!
//! # Why this file exists
//!
//! `Cas::discard_scratch` used to document its precondition and understate what breaking it costs:
//! *"calling this while another writer is staging would delete that writer's file out from under
//! it."* A reader weighing a missing lock against a failed promotion weighs it wrongly, because the
//! real cost is a chunk that was never verified standing at a verified name
//! (`01KZDSHZAXQ223DB2GAV9GEVKM`).
//!
//! The sequence needs no crash and no filesystem fault. A staging name is a pure function of
//! digest, process and attempt, so a name that is freed is the *first* name the next promotion of
//! the same content picks:
//!
//! 1. Promotion A stages, flushes, verifies and records its arrival at `scratch/<hex>.<pid>.0.chunk`.
//! 2. The staging file is removed; the name is free.
//! 3. A second writer stages the same digest, is granted attempt zero, and is mid-write.
//! 4. A renames the path it remembers — now the second writer's partial file — into `chunks/`.
//! 5. `contains` is true and `read` returns `Corrupt`.
//!
//! # How this file is split
//!
//! [`discarding_scratch_under_a_live_promotion_leaves_its_staged_file_alone`] and
//! [`discarding_scratch_does_not_hand_a_live_promotion_s_name_to_the_next_writer`] are the fix, and
//! they are the gate: both go red the instant `discard_scratch` stops asking whether a name is
//! held.
//!
//! [`freeing_a_staging_name_behind_the_promotion_that_holds_it_publishes_unverified_bytes`] is the
//! residue. It removes the file with `std::fs` rather than through the store, which is what a
//! *second process* looks like from here — the register of held names is process-local by
//! construction and cannot see one. That test is not a bug report; it is the documented cost of
//! plan §6.1's single-writer precondition, pinned so that the sentence in `DURABILITY.md`
//! assumption 4 has an oracle rather than a promise.
//!
//! The last two tests are the other half of the guard: a guard that never gives a name back would
//! turn `discard_scratch` into a no-op, and the crash tests that assert it removes exactly one file
//! would be the only thing to notice.

mod support;

use mesh_cas::{Blake3, Cas, CasError, ContentDigest, DurableFs, PromotionStep, StdFs};

use support::{Bytes, TempRoot};

/// The staged file of a promotion that has done everything but the rename survives a
/// `discard_scratch`, and the promotion still lands its own bytes.
///
/// This is the fix stated directly. Without the guard `discard_scratch` returns 1 and the first
/// assertion fails; every later assertion is an independent reason, since the promotion then
/// renames a path with nothing at it.
#[test]
fn discarding_scratch_under_a_live_promotion_leaves_its_staged_file_alone() {
    let root = TempRoot::new("scratch-live-promotion");
    let store = Cas::open(root.path()).expect("the store opens");
    let bytes = Bytes::seeded(801).take(8192);
    let digest = Blake3::digest_bytes(&bytes);

    let mut promotion = store.begin_promotion(bytes.clone());
    promotion
        .run_through(PromotionStep::RecordArrival)
        .expect("staging, flushing, verifying and recording all succeed");
    let staged = promotion
        .staged_path()
        .expect("the promotion holds a staging path")
        .to_path_buf();

    assert_eq!(
        store.discard_scratch().expect("scratch is readable"),
        0,
        "the only file in scratch/ belongs to a promotion that has staged, flushed, verified and \
         recorded its arrival. Removing it frees a name the next writer of the same content takes \
         first, and this promotion's Link then renames that writer's file into chunks/"
    );
    assert!(
        staged.exists(),
        "the staged file at {} was removed under a live promotion",
        staged.display()
    );
    assert_eq!(
        std::fs::read(&staged).expect("the staged file is readable"),
        bytes,
        "the staged file survived by name but not by content"
    );

    let promoted = promotion.finish().expect("the promotion completes");
    assert!(
        promoted.wrote_content(),
        "the promotion that held the name is the one that linked the chunk"
    );
    assert_eq!(
        store.read(&digest).expect("the promoted chunk verifies"),
        bytes,
        "the chunk that landed is the payload the promotion verified"
    );
}

/// After a `discard_scratch` the next writer of the same content does not get the live promotion's
/// name, and both promotions end with the store holding the right bytes.
///
/// Step 3 of the sequence, made unreachable. The second writer is a real second `Promotion` on the
/// same store, staging byte-identical content, so it computes exactly the same attempt-zero name;
/// `create_new` refuses it only because the first promotion's file is still there for it to refuse.
#[test]
fn discarding_scratch_does_not_hand_a_live_promotion_s_name_to_the_next_writer() {
    let root = TempRoot::new("scratch-second-writer");
    let store = Cas::open(root.path()).expect("the store opens");
    let bytes = Bytes::seeded(802).take(4096);
    let digest = Blake3::digest_bytes(&bytes);
    let attempt_zero = store.layout().staging_path(&digest, std::process::id(), 0);

    let mut first = store.begin_promotion(bytes.clone());
    first
        .run_through(PromotionStep::RecordArrival)
        .expect("the first promotion reaches the rename");
    assert_eq!(
        first.staged_path(),
        Some(attempt_zero.as_path()),
        "the first promotion takes attempt zero, which is the name this test is about"
    );

    store.discard_scratch().expect("scratch is readable");

    let mut second = store.begin_promotion(bytes.clone());
    second
        .run_through(PromotionStep::Stage)
        .expect("the second writer finds a free staging name");
    assert_ne!(
        second.staged_path(),
        Some(attempt_zero.as_path()),
        "the second writer was handed the staging name the first promotion is about to rename. \
         The first promotion's Link would then publish whatever the second writer had written so \
         far, under a name that says it was verified"
    );

    let first = first.finish().expect("the first promotion completes");
    let second = second.finish().expect("the second promotion completes");
    assert!(
        first.wrote_content(),
        "the first promotion is the one that put the chunk there"
    );
    assert!(
        !second.wrote_content(),
        "the second promotion found the chunk already present and wrote nothing"
    );
    assert_eq!(
        store.read(&digest).expect("the chunk verifies"),
        bytes,
        "the chunk two overlapping promotions produced is the whole payload"
    );
    assert_eq!(
        store.discard_scratch().expect("scratch is readable"),
        0,
        "both promotions gave their staging names back, so nothing is left to discard"
    );
}

/// A staging name freed by something the register cannot see lets a promotion publish bytes it
/// never verified — the documented cost of the single-writer precondition.
///
/// The removal and the intruder both go through `std::fs` rather than through the store, because
/// that is precisely what a second process looks like from inside this one: the register of held
/// names is process-local, so nothing outside this process consults it. The intruder is eight bytes
/// where the payload is 4096, standing in for a second writer caught part-way through its
/// `write_all`.
///
/// The assertion at the end is the point of the whole task. The promotion reports that it wrote
/// content, `contains` agrees that the chunk is there, and `read` finds bytes that hash to
/// something else. That is the third outcome the crate's guarantee says does not exist, and it is
/// reachable *only* by breaking the precondition — which is why the precondition's cost has to be
/// written down in the words this test uses.
#[test]
fn freeing_a_staging_name_behind_the_promotion_that_holds_it_publishes_unverified_bytes() {
    let root = TempRoot::new("scratch-foreign-writer");
    let store = Cas::open(root.path()).expect("the store opens");
    let bytes = Bytes::seeded(803).take(4096);
    let digest = Blake3::digest_bytes(&bytes);

    let mut promotion = store.begin_promotion(bytes.clone());
    promotion
        .run_through(PromotionStep::RecordArrival)
        .expect("the promotion reaches the rename");
    let staged = promotion
        .staged_path()
        .expect("the promotion holds a staging path")
        .to_path_buf();

    // Steps 2 and 3, performed by an agent outside this process's register.
    std::fs::remove_file(&staged).expect("a second process may remove any file in scratch/");
    let intruder = b"partial!";
    StdFs
        .stage(&staged, intruder)
        .expect("the freed name is the first one the next writer of this digest picks");

    let promoted = promotion.finish().expect(
        "the promotion renames the path it remembers and reports success; it has no way to notice \
         that the file at that path is not the file it verified",
    );
    assert!(
        promoted.wrote_content(),
        "the promotion believes it linked its own bytes, which is the whole trouble"
    );
    assert!(
        store.contains(&digest),
        "the chunk's name resolves to a file, so every reader that trusts `contains` is now wrong"
    );

    let Err(CasError::Corrupt { found, .. }) = store.read(&digest) else {
        panic!(
            "reading the chunk must reject it: what stands at the verified name is the {} bytes \
             the intruder had written, not the {} bytes the promotion verified",
            intruder.len(),
            bytes.len()
        );
    };
    assert_eq!(
        found,
        Blake3::digest_bytes(intruder),
        "the bytes published under the chunk's name are the intruder's, which is what makes this a \
         published-unverified-chunk failure rather than a lost staging file"
    );
}

/// The guard does not stop `discard_scratch` doing its job: a name no live promotion holds is
/// removed, whichever of the four ways the promotion that held it ended.
///
/// A promotion gives its staging name back on the rename, on the deduplicated removal, on a
/// verification failure and in its destructor. A missing release on any one of them leaves that
/// name permanently undeletable, so a leftover at it survives every future startup and plan §6.3's
/// "temporary data is discarded" quietly stops holding for the names that see the most traffic.
///
/// **The four exits are checked one at a time, and that is the point.** Checking them together
/// hides three of the four mutations: a later promotion of the same content stages at the same
/// attempt-zero name and releases it on *its* way out, so a name leaked by the rename is handed
/// back by the deduplicated removal that follows it. Each exit is therefore followed immediately by
/// a leftover planted at the name it should have released, and by the discard that must remove it.
#[test]
fn every_name_a_finished_promotion_gives_back_is_discardable_again() {
    let root = TempRoot::new("scratch-released-names");
    let store = Cas::open(root.path()).expect("the store opens");
    let process = std::process::id();

    /// Plant a leftover at a name the promotion that held it has finished with, and require that a
    /// discard removes it. Both halves fail under a missing release: the `stage` refuses an
    /// occupied name, and a held name is skipped by the discard.
    fn the_name_is_free_again(store: &Cas, name: &std::path::Path, exit: &str) {
        StdFs.stage(name, b"leftover").unwrap_or_else(|error| {
            panic!("after the {exit} exit the staging name should be free on disk: {error}")
        });
        assert_eq!(
            store.discard_scratch().expect("scratch is readable"),
            1,
            "after the {exit} exit the promotion no longer holds {}, so a leftover at it is \
             ordinary temporary data. A name that is never given back makes discard_scratch a \
             no-op on exactly the files it exists for",
            name.display()
        );
    }

    let content = Bytes::seeded(804).take(1024);
    let abandoned = Bytes::seeded(805).take(1024);
    let unverifiable = Bytes::seeded(806).take(1024);
    let name = |bytes: &[u8]| {
        store
            .layout()
            .staging_path(&Blake3::digest_bytes(bytes), process, 0)
    };

    // Exit one: `Link` renamed the file away.
    assert!(
        store
            .promote(content.clone())
            .expect("the promotion completes")
            .wrote_content(),
        "the first promotion of this content links the chunk"
    );
    the_name_is_free_again(&store, &name(&content), "rename");

    // Exit two: `Link` found the chunk present and removed the staged copy.
    assert!(
        !store
            .promote(content.clone())
            .expect("the promotion completes")
            .wrote_content(),
        "the second promotion of identical bytes deduplicates"
    );
    the_name_is_free_again(&store, &name(&content), "deduplicated");

    // Exit three: the destructor, for a promotion abandoned before the rename.
    {
        let mut promotion = store.begin_promotion(abandoned.clone());
        promotion
            .run_through(PromotionStep::Verify)
            .expect("verification succeeds");
    }
    the_name_is_free_again(&store, &name(&abandoned), "destructor");

    // Exit four: verification failed, and the bytes that failed it were removed on the spot.
    let mut promotion = store.begin_promotion(unverifiable.clone());
    promotion
        .run_through(PromotionStep::Flush)
        .expect("staging and flushing succeed");
    support::flip_byte(&name(&unverifiable), 41);
    let outcome = promotion.step();
    assert!(
        matches!(outcome, Err(CasError::StagedVerificationFailed { .. })),
        "a staged file that stopped hashing to its name must fail verification; it returned \
         {outcome:?}"
    );
    the_name_is_free_again(&store, &name(&unverifiable), "verification-failure");
}

/// A leftover from a *crashed* process is still removed — the case `discard_scratch` exists for.
///
/// The guard keys on live promotions in this process, not on process identifiers, so a file left by
/// a process that is gone is held by nobody. Written with a leftover carrying this process's own
/// identifier, which is the hardest case: had the guard been built on "is this pid alive" it would
/// refuse to remove this file forever, and pid reuse would make it refuse to remove other
/// processes' files too.
#[test]
fn a_leftover_from_a_process_that_is_gone_is_still_discarded() {
    let root = TempRoot::new("scratch-crash-leftover");
    let store = Cas::open(root.path()).expect("the store opens");
    let bytes = Bytes::seeded(806).take(512);
    let digest = Blake3::digest_bytes(&bytes);
    let leftover = store.layout().staging_path(&digest, std::process::id(), 7);
    StdFs
        .stage(&leftover, &bytes)
        .expect("the leftover's name is free");

    assert_eq!(
        store.discard_scratch().expect("scratch is readable"),
        1,
        "a staging file no live promotion holds is exactly the temporary data plan §6.3 says to \
         discard, whatever process identifier is in its name"
    );
    assert!(
        !leftover.exists(),
        "the leftover at {} survived a discard",
        leftover.display()
    );
}
