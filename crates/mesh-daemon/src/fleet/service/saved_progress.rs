//! Immutable ordinary-progress reads, independent of explicit handoff reviews and execution.
use super::*;
const PAGE_SIZE: usize = 50;

fn version_id(value: &str) -> Result<RecordDigest, Unavailable> {
    let version =
        RecordDigest::parse_hex(value).map_err(|_| refusal("fleet-progress-version-invalid"))?;
    if version.to_string() != value {
        return Err(refusal("fleet-progress-version-invalid"));
    }
    Ok(version)
}

impl FleetHistory {
    /// Counts for one exact retained version, with no entry names, content or execution adoption.
    pub fn saved_progress_summary(&self, lane: &str, version: &str) -> Result<Json, Unavailable> {
        self.0.saved_progress_summary(lane, version)
    }

    /// Page retained operations without acquiring a worker or creating a handoff checkpoint.
    pub fn saved_progress_versions(
        &self,
        lane: &str,
        after: Option<&str>,
    ) -> Result<Json, Unavailable> {
        self.0.saved_progress_versions(lane, after)
    }

    /// Compare exact retained progress with its original local input, without review authority.
    pub fn saved_progress_comparison(
        &self,
        lane: &str,
        version: &str,
        after: Option<&str>,
        selected: Option<&str>,
    ) -> Result<Json, Unavailable> {
        self.0
            .saved_progress_comparison(lane, version, after, selected)
    }
}

impl FleetService {
    /// Retained operations can include partial capture progress; listing is not handoff completeness.
    /// Cursors follow causal operation order. Refresh to discover newly inserted earlier operations.
    pub fn saved_progress_versions(
        &self,
        lane: &str,
        after: Option<&str>,
    ) -> Result<Json, Unavailable> {
        let cursor = after.map(version_id).transpose()?;
        self.with_progress_history(lane, "mesh.fleet-saved-progress-page/v1", |open, _| {
            let versions = open.workspace_versions();
            let start = match cursor {
                None => 0,
                Some(cursor) => {
                    versions
                        .iter()
                        .position(|v| v.operation() == cursor)
                        .ok_or_else(|| refusal("fleet-progress-cursor-invalid"))?
                        + 1
                }
            };
            let end = (start + PAGE_SIZE).min(versions.len());
            Ok(Json::object([
                ("order", Json::text("causal-operation")),
                ("after", after.map_or(Json::Null, Json::text)),
                ("total", Json::Number(versions.len() as u64)),
                (
                    "versions",
                    Json::Array(
                        versions[start..end]
                            .iter()
                            .map(|version| {
                                Json::object([
                                    ("version", Json::text(version.operation().to_string())),
                                    ("ordinal", Json::Number(version.ordinal())),
                                ])
                            })
                            .collect(),
                    ),
                ),
                (
                    "next_after",
                    if end < versions.len() {
                        Json::text(versions[end - 1].operation().to_string())
                    } else {
                        Json::Null
                    },
                ),
            ]))
        })
    }

    /// Read exact saved bytes even after later saves or session revocation. Working files are ignored.
    pub fn saved_progress_comparison(
        &self,
        lane: &str,
        version: &str,
        after: Option<&str>,
        selected: Option<&str>,
    ) -> Result<Json, Unavailable> {
        let version = version_id(version)?;
        self.with_progress_history(
            lane,
            "mesh.fleet-saved-progress-comparison/v1",
            |open, binding| {
                if !open
                    .workspace_versions()
                    .iter()
                    .any(|entry| entry.operation() == version)
                {
                    return Err(refusal("fleet-progress-version-unavailable"));
                }
                super::super::comparison::compare(
                    open,
                    binding
                        .starting_version()
                        .ok_or_else(|| refusal("fleet-starting-version-unbound"))?,
                    version,
                    after,
                    selected,
                )
            },
        )
    }

    /// Aggregate only; unknown history and substituted custody still refuse through the shared read.
    pub fn saved_progress_summary(&self, lane: &str, version: &str) -> Result<Json, Unavailable> {
        let version = version_id(version)?;
        self.with_progress_history(
            lane,
            "mesh.fleet-saved-progress-summary/v1",
            |open, binding| {
                if !open
                    .workspace_versions()
                    .iter()
                    .any(|entry| entry.operation() == version)
                {
                    return Err(refusal("fleet-progress-version-unavailable"));
                }
                super::super::comparison::summarize(
                    open,
                    binding
                        .starting_version()
                        .ok_or_else(|| refusal("fleet-starting-version-unbound"))?,
                    version,
                )
            },
        )
    }

    fn with_progress_history(
        &self,
        lane: &str,
        schema: &'static str,
        read: impl FnOnce(
            &crate::workspace::OpenWorkspace,
            &super::super::WorkspaceBinding,
        ) -> Result<Json, Unavailable>,
    ) -> Result<Json, Unavailable> {
        super::super::id_valid(lane).map_err(runtime_error)?;
        let (binding, objective, revision, latest) = {
            let mut inner = self.lock()?;
            inner.runtime.refresh().map_err(runtime_error)?;
            let state = inner.runtime.state();
            let entry = state
                .lanes
                .get(lane)
                .ok_or_else(|| refusal("fleet-lane-missing"))?;
            let binding = entry
                .workspace
                .clone()
                .ok_or_else(|| refusal("fleet-progress-workspace-unavailable"))?;
            (
                binding,
                inner.runtime.objective().to_owned(),
                state.revision,
                entry.saved,
            )
        };
        // Reopening a verified journal never attaches an execution context or starts capture timers.
        // All history I/O is outside the shared fleet mutex.
        let history = self.allocator.reopen_history(lane, &binding)?;
        history.verify()?;
        let progress = read(&history.open, &binding)?;
        history.verify()?;
        let mut inner = self.lock()?;
        inner.runtime.refresh().map_err(runtime_error)?;
        if inner
            .runtime
            .state()
            .lanes
            .get(lane)
            .and_then(|entry| entry.workspace.as_ref())
            != Some(&binding)
        {
            return Err(refusal("fleet-progress-workspace-changed"));
        }
        Ok(Json::object([
            ("schema", Json::text(schema)),
            ("objective", Json::text(objective)),
            ("lane", Json::text(lane)),
            ("revision", Json::Number(revision)),
            (
                "source_version",
                Json::text(binding.source_version.to_string()),
            ),
            (
                "starting_version",
                binding
                    .starting_version()
                    .map_or(Json::Null, |v| Json::text(v.to_string())),
            ),
            (
                "latest_acknowledged_version",
                latest.map_or(Json::Null, |v| Json::text(v.to_string())),
            ),
            ("progress", progress),
            ("handoff_authority", Json::Bool(false)),
            ("approval_authority", Json::Bool(false)),
        ]))
    }
}
