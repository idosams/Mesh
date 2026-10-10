//! Exact owner-store control recovery facts, without mutation or collection authority.
use super::*;
use crate::project_attachment::dependency_transaction::text;

impl AttachmentStorage {
    /// Inspect one exact owning-store control recovery through configured publication trust.
    /// These bounded local policy roots are neither complete history nor collection permission.
    pub fn inspect_verified_dependency_control_retention(
        &self,
        owner: &ProvisionedAttachment,
        request: RecordDigest,
        trusted: &TrustedReviewers,
    ) -> io::Result<Json> {
        if request == ZERO {
            return Err(invalid("missing native control retention request"));
        }
        self.exact_registered_work(owner)?;
        if self.candidate_owning_root(owner.id())? != owner.id() {
            return Err(invalid(
                "native control retention requires the owning store",
            ));
        }
        let selection = self.prepare_dependency_work(owner, owner)?;
        let hints = self.catalog_discovery_hints(owner)?;
        let (discovery, recovery) = {
            let guard =
                crate::workspace_custody::lock_workspace_initialization_set(&selection.roots)
                    .map_err(error)?;
            ensure_control_available(owner, owner)?;
            let raw = read_private_in_store(&owner.store, PENDING)?;
            let value = Json::parse(&raw).map_err(error)?;
            let kind = match text(&value, "schema")? {
                "mesh.dependency-decision-intent/v1" => DependencyKind::Eligibility,
                "mesh.dependency-review-intent/v1" => DependencyKind::ReviewSnapshot,
                _ => return Err(invalid("unsupported native control retention intent")),
            };
            let recovery = recovery::VerifiedControlPrefix::read_retention(owner, request, kind)?;
            let discovery = self.private_discovery_with_recovery(
                owner,
                owner.id(),
                &guard,
                hints,
                Some(OwnerRecovery::Control(&recovery)),
            )?;
            (discovery, recovery)
        };
        let works = discovery
            .works
            .keys()
            .map(|id| self.reopen(id))
            .collect::<io::Result<Vec<_>>>()?;
        let available = works.iter().collect::<Vec<_>>();
        let prepared = self.prepare_dependency_graph(owner, owner, ZERO, &available)?;
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&prepared.roots)
            .map_err(error)?;
        let rediscover = || {
            self.private_discovery_with_recovery(
                owner,
                owner.id(),
                &guard,
                self.catalog_discovery_hints(owner)?,
                Some(OwnerRecovery::Control(&recovery)),
            )
        };
        if rediscover()? != discovery {
            return Err(invalid("native control retention roots changed"));
        }
        ensure_control_available(owner, owner)?;
        let context = PrivateContext::resolve_with_recovery(
            self,
            owner,
            &works,
            &guard,
            Some(OwnerRecovery::Control(&recovery)),
        )?;
        let inspect = || {
            self.exact_registered_work(owner)?;
            context.with_replayed_history(self, owner, trusted, &guard, |_, _| Ok(()))?;
            let control = owner.inspect_pending_dependency_control_retention_with_context(
                request,
                |raw| {
                    if raw != recovery.raw_for(owner)? {
                        return Err(invalid("native control retention intent changed"));
                    }
                    let snapshot = context.read(owner, &guard)?;
                    Ok((snapshot.configuration.clone(), snapshot.proof.clone()))
                },
            )?;
            let binding = context.binding(self, owner, &guard)?;
            let identity = owner.store.identity()?;
            let result = Json::object([
                (
                    "schema",
                    Json::text("mesh.trusted-native-control-retention/v1"),
                ),
                ("scope", Json::text("owner-store-control-recovery")),
                ("collection_authority", Json::Bool(false)),
                ("work", Json::text(binding.work().to_hex())),
                ("installation", Json::text(binding.installation().to_hex())),
                ("device", Json::text(format!("{:x}", identity.0))),
                ("inode", Json::text(format!("{:x}", identity.1))),
                ("control", control),
            ]);
            if result.encode().len() > 4 * 1024 * 1024 {
                return Err(invalid("native control retention encoding exceeds bound"));
            }
            context.read(owner, &guard)?;
            Ok(result)
        };
        let result = inspect()?;
        if inspect()? != result || rediscover()? != discovery {
            return Err(invalid(
                "native control retention changed during inspection",
            ));
        }
        self.exact_registered_work(owner)?;
        guard.ensure_current().map_err(error)?;
        Ok(result)
    }
}
