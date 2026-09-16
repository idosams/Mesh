//! Identifier ownership at the materialization boundary.
//!
//! Every identifier carried by an operation is owned by `mesh-operations` and re-exported here.
//! [`StateHash`] remains local because it names the materialized state rather than an operation.

use core::fmt;

pub(crate) use mesh_operations::{
    ActorId, ApprovalId, ChangeSetId, HeadId, ManifestId, ObjectId, VersionId, WorkspaceId,
};

/// A digest of one canonically encoded workspace state.
///
/// This is deliberately distinct from [`HeadId`]: a head is a party's claim about which state is
/// current, while a state hash is the state materialization itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateHash([u8; 32]);

impl StateHash {
    /// How many bytes this identifier occupies.
    pub const WIDTH: usize = 32;

    /// Construct a state hash from its digest bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Parse the hexadecimal representation.
    ///
    /// # Errors
    ///
    /// [`mesh_operations::IdError`] if the text is not exactly one 32-byte hexadecimal digest.
    pub fn parse(text: &str) -> Result<Self, mesh_operations::IdError> {
        mesh_operations::ContentHash::parse(text).map(|digest| Self(*digest.as_bytes()))
    }
}

impl fmt::Display for StateHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        mesh_operations::ContentHash::from_bytes(self.0).fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_hash_keeps_the_materializer_owned_api() {
        let bytes = [0xabu8; 32];
        let hash = StateHash::from_bytes(bytes);
        assert_eq!(StateHash::WIDTH, 32);
        assert_eq!(hash.as_bytes(), &bytes);
        assert_eq!(StateHash::parse(&hash.to_string()), Ok(hash));
    }
}
