import test from 'node:test';
import assert from 'node:assert/strict';
import { createFleetProgress } from './fleet-progress-panels.js';
const h = n => n.toString(16).padStart(64, '0'), objective = `fleet-${h(1)}`, lane = `lane-${h(2)}`;
const settle = () => new Promise(resolve => setImmediate(resolve));
function fixture() {
  let latest = 5, defer = false, calls = [], pending = [];
  const envelope = schema => ({ schema, objective, lane, revision: latest, source_version: h(3), starting_version: h(4), latest_acknowledged_version: h(latest), handoff_authority: false, approval_authority: false });
  const compare = args => ({ ...envelope('mesh.fleet-saved-progress-comparison/v1'), progress: { base: h(4), target: args.version, order: 'object-id', after: args.after, selected: args.selected, total: 0, changes: [], next_after: null, approval_authority: false } });
  const controller = createFleetProgress({ changed() {}, laneFor: (o, l) => o === objective && l === lane,
    invoke: async (command, args) => {
      calls.push({ command, args });
      if (command === 'fleet_saved_progress') return { ...envelope('mesh.fleet-saved-progress-page/v1'), progress: { order: 'causal-operation', after: null, total: latest - 3, versions: Array.from({ length: latest - 3 }, (_, n) => ({ version: h(n + 4), ordinal: n + 1 })), next_after: null } };
      assert.equal(command, 'inspect_fleet_saved_progress');
      if (defer) return new Promise((resolve, reject) => pending.push({ args, resolve: () => resolve(compare(args)), reject }));
      return compare(args);
    } });
  return { controller, calls, pending, latest: n => latest = n, defer: () => defer = true,
    list: () => controller.handle({ type: 'progress-list', objective, lane }),
    pin: n => controller.handle({ type: 'progress-pin', objective, lane, version: h(n) }) };
}
test('two pins remain exact while a third save arrives; only read commands execute', async () => {
  const f = fixture(); f.list(); await settle(); f.pin(4); f.pin(5); await settle();
  const pins = f.controller.snapshot().progressPins, before = structuredClone(pins);
  f.latest(6); f.list(); await settle();
  assert.deepEqual(pins, before);
  assert.equal(f.controller.snapshot().progressQueues[`${objective}/${lane}`].page.latest, h(6));
  f.pin(6); await settle();
  assert.deepEqual(f.controller.snapshot().progressPins.map(pin => pin.input.page.target), [h(4), h(5), h(6)]);
  assert.ok(f.calls.every(call => ['fleet_saved_progress', 'inspect_fleet_saved_progress'].includes(call.command)));
});
test('late reads cannot replace newer requests or reopen a closed progress panel', async () => {
  const f = fixture(); f.list(); await settle(); f.defer(); f.pin(5);
  const pin = f.controller.snapshot().progressPins[0];
  f.controller.handle({ type: 'progress-first', pin: pin.key });
  f.pending[1].resolve(); await settle();
  const verified = structuredClone(pin.input);
  f.pending[0].reject(new Error('stale')); await settle();
  assert.deepEqual(pin.input, verified);
  f.controller.handle({ type: 'progress-first', pin: pin.key });
  f.controller.handle({ type: 'progress-close', pin: pin.key });
  f.pending[2].resolve(); await settle();
  assert.deepEqual(f.controller.snapshot().progressPins, []);
});
test('failed reads retain verified content, exact retry selection and bounded panels', async () => {
  const f = fixture(); f.latest(9); f.list(); await settle();
  for (let n = 4; n <= 8; n++) f.pin(n);
  await settle(); assert.equal(f.controller.snapshot().progressPins.length, 4);
  f.pin(4); assert.equal(f.controller.snapshot().progressPins.length, 4);
  const pin = f.controller.snapshot().progressPins[0], original = structuredClone(pin.input.page);
  f.defer(); f.controller.handle({ type: 'progress-first', pin: pin.key });
  f.pending[0].reject(new Error('history unavailable')); await settle();
  assert.deepEqual(pin.input.page, original); assert.ok(pin.input.error);
  f.controller.handle({ type: 'progress-retry', pin: pin.key });
  assert.equal(f.pending[1].args.version, h(4)); f.pending[1].resolve(); await settle(); assert.equal(pin.input.error, '');
  const count = f.calls.length;
  f.controller.handle({ type: 'progress-file', pin: pin.key, object: '../unverified' });
  f.controller.handle({ type: 'progress-first', pin: pin.key, path: '/tmp' });
  assert.equal(f.calls.length, count);
  f.controller.dispose(); f.list(); f.pin(9); assert.equal(f.calls.length, count);
});

test('pin latest fixes the clicked version beyond the first page while newer work arrives', async () => {
  let observed = h(70), resolvePage; const calls = [];
  const controller = createFleetProgress({ changed() {}, laneFor: () => ({ savedVersion: observed }), invoke: async (command, args) => {
    calls.push({ command, args });
    const common = { objective, lane, revision: 80, source_version: h(3), starting_version: h(4), latest_acknowledged_version: h(80), handoff_authority: false, approval_authority: false };
    if (command === 'fleet_saved_progress') return new Promise(resolve => { resolvePage = () => resolve({ ...common, schema: 'mesh.fleet-saved-progress-page/v1', progress: {
      after: null, order: 'causal-operation', total: 80, versions: Array.from({ length: 50 }, (_, n) => ({ version: h(n + 4), ordinal: n + 1 })), next_after: h(53) } }); });
    assert.equal(command, 'inspect_fleet_saved_progress');
    return { ...common, schema: 'mesh.fleet-saved-progress-comparison/v1', progress: { base: h(4), target: args.version,
      after: null, selected: null, order: 'object-id', approval_authority: false, total: 0, changes: [], next_after: null } };
  } });
  controller.handle({ type: 'progress-pin-latest', objective, lane, version: h(69) }); assert.equal(calls.length, 0);
  controller.handle({ type: 'progress-pin-latest', objective, lane, version: h(70) });
  controller.handle({ type: 'progress-pin-latest', objective, lane, version: h(70) }); assert.equal(calls.length, 1);
  observed = h(80); resolvePage(); await settle();
  assert.equal(controller.snapshot().progressPins[0].selection.version, h(70));
  assert.equal(controller.snapshot().progressPins[0].input.page.target, h(70));
  assert.deepEqual(controller.snapshot().progressLatestBusy, {});
});
test('late latest-save lookup cannot open a panel after disposal', async () => {
  let resolve;
  const page = { schema: 'mesh.fleet-saved-progress-page/v1', objective, lane, revision: 2,
    source_version: h(3), starting_version: h(4), latest_acknowledged_version: h(5), handoff_authority: false, approval_authority: false,
    progress: { after: null, order: 'causal-operation', total: 2, versions: [{ version: h(4), ordinal: 1 }, { version: h(5), ordinal: 2 }], next_after: null } };
  const c = createFleetProgress({ changed() {}, laneFor: () => ({ savedVersion: h(5) }), invoke: () => new Promise(r => resolve = r) });
  c.handle({ type: 'progress-pin-latest', objective, lane, version: h(5) }); c.dispose(); resolve(page); await settle();
  assert.deepEqual(c.snapshot().progressPins, []);
});
