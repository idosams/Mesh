#!/usr/bin/env node

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { lstat, mkdtemp, readFile, readdir, rm } from 'node:fs/promises';
import { execFileSync, spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertCanonicalAlphaChecksum } from './alpha-checksum.mjs';
import { assertSignedEntitlements } from './archive-signed-alpha-lib.mjs';
import {
  assertRegularExactFile,
  assertUniqueArchiveEntries,
} from './alpha-guide-verification.mjs';
import { evaluateProvisioningProfile } from './macos-signing-preflight-lib.mjs';
import {
  assertCompleteArchiveFixtureProof,
  parseRenderedAppProofOutput,
} from './renderer-proof-protocol.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const desktop = resolve(here, '..');
const repository = resolve(desktop, '../..');
const expectedGuide = join(desktop, 'SIGNED-ALPHA-START-HERE.txt');
const expectedSessionGuide = join(desktop, 'ALPHA-FIRST-SESSION.txt');
const expectedVisualGuide = join(desktop, 'ALPHA-VISUAL-GUIDE.html');
const GUIDE_NAME = 'START-HERE.txt';
const SESSION_GUIDE_NAME = 'FIRST-SESSION.txt';
const VISUAL_GUIDE_NAME = 'VISUAL-GUIDE.html';
const VISUAL_ASSET_DIRECTORY = 'alpha-guide/screenshots';
const VISUAL_SCREENSHOTS = Object.freeze([
  'review-workbench.jpg',
  'review-pdf.jpg',
  'review-text-diff.jpg',
]);
let manifestPath = null;
let proveRendered = false;
for (let index = 2; index < process.argv.length; index += 1) {
  const argument = process.argv[index];
  if (argument === '--manifest') {
    assert.equal(manifestPath, null, '--manifest may be supplied only once');
    manifestPath = process.argv[++index];
    assert.ok(manifestPath, '--manifest requires a value');
  } else if (argument === '--prove-rendered') {
    assert.equal(proveRendered, false, '--prove-rendered may be supplied only once');
    proveRendered = true;
  } else throw new Error(`unknown signed alpha archive verifier argument: ${argument}`);
}
assert.ok(manifestPath, '--manifest is required');
manifestPath = resolve(manifestPath);
assert.equal(manifestPath.endsWith('.json'), true, 'the signed alpha manifest must end in .json');

async function sha256(path) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}

function plistJson(program, args, input = undefined) {
  const result = spawnSync(program, args, {
    encoding: 'utf8',
    input,
    maxBuffer: 8 * 1024 * 1024,
  });
  assert.equal(result.status, 0, result.stderr);
  return JSON.parse(result.stdout);
}

const manifestBytes = await readFile(manifestPath, 'utf8');
const record = JSON.parse(manifestBytes);
assert.equal(manifestBytes, `${JSON.stringify(record, null, 2)}\n`, 'signed manifest must be canonical JSON');
assert.deepEqual(Object.keys(record), [
  'schema',
  'artifact',
  'sha256',
  'source_revision',
  'source_exact',
  'platform',
  'architecture',
  'minimum_macos',
  'bundle_signature',
  'developer_id_signed',
  'signing_fingerprint',
  'team_id',
  'application_identifier',
  'hardened_runtime',
  'secure_timestamp',
  'notarized',
  'notarization_submission',
  'stapled',
  'gatekeeper_assessed',
  'human_approval_available',
  'approval_credential_continuity',
]);
assert.equal(record.schema, 'mesh-macos-signed-alpha-archive/v1');
assert.match(record.artifact, /^Mesh-signed-alpha-[0-9a-f]{12}-macos-(arm64|x86_64)\.zip$/);
assert.match(record.sha256, /^[0-9a-f]{64}$/);
assert.match(record.source_revision, /^[0-9a-f]{40}$/);
assert.equal(record.artifact.includes(record.source_revision.slice(0, 12)), true);
assert.equal(record.source_exact, true);
assert.equal(record.platform, 'macos');
assert.match(record.architecture, /^(arm64|x86_64)$/);
assert.equal(record.minimum_macos, '11.0');
assert.equal(record.bundle_signature, 'developer-id-application');
assert.equal(record.developer_id_signed, true);
assert.match(record.signing_fingerprint, /^[0-9A-F]{40}$/);
assert.match(record.team_id, /^[A-Z0-9]{10}$/);
assert.equal(record.application_identifier, `${record.team_id}.dev.mesh.desktop`);
assert.equal(record.hardened_runtime, true);
assert.equal(record.secure_timestamp, true);
assert.equal(record.notarized, true);
assert.match(
  record.notarization_submission,
  /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/,
);
assert.equal(record.stapled, true);
assert.equal(record.gatekeeper_assessed, true);
assert.equal(record.human_approval_available, true);
assert.equal(record.approval_credential_continuity, true);

if (proveRendered) {
  assert.equal(execFileSync('git', ['-C', repository, 'rev-parse', '--verify', 'HEAD'], { encoding: 'utf8' }).trim(), record.source_revision);
  assert.equal(execFileSync('git', ['-C', repository, 'status', '--porcelain'], { encoding: 'utf8' }), '');
  execFileSync('npm', ['test'], { cwd: desktop, stdio: 'inherit' });
}

const archive = join(dirname(manifestPath), record.artifact);
const checksumFile = `${manifestPath.slice(0, -'.json'.length)}.sha256`;
assertCanonicalAlphaChecksum(await readFile(checksumFile, 'utf8'), record);
assert.equal(await sha256(archive), record.sha256, 'signed archive checksum must match its manifest');
const entries = execFileSync('/usr/bin/unzip', ['-Z1', archive], { encoding: 'utf8' })
  .split('\n').filter(Boolean);
assert.ok(entries.length > 0, 'signed archive must not be empty');
assertUniqueArchiveEntries(entries, 'the signed alpha archive');
for (const entry of entries) {
  assert.equal(entry.startsWith('/'), false, `archive entry must be relative: ${entry}`);
  assert.equal(entry.split('/').includes('..'), false, `archive entry must not traverse: ${entry}`);
  assert.equal(
    entry.split('/').some((part) => part === '__MACOSX' || part.startsWith('._')),
    false,
    `signed archive entry must not contain macOS metadata sidecars: ${entry}`,
  );
  assert.ok(
    entry === GUIDE_NAME
      || entry === SESSION_GUIDE_NAME
      || entry === VISUAL_GUIDE_NAME
      || entry === 'alpha-guide/'
      || entry === `${VISUAL_ASSET_DIRECTORY}/`
      || VISUAL_SCREENSHOTS.some((name) => entry === `${VISUAL_ASSET_DIRECTORY}/${name}`)
      || entry === 'Mesh.app/'
      || entry.startsWith('Mesh.app/'),
    `unexpected archive entry: ${entry}`,
  );
}

const extraction = await mkdtemp(join(tmpdir(), 'mesh-signed-alpha-verify-'));
let renderedAppProof = null;
try {
  execFileSync('/usr/bin/ditto', ['-x', '-k', archive, extraction], { stdio: 'inherit' });
  const top = await readdir(extraction);
  assert.ok(top.includes('Mesh.app'));
  assert.ok(top.includes(GUIDE_NAME));
  assert.ok(top.includes(SESSION_GUIDE_NAME));
  assert.ok(top.includes(VISUAL_GUIDE_NAME));
  assert.ok(top.includes('alpha-guide'));
  assert.equal(
    top.every(
      (entry) => entry === 'Mesh.app'
        || entry === GUIDE_NAME
        || entry === SESSION_GUIDE_NAME
        || entry === VISUAL_GUIDE_NAME
        || entry === 'alpha-guide',
    ),
    true,
  );
  await assertRegularExactFile(join(extraction, GUIDE_NAME), expectedGuide, 'the signed alpha start guide');
  await assertRegularExactFile(
    join(extraction, SESSION_GUIDE_NAME),
    expectedSessionGuide,
    'the signed first-session guide',
  );
  await assertRegularExactFile(
    join(extraction, VISUAL_GUIDE_NAME),
    expectedVisualGuide,
    'the signed visual guide',
  );
  const extractedVisualAssets = join(extraction, VISUAL_ASSET_DIRECTORY);
  const visualAssetsStat = await lstat(extractedVisualAssets);
  assert.equal(visualAssetsStat.isDirectory() && !visualAssetsStat.isSymbolicLink(), true);
  assert.deepEqual((await readdir(extractedVisualAssets)).sort(), [...VISUAL_SCREENSHOTS].sort());
  for (const name of VISUAL_SCREENSHOTS) {
    const extractedScreenshot = join(extractedVisualAssets, name);
    const screenshot = await assertRegularExactFile(
      extractedScreenshot,
      join(desktop, VISUAL_ASSET_DIRECTORY, name),
      `the signed visual screenshot ${name}`,
    );
    assert.equal(screenshot.subarray(0, 3).toString('hex'), 'ffd8ff');
  }

  const app = join(extraction, 'Mesh.app');
  execFileSync('/usr/bin/codesign', ['--verify', '--deep', '--strict', '--verbose=4', app], { stdio: 'inherit' });
  const details = spawnSync('/usr/bin/codesign', ['-dv', '--verbose=4', app], { encoding: 'utf8' });
  assert.equal(details.status, 0, details.stderr);
  assert.match(details.stderr, /Authority=Developer ID Application:/);
  assert.match(details.stderr, new RegExp(`TeamIdentifier=${record.team_id}`));
  assert.match(details.stderr, /flags=0x[0-9a-f]*10000\(runtime\)/i);
  assert.match(details.stderr, /Timestamp=/);

  const certificatePrefix = join(extraction, 'signing-certificate-');
  execFileSync('/usr/bin/codesign', ['-d', '--extract-certificates', certificatePrefix, app], { stdio: 'inherit' });
  const certificate = await readFile(`${certificatePrefix}0`);
  assert.equal(createHash('sha1').update(certificate).digest('hex').toUpperCase(), record.signing_fingerprint);

  const entitlementResult = spawnSync('/usr/bin/codesign', ['-d', '--entitlements', ':-', app], {
    encoding: 'utf8',
    maxBuffer: 4 * 1024 * 1024,
  });
  assert.equal(entitlementResult.status, 0, entitlementResult.stderr);
  const entitlements = plistJson('/usr/bin/plutil', ['-convert', 'json', '-o', '-', '-'], entitlementResult.stdout);
  assertSignedEntitlements(entitlements, record.team_id);

  const embeddedProfile = join(app, 'Contents', 'embedded.provisionprofile');
  const profileStat = await lstat(embeddedProfile);
  assert.equal(profileStat.isFile() && !profileStat.isSymbolicLink(), true);
  const decodedProfile = execFileSync('/usr/bin/security', ['cms', '-D', '-i', embeddedProfile], { encoding: 'utf8' });
  const profile = plistJson('/usr/bin/plutil', ['-convert', 'json', '-o', '-', '-'], decodedProfile);
  // Build-time preflight requires an unexpired profile. A securely timestamped, notarized artifact
  // remains verifiable after that profile later expires, so archive re-verification checks the
  // embedded authorization shape against the signature identity without applying today's clock.
  const profileFacts = evaluateProvisioningProfile(profile, {
    fingerprint: record.signing_fingerprint,
    team_id: record.team_id,
  }, new Date(0));
  assert.equal(profileFacts.ok, true, profileFacts.reason);
  assert.equal(profileFacts.application_identifier, record.application_identifier);

  const info = plistJson('/usr/bin/plutil', ['-convert', 'json', '-o', '-', join(app, 'Contents', 'Info.plist')]);
  assert.equal(info.CFBundleIdentifier, 'dev.mesh.desktop');
  assert.equal(info.CFBundleDisplayName, 'Mesh');
  assert.equal(info.CFBundleShortVersionString, '0.1.0');
  const executable = await readFile(join(app, 'Contents', 'MacOS', 'mesh-desktop'));
  assert.equal(executable.includes(Buffer.from(record.source_revision)), true, 'signed executable lacks exact source revision');

  execFileSync('/usr/bin/xcrun', ['stapler', 'validate', app], { stdio: 'inherit' });
  execFileSync('/usr/sbin/spctl', ['--assess', '--type', 'execute', '--verbose=4', app], { stdio: 'inherit' });
  if (proveRendered) {
    const renderedOutput = execFileSync(process.execPath, [join(here, 'prove-rendered-app.mjs')], {
      cwd: desktop,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'inherit'],
      env: {
        ...process.env,
        MESH_LOCAL_APP: app,
        MESH_EXPECTED_BUILD_REVISION: record.source_revision,
      },
    });
    renderedAppProof = assertCompleteArchiveFixtureProof(
      parseRenderedAppProofOutput(renderedOutput),
    );
  }
} finally {
  await rm(extraction, { recursive: true, force: true });
}

process.stdout.write(`${JSON.stringify({
  archive,
  checksum_file: checksumFile,
  verified: true,
  desktop_control_suite: proveRendered,
  extracted_windowed_app_proof: proveRendered,
  extracted_journey_driver: renderedAppProof ? 'renderer+daemon-ipc' : null,
  extracted_renderer_controls_driven: renderedAppProof?.renderer_controls_driven === true,
  extracted_workspace_versions_proved:
    renderedAppProof?.renderer?.versions?.outcome === 'verified-preview-ready',
  extracted_private_export_proved:
    renderedAppProof?.renderer?.private_export?.outcome === 'private-export-completed',
  extracted_agent_handoff_lifecycle_proved:
    renderedAppProof?.renderer?.agent_handoff?.outcome === 'agent-handoff-completed',
  component_interface_mounted: renderedAppProof?.component_interface_mounted === true,
  renderer_proof: renderedAppProof?.renderer || null,
  ...record,
})}\n`);
