// tools/program/npm-wiring.mjs — the rule that keeps a checker from becoming an
// orphan (task 01KZCXY9BHHRNM4CYAW0J1N2NT).
//
// `verify:editions` was defined in `package.json` for a day and invoked by
// nothing. It was failing that whole day and `npm test` stayed green, because
// `verify` never named it. A checker nobody runs is indistinguishable from a
// checker that does not exist, and the difference is invisible in review: the
// script is right there in the file.
//
// This module states the general rule rather than that one instance — **every
// `verify:*` script is invoked by `verify`, and `test` invokes `verify`** — so
// the next checker cannot be added and left unwired either.
//
// It is imported by two checkers on purpose. Whichever one `verify` still runs
// reports that the other has been dropped, so removing a single script from the
// chain is caught rather than silently disabling the thing that would catch it.
// That guard is against drift, not against intent: a diff that removes every
// caller removes the rule with them, and no in-repo check can do better than
// making that a visible, deliberate edit rather than a one-word omission.

import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

/** The manifest whose script graph is the merge path. */
export const MANIFEST = "package.json";

/** The script every lane runs, and the script it must reach. */
const ENTRY = "test";
const CHAIN = "verify";

/** Prefix marking a script that is a member of the chain rather than a leaf
 * utility. `verify:rust` is in; `portal` and `gates` are not, because they are
 * commands a human invokes, not checks a merge depends on. */
const MEMBER = `${CHAIN}:`;

/** The package managers whose invocations count as reaching a script. Accepting
 * all three keeps the rule alive across a package-manager change rather than
 * silently passing after one. */
const RUNNERS = new Set(["npm", "pnpm", "yarn", "npx"]);

/** Every script name invoked by a script body: `npm run <name>`, `yarn <name>`,
 * and the same with flags in any position.
 *
 * Tokenised rather than matched with one regex because the regex form read
 * `npm --silent run verify:editions` as an invocation of `--silent` and then
 * reported `verify:editions` as unreachable — a wrong message that would send a
 * lane hunting for a wiring bug that was not there. Found by the verification
 * pass over this file's own first version, and pinned by a tolerance. */
export function invocations(script) {
  const names = new Set();
  const tokens = (script ?? "").split(/\s+/).filter(Boolean);
  for (let i = 0; i < tokens.length; i += 1) {
    if (!RUNNERS.has(tokens[i])) continue;
    let j = i + 1;
    while (j < tokens.length && (tokens[j].startsWith("-") || tokens[j] === "run")) j += 1;
    if (j < tokens.length && /^[A-Za-z0-9:_-]+$/.test(tokens[j])) names.add(tokens[j]);
  }
  return names;
}

/** Read and parse the manifest. Returns `null` when it is absent or unparseable;
 * the caller reports that as the violation it is, rather than skipping. */
export function readManifest(root) {
  const path = join(root, MANIFEST);
  if (!existsSync(path)) return null;
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return null;
  }
}

/** Violations of the wiring rule over `root`, as `{ file, problem, fix }`. */
export function wiringViolations(root) {
  const out = [];
  const fail = (problem, fix) => out.push({ file: MANIFEST, problem, fix });

  const manifest = readManifest(root);
  if (manifest === null) {
    fail("is missing or is not valid JSON", "the script graph in this file is what puts a checker on the merge path");
    return out;
  }
  const scripts = manifest.scripts ?? {};

  if (!(ENTRY in scripts)) {
    fail(`has no \`${ENTRY}\` script`, `\`npm ${ENTRY}\` is the command every lane runs before pushing; it must exist`);
    return out;
  }
  if (!(CHAIN in scripts)) {
    fail(`has no \`${CHAIN}\` script`, `\`${ENTRY}\` reaches every checker through \`${CHAIN}\`; restore it`);
    return out;
  }
  if (!invocations(scripts[ENTRY]).has(CHAIN)) {
    fail(`\`${ENTRY}\` does not invoke \`${CHAIN}\``,
      `write \`npm run ${CHAIN}\` into \`${ENTRY}\`; without it every check below is defined and unreachable`);
  }

  const reached = invocations(scripts[CHAIN]);
  for (const name of Object.keys(scripts).sort()) {
    if (!name.startsWith(MEMBER) || reached.has(name)) continue;
    fail(`\`${name}\` is defined and \`${CHAIN}\` never invokes it`,
      `add \`npm run ${name}\` to \`${CHAIN}\`, or delete the script — an unwired checker is a checker that does not run`);
  }
  return out;
}
