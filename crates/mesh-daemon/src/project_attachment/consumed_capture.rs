//! Native catalog context shared by consumed capture preparation, commit and exact recovery.
use super::*;
use crate::project_attachment::{
    AttachmentStorage, NativeConsumedStartRequest, NativeGrantInspection, VerifiedDependencyRead,
};
use std::sync::Arc;
pub(super) type CaptureRead<'a> =
    dyn Fn(Option<&str>) -> io::Result<(String, VerifiedDependencyRead)> + 'a;
#[derive(Clone)]
pub(super) struct ConsumedCaptureContext {
    storage: Arc<AttachmentStorage>,
    owner: ProvisionedAttachment,
    source: ProvisionedAttachment,
    destination: ProvisionedAttachment,
    version: SavedAttachmentVersion,
    grant: RecordDigest,
    request: RecordDigest,
    limits: super::super::super::ObservationLimits,
    available: Vec<ProvisionedAttachment>,
}
impl ConsumedCaptureContext {
    fn new(
        storage: &AttachmentStorage,
        owner: &ProvisionedAttachment,
        request: NativeConsumedStartRequest<'_>,
    ) -> Self {
        Self {
            storage: Arc::new(AttachmentStorage {
                path: storage.path.clone(),
                pinned: storage.pinned.clone(),
            }),
            owner: owner.clone(),
            source: request.input.source.clone(),
            destination: request.input.destination.clone(),
            version: request.input.version,
            grant: request.input.grant,
            request: request.request,
            limits: request.limits,
            available: request.available.iter().map(|w| (*w).clone()).collect(),
        }
    }
    fn with_custody<T>(
        &self,
        action: impl FnOnce(&CaptureRead<'_>) -> io::Result<T>,
    ) -> io::Result<T> {
        let available = self.available.iter().collect::<Vec<_>>();
        self.storage.with_completed_capture_context(
            &self.owner,
            NativeConsumedStartRequest {
                input: NativeGrantInspection {
                    source: &self.source,
                    version: self.version,
                    destination: &self.destination,
                    grant: self.grant,
                },
                available: &available,
                request: self.request,
                limits: self.limits,
            },
            action,
        )
    }
}
impl ProvisionedAttachment {
    pub(super) fn with_capture_custody<T>(
        &self,
        consumption: Option<&ConsumedCaptureContext>,
        action: impl FnOnce(&CaptureRead<'_>) -> io::Result<T>,
    ) -> io::Result<T> {
        if let Some(context) = consumption {
            if self.id() != context.destination.id()
                || self.store.identity()? != context.destination.store.identity()?
            {
                return Err(invalid("capture context belongs to another destination"));
            }
            return context.with_custody(action);
        }
        let guard =
            crate::workspace_custody::lock_workspace_initialization(&self.store).map_err(error)?;
        let read = |capture: Option<&str>| {
            let (configuration, proof) = match capture {
                Some(raw) => self.attachment.read_capture_configuration(
                    self.metadata_path(),
                    &self.store,
                    raw,
                )?,
                None => self
                    .attachment
                    .read_configuration(self.metadata_path(), &self.store)?,
            };
            Ok((
                configuration,
                proof.ok_or_else(|| invalid("native capture enrollment missing"))?,
            ))
        };
        let result = action(&read)?;
        guard.ensure_current().map_err(error)?;
        Ok(result)
    }
}
impl AttachmentStorage {
    /// Prepare a later private save with complete native consumption context. Signing remains
    /// outside custody; commit reconstructs and verifies the context again before writing.
    pub fn prepare_consumed_capture<F, E>(
        &self,
        owner: &ProvisionedAttachment,
        start: NativeConsumedStartRequest<'_>,
        input: &CapturedProjectInput,
        actor: PublicKey,
        request: RecordDigest,
        sign: F,
    ) -> io::Result<PreparedNativeCapture>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        let context = ConsumedCaptureContext::new(self, owner, start);
        let destination = context.destination.clone();
        destination.prepare_capture_in(input, actor, request, sign, Some(context))
    }
    /// Recover one exact later capture without resigning or changing the original working files.
    pub fn recover_consumed_capture(
        &self,
        owner: &ProvisionedAttachment,
        start: NativeConsumedStartRequest<'_>,
        request: RecordDigest,
    ) -> io::Result<SavedAttachmentVersion> {
        let context = ConsumedCaptureContext::new(self, owner, start);
        context
            .destination
            .with_capture_custody(Some(&context), |read| {
                context
                    .destination
                    .recover_capture_held(request, read, |file| file.sync_all())
            })
    }
}

#[cfg(test)]
pub(in crate::project_attachment) fn assert_consumed_capture(
    storage: &AttachmentStorage,
    owner: &ProvisionedAttachment,
    start: NativeConsumedStartRequest<'_>,
) {
    use ed25519_dalek::{Signer as _, SigningKey};
    let request = || NativeConsumedStartRequest {
        input: NativeGrantInspection {
            source: start.input.source,
            version: start.input.version,
            destination: start.input.destination,
            grant: start.input.grant,
        },
        available: start.available,
        request: start.request,
        limits: start.limits,
    };
    let destination = start.input.destination;
    let key = SigningKey::from_bytes(&[129; 32]);
    let actor = PublicKey::from_bytes(key.verifying_key().to_bytes());
    let sign = |payload: &SigningPayload| {
        // Neither owning nor destination custody may be retained across a signer callback.
        let _guard = crate::workspace_custody::lock_workspace_initialization_set(&[
            owner.store.clone(),
            destination.store.clone(),
        ])
        .unwrap();
        Ok::<_, &'static str>(Signature::from_bytes(
            key.sign(payload.as_bytes()).to_bytes(),
        ))
    };
    let id = |n| RecordDigest::from_bytes([n; 32]);
    let input = destination.project().capture_inputs(start.limits).unwrap();
    let original = storage.saved_consumed_versions(owner, request()).unwrap();
    assert_eq!(original.len(), 1);
    let stale = storage
        .prepare_consumed_capture(owner, request(), &input, actor, id(111), sign)
        .unwrap();
    let first = storage
        .prepare_consumed_capture(owner, request(), &input, actor, id(110), sign)
        .unwrap();
    let operation = first.operation();
    let journal_path = destination.metadata_path().join(crate::RECORD_FILE_NAME);
    let marker_path = destination.metadata_path().join(super::super::HISTORY);
    let marker = fs::read(&marker_path).unwrap();
    let before = fs::read(&journal_path).unwrap();
    let failure = first
        .commit_with_io(
            |step, file, frames| {
                if matches!(step, CaptureStep::Staged) {
                    file.write_all(&frames[..1])?;
                    file.sync_all()?;
                    return Err(io::Error::other("capture interrupted after one byte"));
                }
                Ok(())
            },
            |file| file.sync_all(),
        )
        .unwrap_err();
    assert_eq!(failure.to_string(), "capture interrupted after one byte");
    assert_eq!(fs::read(&journal_path).unwrap().len(), before.len() + 1);
    assert!(storage.saved_consumed_versions(owner, request()).is_err());
    let child = |capture: RecordDigest, operation: RecordDigest| {
        let value = Json::object([
            ("storage", Json::text(storage.path.to_string_lossy())),
            ("owner", Json::text(owner.id())),
            ("source", Json::text(start.input.source.id())),
            ("destination", Json::text(destination.id())),
            (
                "version",
                Json::text(start.input.version.operation().to_hex()),
            ),
            ("grant", Json::text(start.input.grant.to_hex())),
            ("request", Json::text(start.request.to_hex())),
            ("operation", Json::text(operation.to_hex())),
            ("capture", Json::text(capture.to_hex())),
            ("mode", Json::text("capture-recover")),
        ]);
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "project_attachment::consumption_prepare::tests::saved_ignore_rules_bind_candidate_without_changing_empty_reservation", "--nocapture"])
            .env_remove("MESH_PRIVATE_STAGE_RESTART").env("MESH_FENCED_START_RESTART", value.encode()).output().unwrap();
        assert!(
            child.status.success(),
            "{} {}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
    };
    child(id(110), operation);
    let first_committed = fs::read(&journal_path).unwrap();
    child(id(110), operation);
    assert_eq!(fs::read(&journal_path).unwrap(), first_committed);
    let saved = storage.saved_consumed_versions(owner, request()).unwrap();
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[1].operation(), operation);
    assert_eq!(
        storage
            .consumed_saved_file(owner, request(), saved[0], "kept")
            .unwrap(),
        Some(b"saved bytes".to_vec())
    );
    assert_eq!(
        storage
            .consumed_saved_file(owner, request(), saved[1], "kept")
            .unwrap(),
        Some(b"later unsaved editor work".to_vec())
    );
    assert!(stale.commit().is_err());
    assert_eq!(fs::read(&journal_path).unwrap(), first_committed);
    fs::write(
        destination.project().root().join("kept"),
        b"second captured progress",
    )
    .unwrap();
    let input = destination.project().capture_inputs(start.limits).unwrap();
    let next = storage
        .prepare_consumed_capture(owner, request(), &input, actor, id(112), sign)
        .unwrap()
        .commit()
        .unwrap();
    let later = fs::read(&journal_path).unwrap();
    child(id(110), operation);
    assert_eq!(fs::read(&journal_path).unwrap(), later);
    assert_eq!(
        storage
            .saved_consumed_versions(owner, request())
            .unwrap()
            .last(),
        Some(&next)
    );
    fs::write(
        destination.project().root().join("kept"),
        b"sync recovery progress",
    )
    .unwrap();
    let input = destination.project().capture_inputs(start.limits).unwrap();
    let pending = storage
        .prepare_consumed_capture(owner, request(), &input, actor, id(113), sign)
        .unwrap();
    let last_operation = pending.operation();
    let failure = pending
        .commit_with_io(
            |_, _, _| Ok(()),
            |_| Err(io::Error::other("consumed capture sync refused")),
        )
        .unwrap_err();
    assert_eq!(failure.to_string(), "consumed capture sync refused");
    child(id(113), last_operation);
    let final_journal = fs::read(&journal_path).unwrap();
    child(id(113), last_operation);
    assert_eq!(fs::read(&journal_path).unwrap(), final_journal);
    let final_versions = storage.saved_consumed_versions(owner, request()).unwrap();
    assert_eq!(final_versions.len(), 4);
    assert_eq!(final_versions.last().unwrap().operation(), last_operation);
    assert_eq!(
        storage
            .consumed_saved_file(owner, request(), *final_versions.last().unwrap(), "kept")
            .unwrap(),
        Some(b"sync recovery progress".to_vec())
    );
    assert_eq!(
        fs::read(destination.project().root().join("new-editor-file")).unwrap(),
        b"preserve me"
    );
    assert_eq!(fs::read(&marker_path).unwrap(), marker);
    // A prepared signature is not a cached cross-store permission. Damage owning history after
    // signing and require commit to refuse before touching destination history or its capture line.
    fs::write(
        destination.project().root().join("kept"),
        b"next unsaved work",
    )
    .unwrap();
    let input = destination.project().capture_inputs(start.limits).unwrap();
    let stale_owner = storage
        .prepare_consumed_capture(owner, request(), &input, actor, id(114), sign)
        .unwrap();
    let owner_path = owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let owner_before = fs::read(&owner_path).unwrap();
    fs::write(&owner_path, &owner_before[..owner_before.len() - 1]).unwrap();
    assert!(stale_owner.commit().is_err());
    assert_eq!(fs::read(&journal_path).unwrap(), final_journal);
    assert!(!destination.metadata_path().join(PENDING).exists());
    fs::write(&owner_path, &owner_before).unwrap();
    assert_eq!(
        storage.saved_consumed_versions(owner, request()).unwrap(),
        final_versions
    );
    assert_eq!(
        fs::read(destination.project().root().join("kept")).unwrap(),
        b"next unsaved work"
    );
    let graph = storage
        .inspect_consumed_dependency_graph(owner, request(), *final_versions.last().unwrap())
        .unwrap();
    assert_eq!(graph.operation_count(), final_versions.len() + 1);
    assert_eq!(
        graph,
        storage
            .inspect_consumed_dependency_graph(owner, request(), *final_versions.last().unwrap())
            .unwrap()
    );
    assert_eq!(
        storage
            .inspect_dependency_graph(
                owner,
                destination,
                *final_versions.last().unwrap(),
                &[start.input.source]
            )
            .unwrap(),
        graph,
        "native graph must resolve the consumed history without a caller-provided start request"
    );
    {
        let guard = crate::workspace_custody::lock_workspace_initialization_set(
            std::slice::from_ref(&destination.store),
        )
        .unwrap();
        let context =
            crate::project_attachment::dependency_owner_context::OwnerHistoryContext::current(
                owner,
            );
        assert!(
            storage
                .resolve_consumed_histories(owner, &[destination], &guard, context)
                .is_err(),
            "resolver cannot extend incomplete custody"
        );
    }
    {
        // Restore even when inspection or an assertion panics.
        struct RestoreJournal(std::path::PathBuf, Vec<u8>);
        impl Drop for RestoreJournal {
            fn drop(&mut self) {
                fs::write(&self.0, &self.1).unwrap();
            }
        }
        let path = owner.metadata_path().join(crate::RECORD_FILE_NAME);
        let original = fs::read(&path).unwrap();
        let restore = RestoreJournal(path.clone(), original);
        fs::write(&path, &restore.1[..restore.1.len() - 1]).unwrap();
        let refused = storage
            .inspect_dependency_graph(
                owner,
                destination,
                *final_versions.last().unwrap(),
                &[start.input.source],
            )
            .is_err();
        drop(restore);
        assert!(
            refused,
            "automatic resolution cannot use a torn owning receipt"
        );
    }
    let retained = graph.retained_content_json();
    let Some(Json::Array(stores)) = retained.get("stores") else {
        panic!("retained stores missing")
    };
    assert_eq!(stores.len(), 2);
    let mut sidecars = BTreeSet::new();
    for store in stores {
        let Some(Json::Array(entries)) = store.get("sidecars") else {
            panic!("retained sidecars missing")
        };
        sidecars.extend(
            entries
                .iter()
                .map(|entry| text(entry, "name").unwrap().to_owned()),
        );
    }
    for name in [
        "consumption-start.pending",
        "consumption-history.pending",
        "consumption-complete.pending",
        "consumption-owner.pending",
    ] {
        assert!(
            sidecars.contains(name),
            "missing consumed recovery root {name}"
        );
    }
    let graph_json = graph.to_json();
    let Some(Json::Array(root)) = graph_json.get("root") else {
        panic!("graph root missing")
    };
    let local = stores
        .iter()
        .find(|store| store.get("work") == root.first())
        .unwrap();
    let Some(Json::Array(local_payloads)) = local.get("payloads") else {
        panic!("local payloads missing")
    };
    let local_payloads = local_payloads
        .iter()
        .map(|p| p.as_text().unwrap())
        .collect::<BTreeSet<_>>();
    assert!(
        !local_payloads.contains(start.input.grant.to_hex().as_str()),
        "owner grant is not lane-local content"
    );
    let start_raw = read_private_in_store(&destination.store, "consumption-start.pending").unwrap();
    let start_json = Json::parse(&start_raw).unwrap();
    let cas = Cas::<_, mesh_cas::Blake3>::with_filesystem(
        destination.metadata_path(),
        destination.store.filesystem().read_only(),
    )
    .unwrap();
    let payload = read_payload(
        &cas,
        digest(text(&start_json, "payload").unwrap()).unwrap(),
        65536,
    )
    .unwrap();
    let envelope = Json::parse(std::str::from_utf8(&payload).unwrap()).unwrap();
    let body = envelope.get("body").unwrap();
    for field in [
        "configuration",
        "prospective",
        "closure",
        "operation",
        "staged",
    ] {
        assert!(
            local_payloads.contains(text(body, field).unwrap()),
            "missing local consumed reference {field}"
        );
    }
    let descriptor =
        read_payload(&cas, digest(text(body, "staged").unwrap()).unwrap(), 4096).unwrap();
    let descriptor = Json::parse(std::str::from_utf8(&descriptor).unwrap()).unwrap();
    assert!(local_payloads.contains(text(&descriptor, "stage").unwrap()));
    storage.assert_consumed_graph_context(owner, request());
    assert_eq!(fs::read(&marker_path).unwrap(), marker);
    assert_eq!(fs::read(&journal_path).unwrap(), final_journal);
    assert!(destination.saved_versions().is_err());
    storage.assert_consumed_child_reservation(owner, &start, *final_versions.last().unwrap());
}
