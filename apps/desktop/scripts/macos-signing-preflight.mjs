#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import { lstatSync } from 'node:fs';
import { resolve } from 'node:path';
import {
  MACOS_SIGNING_PREFLIGHT_USAGE,
  parseMacosSigningPreflightArguments,
} from './macos-signing-preflight-args.mjs';
import {
  BUNDLE_IDENTIFIER,
  developerIdApplicationIdentities,
  evaluateProvisioningProfile,
  selectDeveloperIdentity,
} from './macos-signing-preflight-lib.mjs';

process.once('uncaughtException', (error) => {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`Mesh signing preflight refused: ${message}\n`);
  process.exitCode = 1;
});

const options = parseMacosSigningPreflightArguments(process.argv.slice(2));
if (options.help) {
  process.stdout.write(`${MACOS_SIGNING_PREFLIGHT_USAGE}\n`);
  process.exit(0);
}

function check(name, ok, reason, facts = {}) {
  return { name, ok, reason: ok ? null : reason, ...facts };
}

function command(program, args, extra = {}) {
  return spawnSync(program, args, {
    encoding: 'utf8',
    timeout: 30_000,
    maxBuffer: 4 * 1024 * 1024,
    ...extra,
  });
}

const checks = [];
checks.push(check(
  'platform',
  process.platform === 'darwin',
  'Apple signing requires macOS',
  { platform: process.platform },
));

const developerPath = command('/usr/bin/xcode-select', ['-p']);
const selectedDeveloperPath = developerPath.status === 0 ? developerPath.stdout.trim() : null;
checks.push(check(
  'full-xcode',
  selectedDeveloperPath?.endsWith('.app/Contents/Developer') === true,
  'select a full Xcode installation with xcode-select; Command Line Tools alone are insufficient',
  { selected: selectedDeveloperPath },
));

for (const tool of ['notarytool', 'stapler']) {
  const located = command('/usr/bin/xcrun', ['--find', tool]);
  checks.push(check(
    tool,
    located.status === 0 && located.stdout.trim().startsWith('/'),
    `${tool} is unavailable through the selected Xcode installation`,
  ));
}

const identityResult = command('/usr/bin/security', ['find-identity', '-v', '-p', 'codesigning']);
const identities = identityResult.status === 0
  ? developerIdApplicationIdentities(identityResult.stdout)
  : [];
const selectedIdentity = selectDeveloperIdentity(identities, options.identity);
checks.push(check(
  'developer-id-application',
  selectedIdentity.ok,
  selectedIdentity.reason,
  selectedIdentity.ok
    ? { fingerprint: selectedIdentity.identity.fingerprint, team_id: selectedIdentity.identity.team_id }
    : { installed: identities.length },
));

if (options.profile === null) {
  checks.push(check('provisioning-profile', false, '--profile is required'));
} else if (!selectedIdentity.ok) {
  checks.push(check('provisioning-profile', false, 'a signing identity is required before the profile can be verified'));
} else {
  const profilePath = resolve(options.profile);
  let profileStat = null;
  try {
    profileStat = lstatSync(profilePath);
  } catch {}
  if (!profileStat?.isFile() || profileStat.isSymbolicLink()) {
    checks.push(check('provisioning-profile', false, 'profile must be one existing non-symlink regular file'));
  } else {
    const decoded = command('/usr/bin/security', ['cms', '-D', '-i', profilePath]);
    if (decoded.status !== 0) {
      checks.push(check('provisioning-profile', false, 'security could not decode the provisioning profile'));
    } else {
      let profileAfter = null;
      try {
        profileAfter = lstatSync(profilePath);
      } catch {}
      const unchanged = profileAfter?.isFile()
        && !profileAfter.isSymbolicLink()
        && profileAfter.dev === profileStat.dev
        && profileAfter.ino === profileStat.ino
        && profileAfter.size === profileStat.size
        && profileAfter.mtimeMs === profileStat.mtimeMs;
      if (!unchanged) {
        checks.push(check('provisioning-profile', false, 'profile changed or was replaced during verification'));
      } else {
        const converted = command('/usr/bin/plutil', ['-convert', 'json', '-o', '-', '-'], {
          input: decoded.stdout,
        });
        if (converted.status !== 0) {
          checks.push(check('provisioning-profile', false, 'plutil could not decode the provisioning profile payload'));
        } else {
          try {
            const profileFacts = evaluateProvisioningProfile(
              JSON.parse(converted.stdout),
              selectedIdentity.identity,
            );
            checks.push(check('provisioning-profile', profileFacts.ok, profileFacts.reason, profileFacts.ok ? {
              team_id: profileFacts.team_id,
              application_identifier: profileFacts.application_identifier,
              expires_at: profileFacts.expires_at,
            } : {}));
          } catch {
            checks.push(check('provisioning-profile', false, 'profile payload is not valid JSON'));
          }
        }
      }
    }
  }
}

if (options.notaryProfile === null) {
  checks.push(check('notary-credentials', false, '--notary-profile is required'));
} else {
  const history = command('/usr/bin/xcrun', [
    'notarytool', 'history', '--keychain-profile', options.notaryProfile, '--output-format', 'json',
  ]);
  checks.push(check(
    'notary-credentials',
    history.status === 0,
    'notarytool could not authenticate with the named Keychain profile',
  ));
}

const report = {
  schema: 'mesh-macos-signing-readiness/v1',
  ready: checks.every((entry) => entry.ok),
  bundle_identifier: BUNDLE_IDENTIFIER,
  checks,
  next: checks.filter((entry) => !entry.ok).map((entry) => entry.name),
};

if (options.json) process.stdout.write(`${JSON.stringify(report)}\n`);
else {
  for (const entry of checks) {
    process.stdout.write(`${entry.ok ? 'PASS' : 'MISSING'} ${entry.name}${entry.reason ? ` — ${entry.reason}` : ''}\n`);
  }
  process.stdout.write(report.ready
    ? 'READY Apple signing prerequisites are verified.\n'
    : `NOT READY Complete: ${report.next.join(', ')}.\n`);
}
process.exitCode = report.ready ? 0 : 1;
