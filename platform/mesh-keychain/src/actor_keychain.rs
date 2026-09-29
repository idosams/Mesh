//! Persistent macOS execution identity. Storage is OS-gated; Ed25519 signing uses process memory.
use ed25519_dalek::{Signer as _, SigningKey};
use mesh_crypto::{CustodyBackend, CustodyError, ForActor, KeyCustody, KeyPair, SigningPayload};
use mesh_types::{PublicKey, Signature};

struct Seed([u8; 32]);
impl Drop for Seed {
    fn drop(&mut self) {
        crate::entropy::scrub(&mut self.0);
    }
}

/// An app-scoped persistent actor identity, never a human approval credential.
/// The handle holds only its native installation account and expected public identity. Each
/// signature reloads the stored value, checks that identity, and drops the temporary signing key.
/// No secret import/export, implicit creation, replacement, deletion or fallback is available.
#[derive(Debug)]
pub struct AppleActorCustody {
    account: String,
    public: KeyPair<ForActor>,
}
impl AppleActorCustody {
    /// Read-only check of the signed Mesh application identity. Never accesses a keychain item.
    pub fn availability() -> Result<(), CustodyError> {
        backend::availability()
    }

    /// Explicit first provisioning for a native installation identifier. Existing items refuse,
    /// including after a partial provisioning failure. The caller must persist/admit the returned
    /// public identity before using it for a worker. It must retain failed provisioning evidence.
    pub fn create(installation: [u8; 16]) -> Result<Self, CustodyError> {
        backend::availability()?;
        Self::create_using(installation, backend::create)
    }

    /// Open an existing identity against an independently admitted public key. Missing, locked,
    /// malformed or changed items refuse; none of those conditions creates another identity.
    pub fn open(installation: [u8; 16], expected: PublicKey) -> Result<Self, CustodyError> {
        Self::open_using(installation, expected, backend::load)
    }

    fn account(installation: [u8; 16]) -> String {
        installation
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
    fn create_using(
        installation: [u8; 16],
        store: impl FnOnce(&str, &[u8; 32]) -> Result<(), CustodyError>,
    ) -> Result<Self, CustodyError> {
        let account = Self::account(installation);
        let mut seed = Seed([0; 32]);
        crate::entropy::fill_from_os(&mut seed.0)?;
        let signing = SigningKey::from_bytes(&seed.0);
        let public = KeyPair::from_public_bytes(signing.verifying_key().to_bytes());
        store(&account, &seed.0)?;
        Ok(Self { account, public })
    }
    fn open_using(
        installation: [u8; 16],
        expected: PublicKey,
        load: impl FnOnce(&str, &mut [u8; 32]) -> Result<(), CustodyError>,
    ) -> Result<Self, CustodyError> {
        let value = Self {
            account: Self::account(installation),
            public: KeyPair::from_public_key(expected),
        };
        value.with_signing(load, |_| ())?;
        Ok(value)
    }
    fn with_signing<T>(
        &self,
        load: impl FnOnce(&str, &mut [u8; 32]) -> Result<(), CustodyError>,
        use_key: impl FnOnce(&SigningKey) -> T,
    ) -> Result<T, CustodyError> {
        let mut seed = Seed([0; 32]);
        load(&self.account, &mut seed.0)?;
        let signing = SigningKey::from_bytes(&seed.0);
        if signing.verifying_key().to_bytes() != *self.public.as_bytes() {
            return Err(CustodyError::Refused);
        }
        Ok(use_key(&signing))
    }
}
impl KeyCustody<ForActor> for AppleActorCustody {
    fn backend(&self) -> CustodyBackend {
        CustodyBackend::AppleKeychain
    }
    fn public_key(&self) -> KeyPair<ForActor> {
        self.public
    }
    fn sign(&self, payload: &SigningPayload) -> Result<Signature, CustodyError> {
        self.with_signing(backend::load, |signing| {
            Signature::from_bytes(signing.sign(payload.as_bytes()).to_bytes())
        })
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod backend {
    use super::CustodyError;
    unsafe extern "C" {
        fn mesh_actor_keychain_available() -> i32;
        fn mesh_actor_keychain_create(account: *const u8, length: usize, value: *const u8) -> i32;
        fn mesh_actor_keychain_load(account: *const u8, length: usize, value: *mut u8) -> i32;
    }
    #[cfg(test)]
    #[test]
    fn native_stored_shape_refuses_malformed_or_changed_item_without_copying() {
        unsafe extern "C" {
            fn mesh_actor_keychain_shape_for_test(mode: u32) -> i32;
        }
        // SAFETY: fixed fixture selector only, no pointers or keychain operations.
        assert_eq!(unsafe { mesh_actor_keychain_shape_for_test(0) }, 0);
        for mode in 1..=9 {
            // SAFETY: same pure fixture seam.
            assert_eq!(
                unsafe { mesh_actor_keychain_shape_for_test(mode) },
                3,
                "mode {mode}"
            );
        }
    }
    fn result(code: i32) -> Result<(), CustodyError> {
        match code {
            0 => Ok(()),
            1 => Err(CustodyError::NotFound),
            2 => Err(CustodyError::Locked),
            3 => Err(CustodyError::Refused),
            _ => Err(CustodyError::BackendUnavailable),
        }
    }
    pub(super) fn availability() -> Result<(), CustodyError> {
        // SAFETY: no pointers, item operations or retained foreign state.
        result(unsafe { mesh_actor_keychain_available() })
    }
    pub(super) fn create(account: &str, value: &[u8; 32]) -> Result<(), CustodyError> {
        // SAFETY: account is live for its explicit length; value has exactly 32 readable bytes.
        // The bridge copies synchronously and never retains either pointer.
        result(unsafe {
            mesh_actor_keychain_create(account.as_ptr(), account.len(), value.as_ptr())
        })
    }
    pub(super) fn load(account: &str, value: &mut [u8; 32]) -> Result<(), CustodyError> {
        // SAFETY: account is live for its explicit length; value has 32 writable bytes. The bridge
        // checks stored length before copying and never retains either pointer.
        result(unsafe {
            mesh_actor_keychain_load(account.as_ptr(), account.len(), value.as_mut_ptr())
        })
    }
}
#[cfg(not(target_os = "macos"))]
mod backend {
    use super::CustodyError;
    pub(super) fn availability() -> Result<(), CustodyError> {
        Err(CustodyError::BackendUnavailable)
    }
    pub(super) fn create(_: &str, _: &[u8; 32]) -> Result<(), CustodyError> {
        availability()
    }
    pub(super) fn load(_: &str, _: &mut [u8; 32]) -> Result<(), CustodyError> {
        availability()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mesh_crypto::{DomainSeparator, Ed25519, SignatureScheme};
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Store(RefCell<BTreeMap<String, [u8; 32]>>);
    impl Store {
        fn create(&self, account: &str, value: &[u8; 32]) -> Result<(), CustodyError> {
            let mut values = self.0.borrow_mut();
            if values.contains_key(account) {
                return Err(CustodyError::Refused);
            }
            values.insert(account.into(), *value);
            Ok(())
        }
        fn load(&self, account: &str, value: &mut [u8; 32]) -> Result<(), CustodyError> {
            *value = *self.0.borrow().get(account).ok_or(CustodyError::NotFound)?;
            Ok(())
        }
    }
    #[test]
    fn ordinary_test_binary_has_no_persistent_actor_authority() {
        // A cargo test executable is not the enrolled signed dev.mesh.desktop application.
        // Check the read-only preflight first so even an unexpectedly eligible test binary never
        // reaches a provisioning call. No user keychain item is created by this regression.
        assert_eq!(
            AppleActorCustody::availability(),
            Err(CustodyError::BackendUnavailable)
        );
        assert!(matches!(
            AppleActorCustody::create([9; 16]),
            Err(CustodyError::BackendUnavailable)
        ));
        assert!(matches!(
            AppleActorCustody::open([9; 16], PublicKey::from_bytes([0; 32])),
            Err(CustodyError::BackendUnavailable)
        ));
    }
    #[test]
    fn actor_reopens_exact_identity_and_create_never_replaces_it() {
        let store = Store::default();
        let original = AppleActorCustody::create_using([1; 16], |a, s| store.create(a, s)).unwrap();
        let expected = original.public_key().public_key();
        assert!(AppleActorCustody::create_using([1; 16], |a, s| store.create(a, s)).is_err());
        let payload = SigningPayload::new(DomainSeparator::new("mesh.test.actor"), b"execution");
        let sign = |custody: &AppleActorCustody| {
            custody
                .with_signing(
                    |a, s| store.load(a, s),
                    |key| Signature::from_bytes(key.sign(payload.as_bytes()).to_bytes()),
                )
                .unwrap()
        };
        let signature = sign(&original);
        drop(original);
        for _ in 0..3 {
            let reopened =
                AppleActorCustody::open_using([1; 16], expected, |a, s| store.load(a, s)).unwrap();
            assert_eq!(sign(&reopened), signature);
            Ed25519::verify(&expected, payload.as_bytes(), &signature).unwrap();
            assert_eq!(reopened.backend().isolation_proof(), None);
        }
        assert!(AppleActorCustody::open_using([2; 16], expected, |a, s| store.load(a, s)).is_err());
        assert!(
            AppleActorCustody::open_using([1; 16], PublicKey::from_bytes([0; 32]), |a, s| store
                .load(a, s))
            .is_err()
        );
        assert_eq!(store.0.borrow().len(), 1);
    }
    #[test]
    fn actor_refuses_backend_loss_or_substitution_after_open() {
        let store = Store::default();
        let custody = AppleActorCustody::create_using([3; 16], |a, s| store.create(a, s)).unwrap();
        assert_eq!(
            custody.with_signing(
                |_, _| Err(CustodyError::Locked),
                |_| panic!("must not sign")
            ),
            Err(CustodyError::Locked)
        );
        store
            .0
            .borrow_mut()
            .insert(custody.account.clone(), [0; 32]);
        assert_eq!(
            custody.with_signing(|a, s| store.load(a, s), |_| panic!("must not sign")),
            Err(CustodyError::Refused)
        );
        store.0.borrow_mut().clear();
        assert_eq!(
            custody.with_signing(|a, s| store.load(a, s), |_| panic!("must not sign")),
            Err(CustodyError::NotFound)
        );
    }
}
