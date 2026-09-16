/**
 * Just enough Markdown to read the structured tables the PRD publishes as
 * machine-checkable contracts. Tables the lint depends on are anchored by an
 * HTML comment marker so that the rule binds to an explicit declaration rather
 * than to document order — moving a section must not silently disable a check.
 */

const ROW = /^\s*\|(.+)\|\s*$/;
const SEPARATOR = /^\s*\|[\s:|-]+\|\s*$/;

/** @typedef {{ header: string[], rows: {cells: string[], line: number}[], startLine: number }} Table */

/** Split a `| a | b |` row into trimmed cells, respecting `\|` escapes. */
export function splitRow(raw) {
  const inner = raw.match(ROW)?.[1] ?? '';
  const cells = [];
  let current = '';
  for (let i = 0; i < inner.length; i += 1) {
    if (inner[i] === '\\' && inner[i + 1] === '|') {
      current += '|';
      i += 1;
      continue;
    }
    if (inner[i] === '|') {
      cells.push(current.trim());
      current = '';
      continue;
    }
    current += inner[i];
  }
  cells.push(current.trim());
  return cells;
}

/**
 * Every table in the document.
 * @returns {Table[]}
 */
export function parseTables(text) {
  const lines = text.split('\n');
  const tables = [];
  let i = 0;
  while (i < lines.length) {
    if (ROW.test(lines[i]) && !SEPARATOR.test(lines[i]) && SEPARATOR.test(lines[i + 1] ?? '')) {
      const header = splitRow(lines[i]);
      const startLine = i + 1;
      const rows = [];
      let j = i + 2;
      while (j < lines.length && ROW.test(lines[j]) && !SEPARATOR.test(lines[j])) {
        rows.push({ cells: splitRow(lines[j]), line: j + 1 });
        j += 1;
      }
      tables.push({ header, rows, startLine });
      i = j;
      continue;
    }
    i += 1;
  }
  return tables;
}

/**
 * The first table appearing after `<!-- vocab-lint:<marker> -->`.
 * @returns {Table|null}
 */
export function tableAfterMarker(text, marker) {
  const lines = text.split('\n');
  const anchor = new RegExp(`<!--\\s*vocab-lint:${marker}\\s*-->`);
  const anchorIndex = lines.findIndex((line) => anchor.test(line));
  if (anchorIndex === -1) return null;
  const tail = lines.slice(anchorIndex + 1).join('\n');
  const [first] = parseTables(tail);
  if (!first) return null;
  const offset = anchorIndex + 1;
  return {
    header: first.header,
    startLine: first.startLine + offset,
    rows: first.rows.map((row) => ({ cells: row.cells, line: row.line + offset })),
  };
}

/** Index of the column whose header equals `name` (case-insensitive). */
export function columnIndex(table, name) {
  return table.header.findIndex(
    (heading) => normalizeCell(heading).toLowerCase() === name.toLowerCase(),
  );
}

/** Strip Markdown emphasis and code fencing so cell values compare cleanly. */
export function normalizeCell(cell) {
  return cell
    .replace(/`/g, '')
    .replace(/\*\*/g, '')
    .replace(/\s+/g, ' ')
    .trim();
}
