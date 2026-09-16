// What a person actually sees, asserted on the exact frame.
//
// `renderWindow` is pure, so the three situations that matter — a service that is running, one
// that was never started, and one too far apart in age to talk to — are reachable here without any
// timing at all. `live.test.ts` then reaches the same three over a real socket, which is the
// difference between "the renderer can draw this" and "this is what comes out".
//
// The vocabulary check at the bottom is deliberately over the RENDERED FRAME rather than over the
// copy tables. `src/app/copy.test.ts` walks the tables, and every table is also linted by
// `tools/program/vocab-lint`; neither of them sees the sentence the BACKGROUND SERVICE supplies,
// which arrives over a socket at run time and lands in the middle of this window. That is the one
// place an internal word could reach a person without any file in this repository containing it.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { frameLines, frameText } from '../../test-support/frame.ts';
import { CONNECTION_COPY } from '../strings/connection.ts';
import { WINDOW_COPY } from '../strings/window.ts';
import type { ServiceFacts } from './facts.ts';
import { INNER_WIDTH, connectionSentence, renderWindow, wrapText, type WindowView } from './window.ts';

/** The nine terms banned from anything a person reads, as in `copy.test.ts`. */
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

const FACTS: ServiceFacts = {
  serving: true,
  surfaceVersion: 1,
  startupSentence: 'Mesh started and your workspace is up to date. 0 saved changes were read back.',
  startupElapsedMs: 3,
  methodNames: ['daemon.status', 'startup.report', 'surface.describe'],
};

const view = (over: Partial<WindowView> = {}): WindowView => ({
  endpoint: '/home/person/.mesh/run/daemon.sock',
  connection: 'connected',
  everConnected: true,
  facts: FACTS,
  fault: null,
  staysOpen: true,
  ...over,
});

describe('the Mesh window', () => {
  it('is a rectangle, whatever it is showing', () => {
    const frames = [
      renderWindow(view()),
      renderWindow(view({ facts: null, connection: 'reconnecting', everConnected: false })),
      renderWindow(view({ fault: CONNECTION_COPY.uncorrelated })),
      renderWindow(view({ endpoint: `/${'very-long-directory-name'.repeat(6)}/daemon.sock` })),
    ];
    for (const frame of frames) {
      const widths = new Set(frameLines(frame).map((line) => [...line].length));
      assert.deepEqual([...widths], [INNER_WIDTH + 4], `a frame came out ragged:\n${frame}`);
    }
  });

  it('shows what the background service said, word for word', () => {
    const frame = renderWindow(view());
    const text = frameText(frame);
    assert.match(text, /Connected to Mesh\./);
    assert.ok(text.includes('0 saved changes were read back.'), `the service’s own sentence is missing:\n${frame}`);
    assert.ok(text.includes('version 1'), 'the interface version is missing');
    assert.ok(text.includes('3 ms'), 'how long the start-up took is missing');
    assert.ok(text.includes(WINDOW_COPY.serving), 'whether the service is serving is missing');
  });

  it('says the service is not running, rather than that it is reconnecting to one that never was', () => {
    const never = view({ connection: 'reconnecting', everConnected: false, facts: null });
    assert.equal(connectionSentence(never), CONNECTION_COPY.notRunning);
    assert.ok(frameText(renderWindow(never)).includes('is not running on this device'));

    const dropped = view({ connection: 'reconnecting', everConnected: true });
    assert.equal(connectionSentence(dropped), CONNECTION_COPY.reconnecting);
  });

  it('keeps the last reading on the screen while the link is down', () => {
    const text = frameText(renderWindow(view({ connection: 'reconnecting', everConnected: true })));
    assert.ok(text.includes('Reconnecting to Mesh.'), 'the link is not reported');
    assert.ok(text.includes('0 saved changes were read back.'), 'the last reading was thrown away');
  });

  it('says so when the two ends are too far apart in age', () => {
    const frame = renderWindow(view({ connection: 'unusable', facts: null }));
    assert.ok(frameText(frame).includes('too far apart in age'), `the version mismatch is not reported:\n${frame}`);
    assert.ok(frameText(frame).includes(WINDOW_COPY.nothingYet), 'the service section invented a reading');
  });

  it('marks an operation the running service does not offer', () => {
    const older: ServiceFacts = { ...FACTS, methodNames: ['daemon.status'] };
    const frame = renderWindow(view({ facts: older }));
    assert.ok(frameText(frame).includes(WINDOW_COPY.operationMissing), `nothing was marked missing:\n${frame}`);
  });

  it('gives a label too long for its column a line of its own', () => {
    // The regression this is here for printed `What this version offersready`, with no space.
    const frame = renderWindow(view());
    assert.ok(!frame.includes('offersready'), `a label ran into its value:\n${frame}`);
  });

  it('uses no word from the internal model, including the words the service supplied', () => {
    const leaked: ServiceFacts = { ...FACTS, startupSentence: 'Mesh replayed the operation log and rebased your branch.' };
    const clean = frameText(renderWindow(view()));
    for (const pattern of BANNED) {
      assert.ok(!pattern.test(clean), `the frame matches ${pattern}:\n${clean}`);
    }
    // And the check is not vacuous: a service that leaked one would be visible here.
    const dirty = frameText(renderWindow(view({ facts: leaked })));
    assert.ok(
      BANNED.some((pattern) => pattern.test(dirty)),
      'the scan cannot see a sentence that arrived from the service, so it proves nothing',
    );
  });

  it('breaks a word that cannot fit rather than losing it', () => {
    const path = 'a'.repeat(30);
    assert.deepEqual(wrapText(path, 10), ['aaaaaaaaaa', 'aaaaaaaaaa', 'aaaaaaaaaa']);
    assert.deepEqual(wrapText('one two three', 9), ['one two', 'three']);
    assert.deepEqual(wrapText('', 10), ['']);
  });
});
