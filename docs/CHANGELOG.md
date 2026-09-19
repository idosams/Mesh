# Mesh changelog

Mesh is not yet a stable product. This changelog records public, installable releases; internal
development commits and private test builds are deliberately omitted.

## [0.1.0-alpha.4] — 2026-09-19

Preview and workspace-identity fixes for the macOS technical alpha.

### Fixed

- Text and JSON files in managed and agent-assigned workspaces now open in the inline preview
  instead of incorrectly reporting that inspection is unavailable.
- Workspace refreshes preserve the selected file and reuse stable projections, reducing visible
  flicker while a workspace is being monitored.
- Version folders exposed to Finder, terminals, editors, and software pickers now use the human
  name `Mesh Version - Working Folder` instead of the internal name `mounts`.
- Approval setup in an ad-hoc build now stops immediately with the real Apple-identity requirement
  instead of beginning a flow that cannot complete.
- The packaged export journey waits for the filesystem receipt before declaring success.

Human approval itself remains unavailable in this ad-hoc, unnotarized build. It requires a
Developer-ID-signed Mesh application with a validated stable Apple identity.

## [0.1.0-alpha.3] — 2026-09-18

Explorer and review candidate for the macOS technical alpha.

### Improved

- Files is now a content-first workspace explorer with persistent folders, search, keyboard
  navigation, breadcrumbs, recognizable file types, and collapsible explorer and details panes.
- Changes keeps the selected file visible across bounded large-workspace windows and shows actual
  text changes directly in the workbench.
- Saved Review groups changed files, preserves the selected viewer, previews supported text,
  images, PDF, and Office content, and treats unsupported binaries as exact metadata.
- Live agent work can be inspected before handoff through bounded, read-only snapshots without
  exposing save, approval, export, or update-original authority.
- Workspace-open failures now distinguish a missing managed workspace from an ordinary folder and
  provide clear retry, import, or forget actions without replacing the current safe workspace.

The signing, notarization, approval, platform, and backup limitations below are unchanged.

## [0.1.0-alpha.2] — 2026-09-16

Tester-feedback hotfix for the first macOS technical alpha.

### Fixed

- Review renders generic binary changes such as `.DS_Store` as exact metadata instead of failing
  the whole page.
- Files shows the managed folder hierarchy as an accessible tree.
- Current monitors exact changed paths in an assigned agent folder without saving or approving
  them, and offers direct switching among recent and agent-assigned workspaces.

The signing, notarization, approval, platform, and backup limitations below are unchanged.

## [0.1.0-alpha.1] — 2026-09-16

First public macOS technical alpha.

### Included

- A React-only desktop interface organized into route pages and reusable atoms, molecules,
  organisms, layouts, and pages.
- Protected import into an application-managed working copy while retaining the selected source
  folder as an untouched backup.
- Explicit private saves, retained workspace points, restore and undo, review, and guarded export
  to a separate ordinary folder.
- Isolated Codex and terminal handoffs, complete finish-time inspection, Git-backed workspaces, and
  reviewed Git export.
- Text, image, PDF, and Office review previews with exact source-revision and packaged-window proof.
- An ad-hoc-signed, unnotarized macOS archive with a canonical manifest and SHA-256 checksum.

### Supported environment

- Apple-silicon Mac (`arm64`).
- macOS 11 or newer.
- Local workspaces only. Hosted collaboration and multi-device synchronization are not included.

### Known limitations

- The downloaded app requires an explicit macOS **Open** confirmation. Never disable Gatekeeper
  globally or remove quarantine metadata.
- Human approval and **Update original folder** are unavailable in this ad-hoc build because both
  require a stable Developer ID application identity.
- There is no automatic update channel. Quit every running Mesh process before replacing the app.
- Windows and supported Linux installers are not part of this release.
- Filesystem changes made while Mesh is not running are inspected when the workspace is reopened;
  continuous background capture is not claimed.

[0.1.0-alpha.4]: launch/v0.1.0-alpha.4.md
[0.1.0-alpha.3]: launch/v0.1.0-alpha.3.md
[0.1.0-alpha.2]: launch/v0.1.0-alpha.2.md
[0.1.0-alpha.1]: launch/v0.1.0-alpha.1.md
