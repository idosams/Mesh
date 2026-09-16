//! Directory entry names, mirrored from `mesh-types`.
//!
//! `src/ids.rs` states why this crate mirrors rather than imports. The rules below are
//! `mesh-types`' rules, character for character, and `tests/mesh_types_drift.rs` holds them against
//! that crate's source: a name this register accepts and `mesh-types` rejects would be a directory
//! entry that resolves here and cannot be materialized.

use core::fmt;

/// A directory entry name that is structurally usable as one.
///
/// Rejects the names that cannot be an entry on any platform: the empty string, a name containing
/// a path separator or a NUL byte, and the two relative names. It does **not** apply a Unicode
/// normalization form — the plan calls for filename normalization but does not specify which, and
/// this crate has no authority to pin a protocol-visible rule `mesh-materializer` owns.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NormalizedName(String);

impl NormalizedName {
    /// Accept a name, rejecting the structurally impossible ones.
    ///
    /// # Errors
    ///
    /// [`NameError`] naming which rule the input broke.
    pub fn new(name: impl Into<String>) -> Result<Self, NameError> {
        let name = name.into();
        if name.is_empty() {
            return Err(NameError::Empty);
        }
        if name == "." || name == ".." {
            return Err(NameError::Relative);
        }
        if name.contains('/') || name.contains('\\') {
            return Err(NameError::Separator);
        }
        if name.contains('\0') {
            return Err(NameError::Nul);
        }
        Ok(Self(name))
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// How many bytes the name occupies.
    ///
    /// The register reports the metadata cost of a change, and a name is the only part of a
    /// directory-entry change whose width is not fixed.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.0.len()
    }
}

impl fmt::Display for NormalizedName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Why a directory entry name was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    /// The name was empty.
    Empty,
    /// The name was `.` or `..`.
    Relative,
    /// The name contained a path separator.
    Separator,
    /// The name contained a NUL byte.
    Nul,
}

impl fmt::Display for NameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "a directory entry name is not empty",
            Self::Relative => "a directory entry name is neither \".\" nor \"..\"",
            Self::Separator => "a directory entry name contains no path separator",
            Self::Nul => "a directory entry name contains no NUL byte",
        })
    }
}

impl std::error::Error for NameError {}

/// A path an object has, or once had, in the workspace.
///
/// A path is **derived**, never stored: it is the sequence of names the register reads by walking
/// an object's placement up to the root. That is the whole point — a path is a view of the entry
/// chain at one moment, so moving an ancestor changes every descendant's path without any
/// descendant record changing at all.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspacePath(Vec<NormalizedName>);

impl WorkspacePath {
    /// The path made of these names, root first.
    #[must_use]
    pub const fn new(segments: Vec<NormalizedName>) -> Self {
        Self(segments)
    }

    /// The names, root first.
    #[must_use]
    pub fn segments(&self) -> &[NormalizedName] {
        &self.0
    }

    /// How deep the path is. The root itself is depth zero.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.0.len()
    }
}

impl fmt::Display for WorkspacePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return formatter.write_str("/");
        }
        for segment in &self.0 {
            write!(formatter, "/{segment}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_name_is_accepted() {
        assert_eq!(
            NormalizedName::new("report.md").unwrap().as_str(),
            "report.md"
        );
        assert_eq!(NormalizedName::new("\u{6c34}").unwrap().byte_len(), 3);
    }

    #[test]
    fn the_structurally_impossible_names_are_refused() {
        assert_eq!(NormalizedName::new(""), Err(NameError::Empty));
        assert_eq!(NormalizedName::new("."), Err(NameError::Relative));
        assert_eq!(NormalizedName::new(".."), Err(NameError::Relative));
        assert_eq!(NormalizedName::new("a/b"), Err(NameError::Separator));
        assert_eq!(NormalizedName::new("a\\b"), Err(NameError::Separator));
        assert_eq!(NormalizedName::new("a\0b"), Err(NameError::Nul));
    }

    #[test]
    fn a_leading_dot_is_not_a_relative_name() {
        assert!(NormalizedName::new(".git").is_ok());
        assert!(NormalizedName::new("...").is_ok());
    }

    #[test]
    fn a_path_renders_root_first() {
        let path = WorkspacePath::new(vec![
            NormalizedName::new("src").unwrap(),
            NormalizedName::new("main.rs").unwrap(),
        ]);
        assert_eq!(path.to_string(), "/src/main.rs");
        assert_eq!(path.depth(), 2);
        assert_eq!(WorkspacePath::new(Vec::new()).to_string(), "/");
    }
}
