import assert from 'node:assert/strict';
import test from 'node:test';
import { captureProofArguments } from './attached-capture-proof-args.mjs';
test('capture proof defaults to development and requires an exact packaged revision', () => {
  assert.deepEqual(captureProofArguments([]), { app: null, revision: null });
  assert.deepEqual(captureProofArguments(['--app', '/local/Mesh.app', '--revision', 'a'.repeat(40)]),
    { app: '/local/Mesh.app', revision: 'a'.repeat(40) });
  for (const argv of [['--app', '/local/Mesh.app'], ['--revision', 'a'.repeat(40)], ['--app', 'x', '--revision', 'main'],
    ['--app', 'x', '--app', 'y'], ['--unknown'], ['--app', '--revision'], ['--revision', 'A'.repeat(40), '--app', 'x']]) {
    assert.throws(() => captureProofArguments(argv));
  }
});
