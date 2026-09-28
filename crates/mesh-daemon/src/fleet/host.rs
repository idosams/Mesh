//! Native local worker scheduling. The embedding host drives ticks; no renderer process authority.
//!
//! Unowned launch attempts and cancelled process trees require reconciliation. This host never
//! retries them, releases workspace custody, approves reviews or advances shared state.

use mesh_types::{Blake3, ContentDigest};
use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use super::provider::{protocol::ProviderObservation, NativeAdapter, NativeProcess};
use super::service::{AgentCredential, CheckpointSigner, FleetService};
use super::{Command, RunState};
use crate::ipc::Unavailable;

/// Native key custody supplies a signer for each lane's session. Never exposed over IPC.
pub trait WorkerSignerFactory: Send + Sync {
    /// Issue a signer for this exact lane and attempt, or refuse before the provider starts.
    fn signer(&self, lane: &str, run: &str) -> Result<Arc<dyn CheckpointSigner>, Unavailable>;
}

/// Bounded provider facts for live native presentation, with an explicit observation time.
#[derive(Clone, Debug)]
pub struct WorkerObservation {
    /// Lane whose process this host owns.
    pub lane: String,
    /// Exact execution attempt.
    pub run: String,
    /// Time these process facts were polled; no background freshness is implied.
    pub observed_at: SystemTime,
    /// Redacted protocol activity.
    pub activity: ProviderObservation,
    /// Direct process/protocol outcome, not approval or proof of descendant termination.
    pub outcome: Option<bool>,
}

struct Worker {
    run: String,
    credential: AgentCredential,
    process: NativeProcess,
    stop_requested: bool,
    acknowledged: bool,
}

/// Automatically dispatches first attempts for lanes matching its admitted provider, including delegated children.
/// A new host never adopts uncertain runs from an earlier host or silently retries failed work.
pub struct NativeFleetHost {
    service: Arc<FleetService>,
    adapter: NativeAdapter,
    endpoint: PathBuf,
    signers: Arc<dyn WorkerSignerFactory>,
    identity: String,
    workers: BTreeMap<String, Worker>,
}

/// Compatibility name for the original host; native callers may admit either supported provider.
pub type CodexFleetHost = NativeFleetHost;

impl NativeFleetHost {
    /// Compose trusted native dependencies; starting work requires an explicit subsequent tick.
    pub fn new(
        service: Arc<FleetService>,
        adapter: impl Into<NativeAdapter>,
        endpoint: PathBuf,
        signers: Arc<dyn WorkerSignerFactory>,
    ) -> Result<Self, Unavailable> {
        if !endpoint.is_absolute() {
            return Err(unavailable("fleet-host-endpoint"));
        }
        let mut bytes = [0_u8; 16];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut bytes))
            .map_err(|_| unavailable("fleet-host-identity"))?;
        let identity = bytes.iter().map(|b| format!("{b:02x}")).collect();
        Ok(Self {
            service,
            adapter: adapter.into(),
            endpoint,
            signers,
            identity,
            workers: BTreeMap::new(),
        })
    }

    /// Poll owned workers and dispatch eligible lanes within the durable concurrency budget.
    /// The caller schedules ticks. Failure retains all owned handles and durable launch intent.
    pub fn tick(&mut self) -> Result<Vec<WorkerObservation>, Unavailable> {
        self.tick_with_dispatch(true)
    }

    /// Continue observation and cancellation after a host error without starting more workers.
    /// Existing handles and uncertain attempts remain owned; this is not restart reconciliation.
    pub fn poll_owned(&mut self) -> Result<Vec<WorkerObservation>, Unavailable> {
        self.tick_with_dispatch(false)
    }

    fn tick_with_dispatch(
        &mut self,
        dispatch: bool,
    ) -> Result<Vec<WorkerObservation>, Unavailable> {
        let state = self.service.native_state()?;
        let mut observations = Vec::with_capacity(self.workers.len());
        for (lane, worker) in &mut self.workers {
            let cancelled = state.cancelled
                || state
                    .lanes
                    .get(lane)
                    .and_then(|lane| lane.runs.last())
                    .is_some_and(|run| run.id == worker.run && run.state == RunState::Stopping);
            if cancelled && !worker.acknowledged {
                self.service.revoke(&worker.credential)?;
                if !worker.stop_requested {
                    // Keep the slot and custody even if the direct process exits. Descendants may live.
                    // A failed stop request must remain retryable on the next native tick.
                    worker.stop_requested = worker.process.request_stop().is_ok();
                }
            }
            let (activity, outcome) = worker
                .process
                .poll()
                .map_err(|_| unavailable("fleet-host-observation"))?;
            if let Some(success) = outcome.filter(|_| !cancelled && !worker.acknowledged) {
                self.service.revoke(&worker.credential)?;
                worker.acknowledged =
                    self.service
                        .record_provider_completion(lane, &worker.run, success)?;
            }
            observations.push(WorkerObservation {
                lane: lane.clone(),
                run: worker.run.clone(),
                observed_at: SystemTime::now(),
                activity,
                outcome,
            });
        }
        // Refresh after terminal acknowledgments, so freed slots are usable in the same tick.
        let state = self.service.native_state()?;
        if state.cancelled || !dispatch {
            return Ok(observations);
        }
        let limit = state
            .limits
            .as_ref()
            .ok_or_else(|| unavailable("fleet-host-not-started"))?
            .concurrency;
        let occupied = state
            .lanes
            .values()
            .flat_map(|lane| &lane.runs)
            .filter(|run| run.state.occupies_slot())
            .count() as u64;
        for lane in state
            .lanes
            .values()
            .filter(|lane| {
                lane.provider == self.adapter.provider()
                    && lane.workspace.is_some()
                    && lane.runs.is_empty()
            })
            .take(limit.saturating_sub(occupied) as usize)
        {
            // Per-host names make concurrent dispatches conflict instead of sharing a grant/session.
            let run = format!(
                "{}-{}",
                self.identity,
                Blake3::digest_bytes(lane.id.as_bytes()).to_hex()
            );
            self.service.native_command(
                &format!("dispatch-{run}"),
                Command::Dispatch {
                    lane: lane.id.clone(),
                    run: run.clone(),
                },
            )?;
            let signer = self.signers.signer(&lane.id, &run)?;
            let credential = self
                .service
                .grant_with_signer(&lane.id, &run, &run, signer)?;
            let process =
                match self
                    .service
                    .start_provider(&credential, &self.adapter, &self.endpoint)
                {
                    Ok(process) => process,
                    Err(error) => {
                        // A failed launch may have crossed the external-effect boundary. Preserve its
                        // durable claim and custody, but never leave a failed session authorized.
                        let _ = self.service.revoke(&credential);
                        return Err(error);
                    }
                };
            self.workers.insert(
                lane.id.clone(),
                Worker {
                    run,
                    credential,
                    process,
                    stop_requested: false,
                    acknowledged: false,
                },
            );
        }
        Ok(observations)
    }
}

fn unavailable(code: &str) -> Unavailable {
    Unavailable::new(
        code,
        "The native fleet host needs reconciliation before this action can continue.",
    )
}
