//! Actor and device keys, the signing seam, key custody, rotation and the capability model.
//!
//! Mesh makes one central claim: **an agent cannot advance canonical state.** Not "is not
//! authorized to" — cannot. This crate is where that claim is either true or decorative, so it is
//! worth being precise about which parts of it are enforced by what.
//!
//! # What is enforced by the type system
//!
//! * **An agent's capability cannot name canonical-head advancement.** [`DelegatedAction`] — the
//!   vocabulary every non-human capability draws from — has no such variant. It is not rejected by
//!   a check; the argument cannot be written. [`HumanAction`] has it, and no delegation produces a
//!   [`HumanHeld`] capability, because [`AuthorityTier::Delegated`] is [`Delegated`] for **both**
//!   tiers. The lattice has no way up.
//! * **A capability cannot be widened.** [`Capability::delegate`] is the only derivation on the
//!   type. There is no `widen`, no `with_action`, no `set_actions`, no public field. Every field of
//!   a delegation is equal to or narrower than its parent's, and the delegation budget strictly
//!   decreases so a chain is finite.
//! * **A private key cannot be exported from this crate**, because no function returns one.
//!   [`KeyCustody`] signs; it has no accessor for the secret half, and [`no_secret_material`] fails
//!   the **build** if one is added.
//! * **An unverified capability is not a value.** A [`CapabilityToken`] holds bytes.
//!   [`CapabilityToken::verify`] is the only method that produces a [`Capability`], and it needs a
//!   [`SignatureScheme`], the issuer's key, **the key of the peer presenting it**, the time and the
//!   policy epoch. The presenting peer is an argument rather than a getter because a token is
//!   bearer evidence: a getter makes the one check that stops a stolen token optional.
//! * **An `actor key` is not a `device key`.** Different types, no conversion.
//!
//! # What is not, and is not claimed to be
//!
//! A capability is **not** the cryptographic control. Canonical state advances on a signature over
//! an approval envelope made by a human's key, and no capability substitutes for one. The tier
//! types make an over-broad grant unwritable — defence in depth over key custody — and they do not
//! stop a crate compiled into the same binary from implementing [`HumanKeyCustody`] and attesting
//! to a key of its own choosing.
//!
//! **Nothing in this workspace bounds the set of implementations of that trait today.** No source
//! scan here reads another crate, and no decision record in this repository constrains which
//! custody a binary composes. [`HumanKeyCustody`] says so in its own docs, because a reader who
//! stops worrying at a control that does not exist is worse off than one who knows the shape of
//! the hole. What no implementation can do without the person's private half is produce a valid
//! signature, and that is the control the central claim rests on.
//!
//! # The Ed25519 in this crate is borrowed, not written
//!
//! Plan §8.1 pins Ed25519 and [`Ed25519`] is the workspace's only implementation of
//! [`SignatureScheme`]. Every line of curve arithmetic, field arithmetic, SHA-512 and point
//! decompression behind it is `ed25519-dalek`, pinned at an exact version; the file here is an
//! adapter of roughly a hundred lines, plus two checks it makes itself so that the rejection of a
//! malleable signature does not depend on a feature flag anywhere in the dependency graph.
//!
//! A hand-written Ed25519 carries timing side channels in scalar multiplication, incomplete point
//! and order validation, signature malleability and cofactor pitfalls — failure modes that are
//! silent, that no passing test vector reveals, and that would land in the crate ninety other tasks
//! depend on. `mesh-types` reached the opposite conclusion for BLAKE3 in
//! `docs/adr/0002-derive-record-ids-with-an-in-crate-blake3.md`, correctly: a hash is fully
//! specified, has published vectors, and runs no secret through its control flow. A signature scheme
//! has all three properties reversed, which is
//! `docs/adr/0009-escalate-the-lockfile-fence-for-an-audited-ed25519-rather-than-hand-roll-one.md`,
//! and the answer to that escalation is
//! `docs/adr/0014-narrow-the-lockfile-fence-to-admit-an-audited-cryptographic-dependency.md`.
//!
//! **Nothing here returns `true` for an unchecked signature**, and nothing here can be configured
//! into doing so. There is no permissive mode, no `Default`, no blanket implementation, and
//! [`no_secret_material`] fails `cargo test -p mesh-crypto` if a second [`SignatureScheme`] appears
//! in this crate — so the way a stub arrives, which is beside the real one rather than instead of
//! it, is closed. **A test, not the build**: the banned-identifier half of that module is a `const`
//! assertion and does fail the build, this half is a `#[cfg(test)]` guard over the source text, and
//! rounding the second up to the first would tell a reader the crate cannot compile with a stub in
//! it when it can. What the guard reads is the impl's target trait rather than the prefix of a line,
//! so a path-qualified, line-broken or blanket header is caught; a `use … as` rename of the trait is
//! refused separately, and the residual limits are enumerated in [`no_secret_material`] itself.
//! The oracle in [`conformance`] — RFC 8032 §7.1 plus six rejection vectors, including the
//! non-canonical-scalar malleability case — runs against [`Ed25519`] in this crate's own unit tests,
//! so a supply-chain change that weakened the check fails `cargo test -p mesh-crypto`.
//!
//! # What this crate depends on, and why that list is short
//!
//! Two things. `mesh-types`, which owns `PublicKey`, `Signature`, `ActorId` and the `mesh-cbor/0`
//! canonical encoding — this crate defines no competing 32-byte or 64-byte newtype, and adds only
//! the two names the terminology register assigns to *it*: `actor key` and `device key`. And
//! `ed25519-dalek`, the primitive. That is the whole list, both are pinned with `=` rather than a
//! caret, and ADR-0014 bounds what may join it: an audited *cryptographic* dependency under a named
//! reviewer of the lockfile diff, and nothing else.
//!
//! The canonical encoding remains a seam, [`CapabilityCodec`], with no implementation here: a
//! second encoding of a signed record is a verification failure waiting to happen (ADR-0007), and
//! `mesh-cbor/0` lives in `mesh-types`.
//!
//! # Where to start
//!
//! ```
//! use mesh_crypto::{
//!     ActorKey, Capability, DelegatedAction, Delegation, DelegationBudget, Expiry, KeyRing,
//!     PolicyEpoch,
//! };
//!
//! // Rotation keeps every historical key verifiable and lets only the current one sign.
//! let ring = KeyRing::new(ActorKey::from_public_bytes([1; 32]));
//! let ring = ring.rotate(ActorKey::from_public_bytes([2; 32]), 1_000).expect("rotation");
//! assert!(ring.may_verify(&ActorKey::from_public_bytes([1; 32])));
//! assert_eq!(ring.signing_key(), &ActorKey::from_public_bytes([2; 32]));
//!
//! // `Delegation` carries `DelegatedAction`, which has no canonical-advance variant at all.
//! let _ = Delegation::new(
//!     ActorKey::from_public_bytes([3; 32]),
//!     [DelegatedAction::AuthorChangeSet],
//!     Expiry::at_unix_millis(2_000),
//! );
//! ```

// `forbid`, not `deny`: no module in this crate may re-allow it. What that attribute is, precisely,
// is a claim about **this crate's own source** — and as of ADR-0014 that is no longer the same thing
// as a claim about the compiled artifact. `ed25519-dalek` itself contains no `unsafe` and forbids it
// too, but its graph does: measured over the compiled set, `generic-array` 78 mentions,
// `curve25519-dalek` 35, `sha2` 29, `zeroize` 18, `cpufeatures` 9, `block-buffer` 4, `subtle` 2.
// None of it is reviewable from here and none of it is covered by this line. Saying so is the point:
// an attribute that reads as "there is no unsafe code in Mesh's signature path" would be false, and
// a false security claim is worse than an absent one. Accounting for third-party `unsafe` is
// `01KZDXK6T1NZG9GE4YZGZ06TKS`.
#![forbid(unsafe_code)]

// The modules are private and every public item is re-exported at the crate root, matching
// `mesh-types`: `docs/protocol.md` §3.10 requires every public item to resolve to a register term,
// and a module path is a second name for the same item that no register row covers.
mod capability;
mod conformance_impl;
mod custody;
mod domain;
mod ed25519;
mod keys;
mod no_secret_material;
mod parts;
mod rotation;
mod scheme;
mod token;

/// The conformance oracle a signature backend and a capability codec must pass.
///
/// The one module exposed as a path rather than flattened: it is a *test* surface consumed by
/// another crate's test suite, and putting `ED25519_VECTORS` at the crate root would put a
/// corpus of test data in the same namespace as the protocol's types.
pub mod conformance {
    pub use crate::conformance_impl::{
        check_codec_conformance, check_ed25519_conformance, ConformanceFailure, Ed25519Vector,
        ED25519_REJECTION_VECTORS, ED25519_VECTORS,
    };
}

pub use crate::capability::{
    AuthorityTier, Capability, CapabilityError, Delegated, DelegatedAction, Delegation,
    DelegationBudget, DelegationError, Expiry, HumanAction, HumanHeld, WorkspaceScope,
};
// Re-exported rather than redefined. `mesh-types` owns `PolicyEpoch` because a ChangeSet is sealed
// under one, and a capability that carried a *different* `PolicyEpoch` type would need a conversion
// at every comparison — see the note in `capability.rs`.
pub use crate::custody::{
    CustodyBackend, CustodyError, CustodyRequirement, HumanKeyAttestation, HumanKeyCustody,
    IsolationClass, IsolationProof, KeyCustody, KeyGenerator,
};
pub use crate::domain::{DomainSeparator, SigningPayload};
pub use crate::ed25519::Ed25519;
pub use crate::keys::{
    ActorKey, DeviceKey, ForActor, ForDevice, KeyPair, KeyParseError, KeyPurpose, KeyRole,
    PUBLIC_KEY_BYTES, SIGNATURE_BYTES,
};
pub use crate::parts::{CapabilityParts, PartsError};
pub use crate::rotation::{KeyRing, KeyStanding, RetiredKey, RotationError};
pub use crate::scheme::{SignatureScheme, VerifyError};
pub use crate::token::{
    CapabilityCodec, CapabilityToken, CodecError, TokenError, MAX_PAYLOAD_BYTES, TOKEN_MAGIC,
    TOKEN_VERSION,
};
pub use mesh_types::PolicyEpoch;

/// The crate's name, so a placeholder still carries one verifiable behaviour.
pub const CRATE_NAME: &str = "mesh-crypto";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-crypto");
    }
}
