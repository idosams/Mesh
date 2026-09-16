//! Capability tokens: the wire envelope, and the type that makes an unverified capability
//! unrepresentable.
//!
//! # The shape
//!
//! ```text
//! MCT0 ‖ version ‖ signature(64) ‖ payload_len(u32 be) ‖ payload
//! ```
//!
//! Four magic bytes, a version byte, the signature, a length and the payload. The **payload** is
//! the capability's canonical `mesh-cbor/0` encoding. Product callers use
//! [`CapabilityToken::canonical_payload`] and [`CapabilityToken::verify_canonical`]; the generic
//! [`CapabilityCodec`] seam remains available for conformance and negative testing. A second
//! product encoding of the same record is exactly the divergence a signature turns into a silent
//! verification failure.
//!
//! Verification is bound to *both* ends of the grant: the caller names the issuer it trusts and the
//! peer it authenticated, and the token has to agree with both. Binding only the issuer would leave
//! a stolen token verifying perfectly for whoever stole it.
//!
//! Nothing that could be substituted lives in the envelope. There is no issuer field, no tier byte,
//! no expiry: those are inside the signed payload, and the only copy. An unsigned field beside a
//! signed one is a field an attacker can set, and a reader can read the wrong one.
//!
//! # There is no way to read a capability out of an unverified token
//!
//! [`CapabilityToken`] holds bytes. Its only method that produces a [`Capability`] is
//! [`CapabilityToken::verify`], which needs a [`SignatureScheme`], the issuer's key, the current
//! time and the current policy epoch, and returns `Result`. There is no `capability()`, no
//! `Deref`, no `into_inner`. A caller cannot accidentally act on an unchecked token because the
//! value it would act on does not exist until the check has passed.
//!
//! This is why the seam matters more than it looks: with no [`SignatureScheme`] in the workspace,
//! `verify` cannot be *called*. The failure mode of "no backend yet" is a compile error at the call
//! site, not a token that verifies.

use core::fmt;

use mesh_types::{CborReader, CborWriter, PolicyEpoch, Signature};

use crate::capability::{AuthorityTier, Capability, CapabilityError};
use crate::domain::{DomainSeparator, SigningPayload};
use crate::keys::{ActorKey, SIGNATURE_BYTES};
use crate::parts::{CapabilityParts, PartsError};
use crate::scheme::{SignatureScheme, VerifyError};

/// The envelope's magic bytes.
pub const TOKEN_MAGIC: [u8; 4] = *b"MCT0";

/// The envelope version this crate reads and writes.
pub const TOKEN_VERSION: u8 = 0;

/// The largest payload the envelope accepts, so a hostile length cannot ask for an allocation.
///
/// A capability is a handful of keys, a small action set and four scalars. Sixty-four kilobytes is
/// three orders of magnitude of headroom and still a bound.
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

/// The fixed part of the envelope: magic, version, signature and length.
const HEADER_BYTES: usize = 4 + 1 + SIGNATURE_BYTES + 4;

/// Encodes a capability into the bytes a signature covers, and decodes them back.
///
/// The conformance seam for capability payload encodings. Production code uses the crate's private
/// canonical implementation through [`CapabilityToken::canonical_payload`] and
/// [`CapabilityToken::verify_canonical`]; alternate implementations exist only to exercise the
/// generic verification and conformance boundary.
///
/// # What an implementation must guarantee
///
/// [`crate::conformance::check_codec_conformance`] checks these against a candidate:
///
/// 1. **Round trip.** `decode(encode(p)) == p` for every parts value.
/// 2. **Determinism.** `encode` of the same parts is the same bytes, every time, everywhere.
/// 3. **Tier binding.** The tier name is encoded, so a human payload cannot be read as a delegated
///    one. [`Capability::from_parts`] is what enforces it, and it is not reachable except through
///    [`CapabilityToken::verify`].
/// 4. **Totality.** `decode` never panics, for any input of any length.
///
/// # Why this speaks [`CapabilityParts`] and not [`Capability`]
///
/// Because `decode` returning a `Capability` would be a public constructor from bytes, and a public
/// constructor from bytes is a public way to mint a human capability that grants publication
/// authority. The codec handles inert field values; only a verified signature turns them into
/// authority. See the [`crate::parts`](CapabilityParts) documentation.
pub trait CapabilityCodec {
    /// The codec's wire name, recorded with the signature's provenance.
    const CODEC: &'static str;

    /// Encode a capability's field values into the bytes a signature covers.
    fn encode(parts: &CapabilityParts) -> Vec<u8>;

    /// Decode field values.
    ///
    /// # Errors
    ///
    /// [`CodecError`] for every malformed input.
    fn decode(bytes: &[u8]) -> Result<CapabilityParts, CodecError>;
}

/// The sole production payload spelling behind [`CapabilityToken`]'s canonical methods.
///
/// Private on purpose: callers choose the canonical public methods rather than naming an encoding
/// implementation. The fixed array is:
///
/// ```text
/// [version, issuer, subject, workspace, tier, actions, policy_epoch, not_after, budget]
/// ```
///
/// Keys and workspace are byte strings of exactly 32, 32 and 16 bytes. Actions are a strictly
/// increasing text array. Every integer and container uses the one `mesh-cbor/0` spelling enforced
/// by [`CborReader`].
struct CanonicalCapabilityCodec;

const CAPABILITY_PAYLOAD_VERSION: u64 = 0;
const CAPABILITY_PAYLOAD_FIELDS: u64 = 9;
const MAX_CAPABILITY_ACTIONS: u64 = 64;

impl CapabilityCodec for CanonicalCapabilityCodec {
    const CODEC: &'static str = "mesh-cbor/0;capability/0";

    fn encode(parts: &CapabilityParts) -> Vec<u8> {
        let mut writer = CborWriter::new();
        writer
            .array(CAPABILITY_PAYLOAD_FIELDS)
            .unsigned(CAPABILITY_PAYLOAD_VERSION)
            .bytes(parts.issuer().as_bytes())
            .bytes(parts.subject().as_bytes())
            .bytes(parts.workspace().as_bytes())
            .text(parts.tier())
            .array(parts.actions().len() as u64);
        for action in parts.actions() {
            writer.text(action);
        }
        writer
            .unsigned(parts.policy_epoch().value())
            .unsigned(parts.not_after().as_unix_millis())
            .unsigned(u64::from(parts.budget().remaining()));
        writer.finish()
    }

    fn decode(bytes: &[u8]) -> Result<CapabilityParts, CodecError> {
        let mut reader = CborReader::new(bytes);
        if reader.array().map_err(|_| CodecError::Malformed)? != CAPABILITY_PAYLOAD_FIELDS {
            return Err(CodecError::Malformed);
        }
        if reader.unsigned().map_err(|_| CodecError::Malformed)? != CAPABILITY_PAYLOAD_VERSION {
            return Err(CodecError::UnsupportedVersion);
        }
        let issuer = ActorKey::from_public_bytes(exact_bytes(
            reader.bytes().map_err(|_| CodecError::Malformed)?,
        )?);
        let subject = ActorKey::from_public_bytes(exact_bytes(
            reader.bytes().map_err(|_| CodecError::Malformed)?,
        )?);
        let workspace = crate::WorkspaceScope::from_bytes(exact_bytes(
            reader.bytes().map_err(|_| CodecError::Malformed)?,
        )?);
        let tier = reader.text().map_err(|_| CodecError::Malformed)?.to_owned();
        let action_count = reader.array().map_err(|_| CodecError::Malformed)?;
        if action_count > MAX_CAPABILITY_ACTIONS {
            return Err(CodecError::Malformed);
        }
        let mut actions = Vec::with_capacity(action_count as usize);
        for _ in 0..action_count {
            let action = reader.text().map_err(|_| CodecError::Malformed)?.to_owned();
            if actions.last().is_some_and(|previous| previous >= &action) {
                return Err(CodecError::Malformed);
            }
            actions.push(action);
        }
        let policy_epoch = PolicyEpoch::new(reader.unsigned().map_err(|_| CodecError::Malformed)?);
        let not_after =
            crate::Expiry::at_unix_millis(reader.unsigned().map_err(|_| CodecError::Malformed)?);
        let budget = u8::try_from(reader.unsigned().map_err(|_| CodecError::Malformed)?)
            .map_err(|_| CodecError::Malformed)?;
        if !reader.is_exhausted() {
            return Err(CodecError::Malformed);
        }
        Ok(CapabilityParts::new(
            issuer,
            subject,
            workspace,
            tier,
            actions,
            policy_epoch,
            not_after,
            crate::DelegationBudget::new(budget),
        ))
    }
}

fn exact_bytes<const N: usize>(bytes: &[u8]) -> Result<[u8; N], CodecError> {
    bytes.try_into().map_err(|_| CodecError::Malformed)
}

/// Why a payload did not decode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodecError {
    /// The bytes are not a capability encoding.
    Malformed,
    /// The bytes encode a capability at a different tier than the one asked for.
    WrongTier,
    /// The bytes encode a version this codec does not read.
    UnsupportedVersion,
}

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Malformed => "the payload is not a capability encoding",
            Self::WrongTier => "the payload encodes a different authority tier",
            Self::UnsupportedVersion => "the payload encodes an unsupported version",
        })
    }
}

impl std::error::Error for CodecError {}

/// The signed, verifiable encoding of a capability that a peer can check without contacting its
/// issuer.
///
/// Holds bytes and a signature. It does **not** hold a [`Capability`] — see the module docs.
#[derive(Clone, PartialEq, Eq)]
pub struct CapabilityToken {
    signature: Signature,
    payload: Vec<u8>,
}

impl CapabilityToken {
    /// Assemble a token from a signature and the payload it covers.
    ///
    /// Nothing is checked here, and nothing should be: this is the constructor an issuer uses after
    /// [`CapabilityToken::payload_to_sign`] and its custody produced the signature. The check is
    /// [`CapabilityToken::verify`], on the reading side, where the trust decision is.
    ///
    /// # Errors
    ///
    /// [`TokenError::PayloadTooLarge`] when the payload exceeds [`MAX_PAYLOAD_BYTES`].
    pub fn new(signature: Signature, payload: Vec<u8>) -> Result<Self, TokenError> {
        if payload.len() > MAX_PAYLOAD_BYTES {
            return Err(TokenError::PayloadTooLarge {
                found: payload.len(),
            });
        }
        Ok(Self { signature, payload })
    }

    /// The exact bytes an issuer signs, framed in the capability-token domain.
    #[must_use]
    pub fn payload_to_sign(payload: &[u8]) -> SigningPayload {
        SigningPayload::new(DomainSeparator::CAPABILITY_TOKEN, payload)
    }

    /// Encode inert capability fields with the sole production `mesh-cbor/0` payload schema.
    ///
    /// An issuer signs [`CapabilityToken::payload_to_sign`] over these bytes, then assembles the
    /// envelope with [`CapabilityToken::new`]. Keeping the codec marker private prevents a caller
    /// from accidentally selecting a test or research encoding on the product path.
    #[must_use]
    pub fn canonical_payload(parts: &CapabilityParts) -> Vec<u8> {
        CanonicalCapabilityCodec::encode(parts)
    }

    /// The wire form.
    #[must_use]
    pub fn to_wire(&self) -> Vec<u8> {
        let mut wire = Vec::with_capacity(HEADER_BYTES + self.payload.len());
        wire.extend_from_slice(&TOKEN_MAGIC);
        wire.push(TOKEN_VERSION);
        wire.extend_from_slice(self.signature.as_bytes());
        wire.extend_from_slice(&(self.payload.len() as u32).to_be_bytes());
        wire.extend_from_slice(&self.payload);
        wire
    }

    /// Parse the wire form.
    ///
    /// Total: no input of any length or content makes this panic, and every rejection is explicit.
    /// Trailing bytes after a complete token are a rejection, not something to ignore — a decoder
    /// that skips them lets two readers disagree about what a message contained.
    ///
    /// # Errors
    ///
    /// [`TokenError`] for every malformed input.
    pub fn from_wire(bytes: &[u8]) -> Result<Self, TokenError> {
        if bytes.len() < HEADER_BYTES {
            return Err(TokenError::Truncated {
                need: HEADER_BYTES,
                found: bytes.len(),
            });
        }
        if bytes[..4] != TOKEN_MAGIC {
            return Err(TokenError::BadMagic);
        }
        if bytes[4] != TOKEN_VERSION {
            return Err(TokenError::UnsupportedVersion { found: bytes[4] });
        }
        let mut signature = [0u8; SIGNATURE_BYTES];
        signature.copy_from_slice(&bytes[5..5 + SIGNATURE_BYTES]);
        let signature = Signature::from_bytes(signature);

        let length_at = 5 + SIGNATURE_BYTES;
        let declared = u32::from_be_bytes([
            bytes[length_at],
            bytes[length_at + 1],
            bytes[length_at + 2],
            bytes[length_at + 3],
        ]) as usize;
        if declared > MAX_PAYLOAD_BYTES {
            return Err(TokenError::PayloadTooLarge { found: declared });
        }
        let body = &bytes[HEADER_BYTES..];
        if body.len() < declared {
            return Err(TokenError::Truncated {
                need: HEADER_BYTES + declared,
                found: bytes.len(),
            });
        }
        if body.len() > declared {
            return Err(TokenError::TrailingBytes {
                extra: body.len() - declared,
            });
        }
        Ok(Self {
            signature,
            payload: body.to_vec(),
        })
    }

    /// Check the signature, decode the payload, and confirm the capability is usable now.
    ///
    /// Every check is here, in this order, and each one fails closed:
    ///
    /// 1. the signature over the **framed** payload, under `expected_issuer`;
    /// 2. the payload decodes, and its field values resolve at tier `T` — a tier mismatch or an
    ///    action outside `T`'s vocabulary is a rejection, and `advance-canonical-head` is outside
    ///    the delegated vocabulary by construction;
    /// 3. the decoded issuer is `expected_issuer`, so a token signed by a key the caller trusts
    ///    cannot claim to have been issued by another;
    /// 4. the decoded subject is `presented_by` — see below;
    /// 5. the policy epoch is the one in force, so a revoked capability is dead;
    /// 6. the expiry has not passed at `now`.
    ///
    /// # Why the subject is an argument and not something the caller reads off afterwards
    ///
    /// A capability token is bearer evidence: the bytes are not secret, and anything that can read
    /// them can present them. Nothing in the bytes says *who is presenting*, so a token stolen from
    /// one agent verifies perfectly when replayed by another. The only defence is for the caller —
    /// which is the side that authenticated the peer — to say who it believes is presenting, and
    /// this crate cannot know that.
    ///
    /// It is therefore a required argument rather than a getter. A getter makes the check optional,
    /// and an optional check on the one bound that stops token theft is a check some call site will
    /// not make.
    ///
    /// # Errors
    ///
    /// [`TokenError`] naming which check failed.
    pub fn verify<S: SignatureScheme, C: CapabilityCodec, T: AuthorityTier>(
        &self,
        expected_issuer: &ActorKey,
        presented_by: &ActorKey,
        now: u64,
        epoch: PolicyEpoch,
    ) -> Result<Capability<T>, TokenError> {
        let payload = Self::payload_to_sign(&self.payload);
        S::verify(
            &expected_issuer.public_key(),
            payload.as_bytes(),
            &self.signature,
        )
        .map_err(TokenError::Signature)?;

        let parts = C::decode(&self.payload).map_err(TokenError::Codec)?;
        let capability = Capability::<T>::from_parts(&parts).map_err(TokenError::Parts)?;

        if capability.issuer() != expected_issuer {
            return Err(TokenError::IssuerMismatch);
        }
        if capability.subject() != presented_by {
            return Err(TokenError::SubjectMismatch);
        }
        capability
            .check_current(now, epoch)
            .map_err(TokenError::NotCurrent)?;
        Ok(capability)
    }

    /// Verify this token using the sole production `mesh-cbor/0` payload schema.
    ///
    /// This is the product entry point. [`CapabilityToken::verify`] remains generic so the
    /// conformance suite can prove a candidate codec fails, but product callers do not name one.
    pub fn verify_canonical<S: SignatureScheme, T: AuthorityTier>(
        &self,
        expected_issuer: &ActorKey,
        presented_by: &ActorKey,
        now: u64,
        epoch: PolicyEpoch,
    ) -> Result<Capability<T>, TokenError> {
        self.verify::<S, CanonicalCapabilityCodec, T>(expected_issuer, presented_by, now, epoch)
    }
}

impl fmt::Debug for CapabilityToken {
    /// Lengths only. A token is not secret, but a debug line is not where a reader should be able
    /// to reconstruct one, and a payload can carry whatever a codec put in it.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "CapabilityToken(signature: 64 bytes, payload: {} bytes)",
            self.payload.len()
        )
    }
}

/// Why a token was not accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TokenError {
    /// The input is shorter than the token it claims to be.
    Truncated {
        /// The length the header implies.
        need: usize,
        /// The length supplied.
        found: usize,
    },
    /// The first four bytes are not a capability token.
    BadMagic,
    /// The envelope version is not one this crate reads.
    UnsupportedVersion {
        /// The version byte found.
        found: u8,
    },
    /// The payload is longer than [`MAX_PAYLOAD_BYTES`].
    PayloadTooLarge {
        /// The length declared or supplied.
        found: usize,
    },
    /// Bytes followed a complete token.
    TrailingBytes {
        /// How many.
        extra: usize,
    },
    /// The signature did not verify.
    Signature(VerifyError),
    /// The payload did not decode.
    Codec(CodecError),
    /// The payload decoded but its field values are not a capability at the requested tier.
    Parts(PartsError),
    /// The payload names an issuer other than the key the signature was checked against.
    IssuerMismatch,
    /// The payload names a subject other than the peer presenting it — a replayed bearer token.
    SubjectMismatch,
    /// The capability is expired or from a rotated policy epoch.
    NotCurrent(CapabilityError),
}

impl fmt::Display for TokenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { need, found } => {
                write!(formatter, "a token of {need} bytes was cut off at {found}")
            }
            Self::BadMagic => formatter.write_str("these bytes are not a capability token"),
            Self::UnsupportedVersion { found } => {
                write!(
                    formatter,
                    "capability token version {found} is not supported"
                )
            }
            Self::PayloadTooLarge { found } => write!(
                formatter,
                "a capability token payload is at most {MAX_PAYLOAD_BYTES} bytes, found {found}"
            ),
            Self::TrailingBytes { extra } => {
                write!(formatter, "{extra} bytes followed a complete token")
            }
            Self::Signature(error) => write!(formatter, "signature rejected: {error}"),
            Self::Codec(error) => write!(formatter, "payload rejected: {error}"),
            Self::Parts(error) => write!(formatter, "capability rejected: {error}"),
            Self::IssuerMismatch => formatter
                .write_str("the token names an issuer other than the key it was checked against"),
            Self::SubjectMismatch => {
                formatter.write_str("the token names a subject other than the peer presenting it")
            }
            Self::NotCurrent(error) => write!(formatter, "capability rejected: {error}"),
        }
    }
}

impl std::error::Error for TokenError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DelegationBudget, Expiry, WorkspaceScope};

    fn token() -> CapabilityToken {
        CapabilityToken::new(
            Signature::from_bytes([9u8; SIGNATURE_BYTES]),
            b"payload".to_vec(),
        )
        .expect("small payload")
    }

    fn canonical_parts() -> CapabilityParts {
        CapabilityParts::new(
            ActorKey::from_public_bytes([1; 32]),
            ActorKey::from_public_bytes([2; 32]),
            WorkspaceScope::from_bytes([3; 16]),
            "delegated".to_owned(),
            vec![
                "write-own-actor-state".to_owned(),
                "author-change-set".to_owned(),
                "author-change-set".to_owned(),
            ],
            PolicyEpoch::new(7),
            Expiry::at_unix_millis(200),
            DelegationBudget::new(0),
        )
    }

    #[test]
    fn the_wire_form_round_trips() {
        let original = token();
        let parsed = CapabilityToken::from_wire(&original.to_wire()).expect("round trip");
        assert_eq!(parsed, original);
    }

    #[test]
    fn every_truncation_of_a_valid_token_is_refused() {
        let wire = token().to_wire();
        for cut in 0..wire.len() {
            assert!(
                CapabilityToken::from_wire(&wire[..cut]).is_err(),
                "a token truncated to {cut} bytes parsed"
            );
        }
    }

    #[test]
    fn trailing_bytes_are_refused_rather_than_ignored() {
        let mut wire = token().to_wire();
        wire.push(0);
        assert_eq!(
            CapabilityToken::from_wire(&wire),
            Err(TokenError::TrailingBytes { extra: 1 })
        );
    }

    #[test]
    fn a_declared_length_beyond_the_bound_allocates_nothing() {
        let mut wire = token().to_wire();
        let length_at = 5 + SIGNATURE_BYTES;
        wire[length_at..length_at + 4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(
            CapabilityToken::from_wire(&wire),
            Err(TokenError::PayloadTooLarge {
                found: u32::MAX as usize
            })
        );
    }

    #[test]
    fn the_debug_form_carries_no_payload() {
        let token = CapabilityToken::new(
            Signature::from_bytes([0u8; SIGNATURE_BYTES]),
            b"look-at-me".to_vec(),
        )
        .expect("small payload");
        assert!(!format!("{token:?}").contains("look-at-me"));
    }

    #[test]
    fn the_private_canonical_codec_passes_the_shared_conformance_oracle() {
        crate::conformance::check_codec_conformance::<CanonicalCapabilityCodec>(&[
            canonical_parts(),
        ])
        .expect("canonical codec");
    }

    #[test]
    fn the_canonical_decoder_refuses_wrong_version_and_trailing_bytes() {
        let encoded = CanonicalCapabilityCodec::encode(&canonical_parts());
        let mut wrong_version = encoded.clone();
        wrong_version[1] = 1;
        assert_eq!(
            CanonicalCapabilityCodec::decode(&wrong_version),
            Err(CodecError::UnsupportedVersion)
        );

        let mut trailing = encoded;
        trailing.push(0);
        assert_eq!(
            CanonicalCapabilityCodec::decode(&trailing),
            Err(CodecError::Malformed)
        );
    }
}
