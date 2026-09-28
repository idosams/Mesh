//! `IdentityChange` is a projection of six of `mesh-operations`' nineteen verbs. This is what keeps
//! it projecting them.
//!
//! # Why a source-text lint and not a dependency
//!
//! `mesh-state` declares no dependency at all — `src/ids.rs` states the reason and
//! `docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md` records it — so it cannot
//! import the operation vocabulary. [`mesh_state::IdentityChange`] therefore re-declares the six
//! members that touch object identity, and a projection nothing checks is a guess about another
//! crate.
//!
//! This test reads `mesh-operations`' own source and requires:
//!
//! * each of the six members to still be declared there, so a rename turns this red;
//! * the fields the projection reads to still be carried, so a *narrowed* operation is caught;
//! * `MoveEntry` to still name no descendant, which is the whole subtree-move budget;
//! * the vocabulary to still hold exactly nineteen members, so a twentieth that touches identity
//!   cannot be added without somebody deciding whether it belongs in the projection.
//!
//! # What it is not
//!
//! A lint over source text. A member declared some other way, or the file moved, would be missed —
//! and would surface as this test finding nothing, which the arity assertion turns into a failure
//! rather than a silent pass.

use std::fs;
use std::path::PathBuf;

/// The vocabulary members [`mesh_state::IdentityChange`] projects, and the fields it reads from
/// each. A field named here that stops being declared there means the projection is now of
/// something else.
const PROJECTED: [(&str, &[&str]); 6] = [
    ("CreateFile", &["object_id: ObjectId"]),
    ("CreateDirectory", &["object_id: ObjectId"]),
    (
        "LinkDirectoryEntry",
        &["directory_id: ObjectId", "name: NormalizedName"],
    ),
    (
        "UnlinkDirectoryEntry",
        &["directory_id: ObjectId", "name: NormalizedName"],
    ),
    (
        "RenameEntry",
        &["from_name: NormalizedName", "to_name: NormalizedName"],
    ),
    (
        "MoveEntry",
        &[
            "from_directory_id: ObjectId",
            "to_directory_id: ObjectId",
            "to_name: NormalizedName",
        ],
    ),
];

/// The thirteen members that do not touch object identity, with the reason each is left out.
///
/// A new member of the vocabulary must join one list or the other. Adding it to this one is a
/// sentence somebody has to write, which is the friction that makes the choice deliberate.
const NOT_PROJECTED: [(&str, &str); 13] = [
    ("InitializeWorkspace", "declares the immutable root supplied to materialization; it never mints or changes a register entry"),
    (
        "WriteFileVersion",
        "projected as WriteVersion, which carries the version and not the manifest: content is \
         mesh-cas' and the register only needs to know that the content facet moved",
    ),
    (
        "DeleteObject",
        "deletion is a state with a tombstone, which is mesh-conflicts' rule and not the register's",
    ),
    (
        "RestoreObject",
        "the other half of deletion, and it belongs wherever deletion does",
    ),
    (
        "SetPortableMetadata",
        "metadata rides a version; the register holds the version identifier and nothing inside it",
    ),
    (
        "ResolveNameConflict",
        "the register reports every contender for a contested name and settles none of them",
    ),
    (
        "ResolveContentConflict",
        "settling divergent content is the conflict engine's",
    ),
    ("AdvanceActorHead", "head advancement, which src/advance.rs owns"),
    (
        "RecordReadObservation",
        "a read observation is mesh-context-ledger's and changes no entry",
    ),
    (
        "RecordDerivedNode",
        "a derived node is mesh-derivations' and changes no entry",
    ),
    (
        "CreateReviewBundle",
        "a review bundle is mesh-approval's and changes no entry",
    ),
    (
        "RecordValidation",
        "a validation conclusion is mesh-validator's and changes no entry",
    ),
    (
        "AdvanceCanonicalHead",
        "canonical advancement is mesh-approval's",
    ),
];

fn operations_source() -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "mesh-operations",
        "src",
        "operation.rs",
    ]
    .iter()
    .collect();
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). IdentityChange is a projection of the vocabulary declared \
             there, and no dependency edge is permitted, so if that file has moved the projection \
             is unchecked until this path is corrected.",
            path.display()
        )
    })
}

/// The members of `OperationKind::ALL`, in declaration order.
fn vocabulary(source: &str) -> Vec<String> {
    let Some(start) = source.find("pub const ALL: [Self; 19]") else {
        panic!(
            "mesh-operations no longer declares OperationKind::ALL with nineteen members, so this \
             test is reading the wrong thing rather than checking it"
        );
    };
    let tail = &source[start..];
    let end = tail.find("];").expect("the array literal terminates");
    tail[..end]
        .lines()
        .filter_map(|line| line.trim().strip_prefix("Self::"))
        .map(|member| member.trim_end_matches(',').to_owned())
        .collect()
}

#[test]
fn the_vocabulary_still_has_nineteen_members_and_this_test_can_read_them() {
    let members = vocabulary(&operations_source());
    assert_eq!(
        members.len(),
        19,
        "read {} members out of OperationKind::ALL: {members:?}",
        members.len()
    );
}

#[test]
fn every_member_is_either_projected_or_accounted_for() {
    let members = vocabulary(&operations_source());
    for member in &members {
        let projected = PROJECTED.iter().any(|(name, _)| name == member);
        let excused = NOT_PROJECTED.iter().any(|(name, _)| name == member);
        assert!(
            projected || excused,
            "mesh-operations declares `{member}`, which IdentityChange neither projects nor lists \
             in NOT_PROJECTED. Decide whether it touches object identity, and if it does not, say \
             why."
        );
    }
    for (name, _) in PROJECTED {
        assert!(
            members.iter().any(|member| member == name),
            "IdentityChange projects `{name}`, which mesh-operations no longer declares"
        );
    }
    for (name, reason) in NOT_PROJECTED {
        assert!(
            members.iter().any(|member| member == name),
            "NOT_PROJECTED excuses `{name}` ({reason}), which mesh-operations no longer declares"
        );
        assert!(
            !reason.trim().is_empty(),
            "`{name}` is excused with no reason"
        );
    }
}

#[test]
fn every_projected_member_still_carries_the_fields_the_projection_reads() {
    let source = operations_source();
    for (member, fields) in PROJECTED {
        for field in fields {
            assert!(
                source.contains(field),
                "mesh-operations' `{member}` no longer carries `{field}`, so IdentityChange is a \
                 projection of something else"
            );
        }
    }
}

/// The budget in one assertion. `MoveEntry` carrying a descendant list is the one change to that
/// crate that would make plan §11's subtree-move budget unreachable no matter what this register
/// does, so it is checked from this side too.
#[test]
fn a_move_still_names_no_descendant() {
    let source = operations_source();
    let start = source
        .find("    MoveEntry {")
        .expect("mesh-operations no longer declares a MoveEntry variant");
    let body = &source[start..];
    let end = body.find("\n    },").expect("the variant terminates");
    // The declared fields only. A doc comment saying "the root of a subtree" is prose about the
    // shape, not a field carrying one.
    let fields: String = body[..end]
        .lines()
        .filter(|line| !line.trim_start().starts_with("///"))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in ["descendant", "children", "Vec<", "subtree"] {
        assert!(
            !fields.contains(forbidden),
            "mesh-operations' MoveEntry now mentions `{forbidden}`. A move that names what is \
             under it costs one record per descendant, and plan §11's budget — a million \
             descendants under 100 ms, under 10 KiB — is unreachable by construction from that \
             moment on."
        );
    }
    assert!(fields.contains("object_id: ObjectId"));
}
