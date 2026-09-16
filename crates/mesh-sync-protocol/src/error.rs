//! Why a peer refuses a message.
//!
//! Every refusal in this protocol is one value: a [`ProtocolError`] naming the code, the message
//! it is about, and a human-readable detail. The same value is what an `ERROR` message carries, so
//! a precondition checked locally and a refusal received from a peer are the same shape and cannot
//! drift apart.
//!
//! **The detail is diagnostic, never semantic.** A peer decides what to do from the
//! [`ErrorCode`] alone; the detail exists so a human reading a log knows which of the many ways
//! into that code actually happened. A peer that parses the detail has coupled itself to a string.

use core::fmt;

use crate::plane::MessageKind;

/// Why a message was refused. A closed set: adding a member is a protocol change.
///
/// Exactly one of `unsupported version`, `unknown message`, `unauthenticated session`,
/// `unverified peer`, `malformed message`, `unknown actor`, `sequence gap`, `unknown ChangeSet`,
/// `unknown chunk`, `offset past end`, `integrity failure`, `unknown policy epoch`,
/// `batch too large` and `busy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ErrorCode {
    /// The sender's protocol version is one this peer does not speak.
    UnsupportedVersion,
    /// The wire tag names no message this version defines.
    UnknownMessage,
    /// A message that needs an established session arrived before `HELLO` completed.
    UnauthenticatedSession,
    /// The peer is not verified and this session requires verified peers.
    UnverifiedPeer,
    /// The bytes did not decode, or decoded to a shape the schema forbids.
    MalformedMessage,
    /// The message names an actor this peer has never heard of.
    UnknownActor,
    /// A request would leave a hole in an actor's sequence, which no peer may create.
    SequenceGap,
    /// The message names a ChangeSet this peer does not hold.
    UnknownChangeSet,
    /// The message names a chunk this peer does not hold.
    UnknownChunk,
    /// A resumable transfer asked to start past the end of the chunk.
    OffsetPastEnd,
    /// Received bytes did not hash to the content hash that named them.
    IntegrityFailure,
    /// The message was authored under a policy epoch this peer has not learned.
    UnknownPolicyEpoch,
    /// The batch exceeded what the receiver declared it would accept.
    BatchTooLarge,
    /// The receiver is applying backpressure; the request may be retried.
    Busy,
}

/// Every error code, so a conformance harness can enumerate the set.
pub const ERROR_CODES: [ErrorCode; 14] = [
    ErrorCode::UnsupportedVersion,
    ErrorCode::UnknownMessage,
    ErrorCode::UnauthenticatedSession,
    ErrorCode::UnverifiedPeer,
    ErrorCode::MalformedMessage,
    ErrorCode::UnknownActor,
    ErrorCode::SequenceGap,
    ErrorCode::UnknownChangeSet,
    ErrorCode::UnknownChunk,
    ErrorCode::OffsetPastEnd,
    ErrorCode::IntegrityFailure,
    ErrorCode::UnknownPolicyEpoch,
    ErrorCode::BatchTooLarge,
    ErrorCode::Busy,
];

impl ErrorCode {
    /// The wire tag. Assigned once, never reused, never renumbered.
    #[must_use]
    pub const fn tag(self) -> u64 {
        match self {
            Self::UnsupportedVersion => 1,
            Self::UnknownMessage => 2,
            Self::UnauthenticatedSession => 3,
            Self::UnverifiedPeer => 4,
            Self::MalformedMessage => 5,
            Self::UnknownActor => 6,
            Self::SequenceGap => 7,
            Self::UnknownChangeSet => 8,
            Self::UnknownChunk => 9,
            Self::OffsetPastEnd => 10,
            Self::IntegrityFailure => 11,
            Self::UnknownPolicyEpoch => 12,
            Self::BatchTooLarge => 13,
            Self::Busy => 14,
        }
    }

    /// The code a wire tag names, or `None` for a tag this version does not define.
    #[must_use]
    pub fn from_tag(tag: u64) -> Option<Self> {
        ERROR_CODES.into_iter().find(|code| code.tag() == tag)
    }

    /// The wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedVersion => "unsupported version",
            Self::UnknownMessage => "unknown message",
            Self::UnauthenticatedSession => "unauthenticated session",
            Self::UnverifiedPeer => "unverified peer",
            Self::MalformedMessage => "malformed message",
            Self::UnknownActor => "unknown actor",
            Self::SequenceGap => "sequence gap",
            Self::UnknownChangeSet => "unknown ChangeSet",
            Self::UnknownChunk => "unknown chunk",
            Self::OffsetPastEnd => "offset past end",
            Self::IntegrityFailure => "integrity failure",
            Self::UnknownPolicyEpoch => "unknown policy epoch",
            Self::BatchTooLarge => "batch too large",
            Self::Busy => "busy",
        }
    }

    /// Whether the sender may send the same message again unchanged and expect a different answer.
    ///
    /// Backpressure and a not-yet-learned policy epoch are transient; every other code says the
    /// message itself is wrong, and resending it is a loop.
    #[must_use]
    pub const fn is_retryable(self) -> bool {
        matches!(self, Self::Busy | Self::UnknownPolicyEpoch)
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A refusal: the code, the message it is about, and a diagnostic detail.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolError {
    code: ErrorCode,
    about: MessageKind,
    detail: String,
}

impl ProtocolError {
    /// A refusal of `about` with `code`, explained by `detail`.
    #[must_use]
    pub fn new(code: ErrorCode, about: MessageKind, detail: impl Into<String>) -> Self {
        Self {
            code,
            about,
            detail: detail.into(),
        }
    }

    /// Why the message was refused.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    /// Which message was refused.
    #[must_use]
    pub const fn about(&self) -> MessageKind {
        self.about
    }

    /// The diagnostic detail. Never parsed by a peer.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} refused: {}", self.about, self.code)?;
        if !self.detail.is_empty() {
            write!(formatter, " ({})", self.detail)?;
        }
        Ok(())
    }
}

impl std::error::Error for ProtocolError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_code_has_a_distinct_tag_that_round_trips() {
        let tags: BTreeSet<u64> = ERROR_CODES.iter().map(|code| code.tag()).collect();
        assert_eq!(tags.len(), ERROR_CODES.len());
        for code in ERROR_CODES {
            assert_eq!(ErrorCode::from_tag(code.tag()), Some(code));
        }
    }

    #[test]
    fn every_code_has_a_distinct_name() {
        let names: BTreeSet<&str> = ERROR_CODES.iter().map(|code| code.as_str()).collect();
        assert_eq!(names.len(), ERROR_CODES.len());
    }

    #[test]
    fn an_undefined_error_tag_resolves_to_nothing() {
        assert_eq!(ErrorCode::from_tag(0), None);
        assert_eq!(ErrorCode::from_tag(15), None);
    }

    /// A code that is wrong about the message is a resend loop. Only the two transient ones say
    /// "try again", and this pins which two they are.
    #[test]
    fn only_backpressure_and_an_unlearned_epoch_are_retryable() {
        let retryable: BTreeSet<&str> = ERROR_CODES
            .iter()
            .filter(|code| code.is_retryable())
            .map(|code| code.as_str())
            .collect();
        assert_eq!(retryable, BTreeSet::from(["busy", "unknown policy epoch"]));
    }

    #[test]
    fn a_refusal_prints_the_message_and_the_code() {
        let refusal = ProtocolError::new(
            ErrorCode::SequenceGap,
            MessageKind::RequestOperations,
            "asked from 5 while holding through 2",
        );
        assert_eq!(
            refusal.to_string(),
            "REQUEST_OPERATIONS refused: sequence gap (asked from 5 while holding through 2)"
        );
        assert_eq!(refusal.code(), ErrorCode::SequenceGap);
        assert_eq!(refusal.about(), MessageKind::RequestOperations);
    }

    #[test]
    fn an_empty_detail_does_not_print_empty_brackets() {
        let refusal = ProtocolError::new(ErrorCode::Busy, MessageKind::ChunkBatch, "");
        assert_eq!(refusal.to_string(), "CHUNK_BATCH refused: busy");
        assert!(refusal.detail().is_empty());
    }
}
