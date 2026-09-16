//! The three identifiers this crate keys on, mirrored rather than imported.
//!
//! # Why they are mirrored
//!
//! No crate in this workspace declares a dependency on another. A dependency edge rewrites
//! `Cargo.lock`, which the repository treats as governance surface a lane escalates rather than
//! writes (`docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md`, and the fence
//! narrowed by decision `01KZDWWBXM2HGW1J7K47HM7781`). `mesh-state` hit the same wall first and
//! answered it the same way, in its own `src/ids.rs`.
//!
//! So [`ObjectId`], [`VersionId`] and [`ActorId`] are re-declarations of `mesh-state`'s, byte-width
//! for byte-width, built to be deleted the day the edge is allowed.
//! `tests/mesh_state_drift.rs` reads that crate's source and fails if any of the three stops being
//! declared there with the same width.
//!
//! # What a mirrored identifier does not buy
//!
//! Nothing here verifies that an identifier is the digest of the thing it names. This crate keys
//! on the identifiers it is handed and treats two records carrying one [`VersionId`] as one
//! version. **A caller that hands this crate an identifier it did not verify gets a resolution
//! derived from records it did not verify.** Verification belongs at the boundary holding the
//! bytes.

use core::fmt;

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
/// This is the whole of conflict rows one and two: an edit is keyed by this identifier and never
/// by a path, so a rename concurrent with an edit cannot lose the edit and a moved directory
/// cannot detach its children.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId([u8; OBJECT_ID_BYTES]);

/// The identity of one durable version of one object's content.
///
/// Every value of this type that has ever been durable stays reachable through
/// [`Resolution::reachable_versions`](crate::Resolution::reachable_versions). That is the promise
/// the whole crate exists to keep.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionId([u8; VERSION_ID_BYTES]);

/// The identity of the party that authored a change.
///
/// Carried through resolution so a preserved version can be attributed to the actor whose work it
/// is. Nothing in this crate makes a resolution decision from it: a rule that consulted the author
/// would resolve differently on different peers the moment two peers disagreed about who is who.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActorId([u8; ACTOR_ID_BYTES]);

/// The identity of one head, as `mesh-state` derives it.
///
/// Used only by conflict row ten, where the question is whether the head an approval was reviewed
/// against is still the head it would advance.
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

impl ObjectId {
    /// The first eight hexadecimal digits of this identifier.
    ///
    /// The only place a conflict resolution turns an identifier into text a person reads: a
    /// same-name create keeps both objects and has to give the second one a distinguishable name.
    /// Deriving that suffix from the identifier rather than from a counter is what makes the
    /// resulting name identical on every peer.
    #[must_use]
    pub fn short_hex(&self) -> String {
        let mut out = String::with_capacity(8);
        for byte in &self.0[..4] {
            out.push_str(&format!("{byte:02x}"));
        }
        out
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
    fn the_short_form_is_the_leading_four_bytes() {
        let object =
            ObjectId::from_bytes([0xa1, 0xb2, 0xc3, 0xd4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(object.short_hex(), "a1b2c3d4");
    }

    #[test]
    fn ordering_is_memcmp_over_the_bytes() {
        let low = ObjectId::from_bytes([0; 16]);
        let high = ObjectId::from_bytes([1; 16]);
        assert!(low < high);
    }

    #[test]
    fn the_declared_widths_are_the_mirrored_widths() {
        assert_eq!(ObjectId::BYTE_WIDTH, 16);
        assert_eq!(VersionId::BYTE_WIDTH, 32);
        assert_eq!(ActorId::BYTE_WIDTH, 32);
        assert_eq!(HeadId::BYTE_WIDTH, 32);
    }
}
