//! Exact retained-file restart evidence, not permission to access or install a consumed input.
use super::*;
use crate::ipc::Json;

impl RetainedAddition {
    /// The native transaction must authenticate and durably retain this receipt before installing.
    /// Content stays in its retained store; the receipt binds its digest and exact allocation.
    pub(crate) fn recovery_receipt(&self) -> io::Result<String> {
        self.source.ensure_namespace_identity()?;
        self.recovery.ensure_namespace_identity()?;
        let identity = |root: &PinnedWorkspaceRoot| -> io::Result<String> {
            let (device, inode) = root.identity()?;
            Ok(ManagedDirectoryIdentity { device, inode }.token())
        };
        let path = self
            .relative
            .to_str()
            .ok_or_else(|| io::Error::other("file path is not UTF-8"))?;
        Ok(Json::object([
            ("schema", Json::text("mesh.retained-file-addition/v1")),
            ("source", Json::text(identity(&self.source)?)),
            ("recovery", Json::text(identity(&self.recovery)?)),
            ("path", Json::text(path)),
            ("parent", Json::text(&self.parent)),
            ("parent_metadata", Json::text(&self.parent_metadata)),
            ("parent_mode", Json::Number(u64::from(self.parent_mode))),
            ("installation", Json::text(self.installed.token())),
            ("mode", Json::Number(u64::from(self.mode))),
            ("metadata", Json::text(&self.metadata)),
            ("bytes", Json::Number(self.bytes.len() as u64)),
            (
                "digest",
                Json::text(mesh_types::Blake3::digest_bytes(&self.bytes).to_string()),
            ),
        ])
        .encode())
    }

    /// Restore an exact native stage or installed file using independently retained saved bytes.
    /// The caller must authorize the operation, authenticate the receipt and hold native custody.
    pub(crate) fn resume(
        source: PinnedWorkspaceRoot,
        relative: PathBuf,
        recovery: PinnedWorkspaceRoot,
        bytes: Vec<u8>,
        receipt: &str,
        byte_limit: u64,
    ) -> io::Result<Self> {
        if bytes.len() as u64 > byte_limit || receipt.len() > 65_536 {
            return Err(io::Error::other("file recovery exceeds admitted bounds"));
        }
        let json = Json::parse(receipt).map_err(io::Error::other)?;
        let text = |key| -> io::Result<String> {
            json.get(key)
                .and_then(Json::as_text)
                .map(str::to_owned)
                .ok_or_else(|| io::Error::other("invalid file receipt field"))
        };
        let number = |key| -> io::Result<u64> {
            json.get(key)
                .and_then(Json::as_u64)
                .ok_or_else(|| io::Error::other("invalid file receipt number"))
        };
        if number("bytes")? != bytes.len() as u64
            || text("digest")? != mesh_types::Blake3::digest_bytes(&bytes).to_string()
        {
            return Err(io::Error::other(
                "retained bytes differ from the file receipt",
            ));
        }
        source.ensure_namespace_identity()?;
        recovery.ensure_namespace_identity()?;
        let (_, destination) = source
            .filesystem()
            .inspect_optional_entry_with_parent(&relative)?;
        let (_, staged) = recovery
            .filesystem()
            .inspect_optional_entry_with_parent(Path::new(EXCHANGE))?;
        let selected = match (destination, staged) {
            (Some(file), None) | (None, Some(file)) => file,
            _ => {
                return Err(io::Error::other(
                    "file recovery has missing or conflicting entries",
                ))
            }
        };
        let stat = selected.metadata()?;
        if !stat.is_file() {
            return Err(io::Error::other(
                "file recovery entry is not a regular file",
            ));
        }
        let prepared = Self {
            source,
            relative,
            recovery,
            bytes,
            installed: managed_file_identity(&selected, &stat)?,
            parent: text("parent")?,
            parent_metadata: text("parent_metadata")?,
            parent_mode: u32::try_from(number("parent_mode")?).map_err(io::Error::other)?,
            mode: u32::try_from(number("mode")?).map_err(io::Error::other)?,
            metadata: text("metadata")?,
        };
        if prepared.recovery_receipt()? != receipt {
            return Err(io::Error::other(
                "file receipt is noncanonical or bound to other identities/path",
            ));
        }
        if !prepared.installed()? {
            prepared.validate()?;
        }
        Ok(prepared)
    }

    pub(super) fn installed(&self) -> io::Result<bool> {
        self.source.ensure_namespace_identity()?;
        self.recovery.ensure_namespace_identity()?;
        let (_, entry) = self
            .source
            .filesystem()
            .inspect_optional_entry_with_parent(&self.relative)?;
        let Some(entry) = entry else { return Ok(false) };
        if !entry.metadata()?.is_file()
            || absent_parent(&self.recovery, Path::new(EXCHANGE))?.is_none()
        {
            return Err(io::Error::other(
                "file installation conflicts with its private stage",
            ));
        }
        let check_parent = || -> io::Result<()> {
            let (policy, mode, identity) = parent_policy(&self.source, &self.relative)?;
            if policy != self.parent_metadata || mode != self.parent_mode || identity != self.parent
            {
                return Err(io::Error::other("file installation parent changed"));
            }
            Ok(())
        };
        check_parent()?;
        let current = observe_file(&self.source, &self.relative, self.bytes.len() as u64)?;
        if current.parent != self.parent
            || current.installation != self.installed.token()
            || current.mode != self.mode
            || current.metadata != self.metadata
            || current.bytes != self.bytes.len() as u64
            || current.digest != mesh_types::Blake3::digest_bytes(&self.bytes).to_string()
        {
            return Err(io::Error::other(
                "installed file differs from retained identity or bytes",
            ));
        }
        check_parent()?;
        self.source.ensure_namespace_identity()?;
        self.recovery.ensure_namespace_identity()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
