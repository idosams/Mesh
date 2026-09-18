import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";
import { build } from "esbuild";

async function loadModule(path) {
  const result = await build({
    entryPoints: [new URL(path, import.meta.url).pathname],
    bundle: true,
    format: "cjs",
    platform: "node",
    write: false,
    external: ["react", "react-dom", "react/jsx-runtime"],
  });
  const require = createRequire(import.meta.url);
  const module = { exports: {} };
  Function("require", "module", "exports", result.outputFiles[0].text)(require, module, module.exports);
  return module.exports;
}

const requiredActions = [
  "refresh",
  "open-folder",
  "open-version",
  "start-codex",
  "start-agent-copy",
  "update-destination",
  "open-terminal",
  "copy-diagnostics",
  "copy-working-path",
  "copy-agent-path",
  "rollback",
];

const projection = {
  state: "Working",
  recordSummary: "12 durable records",
  workingFolder: "/Users/finance/current",
  agentFolderLabel: "Pinned agent folder",
  agentFolder: "/private/mesh/workspace",
  agentAssigned: true,
  nativeFolderHint: "The fixed agent folder stays on this version until its handoff finishes.",
  privateVersion: "Saved point 4",
  privateVersionTitle: "Exact private version: abc",
  sharedVersion: "Approved point 3",
  sharedVersionTitle: "Exact shared version: def",
  destination: "/Users/finance/original",
  entryCount: 2,
  entries: ["forecast.xlsx · file", "reports · folder"],
  conditions: ["Finish agent handoff before changing this folder."],
  agentActivity: {
    state: "ready",
    summary: "2 live changes detected. These remain unsaved until Finish agent handoff.",
    changes: [
      { path: "forecast.xlsx", kind: "modified-file" },
      { path: "notes/today.txt", kind: "new-file" },
    ],
  },
  workspaces: [
    { path: "/private/mesh/workspace", label: "Finance · point 4", state: "current", canOpen: false },
    { path: "/private/mesh/agent-two", label: "Finance · point 3", state: "agent-assigned", canOpen: true },
  ],
  actions: [
    ...requiredActions.map((id) => ({ id, label: id, enabled: id !== "rollback" })),
    { id: "finish-agent", label: "Finish agent handoff", enabled: true },
  ],
};

test("the detailed current projection is bounded, complete, and generation-bound", async () => {
  const module = await loadModule("./src/models/workspace-current.ts");
  const accepted = module.workspaceCurrentEnvelope({ generation: 4, current: projection }, 3);

  assert.equal(accepted.model.agentAssigned, true);
  assert.equal(accepted.model.actions.length, 12);
  assert.equal(Object.isFrozen(accepted.model.actions), true);
  assert.equal(Object.isFrozen(accepted.model.entries), true);
  assert.equal(accepted.model.agentActivity.changes[0].kind, "modified-file");
  assert.equal(accepted.model.workspaces[1].state, "agent-assigned");
  assert.deepEqual(module.workspaceCurrentIntent({ type: "activate", action: "finish-agent" }), {
    type: "activate",
    action: "finish-agent",
  });
  assert.deepEqual(module.workspaceCurrentIntent({ type: "switch-workspace", path: "/private/mesh/agent-two" }), {
    type: "switch-workspace",
    path: "/private/mesh/agent-two",
  });
  assert.throws(
    () => module.workspaceCurrentEnvelope({ generation: 4, current: projection }, 4),
    /stale or invalid/,
  );
  assert.throws(
    () => module.workspaceCurrentEnvelope({
      generation: 5,
      current: { ...projection, workingFolder: "/safe\u202eexe" },
    }, 4),
    /unsafe or unbounded/,
  );
  assert.throws(
    () => module.workspaceCurrentEnvelope({
      generation: 5,
      current: { ...projection, actions: projection.actions.filter((action) => action.id !== "rollback") },
    }, 4),
    /omitted an established control/,
  );
  assert.throws(
    () => module.workspaceCurrentIntent({ type: "activate", action: "rollback", force: true }),
    /unrecognized or missing fields/,
  );
});

test("Current bounds large monitor and workspace projections while search reaches later matches", async () => {
  const module = await loadModule("./src/models/workspace-current.ts");
  const list = module.workspaceCurrentListProjection(
    Array.from({ length: 10_000 }, (_, index) => `entry-${index}`),
    module.CURRENT_MONITOR_ROW_LIMIT,
  );
  assert.equal(list.items.length, 500);
  assert.equal(list.matched, 10_000);
  assert.equal(list.truncated, true);
  assert.equal(Object.isFrozen(list.items), true);

  const workspaces = Array.from({ length: 1_000 }, (_, index) => ({
    path: `/private/mesh/workspace-${index}`,
    label: `Workspace ${String(index).padStart(4, "0")}`,
    state: index === 998 ? "current" : index === 997 ? "agent-assigned" : "available",
    canOpen: index !== 998,
  }));
  const bounded = module.workspaceCurrentWorkspaceProjection(workspaces, "");
  assert.equal(bounded.items.length, 100);
  assert.equal(bounded.matched, 1_000);
  assert.equal(bounded.items[0].state, "current");
  assert.equal(bounded.items[1].state, "agent-assigned");

  const searched = module.workspaceCurrentWorkspaceProjection(workspaces, "workspace 0999");
  assert.equal(searched.matched, 1);
  assert.equal(searched.items[0].path, "/private/mesh/workspace-999");
  assert.throws(() => module.workspaceCurrentListProjection(workspaces, 0), /bound was invalid/);
});

test("the detailed current view exposes custody, conditions, destination, diagnostics, and rollback without native authority", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { WorkspaceCurrent } = await loadModule("./src/organisms/workspace-current.tsx");
  const intents = [];
  const html = renderToStaticMarkup(React.createElement(WorkspaceCurrent, {
    model: projection,
    onIntent: (intent) => intents.push(intent),
  }));

  assert.match(html, /Agent folder assigned/);
  assert.match(html, /In custody/);
  assert.match(html, /Original or destination folder/);
  assert.match(html, /Workspaces and agents/);
  assert.match(html, /Agent running/);
  assert.match(html, /Live agent work/);
  assert.match(html, /forecast\.xlsx/);
  assert.match(html, /Monitoring never saves or approves work/);
  assert.match(html, /Conditions and unavailable controls \(1\)/);
  assert.match(html, />copy-diagnostics</);
  assert.match(html, />rollback</);
  assert.match(html, /disabled=""[^>]*>rollback|disabled=""/);
  assert.doesNotMatch(html, /__TAURI__|invoke\(/);
  assert.deepEqual(intents, []);
});

test("the Current organism emits exact action intents without a legacy control proxy", async () => {
  const { WorkspaceCurrentView } = await loadModule("./src/organisms/workspace-current.tsx");
  const intents = [];
  const tree = WorkspaceCurrentView({
    model: projection,
    onIntent: (intent) => intents.push(intent),
    workspaceQuery: "",
    onWorkspaceQueryChange: () => {},
  });
  const buttons = [];
  const visit = (node) => {
    if (!node || typeof node !== "object") return;
    if (typeof node.props?.onClick === "function") buttons.push(node);
    const children = node.props?.children;
    if (Array.isArray(children)) children.forEach(visit);
    else visit(children);
  };
  visit(tree);

  const start = buttons.find((button) => button.props.children === "start-codex");
  const finish = buttons.find((button) => button.props.children === "Finish agent handoff");
  const rollback = buttons.find((button) => button.props.children === "rollback");
  const switchWorkspace = buttons.find((button) => button.props["data-mesh-current-workspace"] === "/private/mesh/agent-two");
  assert.equal(start.props.disabled, false);
  assert.equal(finish.props.disabled, false);
  assert.equal(rollback.props.disabled, true);
  start.props.onClick();
  finish.props.onClick();
  switchWorkspace.props.onClick();
  assert.deepEqual(intents, [
    { type: "activate", action: "start-codex" },
    { type: "activate", action: "finish-agent" },
    { type: "switch-workspace", path: "/private/mesh/agent-two" },
  ]);
});

test("the Current organism does not render every accepted large-workspace row", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { WorkspaceCurrent } = await loadModule("./src/organisms/workspace-current.tsx");
  const largeProjection = {
    ...projection,
    entries: Array.from({ length: 10_000 }, (_, index) => `path-${index}.txt · file`),
    conditions: Array.from({ length: 2_048 }, (_, index) => `Condition ${index}.`),
    agentActivity: {
      ...projection.agentActivity,
      summary: "10,000 live changes detected.",
      changes: Array.from({ length: 10_000 }, (_, index) => ({
        path: `changed-${index}.txt`,
        kind: "modified-file",
      })),
    },
    workspaces: Array.from({ length: 1_000 }, (_, index) => ({
      path: `/private/mesh/workspace-${index}`,
      label: `Workspace ${String(index).padStart(4, "0")}`,
      state: index === 999 ? "current" : index === 998 ? "agent-assigned" : "available",
      canOpen: index !== 999,
    })),
  };
  const html = renderToStaticMarkup(React.createElement(WorkspaceCurrent, {
    model: largeProjection,
    onIntent: () => {},
  }));

  assert.equal((html.match(/data-mesh-current-workspace=/g) ?? []).length, 100);
  assert.equal((html.match(/data-mesh-live-change=/g) ?? []).length, 500);
  assert.equal((html.match(/data-mesh-materialized-entry=/g) ?? []).length, 500);
  assert.equal((html.match(/data-mesh-current-condition=/g) ?? []).length, 200);
  assert.match(html, /100 of 1,000 matching workspaces shown/);
  assert.match(html, /first 500 of 10,000 live changes/);
  assert.match(html, /first 500 of 10,000 paths/);
  assert.match(html, /first 200 of 2,048 conditions/);
  assert.doesNotMatch(html, /changed-9999\.txt|path-9999\.txt|Condition 2047/);
});
