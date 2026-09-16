//! Reconstructing a ChangeSet from the canonical bytes a peer sent, so its identifier can be
//! re-derived from them.
//!
//! Nothing here trusts a field a sender supplied beside the body. The only input is
//! `CarriedChangeSet::body` — "the ChangeSet's `mesh-cbor/0` bytes, exactly as its author signed
//! them" — and the only output is the record those bytes describe.
//!
//! # The one field the bytes cannot supply
//!
//! The canonical encoding binds ten fields and **not** the signature, because the signature is made
//! over the record. So a ChangeSet rebuilt from canonical bytes has no signature to put in it, and
//! [`rebuild`] fills that slot with zeros. The rebuilt value therefore never leaves this module: it
//! exists for exactly as long as it takes [`mesh_types::derive_id`] to digest it, and
//! [`crate::AdmittedChangeSet`] carries the causal facts instead. Signature verification is
//! `mesh-crypto`'s, is a separate check with a separate failure mode, and is not done here.

use mesh_types::{
    decode_canonical, ActorId, ActorSequence, CausalParents, ChangeSet, ChangeSetDraft,
    ChangeSetId, DecodeError, Digest32, HeadId, Hlc, PolicyEpoch, SessionId, Signature, Uuid,
    WorkspaceId,
};

use crate::operation::OperationDecoder;

/// Why some received bytes are not a ChangeSet this peer can re-derive an identifier from.
///
/// Every variant is a refusal, never a warning: a body that cannot be decoded cannot have its
/// identifier checked, and an identifier that has not been checked may not reach head advancement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BodyRefused {
    /// The bytes are not the canonical encoding of a ChangeSet.
    NotCanonical(DecodeError),
    /// A field decoded, and then did not satisfy the type it names — a workspace identifier that
    /// is not a UUIDv7, a logical clock reading wider than the field it lives in.
    FieldOutOfRange {
        /// The field, by its canonical schema name.
        field: &'static str,
        /// Why the value cannot be what the field means.
        why: &'static str,
    },
    /// One of the operations could not be decoded by the supplied [`OperationDecoder`].
    ///
    /// The identifier binds the operations, so an operation that cannot be decoded is an
    /// identifier that cannot be recomputed. It is refused rather than skipped.
    Operation {
        /// Which operation, by position in the sequence.
        index: usize,
        /// What the decoder said.
        why: String,
    },
}

impl core::fmt::Display for BodyRefused {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotCanonical(error) => {
                write!(
                    formatter,
                    "the body is not a canonical ChangeSet: {error:?}"
                )
            }
            Self::FieldOutOfRange { field, why } => {
                write!(formatter, "the field {field} is out of range: {why}")
            }
            Self::Operation { index, why } => {
                write!(formatter, "operation {index} could not be decoded: {why}")
            }
        }
    }
}

impl std::error::Error for BodyRefused {}

/// The ten canonical fields of a ChangeSet, in schema order, as this module indexes them.
mod field {
    pub const WORKSPACE_ID: usize = 0;
    pub const ACTOR_ID: usize = 1;
    pub const SESSION_ID: usize = 2;
    pub const ACTOR_SEQUENCE: usize = 3;
    pub const CAUSAL_PARENTS: usize = 4;
    pub const BASE_HEAD: usize = 5;
    pub const RESULTING_HEAD: usize = 6;
    pub const OPERATIONS: usize = 7;
    pub const POLICY_EPOCH: usize = 8;
    pub const HYBRID_LOGICAL_TIME: usize = 9;
}

/// The ChangeSet `bytes` encode, with its operations decoded by `decoder`.
///
/// # Errors
///
/// [`BodyRefused`] when the bytes are not a canonical ChangeSet, when a field cannot mean what its
/// type says, or when an operation cannot be decoded.
pub(crate) fn rebuild<D: OperationDecoder>(
    bytes: &[u8],
    decoder: &D,
) -> Result<ChangeSet<D::Operation>, BodyRefused> {
    // `ChangeSet<Op>`'s canonical schema does not depend on `Op` — `operations` is a sequence of
    // complete nested encodings whatever the vocabulary is — so the unit instantiation reads the
    // field values for every instantiation.
    let values = decode_canonical::<ChangeSet<()>>(bytes).map_err(BodyRefused::NotCanonical)?;

    let workspace_id = uuid_field(&values, field::WORKSPACE_ID)?;
    let workspace_id =
        WorkspaceId::from_uuid(workspace_id).map_err(|_| BodyRefused::FieldOutOfRange {
            field: "workspace_id",
            why: "an entity identifier is a UUIDv7 in the RFC variant",
        })?;
    let session_id = uuid_field(&values, field::SESSION_ID)?;
    let session_id =
        SessionId::from_uuid(session_id).map_err(|_| BodyRefused::FieldOutOfRange {
            field: "session_id",
            why: "an entity identifier is a UUIDv7 in the RFC variant",
        })?;

    let actor_id = ActorId::from_digest(digest_field(&values, field::ACTOR_ID, "actor_id")?);
    let base_head = HeadId::from_digest(digest_field(&values, field::BASE_HEAD, "base_head")?);
    let resulting_head = HeadId::from_digest(digest_field(
        &values,
        field::RESULTING_HEAD,
        "resulting_head",
    )?);

    let actor_sequence = ActorSequence::new(unsigned_field(
        &values,
        field::ACTOR_SEQUENCE,
        "actor_sequence",
    )?);
    let policy_epoch = PolicyEpoch::new(unsigned_field(
        &values,
        field::POLICY_EPOCH,
        "policy_epoch",
    )?);
    let hybrid_logical_time = hlc_field(&values)?;
    let causal_parents = parents_field(&values)?;
    let operations = operations_field(&values, decoder)?;

    Ok(ChangeSetDraft::new(
        workspace_id,
        actor_id,
        session_id,
        actor_sequence,
        hybrid_logical_time,
    )
    .causal_parents(causal_parents)
    .base_head(base_head)
    .policy_epoch(policy_epoch)
    // The canonical encoding does not carry a signature; see this module's header. The rebuilt
    // record never escapes `crate::receipt`, and `derive_id` does not absorb this field.
    .seal(operations, resulting_head, Signature::from_bytes([0; 64])))
}

/// A shape the decoder already validated against the schema. Reaching one of these is a bug in
/// `decode_canonical` rather than a peer's doing, so it is a refusal with a fixed reason rather
/// than a panic: a peer must never be able to abort this process.
const SHAPE: &str = "the canonical schema promises this field's shape";

fn unsigned_field(
    values: &[mesh_types::CanonicalValue],
    index: usize,
    name: &'static str,
) -> Result<u64, BodyRefused> {
    match values.get(index) {
        Some(mesh_types::CanonicalValue::Unsigned(value)) => Ok(*value),
        _ => Err(BodyRefused::FieldOutOfRange {
            field: name,
            why: SHAPE,
        }),
    }
}

fn bytes_field<'a>(
    values: &'a [mesh_types::CanonicalValue],
    index: usize,
    name: &'static str,
) -> Result<&'a [u8], BodyRefused> {
    match values.get(index) {
        Some(mesh_types::CanonicalValue::Bytes(value)) => Ok(value),
        _ => Err(BodyRefused::FieldOutOfRange {
            field: name,
            why: SHAPE,
        }),
    }
}

fn digest_field(
    values: &[mesh_types::CanonicalValue],
    index: usize,
    name: &'static str,
) -> Result<Digest32, BodyRefused> {
    let bytes = bytes_field(values, index, name)?;
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| BodyRefused::FieldOutOfRange {
        field: name,
        why: "an identifier is exactly thirty-two bytes",
    })?;
    Ok(Digest32::from_bytes(bytes))
}

fn uuid_field(values: &[mesh_types::CanonicalValue], index: usize) -> Result<Uuid, BodyRefused> {
    let name = if index == field::WORKSPACE_ID {
        "workspace_id"
    } else {
        "session_id"
    };
    let bytes = bytes_field(values, index, name)?;
    let bytes: [u8; 16] = bytes.try_into().map_err(|_| BodyRefused::FieldOutOfRange {
        field: name,
        why: "an entity identifier is exactly sixteen bytes",
    })?;
    Ok(Uuid::from_bytes(bytes))
}

fn hlc_field(values: &[mesh_types::CanonicalValue]) -> Result<Hlc, BodyRefused> {
    let Some(mesh_types::CanonicalValue::Group(parts)) = values.get(field::HYBRID_LOGICAL_TIME)
    else {
        return Err(BodyRefused::FieldOutOfRange {
            field: "hybrid_logical_time",
            why: SHAPE,
        });
    };
    let physical = unsigned_field(parts, 0, "hybrid_logical_time.physical_millis")?;
    let logical = unsigned_field(parts, 1, "hybrid_logical_time.logical")?;
    let logical = u32::try_from(logical).map_err(|_| BodyRefused::FieldOutOfRange {
        field: "hybrid_logical_time.logical",
        why: "the logical half of a clock reading is thirty-two bits wide",
    })?;
    Ok(Hlc::new(physical, logical))
}

fn parents_field(values: &[mesh_types::CanonicalValue]) -> Result<CausalParents, BodyRefused> {
    let Some(mesh_types::CanonicalValue::Sequence(items)) = values.get(field::CAUSAL_PARENTS)
    else {
        return Err(BodyRefused::FieldOutOfRange {
            field: "causal_parents",
            why: SHAPE,
        });
    };
    let mut parents = Vec::with_capacity(items.len());
    for (index, _) in items.iter().enumerate() {
        parents.push(ChangeSetId::from_digest(digest_field(
            items,
            index,
            "causal_parents",
        )?));
    }
    let mut parents = parents.into_iter();
    Ok(match parents.next() {
        None => CausalParents::genesis(),
        Some(first) => CausalParents::after(first, parents.collect()),
    })
}

fn operations_field<D: OperationDecoder>(
    values: &[mesh_types::CanonicalValue],
    decoder: &D,
) -> Result<Vec<D::Operation>, BodyRefused> {
    let Some(mesh_types::CanonicalValue::Sequence(items)) = values.get(field::OPERATIONS) else {
        return Err(BodyRefused::FieldOutOfRange {
            field: "operations",
            why: SHAPE,
        });
    };
    let mut operations = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let mesh_types::CanonicalValue::Record(encoding) = item else {
            return Err(BodyRefused::FieldOutOfRange {
                field: "operations",
                why: SHAPE,
            });
        };
        operations.push(
            decoder
                .decode(encoding)
                .map_err(|why| BodyRefused::Operation { index, why })?,
        );
    }
    Ok(operations)
}
