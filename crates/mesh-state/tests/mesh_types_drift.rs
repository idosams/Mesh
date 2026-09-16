//! `src/ids.rs` and `src/parents.rs` mirror `mesh-types`. This is what keeps them mirroring it.
//!
//! # Why a source-text lint and not a dependency
//!
//! `mesh-state` declares no dependency, not even a path dependency on `mesh-types`, because any
//! dependency edge rewrites `Cargo.lock` — see
//! `docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md`, which `mesh-store` wrote when
//! it hit the same fence. So this crate re-declares four record identifiers, one entity identifier,
//! `NormalizedName` and `CausalParents` under their own names, and a declaration nothing checks is
//! a comment.
//!
//! This test reads `mesh-types`' own source and holds the two against each other in both
//! directions:
//!
//! * every name mirrored here still exists there, so a rename turns this red rather than leaving a
//!   mirror of a type that is gone;
//! * every record identifier `mesh-types` declares is either mirrored here or listed in
//!   [`NOT_MIRRORED`] with a reason, so a *new* identifier cannot quietly go unconsidered.
//!
//! It also pins the two constructors that make [`mesh_state::CausalParents`] worth having, and the
//! three ChangeSet fields head advancement reads.
//!
//! # What it is not
//!
//! A lint over source text, with that technique's limits: it reads two files by path and
//! recognises one macro invocation and a handful of signatures. A record identifier declared some
//! other way, or those files moved, would be missed — and would surface as this test finding
//! nothing, which the arity assertions below turn into a failure rather than a silent pass.

use std::fs;
use std::path::PathBuf;

/// The `mesh-types` record identifiers `src/ids.rs` re-declares, by name.
const MIRRORED: [&str; 4] = ["ActorId", "ChangeSetId", "HeadId", "VersionId"];

/// The `mesh-types` record identifiers this crate does not mirror, and why not.
///
/// A new identifier there must be added to one list or the other. Adding it to this one is a
/// sentence somebody has to write, which is the friction that makes the choice deliberate.
const NOT_MIRRORED: [(&str, &str); 4] = [
    (
        "ManifestId",
        "names a file manifest; content is mesh-cas' and never reaches head advancement",
    ),
    (
        "ContentHash",
        "names a chunk; the content plane never reaches head advancement",
    ),
    (
        "ReviewBundleId",
        "names a review bundle; this crate offers a head for review and never builds the bundle",
    ),
    (
        "ApprovalId",
        "names an approval envelope; canonical advancement is mesh-approval's",
    ),
];

fn mesh_types_source(file: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", "mesh-types", "src", file]
        .iter()
        .collect();
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). This test holds mesh-state's mirrors against the types \
             mesh-types actually declares by reading its source, because no dependency edge is \
             permitted; if the file has moved, src/ids.rs and src/parents.rs are unchecked until \
             this path is corrected.",
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
    names
}

#[test]
fn every_identifier_this_crate_mirrors_still_exists_in_mesh_types() {
    let declared = names_declared_by(&mesh_types_source("record_id.rs"), "record_id");
    assert!(
        declared.len() >= MIRRORED.len() + NOT_MIRRORED.len(),
        "found only {} record identifiers in mesh-types, which means the scan stopped working \
         rather than that mesh-types shrank: {declared:?}",
        declared.len()
    );

    for name in MIRRORED {
        assert!(
            declared.iter().any(|found| found == name),
            "src/ids.rs mirrors `{name}`, which mesh-types no longer declares. Rename the mirror \
             to whatever replaced it, or delete it."
        );
    }
}

#[test]
fn every_mesh_types_identifier_is_either_mirrored_or_accounted_for() {
    let declared = names_declared_by(&mesh_types_source("record_id.rs"), "record_id");

    for name in &declared {
        let mirrored = MIRRORED.iter().any(|known| known == name);
        let excused = NOT_MIRRORED.iter().any(|(known, _)| known == name);
        assert!(
            mirrored || excused,
            "mesh-types declares `{name}`, which src/ids.rs neither mirrors nor lists in \
             NOT_MIRRORED. Decide which, and if it is the second, say why."
        );
    }
}

#[test]
fn no_excuse_is_left_behind_by_a_deleted_identifier() {
    let declared = names_declared_by(&mesh_types_source("record_id.rs"), "record_id");
    for (name, reason) in NOT_MIRRORED {
        assert!(
            declared.iter().any(|found| found == name),
            "NOT_MIRRORED excuses `{name}` ({reason}), which mesh-types no longer declares"
        );
        assert!(
            !reason.trim().is_empty(),
            "`{name}` is excused with no reason"
        );
    }
}

#[test]
fn causal_parents_still_has_its_two_deliberate_constructors() {
    let source = mesh_types_source("changeset.rs");
    assert!(
        source.contains("pub struct CausalParents"),
        "mesh-types no longer declares CausalParents, so src/parents.rs mirrors nothing"
    );
    for signature in [
        "pub const fn genesis()",
        "pub fn after(first: ChangeSetId, rest: Vec<ChangeSetId>)",
    ] {
        assert!(
            source.contains(signature),
            "mesh-types' CausalParents no longer declares `{signature}`. The split between a \
             deliberate genesis and a non-empty parent list is the reason src/parents.rs mirrors \
             this type rather than using a plain Vec; if it is gone there, it should not survive \
             here by accident."
        );
    }
}

#[test]
fn a_changeset_still_carries_the_three_fields_head_advancement_reads() {
    let source = mesh_types_source("changeset.rs");
    for accessor in [
        "pub const fn causal_parents(&self) -> &CausalParents",
        "pub const fn base_head(&self) -> HeadId",
        "pub const fn resulting_head(&self) -> HeadId",
    ] {
        assert!(
            source.contains(accessor),
            "mesh-types' ChangeSet no longer offers `{accessor}`. DeliveredChangeSet is the \
             projection of exactly those fields, so the projection is now of something else."
        );
    }
}

/// The entity identifiers are the other of plan §4.2's two families, generated by a different
/// macro in a different file. `src/object.rs` mirrors exactly one of them, so the same
/// mirrored-or-excused discipline applies here.
const MIRRORED_ENTITY_IDS: [&str; 1] = ["ObjectId"];

/// The `mesh-types` entity identifiers this crate does not mirror, and why not.
const NOT_MIRRORED_ENTITY_IDS: [(&str, &str); 3] = [
    (
        "WorkspaceId",
        "names a whole workspace; the register holds one workspace's objects and never names it",
    ),
    (
        "SessionId",
        "names one bounded interval of an actor's work; the register records what changed, not who \
         was sitting down at the time",
    ),
    (
        "CapabilityId",
        "names a scoped grant of authority; authorisation is mesh-policy's",
    ),
];

#[test]
fn every_entity_identifier_is_either_mirrored_or_accounted_for() {
    let declared = names_declared_by(&mesh_types_source("entity_id.rs"), "entity_id");
    assert!(
        declared.len() >= MIRRORED_ENTITY_IDS.len() + NOT_MIRRORED_ENTITY_IDS.len(),
        "found only {} entity identifiers in mesh-types, which means the scan stopped working \
         rather than that mesh-types shrank: {declared:?}",
        declared.len()
    );
    for name in MIRRORED_ENTITY_IDS {
        assert!(
            declared.iter().any(|found| found == name),
            "src/object.rs mirrors `{name}`, which mesh-types no longer declares"
        );
    }
    for name in &declared {
        let mirrored = MIRRORED_ENTITY_IDS.iter().any(|known| known == name);
        let excused = NOT_MIRRORED_ENTITY_IDS
            .iter()
            .any(|(known, _)| known == name);
        assert!(
            mirrored || excused,
            "mesh-types declares entity identifier `{name}`, which this crate neither mirrors nor \
             lists in NOT_MIRRORED_ENTITY_IDS. Decide which, and if it is the second, say why."
        );
    }
    for (name, reason) in NOT_MIRRORED_ENTITY_IDS {
        assert!(
            declared.iter().any(|found| found == name),
            "NOT_MIRRORED_ENTITY_IDS excuses `{name}` ({reason}), which mesh-types no longer \
             declares"
        );
    }
}

/// The two families have different widths, and the split is the reason both mirrors exist. A
/// `mesh-types` that unified them would make `src/object.rs`'s sixteen bytes wrong without any
/// name changing, which no name-based check would catch.
#[test]
fn the_two_identifier_families_still_have_different_widths() {
    let entity = mesh_types_source("entity_id.rs");
    assert!(
        entity.contains("Uuid"),
        "mesh-types' entity identifiers no longer wrap a UUID, so src/object.rs' sixteen-byte \
         mirror may no longer be the right width"
    );
    let record = mesh_types_source("record_id.rs");
    assert!(
        record.contains("Digest32"),
        "mesh-types' record identifiers no longer wrap a thirty-two-byte digest, so src/ids.rs mirrors the \
         wrong width"
    );
}

/// `src/name.rs` re-declares `mesh-types`' entry-name rules character for character. A name this
/// crate accepts and `mesh-types` rejects would be a directory entry that resolves in the register
/// and cannot be materialized, so the four rejections are pinned individually.
#[test]
fn the_entry_name_rules_are_still_the_four_this_crate_mirrors() {
    let source = mesh_types_source("object.rs");
    assert!(
        source.contains("pub struct NormalizedName"),
        "mesh-types no longer declares NormalizedName, so src/name.rs mirrors nothing"
    );
    for rule in [
        "return Err(NameError::Empty)",
        "return Err(NameError::Relative)",
        "return Err(NameError::Separator)",
        "return Err(NameError::Nul)",
    ] {
        assert!(
            source.contains(rule),
            "mesh-types' NormalizedName no longer enforces `{rule}`. src/name.rs still does, so \
             the two now disagree about what a directory entry name is."
        );
    }
}

/// `ObjectKind` is what lets the register refuse to link a child into a file. Its three members
/// are `mesh-types`', and a fourth appearing there without one appearing here would mean the
/// register silently treats an unknown kind as a file.
#[test]
fn an_object_still_has_exactly_the_three_kinds_this_crate_mirrors() {
    let source = mesh_types_source("object.rs");
    assert!(source.contains("pub enum ObjectKind"));
    for member in ["File", "Directory", "Symlink"] {
        assert!(
            source.contains(member),
            "mesh-types' ObjectKind no longer has `{member}`"
        );
    }
    assert!(
        source.contains("pub const ALL: [Self; 3]"),
        "mesh-types' ObjectKind no longer has exactly three members, so src/object.rs' three are \
         no longer the whole set"
    );
}

/// The field this crate carries and refuses to read. If `mesh-types` ever stopped carrying it,
/// `DeliveredChangeSet`'s two clock fields would be mirroring nothing and the claim they exist to
/// make — carried, never consulted — would be about a field no peer sends.
#[test]
fn a_changeset_still_carries_a_hybrid_logical_time() {
    let source = mesh_types_source("changeset.rs");
    assert!(source.contains("pub const fn hybrid_logical_time(&self) -> Hlc"));
    assert!(
        source.contains("never for causality"),
        "mesh-types no longer states that hybrid logical time is never used for causality, which \
         is the rule src/no_ambient_input.rs enforces on this side"
    );
}
