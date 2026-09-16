//! Key material: what a public key *is* here, and why the secret half is not a value in this crate.
//!
//! # Two key families that cannot be confused for each other
//!
//! `docs/protocol.md` defines an `actor key` and a `device key` as different things and says why:
//! they are "distinct … so that device compromise and actor compromise are separable". A separation
//! that lives in a field name is separable until somebody passes the wrong variable. So the two are
//! different Rust types with no conversion between them — [`ActorKey`] and [`DeviceKey`], both
//! aliases of one [`KeyPair`] generic over a [`KeyPurpose`] marker. One implementation, two types,
//! no `From` impl, no `as_actor()`.
//!
//! # There is no secret key type, and that is the point
//!
//! Nothing in this crate can hold a private scalar or a seed. A secret is reached only through
//! [`KeyCustody`](crate::KeyCustody), which signs on the holder's behalf and has no method that
//! returns key bytes. "The approval key cannot be exported from an agent-reachable context" is
//! therefore not a rule this crate enforces at a call site; it is a function that does not exist.
//!
//! [`crate::no_secret_material`] is the lint that keeps it that way as the crate grows.
//!
//! # `mesh_types::PublicKey` is what a key pair carries
//!
//! `crates/mesh-types` owns `PublicKey` — "the thirty-two Ed25519 bytes an `actor key` or a
//! `device key` is carried as" — and `Signature`. This crate defines **neither**, and holds a
//! `PublicKey` inside [`KeyPair`] rather than a second 32-byte newtype beside it.
//!
//! That was not always true. Until ADR-0014 narrowed the `Cargo.lock` fence, this crate could not
//! declare `mesh-types = { path = "../mesh-types" }` at all — a workspace path dependency rewrites
//! the lockfile exactly as a third-party one does — so its API spoke `[u8; PUBLIC_KEY_BYTES]` at
//! every boundary where `mesh-types` already had the type. `docs/protocol.md` §1.1 gives a term one
//! definition and one home, and two 32-byte public-key types in one workspace is the beginning of a
//! conversion function that loses the distinction between an `actor key` and a `device key`. The
//! interim shape is gone; [`KeyPair::public_key`] is now the whole conversion, and it is the
//! identity.
//!
//! What this crate does still add is the two names the register assigns to *it* — `actor key` and
//! `device key` — which `mesh-types` does not define, and which are the reason [`KeyPair`] exists
//! at all rather than callers passing a bare `PublicKey` around.

use core::fmt;
use core::marker::PhantomData;

use mesh_types::PublicKey;

/// The length of an Ed25519 public key.
pub const PUBLIC_KEY_BYTES: usize = 32;

/// The length of an Ed25519 signature.
pub const SIGNATURE_BYTES: usize = 64;

mod sealed {
    /// Closes [`super::KeyPurpose`]: a third key family is a protocol change, not a downstream impl.
    pub trait Sealed {}
    impl Sealed for super::ForActor {}
    impl Sealed for super::ForDevice {}
}

/// Which key family a [`KeyPair`] belongs to, as a value.
///
/// The type-level distinction is what the compiler checks; this is for messages and wire formats,
/// where a type parameter cannot travel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyRole {
    /// Signs an actor's ChangeSets and, for a human actor, approval envelopes.
    Actor,
    /// Identifies a device to a relay. Never signs an approval envelope.
    Device,
}

impl KeyRole {
    /// The register's spelling.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Actor => "actor key",
            Self::Device => "device key",
        }
    }

    /// Whether a key in this role may ever carry publication authority.
    ///
    /// A device key never can. This is a statement about the family, not an authorization decision
    /// — the tier types in [`crate::capability`] are what make over-broad authority unrepresentable.
    #[must_use]
    pub const fn may_carry_publication_authority(&self) -> bool {
        matches!(self, Self::Actor)
    }
}

impl fmt::Display for KeyRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The marker half of the actor/device split. Sealed: two families, permanently.
pub trait KeyPurpose: sealed::Sealed + Copy + fmt::Debug + 'static {
    /// The role a key of this purpose reports at runtime.
    const ROLE: KeyRole;
}

/// The purpose marker for an `actor key`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ForActor;

/// The purpose marker for a `device key`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ForDevice;

impl KeyPurpose for ForActor {
    const ROLE: KeyRole = KeyRole::Actor;
}

impl KeyPurpose for ForDevice {
    const ROLE: KeyRole = KeyRole::Device;
}

/// An Ed25519 key pair, named by its public half.
///
/// A pair is determined by its secret and *named* by its public key, which is the only half that
/// ever exists as a value here. Two pairs are equal when their public halves are equal and their
/// purposes are the same type — there is no way to compare an [`ActorKey`] with a [`DeviceKey`],
/// because there is no way to write the comparison.
pub struct KeyPair<P: KeyPurpose> {
    public: PublicKey,
    purpose: PhantomData<P>,
}

/// The Ed25519 key pair that signs an actor's ChangeSets and, for a human actor, approval envelopes.
pub type ActorKey = KeyPair<ForActor>;

/// The Ed25519 key pair identifying a device to a relay, distinct from every actor key.
pub type DeviceKey = KeyPair<ForDevice>;

impl<P: KeyPurpose> KeyPair<P> {
    /// Name a key pair by its public half.
    ///
    /// The bytes are not validated as a curve point. Point validation is the verifying backend's
    /// job and it must happen there — a key that passes a shape check here and fails on the curve
    /// would otherwise read as "checked". [`crate::VerifyError::MalformedPublicKey`] is the answer
    /// a backend returns; this constructor deliberately has no error case to be mistaken for one.
    #[must_use]
    pub const fn from_public_bytes(public: [u8; PUBLIC_KEY_BYTES]) -> Self {
        Self::from_public_key(PublicKey::from_bytes(public))
    }

    /// Name a key pair by the `mesh-types` public key it is carried as.
    #[must_use]
    pub const fn from_public_key(public: PublicKey) -> Self {
        Self {
            public,
            purpose: PhantomData,
        }
    }

    /// The public key, in the one type the workspace defines for it.
    ///
    /// Where the purpose marker stops travelling. A caller that has narrowed an [`ActorKey`] to a
    /// `PublicKey` has thrown away the compiler's guarantee that it is not a [`DeviceKey`], so this
    /// is the boundary to hand to `mesh-types` — an actor identifier, a record's author field —
    /// and not a conversion to reach for inside this crate.
    #[must_use]
    pub const fn public_key(&self) -> PublicKey {
        self.public
    }

    /// The public bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; PUBLIC_KEY_BYTES] {
        self.public.as_bytes()
    }

    /// Which family this key belongs to.
    #[must_use]
    pub const fn role(&self) -> KeyRole {
        P::ROLE
    }

    /// The 64-character lowercase hex form of the public half.
    #[must_use]
    pub fn to_hex(&self) -> String {
        let mut text = String::with_capacity(PUBLIC_KEY_BYTES * 2);
        for byte in *self.public.as_bytes() {
            text.push(hex_digit(byte >> 4));
            text.push(hex_digit(byte & 0x0f));
        }
        text
    }

    /// Parse the 64-character hex form of a public half.
    ///
    /// Total over every input: no length, no character and no encoding makes it panic.
    ///
    /// # Errors
    ///
    /// [`KeyParseError`] when the input is not exactly 64 hex characters.
    pub fn parse_hex(text: &str) -> Result<Self, KeyParseError> {
        let bytes = text.as_bytes();
        if bytes.len() != PUBLIC_KEY_BYTES * 2 {
            return Err(KeyParseError::Length { found: bytes.len() });
        }
        let mut public = [0u8; PUBLIC_KEY_BYTES];
        for (slot, pair) in public.iter_mut().zip(bytes.chunks_exact(2)) {
            let high = hex_value(pair[0]).ok_or(KeyParseError::NotHex)?;
            let low = hex_value(pair[1]).ok_or(KeyParseError::NotHex)?;
            *slot = (high << 4) | low;
        }
        Ok(Self::from_public_bytes(public))
    }
}

const fn hex_digit(nibble: u8) -> char {
    (if nibble < 10 {
        b'0' + nibble
    } else {
        b'a' + nibble - 10
    }) as char
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl<P: KeyPurpose> Clone for KeyPair<P> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<P: KeyPurpose> Copy for KeyPair<P> {}

impl<P: KeyPurpose> PartialEq for KeyPair<P> {
    fn eq(&self, other: &Self) -> bool {
        self.public == other.public
    }
}

impl<P: KeyPurpose> Eq for KeyPair<P> {}

impl<P: KeyPurpose> PartialOrd for KeyPair<P> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<P: KeyPurpose> Ord for KeyPair<P> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.public.cmp(&other.public)
    }
}

impl<P: KeyPurpose> core::hash::Hash for KeyPair<P> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.public.hash(state);
    }
}

impl<P: KeyPurpose> fmt::Debug for KeyPair<P> {
    /// A public key is public, and a full one in every log line is noise. Four bytes identify a key
    /// in a test failure without printing it. Nothing secret can appear here because this type
    /// holds nothing secret.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}({:02x}{:02x}{:02x}{:02x}\u{2026})",
            P::ROLE.as_str().replace(' ', "-"),
            self.as_bytes()[0],
            self.as_bytes()[1],
            self.as_bytes()[2],
            self.as_bytes()[3]
        )
    }
}

impl<P: KeyPurpose> fmt::Display for KeyPair<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

/// Why a hex public key failed to parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyParseError {
    /// The input was not 64 characters long.
    Length {
        /// How many characters were supplied.
        found: usize,
    },
    /// The input contained a character that is not a hex digit.
    NotHex,
}

impl fmt::Display for KeyParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length { found } => write!(
                formatter,
                "a public key is {} hex characters, found {found}",
                PUBLIC_KEY_BYTES * 2
            ),
            Self::NotHex => formatter.write_str("a public key contains only hex characters"),
        }
    }
}

impl std::error::Error for KeyParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_in_either_case() {
        let key = ActorKey::from_public_bytes([0xab; PUBLIC_KEY_BYTES]);
        assert_eq!(ActorKey::parse_hex(&key.to_hex()), Ok(key));
        assert_eq!(
            ActorKey::parse_hex(&key.to_hex().to_uppercase()),
            Ok(key),
            "parsing is case-insensitive; printing is not"
        );
    }

    #[test]
    fn parsing_is_total_over_hostile_input() {
        for text in [
            "",
            "0",
            &"0".repeat(63),
            &"0".repeat(65),
            &"g".repeat(64),
            "\u{00e9}".repeat(32).as_str(),
            &"\u{0000}".repeat(64),
        ] {
            let _ = ActorKey::parse_hex(text);
        }
    }

    #[test]
    fn the_debug_form_prints_four_bytes_and_the_role() {
        let actor = ActorKey::from_public_bytes([0xde; PUBLIC_KEY_BYTES]);
        let device = DeviceKey::from_public_bytes([0xde; PUBLIC_KEY_BYTES]);
        assert_eq!(format!("{actor:?}"), "actor-key(dededede\u{2026})");
        assert!(format!("{device:?}").starts_with("device-key("));
        assert!(
            !format!("{actor:?}").contains(&actor.to_hex()),
            "the whole key must not appear in a debug line"
        );
    }

    #[test]
    fn a_device_key_and_an_actor_key_with_the_same_bytes_are_different_types() {
        let actor = ActorKey::from_public_bytes([7; PUBLIC_KEY_BYTES]);
        let device = DeviceKey::from_public_bytes([7; PUBLIC_KEY_BYTES]);
        // `actor == device` does not compile: `PartialEq` is only implemented within one purpose.
        assert_eq!(actor.as_bytes(), device.as_bytes());
        assert_ne!(actor.role(), device.role());
        assert!(actor.role().may_carry_publication_authority());
        assert!(!device.role().may_carry_publication_authority());
    }
}
