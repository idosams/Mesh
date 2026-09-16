//! A deliberately small support bundle that is safe to preview before it leaves the device.
//!
//! This first bundle is an allowlist, not a redaction pass. It carries only the structured crash
//! report whose contract already excludes paths, record content and actor names. Configuration,
//! ledger excerpts, file content and key material are named as excluded until each has its own
//! verifiable redaction contract. Adding an unverified class would be worse than omitting it.

use std::fs::File;
use std::io;
use std::path::Path;

use mesh_types::{Blake3, ContentDigest as _};

use crate::crash_report::{pin_support_journal, SupportFileIdentity};
use crate::ipc::json::Json;
use crate::recovery::RecoveryOutcome;
use crate::CrashReport;

const TOP_KEYS: [&str; 6] = [
    "schema",
    "producer",
    "workspace_correlation",
    "included",
    "excluded",
    CrashReport::BUNDLE_SECTION,
];
const PRODUCER_KEYS: [&str; 2] = ["component", "version"];
const CRASH_KEYS: [&str; 13] = [
    "section",
    "serving",
    "severity",
    "saved_records",
    "boundary_bytes",
    "unfinished_bytes",
    "checkpoint_state_available",
    "meaningful_checkpoint_through",
    "recovery_preserved_through",
    "open_activity_from",
    "open_activity_through",
    "elapsed_ms",
    "sentence",
];
const INCLUDED: [&str; 1] = [CrashReport::BUNDLE_SECTION];
const EXCLUDED: [&str; 5] = [
    "configuration",
    "event-ledger",
    "file-content",
    "key-material",
    "raw-paths",
];
const MAX_SUPPORT_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;
const MIN_JOURNAL_FRAME_BYTES: u64 = 40;
const RECOVERY_BUDGET_MS: u64 = 5_000;
const MAX_SAFE_JSON_INTEGER: u64 = 9_007_199_254_740_991;

/// The exact local document a person previews and may then choose to share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupportBundle {
    document: Json,
}

/// The requested live workspace journal, pinned until a daemon reply is accepted.
///
/// The file descriptor is intentionally retained and not exposed. Its lifetime prevents the
/// operating system from recycling the journal identity while `meshctl` compares it with the
/// daemon's already-open journal correlation.
#[derive(Debug)]
pub struct PinnedSupportTarget {
    _journal: File,
    correlation: String,
}

impl PinnedSupportTarget {
    /// The exact support correlation expected from the daemon's open journal generation.
    #[must_use]
    pub fn correlation(&self) -> &str {
        &self.correlation
    }
}

impl SupportBundle {
    /// The scanner contract for this document shape.
    pub const SCHEMA: &'static str = "mesh-support-bundle/v1";

    /// Pin the requested workspace's current journal before asking a daemon for its live report.
    ///
    /// This uses the same no-follow, nonblocking descriptor admission as immutable support
    /// inspection. The returned handle must remain alive until the reply correlation is checked.
    ///
    /// # Errors
    ///
    /// Refuses a missing, linked, special, replaced, or platform-unidentifiable journal.
    pub fn pin_live_target(workspace: &Path) -> io::Result<PinnedSupportTarget> {
        let (journal, identity) = pin_support_journal(workspace)?;
        Ok(PinnedSupportTarget {
            _journal: journal,
            correlation: workspace_correlation(workspace, Some(identity)),
        })
    }

    /// Collect the safe subset available from a workspace, even when that workspace cannot open.
    ///
    /// This never returns an open error: a failed open becomes the same sanitized crash report the
    /// daemon uses for start-up diagnostics. In particular, the low-level failure text is not
    /// copied into the bundle because it can contain a path.
    #[must_use]
    pub fn collect(workspace: &Path) -> Self {
        let inspection = CrashReport::inspect_read_only(workspace);
        Self::from_report(workspace, inspection.source_identity, &inspection.report)
    }

    /// Compose the same safe document from a running daemon's already-verified state.
    ///
    /// A live WAL family is intentionally unsafe for the endpoint-free immutable inspector, but
    /// it is ordinary state to the process that already opened and restored it. The running path
    /// therefore consumes only the report held by that process; it never reopens SQLite and never
    /// relaxes the stopped-workspace refusal.
    #[must_use]
    pub(crate) fn from_live_report(
        workspace: &Path,
        source_identity: Option<(u64, u64)>,
        report: &CrashReport,
    ) -> Self {
        let source_identity =
            source_identity.map(|(device, inode)| SupportFileIdentity { device, inode });
        let document = Self::from_report(workspace, source_identity, report).document;
        Self { document }
    }

    /// The exact one-line preview. There is no separate serializer on a later sharing path.
    #[must_use]
    pub fn preview(&self) -> String {
        self.document.encode()
    }

    /// The document for callers that need to embed the preview without reparsing it.
    #[must_use]
    pub const fn document(&self) -> &Json {
        &self.document
    }

    /// Validate an untrusted live document before any of its bytes reach preview output.
    ///
    /// The daemon is a separate local process. Correlation proves which journal it had open; it
    /// does not prove the returned JSON stayed inside this bundle's privacy allowlist. This check
    /// mirrors the shipped scanner's closed shape, value domains, and recovery relationships so a
    /// stale, buggy, or replaced service cannot smuggle an extra field or unsafe sentence through
    /// `meshctl support-bundle`.
    ///
    /// # Errors
    ///
    /// Returns one fixed sentence and never reflects an untrusted value.
    pub fn validate_untrusted_preview(document: &Json) -> Result<(), &'static str> {
        if support_document_is_safe(document) {
            Ok(())
        } else {
            Err("the service returned data outside the safe support preview contract")
        }
    }

    fn from_report(
        workspace: &Path,
        source_identity: Option<SupportFileIdentity>,
        report: &CrashReport,
    ) -> Self {
        let document = Json::object([
            ("schema", Json::text(Self::SCHEMA)),
            (
                "producer",
                Json::object([
                    ("component", Json::text("mesh-daemon")),
                    ("version", Json::text(env!("CARGO_PKG_VERSION"))),
                ]),
            ),
            (
                "workspace_correlation",
                Json::text(workspace_correlation(workspace, source_identity)),
            ),
            (
                "included",
                Json::Array(vec![Json::text(CrashReport::BUNDLE_SECTION)]),
            ),
            (
                "excluded",
                Json::Array(
                    [
                        "configuration",
                        "event-ledger",
                        "file-content",
                        "key-material",
                        "raw-paths",
                    ]
                    .into_iter()
                    .map(Json::text)
                    .collect(),
                ),
            ),
            (CrashReport::BUNDLE_SECTION, report.to_bundle_section()),
        ]);
        Self { document }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SupportOutcome {
    Clean,
    Interrupted,
    NothingReadable,
    Unrecoverable,
}

fn support_document_is_safe(document: &Json) -> bool {
    let Some([schema, producer, correlation, included, excluded, crash]) =
        exact_object(document, TOP_KEYS)
    else {
        return false;
    };
    if schema.as_text() != Some(SupportBundle::SCHEMA)
        || !exact_text_array(included, &INCLUDED)
        || !exact_text_array(excluded, &EXCLUDED)
        || !correlation.as_text().is_some_and(is_canonical_correlation)
    {
        return false;
    }

    let Some([component, version]) = exact_object(producer, PRODUCER_KEYS) else {
        return false;
    };
    if component.as_text() != Some("mesh-daemon")
        || version.as_text() != Some(env!("CARGO_PKG_VERSION"))
    {
        return false;
    }

    let Some(
        [section, serving, severity, saved_records, boundary_bytes, unfinished_bytes, checkpoint_available, meaningful, recovery, activity_from, activity_through, elapsed_ms, sentence],
    ) = exact_object(crash, CRASH_KEYS)
    else {
        return false;
    };
    let (
        Some(serving),
        Some(saved_records),
        Some(boundary_bytes),
        Some(unfinished_bytes),
        Some(checkpoint_available),
        Some(elapsed_ms),
        Some(sentence),
    ) = (
        serving.as_bool(),
        saved_records.as_u64(),
        boundary_bytes.as_u64(),
        unfinished_bytes.as_u64(),
        checkpoint_available.as_bool(),
        elapsed_ms.as_u64(),
        sentence.as_text(),
    )
    else {
        return false;
    };
    let Some(meaningful) = optional_positive_sequence(meaningful) else {
        return false;
    };
    let Some(recovery) = optional_positive_sequence(recovery) else {
        return false;
    };
    let Some(activity_from) = optional_positive_sequence(activity_from) else {
        return false;
    };
    let Some(activity_through) = optional_positive_sequence(activity_through) else {
        return false;
    };
    if section.as_text() != Some(CrashReport::BUNDLE_SECTION)
        || !matches!(severity.as_text(), Some("routine" | "notable" | "blocking"))
        || (saved_records == 0) != (boundary_bytes == 0)
        || saved_records
            .checked_mul(MIN_JOURNAL_FRAME_BYTES)
            .is_none_or(|minimum| boundary_bytes < minimum)
        || boundary_bytes
            .checked_add(unfinished_bytes)
            .is_none_or(|total| total > MAX_SUPPORT_JOURNAL_BYTES)
        || [saved_records, boundary_bytes, unfinished_bytes, elapsed_ms]
            .into_iter()
            .any(|value| value > MAX_SAFE_JSON_INTEGER)
        || (!checkpoint_available
            && [meaningful, recovery, activity_from, activity_through]
                .into_iter()
                .any(|value| value.is_some()))
        || (!serving && checkpoint_available)
        || activity_from.is_some() != activity_through.is_some()
        || activity_from
            .zip(activity_through)
            .is_some_and(|(from, through)| from > through)
        || meaningful
            .zip(activity_from)
            .is_some_and(|(meaningful, from)| meaningful >= from)
        || !recovery_relationship_holds(recovery, meaningful, activity_from, activity_through)
    {
        return false;
    }

    let Some(outcome) = support_outcome(saved_records, sentence) else {
        return false;
    };
    match outcome {
        SupportOutcome::Clean => {
            serving
                && unfinished_bytes == 0
                && severity.as_text()
                    == Some(if elapsed_ms < RECOVERY_BUDGET_MS {
                        "routine"
                    } else {
                        "notable"
                    })
        }
        SupportOutcome::Interrupted => {
            serving && unfinished_bytes > 0 && severity.as_text() == Some("notable")
        }
        SupportOutcome::NothingReadable => {
            !serving
                && saved_records == 0
                && boundary_bytes == 0
                && unfinished_bytes > 0
                && severity.as_text() == Some("blocking")
        }
        SupportOutcome::Unrecoverable => !serving && severity.as_text() == Some("blocking"),
    }
}

fn exact_object<'a, const N: usize>(value: &'a Json, expected: [&str; N]) -> Option<[&'a Json; N]> {
    let Json::Object(pairs) = value else {
        return None;
    };
    if pairs.len() != N
        || pairs
            .iter()
            .zip(expected)
            .any(|((found, _), expected)| found != expected)
    {
        return None;
    }
    Some(std::array::from_fn(|index| &pairs[index].1))
}

fn exact_text_array(value: &Json, expected: &[&str]) -> bool {
    value.as_array().is_some_and(|found| {
        found.len() == expected.len()
            && found
                .iter()
                .zip(expected)
                .all(|(found, expected)| found.as_text() == Some(*expected))
    })
}

fn is_canonical_correlation(value: &str) -> bool {
    value.strip_prefix("blake3:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .as_bytes()
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    })
}

fn optional_positive_sequence(value: &Json) -> Option<Option<u64>> {
    match value {
        Json::Null => Some(None),
        Json::Text(text)
            if !text.starts_with('0')
                && text
                    .parse::<u64>()
                    .is_ok_and(|parsed| parsed > 0 && parsed.to_string() == *text) =>
        {
            Some(text.parse().ok())
        }
        _ => None,
    }
}

fn recovery_relationship_holds(
    recovery: Option<u64>,
    meaningful: Option<u64>,
    activity_from: Option<u64>,
    activity_through: Option<u64>,
) -> bool {
    match (recovery, activity_from, activity_through) {
        (Some(recovery), Some(from), Some(through)) => {
            (recovery >= from && recovery <= through)
                || meaningful.is_some_and(|meaningful| recovery <= meaningful)
        }
        (Some(recovery), None, None) => meaningful.is_some_and(|meaningful| recovery <= meaningful),
        (None, _, _) => true,
        _ => false,
    }
}

fn support_outcome(saved_records: u64, sentence: &str) -> Option<SupportOutcome> {
    let digest = mesh_store::Digest16::from_bytes([0; 16]);
    let candidates = [
        (
            SupportOutcome::Clean,
            RecoveryOutcome::Rebuilt {
                records: saved_records,
                rows: 0,
                digest,
            },
        ),
        (
            SupportOutcome::Interrupted,
            RecoveryOutcome::RebuiltAfterAnInterruptedSave {
                records: saved_records,
                rows: 0,
                digest,
                discarded_bytes: 1,
            },
        ),
        (
            SupportOutcome::NothingReadable,
            RecoveryOutcome::NothingDurableToRecover {
                unfinished_bytes: 1,
            },
        ),
        (
            SupportOutcome::Unrecoverable,
            RecoveryOutcome::Unrecoverable {
                detail: String::new(),
            },
        ),
    ];
    candidates.into_iter().find_map(|(kind, outcome)| {
        (crate::user_messages::startup_sentence(&outcome) == sentence).then_some(kind)
    })
}

fn workspace_correlation(workspace: &Path, source_identity: Option<SupportFileIdentity>) -> String {
    let mut tagged = b"mesh-support-bundle-workspace-v1\0".to_vec();
    // The correlation is shared outside the device, so an unkeyed digest of the canonical path is
    // not a redaction: a recipient can hash likely paths and recover the private folder name. A
    // successful inspection therefore binds the operating system identity of the exact opened
    // journal generation to the facts read from it. A non-workspace/failure has no opened journal,
    // so it retains the directory identity used by the original preview contract. Neither form can
    // be reproduced from a pathname dictionary, and an unreachable path still gets the same
    // explicit unavailable identity rather than falling back to private path bytes.
    if let Some(identity) = source_identity {
        tagged.extend_from_slice(b"unix-journal-file-id\0");
        tagged.extend_from_slice(&identity.device.to_be_bytes());
        tagged.extend_from_slice(&identity.inode.to_be_bytes());
    } else {
        append_workspace_identity(workspace, &mut tagged);
    }
    format!("blake3:{}", Blake3::digest_bytes(&tagged).to_hex())
}

#[cfg(unix)]
fn append_workspace_identity(path: &Path, tagged: &mut Vec<u8>) {
    use std::os::unix::fs::MetadataExt as _;

    match std::fs::metadata(path) {
        Ok(metadata) => {
            tagged.extend_from_slice(b"unix-file-id\0");
            tagged.extend_from_slice(&metadata.dev().to_be_bytes());
            tagged.extend_from_slice(&metadata.ino().to_be_bytes());
        }
        Err(_) => tagged.extend_from_slice(b"unavailable\0"),
    }
}

#[cfg(not(unix))]
fn append_workspace_identity(_path: &Path, tagged: &mut Vec<u8>) {
    tagged.extend_from_slice(b"unavailable\0");
}
