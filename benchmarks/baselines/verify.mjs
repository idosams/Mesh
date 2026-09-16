#!/usr/bin/env node
// The baseline contract check: pins, schema, percentiles, terms, published rows.
//
// Six checks, one rule each. Nothing here times anything — it is deterministic,
// offline and fast enough to be a gate, and it fails when a baseline stops being
// the baseline the published rows were measured against.
//
// Exit codes: 0 the contract holds, 1 something drifted, 2 the invocation was wrong.

import { existsSync, readdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { attempt } from './lib/exec.mjs';
import { loadVersions, resolveTool, parseVersion } from './lib/tools.mjs';
import { latencySummary } from './lib/stats.mjs';
import { SCHEMA_VERSION, buildRow, refusals, PUBLISHABLE } from './lib/row.mjs';
import * as nativeFs from './adapters/native-fs.mjs';
import * as gitWorktree from './adapters/git-worktree.mjs';
import * as jujutsu from './adapters/jujutsu.mjs';
import * as replication from './adapters/replication.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, '..', '..');
const REPORTS = join(HERE, 'reports');
const ADAPTERS = [nativeFs, gitWorktree, jujutsu, replication];
const ADAPTER_IDS = ADAPTERS.map((adapter) => adapter.id);

const failures = [];
const notes = [];

function check(name, body) {
  try {
    body();
    process.stdout.write(`ok    ${name}\n`);
  } catch (error) {
    failures.push(`${name}: ${error.message}`);
    process.stdout.write(`FAIL  ${name}: ${error.message}\n`);
  }
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

const meshBench = process.env.MESH_BENCH_BIN ?? join(REPO_ROOT, 'target', 'release', 'mesh-bench');

check('every baseline in versions.json has an adapter, and every adapter a pin', () => {
  const versions = loadVersions();
  const pinned = versions.baselines.map((entry) => entry.id).sort();
  assert(
    JSON.stringify(pinned) === JSON.stringify([...ADAPTER_IDS].sort()),
    `versions.json pins ${pinned.join(', ')} but the adapters are ${[...ADAPTER_IDS].sort().join(', ')}`,
  );
  for (const entry of versions.baselines) {
    assert(existsSync(join(HERE, 'adapters', `${entry.id}.mjs`)), `no adapter file for ${entry.id}`);
    assert(typeof entry.terms === 'string' && entry.terms.length > 0, `${entry.id} states no terms`);
    assert(
      entry.pin_mode === 'exact' || entry.pin_mode === 'host-recorded',
      `${entry.id} has pin_mode \`${entry.pin_mode}\`, which is not one of exact | host-recorded`,
    );
  }
});

check('every adapter implements the baseline interface', () => {
  for (const adapter of ADAPTERS) {
    assert(typeof adapter.id === 'string' && adapter.id.length > 0, 'an adapter has no id');
    assert(typeof adapter.supports === 'function', `${adapter.id} has no supports()`);
    assert(typeof adapter.create === 'function', `${adapter.id} has no create()`);
    const unsupported = adapter.supports('store-tree');
    assert(
      unsupported === null || typeof unsupported === 'string',
      `${adapter.id}.supports() must return null or a reason, not ${typeof unsupported}`,
    );
  }
  assert(
    replication.supports('checkpoint-history') !== null,
    'the replication baseline must declare the workload it cannot perform, rather than losing it',
  );
});

check('no runner exists for a product whose terms forbid publishing a comparison', () => {
  for (const product of loadVersions().competitor_products) {
    assert(product.publishable === false, `${product.id} is listed as publishable in the wrong list`);
    assert(
      typeof product.reason === 'string' && product.reason.length > 0,
      `${product.id} is refused without a stated reason`,
    );
    assert(
      !existsSync(join(HERE, 'adapters', `${product.id}.mjs`)),
      `${product.id} has an adapter but its terms forbid publishing a comparison`,
    );
  }
});

check('every pinned tool present on this host is at a pinned version', () => {
  for (const pin of loadVersions().baselines) {
    const tool = resolveTool(pin);
    if (!tool.available) {
      notes.push(`${pin.id}: ${tool.reason} — recorded as unsupported, not as a loss`);
      continue;
    }
    assert(
      tool.pin_satisfied,
      `${pin.id} is ${pin.tool} ${tool.version} but the pin accepts ${JSON.stringify(pin.accepted)}; ` +
        'that is a different baseline, not a regression — re-pin deliberately or run --exploratory',
    );
  }
});

check('a version banner reduces to the version', () => {
  assert(parseVersion('git version 2.51.0') === '2.51.0', 'git banner');
  assert(parseVersion('jj 0.34.0\nmore') === '0.34.0', 'jj banner');
  assert(
    parseVersion('rsync  version 3.4.1  protocol version 32') === '3.4.1',
    'rsync banner',
  );
  assert(parseVersion('no version here') === 'no version here', 'a bannerless tool must fail loudly');
});

check('a baseline row carries every field mesh-bench requires, and mesh-bench accepts it', () => {
  assert(
    existsSync(meshBench),
    `mesh-bench is not built at ${meshBench}; run \`cargo build --release -p mesh-bench --bin mesh-bench\``,
  );
  const required = attempt(meshBench, ['fields']);
  assert(required.code === 0, `\`mesh-bench fields\` exited ${required.code}`);
  const row = sampleRow();
  for (const path of required.stdout.trim().split('\n')) {
    assert(hasPath(row, path), `a baseline row is missing the required field \`${path}\``);
  }
  assert(row.schema_version === SCHEMA_VERSION, 'the row does not carry the shared schema version');

  const scratch = join(tmpdir(), `mesh-baseline-verify-${process.pid}.jsonl`);
  writeFileSync(scratch, `${JSON.stringify(row)}\n`, 'utf8');
  const validated = attempt(meshBench, ['validate', scratch]);
  rmSync(scratch, { force: true });
  assert(
    validated.code === 0,
    `mesh-bench refused a row this runner produced: ${validated.stderr.trim()}`,
  );
});

check('the JavaScript percentiles agree with the Rust decoder, sample for sample', () => {
  // The decoder recomputes `latency` from `samples_ns` and refuses a row whose
  // summary disagrees. Feeding it a row whose summary came from lib/stats.mjs is
  // therefore a direct check that the two implementations land on the same
  // integers — the property a reader re-deriving p99 from a published row needs.
  const samples = [];
  for (let index = 1; index <= 250; index += 1) samples.push(index * 37 + (index % 7) * 11);
  const summary = latencySummary(samples);
  assert(summary.p50_ns === [...samples].sort((a, b) => a - b)[124], 'p50 is not nearest-rank');
  const row = { ...sampleRow(), samples_ns: samples, sample_count: samples.length, iterations_attempted: samples.length, latency: summary };
  const scratch = join(tmpdir(), `mesh-baseline-percentiles-${process.pid}.jsonl`);
  writeFileSync(scratch, `${JSON.stringify(row)}\n`, 'utf8');
  const validated = attempt(meshBench, ['validate', scratch]);
  rmSync(scratch, { force: true });
  assert(validated.code === 0, `percentile disagreement: ${validated.stderr.trim()}`);
});

check('the publishing policy refuses what it says it refuses', () => {
  const good = sampleRow();
  assert(refusals(good, PUBLISHABLE).length === 0, 'a clean row was refused');
  assert(
    refusals({ ...good, repository: { ...good.repository, dirty: true } }, PUBLISHABLE).length === 1,
    'a dirty-worktree row was accepted',
  );
  assert(
    refusals({ ...good, sample_count: 4 }, PUBLISHABLE).length === 1,
    'a four-sample row was accepted',
  );
  assert(
    refusals(
      { ...good, verification: { ...good.verification, observed_digest: 'x', verified: false } },
      PUBLISHABLE,
    ).length === 1,
    'an unverified row was accepted',
  );
  assert(
    refusals({ ...good, baseline: { ...good.baseline, pin_satisfied: false } }, PUBLISHABLE)
      .length === 1,
    'a drifted-version row was accepted',
  );
});

check('every published row still validates and still names a pinned version', () => {
  if (!existsSync(REPORTS)) {
    notes.push('benchmarks/baselines/reports/ holds no published rows yet');
    return;
  }
  const files = readdirSync(REPORTS).filter((name) => name.endsWith('.jsonl'));
  for (const name of files) {
    const path = join(REPORTS, name);
    const rows = readFileSync(path, 'utf8')
      .split('\n')
      .filter((line) => line.trim().length > 0)
      .map((line) => JSON.parse(line));
    if (name.endsWith('-support.jsonl')) {
      for (const record of rows) {
        assert(record.supported === false, `${name}: a support record claims support`);
        assert(record.reason?.length > 0, `${name}: a support record states no reason`);
      }
      continue;
    }
    const validated = attempt(meshBench, ['validate', path]);
    assert(validated.code === 0, `${name}: ${validated.stderr.trim()}`);
    for (const row of rows) {
      const pin = loadVersions().baselines.find((entry) => entry.id === row.baseline.id);
      assert(pin, `${name}: published row names unknown baseline ${row.baseline.id}`);
      assert(
        pin.accepted === null || pin.accepted.includes(row.baseline.version),
        `${name}: published row was measured on ${row.baseline.tool} ${row.baseline.version}, ` +
          `which versions.json no longer accepts (${JSON.stringify(pin.accepted)})`,
      );
      assert(row.verification.verified === true, `${name}: a published row is unverified`);
    }
  }
});

if (notes.length > 0) {
  process.stdout.write(`\n${notes.map((note) => `note  ${note}`).join('\n')}\n`);
}
// The verdict is set and node is left to exit on its own — NOT `process.exit(code)`.
// `process.exit()` under Node v24.7.0 on `aarch64-apple-darwin` kills the process with `SIGSEGV`
// on roughly a quarter of runs, so a caller reads exit 139 where a verdict was promised — and the
// exit 0 below segfaulted on a PASS as readily as the exit 1 did on a FAIL. The mechanism, the
// crash stack and the control measurements are in `tools/program/invocation-check.mjs` beside
// rule 6, which fails if this shape comes back.
if (failures.length > 0) {
  process.stderr.write(`\nverify.mjs: ${failures.length} check(s) failed\n`);
  process.exitCode = 1;
} else {
  process.stdout.write('\nverify.mjs: the baseline contract holds\n');
  process.exitCode = 0;
}

/** A complete, valid row with fabricated numbers — the shape, not a measurement. */
function sampleRow() {
  const samples = [];
  for (let index = 0; index < 32; index += 1) samples.push(1_000_000 + index * 137);
  return buildRow({
    benchmarkId: 'mesh-baseline/self-check/store-tree',
    invocation: 'benchmarks/baselines/verify.mjs',
    recordedAtUnixMs: 1_700_000_000_000,
    repository: { remote: 'https://example.invalid/mesh.git', commit: 'a'.repeat(40), dirty: false },
    hardware: { cpu_model: 'self-check', physical_cores: 1, logical_cores: 1, memory_bytes: 1 << 30 },
    platform: { os: 'macos', os_version: '0.0', arch: 'aarch64', filesystem: 'apfs' },
    build: {
      profile: 'vendor-binary',
      opt_level: 'vendor-default',
      debug_info: 'vendor-default',
      rustc_version: 'n/a',
      target_triple: 'aarch64-apple-darwin',
    },
    workload: { generator: 'self-check', generator_version: '1', seed: 42, parameters: {} },
    cacheState: 'warm',
    samplesNs: samples,
    failureCount: 0,
    verification: {
      method: 'self-check',
      expected_digest: 'fnv1a64:0000000000000000',
      observed_digest: 'fnv1a64:0000000000000000',
      verified: true,
    },
    baseline: {
      id: 'native-fs',
      title: 'self-check',
      tool: 'self-check',
      version: 'host',
      pin_satisfied: true,
      pin_accepted: null,
    },
    footprint: { unit: 'bytes', store_bytes_compacted: 1 },
  });
}

function hasPath(object, path) {
  let cursor = object;
  for (const key of path.split('.')) {
    if (cursor === null || typeof cursor !== 'object' || !(key in cursor)) return false;
    cursor = cursor[key];
  }
  return true;
}
