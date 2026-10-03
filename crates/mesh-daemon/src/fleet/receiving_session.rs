//! Supervisor-owned transfer state. Broker connections borrow it; they never own its lifetime.
use super::{
    refuse, Error, NativeRemoteInputReceiver, RemoteAdmissionChallenge, RemoteAdmissionOutcome,
    RemoteAdmissionProof, RemoteAdmissionReceipt, RemoteAdmissionRegistry, RemoteFrame,
    RemoteInputAllocation, RemoteInputDestination, RemoteInputManifest, RemoteInputReservation,
    RemoteWork,
};
use mesh_cas::Digest32;
use mesh_types::Signature;

/// Authenticated admission facts for the current connection, not a transferable permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteReceivingAccess {
    /// This supervisor holds the original reservation and may receive its input.
    Receiving,
    /// Admission exists but this supervisor did not create it; no reservation is reconstructed.
    Retained,
}

/// Durable transfer progress, returned only after native storage revalidation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteReceivingProgress {
    /// Exact manifest accepted. This is not complete input or launch readiness.
    Manifest,
    /// Confirmed CAS offset and completeness for a declared chunk.
    Chunk {
        /// Content identity, never a path.
        digest: Digest32,
        /// Confirmed durable offset, not merely bytes received from the connection.
        offset: u64,
        /// CAS has verified the declared complete chunk.
        complete: bool,
    },
}

/// One fixed assignment in a native supervisor, retained across broker connection lifetimes.
/// Native configuration supplies the guarded shared ledger, keys, limits, task and destination.
/// This is not a deployed endpoint or a restart adopter; retained receipts never recreate grants.
pub struct RemoteReceivingSession<'d> {
    registry: Option<RemoteAdmissionRegistry>,
    work: RemoteWork,
    allocation: String,
    destination: &'d RemoteInputDestination,
    receipt: Option<RemoteAdmissionReceipt>,
    reservation: Option<RemoteInputReservation>,
    receiver: Option<NativeRemoteInputReceiver<'d>>,
    terminal: bool,
}
impl<'d> RemoteReceivingSession<'d> {
    /// Retain native configuration. No peer authentication, admission or CAS creation occurs here.
    pub fn new(
        registry: RemoteAdmissionRegistry,
        work: RemoteWork,
        allocation: &str,
        destination: &'d RemoteInputDestination,
    ) -> Self {
        Self {
            registry: Some(registry),
            work,
            allocation: allocation.into(),
            destination,
            receipt: None,
            reservation: None,
            receiver: None,
            terminal: false,
        }
    }

    /// Borrow exclusive connection access and issue a fresh proof. Dropping the connection consumes
    /// its nonce and authentication, retaining the session's original reservation and partial CAS.
    pub fn connect(&mut self) -> Result<RemoteReceivingConnection<'_, 'd>, Error> {
        if self.terminal {
            return refuse("remote-receiving-terminal");
        }
        let registry = self.registry.take().ok_or(Error::InvalidHistory)?;
        let challenge = match registry.challenge(self.work.clone(), &self.allocation) {
            Ok(challenge) => challenge,
            Err(error) => {
                // No automatic replacement ledger or renewed reservation after setup uncertainty.
                self.terminal = true;
                return Err(error);
            }
        };
        Ok(RemoteReceivingConnection {
            session: self,
            challenge: Some(challenge),
            authenticated: false,
            refused: false,
        })
    }
}

/// Exclusive, non-cloneable broker connection borrowing supervisor state. No Debug: tasks are private.
/// The embedding broker must close on framing/schema errors and drop this guard. No socket is opened
/// here; authentication and typed frame routing are native and do not expose generic commands.
pub struct RemoteReceivingConnection<'s, 'd> {
    session: &'s mut RemoteReceivingSession<'d>,
    challenge: Option<RemoteAdmissionChallenge>,
    authenticated: bool,
    refused: bool,
}
impl RemoteReceivingConnection<'_, '_> {
    /// Canonical task-bearing challenge for the configured coordinator, never diagnostic output.
    pub fn proof(&self) -> Result<&RemoteAdmissionProof, Error> {
        if self.refused {
            return refuse("remote-receiving-connection-refused");
        }
        self.challenge
            .as_ref()
            .map(RemoteAdmissionChallenge::proof)
            .ok_or(Error::Refused("remote-receiving-proof-consumed"))
    }

    /// Consume this connection's challenge. Even a failed signature restores only native ledger
    /// ownership, so another connection can issue a fresh proof without losing retained input.
    pub fn authenticate(&mut self, signature: &Signature) -> Result<RemoteReceivingAccess, Error> {
        self.authenticated = false;
        if self.refused {
            return refuse("remote-receiving-connection-refused");
        }
        let challenge = self
            .challenge
            .take()
            .ok_or(Error::Refused("remote-receiving-proof-consumed"))?;
        let (registry, outcome) = challenge.verify_retaining(signature);
        self.session.registry = Some(registry);
        match outcome? {
            RemoteAdmissionOutcome::Reserved(reservation) => {
                if self.session.receipt.is_some() {
                    return refuse("remote-receiving-admission-changed");
                }
                self.session.receipt = Some(reservation.receipt().clone());
                self.session.reservation = Some(reservation);
            }
            RemoteAdmissionOutcome::Retained(receipt) => {
                if self
                    .session
                    .receipt
                    .as_ref()
                    .is_some_and(|expected| expected != &receipt)
                {
                    return refuse("remote-receiving-admission-changed");
                }
                self.session.receipt = Some(receipt);
            }
        }
        self.authenticated = true;
        Ok(if self.session.reservation.is_some() {
            RemoteReceivingAccess::Receiving
        } else {
            RemoteReceivingAccess::Retained
        })
    }

    /// Accept only an assigned manifest or chunk part after fresh authentication. Control messages
    /// are not executable here. Any refusal clears connection authentication; retain state and
    /// reconnect with a fresh challenge, never resynchronize/retry within the refused connection.
    pub fn receive(&mut self, frame: RemoteFrame) -> Result<RemoteReceivingProgress, Error> {
        let result = self.receive_inner(frame).and_then(|progress| {
            self.check()?;
            Ok(progress)
        });
        if result.is_err() {
            self.authenticated = false;
            self.refused = true;
        }
        result
    }

    fn receive_inner(&mut self, frame: RemoteFrame) -> Result<RemoteReceivingProgress, Error> {
        self.check()?;
        if self.session.reservation.is_none() {
            return refuse("remote-receiving-retained-only");
        }
        match frame {
            RemoteFrame::Manifest(bytes) => {
                let raw = std::str::from_utf8(&bytes)
                    .map_err(|_| Error::Refused("remote-input-format"))?;
                let assignment = &self.session.work.assignment;
                let manifest =
                    RemoteInputManifest::decode(raw, assignment.input, assignment.bundle)?;
                if self.session.receiver.is_none() {
                    self.session.receiver = Some(NativeRemoteInputReceiver::new(
                        self.session.destination,
                        manifest,
                        assignment,
                    )?);
                }
                // A repeated exact manifest does not replace the pinned receiver or partial offsets.
                Ok(RemoteReceivingProgress::Manifest)
            }
            RemoteFrame::Chunk {
                digest,
                offset,
                final_part,
                bytes,
            } => {
                let receiver = self
                    .session
                    .receiver
                    .as_mut()
                    .ok_or(Error::Refused("remote-receiving-manifest-required"))?;
                receiver.accept(digest, offset, &bytes, final_part)?;
                let (offset, complete) = receiver.status(digest)?;
                Ok(RemoteReceivingProgress::Chunk {
                    digest,
                    offset,
                    complete,
                })
            }
            RemoteFrame::Control(_) => refuse("remote-receiving-unexpected-control"),
        }
    }

    /// Authenticated, currently revalidated admission facts for a bounded broker reply.
    /// Reading or cloning this receipt cannot recreate the original reservation.
    pub fn receipt(&self) -> Result<&RemoteAdmissionReceipt, Error> {
        self.check()?;
        self.session.receipt.as_ref().ok_or(Error::InvalidHistory)
    }

    /// Read confirmed chunk state after reauthentication, not after a bare reconnect.
    pub fn status(&mut self, digest: Digest32) -> Result<(u64, bool), Error> {
        self.check()?;
        let status = self
            .session
            .receiver
            .as_mut()
            .ok_or(Error::Refused("remote-receiving-manifest-required"))?
            .status(digest)?;
        self.check()?;
        Ok(status)
    }

    /// Consume the original reservation into a verified native allocation and return the same
    /// guarded ledger for workspace/launch composition. Failure preserves partial work and is
    /// terminal for this session. This grants no provider launch or protected-main authority.
    pub fn materialize(
        &mut self,
    ) -> Result<(RemoteInputAllocation, RemoteAdmissionRegistry), Error> {
        self.check()?;
        self.session
            .receiver
            .as_mut()
            .ok_or(Error::Refused("remote-receiving-manifest-required"))?
            .verify_complete()?;
        self.check()?;
        let reservation = self
            .session
            .reservation
            .take()
            .ok_or(Error::Refused("remote-receiving-retained-only"))?;
        self.session.terminal = true;
        self.authenticated = false;
        let allocation = self
            .session
            .receiver
            .as_mut()
            .ok_or(Error::InvalidHistory)?
            .materialize_reserved(reservation)
            .map_err(|_| Error::Refused("remote-receiving-materialization"))?;
        self.facts()?;
        let mut registry = self.session.registry.take().ok_or(Error::InvalidHistory)?;
        registry.retain_materialization(&allocation)?;
        self.session.receiver = None;
        Ok((allocation, registry))
    }

    /// Continue only the original acknowledged initialization after fresh connection authentication.
    /// This returns the same guarded ledger and one exclusively held workspace, never a launch
    /// permit or an adopted process. Existing launch intent always requires separate reconciliation.
    pub fn resume_initialization(
        &mut self,
        reviewers: crate::TrustedReviewers,
        checkpoint: crate::CheckpointRuntimeParameters,
    ) -> Result<(super::ReceivedWorkerWorkspace, RemoteAdmissionRegistry), Error> {
        self.check()?;
        if self.session.reservation.is_some() || self.session.receiver.is_some() {
            return refuse("remote-recovery-original-transfer-owned");
        }

        let registry = self
            .session
            .registry
            .as_ref()
            .ok_or(Error::InvalidHistory)?;
        let admission = self.session.receipt.as_ref().ok_or(Error::InvalidHistory)?;
        let result = registry.recover_initialization(
            self.session.destination,
            admission,
            reviewers,
            checkpoint,
        );
        self.session.terminal = true;
        self.authenticated = false;
        let workspace = result?;
        let registry = self.session.registry.take().ok_or(Error::InvalidHistory)?;
        self.session.receiver = None;
        self.session.reservation = None;
        Ok((workspace, registry))
    }

    fn check(&self) -> Result<(), Error> {
        if !self.authenticated || self.refused || self.session.terminal {
            return refuse("remote-receiving-authentication-required");
        }
        self.facts()
    }

    fn facts(&self) -> Result<(), Error> {
        let now = super::service::received_clock()
            .map_err(|_| Error::Refused("remote-receiving-clock"))?;
        if self.session.work.assignment.lease_until_ms <= now {
            return refuse("remote-receiving-expired");
        }
        self.session
            .destination
            .verify()
            .map_err(|_| Error::Refused("remote-input-store-changed"))?;
        let receipt = self.session.receipt.as_ref().ok_or(Error::InvalidHistory)?;
        let registry = self
            .session
            .registry
            .as_ref()
            .ok_or(Error::InvalidHistory)?;
        if !registry.receipts()?.contains(receipt) {
            return refuse("remote-receiving-admission-changed");
        }
        Ok(())
    }
}
impl Drop for RemoteReceivingConnection<'_, '_> {
    fn drop(&mut self) {
        if let Some(challenge) = self.challenge.take() {
            self.session.registry = Some(challenge.abandon());
        }
        self.authenticated = false;
    }
}

#[cfg(test)]
pub(in crate::fleet) mod tests;
