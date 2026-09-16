export const BUNDLE_IDENTIFIER = 'dev.mesh.desktop';

export function developerIdApplicationIdentities(output) {
  const identities = [];
  for (const line of output.split('\n')) {
    const match = /^\s*\d+\)\s+([0-9A-Fa-f]{40})\s+"Developer ID Application: .+ \(([A-Z0-9]{10})\)"\s*$/.exec(line);
    if (!match) continue;
    identities.push({ fingerprint: match[1].toUpperCase(), team_id: match[2] });
  }
  return identities;
}

export function selectDeveloperIdentity(identities, requested) {
  if (requested !== null) {
    const selected = identities.find((identity) => identity.fingerprint === requested);
    if (!selected) return { ok: false, reason: 'requested Developer ID Application identity is unavailable' };
    return { ok: true, identity: selected };
  }
  if (identities.length === 0) {
    return { ok: false, reason: 'no Developer ID Application identity is installed' };
  }
  if (identities.length !== 1) {
    return { ok: false, reason: 'multiple Developer ID Application identities are installed; select one with --identity' };
  }
  return { ok: true, identity: identities[0] };
}

function authorizedApplicationIdentifier(value, teamId) {
  return value === `${teamId}.${BUNDLE_IDENTIFIER}` || value === `${teamId}.*`;
}

function authorizedKeychainGroup(groups, teamId) {
  return Array.isArray(groups) && groups.some((group) => (
    group === `${teamId}.${BUNDLE_IDENTIFIER}` || group === `${teamId}.*`
  ));
}

export function evaluateProvisioningProfile(profile, identity, now = new Date()) {
  if (profile === null || typeof profile !== 'object' || Array.isArray(profile)) {
    return { ok: false, reason: 'provisioning profile payload is not an object' };
  }
  const teams = profile.TeamIdentifier;
  if (!Array.isArray(teams) || teams.length !== 1 || teams[0] !== identity.team_id) {
    return { ok: false, reason: 'provisioning profile team does not match the signing identity' };
  }
  const expiration = new Date(profile.ExpirationDate);
  if (!Number.isFinite(expiration.getTime()) || expiration <= now) {
    return { ok: false, reason: 'provisioning profile is expired or has no valid expiration' };
  }
  const entitlements = profile.Entitlements;
  if (entitlements === null || typeof entitlements !== 'object' || Array.isArray(entitlements)) {
    return { ok: false, reason: 'provisioning profile has no entitlement authorization' };
  }
  if (entitlements['com.apple.developer.team-identifier'] !== identity.team_id) {
    return { ok: false, reason: 'profile developer-team entitlement does not match the signing identity' };
  }
  if (!authorizedApplicationIdentifier(
    entitlements['com.apple.application-identifier'] ?? entitlements['application-identifier'],
    identity.team_id,
  )) {
    return { ok: false, reason: `profile does not authorize ${BUNDLE_IDENTIFIER}` };
  }
  if (!authorizedKeychainGroup(entitlements['keychain-access-groups'], identity.team_id)) {
    return { ok: false, reason: `profile does not authorize ${BUNDLE_IDENTIFIER} keychain access` };
  }
  return {
    ok: true,
    team_id: identity.team_id,
    application_identifier: `${identity.team_id}.${BUNDLE_IDENTIFIER}`,
    expires_at: expiration.toISOString(),
  };
}
