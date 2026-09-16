//! The compatibility oracle: an encoding change without a vector update fails here.
//!
//! This is the acceptance criterion "an encoding change without a corresponding vector update
//! fails CI" as a mechanism. `mesh_types::published_documents()` produces the **complete text** of
//! every file under `protocol/schemas/` and `protocol/test-vectors/`, and the tests below compare
//! that text to the working tree in both directions:
//!
//! * every generated document matches the file on disk, byte for byte;
//! * every file on disk is generated — nothing hand-written, nothing orphaned.
//!
//! Change a field order, a domain tag, a head width or an integer's framing and every affected
//! vector's bytes move, so the first comparison fails. Delete or hand-edit a vector and the same
//! comparison fails. Add a signed record type and forget its vectors, and the coverage check
//! below fails. There is no path through this file that lets the encoding and the published corpus
//! disagree.
//!
//! **The oracle is only worth having if it can fail, so it is shown failing.** The transcripts —
//! one field order reversed, one published byte edited, one vector file deleted, one new
//! `CanonicalRecord` added — are in this task's PR body, produced by making each change and
//! running this file.
//!
//! Regenerating after a deliberate encoding change:
//!
//! ```console
//! $ cargo test -p mesh-types --test serialization-compat -- --ignored write_published_documents
//! ```
//!
//! Under the task's failure-and-recovery clause a vector that has to change is a **compatibility
//! event**: the historical vector is retained under its own format version, never deleted, because
//! signatures were made over the bytes it holds. `protocol/test-vectors/v0/` is that version
//! directory, and a change to the encoding produces `v1/` beside it rather than edits inside it.
//!
//! Behind the `vectors` feature, which is on by default: the published corpus is what this file
//! compares against, and it does not exist without it. Cold verification found that without this
//! gate `cargo test -p mesh-types --no-default-features` did not compile at all, which is a legal
//! feature combination the crate is supposed to support. That the feature is still a *default* —
//! and so that this oracle still runs under `npm test` — is asserted from
//! `tests/canonical_encoding.rs`, which is not gated and therefore cannot be switched off with it.

#![cfg(feature = "vectors")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mesh_types::{published_documents, PublishedDocument};

/// The repository root, from this crate's manifest directory.
fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("the crate sits two directories below the repository root")
}

/// Every published path, relative to the repository root.
fn published_paths() -> BTreeSet<String> {
    published_documents()
        .into_iter()
        .map(|document| document.path)
        .collect()
}

/// Every file the generator owns that exists on disk today, relative to the repository root.
///
/// Walks the owned directories rather than trusting the generator's own list, which is what makes
/// an orphaned file visible. **Recursively** — a non-recursive walk would have missed a stray file
/// one directory down, and the whole job of this function is to see what the generator does not
/// know about.
fn files_on_disk() -> BTreeSet<String> {
    let root = repository_root();
    let mut found = BTreeSet::new();
    for directory in ["protocol/test-vectors", "protocol/schemas"] {
        collect_files(&root, directory, &mut found);
    }
    found
}

/// Add every file under `directory`, depth-first, to `found`.
fn collect_files(root: &Path, directory: &str, found: &mut BTreeSet<String>) {
    let absolute = root.join(directory);
    if !absolute.exists() {
        return;
    }
    for entry in std::fs::read_dir(&absolute).expect("the directory is readable") {
        let entry = entry.expect("the entry is readable");
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = format!("{directory}/{name}");
        let file_type = entry.file_type().expect("the type is readable");
        if file_type.is_dir() {
            collect_files(root, &relative, found);
        } else if file_type.is_file() && name != "README.md" {
            // `README.md` is prose for a human and is not generated. Everything else under these
            // directories is the generator's.
            found.insert(relative);
        }
    }
}

/// **The criterion.** Every published document matches the working tree exactly.
#[test]
fn every_published_document_matches_the_working_tree() {
    let root = repository_root();
    for PublishedDocument { path, content } in published_documents() {
        let absolute = root.join(&path);
        let on_disk = std::fs::read_to_string(&absolute).unwrap_or_else(|error| {
            panic!(
                "{path} is missing or unreadable ({error}). The encoding produced a document the \
                 published corpus does not have. Regenerate with:\n  cargo test -p mesh-types \
                 --test serialization-compat -- --ignored write_published_documents"
            )
        });
        assert!(
            on_disk == content,
            "\n{path} does not match what the encoder produces.\n{}\n\
             This is an encoding change without a vector update. If the change was deliberate it \
             is a COMPATIBILITY EVENT: publish the new bytes under a new format version and keep \
             the old directory, because signatures were made over it. Regenerate with:\n  cargo \
             test -p mesh-types --test serialization-compat -- --ignored \
             write_published_documents\n",
            first_difference(&on_disk, &content)
        );
    }
}

/// The first line at which two documents differ, with a little context.
///
/// A published document is several hundred lines, and `assert_eq!` on two of them prints both in
/// full — sixteen kilobytes of escaped JSON in which the one changed character is invisible. That
/// is a failure message that makes a lane guess, so it is not the one this test produces. Found by
/// running the mutation rather than by imagining it.
fn first_difference(on_disk: &str, generated: &str) -> String {
    let mut report = String::new();
    let on_disk_lines: Vec<&str> = on_disk.lines().collect();
    let generated_lines: Vec<&str> = generated.lines().collect();

    for index in 0..on_disk_lines.len().max(generated_lines.len()) {
        let left = on_disk_lines.get(index);
        let right = generated_lines.get(index);
        if left == right {
            continue;
        }
        report.push_str(&format!("  first difference at line {}\n", index + 1));
        report.push_str(&format!(
            "    on disk:   {}\n",
            left.map_or("<end of file>", |line| line.trim())
        ));
        report.push_str(&format!(
            "    generated: {}\n",
            right.map_or("<end of file>", |line| line.trim())
        ));
        let extra = on_disk_lines.len().abs_diff(generated_lines.len());
        if extra > 0 {
            report.push_str(&format!("    ({extra} more line(s) differ in length)\n"));
        }
        return report;
    }
    "  the documents differ only in trailing bytes".to_owned()
}

/// The other direction: nothing on disk that the generator does not produce.
///
/// Without this, a vector file could be hand-edited into existence, or a file left behind after a
/// record type was renamed, and every other check here would stay green while the corpus lied.
#[test]
fn no_published_file_is_unaccounted_for() {
    let generated = published_paths();
    let on_disk = files_on_disk();

    let orphans: Vec<&String> = on_disk.difference(&generated).collect();
    assert!(
        orphans.is_empty(),
        "these files are in the published directories but are not produced by the generator: \
         {orphans:?}"
    );

    let missing: Vec<&String> = generated.difference(&on_disk).collect();
    assert!(
        missing.is_empty(),
        "the generator produces these files and they are not on disk: {missing:?}"
    );
}

/// Every record type this crate names by a record identifier, and the vector file it publishes.
///
/// A record type reaches this table only by being written into it, and it must be in it: the two
/// tests below fail from opposite sides, so neither adding a signed record type without vectors
/// nor listing a type whose vectors are absent can pass.
const COVERAGE: [(&str, &str); 4] = [
    ("ChangeSet<Op>", "changeset"),
    ("DirectoryVersion", "directory-version"),
    ("FileManifest", "file-manifest"),
    ("FileVersion", "file-version"),
];

/// A new `impl CanonicalRecord` has to be declared here before it can pass.
///
/// The comparison is against the crate's own source, so this fails the moment a fifth record type
/// appears — which is the point at which somebody has to decide what its vectors are.
#[test]
fn every_canonical_record_type_is_covered() {
    let declared = canonical_record_types();
    assert!(
        !declared.is_empty(),
        "no `impl CanonicalRecord for` was found: the scan has stopped working, and a scan that \
         matches nothing passes every coverage check ever written on it"
    );
    let covered: BTreeSet<String> = COVERAGE
        .iter()
        .map(|(record_type, _)| (*record_type).to_owned())
        .collect();
    assert_eq!(
        declared, covered,
        "the record types this crate declares and the ones this file covers have drifted. A \
         signed record type with no published vector cannot be verified by an implementation that \
         is not this one."
    );
}

/// Every covered record type has a vector file, and that file holds at least one vector.
#[test]
fn every_covered_record_type_has_published_vectors() {
    let published = published_paths();
    for (record_type, slug) in COVERAGE {
        let path = format!("protocol/test-vectors/v0/{slug}.json");
        assert!(
            published.contains(&path),
            "{record_type} has no vector file at {path}"
        );
        let document = published_documents()
            .into_iter()
            .find(|document| document.path == path)
            .expect("the path is published");
        assert!(
            document.content.contains("\"canonical_encoding_hex\""),
            "{path} carries no encoded bytes"
        );
        assert!(
            document
                .content
                .contains("\"canonical_encoding_digest_hex\""),
            "{path} carries no digest"
        );
    }
}

/// The record types the crate declares, read from its source.
///
/// Text, not types: Rust has no reflection over trait implementations. The scan is narrow on
/// purpose — a line that begins with `impl` and carries `CanonicalRecord for ` — so the same words
/// inside a doc comment are not counted.
///
/// The marker deliberately carries no leading space. The first version required one, and running
/// the mutation showed why that was wrong: `impl crate::digest::CanonicalRecord for X` has `::`
/// there, so a fully-qualified implementation added anywhere in the crate was invisible and the
/// coverage check passed with a signed record type that had no vectors.
fn canonical_record_types() -> BTreeSet<String> {
    const MARKER: &str = "CanonicalRecord for ";

    let mut found = BTreeSet::new();
    for (_, source) in crate_sources() {
        for line in source.lines() {
            let line = line.trim();
            if !line.starts_with("impl") {
                continue;
            }
            let Some(at) = line.find(MARKER) else {
                continue;
            };
            found.insert(
                line[at + MARKER.len()..]
                    .trim_end_matches('{')
                    .trim()
                    .to_owned(),
            );
        }
    }
    found
}

/// Every `.rs` file under `src/`, by name and content.
fn crate_sources() -> Vec<(String, String)> {
    let source_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = Vec::new();
    for entry in std::fs::read_dir(&source_dir).expect("the source directory is readable") {
        let entry = entry.expect("the entry is readable");
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".rs") {
            sources.push((
                name,
                std::fs::read_to_string(entry.path()).expect("the source is readable"),
            ));
        }
    }
    sources.sort();
    sources
}

/// The scan reads the source directory rather than a checked-in list, so a module added later is
/// scanned without anybody remembering to add it. This asserts the directory is actually being
/// found — an empty or missing directory would make every scan above vacuous.
#[test]
fn the_scan_reads_the_whole_source_directory() {
    let sources = crate_sources();
    assert!(
        sources.len() >= 10,
        "only {} source files were found; the scan is not reading the crate",
        sources.len()
    );
    assert!(
        sources.iter().any(|(name, _)| name == "lib.rs"),
        "lib.rs was not among the scanned sources"
    );
}

// ---------------------------------------------------------------------------------------------
// The protobuf projection must describe the same records
// ---------------------------------------------------------------------------------------------

/// Which protobuf message projects which canonical record, and in what field order.
///
/// Plan §8.1 puts protobuf on the network and canonical CBOR under the signature, which means one
/// record has two descriptions. Two descriptions nothing compares are two descriptions that
/// disagree — a field added to the wire message and not to the signed encoding is a field a peer
/// sends, a verifier ignores, and nobody notices until a signature covers less than it appears to.
const PROTO_RECORDS: [(&str, &str); 5] = [
    ("FileManifest", "file-manifest"),
    ("FileVersion", "file-version"),
    ("DirectoryVersion", "directory-version"),
    ("ChangeSet", "changeset"),
    ("EmptyOperation", "empty-operation"),
];

/// The compound-element messages, which are not records and so carry no schema of their own.
///
/// Written out rather than derived: a group is a field's type inside a schema, and mapping it to a
/// message name is a naming decision this table records.
const PROTO_GROUPS: [(&str, &[&str]); 4] = [
    ("ChunkRef", &["content_hash", "offset", "length"]),
    ("PortableMetadata", &["executable"]),
    ("DirectoryEntry", &["name", "object_id", "version_id"]),
    ("Hlc", &["physical_millis", "logical"]),
];

/// Every record message's field names and order match the published canonical schema.
#[test]
fn the_protobuf_projection_matches_the_canonical_schemas() {
    let messages = proto_messages();
    for (message, slug) in PROTO_RECORDS {
        let fields = messages
            .iter()
            .find(|(name, _)| name == message)
            .map(|(_, fields)| fields.clone())
            .unwrap_or_else(|| panic!("protocol/proto/mesh/v0/records.proto has no {message}"));
        assert_eq!(
            fields,
            schema_field_names(slug),
            "{message} and the canonical schema for {slug} declare different fields, or the same \
             fields in a different order"
        );
    }
}

#[test]
fn every_group_message_is_declared_as_expected() {
    let messages = proto_messages();
    for (message, expected) in PROTO_GROUPS {
        let fields = messages
            .iter()
            .find(|(name, _)| name == message)
            .map(|(_, fields)| fields.clone())
            .unwrap_or_else(|| panic!("protocol/proto/mesh/v0/records.proto has no {message}"));
        assert_eq!(fields, expected.to_vec(), "{message} drifted");
    }
}

/// No message in the proto file is unaccounted for, so a message cannot be added there and
/// silently escape the comparison above.
#[test]
fn the_proto_declares_no_message_the_tests_do_not_know_about() {
    let declared: BTreeSet<String> = proto_messages().into_iter().map(|(name, _)| name).collect();
    let known: BTreeSet<String> = PROTO_RECORDS
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .chain(PROTO_GROUPS.iter().map(|(name, _)| (*name).to_owned()))
        .collect();
    assert_eq!(declared, known);
}

/// Field numbers are sequential from one, in declaration order. A gap or a repeat is either a
/// mistake or a reservation that has to be written as `reserved`.
#[test]
fn proto_field_numbers_are_sequential_from_one() {
    for (message, numbers) in proto_field_numbers() {
        let expected: Vec<u32> = (1..=numbers.len() as u32).collect();
        assert_eq!(numbers, expected, "{message} field numbers");
    }
}

/// The published field names of the record whose vector file is `slug`, in schema order.
fn schema_field_names(slug: &str) -> Vec<String> {
    let path = format!("protocol/test-vectors/v0/{slug}.json");
    let document = published_documents()
        .into_iter()
        .find(|document| document.path == path)
        .unwrap_or_else(|| panic!("{path} is not published"));

    // The vector file's `schema.fields` block, read by its shape: the published renderer emits one
    // `"name": "…"` per line at a fixed indent, and the schema block precedes `"vectors"`.
    let schema_block = document
        .content
        .split("\"vectors\"")
        .next()
        .expect("the schema precedes the vectors");
    schema_block
        .lines()
        .filter(|line| line.starts_with("        \"name\": "))
        .filter_map(|line| line.split('"').nth(3))
        .map(ToOwned::to_owned)
        .collect()
}

/// Every message in the proto file, with its field names in declaration order.
fn proto_messages() -> Vec<(String, Vec<String>)> {
    parse_proto()
        .into_iter()
        .map(|(name, fields)| (name, fields.into_iter().map(|(name, _)| name).collect()))
        .collect()
}

/// Every message in the proto file, with its field numbers in declaration order.
fn proto_field_numbers() -> Vec<(String, Vec<u32>)> {
    parse_proto()
        .into_iter()
        .map(|(name, fields)| (name, fields.into_iter().map(|(_, number)| number).collect()))
        .collect()
}

/// A deliberately small proto3 reader: `message X {` opens a message and `… name = n;` is a field.
///
/// Enough for a file this repository writes and no more. It is not a protobuf parser and must not
/// grow into one; if the projection ever needs nested messages, oneofs or imports, the right move
/// is a real parser in a tool that owns one, not more line matching here.
fn parse_proto() -> Vec<(String, Vec<(String, u32)>)> {
    let path = repository_root().join("protocol/proto/mesh/v0/records.proto");
    let source = std::fs::read_to_string(&path).expect("the proto file is readable");

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
            // `message X {}` opens and closes on one line.
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

/// The reader is only worth having if it can see. A file it parses to nothing would pass every
/// comparison above.
#[test]
fn the_proto_reader_finds_fields() {
    let messages = parse_proto();
    assert_eq!(messages.len(), 9, "five records and four group messages");
    let changeset = messages
        .iter()
        .find(|(name, _)| name == "ChangeSet")
        .expect("ChangeSet is declared");
    assert_eq!(changeset.1.len(), 10, "the ten bound fields");
    assert_eq!(changeset.1[0], ("workspace_id".to_owned(), 1));
    assert!(
        messages
            .iter()
            .any(|(name, fields)| name == "EmptyOperation" && fields.is_empty()),
        "a message with no field is still a message"
    );
}

/// Write the published corpus. Ignored, so it never runs as part of the suite — it is the
/// deliberate act of accepting an encoding change, not a step of verifying one.
#[test]
#[ignore = "writes the published corpus; run deliberately after an intended encoding change"]
fn write_published_documents() {
    let root = repository_root();
    for PublishedDocument { path, content } in published_documents() {
        let absolute = root.join(&path);
        if let Some(parent) = absolute.parent() {
            std::fs::create_dir_all(parent).expect("the directory can be created");
        }
        std::fs::write(&absolute, content).expect("the document can be written");
        println!("wrote {path}");
    }
}
