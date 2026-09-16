//! Why an operation on the store failed, with enough context to act on it.
//!
//! Every variant names the path it is about, because "No such file or directory" without a path is
//! a report nobody can follow. [`CasError::Corrupt`] additionally names where the bad bytes were
//! moved to, so the ledger entry the task's failure-and-recovery clause asks for can point at the
//! evidence rather than describing it.

use core::fmt;
use std::io;
use std::path::PathBuf;

use crate::digest::Digest32;

/// Everything that can go wrong in the store.
#[derive(Debug)]
pub enum CasError {
    /// The filesystem refused an operation.
    Io {
        /// The operation attempted, in the vocabulary of [`crate::DurableFs`].
        operation: &'static str,
        /// The path it was attempted on.
        path: PathBuf,
        /// What the filesystem said.
        source: io::Error,
    },
    /// The caller named a digest for bytes that do not hash to it. Nothing was staged.
    DigestMismatch {
        /// What the caller said the bytes were.
        expected: Digest32,
        /// What the bytes actually hash to.
        found: Digest32,
    },
    /// A staged chunk did not hash to its name when read back from disk before promotion.
    ///
    /// Distinct from [`Self::Corrupt`]: this one never reached the store, so there is nothing to
    /// quarantine and nothing to re-request.
    StagedVerificationFailed {
        /// The name the chunk was to be promoted under.
        expected: Digest32,
        /// What the staged file actually hashes to.
        found: Digest32,
        /// The staged file, which has been removed by the time this is returned.
        staged: PathBuf,
    },
    /// A promoted chunk did not hash to its name at read time. It has been quarantined.
    Corrupt {
        /// The name it was stored under, which is what to re-request from a peer.
        digest: Digest32,
        /// What the bytes on disk actually hash to.
        found: Digest32,
        /// Where the bad bytes were moved to, for the ledger and for diagnosis.
        quarantined: PathBuf,
    },
    /// The store holds no chunk under that name.
    Absent {
        /// The name that was asked for.
        digest: Digest32,
    },
    /// The arrival journal held a line that is neither a record nor a torn tail.
    ///
    /// A torn *tail* — the last line, cut short by a crash mid-append — is ignored by design and
    /// never produces this. A malformed line anywhere else is real damage and is reported rather
    /// than guessed at, because guessing turns a leaked chunk into a deleted one.
    JournalMalformed {
        /// The journal file.
        path: PathBuf,
        /// The 1-based line number that could not be read.
        line: usize,
    },
    /// Every staging name this promotion tried was already taken.
    ///
    /// Two ways to get here. Either leftovers from a crashed process that had this process's
    /// identifier are still in `scratch/`, and the cure is [`crate::Cas::discard_scratch`] at
    /// startup; or more than sixty-four threads of *this* process are promoting byte-for-byte
    /// identical content at the same instant, since staging names are per digest, per process, per
    /// attempt. The second is not a failure of anything, just a bound.
    StagingNamesExhausted {
        /// The last staging path attempted.
        path: PathBuf,
        /// How many names were tried.
        attempts: u32,
    },
}

impl fmt::Display for CasError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                operation,
                path,
                source,
            } => write!(formatter, "{operation} {} failed: {source}", path.display()),
            Self::DigestMismatch { expected, found } => write!(
                formatter,
                "bytes named {expected} hash to {found}, so nothing was staged"
            ),
            Self::StagedVerificationFailed {
                expected,
                found,
                staged,
            } => write!(
                formatter,
                "staged chunk {} hashes to {found}, not {expected}; it was discarded and never \
                 promoted",
                staged.display()
            ),
            Self::Corrupt {
                digest,
                found,
                quarantined,
            } => write!(
                formatter,
                "chunk {digest} hashes to {found} on disk; it was quarantined at {} and must be \
                 re-requested",
                quarantined.display()
            ),
            Self::Absent { digest } => write!(formatter, "the store holds no chunk {digest}"),
            Self::JournalMalformed { path, line } => write!(
                formatter,
                "the arrival journal {} is unreadable at line {line}",
                path.display()
            ),
            Self::StagingNamesExhausted { path, attempts } => write!(
                formatter,
                "no free staging name after {attempts} attempts, last {}; discard scratch at \
                 startup",
                path.display()
            ),
        }
    }
}

impl std::error::Error for CasError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl CasError {
    /// Attach an operation name and a path to a filesystem error.
    pub(crate) fn io(operation: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.into(),
            source,
        }
    }
}
