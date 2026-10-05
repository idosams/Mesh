//! Exact pending native-frame validation. No journal writes, admission or repair occur here.
use super::*;
use crate::project_attachment::dependency_transaction::{digest, text};
pub(in crate::project_attachment) fn pending_prefix(
    cas: &Cas<PinnedRootFs, Blake3>,
    intent: &str,
    identity: (u64, u64),
    journal: &[u8],
) -> io::Result<(usize, DependencyRecord, Vec<u8>)> {
    let value = Json::parse(intent).map_err(error)?;
    let kind = match text(&value, "schema")? {
        "mesh.dependency-decision-intent/v1" => DependencyKind::Eligibility,
        "mesh.dependency-grant-intent/v1" => DependencyKind::Grant,
        "mesh.dependency-review-intent/v1" => DependencyKind::ReviewSnapshot,
        "mesh.native-consumption-owner-commit/v1" => DependencyKind::Consumption,
        "mesh.native-consumption-start-intent/v1" => DependencyKind::ConsumptionStart,
        _ => return Err(invalid("unknown native control intent")),
    };
    let request = digest(text(&value, "request")?)?;
    let length = value
        .get("journal_bytes")
        .and_then(Json::as_u64)
        .filter(|n| *n <= journal.len() as u64)
        .ok_or_else(|| invalid("invalid decision prefix"))? as usize;
    let payload = digest(text(&value, "payload")?)?;
    if transaction_intent(kind, request, identity, &journal[..length], payload) != intent {
        return Err(invalid(
            "decision intent does not match pinned journal prefix",
        ));
    }
    let bytes = read_payload(cas, payload, 65_536)?;
    let decoded = Json::parse(std::str::from_utf8(&bytes).map_err(error)?).map_err(error)?;
    if decoded.get("kind").and_then(Json::as_u64) != Some(u64::from(kind.code())) {
        return Err(invalid("pending record is not an eligibility decision"));
    }
    let record = DependencyRecord {
        authority: digest(text(&decoded, "authority")?)?,
        revision: decoded
            .get("revision")
            .and_then(Json::as_u64)
            .ok_or_else(|| invalid("missing decision ordinal"))?,
        previous: RecordDigest::parse_hex(text(&decoded, "previous")?).map_err(error)?,
        payload,
        kind,
    };
    if decoded
        .get("body")
        .and_then(|body| body.get("request"))
        .and_then(Json::as_text)
        != Some(request.to_hex().as_str())
    {
        return Err(invalid("pending decision request does not match payload"));
    }
    let frame = frame_record(&StoredRecord::Dependency(record));
    if !frame.starts_with(&journal[length..]) {
        return Err(invalid("foreign bytes after decision prefix"));
    }
    Ok((length, record, bytes))
}

impl ProvisionedAttachment {
    /// Inspect the exact unfinished control request without applying its decision or repairing bytes.
    /// These are local policy recovery roots, not input access, publication or collection authority.
    pub fn inspect_pending_dependency_control_retention(
        &self,
        request: RecordDigest,
    ) -> io::Result<Json> {
        if request == ZERO {
            return Err(invalid("missing native control request"));
        }
        let guard =
            crate::workspace_custody::lock_workspace_initialization(&self.store).map_err(error)?;
        self.check_dependency_registration()?;
        self.attachment.ensure_current()?;
        crate::project_attachment::detachment::ensure_attached(&self.store)?;
        crate::project_attachment::history::dependency_capture::ensure_no_pending_capture(
            &self.store,
        )?;
        let raw = read_private_in_store(&self.store, PENDING)?;
        let value = Json::parse(&raw).map_err(error)?;
        if digest(text(&value, "request")?)? != request {
            return Err(invalid("another native control request owns this recovery"));
        }
        let (configuration, proof) = self.attachment.read_decision_configuration(
            self.metadata_path(),
            &self.store,
            Some(&raw),
        )?;
        let proof = proof.ok_or_else(|| invalid("native control enrollment is missing"))?;
        let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
            self.metadata_path(),
            self.store.filesystem().read_only(),
        )
        .map_err(error)?;
        let mut file = self
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(RECORD_FILE_NAME))?;
        let mut bytes = Vec::new();
        (&mut file)
            .take((MAX_JOURNAL + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_JOURNAL {
            return Err(invalid("native control journal exceeds bound"));
        }
        proof.verify(&self.store, &file, &bytes)?;
        let identity = (file.metadata()?.dev(), file.metadata()?.ino());
        let (prefix, record, staged_payload) = pending_prefix(&cas, &raw, identity, &bytes)?;
        if proof.pending() != Some((prefix, record)) {
            return Err(invalid("native control recovery record changed"));
        }
        let mut payloads = proof
            .policy()
            .policy_payloads()
            .collect::<std::collections::BTreeSet<_>>();
        let mut staged_policy = proof.policy().clone();
        staged_policy
            .apply(record, &staged_payload)
            .map_err(error)?;
        payloads.extend(staged_policy.policy_payloads());
        payloads.extend([proof.binding().authority, record.payload]);
        let store = self.store.identity()?;
        let facts = Json::object([
            ("schema", Json::text("mesh.native-control-retention/v1")),
            ("request", Json::text(request.to_hex())),
            ("authority", Json::text(proof.binding().authority.to_hex())),
            ("project", Json::text(proof.binding().project.to_hex())),
            (
                "installation",
                Json::text(proof.binding().installation.to_hex()),
            ),
            ("device", Json::text(format!("{:x}", store.0))),
            ("inode", Json::text(format!("{:x}", store.1))),
            ("journal_digest", Json::text(hash(&bytes).to_hex())),
            ("sidecar", Json::text(PENDING)),
            ("sidecar_digest", Json::text(hash(raw.as_bytes()).to_hex())),
            ("kind", Json::Number(u64::from(record.kind.code()))),
            (
                "written_frame_bytes",
                Json::Number((bytes.len() - prefix) as u64),
            ),
            (
                "payloads",
                Json::Array(payloads.iter().map(|p| Json::text(p.to_hex())).collect()),
            ),
        ]);
        if facts.encode().len() > 4 * 1024 * 1024 {
            return Err(invalid("native control recovery roots exceed bound"));
        }
        let (current, current_proof) = self.attachment.read_decision_configuration(
            self.metadata_path(),
            &self.store,
            Some(&raw),
        )?;
        if current != configuration
            || current_proof.as_ref() != Some(&proof)
            || read_private_in_store(&self.store, PENDING)? != raw
        {
            return Err(invalid("native control recovery evidence changed"));
        }
        self.check_dependency_registration()?;
        self.attachment.ensure_current()?;
        guard.ensure_current().map_err(error)?;
        Ok(facts)
    }
}
