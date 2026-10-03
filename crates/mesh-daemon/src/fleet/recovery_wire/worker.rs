use super::*;

/// Native worker configuration and original guarded registry. No peer chooses paths or trust.
pub struct RemoteRecoveryWorkerRequest<'a> {
    /// Fresh coordinator request, verified against independently configured native policy.
    pub request: VerifiedWorkerRecoveryRequest,
    /// Original installation-owned shared registry.
    pub registry: RemoteAdmissionRegistry,
    /// Independently admitted storage roots.
    pub destination: &'a RemoteInputDestination,
    /// Native trust policy for the recovered workspace.
    pub reviewers: crate::TrustedReviewers,
    /// Native checkpoint policy; never received over the wire.
    pub checkpoint: crate::CheckpointRuntimeParameters,
}
/// Native ownership survives final signature or reply failure. The caller must retain/queue it.
pub struct RemoteRecoveryBrokerOutcome {
    /// Exclusively held original workspace and guarded registry, delivered to native policy once.
    pub handoff: Box<RemoteRecoveredHandoff>,
    /// Local final write/flush success, not durable receipt or provider execution.
    pub reply_written: bool,
}
/// Serve one bounded recovery exchange after its signed initial request was verified. The caller
/// supplies authenticated transport/deadlines and independently admitted worker signing custody.
/// After recovery, every final-response error is returned with ownership instead of dropping it.
pub fn serve_remote_recovery<R: Read, W: Write>(
    native: RemoteRecoveryWorkerRequest<'_>,
    input: R,
    output: W,
    mut sign: impl FnMut(&SigningPayload) -> Result<Signature, String>,
) -> io::Result<RemoteRecoveryBrokerOutcome> {
    let RemoteRecoveryWorkerRequest {
        request,
        registry,
        destination,
        reviewers,
        checkpoint,
    } = native;
    request.check(&registry)?;
    let challenge = registry
        .recovery_challenge(request.assignment())
        .map_err(|_| refused())?;
    let request_hash = digest(&request.request.encode());
    let proof = challenge.proof().encode();
    let proof_hash = digest(&proof);
    let body = Json::object([
        ("request", Json::text(&request_hash)),
        ("proof", Json::text(&proof)),
    ]);
    let signature = sign(&payload(PROOF_DOMAIN, &body)).map_err(|_| refused())?;
    Ed25519::verify(
        &request.worker,
        payload(PROOF_DOMAIN, &body).as_bytes(),
        &signature,
    )
    .map_err(|_| refused())?;
    fresh(&request.request.body)?;
    let mut reader = RemoteFrameReader::new(input);
    let mut writer = RemoteFrameWriter::new(output);
    writer.write_frame(&frame(envelope(PROOF, &body, &signature))?)?;
    let encoded = control(&mut reader)?;
    let commit = Json::parse(&encoded).map_err(|_| refused())?;
    let signature = Signature::from_bytes(bytes::<64>(text(&commit, "signature")?)?);
    let canonical = Json::object([
        ("schema", Json::text(COMMIT)),
        ("request", Json::text(&request_hash)),
        ("proof", Json::text(&proof_hash)),
        ("signature", Json::text(hex(signature.as_bytes()))),
    ])
    .encode();
    if encoded != canonical {
        return Err(refused());
    }
    fresh(&request.request.body)?;
    let (workspace, registry) = challenge
        .recover(&signature, destination, reviewers, checkpoint)
        .map_err(|_| refused())?;
    let handoff = Box::new(RemoteRecoveredHandoff {
        workspace,
        registry,
    });
    let reply_written = (|| -> io::Result<()> {
        request.check(&handoff.registry)?;
        handoff.workspace.verify()?;
        let initial = handoff
            .workspace
            .binding()
            .starting_version()
            .ok_or_else(refused)?;
        let body = Json::object([
            ("request", Json::text(&request_hash)),
            ("proof", Json::text(&proof_hash)),
            (
                "mapping",
                Json::text(digest(&handoff.workspace.receipt().encode())),
            ),
            ("initial_operation", Json::text(initial.to_string())),
        ]);
        let signature = sign(&payload(REPLY_DOMAIN, &body)).map_err(|_| refused())?;
        Ed25519::verify(
            &request.worker,
            payload(REPLY_DOMAIN, &body).as_bytes(),
            &signature,
        )
        .map_err(|_| refused())?;
        request.check(&handoff.registry)?;
        handoff.workspace.verify()?;
        writer.write_frame(&frame(envelope(REPLY, &body, &signature))?)
    })()
    .is_ok();
    Ok(RemoteRecoveryBrokerOutcome {
        handoff,
        reply_written,
    })
}
