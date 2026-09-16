//! Project test detection: which build tool a project uses, decided from an inventory of paths.
//!
//! Plan §9.4 wants Cargo, npm/pnpm/yarn, pyproject, go, Make and continuous-integration
//! configuration detected. What that turns into here is a **pure function of a path list**. It
//! opens nothing and shells out to nothing, which is not a stylistic preference: detection that
//! read the disk would run project code (a `Makefile` is a program, `package.json` names scripts)
//! before the user had approved anything, and the first acceptance criterion of this task forbids
//! exactly that.
//!
//! Detection **proposes**. A detected tool becomes a validator, a validator becomes a proposed
//! command, and a proposed command becomes an executable one only through
//! [`ProfileProposal::approve`](crate::ProfileProposal::approve).
//!
//! # The one ambiguity, decided once
//!
//! A JavaScript project can carry more than one lockfile. Detecting all of them would ask three
//! package managers to install three trees, so the inventory records exactly one, by a fixed
//! precedence: **pnpm, then yarn, then npm**, and npm when a `package.json` carries no lockfile at
//! all. The precedence is a constant, not a heuristic, so two machines given the same path list
//! reach the same answer.

use std::collections::BTreeSet;

/// A build or test tool a project was detected to use.
///
/// Ordered as declared, and that order is the order [`ToolingInventory::iter`] yields, so a plan
/// built from an inventory does not depend on how the inventory was assembled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DetectedTool {
    /// A Cargo workspace or package.
    Cargo,
    /// A JavaScript project managed with npm.
    Npm,
    /// A JavaScript project managed with pnpm.
    Pnpm,
    /// A JavaScript project managed with Yarn.
    Yarn,
    /// A Python project with a `pyproject.toml`.
    Pyproject,
    /// A Go module.
    Go,
    /// A project with a make file.
    Make,
    /// A project with continuous-integration configuration.
    ContinuousIntegration,
}

impl DetectedTool {
    /// Every tool this crate can detect. Exhaustive by construction: the matches below stop
    /// compiling if a variant is added without being listed here.
    pub const ALL: [Self; 8] = [
        Self::Cargo,
        Self::Npm,
        Self::Pnpm,
        Self::Yarn,
        Self::Pyproject,
        Self::Go,
        Self::Make,
        Self::ContinuousIntegration,
    ];

    /// The tool's stable wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Pyproject => "pyproject",
            Self::Go => "go",
            Self::Make => "make",
            Self::ContinuousIntegration => "continuous-integration",
        }
    }

    /// Whether this tool is one of the JavaScript package managers, of which an inventory holds at
    /// most one.
    #[must_use]
    pub const fn is_package_manager(self) -> bool {
        matches!(self, Self::Npm | Self::Pnpm | Self::Yarn)
    }
}

impl core::fmt::Display for DetectedTool {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The make files a project may name its build rules in.
const MAKE_FILES: [&str; 3] = ["Makefile", "makefile", "GNUmakefile"];

/// Continuous-integration configuration that is one exact path rather than a directory.
const CI_FILES: [&str; 4] = [
    ".gitlab-ci.yml",
    ".circleci/config.yml",
    "azure-pipelines.yml",
    ".travis.yml",
];

/// The directory whose YAML files are continuous-integration workflows.
const CI_WORKFLOW_DIR: &str = ".github/workflows/";

/// What a project was detected to build and test with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolingInventory {
    tools: BTreeSet<DetectedTool>,
}

impl ToolingInventory {
    /// An inventory of nothing. A project detected to use no tooling proposes no commands, which
    /// is the fail-closed answer rather than a guess.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Detect the tooling a project uses from the paths it holds.
    ///
    /// Paths are matched by **final component**, so a Cargo workspace member's manifest counts as
    /// much as the root one, except for the continuous-integration workflow directory, which is
    /// matched by prefix because its file names are arbitrary.
    #[must_use]
    pub fn detect<'paths>(paths: impl IntoIterator<Item = &'paths str>) -> Self {
        let mut tools = BTreeSet::new();
        let mut package_json = false;
        let mut lockfiles = BTreeSet::new();

        for path in paths {
            let name = path.rsplit('/').next().unwrap_or(path);
            match name {
                "Cargo.toml" => {
                    tools.insert(DetectedTool::Cargo);
                }
                "pyproject.toml" => {
                    tools.insert(DetectedTool::Pyproject);
                }
                "go.mod" => {
                    tools.insert(DetectedTool::Go);
                }
                "package.json" => package_json = true,
                "pnpm-lock.yaml" => {
                    lockfiles.insert(DetectedTool::Pnpm);
                }
                "yarn.lock" => {
                    lockfiles.insert(DetectedTool::Yarn);
                }
                "package-lock.json" => {
                    lockfiles.insert(DetectedTool::Npm);
                }
                _ => {}
            }
            if MAKE_FILES.contains(&name) {
                tools.insert(DetectedTool::Make);
            }
            if CI_FILES.contains(&path)
                || (path.starts_with(CI_WORKFLOW_DIR)
                    && (path.ends_with(".yml") || path.ends_with(".yaml")))
            {
                tools.insert(DetectedTool::ContinuousIntegration);
            }
        }

        if package_json {
            tools.insert(package_manager(&lockfiles));
        }
        Self { tools }
    }

    /// Whether `tool` was detected.
    #[must_use]
    pub fn contains(&self, tool: DetectedTool) -> bool {
        self.tools.contains(&tool)
    }

    /// The detected tools, in [`DetectedTool`] declaration order.
    pub fn iter(&self) -> impl Iterator<Item = DetectedTool> + '_ {
        self.tools.iter().copied()
    }

    /// The one JavaScript package manager detected, when a `package.json` was present.
    #[must_use]
    pub fn package_manager(&self) -> Option<DetectedTool> {
        self.tools
            .iter()
            .copied()
            .find(|tool| tool.is_package_manager())
    }

    /// How many tools were detected.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether nothing was detected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

/// The one package manager a `package.json` resolves to, by fixed precedence.
fn package_manager(lockfiles: &BTreeSet<DetectedTool>) -> DetectedTool {
    for candidate in [DetectedTool::Pnpm, DetectedTool::Yarn, DetectedTool::Npm] {
        if lockfiles.contains(&candidate) {
            return candidate;
        }
    }
    DetectedTool::Npm
}

#[cfg(test)]
mod tests {
    use super::{DetectedTool, ToolingInventory};

    #[test]
    fn each_marker_is_detected() {
        let cases: [(&str, DetectedTool); 6] = [
            ("Cargo.toml", DetectedTool::Cargo),
            ("pyproject.toml", DetectedTool::Pyproject),
            ("go.mod", DetectedTool::Go),
            ("Makefile", DetectedTool::Make),
            ("makefile", DetectedTool::Make),
            ("GNUmakefile", DetectedTool::Make),
        ];
        for (path, expected) in cases {
            let inventory = ToolingInventory::detect([path]);
            assert!(inventory.contains(expected), "`{path}` detected nothing");
            assert_eq!(inventory.len(), 1, "`{path}` detected more than one tool");
        }
    }

    #[test]
    fn a_workspace_member_manifest_counts() {
        let inventory = ToolingInventory::detect(["crates/mesh-validator/Cargo.toml"]);
        assert!(inventory.contains(DetectedTool::Cargo));
    }

    #[test]
    fn the_package_manager_precedence_is_fixed() {
        let all = ToolingInventory::detect([
            "package.json",
            "package-lock.json",
            "yarn.lock",
            "pnpm-lock.yaml",
        ]);
        assert_eq!(all.package_manager(), Some(DetectedTool::Pnpm));

        let yarn_and_npm =
            ToolingInventory::detect(["package.json", "package-lock.json", "yarn.lock"]);
        assert_eq!(yarn_and_npm.package_manager(), Some(DetectedTool::Yarn));

        let npm = ToolingInventory::detect(["package.json", "package-lock.json"]);
        assert_eq!(npm.package_manager(), Some(DetectedTool::Npm));

        let bare = ToolingInventory::detect(["package.json"]);
        assert_eq!(bare.package_manager(), Some(DetectedTool::Npm));
    }

    #[test]
    fn a_lockfile_without_a_package_json_detects_nothing() {
        assert!(ToolingInventory::detect(["pnpm-lock.yaml"]).is_empty());
    }

    #[test]
    fn continuous_integration_is_detected_by_directory_and_by_name() {
        for path in [
            ".github/workflows/ci.yml",
            ".github/workflows/nightly.yaml",
            ".gitlab-ci.yml",
            ".circleci/config.yml",
            "azure-pipelines.yml",
            ".travis.yml",
        ] {
            assert!(
                ToolingInventory::detect([path]).contains(DetectedTool::ContinuousIntegration),
                "`{path}` was not detected as CI configuration"
            );
        }
        assert!(
            !ToolingInventory::detect([".github/workflows/README.md"])
                .contains(DetectedTool::ContinuousIntegration),
            "a non-YAML file in the workflow directory is not a workflow"
        );
        assert!(
            !ToolingInventory::detect(["docs/.circleci/config.yml"])
                .contains(DetectedTool::ContinuousIntegration),
            "a CI file matched by exact path must not match a nested copy"
        );
    }

    #[test]
    fn detection_is_order_independent() {
        let forwards = ToolingInventory::detect(["Cargo.toml", "package.json", "go.mod"]);
        let backwards = ToolingInventory::detect(["go.mod", "package.json", "Cargo.toml"]);
        assert_eq!(forwards, backwards);
        assert_eq!(forwards.len(), 3);
    }

    #[test]
    fn an_empty_inventory_detects_nothing() {
        let empty = ToolingInventory::empty();
        assert!(empty.is_empty());
        assert_eq!(empty.package_manager(), None);
        for tool in DetectedTool::ALL {
            assert!(!empty.contains(tool));
        }
    }

    #[test]
    fn the_wire_names_are_distinct() {
        let names: Vec<&str> = DetectedTool::ALL.iter().map(|tool| tool.as_str()).collect();
        for (at, name) in names.iter().enumerate() {
            assert!(!names[at + 1..].contains(name), "`{name}` is repeated");
        }
    }
}
