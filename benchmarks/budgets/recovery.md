# Recovery-time budget

Plan §6.3 budgets **recovery after daemon crash: under five seconds**. This page records what that
number is, what enforces it, and — the part that makes this page necessary — **what `npm test` no
longer enforces, and which command does instead**.

Tracked as `01KZGHMQ7NED1AH1T9X6S63N4K`.

## 1. The change, stated plainly

`crates/mesh-store/tests/recovery.rs` used to compare an `Instant::now()` elapsed span against the
five-second constant, on the `npm test` path. It does not any more.

| Assertion | Where it runs now | On the merge path? |
|---|---|---|
| Recovery finishes inside five seconds | `recovery_over_a_populated_workspace_is_inside_the_five_second_budget`, `#[ignore]`d | **No.** `npm test` skips it |
| Recovery stays inside its **cost model** — driver processes, statements per record, SQL bytes per record | `recovery_over_a_populated_workspace_stays_inside_its_cost_model` | **Yes** |

The wall-clock assertion was not deleted, and its constant was not raised. It was moved off the
merge path, because its verdict was a function of what else the machine was doing:

> seven consecutive whole-workspace runs under a sixteen-writer disk load ramp, **three of them
> exit 100**, and every one of the three failed at this assertion and nowhere else — on a tree
> that passes on an idle machine.

That measurement is `01KZGHMQ7NED1AH1T9X6S63N4K`'s, taken before this repair. A merge gate a busy
laptop can turn red teaches every lane that a red `npm test` may mean nothing, and under the
delegation in `01KZCJ4XNNTHA0BB457P4SSJ5R` the merge path rests on a verification run believing its
own instruments.

## 2. The command that still measures the five seconds

```sh
PATH="$HOME/.cargo/bin:$PATH" cargo nextest run -p mesh-store --test recovery \
  --run-ignored all -E 'test(recovery_over_a_populated_workspace_is_inside_the_five_second_budget)'
```

Run it on a quiet machine. It prints the measured span next to the budget, so a number drifting
towards the ceiling is visible before it crosses it.

## 3. What was measured, and on what

| Plan §11 field | This measurement |
|---|---|
| Repository commit | `41c41697c313b9cd907d1d2c1aec7ec61ebc2417` plus this change, clean worktree |
| Hardware | Apple M2 Pro, `aarch64-apple-darwin` |
| OS | macOS 23.5.0 |
| Build profile | `cargo nextest` test profile (debug), through the process-per-batch `sqlite3` driver the tests supply |
| Driver | system `sqlite3` as a subprocess — `docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md` |
| Workload | 4,000 operations plus one record in each of the six other tables — 4,006 records, 12,008 rows, an 880,750-byte record journal |
| Sample count | 3 runs of the timed assertion, 3 of the counted one |
| Failure count | 0 |

| Figure | Measured | Budget | Enforced by |
|---|---:|---:|---|
| Recovery span | **253–259 ms** | 5,000 ms | the `#[ignore]`d test in §2 |
| Driver processes | **1** | 50 | `npm test` |
| Statements | **12,019** — 3.0002 per record | 3 per record + 256 | `npm test` |
| SQL bytes | **3,670,440** — 916 per record | 2,048 per record | `npm test` |

The span is an **upper bound on this hardware with this driver**, never "recovery takes 256 ms": the
driver pays a process spawn a production one would not.

## 4. Why counts are a faithful stand-in for the clock

Under the process-per-batch driver a `sqlite3` spawn costs milliseconds and dominates everything
else, so the five-second budget is spent in exactly two places: how many processes the rebuild asks
for, and how much SQL it hands each one. A rebuild pinned at a fixed process count and a fixed
number of statements per record cannot leave the budget without the hardware changing. A rebuild
that spawns per record, or composes a second statement per record, breaks the counters — and, as §5
shows by measurement rather than by argument, breaks the clock at the same time.

The counters are identical on a saturated machine and an idle one, which is the whole property the
elapsed-time comparison did not have.

What the counters do **not** catch is a change that makes SQLite itself slower per statement at
constant statement count. That is what §2 is for, and it is stated here rather than left implied.

## 5. The gate has been shown to fire, with the exit codes it fired with

A budget nobody has watched fail is a budget nobody knows is wired up. Two regressions were injected
into the shipping crate, `crates/mesh-store/src/recovery.rs`, and both were reverted before anything
was committed — `git status --porcelain` showed only the test file.

**Injection A — one extra statement per record.** The smallest change that does more work per record
and nothing per byte:

```rust
let mut injected = plan.sql();
for _ in 0..report.records_replayed { injected.push_str("SELECT 1;"); }
```

**Injection B — one `sqlite3` process per record.** The regression that actually spends the budget:

```rust
for _ in 0..report.records_replayed {
    self.executor_mut().execute_batch("SELECT 1;").map_err(RecoveryError::Rewrite)?;
}
```

| Tree | Command | Exit | What it said |
|---|---|---:|---|
| clean | `cargo nextest run -p mesh-store --test recovery -E 'test(cost_model)'` | **0** | 1 test run: 1 passed |
| injection A | the same | **100** | `the rebuild issued 16025 statements for 4006 records — 4.0002 per record, over the 3 per record plus 256 this cost model allows` |
| injection B | the same | **100** | `the rebuild spawned 4007 sqlite3 processes for 4006 records, over the 50 this cost model allows` |
| injection B | the §2 command | **100** | `recovery took 11.259205125s, over plan §6.3's budget of 5s` |

The last two rows are the point. One regression, and the counter on the merge path and the clock off
it fired together — so the merge path did not lose the property, it stopped reading it off an
instrument the machine could move.

**100, not 101.** `verify:rust` runs `cargo nextest`, whose failure exit is 100; a plain `cargo test`
would exit 101. The number recorded here is the one a lane actually sees.

## 6. Raising a number on this page

Do not, to make a red gate green. A breach is a measurement to be understood: run the named test,
read the printed figure, and find out what changed. The statement bound is deliberately set *at* the
measured rate rather than above it, so a regression adding one statement per record fails instead of
fitting in slack — which means a legitimate change that adds a table will breach it, and the raise
ships **with the measurement that justifies it in the same pull request**, exactly as
`benchmarks/budgets/storage.md` §8 requires.
