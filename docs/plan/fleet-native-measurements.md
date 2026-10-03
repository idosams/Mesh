# Four-worker native measurements

This acceptance driver compares four identical saved-input tasks executed serially
and with capacity for four workers plus the coordinator. It uses the actual Codex
adapter and Mesh MCP bridge. Each child edits its own lane, creates a checkpoint and
submits a private review. The test reconstructs all four saved reviews, verifies exact
results and one attempt per lane, and checks that original content and source state
remain unchanged. It does not approve or integrate main.

Run on macOS with an authenticated installed provider and a bridge built from the
same source revision. This executes eight worker tasks and two coordinators and can
consume provider usage. Record the source revision, binary hashes and command log
alongside its output. Do not run a second build against an active verification target.

```sh
MESH_TEST_CODEX=/absolute/path/to/codex \
MESH_TEST_MCP=/absolute/path/to/mesh-mcp \
cargo test -p mesh-daemon --test fleet-agent \
  actual_four_workers_compare_serial_and_parallel_saved_reviews \
  -- --ignored --exact --nocapture
```

The existing two-worker journey remains available. Each run retains a disposable
fleet and `measurement.json`, and the four-worker test writes a comparison receipt
under the system temporary directory. The receipt reports provider version, elapsed
native execution, observed peak worker count, host tick p95, state-read p95, first
observed worker-event times and total saved-review inspection time. Percentiles use
nearest rank over recorded samples. Four-worker parallel acceptance fails if overlap
of all four workers was not observed, retaining its measurements for diagnosis.
Overlap counts native observations of acknowledged provider sessions without observed
completion, failure or closed streams; saved scheduling state alone is insufficient.
Failures never retry provider work or turn an incomplete run into a timing success.

Elapsed execution starts before fixture preparation and ends when all recorded runs
have succeeded. First-event time is measured from that same origin, not from provider
emission. Tick time includes native dispatch and observation work. Review inspection
covers sequential native reconstruction after execution. These are deliberately
separate from renderer responsiveness, event visibility lag and time to accepted main.

A single serial/parallel pair is not a statistical speed claim or a human-operated
harness comparison. Report measured values without asserting the plan's percentage
improvements. Cost, renderer lag and human coordination time remain null, not zero.
CPU/memory measurement, a matched external-harness baseline, interactive parallel
review and actual human acceptance remain required. The driver's source and CI do
not establish that paid-provider or packaged acceptance has run.

## Recorded native run: 2026-09-28

The [revision-bound measurement record](evidence/fleet-four-worker-2026-09-28.json) records
a completed real Codex comparison at `b08674d4f5ec963204984d2e09e6dacda884fd66`
([PR #120](https://github.com/idosams/Mesh/pull/120)) on macOS 26.6.1 arm64, using
`codex-cli 0.155.0-alpha.16`. The daemon, bridge and integration driver were built from
that same revision in a separate directory, reusing existing dependency artifacts after
checking the dependency source and lockfile were unchanged. This avoided altering an
already-running full verification target; it is not a fresh Cargo full-gate result.
Binary hashes and preserved raw-log hashes are included in the record.

| Measurement | Serial | Parallel |
| --- | ---: | ---: |
| Native execution | 168.531 s | 57.748 s |
| Peak acknowledged unfinished workers | 1 | 4 |
| Host tick p95 | 1.045 ms | 1.158 ms |
| State read p95 | 0.253 ms | 0.277 ms |
| All four saved reviews inspected | 41.376 ms | 41.783 ms |

Both phases verified one attempt per child lane, exact reconstructed saved results,
and unchanged original files/source state. The successful test completed in 226.40 s.
These are native observations and sequential review reconstruction timings, not GUI
latency or human review measurements. Cost, renderer lag and human coordination remain
unknown. One serial/parallel pair does not establish a repeatable percentage speed gain.

The first acceptance attempt failed at coordinator startup after 31.93 s of test time,
before serial completion. Its provider body was not retained. A separate tool-free
Codex diagnostic reproduced a required Mesh MCP handshake timeout at 30 s. Direct
bridge initialization then succeeded in 0.0076 s and a follow-up tool-free Codex start
succeeded in 8.68 s. The separately recorded second acceptance attempt passed with
the same binaries. No timeout, assertion or worker limit was changed. This points to
a startup-readiness concern but does not prove the original discarded error or resolve
cold-start reliability. The failed attempt remains part of the evidence.

The command inside the separate test bundle was the compiled `fleet-agent` integration
binary with `actual_four_workers_compare_serial_and_parallel_saved_reviews --ignored
--exact --nocapture`, and the explicit `MESH_TEST_CODEX` and matching `MESH_TEST_MCP`
paths. The Cargo command above remains the ordinary reproduction route. The matched
external-harness baseline, resource/cost measurements, packaged interactive review,
second-provider run and human acceptance requirements remain open.


## Recorded native run: 2026-10-04

The [current-revision measurement record](evidence/fleet-four-worker-2026-10-04.json)
records one successful real Codex pair on canonical main
`1c0bcd1788878bcbca8936db1c436e8c772fb841`, including merged
[PR #246](https://github.com/idosams/Mesh/pull/246), with `codex-cli 0.158.0-alpha.2.1`.
Cargo built the MCP bridge and integration driver from that exact checkout. The record
contains their hashes, the provider hash, source tree and retained log/comparison hashes.
Local evidence preserves both generated fixture histories and the complete output.

| Measurement | Serial | Parallel |
| --- | ---: | ---: |
| Native execution | 184.925 s | 64.458 s |
| Peak acknowledged unfinished workers | 1 | 4 |
| Host tick p95 | 1.030 ms | 1.084 ms |
| State read p95 | 0.279 ms | 0.262 ms |
| All four saved reviews inspected | 54.165 ms | 53.978 ms |

The test passed in 249.53 seconds on its first attempt in this run. Both phases verified
four distinct child lanes, one attempt per lane, exact reconstructed review bytes and
unchanged original content/source state. Parallel overlap was established by acknowledged
provider sessions, not scheduling state alone. The generated test projects are separate
from the user's fixed checkpoint and ordinary work.

This refreshes native execution evidence on current code; it does not establish a statistical
speed gain or a comparison with a human-operated external harness. The provider version also
changed since the September record, so the two dates are not controlled performance trials.
Renderer latency, cost, resource consumption, human coordination, interactive parallel review
and time to accepted main remain unmeasured. Signed packaged, second-provider and real
second-host acceptance remain required. Test signing custody is not eligible packaged approval.
