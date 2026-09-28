import test from 'node:test';
import assert from 'node:assert/strict';
import { createReviewOutbox, reviewOutbox, pendingReviewEntry } from './fleet-review-outbox.js';
import { createFleetReviews } from './fleet-reviews.js';
const selection = { objective: `fleet-${'a'.repeat(64)}`, lane: 'worker', checkpoint: 'saved', version: 'b'.repeat(64), bundle: 'c'.repeat(64) };
const entry = (token = '1') => pendingReviewEntry(selection, 'change', { request: token.repeat(32), message: 'Please revise.' });
const empty = () => ({ schema: 'mesh.fleet-review-outbox/v1', revision: '0', entries: [] });
const settle = () => new Promise(resolve => setImmediate(resolve));
function storage(initial = empty()) {
  let stored = initial; const calls = []; let fail = null;
  const invoke = async (command, args) => {
    calls.push({ command, args });
    if (command === 'load_fleet_review_outbox') return structuredClone(stored);
    if (command === 'save_fleet_review_outbox') {
      if (fail === 'before') throw new Error('offline');
      const next = reviewOutbox(args.snapshot); assert.equal(next.revision, stored.revision);
      stored = { ...next, revision: String(BigInt(stored.revision) + 1n) };
      if (fail === 'after') throw new Error('lost acknowledgment');
      return structuredClone(stored);
    }
    throw new Error(`Unexpected ${command}`);
  };
  return { invoke, calls, saved: () => stored, fail: value => { fail = value; } };
}
test('closed pending schemas reject malformed selection, message, target and authority', () => {
  const valid = { ...empty(), entries: [entry()] };
  assert.equal(reviewOutbox(valid).entries.length, 1);
  for (const mutate of [v => v.entries[0].authority = true, v => v.entries[0].selection.path = '/tmp', v => v.entries[0].input.message = '\u202ebad', v => v.entries.push(v.entries[0]), v => v.revision = '01']) {
    const value = structuredClone(valid); mutate(value); assert.throws(() => reviewOutbox(value));
  }
  const decision = pendingReviewEntry(selection, 'decision', { operation: '2'.repeat(32), request: `review-change-${'d'.repeat(64)}`, expectedRevision: 2, proposedCheckpoint: null, version: null, bundle: null });
  decision.input.version = 'e'.repeat(64); assert.throws(() => reviewOutbox({ ...empty(), entries: [decision] }));
});
test('durable gate handles failed and lost acknowledgments without changing retry inputs', async () => {
  const s = storage(); const outbox = createReviewOutbox({ invoke: s.invoke, changed() {} });
  s.fail('before'); await assert.rejects(outbox.retain(entry())); assert.equal(s.saved().entries.length, 0);
  s.fail('after'); await assert.rejects(outbox.retain(entry())); assert.equal(s.saved().entries.length, 1);
  s.fail(null); await outbox.retain(entry()); assert.equal(s.saved().revision, '1');
  const changed = entry(); changed.input.message = 'Replacement'; await assert.rejects(outbox.retain(changed));
  await assert.rejects(outbox.retain(entry('2')));
  await outbox.release(entry()); assert.equal(s.saved().entries.length, 0);
});
test('parallel retention preserves independent requests and a disposed reader cannot dispatch', async () => {
  const s = storage(); const outbox = createReviewOutbox({ invoke: s.invoke, changed() {} });
  const second = entry('2'); second.selection.lane = 'second';
  await Promise.all([outbox.retain(entry()), outbox.retain(second)]);
  assert.equal(s.saved().entries.length, 2);
  outbox.dispose(); await assert.rejects(outbox.retain(entry()));
  assert.ok(s.calls.every(call => /^(load|save)_fleet_review_outbox$/.test(call.command)));
});
test('restart reads pending work without replay and explicit retry works with no open panel', async () => {
  const s = storage({ ...empty(), revision: '1', entries: [entry()] }); const sent = [];
  const h = createFleetReviews({ changed() {}, laneFor() { return null; }, invoke: async (command, args) => {
    if (command === 'load_fleet_pins') return { schema: 'mesh.desktop-fleet-pin-selectors/v2', revision: '0', pins: [] };
    if (command === 'request_fleet_review_changes') {
      sent.push(args);
      const { objective, request, message, ...selected } = args;
      return { schema: 'mesh.fleet-review-change-receipt/v1', objective, selection: selected, request, change: { ...selected, id: `review-change-${'e'.repeat(64)}`, message, status: 'recorded', approval_authority: false } };
    }
    return s.invoke(command, args);
  } });
  await h.loadSaved(); assert.equal(sent.length, 0); assert.equal(h.snapshot().reviewOutbox.entries.length, 1);
  h.handle({ type: 'retry-pending-review', operation: '1'.repeat(32) });
  h.handle({ type: 'retry-pending-review', operation: '1'.repeat(32) });
  await settle(); await settle();
  assert.deepEqual(sent, [{ ...selection, ...entry().input }]);
  assert.equal(s.saved().entries.length, 0); assert.equal(h.snapshot().reviewPins.length, 0);
  h.dispose();
});

test('change dispatch is blocked until its exact pending inputs are durably acknowledged', async () => {
  const s = storage(); let sends = 0;
  const review = { bundle: selection.bundle, subject_operation: selection.version, recorded: true, projection_authorizes_approval: false, content_complete: true,
    reviewed_head: 'd'.repeat(64), presentation_digest: 'e'.repeat(64), bundle_changes: [], bundle_changes_not_listed: 0, subject_operations_not_listed: 0, unavailable_code: null };
  const { objective, ...selected } = selection;
  const h = createFleetReviews({ changed() {}, laneFor: () => ({ base: 'd'.repeat(64) }), requestId: () => '1'.repeat(32), invoke: async (command, args) => {
    if (command === 'fleet_saved_reviews') return { schema: 'mesh.fleet-saved-reviews/v1', objective, lane: selection.lane, after: null, order: 'checkpoint-id', revision: 1, total: 1, next_after: null, reviews: [{ ...selected, run: 'run' }] };
    if (command === 'inspect_fleet_saved_review') return { schema: 'mesh.fleet-saved-review/v1', objective, selection: selected, review };
    if (command === 'request_fleet_review_changes') { sends++; throw new Error('lost response'); }
    return s.invoke(command, args);
  } });
  h.handle({ type: 'reviews', objective, lane: selection.lane }); await settle();
  h.handle({ type: 'pin-review', ...selection }); await settle();
  s.fail('before'); h.handle({ type: 'request-review-changes', pin: '1', message: 'Please revise.' }); await settle();
  assert.equal(sends, 0); assert.equal(s.saved().entries.length, 0);
  s.fail('after'); h.handle({ type: 'retry-review-changes', pin: '1' }); await settle();
  assert.equal(sends, 0); assert.equal(s.saved().entries.length, 1);
  s.fail(null); h.handle({ type: 'retry-review-changes', pin: '1' }); await settle();
  assert.equal(sends, 1); assert.equal(s.saved().entries[0].input.request, '1'.repeat(32));
  h.handle({ type: 'close-review', pin: '1' }); assert.equal(h.snapshot().reviewOutbox.entries.length, 1);
  h.dispose();
});

test('decision reconciliation removes retry inputs only after exact current activity verifies', async () => {
  const pending = pendingReviewEntry(selection, 'decision', { operation: '2'.repeat(32), request: `review-change-${'d'.repeat(64)}`, expectedRevision: 0, proposedCheckpoint: null, version: null, bundle: null });
  const s = storage({ ...empty(), revision: '1', entries: [pending] }); let fail = true;
  const { objective, ...selected } = selection;
  const h = createFleetReviews({ changed() {}, laneFor() {}, invoke: async (command, args) => {
    if (command === 'fleet_review_changes') {
      if (fail) throw new Error('offline');
      return { schema: 'mesh.fleet-review-changes/v3', objective, selection: selected, activity: {
        changes: [{ ...selected, id: pending.input.request, message: 'Revise.', status: 'recorded', approval_authority: false }], responses: [],
        decisions: [{ request: pending.input.request, revision: 0, status: 'open', checkpoint: null, version: null, bundle: null, approval_authority: false }],
      } };
    }
    return s.invoke(command, args);
  } });
  h.handle({ type: 'refresh-review-outbox' }); await settle();
  h.handle({ type: 'reconcile-pending-review', operation: pending.input.operation }); await settle();
  assert.equal(s.saved().entries.length, 1);
  fail = false; h.handle({ type: 'reconcile-pending-review', operation: pending.input.operation }); await settle();
  assert.equal(s.saved().entries.length, 0); assert.match(h.snapshot().reviewNotice, /No decision was submitted/);
  h.dispose();
});
