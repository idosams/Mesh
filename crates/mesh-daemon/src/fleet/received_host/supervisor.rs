//! Resident ownership of received providers, independent of broker connection lifetimes.
use super::*;
use crate::fleet::RemoteAdmissionReceipt;
use std::path::PathBuf;

/// Native-admitted launch configuration. Never decoded from a peer or renderer message.
pub struct ReceivedWorkerLaunch {
    /// Independently admitted executable and bridge.
    pub adapter: NativeAdapter,
    /// Private native local endpoint for this one provider.
    pub endpoint: PathBuf,
    /// Native execution-signing custody, never a human approval key.
    pub signers: Arc<dyn WorkerSignerFactory>,
    /// Native review trust configuration for the independent worker history.
    pub reviewers: crate::TrustedReviewers,
    /// Native checkpoint policy.
    pub checkpoint: crate::CheckpointRuntimeParameters,
}

struct Entry {
    admission: RemoteAdmissionReceipt,
    owner: Option<ReceivedWorkerHost>,
}
/// One native resident owner's bounded collection of received providers.
/// The service drives `poll` independently of connection threads. Entries, including failed starts
/// and completed processes, retain their slots until explicit future reconciliation; there is no
/// remove, retry, PID adoption or receipt-to-reservation conversion API. Drop is not termination proof.
pub struct ReceivedWorkerSupervisor {
    maximum: usize,
    worker: String,
    entries: Vec<Entry>,
}
/// Correlated native observation. One unavailable owner does not suppress the others' observations.
pub struct ReceivedWorkerObservation {
    /// Exact admission facts; these are not a reservation or approval.
    pub admission: RemoteAdmissionReceipt,
    /// Poll result for this owner only. An error retains ownership and the occupied slot.
    pub observation: Result<Vec<WorkerObservation>, Unavailable>,
    /// Result of native saved-offer publication in this poll. None means no offer or error to report;
    /// an offered checkpoint is not content transfer, process completion or main approval.
    pub result_publication: Option<Result<String, Unavailable>>,
}
impl ReceivedWorkerSupervisor {
    /// Admit a fixed native resident capacity, independently of per-objective durable limits.
    pub fn new(maximum: usize, worker: mesh_types::PublicKey) -> Result<Self, Unavailable> {
        if !(1..=64).contains(&maximum) {
            return Err(unavailable("remote-supervisor-capacity"));
        }
        Ok(Self {
            maximum,
            worker: mesh_store::RecordDigest::from_bytes(*worker.as_bytes()).to_string(),
            entries: Vec::new(),
        })
    }
    /// Consume one original broker handoff and retain its provider independently of the connection.
    /// Duplicate assignment identities and exhausted capacity refuse before provider startup.
    /// After identity/capacity admission, failures retain a non-retryable slot. Every error consumes
    /// this handoff but preserves native files and durable receipts for reconciliation.
    pub fn start_received(
        &mut self,
        handoff: RemoteReceivedHandoff,
        launch: ReceivedWorkerLaunch,
    ) -> Result<RemoteAdmissionReceipt, Unavailable> {
        let admission = handoff
            .allocation
            .admission
            .as_ref()
            .ok_or_else(|| unavailable("remote-supervisor-admission"))?
            .clone();
        if admission.work().assignment.worker_key != self.worker {
            return Err(unavailable("remote-supervisor-worker-mismatch"));
        }
        if self
            .entries
            .iter()
            .any(|entry| same_assignment(&entry.admission, &admission))
        {
            return Err(unavailable("remote-supervisor-assignment-retained"));
        }
        if self.entries.len() >= self.maximum {
            return Err(unavailable("remote-supervisor-capacity"));
        }
        // Occupy the slot BEFORE any workspace, IPC or process effect. A failed start is never
        // removed, because absence of an owner cannot prove absence of an external effect.
        self.entries.push(Entry {
            admission: admission.clone(),
            owner: None,
        });
        let owner = ReceivedWorkerHost::start_received(
            handoff,
            launch.adapter,
            &launch.endpoint,
            launch.signers,
            launch.reviewers,
            launch.checkpoint,
        )?;
        self.entries
            .last_mut()
            .expect("slot inserted before start")
            .owner = Some(owner);
        Ok(admission)
    }
    /// Observe every retained owner without dispatching any new work. A failed poll is isolated.
    pub fn poll(&mut self) -> Vec<ReceivedWorkerObservation> {
        self.entries
            .iter_mut()
            .map(|entry| ReceivedWorkerObservation {
                admission: entry.admission.clone(),
                result_publication: None,
                observation: match entry.owner.as_mut() {
                    Some(owner) => owner.poll(),
                    None => Err(unavailable("remote-supervisor-start-uncertain")),
                },
            })
            .collect()
    }
    /// Observe providers and offer at most one saved review per eligible owner. Each owner applies
    /// its own bounded retry interval; one failed publication does not suppress other observations.
    #[cfg(target_os = "macos")]
    pub fn poll_with_result_publication(
        &mut self,
        signer: &dyn crate::CheckpointSigner,
    ) -> Vec<ReceivedWorkerObservation> {
        let mut observations = self.poll();
        for (entry, observation) in self.entries.iter_mut().zip(&mut observations) {
            if let Some(owner) = entry.owner.as_mut() {
                observation.result_publication = owner.publish_saved_result(signer);
            }
        }
        observations
    }

    /// Exact retained facts for native reconciliation, including failed starts and occupied slots.
    pub fn admissions(&self) -> Vec<RemoteAdmissionReceipt> {
        self.entries
            .iter()
            .map(|entry| entry.admission.clone())
            .collect()
    }
    /// Cancel only the exact retained admission. Native ownership and occupied slots remain retained.
    pub fn request_cancel(&self, admission: &RemoteAdmissionReceipt) -> Result<(), Unavailable> {
        self.owner(admission)?.request_cancel()
    }
    /// Native snapshot for an exact retained admission, not an authenticated network endpoint.
    pub fn snapshot(&self, admission: &RemoteAdmissionReceipt) -> Result<Json, Unavailable> {
        self.owner(admission)?.snapshot()
    }
    fn owner(
        &self,
        admission: &RemoteAdmissionReceipt,
    ) -> Result<&ReceivedWorkerHost, Unavailable> {
        self.entries
            .iter()
            .find(|entry| &entry.admission == admission)
            .and_then(|entry| entry.owner.as_ref())
            .ok_or_else(|| unavailable("remote-supervisor-owner-unavailable"))
    }
}
fn same_assignment(left: &RemoteAdmissionReceipt, right: &RemoteAdmissionReceipt) -> bool {
    left.coordinator() == right.coordinator()
        && left.objective() == right.objective()
        && left.work().assignment.id == right.work().assignment.id
}
