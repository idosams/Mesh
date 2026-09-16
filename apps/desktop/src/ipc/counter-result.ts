// Validate the one result shape whose integers cannot safely use JavaScript numbers.
//
// `performance.counters` is a surface-v4 contract, not an arbitrary JSON document. The daemon
// keeps every count as `u64` and publishes canonical decimal strings because JavaScript rounds
// integers above 2^53 - 1. Accepting a generic object here would make the transport's safe-number
// check irrelevant: a stale or faulty peer could omit the encoding declaration, change a counter
// back to a number, or publish internally inconsistent catalogue facts and the UI would display it
// as measured truth.

import { WireError, type WireObject, type WireValue } from './protocol.ts';

const MAX_U64 = 18_446_744_073_709_551_615n;
const DECIMAL_U64 = /^(?:0|[1-9][0-9]*)$/u;
export const COUNTER_CONDITIONS =
  'a load-dependent reading is wall-clock nanoseconds and moves with machine load; the conditions a timing number is taken under are benchmarks/runners/README.md, `Getting a trustworthy number`';
export const COUNTER_INTEGER_ENCODING = 'decimal-u64';
export const COUNTER_CATALOGUE_COUNT = 80;
export const COUNTER_ATOMIC_WRITES_PER_OBSERVATION = 5n;
export const COUNTER_ATOMIC_WRITES_PER_GROUP = 2n;
export const COUNTER_ALLOCATIONS_PER_OBSERVATION = 0n;
// Runtime order mirrors the daemon's stable family/catalogue order. The static IPC contract lists
// the same identities alphabetically, so its agreement test sorts a copy instead of weakening the
// live result boundary to an unordered set.
export const COUNTER_CATALOGUE_KEYS = [
  'local_filesystem.lookup.ops',
  'local_filesystem.lookup.ns',
  'local_filesystem.open.ops',
  'local_filesystem.open.ns',
  'local_filesystem.create.ops',
  'local_filesystem.create.ns',
  'local_filesystem.rename.ops',
  'local_filesystem.rename.ns',
  'local_filesystem.directory_enumeration.ops',
  'local_filesystem.directory_enumeration.entries',
  'local_filesystem.directory_enumeration.ns',
  'local_filesystem.random_read.ops',
  'local_filesystem.random_read.bytes',
  'local_filesystem.random_read.ns',
  'local_filesystem.sequential_read.ops',
  'local_filesystem.sequential_read.bytes',
  'local_filesystem.sequential_read.ns',
  'local_filesystem.random_write.ops',
  'local_filesystem.random_write.bytes',
  'local_filesystem.random_write.ns',
  'local_filesystem.sequential_write.ops',
  'local_filesystem.sequential_write.bytes',
  'local_filesystem.sequential_write.ns',
  'local_filesystem.build_test.runs',
  'local_filesystem.build_test.ns',
  'local_filesystem.build_test.baseline_ns',
  'local_filesystem.cpu.samples',
  'local_filesystem.cpu.busy_ns',
  'local_filesystem.memory.samples',
  'local_filesystem.memory.resident_bytes',
  'local_filesystem.context_switches.samples',
  'local_filesystem.context_switches.count',
  'workspace_operations.actor_view_creation.ops',
  'workspace_operations.actor_view_creation.ns',
  'workspace_operations.checkpoint_creation.ops',
  'workspace_operations.checkpoint_creation.bytes',
  'workspace_operations.checkpoint_creation.ns',
  'workspace_operations.review_bundle_calculation.ops',
  'workspace_operations.review_bundle_calculation.ns',
  'workspace_operations.shadow_view_opening.ops',
  'workspace_operations.shadow_view_opening.ns',
  'workspace_operations.selective_approval.ops',
  'workspace_operations.selective_approval.ns',
  'workspace_operations.canonical_publication.ops',
  'workspace_operations.canonical_publication.ns',
  'workspace_operations.crash_recovery.ops',
  'workspace_operations.crash_recovery.records',
  'workspace_operations.crash_recovery.ns',
  'workspace_operations.index_reconstruction.ops',
  'workspace_operations.index_reconstruction.rows',
  'workspace_operations.index_reconstruction.ns',
  'synchronization.remote_metadata_visibility.ops',
  'synchronization.remote_metadata_visibility.ns',
  'synchronization.remote_small_file_availability.ops',
  'synchronization.remote_small_file_availability.ns',
  'synchronization.throughput.transfers',
  'synchronization.throughput.bytes',
  'synchronization.throughput.ns',
  'synchronization.transferred.messages',
  'synchronization.transferred.bytes',
  'synchronization.reconnect_convergence.ops',
  'synchronization.reconnect_convergence.ns',
  'synchronization.relay.cpu_samples',
  'synchronization.relay.cpu_busy_ns',
  'synchronization.relay.storage_samples',
  'synchronization.relay.storage_bytes',
  'synchronization.duplicate_operations.count',
  'synchronization.duplicate_operations.bytes',
  'context.harness.assemblies',
  'context.harness.fixed_bytes',
  'context.tokens.unique',
  'context.tokens.repeated',
  'context.stale_input.checks',
  'context.stale_input.detected',
  'context.cache.lookups',
  'context.cache.hits',
  'context.task.completions',
  'context.task.ns',
  'context.output.accepted',
  'context.output.rejected',
] as const;

export const COUNTER_NANOSECOND_KEYS = [
  'context.task.ns',
  'local_filesystem.build_test.baseline_ns',
  'local_filesystem.build_test.ns',
  'local_filesystem.cpu.busy_ns',
  'local_filesystem.create.ns',
  'local_filesystem.directory_enumeration.ns',
  'local_filesystem.lookup.ns',
  'local_filesystem.open.ns',
  'local_filesystem.random_read.ns',
  'local_filesystem.random_write.ns',
  'local_filesystem.rename.ns',
  'local_filesystem.sequential_read.ns',
  'local_filesystem.sequential_write.ns',
  'synchronization.reconnect_convergence.ns',
  'synchronization.relay.cpu_busy_ns',
  'synchronization.remote_metadata_visibility.ns',
  'synchronization.remote_small_file_availability.ns',
  'synchronization.throughput.ns',
  'workspace_operations.actor_view_creation.ns',
  'workspace_operations.canonical_publication.ns',
  'workspace_operations.checkpoint_creation.ns',
  'workspace_operations.crash_recovery.ns',
  'workspace_operations.index_reconstruction.ns',
  'workspace_operations.review_bundle_calculation.ns',
  'workspace_operations.selective_approval.ns',
  'workspace_operations.shadow_view_opening.ns',
] as const;

export const COUNTER_BYTE_KEYS = [
  'context.harness.fixed_bytes',
  'local_filesystem.memory.resident_bytes',
  'local_filesystem.random_read.bytes',
  'local_filesystem.random_write.bytes',
  'local_filesystem.sequential_read.bytes',
  'local_filesystem.sequential_write.bytes',
  'synchronization.duplicate_operations.bytes',
  'synchronization.relay.storage_bytes',
  'synchronization.throughput.bytes',
  'synchronization.transferred.bytes',
  'workspace_operations.checkpoint_creation.bytes',
] as const;

const NANOSECOND_COUNTER_KEYS = new Set<string>(COUNTER_NANOSECOND_KEYS);
const BYTE_COUNTER_KEYS = new Set<string>(COUNTER_BYTE_KEYS);

export type CounterCatalogueUnit = 'events' | 'bytes' | 'nanoseconds';
export type CounterCatalogueEntry = {
  readonly key: (typeof COUNTER_CATALOGUE_KEYS)[number];
  readonly unit: CounterCatalogueUnit;
};

const unitForPublishedKey = (
  key: (typeof COUNTER_CATALOGUE_KEYS)[number],
): CounterCatalogueUnit => {
  if (NANOSECOND_COUNTER_KEYS.has(key)) return 'nanoseconds';
  if (BYTE_COUNTER_KEYS.has(key)) return 'bytes';
  return 'events';
};

export const COUNTER_CATALOGUE: readonly CounterCatalogueEntry[] = COUNTER_CATALOGUE_KEYS.map(
  (key) => ({ key, unit: unitForPublishedKey(key) }),
);
const COUNTER_UNITS = new Map(COUNTER_CATALOGUE.map(({ key, unit }) => [key, unit]));

export const COUNTER_WIRED_KEYS = [
  'local_filesystem.sequential_read.bytes',
  'local_filesystem.sequential_read.ops',
  'workspace_operations.checkpoint_creation.bytes',
  'workspace_operations.checkpoint_creation.ops',
  'workspace_operations.crash_recovery.ns',
  'workspace_operations.crash_recovery.ops',
  'workspace_operations.crash_recovery.records',
  'workspace_operations.index_reconstruction.ns',
  'workspace_operations.index_reconstruction.ops',
  'workspace_operations.index_reconstruction.rows',
] as const;

const NO_CONTEXT_PATH =
  'this build assembles no agent context, so nothing here is ever observed in it';
const NO_FILESYSTEM_PATH =
  'the filesystem operation exists, but no operation-level metric producer is wired in this build';
const NO_SEPARATE_READ_TIMER =
  'the read is not timed apart from the fold that follows it; the whole open is recorded on workspace_operations.index_reconstruction.ns';
const NO_BUILD_TEST_RUNS = 'no build or test runs inside a Mesh workspace are measured here';
const NO_PROCESS_ACCOUNTING = 'no process accounting is read on any platform in this build';
const NO_TRANSPORT =
  'this build opens no network transport, so nothing here is ever observed in it';
const NO_WORKSPACE_METRIC_PRODUCER =
  'the operation exists, but no workspace-operation metric producer is wired in this build';

export const COUNTER_NOT_YET_REASON_GROUPS = [
  {
    reason: NO_CONTEXT_PATH,
    keys: COUNTER_CATALOGUE_KEYS.filter((key) => key.startsWith('context.')).sort(),
  },
  {
    reason: NO_FILESYSTEM_PATH,
    keys: [
      'local_filesystem.create.ns',
      'local_filesystem.create.ops',
      'local_filesystem.directory_enumeration.entries',
      'local_filesystem.directory_enumeration.ns',
      'local_filesystem.directory_enumeration.ops',
      'local_filesystem.lookup.ns',
      'local_filesystem.lookup.ops',
      'local_filesystem.open.ns',
      'local_filesystem.open.ops',
      'local_filesystem.random_read.bytes',
      'local_filesystem.random_read.ns',
      'local_filesystem.random_read.ops',
      'local_filesystem.random_write.bytes',
      'local_filesystem.random_write.ns',
      'local_filesystem.random_write.ops',
      'local_filesystem.rename.ns',
      'local_filesystem.rename.ops',
      'local_filesystem.sequential_write.bytes',
      'local_filesystem.sequential_write.ns',
      'local_filesystem.sequential_write.ops',
    ],
  },
  {
    reason: NO_SEPARATE_READ_TIMER,
    keys: ['local_filesystem.sequential_read.ns'],
  },
  {
    reason: NO_BUILD_TEST_RUNS,
    keys: [
      'local_filesystem.build_test.baseline_ns',
      'local_filesystem.build_test.ns',
      'local_filesystem.build_test.runs',
    ],
  },
  {
    reason: NO_PROCESS_ACCOUNTING,
    keys: [
      'local_filesystem.context_switches.count',
      'local_filesystem.context_switches.samples',
      'local_filesystem.cpu.busy_ns',
      'local_filesystem.cpu.samples',
      'local_filesystem.memory.resident_bytes',
      'local_filesystem.memory.samples',
    ],
  },
  {
    reason: NO_TRANSPORT,
    keys: COUNTER_CATALOGUE_KEYS.filter((key) => key.startsWith('synchronization.')).sort(),
  },
  {
    reason: NO_WORKSPACE_METRIC_PRODUCER,
    keys: [
      'workspace_operations.actor_view_creation.ns',
      'workspace_operations.actor_view_creation.ops',
      'workspace_operations.canonical_publication.ns',
      'workspace_operations.canonical_publication.ops',
      'workspace_operations.checkpoint_creation.ns',
      'workspace_operations.review_bundle_calculation.ns',
      'workspace_operations.review_bundle_calculation.ops',
      'workspace_operations.selective_approval.ns',
      'workspace_operations.selective_approval.ops',
      'workspace_operations.shadow_view_opening.ns',
      'workspace_operations.shadow_view_opening.ops',
    ],
  },
] as const;

const COUNTER_NOT_YET_REASONS = new Map(
  COUNTER_NOT_YET_REASON_GROUPS.flatMap(({ reason, keys }) =>
    keys.map((key) => [key, reason] as const),
  ),
);
const COUNTER_WIRED_KEY_SET = new Set<string>(COUNTER_WIRED_KEYS);
export const COUNTER_NOT_YET_CATALOGUE = COUNTER_CATALOGUE_KEYS
  .filter((key) => !COUNTER_WIRED_KEY_SET.has(key))
  .map((key) => ({ key, reason: COUNTER_NOT_YET_REASONS.get(key)! }));

const TOP_LEVEL_KEYS = ['conditions', 'integer_encoding', 'collection', 'counters', 'not_yet'] as const;
const COLLECTION_KEYS = [
  'observations_recorded',
  'observations_summed',
  'concurrent_observations',
  'writers_in_flight_before_readings',
  'writers_in_flight_after_readings',
  'snapshot_consistent',
  'snapshots_taken',
  'state_bytes',
  'counters',
  'atomic_writes_per_observation',
  'atomic_writes_per_group',
  'allocations_per_observation',
] as const;
const READING_KEYS = [
  'key',
  'family',
  'unit',
  'determinism',
  'band',
  'observations',
  'total',
  'produced',
] as const;
const NOT_YET_KEYS = ['key', 'reason'] as const;

const fail = (message: string): never => {
  throw new WireError('protocol-mismatch', `The performance counter result is invalid: ${message}.`);
};

const object = (value: WireValue | undefined, at: string): WireObject => {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return fail(`${at} must be an object`);
  }
  return value;
};

const exactKeys = (value: WireObject, expected: readonly string[], at: string): void => {
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    fail(`${at} must contain exactly ${expected.join(', ')}`);
  }
};

const text = (value: WireValue | undefined, at: string): string => {
  if (typeof value !== 'string' || value.length === 0) return fail(`${at} must be non-empty text`);
  return value;
};

const oneOf = (value: WireValue | undefined, accepted: readonly string[], at: string): string => {
  const word = text(value, at);
  if (!accepted.includes(word)) return fail(`${at} has unknown value ${word}`);
  return word;
};

const flag = (value: WireValue | undefined, at: string): boolean => {
  if (typeof value !== 'boolean') return fail(`${at} must be a boolean`);
  return value;
};

const decimalU64 = (value: WireValue | undefined, at: string): bigint => {
  if (typeof value !== 'string' || !DECIMAL_U64.test(value)) {
    return fail(`${at} must be a canonical decimal-u64 string`);
  }
  const parsed = BigInt(value);
  if (parsed > MAX_U64) return fail(`${at} exceeds u64`);
  return parsed;
};

const array = (value: WireValue | undefined, at: string): readonly WireValue[] => {
  if (!Array.isArray(value)) return fail(`${at} must be an array`);
  return value;
};

/**
 * Prove one `performance.counters` result has the exact v4 shape and internal relationships.
 *
 * This returns nothing deliberately. The client resolves the original wire object only after the
 * proof succeeds, preserving the daemon's key order and bytes rather than normalising bad input.
 */
export const validateCounterResult = (value: WireObject): void => {
  exactKeys(value, TOP_LEVEL_KEYS, 'the result');
  if (value['conditions'] !== COUNTER_CONDITIONS) {
    fail('conditions must name the published measurement conditions');
  }
  if (value['integer_encoding'] !== COUNTER_INTEGER_ENCODING) {
    fail('integer_encoding must be decimal-u64');
  }

  const collection = object(value['collection'], 'collection');
  exactKeys(collection, COLLECTION_KEYS, 'collection');
  const observationsRecorded = decimalU64(
    collection['observations_recorded'],
    'collection.observations_recorded',
  );
  const observationsSummed = decimalU64(
    collection['observations_summed'],
    'collection.observations_summed',
  );
  const concurrentObservations = decimalU64(
    collection['concurrent_observations'],
    'collection.concurrent_observations',
  );
  const writersBefore = decimalU64(
    collection['writers_in_flight_before_readings'],
    'collection.writers_in_flight_before_readings',
  );
  const writersAfter = decimalU64(
    collection['writers_in_flight_after_readings'],
    'collection.writers_in_flight_after_readings',
  );
  const snapshotConsistent = flag(
    collection['snapshot_consistent'],
    'collection.snapshot_consistent',
  );
  const snapshotsTaken = decimalU64(collection['snapshots_taken'], 'collection.snapshots_taken');
  const stateBytes = decimalU64(collection['state_bytes'], 'collection.state_bytes');
  decimalU64(collection['counters'], 'collection.counters');
  const atomicWrites = decimalU64(
    collection['atomic_writes_per_observation'],
    'collection.atomic_writes_per_observation',
  );
  const groupAtomicWrites = decimalU64(
    collection['atomic_writes_per_group'],
    'collection.atomic_writes_per_group',
  );
  const allocations = decimalU64(
    collection['allocations_per_observation'],
    'collection.allocations_per_observation',
  );
  if (snapshotsTaken === 0n) fail('collection.snapshots_taken must include this snapshot');
  if (stateBytes === 0n) fail('collection.state_bytes must include the live registry');
  if (atomicWrites !== COUNTER_ATOMIC_WRITES_PER_OBSERVATION) {
    fail('collection.atomic_writes_per_observation does not match the published implementation');
  }
  if (groupAtomicWrites !== COUNTER_ATOMIC_WRITES_PER_GROUP) {
    fail('collection.atomic_writes_per_group does not match the published implementation');
  }
  if (allocations !== COUNTER_ALLOCATIONS_PER_OBSERVATION) {
    fail('collection.allocations_per_observation does not match the published implementation');
  }
  if (concurrentObservations > observationsRecorded) {
    fail('collection.concurrent_observations exceeds the recorded observation tally');
  }

  const readings = array(value['counters'], 'counters');
  const readingKeys = new Set<string>();
  const notProduced = new Set<string>();
  let summedReadings = 0n;
  let readingSaturated = false;
  for (const [index, entry] of readings.entries()) {
    const reading = object(entry, `counters[${index}]`);
    exactKeys(reading, READING_KEYS, `counters[${index}]`);
    const key = text(reading['key'], `counters[${index}].key`);
    if (key !== COUNTER_CATALOGUE_KEYS[index]) {
      fail(`counters[${index}].key is outside the canonical catalogue order`);
    }
    if (readingKeys.has(key)) fail(`counter key ${key} is duplicated`);
    readingKeys.add(key);

    const family = oneOf(
      reading['family'],
      ['local_filesystem', 'workspace_operations', 'synchronization', 'context'],
      `counter ${key}.family`,
    );
    if (!key.startsWith(`${family}.`)) fail(`counter ${key} is outside its named family`);
    const unit = oneOf(reading['unit'], ['events', 'bytes', 'nanoseconds'], `counter ${key}.unit`);
    const expectedUnit = COUNTER_UNITS.get(key);
    if (expectedUnit === undefined || unit !== expectedUnit) {
      fail(`counter ${key} does not use its published unit`);
    }
    const determinism = oneOf(
      reading['determinism'],
      ['deterministic', 'load-dependent'],
      `counter ${key}.determinism`,
    );
    const band = oneOf(reading['band'], ['exact', 'enclosed'], `counter ${key}.band`);
    const expectedDeterminism = unit === 'nanoseconds' ? 'load-dependent' : 'deterministic';
    const expectedBand = unit === 'nanoseconds' ? 'enclosed' : 'exact';
    if (determinism !== expectedDeterminism || band !== expectedBand) {
      fail(`counter ${key} has inconsistent unit, determinism, and band`);
    }
    const observations = decimalU64(reading['observations'], `counter ${key}.observations`);
    const total = decimalU64(reading['total'], `counter ${key}.total`);
    summedReadings = summedReadings + observations > MAX_U64
      ? MAX_U64
      : summedReadings + observations;
    readingSaturated ||= observations === MAX_U64 || total === MAX_U64;
    const produced = flag(reading['produced'], `counter ${key}.produced`);
    const expectedProduced = COUNTER_WIRED_KEY_SET.has(key);
    if (produced !== expectedProduced) {
      fail(`counter ${key}.produced does not match the published producer partition`);
    }
    if (!produced) {
      if (observations !== 0n || total !== 0n) {
        fail(`counter ${key} has measurements without a producer`);
      }
      notProduced.add(key);
    }
  }

  if (decimalU64(collection['counters'], 'collection.counters') !== BigInt(readings.length)) {
    fail('collection.counters does not equal the number of readings');
  }
  if (readings.length !== COUNTER_CATALOGUE_COUNT) {
    fail(`the catalogue must contain exactly ${COUNTER_CATALOGUE_COUNT} readings`);
  }
  if (observationsSummed !== summedReadings) {
    fail('collection.observations_summed does not equal the saturating reading tally');
  }
  const expectedConsistency =
    concurrentObservations === 0n &&
    writersBefore === 0n &&
    writersAfter === 0n &&
    observationsRecorded === observationsSummed &&
    observationsRecorded !== MAX_U64 &&
    observationsSummed !== MAX_U64 &&
    !readingSaturated;
  if (snapshotConsistent !== expectedConsistency) {
    fail('collection.snapshot_consistent does not match the published readings');
  }

  const missing = array(value['not_yet'], 'not_yet');
  const missingKeys = new Set<string>();
  for (const [index, entry] of missing.entries()) {
    const gap = object(entry, `not_yet[${index}]`);
    exactKeys(gap, NOT_YET_KEYS, `not_yet[${index}]`);
    const key = text(gap['key'], `not_yet[${index}].key`);
    const expectedKey = COUNTER_NOT_YET_CATALOGUE[index]?.key;
    if (key !== expectedKey) fail(`not_yet[${index}].key is outside the canonical catalogue order`);
    const reason = text(gap['reason'], `not_yet[${index}].reason`);
    if (missingKeys.has(key)) fail(`not_yet key ${key} is duplicated`);
    if (!readingKeys.has(key)) fail(`not_yet key ${key} has no counter reading`);
    const expectedReason = COUNTER_NOT_YET_REASONS.get(key);
    if (expectedReason === undefined || reason !== expectedReason) {
      fail(`not_yet key ${key} does not use its published reason`);
    }
    missingKeys.add(key);
  }
  if (
    missingKeys.size !== notProduced.size ||
    [...notProduced].some((key) => !missingKeys.has(key))
  ) {
    fail('not_yet does not exactly name the counters without producers');
  }
};
