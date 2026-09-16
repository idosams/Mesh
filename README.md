# Mesh

Mesh is a local-first workspace system for humans and coding agents. Each actor works in a durable
private state, and only an exact human-reviewed state can advance the protected shared version.

The first public alpha targets Apple-silicon Macs. It is intended for disposable or backed-up
projects while the storage, review, agent-handoff, and publication boundaries are tested by real
users. It is not yet a stable API or a hosted collaboration service.

## What works in the alpha

- Import an ordinary folder without changing the original.
- Work in an app-managed native folder with editors, terminals, and coding agents.
- Inspect and privately save supported native file and folder changes.
- Keep durable private saved points and reconstruct any saved point into another native folder.
- Review exact content, record the review, and require native human presence before approval.
- Export selected private or approved content back to an ordinary folder through an explicit plan.
- Finish an agent handoff through a complete scan, private save, release, and rescan sequence.

Mesh fails closed around ambiguous filesystem changes, stale review authority, unsupported entries,
and missing human-presence capability. Read [Project status](docs/project-status.md) for the exact
boundary between working, partial, and planned behavior.

## Approval boundary

On supported Macs, a build signed with a stable, Apple-validated application identity can enroll
one device-only approval credential and require native human presence for the exact reviewed
version. The current ad-hoc technical-alpha archive reports **Approval unavailable** because it
does not have that stable distribution identity.

In an eligible signed build, the original-folder journey proceeds from recording a complete review
to approving that exact saved version with native human presence. Only then can its reviewed files
be previewed and returned to the original project. A private saved point can instead be copied to a
different ordinary folder without presenting that export as approval.

## Run the local proof

On macOS or Linux with the pinned Rust toolchain and Node.js 22.6 or newer:

```bash
node examples/local-daemon-demo.mjs
```

The proof builds and runs the real daemon and CLI, saves a file version, restarts over durable
state, opens an exact review, proves software-held approval cannot publish, creates a redacted
support bundle, and stops cleanly.

## Build and test

Install the tools named in `rust-toolchain.toml`, plus `cargo-nextest`, Node.js 22.6 or newer, and
the native desktop prerequisites for your platform.

```bash
npm test
```

The root test runs the Rust formatting, lint, and workspace suites, the complete desktop/React
suite, and the source-licensing mutation checks. Useful individual commands:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
npm --prefix apps/desktop test
node tools/program/license-check.mjs --self-test
```

## Documentation

- [User guide](docs/user-guide.md)
- [Developer guide](docs/developer-guide.md)
- [Architecture](docs/architecture.md)
- [Protocol](docs/protocol.md)
- [Security policy](.github/SECURITY.md)
- [Public alpha](docs/launch/public-alpha.md)

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. Security issues must use the
private reporting channel described in [.github/SECURITY.md](.github/SECURITY.md), never a public
issue.

## License

Mesh client source is licensed under Apache-2.0. The full text is in [LICENSE](LICENSE).
The operated-service scaffolds and internal engineering lifecycle are not part of this public
source tree and are not required to build or test the local Mesh client.
