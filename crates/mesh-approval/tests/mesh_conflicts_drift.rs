//! `src/ids.rs` and `src/name.rs` mirror `mesh-conflicts`, and `src/ids.rs` mirrors `mesh-types`.
//! This is what keeps them mirroring.
//!
//! # Why these mirrors still use a source-text lint
//!
//! `mesh-approval` deliberately does not depend on `mesh-conflicts`; doing so would collapse the
//! review boundary into the conflict engine. It does depend on the security owners it composes:
//! `mesh-types`, `mesh-crypto`, and `mesh-policy`. This test keeps the remaining conflict-specific
//! mirrors honest without adding the forbidden `mesh-conflicts` edge.
//!
//! This test reads the other crates' source and holds them against this one in both directions:
//!
//! * every name mirrored here still exists there, so a rename turns this red rather than leaving a
//!   mirror of a type that is gone;
//! * every identifier `mesh-conflicts` declares is either mirrored here or listed in
//!   [`NOT_MIRRORED`] with a reason, so a *new* identifier cannot quietly go unconsidered;
//! * `ReviewBundleId` is still declared by `mesh-types`, because this crate derives a value that
//!   crate names.
//!
//! # What it is not
//!
//! A lint over source text, with that technique's limits: it reads files by path and recognises one
//! macro invocation and a handful of declarations. A type declared some other way, or those files
//! moved, would be missed — and would surface as this test finding nothing, which the arity
//! assertions below turn into a failure rather than a silent pass.

use std::fs;
use std::path::PathBuf;

use mesh_approval::{
    ActorId, HeadId, NormalizedName, ObjectId, VersionId, ACTOR_ID_BYTES, HEAD_ID_BYTES,
    MAX_DIFF_LINES, MAX_NAME_BYTES, OBJECT_ID_BYTES, VERSION_ID_BYTES,
};

/// The `mesh-conflicts` identifiers `src/ids.rs` re-declares, with the widths this crate uses.
const MIRRORED: [(&str, usize); 4] = [
    ("ObjectId", OBJECT_ID_BYTES),
    ("VersionId", VERSION_ID_BYTES),
    ("ActorId", ACTOR_ID_BYTES),
    ("HeadId", HEAD_ID_BYTES),
];

/// The `mesh-conflicts` types this crate does not mirror, and why not.
///
/// A new one there must be added to one list or the other. Adding it to this one is a sentence
/// somebody has to write, which is the friction that makes the choice deliberate.
const NOT_MIRRORED: [(&str, &str); 4] = [
    (
        "Change",
        "an operation with a stamp; a bundle is computed from states, never from an operation log",
    ),
    (
        "Snapshot",
        "the conflict engine's base state; this crate carries its own WorkspaceState, which \
         additionally has a digest",
    ),
    (
        "Resolution",
        "the outcome of merging; this crate reports conflicts and never merges",
    ),
    (
        "Stamp",
        "lamport → event ULID → content hash, the conflict engine's ordering key; a bundle orders \
         by object identifier and needs no stamp",
    ),
];

fn source_of(crate_name: &str, relative: &[&str]) -> String {
    let mut path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", crate_name]
        .iter()
        .collect();
    for part in relative {
        path.push(part);
    }
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). mesh-approval holds its mirrors against {crate_name} by \
             reading its source, because no dependency edge is permitted; if that file has moved, \
             the mirror is unchecked until this path is corrected.",
            path.display()
        )
    })
}

#[test]
fn every_mirrored_identifier_is_still_declared_by_mesh_conflicts_at_the_same_width() {
    let source = source_of("mesh-conflicts", &["src", "ids.rs"]);
    let mut found = 0;
    for (name, width) in MIRRORED {
        let declaration = format!("opaque_identifier!({name},");
        assert!(
            source.contains(&declaration),
            "mesh-conflicts no longer declares {name}; this crate's mirror is a mirror of nothing"
        );
        let constant = format!("_ID_BYTES: usize = {width};");
        assert!(
            source.contains(&constant),
            "mesh-conflicts declares no identifier of width {width}, but this crate mirrors {name} \
             at that width"
        );
        found += 1;
    }
    assert_eq!(found, MIRRORED.len());
}

#[test]
fn every_identifier_mesh_conflicts_declares_is_mirrored_or_named_as_not_mirrored() {
    let source = source_of("mesh-conflicts", &["src", "ids.rs"]);
    let declared: Vec<&str> = source
        .lines()
        .filter_map(|line| line.trim().strip_prefix("opaque_identifier!("))
        .filter_map(|rest| rest.split(',').next())
        .collect();

    assert!(
        declared.len() >= MIRRORED.len(),
        "only {} identifiers were parsed out of mesh-conflicts' ids.rs; the macro's shape has \
         changed and this test is no longer comparing anything",
        declared.len()
    );

    for name in declared {
        let mirrored = MIRRORED.iter().any(|(mirrored, _)| *mirrored == name);
        let excused = NOT_MIRRORED.iter().any(|(excused, _)| *excused == name);
        assert!(
            mirrored || excused,
            "mesh-conflicts declares {name} and this crate neither mirrors it nor says why not. \
             Add it to MIRRORED or to NOT_MIRRORED with a reason."
        );
    }
}

#[test]
fn the_types_this_crate_deliberately_does_not_mirror_still_exist_there() {
    let lib = source_of("mesh-conflicts", &["src", "lib.rs"]);
    for (name, reason) in NOT_MIRRORED {
        assert!(
            lib.contains(name),
            "mesh-conflicts no longer offers {name}, so the reason this crate gives for not \
             mirroring it — {reason} — is about something that is gone"
        );
    }
}

#[test]
fn the_entry_name_rules_still_match_mesh_conflicts() {
    let source = source_of("mesh-conflicts", &["src", "name.rs"]);
    assert!(
        source.contains(&format!("MAX_NAME_BYTES: usize = {MAX_NAME_BYTES};")),
        "mesh-conflicts and mesh-approval disagree about how long a directory entry name may be, \
         so one of them would accept a state the other refuses"
    );
    for refusal in ["Empty", "TooLong", "Separator", "Relative"] {
        assert!(
            source.contains(refusal),
            "mesh-conflicts no longer refuses a name for being {refusal}; the mirror in \
             src/name.rs has drifted"
        );
    }

    // The mirror is only worth having if it behaves the same way, so exercise it here too.
    assert!(NormalizedName::new("a/b").is_err());
    assert!(NormalizedName::new("..").is_err());
    assert!(NormalizedName::new("notes.md").is_ok());
}

#[test]
fn the_line_ceiling_and_the_walk_still_match_mesh_conflicts() {
    let source = source_of("mesh-conflicts", &["src", "text.rs"]);
    assert!(
        source.contains(&format!("MAX_MERGE_LINES: usize = {MAX_DIFF_LINES};")),
        "mesh-conflicts and mesh-approval disagree about how many lines may be diffed, so the \
         review surface would render a diff for a file the merge engine refuses to diff, or refuse \
         one it would merge"
    );
    assert!(
        source.contains("pub fn hunks("),
        "mesh-conflicts no longer exposes the longest-common-subsequence walk src/text_diff.rs \
         transcribes, so the hunk boundaries a reviewer approves are no longer the boundaries the \
         merge engine honours"
    );
}

#[test]
fn mesh_types_still_names_the_value_this_crate_derives() {
    let source = source_of("mesh-types", &["src", "record_id.rs"]);
    assert!(
        source.contains("record_id!(ReviewBundleId,"),
        "mesh-types no longer declares ReviewBundleId, so this crate derives a value nothing else \
         in the workspace has a name for"
    );
}

#[test]
fn the_mirrored_widths_are_the_widths_this_crate_uses() {
    assert_eq!(ObjectId::BYTE_WIDTH, OBJECT_ID_BYTES);
    assert_eq!(VersionId::BYTE_WIDTH, VERSION_ID_BYTES);
    assert_eq!(ActorId::BYTE_WIDTH, ACTOR_ID_BYTES);
    assert_eq!(HeadId::BYTE_WIDTH, HEAD_ID_BYTES);
}
