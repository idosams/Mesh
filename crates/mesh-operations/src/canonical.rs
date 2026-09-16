//! The canonical encoding: the bytes a signed record *is*, and the schema that describes them.
//!
//! # The shape, in one sentence
//!
//! A record encodes as a definite-length [`mesh-cbor/0`](crate::CBOR_PROFILE) array whose first
//! element is the record's domain tag and whose remaining elements are its fields, in schema
//! order, one array element per field.
//!
//! ```text
//! [ "mesh.v0.op.create-file", h'01…10' ]
//!   ^ domain tag              ^ object_id
//! ```
//!
//! # One difference from `mesh-types`, and it is the reason this file is not a copy
//!
//! `mesh-types` carries the schema as an associated `const SCHEMA` on a trait. That is right for a
//! record type with one shape, and wrong for a *vocabulary*: [`Operation`](crate::Operation) is a
//! sum with eighteen members, each with its own domain tag and its own field list, and an
//! associated constant cannot vary by variant. So [`CanonicalEncode::schema`] here is a method.
//!
//! That difference is what makes a decoder possible for a sum type at all. The domain tag is the
//! first array element, so [`decode_operation`](crate::decode_operation) reads the tag, looks up
//! the member it names, and decodes the rest against *that* member's schema — an external
//! implementer needs the published schema list and nothing else.
//!
//! # No optional fields, at all
//!
//! The profile has no `null` and no absence marker, so "no optional-field ambiguity" is not a rule
//! this encoding enforces — it is a shape it cannot express. A field that may be absent is modelled
//! as a [`CanonicalType::Sequence`] of at most one element: `[]` for absent, `[v]` for present.

use crate::cbor::CborWriter;
use crate::cbor_reader::{CborError, CborReader};

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
    /// A fixed-arity array of named fields, in order.
    Group(&'static [FieldSchema]),
    /// A complete nested canonical encoding, carrying its own domain tag as its first element.
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

/// The published schema of one record type: its domain tag and its field order.
///
/// **The field order is the wire format.** Reordering this list changes every encoding of every
/// record of this type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordSchema {
    /// The domain tag, encoded as the first element of every record of this type.
    pub domain: &'static str,
    /// The fields, in encoding order.
    pub fields: &'static [FieldSchema],
}

impl RecordSchema {
    /// Declare a record schema.
    #[must_use]
    pub const fn new(domain: &'static str, fields: &'static [FieldSchema]) -> Self {
        Self { domain, fields }
    }
}

/// A value in the canonical profile, ready to encode.
///
/// Holding the fields as data rather than writing them straight into a writer is what makes "a
/// reordered field changes the bytes" a property a test can exercise directly, and what lets the
/// schema be checked against the values over a whole corpus.
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
/// [`CanonicalEncode::schema`] is a method rather than an associated constant so that a
/// vocabulary — one Rust type, eighteen record shapes — can implement it. See this module's
/// header.
pub trait CanonicalEncode {
    /// This value's published schema.
    fn schema(&self) -> &'static RecordSchema;

    /// The field values, in `schema` order and of the types the schema declares.
    fn canonical_fields(&self) -> Vec<CanonicalValue>;
}

/// The canonical encoding of `record`: the bytes a signature covers.
#[must_use]
pub fn encode_canonical<R: CanonicalEncode + ?Sized>(record: &R) -> Vec<u8> {
    let schema = record.schema();
    let fields = record.canonical_fields();
    let mut writer = CborWriter::with_capacity(64);
    writer.array((fields.len() + 1) as u64);
    writer.text(schema.domain);
    for field in &fields {
        field.write(&mut writer);
    }
    writer.finish()
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
    /// The leading domain tag named no member of the vocabulary.
    UnknownDomain {
        /// The tag the bytes carried.
        found: String,
    },
    /// A field held a value the vocabulary does not admit — an out-of-range enumerated value, or a
    /// name that is not normalized.
    UnadmittedValue {
        /// The path to the field.
        path: String,
        /// Why it was refused.
        reason: String,
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
            Self::UnknownDomain { found } => {
                write!(formatter, "{found} names no member of the vocabulary")
            }
            Self::UnadmittedValue { path, reason } => write!(formatter, "{path}: {reason}"),
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

/// The domain tag some canonical bytes lead with, without decoding anything else.
///
/// This is the dispatch step for a sum type: read the tag, then decode against the member it
/// names.
pub fn peek_domain(bytes: &[u8]) -> Result<&str, DecodeError> {
    let mut reader = CborReader::new(bytes);
    let arity = reader.array()?;
    if arity == 0 {
        return Err(DecodeError::WrongArity {
            expected: 1,
            found: 0,
        });
    }
    Ok(reader.text()?)
}

/// Decode `bytes` as `schema`'s field values.
///
/// Refuses a wrong domain tag, a wrong arity, a wrong fixed width, and any trailing byte.
pub fn decode_canonical(
    schema: &RecordSchema,
    bytes: &[u8],
) -> Result<Vec<CanonicalValue>, DecodeError> {
    let mut reader = CborReader::new(bytes);
    let arity = reader.array()?;
    let expected = schema.fields.len() + 1;
    if arity != expected as u64 {
        return Err(DecodeError::WrongArity {
            expected,
            found: arity,
        });
    }
    let domain = reader.text()?;
    if domain != schema.domain {
        return Err(DecodeError::WrongDomain {
            expected: schema.domain,
            found: domain.to_owned(),
        });
    }

    let mut values = Vec::with_capacity(schema.fields.len());
    for field in schema.fields {
        values.push(read_value(&mut reader, &field.ty, field.name)?);
    }
    if !reader.is_exhausted() {
        return Err(DecodeError::TrailingBytes {
            at: reader.position(),
            extra: reader.remaining(),
        });
    }
    Ok(values)
}

fn read_value(
    reader: &mut CborReader<'_>,
    ty: &CanonicalType,
    path: &str,
) -> Result<CanonicalValue, DecodeError> {
    match ty {
        CanonicalType::Unsigned => Ok(CanonicalValue::Unsigned(reader.unsigned()?)),
        CanonicalType::Bool => Ok(CanonicalValue::Bool(reader.bool()?)),
        CanonicalType::Bytes(width) => {
            let bytes = reader.bytes()?;
            if let Some(expected) = width {
                if bytes.len() != *expected as usize {
                    return Err(DecodeError::WrongWidth {
                        path: path.to_owned(),
                        expected: *expected,
                        found: bytes.len(),
                    });
                }
            }
            Ok(CanonicalValue::Bytes(bytes.to_vec()))
        }
        CanonicalType::Text => Ok(CanonicalValue::Text(reader.text()?.to_owned())),
        CanonicalType::Sequence(element) => {
            reader.enter()?;
            let count = reader.array()?;
            let mut items = Vec::new();
            for index in 0..count {
                items.push(read_value(reader, element, &format!("{path}[{index}]"))?);
            }
            reader.leave();
            Ok(CanonicalValue::Sequence(items))
        }
        CanonicalType::Group(fields) => {
            reader.enter()?;
            let count = reader.array()?;
            if count != fields.len() as u64 {
                return Err(DecodeError::WrongArity {
                    expected: fields.len(),
                    found: count,
                });
            }
            let mut items = Vec::with_capacity(fields.len());
            for field in *fields {
                items.push(read_value(
                    reader,
                    &field.ty,
                    &format!("{path}.{}", field.name),
                )?);
            }
            reader.leave();
            Ok(CanonicalValue::Group(items))
        }
        CanonicalType::Record => Ok(CanonicalValue::Record(reader.skip_item()?.to_vec())),
    }
}

/// Whether `values` match `schema` shape for shape.
///
/// Used by the tests over a generated corpus rather than on the encoding path: a schema that
/// disagrees with the values it describes publishes a lie to external implementers, and that is
/// the failure this catches.
#[must_use]
pub fn schema_violations(schema: &RecordSchema, values: &[CanonicalValue]) -> Vec<String> {
    let mut problems = Vec::new();
    if values.len() != schema.fields.len() {
        problems.push(format!(
            "{}: schema declares {} fields, the value carried {}",
            schema.domain,
            schema.fields.len(),
            values.len()
        ));
        return problems;
    }
    for (field, value) in schema.fields.iter().zip(values) {
        check_value(&field.ty, value, field.name, &mut problems);
    }
    problems
}

fn check_value(ty: &CanonicalType, value: &CanonicalValue, path: &str, problems: &mut Vec<String>) {
    match (ty, value) {
        (CanonicalType::Unsigned, CanonicalValue::Unsigned(_))
        | (CanonicalType::Bool, CanonicalValue::Bool(_))
        | (CanonicalType::Text, CanonicalValue::Text(_))
        | (CanonicalType::Record, CanonicalValue::Record(_)) => {}
        (CanonicalType::Bytes(width), CanonicalValue::Bytes(bytes)) => {
            if let Some(expected) = width {
                if bytes.len() != *expected as usize {
                    problems.push(format!(
                        "{path}: schema fixes {expected} bytes, the value carried {}",
                        bytes.len()
                    ));
                }
            }
        }
        (CanonicalType::Sequence(element), CanonicalValue::Sequence(items)) => {
            for (index, item) in items.iter().enumerate() {
                check_value(element, item, &format!("{path}[{index}]"), problems);
            }
        }
        (CanonicalType::Group(fields), CanonicalValue::Group(items)) => {
            if fields.len() != items.len() {
                problems.push(format!(
                    "{path}: group declares {} fields, the value carried {}",
                    fields.len(),
                    items.len()
                ));
                return;
            }
            for (field, item) in fields.iter().zip(items) {
                check_value(&field.ty, item, &format!("{path}.{}", field.name), problems);
            }
        }
        (declared, found) => problems.push(format!(
            "{path}: schema declares {}, the value is {}",
            declared.name(),
            match found {
                CanonicalValue::Unsigned(_) => "unsigned",
                CanonicalValue::Bool(_) => "bool",
                CanonicalValue::Bytes(_) => "bytes",
                CanonicalValue::Text(_) => "text",
                CanonicalValue::Sequence(_) => "sequence",
                CanonicalValue::Group(_) => "group",
                CanonicalValue::Record(_) => "record",
            }
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIELDS: &[FieldSchema] = &[
        FieldSchema::new("count", CanonicalType::Unsigned),
        FieldSchema::new("id", CanonicalType::Bytes(Some(4))),
        FieldSchema::new("names", CanonicalType::Sequence(&CanonicalType::Text)),
    ];
    const SCHEMA: RecordSchema = RecordSchema::new("mesh.v0.test-record", FIELDS);

    struct Sample;

    impl CanonicalEncode for Sample {
        fn schema(&self) -> &'static RecordSchema {
            &SCHEMA
        }

        fn canonical_fields(&self) -> Vec<CanonicalValue> {
            vec![
                CanonicalValue::Unsigned(7),
                CanonicalValue::from_array([1u8, 2, 3, 4]),
                CanonicalValue::Sequence(vec![CanonicalValue::Text("a".into())]),
            ]
        }
    }

    #[test]
    fn a_record_round_trips_through_its_schema() {
        let bytes = encode_canonical(&Sample);
        assert_eq!(peek_domain(&bytes).unwrap(), "mesh.v0.test-record");
        assert_eq!(
            decode_canonical(&SCHEMA, &bytes).unwrap(),
            Sample.canonical_fields()
        );
    }

    #[test]
    fn a_foreign_domain_tag_is_refused() {
        const OTHER: RecordSchema = RecordSchema::new("mesh.v0.other", FIELDS);
        let bytes = encode_canonical(&Sample);
        assert!(matches!(
            decode_canonical(&OTHER, &bytes),
            Err(DecodeError::WrongDomain { .. })
        ));
    }

    #[test]
    fn a_wrong_fixed_width_is_refused() {
        let mut writer = CborWriter::new();
        writer.array(4);
        writer.text(SCHEMA.domain);
        writer.unsigned(7);
        writer.bytes(&[1, 2, 3]); // schema fixes four
        writer.array(0);
        assert!(matches!(
            decode_canonical(&SCHEMA, &writer.finish()),
            Err(DecodeError::WrongWidth {
                expected: 4,
                found: 3,
                ..
            })
        ));
    }

    #[test]
    fn a_trailing_byte_is_refused() {
        let mut bytes = encode_canonical(&Sample);
        bytes.push(0x00);
        assert!(matches!(
            decode_canonical(&SCHEMA, &bytes),
            Err(DecodeError::TrailingBytes { extra: 1, .. })
        ));
    }

    #[test]
    fn a_wrong_arity_is_refused() {
        let mut writer = CborWriter::new();
        writer.array(3);
        writer.text(SCHEMA.domain);
        writer.unsigned(7);
        writer.bytes(&[1, 2, 3, 4]);
        assert!(matches!(
            decode_canonical(&SCHEMA, &writer.finish()),
            Err(DecodeError::WrongArity {
                expected: 4,
                found: 3
            })
        ));
    }

    #[test]
    fn the_schema_checker_reports_a_mismatch() {
        assert!(schema_violations(&SCHEMA, &Sample.canonical_fields()).is_empty());
        let wrong = vec![
            CanonicalValue::Text("seven".into()),
            CanonicalValue::from_array([1u8, 2]),
            CanonicalValue::Sequence(vec![CanonicalValue::Unsigned(1)]),
        ];
        assert_eq!(schema_violations(&SCHEMA, &wrong).len(), 3);
    }
}
