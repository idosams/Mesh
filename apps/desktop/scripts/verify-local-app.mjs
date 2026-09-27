#!/usr/bin/env node

import assert from 'node:assert/strict';
import { verifyBuildIdentity } from './build-identity-proof.mjs';
import { access, stat } from 'node:fs/promises';
import { constants } from 'node:fs';
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const repository = resolve(here, '../../..');
let requestedApp = null;
let revision = null;
for (let index = 2; index < process.argv.length; index += 1) {
  const argument = process.argv[index];
  if (argument === '--revision') {
    assert.equal(revision, null, '--revision may be supplied only once');
    revision = process.argv[++index];
    assert.ok(revision, '--revision requires a value');
  } else if (argument === '--app') {
    assert.equal(requestedApp, null, '--app may be supplied only once');
    requestedApp = process.argv[++index];
    assert.ok(requestedApp, '--app requires a value');
  } else if (!requestedApp && !argument.startsWith('-')) requestedApp = argument;
  else throw new Error(`unknown local app verifier argument: ${argument}`);
}
assert.match(revision || '', /^[0-9a-f]{40}$/, '--revision must name the exact bundled Git commit');
const app = resolve(requestedApp || join(repository, 'target/release/bundle/macos/Mesh.app'));
const executable = join(app, 'Contents/MacOS/mesh-desktop');
const plist = join(app, 'Contents/Info.plist');
const resources = join(app, 'Contents/Resources');
const icon = join(resources, 'Mesh.icns');

await access(plist, constants.R_OK);
await access(resources, constants.R_OK);
await access(icon, constants.R_OK);
await access(executable, constants.R_OK | constants.X_OK);
assert.ok((await stat(executable)).size > 0, 'the bundled executable must not be empty');
assert.ok((await stat(icon)).size > 0, 'the bundled icon resource must not be empty');
const executableStrings = execFileSync('/usr/bin/strings', [executable], {
  encoding: 'utf8',
  maxBuffer: 64 * 1024 * 1024,
});
assert.ok(executableStrings.includes(revision), 'the bundled executable must contain the exact source revision');
const componentInterfaceMarkers = [
  'Use your existing project',
  'Detach Mesh',
  'Reattach project',
  'Pinned comparisons',
  'Review what Mesh will bring in',
  'Recommended next action',
  'Approve exact version',
];
for (const marker of componentInterfaceMarkers) {
  assert.ok(
    executableStrings.includes(marker),
    `the bundled executable must embed the production component interface: ${marker}`,
  );
}

const signatureVerification = spawnSync(
  '/usr/bin/codesign',
  ['--verify', '--deep', '--strict', '--verbose=4', app],
  { encoding: 'utf8' },
);
assert.equal(
  signatureVerification.status,
  0,
  `the local app bundle must have one internally valid resource seal: ${signatureVerification.stderr}`,
);
const signatureDescription = spawnSync('/usr/bin/codesign', ['-dv', '--verbose=4', app], {
  encoding: 'utf8',
});
assert.equal(signatureDescription.status, 0, 'the local app signature must be inspectable');
assert.match(
  signatureDescription.stderr,
  /Signature=adhoc/,
  'the local bundle must remain ad-hoc rather than claiming a Developer ID identity',
);
assert.ok(
  executableStrings.includes('mesh.desktop-build-identity/v1'),
  'the executable must support side-effect-free build identity before it can be queried',
);

// Query only after the local resource seal has been verified. Incidental strings (including
// all-zero hashes) are not evidence of the revision this executable actually reports.
const buildIdentity = spawnSync(executable, ['--mesh-build-identity'], {
  encoding: 'utf8', timeout: 15_000, maxBuffer: 4096,
});
assert.equal(buildIdentity.status, 0, 'the sealed executable must report its build identity without opening a window');
verifyBuildIdentity(buildIdentity.stdout, revision);

const entitlementDescription = spawnSync(
  '/usr/bin/codesign',
  ['-d', '--entitlements', ':-', app],
  { encoding: 'utf8' },
);
assert.equal(entitlementDescription.status, 0, 'the local app entitlements must be inspectable');
const entitlementText = `${entitlementDescription.stdout}${entitlementDescription.stderr}`;
assert.doesNotMatch(
  entitlementText,
  /com\.apple\.application-identifier|keychain-access-groups/,
  'the ad-hoc local bundle must not claim an unvalidated keychain application identity',
);

const loadCommands = execFileSync('/usr/bin/otool', ['-l', executable], { encoding: 'utf8' });
const minimumLoadCommand = loadCommands.match(/cmd LC_BUILD_VERSION[\s\S]*?\n\s+minos ([0-9.]+)/)?.[1];
assert.equal(
  minimumLoadCommand,
  '11.0',
  'the executable must encode the supported macOS minimum',
);

function plistValue(key) {
  return execFileSync('/usr/bin/plutil', ['-extract', key, 'raw', '-o', '-', plist], {
    encoding: 'utf8',
  }).trim();
}

assert.equal(plistValue('CFBundleIdentifier'), 'dev.mesh.desktop');
assert.equal(plistValue('CFBundleDisplayName'), 'Mesh');
assert.equal(plistValue('CFBundleName'), 'Mesh');
assert.equal(
  plistValue('CFBundleShortVersionString'),
  '0.1.0',
  'the first-user alpha must not present the workspace placeholder version as its app version',
);
assert.equal(
  plistValue('CFBundleVersion'),
  '0.1.0',
  'the macOS build version must identify this distributable alpha iteration',
);
assert.equal(
  plistValue('LSMinimumSystemVersion'),
  minimumLoadCommand,
  'Info.plist must not advertise an older macOS version than the executable can load on',
);
assert.equal(
  plistValue('NSQuitAlwaysKeepsWindows'),
  'false',
  'AppKit window restoration must not race Mesh recent-workspace restoration before setup',
);

process.stdout.write(`${JSON.stringify({
  schema: 'mesh-local-app-proof/v2',
  app,
  executable,
  identifier: plistValue('CFBundleIdentifier'),
  display_name: plistValue('CFBundleDisplayName'),
  version: plistValue('CFBundleShortVersionString'),
  minimum_macos: minimumLoadCommand,
  source_revision: revision,
  source_exact: true,
  component_interface_embedded: true,
  appkit_window_restoration: false,
  executable_bytes: (await stat(executable)).size,
  icon_bytes: (await stat(icon)).size,
  bundle_sealed: true,
  signature: 'adhoc',
  developer_id_signed: false,
  signed: false,
  notarized: false,
  human_approval_available: false,
  human_approval_reason: 'validated Apple application identity required',
  approval_credential_continuity: false,
})}\n`);
