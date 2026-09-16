//! `src/ids.rs` and `src/stamp.rs` mirror `mesh-state`. This is what keeps them mirroring it.
//!
//! # Why a source-text lint and not a dependency
//!
//! No crate in this workspace declares a dependency on another: a dependency edge rewrites
//! `Cargo.lock`, which the repository treats as governance surface a lane escalates rather than
//! writes. `docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md` records the
//! reasoning; `mesh-state` carries the same test against `mesh-operations` for the same reason.
//!
//! So this crate re-declares four identifiers and a stamp, and a mirror nothing checks is a guess
//! about another crate. This test reads `mesh-state`'s own source and requires:
//!
//! * each mirrored identifier to still be declared there, at the same byte width;
//! * [`Stamp`](mesh_conflicts::Stamp)'s three fields to still be declared there **in the same
//!   order**, because a derived `Ord` compares fields in declaration order and that order *is* the
//!   protocol's tiebreak — a reordering there would silently reorder every conflict resolution
//!   here;
//! * `mesh-state` to still declare that it has no dependency, since the day that changes is the
//!   day this crate can stop mirroring.
//!
//! # What it is not
//!
//! A lint over source text. An identifier declared some other way, or a file moved, would be
//! missed — and would surface as this test finding nothing, which the arity assertions below turn
//! into a failure rather than a silent pass.

use std::fs;
use std::path::PathBuf;

use mesh_conflicts::{
    ActorId, EventId, HeadId, ObjectId, VersionId, ACTOR_ID_BYTES, HEAD_ID_BYTES, OBJECT_ID_BYTES,
    VERSION_ID_BYTES,
};

/// A file of `mesh-state`, read from this crate's own manifest directory.
fn mesh_state(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../mesh-state/src")
        .join(file);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("mesh-state source at {}: {error}", path.display()))
}

#[test]
fn the_thirty_two_byte_identifiers_are_still_declared_by_mesh_state() {
    let source = mesh_state("ids.rs");
    assert!(
        source.contains("pub struct $name([u8; 32]);"),
        "mesh-state's record identifiers are no longer thirty-two bytes"
    );
    assert!(
        source.contains("pub const BYTE_WIDTH: usize = 32;"),
        "mesh-state's record identifiers no longer declare a thirty-two byte width"
    );
    for mirrored in ["ActorId", "HeadId", "VersionId"] {
        assert!(
            source.contains(&format!("record_id!(\n    {mirrored},"))
                || source.contains(&format!("record_id!({mirrored},")),
            "mesh-state no longer declares {mirrored}, which this crate mirrors in src/ids.rs"
        );
    }
}

#[test]
fn the_object_identifier_is_still_sixteen_bytes_in_mesh_state() {
    let source = mesh_state("object.rs");
    assert!(
        source.contains("pub struct ObjectId([u8; 16]);"),
        "mesh-state's ObjectId is no longer sixteen bytes"
    );
    assert!(
        source.contains("pub const BYTE_WIDTH: usize = 16;"),
        "mesh-state's ObjectId no longer declares a sixteen byte width"
    );
}

#[test]
fn the_mirrored_widths_match_the_widths_mesh_state_declares() {
    assert_eq!(OBJECT_ID_BYTES, 16);
    assert_eq!(VERSION_ID_BYTES, 32);
    assert_eq!(ACTOR_ID_BYTES, 32);
    assert_eq!(HEAD_ID_BYTES, 32);
    assert_eq!(ObjectId::BYTE_WIDTH, OBJECT_ID_BYTES);
    assert_eq!(VersionId::BYTE_WIDTH, VERSION_ID_BYTES);
    assert_eq!(ActorId::BYTE_WIDTH, ACTOR_ID_BYTES);
    assert_eq!(HeadId::BYTE_WIDTH, HEAD_ID_BYTES);
    assert_eq!(EventId::BYTE_WIDTH, 16);
}

#[test]
fn the_stamp_fields_are_still_declared_in_the_tiebreak_order() {
    let source = mesh_state("stamp.rs");
    let declaration = source
        .split("pub struct Stamp {")
        .nth(1)
        .expect("mesh-state no longer declares a Stamp struct")
        .split('}')
        .next()
        .expect("mesh-state's Stamp declaration is unterminated");

    let fields: Vec<&str> = declaration
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .collect();
    assert_eq!(
        fields,
        vec!["lamport: Lamport,", "event: EventId,", "content: [u8; 32],"],
        "mesh-state's Stamp fields changed; a derived Ord compares them in declaration order, so \
         this reorders every conflict resolution in mesh-conflicts"
    );
    assert!(
        source.contains("pub struct Lamport(u64);"),
        "mesh-state's Lamport is no longer a u64"
    );
    assert!(
        source.contains("pub struct EventId([u8; 16]);"),
        "mesh-state's EventId is no longer sixteen bytes"
    );
}

#[test]
fn mesh_state_still_declares_no_dependency_of_its_own() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../mesh-state/Cargo.toml");
    let source = fs::read_to_string(&manifest)
        .unwrap_or_else(|error| panic!("mesh-state manifest at {}: {error}", manifest.display()));
    let dependencies = source
        .split("[dependencies]")
        .nth(1)
        .expect("mesh-state's manifest no longer has a dependencies table");
    assert!(
        dependencies
            .lines()
            .take_while(|line| !line.trim_start().starts_with('['))
            .all(|line| line.trim().is_empty() || line.trim_start().starts_with('#')),
        "mesh-state now declares a dependency; the fence that forces this mirror may have moved, \
         and mesh-conflicts should depend on mesh-state directly rather than mirror it"
    );
}

/// The mirror is worth nothing if this test cannot fail.
#[test]
fn the_field_order_check_rejects_a_reordered_stamp() {
    let reordered =
        "pub struct Stamp {\n    event: EventId,\n    lamport: Lamport,\n    content: [u8; 32],\n}";
    let declaration = reordered
        .split("pub struct Stamp {")
        .nth(1)
        .expect("the fixture declares a Stamp")
        .split('}')
        .next()
        .expect("the fixture terminates");
    let fields: Vec<&str> = declaration
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .collect();
    assert_ne!(
        fields,
        vec!["lamport: Lamport,", "event: EventId,", "content: [u8; 32],"]
    );
}

/// The two copies of the fold are byte-identical, or this crate and `mesh-state` disagree about
/// which names are one name.
///
/// The consequence of drift is not cosmetic. `mesh-state` refuses a materialization when two
/// entries fold together; this crate reports the pair that made it refuse. A fold that answered
/// differently here would produce a refusal with no conflict attached to it, or a conflict for a
/// pair the register was happy to write — and a person shown either one has no way to tell which
/// half is wrong.
///
/// Byte equality rather than behaviour equality, deliberately: behaviour equality would need this
/// test to reimplement the fold, and a third implementation is a third thing to keep in step.
#[test]
fn the_fold_and_its_table_are_byte_identical_in_both_crates() {
    for file in ["fold.rs", "unicode-fold.txt"] {
        let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join(file);
        let there = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../mesh-state/src")
            .join(file);
        let ours = fs::read(&here).unwrap_or_else(|error| panic!("{}: {error}", here.display()));
        let theirs =
            fs::read(&there).unwrap_or_else(|error| panic!("{}: {error}", there.display()));
        assert!(
            !ours.is_empty(),
            "{} is empty, which would make this check pass for the wrong reason",
            here.display()
        );
        assert_eq!(
            ours.len(),
            theirs.len(),
            "{file} differs in length between mesh-conflicts and mesh-state; copy one over the \
             other rather than editing both"
        );
        assert!(
            ours == theirs,
            "{file} differs in content between mesh-conflicts and mesh-state; copy one over the \
             other rather than editing both"
        );
    }
}

/// And the fold really is reachable through this crate's public surface.
#[test]
fn the_mirrored_fold_answers_through_this_crate() {
    use mesh_conflicts::{relate, NameFold, UNICODE_VERSION};

    assert_eq!(UNICODE_VERSION, "15.0.0");
    assert!(relate("README.md", "readme.md").joined_by(NameFold::Upcase));
    assert!(relate("caf\u{e9}", "cafe\u{301}").joined_by(NameFold::Canonical));
    assert!(relate("README.md", "LICENSE").is_distinct());
}
