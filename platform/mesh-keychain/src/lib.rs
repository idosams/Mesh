//! Platform key custody: the backends that hold an actor's private key, and the guarantee each one
//! actually provides.
//!
//! # The actor-key claim
//!
//! **An agent-held Ed25519 actor key cannot advance canonical state.** The legacy capability route
//! needs a `Capability<HumanHeld>` minted from a `mesh_crypto::HumanKeyAttestation`; software actor
//! custody cannot construct the required isolation proof. The current macOS route is deliberately
//! separate: [`SecureEnclaveApprovalCredential`] can sign only one typed P-256 human-approval
//! statement after Security.framework verifies fresh user presence, and the daemon re-verifies its
//! exact context before advancing the shared version.
//!
//! An agent that fully compromises the ordinary actor-key path can author ChangeSets and request an
//! approval ceremony, but it cannot export the approval key or complete that ceremony without the
//! operating system's user-presence result. `tests/key_isolation.rs` walks the actor-key attacks;
//! `tests/secure_enclave_ffi.rs` checks the separate native boundary without prompting a person.
//!
//! # What is here
//!
//! * [`SoftwareCustody`] — the software fallback. A real Ed25519 signer, keyed from the operating
//!   system's random source, with no export path and no constructor that takes secret bytes. Its
//!   weaker guarantee is [`mesh_crypto::IsolationClass::InProcess`], reported by the type and
//!   written down in `GUARANTEES.md`.
//! * [`SecureEnclaveApprovalCredential`] — a separate P-256 approval credential generated and
//!   used inside Apple's Secure Enclave. It signs only a typed Mesh human-approval receipt after
//!   macOS satisfies the credential's user-presence policy; it is not an actor key or a generic
//!   byte-signing API.
//! * [`support`] — what each platform's key store can actually do with an **Ed25519 actor** key. The
//!   load-bearing row is that the Apple Secure Enclave holds NIST P-256 only, so hardware backing
//!   for a Mesh actor key is *unavailable* there rather than unbuilt.
//!
//! # What is not here, stated plainly
//!
//! No operating-system backend exists for Mesh's Ed25519 actor key. The macOS Secure Enclave
//! backend is deliberately limited to the separate P-256 human-approval credential. Its foreign
//! boundary is isolated in `secure_enclave.rs`/`secure_enclave.m`, and read-only load plus exact
//! receipt verification are automated; enrollment and signing still require an interactive macOS
//! acceptance run before a build may claim the ceremony works on that machine.

// Not `forbid`, because `mesh-crypto`'s spelling would be a promise this crate cannot keep once a
// keychain backend lands: reaching Security.framework or CNG is a foreign call and a foreign call
// is `unsafe`. `warn` is the workspace default and `cargo clippy --workspace --all-targets -- -D
// warnings` turns it into an error, so the first `unsafe` block here fails the build and has to be
// argued for in the pull request that adds it, which is the review this crate needs rather than a
// lint it would have to delete.
#![deny(unsafe_code)]

mod entropy;
mod no_export;
mod secure_enclave;
mod software;

pub mod support;

pub use crate::secure_enclave::{
    fresh_approval_challenge, SecureEnclaveApprovalCredential, SecureEnclaveApprovalError,
};
pub use crate::software::{SoftwareActorCustody, SoftwareCustody, SoftwareDeviceCustody};
pub use crate::support::{
    reachable_if_every_backend_were_built, reachable_today, KeyStoreSupport, Platform,
    KEY_STORE_SUPPORT,
};

/// The crate's name, so a diagnostic can say which custody a key came from.
pub const CRATE_NAME: &str = "mesh-keychain";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-keychain");
    }
}
