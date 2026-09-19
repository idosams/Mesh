//! Safe-boundary checks for the macOS Secure Enclave adapter.
//!
//! This target is also the named Miri route for the Rust side of the FFI boundary. It does not
//! enrol or sign: those operations require a person and may never run unattended.

#![cfg(target_os = "macos")]

use mesh_keychain::{fresh_approval_challenge, SecureEnclaveApprovalCredential};

const NATIVE_BRIDGE: &str = include_str!("../src/secure_enclave.m");

const CANCELLED: i32 = 2;
const FAILURE: i32 = 3;
const APPLICATION_IDENTITY_UNAVAILABLE: i32 = 4;
const LOCAL_AUTHENTICATION: i32 = 1;
const OS_STATUS: i32 = 2;

// These stable values are copied from the LocalAuthentication and Security framework headers.
const LA_AUTHENTICATION_FAILED: i64 = -1;
const LA_USER_CANCEL: i64 = -2;
const LA_USER_FALLBACK: i64 = -3;
const LA_SYSTEM_CANCEL: i64 = -4;
const LA_PASSCODE_NOT_SET: i64 = -5;
const LA_BIOMETRY_LOCKOUT: i64 = -8;
const LA_APP_CANCEL: i64 = -9;
const LA_NOT_INTERACTIVE: i64 = -1004;
const ERR_SEC_USER_CANCELED: i64 = -128;
const ERR_SEC_AUTH_FAILED: i64 = -25293;
const ERR_SEC_INTERACTION_NOT_ALLOWED: i64 = -25308;
const ERR_SEC_MISSING_ENTITLEMENT: i64 = -34018;

#[allow(unsafe_code)]
unsafe extern "C" {
    fn mesh_secure_enclave_classify_error_for_test(domain_kind: i32, code: i64) -> i32;
}

fn classify(domain: i32, code: i64) -> i32 {
    // SAFETY: the bridge accepts two integers, performs no credential operation, and retains no
    // caller-owned memory. This test intentionally exercises the compiled Objective-C boundary.
    #[allow(unsafe_code)]
    unsafe {
        mesh_secure_enclave_classify_error_for_test(domain, code)
    }
}

#[test]
fn status_is_read_only_and_challenges_are_fresh() {
    assert_eq!(
        SecureEnclaveApprovalCredential::availability(),
        Err(mesh_keychain::SecureEnclaveApprovalError::ApplicationIdentityUnavailable),
        "the ad-hoc test executable must not advertise approval enrollment",
    );
    let first = SecureEnclaveApprovalCredential::load();
    let second = SecureEnclaveApprovalCredential::load();
    assert_eq!(first.is_ok(), second.is_ok());

    let first = fresh_approval_challenge().expect("OS random challenge");
    let second = fresh_approval_challenge().expect("OS random challenge");
    assert_ne!(first, [0; 32]);
    assert_ne!(first, second);
}

#[test]
fn only_explicit_framework_cancellation_results_are_cancelled() {
    for code in [
        LA_USER_CANCEL,
        LA_USER_FALLBACK,
        LA_SYSTEM_CANCEL,
        LA_APP_CANCEL,
    ] {
        assert_eq!(classify(LOCAL_AUTHENTICATION, code), CANCELLED);
    }
    for code in [
        LA_AUTHENTICATION_FAILED,
        LA_PASSCODE_NOT_SET,
        LA_BIOMETRY_LOCKOUT,
        LA_NOT_INTERACTIVE,
        ERR_SEC_USER_CANCELED,
    ] {
        assert_eq!(classify(LOCAL_AUTHENTICATION, code), FAILURE);
    }

    assert_eq!(classify(OS_STATUS, ERR_SEC_USER_CANCELED), CANCELLED);
    assert_eq!(
        classify(OS_STATUS, ERR_SEC_MISSING_ENTITLEMENT),
        APPLICATION_IDENTITY_UNAVAILABLE
    );
    for code in [
        ERR_SEC_AUTH_FAILED,
        ERR_SEC_INTERACTION_NOT_ALLOWED,
        LA_USER_CANCEL,
    ] {
        assert_eq!(classify(OS_STATUS, code), FAILURE);
    }

    assert_eq!(classify(99, LA_USER_CANCEL), FAILURE);
}

#[test]
fn credential_lookup_requires_the_data_protection_keychain_and_secure_enclave_token() {
    assert!(NATIVE_BRIDGE.contains("SecCodeCopySelf(kSecCSDefaultFlags"));
    assert!(NATIVE_BRIDGE.contains("SecCodeCheckValidity(code, kSecCSStrictValidate"));
    assert!(NATIVE_BRIDGE.contains("kSecCodeInfoTeamIdentifier"));
    assert!(NATIVE_BRIDGE.contains("kSecCodeInfoCertificates"));
    assert!(NATIVE_BRIDGE.contains("kSecCodeInfoEntitlementsDict"));
    assert!(NATIVE_BRIDGE.contains("com.apple.application-identifier"));
    assert!(NATIVE_BRIDGE.contains("com.apple.developer.team-identifier"));
    assert!(NATIVE_BRIDGE.contains("keychain-access-groups"));
    assert!(NATIVE_BRIDGE.contains(".dev.mesh.desktop"));
    assert!(NATIVE_BRIDGE.contains("(id)kSecAttrTokenID: (id)kSecAttrTokenIDSecureEnclave"));
    assert!(NATIVE_BRIDGE.contains("(id)kSecUseDataProtectionKeychain: @YES"));
    assert!(NATIVE_BRIDGE.contains("SecKeyCopyAttributes(key)"));
    assert!(NATIVE_BRIDGE.contains("CFEqual(token, kSecAttrTokenIDSecureEnclave)"));
    assert!(NATIVE_BRIDGE.contains("CFEqual(key_class, kSecAttrKeyClassPrivate)"));
    assert!(NATIVE_BRIDGE.contains("CFEqual(can_sign, kCFBooleanTrue)"));
}
