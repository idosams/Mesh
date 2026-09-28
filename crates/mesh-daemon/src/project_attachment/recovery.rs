//! Bounded restart evidence. Receipts never become replay, cleanup or source-write authority.
use super::inspection::comparison_entries;
use super::{external_store, invalid, ObservationLimits, ProvisionedAttachment};
use crate::ipc::Json;
use crate::managed_file::retained_replacement::{
    absent_parent, observe_file, RetainedFileObservation,
};
use crate::root_authority::PinnedWorkspaceRoot;
use crate::workspace::OpenWorkspace;
use crate::TrustedReviewers;
use mesh_approval::{ExpectedHumanApproval, HeadId, HumanApprovalReceipt};
use mesh_store::RecordDigest;
use mesh_types::{Blake3, ContentDigest as _};
use std::ffi::OsStr;
use std::io::{self, Read as _};
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

const PAGE: usize = 32;
pub(super) const KEYS: &[&str] = &[
    "schema",
    "project",
    "attachment",
    "head",
    "bundle",
    "target",
    "path",
    "source_parent",
    "source_file",
    "source_digest",
    "native_metadata_digest",
    "source_executable",
    "source_mode",
    "store_device",
    "store_inode",
    "installed_file",
    "installed_digest",
    "installed_mode",
    "exclusions",
    "recovery_device",
    "recovery_inode",
    "automatic_replay",
];
pub(super) fn text<'a>(value: &'a Json, key: &str) -> io::Result<&'a str> {
    value
        .get(key)
        .and_then(Json::as_text)
        .ok_or_else(|| invalid("missing recovery text"))
}
fn number(value: &Json, key: &str) -> io::Result<u64> {
    value
        .get(key)
        .and_then(Json::as_u64)
        .ok_or_else(|| invalid("missing recovery number"))
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(super) fn transaction(value: &str) -> bool {
    value
        .strip_prefix("integration-")
        .or_else(|| value.strip_prefix("restoration-"))
        .is_some_and(|id| hex(id, 32))
}
pub(super) fn group_identity(value: &str) -> bool {
    value
        .strip_prefix("integration-group-")
        .is_some_and(|id| hex(id, 32))
}
const RESTORE_KEYS: &[&str] = &[
    "installed_metadata_digest",
    "origin_transaction",
    "origin_proposal_digest",
    "origin_file",
    "origin_digest",
    "origin_mode",
    "origin_metadata_digest",
];
pub(super) fn is_absent_restoration(value: &Json) -> bool {
    value.get("schema") == Some(&Json::text("mesh.attachment-file-restoration-addition/v1"))
}
pub(super) fn is_restoration(value: &Json) -> bool {
    is_absent_restoration(value)
        || value.get("schema") == Some(&Json::text("mesh.attachment-file-restoration/v1"))
}
fn is_insertion(value: &Json) -> bool {
    is_addition(value) || is_absent_restoration(value)
}
fn has_parent_policy(value: &Json) -> bool {
    is_addition_v2(value) || is_absent_restoration(value)
}
pub(super) fn is_removal(value: &Json) -> bool {
    value.get("schema") == Some(&Json::text("mesh.attachment-file-removal/v1"))
}
fn is_addition_v2(value: &Json) -> bool {
    value.get("schema") == Some(&Json::text("mesh.attachment-file-addition/v2"))
}
pub(super) fn is_addition(value: &Json) -> bool {
    is_addition_v2(value)
        || value.get("schema") == Some(&Json::text("mesh.attachment-file-addition/v1"))
}
pub(super) fn result_schema(value: &Json) -> &'static str {
    if is_absent_restoration(value) {
        "mesh.attachment-file-restoration-addition-result/v1"
    } else if is_restoration(value) {
        "mesh.attachment-file-restoration-result/v1"
    } else if is_addition_v2(value) {
        "mesh.attachment-file-addition-result/v2"
    } else if is_addition(value) {
        "mesh.attachment-file-addition-result/v1"
    } else if is_removal(value) {
        "mesh.attachment-file-removal-result/v1"
    } else {
        "mesh.attachment-file-integration-result/v1"
    }
}
fn file_identity(value: &str) -> bool {
    if !value.is_ascii() {
        return false;
    }
    let parts: Vec<_> = value.split(':').collect();
    parts.len() == 3 && hex(parts[0], 16) && hex(parts[1], 16) && {
        let suffix = parts[2].as_bytes();
        suffix.len() == 34
            && matches!(suffix[0], b'b' | b'c')
            && suffix[17] == b'.'
            && hex(&parts[2][1..17], 16)
            && hex(&parts[2][18..], 16)
    }
}
fn digest(value: &str) -> io::Result<RecordDigest> {
    if !hex(value, 64) {
        return Err(invalid("noncanonical recovery digest"));
    }
    RecordDigest::parse_hex(value).map_err(|_| invalid("invalid recovery digest"))
}
pub(super) fn read_json(root: &PinnedWorkspaceRoot, name: &str) -> io::Result<(Json, String)> {
    let file = root.filesystem().inspect_entry(Path::new(name))?;
    if !file.metadata()?.is_file() {
        return Err(invalid("recovery receipt is not a regular file"));
    }
    let mut raw = String::new();
    file.take(65_537).read_to_string(&mut raw)?;
    if raw.len() > 65_536 {
        return Err(invalid("recovery receipt exceeds budget"));
    }
    let value = Json::parse(&raw).map_err(|_| invalid("invalid recovery JSON"))?;
    if value.encode() != raw {
        return Err(invalid("noncanonical recovery JSON"));
    }
    Ok((value, raw))
}
pub(super) fn validate_receipt(
    value: &Json,
    history: &ProvisionedAttachment,
    store: &PinnedWorkspaceRoot,
    recovery: &PinnedWorkspaceRoot,
) -> io::Result<()> {
    let Json::Object(fields) = value else {
        return Err(invalid("expected recovery object"));
    };
    let restoring = is_restoration(value);
    let removing = is_removal(value);
    let adding = is_insertion(value);
    let extra: &[&str] = if restoring {
        RESTORE_KEYS
    } else if is_addition_v2(value) {
        &["parent_metadata_digest", "parent_mode"]
    } else {
        &[]
    };
    let policy_extra: &[&str] = if is_absent_restoration(value) {
        &["parent_metadata_digest", "parent_mode"]
    } else {
        &[]
    };
    if !fields.iter().map(|(key, _)| key.as_str()).eq(KEYS
        .iter()
        .chain(extra)
        .chain(policy_extra)
        .copied())
        || (!restoring
            && !removing
            && !adding
            && text(value, "schema")? != "mesh.attachment-file-integration/v1")
        || text(value, "project")? != history.id()
        || value.get("attachment") != Some(&history.project().receipt()?)
        || value.get("automatic_replay") != Some(&Json::Bool(false))
    {
        return Err(invalid("recovery schema or attachment mismatch"));
    }
    for key in [
        "head",
        "bundle",
        "target",
        "source_digest",
        "installed_digest",
        "native_metadata_digest",
        "exclusions",
    ] {
        if (removing && key == "installed_digest") || (adding && key == "source_digest") {
            continue;
        }
        digest(text(value, key)?)?;
    }
    for key in ["source_file", "installed_file"] {
        if (removing && key == "installed_file") || (adding && key == "source_file") {
            continue;
        }
        if !file_identity(text(value, key)?) {
            return Err(invalid("invalid recovery file identity"));
        }
    }
    if removing
        && ["installed_file", "installed_digest", "installed_mode"]
            .into_iter()
            .any(|key| value.get(key) != Some(&Json::Null))
    {
        return Err(invalid("removal cannot install a file"));
    }
    if adding
        && [
            "source_file",
            "source_digest",
            "source_mode",
            "source_executable",
        ]
        .into_iter()
        .any(|key| value.get(key) != Some(&Json::Null))
    {
        return Err(invalid("addition cannot replace a source file"));
    }
    if !removing && !adding && text(value, "source_file")? == text(value, "installed_file")? {
        return Err(invalid("recovery identities overlap"));
    }
    let parent: Vec<_> = text(value, "source_parent")?.split(':').collect();
    if parent.len() != 2 || !parent.iter().all(|part| hex(part, 16)) {
        return Err(invalid("invalid recovery parent"));
    }
    let path = text(value, "path")?;
    if path.is_empty()
        || path.contains('\0')
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(invalid("invalid recovery relative path"));
    }
    for (prefix, root) in [("store", store), ("recovery", recovery)] {
        let (device, inode) = root.identity()?;
        if text(value, &format!("{prefix}_device"))? != format!("{device:016x}")
            || text(value, &format!("{prefix}_inode"))? != format!("{inode:016x}")
        {
            return Err(invalid("recovery directory identity changed"));
        }
    }
    let source_mode = number(
        value,
        if adding {
            "installed_mode"
        } else {
            "source_mode"
        },
    )?;
    let installed_mode = if removing {
        source_mode
    } else {
        number(value, "installed_mode")?
    };
    if [source_mode, installed_mode]
        .into_iter()
        .any(|mode| mode & !0o100777 != 0 || mode & 0o100000 == 0)
        || (!restoring && source_mode & !0o111 != installed_mode & !0o111)
        || (!adding
            && value.get("source_executable") != Some(&Json::Bool(source_mode & 0o111 != 0)))
        || (is_addition(value)
            && if is_addition_v2(value) {
                installed_mode & !0o100755 != 0
            } else {
                !matches!(installed_mode, 0o100644 | 0o100755)
            })
    {
        return Err(invalid("invalid recovery mode"));
    }
    if has_parent_policy(value) {
        digest(text(value, "parent_metadata_digest")?)?;
        let mode = number(value, "parent_mode")?;
        if mode & !0o047777 != 0 || mode & 0o040000 == 0 {
            return Err(invalid("invalid addition parent mode"));
        }
    }
    if restoring {
        for key in [
            "installed_metadata_digest",
            "origin_proposal_digest",
            "origin_digest",
            "origin_metadata_digest",
        ] {
            digest(text(value, key)?)?;
        }
        if !transaction(text(value, "origin_transaction")?)
            || !file_identity(text(value, "origin_file")?)
            || value.get("origin_digest") != value.get("installed_digest")
            || value.get("origin_mode") != value.get("installed_mode")
            || value.get("origin_metadata_digest") != value.get("installed_metadata_digest")
            || (is_absent_restoration(value)
                && value.get("native_metadata_digest") != value.get("installed_metadata_digest"))
        {
            return Err(invalid("invalid restoration snapshot binding"));
        }
    }
    let (configuration, _) = history.project().history_configuration(store, None)?;
    if Json::parse(&configuration)
        .map_err(|_| invalid("invalid history configuration"))?
        .get("exclusions")
        != value.get("exclusions")
    {
        return Err(invalid("recovery history policy mismatch"));
    }
    Ok(())
}
pub(super) fn verify_history(
    value: &Json,
    workspace: &OpenWorkspace,
    trusted: &TrustedReviewers,
) -> io::Result<()> {
    let fail = |_| invalid("recovery approval cannot be verified");
    super::approval::main_head(workspace)?;
    let head = HeadId::from_bytes(*digest(text(value, "head")?)?.as_bytes());
    let bundle = digest(text(value, "bundle")?)?;
    let review = workspace
        .review(&bundle)
        .ok_or_else(|| invalid("recovery review unavailable"))?;
    if review.subject_operation != digest(text(value, "target")?)?
        || !workspace.has_verified_shared_head(head)
    {
        return Err(invalid("recovery head was not accepted"));
    }
    let context = workspace.human_approval_context(&review).map_err(fail)?;
    if context.reviewed_actor_head() != head {
        return Err(invalid("recovery head mismatch"));
    }
    let approval = workspace.approved_envelope(&bundle).map_err(fail)?;
    let bytes = workspace
        .approval_receipt(approval.approval)
        .map_err(fail)?;
    let receipt = HumanApprovalReceipt::from_canonical_bytes(&bytes)
        .map_err(|_| invalid("invalid approval receipt"))?;
    let expected = receipt.draft().expected();
    let credential = trusted
        .human_credential(expected.credential().id())
        .ok_or_else(|| invalid("recovery credential unavailable"))?;
    let expected = ExpectedHumanApproval::new(context.clone(), credential, *expected.challenge());
    mesh_approval::verify_human_approval_receipt(&bytes, &expected)
        .map_err(|_| invalid("recovery signature mismatch"))?;
    let base = context.expected_canonical_head();
    let before = if base == crate::publication::GENESIS_SHARED_HEAD {
        Default::default()
    } else {
        comparison_entries(
            workspace
                .historical_workspace_preview(workspace.review_target_for_head(base).map_err(fail)?)
                .map_err(|_| invalid("base content unavailable"))?,
        )
    };
    let after = comparison_entries(
        workspace
            .historical_workspace_preview(review.subject_operation)
            .map_err(|_| invalid("approved content unavailable"))?,
    );
    let path = text(value, "path")?;
    let old = before.get(path);
    let new = after.get(path);
    let valid_base = if is_addition(value) {
        old.is_none()
    } else if let Some(old) = old.filter(|entry| entry.kind == "file") {
        old.digest == Some(digest(text(value, "source_digest")?)?)
            && old.executable == Some(number(value, "source_mode")? & 0o111 != 0)
    } else {
        false
    };
    let valid_result = if is_removal(value) {
        new.is_none()
    } else if let Some(new) = new.filter(|entry| entry.kind == "file") {
        new.digest == Some(digest(text(value, "installed_digest")?)?)
            && new.executable == Some(number(value, "installed_mode")? & 0o111 != 0)
    } else {
        false
    };
    if !valid_base || !valid_result || old == new {
        return Err(invalid("recovery content does not match approved history"));
    }
    Ok(())
}
/// A restoration follows a bounded chain back to an accepted integration. Its new content is
/// private retained work, never a claim that the approved main contains those bytes.
#[allow(clippy::too_many_arguments)]
pub(super) fn verify_ancestry(
    value: &Json,
    history: &ProvisionedAttachment,
    store: &PinnedWorkspaceRoot,
    root: &PinnedWorkspaceRoot,
    workspace: &OpenWorkspace,
    trusted: &TrustedReviewers,
    depth: usize,
) -> io::Result<()> {
    if depth >= 16 {
        return Err(invalid("restoration ancestry exceeds its bound"));
    }
    if !is_restoration(value) {
        return verify_history(value, workspace, trusted);
    }
    let origin = root.open_child_directory(OsStr::new(text(value, "origin_transaction")?))?;
    let (parent, raw) = read_json(&origin, "prepared.json")?;
    validate_receipt(&parent, history, store, &origin)?;
    if Blake3::digest_bytes(raw.as_bytes()).to_string() != text(value, "origin_proposal_digest")?
        || parent.get("source_file") != value.get("origin_file")
        || ["head", "bundle", "target", "path", "exclusions"]
            .into_iter()
            .any(|key| parent.get(key) != value.get(key))
    {
        return Err(invalid("restoration ancestry binding changed"));
    }
    verify_ancestry(&parent, history, store, root, workspace, trusted, depth + 1)?;
    origin.ensure_namespace_identity()
}

fn report(id: &str, status: &str, details: Json) -> Json {
    let attention = status != "prepared-arrangement"
        && !(status == "applied-arrangement"
            && details.get("recorded_outcome") == Some(&Json::text("applied-observed")));
    Json::object([
        ("attention_required", Json::Bool(attention)),
        ("observation_final", Json::Bool(false)),
        ("transaction", Json::text(id)),
        ("status", Json::text(status)),
        ("details", details),
        ("atomic_snapshot", Json::Bool(false)),
        ("write_authority", Json::Bool(false)),
        ("automatic_replay", Json::Bool(false)),
        ("cleanup_authority", Json::Bool(false)),
    ])
}
fn observe(
    root: &PinnedWorkspaceRoot,
    path: &Path,
    limits: ObservationLimits,
    remaining: &mut u64,
) -> Option<RetainedFileObservation> {
    if *remaining == 0 {
        return None;
    }
    let budget = limits.file_bytes.min(*remaining);
    let result = observe_file(root, path, budget);
    // Failed reads may already have consumed their entire allowance. Charge conservatively.
    *remaining = remaining.saturating_sub(result.as_ref().map_or(budget, |item| item.bytes));
    result.ok()
}
fn evidence(item: &Option<RetainedFileObservation>) -> Json {
    item.as_ref().map_or(Json::Null, |item| {
        Json::object([
            ("installation", Json::text(&item.installation)),
            ("digest", Json::text(&item.digest)),
            ("mode", Json::Number(u64::from(item.mode))),
            ("bytes", Json::Number(item.bytes)),
            ("native_metadata_digest", Json::text(&item.metadata)),
        ])
    })
}
#[allow(clippy::too_many_arguments)]
fn inspect(
    history: &ProvisionedAttachment,
    store: &PinnedWorkspaceRoot,
    root: &PinnedWorkspaceRoot,
    workspace: &OpenWorkspace,
    trusted: &TrustedReviewers,
    id: &str,
    limits: ObservationLimits,
    remaining: &mut u64,
) -> Json {
    let Ok(recovery) = root.open_child_directory(OsStr::new(id)) else {
        return report(id, "unavailable-directory", Json::Null);
    };
    let Ok((value, raw)) = read_json(&recovery, "prepared.json") else {
        return report(id, "invalid-receipt", Json::Null);
    };
    if validate_receipt(&value, history, store, &recovery).is_err() {
        return report(id, "invalid-receipt", Json::Null);
    }
    if verify_ancestry(&value, history, store, root, workspace, trusted, 0).is_err() {
        return report(id, "unverified-history", Json::Null);
    }
    let outcome = match read_json(&recovery, "observed.json") {
        Err(error) if error.kind() == io::ErrorKind::NotFound => "absent",
        Err(_) => "invalid",
        Ok((observed, _)) => {
            let status = observed.get("status").and_then(Json::as_text).unwrap_or("");
            let expected = Json::object([
                ("schema", Json::text(result_schema(&value))),
                (
                    "proposal_digest",
                    Json::text(Blake3::digest_bytes(raw.as_bytes()).to_string()),
                ),
                ("status", Json::text(status)),
                ("displaced_file_retained", Json::Bool(!is_insertion(&value))),
                ("observation_final", Json::Bool(false)),
            ]);
            if observed != expected
                || !matches!(status, "applied-observed" | "reconciliation-required")
            {
                "invalid"
            } else if status == "applied-observed" {
                "applied-observed"
            } else {
                "reconciliation-required"
            }
        }
    };
    // Validation above established exact types. This code only compares observations; it never
    // turns a matching byte digest or an old success receipt into permission to mutate.
    let removing = is_removal(&value);
    let adding = is_insertion(&value);
    let source_absent = (removing || adding)
        .then(|| {
            absent_parent(
                &history.project().pinned,
                Path::new(text(&value, "path").unwrap()),
            )
            .ok()
            .flatten()
        })
        .flatten();
    let retained_absent = (removing || adding)
        && absent_parent(&recovery, Path::new("exchange")).is_ok_and(|parent| parent.is_some());
    let source = if source_absent.is_some() {
        None
    } else {
        observe(
            &history.project().pinned,
            Path::new(text(&value, "path").unwrap()),
            limits,
            remaining,
        )
    };
    let retained = if retained_absent {
        None
    } else {
        observe(&recovery, Path::new("exchange"), limits, remaining)
    };
    let matching = |item: &RetainedFileObservation, prefix: &str| {
        item.installation == text(&value, &format!("{prefix}_file")).unwrap()
            && item.digest == text(&value, &format!("{prefix}_digest")).unwrap()
            && u64::from(item.mode) == number(&value, &format!("{prefix}_mode")).unwrap()
            && item.metadata
                == text(
                    &value,
                    if prefix == "installed" && is_restoration(&value) {
                        "installed_metadata_digest"
                    } else {
                        "native_metadata_digest"
                    },
                )
                .unwrap()
    };
    let status = if outcome == "invalid" {
        "invalid-outcome"
    } else if adding {
        if source_absent
            .as_deref()
            .is_some_and(|parent| parent != text(&value, "source_parent").unwrap())
            || source
                .as_ref()
                .is_some_and(|item| item.parent != text(&value, "source_parent").unwrap())
        {
            "identity-mismatch"
        } else if let Some(source) = source.as_ref().filter(|_| retained_absent) {
            if matching(source, "installed") {
                "applied-arrangement"
            } else if source.installation == text(&value, "installed_file").unwrap() {
                "changed-files"
            } else {
                "identity-mismatch"
            }
        } else if let Some(retained) = &retained {
            if retained.installation != text(&value, "installed_file").unwrap() {
                "identity-mismatch"
            } else if source_absent.is_some() && matching(retained, "installed") {
                if outcome == "absent" {
                    "prepared-arrangement"
                } else {
                    "contradictory-outcome"
                }
            } else if source_absent.is_some() || source.is_some() {
                "changed-files"
            } else {
                "incomplete-observation"
            }
        } else {
            "incomplete-observation"
        }
    } else if removing {
        if source_absent
            .as_deref()
            .is_some_and(|parent| parent != text(&value, "source_parent").unwrap())
            || source
                .as_ref()
                .is_some_and(|item| item.parent != text(&value, "source_parent").unwrap())
        {
            "identity-mismatch"
        } else if let Some(retained) = &retained {
            if retained.installation != text(&value, "source_file").unwrap() {
                "identity-mismatch"
            } else if source_absent.is_some() {
                if matching(retained, "source") {
                    "applied-arrangement"
                } else {
                    "changed-files"
                }
            } else if source.is_some() {
                "changed-files"
            } else {
                "incomplete-observation"
            }
        } else if retained_absent && source.as_ref().is_some_and(|item| matching(item, "source")) {
            if outcome == "absent" {
                "prepared-arrangement"
            } else {
                "contradictory-outcome"
            }
        } else {
            "incomplete-observation"
        }
    } else if let (Some(source), Some(retained)) = (&source, &retained) {
        if source.parent != text(&value, "source_parent").unwrap() {
            "identity-mismatch"
        } else if matching(source, "source") && matching(retained, "installed") {
            if outcome == "absent" {
                "prepared-arrangement"
            } else {
                "contradictory-outcome"
            }
        } else if matching(source, "installed") && matching(retained, "source") {
            "applied-arrangement"
        } else if source.installation == text(&value, "installed_file").unwrap()
            && retained.installation == text(&value, "source_file").unwrap()
        {
            "changed-files"
        } else {
            "identity-mismatch"
        }
    } else {
        "incomplete-observation"
    };
    if recovery.ensure_namespace_identity().is_err() {
        return report(id, "unavailable-directory", Json::Null);
    }
    let parent_policy_matches = if has_parent_policy(&value) {
        crate::managed_file::retained_replacement::parent_policy(
            &history.project().pinned,
            Path::new(text(&value, "path").unwrap()),
        )
        .ok()
        .map(|(digest, mode, parent)| {
            parent == text(&value, "source_parent").unwrap()
                && digest == text(&value, "parent_metadata_digest").unwrap()
                && u64::from(mode) == number(&value, "parent_mode").unwrap()
        })
    } else {
        None
    };
    let mut details = Json::object([
        (
            "operation",
            Json::text(if is_restoration(&value) {
                "restore-retained"
            } else if adding {
                "add-approved"
            } else if removing {
                "remove-approved"
            } else {
                "apply-approved"
            }),
        ),
        (
            "content_is_approved_main",
            Json::Bool(!is_restoration(&value)),
        ),
        ("path", value.get("path").unwrap().clone()),
        ("approved_head", value.get("head").unwrap().clone()),
        (
            "is_current_main",
            Json::Bool(
                workspace
                    .shared_version()
                    .is_some_and(|head| head.to_string() == text(&value, "head").unwrap()),
            ),
        ),
        ("recorded_outcome", Json::text(outcome)),
        ("source", evidence(&source)),
        ("retained", evidence(&retained)),
        (
            "retained_file_is_displaced",
            Json::Bool(retained.as_ref().is_some_and(|item| {
                value.get("source_file").and_then(Json::as_text) == Some(item.installation.as_str())
            })),
        ),
        ("current_exclusions_checked", Json::Bool(false)),
    ]);
    if removing || adding {
        if let Json::Object(fields) = &mut details {
            fields.push((
                "source_absent_parent".into(),
                source_absent.map_or(Json::Null, Json::text),
            ));
            fields.push(("retained_absent".into(), Json::Bool(retained_absent)));
        }
    }
    if has_parent_policy(&value) {
        if let Json::Object(fields) = &mut details {
            fields.push((
                "parent_policy_matches".into(),
                parent_policy_matches.map_or(Json::Null, Json::Bool),
            ));
        }
    }
    let mut result = report(id, status, details);
    if has_parent_policy(&value) && parent_policy_matches != Some(true) {
        if let Json::Object(fields) = &mut result {
            if let Some((_, attention)) = fields
                .iter_mut()
                .find(|(key, _)| key == "attention_required")
            {
                *attention = Json::Bool(true);
            }
        }
    }
    result
}

pub(super) fn inspect_recovery(
    history: &ProvisionedAttachment,
    store: PinnedWorkspaceRoot,
    recovery_root: &Path,
    selected: Option<&str>,
    trusted: &TrustedReviewers,
    limits: ObservationLimits,
) -> io::Result<Json> {
    limits.validate()?;
    if selected.is_some_and(|id| !transaction(id)) {
        return Err(invalid("invalid recovery transaction identity"));
    }
    let root = external_store(recovery_root, history.project())?;
    if root.try_clone_directory()?.metadata()?.permissions().mode() & 0o077 != 0 {
        return Err(invalid("recovery root must be private to its owner"));
    }
    history.project().with_review_history(
        history.metadata_path(),
        store,
        trusted,
        |workspace, store| {
            let page = PAGE.min(limits.entries);
            let (names, more) = if let Some(id) = selected {
                (vec![id.into()], false)
            } else {
                root.filesystem()
                    .read_directory_prefix(Path::new(""), page)?
            };
            let mut remaining = limits.bytes;
            let mut entries = Vec::new();
            for name in names.iter().take(page) {
                if let Some(id) = name.to_str().filter(|id| group_identity(id)) {
                    // Discovery is only a reference, never a verified group or mutation capability.
                    entries.push(report(id, "group-reference", Json::Null));
                    continue;
                }
                let Some(id) = name.to_str().filter(|name| transaction(name)) else {
                    entries.push(report("", "unrecognized-directory-entry", Json::Null));
                    continue;
                };
                entries.push(inspect(
                    history,
                    store,
                    &root,
                    workspace,
                    trusted,
                    id,
                    limits,
                    &mut remaining,
                ));
            }
            root.ensure_namespace_identity()?;
            Ok(Json::object([
                (
                    "schema",
                    Json::text("mesh.attachment-integration-recovery/v1"),
                ),
                ("project", Json::text(history.id())),
                ("entries", Json::Array(entries)),
                ("more", Json::Bool(more)),
                ("live_content_budget_remaining", Json::Number(remaining)),
                ("automatic_replay", Json::Bool(false)),
                ("write_authority", Json::Bool(false)),
            ]))
        },
    )
}
