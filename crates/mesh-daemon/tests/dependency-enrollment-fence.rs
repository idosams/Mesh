//! A daemon opened before preparation cannot keep writing through its cached workspace.
#![cfg(unix)]
use mesh_cas::{Blake3, ContentDigest};
use mesh_daemon::ipc::surface::{nothing_to_recover, Operations, StartupSummary};
use mesh_daemon::{
    CheckpointRuntimeParameters, DependencyEnrollmentFence, LiveDaemon, OpenWorkspace,
    PreparedFolderImport,
};
use mesh_store::RecordDigest;
use mesh_types::{PublicKey, Signature};
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn cached_native_writer_refuses_after_fence_without_file_or_journal_changes() {
    let base = std::env::temp_dir().join(format!(
        "mesh-enrollment-cached-reader-{}",
        std::process::id()
    ));
    std::fs::create_dir(&base).unwrap();
    let source = base.join("source");
    let root = base.join("workspace");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("note.txt"), b"original\n").unwrap();
    let (confirmed, _) = PreparedFolderImport::prepare(&source, &root)
        .unwrap()
        .confirm_into_workspace()
        .unwrap();
    drop(confirmed);
    let open = OpenWorkspace::open(&root).unwrap();
    let journal = open.record_file().to_path_buf();
    drop(open);
    let daemon = LiveDaemon::with_checkpoint_runtime(
        StartupSummary::from(&nothing_to_recover()),
        CheckpointRuntimeParameters {
            idle_interval: Some(std::time::Duration::from_millis(1)),
            maximum_uncheckpointed_bytes: Some(65_536),
            maximum_uncheckpointed_interval: Some(std::time::Duration::from_secs(3600)),
        },
    )
    .unwrap();
    daemon.open_at_start(&root).unwrap();
    let shown = Operations::workspace_state(&daemon).unwrap();
    let digest = RecordDigest::from_bytes(*Blake3::digest_bytes(b"original\n").as_bytes());
    daemon
        .preserve_managed_text_edit("note.txt", "before fence\n", digest, false)
        .unwrap();
    let before_file = std::fs::read(root.join("note.txt")).unwrap();
    let before_journal = std::fs::read(&journal).unwrap();
    let fence = DependencyEnrollmentFence::prepare(
        &root,
        &shown.installation,
        RecordDigest::from_bytes([1; 32]),
    )
    .unwrap();
    fence.ensure_current().unwrap();
    drop(fence);
    let digest = RecordDigest::from_bytes(*Blake3::digest_bytes(&before_file).as_bytes());
    assert!(daemon
        .preserve_managed_text_edit("note.txt", "after fence\n", digest, false)
        .is_err());
    let signer_called = AtomicBool::new(false);
    assert!(daemon
        .create_managed_text_file("new.txt", "no", PublicKey::from_bytes([7; 32]), |_| {
            signer_called.store(true, Ordering::SeqCst);
            Err::<Signature, &str>("signing must not run")
        })
        .is_err());
    assert!(!signer_called.load(Ordering::SeqCst));
    assert!(!root.join("new.txt").exists());
    assert!(daemon
        .open_current_review_for_workspace(
            &shown.root,
            &shown.digest,
            &shown.installation,
            PublicKey::from_bytes([7; 32])
        )
        .is_err());
    assert!(daemon
        .acquire_workspace_agent_custody(
            &shown.root,
            &shown.digest,
            &shown.installation,
            false,
            None
        )
        .is_err());
    assert_eq!(std::fs::read(root.join("note.txt")).unwrap(), before_file);
    assert_eq!(std::fs::read(&journal).unwrap(), before_journal);
    assert_eq!(
        std::fs::read(source.join("note.txt")).unwrap(),
        b"original\n"
    );
    drop(daemon);
    std::fs::remove_dir_all(base).unwrap();
}
