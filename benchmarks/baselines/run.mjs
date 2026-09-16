#!/usr/bin/env node
// The baseline runner: one configured, version-pinned baseline, one workload,
// one row per arm, through the same schema a Mesh row uses.
//
// Order is the contract, and it is the same order the Rust harness uses:
//   1. resolve the baseline's tool and check it against the pin
//   2. materialise the corpus, and digest it before anything touches it
//   3. verify — store, read back, compare digests
//   4. measure the footprint, deterministically, with compaction where the
//      baseline has a collector
//   5. only then time anything
// A failure at step 3 produces no timing number at all.
//
// Exit codes: 0 accepted, 1 refused, 2 the invocation was wrong.

import { existsSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { dirname } from 'node:path';
import { attempt } from './lib/exec.mjs';
import {
  probeHardware,
  probePlatform,
  probeRepository,
  vendorBuildProfile,
} from './lib/probe.mjs';
import { loadVersions, resolveTool, baselineBlock } from './lib/tools.mjs';
import { CORPUS_IDS, prepareCorpus } from './lib/corpus.mjs';
import { WORKLOAD_IDS, buildWorkload, setupIteration, expectedDigestAfter } from './lib/workloads.mjs';
import { buildRow, refusals, appendRow, supportRecord, PUBLISHABLE, EXPLORATORY } from './lib/row.mjs';
import { percentile, P50 } from './lib/stats.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, '..', '..');

const ADAPTERS = {
  'native-fs': () => import('./adapters/native-fs.mjs'),
  'git-worktree': () => import('./adapters/git-worktree.mjs'),
  jujutsu: () => import('./adapters/jujutsu.mjs'),
  replication: () => import('./adapters/replication.mjs'),
};

const USAGE = `benchmarks/baselines/run.mjs — run a configured baseline

  --baseline ID          ${Object.keys(ADAPTERS).join(' | ')} | all   [default: all]
  --workload ID          ${WORKLOAD_IDS.join(' | ')}                  [required]
  --corpus ID            ${CORPUS_IDS.join(' | ')}                    [default: source-tree]
  --scale NAME           full | reduced | smoke (generated corpora)   [default: smoke]
  --seed N               generator seed                               [default: 42]
  --iterations N         timed iterations                             [default: 50]
  --warmup N             untimed iterations                           [default: 3]
  --footprint-reps N     deterministic footprint repetitions          [default: 3]
  --cache warm|cold      cache state                                  [default: warm]
  --repo PATH            checkout the commit is read from             [default: this repository]
  --work DIR             scratch root for corpora and stores          [default: a temp directory]
  --out FILE             append accepted rows as JSON lines
  --support-out FILE     append unsupported-arm records               [default: --out with -support]
  --exploratory          use the looser local policy and say so in the row`;

/** Set the verdict and let node exit on its own — NOT `process.exit(code)`.
 *
 * `process.exit()` under Node v24.7.0 on `aarch64-apple-darwin` kills the process with `SIGSEGV`
 * on roughly a quarter of runs, so a caller reads exit 139 where a verdict was promised. The
 * mechanism, the crash stack and the control measurements are in
 * `tools/program/invocation-check.mjs` beside rule 6, which fails if this shape comes back.
 *
 * Declared BEFORE the `await main()` below on purpose: a `const` is in its temporal dead zone
 * until its declaration is evaluated, and `main` runs first. */
const finish = (code) => { process.exitCode = code; };

await main();

async function main() {
  let options;
  try {
    options = parseOptions(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`run.mjs: ${error.message}\n\n${USAGE}\n`);
    return finish(2);
  }

  let refused = 0;
  const workRoot = options.work ?? join(tmpdir(), `mesh-baseline-${process.pid}`);
  const ownsWorkRoot = options.work === null;
  mkdirSync(workRoot, { recursive: true });

  try {
    const context = buildContext(options, workRoot);
    for (const baselineId of options.baselines) {
      // Sequential on purpose: two baselines measured at once are two baselines
      // contending for the same disk, and neither number is the one it claims.
      refused += await runOneAsync(baselineId, options, context, workRoot);
    }
  } catch (error) {
    process.stderr.write(`run.mjs: ${error.message}\n`);
    return finish(1);
  } finally {
    if (ownsWorkRoot) rmSync(workRoot, { recursive: true, force: true });
  }
  finish(refused === 0 ? 0 : 1);
}

function buildContext(options, workRoot) {
  if (options.cacheState === 'cold' && !coldCacheCommand()) {
    throw new Error(
      'cold runs need a way to drop the page cache. Set MESH_BASELINE_DROP_CACHES to a command ' +
        'that does so on this host, or run warm. A "cold" row taken without dropping anything is ' +
        'the single most common way a storage benchmark lies, so this refuses rather than pretends.',
    );
  }
  const hardware = probeHardware();
  const repository = probeRepository(options.repo);
  const platform = probePlatform(workRoot);
  return {
    hardware,
    repository,
    platform,
    invocation: `benchmarks/baselines/run.mjs ${process.argv.slice(2).join(' ')}`,
    recordedAtUnixMs: Date.now(),
    versions: loadVersions(),
  };
}

/** One arm: resolve the pin, prepare the corpus, verify, measure, emit. */
async function runOneAsync(baselineId, options, context, workRoot) {
  const pin = context.versions.baselines.find((entry) => entry.id === baselineId);
  const tool = resolveTool(pin);
  const arm = `${baselineId}/${options.workload}/${options.corpus}`;

  if (!tool.available) {
    return recordUnsupported(arm, baselineId, options, context, tool.reason);
  }

  const module = await ADAPTERS[baselineId]();
  const unsupported = module.supports(options.workload);
  if (unsupported) {
    return recordUnsupported(arm, baselineId, options, context, unsupported);
  }

  const corpusRoot = join(workRoot, baselineId, 'corpus');
  const storeRoot = join(workRoot, baselineId, 'store');
  const corpus = prepareCorpus(options.corpus, {
    repoRoot: options.repo,
    root: corpusRoot,
    meshBench: options.meshBench,
    scale: options.scale,
    seed: options.seed,
  });
  const workload = buildWorkload(options.workload, corpus);
  const instance = module.create({ sourceRoot: corpusRoot, storeRoot });

  const verification = verify(instance, workload, corpus);
  const footprint = verification.verified
    ? measureFootprint(instance, workload, options)
    : null;
  const samplesNs = verification.verified ? measureLatency(instance, workload, options) : [];

  if (!verification.verified) {
    process.stderr.write(
      `run.mjs: ${arm} failed verification (${verification.method}); no timing number produced\n`,
    );
    return 1;
  }

  const row = buildRow({
    benchmarkId: `mesh-baseline/${baselineId}/${options.workload}`,
    invocation: context.invocation,
    recordedAtUnixMs: context.recordedAtUnixMs,
    repository: context.repository,
    hardware: context.hardware,
    platform: context.platform,
    build: vendorBuildProfile(pin.tool, tool.version ?? 'host'),
    workload: {
      generator: corpus.generator,
      generator_version: corpus.generator_version,
      seed: corpus.seed,
      parameters: {
        ...corpus.parameters,
        corpus: options.corpus,
        corpus_file_count: corpus.file_count,
        corpus_logical_bytes: corpus.logical_bytes,
        corpus_content_digest: corpus.digest,
        baseline_workload: workload.id,
        baseline_workload_title: workload.title,
        plan_reference: workload.planReference,
        ...workload.parameters,
      },
    },
    cacheState: options.cacheState,
    samplesNs,
    failureCount: 0,
    verification,
    baseline: {
      ...baselineBlock(pin, tool),
      retains_history: instance.retainsHistory,
      history_note: instance.historyNote,
      store: instance.storeDescription,
      store_command: instance.storeCommand,
      compaction_command: instance.compactionCommand,
    },
    footprint,
  });

  const policy = options.exploratory ? EXPLORATORY : PUBLISHABLE;
  const reasons = refusals(row, policy);
  if (reasons.length > 0) {
    process.stderr.write(
      `run.mjs: ${arm} row refused, nothing written (${policy.name} policy):\n` +
        reasons.map((reason) => `  - ${reason}\n`).join(''),
    );
    return 1;
  }
  if (options.out) {
    appendRow(options.out, { ...row, sink_policy: policy.name });
    process.stderr.write(`run.mjs: ${arm} appended 1 row to ${options.out}\n`);
  }
  process.stdout.write(`${JSON.stringify({ ...row, sink_policy: policy.name }, null, 2)}\n`);
  return 0;
}

/** Correctness first: store the tree, read it back, compare digests. */
function verify(instance, workload, corpus) {
  setupIteration(instance, workload);
  instance.storeVersion();
  const expected = expectedDigestAfter(corpus, workload);
  const observed = instance.exportLatest();
  return {
    method:
      'the tree is stored, read back out of the baseline store, and folded with FNV-1a/64 over sorted (path, size, bytes); the timing loop runs only if the digests agree',
    expected_digest: expected,
    observed_digest: observed,
    verified: expected === observed,
  };
}

/** The footprint pass: deterministic, compacted where the baseline has a collector. */
function measureFootprint(instance, workload, options) {
  const uncompacted = [];
  const compacted = [];
  const priorCompacted = [];
  for (let repetition = 0; repetition < options.footprintReps; repetition += 1) {
    if (workload.mutation) workload.mutation.revert();
    instance.reset();
    for (let index = 0; index < workload.versionsBeforeTimedStore; index += 1) {
      instance.storeVersion();
    }
    if (workload.versionsBeforeTimedStore > 0) {
      instance.compact();
      priorCompacted.push(instance.footprint().bytes);
    }
    if (workload.mutation) workload.mutation.apply();
    instance.storeVersion();
    uncompacted.push(instance.footprint().bytes);
    instance.compact();
    compacted.push(instance.footprint().bytes);
  }
  const identical = (values) => values.every((value) => value === values[0]);
  const prior = priorCompacted.length > 0 ? percentile([...priorCompacted].sort((a, b) => a - b), P50) : null;
  const after = percentile([...compacted].sort((a, b) => a - b), P50);
  return {
    unit: 'bytes',
    method:
      'sum of apparent file sizes under the baseline store, the same method benchmarks/budgets/storage.md §6 used for its ad-hoc Git column',
    repetitions: options.footprintReps,
    samples_bytes_compacted: compacted,
    samples_bytes_uncompacted: uncompacted,
    deterministic: identical(compacted) && identical(uncompacted),
    store_bytes_compacted: after,
    store_bytes_uncompacted: percentile([...uncompacted].sort((a, b) => a - b), P50),
    store_bytes_before_timed_version: prior,
    delta_bytes_compacted: prior === null ? null : after - prior,
    delta_bytes_uncompacted:
      prior === null
        ? null
        : percentile([...uncompacted].sort((a, b) => a - b), P50) - prior,
  };
}

/** The timing pass. Compaction is outside it: a collector is not a store operation. */
function measureLatency(instance, workload, options) {
  const samples = [];
  for (let index = 0; index < options.warmup + options.iterations; index += 1) {
    setupIteration(instance, workload);
    dropCachesIfCold(options);
    const started = process.hrtime.bigint();
    instance.storeVersion();
    const elapsed = process.hrtime.bigint() - started;
    if (index >= options.warmup) samples.push(Number(elapsed));
  }
  return samples;
}

function coldCacheCommand() {
  if (process.env.MESH_BASELINE_DROP_CACHES) return process.env.MESH_BASELINE_DROP_CACHES;
  if (existsSync('/proc/sys/vm/drop_caches')) {
    const probe = attempt('/bin/sh', ['-c', 'test -w /proc/sys/vm/drop_caches']);
    if (probe.code === 0) return 'sync; echo 3 > /proc/sys/vm/drop_caches';
  }
  return null;
}

function dropCachesIfCold(options) {
  if (options.cacheState !== 'cold') return;
  const command = coldCacheCommand();
  const result = attempt('/bin/sh', ['-c', command]);
  if (result.code !== 0) {
    throw new Error(`cold run: \`${command}\` exited ${result.code}: ${result.stderr.trim()}`);
  }
}

function recordUnsupported(arm, baselineId, options, context, reason) {
  const record = supportRecord({
    baselineId,
    workloadId: options.workload,
    corpus: options.corpus,
    reason,
    recordedAtUnixMs: context.recordedAtUnixMs,
    repository: context.repository,
    hardware: context.hardware,
    platform: context.platform,
  });
  if (options.supportOut) appendRow(options.supportOut, record);
  process.stderr.write(`run.mjs: ${arm} is unsupported here: ${reason}\n`);
  process.stdout.write(`${JSON.stringify(record, null, 2)}\n`);
  return 0;
}

function parseOptions(argv) {
  const options = {
    baselines: Object.keys(ADAPTERS),
    workload: null,
    corpus: 'source-tree',
    scale: 'smoke',
    seed: 42,
    iterations: 50,
    warmup: 3,
    footprintReps: 3,
    cacheState: 'warm',
    repo: REPO_ROOT,
    work: null,
    out: null,
    supportOut: null,
    exploratory: false,
    meshBench: join(REPO_ROOT, 'target', 'release', 'mesh-bench'),
  };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    if (flag === '--exploratory') {
      options.exploratory = true;
      continue;
    }
    const value = argv[index + 1];
    if (value === undefined) throw new Error(`\`${flag}\` needs a value`);
    index += 1;
    switch (flag) {
      case '--baseline':
        if (value !== 'all' && !(value in ADAPTERS)) {
          throw new Error(`unknown baseline \`${value}\`; known: ${Object.keys(ADAPTERS).join(', ')}`);
        }
        options.baselines = value === 'all' ? Object.keys(ADAPTERS) : [value];
        break;
      case '--workload':
        if (!WORKLOAD_IDS.includes(value)) {
          throw new Error(`unknown workload \`${value}\`; known: ${WORKLOAD_IDS.join(', ')}`);
        }
        options.workload = value;
        break;
      case '--corpus':
        if (!CORPUS_IDS.includes(value)) {
          throw new Error(`unknown corpus \`${value}\`; known: ${CORPUS_IDS.join(', ')}`);
        }
        options.corpus = value;
        break;
      case '--scale':
        options.scale = value;
        break;
      case '--seed':
        options.seed = wholeNumber(flag, value);
        break;
      case '--iterations':
        options.iterations = wholeNumber(flag, value);
        break;
      case '--warmup':
        options.warmup = wholeNumber(flag, value);
        break;
      case '--footprint-reps':
        options.footprintReps = wholeNumber(flag, value);
        break;
      case '--cache':
        if (value !== 'warm' && value !== 'cold') {
          throw new Error(`\`--cache ${value}\` must be \`warm\` or \`cold\``);
        }
        options.cacheState = value;
        break;
      case '--repo':
        options.repo = resolve(value);
        break;
      case '--work':
        options.work = resolve(value);
        break;
      case '--out':
        options.out = resolve(value);
        break;
      case '--support-out':
        options.supportOut = resolve(value);
        break;
      case '--mesh-bench':
        options.meshBench = resolve(value);
        break;
      default:
        throw new Error(`unknown option \`${flag}\``);
    }
  }
  if (options.workload === null) throw new Error('`--workload ID` is required');
  if (options.out && !options.supportOut) {
    options.supportOut = options.out.replace(/(\.jsonl)?$/, '-support.jsonl');
  }
  if (options.corpus !== 'source-tree' && !existsSync(options.meshBench)) {
    throw new Error(
      `a generated corpus needs the mesh-bench binary at ${options.meshBench}; ` +
        'build it with `cargo build --release -p mesh-bench --bin mesh-bench`',
    );
  }
  return options;
}

function wholeNumber(flag, text) {
  const parsed = Number(text);
  if (!Number.isInteger(parsed) || parsed < 0) {
    throw new Error(`\`${flag} ${text}\` is not a whole number`);
  }
  return parsed;
}

export { parseOptions, runOneAsync, USAGE, ADAPTERS };
