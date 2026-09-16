//! Key rotation that does not invalidate the past.
//!
//! # What rotation is here, and what it is not
//!
//! ADR-0003 decided that an actor is named by its key and by nothing else, so **rotating an actor
//! key names a different actor**. Rotation is therefore not a rename: it is a succession, and the
//! records the retired key signed keep naming the retired key, correctly and permanently.
//!
//! That makes one property mandatory rather than nice: a verifier must still be able to check a
//! signature made under a key that is no longer used. A [`KeyRing`] is what carries that — the
//! current key plus every key it has ever succeeded, each with the moment it was retired.
//!
//! # Retired keys verify and cannot sign
//!
//! [`KeyRing::signing_key`] returns the current key and only the current key. There is no accessor
//! that hands back a retired key in a form anything will sign with, so "sign with the compromised
//! key" is not an expressible mistake — which matters because rotation most often happens
//! *because* a key was compromised, at the moment the operator has the least attention to spare.
//!
//! # The ring is append-only
//!
//! [`KeyRing::rotate`] consumes the ring and returns a new one; nothing here takes `&mut self`. A
//! key already in the ring cannot be rotated back in, so a ring cannot be walked backwards to
//! restore a retired key to signing standing.

use core::fmt;

use crate::keys::{KeyPair, KeyPurpose};

/// Where a key stands in a ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyStanding {
    /// The key in use. Signs and verifies.
    Current,
    /// A key that has been succeeded. Verifies historical signatures; never signs again.
    Retired {
        /// The Unix millisecond it was retired at.
        at_unix_millis: u64,
    },
}

impl KeyStanding {
    /// Whether a key in this standing may sign.
    #[must_use]
    pub const fn may_sign(&self) -> bool {
        matches!(self, Self::Current)
    }

    /// Whether a key in this standing may verify. Always true: that is the point of the ring.
    #[must_use]
    pub const fn may_verify(&self) -> bool {
        true
    }
}

/// A key and the moment it stopped being current.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetiredKey<P: KeyPurpose> {
    key: KeyPair<P>,
    at_unix_millis: u64,
}

impl<P: KeyPurpose> RetiredKey<P> {
    /// The retired key.
    #[must_use]
    pub const fn key(&self) -> &KeyPair<P> {
        &self.key
    }

    /// When it was retired.
    #[must_use]
    pub const fn at_unix_millis(&self) -> u64 {
        self.at_unix_millis
    }
}

/// One actor's or device's key succession: the current key and every key it succeeded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyRing<P: KeyPurpose> {
    current: KeyPair<P>,
    retired: Vec<RetiredKey<P>>,
}

impl<P: KeyPurpose> KeyRing<P> {
    /// A ring holding one key and no history.
    #[must_use]
    pub fn new(current: KeyPair<P>) -> Self {
        Self {
            current,
            retired: Vec::new(),
        }
    }

    /// The key that signs. Retired keys are unreachable through this method by construction.
    #[must_use]
    pub const fn signing_key(&self) -> &KeyPair<P> {
        &self.current
    }

    /// Every key this ring has retired, oldest first.
    #[must_use]
    pub fn retired(&self) -> &[RetiredKey<P>] {
        &self.retired
    }

    /// How many keys this ring can verify against — the current one plus its whole history.
    #[must_use]
    pub fn verifiable_keys(&self) -> usize {
        self.retired.len() + 1
    }

    /// Where `key` stands in this ring, or `None` if it was never in it.
    ///
    /// A verifier calls this before checking a signature: a key that is not in the ring is not this
    /// actor's key, whatever the signature says.
    #[must_use]
    pub fn standing_of(&self, key: &KeyPair<P>) -> Option<KeyStanding> {
        if self.current == *key {
            return Some(KeyStanding::Current);
        }
        self.retired
            .iter()
            .find(|retired| retired.key == *key)
            .map(|retired| KeyStanding::Retired {
                at_unix_millis: retired.at_unix_millis,
            })
    }

    /// Whether a signature made under `key` can still be verified against this ring.
    #[must_use]
    pub fn may_verify(&self, key: &KeyPair<P>) -> bool {
        self.standing_of(key).is_some_and(|s| s.may_verify())
    }

    /// Succeed the current key with `next`, retiring the current one at `at_unix_millis`.
    ///
    /// Returns a new ring; the original is consumed. Historical verification is preserved because
    /// the retired key stays in the ring, and only there.
    ///
    /// # Errors
    ///
    /// [`RotationError::AlreadyInRing`] when `next` is the current key or one already retired —
    /// which would either be a no-op recorded as a rotation, or a retired key restored to signing.
    /// [`RotationError::TimeMovedBackwards`] when the new retirement is older than the last, which
    /// would make the succession unorderable.
    pub fn rotate(self, next: KeyPair<P>, at_unix_millis: u64) -> Result<Self, RotationError> {
        if self.standing_of(&next).is_some() {
            return Err(RotationError::AlreadyInRing);
        }
        if let Some(last) = self.retired.last() {
            if at_unix_millis < last.at_unix_millis {
                return Err(RotationError::TimeMovedBackwards {
                    last: last.at_unix_millis,
                    supplied: at_unix_millis,
                });
            }
        }
        let mut retired = self.retired;
        retired.push(RetiredKey {
            key: self.current,
            at_unix_millis,
        });
        Ok(Self {
            current: next,
            retired,
        })
    }
}

/// Why a rotation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RotationError {
    /// The proposed key is already the current key or already retired.
    AlreadyInRing,
    /// The retirement moment precedes the previous one.
    TimeMovedBackwards {
        /// The previous retirement moment.
        last: u64,
        /// The moment supplied.
        supplied: u64,
    },
}

impl fmt::Display for RotationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyInRing => {
                formatter.write_str("that key is already in this ring; a retired key never returns")
            }
            Self::TimeMovedBackwards { last, supplied } => write!(
                formatter,
                "the previous key retired at {last} and this rotation claims {supplied}"
            ),
        }
    }
}

impl std::error::Error for RotationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{ActorKey, ForActor};

    fn key(byte: u8) -> ActorKey {
        ActorKey::from_public_bytes([byte; 32])
    }

    #[test]
    fn rotation_preserves_verification_of_every_historical_key() {
        let ring: KeyRing<ForActor> = KeyRing::new(key(1));
        let ring = ring.rotate(key(2), 100).expect("first rotation");
        let ring = ring.rotate(key(3), 200).expect("second rotation");

        assert_eq!(ring.verifiable_keys(), 3);
        assert!(ring.may_verify(&key(1)));
        assert!(ring.may_verify(&key(2)));
        assert!(ring.may_verify(&key(3)));
        assert!(!ring.may_verify(&key(4)));
        assert_eq!(ring.signing_key(), &key(3));
        assert_eq!(
            ring.standing_of(&key(1)),
            Some(KeyStanding::Retired {
                at_unix_millis: 100
            })
        );
    }

    #[test]
    fn a_retired_key_never_signs_again() {
        let ring: KeyRing<ForActor> = KeyRing::new(key(1)).rotate(key(2), 10).expect("rotation");
        assert!(!ring
            .standing_of(&key(1))
            .expect("retired key is in the ring")
            .may_sign());
        assert!(ring
            .standing_of(&key(2))
            .expect("current key is in the ring")
            .may_sign());
    }

    #[test]
    fn a_key_cannot_be_rotated_back_in() {
        let ring: KeyRing<ForActor> = KeyRing::new(key(1)).rotate(key(2), 10).expect("rotation");
        assert_eq!(
            ring.clone().rotate(key(1), 20),
            Err(RotationError::AlreadyInRing)
        );
        assert_eq!(
            ring.clone().rotate(key(2), 20),
            Err(RotationError::AlreadyInRing)
        );
    }

    #[test]
    fn the_succession_cannot_be_reordered() {
        let ring: KeyRing<ForActor> = KeyRing::new(key(1)).rotate(key(2), 100).expect("rotation");
        assert_eq!(
            ring.rotate(key(3), 99),
            Err(RotationError::TimeMovedBackwards {
                last: 100,
                supplied: 99
            })
        );
    }
}
