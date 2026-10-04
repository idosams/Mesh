//! Empty native destinations are reserved before grants or materialization, never marked ready.
use super::dependency_enrollment::read_private_in_store;
use super::dependency_transaction::{hash, text};
use super::{
    invalid, AttachmentStorage, ObservationLimits, ProvisionedAttachment, SavedAttachmentVersion,
};
use crate::{ipc::Json, root_authority::PinnedWorkspaceRoot};
use mesh_cas::DurableFs as _;
use mesh_store::RecordDigest;
use std::{ffi::OsStr, fs, io, os::unix::fs::PermissionsExt as _, path::Path};
const SCHEMA: &str = "mesh.attachment-lane-reservation/v1";
const INTENT: &str = "intent.json";
const FILES: &str = "files.json";
const STORE: &str = "store.json";
const RESERVED: &str = "reserved.json";
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
fn identity(root: &PinnedWorkspaceRoot) -> io::Result<Json> {
    let (d, i) = root.identity()?;
    Ok(Json::text(format!("{d:016x}:{i:016x}")))
}
fn child(root: &PinnedWorkspaceRoot, name: &str) -> io::Result<PinnedWorkspaceRoot> {
    let result = match root.create_child_directory(OsStr::new(name)) {
        Ok(value) => value,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            root.open_child_directory(OsStr::new(name))?
        }
        Err(e) => return Err(e),
    };
    if result
        .try_clone_directory()?
        .metadata()?
        .permissions()
        .mode()
        & 0o077
        != 0
    {
        return Err(invalid("reservation storage must be private"));
    }
    Ok(result)
}
fn write(root: &PinnedWorkspaceRoot, name: &str, value: &Json) -> io::Result<()> {
    root.filesystem().write_new_file(
        Path::new(name),
        value.encode().as_bytes(),
        fs::Permissions::from_mode(0o600),
    )?;
    root.sync()
}
fn read(root: &PinnedWorkspaceRoot, name: &str) -> io::Result<Option<Json>> {
    match read_private_in_store(root, name) {
        Ok(raw) => {
            let parsed = Json::parse(&raw).map_err(error)?;
            if parsed.encode() != raw {
                return Err(invalid("noncanonical reservation receipt"));
            }
            Ok(Some(parsed))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
fn exact(root: &PinnedWorkspaceRoot, name: &str, expected: &Json) -> io::Result<()> {
    if read(root, name)?.as_ref() != Some(expected) {
        return Err(invalid("reservation identity changed or incomplete"));
    }
    Ok(())
}
fn store_receipt(work: &ProvisionedAttachment) -> io::Result<Json> {
    Ok(Json::object([
        ("project", Json::text(work.id())),
        ("store", identity(&work.store)?),
        ("attachment", work.project().receipt()?),
    ]))
}
fn verify_input(parent: &ProvisionedAttachment, version: SavedAttachmentVersion) -> io::Result<()> {
    parent.project().inspect_saved(
        parent.metadata_path(),
        parent.store.clone(),
        &version.operation().to_string(),
        |history, op| {
            history
                .historical_workspace_preview(op)
                .map(|_| ())
                .map_err(error)
        },
    )
}
fn verify_enrolled(work: &ProvisionedAttachment) -> io::Result<()> {
    let _guard =
        crate::workspace_custody::lock_workspace_initialization(&work.store).map_err(error)?;
    let (_, proof) = work
        .project()
        .read_configuration(work.metadata_path(), &work.store)?;
    if proof.is_none() {
        return Err(invalid("reserved history has no native enrollment"));
    }
    work.saved_versions()?;
    Ok(())
}

fn reserved(work: &ProvisionedAttachment, intent: &Json) -> io::Result<Json> {
    Ok(Json::object([
        ("schema", Json::text("mesh.native-reserved-destination/v1")),
        (
            "intent",
            Json::text(hash(intent.encode().as_bytes()).to_hex()),
        ),
        ("binding", store_receipt(work)?),
    ]))
}

impl AttachmentStorage {
    /// Reserve an empty, fenced native destination. This copies no input, issues no grant, records
    /// no consumption and does not make a lane runnable. Exact retries never allocate a second work.
    pub fn reserve_dependency_lane(
        &self,
        owner: &ProvisionedAttachment,
        parent: &ProvisionedAttachment,
        version: SavedAttachmentVersion,
        request: RecordDigest,
    ) -> io::Result<ProvisionedAttachment> {
        self.reserve_with_hook(owner, parent, version, request, |_| Ok(()))
    }
    fn reserve_with_hook(
        &self,
        owner: &ProvisionedAttachment,
        parent: &ProvisionedAttachment,
        version: SavedAttachmentVersion,
        request: RecordDigest,
        mut hook: impl FnMut(&str) -> io::Result<()>,
    ) -> io::Result<ProvisionedAttachment> {
        if request == RecordDigest::from_bytes([0; 32]) {
            return Err(invalid("missing reservation request"));
        }
        let prepared = self.prepare_dependency_work(owner, parent)?;
        prepared.require_child_capacity()?;
        if prepared.roots.len() + 5 > 32 {
            return Err(invalid("reservation ancestry exceeds custody bound"));
        }
        let binding = self.dependency_work_binding(owner, parent)?;
        parent.project().inspect_saved(
            parent.metadata_path(),
            parent.store.clone(),
            &version.operation().to_string(),
            |history, op| {
                history
                    .historical_workspace_preview(op)
                    .map(|_| ())
                    .map_err(error)
            },
        )?;
        let name = format!(
            "reservation-{}",
            hash(format!("{}:{}", owner.id(), request.to_hex()).as_bytes()).to_hex()
        );
        let root_path = self.path.canonicalize()?;
        let (allocation, intent, fresh) = {
            let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
                .map_err(error)?;
            self.pinned.ensure_namespace_identity()?;
            let lanes = child(&self.pinned, "work-lanes")?;
            let (allocation, fresh) = match lanes.create_child_directory(OsStr::new(&name)) {
                Ok(value) => (value, true),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    (lanes.open_child_directory(OsStr::new(&name))?, false)
                }
                Err(e) => return Err(e),
            };
            let intent = Json::object([
                ("schema", Json::text(SCHEMA)),
                ("owner", Json::text(owner.id())),
                ("authority", Json::text(binding.authority().to_hex())),
                ("parent_binding", Json::text(binding.correlation.to_hex())),
                ("source_project", Json::text(parent.id())),
                ("source_version", Json::text(version.operation().to_hex())),
                ("request", Json::text(request.to_hex())),
                ("allocation", identity(&allocation)?),
                ("catalog", identity(&self.pinned)?),
            ]);
            if fresh {
                write(&allocation, INTENT, &intent)?;
            } else {
                exact(&allocation, INTENT, &intent)?;
            }
            (allocation, intent, fresh)
        };
        let files = if fresh {
            let files = allocation.create_child_directory(OsStr::new("files"))?;
            write(&allocation, FILES, &identity(&files)?)?;
            files
        } else {
            let files = allocation.open_child_directory(OsStr::new("files"))?;
            exact(&allocation, FILES, &identity(&files)?)?;
            files
        };
        let path = root_path.join("work-lanes").join(&name).join("files");
        if let Some(record) = read(&allocation, RESERVED)? {
            let recorded = record
                .get("binding")
                .ok_or_else(|| invalid("missing reserved binding"))?;
            let work = self.reopen(text(recorded, "project")?)?;
            exact(&allocation, STORE, &store_receipt(&work)?)?;
            if record != reserved(&work, &intent)? {
                return Err(invalid("reserved destination changed"));
            }
            let selected = self.prepare_dependency_work(owner, &work)?;
            let mut roots = prepared.roots.clone();
            roots.extend([allocation.clone(), files.clone(), work.store.clone()]);
            let guard = crate::workspace_custody::lock_workspace_initialization_set(&roots)
                .map_err(error)?;
            if self.validate_dependency_work(&prepared, &guard)? != binding {
                return Err(invalid("reservation parent changed during retry"));
            }
            self.validate_dependency_work(&selected, &guard)?;
            verify_input(parent, version)?;
            verify_enrolled(&work)?;
            allocation.filesystem().sync_file(Path::new(RESERVED))?;
            allocation.sync()?;
            self.pinned.sync()?;
            guard.ensure_current().map_err(error)?;
            return Ok(work);
        }
        let staging = child(&allocation, "staging")?;
        let stage_path = root_path.join("work-lanes").join(&name).join("staging");
        let stage_storage = AttachmentStorage::open(&stage_path)?;
        let admitted_files = super::ProjectAttachment::admit(&path)?;
        if stage_storage.pinned.identity()? != staging.identity()?
            || admitted_files.pinned.identity()? != files.identity()?
        {
            return Err(invalid("reservation staging namespace changed"));
        }
        let recorded = read(&allocation, STORE)?;
        let work = if let Some(recorded) = &recorded {
            let id = text(recorded, "project")?;
            match self.reopen(id) {
                Ok(work) => {
                    if store_receipt(&work)? != *recorded {
                        return Err(invalid("published reservation store changed"));
                    }
                    work
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => stage_storage.reopen(id)?,
                Err(e) => return Err(e),
            }
        } else {
            // A stage without its exact native receipt is ambiguous recovery evidence.
            if !staging
                .filesystem()
                .read_directory_names_bounded(Path::new(""), 1)?
                .is_empty()
            {
                return Err(invalid("unbound reservation stage requires reconciliation"));
            }
            let work = stage_storage.provision(&path)?;
            write(&allocation, STORE, &store_receipt(&work)?)?;
            work
        };
        exact(&allocation, STORE, &store_receipt(&work)?)?;
        if work.project().root() != path
            || work.attachment.pinned.identity()? != files.identity()?
        {
            return Err(invalid("reserved files binding changed"));
        }
        let mut roots = prepared.roots.clone();
        roots.extend([
            allocation.clone(),
            files.clone(),
            staging.clone(),
            work.store.clone(),
        ]);
        {
            let guard = crate::workspace_custody::lock_workspace_initialization_set(&roots)
                .map_err(error)?;
            if self.validate_dependency_work(&prepared, &guard)? != binding {
                return Err(invalid("reservation parent changed"));
            }
            verify_input(parent, version)?;
            exact(&allocation, INTENT, &intent)?;
            exact(&allocation, FILES, &identity(&files)?)?;
            exact(&allocation, STORE, &store_receipt(&work)?)?;
            if !files
                .filesystem()
                .read_directory_names_bounded(Path::new(""), 1)?
                .is_empty()
            {
                return Err(invalid(
                    "unfinished reserved destination contains user work",
                ));
            }
            let history = read(&work.store, super::history::HISTORY)?;
            if history
                .as_ref()
                .and_then(|h| h.get("schema"))
                .and_then(Json::as_text)
                != Some("mesh.attachment-history/v3")
            {
                let input = work
                    .project()
                    .capture_inputs(ObservationLimits::default())?;
                let (_, created) = work
                    .project()
                    .history_configuration(&work.store, Some(input.exclusion_digest()))?;
                let empty = crate::workspace::OpenWorkspace::open_attachment_store(
                    work.metadata_path(),
                    work.store.clone(),
                    created,
                )
                .map_err(error)?;
                if empty.operations() != 0 {
                    return Err(invalid("reservation history is not empty"));
                }
                drop(empty);
            }
            hook("initialized")?;
            work.enroll_dependency_history()?;
            if !work.saved_versions()?.is_empty() {
                return Err(invalid("reservation already contains private progress"));
            }
            hook("fenced")?;
            guard.ensure_current().map_err(error)?;
            if work.metadata_path().parent() == Some(stage_path.as_path()) {
                let entry = format!("project-{}", work.id());
                staging.publish_child_directory(
                    OsStr::new(&entry),
                    &work.store,
                    &self.pinned,
                    OsStr::new(&entry),
                )?;
                // The staged namespace has deliberately moved. Release it before reopening the
                // same inode from its published name; old writers now see the required fence.
            }
        }
        hook("published")?;
        let published = self.reopen(work.id())?;
        exact(&allocation, STORE, &store_receipt(&published)?)?;
        let mut roots = prepared.roots.clone();
        roots.extend([allocation.clone(), files, staging, published.store.clone()]);
        let guard =
            crate::workspace_custody::lock_workspace_initialization_set(&roots).map_err(error)?;
        if self.validate_dependency_work(&prepared, &guard)? != binding {
            return Err(invalid("reservation parent changed after publication"));
        }
        if !published
            .attachment
            .pinned
            .filesystem()
            .read_directory_names_bounded(Path::new(""), 1)?
            .is_empty()
        {
            return Err(invalid("unacknowledged reservation contains user work"));
        }
        if !published.saved_versions()?.is_empty() {
            return Err(invalid("unacknowledged reservation history changed"));
        }
        exact(&allocation, INTENT, &intent)?;
        verify_input(parent, version)?;
        verify_enrolled(&published)?;
        write(&allocation, RESERVED, &reserved(&published, &intent)?)?;
        hook("recorded")?;
        guard.ensure_current().map_err(error)?;
        Ok(published)
    }
}

pub(super) fn origin(
    storage: &AttachmentStorage,
    work: &ProvisionedAttachment,
    allocation: PinnedWorkspaceRoot,
    intent: Json,
) -> io::Result<super::lanes::NativeLaneOrigin> {
    if intent.get("schema").and_then(Json::as_text) != Some(SCHEMA)
        || intent.get("catalog") != Some(&identity(&storage.pinned)?)
        || intent.get("allocation") != Some(&identity(&allocation)?)
    {
        return Err(invalid("reserved native ancestry changed"));
    }
    for field in [
        "owner",
        "source_project",
        "source_version",
        "authority",
        "parent_binding",
        "request",
    ] {
        let value = text(&intent, field)?;
        if !super::provisioning::valid_id(value) {
            return Err(invalid("invalid reserved identity"));
        }
    }
    let closed = Json::object([
        ("schema", Json::text(SCHEMA)),
        ("owner", Json::text(text(&intent, "owner")?)),
        ("authority", Json::text(text(&intent, "authority")?)),
        (
            "parent_binding",
            Json::text(text(&intent, "parent_binding")?),
        ),
        (
            "source_project",
            Json::text(text(&intent, "source_project")?),
        ),
        (
            "source_version",
            Json::text(text(&intent, "source_version")?),
        ),
        ("request", Json::text(text(&intent, "request")?)),
        ("allocation", identity(&allocation)?),
        ("catalog", identity(&storage.pinned)?),
    ]);
    if intent != closed {
        return Err(invalid("unknown reservation fields"));
    }
    let request = RecordDigest::parse_hex(text(&intent, "request")?).map_err(error)?;
    if request == RecordDigest::from_bytes([0; 32]) {
        return Err(invalid("missing reserved request"));
    }
    let expected_name = format!(
        "reservation-{}",
        hash(format!("{}:{}", text(&intent, "owner")?, request.to_hex()).as_bytes()).to_hex()
    );
    if work.project().root().parent().and_then(Path::file_name) != Some(OsStr::new(&expected_name))
    {
        return Err(invalid("reserved location changed"));
    }
    exact(&allocation, FILES, &identity(&work.attachment.pinned)?)?;
    exact(&allocation, STORE, &store_receipt(work)?)?;
    exact(&allocation, RESERVED, &reserved(work, &intent)?)?;
    verify_enrolled(work)?; // local history enrollment, not an owning-project grant
    Ok(super::lanes::NativeLaneOrigin {
        value: intent,
        allocation,
    })
}

pub(super) fn is_reservation(value: &Json) -> bool {
    value.get("schema").and_then(Json::as_text) == Some(SCHEMA)
}
#[cfg(test)]
mod tests;
