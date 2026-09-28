//! Exact provider choices persisted with a native allocation, not executable paths or credentials.
use crate::ipc::Json;
use std::collections::BTreeSet;
use std::io;

/// Coordinator provider and the complete set available for this objective's delegated lanes.
/// This closed policy chooses supported integrations; it does not admit installed executables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FleetProviderPolicy {
    pub(super) coordinator: String,
    pub(super) providers: BTreeSet<String>,
}
impl Default for FleetProviderPolicy {
    fn default() -> Self {
        Self {
            coordinator: "codex".into(),
            providers: BTreeSet::from(["codex".into()]),
        }
    }
}
impl FleetProviderPolicy {
    /// Validate exact coordinator/allowed-provider input before any allocation side effect.
    /// Duplicate, unknown or empty entries and an excluded coordinator are refused.
    pub fn new(coordinator: &str, providers: &[String]) -> io::Result<Self> {
        let admitted: BTreeSet<_> = providers.iter().cloned().collect();
        if providers.is_empty()
            || providers.len() > 2
            || admitted.len() != providers.len()
            || !admitted.contains(coordinator)
            || admitted
                .iter()
                .any(|provider| !matches!(provider.as_str(), "codex" | "claude"))
        {
            return Err(super::unavailable());
        }
        Ok(Self {
            coordinator: coordinator.into(),
            providers: admitted,
        })
    }
    /// Fixed provider identity for the coordinator lane.
    pub fn coordinator(&self) -> &str {
        &self.coordinator
    }
    /// Canonically ordered provider identities allowed for delegated lanes.
    pub fn providers(&self) -> impl Iterator<Item = &str> {
        self.providers.iter().map(String::as_str)
    }
    pub(super) fn legacy(&self) -> bool {
        self == &Self::default()
    }
    pub(super) fn decode(value: &Json) -> io::Result<Self> {
        match value.get("schema").and_then(Json::as_text) {
            Some("mesh.native-fleet-allocation/v1") => {
                if value.get("coordinator_provider").is_some() || value.get("providers").is_some() {
                    return Err(super::unavailable());
                }
                Ok(Self::default())
            }
            Some("mesh.native-fleet-allocation/v2") => {
                let coordinator = value
                    .get("coordinator_provider")
                    .and_then(Json::as_text)
                    .ok_or_else(super::unavailable)?;
                let values = value
                    .get("providers")
                    .and_then(Json::as_array)
                    .ok_or_else(super::unavailable)?;
                if values.len() > 2 {
                    return Err(super::unavailable());
                }
                let providers = values
                    .iter()
                    .map(|value| {
                        value
                            .as_text()
                            .map(str::to_owned)
                            .ok_or_else(super::unavailable)
                    })
                    .collect::<io::Result<Vec<_>>>()?;
                let result = Self::new(coordinator, &providers)?;
                // Legacy policy always has its original v1 byte representation.
                if result.legacy() || result.providers().ne(providers.iter().map(String::as_str)) {
                    return Err(super::unavailable());
                }
                Ok(result)
            }
            _ => Err(super::unavailable()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_admits_only_complete_supported_provider_choices() {
        for (coordinator, providers) in [
            ("codex", vec![]),
            ("claude", vec!["codex"]),
            ("other", vec!["other"]),
            ("codex", vec!["codex", "codex"]),
            ("codex", vec!["codex", "../claude"]),
            ("codex", vec!["codex", "claude", "other"]),
        ] {
            assert!(FleetProviderPolicy::new(
                coordinator,
                &providers.into_iter().map(str::to_owned).collect::<Vec<_>>()
            )
            .is_err());
        }
        let mixed = FleetProviderPolicy::new("claude", &["codex".into(), "claude".into()]).unwrap();
        assert_eq!(mixed.coordinator(), "claude");
        assert_eq!(mixed.providers().collect::<Vec<_>>(), ["claude", "codex"]);
        assert!(!mixed.legacy());
    }
    #[test]
    fn receipt_policy_rejects_ambiguous_upgrades_and_noncanonical_sets() {
        let legacy = Json::parse(r#"{"schema":"mesh.native-fleet-allocation/v1"}"#).unwrap();
        assert_eq!(
            FleetProviderPolicy::decode(&legacy).unwrap(),
            FleetProviderPolicy::default()
        );
        for input in [
            r#"{"schema":"mesh.native-fleet-allocation/v1","providers":["claude"]}"#,
            r#"{"schema":"mesh.native-fleet-allocation/v2","coordinator_provider":"claude","providers":["codex","claude"]}"#,
            r#"{"schema":"mesh.native-fleet-allocation/v2","coordinator_provider":"claude","providers":["claude","claude"]}"#,
            r#"{"schema":"mesh.native-fleet-allocation/v2","coordinator_provider":"codex","providers":["codex"]}"#,
            r#"{"schema":"mesh.native-fleet-allocation/v2","coordinator_provider":"claude","providers":[null]}"#,
            r#"{"schema":"mesh.native-fleet-allocation/v3"}"#,
        ] {
            assert!(FleetProviderPolicy::decode(&Json::parse(input).unwrap()).is_err());
        }
        let mixed = Json::parse(r#"{"schema":"mesh.native-fleet-allocation/v2","coordinator_provider":"claude","providers":["claude","codex"]}"#).unwrap();
        assert_eq!(
            FleetProviderPolicy::decode(&mixed)
                .unwrap()
                .providers()
                .count(),
            2
        );
    }
}
