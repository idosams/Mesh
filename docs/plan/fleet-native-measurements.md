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
