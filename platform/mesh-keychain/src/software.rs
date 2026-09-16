//! The software fallback: a real signer, with the weaker guarantee stated in its own type.
//!
//! # What it is for
//!
//! Until this existed, nothing in the workspace outside a test could produce an Ed25519 signature.
//! `mesh-crypto` verifies and deliberately holds no secret, and its only signer lives under
//! `tests/`, which ships in no binary. An actor therefore could not be identified by its own key,
//! which is what authenticated replication needs. [`SoftwareCustody`] is the first implementation
//! of [`KeyCustody`] in the programme and closes that.
//!
//! # What it is not for
//!
//! **It cannot hold a human approval key**, and that is enforced by an absence rather than a
//! check. `HumanKeyCustody::attest_human` needs a `mesh_crypto::IsolationProof`, the only
//! constructor of one is `CustodyBackend::isolation_proof`, and it answers
//! [`CustodyBackend::SoftwareInProcess`] with `None`. This type does not implement
//! `HumanKeyCustody` at all, `key_isolation.rs` asserts that no source in this crate does, and if
//! one did it would still be unable to build the value its own default body returns. So no
//! `Capability<HumanHeld>` is reachable from a software-held key, and `HumanHeld` is the only tier
//! whose action vocabulary contains `AdvanceCanonicalHead`.
//!
//! That chain is the whole security argument for shipping a software fallback at all: an agent
//! that fully compromises this type gets an actor key it can author ChangeSets with, and gets no
//! nearer to publishing than it was before.
//!
//! # The honest limit
//!
//! The secret scalar is in this process's address space. Every control below is real and none of
//! them survives an attacker who can read this process's memory:
//!
//! * There is no accessor, no `to_bytes`, no `Clone`, no `Debug` of the secret, and no
//!   serialization — so no *code path in the workspace* exports it.
//! * `ed25519_dalek::SigningKey` is `ZeroizeOnDrop` under the pinned `zeroize` feature, so the
//!   scalar is overwritten when custody drops rather than left in a freed allocation.
//! * The seed never becomes a long-lived value: it is read into a stack buffer, consumed, and
//!   scrubbed before the constructor returns, and there is **no constructor that takes seed bytes
//!   from a caller**, so "generate here, store there" is not a shape anybody can write.
//!
//! A debugger, a core dump, `task_for_pid`, `ptrace`, or another thread in the same process reads
//! it anyway. `platform/mesh-keychain/GUARANTEES.md` says so in the row for this backend, and
//! [`mesh_crypto::IsolationClass::InProcess`] is the value this type reports.

use ed25519_dalek::{Signer as _, SigningKey};
use mesh_crypto::{
    CustodyBackend, CustodyError, ForActor, ForDevice, KeyCustody, KeyGenerator, KeyPair,
    KeyPurpose, SigningPayload,
};
use mesh_types::Signature;

use crate::entropy::{fill_from_os, scrub, SEED_BYTES};

/// An Ed25519 key held in this process, which signs and cannot be exported.
///
/// Generic over [`KeyPurpose`] so an `actor key` and a `device key` are different types here too:
/// the separation `mesh-crypto` makes at the key is not worth having if custody launders it.
///
/// Deliberately not `Clone`, not `Copy`, not `Default`, and not serializable. Each of those would
/// be a second copy of the scalar, in a place the drop that zeroizes it does not reach.
pub struct SoftwareCustody<P: KeyPurpose> {
    signing: SigningKey,
    public: KeyPair<P>,
}

impl<P: KeyPurpose> SoftwareCustody<P> {
    /// Mint a fresh key from the operating system's random source.
    ///
    /// The only constructor. There is no `from_seed`, no `from_bytes` and no `from_hex`: a
    /// constructor that accepts secret bytes makes the secret an ordinary value at every call
    /// site, and the window it opens is the one this crate exists to keep shut.
    ///
    /// # Errors
    ///
    /// [`CustodyError::BackendUnavailable`] when the operating system's random source cannot be
    /// read, or on a platform this crate has no source for. Never a partially seeded key.
    pub fn generate() -> Result<Self, CustodyError> {
        let mut seed = [0u8; SEED_BYTES];
        let outcome = fill_from_os(&mut seed).map(|()| {
            let signing = SigningKey::from_bytes(&seed);
            let public = KeyPair::<P>::from_public_bytes(signing.verifying_key().to_bytes());
            Self { signing, public }
        });
        scrub(&mut seed);
        outcome
    }
}

impl<P: KeyPurpose> KeyCustody<P> for SoftwareCustody<P> {
    fn backend(&self) -> CustodyBackend {
        CustodyBackend::SoftwareInProcess
    }

    fn public_key(&self) -> KeyPair<P> {
        self.public
    }

    fn sign(&self, payload: &SigningPayload) -> Result<Signature, CustodyError> {
        Ok(Signature::from_bytes(
            self.signing.sign(payload.as_bytes()).to_bytes(),
        ))
    }
}

impl<P: KeyPurpose> KeyGenerator<P> for SoftwareCustody<P> {
    /// Replace the held key with a fresh one and return the new public half.
    ///
    /// The previous [`SigningKey`] is dropped, and dropping it zeroizes the scalar.
    ///
    /// # Errors
    ///
    /// [`CustodyError::BackendUnavailable`] when the random source cannot be read. On failure the
    /// existing key is left in place: a generator that could half-succeed would leave custody
    /// holding a key its public half no longer names.
    fn generate(&mut self) -> Result<KeyPair<P>, CustodyError> {
        let replacement = Self::generate()?;
        *self = replacement;
        Ok(self.public)
    }
}

/// The public half and the backend, and nothing else.
///
/// Written by hand because a derived `Debug` on this struct would print the `SigningKey`, and the
/// place a secret most often escapes is a log line somebody added while debugging something else.
impl<P: KeyPurpose> core::fmt::Debug for SoftwareCustody<P> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("SoftwareCustody")
            .field("backend", &CustodyBackend::SoftwareInProcess.as_str())
            .field("role", &self.public.role().as_str())
            .field("public_key", &self.public.to_hex())
            .finish()
    }
}

/// A software-held `actor key`.
pub type SoftwareActorCustody = SoftwareCustody<ForActor>;

/// A software-held `device key`.
pub type SoftwareDeviceCustody = SoftwareCustody<ForDevice>;

#[cfg(test)]
mod tests {
    use super::*;

    use mesh_crypto::{DomainSeparator, Ed25519, IsolationClass, SignatureScheme};

    #[test]
    fn a_software_signature_verifies_under_the_audited_verifier() {
        let custody = SoftwareActorCustody::generate().expect("os entropy");
        let payload = SigningPayload::new(DomainSeparator::CAPABILITY_TOKEN, b"body");
        let signature = custody.sign(&payload).expect("software custody signs");
        Ed25519::verify(
            &custody.public_key().public_key(),
            payload.as_bytes(),
            &signature,
        )
        .expect("the audited verifier accepts a signature this crate made");
    }

    #[test]
    fn a_signature_does_not_verify_under_a_different_key() {
        let mine = SoftwareActorCustody::generate().expect("os entropy");
        let theirs = SoftwareActorCustody::generate().expect("os entropy");
        let payload = SigningPayload::new(DomainSeparator::CAPABILITY_TOKEN, b"body");
        let signature = mine.sign(&payload).expect("sign");
        assert!(Ed25519::verify(
            &theirs.public_key().public_key(),
            payload.as_bytes(),
            &signature
        )
        .is_err());
    }

    #[test]
    fn software_custody_reports_the_class_it_actually_provides() {
        let custody = SoftwareDeviceCustody::generate().expect("os entropy");
        assert_eq!(custody.backend(), CustodyBackend::SoftwareInProcess);
        assert_eq!(custody.backend().isolation(), IsolationClass::InProcess);
        assert!(!custody.backend().is_hardware_isolated());
        assert_eq!(custody.backend().isolation_proof(), None);
    }

    #[test]
    fn rotating_the_key_names_a_new_public_half() {
        let mut custody = SoftwareActorCustody::generate().expect("os entropy");
        let before = custody.public_key();
        let after = KeyGenerator::generate(&mut custody).expect("os entropy");
        assert_ne!(before, after);
        assert_eq!(custody.public_key(), after);
    }

    #[test]
    fn the_debug_form_carries_the_public_half_and_no_more() {
        let custody = SoftwareActorCustody::generate().expect("os entropy");
        let rendered = format!("{custody:?}");
        assert!(rendered.contains(&custody.public_key().to_hex()));
        assert!(!rendered.to_ascii_lowercase().contains("signing"));
        assert!(!rendered.to_ascii_lowercase().contains("secret"));
    }
}
