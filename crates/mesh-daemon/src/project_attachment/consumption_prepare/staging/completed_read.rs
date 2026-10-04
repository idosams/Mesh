//! Cross-store initial-history admission under complete native custody. No writable capability.
use super::*;
use crate::project_attachment::{
    consumption_complete,
    dependency_read::{NativeDependencyFacts, VerifiedDependencyRead},
    dependency_transaction::read_payload,
    NativeDependencyGraph,
};
use crate::{root_authority::PinnedRootFs, workspace_custody::WorkspaceInitializationGuard};
use mesh_cas::{Blake3, Cas};
use mesh_store::{frame_record, scan_journal, DependencyKind, StoredRecord};

// Only this module can construct the token consumed by VerifiedDependencyRead. It is never
// returned to callers, and the complete custody guard remains held through the actual read.
pub(in crate::project_attachment) struct VerifiedConsumedHistory {
    facts: NativeDependencyFacts,
}
impl VerifiedConsumedHistory {
    pub(in crate::project_attachment) fn into_facts(self) -> NativeDependencyFacts {
        self.facts
    }
}
impl PreparedNativeConsumedStart {
    pub(super) fn read_completed_versions(
        &self,
        graph: &NativeDependencyGraph,
        guard: &WorkspaceInitializationGuard,
    ) -> io::Result<Vec<SavedAttachmentVersion>> {
        self.with_completed_history(graph, guard, |workspace, configuration| {
            let line = crate::project_attachment::capture_line::CaptureLine::load(
                &self.destination.store,
                workspace,
                configuration,
            )?;
            workspace
                .linear_history(line.head)
                .map_err(error)?
                .into_iter()
                .map(|operation| {
                    SavedAttachmentVersion::from_verified_history(workspace, operation)
                })
                .collect()
        })
    }
    pub(super) fn with_completed_history<T>(
        &self,
        graph: &NativeDependencyGraph,
        guard: &WorkspaceInitializationGuard,
        action: impl FnOnce(&OpenWorkspace, &str) -> io::Result<T>,
    ) -> io::Result<T> {
        let (configuration, proof) = self.verify_completed_history(graph, guard, None)?;
        let workspace = OpenWorkspace::open_attachment_read_history(
            self.destination.metadata_path(),
            self.destination.store.clone(),
            &crate::TrustedReviewers::default(),
            Some(&proof),
        )
        .map_err(error)?;
        verify_history_binding(&workspace, &configuration)?;
        let result = action(&workspace, &configuration)?;
        let (after_configuration, after) = self.verify_completed_history(graph, guard, None)?;
        if after_configuration != configuration || after != proof {
            return Err(invalid("consumed history changed during inspection"));
        }
        Ok(result)
    }

    pub(super) fn verify_completed_history(
        &self,
        graph: &NativeDependencyGraph,
        guard: &WorkspaceInitializationGuard,
        capture: Option<&str>,
    ) -> io::Result<(String, VerifiedDependencyRead)> {
        guard.ensure_current().map_err(error)?;
        let destination = &self.destination;
        let pending = read_private_in_store(&destination.store, super::super::START_PENDING)?;
        let (configuration, pending_facts) = destination
            .project()
            .read_completed_start_facts(destination.metadata_path(), &destination.store)?;
        let pending_facts = pending_facts.ok_or_else(|| invalid("start enrollment missing"))?;
        let (prefix, pending_start) = pending_facts
            .pending()
            .ok_or_else(|| invalid("original start missing"))?;
        let read_facts = || match capture {
            Some(raw) => destination.project().read_consumed_capture_facts(
                destination.metadata_path(),
                &destination.store,
                raw,
                &self.basis.prospective,
            ),
            None => destination.project().read_native_facts(
                destination.metadata_path(),
                &destination.store,
                None,
                None,
            ),
        };
        let (full_configuration, facts) = read_facts()?;
        let facts = facts.ok_or_else(|| invalid("completed enrollment missing"))?;
        let (start, complete, owner_receipt) = facts
            .policy()
            .completed_consumption_records()
            .ok_or_else(|| invalid("consumption is not complete"))?;
        if configuration != self.basis.configuration
            || full_configuration != configuration
            || facts.binding() != self.basis.enrollment
            || start != pending_start
            || facts.policy().native_request(self.request) != Some(start)
            || graph.digest() != self.basis.graph
        {
            return Err(invalid("completed starting selection changed"));
        }
        // Read the full owning authority independently. A local completion or pending owner prefix
        // cannot substitute for the exact durable owning receipt, even after a later grant revoke.
        let (owner_configuration, owner) = self
            .owner
            .project()
            .read_configuration(self.owner.metadata_path(), &self.owner.store)?;
        let owner = owner.ok_or_else(|| invalid("owning authority missing"))?;
        let owner_record = owner
            .policy()
            .native_request(self.request)
            .ok_or_else(|| invalid("owning consumption receipt missing"))?;
        if owner.pending().is_some()
            || owner_record.kind != DependencyKind::Consumption
            || owner_record.payload != owner_receipt
        {
            return Err(invalid("completion differs from owning receipt"));
        }
        let owner_cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
            self.owner.metadata_path(),
            self.owner.store.filesystem().read_only(),
        )
        .map_err(error)?;
        let bytes = read_payload(&owner_cas, owner_receipt, 65536)?;
        let envelope = Json::parse(std::str::from_utf8(&bytes).map_err(error)?).map_err(error)?;
        let j = |value: RecordDigest| Json::text(value.to_hex());
        let expected = Json::object([
            ("request", j(self.request)),
            ("grant", j(self.grant)),
            (
                "start",
                Json::Array(vec![
                    Json::Array(vec![
                        j(self.basis.destination.work()),
                        j(self.basis.destination.installation()),
                    ]),
                    j(self.operation()),
                ]),
            ),
            ("inputs", graph.consumption_inputs_json()),
        ]);
        if envelope.get("body") != Some(&expected) {
            return Err(invalid(
                "owning receipt differs from verified source closure",
            ));
        }
        let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
            destination.metadata_path(),
            destination.store.filesystem().read_only(),
        )
        .map_err(error)?;
        let (expected_complete, complete_payload) =
            consumption_complete::payload(start, owner_receipt)?;
        if complete != expected_complete
            || read_payload(&cas, complete.payload, 65536)? != complete_payload
        {
            return Err(invalid("completion payload changed"));
        }
        // The starting checkpoint is independently reconstructed from the exact saved source by
        // recovery, including signatures, exclusions, manifests and content. Check its placement.
        let mut journal = destination
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let mut bytes = Vec::new();
        (&mut journal)
            .take(80 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        facts.verify(&destination.store, &journal, &bytes)?;
        if prefix > bytes.len() {
            return Err(invalid("completed history boundary changed"));
        }
        let before = scan_journal(&bytes[..prefix]).map_err(error)?;
        if before.tail().is_fragment()
            || !matches!(before.records(), [StoredRecord::Dependency(record)] if record.kind == DependencyKind::Enrollment)
        {
            return Err(invalid(
                "consumed start did not follow an empty reservation",
            ));
        }
        let mut expected = frame_record(&StoredRecord::Dependency(start));
        expected.extend_from_slice(&self.frames());
        expected.extend_from_slice(&frame_record(&StoredRecord::Dependency(complete)));
        if !bytes[prefix..].starts_with(&expected) {
            return Err(invalid("completed checkpoint differs from signed source"));
        }
        let later = scan_journal(&bytes[prefix + expected.len()..]).map_err(error)?;
        if (capture.is_none() && later.tail().is_fragment())
            || later.records().iter().any(|record| {
                !matches!(
                    record,
                    StoredRecord::Manifest(_) | StoredRecord::Operation(_)
                )
            })
        {
            return Err(invalid("consumed history contains non-capture suffix"));
        }
        let proof = VerifiedDependencyRead::from_consumption(VerifiedConsumedHistory { facts });
        let (after_configuration, after) = read_facts()?;
        let (after_owner_configuration, after_owner) = self
            .owner
            .project()
            .read_configuration(self.owner.metadata_path(), &self.owner.store)?;
        if after_configuration != configuration
            || !after
                .as_ref()
                .is_some_and(|facts| proof.matches_facts(facts))
            || after_owner_configuration != owner_configuration
            || after_owner.as_ref() != Some(&owner)
            || read_private_in_store(&destination.store, super::super::START_PENDING)? != pending
        {
            return Err(invalid("consumed history changed during inspection"));
        }
        guard.ensure_current().map_err(error)?;
        Ok((self.basis.prospective.clone(), proof))
    }
}

#[cfg(test)]
pub(super) fn assert_completed_read(
    prepared: &PreparedNativeConsumedStart,
    storage: &AttachmentStorage,
) {
    let available = prepared.available.iter().collect::<Vec<_>>();
    let request = || NativeConsumedStartRequest {
        input: NativeGrantInspection {
            source: &prepared.source,
            version: prepared.version,
            destination: &prepared.destination,
            grant: prepared.grant,
        },
        available: &available,
        request: prepared.request,
        limits: prepared.limits,
    };
    let destination = &prepared.destination;
    let journal_path = destination.metadata_path().join(crate::RECORD_FILE_NAME);
    let journal = fs::read(&journal_path).unwrap();
    let marker_path = destination
        .metadata_path()
        .join(crate::project_attachment::history::HISTORY);
    let marker = fs::read(&marker_path).unwrap();
    let versions = storage
        .saved_consumed_versions(&prepared.owner, request())
        .unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].operation(), prepared.operation());
    // Later editor changes are independent of the exact saved starting version, including restart.
    fs::write(
        destination.project().root().join("kept"),
        b"later unsaved editor work",
    )
    .unwrap();
    fs::write(
        destination.project().root().join("new-editor-file"),
        b"preserve me",
    )
    .unwrap();
    let value = Json::object([
        ("storage", Json::text(storage.path.to_string_lossy())),
        ("owner", Json::text(prepared.owner.id())),
        ("source", Json::text(prepared.source.id())),
        ("destination", Json::text(destination.id())),
        ("version", Json::text(prepared.version.operation().to_hex())),
        ("grant", Json::text(prepared.grant.to_hex())),
        ("request", Json::text(prepared.request.to_hex())),
        ("operation", Json::text(prepared.operation().to_hex())),
        ("mode", Json::text("read-completed")),
    ]);
    for _ in 0..2 {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "project_attachment::consumption_prepare::tests::saved_ignore_rules_bind_candidate_without_changing_empty_reservation", "--nocapture"])
            .env_remove("MESH_PRIVATE_STAGE_RESTART").env("MESH_FENCED_START_RESTART", value.encode()).output().unwrap();
        assert!(
            child.status.success(),
            "{} {}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        assert_eq!(fs::read(&journal_path).unwrap(), journal);
        assert_eq!(fs::read(&marker_path).unwrap(), marker);
        assert_eq!(
            fs::read(destination.project().root().join("kept")).unwrap(),
            b"later unsaved editor work"
        );
        assert_eq!(
            fs::read(destination.project().root().join("new-editor-file")).unwrap(),
            b"preserve me"
        );
    }
    assert_eq!(
        storage
            .consumed_saved_file(&prepared.owner, request(), versions[0], "kept")
            .unwrap(),
        Some(b"saved bytes".to_vec())
    );
    // A locally consistent completed journal still cannot invent its owning receipt.
    let records = scan_journal(&journal).unwrap();
    let start = records
        .records()
        .iter()
        .find_map(|r| match r {
            StoredRecord::Dependency(r) if r.kind == DependencyKind::ConsumptionStart => Some(*r),
            _ => None,
        })
        .unwrap();
    let complete = records.records().last().unwrap();
    let prefix = journal.len() - frame_record(complete).len();
    let (wrong, payload) =
        consumption_complete::payload(start, RecordDigest::from_bytes([107; 32])).unwrap();
    let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
        destination.metadata_path(),
        destination.store.filesystem(),
    )
    .unwrap();
    cas.promote(payload).unwrap();
    let intent_path = destination
        .metadata_path()
        .join(consumption_complete::PENDING);
    let original_intent = fs::read(&intent_path).unwrap();
    let m = fs::metadata(&journal_path).unwrap();
    fs::write(
        &intent_path,
        consumption_complete::intent(
            prepared.request,
            (m.dev(), m.ino()),
            &journal[..prefix],
            wrong.payload,
        ),
    )
    .unwrap();
    let mut changed = journal[..prefix].to_vec();
    changed.extend_from_slice(&frame_record(&StoredRecord::Dependency(wrong)));
    fs::write(&journal_path, &changed).unwrap();
    let failure = storage
        .saved_consumed_versions(&prepared.owner, request())
        .unwrap_err();
    assert_eq!(
        failure.to_string(),
        "completion differs from owning receipt"
    );
    assert_eq!(fs::read(&journal_path).unwrap(), changed);
    fs::write(&journal_path, &journal).unwrap();
    fs::write(&intent_path, &original_intent).unwrap();
    assert_eq!(
        storage
            .saved_consumed_versions(&prepared.owner, request())
            .unwrap(),
        versions
    );
    assert!(destination.saved_versions().is_err());
    crate::project_attachment::history::dependency_capture::assert_consumed_capture(
        storage,
        &prepared.owner,
        request(),
    );
}

#[cfg(test)]
pub(super) fn assert_incomplete_read_refused(
    prepared: &PreparedNativeConsumedStart,
    storage: &AttachmentStorage,
) {
    let available = prepared.available.iter().collect::<Vec<_>>();
    assert!(storage
        .saved_consumed_versions(
            &prepared.owner,
            NativeConsumedStartRequest {
                input: NativeGrantInspection {
                    source: &prepared.source,
                    version: prepared.version,
                    destination: &prepared.destination,
                    grant: prepared.grant
                },
                available: &available,
                request: prepared.request,
                limits: prepared.limits,
            }
        )
        .is_err());
}
