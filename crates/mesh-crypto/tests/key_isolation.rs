//! Key isolation, asserted from the seam's own side.
//!
//! The task contract names `cargo nextest run -p mesh-crypto --test key-isolation`; cargo derives
//! a target name from the file name and `key-isolation.rs` is not a legal module path, so the
//! command is `--test key_isolation`. The backend-side half of this suite —  the attacks against a
//! custody that actually holds a key — is `platform/mesh-keychain/tests/key_isolation.rs`.
//!
//! What is asserted here is the part that must hold for **every** backend, present and future:
//! that a custody below `CustodyRequirement::HUMAN_APPROVAL` has no route to the one tier that may
//! advance canonical state, and that the route it does not have is a missing value rather than a
//! failed check.

mod support;

use mesh_crypto::{
    Capability, CustodyBackend, CustodyError, CustodyRequirement, ForActor, HumanKeyCustody,
    IsolationClass, KeyCustody, KeyPair, SigningPayload,
};
use mesh_types::Signature;
use support::TestCustody;

/// A custody that reports a backend the process can read the secret out of.
struct WeaklyIsolatedCustody {
    backend: CustodyBackend,
    key: KeyPair<ForActor>,
}

impl KeyCustody<ForActor> for WeaklyIsolatedCustody {
    fn backend(&self) -> CustodyBackend {
        self.backend
    }

    fn public_key(&self) -> KeyPair<ForActor> {
        self.key
    }

    fn sign(&self, _payload: &SigningPayload) -> Result<Signature, CustodyError> {
        Ok(Signature::from_bytes([0u8; 64]))
    }
}

/// The empty implementation. This is the shape the defect had: `impl HumanKeyCustody for X {}`
/// used to be a complete, working attestation for any backend at all, because the trait's method
/// carried a default body that asked nothing.
impl HumanKeyCustody for WeaklyIsolatedCustody {}

/// The defect, as a test. An in-process software key store must not be able to attest that its key
/// is a person's, because the attestation is the only route to `Capability<HumanHeld>` and
/// `HumanHeld` is the only tier whose actions contain canonical-head advancement.
#[test]
fn a_custody_that_releases_the_secret_to_this_process_cannot_attest_a_human_key() {
    for backend in [
        CustodyBackend::SoftwareInProcess,
        CustodyBackend::AppleKeychain,
        CustodyBackend::LinuxKernelKeyring,
    ] {
        let custody = WeaklyIsolatedCustody {
            backend,
            key: KeyPair::from_public_bytes([9; 32]),
        };
        assert_eq!(
            custody.attest_human(),
            Err(CustodyError::InsufficientIsolation {
                required: IsolationClass::OsMediated,
                offered: backend.isolation(),
            }),
            "{backend} attested a human key it cannot protect"
        );
    }
}

/// The other direction, so the test above is not passing because attestation is broken for
/// everyone. A backend that never releases the secret still attests, and the attestation carries
/// the class it was admitted on.
#[test]
fn a_custody_that_never_releases_the_secret_still_attests() {
    for backend in [
        CustodyBackend::AppleSecureEnclave,
        CustodyBackend::WindowsCng,
        CustodyBackend::HardwareToken,
    ] {
        let custody = WeaklyIsolatedCustody {
            backend,
            key: KeyPair::from_public_bytes([9; 32]),
        };
        let attestation = custody.attest_human().expect("an isolated backend attests");
        assert_eq!(attestation.backend(), backend);
        assert_eq!(attestation.isolation(), backend.isolation());
        assert!(attestation.isolation() >= IsolationClass::OsMediated);
        assert_eq!(*attestation.key(), custody.public_key());
    }
}

/// An attestation is the subject: a human capability cannot be minted for a key other than the one
/// custody vouched for, because `Capability::root` has no subject argument to get wrong.
#[test]
fn a_human_capability_names_the_attested_key_and_no_other() {
    let custody = TestCustody::new(4);
    let attestation = custody.attest_human().expect("the isolated double attests");
    let capability = Capability::root(
        &attestation,
        mesh_crypto::WorkspaceScope::from_bytes([1; 16]),
        [mesh_crypto::HumanAction::AdvanceCanonicalHead],
        mesh_crypto::PolicyEpoch::new(1),
        mesh_crypto::Expiry::at_unix_millis(9_000),
        mesh_crypto::DelegationBudget::new(1),
    );
    assert_eq!(*capability.subject(), *attestation.key());
    assert_eq!(*capability.issuer(), *attestation.key());
}

/// The requirement refuses below its floor and never substitutes something weaker, for every
/// backend the enum names. A resolver that downgrades is how this fails in the field.
#[test]
fn every_backend_below_the_floor_is_refused_rather_than_downgraded_to() {
    let floor = CustodyRequirement::HUMAN_APPROVAL;
    for backend in [
        CustodyBackend::SoftwareInProcess,
        CustodyBackend::AppleKeychain,
        CustodyBackend::LinuxKernelKeyring,
    ] {
        assert!(backend.isolation().secret_enters_process_memory());
        assert!(matches!(
            floor.check(backend),
            Err(CustodyError::InsufficientIsolation { .. })
        ));
        assert_eq!(backend.isolation_proof(), None);
    }
    for backend in [
        CustodyBackend::AppleSecureEnclave,
        CustodyBackend::WindowsCng,
        CustodyBackend::HardwareToken,
    ] {
        assert!(!backend.isolation().secret_enters_process_memory());
        assert!(floor.check(backend).is_ok());
        assert!(backend.isolation_proof().is_some());
    }
}

/// No error this seam can render carries key material or a path, including the new one, which is
/// the first `CustodyError` variant to carry a payload at all.
#[test]
fn the_isolation_error_names_two_classes_and_nothing_else() {
    let rendered = CustodyError::InsufficientIsolation {
        required: IsolationClass::OsMediated,
        offered: IsolationClass::InProcess,
    }
    .to_string();
    assert!(rendered.contains("os-mediated"));
    assert!(rendered.contains("in-process"));
    assert!(!rendered.contains('/'), "{rendered}");
}
