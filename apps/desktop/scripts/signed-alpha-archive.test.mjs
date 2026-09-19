import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const pkg = JSON.parse(await readFile(new URL('../package.json', import.meta.url)));
const runner = await readFile(new URL('./archive-signed-alpha-app.mjs', import.meta.url), 'utf8');
const verifier = await readFile(new URL('./verify-signed-alpha-archive.mjs', import.meta.url), 'utf8');
const readme = await readFile(new URL('../README.md', import.meta.url), 'utf8');
const guide = await readFile(new URL('../SIGNED-ALPHA-START-HERE.txt', import.meta.url), 'utf8');
const sessionGuide = await readFile(new URL('../ALPHA-FIRST-SESSION.txt', import.meta.url), 'utf8');
const visualGuide = await readFile(new URL('../ALPHA-VISUAL-GUIDE.html', import.meta.url), 'utf8');

test('the signed alpha command keeps secrets external and publishes only after full verification', () => {
  assert.equal(pkg.scripts['tauri:archive-signed-alpha'], 'node scripts/archive-signed-alpha-app.mjs');
  assert.equal(pkg.scripts['tauri:verify-signed-alpha'], 'node scripts/verify-signed-alpha-archive.mjs');
  assert.match(runner, /macos-signing-preflight\.mjs/);
  assert.match(runner, /signingReadinessCommand\(preflightArguments\)/);
  assert.match(runner, /signingReadinessFailure\(report\)/);
  assert.match(runner, /const pinnedProfile = join\(staging, 'Mesh\.provisionprofile'\)/);
  assert.match(runner, /unchangedRegularFile\(sourceProfile, sourceProfileStat\)/);
  assert.match(runner, /'clone', '--local', '--no-hardlinks', '--no-checkout'/);
  assert.match(runner, /'checkout', '--detach', revision/);
  assert.match(runner, /'ci', '--ignore-scripts'/);
  assert.match(runner, /join\(sourceHere, 'build-local-app\.mjs'\)/);
  assert.match(runner, /join\(sourceHere, 'verify-signed-alpha-archive\.mjs'\)/);
  assert.match(runner, /copyFile\(sourceSessionGuide, join\(stagedPayload, SESSION_GUIDE_NAME\)\)/);
  assert.match(runner, /copyVisualGuide\(stagedPayload\)/);
  assert.match(runner, /VISUAL_GUIDE_NAME = 'VISUAL-GUIDE\.html'/);
  assert.ok(
    runner.indexOf("['-C', source, 'checkout', '--detach', revision]")
      < runner.indexOf("join(sourceHere, 'build-local-app.mjs')"),
    'the build must run only after exact source materialization',
  );
  assert.match(runner, /'--options', 'runtime'/);
  assert.match(runner, /'--timestamp'/);
  assert.match(runner, /notarytool', 'submit'/);
  assert.match(runner, /'--timeout', '30m'/);
  assert.match(runner, /assertNotaryLog\(notaryLog, submissionId\)/);
  assert.match(runner, /stapler', 'staple'/);
  assert.match(runner, /stapler', 'validate'/);
  assert.match(runner, /spctl.*--assess/s);
  assert.ok(
    runner.indexOf("['stapler', 'staple', app]")
      < runner.indexOf("['-c', '-k', '--norsrc', stagedPayload, stagedArchive]"),
    'the final ZIP must contain the stapled app',
  );
  assert.ok(
    runner.indexOf('await publishVerifiedAlphaArtifacts({')
      > runner.indexOf('await writeFile(stagedChecksum'),
    'public artifact paths must change only through the verified publisher',
  );
  assert.doesNotMatch(runner, /apple-id|app-specific-password|issuer|private[-_ ]key/i);
});

test('the verifier independently proves Apple identity and the exact application', () => {
  assert.match(verifier, /mesh-macos-signed-alpha-archive\/v1/);
  assert.match(verifier, /--extract-certificates/);
  assert.match(verifier, /createHash\('sha1'\)/);
  assert.match(verifier, /assertSignedEntitlements/);
  assert.match(verifier, /embedded\.provisionprofile/);
  assert.match(verifier, /evaluateProvisioningProfile/);
  assert.match(verifier, /flags=0x\[0-9a-f\]\*10000/);
  assert.match(verifier, /Timestamp=/);
  assert.match(verifier, /stapler', 'validate'/);
  assert.match(verifier, /spctl.*--assess/s);
  assert.match(verifier, /executable\.includes\(Buffer\.from\(record\.source_revision\)\)/);
  assert.match(verifier, /prove-rendered-app\.mjs/);
  assert.match(verifier, /assertCompleteArchiveFixtureProof\([\s\S]*parseRenderedAppProofOutput\(renderedOutput\)/);
  assert.match(verifier, /extracted_journey_driver: renderedAppProof \? 'renderer\+daemon-ipc' : null/);
  assert.match(verifier, /extracted_renderer_controls_driven: renderedAppProof\?\.renderer_controls_driven === true/);
  assert.match(verifier, /extracted_workspace_versions_proved:/);
  assert.match(verifier, /extracted_agent_handoff_lifecycle_proved:/);
  assert.match(verifier, /versions\?\.outcome === 'verified-preview-ready'/);
  assert.match(verifier, /component_interface_mounted: renderedAppProof\?\.component_interface_mounted === true/);
  assert.match(verifier, /signed archive entry must not contain macOS metadata sidecars/);
  assert.match(verifier, /expectedSessionGuide/);
  assert.match(verifier, /expectedVisualGuide/);
  assert.match(verifier, /VISUAL_SCREENSHOTS/);
  assert.match(verifier, /top\.includes\(SESSION_GUIDE_NAME\)/);
  assert.match(verifier, /top\.includes\(VISUAL_GUIDE_NAME\)/);
  assert.match(verifier, /assertRegularExactFile\(join\(extraction, GUIDE_NAME\), expectedGuide/);
  assert.match(verifier, /assertUniqueArchiveEntries\(entries, 'the signed alpha archive'\)/);
});

test('signed and ad-hoc user guidance remain explicitly separate', () => {
  assert.match(readme, /does not claim that such an artifact exists/);
  assert.match(readme, /signing private key and notary\s+credential remain in Keychain/);
  assert.match(guide, /signed with an Apple Developer ID, notarized by Apple,\s+stapled/);
  assert.match(guide, /Open Mesh\.app normally/);
  assert.match(guide, /explicit macOS user-presence prompt/);
  assert.match(guide, /stable working-folder shortcut follows the point opened in Mesh/);
  assert.match(guide, /Existing agents\s+deliberately remain pinned/);
  assert.match(guide, /do not disable Gatekeeper or remove quarantine metadata/);
  assert.doesNotMatch(guide, /control-click/);
  assert.match(sessionGuide, /MESH ALPHA — YOUR FIRST 20 MINUTES/);
  assert.match(sessionGuide, /VISUAL-GUIDE\.html/);
  assert.match(visualGuide, /Visual first session/);
});
