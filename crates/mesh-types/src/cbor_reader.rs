//! The `mesh-cbor/0` reader: strict enough that exactly one byte string decodes to any value.
//!
//! # Why a reader exists in a crate that only needs to encode
//!
//! Determinism has two halves and only one of them is about writing. "Encoding a record twice
//! produces identical bytes" is a property of this implementation; "**every** implementation
//! produces those bytes" is a property of the *format*, and the way to establish it is to reject
//! every other encoding of the same value. A writer alone can be deterministic while the format it
//! writes still admits three other spellings of the same record — which is exactly how a second
//! implementation ends up verifying a signature over bytes it would never have produced.
//!
//! So this reader refuses:
//!
//! * an integer or length head longer than the value needs (RFC 8949 §4.2.1 preferred
//!   serialization) — `0x18 0x05` is rejected where `0x05` was available;
//! * an item nested deeper than [`MAX_NESTING`];
//! * an indefinite-length item, and the break code;
//! * a major type outside the profile: negative integers, maps, tags;
//! * any simple value other than `0xf4` and `0xf5` — no `null`, no `undefined`, no float;
//! * bytes left over after a complete item.
//!
//! Together with [`crate::CborWriter`] this closes the loop: `decode(encode(v)) == v` for every
//! value the profile can hold, and `encode(decode(b)) == b` for every byte string it accepts.

use core::fmt;

/// How deeply an item may nest before the reader refuses it.
///
/// Skipping an item of unknown shape is recursive, and array heads cost one byte each, so a
/// hostile peer can buy one stack frame per byte. Two hundred thousand `0x81` bytes — a
/// well-formed nesting of one-element arrays under every *other* rule this profile states —
/// aborts the process:
///
/// ```text
/// thread 'deeply_nested_arrays' has overflowed its stack
/// fatal runtime error: stack overflow, aborting
/// ```
///
/// That transcript is from running it, before this limit existed. A stack overflow in Rust is an
/// abort rather than a memory-safety failure, so this is availability and not corruption — but a
/// decoder on a network path that a remote peer can abort with two hundred kilobytes is not a
/// decoder, and an external implementer reading this file as the reference would copy the shape.
///
/// The bound is far above anything the schemas produce: the deepest record today nests three
/// levels, and a nested operation adds its own depth beneath that.
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
            Self::IndefiniteLength { at } => {
                write!(
                    formatter,
                    "byte {at}: indefinite lengths are not in the profile"
                )
            }
            Self::UnsupportedMajorType { at, major } => write!(
                formatter,
                "byte {at}: major type {major} is not in the profile"
            ),
            Self::UnsupportedSimpleValue { at, byte } => write!(
                formatter,
                "byte {at}: {byte:#04x} is a simple value or float, and the profile admits only \
                 false and true"
            ),
            Self::Malformed { at, byte } => {
                write!(
                    formatter,
                    "byte {at}: {byte:#04x} is not a well-formed head"
                )
            }
            Self::TypeMismatch {
                at,
                expected,
                found,
            } => write!(formatter, "byte {at}: expected {expected}, found {found}"),
            Self::InvalidUtf8 { at } => write!(formatter, "byte {at}: not valid UTF-8"),
            Self::TooDeep { at, limit } => write!(
                formatter,
                "byte {at}: nested deeper than the {limit}-level limit"
            ),
        }
    }
}

impl std::error::Error for CborError {}

/// What kind of item a head introduces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ItemKind {
    Unsigned,
    Bytes,
    Text,
    Array,
    Bool,
}

impl ItemKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Unsigned => "unsigned",
            Self::Bytes => "bytes",
            Self::Text => "text",
            Self::Array => "array",
            Self::Bool => "bool",
        }
    }
}

/// A cursor over `mesh-cbor/0` bytes.
#[derive(Clone, Debug)]
pub struct CborReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> CborReader<'a> {
    /// A reader positioned at the start of `bytes`.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    /// How many bytes have been consumed.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.at
    }

    /// Whether every byte has been consumed.
    #[must_use]
    pub const fn is_exhausted(&self) -> bool {
        self.at >= self.bytes.len()
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], CborError> {
        let start = self.at;
        let end = start.checked_add(count).ok_or(CborError::Truncated {
            at: start,
            needed: count,
        })?;
        if end > self.bytes.len() {
            return Err(CborError::Truncated {
                at: start,
                needed: end - self.bytes.len(),
            });
        }
        self.at = end;
        Ok(&self.bytes[start..end])
    }

    /// Read one head, rejecting every non-canonical spelling of it.
    fn head(&mut self) -> Result<(ItemKind, u64), CborError> {
        let at = self.at;
        let initial = self.take(1)?[0];
        let major = initial >> 5;
        let low = initial & 0x1f;

        if major == 7 {
            return match initial {
                0xf4 => Ok((ItemKind::Bool, 0)),
                0xf5 => Ok((ItemKind::Bool, 1)),
                0xff => Err(CborError::IndefiniteLength { at }),
                byte => Err(CborError::UnsupportedSimpleValue { at, byte }),
            };
        }

        let kind = match major {
            0 => ItemKind::Unsigned,
            2 => ItemKind::Bytes,
            3 => ItemKind::Text,
            4 => ItemKind::Array,
            other => return Err(CborError::UnsupportedMajorType { at, major: other }),
        };

        let (argument, used) = match low {
            0..=23 => (u64::from(low), 0u8),
            24 => (u64::from(self.take(1)?[0]), 1),
            25 => {
                let raw = self.take(2)?;
                (u64::from(u16::from_be_bytes([raw[0], raw[1]])), 2)
            }
            26 => {
                let raw = self.take(4)?;
                (
                    u64::from(u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]])),
                    4,
                )
            }
            27 => {
                let raw = self.take(8)?;
                let mut wide = [0u8; 8];
                wide.copy_from_slice(raw);
                (u64::from_be_bytes(wide), 8)
            }
            31 => return Err(CborError::IndefiniteLength { at }),
            _ => return Err(CborError::Malformed { at, byte: initial }),
        };

        if !head_is_shortest(argument, used) {
            return Err(CborError::NonCanonicalHead { at, argument, used });
        }
        Ok((kind, argument))
    }

    fn expect(&mut self, expected: ItemKind) -> Result<u64, CborError> {
        let at = self.at;
        let (found, argument) = self.head()?;
        if found == expected {
            Ok(argument)
        } else {
            Err(CborError::TypeMismatch {
                at,
                expected: expected.name(),
                found: found.name(),
            })
        }
    }

    /// Read an unsigned integer.
    ///
    /// # Errors
    ///
    /// [`CborError`] when the next item is not a canonically encoded unsigned integer.
    pub fn unsigned(&mut self) -> Result<u64, CborError> {
        self.expect(ItemKind::Unsigned)
    }

    /// Read a boolean.
    ///
    /// # Errors
    ///
    /// [`CborError`] when the next item is not `0xf4` or `0xf5`.
    pub fn bool(&mut self) -> Result<bool, CborError> {
        Ok(self.expect(ItemKind::Bool)? == 1)
    }

    /// Read a byte string.
    ///
    /// # Errors
    ///
    /// [`CborError`] when the next item is not a canonically encoded byte string, or it runs past
    /// the end of the input.
    pub fn bytes(&mut self) -> Result<&'a [u8], CborError> {
        let length = self.expect(ItemKind::Bytes)?;
        self.take(usize::try_from(length).unwrap_or(usize::MAX))
    }

    /// Read a text string.
    ///
    /// # Errors
    ///
    /// [`CborError`] when the next item is not a canonically encoded text string, or its bytes are
    /// not valid UTF-8.
    pub fn text(&mut self) -> Result<&'a str, CborError> {
        let at = self.at;
        let length = self.expect(ItemKind::Text)?;
        let raw = self.take(usize::try_from(length).unwrap_or(usize::MAX))?;
        core::str::from_utf8(raw).map_err(|_| CborError::InvalidUtf8 { at })
    }

    /// Read an array head and return how many items follow.
    ///
    /// # Errors
    ///
    /// [`CborError`] when the next item is not a canonically encoded definite-length array.
    pub fn array(&mut self) -> Result<u64, CborError> {
        self.expect(ItemKind::Array)
    }

    /// Consume one complete item, whatever it is, and return exactly its bytes.
    ///
    /// How a nested record is lifted out without knowing its schema.
    ///
    /// # Errors
    ///
    /// [`CborError`] when the item is not well formed under the profile.
    pub fn skip_item(&mut self) -> Result<&'a [u8], CborError> {
        let start = self.at;
        self.skip_one(0)?;
        Ok(&self.bytes[start..self.at])
    }

    /// Skip one item at nesting `depth`, refusing to go past [`MAX_NESTING`].
    fn skip_one(&mut self, depth: usize) -> Result<(), CborError> {
        let at = self.at;
        if depth >= MAX_NESTING {
            return Err(CborError::TooDeep {
                at,
                limit: MAX_NESTING,
            });
        }
        let (kind, argument) = self.head()?;
        match kind {
            ItemKind::Unsigned | ItemKind::Bool => Ok(()),
            ItemKind::Bytes | ItemKind::Text => {
                self.take(usize::try_from(argument).unwrap_or(usize::MAX))?;
                Ok(())
            }
            ItemKind::Array => {
                for _ in 0..argument {
                    self.skip_one(depth + 1)?;
                }
                Ok(())
            }
        }
    }
}

/// Whether `argument` written in `used` additional bytes is the shortest head that holds it.
const fn head_is_shortest(argument: u64, used: u8) -> bool {
    let needed = if argument < 24 {
        0
    } else if argument <= u8::MAX as u64 {
        1
    } else if argument <= u16::MAX as u64 {
        2
    } else if argument <= u32::MAX as u64 {
        4
    } else {
        8
    };
    used == needed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cbor::CborWriter;

    #[test]
    fn every_head_width_round_trips() {
        for value in [
            0u64,
            1,
            23,
            24,
            255,
            256,
            65535,
            65536,
            u32::MAX as u64,
            u64::MAX,
        ] {
            let mut writer = CborWriter::new();
            writer.unsigned(value);
            let encoded = writer.finish();
            let mut reader = CborReader::new(&encoded);
            assert_eq!(reader.unsigned(), Ok(value));
            assert!(reader.is_exhausted());
        }
    }

    #[test]
    fn strings_arrays_and_booleans_round_trip() {
        let mut writer = CborWriter::new();
        writer.array(4);
        writer.bytes(&[1, 2, 3]);
        writer.text("caf\u{e9}");
        writer.bool(true);
        writer.bool(false);
        let encoded = writer.finish();

        let mut reader = CborReader::new(&encoded);
        assert_eq!(reader.array(), Ok(4));
        assert_eq!(reader.bytes(), Ok(&[1u8, 2, 3][..]));
        assert_eq!(reader.text(), Ok("caf\u{e9}"));
        assert_eq!(reader.bool(), Ok(true));
        assert_eq!(reader.bool(), Ok(false));
        assert!(reader.is_exhausted());
        assert_eq!(reader.position(), encoded.len());
    }

    /// The rule that makes the encoding one-to-one. Every one of these is a *valid* CBOR encoding
    /// of a value the profile can hold, spelled a way this profile does not permit.
    #[test]
    fn a_wider_head_than_necessary_is_rejected() {
        let cases: [(&[u8], u64, u8); 4] = [
            (&[0x18, 0x05], 5, 1),
            (&[0x19, 0x00, 0x18], 24, 2),
            (&[0x1a, 0x00, 0x00, 0x01, 0x00], 256, 4),
            (
                &[0x1b, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00],
                65536,
                8,
            ),
        ];
        for (bytes, argument, used) in cases {
            let mut reader = CborReader::new(bytes);
            assert_eq!(
                reader.unsigned(),
                Err(CborError::NonCanonicalHead {
                    at: 0,
                    argument,
                    used
                }),
                "{bytes:?}"
            );
        }
    }

    #[test]
    fn the_shortest_head_at_every_boundary_is_accepted() {
        for (argument, used) in [
            (0u64, 0u8),
            (23, 0),
            (24, 1),
            (255, 1),
            (256, 2),
            (65535, 2),
            (65536, 4),
            (u32::MAX as u64, 4),
            (u32::MAX as u64 + 1, 8),
        ] {
            assert!(head_is_shortest(argument, used), "{argument} in {used}");
            assert!(!head_is_shortest(argument, used + 1), "{argument} widened");
        }
    }

    #[test]
    fn an_indefinite_length_item_is_rejected() {
        // 0xbf is an indefinite-length *map*, and a map is refused for its major type before its
        // length is looked at; it is checked in the major-type test instead.
        for byte in [0x5fu8, 0x7f, 0x9f, 0xff] {
            let bytes = [byte];
            let mut reader = CborReader::new(&bytes);
            assert_eq!(
                reader.skip_item(),
                Err(CborError::IndefiniteLength { at: 0 }),
                "{byte:#04x}"
            );
        }
    }

    #[test]
    fn a_major_type_outside_the_profile_is_rejected() {
        // 0x20 negative integer, 0xa0 map, 0xbf indefinite-length map, 0xc0 tag.
        for (byte, major) in [(0x20u8, 1u8), (0xa0, 5), (0xbf, 5), (0xc0, 6)] {
            let bytes = [byte];
            let mut reader = CborReader::new(&bytes);
            assert_eq!(
                reader.skip_item(),
                Err(CborError::UnsupportedMajorType { at: 0, major }),
                "{byte:#04x}"
            );
        }
    }

    #[test]
    fn a_simple_value_or_float_is_rejected() {
        // 0xf6 null, 0xf7 undefined, 0xf9 half float, 0xfa single, 0xfb double, 0xf0 simple 16.
        for byte in [0xf6u8, 0xf7, 0xf9, 0xfa, 0xfb, 0xf0] {
            let bytes = [byte];
            let mut reader = CborReader::new(&bytes);
            assert_eq!(
                reader.skip_item(),
                Err(CborError::UnsupportedSimpleValue { at: 0, byte }),
                "{byte:#04x}"
            );
        }
    }

    #[test]
    fn a_reserved_additional_information_value_is_rejected() {
        for byte in [0x1cu8, 0x1d, 0x1e] {
            let bytes = [byte];
            let mut reader = CborReader::new(&bytes);
            assert_eq!(
                reader.skip_item(),
                Err(CborError::Malformed { at: 0, byte }),
                "{byte:#04x}"
            );
        }
    }

    #[test]
    fn a_truncated_item_is_rejected() {
        let mut reader = CborReader::new(&[0x43, 0x01]);
        assert_eq!(
            reader.bytes(),
            Err(CborError::Truncated { at: 1, needed: 2 })
        );
        let mut reader = CborReader::new(&[]);
        assert_eq!(
            reader.unsigned(),
            Err(CborError::Truncated { at: 0, needed: 1 })
        );
    }

    #[test]
    fn a_type_mismatch_names_both_sides() {
        let mut reader = CborReader::new(&[0x00]);
        assert_eq!(
            reader.text(),
            Err(CborError::TypeMismatch {
                at: 0,
                expected: "text",
                found: "unsigned"
            })
        );
    }

    #[test]
    fn invalid_utf8_in_a_text_string_is_rejected() {
        // A text head of length 1 over a continuation byte.
        let mut reader = CborReader::new(&[0x61, 0x80]);
        assert_eq!(reader.text(), Err(CborError::InvalidUtf8 { at: 0 }));
    }

    #[test]
    fn skipping_an_item_returns_exactly_its_bytes() {
        let mut writer = CborWriter::new();
        writer.array(2);
        writer.array(2);
        writer.unsigned(1);
        writer.text("x");
        writer.unsigned(9);
        let encoded = writer.finish();

        let mut reader = CborReader::new(&encoded);
        assert_eq!(reader.array(), Ok(2));
        assert_eq!(reader.skip_item(), Ok(&[0x82, 0x01, 0x61, b'x'][..]));
        assert_eq!(reader.unsigned(), Ok(9));
        assert!(reader.is_exhausted());
    }

    /// Found by cold verification, by running it rather than by reasoning about it: before the
    /// limit existed this input aborted the process with a stack overflow.
    #[test]
    fn a_deeply_nested_item_is_refused_rather_than_overflowing_the_stack() {
        let mut bytes = vec![0x81u8; 200_000];
        bytes.push(0x00);
        let mut reader = CborReader::new(&bytes);
        assert_eq!(
            reader.skip_item(),
            Err(CborError::TooDeep {
                at: MAX_NESTING,
                limit: MAX_NESTING,
            })
        );
    }

    /// The limit must not refuse anything the schemas can produce, or it is a bug rather than a
    /// bound. One below it is accepted; one above it is not.
    #[test]
    fn the_nesting_limit_accepts_everything_below_it() {
        let mut bytes = vec![0x81u8; MAX_NESTING - 1];
        bytes.push(0x00);
        assert_eq!(
            CborReader::new(&bytes).skip_item().map(<[u8]>::len),
            Ok(MAX_NESTING)
        );

        let mut too_deep = vec![0x81u8; MAX_NESTING];
        too_deep.push(0x00);
        assert!(CborReader::new(&too_deep).skip_item().is_err());
    }

    #[test]
    fn every_error_renders_a_message() {
        let errors = [
            CborError::Truncated { at: 1, needed: 2 },
            CborError::NonCanonicalHead {
                at: 0,
                argument: 5,
                used: 1,
            },
            CborError::IndefiniteLength { at: 3 },
            CborError::UnsupportedMajorType { at: 0, major: 5 },
            CborError::UnsupportedSimpleValue { at: 0, byte: 0xf6 },
            CborError::Malformed { at: 0, byte: 0x1c },
            CborError::TypeMismatch {
                at: 0,
                expected: "text",
                found: "unsigned",
            },
            CborError::InvalidUtf8 { at: 2 },
            CborError::TooDeep { at: 9, limit: 64 },
        ];
        for error in &errors {
            assert!(!error.to_string().is_empty(), "{error:?}");
        }
        assert_eq!(
            errors[1].to_string(),
            "byte 0: 5 was written in a 1-byte head, which is wider than it needs"
        );
    }
}
