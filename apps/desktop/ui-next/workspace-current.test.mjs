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
  assert.deepEqual(module.workspaceCurrentIntent({ type: "activate", action: "finish-agent" }), {
    type: "activate",
    action: "finish-agent",
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
  assert.match(html, /Conditions and unavailable controls \(1\)/);
  assert.match(html, />copy-diagnostics</);
  assert.match(html, />rollback</);
  assert.match(html, /disabled=""[^>]*>rollback|disabled=""/);
  assert.doesNotMatch(html, /__TAURI__|invoke\(/);
  assert.deepEqual(intents, []);
});

test("the Current organism emits exact action intents without a legacy control proxy", async () => {
  const { WorkspaceCurrent } = await loadModule("./src/organisms/workspace-current.tsx");
  const intents = [];
  const tree = WorkspaceCurrent({ model: projection, onIntent: (intent) => intents.push(intent) });
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
  assert.equal(start.props.disabled, false);
  assert.equal(finish.props.disabled, false);
  assert.equal(rollback.props.disabled, true);
  start.props.onClick();
  finish.props.onClick();
  assert.deepEqual(intents, [
    { type: "activate", action: "start-codex" },
    { type: "activate", action: "finish-agent" },
  ]);
});
