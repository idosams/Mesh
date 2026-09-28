//! Durable UI selectors only. Stored pins never attest to content or grant approval authority.

use super::{invalid, AttachmentStorage};
use crate::ipc::Json;
use mesh_cas::DurableFs as _;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read as _};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;

const RECORD: &str = "desktop-fleet-pins.json";
const PENDING: &str = "desktop-fleet-pins.pending";
const MAX_BYTES: u64 = 131_072;

/// A persisted navigation selector. Consumers must reverify its versions through native history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FleetPin {
    /// Stable display key; conveys no authority.
    pub key: String,
    /// Exact fleet identity.
    pub objective: String,
    /// Exact lane identity.
    pub lane: String,
    /// Completed checkpoint identity.
    pub checkpoint: String,
    /// Pinned saved operation.
    pub version: String,
    /// Recorded immutable review bundle.
    pub bundle: String,
    /// Exact original lane input.
    pub source_version: String,
    /// Saved text layout: inline or split.
    pub input_layout: String,
    /// Recorded review mode: visual or content.
    pub review_mode: String,
    /// Recorded review layout: inline or split.
    pub review_layout: String,
    /// Changed-object cursor, independent of selected object.
    pub input_after: Option<String>,
    /// Selected input-comparison object.
    pub input_object: Option<String>,
    /// Selected recorded-review object.
    pub review_object: Option<String>,
    /// Whether the starting-version comparison was opened.
    pub input_open: bool,
    /// Exact preparation request, retained before dispatch; no execution or approval authority.
    pub candidate: Option<FleetCandidatePin>,
}

/// Durable input selectors for one fixed project comparison. Content is always reverified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FleetCandidatePin {
    /// Registered original project identity.
    pub project: String,
    /// Idempotent preparation request.
    pub request: String,
    /// Fixed main head; absent means genesis, never latest.
    pub expected_main: Option<String>,
}
impl FleetCandidatePin {
    fn parse(value: &Json) -> io::Result<Self> {
        if !matches!(value, Json::Object(fields) if fields.len() == 3) {
            return Err(invalid("invalid candidate pin fields"));
        }
        Ok(Self {
            project: text(value, "project")?.into(),
            request: text(value, "request")?.into(),
            expected_main: optional_text(value, "expected_main")?,
        })
    }
    fn json(&self) -> Json {
        Json::object([
            ("project", Json::text(&self.project)),
            ("request", Json::text(&self.request)),
            (
                "expected_main",
                self.expected_main.as_deref().map_or(Json::Null, Json::text),
            ),
        ])
    }
}

/// A durable bounded snapshot with a compare-and-swap revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FleetPinState {
    /// Revision zero means no published snapshot exists.
    pub revision: u64,
    /// At most eight selectors, in display order; file contents are never persisted here.
    pub pins: Vec<FleetPin>,
}

impl FleetPinState {
    /// Bounded native/UI selector projection; no content or authority is carried in this value.
    pub fn to_json(&self) -> Json {
        Json::object([
            ("schema", Json::text("mesh.desktop-fleet-pin-selectors/v2")),
            ("revision", Json::text(self.revision.to_string())),
            (
                "pins",
                Json::Array(self.pins.iter().map(FleetPin::json).collect()),
            ),
        ])
    }

    /// Parse a bounded UI snapshot whose revision is the caller's expected native revision.
    pub fn parse_projection(encoded: &str) -> io::Result<Self> {
        if encoded.len() as u64 > MAX_BYTES {
            return Err(invalid("pin projection exceeds limit"));
        }
        let value = Json::parse(encoded).map_err(|_| invalid("invalid pin projection"))?;
        if !matches!(&value, Json::Object(fields) if fields.len() == 3)
            || ![
                "mesh.desktop-fleet-pin-selectors/v1",
                "mesh.desktop-fleet-pin-selectors/v2",
            ]
            .contains(&text(&value, "schema")?)
        {
            return Err(invalid("invalid pin projection schema"));
        }
        let revision_text = text(&value, "revision")?;
        let revision = revision_text
            .parse::<u64>()
            .map_err(|_| invalid("invalid pin revision"))?;
        if revision.to_string() != revision_text {
            return Err(invalid("noncanonical pin revision"));
        }
        let Some(Json::Array(entries)) = value.get("pins") else {
            return Err(invalid("invalid pin list"));
        };
        if entries.len() > 8 {
            return Err(invalid("too many pins"));
        }
        let pins = entries
            .iter()
            .map(|entry| FleetPin::parse(entry, text(&value, "schema")?.ends_with("/v1")))
            .collect::<io::Result<Vec<_>>>()?;
        validate(&pins)?;
        Ok(Self { revision, pins })
    }
}

impl FleetPin {
    fn parse(entry: &Json, legacy: bool) -> io::Result<Self> {
        if !matches!(entry, Json::Object(fields) if fields.len() == if legacy { 14 } else { 15 }) {
            return Err(invalid("invalid fleet pin fields"));
        }
        Ok(Self {
            candidate: if legacy {
                None
            } else {
                match entry.get("candidate") {
                    Some(Json::Null) => None,
                    Some(value) => Some(FleetCandidatePin::parse(value)?),
                    None => return Err(invalid("missing candidate pin")),
                }
            },
            key: text(entry, "key")?.into(),
            objective: text(entry, "objective")?.into(),
            lane: text(entry, "lane")?.into(),
            checkpoint: text(entry, "checkpoint")?.into(),
            version: text(entry, "version")?.into(),
            bundle: text(entry, "bundle")?.into(),
            source_version: text(entry, "source_version")?.into(),
            input_layout: text(entry, "input_layout")?.into(),
            review_mode: text(entry, "review_mode")?.into(),
            review_layout: text(entry, "review_layout")?.into(),
            input_after: optional_text(entry, "input_after")?,
            input_object: optional_text(entry, "input_object")?,
            review_object: optional_text(entry, "review_object")?,
            input_open: match entry.get("input_open") {
                Some(Json::Bool(value)) => *value,
                _ => return Err(invalid("invalid fleet pin comparison state")),
            },
        })
    }
    fn json(&self) -> Json {
        self.json_version(false)
    }
    fn json_version(&self, legacy: bool) -> Json {
        let value = Json::object([
            ("key", Json::text(&self.key)),
            ("objective", Json::text(&self.objective)),
            ("lane", Json::text(&self.lane)),
            ("checkpoint", Json::text(&self.checkpoint)),
            ("version", Json::text(&self.version)),
            ("bundle", Json::text(&self.bundle)),
            ("source_version", Json::text(&self.source_version)),
            ("input_layout", Json::text(&self.input_layout)),
            ("review_mode", Json::text(&self.review_mode)),
            ("review_layout", Json::text(&self.review_layout)),
            (
                "input_after",
                self.input_after.as_deref().map_or(Json::Null, Json::text),
            ),
            (
                "input_object",
                self.input_object.as_deref().map_or(Json::Null, Json::text),
            ),
            (
                "review_object",
                self.review_object.as_deref().map_or(Json::Null, Json::text),
            ),
            ("input_open", Json::Bool(self.input_open)),
        ]);
        if legacy {
            return value;
        }
        let Json::Object(mut fields) = value else {
            unreachable!()
        };
        fields.push((
            "candidate".into(),
            self.candidate
                .as_ref()
                .map_or(Json::Null, FleetCandidatePin::json),
        ));
        Json::Object(fields)
    }
}
fn valid_object(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn validate(pins: &[FleetPin]) -> io::Result<()> {
    if pins.len() > 8 {
        return Err(invalid("too many persisted fleet pins"));
    }
    let mut keys = BTreeSet::new();
    let mut selections = BTreeSet::new();
    for pin in pins {
        let key = pin
            .key
            .parse::<u64>()
            .map_err(|_| invalid("invalid fleet pin key"))?;
        if pin.candidate.as_ref().is_some_and(|candidate| {
            !super::provisioning::valid_id(&candidate.project)
                || !valid_object(&candidate.request)
                || candidate
                    .expected_main
                    .as_ref()
                    .is_some_and(|head| !super::provisioning::valid_id(head))
        }) {
            return Err(invalid("invalid candidate selectors"));
        }
        if key == 0
            || key.to_string() != pin.key
            || !keys.insert(&pin.key)
            || !selections.insert((
                &pin.objective,
                &pin.lane,
                &pin.checkpoint,
                &pin.version,
                &pin.bundle,
            ))
            || !pin
                .objective
                .strip_prefix("fleet-")
                .is_some_and(super::provisioning::valid_id)
            || [&pin.lane, &pin.checkpoint].into_iter().any(|id| {
                id.is_empty()
                    || id.len() > 128
                    || !id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
            })
            || [&pin.version, &pin.bundle, &pin.source_version]
                .into_iter()
                .any(|id| !super::provisioning::valid_id(id))
            || [&pin.input_after, &pin.input_object, &pin.review_object]
                .into_iter()
                .flatten()
                .any(|id| !valid_object(id))
            || !["inline", "split"].contains(&pin.input_layout.as_str())
            || !["inline", "split"].contains(&pin.review_layout.as_str())
            || !["visual", "content"].contains(&pin.review_mode.as_str())
            || (!pin.input_open && (pin.input_after.is_some() || pin.input_object.is_some()))
        {
            return Err(invalid("invalid persisted fleet selector"));
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
    pub fn load_fleet_pins(&self) -> io::Result<FleetPinState> {
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|error| io::Error::other(error.to_string()))?;
        self.read_fleet_pins()
    }

    /// Atomically publish a complete pin snapshot only if its expected revision is current.
    /// A successful return follows file and parent-directory durability. Failed staging is retained.
    pub fn save_fleet_pins(
        &self,
        expected_revision: u64,
        pins: Vec<FleetPin>,
    ) -> io::Result<FleetPinState> {
        validate(&pins)?;
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|error| io::Error::other(error.to_string()))?;
        let current = self.read_fleet_pins()?;
        if current.revision != expected_revision {
            return Err(invalid("comparison pin revision changed"));
        }
        if current.pins == pins {
            return Ok(current);
        }
        let next = FleetPinState {
            revision: current
                .revision
                .checked_add(1)
                .ok_or_else(|| invalid("pin revision exhausted"))?,
            pins,
        };
        let bytes = self.fleet_pin_record(&next, false)?.encode();
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
        if self.read_fleet_pins()? != next {
            return Err(invalid(
                "published pin snapshot changed before acknowledgement",
            ));
        }
        Ok(next)
    }

    fn fleet_pin_record(&self, state: &FleetPinState, legacy: bool) -> io::Result<Json> {
        let (device, inode) = self.pinned.identity()?;
        Ok(Json::object([
            (
                "schema",
                Json::text(if legacy {
                    "mesh.fleet-pins/v1"
                } else {
                    "mesh.fleet-pins/v2"
                }),
            ),
            ("catalog_device", Json::text(format!("{device:016x}"))),
            ("catalog_inode", Json::text(format!("{inode:016x}"))),
            ("revision", Json::text(state.revision.to_string())),
            (
                "pins",
                Json::Array(
                    state
                        .pins
                        .iter()
                        .map(|pin| pin.json_version(legacy))
                        .collect(),
                ),
            ),
        ]))
    }
    fn read_fleet_pins(&self) -> io::Result<FleetPinState> {
        let filesystem = self.pinned.filesystem();
        let file = match filesystem.inspect_entry(Path::new(RECORD)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match filesystem.inspect_entry(Path::new(PENDING)) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        self.pinned.ensure_namespace_identity()?;
                        return Ok(FleetPinState {
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
            || metadata.nlink() != 1
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
        let legacy = match text(&record, "schema")? {
            "mesh.fleet-pins/v1" => true,
            "mesh.fleet-pins/v2" => false,
            _ => return Err(invalid("unknown fleet pin schema")),
        };
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
            .map(|entry| FleetPin::parse(entry, legacy))
            .collect::<io::Result<Vec<_>>>()?;
        validate(&pins)?;
        let state = FleetPinState { revision, pins };
        if self.fleet_pin_record(&state, legacy)?.encode() != encoded {
            return Err(invalid("pin snapshot identity or schema changed"));
        }
        self.pinned.ensure_namespace_identity()?;
        Ok(state)
    }
}
