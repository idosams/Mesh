//! What each platform can actually do with an **Ed25519** key, as data.
//!
//! # Why this table exists separately from `IsolationClass`
//!
//! `mesh_crypto::CustodyBackend::isolation` says what a *mechanism* guarantees. It does not say
//! whether that mechanism can hold the key Mesh actually uses, and on the platform this programme
//! cares about most, it cannot.
//!
//! **The Apple Secure Enclave cannot hold an Ed25519 key.** It generates and holds NIST P-256
//! keys and nothing else: `SecKeyCreateRandomKey` with `kSecAttrTokenIDSecureEnclave` accepts
//! `kSecAttrKeyTypeECSECPrimeRandom` at 256 bits, and CryptoKit exposes exactly
//! `SecureEnclave.P256`. `Curve25519.Signing.PrivateKey` exists in CryptoKit and is software —
//! there is no `SecureEnclave.Curve25519`. Plan §8.1 pins Ed25519 for actor keys, so the
//! combination "Mesh Ed25519 actor key, in the Secure Enclave" is not a thing that can be built.
//! The separate P-256 human-approval credential does not change that actor-key fact.
//!
//! That matters because `docs/threat-model.md` §11 says key isolation "is delegated to the OS
//! keychain and Secure Enclave", and a reader is entitled to take that as a description of where
//! the key will end up. Recording the constraint in code, with a test over it, is how that stops
//! being a claim nobody checked.
//!
//! # What this table is
//!
//! One row per (platform, mechanism) pair Mesh would plausibly use, each saying whether an Ed25519
//! private key can live there and what isolation is therefore *reachable* on that platform. It is
//! a documented survey of other people's platforms, not a measurement of them — the `note` on each
//! row names the specific API or firmware fact it rests on so a reviewer can check it rather than
//! trust it, and `GUARANTEES.md` carries the same rows in prose.

use mesh_crypto::{CustodyBackend, IsolationClass};

/// A platform, named the way this table indexes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Platform {
    /// macOS, and by extension every Apple platform with a Secure Enclave.
    MacOs,
    /// Windows.
    Windows,
    /// Linux.
    Linux,
}

impl Platform {
    /// Every platform in the table.
    pub const ALL: [Self; 3] = [Self::MacOs, Self::Windows, Self::Linux];

    /// The platform's wire name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::MacOs => "macos",
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }

    /// The platform this binary was compiled for, if it is one this table knows.
    #[must_use]
    pub const fn host() -> Option<Self> {
        #[cfg(target_os = "macos")]
        {
            Some(Self::MacOs)
        }
        #[cfg(target_os = "windows")]
        {
            Some(Self::Windows)
        }
        #[cfg(target_os = "linux")]
        {
            Some(Self::Linux)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            None
        }
    }
}

impl core::fmt::Display for Platform {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One (platform, mechanism) row: can an Ed25519 private key live there, and is it implemented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyStoreSupport {
    platform: Platform,
    backend: CustodyBackend,
    holds_ed25519: bool,
    implemented: bool,
    note: &'static str,
}

impl KeyStoreSupport {
    /// The platform.
    #[must_use]
    pub const fn platform(&self) -> Platform {
        self.platform
    }

    /// The mechanism.
    #[must_use]
    pub const fn backend(&self) -> CustodyBackend {
        self.backend
    }

    /// Whether an **Ed25519** private key can live in this mechanism at all.
    #[must_use]
    pub const fn holds_ed25519(&self) -> bool {
        self.holds_ed25519
    }

    /// Whether Mesh has built this backend. `false` for every row but the software fallback.
    #[must_use]
    pub const fn implemented(&self) -> bool {
        self.implemented
    }

    /// The specific platform fact this row rests on.
    #[must_use]
    pub const fn note(&self) -> &'static str {
        self.note
    }

    /// The isolation this row delivers **for a Mesh actor key**.
    ///
    /// The mechanism's class when it can hold the key, and [`IsolationClass::InProcess`] when it
    /// cannot — because a mechanism that cannot hold the key protects nothing about it. This is
    /// the function that stops `AppleSecureEnclave` reading as hardware backing for a key it can
    /// never contain.
    #[must_use]
    pub const fn reachable_isolation(&self) -> IsolationClass {
        if self.holds_ed25519 {
            self.backend.isolation()
        } else {
            IsolationClass::InProcess
        }
    }
}

/// The survey. Every row is a claim about somebody else's platform, with the fact it rests on.
pub const KEY_STORE_SUPPORT: [KeyStoreSupport; 7] = [
    KeyStoreSupport {
        platform: Platform::MacOs,
        backend: CustodyBackend::AppleSecureEnclave,
        holds_ed25519: false,
        implemented: false,
        note: "The Secure Enclave generates and holds NIST P-256 only. SecKeyCreateRandomKey with \
               kSecAttrTokenIDSecureEnclave accepts kSecAttrKeyTypeECSECPrimeRandom at 256 bits, \
               and CryptoKit exposes SecureEnclave.P256 with no Curve25519 counterpart. An \
               Ed25519 actor key cannot enter it, so hardware backing is UNAVAILABLE for Mesh on \
               Apple platforms, not merely unbuilt.",
    },
    KeyStoreSupport {
        platform: Platform::MacOs,
        backend: CustodyBackend::AppleKeychain,
        holds_ed25519: true,
        implemented: false,
        note: "The keychain stores an Ed25519 seed as a generic-password item, under an access \
               control that can require user presence. It has no Ed25519 signing operation, so \
               the seed is READ BACK into this address space to be used: os-gated at rest, \
               in-process in use.",
    },
    KeyStoreSupport {
        platform: Platform::Windows,
        backend: CustodyBackend::WindowsCng,
        holds_ed25519: false,
        implemented: false,
        note: "CNG's key storage providers sign without releasing the key, which is why the \
               mechanism is os-mediated. Neither the Microsoft Software Key Storage Provider nor \
               the Platform Crypto Provider offers Ed25519 — the shipping algorithm set is RSA \
               and the NIST curves — so an Ed25519 actor key needs a third-party KSP that Mesh \
               does not ship.",
    },
    KeyStoreSupport {
        platform: Platform::Linux,
        backend: CustodyBackend::LinuxKernelKeyring,
        holds_ed25519: true,
        implemented: false,
        note: "The kernel keyring holds the seed as a payload. Its asymmetric-key type signs with \
               RSA and ECDSA, not Ed25519, so as with the macOS keychain the seed is read back \
               to be used.",
    },
    KeyStoreSupport {
        platform: Platform::MacOs,
        backend: CustodyBackend::HardwareToken,
        holds_ed25519: true,
        implemented: false,
        note: "An external PKCS#11 or PIV token. Ed25519 in non-exportable hardware is real here \
               and is the ONLY route to it for a Mesh actor key: YubiKey 5 carries Ed25519 in the \
               OpenPGP applet from firmware 5.2.3 and in PIV from 5.7. It needs a device the user \
               bought.",
    },
    KeyStoreSupport {
        platform: Platform::Windows,
        backend: CustodyBackend::HardwareToken,
        holds_ed25519: true,
        implemented: false,
        note: "The same external token, over the same PKCS#11 interface.",
    },
    KeyStoreSupport {
        platform: Platform::Linux,
        backend: CustodyBackend::HardwareToken,
        holds_ed25519: true,
        implemented: false,
        note: "The same external token, over the same PKCS#11 interface.",
    },
];

/// The best isolation reachable for a Mesh **actor key** on `platform`, over backends Mesh has
/// actually built.
///
/// [`IsolationClass::InProcess`] for every platform today, because the software fallback is the
/// only implemented backend. It is a function of `implemented` rather than of the survey so that
/// it reports what a user would get, and it rises the moment a backend lands rather than when a
/// document is edited.
#[must_use]
pub fn reachable_today(platform: Platform) -> IsolationClass {
    KEY_STORE_SUPPORT
        .iter()
        .filter(|row| row.platform == platform && row.implemented)
        .map(KeyStoreSupport::reachable_isolation)
        .max()
        .unwrap_or(IsolationClass::InProcess)
}

/// The best isolation that could be reached on `platform` if every surveyed backend were built.
///
/// The number that says what building the remaining backends buys. On every platform in the table
/// it is [`IsolationClass::HardwareNonExportable`] **only through an external token**, which is
/// the finding this module exists to record.
#[must_use]
pub fn reachable_if_every_backend_were_built(platform: Platform) -> IsolationClass {
    KEY_STORE_SUPPORT
        .iter()
        .filter(|row| row.platform == platform)
        .map(KeyStoreSupport::reachable_isolation)
        .max()
        .unwrap_or(IsolationClass::InProcess)
}

/// The rows for `platform`, in table order.
#[must_use]
pub fn rows_for(platform: Platform) -> Vec<KeyStoreSupport> {
    KEY_STORE_SUPPORT
        .iter()
        .copied()
        .filter(|row| row.platform == platform)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The finding, as an assertion: no built-in operating-system key store on any platform in
    /// this survey holds a Mesh actor key in non-exportable hardware.
    #[test]
    fn no_built_in_platform_key_store_reaches_hardware_isolation_for_an_actor_key() {
        for row in KEY_STORE_SUPPORT {
            if row.backend == CustodyBackend::HardwareToken {
                continue;
            }
            assert_ne!(
                row.reachable_isolation(),
                IsolationClass::HardwareNonExportable,
                "{} / {} claims hardware backing without an external token",
                row.platform,
                row.backend
            );
        }
    }

    /// The Secure Enclave row is the one a reader will assume says the opposite. It has to keep
    /// saying this, and the reason has to keep being in the row.
    #[test]
    fn the_secure_enclave_row_says_it_cannot_hold_the_key() {
        let enclave = KEY_STORE_SUPPORT
            .iter()
            .find(|row| row.backend == CustodyBackend::AppleSecureEnclave)
            .expect("the survey has a Secure Enclave row");
        assert!(!enclave.holds_ed25519());
        assert_eq!(enclave.reachable_isolation(), IsolationClass::InProcess);
        assert!(enclave.backend().is_hardware_isolated(), "the MECHANISM is");
        assert!(enclave.note().contains("P-256"));
    }

    #[test]
    fn nothing_but_the_software_fallback_is_implemented_today() {
        for row in KEY_STORE_SUPPORT {
            assert!(!row.implemented(), "{} / {}", row.platform, row.backend);
        }
        for platform in Platform::ALL {
            assert_eq!(reachable_today(platform), IsolationClass::InProcess);
        }
    }

    #[test]
    fn building_every_surveyed_backend_reaches_hardware_only_through_a_token() {
        for platform in Platform::ALL {
            assert_eq!(
                reachable_if_every_backend_were_built(platform),
                IsolationClass::HardwareNonExportable
            );
            assert!(rows_for(platform)
                .iter()
                .filter(|row| row.reachable_isolation() == IsolationClass::HardwareNonExportable)
                .all(|row| row.backend() == CustodyBackend::HardwareToken));
        }
    }

    #[test]
    fn every_platform_has_at_least_one_row_and_every_row_carries_its_reason() {
        for platform in Platform::ALL {
            assert!(!rows_for(platform).is_empty(), "{platform}");
        }
        for row in KEY_STORE_SUPPORT {
            assert!(row.note().len() > 40, "{} / {}", row.platform, row.backend);
        }
    }
}
