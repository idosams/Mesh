use super::*;
use crate::fleet::catalog::{AttachedFleetRequest, NativeFleetDirectory};
use crate::fleet::Limits;
use crate::project_attachment::{AttachmentStorage, ObservationLimits};
use ed25519_dalek::{Signer as _, SigningKey};
use std::{fs, os::unix::fs::PermissionsExt as _};

#[test]
fn ingestion_runtime_keeps_native_catalogue_authority_without_holding_service_lock() {
    let root = std::env::temp_dir().join(format!("mesh-ingestion-service-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    for name in ["source", "metadata", "fleets"] {
        let path = root.join(name);
        fs::create_dir(&path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::write(root.join("source/work.txt"), b"original work\n").unwrap();
    let attachment = AttachmentStorage::open(&root.join("metadata"))
        .unwrap()
        .provision(&root.join("source"))
        .unwrap();
    let capture = attachment
        .project()
        .capture_inputs(ObservationLimits::default())
        .unwrap();
    let key = SigningKey::from_bytes(&[61; 32]);
    let version = attachment
        .project()
        .save_capture(
            attachment.metadata_path(),
            &capture,
            PublicKey::from_bytes(key.verifying_key().to_bytes()),
            |p| Ok::<_, String>(Signature::from_bytes(key.sign(p.as_bytes()).to_bytes())),
        )
        .unwrap()
        .operation();
    let catalog_path = root.join("fleets");
    let catalog = NativeFleetDirectory::open(
        &catalog_path,
        TrustedReviewers::default(),
        CheckpointRuntimeParameters::selected_defaults(),
    )
    .unwrap();
    let service = catalog
        .create_attached(
            &attachment,
            &AttachedFleetRequest {
                request: "a".repeat(32),
                goal: "Coordinate".into(),
                version,
                limits: Limits {
                    lanes: 4,
                    concurrency: 2,
                    depth: 1,
                    retries: 1,
                },
            },
        )
        .unwrap();
    let objective = service.objective().unwrap();
    let history = catalog.history(&objective).unwrap();
    let before = service.native_state().unwrap();
    history
        .with_ingestion_runtime(|runtime| {
            assert!(service.inner.try_lock().is_ok());
            assert_eq!(runtime.state(), &before);
            assert_eq!(service.native_state().unwrap(), before);
            service
                .native_command("cancel-during-ingestion", Command::Cancel)
                .unwrap();
            runtime.refresh().unwrap();
            assert!(runtime.state().cancelled);
            assert_eq!(runtime.state(), &service.native_state().unwrap());
            Ok(())
        })
        .unwrap();
    assert_eq!(
        fs::read(root.join("source/work.txt")).unwrap(),
        b"original work\n"
    );
    drop(history);
    drop(service);
    drop(catalog);
    let reopened = NativeFleetDirectory::open(
        &catalog_path,
        TrustedReviewers::default(),
        CheckpointRuntimeParameters::selected_defaults(),
    )
    .unwrap();
    let history = reopened.history(&objective).unwrap();
    assert!(reopened.current_service(&objective).is_err());
    history
        .with_ingestion_runtime(|runtime| {
            assert!(runtime.state().cancelled);
            assert!(history.0.inner.try_lock().is_ok());
            Ok(())
        })
        .unwrap();
    fs::rename(&catalog_path, root.join("preserved-fleets")).unwrap();
    fs::create_dir(&catalog_path).unwrap();
    assert!(history
        .with_ingestion_runtime::<()>(|_| panic!("changed catalogue must refuse before receiving"))
        .is_err());
    assert_eq!(fs::read_dir(&catalog_path).unwrap().count(), 0);
    drop(history);
    drop(reopened);
    drop(attachment);
    fs::remove_dir_all(root).unwrap();
}
