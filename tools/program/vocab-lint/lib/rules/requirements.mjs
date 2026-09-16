/**
 * Rule `requirements` — the functional-requirement inventory is complete,
 * unique and unweakened.
 *
 * The expected inventory is `requirements.expected.json`, transcribed from the
 * execution plan §3.5. The rule asserts a set equality in both directions
 * (nothing dropped, nothing invented), that every ID keeps the priority the
 * plan gave it, and that no row ships without a requirement text and an
 * acceptance condition. A silently downgraded P0 is the failure this exists to
 * catch.
 */

import fs from 'node:fs';
import path from 'node:path';
import { parseTables, columnIndex, normalizeCell } from '../markdown.mjs';

export const id = 'requirements';
export const modes = ['prose'];

const ID_PATTERN = /^[A-Z]{3}-\d{3}$/;
const PRIORITIES = new Set(['P0', 'P1']);

export function run({ path: filePath, text, toolRoot }) {
  const expected = loadExpected(toolRoot);
  const rows = collectRows(text);
  const findings = [];

  if (rows.length === 0) {
    return [flag(filePath, 1, 'no requirement table found (needs ID, Priority, Acceptance condition columns)')];
  }

  const seen = new Map();
  for (const row of rows) {
    if (!ID_PATTERN.test(row.id)) {
      findings.push(flag(filePath, row.line, `"${row.id}" is not a requirement ID (expected e.g. WSP-001)`));
      continue;
    }
    if (seen.has(row.id)) {
      findings.push(
        flag(filePath, row.line, `duplicate requirement ID ${row.id} (first seen on line ${seen.get(row.id)})`),
      );
      continue;
    }
    seen.set(row.id, row.line);
    if (row.requirement.length === 0) {
      findings.push(flag(filePath, row.line, `${row.id} has no requirement text`));
    }
    if (row.acceptance.length === 0) {
      findings.push(flag(filePath, row.line, `${row.id} has no acceptance condition`));
    }
    if (!PRIORITIES.has(row.priority)) {
      findings.push(flag(filePath, row.line, `${row.id} has priority "${row.priority}", expected P0 or P1`));
    }
  }

  const expectedById = new Map(expected.requirements.map((entry) => [entry.id, entry]));
  for (const [reqId, line] of seen) {
    const want = expectedById.get(reqId);
    if (!want) {
      findings.push(flag(filePath, line, `${reqId} is not in the plan §3.5 inventory`));
      continue;
    }
    const actual = rows.find((row) => row.id === reqId);
    if (actual.priority !== want.priority) {
      findings.push(
        flag(filePath, line, `${reqId} is ${actual.priority} here and ${want.priority} in plan §3.5`),
      );
    }
  }
  for (const entry of expected.requirements) {
    if (!seen.has(entry.id)) {
      findings.push(flag(filePath, 1, `${entry.id} (plan §3.5) is missing from this document`));
    }
  }
  for (const family of expected.families) {
    const count = [...seen.keys()].filter((reqId) => reqId.startsWith(`${family.prefix}-`)).length;
    if (count !== family.count) {
      findings.push(
        flag(filePath, 1, `${family.prefix} (${family.title}) has ${count} requirements, expected ${family.count}`),
      );
    }
  }
  return findings.sort((a, b) => a.line - b.line || a.message.localeCompare(b.message));
}

function collectRows(text) {
  const rows = [];
  for (const table of parseTables(text)) {
    const idColumn = columnIndex(table, 'ID');
    const priorityColumn = columnIndex(table, 'Priority');
    const acceptanceColumn = columnIndex(table, 'Acceptance condition');
    const requirementColumn = columnIndex(table, 'Requirement');
    if (idColumn === -1 || priorityColumn === -1 || acceptanceColumn === -1) continue;
    for (const row of table.rows) {
      rows.push({
        id: normalizeCell(row.cells[idColumn] ?? ''),
        requirement: normalizeCell(row.cells[requirementColumn] ?? ''),
        priority: normalizeCell(row.cells[priorityColumn] ?? ''),
        acceptance: normalizeCell(row.cells[acceptanceColumn] ?? ''),
        line: row.line,
      });
    }
  }
  return rows;
}

function loadExpected(toolRoot) {
  const file = path.join(toolRoot, 'requirements.expected.json');
  let parsed;
  try {
    parsed = JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (error) {
    throw new Error(`cannot read ${file}: ${error.message}`);
  }
  if (!Array.isArray(parsed?.requirements) || !Array.isArray(parsed?.families)) {
    throw new Error(`${file} must declare "requirements" and "families" arrays`);
  }
  return parsed;
}

function flag(filePath, line, message) {
  return { rule: id, path: filePath, line, column: 1, message, excerpt: '' };
}
