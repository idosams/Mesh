//! Reload the exact fenced candidate without signing, writing, installing or admitting a run.
use super::*;
use crate::project_attachment::dependency_transaction::{digest, read_payload, text};
use crate::root_authority::PinnedRootFs;
use crate::{
    authenticated_changeset::AuthenticatedChangeSet,
    checkpoint_storage::PreparedAuthenticatedCheckpoint,
};
use mesh_cas::{Blake3, Cas};
use mesh_store::{scan_journal, Checkpoint, DependencyKind, StoredRecord};
impl AttachmentStorage {
    /// Reconstruct an interrupted start from its retained signed material. No signer is invoked.
    /// Current grant and the exact original limits are required while consumption is uncommitted.
    /// This start-only recovery leaves ordinary history/runtime admission fenced; it does not
    /// recover installed entries or acknowledge the unfinished owner/completion transaction.
    pub fn recover_fenced_consumed_start(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeConsumedStartRequest<'_>,
    ) -> io::Result<(PreparedNativeConsumedStart, StagedNativeConsumedStart)> {
        self.recover_consumed_start_state(owner, request, false)
    }
    /// Reconstruct an exact start with zero or more installed entries. This only returns
    /// authenticated transaction material; ordinary history and runtime remain fenced.
    pub fn recover_installing_consumed_start(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeConsumedStartRequest<'_>,
    ) -> io::Result<(PreparedNativeConsumedStart, StagedNativeConsumedStart)> {
        self.recover_consumed_start_state(owner, request, true)
    }
    fn recover_consumed_start_state(
        &self,
        owner: &ProvisionedAttachment,
        request: NativeConsumedStartRequest<'_>,
        installed: bool,
    ) -> io::Result<(PreparedNativeConsumedStart, StagedNativeConsumedStart)> {
        request.limits.validate()?;
        if request.request == RecordDigest::from_bytes([0; 32]) {
            return Err(invalid("missing recovery request"));
        }
        let selected_graph = self.prepare_dependency_graph(
            owner,
            request.input.source,
            request.input.version.operation(),
            request.available,
        )?;
        let selected_grant = self.prepare_input_grant(
            owner,
            NativeGrantInspection {
                source: request.input.source,
                version: request.input.version,
                destination: request.input.destination,
                grant: request.input.grant,
            },
        )?;
        let selected_source = self.prepare_dependency_work(owner, request.input.source)?;
        let selected_destination =
            self.prepare_dependency_work(owner, request.input.destination)?;
        let roots = selected_graph
            .roots
            .iter()
            .chain(&selected_grant.roots)
            .chain(&selected_source.roots)
            .chain(&selected_destination.roots)
            .map(|root| Ok((root.identity()?, root.clone())))
            .collect::<io::Result<BTreeMap<_, _>>>()?
            .into_values()
            .collect::<Vec<_>>();
        let guard =
            crate::workspace_custody::lock_workspace_initialization_set(&roots).map_err(error)?;
        let graph = self.inspect_prepared_dependency_graph(&selected_graph, &guard)?;
        if graph.operation_count() > 256 {
            return Err(invalid("consumption closure exceeds bound"));
        }
        let source_binding = self.validate_dependency_work(&selected_source, &guard)?;
        let destination_binding = self.validate_dependency_work(&selected_destination, &guard)?;
        let destination = request.input.destination;
        let origin = self
            .lane_origin_bound(destination)?
            .ok_or_else(|| invalid("reservation missing"))?;
        if !crate::project_attachment::dependency_reservation::is_reservation(&origin.value)
            || (!installed
                && !destination
                    .attachment
                    .pinned
                    .filesystem()
                    .read_directory_names_bounded(Path::new(""), 1)?
                    .is_empty())
        {
            return Err(invalid(
                "start recovery requires unchanged empty reservation",
            ));
        }
        absent(destination, "dependency-capture.pending")?;
        absent(
            destination,
            crate::project_attachment::dependency_decision::PENDING,
        )?;
        let pending = read_private_in_store(&destination.store, super::super::START_PENDING)?;
        let (configuration, facts) = destination.project().read_native_facts(
            destination.metadata_path(),
            &destination.store,
            Some(&pending),
            None,
        )?;
        let facts = facts.ok_or_else(|| invalid("recovery enrollment missing"))?;
        let (_, record) = facts
            .pending()
            .ok_or_else(|| invalid("exact start intent missing"))?;
        if record.kind != DependencyKind::ConsumptionStart {
            return Err(invalid("wrong recovery record kind"));
        }
        let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
            destination.metadata_path(),
            destination.store.filesystem().read_only(),
        )
        .map_err(error)?;
        let payload = read_payload(&cas, record.payload, 65536)?;
        let payload = Json::parse(std::str::from_utf8(&payload).map_err(error)?).map_err(error)?;
        let body = payload
            .get("body")
            .ok_or_else(|| invalid("missing start body"))?;
        let descriptor_id = digest(text(body, "staged")?)?;
        let descriptor = read_payload(&cas, descriptor_id, 4096)?;
        let descriptor_json =
            Json::parse(std::str::from_utf8(&descriptor).map_err(error)?).map_err(error)?;
        let stage_id = digest(text(&descriptor_json, "stage")?)?;
        let expected_descriptor = Json::object([
            (
                "schema",
                Json::text("mesh.native-consumption-transaction/v1"),
            ),
            ("request", Json::text(request.request.to_hex())),
            ("stage", Json::text(stage_id.to_hex())),
            (
                "limits",
                Json::Array(vec![
                    Json::Number(request.limits.entries as u64),
                    Json::Number(request.limits.bytes),
                    Json::Number(request.limits.file_bytes),
                ]),
            ),
        ])
        .encode();
        if descriptor != expected_descriptor.as_bytes() {
            return Err(invalid("original recovery request or limits differ"));
        }
        let root = origin.allocation.open_child_directory(OsStr::new(&format!(
            "consumption-{}",
            request.request.to_hex()
        )))?;
        let raw = read(&root, RECEIPT, MAX_RECEIPT)?;
        if hash(&raw) != stage_id || read_payload(&cas, stage_id, MAX_RECEIPT)? != raw {
            return Err(invalid("retained consumption stage changed"));
        }
        let stage = Json::parse(std::str::from_utf8(&raw).map_err(error)?).map_err(error)?;
        if text(&stage, "configuration")? != configuration {
            return Err(invalid("original destination configuration differs"));
        }
        let original_graph = stage
            .get("graph")
            .ok_or_else(|| invalid("staged graph missing"))?;
        if original_graph.get("closure") != Some(&graph.to_json()) {
            return Err(invalid("recovery source closure differs"));
        }
        let frames = read(&root, "frames.mesh", 80 * 1024 * 1024)?;
        if hash(&frames) != digest(text(&stage, "frames")?)? {
            return Err(invalid("staged frames changed"));
        }
        let scanned = scan_journal(&frames).map_err(error)?;
        if scanned.tail().is_fragment() {
            return Err(invalid("incomplete signed checkpoint"));
        }
        let mut checkpoint = Checkpoint::default();
        for record in scanned.records() {
            match record {
                StoredRecord::Manifest(m) => checkpoint.manifests.push(m.clone()),
                StoredRecord::Operation(o) => checkpoint.operations.push(o.clone()),
                _ => return Err(invalid("unrelated staged checkpoint record")),
            }
        }
        if checkpoint
            .records()
            .iter()
            .flat_map(mesh_store::frame_record)
            .collect::<Vec<_>>()
            != frames
        {
            return Err(invalid("noncanonical staged checkpoint order"));
        }
        let [operation] = checkpoint.operations.as_slice() else {
            return Err(invalid("initial checkpoint must contain one operation"));
        };
        let operation_id = digest(text(&stage, "operation")?)?;
        if operation.id != operation_id || operation.payload_digest != operation_id {
            return Err(invalid("starting operation identity differs"));
        }
        let actor = PublicKey::from_bytes(*operation.actor.as_bytes());
        let signed_bytes = read(
            &root,
            &format!("object-{}", operation_id.to_hex()),
            16 * 1024 * 1024,
        )?;
        if hash(&signed_bytes) != operation_id {
            return Err(invalid("retained signature payload differs"));
        }
        let envelope =
            AuthenticatedChangeSet::from_canonical_bytes(&signed_bytes).map_err(error)?;
        if !envelope.signed_by(actor) {
            return Err(invalid("starting actor changed"));
        }
        let (prospective, operations) =
            self.with_prepared_input_grant(&selected_grant, &guard, |input| {
                let rules = input.starting_exclusion_rules()?;
                let policy = crate::project_attachment::observation::policy_digest(&rules);
                let mut proposed = Json::parse(&configuration).map_err(error)?;
                let Json::Object(fields) = &mut proposed else {
                    return Err(invalid("invalid destination configuration"));
                };
                fields
                    .iter_mut()
                    .find(|(key, _)| key == "exclusions")
                    .ok_or_else(|| invalid("missing exclusions"))?
                    .1 = Json::text(policy.to_string());
                let (prospective, _) = destination.project().history_configuration_with_previous(
                    &destination.store,
                    Some(policy),
                    Some(proposed.encode()),
                )?;
                let snapshot = prepare_initial_snapshot(
                    &input,
                    WorkspaceId::from_bytes(short_id(prospective.as_bytes())),
                    ActorId::from_bytes(*actor.as_bytes()),
                    request.limits,
                )?;
                // Do not retain a second whole-project content copy while loading staged objects.
                Ok((prospective, snapshot.operations))
            })?;
        let expected_statement = operation_checkpoint_signing_body(
            &AuthenticatedOperationCheckpointRequest::new(
                WorkspaceId::from_bytes(short_id(prospective.as_bytes())),
                ActorId::from_bytes(*actor.as_bytes()),
                SessionId::from_bytes(short_id(actor.as_bytes())),
                ActorSequence::FIRST,
                CausalParents::genesis(),
                HeadId::from_bytes([0; 32]),
                PolicyEpoch::new(1),
                Hlc::new(0, 0),
                operations,
                actor,
                Signature::from_bytes([0; 64]),
            ),
            &StartHead,
        );
        if envelope.changeset() != expected_statement || text(&stage, "prospective")? != prospective
        {
            return Err(invalid("retained signed start differs from granted source"));
        }
        drop(envelope);
        drop(signed_bytes);
        let Some(Json::Array(names)) = stage.get("objects") else {
            return Err(invalid("staged objects missing"));
        };
        let mut budget = request
            .limits
            .bytes
            .checked_add(16 * 1024 * 1024)
            .ok_or_else(|| invalid("recovery budget overflow"))?;
        let mut expected_objects = checkpoint
            .manifests
            .iter()
            .flat_map(|manifest| manifest.chunks.iter().map(|chunk| chunk.digest))
            .chain(std::iter::once(operation_id))
            .collect::<std::collections::BTreeSet<_>>();
        if names.len() != expected_objects.len() {
            return Err(invalid("staged object set differs from checkpoint"));
        }
        let mut objects = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for name in names {
            let id = digest(
                name.as_text()
                    .ok_or_else(|| invalid("invalid staged object"))?,
            )?;
            if !seen.insert(id) || !expected_objects.remove(&id) {
                return Err(invalid("duplicate staged object"));
            }
            let bytes = read(
                &root,
                &format!("object-{}", id.to_hex()),
                usize::try_from(budget.min(if id == operation_id {
                    16 * 1024 * 1024
                } else {
                    request.limits.file_bytes
                }))
                .map_err(error)?,
            )?;
            budget = budget
                .checked_sub(bytes.len() as u64)
                .ok_or_else(|| invalid("recovery objects exceed budget"))?;
            if hash(&bytes) != id {
                return Err(invalid("recovery object digest differs"));
            }
            objects.push(bytes);
        }
        let checkpoint = PreparedAuthenticatedCheckpoint {
            checkpoint,
            objects,
            changeset_id: operation_id,
        };
        let plan = crate::project_attachment::consumption_plan::InitialPlan::verify(
            &checkpoint,
            WorkspaceId::from_bytes(short_id(prospective.as_bytes())),
            request.limits,
        )?;
        let candidate = PreparedNativeConsumedStart {
            owner: owner.clone(),
            source: request.input.source.clone(),
            destination: destination.clone(),
            version: request.input.version,
            grant: request.input.grant,
            available: request.available.iter().map(|w| (*w).clone()).collect(),
            request: request.request,
            limits: request.limits,
            actor,
            basis: Basis {
                configuration: configuration.clone(),
                prospective,
                graph: graph.digest(),
                destination: destination_binding.clone(),
                enrollment: facts.binding(),
            },
            checkpoint,
            plan,
        };
        let (_, owner_proof) = owner
            .project()
            .read_configuration(owner.metadata_path(), &owner.store)?;
        let owner_binding = owner_proof
            .ok_or_else(|| invalid("owner enrollment missing"))?
            .binding();
        if body
            != &candidate.start_body(
                owner_binding,
                &source_binding,
                &destination_binding,
                descriptor_id,
            )
            || candidate.verify_stage_state(
                &root,
                original_graph,
                &origin.allocation,
                &guard,
                installed,
            )? != stage_id
        {
            return Err(invalid("recovered consumption selection differs"));
        }
        if installed {
            candidate.verify_install_names()?;
            let occupied = !destination
                .attachment
                .pinned
                .filesystem()
                .read_directory_names_bounded(Path::new(""), candidate.top_entries().len())?
                .is_empty();
            let (before, _) = facts
                .pending()
                .ok_or_else(|| invalid("start intent missing"))?;
            if occupied
                && read(
                    &destination.store,
                    crate::RECORD_FILE_NAME,
                    80 * 1024 * 1024,
                )?
                .len()
                    < before + mesh_store::frame_record(&StoredRecord::Dependency(record)).len()
            {
                return Err(invalid("installed work lacks a complete start"));
            }
        }
        self.with_prepared_input_grant(&selected_grant, &guard, |_| Ok(()))?;
        let (after_configuration, after_facts) = destination.project().read_native_facts(
            destination.metadata_path(),
            &destination.store,
            Some(&pending),
            None,
        )?;
        if after_configuration != configuration
            || after_facts.as_ref() != Some(&facts)
            || read_private_in_store(&destination.store, super::super::START_PENDING)? != pending
        {
            return Err(invalid("recovery history changed during inspection"));
        }
        guard.ensure_current().map_err(error)?;
        Ok((
            candidate,
            StagedNativeConsumedStart {
                root,
                receipt: stage_id,
                request: request.request,
            },
        ))
    }
}

#[cfg(test)]
pub(super) fn run_child_if_requested() -> bool {
    use std::io::Write as _;
    let Ok(raw) = std::env::var("MESH_FENCED_START_RESTART") else {
        return false;
    };
    let value = Json::parse(&raw).unwrap();
    let text = |name| value.get(name).and_then(Json::as_text).unwrap();
    let storage = AttachmentStorage::open(Path::new(text("storage"))).unwrap();
    let owner = storage.reopen(text("owner")).unwrap();
    let source = storage.reopen(text("source")).unwrap();
    let destination = storage.reopen(text("destination")).unwrap();
    let version = source
        .saved_versions()
        .unwrap()
        .into_iter()
        .find(|v| v.operation().to_hex() == text("version"))
        .unwrap();
    let (candidate, staged) = storage
        .recover_consumed_start_state(
            &owner,
            NativeConsumedStartRequest {
                input: NativeGrantInspection {
                    source: &source,
                    version,
                    destination: &destination,
                    grant: digest(text("grant")).unwrap(),
                },
                available: &[],
                request: digest(text("request")).unwrap(),
                limits: ObservationLimits::default(),
            },
            text("mode").starts_with("install") || text("mode").starts_with("checkpoint"),
        )
        .unwrap();
    assert_eq!(candidate.operation().to_hex(), text("operation"));
    assert_eq!(staged.receipt().unwrap().to_hex(), text("stage"));
    assert_eq!(identity(&staged.root).unwrap(), text("physical"));
    if text("mode").starts_with("checkpoint") {
        candidate
            .commit_phase_with_io(
                &storage,
                &staged,
                true,
                true,
                |step, file, frames| {
                    if text("mode") == "checkpoint-partial" && step == "checkpoint-staged" {
                        file.write_all(&frames[..1])?;
                        file.sync_all()?;
                        std::process::exit(75);
                    }
                    if text("mode") == "checkpoint-lost" && step == "checkpoint-synced" {
                        std::process::exit(75);
                    }
                    Ok(())
                },
                |file| file.sync_all(),
            )
            .unwrap();
        assert!(destination.saved_versions().is_err());
        return true;
    }
    if text("mode").starts_with("install") {
        let receipt = candidate
            .fence_and_install_with_io(
                &storage,
                &staged,
                true,
                |step, _, _| {
                    if (text("mode") == "install-partial" && step == "installed-entry")
                        || (text("mode") == "install-lost" && step == "installed")
                    {
                        std::process::exit(75);
                    }
                    Ok(())
                },
                |file| file.sync_all(),
            )
            .unwrap();
        assert_eq!(receipt.to_hex(), text("receipt"));
        assert!(destination.saved_versions().is_err());
        return true;
    }
    if text("mode") == "partial" || text("mode") == "synced" {
        candidate
            .fence_with_io(
                &storage,
                &staged,
                |step, file, frame| {
                    if text("mode") == "partial" && step == "staged" {
                        file.write_all(&frame[..1])?;
                        file.sync_all()?;
                        std::process::exit(75);
                    }
                    if text("mode") == "synced" && step == "synced" {
                        std::process::exit(75);
                    }
                    Ok(())
                },
                |file| file.sync_all(),
            )
            .unwrap();
        panic!("writer must exit before reply");
    }
    let receipt = candidate
        .fence_consumption_start(&storage, &staged)
        .unwrap();
    assert_eq!(receipt.to_hex(), text("receipt"));
    assert!(destination.saved_versions().is_err());
    assert_eq!(
        fs::read_dir(destination.project().root()).unwrap().count(),
        0
    );
    println!("fenced start recovered without signer");
    true
}

#[cfg(test)]
pub(super) fn assert_reloaded(
    prepared: &PreparedNativeConsumedStart,
    storage: &AttachmentStorage,
    staged: &StagedNativeConsumedStart,
) {
    let journal_path = prepared
        .destination
        .metadata_path()
        .join(crate::RECORD_FILE_NAME);
    let before = fs::read(&journal_path).unwrap();
    let pending =
        read_private_in_store(&prepared.destination.store, super::super::START_PENDING).unwrap();
    let value = Json::parse(&pending).unwrap();
    let expected = text(&value, "payload").unwrap();
    let request = |limits| NativeConsumedStartRequest {
        input: NativeGrantInspection {
            source: &prepared.source,
            version: prepared.version,
            destination: &prepared.destination,
            grant: prepared.grant,
        },
        available: &[],
        request: prepared.request,
        limits,
    };
    let mut wrong_limits = prepared.limits;
    wrong_limits.file_bytes += 1;
    assert!(storage
        .recover_fenced_consumed_start(&prepared.owner, request(wrong_limits))
        .is_err());
    assert_eq!(fs::read(&journal_path).unwrap(), before);
    let (stage_path, _pin) = staged.root.stable_namespace().unwrap();
    let signed_path = stage_path.join(format!("object-{}", prepared.operation().to_hex()));
    let signed = fs::read(&signed_path).unwrap();
    let mut corrupted = signed.clone();
    *corrupted.last_mut().unwrap() ^= 1;
    fs::write(&signed_path, &corrupted).unwrap();
    assert!(storage
        .recover_fenced_consumed_start(&prepared.owner, request(prepared.limits))
        .is_err());
    assert_eq!(fs::read(&signed_path).unwrap(), corrupted);
    assert_eq!(fs::read(&journal_path).unwrap(), before);
    fs::write(&signed_path, signed).unwrap();
    for mode in ["partial", "synced", "complete", "complete"] {
        let value = Json::object([
            ("storage", Json::text(storage.path.to_string_lossy())),
            ("owner", Json::text(prepared.owner.id())),
            ("source", Json::text(prepared.source.id())),
            ("destination", Json::text(prepared.destination.id())),
            ("version", Json::text(prepared.version.operation().to_hex())),
            ("request", Json::text(prepared.request.to_hex())),
            ("grant", Json::text(prepared.grant.to_hex())),
            ("stage", Json::text(staged.receipt.to_hex())),
            ("physical", Json::text(identity(&staged.root).unwrap())),
            ("operation", Json::text(prepared.operation().to_hex())),
            ("receipt", Json::text(expected)),
            ("mode", Json::text(mode)),
        ]);
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "project_attachment::consumption_prepare::tests::saved_ignore_rules_bind_candidate_without_changing_empty_reservation", "--nocapture"])
            .env_remove("MESH_PRIVATE_STAGE_RESTART")
            .env("MESH_FENCED_START_RESTART", value.encode()).output().unwrap();
        assert_eq!(
            result.status.code(),
            Some(if mode == "complete" { 0 } else { 75 }),
            "child {mode}: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        if mode == "partial" {
            assert_eq!(fs::read(&journal_path).unwrap().len(), before.len() + 1);
        }
        assert_eq!(staged.receipt().unwrap(), staged.receipt);
        assert_eq!(
            read_private_in_store(&prepared.destination.store, super::super::START_PENDING)
                .unwrap(),
            pending
        );
    }
    // Reset only this fixture's verified journal to exercise each independent byte boundary next.
    fs::write(&journal_path, before).unwrap();
}
