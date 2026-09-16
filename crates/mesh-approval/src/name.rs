//! Directory entry names, mirrored from `mesh-conflicts` for the reason given in [`crate::ids`].
//!
//! A bundle renders a path so a person can read it, and a path is a sequence of these. Checking the
//! name once at construction is what stops a bundle from carrying a path whose segments could be
//! read two ways — `a/b` as one segment and `a/b` as two are different states, and a review that
//! could not tell them apart is a review of nothing in particular.
//!
//! `mesh-conflicts`' copy also derives a disambiguated name for conflict row eight. That is a
//! resolution act, not a review act, and it is deliberately not mirrored here.

use core::fmt;

/// The longest a name may be, in bytes.
pub const MAX_NAME_BYTES: usize = 255;

/// Why a name was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NameError {
    /// The name was empty.
    Empty,
    /// The name was longer than [`MAX_NAME_BYTES`].
    TooLong,
    /// The name contained a separator or a NUL byte.
    Separator,
    /// The name was `.` or `..`, which name a directory rather than an entry in one.
    Relative,
}

impl fmt::Display for NameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Empty => "a directory entry name cannot be empty",
            Self::TooLong => "a directory entry name cannot exceed 255 bytes",
            Self::Separator => "a directory entry name cannot contain '/' or a NUL byte",
            Self::Relative => "'.' and '..' are not directory entry names",
        };
        formatter.write_str(text)
    }
}

impl std::error::Error for NameError {}

/// One directory entry name, checked once at construction.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NormalizedName(String);

impl NormalizedName {
    /// The name, if it is one.
    ///
    /// # Errors
    ///
    /// [`NameError`] when the text is empty, over-long, contains a separator or names `.` or `..`.
    pub fn new(text: &str) -> Result<Self, NameError> {
        if text.is_empty() {
            return Err(NameError::Empty);
        }
        if text.len() > MAX_NAME_BYTES {
            return Err(NameError::TooLong);
        }
        if text.contains('/') || text.contains('\0') {
            return Err(NameError::Separator);
        }
        if text == "." || text == ".." {
            return Err(NameError::Relative);
        }
        Ok(Self(text.to_owned()))
    }

    /// The text.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_name_is_accepted() {
        assert_eq!(
            NormalizedName::new("notes.md").unwrap().as_str(),
            "notes.md"
        );
    }

    #[test]
    fn the_four_refusals_are_refused() {
        assert_eq!(NormalizedName::new(""), Err(NameError::Empty));
        assert_eq!(NormalizedName::new("a/b"), Err(NameError::Separator));
        assert_eq!(NormalizedName::new("a\0b"), Err(NameError::Separator));
        assert_eq!(NormalizedName::new("."), Err(NameError::Relative));
        assert_eq!(NormalizedName::new(".."), Err(NameError::Relative));
        assert_eq!(
            NormalizedName::new(&"x".repeat(MAX_NAME_BYTES + 1)),
            Err(NameError::TooLong)
        );
    }

    #[test]
    fn names_order_lexicographically() {
        let first = NormalizedName::new("a").unwrap();
        let second = NormalizedName::new("b").unwrap();
        assert!(first < second);
    }
}
