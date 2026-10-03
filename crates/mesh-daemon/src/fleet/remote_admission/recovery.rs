//! Original initialization recovery is reachable only through an authenticated receiving connection.
use super::*;
use crate::fleet::{ReceivedWorkerWorkspace, RemoteInputDestination};
use crate::{CheckpointRuntimeParameters, TrustedReviewers};

impl RemoteAdmissionRegistry {
    pub(in crate::fleet) fn verify_recovery_scope(
        &self,
        coordinator: &str,
        worker: &str,
        objective: &str,
        limits: &Limits,
    ) -> Result<(), Error> {
        let _guarded = self.store.reopen_guarded_connection()?;
        if self.coordinator != coordinator
            || self.worker != worker
            || self.objective != objective
            || &self.limits != limits
        {
            return refuse("remote-recovery-registry-scope");
        }
        Ok(())
    }
    pub(in crate::fleet) fn recover_initialization(
        &self,
        destination: &RemoteInputDestination,
        admission: &RemoteAdmissionReceipt,
        reviewers: TrustedReviewers,
        checkpoint: CheckpointRuntimeParameters,
    ) -> Result<ReceivedWorkerWorkspace, Error> {
        self.recover_initialization_guarded(
            destination,
            admission,
            reviewers,
            checkpoint,
            || Ok(()),
        )
    }

    fn recover_initialization_guarded(
        &self,
        destination: &RemoteInputDestination,
        admission: &RemoteAdmissionReceipt,
        reviewers: TrustedReviewers,
        checkpoint: CheckpointRuntimeParameters,
        authorize: impl Fn() -> Result<(), Error>,
    ) -> Result<ReceivedWorkerWorkspace, Error> {
        authorize()?;
        // An ordinary path-opened ledger cannot promote a historical receipt into mutable authority.
        let _guarded = self.store.reopen_guarded_connection()?;
        let materialization = self
            .materialization_receipt(&admission.work.assignment.id)?
            .filter(|receipt| receipt.admission() == admission)
            .ok_or(Error::Refused("remote-recovery-materialization-missing"))?;
        let check = || -> Result<(), Error> {
            authorize()?;
            let now = super::super::service::received_clock()
                .map_err(|_| Error::Refused("remote-recovery-clock"))?;
            let lease = self.effective_lease(admission)?;
            if now == 0 || now < lease.accepted_ms || now >= lease.until_ms {
                return refuse("remote-recovery-lease-expired");
            }
            if self
                .launch_receipt(&admission.work.assignment.id)?
                .is_some()
            {
                return refuse("remote-recovery-launch-already-recorded");
            }
            if self
                .materialization_receipt(&admission.work.assignment.id)?
                .as_ref()
                != Some(&materialization)
            {
                return refuse("remote-recovery-materialization-changed");
            }
            Ok(())
        };
        check()?;
        let workspace = destination
            .recover_worker_workspace(&materialization, reviewers, checkpoint, || {
                check().map_err(|_| std::io::Error::other("original recovery authority changed"))
            })
            .map_err(|_| Error::Refused("remote-recovery-initialization-refused"))?;
        check()?;
        Ok(workspace)
    }
}

mod authentication;
pub use authentication::{RemoteRecoveryChallenge, RemoteRecoveryProof};
