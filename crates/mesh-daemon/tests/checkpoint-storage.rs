//! End-to-end coverage for the production chunker → CAS → durable-commit adapter.

use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use mesh_chunking::ChunkingConfig;
use mesh_daemon::{
    reconstruct_paged_manifest, save_file_version, save_file_version_with_journal,
    CasChunkPromoter, FileVersionCheckpointRequest, ManifestPagingPolicy, OpenWorkspace,
    PreparedCheckpointFile,
};
use mesh_operations::{
    ActorId, ActorSequence, CausalParents, HeadDerivation, HeadId, Hlc, ObjectId, PolicyEpoch,
    PortableMetadata, SessionId, Signature, TransitionCommitment, VersionId, WorkspaceId,
};
use mesh_store::{
    Checkpoint, ChunkPromoter as _, DurableCommit, RecordDigest, RecordJournal, SequenceStep,
    Sqlite, Store,
};
use mesh_types::{Blake3, ContentDigest as _};

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "mesh-checkpoint-storage-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}

fn content(length: usize) -> Vec<u8> {
    (0..length)
        .map(|index| {
            let value = (index as u64)
                .wrapping_mul(0x9e37_79b9_7f4a_7c15)
                .rotate_left((index % 63) as u32);
            (value ^ (value >> 17)) as u8
        })
        .collect()
}

fn config() -> ChunkingConfig {
    ChunkingConfig::always_chunked(64, 256, 1024).expect("valid deterministic policy")
}

struct CanonicalHead;

impl HeadDerivation for CanonicalHead {
    fn resulting_head(&self, commitment: &TransitionCommitment) -> HeadId {
        HeadId::from_bytes(*Blake3::digest_bytes(&commitment.canonical_bytes()).as_bytes())
    }
}

fn request(sequence: u64) -> FileVersionCheckpointRequest {
    FileVersionCheckpointRequest::new(
        WorkspaceId::from_bytes([1; 16]),
        ActorId::from_bytes([2; 32]),
        SessionId::from_bytes([3; 16]),
        ActorSequence::new(sequence),
        CausalParents::genesis(),
        HeadId::from_bytes([4; 32]),
        PolicyEpoch::new(5),
        Hlc::new(1_700_000_000_000, 6),
        ObjectId::from_bytes([7; 16]),
        VersionId::from_bytes([8; 32]),
        vec![VersionId::from_bytes([9; 32])],
        PortableMetadata::new(true),
        Signature::from_bytes([10; 64]),
    )
}

#[test]
fn one_durable_sequence_chunks_promotes_commits_and_reconstructs() {
    let root = scratch("roundtrip");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let bytes = content(32 * 1024);
    let prepared =
        PreparedCheckpointFile::from_bytes(&bytes, &config(), ManifestPagingPolicy::flat())
            .expect("flat preparation");
    assert!(
        prepared.manifest().chunks.len() > 1,
        "content was actually cut"
    );
    assert!(prepared.chunks().len() <= prepared.manifest().chunks.len());

    let cas = mesh_cas::Cas::open(&root).expect("CAS");
    let mut store =
        Store::open(Sqlite::open(root.join("metadata.sqlite")).expect("SQLite")).expect("store");
    let checkpoint = Checkpoint {
        manifests: vec![prepared.manifest().clone()],
        ..Checkpoint::default()
    };
    let mut promoter = CasChunkPromoter::new(&cas);
    let mut sequence = DurableCommit::new(
        &mut store,
        &mut promoter,
        prepared.chunks().to_vec(),
        checkpoint,
    );
    sequence
        .run_through(SequenceStep::PromoteChunks)
        .expect("verified chunks promote");
    assert!(
        sequence.acknowledgement().is_none(),
        "CAS durability alone is not a private-save acknowledgement"
    );
    let saved = sequence.finish().expect("metadata transaction commits");
    assert_eq!(saved.acknowledgement().manifests(), 1);
    assert_eq!(saved.acknowledgement().chunks(), prepared.chunks().len());
    assert_eq!(
        promoter.linked_bytes(),
        prepared.chunks().iter().map(Vec::len).sum::<usize>() as u64
    );

    let mut reconstructed = Vec::new();
    for slice in &prepared.manifest().chunks {
        reconstructed.extend_from_slice(
            &cas.read(&mesh_cas::Digest32::from_bytes(*slice.digest.as_bytes()))
                .expect("manifest chunk is readable"),
        );
    }
    assert_eq!(reconstructed, bytes);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_identical_second_save_links_zero_novel_bytes() {
    let root = scratch("dedup");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let prepared = PreparedCheckpointFile::from_bytes(
        &content(16 * 1024),
        &config(),
        ManifestPagingPolicy::flat(),
    )
    .expect("flat preparation");
    let cas = mesh_cas::Cas::open(&root).expect("CAS");
    let mut store =
        Store::open(Sqlite::open(root.join("metadata.sqlite")).expect("SQLite")).expect("store");

    let mut first = CasChunkPromoter::new(&cas);
    DurableCommit::new(
        &mut store,
        &mut first,
        prepared.chunks().to_vec(),
        Checkpoint {
            manifests: vec![prepared.manifest().clone()],
            ..Checkpoint::default()
        },
    )
    .finish()
    .expect("first save");
    assert!(first.linked_bytes() > 0);
    assert_eq!(first.staged_objects(), prepared.chunks().len());
    assert_eq!(first.reused_objects(), 0);

    let mut second = CasChunkPromoter::new(&cas);
    DurableCommit::new(
        &mut store,
        &mut second,
        prepared.chunks().to_vec(),
        Checkpoint::default(),
    )
    .finish()
    .expect("deduplicated save");
    assert_eq!(second.linked_bytes(), 0);
    assert_eq!(second.staged_objects(), 0);
    assert_eq!(second.reused_objects(), prepared.chunks().len());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_reused_chunk_removed_before_the_transaction_is_refused() {
    let root = scratch("reuse-race");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let prepared = PreparedCheckpointFile::from_bytes(
        &content(16 * 1024),
        &config(),
        ManifestPagingPolicy::flat(),
    )
    .expect("flat preparation");
    let cas = mesh_cas::Cas::open(&root).expect("CAS");
    let mut store =
        Store::open(Sqlite::open(root.join("metadata.sqlite")).expect("SQLite")).expect("store");

    let mut first = CasChunkPromoter::new(&cas);
    DurableCommit::new(
        &mut store,
        &mut first,
        prepared.chunks().to_vec(),
        Checkpoint {
            manifests: vec![prepared.manifest().clone()],
            ..Checkpoint::default()
        },
    )
    .finish()
    .expect("first save");

    let mut second = CasChunkPromoter::new(&cas);
    let checkpoint = Checkpoint {
        manifests: vec![prepared.manifest().clone()],
        ..Checkpoint::default()
    };
    let mut sequence = DurableCommit::new(
        &mut store,
        &mut second,
        prepared.chunks().to_vec(),
        checkpoint,
    );
    sequence
        .run_through(SequenceStep::VerifyChunks)
        .expect("existing names skip redundant staging and flushing");
    let removed = prepared.manifest().chunks[0].digest;
    fs::remove_file(
        cas.layout()
            .chunk_path(&mesh_cas::Digest32::from_bytes(*removed.as_bytes())),
    )
    .expect("plant removal between reuse check and transaction");
    let error = sequence
        .run_through(SequenceStep::PromoteChunks)
        .expect_err("step 4 recheck refuses the vanished reused object");
    assert_eq!(error.step(), SequenceStep::PromoteChunks);
    assert!(sequence.acknowledgement().is_none());
    assert_eq!(store.index().manifest_ids().len(), 1);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_small_edit_links_only_the_changed_content_defined_region() {
    let root = scratch("edit-locality");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let original = content(64 * 1024);
    let first_file =
        PreparedCheckpointFile::from_bytes(&original, &config(), ManifestPagingPolicy::flat())
            .expect("flat preparation");
    let cas = mesh_cas::Cas::open(&root).expect("CAS");
    let mut store =
        Store::open(Sqlite::open(root.join("metadata.sqlite")).expect("SQLite")).expect("store");
    let mut first = CasChunkPromoter::new(&cas);
    DurableCommit::new(
        &mut store,
        &mut first,
        first_file.chunks().to_vec(),
        Checkpoint {
            manifests: vec![first_file.manifest().clone()],
            ..Checkpoint::default()
        },
    )
    .finish()
    .expect("genesis save");

    let mut edited = original.clone();
    let midpoint = edited.len() / 2;
    edited[midpoint] ^= 0x5a;
    let edited_file =
        PreparedCheckpointFile::from_bytes(&edited, &config(), ManifestPagingPolicy::flat())
            .expect("flat preparation");
    let mut delta = CasChunkPromoter::new(&cas);
    DurableCommit::new(
        &mut store,
        &mut delta,
        edited_file.chunks().to_vec(),
        Checkpoint {
            manifests: vec![edited_file.manifest().clone()],
            ..Checkpoint::default()
        },
    )
    .finish()
    .expect("edited save");
    assert!(delta.linked_bytes() > 0);
    assert!(
        delta.linked_bytes() < edited.len() as u64,
        "one-byte edit linked the whole {}-byte file",
        edited.len()
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_manifest_cannot_enter_metadata_when_its_chunk_is_absent() {
    let root = scratch("absent");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let prepared =
        PreparedCheckpointFile::from_bytes(&content(4096), &config(), ManifestPagingPolicy::flat())
            .expect("flat preparation");
    let cas = mesh_cas::Cas::open(&root).expect("CAS");
    let mut store =
        Store::open(Sqlite::open(root.join("metadata.sqlite")).expect("SQLite")).expect("store");
    let mut promoter = CasChunkPromoter::new(&cas);
    let mut sequence = DurableCommit::new(
        &mut store,
        &mut promoter,
        Vec::new(),
        Checkpoint {
            manifests: vec![prepared.manifest().clone()],
            ..Checkpoint::default()
        },
    );
    let error = sequence
        .run_through(SequenceStep::BeginTransaction)
        .expect_err("absent reference is refused");
    assert_eq!(error.step(), SequenceStep::BeginTransaction);
    assert!(sequence.acknowledgement().is_none());
    assert_eq!(store.index().manifest_ids().len(), 0);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_concrete_promoter_refuses_skipped_steps() {
    let root = scratch("order");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let cas = mesh_cas::Cas::open(&root).expect("CAS");
    let mut promoter = CasChunkPromoter::new(&cas);
    assert!(promoter.flush_temporary().is_err());
    assert!(promoter.verify_temporary().is_err());
    assert!(promoter.promote().is_err());
    assert!(!promoter.is_durable(&RecordDigest::from_bytes([0; 32])));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn typed_save_is_journaled_and_rebuilds_after_daemon_restart() {
    let root = scratch("journal-restart");
    let _ = fs::remove_dir_all(&root);
    let mut workspace = OpenWorkspace::open(&root).expect("fresh workspace");
    let cas = mesh_cas::Cas::open(&root).expect("CAS");
    let bytes = content(32 * 1024);
    let expected =
        PreparedCheckpointFile::from_bytes(&bytes, &config(), ManifestPagingPolicy::flat())
            .expect("logical manifest")
            .manifest()
            .clone();
    let saved = save_file_version(
        &mut workspace,
        &cas,
        &bytes,
        &config(),
        ManifestPagingPolicy::page_at_or_above(NonZeroUsize::new(1).unwrap()),
        request(1),
        &CanonicalHead,
    )
    .expect("typed save");
    assert_eq!(saved.acknowledgement().operations(), 1);
    assert_eq!(saved.acknowledgement().manifests(), 1);
    assert!(saved.linked_bytes() > 0);
    let physical_index = saved
        .physical_manifest_index()
        .expect("explicit paging produced an index");
    assert!(cas.contains(&mesh_cas::Digest32::from_bytes(*physical_index.as_bytes())));
    assert_eq!(
        reconstruct_paged_manifest(&cas, physical_index, saved.manifest_id())
            .expect("physical pages reconstruct v0"),
        expected
    );
    assert!(cas.contains(&mesh_cas::Digest32::from_bytes(
        *saved.changeset_id().as_bytes()
    )));
    drop(workspace);

    let reopened = OpenWorkspace::open(&root).expect("restart rebuild");
    assert_eq!(reopened.boundary().records, 2);
    assert_eq!(reopened.manifests(), 1);
    assert_eq!(reopened.operations(), 1);
    assert!(reopened.has_operation(&saved.changeset_id()));
    let _ = fs::remove_dir_all(&root);
}

#[derive(Debug)]
struct RefuseAppend;

impl std::fmt::Display for RefuseAppend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("planted journal refusal")
    }
}

impl std::error::Error for RefuseAppend {}

struct CasDurableThenFail<'a> {
    cas: &'a mesh_cas::Cas,
    attempts: usize,
}

impl RecordJournal for CasDurableThenFail<'_> {
    type Error = RefuseAppend;

    fn read_all(&mut self) -> Result<Vec<u8>, Self::Error> {
        Ok(Vec::new())
    }

    fn append(&mut self, _framed: &[u8]) -> Result<(), Self::Error> {
        self.attempts += 1;
        assert!(
            !self
                .cas
                .sweep_all_chunks()
                .expect("CAS enumeration")
                .is_empty(),
            "journal append ran before CAS held the checkpoint bytes"
        );
        Err(RefuseAppend)
    }
}

#[test]
fn journal_failure_returns_no_ack_and_restart_discards_unjournaled_metadata() {
    let root = scratch("journal-failure");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let cas = mesh_cas::Cas::open(&root).expect("CAS");
    let mut store =
        Store::open(Sqlite::open(root.join("metadata.sqlite")).expect("SQLite")).expect("store");
    let mut journal = CasDurableThenFail {
        cas: &cas,
        attempts: 0,
    };
    let result = save_file_version_with_journal(
        &mut store,
        &cas,
        &mut journal,
        &content(8192),
        &config(),
        ManifestPagingPolicy::flat(),
        request(1),
        &CanonicalHead,
    );
    assert!(
        result.is_err(),
        "no journal means no returned acknowledgement"
    );
    assert_eq!(journal.attempts, 1);
    assert_eq!(store.index().manifest_ids().len(), 1);
    assert_eq!(store.index().operation_count(), 1);
    drop(store);

    let reopened = OpenWorkspace::open(&root).expect("restart from journal truth");
    assert_eq!(reopened.boundary().records, 0);
    assert_eq!(reopened.manifests(), 0);
    assert_eq!(reopened.operations(), 0);
    assert!(!cas.sweep_all_chunks().expect("CAS remains").is_empty());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_unissued_sequence_is_refused_before_cas_or_journal_changes() {
    let root = scratch("zero-sequence");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    let cas = mesh_cas::Cas::open(&root).expect("CAS");
    let mut store =
        Store::open(Sqlite::open(root.join("metadata.sqlite")).expect("SQLite")).expect("store");
    struct EmptyJournal;
    impl RecordJournal for EmptyJournal {
        type Error = io::Error;
        fn read_all(&mut self) -> io::Result<Vec<u8>> {
            Ok(Vec::new())
        }
        fn append(&mut self, _framed: &[u8]) -> io::Result<()> {
            panic!("zero sequence reached journal")
        }
    }
    let refused = save_file_version_with_journal(
        &mut store,
        &cas,
        &mut EmptyJournal,
        &content(4096),
        &config(),
        ManifestPagingPolicy::flat(),
        request(0),
        &CanonicalHead,
    );
    assert!(refused.is_err());
    assert!(cas.sweep_all_chunks().expect("empty CAS").is_empty());
    let _ = fs::remove_dir_all(&root);
}
