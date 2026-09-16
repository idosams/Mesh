//! User-verified P-256 approval receipts.
//!
//! The legacy receipt in [`crate::receipt`] proves only that an Ed25519 software key signed a
//! statement. This module is a separate protocol version: it binds the complete approval context
//! to an enrolled P-256 credential and verifies the ASN.1 DER signature shape emitted by Apple's
//! Secure Enclave. Private-key custody and the user-presence ceremony deliberately do not live in
//! this crate.

use core::fmt;

use mesh_types::{
    decode_canonical, encode_canonical, CanonicalEncode, CanonicalType, CanonicalValue,
    DecodeError, FieldSchema, PolicyEpoch, RecordSchema,
};
use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_ASN1};

use crate::{
    Absorb, ApprovalDecision, Blake3, ContentDigest, Digest32, DigestWriter, DomainTag, HeadId,
    ReviewBundle, ReviewBundleId,
};

/// The fixed application scope of the first production human credential.
pub const APPROVAL_APPLICATION_SCOPE: &str = "dev.mesh.desktop";
/// The algorithm name carried by the protocol.
pub const APPROVAL_ALGORITHM: &str = "es256";
/// The successful authenticator property required for every production approval.
pub const APPROVAL_USER_VERIFICATION: &str = "user-presence";
/// The only selection supported by the alpha ceremony.
pub const APPROVAL_SELECTION: &str = "all-reviewed-changes";

const STATEMENT_DOMAIN: mesh_types::DomainTag =
    mesh_types::DomainTag::new("mesh.v1.approval-receipt-statement");
const RECEIPT_DOMAIN: mesh_types::DomainTag =
    mesh_types::DomainTag::new("mesh.v1.approval-receipt");
const CREDENTIAL_DOMAIN: DomainTag = DomainTag::new("mesh.v1.approval-credential");
const SELECTED_CHANGES_DOMAIN: DomainTag = DomainTag::new("mesh.v1.approval-selected-changes");
const CONFLICT_RESOLUTIONS_DOMAIN: DomainTag =
    DomainTag::new("mesh.v1.approval-conflict-resolutions");
const VALIDATIONS_DOMAIN: DomainTag = DomainTag::new("mesh.v1.approval-validations");

const P256_PUBLIC_KEY_BYTES: usize = 65;
const CHALLENGE_BYTES: usize = 32;
const MIN_DER_SIGNATURE_BYTES: usize = 8;
const MAX_DER_SIGNATURE_BYTES: usize = 72;

const STATEMENT_FIELDS: [FieldSchema; 15] = [
    FieldSchema::new("workspace_id", CanonicalType::Bytes(Some(16))),
    FieldSchema::new("expected_canonical_head", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("reviewed_actor_head", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("review_bundle", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("selected_changes", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("conflict_resolutions", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("validation_digest", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("policy_epoch", CanonicalType::Unsigned),
    FieldSchema::new("approved_by", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("algorithm", CanonicalType::Text),
    FieldSchema::new(
        "credential_public_key",
        CanonicalType::Bytes(Some(P256_PUBLIC_KEY_BYTES as u32)),
    ),
    FieldSchema::new("application_scope", CanonicalType::Text),
    FieldSchema::new("user_verification", CanonicalType::Text),
    FieldSchema::new(
        "ceremony_challenge",
        CanonicalType::Bytes(Some(CHALLENGE_BYTES as u32)),
    ),
    FieldSchema::new("decision", CanonicalType::Text),
];

const RECEIPT_FIELDS: [FieldSchema; 16] = [
    STATEMENT_FIELDS[0],
    STATEMENT_FIELDS[1],
    STATEMENT_FIELDS[2],
    STATEMENT_FIELDS[3],
    STATEMENT_FIELDS[4],
    STATEMENT_FIELDS[5],
    STATEMENT_FIELDS[6],
    STATEMENT_FIELDS[7],
    STATEMENT_FIELDS[8],
    STATEMENT_FIELDS[9],
    STATEMENT_FIELDS[10],
    STATEMENT_FIELDS[11],
    STATEMENT_FIELDS[12],
    STATEMENT_FIELDS[13],
    STATEMENT_FIELDS[14],
    FieldSchema::new("signature", CanonicalType::Bytes(None)),
];

/// An enrolled user-verifying approval credential. It contains public data only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HumanApprovalCredential {
    id: Digest32,
    public_key: [u8; P256_PUBLIC_KEY_BYTES],
}

impl HumanApprovalCredential {
    /// Bind an uncompressed SEC1 P-256 public key to Mesh's fixed app and algorithm scope.
    pub fn from_public_key(
        public_key: [u8; P256_PUBLIC_KEY_BYTES],
    ) -> Result<Self, HumanApprovalReceiptError> {
        if public_key[0] != 0x04 {
            return Err(HumanApprovalReceiptError::InvalidPublicKey);
        }
        let mut writer = DigestWriter::new(CREDENTIAL_DOMAIN, Blake3::hasher());
        writer.text(APPROVAL_ALGORITHM);
        writer.text(APPROVAL_APPLICATION_SCOPE);
        writer.bytes(&public_key);
        Ok(Self {
            id: writer.finish(),
            public_key,
        })
    }

    /// The derived credential identity; never caller-selected.
    #[must_use]
    pub const fn id(&self) -> Digest32 {
        self.id
    }

    /// The public verification key in uncompressed SEC1 form.
    #[must_use]
    pub const fn public_key(&self) -> &[u8; P256_PUBLIC_KEY_BYTES] {
        &self.public_key
    }
}

/// Exact bundle-derived facts that the native approval ceremony must present and sign.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HumanApprovalContext {
    workspace_id: ApprovalWorkspaceId,
    expected_canonical_head: HeadId,
    reviewed_actor_head: HeadId,
    review_bundle: ReviewBundleId,
    selected_changes: Digest32,
    conflict_resolutions: Digest32,
    validation_digest: Digest32,
    policy_epoch: PolicyEpoch,
}

/// The exact opaque 16-byte workspace identifier carried by the shipping ChangeSet protocol.
///
/// This is intentionally not [`mesh_types::WorkspaceId`]. That newer entity type requires UUIDv7
/// structure, while `mesh_operations::WorkspaceId` is an opaque fixed-width protocol identity.
/// Approval must bind the bytes already signed by the ChangeSet, not silently reinterpret them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApprovalWorkspaceId([u8; 16]);

impl ApprovalWorkspaceId {
    /// Preserve the exact workspace bytes from a decoded ChangeSet.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// The bytes signed by the human receipt.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl HumanApprovalContext {
    /// Derive the alpha approve-all context from the immutable bundle.
    ///
    /// Conflicted bundles are refused because selective conflict decisions are a later protocol;
    /// calling an unresolved bundle "approve all" would hide the choice a person must make.
    pub fn from_bundle(
        workspace_id: ApprovalWorkspaceId,
        policy_epoch: PolicyEpoch,
        bundle: &ReviewBundle,
    ) -> Result<Self, HumanApprovalReceiptError> {
        if !bundle.conflicts().is_empty() {
            return Err(HumanApprovalReceiptError::SelectiveApprovalUnavailable);
        }

        let mut selected = DigestWriter::new(SELECTED_CHANGES_DOMAIN, Blake3::hasher());
        selected.sequence(bundle.changes(), |writer, change| change.absorb(writer));
        let mut resolutions = DigestWriter::new(CONFLICT_RESOLUTIONS_DOMAIN, Blake3::hasher());
        resolutions.sequence::<u8>(&[], |_writer, _resolution| {});
        let mut validations = DigestWriter::new(VALIDATIONS_DOMAIN, Blake3::hasher());
        validations.sequence(bundle.validations(), |writer, validation| {
            validation.absorb(writer);
        });

        Ok(Self {
            workspace_id,
            expected_canonical_head: bundle.canonical_head(),
            reviewed_actor_head: bundle.actor_head(),
            review_bundle: bundle.id(),
            selected_changes: selected.finish(),
            conflict_resolutions: resolutions.finish(),
            validation_digest: validations.finish(),
            policy_epoch,
        })
    }

    /// The workspace whose protected head may advance.
    #[must_use]
    pub const fn workspace_id(&self) -> ApprovalWorkspaceId {
        self.workspace_id
    }

    /// The exact immutable bundle shown to the person.
    #[must_use]
    pub const fn review_bundle(&self) -> ReviewBundleId {
        self.review_bundle
    }

    /// The exact proposed actor head.
    #[must_use]
    pub const fn reviewed_actor_head(&self) -> HeadId {
        self.reviewed_actor_head
    }

    /// The canonical parent against which the compare-and-swap must still succeed.
    #[must_use]
    pub const fn expected_canonical_head(&self) -> HeadId {
        self.expected_canonical_head
    }

    /// The digest of the exact ordered change set selected by approve-all.
    #[must_use]
    pub const fn selected_changes(&self) -> Digest32 {
        self.selected_changes
    }

    /// The digest of the exact ordered conflict decisions. Alpha approve-all carries none.
    #[must_use]
    pub const fn conflict_resolutions(&self) -> Digest32 {
        self.conflict_resolutions
    }

    /// The digest of the validation evidence carried by the reviewed bundle.
    #[must_use]
    pub const fn validation_digest(&self) -> Digest32 {
        self.validation_digest
    }

    /// The policy epoch under which this decision was presented.
    #[must_use]
    pub const fn policy_epoch(&self) -> PolicyEpoch {
        self.policy_epoch
    }
}

/// All caller-independent facts expected from one native user-presence ceremony.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpectedHumanApproval {
    context: HumanApprovalContext,
    credential: HumanApprovalCredential,
    challenge: [u8; CHALLENGE_BYTES],
}

impl ExpectedHumanApproval {
    /// Bind verification to current workspace truth, one enrolled credential and one fresh nonce.
    #[must_use]
    pub const fn new(
        context: HumanApprovalContext,
        credential: HumanApprovalCredential,
        challenge: [u8; CHALLENGE_BYTES],
    ) -> Self {
        Self {
            context,
            credential,
            challenge,
        }
    }

    /// The exact workspace and review facts independently recomputed by the verifier.
    #[must_use]
    pub const fn context(&self) -> &HumanApprovalContext {
        &self.context
    }

    /// The enrolled public credential expected to have performed the ceremony.
    #[must_use]
    pub const fn credential(&self) -> &HumanApprovalCredential {
        &self.credential
    }

    /// The one-time challenge issued for this ceremony.
    #[must_use]
    pub const fn challenge(&self) -> &[u8; CHALLENGE_BYTES] {
        &self.challenge
    }
}

/// The exact statement passed to a user-verifying P-256 signer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HumanApprovalReceiptDraft {
    expected: ExpectedHumanApproval,
    decision: ApprovalDecision,
}

impl HumanApprovalReceiptDraft {
    /// Build the deterministic signing statement.
    #[must_use]
    pub const fn new(expected: ExpectedHumanApproval, decision: ApprovalDecision) -> Self {
        Self { expected, decision }
    }

    /// The canonical bytes signed by the platform authenticator.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        encode_canonical(self)
    }

    /// The BLAKE3 content digest of the exact canonical bytes passed to the authenticator.
    ///
    /// This is a presentation aid, not a second approval identity. A native surface can show one
    /// short value that moves when any signed field moves, while verification continues to use the
    /// canonical statement bytes themselves.
    #[must_use]
    pub fn statement_digest(&self) -> Digest32 {
        Blake3::digest_bytes(&self.canonical_bytes())
    }

    /// Attach the platform-returned DER signature. Verification is still required.
    pub fn with_signature(
        self,
        signature: Vec<u8>,
    ) -> Result<HumanApprovalReceipt, HumanApprovalReceiptError> {
        validate_signature_shape(&signature)?;
        Ok(HumanApprovalReceipt {
            draft: self,
            signature,
        })
    }

    /// The expected ceremony facts.
    #[must_use]
    pub const fn expected(&self) -> &ExpectedHumanApproval {
        &self.expected
    }
}

impl CanonicalEncode for HumanApprovalReceiptDraft {
    const SCHEMA: RecordSchema = RecordSchema::new(STATEMENT_DOMAIN, &STATEMENT_FIELDS);

    fn canonical_fields(&self) -> Vec<CanonicalValue> {
        statement_fields(self)
    }
}

/// Canonical ES256 receipt returned by a native user-presence ceremony.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HumanApprovalReceipt {
    draft: HumanApprovalReceiptDraft,
    signature: Vec<u8>,
}

impl HumanApprovalReceipt {
    /// Encode the one accepted `mesh-cbor/0` spelling.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        encode_canonical(self)
    }

    /// Decode structural facts only; authority requires [`verify_human_approval_receipt`].
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, HumanApprovalReceiptError> {
        let fields = decode_canonical::<Self>(bytes).map_err(HumanApprovalReceiptError::Decode)?;
        let mut fields = fields.into_iter();
        let workspace_bytes = fixed_bytes::<16>(fields.next(), "workspace_id")?;
        let workspace_id = ApprovalWorkspaceId::from_bytes(workspace_bytes);
        let context = HumanApprovalContext {
            workspace_id,
            expected_canonical_head: HeadId::from_bytes(fixed_bytes(
                fields.next(),
                "expected_canonical_head",
            )?),
            reviewed_actor_head: HeadId::from_bytes(fixed_bytes(
                fields.next(),
                "reviewed_actor_head",
            )?),
            review_bundle: ReviewBundleId::from_digest(Digest32::from_bytes(fixed_bytes(
                fields.next(),
                "review_bundle",
            )?)),
            selected_changes: Digest32::from_bytes(fixed_bytes(fields.next(), "selected_changes")?),
            conflict_resolutions: Digest32::from_bytes(fixed_bytes(
                fields.next(),
                "conflict_resolutions",
            )?),
            validation_digest: Digest32::from_bytes(fixed_bytes(
                fields.next(),
                "validation_digest",
            )?),
            policy_epoch: PolicyEpoch::new(unsigned_field(fields.next(), "policy_epoch")?),
        };
        let approved_by = Digest32::from_bytes(fixed_bytes(fields.next(), "approved_by")?);
        require_text(fields.next(), "algorithm", APPROVAL_ALGORITHM)?;
        let public_key = fixed_bytes(fields.next(), "credential_public_key")?;
        let credential = HumanApprovalCredential::from_public_key(public_key)?;
        if credential.id() != approved_by {
            return Err(HumanApprovalReceiptError::CredentialIdentityMismatch);
        }
        require_text(
            fields.next(),
            "application_scope",
            APPROVAL_APPLICATION_SCOPE,
        )?;
        require_text(
            fields.next(),
            "user_verification",
            APPROVAL_USER_VERIFICATION,
        )?;
        let challenge = fixed_bytes(fields.next(), "ceremony_challenge")?;
        let decision_text = text_field(fields.next(), "decision")?;
        let decision = match decision_text.as_str() {
            "approve" => ApprovalDecision::Approve,
            "reject" => ApprovalDecision::Reject,
            _ => return Err(HumanApprovalReceiptError::UnknownDecision(decision_text)),
        };
        let signature = variable_bytes(fields.next(), "signature")?;
        validate_signature_shape(&signature)?;
        if fields.next().is_some() {
            return Err(HumanApprovalReceiptError::InvalidFieldShape("receipt"));
        }
        HumanApprovalReceiptDraft::new(
            ExpectedHumanApproval::new(context, credential, challenge),
            decision,
        )
        .with_signature(signature)
    }

    /// The signed statement.
    #[must_use]
    pub const fn draft(&self) -> &HumanApprovalReceiptDraft {
        &self.draft
    }

    /// The platform DER signature, for display or durable transport after verification.
    #[must_use]
    pub fn signature(&self) -> &[u8] {
        &self.signature
    }
}

impl CanonicalEncode for HumanApprovalReceipt {
    const SCHEMA: RecordSchema = RecordSchema::new(RECEIPT_DOMAIN, &RECEIPT_FIELDS);

    fn canonical_fields(&self) -> Vec<CanonicalValue> {
        let mut fields = statement_fields(&self.draft);
        fields.push(CanonicalValue::Bytes(self.signature.clone()));
        fields
    }
}

/// Opaque result of signature, user-verification marker, nonce and exact-context verification.
#[derive(Debug, PartialEq, Eq)]
pub struct VerifiedHumanApprovalReceipt {
    receipt: HumanApprovalReceipt,
}

impl VerifiedHumanApprovalReceipt {
    /// The verified receipt retained for durable audit.
    #[must_use]
    pub const fn receipt(&self) -> &HumanApprovalReceipt {
        &self.receipt
    }
}

/// Verify one native human receipt against current workspace and enrolled-credential truth.
pub fn verify_human_approval_receipt(
    bytes: &[u8],
    expected: &ExpectedHumanApproval,
) -> Result<VerifiedHumanApprovalReceipt, HumanApprovalReceiptError> {
    let receipt = HumanApprovalReceipt::from_canonical_bytes(bytes)?;
    UnparsedPublicKey::new(
        &ECDSA_P256_SHA256_ASN1,
        receipt.draft.expected.credential.public_key(),
    )
    .verify(&receipt.draft.canonical_bytes(), &receipt.signature)
    .map_err(|_| HumanApprovalReceiptError::SignatureInvalid)?;

    if receipt.draft.decision != ApprovalDecision::Approve {
        return Err(HumanApprovalReceiptError::DecisionIsNotApproval);
    }
    if &receipt.draft.expected != expected {
        return Err(HumanApprovalReceiptError::WrongExpectedContext);
    }
    Ok(VerifiedHumanApprovalReceipt { receipt })
}

/// Why a human approval receipt was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HumanApprovalReceiptError {
    /// The bytes were not canonical for this exact record schema.
    Decode(DecodeError),
    /// A field did not have its declared shape.
    InvalidFieldShape(&'static str),
    /// The P-256 public key was not uncompressed SEC1 form.
    InvalidPublicKey,
    /// The derived credential identifier did not match the carried key.
    CredentialIdentityMismatch,
    /// A fixed protocol vocabulary value was substituted.
    WrongProtocolValue(&'static str),
    /// The decision was outside the closed vocabulary.
    UnknownDecision(String),
    /// The DER signature was outside its bounded P-256 shape.
    InvalidSignatureShape,
    /// P-256/SHA-256 verification failed.
    SignatureInvalid,
    /// A correctly signed rejection cannot publish.
    DecisionIsNotApproval,
    /// A signed field, nonce or enrolled credential differed from current truth.
    WrongExpectedContext,
    /// Alpha approve-all cannot silently resolve conflicts.
    SelectiveApprovalUnavailable,
}

impl fmt::Display for HumanApprovalReceiptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(error) => write!(
                formatter,
                "human approval receipt is not canonical: {error}"
            ),
            Self::InvalidFieldShape(field) => write!(
                formatter,
                "human approval field `{field}` has the wrong shape"
            ),
            Self::InvalidPublicKey => {
                formatter.write_str("human approval credential is not an uncompressed P-256 key")
            }
            Self::CredentialIdentityMismatch => formatter
                .write_str("human approval credential identity does not match its public key"),
            Self::WrongProtocolValue(field) => {
                write!(formatter, "human approval field `{field}` is not supported")
            }
            Self::UnknownDecision(decision) => {
                write!(formatter, "human approval decision `{decision}` is unknown")
            }
            Self::InvalidSignatureShape => {
                formatter.write_str("human approval signature has the wrong DER shape")
            }
            Self::SignatureInvalid => {
                formatter.write_str("human approval signature did not verify")
            }
            Self::DecisionIsNotApproval => {
                formatter.write_str("human approval receipt is not an approval")
            }
            Self::WrongExpectedContext => {
                formatter.write_str("human approval receipt does not match current workspace truth")
            }
            Self::SelectiveApprovalUnavailable => {
                formatter.write_str("this alpha cannot approve a bundle with unresolved conflicts")
            }
        }
    }
}

impl std::error::Error for HumanApprovalReceiptError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Decode(error) => Some(error),
            _ => None,
        }
    }
}

fn statement_fields(draft: &HumanApprovalReceiptDraft) -> Vec<CanonicalValue> {
    let context = &draft.expected.context;
    let credential = &draft.expected.credential;
    vec![
        CanonicalValue::Bytes(context.workspace_id.as_bytes().to_vec()),
        CanonicalValue::Bytes(context.expected_canonical_head.as_bytes().to_vec()),
        CanonicalValue::Bytes(context.reviewed_actor_head.as_bytes().to_vec()),
        CanonicalValue::Bytes(context.review_bundle.digest().as_bytes().to_vec()),
        CanonicalValue::Bytes(context.selected_changes.as_bytes().to_vec()),
        CanonicalValue::Bytes(context.conflict_resolutions.as_bytes().to_vec()),
        CanonicalValue::Bytes(context.validation_digest.as_bytes().to_vec()),
        CanonicalValue::Unsigned(context.policy_epoch.value()),
        CanonicalValue::Bytes(credential.id.as_bytes().to_vec()),
        CanonicalValue::Text(APPROVAL_ALGORITHM.to_owned()),
        CanonicalValue::Bytes(credential.public_key.to_vec()),
        CanonicalValue::Text(APPROVAL_APPLICATION_SCOPE.to_owned()),
        CanonicalValue::Text(APPROVAL_USER_VERIFICATION.to_owned()),
        CanonicalValue::Bytes(draft.expected.challenge.to_vec()),
        CanonicalValue::Text(draft.decision.as_str().to_owned()),
    ]
}

fn fixed_bytes<const N: usize>(
    value: Option<CanonicalValue>,
    field: &'static str,
) -> Result<[u8; N], HumanApprovalReceiptError> {
    match value {
        Some(CanonicalValue::Bytes(bytes)) => bytes
            .try_into()
            .map_err(|_| HumanApprovalReceiptError::InvalidFieldShape(field)),
        _ => Err(HumanApprovalReceiptError::InvalidFieldShape(field)),
    }
}

fn variable_bytes(
    value: Option<CanonicalValue>,
    field: &'static str,
) -> Result<Vec<u8>, HumanApprovalReceiptError> {
    match value {
        Some(CanonicalValue::Bytes(bytes)) => Ok(bytes),
        _ => Err(HumanApprovalReceiptError::InvalidFieldShape(field)),
    }
}

fn text_field(
    value: Option<CanonicalValue>,
    field: &'static str,
) -> Result<String, HumanApprovalReceiptError> {
    match value {
        Some(CanonicalValue::Text(text)) => Ok(text),
        _ => Err(HumanApprovalReceiptError::InvalidFieldShape(field)),
    }
}

fn require_text(
    value: Option<CanonicalValue>,
    field: &'static str,
    expected: &str,
) -> Result<(), HumanApprovalReceiptError> {
    if text_field(value, field)? == expected {
        Ok(())
    } else {
        Err(HumanApprovalReceiptError::WrongProtocolValue(field))
    }
}

fn unsigned_field(
    value: Option<CanonicalValue>,
    field: &'static str,
) -> Result<u64, HumanApprovalReceiptError> {
    match value {
        Some(CanonicalValue::Unsigned(number)) => Ok(number),
        _ => Err(HumanApprovalReceiptError::InvalidFieldShape(field)),
    }
}

fn validate_signature_shape(signature: &[u8]) -> Result<(), HumanApprovalReceiptError> {
    if (MIN_DER_SIGNATURE_BYTES..=MAX_DER_SIGNATURE_BYTES).contains(&signature.len()) {
        Ok(())
    } else {
        Err(HumanApprovalReceiptError::InvalidSignatureShape)
    }
}
