use super::*;

/// Native-selected original attempt and independently admitted peer identities.
pub struct RemoteRecoveryClientRequest<'a> {
    /// Current coordinator ledger; cancellation/context changes are rechecked around every exchange.
    pub runtime: &'a mut Runtime,
    /// Exact existing lane.
    pub lane: &'a str,
    /// Exact current claimed attempt.
    pub run: &'a str,
    /// Native coordinator execution identity, never human approval authority.
    pub coordinator: PublicKey,
    /// Independently authenticated worker identity.
    pub worker: PublicKey,
}
/// Signed observation that original initialization was recovered, not evidence of provider execution.
pub struct RemoteRecoveryReceipt(Json);
impl RemoteRecoveryReceipt {
    /// Original request/proof correlation and worker mapping/initial-operation digests.
    pub fn correlation(&self) -> &Json {
        &self.0
    }
}
struct Context<'a> {
    request: RemoteRecoveryClientRequest<'a>,
    lane: Lane,
    limits: Limits,
    body: Json,
}
impl Context<'_> {
    fn check(&mut self) -> io::Result<()> {
        fresh(&self.body)?;
        self.request.runtime.refresh().map_err(|_| refused())?;
        let state = self.request.runtime.state();
        if state.cancelled
            || state.lanes.get(self.request.lane) != Some(&self.lane)
            || state.limits.as_ref() != Some(&self.limits)
        {
            return Err(refused());
        }
        Ok(())
    }
}
/// Complete the bounded signed exchange over owner-supplied authenticated streams and deadlines.
/// Any error requires closing the connection and inspecting retained facts; never automatically retry
/// execution. A lost final reply can coexist with a retained native handoff on the worker.
pub fn recover_remote_worker<R: Read, W: Write>(
    request: RemoteRecoveryClientRequest<'_>,
    input: R,
    output: W,
    mut sign: impl FnMut(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<RemoteRecoveryReceipt> {
    request.runtime.refresh().map_err(|_| refused())?;
    let state = request.runtime.state();
    let lane = state.lanes.get(request.lane).cloned().ok_or_else(refused)?;
    let run = lane.runs.last().ok_or_else(refused)?;
    let assignment = run.remote.as_ref().ok_or_else(refused)?;
    if state.cancelled
        || lane.workspace.is_none()
        || run.id != request.run
        || run.state != RunState::Launching
        || run.launch_owner.as_deref() != Some(format!("remote:{}", assignment.id).as_str())
        || assignment.worker_key != hex(request.worker.as_bytes())
        || lane.base != assignment.input
    {
        return Err(refused());
    }
    let l = state.limits.clone().ok_or_else(refused)?;
    let work = RemoteWork {
        lane: lane.id.clone(),
        run: run.id.clone(),
        assignment: assignment.clone(),
        provider: lane.provider.clone(),
        goal: lane.goal.clone(),
    };
    let time = now()?;
    let mut nonce = [0; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut nonce)?;
    let body = Json::object([
        (
            "target",
            target(
                &hex(request.coordinator.as_bytes()),
                request.runtime.objective(),
                &work,
            ),
        ),
        ("limits", limits_json(&l)),
        ("lease_sequence", Json::Number(assignment.lease_sequence)),
        ("lease_until_ms", Json::Number(assignment.lease_until_ms)),
        ("nonce", Json::text(hex(&nonce))),
        ("issued_ms", Json::Number(time)),
        (
            "expires_ms",
            Json::Number(
                time.checked_add(30_000)
                    .ok_or_else(refused)?
                    .min(assignment.lease_until_ms),
            ),
        ),
    ]);
    canonical_body(&body)?;
    let mut context = Context {
        request,
        lane,
        limits: l,
        body,
    };
    context.check()?;
    let signature = sign(&payload(REQUEST_DOMAIN, &context.body)).map_err(|_| refused())?;
    context.check()?;
    let request = RemoteWorkerRecoveryRequest {
        body: context.body.clone(),
        signature,
    };
    let request_hash = digest(&request.encode());
    let mut reader = RemoteFrameReader::new(input);
    let mut writer = RemoteFrameWriter::new(output);
    writer.write_frame(&frame(request.encode())?)?;
    let (body, signature) = signed(&control(&mut reader)?, PROOF)?;
    if closed(&body, &["request", "proof"])? != body || text(&body, "request")? != request_hash {
        return Err(refused());
    }
    Ed25519::verify(
        &context.request.worker,
        payload(PROOF_DOMAIN, &body).as_bytes(),
        &signature,
    )
    .map_err(|_| refused())?;
    let encoded = text(&body, "proof")?;
    let proof_hash = digest(encoded);
    let proof = RemoteRecoveryProof::decode(encoded).map_err(|_| refused())?;
    context.check()?;
    let signature = proof
        .sign_for(
            context.request.runtime,
            context.request.lane,
            context.request.run,
            &context.request.coordinator,
            &context.request.worker,
            &mut sign,
        )
        .map_err(|_| refused())?;
    context.check()?;
    let commit = Json::object([
        ("schema", Json::text(COMMIT)),
        ("request", Json::text(&request_hash)),
        ("proof", Json::text(&proof_hash)),
        ("signature", Json::text(hex(signature.as_bytes()))),
    ]);
    writer.write_frame(&frame(commit.encode())?)?;
    let (body, signature) = signed(&control(&mut reader)?, REPLY)?;
    if closed(&body, &["request", "proof", "mapping", "initial_operation"])? != body
        || text(&body, "request")? != request_hash
        || text(&body, "proof")? != proof_hash
    {
        return Err(refused());
    }
    for key in ["mapping", "initial_operation"] {
        bytes::<32>(text(&body, key)?)?;
    }
    Ed25519::verify(
        &context.request.worker,
        payload(REPLY_DOMAIN, &body).as_bytes(),
        &signature,
    )
    .map_err(|_| refused())?;
    context.check()?;
    Ok(RemoteRecoveryReceipt(body))
}
