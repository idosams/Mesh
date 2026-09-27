# Mesh MCP alpha bridge

`mesh-mcp` lets an agent inspect the workspace currently open in the local Mesh app. It is a
newline-delimited stdio MCP server. Its default, unscoped mode has one read-only tool:

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

## Scoped fleet integration (development)

A native fleet host can launch this same binary with `MESH_FLEET_OBJECTIVE` and
`MESH_FLEET_CREDENTIAL` in its private process environment. Both must be present. Do not put the
credential in a prompt, command-line argument, workspace file or support log. This is a native
integration surface; the desktop does not yet expose a complete fleet launch journey.

In this mode the bridge advertises five tools:

- `mesh_fleet_context`: the bound objective, actor, session, run, lane and exact workspace.
- `mesh_fleet_delegate`: allocate a child from one of that lane's saved versions, with a stable
  request identity, goal and configured provider. Destinations are chosen by native code.
- `mesh_fleet_children`: observe only that lane's direct children.
- `mesh_fleet_checkpoint`: capture supported private edits and additions using a stable `request`.
  Requires a native capture-enabled session backed by its actual actor signing key. Incomplete
  results preserve any saved progress and identify the remaining issue; they are not full saves.
- `mesh_fleet_submit_review`: submit the opaque `checkpoint` identity returned by a completed
  capture. The native host records a review of its exact saved version; retry returns the same
  bundle, even while newer private files are being edited. Incomplete and other-session captures
  cannot be submitted. This operation retains agent custody and cannot approve anything.

Checkpoint intent and bounded result are durable. Retrying a completed request returns its original
version even after newer working edits. Use a new request for a new capture or to continue after an
incomplete result. A pending request after interruption requires native recovery and is never
silently executed again. Missing or unsupported entries require explicit resolution. Checkpoints
retain agent custody and never advance the main/shared version.

The native host checks the current run and exact custody generation on every call. Rotation,
revocation, cancellation, terminal runs and a service restart invalidate credentials. Repeated
identical delegation returns the same child; reusing a request for different work fails. Delegated
lanes retain durable actor/session/run attribution. Tokens never enter the control ledger.

Fleet mode requires IPC surface 8 and refuses an older daemon before sending credentials. Default
workspace mode remains compatible with earlier supported surfaces. Child allocation is real native
version reconstruction. Scheduling real providers, packaged review UI integration and native restart
reconciliation remain incomplete. These tools cannot approve or publish shared state.
