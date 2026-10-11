import test from 'node:test';
import assert from 'node:assert/strict';
import { verifyPartitionCoverage, requireSuccessfulPartitions } from './macos-test-coverage.mjs';
function fixture() {
  const full = { 'test-count': 7, 'rust-suites': { native: { status: 'listed', 'binary-id': 'native', 'package-name': 'mesh', kind: 'lib', 'build-platform': 'target', 'binary-path': '/fixture/native', cwd: '/fixture', testcases: {} } } };
  for (let i = 0; i < 7; i++) full['rust-suites'].native.testcases[`case-${i}`] = { ignored: i === 6, 'filter-match': i === 6 ? { status: 'mismatch', reason: 'ignored' } : { status: 'matches' } };
  const parts = [0, 1, 2].map(shard => {
    const value = structuredClone(full);
    for (let i = 0; i < 6; i++) if (i % 3 !== shard) value['rust-suites'].native.testcases[`case-${i}`]['filter-match'] = { status: 'mismatch', reason: 'partition' };
    return value;
  });
  return { full, parts };
}
const testcase = (value, number) => value['rust-suites'].native.testcases[`case-${number}`];
test('all required tests execute once and ignored tests remain unchanged', () => {
  const { full, parts } = fixture();
  assert.deepEqual(verifyPartitionCoverage(full, parts), { total: 7, selected: 6, ignored: 1, partitions: [2, 2, 2] });
});
for (const [name, change] of [
  ['missing partition', f => f.parts.pop()],
  ['extra partition', f => f.parts.push(structuredClone(f.parts[0]))],
  ['omitted selection', f => testcase(f.parts[0], 0)['filter-match'] = { status: 'mismatch', reason: 'partition' }],
  ['duplicate selection', f => testcase(f.parts[1], 0)['filter-match'] = { status: 'matches' }],
  ['missing binary', f => delete f.parts[0]['rust-suites'].native],
  ['missing testcase', f => delete f.parts[0]['rust-suites'].native.testcases['case-0']],
  ['foreign testcase', f => { f.parts[0]['rust-suites'].native.testcases.foreign = testcase(f.parts[0], 0); delete f.parts[0]['rust-suites'].native.testcases['case-0']; }],
  ['wrong count', f => f.full['test-count']++],
  ['unlisted suite', f => f.parts[0]['rust-suites'].native.status = 'skipped'],
  ['wrong binary', f => f.parts[0]['rust-suites'].native['binary-path'] = '/foreign'],
  ['wrong ignored state', f => testcase(f.parts[0], 0).ignored = true],
  ['ignored test selected', f => testcase(f.parts[0], 6)['filter-match'] = { status: 'matches' }],
  ['new full filter', f => testcase(f.full, 0)['filter-match'] = { status: 'mismatch', reason: 'partition' }],
  ['unknown filter', f => testcase(f.parts[0], 0)['filter-match'] = { status: 'mismatch', reason: 'expression' }],
  ['unknown result', f => testcase(f.parts[0], 0)['filter-match'] = { status: 'unknown' }],
  ['contradictory match', f => testcase(f.parts[0], 0)['filter-match'] = { status: 'matches', reason: 'partition' }],
]) test(`refuse ${name}`, () => { const f = fixture(); change(f); assert.throws(() => verifyPartitionCoverage(f.full, f.parts)); });
test('aggregate accepts only successful matrix completion', () => {
  requireSuccessfulPartitions({ 'test-macos-native': { result: 'success', outputs: {} } });
});
for (const result of ['failure', 'cancelled', 'skipped', '', 'SUCCESS', null]) test(`aggregate refuses ${JSON.stringify(result)}`, () => {
  assert.throws(() => requireSuccessfulPartitions({ 'test-macos-native': { result } }));
});
test('aggregate refuses missing or unrelated dependency results', () => {
  for (const value of [null, {}, [], { unrelated: { result: 'success' } }, { 'test-macos-native': { result: 'success' }, unrelated: { result: 'success' } }]) assert.throws(() => requireSuccessfulPartitions(value));
});
