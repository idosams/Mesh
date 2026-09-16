//! Git interoperability without making Git Mesh's internal state model.
//!
//! The first production boundary is import: inspect one exact repository root without running a
//! shell, a pager, a user-global configuration, or a configured filesystem-monitor executable.
//! The resulting [`GitProvenanceAnchor`] retains Git's own exact status bytes and object names so
//! a later workspace import can bind its Mesh state to the Git state the person actually had.

use std::process::Command;

mod export;
mod import;
mod workspace;

pub use export::{
    confirm_approved_git_export, inspect_approved_git_export, preview_approved_git_export,
    ApprovedGitExport, ApprovedGitExportSource, GitExportError, GitExportPreview,
};
pub use import::{
    GitImportError, GitObjectFormat, GitObjectId, GitPath, GitProvenanceAnchor, Gitlink,
    RepositoryIdentity,
};
pub use workspace::{
    install_independent_git_context, install_independent_git_context_for_destination,
    install_independent_git_history, install_independent_git_history_for_destination,
    GitContextError, GitDestinationIdentity, IndependentGitContext,
};

/// The crate's stable public name.
pub const CRATE_NAME: &str = "mesh-git-bridge";

/// Start Git without inheriting caller-controlled repository, index, object, transport, helper,
/// or configuration authority.
///
/// `PATH` is retained only so the platform Git executable remains discoverable, and `TMPDIR` is
/// retained so Git's ordinary private temporary files stay on the caller-selected local volume.
/// Every Git-specific variable is deliberately absent and each call site adds its exact required
/// configuration afterward.
pub(crate) fn isolated_git_command() -> Command {
    let path = std::env::var_os("PATH");
    let temporary_directory = std::env::var_os("TMPDIR");
    let mut command = Command::new("git");
    command.env_clear();
    if let Some(path) = path {
        command.env("PATH", path);
    }
    if let Some(temporary_directory) = temporary_directory {
        command.env("TMPDIR", temporary_directory);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-git-bridge");
    }
}
