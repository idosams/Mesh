# Project status

Canonical fleet migration provides durable objective/lane/run state, an independent SQLite event
ledger, and isolated lanes opened from exact saved versions. Native-issued sessions expose scoped
MCP context, child delegation, signed private checkpoints and immutable review submission. Capture
retains custody and durable partial progress; exact retries return the recorded outcome. An agent
can submit a completed checkpoint while newer work continues, without approval or publication power.
Native Codex execution now records durable launch ownership and redacted activity. A tick-driven
host discovers delegated lanes and dispatches them within durable limits; uncertain processes retain
their slots and custody. These are native/MCP integration foundations on unmerged review branches.
Existing-project registration now pins the original folder and keeps metadata outside the project.
Bounded native observation and immutable capture inputs leave source files and Git state untouched,
without claiming custody. Native attachment history now saves captured bytes as signed versions in
the external store and reopens exact old content after restart. Unchanged captures are no-ops;
policy changes and ambiguous store bindings refuse. A native background controller now reconciles
registered projects periodically, coalesces capture signals, retains saved history through incomplete
scans or signing failures, and reports redacted capture health. It needs an explicit host signer and
lifetime. The desktop executable now exposes headless capture, version listing and watch controls
for an existing harness, with ephemeral native capture identities and joined stop handling. Native
provisioning now creates a private per-project store under a host-owned external directory and hands
retained source/store authority directly to capture; partial or replaced stores refuse. Desktop
attachment controls now select or type an existing project path, show native capture health, and
request capture, stop or resume using exact session generations. The attachment and separate-copy
flows retain English/Hebrew localization. Latest and older saved-version pages now remain fixed while
new captures continue; native history reads reject replaced storage and foreign cursors. Session lists
reset on quit; saved history remains. Packaged lifecycle, attached-file previews, comparison and
review, live fleet UI, process-tree recovery
and complete acceptance journeys remain pending migration and verification.
See the [fleet plan](plan/fleet-orchestration.md) and [migration ledger](plan/fleet-migration.md).

Mesh is an early functional local alpha. The strongest supported path is one Apple-silicon Mac
running the local daemon and desktop application against a disposable or backed-up project.

## Implemented

- Durable SQLite-backed workspace records, journals, recovery, and private saved points.
- Existing-folder import that leaves the original folder unchanged.
- App-managed native folders usable by Finder, editors, terminals, and coding agents.
- Bounded native inspection, explicit private saves, safe automatic-save preference, and recovery.
- Exact review bundles, review recording, native user-presence approval, and replay refusal.
- Saved-version reconstruction, restore, private export, and approved pull-back planning.
- Agent-folder creation and a custody-bound finish sequence with scan, save, release, and rescan.
- Redacted support-bundle preview and local diagnostics.

## Partial

- The alpha supports a bounded set of filesystem changes. Ambiguous renames, deletions, links,
  special entries, and files that do not settle require explicit intervention.
- Secure Enclave approval requires a stable Developer ID signed build on supported Macs. There is
  no software fallback.
- Native folders are inspected while the desktop is running and at safety boundaries; continuous
  capture while the application is stopped is not claimed.
- Git export is explicit and guarded. Mesh is not a general Git hosting replacement.

## Planned, not shipped

- Hosted synchronization and multi-device collaboration.
- Stable public SDK and API compatibility promises.
- Automatic application updates.
- Supported Windows distribution and production Linux packaging.
- Continuous capture while the desktop is not running.

See the [user guide](user-guide.md), [architecture](architecture.md), and
[public-alpha guide](launch/public-alpha.md) for operational boundaries.

## Reproduce local evidence

From a prepared checkout, run `npm test` for documentation, licensing, storage budgets, Rust,
desktop/React, and the real daemon demonstration. The local demo should print 44 passing checks:

```bash
node examples/local-daemon-demo.mjs
```

This proves explicit local capture, restart, review, and software-key publication refusal. It does
not prove protected approval in an eligible signed build, remote collaboration, or a clean-Mac
installation. See the [phase assessment](phase-assessment.md) for the candidate's validation status
and the [user playbooks](user-playbooks.md) for the operational journey.
