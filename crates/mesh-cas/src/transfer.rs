//! Resumable receipt of one missing chunk into the verified content namespace.
//!
//! A partial transfer is durable but is never content: it lives under `incoming/`, and only the
//! existing hash-verified [`crate::Promotion`] can give it the content-addressed name. Its file
//! length is the resume offset after a restart. A final part whose complete bytes do not hash to
//! the requested name is discarded and returns offset zero for a re-request; it never reaches
//! promotion.

use core::fmt;
use std::io;

use crate::{Cas, CasError, ContentDigest, Digest32, DurableFs, PromotionOutcome};

/// A durable partial transfer for one requested content digest.
#[derive(Debug)]
pub struct IncomingChunk<'a, F: DurableFs, D: ContentDigest> {
    cas: &'a Cas<F, D>,
    expected: Digest32,
    next_offset: u64,
    complete: bool,
}

impl<F: DurableFs, D: ContentDigest> Cas<F, D> {
    /// Open or resume receipt of a missing chunk.
    ///
    /// The returned offset is the durable length already received. If the chunk was promoted
    /// before a crash but its incoming file was not yet removed, this verifies the promoted copy,
    /// removes the stale partial, and returns an already-complete receiver.
    ///
    /// # Errors
    ///
    /// Store I/O, or a corrupt already-promoted chunk after it has been quarantined by
    /// [`Cas::read`]. A caller should surface that corruption and begin a new request.
    pub fn begin_receive(
        &self,
        expected: Digest32,
    ) -> Result<IncomingChunk<'_, F, D>, ReceiveError> {
        let incoming = self.layout().incoming_path(&expected);
        match self.read(&expected) {
            Ok(bytes) => {
                if self.filesystem().exists(&incoming) {
                    self.remove_incoming(&incoming)?;
                }
                Ok(IncomingChunk {
                    cas: self,
                    expected,
                    next_offset: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                    complete: true,
                })
            }
            Err(CasError::Absent { .. }) => {
                let next_offset = match self.filesystem().file_len(&incoming) {
                    Ok(length) => length,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
                    Err(error) => {
                        return Err(ReceiveError::Store(CasError::io(
                            "file_len", &incoming, error,
                        )))
                    }
                };
                Ok(IncomingChunk {
                    cas: self,
                    expected,
                    next_offset,
                    complete: false,
                })
            }
            Err(error) => Err(ReceiveError::Store(error)),
        }
    }

    pub(crate) fn remove_incoming(&self, path: &std::path::Path) -> Result<(), ReceiveError> {
        self.filesystem()
            .remove_file(path)
            .map_err(|error| ReceiveError::Store(CasError::io("remove_file", path, error)))?;
        let directory = self.layout().incoming_directory();
        self.filesystem()
            .sync_dir(&directory)
            .map_err(|error| ReceiveError::Store(CasError::io("sync_dir", directory, error)))
    }
}

impl<F: DurableFs, D: ContentDigest> IncomingChunk<'_, F, D> {
    /// The digest requested from the peer.
    #[must_use]
    pub const fn expected(&self) -> Digest32 {
        self.expected
    }

    /// The next offset to request. It is durable before [`Self::accept`] returns.
    #[must_use]
    pub const fn next_offset(&self) -> u64 {
        self.next_offset
    }

    /// Whether the verified chunk was already available when this receiver was opened.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.complete
    }

    /// Durably accept one contiguous part and promote only after the complete digest verifies.
    ///
    /// # Errors
    ///
    /// Refuses a stale or skipped offset without writing, a non-final empty part that cannot make
    /// progress, a final digest mismatch after discarding the partial, or a store operation.
    pub fn accept(
        &mut self,
        offset: u64,
        bytes: &[u8],
        is_final: bool,
    ) -> Result<ReceiveProgress, ReceiveError> {
        if self.complete {
            return Err(ReceiveError::AlreadyComplete {
                digest: self.expected,
            });
        }
        if offset != self.next_offset {
            return Err(ReceiveError::OffsetMismatch {
                expected: self.next_offset,
                received: offset,
            });
        }
        if bytes.is_empty() && !is_final {
            return Err(ReceiveError::NoProgress { offset });
        }
        let next_offset = self
            .next_offset
            .checked_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
            .ok_or(ReceiveError::OffsetOverflow { offset })?;

        let path = self.cas.layout().incoming_path(&self.expected);
        let created = !self.cas.filesystem().exists(&path);
        if let Err(error) = self.cas.filesystem().append(&path, bytes) {
            // `write_all` may have appended a prefix before returning an error. Refreshing the
            // offset prevents a retry through this still-live handle from duplicating that prefix;
            // reopening after the error reads the same length by the normal restart path.
            if let Ok(length) = self.cas.filesystem().file_len(&path) {
                self.next_offset = length;
            }
            return Err(ReceiveError::Store(CasError::io("append", &path, error)));
        }
        self.next_offset = next_offset;
        self.cas
            .filesystem()
            .sync_file(&path)
            .map_err(|error| ReceiveError::Store(CasError::io("sync_file", &path, error)))?;
        if created {
            let directory = self.cas.layout().incoming_directory();
            self.cas
                .filesystem()
                .sync_dir(&directory)
                .map_err(|error| ReceiveError::Store(CasError::io("sync_dir", directory, error)))?;
        }
        if !is_final {
            return Ok(ReceiveProgress::Continue {
                next_offset: self.next_offset,
            });
        }

        let complete = self
            .cas
            .filesystem()
            .read(&path)
            .map_err(|error| ReceiveError::Store(CasError::io("read", &path, error)))?;
        let found = D::digest_bytes(&complete);
        if found != self.expected {
            let removal = self.cas.remove_incoming(&path);
            self.next_offset = match self.cas.filesystem().file_len(&path) {
                Ok(length) => length,
                Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
                Err(_) => self.next_offset,
            };
            removal?;
            return Err(ReceiveError::IntegrityMismatch {
                expected: self.expected,
                found,
                retry_from: 0,
            });
        }

        let promoted = self
            .cas
            .begin_promotion_expecting(complete, self.expected)
            .map_err(ReceiveError::Store)?
            .finish()
            .map_err(ReceiveError::Store)?;
        // Promotion is the completion point. A failure to remove or sync the no-longer-needed
        // partial must not make this live handle accept bytes beyond already-verified content.
        self.complete = true;
        self.cas.remove_incoming(&path)?;
        Ok(ReceiveProgress::Complete {
            digest: promoted.digest(),
            outcome: promoted.outcome(),
        })
    }
}

/// Progress after accepting a valid contiguous part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiveProgress {
    /// More bytes are needed, starting exactly here.
    Continue {
        /// The durable length to place in the next request.
        next_offset: u64,
    },
    /// The complete bytes verified and reached the existing atomic promotion path.
    Complete {
        /// The verified content name.
        digest: Digest32,
        /// Whether this receipt linked content or found an existing name.
        outcome: PromotionOutcome,
    },
}

/// Why a received chunk part was refused.
#[derive(Debug)]
pub enum ReceiveError {
    /// The chunk is already complete; more bytes would be a protocol error.
    AlreadyComplete {
        /// The chunk already available.
        digest: Digest32,
    },
    /// A part skipped bytes or retried bytes the receiver already made durable.
    OffsetMismatch {
        /// The durable offset the receiver requires.
        expected: u64,
        /// The offset the part claimed.
        received: u64,
    },
    /// A non-final empty part cannot advance a transfer.
    NoProgress {
        /// The offset that would remain unchanged.
        offset: u64,
    },
    /// The offset could not represent the accepted byte count.
    OffsetOverflow {
        /// The offset before the overflowing part.
        offset: u64,
    },
    /// Complete bytes did not match the requested content name and were discarded.
    IntegrityMismatch {
        /// The requested content name.
        expected: Digest32,
        /// The digest the received bytes actually produced.
        found: Digest32,
        /// The offset the caller must request after the partial was discarded.
        retry_from: u64,
    },
    /// The content store refused an operation.
    Store(CasError),
}

impl fmt::Display for ReceiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyComplete { digest } => {
                write!(formatter, "chunk {digest} is already complete")
            }
            Self::OffsetMismatch { expected, received } => write!(
                formatter,
                "received chunk offset {received}, expected durable offset {expected}"
            ),
            Self::NoProgress { offset } => {
                write!(formatter, "an empty non-final part cannot advance offset {offset}")
            }
            Self::OffsetOverflow { offset } => {
                write!(formatter, "a chunk part overflows offset {offset}")
            }
            Self::IntegrityMismatch {
                expected,
                found,
                retry_from,
            } => write!(
                formatter,
                "received chunk hashes to {found}, not {expected}; it was not promoted and must be re-requested from offset {retry_from}"
            ),
            Self::Store(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ReceiveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            _ => None,
        }
    }
}
