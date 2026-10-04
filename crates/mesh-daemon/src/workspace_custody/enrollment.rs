//! Durable preparation only. This does not enroll policy or grant a policy-aware mutation guard.
use super::*;
use mesh_store::RecordDigest;

const REQUIRED_SCHEMA: &str = "mesh.workspace-agent-custody/v2";

/// Held native preparation fence for one exact dependency authority and installation.
///
/// Generic custody readers (including v1 readers) refuse the required marker. Dropping this
/// thread-bound guard releases the directory lock but never removes the durable fence. This API
/// is not exposed through agent, renderer or CLI operations and grants no publication authority.
pub struct DependencyEnrollmentFence {
    authority: Authority,
    marker: String,
    _lock: CustodyLock,
}
impl DependencyEnrollmentFence {
    /// Install a required marker before a future enrollment journal append, or recover its exact
    /// previous preparation. The caller must obtain installation and authority from native state.
    ///
    /// Assigned workspaces, substituted installations, malformed records and different existing
    /// fences refuse. An interrupted publication may leave the required marker in place; retry
    /// the same identity rather than deleting it. No ordinary-write authority is returned.
    pub fn prepare(
        root: &Path,
        expected_installation: &str,
        dependency_authority: RecordDigest,
    ) -> Result<Self, WorkspaceAgentCustodyError> {
        Self::prepare_with_retry_sync(
            root,
            expected_installation,
            dependency_authority,
            |authority| {
                authority
                    .filesystem
                    .sync_file(Path::new(RECORD_FILE))
                    .and_then(|()| authority.filesystem.sync_dir(Path::new("")))
                    .map_err(|error| {
                        WorkspaceAgentCustodyError::io("sync recovered dependency fence", error)
                    })
            },
        )
    }

    fn prepare_with_retry_sync(
        root: &Path,
        expected_installation: &str,
        dependency_authority: RecordDigest,
        retry_sync: impl FnOnce(&Authority) -> Result<(), WorkspaceAgentCustodyError>,
    ) -> Result<Self, WorkspaceAgentCustodyError> {
        if dependency_authority == RecordDigest::from_bytes([0; 32]) {
            return Err(WorkspaceAgentCustodyError::invalid(
                "dependency authority is missing",
            ));
        }
        let authority = Authority::from_path(root, expected_installation)?;
        let lock = authority.lock()?;
        let marker = Json::object([
            ("schema", Json::text(REQUIRED_SCHEMA)),
            (
                "workspace_installation",
                Json::text(&authority.installation),
            ),
            ("workspace_directory", Json::text(&authority.directory)),
            ("generation", Json::Null),
            (
                "dependency_authority",
                Json::text(dependency_authority.to_hex()),
            ),
        ])
        .encode();
        match read_marker(&authority)? {
            Some(bytes) if bytes == marker.as_bytes() => retry_sync(&authority)?,
            _ => {
                if authority.read()?.is_assigned() {
                    return Err(WorkspaceAgentCustodyError::invalid(
                        "finish assigned work before dependency enrollment",
                    ));
                }
                authority.publish_record(&marker)?;
            }
        }
        let fence = Self {
            authority,
            marker,
            _lock: lock,
        };
        fence.ensure_current()?;
        Ok(fence)
    }

    /// Verify the same pinned namespaces, held custody and exact durable preparation marker.
    pub fn ensure_current(&self) -> Result<(), WorkspaceAgentCustodyError> {
        self.authority
            .physical
            .ensure_namespace_identity()
            .and_then(|()| self.authority.storage.ensure_namespace_identity())
            .map_err(|error| {
                WorkspaceAgentCustodyError::io("verify dependency fence namespace", error)
            })?;
        let identity = self.authority.physical.identity().map_err(|error| {
            WorkspaceAgentCustodyError::io("verify dependency fence identity", error)
        })?;
        if !custody_contains(identity)
            || read_marker(&self.authority)?.as_deref() != Some(self.marker.as_bytes())
        {
            return Err(WorkspaceAgentCustodyError::invalid(
                "dependency enrollment fence changed",
            ));
        }
        Ok(())
    }
}

fn read_marker(authority: &Authority) -> Result<Option<Vec<u8>>, WorkspaceAgentCustodyError> {
    let mut file = match authority.filesystem.read_file(Path::new(RECORD_FILE)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(WorkspaceAgentCustodyError::io(
                "read dependency fence",
                error,
            ))
        }
    };
    let metadata = file
        .metadata()
        .map_err(|error| WorkspaceAgentCustodyError::io("inspect dependency fence", error))?;
    if !metadata.is_file()
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.len() > MAX_RECORD_BYTES
    {
        return Err(WorkspaceAgentCustodyError::invalid(
            "dependency fence is not a bounded owner-only regular file",
        ));
    }
    // Bound the actual read as well as the initial metadata if an uncooperative writer grows it.
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| WorkspaceAgentCustodyError::io("read dependency fence bytes", error))?;
    if bytes.len() > MAX_RECORD_BYTES as usize {
        return Err(WorkspaceAgentCustodyError::invalid(
            "dependency fence exceeds its bound",
        ));
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> (std::path::PathBuf, OpenWorkspace) {
        let root = std::env::temp_dir().join(format!(
            "mesh-enrollment-fence-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let open = OpenWorkspace::open(&root).unwrap();
        (root, open)
    }
    fn id(n: u8) -> RecordDigest {
        RecordDigest::from_bytes([n; 32])
    }
    #[test]
    fn preparation_is_durable_exact_retry_and_never_restores_generic_write_authority() {
        let (root, open) = fixture("retry");
        let install = open.installation();
        let journal = std::fs::read(open.record_file()).unwrap();
        let fence = DependencyEnrollmentFence::prepare(&root, &install, id(1)).unwrap();
        fence.ensure_current().unwrap();
        let marker = open.storage_root().as_path().join(RECORD_FILE);
        let bytes = std::fs::read(&marker).unwrap();
        let expected=format!("{{\"schema\":\"mesh.workspace-agent-custody/v2\",\"workspace_installation\":\"{}\",\"workspace_directory\":\"{}\",\"generation\":null,\"dependency_authority\":\"{}\"}}",install,fence.authority.directory,"01".repeat(32));
        assert_eq!(bytes, expected.as_bytes());
        drop(fence);
        let retry = DependencyEnrollmentFence::prepare(&root, &install, id(1)).unwrap();
        retry.ensure_current().unwrap();
        drop(retry);
        assert_eq!(std::fs::read(&marker).unwrap(), bytes);
        assert!(DependencyEnrollmentFence::prepare(&root, &install, id(2)).is_err());
        let locked = lock_for_workspace_path(&root, &install).unwrap();
        assert!(locked.status().is_err());
        assert!(locked.acquire(false, None).is_err());
        assert!(locked.require_unassigned().is_err());
        assert_eq!(std::fs::read(open.record_file()).unwrap(), journal);
        assert_eq!(std::fs::read(&marker).unwrap(), bytes);
        drop(open);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn assigned_or_wrong_installation_preparation_preserves_previous_authority() {
        let (root, open) = fixture("assigned");
        let install = open.installation();
        assert!(DependencyEnrollmentFence::prepare(&root, "wrong-installation", id(1)).is_err());
        assert!(DependencyEnrollmentFence::prepare(&root, &install, id(0)).is_err());
        let locked = lock_for_workspace_path(&root, &install).unwrap();
        let generation = locked.acquire(false, None).unwrap();
        drop(locked);
        let marker = open.storage_root().as_path().join(RECORD_FILE);
        let before = std::fs::read(&marker).unwrap();
        assert!(DependencyEnrollmentFence::prepare(&root, &install, id(1)).is_err());
        assert_eq!(std::fs::read(&marker).unwrap(), before);
        let locked = lock_for_workspace_path(&root, &install).unwrap();
        assert_eq!(
            locked.status().unwrap().generation(),
            Some(generation.as_str())
        );
        assert!(locked.release(&generation).unwrap());
        drop(locked);
        drop(open);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn interrupted_stage_is_replaced_but_published_fence_is_not_removed() {
        let (root, open) = fixture("interrupted");
        let install = open.installation();
        let temp = open.storage_root().as_path().join(TEMP_FILE);
        std::fs::write(&temp, b"interrupted staging").unwrap();
        let fence = DependencyEnrollmentFence::prepare(&root, &install, id(1)).unwrap();
        assert!(!temp.exists());
        drop(fence);
        let marker = open.storage_root().as_path().join(RECORD_FILE);
        let before = std::fs::read(&marker).unwrap();
        std::fs::set_permissions(&marker, Permissions::from_mode(0o644)).unwrap();
        assert!(DependencyEnrollmentFence::prepare(&root, &install, id(1)).is_err());
        assert_eq!(std::fs::read(&marker).unwrap(), before);
        std::fs::set_permissions(&marker, Permissions::from_mode(0o600)).unwrap();
        drop(DependencyEnrollmentFence::prepare(&root, &install, id(1)).unwrap());
        drop(open);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lost_acknowledgement_retry_must_sync_before_returning_a_guard() {
        let (root, open) = fixture("retry-sync");
        let installation = open.installation();
        drop(DependencyEnrollmentFence::prepare(&root, &installation, id(1)).unwrap());
        let marker = open.storage_root().as_path().join(RECORD_FILE);
        let before = std::fs::read(&marker).unwrap();
        let called = std::cell::Cell::new(false);
        let retry = DependencyEnrollmentFence::prepare_with_retry_sync(
            &root,
            &installation,
            id(1),
            |authority| {
                called.set(true);
                authority
                    .filesystem
                    .sync_file(Path::new(RECORD_FILE))
                    .unwrap();
                Err(WorkspaceAgentCustodyError::io(
                    "injected directory sync refusal",
                    io::Error::other("fault"),
                ))
            },
        );
        assert!(retry.is_err());
        assert!(called.get());
        assert_eq!(std::fs::read(&marker).unwrap(), before);
        assert!(lock_for_workspace_path(&root, &installation)
            .unwrap()
            .require_unassigned()
            .is_err());
        let recovered = DependencyEnrollmentFence::prepare(&root, &installation, id(1)).unwrap();
        recovered.ensure_current().unwrap();
        drop(recovered);
        drop(open);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn held_fence_rejects_replaced_namespace_and_changed_marker() {
        let (root, open) = fixture("replacement");
        let install = open.installation();
        let fence = DependencyEnrollmentFence::prepare(&root, &install, id(1)).unwrap();
        let marker = open.storage_root().as_path().join(RECORD_FILE);
        let original = std::fs::read(&marker).unwrap();
        std::fs::write(&marker, b"changed").unwrap();
        assert!(fence.ensure_current().is_err());
        std::fs::write(&marker, &original).unwrap();
        fence.ensure_current().unwrap();
        let parked = root.with_extension("parked");
        std::fs::rename(&root, &parked).unwrap();
        std::fs::create_dir(&root).unwrap();
        assert!(fence.ensure_current().is_err());
        drop(fence);
        drop(open);
        std::fs::remove_dir(root).unwrap();
        std::fs::remove_dir_all(parked).unwrap();
    }
}
