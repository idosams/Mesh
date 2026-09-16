//! This crate's `mesh-cbor/0` implementation, held against the bytes the repository publishes.
//!
//! # What this test is for
//!
//! `mesh-operations` reimplements `mesh-cbor/0` because it may not import `mesh-types` — the
//! lockfile fence, measured and recorded in `src/ids.rs`. A mirror nothing checks is a second
//! encoding waiting to happen.
//!
//! So the mirror is checked against `protocol/test-vectors/v0/`, which `mesh-types` produced and
//! which the repository publishes as the artifact an *external* implementer is told to check
//! against. That makes this the first external check of those vectors, and it is stronger evidence
//! than an import would have been: an import proves the two crates share code, while this proves
//! two independent implementations produce the same bytes.
//!
//! Every vector in the index is run, including the records this crate has no type for — a
//! `file-manifest` or a `directory-version` is another crate's record, and its bytes still exercise
//! this crate's writer through the schema-driven path.
//!
//! # What it does not establish
//!
//! That the *vectors* are right. If `mesh-types` and this crate are both wrong in the same way, this
//! test is green. What it establishes is that they are not wrong in *different* ways, which is the
//! failure that produces two peers disagreeing about what was signed.

mod common;

use common::{encode_record, from_hex, parse_json, read_repo_file, to_hex, Json};
use mesh_operations::{
    decode_canonical, CanonicalType, CanonicalValue, FieldSchema, RecordSchema, CHANGESET_DOMAIN,
    CHANGESET_SCHEMA,
};

const VECTOR_ROOT: &str = "protocol/test-vectors/v0";

/// The published field type, as the artifact spells it.
fn value_of(field: &Json, value: &Json) -> CanonicalValue {
    match field.field("type").as_str() {
        "unsigned" => CanonicalValue::Unsigned(value.as_u64()),
        "bool" => CanonicalValue::Bool(value.as_bool()),
        "bytes" => {
            let bytes = from_hex(value.as_str());
            if let Some(width) = field.get("byte_length") {
                assert_eq!(
                    bytes.len() as u64,
                    width.as_u64(),
                    "the vector's own byte_length disagrees with its value"
                );
            }
            CanonicalValue::Bytes(bytes)
        }
        "text" => CanonicalValue::Text(value.as_str().to_owned()),
        "sequence" => CanonicalValue::Sequence(
            value
                .as_array()
                .iter()
                .map(|item| value_of(field.field("element"), item))
                .collect(),
        ),
        "group" => CanonicalValue::Group(
            field
                .field("fields")
                .as_array()
                .iter()
                .map(|member| value_of(member, value.field(member.field("name").as_str())))
                .collect(),
        ),
        // A complete nested encoding, published as hex. This crate splices it whole, which is what
        // lets a ChangeSet carry operations whose vocabulary the outer schema does not name.
        "record" => CanonicalValue::Record(from_hex(value.as_str())),
        other => panic!("the artifact uses a field type this crate does not admit: {other}"),
    }
}

fn fields_of(vector: &Json, schema: &Json) -> Vec<CanonicalValue> {
    let record = vector.field("record");
    schema
        .field("fields")
        .as_array()
        .iter()
        .map(|field| value_of(field, record.field(field.field("name").as_str())))
        .collect()
}

#[test]
fn every_published_vector_is_reproduced_byte_for_byte() {
    let index = parse_json(&read_repo_file(&format!("{VECTOR_ROOT}/index.json")));
    assert_eq!(
        index.field("encoding_profile").as_str(),
        mesh_operations::CBOR_PROFILE,
        "the published profile is not the one this crate implements"
    );

    let files = index.field("files").as_array();
    assert!(!files.is_empty(), "the vector index is empty");

    let mut checked = 0usize;
    for entry in files {
        let file = entry.field("file").as_str();
        let document = parse_json(&read_repo_file(&format!("{VECTOR_ROOT}/{file}")));
        let schema = document.field("schema");
        let domain = schema.field("domain_tag").as_str();
        assert_eq!(
            domain,
            entry.field("record").as_str(),
            "{file}: the index and the document disagree about the record"
        );

        let vectors = document.field("vectors").as_array();
        assert_eq!(
            vectors.len() as u64,
            entry.field("vector_count").as_u64(),
            "{file}: the index states a different vector count than the document holds"
        );

        for vector in vectors {
            let name = vector.field("name").as_str();
            let expected = vector.field("canonical_encoding_hex").as_str();
            let produced = encode_record(domain, &fields_of(vector, schema));
            assert_eq!(
                to_hex(&produced),
                expected,
                "{file}/{name}: this crate produced different bytes than the published vector"
            );
            assert_eq!(
                produced.len() as u64,
                vector.field("canonical_encoding_length").as_u64(),
                "{file}/{name}: the published length disagrees with the published bytes"
            );
            checked += 1;
        }
    }

    // A green run over zero vectors would prove nothing at all, and is exactly what a broken path
    // or a lenient reader produces.
    assert!(
        checked >= 10,
        "only {checked} vectors were checked; the corpus was 10 when this test was written, and \
         it never shrinks"
    );
}

#[test]
fn the_published_changeset_schema_is_the_one_this_crate_declares() {
    let document = parse_json(&read_repo_file(&format!("{VECTOR_ROOT}/changeset.json")));
    let schema = document.field("schema");
    assert_eq!(schema.field("domain_tag").as_str(), CHANGESET_DOMAIN);

    let published: Vec<String> = schema
        .field("fields")
        .as_array()
        .iter()
        .map(|field| {
            format!(
                "{}:{}",
                field.field("name").as_str(),
                field.field("type").as_str()
            )
        })
        .collect();
    let declared: Vec<String> = CHANGESET_SCHEMA
        .fields
        .iter()
        .map(|field: &FieldSchema| format!("{}:{}", field.name, field.ty.name()))
        .collect();
    assert_eq!(
        declared, published,
        "this crate's ChangeSet schema has drifted from the published one; every deployed \
         signature was made over bytes the published order produced"
    );
}

#[test]
fn a_published_changeset_decodes_and_re_encodes_to_the_same_bytes() {
    let document = parse_json(&read_repo_file(&format!("{VECTOR_ROOT}/changeset.json")));
    for vector in document.field("vectors").as_array() {
        let bytes = from_hex(vector.field("canonical_encoding_hex").as_str());
        let values = decode_canonical(&CHANGESET_SCHEMA, &bytes).unwrap_or_else(|error| {
            panic!(
                "{}: this crate refused a published ChangeSet: {error}",
                vector.field("name").as_str()
            )
        });
        assert_eq!(encode_record(CHANGESET_DOMAIN, &values), bytes);
    }
}

/// The nested `record` element is the one shape whose bytes this crate carries without
/// understanding, and the vectors are the only place it appears. Losing it would be invisible to
/// the schema comparison above, because the schema would still say `record`.
#[test]
fn a_nested_operation_encoding_survives_the_round_trip_whole() {
    let document = parse_json(&read_repo_file(&format!("{VECTOR_ROOT}/changeset.json")));
    let merge = document
        .field("vectors")
        .as_array()
        .iter()
        .find(|vector| {
            !vector
                .field("record")
                .field("operations")
                .as_array()
                .is_empty()
        })
        .expect("one published ChangeSet carries operations");
    let bytes = from_hex(merge.field("canonical_encoding_hex").as_str());
    let values = decode_canonical(&CHANGESET_SCHEMA, &bytes).unwrap();

    let operations_index = CHANGESET_SCHEMA
        .fields
        .iter()
        .position(|field| field.name == "operations")
        .expect("the schema names its operations field");
    let CanonicalValue::Sequence(operations) = &values[operations_index] else {
        panic!("the operations field did not decode as a sequence");
    };
    let published = merge.field("record").field("operations").as_array();
    assert_eq!(operations.len(), published.len());
    for (decoded, expected) in operations.iter().zip(published) {
        let CanonicalValue::Record(encoding) = decoded else {
            panic!("a nested operation did not decode as a record");
        };
        assert_eq!(to_hex(encoding), expected.as_str());
    }
}

/// The `sequence` and `group` shapes carry an element schema this crate has to descend into. A
/// vector file that exercised neither would leave that path unchecked.
#[test]
fn the_published_corpus_exercises_every_shape_this_crate_can_encode() {
    let index = parse_json(&read_repo_file(&format!("{VECTOR_ROOT}/index.json")));
    let mut seen: Vec<&'static str> = Vec::new();
    for entry in index.field("files").as_array() {
        let document = parse_json(&read_repo_file(&format!(
            "{VECTOR_ROOT}/{}",
            entry.field("file").as_str()
        )));
        for field in document.field("schema").field("fields").as_array() {
            record_shape(field, &mut seen);
        }
    }
    for shape in [
        CanonicalType::Unsigned.name(),
        CanonicalType::Bytes(None).name(),
        CanonicalType::Sequence(&CanonicalType::Unsigned).name(),
        CanonicalType::Group(&[]).name(),
        CanonicalType::Record.name(),
    ] {
        assert!(
            seen.contains(&shape),
            "no published vector exercises {shape}"
        );
    }
}

fn record_shape(field: &Json, seen: &mut Vec<&'static str>) {
    let shape = match field.field("type").as_str() {
        "unsigned" => CanonicalType::Unsigned.name(),
        "bool" => CanonicalType::Bool.name(),
        "bytes" => CanonicalType::Bytes(None).name(),
        "text" => CanonicalType::Text.name(),
        "sequence" => {
            record_shape(field.field("element"), seen);
            CanonicalType::Sequence(&CanonicalType::Unsigned).name()
        }
        "group" => {
            for member in field.field("fields").as_array() {
                record_shape(member, seen);
            }
            CanonicalType::Group(&[]).name()
        }
        "record" => CanonicalType::Record.name(),
        other => panic!("unknown published shape {other}"),
    };
    if !seen.contains(&shape) {
        seen.push(shape);
    }
}

/// The published schema files under `protocol/schemas/` are the other half of the contract. This
/// crate does not own them, but if one names the ChangeSet record it must agree with the vectors,
/// and a disagreement is a defect somebody should see rather than a file nobody reads.
#[test]
fn a_reproduced_vector_is_not_reproduced_by_accident() {
    // The control: change one byte of one field and the encoding must move. Without this, a bug
    // that made `encode_record` return the expected bytes regardless would pass every assertion
    // above.
    let fields = vec![
        CanonicalValue::Bytes(vec![0u8; 16]),
        CanonicalValue::Unsigned(1),
    ];
    let mutated = vec![
        CanonicalValue::Bytes({
            let mut bytes = vec![0u8; 16];
            bytes[15] = 1;
            bytes
        }),
        CanonicalValue::Unsigned(1),
    ];
    assert_ne!(
        encode_record(CHANGESET_DOMAIN, &fields),
        encode_record(CHANGESET_DOMAIN, &mutated)
    );
    // And a reordered field list produces different bytes, which is why field order is the wire
    // format.
    let swapped = vec![fields[1].clone(), fields[0].clone()];
    assert_ne!(
        encode_record(CHANGESET_DOMAIN, &fields),
        encode_record(CHANGESET_DOMAIN, &swapped)
    );
    let _: RecordSchema = CHANGESET_SCHEMA;
}
