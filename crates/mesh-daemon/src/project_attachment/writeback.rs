//! Explicit native file integration; capture and read-only previews cannot invoke this path.
use super::inspection::comparison_entries;
use super::{external_store, invalid, ObservationLimits, ProvisionedAttachment};
use crate::ipc::Json;
use crate::managed_file::retained_replacement::{read_target, RetainedReplacement};
use crate::root_authority::PinnedWorkspaceRoot;
use crate::workspace::OpenWorkspace;
use crate::TrustedReviewers;
use mesh_types::{Blake3, ContentDigest as _};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

/// A native-only, single-use proposal. Preparation never changes the source. The host must obtain
/// explicit permission to apply this exact proposal; accepted Mesh main alone is not that permission.
/// No renderer, daemon IPC or agent tool exposes this type or its apply operation.
pub struct PreparedMainFileIntegration {
    history: ProvisionedAttachment,
    store: PinnedWorkspaceRoot,
    recovery: PinnedWorkspaceRoot,
    recovery_path: PathBuf,
    receipt: Json,
    replacement: RetainedReplacement,
    limits: ObservationLimits,
    bundle: String,
    target: String,
    head: String,
    exclusions: String,
}

fn error(value: impl std::fmt::Display) -> io::Error {
    io::Error::other(value.to_string())
}
fn digest(bytes: &[u8]) -> String {
    Blake3::digest_bytes(bytes).to_string()
}
fn approved_file(
    workspace: &OpenWorkspace,
    bundle: &str,
    target: &str,
    relative: &str,
    maximum_bytes: u64,
) -> io::Result<(String, Vec<u8>, bool, String, bool)> {
    let head = super::approval::main_head(workspace)?
        .ok_or_else(|| invalid("Mesh main has no approved version"))?;
    let review = workspace
        .accepted_main_review()
        .map_err(error)?
        .ok_or_else(|| invalid("accepted review unavailable"))?;
    if review.bundle.to_string() != bundle || review.subject_operation.to_string() != target {
        return Err(invalid("Mesh main changed; refresh its exact review"));
    }
    let base = workspace
        .human_approval_context(&review)
        .map_err(error)?
        .expected_canonical_head();
    if base == crate::publication::GENESIS_SHARED_HEAD {
        return Err(invalid(
            "file replacement requires an existing approved base",
        ));
    }
    let base_entries = comparison_entries(
        workspace
            .historical_workspace_preview(workspace.review_target_for_head(base).map_err(error)?)
            .map_err(error)?,
    );
    let proposed = comparison_entries(
        workspace
            .historical_workspace_preview(review.subject_operation)
            .map_err(error)?,
    );
    let before = base_entries
        .get(relative)
        .filter(|entry| entry.kind == "file")
        .ok_or_else(|| invalid("replacement base is not a saved regular file"))?;
    let after = proposed
        .get(relative)
        .filter(|entry| entry.kind == "file")
        .ok_or_else(|| invalid("replacement result is not a saved regular file"))?;
    if before == after || after.bytes.is_none_or(|size| size > maximum_bytes) {
        return Err(invalid("replacement is unchanged or exceeds its budget"));
    }
    let file = workspace
        .historical_workspace_file(review.subject_operation, relative)
        .map_err(error)?
        .ok_or_else(|| invalid("approved file unavailable"))?;
    Ok((
        head.to_string(),
        file.bytes,
        file.executable,
        before
            .digest
            .ok_or_else(|| invalid("base digest unavailable"))?
            .to_string(),
        before
            .executable
            .ok_or_else(|| invalid("base mode unavailable"))?,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    history: ProvisionedAttachment,
    store: PinnedWorkspaceRoot,
    bundle: &str,
    target: &str,
    relative: &str,
    recovery_root: &Path,
    trusted: &TrustedReviewers,
    limits: ObservationLimits,
) -> io::Result<PreparedMainFileIntegration> {
    limits.validate()?;
    if relative.is_empty()
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(invalid("expected a canonical relative file path"));
    }
    history.project().with_review_history(
        history.metadata_path(),
        store.clone(),
        trusted,
        |workspace, store| {
            let (head, proposed, executable, base_digest, base_executable) =
                approved_file(workspace, bundle, target, relative, limits.file_bytes)?;
            let capture = history.project().capture_inputs(limits)?;
            history
                .project()
                .history_configuration(store, Some(capture.exclusion_digest()))?;
            let current = capture
                .files()
                .iter()
                .find(|file| file.path() == Path::new(relative))
                .ok_or_else(|| invalid("selected file is excluded or unavailable"))?;
            if current.digest().to_string() != base_digest
                || current.executable() != base_executable
            {
                return Err(invalid("current file diverged from the approved base"));
            }
            let (source, expected) = read_target(
                history.project().root(),
                relative,
                limits.file_bytes as usize,
            )
            .map_err(error)?;
            if expected != current.bytes() || source.executable() != current.executable() {
                return Err(invalid("source changed during preparation"));
            }
            let root = external_store(recovery_root, history.project())?;
            let source_device = history.project().device;
            if root.identity()?.0 != source_device {
                return Err(invalid("recovery must share the source filesystem"));
            }
            if root.try_clone_directory()?.metadata()?.permissions().mode() & 0o077 != 0 {
                return Err(invalid("recovery root must be private to its owner"));
            }
            let mut random = [0u8; 16];
            File::open("/dev/urandom")?.read_exact(&mut random)?;
            let name = format!(
                "integration-{}",
                random
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            );
            let recovery = root.create_child_directory(OsStr::new(&name))?;
            let recovery_path = recovery_root.join(&name);
            let recovery_identity = recovery.identity()?;
            let source_parent = source.parent_installation();
            let source_file = source.file_installation();
            let source_mode = source.mode_with_executable(source.executable());
            let mode = source.mode_with_executable(executable);
            let exclusions = capture.exclusion_digest().to_string();
            let mut receipt = Json::Null;
            let replacement = RetainedReplacement::prepare(
                source,
                expected,
                proposed.clone(),
                mode,
                recovery.clone(),
                |installed, metadata_digest| {
                    receipt = Json::object([
                        ("schema", Json::text("mesh.attachment-file-integration/v1")),
                        ("project", Json::text(history.id())),
                        ("attachment", history.project().receipt()?),
                        ("head", Json::text(&head)),
                        ("bundle", Json::text(bundle)),
                        ("target", Json::text(target)),
                        ("path", Json::text(relative)),
                        ("source_parent", Json::text(&source_parent)),
                        ("source_file", Json::text(&source_file)),
                        ("source_digest", Json::text(&base_digest)),
                        ("native_metadata_digest", Json::text(metadata_digest)),
                        ("source_executable", Json::Bool(base_executable)),
                        ("source_mode", Json::Number(u64::from(source_mode))),
                        (
                            "store_device",
                            Json::text(format!("{:016x}", store.identity()?.0)),
                        ),
                        (
                            "store_inode",
                            Json::text(format!("{:016x}", store.identity()?.1)),
                        ),
                        ("installed_file", Json::text(installed.token())),
                        ("installed_digest", Json::text(digest(&proposed))),
                        ("installed_mode", Json::Number(u64::from(mode))),
                        ("exclusions", Json::text(&exclusions)),
                        (
                            "recovery_device",
                            Json::text(format!("{:016x}", recovery_identity.0)),
                        ),
                        (
                            "recovery_inode",
                            Json::text(format!("{:016x}", recovery_identity.1)),
                        ),
                        ("automatic_replay", Json::Bool(false)),
                    ]);
                    recovery.filesystem().write_new_file(
                        Path::new("prepared.json"),
                        receipt.encode().as_bytes(),
                        fs::Permissions::from_mode(0o600),
                    )
                },
            )?;
            Ok(PreparedMainFileIntegration {
                history: history.clone(),
                store: store.clone(),
                recovery,
                recovery_path,
                receipt,
                replacement,
                limits,
                bundle: bundle.to_owned(),
                target: target.to_owned(),
                head,
                exclusions,
            })
        },
    )
}

impl PreparedMainFileIntegration {
    /// Frozen current bytes presented by the native confirmation host.
    pub fn current_content(&self) -> &[u8] {
        self.replacement.current_bytes()
    }

    /// Frozen approved bytes presented by the native confirmation host.
    pub fn proposed_content(&self) -> &[u8] {
        self.replacement.replacement_bytes()
    }

    /// Private durable transaction directory. Never automatically remove its displaced file: an
    /// already-open editor may continue writing it even after apply returns successfully.
    pub fn recovery_path(&self) -> &Path {
        &self.recovery_path
    }

    /// Exact facts for a future native confirmation. This receipt is not a replay capability.
    pub fn proposal(&self) -> &Json {
        &self.receipt
    }

    /// Explicit trusted-native invocation after user confirmation, never an observation side effect.
    /// Errors and lost replies require inspecting retained recovery material; never blindly retry.
    /// A successful observation is not a promise that ordinary tools stopped writing afterward.
    pub fn apply(self, trusted: &TrustedReviewers) -> io::Result<Json> {
        let history = self.history.clone();
        history.project().with_review_history(
            history.metadata_path(),
            self.store.clone(),
            trusted,
            |workspace, store| {
                self.validate(workspace, store)?;
                self.apply_validated()
            },
        )
    }

    pub(super) fn validate(
        &self,
        workspace: &OpenWorkspace,
        store: &PinnedWorkspaceRoot,
    ) -> io::Result<()> {
        let current = workspace
            .accepted_main_review()
            .map_err(error)?
            .ok_or_else(|| invalid("accepted review unavailable"))?;
        if current.bundle.to_string() != self.bundle
            || current.subject_operation.to_string() != self.target
            || super::approval::main_head(workspace)?
                .map(|head| head.to_string())
                .as_deref()
                != Some(&self.head)
        {
            return Err(invalid("Mesh main changed since preparation"));
        }
        let capture = self.history.project().capture_inputs(self.limits)?;
        self.history
            .project()
            .history_configuration(store, Some(capture.exclusion_digest()))?;
        if capture.exclusion_digest().to_string() != self.exclusions {
            return Err(invalid("exclusions changed since preparation"));
        }
        self.recovery.ensure_namespace_identity()?;
        let mut receipt = String::new();
        self.recovery
            .filesystem()
            .read_file(Path::new("prepared.json"))?
            .take(65_537)
            .read_to_string(&mut receipt)?;
        if receipt != self.receipt.encode() {
            return Err(invalid("prepared receipt changed"));
        }
        self.replacement.validate()
    }

    /// The caller holds verified project history and has validated the proposal. The retained
    /// exchange still rechecks source/stage evidence; this never skips filesystem race protection.
    pub(super) fn apply_validated(self) -> io::Result<Json> {
        let applied = self.replacement.apply()?;
        let result = Json::object([
            (
                "schema",
                Json::text("mesh.attachment-file-integration-result/v1"),
            ),
            (
                "proposal_digest",
                Json::text(digest(self.receipt.encode().as_bytes())),
            ),
            (
                "status",
                Json::text(if applied {
                    "applied-observed"
                } else {
                    "reconciliation-required"
                }),
            ),
            ("displaced_file_retained", Json::Bool(true)),
            ("observation_final", Json::Bool(false)),
        ]);
        // Failure here means uncertain acknowledgement, not that the exchange was undone.
        self.recovery.filesystem().write_new_file(
            Path::new("observed.json"),
            result.encode().as_bytes(),
            fs::Permissions::from_mode(0o600),
        )?;
        Ok(result)
    }
}
