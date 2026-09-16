//! The folder-watching fallback backend, graded — plan §7.4, task `01KZC2QR9VVJK6Y60PS8D360JT`.
//!
//! # What decides whether this file is worth anything
//!
//! `mesh_materializer::run_conformance_v1` is the oracle and **nothing here re-implements any of
//! it**. This file runs it against `mesh_daemon::folder_watch::DirectoryAdapter`, asserts the whole
//! grade family by family as numbers, and then checks the three things a conformance suite has no
//! way to see:
//!
//! 1. what a backend finds by re-reading a folder, including when every change notification was
//!    dropped (acceptance criteria 2 and 3);
//! 2. whether the restrictions the backend works under are the ones the product tells a person
//!    about, in both directions (acceptance criterion 1);
//! 3. that the fallback is never selected while a direct connection is available, and never
//!    selected silently (acceptance criterion 4).
//!
//! # Why it is here and not in `mesh-materializer`
//!
//! It was there until this task, because the backend was a module of that crate's test target —
//! the only place a `WorkspaceAdapter` implementation could be compiled at all. The backend now
//! ships inside `mesh-daemon`, so the grade belongs beside it: a test in another crate could not
//! import `mesh_daemon::FallbackRestriction` and had to compare the two halves as source text.
//! Everything under "the product and the backend say the same thing" below is now typed.
//!
//! # Unix only
//!
//! An object identity here is the device and inode the kernel reports. `mesh_daemon::folder_watch`
//! is gated the same way, so on any other platform this file grades nothing and says so rather
//! than reporting an empty pass.
//!
//! Plan §14.3 rule 4: this is not the authoritative oracle for the backend. `run_conformance_v1` and
//! its fifteen planted defects are, and they live in `mesh-materializer`. Verification here is
//! **warm** (`docs/adr/0004`) — the run that moved the backend ran these.

#![cfg(unix)]

use std::fs;
use std::path::Path;

use mesh_daemon::folder_watch::{watch, DirectoryAdapter, ADAPTER_NAME, DECLARED_RESTRICTIONS};
use mesh_daemon::{choose_backend, Availability, FallbackRestriction, WorkspaceBackend};
use mesh_materializer::{
    run_conformance_v1, CaseFamily, CaseResult, ConformanceReport, WorkspaceAdapter as _,
};

// ---------------------------------------------------------------------------------------------
// 1. The grade, as numbers
// ---------------------------------------------------------------------------------------------

/// Whether the volume the scratch folder is on treats two names that differ only in case as one.
///
/// Measured, never assumed. macOS ships APFS case-insensitive by default and Linux ext4 is case
/// sensitive, so a test that hard-coded either would be wrong on one of the two platforms Mesh
/// ships — invisibly, in the direction that reports a green run.
fn the_volume_folds_case() -> bool {
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let folder = std::env::temp_dir().join(format!(
        "mesh-directory-fallback-case-{}-{}-{serial}",
        std::process::id(),
        line!()
    ));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).expect("a scratch folder");
    fs::write(folder.join("case-a"), b"").expect("a file");
    let folded = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(folder.join("case-A"))
        .is_err();
    let _ = fs::remove_dir_all(&folder);
    folded
}

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
///
/// The tally is asserted rather than printed: a report nobody compares against anything is a
/// report that can quietly become "everything unsupported" and still read green. Two families are
/// deliberately not all-pass and both are stated here rather than discovered by a reader —
/// `BND` is `unsupported` because this backend does not declare `ObserveDurableBoundary`, and
/// `ORD` carries one failure on a case-folding volume that
/// `the_only_case_the_directory_backend_fails_is_one_its_volume_cannot_satisfy` accounts for.
#[test]
fn the_directory_backend_is_graded_family_by_family() {
    let backend = DirectoryAdapter::in_scratch("tally");
    let report = run_conformance_v1(&backend);
    assert_eq!(report.adapter(), ADAPTER_NAME);
    assert_eq!(report.cases().len(), 131, "{report}");

    let folded = the_volume_folds_case();
    let expected: [(CaseFamily, (usize, usize, usize)); 8] = [
        // Sixteen declared capabilities answer their declared-capability probes;
        // `ObserveDurableBoundary` does the reverse and `Symlink` has no operation to probe.
        (CaseFamily::Cap, (37, 0, 56)),
        // Every filesystem operation the contract publishes, on real files.
        (CaseFamily::Op, (23, 0, 0)),
        // A presented earlier version refuses every mutation.
        (CaseFamily::ReadOnly, (3, 0, 0)),
        // A `..` in a mountpoint and in a target is refused, and a refused name is
        // unrepresentable on the view.
        (CaseFamily::Name, (3, 0, 0)),
        (
            CaseFamily::Order,
            if folded { (2, 1, 0) } else { (3, 0, 0) },
        ),
        // Not declared. Re-reading a folder never shows an open, a flush, a close or an fsync.
        (CaseFamily::Boundary, (0, 0, 2)),
        // Two actors mount at once, and a released view stops resolving.
        (CaseFamily::Mount, (2, 0, 0)),
        (CaseFamily::Catalogue, (2, 0, 0)),
    ];
    for (family, wanted) in expected {
        assert_eq!(family_tally(&report, family), wanted, "{family}\n{report}");
    }

    let (passed, failed, unsupported) = report.tally();
    assert_eq!(
        (passed, failed, unsupported),
        if folded { (72, 1, 58) } else { (73, 0, 58) },
        "{report}"
    );
    assert_eq!(passed + failed + unsupported, 131);
}

/// The one case that fails, and the evidence that it is the fixture rather than the backend.
///
/// `ORD/enumerate-is-byte-lexicographic` creates `ord-b`, `ord-A` and `ord-a` in one directory and
/// requires all three to be listed. On a case-folding volume — the macOS default, and the platform
/// Mesh ships its file-system connection on — those are two names, not three, so the second create
/// answers `AlreadyExists` and the case cannot be satisfied by *any* correct backend.
///
/// Two claims are separated here, because a reader who saw only the failure would not know which
/// one it was:
///
/// 1. The volume really does fold case, demonstrated directly rather than inferred from the
///    report.
/// 2. The rule the case is about — ascending byte-lexicographic order of the entry names — holds
///    of this backend, over a fixture whose names do not collide under case folding.
///
/// Filed as `01KZG5ASCKFMWQDJY34WCNTCCV`. `mesh_materializer::NormalizedName` treats two names
/// that differ only in case as two names and states that it applies no Unicode normalization form;
/// what it does not state is what happens when the volume underneath disagrees, and this is the
/// first run that could find that out.
#[test]
fn the_only_case_the_directory_backend_fails_is_one_its_volume_cannot_satisfy() {
    let backend = DirectoryAdapter::in_scratch("ord");
    let report = run_conformance_v1(&backend);

    if the_volume_folds_case() {
        assert_eq!(
            report.failing_ids(),
            vec!["ORD/enumerate-is-byte-lexicographic"],
            "on a case-folding volume this is the only case that may fail\n{report}"
        );
        let detail = report
            .cases()
            .iter()
            .find(|case| case.id() == "ORD/enumerate-is-byte-lexicographic")
            .expect("the case is in the report")
            .detail()
            .to_owned();
        assert!(
            detail.contains("AlreadyExists"),
            "the failure is the fixture colliding, not an ordering defect: {detail}"
        );
    } else {
        assert!(report.is_conformant(), "{report}");
    }

    // The rule itself, on names that survive case folding.
    let workspace = mesh_materializer::WorkspaceId::from_bytes([0; 16]);
    let backend = DirectoryAdapter::in_scratch("ord-rule");
    let fixture = backend.prepare_fixture().expect("the backend prepares");
    assert_ne!(fixture.workspace(), workspace);
    let mounted = backend
        .mount_actor_view(
            fixture.workspace(),
            mesh_materializer::ActorId::from_bytes([9; 32]),
            Path::new("/ordering"),
        )
        .expect("a mount");
    let view = backend.view(mounted.id()).expect("the view resolves");
    // Four names that are still four names after case folding, and whose byte order ("Beta",
    // "Zed", "alpha", "yak") is not their case-insensitive order ("alpha", "Beta", "yak", "Zed").
    // A fixture that agreed under both would not tell the two apart.
    for text in ["yak", "Beta", "Zed", "alpha"] {
        let name = mesh_materializer::NormalizedName::new(text).expect("a legal entry name");
        view.create_file(
            view.root(),
            &name,
            mesh_materializer::PortableMetadata::default(),
        )
        .expect("a created file");
    }
    let listed: Vec<String> = view
        .enumerate(view.root())
        .expect("a listing")
        .iter()
        .map(|entry| entry.name().as_str().to_owned())
        .collect();
    let mut sorted = listed.clone();
    sorted.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    assert_eq!(listed, sorted, "enumerate is not in byte order: {listed:?}");
    assert!(
        listed.len() >= 2,
        "nothing was listed, so this proves nothing: {listed:?}"
    );
}

/// The backend refuses a workspace and a head it was never given, which is what makes every
/// mount and read-only case above a real grade rather than a lenient one.
#[test]
fn the_directory_backend_refuses_a_workspace_and_a_head_it_was_never_given() {
    use mesh_materializer::{ActorId, AdapterError, HeadId, WorkspaceId};

    let backend = DirectoryAdapter::in_scratch("strict");
    let stranger_workspace = WorkspaceId::from_bytes([0x77; 16]);
    let stranger_head = HeadId::from_bytes([0x78; 32]);
    let actor = ActorId::from_bytes([0x01; 32]);

    // Before it has prepared, it holds nothing at all — including its own identifiers.
    assert_eq!(
        backend.mount_actor_view(backend.workspace(), actor, Path::new("/somewhere")),
        Err(AdapterError::NotFound)
    );
    let fixture = backend.prepare_fixture().expect("the backend prepares");
    assert_eq!(fixture.workspace(), backend.workspace());
    assert_eq!(fixture.head(), backend.head());
    assert_eq!(
        backend.mount_actor_view(stranger_workspace, actor, Path::new("/somewhere")),
        Err(AdapterError::NotFound)
    );
    assert_eq!(
        backend
            .materialize_readonly_view(stranger_head, Path::new("/elsewhere"))
            .err(),
        Some(AdapterError::NotFound)
    );
    assert!(backend
        .mount_actor_view(fixture.workspace(), actor, Path::new("/somewhere"))
        .is_ok());
}

/// Exact length is a filesystem fact, not an in-memory view fact.
///
/// This reads the complete file after every transition and then drops and reconstructs the
/// adapter over the caller-owned root. The fresh view receives the original object identity, so
/// success proves that it found the file from disk rather than from the first view's index.
#[test]
fn exact_file_length_is_on_disk_and_survives_an_adapter_restart() {
    use mesh_materializer::{
        ActorId, NormalizedName, OpenMode, PortableMetadata, WorkspaceAdapter as _,
    };

    let root = std::env::temp_dir().join(format!(
        "mesh-directory-length-restart-{}-{}",
        std::process::id(),
        line!()
    ));
    let _ = fs::remove_dir_all(&root);
    let path = root.join("workspace/length/draft.txt");

    let object = {
        let backend = DirectoryAdapter::at(root.clone());
        let fixture = backend.prepare_fixture().expect("prepare first adapter");
        let mounted = backend
            .mount_actor_view(
                fixture.workspace(),
                ActorId::from_bytes([0x41; 32]),
                Path::new("/length"),
            )
            .expect("mount first adapter");
        let view = backend.view(mounted.id()).expect("first view");
        let created = view
            .create_file(
                view.root(),
                &NormalizedName::new("draft.txt").expect("portable name"),
                PortableMetadata::default(),
            )
            .expect("create file");
        let handle = view
            .open(created.object(), OpenMode::ReadWrite)
            .expect("open file");
        assert_eq!(
            view.write(&handle, 0, b"prefix and a stale old tail"),
            Ok(27)
        );

        view.set_file_length(created.object(), 6)
            .expect("shrink exactly");
        assert_eq!(fs::read(&path).expect("read shrunken file"), b"prefix");

        view.set_file_length(created.object(), 10)
            .expect("grow exactly");
        assert_eq!(fs::read(&path).expect("read grown file"), b"prefix\0\0\0\0");

        view.set_file_length(created.object(), 0)
            .expect("set zero length");
        assert_eq!(fs::read(&path).expect("read empty file"), b"");

        assert_eq!(view.write(&handle, 0, b"kept"), Ok(4));
        view.close(handle).expect("close file");
        created.object()
    };

    assert_eq!(
        fs::read(&path).expect("caller-owned root survived drop"),
        b"kept"
    );
    {
        let restarted = DirectoryAdapter::at(root.clone());
        let fixture = restarted
            .prepare_fixture()
            .expect("prepare restarted adapter");
        let mounted = restarted
            .mount_actor_view(
                fixture.workspace(),
                ActorId::from_bytes([0x42; 32]),
                Path::new("/length"),
            )
            .expect("mount restarted adapter");
        let view = restarted.view(mounted.id()).expect("restarted view");
        view.set_file_length(object, 2)
            .expect("fresh index resolves the on-disk object");
        assert_eq!(fs::read(&path).expect("read after restart"), b"ke");
    }
    assert!(path.exists(), "a caller-owned root was deleted on drop");
    fs::remove_dir_all(root).expect("clean caller-owned fixture");
}

/// Refusals stay distinct, an unrepresentable length never reaches the filesystem, and replacing
/// the confined path with a symlink cannot redirect truncation outside the backend-owned folder.
#[test]
fn exact_file_length_refuses_overflow_and_an_outside_symlink_without_mutation() {
    use std::os::unix::fs::symlink;

    use mesh_materializer::{
        ActorId, AdapterError, NormalizedName, OpenMode, PortableMetadata, WorkspaceAdapter as _,
    };

    let backend = DirectoryAdapter::in_scratch("length-refusals");
    let fixture = backend.prepare_fixture().expect("prepare adapter");
    let mounted = backend
        .mount_actor_view(
            fixture.workspace(),
            ActorId::from_bytes([0x43; 32]),
            Path::new("/length"),
        )
        .expect("mount adapter");
    let view = backend.view(mounted.id()).expect("writable view");
    let created = view
        .create_file(
            view.root(),
            &NormalizedName::new("inside.txt").expect("portable name"),
            PortableMetadata::default(),
        )
        .expect("create file");
    let handle = view
        .open(created.object(), OpenMode::ReadWrite)
        .expect("open file");
    assert_eq!(view.write(&handle, 0, b"unchanged"), Ok(9));
    view.close(handle).expect("close file");

    assert_eq!(
        view.set_file_length(view.root(), 0),
        Err(AdapterError::IsADirectory)
    );
    assert_eq!(
        view.set_file_length(mesh_materializer::ObjectId::from_bytes([0xff; 16]), 0),
        Err(AdapterError::NotFound)
    );
    assert_eq!(
        view.set_file_length(created.object(), u64::MAX),
        Err(AdapterError::Backend(
            "file length exceeds the platform offset range".to_owned()
        ))
    );

    let inside = backend.workspace_root().join("length/inside.txt");
    assert_eq!(
        fs::read(&inside).expect("overflow preserved file"),
        b"unchanged"
    );

    let outside = std::env::temp_dir().join(format!(
        "mesh-directory-length-outside-{}-{}",
        std::process::id(),
        line!()
    ));
    let _ = fs::remove_file(&outside);
    fs::write(&outside, b"outside must survive").expect("outside fixture");
    fs::remove_file(&inside).expect("remove confined file");
    symlink(&outside, &inside).expect("replace path with outside symlink");
    assert_eq!(
        view.set_file_length(created.object(), 0),
        Err(AdapterError::NotFound)
    );
    assert_eq!(
        fs::read(&outside).expect("read outside fixture"),
        b"outside must survive"
    );
    fs::remove_file(outside).expect("clean outside fixture");

    let readonly = backend
        .materialize_readonly_view(fixture.head(), Path::new("/readonly"))
        .expect("materialize read-only view");
    let readonly_view = backend.view(readonly.id()).expect("read-only view");
    assert_eq!(
        readonly_view.set_file_length(readonly_view.root(), 0),
        Err(AdapterError::ReadOnly)
    );
}

// ---------------------------------------------------------------------------------------------
// 6. What re-reading a folder can and cannot tell you (plan §7.4, §4.9)
// ---------------------------------------------------------------------------------------------

fn mounted_view(
    backend: &DirectoryAdapter,
    at: &str,
) -> (std::path::PathBuf, mesh_materializer::ViewId) {
    let fixture = backend.prepare_fixture().expect("the backend prepares");
    let mounted = backend
        .mount_actor_view(
            fixture.workspace(),
            mesh_materializer::ActorId::from_bytes([7; 32]),
            Path::new(at),
        )
        .expect("a mount");
    let folder = backend.workspace_root().join(at.trim_start_matches('/'));
    (folder, mounted.id())
}

fn nested_folder(root: &Path, depth: usize) -> (std::path::PathBuf, String) {
    let mut folder = root.to_path_buf();
    let mut relative = Vec::with_capacity(depth);
    for index in 0..depth {
        let component = format!("level-{index:02}");
        folder.push(&component);
        relative.push(component);
    }
    fs::create_dir_all(&folder).expect("create the complete deep tree");
    (folder, relative.join("/"))
}

/// A supported file remains visible below the old cutoff in all three readings that matter: the
/// initial snapshot, recovery after notifications were omitted, and the view's repaired object
/// index. Restoring either old `depth > 64` return makes this regression fail.
#[test]
fn a_file_below_depth_64_is_discovered_initially_and_during_recovery() {
    use mesh_materializer::{AttributionConfidence, PortableMetadata, WorkspaceAdapter as _};
    use watch::{reconcile, Snapshot, WatchedChange};

    let backend = DirectoryAdapter::in_scratch("deep-tree");
    let (folder, view_id) = mounted_view(&backend, "/deep");
    let (deepest, relative_folder) = nested_folder(&folder, 66);
    let existing = deepest.join("existing.txt");
    fs::write(&existing, b"before").expect("write the initial deep file");
    let existing_relative = format!("{relative_folder}/existing.txt");

    let before = Snapshot::of(&folder);
    assert!(
        before.get(&existing_relative).is_some(),
        "the initial reading omitted {existing_relative}"
    );

    let object = mesh_daemon::folder_watch::object_of(
        &fs::symlink_metadata(&existing).expect("deep file metadata"),
    );
    assert_eq!(
        backend.view(view_id).expect("deep view").metadata(object),
        Ok(PortableMetadata::default()),
        "the repaired object index stopped before the deep file"
    );

    // No notification is delivered for either change. The later complete reading must recover
    // both an edit and an arrival beneath the old limit.
    fs::write(&existing, b"after!").expect("change the deep file at the same length");
    let appeared = deepest.join("appeared.txt");
    fs::write(&appeared, b"new").expect("create a second deep file");
    let appeared_relative = format!("{relative_folder}/appeared.txt");

    let after = Snapshot::of(&folder);
    assert!(after.get(&appeared_relative).is_some());
    let recovered = reconcile(&before, &after);
    assert!(
        recovered.iter().any(|change| {
            matches!(
                change.change(),
                WatchedChange::ContentChanged { path, .. } if path == &existing_relative
            ) && change.confidence() == AttributionConfidence::RecoveryDetected
        }),
        "the omitted deep edit was not recovered: {recovered:?}"
    );
    assert!(
        recovered.iter().any(|change| {
            matches!(
                change.change(),
                WatchedChange::Appeared { path, .. } if path == &appeared_relative
            ) && change.confidence() == AttributionConfidence::RecoveryDetected
        }),
        "the omitted deep arrival was not recovered: {recovered:?}"
    );
}

/// Acceptance criterion 3: a missed change is recoverable by re-reading, and nothing already
/// acknowledged is lost doing it.
///
/// Every change below is made with `std::fs` directly — behind the backend's back, with **every**
/// notification dropped, which is the worst case a watcher has rather than a plausible one. The
/// reconciliation is a pure function of the two readings, so it recovers the same set whether one
/// notification was dropped or all of them were.
#[test]
fn a_missed_change_is_recovered_by_re_reading_the_folder() {
    use watch::{reconcile, Snapshot, WatchedChange};

    let backend = DirectoryAdapter::in_scratch("recover");
    let (folder, _) = mounted_view(&backend, "/recover");

    // Work Mesh has already told the person is saved privately. It is not touched below, and the
    // point of the test is that it is still exactly as it was afterwards.
    fs::write(
        folder.join("acknowledged.txt"),
        b"work that was reported saved",
    )
    .expect("a file");
    fs::write(folder.join("renamed-away.txt"), b"same bytes, new name").expect("a file");
    fs::write(folder.join("edited.txt"), b"aaaa").expect("a file");
    fs::write(folder.join("removed.txt"), b"gone").expect("a file");

    let before = Snapshot::of(&folder);
    assert_eq!(before.len(), 4);

    // Five changes, none of them observed.
    fs::rename(folder.join("renamed-away.txt"), folder.join("arrived.txt")).expect("a rename");
    fs::write(folder.join("edited.txt"), b"bbbb").expect("an edit of the same length");
    fs::remove_file(folder.join("removed.txt")).expect("a removal");
    fs::write(folder.join("appeared.txt"), b"new").expect("a new file");
    fs::create_dir(folder.join("new-folder")).expect("a new folder");

    let after = Snapshot::of(&folder);
    let recovered = reconcile(&before, &after);
    let kinds: Vec<&str> = recovered
        .iter()
        .map(|change| change.change().kind())
        .collect();
    let paths: Vec<&str> = recovered
        .iter()
        .map(|change| change.change().path())
        .collect();
    assert_eq!(
        recovered.len(),
        5,
        "five changes were made and {} were recovered: {kinds:?} {paths:?}",
        recovered.len()
    );
    assert!(kinds.contains(&"moved-inferred"), "{kinds:?} {paths:?}");
    assert_eq!(
        kinds.iter().filter(|kind| **kind == "appeared").count(),
        2,
        "the new file and the new folder: {kinds:?} {paths:?}"
    );
    assert!(kinds.contains(&"content-changed"), "{kinds:?} {paths:?}");
    assert!(kinds.contains(&"disappeared"), "{kinds:?} {paths:?}");

    // Two readings of one folder, two identical answers. The reconciliation reads no clock, so
    // running it again is a determinism check rather than a coincidence.
    assert_eq!(recovered, reconcile(&before, &after));

    // Nothing acknowledged was lost: the file is not in the change set, and its bytes are the
    // bytes that were there.
    assert!(
        !paths.contains(&"acknowledged.txt"),
        "work that nobody touched was reported as changed: {paths:?}"
    );
    assert_eq!(
        fs::read(folder.join("acknowledged.txt")).expect("the file is still there"),
        b"work that was reported saved".to_vec()
    );
    assert_eq!(
        before.get("acknowledged.txt").map(|entry| entry.object()),
        after.get("acknowledged.txt").map(|entry| entry.object()),
        "the object identity of untouched work moved"
    );

    // The move is worked out, never watched: `RecoveryDetected`, and not an observation.
    let inferred = recovered
        .iter()
        .find(|change| matches!(change.change(), WatchedChange::MovedInferred { .. }))
        .expect("the move is in the change set");
    assert_eq!(
        inferred.confidence(),
        mesh_materializer::AttributionConfidence::RecoveryDetected
    );
    assert!(!inferred.is_exact());
}

/// The native managed-folder layout keeps the journal, index and CAS under a reserved `.mesh`
/// directory. A fallback reading must not turn the daemon's own durable writes into user edits or
/// recursively checkpoint its checkpoint. The reservation applies only at the workspace root;
/// the same ordinary name below a user directory remains visible content.
#[test]
fn private_mesh_storage_is_not_observed_as_user_work() {
    use watch::{reconcile, Snapshot};

    let backend = DirectoryAdapter::in_scratch("private-storage");
    let (folder, _) = mounted_view(&backend, "/private-storage");
    fs::create_dir_all(folder.join(".mesh/chunks")).expect("private CAS");
    fs::create_dir_all(folder.join("src/.mesh")).expect("nested user folder");
    fs::write(folder.join("visible.txt"), b"visible").expect("user file");
    fs::write(folder.join(".mesh/records.mesh"), b"private journal").expect("private journal");
    fs::write(folder.join(".mesh/chunks/private"), b"private content").expect("private chunk");
    fs::write(folder.join("src/.mesh/user.txt"), b"nested user content")
        .expect("nested user content");

    let before = Snapshot::of(&folder);
    assert_eq!(
        before.paths(),
        vec!["src", "src/.mesh", "src/.mesh/user.txt", "visible.txt"]
    );

    fs::write(folder.join(".mesh/records.mesh"), b"new private journal")
        .expect("advance private journal");
    fs::write(folder.join(".mesh/chunks/another"), b"more private content")
        .expect("promote private chunk");
    let after = Snapshot::of(&folder);

    assert_eq!(before, after);
    assert!(reconcile(&before, &after).is_empty());
}

/// The `short-lived-work-is-missed` restriction, held to rather than asserted in a doc comment.
#[test]
fn a_file_that_lived_and_died_between_two_readings_is_never_seen() {
    use watch::{reconcile, Snapshot};

    let backend = DirectoryAdapter::in_scratch("omit");
    let (folder, _) = mounted_view(&backend, "/omit");
    fs::write(folder.join("stays.txt"), b"unchanged").expect("a file");

    let before = Snapshot::of(&folder);
    fs::write(folder.join("here-and-gone.txt"), b"briefly").expect("a file");
    fs::remove_file(folder.join("here-and-gone.txt")).expect("a removal");
    let after = Snapshot::of(&folder);

    assert!(
        reconcile(&before, &after).is_empty(),
        "a file that lived and died between two readings left a trace, so the omission this \
         backend declares is not the omission it has"
    );
    assert_eq!(before, after);
}

/// The `changes-are-found-late` restriction: several edits between two readings arrive as one.
#[test]
fn several_edits_between_two_readings_arrive_as_one_change() {
    use watch::{reconcile, Snapshot};

    let backend = DirectoryAdapter::in_scratch("coalesce");
    let (folder, _) = mounted_view(&backend, "/coalesce");
    fs::write(folder.join("draft.txt"), b"one").expect("a file");

    let before = Snapshot::of(&folder);
    for text in [b"two".as_slice(), b"three", b"four", b"five"] {
        fs::write(folder.join("draft.txt"), text).expect("an edit");
    }
    let after = Snapshot::of(&folder);

    let coalesced = reconcile(&before, &after);
    assert_eq!(
        coalesced.len(),
        1,
        "four edits between two readings arrived as {} changes",
        coalesced.len()
    );
    assert_eq!(coalesced[0].change().kind(), "content-changed");
    assert!(!coalesced[0].is_exact());
}

/// Acceptance criterion 2, over the whole cross product rather than one example.
///
/// A move worked out by comparing two readings answers an inference for **every** source a caller
/// could claim, and a move Mesh performed itself answers an observation. There is no value of
/// `AttributedChange` that pairs a worked-out move with an exact confidence, and this walks all of
/// them rather than trusting the doc comment that says so.
#[test]
fn a_worked_out_move_can_never_be_recorded_as_an_exact_operation() {
    use mesh_materializer::{AttributionConfidence, ObjectId};
    use watch::{AttributedChange, ChangeSource, WatchedChange};

    let object = ObjectId::from_bytes([3; 16]);
    let worked_out = WatchedChange::MovedInferred {
        from: "before.txt".to_owned(),
        to: "after.txt".to_owned(),
        object,
    };
    let performed = WatchedChange::Moved {
        from: "before.txt".to_owned(),
        to: "after.txt".to_owned(),
        object,
    };

    for source in [ChangeSource::PerformedByMesh, ChangeSource::FoundByRescan] {
        let attributed = match source {
            ChangeSource::PerformedByMesh => {
                AttributedChange::performed_by_mesh(worked_out.clone())
            }
            ChangeSource::FoundByRescan => AttributedChange::found_by_rescan(worked_out.clone()),
        };
        assert_eq!(
            attributed.confidence(),
            AttributionConfidence::RecoveryDetected,
            "a worked-out move claimed as {source:?} answered an observation"
        );
        assert!(!attributed.is_exact(), "{source:?}");
    }

    assert!(AttributedChange::performed_by_mesh(performed.clone()).is_exact());
    assert_eq!(
        AttributedChange::performed_by_mesh(performed).confidence(),
        AttributionConfidence::ExactFilesystemRange
    );

    // And the four inferences of plan §4.9 stay four: nothing here widens the vocabulary.
    assert_eq!(AttributionConfidence::ALL.len(), 6);
    assert_eq!(
        AttributionConfidence::ALL
            .into_iter()
            .filter(|confidence| watch::is_observed(*confidence))
            .count(),
        2
    );
}

/// The object index is a cache, and re-reading the folder repairs it.
///
/// This is the reconciliation path inside the adapter rather than beside it: a file moved behind
/// the backend's back is found again by identity, so an object the caller already holds does not
/// become unreachable because a change notification never arrived.
#[test]
fn the_object_index_is_repaired_by_re_reading_the_folder() {
    use mesh_materializer::{NormalizedName, PortableMetadata};

    let backend = DirectoryAdapter::in_scratch("repair");
    let (folder, view_id) = mounted_view(&backend, "/repair");
    let view = backend.view(view_id).expect("the view resolves");

    let name = NormalizedName::new("moved-behind-our-back.txt").expect("a legal name");
    let created = view
        .create_file(view.root(), &name, PortableMetadata::new(true))
        .expect("a created file");
    assert_eq!(
        view.metadata(created.object()),
        Ok(PortableMetadata::new(true))
    );

    fs::create_dir(folder.join("elsewhere")).expect("a folder");
    fs::rename(
        folder.join("moved-behind-our-back.txt"),
        folder.join("elsewhere/still-here.txt"),
    )
    .expect("a rename nobody told the backend about");

    // The cached path is wrong, the object is not: re-reading finds it, with its metadata intact.
    assert_eq!(
        view.metadata(created.object()),
        Ok(PortableMetadata::new(true)),
        "an object moved behind the backend's back became unreachable"
    );
    let elsewhere = view
        .lookup(
            view.root(),
            &NormalizedName::new("elsewhere").expect("a legal name"),
        )
        .expect("the new folder is visible");
    let found = view
        .lookup(
            elsewhere.object(),
            &NormalizedName::new("still-here.txt").expect("a legal name"),
        )
        .expect("the moved file is visible under its new name");
    assert_eq!(
        found.object(),
        created.object(),
        "the object identity did not survive a move made outside Mesh"
    );
}

// ---------------------------------------------------------------------------------------------
// 4. The backend and the product surface say the same thing (acceptance criteria 1 and 4)
//
// Until the workspace edge landed this section compared SOURCE TEXT: the backend lived in another
// crate's test target and could not import `FallbackRestriction`, so a test read `fallback.rs`
// with a string matcher and hoped the two hand-kept lists still agreed. Everything below is typed
// now, and two of the four checks below are things a source-text lint could not have made at all.
// ---------------------------------------------------------------------------------------------

/// Acceptance criterion 1, first half: the backend and the product name one set of restrictions.
///
/// It is one list by construction — `DECLARED_RESTRICTIONS` is `FallbackRestriction::ALL` mapped
/// through `id()` — so what this adds is the two things construction does not give: the order is
/// the order a person is shown, and no identifier is repeated. A duplicate would make the count
/// right and the seventh restriction unreachable.
#[test]
fn the_backend_and_the_product_declare_one_set_of_restrictions() {
    let published: Vec<&str> = FallbackRestriction::ALL
        .iter()
        .map(|restriction| restriction.id())
        .collect();
    assert_eq!(published, DECLARED_RESTRICTIONS.to_vec());
    assert_eq!(DECLARED_RESTRICTIONS.len(), 7);

    let mut unique = DECLARED_RESTRICTIONS.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        DECLARED_RESTRICTIONS.len(),
        "two restrictions share an identifier, so one of them can never be told apart: {:?}",
        DECLARED_RESTRICTIONS
    );
}

/// Acceptance criterion 1, second half: every restriction reaches a person as a sentence.
///
/// A list of machine names is not a surface. This asserts the wording exists, that it is the
/// wording `user_messages` holds — the one file `tools/program/vocab-lint/surfaces.json` scans —
/// and that no two restrictions share one, because two restrictions rendered identically are one
/// restriction as far as the reader is concerned.
#[test]
fn every_declared_restriction_reaches_a_person_as_its_own_sentence() {
    let mut sentences: Vec<&str> = Vec::new();
    for restriction in FallbackRestriction::ALL {
        let headline = restriction.headline();
        assert!(
            !headline.is_empty(),
            "{} has no sentence a person could read",
            restriction.id()
        );
        assert!(
            headline.len() > 40,
            "{} is a label and not a sentence: {headline}",
            restriction.id()
        );
        sentences.push(headline);
    }
    sentences.sort_unstable();
    sentences.dedup();
    assert_eq!(sentences.len(), FallbackRestriction::ALL.len());

    // The announcement is the sentence that makes the choice audible at all, and it is not one of
    // the seven.
    assert_eq!(
        choose_backend(Availability::NoDirectConnection).announcement(),
        Some(mesh_daemon::user_messages::FALLBACK_IN_USE)
    );
}

/// Acceptance criterion 4, and the thing a source-text lint could never have decided: what a
/// person actually gets when this build chooses.
///
/// `Availability::probe` is what `meshd` and `meshctl restrictions` both call, so this is the
/// answer a person receives rather than a hypothetical one. Every restriction the backend works
/// under is in it, in order, each with its sentence.
#[test]
fn the_choice_this_build_makes_carries_every_restriction_and_its_sentence() {
    let choice = choose_backend(Availability::probe());
    assert!(!choice.is_silent_fallback());
    assert_eq!(choice.backend(), WorkspaceBackend::FolderWatch);
    assert!(!choice.backend().is_authoritative());

    let announced: Vec<&str> = choice
        .restrictions()
        .iter()
        .map(|restriction| restriction.id())
        .collect();
    assert_eq!(announced, DECLARED_RESTRICTIONS.to_vec());

    let rendered = choice.to_json().encode();
    for restriction in FallbackRestriction::ALL {
        assert!(
            rendered.contains(restriction.id()),
            "{} never reaches the wire",
            restriction.id()
        );
        assert!(
            rendered.contains(restriction.headline()),
            "{} reaches the wire as a machine name with no sentence",
            restriction.id()
        );
    }
    assert!(rendered.contains("\"authoritative\":false"), "{rendered}");

    // And the terminal rendering, which is what `meshd` writes to standard error.
    let said = format!("{choice}");
    for restriction in FallbackRestriction::ALL {
        assert!(
            said.contains(restriction.headline()),
            "{} is not said to a person starting the service",
            restriction.id()
        );
    }
    assert!(said.starts_with(mesh_daemon::user_messages::FALLBACK_IN_USE));
}

/// Acceptance criterion 4's negative, over both arms rather than the one this build takes.
///
/// A direct connection carries no announcement and no restrictions; the fallback carries both.
/// There is no third value, because `choose_backend` is a total function of a two-valued
/// enumeration and has no argument a caller could use to prefer the fallback.
#[test]
fn the_fallback_is_never_chosen_over_an_available_direct_connection() {
    let direct = choose_backend(Availability::DirectConnection);
    assert_eq!(direct.backend(), WorkspaceBackend::DirectConnection);
    assert!(direct.backend().is_authoritative());
    assert!(direct.restrictions().is_empty());
    assert_eq!(direct.announcement(), None);
    assert!(!direct.is_silent_fallback());
    assert_eq!(
        format!("{direct}"),
        mesh_daemon::user_messages::DIRECT_CONNECTION_IN_USE,
        "a direct connection said something other than the one sentence it owes"
    );

    for availability in [
        Availability::DirectConnection,
        Availability::NoDirectConnection,
    ] {
        assert!(
            !choose_backend(availability).is_silent_fallback(),
            "{availability:?}"
        );
    }
}

/// `Availability::probe` answers a constant, and this is the evidence for the reason it gives.
///
/// The reason is that no direct-connection adapter is linked into this service. That is a fact
/// about `Cargo.toml`, so this reads `Cargo.toml`: the day somebody adds `mesh-fuse` or
/// `mesh-fskit-ffi` as a dependency and leaves `probe` answering `NoDirectConnection`, a person
/// would be told Mesh is watching their folder by a build that could have connected to it.
#[test]
fn the_probe_reports_no_direct_connection_because_none_is_linked_in() {
    let manifest = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("this crate's own manifest");
    let dependencies = manifest
        .split_once("[dependencies]")
        .expect("the manifest declares dependencies")
        .1;
    for crate_name in mesh_daemon::fallback::DIRECT_CONNECTION_CRATES {
        assert!(
            !dependencies.contains(&format!("\n{crate_name} =")),
            "{crate_name} is a dependency now, so Availability::probe must stop answering \
             NoDirectConnection unconditionally"
        );
    }
    assert_eq!(Availability::probe(), Availability::NoDirectConnection);
}

/// The two writings-down of "this confidence is an observation" agree.
///
/// `mesh-operations` publishes the predicate; `mesh-materializer` mirrors the enumeration without
/// it, because that crate may declare no dependency, and `folder_watch::watch::is_observed` is a
/// third copy for the same reason. Two copies of one rule is a cost this holds down rather than
/// hides — and it is held down against source text because `mesh-daemon` does not depend on
/// `mesh-operations` either.
#[test]
fn the_two_writings_of_observed_confidence_agree() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the workspace root is two directories above this crate");
    let operations = fs::read_to_string(root.join("crates/mesh-operations/src/operation.rs"))
        .expect("mesh-operations is readable");
    let anchor = operations
        .find("pub const fn is_observed")
        .expect("mesh-operations publishes the predicate");
    let body = &operations[anchor..anchor + 400];
    for named in ["ExactIntegratedRead", "ExactFilesystemRange"] {
        assert!(
            body.contains(named),
            "mesh-operations no longer treats {named} as an observation, and the copy in \
             crates/mesh-daemon/src/folder_watch/watch.rs still does"
        );
    }
    for not_named in [
        "FilesystemReadAhead",
        "ProcessInferred",
        "RecoveryDetected",
        "Unknown",
    ] {
        assert!(
            !body.contains(not_named),
            "mesh-operations now treats {not_named} as an observation, and the copy in \
             crates/mesh-daemon/src/folder_watch/watch.rs does not"
        );
    }
}
