//! Where the database lives, and why no workspace path can ever name it.
//!
//! # The criterion, and what "by construction" has to mean to be worth anything
//!
//! Plan §6.1: "the SQLite database itself must never be replicated as workspace state." The
//! obvious implementation is an ignore rule — put `metadata.sqlite` on a list the materializer
//! skips. That is exactly what this module refuses to do, for one reason: an ignore rule is a
//! *filter applied to a name that was already expressible*. Somebody has to remember to apply it,
//! it has to be applied at every producer of names, and the day one producer forgets, the database
//! is workspace content and nothing says so.
//!
//! So the exclusion is structural instead. Plan §6.2's layout puts the database and the mount
//! point side by side under the workspace root:
//!
//! ```text
//! ~/.mesh/workspaces/<workspace-id>/
//!   metadata.sqlite        <- DatabasePath
//!   mounts/                <- MountRoot, the only thing materialization is ever handed
//! ```
//!
//! Materialization is handed a [`MountRoot`] and resolves [`WorkspaceRelativePath`]s against it.
//! `WorkspaceRelativePath` rejects every component that could climb out — `..`, a leading
//! separator, a drive prefix, a NUL — so the set of paths materialization can *name* is exactly
//! the subtree under `mounts/`, and the database is not in that subtree. There is no rule to
//! forget because there is no name to filter.
//!
//! # Naming is not resolution, and a link is not part of a spelling
//!
//! That paragraph is about *names*, and about names it is unconditional.
//! [`WorkspaceRelativePath::new`] never touches the filesystem, so it cannot see a symbolic link —
//! and a symbolic link is not part of a path's spelling. A path that spells no escape can still
//! walk out of the mount root through one, and a writer inside the mount is exactly who plants it.
//!
//! Measured against the rules this module enforces (`01KZDR7VD4860QZJBGJXFJP0EQ`, threat model §8
//! F6): with `mounts/up -> ..` planted inside the mount, `up/metadata.sqlite` satisfies every rule
//! the constructor applies and `mounts/up/metadata.sqlite` opens the metadata database — the bytes
//! read back through the mount root began `SQLite format 3\0`. With `mounts/link -> ../../outside`
//! — a target holding `..`, a *path* holding none — `link/key.txt` read a file outside the
//! workspace. Both are tests in `tests/materialization_exclusion.rs`, and both were observed
//! failing before [`MountRoot::resolve`] walked anything.
//!
//! So the exclusion has two halves, enforced in two different places:
//!
//! | half | what it rules out | where | what it costs | conditions |
//! |---|---|---|---|---|
//! | naming | a path that *spells* an escape | [`WorkspaceRelativePath::new`] | no syscall | none |
//! | resolution | a path that *walks* out through a link | [`MountRoot::resolve`] | one `lstat` per component that exists, plus one `canonicalize` | the three below |
//!
//! [`MountRoot::resolve`] is therefore fallible, and [`ResolvedPath`] — the only absolute path
//! this module hands out — cannot be obtained without that walk having happened. A caller holding
//! one is not trusting a filter it had to remember to apply; there is no other constructor.
//!
//! # The three conditions resolution still depends on, stated rather than assumed
//!
//! 1. **Time of check is not time of use.** The walk describes the tree as it was during the walk.
//!    A component replaced by a link between [`MountRoot::resolve`] returning and the caller
//!    opening the path is not visible to it. Closing that gap needs the open itself to refuse to
//!    traverse — `O_NOFOLLOW` per component, or an openat-relative resolution that cannot escape a
//!    directory descriptor — which belongs to whoever performs the open, not to this module.
//! 2. **Only symbolic links are visible to it.** A hard link to a file outside the mount, a bind
//!    mount, a mounted filesystem grafted under the mount root and (on Windows) a directory
//!    junction are not symbolic links. The `canonicalize` cross-check catches the junction and the
//!    grafted mount, because it resolves reparse points and mount points; it does not catch a hard
//!    link, because a hard link *is* the file and no walk can tell it apart from the original.
//! 3. **The workspace directory above the mount root is the store's own.** `resolve` refuses when
//!    the mount root itself is a symbolic link, but it canonicalizes the root before comparing, so
//!    symbolic links *above* the workspace directory are followed — which is what makes the
//!    comparison work at all on a platform whose temporary directory is reached through one. An
//!    attacker who can rewrite the workspace directory's own ancestry has already replaced the
//!    store.
//!
//! # Which layer owns resolution
//!
//! Written down once, here, so that it is not assumed by both layers and implemented by neither.
//! `mesh-store` owns refusal for the paths *it* resolves: this module, [`MountRoot::resolve`], the
//! two halves above and conditions 2 and 3. `01KZC311W6HMM9DHTV6N1FHDV3` owns everything a mount
//! adapter needs beyond that — condition 1's race-free open, the adversarial corpus across
//! adapters, and making a refused resolution observable in the ledger rather than silent. The
//! threat model's §10 G7 row names that task as the owner of the remainder and this finding as the
//! reason. **This module's guarantee stops exactly where [`ResolvedPath`] is handed over**, and a
//! downstream lane that needs it to hold past that point is reading the wrong crate.
//!
//! # The three files, not the one
//!
//! A WAL database is three files: `metadata.sqlite`, `metadata.sqlite-wal` and
//! `metadata.sqlite-shm`. An ignore rule written for the first and not the other two leaks the
//! most recently written pages, which is the freshest data in the store. [`DatabasePath::files`]
//! returns all three and `tests/materialization_exclusion.rs` checks the disjointness for each.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// The file name of the metadata database inside a workspace directory, from plan §6.2.
pub const DATABASE_FILE_NAME: &str = "metadata.sqlite";

/// The directory materialized workspace content is written under, from plan §6.2.
pub const MOUNT_DIRECTORY_NAME: &str = "mounts";

/// Why a workspace-relative path was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathError {
    /// The path was empty, which names the mount root itself rather than content within it.
    Empty,
    /// The path was absolute, so it would not have been resolved against the mount root at all.
    Absolute,
    /// The path held a `..` component, the one component that can leave the subtree.
    ParentComponent,
    /// The path began with a `.` component, which names the mount root by a second spelling.
    CurrentComponent,
    /// The path held a prefix component such as a Windows drive letter.
    Prefix,
    /// The path held a NUL byte, which no filesystem accepts and which truncates a C string.
    InteriorNul,
    /// The path names a real file, but by a second spelling: an interior `.`, a doubled separator
    /// or a trailing one. Two spellings of one path mean two index keys for one object, and the
    /// index would then disagree with itself about how many objects exist.
    NonCanonicalSpelling {
        /// The single spelling this path should have used.
        canonical: PathBuf,
    },
}

impl core::fmt::Display for PathError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => {
                formatter.write_str("a workspace path names content, never the mount root itself")
            }
            Self::Absolute => formatter.write_str("a workspace path is relative to the mount root"),
            Self::ParentComponent => {
                formatter.write_str("a workspace path may not climb out of the mount root")
            }
            Self::CurrentComponent => {
                formatter.write_str("a workspace path has one spelling, so no leading `.`")
            }
            Self::Prefix => formatter.write_str("a workspace path carries no filesystem prefix"),
            Self::InteriorNul => formatter.write_str("a workspace path holds no NUL byte"),
            Self::NonCanonicalSpelling { canonical } => write!(
                formatter,
                "a workspace path has one spelling; this one should be written {}",
                canonical.display()
            ),
        }
    }
}

impl std::error::Error for PathError {}

/// A path that names workspace content, relative to a [`MountRoot`].
///
/// The type is the **naming** half of the exclusion. Constructing one is the only way to name
/// materializable content, and the constructor refuses every *spelling* that could resolve outside
/// the mount root — so the database is not among the paths anything can name, and no ignore rule
/// is needed to say so.
///
/// It says nothing about what a name resolves *to*. The check is lexical and makes no syscall, so
/// it cannot see a symbolic link: under a planted `mounts/up -> ..`, `up/metadata.sqlite` passes
/// every rule here. That is refused by [`MountRoot::resolve`], and the module documentation states
/// the split and the conditions the resolving half still depends on.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceRelativePath(PathBuf);

impl WorkspaceRelativePath {
    /// Check a relative path.
    ///
    /// # Errors
    ///
    /// [`PathError`] naming the component that made the path unusable.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, PathError> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() {
            return Err(PathError::Empty);
        }
        if path.to_string_lossy().contains('\0') {
            return Err(PathError::InteriorNul);
        }

        let mut canonical = PathBuf::new();
        for component in path.components() {
            match component {
                Component::Prefix(_) => return Err(PathError::Prefix),
                Component::RootDir => return Err(PathError::Absolute),
                Component::ParentDir => return Err(PathError::ParentComponent),
                Component::CurDir => return Err(PathError::CurrentComponent),
                Component::Normal(part) => canonical.push(part),
            }
        }
        if canonical.as_os_str().is_empty() {
            return Err(PathError::Empty);
        }
        // `Path::components` normalizes an interior `.`, a doubled separator and a trailing one
        // away, so a path that survived the walk above can still be a second spelling of a path
        // that is already legal. Comparing against the rebuilt canonical form is what catches it —
        // and the comparison is over the raw `OsStr`, because `Path`'s own `PartialEq` compares
        // components and would therefore call every one of those spellings equal.
        if canonical.as_os_str() != path.as_os_str() {
            return Err(PathError::NonCanonicalSpelling { canonical });
        }
        Ok(Self(canonical))
    }

    /// The relative path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// The components, each of which is a plain name.
    ///
    /// The filter is a no-op: [`Self::new`] pushes nothing but `Component::Normal` into the stored
    /// path, so no other variant can appear. It is written as a filter rather than as a panic
    /// because if the invariant ever broke, dropping the component makes the walk *shorter* — one
    /// step nearer the mount root — where unwrapping would abort the process and asserting the
    /// variant would let a `..` through into a `join`.
    fn parts(&self) -> impl Iterator<Item = &OsStr> {
        self.0.components().filter_map(|component| match component {
            Component::Normal(part) => Some(part),
            _ => None,
        })
    }
}

/// A workspace's own directory: `~/.mesh/workspaces/<workspace-id>`.
///
/// It hands out a [`MountRoot`] and a [`DatabasePath`] and nothing that is both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceRoot(PathBuf);

impl WorkspaceRoot {
    /// Name a workspace directory.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// The directory itself.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Where materialized content goes.
    #[must_use]
    pub fn mount_root(&self) -> MountRoot {
        MountRoot(self.0.join(MOUNT_DIRECTORY_NAME))
    }

    /// Where the metadata database goes.
    #[must_use]
    pub fn database(&self) -> DatabasePath {
        DatabasePath(self.0.join(DATABASE_FILE_NAME))
    }
}

/// The root materialized workspace content is written under.
///
/// Deliberately a distinct type from [`WorkspaceRoot`]: materialization is handed this and never
/// the workspace directory, so the database's parent is not reachable from what materialization
/// holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountRoot(PathBuf);

impl MountRoot {
    /// The directory itself.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Resolve workspace content to an absolute path, against the filesystem rather than against
    /// the spelling of the path.
    ///
    /// [`WorkspaceRelativePath`] has already refused every spelling that could climb out. This
    /// refuses what a spelling cannot express: it walks the components one at a time and refuses
    /// the first that is a symbolic link, then canonicalizes the deepest component that exists and
    /// requires the result to lie under the canonical mount root. Components that do not exist yet
    /// are not walked past — nothing can be below a component that is not there — which is the
    /// ordinary case for materializing a new file.
    ///
    /// A link is refused even when its target is *inside* the mount root. Two names for one object
    /// is the same defect [`PathError::NonCanonicalSpelling`] exists to refuse: it gives the index
    /// two keys for one file and the store then disagrees with itself about how many objects it
    /// holds.
    ///
    /// The returned [`ResolvedPath`] is spelled from this root as this root was given, not from
    /// its canonical form: the walk proved the two reach the same directory, and a caller that
    /// logs the path should see the path the workspace uses.
    ///
    /// # Errors
    ///
    /// [`ResolutionError`], naming the component that made the path unusable. The module
    /// documentation states the three conditions a successful resolution still depends on — in
    /// particular that this is a check at resolution time and not at the time of the open.
    pub fn resolve(
        &self,
        relative: &WorkspaceRelativePath,
    ) -> Result<ResolvedPath, ResolutionError> {
        let canonical_root = self.canonical_root()?;

        // Two paths are built in step: `walked` under the canonical root, which is what is
        // inspected, and `resolved` under this root as given, which is what is handed back.
        let mut walked = canonical_root.clone();
        let mut resolved = self.0.clone();
        let mut component = PathBuf::new();
        let mut deepest_existing = (canonical_root.clone(), PathBuf::new());
        let mut still_exists = true;

        for part in relative.parts() {
            resolved.push(part);
            component.push(part);
            if !still_exists {
                continue;
            }
            walked.push(part);
            match fs::symlink_metadata(&walked) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(ResolutionError::SymbolicLinkComponent { component });
                }
                Ok(_) => deepest_existing = (walked.clone(), component.clone()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => still_exists = false,
                Err(error) => {
                    return Err(ResolutionError::Unreadable {
                        component,
                        detail: error.to_string(),
                    })
                }
            }
        }

        // Independent of the walk, and not redundant with it: `canonicalize` resolves the whole
        // chain the way the kernel does, so it also catches what `symlink_metadata` reports as an
        // ordinary directory — a Windows junction, a filesystem mounted under the mount root.
        let (deepest, deepest_component) = deepest_existing;
        let real = fs::canonicalize(&deepest).map_err(|error| ResolutionError::Unreadable {
            component: deepest_component.clone(),
            detail: error.to_string(),
        })?;
        if !real.starts_with(&canonical_root) {
            return Err(ResolutionError::Escapes {
                component: deepest_component,
            });
        }

        Ok(ResolvedPath(resolved))
    }

    /// This root as the kernel sees it: an existing directory that is not itself a symbolic link.
    ///
    /// Canonicalizing is what makes containment comparable, because the workspace directory is
    /// commonly reached through a symbolic link above it — a macOS temporary directory is. The
    /// `lstat` before it is what stops that from being a hole: were the mount root itself a link,
    /// canonicalizing would silently adopt its target as the root and every path under it would
    /// then "lie within the mount root" by definition.
    fn canonical_root(&self) -> Result<PathBuf, ResolutionError> {
        let metadata =
            fs::symlink_metadata(&self.0).map_err(|error| ResolutionError::MountRootUnusable {
                detail: error.to_string(),
            })?;
        if metadata.file_type().is_symlink() {
            return Err(ResolutionError::MountRootIsSymbolicLink);
        }
        if !metadata.is_dir() {
            return Err(ResolutionError::MountRootUnusable {
                detail: "the mount root is not a directory".to_owned(),
            });
        }
        fs::canonicalize(&self.0).map_err(|error| ResolutionError::MountRootUnusable {
            detail: error.to_string(),
        })
    }

    /// Whether a path lies within this root, by spelling.
    ///
    /// Lexical, like [`WorkspaceRelativePath::new`] and for the same reason: it answers a question
    /// about names. It makes no syscall and therefore says nothing about where the path leads —
    /// [`MountRoot::resolve`] is what answers that.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.0)
    }
}

/// An absolute path to workspace content, produced by [`MountRoot::resolve`] and by nothing else.
///
/// There is no other constructor and no `From<PathBuf>`: holding one is evidence that the walk in
/// [`MountRoot::resolve`] ran and refused nothing, which is what makes it different from a
/// `PathBuf` a caller assembled itself. What it is not is evidence about the tree *now* — see the
/// module documentation's condition 1.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolvedPath(PathBuf);

impl ResolvedPath {
    /// The absolute path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// The absolute path, owned.
    #[must_use]
    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl AsRef<Path> for ResolvedPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// Why a [`WorkspaceRelativePath`] could not be resolved against a [`MountRoot`].
///
/// Every variant names the workspace-relative component it refused, never the absolute path it was
/// about to produce: the relative component is what a caller can act on, and it is also the only
/// half that is safe to put in a log a support bundle might carry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolutionError {
    /// A component of the path is a symbolic link, so resolution would leave the walk this module
    /// performed and follow a target chosen by whoever planted it.
    SymbolicLinkComponent {
        /// The workspace-relative path of the component that is a link.
        component: PathBuf,
    },
    /// The path resolves outside the mount root although no component of it is a symbolic link.
    Escapes {
        /// The workspace-relative path of the deepest component that exists.
        component: PathBuf,
    },
    /// A component could not be inspected, so nothing is known about where it leads.
    Unreadable {
        /// The workspace-relative path of the component that could not be inspected.
        component: PathBuf,
        /// What the filesystem said.
        detail: String,
    },
    /// The mount root itself is a symbolic link, so its target — not the workspace — would define
    /// what "inside the mount root" means.
    MountRootIsSymbolicLink,
    /// The mount root is missing, is not a directory, or could not be inspected.
    MountRootUnusable {
        /// What the filesystem said.
        detail: String,
    },
}

impl core::fmt::Display for ResolutionError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::SymbolicLinkComponent { component } => write!(
                formatter,
                "{} is a symbolic link, and a workspace path is resolved without following one",
                component.display()
            ),
            Self::Escapes { component } => write!(
                formatter,
                "{} resolves outside the mount root",
                component.display()
            ),
            Self::Unreadable { component, detail } => write!(
                formatter,
                "{} could not be inspected, so it is not known to be inside the mount root: \
                 {detail}",
                component.display()
            ),
            Self::MountRootIsSymbolicLink => {
                formatter.write_str("the mount root is a symbolic link, not a directory")
            }
            Self::MountRootUnusable { detail } => {
                write!(formatter, "the mount root is unusable: {detail}")
            }
        }
    }
}

impl std::error::Error for ResolutionError {}

/// The metadata database, and the two sidecar files WAL mode creates beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatabasePath(PathBuf);

impl DatabasePath {
    /// The database file itself.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// The write-ahead log SQLite keeps beside the database in WAL mode.
    #[must_use]
    pub fn wal(&self) -> PathBuf {
        self.sidecar("-wal")
    }

    /// The shared-memory index SQLite keeps beside the database in WAL mode.
    #[must_use]
    pub fn shared_memory(&self) -> PathBuf {
        self.sidecar("-shm")
    }

    fn sidecar(&self, suffix: &str) -> PathBuf {
        let mut name = self.0.clone().into_os_string();
        name.push(suffix);
        PathBuf::from(name)
    }

    /// Every file that is part of this database. All three, never just the first.
    #[must_use]
    pub fn files(&self) -> Vec<PathBuf> {
        vec![self.0.clone(), self.wal(), self.shared_memory()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> WorkspaceRoot {
        WorkspaceRoot::new("/home/someone/.mesh/workspaces/0191")
    }

    /// A workspace that exists, because resolution is answered by the filesystem and a workspace
    /// spelled in a string has no mount root to walk. Removes itself.
    struct RealWorkspace {
        root: WorkspaceRoot,
    }

    impl RealWorkspace {
        fn new(label: &str) -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let unique = format!(
                "mesh-store-paths-{label}-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let root = WorkspaceRoot::new(std::env::temp_dir().join(unique));
            fs::create_dir_all(root.mount_root().as_path()).expect("a real mount root");
            Self { root }
        }

        fn mounts(&self) -> MountRoot {
            self.root.mount_root()
        }

        fn resolve(&self, relative: &str) -> Result<ResolvedPath, ResolutionError> {
            let path = WorkspaceRelativePath::new(relative).expect("a legal workspace path");
            self.mounts().resolve(&path)
        }
    }

    impl Drop for RealWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.root.as_path());
        }
    }

    #[test]
    fn the_layout_matches_plan_6_2() {
        let root = workspace();
        assert_eq!(
            root.database().as_path(),
            Path::new("/home/someone/.mesh/workspaces/0191/metadata.sqlite")
        );
        assert_eq!(
            root.mount_root().as_path(),
            Path::new("/home/someone/.mesh/workspaces/0191/mounts")
        );
    }

    /// The criterion itself: none of the database's three files is inside the mount root.
    #[test]
    fn no_database_file_is_inside_the_mount_root() {
        let root = workspace();
        let mounts = root.mount_root();
        for file in root.database().files() {
            assert!(
                !mounts.contains(&file),
                "{} is inside the materialization root",
                file.display()
            );
        }
    }

    #[test]
    fn the_wal_and_shm_sidecars_are_named() {
        let database = workspace().database();
        assert!(database
            .wal()
            .to_string_lossy()
            .ends_with("metadata.sqlite-wal"));
        assert!(database
            .shared_memory()
            .to_string_lossy()
            .ends_with("metadata.sqlite-shm"));
        assert_eq!(database.files().len(), 3);
    }

    /// Every escape a caller might try. Each is refused by the constructor, so none of them is a
    /// path materialization can hold.
    #[test]
    fn every_escaping_path_is_refused() {
        let cases: &[(&str, PathError)] = &[
            ("", PathError::Empty),
            ("..", PathError::ParentComponent),
            ("../metadata.sqlite", PathError::ParentComponent),
            ("../../metadata.sqlite", PathError::ParentComponent),
            ("a/../../metadata.sqlite", PathError::ParentComponent),
            ("a/b/../..", PathError::ParentComponent),
            ("/metadata.sqlite", PathError::Absolute),
            ("/", PathError::Absolute),
            (".", PathError::CurrentComponent),
            ("./a", PathError::CurrentComponent),
            ("a\0b", PathError::InteriorNul),
        ];
        for (input, expected) in cases {
            assert_eq!(
                WorkspaceRelativePath::new(input),
                Err(expected.clone()),
                "{input:?} was not refused as expected"
            );
        }
    }

    /// `Path::components` normalizes an interior `.` and a doubled separator away, so these three
    /// survive the component walk and would otherwise be accepted as a second name for a path that
    /// is already legal.
    #[test]
    fn a_second_spelling_of_a_legal_path_is_refused() {
        for input in ["a/./b", "a//b", "a/", "a/b/", "a/././b"] {
            let error = WorkspaceRelativePath::new(input)
                .expect_err(&format!("{input:?} is a second spelling"));
            let PathError::NonCanonicalSpelling { canonical } = error else {
                panic!("{input:?} was refused for the wrong reason: {error:?}");
            };
            // The canonical form is itself accepted, so the error names a usable path.
            assert!(WorkspaceRelativePath::new(&canonical).is_ok());
        }
    }

    #[test]
    fn an_ordinary_path_is_accepted_and_resolves_inside_the_mount_root() {
        let workspace = RealWorkspace::new("ordinary");
        let mounts = workspace.mounts();
        for input in ["a", "a/b", "a/b/c.txt", "metadata.sqlite", "mounts"] {
            let resolved = workspace.resolve(input).expect("an ordinary path is fine");
            assert!(
                mounts.contains(resolved.as_path()),
                "{input:?} resolved outside"
            );
        }
    }

    /// The ordinary case for materialization: the file is being created, so neither it nor its
    /// parent directories exist yet. Nothing can hide below a component that is not there.
    #[test]
    fn a_path_whose_components_do_not_exist_yet_resolves() {
        let workspace = RealWorkspace::new("not-yet-created");
        let resolved = workspace
            .resolve("src/deeply/nested/new.rs")
            .expect("materialization names files before it writes them");
        assert!(!resolved.as_path().exists());
        assert!(workspace.mounts().contains(resolved.as_path()));
    }

    /// The sharpest case: a workspace path *named* `metadata.sqlite` is perfectly legal content,
    /// and it resolves to `mounts/metadata.sqlite` — a different file from the store's own
    /// database. That is what makes this structural rather than name-based; a name-based rule
    /// would have to refuse the user a file called `metadata.sqlite`.
    #[test]
    fn a_workspace_file_may_be_called_metadata_sqlite_without_being_the_database() {
        let workspace = RealWorkspace::new("named-like-the-database");
        let resolved = workspace
            .resolve(DATABASE_FILE_NAME)
            .expect("legal content");
        assert_ne!(resolved.as_path(), workspace.root.database().as_path());
        assert!(workspace.mounts().contains(resolved.as_path()));
    }

    /// A relative path with many components still cannot climb out, however long it is.
    #[test]
    fn a_deep_path_still_resolves_inside() {
        let workspace = RealWorkspace::new("deep");
        let deep = "a/".repeat(64) + "leaf";
        let resolved = workspace.resolve(&deep).expect("deep is fine");
        assert!(workspace.mounts().contains(resolved.as_path()));
    }

    #[test]
    fn a_windows_style_prefix_is_refused_on_the_platforms_that_have_one() {
        // `Component::Prefix` only ever appears on Windows; on Unix `C:\x` is one normal
        // component. Both outcomes are acceptable — what must never happen is acceptance of a
        // path that resolves outside the mount root, which the component walk already prevents.
        match WorkspaceRelativePath::new("C:\\metadata.sqlite") {
            Ok(relative) => {
                let workspace = RealWorkspace::new("windows-prefix");
                let mounts = workspace.mounts();
                let resolved = mounts.resolve(&relative).expect("one normal component");
                assert!(mounts.contains(resolved.as_path()));
            }
            Err(error) => assert!(matches!(error, PathError::Prefix | PathError::Absolute)),
        }
    }

    /// Resolution is answered by the filesystem, so a mount root that is not there answers
    /// nothing. The alternative — treating a missing root as "nothing can be inside it, so allow"
    /// — is how a check becomes a no-op the day the directory is late.
    #[test]
    fn a_mount_root_that_does_not_exist_resolves_nothing() {
        let missing = MountRoot(std::env::temp_dir().join("mesh-store-paths-no-such-root-01KZ"));
        let relative = WorkspaceRelativePath::new("a.txt").expect("legal");
        assert!(matches!(
            missing.resolve(&relative),
            Err(ResolutionError::MountRootUnusable { .. })
        ));
    }

    /// The hole the `lstat` before `canonicalize` closes: were the mount root allowed to be a
    /// link, its target would become the root and every path under it would be "contained" by
    /// definition — `mounts -> /` would make the whole filesystem workspace content.
    #[cfg(unix)]
    #[test]
    fn a_mount_root_that_is_itself_a_link_is_refused() {
        let workspace = RealWorkspace::new("root-is-a-link");
        let elsewhere = workspace.root.as_path().join("elsewhere");
        fs::create_dir_all(&elsewhere).expect("a directory to point at");
        let linked = workspace.root.as_path().join("linked-mounts");
        std::os::unix::fs::symlink(&elsewhere, &linked).expect("plants");

        let relative = WorkspaceRelativePath::new("a.txt").expect("legal");
        assert_eq!(
            MountRoot(linked).resolve(&relative),
            Err(ResolutionError::MountRootIsSymbolicLink)
        );
    }

    /// A link is refused even when it points *inside* the mount root. Following it would give the
    /// index two keys for one object, which is the defect `NonCanonicalSpelling` already refuses
    /// on the naming side.
    #[cfg(unix)]
    #[test]
    fn a_link_component_is_refused_even_when_its_target_is_inside_the_mount_root() {
        let workspace = RealWorkspace::new("inward-link");
        let mounts = workspace.mounts();
        fs::create_dir_all(mounts.as_path().join("real")).expect("a directory inside");
        fs::write(mounts.as_path().join("real/file.txt"), "content").expect("a file inside");
        std::os::unix::fs::symlink("real", mounts.as_path().join("alias")).expect("plants");

        let error = workspace
            .resolve("alias/file.txt")
            .expect_err("a second name for one object");
        assert_eq!(
            error,
            ResolutionError::SymbolicLinkComponent {
                component: PathBuf::from("alias")
            }
        );
        // And the object is still reachable by its one real name.
        assert!(workspace.resolve("real/file.txt").is_ok());
    }

    /// A refusal names the workspace-relative component, never the absolute path it was about to
    /// produce: that message is what ends up in a log, and a log is what ends up in a support
    /// bundle.
    #[cfg(unix)]
    #[test]
    fn a_refusal_names_the_relative_component_and_no_absolute_path() {
        let workspace = RealWorkspace::new("message");
        std::os::unix::fs::symlink("..", workspace.mounts().as_path().join("up")).expect("plants");
        let error = workspace
            .resolve("up/metadata.sqlite")
            .expect_err("refused at `up`");
        let message = error.to_string();
        assert!(message.contains("up"), "{message}");
        assert!(
            !message.contains(&*workspace.root.as_path().to_string_lossy()),
            "the message carries the absolute workspace path: {message}"
        );
    }
}
