//! Native publication commit. Human presence is completed before entering this module.
use super::*;
use crate::{dependency_policy::NativePublicationClaim, TrustedReviewers};
use mesh_cas::{Blake3, Cas, DurableFs as _};
use mesh_store::{frame_record, DependencyKind, DependencyRecord, StoredRecord};
use std::{
    fs::File,
    io::{Read as _, Seek as _, Write as _},
    os::unix::fs::{MetadataExt as _, PermissionsExt as _},
    path::Path,
};
const PENDING: &str = "native-publication.pending";
const MAX_JOURNAL: usize = 64 * 1024 * 1024;
const ZERO: RecordDigest = RecordDigest::from_bytes([0; 32]);

/// A verified, synchronized owner-journal publication; never a new permission or grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePublicationCommit {
    record: RecordDigest,
    result: RecordDigest,
    revision: u64,
}
impl NativePublicationCommit {
    /// The exact durable publication payload.
    pub fn record(&self) -> RecordDigest {
        self.record
    }
    /// The resulting accepted main.
    pub fn head(&self) -> RecordDigest {
        self.result
    }
    /// The per-work publication revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}
impl From<NativePublicationClaim> for NativePublicationCommit {
    fn from(c: NativePublicationClaim) -> Self {
        Self {
            record: c.record.payload,
            result: c.result,
            revision: c.revision,
        }
    }
}
#[derive(PartialEq, Eq)]
struct Selection {
    claim: NativePublicationClaim,
    payload: Vec<u8>,
    existing: bool,
}
#[derive(Clone, Copy)]
pub(in crate::project_attachment) enum Step {
    Staged,
    Appended,
}

fn intent(
    request: RecordDigest,
    identity: (u64, u64),
    before: &[u8],
    payload: RecordDigest,
) -> String {
    Json::object([
        ("schema", Json::text("mesh.native-publication-intent/v1")),
        ("request", Json::text(request.to_hex())),
        ("journal_device", Json::text(format!("{:016x}", identity.0))),
        ("journal_inode", Json::text(format!("{:016x}", identity.1))),
        ("journal_bytes", Json::Number(before.len() as u64)),
        ("journal_digest", Json::text(hash(before).to_hex())),
        ("payload", Json::text(payload.to_hex())),
    ])
    .encode()
}
fn journal_bytes(file: &mut File) -> io::Result<Vec<u8>> {
    file.rewind()?;
    let mut bytes = Vec::new();
    (&mut *file)
        .take((MAX_JOURNAL + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_JOURNAL {
        return Err(invalid("publication history exceeds bound"));
    }
    Ok(bytes)
}
impl PrivateContext<'_> {
    #[allow(clippy::too_many_arguments)]
    fn select_publication(
        &self,
        storage: &AttachmentStorage,
        work: &ProvisionedAttachment,
        guard: &WorkspaceInitializationGuard,
        request: RecordDigest,
        review: RecordDigest,
        receipt: &[u8],
        trusted: &TrustedReviewers,
    ) -> io::Result<Selection> {
        self.with_replayed_history(storage, work, trusted, guard, |history, owner| {
            let cas = Cas::<_, Blake3>::with_filesystem(
                self.owner.metadata_path(),
                self.owner.store.filesystem().read_only(),
            )
            .map_err(error)?;
            let policy = owner.policy();
            let binding = policy
                .bound_review(review)
                .ok_or_else(|| invalid("publication review is unavailable"))?;
            // This verifies selected native work, configuration and exact prior-main ancestry.
            let verified = history
                .check_receipt(&binding, receipt, trusted)
                .map_err(error)?;
            if let Some(record) = policy.native_request(request) {
                let claim = policy.publication_claim(record.payload).ok_or_else(|| {
                    invalid("publication request was reused for another operation")
                })?;
                if claim.review.record() != review || claim.receipt != hash(receipt) {
                    return Err(invalid("publication retry differs from original ceremony"));
                }
                // Every durable claim was independently replayed above. Present eligibility and
                // present main cannot invalidate an exact retry of an already committed receipt.
                return Ok(Selection {
                    claim,
                    payload: super::super::dependency_transaction::read_payload(
                        &cas,
                        record.payload,
                        65536,
                    )?,
                    existing: true,
                });
            }
            let graph = self.graph(storage, work, binding.evidence().output().2, guard)?;
            if graph.review_output() != binding.evidence().output()
                || policy.review_graph(binding.evidence().snapshot()) != Some(graph.digest())
                || !policy.review_decisions_current(binding.evidence().snapshot())
            {
                return Err(invalid("publication graph or exact decisions changed"));
            }
            let (head, bundle) = history
                .current_review_bundle(binding.evidence())
                .map_err(error)?;
            if head != binding.canonical() || bundle != binding.review().bundle {
                return Err(invalid("publication review is stale against verified main"));
            }
            let prior = history.verified_publication();
            let revision = prior
                .map_or(Some(1), |p| p.revision.checked_add(1))
                .ok_or_else(|| invalid("publication revision exhausted"))?;
            let (ordinal, previous) = policy
                .native_head()
                .ok_or_else(|| invalid("publication enrollment is absent"))?;
            let ordinal = ordinal
                .checked_add(1)
                .ok_or_else(|| invalid("publication history exhausted"))?;
            let body = Json::object([
                ("request", Json::text(request.to_hex())),
                ("revision", Json::Number(revision)),
                (
                    "previous",
                    Json::text(prior.map_or(ZERO, |p| p.record.payload).to_hex()),
                ),
                ("review", Json::text(review.to_hex())),
                ("receipt", Json::text(hash(receipt).to_hex())),
                (
                    "result",
                    Json::text(
                        RecordDigest::from_bytes(
                            *verified.context().reviewed_actor_head().as_bytes(),
                        )
                        .to_hex(),
                    ),
                ),
                (
                    "credential",
                    Json::text(
                        RecordDigest::from_bytes(*verified.credential().id().as_bytes()).to_hex(),
                    ),
                ),
                (
                    "challenge",
                    Json::text(RecordDigest::from_bytes(*verified.challenge()).to_hex()),
                ),
            ]);
            let payload = Json::object([
                ("schema", Json::text("mesh.dependency-policy/v5")),
                ("authority", Json::text(owner.binding().authority.to_hex())),
                ("revision", Json::Number(ordinal)),
                ("previous", Json::text(previous.to_hex())),
                (
                    "kind",
                    Json::Number(DependencyKind::Publication.code().into()),
                ),
                ("body", body),
            ])
            .encode()
            .into_bytes();
            let record = DependencyRecord {
                authority: owner.binding().authority,
                revision: ordinal,
                previous,
                payload: hash(&payload),
                kind: DependencyKind::Publication,
            };
            let mut projected = policy.clone();
            // Includes global unused challenge, exact per-input decision vector and per-work main.
            projected.apply(record, &payload).map_err(error)?;
            let claim = projected
                .publication_claim(record.payload)
                .ok_or_else(|| invalid("publication projection is missing"))?;
            Ok(Selection {
                claim,
                payload,
                existing: false,
            })
        })
    }
}
impl AttachmentStorage {
    /// Commit an exact already-signed human approval using complete native custody and trust.
    /// No caller should wait for a person or provider inside this call. This development API
    /// is not exposed to agents or runtime controls; torn-frame recovery remains refused.
    pub fn commit_native_publication(
        &self,
        work_id: &str,
        request: RecordDigest,
        review: RecordDigest,
        receipt: &[u8],
        trusted: &TrustedReviewers,
    ) -> io::Result<NativePublicationCommit> {
        self.commit_native_publication_with_io(
            work_id,
            request,
            review,
            receipt,
            trusted,
            |_, _, _| Ok(()),
            |f| f.sync_all(),
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(in crate::project_attachment) fn commit_native_publication_with_io(
        &self,
        work_id: &str,
        request: RecordDigest,
        review: RecordDigest,
        receipt: &[u8],
        trusted: &TrustedReviewers,
        mut hook: impl FnMut(Step, &mut File, &[u8]) -> io::Result<()>,
        mut sync: impl FnMut(&File) -> io::Result<()>,
    ) -> io::Result<NativePublicationCommit> {
        if request == ZERO || review == ZERO || receipt.is_empty() || receipt.len() > 65536 {
            return Err(invalid("invalid native publication request"));
        }
        let owner = self.reopen(&self.candidate_owning_root(work_id)?)?;
        let work = self.reopen(work_id)?;
        let owner_selection = self.prepare_dependency_work(&owner, &owner)?;
        let hints = self.catalog_discovery_hints(&owner)?;
        let discovery = {
            let guard =
                crate::workspace_custody::lock_workspace_initialization_set(&owner_selection.roots)
                    .map_err(error)?;
            self.private_discovery(&owner, work_id, &guard, hints)?
        };
        let works = discovery
            .works
            .keys()
            .map(|id| self.reopen(id))
            .collect::<io::Result<Vec<_>>>()?;
        let available = works.iter().collect::<Vec<_>>();
        let prepared = self.prepare_dependency_graph(&owner, &work, ZERO, &available)?;
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&prepared.roots)
            .map_err(error)?;
        let rediscover = || {
            self.private_discovery(
                &owner,
                work_id,
                &guard,
                self.catalog_discovery_hints(&owner)?,
            )
        };
        if rediscover()? != discovery {
            return Err(invalid("publication roots changed"));
        }
        super::super::detachment::ensure_attached(&owner.store)?;
        super::super::detachment::ensure_attached(&work.store)?;
        match read_private_in_store(&owner.store, super::super::dependency_decision::PENDING) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
            Ok(_) => {
                return Err(invalid(
                    "pending native control must recover before publication",
                ))
            }
        }

        super::super::history::dependency_capture::ensure_no_pending_capture(&owner.store)?;
        let context = PrivateContext::resolve(self, &owner, &works, &guard)?;
        let selected =
            context.select_publication(self, &work, &guard, request, review, receipt, trusted)?;
        let mut journal = owner
            .store
            .open_existing_record_file(Path::new(crate::RECORD_FILE_NAME))?;
        let before = journal_bytes(&mut journal)?;
        context
            .read(&owner, &guard)?
            .proof
            .verify(&owner.store, &journal, &before)?;
        let metadata = journal.metadata()?;
        let identity = (metadata.dev(), metadata.ino());
        let pending = match read_private_in_store(&owner.store, PENDING) {
            Ok(p) => Some(p),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let frame = frame_record(&StoredRecord::Dependency(selected.claim.record));
        if selected.existing {
            if let Some(raw) = &pending {
                let value = Json::parse(raw).map_err(error)?;
                let n = value
                    .get("journal_bytes")
                    .and_then(Json::as_u64)
                    .filter(|n| *n <= before.len() as u64)
                    .ok_or_else(|| invalid("invalid completed publication intent"))?
                    as usize;
                if *raw
                    != intent(
                        request,
                        identity,
                        &before[..n],
                        selected.claim.record.payload,
                    )
                    || !before[n..].starts_with(&frame)
                {
                    return Err(invalid("publication retry has foreign pending evidence"));
                }
            }
            sync(&journal)?;
            if rediscover()? != discovery {
                return Err(invalid("publication retry roots changed"));
            }
            if PrivateContext::resolve(self, &owner, &works, &guard)?
                .select_publication(self, &work, &guard, request, review, receipt, trusted)?
                != selected
            {
                return Err(invalid("publication retry changed"));
            }
            if let Some(raw) = pending {
                if read_private_in_store(&owner.store, PENDING)? != raw {
                    return Err(invalid("publication intent changed"));
                }
                owner.store.filesystem().remove_file(Path::new(PENDING))?;
            }
            owner.store.sync()?;
            guard.ensure_current().map_err(error)?;
            return Ok(selected.claim.into());
        }
        if before.len().saturating_add(frame.len()) > MAX_JOURNAL {
            return Err(invalid("publication append exceeds bound"));
        }
        let expected = intent(request, identity, &before, selected.claim.record.payload);
        if pending.as_ref().is_some_and(|raw| raw != &expected) {
            return Err(invalid("another publication requires recovery"));
        }
        let cas =
            Cas::<_, Blake3>::with_filesystem(owner.metadata_path(), owner.store.filesystem())
                .map_err(error)?;
        cas.promote(receipt.to_vec()).map_err(error)?;
        cas.promote(selected.payload.clone()).map_err(error)?;
        if pending.is_none() {
            owner.store.filesystem().write_new_file(
                Path::new(PENDING),
                expected.as_bytes(),
                std::fs::Permissions::from_mode(0o600),
            )?;
        }
        owner.store.filesystem().sync_file(Path::new(PENDING))?;
        owner.store.sync()?;
        hook(Step::Staged, &mut journal, &frame)?;
        let after = PrivateContext::resolve(self, &owner, &works, &guard)?;
        if rediscover()? != discovery
            || after.select_publication(self, &work, &guard, request, review, receipt, trusted)?
                != selected
            || read_private_in_store(&owner.store, PENDING)? != expected
            || super::super::dependency_transaction::read_payload(
                &cas,
                selected.claim.receipt,
                65536,
            )? != receipt
            || super::super::dependency_transaction::read_payload(
                &cas,
                selected.claim.record.payload,
                65536,
            )? != selected.payload
        {
            return Err(invalid("publication evidence changed before append"));
        }
        let observed = journal_bytes(&mut journal)?;
        context
            .read(&owner, &guard)?
            .proof
            .verify(&owner.store, &journal, &observed)?;
        if observed != before {
            return Err(invalid("publication journal changed before append"));
        }
        guard.ensure_current().map_err(error)?;
        journal.write_all(&frame)?;
        // Sole durable commit point. Neither the intent nor staged CAS objects constitute approval.
        sync(&journal)?;
        hook(Step::Appended, &mut journal, &frame)?;
        let replay = PrivateContext::resolve(self, &owner, &works, &guard)?;
        let completed =
            replay.select_publication(self, &work, &guard, request, review, receipt, trusted)?;
        if !completed.existing
            || completed.claim != selected.claim
            || completed.payload != selected.payload
        {
            return Err(invalid("publication failed exact durable replay"));
        }
        if read_private_in_store(&owner.store, PENDING)? != expected {
            return Err(invalid("publication intent changed after append"));
        }
        owner.store.filesystem().remove_file(Path::new(PENDING))?;
        owner.store.sync()?;
        guard.ensure_current().map_err(error)?;
        Ok(selected.claim.into())
    }
}
