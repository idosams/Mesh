#!/usr/bin/env node
/**
 * benchmarks/workloads/verify.mjs — does this build still generate the corpus
 * the manifest was published from?
 *
 * `manifest.json` is the cross-machine half of the determinism claim. The Rust
 * test next door proves that two *processes* on this machine agree; nothing a
 * process can do proves that a stranger's machine agrees. What proves that is a
 * digest computed there and compared with one computed here, and the manifest
 * is where the one computed here is written down. As of the commit that added
 * it, "there" has not happened yet: every published digest came off one machine,
 * so on that machine this script is a drift check and the cross-machine property
 * is unproven until somebody else runs it.
 *
 * So this script is deliberately a *second* implementation of the comparison,
 * in a second language, reading the same committed file. If the generator
 * changes without the manifest changing, both fail; if the manifest is edited
 * to match a drifted generator, the diff shows a digest changing with no
 * generator change behind it, which is the thing a reviewer can actually see.
 *
 *   node benchmarks/workloads/verify.mjs --all
 *   node benchmarks/workloads/verify.mjs --all --content
 *   node benchmarks/workloads/verify.mjs --workload W3 --scale smoke
 *   node benchmarks/workloads/verify.mjs --emit > benchmarks/workloads/manifest.json
 *   node benchmarks/workloads/verify.mjs --self-test
 *
 * Zero dependencies, zero network. The one thing it needs is the release
 * binary, and it says how to build it rather than building it behind your back:
 * a verifier that rebuilds its subject can pass against a binary the caller
 * never asked for.
 *
 * What is compared, and what deliberately is not:
 *
 *   compared      plan digest, content digest (with --content), file counts,
 *                 logical bytes, and — re-derived here rather than taken on
 *                 trust — every shape fact and its verdict
 *   not compared  timings. `generation_seconds` is provenance, not a
 *                 threshold; a slower machine is not a failed corpus, and a
 *                 verifier that failed on it would be a machine benchmark
 *                 wearing a correctness check's clothes.
 *
 * Exit codes: 0 verified · 1 a mismatch · 2 the invocation or the environment
 * was wrong.
 */

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, '..', '..');
const MANIFEST = join(HERE, 'manifest.json');
const DEFAULT_BIN = join(REPO_ROOT, 'target', 'release', 'mesh-bench');

// W1-W6 are plan 12.2's; W7 was added by 01KZE5FDN0NPGJ6NQ1NBYRFVH0 so that a
// storage-amplification number has a pinned, regenerable edit stream to be measured over.
const WORKLOADS = ['W1', 'W2', 'W3', 'W4', 'W5', 'W6', 'W7'];
const SCALES = ['full', 'reduced', 'smoke'];

const OK = 0;
const MISMATCH = 1;
const USAGE = 2;

const HELP = `Usage: node benchmarks/workloads/verify.mjs [options]

  --all                every workload at every scale
  --workload W1..W7    restrict to one workload (repeatable)
  --scale NAME         restrict to one scale (repeatable)
  --content            also compare content digests, where the manifest has them
  --emit               print a fresh manifest on stdout instead of verifying
  --self-test          run planted shape-manifest mismatch cases; no binary needed
  --bin PATH           the mesh-bench binary [default: target/release/mesh-bench]
  -h, --help           this message
`;

export function parseArgs(argv) {
  const options = {
    workloads: [],
    scales: [],
    content: false,
    emit: false,
    selfTest: false,
    all: false,
    bin: DEFAULT_BIN,
    help: false,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    const value = () => {
      const next = argv[index + 1];
      if (next === undefined) throw new Error(`${flag} needs a value`);
      index += 1;
      return next;
    };
    switch (flag) {
      case '-h':
      case '--help':
        options.help = true;
        break;
      case '--all':
        options.all = true;
        break;
      case '--content':
        options.content = true;
        break;
      case '--emit':
        options.emit = true;
        break;
      case '--self-test':
        options.selfTest = true;
        break;
      case '--bin':
        options.bin = value();
        break;
      case '--workload': {
        const workload = value().toUpperCase();
        if (!WORKLOADS.includes(workload)) throw new Error(`unknown workload "${workload}"`);
        options.workloads.push(workload);
        break;
      }
      case '--scale': {
        const scale = value().toLowerCase();
        if (!SCALES.includes(scale)) throw new Error(`unknown scale "${scale}"`);
        options.scales.push(scale);
        break;
      }
      default:
        throw new Error(`unknown option "${flag}"`);
    }
  }
  if (options.workloads.length === 0) options.workloads = [...WORKLOADS];
  if (options.scales.length === 0) options.scales = [...SCALES];
  return options;
}

/** Runs the binary and parses its JSON, timing the call. */
function runBinary(bin, args) {
  const started = process.hrtime.bigint();
  let stdout;
  try {
    stdout = execFileSync(bin, args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  } catch (error) {
    const detail = error.stderr ? String(error.stderr).trim() : error.message;
    throw new Error(`\`${[bin, ...args].join(' ')}\` failed: ${detail}`);
  }
  const seconds = Number(process.hrtime.bigint() - started) / 1e9;
  return { value: JSON.parse(stdout), seconds };
}

/** The observation this build makes for one workload at one scale. */
export function observe(bin, workload, scale, seed, withContent) {
  const args = ['corpus', 'describe', '--workload', workload, '--scale', scale, '--seed', String(seed)];
  const described = runBinary(bin, withContent ? [...args, '--content'] : args);
  const shape = described.value.shape ?? { holds: false, facts: [] };
  return {
    workload,
    scale,
    seed,
    generator_version: described.value.generator_version,
    plan_digest: described.value.plan_digest,
    content_digest: withContent ? (described.value.content_digest ?? null) : null,
    shape_holds: shape.holds === true,
    // Preserve all four inputs to the comparison. Keeping only `observed`
    // makes the JavaScript rule inert and takes the Rust verdict on trust.
    facts: Object.fromEntries(
      (shape.facts ?? []).map((fact) => [
        fact.name,
        {
          stated: fact.stated,
          observed: fact.observed,
          tolerance: fact.tolerance,
          holds: fact.holds === true,
        },
      ]),
    ),
    generation_seconds: Number(described.seconds.toFixed(3)),
  };
}

/** `|observed - stated| <= tolerance` — the one rule, re-implemented here on
 * purpose so that two independent checkers have to agree about it. */
export function factHolds(fact) {
  return Math.abs(fact.observed - fact.stated) <= fact.tolerance;
}

/** One shape fact as published by the manifest.
 *
 * Existing numeric entries pin only the observation. An object also pins the
 * stated value and tolerance, so a workload definition cannot move underneath
 * an unchanged observation. No coercion is allowed in either form. */
export function expectedFact(name, value) {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return { observed: value, pinned: false };
  }
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    return {
      error: `shape fact "${name}" is ${JSON.stringify(value)}, which is neither a finite number nor a { stated, observed, tolerance } object`,
    };
  }
  for (const key of ['stated', 'observed', 'tolerance']) {
    if (typeof value[key] !== 'number' || !Number.isFinite(value[key])) {
      return {
        error: `shape fact "${name}" pins ${key} as ${JSON.stringify(value[key])}, which is not a finite number`,
      };
    }
  }
  return {
    stated: value.stated,
    observed: value.observed,
    tolerance: value.tolerance,
    pinned: true,
  };
}

/** Preserve each fact's published form when `--emit` refreshes its numbers. */
export function publishedFacts(previous, observed) {
  return Object.fromEntries(
    Object.entries(observed).map(([name, fact]) => {
      const before = previous?.[name];
      if (before !== null && typeof before === 'object' && !Array.isArray(before)) {
        return [
          name,
          { stated: fact.stated, observed: fact.observed, tolerance: fact.tolerance },
        ];
      }
      return [name, fact.observed];
    }),
  );
}

/** Compares one manifest row against one observation. */
export function compareRow(expected, observed, withContent) {
  const problems = [];
  const at = `${expected.workload} ${expected.scale}`;
  if (expected.generator_version !== observed.generator_version) {
    problems.push(
      `${at}: generator version ${observed.generator_version} but the manifest was published from ${expected.generator_version}`,
    );
  }
  if (expected.plan_digest !== observed.plan_digest) {
    problems.push(`${at}: plan digest ${observed.plan_digest}, manifest says ${expected.plan_digest}`);
  }
  if (withContent && expected.content_digest !== null && expected.content_digest !== undefined) {
    if (expected.content_digest !== observed.content_digest) {
      problems.push(
        `${at}: content digest ${observed.content_digest}, manifest says ${expected.content_digest}`,
      );
    }
  }
  let allHold = true;
  for (const [name, fact] of Object.entries(observed.facts)) {
    const verdict = factHolds(fact);
    allHold = allHold && verdict;
    if (verdict !== fact.holds) {
      problems.push(
        `${at}: the two implementations of the shape rule disagree about "${name}" — mesh-bench says holds=${fact.holds}, |${fact.observed} - ${fact.stated}| <= ${fact.tolerance} is ${verdict}`,
      );
    } else if (!verdict) {
      problems.push(
        `${at}: ${name} is ${fact.observed}, stated ${fact.stated} with a tolerance of ${fact.tolerance}`,
      );
    }
  }
  if (allHold !== observed.shape_holds) {
    problems.push(
      `${at}: mesh-bench reports shape holds=${observed.shape_holds}, this script computes ${allHold} from the same facts`,
    );
  } else if (!allHold) {
    problems.push(`${at}: the generated corpus does not match its own stated shape`);
  }
  for (const [name, value] of Object.entries(expected.facts ?? {})) {
    const wanted = expectedFact(name, value);
    if (wanted.error !== undefined) {
      problems.push(`${at}: ${wanted.error}`);
      continue;
    }
    const actual = observed.facts[name];
    if (actual === undefined) {
      problems.push(`${at}: shape fact "${name}" is no longer reported`);
      continue;
    }
    if (actual.observed !== wanted.observed) {
      problems.push(`${at}: ${name} is ${actual.observed}, manifest says ${wanted.observed}`);
    }
    if (!wanted.pinned) continue;
    if (actual.stated !== wanted.stated) {
      problems.push(`${at}: ${name} is stated as ${actual.stated}, manifest pins ${wanted.stated}`);
    }
    if (actual.tolerance !== wanted.tolerance) {
      problems.push(
        `${at}: ${name} allows a tolerance of ${actual.tolerance}, manifest pins ${wanted.tolerance}`,
      );
    }
    if (!factHolds(wanted)) {
      problems.push(
        `${at}: the manifest's own numbers for ${name} break the published rule — |${wanted.observed} - ${wanted.stated}| <= ${wanted.tolerance} is false`,
      );
    }
  }
  for (const name of Object.keys(observed.facts)) {
    if (!Object.hasOwn(expected.facts ?? {}, name)) {
      problems.push(`${at}: shape fact "${name}" is reported but not pinned by the manifest`);
    }
  }
  return problems;
}

/** Hermetic mutations for the comparison itself; deliberately needs no build. */
export function selfTest() {
  const expected = {
    workload: 'W0',
    scale: 'test',
    generator_version: 'test-v1',
    plan_digest: 'plan',
    content_digest: null,
    facts: {
      file_count: { stated: 10, observed: 10, tolerance: 0 },
    },
  };
  const observed = {
    workload: 'W0',
    scale: 'test',
    seed: 42,
    generator_version: 'test-v1',
    plan_digest: 'plan',
    content_digest: null,
    shape_holds: true,
    facts: {
      file_count: { stated: 10, observed: 10, tolerance: 0, holds: true },
    },
    generation_seconds: 0,
  };
  const mutations = [
    ['a Rust verdict disagrees with the independently derived rule', (want, got) => {
      got.facts.file_count.holds = false;
    }, 'two implementations of the shape rule disagree'],
    ['a pinned manifest definition mismatches the generator definition', (want) => {
      want.facts.file_count = { stated: 12, observed: 10, tolerance: 1 };
    }, 'manifest pins 12'],
    ['a pinned manifest fact is missing from the generator report', (want) => {
      want.facts.byte_count = { stated: 20, observed: 20, tolerance: 0 };
    }, 'shape fact "byte_count" is no longer reported'],
    ['an extra generator fact is absent from the manifest', (_want, got) => {
      got.facts.byte_count = { stated: 20, observed: 20, tolerance: 0, holds: true };
    }, 'shape fact "byte_count" is reported but not pinned'],
  ];
  const failures = [];
  if (compareRow(expected, observed, false).length !== 0) {
    failures.push('the unmodified fixture does not pass');
  }
  for (const [name, mutate, evidence] of mutations) {
    const want = structuredClone(expected);
    const got = structuredClone(observed);
    mutate(want, got);
    const found = compareRow(want, got, false);
    if (!found.some((problem) => problem.includes(evidence))) {
      failures.push(`${name}: expected ${JSON.stringify(evidence)}, got ${JSON.stringify(found)}`);
    }
  }
  const reemitted = publishedFacts(expected.facts, observed.facts);
  if (JSON.stringify(reemitted.file_count) !== JSON.stringify(expected.facts.file_count)) {
    failures.push('re-emitting a pinned fact did not preserve its published form');
  }
  if (failures.length > 0) {
    process.stderr.write(`verify self-test FAIL — ${failures.length} problem(s)\n`);
    for (const failure of failures) process.stderr.write(`  ${failure}\n`);
    return MISMATCH;
  }
  process.stdout.write(`verify self-test OK — ${mutations.length} planted mutation(s) refused\n`);
  return OK;
}

function loadManifest() {
  if (!existsSync(MANIFEST)) {
    throw new Error(`${MANIFEST} is missing; regenerate it with --emit`);
  }
  return JSON.parse(readFileSync(MANIFEST, 'utf8'));
}

function requireBinary(bin) {
  if (existsSync(bin)) return;
  throw new Error(
    `${bin} is not built. Run:\n  cargo build --release -p mesh-bench --bin mesh-bench`,
  );
}

function emit(options, manifest) {
  const rows = [];
  for (const workload of options.workloads) {
    for (const scale of options.scales) {
      const previous = (manifest?.rows ?? []).find(
        (row) => row.workload === workload && row.scale === scale,
      );
      // Content digests are re-measured only where the manifest already
      // carries one, unless --content asks for the lot. Regenerating a
      // hundred-gigabyte digest as a side effect of editing a comment is not a
      // cost anyone signed up for.
      const withContent = options.content || (previous?.content_digest ?? null) !== null;
      const observed = observe(options.bin, workload, scale, manifest?.canonical_seed ?? 42, withContent);
      rows.push({
        workload,
        scale,
        seed: observed.seed,
        generator_version: observed.generator_version,
        plan_digest: observed.plan_digest,
        content_digest: observed.content_digest,
        facts: publishedFacts(previous?.facts, observed.facts),
        generation_seconds: observed.generation_seconds,
      });
    }
  }
  return {
    schema_version: '1',
    generator_family: 'mesh-bench/corpus',
    canonical_seed: manifest?.canonical_seed ?? 42,
    measured_on: manifest?.measured_on ?? null,
    note: manifest?.note ?? null,
    rows,
  };
}

export function main(argv) {
  let options;
  try {
    options = parseArgs(argv);
  } catch (error) {
    process.stderr.write(`verify: ${error.message}\n\n${HELP}`);
    return USAGE;
  }
  if (options.help) {
    process.stdout.write(HELP);
    return OK;
  }
  if (options.selfTest) return selfTest();

  let manifest = null;
  try {
    requireBinary(options.bin);
    manifest = existsSync(MANIFEST) ? loadManifest() : null;
    if (!options.emit && manifest === null) throw new Error(`${MANIFEST} is missing`);
  } catch (error) {
    process.stderr.write(`verify: ${error.message}\n`);
    return USAGE;
  }

  if (options.emit) {
    try {
      process.stdout.write(`${JSON.stringify(emit(options, manifest), null, 2)}\n`);
    } catch (error) {
      process.stderr.write(`verify: ${error.message}\n`);
      return USAGE;
    }
    return OK;
  }

  const problems = [];
  let checked = 0;
  for (const workload of options.workloads) {
    for (const scale of options.scales) {
      const expected = manifest.rows.find((row) => row.workload === workload && row.scale === scale);
      if (!expected) {
        problems.push(`${workload} ${scale}: no manifest row — the manifest does not cover this build`);
        continue;
      }
      const wantContent = options.content && expected.content_digest !== null;
      let observed;
      try {
        observed = observe(options.bin, workload, scale, expected.seed ?? manifest.canonical_seed, wantContent);
      } catch (error) {
        problems.push(`${workload} ${scale}: ${error.message}`);
        continue;
      }
      checked += 1;
      problems.push(...compareRow(expected, observed, wantContent));
      process.stderr.write(
        `verify: ${workload} ${scale} ${observed.plan_digest}${wantContent ? ` ${observed.content_digest}` : ''} (${observed.generation_seconds}s)\n`,
      );
    }
  }

  if (problems.length > 0) {
    process.stderr.write(`verify FAIL — ${problems.length} problem(s)\n`);
    for (const problem of problems) process.stderr.write(`  ${problem}\n`);
    return MISMATCH;
  }
  process.stdout.write(`verify OK — ${checked} workload/scale pair(s) match the published manifest\n`);
  return OK;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv.slice(2)));
}
