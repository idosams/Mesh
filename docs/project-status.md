# Project status

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
