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
new captures continue; native history reads reject replaced storage and foreign cursors. Registered projects now return stopped after restart through native catalog discovery. Missing or
replaced sources/history remain visible as unavailable; explicit resume revalidates identity and
catches up edits made while Mesh was closed. Saved history remains. Exact saved-file inspection now pages through directories and
files and previews bounded saved text without consulting live file contents. Binary and oversized
files have explicit unavailable states, and paths/content stay literal in the localized UI. Exact
saved-version comparisons now report path, content and executable-mode changes with fixed before/after
previews. Up to eight pinned comparisons now retain independent pages and file selections across
projects while captures and active comparisons advance. Closing a pin during a read cannot recreate
it. Desktop pin selections now persist through native revision-checked snapshots and restore after
restart only by revalidating their exact history, including selected paths outside the restored page.
Unavailable pins retain their selectors and expose retry controls; failed saves keep local views open
without claiming durability, and conflicts require explicit reload of the saved set. Selecting a new comparison base does
not change an open comparison. Persistent detachment now joins the desktop-owned capture worker,
retains original files and history, and refuses subsequent native saves until explicit reattachment.
Reattachment verifies the original source and remains stopped until resume. Detachment survives
restart and unavailable sources; corrupt or linked records require reconciliation. An isolated
capture verifier now accepts a sealed local bundle plus exact revision, checks its seal before and
after execution, and records its executable hash. A fresh canonical bundle passed capture, restart catch-up and
Git-preservation checks, with wrong-revision and broken-seal refusals. Exact revision verification
now queries native build identity after checking the seal; incidental binary strings do not suffice.
This does not claim rendered-window acceptance. macOS capture now uses coalesced native filesystem
events to wake bounded scans, with periodic reconciliation retained for missed or unavailable events.
The desktop shows active native signals or periodic fallback. Events never authorize content or
establish authorship; source/store identities are rechecked on every capture. Linux uses periodic
reconciliation. A canonical sealed executable passed the event-enabled capture journey with an
active stream and an observed callback batch. Incremental hashing and large-project performance
remain unverified. Packaged graphical
lifecycle and review, live fleet UI, process-tree recovery
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

## Pending checkpoint shutdown correction

The current branch drains idle-checkpoint workers before daemon destruction returns, preserving
database identity checks during immediate workspace restart. A focused native regression passed
normal and unwinding worker exits, failed under the original shutdown behavior, and passed again
after restoration. All 26 checkpoint-save integration cases passed on macOS. Full validation,
hosted Linux confirmation and merged delivery remain pending; this does not resolve the separate attachment event-registration startup issue.
