//! The oracle a signature backend has to pass before it is allowed to exist.
//!
//! # Why this ships before the implementation does
//!
//! The dangerous shape of an unfinished crypto crate is not the missing function. It is the
//! placeholder that returns `true`, or the trait implementation somebody adds under deadline that
//! checks the signature length and calls it a day. Both pass a compiler, both pass a smoke test,
//! and both silently remove the only property Mesh promises.
//!
//! So the *test* lands first. Whatever Ed25519 backend arrives — an audited crate, an FFI binding,
//! a hardware-backed verifier — has to pass [`check_ed25519_conformance`] before anything in the
//! programme may call it, and the vectors it is held to are already here and already checked
//! against implementations we did not write.
//!
//! # Where these vectors come from
//!
//! The four positive vectors are RFC 8032 §7.1. Ed25519 signing is deterministic, so a vector is
//! fully determined by its seed and its message, and the check that the seeds transcribed here are
//! the RFC's is that the public keys derived from them equal the RFC's published public keys. That
//! derivation and the signatures were produced by `python-cryptography` 50.0.0, and every vector
//! with a non-empty message was independently re-verified with OpenSSL 3.5.2 — a different
//! command, a different code path, and neither of them ours. The empty-message vector could not be
//! re-verified through the OpenSSL CLI, which refuses a zero-length `-rawin` input; it is carried
//! on the RFC public-key match and the one implementation.
//!
//! The six rejection vectors matter more than the positive ones, because they are the failures a
//! hand-written implementation actually has:
//!
//! * `non-canonical-scalar` — the same signature with `S + L` substituted for `S`. An implementation
//!   that omits the `S < L` check accepts it, and every signature in the system becomes malleable
//!   into a second valid encoding. Confirmed rejected by `python-cryptography`.
//! * `low-order-public-key` — an all-zero public key, the identity point.
//! * `tampered-r`, `tampered-s`, `tampered-message`, `wrong-public-key` — the ordinary substitutions.
//!
//! A backend that returns `Ok(())` for any of the six fails this harness. That is the whole point:
//! `the_harness_rejects_a_scheme_that_verifies_everything` in `tests/` proves the harness has
//! teeth by running it against a scheme that accepts unconditionally and asserting it fails.

use core::fmt;

use mesh_types::{PublicKey, Signature};

use crate::keys::{PUBLIC_KEY_BYTES, SIGNATURE_BYTES};
use crate::parts::CapabilityParts;
use crate::scheme::SignatureScheme;
use crate::token::{CapabilityCodec, CodecError};

/// One known-answer vector: hex public key, hex message, hex signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ed25519Vector {
    /// The vector's name, quoted in a failure.
    pub name: &'static str,
    /// The public key, 64 hex characters.
    pub public_key_hex: &'static str,
    /// The message, hex, possibly empty.
    pub message_hex: &'static str,
    /// The signature, 128 hex characters.
    pub signature_hex: &'static str,
}

/// RFC 8032 §7.1 vectors. Every one of these must verify.
pub const ED25519_VECTORS: [Ed25519Vector; 4] = [
    Ed25519Vector {
        name: "rfc8032-7.1-test-1-empty-message",
        public_key_hex: "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        message_hex: "",
        signature_hex: "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
    },
    Ed25519Vector {
        name: "rfc8032-7.1-test-2-one-byte",
        public_key_hex: "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        message_hex: "72",
        signature_hex: "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    },
    Ed25519Vector {
        name: "rfc8032-7.1-test-3-two-bytes",
        public_key_hex: "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
        message_hex: "af82",
        signature_hex: "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
    },
    Ed25519Vector {
        name: "rfc8032-7.1-test-sha-abc-64-bytes",
        public_key_hex: "ec172b93ad5e563bf4932c70e1245034c35467ef2efd4d64ebf819683467e2bf",
        message_hex: "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
        signature_hex: "dc2a4459e7369633a52b1bf277839a00201009a3efbf3ecb69bea2186c26b58909351fc9ac90b3ecfdfbc7c66431e0303dca179c138ac17ad9bef1177331a704",
    },
];

/// Vectors that must **not** verify. A backend that accepts any of these is broken in a way no
/// positive vector reveals.
pub const ED25519_REJECTION_VECTORS: [Ed25519Vector; 6] = [
    Ed25519Vector {
        // Test 2's signature with `S + L` substituted for `S`. Accepted by any implementation that
        // omits the `S < L` reduction check; every signature is then malleable.
        name: "non-canonical-scalar",
        public_key_hex: "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        message_hex: "72",
        signature_hex: "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69daf52db7415978abc61b2c2eb6aeebfca0387b2eaeb4302aeeb00d291612bb0c10",
    },
    Ed25519Vector {
        name: "low-order-public-key",
        public_key_hex: "0000000000000000000000000000000000000000000000000000000000000000",
        message_hex: "72",
        signature_hex: "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    },
    Ed25519Vector {
        name: "tampered-r",
        public_key_hex: "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        message_hex: "72",
        signature_hex: "93a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    },
    Ed25519Vector {
        name: "tampered-s",
        public_key_hex: "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        message_hex: "72",
        signature_hex: "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c01",
    },
    Ed25519Vector {
        name: "tampered-message",
        public_key_hex: "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        message_hex: "73",
        signature_hex: "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    },
    Ed25519Vector {
        name: "wrong-public-key",
        public_key_hex: "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        message_hex: "72",
        signature_hex: "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    },
];

/// Run every Ed25519 vector against `S`.
///
/// Note that the vectors are raw Ed25519 — the scheme is checked against the standard, not against
/// Mesh's framing. [`crate::SigningPayload`] is what makes a Mesh signature domain-separated, and
/// it sits above this trait, so the backend is held to RFC 8032 and nothing else.
///
/// # Errors
///
/// [`ConformanceFailure`] naming the first vector the backend got wrong.
pub fn check_ed25519_conformance<S: SignatureScheme>() -> Result<(), ConformanceFailure> {
    for vector in ED25519_VECTORS {
        let (key, message, signature) = decode(vector)?;
        S::verify(&key, &message, &signature).map_err(|error| {
            ConformanceFailure::ShouldVerify {
                vector: vector.name,
                reported: format!("{error}"),
            }
        })?;
    }
    for vector in ED25519_REJECTION_VECTORS {
        let (key, message, signature) = decode(vector)?;
        if S::verify(&key, &message, &signature).is_ok() {
            return Err(ConformanceFailure::ShouldReject {
                vector: vector.name,
            });
        }
    }
    Ok(())
}

/// Run a candidate [`CapabilityCodec`] against a corpus of capabilities.
///
/// The corpus is the caller's. A helper here that produced sample capabilities would need to mint
/// them, and this crate has no public way to mint a `Capability<HumanHeld>` on purpose — see
/// [`crate::CapabilityParts`]. Parts are inert, so a caller can build a corpus freely.
///
/// # Errors
///
/// [`ConformanceFailure`] naming the property the codec broke.
pub fn check_codec_conformance<C: CapabilityCodec>(
    corpus: &[CapabilityParts],
) -> Result<(), ConformanceFailure> {
    if corpus.is_empty() {
        return Err(ConformanceFailure::EmptyCorpus);
    }
    for parts in corpus {
        let bytes = C::encode(parts);
        if C::encode(parts) != bytes {
            return Err(ConformanceFailure::NotDeterministic);
        }
        match C::decode(&bytes) {
            Ok(decoded) if decoded == *parts => {}
            Ok(_) => return Err(ConformanceFailure::RoundTripChangedTheCapability),
            Err(error) => return Err(ConformanceFailure::RoundTripFailed { error }),
        }
        // Totality: no prefix of a valid encoding may panic. A decoder that indexes past its input
        // on a truncated payload is a denial of service reachable from any peer.
        for cut in 0..bytes.len() {
            let _ = C::decode(&bytes[..cut]);
        }
    }
    Ok(())
}

/// One vector, decoded into the three arguments a scheme's `verify` takes.
type DecodedVector = (PublicKey, Vec<u8>, Signature);

fn decode(vector: Ed25519Vector) -> Result<DecodedVector, ConformanceFailure> {
    let key: [u8; PUBLIC_KEY_BYTES] = unhex(vector.public_key_hex)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(ConformanceFailure::MalformedVector {
            vector: vector.name,
        })?;
    let signature: [u8; SIGNATURE_BYTES] = unhex(vector.signature_hex)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(ConformanceFailure::MalformedVector {
            vector: vector.name,
        })?;
    let key = PublicKey::from_bytes(key);
    let signature = Signature::from_bytes(signature);
    let message = unhex(vector.message_hex).ok_or(ConformanceFailure::MalformedVector {
        vector: vector.name,
    })?;
    Ok((key, message, signature))
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        out.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
    }
    Some(out)
}

const fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// How a candidate implementation failed conformance.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConformanceFailure {
    /// A vector that must verify did not.
    ShouldVerify {
        /// Which vector.
        vector: &'static str,
        /// What the backend said.
        reported: String,
    },
    /// A vector that must be rejected was accepted. The dangerous direction.
    ShouldReject {
        /// Which vector.
        vector: &'static str,
    },
    /// A vector in this file is not valid hex of the right length — a defect in the corpus itself.
    MalformedVector {
        /// Which vector.
        vector: &'static str,
    },
    /// The codec corpus was empty, so the check proved nothing.
    EmptyCorpus,
    /// The codec produced different bytes for the same capability twice.
    NotDeterministic,
    /// The codec decoded its own output into a different capability.
    RoundTripChangedTheCapability,
    /// The codec failed to decode its own output.
    RoundTripFailed {
        /// What it said.
        error: CodecError,
    },
}

impl fmt::Display for ConformanceFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShouldVerify { vector, reported } => write!(
                formatter,
                "vector `{vector}` must verify and the backend said: {reported}"
            ),
            Self::ShouldReject { vector } => write!(
                formatter,
                "vector `{vector}` must be rejected and the backend accepted it"
            ),
            Self::MalformedVector { vector } => {
                write!(formatter, "vector `{vector}` is not well formed")
            }
            Self::EmptyCorpus => formatter.write_str("a codec conformance corpus is never empty"),
            Self::NotDeterministic => {
                formatter.write_str("the codec encoded one capability two different ways")
            }
            Self::RoundTripChangedTheCapability => {
                formatter.write_str("the codec decoded its own output into a different capability")
            }
            Self::RoundTripFailed { error } => {
                write!(
                    formatter,
                    "the codec could not decode its own output: {error}"
                )
            }
        }
    }
}

impl std::error::Error for ConformanceFailure {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_vector_is_well_formed() {
        for vector in ED25519_VECTORS
            .iter()
            .chain(ED25519_REJECTION_VECTORS.iter())
        {
            decode(*vector).unwrap_or_else(|error| panic!("{error}"));
        }
    }

    /// The corpus is worthless if the rejection vectors are copies of the positive ones.
    #[test]
    fn the_rejection_vectors_differ_from_the_vector_they_were_derived_from() {
        let source = ED25519_VECTORS[1];
        for vector in ED25519_REJECTION_VECTORS {
            assert!(
                vector.public_key_hex != source.public_key_hex
                    || vector.message_hex != source.message_hex
                    || vector.signature_hex != source.signature_hex,
                "rejection vector `{}` is identical to a vector that must verify",
                vector.name
            );
        }
    }

    #[test]
    fn hex_decoding_is_total() {
        for text in ["", "0", "zz", "0g", &"0".repeat(129)] {
            let _ = unhex(text);
        }
    }
}
