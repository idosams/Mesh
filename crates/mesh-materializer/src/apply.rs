//! Materialization: the fold from an applied causal set to an exact workspace state.
//!
//! # One sentence this module is built around
//!
//! **A workspace state is a pure function of the applied causal set, and of nothing else.** Not of
//! delivery order, not of how many times something arrived, not of a clock, and not of who is
//! asking. Everything below is that sentence made operational:
//!
//! * *Delivery order* cannot matter, because [`materialize`] never applies the slice it is given —
//!   it applies [`crate::causal_order`] of that slice, which is a function of the set.
//! * *A clock* cannot matter, because no clock reaches this crate: `AppliedChangeSet` has no
//!   hybrid-logical-time field to read.
//! * *Repetition* cannot matter, because a ChangeSet appearing twice under one identifier is one
//!   ChangeSet in the order.
//! * *Failure* cannot matter, because every operation has an answer:
//!   [`crate::Effect`] or [`crate::Rejection`], never a panic and never a silent skip.
//!
//! # The state is built through a private working copy, and that is deliberate
//!
//! [`WorkspaceState`] exposes no public mutator. Inside this function one is held by value and
//! written through, because materializing a thousand-operation set by cloning the whole state a
//! thousand times is a cost with no buyer: the value never escapes until it is finished, so no
//! caller can observe it half-built. The observable API is a pure function; the private one is a
//! buffer.
//!
//! # Cost, stated rather than implied
//!
//! Ordering is `O(n log n)` in the number of ChangeSets. Applying is `O(1)` amortised per operation
//! except for two: the cycle check on a link or a move walks the ancestor chain, which is `O(depth
//! of the tree)`, and a name-conflict resolution is `O(entries in the directory)`. Moving a subtree
//! is one entry-map edit whatever is under it — plan §11's budget is a property of the vocabulary's
//! shape, and this module does not spend it — but the depth walk means a move is not `O(1)` here
//! and this crate publishes no wall-clock number for one. It carries no benchmark.

use crate::ids::{ActorId, ApprovalId, ChangeSetId, HeadId, ObjectId, VersionId};
use crate::name::{NormalizedName, PortableMetadata};
use crate::operation::{Operation, OperationKind, PreservedEntry};
use crate::order::{canonical_records, causal_order, AppliedChangeSet};
use crate::rejection::{Effect, RejectedOperation, Rejection};
use crate::state::{CanonicalAdvance, WorkspaceState};
use crate::version::{DirectoryEntry, DirectoryVersion, FileVersion, ObjectKind, ObjectRecord};

/// The result of materializing an operation set: the state, and everything the state would not
/// accept.
///
/// Both halves are a pure function of the set. The rejections are kept rather than discarded
/// because "this set produced this state" and "this set also asked for six impossible things" are
/// two facts a caller needs separately — a diff surface renders the first and a conflict surface
/// renders the second.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Materialization {
    state: WorkspaceState,
    order: Vec<ChangeSetId>,
    rejections: Vec<RejectedOperation>,
    applied: usize,
    already_in_effect: usize,
    outside_state_graph: usize,
}

impl Materialization {
    /// The workspace state.
    #[must_use]
    pub const fn state(&self) -> &WorkspaceState {
        &self.state
    }

    /// The order the set was applied in — causal depth, then identifier.
    #[must_use]
    pub fn order(&self) -> &[ChangeSetId] {
        &self.order
    }

    /// Every operation the state would not accept, in the order they were reached.
    #[must_use]
    pub fn rejections(&self) -> &[RejectedOperation] {
        &self.rejections
    }

    /// How many operations changed the state.
    #[must_use]
    pub const fn applied(&self) -> usize {
        self.applied
    }

    /// How many operations asked for something the state already said.
    #[must_use]
    pub const fn already_in_effect(&self) -> usize {
        self.already_in_effect
    }

    /// How many operations belong to the dependency graph or the trust graph and so wrote nothing.
    #[must_use]
    pub const fn outside_state_graph(&self) -> usize {
        self.outside_state_graph
    }

    /// Every operation reached, whatever it did. Equal to the number of operations in the set.
    ///
    /// Not `const`: `Vec::len` became usable in a `const` context in Rust 1.87 and this workspace's
    /// `rust-version` is 1.85, which `clippy::incompatible_msrv` enforces.
    #[must_use]
    pub fn reached(&self) -> usize {
        self.applied + self.already_in_effect + self.outside_state_graph + self.rejections.len()
    }
}

/// Materialize an operation set into an exact workspace state.
///
/// `root` is the workspace root object: it exists before any ChangeSet does, and no operation
/// creates, deletes, renames or moves it.
///
/// ```
/// use mesh_materializer::{
///     materialize, AppliedChangeSet, ChangeSetId, NormalizedName, ObjectId, Operation,
///     PortableMetadata, ManifestId, VersionId,
/// };
///
/// let root = ObjectId::from_bytes([0; 16]);
/// let file = ObjectId::from_bytes([1; 16]);
/// let version = VersionId::from_bytes([2; 32]);
///
/// let set = [AppliedChangeSet::genesis(
///     ChangeSetId::from_bytes([3; 32]),
///     vec![
///         Operation::CreateFile { object_id: file },
///         Operation::WriteFileVersion {
///             object_id: file,
///             version_id: version,
///             parent_versions: vec![],
///             manifest_id: ManifestId::from_bytes([4; 32]),
///             portable_metadata: PortableMetadata::default(),
///         },
///         Operation::LinkDirectoryEntry {
///             directory_id: root,
///             name: NormalizedName::new("report.md").unwrap(),
///             object_id: file,
///             version_id: version,
///         },
///     ],
/// )];
///
/// let materialized = materialize(root, &set);
/// assert!(materialized.rejections().is_empty());
/// assert_eq!(materialized.state().root_directory().len(), 1);
///
/// // Materializing the same set twice produces byte-identical state.
/// assert_eq!(
///     materialize(root, &set).state().canonical_bytes(),
///     materialized.state().canonical_bytes()
/// );
/// ```
#[must_use]
pub fn materialize(root: ObjectId, set: &[AppliedChangeSet]) -> Materialization {
    let order = causal_order(set);
    let records = canonical_records(set);
    let mut state = WorkspaceState::empty(root);
    let mut rejections = Vec::new();
    let mut applied = 0usize;
    let mut already_in_effect = 0usize;
    let mut outside_state_graph = 0usize;

    for id in &order {
        let Some(changeset) = records.get(id) else {
            continue;
        };
        for (index, operation) in changeset.operations().iter().enumerate() {
            match apply_operation(&mut state, *id, operation) {
                Ok(Effect::Applied) => applied += 1,
                Ok(Effect::AlreadyInEffect) => already_in_effect += 1,
                Ok(Effect::OutsideStateGraph) => outside_state_graph += 1,
                Err(rejection) => rejections.push(RejectedOperation::new(
                    *id,
                    index,
                    operation.kind(),
                    rejection,
                )),
            }
        }
    }

    Materialization {
        state,
        order,
        rejections,
        applied,
        already_in_effect,
        outside_state_graph,
    }
}

/// Apply one operation, or say why the state would not take it.
///
/// # Errors
///
/// [`Rejection`] naming the exact entity that made the answer what it is. The state is left
/// untouched: every arm below validates before it writes.
pub fn apply_operation(
    state: &mut WorkspaceState,
    changeset: ChangeSetId,
    operation: &Operation,
) -> Result<Effect, Rejection> {
    match operation {
        Operation::InitializeWorkspace { root_id } => {
            if *root_id == state.root() {
                Ok(Effect::AlreadyInEffect)
            } else {
                Err(Rejection::RootIdentityMismatch {
                    expected: state.root(),
                    declared: *root_id,
                })
            }
        }
        Operation::CreateFile { object_id } => mint(state, *object_id, ObjectKind::File, changeset),
        Operation::CreateDirectory { object_id } => {
            mint(state, *object_id, ObjectKind::Directory, changeset)
        }
        Operation::WriteFileVersion {
            object_id,
            version_id,
            parent_versions,
            manifest_id,
            portable_metadata,
        } => write_file_version(
            state,
            *object_id,
            *version_id,
            FileVersion::new(
                *object_id,
                parent_versions.clone(),
                *manifest_id,
                *portable_metadata,
                changeset,
            ),
        ),
        Operation::LinkDirectoryEntry {
            directory_id,
            name,
            object_id,
            version_id,
        } => link(state, *directory_id, name, *object_id, *version_id),
        Operation::UnlinkDirectoryEntry {
            directory_id,
            name,
            object_id,
        } => unlink(state, *directory_id, name, *object_id),
        Operation::RenameEntry {
            directory_id,
            from_name,
            to_name,
            object_id,
        } => rename(state, *directory_id, from_name, to_name, *object_id),
        Operation::MoveEntry {
            from_directory_id,
            from_name,
            to_directory_id,
            to_name,
            object_id,
        } => move_entry(
            state,
            (*from_directory_id, from_name),
            (*to_directory_id, to_name),
            *object_id,
        ),
        Operation::DeleteObject { object_id } => delete(state, *object_id),
        Operation::RestoreObject {
            object_id,
            restored_version_id,
        } => restore(state, *object_id, *restored_version_id),
        Operation::SetPortableMetadata {
            object_id,
            version_id,
            portable_metadata,
        } => set_metadata(state, *object_id, *version_id, *portable_metadata),
        Operation::ResolveNameConflict {
            directory_id,
            contested_name,
            preserved,
        } => resolve_name(state, *directory_id, contested_name, preserved),
        Operation::ResolveContentConflict {
            object_id,
            resulting_version_id,
            preserved_version_ids,
        } => resolve_content(
            state,
            *object_id,
            *resulting_version_id,
            preserved_version_ids,
        ),
        Operation::AdvanceActorHead {
            actor_id,
            from_head,
            to_head,
        } => advance_actor(state, *actor_id, *from_head, *to_head),
        Operation::AdvanceCanonicalHead {
            from_head,
            to_head,
            approval_id,
        } => advance_canonical(state, *from_head, *to_head, *approval_id),
        // The four dependency-graph and trust-graph verbs. Spelled out rather than caught by a
        // wildcard, so a nineteenth verb added to the vocabulary is a compile error here instead of
        // a verb that quietly materializes to nothing.
        Operation::RecordReadObservation { .. }
        | Operation::RecordDerivedNode { .. }
        | Operation::CreateReviewBundle { .. }
        | Operation::RecordValidation { .. } => Ok(Effect::OutsideStateGraph),
    }
}

/// The object, if it exists and is not deleted and is of the required kind.
fn live_object(
    state: &WorkspaceState,
    id: ObjectId,
    required: ObjectKind,
) -> Result<ObjectRecord, Rejection> {
    let record = state
        .object(id)
        .cloned()
        .ok_or(Rejection::UnknownObject { object: id })?;
    if record.kind() != required {
        return Err(match required {
            ObjectKind::Directory => Rejection::NotADirectory { object: id },
            ObjectKind::File => Rejection::NotAFile { object: id },
        });
    }
    if record.is_deleted() {
        return Err(Rejection::ObjectDeleted { object: id });
    }
    Ok(record)
}

/// The file version, if it exists and belongs to `object`.
fn version_of(
    state: &WorkspaceState,
    object: ObjectId,
    version: VersionId,
) -> Result<FileVersion, Rejection> {
    let record = state
        .file_version(version)
        .cloned()
        .ok_or(Rejection::UnknownVersion { version })?;
    if record.object_id() != object {
        return Err(Rejection::VersionObjectMismatch {
            version,
            expected: object,
            found: record.object_id(),
        });
    }
    Ok(record)
}

fn mint(
    state: &mut WorkspaceState,
    id: ObjectId,
    kind: ObjectKind,
    changeset: ChangeSetId,
) -> Result<Effect, Rejection> {
    if state.object(id).is_some() {
        return Err(Rejection::ObjectAlreadyExists { object: id });
    }
    state.insert_object(id, ObjectRecord::minted(kind, changeset));
    if kind == ObjectKind::Directory {
        state.insert_directory(id, DirectoryVersion::empty());
    }
    Ok(Effect::Applied)
}

fn write_file_version(
    state: &mut WorkspaceState,
    object: ObjectId,
    version: VersionId,
    record: FileVersion,
) -> Result<Effect, Rejection> {
    let existing_object = live_object(state, object, ObjectKind::File)?;
    for parent in record.parent_versions() {
        version_of(state, object, *parent)?;
    }
    if let Some(existing) = state.file_version(version) {
        if !existing.has_same_content(&record) {
            return Err(Rejection::VersionAlreadyRecorded { version });
        }
        if existing_object.current_version() == Some(version) {
            return Ok(Effect::AlreadyInEffect);
        }
        state.insert_object(object, existing_object.with_current_version(Some(version)));
        return Ok(Effect::Applied);
    }
    state.insert_file_version(version, record);
    state.insert_object(object, existing_object.with_current_version(Some(version)));
    Ok(Effect::Applied)
}

/// The checks a link or a move performs on the object it is about to place.
fn placeable(
    state: &WorkspaceState,
    directory: ObjectId,
    object: ObjectId,
    version: VersionId,
) -> Result<(), Rejection> {
    if object == state.root() {
        return Err(Rejection::RootObject { object });
    }
    live_object(state, directory, ObjectKind::Directory)?;
    let record = state
        .object(object)
        .cloned()
        .ok_or(Rejection::UnknownObject { object })?;
    if record.is_deleted() {
        return Err(Rejection::ObjectDeleted { object });
    }
    if record.kind() == ObjectKind::File {
        version_of(state, object, version)?;
    }
    if state.is_self_or_ancestor(object, directory) {
        return Err(Rejection::WouldCycle { object, directory });
    }
    Ok(())
}

/// The name is free in this directory, or it is not.
fn free_name(
    state: &WorkspaceState,
    directory: ObjectId,
    name: &NormalizedName,
) -> Result<(), Rejection> {
    match state.directory(directory).and_then(|held| held.entry(name)) {
        None => Ok(()),
        Some(entry) => Err(Rejection::NameTaken {
            directory,
            name: name.clone(),
            bound_to: entry.object_id(),
        }),
    }
}

/// The name is bound here, to exactly this object.
fn bound_to(
    state: &WorkspaceState,
    directory: ObjectId,
    name: &NormalizedName,
    object: ObjectId,
) -> Result<DirectoryEntry, Rejection> {
    let entry = state
        .directory(directory)
        .and_then(|held| held.entry(name))
        .ok_or_else(|| Rejection::EntryNotBound {
            directory,
            name: name.clone(),
        })?;
    if entry.object_id() != object {
        return Err(Rejection::EntryBoundElsewhere {
            directory,
            name: name.clone(),
            bound_to: entry.object_id(),
        });
    }
    Ok(entry)
}

fn link(
    state: &mut WorkspaceState,
    directory: ObjectId,
    name: &NormalizedName,
    object: ObjectId,
    version: VersionId,
) -> Result<Effect, Rejection> {
    placeable(state, directory, object, version)?;
    if let Some(held) = state.parent_of(object) {
        return Err(Rejection::AlreadyLinked {
            object,
            directory: held,
        });
    }
    free_name(state, directory, name)?;
    state.bind(
        directory,
        name.clone(),
        DirectoryEntry::new(object, version),
    );
    Ok(Effect::Applied)
}

fn unlink(
    state: &mut WorkspaceState,
    directory: ObjectId,
    name: &NormalizedName,
    object: ObjectId,
) -> Result<Effect, Rejection> {
    live_object(state, directory, ObjectKind::Directory)?;
    bound_to(state, directory, name, object)?;
    state.unbind(directory, name);
    Ok(Effect::Applied)
}

fn rename(
    state: &mut WorkspaceState,
    directory: ObjectId,
    from: &NormalizedName,
    to: &NormalizedName,
    object: ObjectId,
) -> Result<Effect, Rejection> {
    if object == state.root() {
        return Err(Rejection::RootObject { object });
    }
    live_object(state, directory, ObjectKind::Directory)?;
    let entry = bound_to(state, directory, from, object)?;
    if from == to {
        return Ok(Effect::AlreadyInEffect);
    }
    free_name(state, directory, to)?;
    state.unbind(directory, from);
    state.bind(directory, to.clone(), entry);
    Ok(Effect::Applied)
}

fn move_entry(
    state: &mut WorkspaceState,
    from: (ObjectId, &NormalizedName),
    to: (ObjectId, &NormalizedName),
    object: ObjectId,
) -> Result<Effect, Rejection> {
    let (from_directory, from_name) = from;
    let (to_directory, to_name) = to;
    if object == state.root() {
        return Err(Rejection::RootObject { object });
    }
    live_object(state, from_directory, ObjectKind::Directory)?;
    live_object(state, to_directory, ObjectKind::Directory)?;
    let entry = bound_to(state, from_directory, from_name, object)?;
    if from_directory == to_directory && from_name == to_name {
        return Ok(Effect::AlreadyInEffect);
    }
    free_name(state, to_directory, to_name)?;
    if state.is_self_or_ancestor(object, to_directory) {
        return Err(Rejection::WouldCycle {
            object,
            directory: to_directory,
        });
    }
    state.unbind(from_directory, from_name);
    state.bind(to_directory, to_name.clone(), entry);
    Ok(Effect::Applied)
}

fn delete(state: &mut WorkspaceState, object: ObjectId) -> Result<Effect, Rejection> {
    if object == state.root() {
        return Err(Rejection::RootObject { object });
    }
    let record = state
        .object(object)
        .cloned()
        .ok_or(Rejection::UnknownObject { object })?;
    if record.is_deleted() {
        return Ok(Effect::AlreadyInEffect);
    }
    // Deletion is a state, never an erasure: every version is preserved and the name binding is
    // left exactly as it was. Removing the binding is `UnlinkDirectoryEntry`'s, and doing both here
    // would make one verb do two things and make `RestoreObject` unable to put the name back.
    state.insert_object(object, record.with_deleted(true));
    Ok(Effect::Applied)
}

fn restore(
    state: &mut WorkspaceState,
    object: ObjectId,
    version: VersionId,
) -> Result<Effect, Rejection> {
    let record = state
        .object(object)
        .cloned()
        .ok_or(Rejection::UnknownObject { object })?;
    if !record.is_deleted() {
        return Ok(Effect::AlreadyInEffect);
    }
    if record.kind() == ObjectKind::File {
        version_of(state, object, version)?;
    }
    state.insert_object(
        object,
        record
            .with_deleted(false)
            .with_current_version(Some(version)),
    );
    Ok(Effect::Applied)
}

fn set_metadata(
    state: &mut WorkspaceState,
    object: ObjectId,
    version: VersionId,
    metadata: PortableMetadata,
) -> Result<Effect, Rejection> {
    live_object(state, object, ObjectKind::File)?;
    let record = version_of(state, object, version)?;
    if record.portable_metadata() == metadata {
        return Ok(Effect::AlreadyInEffect);
    }
    state.insert_file_version(version, record.with_portable_metadata(metadata));
    Ok(Effect::Applied)
}

/// The version a preserved contender keeps: the one its current binding names, or its own current
/// version if it is not bound anywhere.
fn preserved_version(
    state: &WorkspaceState,
    directory: ObjectId,
    object: ObjectId,
) -> Option<VersionId> {
    state
        .directory(directory)
        .and_then(|held| held.name_of(object))
        .and_then(|name| {
            state
                .directory(directory)
                .and_then(|held| held.entry(&name))
        })
        .map(|entry| entry.version_id())
        .or_else(|| state.object(object).and_then(ObjectRecord::current_version))
}

fn resolve_name(
    state: &mut WorkspaceState,
    directory: ObjectId,
    contested: &NormalizedName,
    preserved: &[PreservedEntry],
) -> Result<Effect, Rejection> {
    live_object(state, directory, ObjectKind::Directory)?;
    if preserved.is_empty() {
        return Err(Rejection::EmptyResolution { directory });
    }
    let mut names: Vec<&NormalizedName> = preserved.iter().map(PreservedEntry::name).collect();
    names.sort_unstable();
    let unique_names = names.len();
    names.dedup();
    let mut objects: Vec<ObjectId> = preserved.iter().map(PreservedEntry::object_id).collect();
    objects.sort_unstable();
    let unique_objects = objects.len();
    objects.dedup();
    if names.len() != unique_names || objects.len() != unique_objects {
        return Err(Rejection::DuplicateResolution { subject: directory });
    }

    let mut placements = Vec::with_capacity(preserved.len());
    for entry in preserved {
        let object = entry.object_id();
        if object == state.root() {
            return Err(Rejection::RootObject { object });
        }
        let record = state
            .object(object)
            .cloned()
            .ok_or(Rejection::UnknownObject { object })?;
        if record.is_deleted() {
            return Err(Rejection::ObjectDeleted { object });
        }
        match state.parent_of(object) {
            Some(held) if held != directory => {
                return Err(Rejection::AlreadyLinked {
                    object,
                    directory: held,
                })
            }
            _ => {}
        }
        if state.is_self_or_ancestor(object, directory) {
            return Err(Rejection::WouldCycle { object, directory });
        }
        let version = preserved_version(state, directory, object)
            .ok_or(Rejection::NoKnownVersion { object })?;
        placements.push((entry.name().clone(), DirectoryEntry::new(object, version)));
    }

    // What the directory holds once the contested name and every contender's current binding are
    // taken out. Any target name still occupied belongs to somebody the resolution did not name,
    // and settling one contest by evicting a bystander is not a resolution.
    let mut remaining = state
        .directory(directory)
        .cloned()
        .unwrap_or_else(DirectoryVersion::empty)
        .without_entry(contested);
    for (_, entry) in &placements {
        if let Some(name) = remaining.name_of(entry.object_id()) {
            remaining = remaining.without_entry(&name);
        }
    }
    for (name, entry) in &placements {
        if let Some(held) = remaining.entry(name) {
            return Err(Rejection::NameTaken {
                directory,
                name: name.clone(),
                bound_to: held.object_id(),
            });
        }
        remaining = remaining.with_entry(name.clone(), *entry);
    }

    let before = state.directory(directory).cloned();
    if before.as_ref() == Some(&remaining) {
        return Ok(Effect::AlreadyInEffect);
    }
    if let Some(previous) = before {
        for name in previous.entries().keys() {
            state.unbind(directory, name);
        }
    }
    for (name, entry) in remaining.entries() {
        state.bind(directory, name.clone(), *entry);
    }
    Ok(Effect::Applied)
}

fn resolve_content(
    state: &mut WorkspaceState,
    object: ObjectId,
    resulting: VersionId,
    preserved: &[VersionId],
) -> Result<Effect, Rejection> {
    let record = live_object(state, object, ObjectKind::File)?;
    version_of(state, object, resulting)?;
    let mut seen = preserved.to_vec();
    seen.sort_unstable();
    let supplied = seen.len();
    seen.dedup();
    if seen.len() != supplied || seen.contains(&resulting) {
        return Err(Rejection::DuplicateResolution { subject: object });
    }
    // Every superseded version must already be reachable. A resolution that named a version this
    // state has never seen would be claiming to preserve work the state cannot produce.
    for version in preserved {
        version_of(state, object, *version)?;
    }
    if record.current_version() == Some(resulting) {
        return Ok(Effect::AlreadyInEffect);
    }
    state.insert_object(object, record.with_current_version(Some(resulting)));
    Ok(Effect::Applied)
}

fn advance_actor(
    state: &mut WorkspaceState,
    actor: ActorId,
    from: HeadId,
    to: HeadId,
) -> Result<Effect, Rejection> {
    // Continuity is checked only against a head this state has already seen. The head of an actor
    // nobody has heard from is not derivable here — it is a digest over that actor's applied causal
    // set, which is `mesh-state`'s and needs the digest seam this crate does not carry. Checking a
    // claim we cannot derive would mean either believing it or refusing every first advance.
    if let Some(current) = state.actor_head(actor) {
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
    state.set_actor_head(actor, to);
    Ok(Effect::Applied)
}

fn advance_canonical(
    state: &mut WorkspaceState,
    from: HeadId,
    to: HeadId,
    approval: ApprovalId,
) -> Result<Effect, Rejection> {
    if let Some(current) = state.canonical_head() {
        if current.head() != from {
            return Err(Rejection::HeadNotCurrent {
                claimed: from,
                current: current.head(),
            });
        }
        if current == CanonicalAdvance::new(to, approval) {
            return Ok(Effect::AlreadyInEffect);
        }
    }
    state.set_canonical(CanonicalAdvance::new(to, approval));
    Ok(Effect::Applied)
}

/// Every verb this module answers with [`Effect::OutsideStateGraph`], as a checked list rather than
/// a comment. Used by the test below and by `tests/totality.rs`.
#[must_use]
pub const fn verbs_outside_the_state_graph() -> [OperationKind; 4] {
    [
        OperationKind::RecordReadObservation,
        OperationKind::RecordDerivedNode,
        OperationKind::CreateReviewBundle,
        OperationKind::RecordValidation,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ManifestId;

    fn root() -> ObjectId {
        ObjectId::from_bytes([0; 16])
    }

    fn object(byte: u8) -> ObjectId {
        ObjectId::from_bytes([byte; 16])
    }

    fn version(byte: u8) -> VersionId {
        VersionId::from_bytes([byte; 32])
    }

    fn changeset(byte: u8) -> ChangeSetId {
        ChangeSetId::from_bytes([byte; 32])
    }

    fn name(text: &str) -> NormalizedName {
        NormalizedName::new(text).unwrap()
    }

    fn write(object_id: ObjectId, version_id: VersionId) -> Operation {
        Operation::WriteFileVersion {
            object_id,
            version_id,
            parent_versions: vec![],
            manifest_id: ManifestId::from_bytes([9; 32]),
            portable_metadata: PortableMetadata::default(),
        }
    }

    fn run(operations: Vec<Operation>) -> Materialization {
        materialize(
            root(),
            &[AppliedChangeSet::genesis(changeset(1), operations)],
        )
    }

    #[test]
    fn a_file_is_created_written_and_named() {
        let materialized = run(vec![
            Operation::CreateFile {
                object_id: object(1),
            },
            write(object(1), version(1)),
            Operation::LinkDirectoryEntry {
                directory_id: root(),
                name: name("a.txt"),
                object_id: object(1),
                version_id: version(1),
            },
        ]);
        assert_eq!(materialized.rejections(), &[]);
        assert_eq!(materialized.applied(), 3);
        assert_eq!(
            materialized.state().path_of(object(1)),
            Some(vec![name("a.txt")])
        );
    }

    #[test]
    fn a_move_keeps_the_object_and_changes_only_the_path() {
        let materialized = run(vec![
            Operation::CreateDirectory {
                object_id: object(1),
            },
            Operation::CreateFile {
                object_id: object(2),
            },
            write(object(2), version(2)),
            Operation::LinkDirectoryEntry {
                directory_id: root(),
                name: name("src"),
                object_id: object(1),
                version_id: version(1),
            },
            Operation::LinkDirectoryEntry {
                directory_id: root(),
                name: name("a.txt"),
                object_id: object(2),
                version_id: version(2),
            },
            Operation::MoveEntry {
                from_directory_id: root(),
                from_name: name("a.txt"),
                to_directory_id: object(1),
                to_name: name("b.txt"),
                object_id: object(2),
            },
        ]);
        assert_eq!(materialized.rejections(), &[]);
        assert_eq!(
            materialized.state().path_of(object(2)),
            Some(vec![name("src"), name("b.txt")])
        );
        // Object identity survived: same object, same version.
        assert_eq!(
            materialized
                .state()
                .object(object(2))
                .unwrap()
                .current_version(),
            Some(version(2))
        );
    }

    #[test]
    fn a_directory_cannot_become_its_own_ancestor() {
        let materialized = run(vec![
            Operation::CreateDirectory {
                object_id: object(1),
            },
            Operation::CreateDirectory {
                object_id: object(2),
            },
            Operation::LinkDirectoryEntry {
                directory_id: root(),
                name: name("a"),
                object_id: object(1),
                version_id: version(1),
            },
            Operation::LinkDirectoryEntry {
                directory_id: object(1),
                name: name("b"),
                object_id: object(2),
                version_id: version(2),
            },
            Operation::MoveEntry {
                from_directory_id: root(),
                from_name: name("a"),
                to_directory_id: object(2),
                to_name: name("a"),
                object_id: object(1),
            },
        ]);
        assert_eq!(materialized.rejections().len(), 1);
        assert!(matches!(
            materialized.rejections()[0].rejection(),
            Rejection::WouldCycle { .. }
        ));
    }

    #[test]
    fn deletion_preserves_the_versions_and_a_restore_brings_the_object_back() {
        let materialized = run(vec![
            Operation::CreateFile {
                object_id: object(1),
            },
            write(object(1), version(1)),
            Operation::DeleteObject {
                object_id: object(1),
            },
            Operation::RestoreObject {
                object_id: object(1),
                restored_version_id: version(1),
            },
        ]);
        assert_eq!(materialized.rejections(), &[]);
        let record = materialized.state().object(object(1)).unwrap();
        assert!(!record.is_deleted());
        assert_eq!(record.current_version(), Some(version(1)));
        assert!(materialized.state().file_version(version(1)).is_some());
    }

    #[test]
    fn the_root_is_not_a_subject() {
        for operation in [
            Operation::DeleteObject { object_id: root() },
            Operation::CreateDirectory { object_id: root() },
        ] {
            let materialized = run(vec![operation]);
            assert_eq!(materialized.rejections().len(), 1);
        }
    }

    #[test]
    fn the_four_verbs_outside_the_state_graph_write_nothing_and_refuse_nothing() {
        let empty = WorkspaceState::empty(root()).canonical_bytes();
        let materialized = run(vec![
            Operation::RecordReadObservation {
                actor_id: crate::ActorId::from_bytes([1; 32]),
                object_id: object(7),
                version_id: version(7),
                region: crate::ReadRegion::WholeFile,
                confidence: crate::AttributionConfidence::Unknown,
            },
            Operation::RecordDerivedNode {
                node_id: crate::DerivationId::from_bytes([1; 32]),
                node_kind: crate::DerivationKind::TestResult,
                exact_inputs: vec![version(7)],
                configuration_digest: crate::ContentHash::from_bytes([2; 32]),
                output_versions: vec![],
                deterministic: true,
            },
            Operation::CreateReviewBundle {
                bundle_id: crate::ReviewBundleId::from_bytes([3; 32]),
                actor_head: HeadId::from_bytes([4; 32]),
                base_head: HeadId::from_bytes([5; 32]),
            },
            Operation::RecordValidation {
                subject_head: HeadId::from_bytes([4; 32]),
                validator_id: crate::ActorId::from_bytes([6; 32]),
                outcome: crate::ValidationOutcome::Passed,
                evidence: crate::ContentHash::from_bytes([7; 32]),
            },
        ]);
        assert_eq!(materialized.rejections(), &[]);
        assert_eq!(materialized.outside_state_graph(), 4);
        // Every one of them named an object that does not exist, and the state is still the empty
        // state: a read observation is not a state fact even when it is about nothing.
        assert_eq!(materialized.state().canonical_bytes(), empty);
        assert_eq!(verbs_outside_the_state_graph().len(), 4);
    }

    #[test]
    fn a_name_conflict_resolution_preserves_every_contender() {
        let materialized = run(vec![
            Operation::CreateFile {
                object_id: object(1),
            },
            Operation::CreateFile {
                object_id: object(2),
            },
            write(object(1), version(1)),
            write(object(2), version(2)),
            Operation::LinkDirectoryEntry {
                directory_id: root(),
                name: name("notes.md"),
                object_id: object(1),
                version_id: version(1),
            },
            // The concurrent link loses the name, and the object survives unbound.
            Operation::LinkDirectoryEntry {
                directory_id: root(),
                name: name("notes.md"),
                object_id: object(2),
                version_id: version(2),
            },
            Operation::ResolveNameConflict {
                directory_id: root(),
                contested_name: name("notes.md"),
                preserved: vec![
                    PreservedEntry::new(object(1), name("notes.md")),
                    PreservedEntry::new(object(2), name("notes (2).md")),
                ],
            },
        ]);
        assert_eq!(materialized.rejections().len(), 1);
        assert!(matches!(
            materialized.rejections()[0].rejection(),
            Rejection::NameTaken { .. }
        ));
        assert_eq!(
            materialized.state().path_of(object(1)),
            Some(vec![name("notes.md")])
        );
        assert_eq!(
            materialized.state().path_of(object(2)),
            Some(vec![name("notes (2).md")])
        );
    }

    #[test]
    fn an_actor_head_advance_that_claims_the_wrong_predecessor_is_refused() {
        let actor = crate::ids::ActorId::from_bytes([1; 32]);
        let materialized = run(vec![
            Operation::AdvanceActorHead {
                actor_id: actor,
                from_head: HeadId::from_bytes([0; 32]),
                to_head: HeadId::from_bytes([1; 32]),
            },
            Operation::AdvanceActorHead {
                actor_id: actor,
                from_head: HeadId::from_bytes([9; 32]),
                to_head: HeadId::from_bytes([2; 32]),
            },
        ]);
        assert_eq!(materialized.rejections().len(), 1);
        assert_eq!(
            materialized.state().actor_head(actor),
            Some(HeadId::from_bytes([1; 32]))
        );
    }

    #[test]
    fn the_canonical_head_never_moves_without_an_approval_beside_it() {
        let materialized = run(vec![Operation::AdvanceCanonicalHead {
            from_head: HeadId::from_bytes([0; 32]),
            to_head: HeadId::from_bytes([1; 32]),
            approval_id: crate::ids::ApprovalId::from_bytes([5; 32]),
        }]);
        let advance = materialized.state().canonical_head().unwrap();
        assert_eq!(advance.head(), HeadId::from_bytes([1; 32]));
        assert_eq!(
            advance.approval(),
            crate::ids::ApprovalId::from_bytes([5; 32])
        );
    }

    #[test]
    fn every_operation_is_reached_exactly_once() {
        let materialized = run(vec![
            Operation::CreateFile {
                object_id: object(1),
            },
            Operation::CreateFile {
                object_id: object(1),
            },
            write(object(1), version(1)),
            write(object(1), version(1)),
        ]);
        assert_eq!(materialized.reached(), 4);
        assert_eq!(materialized.already_in_effect(), 1);
        assert_eq!(materialized.rejections().len(), 1);
    }
}
