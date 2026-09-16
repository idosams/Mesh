//! The entity model itself: actors, sessions, objects, versions and manifests.

use std::collections::{BTreeMap, HashSet};

use mesh_types::{
    derive_id, ActivitySession, Actor, ActorKind, ActorSequence, Blake3, CapabilityId,
    CausalParents, ChangeSetDraft, ChangeSetId, ChunkRef, Digest32, DirectoryEntry,
    DirectoryVersion, FileManifest, FileVersion, HeadId, Hlc, ManifestId, NameError,
    NormalizedName, Object, ObjectId, ObjectKind, PolicyEpoch, PortableMetadata, PublicKey,
    SessionId, Signature, Timestamp, VersionId, WorkspaceId,
};

fn digest(byte: u8) -> Digest32 {
    Digest32::from_bytes([byte; 32])
}

// ---------------------------------------------------------------------------------------------
// Actors
// ---------------------------------------------------------------------------------------------

/// Plan §4.2 lists seven kinds. All seven are constructible, distinct, and carry a distinct
/// spelling — the enumeration is the point, so a missing kind is a missing capability model.
#[test]
fn all_seven_actor_kinds_are_representable() {
    assert_eq!(ActorKind::ALL.len(), 7);

    let spellings: HashSet<&str> = ActorKind::ALL.iter().map(ActorKind::as_str).collect();
    assert_eq!(spellings.len(), 7, "two kinds share a spelling");

    let expected: HashSet<&str> = [
        "human",
        "device actor",
        "agent",
        "agent-run",
        "automation",
        "validator",
        "service",
    ]
    .into_iter()
    .collect();
    assert_eq!(spellings, expected);

    let mut ids = HashSet::new();
    for (index, kind) in ActorKind::ALL.into_iter().enumerate() {
        let actor = Actor::new(
            kind,
            PublicKey::from_bytes([index as u8; 32]),
            format!("{kind}"),
            Timestamp::from_unix_millis(index as u64),
        );
        assert_eq!(actor.kind(), kind);
        assert_eq!(actor.display_name(), kind.as_str());
        ids.insert(actor.id::<Blake3>());
    }
    assert_eq!(ids.len(), 7);
}

/// TG-3 in miniature: only a human may ever hold the approval capability. Stating it on the kind
/// is what lets `mesh-policy` make an agent approval unrepresentable rather than merely denied.
#[test]
fn only_a_human_may_hold_the_approval_capability() {
    for kind in ActorKind::ALL {
        assert_eq!(
            kind.may_hold_approval_capability(),
            kind == ActorKind::Human,
            "{kind}"
        );
    }
}

#[test]
fn the_mutable_parts_of_an_actor_do_not_rename_it() {
    let key = PublicKey::from_bytes([9; 32]);
    let sponsor = PublicKey::from_bytes([8; 32]).actor_id::<Blake3>();
    let actor = Actor::new(
        ActorKind::Agent,
        key,
        "planner".to_owned(),
        Timestamp::from_unix_millis(10),
    );
    let id = actor.id::<Blake3>();

    let evolved = actor
        .clone()
        .with_display_name("planner v2".to_owned())
        .with_sponsor_human(sponsor)
        .disabled_at(Timestamp::from_unix_millis(99));

    assert_eq!(evolved.id::<Blake3>(), id);
    assert_eq!(evolved.sponsor_human(), Some(&sponsor));
    assert_eq!(
        evolved.disabled_at_timestamp(),
        Some(Timestamp::from_unix_millis(99))
    );
    assert_eq!(
        actor.disabled_at_timestamp(),
        None,
        "the original is untouched"
    );
    assert_eq!(evolved.created_at(), Timestamp::from_unix_millis(10));
    assert_eq!(evolved.public_key(), &key);
}

/// A public key must never appear whole in a debug line by accident.
#[test]
fn debug_output_does_not_print_key_or_signature_material() {
    let key = PublicKey::from_bytes([0xde; 32]);
    let rendered = format!("{key:?}");
    assert!(rendered.starts_with("PublicKey(dededede"), "{rendered}");
    assert!(!rendered.contains(&"de".repeat(32)), "{rendered}");

    assert_eq!(
        format!("{:?}", Signature::from_bytes([1; 64])),
        "Signature(..)"
    );
}

// ---------------------------------------------------------------------------------------------
// Activity sessions
// ---------------------------------------------------------------------------------------------

#[test]
fn a_session_records_its_context_and_advances_monotonically() {
    let session = ActivitySession::open(
        SessionId::mint(1_700_000_000_000, [1; 10]),
        WorkspaceId::mint(1_699_000_000_000, [2; 10]),
        PublicKey::from_bytes([3; 32]).actor_id::<Blake3>(),
        HeadId::from_digest(digest(4)),
        Timestamp::from_unix_millis(100),
        CapabilityId::mint(1_700_000_000_001, [5; 10]),
    );

    assert_eq!(session.optional_intent(), None);
    assert_eq!(session.started_at(), Timestamp::from_unix_millis(100));
    assert_eq!(session.last_activity_at(), Timestamp::from_unix_millis(100));

    let later = session
        .clone()
        .with_intent("rewrite the parser".to_owned())
        .touched_at(Timestamp::from_unix_millis(200));
    assert_eq!(later.optional_intent(), Some("rewrite the parser"));
    assert_eq!(later.last_activity_at(), Timestamp::from_unix_millis(200));

    let out_of_order = later.clone().touched_at(Timestamp::from_unix_millis(150));
    assert_eq!(
        out_of_order.last_activity_at(),
        Timestamp::from_unix_millis(200),
        "an out-of-order touch must not move a session backwards"
    );

    assert_eq!(
        session.last_activity_at(),
        Timestamp::from_unix_millis(100),
        "the original session is untouched"
    );
}

// ---------------------------------------------------------------------------------------------
// Objects, names and versions
// ---------------------------------------------------------------------------------------------

#[test]
fn every_object_kind_is_representable() {
    assert_eq!(ObjectKind::ALL.len(), 3);
    for kind in ObjectKind::ALL {
        let object = Object::new(
            ObjectId::mint(1, [0; 10]),
            kind,
            ChangeSetId::from_digest(digest(1)),
        );
        assert_eq!(object.kind(), kind);
    }
}

#[test]
fn a_directory_entry_name_rejects_what_cannot_be_one() {
    assert_eq!(NormalizedName::new(""), Err(NameError::Empty));
    assert_eq!(NormalizedName::new("."), Err(NameError::Relative));
    assert_eq!(NormalizedName::new(".."), Err(NameError::Relative));
    assert_eq!(NormalizedName::new("a/b"), Err(NameError::Separator));
    assert_eq!(NormalizedName::new("a\\b"), Err(NameError::Separator));
    assert_eq!(NormalizedName::new("a\0b"), Err(NameError::Nul));

    for accepted in ["a", "..a", "a.b.c", "…", "with space", "-"] {
        assert!(NormalizedName::new(accepted).is_ok(), "{accepted}");
    }
}

#[test]
fn a_directory_version_carries_its_entries_in_sorted_order() {
    let object = ObjectId::mint(5, [0; 10]);
    let mut entries = BTreeMap::new();
    for name in ["zeta", "alpha", "mu"] {
        entries.insert(
            NormalizedName::new(name).expect("literal names are valid"),
            DirectoryEntry::new(
                ObjectId::mint(6, [0; 10]),
                VersionId::from_digest(digest(7)),
            ),
        );
    }
    let directory = DirectoryVersion::new(object, entries);
    let order: Vec<&str> = directory
        .entries()
        .keys()
        .map(NormalizedName::as_str)
        .collect();
    assert_eq!(order, ["alpha", "mu", "zeta"]);
    assert_eq!(directory.object_id(), object);

    assert!(DirectoryVersion::empty(object).entries().is_empty());
}

/// A file version and a directory version of the same object must never share an identifier, which
/// is what the two domain tags buy.
#[test]
fn a_file_version_and_a_directory_version_never_share_an_identifier() {
    let object = ObjectId::mint(11, [0; 10]);
    let file = FileVersion::new(
        object,
        Vec::new(),
        ManifestId::from_digest(digest(1)),
        PortableMetadata::default(),
        ChangeSetId::from_digest(digest(2)),
    );
    let directory = DirectoryVersion::empty(object);

    let file_id: VersionId = derive_id::<Blake3, _>(&file);
    let directory_id: VersionId = derive_id::<Blake3, _>(&directory);
    assert_ne!(file_id, directory_id);
}

#[test]
fn the_executable_bit_is_bound_by_the_file_version_identifier() {
    let build = |executable: bool| {
        FileVersion::new(
            ObjectId::mint(12, [0; 10]),
            vec![VersionId::from_digest(digest(3))],
            ManifestId::from_digest(digest(4)),
            PortableMetadata::new(executable),
            ChangeSetId::from_digest(digest(5)),
        )
    };
    let plain: VersionId = derive_id::<Blake3, _>(&build(false));
    let executable: VersionId = derive_id::<Blake3, _>(&build(true));
    assert_ne!(plain, executable);
    assert!(build(true).portable_metadata().is_executable());
    assert!(!PortableMetadata::default().is_executable());
}

// ---------------------------------------------------------------------------------------------
// Manifests
// ---------------------------------------------------------------------------------------------

#[test]
fn a_manifest_knows_whether_its_chunks_tile_the_file() {
    let contiguous = FileManifest::new(
        30,
        digest(1),
        vec![
            ChunkRef::new(digest(2), 0, 10),
            ChunkRef::new(digest(3), 10, 20),
        ],
    );
    assert!(contiguous.chunks_are_contiguous());
    assert_eq!(contiguous.byte_length(), 30);
    assert_eq!(contiguous.content_hash(), &digest(1));
    assert_eq!(contiguous.chunks().len(), 2);

    let gap = FileManifest::new(
        30,
        digest(1),
        vec![
            ChunkRef::new(digest(2), 0, 10),
            ChunkRef::new(digest(3), 11, 19),
        ],
    );
    assert!(!gap.chunks_are_contiguous());

    let overlong = FileManifest::new(29, digest(1), vec![ChunkRef::new(digest(2), 0, 30)]);
    assert!(!overlong.chunks_are_contiguous());

    let empty_file = FileManifest::new(0, digest(1), Vec::new());
    assert!(empty_file.chunks_are_contiguous());

    let lost_chunks = FileManifest::new(30, digest(1), Vec::new());
    assert!(
        !lost_chunks.chunks_are_contiguous(),
        "a manifest that lost its chunks must be detectable"
    );

    let overflowing = FileManifest::new(
        0,
        digest(1),
        vec![
            ChunkRef::new(digest(2), 0, u64::MAX),
            ChunkRef::new(digest(3), u64::MAX, 1),
        ],
    );
    assert!(!overflowing.chunks_are_contiguous());
}

// ---------------------------------------------------------------------------------------------
// ChangeSets
// ---------------------------------------------------------------------------------------------

fn draft() -> ChangeSetDraft<()> {
    ChangeSetDraft::<()>::new(
        WorkspaceId::mint(20, [0; 10]),
        PublicKey::from_bytes([21; 32]).actor_id::<Blake3>(),
        SessionId::mint(22, [0; 10]),
        ActorSequence::new(7),
        Hlc::new(23, 1),
    )
}

/// The three setters may be called in any order — the guarantee is that all three happened, not
/// that they happened in a particular sequence.
#[test]
fn the_causal_context_may_be_supplied_in_any_order() {
    let head = HeadId::from_digest(digest(24));
    let sealed_one = draft()
        .causal_parents(CausalParents::genesis())
        .base_head(head)
        .policy_epoch(PolicyEpoch::new(3))
        .seal(Vec::new(), head, Signature::from_bytes([0; 64]));
    let sealed_two = draft()
        .policy_epoch(PolicyEpoch::new(3))
        .causal_parents(CausalParents::genesis())
        .base_head(head)
        .seal(Vec::new(), head, Signature::from_bytes([0; 64]));
    let sealed_three = draft()
        .base_head(head)
        .policy_epoch(PolicyEpoch::new(3))
        .causal_parents(CausalParents::genesis())
        .seal(Vec::new(), head, Signature::from_bytes([0; 64]));

    assert_eq!(sealed_one, sealed_two);
    assert_eq!(sealed_two, sealed_three);
    assert_eq!(
        derive_id::<Blake3, _>(&sealed_one),
        derive_id::<Blake3, _>(&sealed_three)
    );
}

#[test]
fn a_sealed_changeset_reports_every_field_it_was_given() {
    let base = HeadId::from_digest(digest(30));
    let resulting = HeadId::from_digest(digest(31));
    let parent = ChangeSetId::from_digest(digest(32));
    let changeset = draft()
        .causal_parents(CausalParents::after(parent, Vec::new()))
        .base_head(base)
        .policy_epoch(PolicyEpoch::new(5))
        .seal(vec![(), ()], resulting, Signature::from_bytes([2; 64]));

    assert_eq!(changeset.actor_sequence(), ActorSequence::new(7));
    assert_eq!(changeset.causal_parents().as_slice(), [parent]);
    assert!(!changeset.causal_parents().is_genesis());
    assert_eq!(changeset.base_head(), base);
    assert_eq!(changeset.resulting_head(), resulting);
    assert_eq!(changeset.operations().len(), 2);
    assert_eq!(changeset.policy_epoch(), PolicyEpoch::new(5));
    assert_eq!(changeset.hybrid_logical_time(), Hlc::new(23, 1));
    assert_eq!(changeset.signature(), &Signature::from_bytes([2; 64]));
}

/// Genesis is said, not defaulted. A merge carries every parent it was given, in order.
#[test]
fn causal_parents_distinguish_genesis_from_a_merge() {
    assert!(CausalParents::genesis().is_genesis());
    assert!(CausalParents::genesis().as_slice().is_empty());

    let first = ChangeSetId::from_digest(digest(40));
    let second = ChangeSetId::from_digest(digest(41));
    let third = ChangeSetId::from_digest(digest(42));
    let merge = CausalParents::after(first, vec![second, third]);
    assert!(!merge.is_genesis());
    assert_eq!(merge.as_slice(), [first, second, third]);
}

/// The actor sequence exists to make an omission detectable, so it must never wrap back into a
/// number a peer has already seen.
#[test]
fn the_actor_sequence_saturates_rather_than_wrapping() {
    assert_eq!(ActorSequence::new(0).next(), ActorSequence::new(1));
    assert_eq!(
        ActorSequence::new(u64::MAX).next(),
        ActorSequence::new(u64::MAX)
    );
}
