//! The host probe: the real machine, the real commit, the real build.

use super::filesystem::probe_filesystem;
use super::git::probe_repository;
use super::host::{probe_hardware, probe_platform};
use super::{build_info, Environment, EnvironmentProbe, ProbeError};
use std::path::{Path, PathBuf};

/// Probes the machine this process is running on.
///
/// Two paths, because they answer two different questions: `repo_root` is the
/// checkout whose commit is under measurement, `data_root` is where the workload
/// keeps its bytes and therefore which filesystem the numbers describe. They are
/// frequently not the same volume, and conflating them is how a tmpfs run gets
/// published as an APFS run.
#[derive(Clone, Debug)]
pub struct SystemProbe {
    repo_root: PathBuf,
    data_root: PathBuf,
    remote_name: String,
}

impl SystemProbe {
    /// Probes `repo_root` for the commit and `data_root` for the filesystem.
    pub fn new(repo_root: impl Into<PathBuf>, data_root: impl Into<PathBuf>) -> Self {
        SystemProbe {
            repo_root: repo_root.into(),
            data_root: data_root.into(),
            remote_name: "origin".to_owned(),
        }
    }

    /// Uses the current directory as both the checkout and the data location.
    pub fn here() -> Result<Self, ProbeError> {
        let cwd = std::env::current_dir().map_err(|error| {
            ProbeError::missing(
                "repository.commit",
                format!("no current directory: {error}"),
            )
        })?;
        Ok(SystemProbe::new(cwd.clone(), cwd))
    }

    /// Reads the commit's remote URL from a different remote than `origin`.
    #[must_use]
    pub fn with_remote_name(mut self, remote_name: impl Into<String>) -> Self {
        self.remote_name = remote_name.into();
        self
    }

    /// The checkout this probe reads the commit from.
    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    /// The directory this probe resolves the filesystem from.
    pub fn data_root(&self) -> &Path {
        &self.data_root
    }
}

impl EnvironmentProbe for SystemProbe {
    fn probe(&self) -> Result<Environment, ProbeError> {
        let repository = probe_repository(&self.repo_root, &self.remote_name)?;
        let hardware = probe_hardware()?;
        let filesystem = probe_filesystem(&self.data_root)?;
        let platform = probe_platform(filesystem)?;
        Ok(Environment {
            repository,
            hardware,
            platform,
            build: build_info::build_profile(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_checkout_fails_before_any_other_probe() {
        let probe = SystemProbe::new(std::env::temp_dir(), std::env::temp_dir());
        let error = probe
            .probe()
            .expect_err("the temp directory is not a checkout");
        let message = error.to_string();
        assert!(
            message.contains("git") || message.contains("repository.commit"),
            "unexpected failure: {message}"
        );
    }

    #[test]
    fn the_probe_reports_the_paths_it_was_given() {
        let probe = SystemProbe::new("/a", "/b").with_remote_name("upstream");
        assert_eq!(probe.repo_root(), Path::new("/a"));
        assert_eq!(probe.data_root(), Path::new("/b"));
    }
}
