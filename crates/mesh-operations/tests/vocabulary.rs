//! The vocabulary is plan §4.3's, and every member round-trips through the canonical encoding.
//!
//! The first test reads the plan. That is the difference between "nineteen operations" as a number
//! somebody typed into an array and as a fact about the document the programme is executing: a
//! member renamed here, or added here, or dropped from the plan, turns this red. It is the
//! mechanical half of this task's rule that **adding an operation is a protocol change**.

mod common;

use common::{encode_record, read_repo_file};
use mesh_operations::{
    decode_operation, encode_canonical, one_of_every_operation, peek_domain, schema_violations,
    variable_length_operations, AttributionConfidence, CanonicalEncode, CanonicalValue,
    DecodeError, DerivationKind, Operation, OperationKind, ReadRegion, ValidationOutcome,
    OPERATION_DOMAIN_PREFIX,
};

const PLAN: &str = "docs/plan/execution-plan.md";

/// The operation names plan §4.3 lists, in order, read out of the plan itself.
fn plan_vocabulary() -> Vec<String> {
    let plan = read_repo_file(PLAN);
    let section = plan
        .split_once("## 4.3 Operations")
        .expect("the plan has a §4.3")
        .1;
    let fence = section
        .split_once("```text")
        .expect("§4.3 opens a text block")
        .1;
    let listing = fence.split_once("```").expect("§4.3 closes it").0;
    listing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_vocabulary_is_the_one_the_plan_lists_in_the_order_it_lists_them() {
    let expected = plan_vocabulary();
    assert_eq!(
        expected.len(),
        19,
        "plan §4.3 no longer lists nineteen operations: {expected:?}"
    );
    let declared: Vec<String> = OperationKind::ALL
        .iter()
        .map(|kind| kind.as_str().to_owned())
        .collect();
    assert_eq!(
        declared, expected,
        "the vocabulary has drifted from plan §4.3. Adding, removing or renaming an operation is \
         a protocol change: it needs a test vector, a compatibility test and protocol review in \
         the same PR."
    );
}

#[test]
fn the_plan_still_says_raw_writes_are_not_distributed() {
    // The sentence the whole coalescing design rests on. If the plan drops it, this crate's
    // central constraint has changed and somebody must decide that deliberately.
    let plan = read_repo_file(PLAN);
    assert!(
        plan.contains(
            "Raw `write()` calls are not distributed as permanent user-visible operations"
        ),
        "plan §4.3 no longer states that raw writes are not distributed"
    );
}

#[test]
fn every_operation_round_trips_through_the_canonical_encoding() {
    let corpus: Vec<Operation> = one_of_every_operation()
        .into_iter()
        .chain(variable_length_operations())
        .collect();
    assert!(corpus.len() >= 23);
    for operation in corpus {
        let bytes = encode_canonical(&operation);
        assert_eq!(
            decode_operation(&bytes).unwrap(),
            operation,
            "{:?} did not survive the round trip",
            operation.kind()
        );
        // And the bytes are a fixed point: re-encoding what was decoded gives the same bytes.
        assert_eq!(encode_canonical(&decode_operation(&bytes).unwrap()), bytes);
    }
}

#[test]
fn encoding_is_deterministic_across_repeated_calls() {
    for operation in one_of_every_operation() {
        let once = encode_canonical(&operation);
        for _ in 0..4 {
            assert_eq!(encode_canonical(&operation), once);
        }
    }
}

#[test]
fn every_operation_leads_with_its_own_domain_tag() {
    for operation in one_of_every_operation() {
        let bytes = encode_canonical(&operation);
        assert_eq!(peek_domain(&bytes).unwrap(), operation.kind().domain());
        assert!(operation
            .kind()
            .domain()
            .starts_with(OPERATION_DOMAIN_PREFIX));
    }
}

#[test]
fn every_operation_agrees_with_its_published_schema() {
    for operation in one_of_every_operation()
        .into_iter()
        .chain(variable_length_operations())
    {
        let problems = schema_violations(operation.schema(), &operation.canonical_fields());
        assert!(problems.is_empty(), "{:?}: {problems:?}", operation.kind());
    }
}

/// The round-trip test is only worth having if a changed byte is visible. Every operation is
/// mutated in its first byte-string field and must encode differently.
#[test]
fn one_changed_field_changes_the_encoding_of_every_operation() {
    for operation in one_of_every_operation() {
        let original = encode_canonical(&operation);
        let mut fields = operation.canonical_fields();
        let mutated = fields.iter_mut().find_map(|field| match field {
            CanonicalValue::Bytes(bytes) if !bytes.is_empty() => {
                bytes[0] ^= 0xff;
                Some(())
            }
            _ => None,
        });
        assert!(
            mutated.is_some(),
            "{:?} has no byte-string field to mutate",
            operation.kind()
        );
        let mutated_bytes = encode_record(operation.kind().domain(), &fields);
        assert_ne!(mutated_bytes, original);
        // And the mutated bytes are still a valid encoding of *some* value of the same member, so
        // the difference is the field and not a broken encoder.
        let decoded = decode_operation(&mutated_bytes).unwrap();
        assert_eq!(decoded.kind(), operation.kind());
        assert_ne!(decoded, operation);
    }
}

/// Two members must never share a domain tag, and no member's encoding may decode as another's.
#[test]
fn no_operation_decodes_as_another_member() {
    let corpus = one_of_every_operation();
    for operation in &corpus {
        let bytes = encode_canonical(operation);
        let decoded = decode_operation(&bytes).unwrap();
        assert_eq!(decoded.kind(), operation.kind());
        for other in &corpus {
            if other.kind() == operation.kind() {
                continue;
            }
            assert_ne!(
                encode_canonical(other),
                bytes,
                "{:?} and {:?} encode identically",
                operation.kind(),
                other.kind()
            );
        }
    }
}

#[test]
fn truncating_any_operation_encoding_is_refused() {
    for operation in one_of_every_operation() {
        let bytes = encode_canonical(&operation);
        for cut in 1..bytes.len() {
            let truncated = &bytes[..cut];
            assert!(
                decode_operation(truncated).is_err(),
                "{:?} decoded from {cut} of {} bytes",
                operation.kind(),
                bytes.len()
            );
        }
    }
}

#[test]
fn appending_a_byte_to_any_operation_encoding_is_refused() {
    for operation in one_of_every_operation() {
        let mut bytes = encode_canonical(&operation);
        bytes.push(0x00);
        assert!(matches!(
            decode_operation(&bytes),
            Err(DecodeError::TrailingBytes { .. })
        ));
    }
}

/// Every schema field name is a stable published name. A duplicate inside one record would make
/// the published schema ambiguous to a reader even though the bytes stay positional.
#[test]
fn no_schema_repeats_a_field_name() {
    for kind in OperationKind::ALL {
        let mut names: Vec<&str> = kind
            .schema()
            .fields
            .iter()
            .map(|field| field.name)
            .collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            before,
            "{} repeats a field name",
            kind.as_str()
        );
    }
}

/// The four enumerated fields ride as **indices into their own `ALL` array**, so the array order is
/// the wire format exactly as the schema field order is. Nothing above catches a reorder: a
/// round-trip still succeeds, both directions agreeing on the new — wrong — number.
///
/// This pins every index to a literal. Reordering `ReadRegion::ALL` after this ships changes what a
/// peer reads a stored observation as, and that is a compatibility event rather than a refactor.
#[test]
fn every_enumerated_wire_value_is_pinned() {
    let expected: [(&str, Vec<&str>); 4] = [
        (
            "ReadRegion",
            ReadRegion::ALL.iter().map(ReadRegion::as_str).collect(),
        ),
        (
            "AttributionConfidence",
            AttributionConfidence::ALL
                .iter()
                .map(AttributionConfidence::as_str)
                .collect(),
        ),
        (
            "DerivationKind",
            DerivationKind::ALL
                .iter()
                .map(DerivationKind::as_str)
                .collect(),
        ),
        (
            "ValidationOutcome",
            ValidationOutcome::ALL
                .iter()
                .map(ValidationOutcome::as_str)
                .collect(),
        ),
    ];
    let pinned: [(&str, &[&str]); 4] = [
        (
            "ReadRegion",
            &[
                "WholeFile",
                "ByteRanges",
                "TextSections",
                "PDFPages",
                "SpreadsheetCells",
                "CodeSymbols",
                "Unknown",
            ],
        ),
        (
            "AttributionConfidence",
            &[
                "ExactIntegratedRead",
                "ExactFilesystemRange",
                "FilesystemReadAhead",
                "ProcessInferred",
                "RecoveryDetected",
                "Unknown",
            ],
        ),
        (
            "DerivationKind",
            &[
                "DocumentParse",
                "SourceIndex",
                "DiffGeneration",
                "Embedding",
                "RunMemory",
                "TestResult",
                "Validation",
                "ModelSummary",
            ],
        ),
        (
            "ValidationOutcome",
            &["Passed", "Failed", "Skipped", "Errored"],
        ),
    ];
    for ((name, found), (pinned_name, pinned_order)) in expected.iter().zip(pinned) {
        assert_eq!(*name, pinned_name);
        assert_eq!(
            found.as_slice(),
            pinned_order,
            "{name}'s wire order changed. The index IS the wire value, so this moves every \
             encoding that carries it — a compatibility event, not a refactor."
        );
    }
}

/// And the pin is only worth having if a reorder is actually observable in the bytes. The control
/// encodes two observations that differ only in their region and requires different bytes.
#[test]
fn an_enumerated_field_reaches_the_bytes() {
    let one = Operation::RecordReadObservation {
        actor_id: mesh_operations::ActorId::from_bytes([1; 32]),
        object_id: mesh_operations::ObjectId::from_bytes([2; 16]),
        version_id: mesh_operations::VersionId::from_bytes([3; 32]),
        region: ReadRegion::WholeFile,
        confidence: AttributionConfidence::ExactIntegratedRead,
    };
    let other = Operation::RecordReadObservation {
        actor_id: mesh_operations::ActorId::from_bytes([1; 32]),
        object_id: mesh_operations::ObjectId::from_bytes([2; 16]),
        version_id: mesh_operations::VersionId::from_bytes([3; 32]),
        region: ReadRegion::CodeSymbols,
        confidence: AttributionConfidence::ExactIntegratedRead,
    };
    assert_ne!(encode_canonical(&one), encode_canonical(&other));
    assert_eq!(decode_operation(&encode_canonical(&one)).unwrap(), one);
}

#[test]
fn workspace_root_matches_the_published_schema_and_vector() {
    let schema = common::parse_json(&read_repo_file(
        "protocol/operations/v0/workspace-root-schema.json",
    ));
    let vector = common::parse_json(&read_repo_file(
        "protocol/operations/v0/workspace-root-vector.json",
    ));
    assert_eq!(
        schema.field("record").as_str(),
        "mesh.v0.op.initialize-workspace"
    );
    let operation = Operation::InitializeWorkspace {
        root_id: mesh_operations::ObjectId::from_bytes([0x17; 16]),
    };
    let encoded = encode_canonical(&operation);
    let hex: String = encoded.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(hex, vector.field("canonical_encoding_hex").as_str());
    assert_eq!(
        vector.field("input").field("root_id").as_str(),
        "17".repeat(16)
    );
    assert_eq!(decode_operation(&encoded).unwrap(), operation);
    let fields = schema.field("schema").field("fields").as_array();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].field("name").as_str(), "root_id");
    assert_eq!(fields[0].field("type").as_str(), "bytes");
    assert_eq!(fields[0].field("byte_length"), &common::Json::Number(16.0));
}
