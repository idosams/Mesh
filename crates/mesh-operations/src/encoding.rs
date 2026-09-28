//! The published schema of every operation, and the two directions across it.
//!
//! # The vocabulary is dispatched by its domain tag
//!
//! Every operation encodes as `[ domain_tag, field, field, … ]`. The tag is the first array
//! element, so [`decode_operation`] reads it, resolves the member it names, and decodes the
//! remaining elements against *that* member's schema. An external implementer needs
//! [`operation_schemas`] and the `mesh-cbor/0` rules and nothing else — no Rust, no lookahead,
//! no ambiguity about which shape follows.
//!
//! An unknown tag is [`DecodeError::UnknownDomain`] rather than a skip. A peer running a later
//! vocabulary is a compatibility event that a receiver must be able to see; silently ignoring an
//! operation it does not understand would let two actors diverge while both believe they applied
//! the same ChangeSet.
//!
//! # Enumerated values are bounds-checked on the way in
//!
//! `region`, `confidence`, `node_kind` and `outcome` ride as unsigned indices into the `ALL`
//! arrays in `src/operation.rs`. A value past the end is [`DecodeError::UnadmittedValue`], not a
//! saturating clamp: clamping an unknown confidence to `Unknown` would present a peer's newer,
//! stronger claim as the weakest one this build knows, which is exactly the overstatement plan
//! §4.8 enumerates the axis to prevent — in the other direction.

use crate::canonical::{
    decode_canonical, encode_canonical, peek_domain, CanonicalEncode, CanonicalType,
    CanonicalValue, DecodeError, FieldSchema, RecordSchema,
};
use crate::ids::{
    ActorId, ApprovalId, ContentHash, DerivationId, HeadId, ManifestId, ObjectId, ReviewBundleId,
    VersionId,
};
use crate::name::{NormalizedName, PortableMetadata};
use crate::operation::{
    AttributionConfidence, DerivationKind, Operation, OperationKind, PreservedEntry, ReadRegion,
    ValidationOutcome,
};

/// The prefix every operation's domain tag carries.
pub const OPERATION_DOMAIN_PREFIX: &str = "mesh.v0.op.";

const OBJECT: CanonicalType = CanonicalType::Bytes(Some(16));
const DIGEST: CanonicalType = CanonicalType::Bytes(Some(32));
const DIGEST_SEQUENCE: CanonicalType = CanonicalType::Sequence(&CanonicalType::Bytes(Some(32)));

const PORTABLE_METADATA_FIELDS: &[FieldSchema] =
    &[FieldSchema::new("executable", CanonicalType::Bool)];
const PORTABLE_METADATA: CanonicalType = CanonicalType::Group(PORTABLE_METADATA_FIELDS);

const PRESERVED_ENTRY_FIELDS: &[FieldSchema] = &[
    FieldSchema::new("object_id", OBJECT),
    FieldSchema::new("name", CanonicalType::Text),
];
const PRESERVED_ENTRY_GROUP: CanonicalType = CanonicalType::Group(PRESERVED_ENTRY_FIELDS);
const PRESERVED_ENTRIES: CanonicalType = CanonicalType::Sequence(&PRESERVED_ENTRY_GROUP);

macro_rules! schema {
    ($konst:ident, $domain:literal, [$($field:literal : $ty:expr),* $(,)?]) => {
        const $konst: RecordSchema = RecordSchema::new(
            $domain,
            &[$(FieldSchema::new($field, $ty)),*],
        );
    };
}

schema!(CREATE_FILE, "mesh.v0.op.create-file", ["object_id": OBJECT]);
schema!(CREATE_DIRECTORY, "mesh.v0.op.create-directory", ["object_id": OBJECT]);
schema!(WRITE_FILE_VERSION, "mesh.v0.op.write-file-version", [
    "object_id": OBJECT,
    "version_id": DIGEST,
    "parent_versions": DIGEST_SEQUENCE,
    "manifest_id": DIGEST,
    "portable_metadata": PORTABLE_METADATA,
]);
schema!(LINK_DIRECTORY_ENTRY, "mesh.v0.op.link-directory-entry", [
    "directory_id": OBJECT,
    "name": CanonicalType::Text,
    "object_id": OBJECT,
    "version_id": DIGEST,
]);
schema!(UNLINK_DIRECTORY_ENTRY, "mesh.v0.op.unlink-directory-entry", [
    "directory_id": OBJECT,
    "name": CanonicalType::Text,
    "object_id": OBJECT,
]);
schema!(RENAME_ENTRY, "mesh.v0.op.rename-entry", [
    "directory_id": OBJECT,
    "from_name": CanonicalType::Text,
    "to_name": CanonicalType::Text,
    "object_id": OBJECT,
]);
schema!(MOVE_ENTRY, "mesh.v0.op.move-entry", [
    "from_directory_id": OBJECT,
    "from_name": CanonicalType::Text,
    "to_directory_id": OBJECT,
    "to_name": CanonicalType::Text,
    "object_id": OBJECT,
]);
schema!(DELETE_OBJECT, "mesh.v0.op.delete-object", ["object_id": OBJECT]);
schema!(RESTORE_OBJECT, "mesh.v0.op.restore-object", [
    "object_id": OBJECT,
    "restored_version_id": DIGEST,
]);
schema!(SET_PORTABLE_METADATA, "mesh.v0.op.set-portable-metadata", [
    "object_id": OBJECT,
    "version_id": DIGEST,
    "portable_metadata": PORTABLE_METADATA,
]);
schema!(RESOLVE_NAME_CONFLICT, "mesh.v0.op.resolve-name-conflict", [
    "directory_id": OBJECT,
    "contested_name": CanonicalType::Text,
    "preserved": PRESERVED_ENTRIES,
]);
schema!(RESOLVE_CONTENT_CONFLICT, "mesh.v0.op.resolve-content-conflict", [
    "object_id": OBJECT,
    "resulting_version_id": DIGEST,
    "preserved_version_ids": DIGEST_SEQUENCE,
]);
schema!(ADVANCE_ACTOR_HEAD, "mesh.v0.op.advance-actor-head", [
    "actor_id": DIGEST,
    "from_head": DIGEST,
    "to_head": DIGEST,
]);
schema!(RECORD_READ_OBSERVATION, "mesh.v0.op.record-read-observation", [
    "actor_id": DIGEST,
    "object_id": OBJECT,
    "version_id": DIGEST,
    "region": CanonicalType::Unsigned,
    "confidence": CanonicalType::Unsigned,
]);
schema!(RECORD_DERIVED_NODE, "mesh.v0.op.record-derived-node", [
    "node_id": DIGEST,
    "node_kind": CanonicalType::Unsigned,
    "exact_inputs": DIGEST_SEQUENCE,
    "configuration_digest": DIGEST,
    "output_versions": DIGEST_SEQUENCE,
    "deterministic": CanonicalType::Bool,
]);
schema!(CREATE_REVIEW_BUNDLE, "mesh.v0.op.create-review-bundle", [
    "bundle_id": DIGEST,
    "actor_head": DIGEST,
    "base_head": DIGEST,
]);
schema!(RECORD_VALIDATION, "mesh.v0.op.record-validation", [
    "subject_head": DIGEST,
    "validator_id": DIGEST,
    "outcome": CanonicalType::Unsigned,
    "evidence": DIGEST,
]);
schema!(ADVANCE_CANONICAL_HEAD, "mesh.v0.op.advance-canonical-head", [
    "from_head": DIGEST,
    "to_head": DIGEST,
    "approval_id": DIGEST,
]);

schema!(INITIALIZE_WORKSPACE, "mesh.v0.op.initialize-workspace", [
    "root_id": OBJECT,
]);

impl OperationKind {
    /// This member's published schema.
    #[must_use]
    pub const fn schema(&self) -> &'static RecordSchema {
        match self {
            Self::InitializeWorkspace => &INITIALIZE_WORKSPACE,
            Self::CreateFile => &CREATE_FILE,
            Self::CreateDirectory => &CREATE_DIRECTORY,
            Self::WriteFileVersion => &WRITE_FILE_VERSION,
            Self::LinkDirectoryEntry => &LINK_DIRECTORY_ENTRY,
            Self::UnlinkDirectoryEntry => &UNLINK_DIRECTORY_ENTRY,
            Self::RenameEntry => &RENAME_ENTRY,
            Self::MoveEntry => &MOVE_ENTRY,
            Self::DeleteObject => &DELETE_OBJECT,
            Self::RestoreObject => &RESTORE_OBJECT,
            Self::SetPortableMetadata => &SET_PORTABLE_METADATA,
            Self::ResolveNameConflict => &RESOLVE_NAME_CONFLICT,
            Self::ResolveContentConflict => &RESOLVE_CONTENT_CONFLICT,
            Self::AdvanceActorHead => &ADVANCE_ACTOR_HEAD,
            Self::RecordReadObservation => &RECORD_READ_OBSERVATION,
            Self::RecordDerivedNode => &RECORD_DERIVED_NODE,
            Self::CreateReviewBundle => &CREATE_REVIEW_BUNDLE,
            Self::RecordValidation => &RECORD_VALIDATION,
            Self::AdvanceCanonicalHead => &ADVANCE_CANONICAL_HEAD,
        }
    }

    /// This member's domain tag.
    #[must_use]
    pub const fn domain(&self) -> &'static str {
        self.schema().domain
    }

    /// The member a domain tag names, if any.
    #[must_use]
    pub fn from_domain(domain: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.domain() == domain)
    }
}

/// Every operation schema, in plan §4.3 order.
///
/// This is the list to publish beside `protocol/test-vectors/`. It is a function rather than a
/// constant so that the nineteen schemas have exactly one home — [`OperationKind::schema`] — and a
/// nineteenth cannot be added to a published list without being added to the vocabulary.
#[must_use]
pub fn operation_schemas() -> Vec<&'static RecordSchema> {
    OperationKind::ALL
        .into_iter()
        .map(|kind| kind.schema())
        .collect()
}

fn digest(bytes: &[u8; 32]) -> CanonicalValue {
    CanonicalValue::from_array(*bytes)
}

fn object(id: ObjectId) -> CanonicalValue {
    CanonicalValue::from_array(*id.as_bytes())
}

fn text(name: &NormalizedName) -> CanonicalValue {
    CanonicalValue::Text(name.as_str().to_owned())
}

fn metadata(value: PortableMetadata) -> CanonicalValue {
    CanonicalValue::Group(vec![CanonicalValue::Bool(value.is_executable())])
}

fn digest_sequence<'a, I: IntoIterator<Item = &'a [u8; 32]>>(items: I) -> CanonicalValue {
    CanonicalValue::Sequence(items.into_iter().map(digest).collect())
}

fn index_of<T: PartialEq + Copy>(all: &[T], value: T) -> u64 {
    all.iter()
        .position(|candidate| *candidate == value)
        .expect("every enumerated value is a member of its own ALL array") as u64
}

impl CanonicalEncode for Operation {
    fn schema(&self) -> &'static RecordSchema {
        self.kind().schema()
    }

    fn canonical_fields(&self) -> Vec<CanonicalValue> {
        match self {
            Self::InitializeWorkspace { root_id } => vec![object(*root_id)],
            Self::CreateFile { object_id } | Self::CreateDirectory { object_id } => {
                vec![object(*object_id)]
            }
            Self::WriteFileVersion {
                object_id,
                version_id,
                parent_versions,
                manifest_id,
                portable_metadata,
            } => vec![
                object(*object_id),
                digest(version_id.as_bytes()),
                digest_sequence(parent_versions.iter().map(VersionId::as_bytes)),
                digest(manifest_id.as_bytes()),
                metadata(*portable_metadata),
            ],
            Self::LinkDirectoryEntry {
                directory_id,
                name,
                object_id,
                version_id,
            } => vec![
                object(*directory_id),
                text(name),
                object(*object_id),
                digest(version_id.as_bytes()),
            ],
            Self::UnlinkDirectoryEntry {
                directory_id,
                name,
                object_id,
            } => vec![object(*directory_id), text(name), object(*object_id)],
            Self::RenameEntry {
                directory_id,
                from_name,
                to_name,
                object_id,
            } => vec![
                object(*directory_id),
                text(from_name),
                text(to_name),
                object(*object_id),
            ],
            Self::MoveEntry {
                from_directory_id,
                from_name,
                to_directory_id,
                to_name,
                object_id,
            } => vec![
                object(*from_directory_id),
                text(from_name),
                object(*to_directory_id),
                text(to_name),
                object(*object_id),
            ],
            Self::DeleteObject { object_id } => vec![object(*object_id)],
            Self::RestoreObject {
                object_id,
                restored_version_id,
            } => vec![object(*object_id), digest(restored_version_id.as_bytes())],
            Self::SetPortableMetadata {
                object_id,
                version_id,
                portable_metadata,
            } => vec![
                object(*object_id),
                digest(version_id.as_bytes()),
                metadata(*portable_metadata),
            ],
            Self::ResolveNameConflict {
                directory_id,
                contested_name,
                preserved,
            } => vec![
                object(*directory_id),
                text(contested_name),
                CanonicalValue::Sequence(
                    preserved
                        .iter()
                        .map(|entry| {
                            CanonicalValue::Group(vec![
                                object(entry.object_id()),
                                text(entry.name()),
                            ])
                        })
                        .collect(),
                ),
            ],
            Self::ResolveContentConflict {
                object_id,
                resulting_version_id,
                preserved_version_ids,
            } => vec![
                object(*object_id),
                digest(resulting_version_id.as_bytes()),
                digest_sequence(preserved_version_ids.iter().map(VersionId::as_bytes)),
            ],
            Self::AdvanceActorHead {
                actor_id,
                from_head,
                to_head,
            } => vec![
                digest(actor_id.as_bytes()),
                digest(from_head.as_bytes()),
                digest(to_head.as_bytes()),
            ],
            Self::RecordReadObservation {
                actor_id,
                object_id,
                version_id,
                region,
                confidence,
            } => vec![
                digest(actor_id.as_bytes()),
                object(*object_id),
                digest(version_id.as_bytes()),
                CanonicalValue::Unsigned(index_of(&ReadRegion::ALL, *region)),
                CanonicalValue::Unsigned(index_of(&AttributionConfidence::ALL, *confidence)),
            ],
            Self::RecordDerivedNode {
                node_id,
                node_kind,
                exact_inputs,
                configuration_digest,
                output_versions,
                deterministic,
            } => vec![
                digest(node_id.as_bytes()),
                CanonicalValue::Unsigned(index_of(&DerivationKind::ALL, *node_kind)),
                digest_sequence(exact_inputs.iter().map(VersionId::as_bytes)),
                digest(configuration_digest.as_bytes()),
                digest_sequence(output_versions.iter().map(VersionId::as_bytes)),
                CanonicalValue::Bool(*deterministic),
            ],
            Self::CreateReviewBundle {
                bundle_id,
                actor_head,
                base_head,
            } => vec![
                digest(bundle_id.as_bytes()),
                digest(actor_head.as_bytes()),
                digest(base_head.as_bytes()),
            ],
            Self::RecordValidation {
                subject_head,
                validator_id,
                outcome,
                evidence,
            } => vec![
                digest(subject_head.as_bytes()),
                digest(validator_id.as_bytes()),
                CanonicalValue::Unsigned(index_of(&ValidationOutcome::ALL, *outcome)),
                digest(evidence.as_bytes()),
            ],
            Self::AdvanceCanonicalHead {
                from_head,
                to_head,
                approval_id,
            } => vec![
                digest(from_head.as_bytes()),
                digest(to_head.as_bytes()),
                digest(approval_id.as_bytes()),
            ],
        }
    }
}

/// A reader over one decoded operation's field values.
struct Fields<'a> {
    domain: &'static str,
    values: &'a [CanonicalValue],
}

impl Fields<'_> {
    fn unadmitted(&self, field: &str, reason: String) -> DecodeError {
        DecodeError::UnadmittedValue {
            path: format!("{}.{field}", self.domain),
            reason,
        }
    }

    fn bytes<const N: usize>(&self, index: usize, field: &str) -> Result<[u8; N], DecodeError> {
        match &self.values[index] {
            CanonicalValue::Bytes(raw) if raw.len() == N => {
                let mut out = [0u8; N];
                out.copy_from_slice(raw);
                Ok(out)
            }
            _ => Err(self.unadmitted(field, format!("expected {N} bytes"))),
        }
    }

    fn name(&self, index: usize, field: &str) -> Result<NormalizedName, DecodeError> {
        match &self.values[index] {
            CanonicalValue::Text(raw) => NormalizedName::new(raw.clone())
                .map_err(|error| self.unadmitted(field, error.to_string())),
            _ => Err(self.unadmitted(field, "expected text".to_owned())),
        }
    }

    fn boolean(&self, index: usize, field: &str) -> Result<bool, DecodeError> {
        match &self.values[index] {
            CanonicalValue::Bool(value) => Ok(*value),
            _ => Err(self.unadmitted(field, "expected a boolean".to_owned())),
        }
    }

    fn metadata(&self, index: usize, field: &str) -> Result<PortableMetadata, DecodeError> {
        match &self.values[index] {
            CanonicalValue::Group(items) if items.len() == 1 => match &items[0] {
                CanonicalValue::Bool(value) => Ok(PortableMetadata::new(*value)),
                _ => Err(self.unadmitted(field, "expected a boolean".to_owned())),
            },
            _ => Err(self.unadmitted(field, "expected a one-field group".to_owned())),
        }
    }

    fn enumerated<T: Copy>(&self, index: usize, field: &str, all: &[T]) -> Result<T, DecodeError> {
        let CanonicalValue::Unsigned(value) = &self.values[index] else {
            return Err(self.unadmitted(field, "expected an unsigned index".to_owned()));
        };
        usize::try_from(*value)
            .ok()
            .and_then(|at| all.get(at))
            .copied()
            .ok_or_else(|| {
                self.unadmitted(
                    field,
                    format!(
                        "{value} names no member of an enumeration with {} of them",
                        all.len()
                    ),
                )
            })
    }

    fn digests<const N: usize, T, F: Fn([u8; N]) -> T>(
        &self,
        index: usize,
        field: &str,
        build: F,
    ) -> Result<Vec<T>, DecodeError> {
        let CanonicalValue::Sequence(items) = &self.values[index] else {
            return Err(self.unadmitted(field, "expected a sequence".to_owned()));
        };
        items
            .iter()
            .map(|item| match item {
                CanonicalValue::Bytes(raw) if raw.len() == N => {
                    let mut out = [0u8; N];
                    out.copy_from_slice(raw);
                    Ok(build(out))
                }
                _ => Err(self.unadmitted(field, format!("expected {N} bytes per element"))),
            })
            .collect()
    }

    fn preserved(&self, index: usize, field: &str) -> Result<Vec<PreservedEntry>, DecodeError> {
        let CanonicalValue::Sequence(items) = &self.values[index] else {
            return Err(self.unadmitted(field, "expected a sequence".to_owned()));
        };
        items
            .iter()
            .map(|item| {
                let CanonicalValue::Group(group) = item else {
                    return Err(self.unadmitted(field, "expected a group per element".to_owned()));
                };
                let inner = Fields {
                    domain: self.domain,
                    values: group,
                };
                Ok(PreservedEntry::new(
                    ObjectId::from_bytes(inner.bytes::<16>(0, field)?),
                    inner.name(1, field)?,
                ))
            })
            .collect()
    }
}

/// Decode one operation from its canonical encoding.
///
/// The domain tag chooses the member; the rest is decoded against that member's published schema.
///
/// # Errors
///
/// [`DecodeError`] when the bytes are not a well-formed `mesh-cbor/0` record, when the tag names
/// no member, when the arity or a fixed width disagrees with the schema, or when a field carries a
/// value the vocabulary does not admit.
///
/// ```
/// use mesh_operations::{decode_operation, encode_canonical, ObjectId, Operation};
///
/// let operation = Operation::CreateFile { object_id: ObjectId::from_bytes([7; 16]) };
/// let bytes = encode_canonical(&operation);
/// assert_eq!(decode_operation(&bytes).unwrap(), operation);
/// ```
pub fn decode_operation(bytes: &[u8]) -> Result<Operation, DecodeError> {
    let domain = peek_domain(bytes)?;
    let kind = OperationKind::from_domain(domain).ok_or_else(|| DecodeError::UnknownDomain {
        found: domain.to_owned(),
    })?;
    let schema = kind.schema();
    let values = decode_canonical(schema, bytes)?;
    let fields = Fields {
        domain: schema.domain,
        values: &values,
    };

    Ok(match kind {
        OperationKind::InitializeWorkspace => Operation::InitializeWorkspace {
            root_id: ObjectId::from_bytes(fields.bytes(0, "root_id")?),
        },
        OperationKind::CreateFile => Operation::CreateFile {
            object_id: ObjectId::from_bytes(fields.bytes(0, "object_id")?),
        },
        OperationKind::CreateDirectory => Operation::CreateDirectory {
            object_id: ObjectId::from_bytes(fields.bytes(0, "object_id")?),
        },
        OperationKind::WriteFileVersion => Operation::WriteFileVersion {
            object_id: ObjectId::from_bytes(fields.bytes(0, "object_id")?),
            version_id: VersionId::from_bytes(fields.bytes(1, "version_id")?),
            parent_versions: fields.digests(2, "parent_versions", VersionId::from_bytes)?,
            manifest_id: ManifestId::from_bytes(fields.bytes(3, "manifest_id")?),
            portable_metadata: fields.metadata(4, "portable_metadata")?,
        },
        OperationKind::LinkDirectoryEntry => Operation::LinkDirectoryEntry {
            directory_id: ObjectId::from_bytes(fields.bytes(0, "directory_id")?),
            name: fields.name(1, "name")?,
            object_id: ObjectId::from_bytes(fields.bytes(2, "object_id")?),
            version_id: VersionId::from_bytes(fields.bytes(3, "version_id")?),
        },
        OperationKind::UnlinkDirectoryEntry => Operation::UnlinkDirectoryEntry {
            directory_id: ObjectId::from_bytes(fields.bytes(0, "directory_id")?),
            name: fields.name(1, "name")?,
            object_id: ObjectId::from_bytes(fields.bytes(2, "object_id")?),
        },
        OperationKind::RenameEntry => Operation::RenameEntry {
            directory_id: ObjectId::from_bytes(fields.bytes(0, "directory_id")?),
            from_name: fields.name(1, "from_name")?,
            to_name: fields.name(2, "to_name")?,
            object_id: ObjectId::from_bytes(fields.bytes(3, "object_id")?),
        },
        OperationKind::MoveEntry => Operation::MoveEntry {
            from_directory_id: ObjectId::from_bytes(fields.bytes(0, "from_directory_id")?),
            from_name: fields.name(1, "from_name")?,
            to_directory_id: ObjectId::from_bytes(fields.bytes(2, "to_directory_id")?),
            to_name: fields.name(3, "to_name")?,
            object_id: ObjectId::from_bytes(fields.bytes(4, "object_id")?),
        },
        OperationKind::DeleteObject => Operation::DeleteObject {
            object_id: ObjectId::from_bytes(fields.bytes(0, "object_id")?),
        },
        OperationKind::RestoreObject => Operation::RestoreObject {
            object_id: ObjectId::from_bytes(fields.bytes(0, "object_id")?),
            restored_version_id: VersionId::from_bytes(fields.bytes(1, "restored_version_id")?),
        },
        OperationKind::SetPortableMetadata => Operation::SetPortableMetadata {
            object_id: ObjectId::from_bytes(fields.bytes(0, "object_id")?),
            version_id: VersionId::from_bytes(fields.bytes(1, "version_id")?),
            portable_metadata: fields.metadata(2, "portable_metadata")?,
        },
        OperationKind::ResolveNameConflict => Operation::ResolveNameConflict {
            directory_id: ObjectId::from_bytes(fields.bytes(0, "directory_id")?),
            contested_name: fields.name(1, "contested_name")?,
            preserved: fields.preserved(2, "preserved")?,
        },
        OperationKind::ResolveContentConflict => Operation::ResolveContentConflict {
            object_id: ObjectId::from_bytes(fields.bytes(0, "object_id")?),
            resulting_version_id: VersionId::from_bytes(fields.bytes(1, "resulting_version_id")?),
            preserved_version_ids: fields.digests(
                2,
                "preserved_version_ids",
                VersionId::from_bytes,
            )?,
        },
        OperationKind::AdvanceActorHead => Operation::AdvanceActorHead {
            actor_id: ActorId::from_bytes(fields.bytes(0, "actor_id")?),
            from_head: HeadId::from_bytes(fields.bytes(1, "from_head")?),
            to_head: HeadId::from_bytes(fields.bytes(2, "to_head")?),
        },
        OperationKind::RecordReadObservation => Operation::RecordReadObservation {
            actor_id: ActorId::from_bytes(fields.bytes(0, "actor_id")?),
            object_id: ObjectId::from_bytes(fields.bytes(1, "object_id")?),
            version_id: VersionId::from_bytes(fields.bytes(2, "version_id")?),
            region: fields.enumerated(3, "region", &ReadRegion::ALL)?,
            confidence: fields.enumerated(4, "confidence", &AttributionConfidence::ALL)?,
        },
        OperationKind::RecordDerivedNode => Operation::RecordDerivedNode {
            node_id: DerivationId::from_bytes(fields.bytes(0, "node_id")?),
            node_kind: fields.enumerated(1, "node_kind", &DerivationKind::ALL)?,
            exact_inputs: fields.digests(2, "exact_inputs", VersionId::from_bytes)?,
            configuration_digest: ContentHash::from_bytes(fields.bytes(3, "configuration_digest")?),
            output_versions: fields.digests(4, "output_versions", VersionId::from_bytes)?,
            deterministic: fields.boolean(5, "deterministic")?,
        },
        OperationKind::CreateReviewBundle => Operation::CreateReviewBundle {
            bundle_id: ReviewBundleId::from_bytes(fields.bytes(0, "bundle_id")?),
            actor_head: HeadId::from_bytes(fields.bytes(1, "actor_head")?),
            base_head: HeadId::from_bytes(fields.bytes(2, "base_head")?),
        },
        OperationKind::RecordValidation => Operation::RecordValidation {
            subject_head: HeadId::from_bytes(fields.bytes(0, "subject_head")?),
            validator_id: ActorId::from_bytes(fields.bytes(1, "validator_id")?),
            outcome: fields.enumerated(2, "outcome", &ValidationOutcome::ALL)?,
            evidence: ContentHash::from_bytes(fields.bytes(3, "evidence")?),
        },
        OperationKind::AdvanceCanonicalHead => Operation::AdvanceCanonicalHead {
            from_head: HeadId::from_bytes(fields.bytes(0, "from_head")?),
            to_head: HeadId::from_bytes(fields.bytes(1, "to_head")?),
            approval_id: ApprovalId::from_bytes(fields.bytes(2, "approval_id")?),
        },
    })
}

/// The canonical encoding of every operation in `operations`, in order.
#[must_use]
pub fn encode_operations(operations: &[Operation]) -> Vec<Vec<u8>> {
    operations.iter().map(encode_canonical).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::schema_violations;
    use crate::cbor::CborWriter;

    #[test]
    fn workspace_root_declaration_has_exact_wire_shape() {
        let root = ObjectId::from_bytes([0x17; 16]);
        let operation = Operation::InitializeWorkspace { root_id: root };
        let mut writer = CborWriter::new();
        writer.array(2);
        writer.text("mesh.v0.op.initialize-workspace");
        writer.bytes(&[0x17; 16]);
        let expected = writer.finish();
        assert_eq!(crate::encode_canonical(&operation), expected);
        assert_eq!(decode_operation(&expected).unwrap(), operation);
        for (fields, length) in [(0, 16), (2, 16), (1, 15), (1, 17)] {
            let mut writer = CborWriter::new();
            writer.array(fields + 1);
            writer.text("mesh.v0.op.initialize-workspace");
            for _ in 0..fields {
                writer.bytes(&vec![0x17; length]);
            }
            assert!(decode_operation(&writer.finish()).is_err());
        }
    }

    #[test]
    fn every_domain_tag_is_unique_and_carries_the_prefix() {
        let mut domains: Vec<&str> = operation_schemas()
            .into_iter()
            .map(|schema| schema.domain)
            .collect();
        assert_eq!(domains.len(), 19);
        for domain in &domains {
            assert!(domain.starts_with(OPERATION_DOMAIN_PREFIX), "{domain}");
        }
        domains.sort_unstable();
        domains.dedup();
        assert_eq!(domains.len(), 19);
    }

    #[test]
    fn a_domain_tag_resolves_to_its_own_member() {
        for kind in OperationKind::ALL {
            assert_eq!(OperationKind::from_domain(kind.domain()), Some(kind));
        }
        assert_eq!(OperationKind::from_domain("mesh.v0.op.teleport"), None);
    }

    #[test]
    fn every_operation_agrees_with_its_published_schema() {
        for operation in crate::corpus::one_of_every_operation() {
            let problems = schema_violations(operation.schema(), &operation.canonical_fields());
            assert!(problems.is_empty(), "{:?}: {problems:?}", operation.kind());
        }
    }

    #[test]
    fn an_unknown_domain_tag_is_reported_rather_than_skipped() {
        let mut writer = CborWriter::new();
        writer.array(2);
        writer.text("mesh.v0.op.teleport");
        writer.bytes(&[0u8; 16]);
        assert!(matches!(
            decode_operation(&writer.finish()),
            Err(DecodeError::UnknownDomain { .. })
        ));
    }

    #[test]
    fn an_enumerated_value_past_the_end_is_refused_rather_than_clamped() {
        let mut writer = CborWriter::new();
        writer.array(6);
        writer.text(RECORD_READ_OBSERVATION.domain);
        writer.bytes(&[1u8; 32]);
        writer.bytes(&[2u8; 16]);
        writer.bytes(&[3u8; 32]);
        writer.unsigned(7); // seven regions, so index 7 is one past the end
        writer.unsigned(0);
        assert!(matches!(
            decode_operation(&writer.finish()),
            Err(DecodeError::UnadmittedValue { .. })
        ));
    }

    #[test]
    fn a_name_the_vocabulary_refuses_does_not_decode() {
        let mut writer = CborWriter::new();
        writer.array(4);
        writer.text(UNLINK_DIRECTORY_ENTRY.domain);
        writer.bytes(&[1u8; 16]);
        writer.text("../escape");
        writer.bytes(&[2u8; 16]);
        assert!(matches!(
            decode_operation(&writer.finish()),
            Err(DecodeError::UnadmittedValue { .. })
        ));
    }

    #[test]
    fn encoding_a_sequence_of_operations_preserves_order() {
        let operations = crate::corpus::one_of_every_operation();
        let encodings = encode_operations(&operations);
        assert_eq!(encodings.len(), operations.len());
        for (bytes, operation) in encodings.iter().zip(&operations) {
            assert_eq!(&decode_operation(bytes).unwrap(), operation);
        }
    }
}
