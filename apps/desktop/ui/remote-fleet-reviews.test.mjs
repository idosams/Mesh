import test from 'node:test';
import assert from 'node:assert/strict';
import { remoteReviewPage, remoteReview, createRemoteFleetReviews } from './remote-fleet-reviews.js';
const objective = `fleet-${'a'.repeat(64)}`;
const selector = n => ({ offer: String(n).repeat(64), correlation: 'b'.repeat(64), lane: 'lane', run: 'run', version: 'c'.repeat(64), bundle: 'd'.repeat(64), remote_version: 'e'.repeat(64) });
const page = () => ({ schema: 'mesh.desktop-remote-reviews/v1', objective, after: 0, snapshot: 2, next: null, entries: [1,2].map(n => ({ sequence: n, offer: selector(n).offer, selection: selector(n) })) });
const result = n => ({ schema: 'mesh.remote-saved-review/v1', objective, ...selector(n), comparison_basis: 'received-result-tree', approval_authority: false, review: { bundle: selector(n).bundle, subject_operation: selector(n).version, recorded: true, content_complete: true, reviewed_head: 'e'.repeat(64), presentation_digest: 'f'.repeat(64), bundle_changes: [], bundle_changes_not_listed: 0, subject_operations_not_listed: 0, unavailable_code: null, projection_authorizes_approval: false } });
const settle = () => new Promise(resolve => setImmediate(resolve));
function harness(otherPinCount = () => 0) {
  const calls = [];
  const controller = createRemoteFleetReviews({ invoke: (command, args) => new Promise((resolve, reject) => calls.push({ command, args, resolve, reject })), changed() {}, otherPinCount });
  return { calls, controller, open: () => controller.handle({ type: 'remote-results', objective }), pin: n => controller.handle({ type: 'remote-pin', objective, offer: selector(n).offer, correlation: selector(n).correlation }) };
}
test('bounded native page binds fixed snapshot and exposes interrupted registration', () => {
  const value = page(); value.entries[1].selection = null;
  assert.equal(remoteReviewPage(value, objective, 0, null).rows[1].selection, null);
  for (const mutate of [v => v.next = 2, v => v.snapshot = 4097, v => v.entries[0].sequence = 3, v => v.entries[0].selection.offer = 'f'.repeat(64), v => v.entries[0].selection.path = '/tmp/private']) {
    const bad = page(); mutate(bad); assert.throws(() => remoteReviewPage(bad, objective, 0, null));
  }
  assert.throws(() => remoteReviewPage(page(), objective, 0, 3));
});
test('remote review cannot substitute another correlation, snapshot or approval', () => {
  assert.equal(remoteReview(result(1), { objective, ...selector(1) }).bundle, selector(1).bundle);
  for (const mutate of [v => v.correlation = 'f'.repeat(64), v => v.version = 'f'.repeat(64), v => v.comparison_basis = 'original-project', v => v.approval_authority = true, v => v.review.projection_authorizes_approval = true]) {
    const bad = result(1); mutate(bad); assert.throws(() => remoteReview(bad, { objective, ...selector(1) }));
  }
});
test('two pending pins finish independently and closing a pin discards late content', async () => {
  const h = harness(); h.open(); h.calls[0].resolve(page()); await settle(); h.pin(1);
  // Distinct offers with the same fixture correlation are rejected as duplicates; use an exact distinct correlation.
  const revised = page(); revised.entries[1].selection.correlation = '9'.repeat(64);
  h.open(); h.calls[2].resolve(revised); await settle();
  h.controller.handle({ type: 'remote-pin', objective, offer: selector(2).offer, correlation: '9'.repeat(64) });
  assert.equal(h.controller.snapshot().remoteReviewPins.length, 2);
  const [first, second] = h.controller.snapshot().remoteReviewPins;
  const answer = result(2); answer.correlation = '9'.repeat(64); h.calls[3].resolve(answer); await settle();
  assert.equal(h.controller.snapshot().remoteReviewPins[1].loading, false);
  assert.equal(h.controller.snapshot().remoteReviewPins[0].loading, true);
  h.controller.handle({ type: 'remote-close', pin: first.key }); h.calls[1].resolve(result(1)); await settle();
  assert.deepEqual(h.controller.snapshot().remoteReviewPins.map(p => p.key), [second.key]);
  assert.ok(h.calls.every(c => ['remote_fleet_reviews','inspect_remote_fleet_review'].includes(c.command)));
});
test('shared capacity and interrupted records never create additional pins', async () => {
  const h = harness(() => 8); h.open(); const value = page(); value.entries[1].selection = null; h.calls[0].resolve(value); await settle(); h.pin(1); h.pin(2);
  assert.equal(h.controller.snapshot().remoteReviewPins.length, 0); assert.equal(h.calls.length, 1);
});
test('closing a list prevents a late page from restoring it', async () => {
  const h = harness(); h.open(); h.controller.handle({ type: 'remote-results-close', objective }); h.calls[0].resolve(page()); await settle();
  assert.equal(h.controller.snapshot().remoteReviewQueues[objective], undefined);
});

test('remote artifact binds correlation, exact file, side and rendered page', async () => {
  const { remoteArtifactSide } = await import('./remote-fleet-reviews.js');
  const selection = { objective, ...selector(1) };
  const summary = { kind: 'binary', version_id: '8'.repeat(64), content_digest: '9'.repeat(64) };
  const change = { object_id: 'f'.repeat(32), path_before: null, path_after: 'slide.pdf', before: null, after: summary };
  const answer = () => ({ schema: 'mesh.remote-artifact-preview/v1', objective, offer: selection.offer, correlation: selection.correlation, object: change.object_id,
    preview: { renderer: 'macos-pdfkit-page-v1', scope: 'exact-page-preview', kind: 'pdf', side: 'after', version_id: summary.version_id, content_digest: summary.content_digest, image_data_url: 'data:image/png;base64,AAAA', text_source: null, text_lines: null, text_sections: null, text_truncated: false, page_number: 1, page_count: 4, rendering_authorizes_approval: false } });
  assert.equal(remoteArtifactSide(answer(), selection, change, 'after', 'pdf', 1).pageCount, 4);
  for (const mutate of [v => v.offer = 'f'.repeat(64), v => v.correlation = 'f'.repeat(64), v => v.preview.content_digest = '0'.repeat(64), v => v.preview.page_number = 2, v => v.preview.image_data_url = 'file:///tmp/private', v => v.preview.rendering_authorizes_approval = true]) {
    const bad = answer(); mutate(bad); assert.throws(() => remoteArtifactSide(bad, selection, change, 'after', 'pdf', 1));
  }
});

test('independent native history reads remain bounded and preserve existing pinned selections', async () => {
  const h=harness();h.open();h.calls[0].resolve(page());await settle();h.pin(1);h.calls[1].resolve(result(1));await settle();
  const original=h.controller.snapshot().remoteReviewPins[0];
  for(let n=1;n<=17;n++)h.controller.handle({type:'remote-results',objective:`fleet-${n.toString(16).padStart(64,'0')}`});
  assert.equal(Object.keys(h.controller.snapshot().remoteReviewQueues).length,16);
  assert.equal(h.calls.filter(c=>c.command==='remote_fleet_reviews').length,16);
  assert.deepEqual(h.controller.snapshot().remoteReviewPins[0],original);
  for(const call of h.calls.slice(2))call.reject(new Error('history unavailable'));await settle();
  assert.deepEqual(h.controller.snapshot().remoteReviewPins[0],original);
});
