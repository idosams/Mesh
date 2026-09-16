#!/usr/bin/env node
// protocol/conformance/run.mjs — the CWP conformance suite (task 01KZC2P6SQ7E00ENWQ6F4YVXJJ).
//
// Run the published protocol against a client, across a process boundary, and report every case as
// `pass`, `fail` or `unsupported`. `protocol/conformance/README.md` is the contract; this file is
// the entry point.
//
// Contract, matching `protocol/verify-published.mjs` deliberately:
//  - Zero network, zero dependencies, no build step.
//  - Exit code is the verdict: 0 no case failed · 1 at least one case failed · 2 usage or adapter
//    error. `unsupported` NEVER contributes to a non-zero exit.
//  - `--self-test` runs the suite against the reference client and against every mutation of the
//    deliberately broken one, and asserts each mutation is caught by the case that names its rule.
//    A conformance case only ever observed to pass is not evidence that it can fail.
//
// Usage:
//   node protocol/conformance/run.mjs --client reference
//   node protocol/conformance/run.mjs --client second
//   node protocol/conformance/run.mjs --client broken --break long-head
//   node protocol/conformance/run.mjs --client "python3 my_adapter.py" --report out.json
//   node protocol/conformance/run.mjs --list
//   node protocol/conformance/run.mjs --self-test

import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { open } from "./lib/adapter.mjs";
import { buildCatalogue, FAMILIES, loadPublished } from "./lib/catalogue.mjs";
import { buildReport, renderText } from "./lib/report.mjs";
import { runCases } from "./lib/run-cases.mjs";
import { MUTATIONS } from "./clients/broken/client-mutations.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(HERE, "..", "..");

const PYTHON = process.env.MESH_CONFORMANCE_PYTHON ?? "python3";

const NAMED_CLIENTS = {
  reference: { command: process.execPath, args: [join(HERE, "clients", "published", "client.mjs")] },
  published: { command: process.execPath, args: [join(HERE, "clients", "published", "client.mjs")] },
  broken: { command: process.execPath, args: [join(HERE, "clients", "broken", "client.mjs")] },
  // The R14 second client (task 01KZC2TBX5BGTQPXXX2DATM3TY): another language, another author, and
  // a process that cannot open `test-vectors/` at all. Needs a `python3` on PATH; set
  // MESH_CONFORMANCE_PYTHON to name a different interpreter.
  second: { command: PYTHON, args: [join(HERE, "clients", "second", "client.py")] },
};

function parseArguments(argv) {
  const options = {
    client: "reference",
    break: null,
    report: null,
    json: false,
    verbose: false,
    only: null,
    list: false,
    selfTest: false,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    const next = () => argv[(index += 1)];
    switch (flag) {
      case "--client":
        options.client = next();
        break;
      case "--break":
        options.break = next();
        break;
      case "--report":
        options.report = next();
        break;
      case "--only":
        options.only = next();
        break;
      case "--json":
        options.json = true;
        break;
      case "--verbose":
        options.verbose = true;
        break;
      case "--list":
        options.list = true;
        break;
      case "--self-test":
        options.selfTest = true;
        break;
      case "--help":
      case "-h":
        options.help = true;
        break;
      default:
        throw new Error(`unknown option ${flag}`);
    }
  }
  return options;
}

function resolveClient(name) {
  if (Object.prototype.hasOwnProperty.call(NAMED_CLIENTS, name)) {
    return { name, ...NAMED_CLIENTS[name] };
  }
  const parts = name.split(/\s+/).filter(Boolean);
  if (parts.length === 0) throw new Error("--client needs a name or a command");
  return { name, command: parts[0], args: parts.slice(1) };
}

async function runAgainst(client, cases, { only, env } = {}) {
  const startedAt = Date.now();
  const { adapter, hello } = await open(client.command, client.args, { cwd: REPO_ROOT, env });
  try {
    const results = await runCases(adapter, hello, cases, { only });
    return buildReport({
      client: { name: client.name, command: [client.command, ...client.args].join(" ") },
      hello,
      results,
      startedAt,
      finishedAt: Date.now(),
    });
  } finally {
    adapter.close();
  }
}

// ---------------------------------------------------------------------------
// --self-test
// ---------------------------------------------------------------------------

async function selfTest(cases) {
  const problems = [];
  const note = (line) => process.stdout.write(`${line}\n`);

  note("self-test 1/3 — the reference client passes every case it claims to support");
  const reference = await runAgainst(resolveClient("reference"), cases);
  if (reference.summary.fail !== 0) {
    problems.push(
      `the reference client failed ${reference.summary.fail} case(s): ` +
        reference.cases
          .filter((result) => result.result === "fail")
          .map((result) => result.id)
          .join(", "),
    );
  }
  if (reference.summary.pass === 0) problems.push("the reference client passed nothing");
  if (reference.summary.unsupported === 0) {
    problems.push(
      "the reference client reported no unsupported case, so `unsupported` is untested. It must " +
        "be reachable: the publication family cannot be answered by anything today.",
    );
  }
  note(
    `  ${reference.summary.pass} pass · ${reference.summary.fail} fail · ` +
      `${reference.summary.unsupported} unsupported`,
  );

  note("self-test 2/3 — a specification gap is reported as one, not hidden as a client gap");
  const gaps = reference.specification_gaps;
  if (gaps.length === 0) {
    problems.push(
      "no case was reported as a specification gap. record_id_hex cannot be recomputed from the " +
        "published material (01KZCZDTVD0D36W5YRGX8CNE17) and the publication family is not " +
        "implemented; a suite that reports neither is papering over both.",
    );
  }
  if (!gaps.some((gap) => gap.tracking === "01KZCZDTVD0D36W5YRGX8CNE17")) {
    problems.push(
      "the record identity gap is not reported against 01KZCZDTVD0D36W5YRGX8CNE17. That " +
        "contradiction is the one an external implementer meets first; it is not removed to make " +
        "the report look complete.",
    );
  }
  const identity = reference.cases.filter((result) => result.family === "ID");
  if (identity.length === 0) problems.push("there are no record-identity cases at all");
  for (const result of identity) {
    if (result.result === "fail") {
      problems.push(`the identity case ${result.id} was graded fail; it is unsupported, not wrong`);
    }
  }
  note(`  ${gaps.length} specification gap(s) reported`);

  note(`self-test 3/3 — every one of ${Object.keys(MUTATIONS).length} injected violations is caught`);
  for (const [mutation, definition] of Object.entries(MUTATIONS)) {
    const report = await runAgainst(resolveClient("broken"), cases, {
      env: { MESH_CONFORMANCE_BREAK: mutation },
    });
    const failures = report.cases.filter((result) => result.result === "fail");
    if (failures.length === 0) {
      problems.push(`the mutation ${mutation} was not caught by any case (${definition.violates})`);
      continue;
    }
    const named = failures.find((result) => result.id === definition.caught_by);
    if (!named) {
      problems.push(
        `the mutation ${mutation} was caught, but not by ${definition.caught_by}; it failed ` +
          `${failures.map((result) => result.id).slice(0, 3).join(", ")}`,
      );
      continue;
    }
    if (!named.rule || !named.citation || !named.detail) {
      problems.push(`the failure for ${mutation} did not name its rule, citation and difference`);
    }
    note(`  ${mutation.padEnd(26)} caught by ${named.id}`);
  }

  if (problems.length > 0) {
    process.stderr.write(`\nself-test FAILED\n${problems.map((line) => `  - ${line}`).join("\n")}\n`);
    return 1;
  }
  process.stdout.write("\nself-test PASSED\n");
  return 0;
}

// ---------------------------------------------------------------------------

const USAGE = `Usage: node protocol/conformance/run.mjs [options]

  --client <name|command>  reference (default) · second · broken · or a command to spawn
  --break <mutation>       with --client broken, which violation to inject
  --only <substring>       run only cases whose id contains this, or one family
  --report <file>          write the JSON report here
  --json                   write the JSON report to stdout instead of the text one
  --verbose                print passing and unsupported cases too
  --list                   print the catalogue and exit
  --self-test              check the suite against itself and exit

Exit code: 0 no case failed · 1 at least one case failed · 2 usage or adapter error.
An unsupported case never makes the exit code non-zero.`;

async function main() {
  let options;
  try {
    options = parseArguments(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`${error.message}\n\n${USAGE}\n`);
    return 2;
  }
  if (options.help) {
    process.stdout.write(`${USAGE}\n`);
    return 0;
  }

  const published = loadPublished();
  const cases = buildCatalogue(published);

  if (options.list) {
    for (const [family, description] of Object.entries(FAMILIES)) {
      const members = cases.filter((item) => item.family === family);
      process.stdout.write(`\n${family} — ${description} (${members.length} cases)\n`);
      for (const item of members) process.stdout.write(`  ${item.id}\n    ${item.rule}\n`);
    }
    process.stdout.write(`\n${cases.length} cases\n`);
    return 0;
  }

  if (options.selfTest) return selfTest(cases);

  const client = resolveClient(options.client);
  const env = options.break ? { MESH_CONFORMANCE_BREAK: options.break } : undefined;

  let report;
  try {
    report = await runAgainst(client, cases, { only: options.only, env });
  } catch (error) {
    process.stderr.write(`could not run ${client.name}: ${error.message}\n`);
    return 2;
  }

  if (options.report) writeFileSync(options.report, `${JSON.stringify(report, null, 2)}\n`);
  process.stdout.write(
    options.json
      ? `${JSON.stringify(report, null, 2)}\n`
      : `${renderText(report, { verbose: options.verbose })}\n`,
  );
  return report.summary.fail === 0 ? 0 : 1;
}

process.exitCode = await main();
