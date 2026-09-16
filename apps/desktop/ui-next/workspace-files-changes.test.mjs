import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import test from "node:test";
import { build } from "esbuild";

async function loadModule(path) {
  const result = await build({
    entryPoints: [new URL(path, import.meta.url).pathname],
    bundle: true,
    format: "cjs",
    platform: "node",
    external: ["react", "react-dom", "react/jsx-runtime"],
    write: false,
  });
  const require = createRequire(import.meta.url);
  const module = { exports: {} };
  Function("require", "module", "exports", result.outputFiles[0].text)(require, module, module.exports);
  return module.exports;
}

const actionIds = [
  "create-text", "create-folder", "move-entry", "delete-entry", "scan-files",
  "load-file", "preserve-edit", "save-private", "save-all-private", "record-structural-change",
];

const workbench = {
  files: {
    entries: [
      { value: "assets", label: "assets", kind: "folder" },
      { value: "assets/images/hero.png", label: "assets/images/hero.png", kind: "file" },
      { value: "report.txt", label: "report.txt", kind: "file" },
    ],
    newPath: "notes/alpha.txt",
    selectedEntry: "report.txt",
    movePath: "reports/final.txt",
    canEditNewPath: true,
    canSelectEntry: true,
    canEditMovePath: true,
    status: "3 materialized entries available to manage.",
  },
  changes: {
    files: [{ value: "report.txt", label: "report.txt" }],
    selectedFile: "report.txt",
    canSelectFile: true,
    editorKind: "text",
    editorText: "after\n",
    baselineText: "before\n",
    canEditText: true,
    editState: "Working · unsaved edits",
    editVersion: "current abc…",
    queueSummary: "2 native changes ready",
    queue: [
      "report.txt · changed tracked file · 6 bytes",
      "old.txt · missing tracked file · choose rename or deletion below",
    ],
    autoSaveChecked: false,
    autoSaveEnabled: true,
    autoSaveHint: "Review-first mode is active.",
    structural: {
      missingSources: [{ value: "old.txt", label: "old.txt" }],
      missingSource: "old.txt",
      moveTargets: [{ value: "new.txt", label: "It moved to new.txt · exact saved bytes" }],
      moveTarget: "",
      canChoose: true,
      hint: "Mesh never guesses file identity.",
    },
  },
  actions: actionIds.map((id) => ({ id, label: id, enabled: id !== "save-private" })),
};

test("the files and changes projection is closed, bounded, and generation-bound", async () => {
  const module = await loadModule("./src/models/workspace-files-changes.ts");
  const accepted = module.workspaceFilesChangesEnvelope({ generation: 8, workbench }, 7);

  assert.equal(accepted.model.changes.editorText, "after\n");
  assert.equal(accepted.model.files.entries[1].kind, "file");
  assert.equal(Object.isFrozen(accepted.model), true);
  assert.equal(Object.isFrozen(accepted.model.changes.queue), true);
  assert.equal(Object.isFrozen(accepted.model.actions), true);
  assert.throws(
    () => module.workspaceFilesChangesEnvelope({ generation: 8, workbench }, 8),
    /stale or invalid/,
  );
  assert.throws(
    () => module.workspaceFilesChangesEnvelope({
      generation: 9,
      workbench: { ...workbench, files: { ...workbench.files, newPath: "safe\u202eexe" } },
    }, 8),
    /unsafe or unbounded/,
  );
  assert.throws(
    () => module.workspaceFilesChangesEnvelope({
      generation: 9,
      workbench: { ...workbench, actions: workbench.actions.slice(1) },
    }, 8),
    /omitted an established control/,
  );
  assert.throws(
    () => module.workspaceFilesChangesIntent({ type: "set-field", field: "newPath", value: "x".repeat(4_097) }),
    /unsafe or unbounded/,
  );
  assert.throws(
    () => module.workspaceFilesChangesIntent({ type: "activate", action: "save-private", force: true }),
    /unrecognized or missing fields/,
  );
  assert.deepEqual(
    module.workspaceFilesChangesIntent({ type: "set-auto-save", checked: true }),
    { type: "set-auto-save", checked: true },
  );
  assert.throws(
    () => module.workspaceFilesChangesIntent({ type: "set-auto-save", checked: "true" }),
    /automatic save choice was not boolean/,
  );
  assert.deepEqual(
    module.workspaceFilesChangesIntent({
      type: "activate",
      action: "record-structural-change",
      missingSource: "old.txt",
      moveTarget: "new.txt",
    }),
    {
      type: "activate",
      action: "record-structural-change",
      missingSource: "old.txt",
      moveTarget: "new.txt",
    },
  );
  assert.throws(
    () => module.workspaceFilesChangesIntent({ type: "activate", action: "record-structural-change" }),
    /unrecognized or missing fields/,
  );
  assert.deepEqual(
    module.workspaceFilesChangesIntent({
      type: "activate",
      action: "preserve-edit",
      field: "editorText",
      value: "latest draft\n",
    }),
    { type: "activate", action: "preserve-edit", field: "editorText", value: "latest draft\n" },
  );
  assert.deepEqual(
    module.workspaceFilesChangesIntent({
      type: "activate",
      action: "load-file",
      field: "selectedFile",
      value: "report.txt",
    }),
    { type: "activate", action: "load-file", field: "selectedFile", value: "report.txt" },
    "the still-mounted workbench must carry the exact live file into an immediate read-only load",
  );
  assert.throws(
    () => module.workspaceFilesChangesIntent({
      type: "activate",
      action: "preserve-edit",
      field: "newPath",
      value: "forged.txt",
    }),
    /field echo was not recognized/,
  );
});

test("the component workbench exposes text review, native queue truth, and source-owned controls", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { WorkspaceFiles, WorkspaceChanges } = await loadModule("./src/organisms/workspace-files-changes.tsx");
  const props = { model: workbench, onIntent: () => assert.fail("SSR must not emit an intent") };
  const filesHtml = renderToStaticMarkup(React.createElement(WorkspaceFiles, props));
  const changesHtml = renderToStaticMarkup(React.createElement(WorkspaceChanges, props));
  const emptyEditorHtml = renderToStaticMarkup(React.createElement(WorkspaceChanges, {
    ...props,
    model: {
      ...workbench,
      changes: {
        ...workbench.changes,
        selectedFile: "",
        editorKind: "none",
        editorText: "",
        baselineText: "",
        canEditText: false,
      },
    },
  }));

  assert.match(filesHtml, /Create and organize workspace entries/);
  assert.match(filesHtml, /Workspace tree/);
  assert.match(filesHtml, /assets\/images/);
  assert.match(filesHtml, /hero\.png/);
  assert.match(filesHtml, /aria-current="true"[^>]*data-mesh-work-entry="report\.txt"/);
  assert.match(filesHtml, /Relative path/);
  assert.match(changesHtml, /Text workspace view/);
  assert.match(changesHtml, />Inline</);
  assert.match(changesHtml, />Split</);
  assert.match(changesHtml, /Managed text file contents/);
  assert.match(
    changesHtml,
    /<label class="grid min-w-0 basis-full[^"]*sm:min-w-\[16rem\][^"]*sm:flex-1">File/,
    "the file selector must fit the narrow workbench before restoring its laptop minimum",
  );
  assert.match(changesHtml, /<select class="min-h-11 min-w-0 w-full/);
  assert.match(changesHtml, /Native change queue/);
  assert.match(changesHtml, /data-mesh-native-queue="true"/);
  assert.match(changesHtml, /data-mesh-work-action="scan-files"/);
  assert.match(changesHtml, /data-mesh-work-action="save-all-private"/);
  assert.match(changesHtml, /data-mesh-work-action="record-structural-change"/);
  assert.match(changesHtml, /data-mesh-work-field="missingSource"/);
  assert.match(changesHtml, /data-mesh-work-field="moveTarget"/);
  assert.match(changesHtml, /missing tracked file/);
  assert.match(changesHtml, /Automatically save safe native edits privately/);
  assert.match(changesHtml, /disabled=""[^>]*>save-private|disabled=""/);
  assert.match(emptyEditorHtml, /aria-labelledby="file-editor-heading"/);
  assert.match(emptyEditorHtml, /id="file-editor-heading"[^>]*>File editor and comparison</);
  assert.equal(
    (emptyEditorHtml.match(/id="file-editor-heading"/g) || []).length,
    1,
    "the empty editor section must retain one resolvable accessible name",
  );
  assert.equal((changesHtml.match(/id="file-editor-heading"/g) || []).length, 1);
  assert.doesNotMatch(`${filesHtml}${changesHtml}`, /__TAURI__|invoke\(/);
});

test("the inline text comparison names both sides independently of visual diff symbols", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { TextComparison } = await loadModule("./src/organisms/workspace-files-changes.tsx");
  const html = renderToStaticMarkup(React.createElement(TextComparison, {
    before: "earlier value\n",
    after: "current value\n",
    split: false,
  }));

  assert.match(html, /<span class="sr-only">Before inspected edit: <\/span>/);
  assert.match(html, /<span class="sr-only">Current draft: <\/span>/);
  assert.equal((html.match(/aria-hidden="true"/g) || []).length, 2);
  assert.match(html, /Before inspected edit: <\/span><span[^>]*aria-hidden="true"[^>]*>− <\/span>earlier value/);
  assert.match(html, /Current draft: <\/span><span[^>]*aria-hidden="true"[^>]*>\+ <\/span>current value/);

  const splitHtml = renderToStaticMarkup(React.createElement(TextComparison, {
    before: "earlier value\n",
    after: "current value\n",
    split: true,
  }));
  assert.match(splitHtml, /<section[^>]*aria-label="Before inspected edit"/);
  assert.match(splitHtml, /<section[^>]*aria-label="Current draft"/);
});

test("production keeps one visible workbench across exact dual-surface commits", () => {
  const island = readFileSync(new URL("./src/island.tsx", import.meta.url), "utf8");
  const organism = readFileSync(new URL("./src/organisms/workspace-files-changes.tsx", import.meta.url), "utf8");
  const html = readFileSync(new URL("../ui/index.html", import.meta.url), "utf8");
  const coordinator = readFileSync(new URL("../ui/app.js", import.meta.url), "utf8");

  assert.match(html, /id="workspace-files-next"[^>]*class="hidden"/);
  assert.doesNotMatch(html, /id="workspace-files-current"/);
  assert.match(html, /id="workspace-changes-next"[^>]*class="hidden"/);
  assert.doesNotMatch(html, /id="workspace-changes-current"|id="editor-card"|id="auto-save-native(?:-hint)?"/);
  assert.match(island, /liveness\.begin\(exactGeneration, \["files", "changes"\]\)/);
  assert.match(island, /liveness\.commit\(exactGeneration, surface\)/);
  assert.match(island, /mesh:workspace-files-changes-mounted/);
  assert.match(organism, /field: "editorText", value: editorRef\.current\?\.value/);
  assert.match(organism, /field: "selectedFile", value: selectedFileRef\.current\?\.value/);
  assert.match(organism, /value=\{selectedFileDraft\}/);
  assert.match(
    coordinator,
    /const exactGeneration = detail\?\.generation === workspaceWorkNextPending\.generation[\s\S]*detail\.generation === workspaceWorkNextMounted\.generation/,
  );
  assert.doesNotMatch(coordinator, /\$\('workspace-changes-current'\)|\$\('auto-save-native(?:-hint)?'\)/);
  assert.match(
    coordinator,
    /projectionHasVisibleContinuity\(\s*workspaceWorkNextPending,\s*workspaceWorkNextMounted,?\s*\)/,
  );
  assert.match(coordinator, /workspaceWorkNextPending\.interactionGeneration === workspaceWorkNextMounted\.generation/);
  assert.match(coordinator, /workspaceWorkNextActions\.has\(kind\)/);
});
