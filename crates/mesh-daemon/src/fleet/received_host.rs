//! Native ownership of one received provider and its local scoped IPC route.
use super::host::{NativeFleetHost, WorkerObservation, WorkerSignerFactory};
use super::provider::NativeAdapter;
use super::service::FleetService;
use super::{
    Command, RemoteLaunchOutcome, RemoteLaunchReceipt, RemoteLaunchReservation,
    RemoteReceivedHandoff,
};
use crate::ipc::{
    nothing_to_recover, IpcServer, Json, Operations, ServerHandle, StartupSummary, Unavailable,
};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
mod resident;
mod supervisor;
pub use resident::{ReceivedWorkerMailbox, ReceivedWorkerRequest};
pub use supervisor::{ReceivedWorkerLaunch, ReceivedWorkerObservation, ReceivedWorkerSupervisor};

/// The resident native worker owns this handle independently of any transport connection.
/// Polling never dispatches another attempt. Drop is not process-tree termination proof.
pub struct ReceivedWorkerHost {
    // Shut down IPC before dropping the host's process/session handles.
    server: ServerHandle,
    host: NativeFleetHost,
    service: Arc<FleetService>,
    receipt: RemoteLaunchReceipt,
}

/// The provider endpoint exposes scoped fleet calls only. All ordinary workspace operations use
/// the Operations trait's refusal defaults, rather than admitting general native workspace IPC.
struct ScopedRouter {
    service: Arc<FleetService>,
    objective: String,
}
impl Operations for ScopedRouter {
    fn serving(&self) -> bool {
        true
    }
    fn startup(&self) -> StartupSummary {
        StartupSummary::from(&nothing_to_recover())
    }
    fn fleet_agent_call(
        &self,
        objective: &str,
        credential: &str,
        action: &str,
        arguments: &Json,
    ) -> Result<Json, Unavailable> {
        if objective != self.objective {
            return Err(unavailable("remote-host-objective-refused"));
        }
        self.service.agent_call(credential, action, arguments)
    }
}

impl ReceivedWorkerHost {
    /// Consume the broker's original native handoff, initialize its independent workspace, commit
    /// launch intent and start one provider. Native policy supplies every execution parameter.
    /// The broker's final reply status is deliberately not an execution permission: this same
    /// original handoff survives a lost reply, while wire replies and retained receipts cannot
    /// construct it. The caller must retain the returned owner independently of any connection.
    ///
    /// Any failure preserves files and committed receipts for reconciliation. It consumes the
    /// handoff, never erases intent or reconstructs a reservation, and does not authorize a retry.
    pub fn start_received(
        handoff: RemoteReceivedHandoff,
        adapter: impl Into<NativeAdapter>,
        endpoint: &Path,
        signers: Arc<dyn WorkerSignerFactory>,
        reviewers: crate::TrustedReviewers,
        checkpoint: crate::CheckpointRuntimeParameters,
    ) -> Result<Self, Unavailable> {
        let adapter = adapter.into();
        let workspace = handoff
            .allocation
            .into_worker_workspace(reviewers, checkpoint)
            .map_err(|_| unavailable("remote-host-input-refused"))?;
        let reservation = handoff
            .registry
            .reserve_launch(
                workspace,
                adapter.provider(),
                super::service::received_clock()?,
            )
            .map_err(|_| unavailable("remote-host-launch-refused"))?;
        let RemoteLaunchOutcome::Reserved(reservation) = reservation else {
            return Err(unavailable("remote-host-launch-retained"));
        };
        Self::start(*reservation, adapter, endpoint, signers)
    }

    /// Bind native-configured private local IPC and launch the one originally admitted attempt.
    /// The endpoint and signer factory are native policy, never peer-selected input. The caller
    /// must retain this owner and poll it even when a broker connection disappears.
    pub fn start(
        reservation: RemoteLaunchReservation,
        adapter: impl Into<NativeAdapter>,
        endpoint: &Path,
        signers: Arc<dyn WorkerSignerFactory>,
    ) -> Result<Self, Unavailable> {
        let adapter = adapter.into();
        let receipt = reservation.receipt().clone();
        let work = receipt.admission().work();
        if adapter.provider() != work.provider {
            return Err(unavailable("remote-host-provider-mismatch"));
        }
        let parent = endpoint
            .parent()
            .ok_or_else(|| unavailable("remote-host-endpoint-refused"))?;
        let metadata = std::fs::symlink_metadata(parent)
            .map_err(|_| unavailable("remote-host-endpoint-refused"))?;
        if !endpoint.is_absolute()
            || !metadata.is_dir()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(unavailable("remote-host-endpoint-refused"));
        }
        let service = Arc::new(reservation.into_native_session()?);
        // A separate limited router avoids ownership cycles and general workspace authority.
        let router = Arc::new(ScopedRouter {
            objective: service.objective()?,
            service: service.clone(),
        });
        let server = IpcServer::bind(endpoint)
            .and_then(|server| server.spawn(router))
            .map_err(|_| unavailable("remote-host-endpoint-unavailable"))?;
        let mut host =
            NativeFleetHost::new(service.clone(), adapter, endpoint.to_path_buf(), signers)?;
        host.start_prepared(&work.lane, &work.run)?;
        Ok(Self {
            server,
            host,
            service,
            receipt,
        })
    }

    /// Original durable assignment and independent workspace mapping; not a retry permit.
    pub fn receipt(&self) -> &RemoteLaunchReceipt {
        &self.receipt
    }

    /// Native local endpoint supplied to the provider's scoped MCP bridge.
    pub fn endpoint(&self) -> &Path {
        self.server.endpoint()
    }

    /// Observe only this owner's existing process and persist existing local completion facts.
    /// Errors retain the owner. Lost connections or expired leases never cause another dispatch.
    pub fn poll(&mut self) -> Result<Vec<WorkerObservation>, Unavailable> {
        self.host.poll_owned()
    }

    /// Native cancellation request. It retains the uncertain slot until independent reconciliation.
    /// Further polls revoke credentials and stop the directly owned process; descendants may remain.
    pub fn request_cancel(&self) -> Result<(), Unavailable> {
        self.service
            .native_command("remote-host-cancel", Command::Cancel)
    }

    /// Native presentation facts. This is not an authenticated remote control endpoint.
    pub fn snapshot(&self) -> Result<Json, Unavailable> {
        self.service.snapshot()
    }
}

fn unavailable(code: &str) -> Unavailable {
    Unavailable::new(
        code,
        "The received native worker needs reconciliation before continuing.",
    )
}

#[cfg(test)]
mod tests;
