//! `src/ids.rs` and `src/wire.rs` mirror `mesh-types`. This is what keeps them mirroring it.
//!
//! # Why a source-text lint and not a dependency
//!
//! `mesh-sync-protocol` declares no dependency, not even a path dependency on `mesh-types`,
//! because any dependency edge rewrites `Cargo.lock` — see
//! `docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md`, which `mesh-store` wrote
//! when it hit the fence, and `docs/adr/0014-narrow-the-lockfile-fence-to-admit-an-audited-cryptographic-dependency.md`,
//! which narrowed it for audited cryptography and for nothing else.
//!
//! So this crate re-declares seven record identifiers, two counters and one encoding profile name
//! under their own names, and a declaration nothing checks is a comment. This test reads
//! `mesh-types`' own source and holds the two against each other in both directions:
//!
//! * every name mirrored here still exists there, so a rename turns this red rather than leaving a
//!   mirror of a type that is gone;
//! * every record identifier `mesh-types` declares is either mirrored here or listed in
//!   [`NOT_MIRRORED`] with a reason, so a *new* identifier cannot quietly go unconsidered.
//!
//! # The encoding claim is the one that matters most
//!
//! `src/wire.rs` says CWP messages are encoded in `mesh-cbor/0` — the same profile a signed record
//! is encoded in. If that name or its rules moved in `mesh-types` and nothing here noticed, this
//! crate would be describing an encoding that no longer exists while still carrying record bodies
//! that were signed under the real one.
//!
//! # What it is not
//!
//! A lint over source text, with that technique's limits: it reads three files by path and
//! recognises one macro invocation and a handful of literals. A record identifier declared some
//! other way, or those files moved, would be missed — and would surface as this test finding
//! nothing, which the arity assertions below turn into a failure rather than a silent pass.

use std::fs;
use std::path::PathBuf;

use mesh_sync_protocol::RECORD_ENCODING_PROFILE;

/// The `mesh-types` record identifiers `src/ids.rs` re-declares, by name.
const MIRRORED: [&str; 7] = [
    "ActorId",
    "ChangeSetId",
    "HeadId",
    "ManifestId",
    "ContentHash",
    "ReviewBundleId",
    "ApprovalId",
];

/// The `mesh-types` record identifiers this crate does not mirror, and why not.
///
/// A new identifier there must be added to one list or the other. Adding it to this one is a
/// sentence somebody has to write, which is the friction that makes the choice deliberate.
const NOT_MIRRORED: [(&str, &str); 1] = [(
    "VersionId",
    "names an object version; materialization is mesh-materializer's and no CWP message carries a \
     version identifier — a peer learns of a changed object through the ChangeSet that changed it",
)];

fn mesh_types_source(file: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", "mesh-types", "src", file]
        .iter()
        .collect();
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). This test holds mesh-sync-protocol's mirrors against the \
             types mesh-types actually declares by reading its source, because no dependency edge \
             is permitted; if the file has moved, src/ids.rs and src/wire.rs are unchecked until \
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

/// The two counters `HeadAdvertisement` and `CarriedChangeSet` carry are `mesh-types`' own, and
/// this crate mirrors their names so the swap is a `use` line rather than a rename.
#[test]
fn the_two_counters_this_crate_mirrors_still_exist_in_mesh_types() {
    let source = mesh_types_source("changeset.rs");
    for declaration in ["pub struct ActorSequence", "pub struct PolicyEpoch"] {
        assert!(
            source.contains(declaration),
            "mesh-types no longer declares `{declaration}`, so src/ids.rs mirrors nothing"
        );
    }
}

/// The claim `src/wire.rs` is built on: CWP messages are in the same encoding profile as the
/// record bodies they carry.
#[test]
fn the_record_encoding_profile_is_still_the_one_mesh_types_publishes() {
    let source = mesh_types_source("cbor.rs");
    let declaration = format!("pub const CBOR_PROFILE: &str = \"{RECORD_ENCODING_PROFILE}\";");
    assert!(
        source.contains(&declaration),
        "mesh-types no longer publishes `{RECORD_ENCODING_PROFILE}` as CBOR_PROFILE. \
         mesh-sync-protocol's RECORD_ENCODING_PROFILE names the profile record bodies are signed \
         under and its own messages are encoded in; if that profile moved, this crate is \
         describing an encoding that no longer exists."
    );
}

/// The rules `src/wire.rs` re-implements. Reimplementing a profile is only safe while the profile
/// it copies still says what it said.
#[test]
fn the_encoding_rules_this_crate_reimplements_are_still_the_profile_s_rules() {
    let source = mesh_types_source("cbor.rs");
    for rule in [
        "const MAJOR_UNSIGNED: u8 = 0;",
        "const MAJOR_BYTES: u8 = 2;",
        "const MAJOR_TEXT: u8 = 3;",
        "const MAJOR_ARRAY: u8 = 4;",
        "const FALSE_BYTE: u8 = 0xf4;",
        "const TRUE_BYTE: u8 = 0xf5;",
    ] {
        assert!(
            source.contains(rule),
            "mesh-types' mesh-cbor/0 no longer states `{rule}`. src/wire.rs encodes CWP messages \
             with exactly these constants; if they moved there, the two encoders disagree."
        );
    }
    assert!(
        source.contains("shortest head that fits the value"),
        "mesh-types no longer states the shortest-head rule, which is the single rule src/wire.rs \
         enforces on decode as WireError::NonMinimalHead"
    );
    assert!(
        source.contains("**no maps**"),
        "mesh-types' profile no longer excludes maps. src/wire.rs has no map writer and no map \
         reader, so a message set that needed one could not be encoded at all."
    );
}
