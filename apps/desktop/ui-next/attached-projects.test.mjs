import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import test from 'node:test';
import { build } from 'esbuild';

async function render(projection) {
  const output = await build({
    stdin: { contents: `import React from 'react'; import { renderToStaticMarkup } from 'react-dom/server'; import { AttachedProjects } from './src/organisms/attached-projects.tsx'; export const render = () => renderToStaticMarkup(React.createElement(AttachedProjects));`, resolveDir: new URL('.', import.meta.url).pathname, loader: 'js' },
    bundle: true, format: 'cjs', platform: 'node', packages: 'external', write: false,
    plugins: [{ name: 'received-native-projection', setup(builder) {
      builder.onLoad({ filter: /attached-projects\.tsx$/ }, ({ path }) => {
        const source = readFileSync(path, 'utf8');
        assert.ok(source.includes('useState<Projection>(empty)'));
        return { contents: source.replace('useState<Projection>(empty)', `useState<Projection>(${JSON.stringify(projection)})`), loader: 'tsx' };
      });
    } }],
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', output.outputFiles[0].text)(createRequire(import.meta.url), module, module.exports);
  return module.exports.render();
}
const projection = () => ({
  available: true, busy: false, error: '', pinStatus: 'saved', pinError: '', pins: [],
  bases: {}, comparisons: {}, inspections: {}, histories: {},
  projects: [{ id: 'original-registration', generation: '1', root: '/existing/project', phase: 'stopped', outcome: 'store-unavailable', recovery: 'unavailable', savedVersion: null, captureAgeMs: null }],
});
function button(html, label) {
  const match = [...html.matchAll(/<button\b([^>]*)>(.*?)<\/button>/gs)].find(item => item[2] === label);
  assert.ok(match, `Missing actual rendered button: ${label}`);
  return { disabled: /\bdisabled=/.test(match[1]) };
}
test('saved history can be looked up without capture readiness or a latest-version pointer', async () => {
  const value = projection(); const html = await render(value);
  assert.equal(button(html, 'Show latest versions').disabled, false);
  assert.equal(button(html, 'Capture now').disabled, true);
  assert.equal(button(html, 'Show review requests').disabled, true);
  assert.match(html, /Saved version unavailable until history is verified/);
  assert.doesNotMatch(html, /Latest saved version:/);
  value.projects[0].detached = true;
  assert.equal(button(await render(value), 'Show latest versions').disabled, false);
});
test('history lookup still obeys busy, unavailable and stale native session boundaries', async () => {
  for (const change of [{ busy: true }, { available: false }, { error: 'Native session unavailable' }]) {
    const value = { ...projection(), ...change };
    assert.equal(button(await render(value), 'Show latest versions').disabled, true);
  }
});
