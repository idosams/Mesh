//! The published corpus: the exact bytes of every file under `protocol/schemas/` and
//! `protocol/test-vectors/`.
//!
//! # The one rule that makes this work
//!
//! [`published_documents`] returns each published file's **complete text**, and
//! `tests/serialization-compat.rs` compares that text to the working tree byte for byte in both
//! directions — no file the generator does not produce, no produced file that differs. So:
//!
//! * change the encoding, and every affected vector's bytes move, and the comparison fails;
//! * change a schema's field order, and the same;
//! * add a signed record type without a vector, and the coverage check fails;
//! * delete or hand-edit a published file, and the comparison fails.
//!
//! There is no way to change the encoding and keep CI green except by regenerating the corpus,
//! which is the acceptance criterion "an encoding change without a corresponding vector update
//! fails CI" stated as a mechanism rather than as an intention.
//!
//! Regenerate with:
//!
//! ```console
//! $ cargo test -p mesh-types --test serialization-compat -- --ignored write_published_documents
//! ```
//!
//! # Why the fixtures are constants and not generated
//!
//! A published vector is read by a person implementing CWP in another language. Its inputs have to
//! be inspectable and boring: fixed byte patterns, digests of short strings they can recompute, and
//! lengths chosen to sit on both sides of every head-width boundary the encoding has. A generated
//! corpus is the right tool for the *property* tests in `tests/canonical_encoding.rs`, which walk
//! four thousand cases; it is the wrong tool for a document somebody reads.
//!
//! Behind the `vectors` feature, which is on by default. `--no-default-features` gives a dependent
//! crate the types without this corpus — and turns the oracle off with it, which is why
//! `tests/canonical_encoding.rs`, which is not gated, asserts that `vectors` is still a *default*
//! feature. Without that assertion, dropping one word from `Cargo.toml` would compile the whole
//! comparison out of `cargo nextest run --workspace` and leave it passing.

use std::collections::BTreeMap;

use crate::actor::{Hlc, Signature};
use crate::canonical::{
    canonical_digest, encode_canonical, CanonicalEncode, CanonicalType, CanonicalValue,
    FieldSchema, RecordSchema, SCHEMA_FORMAT,
};
use crate::cbor::CBOR_PROFILE;
use crate::changeset::{ActorSequence, CausalParents, ChangeSet, ChangeSetDraft, PolicyEpoch};
use crate::digest::{derive_id, Blake3, ContentDigest, Digest32};
use crate::entity_id::{ObjectId, SessionId, WorkspaceId};
use crate::json::{hex, Json};
use crate::manifest::{ChunkRef, FileManifest};
use crate::object::{
    DirectoryEntry, DirectoryVersion, FileVersion, NormalizedName, PortableMetadata,
};
use crate::record_id::{ActorId, ChangeSetId, HeadId, ManifestId, VersionId};

/// The version of the published vector format.
pub const VECTOR_FORMAT: &str = "mesh-canonical-vectors/0";

/// The directory the vector files live in, relative to the repository root.
const VECTOR_DIR: &str = "protocol/test-vectors/v0";

/// The published schema file, relative to the repository root.
const SCHEMA_PATH: &str = "protocol/schemas/canonical-encoding-v0.json";

/// One published file: where it belongs and exactly what it contains.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishedDocument {
    /// The path relative to the repository root.
    pub path: String,
    /// The complete file content, ending in one newline.
    pub content: String,
}

/// Every published file, in a fixed order.
///
/// The order is stable so that a diff of this function is a diff of the corpus.
#[must_use]
pub fn published_documents() -> Vec<PublishedDocument> {
    let files = vector_files();

    let mut documents = vec![PublishedDocument {
        path: SCHEMA_PATH.to_owned(),
        content: schema_document(&files).to_document(),
    }];
    documents.push(PublishedDocument {
        path: format!("{VECTOR_DIR}/index.json"),
        content: index_document(&files).to_document(),
    });
    for file in &files {
        documents.push(PublishedDocument {
            path: format!("{VECTOR_DIR}/{}.json", file.slug),
            content: file.document().to_document(),
        });
    }
    documents
}

// ---------------------------------------------------------------------------------------------
// The corpus
// ---------------------------------------------------------------------------------------------

/// One record type's published file.
struct VectorFile {
    /// The file's basename, without the extension.
    slug: &'static str,
    /// The record type's published schema.
    schema: RecordSchema,
    /// Whether a signature is ever made over this record type's encoding directly.
    signed_record: bool,
    /// What the record is, for a reader who has never seen it.
    summary: &'static str,
    /// The vectors, each already rendered.
    vectors: Vec<Json>,
}

impl VectorFile {
    fn document(&self) -> Json {
        Json::object([
            ("vector_format", Json::string(VECTOR_FORMAT)),
            ("encoding_profile", Json::string(CBOR_PROFILE)),
            ("record", Json::string(self.schema.domain.as_str())),
            ("signed_record", Json::Bool(self.signed_record)),
            ("summary", Json::string(self.summary)),
            ("schema", schema_json(&self.schema)),
            ("vectors", Json::array(self.vectors.clone())),
        ])
    }
}

/// Every record type's vector file, in publication order.
fn vector_files() -> Vec<VectorFile> {
    vec![
        file_manifest_file(),
        file_version_file(),
        directory_version_file(),
        changeset_file(),
        empty_operation_file(),
    ]
}

/// A digest an external implementer can recompute: BLAKE3 of a short ASCII string.
fn digest_of(input: &str) -> Digest32 {
    Blake3::digest_bytes(input.as_bytes())
}

/// A fixed 32-byte pattern, for a value whose provenance does not matter.
fn pattern(byte: u8) -> Digest32 {
    Digest32::from_bytes([byte; 32])
}

fn file_manifest_file() -> VectorFile {
    let empty = FileManifest::new(0, digest_of(""), Vec::new());

    let single = FileManifest::new(
        12,
        digest_of("hello, mesh\n"),
        vec![ChunkRef::new(digest_of("hello, mesh\n"), 0, 12)],
    );

    // 23, 24 and 256 sit on both sides of the one-, two- and three-byte head boundaries, so this
    // vector fails if the head-width rule is implemented off by one anywhere.
    let boundaries = FileManifest::new(
        303,
        digest_of("boundaries"),
        vec![
            ChunkRef::new(pattern(0x11), 0, 23),
            ChunkRef::new(pattern(0x22), 23, 24),
            ChunkRef::new(pattern(0x33), 47, 256),
        ],
    );

    VectorFile {
        slug: "file-manifest",
        schema: FileManifest::SCHEMA,
        signed_record: true,
        summary: "The ordered chunk references that reconstruct one file version's bytes exactly.",
        vectors: vec![
            vector(
                "empty-file",
                "A zero-length file: no chunks, and the content hash of the empty byte string.",
                &empty,
                Some(derive_id::<Blake3, _>(&empty).digest().to_hex()),
            ),
            vector(
                "single-chunk",
                "One chunk covering the whole file. Both digests are BLAKE3 of the ASCII text \
                 \"hello, mesh\\n\", so an implementer can recompute them.",
                &single,
                Some(derive_id::<Blake3, _>(&single).digest().to_hex()),
            ),
            vector(
                "three-chunks-across-head-widths",
                "Chunk lengths 23, 24 and 256 straddle the one-, two- and three-byte integer head \
                 boundaries; the byte offsets do the same.",
                &boundaries,
                Some(derive_id::<Blake3, _>(&boundaries).digest().to_hex()),
            ),
        ],
    }
}

fn file_version_file() -> VectorFile {
    let first = FileVersion::new(
        ObjectId::mint(0x0000_0189_ABCD_EF01, [0x10; 10]),
        Vec::new(),
        ManifestId::from_digest(digest_of("hello, mesh\n")),
        PortableMetadata::new(false),
        ChangeSetId::from_digest(pattern(0xc5)),
    );

    let merge = FileVersion::new(
        ObjectId::mint(0x0000_0189_ABCD_EF01, [0x10; 10]),
        vec![
            VersionId::from_digest(pattern(0xa1)),
            VersionId::from_digest(pattern(0xa2)),
        ],
        ManifestId::from_digest(digest_of("merged")),
        PortableMetadata::new(true),
        ChangeSetId::from_digest(pattern(0xc6)),
    );

    VectorFile {
        slug: "file-version",
        schema: FileVersion::SCHEMA,
        signed_record: true,
        summary: "One immutable version of one file object.",
        vectors: vec![
            vector(
                "first-version",
                "The first version of an object: no parent versions, not executable.",
                &first,
                Some(derive_id::<Blake3, _>(&first).digest().to_hex()),
            ),
            vector(
                "merge-of-two-parents",
                "A version produced by a merge, carrying two parents and the executable bit.",
                &merge,
                Some(derive_id::<Blake3, _>(&merge).digest().to_hex()),
            ),
        ],
    }
}

fn directory_version_file() -> VectorFile {
    let object = ObjectId::mint(0x0000_0189_ABCD_EF02, [0x20; 10]);
    let empty = DirectoryVersion::empty(object);

    let mut entries = BTreeMap::new();
    // Inserted out of order on purpose: the encoding is in ascending UTF-8 byte order, which is
    // "README.md" (0x52), then "caf\u{e9}.txt" (0x63), then "\u{6c34}.txt" (0xe6). A vector whose
    // input order and output order agree would not show that the sort happens.
    for (name, object_byte, version_byte) in [
        ("\u{6c34}.txt", 0x33u8, 0xd3u8),
        ("README.md", 0x31, 0xd1),
        ("caf\u{e9}.txt", 0x32, 0xd2),
    ] {
        entries.insert(
            NormalizedName::new(name).expect("the fixture names are structurally valid"),
            DirectoryEntry::new(
                ObjectId::mint(u64::from(object_byte), [object_byte; 10]),
                VersionId::from_digest(pattern(version_byte)),
            ),
        );
    }
    let populated = DirectoryVersion::new(object, entries);

    VectorFile {
        slug: "directory-version",
        schema: DirectoryVersion::SCHEMA,
        signed_record: true,
        summary: "One immutable version of one directory object: its name-to-child bindings.",
        vectors: vec![
            vector(
                "empty-directory",
                "A directory version with no entries.",
                &empty,
                Some(derive_id::<Blake3, _>(&empty).digest().to_hex()),
            ),
            vector(
                "three-entries-sorted-by-utf8-bytes",
                "Three entries, two of them non-ASCII, in ascending byte-lexicographic order of \
                 the UTF-8 name. The insertion order in the generator is deliberately different.",
                &populated,
                Some(derive_id::<Blake3, _>(&populated).digest().to_hex()),
            ),
        ],
    }
}

/// A ChangeSet over the empty operation vocabulary.
fn changeset(parents: CausalParents, operations: Vec<()>, sequence: u64) -> ChangeSet<()> {
    ChangeSetDraft::<()>::new(
        WorkspaceId::mint(0x0000_0189_ABCD_EF00, [0x01; 10]),
        ActorId::from_digest(digest_of("actor-key")),
        SessionId::mint(0x0000_0189_ABCD_EF03, [0x02; 10]),
        ActorSequence::new(sequence),
        Hlc::new(1_700_000_000_000, 3),
    )
    .causal_parents(parents)
    .base_head(HeadId::from_digest(pattern(0xb0)))
    .policy_epoch(PolicyEpoch::new(7))
    .seal(
        operations,
        HeadId::from_digest(pattern(0xb1)),
        // The signature is not a bound field, so its value cannot move any byte below. It is set
        // to a recognizable pattern precisely so that a vector regenerated with a different one
        // still matches.
        Signature::from_bytes([0xee; 64]),
    )
}

fn changeset_file() -> VectorFile {
    let genesis = changeset(CausalParents::genesis(), Vec::new(), 1);
    let merge = changeset(
        CausalParents::after(
            ChangeSetId::from_digest(pattern(0xc1)),
            vec![ChangeSetId::from_digest(pattern(0xc2))],
        ),
        vec![(), ()],
        2,
    );

    VectorFile {
        slug: "changeset",
        schema: ChangeSet::<()>::SCHEMA,
        signed_record: true,
        summary: "One authored transition. The encoding binds ten fields and never the signature, \
                  because the signature is made over the record.",
        vectors: vec![
            vector(
                "genesis-no-operations",
                "The first ChangeSet in a workspace: no causal parents, no operations.",
                &genesis,
                Some(derive_id::<Blake3, _>(&genesis).digest().to_hex()),
            ),
            vector(
                "merge-two-parents-two-operations",
                "Two causal parents and two operations. The operations are the empty operation, \
                 so this vector also pins how a nested operation encoding is spliced in.",
                &merge,
                Some(derive_id::<Blake3, _>(&merge).digest().to_hex()),
            ),
        ],
    }
}

fn empty_operation_file() -> VectorFile {
    VectorFile {
        slug: "empty-operation",
        schema: <() as CanonicalEncode>::SCHEMA,
        signed_record: false,
        summary: "The placeholder operation, carrying no field. It is not itself signed; it \
                  appears inside a ChangeSet's operations sequence, and mesh-operations replaces \
                  it with the real vocabulary and publishes vectors for each member.",
        vectors: vec![vector(
            "the-only-value",
            "The empty operation has exactly one encoding: a one-element array holding its domain \
             tag.",
            &(),
            None,
        )],
    }
}

// ---------------------------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------------------------

/// One vector: the record's fields as data, its bytes, and the digests over them.
fn vector<R: CanonicalEncode>(
    name: &str,
    description: &str,
    record: &R,
    record_id_hex: Option<String>,
) -> Json {
    let encoding = encode_canonical(record);
    let mut members = vec![
        ("name".to_owned(), Json::string(name)),
        ("description".to_owned(), Json::string(description)),
        (
            "record".to_owned(),
            render_fields(R::SCHEMA.fields, &record.canonical_fields()),
        ),
        (
            "canonical_encoding_hex".to_owned(),
            Json::string(hex(&encoding)),
        ),
        (
            "canonical_encoding_length".to_owned(),
            Json::Unsigned(encoding.len() as u64),
        ),
        (
            "canonical_encoding_digest_hex".to_owned(),
            Json::string(canonical_digest::<Blake3, _>(record).to_hex()),
        ),
    ];
    if let Some(record_id_hex) = record_id_hex {
        members.push(("record_id_hex".to_owned(), Json::string(record_id_hex)));
    }
    Json::Object(members)
}

/// The record's fields as a named object, so a reader can rebuild the input without reading Rust.
fn render_fields(fields: &[FieldSchema], values: &[CanonicalValue]) -> Json {
    Json::Object(
        fields
            .iter()
            .zip(values)
            .map(|(field, value)| (field.name.to_owned(), render_value(field.ty, value)))
            .collect(),
    )
}

/// One field value, rendered under its declared type. Byte strings become lowercase hex.
fn render_value(ty: CanonicalType, value: &CanonicalValue) -> Json {
    match (ty, value) {
        (_, CanonicalValue::Unsigned(number)) => Json::Unsigned(*number),
        (_, CanonicalValue::Bool(flag)) => Json::Bool(*flag),
        (_, CanonicalValue::Bytes(bytes)) => Json::string(hex(bytes)),
        (_, CanonicalValue::Text(text)) => Json::string(text.clone()),
        (_, CanonicalValue::Record(encoding)) => Json::string(hex(encoding)),
        (CanonicalType::Sequence(element), CanonicalValue::Sequence(items)) => {
            Json::array(items.iter().map(|item| render_value(*element, item)))
        }
        (CanonicalType::Group(fields), CanonicalValue::Group(items)) => {
            render_fields(fields, items)
        }
        // Unreachable while the schema and the values agree, which
        // `tests/canonical_encoding.rs` checks over the whole generated corpus. Rendered rather
        // than panicked so that a mismatch shows up in a diff instead of aborting the generator.
        (_, CanonicalValue::Sequence(items) | CanonicalValue::Group(items)) => Json::array(
            items
                .iter()
                .map(|item| render_value(CanonicalType::Text, item)),
        ),
    }
}

/// A record schema as published: the domain tag and the ordered fields.
fn schema_json(schema: &RecordSchema) -> Json {
    Json::object([
        ("domain_tag", Json::string(schema.domain.as_str())),
        ("fields", Json::array(schema.fields.iter().map(field_json))),
    ])
}

/// One field, with enough of its type to decode it.
fn field_json(field: &FieldSchema) -> Json {
    let mut members = vec![
        ("name".to_owned(), Json::string(field.name)),
        ("type".to_owned(), Json::string(field.ty.name())),
    ];
    match field.ty {
        CanonicalType::Bytes(Some(width)) => {
            members.push(("byte_length".to_owned(), Json::Unsigned(u64::from(width))));
        }
        CanonicalType::Sequence(element) => {
            members.push((
                "element".to_owned(),
                field_json(&FieldSchema::new("element", *element)),
            ));
        }
        CanonicalType::Group(fields) => {
            members.push((
                "fields".to_owned(),
                Json::array(fields.iter().map(field_json)),
            ));
        }
        _ => {}
    }
    Json::Object(members)
}

/// The profile description: the five admitted shapes and the rules that close the encoding.
fn profile_json() -> Json {
    Json::object([
        ("name", Json::string(CBOR_PROFILE)),
        ("base", Json::string("RFC 8949 (CBOR)")),
        (
            "admitted_major_types",
            Json::array([
                shape(
                    "unsigned",
                    0,
                    "An unsigned integer, in the shortest head that holds it.",
                ),
                shape("bytes", 2, "A definite-length byte string."),
                shape("text", 3, "A definite-length UTF-8 text string."),
                shape("array", 4, "A definite-length array."),
                shape(
                    "bool",
                    7,
                    "0xf4 for false and 0xf5 for true, and no other simple value.",
                ),
            ]),
        ),
        (
            "excluded",
            Json::array(
                [
                    "negative integers",
                    "maps",
                    "tags",
                    "floating point",
                    "null and undefined",
                    "indefinite-length items",
                    "any simple value other than true and false",
                ]
                .into_iter()
                .map(Json::string),
            ),
        ),
        (
            "rules",
            Json::array(
                [
                    "Every head uses the shortest form: an argument below 24 rides in the head \
                     byte, then one, two, four and eight big-endian bytes.",
                    "A record encodes as an array whose first element is the record's domain tag \
                     and whose remaining elements are its fields in schema order, one element per \
                     field.",
                    "Field order is the schema's; the bytes carry position, never a field name.",
                    "There is no optional field. A field that may be absent is a sequence of at \
                     most one element: [] for absent, [value] for present.",
                    "A sequence of groups that models a keyed collection is in ascending \
                     byte-lexicographic order of its key's UTF-8 bytes.",
                    "A nested record is spliced in whole and carries its own domain tag.",
                ]
                .into_iter()
                .map(Json::string),
            ),
        ),
    ])
}

fn shape(name: &str, major_type: u64, rule: &str) -> Json {
    Json::object([
        ("name", Json::string(name)),
        ("cbor_major_type", Json::Unsigned(major_type)),
        ("rule", Json::string(rule)),
    ])
}

/// The published schema document: the profile, then every record schema.
fn schema_document(files: &[VectorFile]) -> Json {
    Json::object([
        ("schema_format", Json::string(SCHEMA_FORMAT)),
        ("encoding_profile", profile_json()),
        (
            "records",
            Json::array(files.iter().map(|file| {
                Json::object([
                    ("record", Json::string(file.schema.domain.as_str())),
                    ("signed_record", Json::Bool(file.signed_record)),
                    ("summary", Json::string(file.summary)),
                    ("schema", schema_json(&file.schema)),
                ])
            })),
        ),
        (
            "digest",
            Json::object([
                ("algorithm", Json::string("BLAKE3")),
                ("output_bytes", Json::Unsigned(32)),
                (
                    "canonical_encoding_digest",
                    Json::string(
                        "BLAKE3 of the canonical encoding bytes. This is the digest a signature \
                         over a record's bytes covers, and it is also the record's name.",
                    ),
                ),
                (
                    "record_id",
                    Json::string(
                        "A record's name: BLAKE3 of its canonical encoding, which is the same \
                         value as canonical_encoding_digest. A chunk has no schema and is named \
                         by BLAKE3 of its own bytes instead; those are the only two rules. Ruled \
                         in docs/adr/0033-name-an-immutable-record-by-the-digest-of-its-canonical-encoding.md. \
                         The values published TODAY are still the retired identity framing's \
                         output and do NOT equal canonical_encoding_digest: use \
                         canonical_encoding_digest as the name until task \
                         01KZFMZC4MTHTT3BW4Y0BW6NYA moves the implementation onto the rule, which \
                         makes the two equal without moving any encoded byte.",
                    ),
                ),
            ]),
        ),
    ])
}

/// The index: what a harness enumerates before reading anything.
fn index_document(files: &[VectorFile]) -> Json {
    Json::object([
        ("vector_format", Json::string(VECTOR_FORMAT)),
        ("encoding_profile", Json::string(CBOR_PROFILE)),
        ("schema", Json::string(SCHEMA_PATH)),
        (
            "files",
            Json::array(files.iter().map(|file| {
                Json::object([
                    ("record", Json::string(file.schema.domain.as_str())),
                    ("file", Json::string(format!("{}.json", file.slug))),
                    ("signed_record", Json::Bool(file.signed_record)),
                    ("vector_count", Json::Unsigned(file.vectors.len() as u64)),
                ])
            })),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_corpus_covers_every_published_record_type() {
        let slugs: Vec<&str> = vector_files().iter().map(|file| file.slug).collect();
        assert_eq!(
            slugs,
            vec![
                "file-manifest",
                "file-version",
                "directory-version",
                "changeset",
                "empty-operation",
            ]
        );
    }

    #[test]
    fn every_document_has_a_path_and_ends_in_one_newline() {
        let documents = published_documents();
        assert_eq!(documents.len(), 7, "the schema, the index and five records");
        for document in &documents {
            assert!(document.path.starts_with("protocol/"), "{}", document.path);
            assert!(document.content.ends_with('\n'), "{}", document.path);
            assert!(!document.content.ends_with("\n\n"), "{}", document.path);
        }
    }

    #[test]
    fn generating_the_corpus_twice_produces_identical_text() {
        assert_eq!(published_documents(), published_documents());
    }

    #[test]
    fn every_vector_carries_bytes_a_digest_and_a_length() {
        for file in vector_files() {
            assert!(!file.vectors.is_empty(), "{} has no vector", file.slug);
            for entry in &file.vectors {
                let Json::Object(members) = entry else {
                    panic!("a vector is an object");
                };
                let keys: Vec<&str> = members.iter().map(|(key, _)| key.as_str()).collect();
                for required in [
                    "name",
                    "description",
                    "record",
                    "canonical_encoding_hex",
                    "canonical_encoding_length",
                    "canonical_encoding_digest_hex",
                ] {
                    assert!(keys.contains(&required), "{} lacks {required}", file.slug);
                }
                assert_eq!(
                    keys.contains(&"record_id_hex"),
                    file.signed_record,
                    "{}: only a signed record publishes a record id",
                    file.slug
                );
            }
        }
    }

    /// The published length must be the length of the published bytes, or an implementer checking
    /// one against the other finds a corpus that contradicts itself.
    #[test]
    fn the_published_length_matches_the_published_bytes() {
        for file in vector_files() {
            for entry in &file.vectors {
                let Json::Object(members) = entry else {
                    panic!("a vector is an object");
                };
                let find = |key: &str| {
                    members
                        .iter()
                        .find(|(name, _)| name == key)
                        .map(|(_, value)| value.clone())
                        .expect("the key is present")
                };
                let (Json::String(bytes), Json::Unsigned(length)) = (
                    find("canonical_encoding_hex"),
                    find("canonical_encoding_length"),
                ) else {
                    panic!("the bytes are a string and the length a number");
                };
                assert_eq!(bytes.len() as u64, length * 2, "{}", file.slug);
            }
        }
    }

    #[test]
    fn a_digest_in_the_corpus_is_recomputable_from_its_input() {
        assert_eq!(
            digest_of("hello, mesh\n"),
            Blake3::digest_bytes(b"hello, mesh\n")
        );
        assert_eq!(pattern(0x11), Digest32::from_bytes([0x11; 32]));
    }
}
