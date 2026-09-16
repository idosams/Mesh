//! What makes a validator apply to a change.
//!
//! A trigger is a **total, pure predicate** over [`ReviewChange`] and [`ToolingInventory`] and
//! nothing else. It reads no clock, no environment and no disk, so the same change and the same
//! inventory fire the same triggers on any machine — which is what makes "selection is
//! deterministic for a given change set" a property a test can assert rather than a hope.
//!
//! Triggers are also the **explanation**. A selected validator carries the list of triggers that
//! fired, so the answer to "why is this command in my profile proposal" is data the user is shown
//! before approving, not a comment in a selector.

use mesh_types::{Absorb, DigestHasher, DigestWriter};

use crate::change::{ChangeOperation, ReviewChange};
use crate::tooling::{DetectedTool, ToolingInventory};

/// One reason a validator applies.
///
/// `Ord` is derived so a set of triggers has a canonical order; the derived order follows the
/// declaration order below and then the payload.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ValidationTrigger {
    /// A changed path ends with this suffix — `".rs"`, `"Cargo.toml"`.
    PathSuffix(String),
    /// A changed path lies under this directory prefix — `"crates/"`.
    PathUnder(String),
    /// The project was detected to use this tool.
    Tool(DetectedTool),
    /// The change carries this operation kind.
    Operation(ChangeOperation),
    /// The change carries at least one conflict.
    ConflictsPresent,
    /// At least one output is stale with respect to its inputs.
    StaleInputsPresent,
    /// At least this many paths changed.
    PathsAtLeast(u32),
    /// Unconditional. The validator applies to every change.
    Always,
}

impl ValidationTrigger {
    /// Whether this trigger fires for `change` in a project with `tooling`.
    #[must_use]
    pub fn fires(&self, change: &ReviewChange, tooling: &ToolingInventory) -> bool {
        match self {
            Self::PathSuffix(suffix) => change
                .paths()
                .iter()
                .any(|changed| changed.path().ends_with(suffix.as_str())),
            Self::PathUnder(prefix) => change
                .paths()
                .iter()
                .any(|changed| changed.path().starts_with(prefix.as_str())),
            Self::Tool(tool) => tooling.contains(*tool),
            Self::Operation(operation) => change.carries(*operation),
            Self::ConflictsPresent => change.conflicts() > 0,
            Self::StaleInputsPresent => change.stale_inputs() > 0,
            Self::PathsAtLeast(count) => change.volume().paths() >= *count,
            Self::Always => true,
        }
    }

    /// A short, stable phrase naming the axis this trigger reads, for a surface that shows the
    /// user why a command is being proposed.
    #[must_use]
    pub const fn axis(&self) -> &'static str {
        match self {
            Self::PathSuffix(_) => "path-suffix",
            Self::PathUnder(_) => "path-under",
            Self::Tool(_) => "detected-tool",
            Self::Operation(_) => "operation-kind",
            Self::ConflictsPresent => "conflicts",
            Self::StaleInputsPresent => "stale-inputs",
            Self::PathsAtLeast(_) => "change-volume",
            Self::Always => "always",
        }
    }
}

impl core::fmt::Display for ValidationTrigger {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PathSuffix(suffix) => write!(formatter, "a changed path ends with `{suffix}`"),
            Self::PathUnder(prefix) => write!(formatter, "a changed path is under `{prefix}`"),
            Self::Tool(tool) => write!(formatter, "the project uses {tool}"),
            Self::Operation(operation) => {
                write!(formatter, "the change {}s", operation.as_str())
            }
            Self::ConflictsPresent => formatter.write_str("the change carries a conflict"),
            Self::StaleInputsPresent => formatter.write_str("an output is stale"),
            Self::PathsAtLeast(count) => write!(formatter, "at least {count} paths changed"),
            Self::Always => formatter.write_str("always"),
        }
    }
}

impl Absorb for ValidationTrigger {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.text(self.axis());
        match self {
            Self::PathSuffix(text) | Self::PathUnder(text) => {
                writer.text(text);
            }
            Self::Tool(tool) => {
                writer.text(tool.as_str());
            }
            Self::Operation(operation) => {
                writer.text(operation.as_str());
            }
            Self::PathsAtLeast(count) => {
                writer.u64(u64::from(*count));
            }
            Self::ConflictsPresent | Self::StaleInputsPresent | Self::Always => {
                writer.u64(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ValidationTrigger;
    use crate::change::{ChangeOperation, ChangedPath, PathEdit, ReviewChange};
    use crate::tooling::{DetectedTool, ToolingInventory};
    use mesh_types::Digest32;

    fn change() -> ReviewChange {
        ReviewChange::against(Digest32::from_bytes([1; 32]))
            .with_path(
                ChangedPath::new("crates/mesh-validator/src/lib.rs", PathEdit::Modified, 10)
                    .expect("legal"),
            )
            .expect("no duplicate")
            .with_operation(ChangeOperation::EditContent)
    }

    #[test]
    fn every_trigger_fires_when_its_axis_says_so_and_not_otherwise() {
        let tooling = ToolingInventory::detect(["Cargo.toml"]);
        let base = change();

        assert!(ValidationTrigger::Always.fires(&base, &tooling));

        assert!(ValidationTrigger::PathSuffix(".rs".to_owned()).fires(&base, &tooling));
        assert!(!ValidationTrigger::PathSuffix(".py".to_owned()).fires(&base, &tooling));

        assert!(ValidationTrigger::PathUnder("crates/".to_owned()).fires(&base, &tooling));
        assert!(!ValidationTrigger::PathUnder("docs/".to_owned()).fires(&base, &tooling));

        assert!(ValidationTrigger::Tool(DetectedTool::Cargo).fires(&base, &tooling));
        assert!(!ValidationTrigger::Tool(DetectedTool::Go).fires(&base, &tooling));

        assert!(ValidationTrigger::Operation(ChangeOperation::EditContent).fires(&base, &tooling));
        assert!(!ValidationTrigger::Operation(ChangeOperation::DeleteObject).fires(&base, &tooling));

        assert!(!ValidationTrigger::ConflictsPresent.fires(&base, &tooling));
        assert!(
            ValidationTrigger::ConflictsPresent.fires(&base.clone().with_conflicts(1), &tooling)
        );

        assert!(!ValidationTrigger::StaleInputsPresent.fires(&base, &tooling));
        assert!(ValidationTrigger::StaleInputsPresent
            .fires(&base.clone().with_stale_inputs(2), &tooling));

        assert!(ValidationTrigger::PathsAtLeast(1).fires(&base, &tooling));
        assert!(!ValidationTrigger::PathsAtLeast(2).fires(&base, &tooling));
    }

    #[test]
    fn a_trigger_on_an_empty_change_fires_only_when_unconditional() {
        let empty = ReviewChange::against(Digest32::from_bytes([0; 32]));
        let nothing = ToolingInventory::empty();
        assert!(ValidationTrigger::Always.fires(&empty, &nothing));
        assert!(!ValidationTrigger::PathSuffix(String::new()).fires(&empty, &nothing));
        assert!(ValidationTrigger::PathsAtLeast(0).fires(&empty, &nothing));
    }

    #[test]
    fn the_axis_names_are_distinct() {
        let triggers = [
            ValidationTrigger::PathSuffix(String::new()),
            ValidationTrigger::PathUnder(String::new()),
            ValidationTrigger::Tool(DetectedTool::Cargo),
            ValidationTrigger::Operation(ChangeOperation::EditContent),
            ValidationTrigger::ConflictsPresent,
            ValidationTrigger::StaleInputsPresent,
            ValidationTrigger::PathsAtLeast(0),
            ValidationTrigger::Always,
        ];
        let names: Vec<&str> = triggers.iter().map(ValidationTrigger::axis).collect();
        for (at, name) in names.iter().enumerate() {
            assert!(!names[at + 1..].contains(name), "`{name}` is repeated");
        }
    }
}
