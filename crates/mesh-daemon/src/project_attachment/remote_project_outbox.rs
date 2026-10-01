//! Pending review inputs, never execution or approval authority. Reads do not dispatch work.
use super::{invalid, AttachmentStorage};
use crate::ipc::Json;
use mesh_cas::DurableFs as _;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read as _};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;

const RECORD: &str = "remote-project-outbox.json";
const STAGING: &str = "remote-project-outbox.pending";
const LIMIT: u64 = 131_072;

/// A bounded set of exact pending inputs, independent of whether a review panel remains open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteProjectOutbox {
    /// Compare-and-swap revision. Zero represents a never-written outbox.
    pub revision: u64,
    /// Closed-schema pending requests; at most eight. Native history must verify their selectors.
    pub entries: Vec<Json>,
}
impl RemoteProjectOutbox {
    /// Renderer projection. No saved content, receipt or authority is inferred from these inputs.
    pub fn to_json(&self) -> Json {
        Json::object([
            ("schema", Json::text("mesh.remote-project-outbox/v1")),
            ("revision", Json::text(self.revision.to_string())),
            ("entries", Json::Array(self.entries.clone())),
        ])
    }
    /// Validate bounded pending inputs before they may be persisted.
    pub fn parse(encoded: &str) -> io::Result<Self> {
        if encoded.len() as u64 > LIMIT {
            return Err(invalid("review outbox exceeds limit"));
        }
        let value = Json::parse(encoded).map_err(|_| invalid("invalid review outbox"))?;
        fields(&value, 3)?;
        if text(&value, "schema")? != "mesh.remote-project-outbox/v1" {
            return Err(invalid("unknown review outbox schema"));
        }
        let raw = text(&value, "revision")?;
        let revision = raw
            .parse::<u64>()
            .map_err(|_| invalid("invalid outbox revision"))?;
        if revision.to_string() != raw {
            return Err(invalid("noncanonical outbox revision"));
        }
        let Some(Json::Array(entries)) = value.get("entries") else {
            return Err(invalid("missing outbox entries"));
        };
        validate(entries)?;
        Ok(Self {
            revision,
            entries: entries.clone(),
        })
    }
}
fn fields(value: &Json, count: usize) -> io::Result<()> {
    if matches!(value, Json::Object(fields) if fields.len() == count) {
        Ok(())
    } else {
        Err(invalid("invalid pending review fields"))
    }
}
fn text<'a>(value: &'a Json, key: &str) -> io::Result<&'a str> {
    value
        .get(key)
        .and_then(Json::as_text)
        .ok_or_else(|| invalid("missing pending review field"))
}
fn hex(value: &str, size: usize) -> bool {
    value.len() == size
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
/// Validate one exact remote project input; this conveys no import or approval authority.
pub fn validate_remote_project_action(value: &Json) -> io::Result<()> {
    fields(value, 8)?;
    if value.encode().len() > 4096
        || text(value, "schema")? != "mesh.desktop-remote-project-request/v1"
    {
        return Err(invalid("invalid remote project request"));
    }
    for name in ["project", "objective"] {
        let id = text(value, name)?;
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(invalid("invalid remote project identity"));
        }
    }
    for name in ["offer", "correlation"] {
        if !hex(text(value, name)?, 64) {
            return Err(invalid("invalid remote project selection"));
        }
    }
    if !hex(text(value, "request")?, 32) {
        return Err(invalid("invalid remote project retry"));
    }
    match value.get("expected_main") {
        Some(Json::Null) => (),
        Some(Json::Text(v)) if hex(v, 64) => (),
        _ => return Err(invalid("invalid remote project main")),
    }
    if !matches!(
        text(value, "action")?,
        "stage" | "inspect_import" | "import" | "inspect_review" | "create_review"
    ) {
        return Err(invalid("invalid remote project action"));
    }
    Ok(())
}
fn operation_token(entry: &Json) -> io::Result<&str> {
    text(entry, "request")
}
fn validate(entries: &[Json]) -> io::Result<()> {
    if entries.len() > 8 {
        return Err(invalid("too many pending remote project actions"));
    }
    let mut tokens = BTreeSet::new();
    for entry in entries {
        validate_remote_project_action(entry)?;
        if !tokens.insert(text(entry, "request")?) {
            return Err(invalid("duplicate remote project action"));
        }
    }
    Ok(())
}
impl AttachmentStorage {
    /// Read pending inputs only. The caller must explicitly choose whether to retry an operation.
    pub fn load_remote_project_outbox(&self) -> io::Result<RemoteProjectOutbox> {
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|e| io::Error::other(e.to_string()))?;
        self.read_remote_project_outbox()
    }
    /// Durably replace pending inputs at an exact revision; no operation is submitted here.
    pub fn save_remote_project_outbox(
        &self,
        expected: u64,
        entries: Vec<Json>,
    ) -> io::Result<RemoteProjectOutbox> {
        validate(&entries)?;
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|e| io::Error::other(e.to_string()))?;
        let current = self.read_remote_project_outbox()?;
        if current.revision != expected {
            return Err(invalid("pending review revision changed"));
        }
        for previous in &current.entries {
            let token = operation_token(previous)?;
            for next in &entries {
                if operation_token(next)? == token
                    && [
                        "project",
                        "objective",
                        "offer",
                        "correlation",
                        "expected_main",
                    ]
                    .iter()
                    .any(|name| next.get(name) != previous.get(name))
                {
                    return Err(invalid(
                        "pending review inputs changed under the same operation",
                    ));
                }
            }
        }
        if current.entries == entries {
            return Ok(current);
        }
        let next = RemoteProjectOutbox {
            revision: expected
                .checked_add(1)
                .ok_or_else(|| invalid("outbox revision exhausted"))?,
            entries,
        };
        let encoded = self.remote_project_outbox_record(&next)?.encode();
        if encoded.len() as u64 > LIMIT {
            return Err(invalid("review outbox exceeds limit"));
        }
        let filesystem = self.pinned.filesystem();
        filesystem.write_new_file(
            Path::new(STAGING),
            encoded.as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        self.pinned.ensure_namespace_identity()?;
        filesystem.rename(Path::new(STAGING), Path::new(RECORD))?;
        self.pinned.sync()?;
        if self.read_remote_project_outbox()? != next {
            return Err(invalid("outbox changed before acknowledgement"));
        }
        Ok(next)
    }
    fn remote_project_outbox_record(&self, state: &RemoteProjectOutbox) -> io::Result<Json> {
        let (device, inode) = self.pinned.identity()?;
        Ok(Json::object([
            ("schema", Json::text("mesh.bound-remote-project-outbox/v1")),
            ("device", Json::text(format!("{device:016x}"))),
            ("inode", Json::text(format!("{inode:016x}"))),
            ("state", state.to_json()),
        ]))
    }
    fn read_remote_project_outbox(&self) -> io::Result<RemoteProjectOutbox> {
        let filesystem = self.pinned.filesystem();
        let file = match filesystem.inspect_entry(Path::new(RECORD)) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                match filesystem.inspect_entry(Path::new(STAGING)) {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        self.pinned.ensure_namespace_identity()?;
                        return Ok(RemoteProjectOutbox {
                            revision: 0,
                            entries: Vec::new(),
                        });
                    }
                    _ => return Err(invalid("unfinished review outbox retained")),
                }
            }
            Err(e) => return Err(e),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.permissions().mode() & 0o077 != 0
            || metadata.len() > LIMIT
        {
            return Err(invalid("outbox is not a bounded private file"));
        }
        let mut raw = String::new();
        file.take(LIMIT + 1).read_to_string(&mut raw)?;
        if raw.len() as u64 > LIMIT {
            return Err(invalid("outbox exceeds limit"));
        }
        let value = Json::parse(&raw).map_err(|_| invalid("invalid outbox record"))?;
        let state = RemoteProjectOutbox::parse(
            &value
                .get("state")
                .ok_or_else(|| invalid("missing outbox state"))?
                .encode(),
        )?;
        if state.revision == 0 || self.remote_project_outbox_record(&state)?.encode() != raw {
            return Err(invalid("outbox identity or schema changed"));
        }
        self.pinned.ensure_namespace_identity()?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(action: &str) -> Json {
        Json::object([
            (
                "schema",
                Json::text("mesh.desktop-remote-project-request/v1"),
            ),
            ("project", Json::text("a".repeat(64))),
            ("objective", Json::text(format!("fleet-{}", "b".repeat(64)))),
            ("offer", Json::text("c".repeat(64))),
            ("correlation", Json::text("d".repeat(64))),
            ("request", Json::text("e".repeat(32))),
            ("expected_main", Json::Null),
            ("action", Json::text(action)),
        ])
    }
    #[test]
    fn remote_project_outbox_retains_exact_inputs_across_restart_and_refuses_stale_or_changed_work()
    {
        let root =
            std::env::temp_dir().join(format!("mesh-remote-project-outbox-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let store = AttachmentStorage::open(&root).unwrap();
        assert_eq!(store.load_remote_project_outbox().unwrap().revision, 0);
        let saved = store
            .save_remote_project_outbox(0, vec![entry("stage")])
            .unwrap();
        assert_eq!(saved.revision, 1);
        drop(store);
        let store = AttachmentStorage::open(&root).unwrap();
        assert_eq!(store.load_remote_project_outbox().unwrap(), saved);
        assert_eq!(
            store
                .save_remote_project_outbox(1, saved.entries.clone())
                .unwrap(),
            saved
        );
        assert!(store.save_remote_project_outbox(0, vec![]).is_err());
        let mut changed = entry("import");
        let Json::Object(fields) = &mut changed else {
            panic!()
        };
        fields
            .iter_mut()
            .find(|(k, _)| k == "expected_main")
            .unwrap()
            .1 = Json::text("f".repeat(64));
        assert!(store.save_remote_project_outbox(1, vec![changed]).is_err());
        assert!(store
            .save_remote_project_outbox(1, vec![entry("stage"), entry("import")])
            .is_err());
        let next = store
            .save_remote_project_outbox(1, vec![entry("import")])
            .unwrap();
        assert_eq!(next.revision, 2);
        let foreign = root.with_extension("foreign");
        fs::create_dir(&foreign).unwrap();
        let foreign_store = AttachmentStorage::open(&foreign).unwrap();
        fs::copy(root.join(RECORD), foreign.join(RECORD)).unwrap();
        assert!(foreign_store.load_remote_project_outbox().is_err());
        drop(store);
        drop(foreign_store);
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(foreign).unwrap();
    }
    #[test]
    fn remote_project_outbox_preserves_partial_state_and_rejects_paths_or_authority() {
        assert!(validate_remote_project_action(&entry("approve")).is_err());
        let mut v = entry("stage");
        let Json::Object(fields) = &mut v else {
            panic!()
        };
        fields.push(("path".into(), Json::text("/tmp")));
        assert!(validate_remote_project_action(&v).is_err());
        let root = std::env::temp_dir().join(format!(
            "mesh-remote-project-partial-{}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let store = AttachmentStorage::open(&root).unwrap();
        fs::write(root.join(STAGING), b"retained interrupted input").unwrap();
        assert!(store.load_remote_project_outbox().is_err());
        assert!(store
            .save_remote_project_outbox(0, vec![entry("stage")])
            .is_err());
        assert_eq!(
            fs::read(root.join(STAGING)).unwrap(),
            b"retained interrupted input"
        );
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}
