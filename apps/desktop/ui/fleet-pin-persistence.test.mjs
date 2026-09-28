import test from 'node:test';
import assert from 'node:assert/strict';
import { fleetPinSnapshot, createFleetPinPersistence } from './fleet-pin-persistence.js';
import { createFleetReviews } from './fleet-reviews.js';
const schema = 'mesh.desktop-fleet-pin-selectors/v2';
const pin = () => ({ candidate: null, key: '1', objective: `fleet-${'a'.repeat(64)}`, lane: 'worker', checkpoint: 'checkpoint', version: 'b'.repeat(64), bundle: 'c'.repeat(64), source_version: 'd'.repeat(64), input_after: null, input_object: 'e'.repeat(32), input_open: true, input_layout: 'split', review_object: 'f'.repeat(32), review_mode: 'content', review_layout: 'inline' });
const snapshot = (pins = [], revision = '0') => ({ schema, revision, pins });
const settle = () => new Promise(resolve => setImmediate(resolve));
function storeHarness(invoke) {
  let pins = []; const statuses = [];
  const store = createFleetPinPersistence({ invoke, selectors: () => pins, restore: values => { pins = values; }, status: (...state) => statuses.push(state) });
  return { store, statuses, set(values) { pins = values; store.changed(); }, get: () => pins };
}
test('closed selector schema rejects content, duplicates, invalid cursors and numeric widening', () => {
  assert.deepEqual(fleetPinSnapshot(JSON.stringify(snapshot([pin()]))).pins, [pin()]);
  for (const patch of [{ key: '18446744073709551616' }, { key: '01' }, { text: 'private content' }, { input_object: '../file' }, { input_open: false }, { review_layout: 'unified' }, { review_mode: 'approve' }, { objective: 'unknown' }, { source_version: 'live' }]) assert.throws(() => fleetPinSnapshot(snapshot([{ ...pin(), ...patch }])));
  assert.throws(() => fleetPinSnapshot(snapshot([pin(), { ...pin(), key: '2' }])));
  assert.throws(() => fleetPinSnapshot({ ...snapshot(), extra: true }));
});
test('closing during a pending write persists the removal after the earlier acknowledgement', async () => {
  const writes = []; let finish;
  const h = storeHarness(async (command, args) => {
    if (command === 'load_fleet_pins') return snapshot();
    assert.equal(command, 'save_fleet_pins'); const value = JSON.parse(args.snapshot); writes.push(value);
    if (writes.length === 1) return new Promise(resolve => { finish = () => resolve({ ...value, revision: '1' }); });
    return { ...value, revision: '2' };
  });
  await h.store.ensureLoaded(); h.set([pin()]); h.set([]); finish(); await settle();
  assert.deepEqual(writes.map(value => value.pins), [[pin()], []]); assert.equal(writes[1].revision, '1'); assert.equal(h.statuses.at(-1)[0], 'saved');
});
test('lost acknowledgement recovers without rewriting; a conflicting saved set is preserved', async () => {
  let stored = snapshot(), writes = 0;
  const h = storeHarness(async (command, args) => {
    if (command === 'load_fleet_pins') return stored;
    writes++; stored = { ...JSON.parse(args.snapshot), revision: String(writes) }; throw new Error('lost reply');
  });
  await h.store.ensureLoaded(); h.set([pin()]); await settle(); await h.store.retry(); assert.equal(writes, 1); assert.equal(h.statuses.at(-1)[0], 'saved');
  h.set([]); await settle(); stored = snapshot([{ ...pin(), checkpoint: 'different' }], '3');
  await h.store.retry(); assert.equal(writes, 2); assert.equal(h.statuses.at(-1)[0], 'error'); assert.deepEqual(h.get(), []);
  await h.store.reload(); assert.equal(h.get()[0].checkpoint, 'different');
});
function reviewReply(p) { return { schema: 'mesh.fleet-saved-review/v1', objective: p.objective, selection: Object.fromEntries(['lane', 'checkpoint', 'version', 'bundle'].map(key => [key, p[key]])), review: { bundle: p.bundle, subject_operation: p.version, recorded: true, projection_authorizes_approval: false, content_complete: true, reviewed_head: '1'.repeat(64), presentation_digest: '2'.repeat(64), bundle_changes: [], bundle_changes_not_listed: 0, subject_operations_not_listed: 0, unavailable_code: null } }; }
function inputReply(p, after, selected) { return { schema: 'mesh.fleet-starting-comparison/v1', objective: p.objective, selection: Object.fromEntries(['lane', 'checkpoint', 'version', 'bundle'].map(key => [key, p[key]])), approval_authority: false, input: { source_version: p.source_version, comparison: { base: '3'.repeat(64), target: p.version, order: 'object-id', after, selected, total: 1, next_after: null, approval_authority: false, changes: [{ object: selected ?? '4'.repeat(32), effect: 'added', before: null, after: { path: 'note.txt', kind: 'file', digest: '5'.repeat(64), bytes: 4, executable: false, content_state: selected ? 'text' : 'not-requested', text: selected ? 'new\n' : null } }] } } }; }
test('restoration rechecks exact content, including an object outside the saved page, without writes or execution', async () => {
  const p = pin(), calls = [];
  const h = createFleetReviews({ laneFor: () => null, changed() {}, invoke: async (command, args) => {
    if (command === 'load_fleet_review_outbox') return { schema: 'mesh.fleet-review-outbox/v1', revision: '0', entries: [] };
    calls.push({ command, args });
    if (command === 'load_fleet_pins') return snapshot([p], '7');
    if (command === 'inspect_fleet_saved_review') return reviewReply(p);
    if (command === 'inspect_fleet_starting_comparison') return inputReply(p, args.after, args.selected);
    throw new Error('Unexpected mutation');
  } });
  await h.loadSaved(); await settle(); const restored = h.snapshot().reviewPins[0];
  assert.equal(restored.view.review_object, p.review_object); assert.equal(restored.view.input_layout, 'split');
  assert.equal(restored.input.file.object, p.input_object); assert.equal(restored.input.file.after.text, 'new\n');
  assert.equal(restored.review.recorded, true); assert.equal(h.snapshot().reviewPersistence.phase, 'saved');
  assert.deepEqual(calls.map(call => call.command).sort(), ['inspect_fleet_saved_review', 'inspect_fleet_starting_comparison', 'inspect_fleet_starting_comparison', 'load_fleet_pins'].sort());
  h.dispose();
});
test('unavailable history keeps selectors, and closing the last restored panel persists empty state', async () => {
  const p = pin(); let stored = snapshot([p], '1'); const writes = [];
  const h = createFleetReviews({ laneFor: () => null, changed() {}, invoke: async (command, args) => {
    if (command === 'load_fleet_review_outbox') return { schema: 'mesh.fleet-review-outbox/v1', revision: '0', entries: [] };
    if (command === 'load_fleet_pins') return stored;
    if (command === 'save_fleet_pins') { const value = JSON.parse(args.snapshot); writes.push(value); stored = { ...value, revision: '2' }; return stored; }
    throw new Error('History unavailable');
  } });
  await h.loadSaved(); await settle(); assert.equal(h.snapshot().reviewPins.length, 1); assert.match(h.snapshot().reviewPins[0].error, /selection is retained/); assert.equal(writes.length, 0);
  h.handle({ type: 'close-review', pin: '1' }); await settle(); assert.deepEqual(writes[0].pins, []); assert.equal(writes[0].revision, '1'); h.dispose();
});
test('failed initial load cannot overwrite saved state, and disposal does not dispatch follow-up reads', async () => {
  let complete; const calls = [];
  const h = createFleetReviews({ laneFor: () => null, changed() {}, invoke: async (command, args) => {
    if (command === 'load_fleet_review_outbox') return { schema: 'mesh.fleet-review-outbox/v1', revision: '0', entries: [] };
    calls.push(command);
    if (command === 'load_fleet_pins') return snapshot([pin()], '1');
    if (command === 'inspect_fleet_saved_review') throw new Error('Unavailable');
    return new Promise(resolve => { complete = () => resolve(inputReply(pin(), args.after, args.selected)); });
  } });
  await h.loadSaved(); h.dispose(); complete(); await settle(); assert.equal(calls.filter(command => command === 'inspect_fleet_starting_comparison').length, 1);
  let writes = 0;
  const broken = createFleetReviews({ laneFor: () => ({ goal: 'lane', base: pin().source_version }), changed() {}, invoke: async command => { if (command === 'save_fleet_pins') writes++; throw new Error('Invalid stored data'); } });
  await broken.loadSaved(); assert.equal(broken.snapshot().reviewPersistence.editable, false);
  broken.handle({ type: 'close-review', pin: '1' }); await settle(); assert.equal(writes, 0); broken.dispose();
});
test('reloading a saved set ignores old content replies even when the display key is reused', async () => {
  const first = pin(), second = { ...pin(), checkpoint: 'replacement', version: '9'.repeat(64) };
  let stored = snapshot([first], '1'); const pending = [], calls = [];
  const h = createFleetReviews({ laneFor: () => null, changed() {}, invoke: async (command, args) => {
    if (command === 'load_fleet_review_outbox') return { schema: 'mesh.fleet-review-outbox/v1', revision: '0', entries: [] };
    calls.push(command);
    if (command === 'load_fleet_pins') return stored;
    return new Promise(resolve => pending.push({ command, args, resolve }));
  } });
  await h.loadSaved(); stored = snapshot([second], '2'); h.handle({ type: 'reload-saved-reviews' }); await settle();
  for (const request of pending.slice(0, 2)) request.resolve(request.command === 'inspect_fleet_saved_review' ? reviewReply(first) : inputReply(first, request.args.after, request.args.selected));
  await settle();
  assert.equal(pending.length, 4); assert.equal(h.snapshot().reviewPins[0].selection.version, second.version);
  assert.equal(h.snapshot().reviewPins[0].review, null);
  for (const request of pending.slice(2, 4)) request.resolve(request.command === 'inspect_fleet_saved_review' ? reviewReply(second) : inputReply(second, request.args.after, request.args.selected));
  await settle(); const detail = pending[4]; detail.resolve(inputReply(second, detail.args.after, detail.args.selected)); await settle();
  assert.equal(h.snapshot().reviewPins[0].review.subject_operation, second.version);
  assert.equal(h.snapshot().reviewPins[0].input.file.object, second.input_object);
  assert.equal(calls.includes('save_fleet_pins'), false); h.dispose();
});
test('layout edits during content reads persist selectors only and survive late content replies', async () => {
  const p = pin(), writes = []; let finish;
  const h = createFleetReviews({ laneFor: () => null, changed() {}, invoke: async (command, args) => {
    if (command === 'load_fleet_review_outbox') return { schema: 'mesh.fleet-review-outbox/v1', revision: '0', entries: [] };
    if (command === 'load_fleet_pins') return snapshot([p], '1');
    if (command === 'save_fleet_pins') { const value = JSON.parse(args.snapshot); writes.push(value); return { ...value, revision: '2' }; }
    if (command === 'inspect_fleet_saved_review') return new Promise(resolve => { finish = () => resolve(reviewReply(p)); });
    return inputReply(p, args.after, args.selected);
  } });
  await h.loadSaved(); h.handle({ type: 'input-layout', pin: '1', layout: 'inline' }); await settle(); finish(); await settle();
  assert.deepEqual(writes[0].pins, [{ ...p, input_layout: 'inline' }]);
  assert.equal(h.snapshot().reviewPins[0].view.input_layout, 'inline');
  assert.equal(h.snapshot().reviewPins[0].input.file.after.text, 'new\n');
  assert.equal(h.snapshot().reviewPins[0].review.recorded, true); h.dispose();
});
test('failed save retains local choices and repeated retry intents share one read', async () => {
  const p = pin(); let reads = 0, complete;
  const h = createFleetReviews({ laneFor: () => null, changed() {}, invoke: async command => {
    if (command === 'load_fleet_pins') { if (++reads === 1) return snapshot([p], '1'); return new Promise(resolve => { complete = resolve; }); }
    throw new Error('Unavailable');
  } });
  await h.loadSaved(); await settle(); h.handle({ type: 'close-review', pin: '1' }); await settle();
  assert.equal(h.snapshot().reviewPersistence.phase, 'error'); assert.deepEqual(h.snapshot().reviewPins, []);
  h.handle({ type: 'retry-saved-reviews' }); h.handle({ type: 'retry-saved-reviews' }); await settle();
  assert.equal(reads, 2); assert.equal(h.snapshot().reviewPersistence.busy, true);
  complete(snapshot([], '2')); await settle(); assert.equal(h.snapshot().reviewPersistence.phase, 'saved');
  assert.equal(h.snapshot().reviewPersistence.busy, false); h.dispose();
});
