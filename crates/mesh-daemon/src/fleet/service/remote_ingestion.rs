//! Closed native result ingestion with an independent, authority-retaining ledger connection.
use super::*;
use crate::fleet::{
    ingest_remote_result_over_ssh, NativeSshDestination, RemoteInputDestination,
    RemoteInputManifest, RemoteLocalReviewReceipt, RemoteResultIngestionRequest,
    RemoteWorkerStatusRequest,
};
use mesh_crypto::SigningPayload;
use mesh_types::{PublicKey, Signature};
use std::{io, time::Duration};

/// Native-selected saved result. Every destination and identity is independently admitted by the
/// native caller; renderer payloads and worker replies cannot choose local storage or trust policy.
pub struct RemoteHistoryIngestionRequest<'a> {
    /// Retained lane identity.
    pub lane: &'a str,
    /// Exact retained run identity.
    pub run: &'a str,
    /// Native coordinator identity already bound to the assignment.
    pub coordinator: PublicKey,
    /// Independently trusted worker identity.
    pub worker: PublicKey,
    /// Exact retained immutable input manifest.
    pub input: &'a RemoteInputManifest,
    /// Exact signed offer selected from authenticated discovery.
    pub offer: &'a str,
    /// Independently admitted native receiving storage.
    pub destination: &'a RemoteInputDestination,
    /// Stable native allocation identity for this selected result.
    pub allocation: &'a str,
    /// Native local-history trust policy.
    pub reviewers: &'a TrustedReviewers,
    /// Local checkpoint settings; these cannot launch a provider.
    pub checkpoint: CheckpointRuntimeParameters,
    /// Native actor recording review, without protected-main approval.
    pub actor: PublicKey,
}
impl FleetHistory {
    /// Ingest one selected result without holding the fleet mutex across network or filesystem
    /// work. The guarded independent runtime replays durable events and retains normal revision
    /// refusals. Completed replay uses retained evidence without connecting or signing. Errors do
    /// not retry, renew, adopt workers, approve main or write the original project.
    pub fn ingest_remote_result_over_ssh(
        &self,
        peer: &NativeSshDestination,
        request: RemoteHistoryIngestionRequest<'_>,
        budget: Duration,
        sign: impl FnMut(&SigningPayload) -> Result<Signature, String>,
    ) -> io::Result<RemoteLocalReviewReceipt> {
        self.ingest_selected_result(request, |request| {
            ingest_remote_result_over_ssh(peer, request, budget, sign)
        })
    }

    pub(in crate::fleet) fn ingest_selected_result(
        &self,
        request: RemoteHistoryIngestionRequest<'_>,
        receive: impl FnOnce(RemoteResultIngestionRequest<'_>) -> io::Result<RemoteLocalReviewReceipt>,
    ) -> io::Result<RemoteLocalReviewReceipt> {
        self.with_ingestion_runtime(|runtime| {
            receive(RemoteResultIngestionRequest {
                status: RemoteWorkerStatusRequest {
                    runtime,
                    lane: request.lane,
                    run: request.run,
                    coordinator: request.coordinator,
                    worker: request.worker,
                },
                input: request.input,
                offer: request.offer,
                destination: request.destination,
                allocation: request.allocation,
                reviewers: request.reviewers,
                checkpoint: request.checkpoint,
                actor: request.actor,
            })
        })
    }

    fn with_ingestion_runtime<T>(
        &self,
        action: impl FnOnce(&mut Runtime) -> io::Result<T>,
    ) -> io::Result<T> {
        let mut runtime = {
            let inner = self.0.lock().map_err(|_| unavailable())?;
            let store = inner
                .runtime
                .store
                .reopen_guarded_connection()
                .map_err(|_| unavailable())?;
            Runtime::open(store, inner.runtime.objective()).map_err(|_| unavailable())?
        };
        // No mutable runtime escapes the closed native operation. The shared ledger guard retains
        // the catalogue owner and identity pins; the service remains available during every wait.
        action(&mut runtime)
    }
}
fn unavailable() -> io::Error {
    io::Error::other("Remote ingestion unavailable; retained history requires reconciliation")
}

#[cfg(test)]
mod tests;
