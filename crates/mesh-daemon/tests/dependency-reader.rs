//! Until native policy validation exists, recognized storage records must never be ignored.
use mesh_daemon::{OpenFailure, OpenWorkspace, PreparedFolderImport};
use mesh_store::{
    ApprovalRecord, DependencyKind, DependencyRecord, RecordDigest, ReviewRecord, ReviewVerdict,
    StoredRecord,
};
use std::io::Write as _;
fn digest(byte: u8) -> RecordDigest {
    RecordDigest::from_bytes([byte; 32])
}
#[test]
fn incomplete_dependency_reader_preserves_history_and_refuses_cached_approval() {
    let base = std::env::temp_dir().join(format!("mesh-dependency-reader-{}", std::process::id()));
    std::fs::create_dir(&base).unwrap();
    let source = base.join("source");
    let workspace = base.join("workspace");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("note.txt"), b"saved work\n").unwrap();
    let (confirmed, imported) = PreparedFolderImport::prepare(&source, &workspace)
        .unwrap()
        .confirm_into_workspace()
        .unwrap();
    drop(confirmed);
    let mut open = OpenWorkspace::open(&workspace).unwrap();
    let path = open.record_file().to_path_buf();
    let before = std::fs::read(&path).unwrap();
    let dependency = StoredRecord::Dependency(DependencyRecord {
        authority: digest(1),
        revision: 1,
        previous: digest(0),
        payload: digest(2),
        kind: DependencyKind::Enrollment,
    });
    assert_eq!(
        open.append_record(&dependency).unwrap_err().kind(),
        std::io::ErrorKind::Unsupported
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    open.append_record(&StoredRecord::Review(ReviewRecord {
        bundle: digest(5),
        subject_operation: imported.operation(),
        opened_by: digest(6),
    }))
    .unwrap();
    let mut journal = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    journal
        .write_all(&mesh_store::frame_record(&dependency))
        .unwrap();
    journal.sync_all().unwrap();
    drop(journal);
    let enrolled = std::fs::read(&path).unwrap();
    let refused = open
        .append_record(&StoredRecord::Approval(ApprovalRecord {
            approval: digest(7),
            bundle: digest(5),
            approver: digest(6),
            verdict: ReviewVerdict::Approved,
        }))
        .unwrap_err();
    assert_eq!(refused.kind(), std::io::ErrorKind::Unsupported);
    assert_eq!(std::fs::read(&path).unwrap(), enrolled);
    drop(open);
    for _ in 0..2 {
        let failure = match OpenWorkspace::open(&workspace) {
            Err(error) => error,
            Ok(_) => panic!("incomplete reader accepted dependency history"),
        };
        assert!(matches!(
            failure,
            OpenFailure::DependencyPolicyUnavailable { .. }
        ));
        assert_eq!(failure.code(), "workspace-dependency-policy-unavailable");
        assert_eq!(
            failure.readable_boundary(),
            mesh_store::scan_journal(&enrolled).unwrap().boundary()
        );
        assert_eq!(std::fs::read(&path).unwrap(), enrolled);
    }
    assert_eq!(
        std::fs::read(source.join("note.txt")).unwrap(),
        b"saved work\n"
    );
    std::fs::remove_dir_all(base).unwrap();
}
