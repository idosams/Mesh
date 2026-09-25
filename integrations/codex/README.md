# Codex integration

**Maturity: bounded functional alpha.** The macOS desktop opens an exact native Mesh folder in
Codex with optional read-only Mesh context. It does not provide complete per-run identity,
version-attributed reads, or an agent mutation API.

Start Codex using the action on the selected current or saved version. Mesh verifies the physical
workspace, installation, and version before handing off the real folder. The agent stays pinned to
that folder when Mesh selects another version; the moving navigation link does not move its work.

The native launcher supplies session-specific MCP configuration to `codex app`. New workspaces do
not receive a generated `.codex` entry, and existing project or global settings remain unchanged.
Mesh can refresh the exact private compatibility link created by older alpha builds. If a suitable
Codex CLI cannot accept the context configuration, Codex can still open the folder and Mesh reports
that its optional tool is unavailable. This behavior is implemented and tested in
[the native launcher](../../apps/desktop/src-tauri/main.rs) and
[context configuration](../../apps/desktop/src-tauri/codex_workspace.rs).

The only exposed tool is `mesh_workspace_state`. It returns a bounded verified projection from the
running daemon. If Mesh is serving another physical folder or installation, the tool refuses;
select the agent's exact folder in Mesh to inspect it again. The bridge cannot save, approve,
publish, finish a handoff, or update the original folder.

The agent edits ordinary files through its usual tools. When it finishes, stop every writer using
that folder, select it in Mesh, and follow **Finish agent handoff** through inspection and private
saving. See the [user playbooks](../../docs/user-playbooks.md) for the complete sequence, recovery,
and export boundaries. The [MCP bridge guide](../../crates/mesh-mcp/README.md) covers standalone
configuration for development.
