import test from 'node:test';
import assert from 'node:assert/strict';
import { fleetInputComparison } from './fleet-input-comparison.js';
const digest = value => value.repeat(64), object = value => value.toString(16).padStart(32, '0');
const pin = () => ({ startingInput: digest('a'), selection: { objective: `fleet-${digest('b')}`, lane: 'lane', checkpoint: 'checkpoint', version: digest('c'), bundle: digest('d') } });
const side = (text = null) => ({ path: 'file.txt', kind: 'file', digest: digest('e'), bytes: 4, executable: false, content_state: text === null ? 'not-requested' : 'text', text });
const change = (id = 1, selected = false) => ({ object: object(id), effect: 'modified', before: side(selected ? 'old\n' : null), after: { ...side(selected ? 'new\n' : null), digest: digest('f') } });
const response = () => { const { objective, ...selection } = pin().selection; return { schema: 'mesh.fleet-starting-comparison/v1', objective, selection, approval_authority: false, input: { source_version: digest('a'), comparison: { base: digest('9'), target: digest('c'), order: 'object-id', after: null, selected: null, total: 1, changes: [change()], next_after: null, approval_authority: false } } }; };
test('comparison binds source, exact result, local base and closed selection', () => {
  assert.equal(fleetInputComparison(JSON.stringify(response()), pin()).changes[0].before.path, 'file.txt');
  for (const mutate of [v => v.objective = 'other', v => v.selection.bundle = digest('a'), v => v.selection.path = '/tmp', v => v.input.source_version = digest('f'), v => v.approval_authority = true, v => v.input.comparison.approval_authority = true, v => v.input.comparison.target = digest('1'), v => v.input.comparison.base = 'invalid', v => v.input.comparison.after = object(2), v => v.input.comparison.total = 2]) {
    const value = response(); mutate(value); assert.throws(() => fleetInputComparison(value, pin()));
  }
  const bound = pin(); bound.input = { page: fleetInputComparison(response(), bound) };
  const wrongBase = response(); wrongBase.input.comparison.base = digest('1'); assert.throws(() => fleetInputComparison(wrongBase, bound));
});
test('selected text is bounded and a missing side differs from empty text', () => {
  const value = response(); Object.assign(value.input.comparison, { selected: object(1), changes: [change(1, true)] });
  assert.equal(fleetInputComparison(value, pin(), null, object(1)).changes[0].before.text, 'old\n');
  for (const mutate of [v => v.changes[0].before.text = 'wrong-length', v => v.changes[0].before.text = '\u202eabc', v => v.changes[0].before.path = '../escape', v => v.changes[0].object = object(2), v => v.next_after = object(1), v => v.changes[0].before.content_state = 'not-requested']) {
    const altered = structuredClone(value); mutate(altered.input.comparison); assert.throws(() => fleetInputComparison(altered, pin(), null, object(1)));
  }
  Object.assign(value.input.comparison.changes[0], { before: null, effect: 'added', after: { ...side(''), bytes: 0 } });
  const added = fleetInputComparison(value, pin(), null, object(1)).changes[0]; assert.equal(added.before, null); assert.equal(added.after.text, '');
});
test('page cursors admit every changed object without duplicate or invented continuation', () => {
  const value = response(); Object.assign(value.input.comparison, { total: 201, changes: Array.from({ length: 200 }, (_, n) => change(n + 1)), next_after: object(200) });
  const first = fleetInputComparison(value, pin()); assert.equal(first.nextAfter, object(200));
  const bound = pin(); bound.input = { page: first };
  Object.assign(value.input.comparison, { after: object(200), changes: [change(201)], next_after: null });
  assert.equal(fleetInputComparison(value, bound, object(200)).changes[0].object, object(201));
  value.input.comparison.changes[0] = change(200); assert.throws(() => fleetInputComparison(value, bound, object(200)));
});
test('binary, large and folder metadata cannot masquerade as text', () => {
  for (const [state, bytes] of [['binary-or-unsafe-text', 4], ['too-large', 262145]]) {
    const value = response(); Object.assign(value.input.comparison, { selected: object(1), changes: [change(1, true)] });
    Object.assign(value.input.comparison.changes[0].after, { content_state: state, bytes, text: null });
    assert.equal(fleetInputComparison(value, pin(), null, object(1)).changes[0].after.state, state);
    value.input.comparison.changes[0].after.text = 'fake'; assert.throws(() => fleetInputComparison(value, pin(), null, object(1)));
  }
});

test('native file and folder counts are exact, bounded, stable and optional for legacy replies', () => {
  assert.equal(fleetInputComparison(response(), pin()).fileTotal, null);
  const value = response(), page = value.input.comparison;
  Object.assign(page, { file_total: 1, folder_total: 0 });
  const first = fleetInputComparison(value, pin());
  assert.equal(first.fileTotal, 1); assert.equal(first.folderTotal, 0);
  for (const fields of [{ file_total: null }, { folder_total: undefined }, { file_total: 0.5 }, { folder_total: 1 }, { file_total: 0, folder_total: 1 }, { file_total: -1 }, { file_total: Number.MAX_SAFE_INTEGER + 1 }]) {
    const bad = structuredClone(value); Object.assign(bad.input.comparison, fields);
    assert.throws(() => fleetInputComparison(bad, pin()));
  }
  const bound = pin(); bound.input = { page: first };
  assert.throws(() => fleetInputComparison(response(), bound), 'known counts cannot disappear during an exact comparison');
  const folder = { path: 'folder', kind: 'folder', digest: null, bytes: null, executable: null, content_state: 'folder', text: null };
  Object.assign(page, { total: 201, file_total: 200, folder_total: 1, changes: Array.from({ length: 200 }, (_, n) => change(n + 1)), next_after: object(200) });
  const paged = pin(); paged.input = { page: fleetInputComparison(value, pin()) };
  Object.assign(page, { after: object(200), changes: [{ object: object(201), effect: 'added', before: null, after: folder }], next_after: null });
  assert.equal(fleetInputComparison(value, paged, object(200)).fileTotal, 200);
  Object.assign(page, { file_total: 199, folder_total: 2 });
  assert.throws(() => fleetInputComparison(value, paged, object(200)));
});
