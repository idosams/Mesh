//! Entity identifiers: UUIDv7, minted, never derived from content.
//!
//! Plan §4.2 splits identity in two, and this module is one half of it. A workspace, a session and
//! an object all have content that changes over their lifetime, so naming them by their content
//! would rename them every time they changed. They are named by a minted version-7 UUID instead —
//! time-ordered, so an index over them stays local, and opaque, so nothing can be inferred from it
//! beyond the millisecond it was minted in.
//!
//! **There is no path from bytes to an entity identifier in this crate.** The only constructors
//! are [`Uuid::new_v7`] and a validating conversion that rejects any other version; nothing here
//! accepts a digest, and the [`crate::CanonicalRecord`] trait — the crate's single derivation
//! path — cannot name one of these types as its `Id` because none of them implements
//! `From<Digest32>`. That last sentence is a compile error, not a comment:
//!
//! ```compile_fail,E0277
//! use mesh_types::{Digest32, WorkspaceId};
//! // A record identifier converts from a digest. An entity identifier must never.
//! let minted = WorkspaceId::from(Digest32::from_bytes([0; 32]));
//! ```
//!
//! ```compile_fail,E0271
//! use mesh_types::{Absorb, CanonicalRecord, DigestHasher, DigestWriter, DomainTag, WorkspaceId};
//! struct Record;
//! impl Absorb for Record {
//!     fn absorb<H: DigestHasher>(&self, _writer: &mut DigestWriter<H>) {}
//! }
//! // `derive_id` can only produce something built from a digest, so this association is rejected.
//! impl CanonicalRecord for Record {
//!     type Id = WorkspaceId;
//!     const DOMAIN: DomainTag = DomainTag::new("test.record");
//! }
//! ```

use core::fmt;

use crate::uuid::{Uuid, UuidParseError};

/// Why a UUID was refused as an entity identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityIdError {
    /// The value parsed but is not version 7.
    WrongVersion {
        /// The version that was found.
        found: u8,
    },
    /// The value parsed but does not carry the RFC 9562 variant bits.
    WrongVariant,
    /// The text was not a UUID at all.
    Malformed(UuidParseError),
}

impl fmt::Display for EntityIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongVersion { found } => write!(
                formatter,
                "an entity identifier is a version 7 UUID, found version {found}"
            ),
            Self::WrongVariant => {
                formatter.write_str("an entity identifier carries the RFC 9562 variant bits")
            }
            Self::Malformed(inner) => write!(formatter, "{inner}"),
        }
    }
}

impl std::error::Error for EntityIdError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Malformed(inner) => Some(inner),
            _ => None,
        }
    }
}

impl From<UuidParseError> for EntityIdError {
    fn from(error: UuidParseError) -> Self {
        Self::Malformed(error)
    }
}

macro_rules! entity_id {
    ($name:ident, $what:literal) => {
        #[doc = concat!("The identifier of ", $what, ".")]
        ///
        /// A minted version-7 UUID. Never derived from content — see the module documentation for
        /// why that is a type-level property here rather than a convention.
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);

        impl $name {
            /// Mint a fresh identifier from a millisecond and caller-supplied randomness.
            ///
            /// The crate reads neither, on purpose: see [`Uuid::new_v7`].
            #[must_use]
            pub const fn mint(unix_millis: u64, random: [u8; 10]) -> Self {
                Self(Uuid::new_v7(unix_millis, random))
            }

            /// Accept an existing UUID, rejecting anything that is not version 7.
            ///
            /// # Errors
            ///
            /// [`EntityIdError`] when the version or the variant is wrong.
            pub const fn from_uuid(uuid: Uuid) -> Result<Self, EntityIdError> {
                if uuid.version() != 7 {
                    return Err(EntityIdError::WrongVersion {
                        found: uuid.version(),
                    });
                }
                if !uuid.is_rfc_variant() {
                    return Err(EntityIdError::WrongVariant);
                }
                Ok(Self(uuid))
            }

            /// Parse the hyphenated form, rejecting anything that is not version 7.
            ///
            /// # Errors
            ///
            /// [`EntityIdError`] when the text is not a UUID, or is not version 7.
            pub fn parse(text: &str) -> Result<Self, EntityIdError> {
                Self::from_uuid(Uuid::parse(text)?)
            }

            /// The underlying UUID.
            #[must_use]
            pub const fn uuid(&self) -> &Uuid {
                &self.0
            }

            /// The millisecond this identifier was minted in.
            #[must_use]
            pub const fn unix_millis(&self) -> u64 {
                self.0.unix_millis()
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

entity_id!(
    WorkspaceId,
    "a workspace — one shared project with one canonical head"
);
entity_id!(
    SessionId,
    "an activity session — one bounded interval of one actor's work"
);
entity_id!(
    ObjectId,
    "a stable object — a file or directory identity independent of any path"
);
entity_id!(
    CapabilityId,
    "a capability — one scoped, expiring grant of authority"
);
