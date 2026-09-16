//! Real editor and build-tool save patterns, run against this backend — acceptance criterion 2.
//!
//! # What this file found, stated before the code rather than after it
//!
//! Two of the four save patterns below could not be expressed at `mesh-workspace-adapter/0`.
//! Contract 1 now closes atomic replacement with explicit binding evidence; contract 0 remains
//! frozen so old callers still receive `AlreadyExists` rather than silently changed semantics.
//!
//! | Pattern | What happens | Filed as |
//! |---|---|---|
//! | Write to a temporary and **rename over** the target — vim, Emacs, VS Code, `git`, every `write`-then-`rename(2)` in POSIX | Contract 1's `rename_with_evidence(.., Replace)` atomically replaces it and proves the moved object and displaced destination binding. | `01KZM96X4Y849Y23CC2SDX212H` |
//! | Save a file **shorter** than it was | `set_file_length` removes the stale tail before the editor writes the shorter bytes. | `01KZHAGG8Q83Q0YBHGDXSS8J1M` |
//! | A partial write interleaved with a rename | Works, and is the **third** of the three failure modes design `01KZEZGDPMZ5RH7E60WDYDYYEE` named as out of reach for the suite. | — |
//! | A build tree: nested directories, many outputs, a subtree moved, then removed bottom-up | Works. | — |
//!
//! Verification is **warm** (`docs/adr/0004`).

use mesh_fuse::kernel::write_through;
use mesh_fuse::FuseAdapter;
use mesh_materializer::{
    ActorId, AdapterError, DestinationBindingOutcome, EventSequence, MovedObjectIdentity,
    NormalizedName, ObjectKind, OpenMode, PortableMetadata, RenameDisposition,
    WorkspaceAdapter as _, WorkspaceView,
};
use std::path::Path;

fn name(text: &str) -> NormalizedName {
    NormalizedName::new(text).expect("a legal entry name")
}

fn workspace<'a>(adapter: &'a FuseAdapter, at: &str) -> &'a dyn WorkspaceView {
    let fixture = adapter.prepare_fixture().expect("the backend prepares");
    let mounted = adapter
        .mount_actor_view(
            fixture.workspace(),
            ActorId::from_bytes([0x81; 32]),
            Path::new(at),
        )
        .expect("a mount");
    adapter.view(mounted.id()).expect("the view resolves")
}

fn contents(view: &dyn WorkspaceView, object: mesh_materializer::ObjectId) -> Vec<u8> {
    let handle = view.open(object, OpenMode::Read).expect("a handle");
    let mut buffer = [0u8; 256];
    let filled = view.read(&handle, 0, &mut buffer).expect("a read");
    view.close(handle).expect("the handle closes");
    buffer[..filled].to_vec()
}

/// The atomic save every editor performs, now expressed by contract 1.
///
/// `write(tmp); fsync(tmp); rename(tmp, target)` is how a file is replaced without a window in
/// which it is half-written. The final step returns the separate evidence consumers need.
#[test]
fn a_temporary_can_be_atomically_renamed_over_the_document() {
    let adapter = FuseAdapter::new();
    let view = workspace(&adapter, "/atomic-save");
    let target = name("document.txt");
    let temporary = name("document.txt.mesh-tmp");

    let original = view
        .create_file(view.root(), &target, PortableMetadata::default())
        .expect("a created file");
    let handle = view
        .open(original.object(), OpenMode::ReadWrite)
        .expect("a handle");
    view.write(&handle, 0, b"version one").expect("a write");
    view.close(handle).expect("the handle closes");

    let draft = view
        .create_file(view.root(), &temporary, PortableMetadata::default())
        .expect("a created temporary");
    let handle = view
        .open(draft.object(), OpenMode::ReadWrite)
        .expect("a handle");
    view.write(&handle, 0, b"version two").expect("a write");
    view.close(handle).expect("the handle closes");

    let evidence = view
        .rename_with_evidence(
            EventSequence::new(1),
            view.root(),
            &temporary,
            &target,
            RenameDisposition::Replace,
        )
        .expect("the atomic replacement");
    assert_eq!(
        evidence.identity().moved_object(),
        MovedObjectIdentity::Preserved
    );
    assert_eq!(
        evidence.identity().destination_binding(),
        DestinationBindingOutcome::Replaced
    );
    assert_eq!(evidence.destination_after().object(), draft.object());
    assert_eq!(evidence.source_before().name(), &temporary);
    assert_eq!(
        view.lookup(view.root(), &temporary),
        Err(AdapterError::NotFound)
    );
    let saved = view
        .lookup(view.root(), &target)
        .expect("the saved document");
    assert_eq!(contents(view, saved.object()), b"version two".to_vec());
    assert_eq!(
        saved.object(),
        draft.object(),
        "the saved document is the temporary, so every handle held on the original is now orphaned"
    );
}

/// Saving a shorter file removes the tail of the longer one.
#[test]
fn saving_a_shorter_file_in_place_removes_the_old_tail() {
    const FIRST: &[u8] = b"a much longer first draft";
    const SECOND: &[u8] = b"short";

    let adapter = FuseAdapter::new();
    let view = workspace(&adapter, "/truncate");
    let created = view
        .create_file(view.root(), &name("draft.txt"), PortableMetadata::default())
        .expect("a created file");
    let handle = view
        .open(created.object(), OpenMode::ReadWrite)
        .expect("a handle");
    assert_eq!(view.write(&handle, 0, FIRST), Ok(FIRST.len()));
    view.set_file_length(created.object(), SECOND.len() as u64)
        .expect("the file is shortened before the replacement write");
    assert_eq!(view.write(&handle, 0, SECOND), Ok(SECOND.len()));
    view.close(handle).expect("the handle closes");

    let after = contents(view, created.object());
    assert_eq!(
        after, SECOND,
        "the previous version's tail survived the save"
    );
}

/// The third failure mode the conformance suite cannot reach: a partial write interleaved with a
/// rename.
///
/// The handle was taken on an **object**, and a rename changes a name binding. So the second half
/// of a transfer lands in the renamed file, and nothing appears at the name the write started
/// under. That is what a real filesystem does and it is worth pinning: a backend that resolved the
/// handle through its path each time would put the tail somewhere else, or nowhere, and no case in
/// the catalogue would notice.
#[test]
fn a_transfer_interrupted_by_a_rename_finishes_in_the_renamed_file() {
    const PAYLOAD: &[u8] = b"0123456789abcdefghij";
    let adapter = FuseAdapter::with_max_write(4);
    let view = workspace(&adapter, "/interleaved");
    let before = name("being-written.txt");
    let after = name("renamed-mid-write.txt");

    let created = view
        .create_file(view.root(), &before, PortableMetadata::default())
        .expect("a created file");
    let handle = view
        .open(created.object(), OpenMode::ReadWrite)
        .expect("a handle");

    // Half the payload, then the rename, then the rest — through the same handle.
    let head = write_through(view, &handle, 0, &PAYLOAD[..8]).expect("the first half");
    assert!(
        head.was_retried(),
        "max_write is 4, so eight bytes is two requests"
    );
    view.rename(view.root(), &before, &after)
        .expect("a rename in the middle of a save");
    let tail = write_through(view, &handle, 8, &PAYLOAD[8..]).expect("the second half");
    assert!(tail.was_retried());
    view.close(handle).expect("the handle closes");

    assert_eq!(
        view.lookup(view.root(), &before).err(),
        Some(AdapterError::NotFound),
        "the old name came back"
    );
    let renamed = view.lookup(view.root(), &after).expect("the renamed file");
    assert_eq!(renamed.object(), created.object());
    assert_eq!(
        contents(view, renamed.object()),
        PAYLOAD.to_vec(),
        "the two halves of one save did not land in one file"
    );
}

/// A build tree: nested output directories, many files, a subtree moved, then removed bottom-up.
///
/// The workflow half of acceptance criterion 2 that does complete. Nothing exotic; what it checks
/// is that the ordinary shape a compiler produces survives, including the two things a build tool
/// does that an editor does not — moving a whole output directory, and removing one from the
/// leaves up.
#[test]
fn a_build_tree_is_built_moved_and_removed_without_divergence() {
    const OUTPUTS: usize = 24;
    let adapter = FuseAdapter::new();
    let view = workspace(&adapter, "/build");

    let target = view
        .create_directory(view.root(), &name("target"))
        .expect("a created directory");
    let debug = view
        .create_directory(target.object(), &name("debug"))
        .expect("a created directory");
    for index in 0..OUTPUTS {
        let object = view
            .create_file(
                debug.object(),
                &name(&format!("unit-{index:02}.o")),
                PortableMetadata::default(),
            )
            .expect("a created output");
        let handle = view
            .open(object.object(), OpenMode::ReadWrite)
            .expect("a handle");
        view.write(&handle, 0, format!("object {index}").as_bytes())
            .expect("a write");
        view.close(handle).expect("the handle closes");
    }

    let listing = view.enumerate(debug.object()).expect("a listing");
    assert_eq!(listing.len(), OUTPUTS);
    let names: Vec<&str> = listing.iter().map(|entry| entry.name().as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    assert_eq!(names, sorted, "a build directory listed out of order");
    assert!(listing
        .iter()
        .all(|entry| entry.kind() == ObjectKind::File && entry.version().is_none()));

    // The whole output directory moves, and every output moves with it — by identity, not by name.
    let archive = view
        .create_directory(view.root(), &name("archive"))
        .expect("a created directory");
    view.move_entry(
        target.object(),
        &name("debug"),
        archive.object(),
        &name("debug-1"),
    )
    .expect("a moved subtree");
    let moved = view
        .lookup(archive.object(), &name("debug-1"))
        .expect("the moved directory");
    assert_eq!(moved.object(), debug.object());
    assert_eq!(
        view.enumerate(moved.object()).expect("a listing").len(),
        OUTPUTS
    );
    assert_eq!(
        contents(
            view,
            view.lookup(moved.object(), &name("unit-07.o"))
                .expect("an output")
                .object()
        ),
        b"object 7".to_vec(),
        "an output's bytes changed when its directory moved"
    );

    // And it comes down from the leaves. A non-empty directory is refused, never emptied.
    assert_eq!(
        view.remove_directory(archive.object(), &name("debug-1")),
        Err(AdapterError::DirectoryNotEmpty)
    );
    for index in 0..OUTPUTS {
        view.unlink(moved.object(), &name(&format!("unit-{index:02}.o")))
            .expect("an unlinked output");
    }
    view.remove_directory(archive.object(), &name("debug-1"))
        .expect("the emptied directory");
    view.remove_directory(view.root(), &name("archive"))
        .expect("the archive");
    view.remove_directory(view.root(), &name("target"))
        .expect("the target");
    assert!(view.enumerate(view.root()).expect("a listing").is_empty());
}
