use super::*;
use mesh_store::fleet::{FleetStore, FleetStoreAuthority, FleetStoreError};
use mesh_store::RecordDigest;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug)]
struct TestAuthority {
    path: PathBuf,
    directory: fs::File,
    database: fs::File,
}
impl FleetStoreAuthority for TestAuthority {
    fn check(&self) -> Result<(), FleetStoreError> {
        let verify = || -> std::io::Result<bool> {
            let root = fs::symlink_metadata(self.path.parent().unwrap())?;
            let file = fs::symlink_metadata(&self.path)?;
            let held_root = self.directory.metadata()?;
            let held_file = self.database.metadata()?;
            Ok(root.is_dir()
                && !root.file_type().is_symlink()
                && file.is_file()
                && !file.file_type().is_symlink()
                && file.nlink() == 1
                && (root.dev(), root.ino()) == (held_root.dev(), held_root.ino())
                && (file.dev(), file.ino()) == (held_file.dev(), held_file.ino()))
        };
        if verify().unwrap_or(false) {
            Ok(())
        } else {
            Err(FleetStoreError::AuthorityChanged)
        }
    }
}
pub(in crate::fleet) fn guarded(s: &Setup) -> RemoteAdmissionRegistry {
    drop(s.f.registry());
    let path = s.f.path.canonicalize().unwrap().join("worker.sqlite");
    let authority = TestAuthority {
        directory: fs::File::open(path.parent().unwrap()).unwrap(),
        database: fs::File::open(&path).unwrap(),
        path: path.clone(),
    };
    RemoteAdmissionRegistry::new(
        FleetStore::open_guarded(&path, false, Arc::new(authority)).unwrap(),
        &RecordDigest::from_bytes(s.f.coordinator.verifying_key().to_bytes()).to_string(),
        &RecordDigest::from_bytes(s.f.worker.verifying_key().to_bytes()).to_string(),
        "objective",
        crate::fleet::Limits {
            lanes: 2,
            concurrency: 1,
            depth: 1,
            retries: 1,
        },
    )
    .unwrap()
}
fn session(s: &Setup) -> RemoteReceivingSession<'_> {
    RemoteReceivingSession::new(guarded(s), s.f.work.clone(), ALLOCATION, &s.destination)
}
pub(in crate::fleet) fn materialized(
    s: &Setup,
    runtime: &mut Runtime,
) -> (RemoteInputAllocation, RemoteAdmissionRegistry) {
    let mut session = session(s);
    let mut connection = session.connect().unwrap();
    let signature = s.sign(&connection, runtime);
    connection.authenticate(&signature).unwrap();
    connection.receive(s.manifest_frame()).unwrap();
    connection.receive(s.part(0, s.bytes.len())).unwrap();
    connection.materialize().unwrap()
}
fn resume(
    s: &Setup,
    runtime: &mut Runtime,
) -> Result<
    (
        crate::fleet::ReceivedWorkerWorkspace,
        RemoteAdmissionRegistry,
    ),
    Error,
> {
    let mut session = session(s);
    let mut connection = session.connect().unwrap();
    let signature = s.sign(&connection, runtime);
    assert_eq!(
        connection.authenticate(&signature).unwrap(),
        RemoteReceivingAccess::Retained
    );
    connection.resume_initialization(
        TrustedReviewers::default(),
        CheckpointRuntimeParameters::selected_defaults(),
    )
}
fn allocation(s: &Setup) -> PathBuf {
    s.f.path
        .join("allocations")
        .join(format!("input-{ALLOCATION}"))
}
fn private_write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn authenticated_original_recovery_covers_pending_and_confirmed_initialization() {
    for phase in 0..5 {
        let s = Setup::new();
        let mut runtime = s.f.runtime(true);
        let (input, registry) = materialized(&s, &mut runtime);
        let root = allocation(&s);
        let mut original_receipt = None;
        if phase == 4 {
            let workspace = input
                .into_worker_workspace(
                    TrustedReviewers::default(),
                    CheckpointRuntimeParameters::selected_defaults(),
                )
                .unwrap();
            original_receipt = Some(workspace.receipt().encode());
            drop(workspace);
            let bytes = fs::read(root.join("workspace.json")).unwrap();
            private_write(&root.join("workspace.json"), &bytes[..bytes.len() / 2]);
        } else {
            if phase > 0 {
                let prepared = crate::PreparedFolderImport::prepare_received_with_parent(
                    input.path(),
                    &root.join("workspace.mesh"),
                    &[],
                    ProtectedWorkspaceRoot::inspect(&root).unwrap(),
                )
                .unwrap();
                if phase == 1 {
                    private_write(
                        &root
                            .join("workspace.mesh")
                            .join(crate::workspace::PRESENTED_DIRECTORY_NAME)
                            .join("result.txt"),
                        &s.bytes[..7],
                    );
                    drop(prepared);
                } else {
                    let owned = fs::read_dir(root.join("workspace.mesh"))
                        .unwrap()
                        .map(|entry| entry.unwrap().path())
                        .find(|path| {
                            path.file_name()
                                .unwrap()
                                .to_string_lossy()
                                .contains("owned")
                        })
                        .unwrap();
                    let ownership = fs::read(&owned).unwrap();
                    let (confirmed, _) = prepared.confirm_into_workspace_without_origin().unwrap();
                    if phase == 2 {
                        // Retain exact pending ownership and a torn initial journal, before receipt publication.
                        private_write(&owned, &ownership);
                        fs::remove_file(confirmed.receipt()).unwrap();
                        let journal = root
                            .join("workspace.mesh")
                            .join(crate::workspace::RECORD_FILE_NAME);
                        let bytes = fs::read(&journal).unwrap();
                        private_write(&journal, &bytes[..bytes.len() / 2]);
                    }
                }
            }
            drop(input);
        }
        drop(registry);
        let (workspace, registry) =
            resume(&s, &mut runtime).unwrap_or_else(|e| panic!("phase {phase}: {e:?}"));
        workspace.verify().unwrap();
        let operation = workspace.binding().starting_version();
        let installation = workspace.binding().installation.clone();
        let receipt = workspace.receipt().encode();
        if let Some(original) = original_receipt {
            assert_eq!(receipt, original);
        }
        assert_eq!(
            fs::read(
                root.join("workspace.mesh")
                    .join(crate::workspace::PRESENTED_DIRECTORY_NAME)
                    .join("result.txt")
            )
            .unwrap(),
            s.bytes
        );
        assert!(registry.launch_receipt("assignment").unwrap().is_none());
        assert!(
            resume(&s, &mut runtime).is_err(),
            "the existing initialization owner must exclude another resume"
        );
        drop((workspace, registry));
        let (again, registry) = resume(&s, &mut runtime).unwrap();
        assert_eq!(again.binding().starting_version(), operation);
        assert_eq!(again.binding().installation, installation);
        assert_eq!(again.receipt().encode(), receipt);
        assert!(registry.launch_receipt("assignment").unwrap().is_none());
    }
}

#[test]
fn original_recovery_requires_authentication_and_guarded_ledger() {
    let s = Setup::new();
    let mut runtime = s.f.runtime(true);
    let (input, registry) = materialized(&s, &mut runtime);
    drop((input, registry));
    let mut owned = session(&s);
    let mut unauthenticated = owned.connect().unwrap();
    assert!(unauthenticated
        .resume_initialization(
            TrustedReviewers::default(),
            CheckpointRuntimeParameters::selected_defaults()
        )
        .is_err());
    drop(unauthenticated);
    let mut unguarded = s.session();
    let mut connection = unguarded.connect().unwrap();
    let signature = s.sign(&connection, &mut runtime);
    connection.authenticate(&signature).unwrap();
    assert!(connection
        .resume_initialization(
            TrustedReviewers::default(),
            CheckpointRuntimeParameters::selected_defaults()
        )
        .is_err());
    assert!(!allocation(&s).join("initialization.json").exists());
    assert!(!allocation(&s).join("workspace.mesh").exists());
}

#[test]
fn original_recovery_preserves_changed_work_and_refuses_recorded_launch() {
    for launched in [false, true] {
        let s = Setup::new();
        let mut runtime = s.f.runtime(true);
        let (input, registry) = materialized(&s, &mut runtime);
        let workspace = input
            .into_worker_workspace(
                TrustedReviewers::default(),
                CheckpointRuntimeParameters::selected_defaults(),
            )
            .unwrap();
        if launched {
            drop(
                registry
                    .reserve_launch(
                        workspace,
                        "codex",
                        crate::fleet::service::received_clock().unwrap(),
                    )
                    .unwrap(),
            );
        } else {
            drop((workspace, registry));
            private_write(
                &allocation(&s)
                    .join("workspace.mesh")
                    .join(crate::workspace::PRESENTED_DIRECTORY_NAME)
                    .join("result.txt"),
                b"later user work",
            );
        }
        let receipt = fs::read(allocation(&s).join("workspace.json")).unwrap();
        assert!(resume(&s, &mut runtime).is_err());
        assert_eq!(
            fs::read(allocation(&s).join("workspace.json")).unwrap(),
            receipt
        );
        if !launched {
            assert_eq!(
                fs::read(
                    allocation(&s)
                        .join("workspace.mesh")
                        .join(crate::workspace::PRESENTED_DIRECTORY_NAME)
                        .join("result.txt")
                )
                .unwrap(),
                b"later user work"
            );
        }
    }
}

#[test]
fn original_recovery_refuses_replaced_or_unprivate_roots_and_conflicting_receipts() {
    for fault in 0..4 {
        let s = Setup::new();
        let mut runtime = s.f.runtime(true);
        let (input, registry) = materialized(&s, &mut runtime);
        let workspace = input
            .into_worker_workspace(
                TrustedReviewers::default(),
                CheckpointRuntimeParameters::selected_defaults(),
            )
            .unwrap();
        drop((workspace, registry));
        let root = allocation(&s);
        let store = root.join("workspace.mesh");
        let working = store.join(crate::workspace::PRESENTED_DIRECTORY_NAME);
        match fault {
            0 => {
                fs::rename(&working, root.join("retained-working")).unwrap();
                fs::create_dir(&working).unwrap();
                fs::set_permissions(&working, fs::Permissions::from_mode(0o700)).unwrap();
                private_write(&working.join("result.txt"), &s.bytes);
            }
            1 => fs::set_permissions(&store, fs::Permissions::from_mode(0o755)).unwrap(),
            2 => private_write(
                &root.join("workspace.json"),
                b"conflicting mapping to preserve",
            ),
            _ => private_write(&root.join("files/result.txt"), b"changed original input"),
        }
        let journal = fs::read(store.join(crate::workspace::RECORD_FILE_NAME)).unwrap();
        let receipt = fs::read(root.join("workspace.json")).unwrap();
        assert!(resume(&s, &mut runtime).is_err(), "fault {fault}");
        assert_eq!(
            fs::read(store.join(crate::workspace::RECORD_FILE_NAME)).unwrap(),
            journal
        );
        assert_eq!(fs::read(root.join("workspace.json")).unwrap(), receipt);
        assert_eq!(fs::read(working.join("result.txt")).unwrap(), s.bytes);
    }
}

#[test]
fn original_recovery_request_does_not_consume_an_active_transfer() {
    let s = Setup::new();
    let mut runtime = s.f.runtime(true);
    let mut session = session(&s);
    let mut connection = session.connect().unwrap();
    let signature = s.sign(&connection, &mut runtime);
    assert_eq!(
        connection.authenticate(&signature).unwrap(),
        RemoteReceivingAccess::Receiving
    );
    assert!(connection
        .resume_initialization(
            TrustedReviewers::default(),
            CheckpointRuntimeParameters::selected_defaults()
        )
        .is_err());
    connection.receive(s.manifest_frame()).unwrap();
    connection.receive(s.part(0, s.bytes.len())).unwrap();
    let (input, registry) = connection.materialize().unwrap();
    input.verify().unwrap();
    assert!(registry
        .materialization_receipt("assignment")
        .unwrap()
        .is_some());
    assert!(!allocation(&s).join("initialization.json").exists());
}
