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
  assert.match(html, /Reopening rechecks exact history/); assert.match(html, new RegExp(value.review.reviewed_head));
  for (const field of ['canApprove', 'canApproveAndExport', 'canExportGit', 'canExportPrivateCopy', 'canRecordReview', 'canInspectExactCopies']) assert.equal(model[field], false);
  assert.equal(model.canRenderArtifactPreview, true);
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
  selected = reduceReviewWorkbench(selected, { type: 'change-diff-layout', layout: 'inline' });
  const refreshed = reconcileReviewWorkbenchProjection(selected, fleetSavedReviewModel(review()));
  assert.equal(refreshed.selectedChangeId, first.changes[1].id); assert.equal(refreshed.diffLayout, 'inline');
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

test('persistence status remains visible without pins and initial loading disables selector changes', () => {
  let html = render({ pins: [], notice: '', persistence: { phase: 'error', message: 'Save is unconfirmed.', editable: true, busy: true } });
  assert.match(html, /Save is unconfirmed/); assert.match(html, /disabled=""[^>]*>Retry saving or loading review selections/);
  assert.match(html, /Reload replaces local selections/);
  html = render({ pins: [pin()], notice: '', persistence: { phase: 'loading', message: '', editable: false } });
  assert.match(html, /Loading saved review selections/); assert.match(html, /disabled=""[^>]*>Close review <bdi dir="ltr">1<\/bdi>/);
  html = render({ pins: [], notice: '', persistence: { phase: 'saved', message: '', editable: true } });
  assert.match(html, /Review selections saved/);
});

test('saved object choice is restored, and an unavailable choice remains explicitly retained', () => {
  const value = { ...pin(), view: { input_open: false, input_after: null, input_object: null, input_layout: 'split', review_object: 'b'.repeat(32), review_mode: 'content', review_layout: 'inline' } };
  let html = render({ pins: [value], notice: '' });
  assert.match(html, /data-mesh-change-path="file-b.txt"/);
  value.view.review_object = 'f'.repeat(32);
  html = render({ pins: [value], notice: '' });
  assert.match(html, /saved file selection is unavailable/); assert.match(html, /saved selection is retained/);
  assert.match(html, /data-mesh-change-path="file-a.txt"/);
});

test('Hebrew restored reviews translate persistence status and retain literal goals and identities', () => {
  const value = {...pin(), goal: 'Saved lane review', review: null, view: {input_open: true, input_after: null, input_object: 'e'.repeat(32), input_layout: 'split', review_object: null, review_mode: 'content', review_layout: 'inline'}};
  module.exports.setLocale('he');
  try {
    let html = render({pins:[value], notice:'', persistence:{phase:'loading',message:'',editable:false}});
    assert.match(html, /טוען בחירות סקירה שמורות/);
    assert.match(html, /disabled=""[^>]*>סגירת סקירה/);
    assert.match(html, /disabled=""[^>]*>השוואה לגרסת ההתחלה/);
    assert.match(html, /יש לאמת את התוכן לפני הצגתו/);
    assert.ok(html.includes(`<bdi dir="ltr">${value.view.input_object}</bdi>`));
    assert.match(html, />Saved lane review<\/h4>/);
    value.goal = null;
    html = render({pins:[value],notice:'',persistence:{phase:'error',message:'Saved pins could not be loaded. Their stored record has not been replaced.',editable:false,busy:true}});
    assert.match(html, /סקירת מסלול שמורה/);
    assert.match(html, /הרשומה השמורה שלהן לא הוחלפה/);
    assert.match(html, /disabled=""[^>]*>ניסיון נוסף לשמירה או לטעינה/);
    assert.match(html, /disabled=""[^>]*>טעינה מחדש של קבוצת הסקירות השמורה/);
    assert.match(html, /לרבות בחירות מקומיות שלא נשמרו/);
    html = render({pins:[],notice:'',persistence:{phase:'saved',message:'',editable:true}});
    assert.match(html, /בחירות הסקירה נשמרו/);
  } finally { module.exports.setLocale('en'); }
});

test('saved artifact preview renders only for the selected exact object and verified envelope', () => {
  const value = pin(); value.review.bundle_changes = [{ ...change('a'), path_after: 'image.png', verified_text: null }];
  value.view = { review_object: 'a'.repeat(32), review_mode: 'visual', review_layout: 'split' };
  const side = { side: 'after', versionId: '4'.repeat(64), contentDigest: '5'.repeat(64), imageDataUrl: 'data:image/png;base64,AAAA', pageNumber: null, pageCount: null, textSource: null, textLines: null, textSections: null, textTruncated: false };
  value.artifact = { generation: 7, object: 'a'.repeat(32), page: 1, loading: false, error: '', envelope: { generation: 7, bundle: value.selection.bundle, changeId: 'a'.repeat(32), kind: 'image', requestedPage: 1, before: null, after: side, beforeError: null, afterError: null, beforeAbsentPage: null, afterAbsentPage: null } };
  let html = render({ pins: [value], notice: '' }); assert.match(html, /src="data:image\/png;base64,AAAA"/);
  value.artifact.envelope.after.contentDigest = '9'.repeat(64);
  html = render({ pins: [value], notice: '' }); assert.doesNotMatch(html, /src="data:image/); assert.match(html, /could not be verified/);
  value.artifact.object = 'b'.repeat(32);
  html = render({ pins: [value], notice: '' }); assert.doesNotMatch(html, /src="data:image|could not be verified/);
});

test('Hebrew saved artifact failures remain localized in visual and content views without altering identities', () => {
  const value = pin();
  const file = {...change('a'), path_before:'משפחה/image.png', path_after:'משפחה/image.png', before:{...change('a').after}, verified_text:null};
  value.review.bundle_changes = [file];
  value.view = {review_object:'a'.repeat(32),review_mode:'visual',review_layout:'split'};
  const side = {side:'after',versionId:'4'.repeat(64),contentDigest:'5'.repeat(64),imageDataUrl:'data:image/png;base64,AAAA',pageNumber:null,pageCount:null,textSource:null,textLines:null,textSections:null,textTruncated:false};
  value.artifact = {generation:7,object:'a'.repeat(32),page:1,loading:false,error:'',envelope:{generation:7,bundle:value.selection.bundle,changeId:'a'.repeat(32),kind:'image',requestedPage:1,before:null,after:side,beforeError:'This exact saved artifact preview is unavailable. Retry to render it again.',afterError:null,beforeAbsentPage:null,afterAbsentPage:null}};
  module.exports.setLocale('he');
  try {
    for (const mode of ['visual','content']) {
      value.view.review_mode = mode;
      const html = render({pins:[value],notice:''});
      assert.match(html, /גרסה קודמת: התצוגה המקדימה של הפריט השמור המדויק אינה זמינה/);
      assert.ok(html.includes('משפחה/image.png'));
      assert.ok(html.includes(`<bdi dir="ltr">${value.selection.version}</bdi>`));
      assert.doesNotMatch(html, /Earlier version:|This exact saved artifact preview/);
      if (mode === 'visual') assert.match(html, /src="data:image\/png;base64,AAAA"/);
    }
    value.view.review_mode = 'visual';
    value.artifact.envelope.after.contentDigest = '9'.repeat(64);
    let html = render({pins:[value],notice:''});
    assert.match(html, /לא ניתן לאמת את התצוגה המקדימה השמורה המדויקת/);
    assert.doesNotMatch(html, /src="data:image/);
    value.artifact.envelope = null;
    value.artifact.error = 'This exact saved artifact preview could not be verified. Retry to render it again.';
    html = render({pins:[value],notice:''});
    assert.match(html, /לא ניתן לאמת את התצוגה המקדימה של הפריט השמור המדויק/);
  } finally { module.exports.setLocale('en'); }
});

test('review feedback distinguishes recorded requests from delivery and retains an uncertain exact retry', () => {
  const value = pin(); value.feedback = { loaded: true, rows: [{ id: 'request', message: '<script>Feedback</script>', status: 'recorded' }], pending: { request: 'pending', message: 'Original pending message' }, sending: false, error: 'Recording is unconfirmed.' };
  const html = render({ pins: [value], notice: '' });
  assert.match(html, /Recorded does not mean delivered or addressed/);
  assert.match(html, /&lt;script&gt;Feedback&lt;\/script&gt;/); assert.doesNotMatch(html, /<script>/);
  assert.match(html, /Original pending message/); assert.match(html, /Retry this exact change request/);
  assert.doesNotMatch(html, /<textarea/);
});

test('Hebrew feedback translates recording state and keeps the original request text literal', () => {
  const value = pin(); value.feedback = {loaded:true,rows:[{id:'request',message:'Record change request',status:'recorded'}],pending:{request:'pending',message:'משוב <script>original</script>'},sending:false,error:'Recording is unconfirmed. Retry the same request to recover its receipt. Restored fleets require recovery before recording new requests.'};
  module.exports.setLocale('he');
  try {
    let html = render({pins:[value],notice:''});
    assert.match(html, /בקשת שינויים בתוצאה השמורה הזו/);
    assert.match(html, /רישום אינו מעיד על מסירה או טיפול/);
    assert.match(html, /הרישום טרם אושר/);
    assert.match(html, /ניסיון נוסף לאותה בקשת שינוי/);
    assert.match(html, /<bdi dir="auto">Record change request<\/bdi>/);
    assert.match(html, /<bdi dir="auto">משוב &lt;script&gt;original&lt;\/script&gt;<\/bdi>/);
    assert.doesNotMatch(html, /<script>|<textarea/);
    value.feedback.sending = true;
    html = render({pins:[value],notice:''});
    assert.match(html, /רושם את בקשת השינוי המדויקת/);
    assert.match(html, /disabled=""[^>]*>ניסיון נוסף לאותה בקשת שינוי/);
    value.feedback = undefined;
    html = render({pins:[value],notice:'',persistence:{phase:'loading',message:'',editable:false}});
    assert.match(html, /השינויים המבוקשים/);
    assert.match(html, /<textarea dir="auto"[^>]*disabled=""/);
    assert.match(html, /disabled=""[^>]*>רישום בקשת שינוי/);
  } finally { module.exports.setLocale('en'); }
});

test('proposed result controls preserve the distinction from resolution and approval', () => {
  const value = pin(); value.feedback = { loaded: true, rows: [{ id: 'request', message: 'Add an example.', status: 'recorded' }], responses: [{ request: 'request', lane: value.selection.lane, checkpoint: 'revised', version: '9'.repeat(64), bundle: 'a'.repeat(64), status: 'proposed', approval_authority: false }], error: '' };
  let html = render({ pins: [value], notice: '' });
  assert.match(html, /Proposed saved result/); assert.match(html, /does not resolve the request or approve/);
  assert.match(html, /Pin proposed result beside this review/); assert.doesNotMatch(html, /Request resolved/);
  value.feedback.error = 'Historical read is unavailable.';
  html = render({ pins: [value], notice: '' }); assert.match(html, /disabled=""[^>]*>Pin proposed result beside this review/);
});

test('Hebrew proposed results preserve exact versions and feedback while exposing a separate review', () => {
  const value = pin();
  value.feedback = {loaded:true,rows:[{id:'request',message:'Proposed saved result',status:'recorded'}],responses:[{request:'request',lane:value.selection.lane,checkpoint:'revision',version:'9'.repeat(64),bundle:'a'.repeat(64),status:'proposed',approval_authority:false}],error:''};
  module.exports.setLocale('he');
  try {
    let html = render({pins:[value],notice:''});
    assert.match(html, /תוצאה שמורה שהוצעה/);
    assert.match(html, /ההצעה אינה מסיימת את הטיפול בבקשה ואינה מאשרת את התוצאה/);
    assert.match(html, /<button(?![^>]*\sdisabled=)[^>]*>הצמדת התוצאה המוצעת לצד הסקירה הזו<\/button>/);
    assert.match(html, /<bdi dir="auto">Proposed saved result<\/bdi>/);
    for (const version of [value.selection.version,value.feedback.responses[0].version]) assert.ok(html.includes(`<bdi dir="ltr">${version}</bdi>`));
    value.feedback.error = 'Saved change requests could not be verified. Retry reading the same review.';
    html = render({pins:[value],notice:''});
    assert.match(html, /לא ניתן לאמת את בקשות השינוי השמורות/);
    assert.match(html, /<button[^>]*disabled=""[^>]*>הצמדת התוצאה המוצעת לצד הסקירה הזו<\/button>/);
  } finally { module.exports.setLocale('en'); }
});

test('request decisions are explicit and reversible without changing main approval controls', () => {
  const value = pin(); value.feedback = { loaded: true, rows: [{ id: 'request', message: 'Add example.', status: 'recorded' }], responses: [{ request: 'request', lane: value.selection.lane, checkpoint: 'revised', version: '9'.repeat(64), bundle: 'a'.repeat(64), status: 'proposed', approval_authority: false }], decisions: [{ request: 'request', revision: 1, status: 'addressed', checkpoint: 'revised', version: '9'.repeat(64), bundle: 'a'.repeat(64), approval_authority: false }], error: '' };
  let html = render({ pins: [value], notice: '' });
  assert.match(html, /Request marked addressed/); assert.match(html, /Reopen change request/); assert.match(html, /not approval or integration into main/);
  assert.match(html, /disabled=""[^>]*>Mark request addressed by this result/);
  value.feedback.decisionPending = { operation: 'pending', request: 'request', expectedRevision: 1, proposedCheckpoint: null, version: null, bundle: null };
  html = render({ pins: [value], notice: '' }); assert.match(html, /Retry this exact decision/); assert.match(html, /Read latest state and choose again/);
  assert.match(html, /disabled=""[^>]*>Reopen change request/);
});


test('Hebrew request decisions retain raw feedback and disable new choices during uncertain confirmation', () => {
  const value = pin();
  value.feedback = { loaded: true, rows: [{id:'request',message:'Request open.',status:'recorded'}],
    responses:[{request:'request',lane:value.selection.lane,checkpoint:'revised',version:'9'.repeat(64),bundle:'a'.repeat(64),status:'proposed',approval_authority:false}],
    decisions:[{request:'request',revision:1,status:'addressed',checkpoint:'revised',version:'9'.repeat(64),bundle:'a'.repeat(64),approval_authority:false}],error:'' };
  module.exports.setLocale('he');
  try {
    let html = render({pins:[value],notice:''});
    assert.match(html, /הבקשה סומנה כטופלה/);
    assert.match(html, /פעולה זו אינה אישור או שילוב בגרסה הראשית/);
    assert.match(html, /<button(?![^>]*\sdisabled=)[^>]*>פתיחת בקשת השינוי מחדש<\/button>/);
    assert.match(html, /<bdi dir="auto">Request open.<\/bdi>/);
    value.feedback.decisionPending = {operation:'pending',request:'request',expectedRevision:1,proposedCheckpoint:null,version:null,bundle:null};
    value.feedback.decisionError = 'Decision is unconfirmed. Retry the same operation or read the latest state before choosing again.';
    html = render({pins:[value],notice:''});
    assert.match(html, /ההחלטה לא אומתה/);
    assert.match(html, /ניסיון נוסף לאותה החלטה/);
    assert.match(html, /קריאת המצב העדכני ובחירה מחדש/);
    assert.match(html, /<button[^>]*disabled=""[^>]*>פתיחת בקשת השינוי מחדש<\/button>/);
    value.feedback.deciding = true;
    html = render({pins:[value],notice:''});
    assert.match(html, /ממתין לאישור ההחלטה בחלון המערכת/);
    assert.match(html, /<button[^>]*disabled=""[^>]*>ניסיון נוסף לאותה החלטה<\/button>/);
  } finally { module.exports.setLocale('en'); }
});
