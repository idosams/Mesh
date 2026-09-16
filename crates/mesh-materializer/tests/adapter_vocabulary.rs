//! The closed lists at the adapter seam are closed, distinct, and behave the way the published
//! contract says they do.
//!
//! # Not the conformance suite
//!
//! `crates/mesh-materializer/src/conformance.rs` and `tests/adapter-conformance.rs` are the oracle
//! three backends are graded by, and plan §14.3 puts that oracle with a different run than the one
//! that wrote the trait. Nothing here grades an adapter. These are structural facts about the
//! vocabulary itself — eighteen capabilities that are eighteen, twelve refusals that are twelve,
//! a bitset that does not lose one — held from outside the crate, so they are also evidence that
//! every name a backend needs is actually re-exported.
//!
//! The `VOC` family, which holds these enumerations against
//! `tests/compatibility/adapter/v0/vocabulary.json` in both directions, is the suite's and is not
//! here.

use std::path::Path;

use mesh_materializer::{
    ActorId, AdapterCapability, AdapterDescription, AdapterError, BoundaryReason, CapabilitySet,
    CheckpointCandidate, EventSequence, FsEvent, FsEventKind, HeadId, MaterializedView,
    MountedView, NameError, NormalizedName, ObjectId, ObjectKind, OpenHandle, OpenMode,
    PortableMetadata, VersionId, ViewAccess, ViewEntry, ViewId, WorkspaceAdapter, WorkspaceId,
    WorkspaceView, WORKSPACE_ADAPTER_CONTRACT,
};

fn assert_shared<T: Send + Sync + ?Sized>() {}

/// Both trait objects cross threads. `MNT/two-actor-views-coexist` will be the behavioural half of
/// the same claim; this is the type-level half, and it is the one that stops compiling.
#[test]
fn both_trait_objects_are_send_and_sync() {
    assert_shared::<dyn WorkspaceAdapter>();
    assert_shared::<dyn WorkspaceView>();
}

#[test]
fn every_capability_is_in_the_full_set_and_in_no_empty_one() {
    assert_eq!(AdapterCapability::ALL.len(), 18);
    assert_eq!(CapabilitySet::ALL.len(), AdapterCapability::ALL.len());
    for capability in AdapterCapability::ALL {
        assert!(CapabilitySet::ALL.contains(capability), "{capability}");
        assert!(!CapabilitySet::EMPTY.contains(capability), "{capability}");
    }
    assert!(CapabilitySet::EMPTY.is_empty());
    assert!(!CapabilitySet::ALL.is_empty());
}

/// Eighteen distinct names and eighteen distinct bits. A copy-paste in either table would make
/// two capabilities one, and every question about the shadowed one would silently be answered
/// about the other — which is exactly the silent divergence the capability probe exists to catch,
/// arriving through the vocabulary instead of through a backend.
#[test]
fn no_two_capabilities_share_a_name_or_a_bit() {
    let mut names = Vec::new();
    let mut accumulated = CapabilitySet::EMPTY;
    for (index, capability) in AdapterCapability::ALL.into_iter().enumerate() {
        assert!(
            !names.contains(&capability.as_str()),
            "two capabilities are called {capability}"
        );
        accumulated = accumulated.with(capability);
        assert_eq!(
            accumulated.len(),
            index + 1,
            "{capability} occupies a bit another capability already had"
        );
        names.push(capability.as_str());
    }
    assert_eq!(accumulated, CapabilitySet::ALL);
}

#[test]
fn a_set_declares_exactly_what_was_put_in_it_in_declaration_order() {
    let set = CapabilitySet::EMPTY
        .with(AdapterCapability::Write)
        .with(AdapterCapability::Lookup)
        .with(AdapterCapability::Read);
    assert_eq!(
        set.iter().collect::<Vec<_>>(),
        vec![
            AdapterCapability::Lookup,
            AdapterCapability::Read,
            AdapterCapability::Write
        ],
        "a set renders in declaration order, so two backends declaring the same set render alike"
    );
    assert_eq!(set.len(), 3);
    assert!(!set.contains(AdapterCapability::Enumerate));
    assert_eq!(
        set.without(AdapterCapability::Read).iter().count(),
        2,
        "removing a capability removes exactly one"
    );
    assert_eq!(
        CapabilitySet::EMPTY
            .with(AdapterCapability::Read)
            .with(AdapterCapability::Read),
        CapabilitySet::EMPTY.with(AdapterCapability::Read),
        "declaring twice declares once"
    );
}

#[test]
fn every_error_variant_names_itself_and_says_something() {
    let named = [
        AdapterError::unsupported(AdapterCapability::Symlink),
        AdapterError::NotFound,
        AdapterError::AlreadyExists,
        AdapterError::NotADirectory,
        AdapterError::IsADirectory,
        AdapterError::DirectoryNotEmpty,
        AdapterError::NameRejected(NameError::Relative),
        AdapterError::OutsideWorkspace,
        AdapterError::ReadOnly,
        AdapterError::UnknownView,
        AdapterError::WouldCycle,
        AdapterError::Backend("a message for a human, never file content".into()),
    ];
    assert_eq!(named.len(), 12);
    let mut seen = Vec::new();
    for error in &named {
        assert!(!seen.contains(&error.name()), "{} twice", error.name());
        assert!(
            !error.to_string().is_empty(),
            "{} says nothing",
            error.name()
        );
        seen.push(error.name());
    }
}

/// The name rule is wrapped, not restated: the refusal a view gives for a bad name carries the one
/// this crate already refuses it with, so there is no second copy of the rules to disagree.
#[test]
fn a_refused_name_arrives_as_the_rule_that_refused_it() {
    let rejected = NormalizedName::new("..").unwrap_err();
    assert_eq!(
        AdapterError::from(rejected),
        AdapterError::NameRejected(NameError::Relative)
    );
    assert_eq!(
        AdapterError::from(rejected).to_string(),
        NameError::Relative.to_string()
    );
}

#[test]
fn a_materialized_view_is_read_only_and_a_mounted_one_is_not() {
    let mounted = MountedView::new(
        ViewId::new(1),
        WorkspaceId::from_bytes([1; 16]),
        ActorId::from_bytes([2; 32]),
        Path::new("/mesh/alice"),
    );
    let materialized = MaterializedView::new(
        ViewId::new(2),
        HeadId::from_bytes([3; 32]),
        Path::new("/mesh/shared"),
    );
    assert_eq!(mounted.access(), ViewAccess::ReadWrite);
    assert!(!mounted.access().is_read_only());
    assert_eq!(materialized.access(), ViewAccess::ReadOnly);
    assert!(
        materialized.access().is_read_only(),
        "read-only by construction, not by a flag somebody might forget"
    );
    assert_eq!(mounted.mountpoint(), Path::new("/mesh/alice"));
    assert_eq!(materialized.target(), Path::new("/mesh/shared"));
    assert_ne!(mounted.id(), materialized.id());
}

#[test]
fn an_event_carries_a_position_and_no_clock() {
    let event = FsEvent::new(
        ViewId::new(7),
        EventSequence::new(3),
        FsEventKind::Closed,
        ObjectId::from_bytes([4; 16]),
    );
    assert_eq!(event.sequence().number(), 3);
    assert_eq!(event.kind().as_str(), "Closed");
    let candidate =
        CheckpointCandidate::new(event.view(), event.sequence(), BoundaryReason::Closed);
    assert_eq!(candidate.through(), event.sequence());
    assert_eq!(candidate.reason().as_str(), "Closed");
    assert_eq!(candidate.view(), event.view());
}

#[test]
fn the_event_and_boundary_vocabularies_are_eight_and_four_distinct_names() {
    let kinds = [
        FsEventKind::Opened,
        FsEventKind::Written,
        FsEventKind::Flushed,
        FsEventKind::Synced,
        FsEventKind::Closed,
        FsEventKind::Renamed,
        FsEventKind::Unlinked,
        FsEventKind::MetadataChanged,
    ];
    let reasons = [
        BoundaryReason::Closed,
        BoundaryReason::Synced,
        BoundaryReason::RenamedIntoPlace,
        BoundaryReason::MetadataSettled,
    ];
    let mut kind_names: Vec<&str> = kinds.iter().map(|kind| kind.as_str()).collect();
    kind_names.sort_unstable();
    kind_names.dedup();
    assert_eq!(kind_names.len(), 8);
    let mut reason_names: Vec<&str> = reasons.iter().map(|reason| reason.as_str()).collect();
    reason_names.sort_unstable();
    reason_names.dedup();
    assert_eq!(reason_names.len(), 4);
}

#[test]
fn an_open_mode_says_which_direction_it_permits() {
    assert!(OpenMode::Read.reads() && !OpenMode::Read.writes());
    assert!(OpenMode::Write.writes() && !OpenMode::Write.reads());
    assert!(OpenMode::ReadWrite.reads() && OpenMode::ReadWrite.writes());
}

/// A description reports the contract string it was handed. If the constructor stamped the current
/// one on every description, `CAP/contract-string` could never fail and the check would be a
/// decoration.
#[test]
fn a_description_reports_what_it_was_given_and_not_what_is_current() {
    let stale = AdapterDescription::new(
        "elsewhere/0",
        "mesh-workspace-adapter/999",
        CapabilitySet::EMPTY.with(AdapterCapability::Lookup),
    );
    assert_ne!(stale.contract(), WORKSPACE_ADAPTER_CONTRACT);
    assert_eq!(stale.adapter(), "elsewhere/0");
    assert!(stale.to_string().contains("Lookup"));
    assert!(AdapterDescription::new(
        "nothing/0",
        WORKSPACE_ADAPTER_CONTRACT,
        CapabilitySet::EMPTY
    )
    .to_string()
    .contains("(nothing declared)"));
}

#[test]
fn an_entry_keeps_the_metadata_it_was_built_with() {
    let entry = ViewEntry::new(
        NormalizedName::new("run.sh").unwrap(),
        ObjectId::from_bytes([5; 16]),
        ObjectKind::File,
        Some(VersionId::from_bytes([6; 32])),
        PortableMetadata::new(true),
    );
    assert!(entry.metadata().is_executable());
    assert_eq!(entry.kind(), ObjectKind::File);
    assert_eq!(entry.name().as_str(), "run.sh");
    assert!(entry.version().is_some());
}

/// Entries order by name before anything else, which is what makes `enumerate`'s stated
/// byte-lexicographic order reachable by sorting the entries themselves. `"A"` before `"a"` before
/// `"b"` is the byte order, and it is not the order a locale would give.
#[test]
fn entries_order_by_name_before_anything_else() {
    let entry = |name: &str, object: u8| {
        ViewEntry::new(
            NormalizedName::new(name).unwrap(),
            ObjectId::from_bytes([object; 16]),
            ObjectKind::File,
            None,
            PortableMetadata::default(),
        )
    };
    let mut entries = [entry("b", 0), entry("A", 9), entry("a", 1)];
    entries.sort();
    let names: Vec<&str> = entries.iter().map(|e| e.name().as_str()).collect();
    assert_eq!(names, vec!["A", "a", "b"]);
}

#[test]
fn a_handle_remembers_which_view_and_object_it_belongs_to() {
    let handle = OpenHandle::new(
        9,
        ViewId::new(4),
        ObjectId::from_bytes([7; 16]),
        OpenMode::ReadWrite,
    );
    assert_eq!(handle.handle(), 9);
    assert_eq!(handle.view(), ViewId::new(4));
    assert_eq!(handle.object(), ObjectId::from_bytes([7; 16]));
    assert!(handle.mode().writes());
    assert_eq!(ViewId::new(4).to_string(), "view 4");
}
