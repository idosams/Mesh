//! The schema is built against `mesh-types`' identifiers, and this is what keeps it that way.
//!
//! # Why a source-text lint and not a dependency
//!
//! `mesh-store` deliberately has no path dependency on `mesh-types`, so the schema mirrors its
//! identifier families by declaration ([`ColumnDomain::mesh_types_item`]) rather than by import.
//! A declaration nothing checks is a comment.
//!
//! This test reads `mesh-types`' own source and holds the two against each other in both
//! directions:
//!
//! * every name a column claims to mirror still exists in `mesh-types`, so a rename there turns
//!   this red rather than leaving a schema that mirrors a type that is gone;
//! * every record identifier `mesh-types` declares is either mirrored by a column or listed in
//!   [`UNINDEXED_MESH_TYPES_IDS`] with a reason, so a *new* record identifier cannot quietly go
//!   unindexed.
//!
//! # What it is not
//!
//! It is a lint over source text, exactly like `mesh-types`' own `no_ambient_io.rs`, and it has
//! that technique's limits: it reads two files by path and recognises two macro invocations. A
//! record identifier declared some other way, or those files moved, would be missed — and would
//! surface as this test failing to find anything, which the arity assertions below turn into a
//! failure rather than a silent pass.

use std::fs;
use std::path::PathBuf;

use mesh_store::{ColumnDomain, TABLES, UNINDEXED_MESH_TYPES_IDS};

fn mesh_types_source(file: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", "mesh-types", "src", file]
        .iter()
        .collect();
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). This test holds mesh-store's schema against mesh-types' \
             identifiers by reading its source, because no mesh-types edge is present; if the \
             file has moved, the schema's mesh_types_item declarations are unchecked until this \
             path is corrected.",
            path.display()
        )
    })
}

/// The names declared by `macro!(Name, …)` invocations in some source.
fn names_declared_by(source: &str, macro_name: &str) -> Vec<String> {
    let opener = format!("{macro_name}!(");
    let mut names = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find(&opener) {
        // Skip the macro's own definition, which is `macro_rules! <name>` and never `<name>!(`.
        let tail = &rest[at + opener.len()..];
        let identifier: String = tail
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !identifier.is_empty() {
            names.push(identifier);
        }
        rest = tail;
    }
    names.sort();
    names.dedup();
    names
}

fn record_identifiers() -> Vec<String> {
    names_declared_by(&mesh_types_source("record_id.rs"), "record_id")
}

fn entity_identifiers() -> Vec<String> {
    names_declared_by(&mesh_types_source("entity_id.rs"), "entity_id")
}

fn domains() -> Vec<ColumnDomain> {
    TABLES
        .iter()
        .flat_map(|table| table.columns.iter().map(|column| column.domain))
        .collect()
}

fn mirrored_items() -> Vec<String> {
    let mut items: Vec<String> = domains()
        .into_iter()
        .filter_map(|domain| domain.mesh_types_item.map(str::to_owned))
        .collect();
    items.sort();
    items.dedup();
    items
}

/// The lint has to find something, or every assertion below passes vacuously.
#[test]
fn mesh_types_declares_the_identifier_families_this_schema_mirrors() {
    let records = record_identifiers();
    let entities = entity_identifiers();
    println!("mesh-types record identifiers: {records:?}");
    println!("mesh-types entity identifiers: {entities:?}");
    assert!(
        records.len() >= 8,
        "found only {} record identifiers in mesh-types; the reader has stopped working and every \
         other assertion here is vacuous",
        records.len()
    );
    assert!(
        entities.len() >= 4,
        "found only {} entity identifiers in mesh-types",
        entities.len()
    );
    assert!(records.contains(&"ChangeSetId".to_owned()));
    assert!(entities.contains(&"SessionId".to_owned()));
}

/// Direction one: no column claims to mirror something `mesh-types` does not declare.
#[test]
fn every_mirrored_item_still_exists_in_mesh_types() {
    let mut known = record_identifiers();
    known.extend(entity_identifiers());
    for item in mirrored_items() {
        assert!(
            known.contains(&item),
            "a column declares it mirrors `{item}`, which mesh-types no longer declares. Either \
             the type was renamed, or the column's ColumnDomain is stale."
        );
    }
}

/// Direction two: no record identifier goes unindexed without somebody having decided so.
#[test]
fn every_record_identifier_is_either_mirrored_or_deliberately_unindexed() {
    let mirrored = mirrored_items();
    let excused: Vec<&str> = UNINDEXED_MESH_TYPES_IDS
        .iter()
        .map(|(name, _)| *name)
        .collect();

    for identifier in record_identifiers() {
        let is_mirrored = mirrored.contains(&identifier);
        let is_excused = excused.contains(&identifier.as_str());
        assert!(
            is_mirrored || is_excused,
            "mesh-types declares `{identifier}` and no column mirrors it. If the local index \
             should hold it, give a column that domain; if it should not, add it to \
             UNINDEXED_MESH_TYPES_IDS with the reason. Silence is the one option this test removes."
        );
        assert!(
            !(is_mirrored && is_excused),
            "`{identifier}` is both mirrored by a column and listed as unindexed"
        );
    }
}

/// An excuse with no reason is not an excuse, and an excuse for a type that no longer exists is a
/// stale one.
#[test]
fn every_unindexed_identifier_carries_a_real_reason() {
    let known = record_identifiers();
    for (identifier, reason) in UNINDEXED_MESH_TYPES_IDS {
        assert!(
            known.contains(&(*identifier).to_owned()),
            "`{identifier}` is listed as unindexed but mesh-types no longer declares it"
        );
        assert!(
            reason.len() > 30,
            "`{identifier}` is excused with {reason:?}, which is not a reason"
        );
    }
}

/// The reader must not mistake the macro's own definition for an invocation.
#[test]
fn the_reader_ignores_the_macro_definition_itself() {
    let source =
        "macro_rules! record_id {\n    ($name:ident) => {};\n}\nrecord_id!(RealOne, \"x\");";
    assert_eq!(names_declared_by(source, "record_id"), vec!["RealOne"]);
}

#[test]
fn the_reader_handles_a_multi_line_invocation() {
    let source = "record_id!(\n    ActorId,\n    \"an actor\"\n);\nrecord_id!(ManifestId, \"m\");";
    assert_eq!(
        names_declared_by(source, "record_id"),
        vec!["ActorId", "ManifestId"]
    );
}
