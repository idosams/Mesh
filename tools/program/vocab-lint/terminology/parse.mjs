/**
 * Parsing for the terminology lint.
 *
 * Every function here is pure over its input text, so the checks can be run
 * against a real document or against a mutated in-memory copy. That is what the
 * self-test relies on: a check nothing can break is not a check.
 */

import fs from 'node:fs';

/**
 * A fence opener or closer, including one nested inside a blockquote. The
 * blockquote prefix is load-bearing: §6 of `docs/protocol.md` quotes a console
 * transcript inside a `>` block, and a fence regex anchored at column zero walks
 * straight past it, leaving the transcript's backticks to be read as prose
 * tokens. Every stray finding that produced named a fragment of a sentence.
 */
const FENCE = /^\s*(?:>\s*)*```/;
const TOKEN = /`([^`]+)`/g;
const SINGLE_TOKEN_CELL = /^`([^`]+)`$/;
const SEPARATOR_CELL = /^:?-{2,}:?$/;
const DEFINITION_HEADING = /^#{3}\s+\d+(?:\.\d+)*\s+(.+?)\s+—\s+\S/;
const MEMBER_LEAD = /exactly one of:\s*([^.]*)/i;

/** Collapse the whitespace a Markdown line wrap introduces inside a token. */
export function normalize(value) {
  return value.replace(/\s+/g, ' ').trim();
}

/**
 * Read a Markdown document, blanking fenced code so that examples inside a
 * fence never register as terminology usage. Offsets stay aligned with the
 * original file, so every finding can name a real line.
 */
export function readDocument(absolutePath, displayPath) {
  const raw = fs.readFileSync(absolutePath, 'utf8');
  return buildDocument(raw, displayPath);
}

/** @returns {{path: string, lines: string[], text: string, lineStarts: number[]}} */
export function buildDocument(raw, displayPath) {
  const lines = raw.split('\n');
  const stripped = [];
  let inFence = false;
  for (const line of lines) {
    if (FENCE.test(line)) {
      inFence = !inFence;
      stripped.push('');
      continue;
    }
    stripped.push(inFence ? '' : line);
  }
  const text = stripped.join('\n');
  const lineStarts = [0];
  for (let i = 0; i < text.length; i += 1) {
    if (text[i] === '\n') lineStarts.push(i + 1);
  }
  return { path: displayPath, lines, text, lineStarts };
}

/** 1-indexed line number containing `index`. */
export function lineAt(doc, index) {
  let low = 0;
  let high = doc.lineStarts.length - 1;
  while (low < high) {
    const mid = Math.ceil((low + high) / 2);
    if (doc.lineStarts[mid] <= index) low = mid;
    else high = mid - 1;
  }
  return low + 1;
}

/** The text between `<!-- name:begin -->` and `<!-- name:end -->`, or null. */
export function block(doc, name) {
  const begin = `<!-- ${name}:begin -->`;
  const end = `<!-- ${name}:end -->`;
  const start = doc.text.indexOf(begin);
  const stop = doc.text.indexOf(end);
  if (start === -1 || stop === -1 || stop < start) return null;
  return { text: doc.text.slice(start + begin.length, stop), offset: start + begin.length };
}

/** Pipe-table rows inside a section, header and separator rows dropped. */
export function tableRows(doc, section) {
  const rows = [];
  let offset = section.offset;
  for (const line of section.text.split('\n')) {
    const trimmed = line.trim();
    if (trimmed.startsWith('|') && trimmed.endsWith('|') && trimmed.length > 1) {
      const cells = trimmed
        .slice(1, -1)
        .split('|')
        .map((cell) => cell.trim());
      if (!cells.every((cell) => SEPARATOR_CELL.test(cell))) {
        rows.push({ cells, line: lineAt(doc, offset) });
      }
    }
    offset += line.length + 1;
  }
  return rows;
}

/** Rows whose first cell is exactly one backticked token — the data rows. */
function keyedRows(doc, name) {
  const section = block(doc, name);
  if (!section) return null;
  return tableRows(doc, section)
    .map((row) => {
      const match = SINGLE_TOKEN_CELL.exec(row.cells[0] ?? '');
      return match ? { ...row, key: normalize(match[1]) } : null;
    })
    .filter(Boolean);
}

export function registerRows(doc) {
  const rows = keyedRows(doc, 'terminology');
  if (!rows) return null;
  return rows.map((row) => ({
    term: row.key,
    definition: row.cells[1] ?? '',
    graph: row.cells[2] ?? '',
    home: normalize((row.cells[3] ?? '').replace(/`/g, '')),
    line: row.line,
    doc: doc.path,
  }));
}

export function aliasRows(doc) {
  const rows = keyedRows(doc, 'aliases');
  if (!rows) return null;
  return rows.map((row) => ({
    alias: row.key,
    resolvesTo: normalize((row.cells[1] ?? '').replace(/`/g, '')),
    line: row.line,
    doc: doc.path,
  }));
}

export function nonTermRows(doc) {
  const rows = keyedRows(doc, 'non-terms');
  if (!rows) return null;
  return rows.map((row) => ({ literal: row.key, line: row.line, doc: doc.path }));
}

/** The enumerated members a definition declares with "exactly one of: …". */
export function membersOf(definition) {
  const match = MEMBER_LEAD.exec(definition);
  if (!match) return [];
  return [...match[1].matchAll(TOKEN)].map((hit) => normalize(hit[1]));
}

/** Every backticked token in the document, with the line it sits on. */
export function tokens(doc) {
  const found = [];
  for (const match of doc.text.matchAll(TOKEN)) {
    found.push({
      text: normalize(match[1]),
      line: lineAt(doc, match.index),
      doc: doc.path,
    });
  }
  return found;
}

/**
 * Headings of the shape `### 1.2 Some concept — gloss`. A heading in that shape
 * introduces a concept, so the concept has to be a register term; this is what
 * stops a companion document from growing a second definition of its own.
 */
export function definitionHeadings(doc) {
  const found = [];
  doc.lines.forEach((line, index) => {
    const match = DEFINITION_HEADING.exec(line);
    if (match) {
      found.push({
        name: normalize(match[1]).replace(/^The\s+/i, ''),
        line: index + 1,
        doc: doc.path,
      });
    }
  });
  return found;
}
