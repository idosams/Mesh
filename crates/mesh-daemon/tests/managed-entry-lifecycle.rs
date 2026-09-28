//! Authenticated native entry management over one real managed operating-system folder.

#![cfg(unix)]

use std::cell::Cell;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey};
use mesh_daemon::ipc::surface::{nothing_to_recover, Operations, StartupSummary};
use mesh_daemon::{
    CheckpointRuntimeParameters, LiveDaemon, ManagedTextFileError, OpenWorkspace,
    PreparedFolderImport,
};
use mesh_store::{scan_journal, StoredRecord};
use mesh_types::{PublicKey, Signature};

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "mesh-managed-entry-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}

fn parameters() -> CheckpointRuntimeParameters {
    CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(50)),
        maximum_uncheckpointed_bytes: Some(65_536),
        maximum_uncheckpointed_interval: Some(Duration::from_millis(25)),
    }
}

fn workspace(name: &str) -> (PathBuf, PathBuf) {
    let parent = scratch(name);
    let _ = fs::remove_dir_all(&parent);
    let source = parent.join("source");
    let managed = parent.join("managed");
    fs::create_dir_all(source.join("existing")).expect("source folders");
    fs::write(source.join("existing/keep.txt"), "keep\n").expect("source file");
    PreparedFolderImport::prepare(&source, &managed)
        .expect("verified import")
        .confirm_into_workspace()
        .expect("managed workspace");
    (parent, managed)
}

fn private_storage(workspace: &Path) -> PathBuf {
    workspace.join(".mesh")
}

fn open_daemon(managed: &Path) -> LiveDaemon {
    let daemon = LiveDaemon::with_checkpoint_runtime(
        StartupSummary::from(&nothing_to_recover()),
        parameters(),
    )
    .expect("checkpoint runtime");
    daemon.open_at_start(managed).expect("open managed folder");
    daemon
}

fn signer<'a>(
    key: &'a SigningKey,
) -> impl FnOnce(&mesh_crypto::SigningPayload) -> Result<Signature, core::convert::Infallible> + 'a
{
    |payload| {
        Ok(Signature::from_bytes(
            key.sign(payload.as_bytes()).to_bytes(),
        ))
    }
}

#[test]
fn direct_live_daemon_mutation_honors_shared_agent_custody() {
    let (parent, managed) = workspace("direct-custody");
    let daemon = open_daemon(&managed);
    let summary = daemon
        .current_workspace_summary()
        .expect("workspace summary");
    let generation = daemon
        .acquire_workspace_agent_custody(
            &summary.root,
            &summary.digest,
            &summary.installation,
            false,
            None,
        )
        .expect("acquire custody");
    let key = SigningKey::from_bytes(&[0x31; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let refused = daemon.create_managed_folder("agent-owned", public, signer(&key));
    assert!(matches!(refused, Err(ManagedTextFileError::Recovery(_))));
    assert!(!managed.join("agent-owned").exists());
    daemon
        .release_workspace_agent_custody(
            &summary.root,
            &summary.digest,
            &summary.installation,
            &generation,
        )
        .expect("release custody");
    daemon
        .create_managed_folder("released", public, signer(&key))
        .expect("mutation after release");
    assert!(managed.join("released").is_dir());
    fs::remove_dir_all(parent).unwrap();
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir(destination).expect("create clone root");
    for entry in fs::read_dir(source).expect("read clone source") {
        let entry = entry.expect("source entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let kind = entry.file_type().expect("entry type");
        if kind.is_dir() {
            copy_tree(&source_path, &destination_path);
        } else if kind.is_file() {
            fs::copy(&source_path, &destination_path).expect("copy clone file");
        } else {
            panic!("workspace clone contained a non-file entry");
        }
    }
}

#[test]
fn a_byte_identical_root_clone_cannot_inherit_an_open_mutation() {
    let (parent, managed) = workspace("root-clone-authority");
    // Materialize every database/private directory before taking the byte-identical copy.
    drop(OpenWorkspace::open(&managed).expect("materialize workspace"));
    let replacement = parent.join("replacement");
    copy_tree(&managed, &replacement);
    let daemon = open_daemon(&managed);
    let key = SigningKey::from_bytes(&[0x6b; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let original = parent.join("opened-original");
    let original_journal =
        fs::read(private_storage(&managed).join("records.mesh")).expect("original journal");
    let replacement_journal =
        fs::read(private_storage(&replacement).join("records.mesh")).expect("clone journal");

    let refused = daemon.create_managed_text_file(
        "must-not-appear.txt",
        "authority stays with the opened root\n",
        public,
        |payload| {
            // This is arbitrary caller code at the precise pre-mutation boundary.
            fs::rename(&managed, &original).expect("move opened root away");
            fs::rename(&replacement, &managed).expect("install byte-identical clone");
            Ok::<_, core::convert::Infallible>(Signature::from_bytes(
                key.sign(payload.as_bytes()).to_bytes(),
            ))
        },
    );
    assert!(matches!(refused, Err(ManagedTextFileError::Recovery(_))));
    assert!(!managed.join("must-not-appear.txt").exists());
    assert!(!original.join("must-not-appear.txt").exists());
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh"))
            .expect("clone journal after refusal"),
        replacement_journal
    );
    assert_eq!(
        fs::read(private_storage(&original).join("records.mesh"))
            .expect("opened journal after refusal"),
        original_journal
    );
    assert!(!managed.join(".mesh-managed-mutation").exists());
    assert!(!original.join(".mesh-managed-mutation").exists());

    daemon
        .open_workspace(&managed.display().to_string())
        .expect("explicitly admit replacement root");
    daemon
        .create_managed_text_file(
            "after-explicit-open.txt",
            "new authority\n",
            public,
            signer(&key),
        )
        .expect("explicitly admitted replacement can mutate");
    assert_eq!(
        fs::read(managed.join("after-explicit-open.txt")).expect("new file"),
        b"new authority\n"
    );
    assert!(!original.join("after-explicit-open.txt").exists());
    let _ = fs::remove_dir_all(parent);
}

#[test]
fn create_move_and_delete_are_authenticated_durable_and_restart_exact() {
    let (parent, managed) = workspace("roundtrip");
    let key = SigningKey::from_bytes(&[0x61; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let daemon = open_daemon(&managed);

    let folder = daemon
        .create_managed_folder("notes", public, signer(&key))
        .expect("authenticated folder");
    assert!(folder.meaningful_saved());
    assert!(folder.author_authenticated());
    assert!(managed.join("notes").is_dir());

    let file = daemon
        .create_managed_text_file("draft.txt", "hello from Mesh\n", public, signer(&key))
        .expect("authenticated file");
    assert!(!managed.join(".mesh-managed-mutation").exists());
    assert!(file.meaningful_saved());
    assert_eq!(
        fs::read(managed.join("draft.txt")).unwrap(),
        b"hello from Mesh\n"
    );
    let object_before_move = daemon
        .workspace_state()
        .unwrap()
        .file_histories
        .into_iter()
        .find(|history| history.path() == "draft.txt")
        .expect("created history")
        .object()
        .to_string();

    let moved = daemon
        .move_managed_entry_privately("draft.txt", "notes/final.txt", public, signer(&key))
        .expect("stable-identity move");
    assert!(moved.meaningful_saved());
    assert!(!managed.join("draft.txt").exists());
    assert_eq!(
        fs::read(managed.join("notes/final.txt")).unwrap(),
        b"hello from Mesh\n"
    );
    let object_after_move = daemon
        .workspace_state()
        .unwrap()
        .file_histories
        .into_iter()
        .find(|history| history.path() == "notes/final.txt")
        .expect("moved history")
        .object()
        .to_string();
    assert_eq!(object_after_move, object_before_move);

    let inspected = daemon
        .inspect_managed_file("notes/final.txt")
        .expect("inspect before delete");
    let deleted = daemon
        .delete_managed_entry_privately(
            "notes/final.txt",
            Some(inspected.content_digest()),
            Some(inspected.executable()),
            public,
            signer(&key),
        )
        .expect("delete file");
    assert!(deleted.meaningful_saved());
    assert!(!managed.join("notes/final.txt").exists());
    daemon
        .delete_managed_entry_privately("notes", None, None, public, signer(&key))
        .expect("delete empty folder");
    assert!(!managed.join("notes").exists());

    let records =
        scan_journal(&fs::read(private_storage(&managed).join("records.mesh")).unwrap()).unwrap();
    let authored = records
        .records()
        .iter()
        .filter_map(|record| match record {
            StoredRecord::Operation(operation)
                if operation.actor.as_bytes() == public.as_bytes() =>
            {
                Some(operation.actor_sequence)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(authored, vec![1, 2, 3, 4, 5]);
    assert!(!file.manifest().is_empty());

    drop(daemon);
    let restarted = open_daemon(&managed);
    let state = restarted.workspace_state().expect("restart state");
    assert!(state
        .entries
        .iter()
        .any(|entry| entry.path() == "existing/keep.txt"));
    assert!(!state
        .entries
        .iter()
        .any(|entry| entry.path().contains("draft") || entry.path().contains("notes")));
    let opened = OpenWorkspace::open(&managed).expect("second independent rebuild");
    assert_eq!(opened.operations(), 6);
    assert_eq!(
        opened.manifests(),
        2,
        "deleted file content remains retained"
    );
    assert!(!opened
        .conditions()
        .iter()
        .any(|condition| { condition.code().starts_with("managed-mutation-") }));
    let _ = fs::remove_dir_all(parent);
}

#[test]
fn a_review_card_projects_every_change_in_the_computed_bundle() {
    let (parent, managed) = workspace("review-whole-bundle");
    let key = SigningKey::from_bytes(&[0x72; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let daemon = open_daemon(&managed);

    daemon
        .create_managed_text_file("first.txt", "first saved file\n", public, signer(&key))
        .expect("first authenticated save");
    daemon
        .create_managed_text_file("second.txt", "second saved file\n", public, signer(&key))
        .expect("second authenticated save");

    let reviewed = daemon.workspace_state().expect("automatic review source");
    assert_eq!(
        reviewed.reviews, 0,
        "automatic presentation must not claim that a person recorded a review"
    );
    let json = reviewed.to_json().encode();

    assert!(json.contains("\"content_complete\":true"), "{json}");
    assert!(json.contains("\"recorded\":false"), "{json}");
    assert!(json.contains("\"bundle_changes\":["), "{json}");
    assert!(json.contains("\"path_after\":\"/existing\""), "{json}");
    assert!(
        json.contains("\"path_after\":\"/existing/keep.txt\""),
        "{json}"
    );
    assert!(json.contains("\"path_after\":\"/first.txt\""), "{json}");
    assert!(json.contains("\"path_after\":\"/second.txt\""), "{json}");
    assert!(json.contains("\"presentation_digest\":"), "{json}");
    let reviewed_subject = reviewed
        .workspace_versions
        .last()
        .expect("latest saved version")
        .operation()
        .to_string();
    let subject_field = format!("\"subject_operation\":\"{reviewed_subject}\"");
    let candidate_card = reviewed
        .review_items
        .iter()
        .map(mesh_daemon::ipc::Json::encode)
        .find(|item| item.contains(&subject_field))
        .expect("latest automatic review card");
    assert!(candidate_card.contains("\"recorded\":false"));
    assert!(
        candidate_card.contains("\"verified_text\":"),
        "the hash-verified UTF-8 bytes must remain readable in the review card: {candidate_card}"
    );
    assert!(
        candidate_card.contains("\"text\":\"first saved file\""),
        "the review projection dropped the verified text body: {candidate_card}"
    );

    let before_recording = reviewed.records;
    let recorded = daemon
        .open_current_review_for_workspace(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            public,
        )
        .expect("record the exact automatic card");
    assert_eq!(
        recorded.records,
        before_recording + 1,
        "recording the computed card appends one review record"
    );
    assert_eq!(recorded.reviews, 1);
    let original_card = recorded
        .review_items
        .iter()
        .map(mesh_daemon::ipc::Json::encode)
        .find(|item| item.contains(&subject_field))
        .expect("recorded review card");
    assert!(original_card.contains("\"recorded\":true"));

    let retried = daemon
        .open_current_review_for_workspace(
            &recorded.root,
            &recorded.digest,
            &recorded.installation,
            public,
        )
        .expect("reuse the already recorded card");
    assert_eq!(retried.records, recorded.records);

    daemon
        .create_managed_text_file("later.txt", "saved after review\n", public, signer(&key))
        .expect("later authenticated save");
    let after_later_save = daemon.workspace_state().expect("historical review card");
    assert_eq!(
        after_later_save.reviews, 1,
        "later work produces a new automatic candidate, not a record storm"
    );
    assert!(after_later_save
        .review_items
        .iter()
        .map(mesh_daemon::ipc::Json::encode)
        .any(|item| item.contains("\"recorded\":false")));
    let historical_card = after_later_save
        .review_items
        .iter()
        .map(mesh_daemon::ipc::Json::encode)
        .find(|item| item.contains(&subject_field))
        .expect("historical automatic review card");
    assert_eq!(
        historical_card, original_card,
        "later private work must not rewrite the already recorded review presentation"
    );

    let _ = fs::remove_dir_all(parent);
}

#[test]
fn artifact_preview_reconstructs_only_the_exact_reviewed_object_and_side() {
    let parent = scratch("artifact-review-bytes");
    let _ = fs::remove_dir_all(&parent);
    let source = parent.join("source");
    let managed = parent.join("managed");
    fs::create_dir_all(source.join("finance")).expect("source folders");
    let pdf = b"%PDF-1.7\nexact reviewed board pack\n";
    fs::write(source.join("finance/board-pack.pdf"), pdf).expect("source artifact");
    let prepared = PreparedFolderImport::prepare(&source, &managed).expect("verified import");
    let (confirmed, imported) = prepared
        .confirm_into_workspace()
        .expect("managed workspace");
    drop(confirmed);
    let daemon = open_daemon(&managed);
    let shown = daemon.workspace_state().expect("review source");
    let reviewed = daemon
        .open_current_review_for_workspace(
            &shown.root,
            &shown.digest,
            &shown.installation,
            PublicKey::from_bytes([0x72; 32]),
        )
        .expect("record exact review");
    let card = reviewed.review_items.first().expect("review card");
    let bundle = card
        .get("bundle")
        .and_then(mesh_daemon::ipc::Json::as_text)
        .expect("bundle");
    let change = card
        .get("bundle_changes")
        .and_then(mesh_daemon::ipc::Json::as_array)
        .expect("changes")
        .iter()
        .find(|change| {
            change
                .get("path_after")
                .and_then(mesh_daemon::ipc::Json::as_text)
                == Some("/finance/board-pack.pdf")
        })
        .expect("artifact change");
    let object = change
        .get("object_id")
        .and_then(mesh_daemon::ipc::Json::as_text)
        .expect("artifact object");
    let after = change.get("after").expect("after summary");
    let artifact = daemon
        .review_artifact_for_workspace(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            bundle,
            &imported.operation().to_string(),
            object,
            "after",
        )
        .expect("exact reviewed artifact");

    assert_eq!(artifact.path(), "/finance/board-pack.pdf");
    assert_eq!(artifact.bytes(), pdf);
    assert_eq!(
        artifact.version().to_string(),
        after
            .get("version_id")
            .and_then(mesh_daemon::ipc::Json::as_text)
            .expect("reviewed version")
    );
    assert_eq!(
        artifact.digest().to_string(),
        after
            .get("content_digest")
            .and_then(mesh_daemon::ipc::Json::as_text)
            .expect("reviewed digest")
    );
    fs::write(
        managed.join("finance/board-pack.pdf"),
        b"unreviewed native replacement",
    )
    .expect("mutate working copy after review");
    assert_eq!(
        daemon
            .review_artifact_for_workspace(
                &reviewed.root,
                &reviewed.digest,
                &reviewed.installation,
                bundle,
                &imported.operation().to_string(),
                object,
                "after",
            )
            .expect("preview remains bound to immutable CAS")
            .bytes(),
        pdf
    );
    assert_eq!(
        daemon
            .review_artifact_for_workspace(
                &reviewed.root,
                &reviewed.digest,
                &reviewed.installation,
                bundle,
                &imported.operation().to_string(),
                object,
                "before",
            )
            .expect_err("genesis has no before artifact")
            .code,
        "review-artifact-unavailable"
    );
    assert_eq!(
        daemon
            .review_artifact_for_workspace(
                &reviewed.root,
                "stale-workspace-digest",
                &reviewed.installation,
                bundle,
                &imported.operation().to_string(),
                object,
                "after",
            )
            .expect_err("stale workspace cannot read artifact")
            .code,
        "stale-workspace"
    );
    let _ = fs::remove_dir_all(parent);
}

#[test]
fn delete_refuses_when_the_file_changed_after_confirmation() {
    let (parent, managed) = workspace("delete-stale-inspection");
    let key = SigningKey::from_bytes(&[0x69; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let daemon = open_daemon(&managed);
    let inspected = daemon
        .inspect_managed_file("existing/keep.txt")
        .expect("inspect the confirmed file");
    let journal_before =
        fs::read(private_storage(&managed).join("records.mesh")).expect("journal before");
    let state_before = daemon.workspace_state().expect("state before");

    fs::write(
        managed.join("existing/keep.txt"),
        b"new unsaved editor work\n",
    )
    .expect("external editor change");

    let signed = Cell::new(false);
    let refused = daemon.delete_managed_entry_privately(
        "existing/keep.txt",
        Some(inspected.content_digest()),
        Some(inspected.executable()),
        public,
        |payload| {
            signed.set(true);
            Ok::<_, core::convert::Infallible>(Signature::from_bytes(
                key.sign(payload.as_bytes()).to_bytes(),
            ))
        },
    );
    assert!(matches!(
        refused,
        Err(ManagedTextFileError::StaleInspection)
    ));
    assert_eq!(
        fs::read(managed.join("existing/keep.txt")).expect("preserved editor bytes"),
        b"new unsaved editor work\n"
    );
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).expect("journal after"),
        journal_before
    );
    assert_eq!(
        daemon.workspace_state().expect("state after").digest,
        state_before.digest
    );
    assert!(!signed.get(), "stale bytes reached the signing boundary");
    assert!(!managed.join(".mesh-managed-mutation").exists());
    let _ = fs::remove_dir_all(parent);
}

#[test]
fn delete_refuses_when_executable_metadata_changed_after_confirmation() {
    let (parent, managed) = workspace("delete-stale-executable");
    let key = SigningKey::from_bytes(&[0x68; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let daemon = open_daemon(&managed);
    let path = managed.join("existing/keep.txt");
    let inspected = daemon
        .inspect_managed_file("existing/keep.txt")
        .expect("inspect the confirmed file");
    let journal_before =
        fs::read(private_storage(&managed).join("records.mesh")).expect("journal before");
    let state_before = daemon.workspace_state().expect("state before");

    let mut permissions = fs::metadata(&path).expect("metadata").permissions();
    permissions.set_mode(permissions.mode() | 0o100);
    fs::set_permissions(&path, permissions).expect("make file executable after confirmation");

    let signed = Cell::new(false);
    let refused = daemon.delete_managed_entry_privately(
        "existing/keep.txt",
        Some(inspected.content_digest()),
        Some(inspected.executable()),
        public,
        |payload| {
            signed.set(true);
            Ok::<_, core::convert::Infallible>(Signature::from_bytes(
                key.sign(payload.as_bytes()).to_bytes(),
            ))
        },
    );
    assert!(matches!(
        refused,
        Err(ManagedTextFileError::StaleInspection)
    ));
    assert!(path.is_file(), "the changed file remains present");
    assert_ne!(
        fs::metadata(&path)
            .expect("metadata after refusal")
            .permissions()
            .mode()
            & 0o111,
        0,
        "the changed executable state remains untouched"
    );
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).expect("journal after"),
        journal_before
    );
    assert_eq!(
        daemon.workspace_state().expect("state after").digest,
        state_before.digest
    );
    assert!(!signed.get(), "stale metadata reached the signing boundary");
    assert!(!managed.join(".mesh-managed-mutation").exists());
    let _ = fs::remove_dir_all(parent);
}

#[test]
fn empty_folder_delete_refuses_an_entry_created_during_confirmation() {
    let (parent, managed) = workspace("delete-directory-race");
    let key = SigningKey::from_bytes(&[0x6a; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let daemon = open_daemon(&managed);
    daemon
        .create_managed_folder("notes", public, signer(&key))
        .expect("empty managed folder");
    let journal_before =
        fs::read(private_storage(&managed).join("records.mesh")).expect("journal before");
    let state_before = daemon.workspace_state().expect("state before");

    let refused = daemon.delete_managed_entry_privately("notes", None, None, public, |payload| {
        // Signing happens after the initial emptiness check and before staging. This is the exact
        // window an editor or another local process can win without coordinating with Mesh.
        fs::write(
            managed.join("notes/arrived-during-confirmation.txt"),
            b"new unsaved work\n",
        )
        .expect("external file arrives during confirmation");
        Ok::<_, core::convert::Infallible>(Signature::from_bytes(
            key.sign(payload.as_bytes()).to_bytes(),
        ))
    });

    assert!(matches!(
        refused,
        Err(ManagedTextFileError::DirectoryNotEmpty)
    ));
    assert_eq!(
        fs::read(managed.join("notes/arrived-during-confirmation.txt"))
            .expect("new work remains at the reviewed path"),
        b"new unsaved work\n"
    );
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).expect("journal after"),
        journal_before,
        "the refused deletion must not become durable history"
    );
    assert_eq!(
        daemon.workspace_state().expect("state after").digest,
        state_before.digest
    );
    assert!(!managed.join(".mesh-managed-mutation").exists());
    assert!(
        fs::read_dir(&managed)
            .expect("workspace entries")
            .all(|entry| !entry
                .expect("workspace entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".mesh-delete-")),
        "the changed folder must not remain hidden under the private staging name"
    );
    let _ = fs::remove_dir_all(parent);
}

// The move-settling replacement regression lives in live.rs unit tests so a test-only hook
// places the external change after durable move publication and before settling. An observer
// thread racing a 15 ms sleep against the 50 ms idle interval cannot establish that ordering.

#[test]
fn invalid_signature_and_confined_refusals_change_no_folder_or_journal_byte() {
    let (parent, managed) = workspace("refusals");
    let key = SigningKey::from_bytes(&[0x62; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let daemon = open_daemon(&managed);
    let journal = fs::read(private_storage(&managed).join("records.mesh")).unwrap();

    let invalid = daemon.create_managed_text_file("bad.txt", "bad\n", public, |_payload| {
        Ok::<_, core::convert::Infallible>(Signature::from_bytes([0xFF; 64]))
    });
    assert!(matches!(invalid, Err(ManagedTextFileError::Authoring(_))));
    assert!(!managed.join("bad.txt").exists());
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        journal
    );

    let invalid_move =
        daemon.move_managed_entry_privately("existing/keep.txt", "moved.txt", public, |_payload| {
            Ok::<_, core::convert::Infallible>(Signature::from_bytes([0xFF; 64]))
        });
    assert!(matches!(
        invalid_move,
        Err(ManagedTextFileError::Authoring(_))
    ));
    assert!(managed.join("existing/keep.txt").is_file());
    assert!(!managed.join("moved.txt").exists());
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        journal
    );

    let missing_delete_precondition = daemon.delete_managed_entry_privately(
        "existing/keep.txt",
        None,
        None,
        public,
        signer(&key),
    );
    assert!(matches!(
        missing_delete_precondition,
        Err(ManagedTextFileError::StaleInspection)
    ));
    assert_eq!(
        fs::read(managed.join("existing/keep.txt")).unwrap(),
        b"keep\n"
    );
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        journal
    );

    let inspected = daemon
        .inspect_managed_file("existing/keep.txt")
        .expect("inspect before invalid signature");
    let invalid_delete = daemon.delete_managed_entry_privately(
        "existing/keep.txt",
        Some(inspected.content_digest()),
        Some(inspected.executable()),
        public,
        |_payload| Ok::<_, core::convert::Infallible>(Signature::from_bytes([0xFF; 64])),
    );
    assert!(matches!(
        invalid_delete,
        Err(ManagedTextFileError::Authoring(_))
    ));
    assert_eq!(
        fs::read(managed.join("existing/keep.txt")).unwrap(),
        b"keep\n"
    );
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        journal
    );

    assert!(matches!(
        daemon.create_managed_folder("../escape", public, signer(&key)),
        Err(ManagedTextFileError::InvalidPath | ManagedTextFileError::Authoring(_))
    ));
    assert!(matches!(
        daemon.create_managed_text_file("existing/keep.txt", "replace", public, signer(&key)),
        Err(ManagedTextFileError::TargetExists | ManagedTextFileError::Authoring(_))
    ));
    assert!(matches!(
        daemon.delete_managed_entry_privately("existing", None, None, public, signer(&key)),
        Err(ManagedTextFileError::DirectoryNotEmpty)
    ));
    assert_eq!(
        fs::read(managed.join("existing/keep.txt")).unwrap(),
        b"keep\n"
    );
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        journal
    );
    let _ = fs::remove_dir_all(parent);
}

#[test]
fn agent_checkpoint_preserves_custody_and_private_history_for_tracked_and_new_files() {
    let (parent, managed) = workspace("agent-checkpoint");
    let daemon = open_daemon(&managed);
    let initial = daemon.workspace_state().unwrap();
    let generation = daemon
        .acquire_workspace_agent_custody(
            &initial.root,
            &initial.digest,
            &initial.installation,
            false,
            None,
        )
        .unwrap();
    let key = SigningKey::from_bytes(&[0x41; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    for (path, new_file) in [("existing/keep.txt", false), ("added.txt", true)] {
        fs::write(managed.join(path), "agent-authored content\n").unwrap();
        let preflight = daemon
            .inspect_agent_finish_preflight(
                &initial.root,
                &daemon.workspace_state().unwrap().digest,
                &initial.installation,
                &generation,
            )
            .unwrap();
        let (digest, executable) = if new_file {
            let entry = preflight
                .native_files()
                .iter()
                .find(|entry| entry.path() == path)
                .unwrap();
            (entry.content_digest(), entry.executable())
        } else {
            let entry = preflight
                .managed_files()
                .iter()
                .find(|entry| entry.path() == path)
                .unwrap();
            (entry.content_digest(), entry.executable())
        };
        let before = daemon.workspace_state().unwrap();
        let receipt = daemon
            .checkpoint_agent_file(
                mesh_daemon::AgentFileCheckpointRequest {
                    root: &before.root,
                    digest: &before.digest,
                    installation: &before.installation,
                    generation: &generation,
                    path,
                    content_digest: digest,
                    executable,
                    new_file,
                },
                public,
                signer(&key),
            )
            .unwrap();
        assert!(receipt.author_authenticated());
        assert!(receipt.meaningful_saved());
        assert_eq!(
            fs::read_to_string(managed.join(path)).unwrap(),
            "agent-authored content\n"
        );
        let after = daemon.workspace_state().unwrap();
        assert_eq!(after.shared_version, initial.shared_version);
        assert_eq!(
            daemon
                .workspace_agent_custody_for_workspace(
                    &after.root,
                    &after.digest,
                    &after.installation,
                )
                .unwrap()
                .generation(),
            Some(generation.as_str())
        );
        assert!(daemon
            .create_managed_folder("not-authorized", public, signer(&key))
            .is_err());
    }
    drop(daemon);
    let reopened = open_daemon(&managed);
    assert!(!reopened
        .inspect_managed_file("existing/keep.txt")
        .unwrap()
        .modified_from_current_version());
    assert!(!reopened
        .inspect_managed_file("added.txt")
        .unwrap()
        .modified_from_current_version());
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn agent_checkpoint_refuses_stale_custody_and_changed_bytes_before_signing() {
    let (parent, managed) = workspace("agent-checkpoint-stale");
    let daemon = open_daemon(&managed);
    let state = daemon.workspace_state().unwrap();
    let generation = daemon
        .acquire_workspace_agent_custody(
            &state.root,
            &state.digest,
            &state.installation,
            false,
            None,
        )
        .unwrap();
    let inspection = daemon.inspect_managed_file("existing/keep.txt").unwrap();
    fs::write(managed.join("existing/keep.txt"), "later bytes\n").unwrap();
    let key = SigningKey::from_bytes(&[0x42; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let journal = fs::read(private_storage(&managed).join("records.mesh")).unwrap();
    for session in [generation.as_str(), "stale-generation"] {
        let result = daemon.checkpoint_agent_file(
            mesh_daemon::AgentFileCheckpointRequest {
                root: &state.root,
                digest: &state.digest,
                installation: &state.installation,
                generation: session,
                path: "existing/keep.txt",
                content_digest: inspection.content_digest(),
                executable: inspection.executable(),
                new_file: false,
            },
            public,
            |_| -> Result<Signature, &'static str> { panic!("stale capture reached signing") },
        );
        assert!(result.is_err());
        assert_eq!(
            fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
            journal
        );
    }
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn agent_checkpoint_signer_cannot_reuse_mutation_authority() {
    let (parent, managed) = workspace("agent-checkpoint-signer");
    let daemon = open_daemon(&managed);
    let state = daemon.workspace_state().unwrap();
    let generation = daemon
        .acquire_workspace_agent_custody(
            &state.root,
            &state.digest,
            &state.installation,
            false,
            None,
        )
        .unwrap();
    fs::write(managed.join("existing/keep.txt"), "private edit\n").unwrap();
    let inspection = daemon.inspect_managed_file("existing/keep.txt").unwrap();
    let key = SigningKey::from_bytes(&[0x43; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let receipt = daemon
        .checkpoint_agent_file(
            mesh_daemon::AgentFileCheckpointRequest {
                root: &state.root,
                digest: &state.digest,
                installation: &state.installation,
                generation: &generation,
                path: "existing/keep.txt",
                content_digest: inspection.content_digest(),
                executable: inspection.executable(),
                new_file: false,
            },
            public,
            |payload| {
                assert!(daemon
                    .create_managed_folder("smuggled", public, signer(&key))
                    .is_err());
                assert!(daemon
                    .open_current_review_for_workspace(
                        &state.root,
                        &state.digest,
                        &state.installation,
                        public,
                    )
                    .is_err());
                Ok::<_, core::convert::Infallible>(Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap();
    assert!(receipt.meaningful_saved());
    assert!(!managed.join("smuggled").exists());
    let after = daemon.workspace_state().unwrap();
    daemon
        .release_workspace_agent_custody(
            &after.root,
            &after.digest,
            &after.installation,
            &generation,
        )
        .unwrap();
    daemon
        .create_managed_folder("human-after-release", public, signer(&key))
        .unwrap();
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn failed_agent_signer_leaves_journal_and_authority_recoverable() {
    let (parent, managed) = workspace("agent-checkpoint-sign-failure");
    let daemon = open_daemon(&managed);
    let state = daemon.workspace_state().unwrap();
    let generation = daemon
        .acquire_workspace_agent_custody(
            &state.root,
            &state.digest,
            &state.installation,
            false,
            None,
        )
        .unwrap();
    fs::write(
        managed.join("existing/keep.txt"),
        "retryable private edit\n",
    )
    .unwrap();
    let inspection = daemon.inspect_managed_file("existing/keep.txt").unwrap();
    let key = SigningKey::from_bytes(&[0x44; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let request = || mesh_daemon::AgentFileCheckpointRequest {
        root: &state.root,
        digest: &state.digest,
        installation: &state.installation,
        generation: &generation,
        path: "existing/keep.txt",
        content_digest: inspection.content_digest(),
        executable: inspection.executable(),
        new_file: false,
    };
    let journal = fs::read(private_storage(&managed).join("records.mesh")).unwrap();
    assert!(daemon
        .checkpoint_agent_file(request(), public, |_| Err::<Signature, _>(
            "signer unavailable"
        ))
        .is_err());
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        journal
    );
    assert!(daemon
        .create_managed_folder("still-agent-owned", public, signer(&key))
        .is_err());
    assert!(daemon
        .checkpoint_agent_file(request(), public, signer(&key))
        .unwrap()
        .meaningful_saved());
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn agent_workspace_checkpoint_saves_nested_additions_and_retries_without_duplicates() {
    let (parent, managed) = workspace("agent-workspace-checkpoint");
    let daemon = open_daemon(&managed);
    let state = daemon.workspace_state().unwrap();
    let generation = daemon
        .acquire_workspace_agent_custody(
            &state.root,
            &state.digest,
            &state.installation,
            false,
            None,
        )
        .unwrap();
    fs::write(managed.join("existing/keep.txt"), "edited\n").unwrap();
    fs::create_dir_all(managed.join("new/deeper")).unwrap();
    fs::write(managed.join("new/deeper/added.txt"), "new contents\n").unwrap();
    let key = SigningKey::from_bytes(&[0x51; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let mut first_changes = Vec::new();
    for attempt in 0..2 {
        let current = daemon.workspace_state().unwrap();
        let report = daemon
            .checkpoint_agent_workspace(
                mesh_daemon::AgentWorkspaceCheckpointRequest {
                    root: &current.root,
                    digest: &current.digest,
                    installation: &current.installation,
                    generation: &generation,
                },
                public,
                |payload| {
                    Ok::<_, core::convert::Infallible>(Signature::from_bytes(
                        key.sign(payload.as_bytes()).to_bytes(),
                    ))
                },
            )
            .unwrap();
        assert!(report.complete, "{:?}", report.issue);
        assert_eq!(report.workspace.shared_version, state.shared_version);
        if attempt == 0 {
            assert_eq!(report.saved_changes.len(), 4);
            first_changes = report.saved_changes;
        } else {
            assert!(report.saved_changes.is_empty());
        }
    }
    assert!(!daemon
        .inspect_managed_file("existing/keep.txt")
        .unwrap()
        .modified_from_current_version());
    assert!(!daemon
        .inspect_managed_file("new/deeper/added.txt")
        .unwrap()
        .modified_from_current_version());
    let current = daemon.workspace_state().unwrap();
    assert!(first_changes.iter().all(|change| current
        .workspace_versions
        .iter()
        .any(|version| version.operation().to_string() == *change)));
    assert_eq!(
        daemon
            .workspace_agent_custody_for_workspace(
                &current.root,
                &current.digest,
                &current.installation
            )
            .unwrap()
            .generation(),
        Some(generation.as_str())
    );
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn agent_workspace_checkpoint_reports_partial_signing_failure_and_preserves_saved_work() {
    let (parent, managed) = workspace("agent-workspace-partial");
    let daemon = open_daemon(&managed);
    let state = daemon.workspace_state().unwrap();
    let generation = daemon
        .acquire_workspace_agent_custody(
            &state.root,
            &state.digest,
            &state.installation,
            false,
            None,
        )
        .unwrap();
    fs::write(managed.join("existing/keep.txt"), "first durable edit\n").unwrap();
    fs::write(managed.join("added.txt"), "second edit\n").unwrap();
    let key = SigningKey::from_bytes(&[0x52; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let mut signatures = 0;
    let report = daemon
        .checkpoint_agent_workspace(
            mesh_daemon::AgentWorkspaceCheckpointRequest {
                root: &state.root,
                digest: &state.digest,
                installation: &state.installation,
                generation: &generation,
            },
            public,
            |payload| {
                signatures += 1;
                if signatures == 2 {
                    return Err("signer disconnected");
                }
                Ok(Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap();
    assert!(!report.complete);
    assert_eq!(report.issue, Some("checkpoint-file-save-failed"));
    assert_eq!(report.saved_changes.len(), 1);
    assert!(!daemon
        .inspect_managed_file("existing/keep.txt")
        .unwrap()
        .modified_from_current_version());
    let current = daemon.workspace_state().unwrap();
    let retry = daemon
        .checkpoint_agent_workspace(
            mesh_daemon::AgentWorkspaceCheckpointRequest {
                root: &current.root,
                digest: &current.digest,
                installation: &current.installation,
                generation: &generation,
            },
            public,
            |payload| {
                Ok::<_, core::convert::Infallible>(Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap();
    assert!(retry.complete);
    assert_eq!(retry.saved_changes.len(), 1);
    assert_eq!(retry.workspace.shared_version, state.shared_version);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn agent_workspace_checkpoint_final_scan_detects_edits_to_an_already_saved_file() {
    let (parent, managed) = workspace("agent-workspace-moving");
    let daemon = open_daemon(&managed);
    let state = daemon.workspace_state().unwrap();
    let generation = daemon
        .acquire_workspace_agent_custody(
            &state.root,
            &state.digest,
            &state.installation,
            false,
            None,
        )
        .unwrap();
    fs::write(managed.join("existing/keep.txt"), "first observed edit\n").unwrap();
    fs::write(managed.join("added.txt"), "new file\n").unwrap();
    let key = SigningKey::from_bytes(&[0x53; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let mut signatures = 0;
    let report = daemon
        .checkpoint_agent_workspace(
            mesh_daemon::AgentWorkspaceCheckpointRequest {
                root: &state.root,
                digest: &state.digest,
                installation: &state.installation,
                generation: &generation,
            },
            public,
            |payload| {
                signatures += 1;
                if signatures == 2 {
                    fs::write(managed.join("existing/keep.txt"), "later external edit\n").unwrap();
                }
                Ok::<_, core::convert::Infallible>(Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap();
    assert!(!report.complete);
    assert_eq!(report.issue, Some("checkpoint-working-folder-changed"));
    assert_eq!(report.saved_changes.len(), 2);
    assert_eq!(
        fs::read_to_string(managed.join("existing/keep.txt")).unwrap(),
        "later external edit\n"
    );
    assert_eq!(report.workspace.shared_version, state.shared_version);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn agent_workspace_checkpoint_refuses_ambiguous_missing_entries_before_any_save() {
    let (parent, managed) = workspace("agent-workspace-missing");
    let daemon = open_daemon(&managed);
    let state = daemon.workspace_state().unwrap();
    let generation = daemon
        .acquire_workspace_agent_custody(
            &state.root,
            &state.digest,
            &state.installation,
            false,
            None,
        )
        .unwrap();
    fs::rename(
        managed.join("existing/keep.txt"),
        managed.join("renamed.txt"),
    )
    .unwrap();
    let key = SigningKey::from_bytes(&[0x54; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let report = daemon
        .checkpoint_agent_workspace(
            mesh_daemon::AgentWorkspaceCheckpointRequest {
                root: &state.root,
                digest: &state.digest,
                installation: &state.installation,
                generation: &generation,
            },
            public,
            |_| -> Result<Signature, &'static str> { panic!("ambiguous change reached signer") },
        )
        .unwrap();
    assert!(!report.complete);
    assert_eq!(report.issue, Some("checkpoint-entry-resolution-required"));
    assert!(report.saved_changes.is_empty());
    assert_eq!(report.workspace.digest, state.digest);
    assert!(managed.join("renamed.txt").exists());
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn agent_submits_historical_review_while_newer_private_work_continues() {
    let (parent, managed) = workspace("agent-saved-review");
    let daemon = open_daemon(&managed);
    let initial = daemon.workspace_state().unwrap();
    let generation = daemon
        .acquire_workspace_agent_custody(
            &initial.root,
            &initial.digest,
            &initial.installation,
            false,
            None,
        )
        .unwrap();
    let key = SigningKey::from_bytes(&[0x65; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let mut versions = Vec::new();
    for contents in ["first review bytes\n", "second review bytes\n"] {
        fs::write(managed.join("existing/keep.txt"), contents).unwrap();
        let state = daemon.workspace_state().unwrap();
        let report = daemon
            .checkpoint_agent_workspace(
                mesh_daemon::AgentWorkspaceCheckpointRequest {
                    root: &state.root,
                    digest: &state.digest,
                    installation: &state.installation,
                    generation: &generation,
                },
                public,
                |payload| {
                    Ok::<_, core::convert::Infallible>(Signature::from_bytes(
                        key.sign(payload.as_bytes()).to_bytes(),
                    ))
                },
            )
            .unwrap();
        assert!(report.complete);
        versions.push(
            report
                .workspace
                .workspace_versions
                .last()
                .unwrap()
                .operation(),
        );
    }
    fs::write(
        managed.join("existing/keep.txt"),
        "third unsaved working edit\n",
    )
    .unwrap();
    let mut bundles = Vec::new();
    for version in &versions {
        let state = daemon.workspace_state().unwrap();
        bundles.push(
            daemon
                .submit_agent_saved_review(
                    mesh_daemon::AgentWorkspaceCheckpointRequest {
                        root: &state.root,
                        digest: &state.digest,
                        installation: &state.installation,
                        generation: &generation,
                    },
                    *version,
                    public,
                )
                .unwrap(),
        );
    }
    let before_retry = daemon.workspace_state().unwrap();
    assert_eq!(
        daemon
            .submit_agent_saved_review(
                mesh_daemon::AgentWorkspaceCheckpointRequest {
                    root: &before_retry.root,
                    digest: &before_retry.digest,
                    installation: &before_retry.installation,
                    generation: &generation,
                },
                versions[0],
                public
            )
            .unwrap(),
        bundles[0]
    );
    let state = daemon.workspace_state().unwrap();
    assert_eq!(state.records, before_retry.records);
    assert_eq!(state.shared_version, initial.shared_version);
    assert_eq!(state.reviews, 2);
    let first = state
        .review_items
        .iter()
        .find(|item| {
            item.get("bundle").and_then(mesh_daemon::ipc::Json::as_text)
                == Some(bundles[0].to_string().as_str())
        })
        .unwrap();
    assert_eq!(
        first.get("content_complete"),
        Some(&mesh_daemon::ipc::Json::Bool(true))
    );
    let change = first
        .get("bundle_changes")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .find(|change| {
            change
                .get("path_after")
                .and_then(mesh_daemon::ipc::Json::as_text)
                .is_some_and(|path| path.ends_with("existing/keep.txt"))
        })
        .unwrap();
    let object = change.get("object_id").unwrap().as_text().unwrap();
    let artifact = daemon
        .review_artifact_for_workspace(
            &state.root,
            &state.digest,
            &state.installation,
            &bundles[0].to_string(),
            &versions[0].to_string(),
            object,
            "after",
        )
        .unwrap();
    assert_eq!(artifact.bytes(), b"first review bytes\n");
    assert_eq!(
        fs::read_to_string(managed.join("existing/keep.txt")).unwrap(),
        "third unsaved working edit\n"
    );
    assert_eq!(
        daemon
            .workspace_agent_custody_for_workspace(&state.root, &state.digest, &state.installation)
            .unwrap()
            .generation(),
        Some(generation.as_str())
    );
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn explicit_agent_file_deletion_allows_a_complete_checkpoint_and_retains_history() {
    let (parent, managed) = workspace("agent-explicit-delete");
    let daemon = open_daemon(&managed);
    let initial = daemon.workspace_state().unwrap();
    let generation = daemon
        .acquire_workspace_agent_custody(
            &initial.root,
            &initial.digest,
            &initial.installation,
            false,
            None,
        )
        .unwrap();
    let version = daemon
        .inspect_managed_file("existing/keep.txt")
        .unwrap()
        .current_version()
        .to_owned();
    fs::remove_file(managed.join("existing/keep.txt")).unwrap();
    let key = SigningKey::from_bytes(&[0x71; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let before = fs::read(private_storage(&managed).join("records.mesh")).unwrap();
    let request = || mesh_daemon::AgentWorkspaceCheckpointRequest {
        root: &initial.root,
        digest: &initial.digest,
        installation: &initial.installation,
        generation: &generation,
    };
    let report = daemon
        .checkpoint_agent_workspace(request(), public, |_| -> Result<Signature, &'static str> {
            panic!("ordinary capture guessed deletion")
        })
        .unwrap();
    assert!(!report.complete);
    assert_eq!(report.issue, Some("checkpoint-entry-resolution-required"));
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        before
    );
    let receipt = daemon
        .checkpoint_agent_file_deletion(
            request(),
            "existing/keep.txt",
            &version,
            public,
            |payload| {
                assert!(daemon
                    .create_managed_folder("smuggled", public, signer(&key))
                    .is_err());
                Ok::<_, core::convert::Infallible>(Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap();
    assert!(receipt.meaningful_saved());
    assert_eq!(receipt.action(), "adopt_delete");
    assert!(!managed.join("existing/keep.txt").exists());
    assert_eq!(
        fs::read(parent.join("source/existing/keep.txt")).unwrap(),
        b"keep\n"
    );
    let current = daemon.workspace_state().unwrap();
    assert_eq!(current.shared_version, initial.shared_version);
    let report = daemon
        .checkpoint_agent_workspace(
            mesh_daemon::AgentWorkspaceCheckpointRequest {
                root: &current.root,
                digest: &current.digest,
                installation: &current.installation,
                generation: &generation,
            },
            public,
            |_| -> Result<Signature, &'static str> { panic!("unchanged checkpoint signed again") },
        )
        .unwrap();
    assert!(report.complete);
    assert!(report.saved_changes.is_empty());
    assert!(daemon
        .create_managed_folder("still-owned", public, signer(&key))
        .is_err());
    drop(daemon);
    let history = OpenWorkspace::open(&managed).unwrap();
    assert!(history.file_histories().is_empty());
    assert!(history
        .retired_entries()
        .iter()
        .any(|entry| entry.path() == "existing/keep.txt"));
    assert_eq!(
        history
            .retired_entries()
            .iter()
            .find(|entry| entry.path() == "existing/keep.txt")
            .unwrap()
            .entry_type(),
        "file"
    );
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn agent_deletion_refuses_stale_identity_or_version_and_preserves_concurrent_recreation() {
    let (parent, managed) = workspace("agent-delete-refusal");
    let daemon = open_daemon(&managed);
    let initial = daemon.workspace_state().unwrap();
    let generation = daemon
        .acquire_workspace_agent_custody(
            &initial.root,
            &initial.digest,
            &initial.installation,
            false,
            None,
        )
        .unwrap();
    let version = daemon
        .inspect_managed_file("existing/keep.txt")
        .unwrap()
        .current_version()
        .to_owned();
    fs::remove_file(managed.join("existing/keep.txt")).unwrap();
    let key = SigningKey::from_bytes(&[0x72; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let before = fs::read(private_storage(&managed).join("records.mesh")).unwrap();
    for (digest, installation, owner, path, saved) in [
        (
            "wrong",
            initial.installation.as_str(),
            generation.as_str(),
            "existing/keep.txt",
            version.as_str(),
        ),
        (
            initial.digest.as_str(),
            "wrong",
            generation.as_str(),
            "existing/keep.txt",
            version.as_str(),
        ),
        (
            initial.digest.as_str(),
            initial.installation.as_str(),
            "wrong",
            "existing/keep.txt",
            version.as_str(),
        ),
        (
            initial.digest.as_str(),
            initial.installation.as_str(),
            generation.as_str(),
            "../keep.txt",
            version.as_str(),
        ),
        (
            initial.digest.as_str(),
            initial.installation.as_str(),
            generation.as_str(),
            "existing/keep.txt",
            "wrong",
        ),
    ] {
        assert!(daemon
            .checkpoint_agent_file_deletion(
                mesh_daemon::AgentWorkspaceCheckpointRequest {
                    root: &initial.root,
                    digest,
                    installation,
                    generation: owner
                },
                path,
                saved,
                public,
                |_| -> Result<Signature, &'static str> { panic!("stale deletion reached signer") }
            )
            .is_err());
    }
    let request = || mesh_daemon::AgentWorkspaceCheckpointRequest {
        root: &initial.root,
        digest: &initial.digest,
        installation: &initial.installation,
        generation: &generation,
    };
    assert!(daemon
        .checkpoint_agent_file_deletion(
            request(),
            "existing/keep.txt",
            &version,
            public,
            |_| Err::<Signature, _>("signing failed")
        )
        .is_err());
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        before
    );
    assert!(daemon
        .checkpoint_agent_file_deletion(
            request(),
            "existing/keep.txt",
            &version,
            public,
            |payload| {
                fs::write(
                    managed.join("existing/keep.txt"),
                    b"concurrent recreation\n",
                )
                .unwrap();
                Ok::<_, core::convert::Infallible>(Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            }
        )
        .is_err());
    assert_eq!(
        fs::read(managed.join("existing/keep.txt")).unwrap(),
        b"concurrent recreation\n"
    );
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        before
    );
    assert_eq!(
        daemon.workspace_state().unwrap().shared_version,
        initial.shared_version
    );
    fs::remove_file(managed.join("existing/keep.txt")).unwrap();
    assert!(daemon
        .checkpoint_agent_file_deletion(
            request(),
            "existing/keep.txt",
            &version,
            public,
            signer(&key)
        )
        .unwrap()
        .meaningful_saved());
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn prepared_agent_deletion_binds_operation_before_append_and_rechecks_after_callback() {
    let (parent, managed) = workspace("agent-prepared-delete");
    let daemon = open_daemon(&managed);
    let initial = daemon.workspace_state().unwrap();
    let generation = daemon
        .acquire_workspace_agent_custody(
            &initial.root,
            &initial.digest,
            &initial.installation,
            false,
            None,
        )
        .unwrap();
    let version = daemon
        .inspect_managed_file("existing/keep.txt")
        .unwrap()
        .current_version()
        .to_owned();
    fs::remove_file(managed.join("existing/keep.txt")).unwrap();
    let key = SigningKey::from_bytes(&[0x73; 32]);
    let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let before = fs::read(private_storage(&managed).join("records.mesh")).unwrap();
    let request = || mesh_daemon::AgentWorkspaceCheckpointRequest {
        root: &initial.root,
        digest: &initial.digest,
        installation: &initial.installation,
        generation: &generation,
    };
    let prepared = Cell::new(None);
    assert!(daemon
        .checkpoint_agent_file_deletion_prepared(
            request(),
            "existing/keep.txt",
            &version,
            public,
            signer(&key),
            |operation| {
                prepared.set(Some(operation));
                assert!(daemon
                    .inspect_agent_prepared_operation(request(), operation, public)
                    .is_err());
                assert_eq!(
                    fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
                    before
                );
                assert!(daemon
                    .create_managed_folder("smuggled", public, signer(&key))
                    .is_err());
                Err::<(), _>("intent storage unavailable")
            }
        )
        .is_err());
    let intended = prepared.get().unwrap();
    assert!(!daemon
        .inspect_agent_prepared_operation(request(), intended, public)
        .unwrap());
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        before
    );
    assert!(daemon
        .checkpoint_agent_file_deletion_prepared(
            request(),
            "existing/keep.txt",
            &version,
            public,
            signer(&key),
            |operation| {
                assert_eq!(operation, intended);
                fs::write(managed.join("existing/keep.txt"), b"late creator\n").unwrap();
                Ok::<_, core::convert::Infallible>(())
            }
        )
        .is_err());
    assert_eq!(
        fs::read(managed.join("existing/keep.txt")).unwrap(),
        b"late creator\n"
    );
    assert_eq!(
        fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
        before
    );
    fs::remove_file(managed.join("existing/keep.txt")).unwrap();
    let receipt = daemon
        .checkpoint_agent_file_deletion_prepared(
            request(),
            "existing/keep.txt",
            &version,
            public,
            signer(&key),
            |operation| {
                assert_eq!(operation, intended);
                assert_eq!(
                    fs::read(private_storage(&managed).join("records.mesh")).unwrap(),
                    before
                );
                Ok::<_, core::convert::Infallible>(())
            },
        )
        .unwrap();
    assert_eq!(receipt.changeset(), intended.to_string());
    let after = daemon.workspace_state().unwrap();
    let inspect = || mesh_daemon::AgentWorkspaceCheckpointRequest {
        root: &after.root,
        digest: &after.digest,
        installation: &after.installation,
        generation: &generation,
    };
    assert!(daemon
        .inspect_agent_prepared_operation(inspect(), intended, public)
        .unwrap());
    assert!(daemon
        .inspect_agent_prepared_operation(inspect(), intended, PublicKey::from_bytes([0; 32]))
        .is_err());

    assert!(receipt.meaningful_saved());
    assert_eq!(
        daemon.workspace_state().unwrap().shared_version,
        initial.shared_version
    );
    fs::remove_dir_all(parent).unwrap();
}
