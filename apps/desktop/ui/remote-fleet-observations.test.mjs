import test from 'node:test';
import assert from 'node:assert/strict';
import { createRemoteFleetObservations, remoteFleetReply } from './remote-fleet-observations.js';
const worker = 'a'.repeat(64);
const fleet = count => [{ objective: 'fleet-one', lanes: Array.from({ length: count }, (_, n) => ({ id: `lane-${n}`, run: { id: `run-${n}`, remote: { assignment: `assignment-${n}`, worker } } })) }];
const scope = n => ({ objective: 'fleet-one', lane: `lane-${n}`, run: `run-${n}`, assignment: `assignment-${n}`, worker });
const reply = (target, revision = '7', state = 'waiting') => ({ schema: 'mesh.remote-fleet-observation/v1', ...target,
  observation: { schema: 'mesh.remote-panel-observation/v2', id: 'b'.repeat(64), kind: 'execution', observed_ms: '1000', admitted: true, launch_recorded: true, execution: { revision, state } } });
const settle = () => new Promise(resolve => setImmediate(resolve));
function harness() {
  const calls = []; let clock = 0;
  const controller = createRemoteFleetObservations({ now: () => clock, publish() {}, invoke(command, args) {
    return new Promise((resolve, reject) => calls.push({ command, args, resolve, reject }));
  } });
  return { controller, calls, advance: () => { clock += 6000; } };
}
test('four independent reads allow a completed lane to make room while a slow peer remains pending', async () => {
  const h = harness(); h.controller.sync(fleet(5), true); await settle();
  assert.equal(h.calls.length, 4); assert.ok(h.calls.every(call => call.command === 'read_remote_fleet_observation'));
  h.calls[1].resolve(reply(scope(1))); await settle();
  assert.equal(h.calls.length, 5); assert.equal(h.calls[4].args.lane, 'lane-4');
  assert.equal(h.controller.snapshot()['fleet-one/lane-0'].busy, true);
  assert.equal(h.controller.snapshot()['fleet-one/lane-1'].value.recorded.state, 'waiting');
  h.controller.dispose(); for (const call of h.calls) call.reject(new Error('fixture cleanup')); await settle();
});
test('a failed refresh retains verified history and stops scheduling when the view is hidden', async () => {
  const h = harness(); h.controller.sync(fleet(1), true); await settle(); h.calls[0].resolve(reply(scope(0))); await settle();
  h.advance(); h.controller.sync(fleet(1), true); await settle(); h.calls[1].reject(new Error('private path and credentials')); await settle();
  const row = h.controller.snapshot()['fleet-one/lane-0']; assert.equal(row.value.recorded.revision, '7');
  assert.match(row.error, /unavailable/); assert.doesNotMatch(row.error, /private|credentials/);
  h.advance(); h.controller.sync(fleet(1), false); await settle(); assert.equal(h.calls.length, 2);
  h.controller.sync(fleet(1), true); await settle(); assert.equal(h.calls.length, 3);
  h.controller.dispose(); h.calls[2].reject(new Error('cleanup')); await settle();
});
test('late replies cannot replace a new attempt or repopulate a disposed view', async () => {
  const h = harness(); h.controller.sync(fleet(1), true); await settle();
  const next = fleet(1); next[0].lanes[0].run.id = 'replacement'; next[0].lanes[0].run.remote.assignment = 'replacement-assignment';
  h.controller.sync(next, true); await settle(); assert.equal(h.calls.length, 2);
  h.calls[0].resolve(reply(scope(0))); await settle();
  assert.equal(h.controller.snapshot()['fleet-one/lane-0'].value, null);
  h.controller.dispose(); h.calls[1].resolve(reply({ ...scope(0), run: 'replacement', assignment: 'replacement-assignment' })); await settle();
  assert.deepEqual(h.controller.snapshot(), {});
});
test('exact assignment, monotonic observation and original-session revisions are required', () => {
  const target = scope(0), first = remoteFleetReply(reply(target), target, null);
  for (const mutation of [{ worker: 'c'.repeat(64) }, { run: 'another' }, { assignment: 'another' }, { lane: 'another' }]) {
    assert.throws(() => remoteFleetReply({ ...reply(target), ...mutation }, target, first));
  }
  assert.throws(() => remoteFleetReply(reply(target, '6'), target, first));
  assert.throws(() => remoteFleetReply(reply(target, '7', 'running'), target, first));
  const earlier = reply(target); earlier.observation.observed_ms = '999'; assert.throws(() => remoteFleetReply(earlier, target, first));
  const absent = reply(target); absent.observation.execution = null; absent.observation.launch_recorded = false;
  assert.throws(() => remoteFleetReply(absent, target, first));
  assert.equal(remoteFleetReply(reply(target, '8', 'running'), target, first).recorded.state, 'running');
});
