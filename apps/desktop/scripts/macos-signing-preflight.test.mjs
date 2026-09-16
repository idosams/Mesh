import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { parseMacosSigningPreflightArguments } from './macos-signing-preflight-args.mjs';
import {
  developerIdApplicationIdentities,
  evaluateProvisioningProfile,
  selectDeveloperIdentity,
} from './macos-signing-preflight-lib.mjs';

const identity = { fingerprint: 'A'.repeat(40), team_id: 'ABCDEFGHIJ' };
const validProfile = {
  TeamIdentifier: [identity.team_id],
  ExpirationDate: '2035-01-01T00:00:00.000Z',
  Entitlements: {
    'com.apple.developer.team-identifier': identity.team_id,
    'com.apple.application-identifier': `${identity.team_id}.dev.mesh.desktop`,
    'keychain-access-groups': [`${identity.team_id}.dev.mesh.desktop`],
  },
};

test('signing preflight arguments are closed and secret-free', () => {
  assert.deepEqual(parseMacosSigningPreflightArguments([]), {
    help: false, json: false, profile: null, notaryProfile: null, identity: null,
  });
  assert.deepEqual(
    parseMacosSigningPreflightArguments([
      '--profile', '/tmp/profile.provisionprofile',
      '--notary-profile', 'Mesh Notary',
      '--identity', 'a'.repeat(40),
      '--json',
    ]),
    {
      help: false,
      json: true,
      profile: '/tmp/profile.provisionprofile',
      notaryProfile: 'Mesh Notary',
      identity: 'A'.repeat(40),
    },
  );
  for (const argv of [
    ['--profile'], ['--profile', '--json'], ['--profile', 'a', '--profile', 'b'],
    ['--notary-profile'], ['--identity', 'short'], ['--json', '--json'], ['--unknown'],
    ['--help', '--json'],
  ]) assert.throws(() => parseMacosSigningPreflightArguments(argv));
});

test('only exact Developer ID Application identities are eligible', () => {
  const parsed = developerIdApplicationIdentities(`
  1) ${'a'.repeat(40)} "Developer ID Application: Mesh Team (ABCDEFGHIJ)"
  2) ${'b'.repeat(40)} "Apple Development: Mesh Team (ABCDEFGHIJ)"
  3) ${'c'.repeat(40)} "Developer ID Installer: Mesh Team (ABCDEFGHIJ)"
     3 valid identities found
`);
  assert.deepEqual(parsed, [identity]);
  assert.deepEqual(selectDeveloperIdentity(parsed, identity.fingerprint), { ok: true, identity });
  assert.match(selectDeveloperIdentity([], null).reason, /no Developer ID Application/);
  assert.match(selectDeveloperIdentity([identity, { ...identity, fingerprint: 'D'.repeat(40) }], null).reason, /multiple/);
  assert.match(selectDeveloperIdentity(parsed, 'E'.repeat(40)).reason, /requested/);
});

test('the profile must authorize the exact application and keychain identity', () => {
  assert.deepEqual(
    evaluateProvisioningProfile(validProfile, identity, new Date('2030-01-01T00:00:00Z')),
    {
      ok: true,
      team_id: identity.team_id,
      application_identifier: `${identity.team_id}.dev.mesh.desktop`,
      expires_at: '2035-01-01T00:00:00.000Z',
    },
  );
  const mutations = [
    [{ ...validProfile, TeamIdentifier: ['ZZZZZZZZZZ'] }, /team/],
    [{ ...validProfile, ExpirationDate: '2020-01-01T00:00:00Z' }, /expired/],
    [{ ...validProfile, Entitlements: null }, /entitlement/],
    [{ ...validProfile, Entitlements: { ...validProfile.Entitlements, 'com.apple.developer.team-identifier': 'ZZZZZZZZZZ' } }, /developer-team/],
    [{ ...validProfile, Entitlements: { ...validProfile.Entitlements, 'com.apple.application-identifier': `${identity.team_id}.other.app` } }, /does not authorize dev\.mesh\.desktop/],
    [{ ...validProfile, Entitlements: { ...validProfile.Entitlements, 'keychain-access-groups': [] } }, /keychain access/],
  ];
  for (const [profile, expected] of mutations) {
    assert.match(evaluateProvisioningProfile(profile, identity, new Date('2030-01-01')).reason, expected);
  }
});

test('help does not inspect Keychain, profiles, Xcode, or the network', async () => {
  const script = new URL('./macos-signing-preflight.mjs', import.meta.url);
  const source = await readFile(script, 'utf8');
  assert.ok(source.indexOf('if (options.help)') < source.indexOf("xcode-select"));
  assert.doesNotMatch(source, /--password|--apple-id|--team-id|--key-id|--issuer/);
  const help = execFileSync(process.execPath, [script.pathname, '--help'], { encoding: 'utf8' });
  assert.match(help, /Developer ID provisioning profile/);
  const refused = spawnSync(process.execPath, [script.pathname, '--unknown'], { encoding: 'utf8' });
  assert.equal(refused.status, 1);
  assert.equal(refused.stdout, '');
  assert.equal(refused.stderr, 'Mesh signing preflight refused: unknown argument: --unknown\n');
  assert.doesNotMatch(refused.stderr, /\n\s+at /);
});

test('a credential-free run returns one actionable fail-closed record', () => {
  const script = new URL('./macos-signing-preflight.mjs', import.meta.url);
  const result = spawnSync(process.execPath, [script.pathname, '--json'], { encoding: 'utf8' });
  assert.equal(result.status, 1);
  assert.equal(result.stderr, '');
  const report = JSON.parse(result.stdout);
  assert.equal(report.schema, 'mesh-macos-signing-readiness/v1');
  assert.equal(report.ready, false);
  assert.equal(report.bundle_identifier, 'dev.mesh.desktop');
  assert.ok(report.next.includes('provisioning-profile'));
  assert.ok(report.next.includes('notary-credentials'));
  assert.equal(report.checks.every((entry) => Object.hasOwn(entry, 'ok')), true);
});
