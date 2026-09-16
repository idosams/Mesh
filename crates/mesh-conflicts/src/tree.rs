//! Where every object ends up — conflict rows eight and nine.
//!
//! # Applying in total order is the whole determinism argument
//!
//! Two peers holding one operation set must produce one tree. They cannot exchange a message to
//! agree on it, and they may have received the operations in any order. So placement is resolved
//! by sorting every placement-affecting change by its [`Stamp`] and applying the sorted sequence —
//! a pure function of the *set*, with arrival order nowhere in it. `tests/determinism.rs` shuffles
//! a generated operation set a hundred ways and requires one tree.
//!
//! # A move that would form a cycle is refused, and the refusal is recorded
//!
//! Ido moves `a/` into `b/` while an agent moves `b/` into `a/`. Both are valid; together they
//! describe a tree that is not a tree. Applying in total order, the first lands and the second
//! would make `b/` its own ancestor, so it is refused: the object keeps the directory it had.
//!
//! The refusal is the part that matters. It is returned as a [`RefusedMove`] carrying what was
//! attempted and what was kept, so a review bundle can show a person "this move did not land, and
//! here is the one that did" without any resolution logic of its own. **Nothing is deleted by a
//! refusal** — a refused move changes no content and drops no version, which is why row nine is
//! the one row of the table where picking a winner is the correct answer rather than a violation
//! of the preservation promise.
//!
//! # Two creates of one name keep two objects
//!
//! Row eight is explicit that both object identities are retained. The directory cannot hold two
//! entries called `notes.md`, so the later of the two — later in the same total order — hangs under
//! a name derived from its own identifier, and the collision is reported. Neither object is
//! dropped and neither create is refused.

use std::collections::{BTreeMap, BTreeSet};

use crate::change::{Change, Effect};
use crate::fold::{relate, NameFold, NameRelation};
use crate::ids::ObjectId;
use crate::name::NormalizedName;
use crate::object::ObjectKind;
use crate::snapshot::{Snapshot, MAX_PATH_DEPTH};
use crate::stamp::Stamp;

/// Where one object hangs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Placement {
    directory: ObjectId,
    name: NormalizedName,
}

impl Placement {
    /// The directory this object hangs in.
    #[must_use]
    pub const fn directory(&self) -> ObjectId {
        self.directory
    }

    /// The name it hangs under.
    #[must_use]
    pub const fn name(&self) -> &NormalizedName {
        &self.name
    }
}

/// Why a move did not land.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RefusalReason {
    /// The move would have made the object its own ancestor.
    WouldFormCycle,
    /// The target directory is not in the base and no change creates it.
    UnknownDirectory,
    /// The target exists but holds content rather than objects.
    NotADirectory,
    /// The object being moved is not in the base and no change creates it.
    UnknownObject,
}

/// A move that was refused, and what the object kept instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefusedMove {
    object: ObjectId,
    attempted_directory: ObjectId,
    kept_directory: Option<ObjectId>,
    stamp: Stamp,
    reason: RefusalReason,
}

impl RefusedMove {
    /// The object whose move was refused.
    #[must_use]
    pub const fn object(&self) -> ObjectId {
        self.object
    }

    /// The directory the move aimed at.
    #[must_use]
    pub const fn attempted_directory(&self) -> ObjectId {
        self.attempted_directory
    }

    /// The directory the object hangs in instead, absent when the object is not placed at all.
    #[must_use]
    pub const fn kept_directory(&self) -> Option<ObjectId> {
        self.kept_directory
    }

    /// Where the refused move sat in the total order.
    #[must_use]
    pub const fn stamp(&self) -> &Stamp {
        &self.stamp
    }

    /// Why it was refused.
    #[must_use]
    pub const fn reason(&self) -> RefusalReason {
        self.reason
    }
}

/// Two or more objects that asked for one name in one directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameCollision {
    directory: ObjectId,
    name: NormalizedName,
    kept: ObjectId,
    renamed: Vec<(ObjectId, NormalizedName)>,
}

impl NameCollision {
    /// The directory the collision happened in.
    #[must_use]
    pub const fn directory(&self) -> ObjectId {
        self.directory
    }

    /// The contested name.
    #[must_use]
    pub const fn name(&self) -> &NormalizedName {
        &self.name
    }

    /// The object that kept the contested name.
    #[must_use]
    pub const fn kept(&self) -> ObjectId {
        self.kept
    }

    /// Every other object, with the name it hangs under instead. None of them is dropped.
    #[must_use]
    pub fn renamed(&self) -> &[(ObjectId, NormalizedName)] {
        &self.renamed
    }
}

/// Two or more objects whose names some volume would hold as one directory entry.
///
/// **Nothing here is renamed, and that is the whole point.** `README.md` and `readme.md` are two
/// entries in mesh's model and two entries on Linux; they are one entry on a default macOS volume
/// and on NTFS. Picking a winner would be inventing a rename on behalf of a filesystem that has
/// not been asked yet, and a rename nobody performed is a loss no operation records. So both names
/// stand, and the pair is reported for the layer that knows which volume it is writing to —
/// `mesh-state`'s `preflight_directory` refuses the write, naming both names, before the first
/// byte lands.
///
/// A byte-identical pair is [`NameCollision`] and is a different problem: mesh's own model cannot
/// hold it either, so that one *is* resolved, deterministically, by [`resolve_tree`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortabilityCollision {
    directory: ObjectId,
    relation: NameRelation,
    entries: Vec<(ObjectId, NormalizedName)>,
}

impl PortabilityCollision {
    /// The directory the entries share.
    #[must_use]
    pub const fn directory(&self) -> ObjectId {
        self.directory
    }

    /// The fold families that join **every** pair in the group.
    ///
    /// A volume holds the whole group as one entry exactly when its own family is in here, which
    /// is the question `mesh-state`'s `VolumeProfile::folds` asks. Never identical — a
    /// byte-identical pair is a [`NameCollision`] instead — and never empty, since a group joined
    /// by nothing is not a collision on any volume and is not reported.
    #[must_use]
    pub const fn relation(&self) -> NameRelation {
        self.relation
    }

    /// Every object in the collision with the name it actually hangs under, in identifier order.
    #[must_use]
    pub fn entries(&self) -> &[(ObjectId, NormalizedName)] {
        &self.entries
    }
}

/// The tree the operation set describes: acyclic, total, identical on every peer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeResolution {
    root: ObjectId,
    placement: BTreeMap<ObjectId, Placement>,
    kinds: BTreeMap<ObjectId, ObjectKind>,
    refused: Vec<RefusedMove>,
    collisions: Vec<NameCollision>,
    portability: Vec<PortabilityCollision>,
}

impl TreeResolution {
    /// The root directory.
    #[must_use]
    pub const fn root(&self) -> ObjectId {
        self.root
    }

    /// Where one object hangs, absent for the root and for an object nothing placed.
    #[must_use]
    pub fn placement(&self, object: ObjectId) -> Option<&Placement> {
        self.placement.get(&object)
    }

    /// Every placed object, in identifier order.
    pub fn placements(&self) -> impl Iterator<Item = (&ObjectId, &Placement)> {
        self.placement.iter()
    }

    /// Whether this object holds content or other objects.
    #[must_use]
    pub fn kind(&self, object: ObjectId) -> Option<ObjectKind> {
        self.kinds.get(&object).copied()
    }

    /// Every move that did not land.
    #[must_use]
    pub fn refused_moves(&self) -> &[RefusedMove] {
        &self.refused
    }

    /// Every directory that was asked for one name twice.
    #[must_use]
    pub fn name_collisions(&self) -> &[NameCollision] {
        &self.collisions
    }

    /// Every group of entries that some volume would fold into one, with none of them renamed.
    ///
    /// Reported after disambiguation, so the names here are the ones the tree actually holds. A
    /// pair that row eight already separated with a derived suffix cannot reappear here, because
    /// the suffix is hexadecimal and folds to itself.
    #[must_use]
    pub fn portability_collisions(&self) -> &[PortabilityCollision] {
        &self.portability
    }

    /// The path this object ends up at, derived by walking directory entries to the root.
    #[must_use]
    pub fn path_of(&self, object: ObjectId) -> Option<String> {
        if object == self.root {
            return Some("/".to_owned());
        }
        let mut segments: Vec<String> = Vec::new();
        let mut at = object;
        for _ in 0..MAX_PATH_DEPTH {
            let placed = self.placement.get(&at)?;
            segments.push(placed.name.as_str().to_owned());
            if placed.directory == self.root {
                let mut path = String::new();
                for segment in segments.iter().rev() {
                    path.push('/');
                    path.push_str(segment);
                }
                return Some(path);
            }
            at = placed.directory;
        }
        None
    }

    /// Whether any object in this tree is its own ancestor.
    ///
    /// Always `false` for a tree this module produced. It exists so the property campaign can
    /// assert that rather than trust it.
    #[must_use]
    pub fn has_cycle(&self) -> bool {
        self.placement
            .keys()
            .any(|object| self.path_of(*object).is_none())
    }
}

/// The tree `changes` describe when applied to `base`.
#[must_use]
pub fn resolve_tree(base: &Snapshot, changes: &[Change]) -> TreeResolution {
    let root = base.root();
    let mut placement: BTreeMap<ObjectId, Placement> = BTreeMap::new();
    let mut kinds: BTreeMap<ObjectId, ObjectKind> = BTreeMap::new();
    kinds.insert(root, ObjectKind::Directory);
    for (object, held) in base.objects() {
        kinds.insert(*object, held.kind());
        if let (Some(directory), Some(name)) = (held.directory(), held.name()) {
            placement.insert(
                *object,
                Placement {
                    directory,
                    name: name.clone(),
                },
            );
        }
    }

    let mut ordered: Vec<&Change> = changes
        .iter()
        .filter(|change| change.effect().touches_placement())
        .collect();
    ordered.sort_by(|left, right| {
        (
            left.stamp(),
            left.effect().order_rank(),
            left.effect().object(),
        )
            .cmp(&(
                right.stamp(),
                right.effect().order_rank(),
                right.effect().object(),
            ))
    });

    let mut refused = Vec::new();
    let mut placed_at: BTreeMap<ObjectId, Stamp> = BTreeMap::new();
    for change in ordered {
        match change.effect() {
            Effect::Create {
                object,
                kind,
                directory,
                name,
            } => {
                kinds.insert(*object, *kind);
                placement.entry(*object).or_insert_with(|| Placement {
                    directory: *directory,
                    name: name.clone(),
                });
                placed_at.entry(*object).or_insert(*change.stamp());
            }
            Effect::Rename { object, name } => {
                if let Some(current) = placement.get(object) {
                    let moved = Placement {
                        directory: current.directory,
                        name: name.clone(),
                    };
                    placement.insert(*object, moved);
                    placed_at.insert(*object, *change.stamp());
                }
            }
            Effect::Reparent { object, directory } => {
                let refusal = reparent_refusal(
                    &placement,
                    &kinds,
                    root,
                    *object,
                    *directory,
                    MAX_PATH_DEPTH,
                );
                if let Some(reason) = refusal {
                    refused.push(RefusedMove {
                        object: *object,
                        attempted_directory: *directory,
                        kept_directory: placement.get(object).map(Placement::directory),
                        stamp: *change.stamp(),
                        reason,
                    });
                    continue;
                }
                if let Some(current) = placement.get(object) {
                    let moved = Placement {
                        directory: *directory,
                        name: current.name.clone(),
                    };
                    placement.insert(*object, moved);
                    placed_at.insert(*object, *change.stamp());
                }
            }
            Effect::WriteText { .. } | Effect::WriteBinary { .. } | Effect::Delete { .. } => {}
        }
    }

    let (placement, collisions) = disambiguate(placement, &placed_at);
    let portability = portability_collisions(&placement);
    TreeResolution {
        root,
        placement,
        kinds,
        refused,
        collisions,
        portability,
    }
}

/// Every group of entries in one directory that some volume mesh supports would fold into one.
///
/// # Why two passes and a union, rather than one key
///
/// Every volume applies exactly ONE fold family, so one volume needs one key. This function
/// answers for ALL of them at once, and the families are not nested: `caseless` subsumes
/// `canonical`, and `upcase` subsumes neither and is subsumed by neither. U+0131 dotless ı and `i`
/// share an uppercase mapping and do not share a caseless form, so a single caseless pass walks
/// straight past a pair NTFS holds as one entry — and walking past it is exactly the silent merge
/// this whole task exists to prevent.
///
/// So: group by the caseless key, group by the uppercase key, and union any two groups that share
/// an object. The union is a disjoint-set pass over the directory's entries, linear in everything
/// but the pairwise relation of each surviving group, which has at most a handful of members.
fn portability_collisions(placement: &BTreeMap<ObjectId, Placement>) -> Vec<PortabilityCollision> {
    let mut by_directory: BTreeMap<ObjectId, Vec<(ObjectId, NormalizedName)>> = BTreeMap::new();
    for (object, placed) in placement {
        by_directory
            .entry(placed.directory)
            .or_default()
            .push((*object, placed.name.clone()));
    }

    let mut found = Vec::new();
    for (directory, entries) in by_directory {
        for group in fold_groups(&entries) {
            let members: Vec<(ObjectId, NormalizedName)> =
                group.into_iter().map(|at| entries[at].clone()).collect();
            let relation = shared_families(&members);
            // Identical is impossible after disambiguation, and a group joined by nothing is not
            // a collision on any volume. Both are dropped so that every row here is actionable.
            if relation.is_identical() || relation.is_distinct() {
                continue;
            }
            found.push(PortabilityCollision {
                directory,
                relation,
                entries: members,
            });
        }
    }
    found
}

/// The groups of entry indices that any supported fold family joins, each of size two or more.
fn fold_groups(entries: &[(ObjectId, NormalizedName)]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..entries.len()).collect();
    for fold in [NameFold::Caseless, NameFold::Upcase] {
        let mut first_seen: BTreeMap<String, usize> = BTreeMap::new();
        for (at, (_, name)) in entries.iter().enumerate() {
            match first_seen.entry(fold.key(name.as_str())) {
                std::collections::btree_map::Entry::Vacant(slot) => {
                    slot.insert(at);
                }
                std::collections::btree_map::Entry::Occupied(slot) => {
                    union(&mut parent, *slot.get(), at);
                }
            }
        }
    }

    let mut grouped: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for at in 0..entries.len() {
        grouped.entry(find(&mut parent, at)).or_default().push(at);
    }
    grouped
        .into_values()
        .filter(|group| group.len() > 1)
        .collect()
}

/// The representative of `at`'s set, with the path compressed on the way back.
fn find(parent: &mut [usize], at: usize) -> usize {
    let mut root = at;
    while parent[root] != root {
        root = parent[root];
    }
    let mut walk = at;
    while parent[walk] != root {
        let next = parent[walk];
        parent[walk] = root;
        walk = next;
    }
    root
}

/// Put `left` and `right` in one set.
fn union(parent: &mut [usize], left: usize, right: usize) {
    let (left, right) = (find(parent, left), find(parent, right));
    if left != right {
        parent[right.max(left)] = right.min(left);
    }
}

/// The fold families that join every pair in the group.
fn shared_families(entries: &[(ObjectId, NormalizedName)]) -> NameRelation {
    let first = entries[0].1.as_str();
    let mut shared = relate(first, first);
    for (index, (_, left)) in entries.iter().enumerate() {
        for (_, right) in &entries[index + 1..] {
            shared = shared.intersect(relate(left.as_str(), right.as_str()));
        }
    }
    shared
}

/// Why this reparent cannot land, or `None` when it can.
fn reparent_refusal(
    placement: &BTreeMap<ObjectId, Placement>,
    kinds: &BTreeMap<ObjectId, ObjectKind>,
    root: ObjectId,
    object: ObjectId,
    directory: ObjectId,
    depth_limit: usize,
) -> Option<RefusalReason> {
    if !placement.contains_key(&object) {
        return Some(RefusalReason::UnknownObject);
    }
    if directory != root && !placement.contains_key(&directory) {
        return Some(RefusalReason::UnknownDirectory);
    }
    if kinds.get(&directory) != Some(&ObjectKind::Directory) {
        return Some(RefusalReason::NotADirectory);
    }
    if directory == object {
        return Some(RefusalReason::WouldFormCycle);
    }
    let mut at = directory;
    for _ in 0..depth_limit {
        // The walk reaching an unplaced object means it reached the root, which is the only
        // unplaced object a resolved tree has. The move lands.
        let placed = placement.get(&at)?;
        if placed.directory == object {
            return Some(RefusalReason::WouldFormCycle);
        }
        at = placed.directory;
    }
    Some(RefusalReason::WouldFormCycle)
}

/// Give every object in a contested directory entry a name of its own.
///
/// The object placed earliest in the total order keeps the contested name; an object the base
/// already held counts as earliest, since nothing in this operation set moved it. Every other one
/// takes a name derived from its own identifier, so two peers derive the same names without
/// exchanging anything.
fn disambiguate(
    placement: BTreeMap<ObjectId, Placement>,
    placed_at: &BTreeMap<ObjectId, Stamp>,
) -> (BTreeMap<ObjectId, Placement>, Vec<NameCollision>) {
    let mut by_entry: BTreeMap<(ObjectId, NormalizedName), Vec<ObjectId>> = BTreeMap::new();
    for (object, placed) in &placement {
        by_entry
            .entry((placed.directory, placed.name.clone()))
            .or_default()
            .push(*object);
    }

    let mut taken: BTreeSet<(ObjectId, NormalizedName)> = by_entry.keys().cloned().collect();
    let mut settled = placement.clone();
    let mut collisions = Vec::new();
    for ((directory, name), mut objects) in by_entry {
        if objects.len() < 2 {
            continue;
        }
        objects.sort_by_key(|object| (placed_at.get(object).copied(), *object));
        let kept = objects[0];
        let mut renamed = Vec::new();
        for object in objects.into_iter().skip(1) {
            let mut candidate = name.disambiguated(&object.short_hex());
            if taken.contains(&(directory, candidate.clone())) {
                candidate = name.disambiguated(&object.to_string());
            }
            taken.insert((directory, candidate.clone()));
            settled.insert(
                object,
                Placement {
                    directory,
                    name: candidate.clone(),
                },
            );
            renamed.push((object, candidate));
        }
        collisions.push(NameCollision {
            directory,
            name,
            kept,
            renamed,
        });
    }
    (settled, collisions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ActorId;
    use crate::stamp::{EventId, Lamport};

    fn name(text: &str) -> NormalizedName {
        NormalizedName::new(text).unwrap()
    }

    fn at(lamport: u64, event: u8) -> Stamp {
        Stamp::new(
            Lamport::new(lamport),
            EventId::from_bytes([event; 16]),
            [0; 32],
        )
    }

    fn change(stamp: Stamp, effect: Effect) -> Change {
        Change::new(stamp, ActorId::from_bytes([1; 32]), effect)
    }

    fn two_directories() -> (Snapshot, ObjectId, ObjectId, ObjectId) {
        let root = ObjectId::from_bytes([0; 16]);
        let first = ObjectId::from_bytes([1; 16]);
        let second = ObjectId::from_bytes([2; 16]);
        let snapshot = Snapshot::new(root)
            .with_directory(first, root, name("a"))
            .with_directory(second, root, name("b"));
        (snapshot, root, first, second)
    }

    #[test]
    fn an_untouched_base_resolves_to_itself() {
        let (base, _, first, _) = two_directories();
        let tree = resolve_tree(&base, &[]);
        assert_eq!(tree.path_of(first).unwrap(), "/a");
        assert!(!tree.has_cycle());
    }

    #[test]
    fn a_cyclic_pair_of_moves_leaves_one_standing_and_records_the_other() {
        let (base, _, first, second) = two_directories();
        let changes = [
            change(
                at(4, 1),
                Effect::Reparent {
                    object: first,
                    directory: second,
                },
            ),
            change(
                at(4, 2),
                Effect::Reparent {
                    object: second,
                    directory: first,
                },
            ),
        ];
        let tree = resolve_tree(&base, &changes);
        assert!(!tree.has_cycle());
        assert_eq!(tree.path_of(first).unwrap(), "/b/a");
        assert_eq!(tree.refused_moves().len(), 1);
        assert_eq!(tree.refused_moves()[0].object(), second);
        assert_eq!(
            tree.refused_moves()[0].reason(),
            RefusalReason::WouldFormCycle
        );
    }

    #[test]
    fn arrival_order_changes_nothing() {
        let (base, _, first, second) = two_directories();
        let forward = [
            change(
                at(4, 1),
                Effect::Reparent {
                    object: first,
                    directory: second,
                },
            ),
            change(
                at(4, 2),
                Effect::Reparent {
                    object: second,
                    directory: first,
                },
            ),
        ];
        let backward = [forward[1].clone(), forward[0].clone()];
        assert_eq!(
            resolve_tree(&base, &forward),
            resolve_tree(&base, &backward)
        );
    }

    #[test]
    fn moving_a_directory_into_itself_is_refused() {
        let (base, _, first, _) = two_directories();
        let changes = [change(
            at(4, 1),
            Effect::Reparent {
                object: first,
                directory: first,
            },
        )];
        let tree = resolve_tree(&base, &changes);
        assert_eq!(tree.path_of(first).unwrap(), "/a");
        assert_eq!(
            tree.refused_moves()[0].reason(),
            RefusalReason::WouldFormCycle
        );
    }

    #[test]
    fn moving_into_a_file_is_refused() {
        let (base, root, first, _) = two_directories();
        let file = ObjectId::from_bytes([9; 16]);
        let base = base.with_file(
            file,
            root,
            name("f.txt"),
            crate::object::Content::Binary {
                version: crate::ids::VersionId::from_bytes([1; 32]),
                digest: [0; 32],
                byte_length: 1,
            },
        );
        let changes = [change(
            at(4, 1),
            Effect::Reparent {
                object: first,
                directory: file,
            },
        )];
        let tree = resolve_tree(&base, &changes);
        assert_eq!(
            tree.refused_moves()[0].reason(),
            RefusalReason::NotADirectory
        );
    }

    #[test]
    fn two_creates_of_one_name_both_land_under_different_names() {
        let root = ObjectId::from_bytes([0; 16]);
        let base = Snapshot::new(root);
        let mine = ObjectId::from_bytes([0xa1; 16]);
        let theirs = ObjectId::from_bytes([0xb2; 16]);
        let changes = [
            change(
                at(3, 1),
                Effect::Create {
                    object: mine,
                    kind: ObjectKind::File,
                    directory: root,
                    name: name("notes.md"),
                },
            ),
            change(
                at(3, 2),
                Effect::Create {
                    object: theirs,
                    kind: ObjectKind::File,
                    directory: root,
                    name: name("notes.md"),
                },
            ),
        ];
        let tree = resolve_tree(&base, &changes);
        assert_eq!(tree.path_of(mine).unwrap(), "/notes.md");
        assert_eq!(tree.path_of(theirs).unwrap(), "/notes~b2b2b2b2.md");
        assert_eq!(tree.name_collisions().len(), 1);
        assert_eq!(tree.name_collisions()[0].kept(), mine);
        assert_eq!(tree.name_collisions()[0].renamed().len(), 1);
    }

    #[test]
    fn a_rename_and_a_move_of_one_object_both_land() {
        let (base, root, first, second) = two_directories();
        let changes = [
            change(
                at(5, 1),
                Effect::Rename {
                    object: first,
                    name: name("renamed"),
                },
            ),
            change(
                at(5, 2),
                Effect::Reparent {
                    object: first,
                    directory: second,
                },
            ),
        ];
        let tree = resolve_tree(&base, &changes);
        assert_eq!(tree.path_of(first).unwrap(), "/b/renamed");
        assert_eq!(tree.root(), root);
    }

    #[test]
    fn a_move_to_an_unknown_directory_is_refused() {
        let (base, _, first, _) = two_directories();
        let changes = [change(
            at(4, 1),
            Effect::Reparent {
                object: first,
                directory: ObjectId::from_bytes([0xee; 16]),
            },
        )];
        let tree = resolve_tree(&base, &changes);
        assert_eq!(
            tree.refused_moves()[0].reason(),
            RefusalReason::UnknownDirectory
        );
        assert_eq!(tree.refused_moves()[0].kept_directory(), Some(base.root()));
    }
}
