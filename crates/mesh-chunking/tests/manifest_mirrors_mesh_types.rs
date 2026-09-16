//! The manifest shape here against the manifest shape in `mesh-types`.
//!
//! # What is at stake
//!
//! `mesh-types::FileManifest` is the type the rest of Mesh serializes, hashes and puts in the
//! database. This crate produces the values that go into it and cannot import it, for the reason
//! `crates/mesh-chunking/src/blake3.rs` records. A field added on one side and not the other is not
//! a compile error here — it is a manifest that round-trips through canonical encoding having
//! silently dropped something.
//!
//! So the two shapes are compared by reading `mesh-types`' source. This is a **structural** check,
//! not a semantic one: it verifies that both types declare the same fields in the same order with
//! the same types, and that both expose the same accessors. It cannot verify that the two mean the
//! same thing by `offset`. That is what the crate documentation is for, and the limit is stated
//! here rather than left to be assumed away.
//!
//! # How it fails
//!
//! By path, loudly, exactly as the BLAKE3 twin does: if `mesh-types/src/manifest.rs` moves, this
//! test fails rather than quietly checking nothing.

use std::fs;
use std::path::PathBuf;

fn mesh_types_manifest_source() -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "mesh-types",
        "src",
        "manifest.rs",
    ]
    .iter()
    .collect();
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). mesh-chunking mirrors mesh-types' manifest shape by reading \
             its source, because no dependency edge is permitted; if that file has moved, the \
             mirror is unchecked until this path is corrected.",
            path.display()
        )
    })
}

fn own_manifest_source() -> String {
    fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/manifest.rs"))
        .expect("this crate's own manifest module is readable")
}

/// The field declarations inside `pub struct <name> { .. }`, normalized.
fn struct_fields(source: &str, name: &str) -> Vec<String> {
    let needle = format!("pub struct {name} {{");
    let start = source
        .find(&needle)
        .unwrap_or_else(|| panic!("`pub struct {name}` is declared"))
        + needle.len();
    let body = &source[start..];
    let end = body.find("\n}").expect("the struct body is terminated");
    body[..end]
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .map(|line| line.trim_end_matches(',').replace(' ', ""))
        .collect()
}

#[test]
fn the_chunk_reference_has_the_same_fields_in_the_same_order() {
    let theirs = struct_fields(&mesh_types_manifest_source(), "ChunkRef");
    let ours = struct_fields(&own_manifest_source(), "ChunkRef");
    assert_eq!(
        theirs.len(),
        3,
        "mesh-types' ChunkRef has {} fields, not the three this test was written against",
        theirs.len()
    );
    assert_eq!(
        ours, theirs,
        "the two ChunkRef shapes have diverged. A manifest produced here would no longer map onto \
         the type mesh-types serializes."
    );
}

#[test]
fn the_file_manifest_has_the_same_fields_in_the_same_order() {
    let theirs = struct_fields(&mesh_types_manifest_source(), "FileManifest");
    let ours = struct_fields(&own_manifest_source(), "FileManifest");
    assert_eq!(
        theirs.len(),
        3,
        "mesh-types' FileManifest has {} fields, not the three this test was written against",
        theirs.len()
    );
    assert_eq!(
        ours, theirs,
        "the two FileManifest shapes have diverged. A manifest produced here would no longer map \
         onto the type mesh-types serializes."
    );
}

#[test]
fn every_accessor_mesh_types_exposes_exists_here() {
    let theirs = mesh_types_manifest_source();
    let ours = own_manifest_source();

    let mut found = 0usize;
    for line in theirs.lines().map(str::trim) {
        let Some(rest) = line
            .strip_prefix("pub const fn ")
            .or(line.strip_prefix("pub fn "))
        else {
            continue;
        };
        let Some(name) = rest.split('(').next() else {
            continue;
        };
        // `absorb` and `canonical_fields` are the serialization half, which this crate
        // deliberately does not mirror; the module header says so.
        if matches!(name, "absorb" | "canonical_fields") {
            continue;
        }
        found += 1;
        assert!(
            ours.contains(&format!("fn {name}(")),
            "mesh-types' manifest exposes `{name}` and this crate's mirror does not"
        );
    }
    assert!(
        found >= 8,
        "only {found} accessors were found in mesh-types' manifest; the parser is broken and a \
         passing run would prove nothing"
    );
}
