# Operation vocabulary artifacts

`mesh-operations` owns this additive operation schema/vector publication. The existing
`mesh-types` signed-record generator continues to own `protocol/schemas` and
`protocol/test-vectors` unchanged. These artifacts do not add record types to that older subset.

The [root schema](v0/workspace-root-schema.json) and [root vector](v0/workspace-root-vector.json)
are generated from the current operation owner and compared byte-for-byte in the default test gate.
The inventory test rejects missing and unknown files recursively. Regenerate intentionally with:

```sh
cargo test -p mesh-operations --test published_root -- --ignored write_published_root
```

Existing published bytes must be preserved across future compatibility changes. Readers without the
new domain refuse it; see the [operation contract](../../docs/plan/execution-plan.md#explicit-empty-workspace-root).
