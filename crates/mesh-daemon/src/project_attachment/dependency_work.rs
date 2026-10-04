//! Native work selectors. Allocation ancestry never creates a consumption or publication grant.
use super::{
    dependency_transaction::{digest, hash},
    invalid, AttachmentStorage, ProvisionedAttachment,
};
use crate::{ipc::Json, workspace::workspace_installation};
use mesh_store::RecordDigest;
use std::{collections::BTreeSet, io};
const MAX_DEPTH: usize = 8;

/// Immutable native correlation facts. A consuming transaction must retain its own custody and
/// revalidate associations; this value holds no locks and conveys no continuing access authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeDependencyWorkBinding {
    authority: RecordDigest,
    project: RecordDigest,
    work: RecordDigest,
    installation: RecordDigest,
    pub(super) correlation: RecordDigest,
}
impl NativeDependencyWorkBinding {
    /// Owning dependency authority, selected from native enrollment.
    pub fn authority(&self) -> RecordDigest {
        self.authority
    }
    /// Owning native project, distinct from optional provider/run identities.
    pub fn project(&self) -> RecordDigest {
        self.project
    }
    /// Stable root or allocated work identity, independent of subsequent editor/run activity.
    pub fn work(&self) -> RecordDigest {
        self.work
    }
    /// Exact current native history installation; replacement cannot reuse this binding.
    pub fn installation(&self) -> RecordDigest {
        self.installation
    }
    /// Recheck every native association. Success is a point-in-time fact, not a grant or held lock.
    pub fn revalidate(
        &self,
        storage: &AttachmentStorage,
        owner: &ProvisionedAttachment,
        candidate: &ProvisionedAttachment,
    ) -> io::Result<()> {
        if storage.dependency_work_binding(owner, candidate)? != *self {
            return Err(invalid("native work binding changed"));
        }
        Ok(())
    }
}
pub(super) struct PreparedDependencyWork {
    owner: ProvisionedAttachment,
    chain: Vec<Link>,
    pub(super) roots: Vec<crate::root_authority::PinnedWorkspaceRoot>,
}
impl PreparedDependencyWork {
    // Only this allocation copied bytes. Parent allocation identity alone is not consumption.
    pub(super) fn has_legacy_copied_origin(&self) -> bool {
        self.chain
            .first()
            .and_then(|link| link.origin.as_ref())
            .is_some_and(|origin| !super::dependency_reservation::is_reservation(&origin.value))
    }

    pub(super) fn require_child_capacity(&self) -> io::Result<()> {
        if self.chain.len() > MAX_DEPTH {
            return Err(invalid("reserved child would exceed native ancestry depth"));
        }
        Ok(())
    }
}

struct Link {
    work: ProvisionedAttachment,
    origin: Option<super::lanes::NativeLaneOrigin>,
}
fn field<'a>(value: &'a Json, key: &str) -> io::Result<&'a str> {
    value
        .get(key)
        .and_then(Json::as_text)
        .ok_or_else(|| invalid("missing native lane ancestry"))
}
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
fn identity(value: (u64, u64)) -> Json {
    Json::text(format!("{:016x}:{:016x}", value.0, value.1))
}

impl AttachmentStorage {
    fn exact_registered_work(
        &self,
        supplied: &ProvisionedAttachment,
    ) -> io::Result<ProvisionedAttachment> {
        supplied.project().ensure_current()?;
        supplied.store.ensure_namespace_identity()?;
        let registered = self.reopen(supplied.id())?;
        if registered.store.identity()? != supplied.store.identity()?
            || registered.project().receipt()? != supplied.project().receipt()?
        {
            return Err(invalid(
                "work is not this catalog's exact native registration",
            ));
        }
        Ok(registered)
    }

    /// Select the enrolled root or a bounded native descendant without a provider or agent run.
    /// All ancestry and source-version facts are rechecked under one deterministic custody set.
    /// Historical allocation is not retroactively granted, and unrelated work is not adopted.
    pub fn dependency_work_binding(
        &self,
        owner: &ProvisionedAttachment,
        candidate: &ProvisionedAttachment,
    ) -> io::Result<NativeDependencyWorkBinding> {
        let prepared = self.prepare_dependency_work(owner, candidate)?;
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&prepared.roots)
            .map_err(error)?;
        self.validate_dependency_work(&prepared, &guard)
    }

    pub(super) fn prepare_dependency_work(
        &self,
        owner: &ProvisionedAttachment,
        candidate: &ProvisionedAttachment,
    ) -> io::Result<PreparedDependencyWork> {
        self.pinned.ensure_namespace_identity()?;
        let owner = self.exact_registered_work(owner)?;
        if self.lane_origin(&owner)?.is_some() {
            return Err(invalid(
                "a descendant cannot discard its native ancestry by becoming a root authority",
            ));
        }
        let mut current = self.exact_registered_work(candidate)?;
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        loop {
            if chain.len() > MAX_DEPTH || !seen.insert(current.id().to_owned()) {
                return Err(invalid("native work ancestry exceeds its bound or cycles"));
            }
            if current.id() == owner.id() {
                if current.store.identity()? != owner.store.identity()? {
                    return Err(invalid("owning installation changed"));
                }
                chain.push(Link {
                    work: current,
                    origin: None,
                });
                break;
            }
            let origin = self
                .lane_origin_bound(&current)?
                .ok_or_else(|| invalid("work is not a native descendant of the owning project"))?;
            let parent = self.reopen(field(&origin.value, "source_project")?)?;
            chain.push(Link {
                work: current,
                origin: Some(origin),
            });
            current = parent;
        }
        // Nine work nodes, eight allocation containers and catalog: at most 27 roots, below 32.
        let mut roots = vec![self.pinned.clone()];
        for link in &chain {
            roots.push(link.work.store.clone());
            roots.push(link.work.attachment.pinned.clone());
            if let Some(origin) = &link.origin {
                roots.push(origin.allocation.clone());
            }
        }
        Ok(PreparedDependencyWork {
            owner,
            chain,
            roots,
        })
    }

    pub(super) fn validate_dependency_work(
        &self,
        prepared: &PreparedDependencyWork,
        guard: &crate::workspace_custody::WorkspaceInitializationGuard,
    ) -> io::Result<NativeDependencyWorkBinding> {
        guard.require_roots(&prepared.roots).map_err(error)?;
        let owner = &prepared.owner;
        self.pinned.ensure_namespace_identity()?;
        if self.lane_origin(owner)?.is_some() {
            return Err(invalid("owning root ancestry changed"));
        }
        let (configuration, proof) = owner
            .project()
            .read_configuration(owner.metadata_path(), &owner.store)?;
        let proof = proof.ok_or_else(|| invalid("owning dependency enrollment is required"))?;
        let workspace = crate::workspace::OpenWorkspace::open_attachment_read_history(
            owner.metadata_path(),
            owner.store.clone(),
            &crate::TrustedReviewers::default(),
            Some(&proof),
        )
        .map_err(error)?;
        super::history::verify_history_binding(&workspace, &configuration)?;
        self.validate_dependency_work_with_history(prepared, guard, &proof, &workspace)
    }

    pub(super) fn validate_dependency_work_with_history(
        &self,
        prepared: &PreparedDependencyWork,
        guard: &crate::workspace_custody::WorkspaceInitializationGuard,
        proof: &super::dependency_read::VerifiedDependencyRead,
        owner_history: &crate::workspace::OpenWorkspace,
    ) -> io::Result<NativeDependencyWorkBinding> {
        self.validate_dependency_work_with_parents(
            prepared,
            guard,
            proof,
            owner_history,
            |parent, version| {
                parent.attachment.inspect_saved(
                    parent.metadata_path(),
                    parent.store.clone(),
                    version,
                    |workspace, version| {
                        workspace
                            .historical_workspace_preview(version)
                            .map(|_| ())
                            .map_err(error)
                    },
                )
            },
        )
    }
    pub(super) fn validate_dependency_work_with_parents(
        &self,
        prepared: &PreparedDependencyWork,
        guard: &crate::workspace_custody::WorkspaceInitializationGuard,
        proof: &super::dependency_read::VerifiedDependencyRead,
        owner_history: &crate::workspace::OpenWorkspace,
        mut inspect_parent: impl FnMut(&ProvisionedAttachment, &str) -> io::Result<()>,
    ) -> io::Result<NativeDependencyWorkBinding> {
        guard.require_roots(&prepared.roots).map_err(error)?;
        let owner = &prepared.owner;
        let chain = &prepared.chain;
        self.pinned.ensure_namespace_identity()?;
        self.exact_registered_work(owner)?;
        if self.lane_origin(owner)?.is_some() {
            return Err(invalid("owning root ancestry changed"));
        }
        let owner_binding = proof.binding();
        if owner_binding.project != hash(owner.project().receipt()?.encode().as_bytes()) {
            return Err(invalid("work owner differs from policy authority"));
        }
        let mut work = owner_binding.project;
        let mut correlation = vec![identity(self.pinned.identity()?)];
        let mut parent: Option<&ProvisionedAttachment> = None;
        for link in chain.iter().rev() {
            let registered = self.exact_registered_work(&link.work)?;
            if let Some(origin) = &link.origin {
                let refreshed = self
                    .lane_origin_bound(&registered)?
                    .ok_or_else(|| invalid("native lane ancestry disappeared"))?;
                if refreshed.value != origin.value
                    || refreshed.allocation.identity()? != origin.allocation.identity()?
                {
                    return Err(invalid("native lane ancestry changed"));
                }
                origin.allocation.ensure_namespace_identity()?;
                let origin = &origin.value;
                if super::dependency_reservation::is_reservation(origin)
                    && (field(origin, "owner")? != owner.id()
                        || field(origin, "authority")? != owner_binding.authority.to_hex()
                        || field(origin, "parent_binding")?
                            != hash(Json::Array(correlation.clone()).encode().as_bytes()).to_hex())
                {
                    return Err(invalid("reserved work belongs to another owning authority"));
                }
                let parent = parent.ok_or_else(|| invalid("missing native parent work"))?;
                if field(origin, "source_project")? != parent.id() {
                    return Err(invalid("native parent work changed"));
                }
                let version = field(origin, "source_version")?;
                if parent.id() == owner.id() {
                    let version = digest(version.strip_prefix("blake3:").unwrap_or(version))?;
                    if !owner_history
                        .workspace_versions()
                        .iter()
                        .any(|v| v.operation() == version)
                    {
                        return Err(invalid(
                            "ancestry input is not a saved operation of its parent",
                        ));
                    }
                    owner_history
                        .historical_workspace_preview(version)
                        .map_err(error)?;
                } else {
                    inspect_parent(parent, version)?;
                }
                work = hash(
                    Json::object([
                        ("schema", Json::text("mesh.native-dependency-work/v1")),
                        ("project", Json::text(owner_binding.project.to_hex())),
                        ("parent_work", Json::text(work.to_hex())),
                        ("request", Json::text(field(origin, "request")?)),
                        ("source_version", Json::text(version)),
                    ])
                    .encode()
                    .as_bytes(),
                );
            }
            correlation.push(Json::object([
                ("registration", Json::text(registered.id())),
                ("store", identity(registered.store.identity()?)),
                ("source", identity(registered.attachment.pinned.identity()?)),
                (
                    "origin",
                    link.origin
                        .as_ref()
                        .map(|origin| origin.value.clone())
                        .unwrap_or(Json::Null),
                ),
                (
                    "allocation",
                    link.origin
                        .as_ref()
                        .map(|origin| origin.allocation.identity().map(identity))
                        .transpose()?
                        .unwrap_or(Json::Null),
                ),
            ]));
            parent = Some(&link.work);
        }
        guard.ensure_current().map_err(error)?;
        let selected = &chain
            .first()
            .ok_or_else(|| invalid("missing selected work"))?
            .work;
        let native = selected.store.identity()?;
        let installation = workspace_installation(native, native);
        Ok(NativeDependencyWorkBinding {
            authority: owner_binding.authority,
            project: owner_binding.project,
            work,
            installation: digest(
                installation
                    .strip_prefix("blake3:")
                    .ok_or_else(|| invalid("invalid native installation"))?,
            )?,
            correlation: hash(Json::Array(correlation).encode().as_bytes()),
        })
    }
}

#[cfg(test)]
mod tests;
