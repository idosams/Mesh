//! macOS Secure Enclave custody for the exact human-approval statement.

use core::fmt;

use mesh_approval::{
    HumanApprovalCredential, HumanApprovalReceipt, HumanApprovalReceiptDraft,
    HumanApprovalReceiptError,
};

const PUBLIC_KEY_BYTES: usize = 65;
#[cfg(target_os = "macos")]
const MAX_SIGNATURE_BYTES: usize = 72;
#[cfg(target_os = "macos")]
const ERROR_BYTES: usize = 256;

/// Public handle for the one app-scoped Secure Enclave approval key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecureEnclaveApprovalCredential {
    credential: HumanApprovalCredential,
}

impl SecureEnclaveApprovalCredential {
    /// Load the already-enrolled credential. This never creates a replacement key.
    pub fn load() -> Result<Self, SecureEnclaveApprovalError> {
        backend::load().and_then(Self::from_public_key)
    }

    /// Enrol the fixed app-scoped credential, or return the existing credential idempotently.
    pub fn enroll() -> Result<Self, SecureEnclaveApprovalError> {
        backend::enroll().and_then(Self::from_public_key)
    }

    fn from_public_key(
        public_key: [u8; PUBLIC_KEY_BYTES],
    ) -> Result<Self, SecureEnclaveApprovalError> {
        HumanApprovalCredential::from_public_key(public_key)
            .map(|credential| Self { credential })
            .map_err(SecureEnclaveApprovalError::Receipt)
    }

    /// The public credential identity the daemon may enrol as human approval trust.
    #[must_use]
    pub const fn credential(&self) -> &HumanApprovalCredential {
        &self.credential
    }

    /// Ask macOS for user presence and sign exactly one typed approval statement.
    ///
    /// There is intentionally no byte-slice signing method on this public type.
    pub fn approve(
        &self,
        draft: HumanApprovalReceiptDraft,
    ) -> Result<HumanApprovalReceipt, SecureEnclaveApprovalError> {
        let signature = backend::sign(&draft.canonical_bytes())?;
        draft
            .with_signature(signature)
            .map_err(SecureEnclaveApprovalError::Receipt)
    }
}

/// Draw a fresh challenge from the same OS entropy boundary used for actor-key generation.
pub fn fresh_approval_challenge() -> Result<[u8; 32], SecureEnclaveApprovalError> {
    let mut challenge = [0_u8; 32];
    crate::entropy::fill_from_os(&mut challenge)
        .map_err(|_| SecureEnclaveApprovalError::Unavailable)?;
    Ok(challenge)
}

/// Why the native credential ceremony did not produce a receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecureEnclaveApprovalError {
    /// This platform or device cannot provide this custody boundary.
    Unavailable,
    /// The user has not enrolled an approval credential yet.
    NotEnrolled,
    /// This build has no Apple-validated application identity for an app-private keychain group.
    ApplicationIdentityUnavailable,
    /// The person cancelled or macOS could not verify user presence.
    Cancelled,
    /// Security.framework refused the operation.
    Backend(String),
    /// The returned public key or signature did not satisfy the receipt contract.
    Receipt(HumanApprovalReceiptError),
}

impl fmt::Display for SecureEnclaveApprovalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("Secure Enclave approval is unavailable"),
            Self::NotEnrolled => {
                formatter.write_str("no Secure Enclave approval credential is enrolled")
            }
            Self::ApplicationIdentityUnavailable => formatter.write_str(
                "Secure Enclave approval requires a validated Apple application identity",
            ),
            Self::Cancelled => {
                formatter.write_str("approval was cancelled or user presence was not verified")
            }
            Self::Backend(message) => {
                write!(formatter, "Secure Enclave approval failed: {message}")
            }
            Self::Receipt(error) => {
                write!(
                    formatter,
                    "Secure Enclave returned an invalid approval: {error}"
                )
            }
        }
    }
}

impl std::error::Error for SecureEnclaveApprovalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Receipt(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(target_os = "macos")]
mod backend {
    use super::{SecureEnclaveApprovalError, ERROR_BYTES, MAX_SIGNATURE_BYTES, PUBLIC_KEY_BYTES};
    use std::ffi::{c_char, CStr};

    const OK: i32 = 0;
    const NOT_FOUND: i32 = 1;
    const CANCELLED: i32 = 2;
    const APPLICATION_IDENTITY_UNAVAILABLE: i32 = 4;

    // SAFETY: these declarations exactly match the fixed-width C ABI implemented in
    // secure_enclave.m. Every wrapper supplies live buffers for their declared lengths and copies
    // returned data before the call ends; the Objective-C implementation retains none.
    // MIRI: platform/mesh-keychain/tests/secure_enclave_ffi.rs
    #[allow(unsafe_code)]
    unsafe extern "C" {
        fn mesh_secure_enclave_load(
            public_key: *mut u8,
            capacity: usize,
            error_out: *mut c_char,
            error_capacity: usize,
        ) -> i32;
        fn mesh_secure_enclave_enroll(
            public_key: *mut u8,
            capacity: usize,
            error_out: *mut c_char,
            error_capacity: usize,
        ) -> i32;
        fn mesh_secure_enclave_sign(
            message: *const u8,
            message_length: usize,
            signature: *mut u8,
            signature_length: *mut usize,
            error_out: *mut c_char,
            error_capacity: usize,
        ) -> i32;
    }

    pub(super) fn load() -> Result<[u8; PUBLIC_KEY_BYTES], SecureEnclaveApprovalError> {
        call_key(mesh_secure_enclave_load)
    }

    pub(super) fn enroll() -> Result<[u8; PUBLIC_KEY_BYTES], SecureEnclaveApprovalError> {
        call_key(mesh_secure_enclave_enroll)
    }

    fn call_key(
        operation: unsafe extern "C" fn(*mut u8, usize, *mut c_char, usize) -> i32,
    ) -> Result<[u8; PUBLIC_KEY_BYTES], SecureEnclaveApprovalError> {
        let mut public_key = [0_u8; PUBLIC_KEY_BYTES];
        let mut error = [0_i8; ERROR_BYTES];
        // SAFETY: both arrays are live and writable for the exact capacities supplied. The bridge
        // retains no pointers and writes either one complete 65-byte public key or no key.
        // MIRI: platform/mesh-keychain/tests/secure_enclave_ffi.rs
        #[allow(unsafe_code)]
        let result = unsafe {
            operation(
                public_key.as_mut_ptr(),
                public_key.len(),
                error.as_mut_ptr(),
                error.len(),
            )
        };
        match result {
            OK => Ok(public_key),
            NOT_FOUND => Err(SecureEnclaveApprovalError::NotEnrolled),
            CANCELLED => Err(SecureEnclaveApprovalError::Cancelled),
            APPLICATION_IDENTITY_UNAVAILABLE => {
                Err(SecureEnclaveApprovalError::ApplicationIdentityUnavailable)
            }
            _ => Err(SecureEnclaveApprovalError::Backend(message(&error))),
        }
    }

    pub(super) fn sign(message_bytes: &[u8]) -> Result<Vec<u8>, SecureEnclaveApprovalError> {
        let mut signature = [0_u8; MAX_SIGNATURE_BYTES];
        let mut signature_length = signature.len();
        let mut error = [0_i8; ERROR_BYTES];
        // SAFETY: message is borrowed for this call; signature and error are live writable arrays;
        // signature_length starts at their true capacity; the bridge retains no pointer.
        // MIRI: platform/mesh-keychain/tests/secure_enclave_ffi.rs
        #[allow(unsafe_code)]
        let result = unsafe {
            mesh_secure_enclave_sign(
                message_bytes.as_ptr(),
                message_bytes.len(),
                signature.as_mut_ptr(),
                &mut signature_length,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        match result {
            OK if signature_length <= signature.len() => Ok(signature[..signature_length].to_vec()),
            NOT_FOUND => Err(SecureEnclaveApprovalError::NotEnrolled),
            CANCELLED => Err(SecureEnclaveApprovalError::Cancelled),
            APPLICATION_IDENTITY_UNAVAILABLE => {
                Err(SecureEnclaveApprovalError::ApplicationIdentityUnavailable)
            }
            _ => Err(SecureEnclaveApprovalError::Backend(message(&error))),
        }
    }

    fn message(buffer: &[i8; ERROR_BYTES]) -> String {
        // SAFETY: the Objective-C bridge always initializes the first byte and guarantees a NUL
        // terminator within the fixed buffer, even when conversion fails.
        // MIRI: platform/mesh-keychain/tests/secure_enclave_ffi.rs
        #[allow(unsafe_code)]
        let text = unsafe { CStr::from_ptr(buffer.as_ptr()) };
        text.to_string_lossy().into_owned()
    }
}

#[cfg(not(target_os = "macos"))]
mod backend {
    use super::{SecureEnclaveApprovalError, PUBLIC_KEY_BYTES};

    pub(super) fn load() -> Result<[u8; PUBLIC_KEY_BYTES], SecureEnclaveApprovalError> {
        Err(SecureEnclaveApprovalError::Unavailable)
    }

    pub(super) fn enroll() -> Result<[u8; PUBLIC_KEY_BYTES], SecureEnclaveApprovalError> {
        Err(SecureEnclaveApprovalError::Unavailable)
    }

    pub(super) fn sign(_message: &[u8]) -> Result<Vec<u8>, SecureEnclaveApprovalError> {
        Err(SecureEnclaveApprovalError::Unavailable)
    }
}
