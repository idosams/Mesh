//! Record identifiers: BLAKE3-derived, recomputable, never minted.
//!
//! The other half of the plan §4.2 split. A version, a manifest, a ChangeSet, a review bundle and
//! an approval are immutable once written, so their content can name them — and because it does,
//! anyone holding the record can recompute the name and find out whether the bytes they were given
//! are the bytes that were promised. That is what "self-verifying" means here, and it is the whole
//! reason the two halves are different types rather than one identifier type used two ways.
//!
//! Each identifier is a distinct newtype over [`Digest32`], and each canonical record derives in
//! its own [`crate::DomainTag`], so two records with identical field bytes in different domains
//! never collide.

use core::fmt;

use crate::digest::{Digest32, DigestParseError};

macro_rules! record_id {
    ($name:ident, $what:literal) => {
        #[doc = concat!("The identifier of ", $what, ".")]
        ///
        /// A 32-byte content digest. Recomputing it from the record is the verification: an
        /// identifier that does not match the bytes it names is a corrupt or substituted record.
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Digest32);

        impl $name {
            /// Name a record by a digest a [`crate::ContentDigest`] already produced.
            #[must_use]
            pub const fn from_digest(digest: Digest32) -> Self {
                Self(digest)
            }

            /// The digest.
            #[must_use]
            pub const fn digest(&self) -> &Digest32 {
                &self.0
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

        impl From<Digest32> for $name {
            fn from(digest: Digest32) -> Self {
                Self(digest)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, formatter)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, concat!(stringify!($name), "({})"), self.0)
            }
        }
    };
}

record_id!(
    ActorId,
    "an actor, derived from that actor's public key and from nothing else"
);
record_id!(
    VersionId,
    "one immutable version of one object — a file version or a directory version"
);
record_id!(ManifestId, "a file manifest");
record_id!(ChangeSetId, "a ChangeSet");
record_id!(
    HeadId,
    "a workspace state some party treats as current — an actor head or the canonical head"
);
record_id!(ReviewBundleId, "a review bundle");
record_id!(ApprovalId, "an approval envelope");
record_id!(
    ContentHash,
    "a byte sequence, used to name a chunk and to verify content on receipt"
);
