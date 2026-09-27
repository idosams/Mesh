//! The message set the desktop client and the daemon exchange, and its framing.
//!
//! # Framing
//!
//! One message per line, UTF-8, `\n`-terminated, at most [`MAX_LINE_BYTES`] bytes including the
//! newline. A line longer than that is refused rather than buffered: the reader is on the
//! untrusted side of a boundary and an unbounded buffer is how a local client turns into a local
//! denial of service.
//!
//! # Key order is part of the contract
//!
//! Every message below declares a fixed key order, and [`crate::ipc::json`] writes objects in
//! insertion order with no whitespace. One message therefore has exactly one encoding, which is
//! what makes the shared corpus in `crates/mesh-daemon/ipc-contract.json` comparable byte for byte
//! against the JavaScript client. This is *not* the canonical encoding of plan §4 — that is CBOR,
//! it lives in `mesh-types`, and nothing here is signed. This is a local transport format, and
//! saying so here is cheaper than somebody later assuming the two are the same thing.
//!
//! # Correlation, not ordering
//!
//! `id` correlates a reply with its call. It is a per-connection counter and it is **not** an
//! ordering: ordering in this program is `lamport → event id → content hash` and nothing on this
//! socket establishes any of the three.

use core::fmt;

use crate::ipc::json::{Json, JsonError};

/// The protocol name both ends announce, so a socket that is something else fails fast.
pub const PROTOCOL: &str = "mesh-ipc";

/// The version of the IPC surface this daemon implements.
///
/// Version 2 added workspace reads and pushed events. Version 3 adds bounded local-folder import,
/// receipt-owned rollback, and read-only restore preview. Version 4 adds a read-only performance
/// counter snapshot. Version 5 adds whole-workspace historical forks into new native folders.
/// Version 6 adds daemon-computed first-publication reviews with no caller-supplied bundle ID.
/// Version 7 adds bounded multi-frame replies while retaining the per-line denial-of-service
/// bound.
/// Version 8 adds scoped fleet agent calls; it does not broaden ordinary IPC session authority.
/// Older clients never negotiate the newer calls, and every earlier method keeps its existing
/// meaning.
pub const SURFACE_VERSION: u32 = 8;

/// Every surface version this daemon can still speak, newest last.
pub const SUPPORTED_VERSIONS: &[u32] = &[1, 2, 3, 4, 5, 6, 7, 8];

/// The largest line the reader will accept, newline included.
pub const MAX_LINE_BYTES: usize = 65_536;

/// The largest complete daemon message reconstructed from bounded frames.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// Raw bytes carried by one hex-encoded chunk. The resulting JSON line stays below 64 KiB.
pub const CHUNK_DATA_BYTES: usize = 30_000;

/// The longest a method name may be. Names are catalogue entries, not free text.
pub const MAX_METHOD_BYTES: usize = 64;

/// The longest a client-chosen session name may be.
pub const MAX_SESSION_BYTES: usize = 128;

/// What the desktop client sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientMessage {
    /// Open the conversation and negotiate a surface version.
    ///
    /// Key order: `t`, `id`, `protocol`, `versions`, `session`.
    Hello {
        /// Correlation identifier for the reply.
        id: u64,
        /// Every surface version the client can speak.
        versions: Vec<u32>,
        /// The client's own session name, so a reconnection can be recognised as one.
        session: String,
    },
    /// Invoke one catalogue method.
    ///
    /// Key order: `t`, `id`, `method`, `version`, `params`.
    Call {
        /// Correlation identifier for the reply.
        id: u64,
        /// The catalogue method name.
        method: String,
        /// The surface version the client believes it is calling.
        version: u32,
        /// The method's parameters, always an object.
        params: Json,
    },
}

impl ClientMessage {
    /// The correlation identifier this message expects its reply to carry.
    #[must_use]
    pub const fn id(&self) -> u64 {
        match self {
            Self::Hello { id, .. } | Self::Call { id, .. } => *id,
        }
    }

    /// This message as one line, with no trailing newline.
    #[must_use]
    pub fn encode(&self) -> String {
        match self {
            Self::Hello {
                id,
                versions,
                session,
            } => Json::object([
                ("t", Json::text("hello")),
                ("id", Json::Number(*id)),
                ("protocol", Json::text(PROTOCOL)),
                (
                    "versions",
                    Json::Array(
                        versions
                            .iter()
                            .map(|v| Json::Number(u64::from(*v)))
                            .collect(),
                    ),
                ),
                ("session", Json::text(session.clone())),
            ]),
            Self::Call {
                id,
                method,
                version,
                params,
            } => Json::object([
                ("t", Json::text("call")),
                ("id", Json::Number(*id)),
                ("method", Json::text(method.clone())),
                ("version", Json::Number(u64::from(*version))),
                ("params", params.clone()),
            ]),
        }
        .encode()
    }

    /// Read one message from a line.
    ///
    /// # Errors
    ///
    /// Returns [`WireError`] when the line is not JSON, is not one of the two shapes above, or
    /// carries a field outside its declared bounds.
    pub fn decode(line: &str) -> Result<Self, WireError> {
        if line.len() >= MAX_LINE_BYTES {
            return Err(WireError::TooLong { bytes: line.len() });
        }
        let value = Json::parse(line).map_err(WireError::NotJson)?;
        let tag = field_text(&value, "t")?;
        let id = field_u64(&value, "id")?;
        let decoded = match tag {
            "hello" => {
                let protocol = field_text(&value, "protocol")?;
                if protocol != PROTOCOL {
                    return Err(WireError::Malformed {
                        what: "protocol".to_owned(),
                        why: format!("expected `{PROTOCOL}`, got `{protocol}`"),
                    });
                }
                let versions = field_versions(&value, "versions")?;
                let session =
                    bounded(field_text(&value, "session")?, "session", MAX_SESSION_BYTES)?;
                Self::Hello {
                    id,
                    versions,
                    session,
                }
            }
            "call" => {
                let method = bounded(field_text(&value, "method")?, "method", MAX_METHOD_BYTES)?;
                let version = field_version(&value, "version")?;
                let params = field_object(&value, "params")?;
                Self::Call {
                    id,
                    method,
                    version,
                    params,
                }
            }
            other => {
                return Err(WireError::UnknownKind {
                    tag: other.to_owned(),
                })
            }
        };
        require_exact_wire_encoding(line, &decoded.encode())?;
        Ok(decoded)
    }
}

/// What the daemon sends back. One logical reply may occupy multiple surface-v7 chunk frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DaemonMessage {
    /// The negotiated answer to a [`ClientMessage::Hello`].
    ///
    /// Key order: `t`, `id`, `version`, `session`, `resumed`, `surface_version`.
    Welcome {
        /// The correlation identifier of the `hello`.
        id: u64,
        /// The version both ends will use for the rest of the connection.
        version: u32,
        /// The session name, echoed so the client can be sure it was understood.
        session: String,
        /// Whether **this daemon process** already knew this session name.
        ///
        /// `false` after a daemon restart, always: the registry is in memory. The client's own
        /// context does not depend on this flag — see `apps/desktop/src/ipc/client.ts`.
        resumed: bool,
        /// The newest version this daemon implements, so a client can tell it is behind.
        surface_version: u32,
    },
    /// The conversation cannot proceed: no shared version, or a second `hello`.
    ///
    /// Key order: `t`, `id`, `code`, `message`, `supported`.
    Refused {
        /// The correlation identifier of the message being refused.
        id: u64,
        /// A stable machine code.
        code: String,
        /// A sentence for a person.
        message: String,
        /// Every version this daemon supports.
        supported: Vec<u32>,
    },
    /// A method answered.
    ///
    /// Key order: `t`, `id`, `value`.
    Result {
        /// The correlation identifier of the call.
        id: u64,
        /// The answer, always an object.
        value: Json,
    },
    /// A method did not answer.
    ///
    /// Key order: `t`, `id`, `code`, `message`.
    Failed {
        /// The correlation identifier of the call.
        id: u64,
        /// A stable machine code.
        code: String,
        /// A sentence for a person.
        message: String,
    },
    /// Something happened that a subscribed client asked to hear about.
    ///
    /// Key order: `t`, `id`, `sequence`, `kind`, `value`.
    ///
    /// The **only** line the daemon sends that no client message asked for, and it is sent solely
    /// to a connection that called `events.subscribe`. `id` is that subscription's identifier, so
    /// a client correlates a push with the request that opened the stream rather than having to
    /// guess. `sequence` orders entries *on this socket* and nothing else — ordering of work in
    /// this program is `lamport → event id → content hash`, and a feed sequence establishes none
    /// of those.
    Event {
        /// The identifier of the `events.subscribe` call this stream belongs to.
        id: u64,
        /// Where this entry sits in the daemon's feed.
        sequence: u64,
        /// The stable wire word for what happened.
        kind: String,
        /// The entry's own fields, always an object.
        value: Json,
    },
    /// One transport frame of a larger encoded daemon message (surface v7 and newer).
    ///
    /// Key order: `t`, `id`, `index`, `parts`, `total_bytes`, `hex`.
    Chunk {
        /// Correlation identifier copied from the complete message.
        id: u64,
        /// Zero-based frame index.
        index: u64,
        /// Total number of frames.
        parts: u64,
        /// Exact byte length of the complete encoded message.
        total_bytes: u64,
        /// Lower-case hexadecimal bytes for this frame.
        hex: String,
    },
}

impl DaemonMessage {
    /// The correlation identifier this reply carries.
    #[must_use]
    pub const fn id(&self) -> u64 {
        match self {
            Self::Welcome { id, .. }
            | Self::Refused { id, .. }
            | Self::Result { id, .. }
            | Self::Failed { id, .. }
            | Self::Event { id, .. }
            | Self::Chunk { id, .. } => *id,
        }
    }

    /// Whether this line answers a request, as opposed to being pushed.
    ///
    /// A client uses this to keep its own bookkeeping straight: a pushed line must not be matched
    /// against the call table and must not clear an in-flight entry.
    #[must_use]
    pub const fn is_reply(&self) -> bool {
        !matches!(self, Self::Event { .. } | Self::Chunk { .. })
    }

    /// This message as one line, with no trailing newline.
    #[must_use]
    pub fn encode(&self) -> String {
        match self {
            Self::Welcome {
                id,
                version,
                session,
                resumed,
                surface_version,
            } => Json::object([
                ("t", Json::text("welcome")),
                ("id", Json::Number(*id)),
                ("version", Json::Number(u64::from(*version))),
                ("session", Json::text(session.clone())),
                ("resumed", Json::Bool(*resumed)),
                ("surface_version", Json::Number(u64::from(*surface_version))),
            ]),
            Self::Refused {
                id,
                code,
                message,
                supported,
            } => Json::object([
                ("t", Json::text("refused")),
                ("id", Json::Number(*id)),
                ("code", Json::text(code.clone())),
                ("message", Json::text(message.clone())),
                (
                    "supported",
                    Json::Array(
                        supported
                            .iter()
                            .map(|v| Json::Number(u64::from(*v)))
                            .collect(),
                    ),
                ),
            ]),
            Self::Result { id, value } => Json::object([
                ("t", Json::text("result")),
                ("id", Json::Number(*id)),
                ("value", value.clone()),
            ]),
            Self::Failed { id, code, message } => Json::object([
                ("t", Json::text("failed")),
                ("id", Json::Number(*id)),
                ("code", Json::text(code.clone())),
                ("message", Json::text(message.clone())),
            ]),
            Self::Event {
                id,
                sequence,
                kind,
                value,
            } => Json::object([
                ("t", Json::text("event")),
                ("id", Json::Number(*id)),
                ("sequence", Json::Number(*sequence)),
                ("kind", Json::text(kind.clone())),
                ("value", value.clone()),
            ]),
            Self::Chunk {
                id,
                index,
                parts,
                total_bytes,
                hex,
            } => Json::object([
                ("t", Json::text("chunk")),
                ("id", Json::Number(*id)),
                ("index", Json::Number(*index)),
                ("parts", Json::Number(*parts)),
                ("total_bytes", Json::Number(*total_bytes)),
                ("hex", Json::text(hex.clone())),
            ]),
        }
        .encode()
    }

    /// Read one reply from a line.
    ///
    /// # Errors
    ///
    /// Returns [`WireError`] on anything that is not one of the shapes above.
    pub fn decode(line: &str) -> Result<Self, WireError> {
        Self::decode_with_bound(line, MAX_LINE_BYTES)
    }

    fn decode_with_bound(line: &str, bound: usize) -> Result<Self, WireError> {
        if line.len() >= bound {
            return Err(WireError::TooLong { bytes: line.len() });
        }
        let value = Json::parse(line).map_err(WireError::NotJson)?;
        let tag = field_text(&value, "t")?;
        let id = field_u64(&value, "id")?;
        let decoded = match tag {
            "welcome" => Self::Welcome {
                id,
                version: field_version(&value, "version")?,
                session: field_text(&value, "session")?.to_owned(),
                resumed: value
                    .get("resumed")
                    .and_then(Json::as_bool)
                    .ok_or_else(|| WireError::Malformed {
                        what: "resumed".to_owned(),
                        why: "must be a boolean".to_owned(),
                    })?,
                surface_version: field_version(&value, "surface_version")?,
            },
            "refused" => Self::Refused {
                id,
                code: field_text(&value, "code")?.to_owned(),
                message: field_text(&value, "message")?.to_owned(),
                supported: field_versions(&value, "supported")?,
            },
            "result" => {
                let answer = field_object(&value, "value")?;
                Self::Result { id, value: answer }
            }
            "failed" => Self::Failed {
                id,
                code: field_text(&value, "code")?.to_owned(),
                message: field_text(&value, "message")?.to_owned(),
            },
            "event" => {
                let body = field_object(&value, "value")?;
                Self::Event {
                    id,
                    sequence: field_u64(&value, "sequence")?,
                    kind: bounded(field_text(&value, "kind")?, "kind", MAX_METHOD_BYTES)?,
                    value: body,
                }
            }
            "chunk" => Self::Chunk {
                id,
                index: field_u64(&value, "index")?,
                parts: field_u64(&value, "parts")?,
                total_bytes: field_u64(&value, "total_bytes")?,
                hex: field_text(&value, "hex")?.to_owned(),
            },
            other => {
                return Err(WireError::UnknownKind {
                    tag: other.to_owned(),
                })
            }
        };
        require_exact_wire_encoding(line, &decoded.encode())?;
        Ok(decoded)
    }
}

/// Encode one logical daemon message into safe wire frames for `negotiated`.
#[must_use]
pub fn daemon_frames(message: &DaemonMessage, negotiated: Option<u32>) -> Vec<DaemonMessage> {
    let encoded = message.encode();
    if encoded.len() < MAX_LINE_BYTES {
        return vec![message.clone()];
    }
    if negotiated.is_none_or(|version| version < 7) {
        return vec![DaemonMessage::Failed {
            id: message.id(),
            code: "response-too-large".to_owned(),
            message: "This response is too large for the negotiated local IPC surface. Update Mesh and try again. Nothing was changed.".to_owned(),
        }];
    }
    if encoded.len() > MAX_MESSAGE_BYTES {
        return vec![DaemonMessage::Failed {
            id: message.id(),
            code: "response-too-large".to_owned(),
            message: "This workspace view exceeds Mesh's 16 MiB safe local response bound. Use a smaller workspace until paged workspace inspection is available. Nothing was changed.".to_owned(),
        }];
    }
    let parts = encoded.len().div_ceil(CHUNK_DATA_BYTES);
    encoded
        .as_bytes()
        .chunks(CHUNK_DATA_BYTES)
        .enumerate()
        .map(|(index, bytes)| DaemonMessage::Chunk {
            id: message.id(),
            index: u64::try_from(index).expect("frame index fits u64"),
            parts: u64::try_from(parts).expect("frame count fits u64"),
            total_bytes: u64::try_from(encoded.len()).expect("message length fits u64"),
            hex: hex_encode(bytes),
        })
        .collect()
}

/// Fail-closed assembler for surface-v7 daemon chunks.
#[derive(Debug, Default)]
pub struct ChunkAssembler {
    active: Option<PartialMessage>,
}

#[derive(Debug)]
struct PartialMessage {
    id: u64,
    parts: u64,
    total_bytes: usize,
    next: u64,
    bytes: Vec<u8>,
}

impl ChunkAssembler {
    /// Admit one decoded frame, returning a complete logical message when available.
    pub fn push(&mut self, frame: DaemonMessage) -> Result<Option<DaemonMessage>, WireError> {
        let DaemonMessage::Chunk {
            id,
            index,
            parts,
            total_bytes,
            hex,
        } = frame
        else {
            if self.active.is_some() {
                return Err(malformed_chunk(
                    "a non-chunk interrupted an incomplete response",
                ));
            }
            return Ok(Some(frame));
        };
        let total_bytes = usize::try_from(total_bytes)
            .map_err(|_| malformed_chunk("total_bytes does not fit this process"))?;
        if parts == 0 || !(MAX_LINE_BYTES..=MAX_MESSAGE_BYTES).contains(&total_bytes) {
            return Err(malformed_chunk("the declared message bounds are invalid"));
        }
        let bytes = hex_decode(&hex)?;
        if bytes.is_empty() || bytes.len() > CHUNK_DATA_BYTES {
            return Err(malformed_chunk("the chunk payload length is invalid"));
        }
        if self.active.is_none() {
            if index != 0 {
                return Err(malformed_chunk("the first chunk index is not zero"));
            }
            self.active = Some(PartialMessage {
                id,
                parts,
                total_bytes,
                next: 0,
                bytes: Vec::with_capacity(total_bytes),
            });
        }
        let active = self.active.as_mut().expect("initialized above");
        if active.id != id
            || active.parts != parts
            || active.total_bytes != total_bytes
            || active.next != index
            || index >= parts
            || active.bytes.len().saturating_add(bytes.len()) > total_bytes
        {
            self.active = None;
            return Err(malformed_chunk("chunk identity, order, or length changed"));
        }
        active.bytes.extend_from_slice(&bytes);
        active.next += 1;
        if active.next != parts {
            return Ok(None);
        }
        let complete = self.active.take().expect("active response");
        if complete.bytes.len() != complete.total_bytes {
            return Err(malformed_chunk(
                "the final byte length does not match total_bytes",
            ));
        }
        let line = String::from_utf8(complete.bytes)
            .map_err(|_| malformed_chunk("the reconstructed response is not UTF-8"))?;
        let message = DaemonMessage::decode_with_bound(&line, MAX_MESSAGE_BYTES + 1)?;
        if matches!(message, DaemonMessage::Chunk { .. }) || message.id() != complete.id {
            return Err(malformed_chunk(
                "the reconstructed response identity is invalid",
            ));
        }
        Ok(Some(message))
    }
}

fn malformed_chunk(why: &str) -> WireError {
    WireError::Malformed {
        what: "chunk".to_owned(),
        why: why.to_owned(),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn hex_decode(text: &str) -> Result<Vec<u8>, WireError> {
    if text.len() % 2 != 0 {
        return Err(malformed_chunk("hex has an odd number of digits"));
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high =
                hex_digit(pair[0]).ok_or_else(|| malformed_chunk("hex is not lower-case"))?;
            let low = hex_digit(pair[1]).ok_or_else(|| malformed_chunk("hex is not lower-case"))?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// Why a line could not be read as a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireError {
    /// The line is not a value in the JSON subset.
    NotJson(JsonError),
    /// The line is longer than [`MAX_LINE_BYTES`].
    TooLong {
        /// How long it was.
        bytes: usize,
    },
    /// The `t` field names no message this surface has.
    UnknownKind {
        /// The tag that was sent.
        tag: String,
    },
    /// A field is missing, of the wrong type, or outside its bounds.
    Malformed {
        /// Which field.
        what: String,
        /// What was expected.
        why: String,
    },
}

impl WireError {
    /// The stable machine code a [`DaemonMessage::Failed`] carries for this error.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotJson(_) => "not-json",
            Self::TooLong { .. } => "line-too-long",
            Self::UnknownKind { .. } => "unknown-message",
            Self::Malformed { .. } => "malformed-message",
        }
    }
}

impl fmt::Display for WireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotJson(error) => write!(formatter, "the line is not JSON: {error}"),
            Self::TooLong { bytes } => write!(
                formatter,
                "the line is {bytes} bytes, over the {MAX_LINE_BYTES} byte limit"
            ),
            Self::UnknownKind { tag } => {
                write!(formatter, "`{tag}` is not a message on this surface")
            }
            Self::Malformed { what, why } => write!(formatter, "`{what}` {why}"),
        }
    }
}

impl std::error::Error for WireError {}

fn field_text<'a>(value: &'a Json, key: &str) -> Result<&'a str, WireError> {
    value
        .get(key)
        .and_then(Json::as_text)
        .ok_or_else(|| WireError::Malformed {
            what: key.to_owned(),
            why: "must be a string".to_owned(),
        })
}

fn field_u64(value: &Json, key: &str) -> Result<u64, WireError> {
    value
        .get(key)
        .and_then(Json::as_u64)
        .ok_or_else(|| WireError::Malformed {
            what: key.to_owned(),
            why: "must be a non-negative integer".to_owned(),
        })
}

fn field_object(value: &Json, key: &str) -> Result<Json, WireError> {
    value
        .get(key)
        .filter(|field| field.is_object())
        .cloned()
        .ok_or_else(|| WireError::Malformed {
            what: key.to_owned(),
            why: "must be an object".to_owned(),
        })
}

fn field_version(value: &Json, key: &str) -> Result<u32, WireError> {
    u32::try_from(field_u64(value, key)?).map_err(|_| WireError::Malformed {
        what: key.to_owned(),
        why: "must fit in 32 unsigned bits".to_owned(),
    })
}

fn field_versions(value: &Json, key: &str) -> Result<Vec<u32>, WireError> {
    let malformed = |why: &str| WireError::Malformed {
        what: key.to_owned(),
        why: why.to_owned(),
    };
    let values = value
        .get(key)
        .and_then(Json::as_array)
        .ok_or_else(|| malformed("must be an array of versions"))?;
    if values.is_empty() {
        return Err(malformed("must name at least one version"));
    }
    if values.len() > 32 {
        return Err(malformed("must name at most 32 versions"));
    }
    values
        .iter()
        .map(|entry| {
            entry
                .as_u64()
                .and_then(|number| u32::try_from(number).ok())
                .ok_or_else(|| malformed("every entry must be a version number"))
        })
        .collect()
}

fn bounded(value: &str, what: &str, limit: usize) -> Result<String, WireError> {
    if value.is_empty() {
        return Err(WireError::Malformed {
            what: what.to_owned(),
            why: "must not be empty".to_owned(),
        });
    }
    if value.len() > limit {
        return Err(WireError::Malformed {
            what: what.to_owned(),
            why: format!("must be at most {limit} bytes"),
        });
    }
    Ok(value.to_owned())
}

fn require_exact_wire_encoding(line: &str, encoded: &str) -> Result<(), WireError> {
    if line != encoded {
        return Err(WireError::Malformed {
            what: "message".to_owned(),
            why: "must use the one published key order and JSON spelling".to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_round_trips_with_the_declared_key_order() {
        let hello = ClientMessage::Hello {
            id: 1,
            versions: vec![1],
            session: "w16".to_owned(),
        };
        assert_eq!(
            hello.encode(),
            r#"{"t":"hello","id":1,"protocol":"mesh-ipc","versions":[1],"session":"w16"}"#
        );
        assert_eq!(ClientMessage::decode(&hello.encode()), Ok(hello));
    }

    #[test]
    fn call_round_trips_with_the_declared_key_order() {
        let call = ClientMessage::Call {
            id: 2,
            method: "daemon.status".to_owned(),
            version: 1,
            params: Json::empty_object(),
        };
        assert_eq!(
            call.encode(),
            r#"{"t":"call","id":2,"method":"daemon.status","version":1,"params":{}}"#
        );
        assert_eq!(ClientMessage::decode(&call.encode()), Ok(call));
    }

    #[test]
    fn every_daemon_reply_round_trips() {
        let replies = [
            DaemonMessage::Welcome {
                id: 1,
                version: 1,
                session: "w16".to_owned(),
                resumed: false,
                surface_version: SURFACE_VERSION,
            },
            DaemonMessage::Refused {
                id: 1,
                code: "unsupported-version".to_owned(),
                message: "no shared version".to_owned(),
                supported: vec![1],
            },
            DaemonMessage::Result {
                id: 3,
                value: Json::object([("serving", Json::Bool(true))]),
            },
            DaemonMessage::Failed {
                id: 4,
                code: "unknown-method".to_owned(),
                message: "no such method".to_owned(),
            },
        ];
        for reply in replies {
            assert_eq!(DaemonMessage::decode(&reply.encode()), Ok(reply.clone()));
            assert!(!reply.encode().contains('\n'), "a message spans one line");
        }
    }

    #[test]
    fn a_large_reply_round_trips_through_bounded_ordered_frames() {
        let reply = DaemonMessage::Result {
            id: 9,
            value: Json::object([("payload", Json::text("x".repeat(MAX_LINE_BYTES * 2)))]),
        };
        let frames = daemon_frames(&reply, Some(7));
        assert!(frames.len() > 1);
        assert!(frames
            .iter()
            .all(|frame| frame.encode().len() < MAX_LINE_BYTES));
        let mut assembler = ChunkAssembler::default();
        let mut complete = None;
        for frame in frames {
            complete = assembler.push(frame).expect("valid frame");
        }
        assert_eq!(complete, Some(reply));
    }

    #[test]
    fn large_reply_frames_fail_closed_on_old_surface_and_reordering() {
        let reply = DaemonMessage::Result {
            id: 9,
            value: Json::object([("payload", Json::text("x".repeat(MAX_LINE_BYTES)))]),
        };
        assert!(matches!(
            daemon_frames(&reply, Some(6)).as_slice(),
            [DaemonMessage::Failed { code, message, .. }]
                if code == "response-too-large" && message.contains("Update Mesh")
        ));

        let oversized = DaemonMessage::Result {
            id: 10,
            value: Json::object([("payload", Json::text("x".repeat(MAX_MESSAGE_BYTES)))]),
        };
        assert!(matches!(
            daemon_frames(&oversized, Some(7)).as_slice(),
            [DaemonMessage::Failed { code, message, .. }]
                if code == "response-too-large"
                    && message.contains("16 MiB safe local response bound")
                    && !message.contains("Update Mesh")
        ));

        let mut frames = daemon_frames(&reply, Some(7));
        frames.swap(0, 1);
        let mut assembler = ChunkAssembler::default();
        assert!(assembler.push(frames.remove(0)).is_err());
    }

    #[test]
    fn refuses_a_protocol_that_is_not_ours() {
        let line =
            r#"{"t":"hello","id":1,"protocol":"something-else","versions":[1],"session":"a"}"#;
        assert!(matches!(
            ClientMessage::decode(line),
            Err(WireError::Malformed { .. })
        ));
    }

    #[test]
    fn refuses_fields_outside_their_bounds() {
        let long_method = "m".repeat(MAX_METHOD_BYTES + 1);
        let cases = [
            format!(r#"{{"t":"call","id":1,"method":"{long_method}","version":1,"params":{{}}}}"#),
            r#"{"t":"call","id":1,"method":"","version":1,"params":{}}"#.to_owned(),
            r#"{"t":"call","id":1,"method":"a","version":1,"params":[]}"#.to_owned(),
            r#"{"t":"hello","id":1,"protocol":"mesh-ipc","versions":[],"session":"a"}"#.to_owned(),
            r#"{"t":"hello","id":1,"protocol":"mesh-ipc","versions":[1],"session":""}"#.to_owned(),
        ];
        for line in cases {
            assert!(
                matches!(
                    ClientMessage::decode(&line),
                    Err(WireError::Malformed { .. })
                ),
                "accepted: {line}"
            );
        }
    }

    #[test]
    fn refuses_a_line_over_the_limit() {
        let line = format!(
            r#"{{"t":"call","id":1,"method":"a","version":1,"params":{{"p":"{}"}}}}"#,
            "x".repeat(MAX_LINE_BYTES)
        );
        assert_eq!(
            ClientMessage::decode(&line),
            Err(WireError::TooLong { bytes: line.len() })
        );
    }

    #[test]
    fn refuses_a_tag_it_does_not_know() {
        assert!(matches!(
            ClientMessage::decode(r#"{"t":"shutdown","id":1}"#),
            Err(WireError::UnknownKind { .. })
        ));
    }

    #[test]
    fn required_object_fields_are_not_synthesized_when_the_wire_omits_them() {
        let call = r#"{"t":"call","id":1,"method":"daemon.status","version":1}"#;
        assert!(
            matches!(
                ClientMessage::decode(call),
                Err(WireError::Malformed { .. })
            ),
            "accepted a call after manufacturing missing `params`"
        );

        for reply in [
            r#"{"t":"result","id":1}"#,
            r#"{"t":"event","id":1,"sequence":1,"kind":"serving"}"#,
        ] {
            assert!(
                matches!(
                    DaemonMessage::decode(reply),
                    Err(WireError::Malformed { .. })
                ),
                "accepted a reply after manufacturing missing `value`: {reply}"
            );
        }
    }

    #[test]
    fn refuses_noncanonical_outer_message_encodings() {
        for line in [
            r#"{ "t":"call","id":1,"method":"daemon.status","version":1,"params":{} }"#,
            r#"{"id":1,"t":"call","method":"daemon.status","version":1,"params":{}}"#,
            r#"{"t":"call","id":1,"method":"daemon.status","version":1,"params":{},"shadow":true}"#,
            r#"{"t":"call","id":1,"method":"daemon.\u0073tatus","version":1,"params":{}}"#,
        ] {
            assert!(
                matches!(
                    ClientMessage::decode(line),
                    Err(WireError::Malformed { ref what, .. }) if what == "message"
                ),
                "accepted a second client-message encoding: {line}"
            );
        }

        for line in [
            r#"{ "t":"failed","id":1,"code":"x","message":"m" }"#,
            r#"{"id":1,"t":"failed","code":"x","message":"m"}"#,
            r#"{"t":"failed","id":1,"code":"x","message":"m","shadow":true}"#,
            r#"{"t":"failed","id":1,"code":"\u0078","message":"m"}"#,
        ] {
            assert!(
                matches!(
                    DaemonMessage::decode(line),
                    Err(WireError::Malformed { ref what, .. }) if what == "message"
                ),
                "accepted a second daemon-message encoding: {line}"
            );
        }
    }

    #[test]
    fn every_wire_error_has_a_code_and_a_sentence() {
        let errors = [
            ClientMessage::decode("not json").unwrap_err(),
            ClientMessage::decode(&format!("\"{}\"", "x".repeat(MAX_LINE_BYTES))).unwrap_err(),
            ClientMessage::decode(r#"{"t":"nope","id":1}"#).unwrap_err(),
            ClientMessage::decode(r#"{"t":"call","id":1}"#).unwrap_err(),
        ];
        for error in errors {
            assert!(!error.code().is_empty());
            assert!(!error.to_string().is_empty());
        }
    }
}
