//! Store-qualified completed or exact recovery history facts. No durable pin or collection permission.
use super::*;
use mesh_cas::{Blake3, Cas};
#[path = "retention/reconstruction.rs"]
mod reconstruction;

#[derive(Clone, Copy)]
pub(super) enum NativeHistoryRecovery<'a> {
    Capture(&'a crate::project_attachment::NativeCaptureRetention),
    Control(&'a super::control::recovery::VerifiedControlPrefix),
    Publication(&'a super::publication::recovery::VerifiedPublicationPrefix),
}

impl AttachmentStorage {
    /// Inspect all authored versions and known completed transaction roots in one physical store.
    /// Other stores and unfinished transactions require their own verified inventories. This
    /// result cannot authorize deletion and does not turn private graph roots into a GC oracle.
    pub fn inspect_native_history_retention(
        &self,
        work_id: &str,
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<Json> {
        self.with_private_inspection(work_id, |context, work, guard| {
            for selected in [context.owner, work] {
                super::super::detachment::ensure_attached(&selected.store)?;
                for pending in [
                    super::super::history::dependency_capture::PENDING,
                    super::super::dependency_decision::PENDING,
                    super::publication::PENDING,
                ] {
                    super::super::history::dependency_capture::absent(&selected.store, pending)?;
                }
            }
            context.native_history_retention(self, work, trusted, guard, None)
        })
    }
}

impl PrivateContext<'_> {
    pub(super) fn native_history_retention(
        &self,
        storage: &AttachmentStorage,
        work: &ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        guard: &WorkspaceInitializationGuard,
        recovery: Option<NativeHistoryRecovery<'_>>,
    ) -> io::Result<Json> {
        let snapshot = self.read(work, guard)?;
        self.with_replayed_history(storage, work, trusted, guard, |history, _| {
            let mut payload_budget = 1024 * 1024 * 1024usize;
            let mut content_budget = 1024 * 1024 * 1024u64;
            let (operations, manifests, mut payloads, captures, recovery_facts) = match recovery {
                Some(NativeHistoryRecovery::Capture(capture)) => {
                    // Only this exact authenticated reconstruction may inspect a torn capture.
                    let roots = capture.local_roots(work.store.identity()?)?;
                    (
                        capture.retained_operations().clone(),
                        roots.manifests.clone(),
                        BTreeSet::new(),
                        roots,
                        None,
                    )
                }
                Some(
                    kind @ (NativeHistoryRecovery::Control(_)
                    | NativeHistoryRecovery::Publication(_)),
                ) => {
                    if work.id() != self.owner.id() {
                        return Err(invalid(
                            "dependency recovery history requires the exact owning store",
                        ));
                    }
                    let authored = kind.history_roots(
                        work,
                        &snapshot.configuration,
                        &snapshot.proof,
                        &mut payload_budget,
                        &mut content_budget,
                    )?;
                    let mut captures = work.all_completed_capture_receipt_roots(
                        &authored.operations,
                        &mut payload_budget,
                        &snapshot.configuration,
                        &snapshot.proof,
                    )?;
                    let (name, sidecar, facts) = match kind {
                        NativeHistoryRecovery::Control(prefix) => {
                            let control = work
                                .inspect_pending_dependency_control_retention_with_context(
                                    prefix.request(),
                                    |raw| {
                                        if raw != prefix.raw_for(work)? {
                                            return Err(invalid("control history intent changed"));
                                        }
                                        let current = self.read(work, guard)?;
                                        Ok((current.configuration.clone(), current.proof.clone()))
                                    },
                                )?;
                            (
                                "control",
                                super::super::dependency_decision::PENDING,
                                control,
                            )
                        }
                        NativeHistoryRecovery::Publication(prefix) => (
                            "publication",
                            super::publication::PENDING,
                            prefix.retention_facts(work)?,
                        ),
                        NativeHistoryRecovery::Capture(_) => {
                            return Err(invalid("unexpected capture reconstruction"))
                        }
                    };
                    if captures
                        .sidecars
                        .insert(sidecar.to_owned(), hash(kind.raw_for(work)?.as_bytes()))
                        .is_some()
                    {
                        return Err(invalid("recovery sidecar conflicts with capture receipt"));
                    }
                    (
                        authored.operations,
                        authored.manifests,
                        authored.payloads,
                        captures,
                        Some((name, facts)),
                    )
                }
                None => {
                    let authored = history
                        .retained_authoring_objects(&mut payload_budget, &mut content_budget)
                        .map_err(error)?;
                    let captures = work.all_completed_capture_receipt_roots(
                        &authored.operations,
                        &mut payload_budget,
                        &snapshot.configuration,
                        &snapshot.proof,
                    )?;
                    (
                        authored.operations,
                        authored.manifests,
                        authored.payloads,
                        captures,
                        None,
                    )
                }
            };
            if operations.len() > 4096 || manifests.len() > 65536 {
                return Err(invalid("retention history bound exceeded"));
            }
            payloads.extend(captures.payloads);
            payloads.extend(snapshot.proof.policy().policy_payloads());
            // The physical history proof binds this store's retained enrollment intent.
            // The owning project's authority may be different for a consumed child.
            payloads.insert(snapshot.proof.binding().authority);
            payloads.extend(snapshot.retained.payloads.iter().copied());
            if payloads.len() > 65536 {
                return Err(invalid("retention object bound exceeded"));
            }
            let mut sidecars = snapshot.retained.sidecars.clone();
            for (name, digest) in captures.sidecars {
                if sidecars
                    .insert(name, digest)
                    .is_some_and(|old| old != digest)
                {
                    return Err(invalid("retention sidecar conflict"));
                }
            }
            let cas = Cas::<_, Blake3>::with_filesystem(
                work.metadata_path(),
                work.store.filesystem().read_only(),
            )
            .map_err(error)?;
            for digest in &payloads {
                let bytes = super::super::dependency_transaction::read_payload(
                    &cas,
                    *digest,
                    payload_budget.min(64 * 1024 * 1024),
                )?;
                payload_budget = payload_budget
                    .checked_sub(bytes.len())
                    .ok_or_else(|| invalid("retention payload budget exceeded"))?;
            }
            let identity = work.store.identity()?;
            let binding = self.binding(storage, work, guard)?;
            let array = |values: BTreeSet<RecordDigest>| {
                Json::Array(values.into_iter().map(|d| Json::text(d.to_hex())).collect())
            };
            let mut fields = vec![
                (
                    "schema",
                    Json::text(match recovery {
                        Some(NativeHistoryRecovery::Capture(_)) => {
                            "mesh.native-history-capture-retention/v1"
                        }
                        Some(NativeHistoryRecovery::Control(_)) => {
                            "mesh.native-history-control-retention/v1"
                        }
                        Some(NativeHistoryRecovery::Publication(_)) => {
                            "mesh.native-history-publication-retention/v1"
                        }
                        None => "mesh.native-history-retention/v1",
                    }),
                ),
                (
                    "scope",
                    Json::text(match recovery {
                        Some(NativeHistoryRecovery::Capture(_)) => {
                            "selected-store-capture-recovery"
                        }
                        Some(NativeHistoryRecovery::Control(_)) => "owner-store-control-recovery",
                        Some(NativeHistoryRecovery::Publication(_)) => {
                            "owner-store-publication-recovery"
                        }
                        None => "selected-store-completed-history",
                    }),
                ),
                ("collection_authority", Json::Bool(false)),
                ("work", Json::text(binding.work().to_hex())),
                ("installation", Json::text(binding.installation().to_hex())),
                ("device", Json::text(format!("{:x}", identity.0))),
                ("inode", Json::text(format!("{:x}", identity.1))),
                ("operations", array(operations)),
                ("manifests", array(manifests)),
                ("payloads", array(payloads)),
                (
                    "sidecars",
                    Json::Array(
                        sidecars
                            .into_iter()
                            .map(|(name, digest)| {
                                Json::object([
                                    ("name", Json::text(name)),
                                    ("digest", Json::text(digest.to_hex())),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ];
            if let Some(NativeHistoryRecovery::Capture(capture)) = recovery {
                fields.push(("capture", capture.to_json()));
            }
            if let Some((name, facts)) = recovery_facts {
                fields.push((name, facts));
            }
            let result = Json::object(fields);
            if result.encode().len() > 4 * 1024 * 1024 {
                return Err(invalid("retention encoding exceeds bound"));
            }
            self.read(work, guard)?;
            Ok(result)
        })
    }
}
