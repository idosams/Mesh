//! The comparison surface: everything two materializations must agree about, as plain data.
//!
//! The oracle does not share a type, a collection or an encoder with the implementation — that is
//! what makes it worth having — so the two cannot be compared by `==` on a state. They are compared
//! by projecting both onto this structure, which is built only from the primitives both can
//! produce: identifiers, names, booleans and rendered rejection text.
//!
//! Everything derived is in here too, deliberately. The parent index and the path of every object
//! are *computed* on the real side and *rescanned* on the oracle side, so a bug that kept the index
//! and the entry maps out of step would show up as a snapshot difference rather than hiding behind
//! an accessor that reads the same wrong value twice.

use std::collections::BTreeMap;

use mesh_materializer::{Materialization, ObjectKind, WorkspaceState};

/// One object, as both sides can describe it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectFacet {
    pub id: String,
    pub kind: String,
    pub created_by: Option<String>,
    pub deleted: bool,
    pub current_version: Option<String>,
    pub parent: Option<String>,
    pub path: Option<Vec<String>>,
}

/// One name binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryFacet {
    pub directory: String,
    pub name: String,
    pub object: String,
    pub version: String,
}

/// One file version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionFacet {
    pub id: String,
    pub object: String,
    pub parents: Vec<String>,
    pub manifest: String,
    pub executable: bool,
    pub created_by: String,
}

/// Everything two materializations must agree about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub root: String,
    pub objects: Vec<ObjectFacet>,
    pub entries: Vec<EntryFacet>,
    pub versions: Vec<VersionFacet>,
    pub actor_heads: Vec<(String, String)>,
    pub canonical: Option<(String, String)>,
    pub rejections: Vec<String>,
    pub applied: usize,
    pub already_in_effect: usize,
    pub outside_state_graph: usize,
}

impl Snapshot {
    /// The first field the two snapshots disagree about, rendered for a failure message.
    ///
    /// A whole-structure `assert_eq!` on a state with sixty objects prints two screens of `Debug`
    /// and leaves the reader to diff them. This says which field, and shows only that field.
    pub fn first_difference(&self, other: &Self) -> Option<String> {
        fn differ<T: std::fmt::Debug + PartialEq>(
            label: &str,
            mine: &T,
            theirs: &T,
        ) -> Option<String> {
            (mine != theirs).then(|| {
                format!("{label}\n  implementation: {mine:?}\n  oracle:         {theirs:?}")
            })
        }

        differ("root", &self.root, &other.root)
            .or_else(|| differ("objects", &self.objects, &other.objects))
            .or_else(|| differ("entries", &self.entries, &other.entries))
            .or_else(|| differ("versions", &self.versions, &other.versions))
            .or_else(|| differ("actor heads", &self.actor_heads, &other.actor_heads))
            .or_else(|| differ("canonical head", &self.canonical, &other.canonical))
            .or_else(|| differ("rejections", &self.rejections, &other.rejections))
            .or_else(|| differ("applied count", &self.applied, &other.applied))
            .or_else(|| {
                differ(
                    "already-in-effect count",
                    &self.already_in_effect,
                    &other.already_in_effect,
                )
            })
            .or_else(|| {
                differ(
                    "outside-state-graph count",
                    &self.outside_state_graph,
                    &other.outside_state_graph,
                )
            })
    }
}

/// The snapshot of a real materialization.
pub fn of_materialization(materialized: &Materialization) -> Snapshot {
    let state = materialized.state();
    Snapshot {
        root: state.root().to_string(),
        objects: objects_of(state),
        entries: entries_of(state),
        versions: versions_of(state),
        actor_heads: state
            .actor_heads()
            .iter()
            .map(|(actor, head)| (actor.to_string(), head.to_string()))
            .collect(),
        canonical: state
            .canonical_head()
            .map(|advance| (advance.head().to_string(), advance.approval().to_string())),
        rejections: materialized
            .rejections()
            .iter()
            .map(ToString::to_string)
            .collect(),
        applied: materialized.applied(),
        already_in_effect: materialized.already_in_effect(),
        outside_state_graph: materialized.outside_state_graph(),
    }
}

fn objects_of(state: &WorkspaceState) -> Vec<ObjectFacet> {
    state
        .objects()
        .iter()
        .map(|(id, record)| ObjectFacet {
            id: id.to_string(),
            kind: match record.kind() {
                ObjectKind::File => "File".to_owned(),
                ObjectKind::Directory => "Directory".to_owned(),
            },
            created_by: record.created_by().map(|changeset| changeset.to_string()),
            deleted: record.is_deleted(),
            current_version: record.current_version().map(|version| version.to_string()),
            parent: state.parent_of(*id).map(|parent| parent.to_string()),
            path: state.path_of(*id).map(|names| {
                names
                    .iter()
                    .map(|name| name.as_str().to_owned())
                    .collect::<Vec<String>>()
            }),
        })
        .collect()
}

fn entries_of(state: &WorkspaceState) -> Vec<EntryFacet> {
    let mut entries: Vec<EntryFacet> = Vec::new();
    for (directory, version) in state.directories() {
        for (name, entry) in version.entries() {
            entries.push(EntryFacet {
                directory: directory.to_string(),
                name: name.as_str().to_owned(),
                object: entry.object_id().to_string(),
                version: entry.version_id().to_string(),
            });
        }
    }
    sort_entries(&mut entries);
    entries
}

fn versions_of(state: &WorkspaceState) -> Vec<VersionFacet> {
    state
        .file_versions()
        .iter()
        .map(|(id, version)| VersionFacet {
            id: id.to_string(),
            object: version.object_id().to_string(),
            parents: version
                .parent_versions()
                .iter()
                .map(ToString::to_string)
                .collect(),
            manifest: version.manifest_id().to_string(),
            executable: version.portable_metadata().is_executable(),
            created_by: version.created_by().to_string(),
        })
        .collect()
}

/// Sort bindings the same way on both sides, so a difference is a difference and not an ordering.
pub fn sort_entries(entries: &mut [EntryFacet]) {
    entries.sort_by(|one, other| (&one.directory, &one.name).cmp(&(&other.directory, &other.name)));
}

/// The directory-to-object index a full rescan of the bindings produces.
///
/// Used by `tests/state_invariants.rs` to check the real state's cached index against a recount.
pub fn rebuilt_parent_index(state: &WorkspaceState) -> BTreeMap<String, String> {
    let mut index = BTreeMap::new();
    for (directory, version) in state.directories() {
        for entry in version.entries().values() {
            index.insert(entry.object_id().to_string(), directory.to_string());
        }
    }
    index
}
