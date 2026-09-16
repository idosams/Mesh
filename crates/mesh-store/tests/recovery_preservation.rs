//! Production recovery-preservation transitions against the executable v0 compatibility contract.

use std::convert::Infallible;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use mesh_store::{
    BoundaryEvidenceKind, Checkpoint, ChunkPromoter, DurableCommit, PrivateSaved, RecordDigest,
    RecoveryBoundaryEvidence, RecoveryEventUlid, RecoveryMachine, RecoveryMachineError,
    RecoveryPreserved, RecoveryProductStatus, RecoverySequence, RecoverySnapshot, RecoveryStamp,
    RecoveryStateError, RecoveryStatePersistence, RecoveryTransition, RecoveryTrigger,
    RecoveryTriggerInput, Sqlite, Store, TriggerEffect, RECOVERY_PRESERVATION_CONTRACT,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CrashStage {
    BeforeWrite,
    AfterBytesBeforePointer,
    AfterPointer,
}

#[derive(Clone, Debug, Default)]
struct MemoryPersistence {
    shared: Arc<Mutex<MemoryState>>,
}

#[derive(Debug, Default)]
struct MemoryState {
    admitted: Option<RecoverySnapshot>,
    orphan: Option<RecoveryPreserved>,
    crash: Option<CrashStage>,
    persists: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SimulatedCrash(CrashStage);

impl std::fmt::Display for SimulatedCrash {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "simulated crash at {:?}", self.0)
    }
}

impl RecoveryStatePersistence for MemoryPersistence {
    type Error = SimulatedCrash;

    fn load(&mut self) -> Result<Option<RecoverySnapshot>, Self::Error> {
        Ok(self
            .shared
            .lock()
            .expect("memory state lock")
            .admitted
            .clone())
    }

    fn persist(&mut self, snapshot: &RecoverySnapshot) -> Result<(), Self::Error> {
        let mut state = self.shared.lock().expect("memory state lock");
        state.persists += 1;
        match state.crash.take() {
            Some(CrashStage::BeforeWrite) => Err(SimulatedCrash(CrashStage::BeforeWrite)),
            Some(CrashStage::AfterBytesBeforePointer) => {
                state.orphan = snapshot.latest_recovery().cloned();
                Err(SimulatedCrash(CrashStage::AfterBytesBeforePointer))
            }
            Some(CrashStage::AfterPointer) => {
                state.admitted = Some(snapshot.clone());
                Ok(())
            }
            None => {
                state.admitted = Some(snapshot.clone());
                Ok(())
            }
        }
    }

    fn persist_observation_recovery(
        &mut self,
        snapshot: &RecoverySnapshot,
    ) -> Result<(), Self::Error> {
        self.persist(snapshot)
    }
}

impl MemoryPersistence {
    fn crash_next(&self, stage: CrashStage) {
        self.shared.lock().expect("memory state lock").crash = Some(stage);
    }

    fn persist_count(&self) -> usize {
        self.shared.lock().expect("memory state lock").persists
    }

    fn orphan(&self) -> Option<RecoveryPreserved> {
        self.shared
            .lock()
            .expect("memory state lock")
            .orphan
            .clone()
    }
}

#[derive(Default)]
struct NoChunks;

impl ChunkPromoter for NoChunks {
    type Error = Infallible;

    fn write_temporary(&mut self, chunks: &[Vec<u8>]) -> Result<Vec<RecordDigest>, Self::Error> {
        assert!(chunks.is_empty());
        Ok(Vec::new())
    }

    fn flush_temporary(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn verify_temporary(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn promote(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn discard_temporary(&mut self) -> Result<usize, Self::Error> {
        Ok(0)
    }

    fn is_durable(&self, _digest: &RecordDigest) -> bool {
        false
    }
}

fn sequence(value: u64) -> RecoverySequence {
    RecoverySequence::new(value).expect("test sequences are non-zero")
}

fn digest(byte: u8) -> RecordDigest {
    RecordDigest::from_bytes([byte; 32])
}

fn stamp(lamport: u64, event: u8, content: u8) -> RecoveryStamp {
    RecoveryStamp::new(
        lamport,
        RecoveryEventUlid::from_bytes([event; 16]),
        digest(content),
    )
}

fn recovery(lamport: u64, event: u8, content: u8, through: u64) -> RecoveryPreserved {
    RecoveryPreserved::from_verified_bytes(
        stamp(lamport, event, content),
        sequence(through),
        vec![content, event],
        digest(content),
    )
    .expect("test recovery bytes have a verified hash")
}

fn unique_database() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "mesh-recovery-preservation-{}-{}.sqlite",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn private_saved() -> PrivateSaved {
    let path = unique_database();
    let sqlite = Sqlite::open(&path).expect("opens bundled SQLite");
    let mut store = Store::open(sqlite).expect("migrates the test index");
    let mut promoter = NoChunks;
    let saved = DurableCommit::new(&mut store, &mut promoter, Vec::new(), Checkpoint::default())
        .finish()
        .expect("empty meaningful checkpoint commits");
    let acknowledgement = *saved.acknowledgement();
    drop(store);
    let _ = fs::remove_file(path.with_extension("sqlite-shm"));
    let _ = fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = fs::remove_file(path);
    acknowledgement
}

fn open() -> (RecoveryMachine<MemoryPersistence>, MemoryPersistence) {
    let persistence = MemoryPersistence::default();
    let machine = RecoveryMachine::open(persistence.clone()).expect("new recovery machine opens");
    (machine, persistence)
}

#[test]
fn production_vocabulary_tracks_the_executable_contract() {
    let contract = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/compatibility/recovery-preservation/v0/contract.json"),
    )
    .expect("integrated recovery contract is present");

    assert!(contract.contains(&format!(
        "\"contract\": \"{RECOVERY_PRESERVATION_CONTRACT}\""
    )));
    assert_eq!(contract.matches("\"effect\": \"recovery-only\"").count(), 5);
    assert_eq!(contract.matches("\"effect\": \"evidence-only\"").count(), 3);
    assert_eq!(
        RecoveryTrigger::ALL.map(RecoveryTrigger::effect),
        [
            TriggerEffect::MeaningfulAfterSettling,
            TriggerEffect::RecoveryOnly,
            TriggerEffect::RecoveryOnly,
            TriggerEffect::RecoveryOnly,
            TriggerEffect::RecoveryOnly,
            TriggerEffect::RecoveryOnly,
            TriggerEffect::EvidenceOnly,
            TriggerEffect::EvidenceOnly,
            TriggerEffect::EvidenceOnly,
        ]
    );
    for trigger_id in [
        "actor-becomes-idle-after-settling",
        "integrated-agent-requests-flush",
        "actor-process-exits",
        "maximum-uncheckpointed-bytes-or-time",
        "user-opens-review",
        "actor-disconnects",
        "modified-file-handle-closes",
        "fsync-completes",
        "atomic-replacement-completes",
    ] {
        assert!(contract.contains(trigger_id), "contract lost {trigger_id}");
    }
}

#[test]
fn all_nine_triggers_preserve_the_meaningful_boundary() {
    let (mut machine, persistence) = open();
    for value in 1..=10 {
        machine.observe(sequence(value)).expect("window advances");
    }

    for (trigger, kind) in [
        (
            RecoveryTrigger::ModifiedFileHandleClosed,
            BoundaryEvidenceKind::Closed,
        ),
        (
            RecoveryTrigger::FsyncCompleted,
            BoundaryEvidenceKind::Synced,
        ),
        (
            RecoveryTrigger::AtomicReplacementCompleted,
            BoundaryEvidenceKind::RenamedIntoPlace,
        ),
    ] {
        let transition = machine
            .apply(
                trigger,
                RecoveryTriggerInput::Evidence(RecoveryBoundaryEvidence::new(sequence(1), kind)),
            )
            .expect("evidence trigger succeeds");
        assert!(transition.private_saved().is_none());
    }
    assert_eq!(
        persistence.persist_count(),
        0,
        "evidence never moves a pointer"
    );

    let recovery_only = [
        RecoveryTrigger::IntegratedAgentRequestsFlush,
        RecoveryTrigger::ActorProcessExited,
        RecoveryTrigger::MaximumUncheckpointedBytesOrTime,
        RecoveryTrigger::UserOpenedReview,
        RecoveryTrigger::ActorDisconnected,
    ];
    for (offset, trigger) in recovery_only.into_iter().enumerate() {
        let transition = machine
            .apply(
                trigger,
                RecoveryTriggerInput::Recovery(recovery(
                    offset as u64 + 1,
                    offset as u8 + 1,
                    offset as u8 + 1,
                    offset as u64 + 2,
                )),
            )
            .expect("recovery-only trigger succeeds");
        assert!(matches!(
            transition,
            RecoveryTransition::RecoveryPreserved(_)
        ));
        assert!(transition.private_saved().is_none());
        assert!(machine.snapshot().last_meaningful().is_none());
        assert!(machine.snapshot().open_window().is_some());
        assert_eq!(machine.status(), RecoveryProductStatus::Working);
    }

    let acknowledgement = private_saved();
    let transition = machine
        .apply(
            RecoveryTrigger::ActorBecameIdleAfterSettling,
            RecoveryTriggerInput::Meaningful {
                stamp: stamp(20, 20, 20),
                through: sequence(10),
                acknowledgement,
            },
        )
        .expect("settled meaningful save succeeds");
    assert_eq!(transition.private_saved(), Some(&acknowledgement));
    assert!(machine.snapshot().last_meaningful().is_some());
    assert!(machine.snapshot().latest_recovery().is_some());
    assert!(machine.snapshot().open_window().is_none());
    assert_eq!(machine.status(), RecoveryProductStatus::SavedPrivately);
    assert_eq!(persistence.persist_count(), 6);
}

#[test]
fn a_new_window_cannot_reuse_or_rewind_the_meaningful_sequence() {
    let (mut machine, _) = open();
    machine.observe(sequence(10)).expect("first window");
    machine
        .apply(
            RecoveryTrigger::ActorBecameIdleAfterSettling,
            RecoveryTriggerInput::Meaningful {
                stamp: stamp(10, 10, 10),
                through: sequence(10),
                acknowledgement: private_saved(),
            },
        )
        .expect("meaningful checkpoint");

    assert_eq!(
        machine.observe(sequence(10)),
        Err(RecoveryStateError::SequenceOutsideWindow)
    );
    assert_eq!(
        machine.observe(sequence(9)),
        Err(RecoveryStateError::SequenceOutsideWindow)
    );
    machine
        .observe(sequence(11))
        .expect("the next strict sequence opens a new window");
}

#[test]
fn stamp_order_is_lexicographic_and_exact_duplicates_are_idempotent() {
    let candidates = [
        recovery(1, 255, 255, 1),
        recovery(2, 1, 255, 1),
        recovery(2, 2, 1, 1),
        recovery(2, 2, 2, 1),
    ];
    let mut orders = Vec::new();
    for first in 0..4 {
        for second in 0..4 {
            for third in 0..4 {
                for fourth in 0..4 {
                    let order = [first, second, third, fourth];
                    if order
                        .iter()
                        .enumerate()
                        .all(|(index, value)| !order[..index].contains(value))
                    {
                        orders.push(order);
                    }
                }
            }
        }
    }
    assert_eq!(orders.len(), 24, "all input permutations are exercised");

    for order in orders {
        let (mut machine, _) = open();
        machine.observe(sequence(1)).expect("window opens");
        for index in order {
            machine
                .apply(
                    RecoveryTrigger::IntegratedAgentRequestsFlush,
                    RecoveryTriggerInput::Recovery(candidates[index].clone()),
                )
                .expect("ordered recovery candidate applies");
        }
        assert_eq!(machine.snapshot().latest_recovery(), Some(&candidates[3]));
        assert_eq!(
            machine
                .apply(
                    RecoveryTrigger::IntegratedAgentRequestsFlush,
                    RecoveryTriggerInput::Recovery(candidates[3].clone()),
                )
                .expect("exact duplicate is accepted"),
            RecoveryTransition::Duplicate
        );
        assert_eq!(
            machine
                .apply(
                    RecoveryTrigger::IntegratedAgentRequestsFlush,
                    RecoveryTriggerInput::Recovery(candidates[0].clone()),
                )
                .expect("older candidate is safely ignored"),
            RecoveryTransition::OlderIgnored
        );
    }
}

#[test]
fn every_recovery_only_trigger_restores_after_restart_without_private_saved() {
    for (offset, trigger) in [
        RecoveryTrigger::IntegratedAgentRequestsFlush,
        RecoveryTrigger::ActorProcessExited,
        RecoveryTrigger::MaximumUncheckpointedBytesOrTime,
        RecoveryTrigger::UserOpenedReview,
        RecoveryTrigger::ActorDisconnected,
    ]
    .into_iter()
    .enumerate()
    {
        let (mut machine, persistence) = open();
        machine.observe(sequence(1)).expect("window opens");
        let expected = recovery(1, offset as u8, offset as u8 + 1, 1);
        let transition = machine
            .apply(trigger, RecoveryTriggerInput::Recovery(expected.clone()))
            .expect("recovery record persists");
        assert!(transition.private_saved().is_none());
        drop(machine);

        let restarted = RecoveryMachine::open(persistence).expect("restart restores snapshot");
        assert_eq!(restarted.snapshot().latest_recovery(), Some(&expected));
        assert!(restarted.snapshot().last_meaningful().is_none());
        assert!(restarted.snapshot().open_window().is_some());
        assert_eq!(restarted.status(), RecoveryProductStatus::Working);
    }
}

#[test]
fn persistence_failures_keep_the_prior_admitted_state() {
    let (mut machine, persistence) = open();
    machine.observe(sequence(1)).expect("window opens");
    let first = recovery(1, 1, 1, 1);
    machine
        .apply(
            RecoveryTrigger::IntegratedAgentRequestsFlush,
            RecoveryTriggerInput::Recovery(first.clone()),
        )
        .expect("baseline recovery persists");

    for (stage, next) in [
        (CrashStage::BeforeWrite, recovery(2, 2, 2, 1)),
        (CrashStage::AfterBytesBeforePointer, recovery(3, 3, 3, 1)),
    ] {
        persistence.crash_next(stage);
        let error = machine
            .apply(
                RecoveryTrigger::ActorProcessExited,
                RecoveryTriggerInput::Recovery(next.clone()),
            )
            .expect_err("simulated persistence crash is reported");
        assert!(matches!(error, RecoveryMachineError::Persistence { .. }));
        assert_eq!(error.status(), RecoveryProductStatus::NeedsAttention);
        assert_eq!(machine.snapshot().latest_recovery(), Some(&first));
        let restarted =
            RecoveryMachine::open(persistence.clone()).expect("prior pointer remains restartable");
        assert_eq!(restarted.snapshot().latest_recovery(), Some(&first));
        if stage == CrashStage::AfterBytesBeforePointer {
            assert_eq!(persistence.orphan(), Some(next));
        }
    }
}

#[test]
fn meaningful_persistence_failure_keeps_the_window_open_without_emitting_an_ack() {
    let (mut machine, persistence) = open();
    machine.observe(sequence(1)).expect("window opens");
    persistence.crash_next(CrashStage::AfterBytesBeforePointer);
    let error = machine
        .apply(
            RecoveryTrigger::ActorBecameIdleAfterSettling,
            RecoveryTriggerInput::Meaningful {
                stamp: stamp(1, 1, 1),
                through: sequence(1),
                acknowledgement: private_saved(),
            },
        )
        .expect_err("a failed meaningful pointer cannot acknowledge the transition");

    assert!(matches!(error, RecoveryMachineError::Persistence { .. }));
    assert!(machine.snapshot().last_meaningful().is_none());
    assert!(machine.snapshot().open_window().is_some());
    assert_eq!(machine.status(), RecoveryProductStatus::Working);
    let restarted = RecoveryMachine::open(persistence).expect("old snapshot remains durable");
    assert!(restarted.snapshot().last_meaningful().is_none());
}

#[test]
fn atomic_replace_and_settled_idle_are_distinct_transitions() {
    let (mut machine, _) = open();
    for value in 1..=5 {
        machine.observe(sequence(value)).expect("window advances");
    }
    let evidence = machine
        .apply(
            RecoveryTrigger::AtomicReplacementCompleted,
            RecoveryTriggerInput::Evidence(RecoveryBoundaryEvidence::new(
                sequence(5),
                BoundaryEvidenceKind::RenamedIntoPlace,
            )),
        )
        .expect("atomic replacement is evidence");
    assert!(evidence.private_saved().is_none());
    assert!(machine.snapshot().last_meaningful().is_none());

    let preserved = recovery(5, 5, 5, 5);
    machine
        .apply(
            RecoveryTrigger::IntegratedAgentRequestsFlush,
            RecoveryTriggerInput::Recovery(preserved.clone()),
        )
        .expect("recovery pointer advances separately");
    let acknowledgement = private_saved();
    machine
        .apply(
            RecoveryTrigger::ActorBecameIdleAfterSettling,
            RecoveryTriggerInput::Meaningful {
                stamp: stamp(6, 6, 6),
                through: sequence(5),
                acknowledgement,
            },
        )
        .expect("idle after settling closes the window");
    assert_eq!(machine.snapshot().latest_recovery(), Some(&preserved));
    assert_eq!(machine.status(), RecoveryProductStatus::SavedPrivately);
}

#[test]
fn multi_object_window_restores_then_closes_at_its_complete_prefix() {
    let (mut machine, persistence) = open();
    for value in 1..=9 {
        machine
            .observe(sequence(value))
            .expect("first object group advances");
    }
    let preserved = recovery(9, 9, 9, 9);
    machine
        .apply(
            RecoveryTrigger::MaximumUncheckpointedBytesOrTime,
            RecoveryTriggerInput::Recovery(preserved.clone()),
        )
        .expect("prefix recovery persists");
    drop(machine);

    let mut restarted = RecoveryMachine::open(persistence).expect("prefix restores");
    for value in 10..=15 {
        restarted
            .observe(sequence(value))
            .expect("second object group advances");
    }
    let acknowledgement = private_saved();
    restarted
        .apply(
            RecoveryTrigger::ActorBecameIdleAfterSettling,
            RecoveryTriggerInput::Meaningful {
                stamp: stamp(15, 15, 15),
                through: sequence(15),
                acknowledgement,
            },
        )
        .expect("complete multi-object prefix becomes meaningful");
    assert_eq!(restarted.snapshot().latest_recovery(), Some(&preserved));
    assert_eq!(
        restarted
            .snapshot()
            .last_meaningful()
            .expect("meaningful checkpoint exists")
            .through(),
        sequence(15)
    );
}

#[test]
fn crash_stages_admit_only_the_atomic_pointer_winner() {
    for stage in [
        CrashStage::BeforeWrite,
        CrashStage::AfterBytesBeforePointer,
        CrashStage::AfterPointer,
    ] {
        let (mut machine, persistence) = open();
        machine.observe(sequence(1)).expect("window opens");
        let expected = recovery(1, 1, 1, 1);
        persistence.crash_next(stage);
        let result = machine.apply(
            RecoveryTrigger::ActorDisconnected,
            RecoveryTriggerInput::Recovery(expected.clone()),
        );
        drop(machine);

        let restarted = RecoveryMachine::open(persistence).expect("durable state remains valid");
        match stage {
            CrashStage::BeforeWrite | CrashStage::AfterBytesBeforePointer => {
                assert!(result.is_err());
                assert!(restarted.snapshot().latest_recovery().is_none());
            }
            CrashStage::AfterPointer => {
                assert!(result.is_ok());
                assert_eq!(restarted.snapshot().latest_recovery(), Some(&expected));
            }
        }
    }
}

#[test]
fn wrong_trigger_shapes_and_conflicting_duplicates_fail_closed() {
    let (mut machine, _) = open();
    machine.observe(sequence(1)).expect("window opens");
    let wrong = machine
        .apply(
            RecoveryTrigger::ActorProcessExited,
            RecoveryTriggerInput::Evidence(RecoveryBoundaryEvidence::new(
                sequence(1),
                BoundaryEvidenceKind::Closed,
            )),
        )
        .expect_err("shape mismatch is refused");
    assert_eq!(
        wrong,
        RecoveryMachineError::State(RecoveryStateError::WrongInputForTrigger)
    );

    let first = recovery(1, 1, 1, 1);
    machine
        .apply(
            RecoveryTrigger::ActorProcessExited,
            RecoveryTriggerInput::Recovery(first),
        )
        .expect("first record persists");
    let conflicting =
        RecoveryPreserved::from_verified_bytes(stamp(1, 1, 1), sequence(1), vec![9], digest(1))
            .expect("verifier identity matches the reused stamp");
    let error = machine
        .apply(
            RecoveryTrigger::ActorProcessExited,
            RecoveryTriggerInput::Recovery(conflicting),
        )
        .expect_err("same stamp cannot rename different bytes");
    assert_eq!(
        error,
        RecoveryMachineError::State(RecoveryStateError::ConflictingDuplicate)
    );
}
