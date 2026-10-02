//! Current-session native initial dispatch. Saved history never grants execution ownership.
use super::*;
use crate::fleet::{
    deliver_remote_input_over_ssh, NativeSshDestination, RemoteAssignment,
    RemoteInputDeliveryIntent, RemoteInputSource, RemoteInputTransferOutcome,
    RemoteInputTransferRequest,
};
use mesh_crypto::SigningPayload;
use mesh_types::{PublicKey, Signature};
use std::{io, time::Duration};

/// Independently admitted native start selection. This is not an agent/renderer request.
pub struct RemoteNativeStartRequest<'a> {
    /// Native-created lane retained by the current service session.
    pub lane: &'a str,
    /// Stable attempt identity retained even when transport fails.
    pub run: &'a str,
    /// Independently selected worker assignment, immutable input and fixed initial lease.
    pub assignment: RemoteAssignment,
    /// Exact native saved-history export; never live project bytes.
    pub source: &'a RemoteInputSource,
    /// Native coordinator identity.
    pub coordinator: PublicKey,
    /// Independently trusted worker identity.
    pub worker: PublicKey,
}
impl FleetService {
    /// Dispatch a fresh native lane and transfer its saved input, without holding the service mutex
    /// during SSH. Any existing attempt refuses, including a previous failed/uncertain call with
    /// identical input. Errors never retry, renew, reattach, select a new run, or approve main.
    /// Input acceptance is not proof that the provider started or completed its work.
    pub fn start_remote_input_over_ssh(
        &self,
        peer: &NativeSshDestination,
        request: RemoteNativeStartRequest<'_>,
        budget: Duration,
        sign: impl FnMut(&SigningPayload) -> Result<Signature, String>,
    ) -> io::Result<RemoteInputTransferOutcome> {
        self.start_remote_input(request, |request, intent| {
            deliver_remote_input_over_ssh(peer, request, intent, budget, sign)
        })
    }

    pub(in crate::fleet) fn start_remote_input<T>(
        &self,
        request: RemoteNativeStartRequest<'_>,
        transfer: impl FnOnce(
            RemoteInputTransferRequest<'_>,
            RemoteInputDeliveryIntent,
        ) -> io::Result<T>,
    ) -> io::Result<T> {
        super::super::id_valid(request.run).map_err(|_| unavailable())?;
        super::super::id_valid(&format!("remote-start-{}", request.run))
            .map_err(|_| unavailable())?;
        super::super::id_valid(&format!("remote-claim-{}", request.run))
            .map_err(|_| unavailable())?;
        request.assignment.validate().map_err(|_| unavailable())?;
        let now = received_clock().map_err(|_| unavailable())?;
        if request.assignment.input != request.source.manifest().input()
            || request.assignment.bundle != request.source.manifest().bundle()
            || request.assignment.worker_key
                != RecordDigest::from_bytes(*request.worker.as_bytes()).to_string()
            || request.assignment.lease_until_ms <= now
            || request.assignment.lease_until_ms.saturating_sub(now) > 3_600_000
        {
            return Err(unavailable());
        }
        let (mut runtime, workspace) = {
            let mut inner = self.lock().map_err(|_| unavailable())?;
            inner.runtime.refresh().map_err(|_| unavailable())?;
            let lane = inner
                .runtime
                .state()
                .lanes
                .get(request.lane)
                .ok_or_else(unavailable)?;
            if !lane.runs.is_empty() || lane.base != request.source.manifest().input() {
                return Err(unavailable());
            }
            let workspace = inner
                .workspaces
                .get(request.lane)
                .cloned()
                .ok_or_else(unavailable)?;
            exact_state(&workspace).map_err(|_| unavailable())?;
            // Admit the independent connection before changing durable state; never recreate a
            // missing ledger. Current workspace ownership remains pinned through the transfer.
            let store = inner
                .runtime
                .store
                .reopen_guarded_connection()
                .map_err(|_| unavailable())?;
            inner
                .runtime
                .record(
                    &format!("remote-start-{}", request.run),
                    Command::Dispatch {
                        lane: request.lane.into(),
                        run: request.run.into(),
                    },
                )
                .map_err(|_| unavailable())?;
            let runtime =
                Runtime::open(store, inner.runtime.objective()).map_err(|_| unavailable())?;
            (runtime, workspace)
        };
        let result = transfer(
            RemoteInputTransferRequest {
                runtime: &mut runtime,
                lane: request.lane,
                run: request.run,
                source: request.source,
                coordinator: request.coordinator,
                worker: request.worker,
            },
            RemoteInputDeliveryIntent::Claim {
                assignment: request.assignment,
                request: format!("remote-claim-{}", request.run),
            },
        );
        exact_state(&workspace).map_err(|_| unavailable())?;
        result
    }
}
fn unavailable() -> io::Error {
    io::Error::other(
        "Remote start unavailable; preserve the original request and reconcile retained history",
    )
}
#[cfg(test)]
mod tests;
