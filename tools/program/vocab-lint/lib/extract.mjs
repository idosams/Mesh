/**
 * Extract the *user-facing strings* from a file.
 *
 * "User-facing string" is the unit the acceptance criterion names, so it is the
 * unit the lint scans. What counts as one depends on the surface:
 *
 * | mode             | user-facing string                                   |
 * |------------------|------------------------------------------------------|
 * | `prose`          | the whole document, minus explicit allow regions      |
 * | `json-strings`   | every JSON string *value* (keys are identifiers)      |
 * | `status-catalog` | every JSON string value, also checked against the six |
 * | `source-strings` | every string literal, comments excluded               |
 *
 * Comments are not user-facing and are not scanned; a literal inside a comment
 * is not shipped. The C-like extractor therefore tokenizes rather than
 * regex-stripping, so that `"https://example.test"` is not mistaken for a
 * comment and silently dropped from the scan.
 */

import { lineStarts, positionAt } from './positions.mjs';

/** @typedef {{ text: string, offset: number, line: number, column: number }} Segment */

export const MODES = Object.freeze([
  'prose',
  'json-strings',
  'status-catalog',
  'source-strings',
]);

const C_LIKE_EXTENSIONS = new Set(['.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs', '.rs', '.swift']);
const RUST_LIKE_EXTENSIONS = new Set(['.rs', '.swift']);

/**
 * Which file extensions each mode can actually read.
 *
 * This is the guard against the worst failure this tool has: a surface whose
 * mode cannot see its own files. The glob
 * `apps/desktop/src/strings/(**)/*.{json,ts}` in `json-strings` mode passed
 * every check and reported clean forever, because the JSON extractor only
 * recognises double-quoted literals — a `.ts` file written in idiomatic single
 * quotes or template literals was scanned to nothing. Nothing failed; the
 * surface simply stopped being linted, which is the one outcome this tool
 * exists to prevent.
 *
 * The manifest is cross-validated against this table at load, so a mode that
 * cannot read its own extension is a startup error, not a silent hole.
 */
export const MODE_EXTENSIONS = Object.freeze({
  prose: Object.freeze(['.md', '.txt']),
  'json-strings': Object.freeze(['.json']),
  'status-catalog': Object.freeze(['.json']),
  'source-strings': Object.freeze([...C_LIKE_EXTENSIONS]),
});

/**
 * Split a document into the segments the vocabulary rules scan.
 * @returns {Segment[]}
 */
export function extractSegments(text, mode, extension) {
  switch (mode) {
    case 'prose':
      return [{ text, offset: 0, line: 1, column: 1 }];
    case 'json-strings':
    case 'status-catalog':
      return extractJsonValues(text);
    case 'source-strings':
      return extractSourceLiterals(text, extension);
    default:
      throw new Error(`unknown surface mode "${mode}" (expected one of ${MODES.join(', ')})`);
  }
}

/**
 * JSON string *values* — a literal immediately followed by `:` is a key and is
 * an identifier, not copy.
 * @returns {Segment[]}
 */
export function extractJsonValues(text) {
  const starts = lineStarts(text);
  const segments = [];
  const literal = /"(?:[^"\\]|\\.)*"/g;
  let match = literal.exec(text);
  while (match !== null) {
    const after = text.slice(match.index + match[0].length);
    const isKey = /^\s*:/.test(after);
    if (!isKey) {
      const inner = match[0].slice(1, -1);
      const offset = match.index + 1;
      segments.push({ text: decodeJsonEscapes(inner), offset, ...positionAt(starts, offset) });
    }
    match = literal.exec(text);
  }
  return segments;
}

function decodeJsonEscapes(inner) {
  try {
    return JSON.parse(`"${inner}"`);
  } catch {
    return inner;
  }
}

/**
 * String literals in C-like source, with comments excluded.
 * Rust and Swift use `'` for characters and lifetimes, so only `"` opens a
 * string there; TypeScript and JavaScript also use `'` and backticks.
 * @returns {Segment[]}
 */
export function extractSourceLiterals(text, extension) {
  if (!C_LIKE_EXTENSIONS.has(extension)) {
    throw new Error(`mode "source-strings" does not handle "${extension}" files`);
  }
  const starts = lineStarts(text);
  const quotes = RUST_LIKE_EXTENSIONS.has(extension) ? new Set(['"']) : new Set(['"', "'", '`']);
  const segments = [];
  let i = 0;
  while (i < text.length) {
    const char = text[i];
    const next = text[i + 1];
    if (char === '/' && next === '/') {
      const end = text.indexOf('\n', i);
      i = end === -1 ? text.length : end;
    } else if (char === '/' && next === '*') {
      const end = text.indexOf('*/', i + 2);
      i = end === -1 ? text.length : end + 2;
    } else if (quotes.has(char)) {
      const closed = readLiteral(text, i, char);
      const offset = i + 1;
      segments.push({
        text: closed.value,
        offset,
        ...positionAt(starts, offset),
      });
      i = closed.end;
    } else {
      i += 1;
    }
  }
  return segments;
}

function readLiteral(text, start, quote) {
  let i = start + 1;
  let value = '';
  while (i < text.length) {
    const char = text[i];
    if (char === '\\') {
      value += text[i + 1] ?? '';
      i += 2;
      continue;
    }
    if (char === quote) return { value, end: i + 1 };
    if (char === '\n' && quote !== '`') return { value, end: i + 1 };
    value += char;
    i += 1;
  }
  return { value, end: text.length };
}

/**
 * Explicit suppression regions, honoured in `prose` mode only.
 *
 *   <!-- vocab-lint:allow reason="quotes plan §3.5 verbatim" -->
 *   ...
 *   <!-- vocab-lint:end -->
 *
 * A region is legal in exactly two situations: the document must *name* a
 * forbidden term in order to forbid it, or it must name an external system's
 * own vocabulary. Every region is reported, every region needs a reason, and
 * each surface declares a hard budget — suppression that can grow silently is
 * not suppression, it is erosion.
 *
 * @returns {{ regions: {startLine:number,endLine:number,reason:string}[], errors: {line:number,message:string}[] }}
 */
export function findAllowRegions(text) {
  const lines = text.split('\n');
  const regions = [];
  const errors = [];
  let open = null;
  lines.forEach((raw, index) => {
    const line = index + 1;
    const allow = raw.match(/<!--\s*vocab-lint:allow(.*?)-->/);
    const end = raw.match(/<!--\s*vocab-lint:end\s*-->/);
    if (allow) {
      if (open) {
        errors.push({ line, message: 'nested vocab-lint:allow region' });
        return;
      }
      const reason = (allow[1].match(/reason\s*=\s*"([^"]*)"/) || [])[1] || '';
      if (reason.trim().length === 0) {
        errors.push({ line, message: 'vocab-lint:allow needs a non-empty reason="…"' });
      }
      open = { startLine: line, reason: reason.trim() };
      return;
    }
    if (end) {
      if (!open) {
        errors.push({ line, message: 'vocab-lint:end without a matching allow region' });
        return;
      }
      regions.push({ ...open, endLine: line });
      open = null;
    }
  });
  if (open) {
    errors.push({ line: open.startLine, message: 'vocab-lint:allow region is never closed' });
  }
  return { regions, errors };
}

/** Is `line` covered by an allow region? */
export function isAllowed(regions, line) {
  return regions.some((region) => line >= region.startLine && line <= region.endLine);
}
