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
folder after a fresh preview in builds with a validated Developer ID identity. Protected approval
and updating the original folder are unavailable in the ad-hoc public alpha.

Desktop import requires at least one included file or subfolder. Empty folders and Git-metadata-only
folders are unsupported; see the [import troubleshooting playbook](user-playbooks.md#first-session).
The desktop offers English and Hebrew; see [language support](localization.md) for coverage.

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

## Inspect an attached project's saved files (development builds)

The unmerged attachment increments in the [migration ledger](plan/fleet-migration.md) add this
flow to development builds. This is not a claim that the downloadable alpha contains it or that
its packaged UI acceptance is complete.

1. In the folder selection view, use **Choose project to attach**, or enter the existing project
   path and choose **Attach existing project**. Continue using the same folder in your editor,
   terminal or harness; Mesh keeps its history in separate storage.
2. Once a version is saved, choose **Show latest versions**. Use **Older versions** to page back.
3. Select a saved version identity to list its files and folders. **More files** opens the next page.
4. Select a file to inspect its saved text. This preview stays on that exact version even when the
   live folder changes or capture saves a newer version. Refreshing capture status does not replace
   the preview; select another saved version or file to change it.

Text previews are read-only and limited to 256 KiB. Binary and larger files show why text is
unavailable. To compare saved versions, choose **Use as base** on one version and **Compare with base** on
another. Select a changed path to see its saved before/after contents. The open comparison stays
fixed when capture advances or you choose the next comparison base. Missing files, folders and
unavailable text previews have explicit states. Choose **Pin comparison alongside others** to keep up
to eight comparisons open across projects. Each pin has its own page and selected file; you can pin
the same version pair twice to inspect different files. Close a pin to free a slot. Pin selections are saved separately from file contents and return after restart, with contents
verified again from native history. Wait for **Comparison selections saved** before quitting. If
saving fails, views remain open and the status offers retry or an explicit reload of the saved set.
Unavailable comparisons retain their version selection and offer retry rather than showing
unverified content. This view does not approve or apply changes. **Stop capture** and **Resume
capture** control the session without changing the original project's files. After restarting this
development build, registered projects return with capture stopped and their saved history available.
Choose **Resume capture** when ready to capture changes made while Mesh was closed. If the original
project or history is unavailable, Mesh keeps the registration visible and requires the exact original
folder and history before retrying; a replacement folder at the same path is not adopted.

Choose **Detach Mesh** to stop Mesh tracking this project across restarts. Mesh joins its capture
worker before confirming detachment and retains the original files, Git state, saved history and
comparison pins. You can keep using your existing tools. **Reattach project** checks the original
folder identity and restores the controls with capture still stopped; choose **Resume capture**
separately when ready. An unavailable or replaced folder cannot be reattached. If detachment cannot
be confirmed, refresh status and reconcile the reported problem rather than assuming it was saved.

While capture is running, Mesh reports whether file-change signals are active or it is using
periodic checks. On macOS, signals can wake capture sooner; periodic checks still catch missed
changes. This status does not mean an edit has been saved: check the latest saved version and
capture outcome. Mesh does not infer who made a change from filesystem events.

Capture continues while file-change monitoring is being prepared or is unavailable. The status
shows these separately. Once capture shows **Stopped**, it performs no further saves; monitoring
may still be finishing, and that status remains visible until it ends. Resuming starts a new
capture session. An older monitor cannot restart the stopped session.


In the unmerged review-history increment, an already-open pending review stays on its original
comparison base when another review advances main, including after restart. This keeps the reviewed
changes stable; it does not make an old approval valid against the new main version. A stale approval
must be prepared and reviewed again through the normal current-main flow.

In the unmerged desktop recovery increment, **Compare main with working files** can offer **Review
applying this file** for an existing text file that still matches the approved base. The native
confirmation shows the exact current and proposed content, file location and permissions. Confirming
changes that working file and retains its previous file for recovery, including later writes from an
already-open editor. Mesh main and Git are unchanged. Changed files, additions, deletions and grouped
changes cannot use this action yet.

Use **Refresh retained files** to inspect the last observed recovery state. **Review restoring
retained file** prepares a frozen copy of the selected retained work and requires another native
confirmation. The current working file becomes a new retained entry, so undo is another explicit
restoration. Existing retained files remain available. An exact recovery reference can inspect a
record outside the bounded overview. Missing results and later edits require inspection; they do not
prove the earlier action was rolled back. Nothing is retried or deleted automatically.

The native confirmation currently uses English and refuses binary, NUL-containing or oversized
content rather than omitting it. Recovery controls retain English/Hebrew UI text. The default
recovery location must be on the project's volume; offline folders and a location selector for
other volumes remain unfinished. Actual packaged confirmation and recovery acceptance are pending.

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

## Request review of an attached version (development builds)

In the unmerged review-request increment, open **Show latest versions** and choose **Request review**
on the saved version you want reviewed. Mesh records an exact request against its accepted main
(or the empty starting state before the first approval), without changing your working folder.
Use **Show review requests** and **Open review** to return to it after other edits or a restart.
**Inspect saved result** opens that request's saved files rather than the newest capture.

The queue shows up to 32 requests and reports omitted requests. A saved-version request can reopen
its exact current-base request beyond that overview. A selected request remains fixed while capture
continues. Incomplete or unavailable content is labeled; a path overview alone is not complete review.
Change authorship remains unknown. Native main approval and desktop confirmation are separate development increments; applying accepted
content to the working folder remains pending.

## Accepted main for attached projects (native development API)

The unmerged native approval increment can accept an exact saved review as Mesh main, even while your
editor or harness continues producing newer changes. Acceptance records the reviewed saved content
in Mesh history; it does not replace your working files or change Git. A review prepared before main
advances must be prepared again against the new main before approval. Returning to an old review is
still supported for inspection. Desktop approval controls are described below. Applying accepted content to the working folder
remains a separate pending increment.

## Confirm an attached version as Mesh main (development builds)

Choose **Refresh main and approval availability**. Mesh reports the last verified accepted version
and whether this build can approve. If eligible, **Set up approvals on this Mac** enables the native
credential flow. Open a complete saved review, inspect its content, then choose **Approve as Mesh
main…**. The native dialog shows the exact saved result and requires confirmation and Touch ID or
your Mac password. Your working files and Git remain unchanged.

Use **Inspect Mesh main** to reopen the accepted saved result. If confirmation is cancelled or the
response is uncertain, refresh main before retrying; an uncertain response does not mean acceptance
was rolled back. A main change requires a new review against the current main. Ineligible builds and
unverified main status keep approval unavailable. This development flow still needs packaged graphical
and actual platform-presence acceptance evidence.
