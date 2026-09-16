//! Finding unreferenced chunks without reading the store.
//!
//! The acceptance criterion is *"unreferenced chunks are detectable without scanning the whole
//! store on every run"*, and the only honest way to test that is to count what the store asks the
//! filesystem to do. So these tests run against the recording filesystem and assert that
//! [`mesh_cas::Cas::unreferenced_candidates`] lists **no** directory at all — not that it lists
//! few, not that it is fast on a small fixture, which is what a timing test would actually be
//! measuring.
//!
//! The full scan is tested too, in its own test, and named for what it is.

mod support;

use mesh_cas::{Blake3, Cas, CasError, Digest32};

use support::{Bytes, RecordingFs, TempRoot};

fn payload(seed: u64, length: usize) -> Vec<u8> {
    Bytes::seeded(seed).take(length)
}

/// An oracle that knows a fixed set of digests are referenced.
fn referencing(digests: Vec<Digest32>) -> impl Fn(&Digest32) -> bool {
    move |digest: &Digest32| digests.contains(digest)
}

#[test]
fn finding_candidates_never_lists_a_directory() {
    let root = TempRoot::new("unref-nolist");
    let store: Cas<RecordingFs, Blake3> =
        Cas::with_filesystem(root.path(), RecordingFs::new()).expect("the store opens");

    for seed in 0..25 {
        store
            .promote(payload(seed, 512))
            .expect("the promotion completes");
    }

    store.filesystem().clear();
    let candidates = store
        .unreferenced_candidates(&referencing(Vec::new()))
        .expect("the journal is readable");

    assert_eq!(
        candidates.len(),
        25,
        "every promoted chunk is a candidate until something claims it"
    );
    assert_eq!(
        store.filesystem().count("list_dir"),
        0,
        "finding candidates listed a directory, so it scales with the store rather than with \
         recent work; the operations were {:?}",
        store.filesystem().names()
    );
    assert_eq!(
        store.filesystem().count("read"),
        1,
        "exactly one file is read — the journal; the operations were {:?}",
        store.filesystem().names()
    );
}

#[test]
fn a_retained_chunk_stops_being_a_candidate() {
    let root = TempRoot::new("unref-retain");
    let store = Cas::open(root.path()).expect("the store opens");

    let kept = store
        .promote(payload(31, 512))
        .expect("the promotion completes")
        .digest();
    let dropped = store
        .promote(payload(32, 512))
        .expect("the promotion completes")
        .digest();

    store
        .journal()
        .record_retained(&kept)
        .expect("the retention records");

    assert_eq!(
        store
            .unreferenced_candidates(&referencing(Vec::new()))
            .expect("the journal is readable"),
        vec![dropped],
        "the chunk a transaction committed a reference to is no longer a collection candidate"
    );
}

#[test]
fn the_oracle_can_protect_a_chunk_the_journal_still_lists() {
    let root = TempRoot::new("unref-oracle");
    let store = Cas::open(root.path()).expect("the store opens");

    let referenced = store
        .promote(payload(33, 512))
        .expect("the promotion completes")
        .digest();
    let orphan = store
        .promote(payload(34, 512))
        .expect("the promotion completes")
        .digest();

    // The journal has not been told, but durable state has the reference. The oracle is the
    // authority and the journal is only the candidate list — which is what makes a torn or lost
    // retention record safe rather than fatal.
    assert_eq!(
        store
            .unreferenced_candidates(&referencing(vec![referenced]))
            .expect("the journal is readable"),
        vec![orphan]
    );
}

#[test]
fn compaction_preserves_the_candidate_set_and_shrinks_the_journal() {
    let root = TempRoot::new("unref-compact");
    let store = Cas::open(root.path()).expect("the store opens");

    let mut promoted = Vec::new();
    for seed in 40..60 {
        promoted.push(
            store
                .promote(payload(seed, 256))
                .expect("the promotion completes")
                .digest(),
        );
    }
    for digest in promoted.iter().take(15) {
        store
            .journal()
            .record_retained(digest)
            .expect("the retention records");
    }

    let before = std::fs::metadata(store.layout().arrival_journal())
        .expect("the journal exists")
        .len();
    let expected = store
        .journal()
        .candidates()
        .expect("the journal is readable");

    let written = store.journal().compact().expect("the journal compacts");
    assert_eq!(written, expected.len());

    let after = std::fs::metadata(store.layout().arrival_journal())
        .expect("the journal exists")
        .len();
    assert!(
        after < before,
        "compaction left the journal at {after} bytes, no smaller than the {before} it started at"
    );
    assert_eq!(
        store
            .journal()
            .candidates()
            .expect("the journal is readable"),
        expected,
        "compaction changed which chunks are candidates, which would make it a data-losing \
         operation rather than a bookkeeping one"
    );
}

/// A crash need not truncate. If the filesystem persists the *end* of a straddling append and not
/// its beginning, the tail is newline-terminated garbage rather than a short line — the case the
/// "ignore the unterminated tail" rule does not cover. It must fail loudly, because the alternative
/// is a collector deleting a chunk on a misread.
#[test]
fn a_terminated_but_garbled_tail_is_reported_rather_than_guessed_at() {
    let root = TempRoot::new("unref-garbled");
    let store = Cas::open(root.path()).expect("the store opens");
    store
        .promote(payload(65, 256))
        .expect("the promotion completes");
    store
        .promote(payload(66, 256))
        .expect("the promotion completes");

    // Zero the first half of the final record, keeping its newline: what a hole left by an
    // out-of-order block flush looks like.
    let path = store.layout().arrival_journal();
    let mut bytes = std::fs::read(&path).expect("the journal is readable");
    let length = bytes.len();
    for byte in &mut bytes[length - 67..length - 30] {
        *byte = 0;
    }
    std::fs::write(&path, &bytes).expect("the journal is writable");

    assert!(
        matches!(
            store.journal().candidates(),
            Err(CasError::JournalMalformed { line: 2, .. })
        ),
        "a garbled but terminated tail must be reported; the recovery is a full sweep, not a guess"
    );
    // And the recovery the report points at does work.
    assert_eq!(
        store
            .sweep_all_chunks()
            .expect("the store can be swept")
            .len(),
        2,
        "the backstop rebuilds what the damaged journal can no longer answer"
    );
}

#[test]
fn a_torn_final_record_is_ignored_and_the_rest_is_read() {
    let root = TempRoot::new("unref-torn");
    let store = Cas::open(root.path()).expect("the store opens");

    let first = store
        .promote(payload(61, 256))
        .expect("the promotion completes")
        .digest();
    let second = store
        .promote(payload(62, 256))
        .expect("the promotion completes")
        .digest();

    // Cut the last record short, exactly as a crash between `write` and its completion would.
    let path = store.layout().arrival_journal();
    let bytes = std::fs::read(&path).expect("the journal is readable");
    std::fs::write(&path, &bytes[..bytes.len() - 20]).expect("the journal is writable");

    assert_eq!(
        store
            .journal()
            .candidates()
            .expect("the journal is readable"),
        vec![first],
        "the torn record is dropped and the intact one survives"
    );
    // The chunk whose record was torn is still in the store; it has simply stopped being visible
    // to the cheap path, which is the loss the full sweep exists to bound.
    assert!(store.contains(&second));
    assert!(store
        .sweep_all_chunks()
        .expect("the store can be swept")
        .contains(&second));
}

#[test]
fn a_malformed_record_that_is_not_the_tail_is_reported_rather_than_skipped() {
    let root = TempRoot::new("unref-malformed");
    let store = Cas::open(root.path()).expect("the store opens");
    store
        .promote(payload(63, 256))
        .expect("the promotion completes");
    store
        .promote(payload(64, 256))
        .expect("the promotion completes");

    let path = store.layout().arrival_journal();
    let bytes = std::fs::read(&path).expect("the journal is readable");
    let mut damaged = b"this is not a record\n".to_vec();
    damaged.extend_from_slice(&bytes);
    std::fs::write(&path, &damaged).expect("the journal is writable");

    assert!(
        matches!(
            store.journal().candidates(),
            Err(CasError::JournalMalformed { line: 1, .. })
        ),
        "damage in the body of the journal is real damage; skipping it would silently turn a \
         retained chunk into a collection candidate"
    );
}

#[test]
fn the_full_sweep_is_the_backstop_when_the_journal_is_lost() {
    let root = TempRoot::new("unref-backstop");
    let store = Cas::open(root.path()).expect("the store opens");

    let mut promoted = Vec::new();
    for seed in 70..80 {
        promoted.push(
            store
                .promote(payload(seed, 256))
                .expect("the promotion completes")
                .digest(),
        );
    }
    promoted.sort_unstable();

    std::fs::remove_file(store.layout().arrival_journal()).expect("the journal is removable");

    assert!(
        store
            .unreferenced_candidates(&referencing(Vec::new()))
            .expect("a missing journal is not an error")
            .is_empty(),
        "with the journal gone the cheap path finds nothing — it is an index, and an index can be \
         lost"
    );
    assert_eq!(
        store.sweep_all_chunks().expect("the store can be swept"),
        promoted,
        "the full scan still finds every chunk, which is why losing the journal costs a scan and \
         not any content"
    );
}

#[test]
fn the_full_sweep_ignores_files_that_are_not_content_named() {
    let root = TempRoot::new("unref-strays");
    let store = Cas::open(root.path()).expect("the store opens");
    let digest = store
        .promote(payload(81, 256))
        .expect("the promotion completes")
        .digest();

    let stray = store.layout().chunk_directory(&digest).join("README");
    std::fs::write(&stray, b"not a chunk").expect("the stray is writable");
    std::fs::write(store.layout().chunks_directory().join(".DS_Store"), b"junk")
        .expect("the stray is writable");

    assert_eq!(
        store.sweep_all_chunks().expect("the store can be swept"),
        vec![digest],
        "a sweep reports chunks, and a file whose name is not a digest was not put there by this \
         crate"
    );
}
