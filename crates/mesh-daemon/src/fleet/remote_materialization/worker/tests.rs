use super::*;
use crate::fleet::{
    Limits, NativeRemoteInputReceiver, RemoteAdmissionOutcome, RemoteAdmissionRegistry,
    RemoteAssignment, RemoteInputChunk, RemoteWork,
};
use mesh_store::{fleet::FleetStore, RecordDigest};
use std::os::unix::fs::symlink;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
const ID: &str = "0123456789abcdef0123456789abcdef";
const BYTES: &[u8] = b"\x00received\xff";
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "mesh-received-workspace-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        for name in ["store", "allocations", "protected"] {
            fs::create_dir(root.join(name)).unwrap();
            fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(root)
    }
    fn allocation(
        &self,
        manifest: RemoteInputManifest,
        objective: &str,
        reserved: bool,
    ) -> RemoteInputAllocation {
        let destination = RemoteInputDestination::admit(
            &self.0.join("store"),
            ProtectedWorkspaceRoot::inspect(&self.0.join("store")).unwrap(),
            &self.0.join("allocations"),
            ProtectedWorkspaceRoot::inspect(&self.0.join("allocations")).unwrap(),
            &[ProtectedWorkspaceRoot::inspect(&self.0.join("protected")).unwrap()],
        )
        .unwrap();
        let assignment = RemoteAssignment {
            id: "assignment".into(),
            worker_key: "ab".repeat(32),
            input: manifest.input(),
            bundle: manifest.bundle(),
            lease_sequence: 1,
            lease_until_ms: 1000,
        };
        let has_bytes = manifest.entries().iter().any(
            |entry| matches!(entry, RemoteInputEntry::File { chunks, .. } if !chunks.is_empty()),
        );
        let mut receiver =
            NativeRemoteInputReceiver::new(&destination, manifest, &assignment).unwrap();
        if has_bytes {
            receiver
                .accept(Blake3::digest_bytes(BYTES), 0, BYTES, true)
                .unwrap();
        }
        if !reserved {
            return receiver.materialize(ID).unwrap();
        }
        let mut registry = RemoteAdmissionRegistry::new(
            FleetStore::open(self.0.join("worker.sqlite")).unwrap(),
            &"cd".repeat(32),
            &"ab".repeat(32),
            objective,
            Limits {
                lanes: 1,
                concurrency: 1,
                depth: 0,
                retries: 0,
            },
        )
        .unwrap();
        let work = RemoteWork {
            lane: "lane".into(),
            run: "run".into(),
            assignment,
            provider: "codex".into(),
            goal: "Independent task".into(),
        };
        let RemoteAdmissionOutcome::Reserved(reservation) =
            registry.reserve(work, ID, 100).unwrap()
        else {
            panic!("original reservation required")
        };
        receiver.materialize_reserved(reservation).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn manifest(empty: bool) -> RemoteInputManifest {
    RemoteInputManifest::new(
        RecordDigest::from_bytes([7; 32]),
        if empty {
            vec![]
        } else {
            vec![
                RemoteInputEntry::Directory {
                    path: "empty-directory".into(),
                },
                RemoteInputEntry::Directory { path: "src".into() },
                RemoteInputEntry::File {
                    path: "empty".into(),
                    executable: false,
                    digest: Blake3::digest_bytes(&[]),
                    chunks: vec![],
                },
                RemoteInputEntry::File {
                    path: "src/tool".into(),
                    executable: true,
                    digest: Blake3::digest_bytes(BYTES),
                    chunks: vec![RemoteInputChunk {
                        digest: Blake3::digest_bytes(BYTES),
                        bytes: BYTES.len() as u64,
                    }],
                },
            ]
        },
    )
    .unwrap()
}
fn initialize(input: RemoteInputAllocation) -> io::Result<ReceivedWorkerWorkspace> {
    input.into_worker_workspace(
        TrustedReviewers::default(),
        CheckpointRuntimeParameters::selected_defaults(),
    )
}

#[test]
fn verified_input_initializes_independent_history_with_durable_exact_scope() {
    let fixture = Fixture::new();
    let input = fixture.allocation(manifest(false), "objective-A", true);
    let source = input.path().to_owned();
    let parent = source.parent().unwrap().to_owned();
    let workspace = initialize(input).unwrap();
    workspace.verify().unwrap();
    assert_eq!(workspace.admission().objective(), "objective-A");
    assert_eq!(workspace.admission().coordinator(), "cd".repeat(32));
    assert_ne!(
        workspace.binding().starting_version().unwrap(),
        manifest(false).input()
    );
    let root = Path::new(workspace.binding().root());
    assert_eq!(fs::read(root.join("src/tool")).unwrap(), BYTES);
    assert_eq!(
        fs::metadata(root.join("src/tool"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0o100
    );
    assert!(root.join("empty-directory").is_dir());
    assert_eq!(fs::read(root.join("empty")).unwrap(), b"");
    assert_eq!(fs::read(source.join("src/tool")).unwrap(), BYTES);
    let receipt = workspace.receipt().encode();
    assert_eq!(fs::read_to_string(parent.join(RECEIPT)).unwrap(), receipt);
    assert_eq!(
        workspace
            .receipt()
            .get("context")
            .unwrap()
            .get("source_input")
            .unwrap()
            .as_text(),
        Some(manifest(false).input().to_string().as_str())
    );
    let initial = workspace.binding().starting_version().unwrap();
    let root = root.to_owned();
    drop(workspace);
    // Reopen native history for inspection only; no execution handle or new initialization grant.
    let daemon = LiveDaemon::with_trusted_reviewers_and_checkpoint_runtime(
        StartupSummary::from(&nothing_to_recover()),
        TrustedReviewers::default(),
        CheckpointRuntimeParameters::selected_defaults(),
    )
    .unwrap();
    daemon.reopen_at_start(&root).unwrap();
    let state = daemon.workspace_state().unwrap();
    assert_eq!(state.workspace_versions.len(), 1);
    assert_eq!(state.workspace_versions[0].operation(), initial);
    assert_eq!(fs::read_to_string(parent.join(RECEIPT)).unwrap(), receipt);
}

#[test]
fn empty_received_tree_still_has_an_independent_initial_operation() {
    let fixture = Fixture::new();
    let input = fixture.allocation(manifest(true), "empty", true);
    assert!(matches!(
        crate::preview_folder_import(input.path()),
        Err(crate::FolderImportError::NoImportableEntries { .. })
    ));
    let workspace = initialize(input).unwrap();
    workspace.verify().unwrap();
    assert!(workspace.binding().starting_version().is_some());
    assert_ne!(
        workspace.binding().starting_version().unwrap(),
        manifest(true).input()
    );
}

#[test]
fn changed_input_and_unreserved_materialization_cannot_initialize() {
    for reserved in [false, true] {
        let fixture = Fixture::new();
        let input = fixture.allocation(manifest(false), "objective", reserved);
        let parent = input.path().parent().unwrap().to_owned();
        if reserved {
            fs::write(input.path().join("src/tool"), b"changed").unwrap();
        }
        assert!(initialize(input).is_err());
        assert!(!parent.join(INTENT).exists());
        assert!(!parent.join("workspace.mesh").exists());
        assert!(!parent.join(RECEIPT).exists());
    }
}

#[test]
fn existing_intent_or_destination_is_preserved_without_repair() {
    for marker in [INTENT, "workspace.mesh"] {
        let fixture = Fixture::new();
        let input = fixture.allocation(manifest(false), "objective", true);
        let parent = input.path().parent().unwrap().to_owned();
        fs::write(parent.join(marker), b"retained unknown state").unwrap();
        assert!(initialize(input).is_err());
        assert_eq!(
            fs::read(parent.join(marker)).unwrap(),
            b"retained unknown state"
        );
        assert!(!parent.join(RECEIPT).exists());
        assert_eq!(fs::read(parent.join("files/src/tool")).unwrap(), BYTES);
    }
}

#[test]
fn changed_mapping_and_worker_escape_invalidate_retained_workspace() {
    let fixture = Fixture::new();
    let input = fixture.allocation(manifest(false), "objective", true);
    let parent = input.path().parent().unwrap().to_owned();
    let workspace = initialize(input).unwrap();
    for name in [INTENT, RECEIPT] {
        let original = fs::read(parent.join(name)).unwrap();
        fs::write(parent.join(name), b"changed context").unwrap();
        assert!(workspace.verify().is_err());
        fs::write(parent.join(name), original).unwrap();
        workspace.verify().unwrap();
    }
    let moved = fixture.0.join("protected/escaped.mesh");
    fs::rename(parent.join("workspace.mesh"), &moved).unwrap();
    symlink(&moved, parent.join("workspace.mesh")).unwrap();
    assert!(workspace.verify().is_err());
    assert!(moved.exists());
    assert_eq!(fs::read(parent.join("files/src/tool")).unwrap(), BYTES);
}
