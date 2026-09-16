# The durability contract of `mesh-cas`

This document exists because plan §2.10 forbids the words "safe" or "lossless" without reproducible
evidence behind them. Everything below is either something a named test demonstrates, or something
this crate assumes and does not test. The two are kept apart on purpose, and the second list is the
more important one.

## The guarantee

> At every point at which the process can stop, the store either contains the chunk — whole and
> verified — or does not contain it. There is no third outcome.

"Contains" means a file exists at the chunk's content-addressed path. The guarantee says nothing
about whether anything *references* that chunk; references are `mesh-store`'s (plan §6.3 steps
5–9), and the interval between a chunk becoming visible and a transaction referencing it is
precisely what the arrival journal exists to make findable.

It is also a guarantee about a store with **one writer**, which is what plan §6.1 specifies. A
second process that removes a staging file this one is about to rename can produce the third
outcome — a chunk that was never verified, at a verified name — and assumption 4 below states that
sequence, what it costs and what is now done about it. The guarantee and the assumption list are
meant to be read together; where they would disagree, the assumption list wins.

## The sequence, and why each step is where it is

Plan §6.3 steps 1–4, as six separately interruptible steps (`PromotionStep`):

| # | Step | Operation | Why here |
|---|---|---|---|
| 1 | `Stage` | `create_new` + `write_all` into `scratch/` | Incomplete bytes never occupy an addressable name. |
| 2 | `Flush` | `fsync` the staged file | Data is durable **before** any name reveals it. |
| 3 | `Verify` | read the staged file **back from disk**, hash it | Catches a write that landed wrong at write time rather than at some later read. Hashing the caller's in-memory buffer would only prove the caller agrees with itself. |
| 4 | `RecordArrival` | `stat` the chunk's name; if absent, append `+ <digest>` to the arrival journal and `fsync` | Durably notes the chunk *before* it exists, so a crash after the rename cannot leave a chunk nothing knows about. A chunk that is already in the store became visible under an earlier promotion which recorded it, so a second record adds nothing and costs a line per promotion *attempt*. |
| 5 | `Link` | walk `chunks/`→`<aa>`→`<bb>`, creating each level that is missing and `fsync`ing each level's parent whether or not this promotion created it, then `rename` staged → `chunks/aa/bb/<digest>` | The rename is the instant the chunk becomes visible. Atomic, or the platform is unsupported. The path to it is committed *before* it, for the same reason the data is. Unconditional — see "A directory that exists is not a directory whose name is durable" below. |
| 6 | `SyncDirectory` | `fsync` the chunk's directory | Makes the new directory entry survive a power loss, not only a process death. Unconditional — see "The deduplicated path" below. |

Two reorderings are the whole bug, and both are asserted against in
`tests/promotion_ordering.rs`, because both produce a perfectly good chunk when nothing goes wrong
and so are invisible to a promote-then-read test:

* `rename` before `fsync` of the data → after a power loss the chunk is present and empty.
* `rename` before the arrival record → after a crash the chunk is present, referenced by nothing,
  and known to nothing.

### Committing the path, not one directory entry

A chunk is reachable only if **all four** entries on `chunks/` → `<aa>` → `<bb>` → `<digest>` are
durable. Step 6 commits the last of them. On a fresh store the same promotion has just created
`<aa>` and `<bb>`, and until `01KZE8JDBVPQ97MVMD9NFKVB9T` neither of the entries naming them was ever
synced by anybody — so a power loss after `promote` returned `Ok` could leave the chunk's inode
unreachable, which is the atomicity guarantee failing in exactly the case it exists for. The fanout
is two levels of 256, so a workspace with fewer than 65 536 chunks has a new leaf directory for most
promotions, and the *first* promotion into any store has one by construction.

Step 5 therefore walks the levels one at a time, creating each one that is missing and syncing each
one's parent. The recorded trace of one promotion into a fresh store, taken with `RecordingFs` and
pasted from its log rather than written out:

```text
create_dir_all  scratch
         stage  scratch/1cf0….77101.0.chunk
     sync_file  scratch/1cf0….77101.0.chunk
          read  scratch/1cf0….77101.0.chunk
        exists  chunks/1c/f0/1cf0…            <- step 4 asks whether there is anything to record
create_dir_all  logs
        append  logs/cas-arrivals.log
     sync_file  logs/cas-arrivals.log
      sync_dir  logs
        exists  chunks
        exists  chunks/1c
create_dir_all  chunks/1c
      sync_dir  chunks                        <- commits the entry naming 1c
        exists  chunks/1c/f0
create_dir_all  chunks/1c/f0
      sync_dir  chunks/1c                     <- commits the entry naming f0
        exists  chunks/1c/f0/1cf0…            <- step 5 asks for itself, over a committed path
        rename  scratch/1cf0….77101.0.chunk
      sync_dir  chunks/1c/f0                  <- step 6: commits the entry naming the chunk
```

and of a second, deduplicated promotion of the same bytes — no journal append, no rename, and the
same three directory syncs:

```text
create_dir_all  scratch
         stage  scratch/1cf0….77101.0.chunk
     sync_file  scratch/1cf0….77101.0.chunk
          read  scratch/1cf0….77101.0.chunk
        exists  chunks/1c/f0/1cf0…            <- present, so nothing arrived and nothing is recorded
        exists  chunks
        exists  chunks/1c
      sync_dir  chunks                        <- created by nobody here, committed anyway
        exists  chunks/1c/f0
      sync_dir  chunks/1c
        exists  chunks/1c/f0/1cf0…
   remove_file  scratch/1cf0….77101.0.chunk
      sync_dir  chunks/1c/f0
```

Two properties are deliberate and both are asserted:

* **Complete.** `a_promotion_into_a_fresh_store_commits_every_directory_entry_it_creates` asserts the
  *set* of directories synced under `chunks/`, not three pairwise orderings. Three pairwise orderings
  were green over the missing syncs for the life of the defect, because an assertion that A precedes
  B says nothing about a C that never happened.
* **Bounded.** `a_promotion_into_an_existing_fanout_commits_the_whole_path_it_depends_on` asserts
  that the syncs are *exactly* the three directories on this chunk's own path, once each. A repair
  that walked further — every ancestor of every chunk in the store — would be correct and would stop
  being constant-time. A promotion's cost does not grow with the store.

`chunks/` is the one level whose own entry is not committed here: it lives in the workspace root,
which this crate does not create and does not own. Committing the root is the caller's, and so is
`logs/`'s entry in it.

### A directory that exists is not a directory whose name is durable

Step 5 used to *skip* a level that already existed, on the reasoning that whoever created it got as
far as syncing its parent. That was minimality, it kept the whole repair off the steady-state path,
and it was wrong. The assumption is about a **predecessor's** progress, and this crate reaches the
state where it is false with no crash at all, because `fsync` can fail: a promotion that creates
`chunks/aa/bb` and then takes an error from the `fsync` of `chunks/aa` returns `Err` over a
directory that exists and whose entry in its parent is not durable. Every later promotion into that
1-in-65 536 fanout slot then saw a directory that existed and skipped it — so the promotions that
were in a position to repair it were exactly the promotions that walked past it, forever. The
crash variant is the same window, one syscall wide, and needs a kill rather than an `EIO`
(`01KZEBAEK05P3XB9A788TYV4QM`).

`a_fanout_directory_left_uncommitted_by_a_failed_promotion_is_committed_by_the_next_one` reproduces
it: `RecordingFs::refusing("sync_dir", chunks/<aa>)` stops the first promotion in the window, the
test asserts the directory exists and that no `sync_dir` of `chunks/<aa>` is in the recording, and
then a second promotion on a filesystem that refuses nothing must commit it. Restoring the skip
turns that test **and** the bound test red, which is what makes the choice a pin rather than a
paragraph.

What it costs is two `fsync` calls of clean directories per promotion — **14.6–15.0 µs** on the
measured host, against losing every chunk under that fanout directory. "What it costs" below has
the measurement.

### The deduplicated path

Step 6 used to return early when step 5 found the chunk already present, on the reasoning that a
no-op rename has nothing to commit. That holds only if some *earlier* promotion ran step 6, and
`01KZE8KDMMQQDV62C83AZWFVSS` recorded a reachable sequence in which none did: a promotion is killed
between `Link` and `SyncDirectory` — the state
`tests/crash-promotion.rs::killing_after_linking_leaves_a_whole_readable_chunk` produces with a real
`SIGKILL`, and which leaves the chunk visible and readable — and then *every* later promotion of the
same content takes the same early return, so the rename that made the chunk visible is never followed
by a directory sync by anybody, ever. Step 6 is now unconditional. On the deduplicated path it is one
`fsync` of a directory that is usually already clean; see "What it costs" below for what that is
worth in microseconds.

## What is demonstrated, and by what

| Claim | Evidence |
|---|---|
| A kill at each of the six steps leaves the store in the exact state plan §6.3 predicts | `tests/crash-promotion.rs`, seven tests, each spawning a real child process and killing it with `SIGKILL` via `/bin/kill -9` |
| A kill *inside* a step — mid-write, mid-sync, mid-hash — never leaves a partial chunk | `tests/crash-promotion.rs::randomised_kills_never_leave_a_partial_chunk`: sixteen children killed at delays spread across a span measured on the machine running the test, asserting the campaign landed on both sides of the rename |
| The child really was killed and did not exit on its own | every crash test asserts `ExitStatus::code().is_none()` |
| The promotion's destructor really did not run | `killing_after_staging…` asserts exactly one file is left in `scratch/`, which is the file the destructor would have removed |
| Data is synced before the rename that reveals it | `tests/promotion_ordering.rs::the_staged_bytes_are_synced_before_the_rename_that_reveals_them` |
| The arrival record is durable before the chunk is visible | `…::the_arrival_is_recorded_before_the_rename_that_reveals_the_chunk` |
| The directory is synced after the rename, not before | `…::the_chunk_directory_is_synced_after_the_rename` |
| **Every** directory entry a promotion creates on the way to the chunk is committed, not only the leaf | `…::a_promotion_into_a_fresh_store_commits_every_directory_entry_it_creates`, which asserts the *set* of syncs under `chunks/` and the recorded position of each against the operation that created the entry it commits |
| A promotion into fanout directories that already exist commits all three of them, and exactly those three | `…::a_promotion_into_an_existing_fanout_commits_the_whole_path_it_depends_on`, which asserts the sequence of `sync_dir` calls under `chunks/` equals the chain and that the promotion created nothing |
| A fanout directory left standing by a promotion whose `sync_dir` failed is committed by the next promotion into that slot | `…::a_fanout_directory_left_uncommitted_by_a_failed_promotion_is_committed_by_the_next_one`, which stops a real promotion inside the window with `RecordingFs::refusing` and asserts on both recordings |
| A promotion that finds the chunk already there still commits its directory entry, and the whole path above it | `…::a_promotion_that_finds_the_chunk_already_there_still_syncs_its_directory`, which drives a predecessor through `Link`, stops it, and re-promotes on a fresh `Cas` |
| An arrival is recorded once per chunk, not once per promotion | `tests/storage-footprint.rs::the_arrival_journal_costs_the_same_whether_content_is_promoted_once_or_eight_times`, an exact equality over eight actors promoting identical content |
| A re-promotion over a rotted chunk tells the caller nothing was written | `tests/corruption.rs::re_promoting_good_bytes_over_a_rotted_chunk_signals_that_nothing_changed` |
| Verification reads back from disk rather than trusting the buffer | `…::verification_reads_the_staged_file_back_from_disk_rather_than_trusting_the_buffer` |
| A filesystem failure at any operation leaves no partial chunk | `…::a_filesystem_failure_at_any_operation_leaves_no_partial_chunk`, injecting a refusal at each of the first ten mutating operations |
| A corrupt chunk is rejected at read time and quarantined before the call returns | `tests/corruption.rs`, seven tests including a real bit flip on disk |
| Bytes that do not hash to a claimed digest are refused before anything is written | `…::bytes_that_do_not_match_the_named_digest_are_refused_before_anything_is_written` |
| Promote-then-read returns the exact bytes, across size boundaries | `tests/roundtrip.rs`, thirteen sizes plus two hundred randomly sized chunks |
| Unreferenced chunks are found without listing a directory | `tests/unreferenced.rs::finding_candidates_never_lists_a_directory`, which counts `list_dir` calls and asserts zero |
| A torn journal tail is ignored; damage elsewhere is reported | `…::a_torn_final_record_is_ignored_and_the_rest_is_read`, `…::a_malformed_record_that_is_not_the_tail_is_reported_rather_than_skipped` |
| An arrival recorded while the journal is being rewritten is not destroyed by the rewrite | `tests/compaction-interleaving.rs`, three tests and no clock between them. `…::an_arrival_recorded_during_compaction_is_not_destroyed_by_it` forces the record in at the rewrite's `stage`, underneath the crate's own guard, the way a second process would, and asserts the *refusal* plus the journal being left byte-for-byte as it found it. `…::a_refused_compaction_leaves_the_journal_compactable` shows the refusal is not a dead end. `…::a_promotion_racing_a_compaction_is_excluded_rather_than_lost` runs a real promotion on a second thread and pins the two with a handshake each way, so the rewrite is provably still holding the lock when the promotion is provably about to want it. All three are mutation tests, and the two halves fail separately: removing the pre-rename check turns the first two red, and removing the shared guard from `append_record` turns the third red 8 runs out of 8 — 6 out of 6 green before the handshake was added, which is why it was added |
| This crate's digests are the digests `mesh-types` computes | `tests/blake3_agrees_with_mesh_types.rs`: code-identity plus the published reference vectors, read out of `mesh-types`' own table |
| A staging name that is already taken is refused rather than overwritten, and the promotion moves to the next attempt instead | `tests/staging_guard.rs::staging_refuses_to_overwrite_an_existing_file`, `…::a_taken_staging_name_makes_the_promotion_use_the_next_attempt`, `…::every_staging_name_being_taken_is_an_error_rather_than_an_overwrite`. All three deterministic, and all three are mutation tests: replacing `create_new(true)` in `StdFs::stage` with `create(true).truncate(true)` turns each of them red. Before this file existed nothing did — the crate's whole suite passed under that mutation, 41 run and 41 passed on the campaign that planted it, and the 49 tests that predate this file still pass under it today. `create_new` was load-bearing with nothing behind it |
| Eight threads of one process promoting byte-identical content at the same instant each land a whole, verifying chunk | `…::concurrent_identical_promotions_all_land_a_whole_chunk`: 40 rounds, 8 threads released from a barrier, 512 KiB each. **A race, so it is evidence only when it races.** It counts and prints how many of its 320 stagings landed on a non-zero attempt — 280 of 320 on the machine it was written on — and deliberately does *not* assert on that count, because a machine that serialised the threads would pass it without ever reaching the guard. The three deterministic rows above are the gate; this is the property they protect |
| `discard_scratch` leaves the staged file of a live promotion alone, and does not hand its name to the next writer of the same content | `tests/scratch_discard.rs::discarding_scratch_under_a_live_promotion_leaves_its_staged_file_alone` and `…::discarding_scratch_does_not_hand_a_live_promotion_s_name_to_the_next_writer`, both deterministic. Both go red when the held-name check is deleted from `discard_scratch`, and both when a promotion stops claiming its name |
| A staging name freed by something the register cannot see lets a promotion publish bytes it never verified | `…::freeing_a_staging_name_behind_the_promotion_that_holds_it_publishes_unverified_bytes`, which performs steps 2 and 3 with `std::fs` — what a second process looks like from inside this one — and asserts that the promotion reports `wrote_content()`, that `contains` is true, and that `read` returns `Corrupt` naming the intruder's digest. **This is assumption 4's cost pinned, not a defect report** |
| A staging name is given back on every one of the four ways a promotion ends | `…::every_name_a_finished_promotion_gives_back_is_discardable_again`: rename, deduplicated removal, destructor and verification failure, each followed immediately by a leftover planted at that name and a discard that must remove it. Checked one exit at a time, because a later promotion of the same content releases a name an earlier one leaked and hides three of the four mutations |
| The leftover a crashed process left behind is still discarded, whatever process identifier is in its name | `…::a_leftover_from_a_process_that_is_gone_is_still_discarded`, written with *this* process's own identifier in the leftover's name — the case a guard built on "is this pid alive" would refuse forever |

The five tests in `tests/scratch_discard.rs` were held against seven planted mutations — deleting the
held-name check, never claiming a name, treating every name as held, and replacing each of the four
releases with a bare field clear. **Seven planted, seven killed**, each by at least one test that
names the failure in its message.

Run them with `cargo nextest run -p mesh-cas` and `cargo nextest run --test crash-promotion`.

## Repairing a chunk

`Cas::promote` **never replaces bytes.** When a file already stands at the chunk's name it is matched
by *name*, never re-read, and the promotion writes nothing and reports
`PromotionOutcome::AlreadyPresent`. That is right under content addressing — a re-promotion must not
rewrite a chunk something may be reading, which `tests/roundtrip.rs::a_re_promotion_does_not_disturb_the_existing_chunk`
pins — and it is *wrong* as a repair, because bit rot is exactly the case where a file's name and its
bytes have stopped agreeing.

So the repair contract, in one place:

> To replace the bytes of a chunk the store still has a file for, call `Cas::quarantine(&digest)`
> first. A `promote` of good bytes over a chunk that is still at its name repairs nothing, and says
> so by returning `AlreadyPresent`.

`Cas::read` does the quarantine for you when it is the read that finds the damage. The case this
contract exists for is the caller that learns a chunk is bad *without* reading it — a scrubber, or a
sync engine acting on a peer's report — and would otherwise re-promote and be told `Ok`.

What this is deliberately not: `Link` does not verify the existing file. Re-hashing the target would
turn an O(1) deduplicated promotion into an O(chunk) read of up to plan §6.2's 1 MiB maximum, on the
path that replication makes the common one, for a check `Cas::read` already performs where it
matters. The signal is the outcome; the integrity check stays where it was.

**The residual weakness, stated:** a caller that ignores the outcome is back where it started. There
are no callers of this crate in the repository yet — `mesh-store` does not depend on it — so no
caller reads `AlreadyPresent` today, and the sentence above is what the first one has to be held to.

## What it costs

The repairs above add `fsync` calls, and plan §2.10 does not allow "cheap" any more than it allows
"safe". Measured on the host in `benchmarks/budgets/storage.md` §1 (Apple M2 Pro, macOS 14.5, APFS on
`/System/Volumes/Data`, `rustc 1.97.1`), 200 iterations after 5 warm-up calls, `std::fs::File::open`
+ `sync_all` on a directory:

| Directory state | Per call, 8 runs, first campaign | Per call, 8 runs, re-taken for `01KZEBAEK05P3XB9A788TYV4QM` |
|---|---|---|
| Clean — nothing created in it since the last sync | **7.5–19.5 µs**, plus one cold-start outlier of 299 µs on the first run after the probe was compiled | **7.3–7.5 µs** |
| Dirty — an entry created immediately before the call | **1.8–2.6 ms** | **2.52–2.69 ms** |

The spread is reported rather than the best figure, and the outlier is reported rather than dropped:
the conclusion these numbers support is a ratio of two orders of magnitude, and it survives taking
the worst clean reading against the best dirty one. The two campaigns are the same probe on the same
host and agree.

**A third campaign is reported because it disagrees.** Eight runs taken while the machine was
compiling this crate gave clean readings of 7.2 µs, 7.3 µs, 7.3 µs, 1.03 ms, 2.14 ms, 2.85 ms,
3.19 ms and 3.49 ms — three orders of magnitude of spread on the *same* call. What that measures is
the host, not the syscall, and it is the reason the figures above are quoted as an idle-machine cost
and not as a bound. A promotion on a loaded machine pays milliseconds for its directory syncs, and
paid them before this change too: five of the promotion's `fsync` calls predate it.

The added calls, counted off the recorded traces above rather than estimated:

| Promotion | Extra `sync_dir` calls | Which row |
|---|---:|---|
| First into a new `<aa>` (so `<aa>` and `<bb>` are both created) | +2 | dirty |
| Into an existing `<aa>`, new `<bb>` | +2 | one dirty, one clean |
| Into an existing `<bb>` — the steady state, 65 535 promotions out of 65 536 | **+2** | clean — **14.6–15.0 µs** |
| Deduplicated (step 6 no longer returns early, and step 5 now runs on this path) | +3 | clean |

The steady-state row is the one that changed, and it changed on purpose: it was **0** while step 5
skipped a directory it had not created, and the previous edition of this document argued for that
zero. What bought it was assumption 8, and assumption 8 was reachable without a crash — see "A
directory that exists is not a directory whose name is durable". Two clean directory `fsync` calls
per promotion is the price of not depending on what a dead or failing predecessor managed to do.

Reproduce the trace table by reading `RecordingFs`'s log; reproduce the timings with a probe that
opens a directory and calls `sync_all` 200 times after 5 warm-up calls, once with the directory
clean and once with an entry created immediately before each call, repeated eight times **on an
otherwise idle machine**.

## What is assumed and NOT demonstrated

Read this list before quoting the guarantee at anybody.

1. **`fsync` reaches durable storage.** No test here can prove it. A test process that is killed
   loses nothing from the page cache, so every crash test in this crate would pass identically if
   every `fsync` call were removed. What the tests prove is that the calls are *made, in the right
   order*; whether the platform honours them is the platform's promise. Demonstrating otherwise
   needs a fault-injecting filesystem or real power-cut hardware, and neither is in this repository.
2. **`rename` is atomic within a filesystem.** POSIX requires it and the platforms this runs on
   provide it. It is not tested; it is depended on, and it is the single assumption the whole
   guarantee rests on. The task's failure-and-recovery clause says a platform that does not provide
   it is unsupported until it does, and `StdFs::sync_dir` on a non-Unix target returns
   `ErrorKind::Unsupported` with a message naming this file rather than quietly succeeding.
3. **On macOS, `fsync(2)` is weaker than it looks. Measured: `sync_all` does not issue it.**
   `fsync(2)` on macOS asks the drive to write its cache but does not wait for the drive to confirm;
   `fcntl(F_FULLFSYNC)` is the call that does. `StdFs::sync_file` and `StdFs::sync_dir` both go
   through `std::fs::File::sync_all`, and on **`aarch64-apple-darwin` with `rustc 1.97.1
   (8bab26f4f 2026-07-14)`** `sync_all` issues `fcntl(fd, F_FULLFSYNC)` with an `EINTR` retry and no
   `fsync(2)` fallback. That is a reading of the linked binary, not a belief:

   ```bash
   printf 'use std::fs::File;\nfn main(){let p=std::env::args().nth(1).unwrap();\
   File::open(&p).unwrap().sync_all().unwrap();}\n' > syncprobe.rs
   rustc -O syncprobe.rs -o syncprobe
   nm -u ./syncprobe | grep -E 'fcntl|fsync|fdatasync'   # => _fcntl only
   otool -tvV ./syncprobe | grep -B1 '_fcntl'            # => mov w1, #0x33 before each bl _fcntl
   grep -n 'F_FULLFSYNC' \
     /Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/include/sys/fcntl.h  # => 51
   ```

   `nm -u` lists `_fcntl` and lists neither `_fsync` nor `_fdatasync`, so there is no `fsync(2)` path
   in the linked code at all. `otool` shows `mov w1, #0x33` immediately before `bl _fcntl` twice;
   `0x33` is 51, which the SDK header defines as `F_FULLFSYNC`, and the second occurrence is guarded
   by `cmp w8, #0x4` — `EINTR` — so it is a retry rather than a weaker fallback.

   **The claim is scoped to that toolchain and that target triple, and goes stale visibly when either
   changes.** Re-run the four commands after a toolchain bump: if a future standard library adds an
   `fsync(2)` fallback, `nm -u` gains `_fsync` and this paragraph becomes false. Nothing here weakens
   assumption 1: identifying the syscall is not the same as testing that the platform honours it, and
   no test in this repository does the second. Recorded under `01KZDSK974HCHN8RXA5T4D107S`. Linux and
   Windows are unmeasured and stay unmeasured.
4. **One writer per workspace.** `Cas::discard_scratch` cannot see a promotion running in another
   process. **What breaking that precondition costs is an unverified chunk standing at a verified
   name — not a lost staging file and a failed promotion.** A staging name is a pure function of
   digest, process and attempt, so a name that is freed is the *first* name the next promotion of
   the same content picks. Five steps, no crash and no filesystem fault
   (`01KZDSHZAXQ223DB2GAV9GEVKM`):

   1. Promotion A stages, flushes, verifies and records its arrival at `scratch/<hex>.<pid>.0.chunk`.
   2. The staging file is removed and the name is free.
   3. A second writer stages the same digest, is granted attempt zero — A's name — and is mid-write.
   4. A performs `Link`, renaming *the path it remembers*, which now holds the partial file.
   5. `Cas::contains` is true and `Cas::read` returns `CasError::Corrupt`, from a promotion that
      reported success.

   **Within one process the sequence is now unreachable.** Every live `Promotion` holds its staging
   path in a process-wide register, and `discard_scratch` skips a held name and does not count it:
   the name is not free until the file has been renamed away or removed. The register consults no
   clock and no process identifier, so a reused pid cannot defeat it, and two `Cas` handles on one
   root are covered because the register is keyed by path rather than by handle. What it does not
   cover is a **second process**, which is where this assumption still lives: plan §6.1's controlled
   single writer is what makes the precondition keepable, a multi-writer store needs a cross-process
   lock, and nothing today enforces one.

   Re-reading and re-hashing the staged file immediately before the rename was considered and
   rejected. It costs an O(chunk) read on the promotion path — up to §6.2's 1 MiB — and it *narrows*
   the window rather than closing it, because the substitution can happen between the re-hash and
   the `rename`. A name that is never free is a guarantee; a check that ran a moment ago is not.
5. **Whole chunks fit in memory.** `promote` takes a `Vec<u8>` and `read` returns one. Plan §6.2
   caps a chunk at 1 MiB, so this is sound for the intended input, and it is a real limit on any
   caller that wants to hand this crate a whole large file instead of chunking it first.
6. **The arrival journal is an index, not a source of truth.** Losing it loses no content and
   invalidates no reference. It loses the *cheap* way to find unreferenced chunks, after which only
   `sweep_all_chunks` — the O(store) backstop, named for what it is — will find them.

   **Losing all of it and losing part of it are not the same failure.** A journal that is gone or
   unparseable announces itself, and an operator who sees an empty candidate set or
   `JournalMalformed` knows to sweep. A journal that is silently *incomplete* — well-formed,
   readable, missing the `+` for a chunk that is in the store — announces nothing, and no fold over
   the file can detect it, because the evidence that would prove a record missing is the record
   that is missing. Only `compact` and `forget` can manufacture that state, since only they replace
   the file. Both hold an exclusive guard on the journal's path while every append holds the shared
   one, so an append **inside this process** cannot land between the fold and the rename. A second
   process appending is outside assumption 4 and outside this exclusion: what a rewrite does about
   it is check, immediately before the rename, that the file is still the bytes it folded, and
   refuse if it is not. That check detects and does not prevent — an append arriving between the
   check and the rename is still lost — so the guarantee is the exclusion, and the check is what
   makes a violated precondition usually loud rather than always silent.
7. **Content addressing is only as strong as the digest.** A caller that stores bytes chosen by an
   adversary to collide with an existing chunk's digest gets a promotion that no-ops and reports
   `AlreadyPresent`, since `Link` treats an existing file at the target name as the same content.
   That is BLAKE3's collision resistance doing the work, not this crate's.
8. **Nothing is assumed about a fanout directory that already exists — it is committed anyway.**
   This entry used to read *a fanout directory that exists is assumed to have had its own entry
   committed*, and that assumption is gone rather than narrowed. Step 5 syncs the parent of every
   level on the path whether or not this promotion created it, because the assumption was reachable
   without a crash — a promotion whose own `sync_dir` fails leaves the directory standing and its
   name uncommitted, and every later promotion into that slot used to skip it. What replaced the
   assumption is a measured cost, two clean directory `fsync` calls (**14.6–15.0 µs** on the host
   above), and two tests that go red if the skip comes back:
   `a_fanout_directory_left_uncommitted_by_a_failed_promotion_is_committed_by_the_next_one` and
   `a_promotion_into_an_existing_fanout_commits_the_whole_path_it_depends_on`. Closed under
   `01KZEBAEK05P3XB9A788TYV4QM`. The residue this leaves is assumption 9, one level up, and
   assumption 1 underneath all of it: making the call is not the same as the platform honouring it.
9. **The workspace root is the caller's to commit.** `chunks/`, `scratch/`, `quarantine/` and `logs/`
   are created by `Cas::open` and their entries live in the workspace root, which this crate does not
   create and never syncs. A power loss on a store whose root was never synced can lose those
   directories whole. Plan §6.2 makes the root the workspace's, and this line is the record that
   `Cas::open` does not cover it.

## Layout

```text
<workspace-root>/
  chunks/aa/bb/<64-hex>   promoted content. Only a rename ever creates an entry here.
  scratch/                staging. Discarded wholesale at startup; never read as content.
  quarantine/<hex>.<n>    bytes that failed verification, kept as evidence.
  logs/cas-arrivals.log   the arrival journal.
```

`quarantine/` is **not** in plan §6.2's tree. It is an addition, made because the task's
failure-and-recovery clause requires that corrupt content be quarantined and because the three
directories §6.2 does list are each wrong for it: `chunks/` is the content namespace a collector
sweeps, `scratch/` is deleted at every startup, and `logs/` is for logs. This paragraph is the
record that the tree was extended rather than followed.

## Crash behaviour against plan §6.3

| Plan §6.3 says | Here |
|---|---|
| before step 4: temporary data is discarded | `Cas::discard_scratch`, and `Promotion`'s destructor for the non-crash case. Tested by the four early-kill tests. A staging file a live promotion in this process still holds is *not* temporary data and is skipped — see assumption 4 and `tests/scratch_discard.rs`. |
| after step 4 but before step 9: unreferenced chunks are garbage-collected | `Cas::unreferenced_candidates` supplies the candidate set; the collector supplies the `ReferenceOracle` and does the deleting. This crate deletes no chunk, ever. |
| after step 9: the checkpoint is durable | `mesh-store`'s, not this crate's. |
| index corruption: rebuild from immutable operations and manifests | Applies to the SQLite index. The arrival journal is a *second*, smaller index with the same property: losing it costs a full sweep and no content. |

## Plan §13.3 fault-matrix rows this crate answers

| Fault | Required behaviour | Where |
|---|---|---|
| Crash before chunk flush | No durable head references chunk | Nothing is visible before `Link`, so no head can reference it. `killing_after_staging…`, `killing_after_flushing…` |
| Crash after chunk flush, before DB commit | Chunk becomes GC candidate | The arrival record precedes visibility, so it is always in the candidate set. Asserted in every crash test that finds the chunk present. |
| Corrupt content | Rejected and re-requested | `Cas::read` rejects and quarantines; re-requesting is the sync engine's. `tests/corruption.rs` |
| Disk fills | No incomplete durable state | A write that fails leaves a staged file and no chunk. `a_filesystem_failure_at_any_operation_leaves_no_partial_chunk` injects the refusal; it does not fill a real disk. |
