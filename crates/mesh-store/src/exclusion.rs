//! Which workspace paths can produce a durable version, and which cannot.
//!
//! # Why this exists at all
//!
//! Plan §4.5 fires a checkpoint on file-handle close, on `fsync`, on atomic replacement and on
//! idle — **with no predicate on the path**. Plan §2.5 keeps every durable version reachable, and
//! `crates/mesh-store/RETENTION.md` records that the shipped collector frees only content no record
//! mentions at all. Put those three together and a build directory inside a workspace is durable
//! content forever: in this repository `target/` holds three orders of magnitude more bytes than the
//! tracked source, and a compiler rewrites much of it on every build.
//!
//! That makes an absent exclusion predicate the largest single term in what a user's disk holds —
//! larger than the versioning of the work they actually did. `benchmarks/budgets/storage.md` bounds
//! the *rate* at which admitted content costs disk. This module bounds *what is admitted*, which is
//! the only term of the two that can be reduced by orders of magnitude.
//!
//! # This is a different mechanism from [`crate::paths`], on purpose
//!
//! [`crate::paths`] excludes the store's own database **structurally**: there is no
//! [`WorkspaceRelativePath`] that names it, so no rule can be forgotten. That works because the
//! database sits outside the mount root and its exclusion is not a user's choice.
//!
//! A build directory is a different problem. It is legitimately inside the mount root, a user may
//! genuinely want it versioned, and the answer therefore has to be *declared* rather than made
//! unnameable. So this module is an ignore rule — the exact thing `paths.rs` refuses to be — and the
//! two live side by side because they answer two different questions. `paths.rs` decides what can be
//! **named**; this decides what a named path **produces**.
//!
//! # The predicate is pure, which is what makes it uniform across adapters
//!
//! [`ExclusionSet::verdict`] takes a path and returns a verdict. It reads no file, opens no
//! directory, consults no clock and holds no handle to an adapter — so FUSE, FSKit and the
//! folder-watching fallback cannot disagree about it, because none of them is an input.
//! Uniformity across adapters is not tested into existence here; it is the absence of a parameter.
//!
//! # The precedence rule, in one sentence
//!
//! Rules are ordered by their source ([`ExclusionSource`], lowest precedence first) and then by
//! declaration order within a source, and **the last rule that matches decides**. That is
//! `.gitignore`'s own rule, which is the one a user already knows, and it means a re-inclusion in a
//! higher-precedence source overrides an exclusion in a lower one without any second mechanism.
//!
//! # When in doubt, include
//!
//! Over-inclusion costs disk. Wrong exclusion silently loses a user's work, and plan §2.5 forbids
//! that. So an unparseable rule is **refused at construction** ([`ExclusionError`]) rather than
//! skipped: a skipped rule changes the answer for every path it would have matched, in the
//! direction nobody asked for, and says nothing.

use std::path::{Path, PathBuf};

use crate::paths::WorkspaceRelativePath;

/// The file a user edits to tell Mesh what not to version, at the workspace root.
pub const WORKSPACE_EXCLUSION_FILE_NAME: &str = ".meshignore";

/// Where an exclusion rule came from, ordered by precedence with the lowest first.
///
/// The order is the decision, and it is derived rather than written out so a new source cannot be
/// added without placing it: `RepositoryIgnore` < `Configuration` < `WorkspaceFile`.
///
/// * [`Self::RepositoryIgnore`] is *inherited evidence of intent* — a `.gitignore` says what some
///   other tool was told to skip, which is good evidence and not a statement about Mesh. It loses
///   to everything.
/// * [`Self::Configuration`] is what an operator supplied when the workspace was opened.
/// * [`Self::WorkspaceFile`] is [`WORKSPACE_EXCLUSION_FILE_NAME`], which a user wrote *knowing they
///   were talking to Mesh*. It wins, including when it re-includes something a lower source
///   excluded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExclusionSource {
    /// The repository's own ignore rules, read from the workspace content.
    RepositoryIgnore,
    /// Rules supplied explicitly when the workspace was opened.
    Configuration,
    /// The Mesh-native exclusion file at the workspace root.
    WorkspaceFile,
}

impl ExclusionSource {
    /// Every source, lowest precedence first.
    pub const ALL: &'static [Self] = &[
        Self::RepositoryIgnore,
        Self::Configuration,
        Self::WorkspaceFile,
    ];

    /// The name this source is reported under, to a user and in a test alike.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::RepositoryIgnore => "repository ignore rules",
            Self::Configuration => "workspace configuration",
            Self::WorkspaceFile => WORKSPACE_EXCLUSION_FILE_NAME,
        }
    }
}

impl core::fmt::Display for ExclusionSource {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.label())
    }
}

/// Why a rule could not be read, naming the rule rather than dropping it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExclusionError {
    /// The rule was empty, or `!` with nothing after it.
    Empty {
        /// Where it came from.
        source: ExclusionSource,
    },
    /// The rule held a component that could climb out of the workspace, or a NUL byte.
    Unusable {
        /// Where it came from.
        source: ExclusionSource,
        /// The rule as written.
        pattern: String,
    },
    /// The rule used a wildcard this predicate does not implement.
    ///
    /// Only a leading `*.` is understood. Refusing the rest is what keeps two readers of the same
    /// file from disagreeing: a half-implemented glob is a rule whose meaning depends on which
    /// implementation read it.
    UnsupportedWildcard {
        /// Where it came from.
        source: ExclusionSource,
        /// The rule as written.
        pattern: String,
    },
}

impl core::fmt::Display for ExclusionError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty { source } => {
                write!(formatter, "{source} holds an empty exclusion rule")
            }
            Self::Unusable { source, pattern } => write!(
                formatter,
                "{source} holds `{pattern}`, which does not name a path inside the workspace"
            ),
            Self::UnsupportedWildcard { source, pattern } => write!(
                formatter,
                "{source} holds `{pattern}`; only a leading `*.` wildcard is understood, and a rule \
                 this predicate cannot read is refused rather than ignored"
            ),
        }
    }
}

impl std::error::Error for ExclusionError {}

/// How a rule is matched against a path.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Shape {
    /// A pattern with a separator: matched against the whole relative path, anchored at the root.
    Anchored(PathBuf),
    /// A bare name: matched against any single component, at any depth.
    Component(String),
    /// A leading `*.` : matched against the end of any single component.
    Suffix(String),
}

/// One declared rule, with the source that declared it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExclusionRule {
    source: ExclusionSource,
    pattern: String,
    reinclude: bool,
    shape: Shape,
}

impl ExclusionRule {
    /// Read one rule, in the syntax [`ExclusionSet`] documents.
    ///
    /// # Errors
    ///
    /// [`ExclusionError`] naming the source and the rule. A rule is never silently dropped.
    pub fn parse(source: ExclusionSource, line: &str) -> Result<Self, ExclusionError> {
        let trimmed = line.trim();
        let (reinclude, body) = match trimmed.strip_prefix('!') {
            Some(rest) => (true, rest.trim()),
            None => (false, trimmed),
        };
        if body.is_empty() {
            return Err(ExclusionError::Empty { source });
        }
        let pattern = body.to_owned();
        if body.contains('\0') || body.split('/').any(|part| part == ".." || part == ".") {
            return Err(ExclusionError::Unusable { source, pattern });
        }
        let shape = if let Some(extension) = body.strip_prefix("*.") {
            if extension.contains('*') || extension.contains('/') || extension.is_empty() {
                return Err(ExclusionError::UnsupportedWildcard { source, pattern });
            }
            Shape::Suffix(format!(".{extension}"))
        } else if body.contains('*') {
            return Err(ExclusionError::UnsupportedWildcard { source, pattern });
        } else {
            let stripped = body.trim_start_matches('/').trim_end_matches('/');
            if stripped.is_empty() {
                return Err(ExclusionError::Unusable { source, pattern });
            }
            if stripped.contains('/') {
                Shape::Anchored(PathBuf::from(stripped))
            } else {
                Shape::Component(stripped.to_owned())
            }
        };
        Ok(Self {
            source,
            pattern,
            reinclude,
            shape,
        })
    }

    /// Which source declared this rule.
    #[must_use]
    pub const fn source(&self) -> ExclusionSource {
        self.source
    }

    /// The rule exactly as it was written, including any leading `!`.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Whether this rule puts a path back rather than taking it out.
    #[must_use]
    pub const fn is_reinclude(&self) -> bool {
        self.reinclude
    }

    /// The ancestor of `path` this rule matches, if it matches anything.
    ///
    /// **A match on an ancestor is a match on the path**, which is what makes `target/` mean the
    /// whole tree under it rather than one empty directory entry. The ancestor is returned rather
    /// than a bare `bool` so a report can say *which* directory did it.
    fn matched_ancestor(&self, path: &Path) -> Option<PathBuf> {
        let mut prefix = PathBuf::new();
        for component in path.components() {
            prefix.push(component);
            let hit = match &self.shape {
                Shape::Anchored(target) => prefix == *target,
                Shape::Component(name) => component.as_os_str() == name.as_str(),
                Shape::Suffix(suffix) => component
                    .as_os_str()
                    .to_string_lossy()
                    .ends_with(suffix.as_str()),
            };
            if hit {
                return Some(prefix);
            }
        }
        None
    }
}

/// What the predicate says about one path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exclusion {
    /// The path may produce a durable version.
    Included {
        /// The source whose re-inclusion rule put it back, when one did.
        ///
        /// `None` means nothing matched at all, which is the answer for an empty set and for every
        /// path a set has no opinion about.
        reinstated_by: Option<ExclusionSource>,
    },
    /// The path produces no operation, no manifest and no admitted byte.
    Excluded {
        /// Which source excluded it — the answer to "why is this not versioned?".
        source: ExclusionSource,
        /// The rule that did it, as written.
        rule: String,
        /// The ancestor the rule matched, which is the path itself when the rule named it directly.
        matched: PathBuf,
    },
}

impl Exclusion {
    /// Whether this path can produce a durable version.
    #[must_use]
    pub const fn is_included(&self) -> bool {
        matches!(self, Self::Included { .. })
    }

    /// The source that excluded the path, when one did.
    #[must_use]
    pub const fn excluded_by(&self) -> Option<ExclusionSource> {
        match self {
            Self::Included { .. } => None,
            Self::Excluded { source, .. } => Some(*source),
        }
    }
}

/// The workspace's declared exclusion set: every rule, in precedence order.
///
/// # The syntax, complete
///
/// | Written | Means |
/// |---|---|
/// | `target/` or `target` | any path component named `target`, and everything beneath it |
/// | `build/out/` | exactly the path `build/out` from the workspace root, and everything beneath it |
/// | `*.tmp` | any component whose name ends `.tmp`, and everything beneath it |
/// | `!keep.tmp` | put back anything a lower-precedence or earlier rule took out |
///
/// There is no third wildcard and no `**`. A rule outside this table is [`ExclusionError`], never a
/// rule that quietly matches nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExclusionSet {
    rules: Vec<ExclusionRule>,
}

impl ExclusionSet {
    /// An empty set: every path is included.
    #[must_use]
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// This set plus every rule in `text`, attributed to `source`.
    ///
    /// Blank lines and lines whose first non-space character is `#` are comments. Returns a new set
    /// rather than mutating, so a caller can build one candidate set without disturbing another.
    ///
    /// # Errors
    ///
    /// [`ExclusionError`] on the first unreadable rule, naming it and its source. Nothing is added
    /// when anything is refused: a half-loaded source is a set whose behaviour depends on where the
    /// reader stopped.
    pub fn with_source(&self, source: ExclusionSource, text: &str) -> Result<Self, ExclusionError> {
        let mut parsed = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            parsed.push(ExclusionRule::parse(source, trimmed)?);
        }
        let mut rules = self.rules.clone();
        rules.extend(parsed);
        rules.sort_by_key(|rule| rule.source);
        Ok(Self { rules })
    }

    /// Every rule, in the order the predicate consults them.
    pub fn rules(&self) -> impl Iterator<Item = &ExclusionRule> {
        self.rules.iter()
    }

    /// How many rules the set holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether the set declares nothing, in which case every path is included.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The verdict for one path. Pure: no filesystem, no clock, no adapter.
    ///
    /// The last matching rule decides, so precedence falls out of the ordering rather than out of a
    /// second mechanism.
    #[must_use]
    pub fn verdict(&self, path: &WorkspaceRelativePath) -> Exclusion {
        let mut decision = Exclusion::Included {
            reinstated_by: None,
        };
        for rule in &self.rules {
            let Some(matched) = rule.matched_ancestor(path.as_path()) else {
                continue;
            };
            decision = if rule.reinclude {
                Exclusion::Included {
                    reinstated_by: Some(rule.source),
                }
            } else {
                Exclusion::Excluded {
                    source: rule.source,
                    rule: rule.pattern.clone(),
                    matched,
                }
            };
        }
        decision
    }
}

/// What an exclusion set admits from a set of observed `(path, length)` pairs, and what it refuses.
///
/// The observations come from whoever walked the tree — an adapter, a test, the daemon. This type
/// does no I/O, so the same observations produce the same admission on every backend.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Admission {
    admitted: Vec<(WorkspaceRelativePath, u64)>,
    refused: Vec<(WorkspaceRelativePath, ExclusionSource, u64)>,
}

impl Admission {
    /// Split `observed` by the predicate.
    #[must_use]
    pub fn decide(
        set: &ExclusionSet,
        observed: impl IntoIterator<Item = (WorkspaceRelativePath, u64)>,
    ) -> Self {
        let mut admission = Self::default();
        for (path, length) in observed {
            match set.verdict(&path) {
                Exclusion::Included { .. } => admission.admitted.push((path, length)),
                Exclusion::Excluded { source, .. } => {
                    admission.refused.push((path, source, length));
                }
            }
        }
        admission
    }

    /// The paths that may become durable content, with their lengths.
    pub fn admitted(&self) -> impl Iterator<Item = (&WorkspaceRelativePath, u64)> {
        self.admitted.iter().map(|(path, length)| (path, *length))
    }

    /// The paths that may not, each with the source that refused it.
    pub fn refused(&self) -> impl Iterator<Item = (&WorkspaceRelativePath, ExclusionSource, u64)> {
        self.refused
            .iter()
            .map(|(path, source, length)| (path, *source, *length))
    }

    /// Bytes that may become durable content.
    #[must_use]
    pub fn admitted_bytes(&self) -> u64 {
        self.admitted.iter().map(|(_, length)| *length).sum()
    }

    /// Bytes the predicate kept out — the figure `benchmarks/budgets/storage.md` never has to bound.
    #[must_use]
    pub fn refused_bytes(&self) -> u64 {
        self.refused.iter().map(|(_, _, length)| *length).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(text: &str) -> WorkspaceRelativePath {
        WorkspaceRelativePath::new(text).expect("a legal workspace path")
    }

    fn set(source: ExclusionSource, text: &str) -> ExclusionSet {
        ExclusionSet::new()
            .with_source(source, text)
            .expect("the rules parse")
    }

    #[test]
    fn an_empty_set_includes_everything() {
        let empty = ExclusionSet::new();
        assert!(empty.is_empty());
        for candidate in ["a", "a/b", "target/debug/x", "node_modules/p/i.js"] {
            assert!(empty.verdict(&path(candidate)).is_included(), "{candidate}");
        }
    }

    /// The whole point: a directory rule takes the subtree, not one directory entry.
    #[test]
    fn a_directory_rule_excludes_everything_beneath_it() {
        let rules = set(ExclusionSource::WorkspaceFile, "target/\n");
        for candidate in ["target", "target/debug", "target/debug/deps/libx.rlib"] {
            let verdict = rules.verdict(&path(candidate));
            assert_eq!(
                verdict.excluded_by(),
                Some(ExclusionSource::WorkspaceFile),
                "{candidate} was not excluded: {verdict:?}"
            );
        }
        assert!(rules.verdict(&path("src/target.rs")).is_included());
    }

    #[test]
    fn a_bare_name_matches_at_any_depth_and_an_anchored_rule_does_not() {
        let bare = set(ExclusionSource::WorkspaceFile, "node_modules\n");
        assert!(!bare
            .verdict(&path("app/ui/node_modules/x.js"))
            .is_included());

        let anchored = set(ExclusionSource::WorkspaceFile, "app/node_modules\n");
        assert!(!anchored
            .verdict(&path("app/node_modules/x.js"))
            .is_included());
        assert!(anchored
            .verdict(&path("lib/app/node_modules/x.js"))
            .is_included());
    }

    #[test]
    fn a_suffix_rule_matches_the_end_of_a_component() {
        let rules = set(ExclusionSource::WorkspaceFile, "*.tmp\n");
        assert!(!rules.verdict(&path("a/b/scratch.tmp")).is_included());
        assert!(!rules.verdict(&path("a/build.tmp/inner.txt")).is_included());
        assert!(rules.verdict(&path("a/tmp.rs")).is_included());
    }

    /// Precedence, as one decidable statement rather than a paragraph.
    #[test]
    fn the_mesh_native_file_overrides_the_repositorys_own_ignore_rules() {
        let rules = ExclusionSet::new()
            .with_source(ExclusionSource::RepositoryIgnore, "generated/\n")
            .expect("parses")
            .with_source(ExclusionSource::WorkspaceFile, "!generated\n")
            .expect("parses");
        assert_eq!(
            rules.verdict(&path("generated/schema.rs")),
            Exclusion::Included {
                reinstated_by: Some(ExclusionSource::WorkspaceFile)
            }
        );
    }

    #[test]
    fn a_lower_precedence_reinclusion_does_not_override_a_higher_exclusion() {
        let rules = ExclusionSet::new()
            .with_source(ExclusionSource::RepositoryIgnore, "!vendor\n")
            .expect("parses")
            .with_source(ExclusionSource::WorkspaceFile, "vendor/\n")
            .expect("parses");
        assert_eq!(
            rules.verdict(&path("vendor/lib.rs")).excluded_by(),
            Some(ExclusionSource::WorkspaceFile)
        );
    }

    #[test]
    fn sources_are_ordered_lowest_precedence_first() {
        assert!(ExclusionSource::RepositoryIgnore < ExclusionSource::Configuration);
        assert!(ExclusionSource::Configuration < ExclusionSource::WorkspaceFile);
        assert_eq!(ExclusionSource::ALL.len(), 3);
        let mut sorted = ExclusionSource::ALL.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, ExclusionSource::ALL.to_vec());
    }

    /// A rule this predicate cannot read is refused, never skipped.
    #[test]
    fn an_unreadable_rule_is_refused_and_named() {
        for bad in ["**/x", "a*b", "*", "*."] {
            let error = ExclusionSet::new()
                .with_source(ExclusionSource::WorkspaceFile, bad)
                .expect_err(&format!("`{bad}` is not readable"));
            assert!(
                format!("{error}").contains(bad),
                "the refusal did not name `{bad}`: {error}"
            );
        }
        for bad in ["../escape", "a/../b", "./x"] {
            assert!(matches!(
                ExclusionSet::new().with_source(ExclusionSource::Configuration, bad),
                Err(ExclusionError::Unusable { .. })
            ));
        }
        assert!(matches!(
            ExclusionSet::new().with_source(ExclusionSource::WorkspaceFile, "!"),
            Err(ExclusionError::Empty { .. })
        ));
    }

    #[test]
    fn a_refused_source_adds_none_of_its_rules() {
        let base = set(ExclusionSource::WorkspaceFile, "target/\n");
        assert_eq!(base.len(), 1);
        assert!(base
            .with_source(ExclusionSource::Configuration, "logs/\nb**d\n")
            .is_err());
        // The original is untouched, because `with_source` returns a new set or an error.
        assert_eq!(base.len(), 1);
    }

    #[test]
    fn comments_and_blank_lines_are_not_rules() {
        let rules = set(
            ExclusionSource::WorkspaceFile,
            "# what the compiler writes\n\n  \ntarget/\n",
        );
        assert_eq!(rules.len(), 1);
    }

    /// Purity, stated as a test: the verdict is a function of the two arguments and nothing else.
    #[test]
    fn the_same_path_gets_the_same_answer_every_time_it_is_asked() {
        let rules = set(ExclusionSource::WorkspaceFile, "target/\n!target/keep\n");
        let candidate = path("target/debug/x");
        let first = rules.verdict(&candidate);
        for _ in 0..64 {
            assert_eq!(rules.verdict(&candidate), first);
        }
        assert_eq!(
            rules.verdict(&path("target/keep/note.txt")),
            Exclusion::Included {
                reinstated_by: Some(ExclusionSource::WorkspaceFile)
            }
        );
    }

    #[test]
    fn admission_splits_bytes_by_the_predicate() {
        let rules = set(ExclusionSource::WorkspaceFile, "target/\n");
        let admission = Admission::decide(
            &rules,
            [
                (path("src/lib.rs"), 1_024),
                (path("target/debug/x"), 8_388_608),
                (path("target/debug/y"), 4_194_304),
            ],
        );
        assert_eq!(admission.admitted_bytes(), 1_024);
        assert_eq!(admission.refused_bytes(), 12_582_912);
        assert_eq!(admission.admitted().count(), 1);
        assert!(admission
            .refused()
            .all(|(_, source, _)| source == ExclusionSource::WorkspaceFile));
    }

    #[test]
    fn every_source_carries_a_label_a_person_can_read() {
        for source in ExclusionSource::ALL {
            assert!(!source.label().is_empty());
            assert_eq!(format!("{source}"), source.label());
        }
        assert_eq!(
            ExclusionSource::WorkspaceFile.label(),
            WORKSPACE_EXCLUSION_FILE_NAME
        );
    }
}
