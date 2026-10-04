//! Native enrollment transaction. No renderer, agent or CLI entry point invokes this yet.
use super::{
    dependency_enrollment::read_private, history::HISTORY, invalid, read_receipt,
    ProvisionedAttachment,
};
use crate::dependency_policy::{DependencyPolicyHistory, NativeDependencyBinding};
use crate::ipc::Json;
use crate::root_authority::PinnedRootFs;
use crate::workspace::{workspace_installation, RECORD_FILE_NAME};
use mesh_cas::{Blake3, Cas, ContentDigest as _, Digest32, DurableFs as _};
use mesh_store::{
    frame_record, scan_journal, DependencyKind, DependencyRecord, RecordDigest, StoredRecord,
};
use std::fs::File;
use std::io::{self, Read as _, Seek as _, Write as _};
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;

const INTENT_SCHEMA: &str = "mesh.dependency-enrollment-intent/v1";
const FENCE_SCHEMA: &str = "mesh.attachment-history/v3";
const MAX_JOURNAL: usize = 64 * 1024 * 1024;
const MAX_INTENT: usize = 4096;
const ZERO: RecordDigest = RecordDigest::from_bytes([0; 32]);

/// Durable enrollment facts, not a grant, publication permission or policy-aware writer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeDependencyEnrollment {
    binding: NativeDependencyBinding,
    enrollment: RecordDigest,
}
impl NativeDependencyEnrollment {
    /// Independently selected native project and installation binding.
    pub fn binding(&self) -> NativeDependencyBinding {
        self.binding
    }
    /// Exact canonical enrollment payload retained in the owning store.
    pub fn enrollment(&self) -> RecordDigest {
        self.enrollment
    }
}

#[derive(Clone, Copy)]
enum Step {
    Staged,
    ManagedFenced,
    Fenced,
    Appended,
}

impl ProvisionedAttachment {
    /// Enroll this native registration with exact crash recovery. Validated immutable inspection
    /// is supported; complete control/publication integration must precede user-facing enrollment.
    /// Source files, Git and old journal bytes are never rewritten.
    pub fn enroll_dependency_history(&self) -> io::Result<NativeDependencyEnrollment> {
        self.enroll_dependency_with_hook(|_, _, _| Ok(()))
    }

    fn enroll_dependency_with_hook(
        &self,
        hook: impl FnMut(Step, &mut File, &[u8]) -> io::Result<()>,
    ) -> io::Result<NativeDependencyEnrollment> {
        self.enroll_dependency_with_io(hook, |file| file.sync_all())
    }

    fn enroll_dependency_with_io(
        &self,
        mut hook: impl FnMut(Step, &mut File, &[u8]) -> io::Result<()>,
        mut sync: impl FnMut(&File) -> io::Result<()>,
    ) -> io::Result<NativeDependencyEnrollment> {
        self.attachment.ensure_current()?;
        self.store.ensure_namespace_identity()?;
        let _guard =
            crate::workspace_custody::lock_workspace_initialization(&self.store).map_err(error)?;
        self.check_dependency_registration()?;
        super::detachment::ensure_attached(&self.store)?;
        let mut journal = self
            .store
            .open_existing_record_file(Path::new(RECORD_FILE_NAME))?;
        self.check_dependency_journal(&journal)?;
        let journal_metadata = journal.metadata()?;
        let journal_identity = (journal_metadata.dev(), journal_metadata.ino());
        let bytes = read_journal(&mut journal)?;
        let previous = read_private(self, HISTORY)?;
        let parsed = Json::parse(&previous).map_err(error)?;
        let recovering = parsed.get("schema").and_then(Json::as_text) == Some(FENCE_SCHEMA);
        // Completed enrollment remains idempotent after native control advances the ledger.
        // A torn later decision must recover through its own intent, never through enrollment.
        if recovering {
            if let Ok((_, Some(proof))) = self
                .attachment
                .read_configuration(self.metadata_path(), &self.store)
            {
                if proof.policy().len() > 1 {
                    proof.verify(&self.store, &journal, &bytes)?;
                    let scan = scan_journal(&bytes).map_err(error)?;
                    let enrollment = scan
                        .records()
                        .iter()
                        .find_map(|record| match record {
                            StoredRecord::Dependency(record)
                                if record.kind == DependencyKind::Enrollment =>
                            {
                                Some(record.payload)
                            }
                            _ => None,
                        })
                        .ok_or_else(|| invalid("completed history is missing enrollment"))?;
                    sync(&journal)?;
                    return Ok(NativeDependencyEnrollment {
                        binding: proof.binding(),
                        enrollment,
                    });
                }
            }
        }
        let project = digest(self.id())?;
        let identity = self.store.identity()?;
        let installation = workspace_installation(identity, identity);
        let installation = digest(
            installation
                .strip_prefix("blake3:")
                .ok_or_else(|| invalid("invalid native installation"))?,
        )?;
        let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
            self.metadata_path(),
            self.store.filesystem(),
        )
        .map_err(error)?;
        let (intent, authority) = if recovering {
            let authority = digest(text(&parsed, "dependency_authority")?)?;
            let intent = read_payload(&cas, authority, MAX_INTENT)?;
            (intent, authority)
        } else {
            let scan = scan_journal(&bytes).map_err(error)?;
            if bytes.len() > MAX_JOURNAL
                || scan.tail().is_fragment()
                || scan
                    .records()
                    .iter()
                    .any(|r| matches!(r, StoredRecord::Dependency(_)))
            {
                return Err(invalid(
                    "existing history is not a clean unenrolled journal",
                ));
            }
            let intent = intent_bytes(
                project,
                installation,
                journal_identity,
                bytes.len() as u64,
                hash(&bytes),
            );
            let authority = hash(&intent);
            (intent, authority)
        };
        let decoded = Json::parse(std::str::from_utf8(&intent).map_err(error)?).map_err(error)?;
        let base_len = decoded
            .get("journal_bytes")
            .and_then(Json::as_u64)
            .filter(|n| *n <= MAX_JOURNAL as u64)
            .ok_or_else(|| invalid("invalid enrollment boundary"))? as usize;
        let base_digest = digest(text(&decoded, "journal_digest")?)?;
        if intent
            != intent_bytes(
                project,
                installation,
                journal_identity,
                base_len as u64,
                base_digest,
            )
            || base_len > bytes.len()
            || hash(&bytes[..base_len]) != base_digest
        {
            return Err(invalid("enrollment intent does not match native history"));
        }
        let base_scan = scan_journal(&bytes[..base_len]).map_err(error)?;
        if base_scan.tail().is_fragment()
            || base_scan
                .records()
                .iter()
                .any(|r| matches!(r, StoredRecord::Dependency(_)))
        {
            return Err(invalid("enrollment base is not clean legacy history"));
        }
        let basis = if recovering {
            text(&parsed, "previous_binding")?.to_owned()
        } else {
            previous.clone()
        };
        self.attachment.history_configuration_with_previous(
            &self.store,
            None,
            Some(basis.clone()),
        )?;
        let marker = Json::object([
            ("schema", Json::text(FENCE_SCHEMA)),
            ("previous_binding", Json::text(&basis)),
            ("dependency_authority", Json::text(authority.to_hex())),
        ])
        .encode();
        if recovering && marker != previous {
            return Err(invalid("enrollment fence conflicts with its intent"));
        }
        let binding = NativeDependencyBinding {
            authority,
            project,
            installation,
        };
        let payload = Json::object([
            ("schema", Json::text("mesh.dependency-policy/v1")),
            ("authority", Json::text(authority.to_hex())),
            ("revision", Json::Number(1)),
            ("previous", Json::text(ZERO.to_hex())),
            ("kind", Json::Number(0)),
            (
                "body",
                Json::object([
                    ("project", Json::text(project.to_hex())),
                    ("installation", Json::text(installation.to_hex())),
                ]),
            ),
        ])
        .encode()
        .into_bytes();
        let record = DependencyRecord {
            authority,
            revision: 1,
            previous: ZERO,
            payload: hash(&payload),
            kind: DependencyKind::Enrollment,
        };
        let mut policy = DependencyPolicyHistory::new(binding).map_err(error)?;
        policy.apply(record, &payload).map_err(error)?;
        let frame = frame_record(&StoredRecord::Dependency(record));
        let suffix = &bytes[base_len..];
        if suffix.len() > frame.len() || !frame.starts_with(suffix) {
            return Err(invalid("journal suffix is not this enrollment transaction"));
        }
        if !recovering {
            cas.promote(intent.clone()).map_err(error)?;
            cas.promote(payload.clone()).map_err(error)?;
        }
        if read_payload(&cas, authority, MAX_INTENT)? != intent
            || read_payload(&cas, record.payload, MAX_INTENT)? != payload
        {
            return Err(invalid("enrollment payload staging changed"));
        }
        hook(Step::Staged, &mut journal, &frame)?;
        let managed_fence = crate::DependencyEnrollmentFence::prepare_while_initialized(
            self.metadata_path(),
            &format!("blake3:{}", installation.to_hex()),
            authority,
        )
        .map_err(error)?;
        hook(Step::ManagedFenced, &mut journal, &frame)?;
        if !recovering {
            // Reuses registration/history validation and durable old-reader fencing under the
            // already-held native store lock. Never expose a window between fencing and append.
            drop(self.prepare_dependency_enrollment(authority)?);
        } else {
            self.store.filesystem().sync_file(Path::new(HISTORY))?;
            self.store.sync()?;
        }
        self.check_dependency_registration()?;
        self.check_dependency_journal(&journal)?;
        if read_private(self, HISTORY)? != marker || read_journal(&mut journal)? != bytes {
            return Err(invalid("enrollment state changed before append"));
        }
        managed_fence.ensure_current().map_err(error)?;
        hook(Step::Fenced, &mut journal, &frame)?;
        journal.write_all(&frame[suffix.len()..])?;
        sync(&journal)?;
        hook(Step::Appended, &mut journal, &frame)?;
        self.check_dependency_registration()?;
        self.check_dependency_journal(&journal)?;
        if read_private(self, HISTORY)? != marker {
            return Err(invalid("enrollment fence changed"));
        }
        managed_fence.ensure_current().map_err(error)?;
        let committed = read_journal(&mut journal)?;
        if committed.len() != base_len + frame.len()
            || committed[..base_len] != bytes[..base_len]
            || committed[base_len..] != frame
        {
            return Err(invalid("enrollment append was not retained exactly"));
        }
        let scan = scan_journal(&committed).map_err(error)?;
        if scan.tail().is_fragment()
            || scan.records().last() != Some(&StoredRecord::Dependency(record))
        {
            return Err(invalid("enrollment replay is incomplete"));
        }
        let mut replay = DependencyPolicyHistory::new(binding).map_err(error)?;
        replay
            .apply(record, &read_payload(&cas, record.payload, MAX_INTENT)?)
            .map_err(error)?;
        Ok(NativeDependencyEnrollment {
            binding,
            enrollment: record.payload,
        })
    }

    fn check_dependency_registration(&self) -> io::Result<()> {
        self.attachment.ensure_current()?;
        self.store.ensure_namespace_identity()?;
        if read_receipt(&self.store)? != self.attachment.receipt()?.encode() {
            return Err(invalid("native dependency registration changed"));
        }
        Ok(())
    }
    fn check_dependency_journal(&self, file: &File) -> io::Result<()> {
        let held = file.metadata()?;
        let named = self
            .store
            .filesystem()
            .inspect_entry(Path::new(RECORD_FILE_NAME))?
            .metadata()?;
        if !held.is_file()
            || held.nlink() != 1
            || (held.dev(), held.ino()) != (named.dev(), named.ino())
        {
            return Err(invalid("native dependency journal identity changed"));
        }
        Ok(())
    }
}
pub(super) fn intent_bytes(
    project: RecordDigest,
    installation: RecordDigest,
    journal: (u64, u64),
    bytes: u64,
    digest: RecordDigest,
) -> Vec<u8> {
    Json::object([
        ("schema", Json::text(INTENT_SCHEMA)),
        ("project", Json::text(project.to_hex())),
        ("installation", Json::text(installation.to_hex())),
        ("journal_device", Json::text(format!("{:016x}", journal.0))),
        ("journal_inode", Json::text(format!("{:016x}", journal.1))),
        ("journal_bytes", Json::Number(bytes)),
        ("journal_digest", Json::text(digest.to_hex())),
    ])
    .encode()
    .into_bytes()
}
pub(super) fn text<'a>(value: &'a Json, key: &str) -> io::Result<&'a str> {
    value
        .get(key)
        .and_then(Json::as_text)
        .ok_or_else(|| invalid("missing enrollment identity"))
}
pub(super) fn digest(value: &str) -> io::Result<RecordDigest> {
    let digest = RecordDigest::parse_hex(value).map_err(error)?;
    if digest == ZERO || digest.to_hex() != value {
        return Err(invalid("invalid enrollment identity"));
    }
    Ok(digest)
}
pub(super) fn hash(bytes: &[u8]) -> RecordDigest {
    RecordDigest::from_bytes(*Blake3::digest_bytes(bytes).as_bytes())
}
fn read_journal(file: &mut File) -> io::Result<Vec<u8>> {
    file.rewind()?;
    let mut bytes = Vec::new();
    file.take((MAX_JOURNAL + 1025) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_JOURNAL + 1024 {
        return Err(invalid("enrollment journal exceeds its bound"));
    }
    Ok(bytes)
}
pub(super) fn read_payload(
    cas: &Cas<PinnedRootFs, Blake3>,
    digest: RecordDigest,
    limit: usize,
) -> io::Result<Vec<u8>> {
    let id = Digest32::from_bytes(*digest.as_bytes());
    let file = cas.filesystem().read_file(&cas.layout().chunk_path(&id))?;
    if !file.metadata()?.is_file() {
        return Err(invalid("invalid enrollment payload file"));
    }
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > limit || hash(&bytes) != digest {
        return Err(invalid("enrollment payload is missing or corrupt"));
    }
    Ok(bytes)
}
fn error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(test)]
mod tests;
