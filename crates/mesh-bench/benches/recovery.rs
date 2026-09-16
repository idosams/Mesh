//! The recovery bench: how long it takes to put a workspace's index back from records alone.
//!
//! ```text
//! cargo bench -p mesh-bench --bench recovery
//! cargo bench -p mesh-bench --bench recovery -- --records 20000
//! ```
//!
//! # What is timed, and what is deliberately not
//!
//! One iteration is a whole reconstruction: **read the durable record journal, verify every frame,
//! find the last durable boundary, fold every record into a fresh index, and digest it.** That is
//! the part of plan §6.3's recovery that is this program's own work, and it is the part that grows
//! with the workspace.
//!
//! What is **not** timed is the SQL that writes the rebuilt tables back. `mesh-store` ships no
//! SQLite driver — `docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md` — so a number
//! measured here would be a number about whatever driver the bench happened to bring, which is not
//! a number about Mesh. The end-to-end recovery, SQL and all, is measured and held to plan §6.3's
//! five-second budget in `crates/mesh-store/tests/recovery.rs`, against real SQLite, with the
//! measured milliseconds printed on every run.
//!
//! Stating that split is the point. A benchmark that quietly measured half of a thing and reported
//! it under the whole thing's name is exactly the failure plan §2.10 is about.
//!
//! # Correctness before timing
//!
//! [`Workload::verify`] rebuilds once and compares the index digest against the digest the same
//! records produced when they were first saved. A reconstruction that lands anywhere else is plan
//! §6.3's P0, and the harness refuses to time a workload that has not verified.
//!
//! `harness = false`: the bench owns its `main`, so `--records` reaches this code rather than
//! libtest, and no benchmark framework sits between the clock and the workload.

use mesh_bench::clock::{MonotonicClock, SystemWallClock};
use mesh_bench::env::SystemProbe;
use mesh_bench::harness::{Harness, RunConfig, RunOutcome};
use mesh_bench::json::{Json, JsonObject};
use mesh_bench::schema::{CacheState, Verification, WorkloadDescriptor};
use mesh_bench::workload::{Workload, WorkloadError};
use mesh_store::{
    frame_record, no_session, scan_journal, AckRecord, ApprovalRecord, ChunkSlice, ContextAccess,
    ContextRecord, Digest16, Index, ManifestRecord, OperationRecord, PeerRecord, RecordDigest,
    ReviewRecord, ReviewVerdict, StoredRecord, TailResidue,
};

/// How many operations the default corpus carries.
const DEFAULT_OPERATIONS: u64 = 20_000;

/// The seed this corpus is generated from. Fixed, because a benchmark whose input changes between
/// runs measures the input.
const SEED: u64 = 42;

fn main() {
    // Cargo passes `--bench` to benchmark targets; anything else is ours.
    let arguments: Vec<String> = std::env::args()
        .skip(1)
        .filter(|argument| argument != "--bench")
        .collect();
    let operations = match parse_operations(&arguments) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("recovery: {message}");
            std::process::exit(2);
        }
    };

    if let Err(error) = measured_run(operations) {
        eprintln!("recovery: {error}");
        std::process::exit(1);
    }
}

fn parse_operations(arguments: &[String]) -> Result<u64, String> {
    let mut operations = DEFAULT_OPERATIONS;
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index] == "--records" {
            let value = arguments
                .get(index + 1)
                .ok_or_else(|| "--records needs a number".to_owned())?;
            operations = value
                .parse()
                .map_err(|_| format!("--records {value:?} is not a number"))?;
            if operations == 0 {
                return Err("--records 0 measures nothing".to_owned());
            }
            index += 2;
            continue;
        }
        return Err(format!("unrecognised argument {:?}", arguments[index]));
    }
    Ok(operations)
}

fn measured_run(operations: u64) -> Result<(), String> {
    let probe = SystemProbe::here().map_err(|error| error.to_string())?;
    let harness = Harness::new(MonotonicClock::new(), SystemWallClock, probe);
    let config = RunConfig::new(
        "mesh-store/recovery/rebuild-index-from-records",
        "cargo bench -p mesh-bench --bench recovery",
    )
    // Warm: the journal bytes are in memory before timing starts, so what is measured is the
    // reconstruction rather than this machine's page cache. A cold-cache recovery number belongs
    // with a real driver and is measured in `crates/mesh-store/tests/recovery.rs`.
    .with_cache_state(CacheState::Warm)
    .with_iterations(20)
    .with_warmup_iterations(3);

    let mut workload = RecoveryWorkload::new(operations);
    let journal_bytes = workload.journal.len();
    let records = workload.record_count;

    match harness
        .run(&mut workload, &config)
        .map_err(|error| error.to_string())?
    {
        RunOutcome::Measured(result) => {
            println!("{}", result.to_json_pretty());
            println!(
                "recovery: {records} records over {journal_bytes} journal bytes; the SQL rewrite \
                 is measured end to end in crates/mesh-store/tests/recovery.rs"
            );
            Ok(())
        }
        RunOutcome::VerificationFailed(failure) => Err(failure.to_string()),
    }
}

/// A journal of `operations` operations plus one of every other record kind, and the digest the
/// index they describe has to come back to.
struct RecoveryWorkload {
    operations: u64,
    record_count: u64,
    journal: Vec<u8>,
    expected: Digest16,
}

impl RecoveryWorkload {
    fn new(operations: u64) -> Self {
        let records = corpus(operations);
        let journal: Vec<u8> = records.iter().flat_map(frame_record).collect();
        let expected = fold(&records);
        Self {
            operations,
            record_count: records.len() as u64,
            journal,
            expected,
        }
    }

    /// One whole reconstruction, which is what an iteration times.
    fn rebuild(&self) -> Result<Digest16, WorkloadError> {
        let scan = scan_journal(&self.journal)
            .map_err(|damage| WorkloadError::new(format!("the corpus is damaged: {damage}")))?;
        if scan.tail() != TailResidue::Whole {
            return Err(WorkloadError::new(
                "the corpus journal ends mid-record, so this measures a truncation and not a \
                 recovery",
            ));
        }
        let (index, report) = mesh_store::rebuild(scan.into_records(), Vec::new())
            .map_err(|error| WorkloadError::new(error.to_string()))?;
        if report.records_replayed as u64 != self.record_count {
            return Err(WorkloadError::new(format!(
                "{} of {} records were replayed",
                report.records_replayed, self.record_count
            )));
        }
        Ok(index.default_digest())
    }
}

impl Workload for RecoveryWorkload {
    fn descriptor(&self) -> WorkloadDescriptor {
        WorkloadDescriptor {
            generator: "mesh-store/recovery-corpus".to_owned(),
            generator_version: "1".to_owned(),
            seed: SEED,
            parameters: JsonObject::new()
                .with("operations", Json::Uint(self.operations))
                .with("records", Json::Uint(self.record_count))
                .with("journal_bytes", Json::Uint(self.journal.len() as u64))
                .with("timed_phases", Json::string("scan+verify+fold+digest")),
        }
    }

    fn verify(&mut self) -> Result<Verification, WorkloadError> {
        let observed = self.rebuild()?;
        Ok(Verification::new(
            "index digest after a rebuild from records alone",
            self.expected.to_hex(),
            observed.to_hex(),
        ))
    }

    fn prepare(&mut self, _cache_state: CacheState) -> Result<(), WorkloadError> {
        // Nothing to warm and nothing to drop: the journal is already bytes in memory, and the
        // index a rebuild produces is discarded at the end of every iteration.
        Ok(())
    }

    fn iterate(&mut self) -> Result<(), WorkloadError> {
        let digest = self.rebuild()?;
        // Held inside the timed section on purpose: without it an optimiser is free to discard the
        // whole fold, and a benchmark of a discarded fold measures nothing.
        if digest != self.expected {
            return Err(WorkloadError::new("the rebuild diverged mid-run"));
        }
        Ok(())
    }
}

fn fold(records: &[StoredRecord]) -> Digest16 {
    let mut index = Index::new();
    for record in records {
        index
            .apply(record.clone())
            .expect("the corpus is self-consistent");
    }
    index.default_digest()
}

fn digest(seed: u8) -> RecordDigest {
    RecordDigest::from_bytes([seed; 32])
}

fn operation_id(index: u64) -> RecordDigest {
    let mut id = [0u8; 32];
    id[0..8].copy_from_slice(&index.to_be_bytes());
    id[31] = 0xA1;
    RecordDigest::from_bytes(id)
}

/// The corpus: one actor's chain of `operations` operations, a manifest, a peer, an
/// acknowledgement, a review, an approval and a context entry — so the fold is exercised over
/// every table rather than over the operation table alone.
fn corpus(operations: u64) -> Vec<StoredRecord> {
    let mut records: Vec<StoredRecord> = (0..operations)
        .map(|index| {
            StoredRecord::Operation(OperationRecord {
                id: operation_id(index),
                actor: digest(2),
                actor_sequence: index + 1,
                hlc_millis: 1_700_000_000_000 + index,
                hlc_counter: index,
                policy_epoch: 3,
                session: no_session(),
                payload_digest: digest(9),
                parents: if index == 0 {
                    Vec::new()
                } else {
                    vec![operation_id(index - 1)]
                },
            })
        })
        .collect();

    records.push(StoredRecord::Manifest(ManifestRecord {
        id: digest(20),
        byte_length: 64 * 1024,
        content_digest: digest(21),
        chunks: vec![ChunkSlice {
            digest: digest(22),
            byte_offset: 0,
            byte_length: 64 * 1024,
        }],
    }));
    records.push(StoredRecord::Peer(PeerRecord {
        peer: digest(30),
        joined_at: operation_id(0),
    }));
    records.push(StoredRecord::Acknowledgement(AckRecord {
        peer: digest(30),
        actor: digest(2),
        actor_sequence: 1,
    }));
    records.push(StoredRecord::Review(ReviewRecord {
        bundle: digest(40),
        subject_operation: operation_id(0),
        opened_by: digest(2),
    }));
    records.push(StoredRecord::Approval(ApprovalRecord {
        approval: digest(41),
        bundle: digest(40),
        approver: digest(2),
        verdict: ReviewVerdict::Approved,
    }));
    records.push(StoredRecord::ContextEntry(ContextRecord {
        entry: digest(50),
        session: no_session(),
        operation: operation_id(0),
        access: ContextAccess::Wrote,
        byte_length: 60,
    }));
    records
}
