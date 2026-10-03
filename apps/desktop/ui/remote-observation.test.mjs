import test from 'node:test';
import assert from 'node:assert/strict';
import { startRemoteObservation, selectionReply, observationReply, draftReply, setupInput, profilesReply, profileOpenReply, inputRecoveryReply, originalRecoveryReply, receiptAttemptsReply, receivedResultReply } from './remote-observation.js';
const id = 'a'.repeat(64);
const selected = () => ({ schema: 'mesh.remote-panel-selection/v1', id, host: 'worker.example', worker: 'b'.repeat(64), objective: 'fleet-one', lane: 'lane-one', run: 'run-one' });
const status = () => ({ schema: 'mesh.remote-panel-observation/v2', id, kind: 'status', observed_ms: '1000', admitted: true, launch_recorded: false, lease_until_ms: '2000' });
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
  const value = { schema: 'mesh.remote-panel-observation/v2', id, kind: 'results', available: false, revision: null, after: null, count: 0, has_more: false, entries: [] };
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

const resultEntry = n => ({ offer: n.toString(16).padStart(64, '0'), checkpoint: `checkpoint-${n}`, version: 'b'.repeat(64), review: 'c'.repeat(64), manifest: 'd'.repeat(64) });
const resultPage = (before, revision) => { const count = Math.min(16, revision - before); return { schema: 'mesh.remote-panel-observation/v2', id, kind: 'results', available: true, revision: String(revision), after: before + count, count, has_more: before + count < revision, entries: Array.from({ length: count }, (_, i) => resultEntry(before + i + 1)) }; };
test('nonempty first pages and later pages validate the returned cursor rather than expecting zero', () => {
  const first = observationReply(resultPage(0, 17), id, 'results'); assert.equal(first.after, 16); assert.equal(first.count, 16);
  const next = observationReply(resultPage(16, 17), id, 'results', 16); assert.equal(next.after, 17); assert.equal(next.entries[0].checkpoint, 'checkpoint-17'); assert.equal(next.hasMore, false);
  assert.throws(() => observationReply(resultPage(16, 17), id, 'results', 0));
  assert.throws(() => observationReply({ ...resultPage(0, 17), after: 0 }, id, 'results'));
  assert.throws(() => observationReply({ ...resultPage(0, 17), entries: [] }, id, 'results'));
  assert.throws(() => observationReply({ ...resultPage(0, 17), entries: Array(16).fill(resultEntry(1)) }, id, 'results'));
  assert.throws(() => observationReply(resultPage(0, 4097), id, 'results'));
  assert.equal(observationReply({ ...resultPage(0, 1), entries: [{ ...resultEntry(1), target: '/private' }] }, id, 'results').entries[0].target, undefined);
});
test('next and previous page reads use the verified cursor and keep a bounded current page', async () => {
  const calls = [], h = harness(async (name, args) => { calls.push([name, args]); return name === 'pick_remote_observation' ? selected() : resultPage(args.after, 17); });
  h.intent({ type: 'choose' }); await settle(); h.intent({ type: 'results-next' }); await settle(); assert.equal(calls.length, 1);
  h.intent({ type: 'results', after: 800 }); await settle();
  h.intent({ type: 'results-next', after: 800 }); await settle(); assert.equal(calls.at(-1)[1].after, 16); assert.equal(h.projections.at(-1).results.entries.length, 1);
  h.intent({ type: 'results-next' }); await settle(); assert.equal(calls.length, 3);
  h.intent({ type: 'results-previous', after: 800 }); await settle(); assert.equal(calls.at(-1)[1].after, 0); assert.equal(h.projections.at(-1).results.entries.length, 16); h.dispose();
});
test('invalid or regressed page preserves the previous verified results without retry', async () => {
  let revision = 17, fail = false; const calls = [];
  const h = harness(async (name, args) => { calls.push([name,args]); if (name === 'pick_remote_observation') return selected(); return fail ? { ...resultPage(args.after, revision), id: 'wrong' } : resultPage(args.after, revision); });
  h.intent({ type: 'choose' }); await settle(); h.intent({ type: 'results' }); await settle();
  fail = true; h.intent({ type: 'results-next' }); await settle(); assert.equal(h.projections.at(-1).results.after, 16); assert.ok(h.projections.at(-1).error); assert.equal(calls.length,3);
  fail = false; revision = 16; h.intent({ type: 'results' }); await settle(); assert.equal(h.projections.at(-1).results.revision, '17'); assert.ok(h.projections.at(-1).error); h.dispose();
});

const receiptOffer = 'd'.repeat(64);
const receiptEntry = () => ({ offer: receiptOffer, checkpoint: 'checkpoint', version: 'e'.repeat(64) });
const receiptAttempts = () => ({ schema: 'mesh.remote-panel-receipt-attempts/v1', id, entries: [receiptEntry()] });
const receivedResult = () => ({ schema: 'mesh.remote-panel-received-result/v1', id, offer: receiptOffer, objective: 'fleet-one', lane: 'lane-one', run: 'run-one', correlation: 'f'.repeat(64), version: 'e'.repeat(64), review: 'c'.repeat(64) });
test('receipt projections bind exact selection and offer and strip private native data', () => {
  assert.equal(receiptAttemptsReply(receiptAttempts(), selectionReply(selected())).length, 1);
  assert.throws(() => receiptAttemptsReply({ ...receiptAttempts(), id: 'b'.repeat(64) }, selectionReply(selected())));
  assert.throws(() => receiptAttemptsReply({ ...receiptAttempts(), entries: [receiptEntry(), receiptEntry()] }, selectionReply(selected())));
  assert.throws(() => receiptAttemptsReply({ ...receiptAttempts(), entries: Array(65).fill(receiptEntry()) }, selectionReply(selected())));
  const value = receivedResultReply({ ...receivedResult(), path: '/private/receiving' }, selectionReply(selected()), receiptOffer);
  assert.equal(value.path, undefined);
  for (const patch of [{ offer: 'a'.repeat(64) }, { id: 'b'.repeat(64) }, { lane: 'different' }, { review: '../path' }]) {
    assert.throws(() => receivedResultReply({ ...receivedResult(), ...patch }, selectionReply(selected()), receiptOffer));
  }
});
test('download sends only a retained page selector and rejects arbitrary offers', async () => {
  const calls = [], page = resultPage(0, 1); page.entries[0].offer = receiptOffer;
  const h = harness(async (name, args) => { calls.push([name, args]); return name === 'pick_remote_observation' ? selected() : name === 'read_remote_observation' ? page : receivedResult(); });
  h.intent({ type: 'choose' }); await settle();
  h.intent({ type: 'download-result', offer: receiptOffer }); await settle(); assert.equal(calls.length, 1);
  h.intent({ type: 'results' }); await settle();
  h.intent({ type: 'download-result', offer: 'f'.repeat(64) }); await settle(); assert.equal(calls.length, 2);
  h.intent({ type: 'download-result', offer: receiptOffer, path: '/renderer/path', allocation: 'forged' }); await settle();
  assert.deepEqual(calls.at(-1), ['remote_result_receipt', { id, action: 'receive', offer: receiptOffer }]);
  assert.equal(h.projections.at(-1).received.correlation, 'f'.repeat(64)); h.dispose();
});
test('recovery requires loaded native intent and an inflight receipt cannot be duplicated', async () => {
  const calls = []; let release;
  const h = harness(async (name, args) => { calls.push([name, args]); if (name === 'pick_remote_observation') return selected(); if (args.action === 'list') return receiptAttempts(); return new Promise(resolve => { release = resolve; }); });
  h.intent({ type: 'choose' }); await settle();
  h.intent({ type: 'recover-result', offer: receiptOffer }); await settle(); assert.equal(calls.length, 1);
  h.intent({ type: 'receipt-list' }); await settle();
  h.intent({ type: 'recover-result', offer: receiptOffer }); await settle();
  h.intent({ type: 'recover-result', offer: receiptOffer }); h.intent({ type: 'forget' });
  assert.equal(calls.length, 3); assert.equal(h.projections.at(-1).busy, true);
  release({ ...receivedResult(), offer: 'b'.repeat(64) }); await settle();
  assert.equal(h.projections.at(-1).received, null); assert.match(h.projections.at(-1).error, /could not be confirmed/);
  assert.equal(h.projections.at(-1).receiptAttempts.length, 1);
  assert.deepEqual(calls.at(-1), ['remote_result_receipt', { id, action: 'recover', offer: receiptOffer }]); h.dispose();
});
test('a changed selection clears saved download and completion projections', async () => {
  const h = harness(async (name, args) => name === 'pick_remote_observation' ? selected() : args.action === 'list' ? receiptAttempts() : receivedResult());
  h.intent({ type: 'choose' }); await settle(); h.intent({ type: 'receipt-list' }); await settle();
  h.intent({ type: 'recover-result', offer: receiptOffer }); await settle();
  assert.ok(h.projections.at(-1).received);
  h.intent({ type: 'choose' }); await settle();
  assert.equal(h.projections.at(-1).receiptAttempts, null); assert.equal(h.projections.at(-1).received, null); h.dispose();
});

const creationInputFixture = () => ({connection:{host:'worker.example',account:'mesh',port:'22',worker:'b'.repeat(64)},project:'c'.repeat(64),version:'d'.repeat(64),goal:'Use the saved input',provider:'codex',limits:{lanes:4,concurrency:2,depth:1,retries:0}});
const creationEntryFixture = () => ({request:'a'.repeat(32),project:'c'.repeat(64),version:'d'.repeat(64),goal:'Use the saved input',provider:'codex',host:'worker.example',worker:'b'.repeat(64),lease_until_ms:'9999999999999',limits:{lanes:4,concurrency:2,depth:1,retries:0}});
const creationDraft = () => ({schema:'mesh.remote-setup-draft/v1',id,installation:true,identity:true,hosts:true});
test('fresh remote preparation requires no existing run and strips renderer authority', async () => {
  const calls=[];const h=harness(async(name,args)=>{calls.push([name,args]);if(name==='clear_remote_setup')return creationDraft();return {schema:'mesh.remote-creation-prepared/v1',draft:id,entry:{...creationEntryFixture(),identity:'/private/key'}};});
  h.intent({type:'clear-setup'});await settle();
  const input={...creationInputFixture(),request:'forged',storage:'/forged',lease_until_ms:999};input.connection.identity='/forged';
  h.intent({type:'creation-prepare',input});await settle();
  assert.equal(calls[1][0],'remote_creation');assert.equal(calls[1][1].id,id);assert.equal(calls[1][1].action,'prepare');
  assert.deepEqual(JSON.parse(calls[1][1].input),{...creationInputFixture(),connection:{...creationInputFixture().connection,port:22}});
  const p=h.projections.at(-1);assert.equal(p.creationStatus.kind,'prepared');assert.equal(p.creations[0].identity,undefined);assert.equal(p.selection,null);h.dispose();
});
test('lost send reply suppresses duplicate input dispatch until explicit native inspection', async () => {
  const calls=[];let fail;
  const h=harness(async(name,args)=>{calls.push([name,args]);if(name==='clear_remote_setup')return creationDraft();
    if(args.action==='prepare')return{schema:'mesh.remote-creation-prepared/v1',draft:id,entry:creationEntryFixture()};
    if(args.action==='send')return new Promise((_,reject)=>{fail=reject;});
    return{schema:'mesh.remote-creation-inspected/v1',request:creationEntryFixture().request,allocated:true,selection:{...selected(),run:`start-${creationEntryFixture().request}`}};
  });
  h.intent({type:'clear-setup'});await settle();h.intent({type:'creation-prepare',input:creationInputFixture()});await settle();
  const detail={type:'creation-send',request:creationEntryFixture().request};h.intent(detail);await settle();h.intent(detail);assert.equal(calls.filter(([,a])=>a?.action==='send').length,1);
  fail(new Error('lost output'));await settle();h.intent(detail);await settle();assert.equal(calls.filter(([,a])=>a?.action==='send').length,1);
  assert.match(h.projections.at(-1).error,/inspect the original/);assert.equal(h.projections.at(-1).creationStatus,null);
  h.intent({type:'creation-inspect',request:detail.request});await settle();assert.equal(h.projections.at(-1).selection.run,`start-${detail.request}`);assert.equal(h.projections.at(-1).creationStatus.kind,'allocated');
  h.intent(detail);await settle();assert.equal(calls.filter(([,a])=>a?.action==='send').length,1);h.dispose();
});
test('restarted creation listing does not dispatch and mismatched inspection cannot switch selection', async () => {
  const calls=[];const h=harness(async(name,args)=>{calls.push([name,args]);return args.action==='list'?{schema:'mesh.remote-creation-list/v1',entries:[creationEntryFixture()]}:{schema:'mesh.remote-creation-inspected/v1',request:creationEntryFixture().request,allocated:true,selection:selected()};});
  h.intent({type:'creation-list'});await settle();h.intent({type:'creation-send',request:creationEntryFixture().request});await settle();assert.equal(calls.length,1);
  h.intent({type:'creation-inspect',request:'f'.repeat(32)});await settle();assert.equal(calls.length,1);
  h.intent({type:'creation-inspect',request:creationEntryFixture().request});await settle();assert.equal(h.projections.at(-1).selection,null);assert.equal(h.projections.at(-1).creationStatus,null);assert.match(h.projections.at(-1).error,/could not be confirmed/);h.dispose();
});
test('changed prepared inputs cannot present a ready request', async () => {
  const h=harness(async name=>name==='clear_remote_setup'?creationDraft():{schema:'mesh.remote-creation-prepared/v1',draft:id,entry:{...creationEntryFixture(),version:'e'.repeat(64)}});
  h.intent({type:'clear-setup'});await settle();h.intent({type:'creation-prepare',input:creationInputFixture()});await settle();assert.equal(h.projections.at(-1).creations,null);assert.equal(h.projections.at(-1).creationStatus,null);h.dispose();
});

const inspection = (disposition = 'verified') => ({ schema: 'mesh.remote-panel-observation/v2', id, kind: 'input-inspection', observed_ms: '1001', disposition });
test('input inspection accepts only explicit native outcomes and strips unrelated authority', () => {
  for (const disposition of ['verified', 'unrecorded', 'unavailable']) assert.deepEqual(observationReply({ ...inspection(disposition), restart: true, path: '/private' }, id, 'input-inspection'), { kind: 'input-inspection', observed: '1001', disposition });
  for (const disposition of ['running', 'ready', '', null]) assert.throws(() => observationReply(inspection(disposition), id, 'input-inspection'));
  assert.throws(() => observationReply(status(), id, 'input-inspection'));
  assert.throws(() => observationReply(inspection(), 'b'.repeat(64), 'input-inspection'));
});
test('inspection is explicit and keeps status separate; failures retain dated facts and selection changes clear them', async () => {
  const calls = []; let fail = false;
  const h = harness(async (name, args) => {
    calls.push([name, args]);
    if (name === 'pick_remote_observation') return selected();
    if (name === 'forget_remote_observation') return;
    if (args.action === 'status') return status();
    if (fail) throw Error('offline');
    return inspection();
  });
  h.intent({ type: 'input-inspection' }); await settle(); assert.equal(calls.length, 0);
  h.intent({ type: 'choose' }); await settle(); assert.equal(calls.length, 1);
  h.intent({ type: 'status' }); await settle(); assert.equal(h.projections.at(-1).inspection, null);
  h.intent({ type: 'input-inspection', id: 'forged', after: 999, path: '/private' }); await settle();
  assert.deepEqual(calls.at(-1), ['read_remote_observation', { id, action: 'input-inspection', after: 0 }]);
  assert.equal(h.projections.at(-1).inspection.disposition, 'verified');
  assert.equal(h.projections.at(-1).status.observed, '1000');
  fail = true; h.intent({ type: 'input-inspection' }); await settle();
  assert.equal(h.projections.at(-1).inspection.observed, '1001'); assert.match(h.projections.at(-1).error, /out of date/);
  h.intent({ type: 'forget' }); await settle(); assert.equal(h.projections.at(-1).inspection, null);
  h.dispose();
});

const originalRecovered = () => ({ schema: 'mesh.remote-panel-original-recovery/v1', id, objective: 'fleet-one', lane: 'lane-one', run: 'run-one', disposition: 'initialization-recovered' });
test('original recovery rejects mismatched attempts and strips non-authoritative extra claims', () => {
  assert.deepEqual(originalRecoveryReply({ ...originalRecovered(), running: true, path: '/private' }, selected()), { disposition: 'initialization-recovered' });
  for (const [key, value] of [['id', 'b'.repeat(64)], ['objective', 'other'], ['lane', 'other'], ['run', 'other'], ['schema', 'mesh.remote-panel-original-recovery/v2'], ['disposition', 'running']]) {
    assert.throws(() => originalRecoveryReply({ ...originalRecovered(), [key]: value }, selected()));
  }
});
test('original recovery uses retained selection once, preserves observations and never retries after failure', async () => {
  const calls = []; let resolve, fail = false;
  const h = harness(async (name, args) => {
    calls.push([name, args]);
    if (name === 'pick_remote_observation') return selected();
    if (name === 'read_remote_observation') return status();
    if (fail) throw new Error('lost reply');
    return new Promise(done => { resolve = done; });
  });
  h.intent({ type: 'recover-original' }); await settle(); assert.equal(calls.length, 0);
  h.intent({ type: 'choose' }); await settle();
  h.intent({ type: 'status' }); await settle();
  const prior = h.projections.at(-1).status;
  h.intent({ type: 'recover-original', id: 'forged', path: '/private', run: 'other' }); await settle();
  h.intent({ type: 'recover-original' }); h.intent({ type: 'forget' }); await settle();
  assert.equal(calls.length, 3); assert.deepEqual(calls.at(-1), ['recover_original_remote_worker', { id }]);
  resolve(originalRecovered()); await settle();
  assert.equal(h.projections.at(-1).originalRecovery.disposition, 'initialization-recovered');
  assert.equal(h.projections.at(-1).status, prior);
  fail = true; h.intent({ type: 'recover-original' }); await settle(); await settle();
  assert.equal(calls.length, 4); assert.equal(h.projections.at(-1).originalRecovery, null);
  assert.equal(h.projections.at(-1).selection.id, id); assert.equal(h.projections.at(-1).status, prior);
  assert.match(h.projections.at(-1).error, /could not be confirmed/);
  h.dispose();
});
