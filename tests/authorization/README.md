# tests/authorization

**Maturity: partial. The policy-layer publication attack suite exists; the cross-component harness
in this directory does not.** [`mesh-policy`](../../crates/mesh-policy/) tests agent, delegation,
replay, expiry, revocation, epoch, and human-principal attack paths and treats zero unauthorized
policy decisions as the bar. See
[Project status](../../docs/project-status.md) and query the E13 owner in the
[program map](../../docs/program-map.md).

The cross-component publication attack harness. Zero unauthorized publications is the bar.

**Not yet implemented here.** This directory is scaffolded for the E13 harness across process,
approval, storage, and publication boundaries. The existing policy suite proves the decision-layer
guard; it is not evidence for every composed path.

Find the tasks that fill it:

```bash
See the public GitHub issue tracker
```
