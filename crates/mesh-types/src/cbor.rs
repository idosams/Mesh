//! The `mesh-cbor/0` writer: a deliberately small, deliberately closed subset of CBOR.
//!
//! # Why a subset rather than CBOR
//!
//! RFC 8949 is an interchange format with choices in it. A signature is over bytes, so every
//! choice an encoder is allowed to make is a way for two implementations to disagree about what
//! was signed. RFC 8949 §4.2 narrows those choices ("deterministic encoding") but does not remove
//! them: it still admits maps whose key ordering is a rule to be applied, floating point, tags,
//! negative integers and indefinite-length items, and it leaves the *schema* — which field is
//! written where — entirely to the application.
//!
//! `mesh-cbor/0` removes the choices instead of narrowing them. It admits exactly five things:
//!
//! | Admitted | CBOR major type | Rule |
//! |---|---|---|
//! | unsigned integer | 0 | shortest head that fits the value |
//! | byte string | 2 | definite length, shortest length head |
//! | text string | 3 | definite length, shortest length head, valid UTF-8 |
//! | array | 4 | definite length, shortest length head |
//! | boolean | 7 | `0xf4` for false, `0xf5` for true, and no other simple value |
//!
//! Everything else is **not representable by this writer**: there is no method that emits a
//! negative integer, a map, a tag, a float, `null`, `undefined`, or an indefinite-length item, so
//! no caller can emit one by accident and no reviewer has to check that none did. Notably there
//! are **no maps**, which is what makes map-key ordering — the single most common source of
//! canonical-CBOR disagreement — not a rule this profile has to state and enforce, but a shape it
//! cannot express. A record's fields are an array in schema order; a keyed collection is an array
//! of key/value groups sorted by key at the type level, which is where the sort is checkable.
//!
//! # What a decoder needs
//!
//! Every item is self-delimiting and definite-length, so a decoder that knows the schema needs no
//! lookahead. [`crate::CanonicalType`] publishes that schema, and `protocol/test-vectors/` pins the
//! bytes.

/// The name of the encoding profile this writer implements, as published.
///
/// Versioned: a change to any rule above is a new profile, never an edit to this one, because
/// deployed signatures were made over bytes this one produced.
pub const CBOR_PROFILE: &str = "mesh-cbor/0";

/// CBOR major type 0: an unsigned integer.
const MAJOR_UNSIGNED: u8 = 0;
/// CBOR major type 2: a byte string.
const MAJOR_BYTES: u8 = 2;
/// CBOR major type 3: a UTF-8 text string.
const MAJOR_TEXT: u8 = 3;
/// CBOR major type 4: an array.
const MAJOR_ARRAY: u8 = 4;

/// The one-byte encoding of `false` (major type 7, simple value 20).
const FALSE_BYTE: u8 = 0xf4;
/// The one-byte encoding of `true` (major type 7, simple value 21).
const TRUE_BYTE: u8 = 0xf5;

/// A writer that can only produce `mesh-cbor/0`.
///
/// It appends to a byte buffer and never fails: every method takes a value the profile can
/// represent, and the profile cannot represent anything else.
///
/// ```
/// use mesh_types::CborWriter;
///
/// let mut writer = CborWriter::new();
/// writer.array(2);
/// writer.text("mesh");
/// writer.unsigned(0);
/// assert_eq!(writer.finish(), vec![0x82, 0x64, b'm', b'e', b's', b'h', 0x00]);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CborWriter {
    buffer: Vec<u8>,
}

impl CborWriter {
    /// An empty writer.
    #[must_use]
    pub const fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// An empty writer with room for `capacity` bytes.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
        }
    }

    /// The item head: the major type in the top three bits, then the argument in the shortest
    /// form that holds it.
    ///
    /// This is RFC 8949 §4.2.1's "preferred serialization" requirement, and it is the only place
    /// in the profile where an encoder could have made a choice. Making it here, once, is why no
    /// other method in this type can get it wrong.
    fn head(&mut self, major: u8, argument: u64) {
        let tag = major << 5;
        if argument < 24 {
            // The argument rides in the head byte itself.
            self.buffer.push(tag | (argument as u8));
        } else if argument <= u64::from(u8::MAX) {
            self.buffer.push(tag | 24);
            self.buffer.push(argument as u8);
        } else if argument <= u64::from(u16::MAX) {
            self.buffer.push(tag | 25);
            self.buffer
                .extend_from_slice(&(argument as u16).to_be_bytes());
        } else if argument <= u64::from(u32::MAX) {
            self.buffer.push(tag | 26);
            self.buffer
                .extend_from_slice(&(argument as u32).to_be_bytes());
        } else {
            self.buffer.push(tag | 27);
            self.buffer.extend_from_slice(&argument.to_be_bytes());
        }
    }

    /// An unsigned integer.
    pub fn unsigned(&mut self, value: u64) -> &mut Self {
        self.head(MAJOR_UNSIGNED, value);
        self
    }

    /// A byte string.
    pub fn bytes(&mut self, value: &[u8]) -> &mut Self {
        self.head(MAJOR_BYTES, value.len() as u64);
        self.buffer.extend_from_slice(value);
        self
    }

    /// A text string. Rust `str` is UTF-8 by construction, so the profile's validity rule is the
    /// type system's rather than a check.
    pub fn text(&mut self, value: &str) -> &mut Self {
        self.head(MAJOR_TEXT, value.len() as u64);
        self.buffer.extend_from_slice(value.as_bytes());
        self
    }

    /// A boolean.
    pub fn bool(&mut self, value: bool) -> &mut Self {
        self.buffer.push(if value { TRUE_BYTE } else { FALSE_BYTE });
        self
    }

    /// The head of a definite-length array holding `len` items. The items follow, written by the
    /// caller, and the schema is what says how many there are.
    pub fn array(&mut self, len: u64) -> &mut Self {
        self.head(MAJOR_ARRAY, len);
        self
    }

    /// Append bytes that another `mesh-cbor/0` writer already produced.
    ///
    /// The only way to nest a complete encoding, and it takes a whole encoding rather than a
    /// fragment: a partial item spliced in here would produce a stream no decoder could read.
    pub fn nested(&mut self, encoding: &[u8]) -> &mut Self {
        self.buffer.extend_from_slice(encoding);
        self
    }

    /// How many bytes have been written.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Whether nothing has been written.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// The bytes written so far.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.buffer
    }

    /// Take the bytes.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded(build: impl FnOnce(&mut CborWriter)) -> Vec<u8> {
        let mut writer = CborWriter::new();
        build(&mut writer);
        writer.finish()
    }

    /// The head boundaries, taken from RFC 8949 §3 and Appendix A. Every one of the five head
    /// widths, and both sides of every boundary between them, because an off-by-one here changes
    /// every encoding that crosses it.
    #[test]
    fn an_unsigned_integer_uses_the_shortest_head() {
        let cases: [(u64, &[u8]); 13] = [
            (0, &[0x00]),
            (1, &[0x01]),
            (23, &[0x17]),
            (24, &[0x18, 0x18]),
            (25, &[0x18, 0x19]),
            (255, &[0x18, 0xff]),
            (256, &[0x19, 0x01, 0x00]),
            (1000, &[0x19, 0x03, 0xe8]),
            (65535, &[0x19, 0xff, 0xff]),
            (65536, &[0x1a, 0x00, 0x01, 0x00, 0x00]),
            (1_000_000, &[0x1a, 0x00, 0x0f, 0x42, 0x40]),
            (4_294_967_295, &[0x1a, 0xff, 0xff, 0xff, 0xff]),
            (
                4_294_967_296,
                &[0x1b, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00],
            ),
        ];
        for (value, expected) in cases {
            assert_eq!(
                encoded(|writer| {
                    writer.unsigned(value);
                }),
                expected,
                "unsigned {value}"
            );
        }
    }

    #[test]
    fn the_largest_unsigned_integer_uses_the_eight_byte_head() {
        assert_eq!(
            encoded(|writer| {
                writer.unsigned(u64::MAX);
            }),
            vec![0x1b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
        );
    }

    /// RFC 8949 Appendix A's byte- and text-string examples.
    #[test]
    fn strings_carry_a_definite_length_head() {
        assert_eq!(
            encoded(|w| {
                w.bytes(&[]);
            }),
            vec![0x40]
        );
        assert_eq!(
            encoded(|w| {
                w.bytes(&[0x01, 0x02, 0x03, 0x04]);
            }),
            vec![0x44, 0x01, 0x02, 0x03, 0x04]
        );
        assert_eq!(
            encoded(|w| {
                w.text("");
            }),
            vec![0x60]
        );
        assert_eq!(
            encoded(|w| {
                w.text("a");
            }),
            vec![0x61, 0x61]
        );
        assert_eq!(
            encoded(|w| {
                w.text("IETF");
            }),
            vec![0x64, 0x49, 0x45, 0x54, 0x46]
        );
        // Multi-byte UTF-8: the length is the byte length, never the character count.
        assert_eq!(
            encoded(|w| {
                w.text("\u{00fc}");
            }),
            vec![0x62, 0xc3, 0xbc]
        );
        assert_eq!(
            encoded(|w| {
                w.text("\u{6c34}");
            }),
            vec![0x63, 0xe6, 0xb0, 0xb4]
        );
    }

    #[test]
    fn a_long_string_crosses_into_the_wider_head() {
        let long = vec![0u8; 24];
        assert_eq!(
            encoded(|w| {
                w.bytes(&long);
            })[..2],
            [0x58, 0x18]
        );
        let longer = vec![0u8; 256];
        assert_eq!(
            encoded(|w| {
                w.bytes(&longer);
            })[..3],
            [0x59, 0x01, 0x00]
        );
    }

    #[test]
    fn arrays_carry_a_definite_length_head() {
        assert_eq!(
            encoded(|w| {
                w.array(0);
            }),
            vec![0x80]
        );
        assert_eq!(
            encoded(|w| {
                w.array(3);
                w.unsigned(1);
                w.unsigned(2);
                w.unsigned(3);
            }),
            vec![0x83, 0x01, 0x02, 0x03]
        );
        assert_eq!(
            encoded(|w| {
                w.array(25);
            })[..2],
            [0x98, 0x19]
        );
    }

    #[test]
    fn booleans_are_the_only_simple_values() {
        assert_eq!(
            encoded(|w| {
                w.bool(false);
            }),
            vec![0xf4]
        );
        assert_eq!(
            encoded(|w| {
                w.bool(true);
            }),
            vec![0xf5]
        );
    }

    /// A nested encoding is spliced whole, so an outer array of two records is the concatenation
    /// of the head and the two complete encodings.
    #[test]
    fn a_nested_encoding_is_spliced_whole() {
        let inner = encoded(|w| {
            w.array(1);
            w.unsigned(7);
        });
        assert_eq!(
            encoded(|w| {
                w.array(2);
                w.nested(&inner);
                w.nested(&inner);
            }),
            vec![0x82, 0x81, 0x07, 0x81, 0x07]
        );
    }

    #[test]
    fn the_writer_reports_its_own_length() {
        let mut writer = CborWriter::with_capacity(8);
        assert!(writer.is_empty());
        writer.unsigned(1000);
        assert_eq!(writer.len(), 3);
        assert!(!writer.is_empty());
        assert_eq!(writer.as_bytes(), &[0x19, 0x03, 0xe8]);
    }
}
