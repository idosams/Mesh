/**
 * Rule `six-state` — user-facing status is exactly six words, in one order.
 *
 * On a prose surface the rule binds to the table anchored
 * `<!-- vocab-lint:six-state -->` and requires the six words, in the charter's
 * order, with no additions. On a status catalogue the *status values* must each
 * be one of the six — a seventh word is a product design question for the
 * product owner, not a local edit.
 *
 * Which values are the status values is declared by the surface's `statusPath`,
 * not guessed. Checking every string in the file instead meant no realistic
 * catalogue could pass: an entry like
 * `{"working": {"label": "Working", "icon": "spinner"}}` failed on its icon
 * name and on its help sentence, so the surface's only survivable shape was a
 * flat six-key map. A rule that forces the data into an unnatural shape gets
 * the surface dropped from the manifest, which is how a gate quietly stops
 * covering anything.
 *
 * `statusPath` is a small `$.a.*.b` selector: `*` visits every entry of an
 * object or array, a name visits that key. The default `$.*` is the flat map.
 */

import { APPROVED_STATUS } from '../vocabulary.mjs';
import { tableAfterMarker, columnIndex, normalizeCell } from '../markdown.mjs';

export const id = 'six-state';
export const modes = ['prose', 'status-catalog'];

export const DEFAULT_STATUS_PATH = '$.*';

export function run(context) {
  return context.mode === 'prose' ? runProse(context) : runCatalog(context);
}

function runProse({ path, text }) {
  const table = tableAfterMarker(text, 'six-state');
  if (!table) {
    return [
      {
        rule: id,
        path,
        line: 1,
        column: 1,
        message: 'no table anchored <!-- vocab-lint:six-state --> — the six status words are unstated',
        excerpt: '',
      },
    ];
  }
  const statusColumn = columnIndex(table, 'Status');
  if (statusColumn === -1) {
    return [
      {
        rule: id,
        path,
        line: table.startLine,
        column: 1,
        message: 'the six-state table needs a "Status" column',
        excerpt: table.header.join(' | '),
      },
    ];
  }
  const found = table.rows.map((row) => normalizeCell(row.cells[statusColumn] ?? ''));
  const findings = [];
  found.forEach((status, index) => {
    const expected = APPROVED_STATUS[index];
    if (expected === undefined) {
      findings.push(rowFinding(path, table.rows[index].line, `"${status}" is a seventh status word`));
    } else if (status !== expected) {
      findings.push(
        rowFinding(
          path,
          table.rows[index].line,
          `status ${index + 1} is "${status}", expected "${expected}"`,
        ),
      );
    }
  });
  if (found.length < APPROVED_STATUS.length) {
    const missing = APPROVED_STATUS.slice(found.length).join(', ');
    findings.push(rowFinding(path, table.startLine, `status words missing from the table: ${missing}`));
  }
  return findings;
}

function runCatalog({ path, text, segments, statusPath }) {
  const expression = statusPath ?? DEFAULT_STATUS_PATH;
  let document;
  try {
    document = JSON.parse(text);
  } catch (error) {
    return [flagAt(path, 1, 1, `cannot be parsed as JSON: ${error.message}`, '')];
  }

  let steps;
  try {
    steps = parseStatusPath(expression);
  } catch (error) {
    return [flagAt(path, 1, 1, error.message, expression)];
  }

  const selected = selectPath(document, steps);
  if (selected.length === 0) {
    return [
      flagAt(
        path,
        1,
        1,
        `the status path "${expression}" selects nothing in this file — a status catalogue whose statuses are never read is a check that scans nothing; fix the path or the catalogue's shape`,
        expression,
      ),
    ];
  }

  const locate = segmentLocator(segments);
  const findings = [];
  for (const { value, pointer } of selected) {
    if (typeof value !== 'string') {
      findings.push(
        flagAt(path, 1, 1, `the status at ${pointer} is ${describeType(value)}, expected one of the six user-facing status words`, pointer),
      );
      continue;
    }
    if (APPROVED_STATUS.includes(value)) continue;
    const at = locate(value);
    findings.push(
      flagAt(
        path,
        at.line,
        at.column,
        `"${value}" (at ${pointer}) is not one of the six user-facing status words (${APPROVED_STATUS.join(' · ')})`,
        value,
      ),
    );
  }
  return findings;
}

/** `$.*.label` → `['*', 'label']`. */
export function parseStatusPath(expression) {
  if (typeof expression !== 'string' || !expression.startsWith('$')) {
    throw new Error(`status path "${expression}" must start with "$" (for example ${DEFAULT_STATUS_PATH} or $.*.label)`);
  }
  const rest = expression.slice(1);
  if (rest.length === 0) {
    throw new Error(`status path "${expression}" selects the whole document; name the field that holds the status, for example ${DEFAULT_STATUS_PATH}`);
  }
  if (!rest.startsWith('.')) {
    throw new Error(`status path "${expression}" must separate steps with "." (for example ${DEFAULT_STATUS_PATH})`);
  }
  const steps = rest.slice(1).split('.');
  if (steps.some((step) => step.length === 0)) {
    throw new Error(`status path "${expression}" has an empty step`);
  }
  return steps;
}

/** Every value reachable by `steps`, with the pointer that reached it. */
function selectPath(value, steps, pointer = '$') {
  if (steps.length === 0) return [{ value, pointer }];
  const [step, ...rest] = steps;
  if (value === null || typeof value !== 'object') return [];
  if (step === '*') {
    const keys = Array.isArray(value) ? value.map((_, index) => index) : Object.keys(value);
    return keys.flatMap((key) => selectPath(value[key], rest, `${pointer}.${key}`));
  }
  if (Array.isArray(value) || !Object.hasOwn(value, step)) return [];
  return selectPath(value[step], rest, `${pointer}.${step}`);
}

/**
 * Map a selected status value back to where it was written. Segments carry the
 * positions the extractor already computed; each is handed out once so repeated
 * values point at successive occurrences rather than all at the first.
 */
function segmentLocator(segments) {
  const used = new Set();
  return (value) => {
    const index = (segments ?? []).findIndex(
      (segment, at) => !used.has(at) && segment.text === value,
    );
    if (index === -1) return { line: 1, column: 1 };
    used.add(index);
    return { line: segments[index].line, column: segments[index].column };
  };
}

function describeType(value) {
  if (value === null) return 'null';
  return Array.isArray(value) ? 'an array' : `a ${typeof value}`;
}

function flagAt(path, line, column, message, excerpt) {
  return { rule: id, path, line, column, message, excerpt };
}

function rowFinding(path, line, message) {
  return { rule: id, path, line, column: 1, message, excerpt: '' };
}
