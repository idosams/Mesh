import test from 'node:test';
import assert from 'node:assert/strict';
import { pinnedSelections } from './renderer-proof-attached.js';
import { parseRendererProofReport } from '../scripts/renderer-proof-protocol.mjs';
test('saved selection evidence detects retargeting while allowing refreshed content', () => {
  const pin = { key: '1', project: 'a', selector: { base: 'old', target: 'saved' }, comparison: { loaded: false } };
  const initial = pinnedSelections({ pins: [pin] });
  pin.comparison = { loaded: true };
  assert.equal(pinnedSelections({ pins: [pin] }), initial);
  pin.selector.target = 'new';
  assert.notEqual(pinnedSelections({ pins: [pin] }), initial);
});
test('attachment acceptance refuses a report for another launch or restart phase', () => {
  const nonce = 'ab'.repeat(32);
  const report = { schema: 'mesh-renderer-proof/v1', nonce, surface: 'attached-projects',
    mounted: true, visible: true, interaction: 'attach-capture-pin-fork', outcome: 'parallel-saved-comparisons-retained' };
  assert.doesNotThrow(() => parseRendererProofReport(report, { nonce, surface: report.surface }));
  assert.throws(() => parseRendererProofReport(report, { nonce: 'cd'.repeat(32), surface: report.surface }));
  assert.throws(() => parseRendererProofReport(report, { nonce, surface: 'attached-projects-restart' }));
});
