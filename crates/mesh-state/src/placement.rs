//! Directory entries as the thing that changes when a rename or a move happens.
//!
//! # The one sentence this module exists for
//!
//! **A rename or a move rewrites a directory entry; it never touches the object.** Plan §2.6 and
//! `docs/protocol.md` SG-6 state it; the shape of [`IdentityChange`] is what makes it
//! unrepresentable to do otherwise. Not one member of this enumeration can produce an
//! [`ObjectId`] that did not already exist, and none of them names a descendant.
//!
//! # Why this is a projection rather than an import of `mesh-operations`
//!
//! `mesh-state` declares no dependency at all — see `src/ids.rs` — so it cannot import
//! `mesh-operations`' eighteen verbs. [`IdentityChange`] is the projection of the six of them that
//! touch identity, exactly as [`crate::DeliveredChangeSet`] is the projection of the ChangeSet
//! fields head advancement reads. `tests/operations_drift.rs` reads `mesh-operations`' own source
//! and fails if any of the six stops being declared there, or stops carrying the fields this
//! projection reads.
//!
//! # Why a change carries its resulting placement rather than a delta
//!
//! A change arriving out of order must produce the same register as one arriving in order. A
//! delta — "rename whatever is currently at this name" — cannot: applying it before its
//! predecessor and after its predecessor give different answers. So every member below states the
//! **absolute** resulting placement. The `from` fields are carried for the audit trail and for the
//! conflict engine, and are never consulted to decide what the change means.

use crate::ids::VersionId;
use crate::name::NormalizedName;
use crate::object::{ObjectId, ObjectKind};
use crate::stamp::Stamp;

/// Where an object's directory entry sits, or that it has none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Bound as `name` inside `directory`.
    Bound {
        /// The directory holding the entry.
        directory: ObjectId,
        /// The name it is bound under.
        name: NormalizedName,
    },
    /// Bound nowhere: minted but not yet linked, or unlinked.
    ///
    /// Not a deletion. The object, its identity and its whole history survive being unlinked,
    /// which is why an unlink followed by a link somewhere else is a move that kept its history
    /// rather than a delete-and-create that lost it.
    Detached,
}

impl Placement {
    /// The directory this placement binds into, if any.
    #[must_use]
    pub const fn directory(&self) -> Option<ObjectId> {
        match self {
            Self::Bound { directory, .. } => Some(*directory),
            Self::Detached => None,
        }
    }

    /// The name this placement binds under, if any.
    #[must_use]
    pub const fn name(&self) -> Option<&NormalizedName> {
        match self {
            Self::Bound { name, .. } => Some(name),
            Self::Detached => None,
        }
    }
}

/// One entry in an object's placement history: where it sat, and from when.
///
/// The history is append-only and held in stamp order, so an arbitrary chain of renames and moves
/// stays traversable in both directions. Nothing in this crate removes a record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacementRecord {
    stamp: Stamp,
    placement: Placement,
}

impl PlacementRecord {
    /// A placement, and the position in the total order it takes effect at.
    #[must_use]
    pub const fn new(stamp: Stamp, placement: Placement) -> Self {
        Self { stamp, placement }
    }

    /// Where in the total order this placement sits.
    #[must_use]
    pub const fn stamp(&self) -> Stamp {
        self.stamp
    }

    /// The placement.
    #[must_use]
    pub const fn placement(&self) -> &Placement {
        &self.placement
    }
}

/// One version an object took, and the position in the total order it took it at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionRecord {
    stamp: Stamp,
    version: VersionId,
}

impl VersionRecord {
    /// A version, and when it was written.
    #[must_use]
    pub const fn new(stamp: Stamp, version: VersionId) -> Self {
        Self { stamp, version }
    }

    /// Where in the total order this write sits.
    #[must_use]
    pub const fn stamp(&self) -> Stamp {
        self.stamp
    }

    /// The version.
    #[must_use]
    pub const fn version(&self) -> VersionId {
        self.version
    }
}

/// A change to object identity or to a directory entry.
///
/// The projection of the six members of plan §4.3's vocabulary that touch identity. See the module
/// header for why it is a projection and why each member states an absolute placement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityChange {
    /// Mint an object. The only member that introduces an [`ObjectId`], and it introduces one that
    /// does not exist yet — `mesh-operations`' `CreateFile` and `CreateDirectory`.
    Create {
        /// The object being minted.
        object: ObjectId,
        /// What it is.
        kind: ObjectKind,
    },
    /// Bind a name in a directory to an object — `mesh-operations`' `LinkDirectoryEntry`.
    Link {
        /// The object being bound.
        object: ObjectId,
        /// The directory it is bound into.
        directory: ObjectId,
        /// The name it takes.
        name: NormalizedName,
    },
    /// Remove a name binding — `mesh-operations`' `UnlinkDirectoryEntry`.
    ///
    /// The object survives with its identity and history intact; only the entry goes.
    Unlink {
        /// The object whose binding goes.
        object: ObjectId,
        /// The directory it was bound into.
        directory: ObjectId,
        /// The name it was bound under.
        name: NormalizedName,
    },
    /// Change an entry's name within one directory — `mesh-operations`' `RenameEntry`.
    Rename {
        /// The object whose entry is renamed. Unchanged by this change, which is the point.
        object: ObjectId,
        /// The directory the entry stays in.
        directory: ObjectId,
        /// The name it had, carried for the audit trail.
        from_name: NormalizedName,
        /// The name it takes.
        to_name: NormalizedName,
    },
    /// Move an entry between directories — `mesh-operations`' `MoveEntry`.
    ///
    /// **Constant cost in the size of the subtree.** Nothing under `object` is named and nothing
    /// under it can be: a directory entry binds a name to a child object identifier, so every
    /// descendant continues to hang off that object with no record of where its ancestor is
    /// linked.
    Move {
        /// The object whose entry moves — a file or the root of a subtree, identically.
        object: ObjectId,
        /// The directory it leaves, carried for the audit trail.
        from_directory: ObjectId,
        /// The name it had there, carried for the audit trail.
        from_name: NormalizedName,
        /// The directory it joins.
        to_directory: ObjectId,
        /// The name it takes there.
        to_name: NormalizedName,
    },
    /// Record a durable version of an object — `mesh-operations`' `WriteFileVersion`.
    ///
    /// The content facet. It names no directory and no name, which is why an edit and a rename of
    /// the same object are independent and both survive being concurrent.
    WriteVersion {
        /// The object.
        object: ObjectId,
        /// The version it takes.
        version: VersionId,
    },
}

impl IdentityChange {
    /// One byte of member tag, on every member.
    const TAG_BYTES: usize = 1;

    /// The object this change is about.
    ///
    /// Every member names exactly one, and never mints one except [`IdentityChange::Create`].
    #[must_use]
    pub const fn object(&self) -> ObjectId {
        match self {
            Self::Create { object, .. }
            | Self::Link { object, .. }
            | Self::Unlink { object, .. }
            | Self::Rename { object, .. }
            | Self::Move { object, .. }
            | Self::WriteVersion { object, .. } => *object,
        }
    }

    /// Whether this change mints an object identity.
    ///
    /// Exactly one member does. `tests/identity.rs` walks generated rename and move sequences and
    /// requires the object set to be unchanged by every member for which this is `false`.
    #[must_use]
    pub const fn mints_an_identity(&self) -> bool {
        matches!(self, Self::Create { .. })
    }

    /// The placement this change results in, if it is a directory-entry change.
    ///
    /// Absolute, never a delta — see the module header.
    #[must_use]
    pub fn resulting_placement(&self) -> Option<Placement> {
        match self {
            Self::Create { .. } | Self::WriteVersion { .. } => None,
            Self::Link {
                directory, name, ..
            } => Some(Placement::Bound {
                directory: *directory,
                name: name.clone(),
            }),
            Self::Unlink { .. } => Some(Placement::Detached),
            Self::Rename {
                directory, to_name, ..
            } => Some(Placement::Bound {
                directory: *directory,
                name: to_name.clone(),
            }),
            Self::Move {
                to_directory,
                to_name,
                ..
            } => Some(Placement::Bound {
                directory: *to_directory,
                name: to_name.clone(),
            }),
        }
    }

    /// How many bytes this change contributes to a record, beside its [`Stamp`].
    ///
    /// A **count**, not an encoding: the tag, the identifiers and the names, each at its declared
    /// width. The canonical encoding is `mesh-operations`', which measures its own bytes in
    /// `tests/subtree_move_is_constant_cost.rs`; this is what the register can state about cost
    /// without owning an encoder. The two agree on the property that matters — the count for
    /// [`IdentityChange::Move`] does not mention the subtree, so it cannot vary with it.
    #[must_use]
    pub fn metadata_byte_count(&self) -> usize {
        let width = ObjectId::BYTE_WIDTH;
        Self::TAG_BYTES
            + match self {
                Self::Create { .. } => width + 1,
                Self::Link { name, .. } | Self::Unlink { name, .. } => {
                    width + width + name.byte_len()
                }
                Self::Rename {
                    from_name, to_name, ..
                } => width + width + from_name.byte_len() + to_name.byte_len(),
                Self::Move {
                    from_name, to_name, ..
                } => width + width + width + from_name.byte_len() + to_name.byte_len(),
                Self::WriteVersion { .. } => width + VersionId::BYTE_WIDTH,
            }
    }
}

/// What applying a change did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityOutcome {
    /// Applied, and it moved the register.
    Applied {
        /// How many directory entries were bound or unbound. A move is always two — one unbound,
        /// one bound — whatever is under it.
        entries_touched: usize,
        /// How many bytes the change and its stamp occupy.
        metadata_bytes: usize,
    },
    /// Recorded in the object's history, but a change already applied at a higher stamp still
    /// holds the current placement or version.
    ///
    /// Convergent, not an error: it is what a change arriving late looks like, and the register it
    /// leaves behind is the same one in-order delivery would have produced.
    Superseded {
        /// How many bytes the change and its stamp occupy.
        metadata_bytes: usize,
    },
    /// This exact stamp is already in this object's history. Reapplying changes nothing.
    AlreadyApplied,
    /// Refused, with the reason.
    Refused(IdentityRefusal),
}

/// Why a change was refused.
///
/// Each of these is a statement the register cannot make true without inventing something. None of
/// them is a conflict: two actors disagreeing is resolved by the stamp order, never refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityRefusal {
    /// The change names an object the register has never seen minted.
    UnknownObject(ObjectId),
    /// The change binds into a directory the register has never seen minted.
    UnknownDirectory(ObjectId),
    /// The change binds into an object that is not a directory.
    NotADirectory(ObjectId),
    /// The change mints an object identity that already exists.
    AlreadyMinted(ObjectId),
    /// The change would place a directory inside its own subtree, so neither would have a path.
    WouldContainItself {
        /// The object being placed.
        object: ObjectId,
        /// The directory it would be placed in, which is beneath it.
        directory: ObjectId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> NormalizedName {
        NormalizedName::new(text).unwrap()
    }

    fn object(byte: u8) -> ObjectId {
        ObjectId::from_bytes([byte; 16])
    }

    #[test]
    fn exactly_one_member_mints_an_identity() {
        let members = [
            IdentityChange::Create {
                object: object(1),
                kind: ObjectKind::File,
            },
            IdentityChange::Link {
                object: object(1),
                directory: object(2),
                name: name("a"),
            },
            IdentityChange::Unlink {
                object: object(1),
                directory: object(2),
                name: name("a"),
            },
            IdentityChange::Rename {
                object: object(1),
                directory: object(2),
                from_name: name("a"),
                to_name: name("b"),
            },
            IdentityChange::Move {
                object: object(1),
                from_directory: object(2),
                from_name: name("a"),
                to_directory: object(3),
                to_name: name("b"),
            },
            IdentityChange::WriteVersion {
                object: object(1),
                version: VersionId::from_bytes([9; 32]),
            },
        ];
        let minting = members
            .iter()
            .filter(|change| change.mints_an_identity())
            .count();
        assert_eq!(minting, 1);
        for change in &members {
            assert_eq!(change.object(), object(1));
        }
    }

    /// The budget's metadata half, at the level this crate can state it: the count for a move is a
    /// function of two names and three identifiers and of nothing else, so no subtree can enter it.
    #[test]
    fn a_move_costs_the_same_whatever_it_is_named_over() {
        let one = IdentityChange::Move {
            object: object(1),
            from_directory: object(2),
            from_name: name("src"),
            to_directory: object(3),
            to_name: name("lib"),
        };
        let other = IdentityChange::Move {
            object: object(9),
            from_directory: object(8),
            from_name: name("src"),
            to_directory: object(7),
            to_name: name("lib"),
        };
        assert_eq!(one.metadata_byte_count(), other.metadata_byte_count());
        assert_eq!(one.metadata_byte_count(), 1 + 48 + 3 + 3);
    }

    #[test]
    fn a_directory_entry_change_states_an_absolute_placement() {
        let moved = IdentityChange::Move {
            object: object(1),
            from_directory: object(2),
            from_name: name("a"),
            to_directory: object(3),
            to_name: name("b"),
        };
        assert_eq!(
            moved.resulting_placement(),
            Some(Placement::Bound {
                directory: object(3),
                name: name("b"),
            })
        );
        assert_eq!(
            IdentityChange::Unlink {
                object: object(1),
                directory: object(2),
                name: name("a"),
            }
            .resulting_placement(),
            Some(Placement::Detached)
        );
        assert_eq!(
            IdentityChange::WriteVersion {
                object: object(1),
                version: VersionId::from_bytes([0; 32]),
            }
            .resulting_placement(),
            None
        );
    }

    #[test]
    fn a_detached_placement_names_nothing() {
        assert_eq!(Placement::Detached.directory(), None);
        assert_eq!(Placement::Detached.name(), None);
        let bound = Placement::Bound {
            directory: object(4),
            name: name("notes.md"),
        };
        assert_eq!(bound.directory(), Some(object(4)));
        assert_eq!(bound.name(), Some(&name("notes.md")));
    }
}
