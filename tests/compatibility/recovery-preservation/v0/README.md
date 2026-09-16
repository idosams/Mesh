# `mesh-recovery-preservation/0`

This directory is the executable product contract for TASK-348. It separates
the newest crash-recoverable bytes (`latest_recovery`) from the last meaningful
checkpoint (`last_meaningful`) and the `PrivateSaved` acknowledgement.

Run only the owned checks:

```sh
node tests/compatibility/recovery-preservation/v0/replay.mjs
node tests/compatibility/recovery-preservation/v0/replay.mjs --mutations
```

The normal run checks:

- all nine plan triggers crossed with all six required situations (54 rows);
- Lamport, event-ULID, content-hash ordering and exact-tuple idempotence;
- a forced file followed by `SIGKILL` and a fresh reader process for each of
  the five recovery-only triggers;
- the existing `npm-install-cold` multi-object stream; and
- the existing `vscode-atomic-replace` single-file stream.

The mutation run rejects 17 stable named changes: each of the five
recovery-only triggers promoted to meaningful, lost restart bytes, changed
tuple ordering, a seventh status, and each of the nine banned user terms.

The fixtures are data, not a runtime implementation. TASK-73 owns the store,
scheduler, and product-surface implementation described in the design handoff.
