// The cross-language half of the IPC contract.
//
// `crates/mesh-daemon/ipc-contract.json` is ONE file, published by the daemon because the daemon
// owns the surface. This suite checks the TypeScript implementation against it;
// `crates/mesh-daemon/tests/ipc.rs` checks the Rust implementation against the same file with the
// same assertions. Neither side can move without the other going red.
//
// The file is read here at TEST time and never at run time — a shipped desktop application has no
// `crates/` directory beside it, and `methods.ts` is a literal for exactly that reason.
//
// If the file is missing this suite FAILS rather than skips. A skipped conformance test reads
// exactly like a passing one in a summary, and this repository has already shipped a check that
// nothing ran.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, it } from 'node:test';

import { METHODS } from './methods.ts';
import {
  COUNTER_ALLOCATIONS_PER_OBSERVATION,
  COUNTER_ATOMIC_WRITES_PER_OBSERVATION,
  COUNTER_ATOMIC_WRITES_PER_GROUP,
  COUNTER_BYTE_KEYS,
  COUNTER_CATALOGUE_COUNT,
  COUNTER_CATALOGUE_KEYS,
  COUNTER_CONDITIONS,
  COUNTER_INTEGER_ENCODING,
  COUNTER_NANOSECOND_KEYS,
  COUNTER_NOT_YET_REASON_GROUPS,
  COUNTER_WIRED_KEYS,
} from './counter-result.ts';
import {
  CLIENT_VERSIONS,
  MAX_JSON_DEPTH,
  MAX_LINE_BYTES,
  MAX_METHOD_BYTES,
  MAX_SESSION_BYTES,
  PROTOCOL,
  decodeClient,
  decodeDaemon,
  encodeClient,
  encodeDaemon,
} from './protocol.ts';

const HERE = dirname(fileURLToPath(import.meta.url));
const CONTRACT_PATH = join(HERE, '..', '..', '..', '..', 'crates', 'mesh-daemon', 'ipc-contract.json');

type PublishedMethod = { name: string; since: number; summary: string };
type PublishedVector = { name: string; direction: string; line: string };
type Contract = {
  protocol: string;
  surface_version: number;
  supported_versions: number[];
  framing: {
    kind: string;
    max_line_bytes: number;
    max_message_bytes: number;
    chunk_data_bytes: number;
    chunk_since: number;
  };
  transport: { kind: string; network_listener: boolean };
  limits: Record<string, number>;
  answers: {
    'performance.counters': {
      conditions: string;
      integer_encoding: string;
      catalogue_count: number;
      catalogue: string[];
      unit_partitions: {
        bytes: string[];
        nanoseconds: string[];
        events_are_catalogue_remainder: boolean;
      };
      producer_partitions: {
        wired: string[];
        not_yet_reason_groups: { reason: string; keys: string[] }[];
      };
      atomic_writes_per_observation: number;
      atomic_writes_per_group: number;
      allocations_per_observation: number;
      consistency: string;
    };
  };
  methods: PublishedMethod[];
  vectors: PublishedVector[];
};

const contract: Contract = JSON.parse(readFileSync(CONTRACT_PATH, 'utf8')) as Contract;

describe('the published IPC contract', () => {
  it('names the protocol and the versions this client speaks', () => {
    assert.equal(contract.protocol, PROTOCOL);
    assert.deepEqual(contract.supported_versions, [...CLIENT_VERSIONS]);
    assert.equal(contract.framing.max_line_bytes, MAX_LINE_BYTES);
    assert.equal(contract.framing.max_message_bytes, 16 * 1024 * 1024);
    assert.equal(contract.framing.chunk_data_bytes, 30000);
    assert.equal(contract.framing.chunk_since, 7);
    assert.equal(contract.framing.kind, 'newline-delimited-json');
    assert.equal(contract.limits['max_json_depth'], MAX_JSON_DEPTH);
    assert.equal(contract.limits['max_method_bytes'], MAX_METHOD_BYTES);
    assert.equal(contract.limits['max_session_bytes'], MAX_SESSION_BYTES);
  });

  it('declares the transport local, and this client agrees', () => {
    assert.equal(contract.transport.kind, 'unix-domain-socket');
    assert.equal(contract.transport.network_listener, false);
  });

  it('publishes exactly the method catalogue this client has', () => {
    assert.deepEqual(
      contract.methods.map((entry) => ({ name: entry.name, since: entry.since, summary: entry.summary })),
      METHODS.map((entry) => ({ name: entry.name, since: entry.since, summary: entry.summary })),
    );
  });

  it('binds the performance result semantics to the same constants as the client', () => {
    assert.equal(contract.answers['performance.counters'].conditions, COUNTER_CONDITIONS);
    assert.equal(
      contract.answers['performance.counters'].integer_encoding,
      COUNTER_INTEGER_ENCODING,
    );
    assert.equal(
      contract.answers['performance.counters'].catalogue_count,
      COUNTER_CATALOGUE_COUNT,
    );
    assert.deepEqual(
      contract.answers['performance.counters'].catalogue,
      [...COUNTER_CATALOGUE_KEYS].sort(),
    );
    assert.deepEqual(contract.answers['performance.counters'].unit_partitions, {
      bytes: [...COUNTER_BYTE_KEYS],
      nanoseconds: [...COUNTER_NANOSECOND_KEYS],
      events_are_catalogue_remainder: true,
    });
    assert.deepEqual(contract.answers['performance.counters'].producer_partitions, {
      wired: [...COUNTER_WIRED_KEYS],
      not_yet_reason_groups: COUNTER_NOT_YET_REASON_GROUPS.map(({ reason, keys }) => ({
        reason,
        keys: [...keys],
      })),
    });
    assert.equal(
      BigInt(contract.answers['performance.counters'].atomic_writes_per_observation),
      COUNTER_ATOMIC_WRITES_PER_OBSERVATION,
    );
    assert.equal(
      BigInt(contract.answers['performance.counters'].atomic_writes_per_group),
      COUNTER_ATOMIC_WRITES_PER_GROUP,
    );
    assert.equal(
      BigInt(contract.answers['performance.counters'].allocations_per_observation),
      COUNTER_ALLOCATIONS_PER_OBSERVATION,
    );
    assert.match(contract.answers['performance.counters'].consistency, /independently re-derives/u);
  });

  it('re-encodes every published vector byte for byte', () => {
    assert.ok(contract.vectors.length >= 7, 'the corpus lost entries');
    for (const vector of contract.vectors) {
      const reEncoded =
        vector.direction === 'client-to-daemon'
          ? encodeClient(decodeClient(vector.line))
          : encodeDaemon(decodeDaemon(vector.line));
      assert.equal(reEncoded, vector.line, `${vector.name} does not re-encode to its own bytes`);
    }
  });

  it('covers both directions and every message shape', () => {
    const tags = contract.vectors.map((vector) => JSON.parse(vector.line)['t'] as string);
    for (const shape of ['hello', 'call', 'welcome', 'refused', 'result', 'failed', 'chunk']) {
      assert.ok(tags.includes(shape), `the corpus has no \`${shape}\``);
    }
  });
});
