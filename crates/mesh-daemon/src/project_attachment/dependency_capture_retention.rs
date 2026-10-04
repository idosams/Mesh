//! Read-only capture recovery roots, including staged objects beyond a torn journal prefix.
use super::*;
use mesh_store::StoredRecord;

/// Exact local recovery objects, not a whole-project collection plan or publication permission.
/// The sidecar and journal remain required alongside these CAS roots. No durable pin is acquired.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeCaptureRetention {
    operation: RecordDigest,
    request: RecordDigest,
    store: (u64, u64),
    authority: RecordDigest,
    journal: RecordDigest,
    sidecar: String,
    sidecar_digest: RecordDigest,
    pending: bool,
    payloads: BTreeSet<RecordDigest>,
    manifests: BTreeSet<RecordDigest>,
}
impl NativeCaptureRetention {
    /// Exact operation this request can recover; its presence does not imply completed commit.
    pub fn operation(&self) -> RecordDigest {
        self.operation
    }
    /// Whether the request still has a durable unfinished-capture intent.
    pub fn pending(&self) -> bool {
        self.pending
    }
    /// Immutable local recovery facts. Other work and other transaction types require their own roots.
    pub fn to_json(&self) -> Json {
        Json::object([
            ("schema", Json::text("mesh.native-capture-retention/v1")),
            ("operation", Json::text(self.operation.to_hex())),
            ("request", Json::text(self.request.to_hex())),
            ("device", Json::text(format!("{:x}", self.store.0))),
            ("inode", Json::text(format!("{:x}", self.store.1))),
            ("authority", Json::text(self.authority.to_hex())),
            ("journal_digest", Json::text(self.journal.to_hex())),
            ("sidecar", Json::text(&self.sidecar)),
            ("sidecar_digest", Json::text(self.sidecar_digest.to_hex())),
            ("pending", Json::Bool(self.pending)),
            (
                "payloads",
                Json::Array(
                    self.payloads
                        .iter()
                        .map(|d| Json::text(d.to_hex()))
                        .collect(),
                ),
            ),
            (
                "manifests",
                Json::Array(
                    self.manifests
                        .iter()
                        .map(|d| Json::text(d.to_hex()))
                        .collect(),
                ),
            ),
        ])
    }
}
impl ProvisionedAttachment {
    /// Inspect an exact pending or completed capture request without recovering it or changing bytes.
    /// Verifies all local authoring objects in the reconstructed journal and staged frame set.
    /// Cross-work consumption and pending control transactions are not authorized by these facts.
    pub fn inspect_dependency_capture_retention(
        &self,
        request: RecordDigest,
    ) -> io::Result<NativeCaptureRetention> {
        if request == RecordDigest::from_bytes([0; 32]) {
            return Err(invalid("missing capture request"));
        }
        let guard =
            crate::workspace_custody::lock_workspace_initialization(&self.store).map_err(error)?;
        self.check_dependency_registration()?;
        self.attachment.ensure_current()?;
        crate::project_attachment::detachment::ensure_attached(&self.store)?;
        absent(&self.store, dependency_decision::PENDING)?;
        let (sidecar, raw, pending) = match read_private_in_store(&self.store, PENDING) {
            Ok(raw) => (PENDING.to_owned(), raw, true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let name = receipt_name(request);
                let raw = read_private_in_store(&self.store, &name)?;
                (name, raw, false)
            }
            Err(e) => return Err(e),
        };
        let intent = CaptureIntent::parse(&raw)?;
        if intent.request != request {
            return Err(invalid("another capture owns this recovery"));
        }
        let (configuration, proof) =
            self.attachment
                .read_capture_configuration(self.metadata_path(), &self.store, &raw)?;
        let proof = proof.ok_or_else(|| invalid("capture recovery enrollment missing"))?;
        let cas = Cas::with_filesystem(self.metadata_path(), self.store.filesystem().read_only())
            .map_err(error)?;
        let frames = validate_frames(&cas, &intent)?;
        let mut file = self
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let mut bytes = Vec::new();
        (&mut file)
            .take((MAX_JOURNAL + 1) as u64)
            .read_to_end(&mut bytes)?;
        proof.verify(&self.store, &file, &bytes)?;
        let prefix = capture_prefix(
            &cas,
            &raw,
            (file.metadata()?.dev(), file.metadata()?.ino()),
            &bytes,
            proof.binding().authority,
            &configuration,
        )?;
        let mut complete = bytes.clone();
        if bytes.len() < intent.before_bytes + frames.len() {
            if !pending || prefix != intent.before_bytes {
                return Err(invalid("completed capture has partial frames"));
            }
            complete.truncate(intent.before_bytes);
            complete.extend_from_slice(&frames);
        }
        let scan = mesh_store::scan_journal(&complete).map_err(error)?;
        if scan.tail().is_fragment() {
            return Err(invalid("unknown capture journal suffix"));
        }
        let workspace = WorkspaceId::from_bytes(short_id(configuration.as_bytes()));
        let mut payloads = proof.policy().policy_payloads().collect::<BTreeSet<_>>();
        payloads.extend([intent.frames, intent.authority]);
        let mut manifests = BTreeMap::new();
        let mut referenced = BTreeSet::new();
        let mut operations = BTreeSet::new();
        let mut payload_budget = 1024 * 1024 * 1024usize;
        let mut parents = BTreeSet::new();
        for record in scan.records() {
            match record {
                StoredRecord::Manifest(m) => {
                    if manifests.insert(m.id, m).is_some_and(|old| old != m) {
                        return Err(invalid("conflicting recovery manifests"));
                    }
                }
                StoredRecord::Operation(op) => {
                    let payload = read_payload(&cas, op.payload_digest, 16 * 1024 * 1024)?;
                    payload_budget = payload_budget
                        .checked_sub(payload.len())
                        .ok_or_else(|| invalid("capture recovery payload bytes exceed bound"))?;
                    let fact = OpenWorkspace::verify_dependency_staged_operation(op, &payload)
                        .map_err(error)?;
                    if fact.workspace != workspace || !operations.insert(fact.operation) {
                        return Err(invalid("conflicting capture operation identity"));
                    }
                    referenced.extend(fact.manifests);
                    parents.extend(fact.parents);
                    payloads.insert(op.payload_digest);
                }
                _ => {}
            }
            if operations.len() > 4096
                || manifests.len() > 65536
                || referenced.len() > 65536
                || parents.len() > 16384
            {
                return Err(invalid("capture recovery references exceed bound"));
            }
        }
        if !operations.contains(&intent.operation)
            || !parents.is_subset(&operations)
            || !referenced.iter().all(|id| manifests.contains_key(id))
        {
            return Err(invalid("capture recovery ancestry or manifest missing"));
        }
        let mut budget = 1024 * 1024 * 1024u64;
        // Retain all staged/journaled manifest content, including superseded or not-yet-committed files.
        for manifest in manifests.values() {
            payloads.extend(
                OpenWorkspace::verify_dependency_manifest_content(&cas, manifest, &mut budget)
                    .map_err(error)?,
            );
            if payloads.len() > 65536 {
                return Err(invalid("capture recovery roots exceed bound"));
            }
        }
        let facts = NativeCaptureRetention {
            operation: intent.operation,
            request,
            store: self.store.identity()?,
            authority: intent.authority,
            journal: hash(&bytes),
            sidecar: sidecar.clone(),
            sidecar_digest: hash(raw.as_bytes()),
            pending,
            payloads,
            manifests: manifests.keys().copied().collect(),
        };
        if facts.to_json().encode().len() > 4 * 1024 * 1024 {
            return Err(invalid("capture recovery encoding exceeds bound"));
        }
        file.rewind()?;
        let mut current = Vec::new();
        (&mut file)
            .take((MAX_JOURNAL + 1) as u64)
            .read_to_end(&mut current)?;
        proof.verify(&self.store, &file, &current)?;
        if read_private_in_store(&self.store, &sidecar)? != raw {
            return Err(invalid("capture recovery sidecar changed"));
        }
        self.check_dependency_registration()?;
        self.attachment.ensure_current()?;
        guard.ensure_current().map_err(error)?;
        Ok(facts)
    }
}

pub(in crate::project_attachment) struct CaptureReceiptRoots {
    pub(in crate::project_attachment) payloads: BTreeSet<RecordDigest>,
    pub(in crate::project_attachment) sidecars: BTreeMap<String, RecordDigest>,
}
impl ProvisionedAttachment {
    // Called while the graph holds the complete native custody set. Read all receipt names once,
    // and validate selected historical receipts against the same physical journal snapshot.
    pub(in crate::project_attachment) fn completed_capture_receipt_roots(
        &self,
        operations: &BTreeSet<RecordDigest>,
        budget: &mut usize,
    ) -> io::Result<CaptureReceiptRoots> {
        let guard =
            crate::workspace_custody::lock_workspace_initialization(&self.store).map_err(error)?;
        ensure_no_pending_capture(&self.store)?;
        self.check_dependency_registration()?;
        let (configuration, proof) = self
            .attachment
            .read_configuration(self.metadata_path(), &self.store)?;
        let proof = proof.ok_or_else(|| invalid("capture receipt enrollment missing"))?;
        let names = self
            .store
            .filesystem()
            .read_directory_names_bounded(Path::new(""), 16384)?;
        let cas = Cas::with_filesystem(self.metadata_path(), self.store.filesystem().read_only())
            .map_err(error)?;
        let mut file = self
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let mut bytes = Vec::new();
        (&mut file)
            .take((MAX_JOURNAL + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_JOURNAL {
            return Err(invalid("capture receipt journal exceeds bound"));
        }
        proof.verify(&self.store, &file, &bytes)?;
        let journal = (file.metadata()?.dev(), file.metadata()?.ino());
        let mut result = CaptureReceiptRoots {
            payloads: BTreeSet::new(),
            sidecars: BTreeMap::new(),
        };
        let mut claimed = BTreeMap::new();
        for name in &names {
            if !name.as_encoded_bytes().starts_with(b"dependency-capture-") {
                continue;
            }
            let name = name
                .to_str()
                .ok_or_else(|| invalid("invalid capture receipt name"))?;
            let raw = read_private_in_store(&self.store, name)?;
            *budget = budget
                .checked_sub(raw.len())
                .ok_or_else(|| invalid("capture receipt bytes exceed bound"))?;
            let intent = CaptureIntent::parse(&raw)?;
            if name != receipt_name(intent.request) {
                return Err(invalid("capture receipt request name changed"));
            }
            if !operations.contains(&intent.operation) {
                continue;
            }
            if claimed.insert(intent.operation, intent.request).is_some() {
                return Err(invalid("conflicting completed capture requests"));
            }
            *budget = budget
                .checked_sub(intent.before_bytes)
                .ok_or_else(|| invalid("capture receipt prefix verification exceeds bound"))?;
            let frames = validate_frames(&cas, &intent)?;
            *budget = budget
                .checked_sub(frames.len().saturating_mul(2))
                .ok_or_else(|| invalid("capture receipt frame bytes exceed bound"))?;
            capture_prefix(
                &cas,
                &raw,
                journal,
                &bytes,
                proof.binding().authority,
                &configuration,
            )?;
            if bytes.len() < intent.before_bytes.saturating_add(frames.len()) {
                return Err(invalid("completed capture receipt is not fully journaled"));
            }
            result.payloads.insert(intent.frames);
            result
                .sidecars
                .insert(name.to_owned(), hash(raw.as_bytes()));
        }
        for (name, expected) in &result.sidecars {
            if hash(read_private_in_store(&self.store, name)?.as_bytes()) != *expected {
                return Err(invalid("capture receipt changed during inspection"));
            }
        }
        if self
            .store
            .filesystem()
            .read_directory_names_bounded(Path::new(""), 16384)?
            != names
        {
            return Err(invalid("capture receipt inventory changed"));
        }
        file.rewind()?;
        let mut current = Vec::new();
        (&mut file)
            .take((MAX_JOURNAL + 1) as u64)
            .read_to_end(&mut current)?;
        proof.verify(&self.store, &file, &current)?;
        self.check_dependency_registration()?;
        guard.ensure_current().map_err(error)?;
        Ok(result)
    }
}
