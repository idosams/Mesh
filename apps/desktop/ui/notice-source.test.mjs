import assert from 'node:assert/strict';
import test from 'node:test';

import { createNoticeSource } from './notice-source.js';

class TestCustomEvent extends Event {
  constructor(type, init = {}) {
    super(type);
    this.detail = init.detail;
  }
}

test('notice source retains, replays, and proof-binds one exact frozen generation', () => {
  const document = new EventTarget();
  const projections = [];
  const source = createNoticeSource(document, TestCustomEvent);

  const first = source.show('Workspace verified.', false);
  assert.deepEqual(first, {
    schema: 'mesh.notice/v1', generation: 1,
    message: 'Workspace verified.', error: false, proof: null,
  });
  assert.equal(Object.isFrozen(first), true);

  document.addEventListener('mesh:notice-projection', (event) => projections.push(event.detail));
  document.dispatchEvent(new Event('mesh:notice-snapshot-request'));
  assert.equal(projections.length, 1);
  assert.equal(projections[0], first, 'replay cloned or changed the retained snapshot');

  assert.equal(source.markProof(0, 'agent-handoff-rescanned'), false);
  assert.equal(source.markProof(1, 'invented'), false);
  assert.equal(source.markProof(1, 'agent-handoff-rescanned'), true);
  const proven = source.snapshot();
  assert.deepEqual(proven, {
    schema: 'mesh.notice/v1', generation: 2,
    message: 'Workspace verified.', error: false, proof: 'agent-handoff-rescanned',
  });
  assert.equal(projections.at(-1), proven);

  source.clearProof();
  assert.deepEqual(source.snapshot(), {
    schema: 'mesh.notice/v1', generation: 3,
    message: 'Workspace verified.', error: false, proof: null,
  });
  const failure = source.show('Refresh required.', true);
  assert.equal(failure.generation, 4);
  assert.equal(source.markProof(4, 'agent-handoff-rescanned'), false, 'an error notice gained success proof');

  const bounded = source.show('x'.repeat(5_000));
  assert.equal(bounded.generation, 5);
  assert.equal(bounded.message.length, 4_096);
  assert.match(bounded.message, /\[notice shortened\]$/);
  assert.equal(source.show('').message, 'Mesh status unavailable.');
});
