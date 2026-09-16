/**
 * The self-test: fixtures that prove every matcher actually fires.
 *
 * A lint whose matchers have quietly stopped matching reports "clean" forever,
 * which is worse than no lint at all. So the fixture suite runs by default on
 * every invocation, and it asserts four things: each declared expectation is
 * produced, each clean fixture stays clean, every one of the nine forbidden
 * terms is covered by at least one fixture, and the manifest validator still
 * refuses the configurations that would make a surface scan nothing.
 *
 * The fixture expectations pin *exact* messages wherever a message encodes an
 * ordering. A loose `messageIncludes` let a real reordering of the six status
 * words pass 23/23 green, which is the same class of failure as a dead matcher.
 */

import fs from 'node:fs';
import path from 'node:path';
import { lintFile } from './engine.mjs';
import { validateSurface } from './surfaces.mjs';
import { FORBIDDEN_TERMS } from './vocabulary.mjs';

const FIXTURE_DIR = 'fixtures';

export function runSelfTest(toolRoot) {
  const manifest = loadFixtureManifest(toolRoot);
  const failures = [];
  const firedTerms = new Set();
  let passed = 0;

  for (const fixture of manifest.fixtures) {
    const outcome = runFixture(toolRoot, fixture, firedTerms);
    if (outcome === null) passed += 1;
    else failures.push({ fixture: fixture.file, message: outcome });
  }

  for (const testCase of manifest.manifestCases ?? []) {
    const outcome = runManifestCase(testCase);
    if (outcome === null) passed += 1;
    else failures.push({ fixture: `manifest case "${testCase.name}"`, message: outcome });
  }

  const uncovered = FORBIDDEN_TERMS.filter((term) => !firedTerms.has(term.id)).map((term) => term.term);
  if (uncovered.length > 0) {
    failures.push({
      fixture: `${FIXTURE_DIR}/expectations.json`,
      message: `no fixture proves these terms fire: ${uncovered.join(', ')}`,
    });
  }

  return {
    ok: failures.length === 0,
    total: manifest.fixtures.length + (manifest.manifestCases?.length ?? 0),
    passed,
    termsCovered: FORBIDDEN_TERMS.length - uncovered.length,
    failures,
  };
}

/**
 * Assert that `validateSurface` rejects a manifest entry that would silently
 * disable a check — a mode that cannot read its own files, an unbounded glob, a
 * misspelled rule.
 */
function runManifestCase(testCase) {
  let thrown = null;
  try {
    validateSurface(testCase.surface, 0, 'surfaces.json');
  } catch (error) {
    thrown = error;
  }
  if (testCase.expectError === undefined) {
    return thrown === null ? null : `expected the surface to validate, but it threw: ${thrown.message}`;
  }
  if (thrown === null) {
    return `expected validation to reject this surface with a message mentioning "${testCase.expectError}", but it was accepted — the manifest can once again declare a surface that scans nothing`;
  }
  if (!thrown.message.includes(testCase.expectError)) {
    return `expected the rejection to mention "${testCase.expectError}"; got: ${thrown.message}`;
  }
  return null;
}

function runFixture(toolRoot, fixture, firedTerms) {
  let result;
  try {
    result = lintFile({
      root: path.join(toolRoot, FIXTURE_DIR),
      toolRoot,
      relPath: fixture.file,
      mode: fixture.mode,
      rules: fixture.rules,
      allowRegionBudget: fixture.allowRegionBudget ?? 0,
      statusPath: fixture.statusPath,
    });
  } catch (error) {
    return `threw while linting: ${error.message}`;
  }
  for (const finding of result.findings) {
    if (finding.termId) firedTerms.add(finding.termId);
  }

  if (fixture.expect === 'clean') {
    if (result.findings.length === 0) return null;
    const first = result.findings[0];
    return `expected clean, got ${result.findings.length} finding(s), first: [${first.rule}] ${first.message}`;
  }
  if (!Array.isArray(fixture.expect) || fixture.expect.length === 0) {
    return 'expectation must be "clean" or a non-empty array';
  }
  for (const wanted of fixture.expect) {
    const hit = result.findings.find(
      (finding) =>
        finding.rule === wanted.rule &&
        (wanted.termId === undefined || finding.termId === wanted.termId) &&
        (wanted.messageIncludes === undefined || finding.message.includes(wanted.messageIncludes)),
    );
    if (!hit) {
      return `expected a ${wanted.rule} finding${wanted.termId ? ` for "${wanted.termId}"` : ''}${
        wanted.messageIncludes ? ` mentioning "${wanted.messageIncludes}"` : ''
      }; got ${describe(result.findings)}`;
    }
  }
  return null;
}

function describe(findings) {
  if (findings.length === 0) return 'no findings';
  return findings.map((finding) => `[${finding.rule}${finding.termId ? `/${finding.termId}` : ''}]`).join(' ');
}

function loadFixtureManifest(toolRoot) {
  const file = path.join(toolRoot, FIXTURE_DIR, 'expectations.json');
  let parsed;
  try {
    parsed = JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (error) {
    throw new Error(`cannot read ${file}: ${error.message}`);
  }
  if (!Array.isArray(parsed?.fixtures) || parsed.fixtures.length === 0) {
    throw new Error(`${file} must declare a non-empty "fixtures" array`);
  }
  for (const fixture of parsed.fixtures) {
    if (typeof fixture.file !== 'string' || typeof fixture.mode !== 'string' || !Array.isArray(fixture.rules)) {
      throw new Error(`${file}: every fixture needs "file", "mode" and "rules"`);
    }
  }
  return parsed;
}
