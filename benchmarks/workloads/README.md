# benchmarks/workloads

The six datasets of plan §12.2 — W1 small codebase, W2 large monorepo, W3 agent swarm, W4 mixed
business workspace, W5 large binary, W6 failure workload — and **W7 agent week**, which the plan
does not name. Every baseline comparison and every structural-win claim is measured against
these, so the deliverable is not speed and it is not fidelity. It is **reproducibility**: the same seed produces the same workload, in this process,
in the next process, and on somebody else's machine. The first two are proven by tests here. The
third was published and unobserved until 2026-08-08, when all thirty-six digests — eighteen plan
and eighteen content — were reproduced on two Linux hosts, one AArch64 and one x86-64, neither of
them the arm64 macOS host they were published from. Both of those hosts are virtual machines on
that same physical Apple M2 Pro, so the property is now **observed across operating systems and
across instruction sets, and still not across two separate computers**. See
["Determinism, and how it is checked"](#determinism-and-how-it-is-checked) layer 3 for what that
does and does not settle before quoting this page.

```bash
cargo build --release -p mesh-bench --bin mesh-bench

./target/release/mesh-bench corpus list
./target/release/mesh-bench corpus describe --workload W3 --scale full
./target/release/mesh-bench corpus digest   --workload W5 --scale smoke --content
./target/release/mesh-bench corpus materialize --workload W1 --scale reduced --root /tmp/w1

node benchmarks/workloads/verify.mjs --all
```

Exit codes are the contract: `0` accepted, `1` refused (a digest mismatch, or a corpus that does
not match its own stated shape), `2` the invocation was wrong.

## Where the code is, and why it is not in this directory

The generators are `crates/mesh-bench/src/corpus/**`; the determinism oracle is
`crates/mesh-bench/tests/workload-generators.rs`. This directory holds the **contract surface**:
the published digests, the verifier that re-checks them, and this page.

That split is [ADR-0013](../../docs/adr/0013-describe-a-benchmark-corpus-before-materialising-it.md).
The short version: `Cargo.lock` is governance surface no lane may write, so a new workspace member
cannot exist, and the generators have to live inside a crate that already does. `mesh-bench` is
the right one — the baseline runners and the Mesh runs must consume corpora from the *same*
binary, or "identical corpora" is an assumption rather than a fact.

## W7, and why there is a seventh

W1–W6 are the plan's, with the plan's own magnitudes, and each generator pins the plan's figure
in a test that cites the line. **W7 is not in the plan.** It was added by
`01KZE5FDN0NPGJ6NQ1NBYRFVH0`, because none of plan §12.4's release targets budgeted a byte on
disk and none of W1–W6 produces the thing a storage-amplification number has to be measured
over: *the same files edited over and over*.

W5 comes closest and is not it — one file, four edit shapes, aimed at content-defined chunking.
W3 has many actors and many changes but its files are small and uniform. W7 is a source tree
edited for seven days by several actors, and the one thing about it that is not a free choice is
where its files sit relative to plan §6.2's **1 MiB whole-file threshold**: `near_threshold_files`
sit exactly one byte below it, which is where the whole-file policy stores a complete new copy
per edit and where the ratio is at its worst, and `large_files` sit above it, where
content-defined chunking admits only the disturbed chunks. The first edits of the week are aimed
one at each near-threshold file before the drawn schedule starts, for the same reason W6 injects
one of each fault kind first: a week that happened to miss the expensive file because the dice
said so would silently stop measuring the expensive case.

`WorkloadId::is_from_the_plan` and `PLAN_WORKLOADS` keep the two apart in code, so a list that
says "plan §12.2" cannot quietly grow a seventh member.

## The three scales

`full` is plan §12.2 as written. `reduced` is the same shape on a developer machine. `smoke` is
small enough for a test. **A result measured at a reduced scale is published as a reduced-scale
result** — the plan is explicit that a scale that could not be reached is stated rather than
extrapolated from.

| | W1 | W2 | W3 | W4 | W5 | W6 | W7 |
|---|---|---|---|---|---|---|---|
| **full** | 20,000 files · 1.5 GB | 1,000,000 files · 100 GB · 5% read | 100 actors · 10 changes · 20% overlap | 5,000 files · 20 GB | 1 GiB · 64×4 KiB rewrites | 1,024 files · 512 faults | 2,056 files · 85.7 MB · 8 actors · 7 days · 1,344 edits |
| **reduced** | 2,000 files · 150 MB | 20,000 files · 2 GB · 5% read | 20 actors · 10 changes | 1,000 files · 2 GB | 64 MiB · 16 rewrites | 256 files · 128 faults | 260 files · 16.5 MB · 4 actors · 7 days · 224 edits |
| **smoke** | 64 files · 1 MB | 500 files · 4 MB · 5% read | 4 actors · 5 changes | 80 files · 4 MB | 1 MiB · 4 rewrites | 32 files · 21 faults | 32 files · 3.6 MB · 2 actors · 7 days · 28 edits |

Every number the plan states is pinned by a test that cites the plan line —
`the_full_scale_is_the_plan_figure` in each generator's module. W6 and W7 are the exceptions and
say so: plan §12.2 states W6 as a list of failure kinds with no magnitudes and does not state W7
at all, so their counts are this generator's choice, exposed as parameters rather than buried.
W7's week is seven days at every scale, and its near-threshold files are one byte under 1 MiB at
every scale — a reduced scale that moved either would stop measuring the thing the workload
exists for, and both are asserted.

## What it costs

Measured, not estimated. Same host for every row, stated in full because a generation time is a
machine number like any other:

> Apple M2 Pro · 10 logical cores · 16 GiB · macOS 14.5 · arm64 · APFS · `--release`
> (`lto = "thin"`, `codegen-units = 1`) · rustc 1.97.1 · repository commit `018bbaf`.
> `du -sk` after materialisation; wall clock around a single `corpus materialize` run into a
> fresh empty directory. **Nothing here is fsynced**, so a materialise time is the cost of
> reaching the page cache, not of reaching the disk — on a corpus larger than memory (W2 full,
> W4 full) writeback is inside the window anyway, and on a small one it is not. Digest rows are
> CPU-bound and touch no filesystem, so cache state does not apply to them.
> Single samples, not distributions — this table exists so a contributor knows what they are
> committing to, not to be a benchmark result. Nothing published from it goes in a result row.

| Workload | Scale | Files | Disk | Materialise | Describe | Digest (content) |
|---|---|---|---|---|---|---|
| W1 | full | 20,000 | 1.44 GiB | 5.8 s | 0.02 s | 4.2 s |
| W1 | reduced | 2,000 | 147 MiB | 0.45 s | — | 0.42 s |
| W1 | smoke | 64 | 1.1 MiB | 0.02 s | — | 0.004 s |
| W2 | full | 1,000,000 | 95.3 GiB | 252.3 s | 1.05 s | 143.7 s |
| W2 | reduced | 20,000 | 1.91 GiB | 2.4 s | — | 3.1 s |
| W2 | smoke | 500 | 5.6 MiB | 0.05 s | — | 0.008 s |
| W3 | full | 2,050 | 69.6 MiB | 0.30 s | 0.005 s | 0.19 s |
| W3 | reduced | 410 | 13.8 MiB | 0.14 s | — | 0.04 s |
| W3 | smoke | 43 | 1.5 MiB | 0.03 s | — | 0.007 s |
| W4 | full | 5,000 | 18.6 GiB | 13.2 s | 0.01 s | 34.3 s |
| W4 | reduced | 1,000 | 1.86 GiB | 1.6 s | — | 3.4 s |
| W4 | smoke | 80 | 4.0 MiB | 0.08 s | — | 0.008 s |
| W5 | full | 1 | 1.00 GiB | 0.54 s | 0.001 s | 1.6 s |
| W5 | reduced | 1 | 64 MiB | 0.04 s | — | 0.09 s |
| W5 | smoke | 1 | 1.0 MiB | 0.01 s | — | 0.003 s |
| W6 | full | 1,024 | 17.2 MiB | 0.10 s | 0.003 s | 0.05 s |
| W6 | reduced | 256 | 4.3 MiB | 0.04 s | — | 0.012 s |
| W6 | smoke | 32 | 272 KiB | 0.02 s | — | 0.002 s |

Three things worth reading off that table before you run anything:

- **`describe` is free and `materialize` is not.** Checking that a build still generates the
  right hundred-gigabyte corpus takes about a second, because it never writes a byte. That is
  the whole reason the description exists.
- **W4 full is 20 GB and W2 full is 100 GB.** Neither is something to start on a laptop with a
  full disk and no plan. W2 full is four minutes and a million inodes on this host, and some
  filesystems handle the inode count much worse than the byte count suggests.
- **The disk column is `du`, not the logical size.** W2 full's 100 GB of content occupies
  95.3 GiB — 2.2 GiB more than its 93.13 GiB logical size — because a million files round up to
  block boundaries. Budget the measured figure, not the stated one.
- **The content digest costs what the corpus costs.** `--content` on W2 full is a
  two-and-a-half-minute command on this host. Do not put it in a loop.

## Determinism, and how it is checked

Three layers, each strictly stronger than the one above it. All three are in
`crates/mesh-bench/tests/workload-generators.rs`, which is the file the task's automated
validation names:

```bash
cargo nextest run -p mesh-bench --test workload-generators
```

1. **Within one process.** Build twice, compare digests. Catches a generator reading mutable
   state it should not.
2. **Across processes.** Spawn the real `mesh-bench` binary twice and compare, then compare that
   against the in-process value. Catches dependence on address-space layout, hash iteration
   order, allocator behaviour, a clock or an environment variable — the defects an in-process
   test structurally cannot see. `two_separate_processes_write_byte_identical_corpora` does the
   strongest version: two child processes write two real trees and every byte of both is
   compared.
3. **Across hosts — observed on 2026-08-08, on two of the three axes.** `manifest.json` no longer
   records one host. `measured_on` is an array: the publishing host, plus every host that has
   since reproduced all thirty-six digests, each carrying the two commands it ran and the exit
   code each returned.

   | | host 1 (published) | host 2 | host 3 |
   |---|---|---|---|
   | OS | macOS 14.5 | Debian 12, kernel 6.10.14-linuxkit | Debian 12, kernel 6.10.14-linuxkit |
   | Arch | arm64 | aarch64 | **x86_64** |
   | Filesystem | APFS | overlayfs | overlayfs |
   | C library | Darwin libSystem | glibc | glibc |
   | rustc | 1.97.1 | 1.97.1 | 1.97.1 |
   | `cargo nextest run …` | exit **0** | exit **0** | exit **0** |
   | `verify.mjs --all --content` | exit **0** | exit **0** | exit **0** |
   | plan digests matched | 18/18 | 18/18 | 18/18 |
   | content digests matched | 18/18 | 18/18 | 18/18 |

   Nothing differed. Not one of the thirty-six digests, and not one of the shape facts —
   including `text_file_ratio`, `overlap_ratio` and `accessed_ratio`, which are `f64` and which
   `verify.mjs` compares by **exact equality**. That was the narrow risk this run existed to
   settle, and IEEE-754 division agreed bit for bit between the AArch64 and x86-64 backends.

   **What this does not establish, stated plainly.** Hosts 2 and 3 are Linux virtual machines
   (Docker Desktop 28.3.2, linuxkit) running on the *same physical Apple M2 Pro* as host 1. They
   are a different operating system, kernel, C library and filesystem, and host 3 is a different
   instruction set — the binary there is ELF `e_machine` 0x3E (`EM_X86_64`), compiled by the LLVM
   x86-64 backend, so the code computing those digests is x86-64 code rather than translated
   AArch64 code. What they are not is a second computer. Anything shared below the hypervisor —
   the silicon itself — is still uncontrolled, and a reader who needs "a stranger's machine"
   should read this layer as **two of three axes closed: OS yes, architecture yes, hardware no.**

   If you are on genuinely separate hardware, this is the whole test, and it is still worth
   running:

   ```bash
   cargo nextest run -p mesh-bench --test workload-generators
   node benchmarks/workloads/verify.mjs --all --content
   ```

   A pass is worth adding to `measured_on`; a failure is worth reporting louder, and it names the
   workload, the scale and which of the two digests moved.

   The design reasons to expect a pass are unchanged and are what made the above cheap to predict:
   no clock, no entropy, no environment, no floating point on any path that reaches a digest,
   fixed-width little-endian everywhere, and no `usize` in a digest.

Nothing in the generators reads a clock, draws entropy, looks at an environment variable or
depends on floating point — the size rescale is integer arithmetic for exactly that reason.

## The manifest

`manifest.json` is one row per workload per scale: the seed, the generator version, the plan
digest, the content digest, the shape facts and the generation time it took where it was
measured. `verify.mjs` re-checks it, and is deliberately a **second implementation** of the
comparison in a second language reading the same committed file:

```bash
node benchmarks/workloads/verify.mjs --all              # plan digests + shape, ~2 s
node benchmarks/workloads/verify.mjs --all --content    # + every byte, ~3.5 min
node benchmarks/workloads/verify.mjs --workload W3 --scale smoke
```

Timings in the manifest are **provenance, never a threshold**. A slower machine is not a failed
corpus, and a verifier that failed on one would be a machine benchmark wearing a correctness
check's clothes.

`measured_on` is an **array of hosts**, not one host. Entry 0 has `"role": "published"` and is
where the digests were computed; every later entry has `"role": "reproduced"` and carries the
CPU, OS, architecture, filesystem and rustc version of a host that re-derived all thirty-six,
together with the exact commands it ran and the exit code each returned. `verify.mjs` passes the
block through untouched on `--emit`, and no test reads it — it is provenance for a reader, and
appending to it is how a new host is recorded.

### Changing a generator

Any change to path construction, size distribution, content bytes, item order or digest framing
changes the corpus, and **every number ever measured against the old corpus is invalidated by
it**. That is not a reason never to change one; it is a reason to make the change visible:

1. Bump `mesh_bench::corpus::GENERATOR_VERSION`.
2. Re-emit: `node benchmarks/workloads/verify.mjs --all --content --emit > benchmarks/workloads/manifest.json`
3. Replace `measured_on` with a single `"role": "published"` entry for your host — the digests
   are machine-independent, the timings are not, and every `"role": "reproduced"` entry recorded
   against the old generator is evidence about a corpus that no longer exists. Re-emitting keeps
   the array as it was, so this step is a deliberate edit and not a side effect.
4. Say in the pull request which published results the bump invalidates.

Step 4 is the one that matters. The other three are mechanical, and the test that fails without
them says so by name.

## What these corpora are not

**The office formats are byte profiles, not documents.** A file W4 writes with a `.docx` name is
not a valid DOCX. It is high-entropy bytes that behave the way a deflate-compressed container
behaves under chunking and delta — a one-byte edit near the front changes everything after it,
and a chunker gets no reuse. `.pdf` and image extensions get a stable header over an
incompressible body. Anything that needs to *parse* these files needs real ones, and this
generator will mislead it.

**W1's activities are declared, not executed.** `install`, `build` and `test` are emitted as
items for a runner to replay. Running a toolchain inside a data generator would make the corpus
depend on the toolchain.

**W5's edits are described, not applied.** Applying them is what a benchmark measures; a
generator that applied its own edits would be timing itself, and there would be no before-state
to measure against.

**W6 names no path from its own base tree.** The schedule is meant to be replayed against any
workload's corpus, so it addresses peers and magnitudes rather than files.

**The digests are FNV-1a/64 and are not cryptographic.** They defend against a generator
drifting, not against an adversary choosing a corpus to collide with a published digest.

## Related tasks

`01KZC26MFSS1PMD27ATRXXMNZB` built this. `01KZC289ZKJVD0T5C0RJGJ2JJC` (baseline runners) and
`01KZC38QYBTRJDEZVP2FQPSNNY` (scale tests) consume it — the handoff condition is that the
baseline runners and the Mesh runs consume identical generated corpora, which is what the
manifest exists to make checkable.

```bash
See the public GitHub issue tracker
```
