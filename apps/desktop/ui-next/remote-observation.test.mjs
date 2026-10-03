import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { build } from 'esbuild';
const output = await build({ stdin: { contents: `import React from "react"; import { renderToStaticMarkup } from "react-dom/server"; import { RemoteObservationView } from "./src/organisms/remote-observation.tsx"; import { setLocale } from "./src/lib/localization.ts"; module.exports = { setLocale, render: projection => renderToStaticMarkup(React.createElement(RemoteObservationView, { projection })) };`, resolveDir: new URL('.', import.meta.url).pathname }, bundle: true, format: 'cjs', platform: 'node', packages: 'external', write: false });
const module = { exports: {} }; Function('require', 'module', 'exports', output.outputFiles[0].text)(createRequire(import.meta.url), module, module.exports);
const projection = () => ({ selection: { id: 'opaque', host: '<script>host</script>', worker: 'worker', objective: 'fleet', lane: 'lane', run: 'run' }, busy: false, available: true, error: '', status: { observed: '1000', admitted: true, launchRecorded: true, leaseUntil: '18446744073709551615' }, results: { available: true, count: 0, revision: '0', hasMore: true, before: 0, after: 0, entries: [] } });
test('worker panel escapes identities and distinguishes retained launch records from running agents', () => {
  const html = module.exports.render(projection());
  assert.match(html, /&lt;script&gt;host&lt;\/script&gt;/); assert.doesNotMatch(html, /<script>/);
  assert.match(html, /does not establish that the process is still running/);
  assert.match(html, /Observation time unavailable/); assert.match(html, /More saved results exist/);
  assert.doesNotMatch(html, />Start agents<|>Approve/);
});
test('busy controls disable duplicate reads and unavailable history is explicit', () => {
  const p = projection(); p.busy = true; p.error = 'stale'; p.results.available = false;
  const html = module.exports.render(p);
  assert.match(html, /role="alert"/); assert.match(html, /history is unavailable/);
  for (const button of html.matchAll(/<button([^>]*)>/g)) assert.match(button[1], /disabled=""/);
});
test('Hebrew panel translates controls while retaining literal worker identities', () => {
  module.exports.setLocale('he');
  try {
    const html = module.exports.render(projection());
    assert.match(html, /בדיקת סוכן במחשב מרוחק/);
    assert.match(html, /קריאת מצב הסוכן/);
    assert.match(html, /<bdi dir="ltr">worker<\/bdi>/);
  } finally { module.exports.setLocale('en'); }
});

test('connection setup offers native selectors and disables use until complete without credential path fields', () => {
  const html = module.exports.render(projection());
  for (const label of ['Choose coordinator identity folder', 'Choose private SSH identity', 'Choose trusted hosts file', 'Clear selected setup files']) assert.ok(html.includes(label));
  assert.match(html, /<button[^>]*disabled=""[^>]*>Use these connection settings<\/button>/);
  assert.match(html, /Use Save current connection to keep these settings for later/);
  assert.doesNotMatch(html, /type="file"|type="password"/);
});

test('saved connection controls expose metadata operations, escape labels and disable while busy', () => {
  const p = projection(); p.profiles = { revision: '4', entries: [{ id: 'a', label: '<img onerror=bad>', host: 'worker', worker: 'key', objective: 'fleet', lane: 'lane', run: 'run' }] };
  let html = module.exports.render(p);
  for (const label of ['Load saved connections', 'Open saved settings', 'Remove saved settings', 'Save current connection', 'Recover interrupted settings save']) assert.ok(html.includes(label));
  assert.match(html, /&lt;img onerror=bad&gt;/); assert.doesNotMatch(html, /<img/);
  p.busy = true; html = module.exports.render(p); for (const button of html.matchAll(/<button([^>]*)>/g)) assert.match(button[1], /disabled=""/);
  module.exports.setLocale('he'); try { assert.match(module.exports.render(p), /פתיחת הגדרות שמורות/); } finally { module.exports.setLocale('en'); }
});

test('explicit input recovery distinguishes input acceptance from an agent running', () => {
  const p = projection(); p.recovery = { disposition: 'input-retained' };
  const html = module.exports.render(p);
  assert.match(html, /Resume saved input transfer/); assert.match(html, /must still retain its reservation/);
  assert.match(html, /does not establish that an agent is running/);
});

test('remote result page exposes separate identities, escapes labels and bounds navigation', () => {
  const p = projection(); p.results = { available: true, count: 1, revision: '17', hasMore: false, before: 16, after: 17, entries: [{ offer: 'offer', checkpoint: '<script>x</script>', version: 'version', review: 'review', manifest: 'manifest' }] };
  const html = module.exports.render(p);
  assert.match(html, /Saved result<!-- --> 17|Saved result 17/);
  assert.match(html, /&lt;script&gt;x&lt;\/script&gt;/); assert.doesNotMatch(html, /<script>/);
  assert.match(html, /Listing a result does not download or accept it/);
  assert.match(html, /Download for review/);
  assert.match(html, /<button[^>]*disabled=""[^>]*>Next results<\/button>/);
  assert.match(html, /<bdi dir="ltr">version<\/bdi>/);
});


test('saved download recovery exposes review only after a verified completion and disables busy actions', () => {
  const p = projection(); p.receiptAttempts = [{ offer: 'offer', checkpoint: '<script>checkpoint</script>', version: 'version' }];
  let html = module.exports.render(p);
  assert.match(html, /Load saved downloads/); assert.match(html, /Check or resume saved download/);
  assert.match(html, /&lt;script&gt;checkpoint&lt;\/script&gt;/); assert.doesNotMatch(html, /Show downloaded reviews/);
  p.received = { offer: 'offer', correlation: 'correlation', version: 'version', review: 'review', objective: 'fleet' }; p.busy = true;
  html = module.exports.render(p);
  assert.match(html, /has not been approved or applied/);
  assert.match(html, /<button[^>]*disabled=""[^>]*>Show downloaded reviews<\/button>/);
  assert.match(html, /<button[^>]*disabled=""[^>]*>Check or resume saved download<\/button>/);
});

test('fresh creation is available without an old fleet and retained attempts remain separate from execution', () => {
  const p=projection();p.selection=null;p.status=null;p.results=null;
  p.creations=[{request:'a'.repeat(32),project:'project',version:'version',goal:'<script>goal</script>',provider:'codex',host:'worker',worker:'key',lease_until_ms:'9999999999999',limits:{lanes:4,concurrency:2,depth:1,retries:0}}];
  let html=module.exports.render(p);assert.match(html,/Prepare new remote work/);assert.match(html,/No existing attempt is needed/);assert.match(html,/Sending input authorizes the configured worker/);assert.match(html,/&lt;script&gt;goal&lt;\/script&gt;/);
  assert.match(html,/<button[^>]*disabled=""[^>]*>Send saved input to worker<\/button>/);
  p.creationStatus={request:'a'.repeat(32),kind:'prepared'};html=module.exports.render(p);assert.doesNotMatch(html,/<button[^>]*disabled=""[^>]*>Send saved input to worker<\/button>/);
  p.creationStatus.kind='sent';html=module.exports.render(p);assert.match(html,/does not establish that an agent is running/);assert.match(html,/<button[^>]*disabled=""[^>]*>Send saved input to worker<\/button>/);
  p.busy=true;html=module.exports.render(p);for(const button of html.matchAll(/<button([^>]*)>/g))assert.match(button[1],/disabled=""/);
  module.exports.setLocale('he');try{assert.match(module.exports.render(p),/בקשות שמורות ליצירת עבודה מרוחקת/);}finally{module.exports.setLocale('en');}
});

test('original input inspection is dated, explicit, localized and never presented as restart authority', () => {
  const p = projection(); p.inspection = { observed: '1001', disposition: 'verified' };
  let html = module.exports.render(p);
  assert.match(html, /Inspect original input/); assert.match(html, /verified at the observation time/);
  assert.match(html, /does not establish that an agent is running or can be restarted/);
  assert.match(html, /1970-01-01T00:00:01.001Z/);
  p.inspection.disposition = 'unrecorded'; assert.match(module.exports.render(p), /does not prove that no work was started/);
  p.inspection.disposition = 'unavailable'; assert.match(module.exports.render(p), /could not verify the original input/);
  p.busy = true; assert.match(module.exports.render(p), /<button[^>]*disabled=""[^>]*>Inspect original input<\/button>/);
  module.exports.setLocale('he'); try { assert.match(module.exports.render(p), /בדיקת הקלט המקורי/); } finally { module.exports.setLocale('en'); }
  p.selection = null; assert.doesNotMatch(module.exports.render(p), /Inspect original input/);
});

test('original recovery explicitly describes execution effects and retains observation distinctions', () => {
  const p = projection(); p.originalRecovery = { disposition: 'initialization-recovered' };
  let html = module.exports.render(p);
  assert.match(html, /may start its assigned agent/); assert.match(html, /does not prove that the agent is running/);
  assert.match(html, /does not establish that the process is still running/);
  p.busy = true; html = module.exports.render(p);
  assert.match(html, /<button[^>]*disabled=""[^>]*>Recover original worker workspace<\/button>/);
  module.exports.setLocale('he'); html = module.exports.render(p);
  assert.match(html, /שחזור סביבת העבודה המקורית/); assert.doesNotMatch(html, /Recover original worker workspace/);
  module.exports.setLocale('en'); p.selection = null;
  assert.doesNotMatch(module.exports.render(p), /Recover original worker workspace/);
});

test('recorded execution presents dated historical progress and explicit unknown, partial and stopping outcomes', () => {
  const p = projection();
  const states = { unrecorded: 'Launch recorded; progress unknown', 'setup-incomplete': 'Worker setup incomplete', running: 'Agent activity recorded', stopping: 'Stop requested; termination unconfirmed', succeeded: 'Successful completion recorded', failed: 'Failed completion recorded', cancelled: 'Cancellation completion recorded' };
  for (const [state, label] of Object.entries(states)) {
    p.execution = { observed: '1000', admitted: true, launchRecorded: true, recorded: { revision: '9223372036854775807', state } };
    const html = module.exports.render(p);
    assert.ok(html.includes(label)); assert.match(html, /1970-01-01T00:00:01.000Z/); assert.match(html, /9223372036854775807/);
    assert.match(html, /does not confirm current process activity, release capacity, or authorize another attempt/);
    assert.match(html, /Read worker status/); assert.match(html, /Inspect original input/);
  }
  p.execution.recorded = null; assert.match(module.exports.render(p), /No launch record was returned/);
  p.execution.admitted = false; assert.match(module.exports.render(p), /no admission record/);
  p.busy = true; assert.match(module.exports.render(p), /<button[^>]*disabled=""[^>]*>Read recorded execution<\/button>/);
  module.exports.setLocale('he'); try { const html = module.exports.render(p); assert.match(html, /קריאת היסטוריית הביצוע/); assert.doesNotMatch(html, /Last recorded execution/); } finally { module.exports.setLocale('en'); }
  p.selection = null; assert.doesNotMatch(module.exports.render(p), /Read recorded execution|Last recorded execution/);
});
