//! The validator registry, automatic validator planning, and the seam validators run through.
//!
//! Plan §9.1 and §9.4. Validation should happen without the user creating a task, which means the
//! system decides what to run from the change itself. The danger in that is executing project
//! scripts silently, so **detection proposes and the user approves a profile once**.
//!
//! # The load-bearing property
//!
//! A validator cannot influence what it validates. Three separate mechanisms hold it, and none of
//! them is a check somebody has to remember to write:
//!
//! 1. **No write path exists in this crate.** [`no_ambient_io`] asserts at compile time that no
//!    source here names an ambient `std` module, so this crate cannot open a file, open a socket
//!    or spawn a process. Execution is a caller-supplied [`ValidatorExecutor`], and this crate's
//!    guarantee is about which commands reach that port, never about what happens beyond it.
//! 2. **No capability to advance canonical state.** [`ValidationAuthority`] is built only from a
//!    `Grant<Delegated>`, and `mesh-crypto`'s delegated action vocabulary has no
//!    canonical-advance variant to carry. The manifest matches: this crate declares no dependency
//!    on `mesh-approval`, which is the crate holding `canonical-mutation`, and
//!    `tools/program/arch-check`'s `validators-cannot-mutate-canonical-state` restriction is what
//!    keeps it that way.
//! 3. **A run that changed its subject is detected.** [`ExecutionReport`] must carry the snapshot
//!    digest measured after the run; a mismatch becomes [`RunFault::SnapshotDrift`] and never a
//!    pass.
//! 4. **Evidence names its review, and cannot be moved to another.** A [`ValidationPlan`] carries
//!    the immutable review snapshot it was planned for and every [`ValidationRecord`] carries the
//!    one it was produced against, so [`RunLedger::clears`] can require the two to be the same.
//!    Without that the ledger of a review that passed satisfies a review nothing ever ran against
//!    — the transferable form of a validator deciding its own verdict, since the actor chooses
//!    which evidence to submit. `tests/evidence-replay.rs` constructs the attack.
//!
//! # Fail closed
//!
//! An unknown or crashed validator must never read as a pass. [`RunOutcome`] has no exit-status
//! field to fill for a step that never started, [`RunVerdict::is_pass`] is true for exactly one
//! word, and [`RunLedger::clears`] requires, for *every* planned step, at least one record against
//! this review and no failing one. Three cases follow that a "no failures recorded" check gets
//! wrong: a step with no record at all fails, a step whose only evidence belongs to another review
//! fails, and a failure does not stop counting because a pass was recorded beside it.
//!
//! # The flow
//!
//! ```
//! use mesh_validator::{
//!     ChangedPath, IsolatedWorkspace, PathEdit, ProfileProposal, ReviewChange, ToolingInventory,
//!     ValidatorRegistry, plan_validation,
//! };
//! use mesh_types::Digest32;
//!
//! // 1. Describe the change, against the immutable review snapshot.
//! let snapshot = Digest32::from_bytes([1; 32]);
//! let change = ReviewChange::against(snapshot)
//!     .with_path(ChangedPath::new("src/lib.rs", PathEdit::Modified, 512)?)?;
//!
//! // 2. Detect the project's tooling from paths alone. Nothing is opened and nothing is run.
//! let tooling = ToolingInventory::detect(["Cargo.toml", "src/lib.rs"]);
//!
//! // 3. Plan. Deterministic for a given change set, and it starts nothing.
//! let plan = plan_validation(&ValidatorRegistry::standard(), &change, &tooling);
//! assert_eq!(plan.len(), 1);
//!
//! // 4. Propose. This is what the user is shown, once.
//! let proposal = ProfileProposal::from_plan(&plan);
//! assert_eq!(proposal.commands()[0].to_line(), "cargo nextest run");
//!
//! // 5. Until `proposal.approve(…)` returns a profile, `execute_plan` blocks every step.
//! let workspace = IsolatedWorkspace::over(snapshot, "")?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Module map
//!
//! | Module | What it owns |
//! |---|---|
//! | [`change`] | the change under review, canonically ordered |
//! | [`tooling`] | project test detection from a path list |
//! | [`trigger`] | what makes a validator apply |
//! | [`command`] | an argument vector and its two digests |
//! | [`registry`] | which validators exist |
//! | [`selection`] | automatic planning, deterministic per change set |
//! | [`profile`] | the one-time approval that lets anything run |
//! | [`workspace`] | the read-only handle onto the review snapshot |
//! | [`record`] | what ran, in what environment, with what exit status |
//! | [`run`] | the authority, the execution port, and the six refusals |

pub mod change;
pub mod command;
pub mod no_ambient_io;
pub mod profile;
pub mod record;
pub mod registry;
pub mod run;
pub mod selection;
pub mod tooling;
pub mod trigger;
pub mod workspace;

pub use crate::change::{
    ChangeError, ChangeOperation, ChangeVolume, ChangedPath, PathEdit, ReviewChange, MAX_PATH_BYTES,
};
pub use crate::command::{
    CommandDigest, CommandError, EnvironmentDigest, ValidationCommand, MAX_ARGUMENTS,
    MAX_ARGUMENT_BYTES,
};
pub use crate::profile::{ProfileProposal, ValidationProfile};
pub use crate::record::{
    Artifact, ArtifactError, BlockReason, ExitStatus, RunFault, RunLedger, RunOutcome, RunVerdict,
    ValidationRecord, ValidationRecordId, MAX_ARTIFACT_NAME_BYTES,
};
pub use crate::registry::{
    RegistryError, SandboxRequirement, ValidatorId, ValidatorRegistry, ValidatorSpec,
    MAX_VALIDATOR_ID_BYTES,
};
pub use crate::run::{
    execute_plan, AuthorityError, ExecutionFault, ExecutionReport, SandboxedRun,
    ValidationAuthority, ValidatorExecutor,
};
pub use crate::selection::{plan_validation, PlannedValidation, ValidationPlan};
pub use crate::tooling::{DetectedTool, ToolingInventory};
pub use crate::trigger::ValidationTrigger;
pub use crate::workspace::{IsolatedWorkspace, WorkspaceError, MAX_ROOT_BYTES};

/// The crate's name, so a placeholder still carries one verifiable behaviour.
pub const CRATE_NAME: &str = "mesh-validator";

#[cfg(test)]
mod tests {
    use super::CRATE_NAME;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-validator");
    }
}
