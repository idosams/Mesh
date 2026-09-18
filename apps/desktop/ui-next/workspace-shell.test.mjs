import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";
import { build } from "esbuild";

const root = new URL(".", import.meta.url).pathname;

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

const loadShell = () => loadModule("./src/layouts/workspace-shell.tsx");

test("the shell never invents local-service readiness", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { WorkspaceShell } = await loadShell();

  const html = renderToStaticMarkup(
    React.createElement(WorkspaceShell, null, React.createElement("p", null, "Workspace")),
  );

  assert.match(html, /role="status"/);
  assert.match(html, /aria-live="polite"/);
  assert.match(html, /data-state="checking"/);
  assert.match(html, />Checking local service</);
  assert.doesNotMatch(html, /Local service ready/);
});

test("the gallery labels its disconnected state instead of impersonating a workspace", async () => {
  const { readFile } = await import("node:fs/promises");
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { WorkspaceShell } = await loadShell();
  const gallery = await readFile(`${root}src/pages/gallery-page.tsx`, "utf8");

  assert.match(gallery, /contextLabel="Interface preview"/);
  assert.match(gallery, /status=\{\{ state: "preview", label: "Preview only" \}\}/);

  const html = renderToStaticMarkup(
    React.createElement(
      WorkspaceShell,
      {
        contextLabel: "Interface preview",
        status: { state: "preview", label: "Preview only" },
      },
      React.createElement("p", null, "Gallery"),
    ),
  );

  assert.match(html, />Interface preview</);
  assert.match(html, /data-state="preview"/);
  assert.match(html, />Preview only</);
  assert.doesNotMatch(html, />Private workspace</);
});

test("the shell header can grow and wrap at narrow widths", async () => {
  const { readFile } = await import("node:fs/promises");
  const source = await readFile(`${root}src/layouts/workspace-shell.tsx`, "utf8");

  assert.match(source, /min-h-16/);
  assert.match(source, /flex-wrap/);
  assert.match(source, /py-3/);
  assert.doesNotMatch(source, /flex h-16 max-w/);
});

test("the production navigation renders keyboard page controls only for an open workspace", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { WorkspaceHeader } = await loadModule("./src/organisms/workspace-chrome.tsx");
  const { ProductionNavigation } = await loadModule("./src/organisms/production-navigation.tsx");
  const model = {
    serviceState: "attention",
    serviceLabel: "Local service needs attention",
    workspaceReady: true,
    nativeChangeCount: 3,
  };
  const header = renderToStaticMarkup(React.createElement(WorkspaceHeader, { model }));
  const navigation = renderToStaticMarkup(React.createElement(ProductionNavigation, {
    activePage: "current",
    workspaceReady: true,
    nativeChangeCount: 3,
    onNavigate: () => {},
  }));

  assert.match(header, /role="status"/);
  assert.match(header, /data-state="attention"/);
  assert.match(navigation, /<nav aria-label="Primary pages"/);
  assert.equal((navigation.match(/<button/g) || []).length, 9);
  assert.match(navigation, /aria-current="page"[^>]*>Current/);
  assert.match(navigation, /<button[^>]*class="[^"]* border-primary text-foreground[^"]*"[^>]*aria-current="page"[^>]*data-state="active"/);
  assert.match(navigation, />Review</);
  assert.match(navigation, /aria-label="3 folder changes"/);
  const emptyNavigation = renderToStaticMarkup(React.createElement(ProductionNavigation, {
    activePage: "workspaces",
    workspaceReady: false,
    nativeChangeCount: 0,
    onNavigate: () => {},
  }));
  assert.equal((emptyNavigation.match(/<button/g) || []).length, 2);
  assert.doesNotMatch(emptyNavigation, />Current</);
});

test("the workspace entry page keeps one primary action and responsive keyboard-native controls", async () => {
  const { readFile } = await import("node:fs/promises");
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { WorkspaceEntryPage } = await loadModule("./src/pages/workspace-entry-page.tsx");
  const { WorkspaceEntry } = await loadModule("./src/organisms/workspace-entry.tsx");
  const model = {
    mode: "empty",
    eyebrow: "LOCAL WORKSPACE",
    title: "Your folder, with a private history.",
    description: "Bring in an ordinary project folder. The original stays untouched.",
    disclosureLabel: "Return to a recent Mesh workspace",
    disclosureOpen: true,
    chooseLabel: "Choose a folder",
    canChoose: true,
    canChooseManaged: true,
    retryLabel: 'Refresh',
    canRetry: true,
    openPath: "",
    canEditPath: true,
    canOpenPath: false,
    recents: [{ path: "/private/payroll.mesh", label: "Payroll · Managed workspace", state: "available" }],
    selectedRecentPath: "/private/payroll.mesh",
    canSelectRecent: true,
    recentHint: "Forgetting removes only this navigation shortcut.",
    recentOpenLabel: "Switch workspace",
    canOpenRecent: true,
    canForgetRecent: true,
    forgetRecentTitle: "",
  };
  const html = renderToStaticMarkup(React.createElement(WorkspaceEntryPage, { model, onIntent: () => {} }));
  const organism = await readFile(`${root}src/organisms/workspace-entry.tsx`, "utf8");
  const recentPicker = await readFile(`${root}src/molecules/recent-workspace-picker.tsx`, "utf8");

  assert.match(html, /<section aria-labelledby="workspace-entry-title"/);
  assert.match(html, /<h1 id="workspace-entry-title"/);
  assert.equal((html.match(/bg-primary text-primary-foreground/g) || []).length, 1);
  assert.match(html, /<details open=""/);
  assert.match(html, /<summary class="[^"]*min-h-11[^"]*focus-visible:ring-2/);
  assert.match(html, /<form class="space-y-2">/);
  assert.match(html, /id="workspace-entry-path"/);
  assert.match(html, /id="workspace-entry-recent"/);
  assert.match(html, /aria-describedby="workspace-entry-recent-hint"/);
  assert.match(html, /flex flex-col gap-2 sm:flex-row/);
  assert.match(
    html,
    /id="workspace-entry-recent"[\s\S]*<div class="grid grid-cols-1 gap-2 sm:grid-cols-2">[\s\S]*Switch workspace[\s\S]*Forget from list/,
    "recent selection and its two actions must not compete for one narrow laptop row",
  );
  assert.match(html, /Your original folder stays untouched\./);
  assert.match(organism, /onToggle=\{\(event\) => onIntent\(\{ type: "set-disclosure", open: event\.currentTarget\.open \}\)\}/);
  assert.doesNotMatch(recentPicker, /key=\{selectedPath\}/);
  assert.match(recentPicker, /value=\{selectedPath\}/);
  assert.match(recentPicker, /ref=\{\(element\) => \{ recentSelect = element; \}\}/);
  assert.equal(
    (recentPicker.match(/path: currentSelectedPath\(\)/g) || []).length,
    2,
    "Open and Forget must emit the live selected option while its projection commit is delayed",
  );
  const intents = [];
  const tree = WorkspaceEntry({ model, onIntent: (intent) => intents.push(intent) });
  const findElement = (value, predicate) => {
    if (Array.isArray(value)) {
      for (const item of value) {
        const found = findElement(item, predicate);
        if (found) return found;
      }
      return null;
    }
    if (!value || typeof value !== "object") return null;
    if (predicate(value)) return value;
    return findElement(value.props?.children, predicate);
  };
  const pathInput = findElement(tree, (element) => element.props?.id === "workspace-entry-path");
  const retryButton = findElement(tree, (element) => element.props?.children === "Refresh"
    && typeof element.props?.onClick === "function");
  assert.ok(pathInput, "the managed path input was absent from the component tree");
  assert.ok(retryButton, "the workspace-independent Refresh action was absent from the component tree");
  retryButton.props.onClick();
  pathInput.props.onChange({ currentTarget: { value: "/private/live.mesh" } });
  let prevented = false;
  pathInput.props.onKeyDown({
    key: "Enter",
    nativeEvent: { isComposing: false },
    currentTarget: { value: "/private/live.mesh" },
    preventDefault: () => { prevented = true; },
  });
  assert.equal(prevented, true);
  assert.deepEqual(intents, [
    { type: "retry" },
    { type: "update-managed-path", path: "/private/live.mesh" },
    { type: "open-managed-path", path: "/private/live.mesh" },
  ]);
});

test("the Restore panel explains document changes with responsive keyboard-native controls", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { WorkspaceRestore } = await loadModule("./src/organisms/workspace-restore.tsx");
  const fileNames = [
    ["notes.txt", "Text"],
    ["policy.pdf", "PDF"],
    ["contract.docx", "Word"],
    ["planning.pptx", "PowerPoint"],
    ["forecast.xlsx", "Excel"],
  ];
  const model = {
    files: fileNames.map(([path, format], index) => ({ id: `object-${index}`, path, label: `${path} · 2 saved versions`, format })),
    selectedFileId: "object-4",
    versions: [{ id: "version-1", label: "Saved version · version-1" }],
    selectedVersionId: "version-1",
    canSelectFile: true,
    canSelectVersion: true,
    canPreview: true,
    canApply: true,
    canUndo: true,
    hint: "Choose an earlier version to preview its exact retained bytes.",
    preview: {
      filePath: "forecast.xlsx",
      format: "Excel",
      currentVersion: "current-version",
      targetVersion: "version-1",
      change: "Replace this file in the working folder with exact retained bytes.",
      historyNote: "Private history stays unchanged until you choose Save privately.",
      undoNote: "Available immediately after this restore.",
    },
    undoLabel: "Undo last restore",
  };
  const html = renderToStaticMarkup(React.createElement(WorkspaceRestore, { model, onIntent: () => {} }));

  assert.match(html, /aria-label="Restore an earlier file version"/);
  assert.match(html, /Working copy only/);
  assert.match(html, /Private history is not rewritten/);
  assert.match(html, /id="restore-next-file"/);
  assert.match(html, /id="restore-next-version"/);
  assert.match(html, /aria-describedby="restore-next-hint"/);
  assert.match(html, /role="status" aria-live="polite" aria-atomic="true"/);
  assert.match(html, /Ready to restore/);
  assert.doesNotMatch(html, /Restore is currently unavailable/);
  for (const [path, format] of fileNames) {
    assert.match(html, new RegExp(path.replace('.', '\\.')));
    const selectedHtml = renderToStaticMarkup(React.createElement(WorkspaceRestore, {
      model: { ...model, selectedFileId: `object-${fileNames.findIndex(([candidate]) => candidate === path)}` },
      onIntent: () => {},
    }));
    assert.match(selectedHtml, new RegExp(`>${format}<`));
  }
  assert.match(html, /flex flex-col gap-3[^>]*sm:flex-row/);
  assert.match(html, /Restore in working copy/);
  assert.match(html, /Undo last restore/);
});

test("Restore keeps large retained-file and version choices bounded and selected", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const restoreModule = await loadModule("./src/models/workspace-restore.ts");
  const { WorkspaceRestore } = await loadModule("./src/organisms/workspace-restore.tsx");
  const files = Array.from({ length: 4_096 }, (_, index) => ({
    id: `object-${String(index).padStart(4, "0")}`,
    path: `archive/file-${String(index).padStart(4, "0")}.txt`,
    label: `archive/file-${String(index).padStart(4, "0")}.txt`,
    format: "Text",
  }));
  const versions = Array.from({ length: 4_096 }, (_, index) => ({
    id: `version-${String(index).padStart(4, "0")}`,
    label: `Saved version ${String(index).padStart(4, "0")}`,
  }));
  const selectedFileId = files.at(-1).id;
  const selectedVersionId = versions.at(-1).id;
  assert.equal(restoreModule.workspaceRestoreFileProjection(files, "", selectedFileId).items.length, 500);
  assert.equal(restoreModule.workspaceRestoreVersionProjection(versions, "", selectedVersionId).items.length, 500);

  const html = renderToStaticMarkup(React.createElement(WorkspaceRestore, {
    model: {
      files,
      selectedFileId,
      versions,
      selectedVersionId,
      canSelectFile: true,
      canSelectVersion: true,
      canPreview: true,
      canApply: false,
      canUndo: false,
      hint: "Choose an earlier version.",
      preview: null,
      undoLabel: "Undo last restore",
    },
    onIntent: () => assert.fail("SSR must not emit an intent"),
  }));
  assert.equal((html.match(/data-mesh-restore-file=/g) || []).length, 500);
  assert.equal((html.match(/data-mesh-restore-version=/g) || []).length, 500);
  assert.match(html, /500 of 4,096 matching retained files shown/);
  assert.match(html, /500 of 4,096 matching saved versions shown/);
  assert.match(html, /file-4095\.txt/);
  assert.match(html, /version-4095/);
  assert.doesNotMatch(html, /file-0000\.txt|version-0000/);
});

test("a verified Restore preview never claims it is ready while applying is blocked", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { WorkspaceRestore } = await loadModule("./src/organisms/workspace-restore.tsx");
  const html = renderToStaticMarkup(React.createElement(WorkspaceRestore, {
    model: {
      files: [{ id: "object-1", path: "forecast.xlsx", label: "forecast.xlsx · 2 saved versions", format: "Excel" }],
      selectedFileId: "object-1",
      versions: [{ id: "version-1", label: "Saved version · version-1" }],
      selectedVersionId: "version-1",
      canSelectFile: true,
      canSelectVersion: true,
      canPreview: true,
      canApply: false,
      canUndo: false,
      hint: "This file has 2 immutable saved versions.",
      preview: {
        filePath: "forecast.xlsx",
        format: "Excel",
        currentVersion: "current-version",
        targetVersion: "version-1",
        change: "Replace this file in the working folder with exact retained bytes.",
        historyNote: "Private history stays unchanged until you choose Save privately.",
        undoNote: "Available immediately after this restore.",
      },
      undoLabel: "Undo last restore",
    },
    onIntent: () => {},
  }));

  assert.doesNotMatch(html, /Ready to restore/);
  assert.match(html, /Restore is currently unavailable/);
});
