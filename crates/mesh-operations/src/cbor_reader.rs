//! The `mesh-cbor/0` reader: strict enough that exactly one byte string decodes to any value.
//!
//! A writer alone can be deterministic while the format it writes still admits three other
//! spellings of the same record — which is how a second implementation ends up verifying a
//! signature over bytes it would never have produced. So this reader refuses:
//!
//! * a head wider than the argument needs (RFC 8949 §4.2.1 preferred serialization);
//! * an item nested deeper than [`MAX_NESTING`];
//! * an indefinite-length item, and the break code;
//! * a major type outside the profile: negative integers, maps, tags;
//! * any simple value other than `0xf4` and `0xf5`;
//! * bytes left over after a complete item.
//!
//! Together with [`CborWriter`](crate::CborWriter) this closes the loop: `decode(encode(v)) == v`
//! for every value the profile can hold, and `encode(decode(b)) == b` for every byte string it
//! accepts. `tests/round_trip.rs` exercises the second direction over a generated corpus.

use core::fmt;

use crate::cbor::{FALSE_BYTE, MAJOR_ARRAY, MAJOR_BYTES, MAJOR_TEXT, MAJOR_UNSIGNED, TRUE_BYTE};

/// How deeply an item may nest before the reader refuses it.
///
/// Skipping an item of unknown shape is recursive and array heads cost one byte each, so without a
/// limit a hostile peer buys one stack frame per byte and aborts the process. The bound is far
/// above anything the schemas produce: the deepest record here nests four levels.
pub const MAX_NESTING: usize = 64;

/// Why some bytes are not a `mesh-cbor/0` item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CborError {
    /// The item ran past the end of the input.
    Truncated {
        /// Where the item started.
        at: usize,
        /// How many more bytes it needed.
        needed: usize,
    },
    /// The head was wider than the argument needs. The profile has one spelling per value.
    NonCanonicalHead {
        /// Where the head started.
        at: usize,
        /// The argument that was encoded.
        argument: u64,
        /// The width that was used, in additional bytes.
        used: u8,
    },
    /// An indefinite-length item, or the break code that ends one.
    IndefiniteLength {
        /// Where it appeared.
        at: usize,
    },
    /// A major type the profile does not admit: negative integer, map or tag.
    UnsupportedMajorType {
        /// Where it appeared.
        at: usize,
        /// The major type.
        major: u8,
    },
    /// A simple value or float. The profile admits only `false` and `true`.
    UnsupportedSimpleValue {
        /// Where it appeared.
        at: usize,
        /// The initial byte.
        byte: u8,
    },
    /// A reserved additional-information value (28, 29 or 30).
    Malformed {
        /// Where it appeared.
        at: usize,
        /// The initial byte.
        byte: u8,
    },
    /// The item found was not the item the schema expected.
    TypeMismatch {
        /// Where it appeared.
        at: usize,
        /// What the schema expected.
        expected: &'static str,
        /// What was there.
        found: &'static str,
    },
    /// A text string that was not valid UTF-8.
    InvalidUtf8 {
        /// Where the string started.
        at: usize,
    },
    /// An item nested deeper than [`MAX_NESTING`].
    TooDeep {
        /// Where the item that exceeded the limit began.
        at: usize,
        /// The limit.
        limit: usize,
    },
}

impl fmt::Display for CborError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { at, needed } => {
                write!(formatter, "byte {at}: {needed} more bytes were needed")
            }
            Self::NonCanonicalHead { at, argument, used } => write!(
                formatter,
                "byte {at}: {argument} was written in a {used}-byte head, which is wider than it \
                 needs"
            ),
            Self::IndefiniteLength { at } => write!(
                formatter,
                "byte {at}: indefinite lengths are not in the profile"
            ),
            Self::UnsupportedMajorType { at, major } => write!(
                formatter,
                "byte {at}: major type {major} is not in the profile"
            ),
            Self::UnsupportedSimpleValue { at, byte } => write!(
                formatter,
                "byte {at}: {byte:#04x} is a simple value the profile does not admit"
            ),
            Self::Malformed { at, byte } => {
                write!(formatter, "byte {at}: {byte:#04x} is a reserved head")
            }
            Self::TypeMismatch {
                at,
                expected,
                found,
            } => write!(formatter, "byte {at}: expected {expected}, found {found}"),
            Self::InvalidUtf8 { at } => write!(formatter, "byte {at}: not valid UTF-8"),
            Self::TooDeep { at, limit } => {
                write!(formatter, "byte {at}: nested deeper than {limit}")
            }
        }
    }
}

impl std::error::Error for CborError {}

/// One decoded `mesh-cbor/0` head: what shape follows, and its argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Head {
    Unsigned(u64),
    Bytes(u64),
    Text(u64),
    Array(u64),
    Bool(bool),
}

impl Head {
    /// The published name of this shape, for a type-mismatch message.
    const fn name(self) -> &'static str {
        match self {
            Self::Unsigned(_) => "an unsigned integer",
            Self::Bytes(_) => "a byte string",
            Self::Text(_) => "a text string",
            Self::Array(_) => "an array",
            Self::Bool(_) => "a boolean",
        }
    }
}

/// A cursor over `mesh-cbor/0` bytes.
///
/// The reader knows the profile, never the schema: a caller that knows the schema asks for the
/// shape it expects and gets a [`CborError::TypeMismatch`] when the bytes hold something else.
#[derive(Clone, Debug)]
pub struct CborReader<'a> {
    bytes: &'a [u8],
    at: usize,
    depth: usize,
}

impl<'a> CborReader<'a> {
    /// A reader positioned at the start of `bytes`.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            at: 0,
            depth: 0,
        }
    }

    /// How many bytes have been consumed.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.at
    }

    /// How many bytes remain unconsumed.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }

    /// Whether every byte has been consumed.
    #[must_use]
    pub const fn is_exhausted(&self) -> bool {
        self.at == self.bytes.len()
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], CborError> {
        if self.bytes.len() - self.at < count {
            return Err(CborError::Truncated {
                at: self.at,
                needed: count - (self.bytes.len() - self.at),
            });
        }
        let slice = &self.bytes[self.at..self.at + count];
        self.at += count;
        Ok(slice)
    }

    /// Read one head, applying every profile rule that concerns it.
    fn head(&mut self) -> Result<Head, CborError> {
        let start = self.at;
        let initial = *self.take(1)?.first().expect("one byte was taken");
        let major = initial >> 5;
        let info = initial & 0x1f;

        if major == 7 {
            return match initial {
                FALSE_BYTE => Ok(Head::Bool(false)),
                TRUE_BYTE => Ok(Head::Bool(true)),
                0xff => Err(CborError::IndefiniteLength { at: start }),
                byte => Err(CborError::UnsupportedSimpleValue { at: start, byte }),
            };
        }
        if major != MAJOR_UNSIGNED
            && major != MAJOR_BYTES
            && major != MAJOR_TEXT
            && major != MAJOR_ARRAY
        {
            return Err(CborError::UnsupportedMajorType { at: start, major });
        }

        let argument = match info {
            0..=23 => u64::from(info),
            24 => {
                let value = u64::from(*self.take(1)?.first().expect("one byte was taken"));
                if value < 24 {
                    return Err(CborError::NonCanonicalHead {
                        at: start,
                        argument: value,
                        used: 1,
                    });
                }
                value
            }
            25 => {
                let raw = self.take(2)?;
                let value = u64::from(u16::from_be_bytes([raw[0], raw[1]]));
                if value <= u64::from(u8::MAX) {
                    return Err(CborError::NonCanonicalHead {
                        at: start,
                        argument: value,
                        used: 2,
                    });
                }
                value
            }
            26 => {
                let raw = self.take(4)?;
                let value = u64::from(u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]));
                if value <= u64::from(u16::MAX) {
                    return Err(CborError::NonCanonicalHead {
                        at: start,
                        argument: value,
                        used: 4,
                    });
                }
                value
            }
            27 => {
                let raw = self.take(8)?;
                let mut wide = [0u8; 8];
                wide.copy_from_slice(raw);
                let value = u64::from_be_bytes(wide);
                if value <= u64::from(u32::MAX) {
                    return Err(CborError::NonCanonicalHead {
                        at: start,
                        argument: value,
                        used: 8,
                    });
                }
                value
            }
            31 => return Err(CborError::IndefiniteLength { at: start }),
            _ => {
                return Err(CborError::Malformed {
                    at: start,
                    byte: initial,
                })
            }
        };

        Ok(match major {
            MAJOR_UNSIGNED => Head::Unsigned(argument),
            MAJOR_BYTES => Head::Bytes(argument),
            MAJOR_TEXT => Head::Text(argument),
            _ => Head::Array(argument),
        })
    }

    /// Read one head, remembering where it started so a mismatch can name the byte.
    fn located_head(&mut self) -> Result<(Head, usize), CborError> {
        let start = self.at;
        Ok((self.head()?, start))
    }

    /// Read an unsigned integer.
    pub fn unsigned(&mut self) -> Result<u64, CborError> {
        let (head, at) = self.located_head()?;
        match head {
            Head::Unsigned(value) => Ok(value),
            other => Err(CborError::TypeMismatch {
                at,
                expected: "an unsigned integer",
                found: other.name(),
            }),
        }
    }

    /// Read a boolean.
    pub fn bool(&mut self) -> Result<bool, CborError> {
        let (head, at) = self.located_head()?;
        match head {
            Head::Bool(value) => Ok(value),
            other => Err(CborError::TypeMismatch {
                at,
                expected: "a boolean",
                found: other.name(),
            }),
        }
    }

    /// Read a byte string.
    pub fn bytes(&mut self) -> Result<&'a [u8], CborError> {
        let (head, at) = self.located_head()?;
        match head {
            Head::Bytes(length) => self.take(usize::try_from(length).unwrap_or(usize::MAX)),
            other => Err(CborError::TypeMismatch {
                at,
                expected: "a byte string",
                found: other.name(),
            }),
        }
    }

    /// Read a text string.
    pub fn text(&mut self) -> Result<&'a str, CborError> {
        let (head, at) = self.located_head()?;
        match head {
            Head::Text(length) => {
                let start = self.at;
                let raw = self.take(usize::try_from(length).unwrap_or(usize::MAX))?;
                core::str::from_utf8(raw).map_err(|_| CborError::InvalidUtf8 { at: start })
            }
            other => Err(CborError::TypeMismatch {
                at,
                expected: "a text string",
                found: other.name(),
            }),
        }
    }

    /// Read an array head, returning how many items follow.
    ///
    /// The depth counter is incremented by [`CborReader::enter`] and decremented by
    /// [`CborReader::leave`], which the schema-driven decoder pairs around every nested item.
    pub fn array(&mut self) -> Result<u64, CborError> {
        let (head, at) = self.located_head()?;
        match head {
            Head::Array(length) => Ok(length),
            other => Err(CborError::TypeMismatch {
                at,
                expected: "an array",
                found: other.name(),
            }),
        }
    }

    /// Enter one level of nesting, refusing to go deeper than [`MAX_NESTING`].
    pub fn enter(&mut self) -> Result<(), CborError> {
        if self.depth == MAX_NESTING {
            return Err(CborError::TooDeep {
                at: self.at,
                limit: MAX_NESTING,
            });
        }
        self.depth += 1;
        Ok(())
    }

    /// Leave one level of nesting.
    pub fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// Skip one complete item of any admitted shape, returning the bytes it occupied.
    ///
    /// This is how a nested record whose schema this crate does not own is carried through: the
    /// bytes are lifted whole, and the caller that knows the vocabulary decodes them.
    pub fn skip_item(&mut self) -> Result<&'a [u8], CborError> {
        let start = self.at;
        self.skip_one()?;
        Ok(&self.bytes[start..self.at])
    }

    fn skip_one(&mut self) -> Result<(), CborError> {
        self.enter()?;
        let head = self.head()?;
        match head {
            Head::Unsigned(_) | Head::Bool(_) => {}
            Head::Bytes(length) | Head::Text(length) => {
                self.take(usize::try_from(length).unwrap_or(usize::MAX))?;
            }
            Head::Array(length) => {
                for _ in 0..length {
                    self.skip_one()?;
                }
            }
        }
        self.leave();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cbor::CborWriter;

    #[test]
    fn every_admitted_shape_round_trips() {
        let mut writer = CborWriter::new();
        writer.array(5);
        writer.unsigned(1_000_000);
        writer.bytes(&[1, 2, 3]);
        writer.text("mesh");
        writer.bool(true);
        writer.array(0);
        let bytes = writer.finish();

        let mut reader = CborReader::new(&bytes);
        assert_eq!(reader.array().unwrap(), 5);
        assert_eq!(reader.unsigned().unwrap(), 1_000_000);
        assert_eq!(reader.bytes().unwrap(), &[1, 2, 3]);
        assert_eq!(reader.text().unwrap(), "mesh");
        assert!(reader.bool().unwrap());
        assert_eq!(reader.array().unwrap(), 0);
        assert!(reader.is_exhausted());
    }

    #[test]
    fn a_wider_head_than_the_value_needs_is_refused() {
        // 5 written in a one-byte-argument head; `0x05` was available.
        let mut reader = CborReader::new(&[0x18, 0x05]);
        assert_eq!(
            reader.unsigned(),
            Err(CborError::NonCanonicalHead {
                at: 0,
                argument: 5,
                used: 1
            })
        );
        // 300 written in a four-byte head; a two-byte head was available.
        let mut reader = CborReader::new(&[0x1a, 0x00, 0x00, 0x01, 0x2c]);
        assert!(matches!(
            reader.unsigned(),
            Err(CborError::NonCanonicalHead { used: 4, .. })
        ));
    }

    #[test]
    fn shapes_outside_the_profile_are_refused() {
        assert!(matches!(
            CborReader::new(&[0x20]).unsigned(),
            Err(CborError::UnsupportedMajorType { major: 1, .. })
        ));
        assert!(matches!(
            CborReader::new(&[0xa1, 0x01, 0x01]).unsigned(),
            Err(CborError::UnsupportedMajorType { major: 5, .. })
        ));
        assert!(matches!(
            CborReader::new(&[0xc0]).unsigned(),
            Err(CborError::UnsupportedMajorType { major: 6, .. })
        ));
        assert!(matches!(
            CborReader::new(&[0xf6]).bool(),
            Err(CborError::UnsupportedSimpleValue { byte: 0xf6, .. })
        ));
        assert!(matches!(
            CborReader::new(&[0xfb, 0, 0, 0, 0, 0, 0, 0, 0]).unsigned(),
            Err(CborError::UnsupportedSimpleValue { byte: 0xfb, .. })
        ));
        assert!(matches!(
            CborReader::new(&[0x9f]).array(),
            Err(CborError::IndefiniteLength { at: 0 })
        ));
        assert!(matches!(
            CborReader::new(&[0x1c]).unsigned(),
            Err(CborError::Malformed { byte: 0x1c, .. })
        ));
    }

    #[test]
    fn a_truncated_item_is_refused() {
        assert!(matches!(
            CborReader::new(&[0x43, 0x01]).bytes(),
            Err(CborError::Truncated { .. })
        ));
    }

    #[test]
    fn invalid_utf8_is_refused() {
        assert!(matches!(
            CborReader::new(&[0x62, 0xff, 0xfe]).text(),
            Err(CborError::InvalidUtf8 { at: 1 })
        ));
    }

    /// A hostile peer buys one stack frame per byte without this limit; the profile's own writer
    /// never produces anything close to it.
    #[test]
    fn nesting_past_the_limit_is_refused_rather_than_overflowing_the_stack() {
        let deep = vec![0x81u8; MAX_NESTING + 8];
        assert!(matches!(
            CborReader::new(&deep).skip_item(),
            Err(CborError::TooDeep {
                limit: MAX_NESTING,
                ..
            })
        ));
        let shallow: Vec<u8> = {
            let mut bytes = vec![0x81u8; MAX_NESTING - 1];
            bytes.push(0x00);
            bytes
        };
        assert!(CborReader::new(&shallow).skip_item().is_ok());
    }

    #[test]
    fn skipping_lifts_the_exact_bytes_of_a_nested_item() {
        let bytes = [0x82, 0x82, 0x01, 0x02, 0x03];
        let mut reader = CborReader::new(&bytes);
        assert_eq!(reader.array().unwrap(), 2);
        assert_eq!(reader.skip_item().unwrap(), &[0x82, 0x01, 0x02]);
        assert_eq!(reader.remaining(), 1);
        assert_eq!(reader.skip_item().unwrap(), &[0x03]);
        assert!(reader.is_exhausted());
    }

    #[test]
    fn a_type_mismatch_names_both_shapes() {
        let mut reader = CborReader::new(&[0x64, b'm', b'e', b's', b'h']);
        assert_eq!(
            reader.unsigned(),
            Err(CborError::TypeMismatch {
                at: 0,
                expected: "an unsigned integer",
                found: "a text string"
            })
        );
    }
}
