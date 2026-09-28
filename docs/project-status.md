# Project status

Canonical fleet migration F01 adds the native durable objective/lane/run control model, a separate
SQLite event ledger with transactional retries and revision checks, and isolated lane allocation
from exact saved versions. This is a library foundation: scoped agent tools, providers, attachment,
live fleet UI and the complete acceptance journeys remain pending migration and verification.
F02 adds native-issued, revocable agent sessions and MCP context, child observation and delegation.
Agents can request child lanes from their own saved versions; native code chooses and pins destinations.
F03 adds custody-bound private file/workspace capture and signed checkpoint receipts through scoped
MCP. Incomplete capture retains durable partial progress; exact retries return the recorded outcome.
Provider scheduling and complete desktop fleet journeys remain pending.
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
