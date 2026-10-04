import test from 'node:test';
import assert from 'node:assert/strict';
import { savedProgressPage, savedProgressComparison } from './fleet-progress.js';
const hash = n => n.toString(16).padStart(64, '0');
const selection = { objective: `fleet-${hash(1)}`, lane: `lane-${hash(2)}`, source: hash(3), starting: hash(4), version: hash(5) };
const envelope = schema => ({ schema, objective: selection.objective, lane: selection.lane, revision: 7,
  source_version: selection.source, starting_version: selection.starting, latest_acknowledged_version: hash(9),
  handoff_authority: false, approval_authority: false });
const page = () => ({ ...envelope('mesh.fleet-saved-progress-page/v1'), progress: { order: 'causal-operation', after: null, total: 2,
  versions: [{ version: hash(4), ordinal: 1 }, { version: hash(5), ordinal: 2 }], next_after: null } });
const comparison = () => ({ ...envelope('mesh.fleet-saved-progress-comparison/v1'), progress: {
  base: selection.starting, target: selection.version, order: 'object-id', after: null, selected: null, total: 0,
  changes: [], next_after: null, approval_authority: false } });
test('progress pages preserve exact operation order and distinguish latest acknowledgment from selection', () => {
  const result = savedProgressPage(JSON.stringify(page()), selection);
  assert.equal(result.latest, hash(9)); assert.equal(result.versions[1].version, hash(5));
  for (const mutate of [v => v.lane = `lane-${hash(8)}`, v => v.objective = `fleet-${hash(8)}`, v => v.handoff_authority = true,
    v => v.approval_authority = true, v => v.revision = -1, v => v.starting_version = 'bad',
    v => v.progress.versions[1].version = hash(4), v => v.progress.versions[1].ordinal = 1,
    v => v.progress.versions[0].ordinal = 0, v => v.progress.total = 3,
    v => v.progress.next_after = hash(5), v => v.progress.after = hash(4)]) {
    const value = page(); mutate(value); assert.throws(() => savedProgressPage(value, selection));
  }
});
test('progress pagination requires bounded distinct operations and an exact continuation', () => {
  const value = page(); Object.assign(value.progress, { total: 51,
    versions: Array.from({ length: 50 }, (_, n) => ({ version: hash(n + 10), ordinal: n + 1 })), next_after: hash(59) });
  assert.equal(savedProgressPage(value, selection).nextAfter, hash(59));
  Object.assign(value.progress, { after: hash(59), versions: [{ version: hash(60), ordinal: 51 }], next_after: null });
  assert.equal(savedProgressPage(value, selection, hash(59)).versions[0].ordinal, 51);
  value.progress.versions[0].version = hash(59); assert.throws(() => savedProgressPage(value, selection, hash(59)));
  value.progress.versions = Array.from({ length: 51 }, (_, n) => ({ version: hash(n + 10), ordinal: n + 1 }));
  assert.throws(() => savedProgressPage(value, selection, hash(59)));
});
test('progress comparison binds exact source and starting version without a fabricated review selector', () => {
  const result = savedProgressComparison(comparison(), selection);
  assert.equal(result.target, hash(5)); assert.equal(result.total, 0);
  for (const mutate of [v => v.source_version = hash(8), v => v.starting_version = hash(8), v => v.progress.base = hash(8),
    v => v.progress.target = hash(9), v => v.progress.approval_authority = true, v => v.handoff_authority = true,
    v => v.progress.total = 1, v => v.lane = `lane-${hash(8)}`, v => v.schema = 'mesh.fleet-starting-comparison/v1']) {
    const value = comparison(); mutate(value); assert.throws(() => savedProgressComparison(value, selection));
  }
  assert.throws(() => savedProgressComparison(comparison(), selection, null, null, { ...result, total: 1 }));
});
test('selected progress content verifies object identity, paths, bytes and text safety', () => {
  const value = comparison(), object = '1'.repeat(32);
  const side = { path: 'note.txt', kind: 'file', digest: hash(7), bytes: 4, executable: false, content_state: 'text', text: 'new\n' };
  Object.assign(value.progress, { selected: object, total: 1, changes: [{ object, effect: 'added', before: null, after: side }] });
  assert.equal(savedProgressComparison(value, selection, null, object).changes[0].after.text, 'new\n');
  for (const mutate of [v => v.progress.changes[0].object = '2'.repeat(32), v => v.progress.changes[0].after.path = '../secret',
    v => v.progress.changes[0].after.bytes = 999, v => v.progress.changes[0].after.text = '\u202eX',
    v => v.progress.changes[0].after.content_state = 'not-requested']) {
    const altered = structuredClone(value); mutate(altered); assert.throws(() => savedProgressComparison(altered, selection, null, object));
  }
});
