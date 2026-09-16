//! The kernel boundary — the half of a FUSE backend the conformance suite cannot see.
//!
//! # Why this module exists
//!
//! Design `01KZEZGDPMZ5RH7E60WDYDYYEE` names three failure modes its suite has no way to reach:
//! *"concurrent opens, partial writes interleaved with a rename, a kernel retry. Those need a
//! mounted target and belong to the FUSE task."* This module is where the third one becomes
//! reachable, because **a kernel retry is not something a caller does — it is something the
//! transport does to a caller that never asked.**
//!
//! Two mechanisms, both of them protocol rather than policy:
//!
//! | What the kernel does | What userspace owes it |
//! |---|---|
//! | Re-sends a request it already sent, carrying the same `unique` — after an interrupt, or when a reply was lost | The **recorded reply**, not a second application. A `create` applied twice answers `EEXIST` for a call that succeeded; an `unlink` applied twice answers `ENOENT` for a call that succeeded. Both are lies about work that was done. |
//! | Splits a transfer at the negotiated `max_write` and re-issues the remainder at the advanced offset | An honest short count. A backend that answers `from.len()` while storing `max_write` loses every byte past the limit, silently. |
//!
//! # What this is not
//!
//! It is not a `fuser` session and does not pretend to be one. It is the decision layer a
//! `fuser::Filesystem` implementation would call, driven here by a request stream a test writes
//! instead of by `/dev/fuse`. The dispatch is a pure function of the request and the state, which
//! is what makes replaying a request a check rather than a race.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use mesh_materializer::{
    AdapterError, NormalizedName, ObjectId, OpenHandle, PortableMetadata, WorkspaceView,
};

use crate::errno::{errno_of, Errno};

/// One request the kernel sends, in the vocabulary of the operations that are **not idempotent**.
///
/// Reads and writes are absent on purpose. A `pread`/`pwrite` at a stated offset with stated bytes
/// applied twice leaves the same file, so re-applying one is harmless and caching a reply for it
/// would only cost memory. The operations here change a name binding, and applying one of those
/// twice is the defect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KernelOp {
    /// `mknod`/`create`.
    CreateFile {
        /// The directory the name is bound in.
        parent: ObjectId,
        /// The entry name.
        name: NormalizedName,
        /// The metadata the file is created with.
        metadata: PortableMetadata,
    },
    /// `mkdir`.
    CreateDirectory {
        /// The directory the name is bound in.
        parent: ObjectId,
        /// The entry name.
        name: NormalizedName,
    },
    /// `unlink`.
    Unlink {
        /// The directory the name is bound in.
        parent: ObjectId,
        /// The entry name.
        name: NormalizedName,
    },
    /// `rmdir`.
    RemoveDirectory {
        /// The directory the name is bound in.
        parent: ObjectId,
        /// The entry name.
        name: NormalizedName,
    },
    /// `rename` within one directory.
    Rename {
        /// The directory both names are in.
        parent: ObjectId,
        /// The name that is bound now.
        from: NormalizedName,
        /// The name it takes.
        to: NormalizedName,
    },
    /// `rename` across two directories.
    Move {
        /// Where the entry is bound now.
        from_parent: ObjectId,
        /// The name it is bound under.
        from: NormalizedName,
        /// Where it is going.
        to_parent: ObjectId,
        /// The name it takes there.
        to: NormalizedName,
    },
}

/// What userspace put in the reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    /// The object the operation bound.
    Entry(ObjectId),
    /// The operation succeeded and names nothing.
    Done,
    /// The operation refused, with the error number this backend projects.
    Failed(Errno),
}

impl Reply {
    /// Whether this reply is a refusal.
    #[must_use]
    pub const fn is_failure(self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

/// The reply cache one FUSE session keeps, and the only thing standing between a re-sent request
/// and a second application of it.
#[derive(Debug, Default)]
pub struct RequestGate {
    replies: Mutex<HashMap<u64, Reply>>,
    served: AtomicU64,
    replayed: AtomicU64,
}

fn cache(replies: &Mutex<HashMap<u64, Reply>>) -> MutexGuard<'_, HashMap<u64, Reply>> {
    replies.lock().unwrap_or_else(PoisonError::into_inner)
}

impl RequestGate {
    /// An empty gate.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many requests reached the backend.
    #[must_use]
    pub fn served(&self) -> u64 {
        self.served.load(Ordering::SeqCst)
    }

    /// How many requests were answered from the cache because the kernel had sent them before.
    #[must_use]
    pub fn replayed(&self) -> u64 {
        self.replayed.load(Ordering::SeqCst)
    }

    /// Forget a `unique` the kernel has acknowledged.
    ///
    /// A real session bounds this cache; nothing here does it automatically, because a policy
    /// invented in this file would be a policy nobody could tune. Dropping an entry the kernel may
    /// still retry re-opens exactly the hole this module closes, so it is the caller's call.
    pub fn forget(&self, unique: u64) {
        cache(&self.replies).remove(&unique);
    }

    /// Answer one request.
    ///
    /// A `unique` this gate has already answered is answered again with the recorded reply, and
    /// the backend is not touched.
    pub fn dispatch(&self, unique: u64, view: &dyn WorkspaceView, op: &KernelOp) -> Reply {
        if let Some(recorded) = cache(&self.replies).get(&unique).copied() {
            self.replayed.fetch_add(1, Ordering::SeqCst);
            return recorded;
        }
        let reply = apply(view, op);
        self.served.fetch_add(1, Ordering::SeqCst);
        cache(&self.replies).insert(unique, reply);
        reply
    }
}

fn apply(view: &dyn WorkspaceView, op: &KernelOp) -> Reply {
    let outcome = match op {
        KernelOp::CreateFile {
            parent,
            name,
            metadata,
        } => view
            .create_file(*parent, name, *metadata)
            .map(|entry| Reply::Entry(entry.object())),
        KernelOp::CreateDirectory { parent, name } => view
            .create_directory(*parent, name)
            .map(|entry| Reply::Entry(entry.object())),
        KernelOp::Unlink { parent, name } => view.unlink(*parent, name).map(|()| Reply::Done),
        KernelOp::RemoveDirectory { parent, name } => {
            view.remove_directory(*parent, name).map(|()| Reply::Done)
        }
        KernelOp::Rename { parent, from, to } => {
            view.rename(*parent, from, to).map(|()| Reply::Done)
        }
        KernelOp::Move {
            from_parent,
            from,
            to_parent,
            to,
        } => view
            .move_entry(*from_parent, from, *to_parent, to)
            .map(|()| Reply::Done),
    };
    outcome.unwrap_or_else(|error| Reply::Failed(errno_of(&error)))
}

/// What one logical transfer cost in kernel requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transfer {
    transferred: usize,
    requests: usize,
}

impl Transfer {
    /// How many bytes moved.
    #[must_use]
    pub const fn transferred(self) -> usize {
        self.transferred
    }

    /// How many requests the kernel had to issue to move them.
    ///
    /// One means the backend took the whole payload. More than one means it reported a short
    /// transfer and the kernel came back for the rest — which is the retry this module exists to
    /// make observable.
    #[must_use]
    pub const fn requests(self) -> usize {
        self.requests
    }

    /// Whether the transfer needed more than one request.
    #[must_use]
    pub const fn was_retried(self) -> bool {
        self.requests > 1
    }
}

/// Write `bytes` at `offset`, re-issuing at the advanced offset until the backend stops taking.
///
/// The loop the kernel runs, run here instead. It stops on a zero-byte answer rather than
/// spinning: a backend that will take nothing is a backend that will take nothing next time, and a
/// transfer loop that could not end is a worse failure than a short write.
///
/// # Errors
///
/// Whatever [`WorkspaceView::write`] refused with, on the first request that refuses.
pub fn write_through(
    view: &dyn WorkspaceView,
    handle: &OpenHandle,
    offset: u64,
    bytes: &[u8],
) -> Result<Transfer, AdapterError> {
    let mut moved = 0usize;
    let mut requests = 0usize;
    while moved < bytes.len() {
        let taken = view.write(handle, offset + moved as u64, &bytes[moved..])?;
        requests += 1;
        if taken == 0 {
            break;
        }
        moved += taken.min(bytes.len() - moved);
    }
    Ok(Transfer {
        transferred: moved,
        requests: requests.max(1),
    })
}

/// Fill `into` from `offset`, re-issuing until it is full or the backend answers nothing.
///
/// # Errors
///
/// Whatever [`WorkspaceView::read`] refused with, on the first request that refuses.
pub fn read_through(
    view: &dyn WorkspaceView,
    handle: &OpenHandle,
    offset: u64,
    into: &mut [u8],
) -> Result<Transfer, AdapterError> {
    let mut moved = 0usize;
    let mut requests = 0usize;
    while moved < into.len() {
        let filled = view.read(handle, offset + moved as u64, &mut into[moved..])?;
        requests += 1;
        if filled == 0 {
            break;
        }
        moved += filled.min(into.len() - moved);
    }
    Ok(Transfer {
        transferred: moved,
        requests: requests.max(1),
    })
}
