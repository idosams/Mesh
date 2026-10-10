//! Store-qualified completed-history facts. No durable pin or collection permission.
use super::*;
use mesh_cas::{Blake3, Cas};

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
            let snapshot = context.read(work, guard)?;
            context.with_replayed_history(self, work, trusted, guard, |history, _| {
                let mut payload_budget = 1024 * 1024 * 1024usize;
                let mut content_budget = 1024 * 1024 * 1024u64;
                let authored = history
                    .retained_authoring_objects(&mut payload_budget, &mut content_budget)
                    .map_err(error)?;
                let operations = authored.operations;
                let manifests = authored.manifests;
                let mut payloads = authored.payloads;
                let captures = work.all_completed_capture_receipt_roots(
                    &operations,
                    &mut payload_budget,
                    &snapshot.configuration,
                    &snapshot.proof,
                )?;
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
                let binding = context.binding(self, work, guard)?;
                let array = |values: BTreeSet<RecordDigest>| {
                    Json::Array(values.into_iter().map(|d| Json::text(d.to_hex())).collect())
                };
                let result = Json::object([
                    ("schema", Json::text("mesh.native-history-retention/v1")),
                    ("scope", Json::text("selected-store-completed-history")),
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
                ]);
                if result.encode().len() > 4 * 1024 * 1024 {
                    return Err(invalid("retention encoding exceeds bound"));
                }
                context.read(work, guard)?;
                Ok(result)
            })
        })
    }
}
