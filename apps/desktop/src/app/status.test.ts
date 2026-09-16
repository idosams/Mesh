// The completeness test: every internal state has a wording, and the words are the product's.
//
// # What it binds to
//
// `docs/product-prd.md` is the normative source for the user-visible state model — §4.1 the
// mapping from an internal state to the wording, §4.3 the six status words and their order. This
// test reads those two tables and compares them with `src/strings/status.json` in BOTH
// directions. One direction alone is half a check: a state in the product model with no wording
// here is a fallback string waiting to happen, and a wording here with no row in the product
// model is a word nobody approved.
//
// Reading a document from the repository root at test time is the same cross-tree read
// `src/ipc/contract.test.ts` already makes of `crates/mesh-daemon/ipc-contract.json`, and for the
// same reason: the owner of the fact publishes one file, and everyone who depends on it checks
// itself against that file rather than keeping a copy.
//
// # What it does not check
//
// That the six words are the RIGHT six. That is `tools/program/vocab-lint`, which holds
// `APPROVED_STATUS` and fails the desktop build on a seventh value in this catalogue. The chain
// is product requirements → this test → catalogue → lint, and every link in it is mechanical.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, it } from 'node:test';

import { ERROR_COPY } from '../strings/errors.ts';
import {
  INTERNAL_STATES,
  STATE_WORDINGS,
  UnmappedInternalState,
  USER_STATES,
  USER_STATUSES,
  isUserStatus,
  stateById,
  stateByStatus,
  statusFor,
  wordingFor,
} from './status.ts';

const HERE = dirname(fileURLToPath(import.meta.url));
const PRD = join(HERE, '..', '..', '..', '..', 'docs', 'product-prd.md');

/** One markdown table: its header cells and its body rows. */
type Table = { header: string[]; rows: string[][] };

const cells = (line: string): string[] =>
  line
    .trim()
    .replace(/^\|/, '')
    .replace(/\|$/, '')
    .split('|')
    .map((cell) => cell.trim().replace(/\*\*/g, '').replace(/`/g, '').trim());

/** The first table after `<!-- vocab-lint:<marker> -->`. */
const tableAfter = (text: string, marker: string): Table => {
  const lines = text.split('\n');
  const start = lines.findIndex((line) => line.includes(`<!-- vocab-lint:${marker} -->`));
  assert.notEqual(start, -1, `docs/product-prd.md has no <!-- vocab-lint:${marker} --> marker`);
  let at = start + 1;
  while (at < lines.length && !lines[at].trim().startsWith('|')) at += 1;
  assert.ok(at < lines.length, `no table follows the ${marker} marker`);
  const header = cells(lines[at]);
  const rows: string[][] = [];
  for (let i = at + 2; i < lines.length && lines[i].trim().startsWith('|'); i += 1) rows.push(cells(lines[i]));
  assert.ok(rows.length > 0, `the ${marker} table has no rows`);
  return { header, rows };
};

const column = (table: Table, name: string): number => {
  const index = table.header.indexOf(name);
  assert.notEqual(index, -1, `the table has no "${name}" column; it has ${table.header.join(', ')}`);
  return index;
};

const prd = readFileSync(PRD, 'utf8');

/** §4.1 — internal state to the wording a person reads, as the product requirements state it. */
const documented = (): Map<string, string> => {
  const table = tableAfter(prd, 'mapping-forward');
  const state = column(table, 'Internal state');
  const wording = column(table, 'User-facing wording');
  return new Map(table.rows.map((row) => [row[state] ?? '', row[wording] ?? '']));
};

/** §4.3 — the six status words, in the order the document presents them. */
const documentedStatuses = (): string[] => {
  const table = tableAfter(prd, 'six-state');
  const status = column(table, 'Status');
  return table.rows.map((row) => row[status] ?? '');
};

describe('the six status words', () => {
  it('are exactly the six the product requirements list, in that order', () => {
    assert.deepEqual([...USER_STATUSES], documentedStatuses());
  });

  it('are six, and no more', () => {
    assert.equal(USER_STATES.length, 6);
    assert.equal(new Set(USER_STATUSES).size, 6, 'two states share a status word');
  });

  it('have unique identifiers, and each is reachable both ways', () => {
    const ids = USER_STATES.map((state) => state.id);
    assert.equal(new Set(ids).size, ids.length, 'two states share an identifier');
    for (const state of USER_STATES) {
      assert.equal(stateById(state.id), state);
      assert.equal(stateByStatus(state.status), state);
      assert.ok(isUserStatus(state.status));
    }
  });

  it('each say what they mean and what to do about it', () => {
    for (const state of USER_STATES) {
      assert.ok(state.means.trim().length > 0, `${state.id} says nothing`);
      assert.ok(state.nextStep.trim().length > 0, `${state.id} offers no next step`);
      assert.ok(state.means.endsWith('.'), `${state.id} does not read as a sentence: ${state.means}`);
      assert.ok(state.nextStep.endsWith('.'), `${state.id} does not read as a sentence: ${state.nextStep}`);
    }
  });

  it('answers nothing for a word that is not one of them', () => {
    assert.equal(stateByStatus('Blocked'), undefined);
    assert.equal(stateById('blocked'), undefined);
    assert.equal(isUserStatus('Blocked'), false);
  });
});

describe('the mapping from an internal state to a wording', () => {
  it('covers every internal state the product requirements name', () => {
    const missing = [...documented().keys()].filter((state) => !INTERNAL_STATES.includes(state));
    assert.deepEqual(
      missing,
      [],
      `these internal states have a row in the product requirements and no wording in `
        + `src/strings/status.json, so this application would have to invent one: ${missing.join(', ')}`,
    );
  });

  it('invents no internal state the product requirements do not have', () => {
    const documentedStates = documented();
    const extra = INTERNAL_STATES.filter((state) => !documentedStates.has(state));
    assert.deepEqual(extra, [], `these wordings answer for a state nobody approved: ${extra.join(', ')}`);
  });

  it('uses the exact wording the product requirements approved for each one', () => {
    for (const [state, wording] of documented()) {
      assert.equal(wordingFor(state), wording, `the wording for "${state}" has drifted from the product requirements`);
    }
  });

  it('lists each internal state once', () => {
    assert.equal(new Set(INTERNAL_STATES).size, INTERNAL_STATES.length, 'an internal state is mapped twice');
    assert.equal(STATE_WORDINGS.length, INTERNAL_STATES.length);
  });

  it('answers a status entry where the wording is one of the six, and undefined where it names a thing', () => {
    for (const { internalState, wording } of STATE_WORDINGS) {
      const entry = statusFor(internalState);
      if (isUserStatus(wording)) assert.equal(entry?.status, wording);
      else assert.equal(entry, undefined, `"${wording}" is not a status and must not answer as one`);
    }
  });
});

describe('an internal state with no wording', () => {
  const unknown = 'Some state nobody has mapped';

  it('throws rather than answering with a fallback', () => {
    assert.throws(() => wordingFor(unknown), UnmappedInternalState);
    assert.throws(() => statusFor(unknown), UnmappedInternalState);
  });

  it('never returns the internal name, an empty string or a default word', () => {
    let answered: unknown = 'not thrown';
    try {
      answered = wordingFor(unknown);
    } catch (error) {
      answered = error;
    }
    assert.ok(answered instanceof UnmappedInternalState, `wordingFor answered ${String(answered)} instead of throwing`);
  });

  it('keeps the unmapped name out of the sentence and on the error', () => {
    const error = new UnmappedInternalState(unknown);
    assert.equal(error.internalState, unknown);
    assert.equal(error.message, ERROR_COPY.unmappedInternalState());
    assert.ok(
      !error.message.includes(unknown),
      'the message repeats a word this application has never seen, which is the one string the lint cannot check',
    );
  });
});
