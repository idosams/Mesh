//! Durable UI selectors only. Stored pins never attest to content or grant approval authority.

use super::{invalid, AttachmentStorage};
use crate::ipc::Json;
use mesh_cas::DurableFs as _;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

const RECORD: &str = "desktop-comparison-pins.json";
const PENDING: &str = "desktop-comparison-pins.pending";
const MAX_BYTES: u64 = 131_072;

/// A persisted navigation selector. Consumers must reverify its versions through native history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentPin {
    /// Stable display key, unique within this snapshot; conveys no authority.
    pub key: String,
    /// Native registered project identifier.
    pub project: String,
    /// Exact comparison base operation.
    pub base: String,
    /// Exact comparison target operation.
    pub target: String,
    /// Cursor preceding the displayed change page, or the first page.
    pub after: Option<String>,
    /// Selected changed path, independently of the displayed page.
    pub path: Option<String>,
}

/// A durable bounded snapshot with a compare-and-swap revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentPinState {
    /// Revision zero means no published snapshot exists.
    pub revision: u64,
    /// At most eight selectors, in display order; file contents are never persisted here.
    pub pins: Vec<AttachmentPin>,
}

impl AttachmentPin {
    fn json(&self) -> Json {
        Json::object([
            ("key", Json::text(&self.key)),
            ("project", Json::text(&self.project)),
            ("base", Json::text(&self.base)),
            ("target", Json::text(&self.target)),
            (
                "after",
                self.after.as_deref().map_or(Json::Null, Json::text),
            ),
            ("path", self.path.as_deref().map_or(Json::Null, Json::text)),
        ])
    }
}
fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.starts_with('/')
        && !path.chars().any(char::is_control)
        && !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
}
fn validate(pins: &[AttachmentPin]) -> io::Result<()> {
    if pins.len() > 8 {
        return Err(invalid("too many persisted comparison pins"));
    }
    let mut keys = BTreeSet::new();
    for pin in pins {
        let key = pin
            .key
            .parse::<u64>()
            .map_err(|_| invalid("invalid pin key"))?;
        if key == 0
            || key.to_string() != pin.key
            || !keys.insert(&pin.key)
            || [&pin.project, &pin.base, &pin.target]
                .iter()
                .any(|id| !super::provisioning::valid_id(id))
            || [&pin.after, &pin.path]
                .into_iter()
                .flatten()
                .any(|path| !valid_path(path))
        {
            return Err(invalid("invalid persisted comparison selector"));
        }
    }
    Ok(())
}
fn text<'a>(value: &'a Json, key: &str) -> io::Result<&'a str> {
    value
        .get(key)
        .and_then(Json::as_text)
        .ok_or_else(|| invalid("missing pin record field"))
}
fn optional_text(value: &Json, key: &str) -> io::Result<Option<String>> {
    match value.get(key) {
        Some(Json::Null) => Ok(None),
        Some(value) => value
            .as_text()
            .map(|value| Some(value.to_owned()))
            .ok_or_else(|| invalid("invalid optional pin field")),
        None => Err(invalid("missing optional pin field")),
    }
}
impl AttachmentStorage {
    /// Load selectors without resolving source folders or trusting them as verified history.
    pub fn load_comparison_pins(&self) -> io::Result<AttachmentPinState> {
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|error| io::Error::other(error.to_string()))?;
        self.read_pins()
    }

    /// Atomically publish a complete pin snapshot only if its expected revision is current.
    /// A successful return follows file and parent-directory durability. Failed staging is retained.
    pub fn save_comparison_pins(
        &self,
        expected_revision: u64,
        pins: Vec<AttachmentPin>,
    ) -> io::Result<AttachmentPinState> {
        validate(&pins)?;
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|error| io::Error::other(error.to_string()))?;
        let current = self.read_pins()?;
        if current.revision != expected_revision {
            return Err(invalid("comparison pin revision changed"));
        }
        if current.pins == pins {
            return Ok(current);
        }
        let next = AttachmentPinState {
            revision: current
                .revision
                .checked_add(1)
                .ok_or_else(|| invalid("pin revision exhausted"))?,
            pins,
        };
        let bytes = self.pin_record(&next)?.encode();
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid("pin snapshot exceeds limit"));
        }
        let filesystem = self.pinned.filesystem();
        filesystem.write_new_file(
            Path::new(PENDING),
            bytes.as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        self.pinned.ensure_namespace_identity()?;
        filesystem.rename(Path::new(PENDING), Path::new(RECORD))?;
        self.pinned.sync()?;
        self.pinned.ensure_namespace_identity()?;
        if self.read_pins()? != next {
            return Err(invalid(
                "published pin snapshot changed before acknowledgement",
            ));
        }
        Ok(next)
    }

    fn pin_record(&self, state: &AttachmentPinState) -> io::Result<Json> {
        let (device, inode) = self.pinned.identity()?;
        Ok(Json::object([
            ("schema", Json::text("mesh.attachment-pins/v1")),
            ("catalog_device", Json::text(format!("{device:016x}"))),
            ("catalog_inode", Json::text(format!("{inode:016x}"))),
            ("revision", Json::text(state.revision.to_string())),
            (
                "pins",
                Json::Array(state.pins.iter().map(AttachmentPin::json).collect()),
            ),
        ]))
    }
    fn read_pins(&self) -> io::Result<AttachmentPinState> {
        let filesystem = self.pinned.filesystem();
        let file = match filesystem.inspect_entry(Path::new(RECORD)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match filesystem.inspect_entry(Path::new(PENDING)) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        self.pinned.ensure_namespace_identity()?;
                        return Ok(AttachmentPinState {
                            revision: 0,
                            pins: Vec::new(),
                        });
                    }
                    _ => {
                        return Err(invalid(
                            "unfinished comparison pin snapshot needs reconciliation",
                        ))
                    }
                }
            }
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.permissions().mode() & 0o077 != 0
            || metadata.len() > MAX_BYTES
        {
            return Err(invalid("pin snapshot is not a bounded private file"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid("pin snapshot exceeds limit"));
        }
        let encoded = String::from_utf8(bytes).map_err(|_| invalid("pin snapshot is not UTF-8"))?;
        let record = Json::parse(&encoded).map_err(|_| invalid("invalid pin snapshot"))?;
        let revision = text(&record, "revision")?
            .parse::<u64>()
            .map_err(|_| invalid("invalid pin revision"))?;
        if revision == 0 {
            return Err(invalid("published pin revision cannot be zero"));
        }
        let Some(Json::Array(entries)) = record.get("pins") else {
            return Err(invalid("invalid pin list"));
        };
        if entries.len() > 8 {
            return Err(invalid("too many persisted comparison pins"));
        }
        let pins = entries
            .iter()
            .map(|entry| {
                Ok(AttachmentPin {
                    key: text(entry, "key")?.to_owned(),
                    project: text(entry, "project")?.to_owned(),
                    base: text(entry, "base")?.to_owned(),
                    target: text(entry, "target")?.to_owned(),
                    after: optional_text(entry, "after")?,
                    path: optional_text(entry, "path")?,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        validate(&pins)?;
        let state = AttachmentPinState { revision, pins };
        if self.pin_record(&state)?.encode() != encoded {
            return Err(invalid("pin snapshot identity or schema changed"));
        }
        self.pinned.ensure_namespace_identity()?;
        Ok(state)
    }
}
