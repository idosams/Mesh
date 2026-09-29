import test from 'node:test';
import assert from 'node:assert/strict';
import { remotePinSnapshot } from './remote-fleet-pin-persistence.js';
import { createRemoteFleetReviews } from './remote-fleet-reviews.js';
const schema = 'mesh.desktop-remote-fleet-pin-selectors/v1', objective = `fleet-${'a'.repeat(64)}`;
const selection = { objective, offer: 'b'.repeat(64), correlation: 'c'.repeat(64), lane: 'lane', run: 'run', version: 'd'.repeat(64), bundle: 'e'.repeat(64), remote_version: 'f'.repeat(64) };
const pin = () => ({ key: '1', ...selection, object: null, mode: 'content', layout: 'split' });
const snapshot = (pins = []) => ({ schema, revision: '0', pins });
const settle = () => new Promise(resolve => setImmediate(resolve));
const result = () => ({ schema: 'mesh.remote-saved-review/v1', ...selection, comparison_basis: 'received-result-tree', approval_authority: false, review: { bundle: selection.bundle, subject_operation: selection.version, recorded: true, content_complete: true, reviewed_head: '1'.repeat(64), presentation_digest: '2'.repeat(64), bundle_changes: [], bundle_changes_not_listed: 0, subject_operations_not_listed: 0, unavailable_code: null, projection_authorizes_approval: false } });
test('remote selector schema rejects content, malformed identities, duplicate selectors and unknown versions', () => {
  assert.equal(remotePinSnapshot(snapshot([pin()])).pins.length, 1);
  for (const mutate of [v => v.schema = 'future', v => v.revision = '01', v => v.pins[0].path = '/tmp/private', v => v.pins[0].object = 'live', v => v.pins[0].mode = 'approve', v => v.pins[0].correlation = 'C'.repeat(64), v => v.pins.push({ ...pin(), key: '2' })]) {
    const bad = snapshot([pin()]); mutate(bad); assert.throws(() => remotePinSnapshot(bad));
  }
});
test('restart restores exact remote selectors while unavailable history neither launches nor deletes them', async () => {
  const stored = snapshot([pin()]), calls = [];
  const c = createRemoteFleetReviews({ invoke: async (command, args) => { calls.push({ command, args }); if (command === 'load_remote_fleet_pins') return stored; throw new Error('history unavailable'); }, changed() {}, objectiveFor: () => null });
  await c.loadSaved(); await settle();
  const state = c.snapshot(); assert.equal(state.remoteReviewPins.length, 1); assert.deepEqual(state.remoteReviewPins[0].selection, selection); assert.match(state.remoteReviewPins[0].error, /unavailable/);
  assert.deepEqual(calls.map(c => c.command), ['load_remote_fleet_pins','inspect_remote_fleet_review']);
  assert.deepEqual(calls[1].args, { objective, offer: selection.offer, correlation: selection.correlation });
  assert.equal(state.remoteReviewPersistence.phase, 'saved'); c.dispose();
});
test('lost save acknowledgement is reconciled without duplicate publication and pins store no review content', async () => {
  let stored = snapshot(), saves = 0, lose = true;
  const c = createRemoteFleetReviews({ invoke: async (command, args) => {
    if (command === 'load_remote_fleet_pins') return stored;
    if (command === 'save_remote_fleet_pins') { saves++; const requested = remotePinSnapshot(args.snapshot); stored = { ...requested, revision: String(BigInt(stored.revision) + 1n) }; if (lose) { lose = false; throw new Error('lost reply'); } return stored; }
    if (command === 'remote_fleet_reviews') return { schema: 'mesh.desktop-remote-reviews/v1', objective, after: 0, snapshot: 1, next: null, entries: [{ sequence: 1, offer: selection.offer, selection: Object.fromEntries(Object.entries(selection).filter(([k]) => k !== 'objective')) }] };
    if (command === 'inspect_remote_fleet_review') return result(); throw new Error(command);
  }, changed() {}, objectiveFor: () => true });
  await c.loadSaved(); c.handle({ type: 'remote-results', objective }); await settle(); c.handle({ type: 'remote-pin', objective, offer: selection.offer, correlation: selection.correlation }); await settle();
  assert.equal(c.snapshot().remoteReviewPersistence.phase, 'error'); assert.equal(c.snapshot().remoteReviewPins.length, 1);
  assert.deepEqual(stored.pins, [pin()]); assert.doesNotMatch(JSON.stringify(stored), /bundle_changes|preview|content_digest/);
  c.handle({ type: 'remote-pins-retry' }); await settle(); assert.equal(saves, 1); assert.equal(c.snapshot().remoteReviewPersistence.phase, 'saved');
  c.dispose();
});
test('unknown stored data freezes edits without replacing the saved record', async () => {
  let writes = 0;
  const c = createRemoteFleetReviews({ invoke: async command => { if (command === 'load_remote_fleet_pins') return { schema: 'future', pins: [] }; writes++; throw new Error(command); }, changed() {}, objectiveFor: () => true });
  await c.loadSaved(); assert.equal(c.snapshot().remoteReviewPersistence.editable, false);
  c.handle({ type: 'remote-pin', objective, offer: selection.offer, correlation: selection.correlation }); await settle(); assert.equal(writes, 0); c.dispose();
});
test('concurrent saved changes are preserved until explicit reload', async () => {
  let stored = snapshot([pin()]), saves = 0;
  const c = createRemoteFleetReviews({ invoke: async (command) => {
    if (command === 'load_remote_fleet_pins') return stored;
    if (command === 'inspect_remote_fleet_review') throw new Error('unavailable');
    if (command === 'save_remote_fleet_pins') { saves++; throw new Error('concurrent change'); }
    throw new Error(command);
  }, changed() {}, objectiveFor: () => true });
  await c.loadSaved(); await settle(); c.handle({ type: 'remote-close', pin: 'remote-1' }); await settle();
  stored = { schema, revision: '2', pins: [{ ...pin(), key: '2', correlation: '9'.repeat(64) }] };
  c.handle({ type: 'remote-pins-retry' }); await settle(); assert.equal(saves, 1); assert.equal(c.snapshot().remoteReviewPins.length, 0); assert.equal(c.snapshot().remoteReviewPersistence.phase, 'error');
  c.handle({ type: 'remote-pins-reload' }); await settle(); assert.equal(c.snapshot().remoteReviewPins[0].selection.correlation, '9'.repeat(64)); assert.equal(saves, 1); c.dispose();
});
