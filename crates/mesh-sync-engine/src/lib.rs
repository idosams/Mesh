//! Metadata and content planes, durable outbox, offline reconciliation, anti-entropy.
//!
//! **Mostly a placeholder still.** The transport lands under **E07 synchronization**; run
//! See the public roadmap and issue tracker for planned transport work. What is here today is one thing, and it is
//! here because ADR-0015 put it here by name.
//!
//! # The receipt-side identifier check
//!
//! `crates/mesh-state` derives an actor head from the applied **identifier** set, so convergence
//! holds exactly when a ChangeSet's identifier binds its causal parent set — and stays held only
//! while every receiver verifies that binding. `mesh-types` binds it; ADR-0015 measured that
//! nothing recomputed it on receipt, and constructed a two-record divergence in which two peers
//! apply the same two identifiers, both report nothing outstanding, reach two different heads,
//! refuse nothing, and then permanently reject every subsequent honest ChangeSet from each other.
//!
//! [`admit`] is the check that makes that unreachable. It recomputes a carried ChangeSet's
//! identifier from its canonical `mesh-cbor/0` bytes under BLAKE3 and refuses the record if the two
//! disagree. It lives here rather than in `mesh-state` because this is the boundary that holds the
//! bytes — ADR-0015 decision point 4.
//!
//! ```
//! use mesh_sync_engine::{admit, EmptyOperations, Refusal};
//! # use mesh_sync_protocol::CarriedChangeSet;
//! # fn check(honest: &CarriedChangeSet, tampered: &CarriedChangeSet) {
//! // An honest record is admitted, and what comes out is read from the body.
//! let admitted = admit(honest, &EmptyOperations).expect("an honest record is admitted");
//! assert_eq!(admitted.id(), honest.id);
//!
//! // A record whose identifier disagrees with its bytes never becomes one.
//! assert!(matches!(
//!     admit(tampered, &EmptyOperations),
//!     Err(Refusal::IdentifierMismatch { .. })
//! ));
//! # }
//! ```
//!
//! The dependency rules in plan §8.3 apply to this crate and are checked by
//! `node tools/program/arch-check/check.mjs`. Its `mesh-types` and `mesh-sync-protocol` edges are
//! downward; its same-layer `mesh-store` edge composes delivery with the durable metadata owner.
//! All three are workspace paths, which
//! `docs/adr/0014-narrow-the-lockfile-fence-to-admit-an-audited-cryptographic-dependency.md` admits
//! without the five conditions for a new external dependency.

mod decode;
mod delivery;
mod inbound;
mod operation;
mod receipt;

pub use crate::decode::BodyRefused;
pub use crate::delivery::{
    persist_delivery_acknowledgement, DeliveryPersistenceError, DeliveryReceipt,
};
pub use crate::inbound::{
    persist_inbound_operation, InboundOperationReceipt, InboundPersistenceError,
};
pub use crate::operation::{EmptyOperations, OperationDecoder};
pub use crate::receipt::{admit, AdmittedChangeSet, Refusal};

/// The crate's name, so a placeholder still carries one verifiable behaviour.
pub const CRATE_NAME: &str = "mesh-sync-engine";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-sync-engine");
    }
}
