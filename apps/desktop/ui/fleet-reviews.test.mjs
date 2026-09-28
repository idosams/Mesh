import test from 'node:test';
import assert from 'node:assert/strict';
import { savedFleetReviewPage, savedFleetReview, createFleetReviews } from './fleet-reviews.js';
const objective = `fleet-${'a'.repeat(64)}`, lane = 'lane-one';
const row = n => ({ checkpoint: `checkpoint-${String(n).padStart(3, '0')}`, version: 'b'.repeat(64), bundle: 'c'.repeat(64), run: 'run-one' });
const selection = n => ({ objective, lane, ...Object.fromEntries(Object.entries(row(n)).filter(([key]) => key !== 'run')) });
const page = (rows = [row(1)], after = null, total = rows.length, next = null) => ({ schema: 'mesh.fleet-saved-reviews/v1', objective, lane, revision: 1, order: 'checkpoint-id', after, total, next_after: next, reviews: rows });
const result = (n = 1) => ({ schema: 'mesh.fleet-saved-review/v1', objective, selection: Object.fromEntries(Object.entries(selection(n)).filter(([key]) => key !== 'objective')), review: { bundle: row(n).bundle, subject_operation: row(n).version, recorded: true, projection_authorizes_approval: false, content_complete: true, reviewed_head: 'd'.repeat(64), presentation_digest: 'e'.repeat(64), bundle_changes: [], bundle_changes_not_listed: 0, subject_operations_not_listed: 0, unavailable_code: null } });
const settle = () => new Promise(resolve => setImmediate(resolve));
function harness() {
  const calls = []; let changes = 0;
  const controller = createFleetReviews({ invoke: (command, args) => new Promise((resolve, reject) => calls.push({ command, args, resolve, reject })), laneFor: (o, l) => o === objective && l === lane ? { goal: 'Existing work', base: 'f'.repeat(64) } : null, changed: () => changes++ });
  return { ...controller, calls, changes: () => changes, open: () => controller.handle({ type: 'reviews', objective, lane }), pin: n => controller.handle({ type: 'pin-review', ...selection(n) }) };
}
test('pages bind exact lane, cursor and ordered unique results', () => {
  assert.equal(savedFleetReviewPage(JSON.stringify(page()), objective, lane, null).rows[0].objective, objective);
  for (const mutate of [v => v.objective = 'fleet-wrong', v => v.lane = 'other', v => v.after = 'different', v => v.reviews.push(row(1)), v => v.reviews[0].version = 'invalid', v => v.total = 2, v => v.next_after = 'invented', v => v.revision = -1]) {
    const value = page(); mutate(value); assert.throws(() => savedFleetReviewPage(value, objective, lane, null));
  }
  const rows = Array.from({ length: 50 }, (_, n) => row(n));
  assert.equal(savedFleetReviewPage(page(rows, null, 53, rows.at(-1).checkpoint), objective, lane, null).nextAfter, rows.at(-1).checkpoint);
  assert.throws(() => savedFleetReviewPage(page([row(1)], row(2).checkpoint), objective, lane, row(2).checkpoint));
});
test('exact results reject substitution and authority and preserve incomplete evidence', () => {
  assert.equal(savedFleetReview(result(), selection(1)).recorded, true);
  for (const mutate of [v => v.selection.checkpoint = 'other', v => v.selection.path = '/tmp', v => v.review.bundle = 'f'.repeat(64), v => v.review.recorded = false, v => v.review.projection_authorizes_approval = true, v => v.review.bundle_changes_not_listed = 1, v => v.review.reviewed_head = null, v => v.review.unavailable_code = 'missing']) {
    const value = result(); mutate(value); assert.throws(() => savedFleetReview(value, selection(1)));
  }
  const partial = result(); Object.assign(partial.review, { content_complete: false, unavailable_code: 'missing-content', reviewed_head: null, presentation_digest: null, bundle_changes_not_listed: 3 });
  assert.equal(savedFleetReview(partial, selection(1)).bundle_changes_not_listed, 3);
});
test('simultaneous pins finish independently; closing a pending pin ignores late content', async () => {
  const h = harness(); h.open(); h.calls[0].resolve(page([row(1), row(2)])); await settle();
  h.pin(1); h.pin(2);
  assert.equal(h.calls.length, 3);
  h.calls[2].resolve(result(2)); await settle();
  assert.equal(h.snapshot().reviewPins[0].loading, true);
  assert.equal(h.snapshot().reviewPins[1].review.recorded, true);
  h.handle({ type: 'close-review', pin: '1' }); h.calls[1].resolve(result(1)); await settle();
  assert.deepEqual(h.snapshot().reviewPins.map(pin => pin.selection.checkpoint), [row(2).checkpoint]);
  assert.ok(h.calls.every(call => ['fleet_saved_reviews', 'inspect_fleet_saved_review'].includes(call.command)));
});
test('pins keep exact selections across refreshed pages and retry failure', async () => {
  const h = harness(); h.open(); h.calls[0].resolve(page()); await settle(); h.pin(1); h.calls[1].resolve(result()); await settle();
  h.open(); h.calls[2].resolve(page([row(2)])); await settle();
  assert.deepEqual(h.snapshot().reviewPins[0].selection, selection(1));
  h.handle({ type: 'retry-review', pin: '1' }); assert.deepEqual(h.calls[3].args, selection(1));
  h.calls[3].reject(new Error('private native diagnostic')); await settle();
  assert.equal(h.snapshot().reviewPins[0].review.recorded, true);
  assert.doesNotMatch(h.snapshot().reviewPins[0].error, /private native/);
  h.pin(1); assert.equal(h.calls.length, 4); // Old rows cannot create new selections.
});
test('bounded pins deduplicate exact results and reject added fields or unlisted identities', async () => {
  const h = harness(); h.open(); h.calls[0].resolve(page(Array.from({ length: 9 }, (_, n) => row(n)))); await settle();
  h.handle({ type: 'pin-review', ...selection(0), path: '/tmp' }); h.pin(99); assert.equal(h.calls.length, 1);
  for (let n = 0; n < 9; n++) h.pin(n);
  assert.equal(h.snapshot().reviewPins.length, 8); assert.equal(h.calls.length, 9);
  h.pin(0); assert.equal(h.calls.length, 9); assert.match(h.snapshot().reviewNotice, /already pinned/);
});
test('closed or disposed requests cannot repopulate state and pages do not auto-retry', async () => {
  const h = harness(); h.open(); h.handle({ type: 'close-reviews', objective, lane }); h.open();
  h.calls[0].resolve(page()); await settle(); assert.equal(h.snapshot().reviewQueues[`${objective}/${lane}`].loading, true);
  h.calls[1].reject(new Error('unavailable')); await settle(); assert.equal(h.calls.length, 2);
  h.open(); const count = h.changes(); h.dispose(); h.calls[2].resolve(page()); await settle(); assert.equal(h.changes(), count);
});

function comparisonReply(n = 1, selected = null) {
  const { objective: objectiveId, ...selectedResult } = selection(n);
  const side = text => ({ path: 'note.txt', kind: 'file', bytes: 4, digest: '9'.repeat(64), executable: false, content_state: selected ? 'text' : 'not-requested', text: selected ? text : null });
  return { schema: 'mesh.fleet-starting-comparison/v1', objective: objectiveId, selection: selectedResult, approval_authority: false, input: { source_version: 'f'.repeat(64), comparison: { base: '8'.repeat(64), target: selectedResult.version, order: 'object-id', after: null, selected, total: 1, next_after: null, approval_authority: false, changes: [{ object: '7'.repeat(32), effect: 'modified', before: side('old\n'), after: side('new\n') }] } } };
}
test('recorded and input comparisons finish in either order without discarding each other', async () => {
  for (const inputFirst of [true, false]) {
    const h = harness(); h.open(); h.calls[0].resolve(page()); await settle(); h.pin(1);
    h.handle({ type: 'input-review', pin: '1' }); assert.equal(h.calls[2].command, 'inspect_fleet_starting_comparison');
    if (inputFirst) { h.calls[2].resolve(comparisonReply()); await settle(); h.calls[1].resolve(result()); }
    else { h.calls[1].resolve(result()); await settle(); h.calls[2].resolve(comparisonReply()); }
    await settle(); const pinned = h.snapshot().reviewPins[0];
    assert.equal(pinned.review.recorded, true); assert.equal(pinned.input.page.total, 1);
    assert.equal(pinned.loading, false); assert.equal(pinned.input.loading, false);
  }
});
test('comparison retries retain exact requests and ignore replies after panel close', async () => {
  const h = harness(); h.open(); h.calls[0].resolve(page()); await settle(); h.pin(1);
  h.handle({ type: 'input-review', pin: '1' }); h.calls[2].resolve(comparisonReply()); await settle();
  h.handle({ type: 'input-file', pin: '1', object: '0'.repeat(32) });
  h.handle({ type: 'input-file', pin: '1', object: '7'.repeat(32), path: '/tmp' }); assert.equal(h.calls.length, 3);
  h.handle({ type: 'input-file', pin: '1', object: '7'.repeat(32) }); h.calls[3].reject(new Error('private detail')); await settle();
  assert.match(h.snapshot().reviewPins[0].input.error, /previously verified/);
  h.handle({ type: 'retry-input', pin: '1' }); assert.deepEqual(h.calls[4].args, h.calls[3].args);
  h.handle({ type: 'close-review', pin: '1' }); h.calls[4].resolve(comparisonReply(1, '7'.repeat(32))); h.calls[1].resolve(result()); await settle();
  assert.equal(h.snapshot().reviewPins.length, 0);
});
test('two input comparisons retain independent selected objects and do not block one another', async () => {
  const h = harness(); h.open(); h.calls[0].resolve(page([row(1), row(2)])); await settle(); h.pin(1); h.pin(2);
  h.handle({ type: 'input-review', pin: '1' }); h.handle({ type: 'input-review', pin: '2' });
  h.calls[4].resolve(comparisonReply(2)); await settle(); assert.equal(h.snapshot().reviewPins[0].input.loading, true);
  h.handle({ type: 'input-file', pin: '2', object: '7'.repeat(32) }); h.calls[5].resolve(comparisonReply(2, '7'.repeat(32))); await settle();
  assert.equal(h.snapshot().reviewPins[1].input.file.after.text, 'new\n');
  h.calls[3].resolve(comparisonReply()); await settle(); assert.equal(h.snapshot().reviewPins[0].input.file, null);
  h.dispose();
});

const imageChange = () => ({ object_id: '7'.repeat(32), path_before: null, path_after: 'image.png', before: null,
  after: { kind: 'binary', version_id: '8'.repeat(64), content_digest: '9'.repeat(64) } });
function imageResult(n) { const value = result(n); value.review.bundle_changes = [imageChange()]; return value; }
function imagePreview(n) { const { objective: id, ...selected } = selection(n); return { schema: 'mesh.fleet-artifact-preview/v1', objective: id, selection: selected, object: imageChange().object_id,
  preview: { renderer: 'macos-imageio-thumbnail-v1', scope: 'representative-preview', side: 'after', ...imageChange().after, kind: 'image',
    image_data_url: 'data:image/png;base64,AAAA', text_source: null, text_lines: null, text_sections: null, text_truncated: false, page_number: null, page_count: null, rendering_authorizes_approval: false } }; }
const previewIntent = pin => ({ type: 'artifact-preview', pin, object: imageChange().object_id, page: '1' });
test('artifact panels render concurrently and closing one ignores its late reply', async () => {
  const h = harness(); h.open(); h.calls[0].resolve(page([row(1), row(2)])); await settle();
  h.pin(1); h.pin(2); h.calls[1].resolve(imageResult(1)); h.calls[2].resolve(imageResult(2)); await settle();
  h.handle(previewIntent('1')); h.handle(previewIntent('2')); h.handle(previewIntent('1'));
  assert.equal(h.calls.length, 5); assert.equal(h.calls[3].command, 'render_fleet_review_artifact');
  h.calls[4].resolve(imagePreview(2)); await settle();
  assert.equal(h.snapshot().reviewPins[0].artifact.loading, true); assert.equal(h.snapshot().reviewPins[1].artifact.envelope.after.versionId, '8'.repeat(64));
  h.handle({ type: 'close-review', pin: '1' }); h.calls[3].resolve(imagePreview(1)); await settle();
  assert.equal(h.snapshot().reviewPins.length, 1); assert.equal(h.snapshot().reviewPins[0].key, '2');
});
test('artifact selection is bounded and failures remain retryable without exposing native diagnostics', async () => {
  const h = harness(); h.open(); h.calls[0].resolve(page()); await settle(); h.pin(1); h.calls[1].resolve(imageResult(1)); await settle();
  for (const patch of [{ object: 'unknown' }, { page: '65' }, { page: '2' }, { path: '/tmp' }]) h.handle({ ...previewIntent('1'), ...patch });
  assert.equal(h.calls.length, 2);
  h.handle(previewIntent('1')); h.calls[2].reject(new Error('private native path')); await settle();
  assert.equal(h.snapshot().reviewPins[0].artifact.envelope, null); assert.doesNotMatch(h.snapshot().reviewPins[0].artifact.error, /private native/);
  h.handle(previewIntent('1')); h.calls[3].resolve(imagePreview(1)); await settle();
  assert.equal(h.snapshot().reviewPins[0].artifact.loading, false); assert.ok(h.snapshot().reviewPins[0].artifact.envelope);
});

function feedbackReceipt(call) {
  const { objective: id, request, message, ...selected } = call.args;
  return { schema: 'mesh.fleet-review-change-receipt/v1', objective: id, selection: selected, request,
    change: { ...selected, id: `review-change-${'f'.repeat(64)}`, message, status: 'recorded', approval_authority: false } };
}
test('uncertain feedback retries the exact request and does not accept a replacement draft', async () => {
  const h = harness(); h.open(); h.calls[0].resolve(page()); await settle(); h.pin(1); h.calls[1].resolve(result()); await settle();
  h.handle({ type: 'request-review-changes', pin: '1', message: 'Add the example.' });
  assert.equal(h.calls[2].command, 'request_fleet_review_changes');
  h.calls[2].reject(new Error('private native error')); await settle();
  const pending = h.snapshot().reviewPins[0].feedback.pending;
  assert.equal(pending.message, 'Add the example.'); assert.doesNotMatch(h.snapshot().reviewPins[0].feedback.error, /private native/);
  h.handle({ type: 'request-review-changes', pin: '1', message: 'Replacement' }); assert.equal(h.calls.length, 3);
  h.handle({ type: 'retry-review-changes', pin: '1' }); assert.deepEqual(h.calls[3].args, h.calls[2].args);
  h.calls[3].resolve(feedbackReceipt(h.calls[3])); await settle();
  const feedback = h.snapshot().reviewPins[0].feedback;
  assert.equal(feedback.pending, null); assert.equal(feedback.rows[0].status, 'recorded'); assert.equal(feedback.sending, false);
});
test('feedback reads are exact, and closing a panel never restores it from a late request acknowledgment', async () => {
  const h = harness(); h.open(); h.calls[0].resolve(page()); await settle(); h.pin(1); h.calls[1].resolve(result()); await settle();
  h.handle({ type: 'review-changes', pin: '1' }); assert.equal(h.calls[2].command, 'fleet_review_changes');
  const { objective: id, ...selected } = selection(1);
  h.calls[2].resolve({ schema: 'mesh.fleet-review-changes/v3', objective: id, selection: selected, activity: { changes: [], responses: [], decisions: [] } }); await settle();
  assert.equal(h.snapshot().reviewPins[0].feedback.loaded, true);
  h.handle({ type: 'request-review-changes', pin: '1', message: 'Revise.' });
  h.handle({ type: 'close-review', pin: '1' }); h.calls[3].resolve(feedbackReceipt(h.calls[3])); await settle();
  assert.equal(h.snapshot().reviewPins.length, 0);
});

test('a proposed result opens beside its original review only from verified feedback, with normal pin bounds', async () => {
  const h = harness(); h.open(); h.calls[0].resolve(page()); await settle(); h.pin(1); h.calls[1].resolve(result()); await settle();
  const original = h.snapshot().reviewPins[0].selection;
  const id = `review-change-${'f'.repeat(64)}`;
  const intent = { type: 'pin-review-response', pin: '1', request: id, checkpoint: 'revised' };
  h.handle(intent); assert.equal(h.calls.length, 2);
  h.handle({ type: 'review-changes', pin: '1' });
  const { objective: objectiveId, ...selected } = selection(1);
  h.calls[2].resolve({ schema: 'mesh.fleet-review-changes/v3', objective: objectiveId, selection: selected,
    activity: { decisions: [{ request: id, revision: 0, status: 'open', checkpoint: null, version: null, bundle: null, approval_authority: false }], changes: [{ ...selected, id, message: 'Add the example.', status: 'recorded', approval_authority: false }],
      responses: [{ request: id, lane, checkpoint: 'revised', version: '8'.repeat(64), bundle: '9'.repeat(64), status: 'proposed', approval_authority: false }] } });
  await settle();
  h.handle({ ...intent, path: '/tmp' }); h.handle({ ...intent, checkpoint: 'invented' }); assert.equal(h.calls.length, 3);
  h.handle(intent);
  assert.deepEqual(h.calls[3].args, { objective, lane, checkpoint: 'revised', version: '8'.repeat(64), bundle: '9'.repeat(64) });
  assert.equal(h.calls[3].command, 'inspect_fleet_saved_review');
  assert.strictEqual(h.snapshot().reviewPins[0].selection, original);
  assert.equal(h.snapshot().reviewPins[1].startingInput, h.snapshot().reviewPins[0].startingInput);
  h.handle(intent); assert.equal(h.snapshot().reviewPins.length, 2); assert.equal(h.calls.length, 4);
  h.calls[3].reject(new Error('missing retained result')); await settle();
  assert.equal(h.snapshot().reviewPins[0].error, ''); assert.match(h.snapshot().reviewPins[1].error, /could not be verified/);
});

async function decisionHarness() {
  const h = harness(); h.open(); h.calls[0].resolve(page()); await settle(); h.pin(1); h.calls[1].resolve(result()); await settle();
  h.handle({ type: 'review-changes', pin: '1' });
  const { objective: objectiveId, ...selected } = selection(1), request = `review-change-${'f'.repeat(64)}`;
  const current = { request, revision: 0, status: 'open', checkpoint: null, version: null, bundle: null, approval_authority: false };
  const activity = { changes: [{ ...selected, id: request, message: 'Add example.', status: 'recorded', approval_authority: false }],
    responses: [{ request, lane, checkpoint: 'revised', version: '8'.repeat(64), bundle: '9'.repeat(64), status: 'proposed', approval_authority: false }], decisions: [current] };
  const read = { schema: 'mesh.fleet-review-changes/v3', objective: objectiveId, selection: selected, activity };
  h.calls[2].resolve(read); await settle();
  const reply = (call, cancelled = false) => ({ schema: 'mesh.fleet-review-decision/v1', objective: objectiveId, selection: selected,
    request, operation: call.args.operation, outcome: { cancelled,
      receipt: cancelled ? null : { ...current, revision: 1, status: 'addressed', checkpoint: 'revised', version: '8'.repeat(64), bundle: '9'.repeat(64) },
      current: cancelled ? current : { ...current, revision: 2 } } });
  return { h, request, current, read, reply };
}
test('an uncertain decision retries exact native arguments and applies the latest recorded state', async () => {
  const { h, request, reply } = await decisionHarness();
  const intent = { type: 'decide-review-change', pin: '1', request, checkpoint: 'revised' };
  h.handle({ ...intent, checkpoint: 'invented' }); assert.equal(h.calls.length, 3);
  h.handle(intent); assert.equal(h.calls[3].command, 'decide_fleet_review_change');
  assert.equal(h.calls[3].args.expectedRevision, 0); assert.equal(h.calls[3].args.proposedCheckpoint, 'revised');
  h.calls[3].reject(new Error('private failure')); await settle();
  h.handle(intent); assert.equal(h.calls.length, 4);
  h.handle({ type: 'retry-review-decision', pin: '1' }); assert.deepEqual(h.calls[4].args, h.calls[3].args);
  h.calls[4].resolve(reply(h.calls[4])); await settle();
  const feedback = h.snapshot().reviewPins[0].feedback;
  assert.equal(feedback.decisionPending, null); assert.equal(feedback.decisions[0].status, 'open'); assert.equal(feedback.decisions[0].revision, 2);
  assert.doesNotMatch(feedback.decisionError, /private/);
});
test('cancelled confirmation clears its pending operation and a closed panel ignores late decision replies', async () => {
  const { h, request, reply } = await decisionHarness();
  const intent = { type: 'decide-review-change', pin: '1', request, checkpoint: 'revised' };
  h.handle(intent); h.calls[3].resolve(reply(h.calls[3], true)); await settle();
  assert.equal(h.snapshot().reviewPins[0].feedback.decisionPending, null);
  assert.match(h.snapshot().reviewPins[0].feedback.decisionNotice, /cancelled/);
  h.handle(intent); h.handle({ type: 'close-review', pin: '1' }); h.calls[4].resolve(reply(h.calls[4])); await settle();
  assert.equal(h.snapshot().reviewPins.length, 0);
});
test('explicit reload abandons an uncertain retry only after current decisions verify', async () => {
  const { h, request, read } = await decisionHarness();
  h.handle({ type: 'decide-review-change', pin: '1', request, checkpoint: 'revised' }); h.calls[3].reject(new Error('lost')); await settle();
  h.handle({ type: 'reload-review-decision', pin: '1' }); h.calls[4].reject(new Error('offline')); await settle();
  assert.ok(h.snapshot().reviewPins[0].feedback.decisionPending);
  h.handle({ type: 'reload-review-decision', pin: '1' }); h.calls[5].resolve(read); await settle();
  assert.equal(h.snapshot().reviewPins[0].feedback.decisionPending, null); assert.equal(h.snapshot().reviewPins[0].feedback.decisionError, '');
});
