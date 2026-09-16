#!/usr/bin/env node

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { copyFile, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { canonicalAlphaChecksum } from './alpha-checksum.mjs';
import {
  ARCHIVE_ALPHA_APP_USAGE,
  parseArchiveArguments,
} from './archive-alpha-app-args.mjs';
import { publishVerifiedAlphaArtifacts } from './publish-alpha-artifacts.mjs';

const options = parseArchiveArguments(process.argv.slice(2));
if (options.help) {
  process.stdout.write(`${ARCHIVE_ALPHA_APP_USAGE}\n`);
  process.exit(0);
}

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

const revision = git('rev-parse', '--verify', 'HEAD');
assert.match(revision, /^[0-9a-f]{40}$/, 'the alpha archive requires one canonical Git commit');
assert.equal(
  git('status', '--porcelain'),
  '',
  'the alpha archive refuses tracked changes so its embedded Git revision remains exact',
);

const architecture = execFileSync('/usr/bin/uname', ['-m'], { encoding: 'utf8' }).trim();
assert.match(architecture, /^(arm64|x86_64)$/, 'the alpha archive supports macOS arm64 or x86_64');
const stem = `Mesh-alpha-${revision.slice(0, 12)}-macos-${architecture}`;
const archive = join(bundleDirectory, `${stem}.zip`);
const manifest = join(bundleDirectory, `${stem}.json`);
const checksumFile = join(bundleDirectory, `${stem}.sha256`);
const delivery = join(bundleDirectory, `${stem}-delivery.zip`);
const staging = await mkdtemp(join(tmpdir(), 'mesh-alpha-archive-'));
const buildTarget = join(staging, 'target');
const app = join(buildTarget, 'release/bundle/macos/Mesh.app');
const stagedArchive = join(staging, basename(archive));
const stagedManifest = join(staging, basename(manifest));
const stagedChecksum = join(staging, basename(checksumFile));
const stagedDelivery = join(staging, basename(delivery));
const source = join(staging, 'source');
const sourceDesktop = join(source, 'apps', 'desktop');
const sourceHere = join(sourceDesktop, 'scripts');
const sourceGuide = join(sourceDesktop, 'ALPHA-START-HERE.txt');
const sourceSessionGuide = join(sourceDesktop, 'ALPHA-FIRST-SESSION.txt');
const sourceVisualGuide = join(sourceDesktop, 'ALPHA-VISUAL-GUIDE.html');
const sourceVisualAssets = join(sourceDesktop, VISUAL_ASSET_DIRECTORY);
const stagedPayload = join(staging, 'payload');
const stagedDeliveryPayload = join(staging, 'delivery-payload');
const stagedDeliveryFolder = join(stagedDeliveryPayload, stem);

async function copyVisualGuide(target) {
  await copyFile(sourceVisualGuide, join(target, VISUAL_GUIDE_NAME));
  const targetAssets = join(target, VISUAL_ASSET_DIRECTORY);
  await mkdir(targetAssets, { recursive: true, mode: 0o755 });
  await Promise.all(VISUAL_SCREENSHOTS.map((name) =>
    copyFile(join(sourceVisualAssets, name), join(targetAssets, name))));
}

try {
  // Materialize the exact revision in a private clone. A clean check authenticates only one
  // instant in the caller worktree; later edits must not enter bytes that claim this revision.
  execFileSync('git', ['clone', '--local', '--no-hardlinks', '--no-checkout', repository, source], {
    stdio: 'inherit',
  });
  execFileSync('git', ['-C', source, 'checkout', '--detach', revision], { stdio: 'inherit' });
  assert.equal(
    execFileSync('git', ['-C', source, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(),
    revision,
  );
  assert.equal(execFileSync('git', ['-C', source, 'status', '--porcelain'], { encoding: 'utf8' }), '');
  // Build in private staging. The normal target bundle may be the app currently serving a
  // dogfood workspace; replacing that path would leave a running process backed by a different
  // on-disk revision. Only the verified ZIP, manifest, and checksum are published below.
  execFileSync(process.execPath, [join(sourceHere, 'build-local-app.mjs')], {
    cwd: sourceDesktop,
    env: { ...process.env, CARGO_TARGET_DIR: buildTarget },
    stdio: 'inherit',
  });
  await mkdir(stagedPayload, { mode: 0o755 });
  execFileSync('/usr/bin/ditto', [app, join(stagedPayload, 'Mesh.app')], { stdio: 'inherit' });
  await copyFile(sourceGuide, join(stagedPayload, GUIDE_NAME));
  await copyFile(sourceSessionGuide, join(stagedPayload, SESSION_GUIDE_NAME));
  await copyVisualGuide(stagedPayload);
  execFileSync(
    '/usr/bin/ditto',
    ['-c', '-k', '--norsrc', stagedPayload, stagedArchive],
    { stdio: 'inherit' },
  );
  const checksum = await sha256(stagedArchive);
  const record = {
    schema: 'mesh-macos-alpha-archive/v2',
    artifact: basename(archive),
    sha256: checksum,
    source_revision: revision,
    source_exact: true,
    platform: 'macos',
    architecture,
    minimum_macos: '11.0',
    bundle_signature: 'adhoc',
    developer_id_signed: false,
    notarized: false,
    human_approval_available: false,
    human_approval_reason: 'validated Apple application identity required',
    approval_credential_continuity: false,
    gatekeeper: 'downloaded copies require an explicit macOS Open confirmation',
  };
  await writeFile(stagedManifest, `${JSON.stringify(record, null, 2)}\n`, { flag: 'wx', mode: 0o644 });
  await writeFile(stagedChecksum, canonicalAlphaChecksum(record), { flag: 'wx', mode: 0o644 });
  await mkdir(stagedDeliveryFolder, { recursive: true, mode: 0o755 });
  await Promise.all([
    copyFile(sourceGuide, join(stagedDeliveryFolder, GUIDE_NAME)),
    copyFile(sourceSessionGuide, join(stagedDeliveryFolder, SESSION_GUIDE_NAME)),
    copyFile(stagedArchive, join(stagedDeliveryFolder, basename(archive))),
    copyFile(stagedManifest, join(stagedDeliveryFolder, basename(manifest))),
    copyFile(stagedChecksum, join(stagedDeliveryFolder, basename(checksumFile))),
  ]);
  await copyVisualGuide(stagedDeliveryFolder);
  execFileSync(
    '/usr/bin/ditto',
    ['-c', '-k', '--norsrc', stagedDeliveryPayload, stagedDelivery],
    { stdio: 'inherit' },
  );
  const deliveryEntries = execFileSync('/usr/bin/unzip', ['-Z1', stagedDelivery], {
    encoding: 'utf8',
  })
    .split('\n')
    .filter(Boolean)
    .filter((entry) => !entry.endsWith('/'));
  assert.deepEqual(
    deliveryEntries.sort(),
    [
      `${stem}/${GUIDE_NAME}`,
      `${stem}/${SESSION_GUIDE_NAME}`,
      `${stem}/${VISUAL_GUIDE_NAME}`,
      ...VISUAL_SCREENSHOTS.map((name) => `${stem}/${VISUAL_ASSET_DIRECTORY}/${name}`),
      `${stem}/${basename(archive)}`,
      `${stem}/${basename(manifest)}`,
      `${stem}/${basename(checksumFile)}`,
    ].sort(),
    'the alpha delivery must contain only its guides, screenshots, and independently verifiable app artifacts',
  );
  await publishVerifiedAlphaArtifacts({
    stagedArchive,
    stagedManifest,
    stagedChecksum,
    stagedDelivery,
    archive,
    manifest,
    checksumFile,
    delivery,
    verify: (candidateManifest) => {
      execFileSync(
        process.execPath,
        [join(sourceHere, 'verify-alpha-archive.mjs'), '--manifest', candidateManifest, '--prove-rendered'],
        { cwd: sourceDesktop, stdio: 'inherit' },
      );
    },
  });
  process.stdout.write(`${JSON.stringify({
    delivery,
    delivery_sha256: await sha256(delivery),
    archive,
    manifest,
    checksum_file: checksumFile,
    ...record,
  })}\n`);
} finally {
  await rm(staging, { recursive: true, force: true });
}
