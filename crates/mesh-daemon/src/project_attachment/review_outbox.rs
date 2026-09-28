//! Pending review inputs, never execution or approval authority. Reads do not dispatch work.
use super::{invalid, AttachmentStorage};
use crate::ipc::Json;
use mesh_cas::DurableFs as _;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read as _};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;

const RECORD: &str = "fleet-review-outbox.json";
const STAGING: &str = "fleet-review-outbox.pending";
const LIMIT: u64 = 131_072;

/// A bounded set of exact pending inputs, independent of whether a review panel remains open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FleetReviewOutbox {
    /// Compare-and-swap revision. Zero represents a never-written outbox.
    pub revision: u64,
    /// Closed-schema pending requests; at most eight. Native history must verify their selectors.
    pub entries: Vec<Json>,
}
impl FleetReviewOutbox {
    /// Renderer projection. No saved content, receipt or authority is inferred from these inputs.
    pub fn to_json(&self) -> Json {
        Json::object([
            ("schema", Json::text("mesh.fleet-review-outbox/v1")),
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
        if text(&value, "schema")? != "mesh.fleet-review-outbox/v1" {
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
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
}
fn operation_token(entry: &Json) -> io::Result<&str> {
    let input = entry
        .get("input")
        .ok_or_else(|| invalid("missing pending input"))?;
    text(
        input,
        if text(entry, "kind")? == "change" {
            "request"
        } else {
            "operation"
        },
    )
}
fn validate(entries: &[Json]) -> io::Result<()> {
    if entries.len() > 8 {
        return Err(invalid("too many pending review operations"));
    }
    let mut operations = BTreeSet::new();
    for entry in entries {
        fields(entry, 4)?;
        let objective = text(entry, "objective")?;
        if !objective.strip_prefix("fleet-").is_some_and(|v| hex(v, 64)) {
            return Err(invalid("invalid pending objective"));
        }
        let selection = entry
            .get("selection")
            .ok_or_else(|| invalid("missing pending selection"))?;
        fields(selection, 4)?;
        for key in ["lane", "checkpoint"] {
            if !identifier(text(selection, key)?) {
                return Err(invalid("invalid pending selection"));
            }
        }
        for key in ["version", "bundle"] {
            if !hex(text(selection, key)?, 64) {
                return Err(invalid("invalid pending selection"));
            }
        }
        let input = entry
            .get("input")
            .ok_or_else(|| invalid("missing pending input"))?;
        let operation = match text(entry, "kind")? {
            "change" => {
                fields(input, 2)?;
                let message = text(input, "message")?;
                if message.trim().is_empty() || message.len() > 8192 || message.chars().any(|c| matches!(c as u32, 0..=8 | 11..=31 | 127..=159 | 0x061c | 0x200e..=0x200f | 0x202a..=0x202e | 0x2066..=0x2069)) { return Err(invalid("invalid pending change message")); }
                text(input, "request")?
            }
            "decision" => {
                fields(input, 6)?;
                if !text(input, "request")?
                    .strip_prefix("review-change-")
                    .is_some_and(|v| hex(v, 64))
                {
                    return Err(invalid("invalid pending change identity"));
                }
                let revision = text(input, "expected_revision")?;
                let parsed = revision
                    .parse::<u8>()
                    .map_err(|_| invalid("invalid pending decision revision"))?;
                if parsed >= 64 || parsed.to_string() != revision {
                    return Err(invalid("invalid pending decision revision"));
                }
                match input.get("checkpoint") {
                    Some(Json::Null)
                        if input.get("version") == Some(&Json::Null)
                            && input.get("bundle") == Some(&Json::Null) => {}
                    Some(Json::Text(checkpoint))
                        if identifier(checkpoint)
                            && hex(text(input, "version")?, 64)
                            && hex(text(input, "bundle")?, 64) => {}
                    _ => return Err(invalid("invalid pending decision target")),
                }
                text(input, "operation")?
            }
            _ => return Err(invalid("unknown pending review operation")),
        };
        if !hex(operation, 32) || !operations.insert(operation) {
            return Err(invalid("duplicate or invalid pending operation"));
        }
    }
    Ok(())
}
impl AttachmentStorage {
    /// Read pending inputs only. The caller must explicitly choose whether to retry an operation.
    pub fn load_fleet_review_outbox(&self) -> io::Result<FleetReviewOutbox> {
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|e| io::Error::other(e.to_string()))?;
        self.read_review_outbox()
    }
    /// Durably replace pending inputs at an exact revision; no operation is submitted here.
    pub fn save_fleet_review_outbox(
        &self,
        expected: u64,
        entries: Vec<Json>,
    ) -> io::Result<FleetReviewOutbox> {
        validate(&entries)?;
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|e| io::Error::other(e.to_string()))?;
        let current = self.read_review_outbox()?;
        if current.revision != expected {
            return Err(invalid("pending review revision changed"));
        }
        for previous in &current.entries {
            let token = operation_token(previous)?;
            for next in &entries {
                if operation_token(next)? == token && next != previous {
                    return Err(invalid(
                        "pending review inputs changed under the same operation",
                    ));
                }
            }
        }
        if current.entries == entries {
            return Ok(current);
        }
        let next = FleetReviewOutbox {
            revision: expected
                .checked_add(1)
                .ok_or_else(|| invalid("outbox revision exhausted"))?,
            entries,
        };
        let encoded = self.review_outbox_record(&next)?.encode();
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
        if self.read_review_outbox()? != next {
            return Err(invalid("outbox changed before acknowledgement"));
        }
        Ok(next)
    }
    fn review_outbox_record(&self, state: &FleetReviewOutbox) -> io::Result<Json> {
        let (device, inode) = self.pinned.identity()?;
        Ok(Json::object([
            ("schema", Json::text("mesh.bound-fleet-review-outbox/v1")),
            ("device", Json::text(format!("{device:016x}"))),
            ("inode", Json::text(format!("{inode:016x}"))),
            ("state", state.to_json()),
        ]))
    }
    fn read_review_outbox(&self) -> io::Result<FleetReviewOutbox> {
        let filesystem = self.pinned.filesystem();
        let file = match filesystem.inspect_entry(Path::new(RECORD)) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                match filesystem.inspect_entry(Path::new(STAGING)) {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        self.pinned.ensure_namespace_identity()?;
                        return Ok(FleetReviewOutbox {
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
        let state = FleetReviewOutbox::parse(
            &value
                .get("state")
                .ok_or_else(|| invalid("missing outbox state"))?
                .encode(),
        )?;
        if state.revision == 0 || self.review_outbox_record(&state)?.encode() != raw {
            return Err(invalid("outbox identity or schema changed"));
        }
        self.pinned.ensure_namespace_identity()?;
        Ok(state)
    }
}
