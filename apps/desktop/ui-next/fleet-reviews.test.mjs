import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { build } from 'esbuild';
const output = await build({ stdin: { contents: `import React from 'react'; import { renderToStaticMarkup } from 'react-dom/server'; import { FleetReviewPanels, FleetSavedResults, fleetSavedReviewModel } from './src/organisms/fleet-reviews.tsx'; export { reduceReviewWorkbench, reconcileReviewWorkbenchProjection } from './src/models/review-workbench.ts'; export { setLocale } from "./src/lib/localization.ts"; export { fleetSavedReviewModel }; export const renderQueue = props => renderToStaticMarkup(React.createElement(FleetSavedResults, props)); export const render = props => renderToStaticMarkup(React.createElement(FleetReviewPanels, props));`, resolveDir: new URL('.', import.meta.url).pathname, loader: 'js' }, bundle: true, format: 'cjs', platform: 'node', packages: 'external', write: false });
const module = { exports: {} };
Function('require', 'module', 'exports', output.outputFiles[0].text)(createRequire(import.meta.url), module, module.exports);
const { render, fleetSavedReviewModel, reduceReviewWorkbench, reconcileReviewWorkbenchProjection } = module.exports;
const change = id => ({ object_id: id.repeat(32), path_before: null, path_after: `file-${id}.txt`, effect: 'content-written', body: 'binary', before: null, after: { kind: 'binary', version_id: '4'.repeat(64), content_digest: '5'.repeat(64), byte_length: '12', line_count: null }, verified_text: { source: 'before-after', before: null, after: { version_id: '4'.repeat(64), content_digest: '5'.repeat(64) }, hunks: [{ before_start: 1, before_len: 0, after_start: 1, after_len: 1, lines: [{ kind: 'added', before: null, after: 1, text: 'saved <text>' }] }] } });
const review = () => ({ bundle: '1'.repeat(64), subject_operation: '2'.repeat(64), recorded: true, reviewed_head: '3'.repeat(64), presentation_digest: '6'.repeat(64), content_complete: true, projection_authorizes_approval: false, subject_operations_not_listed: 0, bundle_changes_not_listed: 0, unavailable_code: null, bundle_changes: [change('a'), change('b')] });
const pin = () => ({ key: '1', selection: { objective: `fleet-${'7'.repeat(64)}`, lane: 'lane-one', checkpoint: 'checkpoint-one', version: '2'.repeat(64), bundle: '1'.repeat(64) }, goal: '<script>goal</script>', startingInput: '8'.repeat(64), review: review(), loading: false, error: '' });
test('panels render verified text, exact base and disabled authority', () => {
  const value = pin(), model = fleetSavedReviewModel(value.review), html = render({ pins: [value], notice: '' });
  assert.match(html, /saved &lt;text&gt;/); assert.match(html, /&lt;script&gt;goal/); assert.doesNotMatch(html, /<script>/);
  assert.match(html, /Comparison base: the recorded review/); assert.match(html, /starting-version comparison below/);
  assert.match(html, /not restored after an app reload/); assert.match(html, new RegExp(value.review.reviewed_head));
  for (const field of ['canApprove', 'canApproveAndExport', 'canExportGit', 'canExportPrivateCopy', 'canRecordReview', 'canRenderArtifactPreview', 'canInspectExactCopies']) assert.equal(model[field], false);
  assert.strictEqual(reduceReviewWorkbench(model, { type: 'approve-version' }), model);
});
test('incomplete and malformed content cannot look like a complete empty result', () => {
  const value = pin(); Object.assign(value.review, { content_complete: false, bundle_changes: [], bundle_changes_not_listed: 5 });
  let html = render({ pins: [value], notice: '' }); assert.match(html, /5 changes/); assert.doesNotMatch(html, /No file changes/);
  value.review = review(); value.review.bundle_changes[0].verified_text.after.version_id = 'f'.repeat(64);
  html = render({ pins: [value], notice: '' }); assert.match(html, /could not be safely displayed/); assert.doesNotMatch(html, /saved &lt;text&gt;/);
  value.review = review(); value.review.bundle_changes = [];
  assert.match(render({ pins: [value], notice: '' }), /No file changes in this recorded comparison/);
});
test('file and layout state are independent and survive same-result refresh', () => {
  const first = fleetSavedReviewModel(review()), second = fleetSavedReviewModel(review());
  let selected = reduceReviewWorkbench(first, { type: 'select-change', changeId: first.changes[1].id });
  selected = reduceReviewWorkbench(selected, { type: 'change-diff-layout', layout: 'unified' });
  const refreshed = reconcileReviewWorkbenchProjection(selected, fleetSavedReviewModel(review()));
  assert.equal(refreshed.selectedChangeId, first.changes[1].id); assert.equal(refreshed.diffLayout, 'unified');
  assert.equal(second.selectedChangeId, second.changes[0].id); assert.equal(second.diffLayout, 'split');
});
test('pending or failed reads keep independent close controls and cached evidence', () => {
  const first = pin(), second = { ...pin(), key: '2', loading: true, review: null }; first.error = 'Read failed.';
  const html = render({ pins: [first, second], notice: '' });
  assert.match(html, /data-mesh-fleet-review="1"/); assert.match(html, /data-mesh-fleet-review="2"/);
  assert.match(html, /previously verified cached result/); assert.match(html, /Reading the exact saved result/);
  assert.match(html, /<button(?![^>]*\sdisabled=)[^>]*>Close review <bdi dir="ltr">2<\/bdi><\/button>/);
});

test('parallel reviews of the same objects have distinct accessible folder headings', () => {
  const first = pin(), second = { ...pin(), key: '2' };
  const html = render({ pins: [first, second], notice: '' });
  const ids = [...html.matchAll(/ id="([^"]+)"/g)].map(match => match[1]);
  assert.ok(ids.length >= 2); assert.equal(new Set(ids).size, ids.length);
  for (const match of html.matchAll(/aria-labelledby="([^"]+)"/g)) assert.ok(ids.includes(match[1]));
});

test('Hebrew parallel review keeps exact identities and file bytes literal while explaining cached and read-only state', () => {
  const first = pin(); first.goal = 'Working'; first.error = 'This exact saved result could not be verified. Its selection is retained.';
  const second = { ...pin(), key: '2', loading: true, review: null };
  module.exports.setLocale('he');
  try {
    const html = render({ pins: [first, second], notice: 'This exact result is already pinned.' });
    assert.match(html, /aria-label="סקירות צי מוצמדות"/);
    assert.match(html, /aria-label="סקירת צי מוצמדת 1"/);
    assert.match(html, /תוצאה מדויקת זו כבר מוצמדת/);
    assert.match(html, /התוצאה הקודמת שנשמרה במטמון ואומתה/);
    assert.match(html, /קורא את התוצאה השמורה המדויקת/);
    assert.match(html, /תוצאה שמורה לקריאה בלבד/);
    assert.match(html, /2 שינויים בסקירה שמורה זו/);
    assert.match(html, /<h4 dir="auto"[^>]*>Working<\/h4>/);
    for (const literal of [...Object.values(first.selection), first.startingInput, first.review.reviewed_head]) assert.ok(html.includes(`<bdi dir="ltr">${literal}</bdi>`));
    assert.match(html, /saved &lt;text&gt;/);
    assert.match(html, /<button[^>]*disabled=""[^>]*>אישור הגרסה המדויקת<\/button>/);
    assert.match(html, /<button(?![^>]*\sdisabled=)[^>]*>סגירת סקירה <bdi dir="ltr">2<\/bdi><\/button>/);
    const ids = [...html.matchAll(/ id="([^"]+)"/g)].map(match => match[1]);
    assert.equal(new Set(ids).size, ids.length);
  } finally { module.exports.setLocale('en'); }
});

test('Hebrew saved-result queues keep stale pages identifiable and disable pinning and pagination on failure', () => {
  const value = pin();
  const queue = { objective: value.selection.objective, lane: value.selection.lane, loading: false, error: 'Saved results could not be loaded. The previous page is retained.', page: { after: null, rows: [{...value.selection, run:'Working'}], total:51, nextAfter:value.selection.checkpoint, revision:4 } };
  module.exports.setLocale('he');
  try {
    const html = module.exports.renderQueue({ objective: queue.objective, lane:queue.lane, queue, available:true });
    assert.match(html, /aria-label="תוצאות מסלול שמורות"/);
    assert.match(html, /הדף הקודם נשמר/);
    assert.match(html, /51 תוצאות שמורות/);
    assert.match(html, /<bdi dir="ltr">Working<\/bdi>/);
    assert.match(html, /<button[^>]*disabled=""[^>]*>הצמדת סקירה שמורה<\/button>/);
    assert.match(html, /<button[^>]*disabled=""[^>]*>התוצאות השמורות הבאות<\/button>/);
    assert.match(html, /<button(?![^>]*\sdisabled=)[^>]*>סגירת רשימת תוצאות<\/button>/);
    value.review = {...review(), content_complete:false, bundle_changes:[], bundle_changes_not_listed:5, subject_operations_not_listed:2};
    const incomplete = render({pins:[value],notice:''});
    assert.match(incomplete, /סקירה זו אינה שלמה/);
    assert.match(incomplete, /5 שינויים ועוד 2 פעולות אינם מוצגים/);
    assert.doesNotMatch(incomplete, /אין שינויים בקבצים/);
  } finally { module.exports.setLocale('en'); }
});

const inputSide = text => ({ path: 'note.txt', kind: 'file', digest: '9'.repeat(64), bytes: text.length, executable: false, state: 'text', text });
const inputState = () => { const file = { object: '7'.repeat(32), effect: 'modified', before: inputSide('old\n'), after: inputSide('new\n') }; return { loading: false, error: '', file, page: { base: '8'.repeat(64), target: '2'.repeat(64), total: 1, after: null, nextAfter: null, changes: [file] } }; };
test('starting-version panel renders actual saved text differences without working-copy claims', () => {
  const value = { ...pin(), review: null, input: inputState() };
  const html = render({ pins: [value], notice: '' });
  assert.match(html, /Changes since this lane started/); assert.match(html, /data-mesh-work-diff-line="removed"/); assert.match(html, /data-mesh-work-diff-line="added"/);
  assert.match(html, /Local starting version/); assert.match(html, /Pinned result/);
  assert.doesNotMatch(html, /Working tree diff|The working copy matches|remains available in Edit/);
  assert.match(html, /does not approve or apply changes to main/);
});
test('metadata-only and unavailable content never claim that saved text is unchanged', () => {
  const value = { ...pin(), review: null, input: inputState() };
  value.input.file.after = { ...inputSide(''), bytes: 300000, text: null, state: 'too-large' };
  let html = render({ pins: [value], notice: '' }); assert.match(html, /256 KiB text preview limit/); assert.doesNotMatch(html, /No text changes/);
  value.input.file.after = { ...inputSide(''), bytes: 4, text: null, state: 'binary-or-unsafe-text' };
  html = render({ pins: [value], notice: '' }); assert.match(html, /Binary or unsafe text/); assert.doesNotMatch(html, /No text changes/);
  value.input.file.after = { ...value.input.file.before, executable: true };
  html = render({ pins: [value], notice: '' }); assert.match(html, /These saved texts are identical. Paths and executable modes may differ/);
});
test('empty comparison and selected absence are explicit; pending reads retain the previous file', () => {
  const value = { ...pin(), review: null, input: inputState() };
  value.input.file.before = null; value.input.file.effect = 'added'; value.input.loading = true;
  let html = render({ pins: [value], notice: '' }); assert.match(html, /Absent in this version/); assert.match(html, /previous selection remains below/);
  value.input = { ...inputState(), file: null, page: { ...inputState().page, total: 0, changes: [] } };
  html = render({ pins: [value], notice: '' }); assert.match(html, /No path, content or executable-mode changes/); assert.doesNotMatch(html, /Selected: note.txt/);
});

test('Hebrew starting comparison preserves paths, versions and saved text while identifying retained content', () => {
  const value = {...pin(), review:null, input:inputState()};
  value.input.file.before.path = 'משפחה/Working.txt'; value.input.file.after.path = 'משפחה/Working.txt';
  value.input.error = 'This starting-version comparison could not be verified. Any displayed content is the previously verified result.';
  module.exports.setLocale('he');
  try {
    const html = render({pins:[value],notice:''});
    assert.match(html, /aria-label="שינויים מאז תחילת המסלול"/);
    assert.match(html, /השוואת גרסאות שמורות/);
    assert.match(html, /התוצאה הקודמת שאומתה/);
    assert.match(html, /השוואה לקריאה בלבד/);
    assert.match(html, /<bdi dir="ltr">משפחה\/Working.txt<\/bdi>/);
    assert.ok(html.includes(`<bdi dir="ltr">${value.input.page.base}</bdi>`));
    assert.ok(html.includes(`<bdi dir="ltr">${value.input.page.target}</bdi>`));
    assert.match(html, /<pre dir="ltr"[^>]*>old\n<\/pre>/);
    assert.match(html, /<pre dir="ltr"[^>]*>new\n<\/pre>/);
    assert.match(html, /ניסיון נוסף לאותה בקשת השוואה/);
    assert.doesNotMatch(html, /Working tree diff|The working copy matches|remains available in Edit/);
  } finally { module.exports.setLocale('en'); }
});

test('Hebrew saved comparison distinguishes identical text from metadata and unavailable content', () => {
  const value = {...pin(), review:null, input:inputState()};
  module.exports.setLocale('he');
  try {
    value.input.file.after = {...value.input.file.before, executable:true};
    let html = render({pins:[value],notice:''});
    assert.match(html, /הטקסטים השמורים האלה זהים/);
    assert.match(html, /הרשאות ההרצה עשויים להיות שונים/);
    assert.doesNotMatch(html, /עותק העבודה תואם/);
    value.input.file.after = {...inputSide(''), bytes:300000, text:null, state:'too-large'};
    html = render({pins:[value],notice:''});
    assert.match(html, /חורג ממגבלת תצוגת הטקסט/);
    assert.doesNotMatch(html, /הטקסטים השמורים האלה זהים/);
    value.input.file.before = null;
    html = render({pins:[value],notice:''});
    assert.match(html, /אינו קיים בגרסה זו/);
    assert.match(html, /300000 בתים/);
  } finally { module.exports.setLocale('en'); }
});
