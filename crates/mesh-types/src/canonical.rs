//! The canonical encoding: the bytes a signed record *is*, and the schema that describes them.
//!
//! # The shape, in one sentence
//!
//! A record encodes as a definite-length [`mesh-cbor/0`](crate::CBOR_PROFILE) array whose first
//! element is the record's [`DomainTag`] and whose remaining elements are its fields, in schema
//! order, one array element per field.
//!
//! ```text
//! [ "mesh.v0.file-manifest", 6, h'ab…ab', [ [ h'cd…cd', 0, 6 ] ] ]
//!   ^ domain tag             ^ byte_length ^ content_hash ^ chunks
//! ```
//!
//! # Why an array and not a map
//!
//! A map would carry field names, which reads better and costs the one property this encoding
//! exists for. Two encoders emitting the same map must agree on key ordering, on whether a key is
//! text or an integer, and on what to do with a key whose value is absent — three ways to produce
//! different bytes for the same record, each of which has to be specified, implemented and tested
//! by every implementation. An array has none of them: the order *is* the schema, an absent field
//! is not expressible, and there is nothing to sort.
//!
//! The cost is that the bytes are not self-describing, and it is paid in the open:
//! [`RecordSchema`] is published to `protocol/schemas/`, so the names an external implementer
//! needs are a file they read rather than a comment they cannot.
//!
//! # No optional fields, at all
//!
//! The profile has no `null` and no absence marker, so "no optional-field ambiguity" is not a rule
//! this encoding enforces — it is a shape it cannot express. A field that may be absent is modelled
//! as a [`CanonicalType::Sequence`] of at most one element: `[]` for absent, `[v]` for present.
//! That is an ordinary array, decoded by the ordinary rule, and it removes the only remaining
//! question an encoder could answer two ways.
//!
//! # This IS how a record ID is derived, and the code has not caught up yet
//!
//! `docs/adr/0033-name-an-immutable-record-by-the-digest-of-its-canonical-encoding.md` ruled that a
//! record's name is [`canonical_digest`] — BLAKE3 of the bytes this module produces — and retired
//! the [`DigestWriter`](crate::DigestWriter) identity framing that
//! [`derive_id`](crate::derive_id) still uses. `docs/protocol.md` §2.1 now states one rule: a record
//! has a schema and is named by the digest of its canonical encoding, while a byte sequence with no
//! schema, such as a chunk, is named by the digest of those bytes directly.
//!
//! Until `01KZFMZC4MTHTT3BW4Y0BW6NYA` lands, [`derive_id`](crate::derive_id) still produces the
//! retired framing's value, so it is **not** yet [`canonical_digest`]. That gap is the register
//! leading the implementation on purpose: plan §14.3 rule 3 forbids one unsupervised run from
//! moving a signed record's definition and its implementation together. It is no longer a
//! contradiction — §3.10 names each item that lags and the change that closes it.
//!
//! The earlier decision to keep the two apart, and the four alternatives weighed then, are in
//! `docs/adr/0007-encode-signed-records-as-fixed-order-cbor-arrays.md`; ADR-0033 is the record of
//! its first alternative being taken.

use crate::cbor::CborWriter;
use crate::cbor_reader::{CborError, CborReader};
use crate::digest::{ContentDigest, Digest32, DomainTag};

/// The version of the record-schema vocabulary published to `protocol/schemas/`.
pub const SCHEMA_FORMAT: &str = "mesh-record-schema/0";

/// What one canonical field holds, in enough detail to decode it without reading Rust.
///
/// Every variant maps to exactly one [`mesh-cbor/0`](crate::CBOR_PROFILE) shape. There is no
/// variant for a floating-point number, a negative integer, a map or an absent value, because the
/// profile has no encoding for any of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalType {
    /// An unsigned integer.
    Unsigned,
    /// A boolean.
    Bool,
    /// A byte string. `Some(n)` when the schema fixes its length at `n` bytes.
    Bytes(Option<u32>),
    /// A UTF-8 text string.
    Text,
    /// A homogeneous array. Also how an optional field is modelled: length zero or one.
    Sequence(&'static CanonicalType),
    /// A fixed-arity array of named fields, in order. A record's own body is not a group — it is
    /// the outer array — but a repeated compound element inside a sequence is.
    Group(&'static [FieldSchema]),
    /// A complete nested canonical encoding, carrying its own domain tag as its first element.
    ///
    /// Used where the element type is supplied by another crate — the operations a `ChangeSet`
    /// carries — so the outer schema does not have to name a vocabulary it does not own.
    Record,
}

impl CanonicalType {
    /// The published name of this type's shape.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Unsigned => "unsigned",
            Self::Bool => "bool",
            Self::Bytes(_) => "bytes",
            Self::Text => "text",
            Self::Sequence(_) => "sequence",
            Self::Group(_) => "group",
            Self::Record => "record",
        }
    }
}

/// One field of a record or a group: its published name and its type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldSchema {
    /// The published field name. Descriptive only — the bytes carry position, never this.
    pub name: &'static str,
    /// What the field holds.
    pub ty: CanonicalType,
}

impl FieldSchema {
    /// Declare a field.
    #[must_use]
    pub const fn new(name: &'static str, ty: CanonicalType) -> Self {
        Self { name, ty }
    }
}

/// The published schema of one signed record type: its domain tag and its field order.
///
/// **The field order is the wire format.** Reordering this list changes every encoding of every
/// record of this type, which is why the vectors in `protocol/test-vectors/` are compared
/// byte-for-byte and why a change to one without the other fails the build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordSchema {
    /// The domain tag, encoded as the first element of every record of this type.
    pub domain: DomainTag,
    /// The fields, in encoding order.
    pub fields: &'static [FieldSchema],
}

impl RecordSchema {
    /// Declare a record schema.
    #[must_use]
    pub const fn new(domain: DomainTag, fields: &'static [FieldSchema]) -> Self {
        Self { domain, fields }
    }
}

/// A value in the canonical profile, ready to encode.
///
/// Built by [`CanonicalEncode::canonical_fields`] in schema order. Holding the fields as data
/// rather than writing them straight into a hasher is what makes "a reordered field changes the
/// bytes" a property a test can exercise directly — it permutes this list and encodes again — and
/// what lets the schema be checked against the values over a whole corpus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalValue {
    /// An unsigned integer.
    Unsigned(u64),
    /// A boolean.
    Bool(bool),
    /// A byte string.
    Bytes(Vec<u8>),
    /// A UTF-8 text string.
    Text(String),
    /// A homogeneous array, or a zero-or-one-element array standing for an optional field.
    Sequence(Vec<CanonicalValue>),
    /// A fixed-arity array of fields, in schema order.
    Group(Vec<CanonicalValue>),
    /// A complete nested canonical encoding, produced by [`encode_canonical`].
    Record(Vec<u8>),
}

impl CanonicalValue {
    /// A byte-string field from a fixed-width array.
    #[must_use]
    pub fn from_array<const N: usize>(bytes: [u8; N]) -> Self {
        Self::Bytes(bytes.to_vec())
    }

    /// A byte-string field from a digest.
    #[must_use]
    pub fn from_digest(digest: &Digest32) -> Self {
        Self::Bytes(digest.as_bytes().to_vec())
    }

    /// Write this value into `writer`.
    fn write(&self, writer: &mut CborWriter) {
        match self {
            Self::Unsigned(value) => {
                writer.unsigned(*value);
            }
            Self::Bool(value) => {
                writer.bool(*value);
            }
            Self::Bytes(value) => {
                writer.bytes(value);
            }
            Self::Text(value) => {
                writer.text(value);
            }
            Self::Sequence(items) | Self::Group(items) => {
                writer.array(items.len() as u64);
                for item in items {
                    item.write(writer);
                }
            }
            Self::Record(encoding) => {
                writer.nested(encoding);
            }
        }
    }
}

/// A record with a canonical encoding: a published schema, and its fields in that order.
///
/// Implementing this is what makes a record signable. The encoding is a pure function of the
/// values [`CanonicalEncode::canonical_fields`] returns, so an implementation cannot introduce a
/// platform-dependent or run-dependent byte: there is nowhere to put one.
pub trait CanonicalEncode {
    /// This record type's published schema.
    const SCHEMA: RecordSchema;

    /// The field values, in `SCHEMA` order and of the types `SCHEMA` declares.
    ///
    /// Agreement with `SCHEMA` is checked over a generated corpus by
    /// `tests/canonical_encoding.rs` rather than asserted here, so the encoding path stays
    /// allocation-for-allocation the same in a test and in production.
    fn canonical_fields(&self) -> Vec<CanonicalValue>;
}

/// The canonical encoding of `record`: the bytes a signature covers.
///
/// ```
/// use mesh_types::{encode_canonical, ChunkRef, Digest32, FileManifest};
///
/// let manifest = FileManifest::new(0, Digest32::from_bytes([0; 32]), Vec::new());
/// let bytes = encode_canonical(&manifest);
///
/// // The same record, encoded twice, in any process on any platform: the same bytes.
/// assert_eq!(bytes, encode_canonical(&manifest));
/// // The domain tag leads, so the first element identifies the record type.
/// assert_eq!(bytes[0], 0x84); // a four-element array: tag + three fields
/// # let _ = ChunkRef::new(Digest32::from_bytes([0; 32]), 0, 0);
/// ```
#[must_use]
pub fn encode_canonical<R: CanonicalEncode + ?Sized>(record: &R) -> Vec<u8> {
    let fields = record.canonical_fields();
    let mut writer = CborWriter::with_capacity(64);
    writer.array((fields.len() + 1) as u64);
    writer.text(R::SCHEMA.domain.as_str());
    for field in &fields {
        field.write(&mut writer);
    }
    writer.finish()
}

/// The digest of a record's canonical encoding under `D`.
///
/// This is the digest the published vectors carry as `canonical_encoding_digest_hex`, and the one
/// a signature over "the record's bytes" would be made over. It is **not**
/// [`derive_id`](crate::derive_id) — see this module's header for why the two differ and where the
/// contradiction is filed.
#[must_use]
pub fn canonical_digest<D: ContentDigest, R: CanonicalEncode + ?Sized>(record: &R) -> Digest32 {
    D::digest_bytes(&encode_canonical(record))
}

/// Why some bytes are not the canonical encoding of a record under a given schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The bytes are not a well-formed [`mesh-cbor/0`](crate::CBOR_PROFILE) item.
    Cbor(CborError),
    /// The leading domain tag was some other record type's.
    WrongDomain {
        /// The tag the schema declares.
        expected: &'static str,
        /// The tag the bytes carried.
        found: String,
    },
    /// The outer array held a different number of elements than the schema declares.
    WrongArity {
        /// How many the schema declares, counting the domain tag.
        expected: usize,
        /// How many the bytes carried.
        found: u64,
    },
    /// A fixed-width byte field carried the wrong number of bytes.
    WrongWidth {
        /// The path to the field.
        path: String,
        /// The width the schema fixes.
        expected: u32,
        /// The width the bytes carried.
        found: usize,
    },
    /// Bytes were left over after a complete record. Under a self-delimiting encoding this is
    /// always a framing error, never padding.
    TrailingBytes {
        /// Where the record ended.
        at: usize,
        /// How many bytes followed it.
        extra: usize,
    },
}

impl From<CborError> for DecodeError {
    fn from(error: CborError) -> Self {
        Self::Cbor(error)
    }
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Cbor(inner) => write!(formatter, "{inner}"),
            Self::WrongDomain { expected, found } => write!(
                formatter,
                "expected the domain tag {expected}, found {found}"
            ),
            Self::WrongArity { expected, found } => write!(
                formatter,
                "the schema declares {expected} array elements, the bytes carried {found}"
            ),
            Self::WrongWidth {
                path,
                expected,
                found,
            } => write!(
                formatter,
                "{path}: expected {expected} bytes, found {found}"
            ),
            Self::TrailingBytes { at, extra } => write!(
                formatter,
                "the record ended at byte {at} and {extra} bytes followed it"
            ),
        }
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cbor(inner) => Some(inner),
            _ => None,
        }
    }
}

/// Decode `bytes` as `R`'s field values.
///
/// The inverse of [`encode_canonical`]. It exists so that determinism can be stated as a property
/// of the *format* rather than of this writer: the reader beneath it rejects every non-canonical
/// spelling, so `decode_canonical` accepts exactly the byte string `encode_canonical` produces and
/// no other. `tests/canonical_encoding.rs` walks both directions over a generated corpus.
///
/// # Errors
///
/// [`DecodeError`] when the bytes are not well formed, carry another record type's domain tag,
/// disagree with the schema, or are followed by anything at all.
pub fn decode_canonical<R: CanonicalEncode + ?Sized>(
    bytes: &[u8],
) -> Result<Vec<CanonicalValue>, DecodeError> {
    let mut reader = CborReader::new(bytes);
    let expected = R::SCHEMA.fields.len() + 1;
    let arity = reader.array()?;
    if arity != expected as u64 {
        return Err(DecodeError::WrongArity {
            expected,
            found: arity,
        });
    }
    let domain = reader.text()?;
    if domain != R::SCHEMA.domain.as_str() {
        return Err(DecodeError::WrongDomain {
            expected: R::SCHEMA.domain.as_str(),
            found: domain.to_owned(),
        });
    }

    let mut values = Vec::with_capacity(R::SCHEMA.fields.len());
    for field in R::SCHEMA.fields {
        values.push(decode_value(
            &format!("{}.{}", R::SCHEMA.domain.as_str(), field.name),
            field.ty,
            &mut reader,
        )?);
    }
    if !reader.is_exhausted() {
        return Err(DecodeError::TrailingBytes {
            at: reader.position(),
            extra: bytes.len() - reader.position(),
        });
    }
    Ok(values)
}

/// Decode one value of the declared type.
fn decode_value(
    path: &str,
    ty: CanonicalType,
    reader: &mut CborReader<'_>,
) -> Result<CanonicalValue, DecodeError> {
    match ty {
        CanonicalType::Unsigned => Ok(CanonicalValue::Unsigned(reader.unsigned()?)),
        CanonicalType::Bool => Ok(CanonicalValue::Bool(reader.bool()?)),
        CanonicalType::Text => Ok(CanonicalValue::Text(reader.text()?.to_owned())),
        CanonicalType::Bytes(width) => {
            let bytes = reader.bytes()?;
            if let Some(width) = width {
                if bytes.len() != width as usize {
                    return Err(DecodeError::WrongWidth {
                        path: path.to_owned(),
                        expected: width,
                        found: bytes.len(),
                    });
                }
            }
            Ok(CanonicalValue::Bytes(bytes.to_vec()))
        }
        CanonicalType::Sequence(element) => {
            let count = reader.array()?;
            let mut items = Vec::new();
            for index in 0..count {
                items.push(decode_value(&format!("{path}[{index}]"), *element, reader)?);
            }
            Ok(CanonicalValue::Sequence(items))
        }
        CanonicalType::Group(fields) => {
            let count = reader.array()?;
            if count != fields.len() as u64 {
                return Err(DecodeError::WrongArity {
                    expected: fields.len(),
                    found: count,
                });
            }
            let mut items = Vec::with_capacity(fields.len());
            for field in fields {
                items.push(decode_value(
                    &format!("{path}.{}", field.name),
                    field.ty,
                    reader,
                )?);
            }
            Ok(CanonicalValue::Group(items))
        }
        CanonicalType::Record => Ok(CanonicalValue::Record(reader.skip_item()?.to_vec())),
    }
}

/// Why a record's fields did not match its published schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaViolation {
    /// The record produced a different number of fields than the schema declares.
    Arity {
        /// The path to the record or group.
        path: String,
        /// How many fields the schema declares.
        expected: usize,
        /// How many the record produced.
        found: usize,
    },
    /// A field held a value of the wrong shape.
    Shape {
        /// The path to the field.
        path: String,
        /// The type the schema declares.
        expected: &'static str,
        /// The shape the value actually had.
        found: &'static str,
    },
    /// A fixed-width byte field held the wrong number of bytes.
    Width {
        /// The path to the field.
        path: String,
        /// The width the schema fixes.
        expected: u32,
        /// The width the value had.
        found: usize,
    },
}

impl core::fmt::Display for SchemaViolation {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Arity {
                path,
                expected,
                found,
            } => write!(
                formatter,
                "{path}: the schema declares {expected} fields, the record produced {found}"
            ),
            Self::Shape {
                path,
                expected,
                found,
            } => write!(formatter, "{path}: expected {expected}, found {found}"),
            Self::Width {
                path,
                expected,
                found,
            } => write!(
                formatter,
                "{path}: expected {expected} bytes, found {found}"
            ),
        }
    }
}

impl std::error::Error for SchemaViolation {}

/// The shape name of a value, for a violation message.
const fn shape_of(value: &CanonicalValue) -> &'static str {
    match value {
        CanonicalValue::Unsigned(_) => "unsigned",
        CanonicalValue::Bool(_) => "bool",
        CanonicalValue::Bytes(_) => "bytes",
        CanonicalValue::Text(_) => "text",
        CanonicalValue::Sequence(_) => "sequence",
        CanonicalValue::Group(_) => "group",
        CanonicalValue::Record(_) => "record",
    }
}

/// Check `values` against `fields` at `path`, appending every violation found.
fn check_fields(
    path: &str,
    fields: &[FieldSchema],
    values: &[CanonicalValue],
    found: &mut Vec<SchemaViolation>,
) {
    if fields.len() != values.len() {
        found.push(SchemaViolation::Arity {
            path: path.to_owned(),
            expected: fields.len(),
            found: values.len(),
        });
        return;
    }
    for (field, value) in fields.iter().zip(values) {
        check_value(&format!("{path}.{}", field.name), field.ty, value, found);
    }
}

/// Check one value against one declared type.
fn check_value(
    path: &str,
    ty: CanonicalType,
    value: &CanonicalValue,
    found: &mut Vec<SchemaViolation>,
) {
    match (ty, value) {
        (CanonicalType::Unsigned, CanonicalValue::Unsigned(_))
        | (CanonicalType::Bool, CanonicalValue::Bool(_))
        | (CanonicalType::Text, CanonicalValue::Text(_))
        | (CanonicalType::Record, CanonicalValue::Record(_)) => {}
        (CanonicalType::Bytes(width), CanonicalValue::Bytes(bytes)) => {
            if let Some(width) = width {
                if bytes.len() != width as usize {
                    found.push(SchemaViolation::Width {
                        path: path.to_owned(),
                        expected: width,
                        found: bytes.len(),
                    });
                }
            }
        }
        (CanonicalType::Sequence(element), CanonicalValue::Sequence(items)) => {
            for (index, item) in items.iter().enumerate() {
                check_value(&format!("{path}[{index}]"), *element, item, found);
            }
        }
        (CanonicalType::Group(fields), CanonicalValue::Group(items)) => {
            check_fields(path, fields, items, found);
        }
        (expected, actual) => found.push(SchemaViolation::Shape {
            path: path.to_owned(),
            expected: expected.name(),
            found: shape_of(actual),
        }),
    }
}

/// Every way `record`'s fields disagree with its published schema; empty when they agree.
///
/// A record type whose `canonical_fields` drifts from its `SCHEMA` would keep encoding without
/// complaint and publish a schema that no longer describes the bytes. This is what makes that
/// drift a test failure instead.
#[must_use]
pub fn schema_violations<R: CanonicalEncode + ?Sized>(record: &R) -> Vec<SchemaViolation> {
    let mut found = Vec::new();
    check_fields(
        R::SCHEMA.domain.as_str(),
        R::SCHEMA.fields,
        &record.canonical_fields(),
        &mut found,
    );
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::Blake3;

    const GROUP_FIELDS: &[FieldSchema] = &[
        FieldSchema::new("left", CanonicalType::Unsigned),
        FieldSchema::new("right", CanonicalType::Text),
    ];

    struct Sample {
        fields: Vec<CanonicalValue>,
    }

    impl CanonicalEncode for Sample {
        const SCHEMA: RecordSchema = RecordSchema::new(
            DomainTag::new("test.sample"),
            &[
                FieldSchema::new("count", CanonicalType::Unsigned),
                FieldSchema::new("flag", CanonicalType::Bool),
                FieldSchema::new("key", CanonicalType::Bytes(Some(4))),
                FieldSchema::new("free", CanonicalType::Bytes(None)),
                FieldSchema::new("label", CanonicalType::Text),
                FieldSchema::new(
                    "many",
                    CanonicalType::Sequence(&CanonicalType::Group(GROUP_FIELDS)),
                ),
            ],
        );

        fn canonical_fields(&self) -> Vec<CanonicalValue> {
            self.fields.clone()
        }
    }

    fn well_formed() -> Sample {
        Sample {
            fields: vec![
                CanonicalValue::Unsigned(1),
                CanonicalValue::Bool(true),
                CanonicalValue::Bytes(vec![1, 2, 3, 4]),
                CanonicalValue::Bytes(vec![9]),
                CanonicalValue::Text("x".to_owned()),
                CanonicalValue::Sequence(vec![CanonicalValue::Group(vec![
                    CanonicalValue::Unsigned(2),
                    CanonicalValue::Text("y".to_owned()),
                ])]),
            ],
        }
    }

    #[test]
    fn a_well_formed_record_has_no_violation() {
        assert_eq!(schema_violations(&well_formed()), Vec::new());
    }

    #[test]
    fn the_domain_tag_is_the_first_element() {
        let bytes = encode_canonical(&well_formed());
        // Seven elements: the tag and six fields.
        assert_eq!(bytes[0], 0x87);
        assert_eq!(&bytes[1..13], b"\x6btest.sample");
    }

    #[test]
    fn encoding_is_a_pure_function_of_the_fields() {
        let sample = well_formed();
        assert_eq!(encode_canonical(&sample), encode_canonical(&sample));
        assert_eq!(
            canonical_digest::<Blake3, _>(&sample),
            canonical_digest::<Blake3, _>(&sample)
        );
        assert_eq!(
            canonical_digest::<Blake3, _>(&sample),
            Blake3::digest_bytes(&encode_canonical(&sample))
        );
    }

    /// The property the whole encoding exists for, at its smallest: two fields swapped is two
    /// different byte strings.
    #[test]
    fn swapping_two_fields_changes_the_bytes() {
        let sample = well_formed();
        let mut swapped = well_formed();
        swapped.fields.swap(0, 1);
        assert_ne!(encode_canonical(&sample), encode_canonical(&swapped));
    }

    #[test]
    fn a_wrong_arity_is_reported() {
        let mut sample = well_formed();
        sample.fields.pop();
        assert_eq!(
            schema_violations(&sample),
            vec![SchemaViolation::Arity {
                path: "test.sample".to_owned(),
                expected: 6,
                found: 5,
            }]
        );
    }

    #[test]
    fn a_wrong_shape_is_reported_with_its_path() {
        let mut sample = well_formed();
        sample.fields[1] = CanonicalValue::Unsigned(1);
        assert_eq!(
            schema_violations(&sample),
            vec![SchemaViolation::Shape {
                path: "test.sample.flag".to_owned(),
                expected: "bool",
                found: "unsigned",
            }]
        );
    }

    #[test]
    fn a_wrong_fixed_width_is_reported() {
        let mut sample = well_formed();
        sample.fields[2] = CanonicalValue::Bytes(vec![1, 2, 3]);
        assert_eq!(
            schema_violations(&sample),
            vec![SchemaViolation::Width {
                path: "test.sample.key".to_owned(),
                expected: 4,
                found: 3,
            }]
        );
        // A field whose width is not fixed accepts any length.
        let mut free = well_formed();
        free.fields[3] = CanonicalValue::Bytes(vec![1; 100]);
        assert_eq!(schema_violations(&free), Vec::new());
    }

    #[test]
    fn a_violation_inside_a_group_carries_its_index() {
        let mut sample = well_formed();
        sample.fields[5] = CanonicalValue::Sequence(vec![CanonicalValue::Group(vec![
            CanonicalValue::Unsigned(2),
            CanonicalValue::Unsigned(3),
        ])]);
        assert_eq!(
            schema_violations(&sample),
            vec![SchemaViolation::Shape {
                path: "test.sample.many[0].right".to_owned(),
                expected: "text",
                found: "unsigned",
            }]
        );
    }

    #[test]
    fn every_violation_renders_a_message_naming_its_path() {
        let violations = [
            SchemaViolation::Arity {
                path: "a".to_owned(),
                expected: 1,
                found: 2,
            },
            SchemaViolation::Shape {
                path: "b".to_owned(),
                expected: "bool",
                found: "text",
            },
            SchemaViolation::Width {
                path: "c".to_owned(),
                expected: 32,
                found: 31,
            },
        ];
        for violation in &violations {
            let rendered = violation.to_string();
            assert!(!rendered.is_empty());
        }
        assert_eq!(violations[2].to_string(), "c: expected 32 bytes, found 31");
    }

    #[test]
    fn an_optional_field_is_a_zero_or_one_element_sequence() {
        // The profile has no absence marker; both shapes are ordinary arrays.
        let mut writer = CborWriter::new();
        CanonicalValue::Sequence(Vec::new()).write(&mut writer);
        assert_eq!(writer.finish(), vec![0x80]);

        let mut writer = CborWriter::new();
        CanonicalValue::Sequence(vec![CanonicalValue::Unsigned(1)]).write(&mut writer);
        assert_eq!(writer.finish(), vec![0x81, 0x01]);
    }

    #[test]
    fn the_helpers_build_byte_fields() {
        assert_eq!(
            CanonicalValue::from_array([1u8, 2, 3]),
            CanonicalValue::Bytes(vec![1, 2, 3])
        );
        assert_eq!(
            CanonicalValue::from_digest(&Digest32::from_bytes([7; 32])),
            CanonicalValue::Bytes(vec![7; 32])
        );
    }

    #[test]
    fn every_canonical_type_publishes_a_name() {
        assert_eq!(CanonicalType::Unsigned.name(), "unsigned");
        assert_eq!(CanonicalType::Bool.name(), "bool");
        assert_eq!(CanonicalType::Bytes(Some(32)).name(), "bytes");
        assert_eq!(CanonicalType::Text.name(), "text");
        assert_eq!(
            CanonicalType::Sequence(&CanonicalType::Unsigned).name(),
            "sequence"
        );
        assert_eq!(CanonicalType::Group(GROUP_FIELDS).name(), "group");
        assert_eq!(CanonicalType::Record.name(), "record");
    }
}
