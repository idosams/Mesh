/**
 * TL-1 … TL-10 — the terminology checks.
 *
 * Each check is a pure function from the model to a list of findings, and each
 * one names the offending term, its file and its line. The registry at the
 * bottom is the seam: adding a rule means adding a module-level function and one
 * registry entry, never editing the ones already there.
 */

import { REGISTER_DOC, VALID_GRAPHS, isStructuralToken } from './model.mjs';

function finding(id, doc, line, message) {
  return { id, doc, line, message };
}

function termSet(model) {
  return new Set(model.register.map((row) => row.term));
}

/** TL-1 — every register term is unique across the whole register. */
export function checkUniqueTerms(model) {
  const seen = new Map();
  const findings = [];
  for (const row of model.register) {
    const first = seen.get(row.term);
    if (first === undefined) {
      seen.set(row.term, row.line);
      continue;
    }
    findings.push(
      finding('TL-1', row.doc, row.line, `\`${row.term}\` is defined twice — first at line ${first}`),
    );
  }
  return findings;
}

/** TL-2 — every register row has a definition, a valid graph and a home. */
export function checkRowShape(model) {
  const findings = [];
  for (const row of model.register) {
    if (row.definition.replace(/\s/g, '') === '') {
      findings.push(finding('TL-2', row.doc, row.line, `\`${row.term}\` has an empty definition`));
    }
    if (!VALID_GRAPHS.includes(row.graph.replace(/`/g, '').trim())) {
      findings.push(
        finding(
          'TL-2',
          row.doc,
          row.line,
          `\`${row.term}\` has graph "${row.graph}", not one of ${VALID_GRAPHS.join(', ')}`,
        ),
      );
    }
    if (row.home === '') {
      findings.push(finding('TL-2', row.doc, row.line, `\`${row.term}\` has an empty home`));
    }
  }
  return findings;
}

/** TL-3 — every alias resolves to a register term and is not a term itself. */
export function checkAliases(model) {
  const terms = termSet(model);
  const findings = [];
  for (const row of model.aliases) {
    if (!terms.has(row.resolvesTo)) {
      findings.push(
        finding('TL-3', row.doc, row.line, `alias \`${row.alias}\` resolves to \`${row.resolvesTo}\`, which is not a register term`),
      );
    }
    if (terms.has(row.alias)) {
      findings.push(
        finding('TL-3', row.doc, row.line, `\`${row.alias}\` is both an alias and a register term — one of the two is a second definition`),
      );
    }
  }
  return findings;
}

/** TL-4 — every directory under `crates/` appears as a register term. */
export function checkCrateNames(model) {
  const terms = termSet(model);
  return model.crates
    .filter((crate) => !terms.has(crate))
    .map((crate) => finding('TL-4', REGISTER_DOC, 0, `crate \`${crate}\` has no register row`));
}

/**
 * TL-5 — every public item exported from a crate appears as a register term.
 *
 * "Exported" covers both shapes a crate root uses to publish a name: an item
 * declared there, and an item declared in a private module and re-exported from
 * there. A glob re-export names nothing, so it is reported rather than expanded
 * — expanding it would let a crate's public surface change without `src/lib.rs`
 * changing, which is exactly the silent register drift this gate exists to stop.
 */
export function checkPublicItems(model) {
  const terms = termSet(model);
  const findings = model.publicItems
    .filter((item) => !terms.has(item.name))
    .map((item) =>
      finding('TL-5', item.file, item.line ?? 0, `public item \`${item.name}\` has no register row`),
    );
  for (const glob of model.wildcardExports ?? []) {
    findings.push(
      finding(
        'TL-5',
        glob.file,
        glob.line,
        `\`${glob.path}\` re-exports a whole module, so what it publishes cannot be enumerated — re-export each item by name`,
      ),
    );
  }
  return findings;
}

/** TL-6 — a crate-name term's home names that crate's own directory. */
export function checkCrateHomes(model) {
  const crates = new Set(model.crates);
  return model.register
    .filter((row) => crates.has(row.term))
    .filter((row) => row.home !== `crates/${row.term}`)
    .map((row) =>
      finding('TL-6', row.doc, row.line, `\`${row.term}\` has home "${row.home}", expected "crates/${row.term}"`),
    );
}

/**
 * TL-7 — no definition outside the register.
 *
 * A `### 1.2 Some concept — gloss` heading introduces a concept. If the concept
 * is not a register term, the section that follows is a second definition living
 * outside the register, which is exactly the failure the register exists to
 * prevent.
 */
export function checkHeadingsAreTerms(model) {
  const known = new Set(model.register.map((row) => row.term.toLowerCase()));
  for (const row of model.aliases) known.add(row.alias.toLowerCase());
  return model.headings
    .filter((heading) => !known.has(heading.name.toLowerCase()))
    .map((heading) =>
      finding('TL-7', heading.doc, heading.line, `section defines "${heading.name}", which is not a register term`),
    );
}

/**
 * TL-8 — no undefined term in prose.
 *
 * Every backticked token in the two documents resolves to a register term, an
 * alias, a declared member value, or a literal declared in the non-terms table.
 * Repository paths, CLI flags, HTML markers and entity IDs are structural and
 * are never asked to resolve.
 */
export function checkTokensResolve(model) {
  const known = termSet(model);
  for (const row of model.aliases) known.add(row.alias);
  for (const row of model.nonTerms) known.add(row.literal);
  for (const member of model.members) known.add(member.value);

  const reported = new Set();
  const findings = [];
  for (const token of model.tokens) {
    if (isStructuralToken(token.text) || known.has(token.text)) continue;
    const seen = `${token.doc}:${token.text}`;
    if (reported.has(seen)) continue;
    reported.add(seen);
    findings.push(
      finding('TL-8', token.doc, token.line, `\`${token.text}\` is used as a term but has no register row, alias or non-term declaration`),
    );
  }
  return findings;
}

/**
 * TL-9 — no member value carries two meanings.
 *
 * `head state` and `availability state` once both enumerated `local only`,
 * `metadata replicated` and `content available`, which gave three words two
 * meanings inside the document whose whole job is to stop that. A member value
 * belongs to exactly one enumerated term.
 */
export function checkMemberValues(model) {
  const owners = new Map();
  for (const member of model.members) {
    if (!owners.has(member.value)) owners.set(member.value, []);
    owners.get(member.value).push(member);
  }
  const findings = [];
  for (const [value, holders] of owners) {
    const distinct = [...new Set(holders.map((holder) => holder.term))];
    if (distinct.length < 2) continue;
    const last = holders[holders.length - 1];
    findings.push(
      finding('TL-9', last.doc, last.line, `member value \`${value}\` is enumerated by ${distinct.map((term) => `\`${term}\``).join(' and ')} — one value, two meanings`),
    );
  }
  return findings;
}

/** A home cell naming exactly one crate directory. `crates/*` is deliberately
 *  not matched: it is the home of an item every crate publishes, not of an item
 *  one crate publishes. */
const CRATE_HOME = /^crates\/([A-Za-z0-9_][A-Za-z0-9_-]*)$/;

/**
 * TL-10 — every register row homed at one crate names an item that crate publishes.
 *
 * TL-5 walks public items and demands rows. Nothing walked rows and demanded
 * items, so a row for an item that was renamed or deleted — or one that never
 * existed — sat in the register and every gate stayed green. That is the mirror
 * image of the drift TL-5 exists to catch, and a register with a row nothing
 * backs is worse than a missing row: it reads as a specification.
 *
 * The home cell is what makes the direction decidable. A row homed `crates/*`
 * is claimed by every crate, and a row homed at a document or at a bare crate
 * name is a concept rather than a published item; neither is asked to name one.
 * A crate-name row is skipped too — TL-4 and TL-6 own that column.
 */
export function checkRegisteredItemsExist(model) {
  const crates = new Set(model.crates);
  const published = new Map();
  for (const item of model.publicItems) {
    if (!published.has(item.crate)) published.set(item.crate, new Set());
    published.get(item.crate).add(item.name);
  }

  const findings = [];
  for (const row of model.register) {
    const match = CRATE_HOME.exec(row.home);
    if (match === null) continue;
    const crate = match[1];
    if (!crates.has(crate) || crates.has(row.term)) continue;
    if (published.get(crate)?.has(row.term)) continue;
    findings.push(
      finding(
        'TL-10',
        row.doc,
        row.line,
        `\`${row.term}\` is registered as a public item of \`crates/${crate}\`, which publishes no item of that name`,
      ),
    );
  }
  return findings;
}

export const CHECKS = Object.freeze([
  { id: 'TL-1', title: 'Register terms are unique', run: checkUniqueTerms },
  { id: 'TL-2', title: 'Register rows are well formed', run: checkRowShape },
  { id: 'TL-3', title: 'Aliases resolve and carry no definition', run: checkAliases },
  { id: 'TL-4', title: 'Every crate name is a term', run: checkCrateNames },
  { id: 'TL-5', title: 'Every public item is a term', run: checkPublicItems },
  { id: 'TL-6', title: 'Crate terms name their own directory', run: checkCrateHomes },
  { id: 'TL-7', title: 'No definition outside the register', run: checkHeadingsAreTerms },
  { id: 'TL-8', title: 'No undefined term in prose', run: checkTokensResolve },
  { id: 'TL-9', title: 'No member value carries two meanings', run: checkMemberValues },
  { id: 'TL-10', title: 'Every registered public item exists', run: checkRegisteredItemsExist },
]);

export const CHECK_IDS = Object.freeze(CHECKS.map((check) => check.id));

/** @returns {Array<{id: string, doc: string, line: number, message: string}>} */
export function runChecks(model, only = null) {
  const selected = only ? CHECKS.filter((check) => only.includes(check.id)) : CHECKS;
  return selected.flatMap((check) => check.run(model));
}
