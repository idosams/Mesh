//! Content-addressed storage with atomic, hash-verified chunk promotion.
//!
//! Plan §6.2 says where bytes live and §6.3 says in what order they get there. This crate is steps
//! 1–4 of that sequence, and the whole of its value is a single guarantee:
//!
//! > **At every point at which the process can stop, the store either contains the chunk — whole
//! > and verified — or does not contain it. There is no third outcome.**
//!
//! A chunk that is half-written but readable is the failure that corrupts everything downstream,
//! because everything downstream is entitled to assume that a name in the store resolves to the
//! bytes that name says. So the guarantee is demonstrated rather than asserted: `crash-promotion`
//! kills a real child process at each of the six promotion steps and after randomised delays, and
//! checks the invariant on what is left. Plan §2.10 forbids calling this safe without that
//! evidence; `DURABILITY.md` states in plain words what the evidence covers and what it does not.
//!
//! ```no_run
//! use mesh_cas::Cas;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let store = Cas::open("/tmp/workspace")?;
//! store.discard_scratch()?; // plan §6.3: temporary data from a previous crash is discarded
//!
//! let promoted = store.promote(b"file content".to_vec())?;
//! // `promoted.outcome()` says whether this call is the reason the chunk is there. `AlreadyPresent`
//! // means the store's copy predates this call and was not read, so a re-promotion is never a
//! // repair — `DURABILITY.md`, "Repairing a chunk", has the sequence that is.
//! assert_eq!(store.read(&promoted.digest())?, b"file content");
//! # Ok(())
//! # }
//! ```
//!
//! # The six things this crate is
//!
//! * [`Promotion`] — plan §6.3 steps 1–4 as six named, individually interruptible steps.
//! * [`IncomingChunk`] — durable resumable receipt whose bytes remain outside the content
//!   namespace until their complete digest passes the existing promotion path.
//! * [`TransferPlan`] — integrity-aware local availability projected into deterministic bounded
//!   requests from the digests a manifest layer says are required.
//! * [`Cas::read`] — the only way to get bytes out, and it hashes them first. A chunk that fails is
//!   quarantined before the call returns, so the same bad bytes cannot be handed to a caller that
//!   forgets to check.
//! * [`ArrivalJournal`] — a durable record written *before* a chunk becomes visible, so the
//!   collector can find unreferenced chunks by reading a small file instead of walking the store.
//! * [`Cas::collect`] — the only thing in this repository that removes a chunk, and it cannot be
//!   called without a [`ReferenceOracle`] that gets to veto every deletion at the instant it
//!   happens. `crates/mesh-store/RETENTION.md` states what the two halves promise together.
//! * [`DurableFs`] — the seam that makes the ordering above observable, so `promotion_ordering`
//!   can assert "the data was synced before the name existed" rather than trusting this paragraph.
//!
//! # What it deliberately is not
//!
//! It does not decide which content a workspace requires (manifests do that), chunk (that is
//! `mesh-chunking`), compress, know what references a chunk (that is
//! [`ReferenceOracle`], supplied by the caller), or decide retention (plan §6.4). It does not open
//! the database: plan §6.3 steps 5–9 are `mesh-store`'s, and the reason this crate stops at step 4
//! is that the two must be able to fail independently for the crash behaviour in §6.3 to hold.
//!
//! # Dependencies
//!
//! None, including on `mesh-types` — see [`blake3`] for why a crate that must produce byte-identical
//! digests to `mesh-types` nonetheless carries its own copy of the hasher, and what holds the two
//! together in the absence of a compiler-checked edge.

mod blake3;
mod collect;
mod digest;
mod error;
mod fs;
mod journal;
mod layout;
mod promotion;
mod store;
mod transfer;
mod transfer_plan;

pub use crate::blake3::Blake3Hasher;
pub use crate::collect::{Collected, CollectionMode};
pub use crate::digest::{Blake3, ContentDigest, Digest32, DigestHasher, DigestParseError};
pub use crate::error::CasError;
pub use crate::fs::{DurableFs, StdFs};
pub use crate::journal::ArrivalJournal;
pub use crate::layout::{
    StoreLayout, ARRIVAL_JOURNAL_FILE_NAME, ARRIVAL_JOURNAL_REWRITE_FILE_NAME,
    CHUNKS_DIRECTORY_NAME, INCOMING_DIRECTORY_NAME, LOGS_DIRECTORY_NAME, QUARANTINE_DIRECTORY_NAME,
    SCRATCH_DIRECTORY_NAME,
};
pub use crate::promotion::{Promoted, Promotion, PromotionOutcome, PromotionStep};
pub use crate::store::{Cas, ReferenceOracle};
pub use crate::transfer::{IncomingChunk, ReceiveError, ReceiveProgress};
pub use crate::transfer_plan::{
    DetectedCorruption, PlannedChunkRequest, TransferBatch, TransferPlan, TransferPlanError,
};
