# Benchmarks

Two things live here: where the benchmark methodology is written down, and the
**structural-win scorecard** that plan §12.5 makes the condition of the generic
infrastructure thesis. `docs/charter.md` points here for both.

Nothing on this page is a result. **No benchmark row settles a structural-win criterion
yet**, so every verdict below is `not-measured` — that is the tree's real state, not a
rendering problem, and it is what plan §2.10 requires of a page that would otherwise be
free to imply otherwise.

One published row does exist, and it settles none of the eight. `benchmarks/reports/storage-amplification.jsonl`
carries the storage-amplification measurement of `01KZE5FDN0NPGJ6NQ1NBYRFVH0`: what a week
of agent work leaves on disk, against the plan §12.4 row it added. It is a **cost**
measurement with no baseline arm, so it supports no win in either direction — see
"What the storage row does and does not support" below.

## Methodology — where it actually lives

The methodology is not restated here, because a second copy of it is a second thing to
keep true.

| Question | Answer lives in |
|---|---|
| How a run is invoked, and why a row can be refused | [`benchmarks/runners/README.md`](../benchmarks/runners/README.md) |
| Which fields a result row must carry | the same file, and `mesh-bench fields` |
| Baselines, workloads W1–W6, metrics, release targets | plan §12.1–§12.4 |
| The structural-win requirement | plan §12.5, and the scorecard below |
| Which published row settles which gate condition | `tools/gate-dashboard/conditions.json` |

Three properties of that harness are what make a row here quotable, and each is enforced
rather than asked for: a row measured from a dirty worktree is refused; a row with fewer
than 20 samples is refused; percentiles are recomputed from `samples_ns` on read, so a
hand-edited number is refused.

## The structural-win scorecard

Plan §12.5: the generic infrastructure thesis proceeds only if Mesh demonstrates **at
least three** of these eight. Gate D — Week 11 is where that is decided, and its decision
rule is exactly this table with at least three rows reading `met`.

Each row names the artifact that settles it. The artifact is an evidence envelope
(`tools/gate-dashboard/README.md` defines the shape); the envelope cites the raw result
rows, and a row whose citation does not resolve on the branch is not evidence.

| Win | Claim | Threshold | Evidence artifact | Verdict |
|---|---|---|---|---|
| `D1` | Five-times-faster actor creation than Git worktrees | `speedup_vs_git_worktree >= 5` | `benchmarks/reports/evidence/D1-actor-creation-speedup.json` | not measured |
| `D2` | No explicit task, branch, commit, or checkpoint workflow | `workflow_terms_on_user_surfaces == 0` | `tools/program/vocab-lint/evidence/D2-no-explicit-workflow.json` | not measured |
| `D3` | No global repository lock under 100 concurrent actors | `global_serialization_points == 0` | `tests/convergence/evidence/D3-no-global-lock.json` | not measured |
| `D4` | Ten-times-lower transfer for representative binary incremental edits | `transfer_reduction_vs_baseline >= 10` | `benchmarks/reports/evidence/D4-binary-edit-transfer.json` | not measured |
| `D5` | Near-real-time remote private-state visibility | `metadata_visibility_p95_ms <= 500` | `benchmarks/reports/evidence/D5-remote-visibility.json` | not measured |
| `D6` | Zero acknowledged-state loss across the fault suite | `acknowledged_state_loss_events == 0` | `tests/crash/evidence/D6-acknowledged-state-loss.json` | not measured |
| `D7` | Exact stale-context detection unavailable in baselines | `exact_read_tracking_pct >= 99` | `crates/mesh-context-ledger/evidence/D7-stale-context-detection.json` | not measured |
| `D8` | Cryptographic inability for agents to publish canonical state | `agent_key_publications == 0` | `tests/authorization/evidence/D8-agent-publication.json` | not measured |

The `Verdict` column above is the state of this branch when the page was written and is
**not** the live answer. The live answer is generated:

```bash
node tools/gate-dashboard/generate.mjs --gate D --stdout
```

That command reads the artifacts on your checkout and renders one verdict per row —
`met`, `unmet`, `stale` or `not-measured` — with the commit each artifact describes. It
is the version to quote, because it cannot be older than the tree it was run on.

`node tools/gate-dashboard/generate.mjs --verify` fails if this table and
`tools/gate-dashboard/conditions.json` stop naming the same eight wins, so the page cannot
drift away from the checker without somebody noticing.

## What the storage row does and does not support

The one published row is `benchmarks/reports/storage-amplification.jsonl`. Stated in both
directions, because a row that only listed what it supported would be doing the framing
plan §12.5 exists to prevent:

| Win | Does the storage row support it? |
|---|---|
| `D1` actor creation vs Git worktrees | **does not support** — no actor-creation timing and no Git arm |
| `D2` no explicit workflow vocabulary | **does not support** — nothing about user surfaces |
| `D3` no global lock under 100 actors | **does not support** — a single-process replay, no concurrency |
| `D4` ten-times-lower binary edit transfer | **does not support** — it measures bytes admitted to disk, not bytes on a wire, and the two are separated by three orders of magnitude on this workload |
| `D5` remote private-state visibility | **does not support** — no replication exists to measure |
| `D6` zero acknowledged-state loss | **does not support** — no faults are injected; W6 is the workload for that |
| `D7` exact stale-context detection | **does not support** — no context ledger is touched |
| `D8` agents cannot publish canonical state | **does not support** — no authorization path is exercised |

It supports **none of the eight**, and it was not built to. It settles a plan §12.4
release target, which is a different question from a structural win, and the two are kept
apart here on purpose.

**Baselines that beat Mesh on this run: not established.** The row has no baseline arm at
all — no Git, no Jujutsu, no folder replication — so this run names no winner and no loser.
The nearest thing to a comparator is the ad-hoc Git measurement in `benchmarks/budgets/storage.md` §6,
which Mesh loses by 3.42× on a source tree and 14.7× on a one-byte edit and which that page
says plainly is stale in Mesh's favour and to be re-run rather than quoted. Configured,
version-pinned baselines are `01KZC289ZKJVD0T5C0RJGJ2JJC`.

## Losses are published here too

Plan §12.5 asks for three wins out of eight; it does not ask for the other five to be
quiet. Gate D's own contract requires every loss against a baseline to be recorded at the
same prominence as every win, so when rows exist this page carries them in the same table
with the same wording. A scorecard that only lists what went well is a marketing page, and
a gate review cannot be run from one.
