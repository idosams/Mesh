//! Resident-owned authenticated transfers. Connections borrow state; they never own reservations.
use super::*;
use mesh_crypto::SigningPayload;
use mesh_types::Signature;
use std::io::{self, Read, Write};
use std::sync::mpsc::{SyncSender, TrySendError};

struct Transfer<'a> {
    coordinator: String,
    objective: String,
    work: RemoteWork,
    limits: Limits,
    session: RemoteReceivingSession<'a>,
    receipt: Option<RemoteAdmissionReceipt>,
    handoff: Option<Box<RemoteReceivedHandoff>>,
    pending: Option<ReceivedWorkerRequest>,
}

/// Native connection disposition; materialized input is not provider execution or completion.
pub enum WorkerConnectionOutcome {
    /// EOF before materialization. The original reservation and partial input remain resident-owned.
    Disconnected,
    /// Native input is retained for one-time delivery to the independent provider supervisor.
    Materialized {
        /// Exact facts, never a replacement reservation.
        admission: RemoteAdmissionReceipt,
        /// Only local final-response write/flush success, not durable peer receipt.
        reply_written: bool,
    },
}

/// Bounded resident transfer owner backed by one guarded worker installation across objectives.
/// The embedding endpoint authenticates its transport and supplies read/write deadlines. It runs
/// provider observation independently, because a connection may block until its I/O deadline.
/// No entry eviction, receipt adoption, retry of failed launch, or key provisioning occurs here.
#[cfg(target_os = "macos")]
pub struct NativeWorkerConnections<'a> {
    installation: &'a NativeWorkerInstallation,
    destination: &'a RemoteInputDestination,
    policy: RemoteDispatchPolicy<'a>,
    maximum: usize,
    transfers: Vec<Transfer<'a>>,
}
#[cfg(target_os = "macos")]
impl<'a> NativeWorkerConnections<'a> {
    /// Admit native policy and storage before exposing an endpoint. Capacity includes terminal and
    /// uncertain transfers; neither disconnect nor expiration makes a slot available again.
    pub fn new(
        installation: &'a NativeWorkerInstallation,
        destination: &'a RemoteInputDestination,
        policy: RemoteDispatchPolicy<'a>,
        maximum: usize,
    ) -> io::Result<Self> {
        if !(1..=64).contains(&maximum) || installation.identity()?.worker() != policy.worker {
            return Err(refused());
        }
        destination.verify()?;
        Ok(Self {
            installation,
            destination,
            policy,
            maximum,
            transfers: Vec::new(),
        })
    }

    /// Verify a fresh signed dispatch, bind its exact facts to the retained session, and serve one
    /// input connection. Worker proof signing is narrowly typed and rechecked against native custody.
    /// Any error closes this connection in the embedding endpoint, without dropping retained state.
    pub fn serve<R: Read, W: Write>(
        &mut self,
        mut input: R,
        mut output: W,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> io::Result<WorkerConnectionOutcome> {
        self.installation.verify()?;
        self.destination.verify()?;
        let Some(RemoteFrame::Control(bytes)) = RemoteFrameReader::new(&mut input).read_frame()?
        else {
            return Err(refused());
        };
        let dispatch = RemoteDispatch::decode(std::str::from_utf8(&bytes).map_err(|_| refused())?)
            .and_then(|dispatch| dispatch.verify(&self.policy))
            .map_err(|_| refused())?;
        let index = match self.transfers.iter().position(|entry| {
            entry.coordinator == dispatch.coordinator()
                && entry.objective == dispatch.objective()
                && entry.work.assignment.id == dispatch.work().assignment.id
        }) {
            Some(index) => {
                let entry = &self.transfers[index];
                if entry.work != *dispatch.work()
                    || entry.limits != *dispatch.limits()
                    || entry.receipt.is_some()
                {
                    return Err(refused());
                }
                index
            }
            None => {
                if self.transfers.len() >= self.maximum {
                    return Err(refused());
                }
                let registry = self.installation.registry(
                    dispatch.coordinator(),
                    dispatch.objective(),
                    dispatch.limits().clone(),
                )?;
                let mut allocation = [0u8; 16];
                std::fs::File::open("/dev/urandom")?.read_exact(&mut allocation)?;
                let allocation: String = allocation.iter().map(|b| format!("{b:02x}")).collect();
                let index = self.transfers.len();
                self.transfers.push(Transfer {
                    coordinator: dispatch.coordinator().into(),
                    objective: dispatch.objective().into(),
                    work: dispatch.work().clone(),
                    limits: dispatch.limits().clone(),
                    session: RemoteReceivingSession::new(
                        registry,
                        dispatch.work().clone(),
                        &allocation,
                        self.destination,
                    ),
                    receipt: None,
                    handoff: None,
                    pending: None,
                });
                index
            }
        };
        self.installation.verify()?;
        let reply = dispatch.worker_reply(sign).map_err(|_| refused())?;
        self.installation.verify()?;
        RemoteFrameWriter::new(&mut output).write_frame(&reply)?;
        match serve_remote_receiving(&mut self.transfers[index].session, input, output)? {
            RemoteReceivingBrokerOutcome::Disconnected => Ok(WorkerConnectionOutcome::Disconnected),
            RemoteReceivingBrokerOutcome::Materialized {
                handoff,
                reply_written,
            } => {
                let admission = handoff
                    .allocation
                    .admission
                    .as_ref()
                    .ok_or_else(refused)?
                    .clone();
                let entry = &mut self.transfers[index];
                entry.receipt = Some(admission.clone());
                entry.handoff = Some(handoff);
                Ok(WorkerConnectionOutcome::Materialized {
                    admission,
                    reply_written,
                })
            }
        }
    }

    /// Bind native launch policy once to an original materialized handoff. A full or disconnected
    /// mailbox retains the entire request, including native launch policy, for delivery reconciliation.
    /// Repeating this call cannot replace policy or create another request from an admission receipt.
    pub fn queue_received(
        &mut self,
        admission: &RemoteAdmissionReceipt,
        launch: ReceivedWorkerLaunch,
        reply: SyncSender<Result<RemoteAdmissionReceipt, crate::ipc::Unavailable>>,
        sender: &SyncSender<ReceivedWorkerRequest>,
    ) -> io::Result<bool> {
        self.installation.verify()?;
        let entry = self
            .transfers
            .iter_mut()
            .find(|entry| entry.receipt.as_ref() == Some(admission))
            .ok_or_else(refused)?;
        let handoff = entry.handoff.take().ok_or_else(refused)?;
        entry.pending = Some(ReceivedWorkerRequest::Start {
            handoff,
            launch: Box::new(launch),
            reply,
        });
        Self::deliver(entry, sender)
    }

    /// Try delivery of the same retained requests, never reconstructing a handoff or invoking a new
    /// launch-policy factory. Returns the number delivered now; full/disconnected requests stay held.
    pub fn flush_pending(
        &mut self,
        sender: &SyncSender<ReceivedWorkerRequest>,
    ) -> io::Result<usize> {
        self.installation.verify()?;
        let mut count = 0;
        for entry in &mut self.transfers {
            if entry.pending.is_some() && Self::deliver(entry, sender)? {
                count += 1;
            }
        }
        Ok(count)
    }
    fn deliver(
        entry: &mut Transfer<'a>,
        sender: &SyncSender<ReceivedWorkerRequest>,
    ) -> io::Result<bool> {
        let request = entry.pending.take().ok_or_else(refused)?;
        match sender.try_send(request) {
            Ok(()) => Ok(true),
            Err(TrySendError::Full(request) | TrySendError::Disconnected(request)) => {
                entry.pending = Some(request);
                Ok(false)
            }
        }
    }
}
fn refused() -> io::Error {
    io::Error::other("worker connection unavailable or requires reconciliation")
}

#[cfg(test)]
mod tests;
