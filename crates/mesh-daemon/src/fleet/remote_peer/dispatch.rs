//! Authenticated bootstrap facts. Verification and proof replies never reserve or launch work.
use super::*;
use crate::fleet::{goal_valid, id_valid, limits_valid, Limits, RemoteFrame, RemoteWork};

const DISPATCH_DOMAIN: DomainSeparator = DomainSeparator::new("mesh.v1.fleet-coordinator-dispatch");
const MAX_BYTES: usize = 65_536;

/// Native worker policy. Keys, provider and caps must not come from the dispatch message.
pub struct RemoteDispatchPolicy<'a> {
    /// Independently admitted coordinator execution key, never a human approval key.
    pub coordinator: PublicKey,
    /// This worker's independently configured execution key.
    pub worker: PublicKey,
    /// Native-admitted provider identity.
    pub provider: &'a str,
    /// Worker-local upper bounds for the coordinator's objective.
    pub maximum: Limits,
    /// Maximum remaining initial assignment lease in milliseconds.
    pub max_lease_ms: u64,
}
/// Canonical coordinator-signed dispatch message. Task-bearing data is deliberately not Debug.
pub struct RemoteDispatch {
    body: Json,
    signature: Signature,
}
/// Authenticated task facts and narrowly scoped proof payload, not receiving or execution authority.
/// A replay can reproduce these facts; the durable admission/launch transactions still grant once.
pub struct VerifiedRemoteDispatch {
    coordinator: String,
    objective: String,
    work: RemoteWork,
    limits: Limits,
    worker: PublicKey,
    nonce: RecordDigest,
    payload: SigningPayload,
    issued_ms: u64,
    expires_ms: u64,
}
impl RemotePeerChallenge {
    /// Sign the exact pending native attempt and its objective limits with a separate coordinator
    /// domain. Native state is checked before and after signing. The original worker-proof bytes
    /// remain unchanged; dispatching this message itself performs no claim or external effect.
    pub fn signed_dispatch(
        &self,
        runtime: &mut Runtime,
        coordinator: &PublicKey,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> Result<RemoteDispatch, Error> {
        let limits = self.dispatch_context(runtime)?;
        let body = dispatch_body(coordinator, &self.peer, &limits, self.body.clone());
        let signature = sign(&SigningPayload::new(
            DISPATCH_DOMAIN,
            body.encode().as_bytes(),
        ))
        .map_err(|_| invalid())?;
        if self.dispatch_context(runtime)? != limits {
            return Err(invalid());
        }
        let dispatch = RemoteDispatch { body, signature };
        if dispatch.encode().len() > MAX_BYTES {
            return Err(invalid());
        }
        Ed25519::verify(
            coordinator,
            dispatch.payload().as_bytes(),
            &dispatch.signature,
        )
        .map_err(|_| invalid())?;
        Ok(dispatch)
    }
    fn dispatch_context(&self, runtime: &mut Runtime) -> Result<Limits, Error> {
        fresh(self.issued_ms, self.expires_ms, now_ms()?)?;
        runtime.refresh()?;
        if runtime.objective() != self.objective
            || runtime.state().cancelled
            || runtime.state().lanes.get(&self.lane.id) != Some(&self.lane)
        {
            return Err(invalid());
        }
        runtime.state().limits.clone().ok_or_else(invalid)
    }
    /// Consume this original single-use challenge after checking the canonical reply and its nonce.
    /// Successful proof claims coordinator-side ownership only; receiving and launch remain separate.
    pub fn verify_dispatch_reply(
        self,
        runtime: &mut Runtime,
        request: &str,
        encoded: &str,
    ) -> Result<FleetEvent, Error> {
        let signature = self.reply_signature(encoded)?;
        self.verify_and_claim(runtime, request, &signature)
    }
    fn reply_signature(&self, encoded: &str) -> Result<Signature, Error> {
        if encoded.len() > MAX_BYTES {
            return Err(invalid());
        }
        let value = Json::parse(encoded).map_err(|_| invalid())?;
        let nonce = RecordDigest::parse_hex(text(&self.body, "nonce")?).map_err(|_| invalid())?;
        let signature = signature(text(&value, "signature")?)?;
        if reply(nonce, &signature).encode() != encoded {
            return Err(invalid());
        }
        Ok(signature)
    }
}
impl RemoteInputReconnectChallenge {
    /// Sign fresh authenticated work facts using the existing v1 dispatch encoding. The worker
    /// still requires its fresh receiving proof and the original retained reservation.
    pub fn signed_dispatch(
        &self,
        runtime: &mut Runtime,
        coordinator: &PublicKey,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> Result<RemoteDispatch, Error> {
        self.0.signed_dispatch(runtime, coordinator, sign)
    }
    /// Consume the fresh proof and revalidate retained ownership, without submitting any command.
    /// Success establishes worker identity only; source transfer and receiving admission recheck
    /// their own current state. It is never a completion, lease renewal or launch grant.
    pub fn verify_dispatch_reply(self, runtime: &mut Runtime, encoded: &str) -> Result<(), Error> {
        let signature = self.0.reply_signature(encoded)?;
        self.0.verify_context_at(runtime, &signature, now_ms()?)
    }
}
impl RemoteDispatch {
    /// Canonical private control body; never log task content or signatures.
    pub fn encode(&self) -> String {
        Json::object([
            ("schema", Json::text("mesh.remote-dispatch/v1")),
            ("body", self.body.clone()),
            ("signature", Json::text(hex(self.signature.as_bytes()))),
        ])
        .encode()
    }
    /// Bounded control frame for already-authenticated transport with owner-supplied deadlines.
    pub fn frame(&self) -> Result<RemoteFrame, Error> {
        let bytes = self.encode().into_bytes();
        if bytes.len() > MAX_BYTES {
            return Err(invalid());
        }
        Ok(RemoteFrame::Control(bytes))
    }
    /// Decode the closed canonical envelope only; callers must verify native policy before use.
    pub fn decode(encoded: &str) -> Result<Self, Error> {
        if encoded.len() > MAX_BYTES {
            return Err(invalid());
        }
        let value = Json::parse(encoded).map_err(|_| invalid())?;
        let dispatch = Self {
            body: value.get("body").cloned().ok_or_else(invalid)?,
            signature: signature(text(&value, "signature")?)?,
        };
        if dispatch.encode() != encoded {
            return Err(invalid());
        }
        Ok(dispatch)
    }
    /// Verify coordinator identity, canonical work, freshness and native provider/budget/lease caps
    /// before exposing any worker signing payload. This does not create a ledger or allocation.
    pub fn verify(
        &self,
        policy: &RemoteDispatchPolicy<'_>,
    ) -> Result<VerifiedRemoteDispatch, Error> {
        self.verify_at(policy, now_ms()?)
    }
    fn verify_at(
        &self,
        policy: &RemoteDispatchPolicy<'_>,
        now: u64,
    ) -> Result<VerifiedRemoteDispatch, Error> {
        Ed25519::verify(
            &policy.coordinator,
            self.payload().as_bytes(),
            &self.signature,
        )
        .map_err(|_| invalid())?;
        limits_valid(&policy.maximum)?;
        id_valid(policy.provider)?;
        if policy.max_lease_ms == 0 {
            return Err(invalid());
        }
        let limits = parse_limits(self.body.get("limits").ok_or_else(invalid)?)?;
        limits_valid(&limits)?;
        if limits.lanes > policy.maximum.lanes
            || limits.concurrency > policy.maximum.concurrency
            || limits.depth > policy.maximum.depth
            || limits.retries > policy.maximum.retries
        {
            return Err(invalid());
        }
        let challenge = self.body.get("challenge").ok_or_else(invalid)?;
        let issued_ms = number(challenge, "issued_ms")?;
        let expires_ms = number(challenge, "expires_ms")?;
        fresh(issued_ms, expires_ms, now)?;
        let objective = text(challenge, "objective")?.to_owned();
        id_valid(&objective)?;
        let command = wire::decode(text(challenge, "claim")?)?;
        if wire::encode(&command) != text(challenge, "claim")? {
            return Err(invalid());
        }
        let Command::ClaimRemoteLaunch {
            lane,
            run,
            assignment,
        } = command
        else {
            return Err(invalid());
        };
        id_valid(&lane)?;
        id_valid(&run)?;
        assignment.validate()?;
        let provider = text(challenge, "provider")?.to_owned();
        let goal = text(challenge, "goal")?.to_owned();
        goal_valid(&goal)?;
        if provider != policy.provider
            || assignment.worker_key != key(&policy.worker)
            || expires_ms > assignment.lease_until_ms
            || !assignment
                .lease_until_ms
                .checked_sub(now)
                .is_some_and(|left| left > 0 && left <= policy.max_lease_ms)
        {
            return Err(invalid());
        }
        let nonce = RecordDigest::parse_hex(text(challenge, "nonce")?).map_err(|_| invalid())?;
        let expected = Json::object([
            ("schema", Json::text("mesh.fleet-worker-challenge/v1")),
            ("objective", Json::text(&objective)),
            (
                "claim",
                Json::text(wire::encode(&Command::ClaimRemoteLaunch {
                    lane: lane.clone(),
                    run: run.clone(),
                    assignment: assignment.clone(),
                })),
            ),
            ("provider", Json::text(&provider)),
            ("goal", Json::text(&goal)),
            ("nonce", Json::text(nonce.to_string())),
            ("issued_ms", Json::Number(issued_ms)),
            ("expires_ms", Json::Number(expires_ms)),
        ]);
        if &expected != challenge
            || dispatch_body(
                &policy.coordinator,
                &policy.worker,
                &limits,
                expected.clone(),
            ) != self.body
        {
            return Err(invalid());
        }
        Ok(VerifiedRemoteDispatch {
            coordinator: key(&policy.coordinator),
            objective,
            work: RemoteWork {
                lane,
                run,
                assignment,
                provider,
                goal,
            },
            limits,
            worker: policy.worker,
            nonce,
            payload: SigningPayload::new(DOMAIN, expected.encode().as_bytes()),
            issued_ms,
            expires_ms,
        })
    }
    fn payload(&self) -> SigningPayload {
        SigningPayload::new(DISPATCH_DOMAIN, self.body.encode().as_bytes())
    }
}
impl VerifiedRemoteDispatch {
    /// Configured coordinator identity authenticated by the dispatch signature.
    pub fn coordinator(&self) -> &str {
        &self.coordinator
    }
    /// Authenticated objective identity; no namespace or filesystem authority.
    pub fn objective(&self) -> &str {
        &self.objective
    }
    /// Exact immutable task facts for later native receiving admission.
    pub fn work(&self) -> &RemoteWork {
        &self.work
    }
    /// Authenticated objective limits already checked against native worker caps.
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
    /// Produce only this fresh, typed worker proof. The key must match the configured worker.
    /// Repeating this reply grants no reservation; the coordinator consumes its original challenge.
    pub fn worker_reply(
        &self,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> Result<RemoteFrame, Error> {
        fresh(self.issued_ms, self.expires_ms, now_ms()?)?;
        let signature = sign(&self.payload).map_err(|_| invalid())?;
        fresh(self.issued_ms, self.expires_ms, now_ms()?)?;
        Ed25519::verify(&self.worker, self.payload.as_bytes(), &signature)
            .map_err(|_| invalid())?;
        Ok(RemoteFrame::Control(
            reply(self.nonce, &signature).encode().into_bytes(),
        ))
    }
}
fn dispatch_body(
    coordinator: &PublicKey,
    worker: &PublicKey,
    limits: &Limits,
    challenge: Json,
) -> Json {
    Json::object([
        ("coordinator", Json::text(key(coordinator))),
        ("worker", Json::text(key(worker))),
        (
            "limits",
            Json::object([
                ("lanes", Json::Number(limits.lanes)),
                ("concurrency", Json::Number(limits.concurrency)),
                ("depth", Json::Number(limits.depth)),
                ("retries", Json::Number(limits.retries)),
            ]),
        ),
        ("challenge", challenge),
    ])
}
fn parse_limits(value: &Json) -> Result<Limits, Error> {
    Ok(Limits {
        lanes: number(value, "lanes")?,
        concurrency: number(value, "concurrency")?,
        depth: number(value, "depth")?,
        retries: number(value, "retries")?,
    })
}
fn fresh(issued: u64, expires: u64, now: u64) -> Result<(), Error> {
    if issued == 0
        || expires <= issued
        || expires - issued > LIFETIME_MS
        || now < issued
        || now >= expires
    {
        return Err(invalid());
    }
    Ok(())
}
fn reply(nonce: RecordDigest, signature: &Signature) -> Json {
    Json::object([
        ("schema", Json::text("mesh.worker-dispatch-reply/v1")),
        ("nonce", Json::text(nonce.to_string())),
        ("signature", Json::text(hex(signature.as_bytes()))),
    ])
}
fn text<'a>(value: &'a Json, name: &str) -> Result<&'a str, Error> {
    value.get(name).and_then(Json::as_text).ok_or_else(invalid)
}
fn number(value: &Json, name: &str) -> Result<u64, Error> {
    value.get(name).and_then(Json::as_u64).ok_or_else(invalid)
}
fn key(key: &PublicKey) -> String {
    hex(key.as_bytes())
}
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
fn signature(text: &str) -> Result<Signature, Error> {
    if text.len() != 128 {
        return Err(invalid());
    }
    let mut signature = [0; 64];
    for (target, pair) in signature.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        let digit = |b| match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            _ => Err(invalid()),
        };
        *target = digit(pair[0])? * 16 + digit(pair[1])?;
    }
    Ok(Signature::from_bytes(signature))
}
fn invalid() -> Error {
    Error::Refused("remote-dispatch-refused")
}
#[cfg(test)]
mod tests;
