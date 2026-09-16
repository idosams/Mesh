//! The authoritative diff: what changed between two states, computed and never declared.
//!
//! # The one sentence this module is built around
//!
//! **The actor does not decide what it changed.** [`diff`] takes two states and nothing else —
//! there is no parameter through which a caller can say what it touched, so there is nothing to
//! lie with and nothing to forget. An agent that rewrote a file it never mentioned is in the
//! bundle; an agent that claims a change it did not make is not.
//!
//! # Why the change list is keyed by object and not by path
//!
//! A path is derived from directory entries (`docs/protocol.md` SG-6). Keying a diff by path turns
//! one rename into a delete and a create of an unrelated file, which loses the reviewer's ability
//! to see that a version survived. So an object is followed through the two states by its
//! identifier, and a rename is reported as a rename. Paths are carried alongside as
//! [`ObjectChange::path_before`] and [`ObjectChange::path_after`], derived from the states rather
//! than stored, so a person reads a location while the machine reads an identity.
//!
//! # Determinism
//!
//! Objects are visited in identifier order and each object's effects are emitted in a fixed rank
//! order ([`Effect::rank`]). Nothing here iterates a hash map, consults a clock or reads the
//! environment, so the same pair of states produces the same list — same order, same bytes — in
//! every process.

use std::collections::BTreeSet;

use crate::digest::{Absorb, DigestHasher, DigestWriter};
use crate::ids::ObjectId;
use crate::name::NormalizedName;
use crate::state::{Content, ObjectKind, StateObject, WorkspaceState};

/// One thing that happened to one object between two states.
///
/// A single object can carry several: a file that was renamed, moved and rewritten produces three,
/// in [`Effect::rank`] order, because collapsing them would hide two of the three from the person
/// approving.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// The object exists in the later state and not in the earlier one.
    Created {
        /// Whether it holds content or other objects.
        kind: ObjectKind,
        /// The directory it hangs in.
        directory: ObjectId,
        /// The name it hangs under.
        name: NormalizedName,
        /// The version it holds, absent for a directory and for an empty file.
        content: Option<Content>,
    },
    /// The object exists in the earlier state and not in the later one.
    ///
    /// Carries what was there, so the reviewer sees what would stop being reachable and so the
    /// change list can be replayed in either direction.
    Removed {
        /// What it was.
        kind: ObjectKind,
        /// Where it hung.
        directory: ObjectId,
        /// What it was called.
        name: NormalizedName,
        /// The version it held.
        content: Option<Content>,
    },
    /// The object hangs in a different directory.
    Moved {
        /// The directory it hung in.
        from: ObjectId,
        /// The directory it hangs in now.
        to: ObjectId,
    },
    /// The object hangs under a different name in the same directory.
    Renamed {
        /// The name it hung under.
        from: NormalizedName,
        /// The name it hangs under now.
        to: NormalizedName,
    },
    /// The object holds a different version.
    ContentWritten {
        /// The version it held, absent when it held none.
        from: Option<Content>,
        /// The version it holds now.
        to: Content,
    },
    /// The object held a version and now holds none.
    ContentCleared {
        /// The version it held.
        from: Content,
    },
}

impl Effect {
    /// The order effects on one object are emitted in.
    ///
    /// Fixed, and part of the bundle's byte identity. Creation and removal come first because they
    /// bracket everything else; placement before content because a reviewer reads *where* before
    /// *what*.
    #[must_use]
    pub const fn rank(&self) -> u64 {
        match self {
            Self::Created { .. } => 0,
            Self::Removed { .. } => 1,
            Self::Moved { .. } => 2,
            Self::Renamed { .. } => 3,
            Self::ContentWritten { .. } => 4,
            Self::ContentCleared { .. } => 5,
        }
    }

    /// A short, stable word for this effect, for a surface that renders one.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Created { .. } => "created",
            Self::Removed { .. } => "removed",
            Self::Moved { .. } => "moved",
            Self::Renamed { .. } => "renamed",
            Self::ContentWritten { .. } => "content-written",
            Self::ContentCleared { .. } => "content-cleared",
        }
    }
}

impl Absorb for Effect {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.u64(self.rank());
        match self {
            Self::Created {
                kind,
                directory,
                name,
                content,
            } => {
                writer.u64(kind_tag(*kind));
                writer.bytes(directory.as_bytes());
                writer.text(name.as_str());
                writer.option(content.as_ref(), |writer, content| content.absorb(writer));
            }
            Self::Removed {
                kind,
                directory,
                name,
                content,
            } => {
                writer.u64(kind_tag(*kind));
                writer.bytes(directory.as_bytes());
                writer.text(name.as_str());
                writer.option(content.as_ref(), |writer, content| content.absorb(writer));
            }
            Self::Moved { from, to } => {
                writer.bytes(from.as_bytes());
                writer.bytes(to.as_bytes());
            }
            Self::Renamed { from, to } => {
                writer.text(from.as_str());
                writer.text(to.as_str());
            }
            Self::ContentWritten { from, to } => {
                writer.option(from.as_ref(), |writer, from| from.absorb(writer));
                to.absorb(writer);
            }
            Self::ContentCleared { from } => {
                from.absorb(writer);
            }
        }
    }
}

/// The byte an object kind is absorbed as. Mirrors the private tag in [`crate::state`].
const fn kind_tag(kind: ObjectKind) -> u64 {
    match kind {
        ObjectKind::File => 0,
        ObjectKind::Directory => 1,
    }
}

/// One effect, attributed to one object, with the two paths a person reads it by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectChange {
    object: ObjectId,
    path_before: Option<String>,
    path_after: Option<String>,
    effect: Effect,
}

impl ObjectChange {
    /// The object this happened to.
    #[must_use]
    pub const fn object(&self) -> ObjectId {
        self.object
    }

    /// Where the object was, derived from the earlier state. Absent when it did not exist there.
    #[must_use]
    pub fn path_before(&self) -> Option<&str> {
        self.path_before.as_deref()
    }

    /// Where the object is, derived from the later state. Absent when it does not exist there.
    #[must_use]
    pub fn path_after(&self) -> Option<&str> {
        self.path_after.as_deref()
    }

    /// What happened.
    #[must_use]
    pub const fn effect(&self) -> &Effect {
        &self.effect
    }
}

impl Absorb for ObjectChange {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.bytes(self.object.as_bytes());
        writer.option(self.path_before.as_ref(), |writer, path| {
            writer.text(path);
        });
        writer.option(self.path_after.as_ref(), |writer, path| {
            writer.text(path);
        });
        self.effect.absorb(writer);
    }
}

/// Why a diff could not be computed, so the bundle refuses instead of approximating.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffError {
    /// The two states disagree about which object is the root, so they are not two versions of one
    /// workspace and the difference between them is not a diff.
    RootDiffers {
        /// The earlier state's root.
        base: ObjectId,
        /// The later state's root.
        actor: ObjectId,
    },
    /// One object is a file in one state and a directory in the other. Nothing in the operation
    /// vocabulary produces that, so the states did not come from where they claim to have.
    KindChanged {
        /// The object.
        object: ObjectId,
    },
    /// A non-root object has no directory or no name, so it has no derivable path.
    Detached {
        /// The object.
        object: ObjectId,
    },
    /// An object's path could not be derived — an unknown ancestor, or a directory cycle.
    UnreachablePath {
        /// The object.
        object: ObjectId,
    },
}

impl core::fmt::Display for DiffError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RootDiffers { base, actor } => write!(
                formatter,
                "the two states have different roots ({base} and {actor}), so they are not two \
                 versions of one workspace"
            ),
            Self::KindChanged { object } => write!(
                formatter,
                "object {object} is a file in one state and a directory in the other"
            ),
            Self::Detached { object } => write!(
                formatter,
                "object {object} is not the root and has no directory entry, so it has no path"
            ),
            Self::UnreachablePath { object } => write!(
                formatter,
                "object {object} has no derivable path — an unknown ancestor, or a directory cycle"
            ),
        }
    }
}

impl std::error::Error for DiffError {}

/// Every difference between `base` and `actor`, in a fixed order.
///
/// # Errors
///
/// [`DiffError`] when the two states are not two versions of one workspace, or when a changed
/// object has no derivable path. Refusing is the contract: a bundle that approximated a location
/// would be approved for something other than what it says.
pub fn diff(base: &WorkspaceState, actor: &WorkspaceState) -> Result<Vec<ObjectChange>, DiffError> {
    if base.root() != actor.root() {
        return Err(DiffError::RootDiffers {
            base: base.root(),
            actor: actor.root(),
        });
    }

    let touched: BTreeSet<ObjectId> = base
        .objects()
        .map(|(object, _)| *object)
        .chain(actor.objects().map(|(object, _)| *object))
        .collect();

    let mut changes = Vec::new();
    for object in touched {
        let before = base.object(object);
        let after = actor.object(object);
        let effects = effects_for(object, before, after)?;
        if effects.is_empty() {
            continue;
        }
        let path_before = derive_path(base, object, before)?;
        let path_after = derive_path(actor, object, after)?;
        for effect in effects {
            changes.push(ObjectChange {
                object,
                path_before: path_before.clone(),
                path_after: path_after.clone(),
                effect,
            });
        }
    }
    Ok(changes)
}

/// The path of `object` in `state`, or `None` when the state does not hold it.
fn derive_path(
    state: &WorkspaceState,
    object: ObjectId,
    held: Option<&StateObject>,
) -> Result<Option<String>, DiffError> {
    let Some(held) = held else {
        return Ok(None);
    };
    if object != state.root() && (held.directory().is_none() || held.name().is_none()) {
        return Err(DiffError::Detached { object });
    }
    state
        .path_of(object)
        .map(Some)
        .ok_or(DiffError::UnreachablePath { object })
}

/// Every effect on one object, in rank order.
fn effects_for(
    object: ObjectId,
    before: Option<&StateObject>,
    after: Option<&StateObject>,
) -> Result<Vec<Effect>, DiffError> {
    match (before, after) {
        (None, None) => Ok(Vec::new()),
        (None, Some(after)) => {
            let (directory, name) = located(object, after)?;
            Ok(vec![Effect::Created {
                kind: after.kind(),
                directory,
                name,
                content: after.content().cloned(),
            }])
        }
        (Some(before), None) => {
            let (directory, name) = located(object, before)?;
            Ok(vec![Effect::Removed {
                kind: before.kind(),
                directory,
                name,
                content: before.content().cloned(),
            }])
        }
        (Some(before), Some(after)) => {
            if before.kind() != after.kind() {
                return Err(DiffError::KindChanged { object });
            }
            let mut effects = Vec::new();
            if let (Some(from), Some(to)) = (before.directory(), after.directory()) {
                if from != to {
                    effects.push(Effect::Moved { from, to });
                }
            }
            if let (Some(from), Some(to)) = (before.name(), after.name()) {
                if from != to {
                    effects.push(Effect::Renamed {
                        from: from.clone(),
                        to: to.clone(),
                    });
                }
            }
            match (before.content(), after.content()) {
                (from, Some(to)) if from != Some(to) => effects.push(Effect::ContentWritten {
                    from: from.cloned(),
                    to: to.clone(),
                }),
                (Some(from), None) => effects.push(Effect::ContentCleared { from: from.clone() }),
                _ => {}
            }
            Ok(effects)
        }
    }
}

/// The directory and name of a non-root object, or [`DiffError::Detached`].
fn located(object: ObjectId, held: &StateObject) -> Result<(ObjectId, NormalizedName), DiffError> {
    match (held.directory(), held.name()) {
        (Some(directory), Some(name)) => Ok((directory, name.clone())),
        _ => Err(DiffError::Detached { object }),
    }
}

/// Why a change list could not be replayed onto a state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplyError {
    /// A change names an object the state being built does not hold.
    Missing {
        /// The object.
        object: ObjectId,
    },
    /// A change would create an object the state already holds.
    AlreadyPresent {
        /// The object.
        object: ObjectId,
    },
}

impl core::fmt::Display for ApplyError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Missing { object } => {
                write!(
                    formatter,
                    "a change names object {object}, which is not present"
                )
            }
            Self::AlreadyPresent { object } => write!(
                formatter,
                "a change would create object {object}, which is already present"
            ),
        }
    }
}

impl std::error::Error for ApplyError {}

/// Replay `changes` onto `base`.
///
/// The inverse of [`diff`], and the property that makes the change list authoritative rather than
/// descriptive: `apply(base, diff(base, actor)) == actor`, for every pair of states a diff can be
/// computed between. `tests/bundle.rs` runs it over generated states.
///
/// # Errors
///
/// [`ApplyError`] when a change names an object the state does not hold, or creates one it already
/// holds — which is what a change list that did not come from this base looks like.
pub fn apply(
    base: &WorkspaceState,
    changes: &[ObjectChange],
) -> Result<WorkspaceState, ApplyError> {
    let mut state = base.clone();
    for change in changes {
        let object = change.object;
        state = match &change.effect {
            Effect::Created {
                kind,
                directory,
                name,
                content,
            } => {
                if state.object(object).is_some() {
                    return Err(ApplyError::AlreadyPresent { object });
                }
                state.with_record(
                    object,
                    StateObject::new(*kind, Some(*directory), Some(name.clone()), content.clone()),
                )
            }
            Effect::Removed { .. } => {
                if state.object(object).is_none() {
                    return Err(ApplyError::Missing { object });
                }
                state.without(object)
            }
            Effect::Moved { to, .. } => {
                let held = record(&state, object)?;
                state.with_record(
                    object,
                    StateObject::new(
                        held.kind(),
                        Some(*to),
                        held.name().cloned(),
                        held.content().cloned(),
                    ),
                )
            }
            Effect::Renamed { to, .. } => {
                let held = record(&state, object)?;
                state.with_record(
                    object,
                    StateObject::new(
                        held.kind(),
                        held.directory(),
                        Some(to.clone()),
                        held.content().cloned(),
                    ),
                )
            }
            Effect::ContentWritten { to, .. } => {
                let held = record(&state, object)?;
                state.with_record(
                    object,
                    StateObject::new(
                        held.kind(),
                        held.directory(),
                        held.name().cloned(),
                        Some(to.clone()),
                    ),
                )
            }
            Effect::ContentCleared { .. } => {
                let held = record(&state, object)?;
                state.with_record(
                    object,
                    StateObject::new(held.kind(), held.directory(), held.name().cloned(), None),
                )
            }
        };
    }
    Ok(state)
}

/// A clone of one object's record, or [`ApplyError::Missing`].
fn record(state: &WorkspaceState, object: ObjectId) -> Result<StateObject, ApplyError> {
    state
        .object(object)
        .cloned()
        .ok_or(ApplyError::Missing { object })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::VersionId;

    fn name(text: &str) -> NormalizedName {
        NormalizedName::new(text).unwrap()
    }

    fn text(byte: u8, lines: &[&str]) -> Content {
        Content::Text {
            version: VersionId::from_bytes([byte; 32]),
            lines: lines.iter().map(|line| (*line).to_owned()).collect(),
        }
    }

    fn root() -> ObjectId {
        ObjectId::from_bytes([0; 16])
    }

    fn base() -> WorkspaceState {
        WorkspaceState::new(root())
            .with_directory(ObjectId::from_bytes([2; 16]), root(), name("archive"))
            .with_file(
                ObjectId::from_bytes([1; 16]),
                root(),
                name("notes.md"),
                text(7, &["one"]),
            )
    }

    #[test]
    fn two_identical_states_differ_in_nothing() {
        assert_eq!(diff(&base(), &base()).unwrap(), Vec::new());
    }

    #[test]
    fn a_rename_is_a_rename_and_not_a_delete_and_a_create() {
        let notes = ObjectId::from_bytes([1; 16]);
        let after = base().with_file(notes, root(), name("journal.md"), text(7, &["one"]));
        let changes = diff(&base(), &after).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(
            changes[0].effect(),
            &Effect::Renamed {
                from: name("notes.md"),
                to: name("journal.md")
            }
        );
        assert_eq!(changes[0].path_before(), Some("/notes.md"));
        assert_eq!(changes[0].path_after(), Some("/journal.md"));
    }

    #[test]
    fn a_move_a_rename_and_a_write_are_three_effects_in_rank_order() {
        let notes = ObjectId::from_bytes([1; 16]);
        let archive = ObjectId::from_bytes([2; 16]);
        let after = base().with_file(notes, archive, name("journal.md"), text(8, &["two"]));
        let changes = diff(&base(), &after).unwrap();
        let ranks: Vec<u64> = changes
            .iter()
            .map(|change| change.effect().rank())
            .collect();
        assert_eq!(ranks, vec![2, 3, 4]);
        assert_eq!(changes[2].path_after(), Some("/archive/journal.md"));
    }

    #[test]
    fn a_removal_carries_what_was_there() {
        let notes = ObjectId::from_bytes([1; 16]);
        let changes = diff(&base(), &base().without(notes)).unwrap();
        assert_eq!(
            changes[0].effect(),
            &Effect::Removed {
                kind: ObjectKind::File,
                directory: root(),
                name: name("notes.md"),
                content: Some(text(7, &["one"])),
            }
        );
    }

    #[test]
    fn two_states_with_different_roots_are_refused() {
        let other = WorkspaceState::new(ObjectId::from_bytes([9; 16]));
        assert_eq!(
            diff(&base(), &other),
            Err(DiffError::RootDiffers {
                base: root(),
                actor: ObjectId::from_bytes([9; 16])
            })
        );
    }

    #[test]
    fn an_object_that_changed_kind_is_refused() {
        let notes = ObjectId::from_bytes([1; 16]);
        let after = base().with_directory(notes, root(), name("notes.md"));
        assert_eq!(
            diff(&base(), &after),
            Err(DiffError::KindChanged { object: notes })
        );
    }

    #[test]
    fn a_detached_object_is_refused() {
        let stray = ObjectId::from_bytes([5; 16]);
        let after = base().with_record(
            stray,
            StateObject::new(ObjectKind::File, None, None, Some(text(9, &["x"]))),
        );
        assert_eq!(
            diff(&base(), &after),
            Err(DiffError::Detached { object: stray })
        );
    }

    #[test]
    fn a_directory_cycle_is_refused_rather_than_approximated() {
        let first = ObjectId::from_bytes([6; 16]);
        let second = ObjectId::from_bytes([7; 16]);
        let after = base()
            .with_directory(first, second, name("first"))
            .with_directory(second, first, name("second"));
        assert_eq!(
            diff(&base(), &after),
            Err(DiffError::UnreachablePath { object: first })
        );
    }

    #[test]
    fn replaying_the_diff_onto_the_base_reproduces_the_later_state() {
        let notes = ObjectId::from_bytes([1; 16]);
        let archive = ObjectId::from_bytes([2; 16]);
        let extra = ObjectId::from_bytes([3; 16]);
        let after = base()
            .with_file(notes, archive, name("journal.md"), text(8, &["two"]))
            .with_file(extra, archive, name("new.md"), text(9, &["fresh"]));
        let changes = diff(&base(), &after).unwrap();
        assert_eq!(apply(&base(), &changes).unwrap(), after);
    }

    #[test]
    fn replaying_a_change_list_from_another_base_is_refused() {
        let notes = ObjectId::from_bytes([1; 16]);
        let changes = diff(&base(), &base().without(notes)).unwrap();
        let empty = WorkspaceState::new(root());
        assert_eq!(
            apply(&empty, &changes),
            Err(ApplyError::Missing { object: notes })
        );
    }

    #[test]
    fn every_effect_has_a_distinct_rank_and_a_label() {
        let effects = [
            Effect::Created {
                kind: ObjectKind::File,
                directory: root(),
                name: name("a"),
                content: None,
            },
            Effect::Removed {
                kind: ObjectKind::File,
                directory: root(),
                name: name("a"),
                content: None,
            },
            Effect::Moved {
                from: root(),
                to: root(),
            },
            Effect::Renamed {
                from: name("a"),
                to: name("b"),
            },
            Effect::ContentWritten {
                from: None,
                to: text(1, &[]),
            },
            Effect::ContentCleared { from: text(1, &[]) },
        ];
        let ranks: BTreeSet<u64> = effects.iter().map(Effect::rank).collect();
        assert_eq!(ranks.len(), effects.len());
        for effect in &effects {
            assert!(!effect.label().is_empty());
        }
    }
}
