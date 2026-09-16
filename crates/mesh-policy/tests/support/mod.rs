//! Fixtures for the policy tests.
//!
//! **Nothing here signs.** [`AttestingCustody`] implements `HumanKeyCustody` so that a
//! `Capability<HumanHeld>` can be minted in a test, and its `sign` returns
//! `CustodyError::BackendUnavailable` unconditionally. That is deliberate: a test double that could
//! produce a signature would be the most attractive thing in the workspace to promote into
//! production, and the capability tests need attestation, never a signature.
//!
//! This is a `tests/` module. It compiles as part of a separate test crate that ships in no binary,
//! and `mesh-policy`'s own `src/` names none of it.

#![allow(dead_code)]

use mesh_crypto::{
    ActorKey, Capability, CustodyBackend, CustodyError, DelegatedAction, Delegation,
    DelegationBudget, Expiry, ForActor, HumanAction, HumanHeld, HumanKeyCustody, KeyCustody,
    KeyPair, SigningPayload, WorkspaceScope,
};
use mesh_types::{ActorKind, PolicyEpoch, Signature};

use mesh_policy::{HumanPrincipal, Principal};

/// The workspace every fixture is scoped to.
pub const WORKSPACE: WorkspaceScope = WorkspaceScope::from_bytes([0x5a; 16]);

/// A second workspace, for scope-escape cases.
pub const OTHER_WORKSPACE: WorkspaceScope = WorkspaceScope::from_bytes([0xa5; 16]);

/// The epoch the fixtures are issued in.
pub const EPOCH: PolicyEpoch = PolicyEpoch::new(7);

/// A capability expiry comfortably after [`NOW`].
pub const NOT_AFTER: Expiry = Expiry::at_unix_millis(10_000);

/// The moment decisions are made at, in Unix milliseconds.
pub const NOW: u64 = 5_000;

/// A key named by `byte` repeated.
#[must_use]
pub fn key(byte: u8) -> ActorKey {
    ActorKey::from_public_bytes([byte; 32])
}

/// A custody that attests and cannot sign.
pub struct AttestingCustody {
    key: ActorKey,
}

impl AttestingCustody {
    /// Custody of the key named by `byte` repeated.
    #[must_use]
    pub fn holding(byte: u8) -> Self {
        Self { key: key(byte) }
    }
}

impl KeyCustody<ForActor> for AttestingCustody {
    fn backend(&self) -> CustodyBackend {
        CustodyBackend::AppleSecureEnclave
    }

    fn public_key(&self) -> KeyPair<ForActor> {
        self.key
    }

    /// Always refuses. See the module documentation.
    fn sign(&self, _payload: &SigningPayload) -> Result<Signature, CustodyError> {
        Err(CustodyError::BackendUnavailable)
    }
}

impl HumanKeyCustody for AttestingCustody {}

/// A human-held capability for `byte`'s key, granting `actions`.
#[must_use]
pub fn human_capability(
    byte: u8,
    actions: impl IntoIterator<Item = HumanAction>,
) -> Capability<HumanHeld> {
    human_capability_in(byte, actions, EPOCH, NOT_AFTER, WORKSPACE)
}

/// A human-held capability with every field chosen.
#[must_use]
pub fn human_capability_in(
    byte: u8,
    actions: impl IntoIterator<Item = HumanAction>,
    epoch: PolicyEpoch,
    not_after: Expiry,
    workspace: WorkspaceScope,
) -> Capability<HumanHeld> {
    let custody = AttestingCustody::holding(byte);
    let attestation = custody.attest_human().expect("the double always attests");
    Capability::<HumanHeld>::root(
        &attestation,
        workspace,
        actions,
        epoch,
        not_after,
        DelegationBudget::new(3),
    )
}

/// The full human authority: every delegable action plus canonical-head advancement.
#[must_use]
pub fn every_human_action() -> Vec<HumanAction> {
    let mut actions: Vec<HumanAction> = DelegatedAction::ALL
        .into_iter()
        .map(HumanAction::Delegated)
        .collect();
    actions.push(HumanAction::AdvanceCanonicalHead);
    actions
}

/// An agent capability delegated from a human's, granting every delegable action.
///
/// Note what this function *cannot* ask for: `DelegatedAction` has no canonical-advance variant, so
/// the widest capability an agent can be given is exactly this.
#[must_use]
pub fn agent_capability(agent: u8) -> Capability<mesh_crypto::Delegated> {
    delegated_capability(agent, DelegatedAction::ALL, NOT_AFTER)
}

/// An agent capability with a chosen action set and expiry.
#[must_use]
pub fn delegated_capability(
    agent: u8,
    actions: impl IntoIterator<Item = DelegatedAction>,
    not_after: Expiry,
) -> Capability<mesh_crypto::Delegated> {
    let root = human_capability(1, every_human_action());
    root.delegate(&Delegation::new(key(agent), actions, not_after))
        .expect("a delegation of actions the root holds")
}

/// An agent capability issued in a chosen policy epoch.
///
/// The revocation suite needs a capability *freshly issued in the epoch now in force* to a key that
/// a rotation revoked, because that is the mistake the revocation set exists to catch: rotating the
/// epoch already kills everything issued before it, so only a re-issue reaches the revocation check
/// at all.
#[must_use]
pub fn delegated_capability_in(
    agent: u8,
    actions: impl IntoIterator<Item = DelegatedAction>,
    epoch: PolicyEpoch,
    not_after: Expiry,
) -> Capability<mesh_crypto::Delegated> {
    let root = human_capability_in(1, every_human_action(), epoch, not_after, WORKSPACE);
    root.delegate(&Delegation::new(key(agent), actions, not_after))
        .expect("a delegation of actions the root holds")
}

/// A human principal for `byte`'s key.
#[must_use]
pub fn human(byte: u8) -> HumanPrincipal {
    HumanPrincipal::enrol(Principal::new(key(byte), ActorKind::Human)).expect("a human enrols")
}

/// Every actor kind that is not `human`.
#[must_use]
pub fn non_human_kinds() -> Vec<ActorKind> {
    ActorKind::ALL
        .into_iter()
        .filter(|kind| !kind.may_hold_approval_capability())
        .collect()
}
