import test from 'node:test';
import assert from 'node:assert/strict';
import { projectCandidateReview, projectCandidateReceipt, projectMappingMain } from './fleet-project-comparison.js';
import { createFleetReviews } from './fleet-reviews.js';
import { defaultFleetView, fleetPinSnapshot } from './fleet-pin-persistence.js';
const hex = c => c.repeat(64), project = hex('1');
const pending = () => ({ project, request: '2'.repeat(32), expected_main: null });
const pin = (key = '1') => ({ key, selection: { objective: `fleet-${hex('a')}`, lane: `lane-${key}`, checkpoint: `checkpoint-${key}`, version: hex('b'), bundle: hex('c') }, startingInput: hex('d'), view: defaultFleetView() });
const selection = p => Object.fromEntries(['lane', 'checkpoint', 'version', 'bundle'].map(key => [key, p.selection[key]]));
const provenance = (p, input) => ({ schema: 'mesh.fleet-candidate-provenance/v1', objective: p.selection.objective, selection: selection(p), source_project: project, source_version: p.startingInput,
  lineage: [{ lane: p.selection.lane, source_version: p.startingInput, starting_version: hex('e'), result_version: p.selection.version }], expected_main: input.expected_main,
  origin: { actor: 'actor', session: 'session', run: 'run', generation: 'generation' }, attribution: 'recorded-agent-checkpoint', approval_authority: false });
const envelope = (p, input, result, review = false) => ({ schema: review ? 'mesh.desktop-fleet-candidate-review/v1' : 'mesh.desktop-fleet-candidate/v1', project, objective: p.selection.objective,
  selection: selection(p), request: input.request, expected_main: input.expected_main, result });
const receipt = (p, input) => envelope(p, input, { schema: 'mesh.fleet-project-candidate/v1', candidate: `candidate-${hex('f')}`, project, provenance: provenance(p, input), content_digest: hex('3'), files: 1, directories: 0, bytes: 6, state: 'staged', approval_authority: false });
function review(p, input, after = null, selected = null) {
  return envelope(p, input, { schema: 'mesh.fleet-project-candidate-review/v1', review: hex('4'), context: { schema: 'mesh.fleet-project-review-context/v1', scope: 'whole-project-snapshot', project,
    candidate: `candidate-${hex('f')}`, candidate_receipt_digest: hex('5'), base_head: input.expected_main, base_version: input.expected_main === null ? null : hex('6'), target_version: p.selection.version, content_digest: hex('3') },
  observed_main: input.expected_main === null ? null : { head: input.expected_main, bundle: hex('7'), target: hex('6') }, provenance: provenance(p, input), base_is_current: true, approval_authority: false,
  comparison: { order: 'path', after, selected, total: 1, next_after: null, changes: [{ path: selected ?? 'work.txt', change: 'added', before: null,
    after: { entry: { kind: 'file', digest: hex('8'), bytes: 6, executable: false }, content_state: selected ? 'text' : 'not-requested', text: selected ? 'saved\n' : null } }] } }, true);
}
const mapping = (p, main = null) => ({ schema: 'mesh.fleet-project-mapping/v2', objective: p.selection.objective, selection: selection(p), mapping: { source_project: project, scope: 'recorded-input-ancestry', observed_main: main }, approval_authority: false });
const storedPin = p => ({ key: p.key, ...p.selection, source_version: p.startingInput, ...p.view });
const snapshot = pins => ({ schema: 'mesh.desktop-fleet-pin-selectors/v2', revision: '1', pins: pins.map(storedPin) });
const settle = () => new Promise(resolve => setImmediate(resolve));
test('candidate envelopes reject substituted requests, origins, main and authority', () => {
  const p = pin(), input = pending();
  assert.equal(projectCandidateReceipt(receipt(p, input), p, input).candidate, `candidate-${hex('f')}`);
  assert.equal(projectMappingMain(mapping(p), p, project), null);
  for (const mutate of [v => v.request = '9'.repeat(32), v => v.result.provenance.selection.lane = 'other', v => v.result.project = hex('9'),
    v => v.result.provenance.lineage[0].source_version = hex('9'), v => v.result.provenance.origin.extra = true, v => v.result.approval_authority = true]) {
    const value = receipt(p, input); mutate(value); assert.throws(() => projectCandidateReceipt(value, p, input));
  }
  const wrongMain = mapping(p, { head: 'latest', bundle: hex('7'), target: hex('6') }); assert.throws(() => projectMappingMain(wrongMain, p, project));
});
test('fixed reviews bind identity across pages and main observations, and bound text', () => {
  const p = pin(), input = pending(), first = projectCandidateReview(review(p, input), p, input);
  const moved = review(p, input, null, 'work.txt'); moved.result.observed_main = { head: hex('9'), bundle: hex('7'), target: hex('6') }; moved.result.base_is_current = false;
  const file = projectCandidateReview(moved, p, input, null, 'work.txt', first);
  assert.equal(file.baseIsCurrent, false); assert.equal(file.changes[0].after.text, 'saved\n');
  for (const mutate of [v => v.result.review = hex('9'), v => v.result.context.base_head = hex('9'), v => v.result.base_is_current = true,
    v => v.result.comparison.changes[0].after.text = 'bad\u202etext', v => v.result.comparison.changes[0].after.entry.bytes = 262145,
    v => v.result.comparison.changes[0].path = '../work.txt', v => v.result.comparison.selected = 'different', v => v.result.comparison.changes[0].change = 'removed']) {
    const value = structuredClone(moved); mutate(value); assert.throws(() => projectCandidateReview(value, p, input, null, 'work.txt', first));
  }
  assert.throws(() => projectCandidateReview(moved, p, input, 'work.txt', 'work.txt', first));
});
test('legacy navigation upgrades without inventing a pending operation; candidate selectors stay closed', () => {
  const p = storedPin(pin()); delete p.candidate;
  const restored = fleetPinSnapshot({ schema: 'mesh.desktop-fleet-pin-selectors/v1', revision: '1', pins: [p] });
  assert.equal(restored.schema, 'mesh.desktop-fleet-pin-selectors/v2'); assert.equal(restored.pins[0].candidate, null);
  for (const candidate of [{ ...pending(), content: 'secret' }, { ...pending(), expected_main: 'latest' }, { ...pending(), request: '' }, { ...pending(), project: '../project' }]) {
    assert.throws(() => fleetPinSnapshot(snapshot([{ ...pin(), view: { ...defaultFleetView(), candidate } }])));
  }
});
function harness(pins = [pin()], intercept = () => undefined) {
  let stored = snapshot(pins); const calls = [];
  const h = createFleetReviews({ changed() {}, laneFor: () => ({ sourceProject: project, goal: 'Work', base: hex('d') }), requestId: () => '2'.repeat(32),
    invoke: async (command, args) => {
      calls.push({ command, args }); const intercepted = intercept(command, args); if (intercepted !== undefined) return intercepted;
      if (command === 'load_fleet_pins') return stored;
      if (command === 'save_fleet_pins') { stored = { ...JSON.parse(args.snapshot), revision: String(Number(stored.revision) + 1) }; return stored; }
      const p = pins.find(p => p.selection.lane === args.lane);
      if (command === 'inspect_fleet_saved_review') throw new Error('Separate recorded review unavailable');
      if (command === 'fleet_project_mapping') return mapping(p);
      const input = { project: args.project, request: args.request, expected_main: args.expectedMain };
      if (command === 'prepare_fleet_project_candidate') return receipt(p, input);
      if (command === 'review_fleet_project_candidate') return review(p, input, args.after, args.selected);
      throw new Error(`Unexpected ${command}`);
    } });
  return { ...h, calls, stored: () => stored };
}
test('preparation waits for durable selectors, and retry never reads a newer main', async () => {
  let finishSave, failPreparation = true;
  const h = harness([pin()], (command, args) => {
    if (command === 'save_fleet_pins') return new Promise(resolve => { finishSave = () => resolve({ ...JSON.parse(args.snapshot), revision: '2' }); });
    if (command === 'prepare_fleet_project_candidate' && failPreparation) return Promise.reject(new Error('lost receipt'));
  });
  await h.loadSaved(); h.handle({ type: 'candidate-prepare', pin: '1' }); await settle();
  assert.equal(h.calls.some(c => c.command === 'prepare_fleet_project_candidate'), false);
  finishSave(); await settle(); assert.match(h.snapshot().reviewPins[0].candidate.error, /unconfirmed/);
  const original = h.calls.find(c => c.command === 'prepare_fleet_project_candidate').args;
  failPreparation = false; h.handle({ type: 'candidate-retry', pin: '1' }); await settle();
  assert.deepEqual(h.calls.filter(c => c.command === 'prepare_fleet_project_candidate').map(c => c.args), [original, original]);
  assert.equal(h.calls.filter(c => c.command === 'fleet_project_mapping').length, 1);
  assert.equal(h.snapshot().reviewPins[0].candidate.page.total, 1); h.dispose();
});
test('failed persistence prevents preparation and exact saved retry recovers', async () => {
  let fail = true;
  const h = harness([pin()], (command) => command === 'save_fleet_pins' && fail ? Promise.reject(new Error('disk unavailable')) : undefined);
  await h.loadSaved(); h.handle({ type: 'candidate-prepare', pin: '1' }); await settle();
  assert.equal(h.calls.some(c => c.command === 'prepare_fleet_project_candidate'), false);
  const expected = h.snapshot().reviewPins[0].view.candidate;
  fail = false; h.handle({ type: 'retry-saved-reviews' }); await settle();
  h.handle({ type: 'candidate-retry', pin: '1' }); await settle();
  assert.deepEqual(h.snapshot().reviewPins[0].view.candidate, expected);
  assert.equal(h.snapshot().reviewPins[0].candidate.page.total, 1); h.dispose();
});
test('restart reads exact inputs without preparation and parallel pins retain independent files', async () => {
  const first = pin('1'), second = pin('2'); first.view.candidate = pending(); second.view.candidate = { ...pending(), request: '3'.repeat(32) };
  const h = harness([first, second]); await h.loadSaved(); await settle();
  assert.equal(h.calls.some(c => ['fleet_project_mapping', 'prepare_fleet_project_candidate', 'save_fleet_pins'].includes(c.command)), false);
  h.handle({ type: 'candidate-file', pin: '1', path: 'work.txt' }); await settle();
  assert.equal(h.snapshot().reviewPins[0].candidate.file.after.text, 'saved\n'); assert.equal(h.snapshot().reviewPins[1].candidate.file, null);
  h.dispose();
});
test('closed and replaced panels ignore late comparisons and do not recreate pins', async () => {
  const p = pin(); p.view.candidate = pending(); let finish;
  const h = harness([p], command => command === 'review_fleet_project_candidate' ? new Promise(resolve => { finish = resolve; }) : undefined);
  await h.loadSaved(); await settle(); h.handle({ type: 'close-review', pin: '1' }); finish(review(p, pending())); await settle();
  assert.equal(h.snapshot().reviewPins.length, 0); assert.equal(h.stored().pins.length, 0); h.dispose();
});

test('pagination preserves review identity and uses native Unicode path order', () => {
  const p = pin(), input = pending(), value = review(p, input);
  const row = value.result.comparison.changes[0];
  value.result.comparison.total = 202;
  value.result.comparison.changes = Array.from({ length: 200 }, (_, n) => ({ ...structuredClone(row), path: `file-${String(n).padStart(3, '0')}` }));
  value.result.comparison.next_after = 'file-199';
  const first = projectCandidateReview(value, p, input);
  const second = review(p, input, 'file-199'); second.result.comparison.total = 202;
  second.result.comparison.changes = ['\ue000', '\u{10000}'].map(path => ({ ...structuredClone(row), path }));
  const next = projectCandidateReview(second, p, input, 'file-199', null, first);
  assert.equal(next.changes.length, 2);
  second.result.comparison.changes.reverse(); assert.throws(() => projectCandidateReview(second, p, input, 'file-199', null, first));
});
test('reloading during base discovery prevents the old request from persisting or preparing', async () => {
  let finish;
  const h = harness([pin()], command => command === 'fleet_project_mapping' ? new Promise(resolve => { finish = resolve; }) : undefined);
  await h.loadSaved(); h.handle({ type: 'candidate-prepare', pin: '1' }); await settle();
  h.handle({ type: 'reload-saved-reviews' }); await settle(); finish(mapping(pin())); await settle();
  assert.equal(h.calls.some(c => ['save_fleet_pins', 'prepare_fleet_project_candidate'].includes(c.command)), false);
  assert.equal(h.snapshot().reviewPins[0].view.candidate, null); h.dispose();
});
