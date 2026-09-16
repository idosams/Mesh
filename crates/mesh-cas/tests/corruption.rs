//! Corrupt bytes are rejected at read time and never handed to a caller.
//!
//! Plan §13.3's fault matrix has one row for this: *corrupt content → rejected and re-requested*.
//! Rejection is this crate's half; re-requesting is the sync engine's, and what this crate owes it
//! is a chunk that has been moved out of the way so the re-request has somewhere to land, and an
//! error that names the digest to ask for.
//!
//! # Two different corruptions, two different answers
//!
//! * **Bytes that go bad after promotion** — bit rot, a bad sector, a peer that lied. Caught by
//!   [`mesh_cas::Cas::read`], which hashes before it returns; the chunk is quarantined so a second
//!   caller cannot be served the same bad bytes.
//! * **Bytes that were never right** — a caller that names a digest its buffer does not hash to.
//!   Caught before anything is written at all, because a store that stages first and checks later
//!   has a window in which bad bytes exist under a name.
//!
//! # The third case: a caller that thinks it is repairing
//!
//! A scrubber, or a sync engine acting on a peer's report, learns a chunk is bad *without reading
//! it* — so nothing has quarantined it — and re-promotes known-good bytes. `Link` finds a file at
//! the chunk's name, treats it as the same content, and writes nothing. The repair reports success
//! and repairs nothing (`01KZDSJVMT1H82594HTDM63VEJ`). Re-hashing the target on every promotion is
//! the wrong price for that — it makes deduplication O(chunk) — so the answer is that the outcome
//! says which happened, and `DURABILITY.md` states the sequence that actually replaces bytes.

mod support;

use mesh_cas::{Blake3, Cas, CasError, ContentDigest, Digest32, PromotionOutcome};

use support::{flip_byte, Bytes, TempRoot};

fn payload(seed: u64, length: usize) -> Vec<u8> {
    Bytes::seeded(seed).take(length)
}

#[test]
fn a_flipped_bit_is_rejected_at_read_time_and_the_bytes_are_never_returned() {
    let root = TempRoot::new("corrupt-read");
    let store = Cas::open(root.path()).expect("the store opens");
    let bytes = payload(21, 16 * 1024);
    let digest = store
        .promote(bytes.clone())
        .expect("the promotion completes")
        .digest();

    assert_eq!(store.read(&digest).expect("the chunk verifies"), bytes);

    flip_byte(&store.layout().chunk_path(&digest), 9_000);

    match store.read(&digest) {
        Err(CasError::Corrupt {
            digest: reported,
            found,
            quarantined,
        }) => {
            assert_eq!(reported, digest, "the error names the digest to re-request");
            assert_ne!(found, digest, "the error names what the bytes actually are");
            assert!(
                quarantined.exists(),
                "the corrupt bytes are kept for diagnosis rather than deleted"
            );
        }
        other => panic!("a corrupt chunk must not read as anything else, got {other:?}"),
    }
}

#[test]
fn a_corrupt_chunk_is_out_of_the_store_before_the_failed_read_returns() {
    let root = TempRoot::new("corrupt-quarantine");
    let store = Cas::open(root.path()).expect("the store opens");
    let digest = store
        .promote(payload(22, 4096))
        .expect("the promotion completes")
        .digest();

    flip_byte(&store.layout().chunk_path(&digest), 100);
    let _ = store.read(&digest);

    assert!(
        !store.contains(&digest),
        "a chunk that failed verification must not still be at its content name; leaving it there \
         means the next caller — one that forgets to check — is served the same bad bytes"
    );
    assert!(
        matches!(store.read(&digest), Err(CasError::Absent { .. })),
        "after quarantine the chunk reads as missing, which is a state the sync engine knows how \
         to repair, rather than as present-and-wrong, which it does not"
    );
}

#[test]
fn a_quarantined_chunk_can_be_promoted_again_from_good_bytes() {
    let root = TempRoot::new("corrupt-repair");
    let store = Cas::open(root.path()).expect("the store opens");
    let bytes = payload(23, 4096);
    let digest = store
        .promote(bytes.clone())
        .expect("the promotion completes")
        .digest();

    flip_byte(&store.layout().chunk_path(&digest), 7);
    let _ = store.read(&digest);
    assert!(!store.contains(&digest));

    // This is what a re-request from a peer amounts to locally.
    let again = store
        .promote(bytes.clone())
        .expect("the repair promotes")
        .digest();
    assert_eq!(again, digest);
    assert_eq!(
        store.read(&digest).expect("the repaired chunk verifies"),
        bytes
    );
}

/// Re-promoting good bytes over a rotted chunk tells the caller nothing was written.
///
/// The four steps are the reproduction recorded on `01KZDSJVMT1H82594HTDM63VEJ`: promote, flip a
/// byte on disk **without reading**, re-promote, read. Step three used to return a bare `Ok(digest)`
/// indistinguishable from a real repair; it now reports `AlreadyPresent`, and step four still shows
/// why that matters.
#[test]
fn re_promoting_good_bytes_over_a_rotted_chunk_signals_that_nothing_changed() {
    let root = TempRoot::new("corrupt-no-repair");
    let store = Cas::open(root.path()).expect("the store opens");
    let bytes = payload(26, 4096);

    let first = store
        .promote(bytes.clone())
        .expect("the promotion completes");
    assert_eq!(
        first.outcome(),
        PromotionOutcome::Linked,
        "the first promotion is the reason the chunk is in the store"
    );
    let digest = first.digest();

    // A scrub finding or a peer report: the bytes go bad and nothing reads them, so nothing has
    // quarantined the chunk.
    flip_byte(&store.layout().chunk_path(&digest), 11);

    let repair = store
        .promote(bytes.clone())
        .expect("the re-promotion completes");
    assert_eq!(
        repair.digest(),
        digest,
        "content addressing still names the chunk the same thing"
    );
    assert_eq!(
        repair.outcome(),
        PromotionOutcome::AlreadyPresent,
        "a promotion that found a file at the chunk's name wrote nothing, and a caller that was \
         trying to repair a rotted chunk has to be able to tell that from a repair"
    );
    assert!(
        !repair.wrote_content(),
        "`wrote_content` is the one-word form of the same fact"
    );

    // And the reason it matters: the store still cannot serve the digest.
    assert!(
        matches!(store.read(&digest), Err(CasError::Corrupt { .. })),
        "nothing was repaired, so the rotted bytes are still what is on disk"
    );

    // The documented repair sequence — quarantine, then promote — does replace them.
    let store = Cas::open(root.path()).expect("the store reopens");
    let repaired = store
        .promote(bytes.clone())
        .expect("the repair promotion completes");
    assert_eq!(
        repaired.outcome(),
        PromotionOutcome::Linked,
        "the failed read quarantined the rotted chunk, so this promotion is a real repair"
    );
    assert_eq!(
        store.read(&digest).expect("the repaired chunk verifies"),
        bytes
    );
}

#[test]
fn corrupting_a_chunk_twice_keeps_both_samples() {
    let root = TempRoot::new("corrupt-samples");
    let store = Cas::open(root.path()).expect("the store opens");
    let bytes = payload(24, 2048);
    let digest = store
        .promote(bytes.clone())
        .expect("the promotion completes")
        .digest();

    flip_byte(&store.layout().chunk_path(&digest), 1);
    let first = quarantined_path(&store, &digest);

    store.promote(bytes.clone()).expect("the repair promotes");
    flip_byte(&store.layout().chunk_path(&digest), 2);
    let second = quarantined_path(&store, &digest);

    assert_ne!(
        first, second,
        "a second sample must not overwrite the first; two samples are the difference between one \
         flipped bit and a disk that is failing"
    );
    assert!(first.exists() && second.exists());
}

fn quarantined_path(store: &Cas, digest: &Digest32) -> std::path::PathBuf {
    match store.read(digest) {
        Err(CasError::Corrupt { quarantined, .. }) => quarantined,
        other => panic!("expected a corruption report, got {other:?}"),
    }
}

#[test]
fn bytes_that_do_not_match_the_named_digest_are_refused_before_anything_is_written() {
    let root = TempRoot::new("corrupt-claim");
    let store = Cas::open(root.path()).expect("the store opens");
    let bytes = payload(25, 4096);
    let wrong = Blake3::digest_bytes(b"different content entirely");

    match store.begin_promotion_expecting(bytes, wrong) {
        Err(CasError::DigestMismatch { expected, found }) => {
            assert_eq!(expected, wrong);
            assert_ne!(found, wrong);
        }
        other => panic!("a mismatched claim must be refused, got {other:?}"),
    }

    assert!(!store.contains(&wrong));
    assert_eq!(
        store.discard_scratch().expect("scratch is readable"),
        0,
        "a refused claim writes nothing at all, so there is no window in which bad bytes exist \
         under a name"
    );
}

#[test]
fn a_missing_chunk_reads_as_absent_rather_than_as_empty() {
    let root = TempRoot::new("corrupt-absent");
    let store = Cas::open(root.path()).expect("the store opens");
    let never = Blake3::digest_bytes(b"never promoted");

    assert!(!store.contains(&never));
    assert!(matches!(store.read(&never), Err(CasError::Absent { .. })));
}

/// An empty chunk is real content with a real digest, and must not be confused with absence.
#[test]
fn the_empty_chunk_is_content_and_not_absence() {
    let root = TempRoot::new("corrupt-empty");
    let store = Cas::open(root.path()).expect("the store opens");

    let digest = store
        .promote(Vec::new())
        .expect("an empty chunk promotes")
        .digest();
    assert!(store.contains(&digest));
    assert_eq!(
        store.read(&digest).expect("the empty chunk verifies"),
        Vec::<u8>::new()
    );
}
