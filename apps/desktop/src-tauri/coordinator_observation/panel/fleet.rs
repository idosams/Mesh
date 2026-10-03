//! Independent bounded reads selected from retained native fleet and connection records.
use super::*;
use std::collections::BTreeSet;
#[derive(Default)]
pub(super) struct Reads(Mutex<BTreeSet<String>>);
struct Permit<'a>(&'a Reads, String);
impl Reads {
    fn acquire(&self, key: String) -> Result<Permit<'_>, String> {
        let mut active = self.0.try_lock().map_err(|_| BUSY)?;
        if active.len() >= 4 || !active.insert(key.clone()) {
            return Err(BUSY.into());
        }
        Ok(Permit(self, key))
    }
}
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.0 .0.lock() {
            active.remove(&self.1);
        }
    }
}
impl RemotePanel {
    pub(crate) fn read_fleet(
        &self,
        objective: &str,
        lane: &str,
        run: &str,
        assignment: &str,
        host: &crate::attachment_host::AttachmentHost,
    ) -> Result<String, String> {
        eligible()?;
        for part in [objective, lane, run, assignment] {
            if part.is_empty()
                || part.len() > 128
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            {
                return Err(UNAVAILABLE.into());
            }
        }
        let _permit = self.2.acquire(format!("{objective}/{lane}/{run}"))?;
        let history =
            host.configured_remote_history(host.remote_fleet_storage_path(), objective)?;
        let original = history
            .remote_observation_assignment(lane, run)
            .map_err(|_| UNAVAILABLE)?;
        if original.id != assignment {
            return Err(UNAVAILABLE.into());
        }
        let settings = host.connection_settings(false)?;
        let mut matching = settings.entries.iter().filter_map(|entry| {
            let (configuration, bindings) = profiles::decode(entry).ok()?;
            (configuration.objective == objective
                && configuration.lane == lane
                && configuration.run == run
                && configuration
                    .connection
                    .worker
                    .as_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
                    == original.worker_key)
                .then_some((configuration, bindings))
        });
        let (configuration, bindings) = matching
            .next()
            .ok_or("No unique saved connection matches this remote lane")?;
        if matching.next().is_some() {
            return Err("No unique saved connection matches this remote lane".into());
        }
        profiles::verify_paths(&configuration, &bindings)?;
        let mut selected = admit_selection(configuration, host)?;
        if profiles::bindings(&selected) != bindings {
            return Err(UNAVAILABLE.into());
        }
        if host.connection_settings(false)?.revision != settings.revision {
            return Err(UNAVAILABLE.into());
        }
        let observation = selected.read(RemoteObservationKind::Execution)?;
        if history
            .remote_observation_assignment(lane, run)
            .map_err(|_| UNAVAILABLE)?
            != original
            || host.connection_settings(false)?.revision != settings.revision
        {
            return Err(UNAVAILABLE.into());
        }
        Ok(Json::object([
            ("schema", Json::text("mesh.remote-fleet-observation/v1")),
            ("objective", Json::text(objective)),
            ("lane", Json::text(lane)),
            ("run", Json::text(run)),
            ("assignment", Json::text(assignment)),
            ("worker", Json::text(&original.worker_key)),
            (
                "observation",
                Json::parse(&observation).map_err(|_| UNAVAILABLE)?,
            ),
        ])
        .encode())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_reads_bound_capacity_and_duplicate_attempts_until_the_owner_returns() {
        let reads = Reads::default();
        let first = reads.acquire("fleet/lane/one".into()).unwrap();
        assert!(reads.acquire("fleet/lane/one".into()).is_err());
        let others: Vec<_> = (2..=4)
            .map(|n| reads.acquire(format!("fleet/lane/{n}")).unwrap())
            .collect();
        assert!(reads.acquire("fleet/lane/five".into()).is_err());
        drop(first);
        let fifth = reads.acquire("fleet/lane/five".into()).unwrap();
        drop((others, fifth));
        assert!(reads.0.lock().unwrap().is_empty());
    }
    #[test]
    fn fleet_reads_do_not_take_the_selected_worker_or_setup_lock() {
        let panel = RemotePanel::default();
        let _permit = panel.2.acquire("fleet/lane/run".into()).unwrap();
        let _selection = panel.0.try_lock().unwrap();
        let _setup = panel.1.try_lock().unwrap();
        assert!(panel
            .read_fleet(
                "fleet",
                "lane",
                "run",
                "assignment",
                &crate::attachment_host::AttachmentHost::new(Path::new("/unused-application-data"))
            )
            .is_err());
    }
}
