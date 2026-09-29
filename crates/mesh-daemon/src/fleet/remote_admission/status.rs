//! Fresh signed read-only recovery. Retained intent is never process/completion evidence.
use super::*;
use crate::fleet::{
    Lane, NativeSshDestination, RemoteDispatchPolicy, RemoteFrame, RemoteFrameReader,
    RemoteFrameWriter, Runtime,
};
use mesh_crypto::{DomainSeparator, Ed25519, SignatureScheme, SigningPayload};
use mesh_types::{PublicKey, Signature};
use std::io::{self, Read, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
const QUERY: &str = "mesh.worker-status-query/v1";
const REPLY: &str = "mesh.worker-status-reply/v1";
const QUERY_DOMAIN: DomainSeparator = DomainSeparator::new("mesh.v1.fleet-worker-status-query");
const REPLY_DOMAIN: DomainSeparator = DomainSeparator::new("mesh.v1.fleet-worker-status-reply");
#[derive(Clone, Copy, PartialEq, Eq)]
enum StatusVersion {
    Initial,
    Effective,
}
impl StatusVersion {
    fn query(self) -> &'static str {
        match self {
            Self::Initial => QUERY,
            Self::Effective => "mesh.worker-status-query/v2",
        }
    }
    fn reply(self) -> &'static str {
        match self {
            Self::Initial => REPLY,
            Self::Effective => "mesh.worker-status-reply/v2",
        }
    }
    fn query_domain(self) -> DomainSeparator {
        match self {
            Self::Initial => QUERY_DOMAIN,
            Self::Effective => DomainSeparator::new("mesh.v2.fleet-worker-status-query"),
        }
    }
    fn reply_domain(self) -> DomainSeparator {
        match self {
            Self::Initial => REPLY_DOMAIN,
            Self::Effective => DomainSeparator::new("mesh.v2.fleet-worker-status-reply"),
        }
    }
}
const MAX: usize = 65_536;
fn invalid() -> Error {
    Error::Refused("remote-status-unavailable")
}
fn now() -> Result<u64, Error> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|v| v.as_millis().try_into().ok())
        .ok_or_else(invalid)
}
fn text<'a>(v: &'a Json, k: &str) -> Result<&'a str, Error> {
    v.get(k).and_then(Json::as_text).ok_or_else(invalid)
}
fn number(v: &Json, k: &str) -> Result<u64, Error> {
    v.get(k).and_then(Json::as_u64).ok_or_else(invalid)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn bytes<const N: usize>(value: &str) -> Result<[u8; N], Error> {
    if value.len() != 2 * N
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let mut out = [0; N];
    for (n, item) in out.iter_mut().enumerate() {
        *item = u8::from_str_radix(&value[n * 2..n * 2 + 2], 16).map_err(|_| invalid())?;
    }
    Ok(out)
}
fn closed(value: &Json, names: &[&str]) -> Result<Json, Error> {
    let Json::Object(fields) = value else {
        return Err(invalid());
    };
    if fields.len() != names.len()
        || names
            .iter()
            .any(|n| fields.iter().filter(|(key, _)| key == n).count() != 1)
    {
        return Err(invalid());
    }
    Ok(Json::object(
        names
            .iter()
            .map(|name| (*name, value.get(name).unwrap().clone())),
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
fn parse_limits(v: &Json) -> Result<Limits, Error> {
    closed(v, &["lanes", "concurrency", "depth", "retries"])?;
    let l = Limits {
        lanes: number(v, "lanes")?,
        concurrency: number(v, "concurrency")?,
        depth: number(v, "depth")?,
        retries: number(v, "retries")?,
    };
    crate::fleet::limits_valid(&l)?;
    Ok(l)
}
fn identity(coordinator: &str, objective: &str, work: &RemoteWork) -> Json {
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
        (
            "goal_digest",
            Json::text(Blake3::digest_bytes(work.goal.as_bytes()).to_string()),
        ),
    ])
}
fn query_body(v: &Json) -> Result<Json, Error> {
    closed(v, &["target", "limits", "nonce", "issued_ms", "expires_ms"])?;
    let target = closed(
        v.get("target").ok_or_else(invalid)?,
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
    for k in ["coordinator", "worker", "input", "bundle", "goal_digest"] {
        bytes::<32>(text(&target, k)?)?;
    }
    for k in ["objective", "lane", "run", "assignment", "provider"] {
        id_valid(text(&target, k)?)?;
    }
    let limits = parse_limits(v.get("limits").ok_or_else(invalid)?)?;
    bytes::<32>(text(v, "nonce")?)?;
    let (issued, expires) = (number(v, "issued_ms")?, number(v, "expires_ms")?);
    if issued == 0 || expires.checked_sub(issued) != Some(30_000) {
        return Err(invalid());
    }
    Ok(Json::object([
        ("target", target),
        ("limits", limits_json(&limits)),
        ("nonce", Json::text(text(v, "nonce")?)),
        ("issued_ms", Json::Number(issued)),
        ("expires_ms", Json::Number(expires)),
    ]))
}
fn fresh(body: &Json, time: u64) -> Result<(), Error> {
    if time < number(body, "issued_ms")? || time >= number(body, "expires_ms")? {
        return Err(invalid());
    }
    Ok(())
}
fn envelope(schema: &str, body: Json, signature: &Signature) -> String {
    Json::object([
        ("schema", Json::text(schema)),
        ("body", body),
        ("signature", Json::text(hex(signature.as_bytes()))),
    ])
    .encode()
}
fn payload(domain: DomainSeparator, body: &Json) -> SigningPayload {
    SigningPayload::new(domain, body.encode().as_bytes())
}

/// Single-use current-attempt query. Issuing/verifying this challenge never changes a ledger.
pub struct RemoteWorkerStatusChallenge {
    version: StatusVersion,
    body: Json,
    lane: Lane,
    objective: String,
    limits: Limits,
    worker: PublicKey,
    coordinator: PublicKey,
}
impl RemoteWorkerStatusChallenge {
    /// Query even expired/cancelled retained work, using fresh native authentication. This cannot
    /// recover an unclaimed assignment, adopt a process, or infer that an absent record is safe.
    pub fn issue(
        runtime: &mut Runtime,
        lane: &str,
        run: &str,
        coordinator: PublicKey,
        worker: PublicKey,
    ) -> Result<Self, Error> {
        let mut nonce = [0; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut nonce))
            .map_err(|_| invalid())?;
        Self::issue_at(runtime, lane, run, coordinator, worker, now()?, nonce)
    }
    /// Request versioned current lease facts without changing or renewing any lease.
    /// Expired/cancelled work remains readable; this never grants another launch or retry.
    pub fn issue_with_current_lease(
        runtime: &mut Runtime,
        lane: &str,
        run: &str,
        coordinator: PublicKey,
        worker: PublicKey,
    ) -> Result<Self, Error> {
        let mut challenge = Self::issue(runtime, lane, run, coordinator, worker)?;
        challenge.version = StatusVersion::Effective;
        Ok(challenge)
    }
    fn issue_at(
        runtime: &mut Runtime,
        lane: &str,
        run: &str,
        coordinator: PublicKey,
        worker: PublicKey,
        time: u64,
        nonce: [u8; 32],
    ) -> Result<Self, Error> {
        runtime.refresh()?;
        let lane = runtime
            .state()
            .lanes
            .get(lane)
            .cloned()
            .ok_or_else(invalid)?;
        let current = lane
            .runs
            .last()
            .filter(|r| r.id == run)
            .ok_or_else(invalid)?;
        let assignment = current
            .remote
            .clone()
            .filter(|a| a.worker_key == hex(worker.as_bytes()))
            .ok_or_else(invalid)?;
        let work = RemoteWork {
            lane: lane.id.clone(),
            run: run.into(),
            assignment,
            provider: lane.provider.clone(),
            goal: lane.goal.clone(),
        };
        let limits = runtime.state().limits.clone().ok_or_else(invalid)?;
        let body = Json::object([
            (
                "target",
                identity(&hex(coordinator.as_bytes()), runtime.objective(), &work),
            ),
            ("limits", limits_json(&limits)),
            ("nonce", Json::text(hex(&nonce))),
            ("issued_ms", Json::Number(time)),
            (
                "expires_ms",
                Json::Number(time.checked_add(30_000).ok_or_else(invalid)?),
            ),
        ]);
        query_body(&body)?;
        Ok(Self {
            version: StatusVersion::Initial,
            body,
            lane,
            objective: runtime.objective().into(),
            limits,
            worker,
            coordinator,
        })
    }
    fn context(&self, runtime: &mut Runtime, time: u64) -> Result<(), Error> {
        fresh(&self.body, time)?;
        runtime.refresh()?;
        if runtime.objective() != self.objective
            || runtime.state().lanes.get(&self.lane.id) != Some(&self.lane)
            || runtime.state().limits.as_ref() != Some(&self.limits)
        {
            return Err(invalid());
        }
        Ok(())
    }
    /// Sign only this fresh native-derived query; no general peer-selected payload is accepted.
    pub fn signed_query(
        &self,
        runtime: &mut Runtime,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> Result<RemoteWorkerStatusQuery, Error> {
        self.context(runtime, now()?)?;
        let p = payload(self.version.query_domain(), &self.body);
        let signature = sign(&p).map_err(|_| invalid())?;
        Ed25519::verify(&self.coordinator, p.as_bytes(), &signature).map_err(|_| invalid())?;
        self.context(runtime, now()?)?;
        Ok(RemoteWorkerStatusQuery {
            version: self.version,
            body: self.body.clone(),
            signature,
        })
    }
    /// Consume the exact fresh query. A reply is read-only retained evidence, never execution rights.
    pub fn verify_reply(
        self,
        runtime: &mut Runtime,
        encoded: &str,
    ) -> Result<RemoteWorkerStatusReceipt, Error> {
        let body = self.body.clone();
        let result = self.verify_at(runtime, encoded, now()?)?;
        fresh(&body, now()?)?;
        Ok(result)
    }
    fn verify_at(
        self,
        runtime: &mut Runtime,
        encoded: &str,
        time: u64,
    ) -> Result<RemoteWorkerStatusReceipt, Error> {
        self.context(runtime, time)?;
        let v = parse_envelope(encoded, self.version.reply())?;
        let b = v.get("body").ok_or_else(invalid)?;
        closed(b, &["query", "observed_ms", "facts"])?;
        if text(b, "query")? != Blake3::digest_bytes(self.body.encode().as_bytes()).to_string()
            || number(b, "observed_ms")? < number(&self.body, "issued_ms")?
            || number(b, "observed_ms")? > time
        {
            return Err(invalid());
        }
        let facts = canonical_facts(b.get("facts").ok_or_else(invalid)?, self.version)?;
        if let Some(lease) = facts
            .get("effective_lease")
            .filter(|v| !matches!(v, Json::Null))
        {
            if number(lease, "accepted_ms")? > number(b, "observed_ms")? {
                return Err(invalid());
            }
        }
        let body = Json::object([
            ("query", Json::text(text(b, "query")?)),
            ("observed_ms", Json::Number(number(b, "observed_ms")?)),
            ("facts", facts.clone()),
        ]);
        let signature = Signature::from_bytes(bytes::<64>(text(&v, "signature")?)?);
        if envelope(self.version.reply(), body.clone(), &signature) != encoded {
            return Err(invalid());
        }
        Ed25519::verify(
            &self.worker,
            payload(self.version.reply_domain(), &body).as_bytes(),
            &signature,
        )
        .map_err(|_| invalid())?;
        self.context(runtime, time)?;
        Ok(RemoteWorkerStatusReceipt {
            target: self.body.get("target").ok_or_else(invalid)?.clone(),
            facts,
            observed_ms: number(&body, "observed_ms")?,
        })
    }
}
/// Native verified read-only facts. Null admission is unknown/unrecorded, not proof of no process.
pub struct RemoteWorkerStatusReceipt {
    target: Json,
    facts: Json,
    observed_ms: u64,
}
impl RemoteWorkerStatusReceipt {
    /// Exact verified coordinator/worker, objective, attempt, input and task/provider correlation.
    pub fn target(&self) -> &Json {
        &self.target
    }
    /// Signed admission and optional launch intent. Neither proves materialization or live execution.
    pub fn facts(&self) -> &Json {
        &self.facts
    }
    /// Whether this version reports effective leases. A v2 null value means no admission was
    /// recorded; neither null nor a historical v1 response authorizes retry or launch.
    pub fn reports_effective_lease(&self) -> bool {
        self.facts.get("effective_lease").is_some()
    }
    /// Current retained lease at observation time, never process liveness or launch permission.
    pub fn effective_lease(&self) -> Option<crate::fleet::RemoteWorkerLease> {
        let value = self.facts.get("effective_lease")?;
        Some(crate::fleet::RemoteWorkerLease {
            sequence: value.get("sequence")?.as_u64()?,
            until_ms: value.get("until_ms")?.as_u64()?,
            accepted_ms: value.get("accepted_ms")?.as_u64()?,
        })
    }
    /// Worker observation time, bounded by the consumed query; queued data does not stay fresh.
    pub fn observed_ms(&self) -> u64 {
        self.observed_ms
    }
}
/// Closed coordinator-signed status query. Decoding alone does not authenticate it.
pub struct RemoteWorkerStatusQuery {
    version: StatusVersion,
    body: Json,
    signature: Signature,
}
fn parse_envelope(encoded: &str, schema: &str) -> Result<Json, Error> {
    if encoded.len() > MAX {
        return Err(invalid());
    }
    let v = Json::parse(encoded).map_err(|_| invalid())?;
    closed(&v, &["schema", "body", "signature"])?;
    if text(&v, "schema")? != schema {
        return Err(invalid());
    }
    Ok(v)
}
impl RemoteWorkerStatusQuery {
    /// Canonical bounded private control bytes; never log them.
    pub fn encode(&self) -> String {
        envelope(self.version.query(), self.body.clone(), &self.signature)
    }
    /// One bounded control frame for the configured authenticated transport.
    pub fn frame(&self) -> Result<RemoteFrame, Error> {
        let bytes = self.encode().into_bytes();
        if bytes.len() > MAX {
            return Err(invalid());
        }
        Ok(RemoteFrame::Control(bytes))
    }
    /// Reject unknown fields, noncanonical encodings and unsupported versions before verification.
    pub fn decode(encoded: &str) -> Result<Self, Error> {
        if encoded.len() > MAX {
            return Err(invalid());
        }
        let schema = Json::parse(encoded).map_err(|_| invalid())?;
        let version = match text(&schema, "schema")? {
            QUERY => StatusVersion::Initial,
            "mesh.worker-status-query/v2" => StatusVersion::Effective,
            _ => return Err(invalid()),
        };
        let v = parse_envelope(encoded, version.query())?;
        let q = Self {
            version,
            body: query_body(v.get("body").ok_or_else(invalid)?)?,
            signature: Signature::from_bytes(bytes::<64>(text(&v, "signature")?)?),
        };
        if q.encode() != encoded {
            return Err(invalid());
        }
        Ok(q)
    }
    /// Check native keys/provider/caps and freshness. This never reserves or launches work.
    pub fn verify(
        self,
        policy: &RemoteDispatchPolicy<'_>,
    ) -> Result<VerifiedRemoteWorkerStatusQuery, Error> {
        self.verify_at(policy, now()?)
    }
    fn verify_at(
        self,
        policy: &RemoteDispatchPolicy<'_>,
        time: u64,
    ) -> Result<VerifiedRemoteWorkerStatusQuery, Error> {
        fresh(&self.body, time)?;
        let t = self.body.get("target").ok_or_else(invalid)?;
        let limits = parse_limits(self.body.get("limits").ok_or_else(invalid)?)?;
        crate::fleet::limits_valid(&policy.maximum)?;
        if text(t, "coordinator")? != hex(policy.coordinator.as_bytes())
            || text(t, "worker")? != hex(policy.worker.as_bytes())
            || text(t, "provider")? != policy.provider
            || limits.lanes > policy.maximum.lanes
            || limits.concurrency > policy.maximum.concurrency
            || limits.depth > policy.maximum.depth
            || limits.retries > policy.maximum.retries
        {
            return Err(invalid());
        }
        Ed25519::verify(
            &policy.coordinator,
            payload(self.version.query_domain(), &self.body).as_bytes(),
            &self.signature,
        )
        .map_err(|_| invalid())?;
        Ok(VerifiedRemoteWorkerStatusQuery {
            query: self,
            limits,
            worker: policy.worker,
        })
    }
}
/// Authenticated read-only query, scoped to one coordinator/objective and immutable work identity.
pub struct VerifiedRemoteWorkerStatusQuery {
    query: RemoteWorkerStatusQuery,
    limits: Limits,
    worker: PublicKey,
}
impl VerifiedRemoteWorkerStatusQuery {
    /// Verified coordinator namespace for the original guarded worker ledger.
    pub fn coordinator(&self) -> &str {
        text(self.query.body.get("target").unwrap(), "coordinator").unwrap()
    }
    /// Verified objective namespace, not a filesystem path.
    pub fn objective(&self) -> &str {
        text(self.query.body.get("target").unwrap(), "objective").unwrap()
    }
    /// Original signed objective limits, checked against native caps.
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
    fn facts(&self, registry: &RemoteAdmissionRegistry) -> Result<Json, Error> {
        if registry.coordinator != self.coordinator()
            || registry.objective != self.objective()
            || registry.worker != hex(self.worker.as_bytes())
            || registry.limits != self.limits
        {
            return Err(invalid());
        }
        let t = self.query.body.get("target").ok_or_else(invalid)?;
        let receipt = registry
            .receipts()?
            .into_iter()
            .find(|r| r.work.assignment.id == text(t, "assignment").unwrap());
        let Some(a) = receipt else {
            let mut facts = vec![("admission", Json::Null), ("launch", Json::Null)];
            if self.query.version == StatusVersion::Effective {
                facts.push(("effective_lease", Json::Null));
            }
            return canonical_facts(&Json::object(facts), self.query.version);
        };
        if identity(&registry.coordinator, &registry.objective, &a.work).encode() != t.encode() {
            return Err(invalid());
        }
        let admission = Json::object([
            ("allocation", Json::text(&a.allocation)),
            ("revision", Json::Number(a.revision)),
            (
                "initial_lease_sequence",
                Json::Number(a.work.assignment.lease_sequence),
            ),
            (
                "initial_lease_until_ms",
                Json::Number(a.work.assignment.lease_until_ms),
            ),
        ]);
        let launch = match registry.launch_receipt(&a.work.assignment.id)? {
            None => Json::Null,
            Some(l) => Json::object([
                ("owner", Json::text(l.owner())),
                (
                    "workspace_mapping",
                    Json::text(l.workspace_mapping().to_string()),
                ),
                (
                    "worker_initial",
                    Json::text(l.initial_operation().to_string()),
                ),
                ("installation", Json::text(l.installation())),
            ]),
        };
        let mut facts = vec![("admission", admission), ("launch", launch)];
        if self.query.version == StatusVersion::Effective {
            let current = registry.effective_lease(&a)?;
            facts.push((
                "effective_lease",
                Json::object([
                    ("sequence", Json::Number(current.sequence)),
                    ("until_ms", Json::Number(current.until_ms)),
                    ("accepted_ms", Json::Number(current.accepted_ms)),
                ]),
            ));
        }
        canonical_facts(&Json::object(facts), self.query.version)
    }
    /// Sign only current guarded ledger facts; recheck them after signing. No materialization,
    /// process adoption, retry permission, lease mutation or concurrency release occurs.
    pub fn reply(
        &self,
        registry: &RemoteAdmissionRegistry,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    ) -> Result<RemoteFrame, Error> {
        let reply = self.reply_at(registry, sign, now()?)?;
        fresh(&self.query.body, now()?)?;
        Ok(reply)
    }
    fn reply_at(
        &self,
        registry: &RemoteAdmissionRegistry,
        sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
        time: u64,
    ) -> Result<RemoteFrame, Error> {
        fresh(&self.query.body, time)?;
        let facts = self.facts(registry)?;
        let body = Json::object([
            (
                "query",
                Json::text(Blake3::digest_bytes(self.query.body.encode().as_bytes()).to_string()),
            ),
            ("observed_ms", Json::Number(time)),
            ("facts", facts.clone()),
        ]);
        let p = payload(self.query.version.reply_domain(), &body);
        let signature = sign(&p).map_err(|_| invalid())?;
        Ed25519::verify(&self.worker, p.as_bytes(), &signature).map_err(|_| invalid())?;
        if facts.encode() != self.facts(registry)?.encode() {
            return Err(invalid());
        }
        let encoded = envelope(self.query.version.reply(), body, &signature);
        if encoded.len() > MAX {
            return Err(invalid());
        }
        Ok(RemoteFrame::Control(encoded.into_bytes()))
    }
}
fn canonical_facts(v: &Json, version: StatusVersion) -> Result<Json, Error> {
    if version == StatusVersion::Initial {
        return canonical_base_facts(v);
    }
    closed(v, &["admission", "launch", "effective_lease"])?;
    let base = canonical_base_facts(&Json::object([
        ("admission", v.get("admission").ok_or_else(invalid)?.clone()),
        ("launch", v.get("launch").ok_or_else(invalid)?.clone()),
    ]))?;
    let admission = base.get("admission").ok_or_else(invalid)?;
    let value = v.get("effective_lease").ok_or_else(invalid)?;
    let lease = if matches!(admission, Json::Null) {
        if !matches!(value, Json::Null) {
            return Err(invalid());
        }
        Json::Null
    } else {
        let lease = closed(value, &["sequence", "until_ms", "accepted_ms"])?;
        let sequence = number(&lease, "sequence")?;
        let until = number(&lease, "until_ms")?;
        let accepted = number(&lease, "accepted_ms")?;
        let initial = number(admission, "initial_lease_until_ms")?;
        if !(1..=4097).contains(&sequence)
            || until < initial
            || (sequence == 1 && (until != initial || accepted != 0))
            || (sequence > 1 && (until <= initial || accepted == 0 || accepted >= until))
        {
            return Err(invalid());
        }
        lease
    };
    Ok(Json::object([
        ("admission", admission.clone()),
        ("launch", base.get("launch").ok_or_else(invalid)?.clone()),
        ("effective_lease", lease),
    ]))
}
fn canonical_base_facts(v: &Json) -> Result<Json, Error> {
    closed(v, &["admission", "launch"])?;
    let a = v.get("admission").ok_or_else(invalid)?;
    let admission = if matches!(a, Json::Null) {
        Json::Null
    } else {
        let a = closed(
            a,
            &[
                "allocation",
                "revision",
                "initial_lease_sequence",
                "initial_lease_until_ms",
            ],
        )?;
        bytes::<16>(text(&a, "allocation")?)?;
        if number(&a, "revision")? == 0
            || number(&a, "initial_lease_sequence")? != 1
            || number(&a, "initial_lease_until_ms")? == 0
        {
            return Err(invalid());
        }
        a
    };
    let l = v.get("launch").ok_or_else(invalid)?;
    let launch = if matches!(l, Json::Null) {
        Json::Null
    } else {
        if matches!(admission, Json::Null) {
            return Err(invalid());
        }
        let l = closed(
            l,
            &[
                "owner",
                "workspace_mapping",
                "worker_initial",
                "installation",
            ],
        )?;
        for k in ["owner", "workspace_mapping", "worker_initial"] {
            bytes::<32>(text(&l, k)?)?;
        }
        let i = text(&l, "installation")?;
        if i.is_empty() || i.len() > 4096 || i.contains('\0') {
            return Err(invalid());
        }
        l
    };
    Ok(Json::object([("admission", admission), ("launch", launch)]))
}
/// Native-selected status context. Keys and SSH destination never come from a worker reply.
pub struct RemoteWorkerStatusRequest<'a> {
    /// Current coordinator context; status verification does not submit any command.
    pub runtime: &'a mut Runtime,
    /// Exact current lane.
    pub lane: &'a str,
    /// Exact current run.
    pub run: &'a str,
    /// Independently configured coordinator execution key.
    pub coordinator: PublicKey,
    /// Independently configured worker execution key.
    pub worker: PublicKey,
}
/// Query retained facts over a single bounded SSH connection. An error remains uncertainty; even
/// a signed unrecorded response grants no retry, adoption or protected-main authority.
pub fn inspect_remote_worker_over_ssh(
    destination: &NativeSshDestination,
    request: RemoteWorkerStatusRequest<'_>,
    budget: Duration,
    sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<RemoteWorkerStatusReceipt> {
    inspect_version(destination, request, budget, sign, StatusVersion::Initial)
}
/// Read current retained lease facts over a fresh v2 query, without renewing or adopting work.
pub fn inspect_remote_worker_current_lease_over_ssh(
    destination: &NativeSshDestination,
    request: RemoteWorkerStatusRequest<'_>,
    budget: Duration,
    sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<RemoteWorkerStatusReceipt> {
    inspect_version(destination, request, budget, sign, StatusVersion::Effective)
}
fn inspect_version(
    destination: &NativeSshDestination,
    request: RemoteWorkerStatusRequest<'_>,
    budget: Duration,
    sign: impl FnOnce(&SigningPayload) -> Result<Signature, String>,
    version: StatusVersion,
) -> io::Result<RemoteWorkerStatusReceipt> {
    let err = |_| io::Error::other("remote worker status unavailable");
    let mut challenge = RemoteWorkerStatusChallenge::issue(
        request.runtime,
        request.lane,
        request.run,
        request.coordinator,
        request.worker,
    )
    .map_err(err)?;
    challenge.version = version;
    let query = challenge.signed_query(request.runtime, sign).map_err(err)?;
    let mut connection = destination.connect(budget)?;
    let (input, output) = connection.streams()?;
    exchange(request.runtime, challenge, query, input, output)
}
fn exchange(
    runtime: &mut Runtime,
    challenge: RemoteWorkerStatusChallenge,
    query: RemoteWorkerStatusQuery,
    input: impl Read,
    output: impl Write,
) -> io::Result<RemoteWorkerStatusReceipt> {
    let err = |_| io::Error::other("remote worker status unavailable");
    RemoteFrameWriter::new(output).write_frame(&query.frame().map_err(err)?)?;
    let Some(RemoteFrame::Control(reply)) = RemoteFrameReader::new(input).read_frame()? else {
        return Err(io::Error::other("remote worker status unavailable"));
    };
    challenge
        .verify_reply(
            runtime,
            std::str::from_utf8(&reply)
                .map_err(|_| io::Error::other("remote worker status unavailable"))?,
        )
        .map_err(err)
}
#[cfg(test)]
mod tests;

#[path = "renewal.rs"]
pub(in crate::fleet) mod renewal;

#[path = "result.rs"]
pub(in crate::fleet) mod result;
