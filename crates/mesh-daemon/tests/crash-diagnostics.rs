//! Killing a real daemon-side save at every step of plan §6.3, and checking what the report says.
//!
//! # The claim under test
//!
//! > **The reported last durable boundary is the recovered state.** Not close to it, not usually
//! > it — the same number, at every point a process can die, including the points where the answer
//! > is "nothing survived" and the points where the answer is "everything you were told about did".
//!
//! A boundary report is the one diagnostic a person has no way to check. If it says four saves
//! survived and three did, nothing in the product contradicts it and the mistake is discovered by
//! whoever loses the fourth. So the report is not asserted against a constant here: it is asserted
//! against the *fold*, record by record, after every kill.
//!
//! # Why a killed child and not a simulated interruption
//!
//! An interruption modelled by returning `Err` runs destructors, flushes buffers and performs the
//! cleanup a crash skips. So each case spawns **this same test binary** as a child, drives it to a
//! chosen step of plan §6.3's commit sequence, and `SIGKILL`s it. `ExitStatus::code()` returning
//! `None` is asserted every time, so a child that exited on its own can never be read as a crash.
//!
//! The machinery is a smaller relative of `crates/mesh-store/tests/support/crash.rs` and is not
//! shared with it: a test-support module lives inside one crate's `tests/` directory and is not a
//! published item, so the alternative to a second copy is a dependency edge between two crates'
//! test trees, which Cargo does not have. This copy is deliberately narrower — the daemon spawns
//! no database engine, so there is no grandchild to collect.
//!
//! # How the eleven steps land on a daemon that has one file
//!
//! The daemon's whole durable resource is `records.mesh`: whole frames, each forced to disk before
//! its append returns (`docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md` is why
//! there is no database beside it). Plan §6.3's steps map onto that resource without inventing
//! one:
//!
//! | Plan step | What the child does | What is durable afterwards |
//! |---|---|---|
//! | 1–3 write, flush, verify chunks | writes, forces and re-reads a scratch file | nothing addressable |
//! | 4 promote chunks | renames it into the content folder | content, referenced by nothing |
//! | 5–8 begin, insert, heads, outbox | builds the records in memory | nothing |
//! | 9 commit | appends every record, each forced to disk | **all of them** |
//! | 10 report saved | writes and forces the acknowledgement marker | the marker |
//! | 11 replicate | nothing durable | unchanged |
//!
//! `SequenceStep::residue_if_killed_after` is plan §6.3's crash clause as data, and every kill in
//! the first campaign is cross-checked against it rather than against a number written here twice.
//!
//! # Every kill point is a place, never a moment
//!
//! No case here sleeps. The child reaches the point it was told to reach, prints a ready line, and
//! waits; the parent kills it there. That is what lets the interior campaign assert an exact
//! number — *three records were appended, so three survived* — instead of the weaker property that
//! the report and the fold agree on whatever happened.
//!
//! The first version of this file did sleep, for fractions of a span it measured with a
//! calibration run, and it cost twice over. Its own interior assertion could only be "these two
//! numbers match", and the sustained `fsync` load of its 240-record children pushed `mesh-store`'s
//! two calibrated crash campaigns off the spans *they* had measured: `npm test` on the merge
//! result failed **2 times in 6** against **0 in 4** on `origin/main`, always at
//! `kills_inside_the_transaction_leave_all_of_it_or_none_of_it`, and always with that campaign's
//! own "the calibration is wrong on this machine". The fragility is `01KZERXN1BC2FEDNNXNBKTNY7E`,
//! filed before this task began; the load that exposed it was this file's, and the fix was this
//! file's to make. The campaign now runs in 0.23 s rather than 5.8 s and writes 15 records rather
//! than roughly 1 450.
//!
//! # What a `SIGKILL` proves here, and what it does not
//!
//! It covers every failure in which the machine keeps running. It does **not** prove behaviour
//! under power loss: the page cache survives a killed process and does not survive a power cut, so
//! a torn frame is *rare* under `SIGKILL` and ordinary under a power cut. That is why the two
//! byte-states a torn append leaves — a fragment behind whole records, and a fragment with nothing
//! behind it — are asserted directly, on files written to be exactly those states. The report is a
//! pure function of the bytes, so constructing the bytes is what makes the assertion deterministic;
//! the kill campaign shows the code path is real, not that a particular byte-state is common.
//!
//! ## A third byte-state, measured rather than predicted
//!
//! This file first asserted that a kill can only *truncate* an append, and that a whole frame with
//! wrong bytes therefore had to be a durability defect. **That assertion is false, and this suite
//! is what falsified it.** Under load, one kill inside the append left `records.mesh` at exactly
//! 12 032 bytes — 64 frames — with frames 0 to 62 whole and byte 11 844 onward, a full frame's
//! width, entirely zero. The file had been *extended* by a frame whose data never landed.
//! `scan_journal` reads a zero run as `DamageKind::NotAFrame`, so 63 durable records became
//! unopenable, and the recovery consequence of that is filed as `01KZFQBSZW0W2BKXE05HCTEMNH`
//! rather than fixed here — the fix is a change to what the *store* calls damage, and this task
//! owns the report rather than the recovery.
//!
//! What this suite asserts is therefore the thing that must hold whatever the bytes turn out to
//! be: **a refused open still names the boundary exactly** — `saved_records` is the intact prefix,
//! to the record and to the byte — **and nothing acknowledged is ever behind that boundary.** An
//! assertion that a byte-state cannot occur is a prediction; those two are properties.

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

use mesh_daemon::workspace::{OpenFailure, OpenWorkspace, RecordFile, RECORD_FILE_NAME};
use mesh_daemon::{CrashReport, Severity};
use mesh_store::{
    frame_record, journal_records, no_session, CrashResidue, OperationRecord, RecordDigest,
    RecordJournal as _, SequenceStep, StoredRecord,
};

/// What the child prints when it has reached its stop point. Matched as a substring: libtest
/// writes `test <name> ... ` with no newline before the body runs.
const READY_TOKEN: &str = "MESH-DAEMON-CRASH-READY";

/// Where the child saves.
const ROOT_VARIABLE: &str = "MESH_DAEMON_CRASH_ROOT";
/// The last plan §6.3 step the child performs before it waits to be killed.
const STOP_VARIABLE: &str = "MESH_DAEMON_CRASH_STOP";
/// How many records the child's save carries.
const RECORDS_VARIABLE: &str = "MESH_DAEMON_CRASH_RECORDS";

/// The value of [`STOP_VARIABLE`] meaning "stop before performing any step".
const STOP_BEFORE_ANY: &str = "none";

/// The [`STOP_VARIABLE`] prefix meaning "stop *inside* step 9, after this many records".
///
/// Step 9 is the only step with an interior, and an interior kill point is where a boundary report
/// is easiest to get wrong. `stop-inside-commit:3` is a place, not a moment: whatever the machine
/// is doing, the child appends exactly three whole frames — each forced to disk before the next
/// begins — and then waits. See `the_reported_boundary_is_the_recovered_state_inside_the_append`
/// for why this is a schedule rather than a stopwatch.
const STOP_INSIDE_COMMIT: &str = "stop-inside-commit:";

/// The libtest name of the child entry point.
const CHILD_TEST: &str = "crash_child";

/// The file whose existence means the user was told the work is saved privately.
const ACKNOWLEDGED_MARKER: &str = "acknowledged";
/// Where an unpromoted scratch file lives.
const SCRATCH: &str = "scratch.part";
/// Where a promoted one lives.
const CONTENT: &str = "content.whole";

/// How many records the step-boundary campaign saves.
const BOUNDARY_RECORDS: u64 = 5;
/// How many the interior campaign saves, so there are interior points strictly inside step 9.
const INTERIOR_RECORDS: u64 = 6;

// ---------------------------------------------------------------------------------------------
// The child
// ---------------------------------------------------------------------------------------------

/// The child process's body. Inert unless the parent set [`ROOT_VARIABLE`].
///
/// Run as an ordinary case in this suite it asserts its own inertness, so it is not a test that
/// passes by doing nothing without saying so.
#[test]
fn crash_child() {
    let Ok(root) = std::env::var(ROOT_VARIABLE) else {
        assert!(
            std::env::var(STOP_VARIABLE).is_err(),
            "the child was told where to stop and not where to work; the parent sets both or \
             neither"
        );
        return;
    };
    let root = PathBuf::from(root);
    let stop = std::env::var(STOP_VARIABLE).expect("the parent sets the stop point");
    let count: u64 = std::env::var(RECORDS_VARIABLE)
        .expect("the parent sets the record count")
        .parse()
        .expect("the record count is a number");

    // Opening the workspace is the daemon's own recovery path, and it runs before any save.
    let _ = OpenWorkspace::open(&root).expect("a fresh or recovered workspace opens");

    if stop == STOP_BEFORE_ANY {
        announce_ready();
        wait_to_be_killed();
    }

    // A stop point strictly inside step 9: every step before it in full, then exactly this many
    // records of the append, then nothing.
    if let Some(appended) = stop.strip_prefix(STOP_INSIDE_COMMIT) {
        let appended: u64 = appended
            .parse()
            .expect("the interior stop point is a number");
        assert!(
            appended < count,
            "an interior stop point must leave records unappended; {appended} of {count} does not"
        );
        for step in SequenceStep::ORDER {
            if step == SequenceStep::CommitTransaction {
                break;
            }
            perform(step, &root, count);
        }
        append(&root, appended);
        announce_ready();
        wait_to_be_killed();
    }

    let stop_after = SequenceStep::from_name(&stop).expect("the stop point names a plan step");

    for step in SequenceStep::ORDER {
        perform(step, &root, count);
        if step == stop_after {
            break;
        }
    }
    announce_ready();
    wait_to_be_killed();
}

/// Append the first `how_many` records of the save, each forced to disk before the next begins.
fn append(root: &Path, how_many: u64) {
    let mut file = RecordFile::open(&journal_path(root)).expect("the record file opens");
    journal_records(&mut file, records(how_many).iter()).expect("every record is appended");
}

/// One plan §6.3 step, as this daemon's one durable resource can perform it.
fn perform(step: SequenceStep, root: &Path, count: u64) {
    match step {
        SequenceStep::WriteChunks => {
            std::fs::write(root.join(SCRATCH), payload()).expect("scratch is writable");
        }
        SequenceStep::FlushChunks => {
            std::fs::File::open(root.join(SCRATCH))
                .expect("scratch opens")
                .sync_all()
                .expect("scratch is forced to disk");
        }
        SequenceStep::VerifyChunks => {
            assert_eq!(
                std::fs::read(root.join(SCRATCH)).expect("scratch reads back"),
                payload(),
                "the scratch file did not read back as what was written"
            );
        }
        SequenceStep::PromoteChunks => {
            std::fs::rename(root.join(SCRATCH), root.join(CONTENT)).expect("promotion is a rename");
        }
        // Steps 5 to 8 compose the save in memory. Nothing durable moves, which is exactly what
        // the parent asserts after killing here.
        SequenceStep::BeginTransaction
        | SequenceStep::InsertImmutable
        | SequenceStep::AdvanceHeads
        | SequenceStep::FillOutbox => {
            let _ = records(count);
        }
        SequenceStep::CommitTransaction => append(root, count),
        SequenceStep::ReportSaved => {
            let mut marker =
                std::fs::File::create(root.join(ACKNOWLEDGED_MARKER)).expect("the marker is made");
            marker
                .write_all(b"saved privately\n")
                .expect("the marker is written");
            marker.sync_all().expect("the marker is durable");
        }
        SequenceStep::Replicate => {}
    }
}

fn payload() -> Vec<u8> {
    (0..4096u32).map(|byte| byte as u8).collect()
}

fn operation(index: u64) -> StoredRecord {
    let mut id = [0u8; 32];
    id[0..8].copy_from_slice(&index.to_be_bytes());
    StoredRecord::Operation(OperationRecord {
        id: RecordDigest::from_bytes(id),
        actor: RecordDigest::from_bytes([9; 32]),
        actor_sequence: index + 1,
        hlc_millis: 1_700_000_000_000,
        hlc_counter: index,
        policy_epoch: 1,
        session: no_session(),
        payload_digest: RecordDigest::from_bytes([7; 32]),
        parents: Vec::new(),
    })
}

fn records(count: u64) -> Vec<StoredRecord> {
    (0..count).map(operation).collect()
}

fn announce_ready() {
    println!("{READY_TOKEN}");
    std::io::stdout().flush().expect("stdout flushes");
}

fn wait_to_be_killed() -> ! {
    for _ in 0..600 {
        std::thread::sleep(Duration::from_millis(100));
    }
    // An orphan must not outlive the suite, and it must not look like a clean exit either.
    std::process::exit(97);
}

// ---------------------------------------------------------------------------------------------
// The parent
// ---------------------------------------------------------------------------------------------

/// Spawn the child, wait until it says it has reached `stop`, and `SIGKILL` it there.
///
/// Readiness is a line the child prints, never an elapsed duration. A campaign that sleeps for a
/// fraction of a measured span is a campaign whose kill points move with the machine's load — it
/// passes on an idle laptop, misses the interval it was aiming at under a full test run, and takes
/// its neighbours' timing-calibrated suites with it. `01KZERXN1BC2FEDNNXNBKTNY7E` is that defect,
/// already filed against two campaigns in `mesh-store`, and this file declines to be a third.
fn spawn_and_kill(root: &Path, stop: &str, count: u64) -> ExitStatus {
    let binary = std::env::current_exe().expect("the test binary knows its own path");
    let mut child = Command::new(binary)
        .args([CHILD_TEST, "--exact", "--nocapture", "--test-threads=1"])
        .env(ROOT_VARIABLE, root.display().to_string())
        .env(STOP_VARIABLE, stop)
        .env(RECORDS_VARIABLE, count.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the child test binary starts");

    let stdout = child.stdout.take().expect("the child's stdout is piped");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .expect("the child's output is readable");
        if read == 0 {
            let _ = child.kill();
            let status = child.wait().expect("the child is reaped");
            panic!("the child stopping at {stop} exited ({status:?}) before announcing readiness");
        }
        if line.contains(READY_TOKEN) {
            break;
        }
    }

    sigkill(child.id());
    child.wait().expect("the child is reaped")
}

/// `SIGKILL` through `/bin/kill`, which is how this is sent without an `unsafe` block or a
/// dependency on `libc`. A failure to send fails the test rather than skipping it silently.
fn sigkill(process: u32) {
    let output = Command::new("/bin/kill")
        .args(["-9", &process.to_string()])
        .output()
        .expect("/bin/kill runs; this suite cannot test a crash without it");
    assert!(
        output.status.success(),
        "/bin/kill -9 {process} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_killed(status: ExitStatus, context: &str) {
    assert!(
        status.code().is_none(),
        "the child at {context} exited with {status:?} instead of being killed by a signal, so \
         nothing about a crash was tested"
    );
}

fn scratch_root(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "mesh-crash-diagnostics-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("the workspace folder is made");
    path
}

/// Whether the user had been told the work was saved privately before the kill.
fn acknowledged(root: &Path) -> bool {
    root.join(ACKNOWLEDGED_MARKER).exists()
}

fn file_bytes(root: &Path) -> u64 {
    std::fs::metadata(journal_path(root)).map_or(0, |data| data.len())
}

fn journal_path(root: &Path) -> PathBuf {
    let private = root.join(".mesh");
    if private.is_dir() {
        private.join(RECORD_FILE_NAME)
    } else {
        root.join(RECORD_FILE_NAME)
    }
}

// ---------------------------------------------------------------------------------------------
// What the parent checks after every kill
// ---------------------------------------------------------------------------------------------

/// The four invariants, checked on every corpse this file produces.
///
/// 1. **The reported boundary is the recovered state.** The record count in the report is the
///    number of records the fold actually indexed — not a number the scan carried past it.
/// 2. **The report accounts for every byte.** The boundary plus the unfinished bytes is the file's
///    length, so nothing is quietly unexplained.
/// 3. **An acknowledgement is never ahead of the report.** If the marker is on disk, every record
///    the save carried is in the report.
/// 4. **A refusal still names the boundary.** An open that fails is the case where the report is
///    the only thing a person has, so it is held to the *same* arithmetic: the record count and
///    the byte offset are the intact prefix the scan reached, exactly.
fn assert_report_is_the_recovered_state(
    root: &Path,
    expected_records: u64,
    context: &str,
) -> CrashReport {
    let open = match OpenWorkspace::open(root) {
        Ok(open) => open,
        Err(failure) => {
            assert!(
                !acknowledged(root),
                "{context}: the user was told the work was saved privately and the workspace will \
                 not open ({failure}). This is acknowledged-state loss, which is a P0."
            );
            let report = CrashReport::of_failure(&failure, Duration::from_millis(0));
            assert!(
                !report.is_serving(),
                "{context}: a refusal that reads as serving"
            );
            assert_eq!(
                report.severity(),
                Severity::Blocking,
                "{context}: a workspace that will not open was not reported loudly"
            );
            // See this module's header: a kill can leave the file extended by a frame's width of
            // zeroes, which the scan calls damage. The byte-state was a surprise; what is asserted
            // is not that it cannot happen but that the report survives it intact.
            if let OpenFailure::Damaged(damage) = &failure {
                let intact = damage.intact_prefix();
                assert_eq!(
                    report.saved_records(),
                    intact.records,
                    "{context}: the report and the damage disagree about how many records are whole"
                );
                assert_eq!(
                    report.boundary_bytes(),
                    intact.byte_offset,
                    "{context}: the report and the damage disagree about where durability stops"
                );
                assert!(
                    report.boundary_bytes() <= file_bytes(root),
                    "{context}: the reported boundary is past the end of the file"
                );
            }
            return report;
        }
    };

    let report = CrashReport::of(&open);

    assert_eq!(
        report.saved_records(),
        open.operations() as u64,
        "{context}: the report says {} records survived and the fold indexed {}",
        report.saved_records(),
        open.operations()
    );
    assert_eq!(
        report.boundary_bytes() + report.unfinished_bytes(),
        file_bytes(root),
        "{context}: the report does not account for every byte in the file"
    );
    if acknowledged(root) {
        assert_eq!(
            report.saved_records(),
            expected_records,
            "{context}: the user was told the work was saved privately and the report says only \
             {} of {expected_records} records survived. This is acknowledged-state loss, which is \
             a P0.",
            report.saved_records()
        );
        assert!(
            report.is_serving(),
            "{context}: acknowledged work is on disk and the workspace will not serve"
        );
    }
    report
}

// ---------------------------------------------------------------------------------------------
// Campaign 1 — every step boundary
// ---------------------------------------------------------------------------------------------

/// Plan §6.3's eleven steps plus the point before any of them: twelve kills, each cross-checked
/// against the residue class the plan names for it.
#[test]
fn the_reported_boundary_is_the_recovered_state_at_every_step_boundary() {
    for (index, stop) in std::iter::once(STOP_BEFORE_ANY.to_owned())
        .chain(
            SequenceStep::ORDER
                .iter()
                .map(|step| step.name().to_owned()),
        )
        .enumerate()
    {
        let root = scratch_root(&format!("step-{index}"));
        assert_killed(spawn_and_kill(&root, &stop, BOUNDARY_RECORDS), &stop);

        let context = format!("killed after {stop}");
        let report = assert_report_is_the_recovered_state(&root, BOUNDARY_RECORDS, &context);

        match SequenceStep::from_name(&stop) {
            None => {
                assert_eq!(
                    report.saved_records(),
                    0,
                    "{context}: records survived a kill before the first step ran"
                );
                assert!(!acknowledged(&root));
            }
            Some(step) => match step.residue_if_killed_after() {
                // Before the append, nothing durable is a record — whether or not a chunk was
                // promoted, which is the difference between the plan's first two classes and is
                // not a difference the boundary report can see.
                CrashResidue::DiscardTemporary | CrashResidue::CollectUnreferencedChunks => {
                    assert_eq!(
                        report.saved_records(),
                        0,
                        "{context}: the plan says nothing is recoverable yet and {} records were \
                         reported",
                        report.saved_records()
                    );
                    assert!(
                        !acknowledged(&root),
                        "{context}: the user was told before the save was durable"
                    );
                    assert!(report.is_serving());
                }
                CrashResidue::RecoverCheckpoint => {
                    assert_eq!(
                        report.saved_records(),
                        BOUNDARY_RECORDS,
                        "{context}: the plan says the save is durable and the report disagrees"
                    );
                    assert_eq!(report.unfinished_bytes(), 0);
                    assert!(report.is_serving());
                    // Step 9 makes it durable; the user is told at step 10.
                    assert_eq!(acknowledged(&root), step.acknowledged_by_here());
                }
            },
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// The promoted content of steps 4 to 8 is on disk and referenced by nothing, and the report does
/// not count it. A report that counted unreferenced content would tell a person their save
/// survived because a temporary file did.
#[test]
fn content_promoted_before_the_save_is_not_reported_as_saved_work() {
    let root = scratch_root("promoted");
    assert_killed(
        spawn_and_kill(&root, SequenceStep::FillOutbox.name(), BOUNDARY_RECORDS),
        "fill-outbox",
    );

    assert!(
        root.join(CONTENT).is_file(),
        "the promotion did not happen, so this test is checking nothing"
    );
    let report = assert_report_is_the_recovered_state(&root, BOUNDARY_RECORDS, "after fill-outbox");
    assert_eq!(report.saved_records(), 0);
    assert!(!acknowledged(&root));
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------------------------
// Campaign 2 — inside the append, where a step boundary cannot reach
// ---------------------------------------------------------------------------------------------

/// Step 9 is not an instant: it appends every record, forcing each one to disk before the next
/// begins. The boundary campaign can only kill *between* steps, so this one kills at five points
/// strictly inside step 9 — after one record, after two, and so on — and asserts the exact number.
///
/// **The kill point is a place, not a moment.** The child reaches the interior point, says so, and
/// waits; the parent kills it there. So the assertion is `saved_records == appended` rather than
/// "whatever the report and the fold happen to agree on", which is the strongest form this
/// property has, and it holds on a loaded machine and an idle one alike. The earlier version of
/// this campaign slept for a fraction of a calibrated span, and it cost twice: its own assertion
/// was only that two numbers agreed, and the fsync load it generated pushed
/// `mesh-store`'s two calibrated campaigns off their measured spans — `01KZERXN1BC2FEDNNXNBKTNY7E`
/// — turning `npm test` red twice in six runs against zero in four on `origin/main`.
#[test]
fn the_reported_boundary_is_the_recovered_state_inside_the_append() {
    for appended in 1..INTERIOR_RECORDS {
        let root = scratch_root(&format!("inside-{appended}"));
        let stop = format!("{STOP_INSIDE_COMMIT}{appended}");
        assert_killed(spawn_and_kill(&root, &stop, INTERIOR_RECORDS), &stop);

        let context =
            format!("killed after {appended} of {INTERIOR_RECORDS} records were appended");
        let report = assert_report_is_the_recovered_state(&root, INTERIOR_RECORDS, &context);

        assert_eq!(
            report.saved_records(),
            appended,
            "{context}: the report says {} survived",
            report.saved_records()
        );
        assert_eq!(
            report.unfinished_bytes(),
            0,
            "{context}: every append returns a whole frame, so an interior kill leaves none"
        );
        assert!(
            report.is_serving(),
            "{context}: the survivors are still served"
        );
        assert!(
            !acknowledged(&root),
            "{context}: the user was told the save had finished while it was still running"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

// ---------------------------------------------------------------------------------------------
// The two byte-states a torn append leaves, asserted directly
// ---------------------------------------------------------------------------------------------

/// A fragment behind whole records: the survivors are reported, and the unfinished bytes are
/// reported as what nobody was ever told about.
#[test]
fn an_unfinished_save_behind_whole_records_is_separated_from_them() {
    let root = scratch_root("fragment");
    let path = root.join(RECORD_FILE_NAME);
    let mut file = RecordFile::open(&path).expect("the record file opens");
    let whole = journal_records(&mut file, records(3).iter()).expect("append");
    let torn = frame_record(&operation(3));
    let kept = torn.len() / 2;
    file.append(&torn[..kept]).expect("a torn append");

    let open = OpenWorkspace::open(&root).expect("a fragment is not damage");
    let report = CrashReport::of(&open);

    assert_eq!(report.saved_records(), 3);
    assert_eq!(report.boundary_bytes(), whole);
    assert_eq!(report.unfinished_bytes(), kept as u64);
    assert!(report.interrupted());
    assert!(report.is_serving());
    assert_eq!(report.severity(), Severity::Notable);
    let _ = std::fs::remove_dir_all(&root);
}

/// A fragment with nothing behind it: there is no boundary, and the report says so instead of
/// reading like the report an empty folder produces.
#[test]
fn a_crash_with_no_recoverable_boundary_is_loud_and_not_a_clean_start() {
    let crashed = scratch_root("no-boundary");
    let torn = frame_record(&operation(0));
    let kept = torn.len() / 2;
    std::fs::write(crashed.join(RECORD_FILE_NAME), &torn[..kept]).expect("write");

    let failure = OpenWorkspace::open(&crashed).expect_err("there is no boundary to open at");
    assert_eq!(failure.code(), "workspace-nothing-readable");
    let report = CrashReport::of_failure(&failure, Duration::from_millis(1));
    assert_eq!(report.saved_records(), 0);
    assert_eq!(report.unfinished_bytes(), kept as u64);
    assert_eq!(report.severity(), Severity::Blocking);
    assert!(!report.is_serving());

    let empty = scratch_root("empty");
    let opened = OpenWorkspace::open(&empty).expect("an empty folder opens");
    let clean = CrashReport::of(&opened);
    assert_eq!(clean.saved_records(), report.saved_records());
    assert!(clean.is_serving());
    assert_ne!(
        clean.sentence(),
        report.sentence(),
        "a crash that recovered nothing reads exactly like a first start"
    );
    assert_ne!(clean.severity(), report.severity());

    let _ = std::fs::remove_dir_all(&crashed);
    let _ = std::fs::remove_dir_all(&empty);
}

/// The corrupted-ledger case the contract asks for: a whole frame whose bytes are wrong is a loud
/// failure, and the report still says how far the file was good.
#[test]
fn a_corrupted_record_is_a_loud_failure_that_still_names_the_boundary_before_it() {
    let root = scratch_root("corrupt");
    let path = root.join(RECORD_FILE_NAME);
    let mut file = RecordFile::open(&path).expect("the record file opens");
    let whole = journal_records(&mut file, records(2).iter()).expect("append");
    journal_records(&mut file, [&operation(2)]).expect("append");

    let mut bytes = std::fs::read(&path).expect("read");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    std::fs::write(&path, &bytes).expect("write");

    let failure = OpenWorkspace::open(&root).expect_err("a wrong byte is damage");
    assert_eq!(failure.code(), "workspace-damaged");
    let report = CrashReport::of_failure(&failure, Duration::from_millis(1));
    assert_eq!(report.saved_records(), 2, "the two whole records before it");
    assert_eq!(report.boundary_bytes(), whole);
    assert_eq!(report.severity(), Severity::Blocking);
    assert!(!report.is_serving());
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------------------------
// The support bundle's section
// ---------------------------------------------------------------------------------------------

/// The report a support bundle carries, taken from a workspace a real kill produced.
///
/// The bundle itself is `01KZC2XVD81A2183MWE5VDC9ER`, which depends on this task and owns
/// `tools/support-bundle/**`. What is asserted here is the section it includes: the counts are the
/// ones the report just made, and the workspace path is not in it.
#[test]
fn the_bundle_section_carries_the_boundary_and_no_path() {
    let root = scratch_root("bundle");
    assert_killed(
        spawn_and_kill(&root, SequenceStep::ReportSaved.name(), BOUNDARY_RECORDS),
        "report-saved",
    );

    let report =
        assert_report_is_the_recovered_state(&root, BOUNDARY_RECORDS, "after report-saved");
    let section = report.to_bundle_section().to_string();

    assert!(section.contains(CrashReport::BUNDLE_SECTION), "{section}");
    assert!(
        section.contains(&format!("\"saved_records\":{}", BOUNDARY_RECORDS)),
        "{section}"
    );
    assert!(section.contains("\"unfinished_bytes\":0"), "{section}");
    assert!(
        !section.contains(&root.display().to_string()),
        "the workspace path reached the bundle: {section}"
    );
    assert!(
        !section.contains("mesh-crash-diagnostics"),
        "a path fragment reached the bundle: {section}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
