# benchmarks/runners

The runners every Mesh benchmark goes through. Nothing here times anything itself — the
timing, the metadata capture and the rejection rules all live in `crates/mesh-bench` (and,
for the baselines, in `benchmarks/baselines/`), and these scripts are the reproducible way
to invoke them.

Five scripts, one rule each:

| Script | What it does | Fails when |
|---|---|---|
| `verify-schema.sh` | Runs the required-field contract against this build | any required field stopped being required |
| `run.sh` | One release-profile run, appended to a results file | the row is incomplete, unverified, or not publishable |
| `repeatability.sh` | Two runs of the same commit, compared | the pair falls outside the stated variance band |
| `selftest.sh` | Runs the three above, twice, and checks the worktree afterwards | running the harness breaks the harness |
| `baselines.sh` | One **baseline** run — native FS, Git, Jujutsu, folder replication | the baseline drifted off its pinned version, failed verification, or the row is not publishable |

```bash
benchmarks/runners/verify-schema.sh
benchmarks/runners/run.sh --workload blob-scan --iterations 200
benchmarks/runners/repeatability.sh --workload blob-scan --iterations 200
benchmarks/runners/baselines.sh --workload one-byte-edit --corpus source-tree
```

One workload is **cold-only** and says so rather than quietly accepting a warm run:
`storage-amplification` refuses `--cache warm` by name, because a store that already holds
the workload deduplicates the replay, so a warm sample would measure the store refusing
work rather than absorbing it.

```bash
benchmarks/runners/run.sh --workload storage-amplification --param scale=smoke \
  --cache cold --iterations 20 --warmup 1
```

`baselines.sh` is the one that measures something this repository did not write. Its rules,
its version pins, and why a tool that is not installed is recorded as `unsupported` rather
than as a loss, are in [`benchmarks/baselines/README.md`](../baselines/README.md); the
contract check is `node benchmarks/baselines/verify.mjs`.

Exit codes are the contract: `0` accepted, `1` refused, `2` the invocation was wrong.

## Where rows go, and why not in here

`run.sh` writes rows **outside the checkout** by default — `$MESH_BENCH_REPORTS`, else
`$XDG_STATE_HOME/mesh-bench/reports`, else `~/.local/state/mesh-bench/reports` — and prints
the path it chose on stderr.

That is not tidiness. The publishing policy refuses a row measured from a dirty worktree,
and `git status --porcelain` does not care *who* made the tree dirty. A results file written
inside the checkout is itself dirt: the first run succeeds, and every run after it is refused
with `measured from a dirty worktree`, which blames the commit for the harness's own output.
An earlier version of this script defaulted to `benchmarks/reports/<workload>.jsonl` and did
exactly that — the quickstart above was single-use per clone.

`benchmarks/reports/` **is** a tracked directory (it carries a committed `README.md`), and
`*.jsonl` in it is **not** gitignored. Publishing a row there is therefore a deliberate act
with a commit attached, not a side effect of measuring:

```bash
benchmarks/runners/run.sh --workload blob-scan --iterations 200
tail -1 ~/.local/state/mesh-bench/reports/blob-scan.jsonl >> benchmarks/reports/blob-scan.jsonl
git add benchmarks/reports/blob-scan.jsonl && git commit -m "bench: publish a blob-scan row"
```

Passing `--out` into the checkout still works and still means what it says; `run.sh` warns
that the next publishable run is refused until the row is committed or removed.

`selftest.sh` is what keeps this true. It requires a pristine worktree, runs the documented
sequence, runs the first step a second time, and asserts the tree is exactly as clean at the
end as at the start. A comment claiming the harness no longer poisons itself is not a check;
that script is.

## Why a row can be refused

The result schema requires every field below. A row missing any one of them is rejected
**at write time**, by name, and never reaches the results file — see
`mesh-bench fields` for the machine-readable list.

| Group | Fields | Why it is required |
|---|---|---|
| Identity | `schema_version` · `benchmark_id` · `invocation` · `recorded_at_unix_ms` | the row has to say what it is and how it was produced |
| Commit | `repository.remote` · `repository.commit` · `repository.dirty` | a stranger clones the remote and checks out the commit |
| Hardware | `hardware.cpu_model` · `hardware.physical_cores` · `hardware.logical_cores` · `hardware.memory_bytes` | the same code is a different number on a different machine |
| OS and filesystem | `platform.os` · `platform.os_version` · `platform.arch` · `platform.filesystem` | Mesh's numbers are storage numbers; APFS and tmpfs are not the same benchmark |
| Build | `build.profile` · `build.opt_level` · `build.debug_info` · `build.rustc_version` · `build.target_triple` | a `debug` number published as `release` is off by an order of magnitude |
| Workload | `workload.generator` · `workload.generator_version` · `workload.seed` · `workload.parameters` | the data has to be regenerable byte for byte |
| Cache state | `cache_state` (`cold` \| `warm`) | "whatever the machine had cached" is the most common way a storage benchmark lies |
| Samples | `sample_count` · `iterations_attempted` · `failure_count` · `samples_ns` | the raw data, and an honest count of what failed |
| Summary | `latency.min_ns` · `latency.p50_ns` · `latency.p95_ns` · `latency.p99_ns` · `latency.max_ns` · `latency.mean_ns` | recomputed from `samples_ns` on read; a hand-edited percentile is refused |
| Correctness | `verification.method` · `verification.expected_digest` · `verification.observed_digest` · `verification.verified` | verification runs *before* timing, and a failed verification produces no timing number |

### The one optional section

A workload that leaves bytes on disk also carries `storage`. It is the **only** optional
section, and optional means the *section*: a row that has it and is missing one field
inside it is refused by that field's name, exactly like a missing percentile.

| Field | Meaning |
|---|---|
| `storage.boundary` | `store` (counted in a real content-addressed store) or `chunking` (counted at the chunker, because no store was reachable). Closed vocabulary |
| `storage.granularity` | the unit the store admits bytes in — `chunk` |
| `storage.admitted_bytes` | every byte under the store root afterwards, its bookkeeping included |
| `storage.distinct_content_bytes` | the bytes of distinct final content the workload asked it to hold |
| `storage.amplification_per_mille` | **derived**: `admitted × 1000 / distinct`, recomputed on read and refused on a mismatch, the same way a hand-edited p99 is |

Most rows omit it, and that is the honest state for a workload that measures a latency and
stores nothing: a section of zeroes would be a measurement nobody took.

Beyond the schema, the publishing policy refuses a run from a dirty worktree, a run with
fewer than 20 samples, and a run with any failed iteration. `--exploratory` relaxes all
three for local work — and marks the row as exactly that, rather than silently disabling a
check.

A refused run prints **nothing** on stdout. The row is emitted only once the sink has taken
it, so `nothing written` on stderr is true of the terminal as well as the file — a refused
run's percentiles are real numbers, and a real number on a terminal is one copy-paste away
from being quoted as a result.

## Reproducing a published run from the row alone

Every published row is self-contained. Given one line of a results file:

```bash
row=benchmarks/reports/blob-scan.jsonl        # any row you want to reproduce

git clone "$(grep -o '"remote":"[^"]*"' "$row" | head -1 | cut -d'"' -f4)" mesh-repro
cd mesh-repro
git checkout "$(grep -o '"commit":"[^"]*"' "$row" | head -1 | cut -d'"' -f4)"

# `invocation` names ./target/release/mesh-bench, so build it before running it
cargo build --release -p mesh-bench --bin mesh-bench

grep -o '"invocation":"[^"]*"' "$row" | head -1 | cut -d'"' -f4    # run this
```

Then compare your row against the published one:

```bash
cargo run --release -p mesh-bench --bin mesh-bench -- compare published.jsonl yours.jsonl
```

`compare` refuses to compare runs that differ in commit, CPU, OS, filesystem, build profile,
generator, seed or cache state — a difference there is a different benchmark, not a
regression. Only when everything matches does it apply the stated band: **5 % on p50, 10 %
on p95, 15 % on p99**.

`compare` takes the **last** row of each file, and says so on stderr — `row 4 of 4 (the
last) in published.jsonl`. Results files accumulate, and a verdict against an unnamed row of
an unknown many is not a verdict anyone can check.

## Getting a trustworthy number

The band is enforced, not aspirational; these are the conditions it was set for.

- **Release profile.** `run.sh` builds `--release`; a debug run is 5–20x slower and far noisier.
- **At least 200 iterations.** p99 over 25 samples is one sample, and it will breach.
- **A quiet machine.** No builds, no sync clients, laptop on mains.
- **The same cache state.** Compare cold with cold, warm with warm. A cold sample is
  *supposed* to be slower: `prepare(cold)` makes the data unavailable and the timed section
  pays to make it available again, the same way a storage workload's cold sample pays the
  page-cache miss. Cold numbers below warm ones mean the cache was never actually dropped.

If a pair still breaches the band, the instrument is not trustworthy yet on that host —
find the variance source before publishing anything from it. That is the failure mode this
directory exists to make loud.
