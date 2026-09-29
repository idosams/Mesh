# Developer guide

The canonical development repository is [idosams/Mesh](https://github.com/idosams/Mesh). Verify actual remotes and base ancestry; the local folder name proves nothing. Follow the [agent workflow](../AGENTS.md) and [migration ledger](plan/fleet-migration.md) when transferring older work.

Mesh is a Rust workspace with a Tauri desktop application and a React interface. The daemon owns
durable workspace state; clients use the local IPC protocol rather than opening its database.

## Prerequisites

- Git
- The Rust toolchain pinned in `rust-toolchain.toml`
- Node.js 22.18 or newer (the locked Vite build and direct TypeScript tests require it)
- `cargo-nextest`
- The native Tauri prerequisites for your operating system

On macOS, install Xcode command-line tools. On Linux, install the WebKitGTK and application-indicator
development packages named in `.github/workflows/rust.yml`.

## Verify a checkout

```bash
npm test
```

This runs the documentation audit and its regression tests, license and storage-budget mutation
checks, Rust formatting and linting, all Rust workspace tests, the complete desktop/React suite, and the real daemon restart demo. Individual commands:

```bash
npm run verify:docs
npm run verify:storage
npm run verify:demo
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace --no-fail-fast
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

## Validation boundaries

Run commands from the repository root unless a guide says otherwise. The documentation audit
checks local Markdown file destinations and literal npm script examples against package manifests.
It checks neither external URLs nor heading fragments and does not execute examples. Rendered HTML
and packaged text guides still need the desktop guide tests and visual verification.

Pull requests and main pushes run the complete `npm test` gate in the macOS test job, followed by
the native renderer checks. The seven existing jobs remain, including independent Linux Rust,
desktop/React, documentation, storage, license and dependency-policy checks. Rust test failures do
not stop the remaining Rust cases; the command still fails, and later gate stages require success.
The nightly job also runs the complete local command plus dependency policy. A configured workflow
is not evidence of a successful hosted run;
inspect the exact revision's results before release.

On macOS, `npm run test:macos-renderers` runs all four PDFKit and Office integration cases using checked-in synthetic fixtures.
It is also a separate macOS CI step. Other measurement and helper cases remain explicitly ignored in the ordinary Rust suite. Run those separately
on the required platform before claiming their behavior. For a local macOS package, use the
[desktop build and rendered-proof instructions](../apps/desktop/README.md). Build from a clean,
committed tree so the embedded revision identifies the tested bytes. Ad-hoc packaging cannot prove
Secure Enclave approval in an eligible signed distribution or installation on a clean Mac.

When a test fails, retain its output and reproduce the named case before changing code. Local
socket restrictions, absent renderer permissions, and missing tools are environment failures;
report them separately from product defects. Never remove a check or relax its assertion merely
to obtain a passing run.

The Node minimum includes default TypeScript stripping, introduced in
[Node 22.18](https://nodejs.org/en/blog/release/v22.18.0), and satisfies the locked Vite engine.

## Checkpoint worker shutdown

Dropping `LiveDaemon` requests idle-checkpoint shutdown, wakes waiting workers and waits until
all worker generations release their strong workspace/checkpoint references. Completion is separate
from timer notifications so draining a retired worker cannot restart another worker's idle interval.
The scheduler's returned join handle remains available to explicit callers. An in-flight native
filesystem or database operation can delay shutdown; requesting stop does not cancel durable work.

Immediate same-path restart must occur after ownership is released. Otherwise a prior connection
can close or replace SQLite sidecars while the new open verifies their identities. Database
single-link, permission and physical identity checks must remain intact. The regression
`daemon_drop_waits_for_checkpoint_database_owners_even_when_worker_unwinds` parks a real pending-save
worker after it owns database references, checks both normal exit and unwind, and immediately
reopens the preserved journal. This does not verify macOS attachment-event registration or packaged
application shutdown; those are separate boundaries.

## Native worker identity provisioning

The native desktop binary has two explicit worker-identity modes. These are source-supported setup
commands; an eligible Apple-signed Mesh application and actual signed-app acceptance are still
required. Unsigned builds refuse before touching worker files. No installed worker listener or
second-machine execution is implied.

Choose an existing, empty metadata directory owned by the current user, with permissions `0700`,
outside existing projects. The native command does not create this directory or select one implicitly.
For an eligible signed application, the invocation is:

```bash
/path/to/signed/Mesh.app/Contents/MacOS/Mesh --worker provision /absolute/private/worker-state
/path/to/signed/Mesh.app/Contents/MacOS/Mesh --worker identity /absolute/private/worker-state
```

Provision writes a durable setup intent, creates an execution-only keychain identity and initializes
its private ledger. Identity reopens existing complete state and checks the expected stored key;
it never creates a replacement. Both print one `mesh.worker-public-identity/v1` JSON object with
public installation and worker identifiers. This output does not establish peer trust or authorize
execution. No project files, selected desktop workspace, human approval key or provider process are
changed by these modes.

Failed or interrupted setup retains its files and any created key for reconciliation. Repeating
provision against a nonempty directory refuses. Keep retained evidence; these commands provide no
repair, cleanup, rotation or deletion operation. The private installation contains only `intent.json`,
`identity.json` and `ledger/`; additional entries make it unavailable rather than being ignored.
Existing standalone worker-ledger directories are not automatically converted into installations.

Current automated evidence covers native directory/receipt/lock behavior with a custody test double,
and command parsing plus refusal by an unentitled executable. It does not prove successful OS
keychain provisioning, an installed resident service or actual remote execution.
