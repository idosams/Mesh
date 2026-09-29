//! Private navigation only: exact remote selectors and view choices, never content or authority.
use super::{invalid, AttachmentStorage};
use crate::ipc::Json;
use mesh_cas::DurableFs as _;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read as _};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;
const RECORD: &str = "desktop-remote-fleet-pins.json";
const PENDING: &str = "desktop-remote-fleet-pins.pending";
const LIMIT: u64 = 65_536;
const SCHEMA: &str = "mesh.desktop-remote-fleet-pin-selectors/v1";
fn text<'a>(value: &'a Json, key: &str) -> io::Result<&'a str> {
    value
        .get(key)
        .and_then(Json::as_text)
        .ok_or_else(|| invalid("invalid remote pin field"))
}
fn hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn decimal(value: &str) -> io::Result<u64> {
    let n = value
        .parse::<u64>()
        .map_err(|_| invalid("invalid pin revision"))?;
    if n.to_string() != value {
        return Err(invalid("noncanonical pin revision"));
    }
    Ok(n)
}
/// Bounded exact remote selectors. Every consumer must reverify native history before showing bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteFleetPinState {
    /// Compare-and-swap revision; zero represents an absent record.
    pub revision: u64,
    /// At most eight closed selector objects. Saved content and paths are prohibited.
    pub pins: Vec<Json>,
}
impl RemoteFleetPinState {
    /// Public projection contains only selectors and view choices.
    pub fn to_json(&self) -> Json {
        Json::object([
            ("schema", Json::text(SCHEMA)),
            ("revision", Json::text(self.revision.to_string())),
            ("pins", Json::Array(self.pins.clone())),
        ])
    }
    /// Validate renderer input before touching native storage.
    pub fn parse_projection(raw: &str) -> io::Result<Self> {
        if raw.len() as u64 > LIMIT {
            return Err(invalid("remote pin snapshot exceeds limit"));
        }
        let value = Json::parse(raw).map_err(|_| invalid("invalid remote pin snapshot"))?;
        if !matches!(&value, Json::Object(fields) if fields.len() == 3)
            || text(&value, "schema")? != SCHEMA
        {
            return Err(invalid("invalid remote pin schema"));
        }
        let pins = value
            .get("pins")
            .and_then(Json::as_array)
            .ok_or_else(|| invalid("invalid remote pin list"))?
            .to_vec();
        validate(&pins)?;
        Ok(Self {
            revision: decimal(text(&value, "revision")?)?,
            pins,
        })
    }
}
fn validate(pins: &[Json]) -> io::Result<()> {
    if pins.len() > 8 {
        return Err(invalid("too many remote pins"));
    }
    let mut keys = BTreeSet::new();
    let mut selections = BTreeSet::new();
    for pin in pins {
        if !matches!(pin, Json::Object(fields) if fields.len() == 12) {
            return Err(invalid("unknown remote pin fields"));
        }
        let key = text(pin, "key")?;
        let objective = text(pin, "objective")?;
        if decimal(key)? == 0
            || !keys.insert(key)
            || !objective.strip_prefix("fleet-").is_some_and(|v| hex(v, 64))
        {
            return Err(invalid("invalid remote pin identity"));
        }
        for field in [
            "offer",
            "correlation",
            "version",
            "bundle",
            "remote_version",
        ] {
            if !hex(text(pin, field)?, 64) {
                return Err(invalid("invalid remote pin digest"));
            }
        }
        for field in ["lane", "run"] {
            let value = text(pin, field)?;
            if value.is_empty()
                || value.len() > 128
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
            {
                return Err(invalid("invalid remote pin run"));
            }
        }
        if !selections.insert((objective, text(pin, "correlation")?)) {
            return Err(invalid("duplicate remote selection"));
        }
        match pin.get("object") {
            Some(Json::Null) => (),
            Some(Json::Text(v)) if hex(v, 32) => (),
            _ => return Err(invalid("invalid remote pin object")),
        }
        if !["content", "visual"].contains(&text(pin, "mode")?)
            || !["inline", "split"].contains(&text(pin, "layout")?)
        {
            return Err(invalid("invalid remote pin view"));
        }
    }
    Ok(())
}
impl AttachmentStorage {
    /// Read selectors without resolving projects or creating any native history.
    pub fn load_remote_fleet_pins(&self) -> io::Result<RemoteFleetPinState> {
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|e| io::Error::other(e.to_string()))?;
        self.read_remote_fleet_pins()
    }
    /// Durable whole-set publication with a revision check and shared local/remote capacity.
    pub fn save_remote_fleet_pins(
        &self,
        expected: u64,
        pins: Vec<Json>,
    ) -> io::Result<RemoteFleetPinState> {
        validate(&pins)?;
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|e| io::Error::other(e.to_string()))?;
        let current = self.read_remote_fleet_pins()?;
        if current.revision != expected {
            return Err(invalid("remote pin revision changed"));
        }
        if pins.len() + self.read_fleet_pins()?.pins.len() > 8 {
            return Err(invalid("shared review capacity exceeded"));
        }
        if current.pins == pins {
            return Ok(current);
        }
        let next = RemoteFleetPinState {
            revision: current
                .revision
                .checked_add(1)
                .ok_or_else(|| invalid("pin revision exhausted"))?,
            pins,
        };
        let bytes = self.remote_pin_record(&next)?.encode();
        if bytes.len() as u64 > LIMIT {
            return Err(invalid("remote pin record exceeds limit"));
        }
        let fs = self.pinned.filesystem();
        fs.write_new_file(
            Path::new(PENDING),
            bytes.as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        self.pinned.ensure_namespace_identity()?;
        fs.rename(Path::new(PENDING), Path::new(RECORD))?;
        self.pinned.sync()?;
        if self.read_remote_fleet_pins()? != next {
            return Err(invalid("remote pin acknowledgement changed"));
        }
        Ok(next)
    }
    fn remote_pin_record(&self, state: &RemoteFleetPinState) -> io::Result<Json> {
        let (device, inode) = self.pinned.identity()?;
        Ok(Json::object([
            ("schema", Json::text("mesh.remote-fleet-pins/v1")),
            ("catalog_device", Json::text(format!("{device:016x}"))),
            ("catalog_inode", Json::text(format!("{inode:016x}"))),
            ("snapshot", state.to_json()),
        ]))
    }
    pub(super) fn read_remote_fleet_pins(&self) -> io::Result<RemoteFleetPinState> {
        let fs = self.pinned.filesystem();
        let file = match fs.inspect_entry(Path::new(RECORD)) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                match fs.inspect_entry(Path::new(PENDING)) {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        self.pinned.ensure_namespace_identity()?;
                        return Ok(RemoteFleetPinState {
                            revision: 0,
                            pins: vec![],
                        });
                    }
                    _ => return Err(invalid("unfinished remote pins need reconciliation")),
                }
            }
            Err(e) => return Err(e),
        };
        let m = file.metadata()?;
        if !m.is_file() || m.nlink() != 1 || m.permissions().mode() & 0o077 != 0 || m.len() > LIMIT
        {
            return Err(invalid("remote pins are not a bounded private file"));
        }
        let mut bytes = Vec::new();
        file.take(LIMIT + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > LIMIT {
            return Err(invalid("remote pins grew"));
        }
        let raw = std::str::from_utf8(&bytes).map_err(|_| invalid("remote pins are not UTF-8"))?;
        let record = Json::parse(raw).map_err(|_| invalid("invalid remote pin record"))?;
        let state = RemoteFleetPinState::parse_projection(
            &record
                .get("snapshot")
                .ok_or_else(|| invalid("missing remote snapshot"))?
                .encode(),
        )?;
        if state.revision == 0 || self.remote_pin_record(&state)?.encode() != raw {
            return Err(invalid("remote pin storage identity changed"));
        }
        self.pinned.ensure_namespace_identity()?;
        Ok(state)
    }
}
