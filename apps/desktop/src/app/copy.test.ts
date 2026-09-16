// A smoke alarm for the product vocabulary.
//
// `tools/program/vocab-lint/lint.mjs --user-facing` is the AUTHORITY on this rule: it owns the
// word list, the inflections, the near-misses each pattern must not catch, and the exemption
// budget of zero for every shipped source in this application. This file is not a second authority and does
// not try to be one. It exists so that a lane running `npm --prefix apps/desktop test` alone —
// with no tooling from the repository root in scope — finds out immediately, rather than at gate
// time, that a sentence picked up an internal word.
//
// It lives under `src/app/` rather than beside the copy it guards for one reason: `src/strings/`
// IS the linted surface, and a file whose whole job is to contain the nine banned terms would be
// a finding in it. Routing around a zero budget with a suppression directive is the exact move
// the zero budget exists to stop.
//
// It walks ERROR text and STATUS text as well as labels. A lint that covers the words on the
// buttons and not the sentence a person meets when something breaks covers the easy half: the
// error message is where the internal model leaks, because it is written in a hurry by whoever is
// fixing the fault. `ERROR_COPY` holds functions rather than strings, so each is called with a
// stand-in identifier and the finished sentence is what gets scanned.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { CONNECTION_COPY } from '../strings/connection.ts';
import { ERROR_COPY } from '../strings/errors.ts';
import { OPERATION_COPY } from '../strings/operations.ts';
import { USER_STATES } from './status.ts';

/** The nine terms banned from anything a person reads. */
const BANNED = [
  /\bdags?\b/i,
  /\bfrontiers?\b/i,
  /\bvector[\s-]*clocks?\b/i,
  /\bbranch(es|ed|ing)?\b/i,
  /\bcommit(s|ted|ting)?\b/i,
  /\brebas(e|es|ed|ing)\b/i,
  /\bstaging\b/i,
  /\brefs?\b/i,
  /\b(operation[\s-]*logs?|op[\s-]?logs?)\b/i,
];

/** A stand-in for whatever an error sentence names, so a function becomes a finished sentence. */
const SAMPLE = 'sample.identifier';

const everySentence = (): [string, string][] => [
  ...Object.entries(CONNECTION_COPY).map(([key, value]) => [`connection.${key}`, value] as [string, string]),
  ...Object.entries(OPERATION_COPY).flatMap(([key, value]) => [
    [`operations.${key}.label`, value.label] as [string, string],
    [`operations.${key}.description`, value.description] as [string, string],
  ]),
  ...Object.entries(ERROR_COPY).map(
    ([key, build]) => [`errors.${key}`, (build as (...args: string[]) => string)(SAMPLE, SAMPLE)] as [string, string],
  ),
  ...USER_STATES.flatMap((state) => [
    [`status.${state.id}.label`, state.status] as [string, string],
    [`status.${state.id}.means`, state.means] as [string, string],
    [`status.${state.id}.nextStep`, state.nextStep] as [string, string],
  ]),
];

describe('the copy this application shows', () => {
  it('has sentences to check', () => {
    assert.ok(everySentence().length >= 10);
  });

  it('covers the error text and the status text, not only the labels', () => {
    const where = everySentence().map(([key]) => key);
    assert.ok(
      where.some((key) => key.startsWith('errors.')),
      'no error sentence is being scanned — the half of the copy that leaks is unguarded',
    );
    assert.equal(where.filter((key) => key.startsWith('status.')).length, 18, 'the six states are not all scanned');
  });

  it('uses no word from the internal model', () => {
    for (const [where, sentence] of everySentence()) {
      for (const pattern of BANNED) {
        assert.ok(!pattern.test(sentence), `${where} matches ${pattern}: ${sentence}`);
      }
    }
  });

  it('says something a person can act on', () => {
    for (const [where, sentence] of everySentence()) {
      assert.ok(sentence.trim().length > 0, `${where} is empty`);
      assert.equal(sentence, sentence.trim(), `${where} has stray whitespace`);
    }
  });

  it('ends every description in a full stop, so it reads as a sentence', () => {
    for (const [where, sentence] of everySentence()) {
      if (where.endsWith('.label')) continue;
      assert.ok(sentence.endsWith('.') || sentence.endsWith('…'), `${where} is not a sentence: ${sentence}`);
    }
  });
});
