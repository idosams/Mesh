use super::*;
use crate::authenticated_changeset::CHANGESET_SIGNATURE_DOMAIN;
use crate::checkpoint_storage::{
    operation_checkpoint_signing_body, save_authenticated_operations,
    AuthenticatedOperationCheckpointRequest,
};
use ed25519_dalek::{Signer as _, SigningKey};
use mesh_crypto::SigningPayload;
use mesh_operations::{HeadDerivation, TransitionCommitment};
use mesh_types::Signature;

struct DerivedHead;
impl HeadDerivation for DerivedHead {
    fn resulting_head(&self, value: &TransitionCommitment) -> HeadId {
        HeadId::from_bytes(*Blake3::digest_bytes(&value.canonical_bytes()).as_bytes())
    }
}
fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}
fn public(key: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(key.verifying_key().to_bytes())
}
fn object(seed: u8) -> ObjectId {
    ObjectId::from_bytes([seed; 16])
}
fn directory(seed: u8, parent: u8, name: &str) -> Vec<Operation> {
    vec![
        Operation::CreateDirectory {
            object_id: object(seed),
        },
        Operation::LinkDirectoryEntry {
            directory_id: object(parent),
            name: NormalizedName::new(name).unwrap(),
            object_id: object(seed),
            version_id: VersionId::from_bytes([0; 32]),
        },
    ]
}
fn append(
    open: &mut OpenWorkspace,
    basis: &ManagedAuthoringBasis,
    key: &SigningKey,
    operations: Vec<Operation>,
) -> RecordDigest {
    let request = |signature| {
        AuthenticatedOperationCheckpointRequest::new(
            basis.workspace_id,
            basis.actor_id,
            basis.session_id,
            basis.actor_sequence,
            basis.causal_parents.clone(),
            basis.base_head,
            basis.policy_epoch,
            basis.hybrid_logical_time,
            operations.clone(),
            public(key),
            signature,
        )
    };
    let unsigned = request(Signature::from_bytes([0; 64]));
    let payload = SigningPayload::new(
        CHANGESET_SIGNATURE_DOMAIN,
        &operation_checkpoint_signing_body(&unsigned, &DerivedHead),
    );
    let signature = Signature::from_bytes(key.sign(payload.as_bytes()).to_bytes());
    let cas = Cas::with_filesystem(
        open.storage_root().as_path().to_owned(),
        open.storage_pinned_root().filesystem(),
    )
    .unwrap();
    save_authenticated_operations(open, &cas, request(signature), &DerivedHead)
        .unwrap()
        .changeset_id()
}
fn initial(open: &mut OpenWorkspace, key: &SigningKey) -> RecordDigest {
    let basis = ManagedAuthoringBasis {
        workspace_id: WorkspaceId::from_bytes([1; 16]),
        actor_id: ActorId::from_bytes(*public(key).as_bytes()),
        session_id: SessionId::from_bytes([2; 16]),
        actor_sequence: ActorSequence::FIRST,
        causal_parents: CausalParents::genesis(),
        base_head: HeadId::from_bytes([0; 32]),
        policy_epoch: PolicyEpoch::new(1),
        hybrid_logical_time: Hlc::new(10, 0),
    };
    append(open, &basis, key, directory(1, 0, "original"))
}
fn paths(open: &OpenWorkspace, version: RecordDigest) -> Vec<String> {
    open.historical_workspace_materialization(version)
        .unwrap()
        .1
        .iter()
        .map(|entry| entry.path().to_owned())
        .collect()
}
#[test]
fn historical_proposals_exclude_later_work_and_append_as_independent_signed_history() {
    let root =
        std::env::temp_dir().join(format!("mesh-historical-authoring-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("editor.txt"), "ongoing editor work").unwrap();
    let capture_actor = key(71);
    let import_actor = key(72);
    let mut open = OpenWorkspace::open(&root).unwrap();
    let base = initial(&mut open, &capture_actor);
    drop(open);
    let mut open = OpenWorkspace::open(&root).unwrap();
    let mut proposal = vec![Operation::UnlinkDirectoryEntry {
        directory_id: object(0),
        name: NormalizedName::new("original").unwrap(),
        object_id: object(1),
    }];
    proposal.extend(directory(2, 0, "proposal"));
    let plan = open
        .prepare_historical_operations(base, public(&import_actor), &proposal)
        .unwrap();
    assert_eq!(plan.target(), base);
    assert_eq!(plan.operations(), proposal);
    assert_eq!(
        plan.basis.causal_parents.as_slice(),
        &[ChangeSetId::from_bytes(*base.as_bytes())]
    );
    assert_eq!(plan.basis.actor_sequence, ActorSequence::FIRST);
    let continuing = open
        .prepare_historical_operations(base, public(&capture_actor), &directory(4, 0, "continued"))
        .unwrap();
    assert_eq!(continuing.basis.actor_sequence.value(), 2);
    let before = fs::read(open.record_file()).unwrap();
    assert!(open
        .prepare_historical_operations(base, public(&import_actor), &[])
        .is_err());
    assert!(open
        .prepare_historical_operations(
            RecordDigest::from_bytes([99; 32]),
            public(&import_actor),
            &proposal
        )
        .is_err());
    assert_eq!(fs::read(open.record_file()).unwrap(), before);
    let basis = open
        .managed_authoring_basis(public(&capture_actor))
        .unwrap();
    let later = append(&mut open, &basis, &capture_actor, directory(3, 0, "later"));
    drop(open);
    let mut open = OpenWorkspace::open(&root).unwrap();
    assert_eq!(
        open.prepare_historical_operations(base, public(&import_actor), &proposal)
            .unwrap(),
        plan
    );
    assert!(open
        .prepare_historical_operations(base, public(&capture_actor), &proposal)
        .is_err());
    assert!(open
        .prepare_historical_operations(base, public(&import_actor), &directory(4, 3, "child"))
        .is_err());
    assert!(open
        .validate_managed_operations(&directory(4, 3, "child"))
        .is_ok());
    assert_eq!(
        plan.context().get("approval_authority"),
        Some(&crate::ipc::Json::Bool(false))
    );
    let candidate = append(
        &mut open,
        &plan.basis,
        &import_actor,
        plan.operations.clone(),
    );
    drop(open);
    let open = OpenWorkspace::open(&root).unwrap();
    assert_eq!(paths(&open, base), vec!["original"]);
    assert_eq!(paths(&open, later), vec!["later", "original"]);
    assert_eq!(paths(&open, candidate), vec!["proposal"]);
    assert!(open
        .prepare_historical_operations(base, public(&import_actor), &proposal)
        .is_err());
    assert_eq!(
        fs::read_to_string(root.join("editor.txt")).unwrap(),
        "ongoing editor work"
    );
    assert!(!root.join("proposal").exists());
    assert!(open.shared_version().is_none());
    let fresh = key(73);
    let prior_policy = open
        .prepare_historical_operations(base, public(&fresh), &proposal)
        .unwrap();
    let mut open = open;
    let policy_actor = key(74);
    let mut policy_basis = open.managed_authoring_basis(public(&policy_actor)).unwrap();
    policy_basis.policy_epoch = PolicyEpoch::new(2);
    append(
        &mut open,
        &policy_basis,
        &policy_actor,
        directory(5, 0, "new-policy"),
    );
    drop(open);
    let open = OpenWorkspace::open(&root).unwrap();
    let current_policy = open
        .prepare_historical_operations(base, public(&fresh), &proposal)
        .unwrap();
    assert_eq!(current_policy.basis.base_head, prior_policy.basis.base_head);
    assert_eq!(
        current_policy.basis.hybrid_logical_time,
        prior_policy.basis.hybrid_logical_time
    );
    assert_eq!(current_policy.basis.policy_epoch.value(), 2);
    assert_ne!(
        current_policy, prior_policy,
        "a stale policy cannot reuse an earlier plan"
    );
    let displaced = root.with_extension("held");
    fs::rename(&root, &displaced).unwrap();
    fs::create_dir(&root).unwrap();
    assert!(open
        .prepare_historical_operations(base, public(&fresh), &proposal)
        .is_err());
    drop(open);
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(displaced).unwrap();
}

#[test]
fn refused_partial_and_oversized_plans_preserve_history_and_authoring_context() {
    let root = std::env::temp_dir().join(format!(
        "mesh-historical-authoring-refusal-{}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let capture_actor = key(81);
    let proposal_actor = key(82);
    let mut open = OpenWorkspace::open(&root).unwrap();
    let base = initial(&mut open, &capture_actor);
    drop(open);
    let open = OpenWorkspace::open(&root).unwrap();
    let valid = directory(2, 0, "proposal");
    let expected = open
        .prepare_historical_operations(base, public(&proposal_actor), &valid)
        .unwrap();
    let journal = fs::read(open.record_file()).unwrap();
    let prior_paths = paths(&open, base);

    // The first operations apply in the temporary materialization; the final link is invalid.
    let mut invalid = valid.clone();
    invalid.extend(directory(3, 99, "missing-parent"));
    assert!(open
        .prepare_historical_operations(base, public(&proposal_actor), &invalid)
        .is_err());
    let oversized = vec![valid[0].clone(); 100_001];
    let error = open
        .prepare_historical_operations(base, public(&proposal_actor), &oversized)
        .unwrap_err();
    assert!(error.contains("exceeds its limit"));
    assert_eq!(fs::read(open.record_file()).unwrap(), journal);
    assert_eq!(paths(&open, base), prior_paths);
    assert_eq!(
        open.prepare_historical_operations(base, public(&proposal_actor), &valid)
            .unwrap(),
        expected
    );
    assert!(!root.join("proposal").exists());
    assert!(open.shared_version().is_none());
    drop(open);
    let reopened = OpenWorkspace::open(&root).unwrap();
    assert_eq!(
        reopened
            .prepare_historical_operations(base, public(&proposal_actor), &valid)
            .unwrap(),
        expected
    );
    assert_eq!(fs::read(reopened.record_file()).unwrap(), journal);
    drop(reopened);
    fs::remove_dir_all(root).unwrap();
}
