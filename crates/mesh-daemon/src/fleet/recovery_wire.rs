//! Explicit original recovery exchange. Native owners supply transport, policy and deadlines.
use super::*;
use crate::ipc::Json;
use mesh_cas::ContentDigest as _;
use mesh_crypto::{DomainSeparator, Ed25519, SignatureScheme, SigningPayload};
use mesh_types::{PublicKey, Signature};
use std::io::{self, Read, Write};

const REQUEST: &str = "mesh.worker-recovery-request/v1";
const PROOF: &str = "mesh.worker-recovery-proof/v1";
const COMMIT: &str = "mesh.worker-recovery-commit/v1";
const REPLY: &str = "mesh.worker-recovery-reply/v1";
const REQUEST_DOMAIN: DomainSeparator = DomainSeparator::new("mesh.v1.worker-recovery-request");
const PROOF_DOMAIN: DomainSeparator = DomainSeparator::new("mesh.v1.worker-recovery-proof");
const REPLY_DOMAIN: DomainSeparator = DomainSeparator::new("mesh.v1.worker-recovery-reply");
const MAX: usize = 65_536;
fn refused() -> io::Error {
    io::Error::other("original recovery exchange unavailable")
}
fn now() -> io::Result<u64> {
    service::received_clock().map_err(|_| refused())
}
fn text<'a>(v: &'a Json, k: &str) -> io::Result<&'a str> {
    v.get(k).and_then(Json::as_text).ok_or_else(refused)
}
fn number(v: &Json, k: &str) -> io::Result<u64> {
    v.get(k).and_then(Json::as_u64).ok_or_else(refused)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn bytes<const N: usize>(value: &str) -> io::Result<[u8; N]> {
    if value.len() != 2 * N
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(refused());
    }
    let mut result = [0; N];
    for (n, item) in result.iter_mut().enumerate() {
        *item = u8::from_str_radix(&value[n * 2..n * 2 + 2], 16).map_err(|_| refused())?;
    }
    Ok(result)
}
fn digest(value: &str) -> String {
    mesh_cas::Blake3::digest_bytes(value.as_bytes()).to_string()
}
fn closed(v: &Json, names: &[&str]) -> io::Result<Json> {
    let Json::Object(fields) = v else {
        return Err(refused());
    };
    if fields.len() != names.len()
        || names
            .iter()
            .any(|name| fields.iter().filter(|(key, _)| key == name).count() != 1)
    {
        return Err(refused());
    }
    Ok(Json::object(
        names
            .iter()
            .map(|name| (*name, v.get(name).unwrap().clone())),
    ))
}
fn limits_json(l: &Limits) -> Json {
    Json::object([
        ("lanes", Json::Number(l.lanes)),
        ("concurrency", Json::Number(l.concurrency)),
        ("depth", Json::Number(l.depth)),
        ("retries", Json::Number(l.retries)),
    ])
}
fn limits(v: &Json) -> io::Result<Limits> {
    closed(v, &["lanes", "concurrency", "depth", "retries"])?;
    let result = Limits {
        lanes: number(v, "lanes")?,
        concurrency: number(v, "concurrency")?,
        depth: number(v, "depth")?,
        retries: number(v, "retries")?,
    };
    limits_valid(&result).map_err(|_| refused())?;
    Ok(result)
}
fn target(coordinator: &str, objective: &str, work: &RemoteWork) -> Json {
    let a = &work.assignment;
    Json::object([
        ("coordinator", Json::text(coordinator)),
        ("worker", Json::text(&a.worker_key)),
        ("objective", Json::text(objective)),
        ("lane", Json::text(&work.lane)),
        ("run", Json::text(&work.run)),
        ("assignment", Json::text(&a.id)),
        ("input", Json::text(a.input.to_string())),
        ("bundle", Json::text(a.bundle.to_string())),
        ("provider", Json::text(&work.provider)),
        ("goal_digest", Json::text(digest(&work.goal))),
    ])
}
fn canonical_body(v: &Json) -> io::Result<Json> {
    closed(
        v,
        &[
            "target",
            "limits",
            "lease_sequence",
            "lease_until_ms",
            "nonce",
            "issued_ms",
            "expires_ms",
        ],
    )?;
    let t = closed(
        v.get("target").ok_or_else(refused)?,
        &[
            "coordinator",
            "worker",
            "objective",
            "lane",
            "run",
            "assignment",
            "input",
            "bundle",
            "provider",
            "goal_digest",
        ],
    )?;
    for key in ["coordinator", "worker", "input", "bundle", "goal_digest"] {
        bytes::<32>(text(&t, key)?)?;
    }
    for key in ["objective", "lane", "run", "assignment", "provider"] {
        id_valid(text(&t, key)?).map_err(|_| refused())?;
    }
    let l = limits(v.get("limits").ok_or_else(refused)?)?;
    bytes::<32>(text(v, "nonce")?)?;
    let sequence = number(v, "lease_sequence")?;
    let until = number(v, "lease_until_ms")?;
    let issued = number(v, "issued_ms")?;
    let expires = number(v, "expires_ms")?;
    if !(1..=4097).contains(&sequence)
        || issued == 0
        || expires <= issued
        || expires - issued > 30_000
        || expires > until
    {
        return Err(refused());
    }
    Ok(Json::object([
        ("target", t),
        ("limits", limits_json(&l)),
        ("lease_sequence", Json::Number(sequence)),
        ("lease_until_ms", Json::Number(until)),
        ("nonce", Json::text(text(v, "nonce")?)),
        ("issued_ms", Json::Number(issued)),
        ("expires_ms", Json::Number(expires)),
    ]))
}
fn fresh(body: &Json) -> io::Result<()> {
    let time = now()?;
    if time < number(body, "issued_ms")? || time >= number(body, "expires_ms")? {
        return Err(refused());
    }
    Ok(())
}
fn payload(domain: DomainSeparator, body: &Json) -> SigningPayload {
    SigningPayload::new(domain, body.encode().as_bytes())
}
fn envelope(schema: &str, body: &Json, signature: &Signature) -> String {
    Json::object([
        ("schema", Json::text(schema)),
        ("body", body.clone()),
        ("signature", Json::text(hex(signature.as_bytes()))),
    ])
    .encode()
}
fn signed(encoded: &str, schema: &str) -> io::Result<(Json, Signature)> {
    if encoded.len() > MAX {
        return Err(refused());
    }
    let v = Json::parse(encoded).map_err(|_| refused())?;
    let body = v.get("body").cloned().ok_or_else(refused)?;
    let signature = Signature::from_bytes(bytes::<64>(text(&v, "signature")?)?);
    if envelope(schema, &body, &signature) != encoded {
        return Err(refused());
    }
    Ok((body, signature))
}
fn frame(value: String) -> io::Result<RemoteFrame> {
    if value.len() > MAX {
        return Err(refused());
    }
    Ok(RemoteFrame::Control(value.into_bytes()))
}
fn control<R: Read>(reader: &mut RemoteFrameReader<R>) -> io::Result<String> {
    let Some(RemoteFrame::Control(bytes)) = reader.read_frame()? else {
        return Err(refused());
    };
    if bytes.len() > MAX {
        return Err(refused());
    }
    String::from_utf8(bytes).map_err(|_| refused())
}

/// Coordinator-signed, bounded request for one retained original attempt. No paths or executables.
pub struct RemoteWorkerRecoveryRequest {
    body: Json,
    signature: Signature,
}
impl RemoteWorkerRecoveryRequest {
    /// Canonical signed request. It is not a launch permit or proof of current worker state.
    pub fn encode(&self) -> String {
        envelope(REQUEST, &self.body, &self.signature)
    }
    /// Strictly parse bounded facts before native policy verification.
    pub fn decode(encoded: &str) -> io::Result<Self> {
        let (body, signature) = signed(encoded, REQUEST)?;
        if canonical_body(&body)? != body {
            return Err(refused());
        }
        Ok(Self { body, signature })
    }
    /// Authenticate against independently configured worker/coordinator, provider and caps.
    pub fn verify(
        self,
        policy: &RemoteDispatchPolicy<'_>,
    ) -> io::Result<VerifiedWorkerRecoveryRequest> {
        fresh(&self.body)?;
        let t = self.body.get("target").ok_or_else(refused)?;
        let l = limits(self.body.get("limits").ok_or_else(refused)?)?;
        limits_valid(&policy.maximum).map_err(|_| refused())?;
        if text(t, "coordinator")? != hex(policy.coordinator.as_bytes())
            || text(t, "worker")? != hex(policy.worker.as_bytes())
            || text(t, "provider")? != policy.provider
            || l.lanes > policy.maximum.lanes
            || l.concurrency > policy.maximum.concurrency
            || l.depth > policy.maximum.depth
            || l.retries > policy.maximum.retries
            || policy.max_lease_ms == 0
            || number(&self.body, "lease_until_ms")?.saturating_sub(now()?) > policy.max_lease_ms
        {
            return Err(refused());
        }
        Ed25519::verify(
            &policy.coordinator,
            payload(REQUEST_DOMAIN, &self.body).as_bytes(),
            &self.signature,
        )
        .map_err(|_| refused())?;
        Ok(VerifiedWorkerRecoveryRequest {
            request: self,
            limits: l,
            worker: policy.worker,
        })
    }
}
/// Verified native scope for looking up the original guarded registry. Still no mutable grant.
pub struct VerifiedWorkerRecoveryRequest {
    request: RemoteWorkerRecoveryRequest,
    limits: Limits,
    worker: PublicKey,
}
impl VerifiedWorkerRecoveryRequest {
    /// Authenticated coordinator namespace.
    pub fn coordinator(&self) -> &str {
        text(self.request.body.get("target").unwrap(), "coordinator").unwrap()
    }
    /// Authenticated objective namespace.
    pub fn objective(&self) -> &str {
        text(self.request.body.get("target").unwrap(), "objective").unwrap()
    }
    /// Original objective limits, already checked against native caps.
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
    /// Original assignment identity, never a storage path.
    pub fn assignment(&self) -> &str {
        text(self.request.body.get("target").unwrap(), "assignment").unwrap()
    }
    fn check(&self, registry: &RemoteAdmissionRegistry) -> io::Result<()> {
        fresh(&self.request.body)?;
        registry
            .verify_recovery_scope(
                self.coordinator(),
                &hex(self.worker.as_bytes()),
                self.objective(),
                &self.limits,
            )
            .map_err(|_| refused())?;
        let admission = registry
            .receipts()
            .map_err(|_| refused())?
            .into_iter()
            .find(|r| r.work().assignment.id == self.assignment())
            .ok_or_else(refused)?;
        let lease = registry
            .effective_lease(&admission)
            .map_err(|_| refused())?;
        if target(
            admission.coordinator(),
            admission.objective(),
            admission.work(),
        ) != *self.request.body.get("target").ok_or_else(refused)?
            || lease.sequence != number(&self.request.body, "lease_sequence")?
            || lease.until_ms != number(&self.request.body, "lease_until_ms")?
        {
            return Err(refused());
        }
        Ok(())
    }
}

mod client;
mod worker;
pub use client::{recover_remote_worker, RemoteRecoveryClientRequest, RemoteRecoveryReceipt};
pub use worker::{serve_remote_recovery, RemoteRecoveryBrokerOutcome, RemoteRecoveryWorkerRequest};

#[cfg(test)]
mod tests;
