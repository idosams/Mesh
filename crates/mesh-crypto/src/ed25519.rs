//! The audited Ed25519 backend: the one place in this workspace where a signature is checked.
//!
//! # What this file is, and what it deliberately is not
//!
//! It is **not** an implementation of Ed25519. Every line of curve arithmetic, field arithmetic,
//! SHA-512 and point decompression is in `ed25519-dalek`, pinned at an exact version. This file is
//! the adapter: it converts `mesh-types`' `PublicKey` and `Signature` into the dependency's types,
//! applies two checks of its own, and maps the result onto [`VerifyError`].
//!
//! Why the split matters is the whole of ADR-0009. A hand-written Ed25519 fails silently — variable
//! time scalar multiplication leaks the secret, an omitted `S < L` check makes every signature
//! malleable, an accepted low-order point makes two verifiers disagree about the same bytes — and
//! none of those failures is revealed by a passing test vector. So the primitive is borrowed, from
//! the implementation the rest of the ecosystem has been attacking for a decade, and the code here
//! is small enough to read in one sitting.
//!
//! # `verify_strict`, not `verify`
//!
//! `ed25519-dalek` offers both. `VerifyingKey::verify` is the permissive, RFC-compatible check;
//! `verify_strict` additionally rejects a low-order public key `A` and a low-order commitment `R`,
//! and verifies cofactorlessly. The permissive one is correct for interoperating with everything
//! that has ever produced an Ed25519 signature. It is the wrong one here, because Mesh's peers all
//! run this code and the property Mesh needs is that **two verifiers never disagree about the same
//! bytes**: a signature that one relay accepts and another rejects is a fork of canonical state
//! arriving as a compatibility difference. `verify_strict` is the check that has one answer.
//!
//! # The three checks this file makes itself, and why duplicating two of them is not paranoia
//!
//! 1. **`S < L`** — the malleability check. `ed25519-dalek` makes it too, in `check_scalar`, and
//!    **compiles it out** under its `legacy_compatibility` feature, replacing it with the historic
//!    top-three-bits test its own source calls "semi-functional, hacky". This crate does not enable
//!    that feature — but Cargo features are **additive across a whole dependency graph**, so any
//!    crate anywhere in a future graph that enables it turns it on here too, silently, arriving as a
//!    lockfile diff, and every signature in Mesh becomes malleable. [`is_below`] against
//!    [`GROUP_ORDER_LITTLE_ENDIAN`] is a borrow-propagating 32-byte subtraction: a bounds check on
//!    public data, not cryptography, and it makes the rejection independent of every feature flag in
//!    the graph.
//!
//! 2. **The public key is a canonical encoding** — `y < p`, where `p = 2^255 - 19`. This one the
//!    dependency does **not** make: `VerifyingKey::from_bytes` decompresses through
//!    `CompressedEdwardsY`, whose field-element decoder masks the sign bit and reduces modulo `p`,
//!    so the thirty-eight byte strings whose masked value lands in `[p, 2^255)` decode to the same
//!    points as `y ∈ [0, 18]` and are accepted. That is not a signature-forgery hole — the aliased
//!    `y` values are a fixed tiny set and nobody knows their discrete logs — but it is an **identity
//!    aliasing** hole, which is a different thing and it matters here: `mesh_types::PublicKey`
//!    derives an `actor id` by hashing these thirty-two bytes, so two encodings of one point are two
//!    actor identities with one secret between them. Mesh attributes work to actors. One key, one
//!    encoding, one actor.
//!
//! 3. **The public key is not of low order.** Stated at this boundary rather than inferred from the
//!    dependency's choice of entry point, so that a future edit from `verify_strict` to `verify` —
//!    the exact edit somebody makes to close an interoperability complaint — does not silently drop
//!    it. [`Ed25519::verify`] would still reject the whole low-order family.
//!
//! Not duplicated, and deliberately: **the commitment `R`'s encoding**. `verify_strict` recomputes
//! `R` and compares the *compressed bytes* (`verifying.rs:420`, `expected_R == signature.R`, where
//! both sides are a `CompressedEdwardsY`), and a recomputed `R` is canonical by construction, so a
//! non-canonical `R` cannot equal it. Re-checking it here would add a second thing to keep correct
//! for a rejection the dependency cannot stop making.
//!
//! All of it is held to `crate::conformance::check_ed25519_conformance`, which runs the RFC 8032
//! §7.1 vectors *and* six rejection vectors, and which was watched failing against a backend that
//! accepts everything before it was ever pointed at this one.
//!
//! # Timing
//!
//! Verification handles no secret: a public key, a message and a signature are all public, so a
//! timing signal about them reveals nothing an observer did not already have. The constant-time
//! obligation is on **signing**, and this crate does not sign — a secret is reached only through
//! `crate::KeyCustody`, whose implementations are operating-system and hardware backends in other
//! crates. `scalar_is_canonical` is written branch-free regardless, because a helper that is
//! secret-independent today is one somebody reuses on a secret tomorrow.

use ed25519_dalek::{Signature as AuditedSignature, VerifyingKey};
use mesh_types::{PublicKey, Signature};

use crate::keys::PUBLIC_KEY_BYTES;
use crate::scheme::{SignatureScheme, VerifyError};

/// `L`, the order of Ed25519's prime-order subgroup, little-endian.
///
/// `L = 2^252 + 27742317777372353535851937790883648493` (RFC 8032 §5.1). A signature's `S` is a
/// scalar modulo `L`; an `S` at or above it is a second encoding of a signature that already has
/// one, which is the definition of malleability.
const GROUP_ORDER_LITTLE_ENDIAN: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
];

/// `p = 2^255 - 19`, the field's prime, little-endian.
///
/// A public key is the `y` coordinate with the sign of `x` in the top bit. `y` is a field element,
/// so `y < p`; an encoding at or above `p` is a second name for a point that already has one.
const FIELD_PRIME_LITTLE_ENDIAN: [u8; 32] = [
    0xed, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f,
];

/// How many bytes of a signature the commitment `R` occupies; `S` is the rest.
const COMMITMENT_BYTES: usize = 32;

/// The bit a compressed Edwards point carries the sign of `x` in — not part of `y`.
const SIGN_BIT: u8 = 0x80;

/// Whether the little-endian `value` is strictly below the little-endian `modulus`, branch-free.
///
/// Computes `value - modulus` with borrow propagation and reports whether the subtraction borrowed
/// out of the top byte, which happens exactly when `value < modulus`. Nothing short-circuits, so
/// the running time depends on nothing but the fixed length. Both callers pass public data; it is
/// written this way because a helper that is secret-independent today is one somebody reuses on a
/// secret tomorrow.
const fn is_below(value: &[u8; 32], modulus: &[u8; 32]) -> bool {
    let mut borrow: u16 = 0;
    let mut index = 0;
    while index < 32 {
        let difference = (value[index] as u16)
            .wrapping_sub(modulus[index] as u16)
            .wrapping_sub(borrow);
        borrow = (difference >> 8) & 1;
        index += 1;
    }
    borrow == 1
}

/// Whether `scalar` is reduced below the group order — the `S < L` malleability check.
const fn scalar_is_canonical(scalar: &[u8; 32]) -> bool {
    is_below(scalar, &GROUP_ORDER_LITTLE_ENDIAN)
}

/// Whether a compressed point's `y` coordinate is a canonical field element — the `y < p` check.
///
/// The sign bit is cleared first: it belongs to `x`, not to `y`, and leaving it in would reject
/// every key whose `x` is negative.
const fn public_key_is_canonical(encoded: &[u8; PUBLIC_KEY_BYTES]) -> bool {
    let mut coordinate = *encoded;
    coordinate[PUBLIC_KEY_BYTES - 1] &= !SIGN_BIT;
    is_below(&coordinate, &FIELD_PRIME_LITTLE_ENDIAN)
}

/// Ed25519 as RFC 8032 defines it, checked strictly, backed by `ed25519-dalek`.
///
/// The only [`SignatureScheme`] in this workspace. It is a unit type with no state and no
/// constructor argument, so there is no configuration to get wrong and no permissive mode to select.
///
/// ```
/// use mesh_crypto::{conformance::check_ed25519_conformance, Ed25519, SignatureScheme};
///
/// // The RFC 8032 §7.1 vectors and six rejection vectors, including the malleability case.
/// check_ed25519_conformance::<Ed25519>().expect("the audited backend is conformant");
/// assert_eq!(Ed25519::NAME, "ed25519");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ed25519;

impl SignatureScheme for Ed25519 {
    const NAME: &'static str = "ed25519";

    fn verify(
        public_key: &PublicKey,
        message: &[u8],
        signature: &Signature,
    ) -> Result<(), VerifyError> {
        if !public_key_is_canonical(public_key.as_bytes()) {
            return Err(VerifyError::MalformedPublicKey);
        }
        let verifying_key: VerifyingKey = VerifyingKey::from_bytes(public_key.as_bytes())
            .map_err(|_| VerifyError::MalformedPublicKey)?;
        if verifying_key.is_weak() {
            return Err(VerifyError::MalformedPublicKey);
        }

        let bytes = signature.as_bytes();
        let mut scalar = [0u8; COMMITMENT_BYTES];
        scalar.copy_from_slice(&bytes[COMMITMENT_BYTES..]);
        if !scalar_is_canonical(&scalar) {
            return Err(VerifyError::MalformedSignature);
        }

        verifying_key
            .verify_strict(message, &AuditedSignature::from_bytes(bytes))
            .map_err(|_| VerifyError::Mismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::conformance::check_ed25519_conformance;

    /// The acceptance criterion this crate was short of for a whole task. It is asserted here, in
    /// the crate's own unit tests, rather than only in `tests/`, so that `cargo test -p mesh-crypto`
    /// fails the moment a feature-unification accident anywhere in the dependency graph turns the
    /// malleability check off.
    #[test]
    fn the_backend_passes_rfc_8032_and_every_rejection_vector() {
        check_ed25519_conformance::<Ed25519>().unwrap_or_else(|failure| panic!("{failure}"));
    }

    /// The boundary of the `S < L` check, from both sides. A check that is too strict rejects real
    /// signatures and a check that is too lax accepts malleable ones, and only testing the exact
    /// boundary distinguishes a correct constant from a plausible one.
    #[test]
    fn the_scalar_check_is_exact_at_the_group_order() {
        assert!(
            !scalar_is_canonical(&GROUP_ORDER_LITTLE_ENDIAN),
            "S == L is not canonical"
        );

        let mut one_below = GROUP_ORDER_LITTLE_ENDIAN;
        one_below[0] -= 1;
        assert!(scalar_is_canonical(&one_below), "S == L - 1 is canonical");

        let mut one_above = GROUP_ORDER_LITTLE_ENDIAN;
        one_above[0] += 1;
        assert!(
            !scalar_is_canonical(&one_above),
            "S == L + 1 is not canonical"
        );

        assert!(scalar_is_canonical(&[0u8; 32]), "zero is below the order");
        assert!(
            !scalar_is_canonical(&[0xffu8; 32]),
            "the largest 32-byte value is above the order"
        );
    }

    /// The high byte of `L` is `0x10`, so any scalar whose high byte exceeds it is non-canonical
    /// whatever the rest says, and any scalar whose high byte is below it is canonical whatever the
    /// rest says. A borrow implementation that lost a carry fails one of these two directions.
    #[test]
    fn the_scalar_check_reads_the_whole_number_not_just_the_high_byte() {
        for byte in [0x11u8, 0x20, 0x40, 0x80, 0xff] {
            let mut scalar = [0u8; 32];
            scalar[31] = byte;
            assert!(!scalar_is_canonical(&scalar), "high byte {byte:#04x}");
        }

        let mut just_under = [0xffu8; 32];
        just_under[31] = 0x0f;
        assert!(scalar_is_canonical(&just_under), "0x0f… is below L");

        // 2^252 shares L's high byte and is below it: a high-byte-only test would reject this.
        let mut two_to_the_252 = [0u8; 32];
        two_to_the_252[31] = 0x10;
        assert!(scalar_is_canonical(&two_to_the_252), "2^252 < L");

        // Same high byte, above L in the low bytes.
        let mut above = GROUP_ORDER_LITTLE_ENDIAN;
        above[15] += 1;
        assert!(!scalar_is_canonical(&above), "L + 2^120 > L");
    }

    /// The `y < p` check, at its boundary and against the aliases it exists to refuse.
    ///
    /// `0xff * 32` masks to `2^255 - 1`, which is `p + 18`: the dependency decodes it as `y = 18`,
    /// a perfectly good point, and would hand back a `VerifyingKey` whose thirty-two bytes are not
    /// the thirty-two bytes of that key. That is a second `actor id` for one secret.
    #[test]
    fn a_public_key_encoding_above_the_field_prime_is_refused() {
        assert!(
            !public_key_is_canonical(&[0xff; PUBLIC_KEY_BYTES]),
            "0xff… masks to p + 18"
        );
        assert!(
            !public_key_is_canonical(&FIELD_PRIME_LITTLE_ENDIAN),
            "y == p is not canonical"
        );

        let mut one_below = FIELD_PRIME_LITTLE_ENDIAN;
        one_below[0] -= 1;
        assert!(
            public_key_is_canonical(&one_below),
            "y == p - 1 is canonical"
        );

        // The sign bit belongs to x. Setting it must not change the verdict on y.
        let mut signed = one_below;
        signed[PUBLIC_KEY_BYTES - 1] |= SIGN_BIT;
        assert!(
            public_key_is_canonical(&signed),
            "a negative x made a canonical y look non-canonical"
        );
        assert!(public_key_is_canonical(&[0u8; PUBLIC_KEY_BYTES]), "y == 0");
    }

    /// End to end through [`Ed25519::verify`], not just through the helper: the aliases of the
    /// small `y` values are the encodings a hostile peer would actually present.
    #[test]
    fn every_non_canonical_public_key_encoding_is_refused_by_the_scheme() {
        // `p + y` for y in {0, 1, 3, 4, 18}: each decodes to a valid point under a decoder that
        // reduces, and each is a byte string that is not that point's name.
        for hex in [
            "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            "f0ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            "f1ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
        ] {
            let key = key_from_hex(hex);
            assert_eq!(
                Ed25519::verify(&key, b"anything", &Signature::from_bytes([0u8; 64])),
                Err(VerifyError::MalformedPublicKey),
                "non-canonical encoding {hex} was not refused"
            );
        }
    }

    /// The low-order rejection must not depend on `verify_strict` being the entry point, because
    /// swapping it for the permissive `verify` is the one-word edit somebody makes to fix an
    /// interoperability complaint. The check is asserted against the whole published family.
    #[test]
    fn every_low_order_public_key_is_refused_before_the_curve_is_touched() {
        // The eight canonical encodings of the points of order dividing 8 on Ed25519, as published
        // in the small-order-point corpora every Ed25519 test suite carries.
        const LOW_ORDER_KEYS: [&str; 7] = [
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0100000000000000000000000000000000000000000000000000000000000000",
            "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05",
            "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa",
            "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
            "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
        ];

        for hex in LOW_ORDER_KEYS {
            let key = key_from_hex(hex);
            let outcome = Ed25519::verify(&key, b"anything", &Signature::from_bytes([0u8; 64]));
            assert_eq!(
                outcome,
                Err(VerifyError::MalformedPublicKey),
                "low-order key {hex} was not refused as a key"
            );
        }
    }

    /// A key that is not a curve point at all is a different rejection from one that is a weak
    /// point or a non-canonical encoding, and none of the three may reach the verifier.
    ///
    /// `y = 2` is the smallest coordinate for which `(y^2 - 1) / (d·y^2 + 1)` is not a square, so
    /// there is no `x` and the encoding names nothing.
    #[test]
    fn a_public_key_that_is_not_a_curve_point_is_refused() {
        let outcome = Ed25519::verify(
            &key_from_hex("0200000000000000000000000000000000000000000000000000000000000000"),
            b"anything",
            &Signature::from_bytes([0u8; 64]),
        );
        assert_eq!(outcome, Err(VerifyError::MalformedPublicKey));
    }

    fn key_from_hex(hex: &str) -> PublicKey {
        crate::keys::ActorKey::parse_hex(hex)
            .expect("a 64-character hex key")
            .public_key()
    }

    #[test]
    fn the_scheme_names_itself_for_the_wire() {
        assert_eq!(Ed25519::NAME, "ed25519");
    }
}
