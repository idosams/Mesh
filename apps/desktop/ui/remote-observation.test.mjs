import test from 'node:test';
import assert from 'node:assert/strict';
import { startRemoteObservation, selectionReply, observationReply, draftReply, setupInput, profilesReply, profileOpenReply, inputRecoveryReply } from './remote-observation.js';
const id = 'a'.repeat(64);
const selected = () => ({ schema: 'mesh.remote-panel-selection/v1', id, host: 'worker.example', worker: 'b'.repeat(64), objective: 'fleet-one', lane: 'lane-one', run: 'run-one' });
const status = () => ({ schema: 'mesh.remote-panel-observation/v1', id, kind: 'status', observed_ms: '1000', admitted: true, launch_recorded: false, lease_until_ms: '2000' });
class CustomEvent extends Event { constructor(type, options = {}) { super(type); this.detail = options.detail; } }
const settle = () => new Promise(resolve => setImmediate(resolve));
function harness(invoke) {
  const document = new EventTarget(), projections = [];
  document.addEventListener('mesh:remote-observation-projection', event => projections.push(event.detail));
  const dispose = startRemoteObservation({ document, invoke, CustomEvent });
  return { projections, dispose, intent: detail => document.dispatchEvent(new CustomEvent('mesh:remote-observation-intent', { detail })) };
}
test('selection strips private fields and observation refuses cross-selection and cross-kind replies', () => {
  assert.equal(selectionReply({ ...selected(), identity: '/private/key' }).identity, undefined);
  assert.throws(() => selectionReply({ ...selected(), host: 'host\ncommand' }));
  assert.throws(() => observationReply(status(), 'c'.repeat(64), 'status'));
  assert.throws(() => observationReply(status(), id, 'results'));
  assert.throws(() => observationReply({ ...status(), admitted: false, launch_recorded: true }, id, 'status'));
});
test('only native selection and opaque read arguments reach the bridge', async () => {
  const calls = [], h = harness(async (name, args) => { calls.push([name, args]); return name === 'pick_remote_observation' ? selected() : status(); });
  h.intent({ type: 'choose', path: '/renderer/private.json' }); await settle();
  h.intent({ type: 'status', id: 'forged', path: '/renderer/key', after: 8000 }); await settle();
  assert.deepEqual(calls, [['pick_remote_observation', undefined], ['read_remote_observation', { id, action: 'status', after: 0 }]]);
  assert.equal(h.projections.at(-1).status.admitted, true); h.dispose();
});
test('inflight reads suppress duplicate actions and stale replies retain old verified observations', async () => {
  let fail = false, release;
  const h = harness(async name => { if (name === 'pick_remote_observation') return selected(); if (fail) return new Promise(resolve => { release = resolve; }); return status(); });
  h.intent({ type: 'choose' }); await settle(); h.intent({ type: 'status' }); await settle();
  fail = true; h.intent({ type: 'status' }); await settle(); h.intent({ type: 'forget' });
  assert.equal(h.projections.at(-1).busy, true);
  release({ ...status(), id: 'c'.repeat(64) }); await settle();
  assert.equal(h.projections.at(-1).selection.id, id);
  assert.equal(h.projections.at(-1).status.observed, '1000');
  assert.match(h.projections.at(-1).error, /out of date/); h.dispose();
});
test('chooser cancellation preserves selection and forgetting clears displayed observations', async () => {
  let cancel = false;
  const h = harness(async name => name === 'pick_remote_observation' ? cancel ? null : selected() : name === 'read_remote_observation' ? status() : undefined);
  h.intent({ type: 'choose' }); await settle(); h.intent({ type: 'status' }); await settle();
  cancel = true; h.intent({ type: 'choose' }); await settle(); assert.equal(h.projections.at(-1).status.admitted, true);
  h.intent({ type: 'forget' }); await settle(); assert.equal(h.projections.at(-1).selection, null); assert.equal(h.projections.at(-1).status, null); h.dispose();
});
test('missing results are not presented as an empty verified history', () => {
  const value = { schema: 'mesh.remote-panel-observation/v1', id, kind: 'results', available: false, revision: null, after: null, count: 0, has_more: false };
  assert.equal(observationReply(value, id, 'results').available, false);
  assert.throws(() => observationReply({ ...value, count: 1 }, id, 'results'));
  assert.equal(observationReply({ ...value, available: true, revision: '0', after: 0 }, id, 'results').available, true);
});
test('unrecognized actions never dispatch authority-bearing operations', async () => {
  let calls = 0; const h = harness(async () => { calls++; return selected(); });
  for (const type of ['start', 'receive', 'reconnect-input', 'status']) h.intent({ type });
  await settle(); assert.equal(calls, 0); h.dispose(); h.intent({ type: 'choose' }); await settle(); assert.equal(calls, 0);
});

const draft = (overrides = {}) => ({ schema: 'mesh.remote-setup-draft/v1', id: 'd'.repeat(64), installation: true, identity: true, hosts: true, ...overrides });
const form = () => ({ host: 'worker.example', account: 'mesh', port: '22', worker: 'b'.repeat(64), objective: 'fleet-one', lane: 'lane-one', run: 'run-one' });
test('setup accepts only public form fields and strips renderer-authored paths', () => {
  const input = setupInput({ ...form(), identity: '/forged', fleets: '/forged' });
  assert.equal(input.port, 22); assert.equal(input.identity, undefined); assert.equal(input.fleets, undefined);
  for (const port of ['0', '022', '65536', '1e3', '-1']) assert.throws(() => setupInput({ ...form(), port }));
  assert.throws(() => draftReply({ ...draft(), identity: '/forged' }));
});
test('native picker tokens bind configuration and cancelled picker retains draft', async () => {
  const calls = []; let cancelled = false;
  const h = harness(async (name, args) => { calls.push([name, args]); return name === 'pick_remote_setup_file' ? cancelled ? null : draft() : selected(); });
  h.intent({ type: 'pick-setup', part: 'identity', path: '/forged' }); await settle();
  assert.deepEqual(calls[0], ['pick_remote_setup_file', { draft: '', part: 'identity' }]);
  cancelled = true; h.intent({ type: 'pick-setup', part: 'hosts' }); await settle();
  assert.equal(h.projections.at(-1).draft.id, 'd'.repeat(64));
  h.intent({ type: 'configure', input: { ...form(), identity: '/forged' } }); await settle();
  assert.equal(calls.at(-1)[0], 'configure_remote_observation');
  assert.equal(calls.at(-1)[1].draft, 'd'.repeat(64));
  assert.equal(JSON.parse(calls.at(-1)[1].input).identity, undefined);
  assert.equal(h.projections.at(-1).selection.id, id); h.dispose();
});
test('incomplete drafts cannot configure and clearing setup does not forget active selection', async () => {
  const calls = [], h = harness(async name => { calls.push(name); return name === 'pick_remote_observation' ? selected() : draft({ installation: false, identity: false, hosts: false }); });
  h.intent({ type: 'configure', input: form() }); await settle(); assert.equal(calls.length, 0);
  h.intent({ type: 'choose' }); await settle(); h.intent({ type: 'clear-setup' }); await settle();
  assert.equal(h.projections.at(-1).selection.id, id); assert.equal(h.projections.at(-1).draft.identity, false);
  h.intent({ type: 'configure', input: form() }); await settle(); assert.equal(calls.includes('configure_remote_observation'), false); h.dispose();
});
test('refused setup preserves the active connection and suppresses duplicate configuration', async () => {
  let reject; const calls = [];
  const h = harness(async name => { calls.push(name); if (name === 'pick_remote_observation') return selected(); if (name === 'pick_remote_setup_file') return draft(); return new Promise((_, failure) => { reject = failure; }); });
  h.intent({ type: 'choose' }); await settle(); h.intent({ type: 'pick-setup', part: 'identity' }); await settle();
  h.intent({ type: 'configure', input: form() }); await settle(); h.intent({ type: 'configure', input: form() });
  reject(new Error('changed file')); await settle();
  assert.equal(calls.filter(name => name === 'configure_remote_observation').length, 1);
  assert.equal(h.projections.at(-1).selection.id, id); assert.ok(h.projections.at(-1).error); h.dispose();
});

const profile = () => ({ id: 'e'.repeat(64), label: 'My worker', ...Object.fromEntries(['host', 'worker', 'objective', 'lane', 'run'].map(key => [key, selected()[key]])) });
const profiles = () => ({ schema: 'mesh.remote-connection-profiles/v1', revision: '4', entries: [profile()] });
const opened = () => ({ schema: 'mesh.remote-profile-open/v1', profile: profile().id, label: profile().label, selection: selected(), draft: draft(), input: form() });
test('saved profiles bound labels, count, revision and identities while stripping private fields', () => {
  assert.equal(profilesReply({ ...profiles(), entries: [{ ...profile(), configuration: '/private/key' }] }).entries[0].configuration, undefined);
  for (const revision of ['04', '-1', '18446744073709551616']) assert.throws(() => profilesReply({ ...profiles(), revision }));
  assert.throws(() => profilesReply({ ...profiles(), entries: [profile(), profile()] }));
  for (const label of ['', 'x'.repeat(129), 'worker\u202e', 'worker\n', 'worker\u061c']) assert.throws(() => profilesReply({ ...profiles(), entries: [{ ...profile(), label }] }));
  assert.throws(() => profileOpenReply({ ...opened(), profile: 'f'.repeat(64) }, profile()));
  assert.throws(() => profileOpenReply({ ...opened(), input: { ...form(), run: 'other' } }, profile()));
  assert.throws(() => profileOpenReply({ ...opened(), draft: draft({ hosts: false }) }, profile()));
});
test('profiles open explicitly with retained revision, prefill only public fields and never contact worker', async () => {
  const calls = [], h = harness(async (name, args) => { calls.push([name, args]); return args.action === 'open' ? { ...opened(), input: { ...form(), identity: '/private' } } : profiles(); });
  assert.equal(calls.length, 0);
  h.intent({ type: 'profile-open', profile: profile().id }); await settle(); assert.equal(calls.length, 0);
  h.intent({ type: 'profiles-list' }); await settle();
  h.intent({ type: 'profile-open', profile: profile().id, revision: '999', path: '/forged' }); await settle();
  assert.deepEqual(calls.at(-1), ['remote_connection_profiles', { action: 'open', selection: '', label: '', profile: profile().id, revision: '4' }]);
  const p = h.projections.at(-1); assert.deepEqual(p.preset.input, form()); assert.equal(p.selection.id, id); assert.equal(p.status, null);
  assert.equal(calls.length, 2); h.dispose();
});
test('save uses native selection and removal preserves active selection; failed reopen preserves state', async () => {
  const calls = []; let fail = false;
  const h = harness(async (name, args) => { calls.push([name, args]); if (name === 'pick_remote_observation') return selected(); if (fail) throw Error('stale'); return profiles(); });
  h.intent({ type: 'choose' }); await settle(); h.intent({ type: 'profiles-list' }); await settle();
  h.intent({ type: 'profile-save', selection: 'forged', label: 'Named', revision: '0' }); await settle();
  assert.deepEqual(calls.at(-1)[1], { action: 'save', selection: id, label: 'Named', profile: '', revision: '4' });
  h.intent({ type: 'profile-remove', profile: profile().id }); await settle(); assert.equal(h.projections.at(-1).selection.id, id);
  fail = true; h.intent({ type: 'profile-open', profile: profile().id }); await settle();
  assert.equal(h.projections.at(-1).selection.id, id); assert.equal(h.projections.at(-1).profiles.entries[0].id, profile().id); assert.ok(h.projections.at(-1).error); h.dispose();
});
test('profile recovery is explicit and pending actions suppress duplicates', async () => {
  const calls = []; let release;
  const h = harness(async (name, args) => { calls.push([name, args]); return new Promise(resolve => { release = resolve; }); });
  h.intent({ type: 'profiles-recover', revision: '4', profile: 'forged' }); await settle();
  h.intent({ type: 'profiles-list' }); await settle(); assert.equal(calls.length, 1);
  assert.deepEqual(calls[0][1], { action: 'recover', selection: '', label: '', profile: '', revision: '' });
  release(profiles()); await settle(); assert.equal(h.projections.at(-1).profiles.revision, '4'); h.dispose();
});

const recovered = () => ({ schema: 'mesh.remote-panel-input-recovery/v1', id, objective: 'fleet-one', lane: 'lane-one', run: 'run-one', disposition: 'input-retained' });
test('input recovery verifies exact selection and outcome without claiming worker liveness', () => {
  assert.equal(inputRecoveryReply(recovered(), selected()).disposition, 'input-retained');
  for (const key of ['id', 'objective', 'lane', 'run', 'disposition']) assert.throws(() => inputRecoveryReply({ ...recovered(), [key]: 'different' }, selected()));
});
test('input recovery sends only native selection and never retries after an uncertain reply', async () => {
  const calls = []; let release;
  const h = harness(async (name, args) => { calls.push([name, args]); return name === 'pick_remote_observation' ? selected() : new Promise(resolve => { release = resolve; }); });
  h.intent({ type: 'choose' }); await settle();
  h.intent({ type: 'reconnect-input', id: 'forged', input: '/forged', lease: 999 }); await settle();
  h.intent({ type: 'reconnect-input' }); await settle(); assert.equal(calls.length, 2);
  assert.deepEqual(calls[1], ['reconnect_remote_input', { id }]);
  release({ ...recovered(), run: 'wrong' }); await settle(); assert.equal(calls.length, 2);
  assert.match(h.projections.at(-1).error, /No new attempt/); assert.equal(h.projections.at(-1).recovery, null); h.dispose();
});
test('successful recovery is displayed and forgetting clears its selection-bound outcome', async () => {
  const h = harness(async name => name === 'pick_remote_observation' ? selected() : name === 'reconnect_remote_input' ? recovered() : undefined);
  h.intent({ type: 'choose' }); await settle(); h.intent({ type: 'reconnect-input' }); await settle();
  assert.equal(h.projections.at(-1).recovery.disposition, 'input-retained');
  h.intent({ type: 'forget' }); await settle(); assert.equal(h.projections.at(-1).recovery, null); h.dispose();
});
