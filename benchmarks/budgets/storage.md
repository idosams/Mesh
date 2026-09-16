# Steady-state storage budget

The plan's acceptance table (plan §12.4) carries **eighteen** rows. Seventeen of them are
latencies, throughputs, transfer sizes or counts of a *single operation*; the eighteenth is idle
daemon CPU. **None of them budgets how many bytes a Mesh workspace occupies while it sits there.**

That gap is not cosmetic. Plan §2.5 says every durable version remains reachable until an explicit
retention policy permits deletion, so on-disk size grows monotonically by construction — for every
actor, for the life of the workspace. Plan §12.4's closest rows are `Metadata bytes for subtree
move` and `1 KiB edit in 1 GiB file → <4 MiB transfer`: both budget the **wire**, and this document
exists because nothing budgeted the **disk**.

Tracked as `01KZE6CMABVAV37FPJ3ZTPP8CJ`.

This page is the missing budget. It is enforced, not asserted: `npm run verify:storage` fails when
any number here disagrees with the constant that enforces it, and `npm run verify:rust` fails when
a measurement breaches the constant. Both are on the `npm test` path.

## 1. What is measured, and on what

Every figure below comes from a test that performs real writes through the shipping crates. Nothing
here is modelled, extrapolated or scaled up from a smaller run, except where a line says
**extrapolation** in those words.

| Plan §11 field | This measurement |
|---|---|
| Repository commit | `2dfcdf72022d83dd7616c8284358df011fd10073` (clean worktree) |
| Hardware | Apple M2 Pro, 10 physical / 10 logical cores, 17,179,869,184 B RAM |
| OS and filesystem | macOS 14.5 (build 23F79), arm64; APFS (`/System/Volumes/Data`, the volume `std::env::temp_dir()` resolves onto) |
| Build profile | `cargo test` debug for the gated figures; `cargo test --release` for the reported-only figures, stated per row |
| Workload data generator | `GateCorpus` in `crates/mesh-cas/tests/storage-footprint.rs`, seed 42, committed; the record stream in `crates/mesh-store/tests/index-footprint.rs`, derived from its parameters and committed |
| Warm or cold cache state | **Not applicable, and that is a claim.** Every gated figure is a count of bytes written, not a latency; the page cache cannot change it. `records_cost_the_same_whichever_way_they_arrive` and the three-repetition rule in `repeatable` are what turn that from an assumption into a check |
| Sample count | 3 repetitions per gated CAS figure, 2 independent runs per gated index figure; every repetition must agree **exactly** |
| Raw results | §2 below, and `benchmarks/reports/storage-footprint.jsonl` |
| p50 / p95 / p99 | Identical to the reported value, by construction: the repetitions agree exactly. That they agree is the evidence; a spread would mean the instrument, not the store, was being measured |
| Failure count | 0 |
| Correctness verification | The crates' own suites run first — `cargo nextest run -p mesh-cas -p mesh-store`, 100 tests, all passing at this commit, including the promote/read byte-for-byte round trip and the reconstruction proof. No timing or byte figure below was taken from a build whose correctness suite had not passed |

Other supporting versions: `rustc 1.97.1 (8bab26f4f 2026-07-14)`, host `aarch64-apple-darwin`;
`sqlite3 3.43.2`, which the index measurement talks to as a process (see
`docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md`).

## 2. What was measured

### 2.1 The content-addressed store is not the problem

| Figure | Measured | Method |
|---|---|---|
| Chunk bytes over content bytes | **0** | 96 files, 1,808,424 content bytes; the `chunks/` tree holds 1,808,424 bytes |
| Arrival journal | **67 bytes per chunk**, exactly, at 32 chunks and at 128 | the only per-chunk term in the store |
| Arrival journal, same content promoted 8x | **67 bytes per chunk** — 2,144 bytes for 32 chunks, whether promoted once or eight times | the rate is per *chunk*, not per promotion attempt |
| Gate corpus store bytes | **1,814,856** for 1,808,424 of content — **1.003x** | 3 repetitions, byte-identical |
| Allocated blocks (APFS, 4 KiB) | 2,019,328 — **1.112x** | reported, never gated: this is the host's block size, not Mesh's overhead |

`mesh-cas` neither compresses nor frames, so a chunk file *is* its content and content entropy is
irrelevant: source code and random bytes cost the same.

### 2.2 Actor count — the shape is affine, not 1x and not Nx

Eight actors, one file in five private to each, the rest shared. Chunk counts measured:

| Actors | Chunks | Over one actor |
|---:|---:|---:|
| 1 | 32 | 1.000x |
| 2 | 39 | 1.218x |
| 4 | 53 | 1.656x |
| 8 | 81 | 2.531x |

`chunks(N) = shared + N x private`, **exactly**, with no third term — asserted as an equality, not a
bound. Actors holding *identical* content add **zero** chunks and zero bytes, checked to eight
actors.

That last sentence used to be true of `chunks/` and false of the store. `01KZE8KYZGGCD732XZHZSFT9B6`
found the arrival journal recording a `+` line for every promotion *attempt*, including the ones that
discovered the chunk was already there: eight actors promoting the same 32 files cost
8 x 32 x 67 = **17,152** journal bytes, **536 per chunk against a budget of 80** — 6.7x over, for
content that adds no chunk bytes at all. Neither footprint test could see it, and the reasons were
different: the journal test promoted only distinct content, so every promotion was a first promotion;
and `actors_holding_identical_content_add_no_chunks` — the one test whose *name* claims N actors add
nothing — asserted on `chunk_count` and `chunk_bytes` only, which are exactly the two terms that did
not grow. Both tests now measure the term that did: the journal test promotes each chunk eight times
over, and the actor test asserts the whole store footprint under the root. Measured after the fix:
**2,144 journal bytes at one actor and 2,144 at eight**, an exact equality rather than a bound,
because a promotion that makes nothing visible has nothing to record.

So the answer to "is storage 1x the content or Nx?" is: **neither, and the slope is the divergence,
not the actor count.** N actors who agree cost 1x. N actors who have each rewritten a fifth of the
tree cost `1 + (N-1)/5`. At full divergence it is Nx, and content addressing can do nothing about
that, because there is nothing shared to share.

### 2.3 History depth — every version costs its whole file

`mesh-cas` declares **no dependencies at all** — its `Cargo.toml` carries an empty `[dependencies]`
table — so it cannot call `mesh-chunking` even now that content-defined chunking ships
(`docs/adr/0021-cut-every-file-by-content-and-split-the-parameters-at-one-mebibyte.md`). Whatever a
caller hands `Cas::promote` is one whole chunk. Measured: a **one-byte** edit to a 262,144-byte file
costs **262,211 bytes** on disk — the whole file again, plus one 67-byte journal line.

**Read that as a measurement of the store, not of the system.** It is what the CAS costs when a whole
file is promoted to it; a caller that cuts the file with `mesh-chunking` first promotes only the cut
chunks and pays a different, smaller number this page has not measured. The row stays because the
store is still reachable by whole-file promotion and nothing forbids it. When a caller on the
checkpoint path is wired to `mesh-chunking`, the figure to publish is that path's, and this row
becomes the ceiling rather than the expectation.

Stated as the plan states its closest row: plan §12.4 budgets **under 4 MiB of transfer** for a
1 KiB edit in a 1 GiB file. The same edit costs, on disk, **1 GiB** — and, per plan §2.5, keeps
costing it until a retention policy exists. Transfer and disk are budgeted separately for the first
time on this page, because they are separated by three orders of magnitude and only one of them
was ever budgeted.

### 2.4 The index — cost per record, not per byte

Real SQLite, WAL checkpointed with `TRUNCATE` before each reading.

| Records | Index growth | Per record |
|---:|---:|---:|
| 1,800 | 401,408 B | **223 B** |
| 7,200 | 1,552,384 B | **215 B** |

Flat to within 3.4 % across a 4x range, so growth is linear in records. An empty index costs
**131,072 B** before it holds anything, and that floor is paid **per actor workspace**.

One file version is three indexed records (manifest, operation, chunk slice) plus its parent edge,
so a file version costs about **645 B** of index.

### 2.5 Memory and CPU per actor at rest — reported, and one of them cannot be measured

**Idle daemon CPU: still no number is offered, and the reason has changed.** When this page was
first written `crates/mesh-daemon` was a 21-line scaffold and the workspace held one binary target,
so there was nothing at rest to measure. That is no longer true — `meshd` and `meshctl` both build
and run — so the honest statement today is narrower: **nobody has measured it**, not that nobody
could. The plan §12.4 idle-CPU row has an owner in `01KZC321GF6PEYYRC88DRET2XP`, and it is now
unblocked. This page will not carry an idle-CPU figure in any case: a CPU reading is a clock reading,
and §5 says why nothing with a clock in it is gated here.

Two things about memory **are** measurable today, and both are reported rather than gated because
resident set size is not deterministic:

* `mesh_store::Index` holds every operation, manifest, peer, review, approval and context record in
  `BTreeMap`s, and `Store` holds an `Index`. Measured growth: **~1.4 KiB of resident memory per
  indexed record** (release, 2,400 → 9,600 records, `ps -o rss=`), against 215 B on disk — a
  6.7x memory-to-disk ratio. *Extrapolation, labelled as such:* W1 (20,000 files) at one version is
  60,000 records, or roughly 84 MiB resident per actor, and it does not shrink.
* `CommitPlan::stage` does `before.clone()` and then renders every immutable table **twice** — once
  from the index before and once from the index after — and diffs them. Per-checkpoint staging cost
  therefore grows with total history, which makes H checkpoints O(H²) work. Measured, release,
  50 files per checkpoint held constant: **263 µs** to stage the 1st checkpoint, **4,679 µs** to
  stage the 16th, rising monotonically with the 0 → 2,251 rows already resident.

The second of those is a defect rather than a budget, and this page does not gate it: setting a
budget around a quadratic would ratify it. It is filed as `01KZE6DT88FKWMTXHQV00QFEGE`.

## 3. Budget register

Every row's value is mirrored in the named file as a `pub const` of the same name.
`tools/program/storage-budget/check.mjs` fails if the two disagree **in either direction**, if a
constant exists here with no enforcing file, or if a constant exists in an enforcing file with no
row here.

| Budget | Value | Unit | Enforced by | Justification |
|---|---:|---|---|---|
| `BUDGET_CAS_CHUNK_BYTES_OVER_CONTENT` | 0 | bytes | `crates/mesh-cas/tests/storage-footprint.rs` | The store neither compresses nor frames, so a chunk file is its content. Asserted as equality: a nonzero value is a framing change that multiplies across every chunk in every workspace |
| `BUDGET_CAS_JOURNAL_BYTES_PER_CHUNK` | 80 | bytes | `crates/mesh-cas/tests/storage-footprint.rs` | Measured 67, per distinct chunk and not per promotion attempt — the enforcing test now promotes each chunk eight times over, so the two rates are compared rather than conflated (`01KZE8KYZGGCD732XZHZSFT9B6`). The only per-chunk term in the store, so it is what decides the cost of a tree of small files. 80 admits a digest-encoding change and admits no second line per arrival |
| `BUDGET_CAS_AMPLIFICATION_PER_MILLE` | 1010 | per mille | `crates/mesh-cas/tests/storage-footprint.rs` | Measured 1003 on the gate corpus. 2.5x the observed overhead — enough headroom for a journal-format change, nowhere near enough for a second copy of anything |
| `BUDGET_IDENTICAL_ACTOR_CHUNK_GROWTH` | 0 | chunks | `crates/mesh-cas/tests/storage-footprint.rs` | The structural claim in its smallest testable form. Nonzero means content addressing has stopped deduplicating and every per-actor figure in the programme is wrong |
| `BUDGET_WHOLE_FILE_REWRITE_OVERHEAD_BYTES` | 80 | bytes | `crates/mesh-cas/tests/storage-footprint.rs` | Records a cost rather than defending one: with no chunking, one more version is one more whole file plus its journal line. To be tightened the day content-defined chunking lands |
| `GATE_CORPUS_FILES` | 96 | files | `crates/mesh-cas/tests/storage-footprint.rs` | The denominator of every ratio above. Pinned so the ratios are reproducible |
| `GATE_CORPUS_CONTENT_BYTES` | 1808424 | bytes | `crates/mesh-cas/tests/storage-footprint.rs` | As above. The test regenerates the corpus and fails if the generator drifts from this total |
| `GATE_CORPUS_SEED` | 42 | seed | `crates/mesh-cas/tests/storage-footprint.rs` | A third party regenerates the input from this and the committed generator |
| `ACTOR_SCALING_FILES` | 32 | files | `crates/mesh-cas/tests/storage-footprint.rs` | The actor law is exact rather than statistical, so a third of the tree demonstrates it; eight actors over the whole corpus would add a thousand `fsync`-bound promotions to every merge for no extra evidence |
| `BUDGET_INDEX_BYTES_PER_RECORD` | 320 | bytes | `crates/mesh-store/tests/index-footprint.rs` | Measured 215–223 with SQLite 3.43.2. 1.4x headroom because B-tree page fill is a property of the SQLite build, and a budget set at the measured value fires on a host that packs pages differently — a false regression is the fastest way to teach a team to raise a threshold. Still far below what one more indexed column-set per record would cost |
| `BUDGET_INDEX_SCHEMA_FLOOR_BYTES` | 196608 | bytes | `crates/mesh-store/tests/index-footprint.rs` | Measured 131,072 (32 pages of 4 KiB). Budgeted separately from the per-record term because it is paid per actor workspace and carries no content, so it multiplies by the actor count |
| `BUDGET_INDEX_RECORDS_PER_FILE_VERSION` | 3 | records | `crates/mesh-store/tests/index-footprint.rs` | Manifest, operation, chunk slice. The multiplier §4 composes with; the test fails if the commit sequence starts writing a fourth |
| `BUDGET_INDEX_LINEARITY_DRIFT_PER_MILLE` | 1250 | per mille | `crates/mesh-store/tests/index-footprint.rs` | Measured 966 across a 4x range. The tolerance is not the point; linearity is. A superlinear index is unbounded history wearing a different hat, and no single-size measurement would show it |

## 4. The composed headline

The two halves are measured in two processes because
`docs/adr/0014-narrow-the-lockfile-fence-to-admit-an-audited-cryptographic-dependency.md` forbids
the dependency edge that would join them. They are composed here, arithmetically, from the budgets
above — and the checker recomputes each line and fails on a mismatch, so the arithmetic cannot rot.

```
mean_file_bytes        = GATE_CORPUS_CONTENT_BYTES / GATE_CORPUS_FILES
per_file_overhead      = BUDGET_CAS_JOURNAL_BYTES_PER_CHUNK
                       + BUDGET_INDEX_RECORDS_PER_FILE_VERSION * BUDGET_INDEX_BYTES_PER_RECORD
marginal_per_mille     = 1000 + per_file_overhead * 1000 / mean_file_bytes
total_per_mille        = marginal_per_mille
                       + BUDGET_INDEX_SCHEMA_FLOOR_BYTES * 1000 / GATE_CORPUS_CONTENT_BYTES
```

| Derived figure | Budget | Measured |
|---|---:|---:|
| `MEAN_FILE_BYTES` | 18837 | — |
| `PER_FILE_OVERHEAD_BYTES` | 1040 | 712 |
| `MARGINAL_AMPLIFICATION_PER_MILLE` | 1055 | 1037 |
| `TOTAL_AMPLIFICATION_PER_MILLE` | 1163 | 1109 |

**The headline, in one sentence: a Mesh workspace costs about 1.04x the bytes of the content it
holds, plus a fixed 128 KiB per actor — for a tree whose average file is 18.8 KiB.**

That qualifier is the whole finding, because the overhead is **per file**, not per byte:

| Mean file size | Marginal amplification (measured rate) | Marginal amplification (budget) |
|---:|---:|---:|
| 256 KiB | 1.003x | 1.004x |
| 64 KiB | 1.011x | 1.016x |
| 18.8 KiB (gate corpus) | 1.038x | 1.055x |
| 4 KiB | 1.174x | 1.254x |
| 1 KiB | 1.695x | 2.016x |

A monorepo of large assets is essentially free to index. A tree of 1 KiB source files pays **70 %**
overhead before it has stored a second version of anything. Nothing on this page is a claim about
Mesh being "efficient"; it is a claim about which workloads the current design is efficient *for*,
with the number that decides it.

## 5. What this page deliberately does not budget

Stating these is part of the budget. An unlisted term is an unnoticed term.

* **Allocated blocks.** Reported (1.112x on APFS with a 4 KiB block) and never gated: it is the
  host's block size. A gate on it would fail on tmpfs and pass on ext4 for reasons that have
  nothing to do with Mesh.
* **Anything with a clock in it.** `01KZD51YC12BP9AYVTX557RGAS` recorded that `npm test` already carries two
  wall-clock budget assertions. Bytes were chosen precisely so this budget adds no third.
* **Retention.** Plan §2.5 makes growth unbounded by construction. `01KZC2KZ29Y8Z6H9W93M40NPJ0` has
  since shipped a collector, and `crates/mesh-store/RETENTION.md` records the two things that keep
  this bullet where it is: nothing *schedules* a collection, and `RetainedRoots::conservative` frees
  only content no record mentions at all. So this page still bounds the *rate* and still cannot bound
  the total. What the policy has to say for the total to become bounded is decided in
  `01KZG94AFZP999E8F8ZF2X574E`. The number a week of agent work leaves behind is no longer open —
  it is measured in §9 below — but the *total* still is: §9 measures one week with no collection
  scheduled, and says nothing about the fifty-second. This remains the single largest open term in
  the storage story.
* **Sync amplification.** No sync engine exists (`mesh-sync-engine` is a 21-line scaffold), so what
  replication multiplies this by is unmeasured and unbudgeted. It is the term most likely to change
  the answer, and it is honestly unknown at this commit.
* **The O(H²) checkpoint cost of §2.5.** A defect, not a budget — `01KZE6DT88FKWMTXHQV00QFEGE`.
  Gating it would ratify it.
* **Compression and content-defined chunking.** Both now exist in `mesh-chunking` and neither is
  reachable from `mesh-cas`, which declares no dependencies. §6 measures what a whole-file promotion
  costs against Git and has not been re-run since they landed; `01KZC2BSFS40V3ZA6WR88KNZZ5` owns
  both.
* **Content admitted for paths a user never meant to version.** A build directory inside a workspace
  is durable content unless a predicate says otherwise, and in this repository `target/` alone is
  three orders of magnitude larger than the tracked source. That term is bounded by an exclusion
  predicate rather than by a byte budget — `01KZE5FZ3QKWEZDQE9KNWQD6AH` — and multiplies every
  figure on this page when it is absent.

## 6. What this budget is not: an efficiency claim

A budget says a number will not get worse. It says nothing about whether the number is good, and
1.04x reads like "good" to anyone who does not have a comparator. So one was **run**, on the same
host on the same day, rather than recalled: `git version 2.51.0`, same APFS volume, same content.

| Arm | Content | Git, one commit, after `git gc --aggressive` | Mesh, one version |
|---|---:|---:|---:|
| Random bytes, 96 files (the gate corpus) | 1,808,424 B | 1,851,745 B — **1.024x** | 2,007,848 B — **1.110x** |
| The repository's own 174 `.rs` files | 1,783,575 B | 596,758 B — **0.335x** | 2,038,535 B — **1.143x** |
| A one-byte edit to an 8,895-byte file | 1 B changed | **+608 B** (+11,707 B before `gc`) | **+8,962 B** |

**Mesh is 3.42x Git's on-disk footprint for a real source tree, and 14.7x Git's cost for a one-byte
edit.** Both losses are stated here, at the same prominence as the wins, because a budget page that
reported only the ratios in §4 would be true and misleading at once.

Two causes, both of them **not reached by the store**, which is a different statement from *absent*
and the difference has grown since this table was run:

* **No compression on the CAS path.** Plan §6.2 specifies Zstandard. `mesh-chunking` now implements
  it (`crates/mesh-chunking/src/compress.rs`) and `mesh-cas` applies none, because `mesh-cas`
  declares no dependencies and therefore cannot reach it. Git's 0.335x is zlib and delta packing;
  Mesh's 1.007x is the bytes as handed over.
* **No content-defined chunking on the CAS path.** Same shape: `mesh-chunking` now cuts by content
  (`docs/adr/0021-…`), and a whole-file promotion to `mesh-cas` still stores a whole new object where
  Git deltas it into 608 bytes.

Both belong to `01KZC2BSFS40V3ZA6WR88KNZZ5`. **This table has not been re-run since either landed**,
so 3.42x and 14.7x are the whole-file-promotion arm and nothing on this page says what the chunked
arm costs. Treat them as the ceiling. Git was also allowed a collector and Mesh was not, because at
the time it had none; `01KZC2KZ29Y8Z6H9W93M40NPJ0` has since shipped one, which is a third reason
this table is stale in Mesh's favour to re-run rather than to quote.

**What this table is not.** It is an ad-hoc runner, not the configured, version-pinned baseline that
`01KZC289ZKJVD0T5C0RJGJ2JJC` owns; it is one workload on one host, with no Jujutsu arm and no
replication arm. It is here so that §4 is not read as a claim of efficiency, and it is to be
replaced by a real baseline row rather than cited as one. Reproduce it with:

```bash
git init && git add -A && git commit -m one && git gc --aggressive
find .git -type f -exec stat -f "%z" {} \; | awk '{s+=$1} END {print s}'
```

The Mesh column for the second and third rows is derived, and says so: chunk bytes equal content
bytes **exactly** (§3, asserted as an equality, not a bound), the journal costs 67 bytes per chunk,
and the index costs 215 bytes per record at three records per file version. The first row's Mesh
figure is measured directly.

## 7. The gate has been shown to fire, with the exit code it fired with

A budget nobody has watched fail is a budget nobody knows is wired up. So a real storage regression
was injected into the shipping crate and the gate was run, rather than reasoned about.

**The injection.** One line in `crates/mesh-cas/src/journal.rs::append_record`, doubling the record
every arrival appends — the smallest change that costs disk per chunk and nothing per byte, which is
the shape of regression this page exists to catch:

```rust
let mut record = record.clone();
record.extend_from_slice(&record.clone());
```

**The result**, run locally at base `c9411c2739d12dc7bbf0f7c334dfb29da067b32b`:

| Command | Exit | What it said |
|---|---:|---|
| `cargo nextest run -p mesh-cas --test storage-footprint` (clean) | **0** | 9 tests run: 9 passed |
| `cargo nextest run -p mesh-cas --test storage-footprint` (injected) | **100** | 9 tests run: 7 passed, **2 failed** |

The two failures name the two breached rows and print the measured figure against the budget, which
is the half that makes a red gate actionable:

```
the arrival journal costs 134 bytes per chunk, over the budget of 80
a one-byte edit cost 262278 bytes for a 262144-byte file, more than the whole file plus the
80-byte journal line
```

**100, not 101.** `verify:rust` runs `cargo nextest`, whose failure exit is 100; a plain `cargo test`
would exit 101. The number recorded here is the one a lane actually sees, because `verify:rust` is on
the `npm test` path and `cargo test` is not. The injection was reverted in the same session and
`git status` was clean before anything was pushed.

Two rows fired for one injection, and that is worth noting rather than hiding: the per-chunk journal
term appears in the journal budget *and* inside the whole-file rewrite budget, so a single per-chunk
regression is visible twice. Neither row is redundant — a compression change would fire the second
and not the first — but a lane reading two red rows should look for one cause before assuming two.

## 8. Raising a number on this page

Do not, to make a red gate green. A breach of any row is a measurement to be understood: run the
named test, read the printed figure, and find out what changed. If the new cost is correct and
intended, the raise ships **with the measurement that justifies it in the same pull request**, and
§2's table is updated in the same commit as §3's. A budget raised without a new measurement beside
it is the failure mode this page exists to prevent.

## 9. A week of agent work, measured — the plan §12.4 storage row

Everything above this section measures a *rate*: bytes per chunk, bytes per record, bytes per
actor. None of it answers the question the owner actually asked, which is week-shaped: **after
seven days of agents editing a tree, how much bigger is the store than the tree?** That is
`01KZE5FDN0NPGJ6NQ1NBYRFVH0`, and this section is its answer.

The published row is `benchmarks/reports/storage-amplification.jsonl`. It carries every plan §11
field, it was refused until it verified, and it is reproducible from itself — the row names the
remote, the commit, the generator, the seed and the whole week's parameters.

### 9.1 What was measured, and how

| Plan §11 field | This measurement |
|---|---|
| Repository commit | in the row's `repository.commit`; the worktree was clean or the row would have been refused |
| Hardware | Apple M2 Pro, 10 physical / 10 logical cores, 17,179,869,184 B RAM |
| OS and filesystem | macOS 14.5 (23F79), aarch64; APFS (the volume `std::env::temp_dir()` resolves onto) |
| Build profile | `release`, `opt_level` 3, `lto = "thin"`, `codegen-units = 1`, rustc 1.97.1 |
| Workload data generator | `mesh-bench/corpus/W7` v1, seed 42, committed; pinned in `benchmarks/workloads/manifest.json` at all three scales, plan **and** content digest |
| Warm or cold cache state | **cold**, and it cannot be anything else: the workload refuses `--cache warm` by name, because a store that already holds the week deduplicates the replay |
| Sample count | 20 timed replays after 1 untimed warmup — the harness floor, and see §9.4 |
| Host quiet or busy | **busy, and that is a result** — load average 19–36 throughout, from other lanes' `rustc` and `clippy-driver` processes on the same box. `benchmarks/runners/README.md` requires a quiet machine and this host was not one. §9.4 says exactly what that invalidates and what it does not |
| Raw results | `samples_ns` on the row, one entry per replay |
| p50 / p95 / p99 | on the row, recomputed from `samples_ns` on read |
| Failure count | 0. The publishing policy tolerates none, and every replay must admit **byte-identical** totals or the iteration fails |
| Correctness verification | before any timing: every file's final content is read back **out of the store**, chunk by chunk in manifest order, and folded; the fold must equal the fold of the bytes that went in. A failed verification produces no timing number at all |

One replay is the whole week: generate the tree, cut every version with `mesh-chunking` under
plan §6.2's parameters (1 MiB whole-file threshold, 64 KiB / 256 KiB / 1 MiB), and promote every
chunk into a real `mesh-cas` store. The store is destroyed and rebuilt empty before every sample,
outside the timed section.

### 9.2 The number

W7 at **smoke** scale — 32 files, 3,595,727 bytes, 2 actors, 7 days, 28 edits, each rewriting one
per cent of the file it touches:

| Figure | Measured |
|---|---:|
| Distinct final content | 3,595,727 B |
| Bytes admitted to the store | 7,170,635 B |
| **Storage amplification** | **1.994×** |
| Objects in the store | 68 chunks + 1 arrival journal |
| Arrival journal | 4,556 B — 67 B per chunk exactly, as §2.1 says |

Those byte figures were produced **twice**, by two independent twenty-sample runs of the same
command forty minutes apart at two different commits and under two very different machine loads,
and they came out identical to the byte: 7,170,635 over 3,595,727 both times. That is the
evidence that this figure is a property of the code and not of the day — and it is the only
figure on this row for which such evidence exists. See §9.4 for the one that is not.

**A week of agent work costs very nearly a second copy of the tree.** Not because of framing,
compression or bookkeeping — the journal is 0.06 % of the total — but because 28 edits to files
below the 1 MiB threshold admit 28 complete new copies. Twenty-eight edits that rewrote 1 % of
their file each admitted 3,574,908 bytes, about **128 KiB per edit**, against roughly 1.3 KiB of
bytes actually changed. That is the whole-file policy doing exactly what plan §6.2 says it does,
priced.

### 9.2.1 The same measurement at seven times the scale

One reduced-scale replay was run as well — a single sample, marked `--exploratory`, and reported
here rather than published as a row, because one sample is not a distribution:

| | smoke | reduced |
|---|---:|---:|
| Files | 32 | 260 |
| Distinct final content | 3,595,727 B | 16,485,758 B |
| Edits in the week | 28 | 224 |
| Mean file size | 112.4 KiB | 62.0 KiB |
| Bytes admitted | 7,170,635 B | 26,834,538 B |
| Objects in the store | 68 chunks | 514 chunks |
| **Amplification** | **1.994×** | **1.627×** |
| One replay | ~35 s | 260 s |

The ratio went **down** as the week got bigger, and the reason is the finding worth taking away:
the reduced week has *more* edits per megabyte (13.6 against 7.8) and still costs less, because
what an edit costs is the size of the file it lands on, not the size of the edit or of the tree.
Reduced's mean file is 62 KiB against smoke's 112 KiB. This is §4's "the overhead is per file,
not per byte" again, one level up: at the version level the cost is **per edited file**, and the
number that decides a workspace's storage bill is the mean size of the files its agents touch.

A corollary, stated because it is the actionable half: the same week on a tree of 1 KiB source
files would be nearly free, and on a tree of near-threshold files would cost close to
`1 + edits/files` copies of everything.

### 9.3 Why the row is a smoke-scale row, stated rather than buried

W7 has three scales and this row is the smallest. The reason is not laziness, it is `fsync`:
every promotion in `mesh-cas` is durability-ordered — `sync_all`, which on macOS is
`F_FULLFSYNC` — so a replay costs per **chunk** and almost nothing per byte. A smoke replay
promotes 68 chunks and took between 1.2 and 62 seconds depending on what else was touching the
volume; a reduced replay promotes 514 and took 260 seconds; twenty samples of the full scale is
hours. Plan §12.2's own rule applies: **a result measured at a reduced scale is
published as a reduced-scale result**, never extrapolated from.

What that costs the reader is stated plainly: **the ratio is a property of the week's shape, not
a constant of the system.** It rises with the edit rate and with the share of edited files below
the threshold, and falls with the mean file size. A row that does not name its workload, its
scale and its chunk policy is not comparable with this one, which is why the row carries all
three.

### 9.4 What this row is weak at

* **The latency on this row measures the machine, not Mesh, and must not be compared against.**
  This is the loudest caveat on the page, so it is first. The twenty samples, in observation
  order, were 45.8, 61.9, 54.2, 38.4, 36.9, 28.6, 35.7, 18.7, then 2.5, 2.9, 1.9, 1.7, 1.4, 1.2,
  1.3, 1.3, 1.2, 1.3, 1.4, 1.5 seconds. That is not a distribution, it is a step: another lane's
  compiler finished around the eighth sample and `F_FULLFSYNC` on this volume got thirty times
  cheaper. `benchmarks/runners/README.md` lists "a quiet machine" among the conditions its
  variance band was set for, and this host — load average 19 to 36 throughout, from `rustc` and
  `clippy-driver` belonging to other lanes — did not meet it. **Do not run `mesh-bench compare`
  against this row's percentiles.** A p50 of 1.85 s and a p95 of 54.2 s from one twenty-sample
  run are what an unquiet host produces, and reading a regression out of them would be reading
  somebody else's build.
* **Twenty samples is the floor, not a good p99.** `benchmarks/runners/README.md` says p99 over
  fewer than 200 samples is a handful of samples wearing a percentile's name. Even on a quiet
  host this row's percentiles would be weak; the number this row exists for is the byte ratio,
  which is exact, identical across all twenty replays by construction, and reproduced across two
  runs under different loads.
* **One host, one filesystem.** APFS, and the `F_FULLFSYNC` cost above is an APFS cost.
* **No baseline arm.** This row measures Mesh against itself. It says nothing about Git, and §6
  above — which does, and loses — has not been re-run.
* **One week, not fifty-two.** Nothing here bounds the total, because nothing schedules a
  collection. See §5.

### 9.5 The budget, and why those two numbers

Plan §12.4 now carries `Week of agent work on disk (W7): <3× distinct content` at the POC gate
and `<1.5×` as the stretch target. Both are defended here rather than asserted:

* **3× at the gate.** Measured 1.994×. Three admits a fifty per cent regression before firing,
  which is enough headroom for a framing or journal-format change on a workload whose ratio is
  already dominated by whole-file copies — and it refuses a *second* second copy of the tree,
  which is the shape of regression that matters. A gate set at the measured value would fire on
  a week whose dice fell differently, and a threshold that fires for the dice is a threshold
  somebody raises.
* **1.5× as the stretch.** This is not a tuning target, it is a design claim: getting below 1.5×
  on this workload requires content-defined chunking **below** the 1 MiB threshold, so that a
  1 % edit costs a chunk rather than a file. `mesh-chunking`'s own measured default already sets
  the threshold at 1 KiB for exactly that reason. The stretch number is therefore the number
  that says whether that change happened, which is what a stretch target is for.

Neither number is enforced by a test on the `npm test` path, and that is deliberate: §5 says
nothing with a clock in it is gated here, and a twenty-replay run is eleven minutes of `fsync`.
The row is published, the plan carries the target, and the comparison is
`mesh-bench compare` against the published row — not a unit test.
