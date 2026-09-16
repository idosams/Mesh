import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import {
  COUNTER_CATALOGUE_COUNT,
  COUNTER_CATALOGUE,
  COUNTER_CATALOGUE_KEYS,
  COUNTER_CONDITIONS,
  COUNTER_NOT_YET_CATALOGUE,
  COUNTER_ATOMIC_WRITES_PER_OBSERVATION,
  COUNTER_ATOMIC_WRITES_PER_GROUP,
  COUNTER_WIRED_KEYS,
  validateCounterResult,
} from './counter-result.ts';
import type { WireObject } from './protocol.ts';

const VALID = {
  conditions: COUNTER_CONDITIONS,
  integer_encoding: 'decimal-u64',
  collection: {
    observations_recorded: '0',
    observations_summed: '0',
    concurrent_observations: '0',
    writers_in_flight_before_readings: '0',
    writers_in_flight_after_readings: '0',
    snapshot_consistent: true,
    snapshots_taken: '1',
    state_bytes: '48',
    counters: String(COUNTER_CATALOGUE_COUNT),
    atomic_writes_per_observation: String(COUNTER_ATOMIC_WRITES_PER_OBSERVATION),
    atomic_writes_per_group: String(COUNTER_ATOMIC_WRITES_PER_GROUP),
    allocations_per_observation: '0',
  },
  counters: COUNTER_CATALOGUE.map(({ key, unit }) => {
    return {
      key,
      family: key.slice(0, key.indexOf('.')),
      unit,
      determinism: unit === 'nanoseconds' ? 'load-dependent' : 'deterministic',
      band: unit === 'nanoseconds' ? 'enclosed' : 'exact',
      observations: '0',
      total: '0',
      produced: COUNTER_WIRED_KEYS.includes(key as (typeof COUNTER_WIRED_KEYS)[number]),
    };
  }),
  not_yet: COUNTER_NOT_YET_CATALOGUE,
} as const;

const clone = (): Record<string, unknown> =>
  JSON.parse(JSON.stringify(VALID)) as Record<string, unknown>;

const collection = (value: Record<string, unknown>): Record<string, unknown> =>
  value['collection'] as Record<string, unknown>;

const firstReading = (value: Record<string, unknown>): Record<string, unknown> =>
  (value['counters'] as Record<string, unknown>[])[0] as Record<string, unknown>;

describe('the desktop performance-counter result boundary', () => {
  it('admits an exact self-described decimal-u64 snapshot', () => {
    assert.doesNotThrow(() => validateCounterResult(clone() as WireObject));
  });

  for (const [name, mutate] of [
    ['unknown measurement conditions', (value: Record<string, unknown>) => {
      value['conditions'] = 'trust this reading without its published measurement conditions';
    }],
    ['missing integer encoding', (value: Record<string, unknown>) => delete value['integer_encoding']],
    ['numeric counter', (value: Record<string, unknown>) => { firstReading(value)['total'] = 1; }],
    ['noncanonical decimal', (value: Record<string, unknown>) => { firstReading(value)['total'] = '01'; }],
    ['u64 overflow', (value: Record<string, unknown>) => {
      firstReading(value)['total'] = '18446744073709551616';
    }],
    ['wrong catalogue cardinality', (value: Record<string, unknown>) => {
      collection(value)['counters'] = '2';
    }],
    ['zero live registry size', (value: Record<string, unknown>) => {
      collection(value)['state_bytes'] = '0';
    }],
    ['wrong atomic write cost', (value: Record<string, unknown>) => {
      collection(value)['atomic_writes_per_observation'] = '4';
    }],
    ['wrong group atomic write cost', (value: Record<string, unknown>) => {
      collection(value)['atomic_writes_per_group'] = '1';
    }],
    ['impossible concurrent tally', (value: Record<string, unknown>) => {
      collection(value)['concurrent_observations'] = '1';
      collection(value)['snapshot_consistent'] = false;
    }],
    ['truncated catalogue', (value: Record<string, unknown>) => {
      (value['counters'] as unknown[]).pop();
      (value['not_yet'] as unknown[]).pop();
      collection(value)['counters'] = String(COUNTER_CATALOGUE_COUNT - 1);
    }],
    ['reordered catalogue', (value: Record<string, unknown>) => {
      const readings = value['counters'] as Record<string, unknown>[];
      [readings[0], readings[1]] = [readings[1]!, readings[0]!];
    }],
    ['counter outside its named family', (value: Record<string, unknown>) => {
      firstReading(value)['key'] = 'workspace_operations.fixture_0.events';
    }],
    ['forged catalogue identity', (value: Record<string, unknown>) => {
      firstReading(value)['key'] = 'context.substituted_metric.events';
      (value['not_yet'] as Record<string, unknown>[])[0]!['key'] =
        'context.substituted_metric.events';
    }],
    ['dishonest consistency verdict', (value: Record<string, unknown>) => {
      collection(value)['observations_recorded'] = '1';
      collection(value)['snapshot_consistent'] = true;
    }],
    ['forged summed tally', (value: Record<string, unknown>) => {
      collection(value)['observations_summed'] = '1';
      collection(value)['snapshot_consistent'] = false;
    }],
    ['missing producer gap', (value: Record<string, unknown>) => { value['not_yet'] = []; }],
    ['reordered not-yet catalogue', (value: Record<string, unknown>) => {
      const gaps = value['not_yet'] as Record<string, unknown>[];
      [gaps[0], gaps[1]] = [gaps[1]!, gaps[0]!];
    }],
    ['co-mutated live producer hidden as not-yet', (value: Record<string, unknown>) => {
      const key = COUNTER_WIRED_KEYS[0];
      const reading = (value['counters'] as Record<string, unknown>[])
        .find((entry) => entry['key'] === key)!;
      reading['produced'] = false;
      (value['not_yet'] as Record<string, unknown>[]).push({
        key,
        reason: 'the producer was removed from this forged result',
      });
    }],
    ['co-mutated missing producer presented as live', (value: Record<string, unknown>) => {
      const gaps = value['not_yet'] as Record<string, unknown>[];
      const key = gaps[0]!['key'];
      const reading = (value['counters'] as Record<string, unknown>[])
        .find((entry) => entry['key'] === key)!;
      reading['produced'] = true;
      gaps.shift();
    }],
    ['not-yet counter carrying invented measurements', (value: Record<string, unknown>) => {
      const reading = (value['counters'] as Record<string, unknown>[])
        .find((entry) => entry['produced'] === false)!;
      reading['observations'] = '1';
      reading['total'] = '1';
      collection(value)['observations_recorded'] = '1';
      collection(value)['observations_summed'] = '1';
    }],
    ['substituted not-yet explanation', (value: Record<string, unknown>) => {
      (value['not_yet'] as Record<string, unknown>[])[0]!['reason'] =
        'the metric exists elsewhere and should be trusted';
    }],
    ['inconsistent timing metadata', (value: Record<string, unknown>) => {
      firstReading(value)['unit'] = 'nanoseconds';
    }],
    ['co-mutated units across exact catalogue identities', (value: Record<string, unknown>) => {
      const readings = value['counters'] as Record<string, unknown>[];
      const events = readings.find((reading) => reading['key'] === 'context.cache.hits')!;
      const timing = readings.find((reading) => reading['key'] === 'context.task.ns')!;
      for (const field of ['unit', 'determinism', 'band']) {
        [events[field], timing[field]] = [timing[field], events[field]];
      }
    }],
    ['unknown result field', (value: Record<string, unknown>) => { value['rounded_total'] = 0; }],
  ] as const) {
    it(`refuses ${name}`, () => {
      const value = clone();
      mutate(value);
      assert.throws(
        () => validateCounterResult(value as WireObject),
        /The performance counter result is invalid/u,
      );
    });
  }
});
