//! Explicit restoration of retained work creates a new transaction; it never reverses an old one.
use super::{external_store, invalid, recovery, ObservationLimits, ProvisionedAttachment};
use crate::ipc::Json;
use crate::managed_file::retained_replacement::{
    absent_parent, observe_file, read_target, snapshot_file, RetainedAddition,
    RetainedFileObservation, RetainedReplacement,
};
use crate::root_authority::PinnedWorkspaceRoot;
use crate::TrustedReviewers;
use mesh_types::{Blake3, ContentDigest as _};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

/// Native-only, single-use restoration proposal. The host must obtain explicit user confirmation
/// of its frozen before/after content. It is not a serialized capability or an approval of main.
pub struct PreparedRetainedRestoration {
    history: ProvisionedAttachment,
    store: PinnedWorkspaceRoot,
    root: PinnedWorkspaceRoot,
    origin: PinnedWorkspaceRoot,
    origin_receipt: String,
    origin_snapshot: RetainedFileObservation,
    recovery: PinnedWorkspaceRoot,
    path: PathBuf,
    receipt: Json,
    replacement: RestorationChange,
    limits: ObservationLimits,
}
enum RestorationChange {
    Replace(RetainedReplacement),
    Add(RetainedAddition),
}
impl RestorationChange {
    fn apply(self) -> io::Result<bool> {
        match self {
            Self::Replace(change) => change.apply(),
            Self::Add(change) => change.apply(),
        }
    }
}
fn hash(bytes: &[u8]) -> String {
    Blake3::digest_bytes(bytes).to_string()
}
fn text(value: &Json, key: &str) -> io::Result<String> {
    recovery::text(value, key).map(str::to_owned)
}
fn write(root: &PinnedWorkspaceRoot, name: &str, value: &Json) -> io::Result<()> {
    root.filesystem().write_new_file(
        Path::new(name),
        value.encode().as_bytes(),
        fs::Permissions::from_mode(0o600),
    )
}
fn replace(value: &mut Json, field: &str, replacement: Json) -> io::Result<()> {
    let Json::Object(fields) = value else {
        return Err(invalid("invalid native restoration proposal"));
    };
    let item = fields
        .iter_mut()
        .find(|(key, _)| key == field)
        .ok_or_else(|| invalid("missing native restoration field"))?;
    item.1 = replacement;
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    history: &ProvisionedAttachment,
    store: PinnedWorkspaceRoot,
    recovery_root: &Path,
    transaction: &str,
    trusted: &TrustedReviewers,
    limits: ObservationLimits,
) -> io::Result<PreparedRetainedRestoration> {
    limits.validate()?;
    if !recovery::transaction(transaction) {
        return Err(invalid("invalid restoration origin"));
    }
    let root = external_store(recovery_root, history.project())?;
    if root.identity()?.0 != history.project().device
        || root.try_clone_directory()?.metadata()?.permissions().mode() & 0o077 != 0
    {
        return Err(invalid(
            "restoration requires private recovery storage on the source filesystem",
        ));
    }
    history.project().with_review_history(
        history.metadata_path(),
        store,
        trusted,
        |workspace, store| {
            let origin = root.open_child_directory(OsStr::new(transaction))?;
            let (original, origin_receipt) = recovery::read_json(&origin, "prepared.json")?;
            recovery::validate_receipt(&original, history, store, &origin)?;
            // Reserve one ancestry level for the new transaction; verification never replays a parent.
            recovery::verify_ancestry(&original, history, store, &root, workspace, trusted, 1)?;
            let relative = text(&original, "path")?;
            let capture = history.project().capture_inputs(limits)?;
            history
                .project()
                .history_configuration(store, Some(capture.exclusion_digest()))?;
            if !capture.admits_file_path(&relative)? {
                return Err(invalid("restoration destination is excluded"));
            }
            let live = capture
                .files()
                .iter()
                .find(|file| file.path() == Path::new(&relative));
            let captured_bytes = capture
                .files()
                .iter()
                .map(|file| file.bytes().len() as u64)
                .sum::<u64>();
            let retained_limit = limits
                .file_bytes
                .min(limits.bytes.saturating_sub(captured_bytes));
            let (snapshot, retained_bytes, retained_file) =
                snapshot_file(&origin, Path::new("exchange"), retained_limit)?;
            if snapshot.installation != recovery::text(&original, "source_file")? {
                return Err(invalid(
                    "origin does not retain its exact displaced source file",
                ));
            }
            let source = if let Some(live) = live {
                let (source, expected) = read_target(
                    history.project().root(),
                    &relative,
                    limits.file_bytes as usize,
                )?;
                if expected != live.bytes() || source.executable() != live.executable() {
                    return Err(invalid(
                        "working file changed during restoration preparation",
                    ));
                }
                Some((source, expected))
            } else {
                None
            };
            let parent = if let Some((source, _)) = &source {
                source.parent_installation()
            } else {
                absent_parent(&history.project().pinned, Path::new(&relative))?
                    .ok_or_else(|| invalid("restoration destination is occupied or unavailable"))?
            };
            let adding = source.is_none();
            let mut receipt = Json::object(recovery::KEYS.iter().map(|key| {
                (
                    *key,
                    original
                        .get(key)
                        .expect("validated recovery fields")
                        .clone(),
                )
            }));
            let mut random = [0u8; 16];
            File::open("/dev/urandom")?.read_exact(&mut random)?;
            let name = format!(
                "restoration-{}",
                random
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            );
            let recovery = root.create_child_directory(OsStr::new(&name))?;
            let identity = recovery.identity()?;
            for (key, value) in [
                (
                    "schema",
                    Json::text(if adding {
                        "mesh.attachment-file-restoration-addition/v1"
                    } else {
                        "mesh.attachment-file-restoration/v1"
                    }),
                ),
                ("source_parent", Json::text(&parent)),
                (
                    "source_file",
                    source.as_ref().map_or(Json::Null, |(source, _)| {
                        Json::text(source.file_installation())
                    }),
                ),
                (
                    "source_digest",
                    source
                        .as_ref()
                        .map_or(Json::Null, |(_, bytes)| Json::text(hash(bytes))),
                ),
                (
                    "source_mode",
                    source.as_ref().map_or(Json::Null, |(source, _)| {
                        Json::Number(u64::from(source.mode_with_executable(source.executable())))
                    }),
                ),
                (
                    "source_executable",
                    source
                        .as_ref()
                        .map_or(Json::Null, |(source, _)| Json::Bool(source.executable())),
                ),
                ("installed_digest", Json::text(&snapshot.digest)),
                ("installed_mode", Json::Number(u64::from(snapshot.mode))),
                (
                    "recovery_device",
                    Json::text(format!("{:016x}", identity.0)),
                ),
                ("recovery_inode", Json::text(format!("{:016x}", identity.1))),
            ] {
                replace(&mut receipt, key, value)?;
            }
            let mut record = |installed: crate::managed_file::ManagedFileIdentity,
                              source_metadata: &str,
                              installed_metadata: &str,
                              policy: Option<(&str, u32)>| {
                if installed_metadata != snapshot.metadata {
                    return Err(invalid("retained metadata changed during preparation"));
                }
                replace(
                    &mut receipt,
                    "installed_file",
                    Json::text(installed.token()),
                )?;
                replace(
                    &mut receipt,
                    "native_metadata_digest",
                    Json::text(source_metadata),
                )?;
                let Json::Object(fields) = &mut receipt else {
                    return Err(invalid("invalid restoration receipt"));
                };
                fields.extend([
                    (
                        "installed_metadata_digest".to_owned(),
                        Json::text(installed_metadata),
                    ),
                    ("origin_transaction".to_owned(), Json::text(transaction)),
                    (
                        "origin_proposal_digest".to_owned(),
                        Json::text(hash(origin_receipt.as_bytes())),
                    ),
                    ("origin_file".to_owned(), Json::text(&snapshot.installation)),
                    ("origin_digest".to_owned(), Json::text(&snapshot.digest)),
                    (
                        "origin_mode".to_owned(),
                        Json::Number(u64::from(snapshot.mode)),
                    ),
                    (
                        "origin_metadata_digest".to_owned(),
                        Json::text(&snapshot.metadata),
                    ),
                ]);
                if let Some((digest, mode)) = policy {
                    fields.extend([
                        ("parent_metadata_digest".to_owned(), Json::text(digest)),
                        ("parent_mode".to_owned(), Json::Number(u64::from(mode))),
                    ]);
                }
                write(&recovery, "prepared.json", &receipt)
            };
            let replacement = if let Some((source, expected)) = source {
                RestorationChange::Replace(RetainedReplacement::prepare_with_metadata(
                    source,
                    expected,
                    retained_bytes,
                    snapshot.mode,
                    recovery.clone(),
                    Some(&retained_file),
                    |installed, before, after| record(installed, before, after, None),
                )?)
            } else {
                RestorationChange::Add(RetainedAddition::prepare_restoration(
                    history.project().pinned.clone(),
                    PathBuf::from(&relative),
                    parent,
                    retained_bytes,
                    snapshot.mode,
                    recovery.clone(),
                    &retained_file,
                    |installed, _, metadata, parent_metadata, parent_mode| {
                        record(
                            installed,
                            metadata,
                            metadata,
                            Some((parent_metadata, parent_mode)),
                        )
                    },
                )?)
            };
            Ok(PreparedRetainedRestoration {
                history: history.clone(),
                store: store.clone(),
                root: root.clone(),
                origin,
                origin_receipt,
                origin_snapshot: snapshot,
                recovery,
                path: recovery_root.join(name),
                receipt,
                replacement,
                limits,
            })
        },
    )
}

impl PreparedRetainedRestoration {
    /// Exact current content which would be displaced and preserved, for native confirmation.
    pub fn current_content(&self) -> &[u8] {
        match &self.replacement {
            RestorationChange::Replace(change) => change.current_bytes(),
            RestorationChange::Add(_) => &[],
        }
    }
    /// True means creation at a confirmed absent path, not replacement of an empty file.
    pub fn adds_path(&self) -> bool {
        matches!(self.replacement, RestorationChange::Add(_))
    }
    /// Exact frozen retained content to restore, never a moving editor file.
    pub fn restored_content(&self) -> &[u8] {
        match &self.replacement {
            RestorationChange::Replace(change) => change.replacement_bytes(),
            RestorationChange::Add(change) => change.replacement_bytes(),
        }
    }
    /// Native receipt facts including metadata and origin binding. This is not approval of main.
    pub fn proposal(&self) -> &Json {
        &self.receipt
    }
    /// New recovery directory. Both it and the original retained file survive every outcome.
    pub fn recovery_path(&self) -> &Path {
        &self.path
    }

    /// Apply once after explicit native user confirmation. Revalidate both current and retained
    /// inputs; races after the last check preserve all displaced inodes for later reconciliation.
    pub fn apply(self, trusted: &TrustedReviewers) -> io::Result<Json> {
        let adding = self.adds_path();
        self.history.project().with_review_history(
            self.history.metadata_path(),
            self.store,
            trusted,
            |workspace, store| {
                self.root.ensure_namespace_identity()?;
                self.origin.ensure_namespace_identity()?;
                self.recovery.ensure_namespace_identity()?;
                let (original, raw) = recovery::read_json(&self.origin, "prepared.json")?;
                if raw != self.origin_receipt {
                    return Err(invalid("restoration origin receipt changed"));
                }
                recovery::validate_receipt(&original, &self.history, store, &self.origin)?;
                recovery::verify_ancestry(
                    &original,
                    &self.history,
                    store,
                    &self.root,
                    workspace,
                    trusted,
                    1,
                )?;
                let capture = self.history.project().capture_inputs(self.limits)?;
                self.history
                    .project()
                    .history_configuration(store, Some(capture.exclusion_digest()))?;
                if !capture.admits_file_path(recovery::text(&self.receipt, "path")?)? {
                    return Err(invalid("restoration destination is excluded"));
                }
                let captured_bytes = capture
                    .files()
                    .iter()
                    .map(|file| file.bytes().len() as u64)
                    .sum::<u64>();
                let limit = self
                    .limits
                    .file_bytes
                    .min(self.limits.bytes.saturating_sub(captured_bytes));
                if observe_file(&self.origin, Path::new("exchange"), limit)? != self.origin_snapshot
                {
                    return Err(invalid(
                        "retained work changed since confirmation was prepared",
                    ));
                }
                let (prepared, _) = recovery::read_json(&self.recovery, "prepared.json")?;
                if prepared != self.receipt {
                    return Err(invalid("restoration proposal changed"));
                }
                let applied = self.replacement.apply()?;
                let result = Json::object([
                    ("schema", Json::text(recovery::result_schema(&self.receipt))),
                    (
                        "proposal_digest",
                        Json::text(hash(self.receipt.encode().as_bytes())),
                    ),
                    (
                        "status",
                        Json::text(if applied {
                            "applied-observed"
                        } else {
                            "reconciliation-required"
                        }),
                    ),
                    ("displaced_file_retained", Json::Bool(!adding)),
                    ("observation_final", Json::Bool(false)),
                ]);
                write(&self.recovery, "observed.json", &result)?;
                Ok(result)
            },
        )
    }
}
