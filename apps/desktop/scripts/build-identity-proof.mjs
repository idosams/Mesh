import assert from 'node:assert/strict';

export function verifyBuildIdentity(raw, revision) {
  assert.match(revision, /^[0-9a-f]{40}$/);
  const identity = JSON.parse(raw);
  assert.deepEqual(Object.keys(identity).sort(), ['exact', 'revision', 'schema'], 'unexpected build identity fields');
  assert.equal(identity.schema, 'mesh.desktop-build-identity/v1', 'unrecognized executable build identity');
  assert.equal(identity.exact, true, 'the executable must identify an exact build');
  assert.equal(identity.revision, revision, 'the running executable must report the exact requested source revision');
}
