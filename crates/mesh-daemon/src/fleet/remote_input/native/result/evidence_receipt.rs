//! Durable worker attestation retention, bound to verified content and native store identity.
use super::*;
use crate::fleet::{AuthenticatedRemoteResultEvidence, RemoteResultEvidenceRequest};
use std::os::unix::fs::PermissionsExt as _;
const MAXIMUM: usize = 4_194_304;
fn refused() -> Error {
    Error::Refused("remote-result-evidence-receipt")
}
fn hash(raw: &str) -> RecordDigest {
    RecordDigest::from_bytes(*Blake3::digest_bytes(raw.as_bytes()).as_bytes())
}
fn text<'a>(v: &'a Json, name: &str) -> Result<&'a str, Error> {
    v.get(name).and_then(Json::as_text).ok_or_else(refused)
}
fn path(digest: RecordDigest) -> PathBuf {
    PathBuf::from(format!("result-evidence-{digest}.json"))
}
fn identity(content: RecordDigest, offer: RecordDigest, evidence: RecordDigest) -> Json {
    Json::object([
        (
            "schema",
            Json::text("mesh.remote-result-evidence-receipt/v1"),
        ),
        ("content", Json::text(content.to_string())),
        ("offer", Json::text(offer.to_string())),
        ("evidence", Json::text(evidence.to_string())),
    ])
}
fn payload(identity: Json, attestation: &str) -> String {
    Json::object([
        ("identity", identity),
        ("attestation", Json::text(attestation)),
    ])
    .encode()
}
/// Exact retained content/evidence correlation. This grants no candidate import, retention policy,
/// process-completion fact or protected-main approval.
pub struct RemoteResultEvidenceReceipt {
    digest: RecordDigest,
    content: RecordDigest,
    evidence: AuthenticatedRemoteResultEvidence,
}
impl RemoteResultEvidenceReceipt {
    /// Stable receipt identity, independent of a later query nonce or duplicate response.
    pub fn digest(&self) -> RecordDigest {
        self.digest
    }
    /// Exact content receipt, which also binds the native receiving store identity.
    pub fn content_receipt(&self) -> RecordDigest {
        self.content
    }
    /// Original retained worker attestation and fully validated immutable correspondence.
    pub fn evidence(&self) -> &AuthenticatedRemoteResultEvidence {
        &self.evidence
    }
}
impl<'a> NativeRemoteResultReceiver<'a> {
    fn verify_evidence(
        &self,
        runtime: &mut Runtime,
        input: &RemoteInputManifest,
        attestation: &str,
        raw: &str,
    ) -> Result<AuthenticatedRemoteResultEvidence, Error> {
        AuthenticatedRemoteResultEvidence::verify_retained(
            RemoteResultEvidenceRequest {
                status: RemoteWorkerStatusRequest {
                    runtime,
                    lane: &self.lane,
                    run: &self.run,
                    coordinator: self.coordinator,
                    worker: self.worker,
                },
                offer: &self.offer,
                input,
                result: &self.manifest,
            },
            attestation,
            raw,
        )
        .map_err(|_| refused())
    }
    fn load_evidence(
        &self,
        runtime: &mut Runtime,
        input: &RemoteInputManifest,
        content: RecordDigest,
    ) -> Result<Option<RemoteResultEvidenceReceipt>, Error> {
        self.check(runtime)?;
        let events = runtime
            .store
            .events(&format!("result-evidence-{content}"), 0, 2)?;
        let event = match events.as_slice() {
            [] => return Ok(None),
            [event] if event.revision == 1 && event.request == "evidence" => event,
            _ => return Err(refused()),
        };
        let value = Json::parse(&event.payload).map_err(|_| refused())?;
        let body = value.get("identity").ok_or_else(refused)?;
        let digest = RecordDigest::parse_hex(text(body, "evidence")?).map_err(|_| refused())?;
        let expected = identity(content, hash(&self.offer), digest);
        let attestation = text(&value, "attestation")?;
        if body != &expected || event.payload != payload(expected.clone(), attestation) {
            return Err(refused());
        }
        let (root, _) = self.destination.receiving_store().map_err(store_error)?;
        let bytes = super::receipt::read_metadata(&root, &path(digest), MAXIMUM)
            .map_err(store_error)?
            .ok_or_else(refused)?;
        let raw = std::str::from_utf8(&bytes).map_err(|_| refused())?;
        let evidence = self.verify_evidence(runtime, input, attestation, raw)?;
        if evidence.correspondence().digest() != digest {
            return Err(refused());
        }
        self.check(runtime)?;
        Ok(Some(RemoteResultEvidenceReceipt {
            digest: hash(&expected.encode()),
            content,
            evidence,
        }))
    }
    /// Retain exact authenticated evidence only alongside verified complete content. Replays keep
    /// the first valid attestation; a different descriptor for the same content refuses. Missing or
    /// partial recorded metadata is preserved and refused, never silently recreated.
    pub fn record_evidence_receipt(
        &self,
        runtime: &mut Runtime,
        input: &RemoteInputManifest,
        evidence: &AuthenticatedRemoteResultEvidence,
    ) -> Result<RemoteResultEvidenceReceipt, Error> {
        self.verify_complete(runtime)?;
        let verified = self.verify_evidence(
            runtime,
            input,
            evidence.attestation(),
            evidence.correspondence().encoded(),
        )?;
        let content = self.record_content_receipt(runtime)?.digest();
        if let Some(retained) = self.load_evidence(runtime, input, content)? {
            if retained.evidence.correspondence().digest() != verified.correspondence().digest() {
                return Err(refused());
            }
            self.verify_complete(runtime)?;
            return Ok(retained);
        }
        let digest = verified.correspondence().digest();
        let (root, _) = self.destination.receiving_store().map_err(store_error)?;
        let name = path(digest);
        if super::receipt::read_metadata(&root, &name, MAXIMUM)
            .map_err(store_error)?
            .is_none()
        {
            match root.filesystem().write_new_file(
                &name,
                verified.correspondence().encoded().as_bytes(),
                std::fs::Permissions::from_mode(0o600),
            ) {
                Ok(()) => (),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
                Err(error) => return Err(store_error(error)),
            }
        }
        if super::receipt::read_metadata(&root, &name, MAXIMUM)
            .map_err(store_error)?
            .as_deref()
            != Some(verified.correspondence().encoded().as_bytes())
        {
            return Err(refused());
        }
        root.filesystem()
            .read_only()
            .read_file(&name)
            .map_err(store_error)?
            .sync_all()
            .map_err(store_error)?;
        root.sync().map_err(store_error)?;
        self.verify_complete(runtime)?;
        let body = identity(content, hash(&self.offer), digest);
        let result = runtime.store.append_with_outcome(
            &format!("result-evidence-{content}"),
            0,
            "evidence",
            &payload(body, verified.attestation()),
        );
        let retained = match self.load_evidence(runtime, input, content)? {
            Some(value) if value.evidence.correspondence().digest() == digest => value,
            Some(_) => return Err(refused()),
            None => {
                result?;
                return Err(refused());
            }
        };
        self.verify_complete(runtime)?;
        Ok(retained)
    }
    /// Copy one exact retained result into an independent create-only private allocation.
    /// The supplied receipt is revalidated against durable state before and after copying.
    /// Missing metadata refuses without repair; existing/partial allocations are never adopted.
    pub fn materialize_result(
        &self,
        runtime: &mut Runtime,
        input: &RemoteInputManifest,
        receipt: &RemoteResultEvidenceReceipt,
        allocation_id: &str,
    ) -> Result<crate::fleet::RemoteResultAllocation, Error> {
        let verify = |runtime: &mut Runtime| -> Result<(), Error> {
            let content = self.verify_content_receipt(runtime)?.digest();
            let retained = self
                .load_evidence(runtime, input, content)?
                .ok_or_else(refused)?;
            if retained.digest() != receipt.digest() || content != receipt.content_receipt() {
                return Err(refused());
            }
            self.verify_complete(runtime)
        };
        verify(runtime)?;
        let allocation = self
            .destination
            .materialize_result(&self.manifest, &self.cas, allocation_id, receipt.digest())
            .map_err(store_error)?;
        verify(runtime)?;
        allocation.verify().map_err(store_error)?;
        Ok(allocation)
    }
    /// Reopen existing content and authenticated provenance after restart, without network access,
    /// execution reattachment or metadata repair. Every signature, context and content hash is checked.
    pub fn reopen_evidence_receipt(
        destination: &'a RemoteInputDestination,
        request: RemoteResultEvidenceRequest<'_>,
    ) -> Result<(Self, RemoteResultEvidenceReceipt), Error> {
        let status = request.status;
        let runtime = status.runtime;
        let (receiver, content) = Self::reopen_content_receipt(
            destination,
            request.offer,
            RemoteWorkerStatusRequest {
                runtime,
                lane: status.lane,
                run: status.run,
                coordinator: status.coordinator,
                worker: status.worker,
            },
        )?;
        if receiver.manifest() != request.result {
            return Err(refused());
        }
        let receipt = receiver
            .load_evidence(runtime, request.input, content.digest())?
            .ok_or_else(refused)?;
        receiver.verify_complete(runtime)?;
        Ok((receiver, receipt))
    }
}
