//! Capturing the environment a run happened in.
//!
//! Nothing here has a default. Every probe either produces the real value or
//! returns a [`ProbeError`] naming the field it could not establish, and the
//! run fails. A benchmark row with a guessed CPU model or an assumed build
//! profile is worse than no row: it is a number nobody can contradict.

pub mod build_info;
mod exec;
mod filesystem;
mod git;
mod host;
mod system;

pub use system::SystemProbe;

use crate::schema::{BuildProfile, Hardware, HostPlatform, Repository};
use std::fmt;

/// Everything about the machine, the build and the commit under measurement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Environment {
    /// The commit the measured binary was built from.
    pub repository: Repository,
    /// The machine.
    pub hardware: Hardware,
    /// The OS and the filesystem the workload data lives on.
    pub platform: HostPlatform,
    /// How the binary was compiled.
    pub build: BuildProfile,
}

/// A source of [`Environment`] facts.
///
/// The seam exists so a harness run in a test does not shell out, and so a
/// future container or CI probe can replace the host probe without touching the
/// harness.
pub trait EnvironmentProbe {
    /// Captures the environment, or fails naming the field it could not get.
    fn probe(&self) -> Result<Environment, ProbeError>;
}

/// A fact about the environment could not be established.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeError {
    /// A helper command failed or was absent.
    Command {
        /// The command that was run.
        program: String,
        /// What went wrong.
        detail: String,
    },
    /// The value was not present where it was expected.
    Missing {
        /// The schema field that would have carried it.
        field: &'static str,
        /// Where it was looked for.
        detail: String,
    },
    /// This build cannot probe this host at all.
    UnsupportedHost {
        /// The target OS the probe was compiled for.
        os: &'static str,
    },
}

impl ProbeError {
    /// Builds a [`ProbeError::Missing`].
    pub fn missing(field: &'static str, detail: impl Into<String>) -> Self {
        ProbeError::Missing {
            field,
            detail: detail.into(),
        }
    }

    /// Builds a [`ProbeError::Command`].
    pub fn command(program: impl Into<String>, detail: impl Into<String>) -> Self {
        ProbeError::Command {
            program: program.into(),
            detail: detail.into(),
        }
    }
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProbeError::Command { program, detail } => {
                write!(f, "probe command `{program}` failed: {detail}")
            }
            ProbeError::Missing { field, detail } => {
                write!(f, "cannot establish `{field}`: {detail}")
            }
            ProbeError::UnsupportedHost { os } => {
                write!(f, "no environment probe implemented for `{os}`")
            }
        }
    }
}

impl std::error::Error for ProbeError {}
