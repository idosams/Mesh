/**
 * A minimal glob matcher and repository walker. No dependencies, by policy:
 * this lint runs in CI, in the desktop build and on a developer laptop with
 * nothing installed but Node.
 */

import fs from 'node:fs';
import path from 'node:path';

const REGEX_SPECIAL = /[.+^${}()|[\]\\]/g;

/** Directories that never contain a user-facing surface. */
export const SKIPPED_DIRS = Object.freeze([
  '.git',
  'node_modules',
  'target',
  'vendor',
  'graphify-out',
  'dist',
  'build',
]);

function escapeLiteral(text) {
  return text.replace(REGEX_SPECIAL, '\\$&');
}

/**
 * Translate a POSIX-style glob into an anchored RegExp.
 * Supports `**`, `*`, `?` and `{a,b}`; `**` never crosses into a skipped tree
 * because the walker filters those before matching.
 */
export function globToRegExp(glob) {
  if (typeof glob !== 'string' || glob.length === 0) {
    throw new TypeError('glob must be a non-empty string');
  }
  let source = '';
  let i = 0;
  while (i < glob.length) {
    const char = glob[i];
    if (char === '*' && glob[i + 1] === '*') {
      if (glob[i + 2] === '/') {
        source += '(?:[^/]+/)*';
        i += 3;
      } else {
        source += '.*';
        i += 2;
      }
    } else if (char === '*') {
      source += '[^/]*';
      i += 1;
    } else if (char === '?') {
      source += '[^/]';
      i += 1;
    } else if (char === '{') {
      const end = glob.indexOf('}', i);
      if (end === -1) {
        source += '\\{';
        i += 1;
      } else {
        const parts = glob
          .slice(i + 1, end)
          .split(',')
          .map((part) => escapeLiteral(part));
        source += `(?:${parts.join('|')})`;
        i = end + 1;
      }
    } else {
      source += escapeLiteral(char);
      i += 1;
    }
  }
  return new RegExp(`^${source}$`);
}

/**
 * Expand every `{a,b}` alternative in a glob into the concrete patterns it
 * stands for. `**\/*.{json,ts}` becomes two patterns, which is what lets the
 * manifest ask "can this surface's mode actually read every file this glob can
 * match?" before any file exists to prove it cannot.
 * @returns {string[]}
 */
export function expandBraces(glob) {
  const open = glob.indexOf('{');
  if (open === -1) return [glob];
  const close = glob.indexOf('}', open);
  if (close === -1) return [glob];
  const alternatives = glob.slice(open + 1, close).split(',');
  const prefix = glob.slice(0, open);
  const suffix = glob.slice(close + 1);
  return alternatives.flatMap((alternative) => expandBraces(`${prefix}${alternative}${suffix}`));
}

/**
 * The set of file extensions a glob can match, or `null` when the glob's
 * extension is itself a wildcard (`src/**` , `*.*`) and therefore unbounded.
 * @returns {string[]|null}
 */
export function globExtensions(glob) {
  const extensions = new Set();
  for (const expanded of expandBraces(glob)) {
    const basename = expanded.slice(expanded.lastIndexOf('/') + 1);
    const dot = basename.lastIndexOf('.');
    if (dot <= 0) return null;
    const extension = basename.slice(dot);
    if (extension.includes('*') || extension.includes('?')) return null;
    extensions.add(extension);
  }
  return [...extensions].sort();
}

/**
 * Walk `root` and return every file as a repository-relative POSIX path,
 * sorted so that reports are deterministic.
 */
export function walkFiles(root) {
  const found = [];
  const visit = (absolute, relative) => {
    let entries;
    try {
      entries = fs.readdirSync(absolute, { withFileTypes: true });
    } catch (error) {
      throw new Error(`cannot read directory ${absolute}: ${error.message}`);
    }
    for (const entry of entries) {
      if (entry.isSymbolicLink()) continue;
      const childRelative = relative ? `${relative}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        if (SKIPPED_DIRS.includes(entry.name)) continue;
        visit(path.join(absolute, entry.name), childRelative);
      } else if (entry.isFile()) {
        found.push(childRelative);
      }
    }
  };
  visit(root, '');
  return found.sort();
}

/** Every repository-relative file under `root` matching `glob`. */
export function matchFiles(root, glob) {
  const pattern = globToRegExp(glob);
  return walkFiles(root).filter((relative) => pattern.test(relative));
}
