//! **The attack M15 does not stop, written as a test rather than as prose.**
//!
//! `docs/threat-model.md` §9 M15 says a key custody that releases the secret into this process
//! cannot attest that its key is a person's. That is true of an *honest* custody, and
//! `key_isolation.rs` is the suite that proves it. This file is the negative space of the same
//! mitigation: [`KeyCustody::backend`] is a method the implementation writes, so a custody that
//! holds its key in this address space and **reports a backend it does not have** receives an
//! attestation for it, and `mesh-crypto` cannot check the claim. `custody.rs` says so in
//! `HumanKeyCustody`'s own documentation; this file measures it.
//!
//! The gap register carries it as `docs/threat-model.md` §10 **G30**, with the owner named there.
//!
//! # Why the test asserts the attack *succeeds*
//!
//! So that a fix which actually closes it turns this file red. An assertion that the chain stops
//! somewhere it does not stop would be the same class of defect as the claim it is testing: a
//! statement that reads as evidence and is not.
//!
//! # What changed, and why this is filed now rather than earlier
//!
//! The workspace's accidental protection used to be that nothing outside a test could produce an
//! Ed25519 signature at all. `platform/mesh-keychain`'s software fallback ended that. The custody
//! below therefore does not stop at a `BackendUnavailable` the way this crate's older doubles do —
//! it signs, with a real key it generated for itself, and the signature verifies under the same
//! `Ed25519` every peer verifies with.
//!
//! # This is latent, not live
//!
//! Nothing consumes a publication authority to advance a head. There is no approval envelope (G1),
//! no compare-and-swap admission (G2) and no relay (G9), and `mesh-materializer`'s
//! `advance_canonical` is an unauthenticated state machine that takes no capability at all. The
//! exposure arrives with the first of those.

mod support;

use mesh_crypto::{
    Capability, CustodyBackend, CustodyError, CustodyRequirement, DelegationBudget,
    DomainSeparator, Ed25519, Expiry, ForActor, HumanAction, HumanHeld, HumanKeyCustody,
    IsolationClass, KeyCustody, KeyPair, PolicyEpoch, SignatureScheme, SigningPayload,
};
use mesh_types::Signature;
use support::{workspace, RealSigner};

/// A custody that generates its key in this process and calls it a Secure Enclave key.
///
/// Thirty lines, no privilege, nothing hidden. `backend()` returns whatever this file says it
/// returns; the empty `impl HumanKeyCustody` block inherits the trait's body, which asks the
/// backend and believes the answer.
struct LyingCustody {
    signer: RealSigner,
}

impl LyingCustody {
    /// A custody over a real Ed25519 key held in this address space.
    fn in_process(filler: u8) -> Self {
        Self {
            signer: RealSigner::from_filler(filler),
        }
    }
}

impl KeyCustody<ForActor> for LyingCustody {
    /// The lie, and the whole of it.
    fn backend(&self) -> CustodyBackend {
        CustodyBackend::AppleSecureEnclave
    }

    fn public_key(&self) -> KeyPair<ForActor> {
        self.signer.actor_key()
    }

    fn sign(&self, payload: &SigningPayload) -> Result<Signature, CustodyError> {
        Ok(self.signer.sign_payload(payload))
    }
}

impl HumanKeyCustody for LyingCustody {}

/// The approval payload a publication would be signed over, once one exists.
fn approval_payload() -> SigningPayload {
    SigningPayload::new(
        DomainSeparator::new("mesh.v0.approval-envelope"),
        b"expected-head=00",
    )
}

/// **The attack.** An in-process key reaches the one capability tier that may advance canonical
/// state, because it says it is somewhere else.
#[test]
fn a_custody_that_names_a_backend_it_does_not_have_reaches_the_human_tier() {
    let custody = LyingCustody::in_process(0x5a);

    // The floor M15 enforces is checked against the *reported* backend, so it passes.
    assert_eq!(custody.backend(), CustodyBackend::AppleSecureEnclave);
    assert_eq!(
        CustodyRequirement::HUMAN_APPROVAL.check(custody.backend()),
        Ok(())
    );

    let attestation = custody
        .attest_human()
        .expect("a reported backend above the floor attests");
    assert_eq!(attestation.backend(), CustodyBackend::AppleSecureEnclave);
    assert_eq!(
        attestation.isolation(),
        IsolationClass::HardwareNonExportable
    );

    let capability = Capability::<HumanHeld>::root(
        &attestation,
        workspace(),
        [HumanAction::AdvanceCanonicalHead],
        PolicyEpoch::new(1),
        Expiry::at_unix_millis(10_000),
        DelegationBudget::new(3),
    );

    assert!(capability.grants(HumanAction::AdvanceCanonicalHead));
    assert!(capability.advances_canonical_head());
    assert_eq!(*capability.subject(), custody.public_key());
    assert_eq!(
        capability.check_current(5_000, PolicyEpoch::new(1)),
        Ok(()),
        "and it is current, so nothing downstream rejects it on age or epoch"
    );
}

/// The attestation carries the claim and nothing about the mechanism.
///
/// `IsolationProof` is evidence that the *reported* backend is at or above the floor. It is not
/// evidence about where the key is. Two custodies holding the same key in the same place, one
/// honest and one not, differ only in the string one of them returns.
#[test]
fn the_attestation_is_evidence_about_a_claim_and_not_about_a_mechanism() {
    let liar = LyingCustody::in_process(0x11);
    let honest = HonestCustody {
        signer: RealSigner::from_filler(0x11),
    };

    assert_eq!(liar.public_key(), honest.public_key());
    assert_eq!(
        liar.sign(&approval_payload()).expect("the liar signs"),
        honest.sign(&approval_payload()).expect("the honest signs"),
        "the same key, in the same process, signing the same bytes"
    );

    // The only difference between them is the value one of them chose to return.
    assert!(liar.attest_human().is_ok());
    assert_eq!(
        honest.attest_human(),
        Err(CustodyError::InsufficientIsolation {
            required: IsolationClass::OsMediated,
            offered: IsolationClass::InProcess,
        })
    );
}

/// The same key and the same signer, reporting what it actually is.
struct HonestCustody {
    signer: RealSigner,
}

impl KeyCustody<ForActor> for HonestCustody {
    fn backend(&self) -> CustodyBackend {
        CustodyBackend::SoftwareInProcess
    }

    fn public_key(&self) -> KeyPair<ForActor> {
        self.signer.actor_key()
    }

    fn sign(&self, payload: &SigningPayload) -> Result<Signature, CustodyError> {
        Ok(self.signer.sign_payload(payload))
    }
}

impl HumanKeyCustody for HonestCustody {}

/// **What still stops it.** The signature has to be checked against the person's *registered* key.
///
/// This is the compensating control `custody.rs` names — "the control an agent cannot defeat is
/// the *signature*" — measured rather than asserted. The lying custody produces a real Ed25519
/// signature, and it verifies under the key the liar minted; it does **not** verify under the
/// human's key, which is the check that would refuse it.
///
/// **That check exists nowhere on a publication path in this workspace**, and there is no registry
/// of human keys to check against: `mesh_policy::ActorKind` is a value a caller writes, not a fact
/// anything binds to a key. So this test measures a control that is available and unbuilt, which
/// is precisely what `docs/threat-model.md` §10 G30 records.
#[test]
fn the_remaining_control_is_a_signature_under_a_key_nobody_registers() {
    let agent = LyingCustody::in_process(0x5a);
    let human = RealSigner::from_filler(0x77);
    let payload = approval_payload();

    let forged = agent.sign(&payload).expect("the lying custody signs");

    // Under its own key it is a valid signature. This is the part TASK-31 changed: before the
    // software fallback landed, nothing outside a test could get this far.
    Ed25519::verify(
        &agent.public_key().public_key(),
        payload.as_bytes(),
        &forged,
    )
    .expect("a real Ed25519 signature under the key the agent minted");

    // Under the human's registered key it is not. This is the whole of the remaining distance.
    assert!(Ed25519::verify(&human.public_key(), payload.as_bytes(), &forged).is_err());

    // And the capability the agent holds says nothing about which of the two keys is a person's:
    // the subject is simply the key that attested.
    let capability = Capability::<HumanHeld>::root(
        &agent.attest_human().expect("the liar attests"),
        workspace(),
        [HumanAction::AdvanceCanonicalHead],
        PolicyEpoch::new(1),
        Expiry::at_unix_millis(10_000),
        DelegationBudget::new(3),
    );
    assert_ne!(
        capability.subject().public_key(),
        human.public_key(),
        "nothing in this crate binds the attested key to a person"
    );
}
