//! Explicit expensive input observation; ordinary v1/v2 polling never opens input files.
use super::*;
use crate::fleet::RemoteInputDestination;

impl RemoteWorkerStatusChallenge {
    /// Request a fresh verification of the original retained input, not execution or retry rights.
    pub fn issue_with_input_inspection(
        runtime: &mut Runtime,
        lane: &str,
        run: &str,
        coordinator: PublicKey,
        worker: PublicKey,
    ) -> Result<Self, Error> {
        let mut challenge = Self::issue(runtime, lane, run, coordinator, worker)?;
        challenge.version = StatusVersion::Inspected;
        Ok(challenge)
    }
}
impl RemoteWorkerStatusReceipt {
    /// None denotes an older protocol, not missing input. "unrecorded" is uncertain historical
    /// evidence; "unavailable" means retained input could not be verified. "verified" proves only
    /// the original input at observation time, never current process ownership or safe execution.
    pub fn input_inspection(&self) -> Option<&str> {
        self.facts.get("input_inspection")?.as_text()
    }
}
impl VerifiedRemoteWorkerStatusQuery {
    pub(super) fn inspected_facts(
        &self,
        registry: &RemoteAdmissionRegistry,
        destination: Option<&RemoteInputDestination>,
    ) -> Result<Json, Error> {
        // Authenticate registry and exact assignment before any retained-path access.
        let base = self.facts(registry)?;
        if self.query.version == StatusVersion::Execution {
            return self.execution_facts(registry, base);
        }
        if self.query.version != StatusVersion::Inspected {
            return Ok(base);
        }
        let destination = destination.ok_or_else(invalid)?;
        let state = if matches!(base.get("admission"), Some(Json::Null)) {
            "unrecorded"
        } else {
            let assignment = text(
                self.query.body.get("target").ok_or_else(invalid)?,
                "assignment",
            )?;
            // Malformed ledger evidence refuses the whole reply; never fabricate absence.
            match registry.materialization_receipt(assignment)? {
                None => "unrecorded",
                Some(receipt) => match destination.inspect_materialization(&receipt) {
                    Ok(()) => "verified",
                    Err(_) => "unavailable",
                },
            }
        };
        canonical_inspected_facts(&Json::object([
            ("admission", base.get("admission").unwrap().clone()),
            ("launch", base.get("launch").unwrap().clone()),
            (
                "effective_lease",
                base.get("effective_lease").unwrap().clone(),
            ),
            ("input_inspection", Json::text(state)),
        ]))
    }
    /// v3 explicitly inspects original input before and after native signing. v1/v2 keep their
    /// original ledger-only behavior. No storage repair, reservation, lease or launch is granted.
    pub fn reply_with_input_inspection(
        &self,
        registry: &RemoteAdmissionRegistry,
        destination: &RemoteInputDestination,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> Result<RemoteFrame, Error> {
        let reply = self.reply_with_destination_at(registry, Some(destination), sign, now()?)?;
        fresh(&self.query.body, now()?)?;
        Ok(reply)
    }
}
pub(super) fn canonical_inspected_facts(v: &Json) -> Result<Json, Error> {
    closed(
        v,
        &["admission", "launch", "effective_lease", "input_inspection"],
    )?;
    let base = canonical_facts(
        &Json::object([
            ("admission", v.get("admission").ok_or_else(invalid)?.clone()),
            ("launch", v.get("launch").ok_or_else(invalid)?.clone()),
            (
                "effective_lease",
                v.get("effective_lease").ok_or_else(invalid)?.clone(),
            ),
        ]),
        StatusVersion::Effective,
    )?;
    let state = text(v, "input_inspection")?;
    if !matches!(state, "unrecorded" | "verified" | "unavailable")
        || (matches!(base.get("admission"), Some(Json::Null)) && state != "unrecorded")
    {
        return Err(invalid());
    }
    Ok(Json::object([
        ("admission", base.get("admission").unwrap().clone()),
        ("launch", base.get("launch").unwrap().clone()),
        (
            "effective_lease",
            base.get("effective_lease").unwrap().clone(),
        ),
        ("input_inspection", Json::text(state)),
    ]))
}
/// Explicit v3 input inspection over configured SSH. The bounded request may expire during a
/// large input scan. Failure remains uncertain; callers must not automatically dispatch again.
pub fn inspect_remote_worker_input_over_ssh(
    destination: &NativeSshDestination,
    request: RemoteWorkerStatusRequest<'_>,
    budget: Duration,
    sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<RemoteWorkerStatusReceipt> {
    inspect_version(destination, request, budget, sign, StatusVersion::Inspected)
}

#[cfg(test)]
mod tests;
