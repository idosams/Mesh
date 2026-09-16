# Developer guide

Mesh is a Rust workspace with a Tauri desktop application and a React interface. The daemon owns
durable workspace state; clients use the local IPC protocol rather than opening its database.

## Prerequisites

- Git
- The Rust toolchain pinned in `rust-toolchain.toml`
- Node.js 22.6 or newer
- `cargo-nextest`
- The native Tauri prerequisites for your operating system

On macOS, install Xcode command-line tools. On Linux, install the WebKitGTK and application-indicator
development packages named in `.github/workflows/rust.yml`.

## Verify a checkout

```bash
npm test
```

This runs the license mutation checks, Rust formatting and linting, all Rust workspace tests, and
the complete desktop/React test suite. Individual commands:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
npm --prefix apps/desktop test
```

## Run the desktop app

```bash
npm --prefix apps/desktop run tauri:dev
```

The desktop client talks to the local daemon through a user-owned Unix socket. Use disposable or
backed-up projects during the alpha.

## Repository map

- `crates/` — protocol, storage, materialization, policy, approval, daemon, SDK, and adapters.
- `apps/desktop/` — Tauri shell, React interface, packaging, and renderer proofs.
- `protocol/` — schemas, wire format, test vectors, and conformance material.
- `tests/` — cross-crate compatibility, authorization, recovery, and fault evidence.
- `benchmarks/` — reproducible workloads and measured budgets.
- `integrations/` — agent and Git integration surfaces.
- `docs/` — user, architecture, protocol, security, and status documentation.

Do not treat an internal Rust module or current JSON shape as a stable public API unless the
protocol documentation explicitly says it is versioned.
