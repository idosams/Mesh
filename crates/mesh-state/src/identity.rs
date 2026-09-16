//! The object identity register: what a rename and a move are allowed to change, and what they are
//! not.
//!
//! # One sentence this module is built around
//!
//! **An object's identity survives every path it has ever had.** A path is not stored anywhere here
//! — it is *derived*, by walking an object's directory entry up to the root — so moving a directory
//! changes the path of everything beneath it without any record beneath it changing at all.
//!
//! # Why that is the whole subtree-move budget
//!
//! Plan §11 publishes a budget: **one million descendants moved in under 100 ms, under 10 KiB of
//! metadata.** A register that stored a path per object would have to rewrite one million records
//! to satisfy a single move, and no implementation effort would recover the budget. Storing the
//! *entry* instead makes a move two map operations and one appended history record — a cost that
//! cannot vary with the subtree, because nothing in the write path can even name a descendant.
//! `tests/identity.rs` measures it at five subtree sizes up to a million and prints the numbers.
//!
//! # What convergence this register does and does not claim
//!
//! Every facet of an object is a last-writer-wins register keyed on [`Stamp`], whose order is
//! `lamport → event ULID → content hash` and never a clock. Placement and content are **separate**
//! facets, which is why a rename concurrent with an edit yields one object carrying both changes
//! rather than one of them winning.
//!
//! A change that arrives after a higher-stamped change to the same facet is still recorded — it
//! lands in the history and reports [`IdentityOutcome::Superseded`] — so delivery order does not
//! change the register that results. `tests/identity.rs` shuffles generated histories across
//! several replicas and requires one register.
//!
//! **The stated ceiling.** One case is not convergent and is not claimed to be: two concurrent
//! moves that would place each of two directories inside the other. Whichever arrives second is
//! refused with [`IdentityRefusal::WouldContainItself`], and which one that is depends on delivery
//! order, so two replicas can refuse different halves of the cycle. Making that case converge needs
//! the undo-and-replay construction from Kleppmann et al.'s highly-available move operation, which
//! is the conflict engine's to build and not this register's. It is written down here rather than
//! left to be discovered, and `tests/identity.rs` pins the current behaviour so the day it changes
//! is a visible one.

use std::collections::BTreeMap;

use crate::ids::VersionId;
use crate::name::{NormalizedName, WorkspacePath};
use crate::object::{ObjectId, ObjectKind};
use crate::placement::{
    IdentityChange, IdentityOutcome, IdentityRefusal, Placement, PlacementRecord, VersionRecord,
};
use crate::stamp::Stamp;

/// How deep a path may be before resolution gives up.
///
/// [`ObjectRegister::apply`] refuses a placement that would put a directory inside its own subtree,
/// so a cycle is not reachable through the public API. This bound is what keeps a register built
/// some other way — a decoder handed a hostile snapshot, a future member of the vocabulary — from
/// turning path resolution into a hang.
pub const MAX_PATH_DEPTH: usize = 4096;

/// Who currently claims one name inside one directory.
///
/// `Sole` is the ordinary case and allocates nothing, which matters: there is one of these per
/// directory entry, and a workspace has as many directory entries as it has files.
#[derive(Clone, Debug, PartialEq, Eq)]
enum NameClaim {
    /// Exactly one object claims the name.
    Sole(ObjectId),
    /// Several objects claim it concurrently, highest stamp first. **Nobody is evicted**: a name
    /// contest never destroys an identity, it only decides which claimant the name resolves to
    /// until `ResolveNameConflict` settles it.
    Contested(Vec<ObjectId>),
}

impl NameClaim {
    /// The object the name resolves to.
    fn holder(&self) -> Option<ObjectId> {
        match self {
            Self::Sole(object) => Some(*object),
            Self::Contested(claimants) => claimants.first().copied(),
        }
    }

    /// Every claimant, highest stamp first.
    fn claimants(&self) -> Vec<ObjectId> {
        match self {
            Self::Sole(object) => vec![*object],
            Self::Contested(claimants) => claimants.clone(),
        }
    }
}

/// Everything the register knows about one object.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ObjectRecord {
    kind: ObjectKind,
    minted: Stamp,
    /// Append-only, ascending by stamp. The last is current.
    placements: Vec<PlacementRecord>,
    /// Append-only, ascending by stamp. The last is current.
    versions: Vec<VersionRecord>,
}

impl ObjectRecord {
    fn current_placement(&self) -> Option<&PlacementRecord> {
        self.placements.last()
    }

    /// The placement in force at `stamp`, which is the last one stamped at or before it.
    fn placement_at(&self, stamp: Stamp) -> Option<&PlacementRecord> {
        let at = self
            .placements
            .partition_point(|record| record.stamp() <= stamp);
        at.checked_sub(1).map(|index| &self.placements[index])
    }
}

/// The register of object identities and the directory entries that name them.
///
/// Immutable in the crate's established style: [`ObjectRegister::apply`] takes `self` by value and
/// returns the next register, so a caller cannot hold a stale one. It moves rather than clones, so
/// the cost is a handful of pointers whatever the register holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectRegister {
    root: ObjectId,
    /// Ordered rather than hashed on purpose: a `HashMap`'s default hasher draws a random seed from
    /// the operating system, and this crate's claim is that its answers come from the applied
    /// causal set and from nothing else. An ordered map also gives every iteration a defined order,
    /// so two replicas enumerate a register identically.
    objects: BTreeMap<ObjectId, ObjectRecord>,
    children: BTreeMap<ObjectId, BTreeMap<NormalizedName, NameClaim>>,
}

impl ObjectRegister {
    /// A register holding nothing but the root directory.
    ///
    /// The root is minted here rather than through a change, because a change naming the directory
    /// it binds into has nowhere to start.
    #[must_use]
    pub fn new(root: ObjectId, minted: Stamp) -> Self {
        let mut objects = BTreeMap::new();
        objects.insert(
            root,
            ObjectRecord {
                kind: ObjectKind::Directory,
                minted,
                placements: Vec::new(),
                versions: Vec::new(),
            },
        );
        let mut children = BTreeMap::new();
        children.insert(root, BTreeMap::new());
        Self {
            root,
            objects,
            children,
        }
    }

    /// The root directory.
    #[must_use]
    pub const fn root(&self) -> ObjectId {
        self.root
    }

    /// How many objects the register holds, including the root.
    #[must_use]
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Every object, in identifier order.
    pub fn object_ids(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.objects.keys().copied()
    }

    /// Whether the register has seen this object minted.
    #[must_use]
    pub fn contains(&self, object: ObjectId) -> bool {
        self.objects.contains_key(&object)
    }

    /// What an object is.
    #[must_use]
    pub fn kind_of(&self, object: ObjectId) -> Option<ObjectKind> {
        self.objects.get(&object).map(|record| record.kind)
    }

    /// Where an object's directory entry currently sits.
    ///
    /// `None` for an object the register has never seen; [`Placement::Detached`] for one that has
    /// been minted but never linked, or unlinked since.
    #[must_use]
    pub fn placement_of(&self, object: ObjectId) -> Option<&Placement> {
        self.objects
            .get(&object)?
            .current_placement()
            .map(PlacementRecord::placement)
    }

    /// Every placement an object has ever had, in stamp order.
    ///
    /// This is what makes an arbitrary chain of renames and moves traversable: nothing is ever
    /// removed from it, so the whole chain is reachable from either end.
    #[must_use]
    pub fn placement_history(&self, object: ObjectId) -> &[PlacementRecord] {
        self.objects
            .get(&object)
            .map_or(&[], |record| record.placements.as_slice())
    }

    /// The version an object currently holds.
    #[must_use]
    pub fn version_of(&self, object: ObjectId) -> Option<VersionId> {
        self.objects
            .get(&object)?
            .versions
            .last()
            .map(VersionRecord::version)
    }

    /// Every version an object has held, in stamp order.
    #[must_use]
    pub fn version_history(&self, object: ObjectId) -> &[VersionRecord] {
        self.objects
            .get(&object)
            .map_or(&[], |record| record.versions.as_slice())
    }

    /// The names a directory currently binds, in name order, with the object each resolves to.
    pub fn entries_of(
        &self,
        directory: ObjectId,
    ) -> impl Iterator<Item = (&NormalizedName, ObjectId)> + '_ {
        self.children
            .get(&directory)
            .into_iter()
            .flat_map(|entries| {
                entries
                    .iter()
                    .filter_map(|(name, claim)| claim.holder().map(|object| (name, object)))
            })
    }

    /// How many names a directory currently binds.
    #[must_use]
    pub fn child_count(&self, directory: ObjectId) -> usize {
        self.children.get(&directory).map_or(0, BTreeMap::len)
    }

    /// Every object concurrently claiming one name, highest stamp first.
    ///
    /// One in the ordinary case. More than one means two actors bound the same name concurrently,
    /// and **both objects are intact** — the register resolves the name to the first and keeps the
    /// rest reachable by identity, which is what lets `ResolveNameConflict` preserve every
    /// contender instead of inventing one.
    #[must_use]
    pub fn contenders_for(&self, directory: ObjectId, name: &NormalizedName) -> Vec<ObjectId> {
        self.children
            .get(&directory)
            .and_then(|entries| entries.get(name))
            .map(NameClaim::claimants)
            .unwrap_or_default()
    }

    /// The path an object currently has, derived by walking its entry up to the root.
    ///
    /// `None` when the object is unknown, when it is detached, or when its chain does not reach the
    /// root. Deriving rather than storing is the whole design: a moved ancestor changes this answer
    /// for every descendant without any descendant record being touched.
    #[must_use]
    pub fn path_of(&self, object: ObjectId) -> Option<WorkspacePath> {
        self.walk_up(object, |record| record.current_placement())
    }

    /// The path an object had at a position in the total order.
    ///
    /// Every ancestor is read at the placement in force at `stamp`, so this answers for a
    /// descendant that was never named by any rename or move — the historical path of a file whose
    /// grandparent directory was renamed twice is derived, not looked up, and no record was written
    /// for it at the time.
    #[must_use]
    pub fn path_at(&self, object: ObjectId, stamp: Stamp) -> Option<WorkspacePath> {
        self.walk_up(object, |record| record.placement_at(stamp))
    }

    /// Walk from an object to the root, collecting names by whatever placement `select` picks.
    fn walk_up(
        &self,
        object: ObjectId,
        select: impl Fn(&ObjectRecord) -> Option<&PlacementRecord>,
    ) -> Option<WorkspacePath> {
        if object == self.root {
            return Some(WorkspacePath::new(Vec::new()));
        }
        let mut segments = Vec::new();
        let mut current = object;
        for _ in 0..MAX_PATH_DEPTH {
            let record = self.objects.get(&current)?;
            let placement = select(record)?.placement();
            let Placement::Bound { directory, name } = placement else {
                return None;
            };
            segments.push(name.clone());
            if *directory == self.root {
                segments.reverse();
                return Some(WorkspacePath::new(segments));
            }
            current = *directory;
        }
        None
    }

    /// Whether `directory` is `object` or sits beneath it, by current placement.
    fn is_within(&self, directory: ObjectId, object: ObjectId) -> bool {
        let mut current = directory;
        for _ in 0..MAX_PATH_DEPTH {
            if current == object {
                return true;
            }
            let Some(record) = self.objects.get(&current) else {
                return false;
            };
            match record.current_placement().map(PlacementRecord::placement) {
                Some(Placement::Bound { directory, .. }) => current = *directory,
                _ => return false,
            }
        }
        true
    }

    /// Apply one change at one position in the total order.
    ///
    /// Returns the next register and what the change did. A change is never dropped: one that is
    /// superseded by a higher stamp still lands in the object's history, which is what makes the
    /// resulting register independent of delivery order.
    #[must_use]
    pub fn apply(self, change: &IdentityChange, stamp: Stamp) -> (Self, IdentityOutcome) {
        let metadata_bytes = change.metadata_byte_count() + Stamp::BYTE_WIDTH;
        match change {
            IdentityChange::Create { object, kind } => self.mint(*object, *kind, stamp),
            IdentityChange::WriteVersion { object, version } => {
                self.write_version(*object, *version, stamp, metadata_bytes)
            }
            _ => self.place(change, stamp, metadata_bytes),
        }
    }

    /// Mint an object identity. The only path that introduces one.
    fn mint(mut self, object: ObjectId, kind: ObjectKind, stamp: Stamp) -> (Self, IdentityOutcome) {
        if self.objects.contains_key(&object) {
            return (
                self,
                IdentityOutcome::Refused(IdentityRefusal::AlreadyMinted(object)),
            );
        }
        self.objects.insert(
            object,
            ObjectRecord {
                kind,
                minted: stamp,
                placements: Vec::new(),
                versions: Vec::new(),
            },
        );
        if kind.holds_entries() {
            self.children.insert(object, BTreeMap::new());
        }
        let metadata_bytes = 1 + ObjectId::BYTE_WIDTH + 1 + Stamp::BYTE_WIDTH;
        (
            self,
            IdentityOutcome::Applied {
                entries_touched: 0,
                metadata_bytes,
            },
        )
    }

    /// Record a version. The content facet, which no rename or move touches.
    fn write_version(
        mut self,
        object: ObjectId,
        version: VersionId,
        stamp: Stamp,
        metadata_bytes: usize,
    ) -> (Self, IdentityOutcome) {
        if !self.objects.contains_key(&object) {
            return (
                self,
                IdentityOutcome::Refused(IdentityRefusal::UnknownObject(object)),
            );
        }
        let outcome = {
            let record = self
                .objects
                .get_mut(&object)
                .expect("the object was present one line above");
            let at = record
                .versions
                .partition_point(|written| written.stamp() <= stamp);
            if at > 0 && record.versions[at - 1].stamp() == stamp {
                IdentityOutcome::AlreadyApplied
            } else {
                let becomes_current = at == record.versions.len();
                record
                    .versions
                    .insert(at, VersionRecord::new(stamp, version));
                if becomes_current {
                    IdentityOutcome::Applied {
                        entries_touched: 0,
                        metadata_bytes,
                    }
                } else {
                    IdentityOutcome::Superseded { metadata_bytes }
                }
            }
        };
        (self, outcome)
    }

    /// Apply a directory-entry change: link, unlink, rename or move, which differ only in the
    /// placement they result in.
    fn place(
        mut self,
        change: &IdentityChange,
        stamp: Stamp,
        metadata_bytes: usize,
    ) -> (Self, IdentityOutcome) {
        let object = change.object();
        let Some(placement) = change.resulting_placement() else {
            return (
                self,
                IdentityOutcome::Refused(IdentityRefusal::UnknownObject(object)),
            );
        };
        if let Some(refusal) = self.refuse_placement(object, &placement) {
            return (self, IdentityOutcome::Refused(refusal));
        }

        let previous = {
            let record = self
                .objects
                .get_mut(&object)
                .expect("refuse_placement already established that the object is present");
            let at = record
                .placements
                .partition_point(|held| held.stamp() <= stamp);
            if at > 0 && record.placements[at - 1].stamp() == stamp {
                return (self, IdentityOutcome::AlreadyApplied);
            }
            if at != record.placements.len() {
                record
                    .placements
                    .insert(at, PlacementRecord::new(stamp, placement));
                return (self, IdentityOutcome::Superseded { metadata_bytes });
            }
            let previous = record
                .current_placement()
                .map(|current| current.placement().clone());
            record
                .placements
                .push(PlacementRecord::new(stamp, placement.clone()));
            previous
        };

        let mut entries_touched = 0;
        if let Some(Placement::Bound { directory, name }) = previous {
            self.unbind(directory, &name, object);
            entries_touched += 1;
        }
        if let Placement::Bound { directory, name } = placement {
            self.bind(directory, name, object);
            entries_touched += 1;
        }
        (
            self,
            IdentityOutcome::Applied {
                entries_touched,
                metadata_bytes,
            },
        )
    }

    /// The structural reasons a placement cannot be made true.
    ///
    /// The cycle check runs against the register as it stands, which is the ceiling the module
    /// header states: two concurrent moves forming a cycle refuse whichever arrives second.
    fn refuse_placement(&self, object: ObjectId, placement: &Placement) -> Option<IdentityRefusal> {
        if !self.objects.contains_key(&object) {
            return Some(IdentityRefusal::UnknownObject(object));
        }
        let Placement::Bound { directory, .. } = placement else {
            return None;
        };
        let Some(parent) = self.objects.get(directory) else {
            return Some(IdentityRefusal::UnknownDirectory(*directory));
        };
        if !parent.kind.holds_entries() {
            return Some(IdentityRefusal::NotADirectory(*directory));
        }
        if self.is_within(*directory, object) {
            return Some(IdentityRefusal::WouldContainItself {
                object,
                directory: *directory,
            });
        }
        None
    }

    /// Bind a name to an object, keeping every concurrent claimant of that name.
    ///
    /// The object's new placement is already in its history when this runs, so a claimant's stamp
    /// is read from the register rather than passed alongside.
    fn bind(&mut self, directory: ObjectId, name: NormalizedName, object: ObjectId) {
        let Self {
            objects, children, ..
        } = self;
        let Some(entries) = children.get_mut(&directory) else {
            return;
        };
        let Some(mut claimants) = entries.get(&name).map(NameClaim::claimants) else {
            entries.insert(name, NameClaim::Sole(object));
            return;
        };
        claimants.retain(|claimant| *claimant != object);
        claimants.push(object);
        let claim = if claimants.len() == 1 {
            NameClaim::Sole(object)
        } else {
            sort_by_stamp(&mut claimants, objects);
            NameClaim::Contested(claimants)
        };
        entries.insert(name, claim);
    }

    /// Remove one object's claim on a name, promoting whatever else still claims it.
    fn unbind(&mut self, directory: ObjectId, name: &NormalizedName, object: ObjectId) {
        let Some(entries) = self.children.get_mut(&directory) else {
            return;
        };
        let Some(claim) = entries.get(name) else {
            return;
        };
        let mut claimants = claim.claimants();
        claimants.retain(|claimant| *claimant != object);
        match claimants.len() {
            0 => {
                entries.remove(name);
            }
            1 => {
                entries.insert(name.clone(), NameClaim::Sole(claimants[0]));
            }
            _ => {
                entries.insert(name.clone(), NameClaim::Contested(claimants));
            }
        }
    }
}

/// Order claimants highest stamp first, with the identifier as the final tiebreak.
///
/// The order is total and reads no clock, so two replicas holding the same contest resolve the name
/// to the same object.
fn sort_by_stamp(claimants: &mut [ObjectId], objects: &BTreeMap<ObjectId, ObjectRecord>) {
    let stamp_of = |object: &ObjectId| -> Option<Stamp> {
        objects
            .get(object)
            .and_then(ObjectRecord::current_placement)
            .map(PlacementRecord::stamp)
    };
    claimants.sort_by(|left, right| {
        stamp_of(right)
            .cmp(&stamp_of(left))
            .then_with(|| left.cmp(right))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stamp::{EventId, Lamport};

    fn stamp(lamport: u64) -> Stamp {
        Stamp::new(
            Lamport::new(lamport),
            EventId::from_bytes([lamport as u8; 16]),
            [0; 32],
        )
    }

    fn object(byte: u8) -> ObjectId {
        ObjectId::from_bytes([byte; 16])
    }

    fn name(text: &str) -> NormalizedName {
        NormalizedName::new(text).unwrap()
    }

    /// A register with the root, one directory `src` and one file `main.rs` inside it.
    fn small() -> ObjectRegister {
        let root = object(0);
        let mut register = ObjectRegister::new(root, stamp(0));
        for (change, at) in [
            (
                IdentityChange::Create {
                    object: object(1),
                    kind: ObjectKind::Directory,
                },
                1,
            ),
            (
                IdentityChange::Link {
                    object: object(1),
                    directory: root,
                    name: name("src"),
                },
                2,
            ),
            (
                IdentityChange::Create {
                    object: object(2),
                    kind: ObjectKind::File,
                },
                3,
            ),
            (
                IdentityChange::Link {
                    object: object(2),
                    directory: object(1),
                    name: name("main.rs"),
                },
                4,
            ),
        ] {
            let (next, outcome) = register.apply(&change, stamp(at));
            assert!(
                matches!(outcome, IdentityOutcome::Applied { .. }),
                "{outcome:?}"
            );
            register = next;
        }
        register
    }

    #[test]
    fn a_rename_keeps_the_identity_and_changes_only_the_path() {
        let register = small();
        let before = register.path_of(object(2)).unwrap().to_string();
        assert_eq!(before, "/src/main.rs");

        let (register, outcome) = register.apply(
            &IdentityChange::Rename {
                object: object(2),
                directory: object(1),
                from_name: name("main.rs"),
                to_name: name("entry.rs"),
            },
            stamp(5),
        );
        assert_eq!(
            outcome,
            IdentityOutcome::Applied {
                entries_touched: 2,
                metadata_bytes: 1 + 32 + 7 + 8 + 56,
            }
        );
        assert!(register.contains(object(2)));
        assert_eq!(
            register.path_of(object(2)).unwrap().to_string(),
            "/src/entry.rs"
        );
        assert_eq!(register.placement_history(object(2)).len(), 2);
    }

    #[test]
    fn a_move_of_a_directory_never_names_a_descendant() {
        let register = small();
        let root = register.root();
        let (register, _) = register.apply(
            &IdentityChange::Create {
                object: object(3),
                kind: ObjectKind::Directory,
            },
            stamp(5),
        );
        let (register, _) = register.apply(
            &IdentityChange::Link {
                object: object(3),
                directory: root,
                name: name("lib"),
            },
            stamp(6),
        );
        let (register, outcome) = register.apply(
            &IdentityChange::Move {
                object: object(1),
                from_directory: root,
                from_name: name("src"),
                to_directory: object(3),
                to_name: name("core"),
            },
            stamp(7),
        );
        let IdentityOutcome::Applied {
            entries_touched, ..
        } = outcome
        else {
            panic!("{outcome:?}");
        };
        assert_eq!(entries_touched, 2);
        // The descendant was never named and its record never changed.
        assert_eq!(register.placement_history(object(2)).len(), 1);
        assert_eq!(
            register.path_of(object(2)).unwrap().to_string(),
            "/lib/core/main.rs"
        );
    }

    #[test]
    fn a_directory_cannot_be_placed_inside_itself() {
        let register = small();
        let (register, outcome) = register.apply(
            &IdentityChange::Create {
                object: object(4),
                kind: ObjectKind::Directory,
            },
            stamp(5),
        );
        let (register, _) = register.apply(
            &IdentityChange::Link {
                object: object(4),
                directory: object(1),
                name: name("inner"),
            },
            stamp(6),
        );
        assert!(matches!(outcome, IdentityOutcome::Applied { .. }));

        let root = register.root();
        let (register, outcome) = register.apply(
            &IdentityChange::Move {
                object: object(1),
                from_directory: root,
                from_name: name("src"),
                to_directory: object(4),
                to_name: name("src"),
            },
            stamp(7),
        );
        assert_eq!(
            outcome,
            IdentityOutcome::Refused(IdentityRefusal::WouldContainItself {
                object: object(1),
                directory: object(4),
            })
        );
        assert_eq!(register.path_of(object(1)).unwrap().to_string(), "/src");
    }

    #[test]
    fn an_unknown_object_and_an_unknown_directory_are_refused_separately() {
        let register = small();
        let (register, outcome) = register.apply(
            &IdentityChange::Link {
                object: object(9),
                directory: object(1),
                name: name("ghost"),
            },
            stamp(5),
        );
        assert_eq!(
            outcome,
            IdentityOutcome::Refused(IdentityRefusal::UnknownObject(object(9)))
        );
        let (register, outcome) = register.apply(
            &IdentityChange::Link {
                object: object(2),
                directory: object(9),
                name: name("ghost"),
            },
            stamp(5),
        );
        assert_eq!(
            outcome,
            IdentityOutcome::Refused(IdentityRefusal::UnknownDirectory(object(9)))
        );
        let (_, outcome) = register.apply(
            &IdentityChange::Link {
                object: object(1),
                directory: object(2),
                name: name("under-a-file"),
            },
            stamp(5),
        );
        assert_eq!(
            outcome,
            IdentityOutcome::Refused(IdentityRefusal::NotADirectory(object(2)))
        );
    }

    #[test]
    fn minting_an_identity_twice_is_refused() {
        let register = small();
        let (_, outcome) = register.apply(
            &IdentityChange::Create {
                object: object(1),
                kind: ObjectKind::Directory,
            },
            stamp(9),
        );
        assert_eq!(
            outcome,
            IdentityOutcome::Refused(IdentityRefusal::AlreadyMinted(object(1)))
        );
    }

    #[test]
    fn the_same_stamp_applied_twice_changes_nothing() {
        let register = small();
        let change = IdentityChange::Rename {
            object: object(2),
            directory: object(1),
            from_name: name("main.rs"),
            to_name: name("entry.rs"),
        };
        let (register, first) = register.apply(&change, stamp(5));
        assert!(matches!(first, IdentityOutcome::Applied { .. }));
        let before = register.clone();
        let (register, second) = register.apply(&change, stamp(5));
        assert_eq!(second, IdentityOutcome::AlreadyApplied);
        assert_eq!(register, before);
    }

    #[test]
    fn a_late_change_lands_in_history_without_moving_the_current_placement() {
        let register = small();
        let (register, _) = register.apply(
            &IdentityChange::Rename {
                object: object(2),
                directory: object(1),
                from_name: name("main.rs"),
                to_name: name("late.rs"),
            },
            stamp(9),
        );
        let (register, outcome) = register.apply(
            &IdentityChange::Rename {
                object: object(2),
                directory: object(1),
                from_name: name("main.rs"),
                to_name: name("early.rs"),
            },
            stamp(6),
        );
        assert!(matches!(outcome, IdentityOutcome::Superseded { .. }));
        assert_eq!(
            register.path_of(object(2)).unwrap().to_string(),
            "/src/late.rs"
        );
        assert_eq!(register.placement_history(object(2)).len(), 3);
    }

    #[test]
    fn two_objects_can_claim_one_name_and_both_survive() {
        let register = small();
        let (register, _) = register.apply(
            &IdentityChange::Create {
                object: object(5),
                kind: ObjectKind::File,
            },
            stamp(5),
        );
        let (register, outcome) = register.apply(
            &IdentityChange::Link {
                object: object(5),
                directory: object(1),
                name: name("main.rs"),
            },
            stamp(6),
        );
        assert!(matches!(outcome, IdentityOutcome::Applied { .. }));
        let contenders = register.contenders_for(object(1), &name("main.rs"));
        assert_eq!(contenders, vec![object(5), object(2)]);
        // Neither identity is gone, and the loser still has its placement history.
        assert!(register.contains(object(2)));
        assert_eq!(register.placement_history(object(2)).len(), 1);
        assert_eq!(register.child_count(object(1)), 1);
    }

    #[test]
    fn the_root_has_a_path_and_a_detached_object_has_none() {
        let register = small();
        assert_eq!(register.path_of(register.root()).unwrap().to_string(), "/");
        let (register, _) = register.apply(
            &IdentityChange::Unlink {
                object: object(2),
                directory: object(1),
                name: name("main.rs"),
            },
            stamp(5),
        );
        assert_eq!(register.placement_of(object(2)), Some(&Placement::Detached));
        assert_eq!(register.path_of(object(2)), None);
        assert!(register.contains(object(2)));
        assert_eq!(register.path_of(object(9)), None);
    }

    #[test]
    fn an_edit_and_a_rename_are_separate_facets() {
        let register = small();
        let version = VersionId::from_bytes([7; 32]);
        let (register, _) = register.apply(
            &IdentityChange::WriteVersion {
                object: object(2),
                version,
            },
            stamp(5),
        );
        let (register, _) = register.apply(
            &IdentityChange::Rename {
                object: object(2),
                directory: object(1),
                from_name: name("main.rs"),
                to_name: name("entry.rs"),
            },
            stamp(6),
        );
        assert_eq!(register.version_of(object(2)), Some(version));
        assert_eq!(
            register.path_of(object(2)).unwrap().to_string(),
            "/src/entry.rs"
        );
    }

    #[test]
    fn a_directory_lists_its_entries_in_name_order() {
        let register = small();
        let entries: Vec<String> = register
            .entries_of(register.root())
            .map(|(name, _)| name.to_string())
            .collect();
        assert_eq!(entries, vec!["src".to_owned()]);
        assert_eq!(register.entries_of(object(9)).count(), 0);
        assert_eq!(register.object_count(), 3);
        assert_eq!(register.object_ids().count(), 3);
        assert_eq!(register.kind_of(object(1)), Some(ObjectKind::Directory));
    }

    #[test]
    fn a_minted_object_remembers_when_it_was_minted() {
        let register = small();
        assert_eq!(register.objects[&object(1)].minted, stamp(1));
    }
}
