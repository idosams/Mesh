# tests/crash

**Maturity: implemented cross-component evidence index.** The executing harnesses live beside the
sequences they test. See [Project status](../../docs/project-status.md) for the product boundary.

Research item **R2** — *what is the exact durable commit sequence, and what survives a kill after
each step?* — answered by killing real processes and reading the corpse.

This directory is the **index and the finding**. It holds no executing code, and
[§5](#5-why-no-runner-lives-here) states why that is deliberate rather than unfinished. The
harnesses that do the killing live in the crates whose sequences they kill, and they already run
unattended on every `npm test`:

| Harness | Kills | Runs under |
|---|---|---|
| [`crates/mesh-store/tests/crash-commit-sequence.rs`](../../crates/mesh-store/tests/crash-commit-sequence.rs) | plan §6.3's eleven steps, and inside the transaction | `cargo nextest run --workspace` |
| [`crates/mesh-cas/tests/crash-promotion.rs`](../../crates/mesh-cas/tests/crash-promotion.rs) | the six promotion steps that compose plan §6.3 steps 1–4, and inside them | `cargo nextest run --workspace` |
| [`crates/mesh-cas/tests/sequence_steps_agree_with_mesh_store.rs`](../../crates/mesh-cas/tests/sequence_steps_agree_with_mesh_store.rs) | nothing — it holds the seam between the two step enumerations shut | `cargo nextest run --workspace` |

The recovery specification these observations produced is
[`docs/consistency.md`](../../docs/consistency.md) §5.1–§5.4. This file is the measurement; that
one is the contract.

---

## 1. Reproduction

```sh
# the two kill campaigns alone
PATH="$HOME/.cargo/bin:$PATH" cargo nextest run -p mesh-store -p mesh-cas \
  -E 'binary(crash-commit-sequence) or binary(crash-promotion)'

# the crates whole, which is what the merge path runs
PATH="$HOME/.cargo/bin:$PATH" cargo nextest run -p mesh-store -p mesh-cas
```

**Campaign as run.** Every field, because a campaign recorded after the fact has none of them.

| Field | Value |
|---|---|
| Revision | `0d1f069618d241b6b83c3fbd07953f0569c6cfe6` |
| Seed range | **none, and none is needed — see [§4.7](#47-the-timing-campaigns-were-calibrated-not-seeded--repaired)**. All four campaigns are *exhaustive over an enumeration* rather than sampled: the boundary campaigns over the step enumerations, the two interior campaigns over `PausePoint::ORDER` (sixteen points inside the six promotion steps) and `Prefix::ORDER` (twelve statement offsets inside the transaction). Nothing anywhere is drawn from a random source or from a clock |
| Operation mix | store: one 64 KiB chunk, one manifest, one peer, and either 1 operation (boundary campaign) or 2 000 (interior campaign). CAS: one chunk of 64 KiB (boundary) or 2 MiB (interior) |
| Fault mix | `SIGKILL` of a real child at a chosen step, or at a chosen point *inside* one — a partial staged write, an `fsync` in flight, a torn arrival record, a `rename` about to run or just returned, an open uncommitted SQLite transaction at a chosen statement offset; injected `ENOSPC` at plan steps 1–4; a step-9 transaction failure induced by replacing the database file with a directory |
| Protocol version | the step enumerations `SequenceStep::ORDER` (eleven) and `PromotionStep::ORDER` (six), pinned against each other by `sequence_steps_agree_with_mesh_store.rs` |
| Configuration digest | SQLite under WAL with `synchronous = NORMAL` and foreign keys on, asserted by `mesh-store sql::tests::wal_and_foreign_keys_are_both_set` |
| Build profile | `test` — unoptimized with debuginfo. `rustc 1.97.1 (8bab26f4f 2026-07-14)`, `cargo-nextest 0.9.143`, `aarch64-apple-darwin`, macOS 14.5, 10 cores, APFS |

**Observed.** Three consecutive runs of the two kill binaries: **18 tests, 18 passed, 0 skipped,
exit 0**, in 5.395 s, 5.508 s and 5.238 s (5.90 s, 5.62 s, 5.34 s wall). The two crates whole:
**253 tests, 253 passed, exit 0**, 22.443 s. The whole workspace: **1437 tests, exit 0**, in five
consecutive runs of 30.7 s to 54.1 s — and **two failures across the eleven whole-suite runs this
spike made**, which was
[§4.7](#47-the-timing-campaigns-were-calibrated-not-seeded--repaired).

**Zero durability violations.** No corpse in any run was ever found carrying the acknowledgement
record with a database that did not hold the checkpoint, and no corpse was ever found holding a
reference to content that was not there. The residue at every step matched the declared class.

**One defect found, filed as `01KZERXN1BC2FEDNNXNBKTNY7E`, since repaired** — not a durability
defect but an evidence defect: both interior campaigns derived their kill schedule from a single
calibration sample, and both failed when machine load rose between the sample and the campaign.
[§4.7](#47-the-timing-campaigns-were-calibrated-not-seeded--repaired) has the reproduction, what
replaced the schedule, and the measurement after the repair. It was filed rather than fixed by the
spike because a spike does not spawn the run that repairs what it found (ADR-0004), and because
repairing a harness is not what a task allowed `tests/crash/**` and `docs/consistency.md` may do.

Everything else in [§4](#4-what-is-not-covered) is uncovered ground rather than a contradicted
claim, and is recorded rather than filed.

---

## 2. What is killed, and how many times

**47 real child processes are `SIGKILL`ed per run**, plus five in-process fault injections. Every
kill asserts `ExitStatus::code() == None`, so a child that exited on its own can never be mistaken
for one that was killed.

### 2.1 The durable commit sequence — 24 killed children, 5 injected faults

| Test | Kills | What it asserts |
|---|---|---|
| `killing_before_any_step_leaves_the_workspace_untouched` | 1 | nothing acknowledged, nothing staged, nothing indexed, no chunk |
| `killing_after_each_of_the_eleven_steps_leaves_exactly_what_the_plan_says` | 11 | the exact residue plan §6.3 names, per step — see [§3](#3-the-observed-residue-per-step) |
| `kills_inside_the_transaction_leave_all_of_it_or_none_of_it` | 12 | the index holds 0 or 2 000 operations and never a number in between, **and that which of the two it is agrees with whether `COMMIT` was sent** |
| `every_interior_prefix_is_nameable_and_cuts_the_batch_where_it_says`, `the_batch_splits_into_whole_statements`, `the_straddle_check_still_fails_when_the_schedule_covers_one_side` | — | the twelve statement offsets are distinct, nameable and cut the batch where they claim; the splitter keeps a two-line statement whole; the straddle check still fails on a one-sided schedule |
| `a_full_disk_at_any_chunk_step_acknowledges_nothing_and_indexes_nothing` | 4 injected `ENOSPC` | the failure is reported at the step it was injected at, nothing is acknowledged, the index stays empty |
| `a_failing_transaction_acknowledges_nothing_and_says_the_outcome_is_unknown` | 1 induced failure | the outcome is reported as *unknown*, not as a clean rollback, and nothing is acknowledged |
| `durability_is_reached_before_the_acknowledgement_is_given` | — | step 9 makes the checkpoint recoverable, step 10 tells the user, in that order and never the other |
| `every_sequence_step_has_a_kill_point` | — | eleven steps, each nameable on the child's command line; a twelfth step fails this |

### 2.2 Chunk promotion — 23 killed children

| Test | Kills | What it asserts |
|---|---|---|
| `killing_before_any_step_leaves_the_store_untouched` | 1 | nothing staged, no chunk, no arrival |
| six per-step tests, one each for stage / flush / verify / record-arrival / link / sync-directory | 6 | the exact residue per step, **and** that a restart's discard removes exactly the staged file and then removes nothing |
| `randomised_kills_never_leave_a_partial_chunk` | 16 | no partial chunk is ever visible, **and** that each corpse falls on the side of the rename its interior point names |
| `every_promotion_step_has_a_kill_test` | — | the six-step enumeration matches the tests, in order |
| `every_interior_point_is_nameable_and_distinct`, `the_straddle_check_still_fails_when_the_schedule_covers_one_side`, `a_planted_partial_chunk_is_still_rejected` | — | the sixteen interior points are distinct and nameable; the straddle check still fails on a one-sided schedule; a planted truncated chunk is still rejected by the invariant |

Both interior campaigns **fail loudly when their schedule stops straddling the boundary** — "the
schedule places all of its kills on one side" is an assertion, not a shrug, and each one has a test
that watches it fail. That is what stops a repair from being indistinguishable from deleting the
check. The kill points themselves are named rather than timed
([§4.7](#47-the-timing-campaigns-were-calibrated-not-seeded--repaired)), so neither campaign's
verdict is a function of how busy the machine is.

---

## 3. The observed residue, per step

Two invariants are checked on **every** corpse this suite produces, at every step, not only at the
interesting ones:

1. **An acknowledgement is never ahead of durability.** If the marker the child wrote and
   `fsync`ed the instant it was told "saved privately" is present, the database holds the whole
   checkpoint. This is acknowledged-state loss stated as a one-line implication checkable on a
   corpse.
2. **A durable reference is never ahead of content.** If the database references a chunk, the
   chunk is in the store, whole, and hashing to its own name.

Plus: a chunk that became visible always carries a recorded arrival, so it is a collection
candidate rather than a leak.

| Step | Name | Observed on disk after the kill | Told? |
|---|---|---|---|
| 1 | write chunks | 1 staged file, no chunk, no arrival, empty index | no |
| 2 | flush chunks | as 1 — durable but not addressable | no |
| 3 | verify chunks | as 1 — verification reads nothing into the store | no |
| 4 | promote chunks | chunk present, whole, hashing to its name; arrival recorded; **0 staged files**; empty index; **no reference** | no |
| 5 | begin transaction | as 4 — the index is untouched | no |
| 6 | insert immutable records | as 4 | no |
| 7 | advance heads | as 4 | no |
| 8 | fill outbox | as 4 | no |
| 9 | commit transaction | chunk present; operations, heads and manifest references all durable | **no** |
| 10 | report saved | as 9 | **yes** |
| 11 | replicate | as 9 | **yes** |

**The acknowledgement boundary sits between step 9 and step 10, and was observed there.** The
checkpoint becomes recoverable one step before the user is told it is. A kill in that one-step
window loses nothing and promises nothing, which is the only ordering that makes the promise
honest.

The residue class changes at exactly two places — step 4 and step 9 — and nowhere else. That is
asserted as data (`SequenceStep::residue_if_killed_after`) and cross-checked against what is on
disk, so the harness and the implementation cannot drift apart quietly.

**Answering plan §13.3's crash rows directly:**

| §13.3 row | Answer |
|---|---|
| Crash before chunk flush | Steps 1–3. A discardable staged file, no chunk, no reference. Nothing durable points anywhere. |
| Crash after flush, before database commit | Steps 4–8. The chunk is in the store, nothing references it, and its arrival is recorded — a **collection candidate, never a dangling reference**. |
| Crash after commit | Steps 9–11. The checkpoint is there. Killed at step 9 it is there and unacknowledged; at 10 and 11 it is there and acknowledged. |
| Disk fills | Injected at plan steps 1–4: reported at the injected step, nothing acknowledged, nothing indexed. See [§4.3](#43-enospc-covers-plan-steps-14-only). |

---

## 4. What is not covered

Named plainly, because an unstated gap gets read as coverage.

### 4.1 Restart is not observed at the sequence layer — only residue is

This is the largest gap and the one that most changes what the word *recovery* is worth.

Every child in the sequence campaign gets a **fresh workspace**. `ChunkPromoter::discard_temporary`
is called by the child at its own startup, on an empty scratch directory, so the recovery routine
is *invoked* and never *observed doing work*. **No test in this repository restarts a durable
commit sequence in a workspace that a previous crash left dirty.**

The promotion campaign is better: every kill test opens the store on the dirty workspace, calls
`Cas::discard_scratch`, asserts how many files it removed, and asserts a second call removes zero.
So **discarding is observed at the promotion layer; resuming is observed nowhere.**

A second consequence, which is a matter of fact rather than of testing: `Cas::discard_scratch` and
`ChunkPromoter::discard_temporary` have **no production caller** anywhere in the tree — the crash
tests and two unit tests are the only invokers. That is not yet a defect, because the process that
would call them at startup has not shipped; it is an obligation with no holder, recorded here so
whoever writes that startup path knows it is theirs.

### 4.2 A recovered checkpoint is asserted non-empty, not identical

At steps 9–11 the harness asserts `operations > 0`, `heads > 0`, `references > 0`. The boundary
campaign carries a weight of one operation, so *greater than zero* and *complete* coincide there —
but the assertion is the weaker of the two, and it would keep passing at a weight where they do
not. The stronger statement is available and unused: `PrivateSaved::index_digest` fingerprints the
whole index, and `mesh-store::reconstruction` already compares such digests across processes;
nothing compares a **post-crash** digest against the digest a clean run of the same checkpoint
produces.

The interior campaign is the strongest post-crash statement currently made: at 2 000 operations it
asserts the index holds 0 or 2 000 and never a number in between. That is all-or-nothing measured
on this machine, which is what turns *"SQLite's transaction is the atom"* from a citation into an
observation.

### 4.3 `ENOSPC` covers plan steps 1–4 only

Steps 5–8 compose the transaction in memory and cannot fail for want of space, which is a real
reason and not an omission. Step 9 is exercised, but its failure is induced by replacing the
database file with a directory — a real failure from the real driver, and **not** a full disk. A
genuinely full disk at `COMMIT`, where the write-ahead log cannot be extended, is not exercised
anywhere.

### 4.4 There is no read-only-filesystem variant

This spike's contract names one under *Tests and benchmarks*. Nothing in the tree injects `EROFS`,
at any layer, in any campaign. Not started.

### 4.5 There is no nightly suite to run unattended in

All three GitHub workflows report `disabled_manually` (`docs/threat-model.md` G16). *Unattended* is
satisfied today by one thing only: these campaigns are ordinary members of
`cargo nextest run --workspace`, which `npm test` runs and every lane runs before pushing. They
carry no `#[ignore]`, so skipping them requires deleting them. The nightly suite is
`01KZC3F54T5W6C3EEN8A84AR3Q`.

### 4.6 `SIGKILL` is not power loss

Both harnesses say so in their own headers, and it stays said here. A killed process leaves the
page cache intact; a power cut does not. Everything above covers process crashes, out-of-memory
kills and forced termination. Power-loss durability rests on the platform honouring `fsync`, on
`rename` being atomic, and on SQLite's guarantees under WAL — assumptions, stated, not measured.
Reading this suite as proof of power-loss safety is reading more than it says.

### 4.7 The timing campaigns were calibrated, not seeded — **repaired**

**This was a live defect, filed as `01KZERXN1BC2FEDNNXNBKTNY7E` and repaired there. It is kept
here rather than deleted because the reproduction below is the regression case, and a campaign
record that erases a finding once it is fixed cannot be used to check that it stayed fixed.**

**What it was.** The two interior campaigns placed their kills at fractions of a span **measured
once, on the machine, immediately before the campaign ran**. The schedule was therefore correct
only while the machine's load after the sample matched its load during it, and nothing made that
true: under a whole-workspace run the load is decided by the test runner's scheduler, which starts
long disk-heavy binaries — the storage-footprint tests run 5 to 22 seconds each — at arbitrary
points relative to these two.

It was observed in ordinary use twice while the R2 spike was running: once in a whole-workspace
run, and once more in a pre-push gate run **on a branch whose only change was a single journal JSON
file** — a branch that cannot affect a Rust test at all. Eleven whole-suite runs were made in total
during that work; the other nine passed. It reproduced deterministically, first attempt, both
campaigns, by making the schedule explicit: calibrate quiet, then load.

```sh
LOAD=$(mktemp -d)
( sleep 2
  for i in $(seq 1 16); do
    ( while [ ! -f "$LOAD/stop" ]; do dd if=/dev/zero of="$LOAD/f.$i" bs=1m count=48 2>/dev/null; done ) &
  done ) &
cargo nextest run -p mesh-cas  -E 'test(randomised_kills_never_leave_a_partial_chunk)'
cargo nextest run -p mesh-store -E 'test(kills_inside_the_transaction_leave_all_of_it_or_none_of_it)'
touch "$LOAD/stop"
```

Both exited 100 on the first attempt, with two different symptoms and one cause:

- the promotion campaign fired its own straddle assertion, correctly — *"the kills all landed on the
  same side of the rename (0 present, 16 absent) despite a measured span of 258.165041ms"*;
- the sequence campaign did something worse. Its kill, aimed inside the transaction, landed instead
  during the database open, and the harness panicked with *"`operation` could not be counted after
  the crash: … no such table: operation"*. The retry loop that guarded this read was written for a
  write-ahead log still being recovered and could not tell that case from a schema that was never
  created, so the message a reader saw was one word away from the sentence this whole suite exists
  to make impossible.

**What replaced it.** Both campaigns are now scheduled by the *work the child has done*, and
neither reads a clock:

- **`crash-promotion.rs`** runs its child on a `PausingFs` — the same `DurableFs` seam
  `promotion_ordering.rs` uses to observe order — which performs the real operation up to a named
  interior point, announces, and blocks. The sixteen points are enumerated in `PausePoint::ORDER`:
  `stage` before the file exists, six partial writes through it, the last instant of `stage`, both
  ends of the `fsync`, the read-back in `verify`, a **torn** arrival record and a complete one,
  both ends of the `rename`, and the directory sync that follows it. Fourteen fall before the
  rename and two after it.
- **`crash-commit-sequence.rs`** runs its transaction through a `PausingExecutor` which feeds
  `sqlite3` a *prefix* of the batch on a live connection, waits for the engine to answer that it
  executed exactly that prefix, and then blocks with the transaction open and uncommitted. The
  twelve points are a list of statement offsets in `Prefix::ORDER`: `BEGIN IMMEDIATE` alone, nine
  further percentages of the batch, everything but `COMMIT`, and the whole batch. Eleven are
  uncommitted and one is committed. The executor is **armed after step 8** rather than matched on
  the batch text, because `Store::open`'s migrations are also a batch opening with
  `BEGIN IMMEDIATE`, and stopping in one of those is precisely the accident that produced
  `no such table: operation`.

Both campaigns keep their child count — sixteen and twelve — and neither drops a kill point. The
straddle assertions were not weakened, which the filed task put out of scope in as many words; they
became stronger. Each is now a statement about the *schedule*, which is a constant in the file
rather than a quantity a busy machine can empty, and each round additionally asserts the exact side
of the boundary its own point falls on — so a chunk visible before its rename returned fails the
round that found it and names the point. Each campaign carries a negative control that watches the
straddle check fail (`the_straddle_check_still_fails_when_the_schedule_covers_one_side`), and the
promotion campaign carries one that plants a truncated chunk and watches the invariant reject it
(`a_planted_partial_chunk_is_still_rejected`).

The reader that panicked also changed. `inspect` now asks whether the schema exists *before* it
counts anything, and reports `no database file`, `a database file with no schema — the child died
inside Store::open's migrations, so there was nothing to lose and nothing was acknowledged`, or a
count. The retry loop is still there for a recovering write-ahead log, and its exhaustion message
now says plainly that it is the harness failing to read rather than the database failing to hold
the checkpoint.

**Measured after the repair.** The load-ramp schedule above, ten consecutive times on a machine
running sixteen concurrent `dd` writers: **twenty campaign runs, twenty exit 0**. Neither campaign
now contains a `Duration` that decides where a kill lands.

**None of this ever was evidence of data loss**, and it should not be read as any. The boundary
campaigns — exhaustive over the step enumerations, no timing anywhere — passed every run
throughout. What was broken was the determinism of two campaigns, and a failure whose determinism
is broken is a real failure and not a flaky one: the unseeded source was the machine load between
calibration and campaign, it was named rather than assumed, and it is now gone rather than made
rare.

### 4.8 The gate artifacts are owned elsewhere and are still absent

`tools/gate-dashboard/conditions.json` reads B2 *"checkpoints are crash-safe"* from
`tests/crash/evidence/B2-checkpoint-crash-safety.json` and D6 *"zero acknowledged-state loss"* from
`tests/crash/evidence/D6-acknowledged-state-loss.json`. **Both files are absent, and both are owned
by `01KZC305MNVR598WD3A79SKW40`**, not by this spike, so this spike does not write them.
`docs/benchmarks.md` records D6 as *not measured*.

After this campaign the number exists: **`acknowledged_state_loss_events == 0`, over 47 killed
children and 5 injected faults, at revision `0d1f069618d241b6b83c3fbd07953f0569c6cfe6`.** It covers
the crash rows of plan §13.3 and not the other rows of the fault matrix, which is why the artifact
belongs to the task that covers all of them.

---

## 5. Why no runner lives here

The obvious deliverable — a script in this directory that runs the campaigns and prints a matrix —
would be **reachable from nothing**. `tools/program/invocation-check.mjs` scans `tools/` and
`benchmarks/`; it does not scan `tests/`. So a runner placed here would not run on the merge path,
and would not even be *reported* as an orphan; it would be indistinguishable in review from a
runner that works. That is precisely the shape of defect `01KZCZBVAND8G9Q1KMM34YN0DQ` was written
to make impossible, and reintroducing it one directory over in order to satisfy the word *harness*
would be a worse outcome than saying so.

The harnesses are in the crates instead, where `cargo nextest run --workspace` finds them without
being told, and where they can reach the crate-private surfaces they must drive. Wiring anything
here onto the merge path requires editing `package.json`, which is outside this spike's allowed
paths and is held by `01KZCYQRPHJNB7E87W57JTWMPB`.

Two claims this arrangement still needs and does not have, both recorded above rather than
papered over: nothing counts the kill points in one place ([§4.1](#41-restart-is-not-observed-at-the-sequence-layer--only-residue-is)
notwithstanding, the per-crate coverage assertions are per-crate), and nothing here fails when this
document goes stale. `sequence_steps_agree_with_mesh_store.rs` holds the one seam that matters —
that the promotion crate covers exactly plan §6.3's first four steps and the sequence crate covers
eleven — so a step added on either side turns the suite red. The rest of this document is prose,
and prose rots.
