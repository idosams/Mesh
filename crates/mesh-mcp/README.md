# Mesh MCP alpha bridge

`mesh-mcp` lets an agent inspect the workspace currently open in the local Mesh app. It is a
newline-delimited stdio MCP server with one tool:

- `mesh_workspace_state` returns the verified `workspace.state` projection, including the native
  working root, durable versions, native untracked files, reviews, and explicit `not_yet` gaps.

This surface is intentionally read-only. The agent first calls `mesh_workspace_state`, then edits
only the native folder returned as `structuredContent.root` with its normal filesystem tools. The
agent session stays pinned to that folder even when Mesh later switches versions; start a new agent
from Mesh for the new version instead of following a moving link. The existing agent can keep
working safely in its pinned folder. While another version is selected, its read-only Mesh context
will refuse rather than showing the wrong workspace; select that exact agent folder in Mesh before
inspecting or saving its changes.

When the agent finishes and every related agent, terminal, and editor using that folder has stopped,
it tells the person to select that exact folder in Mesh, choose **Finish agent handoff**, inspect the
complete result, and choose **Save all privately**. The person can review and approve the resulting
durable version, then use **Update original folder** in the primary **Current state** actions when
they intentionally want those approved bytes copied back to the original unmanaged folder. This
server cannot finish the handoff, save, switch versions, update the original folder, review,
publish, approve, or bypass those boundaries.

Start Mesh and open the workspace first. Build the bridge with:

```sh
cargo build -p mesh-mcp --release
```

Then configure the MCP client to launch the absolute binary path. On macOS no endpoint argument is
needed when using the packaged app's default socket:

```toml
[mcp_servers.mesh]
command = "/absolute/path/to/Mesh/target/release/mesh-mcp"
```

For a manually started daemon, add the exact endpoint it printed:

```toml
args = ["--endpoint", "/absolute/private/runtime/daemon.sock"]
```

The socket must be real (not a symlink), inside a real owner-only directory, and owned by the same
user as that directory. Daemon failures become visible tool errors rather than empty state.
