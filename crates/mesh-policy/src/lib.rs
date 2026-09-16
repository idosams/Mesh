//! The capability model's decision layer: policy epochs, the authority gate, and the publication
//! guard.
//!
//! Mesh makes one central claim: **an agent cannot advance canonical state.** `mesh-crypto` makes an
//! over-broad *grant* unwritable. This crate makes the *decision* — and the shape of the decision
//! layer is where the claim is either enforced or merely asserted.
//!
//! # What is enforced by the type system
//!
//! * **Publication cannot be asked for with an agent's capability.**
//!   [`DecisionLedger::authorize_publication`] takes `&Capability<HumanHeld>` and
//!   `&HumanPrincipal`. An agent holds a `Capability<Delegated>`, and there is no conversion:
//!   `AuthorityTier::Delegated` is `Delegated` for both tiers, so no chain of delegations climbs.
//!   Passing one is a **compile error at the call site**.
//!
//!   ```compile_fail,E0308
//!   # use mesh_crypto::{Capability, Delegated};
//!   # use mesh_policy::{AuthorityRequest, DecisionLedger, EpochChain, HumanPrincipal};
//!   fn publish(
//!       ledger: DecisionLedger,
//!       chain: &EpochChain,
//!       approver: &HumanPrincipal,
//!       agent_capability: &Capability<Delegated>,   // what an agent holds
//!       request: &AuthorityRequest,
//!   ) {
//!       // expected `&Capability<HumanHeld>`, found `&Capability<Delegated>`
//!       let _ = ledger.authorize_publication(chain, approver, agent_capability, request);
//!   }
//!   ```
//!
//! * **A capability cannot be widened.** A delegation carries `DelegatedAction`, which has no
//!   canonical-advance variant. The widening request does not fail a check; it does not compile.
//!
//!   ```compile_fail,E0599
//!   # use mesh_crypto::{ActorKey, Delegation, DelegatedAction, Expiry};
//!   let _ = Delegation::new(
//!       ActorKey::from_public_bytes([1; 32]),
//!       [DelegatedAction::AdvanceCanonicalHead],   // no such variant
//!       Expiry::at_unix_millis(1),
//!   );
//!   ```
//!
//! * **The general gate cannot grant publication at any tier.**
//!   [`DecisionLedger::authorize`] is generic over the tier, so the only `T::Action` it can build
//!   is one lifted from a `DelegatedAction` — and there is none for canonical advancement. It
//!   denies [`Operation::AdvanceCanonicalHead`] because it has no value to check, not because a
//!   branch says no.
//!
//! * **A grant is not a value a caller can forge.** [`Grant`] and [`PublicationAuthority`] have no
//!   public constructor and no public field. Every one that exists came out of the ledger.
//!
//! * **A decision cannot be reached without being recorded.** The gate functions are `pub(crate)`.
//!   [`DecisionLedger`] is the only public door, and both of its methods return the new ledger
//!   beside the outcome.
//!
//! * **A reconnected peer cannot admit an operation before applying the policy records it was
//!   offered.** [`PendingSession`] has no `admit` method, and the only route to
//!   [`AdmittingSession`] — which does — is [`PendingSession::catch_up`], which consumes the pending
//!   session. Admitting first is not refused; it does not compile.
//!
//! # What is enforced at run time, and stated as such
//!
//! Expiry, epoch standing, revocation, workspace scope and subject binding are value comparisons.
//! They are checked in one place ([`crate::decision::check_preconditions`]) that both gates call,
//! because two copies of an epoch check is two places for a revocation to stop biting.
//!
//! # Revocation, and what it does to history
//!
//! Revoking a key must not delete what that key already signed. [`EpochChain::authorship_standing`]
//! decides an existing record by comparing the epoch it was sealed under against the epoch its
//! author's revocation became effective in, and a rotation issued at epoch `n` is effective at
//! `n + 1` — strictly after every record that already exists. So revocation cannot reach backwards,
//! structurally, and [`crate::revocation`] is where that argument is written out in full. An epoch
//! this peer has not observed reads [`AuthorshipStanding::Indeterminate`], never valid.
//!
//! # What this crate does not claim
//!
//! A [`PublicationAuthority`] is **not** a signature and does not advance anything. Canonical state
//! advances on a human key's signature over an approval envelope; this crate holds no key, signs
//! nothing, and is the gate in front of the lock rather than the lock. `mesh-crypto` ships no
//! `KeyCustody` implementation at all today, so nothing in the workspace can produce that signature
//! — which is the state that makes the central claim true right now, independently of everything
//! here.
//!
//! Nor does it claim the approval path *uses* it. Wiring the relay and `mesh-approval` through this
//! model is those crates' work; this crate is out of their reach until they take the dependency.
//!
//! # Ordering and clocks
//!
//! No wall clock is read anywhere in this crate. `now` is always an argument, epochs are counters,
//! and the ledger is ordered by its own sequence number.

#![forbid(unsafe_code)]

// The modules are private and every public item is re-exported at the crate root, matching
// `mesh-types` and `mesh-crypto`: a module path is a second name for the same item.
mod decision;
mod epoch;
mod ledger;
mod principal;
mod publication;
mod revocation;
mod session;

pub use crate::decision::{AuthorityRequest, Denial, DenialReason, Grant, Operation};
pub use crate::epoch::{EpochChain, EpochError, EpochRotation, EpochStanding, RotationReason};
pub use crate::ledger::{DecisionLedger, DecisionRecord, Outcome};
pub use crate::principal::{HumanPrincipal, Principal, PrincipalError};
pub use crate::publication::PublicationAuthority;
pub use crate::revocation::{
    AuthorshipStanding, RevocationEntry, RevocationLedger, RevocationStanding,
};
pub use crate::session::{AdmittingSession, CatchUpError, PendingSession, PolicyHeadAssertion};

/// The crate's name, so a placeholder still carries one verifiable behaviour.
pub const CRATE_NAME: &str = "mesh-policy";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-policy");
    }
}
