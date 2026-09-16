/**
 * Rule `mapping` — the internal-to-user-facing state mapping is complete in
 * both directions.
 *
 * Two tables are published: internal → user-facing, and user-facing → internal.
 * The rule asserts they are exact inverses of one another as sets of pairs. A
 * one-way table is how a graph concept leaks: a reviewer reading only the
 * forward table cannot tell whether a user-facing word is overloaded, and a
 * developer reading only the reverse table cannot tell whether an internal
 * state has any wording at all.
 *
 * The rule also closes the user-facing column against `APPROVED_MAPPING_WORDING`
 * — the six status words plus the four object wordings. Symmetry alone is not
 * enough: two tables can be perfect inverses of each other and still both
 * contradict the six status words, which is exactly how "Needs review" came to
 * sit beside "Needs attention" and "Ready for review" without any rule
 * noticing.
 */

import { tableAfterMarker, columnIndex, normalizeCell } from '../markdown.mjs';
import { APPROVED_MAPPING_WORDING, APPROVED_STATUS } from '../vocabulary.mjs';

export const id = 'mapping';
export const modes = ['prose'];

const MINIMUM_PAIRS = 8;

export function run({ path, text }) {
  const forward = tableAfterMarker(text, 'mapping-forward');
  const reverse = tableAfterMarker(text, 'mapping-reverse');
  const findings = [];
  if (!forward) findings.push(flag(path, 1, 'no table anchored <!-- vocab-lint:mapping-forward -->'));
  if (!reverse) findings.push(flag(path, 1, 'no table anchored <!-- vocab-lint:mapping-reverse -->'));
  if (findings.length > 0) return findings;

  const forwardPairs = readPairs(path, forward, 'Internal state', 'User-facing wording', findings);
  const reversePairs = readPairs(path, reverse, 'Internal state', 'User-facing wording', findings);
  if (findings.length > 0) return findings;

  if (forwardPairs.size < MINIMUM_PAIRS) {
    findings.push(
      flag(path, forward.startLine, `the forward mapping has ${forwardPairs.size} rows, expected at least ${MINIMUM_PAIRS}`),
    );
  }
  for (const [key, entry] of forwardPairs) {
    if (!reversePairs.has(key)) {
      findings.push(
        flag(path, entry.line, `"${entry.internal}" → "${entry.user}" has no row in the reverse mapping`),
      );
    }
  }
  for (const [key, entry] of reversePairs) {
    if (!forwardPairs.has(key)) {
      findings.push(
        flag(path, entry.line, `"${entry.user}" → "${entry.internal}" has no row in the forward mapping`),
      );
    }
  }
  findings.push(...closedVocabulary(path, forwardPairs), ...closedVocabulary(path, reversePairs));
  return findings.sort((a, b) => a.line - b.line);
}

/**
 * Every user-facing wording in a mapping table must come from the closed set.
 * A wording that is not a status word and not a declared object wording is
 * either a seventh status word in disguise or a term nobody has approved.
 */
function closedVocabulary(path, pairs) {
  const findings = [];
  for (const entry of pairs.values()) {
    if (APPROVED_MAPPING_WORDING.includes(entry.user)) continue;
    findings.push(
      flag(
        path,
        entry.line,
        `"${entry.user}" is not approved user-facing wording — the mapping may only use the six status words (${APPROVED_STATUS.join(' · ')}) or a declared object wording; add it to APPROVED_OBJECT_WORDING in a reviewed change, or reuse a status word`,
      ),
    );
  }
  return findings;
}

function readPairs(path, table, internalHeader, userHeader, findings) {
  const internalColumn = columnIndex(table, internalHeader);
  const userColumn = columnIndex(table, userHeader);
  const pairs = new Map();
  if (internalColumn === -1 || userColumn === -1) {
    findings.push(
      flag(path, table.startLine, `a mapping table needs "${internalHeader}" and "${userHeader}" columns`),
    );
    return pairs;
  }
  for (const row of table.rows) {
    const internal = normalizeCell(row.cells[internalColumn] ?? '');
    const user = normalizeCell(row.cells[userColumn] ?? '');
    if (internal.length === 0 || user.length === 0) {
      findings.push(flag(path, row.line, 'a mapping row has an empty cell'));
      continue;
    }
    pairs.set(`${internal.toLowerCase()}→${user.toLowerCase()}`, { internal, user, line: row.line });
  }
  return pairs;
}

function flag(path, line, message) {
  return { rule: id, path, line, column: 1, message, excerpt: '' };
}
