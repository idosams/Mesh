import assert from 'node:assert/strict';
import test from 'node:test';
import { proofDaemonIdleTimeoutMs } from './proof-daemon-timeout.mjs';

test('filesystem-wide proof calls receive bounded large-workspace idle windows', () => {
  assert.equal(proofDaemonIdleTimeoutMs('folder.import.preview'), 60_000);
  for (const method of [
    'folder.import.confirm',
    'review.open-current',
    'workspace.state',
    'workspace.version.fork',
  ]) {
    assert.equal(proofDaemonIdleTimeoutMs(method), 300_000, method);
  }
});

test('memory-only and unknown proof calls retain the fail-fast idle window', () => {
  assert.equal(proofDaemonIdleTimeoutMs('unknown'), 1_000);
});
