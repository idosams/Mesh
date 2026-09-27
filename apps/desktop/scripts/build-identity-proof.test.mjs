import assert from 'node:assert/strict';
import test from 'node:test';
import { verifyBuildIdentity } from './build-identity-proof.mjs';

test('exact build verification uses the native identity, not incidental executable strings', () => {
  const revision = 'a'.repeat(40);
  const identity = { schema: 'mesh.desktop-build-identity/v1', revision, exact: true };
  verifyBuildIdentity(JSON.stringify(identity), revision);
  // Forty zeroes can occur in unrelated constants inside an otherwise valid sealed executable.
  assert.throws(() => verifyBuildIdentity(JSON.stringify(identity), '0'.repeat(40)), /running executable/);
  for (const value of [{ ...identity, exact: false }, { ...identity, schema: 'other' },
    { ...identity, revision: 'development' }, { ...identity, extra: true }, null, {}, []]) {
    assert.throws(() => verifyBuildIdentity(JSON.stringify(value), revision));
  }
  assert.throws(() => verifyBuildIdentity('not json', revision));
});
