#!/usr/bin/env node
// tools/program/license-check.mjs — the licence boundary as a program rather
// than a paragraph (task 01KZC23NED0RVR69QKFXCQKXWY).
//
// Mesh does not use per-file licence headers. The licence attaches at two
// places that a machine can read: the verbatim Apache-2.0 text at `LICENSE`,
// and the `license` field of every package manifest in the community surface.
// This checker asserts both, asserts that nothing contradicts them, and asserts
// that the paid surface stays outside the Apache-2.0 workspace.
//
// Contract, deliberately:
//  - Zero network, zero dependencies, no build step, well under a second.
//  - Exit code is the verdict. Every failure names the file and the fix.
//  - `--self-test` breaks a synthetic tree and asserts every break is REJECTED,
//    then applies the tolerances and asserts each is ACCEPTED. A check only ever
//    observed to pass is not evidence that it can fail, and one that rejects
//    everything is no more useful than one that rejects nothing.

import { existsSync, readFileSync, readdirSync, statSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join, relative, dirname, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";

const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");

/** sha256 of the full Apache-2.0 text, appendix included, 201 lines.
 * Cross-checked byte-for-byte against the copies shipped by three independent
 * crates.io packages (regex 1.13.1, log 0.4.29, crossbeam-utils 0.8.21), which
 * all agree on this digest. Changing this constant is a licence change and
 * belongs in a decision doc, never in a passing-the-build edit. */
const APACHE_2_0_SHA256 = "a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2";

/** Structural landmarks that must survive independently of the digest, so that
 * editing LICENSE *and* the digest together still fails. */
const APACHE_2_0_LANDMARKS = [
  "                              Apache License",
  "                        Version 2.0, January 2004",
  "END OF TERMS AND CONDITIONS",
  "APPENDIX: How to apply the Apache License to your work.",
  "9. Accepting Warranty or Additional Liability. While redistributing",
];

const SPDX_ID = "Apache-2.0";

/** Assembled at runtime so this file does not itself contain the literal tag it
 * scans for. That is what lets R5 apply to the checker too, with no
 * self-exemption for anyone to hide a header behind. */
const SPDX_TAG = ["SPDX", "License", "Identifier"].join("-");

/** Licences that may never enter the dependency graph of an Apache-2.0 client.
 * A dependency carrying one of these is replaced, never waived (task
 * "## Failure and recovery"). */
const COPYLEFT_DENY = [
  "GPL-2.0", "GPL-3.0", "AGPL-3.0", "LGPL-2.1", "LGPL-3.0",
  "MPL-2.0", "SSPL-1.0", "EUPL-1.2", "CDDL-1.0", "CPAL-1.0", "OSL-3.0", "BUSL-1.1",
];

/** Directories never walked: build output, third-party trees, disposable cache. */
const SKIP_DIRS = new Set([".git", "target", "node_modules", "vendor", "graphify-out"]);

/** The declaration files inside `vendor/`. They describe the redistributed
 * artifacts; they are not themselves redistributed artifacts, so R10-R13 must
 * not demand that they declare themselves. */
const VENDOR_MANIFEST = "VENDORED.json";
const VENDOR_OFFER = "SOURCE-OFFER.md";
const VENDOR_META = new Set([VENDOR_MANIFEST, VENDOR_OFFER]);

/** How a vendored artifact is redistributed. Exactly the three options decision
 * 01KZCN3843P20QTNEZ3XDF9K00 weighed; a fourth is a decision, not a value. */
const VENDOR_ROUTES = new Set([
  "ship-with-source-offer",          // the archive stays, corresponding-source directions beside it
  "registry-dependency",             // the archive is gone; a package registry conveys it instead
  "removed-with-documented-install", // the archive is gone; the README documents how to obtain it
]);

/** The only route compatible with the artifact still being present in the tree.
 * The other two both assert the archive was removed, so finding it on disk means
 * the declared route is not the one the repository actually takes. */
const ROUTE_KEEPS_ARTIFACT = "ship-with-source-offer";

/** Files that must exist and carry real content for the repository to be
 * publishable. The byte floor only asserts "not a stub"; it does not read. */
const CONTRIBUTION_SURFACE = [
  { path: "CONTRIBUTING.md", minBytes: 2000 },
  { path: ".github/CODE_OF_CONDUCT.md", minBytes: 1500 },
  { path: ".github/SECURITY.md", minBytes: 1000 },
  { path: ".github/CODEOWNERS", minBytes: 200 },
];

// ---------------------------------------------------------------- utilities

const read = (root, rel) => readFileSync(join(root, rel), "utf8");
const has = (root, rel) => existsSync(join(root, rel));

/** Every file under `root`, repo-relative, skipping SKIP_DIRS. Sorted, so two
 * runs on the same tree produce identical output. */
function walk(root, dir = root, acc = []) {
  for (const name of readdirSync(dir).sort()) {
    if (SKIP_DIRS.has(name)) continue;
    const abs = join(dir, name);
    if (statSync(abs).isDirectory()) walk(root, abs, acc);
    else acc.push(relative(root, abs).split(sep).join("/"));
  }
  return acc;
}

/** The body of a named TOML table: every line after `[table]` up to the next
 * table header. Line-based on purpose — a regex spanning `[…]` headers reads
 * array brackets as table starts, which is how the first draft silently
 * returned null for `[workspace.package]`. */
function tomlTable(text, table) {
  const lines = text.split("\n");
  const start = lines.findIndex((l) => l.trim() === `[${table}]`);
  if (start === -1) return null;
  const body = [];
  for (const line of lines.slice(start + 1)) {
    if (/^\s*\[[^\]]+\]\s*$/.test(line)) break;
    body.push(line);
  }
  return body.join("\n");
}

/** Value of a `key = "…"` assignment inside a named TOML table. */
function tomlTableValue(text, table, key) {
  const body = tomlTable(text, table);
  if (body === null) return null;
  const hit = body.match(new RegExp(`^\\s*${key}\\s*=\\s*"([^"]*)"`, "m"));
  return hit ? hit[1] : null;
}

/** The `members = [...]` list of the `[workspace]` table. */
function workspaceMembers(text) {
  const body = tomlTable(text, "workspace");
  if (body === null) return [];
  const list = body.match(/members\s*=\s*\[([\s\S]*?)\]/);
  return list ? [...list[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]) : [];
}

/** Which COPYLEFT_DENY family an identifier belongs to, or null.
 * Family match, never equality: `AGPL-3.0-only` and `AGPL-3.0-or-later` are the
 * canonical modern spellings, so `=== "AGPL-3.0"` waves both through. Any
 * `WITH <exception>` suffix is stripped first — the exception narrows the
 * copyleft, it does not stop the identifier naming it. */
function copyleftFamily(identifier) {
  const bare = String(identifier).split(" ")[0];
  return COPYLEFT_DENY.find((b) => bare === b || bare.startsWith(`${b}-`)) ?? null;
}

/** Every file under `vendor/`, repo-relative, recursively, excluding the
 * declaration files. Recursive on purpose: a top-level-only listing lets
 * `vendor/nested/thing.tgz` be redistributed without ever being declared. */
function vendoredArtifacts(root) {
  const out = [];
  const descend = (dir) => {
    for (const name of readdirSync(dir).sort()) {
      if (name.startsWith(".")) continue;
      const abs = join(dir, name);
      if (statSync(abs).isDirectory()) descend(abs);
      else if (!VENDOR_META.has(name) || dir !== join(root, "vendor")) {
        out.push(relative(root, abs).split(sep).join("/"));
      }
    }
  };
  descend(join(root, "vendor"));
  return out;
}

const sha256File = (abs) => createHash("sha256").update(readFileSync(abs)).digest("hex");

/** How a crate manifest declares its licence: inherited, literal, or absent. */
function manifestLicence(text) {
  if (/^\s*license\.workspace\s*=\s*true\s*$/m.test(text)) return { mode: "workspace" };
  const literal = text.match(/^\s*license\s*=\s*"([^"]*)"/m);
  return literal ? { mode: "literal", value: literal[1] } : { mode: "absent" };
}

// -------------------------------------------------------------------- rules

/** R1 — LICENSE is the actual Apache-2.0 text, not a pointer to one. */
function ruleLicenceFile(root, fail) {
  if (!has(root, "LICENSE")) {
    return fail("LICENSE", "no LICENSE file", "add the full Apache-2.0 text at LICENSE");
  }
  const text = read(root, "LICENSE");
  // Hash the normalized content, not the bytes on disk. This repository has no
  // .gitattributes, so a Windows clone with Git's default `core.autocrlf=true`
  // checks LICENSE out with CRLF — an unnormalized digest would fail for every
  // Windows contributor, and Linux CI would never see it.
  const digest = createHash("sha256").update(text.replace(/^﻿/, "").replace(/\r\n/g, "\n")).digest("hex");
  if (digest !== APACHE_2_0_SHA256) {
    fail("LICENSE", `sha256 is ${digest}, expected ${APACHE_2_0_SHA256}`,
      "restore the verbatim Apache-2.0 text; a licence edit is a decision doc, not a build fix");
  }
  for (const landmark of APACHE_2_0_LANDMARKS) {
    if (!text.includes(landmark)) {
      fail("LICENSE", `missing Apache-2.0 landmark: ${JSON.stringify(landmark.trim().slice(0, 48))}`,
        "LICENSE must be the complete licence text including the appendix");
    }
  }
}

/** R2 — every workspace crate resolves to Apache-2.0. */
function ruleCrateManifests(root, fail) {
  if (!has(root, "Cargo.toml")) return;
  const rootToml = read(root, "Cargo.toml");
  const inherited = tomlTableValue(rootToml, "workspace.package", "license");
  if (inherited !== SPDX_ID) {
    fail("Cargo.toml", `[workspace.package] license = ${JSON.stringify(inherited)}`,
      `set license = "${SPDX_ID}" so every member inherits the community licence`);
  }
  for (const member of workspaceMembers(rootToml)) {
    const rel = `${member}/Cargo.toml`;
    if (!has(root, rel)) {
      fail(rel, `workspace member "${member}" has no manifest`, "add the manifest or drop the member");
      continue;
    }
    const declared = manifestLicence(read(root, rel));
    if (declared.mode === "workspace" && inherited === SPDX_ID) continue;
    if (declared.mode === "literal" && declared.value === SPDX_ID) continue;
    const seen = declared.mode === "absent" ? "no license field" : `license = ${JSON.stringify(declared.value)}`;
    fail(rel, seen, `add license.workspace = true (or license = "${SPDX_ID}")`);
  }
}

/** R3 — the paid surface is never inside the Apache-2.0 workspace. */
function rulePaidSurfaceOutsideWorkspace(root, fail) {
  if (!has(root, "Cargo.toml")) return;
  for (const member of workspaceMembers(read(root, "Cargo.toml"))) {
    if (member === "services" || member.startsWith("services/")) {
      fail("Cargo.toml", `workspace member "${member}" is under services/`,
        "the operated services are not Apache-2.0; they must not inherit the workspace licence");
    }
  }
}

/** R4 — every directory under services/ is named in the README licence section. */
function rulePaidSurfaceDocumented(root, fail) {
  if (!has(root, "services") || !has(root, "README.md")) return;
  const readme = read(root, "README.md");
  for (const name of readdirSync(join(root, "services")).sort()) {
    if (!statSync(join(root, "services", name)).isDirectory()) continue;
    if (!readme.includes(`services/${name}`)) {
      fail("README.md", `services/${name} is not named in README.md`,
        "every paid surface is enumerated where the licence boundary is stated");
    }
  }
}

/** R5 — no file in the community surface claims a licence other than Apache-2.0. */
function ruleNoContradictingHeader(root, fail) {
  for (const rel of walk(root)) {
    if (rel.startsWith("services/")) continue; // the paid surface is not Apache-2.0
    if (rel === "LICENSE") continue; // the licence text quotes its own appendix
    let text;
    try {
      text = readFileSync(join(root, rel), "utf8");
    } catch {
      continue; // unreadable — nothing to claim a licence with
    }
    for (const m of text.matchAll(new RegExp(`${SPDX_TAG}:\\s*([^\\s*/]+)`, "g"))) {
      if (m[1] !== SPDX_ID) {
        fail(rel, `declares ${SPDX_TAG}: ${m[1]}`,
          `the community surface is ${SPDX_ID}; remove the header or correct it`);
      }
    }
  }
}

/** R6 — the Node manifest agrees with LICENSE. */
function ruleNodeManifest(root, fail) {
  if (!has(root, "package.json")) return;
  let pkg;
  try {
    pkg = JSON.parse(read(root, "package.json"));
  } catch (err) {
    return fail("package.json", `unparseable: ${err.message}`, "fix the JSON");
  }
  if (pkg.license !== SPDX_ID) {
    fail("package.json", `license = ${JSON.stringify(pkg.license ?? null)}`,
      `set "license": "${SPDX_ID}" to match LICENSE`);
  }
}

/** R7 — cargo-deny carries a real dependency licence policy. */
function ruleDependencyPolicy(root, fail) {
  if (!has(root, "deny.toml")) {
    return fail("deny.toml", "no deny.toml", "add the cargo-deny dependency licence policy");
  }
  const body = tomlTable(read(root, "deny.toml"), "licenses");
  if (body === null) {
    return fail("deny.toml", "no [licenses] table", "declare the allow-list cargo-deny enforces");
  }
  const allow = [...(body.match(/allow\s*=\s*\[([\s\S]*?)\]/)?.[1] ?? "").matchAll(/"([^"]+)"/g)].map((m) => m[1]);
  if (!allow.includes(SPDX_ID)) {
    fail("deny.toml", `[licenses] allow does not include ${SPDX_ID}`,
      "the client's own licence must be allowed for its own crates");
  }
  for (const entry of allow) {
    if (copyleftFamily(entry)) {
      fail("deny.toml", `[licenses] allow contains ${entry}`,
        "a copyleft dependency is a licensing incident, not a warning — replace it, never waive it");
    }
  }
}

/** R8 — the contribution surface exists and is not a stub. */
function ruleContributionSurface(root, fail) {
  for (const { path, minBytes } of CONTRIBUTION_SURFACE) {
    if (!has(root, path)) {
      fail(path, "missing", "the repository cannot be made public without it");
      continue;
    }
    const size = Buffer.byteLength(read(root, path));
    if (size < minBytes) {
      fail(path, `${size} bytes, below the ${minBytes}-byte floor`, "write the real document, not a placeholder");
    }
  }
  if (has(root, "CONTRIBUTING.md")) {
    const text = read(root, "CONTRIBUTING.md");
    if (!/human review(er)?/i.test(text)) {
      fail("CONTRIBUTING.md", "does not say what requires a human reviewer",
        "the external contribution path must state which changes a human must review");
    }
  }
}

/** R9 — the README states the licence rather than promising one. */
function ruleReadmeClaim(root, fail) {
  if (!has(root, "README.md")) return;
  const text = read(root, "README.md");
  if (/not yet licensed/i.test(text)) {
    fail("README.md", 'still says "Not yet licensed"', "the repository is licensed; state the licence");
  }
  if (!text.includes(SPDX_ID) || !text.includes("LICENSE")) {
    fail("README.md", `does not name both ${SPDX_ID} and LICENSE`,
      "a clean clone learns the licence from the README alone");
  }
}

/** Any licence identifier a declaration line could plausibly carry. Deliberately
 * permissive: R10 asserts that a licence was *named*, not which one is allowed. */
const LICENCE_IDENTIFIER = /\b(?:AGPL|LGPL|GPL|MPL|EPL|CDDL|BSD|MIT|ISC|Zlib|Unlicense|Apache|Proprietary)[-\w.+]*/;

/** R10 — every vendored third-party artifact is declared with its licence.
 * Vendoring a blob is redistributing it; an undeclared one is the licensing
 * incident that only surfaces after the repository is public. */
function ruleVendoredArtifactsDeclared(root, fail) {
  if (!has(root, "vendor") || !has(root, "README.md")) return;
  const lines = read(root, "README.md").split("\n");
  for (const rel of vendoredArtifacts(root)) {
    // Every mention counts, not the first: an artifact may be referenced in
    // prose elsewhere, and the declaration is whichever line carries a licence.
    const mentions = lines.filter((line) => line.includes(rel));
    if (mentions.length === 0) {
      fail("README.md", `${rel} is redistributed but never declared`,
        "name the artifact and its licence in the licence-region table");
    } else if (!mentions.some((line) => LICENCE_IDENTIFIER.test(line))) {
      fail("README.md", `${rel} is mentioned but no mention states a licence`,
        "state the artifact's licence on the same line, so the declaration is readable as one");
    }
  }
}

/** Parse `vendor/VENDORED.json` once for R11-R13. Returns null and reports the
 * problem if it cannot be read — the three rules then have nothing to say, which
 * is correct: one missing manifest should not produce three identical failures. */
function loadVendorManifest(root, fail) {
  const rel = `vendor/${VENDOR_MANIFEST}`;
  if (!has(root, rel)) {
    // An empty vendor/ redistributes nothing, so it owes no declaration. The
    // manifest is only mandatory once there is something to declare.
    if (vendoredArtifacts(root).length > 0) {
      fail(rel, "vendor/ redistributes artifacts but carries no declaration",
        "declare every redistributed artifact, its licence and its source route");
    }
    return null;
  }
  try {
    const parsed = JSON.parse(read(root, rel));
    if (!Array.isArray(parsed.artifacts)) {
      fail(rel, "no `artifacts` array", "declare each redistributed artifact as an entry");
      return null;
    }
    return parsed;
  } catch (err) {
    fail(rel, `unparseable: ${err.message}`, "fix the JSON");
    return null;
  }
}

/** R11 — every redistributed artifact is declared, pinned to its bytes, and the
 * declaration agrees with the README. Three surfaces, one fact: the file on
 * disk, the machine-readable declaration, and the human-readable table. Any two
 * of them drifting apart is the failure this rule exists to catch. */
function ruleVendoredArtifactsPinned(root, fail) {
  if (!has(root, "vendor")) return;
  const rel = `vendor/${VENDOR_MANIFEST}`;
  const manifest = loadVendorManifest(root, fail);
  if (!manifest) return;

  const declared = new Map(manifest.artifacts.map((a) => [a.path, a]));
  const onDisk = vendoredArtifacts(root);

  for (const path of onDisk) {
    if (!declared.has(path)) {
      fail(rel, `${path} is redistributed but not declared`,
        "add an entry naming its licence, its form and how a recipient reaches its source");
    }
  }
  for (const entry of manifest.artifacts) {
    if (typeof entry.path !== "string" || !has(root, entry.path)) {
      fail(rel, `declares ${JSON.stringify(entry.path ?? null)}, which is not in the tree`,
        "drop the entry, or restore the artifact it describes");
      continue;
    }
    const actual = sha256File(join(root, entry.path));
    if (entry.sha256 !== actual) {
      fail(rel, `${entry.path} sha256 is ${actual}, declared ${entry.sha256}`,
        "a replaced artifact is a new redistribution — re-declare it, and re-verify its source route");
    }
    if (!VENDOR_ROUTES.has(entry.route)) {
      fail(rel, `${entry.path} route = ${JSON.stringify(entry.route ?? null)}`,
        `route must be one of: ${[...VENDOR_ROUTES].join(", ")}`);
    } else if (entry.route !== ROUTE_KEEPS_ARTIFACT) {
      fail(rel, `${entry.path} declares route "${entry.route}", but the archive is still in the tree`,
        `that route promises the archive was removed; either remove it or declare "${ROUTE_KEEPS_ARTIFACT}"`);
    }
    if (typeof entry.license !== "string" || entry.license.length === 0) {
      fail(rel, `${entry.path} declares no licence`, "state the artifact's licence identifier");
    } else if (has(root, "README.md")) {
      const mentions = read(root, "README.md").split("\n").filter((l) => l.includes(entry.path));
      if (mentions.length > 0 && !mentions.some((l) => l.includes(entry.license))) {
        fail("README.md", `${entry.path} is declared ${entry.license} in ${rel}, but no README line saying so`,
          "the table a human reads and the file a machine reads must name the same licence");
      }
    }
  }
}

/** R12 — a copyleft artifact carries a source route a stranger can actually
 * take. Redistributing built copyleft code obliges whoever conveys it to hand
 * over the corresponding source; an offer nobody can act on is worse than no
 * offer, because it is a promise made to everyone who clones the repository. */
function ruleCopyleftSourceRoute(root, fail) {
  if (!has(root, "vendor")) return;
  const rel = `vendor/${VENDOR_MANIFEST}`;
  const manifest = loadVendorManifest(root, () => {}); // R11 already reported it
  if (!manifest) return;

  for (const entry of manifest.artifacts) {
    if (!copyleftFamily(entry.license ?? "")) continue;
    const src = entry.source;
    if (!src || typeof src !== "object") {
      fail(rel, `${entry.path} is ${entry.license} with no source route`,
        "name the clause relied on and the place a recipient obtains the corresponding source");
      continue;
    }
    for (const field of ["clause", "url"]) {
      if (typeof src[field] !== "string" || src[field].length === 0) {
        fail(rel, `${entry.path} source.${field} is empty`,
          `a copyleft artifact must state its ${field === "url" ? "source location" : "licence clause"}`);
      }
    }
    if (typeof src.publicly_reachable !== "boolean") {
      fail(rel, `${entry.path} source.publicly_reachable is not a boolean`,
        "record whether the source is reachable with no account: true or false, never absent");
    }
    // The reachability check is bound to the bytes it was run against, so
    // replacing the archive silently expires the verification instead of
    // letting a stale "we checked once" carry new object code.
    const checkedAgainst = src.checked?.artifact_sha256;
    if (checkedAgainst !== entry.sha256) {
      fail(rel, `${entry.path} source check was run against ${JSON.stringify(checkedAgainst ?? null)}, not ${entry.sha256}`,
        "re-verify the source route for the artifact actually shipped, and record the digest you verified");
    }
    if (typeof src.checked?.on !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(src.checked.on)) {
      fail(rel, `${entry.path} source check has no ISO date`, "record when the route was last verified");
    }
    if (entry.route === ROUTE_KEEPS_ARTIFACT) {
      const offerRel = entry.offer ?? `vendor/${VENDOR_OFFER}`;
      if (!has(root, offerRel)) {
        fail(rel, `${entry.path} promises directions at ${offerRel}, which does not exist`,
          "the directions sit next to the object code; that placement is the whole point of this route");
        continue;
      }
      const offer = read(root, offerRel);
      if (!offer.includes(entry.path)) {
        fail(offerRel, `does not name ${entry.path}`, "the directions must say which artifact they are for");
      }
      if (typeof src.url === "string" && src.url.length > 0 && !offer.includes(src.url)) {
        fail(offerRel, `does not name the declared source location ${src.url}`,
          "the machine-readable route and the human-readable directions must point at the same place");
      }
    }
  }
}

/** R13 — the publication gate is recomputed, never merely asserted.
 * The declared state is compared against the state derived from the artifacts,
 * so it cannot be opened by editing one word: clearing it requires every
 * copyleft source to be declared reachable AND pinned to a revision, which is a
 * claim a reviewer sees in the diff rather than a default nobody re-reads. */
function rulePublicationGate(root, fail) {
  if (!has(root, "vendor")) return;
  const rel = `vendor/${VENDOR_MANIFEST}`;
  const manifest = loadVendorManifest(root, () => {}); // R11 already reported it
  if (!manifest) return;

  const blocking = manifest.artifacts
    .filter((entry) => {
      if (!copyleftFamily(entry.license ?? "")) return false;
      const src = entry.source ?? {};
      const pinned = typeof src.revision === "string" && src.revision.length > 0;
      return src.publicly_reachable !== true || !pinned;
    })
    .map((entry) => entry.path)
    .sort();

  const expected = blocking.length > 0 ? "blocked" : "cleared";
  const declared = manifest.publication ?? {};
  if (declared.state !== expected) {
    fail(rel, `publication.state = ${JSON.stringify(declared.state ?? null)}, derived state is ${JSON.stringify(expected)}`,
      expected === "blocked"
        ? "a copyleft source that is not publicly reachable and pinned blocks publication; say so"
        : 'every copyleft source is reachable and pinned — set publication.state to "cleared"');
  }
  const listed = Array.isArray(declared.blocked_by) ? [...declared.blocked_by].sort() : null;
  // Compared as JSON rather than as a joined string: ["a b"] and ["a", "b"]
  // join to the same text, so a path containing a space would let two
  // different lists compare equal. It also keeps a control byte out of a
  // source file: the earlier form separated with a NUL, which made `grep`
  // treat this checker as a binary blob and silently match nothing in it.
  if (listed === null || JSON.stringify(listed) !== JSON.stringify(blocking)) {
    fail(rel, `publication.blocked_by = ${JSON.stringify(declared.blocked_by ?? null)}, derived ${JSON.stringify(blocking)}`,
      "list exactly the artifacts whose source route is not yet satisfiable");
  }

  // Discoverability: a stranger reading only the README must be able to find
  // both the directions and the reasoning behind them.
  if (!has(root, "README.md")) return;
  const readme = read(root, "README.md");
  const offers = new Set(
    manifest.artifacts
      .filter((entry) => copyleftFamily(entry.license ?? "") && entry.route === ROUTE_KEEPS_ARTIFACT)
      .map((entry) => entry.offer ?? `vendor/${VENDOR_OFFER}`),
  );
  for (const offer of offers) {
    if (!readme.includes(offer)) {
      fail("README.md", `does not point at ${offer}`,
        "a clean clone finds the corresponding-source directions from the README alone");
    }
  }
  if (offers.size > 0 && typeof manifest.decision === "string" && !readme.includes(manifest.decision)) {
    fail("README.md", `does not name the decision ${manifest.decision} that set this posture`,
      "the reasoning is part of the declaration; link it where the licence regions are stated");
  }
}

const RULES = [
  ["R1 licence file is verbatim", ruleLicenceFile],
  ["R2 crate manifests resolve to Apache-2.0", ruleCrateManifests],
  ["R3 paid surface outside the workspace", rulePaidSurfaceOutsideWorkspace],
  ["R4 paid surface documented", rulePaidSurfaceDocumented],
  ["R5 no contradicting licence header", ruleNoContradictingHeader],
  ["R6 node manifest agrees", ruleNodeManifest],
  ["R7 dependency licence policy", ruleDependencyPolicy],
  ["R8 contribution surface", ruleContributionSurface],
  ["R9 readme states the licence", ruleReadmeClaim],
  ["R10 vendored artifacts declared", ruleVendoredArtifactsDeclared],
  ["R11 vendored artifacts pinned", ruleVendoredArtifactsPinned],
  ["R12 copyleft source route", ruleCopyleftSourceRoute],
  ["R13 publication gate", rulePublicationGate],
];

/** Run every rule over `root`. Returns the violation list; never throws for a
 * policy failure, only for an unreadable tree. */
export function check(root) {
  const violations = [];
  for (const [rule, fn] of RULES) {
    fn(root, (file, problem, fix) => violations.push({ rule, file, problem, fix }));
  }
  return violations;
}

// ---------------------------------------------------------------- self-test

/** A minimal tree that passes every rule, so each mutation isolates one rule. */
function synthTree(dir) {
  const write = (rel, body) => {
    mkdirSync(join(dir, dirname(rel)), { recursive: true });
    writeFileSync(join(dir, rel), body);
  };
  write("LICENSE", readFileSync(join(REPO_ROOT, "LICENSE"), "utf8"));
  write("Cargo.toml", `[workspace]\nresolver = "2"\nmembers = [\n  "crates/a",\n]\n\n[workspace.package]\nlicense = "${SPDX_ID}"\n`);
  write("crates/a/Cargo.toml", `[package]\nname = "a"\nlicense.workspace = true\n`);
  write("crates/a/src/lib.rs", "pub const A: &str = \"a\";\n");
  write("services/relay/README.md", "# services/relay\n");
  write("package.json", JSON.stringify({ name: "synth", license: SPDX_ID }, null, 2));
  write("deny.toml", `[licenses]\nversion = 2\nallow = [\n  "${SPDX_ID}",\n  "MIT",\n]\n`);
  // Two vendored artifacts, on purpose: a permissive one that owes no source
  // route, and a copyleft one that owes the whole of R12 and R13. One of each is
  // what makes "must reject" and "must still accept" separable.
  write("vendor/tool.tgz", "not really a tarball\n");
  write("vendor/copyleft.tgz", "not really a copyleft tarball\n");
  const digest = (rel) => sha256File(join(dir, rel));
  write("README.md",
    `# synth\n\nLicensed under ${SPDX_ID}; see LICENSE. Paid: services/relay.\n\n` +
    `| vendor/tool.tgz | MIT | a vendored tool |\n` +
    `| vendor/copyleft.tgz | AGPL-3.0-or-later | a vendored copyleft tool |\n\n` +
    `Directions: vendor/${VENDOR_OFFER}. Decision: 01SYNTHDECISIONULID00000000.\n`);
  write(`vendor/${VENDOR_OFFER}`,
    `# corresponding source\n\nvendor/copyleft.tgz — https://example.invalid/copyleft at tag v1.\n`);
  write(`vendor/${VENDOR_MANIFEST}`, JSON.stringify({
    decision: "01SYNTHDECISIONULID00000000",
    publication: { state: "cleared", blocked_by: [] },
    artifacts: [
      {
        path: "vendor/tool.tgz",
        sha256: digest("vendor/tool.tgz"),
        license: "MIT",
        form: "object-code",
        route: ROUTE_KEEPS_ARTIFACT,
      },
      {
        path: "vendor/copyleft.tgz",
        sha256: digest("vendor/copyleft.tgz"),
        license: "AGPL-3.0-or-later",
        form: "object-code",
        route: ROUTE_KEEPS_ARTIFACT,
        offer: `vendor/${VENDOR_OFFER}`,
        source: {
          clause: "AGPL-3.0 section 6(d)",
          url: "https://example.invalid/copyleft",
          revision: "v1",
          publicly_reachable: true,
          checked: { on: "2026-08-07", artifact_sha256: digest("vendor/copyleft.tgz"), result: "200" },
        },
      },
    ],
  }, null, 2));
  for (const { path, minBytes } of CONTRIBUTION_SURFACE) {
    write(path, `# ${path}\n\nhuman reviewer\n` + "x".repeat(minBytes));
  }
  return dir;
}

/** Rewrite `vendor/VENDORED.json` through a mutator, for the R11-R13 cases. */
const patchManifest = (mutate) => (dir, w, r) => {
  const rel = `vendor/${VENDOR_MANIFEST}`;
  const manifest = JSON.parse(r(rel));
  mutate(manifest, manifest.artifacts.find((a) => a.path === "vendor/copyleft.tgz"));
  w(rel, JSON.stringify(manifest, null, 2));
};

/** Each mutation must be rejected by the rule named beside it. */
const MUTATIONS = [
  ["R1", "truncate LICENSE to a pointer", (d, w) => w("LICENSE", "Apache-2.0 — see https://apache.org/licenses/LICENSE-2.0\n")],
  ["R2", "crate drops its licence field", (d, w) => w("crates/a/Cargo.toml", '[package]\nname = "a"\n')],
  ["R3", "a paid service joins the workspace", (d, w, r) => w("Cargo.toml", r("Cargo.toml").replace('"crates/a",', '"crates/a",\n  "services/relay",'))],
  ["R4", "a paid service is undocumented", (d, w) => w("README.md", `# synth\n\nLicensed under ${SPDX_ID}; see LICENSE.\n`)],
  ["R5", "a source file claims another licence", (d, w) => w("crates/a/src/lib.rs", `// ${SPDX_TAG}: GPL-3.0\n`)],
  ["R6", "package.json disagrees", (d, w) => w("package.json", JSON.stringify({ name: "synth", license: "UNLICENSED" }))],
  ["R7", "the allow-list admits copyleft", (d, w) => w("deny.toml", `[licenses]\nallow = [\n  "${SPDX_ID}",\n  "AGPL-3.0",\n]\n`)],
  ["R7", "the allow-list admits copyleft under its modern SPDX spelling", (d, w) => w("deny.toml", `[licenses]\nallow = [\n  "${SPDX_ID}",\n  "GPL-3.0-or-later",\n]\n`)],
  ["R8", "the contribution guide is a stub", (d, w) => w("CONTRIBUTING.md", "# Contributing\n")],
  ["R9", 'the README still says "Not yet licensed"', (d, w, r) => w("README.md", r("README.md") + "\n**Not yet licensed.**\n")],
  ["R10", "a vendored artifact loses its licence identifier", (d, w, r) => w("README.md", r("README.md").replace("| vendor/tool.tgz | MIT |", "| vendor/tool.tgz | |"))],
  ["R10", "a vendored artifact is never declared at all", (d, w, r) => w("README.md", r("README.md").replace(/^\| vendor\/tool.*$/m, ""))],
  ["R11", "a vendored artifact is never declared in the manifest",
    patchManifest((m) => { m.artifacts = m.artifacts.filter((a) => a.path !== "vendor/tool.tgz"); })],
  ["R11", "a vendored artifact hides one directory down",
    (d, w) => w("vendor/nested/smuggled.tgz", "undeclared\n")],
  ["R11", "the archive is replaced without re-declaring it",
    (d, w) => w("vendor/copyleft.tgz", "different bytes entirely\n")],
  ["R11", "the manifest and the README name different licences",
    patchManifest((m, c) => { c.license = "MIT"; })],
  ["R11", "the declared route says the archive was removed while it is still shipped",
    patchManifest((m, c) => { c.route = "registry-dependency"; })],
  ["R11", "the route is not one of the options the decision weighed",
    patchManifest((m, c) => { c.route = "trust-me"; })],
  ["R12", "a copyleft artifact declares no source route",
    patchManifest((m, c) => { delete c.source; })],
  ["R12", "a copyleft artifact names no source location",
    patchManifest((m, c) => { c.source.url = ""; })],
  ["R12", "the directions do not name the declared source location",
    (d, w) => w(`vendor/${VENDOR_OFFER}`, "# corresponding source\n\nvendor/copyleft.tgz — somewhere.\n")],
  ["R12", "the directions next to the object code are deleted",
    (d, w, r) => { rmSync(join(d, "vendor", VENDOR_OFFER)); }],
  ["R12", "the reachability check was run against different bytes",
    patchManifest((m, c) => { c.source.checked.artifact_sha256 = "0".repeat(64); })],
  ["R13", "publication is cleared while a copyleft source is not reachable",
    patchManifest((m, c) => { c.source.publicly_reachable = false; })],
  ["R13", "publication is cleared with no corresponding revision pinned",
    patchManifest((m, c) => { c.source.revision = null; })],
  ["R13", "publication is declared blocked when nothing blocks it",
    patchManifest((m) => { m.publication = { state: "blocked", blocked_by: ["vendor/copyleft.tgz"] }; })],
  ["R13", "the README stops pointing at the corresponding-source directions",
    (d, w, r) => w("README.md", r("README.md").replace(`vendor/${VENDOR_OFFER}`, "somewhere else"))],
  ["R13", "the README stops naming the decision that set the posture",
    (d, w, r) => w("README.md", r("README.md").replace("01SYNTHDECISIONULID00000000", "an earlier discussion"))],
];

/** Changes the checker must NOT reject. */
const TOLERANCES = [
  ["LICENSE checked out with CRLF line endings", (d, w, r) => w("LICENSE", r("LICENSE").replace(/\n/g, "\r\n"))],
  ["LICENSE carrying a UTF-8 byte-order mark", (d, w, r) => w("LICENSE", "﻿" + r("LICENSE"))],
  // The state this repository is actually in today. If the checker rejected an
  // honestly-declared blocked gate, `npm test` would be red on main for every
  // lane until the source repository is published — the check would be punishing
  // the disclosure instead of the omission.
  ["a copyleft source that is not yet reachable, declared honestly as blocking",
    patchManifest((m, c) => {
      c.source.publicly_reachable = false;
      c.source.revision = null;
      m.publication = { state: "blocked", blocked_by: ["vendor/copyleft.tgz"] };
    })],
  // A permissive artifact owes no corresponding source. R12 must not demand one.
  ["a permissive vendored artifact with no source block at all",
    patchManifest((m) => { delete m.artifacts.find((a) => a.path === "vendor/tool.tgz").source; })],
];

function selfTest() {
  const base = join(tmpdir(), `mesh-license-check-selftest-${process.pid}`);
  rmSync(base, { recursive: true, force: true });
  const failures = [];
  const build = (name) => {
    const dir = join(base, name);
    mkdirSync(dir, { recursive: true });
    synthTree(dir);
    return dir;
  };
  try {
    const clean = check(build("clean"));
    if (clean.length !== 0) {
      failures.push(`the unmutated synthetic tree must pass, got: ${JSON.stringify(clean)}`);
    }
    for (const [index, [rule, label, mutate]] of MUTATIONS.entries()) {
      const dir = build(`${index}-${rule.toLowerCase()}`); // indexed: one rule may have several mutations
      mutate(dir, (rel, body) => {
        mkdirSync(join(dir, dirname(rel)), { recursive: true });
        writeFileSync(join(dir, rel), body);
      }, (rel) => readFileSync(join(dir, rel), "utf8"));
      // Exact id match: `startsWith` would let R10 stand in for a dead R1.
      const caught = check(dir).filter((v) => v.rule.split(" ")[0] === rule);
      if (caught.length === 0) failures.push(`${rule} did not reject: ${label}`);
      else process.stdout.write(`  rejected  ${rule}  ${label}\n`);
    }
    // Tolerances: changes that MUST still pass. A checker that rejects everything
    // is as useless as one that rejects nothing, and this one is the difference
    // between a Windows contributor being right and being told they are wrong.
    for (const [label, mutate] of TOLERANCES) {
      const dir = build(`tolerance-${label.replace(/\W+/g, "-")}`);
      mutate(dir, (rel, body) => writeFileSync(join(dir, rel), body), (rel) => readFileSync(join(dir, rel), "utf8"));
      const violations = check(dir);
      if (violations.length > 0) failures.push(`must still accept ${label}: ${JSON.stringify(violations)}`);
      else process.stdout.write(`  accepted  --  ${label}\n`);
    }
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
  if (failures.length > 0) {
    process.stderr.write("\nself-test FAILED\n" + failures.map((f) => `  ${f}\n`).join(""));
    return finish(1);
  }
  process.stdout.write(
    `\nself-test PASS — ${MUTATIONS.length} mutations rejected, ` +
      `${TOLERANCES.length} tolerances accepted, clean tree accepted\n`,
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
  if (process.argv.includes("--self-test")) return selfTest();
  const violations = check(REPO_ROOT);
  if (violations.length === 0) {
    process.stdout.write(`license-check PASS — ${RULES.length} rules over ${REPO_ROOT}\n`);
    return;
  }
  process.stderr.write(`license-check FAIL — ${violations.length} violation(s)\n\n`);
  for (const v of violations) {
    process.stderr.write(`  ${v.file}\n    ${v.rule}: ${v.problem}\n    fix: ${v.fix}\n\n`);
  }
  finish(1);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
