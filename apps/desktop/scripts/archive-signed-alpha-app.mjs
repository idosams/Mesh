#!/usr/bin/env node

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { createReadStream, lstatSync } from 'node:fs';
import { chmod, copyFile, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { execFileSync, spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { canonicalAlphaChecksum } from './alpha-checksum.mjs';
import {
  ARCHIVE_SIGNED_ALPHA_APP_USAGE,
  parseArchiveSignedAlphaArguments,
} from './archive-signed-alpha-app-args.mjs';
import {
  assertNotaryLog,
  assertNotarySubmission,
  assertSigningReadiness,
  entitlementsPlist,
  signingReadinessFailure,
} from './archive-signed-alpha-lib.mjs';
import { publishVerifiedAlphaArtifacts } from './publish-alpha-artifacts.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const desktop = resolve(here, '..');
const repository = resolve(desktop, '../..');
const bundleDirectory = resolve(repository, 'target/release/bundle/macos');
const GUIDE_NAME = 'START-HERE.txt';
const SESSION_GUIDE_NAME = 'FIRST-SESSION.txt';
const VISUAL_GUIDE_NAME = 'VISUAL-GUIDE.html';
const VISUAL_ASSET_DIRECTORY = 'alpha-guide/screenshots';
const VISUAL_SCREENSHOTS = Object.freeze([
  'review-workbench.jpg',
  'review-pdf.jpg',
  'review-text-diff.jpg',
]);

function git(...args) {
  return execFileSync('git', ['-C', repository, ...args], { encoding: 'utf8' }).trim();
}

async function sha256(path) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}

function jsonCommand(program, args, extra = {}) {
  return JSON.parse(execFileSync(program, args, {
    encoding: 'utf8',
    maxBuffer: 8 * 1024 * 1024,
    ...extra,
  }));
}

function signingReadinessCommand(args) {
  const result = spawnSync(process.execPath, args, {
    encoding: 'utf8',
    maxBuffer: 8 * 1024 * 1024,
  });
  assert.equal(result.signal, null, 'Apple signing preflight was interrupted');
  assert.equal(result.stderr, '', 'Apple signing preflight wrote an unexpected diagnostic');
  let report = null;
  try { report = JSON.parse(result.stdout); } catch {
    throw new Error('Apple signing preflight did not return its canonical readiness record.');
  }
  if (result.status !== 0 || report.ready !== true) {
    throw new Error(signingReadinessFailure(report));
  }
  return report;
}

function unchangedRegularFile(path, before) {
  let after = null;
  try { after = lstatSync(path); } catch {}
  return Boolean(
    after?.isFile()
    && !after.isSymbolicLink()
    && after.dev === before.dev
    && after.ino === before.ino
    && after.size === before.size
    && after.mtimeMs === before.mtimeMs
  );
}

async function main() {
const options = parseArchiveSignedAlphaArguments(process.argv.slice(2));
if (options.help) {
  process.stdout.write(`${ARCHIVE_SIGNED_ALPHA_APP_USAGE}\n`);
  return;
}

const revision = git('rev-parse', '--verify', 'HEAD');
assert.match(revision, /^[0-9a-f]{40}$/, 'the signed alpha archive requires one canonical Git commit');
assert.equal(git('status', '--porcelain'), '', 'the signed alpha archive refuses tracked changes');
const architecture = execFileSync('/usr/bin/uname', ['-m'], { encoding: 'utf8' }).trim();
assert.match(architecture, /^(arm64|x86_64)$/, 'the signed alpha supports macOS arm64 or x86_64');

const sourceProfile = resolve(options.profile);
const sourceProfileStat = lstatSync(sourceProfile);
assert.equal(sourceProfileStat.isFile() && !sourceProfileStat.isSymbolicLink(), true,
  'profile must be one existing non-symlink regular file');

const stem = `Mesh-signed-alpha-${revision.slice(0, 12)}-macos-${architecture}`;
const archive = join(bundleDirectory, `${stem}.zip`);
const manifest = join(bundleDirectory, `${stem}.json`);
const checksumFile = join(bundleDirectory, `${stem}.sha256`);
const staging = await mkdtemp(join(tmpdir(), 'mesh-signed-alpha-archive-'));
const source = join(staging, 'source');
const sourceDesktop = join(source, 'apps', 'desktop');
const sourceHere = join(sourceDesktop, 'scripts');
const sourceGuide = join(sourceDesktop, 'SIGNED-ALPHA-START-HERE.txt');
const sourceSessionGuide = join(sourceDesktop, 'ALPHA-FIRST-SESSION.txt');
const sourceVisualGuide = join(sourceDesktop, 'ALPHA-VISUAL-GUIDE.html');
const sourceVisualAssets = join(sourceDesktop, VISUAL_ASSET_DIRECTORY);
const buildTarget = join(staging, 'target');
const app = join(buildTarget, 'release/bundle/macos/Mesh.app');
const pinnedProfile = join(staging, 'Mesh.provisionprofile');
const entitlements = join(staging, 'Mesh.entitlements.plist');
const notaryArchive = join(staging, 'Mesh-notary.zip');
const stagedArchive = join(staging, basename(archive));
const stagedManifest = join(staging, basename(manifest));
const stagedChecksum = join(staging, basename(checksumFile));
const stagedPayload = join(staging, 'payload');

async function copyVisualGuide(target) {
  await copyFile(sourceVisualGuide, join(target, VISUAL_GUIDE_NAME));
  const targetAssets = join(target, VISUAL_ASSET_DIRECTORY);
  await mkdir(targetAssets, { recursive: true, mode: 0o755 });
  await Promise.all(VISUAL_SCREENSHOTS.map((name) =>
    copyFile(join(sourceVisualAssets, name), join(targetAssets, name))));
}

try {
  await copyFile(sourceProfile, pinnedProfile);
  await chmod(pinnedProfile, 0o600);
  assert.equal(unchangedRegularFile(sourceProfile, sourceProfileStat), true,
    'profile changed or was replaced while it was pinned');

  const preflightArguments = [
    join(here, 'macos-signing-preflight.mjs'),
    '--profile', pinnedProfile,
    '--notary-profile', options.notaryProfile,
    '--json',
  ];
  if (options.identity !== null) preflightArguments.push('--identity', options.identity);
  const readiness = signingReadinessCommand(preflightArguments);
  const identity = assertSigningReadiness(readiness);

  // A clean check only authenticates one instant in the caller worktree. Build and execute the
  // release controls from a separate exact-commit materialization so a concurrent edit during the
  // long compile/notarization path cannot produce bytes that falsely claim the pinned revision.
  execFileSync('git', ['clone', '--local', '--no-hardlinks', '--no-checkout', repository, source], {
    stdio: 'inherit',
  });
  execFileSync('git', ['-C', source, 'checkout', '--detach', revision], { stdio: 'inherit' });
  assert.equal(
    execFileSync('git', ['-C', source, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(),
    revision,
  );
  assert.equal(
    execFileSync('git', ['-C', source, 'status', '--porcelain'], { encoding: 'utf8' }),
    '',
  );
  execFileSync('npm', ['ci', '--ignore-scripts'], { cwd: source, stdio: 'inherit' });

  execFileSync(process.execPath, [join(sourceHere, 'build-local-app.mjs')], {
    cwd: sourceDesktop,
    env: { ...process.env, CARGO_TARGET_DIR: buildTarget },
    stdio: 'inherit',
  });
  await copyFile(pinnedProfile, join(app, 'Contents', 'embedded.provisionprofile'));
  await writeFile(entitlements, entitlementsPlist(identity.teamId), { flag: 'wx', mode: 0o600 });
  execFileSync('/usr/bin/codesign', [
    '--force',
    '--sign', identity.fingerprint,
    '--identifier', 'dev.mesh.desktop',
    '--options', 'runtime',
    '--timestamp',
    '--entitlements', entitlements,
    app,
  ], { stdio: 'inherit' });
  execFileSync('/usr/bin/codesign', ['--verify', '--deep', '--strict', '--verbose=4', app], {
    stdio: 'inherit',
  });

  execFileSync('/usr/bin/ditto', ['-c', '-k', '--keepParent', app, notaryArchive], {
    stdio: 'inherit',
  });
  const submission = jsonCommand('/usr/bin/xcrun', [
    'notarytool', 'submit', notaryArchive,
    '--keychain-profile', options.notaryProfile,
    '--wait',
    '--timeout', '30m',
    '--output-format', 'json',
  ]);
  const submissionId = assertNotarySubmission(submission);
  const notaryLog = jsonCommand('/usr/bin/xcrun', [
    'notarytool', 'log', submissionId,
    '--keychain-profile', options.notaryProfile,
  ]);
  assertNotaryLog(notaryLog, submissionId);
  execFileSync('/usr/bin/xcrun', ['stapler', 'staple', app], { stdio: 'inherit' });
  execFileSync('/usr/bin/xcrun', ['stapler', 'validate', app], { stdio: 'inherit' });
  execFileSync('/usr/sbin/spctl', ['--assess', '--type', 'execute', '--verbose=4', app], {
    stdio: 'inherit',
  });

  await mkdir(stagedPayload, { mode: 0o755 });
  execFileSync('/usr/bin/ditto', [app, join(stagedPayload, 'Mesh.app')], { stdio: 'inherit' });
  await copyFile(sourceGuide, join(stagedPayload, GUIDE_NAME));
  await copyFile(sourceSessionGuide, join(stagedPayload, SESSION_GUIDE_NAME));
  await copyVisualGuide(stagedPayload);
  execFileSync('/usr/bin/ditto', ['-c', '-k', '--norsrc', stagedPayload, stagedArchive], {
    stdio: 'inherit',
  });
  const checksum = await sha256(stagedArchive);
  const record = {
    schema: 'mesh-macos-signed-alpha-archive/v1',
    artifact: basename(archive),
    sha256: checksum,
    source_revision: revision,
    source_exact: true,
    platform: 'macos',
    architecture,
    minimum_macos: '11.0',
    bundle_signature: 'developer-id-application',
    developer_id_signed: true,
    signing_fingerprint: identity.fingerprint,
    team_id: identity.teamId,
    application_identifier: identity.applicationIdentifier,
    hardened_runtime: true,
    secure_timestamp: true,
    notarized: true,
    notarization_submission: submissionId,
    stapled: true,
    gatekeeper_assessed: true,
    human_approval_available: true,
    approval_credential_continuity: true,
  };
  await writeFile(stagedManifest, `${JSON.stringify(record, null, 2)}\n`, { flag: 'wx', mode: 0o644 });
  await writeFile(stagedChecksum, canonicalAlphaChecksum(record), { flag: 'wx', mode: 0o644 });
  await publishVerifiedAlphaArtifacts({
    stagedArchive,
    stagedManifest,
    stagedChecksum,
    archive,
    manifest,
    checksumFile,
    verify: (candidateManifest) => {
      execFileSync(process.execPath, [
        join(sourceHere, 'verify-signed-alpha-archive.mjs'),
        '--manifest', candidateManifest,
        '--prove-rendered',
      ], { cwd: sourceDesktop, stdio: 'inherit' });
    },
  });
  process.stdout.write(`${JSON.stringify({ archive, manifest, checksum_file: checksumFile, ...record })}\n`);
} finally {
  await rm(staging, { recursive: true, force: true });
}
}

main().catch((error) => {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`Mesh signed alpha archive refused: ${message}\n`);
  process.exitCode = 1;
});
