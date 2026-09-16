//! The reference oracle: a second materializer, written to be obviously right rather than fast.
//!
//! # What makes it worth having
//!
//! An oracle that shared the implementation's data structures would agree with it for the same
//! reason it agrees with itself. This one shares none:
//!
//! | | `mesh-materializer` | this oracle |
//! |---|---|---|
//! | objects | `BTreeMap`, `O(log n)` lookup | `Vec`, linear scan |
//! | bindings | one `BTreeMap` per directory | one flat `Vec` of `(directory, name, object, version)` |
//! | "where is this object bound?" | a maintained index | a full rescan, every time |
//! | causal order | a topological pass with a ready queue | relaxation to a fixed point |
//! | encoding | canonical bytes | none — it is compared through `Snapshot` |
//!
//! What it *does* share is the rules, because the rules are the thing under test. Where a rule is
//! subtle the comment says which line of the implementation it corresponds to, so that a divergence
//! can be resolved against the protocol rather than by moving whichever assertion is nearer.
//!
//! # This is a test oracle and nothing else
//!
//! It lives under `tests/` and no shipped code can reach it. It is quadratic in the number of
//! objects and does not care.

use mesh_materializer::{
    ActorId, AppliedChangeSet, ApprovalId, ChangeSetId, Effect, HeadId, ManifestId, NormalizedName,
    ObjectId, ObjectKind, Operation, OperationKind, PortableMetadata, PreservedEntry,
    RejectedOperation, Rejection, VersionId,
};

use super::snapshot::{sort_entries, EntryFacet, ObjectFacet, Snapshot, VersionFacet};

#[derive(Clone, Debug)]
struct NaiveObject {
    kind: ObjectKind,
    created_by: Option<ChangeSetId>,
    deleted: bool,
    current_version: Option<VersionId>,
}

#[derive(Clone, Debug)]
struct NaiveVersion {
    object: ObjectId,
    parents: Vec<VersionId>,
    manifest: ManifestId,
    metadata: PortableMetadata,
    created_by: ChangeSetId,
}

#[derive(Clone, Debug)]
struct NaiveEntry {
    directory: ObjectId,
    name: NormalizedName,
    object: ObjectId,
    version: VersionId,
}

/// The naive state.
struct Oracle {
    root: ObjectId,
    objects: Vec<(ObjectId, NaiveObject)>,
    versions: Vec<(VersionId, NaiveVersion)>,
    entries: Vec<NaiveEntry>,
    actor_heads: Vec<(ActorId, HeadId)>,
    canonical: Option<(HeadId, ApprovalId)>,
}

fn sorted_unique(mut values: Vec<VersionId>) -> Vec<VersionId> {
    values.sort_unstable();
    values.dedup();
    values
}

impl Oracle {
    fn new(root: ObjectId) -> Self {
        Self {
            root,
            objects: vec![(
                root,
                NaiveObject {
                    kind: ObjectKind::Directory,
                    created_by: None,
                    deleted: false,
                    current_version: None,
                },
            )],
            versions: Vec::new(),
            entries: Vec::new(),
            actor_heads: Vec::new(),
            canonical: None,
        }
    }

    fn find_object(&self, id: ObjectId) -> Option<&NaiveObject> {
        self.objects
            .iter()
            .find(|(held, _)| *held == id)
            .map(|(_, record)| record)
    }

    fn set_object(&mut self, id: ObjectId, record: NaiveObject) {
        match self.objects.iter_mut().find(|(held, _)| *held == id) {
            Some(slot) => slot.1 = record,
            None => self.objects.push((id, record)),
        }
    }

    fn find_version(&self, id: VersionId) -> Option<&NaiveVersion> {
        self.versions
            .iter()
            .find(|(held, _)| *held == id)
            .map(|(_, record)| record)
    }

    fn set_version(&mut self, id: VersionId, record: NaiveVersion) {
        match self.versions.iter_mut().find(|(held, _)| *held == id) {
            Some(slot) => slot.1 = record,
            None => self.versions.push((id, record)),
        }
    }

    /// Where an object is bound — by rescanning every binding there is.
    fn parent_of(&self, object: ObjectId) -> Option<ObjectId> {
        self.entries
            .iter()
            .find(|entry| entry.object == object)
            .map(|entry| entry.directory)
    }

    fn entry_at(&self, directory: ObjectId, name: &NormalizedName) -> Option<&NaiveEntry> {
        self.entries
            .iter()
            .find(|entry| entry.directory == directory && entry.name == *name)
    }

    fn entry_of(&self, directory: ObjectId, object: ObjectId) -> Option<&NaiveEntry> {
        self.entries
            .iter()
            .find(|entry| entry.directory == directory && entry.object == object)
    }

    fn unbind(&mut self, directory: ObjectId, name: &NormalizedName) {
        self.entries
            .retain(|entry| !(entry.directory == directory && entry.name == *name));
    }

    fn bind(
        &mut self,
        directory: ObjectId,
        name: NormalizedName,
        object: ObjectId,
        version: VersionId,
    ) {
        self.unbind(directory, &name);
        self.entries.push(NaiveEntry {
            directory,
            name,
            object,
            version,
        });
    }

    /// Walk up, one full rescan per step. Bounded by the object count, like the implementation's.
    fn is_self_or_ancestor(&self, candidate: ObjectId, object: ObjectId) -> bool {
        let mut at = object;
        let mut steps = 0;
        loop {
            if at == candidate {
                return true;
            }
            let Some(next) = self.parent_of(at) else {
                return false;
            };
            steps += 1;
            if steps > self.objects.len() {
                return true;
            }
            at = next;
        }
    }

    fn path_of(&self, object: ObjectId) -> Option<Vec<String>> {
        if object == self.root {
            return Some(Vec::new());
        }
        let mut names = Vec::new();
        let mut at = object;
        while at != self.root {
            let directory = self.parent_of(at)?;
            let entry = self.entry_of(directory, at)?;
            names.push(entry.name.as_str().to_owned());
            if names.len() > self.objects.len() {
                return None;
            }
            at = directory;
        }
        names.reverse();
        Some(names)
    }

    fn live(&self, id: ObjectId, required: ObjectKind) -> Result<NaiveObject, Rejection> {
        let record = self
            .find_object(id)
            .cloned()
            .ok_or(Rejection::UnknownObject { object: id })?;
        if record.kind != required {
            return Err(match required {
                ObjectKind::Directory => Rejection::NotADirectory { object: id },
                ObjectKind::File => Rejection::NotAFile { object: id },
            });
        }
        if record.deleted {
            return Err(Rejection::ObjectDeleted { object: id });
        }
        Ok(record)
    }

    fn version_of(&self, object: ObjectId, version: VersionId) -> Result<NaiveVersion, Rejection> {
        let record = self
            .find_version(version)
            .cloned()
            .ok_or(Rejection::UnknownVersion { version })?;
        if record.object != object {
            return Err(Rejection::VersionObjectMismatch {
                version,
                expected: object,
                found: record.object,
            });
        }
        Ok(record)
    }

    fn free_name(&self, directory: ObjectId, name: &NormalizedName) -> Result<(), Rejection> {
        match self.entry_at(directory, name) {
            None => Ok(()),
            Some(entry) => Err(Rejection::NameTaken {
                directory,
                name: name.clone(),
                bound_to: entry.object,
            }),
        }
    }

    fn bound_to(
        &self,
        directory: ObjectId,
        name: &NormalizedName,
        object: ObjectId,
    ) -> Result<VersionId, Rejection> {
        let entry = self
            .entry_at(directory, name)
            .ok_or_else(|| Rejection::EntryNotBound {
                directory,
                name: name.clone(),
            })?;
        if entry.object != object {
            return Err(Rejection::EntryBoundElsewhere {
                directory,
                name: name.clone(),
                bound_to: entry.object,
            });
        }
        Ok(entry.version)
    }

    fn apply(
        &mut self,
        changeset: ChangeSetId,
        operation: &Operation,
    ) -> Result<Effect, Rejection> {
        match operation {
            Operation::CreateFile { object_id } => {
                self.mint(*object_id, ObjectKind::File, changeset)
            }
            Operation::CreateDirectory { object_id } => {
                self.mint(*object_id, ObjectKind::Directory, changeset)
            }
            Operation::WriteFileVersion {
                object_id,
                version_id,
                parent_versions,
                manifest_id,
                portable_metadata,
            } => self.write(
                changeset,
                *object_id,
                *version_id,
                parent_versions.clone(),
                *manifest_id,
                *portable_metadata,
            ),
            Operation::LinkDirectoryEntry {
                directory_id,
                name,
                object_id,
                version_id,
            } => self.link(*directory_id, name, *object_id, *version_id),
            Operation::UnlinkDirectoryEntry {
                directory_id,
                name,
                object_id,
            } => {
                self.live(*directory_id, ObjectKind::Directory)?;
                self.bound_to(*directory_id, name, *object_id)?;
                self.unbind(*directory_id, name);
                Ok(Effect::Applied)
            }
            Operation::RenameEntry {
                directory_id,
                from_name,
                to_name,
                object_id,
            } => self.rename(*directory_id, from_name, to_name, *object_id),
            Operation::MoveEntry {
                from_directory_id,
                from_name,
                to_directory_id,
                to_name,
                object_id,
            } => self.move_entry(
                *from_directory_id,
                from_name,
                *to_directory_id,
                to_name,
                *object_id,
            ),
            Operation::DeleteObject { object_id } => self.delete(*object_id),
            Operation::RestoreObject {
                object_id,
                restored_version_id,
            } => self.restore(*object_id, *restored_version_id),
            Operation::SetPortableMetadata {
                object_id,
                version_id,
                portable_metadata,
            } => self.set_metadata(*object_id, *version_id, *portable_metadata),
            Operation::ResolveNameConflict {
                directory_id,
                contested_name,
                preserved,
            } => self.resolve_name(*directory_id, contested_name, preserved),
            Operation::ResolveContentConflict {
                object_id,
                resulting_version_id,
                preserved_version_ids,
            } => self.resolve_content(*object_id, *resulting_version_id, preserved_version_ids),
            Operation::AdvanceActorHead {
                actor_id,
                from_head,
                to_head,
            } => self.advance_actor(*actor_id, *from_head, *to_head),
            Operation::AdvanceCanonicalHead {
                from_head,
                to_head,
                approval_id,
            } => self.advance_canonical(*from_head, *to_head, *approval_id),
            Operation::RecordReadObservation { .. }
            | Operation::RecordDerivedNode { .. }
            | Operation::CreateReviewBundle { .. }
            | Operation::RecordValidation { .. } => Ok(Effect::OutsideStateGraph),
        }
    }

    fn mint(
        &mut self,
        id: ObjectId,
        kind: ObjectKind,
        changeset: ChangeSetId,
    ) -> Result<Effect, Rejection> {
        if self.find_object(id).is_some() {
            return Err(Rejection::ObjectAlreadyExists { object: id });
        }
        self.set_object(
            id,
            NaiveObject {
                kind,
                created_by: Some(changeset),
                deleted: false,
                current_version: None,
            },
        );
        Ok(Effect::Applied)
    }

    fn write(
        &mut self,
        changeset: ChangeSetId,
        object: ObjectId,
        version: VersionId,
        parents: Vec<VersionId>,
        manifest: ManifestId,
        metadata: PortableMetadata,
    ) -> Result<Effect, Rejection> {
        let record = self.live(object, ObjectKind::File)?;
        // The implementation normalizes the parent set inside `FileVersion::new` before it checks
        // any of them, so the *first* parent it rejects is the lowest identifier, not the first one
        // written. Checking them in the supplied order would diverge on a set with two bad parents.
        let parents = sorted_unique(parents);
        for parent in &parents {
            self.version_of(object, *parent)?;
        }
        let written = NaiveVersion {
            object,
            parents,
            manifest,
            metadata,
            created_by: changeset,
        };
        if let Some(existing) = self.find_version(version).cloned() {
            let same = existing.object == written.object
                && existing.parents == written.parents
                && existing.manifest == written.manifest
                && existing.metadata == written.metadata;
            if !same {
                return Err(Rejection::VersionAlreadyRecorded { version });
            }
            if record.current_version == Some(version) {
                return Ok(Effect::AlreadyInEffect);
            }
            self.set_object(
                object,
                NaiveObject {
                    current_version: Some(version),
                    ..record
                },
            );
            return Ok(Effect::Applied);
        }
        self.set_version(version, written);
        self.set_object(
            object,
            NaiveObject {
                current_version: Some(version),
                ..record
            },
        );
        Ok(Effect::Applied)
    }

    fn placeable(
        &self,
        directory: ObjectId,
        object: ObjectId,
        version: VersionId,
    ) -> Result<(), Rejection> {
        if object == self.root {
            return Err(Rejection::RootObject { object });
        }
        self.live(directory, ObjectKind::Directory)?;
        let record = self
            .find_object(object)
            .cloned()
            .ok_or(Rejection::UnknownObject { object })?;
        if record.deleted {
            return Err(Rejection::ObjectDeleted { object });
        }
        if record.kind == ObjectKind::File {
            self.version_of(object, version)?;
        }
        if self.is_self_or_ancestor(object, directory) {
            return Err(Rejection::WouldCycle { object, directory });
        }
        Ok(())
    }

    fn link(
        &mut self,
        directory: ObjectId,
        name: &NormalizedName,
        object: ObjectId,
        version: VersionId,
    ) -> Result<Effect, Rejection> {
        self.placeable(directory, object, version)?;
        if let Some(held) = self.parent_of(object) {
            return Err(Rejection::AlreadyLinked {
                object,
                directory: held,
            });
        }
        self.free_name(directory, name)?;
        self.bind(directory, name.clone(), object, version);
        Ok(Effect::Applied)
    }

    fn rename(
        &mut self,
        directory: ObjectId,
        from: &NormalizedName,
        to: &NormalizedName,
        object: ObjectId,
    ) -> Result<Effect, Rejection> {
        if object == self.root {
            return Err(Rejection::RootObject { object });
        }
        self.live(directory, ObjectKind::Directory)?;
        let version = self.bound_to(directory, from, object)?;
        if from == to {
            return Ok(Effect::AlreadyInEffect);
        }
        self.free_name(directory, to)?;
        self.unbind(directory, from);
        self.bind(directory, to.clone(), object, version);
        Ok(Effect::Applied)
    }

    fn move_entry(
        &mut self,
        from_directory: ObjectId,
        from_name: &NormalizedName,
        to_directory: ObjectId,
        to_name: &NormalizedName,
        object: ObjectId,
    ) -> Result<Effect, Rejection> {
        if object == self.root {
            return Err(Rejection::RootObject { object });
        }
        self.live(from_directory, ObjectKind::Directory)?;
        self.live(to_directory, ObjectKind::Directory)?;
        let version = self.bound_to(from_directory, from_name, object)?;
        if from_directory == to_directory && from_name == to_name {
            return Ok(Effect::AlreadyInEffect);
        }
        self.free_name(to_directory, to_name)?;
        if self.is_self_or_ancestor(object, to_directory) {
            return Err(Rejection::WouldCycle {
                object,
                directory: to_directory,
            });
        }
        self.unbind(from_directory, from_name);
        self.bind(to_directory, to_name.clone(), object, version);
        Ok(Effect::Applied)
    }

    fn delete(&mut self, object: ObjectId) -> Result<Effect, Rejection> {
        if object == self.root {
            return Err(Rejection::RootObject { object });
        }
        let record = self
            .find_object(object)
            .cloned()
            .ok_or(Rejection::UnknownObject { object })?;
        if record.deleted {
            return Ok(Effect::AlreadyInEffect);
        }
        self.set_object(
            object,
            NaiveObject {
                deleted: true,
                ..record
            },
        );
        Ok(Effect::Applied)
    }

    fn restore(&mut self, object: ObjectId, version: VersionId) -> Result<Effect, Rejection> {
        let record = self
            .find_object(object)
            .cloned()
            .ok_or(Rejection::UnknownObject { object })?;
        if !record.deleted {
            return Ok(Effect::AlreadyInEffect);
        }
        if record.kind == ObjectKind::File {
            self.version_of(object, version)?;
        }
        self.set_object(
            object,
            NaiveObject {
                deleted: false,
                current_version: Some(version),
                ..record
            },
        );
        Ok(Effect::Applied)
    }

    fn set_metadata(
        &mut self,
        object: ObjectId,
        version: VersionId,
        metadata: PortableMetadata,
    ) -> Result<Effect, Rejection> {
        self.live(object, ObjectKind::File)?;
        let record = self.version_of(object, version)?;
        if record.metadata == metadata {
            return Ok(Effect::AlreadyInEffect);
        }
        self.set_version(version, NaiveVersion { metadata, ..record });
        Ok(Effect::Applied)
    }

    fn resolve_name(
        &mut self,
        directory: ObjectId,
        contested: &NormalizedName,
        preserved: &[PreservedEntry],
    ) -> Result<Effect, Rejection> {
        self.live(directory, ObjectKind::Directory)?;
        if preserved.is_empty() {
            return Err(Rejection::EmptyResolution { directory });
        }
        let mut names: Vec<&NormalizedName> = preserved.iter().map(PreservedEntry::name).collect();
        let supplied_names = names.len();
        names.sort_unstable();
        names.dedup();
        let mut objects: Vec<ObjectId> = preserved.iter().map(PreservedEntry::object_id).collect();
        let supplied_objects = objects.len();
        objects.sort_unstable();
        objects.dedup();
        if names.len() != supplied_names || objects.len() != supplied_objects {
            return Err(Rejection::DuplicateResolution { subject: directory });
        }

        let mut placements: Vec<(NormalizedName, ObjectId, VersionId)> = Vec::new();
        for entry in preserved {
            let object = entry.object_id();
            if object == self.root {
                return Err(Rejection::RootObject { object });
            }
            let record = self
                .find_object(object)
                .cloned()
                .ok_or(Rejection::UnknownObject { object })?;
            if record.deleted {
                return Err(Rejection::ObjectDeleted { object });
            }
            if let Some(held) = self.parent_of(object) {
                if held != directory {
                    return Err(Rejection::AlreadyLinked {
                        object,
                        directory: held,
                    });
                }
            }
            if self.is_self_or_ancestor(object, directory) {
                return Err(Rejection::WouldCycle { object, directory });
            }
            let version = self
                .entry_of(directory, object)
                .map(|held| held.version)
                .or(record.current_version)
                .ok_or(Rejection::NoKnownVersion { object })?;
            placements.push((entry.name().clone(), object, version));
        }

        let mut remaining: Vec<(NormalizedName, ObjectId, VersionId)> = self
            .entries
            .iter()
            .filter(|entry| entry.directory == directory && entry.name != *contested)
            .filter(|entry| {
                !placements
                    .iter()
                    .any(|(_, object, _)| *object == entry.object)
            })
            .map(|entry| (entry.name.clone(), entry.object, entry.version))
            .collect();
        for (name, object, version) in &placements {
            if let Some((_, held, _)) = remaining.iter().find(|(held, _, _)| held == name) {
                return Err(Rejection::NameTaken {
                    directory,
                    name: name.clone(),
                    bound_to: *held,
                });
            }
            remaining.push((name.clone(), *object, *version));
        }
        remaining.sort_by(|one, other| one.0.cmp(&other.0));

        let mut before: Vec<(NormalizedName, ObjectId, VersionId)> = self
            .entries
            .iter()
            .filter(|entry| entry.directory == directory)
            .map(|entry| (entry.name.clone(), entry.object, entry.version))
            .collect();
        before.sort_by(|one, other| one.0.cmp(&other.0));
        if before == remaining {
            return Ok(Effect::AlreadyInEffect);
        }

        self.entries.retain(|entry| entry.directory != directory);
        for (name, object, version) in remaining {
            self.entries.push(NaiveEntry {
                directory,
                name,
                object,
                version,
            });
        }
        Ok(Effect::Applied)
    }

    fn resolve_content(
        &mut self,
        object: ObjectId,
        resulting: VersionId,
        preserved: &[VersionId],
    ) -> Result<Effect, Rejection> {
        let record = self.live(object, ObjectKind::File)?;
        self.version_of(object, resulting)?;
        let mut seen = preserved.to_vec();
        let supplied = seen.len();
        seen.sort_unstable();
        seen.dedup();
        if seen.len() != supplied || seen.contains(&resulting) {
            return Err(Rejection::DuplicateResolution { subject: object });
        }
        for version in preserved {
            self.version_of(object, *version)?;
        }
        if record.current_version == Some(resulting) {
            return Ok(Effect::AlreadyInEffect);
        }
        self.set_object(
            object,
            NaiveObject {
                current_version: Some(resulting),
                ..record
            },
        );
        Ok(Effect::Applied)
    }

    fn advance_actor(
        &mut self,
        actor: ActorId,
        from: HeadId,
        to: HeadId,
    ) -> Result<Effect, Rejection> {
        if let Some((_, current)) = self.actor_heads.iter().find(|(held, _)| *held == actor) {
            let current = *current;
            if current != from {
                return Err(Rejection::HeadNotCurrent {
                    claimed: from,
                    current,
                });
            }
            if current == to {
                return Ok(Effect::AlreadyInEffect);
            }
        }
        match self.actor_heads.iter_mut().find(|(held, _)| *held == actor) {
            Some(slot) => slot.1 = to,
            None => self.actor_heads.push((actor, to)),
        }
        Ok(Effect::Applied)
    }

    fn advance_canonical(
        &mut self,
        from: HeadId,
        to: HeadId,
        approval: ApprovalId,
    ) -> Result<Effect, Rejection> {
        if let Some((head, held)) = self.canonical {
            if head != from {
                return Err(Rejection::HeadNotCurrent {
                    claimed: from,
                    current: head,
                });
            }
            if (head, held) == (to, approval) {
                return Ok(Effect::AlreadyInEffect);
            }
        }
        self.canonical = Some((to, approval));
        Ok(Effect::Applied)
    }
}

/// The one record for an identifier: the least of every record carrying it.
///
/// `src/order.rs` states why arrival order may not decide this. Found here by a full scan of the
/// slice for each identifier rather than by one pass building a map.
fn record_for(set: &[AppliedChangeSet], id: ChangeSetId) -> Option<&AppliedChangeSet> {
    set.iter()
        .filter(|candidate| candidate.id() == id)
        .min_by(|left, right| {
            left.causal_parents()
                .cmp(right.causal_parents())
                .then_with(|| {
                    mesh_operations::encode_operations(left.operations())
                        .cmp(&mesh_operations::encode_operations(right.operations()))
                })
        })
}

/// The causal order, by relaxation to a fixed point.
///
/// A ChangeSet takes a depth only once every in-set causal parent has one — the same settling rule
/// `src/order.rs` states, reached by repeatedly sweeping the whole set instead of by a ready queue.
/// Anything that never settles keeps depth zero.
fn naive_order(set: &[AppliedChangeSet]) -> Vec<ChangeSetId> {
    let mut present: Vec<ChangeSetId> = set.iter().map(AppliedChangeSet::id).collect();
    present.sort_unstable();
    present.dedup();

    let mut settled: Vec<ChangeSetId> = Vec::new();
    let mut depth: Vec<(ChangeSetId, u64)> = present.iter().map(|id| (*id, 0)).collect();

    loop {
        let mut progressed = false;
        for id in &present {
            if settled.contains(id) {
                continue;
            }
            let changeset =
                record_for(set, *id).expect("every present identifier came from the set");
            let parents: Vec<ChangeSetId> = changeset
                .causal_parents()
                .iter()
                .copied()
                .filter(|parent| present.contains(parent) && *parent != *id)
                .collect();
            if !parents.iter().all(|parent| settled.contains(parent)) {
                continue;
            }
            let value = parents
                .iter()
                .map(|parent| {
                    depth
                        .iter()
                        .find(|(held, _)| held == parent)
                        .map_or(0, |(_, value)| value + 1)
                })
                .max()
                .unwrap_or(0);
            if let Some(slot) = depth.iter_mut().find(|(held, _)| held == id) {
                slot.1 = value;
            }
            settled.push(*id);
            progressed = true;
        }
        if !progressed {
            return {
                let mut ranked: Vec<(u64, ChangeSetId)> =
                    depth.into_iter().map(|(id, value)| (value, id)).collect();
                ranked.sort_unstable();
                ranked.into_iter().map(|(_, id)| id).collect()
            };
        }
    }
}

/// Materialize with the oracle, and project the result onto the comparison surface.
pub fn materialize(root: ObjectId, set: &[AppliedChangeSet]) -> Snapshot {
    let order = naive_order(set);
    let mut oracle = Oracle::new(root);
    let mut rejections: Vec<String> = Vec::new();
    let mut applied = 0usize;
    let mut already_in_effect = 0usize;
    let mut outside_state_graph = 0usize;

    for id in &order {
        let Some(changeset) = record_for(set, *id) else {
            continue;
        };
        for (index, operation) in changeset.operations().iter().enumerate() {
            match oracle.apply(*id, operation) {
                Ok(Effect::Applied) => applied += 1,
                Ok(Effect::AlreadyInEffect) => already_in_effect += 1,
                Ok(Effect::OutsideStateGraph) => outside_state_graph += 1,
                Err(rejection) => rejections.push(
                    RejectedOperation::new(*id, index, kind_of(operation), rejection).to_string(),
                ),
            }
        }
    }

    oracle.into_snapshot(rejections, applied, already_in_effect, outside_state_graph)
}

/// Which verb an operation is — asked of the operation rather than of the crate's own `kind()`, so
/// the oracle's rendering does not borrow the implementation's answer.
fn kind_of(operation: &Operation) -> OperationKind {
    match operation {
        Operation::CreateFile { .. } => OperationKind::CreateFile,
        Operation::CreateDirectory { .. } => OperationKind::CreateDirectory,
        Operation::WriteFileVersion { .. } => OperationKind::WriteFileVersion,
        Operation::LinkDirectoryEntry { .. } => OperationKind::LinkDirectoryEntry,
        Operation::UnlinkDirectoryEntry { .. } => OperationKind::UnlinkDirectoryEntry,
        Operation::RenameEntry { .. } => OperationKind::RenameEntry,
        Operation::MoveEntry { .. } => OperationKind::MoveEntry,
        Operation::DeleteObject { .. } => OperationKind::DeleteObject,
        Operation::RestoreObject { .. } => OperationKind::RestoreObject,
        Operation::SetPortableMetadata { .. } => OperationKind::SetPortableMetadata,
        Operation::ResolveNameConflict { .. } => OperationKind::ResolveNameConflict,
        Operation::ResolveContentConflict { .. } => OperationKind::ResolveContentConflict,
        Operation::AdvanceActorHead { .. } => OperationKind::AdvanceActorHead,
        Operation::RecordReadObservation { .. } => OperationKind::RecordReadObservation,
        Operation::RecordDerivedNode { .. } => OperationKind::RecordDerivedNode,
        Operation::CreateReviewBundle { .. } => OperationKind::CreateReviewBundle,
        Operation::RecordValidation { .. } => OperationKind::RecordValidation,
        Operation::AdvanceCanonicalHead { .. } => OperationKind::AdvanceCanonicalHead,
    }
}

impl Oracle {
    fn into_snapshot(
        self,
        rejections: Vec<String>,
        applied: usize,
        already_in_effect: usize,
        outside_state_graph: usize,
    ) -> Snapshot {
        let mut objects: Vec<ObjectFacet> = self
            .objects
            .iter()
            .map(|(id, record)| ObjectFacet {
                id: id.to_string(),
                kind: match record.kind {
                    ObjectKind::File => "File".to_owned(),
                    ObjectKind::Directory => "Directory".to_owned(),
                },
                created_by: record.created_by.map(|changeset| changeset.to_string()),
                deleted: record.deleted,
                current_version: record.current_version.map(|version| version.to_string()),
                parent: self.parent_of(*id).map(|parent| parent.to_string()),
                path: self.path_of(*id),
            })
            .collect();
        objects.sort_by(|one, other| one.id.cmp(&other.id));

        let mut entries: Vec<EntryFacet> = self
            .entries
            .iter()
            .map(|entry| EntryFacet {
                directory: entry.directory.to_string(),
                name: entry.name.as_str().to_owned(),
                object: entry.object.to_string(),
                version: entry.version.to_string(),
            })
            .collect();
        sort_entries(&mut entries);

        let mut versions: Vec<VersionFacet> = self
            .versions
            .iter()
            .map(|(id, record)| VersionFacet {
                id: id.to_string(),
                object: record.object.to_string(),
                parents: record.parents.iter().map(ToString::to_string).collect(),
                manifest: record.manifest.to_string(),
                executable: record.metadata.is_executable(),
                created_by: record.created_by.to_string(),
            })
            .collect();
        versions.sort_by(|one, other| one.id.cmp(&other.id));

        let mut actor_heads: Vec<(String, String)> = self
            .actor_heads
            .iter()
            .map(|(actor, head)| (actor.to_string(), head.to_string()))
            .collect();
        actor_heads.sort();

        Snapshot {
            root: self.root.to_string(),
            objects,
            entries,
            versions,
            actor_heads,
            canonical: self
                .canonical
                .map(|(head, approval)| (head.to_string(), approval.to_string())),
            rejections,
            applied,
            already_in_effect,
            outside_state_graph,
        }
    }
}
