#import <Foundation/Foundation.h>
#import <LocalAuthentication/LocalAuthentication.h>
#import <Security/Security.h>

#include <stddef.h>
#include <stdint.h>
#include <string.h>

static const char *MESH_APPROVAL_TAG = "dev.mesh.desktop.approval.es256.v1";

enum {
    MESH_SE_OK = 0,
    MESH_SE_NOT_FOUND = 1,
    MESH_SE_CANCELLED = 2,
    MESH_SE_FAILURE = 3,
    MESH_SE_APPLICATION_IDENTITY_UNAVAILABLE = 4,
};

static NSData *mesh_tag(void) {
    return [NSData dataWithBytes:MESH_APPROVAL_TAG length:strlen(MESH_APPROVAL_TAG)];
}

static void mesh_copy_string(CFStringRef value, char *out, size_t capacity) {
    if (out == NULL || capacity == 0) return;
    out[0] = '\0';
    if (value == NULL || !CFStringGetCString(value, out, (CFIndex)capacity, kCFStringEncodingUTF8)) {
        strncpy(out, "Secure Enclave operation failed", capacity - 1);
        out[capacity - 1] = '\0';
    }
}

static void mesh_copy_status(OSStatus status, char *out, size_t capacity) {
    CFStringRef message = SecCopyErrorMessageString(status, NULL);
    mesh_copy_string(message, out, capacity);
    if (message != NULL) CFRelease(message);
}

static void mesh_copy_error(CFErrorRef error, char *out, size_t capacity) {
    CFStringRef message = error == NULL ? NULL : CFErrorCopyDescription(error);
    mesh_copy_string(message, out, capacity);
    if (message != NULL) CFRelease(message);
}

static bool mesh_is_string(CFTypeRef value) {
    return value != NULL && CFGetTypeID(value) == CFStringGetTypeID();
}

static bool mesh_is_dictionary(CFTypeRef value) {
    return value != NULL && CFGetTypeID(value) == CFDictionaryGetTypeID();
}

static bool mesh_is_array(CFTypeRef value) {
    return value != NULL && CFGetTypeID(value) == CFArrayGetTypeID();
}

static bool mesh_array_contains_string(CFArrayRef values, CFStringRef expected) {
    if (values == NULL || expected == NULL) return false;
    CFIndex count = CFArrayGetCount(values);
    for (CFIndex index = 0; index < count; index++) {
        CFTypeRef value = CFArrayGetValueAtIndex(values, index);
        if (mesh_is_string(value) && CFEqual(value, expected)) return true;
    }
    return false;
}

static bool mesh_identity_values_match(CFStringRef team, CFStringRef identifier,
                                       CFArrayRef certificates, CFDictionaryRef entitlements) {
    if (!mesh_is_string(team) || CFStringGetLength(team) == 0
        || !mesh_is_string(identifier) || !CFEqual(identifier, CFSTR("dev.mesh.desktop"))
        || !mesh_is_array(certificates) || CFArrayGetCount(certificates) == 0
        || !mesh_is_dictionary(entitlements)) return false;

    CFTypeRef entitlement_team = CFDictionaryGetValue(
        entitlements, CFSTR("com.apple.developer.team-identifier"));
    CFTypeRef application_identifier = CFDictionaryGetValue(
        entitlements, CFSTR("com.apple.application-identifier"));
    CFTypeRef keychain_groups = CFDictionaryGetValue(
        entitlements, CFSTR("keychain-access-groups"));
    if (!mesh_is_string(entitlement_team) || !CFEqual(entitlement_team, team)
        || !mesh_is_string(application_identifier) || !mesh_is_array(keychain_groups)) return false;

    CFMutableStringRef expected_application = CFStringCreateMutableCopy(
        kCFAllocatorDefault, 0, team);
    if (expected_application == NULL) return false;
    CFStringAppend(expected_application, CFSTR(".dev.mesh.desktop"));
    bool valid = CFEqual(application_identifier, expected_application)
        && mesh_array_contains_string((CFArrayRef)keychain_groups, expected_application);
    CFRelease(expected_application);
    return valid;
}

// Read-only application identity preflight. Looking up a missing credential can return
// errSecItemNotFound before Security.framework evaluates the app's signing identity, which used to
// make an ad-hoc build advertise a setup ceremony that was guaranteed to fail. Inspect the running
// code signature and its exact app-private keychain entitlements without creating or loading a key.
int mesh_secure_enclave_availability(char *error_out, size_t error_capacity) {
    @autoreleasepool {
        SecCodeRef code = NULL;
        OSStatus status = SecCodeCopySelf(kSecCSDefaultFlags, &code);
        if (status != errSecSuccess || code == NULL) {
            mesh_copy_string(CFSTR("validated Apple application identity is unavailable"),
                             error_out, error_capacity);
            if (code != NULL) CFRelease(code);
            return MESH_SE_APPLICATION_IDENTITY_UNAVAILABLE;
        }
        status = SecCodeCheckValidity(code, kSecCSStrictValidate, NULL);
        if (status != errSecSuccess) {
            mesh_copy_string(CFSTR("validated Apple application identity is unavailable"),
                             error_out, error_capacity);
            CFRelease(code);
            return MESH_SE_APPLICATION_IDENTITY_UNAVAILABLE;
        }

        CFDictionaryRef information = NULL;
        status = SecCodeCopySigningInformation(
            code, kSecCSSigningInformation | kSecCSRequirementInformation, &information);
        CFRelease(code);
        if (status != errSecSuccess || information == NULL) {
            mesh_copy_string(CFSTR("validated Apple application identity is unavailable"),
                             error_out, error_capacity);
            if (information != NULL) CFRelease(information);
            return MESH_SE_APPLICATION_IDENTITY_UNAVAILABLE;
        }

        CFTypeRef team = CFDictionaryGetValue(information, kSecCodeInfoTeamIdentifier);
        CFTypeRef identifier = CFDictionaryGetValue(information, kSecCodeInfoIdentifier);
        CFTypeRef certificates = CFDictionaryGetValue(information, kSecCodeInfoCertificates);
        CFTypeRef entitlements = CFDictionaryGetValue(information, kSecCodeInfoEntitlementsDict);
        bool valid = mesh_identity_values_match(
            (CFStringRef)team, (CFStringRef)identifier,
            (CFArrayRef)certificates, (CFDictionaryRef)entitlements);
        CFRelease(information);
        if (!valid) {
            mesh_copy_string(CFSTR("validated Apple application identity is unavailable"),
                             error_out, error_capacity);
            return MESH_SE_APPLICATION_IDENTITY_UNAVAILABLE;
        }
        return MESH_SE_OK;
    }
}

enum { MESH_ERROR_DOMAIN_LOCAL_AUTHENTICATION = 1, MESH_ERROR_DOMAIN_OS_STATUS = 2 };

static int mesh_error_result_for_domain(CFStringRef domain, CFIndex code) {
    if (domain != NULL && CFEqual(domain, (__bridge CFStringRef)LAErrorDomain)) {
        switch (code) {
            case LAErrorUserCancel:
            case LAErrorUserFallback:
            case LAErrorSystemCancel:
            case LAErrorAppCancel:
                return MESH_SE_CANCELLED;
            default:
                return MESH_SE_FAILURE;
        }
    }
    if (domain != NULL && CFEqual(domain, kCFErrorDomainOSStatus)) {
        if (code == errSecUserCanceled) return MESH_SE_CANCELLED;
        if (code == errSecMissingEntitlement) return MESH_SE_APPLICATION_IDENTITY_UNAVAILABLE;
    }
    return MESH_SE_FAILURE;
}

static int mesh_error_result(CFErrorRef error) {
    if (error == NULL) return MESH_SE_FAILURE;
    return mesh_error_result_for_domain(CFErrorGetDomain(error), CFErrorGetCode(error));
}

// Narrow native test seam: callers select one real Apple error domain and receive only the
// bridge's public result category. No credential or signing operation is performed.
int mesh_secure_enclave_classify_error_for_test(int domain_kind, long code) {
    @autoreleasepool {
        CFStringRef domain = NULL;
        if (domain_kind == MESH_ERROR_DOMAIN_LOCAL_AUTHENTICATION)
            domain = (__bridge CFStringRef)LAErrorDomain;
        else if (domain_kind == MESH_ERROR_DOMAIN_OS_STATUS)
            domain = kCFErrorDomainOSStatus;
        return mesh_error_result_for_domain(domain, (CFIndex)code);
    }
}

static SecKeyRef mesh_copy_private_key(OSStatus *status_out) {
    NSDictionary *query = @{
        (id)kSecClass: (id)kSecClassKey,
        (id)kSecAttrApplicationTag: mesh_tag(),
        (id)kSecAttrKeyType: (id)kSecAttrKeyTypeECSECPrimeRandom,
        (id)kSecAttrTokenID: (id)kSecAttrTokenIDSecureEnclave,
        (id)kSecUseDataProtectionKeychain: @YES,
        (id)kSecReturnRef: @YES,
    };
    CFTypeRef result = NULL;
    OSStatus status = SecItemCopyMatching((CFDictionaryRef)query, &result);
    if (status != errSecSuccess) {
        if (status_out != NULL) *status_out = status;
        return NULL;
    }

    SecKeyRef key = (SecKeyRef)result;
    CFDictionaryRef attributes = SecKeyCopyAttributes(key);
    CFTypeRef token = attributes == NULL ? NULL : CFDictionaryGetValue(attributes, kSecAttrTokenID);
    CFTypeRef key_class = attributes == NULL ? NULL : CFDictionaryGetValue(attributes, kSecAttrKeyClass);
    CFTypeRef key_size = attributes == NULL ? NULL : CFDictionaryGetValue(attributes, kSecAttrKeySizeInBits);
    CFTypeRef can_sign = attributes == NULL ? NULL : CFDictionaryGetValue(attributes, kSecAttrCanSign);
    bool valid = token != NULL && CFEqual(token, kSecAttrTokenIDSecureEnclave)
        && key_class != NULL && CFEqual(key_class, kSecAttrKeyClassPrivate)
        && key_size != NULL && CFEqual(key_size, (__bridge CFNumberRef)@256)
        && can_sign != NULL && CFEqual(can_sign, kCFBooleanTrue);
    if (attributes != NULL) CFRelease(attributes);
    if (!valid) {
        CFRelease(key);
        if (status_out != NULL) *status_out = errSecDecode;
        return NULL;
    }
    if (status_out != NULL) *status_out = errSecSuccess;
    return key;
}

static int mesh_export_public_key(SecKeyRef private_key, uint8_t *out, size_t capacity,
                                  char *error_out, size_t error_capacity) {
    if (out == NULL || capacity < 65) return MESH_SE_FAILURE;
    SecKeyRef public_key = SecKeyCopyPublicKey(private_key);
    if (public_key == NULL) {
        mesh_copy_string(CFSTR("Secure Enclave public key is unavailable"), error_out, error_capacity);
        return MESH_SE_FAILURE;
    }
    CFErrorRef error = NULL;
    CFDataRef bytes = SecKeyCopyExternalRepresentation(public_key, &error);
    CFRelease(public_key);
    if (bytes == NULL || CFDataGetLength(bytes) != 65) {
        mesh_copy_error(error, error_out, error_capacity);
        if (error != NULL) CFRelease(error);
        if (bytes != NULL) CFRelease(bytes);
        return MESH_SE_FAILURE;
    }
    memcpy(out, CFDataGetBytePtr(bytes), 65);
    CFRelease(bytes);
    if (error != NULL) CFRelease(error);
    return MESH_SE_OK;
}

int mesh_secure_enclave_load(uint8_t *public_key, size_t capacity,
                             char *error_out, size_t error_capacity) {
    @autoreleasepool {
        int available = mesh_secure_enclave_availability(error_out, error_capacity);
        if (available != MESH_SE_OK) return available;
        OSStatus status = errSecSuccess;
        SecKeyRef private_key = mesh_copy_private_key(&status);
        if (private_key == NULL) {
            if (status == errSecItemNotFound) return MESH_SE_NOT_FOUND;
            if (status == errSecMissingEntitlement)
                return MESH_SE_APPLICATION_IDENTITY_UNAVAILABLE;
            mesh_copy_status(status, error_out, error_capacity);
            return MESH_SE_FAILURE;
        }
        int result = mesh_export_public_key(private_key, public_key, capacity, error_out, error_capacity);
        CFRelease(private_key);
        return result;
    }
}

int mesh_secure_enclave_enroll(uint8_t *public_key, size_t capacity,
                               char *error_out, size_t error_capacity) {
    @autoreleasepool {
        int existing = mesh_secure_enclave_load(public_key, capacity, error_out, error_capacity);
        if (existing == MESH_SE_OK) return MESH_SE_OK;
        if (existing != MESH_SE_NOT_FOUND) return existing;

        CFErrorRef error = NULL;
        SecAccessControlRef access = SecAccessControlCreateWithFlags(
            kCFAllocatorDefault, kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly,
            kSecAccessControlPrivateKeyUsage | kSecAccessControlUserPresence, &error);
        if (access == NULL) {
            mesh_copy_error(error, error_out, error_capacity);
            if (error != NULL) CFRelease(error);
            return MESH_SE_FAILURE;
        }
        NSDictionary *attributes = @{
            (id)kSecAttrKeyType: (id)kSecAttrKeyTypeECSECPrimeRandom,
            (id)kSecAttrKeySizeInBits: @256,
            (id)kSecAttrTokenID: (id)kSecAttrTokenIDSecureEnclave,
            (id)kSecUseDataProtectionKeychain: @YES,
            (id)kSecPrivateKeyAttrs: @{
                (id)kSecAttrIsPermanent: @YES,
                (id)kSecAttrApplicationTag: mesh_tag(),
                (id)kSecAttrAccessControl: (__bridge id)access,
            },
        };
        SecKeyRef private_key = SecKeyCreateRandomKey((CFDictionaryRef)attributes, &error);
        CFRelease(access);
        if (private_key == NULL) {
            int result = mesh_error_result(error);
            mesh_copy_error(error, error_out, error_capacity);
            if (error != NULL) CFRelease(error);
            return result;
        }
        int result = mesh_export_public_key(private_key, public_key, capacity, error_out, error_capacity);
        CFRelease(private_key);
        if (error != NULL) CFRelease(error);
        return result;
    }
}

int mesh_secure_enclave_sign(const uint8_t *message, size_t message_length,
                             uint8_t *signature, size_t *signature_length,
                             char *error_out, size_t error_capacity) {
    @autoreleasepool {
        if (message == NULL || signature == NULL || signature_length == NULL || *signature_length < 72)
            return MESH_SE_FAILURE;
        int available = mesh_secure_enclave_availability(error_out, error_capacity);
        if (available != MESH_SE_OK) return available;
        OSStatus status = errSecSuccess;
        SecKeyRef private_key = mesh_copy_private_key(&status);
        if (private_key == NULL) {
            if (status == errSecItemNotFound) return MESH_SE_NOT_FOUND;
            if (status == errSecMissingEntitlement)
                return MESH_SE_APPLICATION_IDENTITY_UNAVAILABLE;
            mesh_copy_status(status, error_out, error_capacity);
            return MESH_SE_FAILURE;
        }
        if (!SecKeyIsAlgorithmSupported(private_key, kSecKeyOperationTypeSign,
                                        kSecKeyAlgorithmECDSASignatureMessageX962SHA256)) {
            CFRelease(private_key);
            mesh_copy_string(CFSTR("Secure Enclave does not support ES256 signing"), error_out, error_capacity);
            return MESH_SE_FAILURE;
        }
        CFDataRef body = CFDataCreate(kCFAllocatorDefault, message, (CFIndex)message_length);
        CFErrorRef error = NULL;
        CFDataRef signed_bytes = SecKeyCreateSignature(
            private_key, kSecKeyAlgorithmECDSASignatureMessageX962SHA256, body, &error);
        CFRelease(body);
        CFRelease(private_key);
        if (signed_bytes == NULL) {
            int result = mesh_error_result(error);
            mesh_copy_error(error, error_out, error_capacity);
            if (error != NULL) CFRelease(error);
            return result;
        }
        CFIndex length = CFDataGetLength(signed_bytes);
        if (length <= 0 || (size_t)length > *signature_length) {
            CFRelease(signed_bytes);
            if (error != NULL) CFRelease(error);
            return MESH_SE_FAILURE;
        }
        memcpy(signature, CFDataGetBytePtr(signed_bytes), (size_t)length);
        *signature_length = (size_t)length;
        CFRelease(signed_bytes);
        if (error != NULL) CFRelease(error);
        return MESH_SE_OK;
    }
}
