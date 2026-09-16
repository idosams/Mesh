//! The isolated workspace a validation runs in.
//!
//! # What this type is
//!
//! A **read-only handle onto one immutable review snapshot**. It carries the snapshot's digest and
//! the relative root the executor materialised it at, and that is all it carries.
//!
//! What it deliberately has no way to express:
//!
//! * **A write.** There is no `&mut self` method, no path-yielding method that is not `&str`, and
//!   no constructor that takes anything a caller could later write through. The crate compiles
//!   against no filesystem API at all — `crate::no_ambient_io` asserts at compile time that no
//!   source here names the four ambient `std` modules — so there is no route from this handle to
//!   a byte on disk inside this crate.
//! * **The actor's own state.** [`IsolatedWorkspace::over`] refuses an absolute root and refuses
//!   any root holding a `..` component, so a root is always *under* wherever the executor put it
//!   and can never climb out to the actor's private state directory.
//!
//! # What this type is NOT
//!
//! The sandbox. This crate cannot confine a process; it holds no process. The confinement is the
//! executor's — a `ValidatorExecutor` implementation in the service layer, chosen by the
//! composition root — and this handle is the *contract* it is given: an immutable snapshot digest
//! and a relative root.
//!
//! What this crate does enforce, and what a test asserts, is the **detection** half:
//! [`crate::ExecutionReport`] must report the snapshot digest it observed after the run, and
//! [`crate::execute_plan`] turns a mismatch into [`RunFault::SnapshotDrift`](crate::RunFault) —
//! never into a pass. A validator that mutated what it was validating fails, and it fails on
//! evidence rather than on trust.

use mesh_types::Digest32;

/// The longest workspace root this crate will carry, in bytes.
pub const MAX_ROOT_BYTES: usize = 1024;

/// A read-only handle onto the immutable review snapshot a validation runs against.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IsolatedWorkspace {
    snapshot: Digest32,
    root: String,
}

impl IsolatedWorkspace {
    /// A workspace over `snapshot`, materialised at `root`.
    ///
    /// `root` is relative to wherever the executor put the sandbox; the empty string names that
    /// place itself.
    ///
    /// # Errors
    ///
    /// [`WorkspaceError`] when the root is absolute, over [`MAX_ROOT_BYTES`], holds a `.` or `..`
    /// component, or holds a NUL or a backslash.
    pub fn over(snapshot: Digest32, root: &str) -> Result<Self, WorkspaceError> {
        if root.len() > MAX_ROOT_BYTES {
            return Err(WorkspaceError::RootTooLong { bytes: root.len() });
        }
        if root.starts_with('/') {
            return Err(WorkspaceError::AbsoluteRoot(root.to_owned()));
        }
        if root.contains('\0') || root.contains('\\') {
            return Err(WorkspaceError::IllegalByte(root.to_owned()));
        }
        if !root.is_empty()
            && root
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(WorkspaceError::EscapingRoot(root.to_owned()));
        }
        Ok(Self {
            snapshot,
            root: root.to_owned(),
        })
    }

    /// The immutable review snapshot this workspace holds.
    ///
    /// The value a validation run's record is bound to, and the value
    /// [`crate::execute_plan`] compares an executor's report against.
    #[must_use]
    pub const fn snapshot(&self) -> Digest32 {
        self.snapshot
    }

    /// Where the snapshot was materialised, relative to the sandbox. Empty means the sandbox root.
    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Whether `after` is the snapshot this workspace was created over.
    ///
    /// The predicate behind the drift check. Stated as a method rather than inlined so a caller
    /// outside this crate can ask the same question of the same value.
    #[must_use]
    pub fn is_unchanged(&self, after: Digest32) -> bool {
        self.snapshot == after
    }
}

/// Why an isolated workspace was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceError {
    /// The root was over [`MAX_ROOT_BYTES`].
    RootTooLong {
        /// How many bytes it held.
        bytes: usize,
    },
    /// The root was absolute, so it names a place outside the sandbox.
    AbsoluteRoot(String),
    /// The root held a `.` or `..` component, so it can climb out of the sandbox.
    EscapingRoot(String),
    /// The root held a NUL or a backslash.
    IllegalByte(String),
}

impl core::fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RootTooLong { bytes } => {
                write!(formatter, "a workspace root of {bytes} bytes is too long")
            }
            Self::AbsoluteRoot(root) => write!(formatter, "`{root}` is an absolute root"),
            Self::EscapingRoot(root) => {
                write!(formatter, "`{root}` can climb out of the sandbox")
            }
            Self::IllegalByte(root) => write!(formatter, "`{root}` holds a NUL or a backslash"),
        }
    }
}

impl std::error::Error for WorkspaceError {}

#[cfg(test)]
mod tests {
    use super::{IsolatedWorkspace, WorkspaceError, MAX_ROOT_BYTES};
    use mesh_types::Digest32;

    fn snapshot() -> Digest32 {
        Digest32::from_bytes([6; 32])
    }

    #[test]
    fn a_root_that_could_climb_out_is_refused() {
        for root in [
            "/",
            "/tmp/anything",
            "..",
            "../actor-state",
            "a/../../b",
            "./a",
            "a//b",
            "a\\b",
            "a\0b",
        ] {
            assert!(
                IsolatedWorkspace::over(snapshot(), root).is_err(),
                "`{root}` was accepted as an isolated root"
            );
        }
    }

    #[test]
    fn a_relative_root_and_the_sandbox_root_itself_are_accepted() {
        assert_eq!(
            IsolatedWorkspace::over(snapshot(), "")
                .expect("the sandbox root")
                .root(),
            ""
        );
        assert_eq!(
            IsolatedWorkspace::over(snapshot(), "snapshot/tree")
                .expect("a relative root")
                .root(),
            "snapshot/tree"
        );
    }

    #[test]
    fn an_over_long_root_is_refused() {
        let long = "a".repeat(MAX_ROOT_BYTES + 1);
        assert_eq!(
            IsolatedWorkspace::over(snapshot(), &long),
            Err(WorkspaceError::RootTooLong {
                bytes: MAX_ROOT_BYTES + 1
            })
        );
    }

    #[test]
    fn the_workspace_reports_the_snapshot_it_was_made_over() {
        let workspace = IsolatedWorkspace::over(snapshot(), "").expect("legal");
        assert_eq!(workspace.snapshot(), snapshot());
        assert!(workspace.is_unchanged(snapshot()));
        assert!(!workspace.is_unchanged(Digest32::from_bytes([7; 32])));
    }
}
