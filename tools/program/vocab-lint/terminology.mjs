#!/usr/bin/env node
/**
 * terminology — the protocol terminology gate.
 *
 * `docs/protocol.md` claims that every protocol term has exactly one definition
 * and that the register is the only place a term is defined. This lint is what
 * makes that claim mechanical rather than editorial. It is plain Node with no
 * dependencies and no network, so CI, a gate run and a clean laptop all execute
 * the identical check.
 *
 *   node tools/program/vocab-lint/lint.mjs --terminology       # the CI invocation
 *   node tools/program/vocab-lint/terminology.mjs              # the same run
 *   node tools/program/vocab-lint/lint.mjs --terminology --self-test
 *   node tools/program/vocab-lint/lint.mjs --terminology --json
 *   node tools/program/vocab-lint/lint.mjs --terminology --checks TL-1,TL-9
 *   node tools/program/vocab-lint/lint.mjs --terminology --list-terms
 *
 * Exit codes: 0 clean · 1 findings · 2 usage or configuration error.
 *
 * Sibling of `lint.mjs`, which owns the user-facing vocabulary. The two share a
 * directory and an entrypoint and nothing else: this one guards the protocol
 * vocabulary, that one guards the product vocabulary, and §1.2 of
 * `docs/protocol.md` is why the two word lists must never be merged. `lint.mjs`
 * dispatches `--terminology` straight into `run` below, so both spellings of the
 * command execute the identical code and neither can drift from the other.
 */

import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

import { buildModel } from './terminology/model.mjs';
import { CHECKS, CHECK_IDS, runChecks } from './terminology/checks.mjs';
import { renderHuman, renderJson } from './terminology/report.mjs';
import { runSelfTest } from './terminology/selftest.mjs';

const TOOL_ROOT = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(TOOL_ROOT, '..', '..', '..');

const USAGE = `terminology — the protocol terminology gate

Usage:
  node tools/program/vocab-lint/lint.mjs --terminology [options]
  node tools/program/vocab-lint/terminology.mjs [options]

Options:
  --self-test       run the violating-mutation suite only
  --no-self-test    skip the violating-mutation suite
  --checks <a,b>    run only these checks (${CHECK_IDS.join(' | ')})
  --root <dir>      repository root (default: the checkout this file lives in)
  --json            machine-readable report on stdout
  --list-terms      print every register term and exit
  --list-checks     print every check and exit
  -h, --help        this text
`;

/** A usage or configuration error, raised rather than exited on. */
class UsageError extends Error {}

/**
 * Refuse the run.
 *
 * This throws where it used to call `process.exit(2)`. `process.exit()` under Node v24.7.0 on
 * `aarch64-apple-darwin` kills the process with `SIGSEGV` on roughly a quarter of runs, so a lane
 * reads exit 139 where a verdict was promised; the mechanism, the crash stack and the control
 * measurements are in `invocation-check.mjs` beside rule 6, which fails if the shape comes back.
 * `run` catches this and returns 2, so the exported contract — the same bytes on stderr, the same
 * code to the caller — is unchanged, including for `lint.mjs --terminology`, which never saw an
 * exit code at all: it saw a process vanish mid-call.
 */
function fail(message) {
  throw new UsageError(message);
}

function parseArgs(argv) {
  const options = {
    selfTestOnly: false,
    selfTest: true,
    json: false,
    listTerms: false,
    listChecks: false,
    help: false,
    checks: null,
    root: REPO_ROOT,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    switch (arg) {
      case '--self-test':
        options.selfTestOnly = true;
        break;
      case '--no-self-test':
        options.selfTest = false;
        break;
      case '--json':
        options.json = true;
        break;
      case '--list-terms':
        options.listTerms = true;
        break;
      case '--list-checks':
        options.listChecks = true;
        break;
      case '--terminology':
        // Accepted so the documented mode name works on this entrypoint too.
        break;
      case '--checks': {
        index += 1;
        const value = argv[index];
        if (!value) fail('--checks needs a comma-separated list');
        options.checks = value.split(',').map((id) => id.trim().toUpperCase());
        for (const id of options.checks) {
          if (!CHECK_IDS.includes(id)) fail(`unknown check "${id}"`);
        }
        break;
      }
      case '--root': {
        index += 1;
        const value = argv[index];
        if (!value) fail('--root needs a directory');
        options.root = path.resolve(value);
        break;
      }
      case '-h':
      case '--help':
        options.help = true;
        break;
      default:
        fail(`unknown option "${arg}"`);
    }
  }
  return options;
}

/**
 * One run of the terminology gate.
 *
 * @param {string[]} argv arguments after the entrypoint, `--terminology` already
 *   consumed by `lint.mjs` or accepted and ignored here.
 * @returns {number} 0 clean · 1 findings · 2 usage or configuration error.
 */
export function run(argv) {
  try {
    return execute(argv);
  } catch (error) {
    if (!(error instanceof UsageError)) throw error;
    process.stderr.write(`terminology: ${error.message}\n\n${USAGE}`);
    return 2;
  }
}

/** The run itself. Separated from `run` only so the usage refusal has one place to land. */
function execute(argv) {
  const options = parseArgs(argv);
  if (options.help) {
    process.stdout.write(USAGE);
    return 0;
  }
  if (options.listChecks) {
    for (const check of CHECKS) process.stdout.write(`${check.id}  ${check.title}\n`);
    return 0;
  }

  let selfTestOk = true;
  if (options.selfTest || options.selfTestOnly) {
    const result = runSelfTest();
    selfTestOk = result.ok;
    if (!options.json) process.stdout.write(`${result.lines.join('\n')}\n\n`);
    if (options.selfTestOnly) return selfTestOk ? 0 : 1;
  }

  let model;
  try {
    model = buildModel(options.root);
  } catch (error) {
    fail(error.message);
    return 2;
  }

  if (options.listTerms) {
    for (const row of model.register) process.stdout.write(`${row.term}\n`);
    return 0;
  }

  const findings = runChecks(model, options.checks);
  process.stdout.write(
    `${options.json ? renderJson(findings, model) : renderHuman(findings, model)}\n`,
  );
  return findings.length === 0 && selfTestOk ? 0 : 1;
}

/* Run only when invoked directly; `lint.mjs --terminology` imports `run`. The verdict is set and
 * node is left to exit on its own — NOT `process.exit(run(...))`, for the reason `fail` records. */
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = run(process.argv.slice(2));
}
