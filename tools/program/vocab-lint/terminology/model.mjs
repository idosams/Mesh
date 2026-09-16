/**
 * The terminology model: everything the checks need, in one plain object.
 *
 * Building the model is the only step that touches the filesystem. The checks
 * take the model and nothing else, which is what lets the self-test hand them a
 * deliberately broken model and assert that each check fires.
 */

import path from 'node:path';

import { repositoryFiles, scanCrates } from './crates.mjs';
import {
  aliasRows,
  definitionHeadings,
  membersOf,
  nonTermRows,
  readDocument,
  registerRows,
  tokens,
} from './parse.mjs';

/** Documents the lint reads. The first one carries the register. */
export const REGISTER_DOC = 'docs/protocol.md';
export const COMPANION_DOCS = Object.freeze([REGISTER_DOC, 'docs/consistency.md']);

export const VALID_GRAPHS = Object.freeze(['state', 'operation', 'dependency', 'trust', '—']);

const ULID = /^[0-9A-HJKMNP-TV-Z]{26}$/;

/**
 * Tokens the lint never asks to resolve, because they are not words about the
 * protocol: repository paths, CLI flags, HTML markers and entity IDs. Anything
 * outside these shapes must be a term, an alias, a declared member value or a
 * declared non-term literal.
 */
export function isStructuralToken(token) {
  if (token.includes('/')) return true;
  if (token.startsWith('--')) return true;
  if (token.startsWith('<!--')) return true;
  return ULID.test(token);
}

/** @throws {Error} when a required marker block is missing — a usage error, not a finding. */
export function buildModel(root) {
  const docs = COMPANION_DOCS.map((relative) =>
    readDocument(path.join(root, relative), relative),
  );
  return buildModelFromDocuments(docs, scanCrates(repositoryFiles(root)));
}

/**
 * The model over already-parsed documents. The self-test uses this directly with
 * synthetic documents, so every check is exercised without touching the repo.
 *
 * @throws {Error} when a required marker block is missing.
 */
export function buildModelFromDocuments(
  docs,
  { crates = [], publicItems: items = [], wildcardExports = [] } = {},
) {
  const registerDoc = docs[0];

  const register = registerRows(registerDoc);
  const aliases = aliasRows(registerDoc);
  const nonTerms = nonTermRows(registerDoc);
  for (const [name, rows] of [
    ['terminology', register],
    ['aliases', aliases],
    ['non-terms', nonTerms],
  ]) {
    if (rows === null) {
      throw new Error(`${REGISTER_DOC}: missing <!-- ${name}:begin --> / <!-- ${name}:end --> block`);
    }
  }

  const members = [];
  for (const row of register) {
    for (const value of membersOf(row.definition)) {
      members.push({ value, term: row.term, line: row.line, doc: row.doc });
    }
  }

  return {
    register,
    aliases,
    nonTerms,
    members,
    crates,
    publicItems: items,
    wildcardExports,
    headings: docs.flatMap(definitionHeadings),
    tokens: docs.flatMap(tokens),
  };
}
