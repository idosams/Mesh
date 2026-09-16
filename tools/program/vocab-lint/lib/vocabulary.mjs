/**
 * The product vocabulary.
 *
 * Normative source: `docs/product-prd.md` §4 (the user-visible state model —
 * §4.1–4.2 the mapping, §4.3 the six status words, §4.4 the forbidden terms) and
 * `docs/charter.md` §5. The nine forbidden terms are correct — and required —
 * in the protocol and consistency documents. They are forbidden in *product
 * surfaces*: labels, buttons, status values, error messages, notifications,
 * onboarding copy, CLI help text and the product documentation a user reads.
 *
 * Every entry carries the wording to use instead, because a lint that only says
 * "no" teaches nobody. The replacement column is quoted verbatim in the failure
 * message.
 */

/**
 * @typedef {object} ForbiddenTerm
 * @property {string} id        stable identifier, used by fixtures and reports
 * @property {string} term      the term as the charter writes it
 * @property {string} pattern   RegExp source, applied case-insensitively
 * @property {string[]} matches the inflections the pattern is meant to catch
 * @property {string[]} allows  near misses the pattern must NOT catch
 * @property {string} sayInstead the approved wording
 */

/** @type {readonly ForbiddenTerm[]} */
export const FORBIDDEN_TERMS = Object.freeze([
  {
    id: 'dag',
    term: 'DAG',
    pattern: '\\bdags?\\b',
    matches: ['DAG', 'DAGs', 'dag'],
    allows: ['dagger', 'adage'],
    sayInstead: 'version history',
  },
  {
    id: 'frontier',
    term: 'frontier',
    pattern: '\\bfrontiers?\\b',
    matches: ['frontier', 'frontiers'],
    allows: ['front', 'frontal'],
    sayInstead: 'their work',
  },
  {
    id: 'vector-clock',
    term: 'vector clock',
    pattern: '\\bvector[\\s-]*clocks?\\b',
    matches: ['vector clock', 'vector clocks', 'vector-clock'],
    allows: ['vector graphics', 'clock'],
    sayInstead: 'up to date / behind the shared version',
  },
  {
    id: 'branch',
    term: 'branch',
    pattern: '\\bbranch(es|ed|ing)?\\b',
    matches: ['branch', 'branches', 'branched', 'branching'],
    allows: ['branchless'],
    sayInstead: 'their work',
  },
  {
    id: 'commit',
    term: 'commit',
    pattern: '\\bcommit(s|ted|ting)?\\b',
    matches: ['commit', 'commits', 'committed', 'committing'],
    allows: ['commitment', 'committee'],
    sayInstead: 'saved privately',
  },
  {
    id: 'rebase',
    term: 'rebase',
    pattern: '\\brebas(e|es|ed|ing)\\b',
    matches: ['rebase', 'rebases', 'rebased', 'rebasing'],
    allows: ['base', 'database'],
    sayInstead: 'bring onto the current shared version',
  },
  {
    id: 'staging',
    term: 'staging',
    pattern: '\\bstaging\\b',
    matches: ['staging'],
    allows: ['stage', 'staged rollout'],
    sayInstead: 'ready for review',
  },
  {
    id: 'ref',
    term: 'ref',
    pattern: '\\brefs?\\b',
    matches: ['ref', 'refs'],
    allows: ['reference', 'refactor', 'prefer', 'referring'],
    sayInstead: 'version',
  },
  {
    id: 'operation-log',
    term: 'operation log',
    pattern: '\\b(operation[\\s-]*logs?|op[\\s-]?logs?)\\b',
    matches: ['operation log', 'operation logs', 'oplog', 'op-log'],
    allows: ['log', 'logging', 'operations'],
    sayInstead: 'activity',
  },
]);

/**
 * The six user-facing status words, in presentation order. A concept that does
 * not fit here is a product design question for the product owner — never a
 * seventh word added locally (charter §5).
 */
export const APPROVED_STATUS = Object.freeze([
  'Working',
  'Saved privately',
  'Available to team',
  'Ready for review',
  'Needs attention',
  'Approved',
]);

/**
 * The user-facing wordings that name a *thing* rather than a status: the four
 * nouns and one verb phrase the state mapping (PRD §4.1–4.2) is allowed to use
 * in its user-facing column alongside the six status words.
 *
 * Together with `APPROVED_STATUS` this closes the mapping's user-facing
 * vocabulary. Without a closed set the mapping can quietly invent wording that
 * competes with the six — "Needs review" next to "Needs attention" and "Ready
 * for review" — and no rule notices, because each table is internally
 * consistent. The `mapping` rule cross-checks against this union, so §4.1 and
 * §4.3 cannot drift apart.
 */
export const APPROVED_OBJECT_WORDING = Object.freeze([
  'Their work',
  'Shared version',
  'Approve to shared version',
  'Earlier version',
]);

/** The closed user-facing vocabulary of the state mapping: statuses ∪ objects. */
export const APPROVED_MAPPING_WORDING = Object.freeze([
  ...APPROVED_STATUS,
  ...APPROVED_OBJECT_WORDING,
]);

/**
 * Units of work a user must never be asked to create (charter P1). The journey
 * rule rejects any step performed by the user that mentions one of these.
 */
export const CEREMONY_NOUNS = Object.freeze([
  'task',
  'branch',
  'commit',
  'checkpoint',
  'proposal',
  'merge request',
  'pull request',
  'worktree',
]);

/** Build a fresh global matcher — never share a `g` RegExp across scans. */
export function matcherFor(term) {
  return new RegExp(term.pattern, 'gi');
}

/** Build a fresh global matcher over the ceremony nouns. */
export function ceremonyMatcher() {
  const alternation = CEREMONY_NOUNS.map((noun) => noun.replace(/ /g, '\\s+')).join('|');
  return new RegExp(`\\b(${alternation})s?\\b`, 'gi');
}

/** @returns {ForbiddenTerm|undefined} */
export function termById(id) {
  return FORBIDDEN_TERMS.find((term) => term.id === id);
}
