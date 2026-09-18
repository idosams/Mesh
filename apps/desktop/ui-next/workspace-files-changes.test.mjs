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
  "create-text", "create-folder", "move-entry", "delete-entry", "open-entry", "reveal-entry",
  "open-workspace-folder", "scan-files",
  "load-file", "preserve-edit", "save-private", "save-all-private", "record-structural-change",
];

const workbench = {
  files: {
    workspaceLabel: "Alpha workspace",
    workspaceRoot: "/managed/alpha",
    workspaceState: "current",
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
    files: [
      { value: "report.txt", label: "report.txt" },
      { value: "assets/new-image.png", label: "assets/new-image.png" },
    ],
    selectedFile: "report.txt",
    canSelectFile: true,
    editorKind: "text",
    editorText: "after\n",
    baselineText: "before\n",
    baselineAvailable: true,
    canEditText: true,
    editState: "Working · unsaved edits",
    editVersion: "current abc…",
    scanState: "changes",
    queueSummary: "4 native changes ready",
    queue: [
      { path: "report.txt", detail: "6 bytes", description: "changed tracked file", code: "M", status: "Modified" },
      { path: "old.txt", detail: "choose rename or deletion below", description: "missing tracked file", code: "D", status: "Deleted or moved" },
      { path: "assets/new-image.png", detail: "42 bytes", description: "new native file", code: "A", status: "Added" },
      { path: "tmp/agent.sock", detail: "convert or remove before saving or review", description: "special native entry", code: "?", status: "Unsupported" },
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
  assert.deepEqual(accepted.model.changes.queue[0], {
    path: "report.txt", detail: "6 bytes", description: "changed tracked file", code: "M", status: "Modified",
  });
  const unusualName = module.workspaceFilesChangesEnvelope({
    generation: 9,
    workbench: {
      ...workbench,
      changes: {
        ...workbench.changes,
        queue: [{ path: "docs/a · b.txt", detail: "9 bytes", description: "new native file", code: "A", status: "Added" }],
      },
    },
  }, 8);
  assert.equal(unusualName.model.changes.queue[0].path, "docs/a · b.txt");
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
    () => module.workspaceFilesChangesEnvelope({
      generation: 9,
      workbench: {
        ...workbench,
        changes: { ...workbench.changes, queue: [{ ...workbench.changes.queue[0], status: "Added" }] },
      },
    }, 8),
    /status did not match its code/,
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
        baselineAvailable: false,
        canEditText: false,
      },
    },
  }));
  const cleanChangesHtml = renderToStaticMarkup(React.createElement(WorkspaceChanges, {
    ...props,
    model: {
      ...workbench,
      changes: {
        ...workbench.changes,
        files: [],
        selectedFile: "",
        editorKind: "none",
        editorText: "",
        baselineText: "",
        baselineAvailable: false,
        canEditText: false,
        scanState: "clean",
        queueSummary: "Folder matches private history",
        queue: [],
      },
    },
  }));

  assert.match(filesHtml, /Explorer/);
  assert.match(filesHtml, /Alpha workspace/);
  assert.match(filesHtml, /Go to file/);
  assert.match(filesHtml, />Hide</);
  assert.match(filesHtml, />Collapse</);
  assert.match(filesHtml, />Locate</);
  assert.match(filesHtml, /Plain text/);
  assert.match(filesHtml, /data-mesh-file-type="TXT"/);
  assert.match(filesHtml, /Working tree diff/, "Files must show the selected file's actual inspected content or diff");
  assert.match(filesHtml, /Repository actions/);
  assert.match(filesHtml, /data-mesh-proof="files-explorer"/);
  assert.match(filesHtml, /data-mesh-proof="files-tree"/);
  assert.match(filesHtml, /data-mesh-proof="files-filter"/);
  assert.match(filesHtml, /data-mesh-proof="files-selected-entry" data-mesh-entry-path="report\.txt"/);
  assert.match(filesHtml, />2<\/strong> files/);
  assert.doesNotMatch(filesHtml, /📁|📄/, "the explorer must use consistent vector glyphs rather than platform emoji");
  assert.doesNotMatch(filesHtml, /hero\.png/, "collapsed descendants must not be rendered");
  assert.match(filesHtml, /aria-selected="true"[^>]*tabindex="0"[^>]*data-mesh-work-entry="report\.txt"/);
  assert.equal((filesHtml.match(/role="treeitem"[^>]*tabindex="0"/g) || []).length, 1, "the tree must expose one roving tab stop");
  assert.match(filesHtml, /data-mesh-work-action="open-entry"/);
  assert.match(filesHtml, /data-mesh-work-action="reveal-entry"/);
  assert.match(filesHtml, /data-mesh-work-action="open-workspace-folder"/);
  assert.match(changesHtml, /Working changes/);
  assert.match(changesHtml, /Source control/);
  assert.match(changesHtml, /Show Changes|>Hide</);
  assert.match(changesHtml, /Search working changes/);
  assert.match(changesHtml, /placeholder="Filter changed files…"/);
  assert.match(changesHtml, /<button(?=[^>]*data-mesh-change-filter="all")(?=[^>]*aria-pressed="true")[^>]*>/);
  assert.match(changesHtml, /data-mesh-change-filter="M"/);
  assert.match(changesHtml, /data-mesh-change-filter="A"/);
  assert.match(changesHtml, /data-mesh-change-filter="D"/);
  assert.match(changesHtml, /data-mesh-change-filter="\?"/);
  assert.match(changesHtml, /Workspace root/);
  assert.match(changesHtml, />assets</);
  assert.match(changesHtml, />tmp</);
  assert.match(changesHtml, /data-mesh-change-navigation="previous"/);
  assert.match(changesHtml, /data-mesh-change-navigation="next"/);
  const previousButton = changesHtml.match(/<button[^>]*data-mesh-change-navigation="previous"[^>]*>/)?.[0];
  const nextButton = changesHtml.match(/<button[^>]*data-mesh-change-navigation="next"[^>]*>/)?.[0];
  assert.match(previousButton ?? "", /disabled=""/, "the first visible inspectable change must not navigate backward");
  assert.doesNotMatch(nextButton ?? "", /disabled=""/, "the next visible inspectable change must remain reachable");
  assert.match(changesHtml, /Text workspace view/);
  assert.match(changesHtml, />Inline</);
  assert.match(changesHtml, />Split</);
  assert.match(changesHtml, /Working tree diff/);
  assert.match(changesHtml, /aria-pressed="true"[^>]*>Inline</, "diff review must be the stable default instead of forcing Edit for every file");
  assert.match(changesHtml, /data-mesh-change-code="M"/);
  assert.match(changesHtml, /data-mesh-change-code="D"/);
  assert.match(changesHtml, /role="listbox" aria-label="Working changes"/);
  assert.equal((changesHtml.match(/role="option"/g) || []).length, 4);
  assert.equal((changesHtml.match(/role="option"[^>]*tabindex="0"/g) || []).length, 1, "working changes must expose one roving tab stop");
  assert.match(filesHtml, /data-mesh-explorer-change="M"/, "the file tree must surface source-control truth beside the matching path");
  assert.doesNotMatch(changesHtml, /Choose a native file/);
  assert.match(changesHtml, /data-mesh-native-queue="true"/);
  assert.match(changesHtml, /data-mesh-work-action="scan-files"/);
  assert.match(changesHtml, /data-mesh-work-action="save-all-private"/);
  assert.match(changesHtml, /data-mesh-work-action="record-structural-change"/);
  assert.match(changesHtml, /data-mesh-work-field="missingSource"/);
  assert.match(changesHtml, /data-mesh-work-field="moveTarget"/);
  assert.match(changesHtml, /Missing tracked file/);
  assert.match(changesHtml, /Auto-save safe changes/);
  assert.match(changesHtml, /How Mesh handles working changes/);
  assert.match(changesHtml, /disabled=""[^>]*>save-private|disabled=""/);
  assert.match(emptyEditorHtml, /aria-labelledby="file-editor-heading"/);
  assert.match(emptyEditorHtml, /id="file-editor-heading"[^>]*>No file selected</);
  assert.match(emptyEditorHtml, /Select a working change/);
  assert.match(cleanChangesHtml, /No working changes/);
  assert.match(cleanChangesHtml, /matches the latest private version/);
  assert.doesNotMatch(cleanChangesHtml, /Working tree clean/, "the empty state must explain what was checked instead of implying a Git status");
  assert.equal(
    (emptyEditorHtml.match(/id="file-editor-heading"/g) || []).length,
    1,
    "the empty editor section must retain one resolvable accessible name",
  );
  assert.equal((changesHtml.match(/id="file-editor-heading"/g) || []).length, 1);
  assert.doesNotMatch(`${filesHtml}${changesHtml}`, /__TAURI__|invoke\(/);
});

test("the explorer bounds collapsed rendering and implements deterministic tree navigation", async () => {
  const explorer = await loadModule("./src/models/workspace-explorer.ts");
  assert.deepEqual(explorer.workspaceFilePresentation("notes/readme.md"), { category: "document", label: "Markdown", shortLabel: "MD" });
  assert.deepEqual(explorer.workspaceFilePresentation("data/report.json"), { category: "data", label: "JSON data", shortLabel: "JSON" });
  assert.deepEqual(explorer.workspaceFilePresentation(".DS_Store"), { category: "system", label: "macOS folder metadata", shortLabel: "SYS" });
  assert.deepEqual(explorer.workspaceFilePresentation("archive.custom"), { category: "file", label: "CUSTOM file", shortLabel: "CUSTO" });
  assert.deepEqual(explorer.workspaceFilePresentation("scripts/release.sh"), { category: "code", label: "Shell script", shortLabel: "SH" });
  assert.deepEqual(explorer.workspaceFilePresentation("data/export.csv"), { category: "data", label: "CSV data", shortLabel: "CSV" });
  const boundedText = explorer.workspaceTextPreview(Array.from({ length: 700 }, (_, index) => `line-${index}`).join("\n"));
  assert.equal(boundedText.lines.length, explorer.WORKSPACE_FILE_PREVIEW_LINE_LIMIT);
  assert.equal(boundedText.totalLines, 700);
  assert.equal(boundedText.truncated, true);
  assert.throws(() => explorer.workspaceTextPreview("text", 0), /preview bound was invalid/);
  const entries = [
    { value: "src", label: "src", kind: "folder" },
    ...Array.from({ length: 5_000 }, (_, index) => ({
      value: `src/generated/file-${String(index).padStart(4, "0")}.txt`,
      label: `src/generated/file-${index}.txt`,
      kind: "file",
    })),
    { value: "README.md", label: "README.md", kind: "file" },
  ];
  const tree = explorer.workspaceExplorerTree(entries);
  const collapsed = explorer.visibleWorkspaceExplorerRows(tree, new Set(), "");
  assert.deepEqual(collapsed.map((row) => row.node.path), ["src", "README.md"]);
  const srcOpen = explorer.visibleWorkspaceExplorerRows(tree, new Set(["src"]), "");
  assert.deepEqual(srcOpen.map((row) => row.node.path), ["src", "src/generated", "README.md"]);
  const filtered = explorer.visibleWorkspaceExplorerRows(tree, new Set(), "file-4999");
  assert.deepEqual(filtered.map((row) => row.node.path), [
    "src", "src/generated", "src/generated/file-4999.txt",
  ]);
  const broadSearch = explorer.workspaceExplorerProjection(tree, new Set(), "file-");
  assert.equal(broadSearch.rows.length, explorer.WORKSPACE_EXPLORER_ROW_LIMIT);
  assert.equal(broadSearch.matched, 5_002);
  assert.equal(broadSearch.truncated, true);
  assert.equal(broadSearch.rows.at(-1).node.path, "src/generated/file-0497.txt");
  assert.equal(explorer.workspaceExplorerLocateFilter(tree, "README.md"), "");
  assert.equal(
    explorer.workspaceExplorerLocateFilter(tree, "src/generated/file-4999.txt"),
    "src/generated/file-4999.txt",
  );
  assert.throws(() => explorer.workspaceExplorerProjection(tree, new Set(), "", 0), /row bound was invalid/);

  assert.deepEqual(explorer.workspaceExplorerNavigation(collapsed, "src", "ArrowDown"), {
    focusPath: "README.md", expandPath: null, collapsePath: null,
  });
  assert.deepEqual(explorer.workspaceExplorerNavigation(collapsed, "src", "ArrowRight"), {
    focusPath: "src", expandPath: "src", collapsePath: null,
  });
  assert.deepEqual(explorer.workspaceExplorerNavigation(srcOpen, "src", "ArrowRight"), {
    focusPath: "src/generated", expandPath: null, collapsePath: null,
  });
  assert.deepEqual(explorer.workspaceExplorerNavigation(srcOpen, "src/generated", "ArrowLeft"), {
    focusPath: "src", expandPath: null, collapsePath: null,
  });
  assert.equal(explorer.workspaceExplorerNavigation(collapsed, "README.md", "Home").focusPath, "src");
  assert.equal(explorer.workspaceExplorerNavigation(collapsed, "src", "End").focusPath, "README.md");

  assert.deepEqual(explorer.workspaceExplorerSummary(entries), {
    files: 5_001,
    folders: 2,
    total: 5_003,
  });

  const changes = workbench.changes.queue;
  assert.deepEqual(explorer.workspaceChangeGroups(changes, "", "all").map((group) => ({
    folder: group.folder,
    paths: group.changes.map((change) => change.path),
  })), [
    { folder: "", paths: ["old.txt", "report.txt"] },
    { folder: "assets", paths: ["assets/new-image.png"] },
    { folder: "tmp", paths: ["tmp/agent.sock"] },
  ]);
  assert.deepEqual(
    explorer.workspaceChangeGroups(changes, "image", "A").flatMap((group) => group.changes.map((change) => change.path)),
    ["assets/new-image.png"],
  );
  assert.deepEqual(explorer.workspaceChangeGroups(changes, "tracked", "D").flatMap((group) => group.changes.map((change) => change.path)), ["old.txt"]);
  const largeChanges = Array.from({ length: 10_000 }, (_, index) => ({
    path: `generated/file-${String(index).padStart(5, "0")}.txt`,
    detail: `${index} bytes`,
    description: "generated file",
    code: "M",
    status: "Modified",
  }));
  const boundedChanges = explorer.workspaceChangeProjection(largeChanges, "", "all");
  assert.equal(boundedChanges.matched, 10_000);
  assert.equal(boundedChanges.displayed, explorer.WORKSPACE_CHANGE_ROW_LIMIT);
  assert.equal(boundedChanges.offset, 0);
  assert.equal(boundedChanges.orderedPaths.length, 10_000);
  assert.equal(boundedChanges.groups.flatMap((group) => group.changes).length, 500);
  assert.equal(boundedChanges.truncated, true);
  assert.equal(boundedChanges.groups[0].changes.at(-1).path, "generated/file-00499.txt");
  const selectedBoundary = explorer.workspaceChangeProjection(
    largeChanges,
    "",
    "all",
    explorer.WORKSPACE_CHANGE_ROW_LIMIT,
    "generated/file-00500.txt",
  );
  assert.equal(selectedBoundary.offset, 500);
  assert.equal(selectedBoundary.groups[0].changes[0].path, "generated/file-00500.txt");
  assert.equal(selectedBoundary.groups.at(-1).changes.at(-1).path, "generated/file-00999.txt");
  assert.deepEqual(
    explorer.workspaceChangeNavigation(selectedBoundary.orderedPaths, "generated/file-00499.txt"),
    { previous: "generated/file-00498.txt", next: "generated/file-00500.txt" },
  );
  assert.deepEqual(
    explorer.workspaceChangeNavigation(selectedBoundary.orderedPaths, "generated/file-00500.txt"),
    { previous: "generated/file-00499.txt", next: "generated/file-00501.txt" },
  );
  const firstWindowPaths = boundedChanges.groups.flatMap((group) => group.changes.map((change) => change.path));
  const secondWindowPaths = selectedBoundary.groups.flatMap((group) => group.changes.map((change) => change.path));
  let preservedFocus = explorer.workspaceChangeRovingPath(
    firstWindowPaths,
    "generated/file-00499.txt",
    "generated/file-00499.txt",
  );
  preservedFocus = explorer.workspaceChangeRovingPath(
    secondWindowPaths,
    "generated/file-00500.txt",
    preservedFocus,
  );
  assert.equal(preservedFocus, "generated/file-00500.txt", "a stateful 499 to 500 rerender must restore one visible roving tab stop");
  assert.equal(secondWindowPaths.filter((path) => path === preservedFocus).length, 1);
  const narrowProjection = explorer.workspaceChangeProjection(
    largeChanges,
    "file-00010",
    "all",
    explorer.WORKSPACE_CHANGE_ROW_LIMIT,
    "generated/file-00750.txt",
  );
  const narrowPaths = narrowProjection.groups.flatMap((group) => group.changes.map((change) => change.path));
  preservedFocus = explorer.workspaceChangeRovingPath(
    narrowPaths,
    "generated/file-00750.txt",
    "generated/file-00010.txt",
  );
  const restoredProjection = explorer.workspaceChangeProjection(
    largeChanges,
    "",
    "all",
    explorer.WORKSPACE_CHANGE_ROW_LIMIT,
    "generated/file-00750.txt",
  );
  const restoredPaths = restoredProjection.groups.flatMap((group) => group.changes.map((change) => change.path));
  preservedFocus = explorer.workspaceChangeRovingPath(
    restoredPaths,
    "generated/file-00750.txt",
    preservedFocus,
  );
  assert.equal(preservedFocus, "generated/file-00750.txt", "clearing a filter must reconcile focus into the selected window");
  assert.equal(restoredPaths.filter((path) => path === preservedFocus).length, 1);
  const displacedChanges = [
    { ...largeChanges[0], path: "generated/00000-before.txt" },
    { ...largeChanges[0], path: "generated/00001-before.txt" },
    ...largeChanges,
  ];
  const displacedSelection = explorer.workspaceChangeProjection(
    displacedChanges,
    "",
    "all",
    explorer.WORKSPACE_CHANGE_ROW_LIMIT,
    "generated/file-00499.txt",
  );
  assert.equal(displacedSelection.offset, 500);
  assert.equal(
    displacedSelection.groups.flatMap((group) => group.changes).some((change) => change.path === "generated/file-00499.txt"),
    true,
    "a safe refresh must keep the selected change inside the bounded window",
  );
  assert.throws(() => explorer.workspaceChangeProjection(changes, "", "all", 0), /row bound was invalid/);
  const largeChoices = largeChanges.map((change) => ({ value: change.path, label: change.path }));
  const boundedChoices = explorer.workspaceChoiceProjection(largeChoices, "", "generated/file-09999.txt");
  assert.equal(boundedChoices.items.length, explorer.WORKSPACE_CHOICE_ROW_LIMIT);
  assert.equal(boundedChoices.items.some((choice) => choice.value === "generated/file-09999.txt"), true);
  assert.equal(boundedChoices.matched, 10_000);
  assert.equal(boundedChoices.truncated, true);
  const filteredChoices = explorer.workspaceChoiceProjection(largeChoices, "file-00000", "generated/file-09999.txt");
  assert.deepEqual(filteredChoices.items.map((choice) => choice.value), [
    "generated/file-09999.txt",
    "generated/file-00000.txt",
  ]);
  assert.equal(filteredChoices.matched, 1);
  assert.equal(filteredChoices.retainedSelected, true);
  assert.throws(() => explorer.workspaceChoiceProjection(largeChoices, "", "", 0), /choice-row bound was invalid/);
  assert.deepEqual(explorer.workspaceChangeNavigation(["report.txt", "assets/new-image.png"], "report.txt"), {
    previous: null,
    next: "assets/new-image.png",
  });
  assert.deepEqual(explorer.workspaceChangeNavigation(["report.txt", "assets/new-image.png"], "hidden.txt"), {
    previous: "assets/new-image.png",
    next: "report.txt",
  });
  assert.equal(explorer.workspaceChangeFocusNavigation(["one", "two", "three"], "two", "ArrowUp"), "one");
  assert.equal(explorer.workspaceChangeFocusNavigation(["one", "two", "three"], "two", "ArrowDown"), "three");
  assert.equal(explorer.workspaceChangeFocusNavigation(["one", "two", "three"], "two", "Home"), "one");
  assert.equal(explorer.workspaceChangeFocusNavigation(["one", "two", "three"], "two", "End"), "three");
  assert.equal(explorer.workspaceChangeFocusNavigation([], "", "Home"), null);
});

test("the Files tree and Changes queue never render an accepted 10,000-row projection at once", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { WorkspaceFiles, WorkspaceChanges } = await loadModule("./src/organisms/workspace-files-changes.tsx");
  const paths = Array.from({ length: 10_000 }, (_, index) => `generated/file-${String(index).padStart(5, "0")}.txt`);
  const largeWorkbench = {
    ...workbench,
    files: {
      ...workbench.files,
      entries: paths.map((path) => ({ value: path, label: path, kind: "file" })),
      selectedEntry: paths[0],
    },
    changes: {
      ...workbench.changes,
      files: paths.map((path) => ({ value: path, label: path })),
      selectedFile: paths[0],
      queue: paths.map((path, index) => ({
        path,
        detail: `${index} bytes`,
        description: "generated file",
        code: "M",
        status: "Modified",
      })),
    },
  };
  const props = { model: largeWorkbench, onIntent: () => assert.fail("SSR must not emit an intent") };
  const filesHtml = renderToStaticMarkup(React.createElement(WorkspaceFiles, props));
  const changesHtml = renderToStaticMarkup(React.createElement(WorkspaceChanges, props));
  const boundaryHtml = renderToStaticMarkup(React.createElement(WorkspaceChanges, {
    ...props,
    model: {
      ...largeWorkbench,
      changes: { ...largeWorkbench.changes, selectedFile: paths[499] },
    },
  }));
  const nextAtBoundary = boundaryHtml.match(/<button[^>]*data-mesh-change-navigation="next"[^>]*>/u)?.[0] ?? "";
  const selectedPastCutoffHtml = renderToStaticMarkup(React.createElement(WorkspaceChanges, {
    ...props,
    model: {
      ...largeWorkbench,
      changes: { ...largeWorkbench.changes, selectedFile: paths[500] },
    },
  }));

  assert.equal((filesHtml.match(/role="treeitem"/g) || []).length, 500);
  assert.match(filesHtml, /Showing the first 500 visible paths/);
  assert.equal((changesHtml.match(/role="option"/g) || []).length, 500);
  assert.match(changesHtml, /Showing changes 1–500 of 10000/);
  assert.doesNotMatch(nextAtBoundary, /disabled=/, "Next must remain enabled at the first window boundary");
  assert.equal((selectedPastCutoffHtml.match(/role="option"/g) || []).length, 500);
  assert.equal(
    (selectedPastCutoffHtml.match(/<button(?=[^>]*role="option")(?=[^>]*tabindex="0")[^>]*>/g) || []).length,
    1,
    "the selected-aware window must render exactly one roving tab stop",
  );
  assert.match(selectedPastCutoffHtml, /file-00500\.txt/);
  assert.match(selectedPastCutoffHtml, /Showing changes 501–1000 of 10000/);
  assert.doesNotMatch(selectedPastCutoffHtml, /file-00000\.txt/);
  assert.doesNotMatch(`${filesHtml}${changesHtml}`, /file-09999\.txt/);

  const structuralHtml = renderToStaticMarkup(React.createElement(WorkspaceChanges, {
    ...props,
    model: {
      ...largeWorkbench,
      changes: {
        ...largeWorkbench.changes,
        structural: {
          ...workbench.changes.structural,
          missingSources: paths.map((path) => ({ value: path, label: path })),
          missingSource: paths.at(-1),
          moveTargets: paths.map((path) => ({ value: path, label: `It moved to ${path}` })),
          moveTarget: paths.at(-1),
        },
      },
    },
  }));
  assert.equal((structuralHtml.match(/data-mesh-structural-source=/g) || []).length, 500);
  assert.equal((structuralHtml.match(/data-mesh-structural-target=/g) || []).length, 500);
  assert.match(structuralHtml, /500 of 10,000 matching missing files shown/);
  assert.match(structuralHtml, /500 of 10,000 matching possible destinations shown/);
  assert.match(structuralHtml, /file-09999\.txt/);
});

test("working text comparisons are exact, bounded, hunked, and split into aligned review rows", async () => {
  const diff = await loadModule("./src/models/workspace-text-diff.ts");
  const middle = Array.from({ length: 8 }, (_, index) => `keep-${index + 1}`);
  const comparison = diff.workspaceTextDiff(
    ["header", "old-one", ...middle, "old-two", "footer"].join("\n"),
    ["header", "new-one", ...middle, "new-two", "footer"].join("\n"),
  );

  assert.equal(comparison.kind, "ready");
  assert.equal(comparison.additions, 2);
  assert.equal(comparison.deletions, 2);
  assert.equal(comparison.hunks.length, 2, "distant edits must become separate review hunks");
  assert.equal(comparison.hunks.flatMap((hunk) => hunk.lines).some((line) => line.text === "keep-4"), false);
  assert.equal(comparison.hunks.flatMap((hunk) => hunk.lines).some((line) => line.text === "keep-5"), false);
  assert.equal(Object.isFrozen(comparison), true);
  assert.equal(Object.isFrozen(comparison.hunks), true);
  assert.equal(Object.isFrozen(comparison.hunks[0].lines), true);

  const finalNewline = diff.workspaceTextDiff("same\nlast\n", "same\nlast");
  const changedEnding = finalNewline.hunks[0].lines.filter((line) => line.kind !== "context");
  assert.deepEqual(changedEnding.map(({ kind, ending }) => ({ kind, ending })), [
    { kind: "removed", ending: "lf" },
    { kind: "added", ending: "none" },
  ]);
  const aligned = diff.workspaceSplitDiffRows(finalNewline.hunks[0].lines);
  assert.equal(aligned.at(-1).before.kind, "removed");
  assert.equal(aligned.at(-1).after.kind, "added");

  const newFile = diff.workspaceTextDiff("", "first\nsecond\n");
  assert.equal(newFile.kind, "ready");
  assert.deepEqual({
    beforeStart: newFile.hunks[0].beforeStart,
    beforeCount: newFile.hunks[0].beforeCount,
    afterStart: newFile.hunks[0].afterStart,
    afterCount: newFile.hunks[0].afterCount,
  }, { beforeStart: 0, beforeCount: 0, afterStart: 1, afterCount: 2 });
  assert.deepEqual(newFile.hunks[0].lines.map((line) => line.kind), ["added", "added"]);

  const tooManyBefore = Array.from({ length: 2_049 }, (_, index) => `before-${index}`).join("\n");
  const tooManyAfter = Array.from({ length: 2_049 }, (_, index) => `after-${index}`).join("\n");
  const unavailable = diff.workspaceTextDiff(tooManyBefore, tooManyAfter);
  assert.equal(unavailable.kind, "unavailable");
  assert.match(unavailable.reason, /4,096-line/);
  assert.equal(unavailable.hunks.length, 0, "an over-limit comparison must not imply a partial diff");

  const identicalLargeText = "x".repeat(diff.WORKSPACE_TEXT_DIFF_MAX_CHARACTERS + 1);
  assert.equal(diff.workspaceTextDiff(identicalLargeText, identicalLargeText).kind, "unchanged");
});

test("the working text comparison renders IDE-style hunks with audible line meaning", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { TextComparison } = await loadModule("./src/organisms/workspace-files-changes.tsx");
  const html = renderToStaticMarkup(React.createElement(TextComparison, {
    before: "earlier value\n",
    after: "current value\n",
    split: false,
  }));

  assert.match(html, /aria-label="Inline text comparison"/);
  assert.match(html, /Working tree diff/);
  assert.match(html, /\+1 addition/);
  assert.match(html, /−1 deletion/);
  assert.match(html, /data-mesh-work-diff-line="removed"/);
  assert.match(html, /data-mesh-work-diff-line="added"/);
  assert.match(html, /Removed\. Earlier line 1\. No current line/);
  assert.match(html, /Added\. No earlier line\. Current line 1/);
  assert.match(html, /@@ -1,1 \+1,1 @@/);
  assert.ok((html.match(/aria-hidden="true"/g) || []).length >= 6, "line numbers and change symbols must stay presentational");
  assert.match(html, />−<\/span>/);
  assert.match(html, />\+<\/span>/);

  const splitHtml = renderToStaticMarkup(React.createElement(TextComparison, {
    before: "earlier value\n",
    after: "current value\n",
    split: true,
  }));
  assert.match(splitHtml, /aria-label="Split text comparison"/);
  assert.match(splitHtml, />Saved version</);
  assert.match(splitHtml, />Working copy</);
  assert.match(splitHtml, /Earlier line 1\. Removed/);
  assert.match(splitHtml, /Current line 1\. Added/);

  const unchangedHtml = renderToStaticMarkup(React.createElement(TextComparison, {
    before: "same\n",
    after: "same\n",
    split: false,
  }));
  assert.match(unchangedHtml, /No text changes/);

  const unavailableHtml = renderToStaticMarkup(React.createElement(TextComparison, {
    before: "a".repeat(150_000),
    after: "b".repeat(150_000),
    split: false,
  }));
  assert.match(unavailableHtml, /Comparison is too large to display/);
  assert.match(unavailableHtml, /exact file remains available in Edit/);

  const unavailableBaselineHtml = renderToStaticMarkup(React.createElement(TextComparison, {
    before: "",
    after: "small working text\n",
    split: false,
    baselineAvailable: false,
  }));
  assert.match(unavailableBaselineHtml, /Saved baseline cannot be displayed/);
  assert.match(unavailableBaselineHtml, /exact working file remains available in Edit/);
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
  assert.match(organism, /editablePaths\.has\(path\)/);
  assert.match(organism, /loadFile\?\.enabled/);
  assert.match(organism, /action: "load-file", field: "selectedFile", value: path/);
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
