//! Native remote reads: prepare under the service lock, exchange outside it, then revalidate.
use super::*;
use crate::fleet::{
    NativeSshDestination, RemoteFrame, RemoteFrameReader, RemoteFrameWriter,
    RemoteResultDiscoveryChallenge, RemoteSavedResultPage, RemoteWorkerStatusChallenge,
    RemoteWorkerStatusReceipt, RemoteWorkerStatusRequest,
};
use mesh_crypto::SigningPayload;
use mesh_types::{PublicKey, Signature};
use std::{io, time::Duration};

/// Read-only operation chosen by the native coordinator. It never renews or adopts a run.
pub enum RemoteObservationKind {
    /// Retained current lease and execution facts, without renewal.
    CurrentLease,
    /// Explicit complete verification of the originally retained input. Never routine polling.
    InputInspection,
    /// One bounded saved-result catalog page.
    Results {
        /// Last acknowledged catalog revision; zero starts discovery.
        after: u64,
    },
}
/// Authenticated historical facts. Neither variant grants execution or import authority.
pub enum RemoteObservationOutcome {
    /// Verified worker status with the retained effective lease.
    CurrentLease(RemoteWorkerStatusReceipt),
    /// Fresh original-input facts, never permission to restart work.
    InputInspection(RemoteWorkerStatusReceipt),
    /// Verified page, or unknown/unrecorded work; absence grants no retry.
    Results(Option<RemoteSavedResultPage>),
}
enum Challenge {
    CurrentLease(RemoteWorkerStatusChallenge),
    InputInspection(RemoteWorkerStatusChallenge),
    Results(RemoteResultDiscoveryChallenge),
}
/// One-use native query bound to this exact service and assignment. Private wire bytes never
/// become a renderer-selected request; dropping the handle performs no remote operation.
pub struct RemoteObservation {
    history: FleetHistory,
    challenge: Challenge,
    frame: RemoteFrame,
}
impl FleetHistory {
    /// Prepare a fresh signed observation using independently admitted coordinator/worker keys.
    /// The callback signs only the closed native query. No network I/O occurs under the fleet lock.
    pub fn prepare_remote_observation(
        &self,
        lane: &str,
        run: &str,
        coordinator: PublicKey,
        worker: PublicKey,
        kind: RemoteObservationKind,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> Result<RemoteObservation, Unavailable> {
        let mut inner = self.0.lock()?;
        let runtime = &mut inner.runtime;
        let (challenge, frame) = match kind {
            RemoteObservationKind::CurrentLease => {
                let c = RemoteWorkerStatusChallenge::issue_with_current_lease(
                    runtime,
                    lane,
                    run,
                    coordinator,
                    worker,
                )
                .map_err(runtime_error)?;
                let frame = c
                    .signed_query(runtime, sign)
                    .and_then(|q| q.frame())
                    .map_err(runtime_error)?;
                (Challenge::CurrentLease(c), frame)
            }
            RemoteObservationKind::InputInspection => {
                let c = RemoteWorkerStatusChallenge::issue_with_input_inspection(
                    runtime,
                    lane,
                    run,
                    coordinator,
                    worker,
                )
                .map_err(runtime_error)?;
                let frame = c
                    .signed_query(runtime, sign)
                    .and_then(|q| q.frame())
                    .map_err(runtime_error)?;
                (Challenge::InputInspection(c), frame)
            }
            RemoteObservationKind::Results { after } => {
                let c = RemoteResultDiscoveryChallenge::issue(
                    RemoteWorkerStatusRequest {
                        runtime,
                        lane,
                        run,
                        coordinator,
                        worker,
                    },
                    after,
                )
                .map_err(runtime_error)?;
                let frame = c
                    .signed_query(runtime, sign)
                    .and_then(|q| q.frame())
                    .map_err(runtime_error)?;
                (Challenge::Results(c), frame)
            }
        };
        Ok(RemoteObservation {
            history: self.clone(),
            challenge,
            frame,
        })
    }
}
impl RemoteObservation {
    /// Perform one bounded exchange through native-admitted SSH policy. No automatic reconnect,
    /// retry, lease renewal or inference of termination occurs on transport failure.
    pub fn read_over_ssh(
        self,
        peer: &NativeSshDestination,
        budget: Duration,
    ) -> io::Result<RemoteObservationOutcome> {
        self.exchange(|frame| {
            let mut connection = peer.connect(budget)?;
            let (input, output) = connection.streams()?;
            RemoteFrameWriter::new(output).write_frame(&frame)?;
            RemoteFrameReader::new(input)
                .read_frame()?
                .ok_or_else(unavailable)
        })
    }
    fn exchange(
        self,
        transport: impl FnOnce(RemoteFrame) -> io::Result<RemoteFrame>,
    ) -> io::Result<RemoteObservationOutcome> {
        // No service guard survives preparation. Concurrent view, cancellation and other actions
        // remain available throughout connect/write/read and are checked when accepting the reply.
        let RemoteFrame::Control(bytes) = transport(self.frame)? else {
            return Err(unavailable());
        };
        let encoded = std::str::from_utf8(&bytes).map_err(|_| unavailable())?;
        let mut inner = self.history.0.lock().map_err(|_| unavailable())?;
        match self.challenge {
            Challenge::CurrentLease(c) => c
                .verify_reply(&mut inner.runtime, encoded)
                .map(RemoteObservationOutcome::CurrentLease),
            Challenge::InputInspection(c) => c
                .verify_reply(&mut inner.runtime, encoded)
                .map(RemoteObservationOutcome::InputInspection),
            Challenge::Results(c) => c
                .verify_reply(&mut inner.runtime, encoded)
                .map(RemoteObservationOutcome::Results),
        }
        .map_err(|_| unavailable())
    }
}
fn unavailable() -> io::Error {
    io::Error::other("Remote observation unavailable; retained execution state is unchanged")
}

#[cfg(test)]
mod tests;
