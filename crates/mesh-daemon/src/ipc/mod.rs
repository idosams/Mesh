//! The local IPC surface: the one door the desktop application goes through.
//!
//! # The shape of it
//!
//! | Module | What it owns |
//! |---|---|
//! | [`json`] | The JSON subset on the wire, with a strict reader and a deterministic writer |
//! | [`message`] | The message shapes, their fixed key order, and bounded multi-frame transport |
//! | [`surface`] | The versioned method catalogue, the version negotiation, the dispatch seam |
//! | [`events`] | The bounded, sequence-numbered feed a subscribed connection is pushed |
//! | [`server`] | The Unix-domain socket transport |
//!
//! # Why the surface is narrow on purpose
//!
//! Plan §8.3's last bullet says the user interface may not write the database directly. That is
//! only enforceable if there is one narrow place it has to go through instead, and
//! [`surface::METHODS`] is that place: twelve methods, each answered from state the daemon actually
//! holds. `crates/mesh-daemon/ipc-contract.json` publishes the same table plus a corpus of exact
//! wire lines, and both ends check themselves against it — the daemon in
//! `crates/mesh-daemon/tests/ipc.rs`, the desktop client in
//! `apps/desktop/src/ipc/contract.test.ts`. Two implementations of one format drift the day
//! nothing compares them.
//!
//! # What this surface does not do yet, stated rather than stubbed
//!
//! - **No capability check.** `mesh-policy` owns capabilities and nothing on this socket consults
//!   it. Access control here is the socket file's directory mode and nothing more.
//! - **No named-pipe backend.** [`server`] is `unix`-only; see its module documentation.
//! - **No general write path.** Surface v3 admits only the folder-import transaction already
//!   bounded by exact preview identity, receipt-owned rollback and private-history durability.
//!   Arbitrary changes and restore execution remain unavailable until actor and policy authority
//!   are composed; restore is preview-only.
//! - **No unverified shared version.** The daemon derives private state and file names from records
//!   and verified payloads on disk. A configured public key and a checked signature are not human
//!   authority. Moving the shared version also needs mesh-policy's recorded `HumanHeld` decision;
//!   until that record exists, `workspace.state` keeps the obstacle in `not_yet`. Missing payloads
//!   instead appear as typed,
//!   recoverable conditions beside the partial paths they allowed.

pub mod events;
pub mod json;
pub mod message;
#[cfg(unix)]
pub mod server;
pub mod surface;

pub use crate::ipc::events::{DaemonEvent, EventBacklog, EventFeed, EventKind, FEED_CAPACITY};
pub use crate::ipc::json::{Json, JsonError};
pub use crate::ipc::message::{
    daemon_frames, ChunkAssembler, ClientMessage, DaemonMessage, WireError, CHUNK_DATA_BYTES,
    MAX_LINE_BYTES, MAX_MESSAGE_BYTES, MAX_METHOD_BYTES, MAX_SESSION_BYTES, PROTOCOL,
    SUPPORTED_VERSIONS, SURFACE_VERSION,
};
#[cfg(unix)]
pub use crate::ipc::server::{IpcServer, ServerHandle, MAX_CONNECTIONS};
pub use crate::ipc::surface::{
    describe, method, negotiate, nothing_to_recover, severity_word, Conversation, Method,
    Operations, RecoveredDaemon, StartupSummary, Unavailable, WorkspaceSummary, METHODS,
};

/// The published contract both ends check themselves against, embedded at compile time.
///
/// Embedded rather than read from disk so that the daemon's own test cannot pass because it found
/// a file in a working tree that a shipped binary would not have.
pub const CONTRACT_JSON: &str = include_str!("../../ipc-contract.json");
