# Workspace adapter boundary contract v0

This directory is the executable evidence contract for TASK-349. It defines the
single composed transition from `mesh-workspace-adapter/0` to
`mesh-workspace-adapter/1`; it does not implement the adapter.

The files have deliberately separate roles:

- `contract.json` is the closed, machine-readable decision.
- `schema.json` rejects unknown fields and incomplete records.
- `fixtures/identity.json` fixes rename, move, replacement, case-only, missing
  evidence, and directory-descendant semantics.
- `fixtures/producers.json` names one reproducible mount producer for every
  retained boundary reason and makes folder limitations explicit.
- `fixtures/compatibility.json` fixes `/0` to `/1` migration outcomes.
- `verify.mjs` validates the schema, semantic invariants, upstream `/0`
  vocabulary, design handoff, and named mutations.

Run from the repository root:

```sh
node tests/compatibility/workspace-adapter-boundaries/v0/verify.mjs
node tests/compatibility/workspace-adapter-boundaries/v0/verify.mjs --mutations
```

The contract is intentionally fail-closed. A backend either emits all rename
binding evidence or returns `rename-binding-evidence-unavailable`; it never
guesses from names, timestamps, or a later directory snapshot. Adapter `/0`
remains frozen and readable, `/1` requires exact opt-in, and unknown contracts
are refused.
