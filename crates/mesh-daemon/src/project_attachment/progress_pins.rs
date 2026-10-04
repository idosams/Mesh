//! Private navigation only: exact progress selectors and view choices, never content or authority.
use super::{invalid, AttachmentStorage};
use crate::ipc::Json;
use mesh_cas::DurableFs as _;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read as _};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;
const RECORD: &str = "desktop-progress-pins.json";
const PENDING: &str = "desktop-progress-pins.pending";
const LIMIT: u64 = 65_536;
const SCHEMA: &str = "mesh.desktop-progress-pin-selectors/v1";
fn text<'a>(value: &'a Json, key: &str) -> io::Result<&'a str> {
    value
        .get(key)
        .and_then(Json::as_text)
        .ok_or_else(|| invalid("invalid progress pin field"))
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
/// Bounded exact progress selectors. Every consumer must reverify native history before showing bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressPinState {
    /// Compare-and-swap revision; zero represents an absent record.
    pub revision: u64,
    /// At most four closed selector objects. Saved content and paths are prohibited.
    pub pins: Vec<Json>,
}
impl ProgressPinState {
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
            return Err(invalid("progress pin snapshot exceeds limit"));
        }
        let value = Json::parse(raw).map_err(|_| invalid("invalid progress pin snapshot"))?;
        if !matches!(&value, Json::Object(fields) if fields.len() == 3)
            || text(&value, "schema")? != SCHEMA
        {
            return Err(invalid("invalid progress pin schema"));
        }
        let pins = value
            .get("pins")
            .and_then(Json::as_array)
            .ok_or_else(|| invalid("invalid progress pin list"))?
            .to_vec();
        validate(&pins)?;
        Ok(Self {
            revision: decimal(text(&value, "revision")?)?,
            pins,
        })
    }
}
fn validate(pins: &[Json]) -> io::Result<()> {
    if pins.len() > 4 {
        return Err(invalid("too many progress pins"));
    }
    let mut keys = BTreeSet::new();
    let mut selections = BTreeSet::new();
    for pin in pins {
        if !matches!(pin, Json::Object(fields) if fields.len() == 9) {
            return Err(invalid("unknown progress pin fields"));
        }
        let key = text(pin, "key")?;
        let objective = text(pin, "objective")?;
        if decimal(key)? == 0
            || !keys.insert(key)
            || !objective.strip_prefix("fleet-").is_some_and(|v| hex(v, 64))
        {
            return Err(invalid("invalid progress pin identity"));
        }
        for field in ["version", "source", "starting"] {
            if !hex(text(pin, field)?, 64) {
                return Err(invalid("invalid progress pin digest"));
            }
        }
        let lane = text(pin, "lane")?;
        if !lane
            .strip_prefix("lane-")
            .is_some_and(|value| hex(value, 64))
        {
            return Err(invalid("invalid progress lane"));
        }
        if !selections.insert((objective, lane, text(pin, "version")?)) {
            return Err(invalid("duplicate progress selection"));
        }
        match pin.get("after") {
            Some(Json::Null) => (),
            Some(Json::Text(value)) if hex(value, 32) => (),
            _ => return Err(invalid("invalid progress pin cursor")),
        }
        match pin.get("object") {
            Some(Json::Null) => (),
            Some(Json::Text(v)) if hex(v, 32) => (),
            _ => return Err(invalid("invalid progress pin object")),
        }
        if !["inline", "split"].contains(&text(pin, "layout")?) {
            return Err(invalid("invalid progress pin view"));
        }
    }
    Ok(())
}
impl AttachmentStorage {
    /// Read selectors without resolving projects or creating any native history.
    pub fn load_progress_pins(&self) -> io::Result<ProgressPinState> {
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|e| io::Error::other(e.to_string()))?;
        self.read_progress_pins()
    }
    /// Durable whole-set publication with a revision check.
    pub fn save_progress_pins(
        &self,
        expected: u64,
        pins: Vec<Json>,
    ) -> io::Result<ProgressPinState> {
        validate(&pins)?;
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|e| io::Error::other(e.to_string()))?;
        let current = self.read_progress_pins()?;
        if current.revision != expected {
            return Err(invalid("progress pin revision changed"));
        }
        if current.pins == pins {
            return Ok(current);
        }
        let next = ProgressPinState {
            revision: current
                .revision
                .checked_add(1)
                .ok_or_else(|| invalid("pin revision exhausted"))?,
            pins,
        };
        let bytes = self.progress_pin_record(&next)?.encode();
        if bytes.len() as u64 > LIMIT {
            return Err(invalid("progress pin record exceeds limit"));
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
        if self.read_progress_pins()? != next {
            return Err(invalid("progress pin acknowledgement changed"));
        }
        Ok(next)
    }
    fn progress_pin_record(&self, state: &ProgressPinState) -> io::Result<Json> {
        let (device, inode) = self.pinned.identity()?;
        Ok(Json::object([
            ("schema", Json::text("mesh.progress-pins/v1")),
            ("catalog_device", Json::text(format!("{device:016x}"))),
            ("catalog_inode", Json::text(format!("{inode:016x}"))),
            ("snapshot", state.to_json()),
        ]))
    }
    pub(super) fn read_progress_pins(&self) -> io::Result<ProgressPinState> {
        let fs = self.pinned.filesystem();
        let file = match fs.inspect_entry(Path::new(RECORD)) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                match fs.inspect_entry(Path::new(PENDING)) {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        self.pinned.ensure_namespace_identity()?;
                        return Ok(ProgressPinState {
                            revision: 0,
                            pins: vec![],
                        });
                    }
                    _ => return Err(invalid("unfinished progress pins need reconciliation")),
                }
            }
            Err(e) => return Err(e),
        };
        let m = file.metadata()?;
        if !m.is_file() || m.nlink() != 1 || m.permissions().mode() & 0o077 != 0 || m.len() > LIMIT
        {
            return Err(invalid("progress pins are not a bounded private file"));
        }
        let mut bytes = Vec::new();
        file.take(LIMIT + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > LIMIT {
            return Err(invalid("progress pins grew"));
        }
        let raw =
            std::str::from_utf8(&bytes).map_err(|_| invalid("progress pins are not UTF-8"))?;
        let record = Json::parse(raw).map_err(|_| invalid("invalid progress pin record"))?;
        let state = ProgressPinState::parse_projection(
            &record
                .get("snapshot")
                .ok_or_else(|| invalid("missing progress snapshot"))?
                .encode(),
        )?;
        if state.revision == 0 || self.progress_pin_record(&state)?.encode() != raw {
            return Err(invalid("progress pin storage identity changed"));
        }
        self.pinned.ensure_namespace_identity()?;
        Ok(state)
    }
}
