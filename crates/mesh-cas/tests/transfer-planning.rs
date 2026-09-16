//! Integrity-aware availability and bounded resumable request planning.

use std::fs;
use std::path::PathBuf;

use mesh_cas::{Blake3, Cas, ContentDigest as _, Digest32, TransferPlanError};

fn scratch(name: &str) -> PathBuf {
    let mut root = std::env::temp_dir();
    root.push(format!(
        "mesh-transfer-plan-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("mkdir");
    root
}

fn digest(byte: u8) -> Digest32 {
    Blake3::digest_bytes(&[byte; 32])
}

#[test]
fn verified_present_resumable_and_missing_chunks_are_classified_once_and_batched() {
    let root = scratch("mixed");
    let cas = Cas::open(&root).expect("open");
    let present_bytes = [1; 32];
    let present = Blake3::digest_bytes(&present_bytes);
    cas.promote(present_bytes.to_vec()).expect("promote");

    let partial = digest(2);
    {
        let mut incoming = cas.begin_receive(partial).expect("begin partial");
        incoming.accept(0, &[2; 11], false).expect("partial");
    }
    let missing = digest(3);
    let another = digest(4);

    let plan = cas
        .plan_missing_chunks([missing, present, partial, another, partial], 2, 64 * 1024)
        .expect("plan");
    assert_eq!(plan.verified(), &[present]);
    assert_eq!(
        plan.missing_chunks(),
        3,
        "duplicate manifest digest planned once"
    );
    assert_eq!(plan.batches().len(), 2);
    assert!(plan
        .batches()
        .iter()
        .all(|batch| batch.requests().len() <= 2));

    let requests: Vec<_> = plan
        .batches()
        .iter()
        .flat_map(|batch| batch.requests())
        .copied()
        .collect();
    assert!(requests
        .windows(2)
        .all(|pair| pair[0].digest() < pair[1].digest()));
    assert_eq!(
        requests
            .iter()
            .find(|request| request.digest() == partial)
            .expect("partial")
            .from_offset(),
        11
    );
    assert!(requests
        .iter()
        .all(|request| request.max_bytes() == 64 * 1024));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn restart_reconstructs_the_same_resume_plan_from_durable_partial_lengths() {
    let root = scratch("restart");
    let wanted = digest(7);
    {
        let cas = Cas::open(&root).expect("open");
        cas.begin_receive(wanted)
            .expect("begin")
            .accept(0, &[7; 19], false)
            .expect("partial");
    }

    let reopened = Cas::open(&root).expect("restart");
    let plan = reopened
        .plan_missing_chunks([wanted], 8, 1_024)
        .expect("plan");
    let request = &plan.batches()[0].requests()[0];
    assert_eq!(request.digest(), wanted);
    assert_eq!(request.from_offset(), 19);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn corrupt_promoted_content_is_signalled_quarantined_and_requested_from_zero() {
    let root = scratch("corrupt");
    let honest = b"honest content".to_vec();
    let wanted = Blake3::digest_bytes(&honest);
    let cas = Cas::open(&root).expect("open");
    cas.promote(honest).expect("promote");
    fs::write(cas.layout().chunk_path(&wanted), b"corrupt content").expect("rot chunk");
    fs::write(cas.layout().incoming_path(&wanted), b"stale partial").expect("stale partial");

    let plan = cas
        .plan_missing_chunks([wanted], 4, 4_096)
        .expect("plan despite corruption");
    assert_eq!(plan.corruptions().len(), 1);
    assert_eq!(plan.corruptions()[0].digest(), wanted);
    assert!(plan.corruptions()[0].quarantined().exists());
    assert!(!cas.contains(&wanted));
    assert!(!cas.layout().incoming_path(&wanted).exists());
    let request = &plan.batches()[0].requests()[0];
    assert_eq!(request.digest(), wanted);
    assert_eq!(request.from_offset(), 0);

    let next = cas
        .plan_missing_chunks([wanted], 4, 4_096)
        .expect("next plan");
    assert!(
        next.corruptions().is_empty(),
        "same corruption is not reported twice"
    );
    assert_eq!(next.batches()[0].requests()[0].from_offset(), 0);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn zero_batch_bounds_are_refused_before_availability_is_inspected() {
    let root = scratch("bounds");
    let wanted = digest(9);
    let cas = Cas::open(&root).expect("open");
    assert!(matches!(
        cas.plan_missing_chunks([wanted], 0, 1),
        Err(TransferPlanError::ZeroRequestsPerBatch)
    ));
    assert!(matches!(
        cas.plan_missing_chunks([wanted], 1, 0),
        Err(TransferPlanError::ZeroBytesPerPart)
    ));
    assert!(!cas.layout().incoming_path(&wanted).exists());
    let _ = fs::remove_dir_all(root);
}
