//! Domain separation: the framing that stops one signature from meaning two things.
//!
//! A signature is over bytes. If a capability token and an approval envelope can ever produce the
//! same byte string, a signature made over one is a valid signature over the other, and an actor
//! that can get a human to sign anything can get them to sign a publication. That is a
//! substitution vector, and it is closed by construction rather than by review: every signature in
//! Mesh is made over `len(tag) ‖ tag ‖ len(body) ‖ body`, and no two domains share a tag.
//!
//! The framing is the same shape `mesh-types`' `DigestWriter` uses for identity — an eight-byte
//! big-endian length before every variable-length field — deliberately, so that the workspace has
//! one framing convention rather than two. It is **not** the `canonical encoding`: the body is
//! whatever the caller's encoder produced, and this type never inspects it.

use core::fmt;

/// The protocol position a signature was made in.
///
/// Versioned (`mesh.v0.…`) because changing what a domain covers changes the meaning of every
/// signature in it, which is a protocol change and not an edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DomainSeparator(&'static str);

impl DomainSeparator {
    /// The domain a `capability token` signature is made in.
    pub const CAPABILITY_TOKEN: Self = Self("mesh.v0.capability-token");

    /// Declare a domain.
    #[must_use]
    pub const fn new(tag: &'static str) -> Self {
        Self(tag)
    }

    /// The tag text.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

impl fmt::Display for DomainSeparator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

/// The exact bytes a signature covers.
///
/// Built once and passed to both signing and verification, so the two cannot frame differently.
/// There is no constructor that takes an already-framed buffer: the only way to obtain one is to
/// name a domain.
#[derive(Clone, PartialEq, Eq)]
pub struct SigningPayload {
    domain: DomainSeparator,
    framed: Vec<u8>,
}

impl SigningPayload {
    /// Frame `body` in `domain`.
    #[must_use]
    pub fn new(domain: DomainSeparator, body: &[u8]) -> Self {
        let tag = domain.as_str().as_bytes();
        let mut framed = Vec::with_capacity(16 + tag.len() + body.len());
        framed.extend_from_slice(&(tag.len() as u64).to_be_bytes());
        framed.extend_from_slice(tag);
        framed.extend_from_slice(&(body.len() as u64).to_be_bytes());
        framed.extend_from_slice(body);
        Self { domain, framed }
    }

    /// The domain this payload was framed in.
    #[must_use]
    pub const fn domain(&self) -> DomainSeparator {
        self.domain
    }

    /// The framed bytes to sign or verify.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.framed
    }
}

impl fmt::Debug for SigningPayload {
    /// The length and the domain, never the body: a payload can carry anything a caller encoded,
    /// and a diagnostic is not a place to find out what.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "SigningPayload({}, {} bytes)",
            self.domain,
            self.framed.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_domains_never_produce_the_same_bytes() {
        let one = SigningPayload::new(DomainSeparator::new("mesh.v0.a"), b"body");
        let two = SigningPayload::new(DomainSeparator::new("mesh.v0.b"), b"body");
        assert_ne!(one.as_bytes(), two.as_bytes());
    }

    /// Without the length prefixes, tag `"ab"` with body `"c"` and tag `"a"` with body `"bc"` are
    /// the same bytes. This is the whole reason the framing exists.
    #[test]
    fn a_tag_boundary_cannot_be_moved_into_the_body() {
        let one = SigningPayload::new(DomainSeparator::new("ab"), b"c");
        let two = SigningPayload::new(DomainSeparator::new("a"), b"bc");
        assert_ne!(one.as_bytes(), two.as_bytes());
    }

    #[test]
    fn the_debug_form_carries_no_body() {
        let payload = SigningPayload::new(DomainSeparator::CAPABILITY_TOKEN, b"secret-looking");
        let rendered = format!("{payload:?}");
        assert!(!rendered.contains("secret-looking"), "{rendered}");
        assert!(rendered.contains("mesh.v0.capability-token"), "{rendered}");
    }

    #[test]
    fn an_empty_body_is_still_framed() {
        let payload = SigningPayload::new(DomainSeparator::CAPABILITY_TOKEN, b"");
        assert_eq!(payload.as_bytes().len(), 8 + 24 + 8);
    }
}
