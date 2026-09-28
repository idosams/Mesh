//! A deterministic sample of every operation, published so that conformance is checkable.
//!
//! # Why this is public API rather than a test fixture
//!
//! "Every operation round-trips" is only as good as the corpus it is checked over, and a corpus
//! that lives in `#[cfg(test)]` is invisible to the crate's own integration tests, to
//! `mesh-materializer` downstream, and to an external implementer entirely. Publishing it means
//! the same nineteen values are the subject of the round-trip test, the schema-agreement test, the
//! materializer's apply test and anyone else's — so a member that is representable but wrong is
//! wrong in one place rather than in four private copies.
//!
//! The values are fixed byte patterns, not random ones. This crate reads no clock and draws no
//! entropy, and a corpus that did would make every downstream test irreproducible.

use crate::ids::{
    ActorId, ApprovalId, ContentHash, DerivationId, HeadId, ManifestId, ObjectId, ReviewBundleId,
    VersionId,
};
use crate::name::{NormalizedName, PortableMetadata};
use crate::operation::{
    AttributionConfidence, DerivationKind, Operation, OperationKind, PreservedEntry, ReadRegion,
    ValidationOutcome,
};

fn name(text: &str) -> NormalizedName {
    NormalizedName::new(text).expect("the corpus uses structurally valid names")
}

/// One value of every member of the vocabulary, in plan §4.3 order.
///
/// The i-th element's [`Operation::kind`] is `OperationKind::ALL[i]`, which
/// `tests/vocabulary.rs` asserts rather than assumes.
///
/// ```
/// use mesh_operations::{one_of_every_operation, OperationKind};
///
/// let corpus = one_of_every_operation();
/// assert_eq!(corpus.len(), 19);
/// for (operation, kind) in corpus.iter().zip(OperationKind::ALL) {
///     assert_eq!(operation.kind(), kind);
/// }
/// ```
#[must_use]
pub fn one_of_every_operation() -> Vec<Operation> {
    vec![
        Operation::CreateFile {
            object_id: ObjectId::from_bytes([0x11; 16]),
        },
        Operation::CreateDirectory {
            object_id: ObjectId::from_bytes([0x12; 16]),
        },
        Operation::WriteFileVersion {
            object_id: ObjectId::from_bytes([0x11; 16]),
            version_id: VersionId::from_bytes([0x21; 32]),
            parent_versions: vec![
                VersionId::from_bytes([0x20; 32]),
                VersionId::from_bytes([0x1f; 32]),
            ],
            manifest_id: ManifestId::from_bytes([0x31; 32]),
            portable_metadata: PortableMetadata::new(true),
        },
        Operation::LinkDirectoryEntry {
            directory_id: ObjectId::from_bytes([0x12; 16]),
            name: name("report.md"),
            object_id: ObjectId::from_bytes([0x11; 16]),
            version_id: VersionId::from_bytes([0x21; 32]),
        },
        Operation::UnlinkDirectoryEntry {
            directory_id: ObjectId::from_bytes([0x12; 16]),
            name: name("report.md"),
            object_id: ObjectId::from_bytes([0x11; 16]),
        },
        Operation::RenameEntry {
            directory_id: ObjectId::from_bytes([0x12; 16]),
            from_name: name("report.md"),
            to_name: name("\u{6c34}.md"),
            object_id: ObjectId::from_bytes([0x11; 16]),
        },
        Operation::MoveEntry {
            from_directory_id: ObjectId::from_bytes([0x12; 16]),
            from_name: name("src"),
            to_directory_id: ObjectId::from_bytes([0x13; 16]),
            to_name: name("source"),
            object_id: ObjectId::from_bytes([0x14; 16]),
        },
        Operation::DeleteObject {
            object_id: ObjectId::from_bytes([0x11; 16]),
        },
        Operation::RestoreObject {
            object_id: ObjectId::from_bytes([0x11; 16]),
            restored_version_id: VersionId::from_bytes([0x21; 32]),
        },
        Operation::SetPortableMetadata {
            object_id: ObjectId::from_bytes([0x11; 16]),
            version_id: VersionId::from_bytes([0x21; 32]),
            portable_metadata: PortableMetadata::new(false),
        },
        Operation::ResolveNameConflict {
            directory_id: ObjectId::from_bytes([0x12; 16]),
            contested_name: name("notes.md"),
            preserved: vec![
                PreservedEntry::new(ObjectId::from_bytes([0x15; 16]), name("notes.md")),
                PreservedEntry::new(ObjectId::from_bytes([0x16; 16]), name("notes (agent-a).md")),
            ],
        },
        Operation::ResolveContentConflict {
            object_id: ObjectId::from_bytes([0x11; 16]),
            resulting_version_id: VersionId::from_bytes([0x22; 32]),
            preserved_version_ids: vec![
                VersionId::from_bytes([0x20; 32]),
                VersionId::from_bytes([0x21; 32]),
            ],
        },
        Operation::AdvanceActorHead {
            actor_id: ActorId::from_bytes([0x41; 32]),
            from_head: HeadId::from_bytes([0x51; 32]),
            to_head: HeadId::from_bytes([0x52; 32]),
        },
        Operation::RecordReadObservation {
            actor_id: ActorId::from_bytes([0x41; 32]),
            object_id: ObjectId::from_bytes([0x11; 16]),
            version_id: VersionId::from_bytes([0x21; 32]),
            region: ReadRegion::ByteRanges,
            confidence: AttributionConfidence::ProcessInferred,
        },
        Operation::RecordDerivedNode {
            node_id: DerivationId::from_bytes([0x61; 32]),
            node_kind: DerivationKind::SourceIndex,
            exact_inputs: vec![VersionId::from_bytes([0x21; 32])],
            configuration_digest: ContentHash::from_bytes([0x71; 32]),
            output_versions: vec![VersionId::from_bytes([0x23; 32])],
            deterministic: true,
        },
        Operation::CreateReviewBundle {
            bundle_id: ReviewBundleId::from_bytes([0x81; 32]),
            actor_head: HeadId::from_bytes([0x52; 32]),
            base_head: HeadId::from_bytes([0x51; 32]),
        },
        Operation::RecordValidation {
            subject_head: HeadId::from_bytes([0x52; 32]),
            validator_id: ActorId::from_bytes([0x42; 32]),
            outcome: ValidationOutcome::Passed,
            evidence: ContentHash::from_bytes([0x72; 32]),
        },
        Operation::AdvanceCanonicalHead {
            from_head: HeadId::from_bytes([0x51; 32]),
            to_head: HeadId::from_bytes([0x52; 32]),
            approval_id: ApprovalId::from_bytes([0x91; 32]),
        },
        Operation::InitializeWorkspace {
            root_id: ObjectId::from_bytes([0x10; 16]),
        },
    ]
}

/// Every operation whose shape has a variable part, at the empty and the several ends of it.
///
/// A round-trip corpus of one value per member misses the two encodings a sequence field can get
/// wrong: the empty array, and an array whose length crosses a `mesh-cbor/0` head boundary. These
/// are the same members again at those extremes.
#[must_use]
pub fn variable_length_operations() -> Vec<Operation> {
    let many: Vec<VersionId> = (0u8..30)
        .map(|byte| VersionId::from_bytes([byte; 32]))
        .collect();
    vec![
        Operation::WriteFileVersion {
            object_id: ObjectId::from_bytes([0x11; 16]),
            version_id: VersionId::from_bytes([0x21; 32]),
            parent_versions: Vec::new(),
            manifest_id: ManifestId::from_bytes([0x31; 32]),
            portable_metadata: PortableMetadata::new(false),
        },
        Operation::WriteFileVersion {
            object_id: ObjectId::from_bytes([0x11; 16]),
            version_id: VersionId::from_bytes([0x21; 32]),
            parent_versions: many.clone(),
            manifest_id: ManifestId::from_bytes([0x31; 32]),
            portable_metadata: PortableMetadata::new(true),
        },
        Operation::ResolveNameConflict {
            directory_id: ObjectId::from_bytes([0x12; 16]),
            contested_name: name("notes.md"),
            preserved: Vec::new(),
        },
        Operation::ResolveContentConflict {
            object_id: ObjectId::from_bytes([0x11; 16]),
            resulting_version_id: VersionId::from_bytes([0x22; 32]),
            preserved_version_ids: Vec::new(),
        },
        Operation::RecordDerivedNode {
            node_id: DerivationId::from_bytes([0x61; 32]),
            node_kind: DerivationKind::ModelSummary,
            exact_inputs: many.clone(),
            configuration_digest: ContentHash::from_bytes([0x71; 32]),
            output_versions: many,
            deterministic: false,
        },
    ]
}

/// Whether the corpus covers every member of the vocabulary exactly once.
///
/// Published so a downstream conformance suite can state the same coverage claim without
/// reimplementing the check.
#[must_use]
pub fn corpus_covers_the_vocabulary() -> bool {
    let corpus = one_of_every_operation();
    corpus.len() == OperationKind::ALL.len()
        && corpus
            .iter()
            .zip(OperationKind::ALL)
            .all(|(operation, kind)| operation.kind() == kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_corpus_covers_the_vocabulary() {
        assert!(corpus_covers_the_vocabulary());
    }

    #[test]
    fn the_variable_length_corpus_reaches_both_extremes() {
        let corpus = variable_length_operations();
        assert!(corpus.iter().any(|operation| matches!(
            operation,
            Operation::WriteFileVersion { parent_versions, .. } if parent_versions.is_empty()
        )));
        // Thirty elements crosses the `mesh-cbor/0` boundary at twenty-four, where the array head
        // grows from one byte to two.
        assert!(corpus.iter().any(|operation| matches!(
            operation,
            Operation::WriteFileVersion { parent_versions, .. } if parent_versions.len() > 24
        )));
    }
}
