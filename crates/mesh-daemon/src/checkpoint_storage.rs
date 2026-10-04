//! Production composition between content-defined chunking, CAS promotion, and durable commits.
//!
//! `mesh-chunking` decides boundaries without touching storage. `mesh-cas` owns steps 1–4 of the
//! durable sequence, and `mesh-store` owns steps 5–11. This module is the service-layer adapter
//! between them; neither storage crate gains an upward dependency.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use mesh_cas::{
    Blake3 as CasBlake3, Cas, CasError, Digest32 as CasDigest, DurableFs, Promotion, PromotionStep,
};
use mesh_chunking::{chunk_bytes, ChunkingConfig};
use mesh_operations::{
    encode_canonical, ActorId, ActorSequence, CausalParents, ChangeSetDraft, HeadDerivation,
    HeadId, Hlc, ManifestId, ObjectId, Operation, PolicyEpoch, PortableMetadata, SessionId,
    Signature as OperationSignature, VersionId, WorkspaceId,
};
use mesh_store::{
    journal_records, Checkpoint, ChunkPromoter, ChunkSlice, DurableCommit, EntityUuid,
    ManifestRecord, OperationRecord, PrivateSaved, RecordDigest, RecordJournal, SequenceError,
    Sqlite, SqliteError, Store,
};
use mesh_types::{
    canonical_digest, Blake3, ChunkRef, ContentDigest, Digest32, FileManifest, PublicKey, Signature,
};

use crate::authenticated_changeset::{AuthenticatedChangeSet, AuthenticatedChangeSetError};
use crate::workspace::OpenWorkspace;
use crate::{ManifestPagingError, ManifestPagingPolicy, PagedManifest};

/// Native signing capability for captured private history. Key custody stays with the host.
/// This capability grants no approval or main-version authority and requires no agent provider.
pub trait CheckpointSigner: Send + Sync {
    /// Public key of the actor attesting to the capture, not necessarily the original file author.
    fn public_key(&self) -> PublicKey;
    /// Sign a canonical private ChangeSet. Errors must contain no secret material.
    fn sign(&self, payload: &mesh_crypto::SigningPayload) -> Result<Signature, String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PromotionPhase {
    Empty,
    Staged,
    Flushed,
    Verified,
    Promoted,
}

impl PromotionPhase {
    const fn name(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Staged => "staged",
            Self::Flushed => "flushed",
            Self::Verified => "verified",
            Self::Promoted => "promoted",
        }
    }
}

struct PendingPromotion<'a, F: DurableFs> {
    byte_length: u64,
    promotion: Promotion<'a, F, CasBlake3>,
}

/// Concrete plan-§6.3 chunk promoter backed by a workspace CAS.
///
/// One value is single-use. Its phase checks make a caller that bypasses [`mesh_store::DurableCommit`]
/// fail closed instead of silently fusing or skipping durability steps.
pub struct CasChunkPromoter<'a, F: DurableFs = mesh_cas::StdFs> {
    cas: &'a Cas<F, CasBlake3>,
    pending: Vec<PendingPromotion<'a, F>>,
    phase: PromotionPhase,
    linked_bytes: u64,
    staged_objects: usize,
    reused_objects: usize,
    reused_digests: Vec<RecordDigest>,
}

impl<F: DurableFs> fmt::Debug for CasChunkPromoter<'_, F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CasChunkPromoter")
            .field("pending", &self.pending.len())
            .field("phase", &self.phase)
            .field("linked_bytes", &self.linked_bytes)
            .finish_non_exhaustive()
    }
}

impl<'a, F: DurableFs> CasChunkPromoter<'a, F> {
    /// Bind one durable sequence to the workspace CAS.
    #[must_use]
    pub const fn new(cas: &'a Cas<F, CasBlake3>) -> Self {
        Self {
            cas,
            pending: Vec::new(),
            phase: PromotionPhase::Empty,
            linked_bytes: 0,
            staged_objects: 0,
            reused_objects: 0,
            reused_digests: Vec::new(),
        }
    }

    /// Content bytes newly linked by this sequence; already-present chunks add zero.
    #[must_use]
    pub const fn linked_bytes(&self) -> u64 {
        self.linked_bytes
    }

    /// Objects that required a temporary write and durability sequence.
    #[must_use]
    pub const fn staged_objects(&self) -> usize {
        self.staged_objects
    }

    /// Objects whose content name was already present and therefore skipped redundant staging.
    ///
    /// This is the same presence observation the old path made at its final link step. It is not
    /// an integrity claim: reads still verify bytes, and the durable sequence rechecks every
    /// reused name at step 4 plus every logical manifest reference at step 5.
    #[must_use]
    pub const fn reused_objects(&self) -> usize {
        self.reused_objects
    }

    fn require(&self, expected: PromotionPhase) -> Result<(), CasChunkPromoterError> {
        if self.phase == expected {
            Ok(())
        } else {
            Err(CasChunkPromoterError::WrongPhase {
                expected: expected.name(),
                actual: self.phase.name(),
            })
        }
    }
}

/// A CAS failure or an out-of-order call to the concrete promoter.
#[derive(Debug)]
pub enum CasChunkPromoterError {
    /// The underlying content-addressed store refused a durability operation.
    Cas(CasError),
    /// A caller tried to skip or repeat one of the four plan steps.
    WrongPhase {
        /// Required predecessor state.
        expected: &'static str,
        /// State in which the call actually arrived.
        actual: &'static str,
    },
    /// A name observed at step 1 disappeared before step 4's promotion boundary.
    ReusedObjectVanished(RecordDigest),
}

impl fmt::Display for CasChunkPromoterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cas(error) => error.fmt(formatter),
            Self::WrongPhase { expected, actual } => {
                write!(
                    formatter,
                    "CAS promoter expected {expected}, found {actual}"
                )
            }
            Self::ReusedObjectVanished(digest) => write!(
                formatter,
                "reused CAS object {digest} vanished before the promotion boundary"
            ),
        }
    }
}

impl std::error::Error for CasChunkPromoterError {}

impl From<CasError> for CasChunkPromoterError {
    fn from(error: CasError) -> Self {
        Self::Cas(error)
    }
}

impl<F: DurableFs> ChunkPromoter for CasChunkPromoter<'_, F> {
    type Error = CasChunkPromoterError;

    fn write_temporary(&mut self, chunks: &[Vec<u8>]) -> Result<Vec<RecordDigest>, Self::Error> {
        self.require(PromotionPhase::Empty)?;
        let cas = self.cas;
        let mut names = Vec::with_capacity(chunks.len());
        self.pending.reserve(chunks.len());
        for bytes in chunks {
            // Constructing the promotion derives the name without writing. The previous path
            // always staged first, then learned at PromotionStep::Link that this exact name was
            // already present and discarded the staging file. Checking the same predicate here
            // removes only that redundant write/fsync/verify cycle. `DurableCommit` still calls
            // `is_durable` after promotion and before the metadata transaction, closing a removal
            // race without weakening the single-writer durability boundary.
            let mut promotion = cas.begin_promotion(bytes.clone());
            let digest = promotion.digest();
            names.push(RecordDigest::from_bytes(*digest.as_bytes()));
            if cas.contains(&digest) {
                self.reused_objects += 1;
                self.reused_digests
                    .push(RecordDigest::from_bytes(*digest.as_bytes()));
                continue;
            }
            promotion.run_through(PromotionStep::Stage)?;
            self.pending.push(PendingPromotion {
                byte_length: bytes.len() as u64,
                promotion,
            });
            self.staged_objects += 1;
        }
        self.phase = PromotionPhase::Staged;
        Ok(names)
    }

    fn flush_temporary(&mut self) -> Result<(), Self::Error> {
        self.require(PromotionPhase::Staged)?;
        for entry in &mut self.pending {
            entry.promotion.run_through(PromotionStep::Flush)?;
        }
        self.phase = PromotionPhase::Flushed;
        Ok(())
    }

    fn verify_temporary(&mut self) -> Result<(), Self::Error> {
        self.require(PromotionPhase::Flushed)?;
        for entry in &mut self.pending {
            entry.promotion.run_through(PromotionStep::Verify)?;
        }
        self.phase = PromotionPhase::Verified;
        Ok(())
    }

    fn promote(&mut self) -> Result<(), Self::Error> {
        self.require(PromotionPhase::Verified)?;
        // This is the exact boundary at which the old path attempted to link every staged object
        // and discovered that a target name already existed. Rechecking here preserves that
        // timing for reused content, including payload and physical-manifest objects that are not
        // direct `ManifestRecord::chunks` references.
        for digest in &self.reused_digests {
            if !self
                .cas
                .contains(&CasDigest::from_bytes(*digest.as_bytes()))
            {
                return Err(CasChunkPromoterError::ReusedObjectVanished(*digest));
            }
        }
        for entry in core::mem::take(&mut self.pending) {
            let promoted = entry.promotion.finish()?;
            if promoted.wrote_content() {
                self.linked_bytes = self.linked_bytes.saturating_add(entry.byte_length);
            }
        }
        self.phase = PromotionPhase::Promoted;
        Ok(())
    }

    fn discard_temporary(&mut self) -> Result<usize, Self::Error> {
        self.require(PromotionPhase::Empty)?;
        self.cas.discard_scratch().map_err(Into::into)
    }

    fn is_durable(&self, digest: &RecordDigest) -> bool {
        self.cas
            .contains(&CasDigest::from_bytes(*digest.as_bytes()))
    }
}

/// One file cut under an explicit policy, ready for a durable commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedCheckpointFile {
    manifest: ManifestRecord,
    chunks: Vec<Vec<u8>>,
    paged: Option<PagedManifest>,
}

impl PreparedCheckpointFile {
    /// Cut bytes, derive their canonical v0 manifest record, and retain each distinct chunk once.
    pub fn from_bytes(
        bytes: &[u8],
        config: &ChunkingConfig,
        paging_policy: ManifestPagingPolicy,
    ) -> Result<Self, ManifestPagingError> {
        let chunked = chunk_bytes(bytes, config);
        let chunking_manifest = chunked.manifest();
        let references = chunking_manifest
            .chunks()
            .iter()
            .map(|reference| {
                ChunkRef::new(
                    Digest32::from_bytes(*reference.content_hash().as_bytes()),
                    reference.offset(),
                    reference.length(),
                )
            })
            .collect::<Vec<_>>();
        let canonical = FileManifest::new(
            chunking_manifest.byte_length(),
            Digest32::from_bytes(*chunking_manifest.content_hash().as_bytes()),
            references,
        );
        let manifest_id = canonical_digest::<Blake3, _>(&canonical);
        let manifest = ManifestRecord {
            id: RecordDigest::from_bytes(*manifest_id.as_bytes()),
            byte_length: canonical.byte_length(),
            content_digest: RecordDigest::from_bytes(*canonical.content_hash().as_bytes()),
            chunks: canonical
                .chunks()
                .iter()
                .map(|reference| ChunkSlice {
                    digest: RecordDigest::from_bytes(*reference.content_hash().as_bytes()),
                    byte_offset: reference.offset(),
                    byte_length: reference.length(),
                })
                .collect(),
        };

        let mut chunks = Vec::new();
        let mut seen = Vec::new();
        for chunk in chunked.chunks() {
            let digest = *chunk.digest().as_bytes();
            if !seen.contains(&digest) {
                seen.push(digest);
                chunks.push(chunk.bytes().to_vec());
            }
        }
        let paged = PagedManifest::prepare(&manifest, paging_policy)?;
        Ok(Self {
            manifest,
            chunks,
            paged,
        })
    }

    /// Canonical manifest record to include in the same metadata transaction.
    #[must_use]
    pub const fn manifest(&self) -> &ManifestRecord {
        &self.manifest
    }

    /// Distinct chunk bodies for steps 1–4, in first-appearance order.
    #[must_use]
    pub fn chunks(&self) -> &[Vec<u8>] {
        &self.chunks
    }

    /// Candidate B physical manifest, when the explicit policy selected it.
    #[must_use]
    pub const fn paged_manifest(&self) -> Option<&PagedManifest> {
        self.paged.as_ref()
    }

    /// Consume the preparation into the two inputs a durable sequence needs.
    #[must_use]
    pub fn into_parts(self) -> (ManifestRecord, Vec<Vec<u8>>, Option<PagedManifest>) {
        (self.manifest, self.chunks, self.paged)
    }
}

/// Every caller-owned field needed to seal one file-version ChangeSet.
///
/// There is deliberately no default implementation. In particular, the composer does not issue
/// actor sequences, choose a policy epoch, infer parents, mint object/version identities, read a
/// clock, or fabricate a signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileVersionCheckpointRequest {
    workspace_id: WorkspaceId,
    actor_id: ActorId,
    session_id: SessionId,
    actor_sequence: ActorSequence,
    causal_parents: CausalParents,
    base_head: HeadId,
    policy_epoch: PolicyEpoch,
    hybrid_logical_time: Hlc,
    object_id: ObjectId,
    version_id: VersionId,
    parent_versions: Vec<VersionId>,
    portable_metadata: PortableMetadata,
    before_write: Vec<Operation>,
    after_write: Vec<Operation>,
    signature: OperationSignature,
    authenticated_public_key: Option<PublicKey>,
}

/// Complete caller-owned context for one authenticated operation-only ChangeSet.
///
/// This is used by native workspace entry management where no new content manifest is needed
/// (rename, move, and delete). There is no default and no unsigned production constructor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AuthenticatedOperationCheckpointRequest {
    workspace_id: WorkspaceId,
    actor_id: ActorId,
    session_id: SessionId,
    actor_sequence: ActorSequence,
    causal_parents: CausalParents,
    base_head: HeadId,
    policy_epoch: PolicyEpoch,
    hybrid_logical_time: Hlc,
    operations: Vec<Operation>,
    actor_public_key: PublicKey,
    signature: Signature,
}

impl AuthenticatedOperationCheckpointRequest {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        workspace_id: WorkspaceId,
        actor_id: ActorId,
        session_id: SessionId,
        actor_sequence: ActorSequence,
        causal_parents: CausalParents,
        base_head: HeadId,
        policy_epoch: PolicyEpoch,
        hybrid_logical_time: Hlc,
        operations: Vec<Operation>,
        actor_public_key: PublicKey,
        signature: Signature,
    ) -> Self {
        Self {
            workspace_id,
            actor_id,
            session_id,
            actor_sequence,
            causal_parents,
            base_head,
            policy_epoch,
            hybrid_logical_time,
            operations,
            actor_public_key,
            signature,
        }
    }
}

impl FileVersionCheckpointRequest {
    /// Bind the complete typed caller context for one `WriteFileVersion` operation.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        workspace_id: WorkspaceId,
        actor_id: ActorId,
        session_id: SessionId,
        actor_sequence: ActorSequence,
        causal_parents: CausalParents,
        base_head: HeadId,
        policy_epoch: PolicyEpoch,
        hybrid_logical_time: Hlc,
        object_id: ObjectId,
        version_id: VersionId,
        parent_versions: Vec<VersionId>,
        portable_metadata: PortableMetadata,
        signature: OperationSignature,
    ) -> Self {
        Self {
            workspace_id,
            actor_id,
            session_id,
            actor_sequence,
            causal_parents,
            base_head,
            policy_epoch,
            hybrid_logical_time,
            object_id,
            version_id,
            parent_versions,
            portable_metadata,
            before_write: Vec::new(),
            after_write: Vec::new(),
            signature,
            authenticated_public_key: None,
        }
    }

    /// Compose exact lifecycle operations around the file-version write in the same atomic
    /// ChangeSet. Native file creation uses `CreateFile` before and `LinkDirectoryEntry` after.
    #[must_use]
    pub(crate) fn around_write(
        mut self,
        before_write: Vec<Operation>,
        after_write: Vec<Operation>,
    ) -> Self {
        self.before_write = before_write;
        self.after_write = after_write;
        self
    }

    /// Attach the verified actor key and signature that will be persisted around the canonical
    /// ChangeSet statement. The actor identifier must equal the public key bytes; the durable save
    /// rechecks that invariant and the signature before writing.
    #[must_use]
    pub fn authenticated(mut self, actor_public_key: PublicKey, signature: Signature) -> Self {
        self.signature = OperationSignature::from_bytes(*signature.as_bytes());
        self.authenticated_public_key = Some(actor_public_key);
        self
    }
}

/// Produce the exact canonical ChangeSet statement an authenticated actor must sign for this
/// file-version request. The durable path recomputes these bytes and verifies the returned
/// signature before storing an envelope.
pub(crate) fn file_version_signing_body<D: HeadDerivation + ?Sized>(
    bytes: &[u8],
    config: &ChunkingConfig,
    paging_policy: ManifestPagingPolicy,
    request: &FileVersionCheckpointRequest,
    derivation: &D,
) -> Result<Vec<u8>, ManifestPagingError> {
    let prepared = PreparedCheckpointFile::from_bytes(bytes, config, paging_policy)?;
    Ok(file_version_payload(
        prepared.manifest.id,
        request,
        derivation,
    ))
}

fn file_version_payload<D: HeadDerivation + ?Sized>(
    manifest_id: RecordDigest,
    request: &FileVersionCheckpointRequest,
    derivation: &D,
) -> Vec<u8> {
    let operation = Operation::WriteFileVersion {
        object_id: request.object_id,
        version_id: request.version_id,
        parent_versions: request.parent_versions.clone(),
        manifest_id: ManifestId::from_bytes(*manifest_id.as_bytes()),
        portable_metadata: request.portable_metadata,
    };
    let mut operations = request.before_write.clone();
    operations.push(operation);
    operations.extend(request.after_write.clone());
    let changeset = ChangeSetDraft::new(
        request.workspace_id,
        request.actor_id,
        request.session_id,
        request.actor_sequence,
        request.hybrid_logical_time,
    )
    .causal_parents(request.causal_parents.clone())
    .base_head(request.base_head)
    .policy_epoch(request.policy_epoch)
    .seal(operations, derivation, request.signature);
    encode_canonical(&changeset)
}

/// Produce the exact canonical operation-only ChangeSet statement a native actor must sign.
pub(crate) fn operation_checkpoint_signing_body<D: HeadDerivation + ?Sized>(
    request: &AuthenticatedOperationCheckpointRequest,
    derivation: &D,
) -> Vec<u8> {
    operation_checkpoint_payload(request, derivation)
}

fn operation_checkpoint_payload<D: HeadDerivation + ?Sized>(
    request: &AuthenticatedOperationCheckpointRequest,
    derivation: &D,
) -> Vec<u8> {
    let changeset = ChangeSetDraft::new(
        request.workspace_id,
        request.actor_id,
        request.session_id,
        request.actor_sequence,
        request.hybrid_logical_time,
    )
    .causal_parents(request.causal_parents.clone())
    .base_head(request.base_head)
    .policy_epoch(request.policy_epoch)
    .seal(
        request.operations.clone(),
        derivation,
        OperationSignature::from_bytes(*request.signature.as_bytes()),
    );
    encode_canonical(&changeset)
}

/// A completed save whose acknowledgement was withheld until the immutable journal append.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournaledPrivateSave {
    acknowledgement: PrivateSaved,
    manifest_id: RecordDigest,
    changeset_id: RecordDigest,
    linked_bytes: u64,
    physical_manifest_index: Option<RecordDigest>,
    staged_cas_objects: usize,
    reused_cas_objects: usize,
}

/// One authenticated ChangeSet after CAS, index, and immutable journal durability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct JournaledPrivateMutation {
    acknowledgement: PrivateSaved,
    changeset_id: RecordDigest,
    linked_bytes: u64,
}

impl JournaledPrivateMutation {
    pub(crate) const fn acknowledgement(self) -> PrivateSaved {
        self.acknowledgement
    }

    pub(crate) const fn changeset_id(self) -> RecordDigest {
        self.changeset_id
    }

    pub(crate) const fn linked_bytes(self) -> u64 {
        self.linked_bytes
    }
}

impl JournaledPrivateSave {
    /// The existing acknowledgement token, now additionally guarded by the journal append.
    #[must_use]
    pub const fn acknowledgement(self) -> PrivateSaved {
        self.acknowledgement
    }

    /// Canonical v0 manifest identifier appended to the workspace journal.
    #[must_use]
    pub const fn manifest_id(self) -> RecordDigest {
        self.manifest_id
    }

    /// Canonical ChangeSet payload identifier appended to the workspace journal.
    #[must_use]
    pub const fn changeset_id(self) -> RecordDigest {
        self.changeset_id
    }

    /// File and payload bytes newly linked into CAS by this save.
    #[must_use]
    pub const fn linked_bytes(self) -> u64 {
        self.linked_bytes
    }

    /// Candidate B physical index in CAS, when the explicit policy selected paging.
    #[must_use]
    pub const fn physical_manifest_index(self) -> Option<RecordDigest> {
        self.physical_manifest_index
    }

    /// CAS objects that ran the complete temporary-write/flush/verify/promote sequence.
    #[must_use]
    pub const fn staged_cas_objects(self) -> usize {
        self.staged_cas_objects
    }

    /// Existing content names that skipped redundant temporary I/O and passed the step-4 recheck.
    #[must_use]
    pub const fn reused_cas_objects(self) -> usize {
        self.reused_cas_objects
    }
}

/// A typed save failed before a journal-guarded acknowledgement could be returned.
#[derive(Debug)]
pub enum CheckpointSaveError<J> {
    /// Zero is not an issued actor sequence.
    ActorSequenceNotIssued,
    /// A ChangeSet with no operations cannot describe a workspace mutation.
    EmptyOperationSet,
    /// Supplied file content is unrelated to the signed operations, or a referenced manifest is absent.
    ManifestSetMismatch,
    /// Explicit physical manifest preparation failed before CAS was changed.
    ManifestPaging(ManifestPagingError),
    /// The authenticated envelope did not bind and verify the exact actor statement.
    Authentication(AuthenticatedChangeSetError),
    /// CAS promotion or the atomic metadata transaction failed.
    DurableSequence(SequenceError<CasChunkPromoterError, SqliteError>),
    /// The immutable workspace journal did not accept every record.
    Journal(J),
}

impl<J: fmt::Display> fmt::Display for CheckpointSaveError<J> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ActorSequenceNotIssued => {
                formatter.write_str("actor sequence zero was not issued and cannot be saved")
            }
            Self::EmptyOperationSet => {
                formatter.write_str("an empty operation set cannot be saved")
            }
            Self::ManifestSetMismatch => {
                formatter.write_str("checkpoint file manifests do not match the signed operations")
            }
            Self::ManifestPaging(error) => error.fmt(formatter),
            Self::Authentication(error) => error.fmt(formatter),
            Self::DurableSequence(error) => error.fmt(formatter),
            Self::Journal(error) => write!(
                formatter,
                "the immutable workspace journal did not accept the checkpoint: {error}"
            ),
        }
    }
}

/// Save one authenticated operation-only ChangeSet into an already-open workspace.
pub(crate) fn save_authenticated_operations<F: DurableFs, D: HeadDerivation + ?Sized>(
    workspace: &mut OpenWorkspace,
    cas: &Cas<F, CasBlake3>,
    request: AuthenticatedOperationCheckpointRequest,
    derivation: &D,
) -> Result<JournaledPrivateMutation, CheckpointSaveError<std::io::Error>> {
    save_authenticated_checkpoint(workspace, cas, request, Vec::new(), derivation)
}

/// Verify the signature and derive the exact operation identity before recording an append intent.
pub(crate) fn authenticated_checkpoint_identity<D: HeadDerivation + ?Sized>(
    request: &AuthenticatedOperationCheckpointRequest,
    derivation: &D,
) -> Result<RecordDigest, AuthenticatedChangeSetError> {
    let payload = AuthenticatedChangeSet::verified(
        operation_checkpoint_payload(request, derivation),
        request.actor_public_key,
        request.signature,
    )?
    .canonical_bytes();
    Ok(RecordDigest::from_bytes(
        *Blake3::digest_bytes(&payload).as_bytes(),
    ))
}

/// Commit all admitted file content and one signed operation set through the existing journal.
/// Files are immutable prepared bytes; this path never reads a working directory. Acknowledgment
/// follows all manifest records and the operation record, so an interrupted append cannot expose
/// a partially recorded set of file writes as the new operation.
pub(crate) fn save_authenticated_checkpoint<F: DurableFs, D: HeadDerivation + ?Sized>(
    workspace: &mut OpenWorkspace,
    cas: &Cas<F, CasBlake3>,
    request: AuthenticatedOperationCheckpointRequest,
    files: Vec<PreparedCheckpointFile>,
    derivation: &D,
) -> Result<JournaledPrivateMutation, CheckpointSaveError<std::io::Error>> {
    let PreparedAuthenticatedCheckpoint {
        checkpoint,
        objects,
        changeset_id,
    } = prepare_authenticated_checkpoint(workspace, request, files, derivation)?;
    let records = checkpoint.records();
    let mut promoter = CasChunkPromoter::new(cas);
    let saved = DurableCommit::new(
        workspace.checkpoint_store_mut(),
        &mut promoter,
        objects,
        checkpoint,
    )
    .finish()
    .map_err(CheckpointSaveError::DurableSequence)?;
    let acknowledgement = *saved.acknowledgement();
    journal_records(workspace.checkpoint_journal_mut(), records.iter())
        .map_err(CheckpointSaveError::Journal)?;
    Ok(JournaledPrivateMutation {
        acknowledgement,
        changeset_id,
        linked_bytes: promoter.linked_bytes(),
    })
}

/// Verified immutable capture material. Preparing it performs no CAS, index or journal writes.
/// Native enrolled authoring can stage exact recovery evidence before committing these records.
pub(crate) struct PreparedAuthenticatedCheckpoint {
    pub(crate) checkpoint: Checkpoint,
    pub(crate) objects: Vec<Vec<u8>>,
    pub(crate) changeset_id: RecordDigest,
}

pub(crate) fn prepare_authenticated_checkpoint<D: HeadDerivation + ?Sized>(
    workspace: &OpenWorkspace,
    request: AuthenticatedOperationCheckpointRequest,
    files: Vec<PreparedCheckpointFile>,
    derivation: &D,
) -> Result<PreparedAuthenticatedCheckpoint, CheckpointSaveError<std::io::Error>> {
    if request.actor_sequence.value() == 0 {
        return Err(CheckpointSaveError::ActorSequenceNotIssued);
    }
    if request.operations.is_empty() {
        return Err(CheckpointSaveError::EmptyOperationSet);
    }
    let parent_records = request
        .causal_parents
        .as_slice()
        .iter()
        .map(|parent| RecordDigest::from_bytes(*parent.as_bytes()))
        .collect::<Vec<_>>();
    let inner_payload = operation_checkpoint_payload(&request, derivation);
    let payload = AuthenticatedChangeSet::verified(
        inner_payload,
        request.actor_public_key,
        request.signature,
    )
    .map_err(CheckpointSaveError::Authentication)?
    .canonical_bytes();
    let changeset_id = RecordDigest::from_bytes(*Blake3::digest_bytes(&payload).as_bytes());
    let operation_record = OperationRecord {
        id: changeset_id,
        actor: RecordDigest::from_bytes(*request.actor_id.as_bytes()),
        actor_sequence: request.actor_sequence.value(),
        hlc_millis: request.hybrid_logical_time.physical_millis(),
        hlc_counter: u64::from(request.hybrid_logical_time.logical()),
        policy_epoch: request.policy_epoch.value(),
        session: EntityUuid::from_bytes(*request.session_id.as_bytes()),
        payload_digest: changeset_id,
        parents: parent_records,
    };
    let referenced = request
        .operations
        .iter()
        .filter_map(|operation| match operation {
            Operation::WriteFileVersion { manifest_id, .. } => {
                Some(RecordDigest::from_bytes(*manifest_id.as_bytes()))
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let mut manifests = BTreeMap::new();
    let mut objects = BTreeMap::new();
    for file in files {
        let (manifest, chunks, paged) = file.into_parts();
        if !referenced.contains(&manifest.id) {
            return Err(CheckpointSaveError::ManifestSetMismatch);
        }
        manifests.entry(manifest.id).or_insert(manifest);
        for bytes in chunks.into_iter().chain(
            paged
                .into_iter()
                .flat_map(|pages| pages.cas_objects().to_vec()),
        ) {
            let digest = RecordDigest::from_bytes(*Blake3::digest_bytes(&bytes).as_bytes());
            objects.entry(digest).or_insert(bytes);
        }
    }
    for manifest in referenced {
        if !manifests.contains_key(&manifest) && !workspace.has_journaled_manifest(manifest) {
            return Err(CheckpointSaveError::ManifestSetMismatch);
        }
    }
    objects.entry(changeset_id).or_insert(payload);
    let checkpoint = Checkpoint {
        manifests: manifests.into_values().collect(),
        operations: vec![operation_record],
        ..Checkpoint::default()
    };
    Ok(PreparedAuthenticatedCheckpoint {
        checkpoint,
        objects: objects.into_values().collect(),
        changeset_id,
    })
}

impl<J: fmt::Debug + fmt::Display> std::error::Error for CheckpointSaveError<J> {}

/// Save one typed file version into an already-open workspace.
///
/// `config` is required and has no fallback. The returned acknowledgement is unreachable until
/// chunks and the ChangeSet payload are durable in CAS, the metadata transaction commits, and both
/// immutable records have been appended and synced in dependency order. Refresh `workspace`
/// through its retained handles before answering derived queries from it.
pub fn save_file_version<F: DurableFs, D: HeadDerivation + ?Sized>(
    workspace: &mut OpenWorkspace,
    cas: &Cas<F, CasBlake3>,
    bytes: &[u8],
    config: &ChunkingConfig,
    paging_policy: ManifestPagingPolicy,
    request: FileVersionCheckpointRequest,
    derivation: &D,
) -> Result<JournaledPrivateSave, CheckpointSaveError<std::io::Error>> {
    let (store, journal) = workspace.checkpoint_parts_mut();
    save_file_version_with_journal(
        store,
        cas,
        journal,
        bytes,
        config,
        paging_policy,
        request,
        derivation,
    )
}

/// The same composition over an explicit journal, for crash/failure harnesses.
///
/// Production callers should use [`save_file_version`]. This seam exists so a test can refuse the
/// append and prove that no acknowledgement escapes and restart truth still comes only from the
/// immutable journal.
#[allow(clippy::too_many_arguments)]
pub fn save_file_version_with_journal<F, J, D>(
    store: &mut Store<Sqlite>,
    cas: &Cas<F, CasBlake3>,
    journal: &mut J,
    bytes: &[u8],
    config: &ChunkingConfig,
    paging_policy: ManifestPagingPolicy,
    request: FileVersionCheckpointRequest,
    derivation: &D,
) -> Result<JournaledPrivateSave, CheckpointSaveError<J::Error>>
where
    F: DurableFs,
    J: RecordJournal,
    D: HeadDerivation + ?Sized,
{
    if request.actor_sequence.value() == 0 {
        return Err(CheckpointSaveError::ActorSequenceNotIssued);
    }

    let FileVersionCheckpointRequest {
        workspace_id,
        actor_id,
        session_id,
        actor_sequence,
        causal_parents,
        base_head,
        policy_epoch,
        hybrid_logical_time,
        object_id,
        version_id,
        parent_versions,
        portable_metadata,
        before_write,
        after_write,
        signature,
        authenticated_public_key,
    } = request;
    let prepared = PreparedCheckpointFile::from_bytes(bytes, config, paging_policy)
        .map_err(CheckpointSaveError::ManifestPaging)?;
    let manifest_id = prepared.manifest.id;
    let payload_request = FileVersionCheckpointRequest {
        workspace_id,
        actor_id,
        session_id,
        actor_sequence,
        causal_parents: causal_parents.clone(),
        base_head,
        policy_epoch,
        hybrid_logical_time,
        object_id,
        version_id,
        parent_versions,
        portable_metadata,
        before_write,
        after_write,
        signature,
        authenticated_public_key,
    };
    let parent_records = causal_parents
        .as_slice()
        .iter()
        .map(|parent| RecordDigest::from_bytes(*parent.as_bytes()))
        .collect::<Vec<_>>();
    let inner_payload = file_version_payload(manifest_id, &payload_request, derivation);
    let payload = match authenticated_public_key {
        Some(public_key) => AuthenticatedChangeSet::verified(
            inner_payload,
            public_key,
            Signature::from_bytes(*signature.as_bytes()),
        )
        .map_err(CheckpointSaveError::Authentication)?
        .canonical_bytes(),
        None => inner_payload,
    };
    let changeset_digest = Blake3::digest_bytes(&payload);
    let changeset_id = RecordDigest::from_bytes(*changeset_digest.as_bytes());
    let operation_record = OperationRecord {
        id: changeset_id,
        actor: RecordDigest::from_bytes(*actor_id.as_bytes()),
        actor_sequence: actor_sequence.value(),
        hlc_millis: hybrid_logical_time.physical_millis(),
        hlc_counter: u64::from(hybrid_logical_time.logical()),
        policy_epoch: policy_epoch.value(),
        session: EntityUuid::from_bytes(*session_id.as_bytes()),
        payload_digest: changeset_id,
        parents: parent_records,
    };
    let (manifest, mut chunks, paged) = prepared.into_parts();
    let physical_manifest_index = paged.as_ref().map(PagedManifest::index_digest);
    if let Some(paged) = &paged {
        chunks.extend(paged.cas_objects().iter().cloned());
    }
    if !manifest
        .chunks
        .iter()
        .any(|chunk| chunk.digest == changeset_id)
    {
        chunks.push(payload);
    }
    let checkpoint = Checkpoint {
        manifests: vec![manifest],
        operations: vec![operation_record],
        ..Checkpoint::default()
    };
    let records = checkpoint.records();
    let mut promoter = CasChunkPromoter::new(cas);
    let saved = DurableCommit::new(store, &mut promoter, chunks, checkpoint)
        .finish()
        .map_err(CheckpointSaveError::DurableSequence)?;
    let acknowledgement = *saved.acknowledgement();
    let linked_bytes = promoter.linked_bytes();
    let staged_cas_objects = promoter.staged_objects();
    let reused_cas_objects = promoter.reused_objects();

    journal_records(journal, records.iter()).map_err(CheckpointSaveError::Journal)?;
    Ok(JournaledPrivateSave {
        acknowledgement,
        manifest_id,
        changeset_id,
        linked_bytes,
        physical_manifest_index,
        staged_cas_objects,
        reused_cas_objects,
    })
}

#[cfg(test)]
mod tests;
