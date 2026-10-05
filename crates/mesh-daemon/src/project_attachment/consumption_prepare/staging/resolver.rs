//! Resolve completed lane histories from native transaction records under one complete custody set.
use super::*;
use crate::project_attachment::{
    dependency_owner_context::OwnerHistoryContext,
    dependency_transaction::{digest, read_payload, text},
};
use crate::{root_authority::PinnedRootFs, workspace_custody::WorkspaceInitializationGuard};
use mesh_cas::{Blake3, Cas};

struct Selection<'a> {
    work: &'a ProvisionedAttachment,
    source: (RecordDigest, RecordDigest),
    operation: RecordDigest,
    grant: RecordDigest,
    request: RecordDigest,
    limits: ObservationLimits,
}
impl Selection<'_> {
    fn read(work: &ProvisionedAttachment) -> io::Result<Selection<'_>> {
        // These are local facts used only to select a reconstruction attempt, never a read proof.
        let (_, facts) =
            work.project()
                .read_native_facts(work.metadata_path(), &work.store, None, None)?;
        let facts = facts.ok_or_else(|| invalid("resolved lane enrollment missing"))?;
        let (start, _, _) = facts
            .policy()
            .completed_consumption_records()
            .ok_or_else(|| invalid("resolved lane consumption is incomplete"))?;
        Self::from_payload(work, start.payload)
    }

    fn from_payload(
        work: &ProvisionedAttachment,
        payload: RecordDigest,
    ) -> io::Result<Selection<'_>> {
        let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
            work.metadata_path(),
            work.store.filesystem().read_only(),
        )
        .map_err(error)?;
        let bytes = read_payload(&cas, payload, 65536)?;
        let value = Json::parse(std::str::from_utf8(&bytes).map_err(error)?).map_err(error)?;
        let body = value
            .get("body")
            .ok_or_else(|| invalid("resolved start body missing"))?;
        let Some(Json::Array(source)) = body.get("source") else {
            return Err(invalid("resolved source missing"));
        };
        let [Json::Array(pair), operation] = source.as_slice() else {
            return Err(invalid("resolved source shape differs"));
        };
        let [work_id, installation] = pair.as_slice() else {
            return Err(invalid("resolved source binding differs"));
        };
        let id = |value: &Json| {
            digest(
                value
                    .as_text()
                    .ok_or_else(|| invalid("resolved digest missing"))?,
            )
        };
        let bytes = read_payload(&cas, digest(text(body, "staged")?)?, 4096)?;
        let descriptor = Json::parse(std::str::from_utf8(&bytes).map_err(error)?).map_err(error)?;
        let Some(Json::Array(limits)) = descriptor.get("limits") else {
            return Err(invalid("resolved limits missing"));
        };
        let [Json::Number(entries), Json::Number(bytes), Json::Number(file_bytes)] =
            limits.as_slice()
        else {
            return Err(invalid("resolved limits shape differs"));
        };
        let limits = ObservationLimits {
            entries: usize::try_from(*entries).map_err(error)?,
            bytes: *bytes,
            file_bytes: *file_bytes,
        };
        limits.validate()?;
        Ok(Selection {
            work,
            source: (id(work_id)?, id(installation)?),
            operation: id(operation)?,
            grant: digest(text(body, "grant")?)?,
            request: digest(text(body, "request")?)?,
            limits,
        })
    }
}

impl AttachmentStorage {
    pub(in crate::project_attachment) fn consumed_capture_selection_hint(
        &self,
        work: &ProvisionedAttachment,
    ) -> io::Result<Option<crate::project_attachment::consumption_prepare::NativeConsumedCaptureHint>>
    {
        let _guard =
            crate::workspace_custody::lock_workspace_initialization(&work.store).map_err(error)?;
        let raw = match crate::project_attachment::dependency_enrollment::read_private_in_store(
            &work.store,
            super::super::START_PENDING,
        ) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let intent = Json::parse(&raw).map_err(error)?;
        if intent.get("schema").and_then(Json::as_text)
            != Some("mesh.native-consumption-start-intent/v1")
        {
            return Err(invalid("capture start hint has unknown schema"));
        }
        let selection = Selection::from_payload(work, digest(text(&intent, "payload")?)?)?;
        if digest(text(&intent, "request")?)? != selection.request {
            return Err(invalid("capture start hint request differs"));
        }
        Ok(Some(
            crate::project_attachment::consumption_prepare::NativeConsumedCaptureHint {
                source: selection.source,
                operation: selection.operation,
                grant: selection.grant,
                request: selection.request,
                limits: selection.limits,
            },
        ))
    }

    pub(in crate::project_attachment) fn resolve_consumed_histories<'a>(
        &self,
        owner: &'a ProvisionedAttachment,
        works: &[&ProvisionedAttachment],
        guard: &WorkspaceInitializationGuard,
        mut context: OwnerHistoryContext<'a>,
    ) -> io::Result<OwnerHistoryContext<'a>> {
        if works.len() > 256 {
            return Err(invalid("resolved history count exceeds bound"));
        }
        let mut unique = BTreeMap::new();
        for work in std::iter::once(owner).chain(works.iter().copied()) {
            let selected = self.prepare_dependency_work(owner, work)?;
            guard.require_roots(&selected.roots).map_err(error)?;
            if let Some(previous) = unique.insert(work.id(), work) {
                if previous.store.identity()? != work.store.identity()?
                    || previous.project().receipt()? != work.project().receipt()?
                {
                    return Err(invalid("conflicting resolved native histories"));
                }
            }
        }
        let mut admitted = Vec::new();
        let mut pending = Vec::new();
        for work in unique.values().copied() {
            match context.read(work) {
                Ok(_) => admitted.push(work),
                Err(_) => pending.push(Selection::read(work)?),
            }
        }
        // Each successful pass admits at least one native history. Unresolved sources, cycles,
        // missing input handles and corrupt records cannot become partial graph success.
        while !pending.is_empty() {
            let before = pending.len();
            let mut remaining = Vec::new();
            for selection in pending {
                let result = (|| {
                    let mut source = None;
                    let mut available = Vec::new();
                    for work in &admitted {
                        let selected = self.prepare_dependency_work(owner, work)?;
                        let Ok(binding) = context.validate(self, &selected, guard) else {
                            // An empty descendant may need this very parent proof for correlation.
                            // Omit it from this source reconstruction, then validate the full set below.
                            continue;
                        };
                        available.push(*work);
                        if (binding.work(), binding.installation()) == selection.source
                            && source.replace(*work).is_some()
                        {
                            return Err(invalid("ambiguous resolved source"));
                        }
                    }
                    let source = source.ok_or_else(|| invalid("resolved source is unavailable"))?;
                    let (_, _, history) = context.history(source)?;
                    let version = SavedAttachmentVersion::from_verified_history(
                        &history,
                        selection.operation,
                    )?;
                    let request = NativeConsumedStartRequest {
                        input: NativeGrantInspection {
                            source,
                            version,
                            destination: selection.work,
                            grant: selection.grant,
                        },
                        available: &available,
                        request: selection.request,
                        limits: selection.limits,
                    };
                    self.with_recovered_consumed_state_held(
                        owner,
                        request,
                        recovery::RecoveryPhase::CompletedRead,
                        guard,
                        context.clone(),
                        |candidate, staged, graph, guard| {
                            let (configuration, proof) = candidate
                                .verify_completed_history_with_owner(
                                    graph, guard, None, &context,
                                )?;
                            let roots = candidate.completed_graph_roots(&staged, graph)?;
                            context.clone().with_verified_history(
                                selection.work,
                                configuration,
                                proof,
                                roots,
                            )
                        },
                    )
                })();
                match result {
                    Ok(next) => {
                        context = next;
                        admitted.push(selection.work);
                    }
                    Err(_) => remaining.push(selection),
                }
            }
            if remaining.len() == before {
                return Err(invalid(
                    "native consumed histories cannot be resolved completely",
                ));
            }
            pending = remaining;
        }
        for work in admitted {
            let selected = self.prepare_dependency_work(owner, work)?;
            context.validate(self, &selected, guard)?;
            context.read(work)?;
            context.verified_history_roots(work)?;
        }
        guard.ensure_current().map_err(error)?;
        Ok(context)
    }
}
