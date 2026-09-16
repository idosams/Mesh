//! Canonical, signed approval receipts and the one path that turns them into advance authority.
//!
//! A receipt is untrusted data until [`verify_approval_receipt`] returns a
//! [`HumanApprovedAdvance`]. The verifier decodes only `mesh-cbor/0`'s canonical spelling, checks a
//! real Ed25519 signature through `mesh-crypto`, requires a human reviewer and an approving
//! decision, then binds the receipt to the bundle, shared parent, target and reviewer key expected
//! by the caller. It additionally requires mesh-policy's opaque [`PublicationAuthority`], so role
//! text and a software-held signing key cannot manufacture human publication authority.

use core::fmt;

use mesh_crypto::{DomainSeparator, Ed25519, SignatureScheme, SigningPayload, VerifyError};
use mesh_policy::PublicationAuthority;
use mesh_types::{
    decode_canonical, encode_canonical, ActorKind, CanonicalEncode, CanonicalType, CanonicalValue,
    DecodeError, DomainTag as TypesDomainTag, FieldSchema, PublicKey, RecordSchema, Signature,
};

use crate::{Digest32, HeadId, ReviewBundleId};

/// The canonical record domain for a complete approval receipt.
const RECEIPT_DOMAIN: TypesDomainTag = TypesDomainTag::new("mesh.v0.approval-receipt");
/// The canonical record domain for the fields covered by the signature.
const STATEMENT_DOMAIN: TypesDomainTag = TypesDomainTag::new("mesh.v0.approval-receipt-statement");
/// The signature framing domain. A valid signature from another Mesh protocol position cannot be
/// replayed as publication approval even if its body bytes happen to match.
const SIGNATURE_DOMAIN: DomainSeparator =
    DomainSeparator::new("mesh.v0.approval-receipt-signature");

const STATEMENT_FIELDS: [FieldSchema; 6] = [
    FieldSchema::new("review_bundle", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("current_shared_parent", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("approved_target", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("reviewer_public_key", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("reviewer_role", CanonicalType::Text),
    FieldSchema::new("decision", CanonicalType::Text),
];

const RECEIPT_FIELDS: [FieldSchema; 7] = [
    FieldSchema::new("review_bundle", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("current_shared_parent", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("approved_target", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("reviewer_public_key", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("reviewer_role", CanonicalType::Text),
    FieldSchema::new("decision", CanonicalType::Text),
    FieldSchema::new("signature", CanonicalType::Bytes(Some(64))),
];

/// What the reviewer decided about the exact target named by a receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ApprovalDecision {
    /// Permit the exact bound target to advance from the exact bound parent.
    Approve,
    /// Refuse publication of the bound target.
    Reject,
}

impl ApprovalDecision {
    /// The canonical wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Reject => "reject",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "approve" => Some(Self::Approve),
            "reject" => Some(Self::Reject),
            _ => None,
        }
    }
}

impl fmt::Display for ApprovalDecision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The six fields a reviewer signs before the signature is attached to a receipt.
///
/// This is not authority. It is a deterministic signing request whose canonical bytes are framed
/// in the approval-receipt signature domain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalReceiptDraft {
    review_bundle: ReviewBundleId,
    current_shared_parent: HeadId,
    approved_target: HeadId,
    reviewer_public_key: PublicKey,
    reviewer_role: ActorKind,
    decision: ApprovalDecision,
}

impl ApprovalReceiptDraft {
    /// Compose the exact statement a reviewer is being asked to sign.
    #[must_use]
    pub const fn new(
        review_bundle: ReviewBundleId,
        current_shared_parent: HeadId,
        approved_target: HeadId,
        reviewer_public_key: PublicKey,
        reviewer_role: ActorKind,
        decision: ApprovalDecision,
    ) -> Self {
        Self {
            review_bundle,
            current_shared_parent,
            approved_target,
            reviewer_public_key,
            reviewer_role,
            decision,
        }
    }

    /// The review bundle whose exact bytes were presented.
    #[must_use]
    pub const fn review_bundle(&self) -> ReviewBundleId {
        self.review_bundle
    }

    /// The shared head that must still be current when the advance is attempted.
    #[must_use]
    pub const fn current_shared_parent(&self) -> HeadId {
        self.current_shared_parent
    }

    /// The exact head the reviewer approved as the next shared version.
    #[must_use]
    pub const fn approved_target(&self) -> HeadId {
        self.approved_target
    }

    /// The public key whose signature must verify.
    #[must_use]
    pub const fn reviewer_public_key(&self) -> PublicKey {
        self.reviewer_public_key
    }

    /// The actor role bound by the signature.
    #[must_use]
    pub const fn reviewer_role(&self) -> ActorKind {
        self.reviewer_role
    }

    /// The decision bound by the signature.
    #[must_use]
    pub const fn decision(&self) -> ApprovalDecision {
        self.decision
    }

    /// The canonical statement bytes inside the signature framing.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        encode_canonical(self)
    }

    /// The exact payload a human key must sign.
    #[must_use]
    pub fn signing_payload(&self) -> SigningPayload {
        SigningPayload::new(SIGNATURE_DOMAIN, &self.canonical_bytes())
    }

    /// Attach an untrusted signature, producing a receipt that still requires verification.
    #[must_use]
    pub const fn with_signature(self, signature: Signature) -> ApprovalReceipt {
        ApprovalReceipt {
            draft: self,
            signature,
        }
    }
}

impl CanonicalEncode for ApprovalReceiptDraft {
    const SCHEMA: RecordSchema = RecordSchema::new(STATEMENT_DOMAIN, &STATEMENT_FIELDS);

    fn canonical_fields(&self) -> Vec<CanonicalValue> {
        statement_fields(self)
    }
}

/// One canonical signed decision over one exact shared-head advance.
///
/// Construction does not imply validity. Only [`verify_approval_receipt`] returns the opaque
/// authority witness consumed by a publication path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalReceipt {
    draft: ApprovalReceiptDraft,
    signature: Signature,
}

impl ApprovalReceipt {
    /// The signed fields.
    #[must_use]
    pub const fn draft(&self) -> &ApprovalReceiptDraft {
        &self.draft
    }

    /// The attached Ed25519 signature.
    #[must_use]
    pub const fn signature(&self) -> &Signature {
        &self.signature
    }

    /// Encode this receipt in its one accepted `mesh-cbor/0` spelling.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        encode_canonical(self)
    }

    /// Decode only the canonical receipt spelling.
    ///
    /// This checks structure and canonical bytes, not authority. Use
    /// [`verify_approval_receipt`] before publication.
    ///
    /// # Errors
    ///
    /// [`ApprovalReceiptError`] for a wrong domain, non-canonical CBOR, malformed field, unknown
    /// role or unknown decision.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, ApprovalReceiptError> {
        let fields = decode_canonical::<Self>(bytes).map_err(ApprovalReceiptError::Decode)?;
        let mut fields = fields.into_iter();
        let review_bundle = ReviewBundleId::from_digest(Digest32::from_bytes(fixed_bytes(
            fields.next(),
            "review_bundle",
        )?));
        let current_shared_parent =
            HeadId::from_bytes(fixed_bytes(fields.next(), "current_shared_parent")?);
        let approved_target = HeadId::from_bytes(fixed_bytes(fields.next(), "approved_target")?);
        let reviewer_public_key =
            PublicKey::from_bytes(fixed_bytes(fields.next(), "reviewer_public_key")?);
        let reviewer_role_text = text_field(fields.next(), "reviewer_role")?;
        let reviewer_role = ActorKind::ALL
            .into_iter()
            .find(|role| role.as_str() == reviewer_role_text)
            .ok_or(ApprovalReceiptError::UnknownReviewerRole(
                reviewer_role_text,
            ))?;
        let decision_text = text_field(fields.next(), "decision")?;
        let decision = ApprovalDecision::parse(&decision_text)
            .ok_or(ApprovalReceiptError::UnknownDecision(decision_text))?;
        let signature = Signature::from_bytes(fixed_bytes(fields.next(), "signature")?);
        if fields.next().is_some() {
            return Err(ApprovalReceiptError::InvalidFieldShape("receipt"));
        }
        Ok(ApprovalReceiptDraft::new(
            review_bundle,
            current_shared_parent,
            approved_target,
            reviewer_public_key,
            reviewer_role,
            decision,
        )
        .with_signature(signature))
    }
}

impl CanonicalEncode for ApprovalReceipt {
    const SCHEMA: RecordSchema = RecordSchema::new(RECEIPT_DOMAIN, &RECEIPT_FIELDS);

    fn canonical_fields(&self) -> Vec<CanonicalValue> {
        let mut fields = statement_fields(&self.draft);
        fields.push(CanonicalValue::Bytes(self.signature.as_bytes().to_vec()));
        fields
    }
}

/// The exact context in which a verified receipt may be consumed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpectedAdvance {
    review_bundle: ReviewBundleId,
    current_shared_parent: HeadId,
    approved_target: HeadId,
    reviewer_public_key: PublicKey,
}

impl ExpectedAdvance {
    /// Bind verification to the caller's current shared state, candidate and reviewer identity.
    #[must_use]
    pub const fn new(
        review_bundle: ReviewBundleId,
        current_shared_parent: HeadId,
        approved_target: HeadId,
        reviewer_public_key: PublicKey,
    ) -> Self {
        Self {
            review_bundle,
            current_shared_parent,
            approved_target,
            reviewer_public_key,
        }
    }
}

/// Opaque proof that one receipt's bytes, signature and expected context verified.
///
/// This is deliberately not publication authority. In particular, a software-held key can
/// produce one. Only [`verify_approval_receipt`] can combine it with mesh-policy's recorded
/// [`PublicationAuthority`] and yield [`HumanApprovedAdvance`].
#[derive(Debug, PartialEq, Eq)]
pub struct VerifiedApprovalReceipt {
    receipt: ApprovalReceipt,
}

impl VerifiedApprovalReceipt {
    /// The verified receipt retained for audit and policy composition.
    #[must_use]
    pub const fn receipt(&self) -> &ApprovalReceipt {
        &self.receipt
    }
}

/// Opaque proof that one human-approved, signed receipt authorizes one exact shared-head advance.
///
/// There is no public constructor and this type is not `Clone` or `Copy`. A future compare-and-swap
/// admission layer can consume it once; replay prevention remains that admission layer's job.
#[derive(Debug, PartialEq, Eq)]
pub struct HumanApprovedAdvance {
    receipt: ApprovalReceipt,
    authority: PublicationAuthority,
}

impl HumanApprovedAdvance {
    /// The verified receipt retained for audit and downstream compare-and-swap binding.
    #[must_use]
    pub const fn receipt(&self) -> &ApprovalReceipt {
        &self.receipt
    }

    /// The expected shared parent bound by the human signature.
    #[must_use]
    pub const fn current_shared_parent(&self) -> HeadId {
        self.receipt.draft.current_shared_parent
    }

    /// The approved target bound by the human signature.
    #[must_use]
    pub const fn approved_target(&self) -> HeadId {
        self.receipt.draft.approved_target
    }

    /// The recorded HumanHeld policy decision that admitted this exact reviewer.
    #[must_use]
    pub const fn publication_authority(&self) -> PublicationAuthority {
        self.authority
    }
}

/// Decode and verify one receipt's signature and exact expected context.
///
/// This function yields no authority. A plain software key can satisfy signature verification, so
/// callers that need to advance the shared head must call [`verify_approval_receipt`] with the
/// separate opaque result of mesh-policy's recorded HumanHeld gate.
///
/// # Errors
///
/// [`ApprovalReceiptError`] for every malformed, non-canonical, incorrectly signed, non-human,
/// non-approving or context-mismatched receipt.
pub fn verify_approval_receipt_signature(
    bytes: &[u8],
    expected: ExpectedAdvance,
) -> Result<VerifiedApprovalReceipt, ApprovalReceiptError> {
    let receipt = ApprovalReceipt::from_canonical_bytes(bytes)?;
    Ed25519::verify(
        &receipt.draft.reviewer_public_key,
        receipt.draft.signing_payload().as_bytes(),
        &receipt.signature,
    )
    .map_err(ApprovalReceiptError::Signature)?;

    if receipt.draft.reviewer_role != ActorKind::Human {
        return Err(ApprovalReceiptError::ReviewerIsNotHuman {
            found: receipt.draft.reviewer_role,
        });
    }
    if receipt.draft.decision != ApprovalDecision::Approve {
        return Err(ApprovalReceiptError::DecisionIsNotApproval {
            found: receipt.draft.decision,
        });
    }
    if receipt.draft.review_bundle != expected.review_bundle {
        return Err(ApprovalReceiptError::WrongReviewBundle);
    }
    if receipt.draft.current_shared_parent != expected.current_shared_parent {
        return Err(ApprovalReceiptError::WrongSharedParent);
    }
    if receipt.draft.approved_target != expected.approved_target {
        return Err(ApprovalReceiptError::WrongApprovedTarget);
    }
    if receipt.draft.reviewer_public_key != expected.reviewer_public_key {
        return Err(ApprovalReceiptError::WrongReviewerKey);
    }
    Ok(VerifiedApprovalReceipt { receipt })
}

/// Decode and verify one receipt before yielding human-held advance authority.
///
/// Signature verification runs before semantic role, decision and expected-context checks, so a
/// byte mutation cannot be misreported as an authorized but mismatched receipt. A valid signature
/// over a non-human role or rejection still yields no authority.
///
/// # Errors
///
/// [`ApprovalReceiptError`] for every malformed, non-canonical, incorrectly signed, non-human,
/// non-approving or context-mismatched receipt.
pub fn verify_approval_receipt(
    bytes: &[u8],
    expected: ExpectedAdvance,
    authority: &PublicationAuthority,
) -> Result<HumanApprovedAdvance, ApprovalReceiptError> {
    let verified = verify_approval_receipt_signature(bytes, expected)?;
    if authority.approver().public_key() != verified.receipt.draft.reviewer_public_key {
        return Err(ApprovalReceiptError::WrongPublicationAuthority);
    }
    Ok(HumanApprovedAdvance {
        receipt: verified.receipt,
        authority: *authority,
    })
}

/// Why receipt decoding or verification refused publication authority.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ApprovalReceiptError {
    /// The bytes were not the one canonical encoding of this record schema.
    Decode(DecodeError),
    /// A decoded field did not have the shape declared by the schema.
    InvalidFieldShape(&'static str),
    /// The role text is not in the closed actor-kind vocabulary.
    UnknownReviewerRole(String),
    /// The decision text is not in the closed approval vocabulary.
    UnknownDecision(String),
    /// The Ed25519 signature did not verify over the framed canonical statement.
    Signature(VerifyError),
    /// A valid signature named a role that may not approve publication.
    ReviewerIsNotHuman {
        /// The role the signed receipt carried.
        found: ActorKind,
    },
    /// A valid signature carried a rejection rather than an approval.
    DecisionIsNotApproval {
        /// The signed decision.
        found: ApprovalDecision,
    },
    /// The signed bundle is not the bundle the caller is trying to publish.
    WrongReviewBundle,
    /// The signed parent is no longer the current shared head expected by the caller.
    WrongSharedParent,
    /// The signed target is not the target the caller is trying to publish.
    WrongApprovedTarget,
    /// The signing key is not the reviewer key authorized by the caller's trust context.
    WrongReviewerKey,
    /// The recorded HumanHeld policy decision belongs to another reviewer.
    WrongPublicationAuthority,
}

impl fmt::Display for ApprovalReceiptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(error) => write!(formatter, "approval receipt is not canonical: {error}"),
            Self::InvalidFieldShape(field) => {
                write!(
                    formatter,
                    "approval receipt field `{field}` has the wrong shape"
                )
            }
            Self::UnknownReviewerRole(role) => {
                write!(
                    formatter,
                    "approval receipt names unknown reviewer role `{role}`"
                )
            }
            Self::UnknownDecision(decision) => {
                write!(
                    formatter,
                    "approval receipt names unknown decision `{decision}`"
                )
            }
            Self::Signature(error) => {
                write!(formatter, "approval receipt signature failed: {error}")
            }
            Self::ReviewerIsNotHuman { found } => {
                write!(
                    formatter,
                    "reviewer role `{found}` cannot authorize shared publication"
                )
            }
            Self::DecisionIsNotApproval { found } => {
                write!(
                    formatter,
                    "decision `{found}` does not authorize shared publication"
                )
            }
            Self::WrongReviewBundle => {
                formatter.write_str("approval receipt names another review bundle")
            }
            Self::WrongSharedParent => {
                formatter.write_str("approval receipt names another shared parent")
            }
            Self::WrongApprovedTarget => {
                formatter.write_str("approval receipt names another target")
            }
            Self::WrongReviewerKey => {
                formatter.write_str("approval receipt names another reviewer key")
            }
            Self::WrongPublicationAuthority => formatter
                .write_str("approval receipt has no publication authority for its reviewer"),
        }
    }
}

impl std::error::Error for ApprovalReceiptError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Decode(error) => Some(error),
            Self::Signature(error) => Some(error),
            _ => None,
        }
    }
}

fn statement_fields(draft: &ApprovalReceiptDraft) -> Vec<CanonicalValue> {
    vec![
        CanonicalValue::Bytes(draft.review_bundle.digest().as_bytes().to_vec()),
        CanonicalValue::Bytes(draft.current_shared_parent.as_bytes().to_vec()),
        CanonicalValue::Bytes(draft.approved_target.as_bytes().to_vec()),
        CanonicalValue::Bytes(draft.reviewer_public_key.as_bytes().to_vec()),
        CanonicalValue::Text(draft.reviewer_role.as_str().to_owned()),
        CanonicalValue::Text(draft.decision.as_str().to_owned()),
    ]
}

fn fixed_bytes<const N: usize>(
    value: Option<CanonicalValue>,
    field: &'static str,
) -> Result<[u8; N], ApprovalReceiptError> {
    match value {
        Some(CanonicalValue::Bytes(bytes)) => bytes
            .try_into()
            .map_err(|_| ApprovalReceiptError::InvalidFieldShape(field)),
        _ => Err(ApprovalReceiptError::InvalidFieldShape(field)),
    }
}

fn text_field(
    value: Option<CanonicalValue>,
    field: &'static str,
) -> Result<String, ApprovalReceiptError> {
    match value {
        Some(CanonicalValue::Text(text)) => Ok(text),
        _ => Err(ApprovalReceiptError::InvalidFieldShape(field)),
    }
}
