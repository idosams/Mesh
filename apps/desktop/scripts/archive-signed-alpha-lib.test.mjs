import assert from 'node:assert/strict';
import test from 'node:test';
import {
  assertNotaryLog,
  assertNotarySubmission,
  assertSignedEntitlements,
  assertSigningReadiness,
  entitlementsPlist,
  signedEntitlements,
  signingReadinessFailure,
} from './archive-signed-alpha-lib.mjs';

const teamId = 'ABCDEFGHIJ';
const fingerprint = 'A'.repeat(40);

function readiness() {
  return {
    schema: 'mesh-macos-signing-readiness/v1',
    ready: true,
    bundle_identifier: 'dev.mesh.desktop',
    checks: [
      { name: 'developer-id-application', ok: true, fingerprint, team_id: teamId },
      {
        name: 'provisioning-profile',
        ok: true,
        team_id: teamId,
        application_identifier: `${teamId}.dev.mesh.desktop`,
      },
      { name: 'notary-credentials', ok: true },
    ],
  };
}

test('readiness binds identity, team, application, and keychain group', () => {
  assert.deepEqual(assertSigningReadiness(readiness()), {
    fingerprint,
    teamId,
    applicationIdentifier: `${teamId}.dev.mesh.desktop`,
  });
  assert.deepEqual(signedEntitlements(teamId), {
    'com.apple.application-identifier': `${teamId}.dev.mesh.desktop`,
    'com.apple.developer.team-identifier': teamId,
    'keychain-access-groups': [`${teamId}.dev.mesh.desktop`],
  });
  assert.match(entitlementsPlist(teamId), /ABCDEFGHIJ\.dev\.mesh\.desktop/);
  assert.doesNotMatch(entitlementsPlist(teamId), /get-task-allow/);
});

test('identity and entitlement mismatches fail closed', () => {
  const wrongProfile = readiness();
  wrongProfile.checks[1].team_id = 'KLMNOPQRST';
  assert.throws(() => assertSigningReadiness(wrongProfile), /Expected values to be strictly equal/);
  assert.throws(() => signedEntitlements('short'), /team identifier/);
  assert.throws(
    () => assertSignedEntitlements({ ...signedEntitlements(teamId), 'get-task-allow': true }, teamId),
    /signed entitlements/,
  );
});

test('a failed readiness report remains actionable without credential material', () => {
  const message = signingReadinessFailure({
    schema: 'mesh-macos-signing-readiness/v1',
    ready: false,
    checks: [
      { name: 'full-xcode', ok: false, reason: 'select a full Xcode installation' },
      { name: 'notary-credentials', ok: false, reason: 'named Keychain profile did not authenticate' },
    ],
  });
  assert.match(message, /full-xcode — select a full Xcode installation/);
  assert.match(message, /notary-credentials — named Keychain profile did not authenticate/);
  assert.match(message, /tauri:signing-preflight/);
  assert.doesNotMatch(message, /password|private key|profile contents/i);
});

test('notarization must be accepted and issue-free', () => {
  const id = '12345678-1234-1234-1234-123456789abc';
  assert.equal(assertNotarySubmission({ id, status: 'Accepted' }), id);
  assert.doesNotThrow(() => assertNotaryLog({ jobId: id, status: 'Accepted', issues: [] }, id));
  assert.doesNotThrow(() => assertNotaryLog({ jobId: id, status: 'Accepted', issues: null }, id));
  assert.throws(() => assertNotarySubmission({ id, status: 'Invalid' }), /did not accept/);
  assert.throws(() => assertNotarySubmission({ id: '-'.repeat(36), status: 'Accepted' }), /canonical submission/);
  assert.throws(
    () => assertNotaryLog({ jobId: id, status: 'Accepted', issues: [{ message: 'warning' }] }, id),
    /contains issues/,
  );
});
