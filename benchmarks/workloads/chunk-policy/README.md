# benchmarks/workloads/chunk-policy

The R3 chunking-policy spike: twelve policies over twenty corpus segments, and the Git baseline they
are compared against. Task `01KZC2E6N03KVPK93EESJ15Z4V`.

**The decision and all of its numbers live in
[ADR-0021](../../../docs/design-decisions.md#adr-0021),
with the full plan §11 record.** This page is how you re-run it.

```bash
# 240 measurement rows + a 10-row push-size sweep, JSON Lines on stdout
benchmarks/workloads/chunk-policy/run.sh --samples 11 --w1 reduced --w4 smoke --w5 reduced \
  > benchmarks/workloads/chunk-policy/results.jsonl

# the tables ADR-0021 quotes
node benchmarks/workloads/chunk-policy/summarize.mjs benchmarks/workloads/chunk-policy/results.jsonl

# the Git arm, re-run on the same bytes rather than quoted
benchmarks/workloads/chunk-policy/git-baseline.sh
```

Four result files are committed, and they are the artifact:

| File | What it is |
|---|---|
| `results.jsonl` | the run ADR-0021 reports — W1 `reduced`, W4 `smoke`, W5 `reduced`, 11 samples |
| `results-w1-smoke.jsonl` | the same twenty segments with W1 and W5 at `smoke`, so the Git arm has a like-for-like Mesh arm |
| `results-w1-smoke-vs-git.jsonl` | the twelve policies aggregated over W1 `smoke`, next to the Git figure |
| `git-baseline.jsonl` | the Git arm: `git version 2.51.0`, `gc --aggressive` before both readings |

The full run takes about 25 minutes on an M2 Pro; `--w1 smoke --w5 smoke` runs the same twenty
segments in about one.

## This is spike code and it is meant to be thrown away

A spike's output is a decision with its evidence and its limits. `harness.rs`, `summarize.mjs`,
`run.sh` and `git-baseline.sh` exist so a third party can reproduce ADR-0021 and for no other
reason. They are not on the `npm test` path, they have no tests of their own, and nothing should
grow a dependency on them. If chunking policy needs a standing regression gate, that is a
`crates/mesh-bench` benchmark with a budget behind it, not this.

## Why it is compiled with `rustc` and not `cargo`

`harness.rs` is not a workspace member. Making it one writes a `Cargo.lock` entry, which is
governance surface no lane may write (ADR-0013, ADR-0014). `run.sh` builds the *shipping*
`mesh-chunking` and `mesh-bench` rlibs with `cargo` and links the harness against them directly, so:

* the policies measured are the policies that ship — `ChunkingConfig`, `ChunkStream`, the real gear
  table and the real in-crate BLAKE3, not a reimplementation;
* the corpora are the corpora `benchmarks/workloads/manifest.json` publishes digests for — W1, W4
  and W5 straight out of `mesh_bench::corpus`, canonical seed 42;
* no manifest and no lockfile line is written to get either.

`CARGO_PROFILE_RELEASE_LTO=off` is set for the rlib build: the workspace's `lto = "thin"` leaves
rlibs holding bitcode the system linker cannot read. That is part of the build profile and ADR-0021
records it as such.

## Correctness runs before timing, and a failure means no number

Every version of every file, under every policy, is rebuilt out of a digest-keyed chunk store and
compared byte for byte before any clock starts. A `(segment, policy)` pair that fails gets
`ns_p50: null` and `sample_count: 0` — not a slower number, no number. Byte figures are also
recomputed on three independent passes and the row carries `byte_figures_repeatable`; a `false`
there means the instrument was measured, not the policy.

The published run: **25,020 file round-trips, 0 failures, all 240 rows repeatable.**

## The row schema

One JSON object per line. Fields, all present on every measurement row:

| Field | Meaning |
|---|---|
| `workload`, `scale`, `segment`, `byte_profile` | which corpus segment; `scale` is the `mesh-bench` scale as published |
| `edit` | the mutation the transfer column measures |
| `policy`, `policy_family`, `parameters` | the policy, and every parameter it is a function of |
| `files`, `content_bytes` | the segment |
| `chunk_refs`, `distinct_chunks`, `distinct_chunk_bytes` | what chunking produced |
| `store_total_bytes`, `store_amplification_per_mille` | distinct chunk bytes + 67 B `mesh-cas` journal per distinct chunk + 48 B manifest entry per reference |
| `dedup_per_mille` | content bytes the store did not have to hold twice |
| `transfer_bytes`, `transfer_chunks`, `transfer_bytes_with_journal`, `transfer_per_mille_of_content` | what a holder of the before-state still has to fetch |
| `roundtrip_checked`, `roundtrip_failures`, `byte_figures_repeatable` | the correctness gate |
| `sample_count`, `ns_p50`, `ns_p95`, `ns_p99`, `ns_min`, `ns_max`, `mib_per_s_at_p50`, `raw_ns` | chunking CPU. **With `sample_count` 11, `ns_p95` and `ns_p99` are both the maximum reading** and are published as the order statistics they are |

Push-sweep rows carry `workload: "push-sweep"` and `push_block_bytes`, and no byte columns: they
measure `ChunkStream`'s dependence on the size of the buffer it is handed, which is a property of
the API rather than of the policy. ADR-0021 records what that turned up.

The Git baseline emits its own rows: `baseline`, `tool_version`, `arm`, `content_bytes`,
`git_bytes_v1`, `git_bytes_v2`, `git_delta_bytes`, `files`, `gc`.
