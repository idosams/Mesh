//! Test doubles for the two seams `mesh-crypto` deliberately ships empty.
//!
//! **These live under `tests/` and are never compiled into the library.** That is the point: an
//! in-crate software signer or key store would be the most attractive thing in the programme to
//! attack, and the moment one exists somebody wires it into a default.
//!
//! [`PlumbingScheme`] is **not** a signature scheme. It is a keyed mixing function with none of
//! Ed25519's properties, and `conformance_has_teeth.rs` asserts that the RFC 8032 harness rejects
//! it — so this file cannot quietly become the implementation.

#![allow(dead_code)]

use mesh_crypto::{
    ActorKey, CapabilityCodec, CapabilityParts, CodecError, CustodyBackend, CustodyError,
    DelegationBudget, Expiry, ForActor, HumanKeyCustody, KeyCustody, KeyPair, PolicyEpoch,
    SignatureScheme, SigningPayload, VerifyError, WorkspaceScope, PUBLIC_KEY_BYTES,
    SIGNATURE_BYTES,
};
use mesh_types::{PublicKey, Signature};

/// A deterministic keyed mixing function standing in for a signature scheme, so the token, custody
/// and capability plumbing can be exercised without one.
///
/// It is not secure and does not pretend to be: no curve, no scalar, no hash with any security
/// claim. It is key-dependent and message-dependent, which is exactly enough to make a tampering
/// test mean something and not one bit more.
pub struct PlumbingScheme;

impl PlumbingScheme {
    /// The "signature" over `message` under the key material `key`.
    pub fn mac(key: &PublicKey, message: &[u8]) -> Signature {
        let key = key.as_bytes();
        let mut out = [0u8; SIGNATURE_BYTES];
        for (lane, slot) in out.iter_mut().enumerate() {
            // FNV-1a over the key, the message and the lane index.
            let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
            let mut mix = |byte: u8| {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            };
            mix(lane as u8);
            for byte in key {
                mix(*byte);
            }
            for byte in message {
                mix(*byte);
            }
            *slot = (hash >> 24) as u8;
        }
        Signature::from_bytes(out)
    }
}

impl SignatureScheme for PlumbingScheme {
    const NAME: &'static str = "test-plumbing-not-a-signature-scheme";

    fn verify(
        public_key: &PublicKey,
        message: &[u8],
        signature: &Signature,
    ) -> Result<(), VerifyError> {
        if Self::mac(public_key, message) == *signature {
            Ok(())
        } else {
            Err(VerifyError::Mismatch)
        }
    }
}

/// The failure mode this crate exists to make impossible: a backend that verifies everything.
///
/// Used only to prove [`mesh_crypto::conformance::check_ed25519_conformance`] catches it.
pub struct AlwaysAcceptScheme;

impl SignatureScheme for AlwaysAcceptScheme {
    const NAME: &'static str = "always-accept-do-not-ship";

    fn verify(
        _public_key: &PublicKey,
        _message: &[u8],
        _signature: &Signature,
    ) -> Result<(), VerifyError> {
        Ok(())
    }
}

/// A length-prefixed encoder over [`CapabilityParts`], standing in for `mesh-cbor/0`.
pub struct PlumbingCodec;

impl PlumbingCodec {
    fn put(out: &mut Vec<u8>, bytes: &[u8]) {
        out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        out.extend_from_slice(bytes);
    }

    fn take<'a>(input: &mut &'a [u8]) -> Result<&'a [u8], CodecError> {
        if input.len() < 4 {
            return Err(CodecError::Malformed);
        }
        let len = u32::from_be_bytes([input[0], input[1], input[2], input[3]]) as usize;
        if input.len() < 4 + len {
            return Err(CodecError::Malformed);
        }
        let (field, rest) = input[4..].split_at(len);
        *input = rest;
        Ok(field)
    }

    fn take_u64(input: &mut &[u8]) -> Result<u64, CodecError> {
        let field = Self::take(input)?;
        let bytes: [u8; 8] = field.try_into().map_err(|_| CodecError::Malformed)?;
        Ok(u64::from_be_bytes(bytes))
    }

    fn take_key(input: &mut &[u8]) -> Result<ActorKey, CodecError> {
        let field = Self::take(input)?;
        let bytes: [u8; PUBLIC_KEY_BYTES] = field.try_into().map_err(|_| CodecError::Malformed)?;
        Ok(ActorKey::from_public_bytes(bytes))
    }
}

impl CapabilityCodec for PlumbingCodec {
    const CODEC: &'static str = "test-plumbing/0";

    fn encode(parts: &CapabilityParts) -> Vec<u8> {
        let mut out = Vec::new();
        Self::put(&mut out, parts.issuer().as_bytes());
        Self::put(&mut out, parts.subject().as_bytes());
        Self::put(&mut out, parts.workspace().as_bytes());
        Self::put(&mut out, parts.tier().as_bytes());
        Self::put(&mut out, &parts.policy_epoch().value().to_be_bytes());
        Self::put(&mut out, &parts.not_after().as_unix_millis().to_be_bytes());
        Self::put(&mut out, &[parts.budget().remaining()]);
        out.extend_from_slice(&(parts.actions().len() as u32).to_be_bytes());
        for action in parts.actions() {
            Self::put(&mut out, action.as_bytes());
        }
        out
    }

    fn decode(bytes: &[u8]) -> Result<CapabilityParts, CodecError> {
        let mut cursor = bytes;
        let issuer = Self::take_key(&mut cursor)?;
        let subject = Self::take_key(&mut cursor)?;
        let workspace: [u8; 16] = Self::take(&mut cursor)?
            .try_into()
            .map_err(|_| CodecError::Malformed)?;
        let tier = core::str::from_utf8(Self::take(&mut cursor)?)
            .map_err(|_| CodecError::Malformed)?
            .to_owned();
        let epoch = Self::take_u64(&mut cursor)?;
        let not_after = Self::take_u64(&mut cursor)?;
        let budget = *Self::take(&mut cursor)?
            .first()
            .ok_or(CodecError::Malformed)?;
        if cursor.len() < 4 {
            return Err(CodecError::Malformed);
        }
        let count = u32::from_be_bytes([cursor[0], cursor[1], cursor[2], cursor[3]]) as usize;
        cursor = &cursor[4..];
        if count > 64 {
            return Err(CodecError::Malformed);
        }
        let mut actions = Vec::with_capacity(count);
        for _ in 0..count {
            actions.push(
                core::str::from_utf8(Self::take(&mut cursor)?)
                    .map_err(|_| CodecError::Malformed)?
                    .to_owned(),
            );
        }
        if !cursor.is_empty() {
            return Err(CodecError::Malformed);
        }
        Ok(CapabilityParts::new(
            issuer,
            subject,
            WorkspaceScope::from_bytes(workspace),
            tier,
            actions,
            PolicyEpoch::new(epoch),
            Expiry::at_unix_millis(not_after),
            DelegationBudget::new(budget),
        ))
    }
}

/// A key holder for tests. Signs with [`PlumbingScheme`], using the public bytes as the key
/// material, because there is no secret in a test double either.
pub struct TestCustody {
    key: ActorKey,
}

impl TestCustody {
    /// A holder of the key named by `byte` repeated.
    #[must_use]
    pub fn new(byte: u8) -> Self {
        Self {
            key: ActorKey::from_public_bytes([byte; PUBLIC_KEY_BYTES]),
        }
    }
}

impl KeyCustody<ForActor> for TestCustody {
    fn backend(&self) -> CustodyBackend {
        CustodyBackend::AppleSecureEnclave
    }

    fn public_key(&self) -> KeyPair<ForActor> {
        self.key
    }

    fn sign(&self, payload: &SigningPayload) -> Result<Signature, CustodyError> {
        Ok(PlumbingScheme::mac(
            &self.key.public_key(),
            payload.as_bytes(),
        ))
    }
}

impl HumanKeyCustody for TestCustody {}

/// A holder of a **real** Ed25519 secret, for tests only.
///
/// # Why this is here and not in `src/`
///
/// Signing needs a secret half, and `mesh-crypto` deliberately holds none: `KeyCustody` is the
/// whole interface to a secret, it signs and never exports, and every implementation of it is an
/// operating-system or hardware backend in another crate. An in-process software signer in the
/// library would be the single most attractive thing in the programme to attack, and the moment one
/// exists somebody wires it into a default.
///
/// This is a `tests/` module, and `tests/` compiles as a separate crate that ships in no binary and
/// that no production code path can name. **The precise claim is about `mesh-crypto`'s own source,
/// not about the dependency**: `ed25519-dalek` is a normal dependency as well as a dev-dependency —
/// verification needs it — so its signing code is compiled either way, and saying otherwise would be
/// a security claim that is not true. What is true is that nothing under `crates/mesh-crypto/src/`
/// names `SigningKey`, `Signer` or any secret at all, and
/// `mesh_crypto::no_secret_material` fails the **build** if that changes. A secret cannot enter the
/// library because the library has no word for one.
///
/// What this buys is the one thing a verify-only crate cannot otherwise demonstrate: that a
/// signature made by a real Ed25519 implementation is accepted by [`mesh_crypto::Ed25519`], and
/// that a tampered one is not.
pub struct RealSigner {
    signing: ed25519_dalek::SigningKey,
}

impl RealSigner {
    /// A signer from a 32-byte RFC 8032 seed.
    #[must_use]
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        Self {
            signing: ed25519_dalek::SigningKey::from_bytes(seed),
        }
    }

    /// A signer from `byte` repeated, when the test does not care which key it is.
    #[must_use]
    pub fn from_filler(byte: u8) -> Self {
        Self::from_seed(&[byte; 32])
    }

    /// The public half, as an `actor key`.
    #[must_use]
    pub fn actor_key(&self) -> ActorKey {
        ActorKey::from_public_key(self.public_key())
    }

    /// The public half.
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        PublicKey::from_bytes(self.signing.verifying_key().to_bytes())
    }

    /// A real Ed25519 signature over `message`.
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> Signature {
        use ed25519_dalek::Signer;
        Signature::from_bytes(self.signing.sign(message).to_bytes())
    }

    /// A real Ed25519 signature over a framed [`SigningPayload`].
    #[must_use]
    pub fn sign_payload(&self, payload: &SigningPayload) -> Signature {
        self.sign(payload.as_bytes())
    }
}

/// A workspace for tests.
#[must_use]
pub fn workspace() -> WorkspaceScope {
    WorkspaceScope::from_bytes([0x5a; 16])
}
