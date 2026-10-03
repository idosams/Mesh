//! Private native connection settings. Records are convenience data, never execution authority.
use super::{invalid, AttachmentStorage};
use crate::ipc::Json;
use mesh_cas::DurableFs as _;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read as _},
    path::Path,
};
const RECORD: &str = "desktop-remote-connections.json";
const PENDING: &str = "desktop-remote-connections.pending";
const MAX_BYTES: u64 = 524_288;
/// One native-only settings entry. A consumer must independently re-admit all paths and identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteConnectionSettings {
    /// Native-generated opaque identifier; never a path.
    pub id: String,
    /// Operator-selected display label.
    pub label: String,
    /// Private native configuration and identity bindings; never expose this value to a renderer.
    pub value: Json,
}
/// A revisioned snapshot of saved native settings, with no implied active connection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteConnectionSettingsState {
    /// Zero denotes no committed snapshot.
    pub revision: u64,
    /// At most sixteen named settings records.
    pub entries: Vec<RemoteConnectionSettings>,
}
fn validate(entries: &[RemoteConnectionSettings]) -> io::Result<()> {
    let mut ids = BTreeSet::new();
    if entries.len() > 16 {
        return Err(invalid("too many saved connections"));
    }
    for entry in entries {
        if !super::provisioning::valid_id(&entry.id) || !ids.insert(&entry.id)
            || entry.label.trim().is_empty() || entry.label.len() > 128
            || entry.label.chars().any(|c| c.is_control() || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
            || !matches!(entry.value, Json::Object(_)) || entry.value.encode().len() > 24_576 {
            return Err(invalid("invalid saved connection"));
        }
    }
    Ok(())
}
impl AttachmentStorage {
    /// Read private settings without opening their referenced identities, keys or network endpoints.
    /// An interrupted write requires explicit recovery; it is not silently discarded or adopted.
    pub fn load_remote_connection_settings(&self) -> io::Result<RemoteConnectionSettingsState> {
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|e| io::Error::other(e.to_string()))?;
        if self.read_connection_record(PENDING)?.is_some() {
            return Err(invalid("interrupted connection settings need recovery"));
        }
        Ok(self.read_connection_record(RECORD)?.unwrap_or_default())
    }
    /// Persist native-computed settings only at the expected revision. Failed staging is retained.
    pub fn save_remote_connection_settings(
        &self,
        expected: u64,
        entries: Vec<RemoteConnectionSettings>,
    ) -> io::Result<RemoteConnectionSettingsState> {
        validate(&entries)?;
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|e| io::Error::other(e.to_string()))?;
        if self.read_connection_record(PENDING)?.is_some() {
            return Err(invalid("interrupted connection settings need recovery"));
        }
        let current = self.read_connection_record(RECORD)?.unwrap_or_default();
        if current.revision != expected {
            return Err(invalid("connection settings revision changed"));
        }
        if current.entries == entries {
            return Ok(current);
        }
        let next = RemoteConnectionSettingsState {
            revision: expected
                .checked_add(1)
                .ok_or_else(|| invalid("connection revision exhausted"))?,
            entries,
        };
        let bytes = self.connection_record(&next)?.encode();
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid("connection settings exceed limit"));
        }
        self.pinned.filesystem().write_new_file(
            Path::new(PENDING),
            bytes.as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        self.publish_connections(&next)
    }
    /// Explicitly finish an exact next-revision settings write. This performs no connection/action.
    pub fn recover_remote_connection_settings(&self) -> io::Result<RemoteConnectionSettingsState> {
        self.pinned.ensure_namespace_identity()?;
        let _guard = crate::workspace_custody::lock_workspace_initialization(&self.pinned)
            .map_err(|e| io::Error::other(e.to_string()))?;
        let current = self.read_connection_record(RECORD)?.unwrap_or_default();
        let Some(pending) = self.read_connection_record(PENDING)? else {
            return Ok(current);
        };
        if current.revision.checked_add(1) != Some(pending.revision) {
            return Err(invalid("pending connection revision needs reconciliation"));
        }
        self.publish_connections(&pending)
    }
    fn publish_connections(
        &self,
        next: &RemoteConnectionSettingsState,
    ) -> io::Result<RemoteConnectionSettingsState> {
        self.pinned.ensure_namespace_identity()?;
        self.pinned
            .filesystem()
            .rename(Path::new(PENDING), Path::new(RECORD))?;
        self.pinned.sync()?;
        self.pinned.ensure_namespace_identity()?;
        let actual = self
            .read_connection_record(RECORD)?
            .ok_or_else(|| invalid("connection settings disappeared"))?;
        if &actual != next {
            return Err(invalid(
                "connection settings changed before acknowledgement",
            ));
        }
        Ok(actual)
    }
    fn connection_record(&self, state: &RemoteConnectionSettingsState) -> io::Result<Json> {
        let (device, inode) = self.pinned.identity()?;
        Ok(Json::object([
            (
                "schema",
                Json::text("mesh.native-remote-connection-settings/v1"),
            ),
            ("catalog_device", Json::text(format!("{device:016x}"))),
            ("catalog_inode", Json::text(format!("{inode:016x}"))),
            ("revision", Json::text(state.revision.to_string())),
            (
                "entries",
                Json::Array(
                    state
                        .entries
                        .iter()
                        .map(|e| {
                            Json::object([
                                ("id", Json::text(&e.id)),
                                ("label", Json::text(&e.label)),
                                ("value", e.value.clone()),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]))
    }
    fn read_connection_record(
        &self,
        name: &str,
    ) -> io::Result<Option<RemoteConnectionSettingsState>> {
        let file = match self.pinned.filesystem().inspect_entry(Path::new(name)) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let m = file.metadata()?;
        if !m.is_file()
            || m.nlink() != 1
            || m.mode() & 0o077 != 0
            || m.len() > MAX_BYTES
            || m.uid() != fs::symlink_metadata(&self.path)?.uid()
        {
            return Err(invalid(
                "connection settings are not a bounded private file",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid("connection settings exceed limit"));
        }
        let encoded = std::str::from_utf8(&bytes)
            .map_err(|_| invalid("invalid connection settings encoding"))?;
        let record = Json::parse(encoded).map_err(|_| invalid("invalid connection settings"))?;
        let text = |value: &Json, name: &str| {
            value
                .get(name)
                .and_then(Json::as_text)
                .map(String::from)
                .ok_or_else(|| invalid("missing connection field"))
        };
        let revision = text(&record, "revision")?
            .parse::<u64>()
            .map_err(|_| invalid("invalid connection revision"))?;
        if revision == 0 {
            return Err(invalid("published connection revision cannot be zero"));
        }
        let entries = record
            .get("entries")
            .and_then(Json::as_array)
            .ok_or_else(|| invalid("missing connection entries"))?;
        if entries.len() > 16 {
            return Err(invalid("too many saved connections"));
        }
        let entries = entries
            .iter()
            .map(|e| {
                Ok(RemoteConnectionSettings {
                    id: text(e, "id")?,
                    label: text(e, "label")?,
                    value: e
                        .get("value")
                        .ok_or_else(|| invalid("missing native connection value"))?
                        .clone(),
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        validate(&entries)?;
        let state = RemoteConnectionSettingsState { revision, entries };
        if self.connection_record(&state)?.encode() != encoded {
            return Err(invalid("connection settings identity or schema changed"));
        }
        self.pinned.ensure_namespace_identity()?;
        Ok(Some(state))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let p = std::env::temp_dir().join(format!(
                "mesh-connection-records-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir(&p).unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
            Self(p)
        }
        fn open(&self) -> AttachmentStorage {
            AttachmentStorage::open(&self.0).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn entry() -> RemoteConnectionSettings {
        RemoteConnectionSettings {
            id: "a".repeat(64),
            label: "worker".into(),
            value: Json::object([("placeholder", Json::text("not credentials"))]),
        }
    }
    #[test]
    fn invalid_names_values_and_count_never_create_a_pending_record() {
        let f = Fixture::new();
        let store = f.open();
        for label in ["", "  ", "worker\n", "worker\u{202e}"] {
            let mut invalid = entry();
            invalid.label = label.into();
            assert!(store
                .save_remote_connection_settings(0, vec![invalid])
                .is_err());
        }
        let mut invalid = entry();
        invalid.value = Json::Null;
        assert!(store
            .save_remote_connection_settings(0, vec![invalid])
            .is_err());
        let entries = (0..17)
            .map(|n| RemoteConnectionSettings {
                id: format!("{n:064x}"),
                ..entry()
            })
            .collect();
        assert!(store.save_remote_connection_settings(0, entries).is_err());
        assert!(!f.0.join(PENDING).exists());
        assert!(!f.0.join(RECORD).exists());
    }
    #[test]
    fn settings_survive_restart_and_stale_or_duplicate_updates_refuse() {
        let f = Fixture::new();
        let store = f.open();
        assert_eq!(store.load_remote_connection_settings().unwrap().revision, 0);
        let first = store
            .save_remote_connection_settings(0, vec![entry()])
            .unwrap();
        drop(store);
        let store = f.open();
        assert_eq!(store.load_remote_connection_settings().unwrap(), first);
        assert!(store.save_remote_connection_settings(0, vec![]).is_err());
        assert!(store
            .save_remote_connection_settings(1, vec![entry(), entry()])
            .is_err());
        assert_eq!(
            store
                .save_remote_connection_settings(1, vec![entry()])
                .unwrap(),
            first
        );
        assert_eq!(
            store
                .save_remote_connection_settings(1, vec![])
                .unwrap()
                .revision,
            2
        );
    }
    #[test]
    fn pending_state_requires_explicit_recovery_and_never_adopts_wrong_revision() {
        let f = Fixture::new();
        let store = f.open();
        let pending = RemoteConnectionSettingsState {
            revision: 1,
            entries: vec![entry()],
        };
        store
            .pinned
            .filesystem()
            .write_new_file(
                Path::new(PENDING),
                store
                    .connection_record(&pending)
                    .unwrap()
                    .encode()
                    .as_bytes(),
                fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        assert!(store.load_remote_connection_settings().is_err());
        assert!(store.save_remote_connection_settings(0, vec![]).is_err());
        assert_eq!(store.recover_remote_connection_settings().unwrap(), pending);
        let wrong = RemoteConnectionSettingsState {
            revision: 3,
            entries: vec![],
        };
        store
            .pinned
            .filesystem()
            .write_new_file(
                Path::new(PENDING),
                store.connection_record(&wrong).unwrap().encode().as_bytes(),
                fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        assert!(store.recover_remote_connection_settings().is_err());
        assert!(f.0.join(PENDING).exists());
    }
    #[test]
    fn linked_public_and_substituted_catalogues_are_preserved_and_refused() {
        let f = Fixture::new();
        let store = f.open();
        store
            .save_remote_connection_settings(0, vec![entry()])
            .unwrap();
        fs::set_permissions(f.0.join(RECORD), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(store.load_remote_connection_settings().is_err());
        fs::rename(f.0.join(RECORD), f.0.join("retained")).unwrap();
        symlink(f.0.join("retained"), f.0.join(RECORD)).unwrap();
        assert!(store.load_remote_connection_settings().is_err());
        assert!(f.0.join("retained").exists());
        let other = Fixture::new();
        fs::copy(f.0.join("retained"), other.0.join(RECORD)).unwrap();
        fs::set_permissions(other.0.join(RECORD), fs::Permissions::from_mode(0o600)).unwrap();
        assert!(other.open().load_remote_connection_settings().is_err());
    }
}
