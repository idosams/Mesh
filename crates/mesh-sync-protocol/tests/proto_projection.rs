//! `protocol/proto/mesh/v0/sync.proto` projects this crate's message set. This is what holds it there.
//!
//! # Why a source-text lint and not a generator
//!
//! Plan §8.1 puts protobuf on the network and canonical CBOR under the signature, so one CWP
//! message has two descriptions: the `SyncMessage` variant here and the protobuf message there.
//! Two descriptions that nothing compares are two descriptions that will disagree — a field added
//! to the projection and not to the message set is a field an implementer sends and no receiver
//! reads, and a field renamed here and not there is a client generated against a name that is gone.
//!
//! This crate declares no dependency (`mesh_types_drift.rs` explains why, and
//! `docs/adr/0014-narrow-the-lockfile-fence-to-admit-an-audited-cryptographic-dependency.md` is the
//! fence), so the comparison is made by reading source text — the same technique
//! `mesh_types_drift.rs` uses, with the same limits. It reads two files by path and recognises the
//! shapes those files use today; a refactor that changes the shapes makes this fail loudly rather
//! than pass silently, which is the direction it is better to be wrong in.
//!
//! # What is compared, in both directions
//!
//! * every message in the proto file names a Rust definition in [`PROJECTIONS`], and its field
//!   names and order equal that definition's;
//! * every message declared in the proto file is in [`PROJECTIONS`], so a message cannot be added
//!   there and escape the comparison;
//! * every `SyncMessage` variant is in [`PROJECTIONS`], so a nineteenth message cannot be added
//!   here and go unprojected;
//! * field numbers run sequentially from one, so a gap is either a mistake or a `reserved` that
//!   somebody has to write.
//!
//! # What it is not
//!
//! It is not a protobuf parser and must not grow into one. It is not a check that the projection
//! *encodes* anything: protobuf is not the encoding CWP messages are defined in, and the bytes are
//! pinned by `protocol-vectors.rs` and published in `protocol/wire/v0/messages.json`.

use std::fs;
use std::path::PathBuf;

use mesh_sync_protocol::MESSAGE_KINDS;

/// Which protobuf message projects which Rust definition, and where that definition lives.
///
/// The `Variant` rows are `SyncMessage` variants; the `Struct` rows are the compound elements those
/// variants carry. `Error` is a `Struct` row because `SyncMessage::Error` is a tuple variant with no
/// field names of its own — the three fields `docs/protocol.md` §7.3 lists for `ERROR` are
/// `ProtocolError`'s, and that is the definition the projection has to match.
const PROJECTIONS: [(&str, Source); 25] = [
    // Compound elements.
    (
        "SparseChangeSet",
        Source::Struct(MESSAGE_RS, "SparseChangeSet"),
    ),
    (
        "HeadAdvertisement",
        Source::Struct(MESSAGE_RS, "HeadAdvertisement"),
    ),
    (
        "CarriedChangeSet",
        Source::Struct(MESSAGE_RS, "CarriedChangeSet"),
    ),
    ("ChunkRequest", Source::Struct(MESSAGE_RS, "ChunkRequest")),
    ("ChunkPart", Source::Struct(MESSAGE_RS, "ChunkPart")),
    ("SummaryNode", Source::Struct(SUMMARY_RS, "SummaryNode")),
    ("MerkleSummary", Source::Struct(SUMMARY_RS, "MerkleSummary")),
    // The eighteen messages, in wire-tag order.
    ("Hello", Source::Variant("Hello")),
    ("Authenticate", Source::Variant("Authenticate")),
    ("AdvertiseFrontier", Source::Variant("AdvertiseFrontier")),
    ("RequestOperations", Source::Variant("RequestOperations")),
    ("OperationsBatch", Source::Variant("OperationsBatch")),
    ("AckOperations", Source::Variant("AckOperations")),
    ("AdvertiseManifests", Source::Variant("AdvertiseManifests")),
    ("RequestChunks", Source::Variant("RequestChunks")),
    ("ChunkBatch", Source::Variant("ChunkBatch")),
    ("AckChunks", Source::Variant("AckChunks")),
    ("UpdateActorHead", Source::Variant("UpdateActorHead")),
    (
        "UpdateCanonicalHead",
        Source::Variant("UpdateCanonicalHead"),
    ),
    ("Presence", Source::Variant("Presence")),
    ("ReviewBundle", Source::Variant("ReviewBundle")),
    ("ValidationReceipt", Source::Variant("ValidationReceipt")),
    ("ApprovalEnvelope", Source::Variant("ApprovalEnvelope")),
    ("AntiEntropySummary", Source::Variant("AntiEntropySummary")),
    ("Error", Source::Struct(ERROR_RS, "ProtocolError")),
];

const MESSAGE_RS: &str = "message.rs";
const SUMMARY_RS: &str = "summary.rs";
const ERROR_RS: &str = "error.rs";

/// Where a projected message's authoritative field list is written.
#[derive(Clone, Copy)]
enum Source {
    /// A `SyncMessage` variant in `src/message.rs`, by variant name.
    Variant(&'static str),
    /// A struct, by source file and struct name.
    Struct(&'static str, &'static str),
}

impl Source {
    /// The field names this definition declares, in declaration order.
    fn fields(self) -> Vec<String> {
        match self {
            Self::Variant(name) => variant_fields(&crate_source(MESSAGE_RS), name),
            Self::Struct(file, name) => struct_fields(&crate_source(file), name),
        }
    }

    /// How to describe it when a comparison fails.
    fn describe(self) -> String {
        match self {
            Self::Variant(name) => format!("SyncMessage::{name} in src/message.rs"),
            Self::Struct(file, name) => format!("struct {name} in src/{file}"),
        }
    }
}

// -------------------------------------------------------------------------------------------------
// The comparisons
// -------------------------------------------------------------------------------------------------

/// **The criterion.** Every projected message's field names and order equal the Rust definition's.
#[test]
fn every_projected_message_matches_the_rust_definition_it_projects() {
    let declared = parse_proto();
    for (message, source) in PROJECTIONS {
        let projected: Vec<String> = declared
            .iter()
            .find(|(name, _)| name == message)
            .map(|(_, fields)| fields.iter().map(|(name, _)| name.clone()).collect())
            .unwrap_or_else(|| {
                panic!(
                    "{PROTO_PATH} declares no message {message}, which {} needs",
                    source.describe()
                )
            });
        let expected: Vec<String> = source.fields();
        assert!(
            !expected.is_empty(),
            "{} was read as having no field at all, which means this test's reader stopped seeing \
             it rather than that the definition is empty",
            source.describe()
        );
        assert_eq!(
            projected,
            expected,
            "{PROTO_PATH}'s {message} and {} declare different fields, or the same fields in a \
             different order. The projection follows the message set, never the other way round: \
             if the Rust definition moved deliberately, move the projection with it in the same \
             change, and remember that a field NUMBER is never reused or renumbered.",
            source.describe()
        );
    }
}

/// No message in the proto file is unaccounted for, so one cannot be added there and silently
/// escape the comparison above.
#[test]
fn the_proto_declares_no_message_the_projection_table_does_not_know_about() {
    let mut declared: Vec<String> = parse_proto().into_iter().map(|(name, _)| name).collect();
    let mut known: Vec<String> = PROJECTIONS
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect();
    declared.sort();
    known.sort();
    assert_eq!(
        declared, known,
        "a message declared in {PROTO_PATH} with no row in PROJECTIONS is a message nothing \
         compares to the Rust definition it claims to project"
    );
}

/// Every message in the set is projected, so a nineteenth message cannot be added to the crate and
/// left off the wire.
#[test]
fn every_message_kind_has_a_projection() {
    for kind in MESSAGE_KINDS {
        let variant = format!("{kind:?}");
        assert!(
            PROJECTIONS.iter().any(
                |(_, source)| matches!(source, Source::Variant(name) if *name == variant)
                    || matches!(
                        (kind.as_str(), source),
                        ("ERROR", Source::Struct(_, "ProtocolError"))
                    )
            ),
            "{} ({}) has no protobuf projection in {PROTO_PATH}",
            kind.as_str(),
            variant
        );
    }
}

/// Field numbers are sequential from one, in declaration order. A gap or a repeat is either a
/// mistake or a reservation that has to be written as `reserved`.
#[test]
fn proto_field_numbers_are_sequential_from_one() {
    for (message, fields) in parse_proto() {
        let numbers: Vec<u32> = fields.into_iter().map(|(_, number)| number).collect();
        let expected: Vec<u32> = (1..=numbers.len() as u32).collect();
        assert_eq!(numbers, expected, "{message} field numbers");
    }
}

/// The projection says in prose what it is not, because a reader who takes protobuf for the
/// encoding will verify a signature over bytes the author never signed.
#[test]
fn the_projection_states_that_protobuf_is_not_the_canonical_encoding() {
    let source = proto_source();
    assert!(
        source.contains("PROTOBUF IS NOT THE ENCODING CWP MESSAGES ARE DEFINED IN"),
        "the file must say that protobuf is not the encoding CWP messages are defined in"
    );
    assert!(
        source.contains("re-encodes to `mesh-cbor/0` before comparing bytes"),
        "the file must say that a peer re-encodes to mesh-cbor/0 before comparing bytes"
    );
}

/// The reader is only worth having if it can see. A file it parses to nothing would pass every
/// comparison above.
#[test]
fn the_proto_reader_finds_messages_and_fields() {
    let messages = parse_proto();
    assert_eq!(
        messages.len(),
        25,
        "eighteen messages and seven compound elements"
    );
    let carried = messages
        .iter()
        .find(|(name, _)| name == "CarriedChangeSet")
        .expect("CarriedChangeSet is declared");
    assert_eq!(carried.1.len(), 8);
    assert_eq!(carried.1[0], ("id".to_owned(), 1));
    assert_eq!(carried.1[7], ("body".to_owned(), 8));
}

/// The Rust readers are only worth having if they can see, for the same reason.
#[test]
fn the_rust_readers_find_fields() {
    let message_rs = crate_source(MESSAGE_RS);
    assert_eq!(
        variant_fields(&message_rs, "Hello"),
        ["protocol_version", "encoding_profile", "actor", "challenge"]
    );
    assert_eq!(
        struct_fields(&message_rs, "ChunkPart"),
        ["content", "offset", "bytes", "is_final"]
    );
    assert_eq!(
        struct_fields(&crate_source(ERROR_RS), "ProtocolError"),
        ["code", "about", "detail"]
    );
    assert!(
        variant_fields(&message_rs, "NoSuchVariant").is_empty(),
        "a name that is not there reads as nothing, which the emptiness assertion in the \
         comparison turns into a failure"
    );
}

// -------------------------------------------------------------------------------------------------
// Readers
// -------------------------------------------------------------------------------------------------

const PROTO_PATH: &str = "protocol/proto/mesh/v0/sync.proto";

/// The projection, read from the repository root two directories above this crate.
fn proto_source() -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "protocol",
        "proto",
        "mesh",
        "v0",
        "sync.proto",
    ]
    .iter()
    .collect();
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). This test holds the protobuf projection against the message \
             set by reading both; if the file has moved, the projection is unchecked until this \
             path is corrected.",
            path.display()
        )
    })
}

/// One of this crate's own source files.
fn crate_source(file: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "src", file].iter().collect();
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {} ({error})", path.display()))
}

/// Every message in the proto file, with its field names and numbers in declaration order.
///
/// A deliberately small proto3 reader: `message X {` opens a message and `… name = n;` is a field.
/// Enough for a file this repository writes and no more — the same reader
/// `crates/mesh-types/tests/serialization-compat.rs` uses on `records.proto`, kept small on
/// purpose. If the projection ever needs nested messages, oneofs or imports, the right move is a
/// real parser in a tool that owns one, not more line matching here.
fn parse_proto() -> Vec<(String, Vec<(String, u32)>)> {
    let source = proto_source();
    let mut messages: Vec<(String, Vec<(String, u32)>)> = Vec::new();
    let mut open = false;
    for line in source.lines() {
        let line = line.trim();
        if line.starts_with("//") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("message ") {
            let name = rest.trim_end_matches('}').trim_end_matches('{').trim();
            messages.push((name.to_owned(), Vec::new()));
            open = !line.ends_with("{}");
            continue;
        }
        if line == "}" {
            open = false;
            continue;
        }
        if !open || !line.ends_with(';') {
            continue;
        }
        let Some((declaration, number)) = line.trim_end_matches(';').rsplit_once('=') else {
            continue;
        };
        let Some(name) = declaration.trim().rsplit(' ').next() else {
            continue;
        };
        let number: u32 = number.trim().parse().expect("a field number is a number");
        messages
            .last_mut()
            .expect("a field appears inside a message")
            .1
            .push((name.to_owned(), number));
    }
    messages
}

/// The field names of `pub struct <name> {` in `source`, in declaration order.
fn struct_fields(source: &str, name: &str) -> Vec<String> {
    let opener = format!("pub struct {name} {{");
    fields_after(source, &opener)
}

/// The field names of the `SyncMessage` variant `<name> {` in `source`, in declaration order.
///
/// Variants are indented inside the enum, so the opener is matched on the trimmed line. A tuple
/// variant such as `Error(ProtocolError)` has no braces and therefore no fields — which is why
/// `Error` is projected from `ProtocolError` instead.
fn variant_fields(source: &str, name: &str) -> Vec<String> {
    let opener = format!("{name} {{");
    fields_after(source, &opener)
}

/// Field names between an opening line and the first line whose trimmed form ends the block.
///
/// A field line is `name: Type,` — a doc comment, an attribute or a nested brace is skipped, and a
/// line that closes the block ends the scan. Deliberately shallow: every definition this file reads
/// is a flat record of named fields.
fn fields_after(source: &str, opener: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut inside = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if !inside {
            if trimmed == opener {
                inside = true;
            }
            continue;
        }
        if trimmed == "}" || trimmed == "}," {
            break;
        }
        if trimmed.starts_with("///") || trimmed.starts_with("//") || trimmed.starts_with('#') {
            continue;
        }
        let Some((declaration, _)) = trimmed.split_once(':') else {
            continue;
        };
        let field = declaration.trim_start_matches("pub ").trim();
        if !field.is_empty()
            && field.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
            })
        {
            fields.push(field.to_owned());
        }
    }
    fields
}
