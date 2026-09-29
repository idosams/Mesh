//! Immutable signed saved-result facts. No transfer, execution or main-approval authority.
use super::*;
use crate::fleet::{RemoteInputManifest, RemoteLaunchReceipt};
const SCHEMA: &str = "mesh.worker-saved-result/v1";
const DOMAIN: DomainSeparator = DomainSeparator::new("mesh.v1.worker-saved-result");

/// A worker-signed immutable offer. Receiving this does not prove that any content arrived.
/// Private task correlation must not be printed to general logs.
pub struct RemoteSavedResultOffer {
    body: Json,
    signature: Signature,
}
impl RemoteSavedResultOffer {
    pub(in crate::fleet) fn body(
        launch: &RemoteLaunchReceipt,
        checkpoint: &str,
        review: RecordDigest,
        manifest: &RemoteInputManifest,
    ) -> Result<Json, Error> {
        id_valid(checkpoint)?;
        let admission = launch.admission();
        Ok(Json::object([
            (
                "target",
                identity(
                    admission.coordinator(),
                    admission.objective(),
                    admission.work(),
                ),
            ),
            ("owner", Json::text(launch.owner())),
            (
                "mapping",
                Json::text(launch.workspace_mapping().to_string()),
            ),
            (
                "initial",
                Json::text(launch.initial_operation().to_string()),
            ),
            ("installation", Json::text(launch.installation())),
            ("checkpoint", Json::text(checkpoint)),
            ("review", Json::text(review.to_string())),
            ("version", Json::text(manifest.input().to_string())),
            ("manifest", Json::text(manifest.bundle().to_string())),
        ]))
    }
    pub(in crate::fleet) fn stream(body: &Json) -> Result<String, Error> {
        let scope = Json::object([
            ("target", body.get("target").ok_or_else(invalid)?.clone()),
            ("owner", Json::text(text(body, "owner")?)),
            ("checkpoint", Json::text(text(body, "checkpoint")?)),
        ]);
        Ok(format!(
            "remote-result-{}",
            Blake3::digest_bytes(scope.encode().as_bytes())
        ))
    }
    pub(in crate::fleet) fn sign(
        body: Json,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> Result<Self, Error> {
        let signature = sign(&payload(DOMAIN, &body)).map_err(|_| invalid())?;
        let offer = Self { body, signature };
        Self::decode(&offer.encode())
    }
    fn decode(encoded: &str) -> Result<Self, Error> {
        let v = parse_envelope(encoded, SCHEMA)?;
        let body = closed(
            v.get("body").ok_or_else(invalid)?,
            &[
                "target",
                "owner",
                "mapping",
                "initial",
                "installation",
                "checkpoint",
                "review",
                "version",
                "manifest",
            ],
        )?;
        // Reuse the closed assignment schema without granting a fresh-query capability.
        let query = Json::object([
            ("target", body.get("target").ok_or_else(invalid)?.clone()),
            (
                "limits",
                limits_json(&Limits {
                    lanes: 1,
                    concurrency: 1,
                    depth: 0,
                    retries: 0,
                }),
            ),
            ("nonce", Json::text("00".repeat(32))),
            ("issued_ms", Json::Number(1)),
            ("expires_ms", Json::Number(30_001)),
        ]);
        let target = query_body(&query)?
            .get("target")
            .ok_or_else(invalid)?
            .clone();
        if body.get("target") != Some(&target) {
            return Err(invalid());
        }
        for k in [
            "owner", "mapping", "initial", "review", "version", "manifest",
        ] {
            bytes::<32>(text(&body, k)?)?;
        }
        id_valid(text(&body, "checkpoint")?)?;
        let installation = text(&body, "installation")?;
        if installation.is_empty()
            || installation.len() > 256
            || installation.chars().any(char::is_control)
        {
            return Err(invalid());
        }
        let signature = Signature::from_bytes(bytes::<64>(text(&v, "signature")?)?);
        if envelope(SCHEMA, body.clone(), &signature) != encoded {
            return Err(invalid());
        }
        let worker = PublicKey::from_bytes(bytes::<32>(text(&target, "worker")?)?);
        Ed25519::verify(&worker, payload(DOMAIN, &body).as_bytes(), &signature)
            .map_err(|_| invalid())?;
        Ok(Self { body, signature })
    }
    /// Canonical bounded bytes for authenticated private transport; not a completion receipt.
    pub fn encode(&self) -> String {
        envelope(SCHEMA, self.body.clone(), &self.signature)
    }
    /// Verify against the coordinator's exact current assignment and the complete manifest.
    /// Expiry does not erase immutable results. No coordinator state or concurrency slot changes.
    pub fn verify(
        runtime: &mut Runtime,
        lane: &str,
        run: &str,
        coordinator: PublicKey,
        worker: PublicKey,
        encoded: &str,
        manifest: &RemoteInputManifest,
    ) -> Result<Self, Error> {
        let context = RemoteWorkerStatusChallenge::issue(runtime, lane, run, coordinator, worker)?;
        let offer = Self::decode(encoded)?;
        if offer.body.get("target") != context.body.get("target")
            || text(&offer.body, "version")? != manifest.input().to_string()
            || text(&offer.body, "manifest")? != manifest.bundle().to_string()
        {
            return Err(invalid());
        }
        context.context(runtime, now()?)?;
        Ok(offer)
    }
    pub(in crate::fleet) fn retained(
        store: &FleetStore,
        body: &Json,
    ) -> Result<Option<Self>, Error> {
        let stream = Self::stream(body)?;
        let events = store.events(&stream, 0, 2)?;
        match events.as_slice() {
            [] => Ok(None),
            [event] if event.revision == 1 && event.request == "offer" => {
                let offer = Self::decode(&event.payload)?;
                if &offer.body != body {
                    return Err(invalid());
                }
                Ok(Some(offer))
            }
            _ => Err(invalid()),
        }
    }
    pub(in crate::fleet) fn persist(self, store: &mut FleetStore) -> Result<Self, Error> {
        if let Some(retained) = Self::retained(store, &self.body)? {
            return Ok(retained);
        }
        let stream = Self::stream(&self.body)?;
        let result = store.append_with_outcome(&stream, 0, "offer", &self.encode());
        // A competing exact writer may have committed first. Verify its immutable signed fact.
        match Self::retained(store, &self.body)? {
            Some(retained) => Ok(retained),
            None => {
                result?;
                Err(invalid())
            }
        }
    }
}

impl RemoteAdmissionRegistry {
    /// Recover a retained signed offer after restart without reopening or launching execution.
    /// This returns immutable facts only; content availability must be verified separately.
    pub fn saved_result_offer(
        &self,
        assignment: &str,
        checkpoint: &str,
    ) -> Result<Option<RemoteSavedResultOffer>, Error> {
        id_valid(checkpoint)?;
        let Some(launch) = self.launch_receipt(assignment)? else {
            return Ok(None);
        };
        let admission = launch.admission();
        let target = identity(
            admission.coordinator(),
            admission.objective(),
            admission.work(),
        );
        let scope = Json::object([
            ("target", target.clone()),
            ("owner", Json::text(launch.owner())),
            ("checkpoint", Json::text(checkpoint)),
        ]);
        let stream = RemoteSavedResultOffer::stream(&scope)?;
        let events = self.store.events(&stream, 0, 2)?;
        let offer = match events.as_slice() {
            [] => return Ok(None),
            [event] if event.revision == 1 && event.request == "offer" => {
                RemoteSavedResultOffer::decode(&event.payload)?
            }
            _ => return Err(invalid()),
        };
        if offer.body.get("target") != Some(&target)
            || text(&offer.body, "owner")? != launch.owner()
            || text(&offer.body, "checkpoint")? != checkpoint
            || text(&offer.body, "mapping")? != launch.workspace_mapping().to_string()
            || text(&offer.body, "initial")? != launch.initial_operation().to_string()
            || text(&offer.body, "installation")? != launch.installation()
        {
            return Err(invalid());
        }
        Ok(Some(offer))
    }
}

#[cfg(test)]
#[path = "result/tests.rs"]
mod tests;

#[path = "result/query.rs"]
pub(in crate::fleet) mod query;
