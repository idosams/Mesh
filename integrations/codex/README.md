# Codex integration

**Maturity: bounded functional alpha.** The desktop can open one exact native Mesh workspace in
Codex with a bundled, read-only MCP bridge. It does not yet provide the complete distinct-agent-run
identity or mutation surface described in the target architecture. Select **Open in Codex** under
**Current**. Mesh then:

1. re-verifies the displayed workspace root, journal digest, physical installation, and real
   version directory;
2. writes the MCP configuration under the workspace's private store and exposes it through a
   project-local `.codex` symlink, so the machine-specific application and socket paths never
   become saved workspace content;
3. opens the exact version directory in the Codex desktop app, never the retargetable
   `native-workspace/current` convenience link; and
4. runs the bridge from the same installed Mesh executable with the expected physical workspace
   root and installation fixed in its arguments.

The [Mesh MCP alpha bridge](../../crates/mesh-mcp/README.md) documents the standalone protocol and
manual configuration for development builds.

Codex may ask the user to trust that folder once before it loads project configuration. The only
tool currently exposed is `mesh_workspace_state`. It returns the running daemon's bounded,
structured workspace projection and reconnects for every call. Durable saves may advance normally
inside the same physical folder. If Mesh later serves a different root or physical installation,
the tool refuses instead of silently showing another workspace.

The root `.codex` path is created only when absent. An existing file, directory, or unrelated
symlink is preserved and the launch refuses with manual-setup guidance. Reopening after a valid
private save refreshes Mesh's private binding for the new Codex session. Removing the generated
`.codex` symlink does not delete private workspace history or user files.

This is an honest read-only context slice, not the complete §8.2 agent integration. Codex edits the
ordinary native files using its normal filesystem tools; return to Mesh to inspect and explicitly
save those changes. The bridge does not yet expose Mesh file reads, search, changes, checkpoints,
publication, or approval, and it does not yet record distinct agent-run/read-ledger identities.
