//! Native read proof. Readable policy records are not consumption or publication authority.
use super::{
    dependency_enrollment::read_private_in_store,
    dependency_transaction::{digest, hash, intent_bytes, read_payload, text},
    history::{verify_history_binding, HISTORY},
    invalid, read_receipt, ProjectAttachment,
};
use crate::{
    dependency_policy::{DependencyPolicyHistory, NativeDependencyBinding},
    ipc::Json,
    root_authority::{PinnedRootFs, PinnedWorkspaceRoot},
    workspace::{workspace_installation, OpenWorkspace, RECORD_FILE_NAME},
    TrustedReviewers,
};
use mesh_cas::{Blake3, Cas};
use mesh_store::{scan_journal, RecordDigest, StoredRecord};
use std::{
    fs::File,
    io::{self, Read as _},
    os::unix::fs::MetadataExt as _,
    path::Path,
};
const MAX_BASE: usize = 64 * 1024 * 1024;
const MAX_HISTORY: usize = MAX_BASE + 16 * 1024 * 1024;

/// Exact local enrollment and policy facts. These do not authorize opening a workspace,
/// consumption, mutation or publication, even when local completion records are present.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct NativeDependencyFacts {
    configuration: RecordDigest,
    store: (u64, u64),
    journal: (u64, u64),
    bytes: RecordDigest,
    binding: NativeDependencyBinding,
    policy: DependencyPolicyHistory,
    pending: Option<(usize, mesh_store::DependencyRecord)>,
    legacy_operations: std::collections::BTreeSet<RecordDigest>,
}
impl NativeDependencyFacts {
    pub(super) fn binding(&self) -> NativeDependencyBinding {
        self.binding
    }
    pub(super) fn policy(&self) -> &DependencyPolicyHistory {
        &self.policy
    }

    pub(super) fn is_legacy_operation(&self, operation: RecordDigest) -> bool {
        self.legacy_operations.contains(&operation)
    }

    pub(super) fn pending(&self) -> Option<(usize, mesh_store::DependencyRecord)> {
        self.pending
    }

    pub(crate) fn verify(
        &self,
        store: &PinnedWorkspaceRoot,
        file: &File,
        bytes: &[u8],
    ) -> io::Result<()> {
        store.ensure_namespace_identity()?;
        let held = file.metadata()?;
        let named = store
            .filesystem()
            .inspect_entry(Path::new(RECORD_FILE_NAME))?
            .metadata()?;
        if store.identity()? != self.store
            || !held.is_file()
            || held.nlink() != 1
            || (held.dev(), held.ino()) != self.journal
            || (named.dev(), named.ino()) != self.journal
            || hash(bytes) != self.bytes
        {
            return Err(invalid("validated dependency history changed"));
        }
        Ok(())
    }
}
/// Native enrollment and, where present, completed consumption have been verified.
/// Saved operation/content verification still belongs to the read-only workspace opening.
/// It is deliberately not a workspace admission capability or publication authority.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct VerifiedPrivateHistory {
    facts: NativeDependencyFacts,
}
impl VerifiedPrivateHistory {
    pub(super) fn matches_facts(&self, facts: &NativeDependencyFacts) -> bool {
        &self.facts == facts
    }
    pub(super) fn pending(&self) -> Option<(usize, mesh_store::DependencyRecord)> {
        self.facts.pending()
    }
    pub(super) fn verify_configuration(&self, configuration: &str) -> io::Result<()> {
        if self.facts.configuration != hash(configuration.as_bytes()) {
            return Err(invalid(
                "private history configuration differs from verified facts",
            ));
        }
        Ok(())
    }
    pub(crate) fn is_legacy_operation(&self, operation: RecordDigest) -> bool {
        self.facts.is_legacy_operation(operation)
    }
    pub(crate) fn policy(&self) -> &DependencyPolicyHistory {
        &self.facts.policy
    }
    pub(crate) fn binding(&self) -> NativeDependencyBinding {
        self.facts.binding
    }
    pub(crate) fn verify_current(&self, store: &PinnedWorkspaceRoot) -> io::Result<()> {
        let mut file = store
            .filesystem()
            .read_only()
            .read_file(Path::new(RECORD_FILE_NAME))?;
        let mut bytes = Vec::new();
        (&mut file)
            .take((MAX_HISTORY + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_HISTORY {
            return Err(invalid("private history exceeds its verified bound"));
        }
        self.facts.verify(store, &file, &bytes)
    }
    pub(crate) fn verify(
        &self,
        store: &PinnedWorkspaceRoot,
        file: &File,
        bytes: &[u8],
    ) -> io::Result<()> {
        self.facts.verify(store, file, bytes)
    }
    pub(super) fn from_consumption(
        verified: super::consumption_prepare::VerifiedConsumedHistory,
    ) -> Self {
        Self {
            facts: verified.into_facts(),
        }
    }

    fn from_independent_facts(facts: NativeDependencyFacts) -> io::Result<Self> {
        if facts.policy.has_consumption_transaction()
            || facts.pending.is_some_and(|(_, record)| {
                record.kind == mesh_store::DependencyKind::ConsumptionStart
            })
        {
            return Err(invalid(
                "consumed history requires native transaction verification",
            ));
        }
        Ok(Self { facts })
    }

    fn admit_without_publication(self) -> io::Result<VerifiedDependencyRead> {
        if self.facts.policy.has_publication_claims()
            || self
                .facts
                .pending
                .is_some_and(|(_, record)| record.kind == mesh_store::DependencyKind::Publication)
        {
            return Err(invalid(
                "native publication receipt verification is unavailable",
            ));
        }
        Ok(VerifiedDependencyRead(self))
    }
}

/// Ordinary native workspace admission. Private evidence is necessary but cannot by itself
/// admit an owner journal containing publication claims, including a pending publication.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct VerifiedDependencyRead(VerifiedPrivateHistory);
impl VerifiedDependencyRead {
    pub(super) fn private_evidence(&self) -> &VerifiedPrivateHistory {
        &self.0
    }
    pub(super) fn matches_facts(&self, facts: &NativeDependencyFacts) -> bool {
        &self.0.facts == facts
    }
    pub(super) fn from_private_without_publication(
        verified: VerifiedPrivateHistory,
    ) -> io::Result<Self> {
        verified.admit_without_publication()
    }
    fn from_independent_facts(facts: NativeDependencyFacts) -> io::Result<Self> {
        VerifiedPrivateHistory::from_independent_facts(facts)?.admit_without_publication()
    }
    pub(super) fn binding(&self) -> NativeDependencyBinding {
        self.0.facts.binding()
    }
    pub(super) fn policy(&self) -> &DependencyPolicyHistory {
        self.0.facts.policy()
    }
    pub(super) fn is_legacy_operation(&self, operation: RecordDigest) -> bool {
        self.0.facts.is_legacy_operation(operation)
    }
    pub(super) fn pending(&self) -> Option<(usize, mesh_store::DependencyRecord)> {
        self.0.facts.pending()
    }
    pub(crate) fn verify(
        &self,
        store: &PinnedWorkspaceRoot,
        file: &File,
        bytes: &[u8],
    ) -> io::Result<()> {
        self.0.facts.verify(store, file, bytes)
    }
}
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

enum RecoveryInspection<'a> {
    Default,
    CompletedStart,
    ConsumedCapture(&'a str),
    PublicationReplay,
}
impl ProjectAttachment {
    pub(super) fn read_configuration(
        &self,
        metadata: &Path,
        store: &PinnedWorkspaceRoot,
    ) -> io::Result<(String, Option<VerifiedDependencyRead>)> {
        self.read_decision_configuration(metadata, store, None)
    }

    // Native recovery only: validates an exact pending eligibility frame, never a generic
    // ignore-tail flag. Ordinary readers always pass None and continue refusing torn history.
    pub(super) fn read_decision_configuration(
        &self,
        metadata: &Path,
        store: &PinnedWorkspaceRoot,
        pending: Option<&str>,
    ) -> io::Result<(String, Option<VerifiedDependencyRead>)> {
        self.read_native_configuration(metadata, store, pending, None)
    }

    pub(super) fn read_capture_configuration(
        &self,
        metadata: &Path,
        store: &PinnedWorkspaceRoot,
        capture: &str,
    ) -> io::Result<(String, Option<VerifiedDependencyRead>)> {
        self.read_native_configuration(metadata, store, None, Some(capture))
    }

    fn read_native_configuration(
        &self,
        metadata: &Path,
        store: &PinnedWorkspaceRoot,
        pending: Option<&str>,
        capture: Option<&str>,
    ) -> io::Result<(String, Option<VerifiedDependencyRead>)> {
        let (configuration, facts) = self.read_native_facts(metadata, store, pending, capture)?;
        let proof = facts
            .map(VerifiedDependencyRead::from_independent_facts)
            .transpose()?;
        Ok((configuration, proof))
    }

    // Recovery inspection only. It never hides an unrecognized suffix, changes journal bytes,
    // or creates workspace admission. Callers must hold and revalidate their complete custody set.
    pub(super) fn read_native_facts(
        &self,
        metadata: &Path,
        store: &PinnedWorkspaceRoot,
        pending: Option<&str>,
        capture: Option<&str>,
    ) -> io::Result<(String, Option<NativeDependencyFacts>)> {
        self.read_native_facts_for(
            metadata,
            store,
            pending,
            capture,
            RecoveryInspection::Default,
        )
    }

    // A private replay input, never ordinary admission. Durable publication envelopes are
    // structurally checked here; configured trust and signed approval are checked by replay.
    pub(super) fn read_publication_private_history(
        &self,
        metadata: &Path,
        store: &PinnedWorkspaceRoot,
    ) -> io::Result<(String, VerifiedPrivateHistory)> {
        let (configuration, facts) = self.read_native_facts_for(
            metadata,
            store,
            None,
            None,
            RecoveryInspection::PublicationReplay,
        )?;
        let facts = facts.ok_or_else(|| invalid("publication requires native enrollment"))?;
        Ok((
            configuration,
            VerifiedPrivateHistory::from_independent_facts(facts)?,
        ))
    }

    pub(super) fn read_completed_start_facts(
        &self,
        metadata: &Path,
        store: &PinnedWorkspaceRoot,
    ) -> io::Result<(String, Option<NativeDependencyFacts>)> {
        let pending = read_private_in_store(store, super::consumption_prepare::START_PENDING)?;
        self.read_native_facts_for(
            metadata,
            store,
            Some(&pending),
            None,
            RecoveryInspection::CompletedStart,
        )
    }

    pub(super) fn read_consumed_capture_facts(
        &self,
        metadata: &Path,
        store: &PinnedWorkspaceRoot,
        capture: &str,
        configuration: &str,
    ) -> io::Result<(String, Option<NativeDependencyFacts>)> {
        // Still local facts only. The caller must independently verify the effective configuration
        // and owning transaction before constructing a consumed-history read proof.
        self.read_native_facts_for(
            metadata,
            store,
            None,
            Some(capture),
            RecoveryInspection::ConsumedCapture(configuration),
        )
    }

    fn read_native_facts_for(
        &self,
        metadata: &Path,
        store: &PinnedWorkspaceRoot,
        pending: Option<&str>,
        capture: Option<&str>,
        inspection: RecoveryInspection<'_>,
    ) -> io::Result<(String, Option<NativeDependencyFacts>)> {
        self.ensure_current()?;
        store.ensure_namespace_identity()?;
        let receipt = self.receipt()?.encode();
        if read_receipt(store)? != receipt {
            return Err(invalid("history registration changed"));
        }
        let marker = read_private_in_store(store, HISTORY)?;
        let parsed = Json::parse(&marker).map_err(error)?;
        if parsed.get("schema").and_then(Json::as_text) != Some("mesh.attachment-history/v3") {
            let (configuration, _) =
                self.history_configuration_with_previous(store, None, Some(marker))?;
            return Ok((configuration, None));
        }
        let basis = text(&parsed, "previous_binding")?;
        let authority = digest(text(&parsed, "dependency_authority")?)?;
        let canonical = Json::object([
            ("schema", Json::text("mesh.attachment-history/v3")),
            ("previous_binding", Json::text(basis)),
            ("dependency_authority", Json::text(authority.to_hex())),
        ])
        .encode();
        if canonical != marker {
            return Err(invalid("noncanonical dependency fence"));
        }
        let (configuration, _) =
            self.history_configuration_with_previous(store, None, Some(basis.to_owned()))?;
        let identity = store.identity()?;
        let installation = workspace_installation(identity, identity);
        crate::DependencyEnrollmentFence::verify_while_initialized(
            metadata,
            &installation,
            authority,
        )
        .map_err(error)?;
        let installation = digest(
            installation
                .strip_prefix("blake3:")
                .ok_or_else(|| invalid("invalid native installation"))?,
        )?;
        let project = hash(receipt.as_bytes());
        let cas =
            Cas::<PinnedRootFs, Blake3>::with_filesystem(metadata, store.filesystem().read_only())
                .map_err(error)?;
        let intent = read_payload(&cas, authority, 4096)?;
        let decoded = Json::parse(std::str::from_utf8(&intent).map_err(error)?).map_err(error)?;
        let base_len = decoded
            .get("journal_bytes")
            .and_then(Json::as_u64)
            .filter(|n| *n <= MAX_BASE as u64)
            .ok_or_else(|| invalid("invalid enrollment boundary"))? as usize;
        let base_digest = digest(text(&decoded, "journal_digest")?)?;
        let mut journal = store
            .filesystem()
            .read_only()
            .read_file(Path::new(RECORD_FILE_NAME))?;
        let file = journal.metadata()?;
        let journal_identity = (file.dev(), file.ino());
        let mut bytes = Vec::new();
        (&mut journal)
            .take((MAX_HISTORY + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_HISTORY
            || base_len >= bytes.len()
            || intent
                != intent_bytes(
                    project,
                    installation,
                    journal_identity,
                    base_len as u64,
                    base_digest,
                )
            || hash(&bytes[..base_len]) != base_digest
        {
            return Err(invalid("dependency intent does not match native history"));
        }
        let base = scan_journal(&bytes[..base_len]).map_err(error)?;
        if base.tail().is_fragment()
            || base
                .records()
                .iter()
                .any(|r| matches!(r, StoredRecord::Dependency(_)))
        {
            return Err(invalid("invalid legacy enrollment prefix"));
        }
        if matches!(inspection, RecoveryInspection::PublicationReplay)
            && base
                .records()
                .iter()
                .any(|r| matches!(r, StoredRecord::Approval(_)))
        {
            return Err(invalid(
                "legacy approved main requires explicit native migration",
            ));
        }
        let pending = pending
            .map(|intent| {
                let parsed = Json::parse(intent).map_err(error)?;
                if parsed.get("schema").and_then(Json::as_text)
                    == Some("mesh.native-consumption-start-intent/v1")
                {
                    let completion =
                        match read_private_in_store(store, super::consumption_complete::PENDING) {
                            Ok(raw) => Some(raw),
                            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                            Err(e) => return Err(e),
                        };
                    match read_private_in_store(store, super::consumption_history::PENDING) {
                        Ok(history) => {
                            if matches!(inspection, RecoveryInspection::CompletedStart) {
                                return super::consumption_history::completed_prefix(
                                    &cas,
                                    intent,
                                    &history,
                                    journal_identity,
                                    &bytes,
                                    completion.as_deref().ok_or_else(|| {
                                        invalid("completed consumption intent missing")
                                    })?,
                                );
                            }
                            return super::consumption_history::pending_prefix(
                                &cas,
                                intent,
                                &history,
                                journal_identity,
                                &bytes,
                                completion.as_deref(),
                            );
                        }
                        Err(e) if e.kind() == io::ErrorKind::NotFound && completion.is_none() => {}
                        Err(e) => return Err(e),
                    }
                }
                super::dependency_decision::pending_prefix(&cas, intent, journal_identity, &bytes)
            })
            .transpose()?;
        let capture_prefix = capture
            .map(|intent| {
                super::history::dependency_capture::capture_prefix(
                    &cas,
                    intent,
                    journal_identity,
                    &bytes,
                    authority,
                    match inspection {
                        RecoveryInspection::ConsumedCapture(configuration) => configuration,
                        _ => &configuration,
                    },
                )
            })
            .transpose()?;
        let prefix_end = capture_prefix.unwrap_or_else(|| {
            pending
                .as_ref()
                .map_or(bytes.len(), |(length, _, _)| *length)
        });
        if prefix_end <= base_len {
            return Err(invalid("decision prefix predates enrollment"));
        }
        let suffix = scan_journal(&bytes[base_len..prefix_end]).map_err(error)?;
        if suffix.tail().is_fragment()
            || !matches!(suffix.records().first(), Some(StoredRecord::Dependency(_)))
        {
            return Err(invalid("incomplete dependency enrollment"));
        }
        let mut policy = DependencyPolicyHistory::new(NativeDependencyBinding {
            authority,
            project,
            installation,
        })
        .map_err(error)?;
        for record in suffix.records() {
            match record {
                StoredRecord::Dependency(record) => {
                    if record.kind == mesh_store::DependencyKind::Publication
                        && !matches!(inspection, RecoveryInspection::PublicationReplay)
                    {
                        return Err(invalid(
                            "native publication receipt verification is unavailable",
                        ));
                    }
                    policy
                        .apply(*record, &read_payload(&cas, record.payload, 65_536)?)
                        .map_err(error)?;
                    if let Some(graph) = policy.review_graph(record.payload) {
                        policy
                            .verify_review_graph(
                                record.payload,
                                &read_payload(&cas, graph, 4 * 1024 * 1024)?,
                            )
                            .map_err(error)?;
                    }
                }
                // A valid legacy receipt alone cannot establish a dependency-aware publication.
                StoredRecord::Approval(_) => {
                    return Err(invalid(
                        "dependency publication verification is unavailable",
                    ))
                }
                _ => {}
            }
        }
        if let Some((_, record, payload)) = &pending {
            if record.kind == mesh_store::DependencyKind::Publication {
                return Err(invalid(
                    "native publication receipt verification is unavailable",
                ));
            }
            let mut staged_policy = policy.clone();
            staged_policy.apply(*record, payload).map_err(error)?;
            if let Some(graph) = staged_policy.review_graph(record.payload) {
                staged_policy
                    .verify_review_graph(
                        record.payload,
                        &read_payload(&cas, graph, 4 * 1024 * 1024)?,
                    )
                    .map_err(error)?;
            }
            if record.kind == mesh_store::DependencyKind::ConsumptionStart {
                let value =
                    Json::parse(std::str::from_utf8(payload).map_err(error)?).map_err(error)?;
                if value
                    .get("body")
                    .and_then(|v| v.get("configuration"))
                    .and_then(Json::as_text)
                    != Some(hash(configuration.as_bytes()).to_hex().as_str())
                {
                    return Err(invalid("consumption start configuration changed"));
                }
            }
        }
        let proof = NativeDependencyFacts {
            configuration: hash(configuration.as_bytes()),
            store: identity,
            journal: journal_identity,
            bytes: hash(&bytes),
            binding: NativeDependencyBinding {
                authority,
                project,
                installation,
            },
            policy,
            pending: pending.map(|(length, record, _)| (length, record)),
            legacy_operations: base
                .records()
                .iter()
                .filter_map(|r| match r {
                    StoredRecord::Operation(op) => Some(op.id),
                    _ => None,
                })
                .collect(),
        };
        proof.verify(store, &journal, &bytes)?;
        Ok((configuration, Some(proof)))
    }

    pub(super) fn with_read_history<T>(
        &self,
        metadata: &Path,
        store: PinnedWorkspaceRoot,
        trusted: &TrustedReviewers,
        action: impl FnOnce(&OpenWorkspace, &PinnedWorkspaceRoot, &str) -> io::Result<T>,
    ) -> io::Result<T> {
        let _guard =
            crate::workspace_custody::lock_workspace_initialization(&store).map_err(error)?;
        let (configuration, proof) = self.read_configuration(metadata, &store)?;
        let workspace = OpenWorkspace::open_attachment_read_history(
            metadata,
            store.clone(),
            trusted,
            proof.as_ref(),
        )
        .map_err(error)?;
        verify_history_binding(&workspace, &configuration)?;
        let result = action(&workspace, &store, &configuration)?;
        let (after_configuration, after_proof) = self.read_configuration(metadata, &store)?;
        if after_configuration != configuration || after_proof != proof {
            return Err(invalid("history changed during inspection"));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_attachment::{AttachmentStorage, ObservationLimits, ProvisionedAttachment};
    use ed25519_dalek::{Signer as _, SigningKey};
    use std::{fs, path::PathBuf};
    struct Fixture {
        root: PathBuf,
        attachment: ProvisionedAttachment,
    }
    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("mesh-enrolled-read-{name}-{}", std::process::id()));
            fs::create_dir(&root).unwrap();
            fs::create_dir(root.join("source")).unwrap();
            fs::create_dir(root.join("metadata")).unwrap();
            fs::write(root.join("source/note"), b"saved original").unwrap();
            let storage = AttachmentStorage::open(&root.join("metadata")).unwrap();
            let attachment = storage.provision(&root.join("source")).unwrap();
            let input = attachment
                .project()
                .capture_inputs(ObservationLimits::default())
                .unwrap();
            let key = SigningKey::from_bytes(&[67; 32]);
            attachment
                .project()
                .save_capture(
                    attachment.metadata_path(),
                    &input,
                    mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes()),
                    |payload| {
                        Ok::<_, &'static str>(mesh_types::Signature::from_bytes(
                            key.sign(payload.as_bytes()).to_bytes(),
                        ))
                    },
                )
                .unwrap();
            attachment.enroll_dependency_history().unwrap();
            Self { root, attachment }
        }
        fn journal(&self) -> PathBuf {
            self.attachment.metadata_path().join(RECORD_FILE_NAME)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn private_evidence_cannot_admit_a_pending_publication() {
        let fixture = Fixture::new("private-admission");
        let work = &fixture.attachment;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&work.store).unwrap();
        let (_, facts) = work
            .project()
            .read_native_facts(work.metadata_path(), &work.store, None, None)
            .unwrap();
        let facts = facts.unwrap();
        let private = VerifiedPrivateHistory::from_independent_facts(facts.clone()).unwrap();
        assert!(private
            .admit_without_publication()
            .unwrap()
            .matches_facts(&facts));
        let before = fs::read(fixture.journal()).unwrap();
        let mut pending = facts.clone();
        pending.pending = Some((
            before.len(),
            mesh_store::DependencyRecord {
                authority: facts.binding.authority,
                revision: 2,
                previous: facts.policy.native_head().unwrap().1,
                payload: RecordDigest::from_bytes([219; 32]),
                kind: mesh_store::DependencyKind::Publication,
            },
        ));
        let private = VerifiedPrivateHistory::from_independent_facts(pending).unwrap();
        let refusal = private.admit_without_publication().err().unwrap();
        assert!(refusal
            .to_string()
            .contains("native publication receipt verification"));
        assert_eq!(fs::read(fixture.journal()).unwrap(), before);
        assert!(VerifiedDependencyRead::from_independent_facts(facts).is_ok());
    }

    #[test]
    fn private_evidence_cannot_admit_structurally_valid_publication_claims() {
        let fixture = Fixture::new("private-publication-claim");
        let work = &fixture.attachment;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&work.store).unwrap();
        let (_, facts) = work
            .project()
            .read_native_facts(work.metadata_path(), &work.store, None, None)
            .unwrap();
        let mut facts = facts.unwrap();
        let before = fs::read(fixture.journal()).unwrap();
        let id = |n| Json::text(RecordDigest::from_bytes([n; 32]).to_hex());
        let output = Json::Array(vec![Json::Array(vec![id(20), id(21)]), id(22)]);
        let decisions = Json::Array(vec![]);
        let validation = Json::object([
            ("schema", Json::text("mesh.native-review-validation/v1")),
            ("output", output.clone()),
            ("graph", id(80)),
            ("decisions", decisions.clone()),
        ])
        .encode();
        let apply = |policy: &mut DependencyPolicyHistory,
                     kind: mesh_store::DependencyKind,
                     schema: &str,
                     body: Json| {
            let (ordinal, previous) = policy.native_head().unwrap();
            let value = Json::object([
                ("schema", Json::text(schema)),
                ("authority", Json::text(facts.binding.authority.to_hex())),
                ("revision", Json::Number(ordinal + 1)),
                ("previous", Json::text(previous.to_hex())),
                ("kind", Json::Number(kind.code().into())),
                ("body", body),
            ])
            .encode();
            let payload = hash(value.as_bytes());
            policy
                .apply(
                    mesh_store::DependencyRecord {
                        authority: facts.binding.authority,
                        revision: ordinal + 1,
                        previous,
                        payload,
                        kind,
                    },
                    value.as_bytes(),
                )
                .unwrap();
            Json::text(payload.to_hex())
        };
        let snapshot = apply(
            &mut facts.policy,
            mesh_store::DependencyKind::ReviewSnapshot,
            "mesh.dependency-policy/v3",
            Json::object([
                ("request", id(71)),
                ("revision", Json::Number(1)),
                ("output", output.clone()),
                ("graph", id(80)),
                ("decisions", decisions),
                (
                    "validation",
                    Json::text(hash(validation.as_bytes()).to_hex()),
                ),
            ]),
        );
        let review = apply(
            &mut facts.policy,
            mesh_store::DependencyKind::ReviewSnapshot,
            "mesh.dependency-policy/v4",
            Json::object([
                ("request", id(90)),
                ("revision", Json::Number(1)),
                ("snapshot", snapshot),
                ("output", output),
                ("canonical", id(0)),
                ("bundle", id(91)),
                ("opener", id(92)),
            ]),
        );
        apply(
            &mut facts.policy,
            mesh_store::DependencyKind::Publication,
            "mesh.dependency-policy/v5",
            Json::object([
                ("request", id(100)),
                ("revision", Json::Number(1)),
                ("previous", id(0)),
                ("review", review),
                ("receipt", id(101)),
                ("result", id(102)),
                ("credential", id(103)),
                ("challenge", id(104)),
            ]),
        );
        assert!(facts.policy.has_publication_claims());
        // Structural policy success does not prove the retained receipt, closure or human trust.
        let private = VerifiedPrivateHistory::from_independent_facts(facts).unwrap();
        let refusal = private.admit_without_publication().err().unwrap();
        assert!(refusal
            .to_string()
            .contains("native publication receipt verification"));
        assert_eq!(fs::read(fixture.journal()).unwrap(), before);
    }

    #[test]
    fn publication_envelope_without_native_receipt_verification_refuses_ordinary_read() {
        use std::io::Write;
        let f = Fixture::new("publication-fence");
        let original = fs::read(f.journal()).unwrap();
        let scan = scan_journal(&original).unwrap();
        let Some(StoredRecord::Dependency(previous)) = scan.records().last() else {
            panic!("enrollment")
        };
        let record = StoredRecord::Dependency(mesh_store::DependencyRecord {
            authority: previous.authority,
            revision: previous.revision + 1,
            previous: previous.payload,
            payload: RecordDigest::from_bytes([217; 32]),
            kind: mesh_store::DependencyKind::Publication,
        });
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(f.journal())
            .unwrap();
        file.write_all(&mesh_store::frame_record(&record)).unwrap();
        file.sync_all().unwrap();
        let before = fs::read(f.journal()).unwrap();
        let error = f.attachment.saved_versions().unwrap_err();
        assert!(error
            .to_string()
            .contains("native publication receipt verification is unavailable"));
        assert_eq!(fs::read(f.journal()).unwrap(), before);
        assert_eq!(
            fs::read(f.root.join("source/note")).unwrap(),
            b"saved original"
        );
    }

    #[test]
    fn enrolled_history_reads_saved_bytes_and_cannot_promote_receipts() {
        let f = Fixture::new("immutable");
        let history = &f.attachment;
        let before = fs::read(f.journal()).unwrap();
        let versions = history.saved_versions().unwrap();
        let key = SigningKey::from_bytes(&[67; 32]);
        let actor = mesh_types::PublicKey::from_bytes(key.verifying_key().to_bytes());
        let input = history
            .project()
            .capture_inputs(ObservationLimits::default())
            .unwrap();
        assert!(history
            .project()
            .save_capture(
                history.metadata_path(),
                &input,
                actor,
                |_| -> Result<mesh_types::Signature, &'static str> {
                    panic!("fenced capture must not sign")
                }
            )
            .is_err());
        assert!(history
            .request_review(&versions[0].operation().to_string(), actor)
            .is_err());
        fs::write(f.root.join("source/note"), b"editor continues").unwrap();
        assert_eq!(
            history
                .project()
                .saved_file(history.metadata_path(), versions[0], "note")
                .unwrap()
                .unwrap(),
            b"saved original"
        );
        history
            .project()
            .with_read_history(
                history.metadata_path(),
                history.store.clone(),
                &TrustedReviewers::default(),
                |workspace, _, _| {
                    assert!(workspace
                        .promote_approval_receipt(b"cannot write".to_vec())
                        .is_err());
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(fs::read(f.journal()).unwrap(), before);
    }

    #[test]
    fn enrolled_history_refuses_torn_changed_or_replaced_journal_without_repair() {
        for mode in ["torn", "base", "replacement", "no-enrollment"] {
            let f = Fixture::new(mode);
            let mut bytes = fs::read(f.journal()).unwrap();
            match mode {
                "torn" => {
                    bytes.pop();
                }
                "base" => bytes[10] ^= 1,
                "no-enrollment" => bytes.truncate(bytes.len() - 145),
                "replacement" => {
                    fs::rename(f.journal(), f.root.join("old-journal")).unwrap();
                }
                _ => unreachable!(),
            }
            fs::write(f.journal(), &bytes).unwrap();
            assert!(f.attachment.saved_versions().is_err(), "{mode}");
            assert_eq!(fs::read(f.journal()).unwrap(), bytes);
        }
    }

    #[test]
    fn enrolled_history_refuses_missing_fences_and_corrupt_policy_objects() {
        for mode in ["attached", "managed", "intent", "payload"] {
            let f = Fixture::new(mode);
            let before = fs::read(f.journal()).unwrap();
            let scan = scan_journal(&before).unwrap();
            let Some(StoredRecord::Dependency(record)) = scan.records().last() else {
                panic!("enrollment")
            };
            let path = match mode {
                "attached" => f.attachment.metadata_path().join(HISTORY),
                "managed" => {
                    let names = fs::read_dir(f.attachment.metadata_path())
                        .unwrap()
                        .map(|e| e.unwrap().path())
                        .collect::<Vec<_>>();
                    names
                        .into_iter()
                        .find(|p| {
                            fs::read_to_string(p)
                                .is_ok_and(|s| s.contains("mesh.workspace-agent-custody/v2"))
                        })
                        .unwrap()
                }
                "intent" | "payload" => {
                    let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
                        f.attachment.metadata_path(),
                        f.attachment.store.filesystem().read_only(),
                    )
                    .unwrap();
                    let digest = if mode == "intent" {
                        record.authority
                    } else {
                        record.payload
                    };
                    f.attachment.metadata_path().join(
                        cas.layout()
                            .chunk_path(&mesh_cas::Digest32::from_bytes(*digest.as_bytes())),
                    )
                }
                _ => unreachable!(),
            };
            fs::write(&path, b"corrupt evidence").unwrap();
            assert!(f.attachment.saved_versions().is_err(), "{mode}");
            assert_eq!(fs::read(&path).unwrap(), b"corrupt evidence");
            assert_eq!(fs::read(f.journal()).unwrap(), before);
        }
    }

    #[test]
    fn sealed_read_proof_rejects_journal_change_even_without_dependency_records() {
        let f = Fixture::new("proof");
        let h = &f.attachment;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&h.store).unwrap();
        let (_, proof) = h
            .project()
            .read_configuration(h.metadata_path(), &h.store)
            .unwrap();
        let bytes = fs::read(f.journal()).unwrap();
        fs::write(f.journal(), &bytes[..bytes.len() - 145]).unwrap();
        assert!(OpenWorkspace::open_attachment_read_history(
            h.metadata_path(),
            h.store.clone(),
            &TrustedReviewers::default(),
            proof.as_ref()
        )
        .is_err());
    }
    #[test]
    fn enrolled_inspection_does_not_authorize_manual_agent_or_remote_consumption() {
        let f = Fixture::new("consumption-boundary");
        let h = &f.attachment;
        let version = h.saved_versions().unwrap()[0].operation().to_string();
        assert!(h.inspect_text(&version, "note").is_ok());
        let storage = AttachmentStorage::open(&f.root.join("metadata")).unwrap();
        let destination = f.root.join("destination");
        fs::create_dir(&destination).unwrap();
        let pinned = PinnedWorkspaceRoot::open(destination.clone()).unwrap();
        let journal = fs::read(f.journal()).unwrap();
        let refused = [
            storage
                .open_version_lane(
                    h,
                    &version,
                    "0123456789abcdef0123456789abcdef",
                    ObservationLimits::default(),
                )
                .is_err(),
            h.validate_lane_version(&version).is_err(),
            h.materialize_saved_version(&version, &pinned, &destination)
                .is_err(),
            h.prepare_remote_input(&version).is_err(),
        ];
        assert_eq!(refused, [true; 4], "manual allocation, agent validation/materialization and remote export require native grants");
        assert!(!f.root.join("metadata/work-lanes").exists());
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
        assert_eq!(fs::read(f.journal()).unwrap(), journal);
        assert_eq!(
            fs::read(f.root.join("source/note")).unwrap(),
            b"saved original"
        );
    }
}
