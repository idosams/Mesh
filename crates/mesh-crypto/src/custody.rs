//! Key custody: the storage abstraction, and the export function that does not exist.
//!
//! # The threat this closes
//!
//! An agent process can read the workspace, run code the user allowed, drive the UI and observe
//! whatever memory it is given. It must still be **cryptographically unable to publish**. That
//! holds only if the human's approval key is never a value an agent-reachable code path can obtain.
//!
//! So this crate has no secret-key type at all. [`KeyCustody`] is the entire interface to a secret:
//! it signs, and it reports its public half. It has no `export`, no `to_bytes`, no `seed`, no
//! `as_secret`. There is no method to audit for misuse, because there is no method.
//!
//! The backends that will implement it — Apple Secure Enclave, the macOS Keychain, Windows CNG, the
//! Linux kernel keyring — are exactly the systems whose whole design is that the private half never
//! leaves them. This trait is shaped so that a backend which *can* export is not more convenient
//! than one which cannot: neither can, through this interface.
//!
//! # Generation is custody's job too
//!
//! [`KeyGenerator`] mints a pair inside custody and hands back only the public half. There is no
//! constructor anywhere in this crate that turns bytes into a signing key, so "generate a key here
//! and store it there" is not a shape a caller can write, and the window where a fresh secret is an
//! ordinary heap value never opens.
//!
//! This crate deliberately ships **no** implementation of either trait: an in-memory software key
//! store would be the single most useful thing to attack, and the moment it exists somebody wires
//! it into a default.

use core::fmt;

use mesh_types::Signature;

use crate::domain::SigningPayload;
use crate::keys::{KeyPair, KeyPurpose};

/// Where a secret half lives. Named so an operator can tell isolated custody from the absence of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum CustodyBackend {
    /// Apple Secure Enclave: the private half is generated in, and never leaves, the coprocessor.
    AppleSecureEnclave,
    /// The macOS Keychain, access-controlled by the operating system.
    AppleKeychain,
    /// Windows Cryptography API: Next Generation.
    WindowsCng,
    /// The Linux kernel keyring.
    LinuxKernelKeyring,
    /// A hardware token addressed over PKCS#11.
    HardwareToken,
    /// No isolation at all: the secret half is an ordinary value in this process's address space.
    ///
    /// Present so that a software fallback is **representable as what it is**. Before this variant
    /// existed the enum offered a software backend nothing but an operating-system name to wear,
    /// and a fallback that has to describe itself as `AppleKeychain` to compile is a fallback that
    /// reports isolation it does not have. Every isolation predicate here answers this variant with
    /// its weakest value, and [`CustodyBackend::isolation_proof`] answers it with `None`, so it can
    /// never reach [`HumanKeyAttestation`].
    SoftwareInProcess,
}

/// How strongly a backend isolates the secret half, as a **total order**.
///
/// [`CustodyBackend::is_hardware_isolated`] answers one question with a boolean, and three
/// genuinely different guarantees collapse into its `false` arm: a secret the process holds, a
/// secret the operating system releases to the process on demand, and a secret the operating
/// system holds and never releases. Only the last of those three survives an agent running inside
/// the same process, so a boolean cannot express the line that matters.
///
/// The order is the point. A caller states a **minimum** through [`CustodyRequirement`] and is
/// refused rather than downgraded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IsolationClass {
    /// The secret is an ordinary value in this process's address space.
    ///
    /// Defeated by anything that can read this process's memory — a debugger, a core dump, another
    /// thread, and an agent running in-process. This is the fallback's honest class.
    InProcess,
    /// The operating system holds the secret and **releases it to the process** to be used.
    ///
    /// Stronger than [`Self::InProcess`] at rest — an unlock, a policy, a user-presence prompt can
    /// stand in front of the release — and **exactly as weak in use**, because the moment it is
    /// released it is a value in this address space. A macOS Keychain generic-password item and a
    /// Linux kernel keyring payload are both this class: they store bytes, they do not sign.
    OsGated,
    /// The operating system holds the secret and **signs on the process's behalf**. It is never
    /// released.
    ///
    /// The first class where "the key is used, not held" is true, and therefore the first class
    /// where an agent with full read access to this process's memory still cannot obtain the key.
    /// This is the minimum for a human approval key ([`CustodyRequirement::HUMAN_APPROVAL`]).
    OsMediated,
    /// The secret is generated inside, and never leaves, hardware that has no export operation.
    ///
    /// Stronger than [`Self::OsMediated`] against a compromised operating-system kernel, which is
    /// the difference between a software key store the kernel can read and a coprocessor it cannot.
    HardwareNonExportable,
}

impl IsolationClass {
    /// The class's wire name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::InProcess => "in-process",
            Self::OsGated => "os-gated",
            Self::OsMediated => "os-mediated",
            Self::HardwareNonExportable => "hardware-non-exportable",
        }
    }

    /// Whether a secret in this class is ever a value in this process's address space.
    #[must_use]
    pub const fn secret_enters_process_memory(&self) -> bool {
        matches!(self, Self::InProcess | Self::OsGated)
    }
}

impl fmt::Display for IsolationClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Evidence that a backend's isolation is at least [`CustodyRequirement::HUMAN_APPROVAL`].
///
/// A zero-sized token with no public constructor, obtainable only from
/// [`CustodyBackend::isolation_proof`]. It exists so that [`HumanKeyAttestation`] — the one route
/// to a `Capability<HumanHeld>`, which is the one tier that may name canonical-head advancement —
/// takes an argument that a non-isolated custody **cannot produce a value for**. That is a
/// missing value rather than a rejected one: a software fallback does not fail this check, it
/// cannot write the call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsolationProof(());

/// The minimum isolation a caller will accept, so that a downgrade is a refusal and never a
/// silent substitution.
///
/// The failure this closes is the ordinary one: a resolver that tries the enclave, finds none, and
/// returns a software key store, leaving the caller holding something that answers `sign` exactly
/// as well and protects nothing. Here the caller names a floor, and a backend below it produces
/// [`CustodyError::InsufficientIsolation`] — which carries both classes so the message can say
/// what was wanted and what was on offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CustodyRequirement {
    minimum: IsolationClass,
}

impl CustodyRequirement {
    /// What a **human approval key** requires: the secret is never a value in this process.
    ///
    /// [`IsolationClass::OsMediated`], not [`IsolationClass::HardwareNonExportable`]. The threat
    /// this floor answers is an agent inside the user's own session with full read access to this
    /// process's memory, and an operating-system key store that signs without releasing the key
    /// already defeats it. Requiring hardware would additionally answer a compromised kernel, which
    /// `docs/threat-model.md` §11 places out of scope, and would make the requirement unsatisfiable
    /// on every platform where an Ed25519 key cannot enter a coprocessor at all.
    pub const HUMAN_APPROVAL: Self = Self {
        minimum: IsolationClass::OsMediated,
    };

    /// A requirement admitting `minimum` and everything above it.
    #[must_use]
    pub const fn at_least(minimum: IsolationClass) -> Self {
        Self { minimum }
    }

    /// The floor.
    #[must_use]
    pub const fn minimum(&self) -> IsolationClass {
        self.minimum
    }

    /// Whether `backend` meets the floor.
    #[must_use]
    pub fn admits(&self, backend: CustodyBackend) -> bool {
        backend.isolation() >= self.minimum
    }

    /// Refuse a backend below the floor, rather than downgrading to it.
    ///
    /// # Errors
    ///
    /// [`CustodyError::InsufficientIsolation`] naming both the floor and what was offered.
    pub fn check(&self, backend: CustodyBackend) -> Result<(), CustodyError> {
        if self.admits(backend) {
            Ok(())
        } else {
            Err(CustodyError::InsufficientIsolation {
                required: self.minimum,
                offered: backend.isolation(),
            })
        }
    }
}

impl CustodyBackend {
    /// What this backend actually guarantees, on the [`IsolationClass`] order.
    ///
    /// A property of the **mechanism**, not of the key. Whether a *Mesh actor key* — which plan
    /// §8.1 pins to Ed25519 — can be held by that mechanism at all is a second question, and it is
    /// answered per platform in `platform/mesh-keychain/GUARANTEES.md`, because it is a fact about
    /// what the operating system ships rather than about this type. The two are kept apart on
    /// purpose: `AppleSecureEnclave` is genuinely
    /// [`IsolationClass::HardwareNonExportable`] and genuinely cannot hold an Ed25519 key, and a
    /// single value that tried to say both would say neither.
    #[must_use]
    pub const fn isolation(&self) -> IsolationClass {
        match self {
            // The private half is generated in the coprocessor and has no export operation.
            Self::AppleSecureEnclave | Self::HardwareToken => IsolationClass::HardwareNonExportable,
            // CNG's key storage providers sign without handing the key back to the caller.
            Self::WindowsCng => IsolationClass::OsMediated,
            // Both of these STORE bytes. Neither offers an Ed25519 signing operation, so the seed
            // is read back into this address space to be used, which is `OsGated` and not
            // `OsMediated` however strong the access control in front of the read is.
            Self::AppleKeychain | Self::LinuxKernelKeyring => IsolationClass::OsGated,
            Self::SoftwareInProcess => IsolationClass::InProcess,
        }
    }

    /// Evidence that this backend meets [`CustodyRequirement::HUMAN_APPROVAL`], or `None`.
    ///
    /// The only constructor of [`IsolationProof`] in the workspace, and therefore the only route to
    /// a [`HumanKeyAttestation`] and to the `Capability<HumanHeld>` that may name canonical-head
    /// advancement.
    #[must_use]
    pub fn isolation_proof(&self) -> Option<IsolationProof> {
        if CustodyRequirement::HUMAN_APPROVAL.admits(*self) {
            Some(IsolationProof(()))
        } else {
            None
        }
    }

    /// Whether this backend keeps the private half in hardware that cannot export it.
    ///
    /// Reported, never trusted as an authorization input: a compromised process can say anything
    /// about itself. It exists so a review surface can *show* a human what is holding their key.
    #[must_use]
    pub const fn is_hardware_isolated(&self) -> bool {
        matches!(self.isolation(), IsolationClass::HardwareNonExportable)
    }

    /// The backend's wire name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::AppleSecureEnclave => "apple-secure-enclave",
            Self::AppleKeychain => "apple-keychain",
            Self::WindowsCng => "windows-cng",
            Self::LinuxKernelKeyring => "linux-kernel-keyring",
            Self::HardwareToken => "hardware-token",
            Self::SoftwareInProcess => "software-in-process",
        }
    }
}

impl fmt::Display for CustodyBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A holder of the secret half of a key pair, which signs on the holder's behalf.
///
/// Implementing this is a **security-privileged act**, and the trait is not sealed: the keychain
/// backends live in other crates and have to be able to implement it.
///
/// # What does and does not bound the set of implementations
///
/// *Nothing bounds it.* Any crate compiled into the same binary may implement this trait, and this
/// crate has no way to observe that one did. Two things read as if they bound it and do not:
///
/// * The compile-time scan in `no_secret_material` reads **only this crate's own `src/` files**.
///   It asserts an absence *within this crate* — `mesh-crypto` ships no custody — and it lists
///   nothing: it sees no other crate, and it does not see the `impl HumanKeyCustody for
///   TestCustody` in this crate's own `tests/`, which compiles as a separate crate.
/// * Which implementation a running binary composes is a fact about that binary. No decision
///   record in this repository constrains it, and none is cited here.
///
/// The control that does hold is the one in [`HumanKeyCustody`]: an implementation without the
/// person's private half cannot produce a valid signature, and canonical state advances on the
/// signature.
pub trait KeyCustody<P: KeyPurpose> {
    /// Which isolation this custody provides.
    fn backend(&self) -> CustodyBackend;

    /// The public half of the key held. The secret half has no accessor.
    fn public_key(&self) -> KeyPair<P>;

    /// Sign a framed payload.
    ///
    /// The argument is a [`SigningPayload`] rather than a byte slice on purpose: an unframed
    /// signature is one that can be replayed into another protocol position, and the only way to
    /// build a payload is to name a domain.
    ///
    /// # Errors
    ///
    /// [`CustodyError`] when the holder refuses, is locked, or is not present.
    fn sign(&self, payload: &SigningPayload) -> Result<Signature, CustodyError>;
}

/// A custody that can mint new key material. Separate from [`KeyCustody`] so that a read-only
/// holder — a hardware token that was provisioned elsewhere — is expressible.
pub trait KeyGenerator<P: KeyPurpose> {
    /// Mint a fresh pair inside custody and return only its public half.
    ///
    /// # Errors
    ///
    /// [`CustodyError`] when the holder cannot mint a key.
    fn generate(&mut self) -> Result<KeyPair<P>, CustodyError>;
}

/// Why custody could not act.
///
/// No variant carries key material, and none carries an operating-system error string: a backend
/// error can quote a path, a keychain item name or an entitlement, and those end up in logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CustodyError {
    /// The key is present but locked; the human has not authorized this use.
    Locked,
    /// The named key is not in this custody.
    NotFound,
    /// The human declined the authorization prompt.
    Declined,
    /// The backend is unavailable — no enclave, no token, no keyring.
    BackendUnavailable,
    /// The backend refused for a reason it did not classify. Never a reason to proceed.
    Refused,
    /// The backend's isolation is below what the caller required.
    ///
    /// The error a downgrade becomes. Both classes are carried so the message names the floor and
    /// the offer; neither is key material and neither is a path.
    InsufficientIsolation {
        /// The floor the caller stated.
        required: IsolationClass,
        /// What the backend actually provides.
        offered: IsolationClass,
    },
}

impl fmt::Display for CustodyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Locked => "the key is locked and its holder has not authorized this use",
            Self::NotFound => "no such key in this custody",
            Self::Declined => "the authorization was declined",
            Self::BackendUnavailable => "no key custody backend is available",
            Self::Refused => "key custody refused this operation",
            Self::InsufficientIsolation { required, offered } => {
                return write!(
                    formatter,
                    "key custody isolation {offered} is below the required {required}"
                )
            }
        })
    }
}

impl std::error::Error for CustodyError {}

/// A holder of a **human's** actor key, under operating-system key isolation.
///
/// The one thing that can produce a [`HumanKeyAttestation`], and therefore the one route to a
/// `Capability<HumanHeld>`. Separate from [`KeyCustody`] so that "this key belongs to a person" is
/// a claim some implementation makes explicitly, in a named type, rather than a boolean somebody
/// passes.
///
/// **The honest limit.** This does not make a human capability unforgeable against code compiled
/// into the same binary. A crate that implements this trait chooses what
/// [`KeyCustody::backend`] reports, so one that names a backend at or above
/// [`CustodyRequirement::HUMAN_APPROVAL`] receives an attestation for whatever key it holds, and
/// [`Capability::<crate::HumanHeld>::root`](crate::Capability) turns that into a capability that
/// may name canonical-head advancement. What the trait buys is that the claim is greppable, typed
/// and impossible to make by accident — not that it is checked.
///
/// The control that an agent cannot defeat is the *signature*: without the person's private half,
/// no attestation produces a valid approval envelope, and the relay's compare-and-swap sees only
/// the signature.
pub trait HumanKeyCustody: KeyCustody<crate::keys::ForActor> {
    /// Attest that the key this custody holds belongs to a person.
    ///
    /// **This body used to be free.** It was a provided method that built the attestation from
    /// `self.public_key()` and `self.backend()` and asked nothing else, so `impl HumanKeyCustody
    /// for MySoftwareKeyStore {}` — an empty block, no method written — produced the one value that
    /// leads to a `Capability<HumanHeld>`, which is the one tier that may name canonical-head
    /// advancement. The default answer to "is this a person's key, held out of reach?" was yes.
    ///
    /// It is not free now. The attestation cannot be built without an [`IsolationProof`], the proof
    /// comes only from [`CustodyBackend::isolation_proof`], and that returns `None` for every
    /// backend below [`CustodyRequirement::HUMAN_APPROVAL`]. An implementation that reports its
    /// backend honestly and holds the key in this process therefore gets
    /// [`CustodyError::InsufficientIsolation`] here rather than an attestation.
    ///
    /// **The backend is self-reported and this crate cannot check it.** [`KeyCustody::backend`] is
    /// a method the implementation writes, so an in-process store that names
    /// [`CustodyBackend::AppleSecureEnclave`] passes this body — which is exactly what this crate's
    /// own test double in `tests/support/mod.rs` does. That raises the cost of a mistake; it does
    /// not stop a lie, and nothing here can.
    ///
    /// # Errors
    ///
    /// [`CustodyError::InsufficientIsolation`] when the backend holding the key is one this process
    /// could read the secret out of, and any other [`CustodyError`] when the holder cannot vouch
    /// for the key.
    fn attest_human(&self) -> Result<HumanKeyAttestation, CustodyError> {
        let backend = self.backend();
        let proof = backend
            .isolation_proof()
            .ok_or(CustodyError::InsufficientIsolation {
                required: CustodyRequirement::HUMAN_APPROVAL.minimum(),
                offered: backend.isolation(),
            })?;
        Ok(HumanKeyAttestation::new(self.public_key(), backend, proof))
    }
}

/// Evidence that an `actor key` is held by a person under operating-system key isolation.
///
/// Carries the attested key so that a `Capability<HumanHeld>` cannot be minted for a *different*
/// key than the one attested to — there is no subject argument to get wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HumanKeyAttestation {
    key: KeyPair<crate::keys::ForActor>,
    backend: CustodyBackend,
    isolation: IsolationClass,
}

impl HumanKeyAttestation {
    /// Build an attestation, which requires evidence the backend is out of this process's reach.
    ///
    /// `pub(crate)` and taking an [`IsolationProof`] by value: the proof has no public constructor,
    /// so no crate outside this one can build this value at all, and the one place inside this
    /// crate that builds it is the default body of [`HumanKeyCustody::attest_human`].
    pub(crate) const fn new(
        key: KeyPair<crate::keys::ForActor>,
        backend: CustodyBackend,
        _proof: IsolationProof,
    ) -> Self {
        Self {
            key,
            backend,
            isolation: backend.isolation(),
        }
    }

    /// The attested key.
    #[must_use]
    pub const fn key(&self) -> &KeyPair<crate::keys::ForActor> {
        &self.key
    }

    /// The custody that attested.
    #[must_use]
    pub const fn backend(&self) -> CustodyBackend {
        self.backend
    }

    /// The isolation the attesting custody provides.
    ///
    /// Carried on the value rather than recomputed from [`Self::backend`] so that a review surface
    /// showing a human what is holding their key reads the same field the attestation was admitted
    /// on. It is never below [`CustodyRequirement::HUMAN_APPROVAL`], because an attestation below
    /// it cannot be constructed.
    #[must_use]
    pub const fn isolation(&self) -> IsolationClass {
        self.isolation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every backend this enum names, so a variant added without a decision about its isolation
    /// fails to compile here rather than defaulting into somebody's `_` arm.
    const EVERY_BACKEND: [CustodyBackend; 6] = [
        CustodyBackend::AppleSecureEnclave,
        CustodyBackend::AppleKeychain,
        CustodyBackend::WindowsCng,
        CustodyBackend::LinuxKernelKeyring,
        CustodyBackend::HardwareToken,
        CustodyBackend::SoftwareInProcess,
    ];

    #[test]
    fn only_hardware_backends_report_hardware_isolation() {
        assert!(CustodyBackend::AppleSecureEnclave.is_hardware_isolated());
        assert!(CustodyBackend::HardwareToken.is_hardware_isolated());
        for backend in [
            CustodyBackend::AppleKeychain,
            CustodyBackend::WindowsCng,
            CustodyBackend::LinuxKernelKeyring,
            CustodyBackend::SoftwareInProcess,
        ] {
            assert!(!backend.is_hardware_isolated(), "{backend}");
        }
    }

    /// The boolean and the order must never disagree: `is_hardware_isolated` is now derived from
    /// `isolation`, and this is the test that notices if somebody re-hard-codes it.
    #[test]
    fn the_hardware_boolean_is_exactly_the_top_of_the_isolation_order() {
        for backend in EVERY_BACKEND {
            assert_eq!(
                backend.is_hardware_isolated(),
                backend.isolation() == IsolationClass::HardwareNonExportable,
                "{backend}"
            );
        }
    }

    /// The order is what the whole requirement mechanism rests on. Asserted as a chain rather than
    /// trusted to the derive, because reordering the variants silently reorders the guarantee.
    #[test]
    fn the_isolation_order_runs_from_no_protection_to_hardware() {
        assert!(IsolationClass::InProcess < IsolationClass::OsGated);
        assert!(IsolationClass::OsGated < IsolationClass::OsMediated);
        assert!(IsolationClass::OsMediated < IsolationClass::HardwareNonExportable);
        assert!(IsolationClass::InProcess.secret_enters_process_memory());
        assert!(IsolationClass::OsGated.secret_enters_process_memory());
        assert!(!IsolationClass::OsMediated.secret_enters_process_memory());
        assert!(!IsolationClass::HardwareNonExportable.secret_enters_process_memory());
    }

    /// The line the human approval key is held to: the secret is never a value in this process.
    #[test]
    fn human_approval_admits_exactly_the_backends_that_never_release_the_secret() {
        for backend in EVERY_BACKEND {
            assert_eq!(
                CustodyRequirement::HUMAN_APPROVAL.admits(backend),
                !backend.isolation().secret_enters_process_memory(),
                "{backend}"
            );
            assert_eq!(
                CustodyRequirement::HUMAN_APPROVAL.check(backend).is_ok(),
                backend.isolation_proof().is_some(),
                "{backend}: the proof and the requirement disagree"
            );
        }
    }

    /// A requirement refuses; it never substitutes. The error names both classes so the refusal is
    /// actionable without a second lookup.
    #[test]
    fn a_backend_below_the_floor_is_refused_and_never_downgraded_to() {
        let outcome = CustodyRequirement::HUMAN_APPROVAL.check(CustodyBackend::SoftwareInProcess);
        assert_eq!(
            outcome,
            Err(CustodyError::InsufficientIsolation {
                required: IsolationClass::OsMediated,
                offered: IsolationClass::InProcess,
            })
        );
        assert!(CustodyRequirement::at_least(IsolationClass::InProcess)
            .check(CustodyBackend::SoftwareInProcess)
            .is_ok());
    }

    /// The one route to a `Capability<HumanHeld>` is closed to a software backend by a value that
    /// does not exist, not by a check that could be skipped.
    #[test]
    fn a_software_backend_has_no_isolation_proof_to_attest_with() {
        assert_eq!(CustodyBackend::SoftwareInProcess.isolation_proof(), None);
        assert_eq!(CustodyBackend::AppleKeychain.isolation_proof(), None);
        assert_eq!(CustodyBackend::LinuxKernelKeyring.isolation_proof(), None);
        assert!(CustodyBackend::AppleSecureEnclave
            .isolation_proof()
            .is_some());
        assert!(CustodyBackend::WindowsCng.isolation_proof().is_some());
        assert!(CustodyBackend::HardwareToken.isolation_proof().is_some());
    }

    #[test]
    fn every_backend_and_class_has_a_distinct_wire_name() {
        let mut names: Vec<&str> = EVERY_BACKEND.iter().map(CustodyBackend::as_str).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "two backends share a wire name");
        assert_eq!(IsolationClass::InProcess.as_str(), "in-process");
        assert_eq!(
            IsolationClass::HardwareNonExportable.as_str(),
            "hardware-non-exportable"
        );
    }

    /// Every custody error must stop the caller. None of them is a "carry on" value.
    #[test]
    fn no_custody_error_renders_as_a_secret_or_a_path() {
        for error in [
            CustodyError::Locked,
            CustodyError::NotFound,
            CustodyError::Declined,
            CustodyError::BackendUnavailable,
            CustodyError::Refused,
            CustodyError::InsufficientIsolation {
                required: IsolationClass::OsMediated,
                offered: IsolationClass::InProcess,
            },
        ] {
            let rendered = error.to_string();
            assert!(!rendered.contains('/'), "{rendered} looks like a path");
            assert!(!rendered.is_empty());
        }
    }
}
