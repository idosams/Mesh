//! The wire encoding of a [`SyncMessage`]: `mesh-cbor/0`, the same profile signed records use.
//!
//! # Why the same profile, and what that does and does not mean
//!
//! `mesh-types` defines `mesh-cbor/0` — a closed subset of CBOR admitting unsigned integers, byte
//! strings, text strings, definite-length arrays and booleans, each with the shortest head that
//! fits. No maps, no tags, no floats, no negative integers, no indefinite lengths. It exists so
//! that two implementations cannot disagree about the bytes a signature covers.
//!
//! CWP messages are **not signed**, so canonicity is not a signature requirement here. It is a
//! *conformance* requirement: one message must have exactly one encoding, or the published test
//! vectors an external implementer checks against would only pin one encoder's habits. Reusing the
//! profile rather than inventing a second one also means a record body carried opaque inside a
//! message is in the same profile as the message that carries it, so a reader needs one decoder.
//!
//! `tests/mesh_types_drift.rs` holds this claim against `mesh-types`' own source: if the profile
//! name or its rules move there, this file is no longer describing the same encoding and the test
//! says so.
//!
//! # The frame
//!
//! Every message is a two-element array: the [`crate::MessageKind`] tag, then the body, itself an
//! array of the fields in the order [`crate::SyncMessage`] declares them. A decoder reads the tag
//! and knows the arity it must find; an unknown tag is [`WireError::UnknownMessageTag`] rather
//! than a guess, which is what makes a newer peer's message a clean refusal.

use crate::error::{ErrorCode, ProtocolError};
use crate::ids::{
    ActorId, ActorSequence, ApprovalId, ChangeSetId, ContentHash, HeadId, ManifestId, PolicyEpoch,
    ReviewBundleId,
};
use crate::message::{
    CarriedChangeSet, ChunkPart, ChunkRequest, HeadAdvertisement, PresenceState, SparseChangeSet,
    SyncMessage,
};
use crate::plane::MessageKind;
use crate::summary::{MerkleSummary, SummaryNode};

/// The name of the message framing this module implements, as published.
///
/// Versioned separately from the record encoding profile it is built on: a change to the framing
/// is a new name here, never an edit to this one.
pub const WIRE_FORMAT: &str = "mesh-cwp-wire/0";

/// CBOR major type 0: an unsigned integer.
const MAJOR_UNSIGNED: u8 = 0;
/// CBOR major type 2: a byte string.
const MAJOR_BYTES: u8 = 2;
/// CBOR major type 3: a UTF-8 text string.
const MAJOR_TEXT: u8 = 3;
/// CBOR major type 4: an array.
const MAJOR_ARRAY: u8 = 4;
/// The one-byte encoding of `false`.
const FALSE_BYTE: u8 = 0xf4;
/// The one-byte encoding of `true`.
const TRUE_BYTE: u8 = 0xf5;

/// Why some bytes are not a CWP message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireError {
    /// The bytes ended in the middle of an item.
    Truncated,
    /// An item had a major type the schema does not put at that position.
    UnexpectedType {
        /// The CBOR major type the schema requires.
        expected: u8,
        /// The major type found.
        found: u8,
    },
    /// A head used more bytes than the value needs, which `mesh-cbor/0` forbids: it is the one
    /// place an encoder could otherwise choose, and two choices are two encodings of one message.
    NonMinimalHead,
    /// An array had a length the schema does not allow at that position.
    WrongArity {
        /// The length the schema requires.
        expected: usize,
        /// The length found.
        found: usize,
    },
    /// A byte string that must be a thirty-two-byte identifier was another width.
    WrongIdentifierWidth {
        /// The width found.
        found: usize,
    },
    /// A text string was not valid UTF-8.
    NotUtf8,
    /// A simple value other than `true` or `false`.
    NotABoolean,
    /// The message tag names no message this version defines.
    UnknownMessageTag(u64),
    /// The error tag names no error code this version defines.
    UnknownErrorTag(u64),
    /// The presence tag names no presence state this version defines.
    UnknownPresenceTag(u64),
    /// A length or count did not fit this machine's address space.
    LengthOutOfRange,
    /// The message decoded, and bytes were left over.
    TrailingBytes {
        /// How many bytes were left.
        remaining: usize,
    },
}

impl core::fmt::Display for WireError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Truncated => formatter.write_str("the bytes ended inside an item"),
            Self::UnexpectedType { expected, found } => write!(
                formatter,
                "expected CBOR major type {expected}, found {found}"
            ),
            Self::NonMinimalHead => {
                formatter.write_str("a head is longer than the value needs, which is not canonical")
            }
            Self::WrongArity { expected, found } => {
                write!(formatter, "expected {expected} elements, found {found}")
            }
            Self::WrongIdentifierWidth { found } => {
                write!(formatter, "an identifier is 32 bytes, found {found}")
            }
            Self::NotUtf8 => formatter.write_str("a text string is not valid UTF-8"),
            Self::NotABoolean => formatter.write_str("a simple value other than true or false"),
            Self::UnknownMessageTag(tag) => write!(formatter, "no message carries tag {tag}"),
            Self::UnknownErrorTag(tag) => write!(formatter, "no error code carries tag {tag}"),
            Self::UnknownPresenceTag(tag) => {
                write!(formatter, "no presence state carries tag {tag}")
            }
            Self::LengthOutOfRange => formatter.write_str("a length does not fit in memory"),
            Self::TrailingBytes { remaining } => {
                write!(formatter, "{remaining} bytes left after a complete message")
            }
        }
    }
}

impl std::error::Error for WireError {}

impl WireError {
    /// The refusal a receiver answers this decoding failure with.
    ///
    /// Every failure here is [`ErrorCode::MalformedMessage`] except an unknown message tag, which
    /// is [`ErrorCode::UnknownMessage`] — a peer speaking a version this one does not, rather than
    /// a peer sending nonsense.
    #[must_use]
    pub fn refusal(&self) -> ProtocolError {
        let code = match self {
            Self::UnknownMessageTag(_) => ErrorCode::UnknownMessage,
            _ => ErrorCode::MalformedMessage,
        };
        ProtocolError::new(code, MessageKind::Error, self.to_string())
    }
}

/// A writer that can only produce `mesh-cbor/0`.
struct Writer {
    buffer: Vec<u8>,
}

impl Writer {
    const fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// The major type in the top three bits, then the argument in the shortest form that holds it.
    fn head(&mut self, major: u8, argument: u64) {
        let tag = major << 5;
        if argument < 24 {
            self.buffer.push(tag | u8::try_from(argument).unwrap_or(0));
        } else if let Ok(small) = u8::try_from(argument) {
            self.buffer.push(tag | 24);
            self.buffer.push(small);
        } else if let Ok(medium) = u16::try_from(argument) {
            self.buffer.push(tag | 25);
            self.buffer.extend_from_slice(&medium.to_be_bytes());
        } else if let Ok(large) = u32::try_from(argument) {
            self.buffer.push(tag | 26);
            self.buffer.extend_from_slice(&large.to_be_bytes());
        } else {
            self.buffer.push(tag | 27);
            self.buffer.extend_from_slice(&argument.to_be_bytes());
        }
    }

    fn unsigned(&mut self, value: u64) {
        self.head(MAJOR_UNSIGNED, value);
    }

    fn bytes(&mut self, value: &[u8]) {
        self.head(MAJOR_BYTES, value.len() as u64);
        self.buffer.extend_from_slice(value);
    }

    fn text(&mut self, value: &str) {
        self.head(MAJOR_TEXT, value.len() as u64);
        self.buffer.extend_from_slice(value.as_bytes());
    }

    fn array(&mut self, length: usize) {
        self.head(MAJOR_ARRAY, length as u64);
    }

    fn boolean(&mut self, value: bool) {
        self.buffer.push(if value { TRUE_BYTE } else { FALSE_BYTE });
    }

    fn sequence(&mut self, value: ActorSequence) {
        self.unsigned(value.get());
    }

    fn optional_head(&mut self, value: Option<HeadId>) {
        match value {
            Some(head) => {
                self.array(1);
                self.bytes(head.as_bytes());
            }
            None => self.array(0),
        }
    }
}

/// A reader over `mesh-cbor/0`, refusing anything the profile does not admit.
struct Reader<'bytes> {
    bytes: &'bytes [u8],
    at: usize,
}

impl<'bytes> Reader<'bytes> {
    const fn new(bytes: &'bytes [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'bytes [u8], WireError> {
        let end = self.at.checked_add(count).ok_or(WireError::Truncated)?;
        let slice = self.bytes.get(self.at..end).ok_or(WireError::Truncated)?;
        self.at = end;
        Ok(slice)
    }

    fn head(&mut self, expected: u8) -> Result<u64, WireError> {
        let first = *self.take(1)?.first().ok_or(WireError::Truncated)?;
        let major = first >> 5;
        if major != expected {
            return Err(WireError::UnexpectedType {
                expected,
                found: major,
            });
        }
        let short = first & 0x1f;
        let argument = match short {
            0..=23 => u64::from(short),
            24 => u64::from(*self.take(1)?.first().ok_or(WireError::Truncated)?),
            25 => {
                let raw: [u8; 2] = self.take(2)?.try_into().map_err(|_| WireError::Truncated)?;
                u64::from(u16::from_be_bytes(raw))
            }
            26 => {
                let raw: [u8; 4] = self.take(4)?.try_into().map_err(|_| WireError::Truncated)?;
                u64::from(u32::from_be_bytes(raw))
            }
            27 => {
                let raw: [u8; 8] = self.take(8)?.try_into().map_err(|_| WireError::Truncated)?;
                u64::from_be_bytes(raw)
            }
            _ => return Err(WireError::NonMinimalHead),
        };
        if !is_minimal(short, argument) {
            return Err(WireError::NonMinimalHead);
        }
        Ok(argument)
    }

    fn unsigned(&mut self) -> Result<u64, WireError> {
        self.head(MAJOR_UNSIGNED)
    }

    fn sequence(&mut self) -> Result<ActorSequence, WireError> {
        Ok(ActorSequence::new(self.unsigned()?))
    }

    fn epoch(&mut self) -> Result<PolicyEpoch, WireError> {
        Ok(PolicyEpoch::new(self.unsigned()?))
    }

    fn byte_string(&mut self) -> Result<Vec<u8>, WireError> {
        let length = self.length(MAJOR_BYTES)?;
        Ok(self.take(length)?.to_vec())
    }

    fn identifier(&mut self) -> Result<[u8; 32], WireError> {
        let length = self.length(MAJOR_BYTES)?;
        let raw = self.take(length)?;
        raw.try_into()
            .map_err(|_| WireError::WrongIdentifierWidth { found: length })
    }

    fn text(&mut self) -> Result<String, WireError> {
        let length = self.length(MAJOR_TEXT)?;
        let raw = self.take(length)?;
        String::from_utf8(raw.to_vec()).map_err(|_| WireError::NotUtf8)
    }

    fn boolean(&mut self) -> Result<bool, WireError> {
        match *self.take(1)?.first().ok_or(WireError::Truncated)? {
            TRUE_BYTE => Ok(true),
            FALSE_BYTE => Ok(false),
            _ => Err(WireError::NotABoolean),
        }
    }

    fn length(&mut self, major: u8) -> Result<usize, WireError> {
        usize::try_from(self.head(major)?).map_err(|_| WireError::LengthOutOfRange)
    }

    fn array(&mut self) -> Result<usize, WireError> {
        self.length(MAJOR_ARRAY)
    }

    fn array_of(&mut self, expected: usize) -> Result<(), WireError> {
        let found = self.array()?;
        if found == expected {
            Ok(())
        } else {
            Err(WireError::WrongArity { expected, found })
        }
    }

    /// Read `n` elements, where `n` is the array length that precedes them.
    fn list<T>(
        &mut self,
        mut element: impl FnMut(&mut Self) -> Result<T, WireError>,
    ) -> Result<Vec<T>, WireError> {
        let count = self.array()?;
        // A count is not a promise: refuse to reserve more than the remaining bytes could hold.
        let mut values = Vec::with_capacity(count.min(self.bytes.len().saturating_sub(self.at)));
        for _ in 0..count {
            values.push(element(self)?);
        }
        Ok(values)
    }

    fn optional_head(&mut self) -> Result<Option<HeadId>, WireError> {
        match self.array()? {
            0 => Ok(None),
            1 => Ok(Some(HeadId::from_bytes(self.identifier()?))),
            found => Err(WireError::WrongArity { expected: 1, found }),
        }
    }

    const fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }
}

/// Whether `argument` is encoded in the shortest head available.
const fn is_minimal(short: u8, argument: u64) -> bool {
    match short {
        24 => argument >= 24,
        25 => argument > u16::MAX as u64 >> 8,
        26 => argument > u32::MAX as u64 >> 16,
        27 => argument > u64::MAX >> 32,
        _ => true,
    }
}

/// The `mesh-cbor/0` bytes of a message.
#[must_use]
pub fn encode_message(message: &SyncMessage) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.array(2);
    writer.unsigned(message.kind().tag());
    write_body(&mut writer, message);
    writer.buffer
}

fn write_body(writer: &mut Writer, message: &SyncMessage) {
    match message {
        SyncMessage::Hello {
            protocol_version,
            encoding_profile,
            actor,
            challenge,
        } => {
            writer.array(4);
            writer.unsigned(u64::from(*protocol_version));
            writer.text(encoding_profile);
            writer.bytes(actor.as_bytes());
            writer.bytes(challenge);
        }
        SyncMessage::Authenticate {
            actor,
            challenge,
            signature,
        } => {
            writer.array(3);
            writer.bytes(actor.as_bytes());
            writer.bytes(challenge);
            writer.bytes(signature);
        }
        SyncMessage::AdvertiseFrontier {
            heads,
            canonical_head,
            policy_epoch,
        } => {
            writer.array(3);
            writer.array(heads.len());
            for entry in heads {
                writer.array(4);
                writer.bytes(entry.actor.as_bytes());
                writer.bytes(entry.head.as_bytes());
                writer.sequence(entry.contiguous_through);
                write_sparse(writer, &entry.sparse);
            }
            writer.optional_head(*canonical_head);
            writer.unsigned(policy_epoch.get());
        }
        SyncMessage::RequestOperations {
            actor,
            from_sequence,
            specific,
            max_count,
        } => {
            writer.array(4);
            writer.bytes(actor.as_bytes());
            writer.sequence(*from_sequence);
            writer.array(specific.len());
            for id in specific {
                writer.bytes(id.as_bytes());
            }
            writer.unsigned(u64::from(*max_count));
        }
        SyncMessage::OperationsBatch { changesets } => {
            writer.array(1);
            writer.array(changesets.len());
            for carried in changesets {
                write_carried(writer, carried);
            }
        }
        SyncMessage::AckOperations {
            actor,
            contiguous_through,
            sparse,
        } => {
            writer.array(3);
            writer.bytes(actor.as_bytes());
            writer.sequence(*contiguous_through);
            write_sparse(writer, sparse);
        }
        SyncMessage::AdvertiseManifests { manifests } => {
            writer.array(1);
            writer.array(manifests.len());
            for manifest in manifests {
                writer.bytes(manifest.as_bytes());
            }
        }
        SyncMessage::RequestChunks { requests } => {
            writer.array(1);
            writer.array(requests.len());
            for request in requests {
                writer.array(3);
                writer.bytes(request.content.as_bytes());
                writer.unsigned(request.from_offset);
                writer.unsigned(request.max_bytes);
            }
        }
        SyncMessage::ChunkBatch { parts } => {
            writer.array(1);
            writer.array(parts.len());
            for part in parts {
                writer.array(4);
                writer.bytes(part.content.as_bytes());
                writer.unsigned(part.offset);
                writer.bytes(&part.bytes);
                writer.boolean(part.is_final);
            }
        }
        SyncMessage::AckChunks { verified } => {
            writer.array(1);
            writer.array(verified.len());
            for chunk in verified {
                writer.bytes(chunk.as_bytes());
            }
        }
        SyncMessage::UpdateActorHead {
            actor,
            head,
            sequence,
        } => {
            writer.array(3);
            writer.bytes(actor.as_bytes());
            writer.bytes(head.as_bytes());
            writer.sequence(*sequence);
        }
        SyncMessage::UpdateCanonicalHead {
            head,
            policy_epoch,
            receipt,
        } => {
            writer.array(3);
            writer.bytes(head.as_bytes());
            writer.unsigned(policy_epoch.get());
            writer.bytes(receipt.as_bytes());
        }
        SyncMessage::Presence {
            actor,
            state,
            expires_after_millis,
        } => {
            writer.array(3);
            writer.bytes(actor.as_bytes());
            writer.unsigned(state.tag());
            writer.unsigned(*expires_after_millis);
        }
        SyncMessage::ReviewBundle { id, body } => {
            writer.array(2);
            writer.bytes(id.as_bytes());
            writer.bytes(body);
        }
        SyncMessage::ValidationReceipt { about, body } => {
            writer.array(2);
            writer.bytes(about.as_bytes());
            writer.bytes(body);
        }
        SyncMessage::ApprovalEnvelope { id, body } => {
            writer.array(2);
            writer.bytes(id.as_bytes());
            writer.bytes(body);
        }
        SyncMessage::AntiEntropySummary { actor, summary } => {
            writer.array(2);
            writer.bytes(actor.as_bytes());
            writer.array(summary.nodes().len());
            for node in summary.nodes() {
                writer.array(3);
                writer.sequence(node.first());
                writer.sequence(node.last());
                writer.bytes(node.digest());
            }
        }
        SyncMessage::Error(refusal) => {
            writer.array(3);
            writer.unsigned(refusal.code().tag());
            writer.unsigned(refusal.about().tag());
            writer.text(refusal.detail());
        }
    }
}

fn write_sparse(writer: &mut Writer, sparse: &[SparseChangeSet]) {
    writer.array(sparse.len());
    for entry in sparse {
        writer.array(2);
        writer.sequence(entry.sequence);
        writer.bytes(entry.id.as_bytes());
    }
}

fn write_carried(writer: &mut Writer, carried: &CarriedChangeSet) {
    writer.array(8);
    writer.bytes(carried.id.as_bytes());
    writer.bytes(carried.author.as_bytes());
    writer.sequence(carried.sequence);
    writer.array(carried.parents.len());
    for parent in &carried.parents {
        writer.bytes(parent.as_bytes());
    }
    writer.bytes(carried.base_head.as_bytes());
    writer.bytes(carried.resulting_head.as_bytes());
    writer.unsigned(carried.policy_epoch.get());
    writer.bytes(&carried.body);
}

/// The message some bytes encode.
///
/// # Errors
///
/// [`WireError`] when the bytes are not exactly one `mesh-cbor/0` message of this version.
/// Decoding checks the *shape*; the message's own preconditions are [`SyncMessage::check`]'s, so
/// that a receiver can tell a peer that sent nonsense from a peer that sent a well-formed message
/// it must refuse.
pub fn decode_message(bytes: &[u8]) -> Result<SyncMessage, WireError> {
    let mut reader = Reader::new(bytes);
    reader.array_of(2)?;
    let tag = reader.unsigned()?;
    let kind = MessageKind::from_tag(tag).ok_or(WireError::UnknownMessageTag(tag))?;
    let message = read_body(&mut reader, kind)?;
    if reader.remaining() > 0 {
        return Err(WireError::TrailingBytes {
            remaining: reader.remaining(),
        });
    }
    Ok(message)
}

#[allow(clippy::too_many_lines)]
fn read_body(reader: &mut Reader<'_>, kind: MessageKind) -> Result<SyncMessage, WireError> {
    match kind {
        MessageKind::Hello => {
            reader.array_of(4)?;
            Ok(SyncMessage::Hello {
                protocol_version: u32::try_from(reader.unsigned()?)
                    .map_err(|_| WireError::LengthOutOfRange)?,
                encoding_profile: reader.text()?,
                actor: ActorId::from_bytes(reader.identifier()?),
                challenge: reader.identifier()?,
            })
        }
        MessageKind::Authenticate => {
            reader.array_of(3)?;
            Ok(SyncMessage::Authenticate {
                actor: ActorId::from_bytes(reader.identifier()?),
                challenge: reader.identifier()?,
                signature: reader.byte_string()?,
            })
        }
        MessageKind::AdvertiseFrontier => {
            reader.array_of(3)?;
            Ok(SyncMessage::AdvertiseFrontier {
                heads: reader.list(|reader| {
                    reader.array_of(4)?;
                    Ok(HeadAdvertisement {
                        actor: ActorId::from_bytes(reader.identifier()?),
                        head: HeadId::from_bytes(reader.identifier()?),
                        contiguous_through: reader.sequence()?,
                        sparse: read_sparse(reader)?,
                    })
                })?,
                canonical_head: reader.optional_head()?,
                policy_epoch: reader.epoch()?,
            })
        }
        MessageKind::RequestOperations => {
            reader.array_of(4)?;
            Ok(SyncMessage::RequestOperations {
                actor: ActorId::from_bytes(reader.identifier()?),
                from_sequence: reader.sequence()?,
                specific: reader
                    .list(|reader| Ok(ChangeSetId::from_bytes(reader.identifier()?)))?,
                max_count: u32::try_from(reader.unsigned()?)
                    .map_err(|_| WireError::LengthOutOfRange)?,
            })
        }
        MessageKind::OperationsBatch => {
            reader.array_of(1)?;
            Ok(SyncMessage::OperationsBatch {
                changesets: reader.list(read_carried)?,
            })
        }
        MessageKind::AckOperations => {
            reader.array_of(3)?;
            Ok(SyncMessage::AckOperations {
                actor: ActorId::from_bytes(reader.identifier()?),
                contiguous_through: reader.sequence()?,
                sparse: read_sparse(reader)?,
            })
        }
        MessageKind::AdvertiseManifests => {
            reader.array_of(1)?;
            Ok(SyncMessage::AdvertiseManifests {
                manifests: reader
                    .list(|reader| Ok(ManifestId::from_bytes(reader.identifier()?)))?,
            })
        }
        MessageKind::RequestChunks => {
            reader.array_of(1)?;
            Ok(SyncMessage::RequestChunks {
                requests: reader.list(|reader| {
                    reader.array_of(3)?;
                    Ok(ChunkRequest {
                        content: ContentHash::from_bytes(reader.identifier()?),
                        from_offset: reader.unsigned()?,
                        max_bytes: reader.unsigned()?,
                    })
                })?,
            })
        }
        MessageKind::ChunkBatch => {
            reader.array_of(1)?;
            Ok(SyncMessage::ChunkBatch {
                parts: reader.list(|reader| {
                    reader.array_of(4)?;
                    Ok(ChunkPart {
                        content: ContentHash::from_bytes(reader.identifier()?),
                        offset: reader.unsigned()?,
                        bytes: reader.byte_string()?,
                        is_final: reader.boolean()?,
                    })
                })?,
            })
        }
        MessageKind::AckChunks => {
            reader.array_of(1)?;
            Ok(SyncMessage::AckChunks {
                verified: reader
                    .list(|reader| Ok(ContentHash::from_bytes(reader.identifier()?)))?,
            })
        }
        MessageKind::UpdateActorHead => {
            reader.array_of(3)?;
            Ok(SyncMessage::UpdateActorHead {
                actor: ActorId::from_bytes(reader.identifier()?),
                head: HeadId::from_bytes(reader.identifier()?),
                sequence: reader.sequence()?,
            })
        }
        MessageKind::UpdateCanonicalHead => {
            reader.array_of(3)?;
            Ok(SyncMessage::UpdateCanonicalHead {
                head: HeadId::from_bytes(reader.identifier()?),
                policy_epoch: reader.epoch()?,
                receipt: ApprovalId::from_bytes(reader.identifier()?),
            })
        }
        MessageKind::Presence => {
            reader.array_of(3)?;
            let actor = ActorId::from_bytes(reader.identifier()?);
            let tag = reader.unsigned()?;
            Ok(SyncMessage::Presence {
                actor,
                state: PresenceState::from_tag(tag).ok_or(WireError::UnknownPresenceTag(tag))?,
                expires_after_millis: reader.unsigned()?,
            })
        }
        MessageKind::ReviewBundle => {
            reader.array_of(2)?;
            Ok(SyncMessage::ReviewBundle {
                id: ReviewBundleId::from_bytes(reader.identifier()?),
                body: reader.byte_string()?,
            })
        }
        MessageKind::ValidationReceipt => {
            reader.array_of(2)?;
            Ok(SyncMessage::ValidationReceipt {
                about: ChangeSetId::from_bytes(reader.identifier()?),
                body: reader.byte_string()?,
            })
        }
        MessageKind::ApprovalEnvelope => {
            reader.array_of(2)?;
            Ok(SyncMessage::ApprovalEnvelope {
                id: ApprovalId::from_bytes(reader.identifier()?),
                body: reader.byte_string()?,
            })
        }
        MessageKind::AntiEntropySummary => {
            reader.array_of(2)?;
            let actor = ActorId::from_bytes(reader.identifier()?);
            let nodes = reader.list(|reader| {
                reader.array_of(3)?;
                let first = reader.sequence()?;
                let last = reader.sequence()?;
                Ok(SummaryNode::new(first, last, reader.identifier()?))
            })?;
            Ok(SyncMessage::AntiEntropySummary {
                actor,
                summary: MerkleSummary::from_nodes(nodes),
            })
        }
        MessageKind::Error => {
            reader.array_of(3)?;
            let code_tag = reader.unsigned()?;
            let code = ErrorCode::from_tag(code_tag).ok_or(WireError::UnknownErrorTag(code_tag))?;
            let about_tag = reader.unsigned()?;
            let about =
                MessageKind::from_tag(about_tag).ok_or(WireError::UnknownMessageTag(about_tag))?;
            Ok(SyncMessage::Error(ProtocolError::new(
                code,
                about,
                reader.text()?,
            )))
        }
    }
}

fn read_sparse(reader: &mut Reader<'_>) -> Result<Vec<SparseChangeSet>, WireError> {
    reader.list(|reader| {
        reader.array_of(2)?;
        Ok(SparseChangeSet {
            sequence: reader.sequence()?,
            id: ChangeSetId::from_bytes(reader.identifier()?),
        })
    })
}

fn read_carried(reader: &mut Reader<'_>) -> Result<CarriedChangeSet, WireError> {
    reader.array_of(8)?;
    Ok(CarriedChangeSet {
        id: ChangeSetId::from_bytes(reader.identifier()?),
        author: ActorId::from_bytes(reader.identifier()?),
        sequence: reader.sequence()?,
        parents: reader.list(|reader| Ok(ChangeSetId::from_bytes(reader.identifier()?)))?,
        base_head: HeadId::from_bytes(reader.identifier()?),
        resulting_head: HeadId::from_bytes(reader.identifier()?),
        policy_epoch: reader.epoch()?,
        body: reader.byte_string()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_head_is_the_shortest_that_fits() {
        let mut writer = Writer::new();
        writer.unsigned(0);
        writer.unsigned(23);
        writer.unsigned(24);
        writer.unsigned(256);
        writer.unsigned(65_536);
        writer.unsigned(4_294_967_296);
        assert_eq!(
            writer.buffer,
            vec![
                0x00, 0x17, 0x18, 0x18, 0x19, 0x01, 0x00, 0x1a, 0x00, 0x01, 0x00, 0x00, 0x1b, 0x00,
                0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
            ]
        );
    }

    #[test]
    fn a_non_minimal_head_is_refused() {
        // 24 written as a two-byte head where a one-byte head would hold it.
        let mut reader = Reader::new(&[0x18, 0x17]);
        assert_eq!(reader.unsigned(), Err(WireError::NonMinimalHead));
    }

    #[test]
    fn every_minimal_boundary_is_accepted() {
        for value in [
            0u64,
            23,
            24,
            255,
            256,
            65_535,
            65_536,
            u64::from(u32::MAX),
            u64::from(u32::MAX) + 1,
            u64::MAX,
        ] {
            let mut writer = Writer::new();
            writer.unsigned(value);
            let mut reader = Reader::new(&writer.buffer);
            assert_eq!(reader.unsigned(), Ok(value), "{value} did not round trip");
        }
    }

    #[test]
    fn a_wrong_major_type_names_both_types() {
        let mut writer = Writer::new();
        writer.text("no");
        let mut reader = Reader::new(&writer.buffer);
        assert_eq!(
            reader.unsigned(),
            Err(WireError::UnexpectedType {
                expected: MAJOR_UNSIGNED,
                found: MAJOR_TEXT
            })
        );
    }

    #[test]
    fn truncated_bytes_are_refused_rather_than_panicking() {
        for length in 0..12 {
            let message = encode_message(&SyncMessage::UpdateActorHead {
                actor: ActorId::from_bytes([1; 32]),
                head: HeadId::from_bytes([2; 32]),
                sequence: ActorSequence::new(3),
            });
            let truncated = &message[..length.min(message.len())];
            assert!(decode_message(truncated).is_err(), "{length} bytes decoded");
        }
    }

    #[test]
    fn an_identifier_of_the_wrong_width_is_refused() {
        let mut writer = Writer::new();
        writer.bytes(&[1, 2, 3]);
        let mut reader = Reader::new(&writer.buffer);
        assert_eq!(
            reader.identifier(),
            Err(WireError::WrongIdentifierWidth { found: 3 })
        );
    }

    #[test]
    fn a_boolean_is_the_only_simple_value_admitted() {
        let mut reader = Reader::new(&[0xf6]);
        assert_eq!(reader.boolean(), Err(WireError::NotABoolean));
    }

    #[test]
    fn an_unknown_message_tag_refuses_with_unknown_message() {
        let mut writer = Writer::new();
        writer.array(2);
        writer.unsigned(99);
        writer.array(0);
        let error = decode_message(&writer.buffer).expect_err("tag 99 is undefined");
        assert_eq!(error, WireError::UnknownMessageTag(99));
        assert_eq!(error.refusal().code(), ErrorCode::UnknownMessage);
    }

    #[test]
    fn every_other_decoding_failure_refuses_with_malformed_message() {
        assert_eq!(
            WireError::Truncated.refusal().code(),
            ErrorCode::MalformedMessage
        );
        assert_eq!(
            WireError::NonMinimalHead.refusal().code(),
            ErrorCode::MalformedMessage
        );
    }

    #[test]
    fn trailing_bytes_are_refused() {
        let mut bytes = encode_message(&SyncMessage::AckChunks {
            verified: Vec::new(),
        });
        bytes.push(0x00);
        assert_eq!(
            decode_message(&bytes),
            Err(WireError::TrailingBytes { remaining: 1 })
        );
    }

    #[test]
    fn a_length_that_lies_is_refused_rather_than_reserving_memory() {
        let mut writer = Writer::new();
        writer.array(2);
        writer.unsigned(MessageKind::AckChunks.tag());
        writer.array(1);
        writer.head(MAJOR_ARRAY, u64::MAX);
        assert!(decode_message(&writer.buffer).is_err());
    }

    #[test]
    fn a_body_of_the_wrong_arity_is_refused() {
        let mut writer = Writer::new();
        writer.array(2);
        writer.unsigned(MessageKind::UpdateActorHead.tag());
        writer.array(2);
        writer.bytes(&[1; 32]);
        writer.bytes(&[2; 32]);
        assert_eq!(
            decode_message(&writer.buffer),
            Err(WireError::WrongArity {
                expected: 3,
                found: 2
            })
        );
    }

    #[test]
    fn an_optional_head_of_arity_two_is_refused() {
        let mut reader = Reader::new(&[0x82, 0x58, 0x20]);
        assert_eq!(
            reader.optional_head(),
            Err(WireError::WrongArity {
                expected: 1,
                found: 2
            })
        );
    }

    #[test]
    fn the_wire_format_is_named_and_versioned() {
        assert_eq!(WIRE_FORMAT, "mesh-cwp-wire/0");
    }
}
