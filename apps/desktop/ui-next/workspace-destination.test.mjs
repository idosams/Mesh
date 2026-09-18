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

const actionIds = ["choose-destination", "preview-single", "confirm-single", "preview-all", "confirm-batch"];
const destination = {
  files: [
    { value: "reports/forecast.xlsx", label: "reports/forecast.xlsx" },
    { value: "policy.pdf", label: "policy.pdf" },
  ],
  selectedFile: "reports/forecast.xlsx",
  destination: "/Users/finance/Forecast",
  chooserRevision: 4,
  canSelectFile: true,
  canEditDestination: true,
  hint: "Remembered destination folder. Preview the exact plan before updating anything.",
  plan: {
    state: "ready",
    text: "Update 2 saved files\nDestination folder: /Users/finance/Forecast\n\nReplace  reports/forecast.xlsx\nCreate   policy.pdf",
  },
  actions: actionIds.map((id) => ({
    id,
    label: ({
      "choose-destination": "Choose folder",
      "preview-single": "Preview one file",
      "confirm-single": "Replace existing file",
      "preview-all": "Preview saved workspace",
      "confirm-batch": "Update 2 changed files",
    })[id],
    enabled: id !== "confirm-single",
  })),
};

test("the destination projection is closed, bounded, and generation-bound", async () => {
  const module = await loadModule("./src/models/workspace-destination.ts");
  const accepted = module.workspaceDestinationEnvelope({ generation: 8, destination }, 7);

  assert.equal(accepted.model.plan.state, "ready");
  assert.equal(Object.isFrozen(accepted.model), true);
  assert.equal(Object.isFrozen(accepted.model.files), true);
  assert.equal(Object.isFrozen(accepted.model.actions), true);
  assert.throws(
    () => module.workspaceDestinationEnvelope({ generation: 8, destination }, 8),
    /stale or invalid/,
  );
  assert.throws(
    () => module.workspaceDestinationEnvelope({
      generation: 9,
      destination: { ...destination, destination: "/safe\u202eexe" },
    }, 8),
    /unsafe or unbounded/,
  );
  assert.throws(
    () => module.workspaceDestinationEnvelope({
      generation: 9,
      destination: { ...destination, chooserRevision: Number.MAX_SAFE_INTEGER + 1 },
    }, 8),
    /chooser revision was invalid/,
  );
  assert.throws(
    () => module.workspaceDestinationIntent({ type: "set-field", field: "destination", value: "/safe\nspoof" }),
    /unsafe or unbounded/,
  );
  assert.throws(
    () => module.workspaceDestinationEnvelope({
      generation: 9,
      destination: { ...destination, actions: destination.actions.slice(1) },
    }, 8),
    /omitted an established control/,
  );
  assert.throws(
    () => module.workspaceDestinationEnvelope({
      generation: 9,
      destination: { ...destination, plan: { ...destination.plan, text: "x".repeat(262_145) } },
    }, 8),
    /unsafe or unbounded/,
  );
  assert.deepEqual(
    module.workspaceDestinationIntent({ type: "set-field", field: "destination", value: "/tmp/export" }),
    { type: "set-field", field: "destination", value: "/tmp/export" },
  );
  assert.deepEqual(
    module.workspaceDestinationIntent({ type: "activate", action: "choose-destination" }),
    { type: "activate", action: "choose-destination" },
  );
  assert.deepEqual(
    module.workspaceDestinationIntent({
      type: "activate",
      action: "preview-single",
      selectedFile: "reports/forecast.xlsx",
      destination: "/tmp/export",
    }),
    {
      type: "activate",
      action: "preview-single",
      selectedFile: "reports/forecast.xlsx",
      destination: "/tmp/export",
    },
  );
  assert.deepEqual(
    module.workspaceDestinationIntent({
      type: "activate",
      action: "preview-all",
      destination: "/tmp/export",
    }),
    { type: "activate", action: "preview-all", destination: "/tmp/export" },
  );
  assert.throws(
    () => module.workspaceDestinationIntent({ type: "activate", action: "preview-single" }),
    /unrecognized or missing fields/,
  );
  assert.throws(
    () => module.workspaceDestinationIntent({
      type: "activate",
      action: "preview-single",
      selectedFile: "reports/forecast.xlsx",
      destination: "",
    }),
    /unsafe or unbounded/,
  );
  assert.throws(
    () => module.workspaceDestinationIntent({ type: "activate", action: "confirm-batch", force: true }),
    /unrecognized or missing fields/,
  );
});

test("Update destination keeps large saved-file choices searchable and selected", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const destinationModule = await loadModule("./src/models/workspace-destination.ts");
  const { WorkspaceDestination } = await loadModule("./src/organisms/workspace-destination.tsx");
  const files = Array.from({ length: 4_096 }, (_, index) => ({
    value: `reports/file-${String(index).padStart(4, "0")}.txt`,
    label: `reports/file-${String(index).padStart(4, "0")}.txt`,
  }));
  const selectedFile = files.at(-1).value;
  const projection = destinationModule.workspaceDestinationChoiceProjection(files, "", selectedFile);
  assert.equal(projection.items.length, 500);
  assert.equal(projection.items.some((choice) => choice.value === selectedFile), true);
  assert.equal(projection.matched, 4_096);
  assert.equal(projection.truncated, true);

  const html = renderToStaticMarkup(React.createElement(WorkspaceDestination, {
    model: { ...destination, files, selectedFile },
    generation: 20,
    onIntent: () => assert.fail("SSR must not emit an intent"),
  }));
  assert.equal((html.match(/data-mesh-destination-file=/g) || []).length, 500);
  assert.match(html, /500 of 4,096 matching saved files shown/);
  assert.match(html, /file-4095\.txt/);
  assert.doesNotMatch(html, /file-0000\.txt/);
});

test("the destination organism keeps every staged update control and readable plan visible", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { WorkspaceDestination } = await loadModule("./src/organisms/workspace-destination.tsx");
  const html = renderToStaticMarkup(React.createElement(WorkspaceDestination, {
    model: destination,
    generation: 19,
    onIntent: () => assert.fail("SSR must not emit an intent"),
  }));

  assert.match(html, /Update saved work in an ordinary folder/);
  assert.match(html, /Saved file/);
  assert.match(html, /Selected destination/);
  assert.match(html, /Or type a destination folder/);
  assert.match(html, /Preview one file/);
  assert.match(html, /Preview saved workspace/);
  assert.match(html, /Update 2 changed files/);
  assert.match(html, /disabled=""[^>]*>confirm-single|disabled=""/);
  assert.match(html, /role="status" aria-live="polite"/);
  assert.match(html, /Replace  reports\/forecast\.xlsx/);
  assert.match(html, /flex flex-col gap-2 sm:flex-row/);
  assert.match(html, /data-mesh-proof="workspace-destination"/);
  assert.match(html, /data-mesh-generation="19"/);
  assert.match(html, /data-mesh-proof="destination-selected"[^>]*tabindex="0"[^>]*>\/Users\/finance\/Forecast<\/output>/);
  assert.match(html, /select-text/);
  assert.match(html, /data-mesh-proof="destination-draft"/);
  assert.match(html, /Use typed destination/);
  assert.match(html, /data-mesh-proof="destination-choose"/);
  assert.equal((html.match(/data-mesh-transition-action="read-only-preview"/g) || []).length, 2);
  assert.match(html, /data-mesh-proof="destination-preview-all"/);
  assert.match(html, /data-mesh-proof="destination-confirm-all"/);
  assert.match(html, /data-mesh-proof="destination-plan"/);
  assert.doesNotMatch(html, /__TAURI__|invoke\(/);
});

test("the selected destination remains exact and copyable across repeated generations", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { WorkspaceDestination } = await loadModule("./src/organisms/workspace-destination.tsx");
  for (const generation of [20, 21]) {
    const html = renderToStaticMarkup(React.createElement(WorkspaceDestination, {
      model: destination,
      generation,
      onIntent: () => assert.fail("SSR must not emit an intent"),
    }));
    assert.match(html, new RegExp(`data-mesh-generation="${generation}"`));
    assert.match(html, /data-mesh-proof="destination-selected"[^>]*tabindex="0"[^>]*>\/Users\/finance\/Forecast<\/output>/);
  }
  const organism = readFileSync(new URL("./src/organisms/workspace-destination.tsx", import.meta.url), "utf8");
  assert.match(organism, /event\.key !== "Enter"[\s\S]*event\.preventDefault\(\)[\s\S]*useDestinationDraft\(\)/);
  assert.match(organism, /onIntent\(\{ type: "set-field", field: "destination", value: destination \}\)/);
});

test("a canceled or refused chooser preserves the typed destination draft", async () => {
  const { destinationDraftWasAccepted } = await loadModule("./src/organisms/workspace-destination.tsx");
  const draft = "/Users/finance/Typed alternative ";
  const chooserRevisionAtStart = destination.chooserRevision;

  assert.equal(Boolean(draft.trim()), true, "the Use typed destination action should remain enabled");
  assert.equal(
    destinationDraftWasAccepted(null, chooserRevisionAtStart, destination),
    false,
    "a cancel or refused chooser with no coordinator change cleared the typed draft",
  );
  assert.equal(draft, "/Users/finance/Typed alternative ");
  assert.equal(
    destinationDraftWasAccepted(null, chooserRevisionAtStart, { ...destination, chooserRevision: 5, destination: "/Users/finance/Chosen" }),
    true,
    "an accepted chooser result did not acknowledge the prior draft",
  );
  assert.equal(
    destinationDraftWasAccepted(draft, null, { ...destination, destination: draft }),
    true,
    "the exact submitted typed destination was not acknowledged",
  );

  const organism = readFileSync(new URL("./src/organisms/workspace-destination.tsx", import.meta.url), "utf8");
  assert.doesNotMatch(organism, /const chooseDestination = \(\) => \{\s*clearDestinationDraft\(\)/);
  assert.doesNotMatch(
    organism,
    /destinationDraftRef\.current\?\.value\.trim\(\)/,
    "a typed destination ending in a space must not be redirected to another folder",
  );
});

test("production keeps destination continuity while retaining an exact failure fallback", () => {
  const island = readFileSync(new URL("./src/island.tsx", import.meta.url), "utf8");
  const page = readFileSync(new URL("./src/pages/update-destination-workspace-page.tsx", import.meta.url), "utf8");
  const html = readFileSync(new URL("../ui/index.html", import.meta.url), "utf8");
  const coordinator = readFileSync(new URL("../ui/app.js", import.meta.url), "utf8");

  assert.match(html, /id="workspace-destination-next"[^>]*class="hidden"/);
  assert.doesNotMatch(html, /id="workspace-destination-current"/);
  assert.match(page, /<IslandSlot[\s\S]*label=\{slot\.loadingLabel\}[\s\S]*failed=\{surfaceFailures\[slot\.name\]\}/);
  for (const removedId of ["update-destination-eyebrow", "update-destination-title", "export-target", "export-hint", "export-output"]) {
    assert.doesNotMatch(html, new RegExp(`id="${removedId}"`));
    assert.doesNotMatch(coordinator, new RegExp(`\\$\\('${removedId}'\\)`));
  }
  assert.match(island, /mesh:workspace-destination-mounted/);
  assert.match(island, /workspaceDestinationEnvelope/);
  assert.match(
    coordinator,
    /const exactGeneration = detail\?\.generation === workspaceDestinationNextPending\.generation[\s\S]*detail\.generation === workspaceDestinationNextMounted\.generation/,
  );
  assert.match(
    coordinator,
    /projectionHasVisibleContinuity\(\s*workspaceDestinationNextPending,\s*workspaceDestinationNextMounted,?\s*\)/,
  );
  assert.match(
    coordinator,
    /workspaceDestinationNextPending = Object\.freeze\([\s\S]*?continuityKey[\s\S]*?workspaceDestinationNextMounted\?\.continuityKey !== continuityKey[\s\S]*?workspaceDestinationNextMounted = null;[\s\S]*?installWorkspaceDestinationNextVisibility\(\);[\s\S]*?mesh:workspace-destination-projection/,
    "a healthy same-workspace projection may retain only its exact continuity-bound mount",
  );
  assert.match(coordinator, /mesh:workspace-destination-rejected[\s\S]*workspaceDestinationNextMounted = null;[\s\S]*installWorkspaceDestinationNextVisibility\(\)/);
  assert.match(coordinator, /'workspace-destination-next'\)\.classList\.toggle\('hidden', !exactMount\)/);
  assert.doesNotMatch(coordinator, /workspace-destination-current/);
  assert.match(coordinator, /workspaceDestinationActionKey/);
  assert.match(coordinator, /renderWorkspaceDestinationNext/);
  assert.match(coordinator, /chooserRevision: exportDestinationChooserRevision/);
  assert.match(coordinator, /workspaceDestinationDraft = path;[\s\S]*exportDestinationChooserRevision = [\s\S]*private-export-coordinator-accepted/);
  assert.match(coordinator, /workspaceDestinationNextPending\.interactionGeneration === workspaceDestinationNextMounted\.generation/);
  assert.match(coordinator, /applyWorkspaceDestinationDraft\(intent\.value, interactionGeneration\)/);
  assert.match(coordinator, /function workspaceDestinationActionPresentation\(\)/);
  assert.match(coordinator, /actions\.set\('activate:choose-destination', \(\) => chooseExportTarget\(\)\)/);
  assert.match(coordinator, /actions\.set\('activate:preview-single', \(intent\) => previewSingleManagedExport\(\{/);
  assert.match(coordinator, /actions\.set\('activate:confirm-single', \(\) => confirmSingleManagedExport\(preview, previewSequence\)\)/);
  assert.match(coordinator, /actions\.set\('activate:preview-all', \(intent\) => previewAllManagedExports\(\{/);
  assert.match(coordinator, /actions\.set\('activate:confirm-batch', \(\) => confirmBatchManagedExport\(batch, previewSequence\)\)/);
  assert.doesNotMatch(coordinator, /actionControls/);
  assert.doesNotMatch(coordinator, /control\.click\(\)/);
  assert.doesNotMatch(coordinator, /\$\('(?:choose-export-target|export-preview|export-confirm|export-preview-all|export-confirm-all)'\)\.addEventListener/);
  assert.match(island, /createRoot\(container, \{[\s\S]*onUncaughtError: \(\) => rejectDestinationGeneration\(generation\)/);
  assert.match(island, /destinationGenerationIsLive[\s\S]*destination-selected[\s\S]*selected\?\.tagName === "OUTPUT"[\s\S]*selected\.textContent === expectedDestination[\s\S]*destination-draft[\s\S]*destination-confirm-all[\s\S]*destination-hint/);
  assert.match(island, /verificationComplete[\s\S]*setTimeout\(verifyDestinationGeneration, 250\)[\s\S]*requestAnimationFrame\(\(\) => requestAnimationFrame\(\(\) => \{[\s\S]*clearTimeout\(boundedVerification\)[\s\S]*verifyDestinationGeneration\(\)/);
  assert.match(island, /generation !== exactGeneration[\s\S]*!destinationGenerationIsLive\(exactGeneration, candidate\.model\.destination\)[\s\S]*rejectDestinationGeneration\(exactGeneration\)/);
  assert.match(island, /new MutationObserver[\s\S]*queueMicrotask[\s\S]*!destinationGenerationIsLive\(expectedGeneration, expectedDestination\)[\s\S]*rejectDestinationGeneration\(expectedGeneration\)/);
  assert.match(island, /destinationTransitioning = transitioning/);
  assert.match(island, /destinationHost\.setAttribute\("aria-busy", String\(transitioning\)\)/);
  assert.match(island, /event\.type === "keydown" && \(event as KeyboardEvent\)\.key === "Tab"/);
  assert.match(island, /event\.preventDefault\(\);[\s\S]*event\.stopImmediatePropagation\(\)/);
  assert.match(island, /data-mesh-transition-action="read-only-preview"/);
  assert.match(island, /\["beforeinput", "change", "click", "input", "keydown", "mousedown", "pointerdown", "submit"\]/);
  assert.match(
    island,
    /setDestinationTransitioning\(true\)[\s\S]*root\.render[\s\S]*DESTINATION_MOUNTED_EVENT[\s\S]*setDestinationTransitioning\(false\)/,
    "the visible destination must remain non-actionable until its exact generation is accepted",
  );
});
