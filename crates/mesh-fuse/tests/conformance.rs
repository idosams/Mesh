//! The FUSE adapter, graded — task `01KZC2ZTN3YXE6NM270T001RAK`.
//!
//! # What decides whether this file is worth anything
//!
//! `mesh_materializer::run_conformance` is the oracle and **nothing here re-implements any of
//! it**. This file runs it against `mesh_fuse::FuseAdapter`, asserts the whole grade family by
//! family as numbers, and states the two places where this backend's grade differs from the
//! folder-watching fallback's — because a report that only says CONFORMANT hides the two facts
//! that are actually interesting.
//!
//! Plan §14.3 rule 4: **this is not the authoritative oracle for this backend.**
//! `run_conformance` and its planted defects are, and they live in `mesh-materializer`.
//! Verification here is **warm** (`docs/adr/0004`) — the run that wrote the adapter ran these.

use mesh_fuse::{FuseAdapter, ADAPTER_NAME, DECLARED};
use mesh_materializer::{
    run_conformance_v1, AdapterCapability, CaseFamily, CaseResult, ConformanceReport,
    WorkspaceAdapter as _,
};

fn family_tally(report: &ConformanceReport, family: CaseFamily) -> (usize, usize, usize) {
    let count = |wanted: CaseResult| {
        report
            .cases()
            .iter()
            .filter(|case| case.rule().family() == family && case.result() == wanted)
            .count()
    };
    (
        count(CaseResult::Pass),
        count(CaseResult::Fail),
        count(CaseResult::Unsupported),
    )
}

/// The whole grade, family by family, as numbers this file has to keep true.
#[test]
fn the_fuse_adapter_is_graded_family_by_family() {
    let report = run_conformance_v1(&FuseAdapter::new());
    assert_eq!(report.adapter(), ADAPTER_NAME);
    assert_eq!(report.cases().len(), 131, "{report}");

    let expected: [(CaseFamily, (usize, usize, usize)); 8] = [
        // Seventeen capabilities are declared. `Symlink` remains reserved and wholly
        // unprobeable.
        (CaseFamily::Cap, (36, 0, 57)),
        // Every filesystem operation the contract publishes, including exact file length.
        (CaseFamily::Op, (23, 0, 0)),
        // A presented head refuses every mutation, including set_metadata.
        (CaseFamily::ReadOnly, (3, 0, 0)),
        // A `..` in a mountpoint and in a target is refused, and a refused entry name is
        // unrepresentable on the view.
        (CaseFamily::Name, (3, 0, 0)),
        // Three passes where the folder-watching fallback scores (2, 1, 0) on macOS. Nothing here
        // is on a volume, so `ord-A` and `ord-a` are two names rather than one.
        (CaseFamily::Order, (3, 0, 0)),
        // The difference this backend exists to make: the kernel hands a FUSE session the open,
        // the flush, the fsync and the release, so a durable boundary is observable rather than
        // guessed. The fallback scores (0, 0, 2) here and says why.
        (CaseFamily::Boundary, (2, 0, 0)),
        // Two actors mount at once, and a released view stops resolving.
        (CaseFamily::Mount, (2, 0, 0)),
        (CaseFamily::Catalogue, (2, 0, 0)),
    ];
    for (family, wanted) in expected {
        assert_eq!(family_tally(&report, family), wanted, "{family}\n{report}");
    }

    let (passed, failed, unsupported) = report.tally();
    assert_eq!((passed, failed, unsupported), (74, 0, 57), "{report}");
    assert_eq!(passed + failed + unsupported, 131);
    assert!(report.is_conformant(), "{report}");
    assert!(report.failing_ids().is_empty(), "{report}");
}

/// Two runs of two fresh adapters produce one report.
///
/// The suite reads no clock, no file and no environment, so this is a determinism check on the
/// **backend** rather than on the suite: an adapter whose identifiers or ordering came from
/// anything ambient would answer differently the second time.
#[test]
fn two_runs_of_two_fresh_adapters_produce_one_report() {
    let first = run_conformance_v1(&FuseAdapter::new()).to_string();
    let second = run_conformance_v1(&FuseAdapter::new()).to_string();
    assert_eq!(first, second);
}

/// The two capabilities that are not declared, and the reason they are not.
#[test]
fn symlink_is_the_only_capability_this_backend_does_not_declare() {
    let missing: Vec<AdapterCapability> = AdapterCapability::ALL
        .into_iter()
        .filter(|capability| !DECLARED.contains(*capability))
        .collect();
    assert_eq!(missing, vec![AdapterCapability::Symlink]);
    assert_eq!(DECLARED.len(), 17);
}

/// The backend refuses a workspace and a head it was never given, which is what makes every mount
/// and read-only case above a real grade rather than a lenient one.
#[test]
fn the_adapter_refuses_a_workspace_and_a_head_it_was_never_given() {
    use mesh_materializer::{ActorId, AdapterError, HeadId, WorkspaceId};
    use std::path::Path;

    let adapter = FuseAdapter::new();
    let actor = ActorId::from_bytes([0x01; 32]);

    // Before it has prepared it holds nothing at all — including its own identifiers.
    assert_eq!(
        adapter.mount_actor_view(adapter.workspace(), actor, Path::new("/somewhere")),
        Err(AdapterError::NotFound)
    );
    let fixture = adapter.prepare_fixture().expect("the backend prepares");
    assert_eq!(fixture.workspace(), adapter.workspace());
    assert_eq!(fixture.head(), adapter.head());
    assert_eq!(
        adapter.mount_actor_view(
            WorkspaceId::from_bytes([0x77; 16]),
            actor,
            Path::new("/somewhere")
        ),
        Err(AdapterError::NotFound)
    );
    assert_eq!(
        adapter
            .materialize_readonly_view(HeadId::from_bytes([0x78; 32]), Path::new("/elsewhere"))
            .err(),
        Some(AdapterError::NotFound)
    );
    assert!(adapter
        .mount_actor_view(fixture.workspace(), actor, Path::new("/somewhere"))
        .is_ok());
}

/// Two mounts of ONE actor are two views onto one state, and two actors share nothing.
///
/// `MNT/two-actor-views-coexist` checks that two identifiers coexist; it cannot check which state
/// each of them reaches, because the trait publishes no way to ask. This does, and it is the fact
/// `01KZC31Q6MXET4FN828VNQE8PJ` (per-actor mounts and read-only shadow views) is built on.
#[test]
fn a_second_mount_of_one_actor_sees_that_actor_state_and_no_other() {
    use mesh_materializer::{ActorId, NormalizedName, PortableMetadata};
    use std::path::Path;

    let adapter = FuseAdapter::new();
    let fixture = adapter.prepare_fixture().expect("the backend prepares");
    let actor = ActorId::from_bytes([0x21; 32]);
    let stranger = ActorId::from_bytes([0x22; 32]);
    let mount = |who, at| {
        adapter
            .mount_actor_view(fixture.workspace(), who, Path::new(at))
            .expect("a mount")
    };

    let first = mount(actor, "/one");
    let second = mount(actor, "/two");
    let other = mount(stranger, "/three");
    assert_ne!(first.id(), second.id());

    let name = NormalizedName::new("shared-through-two-mounts.txt").expect("a legal name");
    let one = adapter.view(first.id()).expect("the first view resolves");
    let two = adapter.view(second.id()).expect("the second view resolves");
    let theirs = adapter.view(other.id()).expect("the third view resolves");

    let created = one
        .create_file(one.root(), &name, PortableMetadata::new(true))
        .expect("a created file");
    assert_eq!(
        two.lookup(two.root(), &name).map(|entry| entry.object()),
        Ok(created.object()),
        "a second mountpoint of one actor did not see that actor's own work"
    );
    assert_eq!(
        theirs.lookup(theirs.root(), &name).err(),
        Some(mesh_materializer::AdapterError::NotFound),
        "one actor's work is visible in another actor's mount"
    );
    assert_ne!(one.root(), theirs.root(), "two actors share a root object");
}
