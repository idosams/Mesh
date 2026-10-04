import test from 'node:test';
import assert from 'node:assert/strict';
import { createFleetProgressSummaries } from './fleet-progress-summaries.js';
const hash = n => n.toString(16).padStart(64, '0');
const objective = `fleet-${hash(1)}`, laneId = n => `lane-${hash(n)}`, key = n => `${objective}/${laneId(n)}`;
const fleets = (ids = [1], version = hash(900), ownership = 'current-host') => [{ objective, ownership,
  lanes: ids.map(n => ({ id: laneId(n), base: hash(99), savedVersion: version, allocated: true, run: null })) }];
const reply = (args, source = hash(99)) => ({ schema: 'mesh.fleet-saved-progress-summary/v1', ...args,
  source_version: source, starting_version: hash(88), latest_acknowledged_version: args.version, revision: 1,
  handoff_authority: false, approval_authority: false,
  progress: { base: hash(88), target: args.version, total: 3, file_total: 2, folder_total: 1, approval_authority: false } });
const settle = () => new Promise(resolve => setImmediate(resolve));
function harness() {
  const calls = []; let now = 0, published = 0;
  const controller = createFleetProgressSummaries({ now: () => now, publish: () => { published++; }, invoke: (command, args) => {
    assert.equal(command, 'summarize_fleet_saved_progress');
    return new Promise((resolve, reject) => calls.push({ args, resolve, reject }));
  } });
  return { controller, calls, setTime: value => { now = value; }, published: () => published,
    complete: (n, source) => calls[n].resolve(reply(calls[n].args, source)) };
}

test('two independent reads, fair queue and coalescing never block another lane or refetch a cached version', async () => {
  const h = harness(), rows = fleets([1, 2, 3]); h.controller.sync(rows, true); await settle();
  assert.equal(h.calls.length, 2);
  rows[0].lanes[0].savedVersion = hash(901); h.controller.sync(rows, true);
  rows[0].lanes[0].savedVersion = hash(902); h.controller.sync(rows, true);
  h.complete(0); await settle();
  assert.equal(h.calls.length, 3); assert.equal(h.calls[2].args.lane, laneId(3), 'waiting lane precedes a hot lane retry');
  assert.equal(h.controller.snapshot()[key(1)].value, null, 'superseded reply is ignored');
  h.complete(1); await settle();
  assert.equal(h.controller.snapshot()[key(2)].value.version, hash(900));
  assert.equal(h.calls.length, 4); assert.equal(h.calls[3].args.version, hash(902));
  h.complete(2); h.complete(3); await settle();
  for (let n = 0; n < 20; n++) h.controller.sync(rows, true);
  await settle(); assert.equal(h.calls.length, 4); assert.equal(h.controller.snapshot()[key(1)].value.version, hash(902));
  h.controller.dispose();
});

test('older verified counts stay labeled while stale in-flight replies cannot replace them', async () => {
  const h = harness(); h.controller.sync(fleets(), true); await settle(); h.complete(0); await settle();
  h.controller.sync(fleets([1], hash(901)), true); await settle();
  h.controller.sync(fleets([1], hash(902)), true); h.complete(1); await settle();
  const pending = h.controller.snapshot()[key(1)];
  assert.equal(pending.version, hash(902)); assert.equal(pending.value.version, hash(900)); assert.equal(pending.busy, true);
  h.complete(2); await settle(); assert.equal(h.controller.snapshot()[key(1)].value.version, hash(902));
  h.controller.dispose();
});

test('errors retain counts, throttle retries, pause hidden dispatch and ignore disposed replies', async () => {
  const h = harness(); h.controller.sync(fleets(), true); await settle(); h.complete(0); await settle();
  h.controller.sync(fleets([1], hash(901)), true); await settle(); h.setTime(6000); h.calls[1].reject(new Error('private path')); await settle();
  const failed = h.controller.snapshot()[key(1)]; assert.equal(failed.value.version, hash(900)); assert.match(failed.error, /Earlier verified/); assert.doesNotMatch(failed.error, /private path/);
  h.setTime(10999); h.controller.sync(fleets([1], hash(901)), true); await settle(); assert.equal(h.calls.length, 2);
  h.setTime(11000); h.controller.sync(fleets([1], hash(901)), true); await settle(); assert.equal(h.calls.length, 3);
  h.controller.sync(fleets([1], hash(902)), false); h.complete(2); await settle(); assert.equal(h.calls.length, 3);
  h.controller.sync(fleets([1], hash(902)), true); await settle(); assert.equal(h.calls.length, 4);
  h.controller.dispose(); const published = h.published(); h.complete(3); await settle();
  assert.equal(h.published(), published); assert.deepEqual(h.controller.snapshot(), {});
});

test('hiding before dispatch, changed custody, remote and unavailable lanes cannot reuse a summary', async () => {
  const h = harness(); h.controller.sync(fleets(), true); h.controller.sync(fleets(), false); await settle(); assert.equal(h.calls.length, 0);
  h.controller.sync(fleets(), true); await settle(); assert.equal(h.calls.length, 1);
  const changed = fleets(); changed[0].lanes[0].base = hash(98); h.controller.sync(changed, true); await settle();
  h.complete(0); await settle(); assert.equal(h.controller.snapshot()[key(1)].value, null);
  h.complete(1, hash(98)); await settle(); assert.equal(h.controller.snapshot()[key(1)].value.source, hash(98));
  for (const modify of [f => f[0].ownership = 'unavailable', f => f[0].lanes[0].allocated = false,
    f => f[0].lanes[0].savedVersion = null, f => f[0].lanes[0].run = { remote: { worker: hash(9) } }]) {
    const f = fleets(); modify(f); h.controller.sync(f, true); await settle(); assert.deepEqual(h.controller.snapshot(), {});
  }
  h.controller.sync(fleets([1], hash(903), 'restored-unattached'), true); await settle(); assert.equal(h.calls.length, 3);
  h.complete(2); await settle(); assert.equal(h.controller.snapshot()[key(1)].value.version, hash(903)); h.controller.dispose();
});

test('scheduler storage is bounded to the complete native catalogue and dispatch stays at two reads', async () => {
  const h = harness(); h.controller.sync(fleets(Array.from({ length: 16385 }, (_, n) => n + 1)), true); await settle();
  assert.equal(Object.keys(h.controller.snapshot()).length, 16384); assert.equal(h.calls.length, 2); h.controller.dispose();
});
