# tests/corruption

**Maturity: partial. Crate-level corruption and repair evidence exists; the cross-component
re-request campaign does not.** [`mesh-cas`](../../crates/mesh-cas/) tests reject and quarantine
bad chunks, preserve diagnostic samples, and prove good bytes can be promoted after quarantine.
See
[Project status](../../docs/project-status.md) and query the E12 owner in the
[program map](../../docs/program-map.md).

Cross-component corrupt-chunk injection: rejected and re-requested, never materialized.

**Not yet implemented here.** This directory is scaffolded for the E12 campaign that must drive a
re-request through synchronization and prove the corrupt bytes never materialize. Existing
`mesh-cas` coverage proves the local rejection, quarantine, and repair halves, not that full path.

Find the tasks that fill it:

```bash
See the public GitHub issue tracker
```
