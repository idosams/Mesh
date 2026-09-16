//! Directory entry names, and the one place this crate mints a new one.
//!
//! Conflict row eight — same-name create — is the only rule in the table that has to produce a
//! name rather than choose between two. Both objects are retained, so both need somewhere to hang,
//! and two entries cannot share one name in one directory. [`NormalizedName::disambiguated`] is
//! that derivation: it is a pure function of the losing object's identifier, so every peer
//! computes the same name from the same operation set without exchanging a message about it.

use core::fmt;

/// The longest a name may be, in bytes.
///
/// Matches the practical ceiling of the filesystems Mesh materializes onto. A disambiguated name
/// is longer than the name it came from, so the check runs after the derivation, never before.
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

    /// This name with the object's short identifier folded in, for conflict row eight.
    ///
    /// `notes.md` under object `a1b2c3d4…` becomes `notes~a1b2c3d4.md`. The suffix goes before the
    /// final extension so a materialized file keeps opening in the same application — a preserved
    /// version a person cannot open is preserved only in the narrowest sense.
    ///
    /// Truncates the stem, never the suffix, when the result would exceed [`MAX_NAME_BYTES`]:
    /// losing part of a stem costs legibility, losing part of the suffix costs uniqueness, and
    /// uniqueness is what the rule is for.
    #[must_use]
    pub fn disambiguated(&self, marker: &str) -> Self {
        let (stem, extension) = split_extension(&self.0);
        let tail = format!("~{marker}{extension}");
        let room = MAX_NAME_BYTES.saturating_sub(tail.len());
        let mut kept = stem.len().min(room);
        while kept > 0 && !stem.is_char_boundary(kept) {
            kept -= 1;
        }
        Self(format!("{}{tail}", &stem[..kept]))
    }
}

impl fmt::Display for NormalizedName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The stem and the final extension, where a leading dot is part of the stem.
///
/// `.gitignore` has no extension: a dotfile whose whole name is the extension would disambiguate
/// to `~a1b2c3d4.gitignore`, which reads as a different file rather than as a second version of
/// this one.
fn split_extension(text: &str) -> (&str, &str) {
    match text.rfind('.') {
        Some(0) | None => (text, ""),
        Some(at) => (&text[..at], &text[at..]),
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
    fn disambiguation_keeps_the_extension() {
        let name = NormalizedName::new("notes.md").unwrap();
        assert_eq!(name.disambiguated("a1b2c3d4").as_str(), "notes~a1b2c3d4.md");
    }

    #[test]
    fn disambiguation_of_a_dotfile_keeps_the_whole_name_as_the_stem() {
        let name = NormalizedName::new(".gitignore").unwrap();
        assert_eq!(
            name.disambiguated("a1b2c3d4").as_str(),
            ".gitignore~a1b2c3d4"
        );
    }

    #[test]
    fn disambiguation_truncates_the_stem_and_never_the_marker() {
        let name = NormalizedName::new(&format!("{}.md", "s".repeat(250))).unwrap();
        let out = name.disambiguated("a1b2c3d4");
        assert!(out.as_str().len() <= MAX_NAME_BYTES);
        assert!(out.as_str().ends_with("~a1b2c3d4.md"));
        assert!(NormalizedName::new(out.as_str()).is_ok());
    }

    #[test]
    fn disambiguation_of_a_multibyte_stem_stays_on_a_character_boundary() {
        let name = NormalizedName::new(&format!("{}.md", "é".repeat(126))).unwrap();
        let out = name.disambiguated("a1b2c3d4");
        assert!(out.as_str().len() <= MAX_NAME_BYTES);
        assert!(NormalizedName::new(out.as_str()).is_ok());
    }
}
