//! The commit under measurement.

use super::exec::output_in;
use super::ProbeError;
use crate::schema::Repository;
use std::path::Path;

/// Reads the remote, commit and dirtiness of the repository at `repo_root`.
///
/// A directory that is not a repository is an error, not an empty commit: a row
/// whose commit cannot be resolved is not reproducible by anyone.
pub fn probe_repository(repo_root: &Path, remote_name: &str) -> Result<Repository, ProbeError> {
    let inside = output_in(repo_root, "git", &["rev-parse", "--is-inside-work-tree"])?;
    if inside != "true" {
        return Err(ProbeError::missing(
            "repository.commit",
            format!("{} is not inside a git work tree", repo_root.display()),
        ));
    }

    let commit = output_in(repo_root, "git", &["rev-parse", "HEAD"])?;
    if commit.len() != 40 {
        return Err(ProbeError::missing(
            "repository.commit",
            format!("`git rev-parse HEAD` returned `{commit}`"),
        ));
    }

    let remote = output_in(repo_root, "git", &["remote", "get-url", remote_name])?;
    if remote.is_empty() {
        return Err(ProbeError::missing(
            "repository.remote",
            format!("remote `{remote_name}` has no URL"),
        ));
    }

    let dirty = !output_in(repo_root, "git", &["status", "--porcelain"])?.is_empty();

    Ok(Repository {
        remote,
        commit,
        dirty,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn a_directory_outside_a_repository_is_rejected() {
        let error = probe_repository(&env::temp_dir(), "origin")
            .expect_err("the temp directory is not a checkout");
        match error {
            ProbeError::Missing { field, .. } => assert_eq!(field, "repository.commit"),
            ProbeError::Command { program, .. } => assert_eq!(program, "git"),
            other => panic!("unexpected probe failure: {other}"),
        }
    }
}
