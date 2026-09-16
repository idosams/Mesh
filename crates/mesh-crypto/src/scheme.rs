//! The signature seam, and the single implementation of it.
//!
//! # Why the primitive is borrowed rather than written
//!
//! Plan §8.1 pins Ed25519 and this crate does not implement it: [`crate::Ed25519`] adapts
//! `ed25519-dalek`, pinned at an exact version.
//!
//! A hand-written Ed25519 fails in ways a passing test vector does not reveal: variable-time scalar
//! multiplication leaks the secret through timing, incomplete point decoding accepts small-order
//! and non-canonical encodings, an unchecked `S >= L` makes every signature malleable, and
//! cofactor handling decides whether two verifiers agree about the same bytes. Every one of those
//! is silent. `docs/adr/0002-derive-record-ids-with-an-in-crate-blake3.md` took the other road for
//! BLAKE3 and was right to: a hash is fully specified, has published vectors, and runs no secret
//! through its control flow. A signature scheme has all three properties reversed.
//!
//! So the primitive is an audited dependency.
//! `docs/adr/0009-escalate-the-lockfile-fence-for-an-audited-ed25519-rather-than-hand-roll-one.md`
//! is why it was escalated rather than hand-rolled, and
//! `docs/adr/0014-narrow-the-lockfile-fence-to-admit-an-audited-cryptographic-dependency.md` is the
//! answer to that escalation and the bound on what it authorises.
//!
//! # Fail closed, structurally
//!
//! There is no blanket implementation, no `Default`, and exactly one type in this crate implementing
//! [`SignatureScheme`] — the audited adapter, which `crate::no_secret_material` pins by name so that
//! a second one, or a permissive one, fails the build rather than the review. Nothing here verifies
//! a signature without being handed a scheme by its caller, and the only scheme there is to hand it
//! runs the real check.
//!
//! `verify` returns `Result<(), VerifyError>` rather than `bool` for the same reason: a `bool`
//! makes `if verify(..) { }` and `if !verify(..) { }` equally plausible at a glance and makes an
//! ignored return value silent, while an unused `Result` is `unused_must_use`. To be exact about
//! where that becomes fatal, because it is a security claim: **it is a warning by default and an
//! error in the gate.** `[workspace.lints]` does not deny it; `npm run verify:rust` runs
//! `cargo clippy --workspace --all-targets -- -D warnings`, and that is the `-D` that turns
//! discarding a verification result into a build failure. Measured, not assumed — deleting the
//! `?` from `CapabilityToken::verify`'s call to this method produces
//! `error: unused std::result::Result that must be used`.
//!
//! There is also **no default body**: an implementor writes their own check or does not compile.
//! `crate::no_secret_material` asserts that, because a default is exactly how a permissive answer
//! gets inherited by a backend nobody reviewed.

use core::fmt;

use mesh_types::{PublicKey, Signature};

/// A digital signature scheme: the seam an audited Ed25519 backend plugs into.
///
/// # What an implementation must do
///
/// These are requirements on the backend, and [`crate::conformance::check_ed25519_conformance`] is
/// what checks the ones that are checkable from outside:
///
/// 1. **Reject a non-canonical scalar.** A signature whose `S` is not reduced below the group
///    order `L` must fail, or every signature is malleable into a second valid encoding.
/// 2. **Reject a public key that is not a canonical curve point**, including the low-order points.
/// 3. **Take time independent of any secret.** Verification handles no secret, but an
///    implementation shared with signing must not leak one.
/// 4. **Never accept on error.** Every failure path returns `Err`; there is no path that returns
///    `Ok(())` without a completed check.
///
/// # What an implementation must not do
///
/// Return `Ok(())` for anything it cannot check. A backend that cannot yet verify must not exist —
/// the absence of an implementation is a compile error at the call site, which is the loud failure;
/// a permissive one is the silent failure that this seam exists to make impossible to write by
/// accident. `crate::no_secret_material` fails the build if a second implementation appears in this
/// crate, and `tests/conformance_has_teeth.rs` has watched the oracle reject one that accepts
/// everything.
pub trait SignatureScheme {
    /// The scheme's wire name, recorded wherever a signature's provenance is stored.
    const NAME: &'static str;

    /// Verify `signature` over `message` under `public_key`.
    ///
    /// `message` is the framed payload from [`crate::SigningPayload`], never a bare record: a
    /// signature that is valid over undomained bytes is a signature that can be replayed into a
    /// different protocol position.
    ///
    /// # Errors
    ///
    /// [`VerifyError`] for every reason a signature is not valid. There is no success value that
    /// carries doubt.
    fn verify(
        public_key: &PublicKey,
        message: &[u8],
        signature: &Signature,
    ) -> Result<(), VerifyError>;
}

/// Why a signature did not verify.
///
/// Deliberately coarse at the boundary a peer can observe: a caller learns that verification
/// failed, not which internal check failed first. The variants exist so a *local* diagnostic can
/// say something useful, and [`VerifyError::is_rejection`] is what a remote-facing surface uses
/// instead of matching on them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum VerifyError {
    /// The signature is well formed and does not match the message under this key.
    Mismatch,
    /// The signature encoding is not canonical — a non-reduced scalar, or a bad point.
    MalformedSignature,
    /// The public key is not a canonical curve point, or is of low order.
    MalformedPublicKey,
    /// The backend could not complete the check. Never a reason to proceed.
    Unavailable,
}

impl VerifyError {
    /// Whether this is a rejection of the bytes rather than a failure of the checker.
    ///
    /// Both stop the caller. The distinction exists so an operator can tell "somebody presented a
    /// bad signature" from "this machine cannot check signatures", which are different incidents.
    #[must_use]
    pub const fn is_rejection(&self) -> bool {
        !matches!(self, Self::Unavailable)
    }
}

impl fmt::Display for VerifyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Mismatch => "the signature does not match this message under this key",
            Self::MalformedSignature => "the signature encoding is not canonical",
            Self::MalformedPublicKey => "the public key is not a canonical curve point",
            Self::Unavailable => "no signature backend is available to check this signature",
        })
    }
}

impl std::error::Error for VerifyError {}
