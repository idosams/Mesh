import test from 'node:test';
import assert from 'node:assert/strict';
import { reviewChangeMessage, savedReviewChanges, savedReviewChangeActivity, reviewChangeReceipt, reviewDecisionReceipt } from './fleet-review-changes.js';
const selection = { objective: `fleet-${'a'.repeat(64)}`, lane: 'worker', checkpoint: 'saved', version: 'b'.repeat(64), bundle: 'c'.repeat(64) };
const entry = () => ({ id: `review-change-${'d'.repeat(64)}`, lane: selection.lane, checkpoint: selection.checkpoint, version: selection.version, bundle: selection.bundle, message: 'Please add an example.', status: 'recorded', approval_authority: false });
const decision = () => ({ request: entry().id, revision: 0, status: 'open', checkpoint: null, version: null, bundle: null, approval_authority: false });
const response = () => ({ schema: 'mesh.fleet-review-changes/v3', objective: selection.objective,
  selection: Object.fromEntries(Object.entries(selection).filter(([key]) => key !== 'objective')), activity: { changes: [entry()], responses: [], decisions: [decision()] } });
test('feedback messages bound UTF-8 bytes and reject invisible controls while allowing plain lines', () => {
  assert.equal(reviewChangeMessage('Example\n\tDetail'), true);
  assert.equal(reviewChangeMessage('a'.repeat(8192)), true);
  for (const value of ['', ' \n', 'a'.repeat(8193), '😀'.repeat(2049), 'bad\0value', 'bad\rvalue', 'bad\u202evalue', 'bad\u0085value']) assert.equal(reviewChangeMessage(value), false);
});
test('saved feedback rejects substituted selections, unsupported status, duplicate IDs and extra authority', () => {
  assert.equal(savedReviewChanges(JSON.stringify(response()), selection)[0].message, entry().message);
  for (const mutate of [v => v.objective = 'other', v => v.selection.checkpoint = 'other', v => v.selection.path = '/tmp',
    v => v.activity.changes[0].lane = 'other', v => v.activity.changes[0].bundle = 'e'.repeat(64), v => v.activity.changes[0].version = 'e'.repeat(64),
    v => v.activity.changes[0].status = 'delivered', v => v.activity.changes[0].approval_authority = true, v => v.activity.changes.push(entry()), v => v.activity.changes[0].message = '\u202eunsafe', v => v.extra = true]) {
    const value = response(); mutate(value); assert.throws(() => savedReviewChanges(value, selection));
  }
});
test('acknowledgment must match the original retry identity and exact message', () => {
  const { activity, ...base } = response(); const pending = { request: 'f'.repeat(32), message: entry().message };
  const value = { ...base, schema: 'mesh.fleet-review-change-receipt/v1', request: pending.request, change: activity.changes[0] };
  assert.equal(reviewChangeReceipt(value, selection, pending).id, entry().id);
  assert.throws(() => reviewChangeReceipt({ ...value, request: 'a'.repeat(32) }, selection, pending));
  assert.throws(() => reviewChangeReceipt(value, selection, { ...pending, message: 'Different message' }));
});

const proposed = () => ({ request: entry().id, lane: selection.lane, checkpoint: 'revised', version: 'e'.repeat(64), bundle: 'f'.repeat(64), status: 'proposed', approval_authority: false });
test('proposals must refer to a loaded request and a distinct exact result in the same lane', () => {
  const valid = response(); valid.activity.responses = [proposed()];
  assert.equal(savedReviewChangeActivity(valid, selection).responses[0].checkpoint, 'revised');
  for (const mutate of [v => v.activity.responses[0].request = 'unknown', v => v.activity.responses[0].lane = 'other',
    v => v.activity.responses[0].version = selection.version, v => v.activity.responses[0].checkpoint = selection.checkpoint,
    v => v.activity.responses[0].status = 'resolved', v => v.activity.responses[0].approval_authority = true,
    v => v.activity.responses[0].path = '/tmp', v => v.activity.responses.push(proposed())]) {
    const value = structuredClone(valid); mutate(value); assert.throws(() => savedReviewChangeActivity(value, selection));
  }
  valid.activity.responses = Array.from({ length: 9 }, (_, n) => ({ ...proposed(), checkpoint: `revision-${n}` }));
  assert.throws(() => savedReviewChangeActivity(valid, selection));
});

test('decision activity requires one exact decision per request and addressed decisions bind a proposal', () => {
  const value = response(); value.activity.responses = [proposed()];
  value.activity.decisions = [{ ...decision(), revision: 1, status: 'addressed', checkpoint: 'revised', version: proposed().version, bundle: proposed().bundle }];
  assert.equal(savedReviewChangeActivity(value, selection).decisions[0].status, 'addressed');
  for (const mutate of [v => v.activity.decisions = [], v => v.activity.decisions[0].version = '0'.repeat(64),
    v => v.activity.decisions[0].request = 'unknown', v => v.activity.decisions[0].revision = 65,
    v => v.activity.decisions[0].approval_authority = true, v => v.activity.decisions[0].status = 'approved']) {
    const bad = structuredClone(value); mutate(bad); assert.throws(() => savedReviewChangeActivity(bad, selection));
  }
});
test('decision receipt recovery shows a newer reopened state rather than reapplying its older receipt', () => {
  const pending = { operation: '1'.repeat(32), request: entry().id, expectedRevision: 0, proposedCheckpoint: 'revised', version: proposed().version, bundle: proposed().bundle };
  const { activity: _activity, ...base } = response();
  const receipt = { ...decision(), revision: 1, status: 'addressed', checkpoint: 'revised', version: proposed().version, bundle: proposed().bundle };
  const value = { ...base, schema: 'mesh.fleet-review-decision/v1', operation: pending.operation, request: pending.request,
    outcome: { cancelled: false, receipt, current: { ...decision(), revision: 2 } } };
  assert.equal(reviewDecisionReceipt(value, selection, pending).current.status, 'open');
  for (const mutate of [v => v.operation = 'other', v => v.outcome.receipt.version = '0'.repeat(64),
    v => v.outcome.current.revision = 0, v => v.outcome.current.revision = 1, v => v.outcome.cancelled = true, v => v.request = 'unknown']) {
    const bad = structuredClone(value); mutate(bad); assert.throws(() => reviewDecisionReceipt(bad, selection, pending));
  }
  value.outcome = { cancelled: true, receipt: null, current: decision() };
  assert.equal(reviewDecisionReceipt(value, selection, pending).cancelled, true);
});
