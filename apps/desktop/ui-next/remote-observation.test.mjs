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
