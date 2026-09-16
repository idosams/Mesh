//! What an operation did, and why one did nothing.
//!
//! # Totality is the contract, so refusal is a value and never a panic
//!
//! Materialization is *total*: every operation in every valid operation set produces one of the
//! three [`Effect`]s or one [`Rejection`], and nothing produces an undefined state or unwinds. A
//! rejected operation leaves the state exactly as it was and is recorded in
//! [`crate::Materialization::rejections`], so a caller can see what the set asked for and what the
//! state would not accept without re-deriving either.
//!
//! That is a stronger property than "it does not crash". An operation set arriving from a peer is
//! not trusted input — it can name an object that does not exist, bind a name that is taken, or ask
//! a directory to become its own ancestor — and every one of those has to have an answer that two
//! peers compute identically. A panic is not such an answer, and neither is dropping the operation
//! silently.
//!
//! # Three effects, and the third is not the first
//!
//! [`Effect::OutsideStateGraph`] is not [`Effect::AlreadyInEffect`]. The first says *this verb never
//! writes a workspace state* — it is a dependency-graph or trust-graph fact, one of the four
//! members returned by [`crate::verbs_outside_the_state_graph`]. The second says *this verb writes
//! states, and this state already says what it asks for*. Collapsing them would make "the read
//! ledger is not part of the state" indistinguishable from "this write was a duplicate".

use core::fmt;

use crate::ids::{ChangeSetId, HeadId, ObjectId, VersionId};
use crate::name::NormalizedName;
use crate::operation::OperationKind;

/// What applying one operation did to the state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Effect {
    /// The state changed.
    Applied,
    /// The verb writes states, and this state already said what it asked for. Nothing changed.
    AlreadyInEffect,
    /// The verb belongs to the dependency graph or the trust graph and never writes a workspace
    /// state. Nothing changed, and nothing was wrong.
    OutsideStateGraph,
}

/// Why an operation left the state alone.
///
/// Every variant names the exact entity that made the answer what it is, so a divergence between
/// two implementations can be localised to an identifier rather than to a boolean.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rejection {
    /// The operation named an object that has not been created.
    UnknownObject {
        /// The object.
        object: ObjectId,
    },
    /// The operation minted an object identifier that already names an object.
    ObjectAlreadyExists {
        /// The object.
        object: ObjectId,
    },
    /// The operation needed a directory and named a file.
    NotADirectory {
        /// The object.
        object: ObjectId,
    },
    /// The operation needed a file and named a directory.
    NotAFile {
        /// The object.
        object: ObjectId,
    },
    /// The operation named a deleted object. `RestoreObject` is the way back.
    ObjectDeleted {
        /// The object.
        object: ObjectId,
    },
    /// The operation named the workspace root where the root cannot be the subject.
    ///
    /// The root is not deletable, linkable, renamable or movable: it is the node a state is named
    /// by, and a workspace whose root moved would be a different workspace.
    RootObject {
        /// The root.
        object: ObjectId,
    },
    /// The name is already bound to another object. Settling that is `ResolveNameConflict`'s.
    NameTaken {
        /// The directory.
        directory: ObjectId,
        /// The name.
        name: NormalizedName,
        /// What holds it.
        bound_to: ObjectId,
    },
    /// The directory binds no such name.
    EntryNotBound {
        /// The directory.
        directory: ObjectId,
        /// The name.
        name: NormalizedName,
    },
    /// The name is bound, to a different object than the operation expected.
    EntryBoundElsewhere {
        /// The directory.
        directory: ObjectId,
        /// The name.
        name: NormalizedName,
        /// What actually holds it.
        bound_to: ObjectId,
    },
    /// The object is already bound in a directory. An object is bound at most once — see
    /// `crate::WorkspaceState` for why there are no hard links.
    AlreadyLinked {
        /// The object.
        object: ObjectId,
        /// Where it is bound.
        directory: ObjectId,
    },
    /// The operation would make a directory its own ancestor, which SG-5 forbids.
    WouldCycle {
        /// The object being placed.
        object: ObjectId,
        /// Where it was being placed.
        directory: ObjectId,
    },
    /// The operation named a version no `WriteFileVersion` has recorded.
    UnknownVersion {
        /// The version.
        version: VersionId,
    },
    /// A resolution had to name the version a contender keeps, and that contender has none: it is
    /// bound nowhere and nothing has ever been written to it.
    NoKnownVersion {
        /// The object.
        object: ObjectId,
    },
    /// The version identifier is already recorded, with different content.
    VersionAlreadyRecorded {
        /// The version.
        version: VersionId,
    },
    /// The version exists and belongs to another object.
    VersionObjectMismatch {
        /// The version.
        version: VersionId,
        /// The object the operation named.
        expected: ObjectId,
        /// The object the version really belongs to.
        found: ObjectId,
    },
    /// A resolution preserved nothing. Every conflict rule preserves every contender, so an empty
    /// resolution is a resolution that discards work.
    EmptyResolution {
        /// The directory.
        directory: ObjectId,
    },
    /// A resolution named one object or one name twice, so it does not describe one outcome.
    DuplicateResolution {
        /// The directory or object the resolution was for.
        subject: ObjectId,
    },
    /// The head an advance claimed to move from is not the head this state holds.
    HeadNotCurrent {
        /// What the operation claimed.
        claimed: HeadId,
        /// What the state holds.
        current: HeadId,
    },
}

impl fmt::Display for Rejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownObject { object } => write!(formatter, "no object {object}"),
            Self::ObjectAlreadyExists { object } => write!(formatter, "object {object} exists"),
            Self::NotADirectory { object } => write!(formatter, "object {object} is a file"),
            Self::NotAFile { object } => write!(formatter, "object {object} is a directory"),
            Self::ObjectDeleted { object } => write!(formatter, "object {object} is deleted"),
            Self::RootObject { object } => write!(
                formatter,
                "object {object} is the workspace root, which no operation moves"
            ),
            Self::NameTaken {
                directory,
                name,
                bound_to,
            } => write!(
                formatter,
                "{directory} already binds \"{name}\" to {bound_to}"
            ),
            Self::EntryNotBound { directory, name } => {
                write!(formatter, "{directory} binds no \"{name}\"")
            }
            Self::EntryBoundElsewhere {
                directory,
                name,
                bound_to,
            } => write!(
                formatter,
                "{directory} binds \"{name}\" to {bound_to}, not to what the operation expected"
            ),
            Self::AlreadyLinked { object, directory } => {
                write!(formatter, "object {object} is already bound in {directory}")
            }
            Self::WouldCycle { object, directory } => write!(
                formatter,
                "placing {object} in {directory} would make it its own ancestor"
            ),
            Self::UnknownVersion { version } => write!(formatter, "no version {version}"),
            Self::NoKnownVersion { object } => write!(
                formatter,
                "object {object} has no version for a resolution to preserve"
            ),
            Self::VersionAlreadyRecorded { version } => write!(
                formatter,
                "version {version} is recorded with different content"
            ),
            Self::VersionObjectMismatch {
                version,
                expected,
                found,
            } => write!(
                formatter,
                "version {version} belongs to {found}, not to {expected}"
            ),
            Self::EmptyResolution { directory } => write!(
                formatter,
                "a resolution in {directory} preserved no contender"
            ),
            Self::DuplicateResolution { subject } => {
                write!(
                    formatter,
                    "a resolution for {subject} named one thing twice"
                )
            }
            Self::HeadNotCurrent { claimed, current } => write!(
                formatter,
                "the advance claims to move from {claimed}, and the current head is {current}"
            ),
        }
    }
}

impl std::error::Error for Rejection {}

/// One operation the state would not accept, and where it came from.
///
/// The position is kept because an operation set can hold the same operation twice, and "the second
/// one was refused" is a different fact from "one of them was refused".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedOperation {
    changeset: ChangeSetId,
    index: usize,
    kind: OperationKind,
    rejection: Rejection,
}

impl RejectedOperation {
    /// A rejection, with its origin.
    #[must_use]
    pub const fn new(
        changeset: ChangeSetId,
        index: usize,
        kind: OperationKind,
        rejection: Rejection,
    ) -> Self {
        Self {
            changeset,
            index,
            kind,
            rejection,
        }
    }

    /// The ChangeSet the operation arrived in.
    #[must_use]
    pub const fn changeset(&self) -> ChangeSetId {
        self.changeset
    }

    /// Its position within that ChangeSet's operation list.
    #[must_use]
    pub const fn index(&self) -> usize {
        self.index
    }

    /// Which verb it was.
    #[must_use]
    pub const fn kind(&self) -> OperationKind {
        self.kind
    }

    /// Why the state would not accept it.
    #[must_use]
    pub const fn rejection(&self) -> &Rejection {
        &self.rejection
    }
}

impl fmt::Display for RejectedOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} #{} in {}: {}",
            self.kind.as_str(),
            self.index,
            self.changeset,
            self.rejection
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_effect_that_did_nothing_is_not_an_effect_that_was_not_a_state_fact() {
        assert_ne!(Effect::AlreadyInEffect, Effect::OutsideStateGraph);
        assert_ne!(Effect::Applied, Effect::AlreadyInEffect);
    }

    #[test]
    fn every_rejection_says_which_entity_made_it_so() {
        let object = ObjectId::from_bytes([1; 16]);
        let rendered = Rejection::UnknownObject { object }.to_string();
        assert!(rendered.contains(&object.to_string()));
    }

    #[test]
    fn a_rejection_record_names_the_operation_and_its_position() {
        let record = RejectedOperation::new(
            ChangeSetId::from_bytes([2; 32]),
            3,
            OperationKind::MoveEntry,
            Rejection::UnknownObject {
                object: ObjectId::from_bytes([1; 16]),
            },
        );
        let rendered = record.to_string();
        assert!(rendered.starts_with("MoveEntry #3 in "));
        assert_eq!(record.index(), 3);
        assert_eq!(record.kind(), OperationKind::MoveEntry);
    }

    /// The user-facing vocabulary rule, asserted rather than trusted: none of these strings may
    /// carry a word from the six-state model's banned list.
    #[test]
    fn no_rejection_message_names_a_forbidden_word() {
        let object = ObjectId::from_bytes([1; 16]);
        let version = VersionId::from_bytes([2; 32]);
        let head = HeadId::from_bytes([3; 32]);
        let name = NormalizedName::new("report.md").unwrap();
        let rejections = [
            Rejection::UnknownObject { object },
            Rejection::ObjectAlreadyExists { object },
            Rejection::NotADirectory { object },
            Rejection::NotAFile { object },
            Rejection::ObjectDeleted { object },
            Rejection::RootObject { object },
            Rejection::NameTaken {
                directory: object,
                name: name.clone(),
                bound_to: object,
            },
            Rejection::EntryNotBound {
                directory: object,
                name: name.clone(),
            },
            Rejection::EntryBoundElsewhere {
                directory: object,
                name,
                bound_to: object,
            },
            Rejection::AlreadyLinked {
                object,
                directory: object,
            },
            Rejection::WouldCycle {
                object,
                directory: object,
            },
            Rejection::UnknownVersion { version },
            Rejection::NoKnownVersion { object },
            Rejection::VersionAlreadyRecorded { version },
            Rejection::VersionObjectMismatch {
                version,
                expected: object,
                found: object,
            },
            Rejection::EmptyResolution { directory: object },
            Rejection::DuplicateResolution { subject: object },
            Rejection::HeadNotCurrent {
                claimed: head,
                current: head,
            },
        ];
        let forbidden = [
            "DAG",
            "frontier",
            "vector clock",
            "branch",
            "commit",
            "rebase",
            "staging",
            " ref ",
            "operation log",
        ];
        for rejection in &rejections {
            let rendered = rejection.to_string().to_lowercase();
            for word in forbidden {
                assert!(
                    !rendered.contains(&word.to_lowercase()),
                    "{rendered:?} names {word:?}"
                );
            }
        }
        assert_eq!(rejections.len(), 18);
    }
}
