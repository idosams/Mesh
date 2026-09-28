//! Cross-validation of the performance counters, one family at a time.
//!
//! ```text
//! cargo nextest run -p mesh-daemon --test counters
//! ```
//!
//! # What "independent measurement" means here, exactly
//!
//! A counter is checked against a number this file computes **without asking the counter**, by a
//! route the recording path does not share. Three routes are used, and the strength of each is
//! stated rather than implied:
//!
//! - **The test's own tally.** The test opens a workspace four times, so four is the answer. No
//!   code produces it; the test wrote it. This is the strongest form available and it is what the
//!   operation counts are checked against.
//! - **The operating system.** `local_filesystem.sequential_read.bytes` is accumulated from a
//!   durable boundary plus an interrupted-append residue, both computed by a scan over the bytes.
//!   The independent number is `std::fs::metadata(…).len()`, which the kernel answers and which no
//!   part of the recording path consulted.
//! - **A separately written recomputation.** For the two families this build has no producer for,
//!   the test drives the counters from a fixture and recomputes the expected totals with a
//!   different traversal of the same fixture. Weaker than the two above — a shared misunderstanding
//!   of the fixture would fool both — and it is used only where the alternative is not to check the
//!   family at all.
//!
//! # The band is stated before the numbers are seen
//!
//! `mesh_daemon::counters::Band` is a property of the counter's unit, decided in the catalogue:
//!
//! - `Band::Exact` for every count and every byte total. No tolerance. A deterministic counter that
//!   is one off is broken, and a percentage would only hide it.
//! - `Band::Enclosed` for every nanosecond total: the counter times an inner section, so an
//!   independent measurement that **encloses** that section must be at least as large.
//!
//! `Enclosed` is why nothing in this file asserts a duration budget. Two wall-clock budget
//! assertions on this repository's merge path have already failed under machine load; a gate that
//! fails on load teaches its readers to re-run rather than read. An enclosure holds on a machine
//! under any load, and it still fails if the counter is timing something other than what it names.
//!
//! # What is checked without a workspace at all
//!
//! `## Acceptance criteria` bullet 4 — counters are queryable without running a benchmark — is
//! checked directly: a daemon that has opened nothing and measured nothing still answers a full
//! snapshot, and the snapshot names every counter this build does not feed.

use std::collections::BTreeMap;
use std::time::Instant;

use mesh_daemon::counters::{
    catalogue_gaps, counter_count, counters, deterministically_named, gaps, required_metric_count,
    wired_required_metric_count, Band, CounterId, CounterSnapshot, Counters, Determinism, Family,
    Gap,
};

/// Resolve a key, failing the test by name rather than unwrapping an `Option`.
fn id(key: &str) -> CounterId {
    CounterId::of(key).unwrap_or_else(|| panic!("`{key}` is not in the counter catalogue"))
}

/// The observation count and total a snapshot holds for `key`.
fn read(snapshot: &CounterSnapshot, key: &str) -> (u64, u64) {
    let reading = snapshot
        .reading(key)
        .unwrap_or_else(|| panic!("`{key}` is not in the snapshot"));
    (reading.observations, reading.total)
}

// ---------------------------------------------------------------------------
// The edge between plan §12.3 and the catalogue
// ---------------------------------------------------------------------------

#[test]
fn every_required_metric_of_plan_12_3_has_a_structurally_valid_catalogue_row() {
    let found = catalogue_gaps();
    assert!(
        found.is_empty(),
        "the counter catalogue does not answer plan 12.3:\n{}",
        found
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(
        deterministically_named(),
        required_metric_count(),
        "every required metric must keep one reading that survives a loaded machine"
    );
}

#[test]
fn task_completion_stays_red_until_every_required_metric_has_a_live_producer() {
    let found = gaps();
    assert_eq!(
        wired_required_metric_count(),
        4,
        "sequential read, checkpoint creation, crash recovery, and index reconstruction are live; canonical publication is unavailable without HumanHeld authority"
    );
    assert_eq!(
        found.len(),
        required_metric_count() - wired_required_metric_count(),
        "every required metric without a producer must remain visible"
    );
    assert!(
        found
            .iter()
            .all(|gap| matches!(gap, Gap::NoWiredCounter { .. })),
        "the catalogue is structurally complete; these must be producer gaps: {found:?}"
    );
}

#[test]
fn every_family_of_plan_12_3_is_represented() {
    for family in Family::ALL {
        let count = counters().filter(|spec| spec.family == *family).count();
        assert!(count > 0, "{} has no counters", family.word());
    }
    assert_eq!(
        Family::ALL
            .iter()
            .map(|family| counters().filter(|spec| spec.family == *family).count())
            .sum::<usize>(),
        counter_count()
    );
}

// ---------------------------------------------------------------------------
// Family: workspace operations — checked against the test's own tally
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod workspace_operations {
    use super::{id, read, Band, Counters, Determinism, Instant};
    use std::time::Duration;

    use mesh_chunking::ChunkingConfig;
    use mesh_daemon::counters::CounterId;
    use mesh_daemon::ipc::surface::{nothing_to_recover, Operations, StartupSummary};
    use mesh_daemon::{
        FileVersionCheckpointRequest, LiveDaemon, ManifestPagingPolicy, PreparedFolderImport,
        TrustedReviewers, RECORD_FILE_NAME,
    };
    use mesh_operations::{
        ActorId, ActorSequence, CausalParents, HeadDerivation, HeadId, Hlc, ObjectId, PolicyEpoch,
        PortableMetadata, SessionId, Signature as OperationSignature, TransitionCommitment,
        VersionId, WorkspaceId,
    };
    use mesh_store::{CheckpointRuntimeParameters, RecoverySequence, SqlExecutor as _, Sqlite};
    use mesh_types::{Blake3, ContentDigest as _, PublicKey};

    /// A scratch directory nothing else in this process uses.
    fn scratch(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "mesh-counters-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    /// One operation record, distinct in identity and in its writer's sequence.
    fn operation(index: u64) -> mesh_store::StoredRecord {
        let byte = u8::try_from(index % 251).unwrap_or(0) + 1;
        mesh_store::StoredRecord::Operation(mesh_store::OperationRecord {
            id: mesh_store::RecordDigest::from_bytes([byte; 32]),
            actor: mesh_store::RecordDigest::from_bytes([9; 32]),
            actor_sequence: index + 1,
            hlc_millis: 1_700_000_000_000,
            hlc_counter: 0,
            policy_epoch: 1,
            session: mesh_store::no_session(),
            payload_digest: mesh_store::RecordDigest::from_bytes([byte; 32]),
            parents: Vec::new(),
        })
    }

    /// `count` whole framed records, written by the test rather than by the daemon.
    ///
    /// Returns the byte length written, so a test can hold the counter to a number it produced
    /// itself rather than to one it read back from the thing under test.
    fn seed_records(root: &std::path::Path, count: u64) -> u64 {
        std::fs::create_dir_all(root).expect("scratch directory");
        let mut bytes = Vec::new();
        for index in 0..count {
            bytes.extend_from_slice(&mesh_store::frame_record(&operation(index)));
        }
        std::fs::write(root.join(RECORD_FILE_NAME), &bytes).expect("seed the record file");
        bytes.len() as u64
    }

    /// Half of one more framed record, which is what a kill during an append leaves behind.
    fn append_partial_frame(root: &std::path::Path, after: u64) -> u64 {
        let whole = mesh_store::frame_record(&operation(after));
        let partial = &whole[..whole.len() / 2];
        let mut bytes =
            std::fs::read(root.join(RECORD_FILE_NAME)).expect("read back what was seeded");
        bytes.extend_from_slice(partial);
        std::fs::write(root.join(RECORD_FILE_NAME), &bytes).expect("append a partial frame");
        partial.len() as u64
    }

    fn daemon() -> LiveDaemon {
        LiveDaemon::new(StartupSummary::from(&nothing_to_recover()))
    }

    fn checkpoint_parameters() -> CheckpointRuntimeParameters {
        CheckpointRuntimeParameters {
            idle_interval: Some(Duration::from_millis(8)),
            maximum_uncheckpointed_bytes: Some(16),
            maximum_uncheckpointed_interval: Some(Duration::from_millis(20)),
        }
    }

    struct CanonicalHead;

    impl HeadDerivation for CanonicalHead {
        fn resulting_head(&self, commitment: &TransitionCommitment) -> HeadId {
            HeadId::from_bytes(*Blake3::digest_bytes(&commitment.canonical_bytes()).as_bytes())
        }
    }

    fn checkpoint_request(sequence: u64) -> FileVersionCheckpointRequest {
        FileVersionCheckpointRequest::new(
            WorkspaceId::from_bytes([1; 16]),
            ActorId::from_bytes([2; 32]),
            SessionId::from_bytes([3; 16]),
            ActorSequence::new(sequence),
            CausalParents::genesis(),
            HeadId::from_bytes([4; 32]),
            PolicyEpoch::new(5),
            Hlc::new(1_700_000_000_000 + sequence, 6),
            ObjectId::from_bytes([7; 16]),
            VersionId::from_bytes([8; 32]),
            Vec::new(),
            PortableMetadata::new(true),
            OperationSignature::from_bytes([10; 64]),
        )
    }

    fn checkpoint_content() -> Vec<u8> {
        (0..8192)
            .map(|index| 31_u8.wrapping_add((index as u8).rotate_left((index % 7) as u32)))
            .collect()
    }

    #[test]
    fn the_open_count_equals_the_number_of_opens_the_test_performed() {
        let root = scratch("opens");
        seed_records(&root, 3);
        let daemon = daemon();

        // The independent measurement: this number is written here, not computed by anything.
        let opens = 4;
        for _ in 0..opens {
            daemon
                .open_workspace(&root.display().to_string())
                .expect("the seeded workspace opens");
        }

        let snapshot = daemon.counter_snapshot();
        let (observations, total) =
            read(&snapshot, "workspace_operations.index_reconstruction.ops");
        assert_eq!(
            (observations, total),
            (opens, opens),
            "band {}: an operation count is exact or it is broken",
            Band::Exact.word()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_completed_private_checkpoint_records_one_operation_and_exact_new_bytes() {
        let root = scratch("checkpoint-created");
        let daemon = LiveDaemon::with_checkpoint_runtime(
            StartupSummary::from(&nothing_to_recover()),
            checkpoint_parameters(),
        )
        .expect("valid configuration");
        let bytes = checkpoint_content();
        let chunking = ChunkingConfig::always_chunked(64, 256, 1024).expect("explicit policy");
        daemon
            .save_file_version(
                RecoverySequence::new(1).expect("issued sequence"),
                &bytes,
                &chunking,
                ManifestPagingPolicy::flat(),
                checkpoint_request(1),
                &CanonicalHead,
            )
            .expect_err("no workspace cannot create a checkpoint");
        assert_eq!(
            read(
                &daemon.counter_snapshot(),
                "workspace_operations.checkpoint_creation.ops"
            ),
            (0, 0),
            "a refused checkpoint must not become a completed operation"
        );
        daemon.open_at_start(&root).expect("workspace opens");

        let saved = daemon
            .save_file_version(
                RecoverySequence::new(1).expect("issued sequence"),
                &bytes,
                &chunking,
                ManifestPagingPolicy::flat(),
                checkpoint_request(1),
                &CanonicalHead,
            )
            .expect("private checkpoint is durable");

        let snapshot = daemon.counter_snapshot();
        assert_eq!(
            read(&snapshot, "workspace_operations.checkpoint_creation.ops"),
            (1, 1),
            "the test completed exactly one private checkpoint"
        );
        assert_eq!(
            read(&snapshot, "workspace_operations.checkpoint_creation.bytes"),
            (1, saved.linked_bytes()),
            "the counter must agree with the completed promoter rather than the input length"
        );
        assert!(
            saved.linked_bytes() > 0,
            "the fresh workspace linked content"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn software_approval_cannot_produce_a_canonical_publication_counter() {
        let root = scratch("canonical-publication");
        let source = root.with_extension("source");
        std::fs::create_dir_all(&source).expect("review source");
        std::fs::write(source.join("notes.txt"), b"exact review content\n")
            .expect("review source file");
        let prepared = PreparedFolderImport::prepare(&source, &root).expect("prepare import");
        let (confirmed, imported) = prepared.confirm_into_workspace().expect("confirm import");
        drop(confirmed);
        let reviewer = PublicKey::from_bytes([7; 32]);
        let daemon = LiveDaemon::with_trusted_reviewers(
            StartupSummary::from(&nothing_to_recover()),
            TrustedReviewers::new([reviewer]),
        );
        daemon.open_at_start(&root).expect("workspace opens");
        let shown = daemon.workspace_state().expect("review source");
        let opened = daemon
            .open_current_review_for_workspace(
                &shown.root,
                &shown.digest,
                &shown.installation,
                reviewer,
            )
            .expect("review opens");
        let bundle = opened.review_items[0]
            .get("bundle")
            .and_then(mesh_daemon::ipc::Json::as_text)
            .expect("computed review bundle");
        let unavailable = daemon
            .approve_review(bundle, &imported.operation().to_string(), "00")
            .expect_err("software-held approval cannot publish");
        assert_eq!(unavailable.code, "publication-human-authority-unavailable");

        let snapshot = daemon.counter_snapshot();
        assert_eq!(
            read(&snapshot, "workspace_operations.canonical_publication.ops"),
            (0, 0),
            "a refused approval must not become a publication"
        );
        assert!(snapshot
            .not_yet()
            .iter()
            .any(|(key, _)| *key == "workspace_operations.canonical_publication.ops"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_open_refused_during_checkpoint_install_records_nothing() {
        let root = scratch("refused-checkpoint-install");
        seed_records(&root, 1);

        // The record-derived index deliberately preserves this private runtime table. Give it an
        // incompatible shape so folding the journal succeeds but installing checkpoint recovery
        // state fails after that fold.
        let mut sqlite = Sqlite::open(root.join("metadata.sqlite")).expect("open SQLite");
        sqlite
            .execute_batch("CREATE TABLE mesh_recovery_state (wrong_column INTEGER) STRICT;")
            .expect("seed incompatible recovery schema");
        drop(sqlite);

        let daemon = LiveDaemon::with_checkpoint_runtime(
            StartupSummary::from(&nothing_to_recover()),
            checkpoint_parameters(),
        )
        .expect("valid configuration");
        assert!(
            daemon.open_workspace(&root.display().to_string()).is_err(),
            "the incompatible checkpoint table must refuse the open"
        );

        let snapshot = daemon.counter_snapshot();
        assert_eq!(
            read(&snapshot, "workspace_operations.index_reconstruction.ops"),
            (0, 0),
            "a refused workspace transition is not a completed open"
        );
        assert_eq!(snapshot.collection.observations_recorded, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_recovered_record_count_equals_the_records_the_test_wrote() {
        let root = scratch("fragment");
        // Five whole records, then a partial sixth: the shape a kill during an append leaves.
        let whole = seed_records(&root, 5);
        let partial = append_partial_frame(&root, 5);

        let daemon = daemon();
        daemon
            .open_workspace(&root.display().to_string())
            .expect("an interrupted append is served, not refused");

        let snapshot = daemon.counter_snapshot();
        assert_eq!(
            read(&snapshot, "workspace_operations.crash_recovery.ops"),
            (1, 1),
            "one open had to recover"
        );
        assert_eq!(
            read(&snapshot, "workspace_operations.crash_recovery.records").1,
            5,
            "the test wrote five whole records before the partial one"
        );
        assert_eq!(
            read(&snapshot, "local_filesystem.sequential_read.bytes").1,
            whole + partial,
            "the byte count covers the partial frame the scan discarded"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_clean_open_records_no_recovery_at_all() {
        let root = scratch("clean");
        seed_records(&root, 2);
        let daemon = daemon();
        daemon
            .open_workspace(&root.display().to_string())
            .expect("opens");

        let snapshot = daemon.counter_snapshot();
        assert_eq!(
            read(&snapshot, "workspace_operations.crash_recovery.ops"),
            (0, 0),
            "a clean open must not look like a recovery"
        );
        assert_eq!(
            read(&snapshot, "workspace_operations.index_reconstruction.ops"),
            (1, 1)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_fold_time_is_enclosed_by_a_measurement_the_test_takes_around_it() {
        let root = scratch("enclosed");
        seed_records(&root, 64);
        let daemon = daemon();

        // The independent measurement strictly encloses the section the counter times: it starts
        // before the open and stops after it. `Band::Enclosed` is the assertion, and it holds on a
        // loaded machine because loading the machine can only make the OUTER number larger.
        let outer = Instant::now();
        for _ in 0..3 {
            daemon
                .open_workspace(&root.display().to_string())
                .expect("opens");
        }
        let outer_ns = u64::try_from(outer.elapsed().as_nanos()).unwrap_or(u64::MAX);

        let key = "workspace_operations.index_reconstruction.ns";
        let snapshot = daemon.counter_snapshot();
        let (observations, total) = read(&snapshot, key);
        assert_eq!(observations, 3, "one nanosecond observation per open");
        assert_eq!(
            CounterId::of(key).expect("resolves").spec().determinism(),
            Determinism::LoadDependent,
            "a nanosecond counter must declare itself load-dependent"
        );
        assert!(
            total <= outer_ns,
            "band {}: the counter reported {total} ns inside a section the test measured at \
             {outer_ns} ns, so it is timing something other than what it names",
            Band::Enclosed.word()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_read_that_never_happened_is_not_recorded_as_a_read_of_zero_bytes() {
        let daemon = daemon();
        let snapshot = daemon.counter_snapshot();
        assert_eq!(
            read(&snapshot, "local_filesystem.sequential_read.ops"),
            (0, 0)
        );
        assert_eq!(snapshot.collection.observations_recorded, 0);
    }

    #[test]
    fn the_wired_counters_are_exactly_the_ones_this_build_feeds() {
        let root = scratch("wired");
        seed_records(&root, 4);
        append_partial_frame(&root, 4);

        let daemon = daemon();
        daemon
            .open_workspace(&root.display().to_string())
            .expect("opens");

        // One open on a fragmented journal touches every open/recovery counter. The checkpoint
        // producer is exercised by its dedicated test above; publication remains explicitly
        // unavailable until durable HumanHeld authority exists.
        let snapshot = daemon.counter_snapshot();
        for key in [
            "local_filesystem.sequential_read.ops",
            "local_filesystem.sequential_read.bytes",
            "workspace_operations.index_reconstruction.ops",
            "workspace_operations.index_reconstruction.rows",
            "workspace_operations.index_reconstruction.ns",
            "workspace_operations.crash_recovery.ops",
            "workspace_operations.crash_recovery.records",
            "workspace_operations.crash_recovery.ns",
        ] {
            assert!(
                snapshot.reading(key).expect("catalogue key").observations > 0,
                "`{key}` claims an open/recovery producer and recorded nothing"
            );
        }
        assert_eq!(
            snapshot.produced_count(),
            10,
            "this build feeds ten catalogue counters; the rest are named in `not_yet`"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn collection_records_one_observation_per_counter_touched_and_no_more() {
        let root = scratch("perturbation");
        seed_records(&root, 2);
        let daemon = daemon();
        daemon
            .open_workspace(&root.display().to_string())
            .expect("opens");

        let snapshot = daemon.counter_snapshot();
        let summed: u64 = snapshot
            .readings
            .iter()
            .map(|reading| reading.observations)
            .sum();
        assert_eq!(
            snapshot.collection.observations_recorded, summed,
            "the independent tally and the per-counter sum must agree"
        );
        assert_eq!(
            snapshot.collection.concurrent_observations, 0,
            "nothing else was recording"
        );
        assert_eq!(
            summed, 5,
            "a clean open touches five counters: one read, two of its bytes and operations, \
             and three of the fold"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_registry_used_by_no_daemon_still_answers() {
        let registry = Counters::new();
        registry.record(id("local_filesystem.sequential_read.ops"), 1);
        assert_eq!(registry.snapshot().collection.observations_recorded, 1);
    }
}

// ---------------------------------------------------------------------------
// Family: local filesystem — checked against the operating system
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn the_byte_count_equals_what_the_operating_system_says_the_file_holds() {
    use mesh_daemon::ipc::surface::{nothing_to_recover, Operations, StartupSummary};
    use mesh_daemon::{LiveDaemon, RECORD_FILE_NAME};

    let mut root = std::env::temp_dir();
    root.push(format!("mesh-counters-bytes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("scratch directory");

    let mut bytes = Vec::new();
    for index in 0..9u8 {
        bytes.extend_from_slice(&mesh_store::frame_record(
            &mesh_store::StoredRecord::Operation(mesh_store::OperationRecord {
                id: mesh_store::RecordDigest::from_bytes([index + 1; 32]),
                actor: mesh_store::RecordDigest::from_bytes([9; 32]),
                actor_sequence: u64::from(index) + 1,
                hlc_millis: 1_700_000_000_000,
                hlc_counter: 0,
                policy_epoch: 1,
                session: mesh_store::no_session(),
                payload_digest: mesh_store::RecordDigest::from_bytes([index + 1; 32]),
                parents: Vec::new(),
            }),
        ));
    }
    std::fs::write(root.join(RECORD_FILE_NAME), &bytes).expect("seed the record file");

    // The independent measurement: the kernel's own idea of the file's size. Nothing on the
    // recording path consulted it — the counter is fed from a durable boundary and a tail residue,
    // both computed by scanning the bytes.
    let on_disk = std::fs::metadata(root.join(RECORD_FILE_NAME))
        .expect("stat the record file")
        .len();

    let daemon = LiveDaemon::new(StartupSummary::from(&nothing_to_recover()));
    let opens = 3;
    for _ in 0..opens {
        daemon
            .open_workspace(&root.display().to_string())
            .expect("opens");
    }

    let snapshot = daemon.counter_snapshot();
    assert_eq!(
        read(&snapshot, "local_filesystem.sequential_read.bytes"),
        (opens, on_disk * opens),
        "band {}: a byte total is exact or it is broken",
        Band::Exact.word()
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// Family: synchronization — no producer in this build, so the counter is checked
// ---------------------------------------------------------------------------

#[test]
fn the_transferred_byte_total_equals_a_separately_computed_sum() {
    // Nothing in this build feeds this family, so what is under test is the counter and not a
    // producer. Said plainly here so a reader does not take a green test for a wired transport.
    let snapshot_before = Counters::new().snapshot();
    let unfed: Vec<&str> = snapshot_before
        .not_yet()
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    assert!(
        unfed.contains(&"synchronization.transferred.bytes"),
        "this build opens no transport; the counter must say so"
    );

    let registry = Counters::new();
    let messages: Vec<Vec<u8>> = (1..=12u8).map(|size| vec![size; size as usize]).collect();
    for message in &messages {
        registry.record(id("synchronization.transferred.messages"), 1);
        registry.record(
            id("synchronization.transferred.bytes"),
            message.len() as u64,
        );
    }

    // The independent measurement: a different traversal of the same fixture.
    let expected_bytes: u64 = (1..=12u64).sum();
    let snapshot = registry.snapshot();
    assert_eq!(
        read(&snapshot, "synchronization.transferred.messages"),
        (12, 12)
    );
    assert_eq!(
        read(&snapshot, "synchronization.transferred.bytes"),
        (12, expected_bytes),
        "band {}",
        Band::Exact.word()
    );
}

// ---------------------------------------------------------------------------
// Family: context — no producer in this build, so the counter is checked
// ---------------------------------------------------------------------------

#[test]
fn the_cache_counts_equal_a_separately_computed_hit_and_miss_tally() {
    let registry = Counters::new();
    let requests = ["a", "b", "a", "c", "b", "a", "d", "a"];
    let mut held: BTreeMap<&str, u64> = BTreeMap::new();

    for request in requests {
        registry.record(id("context.cache.lookups"), 1);
        let seen_before = held.contains_key(request);
        if seen_before {
            registry.record(id("context.cache.hits"), 1);
        }
        *held.entry(request).or_insert(0) += 1;
    }

    // The independent measurement: a hit is any request after the first for the same key, which is
    // the total minus the number of distinct keys. Computed from the fixture, not from the loop.
    let distinct = requests
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len() as u64;
    let expected_hits = requests.len() as u64 - distinct;

    let snapshot = registry.snapshot();
    assert_eq!(
        read(&snapshot, "context.cache.lookups"),
        (requests.len() as u64, requests.len() as u64)
    );
    assert_eq!(
        read(&snapshot, "context.cache.hits"),
        (expected_hits, expected_hits),
        "band {}: the hit rate is two exact counts and never a stored ratio",
        Band::Exact.word()
    );
}

// ---------------------------------------------------------------------------
// Collection overhead, and the perturbation it causes
// ---------------------------------------------------------------------------

#[test]
fn the_cost_of_collection_is_a_number_rather_than_the_word_negligible() {
    let registry = Counters::new();
    let overhead = registry.overhead();

    assert_eq!(
        overhead.allocations_per_observation, 0,
        "an allocation inside an observation would perturb what is being measured"
    );
    assert_eq!(overhead.atomic_writes_per_observation, 5);
    assert_eq!(overhead.atomic_writes_per_group, 2);
    assert_eq!(overhead.counters, counter_count() as u64);
    // The owned heap slots plus the complete registry value (slice pointer, length, and inline
    // tallies). Omitting the owner metadata publishes a smaller footprint than the live object.
    let owned_bytes = core::mem::size_of_val(&registry) as u64
        + counter_count() as u64 * 2 * core::mem::size_of::<std::sync::atomic::AtomicU64>() as u64;
    assert_eq!(overhead.state_bytes, owned_bytes);
    assert!(
        overhead.state_bytes < 4096,
        "the whole counter plane must stay smaller than one page; it is {} bytes",
        overhead.state_bytes
    );
}

#[test]
fn recording_perturbs_nothing_except_the_counter_it_names() {
    let registry = Counters::new();
    let target = id("context.tokens.unique");
    let observations = 5_000;
    for _ in 0..observations {
        registry.record(target, 3);
    }

    let snapshot = registry.snapshot();
    assert_eq!(
        snapshot.collection.observations_recorded, observations,
        "every observation landed and none was invented"
    );
    assert_eq!(
        read(&snapshot, "context.tokens.unique"),
        (observations, observations * 3)
    );
    for reading in &snapshot.readings {
        if reading.key() != "context.tokens.unique" {
            assert_eq!(
                (reading.observations, reading.total),
                (0, 0),
                "recording into one counter moved `{}`",
                reading.key()
            );
        }
    }
}

#[test]
fn taking_a_snapshot_does_not_move_a_counter() {
    let registry = Counters::new();
    registry.record(id("context.tokens.repeated"), 11);
    let first = registry.snapshot();
    for _ in 0..64 {
        let _ = registry.snapshot();
    }
    let last = registry.snapshot();
    assert_eq!(
        first.readings, last.readings,
        "sixty-six snapshots changed a value, so reading perturbs the measurement"
    );
    assert_eq!(
        last.collection.snapshots_taken, 66,
        "collection's own footprint is reported rather than hidden"
    );
}

#[test]
fn concurrent_recording_is_reported_and_not_smoothed_away() {
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    let registry = Arc::new(Counters::new());
    let target = id("context.task.completions");
    let (recorded, ready) = mpsc::channel();
    let (resume, resumed) = mpsc::channel();
    let writer = {
        let registry = Arc::clone(&registry);
        std::thread::spawn(move || {
            for _ in 0..10_000 {
                registry.record(target, 1);
            }
            recorded.send(()).expect("reader exists");
            resumed
                .recv_timeout(Duration::from_secs(10))
                .expect("reader observed mid-flight state");
            for _ in 0..10_000 {
                registry.record(target, 1);
            }
        })
    };
    // Guarantee a real observation before all queued work finishes, even when the OS schedules
    // the writer first. The remaining half still races the reader as in the original test.
    ready
        .recv_timeout(Duration::from_secs(10))
        .expect("first batch recorded");
    let halfway = registry.snapshot();
    assert_eq!(read(&halfway, "context.task.completions"), (10_000, 10_000));
    assert!(halfway.collection.snapshot_consistent);
    resume.send(()).expect("writer remains alive");
    let mut snapshots = 1_u64;
    while !writer.is_finished() {
        let snapshot = registry.snapshot();
        // What is asserted mid-flight is only what the bracketing tally can prove: the window can
        // be measured, and it can be no wider than the work that was queued.
        //
        // What is deliberately NOT asserted is a direction between the independent tally and the
        // per-counter sum. They occupy different positions in the global atomic order while a
        // writer overlaps the pass, so neither numeric direction is promised. An earlier version
        // of this test asserted one direction and failed on the trial merge's first concurrent
        // run. A flaky assertion teaches its readers to re-run rather than read, which is the
        // failure this whole task is about.
        assert!(snapshot.collection.concurrent_observations <= 20_000);
        if snapshot.collection.snapshot_consistent {
            assert_eq!(
                snapshot.collection.observations_recorded,
                snapshot.collection.observations_summed
            );
        }
        snapshots += 1;
    }
    writer.join().expect("no recorder panics");

    // Once the writer is gone, every number settles and every one of them is exact. This is the
    // assertion that carries the weight: not one observation was lost, and not one was invented.
    let settled = registry.snapshot();
    assert_eq!(
        settled.collection.concurrent_observations, 0,
        "nothing is recording now"
    );
    assert!(settled.collection.snapshot_consistent);
    assert_eq!(
        settled.collection.observations_recorded, settled.collection.observations_summed,
        "the independent tally and the per-counter sum must agree once nothing is racing"
    );
    assert_eq!(read(&settled, "context.task.completions"), (20_000, 20_000));
    assert!(snapshots > 0, "the reader never got a look in");
}

// ---------------------------------------------------------------------------
// Queryable without running a benchmark
// ---------------------------------------------------------------------------

#[test]
fn a_registry_that_has_measured_nothing_still_answers_every_counter() {
    let snapshot = Counters::new().snapshot();
    assert_eq!(snapshot.readings.len(), counter_count());
    assert_eq!(
        snapshot.not_yet().len() + snapshot.produced_count(),
        counter_count(),
        "every counter is either fed or named as unfed"
    );
    for (key, reason) in snapshot.not_yet() {
        assert!(!reason.is_empty(), "`{key}` is unfed and does not say why");
    }
}

#[test]
fn the_published_shape_carries_its_conditions_before_its_numbers() {
    let snapshot = Counters::new().snapshot();
    let mut rendered = String::new();
    snapshot.to_json().write(&mut rendered);

    assert!(
        rendered.starts_with("{\"conditions\":\""),
        "a reader must meet the conditions before the numbers: {}",
        &rendered[..rendered.len().min(80)]
    );
    assert!(
        rendered.contains("benchmarks/runners/README.md"),
        "the conditions must name the document that states them"
    );
    assert!(rendered.contains("\"determinism\":\"deterministic\""));
    assert!(rendered.contains("\"determinism\":\"load-dependent\""));
    assert!(rendered.contains("\"integer_encoding\":\"decimal-u64\""));
    assert!(rendered.contains("\"band\":\"exact\""));
    assert!(rendered.contains("\"band\":\"enclosed\""));
    assert!(rendered.contains("\"writers_in_flight_before_readings\":\"0\""));
    assert!(rendered.contains("\"writers_in_flight_after_readings\":\"0\""));
    assert!(rendered.contains("\"snapshot_consistent\":true"));
}
