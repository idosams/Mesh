//! Explicit owner-prefix context for exact receipt recovery; never an ordinary-read override.
use super::{
    dependency_enrollment::read_private_in_store,
    dependency_read::VerifiedDependencyRead,
    dependency_transaction::{digest, text},
    dependency_work::PreparedDependencyWork,
    invalid, AttachmentStorage, NativeDependencyWorkBinding, ProvisionedAttachment,
};
use crate::{ipc::Json, workspace::OpenWorkspace, workspace_custody::WorkspaceInitializationGuard};
use mesh_store::{DependencyKind, RecordDigest};
use std::io;
pub(super) fn pending_name(request: RecordDigest, payload: RecordDigest) -> String {
    format!(
        "consumption-owner-{}-{}.pending",
        request.to_hex(),
        payload.to_hex()
    )
}
pub(super) struct OwnerHistoryContext<'a> {
    owner: &'a ProvisionedAttachment,
    pending: Option<(String, String)>,
    request: Option<RecordDigest>,
    operation: Option<RecordDigest>,
}
impl<'a> OwnerHistoryContext<'a> {
    pub(super) fn current(owner: &'a ProvisionedAttachment) -> Self {
        Self {
            owner,
            pending: None,
            request: None,
            operation: None,
        }
    }
    pub(super) fn recovering(
        owner: &'a ProvisionedAttachment,
        request: RecordDigest,
    ) -> io::Result<Self> {
        // Full readable history wins: historical receipt lookup must precede current permission.
        let ordinary = owner
            .project()
            .read_configuration(owner.metadata_path(), &owner.store);
        if ordinary.is_ok() {
            let mut context = Self::current(owner);
            context.request = Some(request);
            return Ok(context);
        }
        let ordinary_error = ordinary.err().unwrap().to_string();
        let prefix = format!("consumption-owner-{}-", request.to_hex());
        let names = owner
            .store
            .filesystem()
            .read_directory_names_bounded(std::path::Path::new(""), 16384)?;
        let candidates = names
            .iter()
            .filter_map(|name| name.to_str())
            .filter(|name| name.starts_with(&prefix))
            .collect::<Vec<_>>();
        if candidates.len() > 256 {
            return Err(invalid("owner receipt attempts exceed bound"));
        }
        let mut selected = None;
        for name in candidates {
            let raw = read_private_in_store(&owner.store, name)?;
            let value = Json::parse(&raw).map_err(|e| io::Error::other(e.to_string()))?;
            let payload = digest(text(&value, "payload")?)?;
            if name != pending_name(request, payload)
                || text(&value, "schema")? != "mesh.native-consumption-owner-commit/v1"
                || digest(text(&value, "request")?)? != request
            {
                return Err(invalid("owner receipt attempt identity differs"));
            }
            let context = Self {
                owner,
                pending: Some((name.to_owned(), raw)),
                request: Some(request),
                operation: None,
            };
            if let Ok((_, Some(proof))) = context.read(owner) {
                if !matches!(proof.pending(),Some((_,r)) if r.kind==DependencyKind::Consumption) {
                    return Err(invalid("pending owner record is not consumption"));
                }
                if selected.is_some() {
                    return Err(invalid("ambiguous owner receipt attempts"));
                }
                selected = Some(context);
            }
        }
        selected.ok_or_else(|| {
            io::Error::other(format!(
                "no exact owner receipt attempt matches history: {ordinary_error}"
            ))
        })
    }
    pub(super) fn for_operation(mut self, operation: RecordDigest) -> Self {
        self.operation = Some(operation);
        self
    }
    pub(super) fn historical_input(
        &self,
        proof: &VerifiedDependencyRead,
        source: (RecordDigest, RecordDigest, RecordDigest),
        destination: (RecordDigest, RecordDigest),
        grant: RecordDigest,
        bindings: (RecordDigest, RecordDigest),
    ) -> bool {
        let Some(request) = self.request else {
            return false;
        };
        let Some(record) = proof.policy().native_request(request) else {
            return false;
        };
        record.kind == DependencyKind::Consumption
            && proof.policy().consumption_facts().iter().any(|fact| {
                Some(fact.start.2) == self.operation
                    && fact.record == record.payload
                    && fact.source == source
                    && fact.grant == grant
                    && (fact.start.0, fact.start.1) == destination
                    && fact.bindings == Some(bindings)
            })
    }
    pub(super) fn pending_roots(
        &self,
        work: &ProvisionedAttachment,
    ) -> io::Result<Option<(String, RecordDigest, RecordDigest)>> {
        if work.id() != self.owner.id() {
            return Ok(None);
        }
        let Some((name, raw)) = &self.pending else {
            return Ok(None);
        };
        let (_, proof) = self.read(work)?;
        let record = proof
            .and_then(|p| p.pending())
            .ok_or_else(|| invalid("owner pending record disappeared"))?
            .1;
        Ok(Some((
            name.clone(),
            super::dependency_transaction::hash(raw.as_bytes()),
            record.payload,
        )))
    }
    pub(super) fn read(
        &self,
        work: &ProvisionedAttachment,
    ) -> io::Result<(String, Option<VerifiedDependencyRead>)> {
        if work.id() != self.owner.id() {
            return work
                .project()
                .read_configuration(work.metadata_path(), &work.store);
        }
        if work.store.identity()? != self.owner.store.identity()? {
            return Err(invalid("owner context installation changed"));
        }
        if let Some((name, raw)) = &self.pending {
            if read_private_in_store(&work.store, name)? != *raw {
                return Err(invalid("owner receipt intent changed"));
            }
        }
        work.project().read_decision_configuration(
            work.metadata_path(),
            &work.store,
            self.pending.as_ref().map(|(_, raw)| raw.as_str()),
        )
    }
    pub(super) fn history(
        &self,
        work: &ProvisionedAttachment,
    ) -> io::Result<(String, VerifiedDependencyRead, OpenWorkspace)> {
        let (configuration, proof) = self.read(work)?;
        let proof = proof.ok_or_else(|| invalid("owner context enrollment missing"))?;
        let history = OpenWorkspace::open_attachment_read_history(
            work.metadata_path(),
            work.store.clone(),
            &crate::TrustedReviewers::default(),
            Some(&proof),
        )
        .map_err(|e| io::Error::other(e.to_string()))?;
        super::history::verify_history_binding(&history, &configuration)?;
        Ok((configuration, proof, history))
    }
    pub(super) fn validate(
        &self,
        storage: &AttachmentStorage,
        selected: &PreparedDependencyWork,
        guard: &WorkspaceInitializationGuard,
    ) -> io::Result<NativeDependencyWorkBinding> {
        let (_, proof, history) = self.history(self.owner)?;
        storage.validate_dependency_work_with_history(selected, guard, &proof, &history)
    }
}
