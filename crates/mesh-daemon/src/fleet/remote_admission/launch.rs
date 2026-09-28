//! Durable remote launch ownership. Records are evidence, never reconstructed spawn permission.
use super::*;
use crate::fleet::ReceivedWorkerWorkspace;
use std::io::Read;

/// Retained launch intent. Cloning it grants no execution or workspace authority.
#[derive(Clone, PartialEq, Eq)]
pub struct RemoteLaunchReceipt {
    admission: RemoteAdmissionReceipt,
    owner: String,
    mapping: RecordDigest,
    initial: RecordDigest,
    installation: String,
}
impl RemoteLaunchReceipt {
    /// Exact original admission, including coordinator/objective scope and immutable work.
    pub fn admission(&self) -> &RemoteAdmissionReceipt {
        &self.admission
    }
    /// Native-generated attempt owner; a PID or a connection ID is not this identity.
    pub fn owner(&self) -> &str {
        &self.owner
    }
    /// Digest of the complete durable source-to-worker initialization receipt.
    pub fn workspace_mapping(&self) -> RecordDigest {
        self.mapping
    }
    /// Worker's actual saved initial operation, distinct from source ancestry.
    pub fn initial_operation(&self) -> RecordDigest {
        self.initial
    }
    /// Native installation observed when the intent was committed.
    pub fn installation(&self) -> &str {
        &self.installation
    }
}

/// Original committed launch intent plus retained ledger authority and initialized workspace.
/// This is not an agent credential. The native supervisor must still admit a session, custody,
/// provider process and scoped endpoint before spawning. Drop never erases intent or frees a slot.
pub struct RemoteLaunchReservation {
    registry: RemoteAdmissionRegistry,
    workspace: ReceivedWorkerWorkspace,
    receipt: RemoteLaunchReceipt,
}
impl RemoteLaunchReservation {
    /// Durable evidence; never another reservation.
    pub fn receipt(&self) -> &RemoteLaunchReceipt {
        &self.receipt
    }
    /// Retained native workspace for subsequent supervised session admission.
    pub fn workspace(&self) -> &ReceivedWorkerWorkspace {
        &self.workspace
    }
    /// Revalidate retained intent, immutable input/history/custody and the original lease.
    /// The supervisor supplies its current native clock immediately before a launch decision.
    /// This does not claim provider admission or a fresh inventory of mutable working files.
    pub fn verify(&self, now_ms: u64) -> Result<(), Error> {
        lease(&self.receipt.admission, now_ms)?;
        self.workspace
            .verify()
            .map_err(|_| Error::Refused("remote-launch-workspace-changed"))?;
        if self
            .registry
            .launch_receipt(&self.receipt.admission.work.assignment.id)?
            .as_ref()
            != Some(&self.receipt)
        {
            return refuse("remote-launch-intent-changed");
        }
        Ok(())
    }
}

/// Only the original atomic insertion retains resources for a later supervised launch.
pub enum RemoteLaunchOutcome {
    /// New committed intent, with its guarded ledger and native workspace still owned.
    Reserved(Box<RemoteLaunchReservation>),
    /// Existing intent, including after disconnect/expiry/restart. No launch reservation.
    Retained(Box<RemoteLaunchReceipt>),
}

impl RemoteAdmissionRegistry {
    /// Consume the native registry connection and originally initialized workspace. The reservation
    /// keeps both alive, including the worker-directory lock held by a guarded store. Caller-provided
    /// provider identity must come from an admitted native adapter, never an agent request.
    /// Uncertain writes and failed final revalidation retain intent and capacity, never retry spawn.
    pub fn reserve_launch(
        mut self,
        workspace: ReceivedWorkerWorkspace,
        provider: &str,
        now_ms: u64,
    ) -> Result<RemoteLaunchOutcome, Error> {
        workspace
            .verify()
            .map_err(|_| Error::Refused("remote-launch-workspace-changed"))?;
        let admission = workspace.admission().clone();
        if provider != admission.work.provider {
            return refuse("remote-launch-provider-mismatch");
        }
        let initial = workspace
            .binding()
            .starting_version()
            .ok_or(Error::InvalidHistory)?;
        let mapping = RecordDigest::from_bytes(
            *Blake3::digest_bytes(workspace.receipt().encode().as_bytes()).as_bytes(),
        );
        let mut nonce = [0_u8; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut nonce))
            .map_err(|_| Error::Refused("remote-launch-owner-unavailable"))?;
        let owner = RecordDigest::from_bytes(nonce).to_string();
        let proposed = RemoteLaunchReceipt {
            admission,
            owner,
            mapping,
            initial,
            installation: workspace.binding().installation().to_owned(),
        };
        let (receipt, inserted) = self.claim_launch_record(proposed, now_ms)?;
        if !inserted {
            return Ok(RemoteLaunchOutcome::Retained(Box::new(receipt)));
        }
        let reservation = RemoteLaunchReservation {
            registry: self,
            workspace,
            receipt,
        };
        reservation.verify(now_ms)?;
        Ok(RemoteLaunchOutcome::Reserved(Box::new(reservation)))
    }

    /// Read one bounded launch intent from the same guarded admission ledger. No lost workspace,
    /// missing process, expired lease or receipt replay can turn this fact into a reservation.
    pub fn launch_receipt(&self, assignment: &str) -> Result<Option<RemoteLaunchReceipt>, Error> {
        let admission = self.launch_admission(assignment)?;
        let events = self.store.events(&self.launch_stream(assignment), 0, 2)?;
        match events.as_slice() {
            [] => Ok(None),
            [event] => self.decode_launch(&admission, event).map(Some),
            _ => Err(Error::InvalidHistory),
        }
    }

    fn launch_admission(&self, assignment: &str) -> Result<RemoteAdmissionReceipt, Error> {
        self.receipts()?
            .into_iter()
            .find(|receipt| receipt.work.assignment.id == assignment)
            .ok_or(Error::Refused("remote-launch-admission-missing"))
    }
    fn launch_stream(&self, assignment: &str) -> String {
        let key = Json::object([
            ("admissions", Json::text(&self.stream)),
            ("assignment", Json::text(assignment)),
        ])
        .encode();
        format!("remote-launch-{}", Blake3::digest_bytes(key.as_bytes()))
    }
    fn encode_launch(&self, receipt: &RemoteLaunchReceipt) -> String {
        let admission = self.encode(&receipt.admission.work, &receipt.admission.allocation);
        Json::object([
            ("schema", Json::text("mesh.remote-launch-intent/v1")),
            (
                "admission",
                Json::text(Blake3::digest_bytes(admission.as_bytes()).to_string()),
            ),
            (
                "admission_revision",
                Json::Number(receipt.admission.revision),
            ),
            (
                "assignment",
                Json::text(&receipt.admission.work.assignment.id),
            ),
            ("owner", Json::text(&receipt.owner)),
            ("workspace_mapping", Json::text(receipt.mapping.to_string())),
            ("worker_initial", Json::text(receipt.initial.to_string())),
            ("installation", Json::text(&receipt.installation)),
        ])
        .encode()
    }
    fn decode_launch(
        &self,
        admission: &RemoteAdmissionReceipt,
        event: &FleetEvent,
    ) -> Result<RemoteLaunchReceipt, Error> {
        let value = Json::parse(&event.payload).map_err(|_| Error::InvalidHistory)?;
        let text = |key| {
            value
                .get(key)
                .and_then(Json::as_text)
                .map(str::to_owned)
                .ok_or(Error::InvalidHistory)
        };
        let digest = |key| RecordDigest::parse_hex(&text(key)?).map_err(|_| Error::InvalidHistory);
        let receipt = RemoteLaunchReceipt {
            admission: admission.clone(),
            owner: text("owner")?,
            mapping: digest("workspace_mapping")?,
            initial: digest("worker_initial")?,
            installation: text("installation")?,
        };
        validate(&receipt)?;
        if event.stream != self.launch_stream(&admission.work.assignment.id)
            || event.revision != 1
            || event.request != "launch"
            || self.encode_launch(&receipt) != event.payload
        {
            return Err(Error::InvalidHistory);
        }
        Ok(receipt)
    }
    fn claim_launch_record(
        &mut self,
        proposed: RemoteLaunchReceipt,
        now_ms: u64,
    ) -> Result<(RemoteLaunchReceipt, bool), Error> {
        validate(&proposed)?;
        let assignment = &proposed.admission.work.assignment.id;
        if self.launch_admission(assignment)? != proposed.admission {
            return refuse("remote-launch-admission-mismatch");
        }
        if let Some(retained) = self.launch_receipt(assignment)? {
            if retained.mapping != proposed.mapping
                || retained.initial != proposed.initial
                || retained.installation != proposed.installation
            {
                return refuse("remote-launch-workspace-mismatch");
            }
            return Ok((retained, false));
        }
        lease(&proposed.admission, now_ms)?;
        let outcome = self.store.append_with_outcome(
            &self.launch_stream(assignment),
            0,
            "launch",
            &self.encode_launch(&proposed),
        )?;
        let inserted = matches!(outcome, FleetAppendOutcome::Inserted(_));
        let receipt = self.decode_launch(&proposed.admission, &outcome.into_event())?;
        Ok((receipt, inserted))
    }
}
fn lease(admission: &RemoteAdmissionReceipt, now_ms: u64) -> Result<(), Error> {
    if now_ms == 0 || now_ms >= admission.work.assignment.lease_until_ms {
        return refuse("remote-launch-lease-expired");
    }
    Ok(())
}
fn validate(receipt: &RemoteLaunchReceipt) -> Result<(), Error> {
    if !RecordDigest::parse_hex(&receipt.owner).is_ok_and(|id| id.to_string() == receipt.owner)
        || receipt.installation.is_empty()
        || receipt.installation.len() > 4096
        || receipt.installation.contains('\0')
    {
        return Err(Error::InvalidHistory);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
