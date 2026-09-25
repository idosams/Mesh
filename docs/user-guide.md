# User guide

This guide covers the local Mesh proof that works today. The downloadable technical alpha remains a prerelease.
The default demonstration runs on macOS or Linux, uses a temporary workspace, and does not need an
account or service credential.

## What you can prove today

The command-line demonstration starts the real local service and client, saves an exact file
version, restarts over durable data, opens an exact review, proves a software-held key cannot
publish it, restarts again, previews a redacted support bundle, and shuts down cleanly. The separate
desktop alpha imports an ordinary folder without changing it, opens a stable native working path,
launches Codex in an independently pinned folder, privately saves unambiguous returned changes when
the person confirms that the agent has finished, and can return an approved version to the original
folder after a fresh preview.

It does **not** prove background saving of every editor change, another device receiving the work,
a hosted service, an unattended distributable installer, or a complete agent integration.
See [Project status](project-status.md) for the complete boundary.

## Prerequisites

- macOS or Linux
- Git
- Node.js 22.18 or newer
- the Rust toolchain selected by `rust-toolchain.toml`

The first run may download Rust dependencies. From the repository root:

```bash
node examples/local-daemon-demo.mjs
```

A passing run prints 44 `✓` checks. It exercises real `meshd` and `meshctl` binaries and removes
its temporary workspace when it finishes. If a check fails, the command exits nonzero, identifies
the failed stage, and retains the workspace path for inspection.

After dependencies are cached, the offline proof is:

```bash
node examples/local-daemon-demo.mjs --offline
```

Use `--skip-build` only when the required binaries have already been built. On a Linux machine
configured for FUSE, `--mounted` adds the privileged mounted-workspace proof. The default command
does not claim that privileged path.

For the expanded transcript and equivalent manual commands, read
[Run the local Mesh demo](demo.md). The separate
[mounted-workspace evidence](local-mounted-demo.md) records a privileged environment-specific
proof and its limits.

## The six words Mesh shows people

| Status | Meaning |
|---|---|
| **Working** | An actor is changing files now. |
| **Saved privately** | The work survived locally and is not shared. |
| **Available to team** | A peer can open it read-only; it is not in the shared version. |
| **Ready for review** | An exact change is waiting for a person. |
| **Needs attention** | A person must decide before work can proceed. |
| **Approved** | The reviewed bytes advanced the protected shared version. |

The ad-hoc technical-alpha archive exercises **Saved privately** and recorded review end to end.
The protected **Approved** transition is implemented but requires a separately Apple-signed build
with a stable, validated application identity; it is unavailable in that archive. The other words
define the intended product experience; they do not imply that team delivery or a hosted workflow
is available.

## Try the desktop technical alpha

The desktop app is a real Tauri surface for the local managed-folder journey. It can import a
folder, retain exact versions, recover across restart, work through ordinary native folders, open a
pinned agent copy in Codex, inspect and explicitly or automatically save supported external changes,
restore earlier versions,
and exercise local review. A separately Apple-signed, identity-continuous build additionally
enables macOS user-presence approval.

```bash
npm --prefix apps/desktop run tauri:dev
```

The development command above is for repository evaluation. A consented tester can instead use the
revision-bound archive described in [Alpha start here](../apps/desktop/ALPHA-START-HERE.txt). The
macOS app is ad-hoc signed and unnotarized, so a downloaded copy requires an explicit **Open**
confirmation; Windows support and unattended installers are not ready. Follow the
[desktop guide](../apps/desktop/README.md) for the exact journey and verification commands.

## Privacy and safety

- The default proof stays on the local machine and opens no hosted account.
- Saved content is retained in local storage; treat its directory like any other sensitive
  developer data.
- The support-bundle command is a local preview. Inspect the preview before sharing anything.
- The CLI demonstration key cannot publish. On supported Macs, an Apple-signed desktop build with
  a validated application identity uses a separate P-256 approval credential whose private key
  stays in the Secure Enclave and whose use requires fresh Touch ID or macOS password presence.
  The ad-hoc technical-alpha archive reports this approval path unavailable.
- Do not use this proof as the only copy of important work.

Report security findings through the [private security channel](../.github/SECURITY.md), not a
public issue. Follow that policy for the report contents and response targets.

## Troubleshooting

If the first build cannot download a dependency, restore network access and retry. If the offline
command fails, run the normal command once so Cargo can populate its cache. If a run fails after
starting, use the retained workspace path printed by the script and rerun without `--offline` to
separate an environment problem from a product failure.

For build-tool errors, check the prerequisites and commands in the
[developer guide](developer-guide.md). For ordinary work, agent handoff, recovery, and export,
follow the [user playbooks](user-playbooks.md).

For known limitations or to verify whether a capability has landed since this guide was updated,
check [Project status](project-status.md).
