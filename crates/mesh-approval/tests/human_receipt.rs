//! Real ES256 coverage for the user-presence receipt protocol.

mod support;

use mesh_approval::{
    compute_bundle, verify_human_approval_receipt, ApprovalDecision, ApprovalWorkspaceId,
    BundleRequest, ExpectedHumanApproval, HumanApprovalContext, HumanApprovalCredential,
    HumanApprovalReceipt, HumanApprovalReceiptDraft, HumanApprovalReceiptError, ValidationResult,
    Verdict,
};
use mesh_types::{PolicyEpoch, WorkspaceId};
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair as _, ECDSA_P256_SHA256_ASN1_SIGNING};

use support::{actor_id, head, name, object, root, text};

struct Signer {
    key: EcdsaKeyPair,
    credential: HumanApprovalCredential,
}

impl Signer {
    fn generate() -> Self {
        let rng = SystemRandom::new();
        let document = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng)
            .expect("P-256 test key");
        let key =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, document.as_ref(), &rng)
                .expect("parse generated key");
        let public_key: [u8; 65] = key
            .public_key()
            .as_ref()
            .try_into()
            .expect("P-256 public key is uncompressed SEC1");
        let credential = HumanApprovalCredential::from_public_key(public_key).expect("credential");
        Self { key, credential }
    }

    fn sign(&self, draft: HumanApprovalReceiptDraft) -> HumanApprovalReceipt {
        let signature = self
            .key
            .sign(&SystemRandom::new(), &draft.canonical_bytes())
            .expect("test signature");
        draft
            .with_signature(signature.as_ref().to_vec())
            .expect("DER signature")
    }
}

fn workspace(byte: u8) -> WorkspaceId {
    WorkspaceId::mint(1_700_000_000_000 + u64::from(byte), [byte; 10])
}

fn bundle(with_validation: bool) -> mesh_approval::ReviewBundle {
    let base = mesh_approval::WorkspaceState::new(root());
    let actor = base
        .clone()
        .with_file(object(1), root(), name("alpha.txt"), text(7, &["alpha"]));
    let mut request = BundleRequest::new(
        base.clone(),
        base,
        head(1),
        actor,
        head(if with_validation { 3 } else { 2 }),
        actor_id(4),
    );
    if with_validation {
        request = request.with_validations(vec![ValidationResult::new(
            "alpha-validator",
            Some(object(1)),
            Verdict::Passed,
            "exact bytes checked",
        )]);
    }
    compute_bundle(&request).expect("review bundle")
}

fn expected(
    signer: &Signer,
    workspace_id: WorkspaceId,
    policy_epoch: u64,
    challenge: [u8; 32],
    with_validation: bool,
) -> ExpectedHumanApproval {
    let context = HumanApprovalContext::from_bundle(
        ApprovalWorkspaceId::from_bytes(*workspace_id.uuid().as_bytes()),
        PolicyEpoch::new(policy_epoch),
        &bundle(with_validation),
    )
    .expect("unconflicted context");
    ExpectedHumanApproval::new(context, signer.credential.clone(), challenge)
}

#[test]
fn real_es256_receipt_round_trips_and_verifies_exact_context() {
    let signer = Signer::generate();
    let exact = expected(&signer, workspace(1), 7, [9; 32], false);
    let draft = HumanApprovalReceiptDraft::new(exact.clone(), ApprovalDecision::Approve);
    let statement_digest = draft.statement_digest();
    assert_eq!(
        draft.expected().context().workspace_id().as_bytes(),
        workspace(1).uuid().as_bytes()
    );
    assert_ne!(
        draft.expected().context().selected_changes(),
        draft.expected().context().validation_digest()
    );
    assert_ne!(
        statement_digest,
        HumanApprovalReceiptDraft::new(
            expected(&signer, workspace(1), 7, [8; 32], false),
            ApprovalDecision::Approve,
        )
        .statement_digest()
    );
    let receipt = signer.sign(draft);
    let bytes = receipt.canonical_bytes();

    let decoded = HumanApprovalReceipt::from_canonical_bytes(&bytes).expect("canonical receipt");
    assert_eq!(decoded.canonical_bytes(), bytes);
    assert_eq!(
        verify_human_approval_receipt(&bytes, &exact)
            .expect("signature and exact context")
            .receipt(),
        &receipt
    );
}

#[test]
fn every_current_truth_substitution_is_refused() {
    let signer = Signer::generate();
    let exact = expected(&signer, workspace(1), 7, [9; 32], false);
    let bytes = signer
        .sign(HumanApprovalReceiptDraft::new(
            exact.clone(),
            ApprovalDecision::Approve,
        ))
        .canonical_bytes();

    let other_signer = Signer::generate();
    for substituted in [
        expected(&signer, workspace(2), 7, [9; 32], false),
        expected(&signer, workspace(1), 8, [9; 32], false),
        expected(&signer, workspace(1), 7, [8; 32], false),
        expected(&signer, workspace(1), 7, [9; 32], true),
        expected(&other_signer, workspace(1), 7, [9; 32], false),
    ] {
        assert_eq!(
            verify_human_approval_receipt(&bytes, &substituted),
            Err(HumanApprovalReceiptError::WrongExpectedContext)
        );
    }
}

#[test]
fn a_valid_signature_over_rejection_is_not_approval() {
    let signer = Signer::generate();
    let expected = expected(&signer, workspace(1), 7, [9; 32], false);
    let bytes = signer
        .sign(HumanApprovalReceiptDraft::new(
            expected.clone(),
            ApprovalDecision::Reject,
        ))
        .canonical_bytes();
    assert_eq!(
        verify_human_approval_receipt(&bytes, &expected),
        Err(HumanApprovalReceiptError::DecisionIsNotApproval)
    );
}

#[test]
fn tamper_wrong_key_and_noncanonical_bytes_fail_closed() {
    let signer = Signer::generate();
    let expected = expected(&signer, workspace(1), 7, [9; 32], false);
    let receipt = signer.sign(HumanApprovalReceiptDraft::new(
        expected.clone(),
        ApprovalDecision::Approve,
    ));

    let mut tampered = receipt.canonical_bytes();
    let index = tampered.len() / 2;
    tampered[index] ^= 1;
    assert!(verify_human_approval_receipt(&tampered, &expected).is_err());

    let mut trailing = receipt.canonical_bytes();
    trailing.push(0);
    assert!(matches!(
        HumanApprovalReceipt::from_canonical_bytes(&trailing),
        Err(HumanApprovalReceiptError::Decode(_))
    ));

    assert_eq!(
        HumanApprovalCredential::from_public_key([0; 65]),
        Err(HumanApprovalReceiptError::InvalidPublicKey)
    );
    assert_eq!(
        HumanApprovalReceiptDraft::new(expected, ApprovalDecision::Approve)
            .with_signature(vec![0; 73]),
        Err(HumanApprovalReceiptError::InvalidSignatureShape)
    );
}
