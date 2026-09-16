//! `src/ids.rs`, `src/name.rs` and `src/context.rs` mirror `mesh-types`. This is what keeps them
//! mirroring it.
//!
//! # Why a source-text lint and not a dependency
//!
//! `mesh-operations` declares no dependency, not even a path dependency on `mesh-types`, because
//! any dependency edge rewrites `Cargo.lock` and this repository's declaration gate refuses that
//! write. `mesh-store` hit the fence first and recorded it in
//! `docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md`; `mesh-state` did the same.
//! So this crate re-declares eleven identifiers, two value types and the causal context under
//! their own names, and a declaration nothing checks is a comment.
//!
//! This test reads `mesh-types`' own source and holds the two against each other in both
//! directions:
//!
//! * every name mirrored here still exists there, so a rename turns this red rather than leaving a
//!   mirror of a type that is gone;
//! * every identifier `mesh-types` declares is either mirrored here or listed in [`NOT_MIRRORED`]
//!   with a reason, so a *new* one cannot quietly go unconsidered.
//!
//! It also pins the three things a wire format cannot afford to drift on: the encoding profile
//! name, the ChangeSet domain tag, and the ChangeSet field order.
//!
//! # What it is not
//!
//! A lint over source text, with that technique's limits: it reads files by path and recognises
//! two macro invocations and a handful of literals. A type declared some other way, or those files
//! moved, would be missed — and would surface as this test finding nothing, which the arity
//! assertions turn into a failure rather than a silent pass.

mod common;

use common::read_repo_file;
use mesh_operations::{CBOR_PROFILE, CHANGESET_DOMAIN, CHANGESET_SCHEMA};

/// The `mesh-types` record identifiers `src/ids.rs` re-declares, by name.
const MIRRORED_RECORD_IDS: [&str; 8] = [
    "ActorId",
    "VersionId",
    "ManifestId",
    "ChangeSetId",
    "HeadId",
    "ReviewBundleId",
    "ApprovalId",
    "ContentHash",
];

/// The `mesh-types` entity identifiers `src/ids.rs` re-declares, by name.
const MIRRORED_ENTITY_IDS: [&str; 3] = ["WorkspaceId", "SessionId", "ObjectId"];

/// The `mesh-types` identifiers this crate does not mirror, and why not.
///
/// A new identifier there must be added to one list or the other. Adding it to this one is a
/// sentence somebody has to write, which is the friction that makes the choice deliberate.
const NOT_MIRRORED: [(&str, &str); 1] = [(
    "CapabilityId",
    "names a capability grant; authority is mesh-policy's and no operation carries one",
)];

/// Identifiers this crate declares that `mesh-types` does not, and why.
///
/// The reverse direction of the same discipline: a name that looks mirrored and is not would let a
/// reader assume an agreement that does not exist.
const DECLARED_HERE_ONLY: [(&str, &str); 1] = [(
    "DerivationId",
    "plan §4.10's derived computation node identifier. mesh-types has no such type yet, so \
     RecordDerivedNode would otherwise have had to reuse VersionId and say something it does not \
     mean.",
)];

fn mesh_types_source(file: &str) -> String {
    read_repo_file(&format!("crates/mesh-types/src/{file}"))
}

/// The names declared by `macro!(Name, …)` invocations in some source.
fn names_declared_by(source: &str, macro_name: &str) -> Vec<String> {
    let opener = format!("{macro_name}!(");
    source
        .match_indices(&opener)
        .filter_map(|(at, _)| {
            let rest = &source[at + opener.len()..];
            let name: String = rest
                .chars()
                .skip_while(|character| character.is_whitespace())
                .take_while(|character| character.is_alphanumeric() || *character == '_')
                .collect();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

#[test]
fn every_record_identifier_mesh_types_declares_is_mirrored_or_excused() {
    let declared = names_declared_by(&mesh_types_source("record_id.rs"), "record_id");
    assert_eq!(
        declared.len(),
        8,
        "mesh-types declares {} record identifiers, not the eight this mirror was built against: \
         {declared:?}",
        declared.len()
    );
    for name in &declared {
        let mirrored = MIRRORED_RECORD_IDS.contains(&name.as_str());
        let excused = NOT_MIRRORED.iter().any(|(other, _)| other == name);
        assert!(
            mirrored || excused,
            "mesh-types declares {name} and this crate neither mirrors it nor says why not"
        );
    }
    for mirrored in MIRRORED_RECORD_IDS {
        assert!(
            declared.iter().any(|name| name == mirrored),
            "this crate mirrors {mirrored} and mesh-types no longer declares it"
        );
    }
}

#[test]
fn every_entity_identifier_mesh_types_declares_is_mirrored_or_excused() {
    let declared = names_declared_by(&mesh_types_source("entity_id.rs"), "entity_id");
    assert_eq!(
        declared.len(),
        4,
        "mesh-types declares {} entity identifiers, not the four this mirror was built against: \
         {declared:?}",
        declared.len()
    );
    for name in &declared {
        let mirrored = MIRRORED_ENTITY_IDS.contains(&name.as_str());
        let excused = NOT_MIRRORED.iter().any(|(other, _)| other == name);
        assert!(
            mirrored || excused,
            "mesh-types declares {name} and this crate neither mirrors it nor says why not"
        );
    }
    for mirrored in MIRRORED_ENTITY_IDS {
        assert!(
            declared.iter().any(|name| name == mirrored),
            "this crate mirrors {mirrored} and mesh-types no longer declares it"
        );
    }
}

#[test]
fn an_identifier_declared_only_here_is_declared_only_here() {
    let record = mesh_types_source("record_id.rs");
    let entity = mesh_types_source("entity_id.rs");
    for (name, reason) in DECLARED_HERE_ONLY {
        assert!(!reason.is_empty());
        assert!(
            !names_declared_by(&record, "record_id")
                .iter()
                .any(|other| other == name)
                && !names_declared_by(&entity, "entity_id")
                    .iter()
                    .any(|other| other == name),
            "{name} now exists in mesh-types too; mirror it and move it out of DECLARED_HERE_ONLY"
        );
    }
}

/// The wire format cannot afford to drift. These three literals are what two implementations agree
/// on before they agree on anything else.
#[test]
fn the_encoding_profile_and_the_changeset_domain_still_match() {
    let cbor = mesh_types_source("cbor.rs");
    assert!(
        cbor.contains(&format!("\"{CBOR_PROFILE}\"")),
        "mesh-types no longer publishes the {CBOR_PROFILE} profile this crate implements"
    );
    let changeset = mesh_types_source("changeset.rs");
    assert!(
        changeset.contains(&format!("DomainTag::new(\"{CHANGESET_DOMAIN}\")")),
        "mesh-types no longer derives ChangeSets under {CHANGESET_DOMAIN}"
    );
}

#[test]
fn the_changeset_field_order_still_matches() {
    let changeset = mesh_types_source("changeset.rs");
    let declared: Vec<&str> = CHANGESET_SCHEMA
        .fields
        .iter()
        .map(|field| field.name)
        .collect();
    // `FieldSchema::new("workspace_id", …)` — the order of appearance in the schema block is the
    // wire order. `rustfmt` breaks the longer declarations across lines, so the field name is read
    // after skipping whitespace rather than assumed to follow the parenthesis immediately; a
    // stricter reader silently skipped `causal_parents` and reported it as removed.
    let theirs: Vec<String> = changeset
        .match_indices("FieldSchema::new(")
        .filter_map(|(at, _)| {
            let rest = &changeset[at + "FieldSchema::new(".len()..];
            let mut characters = rest
                .chars()
                .skip_while(|character| character.is_whitespace());
            (characters.next() == Some('"')).then(|| {
                characters
                    .take_while(|character| *character != '"')
                    .collect()
            })
        })
        .collect();
    for name in &declared {
        assert!(
            theirs.iter().any(|other| other == name),
            "mesh-types' ChangeSet no longer carries a {name} field"
        );
    }
    // The ten ChangeSet fields are the tail of that list; the hybrid-logical-time group's own two
    // members are declared before them.
    let start = theirs
        .iter()
        .position(|name| name == "workspace_id")
        .expect("mesh-types declares a workspace_id field");
    let theirs: Vec<&str> = theirs[start..start + declared.len()]
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(
        theirs, declared,
        "the ChangeSet field order has drifted; field order IS the wire format, and every \
         deployed signature was made over bytes the previous order produced"
    );
}

/// `NormalizedName` is the one mirrored type whose *behaviour* matters rather than its width: a
/// name this crate accepts and `mesh-types` rejects is an operation that encodes and cannot be
/// materialized.
#[test]
fn the_name_rules_still_match() {
    let object = mesh_types_source("object.rs");
    for rule in [
        "NameError::Empty",
        "NameError::Relative",
        "NameError::Separator",
        "NameError::Nul",
    ] {
        assert!(
            object.contains(rule),
            "mesh-types no longer refuses names with {rule}"
        );
    }
    for predicate in [
        "name.is_empty()",
        "name == \".\" || name == \"..\"",
        "name.contains('/') || name.contains('\\\\')",
        "name.contains('\\0')",
    ] {
        assert!(
            object.contains(predicate),
            "mesh-types' name rule `{predicate}` has changed; src/name.rs mirrors it verbatim"
        );
    }
}

/// `CausalParents` exists to make an empty parent list a statement rather than an omission. Both
/// crates must keep both constructors, or one of them has quietly lost the distinction.
#[test]
fn the_causal_parent_constructors_still_match() {
    let changeset = mesh_types_source("changeset.rs");
    for constructor in ["pub const fn genesis()", "pub fn after("] {
        assert!(
            changeset.contains(constructor),
            "mesh-types' CausalParents no longer offers `{constructor}`"
        );
    }
}
