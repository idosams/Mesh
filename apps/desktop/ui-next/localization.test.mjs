import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import test from "node:test";
import { build } from "esbuild";

async function loadLocalization(attachmentProjection) {
  const result = await build({ stdin: { contents: `
    import React from 'react';
    import { renderToStaticMarkup } from 'react-dom/server';
    import { ProductionNavigation } from './src/organisms/production-navigation';
    import { LanguagePicker } from './src/organisms/language-picker';
    import { ArtifactContentChanges } from './src/organisms/artifact-review';
    import { ImportWorkbench } from './src/organisms/import-workbench';
    import { AttachedProjects, SavedInspection, SavedComparison } from './src/organisms/attached-projects';
    export * from './src/lib/localization';
    export const renderNavigation = () => renderToStaticMarkup(<ProductionNavigation activePage="files" workspaceReady={true} nativeChangeCount={0} onNavigate={() => {}} />);
    export const renderPicker = () => renderToStaticMarkup(<LanguagePicker />);
    export const renderSections = (comparison) => renderToStaticMarkup(<ArtifactContentChanges change={{diffHunks: [], beforeLabel: 'Before', afterLabel: 'After', beforeValues: [], afterValues: []}} layout="inline" comparison={comparison} />);
    export const renderAttachments = () => renderToStaticMarkup(<AttachedProjects />);
    export const renderSavedComparison = (comparison, props = {}) => renderToStaticMarkup(<SavedComparison project="project" comparison={comparison} disabled={false} {...props} />);
    export const renderSavedInspection = (inspection) => renderToStaticMarkup(<SavedInspection project="project" inspection={inspection} disabled={false} />);
    export const renderImport = (model) => renderToStaticMarkup(<ImportWorkbench model={model} onIntent={() => {}} />);
  `, resolveDir: new URL('.', import.meta.url).pathname, loader: 'tsx' }, bundle: true, platform: 'node', format: 'cjs', packages: 'external', write: false, plugins: [{ name: 'expose-section-view-for-regression', setup(builder) { builder.onLoad({ filter: /attached-projects\.tsx$/ }, ({ path }) => ({ contents: readFileSync(path, 'utf8').replace('useState<Projection>(empty)', attachmentProjection ? `useState<Projection>(${JSON.stringify(attachmentProjection)})` : 'useState<Projection>(empty)') + '\nexport { SavedInspection, SavedComparison };', loader: 'tsx' })); builder.onLoad({ filter: /artifact-review\.tsx$/ }, ({ path }) => ({ contents: readFileSync(path, 'utf8') + '\nexport { ArtifactContentChanges };', loader: 'tsx' })); } }] });
  const module = { exports: {} };
  Function('require', 'module', 'exports', result.outputFiles[0].text)(createRequire(import.meta.url), module, module.exports);
  return module.exports;
}

test('Hebrew changes navigation and accessible names while protocol route identities stay fixed', async () => {
  const ui = await loadLocalization();
  assert.equal(ui.getLocale(), 'en');
  assert.match(ui.renderNavigation(), />Files</);
  ui.setLocale('he');
  const html = ui.renderNavigation();
  assert.match(html, /aria-label="עמודים ראשיים"/);
  assert.match(html, /aria-current="page"[^>]*>קבצים</);
  assert.match(html, />סביבות עבודה</);
  assert.match(ui.renderPicker(), /value="he"[^>]*selected/);
  ui.setLocale('en');
  assert.match(ui.renderNavigation(), />Files</);
});

test('language selection persists only a presentation preference, updates direction, and notifies without remount', async () => {
  const ui = await loadLocalization();
  const oldDocument = globalThis.document;
  const oldStorage = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
  const writes = [];
  globalThis.document = { documentElement: {} };
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: { setItem: (key, value) => writes.push([key, value]) } });
  let updates = 0;
  const unsubscribe = ui.subscribeLocale(() => updates++);
  try {
    ui.setLocale('he');
    assert.deepEqual(globalThis.document.documentElement, { lang: 'he', dir: 'rtl' });
    assert.deepEqual(writes, [['mesh.ui.locale.v1', 'he']]);
    assert.equal(updates, 1);
    ui.setLocale('invalid');
    assert.equal(ui.getLocale(), 'he');
    unsubscribe();
    ui.setLocale('en');
    assert.equal(updates, 1);
    assert.equal(globalThis.document.documentElement.dir, 'ltr');
    assert.equal(ui.resolveLocale('he'), 'he');
    assert.equal(ui.resolveLocale('bogus'), 'en');
  } finally {
    globalThis.document = oldDocument;
    if (oldStorage) Object.defineProperty(globalThis, 'localStorage', oldStorage); else delete globalThis.localStorage;
  }
});

test('unavailable preference storage does not prevent changing the interface', async () => {
  const ui = await loadLocalization();
  const oldStorage = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, get() { throw new Error('denied'); } });
  try { assert.doesNotThrow(() => ui.setLocale('he')); assert.equal(ui.getLocale(), 'he'); }
  finally { if (oldStorage) Object.defineProperty(globalThis, 'localStorage', oldStorage); else delete globalThis.localStorage; }
});

test('Hebrew import explains refusal of empty folders without changing source paths or verified file text', async () => {
  const ui = await loadLocalization();
  ui.setLocale('he');
  const model = { phase: 'review', sourcePath: '/Users/משפחה/Files', destinationPath: '', fileCount: '0', folderCount: '0', byteCount: '0 B', files: ['Files', 'Review'], summary: 'exact-summary', scopeNote: 'This folder has no importable files or folders. Choose another folder containing project content. No workspace can be created from this preview.', busy: false, canChoose: true, canConfirm: false, canEditDestination: true, canChooseDestination: true, confirmLabel: 'Choose a folder with content' };
  const html = ui.renderImport(model);
  assert.match(html, /בחירת תיקייה עם תוכן/);
  assert.match(html, /אין קבצים או תיקיות שניתן לייבא/);
  assert.match(html, /\/Users\/משפחה\/Files/);
  assert.match(html, />Files<\/li>/);
  assert.match(html, />Review<\/li>/);
  assert.match(html, /exact-summary/);
  assert.equal(ui.translate('he', '{"code":"folder-import-refused"}'), '{"code":"folder-import-refused"}');
});

test('RTL styles isolate technical values and never reverse file contents', () => {
  const css = readFileSync(new URL('./src/styles.css', import.meta.url), 'utf8');
  assert.match(css, /:host\s*\{ direction: inherit;/);
  assert.match(css, /\.font-mono, pre, code[^}]*direction: ltr; unicode-bidi: isolate;/);
  assert.doesNotMatch(css, /bidi-override/);
});

test('Hebrew destructive confirmation preserves exact paths and the stop-on-partial-update consequence', async () => {
  const ui = await loadLocalization();
  const deletion = ui.translate('he', 'Delete notes.txt from /Users/משפחה/Review? Saved file content remains in immutable history, but the current working file will be removed.');
  assert.match(deletion, /קובץ העבודה הנוכחי יימחק/);
  assert.ok(deletion.includes('\u2068/Users/משפחה/Review\u2069'));
  assert.ok(deletion.includes('\u2068notes.txt\u2069'));
  const update = ui.translate('he', 'Update 3 proven files in /tmp/target? Mesh keeps 2 unproven files unchanged. Each selected file is installed atomically; if a later file changes, Mesh stops and keeps the already-completed prefix.');
  assert.match(update, /משאיר 2 קבצים שלא אומתו ללא שינוי/);
  assert.match(update, /עוצר ומשאיר את העדכונים שכבר הושלמו/);
  assert.equal(ui.translate('en', 'Delete notes.txt?'), 'Delete notes.txt?');
  assert.equal(ui.translate('he', 'Different unknown safety message.'), 'Different unknown safety message.');
});


test('artifact section names that coincide with translated UI copy remain exact in Hebrew', async () => {
  const ui = await loadLocalization();
  ui.setLocale('he');
  const html = ui.renderSections({ title: 'Sections', note: '', hunks: [], sectionLabels: ['Review', 'Files', 'שמות מקוריים'] });
  assert.match(html, /aria-label="חלק במסמך"/);
  assert.match(html, /<option value="0">Review<\/option>/);
  assert.match(html, /<option value="1">Files<\/option>/);
  assert.match(html, /<option value="2">שמות מקוריים<\/option>/);
  assert.doesNotMatch(html, /<option value="[01]">(?:סקירה|קבצים)<\/option>/);
});

test('existing-project attachment stays localized beside the copy flow and keeps paths literal', async () => {
  const ui = await loadLocalization();
  const model = { phase: 'select', sourcePath: '/Users/משפחה/Files', destinationPath: '', fileCount: '0', folderCount: '0', byteCount: '0 B', files: [], summary: '', scopeNote: '', busy: false, canChoose: true, canConfirm: false, canEditDestination: true, canChooseDestination: true, confirmLabel: 'Choose a folder' };
  ui.setLocale('he');
  const html = ui.renderImport(model);
  assert.match(html, /aria-label="פרויקטים מחוברים"/);
  assert.match(html, /חיבור פרויקט קיים/);
  assert.match(html, /פרויקטים ששוחזרו נשארים עצורים עד לחידוש הלכידה/);
  assert.match(html, /משחזר השוואות שמורות/);
  assert.match(html, /יצירת עותק עבודה/);
  assert.match(html, /dir="ltr"[^>]*value="\/Users\/משפחה\/Files"/);
  ui.setLocale('en');
  assert.match(ui.renderImport(model), /Use your existing project/);
});

test('saved attachment inspection localizes controls while preserving literal paths and inert content', async () => {
  const ui = await loadLocalization();
  ui.setLocale('he');
  const inspection = {
    operation: 'a'.repeat(64),
    entries: [{ path: 'Review/קובץ.txt', kind: 'file', bytes: 40, digest: 'b'.repeat(64), executable: true }],
    nextAfter: 'Review/קובץ.txt',
    file: { path: 'Review/קובץ.txt', state: 'text', text: 'Files <script>alert(1)</script>', bytes: 40 },
  };
  const html = ui.renderSavedInspection(inspection);
  assert.match(html, /aria-label="קבצי הגרסה השמורה"/);
  assert.match(html, /קבצים נוספים/);
  assert.match(html, /ניתן להרצה/);
  assert.match(html, /<bdi dir="ltr">Review\/קובץ.txt<\/bdi>/);
  assert.match(html, /<pre dir="ltr"[^>]*>Files &lt;script&gt;alert\(1\)&lt;\/script&gt;<\/pre>/);
  assert.doesNotMatch(html, /<script>/);
  assert.ok(html.includes(inspection.operation));
  inspection.file = { ...inspection.file, state: 'binary', text: null };
  assert.match(ui.renderSavedInspection(inspection), /קובץ בינארי/);
  ui.setLocale('en');
  assert.match(ui.renderSavedInspection(inspection), /Binary file. Text preview is unavailable/);
});

test('saved comparison localizes both sides without translating or interpreting saved content', async () => {
  const ui = await loadLocalization();
  ui.setLocale('he');
  const side = { kind: 'file', bytes: 5, digest: 'd'.repeat(64), executable: false };
  const comparison = { base: 'a'.repeat(64), target: 'b'.repeat(64), total: 1, nextAfter: null,
    changes: [{ path: 'Review/שלום.txt', change: 'modified', before: side, after: { ...side, executable: true } }],
    file: { path: 'Review/שלום.txt', beforeKind: 'file', afterKind: 'file',
      before: { state: 'text', text: 'Files <script>before</script>' },
      after: { state: 'text', text: 'Review <script>after</script>' } } };
  const html = ui.renderSavedComparison(comparison);
  assert.match(html, /aria-label="השוואת גרסאות שמורות"/);
  assert.match(html, /התוכן השתנה/);
  assert.match(html, /<bdi dir="ltr">Review\/שלום.txt<\/bdi>/);
  assert.match(html, /Files &lt;script&gt;before&lt;\/script&gt;/);
  assert.match(html, /Review &lt;script&gt;after&lt;\/script&gt;/);
  assert.doesNotMatch(html, /<script>/);
  assert.ok(html.includes(comparison.base) && html.includes(comparison.target));
  comparison.file.after = null;
  comparison.file.afterKind = 'absent';
  assert.match(ui.renderSavedComparison(comparison), /אינו קיים בגרסה זו/);
  ui.setLocale('en');
  assert.match(ui.renderSavedComparison(comparison), /Absent in this version/);
});


test('comparison pin controls localize disabled states and pinned views cannot recursively pin', async () => {
  const ui = await loadLocalization();
  const comparison = { base: 'a'.repeat(64), target: 'b'.repeat(64), total: 0, nextAfter: null, changes: [], file: null };
  ui.setLocale('he');
  const available = ui.renderSavedComparison(comparison, { canPin: true });
  assert.match(available, /נעיצת השוואה לצד השוואות אחרות/);
  assert.doesNotMatch(available, / disabled=""/);
  const full = ui.renderSavedComparison(comparison);
  assert.match(full, /נעיצת השוואה לצד השוואות אחרות/);
  assert.match(full, / disabled=""/);
  const pinned = ui.renderSavedComparison(comparison, { pinKey: '17', canPin: true });
  assert.doesNotMatch(pinned, /נעיצת השוואה|שמונה השוואות/);
  assert.ok(pinned.includes(comparison.base) && pinned.includes(comparison.target));
  ui.setLocale('en');
  assert.match(ui.renderSavedComparison(comparison, { canPin: true }), /Pin comparison alongside others/);
});


test('detached projects explain retained work in Hebrew and only enable explicit reattachment', async () => {
  const project = { detached: true, recovery: 'restored-stopped', id: 'a'.repeat(64), generation: '2',
    root: '/Users/משפחה/Files', phase: 'stopped', outcome: 'saved', savedVersion: 'b'.repeat(64), captureAgeMs: null };
  const ui = await loadLocalization({ projects: [project], histories: {}, bases: {}, comparisons: {},
    inspections: {}, pins: [], pinStatus: 'saved', pinError: '', busy: false, error: '', available: true });
  ui.setLocale('he');
  const html = ui.renderAttachments();
  assert.match(html, /מנותק · ההיסטוריה השמורה נשמרת/);
  assert.match(html, /הקבצים, תהליך העבודה עם Git וההיסטוריה השמורה נשמרים/);
  assert.ok(html.includes('<bdi dir="ltr">/Users/משפחה/Files</bdi>'));
  const buttons = [...html.matchAll(/<button([^>]*)>(.*?)<\/button>/g)];
  const button = text => buttons.find(([, , body]) => body === ui.translate('he', text));
  assert.match(button('Capture now')[1], /\sdisabled=""/);
  assert.match(button('Resume capture')[1], /\sdisabled=""/);
  assert.doesNotMatch(button('Reattach project')[1], /\sdisabled=""/);
  assert.doesNotMatch(button('Show latest versions')[1], /\sdisabled=""/);
  assert.equal(button('Detach Mesh'), undefined);
  ui.setLocale('en');
  assert.match(ui.renderAttachments(), /Detached · saved history retained/);
});


test('native event and periodic fallback status are localized and absent for stopped or detached capture', async () => {
  for (const [nativeEvents, phase, detached, expected] of [
    [true, 'waiting', false, 'File-change signals active, with periodic checks for missed changes.'],
    [false, 'waiting', false, 'Using periodic checks for file changes.'],
    [true, 'stopped', false, null], [true, 'waiting', true, null],
  ]) {
    const ui = await loadLocalization({ projects: [{ nativeEvents, phase, detached, recovery: null,
      id: 'a'.repeat(64), generation: '1', root: '/Users/משפחה/Files', outcome: 'unchanged', savedVersion: null, captureAgeMs: null }],
      histories: {}, bases: {}, comparisons: {}, inspections: {}, pins: [], pinStatus: 'saved', pinError: '',
      busy: false, error: '', available: true });
    ui.setLocale('he');
    const html = ui.renderAttachments();
    assert.ok(html.includes('<bdi dir="ltr">/Users/משפחה/Files</bdi>'));
    if (expected) { assert.ok(html.includes(ui.translate('he', expected))); assert.ok(!html.includes(expected)); }
    else assert.doesNotMatch(html, /התראות על שינויי קבצים פעילות|שינויים בקבצים נבדקים באופן תקופתי/);
  }
});


test('saved review requests render translated bounds and preserve inert exact identities and paths', async () => {
  const id = 'a'.repeat(64), target = 'b'.repeat(64), bundle = 'c'.repeat(64);
  const review = { bundle, target, reviewed_head: 'd'.repeat(64), presentation: 'e'.repeat(64),
    complete: false, unavailable: null, changes_not_listed: 2, operations_not_listed: 3,
    changes: [{ before: null, after: '<script>bad</script>.txt', effect: 'added' }] };
  const projection = { projects: [{ detached: true, recovery: null, id, generation: '1',
    root: '/Users/משפחה/Files', phase: 'stopped', outcome: 'saved', savedVersion: target, captureAgeMs: null }],
    histories: { [id]: { versions: [target], nextBefore: null } }, reviewQueues: { [id]: { reviews: [review], notListed: 4 } },
    selectedReviews: { [id]: review }, bases: {}, comparisons: {}, inspections: {}, pins: [], pinStatus: 'saved',
    pinError: '', busy: false, error: '', available: true };
  const ui = await loadLocalization(projection);
  ui.setLocale('he');
  const html = ui.renderAttachments();
  for (const label of ['Show review requests', 'Request review', 'Saved review requests', 'Selected saved review',
    'Review request recorded', 'This overview is incomplete; it cannot stand in for complete review.', 'Inspect saved result']) {
    assert.notEqual(ui.translate('he', label), label);
    assert.ok(html.includes(ui.translate('he', label)), label);
  }
  assert.ok(html.includes('<bdi dir="ltr">' + bundle + '</bdi>'));
  assert.ok(html.includes('<bdi dir="ltr">added: &lt;script&gt;bad&lt;/script&gt;.txt</bdi>'));
  assert.doesNotMatch(html, /<script>/);
  assert.match(html, /2 שינויים ועוד 3 פעולות הושמטו/);
  assert.match(html, /4 בקשות נוספות/);
  assert.doesNotMatch(html, /Saved content verified for this request/);
  ui.setLocale('en');
  assert.match(ui.renderAttachments(), /Review request recorded/);
  const unavailable = await loadLocalization({ ...projection, selectedReviews: { [id]: { ...review, unavailable: 'missing-content', presentation: null } } });
  const missing = unavailable.renderAttachments();
  assert.match(missing, /Review content is unavailable; the request is retained/);
  assert.match(missing, /<button[^>]*disabled=""[^>]*>Inspect saved result<\/button>/);
});


test('attached main approval renders localized availability and never enables an unverified review', async () => {
  const id = 'a'.repeat(64), target = 'b'.repeat(64), bundle = 'c'.repeat(64), head = 'd'.repeat(64);
  const review = { bundle, target, reviewed_head: head, presentation: 'e'.repeat(64), complete: true,
    unavailable: null, changes: [], changes_not_listed: 0, operations_not_listed: 0 };
  const ready = { available: true, enrolled: true, reason: null, mainAvailable: true, main: null };
  const base = { projects: [{ id, generation: '1', root: '/Users/משפחה/Files', phase: 'waiting',
    outcome: 'saved', savedVersion: 'f'.repeat(64), captureAgeMs: null, recovery: null }],
    histories: {}, bases: {}, comparisons: {}, inspections: {}, pins: [], pinStatus: 'saved', pinError: '',
    busy: false, error: '', available: true, selectedReviews: { [id]: review } };
  for (const [state, selected, disabled, label] of [
    [undefined, review, true, 'Approve as Mesh main…'],
    [ready, review, false, 'Approve as Mesh main…'],
    [{ ...ready, enrolled: false }, review, true, 'Approve as Mesh main…'],
    [{ ...ready, mainAvailable: false }, review, true, 'Approve as Mesh main…'],
    [ready, { ...review, complete: false, changes_not_listed: 1 }, true, 'Approve as Mesh main…'],
    [ready, { ...review, complete: false, unavailable: 'missing-content' }, true, 'Approve as Mesh main…'],
    [{ ...ready, main: { head, target, bundle } }, review, true, 'This version is Mesh main'],
  ]) {
    const ui = await loadLocalization({ ...base, selectedReviews: { [id]: selected }, approvalStates: state ? { [id]: state } : {} });
    ui.setLocale('he');
    const html = ui.renderAttachments();
    const button = [...html.matchAll(/<button([^>]*)>(.*?)<\/button>/g)].find(([, , text]) => text === ui.translate('he', label));
    assert.ok(button, label);
    assert.equal(/ disabled=""/.test(button[1]), disabled);
    assert.notEqual(ui.translate('he', label), label);
    assert.ok(html.includes(ui.translate('he', 'Mesh main · last checked')));
    assert.ok(html.includes('<bdi dir="ltr">/Users/משפחה/Files</bdi>'));
    if (state?.main) {
      assert.ok(html.includes('<bdi dir="ltr">' + target + '</bdi>'));
      assert.ok(html.includes(ui.translate('he', 'Inspect Mesh main')));
    }
    if (state && !state.enrolled) assert.ok(html.includes(ui.translate('he', 'Set up approvals on this Mac')));
  }
  const feedback = 'Approval was not confirmed or was cancelled. Refresh Mesh main before retrying.';
  const ui = await loadLocalization({ ...base, approvalStates: { [id]: { ...ready, available: false, enrolled: false, reason: '<script>unavailable</script>' } }, approvalFeedback: { [id]: feedback } });
  ui.setLocale('he');
  const html = ui.renderAttachments();
  assert.ok(html.includes(ui.translate('he', feedback)));
  assert.ok(html.includes('&lt;script&gt;unavailable&lt;/script&gt;'));
  assert.doesNotMatch(html, /<script>/);
  ui.setLocale('en');
  assert.match(ui.renderAttachments(), /Refresh main and approval availability/);
});

test('attached main comparison renders localized read-only conflicts and inert exact paths', async () => {
  const id = 'a'.repeat(64), target = 'b'.repeat(64), bundle = 'c'.repeat(64), head = 'd'.repeat(64);
  const path = 'תיקייה/<script>current</script>.txt';
  const projection = { projects: [{ id, generation: '1', root: '/Users/משפחה/Files', phase: 'waiting',
    outcome: 'saved', savedVersion: target, captureAgeMs: null, recovery: null }],
    histories: {}, bases: {}, comparisons: {}, inspections: {}, pins: [], pinStatus: 'saved', pinError: '',
    busy: false, error: '', available: true,
    approvalStates: { [id]: { available: false, enrolled: false, mainAvailable: true, main: { head, target, bundle } } },
    integrationPreviews: { [id]: { head, target, bundle, base_head: 'e'.repeat(64), observed_digest: 'f'.repeat(64),
      matches_base: 0, already_present: 0, preserve_current: 0, conflicts: 1, blocked: 1, not_listed: 1,
      entries: [{ path, status: 'conflict', reason: 'contains-unobserved-entry' }] } },
    integrationErrors: { [id]: 'A fresh working-folder comparison is unavailable. Any previous observation is retained. Refresh main and retry.' } };
  const ui = await loadLocalization(projection);
  for (const locale of ['en', 'he']) {
    ui.setLocale(locale);
    const html = ui.renderAttachments();
    for (const label of ['Compare main with working files', 'Working folder compared with Mesh main', 'Review applying accepted changes',
      'This directory contains entries outside the captured view, including possible ignored content.',
      'more entries are omitted from this overview. The counts include all entries.',
      'Working files may have changed since this comparison. Applying changes requires fresh native checks and complete text confirmation. Groups can create complete new folders and create, replace or remove regular files. Removing folders, changing between files and folders, and incomplete or oversized text confirmations remain unavailable.',
      projection.integrationErrors[id]]) {
      assert.ok(html.includes(ui.translate(locale, label)), label);
      if (locale === 'he') assert.notEqual(ui.translate(locale, label), label);
    }
    assert.ok(html.includes('<bdi dir="ltr">תיקייה/&lt;script&gt;current&lt;/script&gt;.txt</bdi>'));
    assert.ok(html.includes('<bdi dir="ltr">' + target + '</bdi>'));
    assert.doesNotMatch(html, /<script>|apply-main/);
  }
});

test('pending native setup and cleanup have localized status independent of capture stop', async () => {
  for (const [nativeSignalState, phase, detached, expected] of [
    ['starting', 'waiting', false, 'Preparing file-change monitoring; periodic checks continue.'],
    ['stopping', 'stopped', false, 'File-change monitoring is still stopping.'],
    ['stopping', 'stopped', true, 'File-change monitoring is still stopping.'],
  ]) {
    const ui = await loadLocalization({ projects: [{ nativeSignalState, nativeEvents: false, phase, detached, recovery: null,
      id: 'a'.repeat(64), generation: '1', root: '/Users/משפחה/Files', outcome: 'unchanged', savedVersion: null, captureAgeMs: null }],
      histories: {}, bases: {}, comparisons: {}, inspections: {}, pins: [], pinStatus: 'saved', pinError: '',
      busy: false, error: '', available: true });
    ui.setLocale('he');
    const html = ui.renderAttachments();
    assert.ok(html.includes(ui.translate('he', expected)));
    assert.ok(!html.includes(expected));
    assert.ok(html.includes('<bdi dir="ltr">/Users/משפחה/Files</bdi>'));
    ui.setLocale('en');
    assert.ok(ui.renderAttachments().includes(expected));
  }
});

function attachedRecoveryProjection() {
  const id = 'a'.repeat(64), target = 'b'.repeat(64), head = 'c'.repeat(64), bundle = 'd'.repeat(64);
  return { projects: [{ id, generation: '1', root: '/Users/משפחה/Files', phase: 'waiting', detached: false,
    outcome: 'saved', savedVersion: target, captureAgeMs: null, recovery: null }],
    histories: {}, bases: {}, comparisons: {}, inspections: {}, pins: [], pinStatus: 'saved', pinError: '',
    busy: false, error: '', available: true,
    approvalStates: { [id]: { available: false, enrolled: false, mainAvailable: true, main: { head, target, bundle } } },
    integrationPreviews: { [id]: { head, target, bundle, base_head: 'e'.repeat(64), observed_digest: 'f'.repeat(64),
      matches_base: 1, already_present: 0, preserve_current: 0, conflicts: 0, blocked: 0, not_listed: 0,
      entries: [{ path: 'תיקייה/<script>current</script>.txt', status: 'matches-base', reason: null, base: { kind: 'file' }, target: { kind: 'file' } }] } },
    recoveries: { [id]: { more: false, entries: [{ transaction: `restoration-${'b'.repeat(32)}`, status: 'changed-files',
      attention: true, path: 'תיקייה/<script>retained</script>.txt', operation: 'restore-retained', retainedAvailable: true, recordedOutcome: 'absent' }] } } };
}

test('attached recovery localizes uncertainty while retaining literal file and transaction identities', async () => {
  const projection = attachedRecoveryProjection(), id = projection.projects[0].id;
  projection.recoveries[id].more = true;
  const states = [
    ['prepared-arrangement', 'Prepared content is retained'],
    ['applied-arrangement', 'Installed and retained files match the recorded arrangement'],
    ['identity-mismatch', 'File identity changed'],
    ['incomplete-observation', 'Files could not be fully checked'],
    ['invalid-outcome', 'Outcome record needs attention'],
    ['contradictory-outcome', 'Recorded outcome conflicts with current files'],
    ['invalid-receipt', 'Recovery record needs attention'],
    ['unverified-history', 'Approval history could not be verified'],
    ['unavailable-directory', 'Recovery folder unavailable'],
    ['unrecognized-directory-entry', 'Unrecognized recovery entry'],
  ];
  projection.recoveries[id].entries.push(...states.map(([status], i) => ({
    transaction: i === 9 ? '' : `integration-${String(i).padStart(32, '0')}`, status, attention: true,
    path: null, operation: null, retainedAvailable: false, recordedOutcome: null,
  })));

  projection.selectedRecovery = { [id]: { ...projection.recoveries[id].entries[0], transaction: `restoration-${'f'.repeat(32)}` } };
  projection.recoveryErrors = { [id]: 'Recovery could not be refreshed. Any previous observation is retained and may be out of date.' };
  projection.fileChangeFeedback = { [id]: 'The file change was not confirmed or was cancelled. Inspect recovery before retrying; this does not prove the working file is unchanged.' };
  const ui = await loadLocalization(projection);
  for (const locale of ['en', 'he']) {
    ui.setLocale(locale);
    const html = ui.renderAttachments();
    for (const label of ['Retained file recovery', 'Review applying this file', 'Review restoring retained file',
      'Files changed since the recorded operation', 'Needs attention', 'Recovery reference:',
      'Restored private work; this does not approve it as Mesh main.',
      'No outcome was recorded. This does not prove that the working file was unchanged.',
      'Refresh retained files', 'Last observed recovery entries', 'Exact recovery reference', 'Inspect exact recovery',
      'Selected recovery · last observed', 'More entries exist outside this bounded overview. Use an exact recovery reference to inspect one.',
      'Restoring requires native confirmation. An existing current file is retained in a new recovery entry. Restoring an absent path creates a new file and keeps the original retained file available.',
      'File details unavailable', 'Unavailable', ...states.map(([, label]) => label),
      projection.recoveryErrors[id], projection.fileChangeFeedback[id]]) {
      assert.ok(html.includes(ui.translate(locale, label)), label);
      if (locale === 'he') assert.notEqual(ui.translate(locale, label), label);
    }
    assert.ok(html.includes('<bdi dir="ltr">תיקייה/&lt;script&gt;retained&lt;/script&gt;.txt</bdi>'));
    assert.ok(html.includes('<bdi dir="ltr">תיקייה/&lt;script&gt;current&lt;/script&gt;.txt</bdi>'));
    assert.ok(html.includes('<bdi dir="ltr">' + projection.selectedRecovery[id].transaction + '</bdi>'));
    assert.match(html, /<input dir="ltr"[^>]*autoComplete="off"/);
    assert.doesNotMatch(html, /<script>/);
  }
});

test('attached file controls require current main and preserve detached and uncertain recovery boundaries', async () => {
  const id = 'a'.repeat(64);
  for (const [change, applyDisabled, restoreDisabled] of [
    [() => {}, false, false],
    [p => { p.projects[0].detached = true; }, true, true],
    [p => { p.busy = true; }, true, true],
    [p => { p.approvalStates[id].main.head = 'f'.repeat(64); }, true, false],
    [p => { p.integrationErrors = { [id]: 'Unavailable' }; }, true, false],
    [p => { p.recoveryErrors = { [id]: 'Unavailable' }; }, false, true],
  ]) {
    const projection = attachedRecoveryProjection(); change(projection);
    const ui = await loadLocalization(projection);
    for (const locale of ['en', 'he']) {
      ui.setLocale(locale);
      const html = ui.renderAttachments();
      for (const [label, disabled] of [['Review applying this file', applyDisabled], ['Review restoring retained file', restoreDisabled]]) {
        const button = [...html.matchAll(/<button([^>]*)>(.*?)<\/button>/g)].find(([, , text]) => text === ui.translate(locale, label));
        assert.ok(button, label);
        assert.equal(/ disabled=""/.test(button[1]), disabled, label);
      }
    }
  }
  const projection = attachedRecoveryProjection();
  projection.integrationPreviews[id].entries[0].status = 'conflict';
  projection.recoveries[id].entries[0].retainedAvailable = false;
  const ui = await loadLocalization(projection);
  assert.doesNotMatch(ui.renderAttachments(), /Review applying this file|Review restoring retained file/);
});

test('independent lanes localize ancestry, retain literal identities and keep project main on the source', async () => {
  const id = 'a'.repeat(64), child = 'e'.repeat(64), version = 'b'.repeat(64);
  const project = { id, generation: '1', root: '/Users/משפחה/Original', phase: 'waiting', detached: false,
    recovery: null, nativeSignalState: 'starting', nativeEvents: false, outcome: 'saved', savedVersion: version, captureAgeMs: 12 };
  const projection = { projects: [project, { ...project, id: child, root: '/native/קווים/files',
    lane: { unavailable: false, sourceProject: id, sourceVersion: version } }],
    histories: { [id]: { versions: [version], nextBefore: null }, [child]: { versions: [version], nextBefore: null } },
    bases: {}, comparisons: {}, inspections: {}, pins: [], pinStatus: 'saved', pinError: '', busy: false, error: '', available: true,
    laneRequests: { [id]: { request: 'f'.repeat(32), version } },
    laneFeedback: { [id]: 'The line could not be confirmed. Retry the same request to avoid duplicating it. Partial work is retained.' } };
  const ui = await loadLocalization(projection);
  for (const locale of ['en', 'he']) {
    ui.setLocale(locale);
    const html = ui.renderAttachments();
    const cards = html.match(/<article\b[\s\S]*?<\/article>/g);
    assert.equal(cards.length, 2);
    const [source, lane] = cards;
    for (const label of ['Independent line of work', 'Created from:', 'Starting saved version:',
      'Use this folder with your editor or harness. No Mesh-managed agent is assigned. Allocation ancestry does not identify who makes later changes.',
      'Project main stays in the original project. Integrating this line’s saved result into project main is not available here yet.',
      'Lane review requests describe private saved work. Integration with the original project’s main is still unavailable.',
      'Create line from this version', 'Open folder']) {
      assert.ok(lane.includes(ui.translate(locale, label)), label);
      if (locale === 'he') assert.ok(!lane.includes(label), label);
    }
    assert.ok(lane.includes('<bdi dir="ltr">/Users/משפחה/Original</bdi>'));
    assert.ok(lane.includes('<bdi dir="ltr">' + version + '</bdi>'));
    assert.ok(source.includes('aria-label="' + ui.translate(locale, 'Mesh main') + '"'));
    assert.ok(!lane.includes('aria-label="' + ui.translate(locale, 'Mesh main') + '"'));
    assert.ok(source.includes(ui.translate(locale, 'Retry creating this line')));
    assert.ok(source.includes(ui.translate(locale, projection.laneFeedback[id])));
    const button = source.match(/<button[^>]*>[^<]*<\/button>/g).find(value => value.includes(ui.translate(locale, 'Create line from this version')));
    assert.match(button, /disabled=""/);
    assert.ok(lane.includes(ui.translate(locale, 'Preparing file-change monitoring; periodic checks continue.')));
  }
});

test('unavailable lane ancestry retains the folder and stopped history without inventing a source', async () => {
  const id = 'e'.repeat(64);
  const ui = await loadLocalization({ projects: [{ id, generation: '2', root: '/native/retained/files', phase: 'stopped',
    detached: false, recovery: 'unavailable', lane: { unavailable: true }, outcome: 'pending', savedVersion: null, captureAgeMs: null }],
    histories: {}, bases: {}, comparisons: {}, inspections: {}, pins: [], pinStatus: 'saved', pinError: '', busy: false, error: '', available: true });
  for (const locale of ['en', 'he']) {
    ui.setLocale(locale); const html = ui.renderAttachments();
    assert.ok(html.includes(ui.translate(locale, 'Lane ancestry could not be verified. Its files and history are retained.')));
    assert.ok(html.includes('<bdi dir="ltr">/native/retained/files</bdi>'));
    assert.ok(!html.includes(ui.translate(locale, 'Created from:')));
    assert.ok(!html.includes('aria-label="' + ui.translate(locale, 'Mesh main') + '"'));
  }
});
