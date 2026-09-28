import assert from "node:assert/strict";
import test from "node:test";
import { createRequire } from "node:module";
import { build } from "esbuild";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

const result = await build({
  stdin: { contents: 'export * from "./src/organisms/attached-projects"; export { setLocale } from "./src/lib/localization";', resolveDir: new URL(".", import.meta.url).pathname, loader: "ts" },
  bundle: true, format: "cjs", platform: "node", write: false,
  external: ["react", "react-dom", "react/jsx-runtime"],
});
const module = { exports: {} };
Function("require", "module", "exports", result.outputFiles[0].text)(createRequire(import.meta.url), module, module.exports);
const { FileRecovery, IntegrationPreviewCard, setLocale } = module.exports;
const group = `integration-group-${"a".repeat(32)}`;
const transaction = `integration-${"b".repeat(32)}`;

test("group recovery renders partial outcomes, member observations and historical already-present facts", () => {
  const html = renderToStaticMarkup(React.createElement(FileRecovery, {
    project: "project", disabled: false, detached: false,
    groupOutcome: { group, status: "reconciliation-required", members: [
      { transaction, status: "applied-observed" }, { transaction: "uncertain", status: "reconciliation-required" },
      { transaction: "later", status: "not-attempted" },
    ] },
    group: { group, alreadyPresent: ["present.txt"], restorations: [`restoration-${"c".repeat(32)}`], moreRestorations: true, entries: [{group, transaction, status: "changed-files", path: "kept.txt", attention: true, operation: "remove-approved", retainedAvailable: true, recordedOutcome: "applied-observed"}] },
    recovery: { entries: [{transaction: group, status: "group-reference", attention: true, path: null, operation: null, retainedAvailable: false, recordedOutcome: null}], more: false },
  }));
  for (const text of ["Needs reconciliation", "1 observed", "1 uncertain", "1 not attempted", "kept.txt", "not an atomic snapshot", "not rewritten or reverified", "present.txt", "Inspect recovery group", "Review restoring retained file", "Inspect restoration record", "Exact file recovery reference within this group", "More restoration records"]) assert.ok(html.includes(text), text);
});

test("unavailable group inspection disables restoration and complete-group application respects disabled state", () => {
  const html = renderToStaticMarkup(React.createElement(FileRecovery, {
    project: "project", disabled: false, detached: false, groupError: "Refresh this group",
    group: {group, alreadyPresent: [], entries: [{group, transaction, status: "applied-arrangement", attention: false, path: "kept.txt", operation: "remove-approved", retainedAvailable: true, recordedOutcome: "applied-observed"}]},
  }));
  assert.match(html, /disabled=""[^>]*>Review restoring retained file/);
  const preview = renderToStaticMarkup(React.createElement(IntegrationPreviewCard, {
    project: "project", disabled: true, preview: {target: "saved", matches_base: 1, already_present: 0, preserve_current: 0, conflicts: 0, blocked: 0, entries: [], not_listed: 0},
  }));
  assert.match(preview, /disabled=""[^>]*>Review applying accepted changes/);
  assert.ok(preview.includes("Directory changes"));
});


test("Hebrew group recovery preserves literal paths and identities and refuses stale restoration", () => {
  setLocale("he");
  try {
    const html = renderToStaticMarkup(React.createElement(FileRecovery, {
      project: "project", disabled: false, detached: false,
      groupError: "This recovery group could not be verified. Previous observations may be out of date.",
      group: { group, alreadyPresent: ["שם/CREATE.txt"], entries: [{ group, transaction, status: "changed-files", path: "שם/kept.txt", attention: true, operation: "remove-approved", retainedAvailable: true, recordedOutcome: "applied-observed" }] },
    }));
    assert.ok(html.includes("שחזור קבוצה"));
    assert.ok(html.includes("לא ניתן היה לאמת"));
    assert.ok(html.includes(`<bdi dir="ltr">${group}</bdi>`));
    assert.ok(html.includes('<bdi dir="ltr">שם/CREATE.txt</bdi>'));
    assert.ok(html.includes('<bdi dir="ltr">שם/kept.txt</bdi>'));
    assert.match(html, /disabled=""[^>]*>סקירה לפני שחזור הקובץ שנשמר/);
  } finally { setLocale("en"); }
});
