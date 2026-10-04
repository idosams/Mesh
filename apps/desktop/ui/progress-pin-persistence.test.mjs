import test from 'node:test';
import assert from 'node:assert/strict';
import { progressPinSnapshot } from './progress-pin-persistence.js';
import { createFleetProgress } from './fleet-progress-panels.js';
const h = n => n.toString(16).padStart(64, '0');
const selector = (key = '1') => ({ key, objective: `fleet-${h(1)}`, lane: `lane-${h(2)}`, version: h(3), source: h(4), starting: h(5), after: null, object: null, layout: 'split' });
const snapshot = pins => ({ schema: 'mesh.desktop-progress-pin-selectors/v1', revision: '1', pins });
const settle = () => new Promise(resolve => setImmediate(resolve));
function setup(initial = [selector()]) {
  let stored = snapshot(initial), historyMissing = false, loseAck = false, failLoad = false;
  const calls = [];
  const invoke = async (command, args) => {
    calls.push({ command, args });
    if (command === 'load_progress_pins') { if (failLoad) throw Error('private'); return structuredClone(stored); }
    if (command === 'save_progress_pins') {
      const next = progressPinSnapshot(args.snapshot); assert.equal(next.revision, stored.revision);
      stored = { ...next, revision: String(BigInt(stored.revision) + 1n) };
      if (loseAck) { loseAck = false; throw Error('lost'); } return structuredClone(stored);
    }
    assert.equal(command, 'inspect_fleet_saved_progress');
    if (historyMissing) throw Error('unavailable');
    return { schema: 'mesh.fleet-saved-progress-comparison/v1', objective: args.objective, lane: args.lane, revision: 90,
      source_version: h(4), starting_version: h(5), latest_acknowledged_version: h(99), handoff_authority: false, approval_authority: false,
      progress: { base: h(5), target: args.version, after: args.after, selected: args.selected, order: 'object-id', approval_authority: false,
        total: 0, changes: [], next_after: null } };
  };
  return { calls, create: () => createFleetProgress({ invoke, changed() {}, laneFor: () => null }), stored: () => structuredClone(stored),
    missing: () => historyMissing = true, lost: () => loseAck = true, fail: () => failLoad = true,
    replace: pins => stored = snapshot(pins) };
}
test('restart restores exact versions and layouts without live fleet ownership or saving content', async () => {
  const f = setup([selector(), { ...selector('2'), version: h(6) }]), c = f.create();
  await c.loadSaved();
  assert.deepEqual(c.snapshot().progressPins.map(p => [p.selection.version, p.input.page.target, p.layout]), [[h(3), h(3), 'split'], [h(6), h(6), 'split']]);
  assert.equal(c.snapshot().progressPersistence.phase, 'saved');
  assert.equal(f.calls.filter(v => v.command === 'save_progress_pins').length, 0);
  c.handle({ type: 'progress-layout', pin: '1', layout: 'inline' }); await settle();
  assert.equal(f.stored().pins[0].layout, 'inline');
  assert.deepEqual(Object.keys(f.stored().pins[0]).sort(), ['after','key','lane','layout','object','objective','source','starting','version']);
  c.dispose(); const reopened = f.create(); await reopened.loadSaved(); assert.equal(reopened.snapshot().progressPins[0].layout, 'inline');
});
test('missing retained content preserves stored selectors and reports unavailable independently', async () => {
  const f = setup(); f.missing(); const c = f.create(); await c.loadSaved();
  assert.ok(c.snapshot().progressPins[0].input.error); assert.equal(c.snapshot().progressPins[0].input.page, null);
  assert.equal(f.calls.filter(v => v.command === 'save_progress_pins').length, 0);
  assert.deepEqual(f.stored().pins, [selector()]);
});
test('lost save acknowledgment is reconciled by reading the same selectors without duplicate writes', async () => {
  const f = setup(), c = f.create(); await c.loadSaved(); f.lost();
  c.handle({ type: 'progress-layout', pin: '1', layout: 'inline' }); await settle();
  assert.equal(c.snapshot().progressPersistence.phase, 'error');
  c.handle({ type: 'progress-retry-save' }); await settle();
  assert.equal(c.snapshot().progressPersistence.phase, 'saved');
  assert.equal(f.calls.filter(v => v.command === 'save_progress_pins').length, 1);
});
test('unreadable storage freezes edits and late restoration after disposal stays closed', async () => {
  const f = setup(); f.fail(); const c = f.create(); await c.loadSaved();
  assert.equal(c.snapshot().progressPersistence.editable, false); c.handle({ type: 'progress-close', pin: '1' });
  assert.equal(f.calls.length, 1);
  let resolve; const late = createFleetProgress({ invoke: () => new Promise(r => resolve = r), changed() {}, laneFor: () => null });
  const loading = late.loadSaved(); late.dispose(); resolve(snapshot([selector()])); await loading;
  assert.deepEqual(late.snapshot().progressPins, []);
});
test('selector schema rejects duplicate identities, authority fields and noncanonical numbers', () => {
  for (const mutate of [v => v.revision = '01', v => v.pins[0].content = 'private', v => v.pins[0].after = '../x',
    v => v.pins.push(selector('2')), v => v.pins[0].key = '0', v => v.pins[0].layout = 'approve']) {
    const value = snapshot([selector()]); mutate(value); assert.throws(() => progressPinSnapshot(value));
  }
});
test('restoration preserves the page cursor and selected object while verifying both reads', async () => {
  const object = '2'.repeat(32), after = '1'.repeat(32), row = { ...selector(), after, object };
  const calls = [];
  const c = createFleetProgress({ changed() {}, laneFor: () => null, invoke: async (command, args) => {
    calls.push({ command, args });
    if (command === 'load_progress_pins') return snapshot([row]);
    assert.equal(command, 'inspect_fleet_saved_progress');
    const selected = args.selected !== null;
    return { schema: 'mesh.fleet-saved-progress-comparison/v1', objective: row.objective, lane: row.lane, revision: 8,
      source_version: row.source, starting_version: row.starting, latest_acknowledged_version: h(99), handoff_authority: false, approval_authority: false,
      progress: { base: row.starting, target: row.version, after: args.after, selected: args.selected, order: 'object-id', approval_authority: false,
        total: 201, next_after: null, changes: [{ object, effect: 'added', before: null, after: { path: 'note.txt', kind: 'file', digest: h(9), bytes: 4,
          executable: false, content_state: selected ? 'text' : 'not-requested', text: selected ? 'old\n' : null } }] } };
  } });
  await c.loadSaved(); const pin = c.snapshot().progressPins[0];
  assert.equal(pin.input.page.after, after); assert.equal(pin.input.file.object, object); assert.equal(pin.input.file.after.text, 'old\n');
  assert.deepEqual(pin.view, { after, object });
  assert.deepEqual(calls.slice(1).map(v => [v.args.after, v.args.selected]), [[after, null], [null, object]]);
});
