//! "The database path is excluded from workspace materialization by construction, not by an
//! ignore rule."
//!
//! # What an ignore rule would look like, and why it is not what is here
//!
//! An ignore rule is a filter over names that were already expressible: the materializer could
//! name `metadata.sqlite`, and something remembers to skip it. Two failures follow it around. A
//! second producer of names forgets the filter. And the filter is written for the database file
//! and not for `-wal` and `-shm`, which between them hold the most recently written pages.
//!
//! What this crate does instead: the materializer is handed a [`MountRoot`], and the only way to
//! name content is a [`WorkspaceRelativePath`], which refuses every component that could resolve
//! outside that root. The database is the mount root's *sibling*. There is no filter because there
//! is no name to filter.
//!
//! This file proves that against a real filesystem rather than against string arithmetic: a real
//! workspace directory, a real WAL database with its real sidecars, real files materialized
//! through the type, and a real directory walk over the mount root afterwards.

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use common::{HeldWriter, Sqlite3, TempDir};
use mesh_store::{
    Checkpoint, MountRoot, OperationRecord, PathError, RecordDigest, ResolutionError, Store,
    WorkspaceRelativePath, WorkspaceRoot, DATABASE_FILE_NAME, MOUNT_DIRECTORY_NAME,
};

fn digest(seed: u8) -> RecordDigest {
    RecordDigest::from_bytes([seed; 32])
}

/// Every file under `root`, recursively.
fn walk(root: &Path) -> BTreeSet<PathBuf> {
    let mut found = BTreeSet::new();
    let Ok(entries) = fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.insert(path);
        }
    }
    found
}

/// A real workspace on disk: `~/.mesh/workspaces/<id>` with a WAL database and a mount root.
struct Workspace {
    _directory: TempDir,
    root: WorkspaceRoot,
}

impl Workspace {
    fn new(label: &str) -> Self {
        let directory = TempDir::new(label);
        let root = WorkspaceRoot::new(directory.path());
        fs::create_dir_all(root.mount_root().as_path()).expect("the mount root is created");

        let mut store = Store::open(Sqlite3::at(root.database().as_path())).expect("opens");
        store
            .commit(&Checkpoint {
                operations: vec![OperationRecord {
                    id: digest(1),
                    actor: digest(2),
                    actor_sequence: 1,
                    hlc_millis: 1,
                    hlc_counter: 0,
                    policy_epoch: 1,
                    session: mesh_store::no_session(),
                    payload_digest: digest(3),
                    parents: Vec::new(),
                }],
                ..Checkpoint::default()
            })
            .expect("commits");

        Self {
            _directory: directory,
            root,
        }
    }

    fn mounts(&self) -> MountRoot {
        self.root.mount_root()
    }

    /// Materialize content the only way the types allow.
    fn materialize(&self, relative: &str, contents: &str) -> PathBuf {
        let path = WorkspaceRelativePath::new(relative).expect("legal workspace content");
        let target = self
            .mounts()
            .resolve(&path)
            .expect("the path resolves inside the mount root")
            .into_path_buf();
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).expect("the parent directory is created");
        }
        fs::write(&target, contents).expect("the file is written");
        target
    }
}

/// The layout is plan §6.2's, on a real filesystem.
#[test]
fn the_database_is_the_mount_roots_sibling_not_its_descendant() {
    let workspace = Workspace::new("layout");
    let database = workspace.root.database();

    assert_eq!(
        database.as_path().file_name().and_then(|n| n.to_str()),
        Some(DATABASE_FILE_NAME)
    );
    assert_eq!(
        workspace
            .mounts()
            .as_path()
            .file_name()
            .and_then(|n| n.to_str()),
        Some(MOUNT_DIRECTORY_NAME)
    );
    assert_eq!(
        database.as_path().parent(),
        workspace.mounts().as_path().parent()
    );
    assert!(database.as_path().exists(), "the database was not created");
}

/// The criterion, against a real directory walk: after materializing content — including content
/// deliberately named after the database — nothing under the mount root is a database file.
#[test]
fn no_database_file_ever_appears_under_the_mount_root() {
    let workspace = Workspace::new("walk");

    // Content designed to collide by name if the exclusion were name-based.
    workspace.materialize(DATABASE_FILE_NAME, "not the database");
    workspace.materialize("metadata.sqlite-wal", "not the log either");
    workspace.materialize("metadata.sqlite-shm", "nor the shared memory");
    workspace.materialize("src/main.rs", "fn main() {}");
    workspace.materialize("deeply/nested/dir/file.txt", "content");

    // Hold a write transaction open so the -wal and -shm files definitely exist during the walk.
    let writer = HeldWriter::begin(
        workspace.root.database().as_path(),
        "UPDATE operation SET policy_epoch = 5;",
    )
    .expect("the writer holds");

    let database_files: BTreeSet<PathBuf> = workspace.root.database().files().into_iter().collect();
    let existing: Vec<&PathBuf> = database_files.iter().filter(|p| p.exists()).collect();
    assert_eq!(
        existing.len(),
        3,
        "all three database files should exist while a transaction is open: {existing:?}"
    );

    let materialized = walk(workspace.mounts().as_path());
    assert!(!materialized.is_empty(), "nothing was materialized");

    for file in &database_files {
        assert!(
            !materialized.contains(file),
            "{} appeared under the mount root",
            file.display()
        );
        assert!(
            !workspace.mounts().contains(file),
            "{} is inside the mount root",
            file.display()
        );
    }
    writer.commit().expect("the writer commits");

    println!(
        "{} materialized files, {} database files, no overlap",
        materialized.len(),
        database_files.len()
    );
}

/// The sharpest case. A workspace file *called* `metadata.sqlite` is legal content and resolves to
/// a different file from the store's own database. A name-based ignore rule would either leak the
/// database or forbid the user a perfectly ordinary filename; the structural exclusion does
/// neither.
#[test]
fn a_workspace_file_named_like_the_database_is_a_different_file() {
    let workspace = Workspace::new("collision");
    let materialized = workspace.materialize(DATABASE_FILE_NAME, "user content");

    assert_ne!(materialized, workspace.root.database().as_path());
    assert_eq!(
        fs::read_to_string(&materialized).expect("readable"),
        "user content"
    );
    // The real database is untouched and still a database.
    let executor = Sqlite3::at(workspace.root.database().as_path());
    assert_eq!(
        executor.journal_mode().expect("the mode is readable"),
        "wal"
    );
}

/// Every escape a materializer could be asked for, refused by the constructor. None of these is
/// filtered later; none of them becomes a path at all.
#[test]
fn no_relative_path_can_be_constructed_that_reaches_the_database() {
    let workspace = Workspace::new("escape");
    let database = workspace.root.database();

    let attempts = [
        "../metadata.sqlite",
        "../metadata.sqlite-wal",
        "../metadata.sqlite-shm",
        "../../metadata.sqlite",
        "a/../../metadata.sqlite",
        "a/b/c/../../../../metadata.sqlite",
        "./../metadata.sqlite",
        "/tmp/metadata.sqlite",
    ];

    for attempt in attempts {
        let result = WorkspaceRelativePath::new(attempt);
        assert!(
            result.is_err(),
            "{attempt:?} was accepted as workspace content"
        );
        assert!(matches!(
            result.unwrap_err(),
            PathError::ParentComponent
                | PathError::Absolute
                | PathError::CurrentComponent
                | PathError::Prefix
        ));
    }

    // And the absolute path of the database is not reachable through the mount root either.
    assert!(!workspace.mounts().contains(database.as_path()));
}

/// A generated sweep rather than a hand-picked list: every accepted path resolves inside the mount
/// root, for a few thousand shapes.
#[test]
fn every_acceptable_path_resolves_inside_the_mount_root() {
    let workspace = Workspace::new("sweep");
    let mounts = workspace.mounts();
    let database_files: Vec<PathBuf> = workspace.root.database().files();

    let segments = [
        "a",
        "b",
        "metadata.sqlite",
        "metadata.sqlite-wal",
        "..hidden",
        "with space",
        "..",
        ".",
        "",
        "-",
        "mounts",
    ];

    let mut accepted = 0usize;
    let mut refused = 0usize;
    for first in segments {
        for second in segments {
            for third in segments {
                let candidate = format!("{first}/{second}/{third}");
                match WorkspaceRelativePath::new(&candidate) {
                    Ok(relative) => {
                        accepted += 1;
                        let resolved = mounts
                            .resolve(&relative)
                            .expect("an accepted name resolves inside a mount root with no links");
                        assert!(
                            mounts.contains(resolved.as_path()),
                            "{candidate:?} resolved to {} which is outside the mount root",
                            resolved.as_path().display()
                        );
                        for file in &database_files {
                            assert_ne!(
                                resolved.as_path(),
                                file.as_path(),
                                "{candidate:?} named a database file"
                            );
                        }
                    }
                    Err(_) => refused += 1,
                }
            }
        }
    }
    assert!(
        accepted > 0 && refused > 0,
        "the sweep exercised only one branch"
    );
    println!(
        "{accepted} accepted, {refused} refused across {} shapes",
        accepted + refused
    );
}

/// The type the materializer holds is not the workspace root, so the database's own directory is
/// not reachable from it either.
#[test]
fn the_materializer_is_not_handed_the_workspace_root() {
    let workspace = Workspace::new("handoff");
    let mounts = workspace.mounts();

    // A MountRoot resolves only WorkspaceRelativePaths, and the workspace directory itself is not
    // one of them: it is the mount root's parent, which no accepted path can reach.
    assert!(WorkspaceRelativePath::new("..").is_err());
    assert!(!mounts.contains(workspace.root.as_path()));
    assert!(mounts.contains(mounts.as_path()));
}

// ---------------------------------------------------------------------------------------------
// 01KZDR7VD4860QZJBGJXFJP0EQ / threat model §8 F6. The rules above are applied to the *spelling*
// of a path, and a symbolic link is not part of a spelling. Every test below plants a real link
// inside a real mount root — which a designed writer inside the mount is allowed to do — proves
// the link genuinely reaches its target through the mount root, and then requires resolution to
// refuse it. Each was observed failing before `MountRoot::resolve` walked the tree.
// ---------------------------------------------------------------------------------------------

/// Read whatever is at `mounts/<relative>` by joining the strings, the way the type did before it
/// resolved anything. This is the attacker's view, and it is what the assertions below measure the
/// refusal against: a test that only checked for an error could pass against a link that never
/// pointed anywhere.
#[cfg(unix)]
fn read_through_the_mount_root(mounts: &MountRoot, relative: &str) -> Vec<u8> {
    fs::read(mounts.as_path().join(relative)).unwrap_or_default()
}

/// `mounts/up -> ..` and then `up/metadata.sqlite`: the database, addressed through the mount root
/// by a path that spells no escape.
#[cfg(unix)]
#[test]
fn a_link_planted_inside_the_mount_root_cannot_reach_the_database() {
    let workspace = Workspace::new("planted-parent-link");
    std::os::unix::fs::symlink("..", workspace.mounts().as_path().join("up"))
        .expect("a writer inside the mount plants a link");

    // The name is legal: no `..`, no root, no prefix, no NUL, one spelling.
    let relative = WorkspaceRelativePath::new("up/metadata.sqlite")
        .expect("the lexical rules cannot see a link");

    // And it really does reach the database, so the refusal below is refusing something.
    let reachable = read_through_the_mount_root(&workspace.mounts(), "up/metadata.sqlite");
    assert!(
        reachable.starts_with(b"SQLite format 3\0"),
        "the planted link does not reach the database, so this test proves nothing"
    );

    assert_eq!(
        workspace.mounts().resolve(&relative),
        Err(ResolutionError::SymbolicLinkComponent {
            component: PathBuf::from("up")
        })
    );
    // Including the sidecars, which hold the most recently written pages.
    for sidecar in ["up/metadata.sqlite-wal", "up/metadata.sqlite-shm"] {
        let relative = WorkspaceRelativePath::new(sidecar).expect("legal spelling");
        assert!(workspace.mounts().resolve(&relative).is_err(), "{sidecar}");
    }
}

/// The same shape reaching outside the workspace entirely: the link *target* holds `..`, the path
/// handed to the constructor does not.
#[cfg(unix)]
#[test]
fn a_link_planted_inside_the_mount_root_cannot_reach_outside_the_workspace() {
    let outside = TempDir::new("outside-the-workspace");
    fs::write(outside.join("key.txt"), "secret").expect("the file outside is written");
    let outside_name = outside
        .path()
        .file_name()
        .expect("the temporary directory has a name")
        .to_owned();

    let workspace = Workspace::new("planted-escape-link");
    let mut target = PathBuf::from("../..");
    target.push(&outside_name);
    std::os::unix::fs::symlink(&target, workspace.mounts().as_path().join("link"))
        .expect("a writer inside the mount plants a link");

    let relative = WorkspaceRelativePath::new("link/key.txt").expect("the spelling holds no `..`");
    assert_eq!(
        read_through_the_mount_root(&workspace.mounts(), "link/key.txt"),
        b"secret",
        "the planted link does not reach outside, so this test proves nothing"
    );

    assert_eq!(
        workspace.mounts().resolve(&relative),
        Err(ResolutionError::SymbolicLinkComponent {
            component: PathBuf::from("link")
        })
    );
}

/// The link as the *last* component rather than an interior one. Every component is inspected,
/// including the leaf, because the leaf is what gets opened.
#[cfg(unix)]
#[test]
fn a_link_that_is_the_final_component_is_refused_too() {
    let workspace = Workspace::new("planted-leaf-link");
    let target = PathBuf::from("..").join(DATABASE_FILE_NAME);
    std::os::unix::fs::symlink(&target, workspace.mounts().as_path().join("db"))
        .expect("a writer inside the mount plants a link");

    assert!(
        read_through_the_mount_root(&workspace.mounts(), "db").starts_with(b"SQLite format 3\0"),
        "the planted link does not reach the database, so this test proves nothing"
    );

    let relative = WorkspaceRelativePath::new("db").expect("one plain component");
    assert_eq!(
        workspace.mounts().resolve(&relative),
        Err(ResolutionError::SymbolicLinkComponent {
            component: PathBuf::from("db")
        })
    );
}

/// Nested rather than at the top: the link is three directories down, and the path that walks
/// through it is longer than the one that plants it.
#[cfg(unix)]
#[test]
fn a_link_nested_deep_inside_the_mount_root_is_refused() {
    let workspace = Workspace::new("planted-nested-link");
    let nest = workspace.mounts().as_path().join("a/b/c");
    fs::create_dir_all(&nest).expect("the nest is created");
    // Four `..`: out of `c`, `b`, `a` and then out of the mount root itself.
    std::os::unix::fs::symlink("../../../..", nest.join("out"))
        .expect("a writer inside the mount plants a link");

    assert!(
        read_through_the_mount_root(&workspace.mounts(), "a/b/c/out/metadata.sqlite")
            .starts_with(b"SQLite format 3\0"),
        "the planted link does not reach the database, so this test proves nothing"
    );

    let relative =
        WorkspaceRelativePath::new("a/b/c/out/metadata.sqlite").expect("no `..` in the spelling");
    assert_eq!(
        workspace.mounts().resolve(&relative),
        Err(ResolutionError::SymbolicLinkComponent {
            component: PathBuf::from("a/b/c/out")
        })
    );
    // The unlinked siblings of the same tree still resolve, so the refusal is the link and not the
    // depth.
    let sibling = WorkspaceRelativePath::new("a/b/c/ordinary.txt").expect("legal");
    assert!(workspace.mounts().resolve(&sibling).is_ok());
}

/// The escape that only bites on *write*: a link whose target does not exist yet. Materialization
/// creates files, and creating a file through a dangling link creates the target — outside the
/// workspace. This is the case that separates `symlink_metadata` from `metadata`: the latter
/// follows the link, reports `NotFound` for the missing target, and a resolver that reads that as
/// "nothing is there, so nothing can be hidden below it" hands back a path that writes outside.
#[cfg(unix)]
#[test]
fn a_dangling_link_is_refused_although_nothing_exists_at_its_target() {
    let outside = TempDir::new("dangling-target");
    let workspace = Workspace::new("planted-dangling-link");
    let target = outside.join("planted.txt");
    assert!(!target.exists(), "the target must not exist yet");
    std::os::unix::fs::symlink(&target, workspace.mounts().as_path().join("new.txt"))
        .expect("a writer inside the mount plants a link");

    let relative = WorkspaceRelativePath::new("new.txt").expect("one plain component");
    assert_eq!(
        workspace.mounts().resolve(&relative),
        Err(ResolutionError::SymbolicLinkComponent {
            component: PathBuf::from("new.txt")
        })
    );

    // What a resolver that followed the link would have handed back, exercised to show the write
    // really does land outside: nothing in this crate performs it.
    fs::write(workspace.mounts().as_path().join("new.txt"), "written").expect("the write follows");
    assert_eq!(
        fs::read_to_string(&target).expect("the target now exists"),
        "written",
        "writing through the planted link did not escape, so this test proves nothing"
    );
}

/// The mount root itself replaced by a link. Canonicalizing an unchecked root would adopt its
/// target as the definition of "inside the mount root", and `mounts -> /` would then make the
/// whole filesystem workspace content.
#[cfg(unix)]
#[test]
fn a_mount_root_that_is_itself_a_link_resolves_nothing() {
    let elsewhere = TempDir::new("root-link-target");
    fs::write(elsewhere.join("key.txt"), "secret").expect("the file outside is written");

    // A workspace directory whose `mounts` entry is a link rather than a directory.
    let directory = TempDir::new("root-is-a-link");
    let root = WorkspaceRoot::new(directory.path());
    std::os::unix::fs::symlink(elsewhere.path(), root.mount_root().as_path()).expect("plants");

    let relative = WorkspaceRelativePath::new("key.txt").expect("legal");
    assert_eq!(
        read_through_the_mount_root(&root.mount_root(), "key.txt"),
        b"secret",
        "the planted root link does not reach outside, so this test proves nothing"
    );
    assert_eq!(
        root.mount_root().resolve(&relative),
        Err(ResolutionError::MountRootIsSymbolicLink)
    );
}
