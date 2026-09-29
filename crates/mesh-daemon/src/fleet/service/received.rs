//! Native session composition for an original received assignment; no transport or process adoption.
use super::*;
use crate::fleet::{Limits, ReceivedWorkerWorkspace, RemoteLaunchReceipt};
use mesh_store::fleet::{FleetEvent, FleetStore};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) struct ReceivedSession {
    workspace: ReceivedWorkerWorkspace,
    receipt: RemoteLaunchReceipt,
    admission: FleetEvent,
    launch: FleetEvent,
}

impl ReceivedSession {
    fn scope(&self) -> String {
        // The launch stream already binds coordinator/objective/assignment. Domain separation
        // prevents session commands from being interpreted as admission or launch-intent events.
        format!(
            "remote-session-{}",
            Blake3::digest_bytes(self.launch.stream.as_bytes())
        )
    }

    pub(super) fn verify(&self, runtime: &Runtime) -> Result<(), Unavailable> {
        if runtime.objective() != self.scope() {
            return Err(refusal("remote-session-scope-changed"));
        }
        self.workspace
            .verify()
            .map_err(|_| refusal("remote-session-workspace-changed"))?;
        let store = &runtime.store;
        let admission = store
            .request(&self.admission.stream, &self.admission.request)
            .map_err(|_| refusal("remote-session-ledger-unavailable"))?;
        let launch = store
            .events(&self.launch.stream, 0, 2)
            .map_err(|_| refusal("remote-session-ledger-unavailable"))?;
        if admission.as_ref() != Some(&self.admission) || launch.as_slice() != [self.launch.clone()]
        {
            return Err(refusal("remote-session-intent-changed"));
        }
        Ok(())
    }

    pub(super) fn export_saved_review(
        &self,
        runtime: &Runtime,
        selection: &SavedReviewSelection,
    ) -> Result<crate::fleet::RemoteInputSource, Unavailable> {
        self.verify(runtime)?;
        let binding = saved_review_binding_from_state(runtime.state(), selection)?;
        let checkpoint = runtime
            .state()
            .checkpoints
            .get(&selection.checkpoint)
            .ok_or_else(|| refusal("remote-result-checkpoint-unavailable"))?;
        self.verify_run(&selection.lane, &checkpoint.origin.run)?;
        if binding != self.workspace.binding() {
            return Err(refusal("remote-result-workspace-changed"));
        }
        let source = self
            .workspace
            .export_saved_review(selection.bundle, selection.version)
            .map_err(|_| refusal("remote-result-content-unavailable"))?;
        self.verify(runtime)?;
        Ok(source)
    }

    pub(super) fn verify_run(&self, lane: &str, run: &str) -> Result<(), Unavailable> {
        let work = self.receipt.admission().work();
        if lane != work.lane || run != work.run {
            return Err(refusal("remote-session-attempt-mismatch"));
        }
        Ok(())
    }

    pub(super) fn verify_launch(
        &self,
        runtime: &Runtime,
        lane: &str,
        run: &str,
        provider: &str,
        now_ms: u64,
    ) -> Result<(), Unavailable> {
        self.verify(runtime)?;
        self.verify_run(lane, run)?;
        let work = self.receipt.admission().work();
        let lease = crate::fleet::remote_admission::lease::received_lease(
            &runtime.store,
            &self.admission,
            &work.assignment,
        )
        .map_err(|_| refusal("remote-session-lease-unavailable"))?;
        if now_ms == 0 || now_ms < lease.accepted_ms || now_ms >= lease.until_ms {
            return Err(refusal("remote-session-lease-expired"));
        }
        let state = runtime.state();
        let lane = state
            .lanes
            .get(lane)
            .ok_or_else(|| refusal("remote-session-lane-missing"))?;
        if provider != work.provider
            || lane.provider != work.provider
            || lane.goal != work.goal
            || lane.base != work.assignment.input
            || lane.workspace.as_ref() != Some(self.workspace.binding())
            || state.lanes.len() != 1
            || state.limits.as_ref() != Some(&limits())
        {
            return Err(refusal("remote-session-configuration-changed"));
        }
        Ok(())
    }
}

struct NoReceivedAllocation;
impl LaneAllocator for NoReceivedAllocation {
    fn allocate(&self, _: &str, _: &VersionInput) -> Result<LaneWorkspace, Unavailable> {
        Err(refusal("remote-session-allocation-not-authorized"))
    }
}

fn limits() -> Limits {
    // Remote child admission and retries belong to the coordinator's shared objective budget.
    Limits {
        lanes: 1,
        concurrency: 1,
        depth: 0,
        retries: 0,
    }
}

pub(in crate::fleet) fn received_clock() -> Result<u64, Unavailable> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| refusal("remote-session-clock-unavailable"))?
        .as_millis();
    u64::try_from(millis)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| refusal("remote-session-clock-unavailable"))
}

impl FleetService {
    /// Called only by the consumed original reservation, never reconstructed from retained facts.
    pub(in crate::fleet) fn from_received_parts(
        store: FleetStore,
        workspace: ReceivedWorkerWorkspace,
        receipt: RemoteLaunchReceipt,
        admission: FleetEvent,
        launch: FleetEvent,
    ) -> Result<Self, Unavailable> {
        let received = ReceivedSession {
            workspace,
            receipt,
            admission,
            launch,
        };
        let mut runtime = Runtime::open(store, &received.scope()).map_err(runtime_error)?;
        received.verify(&runtime)?;
        if runtime.state().revision != 0 {
            return Err(refusal("remote-session-needs-reconciliation"));
        }
        let work = received.receipt.admission().work();
        runtime
            .record(
                "start",
                Command::Start {
                    goal: work.goal.clone(),
                    limits: limits(),
                },
            )
            .map_err(runtime_error)?;
        runtime
            .record(
                "lane",
                Command::CreateLane {
                    id: work.lane.clone(),
                    parent: None,
                    goal: work.goal.clone(),
                    provider: work.provider.clone(),
                    base: work.assignment.input,
                },
            )
            .map_err(runtime_error)?;
        runtime
            .record(
                "workspace",
                Command::BindWorkspace {
                    lane: work.lane.clone(),
                    binding: received.workspace.binding().clone(),
                },
            )
            .map_err(runtime_error)?;
        runtime
            .record(
                "dispatch",
                Command::Dispatch {
                    lane: work.lane.clone(),
                    run: work.run.clone(),
                },
            )
            .map_err(runtime_error)?;
        received.verify_launch(
            &runtime,
            &work.lane,
            &work.run,
            &work.provider,
            received_clock()?,
        )?;
        let lane = work.lane.clone();
        let allocated = Arc::new(LaneWorkspace::from_received(&received.workspace));
        let service = Self::new(
            runtime,
            Arc::new(NoReceivedAllocation),
            [work.provider.clone()].into(),
        )?;
        {
            let mut inner = service.lock()?;
            inner.workspaces.insert(lane, allocated);
            inner.received = Some(received);
        }
        // Preserve every committed setup fact on error; a later constructor must not adopt it.
        drop(service.lock()?);
        Ok(service)
    }
}

#[cfg(test)]
mod tests;
