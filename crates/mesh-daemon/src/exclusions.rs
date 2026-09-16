//! What this workspace does not version, and which source said so.
//!
//! # The question this answers
//!
//! *"Why is this file not being saved?"* A user must be able to answer it without opening a
//! configuration file, guessing at precedence, or reading the source of a predicate. So this module
//! loads every exclusion source a workspace has, composes them in
//! [`mesh_store::ExclusionSource`] order, and reports both the effective set and — for any one path
//! — the exact rule and source that decided it.
//!
//! # It is deliberately not on the socket
//!
//! `crate::ipc::METHODS` is the narrow door for anything that needs the *running* service, and this
//! needs nothing from it: [`mesh_store::ExclusionSet::verdict`] is a pure function, and the sources
//! are files. Putting it behind the socket would mean a user cannot ask why a path is not versioned
//! unless the background service happens to be up, which is exactly the moment they are most likely
//! to be asking. It would also bump the surface version for a question that does not touch the
//! surface.
//!
//! # Where the sources live
//!
//! | Source | Read from | Precedence |
//! |---|---|---|
//! | `mesh_store::ExclusionSource::RepositoryIgnore` | `<mount root>/.gitignore` | lowest |
//! | `mesh_store::ExclusionSource::Configuration` | supplied by the caller at open time | middle |
//! | `mesh_store::ExclusionSource::WorkspaceFile` | `<workspace root>/.meshignore` | highest |
//!
//! An absent file is not an error and contributes nothing. An **unreadable rule** is an error that
//! names the rule: `mesh_store` refuses rather than skips, and this module does not soften that,
//! because a silently dropped exclusion rule admits every path it would have kept out.

use std::path::{Path, PathBuf};

use mesh_store::{
    Exclusion, ExclusionError, ExclusionSet, ExclusionSource, PathError, WorkspaceRelativePath,
    WORKSPACE_EXCLUSION_FILE_NAME,
};

use crate::ipc::Json;

/// The ignore file this module reads as [`mesh_store::ExclusionSource::RepositoryIgnore`].
pub const REPOSITORY_IGNORE_FILE_NAME: &str = ".gitignore";

/// Why the effective exclusion set could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExclusionLoadFailure {
    /// A source file exists but could not be read.
    Unreadable {
        /// The file.
        file: PathBuf,
        /// What the filesystem said.
        detail: String,
    },
    /// A source file was read and holds a rule the predicate refuses.
    Unusable(ExclusionError),
}

impl core::fmt::Display for ExclusionLoadFailure {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unreadable { file, detail } => {
                write!(formatter, "could not read {}: {detail}", file.display())
            }
            Self::Unusable(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ExclusionLoadFailure {}

/// One source's contribution: where it came from and how many rules it supplied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedSource {
    /// Which source.
    pub source: ExclusionSource,
    /// The file it was read from, when it was a file.
    pub file: Option<PathBuf>,
    /// How many rules it supplied.
    pub rules: usize,
}

/// The workspace's effective exclusion set, with the provenance of every part of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveExclusions {
    set: ExclusionSet,
    loaded: Vec<LoadedSource>,
    reserve_private_top_level: bool,
}

impl EffectiveExclusions {
    /// Read every source a workspace at `workspace_root` has.
    ///
    /// `configuration` is the rule text an operator supplied when the workspace was opened; pass
    /// `None` when there is none, which is what this build's binaries do and what the report says.
    ///
    /// # Errors
    ///
    /// [`ExclusionLoadFailure`] naming the file or the rule. A missing file is not a failure.
    pub fn load(
        workspace_root: &Path,
        configuration: Option<&str>,
    ) -> Result<Self, ExclusionLoadFailure> {
        // The original store layout presents content beneath `mounts/`; the native alpha layout
        // presents the folder itself and keeps private state beneath `.mesh/`. A plain folder
        // being previewed before import is also its own presented root. Reading `.gitignore` from
        // `mounts/` unconditionally made the command lie for both of the latter cases.
        let legacy_journal = workspace_root.join(crate::workspace::RECORD_FILE_NAME);
        let legacy_mount = workspace_root.join(mesh_store::MOUNT_DIRECTORY_NAME);
        let mount_root = if legacy_journal.is_file() && legacy_mount.is_dir() {
            legacy_mount
        } else {
            workspace_root.to_path_buf()
        };
        let repository_ignore = mount_root.join(REPOSITORY_IGNORE_FILE_NAME);
        let workspace_file = workspace_root.join(WORKSPACE_EXCLUSION_FILE_NAME);

        let repository_text = read_optional(&repository_ignore)?;
        let workspace_text = read_optional(&workspace_file)?;
        let mut exclusions = Self::from_texts(
            Some(repository_ignore),
            repository_text,
            configuration,
            Some(workspace_file),
            workspace_text,
        )?;
        exclusions.reserve_private_top_level =
            crate::workspace::presented_workspace_storage_root(workspace_root)
                .map_or(true, |root| root.is_none());
        Ok(exclusions)
    }

    /// Build the effective set from bytes a caller already read through stronger filesystem
    /// authority than an ordinary pathname.
    ///
    /// Folder import uses this seam after opening `.gitignore` and `.meshignore` relative to its
    /// retained source-directory descriptor. That keeps the shared precedence and parser without
    /// allowing either rules file to be a symlink out of the selected folder.
    pub(crate) fn from_texts(
        repository_file: Option<PathBuf>,
        repository_text: Option<String>,
        configuration: Option<&str>,
        workspace_file: Option<PathBuf>,
        workspace_text: Option<String>,
    ) -> Result<Self, ExclusionLoadFailure> {
        // Lowest precedence first, so `rules()` reads in the order the predicate consults them and
        // so a reader of this list sees the precedence rather than having to know it.
        let contributions: [(ExclusionSource, Option<PathBuf>, Option<String>); 3] = [
            (
                ExclusionSource::RepositoryIgnore,
                repository_file,
                repository_text,
            ),
            (
                ExclusionSource::Configuration,
                None,
                configuration.map(str::to_owned),
            ),
            (
                ExclusionSource::WorkspaceFile,
                workspace_file,
                workspace_text,
            ),
        ];

        let mut set = ExclusionSet::new();
        let mut loaded = Vec::new();
        for (source, file, text) in contributions {
            let before = set.len();
            if let Some(text) = text {
                set = set
                    .with_source(source, &text)
                    .map_err(ExclusionLoadFailure::Unusable)?;
            }
            loaded.push(LoadedSource {
                source,
                file,
                rules: set.len() - before,
            });
        }
        Ok(Self {
            set,
            loaded,
            reserve_private_top_level: true,
        })
    }

    /// The composed predicate.
    #[must_use]
    pub const fn set(&self) -> &ExclusionSet {
        &self.set
    }

    /// What each source contributed, lowest precedence first.
    pub fn sources(&self) -> impl Iterator<Item = &LoadedSource> {
        self.loaded.iter()
    }

    /// Whether an excluded directory can still contain a path restored by a later rule.
    ///
    /// This intentionally answers a conservative traversal question rather than attempting to
    /// duplicate the predicate's matcher. With no re-inclusion rule anywhere, an excluded
    /// directory is a final answer and filesystem discovery may prune it without opening it. If
    /// any re-inclusion exists, callers retain the existing full walk so a descendant cannot be
    /// hidden merely because its parent matched an earlier exclusion.
    #[must_use]
    pub(crate) fn has_reinclusion_rules(&self) -> bool {
        self.set
            .rules()
            .any(mesh_store::ExclusionRule::is_reinclude)
    }

    /// Whether `candidate` may become durable workspace content.
    ///
    /// This is the shared production predicate used by native discovery and the later
    /// authoritative creation/adoption path. A caller must not implement ignore matching from the
    /// report JSON, because doing so would let presentation and admission drift apart.
    ///
    /// # Errors
    ///
    /// [`PathError`] when `candidate` is not one canonical workspace-relative path.
    pub fn versions_path(&self, candidate: &str) -> Result<bool, PathError> {
        self.versions_path_in_layout(candidate, self.reserve_private_top_level)
    }

    /// Whether `candidate` may become durable content in a layout whose private state is
    /// structurally outside the presented folder.
    ///
    /// Names such as `records.mesh`, `chunks`, and `metadata.sqlite` are ordinary user content in
    /// that layout. Git metadata remains structural at every depth.
    pub(crate) fn versions_presented_path(&self, candidate: &str) -> Result<bool, PathError> {
        self.versions_path_in_layout(candidate, false)
    }

    fn versions_path_in_layout(
        &self,
        candidate: &str,
        reserve_private_top_level: bool,
    ) -> Result<bool, PathError> {
        let path = WorkspaceRelativePath::new(candidate)?;
        if path
            .as_path()
            .components()
            .enumerate()
            .any(|(index, component)| {
                crate::workspace::is_private_managed_component_in_layout(
                    index,
                    component.as_os_str().to_string_lossy().as_ref(),
                    reserve_private_top_level,
                )
            })
        {
            return Ok(false);
        }
        Ok(self.set.verdict(&path).is_included())
    }

    /// The whole effective set as one JSON value, and the verdict for `path` when one was given.
    ///
    /// `path` is workspace-relative. A path this workspace cannot name at all is reported as such
    /// rather than as included: "Mesh would not version that" and "that is not a path" are
    /// different answers and a user acting on the wrong one changes the wrong file.
    #[must_use]
    pub fn report(&self, path: Option<&str>) -> Json {
        let sources = Json::Array(
            self.loaded
                .iter()
                .map(|entry| {
                    Json::object([
                        ("source", Json::text(entry.source.label())),
                        (
                            "from",
                            entry.file.as_ref().map_or_else(
                                || Json::text("supplied when the workspace was opened"),
                                |file| Json::text(file.display().to_string()),
                            ),
                        ),
                        ("rules", Json::text(entry.rules.to_string())),
                    ])
                })
                .collect(),
        );
        let rules = Json::Array(
            self.set
                .rules()
                .map(|rule| {
                    Json::object([
                        ("source", Json::text(rule.source().label())),
                        ("rule", Json::text(rule.pattern())),
                        (
                            "effect",
                            Json::text(if rule.is_reinclude() {
                                "include"
                            } else {
                                "exclude"
                            }),
                        ),
                    ])
                })
                .collect(),
        );
        let mut fields = vec![
            ("sources", sources),
            ("rules", rules),
            ("rule_count", Json::text(self.set.len().to_string())),
        ];
        if let Some(candidate) = path {
            fields.push(("path", Json::text(candidate)));
            fields.push(("verdict", self.verdict_json(candidate)));
        }
        Json::object(fields)
    }

    fn verdict_json(&self, candidate: &str) -> Json {
        let Ok(path) = WorkspaceRelativePath::new(candidate) else {
            return Json::object([
                ("answer", Json::text("not-a-workspace-path")),
                (
                    "why",
                    Json::text(crate::user_messages::EXCLUSION_PATH_UNUSABLE),
                ),
            ]);
        };
        if self.reserve_private_top_level
            && Path::new(candidate)
                .components()
                .next()
                .is_some_and(|component| {
                    component.as_os_str() == crate::workspace::STORAGE_DIRECTORY_NAME
                })
        {
            return Json::object([
                ("answer", Json::text("not-versioned")),
                ("source", Json::text("Mesh private storage")),
                ("rule", Json::text("/.mesh/")),
                (
                    "matched",
                    Json::text(crate::workspace::STORAGE_DIRECTORY_NAME),
                ),
            ]);
        }
        if Path::new(candidate)
            .components()
            .enumerate()
            .any(|(index, component)| {
                crate::workspace::is_private_managed_component_in_layout(
                    index,
                    component.as_os_str().to_string_lossy().as_ref(),
                    self.reserve_private_top_level,
                )
            })
        {
            return Json::object([
                ("answer", Json::text("not-versioned")),
                ("source", Json::text("Mesh structural reservation")),
                ("rule", Json::text("Git or private workspace metadata")),
                ("matched", Json::text(candidate)),
            ]);
        }
        match self.set.verdict(&path) {
            Exclusion::Included { reinstated_by } => Json::object([
                ("answer", Json::text("versioned")),
                (
                    "why",
                    reinstated_by.map_or_else(
                        || Json::text("no rule matches this path"),
                        |source| Json::text(format!("put back by {}", source.label())),
                    ),
                ),
            ]),
            Exclusion::Excluded {
                source,
                rule,
                matched,
            } => Json::object([
                ("answer", Json::text("not-versioned")),
                ("source", Json::text(source.label())),
                ("rule", Json::text(rule)),
                ("matched", Json::text(matched.display().to_string())),
            ]),
        }
    }
}

/// Read a file that may not exist. A missing file is `None`, not an error.
fn read_optional(file: &Path) -> Result<Option<String>, ExclusionLoadFailure> {
    match std::fs::read_to_string(file) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ExclusionLoadFailure::Unreadable {
            file: file.to_path_buf(),
            detail: error.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree(PathBuf);

    impl Tree {
        fn new(label: &str) -> Self {
            let mut root = std::env::temp_dir();
            root.push(format!(
                "mesh-daemon-exclusions-{label}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join(mesh_store::MOUNT_DIRECTORY_NAME))
                .expect("a workspace");
            Self(root)
        }

        fn write(&self, relative: &str, text: &str) {
            let full = self.0.join(relative);
            std::fs::create_dir_all(full.parent().expect("a parent")).expect("a parent");
            std::fs::write(full, text).expect("a file");
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_workspace_with_no_sources_excludes_nothing() {
        let tree = Tree::new("empty");
        let effective = EffectiveExclusions::load(&tree.0, None).expect("loads");
        assert!(effective.set().is_empty());
        assert_eq!(effective.sources().count(), ExclusionSource::ALL.len());
        let report = effective.report(Some("target/debug/mesh")).encode();
        assert!(report.contains("versioned"), "{report}");
    }

    #[test]
    fn the_report_names_the_source_that_excluded_the_path() {
        let tree = Tree::new("named");
        tree.write(".meshignore", "target/\n");
        let effective = EffectiveExclusions::load(&tree.0, None).expect("loads");
        let report = effective.report(Some("target/debug/mesh")).encode();
        assert!(report.contains("not-versioned"), "{report}");
        assert!(report.contains(WORKSPACE_EXCLUSION_FILE_NAME), "{report}");
        assert!(report.contains("target/"), "{report}");
    }

    #[test]
    fn the_repositorys_own_ignore_rules_are_read_from_the_mount_root() {
        let tree = Tree::new("gitignore");
        tree.write("records.mesh", "legacy journal");
        tree.write("mounts/.gitignore", "build/\n");
        let effective = EffectiveExclusions::load(&tree.0, None).expect("loads");
        let report = effective.report(Some("build/out/a.o")).encode();
        assert!(report.contains("not-versioned"), "{report}");
        assert!(report.contains("repository ignore rules"), "{report}");
    }

    #[test]
    fn a_plain_or_namespaced_native_folder_reads_its_root_gitignore() {
        for label in ["plain", "namespaced"] {
            let tree = Tree::new(label);
            tree.write(".gitignore", "build/\n");
            if label == "namespaced" {
                tree.write(".mesh/records.mesh", "private journal");
            }
            let report = EffectiveExclusions::load(&tree.0, None)
                .expect("loads")
                .report(Some("build/out/a.o"))
                .encode();
            assert!(report.contains("not-versioned"), "{report}");
            assert!(report.contains(&tree.0.join(".gitignore").display().to_string()));
        }
    }

    #[test]
    fn the_private_namespace_is_structurally_not_versioned() {
        let tree = Tree::new("private");
        tree.write(".mesh/records.mesh", "private journal");
        let effective = EffectiveExclusions::load(&tree.0, None).expect("loads");

        for path in [".mesh", ".mesh/records.mesh", ".mesh/chunks/content"] {
            let report = effective.report(Some(path)).encode();
            assert!(report.contains("not-versioned"), "{report}");
            assert!(report.contains("Mesh private storage"), "{report}");
            assert!(report.contains("/.mesh/"), "{report}");
        }
        assert!(effective
            .report(Some("src/.mesh/user.txt"))
            .encode()
            .contains("versioned"));
    }

    #[test]
    fn an_external_store_makes_private_store_names_ordinary_presented_content() {
        let tree = Tree::new("presented-private-names");
        let store = tree.0.join("external-store");
        let open = crate::OpenWorkspace::open_presented(&store).expect("presented workspace");
        let effective =
            EffectiveExclusions::load(open.root().as_path(), None).expect("presented exclusions");

        for path in [
            ".mesh/user.txt",
            "records.mesh",
            "metadata.sqlite",
            "chunks/content",
            "logs/agent-output",
        ] {
            assert!(
                effective.versions_path(path).expect("canonical path"),
                "{path}"
            );
            let report = effective.report(Some(path)).encode();
            assert!(report.contains("versioned"), "{path}: {report}");
        }
        assert!(!effective
            .versions_path("nested/.git/config")
            .expect("canonical Git path"));
    }

    #[test]
    fn git_metadata_is_structurally_not_versioned_at_any_depth() {
        let tree = Tree::new("git-metadata");
        tree.write(".gitignore", "!.git/\n!nested/.git/\n");
        let effective = EffectiveExclusions::load(&tree.0, None).expect("loads");

        for path in [".git/HEAD", "nested/.git/config"] {
            assert!(
                !effective.versions_path(path).expect("canonical path"),
                "ignore-rule re-inclusion must not override the structural reservation"
            );
            let report = effective.report(Some(path)).encode();
            assert!(report.contains("not-versioned"), "{report}");
            assert!(report.contains("Mesh structural reservation"), "{report}");
        }
    }

    #[test]
    fn the_mesh_native_file_wins_over_the_repositorys_rules() {
        let tree = Tree::new("precedence");
        tree.write("mounts/.gitignore", "generated/\n");
        tree.write(".meshignore", "!generated\n");
        let effective = EffectiveExclusions::load(&tree.0, None).expect("loads");
        let report = effective.report(Some("generated/schema.rs")).encode();
        assert!(report.contains("versioned"), "{report}");
        assert!(report.contains("put back by"), "{report}");
    }

    #[test]
    fn configuration_sits_between_the_two_files() {
        let tree = Tree::new("configuration");
        tree.write("mounts/.gitignore", "!vendor\n");
        let effective = EffectiveExclusions::load(&tree.0, Some("vendor\n")).expect("loads");
        assert!(!effective
            .set()
            .verdict(&WorkspaceRelativePath::new("vendor/dep.rs").expect("legal"))
            .is_included());

        let tree = Tree::new("configuration-loses");
        tree.write(".meshignore", "!vendor\n");
        let effective = EffectiveExclusions::load(&tree.0, Some("vendor\n")).expect("loads");
        assert!(effective
            .set()
            .verdict(&WorkspaceRelativePath::new("vendor/dep.rs").expect("legal"))
            .is_included());
    }

    #[test]
    fn an_unreadable_rule_is_reported_and_not_skipped() {
        let tree = Tree::new("bad-rule");
        tree.write(".meshignore", "target/\n**/nope\n");
        let failure = EffectiveExclusions::load(&tree.0, None).expect_err("refused");
        assert!(format!("{failure}").contains("**/nope"), "{failure}");
    }

    #[test]
    fn a_path_that_is_not_a_workspace_path_is_said_to_be_one_thing_and_not_the_other() {
        let tree = Tree::new("bad-path");
        let effective = EffectiveExclusions::load(&tree.0, None).expect("loads");
        let report = effective.report(Some("../escape")).encode();
        assert!(report.contains("not-a-workspace-path"), "{report}");
    }

    #[test]
    fn every_source_appears_in_the_report_even_when_it_supplied_nothing() {
        let tree = Tree::new("all-sources");
        let report = EffectiveExclusions::load(&tree.0, None)
            .expect("loads")
            .report(None)
            .encode();
        for source in ExclusionSource::ALL {
            assert!(
                report.contains(source.label()),
                "{source} missing: {report}"
            );
        }
    }
}
