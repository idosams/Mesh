// Complete nextest inventory proof for the three macOS workers; no test execution is skipped here.
import { readFileSync, statSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
const fail = message => { throw new Error(message); };
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
function inventory(value) {
  if (!object(value) || !object(value['rust-suites']) || !Number.isSafeInteger(value['test-count'])) fail('Invalid nextest inventory');
  const tests = new Map();
  const suites = new Map();
  for (const [id, suite] of Object.entries(value['rust-suites'])) {
    if (!object(suite) || suite.status !== 'listed' || suite['binary-id'] !== id || !object(suite.testcases)) fail('Incomplete nextest suite');
    const identity = ['binary-id', 'package-name', 'kind', 'build-platform', 'binary-path', 'cwd'].map(key => {
      if (typeof suite[key] !== 'string' || !suite[key]) fail('Missing nextest suite identity');
      return suite[key];
    });
    suites.set(id, JSON.stringify(identity));
    for (const [name, test] of Object.entries(suite.testcases)) {
      if (!name || !object(test) || typeof test.ignored !== 'boolean' || !object(test['filter-match'])) fail('Invalid nextest test');
      const match = test['filter-match'];
      if (match.status !== 'matches' && match.status !== 'mismatch') fail('Unknown nextest match result');
      if (match.status === 'matches' && Object.hasOwn(match, 'reason')) fail('Contradictory nextest match');
      if (match.status === 'mismatch' && !['ignored', 'partition'].includes(match.reason)) fail('Unexpected nextest filter');
      tests.set(JSON.stringify([id, name]), { ignored: test.ignored, selected: match.status === 'matches', reason: match.reason });
    }
  }
  if (tests.size !== value['test-count'] || !tests.size) fail('Incomplete nextest test count');
  return { tests, suites };
}
export function verifyPartitionCoverage(fullValue, partitionValues) {
  if (!Array.isArray(partitionValues) || partitionValues.length !== 3) fail('Exactly three macOS partitions are required');
  const full = inventory(fullValue);
  const expected = new Set();
  for (const [id, test] of full.tests) {
    if (test.ignored ? test.selected || test.reason !== 'ignored' : !test.selected) fail('Full inventory filters changed');
    if (!test.ignored) expected.add(id);
  }
  if (!expected.size) fail('Empty macOS execution inventory');
  const seen = new Set();
  const counts = partitionValues.map(value => {
    const partition = inventory(value);
    if (partition.suites.size !== full.suites.size || partition.tests.size !== full.tests.size) fail('Partition inventory is incomplete');
    for (const [id, identity] of full.suites) if (partition.suites.get(id) !== identity) fail('Partition binary identity differs');
    let count = 0;
    for (const [id, original] of full.tests) {
      const test = partition.tests.get(id);
      if (!test || test.ignored !== original.ignored) fail('Partition test identity or ignored state differs');
      if (test.ignored) {
        if (test.selected || test.reason !== 'ignored') fail('Ignored test selection changed');
      } else if (test.selected) {
        if (seen.has(id)) fail('A test is selected by multiple partitions');
        seen.add(id); count++;
      } else if (test.reason !== 'partition') fail('Partition introduced another filter');
    }
    if (!count) fail('Empty macOS partition');
    return count;
  });
  if (seen.size !== expected.size || [...expected].some(id => !seen.has(id))) fail('Partitions omit required tests');
  return { total: full.tests.size, selected: expected.size, ignored: full.tests.size - expected.size, partitions: counts };
}
export function requireSuccessfulPartitions(needs) {
  if (!object(needs) || Object.keys(needs).length !== 1 || !object(needs['test-macos-native']) || needs['test-macos-native'].result !== 'success') fail('Every macOS partition must succeed');
}
function readInventory(path) {
  const stat = statSync(path);
  if (!stat.isFile() || stat.size > 16 * 1024 * 1024) fail('Nextest inventory must be a bounded regular file');
  return JSON.parse(readFileSync(path, 'utf8'));
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const args = process.argv.slice(2);
    if (args.length === 1 && args[0] === '--result') {
      requireSuccessfulPartitions(JSON.parse(process.env.MESH_MACOS_PARTITION_RESULTS ?? 'null'));
      console.log('All macOS partitions succeeded, including daemon and native-renderer checks.');
    } else {
      if (args.length !== 4) fail('Provide the full inventory and all three partition inventories');
      const [full, ...partitions] = args.map(readInventory);
      console.log(`macOS complete test coverage: ${JSON.stringify(verifyPartitionCoverage(full, partitions))}`);
    }
  } catch (error) {
    console.error(`macOS test coverage refused: ${error.message}`);
    process.exitCode = 1;
  }
}
