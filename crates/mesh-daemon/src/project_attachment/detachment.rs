//! Persistent capture opt-out. History and source contents remain intact.
use super::{invalid, read_receipt};
use crate::ipc::Json;
use crate::root_authority::PinnedWorkspaceRoot;
use mesh_cas::DurableFs as _;
use std::io::{self, Read as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

const RECORD: &str = "attachment-detached.json";

fn expected(store: &PinnedWorkspaceRoot) -> io::Result<String> {
    let identity = store.identity()?;
    Ok(Json::object([
        ("schema", Json::text("mesh.attachment-detached/v1")),
        ("receipt", Json::text(read_receipt(store)?)),
        ("device", Json::text(format!("{:016x}", identity.0))),
        ("inode", Json::text(format!("{:016x}", identity.1))),
    ])
    .encode())
}

pub(super) fn detached(store: &PinnedWorkspaceRoot) -> io::Result<bool> {
    store.ensure_namespace_identity()?;
    let file = match store.filesystem().inspect_entry(Path::new(RECORD)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(invalid("invalid detachment record"));
    }
    let mut bytes = Vec::new();
    file.take(131_073).read_to_end(&mut bytes)?;
    if bytes.len() > 131_072 || bytes != expected(store)?.as_bytes() {
        return Err(invalid("detachment record needs reconciliation"));
    }
    store.ensure_namespace_identity()?;
    Ok(true)
}

pub(super) fn ensure_attached(store: &PinnedWorkspaceRoot) -> io::Result<()> {
    if detached(store)? {
        return Err(invalid(
            "project is detached; explicitly reattach before saving",
        ));
    }
    Ok(())
}

pub(super) fn set_detached(store: &PinnedWorkspaceRoot, value: bool) -> io::Result<()> {
    let _guard = crate::workspace_custody::lock_workspace_initialization(store)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let existing = detached(store)?;
    if existing != value {
        if value {
            store.filesystem().write_new_file(
                Path::new(RECORD),
                expected(store)?.as_bytes(),
                std::fs::Permissions::from_mode(0o600),
            )?;
        } else {
            store.filesystem().remove_file(Path::new(RECORD))?;
        }
    }
    store.filesystem().sync_dir(Path::new(""))?;
    store.ensure_namespace_identity()?;
    if detached(store)? != value {
        return Err(invalid("detachment update was not retained"));
    }
    Ok(())
}
