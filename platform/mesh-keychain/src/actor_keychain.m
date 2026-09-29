// Persistent execution identity only. This bridge never queries the P-256 approval credential.
#import <Foundation/Foundation.h>
#import <LocalAuthentication/LocalAuthentication.h>
#import <Security/Security.h>
#include <stdint.h>
#include <string.h>

extern int mesh_secure_enclave_availability(char *, size_t);
enum { ACTOR_OK = 0, ACTOR_MISSING = 1, ACTOR_LOCKED = 2, ACTOR_REFUSED = 3, ACTOR_UNAVAILABLE = 4 };
static NSString *const ACTOR_SERVICE = @"dev.mesh.desktop.worker.ed25519.v1";

int mesh_actor_keychain_available(void) {
    // This shared preflight inspects code-signing identity only, never an approval key.
    char diagnostic[256] = {0};
    return mesh_secure_enclave_availability(diagnostic, sizeof diagnostic) == 0
        ? ACTOR_OK : ACTOR_UNAVAILABLE;
}
static int actor_status(OSStatus status) {
    if (status == errSecSuccess) return ACTOR_OK;
    if (status == errSecItemNotFound) return ACTOR_MISSING;
    if (status == errSecInteractionNotAllowed || status == errSecAuthFailed) return ACTOR_LOCKED;
    if (status == errSecDuplicateItem) return ACTOR_REFUSED;
    return ACTOR_UNAVAILABLE;
}
static NSMutableDictionary *actor_query(const uint8_t *account, size_t length) {
    if (account == NULL || length != 32 || mesh_actor_keychain_available() != ACTOR_OK) return nil;
    for (size_t n = 0; n < length; n++) {
        if (!((account[n] >= '0' && account[n] <= '9') || (account[n] >= 'a' && account[n] <= 'f'))) return nil;
    }
    // The preflight validated the current code's exact team/application/group relationship.
    // Read that same immutable code identity to select the explicit app-private group, rather
    // than relying on whichever group the keychain would otherwise choose by default.
    SecCodeRef code = NULL;
    CFDictionaryRef information = NULL;
    if (SecCodeCopySelf(kSecCSDefaultFlags, &code) != errSecSuccess || code == NULL) return nil;
    OSStatus status = SecCodeCopySigningInformation(code, kSecCSSigningInformation, &information);
    CFRelease(code);
    if (status != errSecSuccess || information == NULL) {
        if (information != NULL) CFRelease(information);
        return nil;
    }
    CFTypeRef team = CFDictionaryGetValue(information, kSecCodeInfoTeamIdentifier);
    NSString *group = team != NULL && CFGetTypeID(team) == CFStringGetTypeID()
        ? [(__bridge NSString *)team stringByAppendingString:@".dev.mesh.desktop"] : nil;
    CFRelease(information);
    if (group == nil) return nil;
    NSString *name = [[NSString alloc] initWithBytes:account length:length encoding:NSASCIIStringEncoding];
    if (name == nil) return nil;
    LAContext *context = [[LAContext alloc] init];
    context.interactionNotAllowed = YES;
    return [@{
        (id)kSecClass: (id)kSecClassGenericPassword,
        (id)kSecAttrService: ACTOR_SERVICE,
        (id)kSecAttrAccount: name,
        (id)kSecAttrAccessGroup: group,
        (id)kSecUseDataProtectionKeychain: @YES,
        (id)kSecAttrSynchronizable: @NO,
        (id)kSecUseAuthenticationContext: context,
    } mutableCopy];
}
int mesh_actor_keychain_create(const uint8_t *account, size_t length, const uint8_t *value) {
    @autoreleasepool {
        if (value == NULL) return ACTOR_REFUSED;
        NSMutableDictionary *query = actor_query(account, length);
        if (query == nil) return ACTOR_UNAVAILABLE;
        query[(id)kSecAttrAccessible] = (id)kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly;
        // Rust retains the input for the synchronous call. Do not introduce a second explicitly
        // allocated seed copy here. Security.framework may make its own internal copies.
        query[(id)kSecValueData] = [NSData dataWithBytesNoCopy:(void *)value length:32 freeWhenDone:NO];
        return actor_status(SecItemAdd((__bridge CFDictionaryRef)query, NULL));
    }
}
static int actor_copy_value(NSDictionary *item, NSDictionary *query, uint8_t *value) {
    id data = item[(id)kSecValueData];
    if (![data isKindOfClass:[NSData class]] || [data length] != 32
        || ![item[(id)kSecAttrAccessible] isEqual:(id)kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly]
        || ![item[(id)kSecAttrSynchronizable] isEqual:@NO]
        || ![item[(id)kSecAttrService] isEqual:ACTOR_SERVICE]
        || ![item[(id)kSecAttrAccount] isEqual:query[(id)kSecAttrAccount]]
        || ![item[(id)kSecAttrAccessGroup] isEqual:query[(id)kSecAttrAccessGroup]]) return ACTOR_REFUSED;
    memcpy(value, [data bytes], 32);
    return ACTOR_OK;
}
int mesh_actor_keychain_load(const uint8_t *account, size_t length, uint8_t *value) {
    @autoreleasepool {
        if (value == NULL) return ACTOR_REFUSED;
        memset(value, 0, 32);
        NSMutableDictionary *query = actor_query(account, length);
        if (query == nil) return ACTOR_UNAVAILABLE;
        query[(id)kSecMatchLimit] = (id)kSecMatchLimitOne;
        query[(id)kSecReturnData] = @YES;
        query[(id)kSecReturnAttributes] = @YES;
        CFTypeRef result = NULL;
        OSStatus status = SecItemCopyMatching((__bridge CFDictionaryRef)query, &result);
        if (status != errSecSuccess) {
            if (result != NULL) CFRelease(result);
            return actor_status(status);
        }
        int outcome = ACTOR_REFUSED;
        if (result != NULL && CFGetTypeID(result) == CFDictionaryGetTypeID()) {
            outcome = actor_copy_value((__bridge NSDictionary *)result, query, value);
        }
        if (result != NULL) CFRelease(result);
        return outcome;
    }
}

// Deterministic native decoding seam. Fixed fixture bytes only; never queries any keychain.
int mesh_actor_keychain_shape_for_test(unsigned mode) {
    @autoreleasepool {
        uint8_t bytes[33]; memset(bytes, 0x42, sizeof bytes);
        NSDictionary *query = @{ (id)kSecAttrAccount: @"fixture", (id)kSecAttrAccessGroup: @"fixture.group" };
        NSMutableDictionary *item = [@{
            (id)kSecValueData: [NSData dataWithBytes:bytes length:32],
            (id)kSecAttrAccessible: (id)kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
            (id)kSecAttrSynchronizable: @NO,
            (id)kSecAttrService: ACTOR_SERVICE,
            (id)kSecAttrAccount: query[(id)kSecAttrAccount],
            (id)kSecAttrAccessGroup: query[(id)kSecAttrAccessGroup],
        } mutableCopy];
        switch (mode) {
            case 0: break;
            case 1: item[(id)kSecValueData] = [NSData dataWithBytes:bytes length:31]; break;
            case 2: item[(id)kSecValueData] = [NSData dataWithBytes:bytes length:33]; break;
            case 3: item[(id)kSecValueData] = @"not data"; break;
            case 4: item[(id)kSecAttrAccessible] = (id)kSecAttrAccessibleWhenUnlocked; break;
            case 5: item[(id)kSecAttrSynchronizable] = @YES; break;
            case 6: item[(id)kSecAttrService] = @"other"; break;
            case 7: item[(id)kSecAttrAccount] = @"other"; break;
            case 8: item[(id)kSecAttrAccessGroup] = @"other"; break;
            case 9: [item removeObjectForKey:(id)kSecAttrAccessible]; break;
            default: return ACTOR_REFUSED;
        }
        uint8_t output[32] = {0};
        int outcome = actor_copy_value(item, query, output);
        for (size_t n = 0; n < sizeof output; n++) {
            if (output[n] != (mode == 0 ? 0x42 : 0)) return ACTOR_UNAVAILABLE;
        }
        return outcome;
    }
}
