//! Resumption, integrity refusal, and restart coverage for incoming missing chunks.

use std::fs;
use std::path::PathBuf;

use mesh_cas::{
    Blake3, Cas, ContentDigest as _, ReceiveError, ReceiveProgress, INCOMING_DIRECTORY_NAME,
};

fn scratch(name: &str) -> PathBuf {
    let mut root = std::env::temp_dir();
    root.push(format!(
        "mesh-cas-transfer-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("mkdir");
    root
}

#[test]
fn restart_resumes_at_the_durable_offset_without_resending_the_prefix() {
    let root = scratch("restart");
    let bytes: Vec<u8> = (0..=255).cycle().take(8_193).collect();
    let digest = Blake3::digest_bytes(&bytes);

    {
        let cas = Cas::open(&root).expect("open");
        let mut incoming = cas.begin_receive(digest).expect("begin");
        assert_eq!(incoming.next_offset(), 0);
        assert_eq!(
            incoming.accept(0, &bytes[..3_017], false).expect("part"),
            ReceiveProgress::Continue { next_offset: 3_017 }
        );
    }

    let cas = Cas::open(&root).expect("reopen");
    let mut resumed = cas.begin_receive(digest).expect("resume");
    assert_eq!(resumed.next_offset(), 3_017);
    assert!(matches!(
        resumed.accept(3_017, &bytes[3_017..], true),
        Ok(ReceiveProgress::Complete { digest: got, .. }) if got == digest
    ));
    assert_eq!(cas.read(&digest).expect("verified content"), bytes);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn corrupt_complete_bytes_are_refused_discarded_and_requested_again_from_zero() {
    let root = scratch("corrupt");
    let honest = b"the complete honest chunk".to_vec();
    let digest = Blake3::digest_bytes(&honest);
    let mut corrupt = honest.clone();
    corrupt[7] ^= 0x40;
    let cas = Cas::open(&root).expect("open");

    let mut incoming = cas.begin_receive(digest).expect("begin");
    let refusal = incoming.accept(0, &corrupt, true).expect_err("refused");
    assert!(matches!(
        refusal,
        ReceiveError::IntegrityMismatch {
            expected,
            retry_from: 0,
            ..
        } if expected == digest
    ));
    assert!(
        !cas.contains(&digest),
        "corrupt bytes were never materialized"
    );
    assert_eq!(cas.begin_receive(digest).expect("retry").next_offset(), 0);

    let mut retry = cas.begin_receive(digest).expect("retry again");
    assert!(matches!(
        retry.accept(0, &honest, true),
        Ok(ReceiveProgress::Complete { digest: got, .. }) if got == digest
    ));
    assert_eq!(cas.read(&digest).expect("read"), honest);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn skipped_replayed_and_non_advancing_parts_are_refused_without_mutation() {
    let root = scratch("offsets");
    let bytes = b"abcdefgh";
    let digest = Blake3::digest_bytes(bytes);
    let cas = Cas::open(&root).expect("open");
    let mut incoming = cas.begin_receive(digest).expect("begin");

    assert!(matches!(
        incoming.accept(1, b"bc", false),
        Err(ReceiveError::OffsetMismatch {
            expected: 0,
            received: 1
        })
    ));
    assert!(matches!(
        incoming.accept(0, b"", false),
        Err(ReceiveError::NoProgress { offset: 0 })
    ));
    incoming.accept(0, &bytes[..3], false).expect("prefix");
    assert!(matches!(
        incoming.accept(0, &bytes[..3], false),
        Err(ReceiveError::OffsetMismatch {
            expected: 3,
            received: 0
        })
    ));
    assert_eq!(incoming.next_offset(), 3);
    assert!(!cas.contains(&digest));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn restart_after_promotion_cleans_a_stale_partial_and_refuses_more_bytes() {
    let root = scratch("promoted-before-cleanup");
    let bytes = b"already promoted".to_vec();
    let digest = Blake3::digest_bytes(&bytes);
    let cas = Cas::open(&root).expect("open");
    let incoming = cas.layout().incoming_path(&digest);
    fs::write(&incoming, &bytes[..4]).expect("simulate durable stale partial");
    cas.promote(bytes.clone()).expect("promote");
    drop(cas);

    let cas = Cas::open(&root).expect("restart");
    let mut resumed = cas.begin_receive(digest).expect("recognize complete");
    assert!(resumed.is_complete());
    assert_eq!(resumed.next_offset(), bytes.len() as u64);
    assert!(!incoming.exists(), "stale partial was removed");
    assert!(matches!(
        resumed.accept(bytes.len() as u64, b"more", true),
        Err(ReceiveError::AlreadyComplete { digest: got }) if got == digest
    ));
    assert!(root.join(INCOMING_DIRECTORY_NAME).is_dir());
    assert_eq!(cas.read(&digest).expect("still intact"), bytes);
    let _ = fs::remove_dir_all(root);
}
