#!/usr/bin/env node

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { lstat, mkdtemp, readFile, readdir, rm } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertCanonicalAlphaChecksum } from './alpha-checksum.mjs';
import {
  assertRegularExactFile,
  assertUniqueArchiveEntries,
} from './alpha-guide-verification.mjs';
import {
  assertCompleteArchiveFixtureProof,
  parseRenderedAppProofOutput,
} from './renderer-proof-protocol.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const desktop = resolve(here, '..');
const repository = resolve(desktop, '../..');
const expectedGuide = join(here, '..', 'ALPHA-START-HERE.txt');
const expectedSessionGuide = join(here, '..', 'ALPHA-FIRST-SESSION.txt');
const expectedVisualGuide = join(here, '..', 'ALPHA-VISUAL-GUIDE.html');
const GUIDE_NAME = 'START-HERE.txt';
const SESSION_GUIDE_NAME = 'FIRST-SESSION.txt';
const VISUAL_GUIDE_NAME = 'VISUAL-GUIDE.html';
const VISUAL_ASSET_DIRECTORY = 'alpha-guide/screenshots';
const VISUAL_SCREENSHOTS = Object.freeze([
  'review-workbench.jpg',
  'review-pdf.jpg',
  'review-text-diff.jpg',
]);
const usage = `Usage: npm run tauri:verify-alpha --prefix apps/desktop -- --manifest <path> [--prove-rendered]

Verify one exact ad-hoc macOS alpha archive and its canonical manifest/checksum.

Options:
  --manifest <path>  Canonical mesh-macos-alpha-archive/v2 manifest to verify.
  --prove-rendered   Run the exact desktop controls and extracted windowed-app proof.
  -h, --help         Show this help without reading an artifact or launching Mesh.`;
const arguments_ = process.argv.slice(2);
if (arguments_.some((argument) => argument === '-h' || argument === '--help')) {
  assert.equal(arguments_.length, 1, 'help must be used without other arguments');
  console.log(usage);
  process.exit(0);
}
let manifestPath = null;
let proveRendered = false;
for (let index = 2; index < process.argv.length; index += 1) {
  const argument = process.argv[index];
  if (argument === '--manifest') {
    assert.equal(manifestPath, null, '--manifest may be supplied only once');
    manifestPath = process.argv[++index];
    assert.ok(manifestPath && !manifestPath.startsWith('-'), '--manifest requires a value');
  } else if (argument === '--prove-rendered') {
    assert.equal(proveRendered, false, '--prove-rendered may be supplied only once');
    proveRendered = true;
  } else throw new Error(`unknown alpha archive verifier argument: ${argument}`);
}
assert.ok(manifestPath, '--manifest is required');
manifestPath = resolve(manifestPath);
assert.equal(
  manifestPath.endsWith('.json'),
  true,
  'the alpha manifest path must end in .json so its checksum sibling is unambiguous',
);

async function sha256(path) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}

const manifestBytes = await readFile(manifestPath, 'utf8');
const record = JSON.parse(manifestBytes);
assert.equal(
  manifestBytes,
  `${JSON.stringify(record, null, 2)}\n`,
  'the alpha manifest must use its exact canonical JSON encoding',
);
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
  'notarized',
  'human_approval_available',
  'human_approval_reason',
  'approval_credential_continuity',
  'gatekeeper',
]);
assert.equal(record.schema, 'mesh-macos-alpha-archive/v2');
assert.match(record.artifact, /^Mesh-alpha-[0-9a-f]{12}-macos-(arm64|x86_64)\.zip$/);
assert.match(record.sha256, /^[0-9a-f]{64}$/);
assert.match(record.source_revision, /^[0-9a-f]{40}$/);
assert.equal(record.artifact.includes(record.source_revision.slice(0, 12)), true);
assert.equal(record.source_exact, true);
assert.equal(record.platform, 'macos');
assert.match(record.architecture, /^(arm64|x86_64)$/);
assert.equal(record.minimum_macos, '11.0');
assert.equal(record.bundle_signature, 'adhoc');
assert.equal(record.developer_id_signed, false);
assert.equal(record.notarized, false);
assert.equal(record.human_approval_available, false);
assert.equal(record.human_approval_reason, 'validated Apple application identity required');
assert.equal(record.approval_credential_continuity, false);
assert.equal(record.gatekeeper, 'downloaded copies require an explicit macOS Open confirmation');

if (proveRendered) {
  const exactRevision = execFileSync(
    'git',
    ['-C', repository, 'rev-parse', '--verify', 'HEAD'],
    { encoding: 'utf8' },
  ).trim();
  assert.equal(
    exactRevision,
    record.source_revision,
    'the desktop control suite must come from the exact source revision embedded in the archive',
  );
  assert.equal(
    execFileSync('git', ['-C', repository, 'status', '--porcelain'], { encoding: 'utf8' }),
    '',
    'the desktop control suite refuses tracked changes so it remains bound to the archive revision',
  );
  execFileSync('npm', ['test'], { cwd: desktop, stdio: 'inherit' });
}

const archive = join(dirname(manifestPath), record.artifact);
const checksumFile = `${manifestPath.slice(0, -'.json'.length)}.sha256`;
const checksumBytes = await readFile(checksumFile, 'utf8');
assertCanonicalAlphaChecksum(checksumBytes, record);
assert.equal(await sha256(archive), record.sha256, 'the alpha archive checksum must match its manifest');
const entries = execFileSync('/usr/bin/unzip', ['-Z1', archive], { encoding: 'utf8' })
  .split('\n')
  .filter(Boolean);
assert.ok(entries.length > 0, 'the alpha archive must not be empty');
assertUniqueArchiveEntries(entries, 'the alpha archive');
for (const entry of entries) {
  assert.equal(entry.startsWith('/'), false, `archive entry must be relative: ${entry}`);
  assert.equal(entry.split('/').includes('..'), false, `archive entry must not traverse: ${entry}`);
  assert.equal(
    entry.split('/').some((part) => part === '__MACOSX' || part.startsWith('._')),
    false,
    `archive entry must not contain macOS metadata sidecars: ${entry}`,
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

const extraction = await mkdtemp(join(tmpdir(), 'mesh-alpha-verify-'));
let renderedAppProof = null;
try {
  execFileSync('/usr/bin/ditto', ['-x', '-k', archive, extraction], { stdio: 'inherit' });
  const extracted = await readdir(extraction);
  assert.ok(extracted.includes('Mesh.app'), 'the archive must extract one Mesh.app bundle');
  assert.ok(extracted.includes(GUIDE_NAME), 'the archive must extract its alpha start guide');
  assert.ok(
    extracted.includes(SESSION_GUIDE_NAME),
    'the archive must extract its first-session guide',
  );
  assert.ok(extracted.includes(VISUAL_GUIDE_NAME), 'the archive must extract its visual guide');
  assert.ok(extracted.includes('alpha-guide'), 'the archive must extract its visual assets');
  assert.equal(
    extracted.every(
      (entry) => entry === 'Mesh.app'
        || entry === GUIDE_NAME
        || entry === SESSION_GUIDE_NAME
        || entry === VISUAL_GUIDE_NAME
        || entry === 'alpha-guide',
    ),
    true,
    'the archive must not extract unrelated top-level entries',
  );
  const extractedGuide = join(extraction, GUIDE_NAME);
  const extractedSessionGuide = join(extraction, SESSION_GUIDE_NAME);
  const extractedVisualGuide = join(extraction, VISUAL_GUIDE_NAME);
  const extractedVisualAssets = join(extraction, VISUAL_ASSET_DIRECTORY);
  const visualAssetsStat = await lstat(extractedVisualAssets);
  assert.equal(
    visualAssetsStat.isDirectory() && !visualAssetsStat.isSymbolicLink(),
    true,
    'the visual assets must be one real directory',
  );
  await assertRegularExactFile(extractedGuide, expectedGuide, 'the alpha start guide');
  await assertRegularExactFile(
    extractedSessionGuide,
    expectedSessionGuide,
    'the first-session guide',
  );
  await assertRegularExactFile(extractedVisualGuide, expectedVisualGuide, 'the visual guide');
  assert.deepEqual(
    (await readdir(extractedVisualAssets)).sort(),
    [...VISUAL_SCREENSHOTS].sort(),
    'the visual guide must contain only the reviewed screenshots',
  );
  for (const name of VISUAL_SCREENSHOTS) {
    const extractedScreenshot = join(extractedVisualAssets, name);
    const expectedScreenshot = join(desktop, VISUAL_ASSET_DIRECTORY, name);
    const screenshot = await assertRegularExactFile(
      extractedScreenshot,
      expectedScreenshot,
      `the visual screenshot ${name}`,
    );
    assert.equal(screenshot.subarray(0, 3).toString('hex'), 'ffd8ff');
  }
  execFileSync(
    process.execPath,
    [join(here, 'verify-local-app.mjs'), '--app', join(extraction, 'Mesh.app'), '--revision', record.source_revision],
    { stdio: 'inherit' },
  );
  if (proveRendered) {
    const renderedOutput = execFileSync(
      process.execPath,
      [join(here, 'prove-rendered-app.mjs')],
      {
        cwd: desktop,
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'inherit'],
        env: {
          ...process.env,
          MESH_LOCAL_APP: join(extraction, 'Mesh.app'),
          MESH_EXPECTED_BUILD_REVISION: record.source_revision,
        },
      },
    );
    renderedAppProof = assertCompleteArchiveFixtureProof(
      parseRenderedAppProofOutput(renderedOutput),
    );
  }
} finally {
  await rm(extraction, { recursive: true, force: true });
}

process.stdout.write(`${JSON.stringify({
  schema: record.schema,
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
  component_interface_embedded: true,
  component_interface_mounted: renderedAppProof?.component_interface_mounted === true,
  renderer_proof: renderedAppProof?.renderer || null,
  ...record,
})}\n`);
