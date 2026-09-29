import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { build } from 'esbuild';
const output = await build({ stdin: { contents: `import React from 'react'; import { renderToStaticMarkup } from 'react-dom/server'; import { RemoteReviewPanels, RemoteSavedResults } from './src/organisms/remote-fleet-reviews.tsx'; export const render = props => renderToStaticMarkup(React.createElement(RemoteReviewPanels, props)); export const queue = props => renderToStaticMarkup(React.createElement(RemoteSavedResults, props));`, resolveDir: new URL('.', import.meta.url).pathname, loader: 'js' }, bundle: true, format: 'cjs', platform: 'node', packages: 'external', write: false });
const module = { exports: {} }; Function('require', 'module', 'exports', output.outputFiles[0].text)(createRequire(import.meta.url), module, module.exports);
const { render, queue } = module.exports;
const pin = n => ({ key: `remote-${n}`, selection: { objective: `fleet-${'a'.repeat(64)}`, offer: 'b'.repeat(64), correlation: String(n).repeat(64), lane: 'lane', run: 'run', version: 'c'.repeat(64), bundle: 'd'.repeat(64), remote_version: 'e'.repeat(64) }, review: null, loading: true, error: '', view: { object: null, mode: 'content', layout: 'split' } });
test('remote panels keep independent identities and disclose session and import limits', () => {
  const html = render({ pins: [pin(1), pin(2)], notice: '' });
  assert.equal((html.match(/aria-label="Pinned remote review"/g) ?? []).length, 2);
  assert.match(html, /this app session/); assert.match(html, /received result tree/); assert.match(html, /import into the original project are not available/);
  assert.match(html, new RegExp('1'.repeat(64))); assert.match(html, new RegExp('2'.repeat(64)));
  assert.doesNotMatch(html, /Start agents|Apply to project|Approve and/);
});
test('interrupted registration is visible without a pin action and errors are escaped', () => {
  const html = queue({ objective: pin(1).selection.objective, available: true, queue: { loading: false, error: '<script>error</script>', page: { snapshot: 1, next: null, rows: [{ sequence: 1, offer: 'b'.repeat(64), selection: null }] } } });
  assert.match(html, /Registration interrupted/); assert.doesNotMatch(html, /Pin saved review|<script>/); assert.match(html, /&lt;script&gt;/);
});
test('independent remote panels render exact verified text and reject broken content identity', () => {
  const first = pin(1), second = pin(2);
  for (const [value, text] of [[first, 'first <saved>'], [second, 'second saved']]) {
    value.loading = false;
    value.review = { bundle: value.selection.bundle, subject_operation: value.selection.version, recorded: true, content_complete: true, reviewed_head: '3'.repeat(64), presentation_digest: '6'.repeat(64), projection_authorizes_approval: false, subject_operations_not_listed: 0, bundle_changes_not_listed: 0, unavailable_code: null,
      bundle_changes: [{ object_id: 'a'.repeat(32), path_before: null, path_after: 'result.txt', effect: 'content-written', body: 'binary', before: null, after: { kind: 'binary', version_id: '4'.repeat(64), content_digest: '5'.repeat(64), byte_length: '12', line_count: null }, verified_text: { source: 'before-after', before: null, after: { version_id: '4'.repeat(64), content_digest: '5'.repeat(64) }, hunks: [{ before_start: 1, before_len: 0, after_start: 1, after_len: 1, lines: [{ kind: 'added', before: null, after: 1, text }] }] } }] };
  }
  let html = render({ pins: [first, second], notice: '' });
  assert.match(html, /first &lt;saved&gt;/); assert.match(html, /second saved/);
  first.review.bundle_changes[0].verified_text.after.content_digest = '9'.repeat(64);
  html = render({ pins: [first, second], notice: '' });
  assert.doesNotMatch(html, /first &lt;saved&gt;/); assert.match(html, /second saved/); assert.match(html, /could not be safely displayed/);
});
