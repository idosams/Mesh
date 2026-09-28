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
  assert.ok(preview.includes("create complete new folders"));
  assert.ok(preview.includes("move removed folders to recovery"));
  assert.ok(preview.includes("Changing between files and folders"));
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

test("reopened group execution shows historical outcomes and distinguishes missing or changing evidence", () => {
  const entry = {group, transaction, status: "changed-files", path: "later.txt", attention: true, operation: "apply-approved", retainedAvailable: true, recordedOutcome: "applied-observed"};
  const execution = {status: "recorded", attempts: [{transaction, status: "recorded"}],
    outcome: {group, status: "applied-observed", members: [{transaction, status: "applied-observed"}]}};
  const render = () => renderToStaticMarkup(React.createElement(FileRecovery, {
    project: "project", disabled: false, detached: false,
    group: {group, alreadyPresent: [], entries: [entry], execution},
  }));
  let html = render();
  for (const text of ["Saved group execution", "not a fresh verification", "Attempt record found", "Recorded result: change observed", "Files changed since"]) assert.ok(html.includes(text), text);
  execution.status = "no-outcome"; execution.outcome = null;
  html = render();
  assert.ok(html.includes("No final group outcome record"));
  assert.ok(html.includes("does not prove that files were unchanged"));
  assert.ok(!html.includes("Recorded result"));
  for (const status of ["invalid", "changed"]) {
    execution.status = status; html = render();
    assert.ok(html.includes('role="alert"'));
    assert.ok(!html.includes("Attempt record found"));
    assert.ok(html.includes("Files changed since"));
  }
});

test("Hebrew saved execution keeps literal paths and translates uncertainty without exposing unreliable attempts", () => {
  setLocale("he");
  try {
    const execution = { status: "no-outcome", attempts: [{ transaction, status: "recorded" }], outcome: null };
    const render = () => renderToStaticMarkup(React.createElement(FileRecovery, {
      project: "project", disabled: false, detached: false,
      group: { group, alreadyPresent: [], entries: [{ group, transaction, status: "changed-files", path: "שם/UNCHANGED.txt", attention: true, operation: "apply-approved", retainedAvailable: true, recordedOutcome: null }], execution },
    }));
    let html = render();
    assert.ok(html.includes("רישום ביצוע הקבוצה שנשמר"));
    assert.ok(html.includes("לא נמצא רישום תוצאה סופית"));
    assert.ok(html.includes("לעולם אינם מתירים ניסיון חוזר"));
    assert.ok(html.includes('<bdi dir="ltr">שם/UNCHANGED.txt</bdi>'));
    assert.ok(!html.includes("No final group outcome"));
    for (const status of ["invalid", "changed"]) {
      execution.status = status; html = render();
      assert.ok(html.includes('role="alert"'));
      assert.ok(!html.includes("נמצא רישום ניסיון"));
      assert.ok(html.includes('<bdi dir="ltr">שם/UNCHANGED.txt</bdi>'));
      assert.ok(!html.includes("Execution records"));
    }
  } finally { setLocale("en"); }
});

test('new directory recovery renders nested and empty entries without a restore action', () => {
  const html = renderToStaticMarkup(React.createElement(FileRecovery, {
    project: 'a'.repeat(64), disabled: false, detached: false,
    group: { group: `integration-group-${'b'.repeat(32)}`, alreadyPresent: [], entries: [{
      transaction: `directory-${'c'.repeat(32)}`, status: 'source-parent-changed', attention: true,
      path: 'new', operation: 'add-directory', retainedAvailable: false, recordedOutcome: 'applied-observed',
      parentIdentityMatches: false, parentPolicyMatches: true,
      sourceTree: { state: 'observed', entries: [{ path: '', kind: 'directory', bytes: null, digest: null }, { path: 'sub/empty', kind: 'directory', bytes: null, digest: null }, { path: 'sub/file', kind: 'file', bytes: 8, digest: 'a'.repeat(64) }] },
      stagedTree: { state: 'absent', entries: [] },
    }] },
  }));
  for (const text of ['New directory tree', 'containing folder no longer has its recorded identity', '3 entries observed', 'sub/empty', 'sub/file', '8 bytes', 'Prepared tree: absent']) assert.ok(html.includes(text), text);
  assert.ok(!html.includes('Review restoring retained file'));
});

test('Hebrew directory observations preserve literal nested paths without restoration authority', () => {
  setLocale('he');
  try {
    const html = renderToStaticMarkup(React.createElement(FileRecovery, {
      project: 'project', disabled: false, detached: false,
      group: { group, alreadyPresent: [], entries: [{
        transaction: `directory-${'d'.repeat(32)}`, path: 'שם/NEW', status: 'source-parent-changed', attention: true,
        operation: 'add-directory', retainedAvailable: false, recordedOutcome: 'absent', parentIdentityMatches: false,
        sourceTree: { state: 'observed', entries: [{path: '', kind: 'directory'}, {path: 'שם/FILE.txt', kind: 'file', bytes: 8}] },
        stagedTree: { state: 'absent', entries: [] },
      }] },
    }));
    assert.ok(html.includes('עץ תיקיות חדש'));
    assert.ok(html.includes('זהות התיקייה המכילה'));
    assert.ok(html.includes('<bdi dir="ltr">שם/FILE.txt</bdi>'));
    assert.ok(html.includes('העץ שהוכן: חסר'));
    assert.ok(!html.includes('New directory tree'));
    assert.ok(!html.includes('סקירה לפני שחזור הקובץ שנשמר'));
  } finally { setLocale('en'); }
});

test('directory removal recovery shows retained work without offering file restoration', () => {
  const html = renderToStaticMarkup(React.createElement(FileRecovery, {
    project: 'a'.repeat(64), disabled: false, detached: false,
    group: { group: `integration-group-${'b'.repeat(32)}`, alreadyPresent: [], entries: [{
      transaction: `directory-${'c'.repeat(32)}`, status: 'changed-entries', attention: true,
      path: 'old', operation: 'remove-directory', retainedAvailable: false, recordedOutcome: 'applied-observed',
      sourceTree: { state: 'absent', entries: [] },
      stagedTree: { state: 'observed', entries: [{ path: '', kind: 'directory', bytes: null, digest: null }, { path: 'late', kind: 'file', bytes: 4, digest: 'a'.repeat(64) }] },
    }] },
  }));
  for (const text of ['retains the complete original tree', 'Open file and directory handles', 'Retained tree: 2 entries observed', 'late', 'Working tree: absent']) assert.ok(html.includes(text), text);
  assert.ok(!html.includes('Review restoring retained file'));
});

test('Hebrew removal recovery keeps late-work paths literal and offers no file restore', () => {
  setLocale('he');
  try {
    const html = renderToStaticMarkup(React.createElement(FileRecovery, {
      project: 'project', disabled: false, detached: false,
      group: { group, alreadyPresent: [], entries: [{ transaction: `directory-${'c'.repeat(32)}`,
        status: 'changed-entries', attention: true, path: 'שם/OLD', operation: 'remove-directory',
        retainedAvailable: false, recordedOutcome: 'applied-observed', sourceTree: {state: 'absent', entries: []},
        stagedTree: {state: 'observed', entries: [{path: '', kind: 'directory'}, {path: 'שם/LATE.txt', kind: 'file', bytes: 4}]},
      }] },
    }));
    assert.ok(html.includes('הסרת תיקייה שומרת את העץ המקורי בשלמותו'));
    assert.ok(html.includes('העץ שנשמר'));
    assert.ok(html.includes('<bdi dir="ltr">שם/LATE.txt</bdi>'));
    assert.ok(!html.includes('Directory removal retains'));
    assert.ok(!html.includes('סקירה לפני שחזור הקובץ שנשמר'));
  } finally { setLocale('en'); }
});
