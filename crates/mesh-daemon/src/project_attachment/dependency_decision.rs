//! Native control of exact saved-input eligibility. No agent, renderer or CLI caller exists.
use super::{
    dependency_enrollment::read_private_in_store,
    dependency_transaction::{hash, read_payload},
    history::verify_history_binding,
    invalid, ProvisionedAttachment, SavedAttachmentVersion,
};
use crate::{
    ipc::Json,
    root_authority::PinnedRootFs,
    workspace::{OpenWorkspace, RECORD_FILE_NAME},
    TrustedReviewers,
};
use mesh_cas::{Blake3, Cas, DurableFs as _};
use mesh_store::{frame_record, DependencyKind, DependencyRecord, RecordDigest, StoredRecord};
use std::{
    io::{self, Read as _, Seek as _, Write as _},
    os::unix::fs::{MetadataExt as _, PermissionsExt as _},
    path::Path,
};
const ZERO: RecordDigest = RecordDigest::from_bytes([0; 32]);
const PENDING: &str = "dependency-decision.pending";
const MAX_JOURNAL: usize = 80 * 1024 * 1024;

/// Explicit trusted-native-host decision. This is not exposed through actor credentials.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavedInputDecision {
    /// Explicit revalidation of this exact input, not approval of dependent output.
    Eligible,
    /// Refuse new dependent publication; keep retained content and old accepted main.
    Rejected,
    /// Replace the input with another exact saved operation of this native project.
    Replaced(SavedAttachmentVersion),
}
/// Durable facts about one historical control request, not permission to consume or publish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeInputDecision {
    record: RecordDigest,
    revision: u64,
}
impl NativeInputDecision {
    /// Exact retained decision payload.
    pub fn record(&self) -> RecordDigest {
        self.record
    }
    /// Per-input decision revision, separate from the authority journal ordinal.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
fn input(project: RecordDigest, installation: RecordDigest, version: RecordDigest) -> Json {
    Json::Array(vec![
        Json::Array(vec![
            Json::text(project.to_hex()),
            Json::text(installation.to_hex()),
        ]),
        Json::text(version.to_hex()),
    ])
}
fn body(
    project: RecordDigest,
    installation: RecordDigest,
    version: SavedAttachmentVersion,
    decision: SavedInputDecision,
    request: RecordDigest,
    revision: u64,
    previous: RecordDigest,
) -> Json {
    let (state, replacement) = match decision {
        SavedInputDecision::Eligible => ("eligible", Json::Null),
        SavedInputDecision::Rejected => ("rejected", Json::Null),
        SavedInputDecision::Replaced(version) => (
            "replaced",
            input(project, installation, version.operation()),
        ),
    };
    Json::object([
        ("request", Json::text(request.to_hex())),
        ("input", input(project, installation, version.operation())),
        ("revision", Json::Number(revision)),
        ("previous", Json::text(previous.to_hex())),
        ("state", Json::text(state)),
        ("replacement", replacement),
    ])
}
impl ProvisionedAttachment {
    /// Record an explicit native-host input decision. Work identity is the registered root work,
    /// independent of any provider/run. No runtime control interface calls this development API.
    /// Recovery of pending writes must be completed before this primitive is exposed to users.
    pub fn decide_saved_input(
        &self,
        version: SavedAttachmentVersion,
        decision: SavedInputDecision,
        expected_previous: Option<RecordDigest>,
        request: RecordDigest,
    ) -> io::Result<NativeInputDecision> {
        if request == ZERO || expected_previous == Some(ZERO) {
            return Err(invalid("missing decision request or predecessor"));
        }
        let _guard =
            crate::workspace_custody::lock_workspace_initialization(&self.store).map_err(error)?;
        // Unknown pending work is preserved. Exact interrupted-append recovery is integrated next.
        match read_private_in_store(&self.store, PENDING) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
            Ok(_) => {
                return Err(invalid(
                    "native decision requires pending transaction recovery",
                ))
            }
        }
        let (configuration, proof) = self
            .attachment
            .read_configuration(self.metadata_path(), &self.store)?;
        let proof = proof.ok_or_else(|| invalid("native dependency enrollment is required"))?;
        let binding = proof.binding();
        let workspace = OpenWorkspace::open_attachment_read_history(
            self.metadata_path(),
            self.store.clone(),
            &TrustedReviewers::default(),
            Some(&proof),
        )
        .map_err(error)?;
        verify_history_binding(&workspace, &configuration)?;
        let validate = |version: SavedAttachmentVersion| -> io::Result<()> {
            if !workspace
                .workspace_versions()
                .iter()
                .any(|v| v.operation() == version.operation())
            {
                return Err(invalid(
                    "input is not a saved version of this native project",
                ));
            }
            workspace
                .historical_workspace_preview(version.operation())
                .map_err(error)?;
            Ok(())
        };
        validate(version)?;
        if let SavedInputDecision::Replaced(replacement) = decision {
            validate(replacement)?;
        }
        let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
            self.metadata_path(),
            self.store.filesystem(),
        )
        .map_err(error)?;
        let mut journal = self
            .store
            .open_existing_record_file(Path::new(RECORD_FILE_NAME))?;
        journal.rewind()?;
        let mut before = Vec::new();
        (&mut journal)
            .take((MAX_JOURNAL + 1) as u64)
            .read_to_end(&mut before)?;
        if before.len() > MAX_JOURNAL {
            return Err(invalid("decision history exceeds its bound"));
        }
        proof.verify(&self.store, &journal, &before)?;
        let mut policy = proof.policy().clone();
        if let Some(record) = policy.native_request(request) {
            let bytes = read_payload(&cas, record.payload, 65_536)?;
            let value = Json::parse(std::str::from_utf8(&bytes).map_err(error)?).map_err(error)?;
            let previous = expected_previous.unwrap_or(ZERO);
            let revision = value
                .get("body")
                .and_then(|v| v.get("revision"))
                .and_then(Json::as_u64)
                .ok_or_else(|| invalid("request is not an input decision"))?;
            if record.kind != DependencyKind::Eligibility
                || value.get("body")
                    != Some(&body(
                        binding.project,
                        binding.installation,
                        version,
                        decision,
                        request,
                        revision,
                        previous,
                    ))
            {
                return Err(invalid("decision request was reused with different intent"));
            }
            journal.sync_all()?;
            self.store.sync()?;
            return Ok(NativeInputDecision {
                record: record.payload,
                revision,
            });
        }
        let prior =
            policy.native_decision(binding.project, binding.installation, version.operation());
        if prior.map(|(_, p)| p) != expected_previous {
            return Err(invalid("saved-input decision is stale"));
        }
        let revision = prior
            .map_or(Some(1), |(n, _)| n.checked_add(1))
            .ok_or_else(|| invalid("decision revision exhausted"))?;
        let (ordinal, previous) = policy
            .native_head()
            .ok_or_else(|| invalid("missing enrollment"))?;
        let ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| invalid("dependency history exhausted"))?;
        let bytes = Json::object([
            ("schema", Json::text("mesh.dependency-policy/v1")),
            ("authority", Json::text(binding.authority.to_hex())),
            ("revision", Json::Number(ordinal)),
            ("previous", Json::text(previous.to_hex())),
            (
                "kind",
                Json::Number(u64::from(DependencyKind::Eligibility.code())),
            ),
            (
                "body",
                body(
                    binding.project,
                    binding.installation,
                    version,
                    decision,
                    request,
                    revision,
                    expected_previous.unwrap_or(ZERO),
                ),
            ),
        ])
        .encode()
        .into_bytes();
        let record = DependencyRecord {
            authority: binding.authority,
            revision: ordinal,
            previous,
            payload: hash(&bytes),
            kind: DependencyKind::Eligibility,
        };
        policy.apply(record, &bytes).map_err(error)?;
        let identity = journal.metadata()?;
        let pending = Json::object([
            ("schema", Json::text("mesh.dependency-decision-intent/v1")),
            ("request", Json::text(request.to_hex())),
            (
                "journal_device",
                Json::text(format!("{:016x}", identity.dev())),
            ),
            (
                "journal_inode",
                Json::text(format!("{:016x}", identity.ino())),
            ),
            ("journal_bytes", Json::Number(before.len() as u64)),
            ("journal_digest", Json::text(hash(&before).to_hex())),
            ("payload", Json::text(record.payload.to_hex())),
        ])
        .encode();
        cas.promote(bytes.clone()).map_err(error)?;
        if read_payload(&cas, record.payload, 65_536)? != bytes {
            return Err(invalid("decision payload staging changed"));
        }
        self.store.filesystem().write_new_file(
            Path::new(PENDING),
            pending.as_bytes(),
            std::fs::Permissions::from_mode(0o600),
        )?;
        self.store.sync()?;
        let (_, current) = self
            .attachment
            .read_configuration(self.metadata_path(), &self.store)?;
        if current.as_ref() != Some(&proof) {
            return Err(invalid("decision history changed before append"));
        }
        let frame = frame_record(&StoredRecord::Dependency(record));
        journal.write_all(&frame)?;
        journal.sync_all()?;
        let (_, replay) = self
            .attachment
            .read_configuration(self.metadata_path(), &self.store)?;
        if replay
            .as_ref()
            .and_then(|p| p.policy().native_request(request))
            != Some(record)
        {
            return Err(invalid("decision did not replay exactly"));
        }
        self.store.filesystem().remove_file(Path::new(PENDING))?;
        self.store.sync()?;
        Ok(NativeInputDecision {
            record: record.payload,
            revision,
        })
    }
}

#[cfg(test)]
mod tests;
