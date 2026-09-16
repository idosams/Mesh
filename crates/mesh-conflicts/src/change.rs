//! The concurrent changes the table is a table of.
//!
//! # Six effects, and why not eighteen
//!
//! `mesh-operations` carries eighteen verbs. Plan §4.8's left-hand column distinguishes exactly
//! six kinds of thing: creating an entry, renaming one, moving one, writing text, writing bytes
//! and removing one. A conflict rule that needed a seventh distinction would be a row the table
//! does not have — and the task contract is explicit that a case with no rule is a specification
//! gap, not an implementation choice.
//!
//! So [`Effect`] is a projection, and `tests/mesh_state_drift.rs` is what keeps it projecting.
//!
//! # Placement and content are separate facets, and that is rows one and two
//!
//! [`Effect::Rename`] and [`Effect::Reparent`] carry no version, and [`Effect::WriteText`] and
//! [`Effect::WriteBinary`] carry no name. Nothing in this vocabulary can express "write this
//! content to this path", so nothing downstream of it can lose an edit because a path moved. The
//! first two rows of the conflict table are made true here, by what the vocabulary cannot say,
//! rather than by a rule that has to remember to fire.

use crate::ids::{ActorId, ObjectId, VersionId};
use crate::name::NormalizedName;
use crate::object::ObjectKind;
use crate::stamp::Stamp;

/// One durable change by one actor, positioned in the total order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    stamp: Stamp,
    actor: ActorId,
    effect: Effect,
}

impl Change {
    /// The change with this position, author and effect.
    #[must_use]
    pub const fn new(stamp: Stamp, actor: ActorId, effect: Effect) -> Self {
        Self {
            stamp,
            actor,
            effect,
        }
    }

    /// Where the change sits in the total order.
    #[must_use]
    pub const fn stamp(&self) -> &Stamp {
        &self.stamp
    }

    /// Who authored it.
    #[must_use]
    pub const fn actor(&self) -> ActorId {
        self.actor
    }

    /// What it does.
    #[must_use]
    pub const fn effect(&self) -> &Effect {
        &self.effect
    }

    /// The object it is about.
    #[must_use]
    pub const fn object(&self) -> ObjectId {
        self.effect.object()
    }
}

/// What a change does. Six members, one per distinction plan §4.8 draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Bring an object into existence and hang it in a directory under a name.
    Create {
        /// The object being created.
        object: ObjectId,
        /// Whether it holds content or other objects.
        kind: ObjectKind,
        /// The directory it hangs in.
        directory: ObjectId,
        /// The name it hangs under.
        name: NormalizedName,
    },
    /// Give an object a different name in the same directory.
    Rename {
        /// The object being renamed.
        object: ObjectId,
        /// Its new name.
        name: NormalizedName,
    },
    /// Hang an object in a different directory under the name it already has.
    ///
    /// Named `Reparent` rather than `Move` because `Move` is a word with a meaning in Rust that
    /// this is not.
    Reparent {
        /// The object being moved.
        object: ObjectId,
        /// The directory it moves into.
        directory: ObjectId,
    },
    /// Write a version of an object's content as lines of text.
    WriteText {
        /// The object being written.
        object: ObjectId,
        /// The identity of the version being written.
        version: VersionId,
        /// The content, one entry per line, newlines excluded.
        lines: Vec<String>,
    },
    /// Write a version of an object's content as opaque bytes.
    ///
    /// The bytes themselves are not carried: a conflict rule for binary content never inspects it,
    /// because "preserve both versions" is the rule regardless of what the bytes say.
    WriteBinary {
        /// The object being written.
        object: ObjectId,
        /// The identity of the version being written.
        version: VersionId,
        /// The content hash of the bytes.
        digest: [u8; 32],
        /// How many bytes there are.
        byte_length: u64,
    },
    /// Remove an object from the workspace.
    ///
    /// A removal in this vocabulary is a claim, not an outcome. What it produces is a tombstone;
    /// the versions the object held stay reachable either way.
    Delete {
        /// The object being removed.
        object: ObjectId,
    },
}

impl Effect {
    /// The object this effect is about.
    #[must_use]
    pub const fn object(&self) -> ObjectId {
        match self {
            Self::Create { object, .. }
            | Self::Rename { object, .. }
            | Self::Reparent { object, .. }
            | Self::WriteText { object, .. }
            | Self::WriteBinary { object, .. }
            | Self::Delete { object } => *object,
        }
    }

    /// Whether this effect changes where an object hangs or what it is called.
    #[must_use]
    pub const fn touches_placement(&self) -> bool {
        matches!(
            self,
            Self::Create { .. } | Self::Rename { .. } | Self::Reparent { .. }
        )
    }

    /// Whether this effect changes what an object holds.
    #[must_use]
    pub const fn touches_content(&self) -> bool {
        matches!(
            self,
            Self::WriteText { .. } | Self::WriteBinary { .. } | Self::Delete { .. }
        )
    }

    /// The version this effect writes, when it writes one.
    #[must_use]
    pub const fn version(&self) -> Option<VersionId> {
        match self {
            Self::WriteText { version, .. } | Self::WriteBinary { version, .. } => Some(*version),
            _ => None,
        }
    }

    /// A stable rank, used only to keep the applied order total when two stamps are equal.
    ///
    /// Two changes cannot honestly carry one stamp — the content hash is the third component — so
    /// this exists for the dishonest case, where refusing to be total would mean resolving
    /// differently on two peers. Creation is applied before the moves and renames that reference
    /// it.
    #[must_use]
    pub const fn order_rank(&self) -> u8 {
        match self {
            Self::Create { .. } => 0,
            Self::Reparent { .. } => 1,
            Self::Rename { .. } => 2,
            Self::WriteText { .. } => 3,
            Self::WriteBinary { .. } => 4,
            Self::Delete { .. } => 5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stamp::{EventId, Lamport};

    fn name(text: &str) -> NormalizedName {
        NormalizedName::new(text).unwrap()
    }

    fn stamp() -> Stamp {
        Stamp::new(Lamport::new(1), EventId::from_bytes([1; 16]), [0; 32])
    }

    #[test]
    fn every_effect_names_its_object() {
        let object = ObjectId::from_bytes([3; 16]);
        let effects = [
            Effect::Create {
                object,
                kind: ObjectKind::File,
                directory: ObjectId::from_bytes([0; 16]),
                name: name("a"),
            },
            Effect::Rename {
                object,
                name: name("b"),
            },
            Effect::Reparent {
                object,
                directory: ObjectId::from_bytes([1; 16]),
            },
            Effect::WriteText {
                object,
                version: VersionId::from_bytes([1; 32]),
                lines: vec![],
            },
            Effect::WriteBinary {
                object,
                version: VersionId::from_bytes([2; 32]),
                digest: [0; 32],
                byte_length: 0,
            },
            Effect::Delete { object },
        ];
        for effect in effects {
            assert_eq!(effect.object(), object);
        }
    }

    #[test]
    fn placement_and_content_are_disjoint_facets() {
        let object = ObjectId::from_bytes([3; 16]);
        let rename = Effect::Rename {
            object,
            name: name("b"),
        };
        let write = Effect::WriteText {
            object,
            version: VersionId::from_bytes([1; 32]),
            lines: vec![],
        };
        assert!(rename.touches_placement() && !rename.touches_content());
        assert!(write.touches_content() && !write.touches_placement());
    }

    #[test]
    fn a_change_carries_its_position_and_its_author() {
        let change = Change::new(
            stamp(),
            ActorId::from_bytes([9; 32]),
            Effect::Delete {
                object: ObjectId::from_bytes([4; 16]),
            },
        );
        assert_eq!(change.stamp(), &stamp());
        assert_eq!(change.actor(), ActorId::from_bytes([9; 32]));
        assert_eq!(change.object(), ObjectId::from_bytes([4; 16]));
    }

    #[test]
    fn the_order_rank_is_distinct_for_every_member() {
        let object = ObjectId::from_bytes([3; 16]);
        let ranks = [
            Effect::Create {
                object,
                kind: ObjectKind::File,
                directory: object,
                name: name("a"),
            }
            .order_rank(),
            Effect::Reparent {
                object,
                directory: object,
            }
            .order_rank(),
            Effect::Rename {
                object,
                name: name("a"),
            }
            .order_rank(),
            Effect::WriteText {
                object,
                version: VersionId::from_bytes([1; 32]),
                lines: vec![],
            }
            .order_rank(),
            Effect::WriteBinary {
                object,
                version: VersionId::from_bytes([1; 32]),
                digest: [0; 32],
                byte_length: 0,
            }
            .order_rank(),
            Effect::Delete { object }.order_rank(),
        ];
        let mut sorted = ranks.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ranks.len());
    }
}
