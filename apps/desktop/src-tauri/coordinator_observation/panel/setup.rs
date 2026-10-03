//! Native file selections and closed non-path form inputs. No trust or credentials are created.
use super::*;
use std::fs::Metadata;
use std::os::unix::fs::MetadataExt as _;
#[derive(Clone, Copy)]
pub(crate) enum Part {
    Installation,
    Identity,
    Hosts,
}
impl Part {
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "installation" => Ok(Self::Installation),
            "identity" => Ok(Self::Identity),
            "hosts" => Ok(Self::Hosts),
            _ => Err(UNAVAILABLE.into()),
        }
    }
    pub(crate) fn is_directory(self) -> bool {
        matches!(self, Self::Installation)
    }
}
struct BoundPath {
    path: PathBuf,
    metadata: Metadata,
    parent: ProtectedWorkspaceRoot,
    directory: bool,
}
impl BoundPath {
    fn capture(path: &Path, directory: bool) -> Result<Self, String> {
        if !path.is_absolute() {
            return Err(UNAVAILABLE.into());
        }
        let parent = ProtectedWorkspaceRoot::inspect(path.parent().ok_or(UNAVAILABLE)?)
            .map_err(|_| UNAVAILABLE)?;
        let metadata = std::fs::symlink_metadata(path).map_err(|_| UNAVAILABLE)?;
        if metadata.file_type().is_symlink()
            || (directory && !metadata.is_dir())
            || (!directory && !metadata.is_file())
        {
            return Err(UNAVAILABLE.into());
        }
        if !directory
            && (metadata.nlink() != 1
                || metadata.mode() & 0o077 != 0
                || metadata.len() == 0
                || metadata.len() > 1_048_576)
        {
            return Err(UNAVAILABLE.into());
        }
        // Full owner/SSH-policy admission occurs again before a connection selection is installed.
        let bound = Self {
            path: path.to_owned(),
            metadata,
            parent,
            directory,
        };
        bound.verify()?;
        Ok(bound)
    }
    fn verify(&self) -> Result<(), String> {
        let current = std::fs::symlink_metadata(&self.path).map_err(|_| UNAVAILABLE)?;
        let old = &self.metadata;
        if current.dev() != old.dev()
            || current.ino() != old.ino()
            || current.mode() != old.mode()
            || current.uid() != old.uid()
            || current.nlink() != old.nlink()
            || (!self.directory
                && (current.len() != old.len()
                    || current.mtime() != old.mtime()
                    || current.mtime_nsec() != old.mtime_nsec()
                    || current.ctime() != old.ctime()
                    || current.ctime_nsec() != old.ctime_nsec()))
            || ProtectedWorkspaceRoot::inspect(self.path.parent().ok_or(UNAVAILABLE)?)
                .map_err(|_| UNAVAILABLE)?
                != self.parent
        {
            return Err(UNAVAILABLE.into());
        }
        Ok(())
    }
}
#[derive(Default)]
pub(super) struct Draft {
    id: String,
    installation: Option<BoundPath>,
    identity: Option<BoundPath>,
    hosts: Option<BoundPath>,
}
impl Draft {
    fn clear(&mut self) -> Result<String, String> {
        let mut bytes = [0; 32];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| UNAVAILABLE)?;
        *self = Self {
            id: bytes.iter().map(|b| format!("{b:02x}")).collect(),
            ..Self::default()
        };
        Ok(Json::object([
            ("schema", Json::text("mesh.remote-setup-draft/v1")),
            ("id", Json::text(&self.id)),
            ("installation", Json::Bool(false)),
            ("identity", Json::Bool(false)),
            ("hosts", Json::Bool(false)),
        ])
        .encode())
    }
    fn pick(&mut self, expected: &str, part: Part, path: &Path) -> Result<String, String> {
        if self.id != expected {
            return Err(UNAVAILABLE.into());
        }
        let selected = BoundPath::capture(path, part.is_directory())?;
        let mut random = [0; 32];
        SystemRandom::new()
            .fill(&mut random)
            .map_err(|_| UNAVAILABLE)?;
        let id = random.iter().map(|b| format!("{b:02x}")).collect();
        match part {
            Part::Installation => self.installation = Some(selected),
            Part::Identity => self.identity = Some(selected),
            Part::Hosts => self.hosts = Some(selected),
        }
        self.id = id;
        Ok(Json::object([
            ("schema", Json::text("mesh.remote-setup-draft/v1")),
            ("id", Json::text(&self.id)),
            ("installation", Json::Bool(self.installation.is_some())),
            ("identity", Json::Bool(self.identity.is_some())),
            ("hosts", Json::Bool(self.hosts.is_some())),
        ])
        .encode())
    }
    fn configuration(
        &self,
        expected: &str,
        input: &str,
        fleets: &Path,
    ) -> Result<Configuration, String> {
        if self.id.is_empty() || self.id != expected || input.len() > 4096 {
            return Err(UNAVAILABLE.into());
        }
        let (installation, identity, hosts) = (
            self.installation.as_ref().ok_or(UNAVAILABLE)?,
            self.identity.as_ref().ok_or(UNAVAILABLE)?,
            self.hosts.as_ref().ok_or(UNAVAILABLE)?,
        );
        for path in [installation, identity, hosts] {
            path.verify()?;
        }
        let value = Json::parse(input).map_err(|_| UNAVAILABLE)?;
        let fields = [
            "host",
            "account",
            "port",
            "worker",
            "objective",
            "lane",
            "run",
        ];
        receiving::closed(&value, &fields)?;
        let mut pairs = fields
            .into_iter()
            .map(|field| (field, value.get(field).expect("closed fields").clone()))
            .collect::<Vec<_>>();
        pairs.extend([
            (
                "schema",
                Json::text("mesh.coordinator-observation-config/v1"),
            ),
            (
                "installation",
                Json::text(installation.path.to_str().ok_or(UNAVAILABLE)?),
            ),
            (
                "identity",
                Json::text(identity.path.to_str().ok_or(UNAVAILABLE)?),
            ),
            (
                "known_hosts",
                Json::text(hosts.path.to_str().ok_or(UNAVAILABLE)?),
            ),
            ("fleets", Json::text(fleets.to_str().ok_or(UNAVAILABLE)?)),
        ]);
        config(Json::object(pairs))
    }
}
impl RemotePanel {
    pub(crate) fn clear_setup(&self) -> Result<String, String> {
        self.1.try_lock().map_err(|_| BUSY)?.clear()
    }
    pub(crate) fn pick_setup(
        &self,
        expected: &str,
        part: Part,
        path: &Path,
    ) -> Result<String, String> {
        eligible()?;
        self.1
            .try_lock()
            .map_err(|_| BUSY)?
            .pick(expected, part, path)
    }
    pub(crate) fn configure(
        &self,
        expected: &str,
        input: &str,
        host: &crate::attachment_host::AttachmentHost,
    ) -> Result<String, String> {
        eligible()?;
        let draft = self.1.try_lock().map_err(|_| BUSY)?;
        let configuration =
            draft.configuration(expected, input, host.remote_fleet_storage_path())?;
        let mut selected = self.0.try_lock().map_err(|_| BUSY)?;
        let next = admit_selection(configuration, host)?;
        // A native open may race a changed picker selection: revalidate original bindings again.
        draft.configuration(expected, input, host.remote_fleet_storage_path())?;
        let reply = next.render().encode();
        *selected = Some(next);
        Ok(reply)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt as _};
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "mesh-remote-setup-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            std::fs::create_dir(root.join("installation")).unwrap();
            for name in ["identity", "hosts"] {
                let path = root.join(name);
                std::fs::write(&path, b"placeholder, not a credential").unwrap();
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
            Self(root)
        }
        fn draft(&self) -> Draft {
            let mut draft = Draft::default();
            for (part, name) in [
                (Part::Installation, "installation"),
                (Part::Identity, "identity"),
                (Part::Hosts, "hosts"),
            ] {
                draft
                    .pick(&draft.id.clone(), part, &self.0.join(name))
                    .unwrap();
            }
            draft
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn input() -> String {
        format!(
            r#"{{"host":"worker.example","account":"mesh","port":22,"worker":"{}","objective":"fleet-one","lane":"lane-one","run":"run-one"}}"#,
            "a".repeat(64)
        )
    }
    #[test]
    fn clearing_rotates_the_draft_and_refuses_a_late_picker_reply() {
        let f = Fixture::new();
        let mut draft = f.draft();
        let old = draft.id.clone();
        let reply = Json::parse(&draft.clear().unwrap()).unwrap();
        assert_ne!(draft.id, old);
        assert!(matches!(reply.get("identity"), Some(Json::Bool(false))));
        assert!(draft
            .pick(&old, Part::Identity, &f.0.join("identity"))
            .is_err());
        let fresh = draft.id.clone();
        assert!(draft
            .pick(&fresh, Part::Identity, &f.0.join("identity"))
            .is_ok());
    }
    #[test]
    fn unsigned_setup_cannot_open_chosen_paths_or_apply_renderer_fields() {
        let panel = RemotePanel::default();
        let host = crate::attachment_host::AttachmentHost::new(Path::new("/unused-app"));
        assert_eq!(
            panel
                .pick_setup("", Part::Identity, Path::new("/unused-key"))
                .unwrap_err(),
            ELIGIBILITY
        );
        assert_eq!(
            panel.configure("", &input(), &host).unwrap_err(),
            ELIGIBILITY
        );
    }
    #[test]
    fn form_uses_only_bound_native_paths_and_rejects_unknown_path_fields() {
        let f = Fixture::new();
        let draft = f.draft();
        let config = draft
            .configuration(&draft.id, &input(), &f.0.join("fleets"))
            .unwrap();
        assert_eq!(config.connection.identity, f.0.join("identity"));
        assert_eq!(config.connection.fleets, f.0.join("fleets"));
        assert!(draft.configuration("stale", &input(), &f.0).is_err());
        assert!(draft
            .configuration(
                &draft.id,
                &input().replace("\"host\":", "\"identity\":\"/forged\",\"host\":"),
                &f.0
            )
            .is_err());
        assert!(Draft::default().configuration("", &input(), &f.0).is_err());
    }
    #[test]
    fn changed_files_and_replaced_installations_cannot_reuse_picker_consent() {
        let f = Fixture::new();
        let draft = f.draft();
        std::fs::write(f.0.join("identity"), b"changed").unwrap();
        assert!(draft.configuration(&draft.id, &input(), &f.0).is_err());
        let draft = f.draft();
        std::fs::rename(f.0.join("installation"), f.0.join("old-installation")).unwrap();
        std::fs::create_dir(f.0.join("installation")).unwrap();
        assert!(draft.configuration(&draft.id, &input(), &f.0).is_err());
    }
    #[test]
    fn stale_picker_and_unsafe_file_refusal_preserve_the_previous_draft() {
        let f = Fixture::new();
        let mut draft = f.draft();
        let original = draft.id.clone();
        assert!(draft
            .pick("stale", Part::Identity, &f.0.join("identity"))
            .is_err());
        symlink(f.0.join("identity"), f.0.join("link")).unwrap();
        assert!(draft
            .pick(&original, Part::Identity, &f.0.join("link"))
            .is_err());
        std::fs::set_permissions(f.0.join("identity"), std::fs::Permissions::from_mode(0o644))
            .unwrap();
        assert!(draft
            .pick(&original, Part::Identity, &f.0.join("identity"))
            .is_err());
        assert_eq!(draft.id, original);
        assert!(Part::parse("/renderer/path").is_err());
    }
}
