#!/usr/bin/env node
/**
 * vocab-lint — the product vocabulary gate.
 *
 * The internal model is a version history; the user model is six words. This
 * lint is what stops the first from leaking into the second. It is plain Node
 * with no dependencies so that CI, the desktop build and a clean laptop all run
 * the identical check.
 *
 *   node tools/program/vocab-lint/lint.mjs --user-facing   # the CI invocation
 *   node tools/program/vocab-lint/lint.mjs --self-test     # fixtures only
 *   node tools/program/vocab-lint/lint.mjs path/to/file.ts # ad-hoc
 *   node tools/program/vocab-lint/lint.mjs --list-words    # the vocabulary
 *   node tools/program/vocab-lint/lint.mjs --terminology   # the protocol register
 *
 * Two modes, two word lists, one entrypoint. `--user-facing` guards the *product*
 * vocabulary — the words a user must never see. `--terminology` guards the
 * *protocol* register in `docs/protocol.md` — the words this repository must use
 * with exactly one meaning each. §1.2 of that document is why the two lists stay
 * separate: three of the words banned from every product surface are precise and
 * required in the protocol documents, so one merged list would have to contradict
 * itself. `--terminology` is therefore dispatched whole to `terminology.mjs`
 * rather than folded into the surface engine below, which is what stops the two
 * from growing a shared word list by accident.
 *
 * Exit codes: 0 clean · 1 findings · 2 usage or configuration error.
 */

import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

import { run as runTerminology } from './terminology.mjs';

import { loadManifest, resolveSurfaceFiles } from './lib/surfaces.mjs';
import { lintFile, inferMode } from './lib/engine.mjs';
import { runSelfTest } from './lib/selftest.mjs';
import { renderHuman, renderJson } from './lib/report.mjs';
import { FORBIDDEN_TERMS, APPROVED_STATUS } from './lib/vocabulary.mjs';
import { RULE_IDS } from './lib/rules/index.mjs';
import { MODES } from './lib/extract.mjs';

const TOOL_ROOT = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(TOOL_ROOT, '..', '..', '..');

const USAGE = `vocab-lint — the product vocabulary gate

Usage:
  node tools/program/vocab-lint/lint.mjs [options] [files…]

Options:
  --user-facing     lint every surface in surfaces.json (the default)
  --self-test       run the fixture suite only
  --no-self-test    skip the fixture suite
  --mode <mode>     extraction mode for ad-hoc files (${MODES.join(' | ')})
  --rules <a,b>     rules for ad-hoc files (${RULE_IDS.join(' | ')})
  --root <dir>      repository root (default: the checkout this file lives in)
  --json            machine-readable report on stdout
  --list-words      print the vocabulary and exit
  --terminology     run the protocol terminology gate instead (see its --help)
  -h, --help        this text
`;

function parseArgs(argv) {
  const options = {
    userFacing: false,
    selfTestOnly: false,
    selfTest: true,
    json: false,
    listWords: false,
    help: false,
    mode: null,
    rules: null,
    root: REPO_ROOT,
    files: [],
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    switch (arg) {
      case '--user-facing':
        options.userFacing = true;
        break;
      case '--self-test':
        options.selfTestOnly = true;
        break;
      case '--no-self-test':
        options.selfTest = false;
        break;
      case '--json':
        options.json = true;
        break;
      case '--list-words':
        options.listWords = true;
        break;
      case '-h':
      case '--help':
        options.help = true;
        break;
      case '--mode':
        options.mode = argv[++i];
        break;
      case '--rules':
        options.rules = (argv[++i] ?? '').split(',').filter(Boolean);
        break;
      case '--root':
        options.root = path.resolve(argv[++i] ?? '.');
        break;
      default:
        if (arg.startsWith('-')) throw new Error(`unknown option "${arg}"`);
        options.files.push(arg);
    }
  }
  if (options.mode !== null && !MODES.includes(options.mode)) {
    throw new Error(`unknown --mode "${options.mode}"; expected one of ${MODES.join(', ')}`);
  }
  for (const rule of options.rules ?? []) {
    if (!RULE_IDS.includes(rule)) throw new Error(`unknown rule "${rule}"; known rules: ${RULE_IDS.join(', ')}`);
  }
  return options;
}

function listWords() {
  const lines = ['Never exposed to a user:', ''];
  for (const term of FORBIDDEN_TERMS) {
    lines.push(`  ${term.term.padEnd(15)} say instead: ${term.sayInstead}`);
    lines.push(`  ${''.padEnd(15)} catches: ${term.matches.join(', ')}`);
    lines.push(`  ${''.padEnd(15)} allows:  ${term.allows.join(', ')}`);
    lines.push('');
  }
  lines.push(`User-facing status is exactly: ${APPROVED_STATUS.join(' · ')}.`);
  return lines.join('\n');
}

function lintSurfaces(root) {
  const manifest = loadManifest(TOOL_ROOT);
  const files = [];
  for (const surface of manifest.surfaces) {
    for (const relPath of resolveSurfaceFiles(root, surface)) {
      files.push(
        lintFile({
          root,
          toolRoot: TOOL_ROOT,
          relPath,
          mode: surface.mode,
          rules: surface.rules,
          allowRegionBudget: surface.allowRegionBudget,
          statusPath: surface.statusPath,
        }),
      );
    }
  }
  return files;
}

/**
 * Lint the files named on the command line.
 *
 * A file that is already a declared surface is linted *as* that surface: same
 * mode, same rules, same suppression budget. Hardcoding the budget to 0 here
 * made `lint.mjs docs/product-prd.md` report the PRD's four reviewed
 * suppressions as a finding and exit 1, while `--user-facing` called the same
 * file clean. A one-file pre-push check that contradicts the gate is a check
 * people stop believing. Explicit `--mode` / `--rules` flags still win.
 */
function lintAdHoc(options) {
  const manifest = loadManifest(TOOL_ROOT);
  return options.files.map((file) => {
    const relPath = toRepoRelative(options.root, file);
    const surface = surfaceFor(manifest, options.root, relPath);
    return lintFile({
      root: options.root,
      toolRoot: TOOL_ROOT,
      relPath,
      mode: options.mode ?? surface?.mode ?? inferMode(relPath),
      rules: options.rules ?? surface?.rules ?? RULE_IDS,
      allowRegionBudget: surface?.allowRegionBudget ?? 0,
      statusPath: surface?.statusPath,
    });
  });
}

function toRepoRelative(root, file) {
  return path.relative(root, path.resolve(root, file)).split(path.sep).join('/');
}

/** The declared surface covering `relPath`, if any. */
function surfaceFor(manifest, root, relPath) {
  return manifest.surfaces.find((surface) => resolveSurfaceFiles(root, surface).includes(relPath));
}

/**
 * Hand the whole run to the terminology gate when `--terminology` is present.
 *
 * The dispatch happens before this entrypoint's own argument parsing, so the
 * terminology mode owns its flags outright and `--self-test`, `--json` and
 * `--root` mean whatever that mode says they mean rather than being silently
 * reinterpreted by the product-vocabulary parser.
 */
function main(argv) {
  if (argv.includes('--terminology')) {
    return runTerminology(argv.filter((arg) => arg !== '--terminology'));
  }

  let options;
  try {
    options = parseArgs(argv);
  } catch (error) {
    process.stderr.write(`vocab-lint: ${error.message}\n\n${USAGE}`);
    return 2;
  }
  if (options.help) {
    process.stdout.write(USAGE);
    return 0;
  }
  if (options.listWords) {
    process.stdout.write(`${listWords()}\n`);
    return 0;
  }

  let run;
  try {
    const selfTest = options.selfTestOnly || options.selfTest ? runSelfTest(TOOL_ROOT) : null;
    const files = options.selfTestOnly
      ? []
      : options.files.length > 0
        ? lintAdHoc(options)
        : lintSurfaces(options.root);
    run = { files, selfTest };
  } catch (error) {
    process.stderr.write(`vocab-lint: ${error.message}\n`);
    return 2;
  }

  process.stdout.write(`${options.json ? renderJson(run) : renderHuman(run)}\n`);
  const clean = run.files.every((file) => file.findings.length === 0) && (run.selfTest?.ok ?? true);
  return clean ? 0 : 1;
}

process.exitCode = main(process.argv.slice(2));
