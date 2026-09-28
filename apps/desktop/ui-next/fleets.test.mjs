import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { build } from 'esbuild';
const output = await build({ stdin: { contents: `import React from "react"; import { renderToStaticMarkup } from "react-dom/server"; import { FleetCards, FleetPendingReviewOperations } from "./src/organisms/fleets.tsx"; import { setLocale } from "./src/lib/localization.ts"; module.exports.setLocale = setLocale; module.exports.pending = props => renderToStaticMarkup(React.createElement(FleetPendingReviewOperations, props)); module.exports.render = props => renderToStaticMarkup(React.createElement(FleetCards, props));`, resolveDir: new URL('.', import.meta.url).pathname, loader: 'js' }, bundle: true, format: 'cjs', platform: 'node', packages: 'external', write: false });
const module = { exports: {} };
Function('require', 'module', 'exports', output.outputFiles[0].text)(createRequire(import.meta.url), module, module.exports);
const props = () => ({ disabled: false, projects: [], projection: { error: '', fleets: [{ objective: 'fleet-one', ownership: 'current-host', cancelled: false, lanes: [{ id: 'lane-one', parent: null, goal: '<script>private goal</script>', provider: 'codex', base: 'base-version', allocated: true, sourceProject: null, run: null }] }], activity: [] } });
const render = module.exports.render;
test('fleet cards escape goals and make start explicit only for fresh provisioned work', () => {
  const value = props(), html = render(value);
  assert.match(html, /&lt;script&gt;private goal&lt;\/script&gt;/);
  assert.doesNotMatch(html, /<script>/);
  assert.match(html, /Provisioned · agents have not started/);
  assert.match(html, /<button(?![^>]*\sdisabled=)[^>]*>Start agents<\/button>/);
  value.projection.fleets[0].lanes[0].run = { id: 'attempt-one', state: 'failed' };
  assert.match(render(value), /<button[^>]*disabled=""[^>]*>Start agents<\/button>/);
});
test('activity joins the exact lane and attempt and is suppressed for restored owners', () => {
  const value = props();
  value.projection.fleets[0].lanes[0].run = { id: 'attempt-one', state: 'running' };
  value.projection.activity = [{ objective: 'fleet-one', status: 'monitoring', stopRequested: false, observedAt: '1000', workers: [{ lane: 'lane-one', run: 'other-attempt', activity: 'incorrect-worker', events: '1', observedAt: '1000' }] }];
  assert.doesNotMatch(render(value), /incorrect-worker/);
  value.projection.activity[0].workers[0].run = 'attempt-one';
  assert.match(render(value), /incorrect-worker/);
  value.projection.fleets[0].ownership = 'restored-unattached';
  const html = render(value);
  assert.doesNotMatch(html, /incorrect-worker|Monitoring agents/);
  assert.match(html, /workers need recovery/);
  assert.match(html, /Last saved state: Working/);
  assert.match(html, /<button[^>]*disabled=""[^>]*>Start agents<\/button>/);
  assert.match(html, /<button[^>]*disabled=""[^>]*>Stop agents<\/button>/);
});
test('stale observations disable starting while retaining an explicit stop request', () => {
  const value = props(); value.projection.error = 'unavailable';
  const html = render(value);
  assert.match(html, /Status may be out of date/);
  assert.match(html, /<button[^>]*disabled=""[^>]*>Start agents<\/button>/);
  assert.match(html, /<button(?![^>]*\sdisabled=)[^>]*>Stop agents<\/button>/);
});


test('incomplete allocation cannot be presented as ready to launch', () => {
  const value = props(); value.projection.fleets[0].lanes[0].allocated = false;
  const html = render(value);
  assert.match(html, /Lane allocation incomplete/);
  assert.match(html, /<button[^>]*disabled=""[^>]*>Start agents<\/button>/);
});

test('Hebrew fleet status and controls preserve native identities and user goals', () => {
  const value = props(), lane = value.projection.fleets[0].lanes[0];
  lane.goal = 'Working'; lane.provider = 'Waiting'; lane.id = 'Starting'; lane.base = 'Review'; lane.sourceProject = 'source';
  value.projects = [{ id: 'source', root: '/Users/משפחה/Working' }];
  module.exports.setLocale('he');
  try {
    let html = render(value);
    assert.match(html, /הצי הוכן · הסוכנים טרם הופעלו/);
    assert.match(html, /<button(?![^>]*\sdisabled=)[^>]*>הפעלת סוכנים<\/button>/);
    assert.match(html, /<h4 dir="auto"[^>]*>Working<\/h4>/);
    for (const literal of ['fleet-one', 'Waiting', 'Starting', 'Review', '/Users/משפחה/Working']) assert.ok(html.includes(`<bdi dir="ltr">${literal}</bdi>`), literal);
    value.projection.error = 'stale'; html = render(value);
    assert.match(html, /ייתכן שהמצב אינו עדכני/);
    assert.match(html, /<button[^>]*disabled=""[^>]*>הפעלת סוכנים<\/button>/);
    assert.match(html, /<button(?![^>]*\sdisabled=)[^>]*>עצירת סוכנים<\/button>/);
    lane.run = { id: 'attempt', state: 'running' };
    value.projection.fleets[0].ownership = 'restored-unattached'; html = render(value);
    assert.match(html, /נדרש שחזור של הסוכנים לפני הפעלה/);
    assert.match(html, /מצב שמור אחרון: עובד/);
    assert.match(html, /<button[^>]*disabled=""[^>]*>עצירת סוכנים<\/button>/);
  } finally { module.exports.setLocale('en'); }
});

test('Hebrew observations preserve raw activity and label unavailable timestamps', () => {
  const value = props();
  value.projection.fleets[0].lanes[0].run = { id: 'attempt', state: 'stopping' };
  value.projection.activity = [{ objective: 'fleet-one', status: 'needs-attention', stopRequested: true, observedAt: '1000', workers: [{ lane: 'lane-one', run: 'attempt', activity: 'Working', events: '7', observedAt: '99999999999999999999' }] }];
  module.exports.setLocale('he');
  try {
    const html = render(value);
    assert.match(html, /נדרשת בדיקה · סוכנים חדשים לא יופעלו/);
    assert.match(html, /התבקשה עצירה · השליטה נשמרת/);
    assert.match(html, /נצפה לפני \d+ שניות/);
    assert.match(html, /זמן התצפית אינו זמין/);
    assert.match(html, /<bdi dir="ltr">Working<\/bdi> · 7 אירועים/);
  } finally { module.exports.setLocale('en'); }
});

test('unconfirmed provisioning freezes new input and exposes exact input and explicit retry in either language', async () => {
  const projection = { fleets: [], activity: [], pending: { id: 'source', version: 'Review', goal: 'Working', limits: { lanes: 4, concurrency: 2, depth: 1 } }, busy: false, available: true, error: '', feedback: 'Fleet provisioned. Review it below, then choose Start agents.' };
  const output = await build({ stdin: { contents: `import React from "react"; import { renderToStaticMarkup } from "react-dom/server"; import { Fleets } from "./src/organisms/fleets.tsx"; export { setLocale } from "./src/lib/localization.ts"; export const render = () => renderToStaticMarkup(React.createElement(Fleets, {projects: [{id:"source", root:"/Users/משפחה/Working", savedVersion:"Review"}], histories:{}, sourceError:""}));`, resolveDir: new URL('.', import.meta.url).pathname, loader: 'js' }, bundle: true, format: 'cjs', platform: 'node', packages: 'external', write: false, plugins: [{ name: 'pending-native-projection', setup(builder) { builder.onLoad({ filter: /fleets\.tsx$/ }, ({path}) => ({ contents: readFileSync(path, 'utf8').replace('useState<Projection>(empty)', `useState<Projection>(${JSON.stringify(projection)})`), loader:'tsx' })); } }] });
  const module = {exports:{}};
  Function('require','module','exports',output.outputFiles[0].text)(createRequire(import.meta.url),module,module.exports);
  for (const [locale, retry, provision, pending] of [['en','Retry this provisioning request','Provision fleet','Provisioning not yet confirmed'], ['he','ניסיון נוסף לאותה בקשת הכנה','הכנת צי','ההכנה טרם אושרה']]) {
    module.exports.setLocale(locale);
    const html = module.exports.render();
    assert.ok(html.includes(pending));
    assert.match(html, /<bdi dir="ltr">Review<\/bdi>/);
    assert.match(html, /<p dir="auto"[^>]*>Working<\/p>/);
    assert.match(html, /<select dir="ltr"[^>]*disabled=""/);
    assert.match(html, /<textarea dir="auto"[^>]*disabled=""/);
    assert.match(html, new RegExp(`<button[^>]*disabled=""[^>]*>${provision}</button>`));
    assert.match(html, new RegExp(`<button(?![^>]*\\sdisabled=)[^>]*>${retry}</button>`));
    assert.ok(html.includes('/Users/משפחה/Working'));
    if(locale === 'he') {
      assert.match(html, /4 מסלולים · 2 סוכנים בו־זמנית · עומק 1/);
      assert.match(html, /הצי הוכן. בדקו אותו למטה/);
    }
  }
});

test('restored history can be reviewed while start and stop remain disabled', () => {
  const value = props(); value.projection.available = true; value.projection.fleets[0].ownership = 'restored-unattached';
  let html = render(value);
  assert.match(html, /<button(?![^>]*\sdisabled=)[^>]*>Show saved results<\/button>/);
  assert.match(html, /<button[^>]*disabled=""[^>]*>Start agents<\/button>/);
  assert.match(html, /<button[^>]*disabled=""[^>]*>Stop agents<\/button>/);
  value.projection.fleets[0].ownership = 'unavailable'; html = render(value);
  assert.match(html, /<button[^>]*disabled=""[^>]*>Show saved results<\/button>/);
});


test('Hebrew restored history remains reviewable while execution controls stay disabled', () => {
  const value = props(); value.projection.available = true; value.projection.fleets[0].ownership = 'restored-unattached';
  module.exports.setLocale('he');
  try {
    let html = render(value);
    assert.match(html, /<button(?![^>]*\sdisabled=)[^>]*>הצגת תוצאות שמורות<\/button>/);
    assert.match(html, /<button[^>]*disabled=""[^>]*>הפעלת סוכנים<\/button>/);
    assert.match(html, /<button[^>]*disabled=""[^>]*>עצירת סוכנים<\/button>/);
    assert.ok(html.includes('<bdi dir="ltr">fleet-one</bdi>'));
    value.projection.fleets[0].ownership = 'unavailable'; html = render(value);
    assert.match(html, /<button[^>]*disabled=""[^>]*>הצגת תוצאות שמורות<\/button>/);
  } finally { module.exports.setLocale('en'); }
});

test('pending operations retain exact decision inputs and explicit recovery actions', () => {
  const value = { busy: false, loaded: true, error: '', entries: [{ kind: 'decision', objective: 'fleet-test', selection: { lane: '<lane>', checkpoint: 'original', version: 'original-version' }, input: { operation: 'token', request: 'request', expected_revision: '2', checkpoint: 'revised', version: 'proposed-version', bundle: 'proposed-review' } }] };
  let html = module.exports.pending({ value });
  assert.match(html, /&lt;lane&gt;/); assert.match(html, /proposed-version/); assert.match(html, /proposed-review/);
  assert.match(html, /Refresh reads only/); assert.match(html, /Retry this exact operation/); assert.match(html, /Read current decisions and stop retrying/);
  value.busy = true; html = module.exports.pending({ value });
  assert.match(html, /<button[^>]*disabled=""[^>]*>Retry this exact operation/);
  value.loaded = false; value.entries = []; value.error = 'Unavailable'; html = module.exports.pending({ value });
  assert.doesNotMatch(html, /No pending operations/); assert.match(html, /Unavailable/);
});

test('Hebrew pending recovery keeps exact inputs literal and disables dispatch while busy', () => {
  const value = { busy: true, loaded: true, error: '', entries: [{ kind: 'change', objective: 'fleet-test', selection: { lane: 'Pending review operations', checkpoint: 'saved', version: 'version-id' }, input: { request: 'request-id', message: 'Refresh pending operations' } }] };
  module.exports.setLocale('he');
  try {
    const html = module.exports.pending({value});
    assert.match(html, /aria-label="פעולות בדיקה ממתינות"/);
    assert.match(html, /<bdi dir="ltr">Pending review operations<\/bdi>/);
    assert.match(html, /<p dir="auto"[^>]*>Refresh pending operations<\/p>/);
    assert.match(html, /<button[^>]*disabled=""[^>]*>ניסיון נוסף לאותה פעולה בדיוק<\/button>/);
    assert.match(html, /רענון קורא בלבד/);
  } finally { module.exports.setLocale('en'); }
});
