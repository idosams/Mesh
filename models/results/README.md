# The archived model-checking run

**Maturity: frozen evidence for a specific model revision.** It does not establish the state of
the current product by itself; see [Project status](../../docs/project-status.md).

This directory is the **gate artifact**: what was checked, against which bytes, what happened, and
the counterexample for every configuration that was supposed to produce one. Gate A may cite it as
the formal evidence for the seven core invariants of
[`docs/consistency.md`](../../docs/consistency.md) §6, **at the strength §6.2 of that document
states and not above it.**

It is produced by, and only by:

```console
$ TLA2TOOLS_JAR=/path/to/tla2tools.jar models/check.sh --archive models/results
```

| File | What it is |
|---|---|
| `run.json` | one object: the machine, the JRE, the TLC build, the worker count, the number of configurations, the number that misbehaved, and the SHA-256 of **every** file in `models/` that produced the run |
| `campaign.jsonl` | one object per configuration: the config, its digest, the module, its digest, what was expected, what happened, the invariant or property named, the distinct-state count, the counterexample length and the declared depth |
| `counterexamples/` | the TLC trace for every configuration that failed, from the first `Error:` line to the end |

## How to read it, and what not to read into it

**`verdict` is the whole result.** `ok` means the configuration did what `check.sh`'s expectation
table declared it would — passed if it was declared to pass, and failed *on the named invariant* if
it was declared to fail. A mutation that passes is a failure of the campaign, because a guard whose
removal changes nothing was not a guard.

**`distinct_states` reproduces for a passing row and does not for a failing one.** An exhaustive
search visits what it visits; a failing run stops at the first violation, and how much of the level
the workers had explored by then differs between runs. Four passing rows have been reproduced to
the state on two machines: 421,880 · 113,328 · 214,348 · 220.

**`seconds` reproduces on nothing.** It is a property of a machine. No budget has been set against
it and it is not a benchmark.

**`trace_states` is asserted only at `"workers":1`.** TLC's workers are not level synchronized, so a
parallel run can report a violation one level deeper than the shallowest one that exists. At more
than one worker the artifact records the length it saw and `check.sh` does not assert it.

**The digests are the point.** Every row names the SHA-256 of the configuration and the module it
ran. A result whose inputs cannot be identified is a number, not evidence — and a digest that no
longer matches `models/` says the artifact is stale, which is a fact worth being able to establish
rather than assume. Recompute one with `shasum -a 256 models/mesh.tla`.

**What a clean run is not.** It is not a proof: TLC is a finite-state checker and every row is
bounded. At the archived run's source revision, `crates/mesh-approval`, `crates/mesh-sync-engine`
and `crates/mesh-conflicts` were placeholders, so this artifact was never evidence about their
implementations. Those crates have since gained substantial or partial implementations, but the
artifact's pinned digests predate that work and still prove nothing about it. The current
[`models/README.md`](../README.md), under *Assumptions and boundary*, describes that distinction;
the archived bytes and results remain unchanged.
