//! Directory entry names and portable metadata, mirrored from `mesh-types`.
//!
//! `src/ids.rs` states why this crate mirrors rather than imports. The rules below are
//! `mesh-types`' rules, character for character, and `tests/mesh_types_drift.rs` holds them
//! against that crate's source: a name this crate accepts and `mesh-types` rejects would be an
//! operation that encodes and cannot be materialized.

use core::fmt;

/// A directory entry name that is structurally usable as one.
///
/// Rejects the names that cannot be an entry on any platform: the empty string, a name containing
/// a path separator or a NUL byte, and the two relative names. It does **not** apply a Unicode
/// normalization form — the plan calls for filename normalization but does not specify which, and
/// this crate has no authority to pin a protocol-visible rule `mesh-materializer` owns. Widening
/// what is accepted is a compatibility event.
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

/// The subset of file metadata Mesh carries across platforms without loss.
///
/// One field today. The set is `mesh-materializer`'s to define, and widening it moves every
/// version identifier that binds it — a compatibility event, not a field addition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PortableMetadata {
    executable: bool,
}

impl PortableMetadata {
    /// Portable metadata with the executable bit set as given.
    #[must_use]
    pub const fn new(executable: bool) -> Self {
        Self { executable }
    }

    /// Whether the file is executable.
    #[must_use]
    pub const fn is_executable(&self) -> bool {
        self.executable
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
        assert_eq!(
            NormalizedName::new("\u{6c34}").unwrap().to_string(),
            "\u{6c34}"
        );
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
    fn portable_metadata_defaults_to_not_executable() {
        assert!(!PortableMetadata::default().is_executable());
        assert!(PortableMetadata::new(true).is_executable());
    }
}
