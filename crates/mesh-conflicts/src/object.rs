//! What an object is, and what it holds.

use crate::ids::VersionId;

/// Whether an object holds content or other objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectKind {
    /// Holds content.
    File,
    /// Holds other objects.
    Directory,
}

/// One durable version of one object's content.
///
/// A version is either text, which three-way merges, or bytes, which do not. That distinction is
/// the whole difference between conflict rows four and five on one hand and row six on the other,
/// and it is decided by whoever wrote the version rather than inferred here — a heuristic that
/// guessed wrong in the merging direction would corrupt a person's file, and this crate has no
/// business making that guess.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    /// Lines of text, newlines excluded.
    Text {
        /// The identity of this version.
        version: VersionId,
        /// The lines.
        lines: Vec<String>,
    },
    /// Opaque bytes, identified by their content hash.
    Binary {
        /// The identity of this version.
        version: VersionId,
        /// The content hash of the bytes.
        digest: [u8; 32],
        /// How many bytes there are.
        byte_length: u64,
    },
}

impl Content {
    /// The identity of this version.
    #[must_use]
    pub const fn version(&self) -> VersionId {
        match self {
            Self::Text { version, .. } | Self::Binary { version, .. } => *version,
        }
    }

    /// The lines, when this version is text.
    #[must_use]
    pub fn lines(&self) -> Option<&[String]> {
        match self {
            Self::Text { lines, .. } => Some(lines),
            Self::Binary { .. } => None,
        }
    }

    /// Whether this version is opaque bytes.
    #[must_use]
    pub const fn is_binary(&self) -> bool {
        matches!(self, Self::Binary { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_reports_its_lines_and_binary_reports_none() {
        let text = Content::Text {
            version: VersionId::from_bytes([1; 32]),
            lines: vec!["one".into()],
        };
        let binary = Content::Binary {
            version: VersionId::from_bytes([2; 32]),
            digest: [0; 32],
            byte_length: 4,
        };
        assert_eq!(text.lines(), Some(&["one".to_owned()][..]));
        assert_eq!(binary.lines(), None);
        assert!(!text.is_binary());
        assert!(binary.is_binary());
    }

    #[test]
    fn both_shapes_report_their_version() {
        let text = Content::Text {
            version: VersionId::from_bytes([1; 32]),
            lines: vec![],
        };
        assert_eq!(text.version(), VersionId::from_bytes([1; 32]));
    }
}
