//! The kernel retry — the first of the three failure modes design `01KZEZGDPMZ5RH7E60WDYDYYEE`
//! named as out of reach for the conformance suite.
//!
//! # Which of the three this file reaches, and which it does not
//!
//! The design says: *"The failure mode this suite cannot see is the one FUSE has and memory does
//! not — concurrent opens, partial writes interleaved with a rename, a kernel retry."*
//!
//! | Failure mode | Where it is exercised |
//! |---|---|
//! | **a kernel retry** | here, both mechanisms: a re-sent request, and a transfer the kernel completes in more than one request |
//! | **concurrent opens** | `tests/concurrency.rs` |
//! | **a partial write interleaved with a rename** | `tests/editor-workflows.rs`, over the save pattern that actually produces one |
//!
//! None of the three needs a mounted target to be *decided* — they need a driver that behaves like
//! a kernel, and that driver is `mesh_fuse::kernel`. What still needs a mount is the claim that
//! Linux's own kernel drives it this way; this file cannot make that claim and does not.
//!
//! Verification is **warm** (`docs/adr/0004`).

use mesh_fuse::errno::{EEXIST, ENOENT, ENOTEMPTY};
use mesh_fuse::kernel::{read_through, write_through, KernelOp, Reply, RequestGate};
use mesh_fuse::FuseAdapter;
use mesh_materializer::{
    ActorId, NormalizedName, OpenMode, PortableMetadata, WorkspaceAdapter as _, WorkspaceView,
};
use std::path::Path;

fn mounted(adapter: &FuseAdapter) -> &dyn WorkspaceView {
    let fixture = adapter.prepare_fixture().expect("the backend prepares");
    let mounted = adapter
        .mount_actor_view(
            fixture.workspace(),
            ActorId::from_bytes([0x31; 32]),
            Path::new("/retry"),
        )
        .expect("a mount");
    adapter.view(mounted.id()).expect("the view resolves")
}

fn name(text: &str) -> NormalizedName {
    NormalizedName::new(text).expect("a legal entry name")
}

// ---------------------------------------------------------------------------------------------
// 1. A re-sent request is answered, not re-applied
// ---------------------------------------------------------------------------------------------

/// The defect this whole module exists for, in one test.
///
/// The kernel sends `create`, the reply is lost, the kernel sends the **same `unique` again**.
/// Without a reply cache the second application answers `EEXIST` for a `creat(2)` that succeeded,
/// and the application is told a file it just made is in the way of itself. With one, the caller
/// gets the reply it was owed and the tree is touched once.
#[test]
fn a_re_sent_create_is_answered_from_the_cache_and_never_applied_twice() {
    let adapter = FuseAdapter::new();
    let view = mounted(&adapter);
    let gate = RequestGate::new();
    let op = KernelOp::CreateFile {
        parent: view.root(),
        name: name("retried.txt"),
        metadata: PortableMetadata::new(true),
    };

    let first = gate.dispatch(7, view, &op);
    let second = gate.dispatch(7, view, &op);
    assert_eq!(first, second, "a retry answered something else");
    assert!(!first.is_failure(), "the first create refused: {first:?}");
    assert_eq!(gate.served(), 1, "the retry reached the backend");
    assert_eq!(gate.replayed(), 1);

    // And the proof that this is a real hazard rather than a hypothetical: the same request under
    // a DIFFERENT unique — which is what a kernel that had never been told the first one succeeded
    // would send — is the answer the cache is there to prevent reaching an application.
    assert_eq!(gate.dispatch(8, view, &op), Reply::Failed(EEXIST));
    assert_eq!(gate.served(), 2);
}

/// The same hazard on the destructive half, where the lie is worse.
///
/// `unlink` applied twice answers `ENOENT` for a `unlink(2)` that succeeded. A build tool that
/// reads that as "the file was never there" is a build tool that has just been told a stale fact
/// about its own output directory.
#[test]
fn a_re_sent_unlink_is_answered_from_the_cache_and_never_applied_twice() {
    let adapter = FuseAdapter::new();
    let view = mounted(&adapter);
    let gate = RequestGate::new();
    let entry = name("unlink-me.txt");
    view.create_file(view.root(), &entry, PortableMetadata::default())
        .expect("a created file");

    let op = KernelOp::Unlink {
        parent: view.root(),
        name: entry.clone(),
    };
    assert_eq!(gate.dispatch(11, view, &op), Reply::Done);
    assert_eq!(gate.dispatch(11, view, &op), Reply::Done);
    assert_eq!(gate.replayed(), 1);
    assert_eq!(gate.dispatch(12, view, &op), Reply::Failed(ENOENT));
}

/// A refusal is cached too, and this is deliberate rather than incidental.
///
/// A retried request that legitimately failed must fail the same way. Caching only the successes
/// would make a retry of a failed `rmdir` return `Done` if the directory had emptied in between —
/// two different answers to one `unique`, which is the thing the kernel has no way to reconcile.
#[test]
fn a_cached_refusal_is_replayed_rather_than_re_decided() {
    let adapter = FuseAdapter::new();
    let view = mounted(&adapter);
    let gate = RequestGate::new();
    let directory = name("still-full");
    let inside = name("occupant.txt");
    let made = view
        .create_directory(view.root(), &directory)
        .expect("a created directory");
    view.create_file(made.object(), &inside, PortableMetadata::default())
        .expect("a created file");

    let op = KernelOp::RemoveDirectory {
        parent: view.root(),
        name: directory.clone(),
    };
    assert_eq!(gate.dispatch(21, view, &op), Reply::Failed(ENOTEMPTY));

    // The directory empties behind the kernel's back, and the retry still answers what it was
    // answered before.
    view.unlink(made.object(), &inside).expect("an unlink");
    assert_eq!(gate.dispatch(21, view, &op), Reply::Failed(ENOTEMPTY));
    assert_eq!(gate.served(), 1);

    // A fresh request decides afresh, which is the half that keeps the cache from being a lie.
    assert_eq!(gate.dispatch(22, view, &op), Reply::Done);
}

/// Forgetting a `unique` re-opens the hole, which is the evidence that the cache is what closes it.
///
/// A test that only showed the cache working could not distinguish it from an operation that
/// happened to be idempotent. This shows the two states of one mechanism.
#[test]
fn forgetting_a_unique_re_opens_exactly_the_hole_the_cache_closes() {
    let adapter = FuseAdapter::new();
    let view = mounted(&adapter);
    let gate = RequestGate::new();
    let op = KernelOp::CreateDirectory {
        parent: view.root(),
        name: name("made-once"),
    };

    assert!(!gate.dispatch(31, view, &op).is_failure());
    gate.forget(31);
    assert_eq!(gate.dispatch(31, view, &op), Reply::Failed(EEXIST));
    assert_eq!(gate.replayed(), 0);
}

// ---------------------------------------------------------------------------------------------
// 2. A transfer the kernel has to come back for
// ---------------------------------------------------------------------------------------------

/// A write larger than the negotiated `max_write` is taken in part, and the kernel completes it.
///
/// This is the shape `WorkspaceView::write`'s signature was chosen to make observable — *"a short
/// write is legal and is reported; answering `from.len()` while storing fewer is data loss"* — and
/// it is the first backend in this workspace where a short write is a real protocol event rather
/// than a hypothetical one.
#[test]
fn a_write_over_max_write_is_completed_by_the_kernel_re_issuing_it() {
    const PAYLOAD: &[u8] = b"0123456789abcdefghij";
    let adapter = FuseAdapter::with_max_write(4);
    let view = mounted(&adapter);
    let entry = view
        .create_file(
            view.root(),
            &name("chunked.txt"),
            PortableMetadata::default(),
        )
        .expect("a created file");
    let handle = view
        .open(entry.object(), OpenMode::ReadWrite)
        .expect("a handle");

    // One request takes four bytes and says so. A backend answering 20 here would have dropped 16.
    assert_eq!(view.write(&handle, 0, PAYLOAD), Ok(4));

    let transfer = write_through(view, &handle, 0, PAYLOAD).expect("the transfer completes");
    assert_eq!(transfer.transferred(), PAYLOAD.len());
    assert_eq!(transfer.requests(), 5, "20 bytes at 4 per request");
    assert!(transfer.was_retried());

    let mut buffer = [0u8; 32];
    let back =
        read_through(view, &handle, 0, &mut buffer[..PAYLOAD.len()]).expect("the read completes");
    assert_eq!(back.transferred(), PAYLOAD.len());
    assert_eq!(
        &buffer[..PAYLOAD.len()],
        PAYLOAD,
        "the bytes that came back are not the bytes that went in"
    );
    view.close(handle).expect("the handle closes");
}

/// A transfer the backend will not advance ends rather than spinning.
///
/// The loop stops on a zero-byte answer. A retry loop that could not terminate is a worse failure
/// than a short write: the first loses bytes, the second wedges the kernel thread that issued the
/// request.
#[test]
fn a_transfer_that_cannot_advance_ends_rather_than_spinning() {
    let adapter = FuseAdapter::new();
    let view = mounted(&adapter);
    let entry = view
        .create_file(view.root(), &name("empty.txt"), PortableMetadata::default())
        .expect("a created file");
    let handle = view
        .open(entry.object(), OpenMode::ReadWrite)
        .expect("a handle");

    let mut buffer = [0u8; 16];
    let transfer = read_through(view, &handle, 0, &mut buffer).expect("the read answers");
    assert_eq!(transfer.transferred(), 0);
    assert_eq!(transfer.requests(), 1);
    assert!(!transfer.was_retried());

    let nothing = write_through(view, &handle, 0, b"").expect("the write answers");
    assert_eq!(nothing.transferred(), 0);
    assert_eq!(nothing.requests(), 1);
}
