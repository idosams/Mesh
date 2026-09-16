/**
 * Rule `journey` — the primary journey contains zero ceremony steps.
 *
 * Charter P1: the user never has to create a task, a branch, a commit, a
 * checkpoint, a proposal or a merge request. That is checkable, not aspirational,
 * because the journey table declares who acts on each step and what the user has
 * to create. The rule requires:
 *
 *   1. every row's "What you must create" cell to read exactly "Nothing";
 *   2. no row acted by the user to mention a unit of work at all.
 *
 * A journey step that needs a ceremony noun in the user's own hands is a product
 * defect, and this is where it surfaces.
 */

import { ceremonyMatcher } from '../vocabulary.mjs';
import { tableAfterMarker, columnIndex, normalizeCell } from '../markdown.mjs';

export const id = 'journey';
export const modes = ['prose'];

const USER_ACTORS = new Set(['you', 'user', 'the user']);
const NOTHING = 'nothing';
const MINIMUM_STEPS = 12;

export function run({ path, text }) {
  const table = tableAfterMarker(text, 'journey');
  if (!table) {
    return [flag(path, 1, 'no table anchored <!-- vocab-lint:journey --> — the primary journey is unstated')];
  }
  const actorColumn = columnIndex(table, 'Who acts');
  const happensColumn = columnIndex(table, 'What happens');
  const createColumn = columnIndex(table, 'What you must create');
  if (actorColumn === -1 || happensColumn === -1 || createColumn === -1) {
    return [
      flag(
        path,
        table.startLine,
        'the journey table needs "Who acts", "What happens" and "What you must create" columns',
      ),
    ];
  }

  const findings = [];
  if (table.rows.length < MINIMUM_STEPS) {
    findings.push(
      flag(path, table.startLine, `the journey has ${table.rows.length} steps, expected at least ${MINIMUM_STEPS}`),
    );
  }
  for (const row of table.rows) {
    const actor = normalizeCell(row.cells[actorColumn] ?? '');
    const happens = normalizeCell(row.cells[happensColumn] ?? '');
    const create = normalizeCell(row.cells[createColumn] ?? '');
    if (create.toLowerCase() !== NOTHING) {
      findings.push(
        flag(path, row.line, `this step asks the user to create "${create}" — the journey must be ceremony-free`),
      );
    }
    if (!USER_ACTORS.has(actor.toLowerCase())) continue;
    const matcher = ceremonyMatcher();
    const match = matcher.exec(happens);
    if (match !== null) {
      findings.push(
        flag(
          path,
          row.line,
          `a user-acted step mentions "${match[0]}" — the user never handles a unit of work`,
          happens,
        ),
      );
    }
  }
  return findings;
}

function flag(path, line, message, excerpt = '') {
  return { rule: id, path, line, column: 1, message, excerpt };
}
