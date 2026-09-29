//! Native coordinator composition: signed dispatch, exact peer proof, then immutable input.
use super::{
    transfer_remote_input, NativeSshDestination, RemoteAssignment, RemoteDispatch, RemoteFrame,
    RemoteFrameReader, RemoteFrameWriter, RemoteInputReconnectChallenge,
    RemoteInputTransferOutcome, RemoteInputTransferRequest, RemotePeerChallenge,
};
use mesh_crypto::SigningPayload;
use mesh_types::Signature;
use std::io::{self, Read, Write};
use std::time::Duration;

/// Explicit native decision; errors never select another intent automatically.
pub enum RemoteInputDeliveryIntent {
    /// Prove the selected worker and durably claim an existing unclaimed dispatched attempt.
    Claim {
        /// Exact independently selected assignment and immutable input/bundle identity.
        assignment: RemoteAssignment,
        /// Native ledger request identity for this claim; never inferred from a peer reply.
        request: String,
    },
    /// Reprove the worker for the existing initial-lease input transfer. Running/completed work,
    /// renewed leases and lost worker reservations require separate reconciliation and refuse here.
    Reconnect,
}
enum Proof {
    Claim(RemotePeerChallenge, String),
    Reconnect(RemoteInputReconnectChallenge),
}
struct Prepared {
    proof: Proof,
    dispatch: RemoteDispatch,
}
fn refused() -> io::Error {
    io::Error::other("remote input delivery unavailable or requires reconciliation")
}
fn prepare(
    request: &mut RemoteInputTransferRequest<'_>,
    intent: RemoteInputDeliveryIntent,
    sign: &mut impl FnMut(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<Prepared> {
    request.runtime.refresh().map_err(|_| refused())?;
    let proof = match intent {
        RemoteInputDeliveryIntent::Claim {
            assignment,
            request: claim,
        } => {
            super::id_valid(&claim).map_err(|_| refused())?;
            if assignment.input != request.source.manifest().input()
                || assignment.bundle != request.source.manifest().bundle()
            {
                return Err(refused());
            }
            Proof::Claim(
                RemotePeerChallenge::issue(
                    request.runtime,
                    request.lane,
                    request.run,
                    assignment,
                    request.worker,
                )
                .map_err(|_| refused())?,
                claim,
            )
        }
        RemoteInputDeliveryIntent::Reconnect => {
            let assignment = request
                .runtime
                .state()
                .lanes
                .get(request.lane)
                .and_then(|lane| lane.runs.last())
                .filter(|run| run.id == request.run)
                .and_then(|run| run.remote.as_ref())
                .ok_or_else(refused)?;
            if assignment.input != request.source.manifest().input()
                || assignment.bundle != request.source.manifest().bundle()
            {
                return Err(refused());
            }
            Proof::Reconnect(
                RemoteInputReconnectChallenge::issue(
                    request.runtime,
                    request.lane,
                    request.run,
                    request.worker,
                )
                .map_err(|_| refused())?,
            )
        }
    };
    let dispatch = match &proof {
        Proof::Claim(proof, _) => {
            proof.signed_dispatch(request.runtime, &request.coordinator, sign)
        }
        Proof::Reconnect(proof) => {
            proof.signed_dispatch(request.runtime, &request.coordinator, sign)
        }
    }
    .map_err(|_| refused())?;
    Ok(Prepared { proof, dispatch })
}
fn deliver<R: Read, W: Write>(
    request: RemoteInputTransferRequest<'_>,
    prepared: Prepared,
    mut input: R,
    mut output: W,
    sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<RemoteInputTransferOutcome> {
    RemoteFrameWriter::new(&mut output)
        .write_frame(&prepared.dispatch.frame().map_err(|_| refused())?)?;
    // The reader does not prefetch, so dropping this bootstrap wrapper cannot lose transfer bytes.
    let Some(RemoteFrame::Control(reply)) = RemoteFrameReader::new(&mut input).read_frame()? else {
        return Err(refused());
    };
    let reply = std::str::from_utf8(&reply).map_err(|_| refused())?;
    match prepared.proof {
        Proof::Claim(proof, claim) => {
            proof
                .verify_dispatch_reply(request.runtime, &claim, reply)
                .map_err(|_| refused())?;
        }
        Proof::Reconnect(proof) => {
            proof
                .verify_dispatch_reply(request.runtime, reply)
                .map_err(|_| refused())?;
        }
    }
    // This second signer invocation occurs only after peer verification and canonical admission
    // proof/context comparison. Neither signing callback accepts arbitrary bytes from the stream.
    transfer_remote_input(request, input, output, sign)
}

/// Deliver one exact native-selected saved input through an independently configured SSH endpoint.
/// This performs initial claiming OR explicit input reconnect, never an automatic retry. The same
/// signing callback signs only the checked coordinator dispatch and checked receiving admission.
///
/// Every return drops/reaps this local SSH connection. An error may follow a durable claim or worker
/// materialization: preserve uncertainty and inspect the original attempt before another decision.
/// A returned materialization receipt is not provider startup, task completion, a saved result, lease
/// renewal, or approval to advance protected main. Native configuration and SSH provisioning remain
/// external prerequisites. The pipe budget does not extend challenge freshness or lease deadlines.
pub fn deliver_remote_input_over_ssh(
    destination: &NativeSshDestination,
    mut request: RemoteInputTransferRequest<'_>,
    intent: RemoteInputDeliveryIntent,
    budget: Duration,
    mut sign: impl FnMut(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<RemoteInputTransferOutcome> {
    let prepared = prepare(&mut request, intent, &mut sign)?;
    let mut connection = destination.connect(budget)?;
    let (input, output) = connection.streams()?;
    deliver(request, prepared, input, output, sign)
}

#[cfg(test)]
mod tests;
