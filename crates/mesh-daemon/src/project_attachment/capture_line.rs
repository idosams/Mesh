//! Native observation position and pending exact append, independent of candidate branches.
use super::invalid;
use crate::ipc::Json;
use crate::root_authority::PinnedWorkspaceRoot;
use crate::workspace::OpenWorkspace;
use mesh_cas::DurableFs as _;
use mesh_store::{RecordDigest, TailResidue};
use mesh_types::{Blake3, ContentDigest as _};
use std::fs;
use std::io::{self, Read as _};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;

const RECORD: &str = "attachment-capture-line.json";
const TEMP: &str = "attachment-capture-line.pending";
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CaptureLine {
    pub head: Option<RecordDigest>,
    pending: Option<RecordDigest>,
}
fn error(value: impl std::fmt::Display) -> io::Error {
    io::Error::other(value.to_string())
}
fn digest(value: &Json) -> io::Result<Option<RecordDigest>> {
    match value {
        Json::Null => Ok(None),
        Json::Text(value) => {
            let id = RecordDigest::parse_hex(value).map_err(error)?;
            if id.to_string() != *value {
                return Err(invalid("noncanonical capture identity"));
            }
            Ok(Some(id))
        }
        _ => Err(invalid("invalid capture identity")),
    }
}
fn read(store: &PinnedWorkspaceRoot, name: &str) -> io::Result<Option<String>> {
    let file = match store.filesystem().inspect_entry(Path::new(name)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() > 4096
    {
        return Err(invalid("capture position is not a bounded private record"));
    }
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(invalid("capture position exceeds limit"));
    }
    String::from_utf8(bytes).map(Some).map_err(error)
}
impl CaptureLine {
    fn encoded(&self, configuration: &str) -> String {
        Json::object([
            ("schema", Json::text("mesh.attachment-capture-line/v1")),
            (
                "history",
                Json::text(Blake3::digest_bytes(configuration.as_bytes()).to_string()),
            ),
            (
                "head",
                self.head
                    .map_or(Json::Null, |id| Json::text(id.to_string())),
            ),
            (
                "pending",
                self.pending
                    .map_or(Json::Null, |id| Json::text(id.to_string())),
            ),
        ])
        .encode()
    }
    fn parse(bytes: &str, configuration: &str) -> io::Result<Self> {
        let value = Json::parse(bytes).map_err(error)?;
        let state = Self {
            head: digest(
                value
                    .get("head")
                    .ok_or_else(|| invalid("missing capture head"))?,
            )?,
            pending: digest(
                value
                    .get("pending")
                    .ok_or_else(|| invalid("missing pending capture"))?,
            )?,
        };
        if state.encoded(configuration) != bytes
            || state.pending.is_some() && state.pending == state.head
        {
            return Err(invalid("capture position binding or schema changed"));
        }
        Ok(state)
    }
    fn follows(&self, previous: &Self) -> bool {
        self == previous
            || (previous.pending.is_none() && self.head == previous.head && self.pending.is_some())
            || (previous.pending.is_some()
                && self.pending.is_none()
                && (self.head == previous.head || self.head == previous.pending))
    }
    pub fn exists(store: &PinnedWorkspaceRoot) -> io::Result<bool> {
        Ok(read(store, RECORD)?.is_some() || read(store, TEMP)?.is_some())
    }
    pub fn load(
        store: &PinnedWorkspaceRoot,
        workspace: &OpenWorkspace,
        configuration: &str,
    ) -> io::Result<Self> {
        store.ensure_namespace_identity()?;
        let current = read(store, RECORD)?
            .map(|bytes| Self::parse(&bytes, configuration))
            .transpose()?;
        let temporary = read(store, TEMP)?
            .map(|bytes| Self::parse(&bytes, configuration))
            .transpose()?;
        if let (Some(current), Some(temporary)) = (&current, &temporary) {
            if !temporary.follows(current) {
                return Err(invalid("capture position transition needs reconciliation"));
            }
        }
        if current.is_none() {
            if let Some(temporary) = &temporary {
                if temporary.pending.is_some()
                    || workspace
                        .linear_history(temporary.head)
                        .map_err(error)?
                        .len()
                        != workspace.operations()
                {
                    return Err(invalid("initial capture position needs reconciliation"));
                }
            }
        }
        let mut state = match temporary.or(current) {
            Some(state) => state,
            None => {
                // This migration is only for the pre-import capture-only history format.
                let versions = workspace.workspace_versions();
                let head = versions.last().map(|version| version.operation());
                if workspace.linear_history(head).map_err(error)?.len() != workspace.operations() {
                    return Err(invalid(
                        "legacy capture history is not a complete linear history",
                    ));
                }
                Self {
                    head,
                    pending: None,
                }
            }
        };
        workspace.linear_history(state.head).map_err(error)?;
        if let Some(pending) = state.pending {
            if workspace.has_operation(&pending) {
                let chain = workspace.linear_history(Some(pending)).map_err(error)?;
                if chain.iter().rev().nth(1).copied() != state.head {
                    return Err(invalid("pending capture has a different predecessor"));
                }
                state.head = Some(pending);
            } else if workspace.tail() != TailResidue::Whole {
                return Err(invalid("pending capture journal needs reconciliation"));
            }
            // An absent operation in a whole recovered journal was never durably captured.
            state.pending = None;
        }
        store.ensure_namespace_identity()?;
        Ok(state)
    }
    pub fn persist(&self, store: &PinnedWorkspaceRoot, configuration: &str) -> io::Result<()> {
        store.ensure_namespace_identity()?;
        let filesystem = store.filesystem();
        let mut current = read(store, RECORD)?
            .map(|bytes| Self::parse(&bytes, configuration))
            .transpose()?;
        if let Some(bytes) = read(store, TEMP)? {
            let temporary = Self::parse(&bytes, configuration)?;
            if current
                .as_ref()
                .is_some_and(|current| !temporary.follows(current))
            {
                return Err(invalid("interrupted capture position needs reconciliation"));
            }
            filesystem.rename(Path::new(TEMP), Path::new(RECORD))?;
            store.sync()?;
            current = Some(temporary);
        }
        if current.as_ref() == Some(self) {
            return store.ensure_namespace_identity();
        }
        if current
            .as_ref()
            .is_some_and(|current| !self.follows(current))
        {
            return Err(invalid("capture position cannot skip an append"));
        }
        filesystem.write_new_file(
            Path::new(TEMP),
            self.encoded(configuration).as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        store.ensure_namespace_identity()?;
        filesystem.rename(Path::new(TEMP), Path::new(RECORD))?;
        store.sync()?;
        store.ensure_namespace_identity()?;
        if read(store, RECORD)?.as_deref() != Some(&self.encoded(configuration)) {
            return Err(invalid("capture position changed before acknowledgement"));
        }
        Ok(())
    }
    pub fn begin(
        &self,
        operation: RecordDigest,
        store: &PinnedWorkspaceRoot,
        configuration: &str,
    ) -> io::Result<()> {
        Self {
            head: self.head,
            pending: Some(operation),
        }
        .persist(store, configuration)
    }
    pub fn finish(
        &self,
        operation: RecordDigest,
        store: &PinnedWorkspaceRoot,
        configuration: &str,
    ) -> io::Result<()> {
        Self {
            head: Some(operation),
            pending: None,
        }
        .persist(store, configuration)
    }
}
