//! Promote, then read: the bytes that come out are the bytes that went in.
//!
//! The task's test list asks for this as a property rather than an example, so it is checked over a
//! spread of sizes chosen to land on the boundaries a tree hasher and a block-oriented filesystem
//! are most likely to get wrong — zero, one, exactly a BLAKE3 chunk, one either side of it, page
//! and block multiples, and plan §6.2's chunk-policy sizes of 64 KiB, 256 KiB and 1 MiB.
//!
//! Seeds are fixed, so a failure names a case that can be re-run rather than one that has to be
//! reproduced by luck.

mod support;

use std::collections::BTreeSet;

use mesh_cas::{Blake3, Cas, ContentDigest};

use support::{Bytes, TempRoot};

/// Sizes chosen for where they sit, not for their spread.
const SIZES: &[usize] = &[
    0,         // empty content is content
    1,         // one byte
    63,        // one under a compression block
    64,        // one compression block
    65,        // one over
    1023,      // one under a BLAKE3 chunk
    1024,      // exactly one BLAKE3 chunk
    1025,      // one over, so the chaining-value stack is used
    4095,      // one under a page
    4096,      // one page
    65_536,    // plan §6.2's minimum chunk
    262_144,   // plan §6.2's average chunk
    1_048_576, // plan §6.2's maximum chunk
];

#[test]
fn every_size_promotes_and_reads_back_byte_for_byte() {
    let root = TempRoot::new("roundtrip-sizes");
    let store = Cas::open(root.path()).expect("the store opens");

    for (index, size) in SIZES.iter().copied().enumerate() {
        let bytes = Bytes::seeded(300 + index as u64).take(size);
        let digest = store
            .promote(bytes.clone())
            .unwrap_or_else(|error| panic!("promoting {size} bytes failed: {error}"))
            .digest();

        assert_eq!(
            digest,
            Blake3::digest_bytes(&bytes),
            "the name a chunk gets is the digest of its bytes, at size {size}"
        );
        let read = store
            .read(&digest)
            .unwrap_or_else(|error| panic!("reading {size} bytes back failed: {error}"));
        assert_eq!(read, bytes, "the bytes read back differ at size {size}");
    }
}

#[test]
fn many_chunks_coexist_without_colliding() {
    let root = TempRoot::new("roundtrip-many");
    let store = Cas::open(root.path()).expect("the store opens");
    let mut generator = Bytes::seeded(404);

    let mut written = Vec::new();
    for _ in 0..200 {
        let size = generator.below(3_000) as usize;
        let bytes = generator.take(size);
        let digest = store
            .promote(bytes.clone())
            .expect("the promotion completes")
            .digest();
        written.push((digest, bytes));
    }

    for (digest, bytes) in &written {
        assert_eq!(
            &store.read(digest).expect("each chunk verifies"),
            bytes,
            "chunk {digest} came back different after two hundred promotions"
        );
    }

    let swept: BTreeSet<_> = store
        .sweep_all_chunks()
        .expect("the store can be swept")
        .into_iter()
        .collect();
    let expected: BTreeSet<_> = written.iter().map(|(digest, _)| *digest).collect();
    assert_eq!(
        swept, expected,
        "a full sweep finds exactly the chunks that were promoted, no more and no fewer"
    );
}

#[test]
fn promoting_the_same_bytes_twice_is_idempotent() {
    let root = TempRoot::new("roundtrip-idempotent");
    let store = Cas::open(root.path()).expect("the store opens");
    let bytes = Bytes::seeded(505).take(9_000);

    let first = store
        .promote(bytes.clone())
        .expect("the first promotion completes")
        .digest();
    let second = store
        .promote(bytes.clone())
        .expect("the second promotion completes")
        .digest();

    assert_eq!(first, second);
    assert_eq!(store.read(&first).expect("the chunk verifies"), bytes);
    assert_eq!(
        store.sweep_all_chunks().expect("the store can be swept"),
        vec![first],
        "the same content stored twice is one chunk, which is what content addressing is for"
    );
    assert_eq!(
        store.discard_scratch().expect("scratch is readable"),
        0,
        "the redundant promotion cleaned up after itself"
    );
}

#[test]
fn a_re_promotion_does_not_disturb_the_existing_chunk() {
    let root = TempRoot::new("roundtrip-nodisturb");
    let store = Cas::open(root.path()).expect("the store opens");
    let bytes = Bytes::seeded(606).take(4_096);

    let digest = store
        .promote(bytes.clone())
        .expect("the first promotion completes")
        .digest();
    let path = store.layout().chunk_path(&digest);
    let before = std::fs::metadata(&path).expect("the chunk exists");

    let mut promotion = store.begin_promotion(bytes.clone());
    promotion
        .run_through(mesh_cas::PromotionStep::SyncDirectory)
        .expect("the second promotion completes");
    assert!(
        promotion.was_already_present(),
        "the second promotion recognised the chunk was already there"
    );

    let after = std::fs::metadata(&path).expect("the chunk still exists");
    assert_eq!(
        before.len(),
        after.len(),
        "a re-promotion must not rewrite a chunk something may be reading"
    );
    assert_eq!(store.read(&digest).expect("the chunk verifies"), bytes);
}
