import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import {
  assertCanonicalAlphaChecksum,
  canonicalAlphaChecksum,
} from './alpha-checksum.mjs';

const config = JSON.parse(await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url)));
const pkg = JSON.parse(await readFile(new URL('../package.json', import.meta.url)));
const readme = await readFile(new URL('../README.md', import.meta.url), 'utf8');
const rootReadme = await readFile(new URL('../../../README.md', import.meta.url), 'utf8');
const userGuide = await readFile(new URL('../../../docs/user-guide.md', import.meta.url), 'utf8');
const developerGuide = await readFile(
  new URL('../../../docs/developer-guide.md', import.meta.url),
  'utf8',
);
const verifier = await readFile(new URL('./verify-local-app.mjs', import.meta.url), 'utf8');
const localBuilder = await readFile(new URL('./build-local-app.mjs', import.meta.url), 'utf8');
const alphaArchiver = await readFile(new URL('./archive-alpha-app.mjs', import.meta.url), 'utf8');
const alphaArchiverArguments = await readFile(
  new URL('./archive-alpha-app-args.mjs', import.meta.url),
  'utf8',
);
const alphaVerifier = await readFile(new URL('./verify-alpha-archive.mjs', import.meta.url), 'utf8');
const alphaGuide = await readFile(new URL('../ALPHA-START-HERE.txt', import.meta.url), 'utf8');
const alphaSessionGuide = await readFile(
  new URL('../ALPHA-FIRST-SESSION.txt', import.meta.url),
  'utf8',
);
const alphaVisualGuide = await readFile(
  new URL('../ALPHA-VISUAL-GUIDE.html', import.meta.url),
  'utf8',
);
const renderedProof = await readFile(new URL('./prove-rendered-app.mjs', import.meta.url), 'utf8');
const renderedProofArguments = await readFile(
  new URL('./prove-rendered-app-args.mjs', import.meta.url),
  'utf8',
);
const renderedProofImage = await readFile(
  new URL('./rendered-proof-image.mjs', import.meta.url),
  'utf8',
);
const windowProbe = await readFile(new URL('./window-proof.m', import.meta.url), 'utf8');
const buildScript = await readFile(new URL('../src-tauri/build.rs', import.meta.url), 'utf8');

function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function generatedIconBytes() {
  const encoded = buildScript.match(/DEVELOPMENT_ICON_BASE64: &str = "([A-Za-z0-9+/=]+)"/)?.[1];
  assert.ok(encoded, 'the generated icon bytes must remain explicit');
  return Buffer.from(encoded, 'base64');
}

test('the local desktop package emits one macOS application bundle', () => {
  assert.equal(pkg.engines.node, '>=22.18.0');
  assert.match(readme, /Desktop development requires Node\.js 22\.18 or newer/);
  assert.match(developerGuide, /Node\.js 22\.18 or newer/);
  assert.equal(config.productName, 'Mesh');
  assert.equal(config.version, '0.1.0');
  assert.equal(config.identifier, 'dev.mesh.desktop');
  assert.equal(config.bundle.active, true);
  assert.deepEqual(config.bundle.targets, ['app']);
  assert.deepEqual(config.bundle.icon, ['icons/icon.png']);
  assert.equal(config.bundle.macOS.minimumSystemVersion, '11.0');
  assert.equal(pkg.scripts['tauri:bundle-local'], 'node scripts/build-local-app.mjs');
  assert.match(localBuilder, /rev-parse', '--verify', 'HEAD/);
  assert.match(localBuilder, /git\('status', '--porcelain'\)/);
  assert.match(localBuilder, /MESH_BUILD_REVISION: revision/);
  assert.match(localBuilder, /process\.env\.CARGO_TARGET_DIR/);
  assert.match(localBuilder, /release\/bundle\/macos\/Mesh\.app/);
  assert.match(localBuilder, /process\.env\.CARGO/);
  assert.match(localBuilder, /\.cargo\/bin\/cargo/);
  assert.match(localBuilder, /existsSync\(rustupCargo\)/);
  assert.match(localBuilder, /\['run', 'ui:next:install'\]/);
  assert.match(localBuilder, /execFileSync\(cargo, \['tauri', 'build', '--bundles', 'app'\]/);
  assert.match(localBuilder, /\['tauri', 'build', '--bundles', 'app'\]/);
  assert.match(localBuilder, /\['--force', '--sign', '-', '--timestamp=none', app\]/);
  assert.match(localBuilder, /\/usr\/bin\/codesign/);
  assert.match(localBuilder, /verify-local-app\.mjs/);
  assert.match(localBuilder, /'--app', app,[\s\S]*'--revision', revision/);
  assert.doesNotMatch(localBuilder, /\bshell\s*:/);
  assert.match(buildScript, /cargo:rerun-if-env-changed=MESH_BUILD_REVISION/);
  assert.match(buildScript, /cargo:rustc-env=MESH_BUILD_REVISION=/);
  assert.match(readme, /tauri-cli --version 2\.11\.4 --locked/);
});

test('the generated development icon is a CRC-valid PNG', () => {
  const bytes = generatedIconBytes();
  assert.equal(bytes.subarray(0, 8).toString('hex'), '89504e470d0a1a0a');
  let offset = 8;
  const types = [];
  while (offset < bytes.length) {
    const length = bytes.readUInt32BE(offset);
    const typeAndData = bytes.subarray(offset + 4, offset + 8 + length);
    const expected = bytes.readUInt32BE(offset + 8 + length);
    assert.equal(crc32(typeAndData), expected);
    types.push(typeAndData.subarray(0, 4).toString('ascii'));
    offset += 12 + length;
  }
  assert.deepEqual(types, ['IHDR', 'IDAT', 'IEND']);
  assert.equal(offset, bytes.length);
  assert.equal(bytes.readUInt32BE(16), 128);
  assert.equal(bytes.readUInt32BE(20), 128);
  assert.equal(bytes[25], 6, 'the icon must use RGBA color');
});

test('bundle verification checks the runnable envelope and keeps distribution claims out', () => {
  assert.match(verifier, /constants\.R_OK \| constants\.X_OK/);
  assert.match(verifier, /CFBundleIdentifier/);
  assert.match(verifier, /CFBundleDisplayName/);
  assert.match(verifier, /CFBundleShortVersionString/);
  assert.match(verifier, /CFBundleVersion/);
  assert.match(verifier, /'0\.1\.0'/);
  assert.match(verifier, /LSMinimumSystemVersion/);
  assert.match(verifier, /cmd LC_BUILD_VERSION/);
  assert.match(verifier, /minimumLoadCommand/);
  assert.match(verifier, /Mesh\.icns/);
  assert.match(verifier, /mesh-local-app-proof\/v2/);
  assert.match(verifier, /--revision must name the exact bundled Git commit/);
  assert.match(verifier, /the bundled executable must contain the exact source revision/);
  assert.match(verifier, /Use your existing project/);
  assert.match(verifier, /Detach Mesh/);
  assert.match(verifier, /Reattach project/);
  assert.match(verifier, /Pinned comparisons/);
  assert.match(verifier, /Review what Mesh will bring in/);
  assert.match(verifier, /Recommended next action/);
  assert.match(verifier, /Approve exact version/);
  assert.match(verifier, /component_interface_embedded: true/);
  assert.match(verifier, /source_revision: revision/);
  assert.match(verifier, /source_exact: true/);
  assert.match(verifier, /--mesh-build-identity/);
  assert.match(verifier, /verifyBuildIdentity\(buildIdentity.stdout, revision\)/);
  assert.match(verifier, /\['--verify', '--deep', '--strict', '--verbose=4', app\]/);
  assert.match(verifier, /Signature=adhoc/);
  assert.match(verifier, /bundle_sealed: true/);
  assert.match(verifier, /signature: 'adhoc'/);
  assert.match(verifier, /developer_id_signed: false/);
  assert.match(verifier, /signed: false/);
  assert.match(verifier, /notarized: false/);
  assert.match(verifier, /com\\\.apple\\\.application-identifier\|keychain-access-groups/);
  assert.match(verifier, /human_approval_available: false/);
  assert.match(verifier, /validated Apple application identity required/);
  assert.match(verifier, /approval_credential_continuity: false/);
  assert.match(pkg.scripts['tauri:prove-local'], /prove-rendered-app\.mjs/);
  assert.ok(
    renderedProof.indexOf('parseProofArguments(process.argv.slice(2))')
      < renderedProof.indexOf("mkdtemp('/tmp/mesh-app-')"),
    'the proof must validate arguments before creating disposable state',
  );
  assert.match(renderedProofArguments, /unknown argument:/);
  assert.match(renderedProofArguments, /--screenshot may be provided only once/);
  assert.match(renderedProof, /spawn\(executable/);
  assert.match(renderedProof, /mkdtemp\('\/tmp\/mesh-app-'\)/);
  assert.match(renderedProof, /CFFIXED_USER_HOME: home/);
  assert.match(renderedProof, /MESH_RENDERER_PROOF_SCREENSHOT: '1'/);
  assert.match(renderedProof, /the app-owned Files screenshot was not PNG/);
  assert.match(renderedProof, /writeNewPrivateScreenshot\(screenshot, screenshotBytes\)/);
  assert.doesNotMatch(renderedProof, /\/usr\/sbin\/screencapture/);
  assert.doesNotMatch(renderedProof, /env: \{ \.\.\.process\.env, HOME: home/);
  assert.match(renderedProof, /const canonicalWorkspace = await realpath\(empty\.initialized\.root\)/);
  assert.match(renderedProof, /await realpath\(state\.root\)/);
  assert.match(renderedProof, /restart did not upgrade the remembered workspace to its canonical path/);
  assert.match(renderedProof, /rememberedImport\.document\.schema,[\s\S]*mesh-desktop-recent-workspaces\/v9/);
  assert.match(renderedProof, /mesh-desktop-recent-workspaces\/v7/);
  assert.doesNotMatch(renderedProof, /mesh-desktop-recent-workspaces\/v[1-6]/);
  assert.match(renderedProof, /original_update_version: null/);
  assert.match(renderedProof, /project_root: canonicalSource/);
  assert.match(renderedProof, /requestDaemon\('workspace\.state', \{\}, totalTimeoutMs\)/);
  assert.match(renderedProof, /const deadline = Date\.now\(\) \+ proofDaemonIdleTimeoutMs\('workspace\.state'\)/);
  assert.match(renderedProof, /globalThis\.setTimeout\([\s\S]*overall deadline/);
  assert.doesNotMatch(renderedProof, /requestDaemon\('folder\.import\.(?:preview|confirm)'/);
  assert.match(renderedProof, /const importedWorkspace = await waitForWorkspace/);
  assert.match(renderedProof, /const privateStore = dirname\(importedWorkspace\.root\)/);
  assert.match(renderedProof, /the visible import did not remember its exact managed workspace/);
  assert.match(renderedProof, /the seeded large-workspace review did not exercise its bounded state/);
  assert.match(renderedProof, /mirrorOrdinaryDirectories\(canonicalWorkspace, privateExport/);
  assert.match(renderedProof, /async function waitForPrivateExportReceipt\(\)/);
  assert.match(renderedProof, /Promise\.all\(expected\.map\(\(relative\) => lstat\(join\(privateExport, relative\)\)\)\)/);
  assert.match(renderedProof, /waitForPrivateExportReceipt,[\s\S]*'private-export'/);
  assert.match(renderedProof, /the packaged private export omitted the current post-agent text result/);
  assert.match(renderedProof, /the packaged private export omitted the current post-agent image result/);
  assert.ok(
    renderedProof.indexOf('const agentHandoff = await launch(')
      < renderedProof.indexOf('const exported = await launch('),
    'the real-workspace proof must create one complete post-handoff review before private export',
  );
  assert.doesNotMatch(renderedProof, /const privateStore = join\(scratch, 'workspace\.mesh'\)/);
  assert.match(renderedProof, /app_managed_storage: true/);
  assert.match(renderedProof, /requestDaemon\('workspace\.version\.fork'/);
  assert.match(renderedProof, /const IPC_VERSION = 7/);
  assert.match(renderedProof, /message\.t === 'chunk'/);
  assert.match(renderedProof, /MAX_DAEMON_MESSAGE_BYTES/);
  assert.match(renderedProof, /message\.surface_version, IPC_VERSION/);
  assert.match(renderedProof, /verifyStableFolder\(expectedWorkspace\)/);
  assert.match(renderedProof, /ordinary edit through the stable folder was not surfaced as unsaved native work/);
  assert.match(renderedProof, /native_working_edit_detected: restarted\.result\.nativeEdit/);
  assert.match(renderedProof, /agent_path_stayed_pinned: true/);
  assert.match(renderedProof, /await chmod\(join\(source, 'run\.sh'\), 0o755\)/);
  assert.match(renderedProof, /loading the saved workspace version lost its executable bit/);
  assert.match(renderedProof, /execFileSync\(join\(forkWorkspace, 'run\.sh'\)/);
  assert.match(renderedProof, /executable_version_ran_natively: true/);
  assert.match(renderedProof, /const pinnedAgentContext = await proveCodexContextBridge\(state\)/);
  assert.match(renderedProof, /const AGENT_PROOF_RESULT_PATH = 'agent-proof-result\.txt'/);
  assert.match(renderedProof, /const AGENT_PROOF_IMAGE_PATH = 'agent-proof-result\.png'/);
  assert.match(renderedProof, /readRenderedProofImage\(\)/);
  assert.doesNotMatch(renderedProof, /src-tauri\/icons\/icon\.png/);
  assert.match(renderedProofImage, /new URL\('\.\.\/src-tauri\/build\.rs', import\.meta\.url\)/);
  assert.match(renderedProofImage, /DEVELOPMENT_ICON_BASE64/);
  assert.match(renderedProofImage, /89504e470d0a1a0a/);
  assert.match(renderedProof, /assert\.deepEqual\([\s\S]*AGENT_PROOF_IMAGE/);
  assert.match(renderedProof, /join\(pinnedAgentContext\.root, 'agent-pinned\.txt'\)/);
  assert.match(renderedProof, /pinned_agent_context: pinnedAgentContext/);
  assert.match(renderedProof, /selected_agent_context: restarted\.result\.selectedAgentContext/);
  assert.ok(
    renderedProof.indexOf('const pinnedAgentContext = await proveCodexContextBridge(state)')
      < renderedProof.indexOf("requestDaemon('workspace.version.fork'"),
    'the real-app proof must acquire the pinned agent folder before switching versions',
  );
  assert.match(renderedProof, /'--mesh-mcp'/);
  assert.match(renderedProof, /method: 'tools\/call'/);
  assert.match(renderedProof, /name: 'mesh_workspace_state'/);
  assert.match(renderedProof, /setTimeout\(\(\) => resolveDeadline\(null\), 6 \* 60_000\)/);
  assert.match(renderedProof, /clearTimeout\(deadline\)/);
  assert.match(renderedProof, /maxRetries: 10,[\s\S]*retryDelay: 100/);
  assert.match(renderedProof, /the agent bridge returned the wrong native root/);
  assert.match(renderedProof, /read_only: listed\.tools\[0\]\.annotations\?\.readOnlyHint === true/);
  assert.match(renderedProof, /original_preserved: true/);
  assert.match(renderedProof, /const first = await launch\('first', canonicalWorkspace/);
  assert.match(renderedProof, /const empty = await launchEmpty\(\)/);
  assert.match(renderedProof, /code: 'no-workspace-open', service_ready: true/);
  assert.match(renderedProof, /explicit_first_open/);
  assert.match(renderedProof, /const restarted = await launch\('restarted'/);
  assert.match(renderedProof, /stable_across_restart: true/);
  assert.match(renderedProof, /concurrent_process_refused: true/);
  assert.match(renderedProof, /attention_forwarded: true/);
  assert.match(renderedProof, /first_process_retained_endpoint: true/);
  assert.match(renderedProof, /window-proof\.m/);
  assert.match(renderedProof, /SIGTERM/);
  assert.match(windowProbe, /CGWindowListCopyWindowInfo/);
  assert.match(windowProbe, /candidates=/);
  assert.match(windowProbe, /title != nil && !\[title isEqualToString:@"Mesh — Local workspace"\]/);
  assert.match(windowProbe, /width\.doubleValue < 920/);
  assert.match(windowProbe, /height\.doubleValue < 640/);
  assert.match(readme, /unsigned by an\s+Apple Developer ID and unnotarized/);
  assert.match(readme, /ad-hoc resource seal but is not signed with an Apple Developer ID or\s+notarized/);
  assert.doesNotMatch(readme, /macOS bundle is unsigned/);
  assert.match(readme, /refuses tracked source changes/);
  assert.match(readme, /embeds the exact Git commit/);
  assert.match(readme, /not that a\s+distributable installer or Apple trust chain exists/);
  assert.match(readme, /ad-hoc resource seal/);
  assert.match(rootReadme, /a build signed with a stable, Apple-validated application identity can enroll/);
  assert.match(rootReadme, /current ad-hoc technical-alpha archive reports \*\*Approval unavailable\*\*/);
  assert.doesNotMatch(rootReadme, /On supported Macs, the desktop can enroll/);
  assert.match(userGuide, /ad-hoc technical-alpha archive exercises \*\*Saved privately\*\* and recorded review/);
  assert.match(userGuide, /Apple-signed, identity-continuous build additionally/);
  assert.match(userGuide, /ad-hoc technical-alpha archive reports this approval path unavailable/);
});

test('the alpha archive remains checksum-verifiable without claiming Apple trust', () => {
  assert.equal(pkg.scripts['tauri:archive-alpha'], 'node scripts/archive-alpha-app.mjs');
  assert.equal(pkg.scripts['tauri:verify-alpha'], 'node scripts/verify-alpha-archive.mjs');
  assert.match(alphaVerifier, /mesh-macos-alpha-archive\/v2/);
  assert.ok(
    alphaArchiver.indexOf('parseArchiveArguments(process.argv.slice(2))')
      < alphaArchiver.indexOf("git('rev-parse', '--verify', 'HEAD')"),
    'the archive must validate arguments before inspecting Git or building the app',
  );
  assert.match(alphaArchiverArguments, /help must be used without other arguments/);
  assert.match(alphaArchiverArguments, /unknown argument:/);
  assert.match(alphaArchiverArguments, /without building Mesh or writing release artifacts/);
  assert.match(alphaArchiver, /build-local-app\.mjs/);
  assert.match(alphaArchiver, /const buildTarget = join\(staging, 'target'\)/);
  assert.match(alphaArchiver, /CARGO_TARGET_DIR: buildTarget/);
  assert.ok(
    alphaArchiver.indexOf("const staging = await mkdtemp")
      < alphaArchiver.indexOf("execFileSync(process.execPath, [join(sourceHere, 'build-local-app.mjs')]"),
    'the alpha build must enter private staging before it can write a bundle',
  );
  assert.match(alphaArchiver, /\['clone', '--local', '--no-hardlinks', '--no-checkout', repository, source\]/);
  assert.match(alphaArchiver, /\['-C', source, 'checkout', '--detach', revision\]/);
  assert.doesNotMatch(alphaArchiver, /const app = join\(bundleDirectory, 'Mesh\.app'\)/);
  assert.match(alphaArchiver, /copyFile\(sourceGuide, join\(stagedPayload, GUIDE_NAME\)\)/);
  assert.match(alphaArchiver, /copyFile\(sourceSessionGuide, join\(stagedPayload, SESSION_GUIDE_NAME\)\)/);
  assert.match(alphaArchiver, /copyVisualGuide\(stagedPayload\)/);
  assert.match(alphaArchiver, /copyVisualGuide\(stagedDeliveryFolder\)/);
  assert.match(alphaArchiver, /VISUAL_GUIDE_NAME = 'VISUAL-GUIDE\.html'/);
  assert.match(alphaArchiver, /review-workbench\.jpg/);
  assert.match(alphaArchiver, /review-pdf\.jpg/);
  assert.match(alphaArchiver, /review-text-diff\.jpg/);
  assert.match(alphaArchiver, /\['-c', '-k', '--norsrc', stagedPayload, stagedArchive\]/);
  assert.match(alphaArchiver, /createHash\('sha256'\)/);
  assert.match(alphaArchiver, /canonicalAlphaChecksum\(record\)/);
  assert.match(alphaArchiver, /bundle_signature: 'adhoc'/);
  assert.match(alphaArchiver, /developer_id_signed: false/);
  assert.match(alphaArchiver, /notarized: false/);
  assert.match(alphaArchiver, /human_approval_available: false/);
  assert.match(alphaArchiver, /validated Apple application identity required/);
  assert.match(alphaArchiver, /approval_credential_continuity: false/);
  assert.match(alphaArchiver, /minimum_macos: '11\.0'/);
  assert.match(alphaArchiver, /'--prove-rendered'/);
  assert.match(alphaVerifier, /the alpha archive checksum must match its manifest/);
  assert.match(alphaVerifier, /the alpha manifest path must end in \.json/);
  assert.match(alphaVerifier, /assertCanonicalAlphaChecksum\(checksumBytes, record\)/);
  assert.match(alphaVerifier, /the alpha manifest must use its exact canonical JSON encoding/);
  assert.match(alphaVerifier, /archive entry must not traverse/);
  assert.match(alphaVerifier, /archive entry must not contain macOS metadata sidecars/);
  assert.match(alphaVerifier, /the archive must extract its alpha start guide/);
  assert.match(alphaVerifier, /assertRegularExactFile\(extractedGuide, expectedGuide/);
  assert.match(alphaVerifier, /assertRegularExactFile\(extractedVisualGuide, expectedVisualGuide/);
  assert.match(alphaVerifier, /assertUniqueArchiveEntries\(entries, 'the alpha archive'\)/);
  assert.match(alphaVerifier, /the visual guide must contain only the reviewed screenshots/);
  assert.match(alphaVerifier, /screenshot\.subarray\(0, 3\)\.toString\('hex'\), 'ffd8ff'/);
  assert.match(alphaVerifier, /verify-local-app\.mjs/);
  assert.match(alphaVerifier, /argument === '--prove-rendered'/);
  assert.match(alphaVerifier, /const desktop = resolve\(here, '\.\.'\)/);
  assert.match(alphaVerifier, /MESH_LOCAL_APP: join\(extraction, 'Mesh\.app'\)/);
  assert.match(alphaVerifier, /MESH_EXPECTED_BUILD_REVISION: record\.source_revision/);
  assert.match(alphaVerifier, /the desktop control suite must come from the exact source revision embedded in the archive/);
  assert.match(alphaVerifier, /the desktop control suite refuses tracked changes so it remains bound to the archive revision/);
  assert.match(alphaVerifier, /execFileSync\('npm', \['test'\], \{ cwd: desktop, stdio: 'inherit' \}\)/);
  assert.ok(
    alphaVerifier.indexOf("execFileSync('npm', ['test']")
      < alphaVerifier.indexOf("[join(here, 'prove-rendered-app.mjs')"),
    'the exact desktop control suite must pass before the extracted app journey runs',
  );
  assert.ok(
    alphaVerifier.indexOf("[join(here, 'verify-local-app.mjs')")
      < alphaVerifier.indexOf("[join(here, 'prove-rendered-app.mjs')"),
    'the extracted app envelope must verify before its windowed journey runs',
  );
  assert.match(alphaVerifier, /desktop_control_suite: proveRendered/);
  assert.match(alphaVerifier, /extracted_windowed_app_proof: proveRendered/);
  assert.match(alphaVerifier, /extracted_journey_driver: renderedAppProof \? 'renderer\+daemon-ipc' : null/);
  assert.match(alphaVerifier, /assertCompleteArchiveFixtureProof\([\s\S]*parseRenderedAppProofOutput\(renderedOutput\)/);
  assert.match(alphaVerifier, /extracted_renderer_controls_driven: renderedAppProof\?\.renderer_controls_driven === true/);
  assert.match(alphaVerifier, /extracted_workspace_versions_proved:/);
  assert.match(alphaVerifier, /versions\?\.outcome === 'verified-preview-ready'/);
  assert.match(alphaVerifier, /extracted_private_export_proved:/);
  assert.match(alphaVerifier, /extracted_agent_handoff_lifecycle_proved:/);
  assert.match(alphaVerifier, /agent_handoff\?\.outcome === 'agent-handoff-completed'/);
  assert.match(alphaVerifier, /private_export\?\.outcome === 'private-export-completed'/);
  assert.match(alphaVerifier, /component_interface_mounted: renderedAppProof\?\.component_interface_mounted === true/);
  assert.match(alphaVerifier, /renderer_proof: renderedAppProof\?\.renderer \|\| null/);
  assert.match(alphaVerifier, /component_interface_embedded: true/);
  assert.match(renderedProof, /rendererProofReportsFromText/);
  assert.match(renderedProof, /MESH_RENDERER_PROOF_NONCE/);
  assert.match(renderedProof, /rendererProofSession\('onboarding'\)/);
  assert.match(renderedProof, /'versions'/);
  assert.match(renderedProof, /'private-export'/);
  assert.match(renderedProof, /excludedRootNames: \['\.git'\]/);
  assert.match(renderedProof, /the packaged private export lost the executable bit/);
  assert.match(renderedProof, /the packaged private export changed the nested saved image bytes/);
  assert.match(renderedProof, /the private-export proof changed the unmanaged original/);
  assert.ok(
    renderedProof.indexOf("const exported = await launch(\n    'private-export'")
      < renderedProof.indexOf("const files = await launch(\n    'files'"),
    'native Files launches must follow the canonical workspace export proof',
  );
  assert.match(renderedProof, /rendererSurface = null/);
  assert.match(renderedProof, /}, 'review'\)/);
  assert.match(renderedProof, /mesh_desktop_build_revision/);
  assert.match(renderedProof, /mesh_desktop_build_exact/);
  assert.match(readme, /complete desktop control suite from the\s+same clean Git revision/);
  assert.match(readme, /daemon checks and nonce-bound renderer receipts must agree/);
  assert.match(readme, /exact saved file bytes and executable mode on disk/);
  assert.match(alphaVerifier, /assert\.equal\(record\.developer_id_signed, false\)/);
  assert.match(alphaVerifier, /assert\.equal\(record\.notarized, false\)/);
  assert.match(alphaVerifier, /assert\.equal\(record\.human_approval_available, false\)/);
  assert.match(alphaVerifier, /validated Apple application identity required/);
  assert.match(alphaVerifier, /assert\.equal\(record\.approval_credential_continuity, false\)/);
  assert.match(alphaVerifier, /assert\.equal\(record\.minimum_macos, '11\.0'\)/);
  assert.match(alphaArchiver, /writeFile\(stagedManifest/);
  assert.doesNotMatch(alphaArchiver, /writeFile\(manifest,/);
  assert.doesNotMatch(readme, /Mesh-alpha-<revision>/);
  assert.match(readme, /pass the exact printed `\.json` path/);
  assert.match(readme, /START-HERE\.txt/);
  assert.match(readme, /FIRST-SESSION\.txt/);
  assert.match(readme, /VISUAL-GUIDE\.html/);
  assert.match(readme, /Send the\s+single delivery ZIP/);
  assert.match(readme, /revision-named folder containing the three inner\s+verification files/);
  assert.match(alphaGuide, /one received\s+Mesh-alpha-<revision>-macos-<architecture>-delivery\.zip/);
  assert.match(alphaGuide, /FIRST-SESSION\.txt/);
  assert.match(alphaGuide, /VISUAL-GUIDE\.html/);
  assert.match(alphaGuide, /alpha-guide\/screenshots/);
  assert.match(alphaSessionGuide, /MESH ALPHA — YOUR FIRST 20 MINUTES/);
  assert.match(alphaSessionGuide, /Start Codex on this version/);
  assert.match(alphaSessionGuide, /Finish agent[\s\S]*handoff/);
  assert.match(alphaSessionGuide, /PDF, PowerPoint, Word, or\s+Excel/);
  assert.match(alphaSessionGuide, /Review is bound to a recorded exact version/);
  assert.match(alphaVisualGuide, /When a file seems missing/);
  assert.match(alphaSessionGuide, /I expected ___, but Mesh ___/);
  assert.match(alphaSessionGuide, /Please do not include private file contents, credentials, or keys/);
  assert.match(alphaGuide, /inner application archive \(not the outer delivery ZIP\)/);
  assert.match(alphaGuide, /shasum -a 256 -c \.\/Mesh-alpha-\*\.sha256/);
  assert.match(alphaGuide, /Quit every running copy of Mesh before extracting or opening this build/);
  assert.match(alphaGuide, /Never extract a new build over[\s\S]*a Mesh\.app that is running/);
  assert.match(alphaGuide, /System Settings → Privacy & Security/);
  assert.match(alphaGuide, /Open Anyway/);
  assert.match(alphaGuide, /cannot verify the developer/);
  assert.match(alphaGuide, /damaged, will damage your computer, or contains malware/);
  assert.doesNotMatch(alphaGuide, /control-click Mesh\.app/);
  assert.match(alphaGuide, /Never disable Gatekeeper or remove quarantine metadata/);
  assert.match(readme, /System Settings route:[\s\S]*Privacy & Security/);
  assert.match(readme, /Open Anyway/);
  assert.match(readme, /damaged, will damage your computer, or contains malware/);
  assert.match(alphaGuide, /never give the same pinned agent folder to two agents/);
  assert.match(alphaGuide, /choose[\s\S]*Finish agent handoff and confirm/);
  assert.doesNotMatch(alphaGuide, /Agent finished/);
  assert.match(alphaGuide, /confirmed Finish inspects the complete native result and privately saves unambiguous/);
  assert.match(alphaGuide, /Mesh never guesses structural identity/);
  assert.match(alphaGuide, /app reports Approval unavailable instead of offering a setup action/);
  assert.match(alphaGuide, /Do not use this archive to[\s\S]*test approval or Update original folder/);
  assert.match(alphaGuide, /native folders, agents, private saves, reviews, and version switches/);
  assert.match(readme, /after reviewing and approving that exact saved version/);
  assert.match(readme, /human_approval_available: false/);
  assert.match(readme, /Approval unavailable/);
  assert.match(readme, /Quit every[\s\S]*running copy of Mesh before extracting or opening a replacement build/);
  assert.match(readme, /never extract over a running `Mesh\.app`/);
  assert.match(readme, /review the explicit file-by-file[\s\S]*update preview before confirming any write/);
});

test('the alpha checksum sibling binds both the digest and archive basename', () => {
  const record = {
    artifact: 'Mesh-alpha-0123456789ab-macos-arm64.zip',
    sha256: 'a'.repeat(64),
  };
  const checksum = canonicalAlphaChecksum(record);
  assert.equal(checksum, `${'a'.repeat(64)}  Mesh-alpha-0123456789ab-macos-arm64.zip\n`);
  assert.doesNotThrow(() => assertCanonicalAlphaChecksum(checksum, record));
  assert.throws(
    () => assertCanonicalAlphaChecksum(checksum.replace('a', 'b'), record),
    /checksum file must canonically bind/,
  );
  assert.throws(
    () => assertCanonicalAlphaChecksum(checksum.replace('Mesh-alpha', 'Other-alpha'), record),
    /checksum file must canonically bind/,
  );
});
