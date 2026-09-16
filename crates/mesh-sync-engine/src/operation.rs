//! The seam that turns one operation's canonical encoding back into a value that digests the way
//! its author's did.
//!
//! # Why this is a seam and not a function
//!
//! A ChangeSet's identifier binds its operations, so re-deriving that identifier means absorbing
//! every operation exactly as the author absorbed it. The operation *vocabulary* is
//! `mesh-operations`', and `mesh-types` deliberately never names an operation: a types crate that
//! knew the vocabulary would change every time the vocabulary did. So this crate cannot know how to
//! decode an operation either, and asking it to would put the vocabulary in the transport.
//!
//! What it can do is state the obligation. A composition root supplies an [`OperationDecoder`],
//! and the receipt check is generic over it — the same shape `mesh-state` uses for its `HeadDigest`
//! seam, and for the same reason.
//!
//! # What ships today, stated rather than implied
//!
//! [`EmptyOperations`] is the only decoder in this crate, and it decodes exactly one operation
//! encoding: `mesh.v0.empty-operation`, the placeholder `mesh-types` publishes and the one
//! `protocol/test-vectors/v0/changeset.json` uses. **It is not the eighteen-verb vocabulary.**
//! `mesh-operations` declares those verbs with its own `Absorb`-equivalent, and nothing in the tree
//! bridges them into `mesh-types`' digest framing, so a ChangeSet carrying a real operation cannot
//! have its identifier re-derived by any decoder that exists today. That gap is
//! `01KZGA026PN3AS0VW50FP7M42H`; it is a missing bridge, not a hole in this check, and the check
//! refuses such a record rather than admitting it unverified.

use mesh_types::Absorb;

/// How to turn one operation's complete canonical encoding into a value that absorbs the way its
/// author's did.
///
/// The implementation is supplied by whoever composes the receipt path, because the operation
/// vocabulary belongs to `mesh-operations` and not to the transport.
pub trait OperationDecoder {
    /// The operation type this decoder produces. It must absorb identically to the type the author
    /// sealed the ChangeSet with, or the re-derived identifier will disagree for a record that is
    /// honest.
    type Operation: Absorb;

    /// Decode one operation from its complete canonical encoding.
    ///
    /// # Errors
    ///
    /// A sentence naming what is wrong with the encoding. It is carried into
    /// [`crate::BodyRefused::Operation`] and is diagnostic: a caller decides what to do from the
    /// refusal, never from this string.
    fn decode(&self, canonical: &[u8]) -> Result<Self::Operation, String>;
}

/// The domain tag of the placeholder operation, and the complete canonical encoding of one.
///
/// `encode_canonical` of a record with no field is a one-element array holding the domain tag, so
/// there is exactly one legal encoding and it is thirty bytes: `0x81` then the text string.
const EMPTY_OPERATION_DOMAIN: &str = "mesh.v0.empty-operation";

/// A decoder for the placeholder operation, and for nothing else.
///
/// Read the module header before reaching for this: it covers the operation encoding `mesh-types`
/// publishes today and **not** `mesh-operations`' eighteen verbs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmptyOperations;

impl OperationDecoder for EmptyOperations {
    type Operation = ();

    fn decode(&self, canonical: &[u8]) -> Result<Self::Operation, String> {
        let expected = mesh_types::encode_canonical::<()>(&());
        if canonical == expected {
            Ok(())
        } else {
            Err(format!(
                "this composition decodes only {EMPTY_OPERATION_DOMAIN}, and these {} bytes are \
                 some other operation. A ChangeSet carrying an operation this decoder does not \
                 know is REFUSED rather than admitted with an unchecked identifier.",
                canonical.len()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_placeholder_operation_decodes() {
        let encoding = mesh_types::encode_canonical::<()>(&());
        assert_eq!(EmptyOperations.decode(&encoding), Ok(()));
    }

    #[test]
    fn any_other_operation_is_refused_rather_than_ignored() {
        // A record encoding with a different domain tag: a real operation, as far as this decoder
        // can tell. Skipping it would leave the identifier re-derivable and wrong.
        let other = mesh_types::encode_canonical::<mesh_types::FileManifest>(
            &mesh_types::FileManifest::new(
                0,
                mesh_types::Digest32::from_bytes([0; 32]),
                Vec::new(),
            ),
        );
        assert!(EmptyOperations.decode(&other).is_err());
    }
}
