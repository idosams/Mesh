//! Coordinator result ingestion: exact selected transport, durable evidence and native review.
use super::*;
use crate::fleet::{
    AuthenticatedRemoteResultEvidence, NativeRemoteResultReceiver, NativeSshDestination,
    RemoteInputDestination, RemoteLocalReviewReceipt, RemoteResultEvidenceRequest,
    RemoteWorkerStatusRequest,
};
use crate::{CheckpointRuntimeParameters, TrustedReviewers};
use std::io;
use std::time::Duration;

/// Native-selected immutable result and local storage. Peer/renderer paths never enter this request.
pub struct RemoteResultIngestionRequest<'a> {
    /// Current coordinator assignment and independently configured peer identities.
    pub status: RemoteWorkerStatusRequest<'a>,
    /// Complete exact input manifest retained by the coordinator.
    pub input: &'a RemoteInputManifest,
    /// Exact signed offer selected from authenticated discovery, not only a checkpoint name.
    pub offer: &'a str,
    /// Independently admitted private receiving store and result-allocation parent.
    pub destination: &'a RemoteInputDestination,
    /// Stable native allocation identity used only if this offer has no completed local review.
    pub allocation: &'a str,
    /// Native trust policy for local history.
    pub reviewers: &'a TrustedReviewers,
    /// Native history runtime parameters; these do not launch a provider.
    pub checkpoint: CheckpointRuntimeParameters,
    /// Native actor recording the local review, distinct from protected-main approval.
    pub actor: PublicKey,
}
type Sign<'a> = dyn FnMut(&SigningPayload) -> Result<Signature, String> + 'a;
trait Transport {
    fn content(
        &mut self,
        destination: &RemoteInputDestination,
        request: RemoteWorkerStatusRequest<'_>,
        checkpoint: &str,
        offer: &str,
        sign: &mut Sign<'_>,
    ) -> io::Result<RemoteSavedResultOffer>;
    fn evidence(
        &mut self,
        request: RemoteResultEvidenceRequest<'_>,
        sign: &mut Sign<'_>,
    ) -> io::Result<AuthenticatedRemoteResultEvidence>;
}
struct Ssh<'a> {
    peer: &'a NativeSshDestination,
    budget: Duration,
}
impl Transport for Ssh<'_> {
    fn content(
        &mut self,
        destination: &RemoteInputDestination,
        request: RemoteWorkerStatusRequest<'_>,
        checkpoint: &str,
        offer: &str,
        sign: &mut Sign<'_>,
    ) -> io::Result<RemoteSavedResultOffer> {
        let mut connection = self.peer.connect(self.budget)?;
        let (input, output) = connection.streams()?;
        super::transfer::receive_remote_saved_result_selected(
            destination,
            request,
            checkpoint,
            Some(offer),
            input,
            output,
            sign,
        )
    }
    fn evidence(
        &mut self,
        request: RemoteResultEvidenceRequest<'_>,
        sign: &mut Sign<'_>,
    ) -> io::Result<AuthenticatedRemoteResultEvidence> {
        super::evidence::receive_remote_result_evidence_over_ssh(
            self.peer,
            request,
            self.budget,
            sign,
        )
    }
}
fn unavailable() -> io::Error {
    io::Error::other("remote result ingestion unavailable or requires reconciliation")
}
fn ingest(
    request: RemoteResultIngestionRequest<'_>,
    transport: &mut impl Transport,
    mut sign: impl FnMut(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<RemoteLocalReviewReceipt> {
    let status = request.status;
    let runtime = status.runtime;
    macro_rules! status {
        () => {
            RemoteWorkerStatusRequest {
                runtime,
                lane: status.lane,
                run: status.run,
                coordinator: status.coordinator,
                worker: status.worker,
            }
        };
    }
    if request.allocation.len() != 32
        || !request
            .allocation
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(unavailable());
    }
    let offer = RemoteSavedResultOffer::decode(request.offer).map_err(|_| unavailable())?;
    let context = RemoteWorkerStatusChallenge::issue(
        runtime,
        status.lane,
        status.run,
        status.coordinator,
        status.worker,
    )
    .map_err(|_| unavailable())?;
    if offer.body.get("target") != context.body.get("target") {
        return Err(unavailable());
    }
    let target = offer.body.get("target").ok_or_else(unavailable)?;
    if text(target, "input").map_err(|_| unavailable())? != request.input.input().to_string()
        || text(target, "bundle").map_err(|_| unavailable())? != request.input.bundle().to_string()
    {
        return Err(unavailable());
    }
    let digest =
        RecordDigest::from_bytes(*Blake3::digest_bytes(request.offer.as_bytes()).as_bytes());
    // Acknowledged completion is recovered locally. Missing/corrupt evidence refuses without
    // redownloading, repairing, creating another allocation, or reviving a worker.
    if let Some(receipt) = runtime
        .retained_remote_local_review(digest)
        .map_err(|_| unavailable())?
    {
        let (receiver, content) = NativeRemoteResultReceiver::reopen_content_receipt(
            request.destination,
            request.offer,
            status!(),
        )
        .map_err(|_| unavailable())?;
        let evidence = receiver
            .load_evidence(runtime, request.input, content.digest())
            .map_err(|_| unavailable())?
            .ok_or_else(unavailable)?;
        if evidence.digest() != receipt.evidence_receipt() {
            return Err(unavailable());
        }
        receipt.reopen(request.destination, receiver.manifest(), request.reviewers)?;
        return Ok(receipt);
    }
    let checkpoint = text(&offer.body, "checkpoint").map_err(|_| unavailable())?;
    let received = transport.content(
        request.destination,
        status!(),
        checkpoint,
        request.offer,
        &mut sign,
    )?;
    if received.encode() != request.offer {
        return Err(unavailable());
    }
    let (receiver, _) = NativeRemoteResultReceiver::reopen_content_receipt(
        request.destination,
        request.offer,
        status!(),
    )
    .map_err(|_| unavailable())?;
    let evidence = transport.evidence(
        RemoteResultEvidenceRequest {
            status: status!(),
            offer: request.offer,
            input: request.input,
            result: receiver.manifest(),
        },
        &mut sign,
    )?;
    let retained = receiver
        .record_evidence_receipt(runtime, request.input, &evidence)
        .map_err(|_| unavailable())?;
    let allocation = receiver
        .materialize_result(runtime, request.input, &retained, request.allocation)
        .map_err(|_| unavailable())?;
    let local = allocation.into_result_workspace(request.reviewers.clone(), request.checkpoint)?;
    let receipt = receiver
        .record_local_review(runtime, request.input, &retained, &local, request.actor)
        .map_err(|_| unavailable())?;
    receipt.reopen(request.destination, receiver.manifest(), request.reviewers)?;
    Ok(receipt)
}

/// Receive one exact selected remote result over native SSH and durably register a local review.
/// Two separately authenticated bounded connections retrieve content and correspondence. A completed
/// replay verifies retained native history without connecting or signing. Partial/unacknowledged
/// allocations are preserved and refuse adoption; an error never restarts work or renews a lease.
/// This performs no candidate import, protected-main approval, original-folder write or provider launch.
pub fn ingest_remote_result_over_ssh(
    peer: &NativeSshDestination,
    request: RemoteResultIngestionRequest<'_>,
    budget: Duration,
    sign: impl FnMut(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<RemoteLocalReviewReceipt> {
    ingest(request, &mut Ssh { peer, budget }, sign)
}

#[cfg(test)]
#[path = "ingestion/tests.rs"]
mod tests;
