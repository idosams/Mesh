use super::*;
use mesh_cas::Digest32;
use mesh_types::Signature;

/// Closed native broker control commands. No Debug implementation: signatures are not diagnostics.
/// Request IDs correlate replies within this serial connection; they never recreate durable grants.
pub enum RemoteReceivingCommand {
    /// Respond to this connection's fresh coordinator challenge.
    Authenticate {
        /// Bounded request identity.
        request: String,
        /// Signature from the independently configured coordinator.
        signature: Signature,
    },
    /// Inspect one declared chunk's confirmed durable offset after authentication.
    Status {
        /// Bounded request identity.
        request: String,
        /// Declared content digest, never a path.
        digest: Digest32,
    },
    /// Consume the original input reservation, after complete verification.
    Materialize {
        /// Bounded request identity.
        request: String,
    },
}
impl RemoteReceivingCommand {
    pub(super) fn request(&self) -> &str {
        match self {
            Self::Authenticate { request, .. }
            | Self::Status { request, .. }
            | Self::Materialize { request } => request,
        }
    }

    /// Produce a bounded canonical control frame. Invalid native request IDs refuse before I/O.
    pub fn frame(&self) -> io::Result<RemoteFrame> {
        let (request, operation, extra) = match self {
            Self::Authenticate { request, signature } => (
                request,
                "authenticate",
                Some(("signature", Json::text(hex(signature.as_bytes())))),
            ),
            Self::Status { request, digest } => (
                request,
                "status",
                Some(("digest", Json::text(digest.to_hex()))),
            ),
            Self::Materialize { request } => (request, "materialize", None),
        };
        if super::super::id_valid(request).is_err() {
            return Err(refused());
        }
        let mut fields = vec![
            ("schema", Json::text("mesh.receiving-command/v1")),
            ("operation", Json::text(operation)),
            ("request", Json::text(request)),
        ];
        if let Some(extra) = extra {
            fields.push(extra);
        }
        let bytes = Json::object(fields).encode().into_bytes();
        if bytes.len() > super::super::remote_transport::MAX_CONTROL_BYTES {
            return Err(refused());
        }
        Ok(RemoteFrame::Control(bytes))
    }

    /// Parse only bounded, canonical, exact schema/fields. Unknown commands, versions, fields,
    /// uppercase signature encodings and noncanonical JSON refuse without executing anything.
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > super::super::remote_transport::MAX_CONTROL_BYTES {
            return Err(refused());
        }
        let text = std::str::from_utf8(bytes).map_err(|_| refused())?;
        let value = Json::parse(text).map_err(|_| refused())?;
        let get = |key| value.get(key).and_then(Json::as_text).ok_or_else(refused);
        let request = get("request")?.to_owned();
        let command = match get("operation")? {
            "authenticate" => {
                let text = get("signature")?;
                if text.len() != 128 {
                    return Err(refused());
                }
                let mut signature = [0u8; 64];
                for (target, pair) in signature.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
                    let digit = |byte| match byte {
                        b'0'..=b'9' => Ok(byte - b'0'),
                        b'a'..=b'f' => Ok(byte - b'a' + 10),
                        _ => Err(refused()),
                    };
                    *target = digit(pair[0])? * 16 + digit(pair[1])?;
                }
                Self::Authenticate {
                    request,
                    signature: Signature::from_bytes(signature),
                }
            }
            "status" => Self::Status {
                request,
                digest: Digest32::parse_hex(get("digest")?).map_err(|_| refused())?,
            },
            "materialize" => Self::Materialize { request },
            _ => return Err(refused()),
        };
        let RemoteFrame::Control(canonical) = command.frame()? else {
            unreachable!()
        };
        if canonical != bytes {
            return Err(refused());
        }
        Ok(command)
    }
}
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
