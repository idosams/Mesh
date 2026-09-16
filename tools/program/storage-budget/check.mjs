#!/usr/bin/env node
// tools/program/storage-budget/check.mjs — the steady-state storage budget as a
// program rather than a paragraph.
//
// `benchmarks/budgets/storage.md` is the missing plan §12.4 row: how many bytes a
// Mesh workspace occupies while it sits there. A page of numbers is worth nothing
// on its own — this repository has shipped a validator nothing invoked, a checker
// with no writer, and a `verify:editions` that was orphaned AND failing while
// `npm test` read green. So the budget is enforced from two directions and this
// file is one of them:
//
//   * `npm run verify:rust` runs the two footprint tests, which perform real
//     writes and fail when a MEASUREMENT breaches a constant.
//   * `npm run verify:storage` — this file — fails when the DOCUMENT and the
//     constant disagree, when a constant has no row, when a row names a test the
//     workspace does not run, or when the composed headline stops following from
//     its parts.
//
// Neither half is sufficient. A test with a constant nobody documents is a number
// with no justification; a document with no test is a claim. Both are on the
// `npm test` path, and rule 6 is what keeps them there.
//
// Contract, deliberately:
//  - Zero network, zero dependencies, no build step, well under a second.
//  - Exit code is the verdict. Every failure names the file and the fix.
//  - `--self-test` mutates the REAL document and the REAL test files inside a
//    synthetic tree and asserts every mutation is REJECTED, then applies the
//    tolerances and asserts each is ACCEPTED. A check only ever observed to pass
//    is not evidence that it can fail.
//
// Exit codes: 0 the budget holds · 1 it does not · 2 the invocation was wrong.

import { existsSync, readFileSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

import { wiringViolations } from "../npm-wiring.mjs";

const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");

/** The document under test. It is an input to this program, not decoration. */
export const PAGE = "benchmarks/budgets/storage.md";

/** The workspace manifest, read to prove an enforcing crate is actually built. */
const CARGO = "Cargo.toml";

/** Constant-name prefixes the register must cover. A constant outside these is
 * ordinary test scaffolding and carries no budget. */
const REGISTERED = /^(BUDGET_|GATE_CORPUS_|ACTOR_SCALING_)/;

/** The derived rows of §4, and how each is computed from the register. Keeping
 * the arithmetic here rather than in prose is the point: the page cannot state a
 * headline that does not follow from the budgets it publishes. */
const DERIVED = {
  MEAN_FILE_BYTES: (b) => idiv(b.GATE_CORPUS_CONTENT_BYTES, b.GATE_CORPUS_FILES),
  PER_FILE_OVERHEAD_BYTES: (b) =>
    b.BUDGET_CAS_JOURNAL_BYTES_PER_CHUNK
    + b.BUDGET_INDEX_RECORDS_PER_FILE_VERSION * b.BUDGET_INDEX_BYTES_PER_RECORD,
  MARGINAL_AMPLIFICATION_PER_MILLE: (b) =>
    1000 + idiv(DERIVED.PER_FILE_OVERHEAD_BYTES(b) * 1000, DERIVED.MEAN_FILE_BYTES(b)),
  TOTAL_AMPLIFICATION_PER_MILLE: (b) =>
    DERIVED.MARGINAL_AMPLIFICATION_PER_MILLE(b)
    + idiv(b.BUDGET_INDEX_SCHEMA_FLOOR_BYTES * 1000, b.GATE_CORPUS_CONTENT_BYTES),
};

/** Integer division, floored, matching the integer arithmetic the Rust tests use.
 * Floating point here would make the document and the tests disagree by one in
 * the last place, which is a false failure and the worst kind. */
function idiv(numerator, denominator) {
  return denominator === 0 ? 0 : Math.floor(numerator / denominator);
}

// ------------------------------------------------------------------ reading

const read = (root, rel) => {
  const path = join(root, rel);
  if (!existsSync(path)) return null;
  // A byte-order mark and CRLF survive a checkout on someone else's machine.
  return readFileSync(path, "utf8").replace(/^﻿/, "").replace(/\r\n/g, "\n");
};

/** Parses a GitHub-flavoured markdown table into arrays of trimmed cells.
 * `heading` is matched against the section title the table sits under. */
function tableUnder(text, heading) {
  const lines = text.split("\n");
  const start = lines.findIndex((line) => line.trim() === heading);
  if (start < 0) return null;
  const rows = [];
  let seenHeader = false;
  for (let i = start + 1; i < lines.length; i += 1) {
    const line = lines[i].trim();
    if (line.startsWith("## ")) break;
    if (!line.startsWith("|")) {
      if (rows.length > 0) break;
      continue;
    }
    const cells = line.slice(1, line.endsWith("|") ? -1 : undefined).split("|").map((c) => c.trim());
    if (cells.every((c) => /^:?-{2,}:?$/.test(c))) continue;
    if (!seenHeader) { seenHeader = true; continue; }
    rows.push(cells);
  }
  return rows;
}

/** `\`NAME\`` → `NAME`; anything else → null. */
const backticked = (cell) => /^`([^`]+)`$/.exec(cell ?? "")?.[1] ?? null;

/** A decimal integer, with `_` or `,` separators tolerated. */
function integer(cell) {
  const text = String(cell ?? "").trim().replace(/[_,]/g, "");
  return /^-?\d+$/.test(text) ? Number(text) : null;
}

/** The register rows of §3, as `{ name, value, unit, file, justification }`. */
export function registerRows(text) {
  const rows = tableUnder(text, "## 3. Budget register") ?? [];
  return rows
    .map((cells) => ({
      name: backticked(cells[0]),
      value: integer(cells[1]),
      unit: cells[2] ?? "",
      file: backticked(cells[3]),
      justification: cells[4] ?? "",
    }))
    .filter((row) => row.name !== null);
}

/** Every `pub const NAME: type = value;` in a Rust source file. */
export function rustConstants(source) {
  const out = new Map();
  const pattern = /pub const ([A-Z0-9_]+)\s*:\s*[A-Za-z0-9_]+\s*=\s*([0-9_]+)\s*;/g;
  for (const match of source.matchAll(pattern)) {
    out.set(match[1], Number(match[2].replace(/_/g, "")));
  }
  return out;
}

// -------------------------------------------------------------------- rules

/** R1 — the page exists and carries the two tables this checker reads. */
function ruleThePageExists(ctx, fail) {
  if (ctx.text === null) {
    fail(PAGE, "is missing", "the storage budget lives here; without it `verify:storage` has nothing to enforce");
    return;
  }
  if (ctx.rows.length === 0) {
    fail(PAGE, "has no budget register under `## 3. Budget register`",
      "every budgeted number is a row naming its constant, its value, its unit, its enforcing file and its justification");
  }
  if (tableUnder(ctx.text, "## 4. The composed headline") === null) {
    fail(PAGE, "has no composed-headline table under `## 4. The composed headline`",
      "the headline must be recomputable from the register, or it is a number with no derivation");
  }
}

/** R2 — every register row names a file that exists and declares the constant
 * with the same value. A document and a constant that disagree are two budgets. */
function ruleTheDocumentAndTheCodeAgree(ctx, fail) {
  for (const row of ctx.rows) {
    if (row.value === null) {
      fail(PAGE, `\`${row.name}\` has a value that is not an integer`,
        "budgets are integers so the document and the test compare exactly; write bytes, counts or per-mille");
      continue;
    }
    if (row.file === null) {
      fail(PAGE, `\`${row.name}\` names no enforcing file`,
        "put the path to the test that asserts it in the `Enforced by` column; a budget nothing enforces is a wish");
      continue;
    }
    const source = read(ctx.root, row.file);
    if (source === null) {
      fail(PAGE, `\`${row.name}\` is enforced by \`${row.file}\`, which does not exist`,
        "point the row at the test that actually asserts the budget");
      continue;
    }
    const declared = rustConstants(source).get(row.name);
    if (declared === undefined) {
      fail(row.file, `does not declare \`pub const ${row.name}\``,
        `either declare it here or remove its row from ${PAGE}; a documented budget with no constant is enforced by nothing`);
      continue;
    }
    if (declared !== row.value) {
      fail(row.file, `declares ${row.name} = ${declared}, and ${PAGE} says ${row.value}`,
        "make the two equal. If the budget genuinely moved, the raise ships with the measurement that justifies it in the same pull request");
    }
  }
}

/** R3 — every registered constant in an enforcing file has a row. The reverse of
 * R2, and the one that catches a budget added to a test and never justified. */
function ruleEveryConstantIsRegistered(ctx, fail) {
  const registered = new Set(ctx.rows.map((row) => row.name));
  for (const file of ctx.files) {
    const source = read(ctx.root, file);
    if (source === null) continue;
    for (const name of rustConstants(source).keys()) {
      if (!REGISTERED.test(name) || registered.has(name)) continue;
      fail(file, `declares \`${name}\`, which has no row in ${PAGE}`,
        "add a row with its value, unit and justification, or rename the constant if it is not a budget");
    }
  }
}

/** R4 — every enforcing file is on the merge path.
 *
 * The defect class this repository keeps shipping: a check that exists and runs
 * nowhere. `verify:rust` runs `cargo nextest run --workspace`, so an enforcing
 * test earns its place only if it sits in `crates/<name>/tests/` of a crate the
 * workspace actually lists. */
function ruleEnforcingTestsAreRun(ctx, fail) {
  const manifest = read(ctx.root, CARGO);
  if (manifest === null) {
    fail(CARGO, "is missing", "the workspace manifest is what puts an enforcing test on the merge path");
    return;
  }
  const members = new Set(
    [...manifest.matchAll(/"(crates\/[A-Za-z0-9_-]+)"/g)].map((m) => m[1]),
  );
  for (const file of ctx.files) {
    const match = /^(crates\/[A-Za-z0-9_-]+)\/tests\/[^/]+\.rs$/.exec(file);
    if (match === null) {
      fail(PAGE, `\`${file}\` is not an integration test of a workspace crate`,
        "an enforcing test must live at crates/<crate>/tests/<name>.rs, or `cargo nextest run --workspace` never runs it");
      continue;
    }
    if (!members.has(match[1])) {
      fail(CARGO, `does not list \`${match[1]}\`, whose test ${file} enforces a storage budget`,
        "add the crate to [workspace].members, or the budget is enforced by a test nothing builds");
    }
  }
}

/** R5 — the composed headline follows from the register. */
function ruleTheHeadlineFollowsFromItsParts(ctx, fail) {
  const rows = tableUnder(ctx.text ?? "", "## 4. The composed headline");
  if (rows === null) return; // R1 already reported it
  const budgets = Object.fromEntries(ctx.rows.map((row) => [row.name, row.value]));
  const stated = new Map(
    rows.map((cells) => [backticked(cells[0]), integer(cells[1])]).filter(([name]) => name !== null),
  );
  for (const [name, compute] of Object.entries(DERIVED)) {
    if (!stated.has(name)) {
      fail(PAGE, `the composed-headline table has no \`${name}\` row`,
        "every derived figure this checker recomputes must be stated, or the recomputation checks nothing");
      continue;
    }
    let expected;
    try {
      expected = compute(budgets);
    } catch {
      expected = null;
    }
    if (expected === null || !Number.isFinite(expected)) {
      fail(PAGE, `\`${name}\` cannot be computed from the register`,
        "the register is missing a budget the headline is derived from");
      continue;
    }
    if (stated.get(name) !== expected) {
      fail(PAGE, `states ${name} = ${stated.get(name)}, and the register computes ${expected}`,
        "recompute the headline from the budgets, or fix the budget that moved; a headline that does not follow from its parts is a number with no method");
    }
  }
}

/** R6 — this checker is on the merge path, and so is every other `verify:*`.
 * Shared with `edition-check.mjs` and `adr-check.mjs` so that dropping any one of
 * them from `verify` is reported by the two that are still wired. */
function ruleOnTheMergePath(ctx, fail) {
  for (const v of wiringViolations(ctx.root)) fail(v.file, v.problem, v.fix);
}

const RULES = [
  ["1 the page exists", ruleThePageExists],
  ["2 document and code agree", ruleTheDocumentAndTheCodeAgree],
  ["3 every constant is registered", ruleEveryConstantIsRegistered],
  ["4 enforcing tests are run", ruleEnforcingTestsAreRun],
  ["5 the headline follows from its parts", ruleTheHeadlineFollowsFromItsParts],
  ["6 on the merge path", ruleOnTheMergePath],
];

/** Run every rule over `root`. Returns the violation list. */
export function check(root) {
  const text = read(root, PAGE);
  const rows = text === null ? [] : registerRows(text);
  const files = [...new Set(rows.map((row) => row.file).filter((file) => file !== null))].sort();
  const ctx = { root, text, rows, files };
  const violations = [];
  for (const [rule, fn] of RULES) {
    fn(ctx, (file, problem, fix) => violations.push({ rule, file, problem, fix }));
  }
  return violations;
}

// ---------------------------------------------------------------- self-test

/** A synthetic repository holding the REAL budget page, the REAL enforcing tests
 * and the REAL manifests. Mutating the page that ships — rather than a fixture —
 * is the point: a parser that only works on a toy proves nothing about the
 * document a lane will actually edit. */
function synthTree(dir) {
  const write = (rel, body) => {
    mkdirSync(join(dir, dirname(rel)), { recursive: true });
    writeFileSync(join(dir, rel), body);
  };
  write(PAGE, read(REPO_ROOT, PAGE) ?? "");
  write("package.json", readFileSync(join(REPO_ROOT, "package.json"), "utf8"));
  write(CARGO, readFileSync(join(REPO_ROOT, CARGO), "utf8"));
  for (const row of registerRows(read(REPO_ROOT, PAGE) ?? "")) {
    if (row.file === null) continue;
    const source = read(REPO_ROOT, row.file);
    if (source !== null) write(row.file, source);
  }
  return dir;
}

/** The enforcing file every mutation that needs one operates on: resolved from
 * the page rather than hardcoded, so renaming a test does not silently unanchor
 * the self-test. */
const subjectFile = (dir) => registerRows(read(dir, PAGE) ?? "")[0]?.file;

/** The register row every value mutation operates on. */
const subjectRow = (dir) => registerRows(read(dir, PAGE) ?? "")[0];

const patchPage = (dir, edit) => {
  const body = readFileSync(join(dir, PAGE), "utf8");
  writeFileSync(join(dir, PAGE), edit(body));
};

const patchManifest = (dir, edit) => {
  const path = join(dir, "package.json");
  const manifest = JSON.parse(readFileSync(path, "utf8"));
  edit(manifest);
  writeFileSync(path, `${JSON.stringify(manifest, null, 2)}\n`);
};

/** One mutation per rule, at least. A rule with no mutation is a rule nobody has
 * ever seen bite. `[rule, label, mutate]`. */
const MUTATIONS = [
  ["1", "the budget page is deleted", (dir) => rmSync(join(dir, PAGE))],
  ["1", "the budget register table is retitled away", (dir) =>
    patchPage(dir, (b) => b.replace("## 3. Budget register", "## 3. Some numbers"))],
  ["2", "the document raises a budget and the test does not", (dir) => {
    const row = subjectRow(dir);
    patchPage(dir, (b) => b.replace(
      new RegExp(`(\\|\\s*\`${row.name}\`\\s*\\|\\s*)${row.value}(\\s*\\|)`),
      `$1${row.value + 1}$2`,
    ));
  }],
  ["2", "the test raises a budget and the document does not", (dir) => {
    const row = subjectRow(dir);
    const path = join(dir, row.file);
    const source = readFileSync(path, "utf8");
    writeFileSync(path, source.replace(
      new RegExp(`(pub const ${row.name}\\s*:\\s*[A-Za-z0-9_]+\\s*=\\s*)[0-9_]+`),
      `$1${row.value + 7}`,
    ));
  }],
  ["2", "a row points at an enforcing file that does not exist", (dir) => {
    const file = subjectFile(dir);
    patchPage(dir, (b) => b.replaceAll(`\`${file}\``, "`crates/mesh-cas/tests/no-such-test.rs`"));
  }],
  ["2", "the constant is deleted from the test that enforced it", (dir) => {
    const row = subjectRow(dir);
    const path = join(dir, row.file);
    writeFileSync(path, readFileSync(path, "utf8").replace(
      new RegExp(`pub const ${row.name}\\s*:\\s*[A-Za-z0-9_]+\\s*=\\s*[0-9_]+\\s*;`),
      "",
    ));
  }],
  ["3", "a budget is added to a test and never justified on the page", (dir) => {
    const path = join(dir, subjectFile(dir));
    writeFileSync(path, `pub const BUDGET_SMUGGLED_IN: u64 = 999;\n${readFileSync(path, "utf8")}`);
  }],
  ["4", "an enforcing test is moved somewhere `cargo nextest run --workspace` never looks", (dir) => {
    const file = subjectFile(dir);
    const moved = "tools/scratch/storage-footprint.rs";
    mkdirSync(join(dir, dirname(moved)), { recursive: true });
    writeFileSync(join(dir, moved), readFileSync(join(dir, file), "utf8"));
    patchPage(dir, (b) => b.replaceAll(`\`${file}\``, `\`${moved}\``));
  }],
  ["4", "the enforcing crate is dropped from the workspace", (dir) => {
    const file = subjectFile(dir);
    const crate = /^(crates\/[A-Za-z0-9_-]+)\//.exec(file)[1];
    const path = join(dir, CARGO);
    writeFileSync(path, readFileSync(path, "utf8").replace(`  "${crate}",\n`, ""));
  }],
  ["5", "the headline is edited without the budgets it is derived from", (dir) =>
    patchPage(dir, (b) => b.replace(
      /(\|\s*`MARGINAL_AMPLIFICATION_PER_MILLE`\s*\|\s*)(\d+)/,
      (_, head, value) => `${head}${Number(value) + 40}`,
    ))],
  ["5", "a budget moves and the headline is left behind", (dir) =>
    patchPage(dir, (b) => b
      .replace(/(\|\s*`BUDGET_INDEX_BYTES_PER_RECORD`\s*\|\s*)320(\s*\|)/, "$1640$2")
      .replace(
        /(pub const BUDGET_INDEX_BYTES_PER_RECORD\s*:\s*u64\s*=\s*)320/,
        "$1640",
      ))],
  ["6", "this check is dropped from `verify`", (dir) =>
    patchManifest(dir, (m) => {
      m.scripts.verify = m.scripts.verify.replace(/\s*&&\s*npm run verify:storage/, "");
    })],
  ["6", "`test` stops invoking `verify`", (dir) =>
    patchManifest(dir, (m) => { m.scripts.test = "npm run preflight"; })],
];

/** Changes the checker must NOT reject. */
const TOLERANCES = [
  ["the page checked out with CRLF line endings and a byte-order mark", (dir) =>
    patchPage(dir, (b) => `﻿${b.replace(/\n/g, "\r\n")}`)],
  ["a budget written with underscore separators in the document", (dir) =>
    patchPage(dir, (b) => b.replace(
      /(\|\s*`BUDGET_INDEX_SCHEMA_FLOOR_BYTES`\s*\|\s*)196608(\s*\|)/, "$1196_608$2",
    ))],
  ["a budget written without underscore separators in the test", (dir) => {
    const path = join(dir, "crates/mesh-store/tests/index-footprint.rs");
    writeFileSync(path, readFileSync(path, "utf8").replace(
      /(pub const BUDGET_INDEX_SCHEMA_FLOOR_BYTES\s*:\s*u64\s*=\s*)196_608/, "$1196608",
    ));
  }],
  ["a non-budget constant added to an enforcing test", (dir) => {
    const path = join(dir, subjectFile(dir));
    writeFileSync(path, `pub const SAMPLE_LABEL_WIDTH: usize = 12;\n${readFileSync(path, "utf8")}`);
  }],
  ["a justification rewritten without touching a number", (dir) =>
    patchPage(dir, (b) => b.replace(
      "The store neither compresses nor frames",
      "The store performs no compression and adds no framing",
    ))],
  ["a `verify:*` script wired into `verify` in a different position", (dir) =>
    patchManifest(dir, (m) => {
      const parts = m.scripts.verify.split(" && ");
      m.scripts.verify = [parts.at(-1), ...parts.slice(0, -1)].join(" && ");
    })],
  ["a `verify:*` script invoked with a flag between `npm` and `run`", (dir) =>
    patchManifest(dir, (m) => {
      m.scripts.verify = m.scripts.verify.replace("npm run verify:storage", "npm --silent run verify:storage");
    })],
];

function selfTest() {
  const base = join(tmpdir(), `mesh-storage-budget-selftest-${process.pid}`);
  rmSync(base, { recursive: true, force: true });
  const failures = [];
  const build = (name) => {
    const dir = join(base, name);
    mkdirSync(dir, { recursive: true });
    return synthTree(dir);
  };
  /** A mutation that changes nothing is a broken anchor, not a passing test — an
   * unanchored `String.replace` silently does nothing, so the tree is compared
   * before and after rather than the mutation's source being pattern-matched. */
  // Raw bytes rather than the normalized read: the CRLF-and-byte-order-mark
  // tolerance is a real change to the tree that `read` deliberately erases, and
  // comparing normalized text would report it as an unanchored no-op.
  const raw = (dir, rel) =>
    existsSync(join(dir, rel)) ? readFileSync(join(dir, rel), "utf8") : "<absent>";
  const snapshot = (dir) => {
    const parts = [raw(dir, PAGE), raw(dir, CARGO), raw(dir, "package.json")];
    for (const row of registerRows(read(REPO_ROOT, PAGE) ?? "")) {
      if (row.file !== null) parts.push(`${row.file}:${raw(dir, row.file)}`);
    }
    return parts.join("|");
  };
  const apply = (dir, mutate) => {
    const before = snapshot(dir);
    mutate(dir);
    if (snapshot(dir) === before) throw new Error("mutation changed nothing in the tree");
  };
  try {
    const clean = check(build("clean"));
    if (clean.length !== 0) {
      failures.push(`the unmutated synthetic tree must pass, got: ${JSON.stringify(clean, null, 2)}`);
    }
    for (const [index, [rule, label, mutate]] of MUTATIONS.entries()) {
      const dir = build(`m${index}-rule${rule}`); // indexed: one rule may have several mutations
      try {
        apply(dir, mutate);
      } catch (err) {
        failures.push(`could not apply "${label}": ${err.message}`);
        continue;
      }
      // Exact id match: a prefix match would let one rule stand in for a dead one.
      const caught = check(dir).filter((v) => v.rule.split(" ")[0] === rule);
      if (caught.length === 0) failures.push(`rule ${rule} did not reject: ${label}`);
      else process.stdout.write(`  rejected  rule ${rule}  ${label}\n`);
    }
    for (const [label, mutate] of TOLERANCES) {
      const dir = build(`tolerance-${label.replace(/\W+/g, "-").slice(0, 40)}`);
      try {
        apply(dir, mutate);
      } catch (err) {
        failures.push(`could not apply tolerance "${label}": ${err.message}`);
        continue;
      }
      const violations = check(dir);
      if (violations.length > 0) failures.push(`must still accept ${label}: ${JSON.stringify(violations, null, 2)}`);
      else process.stdout.write(`  accepted  --      ${label}\n`);
    }
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
  if (failures.length > 0) {
    process.stderr.write(`\nself-test FAILED\n${failures.map((f) => `  ${f}\n`).join("")}`);
    return finish(1);
  }
  process.stdout.write(
    `\nself-test PASS — ${MUTATIONS.length} mutations rejected, ${TOLERANCES.length} tolerances accepted, clean tree accepted\n`,
  );
}

// -------------------------------------------------------------------- entry

/** Set the verdict and let node exit on its own — NOT `process.exit(code)`.
 *
 * `process.exit()` under Node v24.7.0 on `aarch64-apple-darwin` kills this
 * process with `SIGSEGV` on roughly a quarter of runs. The full mechanism, the
 * crash stack and the control measurements are in `invocation-check.mjs` beside
 * rule 6, which fails if this shape comes back. */
const finish = (code) => { process.exitCode = code; };

function main() {
  const flags = process.argv.slice(2).filter((arg) => arg.startsWith("-"));
  for (const flag of flags) {
    if (flag !== "--self-test") {
      process.stderr.write(`unknown flag ${flag}\nusage: node tools/program/storage-budget/check.mjs [--self-test]\n`);
      return finish(2);
    }
  }
  if (flags.includes("--self-test")) return selfTest();

  const violations = check(REPO_ROOT);
  const rows = registerRows(read(REPO_ROOT, PAGE) ?? "");
  if (violations.length === 0) {
    process.stdout.write(
      `storage-budget PASS — ${RULES.length} rules over ${rows.length} budgets in ${new Set(rows.map((r) => r.file)).size} enforcing test(s)\n`,
    );
    return;
  }
  process.stderr.write(`storage-budget FAIL — ${violations.length} violation(s)\n\n`);
  for (const v of violations) {
    process.stderr.write(`  ${v.file}\n    rule ${v.rule}: ${v.problem}\n    fix: ${v.fix}\n\n`);
  }
  finish(1);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
