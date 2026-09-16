//! End-to-end receipt composition tests using real Ed25519 signatures.

use ed25519_dalek::{Signer, SigningKey};
use mesh_approval::{
    verify_approval_receipt, ApprovalDecision, ApprovalReceipt, ApprovalReceiptDraft,
    ApprovalReceiptError, Digest32, ExpectedAdvance, HeadId, ReviewBundleId,
};
use mesh_crypto::{
    ActorKey, Capability, CustodyBackend, CustodyError, DelegationBudget, DomainSeparator, Expiry,
    ForActor, HumanAction, HumanHeld, HumanKeyCustody, KeyCustody, KeyPair, SigningPayload,
    WorkspaceScope,
};
use mesh_policy::{
    AuthorityRequest, DecisionLedger, EpochChain, HumanPrincipal, Operation, Principal,
    PublicationAuthority,
};
use mesh_types::{ActorKind, DecodeError, PolicyEpoch, PublicKey, Signature};

const SIGNATURE_DOMAIN_MUTATION: DomainSeparator =
    DomainSeparator::new("mesh.v0.not-an-approval-receipt");

fn bundle(byte: u8) -> ReviewBundleId {
    ReviewBundleId::from_digest(Digest32::from_bytes([byte; 32]))
}

fn head(byte: u8) -> HeadId {
    HeadId::from_bytes([byte; 32])
}

fn signing_key(byte: u8) -> SigningKey {
    SigningKey::from_bytes(&[byte; 32])
}

fn public_key(signer: &SigningKey) -> PublicKey {
    PublicKey::from_bytes(signer.verifying_key().to_bytes())
}

struct TestHumanCustody(ActorKey);

impl KeyCustody<ForActor> for TestHumanCustody {
    fn backend(&self) -> CustodyBackend {
        CustodyBackend::HardwareToken
    }

    fn public_key(&self) -> KeyPair<ForActor> {
        self.0
    }

    fn sign(&self, _payload: &SigningPayload) -> Result<Signature, CustodyError> {
        Err(CustodyError::BackendUnavailable)
    }
}

impl HumanKeyCustody for TestHumanCustody {}

fn authority(signer: &SigningKey) -> PublicationAuthority {
    let key = ActorKey::from_public_key(public_key(signer));
    let custody = TestHumanCustody(key);
    let attestation = custody.attest_human().expect("test custody attests");
    let epoch = PolicyEpoch::new(7);
    let workspace = WorkspaceScope::from_bytes([0x5a; 16]);
    let capability = Capability::<HumanHeld>::root(
        &attestation,
        workspace,
        [HumanAction::AdvanceCanonicalHead],
        epoch,
        Expiry::at_unix_millis(10_000),
        DelegationBudget::new(0),
    );
    let principal =
        HumanPrincipal::enrol(Principal::new(key, ActorKind::Human)).expect("human principal");
    let request = AuthorityRequest::new(key, workspace, Operation::AdvanceCanonicalHead, 5_000);
    let (_ledger, result) = DecisionLedger::empty().authorize_publication(
        &EpochChain::genesis(epoch),
        &principal,
        &capability,
        &request,
    );
    result.expect("recorded HumanHeld authority")
}

fn draft(signer: &SigningKey, role: ActorKind, decision: ApprovalDecision) -> ApprovalReceiptDraft {
    ApprovalReceiptDraft::new(
        bundle(1),
        head(2),
        head(3),
        public_key(signer),
        role,
        decision,
    )
}

fn signed(draft: ApprovalReceiptDraft, signer: &SigningKey) -> ApprovalReceipt {
    let signature = signer.sign(draft.signing_payload().as_bytes());
    draft.with_signature(Signature::from_bytes(signature.to_bytes()))
}

fn expected(key: PublicKey) -> ExpectedAdvance {
    ExpectedAdvance::new(bundle(1), head(2), head(3), key)
}

#[test]
fn a_real_human_signature_yields_the_exact_advance_authority() {
    let signer = signing_key(7);
    let receipt = signed(
        draft(&signer, ActorKind::Human, ApprovalDecision::Approve),
        &signer,
    );

    let authority = verify_approval_receipt(
        &receipt.canonical_bytes(),
        expected(public_key(&signer)),
        &authority(&signer),
    )
    .expect("the real signature and exact context must verify");

    assert_eq!(authority.current_shared_parent(), head(2));
    assert_eq!(authority.approved_target(), head(3));
    assert_eq!(authority.receipt(), &receipt);
}

#[test]
fn canonical_receipts_round_trip_byte_for_byte() {
    let signer = signing_key(7);
    let receipt = signed(
        draft(&signer, ActorKind::Human, ApprovalDecision::Approve),
        &signer,
    );
    let bytes = receipt.canonical_bytes();

    let decoded = ApprovalReceipt::from_canonical_bytes(&bytes).expect("canonical receipt decodes");

    assert_eq!(decoded, receipt);
    assert_eq!(decoded.canonical_bytes(), bytes);
}

#[test]
fn every_signed_field_is_covered_by_the_signature() {
    let signer = signing_key(7);
    let other_signer = signing_key(8);
    let original = draft(&signer, ActorKind::Human, ApprovalDecision::Approve);
    let signature = signer
        .sign(original.signing_payload().as_bytes())
        .to_bytes();
    let signature = Signature::from_bytes(signature);

    let mutations = [
        ApprovalReceiptDraft::new(
            bundle(9),
            head(2),
            head(3),
            public_key(&signer),
            ActorKind::Human,
            ApprovalDecision::Approve,
        ),
        ApprovalReceiptDraft::new(
            bundle(1),
            head(9),
            head(3),
            public_key(&signer),
            ActorKind::Human,
            ApprovalDecision::Approve,
        ),
        ApprovalReceiptDraft::new(
            bundle(1),
            head(2),
            head(9),
            public_key(&signer),
            ActorKind::Human,
            ApprovalDecision::Approve,
        ),
        ApprovalReceiptDraft::new(
            bundle(1),
            head(2),
            head(3),
            public_key(&other_signer),
            ActorKind::Human,
            ApprovalDecision::Approve,
        ),
        ApprovalReceiptDraft::new(
            bundle(1),
            head(2),
            head(3),
            public_key(&signer),
            ActorKind::Agent,
            ApprovalDecision::Approve,
        ),
        ApprovalReceiptDraft::new(
            bundle(1),
            head(2),
            head(3),
            public_key(&signer),
            ActorKind::Human,
            ApprovalDecision::Reject,
        ),
    ];

    for mutation in mutations {
        let receipt = mutation.with_signature(signature);
        assert!(matches!(
            verify_approval_receipt(
                &receipt.canonical_bytes(),
                expected(public_key(&signer)),
                &authority(&signer),
            ),
            Err(ApprovalReceiptError::Signature(_))
        ));
    }
}

#[test]
fn valid_signatures_still_refuse_the_wrong_publication_context() {
    let signer = signing_key(7);
    let receipt = signed(
        draft(&signer, ActorKind::Human, ApprovalDecision::Approve),
        &signer,
    );
    let bytes = receipt.canonical_bytes();

    let cases = [
        (
            ExpectedAdvance::new(bundle(9), head(2), head(3), public_key(&signer)),
            ApprovalReceiptError::WrongReviewBundle,
        ),
        (
            ExpectedAdvance::new(bundle(1), head(9), head(3), public_key(&signer)),
            ApprovalReceiptError::WrongSharedParent,
        ),
        (
            ExpectedAdvance::new(bundle(1), head(2), head(9), public_key(&signer)),
            ApprovalReceiptError::WrongApprovedTarget,
        ),
        (
            ExpectedAdvance::new(bundle(1), head(2), head(3), public_key(&signing_key(8))),
            ApprovalReceiptError::WrongReviewerKey,
        ),
    ];

    for (context, error) in cases {
        assert_eq!(
            verify_approval_receipt(&bytes, context, &authority(&signer)),
            Err(error)
        );
    }
}

#[test]
fn a_non_human_role_and_a_rejection_never_yield_advance_authority() {
    let signer = signing_key(7);
    let agent = signed(
        draft(&signer, ActorKind::Agent, ApprovalDecision::Approve),
        &signer,
    );
    assert_eq!(
        verify_approval_receipt(
            &agent.canonical_bytes(),
            expected(public_key(&signer)),
            &authority(&signer),
        ),
        Err(ApprovalReceiptError::ReviewerIsNotHuman {
            found: ActorKind::Agent,
        })
    );

    let rejection = signed(
        draft(&signer, ActorKind::Human, ApprovalDecision::Reject),
        &signer,
    );
    assert_eq!(
        verify_approval_receipt(
            &rejection.canonical_bytes(),
            expected(public_key(&signer)),
            &authority(&signer),
        ),
        Err(ApprovalReceiptError::DecisionIsNotApproval {
            found: ApprovalDecision::Reject,
        })
    );
}

#[test]
fn another_signature_domain_cannot_authorize_publication() {
    let signer = signing_key(7);
    let draft = draft(&signer, ActorKind::Human, ApprovalDecision::Approve);
    let wrong_payload = SigningPayload::new(SIGNATURE_DOMAIN_MUTATION, &draft.canonical_bytes());
    let receipt = draft.with_signature(Signature::from_bytes(
        signer.sign(wrong_payload.as_bytes()).to_bytes(),
    ));

    assert!(matches!(
        verify_approval_receipt(
            &receipt.canonical_bytes(),
            expected(public_key(&signer)),
            &authority(&signer),
        ),
        Err(ApprovalReceiptError::Signature(_))
    ));
}

#[test]
fn a_wrong_record_domain_is_rejected_before_verification() {
    let signer = signing_key(7);
    let receipt = signed(
        draft(&signer, ActorKind::Human, ApprovalDecision::Approve),
        &signer,
    );
    let mut bytes = receipt.canonical_bytes();
    let domain = b"mesh.v0.approval-receipt";
    let offset = bytes
        .windows(domain.len())
        .position(|window| window == domain)
        .expect("receipt domain is present");
    bytes[offset + domain.len() - 1] = b'u';

    assert!(matches!(
        ApprovalReceipt::from_canonical_bytes(&bytes),
        Err(ApprovalReceiptError::Decode(
            DecodeError::WrongDomain { .. }
        ))
    ));
}

#[test]
fn noncanonical_and_trailing_bytes_are_rejected() {
    let signer = signing_key(7);
    let receipt = signed(
        draft(&signer, ActorKind::Human, ApprovalDecision::Approve),
        &signer,
    );
    let bytes = receipt.canonical_bytes();
    assert_eq!(
        bytes[0], 0x88,
        "domain plus seven fields is an eight-item array"
    );

    let mut widened_array_head = vec![0x98, 0x08];
    widened_array_head.extend_from_slice(&bytes[1..]);
    assert!(matches!(
        ApprovalReceipt::from_canonical_bytes(&widened_array_head),
        Err(ApprovalReceiptError::Decode(DecodeError::Cbor(_)))
    ));

    let mut trailing = bytes;
    trailing.push(0);
    assert!(matches!(
        ApprovalReceipt::from_canonical_bytes(&trailing),
        Err(ApprovalReceiptError::Decode(
            DecodeError::TrailingBytes { .. }
        ))
    ));
}

#[test]
fn a_corrupt_signature_is_rejected() {
    let signer = signing_key(7);
    let receipt = signed(
        draft(&signer, ActorKind::Human, ApprovalDecision::Approve),
        &signer,
    );
    let mut signature = *receipt.signature().as_bytes();
    signature[0] ^= 1;
    let corrupt = receipt
        .draft()
        .clone()
        .with_signature(Signature::from_bytes(signature));

    assert!(matches!(
        verify_approval_receipt(
            &corrupt.canonical_bytes(),
            expected(public_key(&signer)),
            &authority(&signer),
        ),
        Err(ApprovalReceiptError::Signature(_))
    ));
}

#[test]
fn a_valid_software_signature_cannot_substitute_for_humanheld_authority() {
    let signer = signing_key(7);
    let other_human = signing_key(8);
    let receipt = signed(
        draft(&signer, ActorKind::Human, ApprovalDecision::Approve),
        &signer,
    );

    assert_eq!(
        verify_approval_receipt(
            &receipt.canonical_bytes(),
            expected(public_key(&signer)),
            &authority(&other_human),
        ),
        Err(ApprovalReceiptError::WrongPublicationAuthority)
    );
}
