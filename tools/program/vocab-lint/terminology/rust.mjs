/**
 * Rust source parsing for TL-5, pure over text.
 *
 * Nothing here touches the filesystem, so the self-test can hand these functions
 * a synthetic crate and observe the same code the repository scan runs. The two
 * shapes a crate root uses to make an item public are both read here:
 *
 * 1. a declaration at the crate root — `pub struct Digest32(…)` in `src/lib.rs`;
 * 2. a re-export at the crate root — `pub use crate::digest::Digest32;`, where
 *    the item is declared in a private module and the crate root republishes it.
 *
 * Shape 2 is the one `mesh-types` uses for every one of its items, so a reach
 * limited to shape 1 saw exactly one of that crate's forty-nine public names.
 *
 * **Module names are still not items.** `pub mod` is deliberately absent from
 * both readers. `mesh-bench` declares twelve public modules — `clock`, `json`,
 * `stats`, `testing` — whose names are internal benchmark plumbing; registering
 * them would put twelve ordinary English words into a register whose entire
 * purpose is that one word carries one protocol meaning. §6 of
 * `docs/protocol.md` states that exclusion and what it still costs.
 */

/**
 * The qualifier sequence Rust allows between `pub` and the item keyword:
 * `async`, `unsafe`, `const`, and `extern` with or without an ABI string. The
 * sequence is optional and repeatable, because `pub const unsafe fn` and
 * `pub unsafe extern "C" fn` are both legal.
 *
 * It has to be optional *and* backtrackable: `pub const CRATE_NAME` is a
 * constant whose item keyword is the same word this group would swallow, so the
 * group gives `const` back when no item keyword follows it. `impl` is
 * deliberately absent — an `impl` block is never `pub` and publishes no name of
 * its own.
 */
const QUALIFIER = '(?:async|unsafe|const|extern(?:\\s+"[^"\\n]*")?)\\s+';

/** The keywords that introduce a *named* item. `mod` is deliberately absent;
 *  §6 of `docs/protocol.md` excludes module names from the register. */
const ITEM_KEYWORD = '(?:const|static|fn|struct|enum|trait|type|union)';

/** A public item declared at the crate root. Column-anchored on purpose: an
 *  indented declaration sits inside an inline module and is not crate-public
 *  unless that module is, and `pub(crate)` never matches because a visibility
 *  qualifier follows `pub` immediately.
 *
 *  Before the qualifier sequence was admitted, `pub async fn`, `pub unsafe fn`,
 *  `pub extern "C" fn` and `pub unsafe trait` were invisible to TL-5 outright,
 *  and `pub const fn no_session()` was read as an item literally named `fn` —
 *  the wrong name demanded and the real one never asked for. */
const DECLARED_ITEM = new RegExp(
  `^pub\\s+(?:${QUALIFIER})*${ITEM_KEYWORD}\\s+([A-Za-z_][A-Za-z0-9_]*)`,
  'gm',
);

/** The head of a crate-root re-export, anchored for the same reason. */
const RE_EXPORT = /^pub\s+use\s+/gm;

const IDENTIFIER = /^[A-Za-z_][A-Za-z0-9_]*$/;

/** Path prefixes that name the current crate rather than a module inside it. */
const CRATE_ROOT_SEGMENTS = new Set(['crate', 'self']);

/** 1-indexed line number containing `index`. */
export function lineOf(source, index) {
  let line = 1;
  for (let at = 0; at < index && at < source.length; at += 1) {
    if (source[at] === '\n') line += 1;
  }
  return line;
}

/** Line and block comments blanked, so a comment inside a use tree is not read
 *  as a path segment. Length is preserved; offsets stay usable. */
function withoutComments(text) {
  return text
    .replace(/\/\*[\s\S]*?\*\//g, (match) => match.replace(/[^\n]/g, ' '))
    .replace(/\/\/[^\n]*/g, (match) => ' '.repeat(match.length));
}

/** Every public item the crate root declares itself. */
export function declaredItems(source) {
  const found = [];
  for (const match of withoutComments(source).matchAll(DECLARED_ITEM)) {
    found.push({ name: match[1], line: lineOf(source, match.index) });
  }
  return found;
}

/**
 * The line a named item is declared on, or 0 when this source does not declare
 * it — which happens when a module re-exports an item of its own submodule.
 */
export function declarationLine(source, name) {
  for (const item of declaredItems(source)) {
    if (item.name === name) return item.line;
  }
  return 0;
}

/** Index of the `;` closing the use statement that starts at `from`, or -1. */
function statementEnd(source, from) {
  let depth = 0;
  for (let at = from; at < source.length; at += 1) {
    const character = source[at];
    if (character === '{') depth += 1;
    else if (character === '}') depth -= 1;
    else if (character === ';' && depth === 0) return at;
  }
  return -1;
}

/** Index of `character` at brace depth zero, or -1. */
function indexOfTopLevel(text, character) {
  let depth = 0;
  for (let at = 0; at < text.length; at += 1) {
    if (text[at] === '{') {
      if (depth === 0 && character === '{') return at;
      depth += 1;
    } else if (text[at] === '}') depth -= 1;
    else if (depth === 0 && text[at] === character) return at;
  }
  return -1;
}

/** Index of the `}` matching the `{` at `open`, or -1. */
function matchingBrace(text, open) {
  let depth = 0;
  for (let at = open; at < text.length; at += 1) {
    if (text[at] === '{') depth += 1;
    else if (text[at] === '}') {
      depth -= 1;
      if (depth === 0) return at;
    }
  }
  return -1;
}

/** Split on `separator` at brace depth zero. */
function splitTopLevel(text, separator) {
  const parts = [];
  let depth = 0;
  let start = 0;
  for (let at = 0; at < text.length; at += 1) {
    if (text[at] === '{') depth += 1;
    else if (text[at] === '}') depth -= 1;
    else if (text[at] === separator && depth === 0) {
      parts.push(text.slice(start, at));
      start = at + 1;
    }
  }
  parts.push(text.slice(start));
  return parts;
}

function segmentsOf(pathText) {
  return pathText
    .split('::')
    .map((segment) => segment.trim())
    .filter((segment) => segment !== '');
}

/**
 * One leaf of a use tree.
 *
 * @returns {{name: string, declaredAs: string, path: string[]}
 *   | {glob: true, path: string[]} | null} `null` for a leaf that publishes no
 *   name of its own: `self` republishes a module, whose name §6 excludes, and
 *   `as _` publishes a trait's methods without publishing a name at all.
 */
function leafOf(prefix, text) {
  const [pathText, aliasText] = renamed(text);
  const segments = [...prefix, ...segmentsOf(pathText)];
  while (segments.length > 0 && CRATE_ROOT_SEGMENTS.has(segments[0])) segments.shift();
  const last = segments.pop();
  if (last === undefined) return null;
  if (last === '*') return { glob: true, path: segments };
  if (last === 'self') return null;
  const name = aliasText ?? last;
  if (name === '_' || !IDENTIFIER.test(name)) return null;
  return { name, declaredAs: last, path: segments };
}

/** `a::b::C as D` split into its path and its new name. */
function renamed(text) {
  const match = /^([\s\S]*?)\s+as\s+([A-Za-z_][A-Za-z0-9_]*)\s*$/.exec(text.trim());
  return match ? [match[1], match[2]] : [text, null];
}

/** The leaves of one use tree, with `prefix` already consumed. */
function useTree(text, prefix) {
  const trimmed = text.trim();
  if (trimmed === '') return [];
  const open = indexOfTopLevel(trimmed, '{');
  if (open === -1) {
    const leaf = leafOf(prefix, trimmed);
    return leaf ? [leaf] : [];
  }
  const close = matchingBrace(trimmed, open);
  if (close === -1) return [];
  const head = trimmed.slice(0, open).replace(/::\s*$/, '');
  const nested = [...prefix, ...segmentsOf(head)];
  return splitTopLevel(trimmed.slice(open + 1, close), ',').flatMap((part) =>
    useTree(part, nested),
  );
}

/**
 * Every name a crate root republishes, and every glob that refuses to say which
 * names it republishes.
 *
 * @returns {Array<{name?: string, declaredAs?: string, glob?: boolean,
 *   path: string[], line: number}>} `path` is the module path the item is
 *   declared under, relative to the crate root and with `crate`/`self` removed.
 */
export function reExports(source) {
  const text = withoutComments(source);
  const found = [];
  for (const match of text.matchAll(RE_EXPORT)) {
    const start = match.index + match[0].length;
    const end = statementEnd(text, start);
    if (end === -1) continue;
    const line = lineOf(source, match.index);
    for (const leaf of useTree(text.slice(start, end), [])) found.push({ ...leaf, line });
  }
  return found;
}
