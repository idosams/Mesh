//! Immutable native creation inputs, retained before allocation or transport. Never launch authority.
use super::{invalid, AttachmentStorage};
use crate::ipc::Json;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::{
    fs,
    io::{self, Read as _},
    path::Path,
};
const PREFIX: &str = "remote-start-";
const MAX_REQUESTS: usize = 64;
const MAX_BYTES: u64 = 32_768;

/// Private original creation inputs. A consumer must re-admit paths, identities, saved input,
/// provider policy and fixed lease before acting. Reading this record never retries a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteStartRequest {
    /// Native-generated 32-character request, shared with the fleet catalogue's idempotency key.
    pub request: String,
    /// Exact native configuration and bindings. Never expose this private value to a renderer.
    pub value: Json,
}
impl RemoteStartRequest {
    fn validate(&self) -> io::Result<()> {
        if self.request.len() != 32
            || !self
                .request
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !matches!(&self.value, Json::Object(_))
            || self.value.encode().len() > 24_576
        {
            return Err(invalid("invalid native remote creation request"));
        }
        Ok(())
    }
    fn name(&self) -> String {
        format!("{PREFIX}{}.json", self.request)
    }
}
impl AttachmentStorage {
    /// Retain exact native inputs before creating a fleet or contacting a worker. The original
    /// record is immutable: a changed peer, deadline or input under the same request refuses.
    /// Partial writes and exhausted capacity are preserved, never evicted or automatically retried.
    pub fn retain_remote_start_request(&self, request: &RemoteStartRequest) -> io::Result<()> {
        request.validate()?;
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|e| io::Error::other(e.to_string()))?;
        let previous = self.read_remote_starts()?;
        if let Some(old) = previous.iter().find(|old| old.request == request.request) {
            return if old == request {
                Ok(())
            } else {
                Err(invalid("remote creation inputs changed"))
            };
        }
        if previous.len() >= MAX_REQUESTS {
            return Err(invalid("remote creation request capacity reached"));
        }
        self.pinned.filesystem().write_new_file(
            Path::new(&request.name()),
            self.remote_start_record(request)?.as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        self.pinned.sync()?;
        if !self.read_remote_starts()?.contains(request) {
            return Err(invalid("remote creation receipt changed"));
        }
        Ok(())
    }
    /// Read bounded original requests without opening their referenced paths or taking any action.
    /// These are pending inputs, not evidence that allocation, transfer or execution completed.
    pub fn remote_start_requests(&self) -> io::Result<Vec<RemoteStartRequest>> {
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|e| io::Error::other(e.to_string()))?;
        self.read_remote_starts()
    }
    fn remote_start_record(&self, request: &RemoteStartRequest) -> io::Result<String> {
        let (device, inode) = self.pinned.identity()?;
        Ok(Json::object([
            ("schema", Json::text("mesh.native-remote-start-request/v1")),
            ("catalog_device", Json::text(format!("{device:016x}"))),
            ("catalog_inode", Json::text(format!("{inode:016x}"))),
            ("request", Json::text(&request.request)),
            ("value", request.value.clone()),
        ])
        .encode())
    }
    fn read_remote_starts(&self) -> io::Result<Vec<RemoteStartRequest>> {
        self.pinned.ensure_namespace_identity()?;
        // Same whole-catalogue bound as registrations; unrelated entries are never adopted.
        let names = self
            .pinned
            .filesystem()
            .read_directory_names_bounded(Path::new(""), 256)?;
        let mut entries = Vec::new();
        for name in names {
            let name = name
                .to_str()
                .ok_or_else(|| invalid("invalid catalogue name"))?;
            if !name.starts_with(PREFIX) {
                continue;
            }
            if entries.len() >= MAX_REQUESTS {
                return Err(invalid("too many remote creation requests"));
            }
            let file = self.pinned.filesystem().inspect_entry(Path::new(name))?;
            let meta = file.metadata()?;
            if !meta.is_file()
                || meta.nlink() != 1
                || meta.mode() & 0o077 != 0
                || meta.len() > MAX_BYTES
                || meta.uid() != fs::symlink_metadata(&self.path)?.uid()
            {
                return Err(invalid(
                    "remote creation request is not a bounded private file",
                ));
            }
            let mut bytes = Vec::new();
            file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_BYTES {
                return Err(invalid("remote creation request exceeds limit"));
            }
            let encoded =
                std::str::from_utf8(&bytes).map_err(|_| invalid("invalid request encoding"))?;
            let value = Json::parse(encoded).map_err(|_| invalid("invalid creation request"))?;
            let request = RemoteStartRequest {
                request: value
                    .get("request")
                    .and_then(Json::as_text)
                    .ok_or_else(|| invalid("missing creation request"))?
                    .into(),
                value: value
                    .get("value")
                    .ok_or_else(|| invalid("missing creation inputs"))?
                    .clone(),
            };
            request.validate()?;
            if request.name() != name || self.remote_start_record(&request)? != encoded {
                return Err(invalid("creation request identity or schema changed"));
            }
            entries.push(request);
        }
        entries.sort_by(|a, b| a.request.cmp(&b.request));
        self.pinned.ensure_namespace_identity()?;
        Ok(entries)
    }
}
#[cfg(test)]
mod tests;
