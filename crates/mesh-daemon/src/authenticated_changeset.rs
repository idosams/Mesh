//! Canonical authenticated envelope for a locally authored ChangeSet.
//!
//! `mesh-operations` intentionally encodes only the statement a signature covers. That is the
//! correct signing preimage, but storing only those bytes loses the public key and signature.
//! This service-layer envelope persists all three without changing the published inner ChangeSet
//! schema. Legacy imported payloads remain readable; every payload using this envelope must verify
//! before its operations are materialized.

use core::fmt;

use mesh_crypto::{DomainSeparator, Ed25519, SignatureScheme, SigningPayload, VerifyError};
use mesh_types::{
    decode_canonical, encode_canonical, CanonicalEncode, CanonicalType, CanonicalValue,
    DecodeError, DomainTag, FieldSchema, PublicKey, RecordSchema, Signature,
};

/// Domain framing the exact canonical ChangeSet statement signed by its actor key.
pub(crate) const CHANGESET_SIGNATURE_DOMAIN: DomainSeparator =
    DomainSeparator::new("mesh.v0.changeset-author");

const ENVELOPE_FIELDS: [FieldSchema; 3] = [
    FieldSchema::new("changeset", CanonicalType::Bytes(None)),
    FieldSchema::new("actor_public_key", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("signature", CanonicalType::Bytes(Some(64))),
];

/// The persisted public proof for one locally authored canonical ChangeSet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AuthenticatedChangeSet {
    changeset: Vec<u8>,
    actor_public_key: PublicKey,
    signature: Signature,
}

impl AuthenticatedChangeSet {
    /// Verify and construct an envelope. Invalid keys, signatures, inner canonical bytes, or an
    /// actor field that differs from the public key are refused before storage.
    pub(crate) fn verified(
        changeset: Vec<u8>,
        actor_public_key: PublicKey,
        signature: Signature,
    ) -> Result<Self, AuthenticatedChangeSetError> {
        validate_inner_actor(&changeset, &actor_public_key)?;
        Ed25519::verify(
            &actor_public_key,
            SigningPayload::new(CHANGESET_SIGNATURE_DOMAIN, &changeset).as_bytes(),
            &signature,
        )
        .map_err(AuthenticatedChangeSetError::Signature)?;
        Ok(Self {
            changeset,
            actor_public_key,
            signature,
        })
    }

    /// Decode the sole canonical spelling and then perform the same cryptographic verification as
    /// the writer.
    pub(crate) fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, AuthenticatedChangeSetError> {
        let fields =
            decode_canonical::<Self>(bytes).map_err(AuthenticatedChangeSetError::Decode)?;
        let mut fields = fields.into_iter();
        let changeset = variable_bytes(fields.next(), "changeset")?;
        let actor_public_key =
            PublicKey::from_bytes(fixed_bytes(fields.next(), "actor_public_key")?);
        let signature = Signature::from_bytes(fixed_bytes(fields.next(), "signature")?);
        Self::verified(changeset, actor_public_key, signature)
    }

    pub(crate) fn has_authentication(&self, actor: PublicKey, signature: Signature) -> bool {
        self.actor_public_key == actor && self.signature == signature
    }

    /// Exact inner canonical ChangeSet statement.
    pub(crate) fn changeset(&self) -> &[u8] {
        &self.changeset
    }

    /// Canonical persisted envelope bytes.
    pub(crate) fn canonical_bytes(&self) -> Vec<u8> {
        encode_canonical(self)
    }
}

impl CanonicalEncode for AuthenticatedChangeSet {
    const SCHEMA: RecordSchema = RecordSchema::new(
        DomainTag::new("mesh.v0.authenticated-changeset-envelope"),
        &ENVELOPE_FIELDS,
    );

    fn canonical_fields(&self) -> Vec<CanonicalValue> {
        vec![
            CanonicalValue::Bytes(self.changeset.clone()),
            CanonicalValue::Bytes(self.actor_public_key.as_bytes().to_vec()),
            CanonicalValue::Bytes(self.signature.as_bytes().to_vec()),
        ]
    }
}

fn validate_inner_actor(
    changeset: &[u8],
    actor_public_key: &PublicKey,
) -> Result<(), AuthenticatedChangeSetError> {
    let fields = mesh_operations::decode_canonical(&mesh_operations::CHANGESET_SCHEMA, changeset)
        .map_err(AuthenticatedChangeSetError::InnerDecode)?;
    match fields.get(1) {
        Some(mesh_operations::CanonicalValue::Bytes(actor))
            if actor.as_slice() == actor_public_key.as_bytes() =>
        {
            Ok(())
        }
        _ => Err(AuthenticatedChangeSetError::ActorMismatch),
    }
}

fn variable_bytes(
    value: Option<CanonicalValue>,
    field: &'static str,
) -> Result<Vec<u8>, AuthenticatedChangeSetError> {
    match value {
        Some(CanonicalValue::Bytes(bytes)) => Ok(bytes),
        _ => Err(AuthenticatedChangeSetError::MalformedField(field)),
    }
}

fn fixed_bytes<const N: usize>(
    value: Option<CanonicalValue>,
    field: &'static str,
) -> Result<[u8; N], AuthenticatedChangeSetError> {
    let bytes = variable_bytes(value, field)?;
    bytes
        .try_into()
        .map_err(|_| AuthenticatedChangeSetError::MalformedField(field))
}

/// Why an authenticated payload could not be admitted or read.
#[derive(Debug)]
pub enum AuthenticatedChangeSetError {
    /// The outer envelope did not have its sole canonical representation.
    Decode(DecodeError),
    /// The signed inner ChangeSet statement was not canonical.
    InnerDecode(mesh_operations::DecodeError),
    /// One envelope field had the wrong shape.
    MalformedField(&'static str),
    /// The inner actor identifier differed from the public key that signed it.
    ActorMismatch,
    /// The audited Ed25519 verifier rejected the signature.
    Signature(VerifyError),
}

impl fmt::Display for AuthenticatedChangeSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(error) => write!(
                formatter,
                "authenticated envelope is not canonical: {error}"
            ),
            Self::InnerDecode(error) => {
                write!(formatter, "inner ChangeSet is not canonical: {error}")
            }
            Self::MalformedField(field) => write!(
                formatter,
                "authenticated envelope field {field} is malformed"
            ),
            Self::ActorMismatch => {
                formatter.write_str("ChangeSet actor does not equal its signing public key")
            }
            Self::Signature(error) => {
                write!(formatter, "ChangeSet actor signature was rejected: {error}")
            }
        }
    }
}

impl std::error::Error for AuthenticatedChangeSetError {}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer as _, SigningKey};
    use mesh_operations::{
        encode_canonical as encode_changeset, ActorId, ActorSequence, CausalParents,
        ChangeSetDraft, HeadDerivation, HeadId, Hlc, PolicyEpoch, SessionId,
        Signature as OperationSignature, TransitionCommitment, WorkspaceId,
    };
    use mesh_types::{Blake3, ContentDigest as _};

    use super::*;

    struct Head;

    impl HeadDerivation for Head {
        fn resulting_head(&self, commitment: &TransitionCommitment) -> HeadId {
            HeadId::from_bytes(*Blake3::digest_bytes(&commitment.canonical_bytes()).as_bytes())
        }
    }

    fn body(actor: [u8; 32]) -> Vec<u8> {
        encode_changeset(
            &ChangeSetDraft::new(
                WorkspaceId::from_bytes([1; 16]),
                ActorId::from_bytes(actor),
                SessionId::from_bytes([2; 16]),
                ActorSequence::FIRST,
                Hlc::new(0, 0),
            )
            .causal_parents(CausalParents::genesis())
            .base_head(HeadId::from_bytes([0; 32]))
            .policy_epoch(PolicyEpoch::new(1))
            .seal(Vec::new(), &Head, OperationSignature::from_bytes([0; 64])),
        )
    }

    #[test]
    fn canonical_envelope_roundtrips_only_with_exact_actor_signature() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let public = PublicKey::from_bytes(key.verifying_key().to_bytes());
        let statement = body(*public.as_bytes());
        let signature = Signature::from_bytes(
            key.sign(SigningPayload::new(CHANGESET_SIGNATURE_DOMAIN, &statement).as_bytes())
                .to_bytes(),
        );
        let envelope = AuthenticatedChangeSet::verified(statement.clone(), public, signature)
            .expect("valid signature");
        let canonical = envelope.canonical_bytes();
        assert_eq!(
            AuthenticatedChangeSet::from_canonical_bytes(&canonical)
                .expect("canonical read")
                .changeset(),
            statement
        );

        let mut damaged = canonical.clone();
        *damaged.last_mut().expect("signature byte") ^= 1;
        assert!(AuthenticatedChangeSet::from_canonical_bytes(&damaged).is_err());
        assert!(matches!(
            AuthenticatedChangeSet::verified(body([9; 32]), public, signature),
            Err(AuthenticatedChangeSetError::ActorMismatch)
        ));
        let mut noncanonical = canonical;
        noncanonical.push(0);
        assert!(AuthenticatedChangeSet::from_canonical_bytes(&noncanonical).is_err());
    }
}
