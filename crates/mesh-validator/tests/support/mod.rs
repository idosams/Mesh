//! Fixtures for the validator tests.
//!
//! **Nothing here signs, and nothing here executes.** [`AttestingCustody`] implements
//! `HumanKeyCustody` so that a `Capability<HumanHeld>` — and from it a delegated capability, and
//! from that a `Grant<Delegated>` — can be minted in a test; its `sign` returns
//! `CustodyError::BackendUnavailable` unconditionally, for the reason `mesh-policy`'s copy of this
//! double gives: a test double that could produce a signature would be the most attractive thing
//! in the workspace to promote into production.
//!
//! The grant matters here. `mesh-policy` gives `Grant<T>` no public constructor, so a test cannot
//! fabricate validation authority any more than a caller can — it has to go through
//! `DecisionLedger::authorize`, exactly as production does.
//!
//! This is a `tests/` module. It compiles into test binaries only and `mesh-validator`'s `src/`
//! names none of it.

#![allow(dead_code)]

use mesh_crypto::{
    ActorKey, Capability, CustodyBackend, CustodyError, Delegated, DelegatedAction, Delegation,
    DelegationBudget, Expiry, ForActor, HumanAction, HumanHeld, HumanKeyCustody, KeyCustody,
    KeyPair, SigningPayload, WorkspaceScope,
};
use mesh_policy::{
    AuthorityRequest, DecisionLedger, EpochChain, Grant, HumanPrincipal, Operation, Principal,
};
use mesh_types::{ActorKind, Digest32, PolicyEpoch, Signature};

use mesh_validator::{
    ChangedPath, PathEdit, ProfileProposal, ReviewChange, SandboxRequirement, ValidationCommand,
    ValidationPlan, ValidationProfile, ValidationTrigger, ValidatorId, ValidatorRegistry,
    ValidatorSpec,
};

/// The workspace every fixture is scoped to.
pub const WORKSPACE: WorkspaceScope = WorkspaceScope::from_bytes([0x5a; 16]);

/// A second workspace, for scope-escape cases.
pub const OTHER_WORKSPACE: WorkspaceScope = WorkspaceScope::from_bytes([0xa5; 16]);

/// The epoch the fixtures are issued in.
pub const EPOCH: PolicyEpoch = PolicyEpoch::new(7);

/// The epoch a rotation moves to.
pub const ROTATED_EPOCH: PolicyEpoch = PolicyEpoch::new(8);

/// A capability expiry comfortably after [`NOW`].
pub const NOT_AFTER: Expiry = Expiry::at_unix_millis(10_000);

/// The moment decisions are made at, in Unix milliseconds.
pub const NOW: u64 = 5_000;

/// The immutable review snapshot the fixtures validate against.
#[must_use]
pub fn snapshot() -> Digest32 {
    Digest32::from_bytes([0x11; 32])
}

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

/// A human principal for `byte`'s key.
#[must_use]
pub fn human(byte: u8) -> HumanPrincipal {
    HumanPrincipal::enrol(Principal::new(key(byte), ActorKind::Human)).expect("a human enrols")
}

/// The root human capability: every delegable action plus canonical-head advancement.
#[must_use]
fn root_capability(epoch: PolicyEpoch) -> Capability<HumanHeld> {
    let custody = AttestingCustody::holding(1);
    let attestation = custody.attest_human().expect("the double always attests");
    let mut actions: Vec<HumanAction> = DelegatedAction::ALL
        .into_iter()
        .map(HumanAction::Delegated)
        .collect();
    actions.push(HumanAction::AdvanceCanonicalHead);
    Capability::<HumanHeld>::root(
        &attestation,
        WORKSPACE,
        actions,
        epoch,
        NOT_AFTER,
        DelegationBudget::new(3),
    )
}

/// An agent capability granting every delegable action.
///
/// Note what this cannot ask for: `DelegatedAction` has no canonical-advance variant, so the
/// widest capability an agent can hold is exactly this.
#[must_use]
pub fn agent_capability(epoch: PolicyEpoch) -> Capability<Delegated> {
    root_capability(epoch)
        .delegate(&Delegation::new(key(2), DelegatedAction::ALL, NOT_AFTER))
        .expect("a delegation of actions the root holds")
}

/// A grant for `operation`, produced the only way a grant can be: by the decision ledger.
#[must_use]
pub fn grant_for(operation: Operation) -> Grant<Delegated> {
    let chain = EpochChain::genesis(EPOCH);
    let request = AuthorityRequest::new(key(2), WORKSPACE, operation, NOW);
    let (_, outcome) =
        DecisionLedger::empty().authorize(&chain, &agent_capability(EPOCH), &request);
    outcome.expect("the delegated capability holds every delegable action")
}

/// A validator that runs `program`, fired unconditionally, confined by `sandbox`.
#[must_use]
pub fn validator(id: &str, program: &str, sandbox: SandboxRequirement) -> ValidatorSpec {
    ValidatorSpec::new(
        ValidatorId::parse(id).expect("a legal identifier"),
        ValidationCommand::at_root(program, []).expect("a legal command"),
        [ValidationTrigger::Always],
        sandbox,
    )
    .expect("one trigger")
}

/// A registry of unconditionally firing validators, one per name.
#[must_use]
pub fn registry_of(names: &[&str]) -> ValidatorRegistry {
    let mut registry = ValidatorRegistry::empty();
    for name in names {
        registry = registry
            .with(validator(name, name, SandboxRequirement::Isolated))
            .expect("distinct identifiers");
    }
    registry
}

/// A one-path change against [`snapshot`].
#[must_use]
pub fn change() -> ReviewChange {
    ReviewChange::against(snapshot())
        .with_path(ChangedPath::new("src/lib.rs", PathEdit::Modified, 128).expect("a legal path"))
        .expect("no duplicate")
}

/// A profile approving exactly what `plan` proposes, in [`WORKSPACE`] and [`EPOCH`].
#[must_use]
pub fn approved(plan: &ValidationPlan) -> ValidationProfile {
    ProfileProposal::from_plan(plan).approve(&human(9), WORKSPACE, EPOCH)
}
