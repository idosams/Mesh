import assert from 'node:assert/strict';

const BUNDLE_IDENTIFIER = 'dev.mesh.desktop';

export function signedEntitlements(teamId) {
  assert.match(teamId, /^[A-Z0-9]{10}$/, 'the Apple team identifier must be exact');
  const applicationIdentifier = `${teamId}.${BUNDLE_IDENTIFIER}`;
  return {
    'com.apple.application-identifier': applicationIdentifier,
    'com.apple.developer.team-identifier': teamId,
    'keychain-access-groups': [applicationIdentifier],
  };
}

export function entitlementsPlist(teamId) {
  const applicationIdentifier = `${teamId}.${BUNDLE_IDENTIFIER}`;
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.application-identifier</key>
  <string>${applicationIdentifier}</string>
  <key>com.apple.developer.team-identifier</key>
  <string>${teamId}</string>
  <key>keychain-access-groups</key>
  <array>
    <string>${applicationIdentifier}</string>
  </array>
</dict>
</plist>
`;
}

export function assertSigningReadiness(report) {
  assert.equal(report?.schema, 'mesh-macos-signing-readiness/v1');
  assert.equal(report?.ready, true, 'Apple signing prerequisites are not ready');
  assert.equal(report?.bundle_identifier, BUNDLE_IDENTIFIER);
  const identity = report.checks.find((entry) => entry.name === 'developer-id-application');
  const profile = report.checks.find((entry) => entry.name === 'provisioning-profile');
  const notary = report.checks.find((entry) => entry.name === 'notary-credentials');
  assert.equal(identity?.ok, true, 'Developer ID identity was not verified');
  assert.match(identity?.fingerprint, /^[0-9A-F]{40}$/);
  assert.match(identity?.team_id, /^[A-Z0-9]{10}$/);
  assert.equal(profile?.ok, true, 'provisioning profile was not verified');
  assert.equal(profile?.team_id, identity.team_id);
  assert.equal(profile?.application_identifier, `${identity.team_id}.${BUNDLE_IDENTIFIER}`);
  assert.equal(notary?.ok, true, 'notary credentials were not verified');
  return {
    fingerprint: identity.fingerprint,
    teamId: identity.team_id,
    applicationIdentifier: profile.application_identifier,
  };
}

export function signingReadinessFailure(report) {
  assert.equal(report?.schema, 'mesh-macos-signing-readiness/v1');
  assert.ok(Array.isArray(report?.checks), 'signing readiness has no canonical checks');
  const missing = report.checks.filter((entry) => entry?.ok === false);
  assert.ok(missing.length > 0, 'a failed signing readiness report must name a missing prerequisite');
  return `Apple signing prerequisites are not ready: ${missing
    .map((entry) => `${entry.name} — ${entry.reason}`)
    .join('; ')}. Run tauri:signing-preflight after repairing them.`;
}

export function assertNotarySubmission(value) {
  assert.equal(value?.status, 'Accepted', 'Apple did not accept the notarization submission');
  assert.match(
    value?.id,
    /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/,
    'notarytool returned no canonical submission identifier',
  );
  return value.id;
}

export function assertNotaryLog(value, submissionId) {
  assert.equal(value?.jobId, submissionId, 'notary log does not belong to the accepted submission');
  assert.equal(value?.status, 'Accepted', 'notary log does not report an accepted submission');
  assert.ok(value?.issues === null || Array.isArray(value?.issues), 'notary log has no canonical issue list');
  if (value.issues !== null) assert.deepEqual(value.issues, [], 'notary log contains issues');
}

export function assertSignedEntitlements(value, teamId) {
  assert.deepEqual(value, signedEntitlements(teamId), 'signed entitlements do not match the approved application identity');
}
