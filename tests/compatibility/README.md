# tests/compatibility

**Maturity: partial evidence corpus.** The table below distinguishes published contracts and
oracles from missing runners. See [Project status](../../docs/project-status.md).

Editor, build-tool, package-manager and filesystem-edge corpus.

**Partly filled.** This directory is part of the plan §8.2 structure and is scaffolded so that
"unstarted" is visible rather than absent. It is filled under **E06 filesystem strategy**.

| Directory | What it holds | State |
|---|---|---|
| [`adapter/v0/`](adapter/v0/README.md) | `mesh-workspace-adapter/0` — the published contract every filesystem backend is graded against, and the closed vocabulary the conformance suite is generated from | contract and Rust conformance suite landed (`crates/mesh-materializer/src/conformance.rs` and `crates/mesh-materializer/tests/adapter-conformance.rs`); the vocabulary is checked against the Rust surface in both directions |
| [`save-patterns/v0/`](save-patterns/v0/README.md) | `mesh-save-patterns/0` — what real editors, version control, formatters and package managers do to a folder when a person saves, and the single meaningful durable change for each | corpus and executable reference oracle published, eight patterns (research item R5, task `01KZC2VTSZCB4Q7DC9C9X5NM1V`, ADR-0039); the consuming Rust `save-patterns` target still belongs to `01KZC2YXH6FG9C67JP9DQX7JH6` |
| the filesystem-edge corpus | adversarial symlinks, name folding, and the rest of plan §8.2's edge material | not yet implemented |

Find the tasks that fill it:

Use the [public issue tracker](https://github.com/idosams/Mesh/issues) to propose or track this work.
