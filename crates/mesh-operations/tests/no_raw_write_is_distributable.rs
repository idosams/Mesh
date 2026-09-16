//! No API exposes a raw write as a distributable operation.
//!
//! Three of the four checks below are structural and hold whatever anyone writes:
//! [`RawWrite`] has no content field, it does not implement `CanonicalEncode` (a `compile_fail`
//! doctest in `src/coalesce.rs`, pinned to `E0277`), and the one exit from the accumulator demands
//! a [`ManifestId`].
//!
//! The fourth is a source-text lint over this crate's own vocabulary, and it is here because the
//! structural ones cannot see a *future* edit that adds a byte-carrying variant. It reads
//! `src/operation.rs` — the file that defines what may be distributed — and fails if content ever
//! appears in it.
//!
//! Its limit, stated rather than claimed away: it is a lint over text. A byte field spelled behind
//! a type alias declared elsewhere would be invisible to it. It catches the way this rule would
//! actually be broken — a lane adding `bytes: Vec<u8>` to a variant because a call site needed it —
//! and the compile-fail doctest is what covers the case where the bytes take a longer route.

mod common;

use common::read_repo_file;
use mesh_operations::{
    encode_canonical, one_of_every_operation, CheckpointTrigger, ManifestId, NotCheckpointed,
    ObjectId, Operation, OperationKind, PortableMetadata, RawWrite, VersionId, WriteCoalescer,
};

const VOCABULARY: &str = "crates/mesh-operations/src/operation.rs";
const COALESCER: &str = "crates/mesh-operations/src/coalesce.rs";

/// Content, in the shapes a Rust type carries it.
const CONTENT_TYPES: [&str; 6] = ["u8", "Vec<u8>", "&[u8]", "String", "&str", "Bytes"];

/// The declaration of `pub enum Operation`, with its doc comments removed.
///
/// The enum body and not the whole file: `as_str` returning `&'static str` is a published *name*
/// and not content, and a lint that could not tell those apart would have to be switched off.
fn operation_enum_body() -> String {
    let source = read_repo_file(VOCABULARY);
    let opened = source
        .split_once("pub enum Operation {")
        .expect("src/operation.rs declares `pub enum Operation`")
        .1;
    let body = opened
        .split_once("\n}")
        .expect("the enum declaration closes at column zero")
        .0;
    body.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_vocabulary_carries_no_content_type() {
    let body = operation_enum_body();
    // A green run over an empty string would prove nothing, and is what a moved declaration
    // produces.
    assert!(
        body.contains("MoveEntry {") && body.len() > 1000,
        "the enum body read as {} characters; the lint below would pass vacuously",
        body.len()
    );

    for content in CONTENT_TYPES {
        assert!(
            !body.contains(content),
            "the Operation enum names {content}. The vocabulary carries no content: plan §4.3 says \
             raw writes are accumulated locally and coalesced into durable file versions, and a \
             byte-carrying variant would distribute them."
        );
    }
}

/// The lint is only worth having if it can fail. The same predicate is run over the variant a
/// lane would plausibly add.
#[test]
fn the_lint_would_catch_a_byte_carrying_variant() {
    let hypothetical = "    WriteBytes {\n        object_id: ObjectId,\n        at: u64,\n        \
                        content: Vec<u8>,\n    },";
    assert!(
        CONTENT_TYPES
            .iter()
            .any(|content| hypothetical.contains(content)),
        "the content-type list no longer catches the variant it exists to catch"
    );
}

/// The one operation that touches content names a manifest, never bytes. If that field were ever
/// renamed or retyped, the whole coalescing argument would be gone.
#[test]
fn the_content_bearing_operation_names_a_manifest() {
    let write = one_of_every_operation()
        .into_iter()
        .find(|operation| operation.kind() == OperationKind::WriteFileVersion)
        .expect("the corpus covers WriteFileVersion");
    let Operation::WriteFileVersion { manifest_id, .. } = &write else {
        panic!("WriteFileVersion changed shape");
    };
    assert_eq!(manifest_id.as_bytes().len(), 32);

    let schema = OperationKind::WriteFileVersion.schema();
    let names: Vec<&str> = schema.fields.iter().map(|field| field.name).collect();
    assert!(names.contains(&"manifest_id"), "{names:?}");
    for forbidden in ["content", "bytes", "data", "payload", "offset", "length"] {
        assert!(
            !names.contains(&forbidden),
            "the write operation's published schema carries a {forbidden} field"
        );
    }
}

/// No member of the vocabulary is a raw write by another name.
#[test]
fn no_member_of_the_vocabulary_is_a_raw_write() {
    for kind in OperationKind::ALL {
        let name = kind.as_str();
        for forbidden in [
            "RawWrite",
            "Write(",
            "WriteBytes",
            "WriteRange",
            "Truncate",
            "Append",
        ] {
            assert_ne!(name, forbidden, "the vocabulary carries {forbidden}");
        }
        // The domain tags are what a peer dispatches on, so they must not carry it either.
        assert!(!kind.domain().contains("raw"), "{}", kind.domain());
    }
}

/// The accumulator's only exit demands a manifest identifier. There is no overload, no default and
/// no builder that omits it, so producing a distributable operation requires the bytes to have been
/// content-addressed by a crate that is not this one.
#[test]
fn the_only_exit_from_the_accumulator_demands_a_manifest() {
    let object = ObjectId::from_bytes([1; 16]);
    let coalescer = WriteCoalescer::new(object)
        .record(RawWrite::new(object, 0, 4096))
        .unwrap()
        .record(RawWrite::new(object, 4096, 4096))
        .unwrap();
    assert_eq!(coalescer.raw_write_count(), 2);

    let (emptied, outcome) = coalescer.checkpoint(
        CheckpointTrigger::AtomicReplacement,
        VersionId::from_bytes([2; 32]),
        Vec::new(),
        ManifestId::from_bytes([3; 32]),
        PortableMetadata::new(false),
    );
    let operation = outcome.expect("two writes coalesce into one version");
    assert_eq!(operation.kind(), OperationKind::WriteFileVersion);

    // The accumulated writes are gone rather than carried: the operation's encoding is the same
    // whether two writes or two thousand produced it.
    assert!(emptied.is_empty());
    let mut many = WriteCoalescer::new(object);
    for index in 0..2000u64 {
        many = many
            .record(RawWrite::new(object, index * 4096, 4096))
            .unwrap();
    }
    let (_, other) = many.checkpoint(
        CheckpointTrigger::AtomicReplacement,
        VersionId::from_bytes([2; 32]),
        Vec::new(),
        ManifestId::from_bytes([3; 32]),
        PortableMetadata::new(false),
    );
    assert_eq!(
        encode_canonical(&operation),
        encode_canonical(&other.unwrap())
    );
}

/// The accumulator itself carries no content, so even a caller that kept every `RawWrite` it ever
/// made has kept no bytes.
#[test]
fn the_accumulator_source_names_no_content_type() {
    let source = read_repo_file(COALESCER);
    let code: String = source
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("//") && !trimmed.starts_with("///") && !trimmed.starts_with("//!")
        })
        .collect::<Vec<_>>()
        .join("\n");
    for content in ["Vec<u8>", "&[u8]", "[u8;"] {
        assert!(
            !code.contains(content),
            "{COALESCER} names {content}; the accumulator holds accounting, never content"
        );
    }
}

/// A raw write that names another object is refused rather than dropped, so "no raw write is
/// distributed" never becomes "a raw write was silently lost".
#[test]
fn a_misrouted_raw_write_is_refused_rather_than_discarded() {
    let object = ObjectId::from_bytes([1; 16]);
    let other = ObjectId::from_bytes([2; 16]);
    assert_eq!(
        WriteCoalescer::new(object).record(RawWrite::new(other, 0, 1)),
        Err(NotCheckpointed::ForeignObject {
            expected: object,
            found: other
        })
    );
}
