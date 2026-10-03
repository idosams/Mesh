import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { build } from 'esbuild';
const output = await build({ stdin: { contents: `import React from "react"; import { renderToStaticMarkup } from "react-dom/server"; import { RemoteObservationView } from "./src/organisms/remote-observation.tsx"; import { setLocale } from "./src/lib/localization.ts"; module.exports = { setLocale, render: projection => renderToStaticMarkup(React.createElement(RemoteObservationView, { projection })) };`, resolveDir: new URL('.', import.meta.url).pathname }, bundle: true, format: 'cjs', platform: 'node', packages: 'external', write: false });
const module = { exports: {} }; Function('require', 'module', 'exports', output.outputFiles[0].text)(createRequire(import.meta.url), module, module.exports);
const projection = () => ({ selection: { id: 'opaque', host: '<script>host</script>', worker: 'worker', objective: 'fleet', lane: 'lane', run: 'run' }, busy: false, available: true, error: '', status: { observed: '1000', admitted: true, launchRecorded: true, leaseUntil: '18446744073709551615' }, results: { available: true, count: 0, revision: '0', hasMore: true } });
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
