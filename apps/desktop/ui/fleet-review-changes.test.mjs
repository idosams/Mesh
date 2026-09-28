import test from 'node:test';
import assert from 'node:assert/strict';
import { reviewChangeMessage, savedReviewChanges, reviewChangeReceipt } from './fleet-review-changes.js';
const selection = { objective: `fleet-${'a'.repeat(64)}`, lane: 'worker', checkpoint: 'saved', version: 'b'.repeat(64), bundle: 'c'.repeat(64) };
const entry = () => ({ id: `review-change-${'d'.repeat(64)}`, lane: selection.lane, checkpoint: selection.checkpoint, version: selection.version, bundle: selection.bundle, message: 'Please add an example.', status: 'recorded', approval_authority: false });
const response = () => ({ schema: 'mesh.fleet-review-changes/v1', objective: selection.objective,
  selection: Object.fromEntries(Object.entries(selection).filter(([key]) => key !== 'objective')), changes: [entry()] });
test('feedback messages bound UTF-8 bytes and reject invisible controls while allowing plain lines', () => {
  assert.equal(reviewChangeMessage('Example\n\tDetail'), true);
  assert.equal(reviewChangeMessage('a'.repeat(8192)), true);
  for (const value of ['', ' \n', 'a'.repeat(8193), '😀'.repeat(2049), 'bad\0value', 'bad\rvalue', 'bad\u202evalue', 'bad\u0085value']) assert.equal(reviewChangeMessage(value), false);
});
test('saved feedback rejects substituted selections, unsupported status, duplicate IDs and extra authority', () => {
  assert.equal(savedReviewChanges(JSON.stringify(response()), selection)[0].message, entry().message);
  for (const mutate of [v => v.objective = 'other', v => v.selection.checkpoint = 'other', v => v.selection.path = '/tmp',
    v => v.changes[0].lane = 'other', v => v.changes[0].bundle = 'e'.repeat(64), v => v.changes[0].version = 'e'.repeat(64),
    v => v.changes[0].status = 'delivered', v => v.changes[0].approval_authority = true, v => v.changes.push(entry()), v => v.changes[0].message = '\u202eunsafe', v => v.extra = true]) {
    const value = response(); mutate(value); assert.throws(() => savedReviewChanges(value, selection));
  }
});
test('acknowledgment must match the original retry identity and exact message', () => {
  const { changes, ...base } = response(); const pending = { request: 'f'.repeat(32), message: entry().message };
  const value = { ...base, schema: 'mesh.fleet-review-change-receipt/v1', request: pending.request, change: changes[0] };
  assert.equal(reviewChangeReceipt(value, selection, pending).id, entry().id);
  assert.throws(() => reviewChangeReceipt({ ...value, request: 'a'.repeat(32) }, selection, pending));
  assert.throws(() => reviewChangeReceipt(value, selection, { ...pending, message: 'Different message' }));
});
