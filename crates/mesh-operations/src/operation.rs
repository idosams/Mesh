//! The operation vocabulary: plan §4.3's eighteen verbs, and nothing else.
//!
//! # What an operation is, and what it is deliberately not
//!
//! An operation is a **meaningful transition**, not a syscall. Plan §4.3 ends on the sentence this
//! whole module is arranged around:
//!
//! > Raw `write()` calls are not distributed as permanent user-visible operations. They are
//! > accumulated locally and coalesced into durable file versions.
//!
//! So there is no variant here that carries bytes, an offset, a file descriptor or a length. The
//! only way content enters the vocabulary is [`Operation::WriteFileVersion`], which names a
//! [`ManifestId`] — a content-addressed manifest that already exists in the content plane.
//! `src/coalesce.rs` is the one path from raw writes to that operation, and it cannot be walked
//! without a manifest identifier. `tests/no_raw_write_is_distributable.rs` reads this crate's own
//! public surface and fails if a byte-carrying constructor ever appears.
//!
//! # Moving a directory is one operation, whatever is under it
//!
//! [`Operation::MoveEntry`] names the *entry*: the directory it leaves, the directory it joins,
//! the two names and the object. It does not name a descendant, and it cannot: a directory version
//! binds a name to a child **object identifier**, so everything beneath the moved object continues
//! to hang off that object with no record of where its ancestor is linked. The published budget —
//! plan §11, one million descendants moved in under 100 ms, under 10 KiB of metadata for a subtree
//! move — is therefore a property of the *shape* of this operation rather than of an optimization,
//! and `tests/subtree_move_is_constant_cost.rs` measures it at four subtree sizes.
//!
//! A vocabulary that spelled a move as "unlink every descendant and relink it" would fail that
//! budget by construction, and no amount of implementation work downstream would recover it.

use crate::ids::{
    ActorId, ApprovalId, ContentHash, DerivationId, HeadId, ManifestId, ObjectId, ReviewBundleId,
    VersionId,
};
use crate::name::{NormalizedName, PortableMetadata};

/// How much of an object an actor read, at the granularity plan §4.8 enumerates.
///
/// The *extent* — which byte ranges, which cells, which symbols — is the context ledger's to
/// carry. What the vocabulary binds is the granularity claim, because that is what a reviewer
/// needs in order not to overstate what the system knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReadRegion {
    /// The whole file.
    WholeFile,
    /// Some byte ranges.
    ByteRanges,
    /// Some text sections.
    TextSections,
    /// Some PDF pages.
    PdfPages,
    /// Some spreadsheet cells.
    SpreadsheetCells,
    /// Some code symbols.
    CodeSymbols,
    /// Not known.
    Unknown,
}

impl ReadRegion {
    /// Every region, in the order plan §4.8 lists them. The index is the wire value.
    pub const ALL: [Self; 7] = [
        Self::WholeFile,
        Self::ByteRanges,
        Self::TextSections,
        Self::PdfPages,
        Self::SpreadsheetCells,
        Self::CodeSymbols,
        Self::Unknown,
    ];

    /// The published name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::WholeFile => "WholeFile",
            Self::ByteRanges => "ByteRanges",
            Self::TextSections => "TextSections",
            Self::PdfPages => "PDFPages",
            Self::SpreadsheetCells => "SpreadsheetCells",
            Self::CodeSymbols => "CodeSymbols",
            Self::Unknown => "Unknown",
        }
    }
}

/// How confident the system is that a read really happened, from plan §4.8.
///
/// The enumeration exists so that an inference is never recorded as an observation. Collapsing it
/// would let a read-ahead in the filesystem layer be reported to a human as "the agent read this".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AttributionConfidence {
    /// Reported by an integrated agent.
    ExactIntegratedRead,
    /// Observed as an exact filesystem range.
    ExactFilesystemRange,
    /// A filesystem read-ahead, which may not have been used.
    FilesystemReadAhead,
    /// Inferred from process behaviour.
    ProcessInferred,
    /// Detected during recovery.
    RecoveryDetected,
    /// Not known.
    Unknown,
}

impl AttributionConfidence {
    /// Every confidence, in the order plan §4.8 lists them. The index is the wire value.
    pub const ALL: [Self; 6] = [
        Self::ExactIntegratedRead,
        Self::ExactFilesystemRange,
        Self::FilesystemReadAhead,
        Self::ProcessInferred,
        Self::RecoveryDetected,
        Self::Unknown,
    ];

    /// The published name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ExactIntegratedRead => "ExactIntegratedRead",
            Self::ExactFilesystemRange => "ExactFilesystemRange",
            Self::FilesystemReadAhead => "FilesystemReadAhead",
            Self::ProcessInferred => "ProcessInferred",
            Self::RecoveryDetected => "RecoveryDetected",
            Self::Unknown => "Unknown",
        }
    }

    /// Whether this confidence may be presented to a human as an observed read.
    ///
    /// Plan §4.8: *"This distinction prevents overstating what the system knows."* Three of the
    /// six are inferences, and a reviewer that treats them as observations is being told something
    /// the system does not know.
    #[must_use]
    pub const fn is_observed(&self) -> bool {
        matches!(self, Self::ExactIntegratedRead | Self::ExactFilesystemRange)
    }
}

/// What kind of computation produced a derived node, from plan §4.10's initial uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DerivationKind {
    /// Document parsing.
    DocumentParse,
    /// Source indexing.
    SourceIndex,
    /// Diff generation.
    DiffGeneration,
    /// Embeddings.
    Embedding,
    /// Compact run memory.
    RunMemory,
    /// Test results.
    TestResult,
    /// Validation.
    Validation,
    /// A model summary.
    ModelSummary,
}

impl DerivationKind {
    /// Every kind, in the order plan §4.10 lists them. The index is the wire value.
    pub const ALL: [Self; 8] = [
        Self::DocumentParse,
        Self::SourceIndex,
        Self::DiffGeneration,
        Self::Embedding,
        Self::RunMemory,
        Self::TestResult,
        Self::Validation,
        Self::ModelSummary,
    ];

    /// The published name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::DocumentParse => "DocumentParse",
            Self::SourceIndex => "SourceIndex",
            Self::DiffGeneration => "DiffGeneration",
            Self::Embedding => "Embedding",
            Self::RunMemory => "RunMemory",
            Self::TestResult => "TestResult",
            Self::Validation => "Validation",
            Self::ModelSummary => "ModelSummary",
        }
    }
}

/// What a validator concluded about a head.
///
/// Plan §9 does not enumerate this. Four values are the smallest set §9.2's hard guards and §9.3's
/// warnings need: a check that passed, one that failed, one that was not run, and one that could
/// not run. Widening this is a protocol change under this crate's failure-and-recovery rule, not a
/// field addition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ValidationOutcome {
    /// The check ran and passed.
    Passed,
    /// The check ran and failed.
    Failed,
    /// The check was not selected for this change.
    Skipped,
    /// The check could not be run, so nothing is known either way.
    Errored,
}

impl ValidationOutcome {
    /// Every outcome. The index is the wire value.
    pub const ALL: [Self; 4] = [Self::Passed, Self::Failed, Self::Skipped, Self::Errored];

    /// The published name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Passed => "Passed",
            Self::Failed => "Failed",
            Self::Skipped => "Skipped",
            Self::Errored => "Errored",
        }
    }

    /// Whether this outcome may be presented as evidence that a check succeeded.
    ///
    /// `Skipped` and `Errored` are the two an interface is tempted to render as a green tick.
    #[must_use]
    pub const fn is_evidence_of_success(&self) -> bool {
        matches!(self, Self::Passed)
    }
}

/// One entry a name-conflict resolution preserves: an object, and the name it keeps.
///
/// Every contender keeps a name. The conflict rules never discard one of two concurrently created
/// objects, so a resolution that named only a winner would have no way to express the loser's new
/// name and the preservation rule would live in prose.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PreservedEntry {
    object_id: ObjectId,
    name: NormalizedName,
}

impl PreservedEntry {
    /// An object and the name it keeps.
    #[must_use]
    pub const fn new(object_id: ObjectId, name: NormalizedName) -> Self {
        Self { object_id, name }
    }

    /// The object.
    #[must_use]
    pub const fn object_id(&self) -> ObjectId {
        self.object_id
    }

    /// The name it keeps.
    #[must_use]
    pub const fn name(&self) -> &NormalizedName {
        &self.name
    }
}

/// Which member of the vocabulary an operation is.
///
/// The order is plan §4.3's order, and `tests/vocabulary_matches_the_plan.rs` reads that section
/// out of `docs/plan/execution-plan.md` and requires the eighteen names to match it exactly, in
/// order. A member added here without a plan change turns that test red, which is the mechanical
/// half of this crate's rule that *adding an operation is a protocol change*.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OperationKind {
    /// Mint a file object.
    CreateFile,
    /// Mint a directory object.
    CreateDirectory,
    /// Record a durable file version over an already content-addressed manifest.
    WriteFileVersion,
    /// Bind a name in a directory to a child object version.
    LinkDirectoryEntry,
    /// Remove a name binding from a directory.
    UnlinkDirectoryEntry,
    /// Change an entry's name within one directory.
    RenameEntry,
    /// Move an entry from one directory to another.
    MoveEntry,
    /// Mark an object deleted.
    DeleteObject,
    /// Bring a deleted object back at a stated version.
    RestoreObject,
    /// Set the portable metadata of an object version.
    SetPortableMetadata,
    /// Settle a contested name, preserving every contender.
    ResolveNameConflict,
    /// Settle divergent content, preserving every superseded version.
    ResolveContentConflict,
    /// Move an actor's private head.
    AdvanceActorHead,
    /// Record that an actor read a version of an object.
    RecordReadObservation,
    /// Record a derived computation node and its exact inputs.
    RecordDerivedNode,
    /// Offer an actor head for human review.
    CreateReviewBundle,
    /// Record an independent validator's conclusion about a head.
    RecordValidation,
    /// Move the protected shared version, under an approval envelope.
    AdvanceCanonicalHead,
}

impl OperationKind {
    /// Every member, in plan §4.3 order.
    pub const ALL: [Self; 18] = [
        Self::CreateFile,
        Self::CreateDirectory,
        Self::WriteFileVersion,
        Self::LinkDirectoryEntry,
        Self::UnlinkDirectoryEntry,
        Self::RenameEntry,
        Self::MoveEntry,
        Self::DeleteObject,
        Self::RestoreObject,
        Self::SetPortableMetadata,
        Self::ResolveNameConflict,
        Self::ResolveContentConflict,
        Self::AdvanceActorHead,
        Self::RecordReadObservation,
        Self::RecordDerivedNode,
        Self::CreateReviewBundle,
        Self::RecordValidation,
        Self::AdvanceCanonicalHead,
    ];

    /// The name plan §4.3 gives this member.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::CreateFile => "CreateFile",
            Self::CreateDirectory => "CreateDirectory",
            Self::WriteFileVersion => "WriteFileVersion",
            Self::LinkDirectoryEntry => "LinkDirectoryEntry",
            Self::UnlinkDirectoryEntry => "UnlinkDirectoryEntry",
            Self::RenameEntry => "RenameEntry",
            Self::MoveEntry => "MoveEntry",
            Self::DeleteObject => "DeleteObject",
            Self::RestoreObject => "RestoreObject",
            Self::SetPortableMetadata => "SetPortableMetadata",
            Self::ResolveNameConflict => "ResolveNameConflict",
            Self::ResolveContentConflict => "ResolveContentConflict",
            Self::AdvanceActorHead => "AdvanceActorHead",
            Self::RecordReadObservation => "RecordReadObservation",
            Self::RecordDerivedNode => "RecordDerivedNode",
            Self::CreateReviewBundle => "CreateReviewBundle",
            Self::RecordValidation => "RecordValidation",
            Self::AdvanceCanonicalHead => "AdvanceCanonicalHead",
        }
    }
}

/// One meaningful transition an actor performed.
///
/// See the module header for what is deliberately absent. Every variant's fields are the complete
/// content of that transition: an applier that needs a field this type does not carry is being
/// asked to guess, which is the failure the vocabulary exists to prevent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    /// Mint a file object. The object exists with no version and no name until a
    /// [`Operation::WriteFileVersion`] and a [`Operation::LinkDirectoryEntry`] follow.
    CreateFile {
        /// The object being minted.
        object_id: ObjectId,
    },
    /// Mint a directory object.
    CreateDirectory {
        /// The object being minted.
        object_id: ObjectId,
    },
    /// Record a durable file version.
    ///
    /// `manifest_id` names content that is already in the content plane. There is no byte here,
    /// and that is the vocabulary's half of "raw writes are not distributed".
    WriteFileVersion {
        /// The file object.
        object_id: ObjectId,
        /// The version being recorded.
        version_id: VersionId,
        /// The versions it supersedes — several when it settles a divergence.
        parent_versions: Vec<VersionId>,
        /// The content-addressed manifest.
        manifest_id: ManifestId,
        /// The portable metadata carried with it.
        portable_metadata: PortableMetadata,
    },
    /// Bind a name in a directory to a child object version.
    LinkDirectoryEntry {
        /// The directory.
        directory_id: ObjectId,
        /// The name being bound.
        name: NormalizedName,
        /// The child object.
        object_id: ObjectId,
        /// The child version.
        version_id: VersionId,
    },
    /// Remove a name binding.
    ///
    /// `object_id` states which binding is meant, so an unlink that arrives after the name was
    /// rebound to another object is refused rather than removing the wrong entry.
    UnlinkDirectoryEntry {
        /// The directory.
        directory_id: ObjectId,
        /// The name being unbound.
        name: NormalizedName,
        /// The object the name is expected to be bound to.
        object_id: ObjectId,
    },
    /// Change an entry's name within one directory.
    RenameEntry {
        /// The directory.
        directory_id: ObjectId,
        /// The name it had.
        from_name: NormalizedName,
        /// The name it takes.
        to_name: NormalizedName,
        /// The object whose entry moves.
        object_id: ObjectId,
    },
    /// Move an entry from one directory to another.
    ///
    /// **Constant cost in the size of the subtree.** Nothing under `object_id` is named, because a
    /// directory version binds a name to a child object identifier and every descendant continues
    /// to hang off that object. See the module header.
    MoveEntry {
        /// The directory it leaves.
        from_directory_id: ObjectId,
        /// The name it had there.
        from_name: NormalizedName,
        /// The directory it joins.
        to_directory_id: ObjectId,
        /// The name it takes there.
        to_name: NormalizedName,
        /// The object whose entry moves — a file or the root of a subtree, identically.
        object_id: ObjectId,
    },
    /// Mark an object deleted. Its versions are preserved; deletion is a state, never an erasure.
    DeleteObject {
        /// The object.
        object_id: ObjectId,
    },
    /// Bring a deleted object back at a stated version.
    RestoreObject {
        /// The object.
        object_id: ObjectId,
        /// The version it comes back at.
        restored_version_id: VersionId,
    },
    /// Set the portable metadata of an object version.
    SetPortableMetadata {
        /// The object.
        object_id: ObjectId,
        /// The version whose metadata is set.
        version_id: VersionId,
        /// The metadata.
        portable_metadata: PortableMetadata,
    },
    /// Settle a contested name.
    ///
    /// `preserved` carries every contender and the name it keeps — including the one that keeps
    /// the contested name. A resolution naming only a winner could not express the others, and the
    /// preservation rule would live in prose instead of in the record.
    ResolveNameConflict {
        /// The directory the contest was in.
        directory_id: ObjectId,
        /// The contested name.
        contested_name: NormalizedName,
        /// Every contender and the name it keeps.
        preserved: Vec<PreservedEntry>,
    },
    /// Settle divergent content for one object.
    ResolveContentConflict {
        /// The object.
        object_id: ObjectId,
        /// The version the resolution produces.
        resulting_version_id: VersionId,
        /// Every version it supersedes, each of which remains reachable.
        preserved_version_ids: Vec<VersionId>,
    },
    /// Move an actor's private head.
    AdvanceActorHead {
        /// The actor.
        actor_id: ActorId,
        /// The head it held.
        from_head: HeadId,
        /// The head it now holds.
        to_head: HeadId,
    },
    /// Record that an actor read a version of an object.
    RecordReadObservation {
        /// The reading actor.
        actor_id: ActorId,
        /// The object read.
        object_id: ObjectId,
        /// The exact version read.
        version_id: VersionId,
        /// How much of it.
        region: ReadRegion,
        /// How well the system knows this happened.
        confidence: AttributionConfidence,
    },
    /// Record a derived computation node and the exact versions it consumed.
    RecordDerivedNode {
        /// The node.
        node_id: DerivationId,
        /// What kind of computation it was.
        node_kind: DerivationKind,
        /// The exact input versions, so staleness is derivable rather than guessed.
        exact_inputs: Vec<VersionId>,
        /// A digest over the tool identity and configuration.
        configuration_digest: ContentHash,
        /// The versions it produced.
        output_versions: Vec<VersionId>,
        /// Whether re-running it on the same inputs is expected to reproduce the outputs.
        deterministic: bool,
    },
    /// Offer an actor head for human review.
    CreateReviewBundle {
        /// The bundle.
        bundle_id: ReviewBundleId,
        /// The actor head being offered.
        actor_head: HeadId,
        /// The head it is diffed against.
        base_head: HeadId,
    },
    /// Record an independent validator's conclusion about a head.
    RecordValidation {
        /// The head that was validated.
        subject_head: HeadId,
        /// The validator.
        validator_id: ActorId,
        /// What it concluded.
        outcome: ValidationOutcome,
        /// A digest over the evidence — the command, the environment, the artifacts.
        evidence: ContentHash,
    },
    /// Move the protected shared version.
    ///
    /// Carries the approval envelope's identifier, so an advance with no human approval behind it
    /// is not expressible.
    AdvanceCanonicalHead {
        /// The head it moves from.
        from_head: HeadId,
        /// The head it moves to.
        to_head: HeadId,
        /// The approval envelope authorising it.
        approval_id: ApprovalId,
    },
}

impl Operation {
    /// Which member of the vocabulary this is.
    #[must_use]
    pub const fn kind(&self) -> OperationKind {
        match self {
            Self::CreateFile { .. } => OperationKind::CreateFile,
            Self::CreateDirectory { .. } => OperationKind::CreateDirectory,
            Self::WriteFileVersion { .. } => OperationKind::WriteFileVersion,
            Self::LinkDirectoryEntry { .. } => OperationKind::LinkDirectoryEntry,
            Self::UnlinkDirectoryEntry { .. } => OperationKind::UnlinkDirectoryEntry,
            Self::RenameEntry { .. } => OperationKind::RenameEntry,
            Self::MoveEntry { .. } => OperationKind::MoveEntry,
            Self::DeleteObject { .. } => OperationKind::DeleteObject,
            Self::RestoreObject { .. } => OperationKind::RestoreObject,
            Self::SetPortableMetadata { .. } => OperationKind::SetPortableMetadata,
            Self::ResolveNameConflict { .. } => OperationKind::ResolveNameConflict,
            Self::ResolveContentConflict { .. } => OperationKind::ResolveContentConflict,
            Self::AdvanceActorHead { .. } => OperationKind::AdvanceActorHead,
            Self::RecordReadObservation { .. } => OperationKind::RecordReadObservation,
            Self::RecordDerivedNode { .. } => OperationKind::RecordDerivedNode,
            Self::CreateReviewBundle { .. } => OperationKind::CreateReviewBundle,
            Self::RecordValidation { .. } => OperationKind::RecordValidation,
            Self::AdvanceCanonicalHead { .. } => OperationKind::AdvanceCanonicalHead,
        }
    }

    /// Move a subtree, whatever is under it.
    ///
    /// Returns the complete operation set for the move: one operation, always, at any subtree
    /// size. This constructor exists so the budget is a fact about the API rather than a claim
    /// about how a caller might use it — there is no correct way to spell a move as N operations,
    /// so there is no way to spell it that way here.
    ///
    /// ```
    /// use mesh_operations::{ObjectId, NormalizedName, Operation};
    ///
    /// let ops = Operation::move_subtree(
    ///     ObjectId::from_bytes([1; 16]),
    ///     NormalizedName::new("src").unwrap(),
    ///     ObjectId::from_bytes([2; 16]),
    ///     NormalizedName::new("source").unwrap(),
    ///     ObjectId::from_bytes([3; 16]),
    /// );
    /// assert_eq!(ops.len(), 1);
    /// ```
    #[must_use]
    pub fn move_subtree(
        from_directory_id: ObjectId,
        from_name: NormalizedName,
        to_directory_id: ObjectId,
        to_name: NormalizedName,
        object_id: ObjectId,
    ) -> Vec<Self> {
        vec![Self::MoveEntry {
            from_directory_id,
            from_name,
            to_directory_id,
            to_name,
            object_id,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vocabulary_has_exactly_eighteen_members_and_no_duplicate_name() {
        assert_eq!(OperationKind::ALL.len(), 18);
        let mut names: Vec<&str> = OperationKind::ALL
            .iter()
            .map(OperationKind::as_str)
            .collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 18);
    }

    #[test]
    fn every_member_of_all_reports_itself_as_its_own_kind() {
        // `kind()` is eighteen hand-written arms and a transposed pair would be invisible to a
        // round-trip test, because both directions would agree on the wrong answer.
        let samples = crate::corpus::one_of_every_operation();
        assert_eq!(samples.len(), OperationKind::ALL.len());
        for (operation, expected) in samples.iter().zip(OperationKind::ALL) {
            assert_eq!(operation.kind(), expected, "{}", expected.as_str());
        }
    }

    #[test]
    fn the_enumerated_vocabularies_have_no_duplicate_name() {
        for names in [
            ReadRegion::ALL
                .iter()
                .map(ReadRegion::as_str)
                .collect::<Vec<_>>(),
            AttributionConfidence::ALL
                .iter()
                .map(AttributionConfidence::as_str)
                .collect(),
            DerivationKind::ALL
                .iter()
                .map(DerivationKind::as_str)
                .collect(),
            ValidationOutcome::ALL
                .iter()
                .map(ValidationOutcome::as_str)
                .collect(),
        ] {
            let mut sorted = names.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), names.len());
        }
    }

    #[test]
    fn an_inference_is_not_an_observation() {
        assert!(AttributionConfidence::ExactIntegratedRead.is_observed());
        assert!(AttributionConfidence::ExactFilesystemRange.is_observed());
        for inferred in [
            AttributionConfidence::FilesystemReadAhead,
            AttributionConfidence::ProcessInferred,
            AttributionConfidence::RecoveryDetected,
            AttributionConfidence::Unknown,
        ] {
            assert!(!inferred.is_observed(), "{}", inferred.as_str());
        }
    }

    #[test]
    fn only_a_pass_is_evidence_of_success() {
        assert!(ValidationOutcome::Passed.is_evidence_of_success());
        for other in [
            ValidationOutcome::Failed,
            ValidationOutcome::Skipped,
            ValidationOutcome::Errored,
        ] {
            assert!(!other.is_evidence_of_success(), "{}", other.as_str());
        }
    }

    #[test]
    fn a_move_is_one_operation() {
        let ops = Operation::move_subtree(
            ObjectId::from_bytes([1; 16]),
            NormalizedName::new("src").unwrap(),
            ObjectId::from_bytes([2; 16]),
            NormalizedName::new("source").unwrap(),
            ObjectId::from_bytes([3; 16]),
        );
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].kind(), OperationKind::MoveEntry);
    }
}
