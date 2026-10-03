//! One bounded broker connection borrowing a native supervisor's fixed receiving session.
use super::{
    RemoteAdmissionReceipt, RemoteAdmissionRegistry, RemoteFrame, RemoteFrameReader,
    RemoteFrameWriter, RemoteInputAllocation, RemoteReceivingAccess, RemoteReceivingProgress,
    RemoteReceivingSession,
};
use crate::ipc::Json;
use std::io::{self, Read, Write};
mod command;
pub use command::RemoteReceivingCommand;

const MAX_CONNECTION_FRAMES: usize = 131_072;
const MAX_CONTROL_REQUESTS: usize = 32_768;
const MAX_CONNECTION_BYTES: u64 = 2_147_483_648 + 16_777_216;

/// Native handoff retained even when the peer loses the final acknowledgment. Not wire data.
/// The supervisor must keep or reconcile this value; a connection cannot reconstruct it from a reply.
pub struct RemoteReceivedHandoff {
    /// Verified independent allocation, ready for native workspace initialization.
    pub allocation: RemoteInputAllocation,
    /// The original guarded worker ledger, ready for native launch-intent composition.
    pub registry: RemoteAdmissionRegistry,
}

/// Native ownership returned by authenticated original-initialization recovery. A reply cannot
/// construct this value; keep it across reply loss and mailbox backpressure until delivered once.
pub struct RemoteRecoveredHandoff {
    /// Exclusively held original workspace, never an adopted process or a new input reservation.
    pub workspace: super::ReceivedWorkerWorkspace,
    /// Original guarded ledger; ordinary reserve_launch remains the single-intent boundary.
    pub registry: RemoteAdmissionRegistry,
}

/// Connection disposition. No result here establishes provider execution or protected-main approval.
pub enum RemoteReceivingBrokerOutcome {
    /// Clean EOF before handoff. Supervisor state survives and requires fresh authentication.
    Disconnected,
    /// The original input reservation was consumed and handed to the supervisor exactly once.
    Materialized {
        /// Native ownership survives a final reply write failure.
        handoff: Box<RemoteReceivedHandoff>,
        /// Local write/flush succeeded; this is not proof that the peer durably received the reply.
        reply_written: bool,
    },
}

/// Serve one connection to an already-configured native assignment. Native configuration, initial
/// worker identity admission, transport authentication, deadlines and bounded stderr are supplied by
/// the embedding supervisor. No endpoint/process/key is opened or chosen by peer bytes here.
///
/// Every error ends this connection; its owner must close the streams. Session ownership remains
/// with the caller. If final reply I/O fails AFTER materialization, return the native handoff with
/// reply_written=false instead of dropping it or returning permission to repeat allocation.
pub fn serve_remote_receiving<R: Read, W: Write>(
    session: &mut RemoteReceivingSession<'_>,
    input: R,
    output: W,
) -> io::Result<RemoteReceivingBrokerOutcome> {
    serve_with_limits(
        session,
        input,
        output,
        MAX_CONNECTION_FRAMES,
        MAX_CONNECTION_BYTES,
    )
}

fn serve_with_limits<R: Read, W: Write>(
    session: &mut RemoteReceivingSession<'_>,
    input: R,
    output: W,
    frame_limit: usize,
    byte_limit: u64,
) -> io::Result<RemoteReceivingBrokerOutcome> {
    let mut connection = session.connect().map_err(|_| refused())?;
    let mut reader = RemoteFrameReader::new(input);
    let mut writer = RemoteFrameWriter::new(output);
    writer.write_frame(&RemoteFrame::Control(
        connection
            .proof()
            .map_err(|_| refused())?
            .encode()
            .into_bytes(),
    ))?;
    let mut authenticated = false;
    let mut received_bytes = 0u64;
    let mut requests = std::collections::BTreeSet::new();
    for _ in 0..frame_limit {
        let Some(frame) = reader.read_frame()? else {
            return Ok(RemoteReceivingBrokerOutcome::Disconnected);
        };
        received_bytes = received_bytes
            .checked_add(frame.encoded_len()?)
            .filter(|total| *total <= byte_limit)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "remote-receiving-byte-budget")
            })?;
        if let RemoteFrame::Control(bytes) = frame {
            let command = RemoteReceivingCommand::decode(&bytes)?;
            if requests.len() >= MAX_CONTROL_REQUESTS
                || !requests.insert(command.request().to_owned())
            {
                return Err(refused());
            }
            match command {
                RemoteReceivingCommand::Authenticate { request, signature } if !authenticated => {
                    let access = connection.authenticate(&signature).map_err(|_| refused())?;
                    authenticated = true;
                    let detail = Json::text(match access {
                        RemoteReceivingAccess::Receiving => "receiving",
                        RemoteReceivingAccess::Retained => "retained",
                    });
                    writer.write_frame(&reply(
                        "authenticated",
                        Some(&request),
                        connection.receipt().map_err(|_| refused())?,
                        detail,
                    ))?;
                }
                RemoteReceivingCommand::Status { request, digest } if authenticated => {
                    let (offset, complete) = connection.status(digest).map_err(|_| refused())?;
                    writer.write_frame(&reply(
                        "chunk",
                        Some(&request),
                        connection.receipt().map_err(|_| refused())?,
                        chunk(digest, offset, complete),
                    ))?;
                }
                RemoteReceivingCommand::Materialize { request } if authenticated => {
                    let receipt = connection.receipt().map_err(|_| refused())?.clone();
                    let (allocation, registry) = connection.materialize().map_err(|_| refused())?;
                    let handoff = Box::new(RemoteReceivedHandoff {
                        allocation,
                        registry,
                    });
                    let reply_written = writer
                        .write_frame(&reply("materialized", Some(&request), &receipt, Json::Null))
                        .is_ok();
                    return Ok(RemoteReceivingBrokerOutcome::Materialized {
                        handoff,
                        reply_written,
                    });
                }
                _ => return Err(refused()),
            }
        } else {
            if !authenticated {
                return Err(refused());
            }
            let progress = connection.receive(frame).map_err(|_| refused())?;
            let (kind, detail) = match progress {
                RemoteReceivingProgress::Manifest => ("manifest", Json::Null),
                RemoteReceivingProgress::Chunk {
                    digest,
                    offset,
                    complete,
                } => ("chunk", chunk(digest, offset, complete)),
            };
            writer.write_frame(&reply(
                kind,
                None,
                connection.receipt().map_err(|_| refused())?,
                detail,
            ))?;
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "remote-receiving-frame-budget",
    ))
}
fn chunk(digest: mesh_cas::Digest32, offset: u64, complete: bool) -> Json {
    Json::object([
        ("digest", Json::text(digest.to_hex())),
        ("offset", Json::Number(offset)),
        ("complete", Json::Bool(complete)),
    ])
}
fn reply(
    kind: &str,
    request: Option<&str>,
    receipt: &RemoteAdmissionReceipt,
    detail: Json,
) -> RemoteFrame {
    let work = receipt.work();
    let admission = Json::object([
        ("coordinator", Json::text(receipt.coordinator())),
        ("objective", Json::text(receipt.objective())),
        ("lane", Json::text(&work.lane)),
        ("run", Json::text(&work.run)),
        ("assignment", Json::text(&work.assignment.id)),
        ("input", Json::text(work.assignment.input.to_string())),
        ("bundle", Json::text(work.assignment.bundle.to_string())),
        ("allocation", Json::text(receipt.allocation())),
        ("revision", Json::Number(receipt.revision())),
    ]);
    RemoteFrame::Control(
        Json::object([
            ("schema", Json::text("mesh.receiving-reply/v1")),
            ("kind", Json::text(kind)),
            ("request", request.map(Json::text).unwrap_or(Json::Null)),
            ("admission", admission),
            ("detail", detail),
        ])
        .encode()
        .into_bytes(),
    )
}
fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "remote-receiving-command-refused",
    )
}

#[cfg(test)]
pub(in crate::fleet) mod tests;
