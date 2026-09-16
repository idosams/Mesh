# benchmarks/baselines

The comparators, as configured artefacts rather than as recollections. A baseline that is
quoted is not a baseline: "Git" is not a comparator, `git version 2.51.0` on a named host on
a named day over a named corpus is. Everything in this directory exists to make the second
sentence cheap enough that nobody writes the first.

```bash
cargo build --release -p mesh-bench --bin mesh-bench     # generated corpora come from it

benchmarks/runners/baselines.sh --workload one-byte-edit --corpus source-tree
benchmarks/runners/baselines.sh --workload store-tree --corpus W1 --scale reduced
node benchmarks/baselines/verify.mjs
```

Exit codes are the contract: `0` accepted, `1` refused (unverified, unpinned, or not
publishable), `2` the invocation was wrong.

## The four baselines

| Baseline | Tool | Pin | Store that is measured | Keeps history |
|---|---|---|---|---|
| `native-fs` | the host filesystem | recorded from `platform`, not pinned | one full copy per version | yes |
| `git-worktree` | `git` | exact, `versions.json` | `.git` only — the working copy is not counted | yes |
| `jujutsu` | `jj` | exact, `versions.json` | `.jj` only | yes |
| `replication` | `rsync` | exact, `versions.json` | the mirror directory | **no** |

That last column is load-bearing. `replication` refreshes one mirror in place, so a second
store after an edit costs it roughly nothing on disk — and reading that as a win over Git or
Mesh is reading a tool that threw the history away as a tool that stored it cheaply. Every
row carries `baseline.retains_history` and `baseline.history_note` so the caveat travels with
the number instead of living in a paragraph somebody skipped.

Syncthing is the other member of the replication family. It is publishable (MPL-2.0) and
deliberately not implemented: two live daemons and a network is a different runner with a
different failure surface, and pretending `rsync` covers it would be the quoting problem
again.

### Jujutsu has never been executed here

`jj` was not on `PATH` on the host that produced the rows in `reports/`, so every Jujutsu arm
was recorded as **unsupported** — a `mesh-baseline/support/v1` record in the `-support.jsonl`
file, with no timing number anywhere. `adapters/jujutsu.mjs` is therefore an unexercised code
path. The first host with `jj` installed is the first evidence it works, and a failure there
is a defect in that adapter rather than in Jujutsu. Do not quote a Jujutsu number from this
directory; there is not one to quote.

## Why a run is refused

Everything `crates/mesh-bench` refuses, plus one more:

| Refusal | Why |
|---|---|
| dirty worktree | a stranger cannot reproduce a number taken from uncommitted code |
| fewer than 20 samples | a percentile over a handful of samples is a sample wearing a percentile's name |
| any failed iteration | a run that partly did not happen is not a measurement of the part that did |
| failed verification | correctness runs first; a workload that produced wrong bytes produces **no timing number at all** |
| **a drifted tool version** | `git 2.52` is a *different baseline*, not a regression against `git 2.51.0`. The run is refused with the observed version named |

`--exploratory` relaxes all five for local work, and marks the row `sink_policy: exploratory`
with `pin_satisfied: false` where it applies — a different sink, never a disabled check.

Cold runs are refused outright on a host where the page cache cannot actually be dropped.
`--cache cold` needs `MESH_BASELINE_DROP_CACHES` set to a command that drops it, or a writable
`/proc/sys/vm/drop_caches`. macOS without `sudo purge` has neither, so this runner says so
rather than labelling a warm number `cold`. Every number in `reports/` is **warm**.

## What is measured, and how

Correctness first, always, and in this order:

1. **Resolve the pin.** Read the tool's own version; hold it against `versions.json`.
2. **Materialise the corpus** and digest it *before any baseline touches it*.
3. **Verify.** Store the tree, read it back out of the baseline's own store, fold both with
   FNV-1a/64 over sorted `(path, size, bytes)`, compare. A mismatch ends the run.
4. **Footprint**, repeated, with compaction where the baseline has a collector.
5. **Only then**, time anything.

**The footprint method is the sum of apparent file sizes under the store** — deliberately the
same method `benchmarks/budgets/storage.md` §6 used for its ad-hoc Git column, so a row from
this runner can be held against that page rather than merely resembling it. Allocated blocks
are a host property, not a baseline property, and are not what these rows compare.

**Compaction is outside the timed section.** Git is the only baseline here with a collector,
and `git gc --aggressive` is not a store operation. Both figures are reported —
`store_bytes_compacted` and `store_bytes_uncompacted` — because a Git number after `gc` and a
Git number before it differ by an order of magnitude and quoting one as the other is the
easiest available way to be wrong.

### The three workloads

| Workload | What the timed section does | Plan reference |
|---|---|---|
| `store-tree` | stores one version of the corpus | §10.4 checkpoint latency; storage.md §6 rows 1–2 |
| `one-byte-edit` | stores a second version after flipping one byte | storage.md §6 row 3; §10.4 large binary delta at file scale |
| `directory-move` | stores a second version after renaming a directory | §10.4 directory moves |

Each mutation is chosen by a rule over the sorted file list, never by a hardcoded path, and
the row records exactly which file or directory it landed on.

### The corpora

`--corpus W1…W6` materialises the committed seeded generators through the **same
`mesh-bench` binary the Mesh runs use** — "the baseline runners and the Mesh runs consume
identical generated corpora" is the handoff condition of
[`benchmarks/workloads/README.md`](../workloads/README.md), and a second generator here would
quietly break it.

`--corpus source-tree` is every `*.rs` file tracked under `crates/` at `repository.commit`,
extracted with `git archive` so an uncommitted edit cannot leak into a corpus the row claims
is defined by a commit. It is not seeded, on purpose: storage.md §6 row 2 is a **real source
tree**, and the commit is what makes it reproducible.

## The rows

Rows go out in `mesh-bench/result/v1` — the same schema, the same version string, the same
required fields as a Mesh row, so `mesh-bench validate` accepts a baseline row and
`mesh-bench compare` can hold one against the other. Two sections are additive:

* `baseline` — id, tool, exact version, version source, pin mode, whether the pin held, the
  licence terms, whether history is retained, and the compaction command.
* `footprint` — the byte figures, their raw per-repetition samples, and whether they came out
  identical every time.

The four Rust-shaped `build.*` fields say `vendor-binary` and name the tool: this repository
does not compile Git, and borrowing the Mesh build profile into a Git row would be a
metadata lie the schema exists to prevent. The load-bearing fact is `baseline.version`, and
it is pinned.

**An unsupported arm produces a different shape in a different file.** `mesh-baseline/support/v1`
records carry the reason and no timing figure at all, into `<out>-support.jsonl`. "Cannot do
this" and "did this slowly" must never share a shape, or one will eventually be read as the
other.

## Terms: what may not be measured here

`versions.json` carries a `competitor_products` list. Every entry is a product whose terms
prohibit publishing a benchmark comparison without written consent — Dropbox, Google Drive
and Perforce Helix Core at the time of writing. No consent has been sought, so **no runner
exists for any of them and no number may be quoted for one**, including from a third-party
blog post. `verify.mjs` fails if an adapter file ever appears for a product on that list.

If consent is later obtained, the entry moves to `baselines` with the consent recorded, and
an adapter is written. It does not move because somebody found a public number.

## Where rows go, and why not in here

`baselines.sh` writes rows **outside the checkout** by default — `$MESH_BENCH_REPORTS`, else
`$XDG_STATE_HOME/mesh-bench/reports`, else `~/.local/state/mesh-bench/reports` — for exactly
the reason [`benchmarks/runners/README.md`](../runners/README.md) gives: a results file
written inside the checkout is itself dirt, and the publishing policy then refuses every run
after the first and blames the commit for the harness's own output.

`benchmarks/baselines/reports/` is tracked. Publishing there is a deliberate act with a
commit attached:

```bash
benchmarks/runners/baselines.sh --workload one-byte-edit --corpus source-tree
cat ~/.local/state/mesh-bench/reports/baseline-one-byte-edit.jsonl \
  >> benchmarks/baselines/reports/one-byte-edit.jsonl
git add benchmarks/baselines/reports && git commit -m "bench: publish baseline rows"
```

## What has been measured here

Everything below is in `reports/`, one JSON line per arm, and every figure is reproducible
from the row alone. The plan §11 record, in full, for every row in this section:

| Field | Value |
|---|---|
| Repository commit | `3ed172ab63999856f0f74612330577bc9824a1be`, clean worktree |
| Hardware | Apple M2 Pro · 10 physical / 10 logical cores · 17,179,869,184 B |
| OS and filesystem | macOS 14.5 (23F79) · aarch64 · **APFS** |
| Build profile | vendor binaries: `git 2.51.0`, `rsync 2.6.9`; `mesh-bench` built `--release` for the W5 corpus |
| Workload generator | `source-tree` = every `*.rs` tracked under `crates/` at that commit (237 files, 2,558,187 B, `fnv1a64:9451d531e8e8d254`) · `W5 smoke` = `crates/mesh-bench/src/w5.rs` v1, seed 42, `fnv1a64:cb478fbfc9ebe081` |
| Cache state | **warm**, every row. Cold was refused: this host cannot drop the page cache |
| Sample count | 60 timed iterations per arm, 5 warmup, 0 failures |
| Footprint repetitions | 5 per arm |
| Correctness | verified before timing on every arm: stored, read back out of the baseline's own store, digests compared |
| Date | 2026-08-07 |
| Host quietness | **not quiet** — 1-minute load average 19.54 on 10 cores; other lanes of this programme were building concurrently. See the repeatability verdict below |

### Footprint — the defensible half

A byte count does not care about CPU contention, and these came out deterministic: identical
across all 5 repetitions for `native-fs` and `replication`, and within **2 bytes of 816,000
(≈2 ppm)** for `git-worktree`, whose `gc --aggressive` output varies slightly run to run.

**`store-tree`, source-tree corpus, 2,558,187 B of content:**

| Baseline | Store after compaction | vs. content | Keeps history |
|---|---:|---:|---|
| `git-worktree` | **815,780 B** | 0.319x | yes |
| `native-fs` | 2,558,187 B | 1.000x | yes |
| `replication` | 2,558,187 B | 1.000x | **no** |
| `jujutsu` | *unsupported here* | — | — |

**`one-byte-edit` — a second stored version after one byte changes:**

| Baseline | Corpus | Before | After | Delta |
|---|---|---:|---:|---:|
| `git-worktree` | source-tree, 14,145 B file | 815,780 B | 816,707 B | **+927 B** |
| `native-fs` | source-tree | 2,558,187 B | 5,116,374 B | +2,558,187 B |
| `replication` | source-tree | 2,558,187 B | 2,558,187 B | +0 B *(no history)* |
| `git-worktree` | W5 smoke, 1 MiB binary | 1,078,577 B | 1,079,357 B | **+780 B** |
| `native-fs` | W5 smoke | 1,048,576 B | 2,097,152 B | +1,048,576 B |
| `replication` | W5 smoke | 1,048,576 B | 1,048,576 B | +0 B *(no history)* |

Git's uncompacted delta before `gc` is a different number and is in the rows:
+6,474 B (source-tree) and +1,049,504 B (W5). Quoting the compacted figure as the cost of
the edit *as it lands* would be wrong by 7x on one corpus and 1,345x on the other.

**`directory-move` — renaming `crates/mesh-bench/src` (9 files):**

| Baseline | Delta after compaction |
|---|---:|
| `git-worktree` | **+981 B** |
| `native-fs` | +2,558,187 B |
| `replication` | +0 B *(no history)* |

### Latency — measured, and **not** trustworthy on this host today

The rows carry 60 real samples each with p50/p95/p99 derived from them. They are published
because the raw data is real, and they are labelled here because a two-run repeatability
check on the `git-worktree` arm **breached the stated band**:

```
store-tree     p50 186.2 ms -> 168.7 ms ( 9.4%, band  5.0%)  BREACH
               p95 420.0 ms -> 192.4 ms (54.1%, band 10.0%)  BREACH
               p99 543.1 ms -> 228.6 ms (57.9%, band 15.0%)  BREACH
one-byte-edit  p50  54.0 ms ->  24.0 ms (55.5%, band  5.0%)  BREACH
               p95  75.6 ms ->  46.9 ms (37.9%, band 10.0%)  BREACH
               p99 106.1 ms -> 100.8 ms ( 4.9%, band 15.0%)  ok
```

The variance source is named and not mysterious: load average 19.54 on a 10-core machine,
because other lanes were compiling while these ran. `benchmarks/runners/README.md` states the
condition — *a quiet machine, no builds* — and it was not met. **Do not quote a latency
figure from this run.** Re-run on a quiet host; the footprint figures are unaffected because
a byte count is not a clock.

### What this does *not* establish

`benchmarks/budgets/storage.md` §6 says **"Mesh is 3.42x Git's on-disk footprint for a real
source tree, and 14.7x Git's cost for a one-byte edit"** and labels its own Git column an
ad-hoc runner to be replaced by this one. This directory now supplies the **Git half** of
that comparison, measured, pinned, verified and re-runnable. It does **not** supply the Mesh
half, and the ratio must not be recomputed from the numbers on that page:

* There is no Mesh runner in this directory. Plan §12.1 baselines are what this task owns;
  a Mesh arm needs a Mesh-side workload, which is a different task and a different allowed
  path. That half is handed back, not guessed at.
* The Mesh column of storage.md §6 is **stale as of this commit**. Content-defined chunking
  landed under `01KZC2BSFS40V3ZA6WR88KNZZ5` (closed 2026-08-07), whose own resolution records
  the one-byte-edit row moving from 8,962 B to 1,949 B. Any ratio derived from the 8,962 B
  figure is a ratio against a build that no longer exists.
* The corpus is not the same one either: storage.md §6 row 2 was 174 `.rs` files and
  1,783,575 B at commit `2dfcdf7`; this run is 237 files and 2,558,187 B at `3ed172a`. Git's
  own ratio moved with it — 0.335x there, **0.319x** here.

When a Mesh footprint row exists for *this* corpus on *this* host, the ratio is a division
and not a campaign. Until then the honest statement is the one this section makes: the Git
baseline for a real source tree at this commit is **815,780 B**, and one byte costs it
**+927 B** compacted.

### Structural-win criteria (plan §12.5), stated in both directions

| | Verdict |
|---|---|
| On-disk footprint against Git | **does not support** — this run measures only the Git side; the Mesh side is absent, and an absent arm is not a win |
| Cost of a one-byte edit | **does not support** — same reason. Git's side is now pinned at +927 B (source-tree) and +780 B (W5) |
| Directory moves | **does not support** — Git's side is +981 B; no Mesh arm exists |
| Every other criterion | **does not support** — nothing here measures actor creation, checkpoint latency, concurrent writes, remote private-state visibility or context tracking |

**Baselines that beat Mesh on this run: none, because Mesh did not run.** Baselines that beat
each other, which is what was actually measured: `git-worktree` beats both `native-fs` and
`replication` on every footprint figure by two to three orders of magnitude on the delta
workloads, and `replication`'s apparent +0 B is a tool that keeps no history rather than a
tool that stores one cheaply.

## Re-pinning a baseline

A new `git` is not a regression; it is a different comparator, and every published row
measured against the old one keeps meaning what it meant. To re-pin:

1. Add the new version to `accepted` in `versions.json` — do **not** remove the old one while
   a published row still names it. `verify.mjs` fails if a published row names a version the
   pin no longer accepts, and that check is the reason the old entry stays.
2. Re-run the affected arms and publish the new rows beside the old ones.
3. Say in the pull request which comparisons the new version changes.

Never widen a pin to make a red check green. A drift is a measurement to understand.
