# Mesh architecture

This document explains the public repository's structure and intended boundaries. The workspace
members and direct dependencies are declared in [Cargo.toml](../Cargo.toml) and each crate's
manifest. The former internal architecture-map checker is not distributed in this public tree;
its capability and ownership rules are not an automated public gate. Rust compilation, linting,
and the subsystem tests validate the executable boundaries described below.

For implementation maturity rather than design intent, use [Project status](project-status.md).

## Current local path

The working proof composes the repository like this:

```text
desktop or meshctl
        │ local versioned IPC
        ▼
      meshd
        │ composes policy, review, state, and recovery
        ├── SQLite store and journal
        └── content-addressed chunk store
```

`meshd` is the single local composition boundary. The user interface reaches durable state
through the service instead of opening the database or content store. The desktop's narrow native
managed-file commands remain inside its Tauri host and call the same local service owner.

The repository also contains a deterministic synchronization engine and wire-message code, but
there is no public peer session, relay-backed user path, or hosted service today. Those components
must not be inferred from the current local proof.

## The eight layers

Dependencies point downward. A layer may depend on a lower-ranked layer, subject to each crate's explicit dependency declarations.

| Rank | Layer | Responsibility |
|---:|---|---|
| 0 | Foundation | Canonical entity model with no workspace or external dependency |
| 1 | Core | Pure operation semantics, in-memory state, materialization, and conflicts |
| 2 | Core services | Keys, policy, chunk boundaries, wire messages, approval envelopes, context records, and validators |
| 3 | Engine | Resource owners: local database, content store, and transport |
| 4 | Adapter | Platform, version-control, and model-provider boundaries |
| 5 | Service | The long-running local daemon that composes lower layers |
| 6 | UI | User-facing surfaces that reach state through the service |
| 7 | Harness | Simulation and measurement; product code never depends on it |

Important capability labels include direct storage and database access, canonical mutation,
network access, platform adaptation, model-provider access, UI presentation, and optional
integration edges. Review changes to these boundaries explicitly; successful compilation alone does not prove
architectural intent.

## Directory map

| Path | Role |
|---|---|
| `crates/` | Apache-2.0 Rust libraries plus the local daemon and clients |
| `apps/desktop/` | Tauri desktop UI and its local native host |
| `platform/` | Operating-system adapters and packaging work |
| `protocol/` | Versioned public schemas, vectors, conformance assets, and compatibility policy |
| `integrations/` | Agent, provider, and version-control integration surfaces |
| Hosted services | Maintained outside this public tree; no operated service is provided |
| `tests/` | Cross-component, fault, security, convergence, and interoperability evidence |
| `benchmarks/` and `models/` | Reproducible performance and correctness evidence |
| `tools/program/` | Repository gates and program automation, not runtime product code |
| `docs/` | Public guides, product authority, decisions, plans, and classified evidence |

## Durable-state and trust boundaries

- `mesh-store` owns the SQLite WAL database and canonical compare-and-swap persistence.
- `mesh-cas` owns immutable content chunks on disk.
- `mesh-daemon` owns process composition, the local IPC boundary, recovery, and service events.
- `mesh-approval` defines exact review and approval envelopes; the service applies their durable
  mutation.
- `mesh-crypto` defines signing and verification. Operating-system-backed key custody belongs in
  `platform/mesh-keychain`; software-held local demonstration keys are not the final human trust
  boundary.
- User interfaces never open the canonical database directly.
- Hosted identity, relay, and web review remain planned scaffolds.

The public wire authority lives in `protocol/schemas/` and `protocol/test-vectors/`. Repository
crates can be ahead of that published surface; such code is not a stable external contract until
the versioned artifacts and conformance expectations are updated.

## Design principles

These are target architecture constraints, not a maturity ledger. The current proof realizes only
the paths listed in [Project status](project-status.md).

### Work is automatic; publication is explicit

The intended product makes saving private work part of the system rather than a manual ceremony.
Advancing protected shared state remains a distinct, exact, human-reviewed action.

### Identity survives path changes

Files and directories have stable identities. Moving or renaming them does not sever their
history.

### Valid work is retained

Recovery and conflict handling preserve durable versions. The system asks for a decision instead
of silently choosing or deleting valid work.

### Approval binds exact evidence

Approval covers the reviewed bytes, base, selection, conflict resolutions, and validation
evidence. A later change cannot enter an earlier approval.

### Local work does not wait for connectivity

Local durability is not conditional on the network. Delivery to peers is separate from
publication to protected shared state.

### Claims need executable evidence

Safety, compatibility, losslessness, and performance claims require tests, mutation fixtures,
benchmarks, simulators, or another reproducible oracle.

## Decisions and evolution

Use the [public decision summaries](design-decisions.md) for durable tradeoffs. Update crate
manifests and this document when dependency authority or the reader-facing model changes. Product requirements belong in the [charter](charter.md) and
[PRD](product-prd.md); observed delivery belongs in [project status](project-status.md).
