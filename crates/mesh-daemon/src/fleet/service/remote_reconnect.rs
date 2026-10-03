//! Explicit continuation of one retained initial transfer; never dispatch or worker adoption.
use super::*;
use crate::fleet::{
    deliver_remote_input_over_ssh, NativeSshDestination, RemoteInputDeliveryIntent,
    RemoteInputSource, RemoteInputTransferOutcome, RemoteInputTransferRequest,
};
use mesh_crypto::SigningPayload;
use mesh_types::{PublicKey, Signature};
use std::{io, time::Duration};
/// Exact native saved input and independently admitted identities. No new assignment or lease.
pub struct RemoteHistoryInputRequest<'a> {
    /// Retained lane identity.
    pub lane: &'a str,
    /// Exact retained attempt.
    pub run: &'a str,
    /// Native saved-history export, never mutable project contents.
    pub source: &'a RemoteInputSource,
    /// Independently admitted coordinator identity.
    pub coordinator: PublicKey,
    /// Independently trusted worker identity.
    pub worker: PublicKey,
}
impl FleetHistory {
    /// Reconnect only a claimed, still-launching initial-lease input transfer. The existing peer
    /// challenge rechecks ownership/input/key/lease and cancellation before signing and after reply.
    /// No coordinator command, lease renewal, workspace adoption or new allocation is authorized.
    /// The resident worker must retain the original reservation. An error preserves uncertainty;
    /// neither an error nor input acceptance is evidence of worker completion or safe replacement.
    pub fn reconnect_remote_input_over_ssh(
        &self,
        peer: &NativeSshDestination,
        request: RemoteHistoryInputRequest<'_>,
        budget: Duration,
        sign: impl FnMut(&SigningPayload) -> Result<Signature, String>,
    ) -> io::Result<RemoteInputTransferOutcome> {
        self.reconnect_remote_input(request, |request| {
            deliver_remote_input_over_ssh(
                peer,
                request,
                RemoteInputDeliveryIntent::Reconnect,
                budget,
                sign,
            )
        })
    }
    pub(in crate::fleet) fn reconnect_remote_input<T>(
        &self,
        request: RemoteHistoryInputRequest<'_>,
        transfer: impl FnOnce(RemoteInputTransferRequest<'_>) -> io::Result<T>,
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
        // Only the closed operation can use this guarded runtime. No service lock survives into
        // transport, and retained history never becomes a generic execution or dispatch surface.
        transfer(RemoteInputTransferRequest {
            runtime: &mut runtime,
            lane: request.lane,
            run: request.run,
            source: request.source,
            coordinator: request.coordinator,
            worker: request.worker,
        })
    }
}
fn unavailable() -> io::Error {
    io::Error::other("Retained input transfer unavailable; reconcile the original assignment")
}
