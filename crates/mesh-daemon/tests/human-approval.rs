//! End-to-end authority proof for one native user-presence approval receipt.

#![cfg(unix)]

use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey};
use mesh_approval::{
    ApprovalDecision, ExpectedHumanApproval, HumanApprovalCredential, HumanApprovalReceiptDraft,
};
use mesh_daemon::ipc::surface::{nothing_to_recover, Operations, StartupSummary};
use mesh_daemon::{
    CheckpointRuntimeParameters, LiveDaemon, ManagedTextFileError, PreparedFolderImport,
    TrustedReviewers,
};
use mesh_types::{PublicKey, Signature as MeshSignature};
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair as _, ECDSA_P256_SHA256_ASN1_SIGNING};

struct TestSigner {
    key: EcdsaKeyPair,
    credential: HumanApprovalCredential,
}

impl TestSigner {
    fn generate() -> Self {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng)
            .expect("test P-256 key");
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng)
            .expect("parse test key");
        let public_key: [u8; 65] = key
            .public_key()
            .as_ref()
            .try_into()
            .expect("uncompressed P-256 key");
        let credential = HumanApprovalCredential::from_public_key(public_key).expect("credential");
        Self { key, credential }
    }

    fn sign(&self, expected: ExpectedHumanApproval) -> Vec<u8> {
        let draft = HumanApprovalReceiptDraft::new(expected, ApprovalDecision::Approve);
        let signature = self
            .key
            .sign(&SystemRandom::new(), &draft.canonical_bytes())
            .expect("ES256 signature");
        draft
            .with_signature(signature.as_ref().to_vec())
            .expect("DER receipt")
            .canonical_bytes()
    }
}

fn startup() -> StartupSummary {
    StartupSummary::from(&nothing_to_recover())
}

fn checkpoint_parameters() -> CheckpointRuntimeParameters {
    CheckpointRuntimeParameters {
        idle_interval: Some(Duration::from_millis(1)),
        maximum_uncheckpointed_bytes: Some(65_536),
        maximum_uncheckpointed_interval: Some(Duration::from_millis(25)),
    }
}

fn scratch(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mesh-human-approval-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    path
}

fn text_field<'a>(value: &'a mesh_daemon::ipc::Json, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(mesh_daemon::ipc::Json::as_text)
        .unwrap_or_else(|| panic!("missing text field {key}"))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn copy_tree(source: &std::path::Path, destination: &std::path::Path) {
    std::fs::create_dir(destination).expect("create clone root");
    for entry in std::fs::read_dir(source).expect("read clone source") {
        let entry = entry.expect("clone source entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let kind = entry.file_type().expect("clone entry type");
        if kind.is_dir() {
            copy_tree(&source_path, &destination_path);
        } else if kind.is_file() {
            std::fs::copy(&source_path, &destination_path).expect("copy clone file");
        } else if kind.is_symlink() {
            std::os::unix::fs::symlink(
                std::fs::read_link(&source_path).expect("read clone symlink"),
                &destination_path,
            )
            .expect("copy clone symlink");
        } else {
            panic!("workspace clone contained an unsupported entry");
        }
    }
}

#[test]
fn exact_human_receipt_advances_once_and_survives_restart() {
    let base = scratch("roundtrip");
    let source = base.join("source");
    let workspace = base.join("workspace");
    std::fs::create_dir_all(&source).expect("source");
    std::fs::write(source.join("notes.txt"), b"alpha approval\n").expect("source bytes");
    let prepared = PreparedFolderImport::prepare(&source, &workspace).expect("preview import");
    let (confirmed, imported) = prepared.confirm_into_workspace().expect("confirm import");
    drop(confirmed);

    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let daemon = LiveDaemon::with_trusted_reviewers(startup(), trust.clone());
    daemon.open_at_start(&workspace).expect("open workspace");
    let shown = Operations::workspace_state(&daemon).expect("workspace state");
    let review_custody = daemon
        .acquire_workspace_agent_custody(
            &shown.root,
            &shown.digest,
            &shown.installation,
            false,
            None,
        )
        .expect("acquire review custody");
    let review_refused = daemon
        .open_current_review_for_workspace(
            &shown.root,
            &shown.digest,
            &shown.installation,
            PublicKey::from_bytes([7; 32]),
        )
        .expect_err("assigned agent blocks review mutation");
    assert_eq!(review_refused.code, "workspace-agent-custody-active");
    daemon
        .release_workspace_agent_custody(
            &shown.root,
            &shown.digest,
            &shown.installation,
            &review_custody,
        )
        .expect("release review custody");
    let reviewed = daemon
        .open_current_review_for_workspace(
            &shown.root,
            &shown.digest,
            &shown.installation,
            PublicKey::from_bytes([7; 32]),
        )
        .expect("record exact review");
    let review = reviewed.review_items.first().expect("review card");
    let bundle = text_field(review, "bundle");
    let reviewed_head = text_field(review, "reviewed_head").to_owned();
    let target = imported.operation().to_string();
    let context = daemon
        .human_approval_context_for_workspace(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            bundle,
            &target,
        )
        .expect("exact context");
    let preview = daemon
        .human_approval_preview_for_workspace(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            bundle,
            &target,
        )
        .expect("trusted native preview");
    assert_eq!(preview.context(), &context);
    assert_eq!(
        preview.review_bundle().actor_state(),
        preview.approved_state().digest()
    );
    assert_eq!(
        preview.review_bundle().actor_head(),
        context.reviewed_actor_head()
    );
    assert_eq!(
        preview.presentation_digest().to_string(),
        text_field(review, "presentation_digest")
    );
    assert!(preview
        .change_summary()
        .contains("Exact reviewed changes: 1"));
    assert!(preview
        .change_summary()
        .contains("created · not present -> \"/notes.txt\""));
    assert!(preview.change_summary().contains("binary version"));
    assert!(preview.change_summary().contains("digest"));
    assert!(preview.change_summary().contains("15 bytes"));
    let receipt = signer.sign(ExpectedHumanApproval::new(
        context,
        signer.credential.clone(),
        [9; 32],
    ));

    let approval_custody = daemon
        .acquire_workspace_agent_custody(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            false,
            None,
        )
        .expect("acquire approval custody");
    let approval_refused = Operations::approve_review(&daemon, bundle, &target, &hex(&receipt))
        .expect_err("assigned agent blocks approval mutation");
    assert_eq!(approval_refused.code, "workspace-agent-custody-active");
    daemon
        .release_workspace_agent_custody(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            &approval_custody,
        )
        .expect("release approval custody");

    let ran_original_export = std::cell::Cell::new(false);
    let refusal = daemon
        .with_verified_shared_managed_workspace(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            || {
                ran_original_export.set(true);
                Ok(())
            },
        )
        .expect_err("a private version must not update the original project before approval");
    assert!(matches!(
        refusal,
        ManagedTextFileError::OriginalExportRequiresSharedVersion
    ));
    assert!(!ran_original_export.get());

    let approved = Operations::approve_review(&daemon, bundle, &target, &hex(&receipt))
        .expect("user-verified approval");
    assert_eq!(
        approved.shared_version.as_deref(),
        Some(reviewed_head.as_str())
    );
    let export_custody = daemon
        .acquire_workspace_agent_custody(
            &approved.root,
            &approved.digest,
            &approved.installation,
            false,
            None,
        )
        .expect("acquire export custody");
    let assigned_export = daemon
        .with_verified_shared_managed_workspace(
            &approved.root,
            &approved.digest,
            &approved.installation,
            || {
                ran_original_export.set(true);
                Ok(())
            },
        )
        .expect_err("alpha export remains blocked while an agent owns the folder");
    assert!(matches!(assigned_export, ManagedTextFileError::Recovery(_)));
    assert!(!ran_original_export.get());
    daemon
        .release_workspace_agent_custody(
            &approved.root,
            &approved.digest,
            &approved.installation,
            &export_custody,
        )
        .expect("release export custody");
    daemon
        .with_verified_shared_managed_workspace(
            &approved.root,
            &approved.digest,
            &approved.installation,
            || {
                ran_original_export.set(true);
                Ok(())
            },
        )
        .expect("the exact approved current version may update the original project");
    assert!(ran_original_export.get());
    let records = approved.records;

    let replay = Operations::approve_review(&daemon, bundle, &target, &hex(&receipt))
        .expect_err("exact receipt replay must not append");
    assert_eq!(replay.code, "publication-already-shared");
    assert_eq!(
        Operations::workspace_state(&daemon).expect("state").records,
        records
    );
    let durable = daemon
        .durable_human_approval_for_workspace(
            &approved.root,
            &approved.digest,
            &approved.installation,
            bundle,
            &target,
        )
        .expect("durable approval before restart");
    assert_eq!(durable.receipt(), receipt);
    assert_eq!(
        durable
            .expected()
            .context()
            .reviewed_actor_head()
            .to_string(),
        reviewed_head
    );
    assert_eq!(
        durable.preview().review_bundle().actor_state(),
        durable.preview().approved_state().digest()
    );
    drop(daemon);

    let restarted = LiveDaemon::with_trusted_reviewers(startup(), trust);
    let reopened = restarted.open_at_start(&workspace).expect("restart");
    assert_eq!(
        reopened.shared_version.as_deref(),
        Some(reviewed_head.as_str())
    );
    assert_eq!(reopened.records, records);
    let durable = restarted
        .durable_human_approval_for_workspace(
            &reopened.root,
            &reopened.digest,
            &reopened.installation,
            bundle,
            &target,
        )
        .expect("durable approval after restart");
    assert_eq!(durable.receipt(), receipt);
    assert_eq!(
        durable
            .expected()
            .context()
            .reviewed_actor_head()
            .to_string(),
        reviewed_head
    );

    let wrong_target = "ff".repeat(32);
    let refusal = match restarted.durable_human_approval_for_workspace(
        &reopened.root,
        &reopened.digest,
        &reopened.installation,
        bundle,
        &wrong_target,
    ) {
        Ok(_) => panic!("a browser target substitution must fail"),
        Err(refusal) => refusal,
    };
    assert_eq!(refusal.code, "publication-review-conflict");

    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn pending_review_keeps_its_original_base_after_main_advances_and_restart() {
    let base = scratch("pending-review-base");
    let source = base.join("source");
    let workspace = base.join("workspace");
    std::fs::create_dir_all(&source).expect("source");
    std::fs::write(source.join("notes.txt"), b"initial\n").expect("source bytes");
    let (confirmed, _) = PreparedFolderImport::prepare(&source, &workspace)
        .expect("prepare")
        .confirm_into_workspace()
        .expect("import");
    drop(confirmed);
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let daemon = LiveDaemon::with_trusted_reviewers_and_checkpoint_runtime(
        startup(),
        trust.clone(),
        checkpoint_parameters(),
    )
    .expect("runtime");
    daemon.open_at_start(&workspace).expect("open");
    let open_review = |daemon: &LiveDaemon| {
        let state = Operations::workspace_state(daemon).expect("state");
        daemon
            .open_current_review_for_workspace(
                &state.root,
                &state.digest,
                &state.installation,
                PublicKey::from_bytes([0x31; 32]),
            )
            .expect("review")
    };
    let context = |daemon: &LiveDaemon, bundle: &str, target: &str| {
        let state = Operations::workspace_state(daemon).expect("state");
        daemon
            .human_approval_context_for_workspace(
                &state.root,
                &state.digest,
                &state.installation,
                bundle,
                target,
            )
            .expect("exact context")
    };
    let initial = open_review(&daemon);
    let card = initial.review_items.first().expect("initial card");
    let initial_bundle = text_field(card, "bundle");
    let initial_target = text_field(card, "subject_operation");
    let initial_context = context(&daemon, initial_bundle, initial_target);
    let original_main = initial_context.reviewed_actor_head();
    let receipt = signer.sign(ExpectedHumanApproval::new(
        initial_context,
        signer.credential.clone(),
        [0x41; 32],
    ));
    Operations::approve_review(&daemon, initial_bundle, initial_target, &hex(&receipt))
        .expect("initial approval");

    let actor = SigningKey::from_bytes(&[0x52; 32]);
    let actor_public = PublicKey::from_bytes(actor.verifying_key().to_bytes());
    let save = |name: &str| {
        daemon
            .create_managed_text_file(name, "saved\n", actor_public, |payload| {
                Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                    actor.sign(payload.as_bytes()).to_bytes(),
                ))
            })
            .expect("save")
            .changeset()
            .to_owned()
    };
    let pending_target = save("pending.txt");
    let pending = open_review(&daemon);
    let pending_card = pending
        .review_items
        .iter()
        .find(|item| text_field(item, "subject_operation") == pending_target)
        .expect("pending card")
        .clone();
    let pending_bundle = text_field(&pending_card, "bundle");
    let pending_context = context(&daemon, pending_bundle, &pending_target);
    assert_eq!(pending_context.expected_canonical_head(), original_main);
    let stale_receipt = signer.sign(ExpectedHumanApproval::new(
        pending_context.clone(),
        signer.credential.clone(),
        [0x42; 32],
    ));

    let latest_target = save("latest.txt");
    let latest = open_review(&daemon);
    let latest_card = latest
        .review_items
        .iter()
        .find(|item| text_field(item, "subject_operation") == latest_target)
        .expect("latest card");
    let latest_bundle = text_field(latest_card, "bundle");
    let latest_context = context(&daemon, latest_bundle, &latest_target);
    assert_eq!(latest_context.expected_canonical_head(), original_main);
    let latest_main = latest_context.reviewed_actor_head();
    let receipt = signer.sign(ExpectedHumanApproval::new(
        latest_context,
        signer.credential.clone(),
        [0x43; 32],
    ));
    Operations::approve_review(&daemon, latest_bundle, &latest_target, &hex(&receipt))
        .expect("advance main past the pending review");
    assert_eq!(
        context(&daemon, pending_bundle, &pending_target),
        pending_context
    );
    drop(daemon);

    let restarted = LiveDaemon::with_trusted_reviewers_and_checkpoint_runtime(
        startup(),
        trust.clone(),
        checkpoint_parameters(),
    )
    .expect("restart runtime");
    let state = restarted.open_at_start(&workspace).expect("restart");
    assert_eq!(
        state.shared_version.as_deref(),
        Some(latest_main.to_string().as_str())
    );
    let card = state
        .review_items
        .iter()
        .find(|item| text_field(item, "bundle") == pending_bundle)
        .expect("retained pending card");
    assert_eq!(card, &pending_card, "immutable review presentation changed");
    assert_eq!(
        context(&restarted, pending_bundle, &pending_target),
        pending_context
    );
    assert!(
        restarted
            .human_approval_preview_for_workspace(
                &state.root,
                &state.digest,
                &state.installation,
                pending_bundle,
                &pending_target,
            )
            .is_err(),
        "historical presentation must not authorize a current approval"
    );
    assert!(
        Operations::approve_review(
            &restarted,
            pending_bundle,
            &pending_target,
            &hex(&stale_receipt),
        )
        .is_err(),
        "stale receipt must not move main backwards"
    );
    let after = Operations::workspace_state(&restarted).expect("state after refusal");
    assert_eq!(after.shared_version, state.shared_version);
    assert_eq!(
        after.digest, state.digest,
        "refusal must not append a record"
    );
    drop(restarted);

    // Simulate a peer delivering the pending request after main has advanced. The journal-order
    // hint is now wrong; only recomputation against verified historical heads can recover its base.
    let journal = workspace.join(".mesh").join(mesh_daemon::RECORD_FILE_NAME);
    let mut records = mesh_store::scan_journal(&std::fs::read(&journal).expect("journal"))
        .expect("valid journal")
        .into_records();
    let index = records.iter().position(|record| matches!(record,
        mesh_store::StoredRecord::Review(review) if review.bundle.to_string() == pending_bundle
    )).expect("pending request record");
    let request = records.remove(index);
    records.push(request);
    let bytes: Vec<u8> = records.iter().flat_map(mesh_store::frame_record).collect();
    std::fs::write(&journal, bytes).expect("delayed request fixture");
    let delayed = LiveDaemon::with_trusted_reviewers_and_checkpoint_runtime(
        startup(),
        trust.clone(),
        checkpoint_parameters(),
    )
    .expect("delayed runtime");
    let state = delayed
        .open_at_start(&workspace)
        .expect("delayed request reopen");
    assert_eq!(
        state.shared_version.as_deref(),
        Some(latest_main.to_string().as_str())
    );
    assert_eq!(
        context(&delayed, pending_bundle, &pending_target),
        pending_context
    );
    assert_eq!(
        state
            .review_items
            .iter()
            .find(|item| text_field(item, "bundle") == pending_bundle),
        Some(&pending_card)
    );
    drop(delayed);

    // A contradictory receipt must still poison current authority. Its presence must not turn
    // a historical read into permission to publish, even though the verified prefix is retained.
    let bad_approval = records
        .iter()
        .find_map(|record| match record {
            mesh_store::StoredRecord::Approval(_) => Some(record.clone()),
            _ => None,
        })
        .expect("approval fixture");
    records.push(bad_approval);
    let bytes: Vec<u8> = records.iter().flat_map(mesh_store::frame_record).collect();
    std::fs::write(&journal, bytes).expect("contradictory receipt fixture");
    let poisoned = LiveDaemon::with_trusted_reviewers_and_checkpoint_runtime(
        startup(),
        trust,
        checkpoint_parameters(),
    )
    .expect("poisoned runtime");
    let state = poisoned
        .open_at_start(&workspace)
        .expect("readable journal");
    assert!(
        state.shared_version.is_none(),
        "invalid authority cannot claim main"
    );
    assert_eq!(
        context(&poisoned, pending_bundle, &pending_target),
        pending_context
    );
    assert!(Operations::approve_review(
        &poisoned,
        pending_bundle,
        &pending_target,
        &hex(&stale_receipt),
    )
    .is_err());
    drop(poisoned);
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn a_second_saved_version_can_be_reviewed_and_approved_against_the_shared_head() {
    let base = scratch("second-roundtrip");
    let source = base.join("source");
    let workspace = base.join("workspace");
    std::fs::create_dir_all(&source).expect("source");
    std::fs::write(source.join("notes.txt"), b"first approval\n").expect("source bytes");
    let prepared = PreparedFolderImport::prepare(&source, &workspace).expect("preview import");
    let (confirmed, imported) = prepared.confirm_into_workspace().expect("confirm import");
    drop(confirmed);

    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let daemon = LiveDaemon::with_trusted_reviewers_and_checkpoint_runtime(
        startup(),
        trust.clone(),
        checkpoint_parameters(),
    )
    .expect("checkpoint runtime");
    daemon.open_at_start(&workspace).expect("open workspace");

    let shown = Operations::workspace_state(&daemon).expect("first state");
    let reviewed = daemon
        .open_current_review_for_workspace(
            &shown.root,
            &shown.digest,
            &shown.installation,
            PublicKey::from_bytes([0x31; 32]),
        )
        .expect("record first review");
    let first = reviewed.review_items.first().expect("first review card");
    let first_bundle = text_field(first, "bundle");
    let first_target = imported.operation().to_string();
    let first_context = daemon
        .human_approval_context_for_workspace(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            first_bundle,
            &first_target,
        )
        .expect("first context");
    let first_shared = first_context.reviewed_actor_head();
    let first_receipt = signer.sign(ExpectedHumanApproval::new(
        first_context,
        signer.credential.clone(),
        [0x41; 32],
    ));
    let approved =
        Operations::approve_review(&daemon, first_bundle, &first_target, &hex(&first_receipt))
            .expect("first approval");
    assert_eq!(
        approved.shared_version.as_deref(),
        Some(first_shared.to_string().as_str())
    );

    let actor = SigningKey::from_bytes(&[0x52; 32]);
    let actor_public = PublicKey::from_bytes(actor.verifying_key().to_bytes());
    let second_save = daemon
        .create_managed_text_file("second.txt", "second approval\n", actor_public, |payload| {
            Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                actor.sign(payload.as_bytes()).to_bytes(),
            ))
        })
        .expect("second private save");
    let second_target = second_save.changeset().to_owned();
    let second_state = Operations::workspace_state(&daemon).expect("second state");
    assert_eq!(
        second_state.shared_version.as_deref(),
        Some(first_shared.to_string().as_str()),
        "private work advanced the shared version before review",
    );
    let second_reviewed = daemon
        .open_current_review_for_workspace(
            &second_state.root,
            &second_state.digest,
            &second_state.installation,
            PublicKey::from_bytes([0x32; 32]),
        )
        .expect("record second exact review");
    let second = second_reviewed
        .review_items
        .iter()
        .find(|item| text_field(item, "subject_operation") == second_target)
        .expect("second review card");
    let second_bundle = text_field(second, "bundle");
    let second_context = daemon
        .human_approval_context_for_workspace(
            &second_reviewed.root,
            &second_reviewed.digest,
            &second_reviewed.installation,
            second_bundle,
            &second_target,
        )
        .expect("second context");
    let second_preview = daemon
        .human_approval_preview_for_workspace(
            &second_reviewed.root,
            &second_reviewed.digest,
            &second_reviewed.installation,
            second_bundle,
            &second_target,
        )
        .expect("second exact preview");
    assert_eq!(second_preview.review_bundle().presentation().len(), 1);
    assert!(second_preview.change_summary().contains("second.txt"));
    assert!(!second_preview.change_summary().contains("notes.txt"));
    assert_eq!(second_preview.context(), &second_context);
    assert_eq!(second_context.expected_canonical_head(), first_shared);
    assert_ne!(second_context.reviewed_actor_head(), first_shared);
    let second_shared = second_context.reviewed_actor_head();
    let second_receipt = signer.sign(ExpectedHumanApproval::new(
        second_context,
        signer.credential.clone(),
        [0x42; 32],
    ));
    let approved = Operations::approve_review(
        &daemon,
        second_bundle,
        &second_target,
        &hex(&second_receipt),
    )
    .expect("second approval");
    assert_eq!(
        approved.shared_version.as_deref(),
        Some(second_shared.to_string().as_str())
    );

    drop(daemon);
    let restarted = LiveDaemon::with_trusted_reviewers_and_checkpoint_runtime(
        startup(),
        trust,
        checkpoint_parameters(),
    )
    .expect("restart runtime");
    let reopened = restarted.open_at_start(&workspace).expect("restart");
    assert_eq!(
        reopened.shared_version.as_deref(),
        Some(second_shared.to_string().as_str())
    );
    let durable = restarted
        .durable_human_approval_for_workspace(
            &reopened.root,
            &reopened.digest,
            &reopened.installation,
            second_bundle,
            &second_target,
        )
        .expect("second approval remains exactly verifiable after restart");
    assert_eq!(
        durable.expected().context().expected_canonical_head(),
        first_shared,
    );
    assert_eq!(
        durable.expected().context().reviewed_actor_head(),
        second_shared,
    );

    let first_durable = match restarted.durable_human_approval_for_workspace(
        &reopened.root,
        &reopened.digest,
        &reopened.installation,
        first_bundle,
        &first_target,
    ) {
        Ok(_) => panic!("the first approval must not authorize export after version two is shared"),
        Err(refusal) => refusal,
    };
    assert_eq!(first_durable.code, "publication-review-conflict");

    let pull_back = restarted
        .preview_managed_file_export("second.txt", &source)
        .expect("preview the second approved version against the original folder");
    assert!(!pull_back.target_exists());
    restarted
        .with_verified_shared_managed_workspace(
            &reopened.root,
            &reopened.digest,
            &reopened.installation,
            || {
                restarted.export_managed_file(
                    pull_back.path(),
                    &source,
                    pull_back.target_installation(),
                    pull_back.target_parent_installation(),
                    pull_back.target_file_installation(),
                    pull_back.source_version(),
                    pull_back.source_content_digest(),
                    pull_back.source_executable(),
                    pull_back.target_content_digest(),
                    pull_back.target_executable(),
                )
            },
        )
        .expect("the second durable approval authorizes exact pull-back after restart");
    assert_eq!(
        std::fs::read(source.join("second.txt")).expect("second approved original bytes"),
        b"second approval\n",
    );
    assert_eq!(
        std::fs::read(source.join("notes.txt")).expect("first approved original bytes"),
        b"first approval\n",
        "pull-back of version two must not rewrite an unchanged original file",
    );

    let stale_context = restarted
        .human_approval_context_for_workspace(
            &reopened.root,
            &reopened.digest,
            &reopened.installation,
            first_bundle,
            &first_target,
        )
        .expect("historical first context remains renderable");
    let stale_receipt = signer.sign(ExpectedHumanApproval::new(
        stale_context,
        signer.credential.clone(),
        [0x43; 32],
    ));
    let stale = Operations::approve_review(
        &restarted,
        first_bundle,
        &first_target,
        &hex(&stale_receipt),
    )
    .expect_err("an old approval cannot advance from a stale shared head");
    assert_eq!(stale.code, "publication-review-conflict");
    assert_eq!(
        Operations::workspace_state(&restarted)
            .expect("state after stale replay")
            .shared_version
            .as_deref(),
        Some(second_shared.to_string().as_str()),
    );

    let raced = restarted
        .with_verified_shared_managed_workspace(
            &reopened.root,
            &reopened.digest,
            &reopened.installation,
            || {
                restarted.create_managed_folder(
                    "unapproved-directory",
                    actor_public,
                    |payload| {
                        Ok::<_, core::convert::Infallible>(MeshSignature::from_bytes(
                            actor.sign(payload.as_bytes()).to_bytes(),
                        ))
                    },
                )?;
                let preview =
                    restarted.preview_managed_directory_export("unapproved-directory", &source)?;
                restarted.export_shared_managed_directory(
                    preview.path(),
                    &source,
                    preview.source_directory_installation(),
                    preview.target_installation(),
                    preview.target_parent_installation(),
                    preview.target_directory_installation(),
                )
            },
        )
        .expect_err(
            "private work created after the outer approval check must not reach the original",
        );
    assert!(matches!(
        raced,
        ManagedTextFileError::OriginalExportRequiresSharedVersion
    ));
    assert!(
        !source.join("unapproved-directory").exists(),
        "the original folder changed after approval became stale"
    );
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn untrusted_and_substituted_receipts_append_nothing() {
    let base = scratch("refusals");
    let source = base.join("source");
    let workspace = base.join("workspace");
    std::fs::create_dir_all(&source).expect("source");
    std::fs::write(source.join("notes.txt"), b"refusal proof\n").expect("source bytes");
    let prepared = PreparedFolderImport::prepare(&source, &workspace).expect("preview import");
    let (confirmed, imported) = prepared.confirm_into_workspace().expect("confirm import");
    drop(confirmed);

    let trusted = TestSigner::generate();
    let untrusted = TestSigner::generate();
    let daemon = LiveDaemon::with_trusted_reviewers(
        startup(),
        TrustedReviewers::with_human_credentials([trusted.credential.clone()]),
    );
    daemon.open_at_start(&workspace).expect("open workspace");
    let shown = Operations::workspace_state(&daemon).expect("state");
    let reviewed = daemon
        .open_current_review_for_workspace(
            &shown.root,
            &shown.digest,
            &shown.installation,
            PublicKey::from_bytes([8; 32]),
        )
        .expect("record review");
    let bundle = text_field(&reviewed.review_items[0], "bundle");
    let target = imported.operation().to_string();
    let context = daemon
        .human_approval_context_for_workspace(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            bundle,
            &target,
        )
        .expect("context");
    let untrusted_receipt = untrusted.sign(ExpectedHumanApproval::new(
        context.clone(),
        untrusted.credential.clone(),
        [3; 32],
    ));
    let before = reviewed.records;
    let refusal = Operations::approve_review(&daemon, bundle, &target, &hex(&untrusted_receipt))
        .expect_err("untrusted credential");
    assert_eq!(refusal.code, "publication-reviewer-untrusted");
    assert_eq!(
        Operations::workspace_state(&daemon).expect("state").records,
        before
    );

    let mut substituted = trusted.sign(ExpectedHumanApproval::new(
        context,
        trusted.credential.clone(),
        [4; 32],
    ));
    let middle = substituted.len() / 2;
    substituted[middle] ^= 1;
    let refusal = Operations::approve_review(&daemon, bundle, &target, &hex(&substituted))
        .expect_err("substituted receipt");
    assert!(matches!(
        refusal.code.as_str(),
        "publication-receipt-invalid" | "publication-approval-invalid"
    ));
    assert_eq!(
        Operations::workspace_state(&daemon).expect("state").records,
        before
    );

    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn native_work_added_after_review_refuses_approval_before_any_append() {
    let base = scratch("native-work-after-review");
    let source = base.join("source");
    let workspace = base.join("workspace");
    std::fs::create_dir_all(&source).expect("source");
    std::fs::write(source.join("notes.txt"), b"reviewed bytes\n").expect("source bytes");
    let prepared = PreparedFolderImport::prepare(&source, &workspace).expect("preview import");
    let (confirmed, imported) = prepared.confirm_into_workspace().expect("confirm import");
    drop(confirmed);

    let signer = TestSigner::generate();
    let daemon = LiveDaemon::with_trusted_reviewers(
        startup(),
        TrustedReviewers::with_human_credentials([signer.credential.clone()]),
    );
    daemon.open_at_start(&workspace).expect("open workspace");
    let shown = Operations::workspace_state(&daemon).expect("workspace state");
    let reviewed = daemon
        .open_current_review_for_workspace(
            &shown.root,
            &shown.digest,
            &shown.installation,
            PublicKey::from_bytes([11; 32]),
        )
        .expect("record exact review");
    let review = reviewed.review_items.first().expect("review card");
    let bundle = text_field(review, "bundle").to_owned();
    let target = imported.operation().to_string();
    let context = daemon
        .human_approval_context_for_workspace(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            &bundle,
            &target,
        )
        .expect("approval context before native edit");
    let receipt = signer.sign(ExpectedHumanApproval::new(
        context,
        signer.credential.clone(),
        [12; 32],
    ));
    daemon
        .human_approval_preview_for_workspace(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            &bundle,
            &target,
        )
        .expect("native approval preview before the editor changes bytes");
    let before = reviewed.records;

    std::fs::write(workspace.join("notes.txt"), b"newer native bytes\n")
        .expect("edit native workspace after review");

    let review_refusal = daemon
        .open_current_review_for_workspace(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            PublicKey::from_bytes([11; 32]),
        )
        .expect_err("opening review again must recheck the ordinary folder");
    assert_eq!(review_refusal.code, "publication-native-work-pending");

    let preview_refusal = daemon
        .human_approval_preview_for_workspace(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            &bundle,
            &target,
        )
        .expect_err("native approval preview must recheck the ordinary folder");
    assert_eq!(preview_refusal.code, "publication-native-work-pending");

    let approval_refusal = daemon
        .approve_review_for_workspace(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            &bundle,
            &target,
            &hex(&receipt),
        )
        .expect_err("native work arriving during the dialog must block approval");
    assert_eq!(approval_refusal.code, "publication-native-work-pending");
    assert_eq!(
        Operations::workspace_state(&daemon)
            .expect("state after refusals")
            .records,
        before,
    );
    assert_eq!(
        Operations::workspace_state(&daemon)
            .expect("shared state after refusals")
            .shared_version,
        None,
    );

    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn byte_identical_same_path_replacement_cannot_receive_the_displayed_approval() {
    let base = scratch("physical-replacement");
    let source = base.join("source");
    let workspace = base.join("workspace");
    let replacement = base.join("replacement");
    let displaced = base.join("displaced");
    std::fs::create_dir_all(&source).expect("source");
    std::fs::write(source.join("notes.txt"), b"same review bytes\n").expect("source bytes");
    let prepared = PreparedFolderImport::prepare(&source, &workspace).expect("preview import");
    let (confirmed, imported) = prepared.confirm_into_workspace().expect("confirm import");
    drop(confirmed);

    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let daemon = LiveDaemon::with_trusted_reviewers(startup(), trust.clone());
    daemon.open_at_start(&workspace).expect("open workspace");
    let shown = Operations::workspace_state(&daemon).expect("workspace state");
    let reviewed = daemon
        .open_current_review_for_workspace(
            &shown.root,
            &shown.digest,
            &shown.installation,
            PublicKey::from_bytes([13; 32]),
        )
        .expect("record exact review");
    let bundle = text_field(&reviewed.review_items[0], "bundle").to_owned();
    let target = imported.operation().to_string();
    let context = daemon
        .human_approval_context_for_workspace(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            &bundle,
            &target,
        )
        .expect("approval context");
    let receipt = signer.sign(ExpectedHumanApproval::new(
        context,
        signer.credential.clone(),
        [14; 32],
    ));
    drop(daemon);

    copy_tree(&workspace, &replacement);
    std::fs::rename(&workspace, &displaced).expect("displace reviewed installation");
    std::fs::rename(&replacement, &workspace).expect("install byte-identical replacement");

    let replacement_daemon = LiveDaemon::with_trusted_reviewers(startup(), trust);
    let replacement_state = replacement_daemon
        .open_at_start(&workspace)
        .expect("open replacement at the same path");
    assert_eq!(replacement_state.root, reviewed.root);
    assert_eq!(replacement_state.digest, reviewed.digest);
    assert_ne!(replacement_state.installation, reviewed.installation);
    let before = replacement_state.records;

    let refusal = replacement_daemon
        .approve_review_for_workspace(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            &bundle,
            &target,
            &hex(&receipt),
        )
        .expect_err("the approval must stay bound to the displayed physical installation");
    assert_eq!(refusal.code, "stale-workspace");
    let after = Operations::workspace_state(&replacement_daemon).expect("replacement state");
    assert_eq!(after.records, before);
    assert_eq!(after.shared_version, None);

    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn a_second_managed_client_cannot_append_an_approval_from_cached_history() {
    let base = scratch("two-cached-clients");
    let source = base.join("source");
    let workspace = base.join("workspace");
    std::fs::create_dir_all(&source).expect("source");
    std::fs::write(source.join("notes.txt"), b"reviewed bytes\n").expect("source bytes");
    let prepared = PreparedFolderImport::prepare(&source, &workspace).expect("preview import");
    let (confirmed, imported) = prepared.confirm_into_workspace().expect("confirm import");
    drop(confirmed);
    let signer = TestSigner::generate();
    let trust = TrustedReviewers::with_human_credentials([signer.credential.clone()]);
    let first = LiveDaemon::with_trusted_reviewers(startup(), trust.clone());
    first.open_at_start(&workspace).expect("first client");
    let shown = first.workspace_state().expect("initial state");
    let reviewed = first
        .open_current_review_for_workspace(
            &shown.root,
            &shown.digest,
            &shown.installation,
            PublicKey::from_bytes([29; 32]),
        )
        .expect("review");
    let bundle = text_field(&reviewed.review_items[0], "bundle").to_owned();
    let target = imported.operation().to_string();
    let context = first
        .human_approval_context_for_workspace(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            &bundle,
            &target,
        )
        .expect("review context");
    let first_receipt = hex(&signer.sign(ExpectedHumanApproval::new(
        context.clone(),
        signer.credential.clone(),
        [30; 32],
    )));
    let second_receipt = hex(&signer.sign(ExpectedHumanApproval::new(
        context,
        signer.credential.clone(),
        [31; 32],
    )));
    let second = LiveDaemon::with_trusted_reviewers(startup(), trust.clone());
    let stale = second
        .open_at_start(&workspace)
        .expect("second caches unapproved review");
    assert_eq!(stale.digest, reviewed.digest);
    let accepted = first
        .approve_review_for_workspace(
            &reviewed.root,
            &reviewed.digest,
            &reviewed.installation,
            &bundle,
            &target,
            &first_receipt,
        )
        .expect("first approval");
    assert!(accepted.shared_version.is_some());
    let journal = workspace.join(".mesh").join(mesh_daemon::RECORD_FILE_NAME);
    let before = std::fs::read(&journal).expect("accepted journal");
    let refused = second
        .approve_review_for_workspace(
            &stale.root,
            &stale.digest,
            &stale.installation,
            &bundle,
            &target,
            &second_receipt,
        )
        .expect_err("cached pre-approval history cannot admit another ceremony");
    assert!(
        std::fs::read(&journal).expect("journal after refusal") == before,
        "refusal must occur before any durable append: {refused:?}"
    );
    assert_eq!(refused.code, "stale-workspace");
    let retry = second
        .approve_review_for_workspace(
            &stale.root,
            &stale.digest,
            &stale.installation,
            &bundle,
            &target,
            &first_receipt,
        )
        .expect_err("even the accepted receipt cannot append through a stale displayed context");
    assert_eq!(retry.code, "stale-workspace");
    assert!(std::fs::read(&journal).expect("journal after retry") == before);
    let reopened = LiveDaemon::with_trusted_reviewers(startup(), trust);
    let recovered = reopened
        .open_at_start(&workspace)
        .expect("durable main still readable");
    assert_eq!(recovered.shared_version, accepted.shared_version);
    assert_eq!(recovered.records, accepted.records);
    let durable = reopened
        .durable_human_approval_for_workspace(
            &recovered.root,
            &recovered.digest,
            &recovered.installation,
            &bundle,
            &target,
        )
        .expect("the first accepted receipt remains recoverable");
    assert_eq!(hex(durable.receipt()), first_receipt);
    std::fs::remove_dir_all(base).expect("remove fixture");
}
