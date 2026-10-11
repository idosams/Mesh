//! Complete owning-store history during an exact already-signed publication recovery.
use super::super::retention::NativeHistoryRecovery;
use super::*;

fn ensure_retention_available(
    owner: &ProvisionedAttachment,
    work: &ProvisionedAttachment,
) -> io::Result<()> {
    for selected in [owner, work] {
        super::super::super::detachment::ensure_attached(&selected.store)?;
        for pending in [
            super::super::super::dependency_decision::PENDING,
            super::super::super::history::dependency_capture::PENDING,
        ] {
            super::super::super::history::dependency_capture::absent(&selected.store, pending)?;
        }
    }
    if owner.id() != work.id() {
        super::super::super::history::dependency_capture::absent(&work.store, PENDING)?;
    }
    Ok(())
}
impl AttachmentStorage {
    /// Inspect all owner-store saved history during one exact signed publication recovery.
    /// This immutable inventory grants no recovery, deletion, admission or approval authority.
    pub fn inspect_native_history_retention_during_publication(
        &self,
        work_id: &str,
        request: RecordDigest,
        review: RecordDigest,
        receipt: &[u8],
        trusted: &TrustedReviewers,
    ) -> io::Result<Json> {
        if request == ZERO || review == ZERO || receipt.is_empty() || receipt.len() > 65536 {
            return Err(invalid("invalid native publication retention request"));
        }
        let owner = self.reopen(&self.candidate_owning_root(work_id)?)?;
        let work = self.reopen(work_id)?;
        self.exact_registered_work(&owner)?;
        self.exact_registered_work(&work)?;
        let selection = self.prepare_dependency_work(&owner, &owner)?;
        let hints = self.catalog_discovery_hints(&owner)?;
        let (discovery, prefix) = {
            let guard =
                crate::workspace_custody::lock_workspace_initialization_set(&selection.roots)
                    .map_err(error)?;
            ensure_retention_available(&owner, &owner)?;
            let prefix = recovery::VerifiedPublicationPrefix::read_retention(
                &owner, request, review, receipt,
            )?;
            let discovery = self.private_discovery_with_recovery(
                &owner,
                work_id,
                &guard,
                hints,
                Some(OwnerRecovery::Publication(&prefix)),
            )?;
            (discovery, prefix)
        };
        let works = discovery
            .works
            .keys()
            .map(|id| self.reopen(id))
            .collect::<io::Result<Vec<_>>>()?;
        let available = works.iter().collect::<Vec<_>>();
        let prepared = self.prepare_dependency_graph(&owner, &work, ZERO, &available)?;
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&prepared.roots)
            .map_err(error)?;
        let rediscover = || {
            self.private_discovery_with_recovery(
                &owner,
                work_id,
                &guard,
                self.catalog_discovery_hints(&owner)?,
                Some(OwnerRecovery::Publication(&prefix)),
            )
        };
        if rediscover()? != discovery {
            return Err(invalid("publication retention roots changed"));
        }
        ensure_retention_available(&owner, &work)?;
        let context = PrivateContext::resolve_with_recovery(
            self,
            &owner,
            &works,
            &guard,
            Some(OwnerRecovery::Publication(&prefix)),
        )?;
        let inspect = || -> io::Result<Json> {
            self.exact_registered_work(&owner)?;
            self.exact_registered_work(&work)?;
            ensure_retention_available(&owner, &work)?;
            // The same independent receipt, graph, decision-vector and prior-main checks as commit.
            let selected = context
                .select_publication(self, &work, &guard, request, review, receipt, trusted)?;
            let mut file = owner
                .store
                .filesystem()
                .read_only()
                .read_file(Path::new(crate::RECORD_FILE_NAME))?;
            let observed = journal_bytes(&mut file)?;
            let (_, record, payload) = prefix.verify(&owner.store, &file, &observed)?;
            if selected.existing || selected.claim.record != record || selected.payload != payload {
                return Err(invalid(
                    "publication retention differs from exact verified ceremony",
                ));
            }
            let output = selected.claim.review.evidence().output();
            let binding = context.binding(self, &work, &guard)?;
            if output.0 != binding.work() || output.1 != binding.installation() {
                return Err(invalid(
                    "publication retention output differs from selected work",
                ));
            }
            let inventory = context.native_history_retention(
                self,
                &owner,
                trusted,
                &guard,
                Some(NativeHistoryRecovery::Publication(&prefix)),
            )?;
            let Json::Object(mut fields) = inventory else {
                return Err(invalid("publication retention result is unavailable"));
            };
            fields.push((
                "output".into(),
                Json::object([
                    ("work", Json::text(output.0.to_hex())),
                    ("installation", Json::text(output.1.to_hex())),
                    ("operation", Json::text(output.2.to_hex())),
                ]),
            ));
            let result = Json::Object(fields);
            if result.encode().len() > 4 * 1024 * 1024 {
                return Err(invalid("publication retention encoding exceeds bound"));
            }
            prefix.raw_for(&owner)?;
            context.read(&owner, &guard)?;
            context.read(&work, &guard)?;
            Ok(result)
        };
        let result = inspect()?;
        if inspect()? != result || rediscover()? != discovery {
            return Err(invalid("publication retention changed during inspection"));
        }
        self.exact_registered_work(&owner)?;
        self.exact_registered_work(&work)?;
        ensure_retention_available(&owner, &work)?;
        guard.ensure_current().map_err(error)?;
        Ok(result)
    }
}
