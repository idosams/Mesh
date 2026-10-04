import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { build } from 'esbuild';
const output = await build({ stdin: { contents: `import React from 'react'; import { renderToStaticMarkup } from 'react-dom/server'; import { ProgressPanels, SavedProgress } from './src/organisms/fleet-progress.tsx'; export const panels = props => renderToStaticMarkup(React.createElement(ProgressPanels, props)); export const queue = props => renderToStaticMarkup(React.createElement(SavedProgress, props));`, resolveDir: new URL('.', import.meta.url).pathname, loader: 'js' }, bundle: true, format: 'cjs', platform: 'node', packages: 'external', write: false });
const module = { exports: {} };
Function('require', 'module', 'exports', output.outputFiles[0].text)(createRequire(import.meta.url), module, module.exports);
const pin = key => ({ key, selection: { objective: 'fleet', lane: 'lane', version: key.repeat(64) }, layout: 'split', input: { loading: false, error: '', file: null, page: { base: 'base', target: key.repeat(64), total: 0, changes: [], after: null, nextAfter: null } } });
test('independent progress panels identify exact versions and offer no handoff or approval controls', () => {
  const html = module.exports.panels({ pins: [pin('1'), pin('2')], notice: '' });
  assert.match(html, /aria-label="Saved progress 1"/); assert.match(html, /aria-label="Saved progress 2"/);
  assert.match(html, /Reopening verifies its exact saved history again/); assert.match(html, /1{64}/); assert.match(html, /2{64}/);
  assert.match(html, /does not approve or apply changes to main/);
  assert.doesNotMatch(html, /<button[^>]*>(Approve|Import|Record change request|Mark request)/);
});
test('progress list labels intermediate saves and refuses pinning without a verified starting version', () => {
  const props = { objective: 'fleet', lane: 'lane', available: true, queue: { loading: false, error: '', page: { total: 1, latest: 'latest', starting: null, versions: [{ version: 'a'.repeat(64), ordinal: 1 }], nextAfter: null } } };
  const html = module.exports.queue(props);
  assert.match(html, /intermediate save/); assert.match(html, /original starting version is unavailable/);
  assert.match(html, /<button[^>]*disabled=""[^>]*>Pin saved version/);
  props.queue.page.starting = 'base'; props.queue.error = '<script>unavailable</script>';
  const failed = module.exports.queue(props); assert.match(failed, /role="alert"/); assert.doesNotMatch(failed, /<script>/);
});

test('saving failure is explicit and loading disables destructive panel changes', () => {
  const pins = [pin('1')];
  const failed = module.exports.panels({ pins, notice: '', persistence: { phase: 'error', message: 'Not confirmed saved', editable: true, busy: false } });
  assert.match(failed, /Not confirmed saved/); assert.match(failed, /Retry saving progress panels/); assert.match(failed, /Reload saved progress panels/);
  const loading = module.exports.panels({ pins, notice: '', persistence: { phase: 'loading', message: '', editable: false, busy: true } });
  assert.match(loading, /<button[^>]*disabled=""[^>]*>Close progress panel/); assert.doesNotMatch(loading, /Progress panel choices saved/);
});

test('parallel saved panels show native file and folder totals with legacy unknown fallback', () => {
  const first = pin('1'), second = pin('2');
  Object.assign(first.input.page, { total: 203, fileTotal: 202, folderTotal: 1 });
  const html = module.exports.panels({ pins: [first, second], notice: '' }).replace(/<!--.*?-->/g, '');
  assert.match(html, /202 changed files · 1 changed folders/);
  assert.match(html, /0 changed objects/);
  assert.doesNotMatch(html, /0 changed files/);
});
