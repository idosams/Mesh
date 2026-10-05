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
    fs::File,
    io::{self, Read as _, Seek as _, Write as _},
    os::unix::fs::{MetadataExt as _, PermissionsExt as _},
    path::Path,
};
const ZERO: RecordDigest = RecordDigest::from_bytes([0; 32]);
pub(super) const PENDING: &str = "dependency-decision.pending";
const MAX_JOURNAL: usize = 80 * 1024 * 1024;

/// Explicit trusted-native-host decision. This is not exposed through actor credentials.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavedInputDecision {
    /// Explicit revalidation of this exact input, not approval of dependent output.
    Eligible,
    /// Refuse new dependent publication; keep retained content and old accepted main.
    Rejected,
    /// Replace the input with another exact saved operation of the same source work.
    Replaced(SavedAttachmentVersion),
}
/// Durable facts about one historical control request, not permission to consume or publish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeInputDecision {
    record: RecordDigest,
    revision: u64,
}
impl NativeInputDecision {
    pub(super) fn from_verified_record(record: RecordDigest, revision: u64) -> Self {
        Self { record, revision }
    }

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
pub(super) fn body(
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
#[derive(Clone, Copy)]
pub(super) enum Step {
    Staged,
    Appended,
}

pub(super) fn transaction_intent(
    kind: DependencyKind,
    request: RecordDigest,
    identity: (u64, u64),
    before: &[u8],
    payload: RecordDigest,
) -> String {
    Json::object([
        (
            "schema",
            Json::text(match kind {
                DependencyKind::Grant => "mesh.dependency-grant-intent/v1",
                DependencyKind::ReviewSnapshot => "mesh.dependency-review-intent/v1",
                DependencyKind::Consumption => "mesh.native-consumption-owner-commit/v1",
                DependencyKind::ConsumptionStart => "mesh.native-consumption-start-intent/v1",
                _ => "mesh.dependency-decision-intent/v1",
            }),
        ),
        ("request", Json::text(request.to_hex())),
        ("journal_device", Json::text(format!("{:016x}", identity.0))),
        ("journal_inode", Json::text(format!("{:016x}", identity.1))),
        ("journal_bytes", Json::Number(before.len() as u64)),
        ("journal_digest", Json::text(hash(before).to_hex())),
        ("payload", Json::text(payload.to_hex())),
    ])
    .encode()
}
/// Native callers construct this only after selecting and validating their exact inputs.
pub(super) struct NativeControlInput {
    pub(super) kind: DependencyKind,
    pub(super) revision_field: &'static str,
    pub(super) body: Json,
    pub(super) prior: Option<(u64, RecordDigest)>,
}
impl NativeControlInput {
    pub(super) fn body_at(&self, revision: u64) -> io::Result<Json> {
        let mut body = self.body.clone();
        let Json::Object(fields) = &mut body else {
            return Err(invalid("invalid native control body"));
        };
        let field = fields
            .iter_mut()
            .find(|(name, _)| name == self.revision_field)
            .ok_or_else(|| invalid("missing native control revision"))?;
        field.1 = Json::Number(revision);
        Ok(body)
    }
}
mod recovery;
pub(super) use recovery::pending_prefix;

impl ProvisionedAttachment {
    /// Record an explicit native-host input decision. Work identity is the registered root work,
    /// independent of any provider/run. No runtime control interface calls this development API.
    /// Interrupted appends resume only their exact recorded intent; unknown work is preserved.
    pub fn decide_saved_input(
        &self,
        version: SavedAttachmentVersion,
        decision: SavedInputDecision,
        expected_previous: Option<RecordDigest>,
        request: RecordDigest,
    ) -> io::Result<NativeInputDecision> {
        self.decide_with_io(
            version,
            decision,
            expected_previous,
            request,
            |_, _, _| Ok(()),
            |file| file.sync_all(),
        )
    }

    fn decide_with_io(
        &self,
        version: SavedAttachmentVersion,
        decision: SavedInputDecision,
        expected_previous: Option<RecordDigest>,
        request: RecordDigest,
        mut hook: impl FnMut(Step, &mut File, &[u8]) -> io::Result<()>,
        mut sync: impl FnMut(&File) -> io::Result<()>,
    ) -> io::Result<NativeInputDecision> {
        self.control_with_io(
            expected_previous,
            request,
            |workspace, proof| {
                let binding = proof.binding();
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
                Ok(NativeControlInput {
                    kind: DependencyKind::Eligibility,
                    revision_field: "revision",
                    body: body(
                        binding.project,
                        binding.installation,
                        version,
                        decision,
                        request,
                        0,
                        expected_previous.unwrap_or(ZERO),
                    ),
                    prior: proof.policy().native_decision(
                        binding.project,
                        binding.installation,
                        version.operation(),
                    ),
                })
            },
            &mut hook,
            &mut sync,
        )
    }

    pub(super) fn control_with_io(
        &self,
        expected_previous: Option<RecordDigest>,
        request: RecordDigest,
        mut select: impl FnMut(
            &OpenWorkspace,
            &super::dependency_read::VerifiedDependencyRead,
        ) -> io::Result<NativeControlInput>,
        mut hook: impl FnMut(Step, &mut File, &[u8]) -> io::Result<()>,
        mut sync: impl FnMut(&File) -> io::Result<()>,
    ) -> io::Result<NativeInputDecision> {
        if request == ZERO || expected_previous == Some(ZERO) {
            return Err(invalid("missing decision request or predecessor"));
        }
        let _guard =
            crate::workspace_custody::lock_workspace_initialization(&self.store).map_err(error)?;
        super::history::dependency_capture::ensure_no_pending_capture(&self.store)?;
        let prior_pending = match read_private_in_store(&self.store, PENDING) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
            Ok(value) => {
                let parsed = Json::parse(&value).map_err(error)?;
                if parsed.get("request").and_then(Json::as_text) != Some(request.to_hex().as_str())
                {
                    return Err(invalid("another native decision requires recovery"));
                }
                Some(value)
            }
        };
        let (configuration, proof) = self.attachment.read_decision_configuration(
            self.metadata_path(),
            &self.store,
            prior_pending.as_deref(),
        )?;
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
        let selected = select(&workspace, &proof)?;
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
        let observed = before.clone();
        if let Some((length, _)) = proof.pending() {
            before.truncate(length);
        }
        let mut policy = proof.policy().clone();
        if let Some(record) = policy.native_request(request) {
            let bytes = read_payload(&cas, record.payload, 65_536)?;
            let value = Json::parse(std::str::from_utf8(&bytes).map_err(error)?).map_err(error)?;
            let revision = value
                .get("body")
                .and_then(|v| v.get(selected.revision_field))
                .and_then(Json::as_u64)
                .ok_or_else(|| invalid("request is not an input decision"))?;
            if record.kind != selected.kind
                || value.get("body") != Some(&selected.body_at(revision)?)
            {
                return Err(invalid("decision request was reused with different intent"));
            }
            sync(&journal)?;
            self.store.sync()?;
            return Ok(NativeInputDecision {
                record: record.payload,
                revision,
            });
        }
        let prior = selected.prior;
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
            (
                "schema",
                Json::text(if selected.kind == DependencyKind::Grant {
                    "mesh.dependency-policy/v2"
                } else if selected.kind == DependencyKind::ReviewSnapshot
                    && selected.body.get("snapshot").is_some()
                {
                    "mesh.dependency-policy/v4"
                } else if selected.kind == DependencyKind::ReviewSnapshot {
                    "mesh.dependency-policy/v3"
                } else {
                    "mesh.dependency-policy/v1"
                }),
            ),
            ("authority", Json::text(binding.authority.to_hex())),
            ("revision", Json::Number(ordinal)),
            ("previous", Json::text(previous.to_hex())),
            ("kind", Json::Number(u64::from(selected.kind.code()))),
            ("body", selected.body_at(revision)?),
        ])
        .encode()
        .into_bytes();
        let record = DependencyRecord {
            authority: binding.authority,
            revision: ordinal,
            previous,
            payload: hash(&bytes),
            kind: selected.kind,
        };
        policy.apply(record, &bytes).map_err(error)?;
        if let Some(graph) = policy.review_graph(record.payload) {
            policy
                .verify_review_graph(record.payload, &read_payload(&cas, graph, 4 * 1024 * 1024)?)
                .map_err(error)?;
        }
        let identity = journal.metadata()?;
        let pending = transaction_intent(
            selected.kind,
            request,
            (identity.dev(), identity.ino()),
            &before,
            record.payload,
        );
        let frame = frame_record(&StoredRecord::Dependency(record));
        if before.len().saturating_add(frame.len()) > MAX_JOURNAL {
            return Err(invalid("decision append exceeds history bound"));
        }
        if let Some(original) = &prior_pending {
            if original != &pending || proof.pending().map(|(_, r)| r) != Some(record) {
                return Err(invalid(
                    "pending decision differs from requested native intent",
                ));
            }
            self.store.filesystem().sync_file(Path::new(PENDING))?;
        } else {
            cas.promote(bytes.clone()).map_err(error)?;
            self.store.filesystem().write_new_file(
                Path::new(PENDING),
                pending.as_bytes(),
                std::fs::Permissions::from_mode(0o600),
            )?;
        }
        if read_payload(&cas, record.payload, 65_536)? != bytes {
            return Err(invalid("decision payload staging changed"));
        }
        self.store.sync()?;
        hook(Step::Staged, &mut journal, &frame)?;
        if read_private_in_store(&self.store, PENDING)? != pending {
            return Err(invalid("decision intent changed before append"));
        }
        let (current_configuration, current_proof) = self.attachment.read_decision_configuration(
            self.metadata_path(),
            &self.store,
            Some(&pending),
        )?;
        let current_proof = current_proof.ok_or_else(|| invalid("decision enrollment changed"))?;
        if current_configuration != configuration
            || current_proof.binding() != binding
            || current_proof.policy() != proof.policy()
            || current_proof.pending().map(|(_, r)| r) != Some(record)
        {
            return Err(invalid("decision authority changed before append"));
        }
        let current_selection = select(&workspace, &current_proof)?;
        if current_selection.kind != selected.kind
            || current_selection.body_at(revision)? != selected.body_at(revision)?
            || current_selection.prior != selected.prior
        {
            return Err(invalid("native control inputs changed before append"));
        }
        journal.rewind()?;
        let mut current = Vec::new();
        (&mut journal)
            .take((MAX_JOURNAL + 1) as u64)
            .read_to_end(&mut current)?;
        proof.verify(&self.store, &journal, &current)?;
        if current != observed {
            return Err(invalid("decision history changed before append"));
        }
        // Recovery has already verified that the only suffix is a prefix of this exact frame.
        let written = observed.len() - before.len();
        if written > frame.len() || !frame.starts_with(&observed[before.len()..]) {
            return Err(invalid("foreign decision suffix"));
        }
        journal.write_all(&frame[written..])?;
        sync(&journal)?;
        hook(Step::Appended, &mut journal, &frame)?;
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
        if read_private_in_store(&self.store, PENDING)? != pending {
            return Err(invalid("decision intent changed after append"));
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
