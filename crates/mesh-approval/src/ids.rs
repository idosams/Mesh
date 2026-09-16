//! The identifiers a bundle keys on, mirrored rather than imported, plus the one it derives.
//!
//! # Why they are mirrored
//!
//! No crate in this workspace declares a dependency on another. A dependency edge rewrites
//! `Cargo.lock`, which the repository treats as governance surface a lane escalates rather than
//! writes (`docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md`, and the fence
//! narrowed by decision `01KZDWWBXM2HGW1J7K47HM7781`). `mesh-state` hit the wall first;
//! `mesh-conflicts` answered it the same way in its own `src/ids.rs`, and so does this.
//!
//! [`ObjectId`], [`VersionId`], [`ActorId`] and [`HeadId`] are re-declarations of
//! `mesh-conflicts`', byte-width for byte-width. `tests/mesh_conflicts_drift.rs` reads that crate's
//! source and fails if any of the four stops being declared there with the same width.
//!
//! # The one identifier this crate derives rather than mirrors
//!
//! [`ReviewBundleId`] is not an opaque handle somebody mints. It is the digest of the bundle's own
//! canonical bytes, in the `mesh.v0.review-bundle` domain — which is the whole point of the crate:
//! an approval that names a bundle identifier names exact bytes, and anyone holding the bundle can
//! recompute the name and find out whether the bytes they were given are the bytes that were
//! approved. `mesh-types` declares the same name over the same 32 bytes; the drift test holds that
//! too.
//!
//! # What a mirrored identifier does not buy
//!
//! Nothing here verifies that an identifier is the digest of the thing it names. This crate keys on
//! the identifiers it is handed. A caller that hands it an identifier it did not verify gets a
//! bundle over records it did not verify; verification belongs at the boundary holding the bytes.

use core::fmt;

use crate::digest::{Digest32, DigestParseError};

/// How many bytes an object identifier occupies.
pub const OBJECT_ID_BYTES: usize = 16;

/// How many bytes a version identifier occupies.
pub const VERSION_ID_BYTES: usize = 32;

/// How many bytes an actor identifier occupies.
pub const ACTOR_ID_BYTES: usize = 32;

/// How many bytes a head identifier occupies.
pub const HEAD_ID_BYTES: usize = 32;

/// The stable identity of one object, which no rename and no move ever changes.
///
/// A bundle's change list is keyed by this and never by a path, which is what lets the diff be
/// computed rather than declared: a rename is a change to a directory entry, not a delete and a
/// create of something that happens to look similar.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId([u8; OBJECT_ID_BYTES]);

/// The identity of one durable version of one object's content.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionId([u8; VERSION_ID_BYTES]);

/// The identity of the party whose work a bundle offers for review.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActorId([u8; ACTOR_ID_BYTES]);

/// The identity of one head — an actor's, or the canonical one a bundle would advance.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HeadId([u8; HEAD_ID_BYTES]);

macro_rules! opaque_identifier {
    ($name:ident, $width:expr, $label:literal) => {
        impl $name {
            /// How many bytes this identifier occupies.
            pub const BYTE_WIDTH: usize = $width;

            /// Wrap bytes some other layer already minted.
            #[must_use]
            pub const fn from_bytes(bytes: [u8; $width]) -> Self {
                Self(bytes)
            }

            /// The raw bytes.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; $width] {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str($label)?;
                formatter.write_str("(")?;
                for byte in self.0 {
                    write!(formatter, "{byte:02x}")?;
                }
                formatter.write_str(")")
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                for byte in self.0 {
                    write!(formatter, "{byte:02x}")?;
                }
                Ok(())
            }
        }
    };
}

opaque_identifier!(ObjectId, OBJECT_ID_BYTES, "ObjectId");
opaque_identifier!(VersionId, VERSION_ID_BYTES, "VersionId");
opaque_identifier!(ActorId, ACTOR_ID_BYTES, "ActorId");
opaque_identifier!(HeadId, HEAD_ID_BYTES, "HeadId");

/// The identifier of a review bundle: the digest of its own canonical bytes.
///
/// Recomputing it from the bundle is the verification. An identifier that does not match the bytes
/// it names is a corrupt or substituted bundle, and an approval that binds it has been made to
/// point at something a person never read.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReviewBundleId(Digest32);

impl ReviewBundleId {
    /// Name a bundle by a digest a [`crate::ContentDigest`] already produced.
    #[must_use]
    pub const fn from_digest(digest: Digest32) -> Self {
        Self(digest)
    }

    /// The digest.
    #[must_use]
    pub const fn digest(&self) -> &Digest32 {
        &self.0
    }

    /// The 64-character lowercase hex form.
    #[must_use]
    pub fn to_hex(self) -> String {
        self.0.to_hex()
    }

    /// Parse the 64-character hex form.
    ///
    /// # Errors
    ///
    /// [`DigestParseError`] when the text is not 64 hex characters.
    pub fn parse_hex(text: &str) -> Result<Self, DigestParseError> {
        Digest32::parse_hex(text).map(Self)
    }
}

impl From<Digest32> for ReviewBundleId {
    fn from(digest: Digest32) -> Self {
        Self(digest)
    }
}

impl fmt::Display for ReviewBundleId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

impl fmt::Debug for ReviewBundleId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "ReviewBundleId({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_round_trip_their_bytes() {
        assert_eq!(ObjectId::from_bytes([7; 16]).as_bytes(), &[7; 16]);
        assert_eq!(VersionId::from_bytes([9; 32]).as_bytes(), &[9; 32]);
        assert_eq!(ActorId::from_bytes([1; 32]).as_bytes(), &[1; 32]);
        assert_eq!(HeadId::from_bytes([2; 32]).as_bytes(), &[2; 32]);
    }

    #[test]
    fn the_declared_widths_are_the_mirrored_widths() {
        assert_eq!(ObjectId::BYTE_WIDTH, 16);
        assert_eq!(VersionId::BYTE_WIDTH, 32);
        assert_eq!(ActorId::BYTE_WIDTH, 32);
        assert_eq!(HeadId::BYTE_WIDTH, 32);
    }

    #[test]
    fn ordering_is_memcmp_over_the_bytes() {
        assert!(ObjectId::from_bytes([0; 16]) < ObjectId::from_bytes([1; 16]));
    }

    #[test]
    fn a_bundle_identifier_round_trips_through_hex() {
        let id = ReviewBundleId::from_digest(Digest32::from_bytes([0xab; 32]));
        assert_eq!(ReviewBundleId::parse_hex(&id.to_hex()), Ok(id));
        assert_eq!(id.to_hex().len(), 64);
    }

    #[test]
    fn a_bundle_identifier_refuses_text_that_is_not_a_digest() {
        assert!(ReviewBundleId::parse_hex("not a digest").is_err());
    }
}
