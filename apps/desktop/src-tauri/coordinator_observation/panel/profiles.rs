//! Explicit saved-settings operations. A persisted record never reconnects or grants execution.
use super::*;
use mesh_daemon::project_attachment::{RemoteConnectionSettings, RemoteConnectionSettingsState};
fn private_configuration(c: &Configuration) -> Json {
    let p = &c.connection;
    Json::object([
        (
            "schema",
            Json::text("mesh.coordinator-observation-config/v1"),
        ),
        ("installation", Json::text(p.installation.to_string_lossy())),
        ("fleets", Json::text(p.fleets.to_string_lossy())),
        ("identity", Json::text(p.identity.to_string_lossy())),
        ("known_hosts", Json::text(p.known_hosts.to_string_lossy())),
        ("host", Json::text(&p.host)),
        ("account", Json::text(&p.account)),
        ("port", Json::Number(p.port.into())),
        ("worker", Json::text(hex(p.worker))),
        ("objective", Json::text(&c.objective)),
        ("lane", Json::text(&c.lane)),
        ("run", Json::text(&c.run)),
    ])
}
fn hex(key: PublicKey) -> String {
    key.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}
fn public_input(c: &Configuration) -> Json {
    Json::object([
        ("host", Json::text(&c.connection.host)),
        ("account", Json::text(&c.connection.account)),
        ("port", Json::text(c.connection.port.to_string())),
        ("worker", Json::text(hex(c.connection.worker))),
        ("objective", Json::text(&c.objective)),
        ("lane", Json::text(&c.lane)),
        ("run", Json::text(&c.run)),
    ])
}
pub(super) fn bindings(s: &Selection) -> Json {
    Json::object([
        ("installation", Json::text(s.installation.directory_token())),
        ("fleets", Json::text(s.fleets.directory_token())),
        ("coordinator", Json::text(hex(s.coordinator))),
        ("identity_file", s.identity_file.fingerprint()),
        ("hosts_file", s.hosts_file.fingerprint()),
    ])
}
pub(super) fn value(s: &Selection) -> Result<Json, String> {
    s.verify()?;
    Ok(Json::object([
        ("schema", Json::text("mesh.native-saved-connection/v1")),
        ("configuration", private_configuration(&s.config)),
        ("bindings", bindings(s)),
    ]))
}
pub(super) fn decode(entry: &RemoteConnectionSettings) -> Result<(Configuration, Json), String> {
    receiving::closed(&entry.value, &["schema", "configuration", "bindings"])?;
    if entry.value.get("schema").and_then(Json::as_text) != Some("mesh.native-saved-connection/v1")
    {
        return Err(UNAVAILABLE.into());
    }
    let b = entry.value.get("bindings").ok_or(UNAVAILABLE)?.clone();
    receiving::closed(
        &b,
        &[
            "installation",
            "fleets",
            "coordinator",
            "identity_file",
            "hosts_file",
        ],
    )?;
    Ok((
        config(entry.value.get("configuration").ok_or(UNAVAILABLE)?.clone())?,
        b,
    ))
}
pub(super) fn verify_paths(c: &Configuration, b: &Json) -> Result<(), String> {
    let installation =
        ProtectedWorkspaceRoot::inspect(&c.connection.installation).map_err(|_| UNAVAILABLE)?;
    let fleets = ProtectedWorkspaceRoot::inspect(&c.connection.fleets).map_err(|_| UNAVAILABLE)?;
    if b.get("installation") != Some(&Json::text(installation.directory_token()))
        || b.get("fleets") != Some(&Json::text(fleets.directory_token()))
        || b.get("identity_file")
            != Some(&setup::BoundPath::capture(&c.connection.identity, false)?.fingerprint())
        || b.get("hosts_file")
            != Some(&setup::BoundPath::capture(&c.connection.known_hosts, false)?.fingerprint())
    {
        return Err(UNAVAILABLE.into());
    }
    Ok(())
}
fn projection(state: &RemoteConnectionSettingsState) -> Result<Json, String> {
    let entries = state
        .entries
        .iter()
        .map(|entry| {
            let (c, _) = decode(entry)?;
            Ok(Json::object([
                ("id", Json::text(&entry.id)),
                ("label", Json::text(&entry.label)),
                ("host", Json::text(c.connection.host)),
                ("worker", Json::text(hex(c.connection.worker))),
                ("objective", Json::text(c.objective)),
                ("lane", Json::text(c.lane)),
                ("run", Json::text(c.run)),
            ]))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Json::object([
        ("schema", Json::text("mesh.remote-connection-profiles/v1")),
        ("revision", Json::text(state.revision.to_string())),
        ("entries", Json::Array(entries)),
    ]))
}
fn revision(text: &str) -> Result<u64, String> {
    let n: u64 = text.parse().map_err(|_| UNAVAILABLE)?;
    if n.to_string() != text {
        return Err(UNAVAILABLE.into());
    }
    Ok(n)
}
fn id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl RemotePanel {
    pub(crate) fn profiles(
        &self,
        action: &str,
        selection: &str,
        label: &str,
        profile: &str,
        expected: &str,
        host: &crate::attachment_host::AttachmentHost,
    ) -> Result<String, String> {
        eligible()?;
        match action {
            "list" | "recover"
                if selection.is_empty()
                    && label.is_empty()
                    && profile.is_empty()
                    && expected.is_empty() =>
            {
                Ok(projection(&host.connection_settings(action == "recover")?)?.encode())
            }
            "save" if id(selection) && profile.is_empty() => {
                let revision = revision(expected)?;
                let mut held = self.0.try_lock().map_err(|_| BUSY)?;
                let selected = held
                    .as_mut()
                    .filter(|s| s.id == selection)
                    .ok_or(UNAVAILABLE)?;
                let data = value(selected)?;
                let mut state = host.connection_settings(false)?;
                if state.revision != revision {
                    return Err(UNAVAILABLE.into());
                }
                let profile = selected
                    .profile_source
                    .as_ref()
                    .unwrap_or(&selected.id)
                    .clone();
                let entry = RemoteConnectionSettings {
                    id: profile.clone(),
                    label: label.into(),
                    value: data,
                };
                if let Some(old) = state.entries.iter_mut().find(|e| e.id == profile) {
                    *old = entry;
                } else {
                    state.entries.push(entry);
                }
                let saved = host.save_connection_settings(revision, state.entries)?;
                selected.profile_source = Some(profile);
                Ok(projection(&saved)?.encode())
            }
            "remove" | "open" if id(profile) && selection.is_empty() && label.is_empty() => {
                let revision = revision(expected)?;
                let mut state = host.connection_settings(false)?;
                if state.revision != revision {
                    return Err(UNAVAILABLE.into());
                }
                let entry = state
                    .entries
                    .iter()
                    .find(|e| e.id == profile)
                    .ok_or(UNAVAILABLE)?
                    .clone();
                if action == "remove" {
                    state.entries.retain(|e| e.id != profile);
                    return Ok(projection(
                        &host.save_connection_settings(revision, state.entries)?,
                    )?
                    .encode());
                }
                let (configuration, original) = decode(&entry)?;
                verify_paths(&configuration, &original)?;
                // Match configure's lock order. No I/O or invocation is repeated on presentation failure.
                let mut draft = self.1.try_lock().map_err(|_| BUSY)?;
                let mut held = self.0.try_lock().map_err(|_| BUSY)?;
                let mut selected = admit_selection(configuration, host)?;
                if bindings(&selected) != original {
                    return Err(UNAVAILABLE.into());
                }
                let next_draft = setup::Draft::reopen(&selected.config, profile)?;
                selected.verify()?;
                if host.connection_settings(false)?.revision != revision {
                    return Err(UNAVAILABLE.into());
                }
                selected.profile_source = Some(profile.into());
                let reply = Json::object([
                    ("schema", Json::text("mesh.remote-profile-open/v1")),
                    ("profile", Json::text(profile)),
                    ("label", Json::text(&entry.label)),
                    ("selection", selected.render()),
                    ("draft", next_draft.projection()),
                    ("input", public_input(&selected.config)),
                ])
                .encode();
                *draft = next_draft;
                *held = Some(selected);
                Ok(reply)
            }
            _ => Err(UNAVAILABLE.into()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_path_admission_accepts_originals_and_refuses_changed_trust_before_custody() {
        use std::os::unix::fs::PermissionsExt as _;
        let root =
            std::env::temp_dir().join(format!("mesh-saved-path-admission-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        for name in ["installation", "fleets"] {
            std::fs::create_dir(root.join(name)).unwrap();
        }
        for name in ["identity", "hosts"] {
            std::fs::write(root.join(name), b"placeholder, not credential material").unwrap();
            std::fs::set_permissions(root.join(name), std::fs::Permissions::from_mode(0o600))
                .unwrap();
        }
        let input = Json::object([
            (
                "schema",
                Json::text("mesh.coordinator-observation-config/v1"),
            ),
            (
                "installation",
                Json::text(root.join("installation").to_string_lossy()),
            ),
            ("fleets", Json::text(root.join("fleets").to_string_lossy())),
            (
                "identity",
                Json::text(root.join("identity").to_string_lossy()),
            ),
            (
                "known_hosts",
                Json::text(root.join("hosts").to_string_lossy()),
            ),
            ("host", Json::text("worker.example")),
            ("account", Json::text("mesh")),
            ("port", Json::Number(22)),
            ("worker", Json::text("a".repeat(64))),
            ("objective", Json::text("fleet-one")),
            ("lane", Json::text("lane-one")),
            ("run", Json::text("run-one")),
        ]);
        let c = config(input).unwrap();
        let bound = Json::object([
            (
                "installation",
                Json::text(
                    ProtectedWorkspaceRoot::inspect(&c.connection.installation)
                        .unwrap()
                        .directory_token(),
                ),
            ),
            (
                "fleets",
                Json::text(
                    ProtectedWorkspaceRoot::inspect(&c.connection.fleets)
                        .unwrap()
                        .directory_token(),
                ),
            ),
            (
                "identity_file",
                setup::BoundPath::capture(&c.connection.identity, false)
                    .unwrap()
                    .fingerprint(),
            ),
            (
                "hosts_file",
                setup::BoundPath::capture(&c.connection.known_hosts, false)
                    .unwrap()
                    .fingerprint(),
            ),
        ]);
        let persisted = Json::parse(&bound.encode()).unwrap();
        verify_paths(&c, &persisted).unwrap();
        std::fs::rename(root.join("hosts"), root.join("retained-hosts")).unwrap();
        std::fs::write(root.join("hosts"), b"placeholder, not credential material").unwrap();
        std::fs::set_permissions(root.join("hosts"), std::fs::Permissions::from_mode(0o600))
            .unwrap();
        assert!(verify_paths(&c, &persisted).is_err());
        assert!(root.join("retained-hosts").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unsigned_profile_commands_refuse_before_storage_or_paths() {
        let panel = RemotePanel::default();
        let host = crate::attachment_host::AttachmentHost::new(Path::new("/unused-profile-store"));
        for action in ["list", "recover", "save", "remove", "open"] {
            assert_eq!(
                panel.profiles(action, "", "", "", "", &host).unwrap_err(),
                ELIGIBILITY
            );
        }
    }
    #[test]
    fn profile_revision_is_canonical_and_public_projection_has_no_private_paths() {
        for r in ["01", "-1", "18446744073709551616", ""] {
            assert!(revision(r).is_err());
        }
        assert_eq!(revision("12").unwrap(), 12);
        let c=config(Json::parse(&format!(r#"{{"schema":"mesh.coordinator-observation-config/v1","installation":"/private/installation","fleets":"/private/fleets","identity":"/private/key","known_hosts":"/private/trust","host":"worker.example","account":"mesh","port":22,"worker":"{}","objective":"fleet-one","lane":"lane-one","run":"run-one"}}"#, "a".repeat(64))).unwrap()).unwrap();
        let data = Json::object([
            ("schema", Json::text("mesh.native-saved-connection/v1")),
            ("configuration", private_configuration(&c)),
            (
                "bindings",
                Json::object([
                    ("installation", Json::Null),
                    ("fleets", Json::Null),
                    ("coordinator", Json::Null),
                    ("identity_file", Json::Null),
                    ("hosts_file", Json::Null),
                ]),
            ),
        ]);
        let state = RemoteConnectionSettingsState {
            revision: 1,
            entries: vec![RemoteConnectionSettings {
                id: "b".repeat(64),
                label: "Worker".into(),
                value: data,
            }],
        };
        let public = projection(&state).unwrap().encode();
        assert!(!public.contains("/private"));
        assert!(!public.contains("known_hosts"));
        assert!(public.contains("worker.example"));
        assert!(!public_input(&c).encode().contains("/private"));
    }
}
