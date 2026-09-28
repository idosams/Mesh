import test from 'node:test';
import assert from 'node:assert/strict';
import { fleetCatalogue, fleetActivity, fleetProvisioned, startFleets } from './fleets.js';
const id = 'a'.repeat(64), version = 'b'.repeat(64), objective = `fleet-${'c'.repeat(64)}`, lane = `lane-${'d'.repeat(64)}`;
const catalogue = () => ({ schema: 'mesh.native-fleets/v1', fleets: [{ objective, ownership: 'current-host', state: {
  objective, revision: 3, cancelled: false, lanes: [{ id: lane, parent: null, source_project: id, goal: 'Coordinate', provider: 'codex', base: version, allocated: true, workspace: { root: '/native/lane', installation: 'native-installation' }, run: null }],
} }] });
const activity = () => ({ schema: 'mesh.desktop-fleet-activity/v1', fleets: [] });
const live = () => ({ objective, status: 'monitoring', stop_requested: false, observed_at: '1000', workers: [{ lane, run: 'run-one', observed_at: '1000', thread: null, activity: 'working', events: '2', stderr_lines: '0', turn_completed: false, failed: false, streams_closed: false, outcome: null }] });
class CustomEvent extends Event { constructor(type, init = {}) { super(type); this.detail = init.detail; } }
const settle = () => new Promise(resolve => setImmediate(resolve));
function harness(invoke) {
  const document = new EventTarget(), projections = [], timers = new Map(); let sequence = 0;
  document.addEventListener('mesh:fleets-projection', event => projections.push(event.detail));
  const dispose = startFleets({ document, invoke, CustomEvent, requestId: () => 'e'.repeat(32), schedule: callback => { timers.set(++sequence, callback); return sequence; }, cancel: key => timers.delete(key) });
  const emit = (name, detail) => document.dispatchEvent(new CustomEvent(name, { detail }));
  emit('mesh:attachments-projection', { projects: [{ id, savedVersion: version }], histories: {}, error: '' });
  emit('mesh:fleets-visible', true);
  return { dispose, projections, timers, emit, intent: detail => emit('mesh:fleets-intent', detail) };
}
const provision = { type: 'provision', id, version, goal: 'Do useful work', lanes: '4', concurrency: '2', depth: '1' };

test('catalogue validates identities, ownership, bounds and parent/run correlation', () => {
  assert.equal(fleetCatalogue(catalogue())[0].lanes[0].sourceProject, id);
  for (const mutate of [
    value => value.fleets.push(value.fleets[0]),
    value => { value.fleets[0].ownership = 'running'; },
    value => { value.fleets[0].state.objective = 'different'; },
    value => { value.fleets[0].state.lanes[0].parent = lane; },
    value => { value.fleets[0].state.lanes[0].parent = 'missing'; },
    value => { value.fleets[0].state.lanes[0].run = { id: 'r', state: 'approved' }; },
    value => { value.fleets[0].state.revision = Number.MAX_SAFE_INTEGER + 1; },
    value => { value.fleets[0].ownership = 'unavailable'; },
  ]) { const value = catalogue(); mutate(value); assert.throws(() => fleetCatalogue(value)); }
  const unavailable = catalogue(); unavailable.fleets[0].ownership = 'unavailable'; unavailable.fleets[0].state = null;
  assert.equal(fleetCatalogue(unavailable)[0].lanes.length, 0);
});
test('activity keeps exact attempt identity and refuses ambiguous observations', () => {
  const value = activity(); value.fleets.push(live());
  assert.equal(fleetActivity(value)[0].workers[0].run, 'run-one');
  for (const mutate of [
    value => { value.fleets[0].observed_at = 1000; },
    value => { value.fleets[0].workers[0].outcome = 'approved'; },
    value => { value.fleets[0].workers.push(value.fleets[0].workers[0]); },
    value => { value.fleets[0].workers[0].events = '-1'; },
    value => { value.fleets[0].stop_requested = null; },
  ]) { const copy = structuredClone(value); mutate(copy); assert.throws(() => fleetActivity(copy)); }
});
test('provisioning requires the exact receipt and never implies agent start', () => {
  const pending = { ...provision, request: 'e'.repeat(32) };
  const value = { schema: 'mesh.desktop-attached-fleet/v1', project: id, request: pending.request, objective, started: false };
  assert.equal(fleetProvisioned(value, pending), objective);
  for (const mutation of [{ request: 'f'.repeat(32) }, { project: version }, { started: true }]) assert.throws(() => fleetProvisioned({ ...value, ...mutation }, pending));
});
test('uncertain provisioning retains exact input and request; polling never retries or starts agents', async () => {
  const calls = []; let fail = true;
  const h = harness(async (command, args) => {
    calls.push({ command, args });
    if (command === 'attached_fleets') return catalogue();
    if (command === 'fleet_activity') return activity();
    if (command === 'provision_attached_fleet') {
      if (fail) throw new Error('private native detail');
      return { schema: 'mesh.desktop-attached-fleet/v1', project: id, request: args.request, objective, started: false };
    }
    throw new Error(command);
  });
  await settle(); h.intent(provision); await settle();
  const first = calls.find(call => call.command === 'provision_attached_fleet');
  assert.deepEqual(JSON.parse(first.args.limitsJson), { lanes: 4, concurrency: 2, depth: 1, retries: 0 });
  assert.equal(h.projections.at(-1).pending.version, version);
  h.emit('mesh:attachments-projection', { projects: [{ id, savedVersion: 'f'.repeat(64) }], histories: {}, error: '' });
  h.intent({ ...provision, goal: 'Replace uncertain intent' }); h.intent({ type: 'refresh' }); await settle();
  assert.equal(calls.filter(call => call.command === 'provision_attached_fleet').length, 1);
  fail = false; h.intent({ type: 'retry-provision' }); await settle();
  assert.deepEqual(calls.filter(call => call.command === 'provision_attached_fleet')[1], first);
  assert.equal(h.projections.at(-1).pending, null);
  assert.equal(calls.some(call => call.command === 'start_attached_fleet'), false);
  assert.doesNotMatch(h.projections.at(-1).feedback, /private native detail/);
  h.dispose();
});
test('closed intents cannot supply paths, invalid budgets or unknown project identities', async () => {
  const calls = []; const h = harness(async (command) => { calls.push(command); return command === 'attached_fleets' ? catalogue() : activity(); });
  await settle(); const before = calls.length;
  for (const mutation of [{ path: '/elsewhere' }, { id: version }, { lanes: '0' }, { concurrency: '5' }, { depth: '33' }, { goal: ' ' }, { goal: 'é'.repeat(8192) }, { version: '../' }, { lanes: '04' }]) h.intent({ ...provision, ...mutation });
  assert.equal(calls.length, before); h.dispose();
});
test('start is explicit, attempted fleets never restart, and stale snapshots cannot enable new launches', async () => {
  let value = catalogue(), observations = activity(), fail = false; const calls = [];
  const h = harness(async (command, args) => {
    calls.push({ command, args });
    if (command === 'start_attached_fleet') {
      value.fleets[0].state.lanes[0].run = { id: 'run-one', state: 'running' };
      observations.fleets = [live()]; return observations;
    }
    if (command === 'stop_attached_fleet') { value.fleets[0].state.cancelled = true; return observations; }
    if (fail) throw new Error('credential-secret');
    return command === 'attached_fleets' ? value : observations;
  });
  await settle(); h.intent({ type: 'start', objective, path: '/wrong' }); assert.equal(calls.length, 2);
  h.intent({ type: 'start', objective }); await settle(); h.intent({ type: 'start', objective }); await settle();
  assert.equal(calls.filter(call => call.command === 'start_attached_fleet').length, 1);
  fail = true; h.intent({ type: 'refresh' }); await settle();
  assert.match(h.projections.at(-1).error, /could not be confirmed/);
  assert.equal(h.projections.at(-1).activity[0].observedAt, '1000');
  h.intent({ type: 'stop', objective }); await settle();
  assert.equal(calls.filter(call => call.command === 'stop_attached_fleet').length, 1);
  assert.doesNotMatch(h.projections.at(-1).error, /credential-secret/); h.dispose();
});
test('restored or unavailable fleets cannot request execution and hidden/disposed views stop polling', async () => {
  const value = catalogue(); value.fleets[0].ownership = 'restored-unattached'; const calls = [];
  const h = harness(async command => { calls.push(command); return command === 'attached_fleets' ? value : activity(); });
  await settle(); h.intent({ type: 'start', objective }); h.intent({ type: 'stop', objective }); assert.equal(calls.length, 2);
  assert.equal(h.timers.size, 1); h.emit('mesh:fleets-visible', false); assert.equal(h.timers.size, 0);
  h.intent(provision); assert.equal(calls.length, 2); h.dispose(); h.emit('mesh:fleets-visible', true); assert.equal(calls.length, 2);
});
test('partial refresh failure retains a coherent prior projection and disposal ignores a late reply', async () => {
  let release; let hold = false;
  const h = harness(async command => {
    if (hold && command === 'attached_fleets') return new Promise(resolve => { release = resolve; });
    return command === 'attached_fleets' ? catalogue() : activity();
  });
  await settle(); hold = true; h.intent({ type: 'refresh' }); const count = h.projections.length; h.dispose();
  release(catalogue()); await settle(); assert.equal(h.projections.length, count); assert.equal(h.timers.size, 0);
});

test('failed start remains visible after a later successful poll and is never replayed automatically', async () => {
  const calls = []; const h = harness(async command => {
    calls.push(command);
    if (command === 'start_attached_fleet') throw new Error('private native error');
    return command === 'attached_fleets' ? catalogue() : activity();
  });
  await settle(); h.intent({ type: 'start', objective }); await settle();
  assert.match(h.projections.at(-1).feedback, /could not yet be confirmed/);
  h.intent({ type: 'refresh' }); await settle();
  assert.equal(h.projections.at(-1).error, '');
  assert.match(h.projections.at(-1).feedback, /installed Codex/);
  assert.equal(calls.filter(command => command === 'start_attached_fleet').length, 1);
  assert.doesNotMatch(h.projections.at(-1).feedback, /private native error/); h.dispose();
});

test('an explicitly retained saved input does not drift when source capture advances', async () => {
  const calls = []; const h = harness(async (command, args) => {
    calls.push({ command, args });
    if (command === 'provision_attached_fleet') return { schema: 'mesh.desktop-attached-fleet/v1', project: id, request: args.request, objective, started: false };
    return command === 'attached_fleets' ? catalogue() : activity();
  });
  await settle(); h.emit('mesh:attachments-projection', { projects: [{ id, savedVersion: 'f'.repeat(64) }], histories: {}, error: '' });
  h.intent(provision); await settle();
  assert.equal(calls.find(call => call.command === 'provision_attached_fleet').args.version, version);
  h.dispose();
});

test('saved-result reads and closing panels remain independent of an active fleet refresh', async () => {
  let holdRefresh = false, pendingCatalogue, pendingReview;
  const commands = [], bundle = 'f'.repeat(64);
  const h = harness(async (command, args) => {
    commands.push(command);
    if (command === 'attached_fleets') return holdRefresh ? new Promise(resolve => { pendingCatalogue = resolve; }) : catalogue();
    if (command === 'fleet_activity') return activity();
    if (command === 'fleet_saved_reviews') return { schema: 'mesh.fleet-saved-reviews/v1', objective, lane, revision: 1, after: null, total: 1, next_after: null, order: 'checkpoint-id', reviews: [{ checkpoint: 'checkpoint-one', version, bundle, run: 'run-one' }] };
    if (command === 'inspect_fleet_saved_review') return new Promise(resolve => { pendingReview = resolve; });
    throw new Error(`Unexpected command ${command}`);
  });
  await settle(); holdRefresh = true; h.intent({ type: 'refresh' });
  assert.equal(h.projections.at(-1).busy, true);
  h.intent({ type: 'reviews', objective, lane }); await settle();
  h.intent({ type: 'pin-review', objective, lane, checkpoint: 'checkpoint-one', version, bundle });
  assert.equal(h.projections.at(-1).reviewPins.length, 1);
  h.intent({ type: 'close-review', pin: '1' }); assert.equal(h.projections.at(-1).reviewPins.length, 0);
  pendingReview({}); pendingCatalogue(catalogue()); await settle();
  assert.equal(h.projections.at(-1).reviewPins.length, 0);
  assert.equal(commands.filter(command => command === 'inspect_fleet_saved_review').length, 1);
  h.dispose();
});
