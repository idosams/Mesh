import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
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

test('Linux aggregate accepts its complete successful worker group', () => {
  requireSuccessfulPartitions({ 'test-linux-native': { result: 'success', outputs: {} } }, 'linux');
});
for (const result of ['failure', 'cancelled', 'skipped', '', 'SUCCESS', null]) test(`Linux aggregate refuses ${JSON.stringify(result)}`, () => {
  assert.throws(() => requireSuccessfulPartitions({ 'test-linux-native': { result } }, 'linux'));
});
test('platform aggregates cannot substitute or omit worker groups', () => {
  for (const platform of ['linux', 'macos']) {
    const own = `test-${platform}-native`;
    const foreign = `test-${platform === 'linux' ? 'macos' : 'linux'}-native`;
    for (const value of [null, {}, [], { [foreign]: { result: 'success' } }, { [own]: { result: 'success' }, [foreign]: { result: 'success' } }]) {
      assert.throws(() => requireSuccessfulPartitions(value, platform));
    }
  }
  assert.throws(() => requireSuccessfulPartitions({ 'test-macos-native': { result: 'success' } }, 'windows'));
});
const cli = (args, environment = {}) => spawnSync(process.execPath, [new URL('./macos-test-coverage.mjs', import.meta.url).pathname, ...args], {
  encoding: 'utf8', env: { ...process.env, MESH_MACOS_PARTITION_RESULTS: '', MESH_LINUX_PARTITION_RESULTS: '', ...environment },
});
test('Linux CLI reads only the actual Linux aggregate results', () => {
  const results = JSON.stringify({ 'test-linux-native': { result: 'success' } });
  const accepted = cli(['--platform', 'linux', '--result'], { MESH_LINUX_PARTITION_RESULTS: results });
  assert.equal(accepted.status, 0, accepted.stderr);
  assert.match(accepted.stdout, /Linux/);
  assert.equal(cli(['--platform', 'linux', '--result'], { MESH_MACOS_PARTITION_RESULTS: results }).status, 1);
});
test('existing macOS aggregate CLI remains compatible', () => {
  const results = JSON.stringify({ 'test-macos-native': { result: 'success' } });
  const accepted = cli(['--result'], { MESH_MACOS_PARTITION_RESULTS: results });
  assert.equal(accepted.status, 0, accepted.stderr);
  assert.match(accepted.stdout, /macOS/);
  assert.equal(cli(['--result'], { MESH_LINUX_PARTITION_RESULTS: results }).status, 1);
});
for (const args of [ ['--platform'], ['--platform', 'windows', '--result'], ['--platform', 'linux'], ['--platform', 'linux', '--result', 'extra'], ['--result-linux'], ['--platform', 'macos', '--platform', 'linux', '--result'] ]) {
  test(`CLI refuses invalid selection ${JSON.stringify(args)}`, () => assert.equal(cli(args).status, 1));
}
