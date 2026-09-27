import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const html = readFileSync(new URL('index.html', import.meta.url), 'utf8');
const script = readFileSync(new URL('app.js', import.meta.url), 'utf8');
const rendererProof = readFileSync(new URL('renderer-proof.js', import.meta.url), 'utf8');
const styles = readFileSync(new URL('styles.css', import.meta.url), 'utf8');
const nextStyles = readFileSync(new URL('../ui-next/src/styles.css', import.meta.url), 'utf8');
const buttonAtom = readFileSync(new URL('../ui-next/src/atoms/button.tsx', import.meta.url), 'utf8');
const nativeHost = readFileSync(new URL('../src-tauri/main.rs', import.meta.url), 'utf8');
const daemonLive = readFileSync(new URL('../../../crates/mesh-daemon/src/live.rs', import.meta.url), 'utf8');
const codexWorkspace = readFileSync(new URL('../src-tauri/codex_workspace.rs', import.meta.url), 'utf8');
const tauriConfig = readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8');
const demo = readFileSync(new URL('../../../docs/demo.md', import.meta.url), 'utf8');
const desktopReadme = readFileSync(new URL('../README.md', import.meta.url), 'utf8');
const rootReadme = readFileSync(new URL('../../../README.md', import.meta.url), 'utf8');
const alphaStart = readFileSync(new URL('../ALPHA-START-HERE.txt', import.meta.url), 'utf8');
const signedAlphaStart = readFileSync(new URL('../SIGNED-ALPHA-START-HERE.txt', import.meta.url), 'utf8');
const productionNavigation = readFileSync(new URL('../ui-next/src/organisms/production-navigation.tsx', import.meta.url), 'utf8');
const productionRoute = readFileSync(new URL('../ui-next/src/models/production-route.ts', import.meta.url), 'utf8');
const workspaceChrome = readFileSync(new URL('../ui-next/src/organisms/workspace-chrome.tsx', import.meta.url), 'utf8');
const productionLayout = readFileSync(new URL('../ui-next/src/layouts/production-workspace-layout.tsx', import.meta.url), 'utf8');
const productionPage = readFileSync(new URL('../ui-next/src/pages/production-workspace-page.tsx', import.meta.url), 'utf8');
const workspaceEntry = readFileSync(new URL('../ui-next/src/organisms/workspace-entry.tsx', import.meta.url), 'utf8');
const importWorkbench = readFileSync(new URL('../ui-next/src/organisms/import-workbench.tsx', import.meta.url), 'utf8');
const recentWorkspacePicker = readFileSync(new URL('../ui-next/src/molecules/recent-workspace-picker.tsx', import.meta.url), 'utf8');
const workspaceRestore = readFileSync(new URL('../ui-next/src/organisms/workspace-restore.tsx', import.meta.url), 'utf8');
const workspaceVersions = readFileSync(new URL('../ui-next/src/organisms/workspace-version-navigator.tsx', import.meta.url), 'utf8');
const artifactReview = readFileSync(new URL('../ui-next/src/organisms/artifact-review.tsx', import.meta.url), 'utf8');
const liveAgentReview = readFileSync(new URL('../ui-next/src/organisms/live-agent-review.tsx', import.meta.url), 'utf8');
const workspaceFilesChanges = readFileSync(new URL('../ui-next/src/organisms/workspace-files-changes.tsx', import.meta.url), 'utf8');
const workspaceCurrent = readFileSync(new URL('../ui-next/src/organisms/workspace-current.tsx', import.meta.url), 'utf8');
const workspaceDestination = readFileSync(new URL('../ui-next/src/organisms/workspace-destination.tsx', import.meta.url), 'utf8');

const removedCurrentControllerIds = Object.freeze([
  'workspace-current-card', 'workspace-current-current', 'reveal-workspace', 'switch-version',
  'open-codex', 'start-isolated-agent', 'update-original', 'finish-agent', 'refresh',
  'workspace-more-actions', 'return-workspace', 'open-agent-terminal', 'copy-diagnostics',
  'empty-workspace', 'workspace-panel', 'state-word', 'record-count', 'working-folder-block',
  'active-folder', 'copy-working-path', 'agent-folder-details', 'agent-path-label',
  'workspace-root', 'copy-agent-path', 'native-folder-hint', 'private-version',
  'shared-version', 'entry-count', 'entries', 'conditions', 'rollback',
]);
const removedImportControllerIds = Object.freeze([
  'import-eyebrow', 'import-title', 'import-next-toggle', 'step-badge', 'empty-import',
  'source-path-input', 'preview-source-path', 'preview-panel', 'source-path', 'file-count',
  'dir-count', 'byte-count', 'import-scope-hint', 'import-file-preview',
  'import-file-preview-summary', 'import-file-list', 'summary-digest', 'destination',
  'choose-destination', 'confirm-import',
]);

function relativeLuminance(hex) {
  const channels = hex.slice(1).match(/../g).map((pair) => Number.parseInt(pair, 16) / 255);
  const [red, green, blue] = channels.map((value) => (
    value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
  ));
  return (0.2126 * red) + (0.7152 * green) + (0.0722 * blue);
}

function contrastRatio(first, second) {
  const high = Math.max(relativeLuminance(first), relativeLuminance(second));
  const low = Math.min(relativeLuminance(first), relativeLuminance(second));
  return (high + 0.05) / (low + 0.05);
}

function nativeCommandSource(command) {
  const start = nativeHost.indexOf(`fn ${command}(`);
  assert.notEqual(start, -1, `missing native command ${command}`);
  const next = nativeHost.indexOf('\n    #[tauri::command', start + 1);
  return nativeHost.slice(start, next === -1 ? nativeHost.length : next);
}

test('filesystem paths retain their exact user-selected names', () => {
  for (const control of [
    'manage-path',
    'move-path',
    'version-destination',
  ]) {
    assert.doesNotMatch(
      script,
      new RegExp(`\\$\\('${control}'\\)\\.value\\.trim\\(\\)`),
      `${control} can redirect a valid trailing-space name to a different filesystem object`,
    );
  }
  assert.doesNotMatch(script, /String\(source \?\? ''\)\.trim\(\)/);
  assert.doesNotMatch(script, /String\(path \?\? ''\)\.trim\(\)/);
  assert.match(script, /String\(path \?\? ''\)\.split\('\/'\)/);
  assert.equal(script.includes('split(/[\\\\/]/)'), false);
  assert.equal(script.includes('replace(/[\\\\/]$/'), false);
  assert.equal(script.includes("part.replace(/\\.mesh$/i, '')"), false);
});

test('every managed mutation enters the process-shared agent custody transaction', () => {
  for (const command of [
    'open_current_review',
    'approve_current_review',
    'rollback_managed_workspace',
    'preserve_managed_text',
    'save_managed_private',
    'adopt_native_file',
    'adopt_native_directory',
    'create_managed_text',
    'create_managed_folder',
    'move_managed_entry',
    'delete_managed_entry',
    'adopt_native_file_deletion',
    'adopt_native_file_move',
    'restore_managed_version',
  ]) {
    assert.match(
      nativeCommandSource(command),
      /with_unassigned_managed_workspace\(/,
      `${command} can race agent custody`,
    );
  }
  assert.match(nativeHost, /matches!\(method, "workspace\.state" \| "folder\.import\.preview"\)/);
});

test('approval availability validates the running Apple identity before offering enrollment', () => {
  const status = nativeCommandSource('approval_credential_status');
  const availability = status.indexOf('SecureEnclaveApprovalCredential::availability()');
  const credentialLoad = status.indexOf('SecureEnclaveApprovalCredential::load()');
  assert.notEqual(availability, -1, 'approval status omitted the read-only identity preflight');
  assert.notEqual(credentialLoad, -1, 'approval status omitted the enrolled credential lookup');
  assert.ok(
    availability < credentialLoad,
    'a missing credential can be mistaken for availability before app identity is checked',
  );
  assert.match(status, /approval_status_json\(None, Some\(&error\.to_string\(\)\)\)/);
});

test('desktop UI exposes the complete local-folder management journey', () => {
  for (const id of ['import-workbench-next', 'workspace-current-next', 'workspace-files-next', 'workspace-changes-next', 'workspace-destination-next']) {
    assert.match(html, new RegExp(`id="${id}"`));
  }
  for (const removedId of [...removedCurrentControllerIds, ...removedImportControllerIds, 'workspace-entry-current', 'hero-eyebrow', 'hero-title', 'hero-lede', 'workspace-entry-controls', 'workspace-entry-summary', 'choose-source', 'open-managed', 'open-managed-path', 'open-managed-path-button', 'recent-workspace', 'recent-workspace-hint', 'open-recent-workspace', 'forget-recent-workspace', 'restore-card', 'restore-file', 'restore-target', 'restore-preview', 'restore-apply', 'restore-undo', 'restore-hint', 'restore-output', 'manage-card', 'workspace-files-current', 'manage-path', 'create-text-entry', 'create-folder-entry', 'manage-entry', 'move-path', 'move-entry', 'delete-entry', 'management-status', 'edit-file', 'load-file', 'file-editor', 'save-file', 'save-private', 'edit-state', 'edit-version', 'update-destination-eyebrow', 'update-destination-title', 'export-target', 'export-hint', 'export-output', 'export-card', 'workspace-destination-current', 'export-file', 'choose-export-target', 'export-preview', 'export-confirm', 'export-preview-all', 'export-confirm-all']) {
    assert.doesNotMatch(html, new RegExp(`id="${removedId}"`));
    assert.doesNotMatch(script, new RegExp(`\\$\\('${removedId}'\\)`));
  }
  assert.match(script, /let workspaceEntryManagedPathDraft = ''/);
  assert.match(script, /let selectedRecentWorkspacePath = ''/);
  assert.match(script, /async function chooseManagedWorkspaceFromEntry\(/);
  assert.match(script, /async function openManagedWorkspaceFromEntry\(/);
  assert.match(script, /async function chooseSourceFolder\(/);
  assert.match(script, /async function seedCrossIslandImportInteraction\(/);
  assert.match(script, /mesh:import-workbench-external-focus/);
  assert.match(script, /let workspaceDestinationDraft = ''/);
  assert.match(script, /let workspaceDestinationHint = /);
  assert.match(script, /let workspaceDestinationPlanText = ''/);
  assert.match(importWorkbench, /data-mesh-proof="import-path"/);
  assert.match(importWorkbench, /data-mesh-proof="import-verified-preview"/);
  assert.match(importWorkbench, /onIntent\(\{ type: "confirm-import" \}\)/);
  assert.match(html, /id="workspace-restore-next" class="hidden"/);
  assert.match(script, /function renderRestoreNext\(\)/);
  assert.match(script, /projectionHasVisibleContinuity\(\s*workspaceRestoreNextPending,\s*workspaceRestoreNextMounted,?\s*\)/);
  assert.match(script, /let selectedRestoreFileId = ''/);
  assert.match(script, /let selectedRestoreVersionId = ''/);
  assert.match(script, /function selectRestoreFile\(objectId\)/);
  assert.match(script, /function selectRestoreVersion\(versionId\)/);
  assert.match(script, /actions\.set\(`select-file:\$\{candidate\.object_id\}`/);
  assert.match(script, /actions\.has\('apply'\)/);
  for (const method of ['folder.import.preview', 'workspace.open', 'workspace.state']) {
    assert.match(script, new RegExp(method.replaceAll('.', '\\.')));
  }
  assert.match(script, /rollback_managed_workspace/);
  assert.match(nativeHost, /rollback_managed_workspace/);
  assert.match(script, /import_managed_workspace/);
  assert.match(script, /finish_managed_workspace_agent_handoff/);
  assert.match(nativeHost, /finish_managed_workspace_agent_handoff/);
  assert.match(nativeHost, /"folder\.import\.confirm"/);
  assert.match(script, /preview_managed_restore/);
  for (const command of ['discover_retired_exports', 'preview_retired_export', 'remove_retired_export']) {
    assert.match(script, new RegExp(command));
    assert.match(nativeHost, new RegExp(command));
  }
  assert.match(script, /review_items/);
  assert.match(script, /open_current_review/);
  assert.match(artifactReview, /Understand exactly what changed/);
  assert.doesNotMatch(desktopReadme, /still lacks the final review form/);
  assert.match(desktopReadme, /Secure Enclave credential plus explicit user presence/);
  assert.doesNotMatch(demo, /Switch and open working folder|Switch and open in Codex/);
  assert.match(demo, /Open in working folder[\s\S]*Start Codex on this point/);
  for (const guide of [desktopReadme, demo, alphaStart, signedAlphaStart]) {
    assert.match(guide, /Start Codex on this (?:version|point)/);
    assert.match(guide, /Start another agent copy/);
    assert.doesNotMatch(guide, /\bOpen in Codex\b|\bStart another agent\b(?! copy)/);
  }
  assert.match(artifactReview, /agents and web content cannot skip the native user-presence approval ceremony/i);
  assert.match(artifactReview, /Approve exact version/);
  assert.doesNotMatch(script, /Select this version to approve/);
  assert.match(desktopReadme, /Review this saved point again/);
  assert.doesNotMatch(desktopReadme, /Select this version to approve/);
  assert.match(demo, /Developer-ID-signed build[\s\S]*approving that exact saved version[\s\S]*Update original folder/);
  assert.match(demo, /ad-hoc local\/archive build[\s\S]*Approval unavailable/);
  assert.match(alphaStart, /because approval is unavailable in this ad-hoc build[\s\S]*Choose export folder[\s\S]*different ordinary folder/i);
  assert.match(alphaStart, /Choose export folder[\s\S]*never select the untouched original project/i);
  assert.match(alphaStart, /supported way to copy reviewed alpha work out without claiming approval or changing the original/i);
  assert.match(rootReadme, /recording[\s\S]*approving[\s\S]*returned to the\s+original project/);
  assert.match(nativeHost, /\.title\("Approve to shared version"\)/);
  assert.doesNotMatch(nativeHost, /\.title\("Approve shared version"\)/);
  assert.match(artifactReview, /Review bundle/);
  assert.doesNotMatch(styles, /\.review-item-current|\.review-item-earlier|\.review-diff|\.review-artifact/);
  assert.match(script, /approve_current_review/);
  assert.match(artifactReview, /Approve and create Git branch/);
  const privateExportAvailability = script.match(/function explicitPrivateExportAvailable\(\) \{[\s\S]*?\n\}/u)?.[0] || '';
  assert.match(privateExportAvailability, /model\.workspaceVerified/);
  assert.match(privateExportAvailability, /!managedWorkspaceMutationBlocked\(\)/);
  assert.doesNotMatch(privateExportAvailability, /model\.approval/);
  assert.match(script, /exportToGit/);
  assert.match(script, /Original working files were not changed/);
  assert.match(nativeHost, /SecureEnclaveApprovalCredential/);
  for (const exactField of [
    'Workspace identity',
    'Expected shared head',
    'Reviewed head',
    'Review bundle',
    'Selected changes',
    'Conflict resolutions',
    'Validation evidence',
    'Policy epoch',
    'Approving credential',
    'Credential public key',
    'Application',
    'User verification',
    'Ceremony challenge',
    'Canonical statement BLAKE3',
    'Presentation BLAKE3',
  ]) {
    assert.match(nativeHost, new RegExp(exactField));
  }
  assert.match(script, /canonical_receipt_blake3/);
  assert.match(script, /complete receipt for the exact version/);
  assert.match(nativeHost, /Mesh approval receipt/);
  assert.match(nativeHost, /will not omit approval details/);
  assert.match(script, /Build \$\{revision\.slice\(0, 12\)\}/);
  assert.match(script, /Build identity unavailable/);
  assert.match(script, /Working in \$\{currentName\}/);
  assert.match(script, /\$\{presentation\.currentName\} — Mesh/);
  assert.match(nativeHost, /build_revision/);
  assert.match(nativeHost, /MESH_BUILD_REVISION/);
  assert.doesNotMatch(html, /id="approve-review"/);
  assert.doesNotMatch(script, /review\.approve/);
  assert.match(workspaceDestination, /Every file is rechecked and installed atomically/);
  assert.match(workspaceDestination, /never rolls back an earlier completed prefix/);
  for (const stalePhrase of [
    'The export folder',
    'file export is previewed',
    'Nothing will be exported',
    'file export plan',
  ]) {
    assert.doesNotMatch(script, new RegExp(stalePhrase), `stale user-facing phrase: ${stalePhrase}`);
  }
});

test('the React Restore page has no hidden legacy controller', () => {
  for (const removedId of [
    'restore-card',
    'workspace-restore-current',
    'restore-file',
    'restore-target',
    'restore-preview',
    'restore-apply',
    'restore-undo',
    'restore-hint',
    'restore-output',
  ]) {
    assert.doesNotMatch(html, new RegExp(`id="${removedId}"`));
    assert.doesNotMatch(script, new RegExp(`\\$\\('${removedId}'\\)`));
  }
  assert.match(html, /id="workspace-restore-next"[^>]*slot="workspace-restore"/);
  assert.match(workspaceRestore, /Restore an earlier file version/);
});

test('the React Versions page has no hidden legacy controller', () => {
  for (const removedId of [
    'workspace-versions-card',
    'workspace-versions-current',
    'versions-next-toggle',
    'workspace-version',
    'fork-version',
    'fork-version-codex',
    'version-location-summary',
    'version-destination',
    'version-hint',
    'workspace-version-preview',
  ]) {
    assert.doesNotMatch(html, new RegExp(`id="${removedId}"`));
    assert.doesNotMatch(script, new RegExp(`\\$\\('${removedId}'\\)`));
  }
  assert.match(html, /id="workspace-versions-next"[^>]*slot="workspace-versions"/);
  assert.match(script, /let selectedWorkspaceVersionOperation = ''/);
  assert.match(script, /let workspaceVersionDestinationDraft = ''/);
  assert.match(script, /function selectWorkspaceVersion\(operation\)/);
  assert.match(script, /changeBasis: preview\?\.change_basis \|\| null/);
  assert.match(script, /basisOrdinal: preview\?\.basis_ordinal \?\? null/);
  assert.match(script, /async function openSelectedWorkspaceVersion\(operation\)/);
  assert.match(script, /async function startCodexFromSelectedWorkspaceVersion\(operation\)/);
  assert.match(workspaceVersions, /Open an exact saved point/);
});

test('the React Review page has no hidden legacy controller', () => {
  for (const removedId of [
    'review-card',
    'review-count',
    'review-next-toggle',
    'setup-approval',
    'open-current-review',
    'empty-reviews',
    'empty-reviews-message',
    'review-panel',
    'review-items',
    'review-overflow',
  ]) {
    assert.doesNotMatch(html, new RegExp(`id="${removedId}"`));
    assert.doesNotMatch(script, new RegExp(`\\$\\('${removedId}'\\)`));
  }
  assert.match(html, /id="review-workbench-next"[^>]*slot="review-workbench"/);
  assert.match(script, /async function setupApprovalCredential\(\)/);
  assert.match(script, /async function recordCurrentReview\(\)/);
});

test('the React Files page has no hidden legacy controller', () => {
  for (const removedId of [
    'manage-card',
    'workspace-files-current',
    'manage-path',
    'create-text-entry',
    'create-folder-entry',
    'manage-entry',
    'move-path',
    'move-entry',
    'delete-entry',
    'management-status',
  ]) {
    assert.doesNotMatch(html, new RegExp(`id="${removedId}"`));
    assert.doesNotMatch(script, new RegExp(`\\$\\('${removedId}'\\)`));
  }
  assert.match(html, /id="workspace-files-next"[^>]*slot="workspace-files"/);
  assert.match(script, /let workspaceFilesState = /);
  assert.match(script, /async function createManagedTextEntry\(/);
  assert.match(script, /async function createManagedFolderEntry\(/);
  assert.match(script, /async function moveManagedEntry\(/);
  assert.match(script, /async function deleteManagedEntry\(/);
});

test('the React Changes editor has no hidden legacy text controller', () => {
  for (const removedId of [
    'edit-file',
    'load-file',
    'save-file',
    'save-private',
    'file-editor',
    'edit-state',
    'edit-version',
  ]) {
    assert.doesNotMatch(html, new RegExp(`id="${removedId}"`));
    assert.doesNotMatch(script, new RegExp(`\\$\\('${removedId}'\\)`));
  }
  assert.match(html, /id="workspace-changes-next"[^>]*slot="workspace-changes"/);
  assert.match(script, /let workspaceChangesEditorState = /);
  assert.match(script, /async function inspectSelectedManagedFile\(/);
  assert.match(script, /async function preserveManagedEditorText\(/);
  assert.match(script, /async function saveInspectedFilePrivately\(/);
  assert.match(script, /baselineText: editorKind === 'text' \? workspaceEditorBaselineText\(model\.editor\) : ''/);
  assert.match(script, /baselineAvailable: editorKind === 'text' && workspaceEditorBaselineAvailable\(model\.editor\)/);
  assert.match(script, /const editorKind = !model\.editor[\s\S]*typeof model\.editor\.text === 'string'/);
  assert.match(nativeHost, /inspect_managed_file_with_durable_text\([\s\S]*&relative_path,[\s\S]*MAX_WORKSPACE_EDITOR_TEXT_BYTES/);
  assert.match(nativeHost, /"baseline_text"/);
  assert.match(nativeHost, /MAX_WORKSPACE_EDITOR_TEXT_BYTES: usize = 1_048_576/);
  assert.doesNotMatch(styles, /\.file-editor(?:\W|$)/);
  assert.doesNotMatch(styles, /\.edit-status(?:\W|$)/);
});

test('the React Changes queue has no hidden legacy scan or structural controller', () => {
  for (const removedId of [
    'scan-files',
    'folder-change-queue',
    'folder-change-count',
    'folder-change-items',
    'save-all-private',
    'native-structural-change',
    'native-missing-source',
    'native-move-target',
    'record-native-structural-change',
    'native-structural-hint',
  ]) {
    assert.doesNotMatch(html, new RegExp(`id="${removedId}"`));
    assert.doesNotMatch(script, new RegExp(`\\$\\('${removedId}'\\)`));
  }
  assert.match(script, /let workspaceChangesQueueState = /);
  assert.match(script, /async function requestNativeFolderScan\(/);
  assert.match(script, /async function recordSelectedStructuralChange\(/);
  assert.match(script, /async function saveAllPrivateChanges\(/);
  for (const removedId of ['auto-save-native', 'auto-save-native-hint', 'workspace-changes-current', 'editor-card']) {
    assert.doesNotMatch(html, new RegExp(`id="${removedId}"`));
    assert.doesNotMatch(script, new RegExp(`\\$\\('${removedId}'\\)`));
  }
  assert.match(script, /async function updateNativeCapturePreference\(requested\)/);
});

test('the empty state reveals one onboarding journey before workspace tools', () => {
  assert.doesNotMatch(html, /id="export-card"/);
  assert.match(html, /id="workspace-files-next" class="hidden"[^>]*slot="workspace-files"/);
  assert.match(script, /const WORKSPACE_REQUIRED_SECTIONS = \[/);
  assert.match(script, /\$\(id\)\.classList\.toggle\('hidden', !presentation\.workspaceReady\)/);
  assert.match(script, /renderWorkspaceDisclosure\(data\)/);
  assert.match(html, /id="workspace-entry-next" class="hidden"/);
  assert.match(script, /function renderWorkspaceEntryNext\(interactionGeneration = null, interactionKind = null\)/);
  assert.match(
    script,
    /const exactGeneration = detail\?\.generation === workspaceEntryNextPending\.generation[\s\S]*detail\.generation === workspaceEntryNextMounted\.generation/,
  );
  assert.match(
    script,
    /projectionHasVisibleContinuity\(\s*workspaceEntryNextPending,\s*workspaceEntryNextMounted,?\s*\)/,
  );
  assert.match(script, /actions\.set\(`select-recent:\$\{entry\.path\}`/);
  assert.match(script, /async function openSelectedRecentWorkspace\(/);
  assert.match(script, /async function forgetSelectedRecentWorkspace\(/);
  assert.match(workspaceEntry, /Or enter its path/);
  assert.match(script, /Your folder, with a private history\./);
});

test('a returning user lands on the native work loop before import', () => {
  assert.match(script, /Work in your folder\. Mesh remembers\./);
  assert.match(script, /workspaceEntryDisclosureOpen = presentation\.defaultDisclosureOpen/);
  assert.match(workspaceCurrent, /Current workspace/);
  assert.match(workspaceCurrent, /Working folder/);
  assert.match(workspaceCurrent, /Agent folder/);
  for (const removedId of ['next-action-card', 'next-action-title', 'next-action-description', 'next-action-button', 'next-version-button']) {
    assert.doesNotMatch(html, new RegExp(`id="${removedId}"`));
    assert.doesNotMatch(script, new RegExp(`\\$\\('${removedId}'\\)`));
  }
  assert.match(script, /function recommendedNextAction\(\)/);
  assert.match(script, /function activateProjectedRecommendation\(/);
  assert.match(script, /workspaceOverviewNextPending\?\.authority !== authority/);
  assert.match(script, /const canSwitchVersion = Boolean\(\s*workspace\?\.verified === true/);
  assert.match(script, /Continue in the native folder/);
  assert.match(script, /Start Codex on this saved version/);
  assert.match(script, /Agent folder is assigned/);
  assert.match(script, /Finish agent handoff/);
  assert.match(script, /if \(workspaceInstallationMatchesHandoff\(\)\) return activeAgentHandoffAction\(\)/);
  assert.match(script, /nativeScanBlocked[\s\S]*workspaceInstallationMatchesHandoff\(\)/);
  assert.match(script, /!agentFinished && refuseOrdinaryInspectionDuringAgentHandoff\(\{ automatic \}\)/);
  assert.match(script, /async function inspectSelectedManagedFile\(\)[\s\S]*refuseOrdinaryInspectionDuringAgentHandoff\(\)/);
  assert.match(script, /Ordinary inspection and changes are paused; Finish agent handoff performs the exact complete inspection/);
  assert.doesNotMatch(script, /File inspection stays available/);
  assert.match(script, /Review .* native/);
  assert.doesNotMatch(styles, /workspace-current-card/);
  assert.match(script, /const NATIVE_SCAN_INTERVAL_MS = 5_000/);
  assert.match(script, /appDocument\.visibilityState !== 'visible'/);
  assert.match(script, /model\.folderChanges\.length > 0/);
  assert.match(script, /periodic: true/);
  assert.match(demo, /review-first mode[\s\S]*read-only check every five seconds[\s\S]*while visible/);
  assert.match(demo, /automatic private save explicitly enabled[\s\S]*window is hidden/);
});

test('closing and reopening the macOS app restores its one workspace window', () => {
  assert.match(nativeHost, /WindowEvent::CloseRequested \{ api, \.\. \}/);
  assert.match(nativeHost, /api\.prevent_close\(\);[\s\S]*window\.hide\(\)/);
  assert.match(nativeHost, /RunEvent::Reopen \{ \.\. \}/);
  assert.match(nativeHost, /fn reveal_desktop_window[\s\S]*app\.show\(\)[\s\S]*get_webview_window\("main"\)[\s\S]*window\.show\(\)[\s\S]*window\.set_focus\(\)/);
  assert.match(nativeHost, /RunEvent::Reopen[\s\S]*reveal_desktop_window\(app_handle\)/);
  assert.doesNotMatch(nativeHost, /RunEvent::Reopen \{[\s\S]*has_visible_windows: false/);
  assert.match(nativeHost, /request_existing_desktop_attention\(&runtime_dir\)/);
  assert.match(nativeHost, /run_on_main_thread[\s\S]*reveal_desktop_window/);
  assert.match(nativeHost, /too old to bring its window forward automatically/);
  assert.match(desktopReadme, /Opening Mesh again[\s\S]*brings its existing window forward/);
  assert.match(desktopReadme, /newer build[\s\S]*older Mesh process[\s\S]*changes no workspace data/);
});

test('verified workspaces expose direct navigation to every alpha journey', () => {
  for (const [id, label] of [
    ['current', 'Current'], ['files', 'Files'], ['changes', 'Changes'], ['review', 'Review'],
    ['versions', 'Versions'], ['update', 'Update destination'], ['restore', 'Restore'],
  ]) {
    assert.match(productionRoute, new RegExp(`id: "${id}",[\\s\\S]*?label: "${label}"`));
  }
  assert.match(productionNavigation, /aria-label=\{t\("Primary pages"\)\}/);
  assert.match(productionNavigation, /nativeChangeCount > 0/);
});

test('the alpha journey has explicit keyboard, status, and non-text accessibility', () => {
  assert.match(
    html,
    /<div id="mesh-app-next" aria-label="Mesh">[\s\S]*?id="workspace-chrome-next"[^>]*slot="workspace-header"[\s\S]*?id="confirmation-dialog-next" slot="confirmation-dialog"[\s\S]*?<\/div>\s*<\/body>/,
  );
  assert.match(productionLayout, /href="#mesh-react-main"[\s\S]*Skip to workspace/);
  assert.match(workspaceChrome, /role="status"[^>]*aria-live="polite"[^>]*aria-atomic="true"/);
  assert.doesNotMatch(html, /id="notice"/);
  assert.doesNotMatch(styles, /\.notice(?:\s|\.|\{)/);
  assert.doesNotMatch(script, /\$\('notice'\)/);
  assert.doesNotMatch(rendererProof, /getElementById\('notice'\)/);
  assert.match(productionPage, /data-mesh-proof="production-notice"[\s\S]*role="status" aria-live="polite" aria-atomic="true"[\s\S]*role="alert" aria-live="assertive" aria-atomic="true"/);
  assert.match(productionPage, /data-mesh-agent-proof=\{notice\?\.proof \?\? undefined\}/);
  assert.match(workspaceCurrent, /aria-label=\{id === "refresh" \? "Refresh workspace state" : undefined\}/);
  assert.match(importWorkbench, /data-mesh-import-choose/);
  for (const removedId of ['workspace-chrome-current', 'service', 'build-identity']) {
    assert.doesNotMatch(html, new RegExp(`id="${removedId}"`));
    assert.doesNotMatch(script, new RegExp(`\\$\\('${removedId}'\\)`));
  }
  assert.match(productionLayout, /buildIdentity\.title/);
  assert.match(productionLayout, /buildIdentity\.label/);
  assert.match(productionNavigation, /aria-label=\{`\$\{nativeChangeCount\} folder changes`\}/);
  assert.match(recentWorkspacePicker, /id="workspace-entry-recent"/);
  assert.match(recentWorkspacePicker, /aria-describedby="workspace-entry-recent-hint"/);
  assert.match(buttonAtom, /min-h-11[\s\S]*focus-visible:ring-2 focus-visible:ring-ring/);
  assert.match(productionNavigation, /focus-visible:ring-2 focus-visible:ring-ring/);
  const variable = (name) => nextStyles.match(new RegExp(`--${name}:\\s*(#[0-9a-f]{6})`))[1];
  assert.ok(
    contrastRatio(variable('border'), variable('background')) >= 3,
    'input boundaries must meet non-text contrast',
  );
  assert.ok(
    contrastRatio(variable('border'), variable('secondary')) >= 3,
    'button boundaries must meet non-text contrast',
  );
  assert.match(workspaceFilesChanges, /<textarea[\s\S]*className="[^"]*border border-border[^"]*focus-visible:ring-2/);
});

test('the production shell has no legacy coordinator container or stylesheet surface', () => {
  assert.doesNotMatch(html, /id="main-content"|coordinator-only/);
  assert.doesNotMatch(script, /\$\('main-content'\)|getElementById\(['"]main-content['"]\)|classList\.(?:add|remove|toggle)\('workspace-open'/);
  for (const legacySelector of [
    'skip-link',
    'shell',
    'eyebrow',
    'lede',
    'field-row',
    'card-heading',
    'restore-form',
    'manage-actions',
    'primary',
    'secondary',
    'quiet',
    'danger',
    'card',
    'read-only',
    'recovery-only',
    'path-block',
    'summary',
    'field',
    'history-hint',
  ]) {
    assert.doesNotMatch(styles, new RegExp(`\\.${legacySelector}(?:\\W|$)`));
  }
  assert.match(styles, /body\s*\{[^}]*margin:0[^}]*min-height:100vh/s);
  assert.match(styles, /\.hidden\s*\{\s*display:none !important;\s*\}/);
});

test('opening the native working folder is bound to the exact verified workspace', () => {
  assert.match(script, /reveal_managed_workspace/);
  assert.match(script, /expectedWorkspaceRoot: binding\.root/);
  assert.match(script, /expectedWorkspaceDigest: binding\.digest/);
  assert.match(script, /expectedWorkspaceInstallation: binding\.installation/);
  assert.match(nativeHost, /verified_managed_workspace_path/);
  assert.match(nativeHost, /is_presented/);
  assert.match(nativeHost, /older workspace layout has no isolated native folder/);
  assert.match(nativeHost, /Command::new\(program\)[\s\S]*\.arg\(path\)[\s\S]*\.spawn\(\)/);
  assert.match(nativeHost, /child\.try_wait\(\)/);
  assert.match(nativeHost, /status\.success\(\)/);
  assert.match(nativeHost, /NATIVE_FOLDER_OPEN_TIMEOUT/);
  assert.match(nativeHost, /fn reconcile_managed_workspace_navigation\(/);
  assert.match(nativeHost, /reconcile_verified_native_folder/);
  assert.doesNotMatch(nativeHost, /Command::new\(["'](?:sh|bash|zsh)["']\)/);
  assert.match(workspaceCurrent, /action\("copy-working-path", "quiet"\)/);
  assert.match(script, /reconcile_managed_workspace_navigation/);
  assert.match(script, /appWindow\.navigator\.clipboard\.writeText\(workingPath\)/);
  assert.match(demo, /Open working folder[^.]*Copy working path/);
  assert.match(desktopReadme, /open or copy this stable path after re-verifying/);
});

test('opening Codex gives the agent a fixed real workspace folder', () => {
  assert.match(script, /open_managed_workspace_in_codex/);
  assert.match(script, /expectedWorkspaceRoot: binding\.root/);
  assert.match(script, /expectedWorkspaceDigest: binding\.digest/);
  assert.match(script, /expectedWorkspaceInstallation: binding\.installation/);
  assert.match(script, /agent has a fixed real folder/);
  assert.match(script, /Create folder \+ start Codex/);
  assert.match(script, /openWorkspaceVersionAsFolder\([\s\S]*reveal: false,[\s\S]*allowNativeChanges: false/);
  assert.match(nativeHost, /fn open_managed_workspace_in_codex\(/);
  assert.match(nativeHost, /verified_managed_workspace_path/);
  assert.match(nativeHost, /open_codex_after_optional_context\([\s\S]*verified\.path\(\)[\s\S]*open_codex_workspace/);
  assert.match(nativeHost, /"fixed_workspace_path"/);
  assert.doesNotMatch(nativeHost, /"pinned_to_version"/);
  assert.match(nativeHost, /ensure_codex_project_config/);
  assert.match(nativeHost, /open_codex_after_optional_context/);
  assert.match(nativeHost, /"unavailable"/);
  assert.match(nativeHost, /"mesh_context_warning"/);
  assert.match(nativeHost, /optional_mesh_context_never_blocks_a_healthy_codex_workspace/);
  assert.match(script, /Mesh preserved your Codex settings/);
  assert.match(script, /Codex remains usable in the independent folder/);
  assert.match(script, /codexContextMessage\(opened\)/);
  assert.match(script, /gitContextMessage\(opened\)/);
  assert.match(script, /Independent Git history and status are ready in this folder/);
  assert.match(script, /Approval is ready; inspect the recorded change summary/);
  assert.match(script, /Set up approvals before this version can be shared/);
  assert.match(nativeHost, /run_mesh_mcp_if_requested/);
  assert.match(script, /launch-scoped read-only Mesh context is ready for this agent session/);
  assert.match(codexWorkspace, /\[mcp_servers\.mesh\]/);
  assert.match(codexWorkspace, /--expected-workspace-root/);
  const digestBindingOption = new RegExp(['--expected', 'workspace-digest'].join('-'));
  assert.doesNotMatch(codexWorkspace, digestBindingOption);
  assert.match(codexWorkspace, /--expected-workspace-installation/);
  assert.match(codexWorkspace, /ExistingConfig/);
  assert.match(nativeHost, /Command::new\(program\)[\s\S]*command\.arg\("app"\)[\s\S]*command\.arg\("-c"\)\.arg\(value\)[\s\S]*\.arg\(path\)[\s\S]*\.spawn\(\)/);
  assert.match(nativeHost, /com\.openai\.codex/);
  assert.doesNotMatch(nativeHost, /Command::new\(["'](?:sh|bash|zsh)["']\)/);
  assert.match(script, /folder remains writable but its optional metadata tool pauses/);
  assert.match(workspaceCurrent, /action\("copy-agent-path", "quiet"\)/);
  assert.match(workspaceCurrent, /action\("start-agent-copy", "secondary"\)/);
  assert.match(workspaceCurrent, /model\.agentFolderLabel/);
  assert.match(workspaceFilesChanges, /starts at the selected durable version and then remains writable/);
  assert.match(script, /invoke\('prepare_managed_workspace_agent_path'/);
  assert.match(script, /appWindow\.navigator\.clipboard\.writeText\(prepared\.path\)/);
  assert.match(script, /const handoffGeneration = assertAgentLaunchResponse\(opened, binding, 'Codex'\);[\s\S]*rememberAgentHandoff\(binding, handoffGeneration\)/);
  assert.match(script, /Agent folder assigned/);
  assert.match(script, /workspaceInstallationMatchesHandoff\(\)[\s\S]*already assigned to an agent/);
  assert.match(nativeHost, /fn prepare_managed_workspace_agent_path\(/);
  assert.match(nativeHost, /persist_agent_handoff\([\s\S]*expected_workspace_installation/);
  assert.match(desktopReadme, /Copy agent path[^.]*handoff is remembered/);
  assert.match(desktopReadme, /records the assignment before exposing its path/);
  assert.match(nativeHost, /"native_folder_path"/);
  assert.match(script, /Create a native working folder before handing this workspace to an agent/);
  assert.match(script, /freshCopy: true/);
  assert.match(script, /blockedAction: 'starting another isolated agent'/);
  assert.match(script, /Mesh reused an existing agent folder instead of creating a fresh isolated copy/);
  assert.match(script, /Do not share one writable folder between agents/);
  assert.match(desktopReadme, /One real folder is\s+for one long-running agent/);
  assert.match(demo, /Do not paste one copied agent path into two simultaneous agents/);
  assert.doesNotMatch(html, /Codex on the stable native folder/);
});

test('opening an agent terminal gives it a stable physical workspace reference', () => {
  assert.match(script, /open_managed_workspace_in_terminal/);
  assert.match(script, /Create folder \+ open terminal/);
  assert.match(script, /Start your local agent there/);
  assert.match(nativeHost, /fn open_managed_workspace_in_terminal\(/);
  assert.match(nativeHost, /open_terminal_workspace\(&agent_reference\)/);
  assert.match(nativeHost, /com\.apple\.Terminal/);
  assert.match(nativeHost, /ensure_current\(\)[\s\S]*open_terminal_workspace[\s\S]*ensure_current\(\)/);
  assert.match(desktopReadme, /Open agent terminal/);
  assert.doesNotMatch(nativeHost, /Command::new\(["'](?:sh|bash|zsh)["']\)/);
});

test('the React Current organism keeps the complete alpha journey visible and groups transitions', () => {
  assert.doesNotMatch(styles, /import-preview-active|\.grid > \.journey/);
  assert.match(script, /workspaceReady \? 'Import another folder' : 'Choose a folder'/);
  assert.match(workspaceCurrent, /action\("open-folder", "primary"\)/);
  assert.match(workspaceCurrent, /action\("start-codex", "primary"\)/);
  assert.match(workspaceCurrent, /action\("copy-diagnostics", "quiet"\)/);
  assert.match(workspaceCurrent, /action\("rollback", "danger"\)/);
  assert.doesNotMatch(html, /workspace-current-card|workspace-more-actions/);
  assert.match(tauriConfig, /"minWidth": 920/);
});

test('Current and Overview project one frozen coordinator-owned presentation', () => {
  assert.match(script, /function currentWorkspaceState\(\)/);
  assert.match(script, /function currentWorkspaceActionPresentation\(\)/);
  assert.match(script, /function currentWorkspacePresentation\(\)/);
  assert.match(script, /const current = currentWorkspacePresentation\(\);[\s\S]*state: current\.state,[\s\S]*recordSummary: current\.recordSummary/);
  assert.match(script, /mesh:workspace-current-projection[\s\S]*current,\s*\}\),/);
  assert.doesNotMatch(script, /\$\('state-word'\)/);
  assert.doesNotMatch(script, /const control = \$\(controlId\)[\s\S]{0,240}control\.textContent/);
});

test('Current and Overview actions call exact coordinator functions without hidden click replay', () => {
  for (const name of [
    'activateCurrentRefresh',
    'activateCurrentOpenFolder',
    'activateCurrentOpenVersion',
    'activateCurrentStartCodex',
    'activateCurrentStartAgentCopy',
    'activateCurrentUpdateDestination',
    'activateCurrentFinishAgent',
    'activateCurrentReturnWorkspace',
    'activateCurrentOpenTerminal',
    'activateCurrentCopyDiagnostics',
    'activateCurrentCopyWorkingPath',
    'activateCurrentCopyAgentPath',
    'activateCurrentRollback',
  ]) {
    assert.match(script, new RegExp(`(?:async )?function ${name}\\(`));
  }
  assert.match(script, /function currentWorkspaceActionAuthority\(surface, generation\)/);
  assert.match(script, /sequence: workspaceVerificationSequence/);
  assert.match(script, /custodyGeneration: workspaceInstallationMatchesHandoff\(\)/);
  assert.match(script, /activateProjectedCurrentAction\(currentActionAuthority, 'open-folder'\)/);
  assert.match(script, /activateProjectedCurrentAction\(authority\.currentActionAuthority, currentAction\)/);
  assert.doesNotMatch(script, /WORKSPACE_CURRENT_CONTROLS|currentWorkspaceActionIdForControl|controlId:/);
  assert.doesNotMatch(script, /actionHandlers\.set\([^\n]+\$\(controlId\)\.click/);
});

test('current-workspace shortcuts expose version choice and truthful destination copy', () => {
  assert.match(script, /id: 'open-version', label: 'Open saved version'/);
  assert.match(script, /id: 'start-codex'/);
  assert.match(script, /id: 'start-agent-copy'/);
  assert.match(script, /title: 'Start Codex on this saved version'/);
  assert.match(script, /Mesh assigns this fixed real folder to one Codex session before opening it/);
  assert.match(script, /agent stays pinned here if you switch the stable working folder/);
  assert.match(script, /Reopen assigned Codex folder/);
  assert.match(script, /Reopen assigned Codex folder\?/);
  assert.match(script, /async function activateCurrentStartCodex\(\)[\s\S]*await confirmAssignedAgentFolderReopen\('Codex', reopenAttempt\)/);
  assert.match(script, /async function activateCurrentOpenTerminal\(\)[\s\S]*await confirmAssignedAgentFolderReopen\('Terminal', reopenAttempt\)/);
  assert.match(script, /title: codex[\s\S]*Reopen this assigned agent terminal\?/);
  assert.doesNotMatch(script, /Reopen in Codex\?/);
  assert.match(script, /Choose another agent version/);
  assert.match(script, /id: 'update-destination'/);
  assert.match(script, /function workspaceUpdateCopy\(\)/);
  assert.match(script, /workspaceProjectRoot\(\) === destination[\s\S]*'Update original folder'[\s\S]*'Update destination folder'/);
  assert.match(script, /workspaceProjectRoot\(\)[\s\S]*\? 'Update original folder'[\s\S]*: updateCopy\.button/);
  assert.match(script, /model\.exportRoot = projectRoot[\s\S]*workspaceDestinationDraft = projectRoot/);
  assert.match(productionRoute, /id: "update",[\s\S]*?label: "Update destination"/);
  assert.match(workspaceDestination, /confirm-batch/);
  assert.match(script, /WORKSPACE_VERSION_FOCUS_SELECTOR = '\[role="radio"\]\[tabindex="0"\]:not\(:disabled\)'/);
  assert.match(script, /function focusWorkspaceVersionJourney\(operation = null\)[\s\S]*?data-mesh-version-operation[\s\S]*?WORKSPACE_VERSION_FOCUS_SELECTOR/);
  assert.match(script, /async function focusWorkspaceVersionChoice\(\{ preferEarlier = false, forAgent = false \} = \{\}\)/);
  assert.match(script, /open-another-version[\s\S]*focusWorkspaceVersionChoice\(\{ preferEarlier: true \}\)/);
  assert.match(script, /activateCurrentStartAgentCopy\(\)[\s\S]*focusWorkspaceVersionChoice\(\{ forAgent: true \}\)/);
  assert.match(script, /freshCopy: freshAgent[\s\S]*allowNativeChanges: !freshAgent[\s\S]*allowUnverified: !freshAgent/);
  assert.match(script, /const nearestEarlier = \[\.\.\.versions\][\s\S]*\.find\(\(version\) => version\.operation !== currentOperation\)/);
  assert.match(script, /Review the verified saved point below, then open it as a working folder or directly in Codex/);
  assert.match(script, /focusReactWorkspacePage\('update', '\[data-mesh-proof="destination-draft"\]'\)/);
  assert.match(script, /function previewPendingOriginalPullBack\(\)[\s\S]*pendingOriginalPullBack\(\)[\s\S]*workspaceDestinationDraft !== expectedTargetRoot[\s\S]*previewAllManagedExports\(\{ verifyNativeWork: true, expectedTargetRoot \}\)/);
  assert.match(script, /Type or paste the original or destination folder you want to update, then press Return/);
  assert.match(demo, /Create workspace and open folder/);
  assert.match(demo, /Open in working folder/);
  assert.match(demo, /scans the complete new folder tree/);
  assert.match(demo, /Save all privately/);
  assert.doesNotMatch(demo, /newly reachable folder\s+frontier/);
  assert.match(workspaceDestination, /Update destination[\s\S]*Update saved work in an ordinary folder/);
  assert.doesNotMatch(demo, /Create private workspace|Open selected version/);
});

test('whole-workspace destination updates use a generation-bound accessible confirmation', () => {
  assert.match(html, /id="confirmation-dialog-next"/);
  assert.match(script, /mesh:confirmation-available/);
  assert.match(script, /mesh:confirmation-mounted/);
  assert.match(script, /mesh:confirmation-intent/);
  assert.match(script, /detail\?\.generation !== confirmationNextPending\.generation/);
  assert.match(script, /Object\.keys\(intent\)\.length !== 1/);
  assert.match(script, /function exportBatchStillCurrent\(batch, sequence\)/);
  assert.match(script, /model\.exportBatchPreview !== batch/);
  assert.match(script, /exportPreviewSequence !== sequence/);
  assert.match(script, /await confirmWholeWorkspaceExport\(\{/);
  assert.match(script, /if \(!isCurrent\(\)\) \{/);
  assert.match(script, /fallbackConfirmationNext/);
});

test('managed-copy rollback uses destructive exact-workspace confirmation authority', () => {
  assert.match(script, /function rollbackWorkspaceStillCurrent\(binding\)/);
  assert.match(script, /assertVerifiedWorkspace\(binding\)/);
  assert.match(script, /model\.folderChanges\.length === 0/);
  assert.match(script, /title: 'Remove this managed workspace\?'/);
  assert.match(script, /confirmLabel: 'Roll back managed copy'/);
  assert.match(script, /cancelLabel: 'Keep workspace'/);
  assert.match(script, /tone: 'destructive'/);
  assert.match(script, /const destination = rollbackBinding\.root/);
  assert.doesNotMatch(script, /confirm\('Remove this unchanged managed copy/);
});

test('the primary import and version actions lead directly to native folders', () => {
  assert.match(importWorkbench, /\{t\(model\.confirmLabel\)\}/);
  assert.match(script, /Create workspace and open folder/);
  assert.match(script, /Counts cover the files Mesh will bring into the native working folder/);
  assert.match(script, /\.gitignore/);
  assert.match(script, /\.meshignore/);
  assert.match(script, /source \.git directory is never copied as workspace content/);
  assert.match(script, /separately recreates independently owned history, branch, and index state/);
  assert.match(script, /Excluded paths stay only in the original folder/);
  assert.match(script, /openLabel: 'Open in working folder'/);
  assert.match(script, /codexLabel: pendingAgentVersionChoice \? 'Start another agent from this point' : 'Start Codex on this point'/);
  assert.match(workspaceVersions, /\{t\(model\.openLabel\)\}/);
  assert.match(workspaceVersions, /\{t\(model\.codexLabel\)\}/);
  assert.match(workspaceVersions, /The workspace you leave stays untouched/);
  assert.match(script, /The stable working path now opens this version/);
  assert.match(workspaceVersions, /Open an exact saved point/);
  assert.ok(workspaceCurrent.indexOf('id="working-folder-heading"') < workspaceCurrent.indexOf('id="agent-folder-heading"'));
  assert.match(script, /Long-running agents remain pinned to their real folder/);
  assert.doesNotMatch(html, /Before switching, Mesh protects text still open in its editor and checks for unsaved native regular-file changes/);
  assert.match(script, /import_managed_workspace[\s\S]*installNavigationStatus\(answer\.navigation, model\.workspace\.root\)[\s\S]*revealCurrentWorkspaceFolder\(\)/);
  assert.match(script, /Mesh manages its private location unless you choose a custom one/);
  assert.match(workspaceVersions, /Custom private location/);
  assert.match(workspaceVersions, /Leave blank for Mesh-managed storage/);
});

test('a verified zero-history folder can enter the protected import preview without reselection', () => {
  assert.match(script, /title: 'Preserve this folder'/);
  assert.match(script, /label: 'Preview this folder'/);
  assert.match(script, /run: previewCurrentWorkspaceImport/);
  assert.match(script, /requiredSourceScope: 'open-zero-history-workspace'/);
});

test('native choosers are primary while typed paths retain daemon verification', () => {
  assert.match(importWorkbench, /Absolute folder path/);
  assert.match(importWorkbench, /data-mesh-import-choose/);
  assert.match(desktopReadme, /Native chooser controls remain the primary journey/);
  assert.match(desktopReadme, /visible typed paths are the\s+fallback when a system dialog is unavailable/);
  assert.doesNotMatch(desktopReadme, /native-picker controls hidden|keeps\s+the native-picker controls hidden/);
  assert.match(importWorkbench, /data-mesh-proof="import-destination"/);
  assert.doesNotMatch(importWorkbench, /data-mesh-proof="import-destination"[^>]*readOnly/);
  assert.match(workspaceDestination, /data-mesh-proof="destination-choose"/);
  assert.doesNotMatch(script, /configureNativeFolderPickers/);
  assert.match(nativeHost, /async fn pick_folder\([\s\S]*app: tauri::AppHandle,[\s\S]*runtime: State<'_, DesktopRuntime>/);
  assert.match(nativeHost, /take_private_export_picker_destination\(\)/);
  assert.match(nativeHost, /blocking_pick_folder\(\)/);
  assert.match(script, /async function previewSource\([\s\S]*requiredSourceScope/);
  assert.match(script, /folder\.import\.preview', \{ source: path \}/);
  assert.match(script, /async function openManagedWorkspace\(\s*path,\s*selection,/);
  assert.match(script, /workspace\.open', \{ path: root \}/);
  assert.match(script, /workspaceSelectionSequence \+= 1/);
});

test('destination actions are source-owned and do not proxy through hidden legacy controls', () => {
  assert.match(script, /function workspaceDestinationActionPresentation\(\)/);
  assert.match(script, /function selectWorkspaceDestinationFile\(value, interactionGeneration = null\)/);
  assert.match(script, /async function previewSingleManagedExport\(\{/);
  assert.match(script, /async function confirmSingleManagedExport\(/);
  assert.match(script, /async function confirmBatchManagedExport\(/);
  assert.doesNotMatch(script, /actionControls/);
  assert.doesNotMatch(script, /control\.click\(\)/);
  for (const id of ['export-card', 'workspace-destination-current', 'choose-export-target', 'export-file', 'export-preview', 'export-confirm', 'export-preview-all', 'export-confirm-all']) {
    assert.doesNotMatch(html, new RegExp(`id="${id}"`));
    assert.doesNotMatch(script, new RegExp(`\\$\\('${id}'\\)`));
  }
});

test('native workspace transitions never block the application event thread', () => {
  for (const command of [
    'open_current_review',
    'approval_credential_status',
    'enroll_approval_credential',
    'approve_current_review',
    'export_shared_review_to_git',
    'inspect_shared_review_git_export',
    'remember_managed_workspace',
    'forget_managed_workspace',
    'reveal_managed_workspace',
    'reconcile_managed_workspace_navigation',
    'reconcile_current_workspace_navigation',
    'open_managed_workspace_in_codex',
    'open_managed_workspace_in_terminal',
    'preview_managed_workspace_version',
    'open_managed_workspace_version',
    'open_fresh_agent_workspace',
    'import_managed_workspace',
    'daemon_call',
    'inspect_managed_file',
    'inspect_native_file',
    'inspect_managed_directory_installation',
    'discover_native_missing_files',
    'discover_native_directories',
    'preserve_managed_text',
    'save_managed_private',
    'adopt_native_file',
    'adopt_native_directory',
    'discover_retired_exports',
    'preview_retired_export',
    'remove_retired_export',
    'preview_managed_exports',
    'preview_managed_export',
    'preview_managed_directory_export',
    'preview_managed_directory_exports',
    'export_managed_directory',
    'export_managed_directories',
    'export_managed_file',
    'create_managed_text',
    'create_managed_folder',
    'move_managed_entry',
    'delete_managed_entry',
    'adopt_native_file_deletion',
    'adopt_native_file_move',
    'restore_managed_version',
    'preview_managed_restore',
    'managed_checkpoint_state',
  ]) {
    assert.match(
      nativeHost,
      new RegExp(`#\\[tauri::command\\(async\\)\\][\\s\\S]{0,320}?fn ${command}\\(`),
      `${command} must run outside the application event thread`,
    );
  }
});

test('the validated workspace survives a complete app restart', () => {
  for (const command of ['recent_workspace_status', 'remember_managed_workspace', 'forget_managed_workspace']) {
    assert.match(script, new RegExp(command));
  }
  assert.match(script, /refresh\(true\)/);
  assert.match(script, /last managed workspace reopened automatically from durable local history/);
  assert.match(script, /remembered for the next launch/);
  assert.match(script, /recent-workspace pointer was removed/);
  assert.match(script, /recent validated workspaces remain available across restarts/);
  assert.match(script, /function reconcileSelectedRecentWorkspace\(/);
  assert.match(script, /function previousWorkspaceActionPresentation\(/);
  assert.match(script, /paths\.find\(\(path\) => path !== current\)/);
  assert.match(script, /await openManagedWorkspace\(path, selection, \{/);
  assert.match(script, /Removed \$\{path\} from the recent-workspace list/);
  assert.match(workspaceVersions, /workspace you leave stays untouched/);
});

test('macOS cannot restore an AppKit crash window ahead of Mesh startup', () => {
  assert.match(nativeHost, /ApplePersistenceIgnoreState/);
  assert.match(
    nativeHost,
    /fn main\(\) \{\s*if let Some\(result\) = attachment_capture::run_if_requested\(\) \{\s*if let Err\(problem\) = result \{\s*eprintln!\("Mesh attachment: \{problem\}"\);\s*std::process::exit\(1\);\s*\}\s*return;\s*\}\s*if let Some\(result\) = run_mesh_mcp_if_requested\(\) \{\s*finish_mesh_mcp_mode\(result\);\s*return;\s*\}\s*ignore_appkit_persistent_window_state\(\);\s*desktop::run\(\);\s*\}/,
  );
});

test('file lifecycle controls use narrow authenticated native commands', () => {
  for (const command of ['create_managed_text', 'create_managed_folder', 'move_managed_entry', 'delete_managed_entry']) {
    assert.match(script, new RegExp(command));
  }
  assert.match(script, /author_authenticated/);
  assert.match(script, /signed change is durable in private history/);
  assert.match(script, /Create text file/);
  assert.match(script, /Move or rename/);
  assert.match(workspaceFilesChanges, /empty folders/);
  assert.match(workspaceFilesChanges, /immutable history/);
  assert.doesNotMatch(script, /workspace\.entry\.(create|move|delete)/);
  assert.match(script, /expectedWorkspaceInstallation: binding\.installation/);
  assert.match(script, /checkpoint\.workspace_installation !== workspace\.installation/);
  assert.match(
    script,
    /delete_managed_entry[\s\S]*expectedContentDigest: inspected\?\.content_digest \?\? null,[\s\S]*expectedExecutable: inspected\?\.executable \?\? null/,
    'delete must bind the complete portable state the person inspected',
  );
});

test('native managed-file reads require the verified workspace identity', () => {
  assert.match(nativeHost, /fn inspect_managed_file\([\s\S]*expected_workspace_root:[\s\S]*expected_workspace_digest:[\s\S]*expected_workspace_installation:/);
  assert.match(nativeHost, /fn inspect_managed_directory_installation\([\s\S]*expected_workspace_root:[\s\S]*expected_workspace_digest:[\s\S]*expected_workspace_installation:/);
  assert.doesNotMatch(
    nativeHost,
    /fn read_managed_text\(/,
    'an obsolete relative-path-only Tauri command can read from a replacement workspace',
  );
});

test('Files previews assigned text and JSON through exact read-only custody authority', () => {
  const filesInspection = script.slice(
    script.indexOf('async function inspectManagedWorkspaceEntryFromFiles'),
    script.indexOf('function updateEditorDraftPresentation'),
  );
  assert.match(filesInspection, /workspaceInstallationMatchesHandoff\(\)/);
  assert.match(filesInspection, /invoke\('inspect_agent_live_file', \{[\s\S]*expectedWorkspaceRoot: binding\.root,[\s\S]*expectedWorkspaceDigest: binding\.digest,[\s\S]*expectedWorkspaceInstallation: binding\.installation,[\s\S]*expectedAgentHandoffGeneration: agentGeneration,[\s\S]*relativePath,/);
  assert.match(filesInspection, /\['current-file', 'modified-file', 'new-file'\]\.includes\(answer\.kind\)/);
  assert.match(filesInspection, /text_editable: false/);
  assert.match(daemonLive, /if first\.modified_from_current_version\(\) \{[\s\S]*"modified-file"[\s\S]*\} else \{[\s\S]*"current-file"/);
  assert.match(workspaceFilesChanges, /Read-only snapshot from the assigned agent folder/);
});

test('an unchanged Current refresh renews authority without repainting the mounted tree', () => {
  const renderer = script.slice(
    script.indexOf('function renderWorkspaceCurrentNext'),
    script.indexOf('async function revealCurrentWorkspaceFolder'),
  );
  const refreshAction = script.slice(
    script.indexOf('async function activateCurrentRefresh'),
    script.indexOf('function rollbackWorkspaceStillCurrent'),
  );
  assert.match(renderer, /currentRefreshInFlight[\s\S]*!model\.workspaceVerified[\s\S]*workspaceCurrentNextActions = new Map\(\[\['refresh', activateCurrentRefresh\]\]\)[\s\S]*return;/);
  assert.match(renderer, /projectionKey = JSON\.stringify\(current\)/);
  assert.match(renderer, /canUpdateAuthorityWithoutPainting[\s\S]*if \(canUpdateAuthorityWithoutPainting\) return;/);
  assert.match(refreshAction, /currentRefreshInFlight \+= 1/);
  assert.match(refreshAction, /finally \{[\s\S]*currentRefreshInFlight = Math\.max\(0, currentRefreshInFlight - 1\)[\s\S]*!model\.workspaceVerified\) renderWorkspaceCurrentNext\(\)/);
});

test('workspace entries open only through exact closed native authority', () => {
  assert.match(script, /invoke\('open_managed_workspace_entry', \{[\s\S]*expectedWorkspaceRoot: binding\.root,[\s\S]*expectedWorkspaceDigest: binding\.digest,[\s\S]*expectedWorkspaceInstallation: binding\.installation,[\s\S]*expectedAgentHandoffGeneration: generation,[\s\S]*relativePath: selected\.path,[\s\S]*entryKind: selected\.type,[\s\S]*action,/);
  assert.match(nativeHost, /fn open_managed_workspace_entry\([\s\S]*expected_workspace_root: String,[\s\S]*expected_workspace_digest: String,[\s\S]*expected_workspace_installation: String,[\s\S]*expected_agent_handoff_generation: Option<String>,[\s\S]*relative_path: String,[\s\S]*entry_kind: String,[\s\S]*action: String,/);
  assert.match(nativeHost, /FINDER_APPLICATION_PATH: &str = "\/System\/Library\/CoreServices\/Finder\.app"/);
  assert.match(nativeHost, /let default_application = if action == "open-entry" \{[\s\S]*if is_directory \{[\s\S]*FINDER_APPLICATION_PATH[\s\S]*default_native_application\(entry\.path\(\)\)/);
  assert.match(nativeHost, /verified_managed_workspace_entry\([\s\S]*&relative_path,[\s\S]*is_directory,/);
  assert.match(nativeHost, /action != "open-entry" && action != "reveal-entry"/);
  assert.match(nativeHost, /Command::new\(program\)/);
  assert.match(script, /if \(!beginNativeWorkspaceLaunch\(\)\) return false;[\s\S]*invoke\('open_managed_workspace_entry',[\s\S]*finally \{[\s\S]*finishNativeWorkspaceLaunch\(\)/);
  assert.match(script, /'open-workspace-folder': async \(\) => \{[\s\S]*beginNativeWorkspaceLaunch\(\)[\s\S]*revealCurrentWorkspaceFolder\(\)[\s\S]*Opened the current workspace folder in Finder\.[\s\S]*finishNativeWorkspaceLaunch\(\)/);
  assert.match(script, /workspaceWorkNextActions\.get\(kind\);[\s\S]*Promise\.resolve\(\)[\s\S]*\.then\(action\)[\s\S]*File action unavailable:/);
  assert.match(script, /function workspaceFileActionEnabled\(id\)[\s\S]*id === 'open-workspace-folder'[\s\S]*!workspaceInteractionInFlight\(\)[\s\S]*id === 'open-entry' \|\| id === 'reveal-entry'[\s\S]*!workspaceInteractionInFlight\(\)/);
  assert.doesNotMatch(nativeHost, /shell\.open|Command::new\([^)]*relative_path/);
});

test('saved review sides open only through exact closed native authority', () => {
  assert.match(artifactReview, /Before and after are exact immutable saved sides/);
  assert.match(artifactReview, /Open in default app/);
  assert.match(artifactReview, /Reveal in Finder/);
  assert.match(artifactReview, /Open copy folder/);
  assert.match(script, /invoke\('open_review_artifact_inspection', \{[\s\S]*expectedWorkspaceRoot: binding\.root,[\s\S]*expectedWorkspaceDigest: binding\.digest,[\s\S]*expectedWorkspaceInstallation: binding\.installation,[\s\S]*bundle: item\.bundle,[\s\S]*target: item\.subject_operation,[\s\S]*objectId: change\.object_id,[\s\S]*side,[\s\S]*expectedVersionId: expectedVersion,[\s\S]*expectedContentDigest: expectedDigest,[\s\S]*action,/);
  assert.match(nativeHost, /fn open_review_artifact_inspection\([\s\S]*expected_workspace_root: String,[\s\S]*expected_workspace_digest: String,[\s\S]*expected_workspace_installation: String,[\s\S]*bundle: String,[\s\S]*target: String,[\s\S]*object_id: String,[\s\S]*side: String,[\s\S]*expected_version_id: String,[\s\S]*expected_content_digest: String,[\s\S]*action: String,/);
  assert.match(nativeHost, /review_artifact_for_workspace\([\s\S]*&object_id,[\s\S]*side,/);
  assert.match(nativeHost, /stable_native_reference\(&file\.path, false\)/);
  assert.match(nativeHost, /else if action == "open-folder" \{[\s\S]*FINDER_APPLICATION_PATH/);
  assert.match(nativeHost, /"mesh\.review-side-open\/v1"/);
  assert.match(script, /if \(!beginNativeWorkspaceLaunch\(\)\) return;[\s\S]*invoke\('open_review_artifact_inspection',[\s\S]*finally \{[\s\S]*finishNativeWorkspaceLaunch\(\)/);
  const savedSideLaunch = script.slice(
    script.indexOf('function installCurrentReviewArtifactActions'),
    script.indexOf('async function loadReviewArtifactPreview'),
  );
  assert.ok(
    savedSideLaunch.indexOf('finishNativeWorkspaceLaunch()')
      < savedSideLaunch.indexOf('showNotice(completion.message, completion.error)'),
    'a saved-side success notice must never become actionable before the native-launch freeze is released',
  );
  assert.match(nativeHost, /admitted_review_inspection_extension\([\s\S]*&expected_version_id,[\s\S]*&expected_content_digest,[\s\S]*&action,/);
  assert.doesNotMatch(nativeHost, /Command::new\([^)]*(artifact\.path|file\.path|object_id)/);
});

test('live Review stays read only, generation bound, stable across unchanged polls, and switch safe', () => {
  assert.match(liveAgentReview, /Mutable/);
  assert.match(liveAgentReview, /Unrecorded/);
  assert.match(liveAgentReview, /cannot record, approve, export, save, or update the original folder/);
  assert.doesNotMatch(liveAgentReview, /Approve exact version|Save privately|Create Git branch/);
  assert.match(script, /invoke\('inspect_agent_live_file', \{[\s\S]*expectedWorkspaceRoot: binding\.root,[\s\S]*expectedWorkspaceDigest: binding\.digest,[\s\S]*expectedWorkspaceInstallation: binding\.installation,[\s\S]*expectedAgentHandoffGeneration: agentGeneration,[\s\S]*relativePath: change\.path,/);
  assert.match(script, /if \(JSON\.stringify\(model\.agentLive\) !== JSON\.stringify\(next\)\) \{[\s\S]*renderReviews\(\)/);
  assert.match(script, /switch-live-workspace:[^`]*\$\{workspace\.path\}[\s\S]*workspaceProjectionContinuityKey\(\)[\s\S]*openRecentWorkspacePath/);
  assert.match(nativeHost, /fn inspect_agent_live_file\([\s\S]*expected_agent_handoff_generation: String,[\s\S]*relative_path: String,/);
  assert.match(nativeHost, /\.inspect_agent_live_file\([\s\S]*&expected_agent_handoff_generation,[\s\S]*&relative_path,/);
});

test('agent handoff scans exact native files before an automatic safe save or structural review', () => {
  assert.match(workspaceFilesChanges, /Find folder changes/);
  assert.match(workspaceFilesChanges, /Returning to Mesh automatically inspects the selected folder/);
  assert.match(workspaceFilesChanges, /Outside the confirmed agent-finish flow, saving remains an explicit authenticated action/);
  assert.match(workspaceFilesChanges, /selecting a different recent agent folder inspects it immediately/);
  assert.match(workspaceFilesChanges, /inactive agent folders are not continuously watched/);
  assert.match(script, /while \(folderScanInFlight\) await folderScanSettled/);
  assert.match(script, /await scanNativeFolder\(\{ automatic: true, startup: true, workspaceReturn: true \}\);/);
  assert.match(workspaceFilesChanges, /Save all privately/);
  assert.match(script, /FOLDER_INSPECTION_CONCURRENCY = 8/);
  assert.match(script, /new Array\(candidates\.length\)/);
  assert.match(script, /await inspectFolderCandidates\(candidates, binding, missingByPath\)/);
  assert.match(script, /for \(const \[index, queuedChange\] of queued\.entries\(\)\)/);
  assert.match(script, /expectedContentDigest: inspection\.content_digest/);
  assert.match(script, /discover_native_missing_files/);
  assert.match(script, /discover_native_directories/);
  assert.match(script, /adopt_native_directory/);
  assert.match(script, /adopt_native_file_deletion/);
  assert.match(script, /adopt_native_file_move/);
  assert.match(script, /Mesh never guesses file identity/);
  assert.match(workspaceFilesChanges, /stops on the first mismatch/);
  const finishHandler = script.slice(script.indexOf('async function activateCurrentFinishAgent()'));
  assert.ok(
    finishHandler.indexOf('await inspectAgentFinishPreflight(releaseAttempt)')
      < finishHandler.indexOf("invoke('finish_managed_workspace_agent_handoff'"),
    'agent custody must remain durable until the first complete native inspection succeeds',
  );
  assert.match(nativeHost, /fn inspect_agent_finish_preflight\([\s\S]*expected_workspace_root:[\s\S]*expected_workspace_digest:[\s\S]*expected_workspace_installation:[\s\S]*expected_agent_handoff_generation:/);
  assert.match(nativeHost, /fn inspect_agent_live_work\([\s\S]*expected_workspace_root:[\s\S]*expected_workspace_digest:[\s\S]*expected_workspace_installation:[\s\S]*expected_agent_handoff_generation:/);
  const liveMonitor = nativeHost.slice(
    nativeHost.indexOf('async fn inspect_agent_live_work('),
    nativeHost.indexOf('fn inspect_managed_file(', nativeHost.indexOf('async fn inspect_agent_live_work(')),
  );
  assert.match(liveMonitor, /daemon\.inspect_agent_finish_preflight/);
  assert.doesNotMatch(liveMonitor, /record_agent_handoff_preflight|finish_managed_workspace_agent_handoff/);
  assert.match(script, /inspect_agent_live_work/);
  assert.match(script, /Monitoring never saves or approves work|Live changes are read-only/);
  assert.match(script, /mesh\.agent-finish-preflight\/v1/);
  assert.match(script, /Ordinary file reads stay[\s\S]*unavailable during assignment/);
  assert.ok(
    finishHandler.indexOf("invoke('finish_managed_workspace_agent_handoff'")
      < finishHandler.indexOf('await saveAllPrivateChanges({'),
    'agent custody must be released before the shared private-save path can run',
  );
  assert.match(finishHandler, /native_missing \|\| change\.native_unsupported/);
  assert.match(finishHandler, /privately save unambiguous file and folder changes/);
  assert.match(finishHandler, /Mesh will not guess a rename, deletion, or unsupported entry/);
  assert.match(finishHandler, /Agent folder remains assigned because Mesh could not complete its native inspection/);
  assert.match(
    script,
    /\|\| \(!agentFinished && workspaceChangesEditorState\.canPreserveEdit\)/,
    'the exact Finish rescan still treated a retained Save-button mirror as lifecycle authority',
  );
  assert.match(script, /Create a new task there and give it your instruction/);
  assert.match(script, /label: 'Finish agent handoff'/);
});

test('ambiguous crash recovery is visible and pauses every managed write control', () => {
  assert.match(script, /managed-mutation-recovery-needed/);
  assert.match(script, /checkpoint-recovery-needs-attention/);
  assert.match(script, /Needs attention/);
  assert.match(script, /Management is paused/);
  assert.match(script, /managedMutationRecoveryBlocked\(\)/);
  assert.match(script, /const available = Boolean\(model\.workspace\)[\s\S]*&& !stateBlocked[\s\S]*&& !recoveryBlocked[\s\S]*&& !inspectionBlocked/);
  assert.match(script, /workspaceChangesEditorState\.canPreserveEdit = false/);
  assert.match(script, /workspaceChangesEditorState\.canSavePrivate = false/);
  assert.match(script, /const canApply = previewMatchesSelection[\s\S]*!managedWorkspaceMutationBlocked\(\)[\s\S]*!editorDraftPending\(\)/);
  assert.match(script, /const canUndo = Boolean\([\s\S]*model\.restoreUndo[\s\S]*!managedWorkspaceMutationBlocked\(\)[\s\S]*!editorDraftPending\(\)/);
});

test('startup re-reads state after the detached checkpoint verifier runs', () => {
  assert.match(script, /setTimeout\(\(\) => refresh\(\), 250\)/);
});

test('an empty workspace is ready while an unverified stale snapshot is read only', () => {
  assert.match(script, /workspaceVerified: false/);
  assert.match(script, /error instanceof DaemonRefusal && error\.code === 'no-workspace-open'/);
  assert.match(script, /model\.workspace = null;[\s\S]*model\.workspaceVerified = true;[\s\S]*setLocalService\('ready'\)/);
  assert.match(script, /model\.workspaceVerified = false;[\s\S]*setLocalService\('attention'\)/);
  assert.match(script, /managedMutationBlocked\(\)/);
  assert.match(script, /current workspace state could not be verified\. Refresh before making another managed change/);
});

test('restore discovery comes from durable workspace history rather than pasted identities', () => {
  assert.match(script, /file_histories/);
  assert.match(script, /retained_versions/);
  assert.match(script, /history\.current\?\.version_id/);
  assert.doesNotMatch(html, /Paste the file identity/);
  assert.doesNotMatch(html, /id="restore-object"/);
});

test('webview uses only the Tauri command bridge and does not open storage or a transport', () => {
  assert.match(script, /__TAURI__/);
  assert.doesNotMatch(script, /node:|WebSocket|indexedDB|localStorage|fetch\s*\(/);
  assert.doesNotMatch(script, /records\.mesh|metadata\.sqlite|mesh-store|mesh-cas/);
});

test('automatic native save is explicit, native-host persisted, and fail-closed', () => {
  assert.doesNotMatch(html, /id="auto-save-native(?:-hint)?"/);
  assert.match(script, /native_capture_preference/);
  assert.match(script, /set_native_capture_preference/);
  assert.match(nativeHost, /NativeCapturePreference/);
  assert.match(nativeHost, /set_native_capture_preference/);
  assert.match(script, /!changes\.some\(\(change\) => change\.native_missing \|\| change\.native_unsupported\)/);
  assert.match(script, /!workspaceInstallationMatchesHandoff\(\)/);
  assert.match(script, /periodic && model\.nativeCaptureEnabled && changes\.length > 0/);
  assert.match(script, /appDocument\.visibilityState !== 'visible' && !model\.nativeCaptureEnabled/);
  assert.match(script, /await loadNativeCapturePreference\(\);[\s\S]*await refresh\(true\);/);
  const preferenceHandler = script.slice(
    script.indexOf('async function updateNativeCapturePreference(requested)'),
    script.indexOf('async function recordSelectedStructuralChange()'),
  );
  assert.match(preferenceHandler, /binding = captureVerifiedWorkspace\(\)/);
  assert.match(preferenceHandler, /assertVerifiedWorkspace\(binding\)/);
  assert.ok(
    preferenceHandler.indexOf("invoke('set_native_capture_preference'")
      < preferenceHandler.indexOf('binding = captureVerifiedWorkspace()'),
    'the global preference must be written and read back before any workspace-scoped binding is captured',
  );
  assert.ok(
    preferenceHandler.indexOf('assertVerifiedWorkspace(binding)')
      < preferenceHandler.indexOf('await saveAllPrivateChanges({'),
    'enabling automatic save must recheck the exact initiating workspace before saving its existing queue',
  );
  assert.match(workspaceFilesChanges, /pauses for deletions, renames, symbolic links, special entries, active agent handoffs/);
  assert.doesNotMatch(styles, /\.auto-save-choice|\.editor\s*\{/);
});

test('managed editing separates recovery preservation from authenticated private save', () => {
  assert.match(script, /inspect_managed_file/);
  assert.match(script, /preserve_managed_text/);
  assert.match(script, /save_managed_private/);
  assert.match(script, /inspect_native_file/);
  assert.match(script, /adopt_native_file/);
  assert.match(script, /discover_native_missing_files/);
  assert.match(script, /discover_native_directories/);
  assert.match(script, /adopt_native_directory/);
  assert.match(script, /adopt_native_file_deletion/);
  assert.match(script, /adopt_native_file_move/);
  assert.match(script, /native_untracked_files/);
  assert.match(script, /author_authenticated/);
  assert.match(script, /exact recovery preserved/);
  assert.match(workspaceFilesChanges, /LOCAL \+ AUTHENTICATED/);
  assert.match(workspaceFilesChanges, /Save privately/);
  assert.match(workspaceFilesChanges, /stable native folder with a local editor/);
  assert.match(script, /modified_from_current_version/);
  assert.match(script, /local folder change detected/);
  assert.match(script, /!model\.editor\.modified_from_current_version/);
  assert.match(script, /binary or large local change detected/);
  assert.match(script, /!model\.editor\.text_editable/);
  assert.equal(
    script.match(/expectedContentDigest: model\.editor\.content_digest/g)?.length,
    2,
    'both working-copy and private saves must bind to the bytes the person inspected',
  );
  assert.equal(
    script.match(/expectedExecutable: model\.editor\.executable/g)?.length,
    2,
    'both working-copy and private saves must bind to the portable metadata the person inspected',
  );
  assert.match(workspaceFilesChanges, /binary and large files/);
  assert.match(workspaceFilesChanges, /reviews a complete new folder tree/);
  assert.match(workspaceFilesChanges, /admits its folders parent-first/);
  assert.doesNotMatch(script, /workspace\.file\.write|workspace\.save/);
});

test('earlier versions restore and undo through the native recovery boundary', () => {
  assert.match(script, /restore_managed_version/);
  assert.match(nativeHost, /preview_managed_working_copy_restore_for_workspace/);
  assert.match(script, /mesh\.managed-working-copy-restore-preview\/v1/);
  assert.match(script, /expectedContentDigest: preview\.working_copy\.content_digest/);
  assert.match(script, /expectedExecutable: preview\.working_copy\.executable/);
  assert.match(script, /expectedExecutable: result\.executable/);
  assert.match(script, /managed_checkpoint_state/);
  assert.match(script, /completed in the working copy; exact recovery is preserved/);
  assert.match(workspaceRestore, /Restore in working copy/);
  assert.match(script, /undoLabel: 'Undo last restore'/);
  assert.match(workspaceRestore, /Private history is not rewritten/);
});

test('native workspace versions are independent folders for people and agents', () => {
  assert.match(script, /open_managed_workspace_version/);
  assert.match(script, /open_fresh_agent_workspace/);
  assert.match(script, /function workspaceDisplayName\(path, projectName = null, sourcePointOrdinal = null\)/);
  assert.match(script, /source_point_ordinal/);
  assert.match(script, /Copy of saved version/);
  assert.match(script, /function projectDisplayName\(projectRoot, entries\)/);
  assert.match(script, /workspace_entries/);
  assert.match(script, /entry\.project_root/);
  assert.match(script, /Working copy/);
  assert.doesNotMatch(script, /Agent copy/);
  assert.match(script, /Current saved workspace/);
  assert.match(nativeHost, /VersionWorkspaceDirectory/);
  assert.match(workspaceVersions, /Leave blank for Mesh-managed storage/);
  assert.match(script, /The stable working path now opens this version in Finder or a newly opened editor/);
  assert.match(script, /Reopen any editor or terminal that was already using the prior folder/);
  assert.match(script, /existing directory handles stay on that prior version/);
  assert.match(script, /Opened \$\{selectedPoint\} in your working folder/);
  assert.doesNotMatch(script, /answer\.source_version\.slice/);
  assert.match(script, /That agent stays on this version if Mesh switches elsewhere/);
  assert.match(script, /workspace metadata, never file content or Mesh mutation authority/);
  assert.match(script, /id: 'return-workspace'/);
  assert.match(script, /paths\[0\] === current \? paths\[1\] \|\| null : null/);
  assert.match(script, /reveal_managed_workspace/);
  assert.match(nativeHost, /ActiveWorkspaceLink/);
  assert.match(script, /stable native folder/);
  assert.match(workspaceVersions, /Open an exact saved point/);
  assert.match(workspaceVersions, /data-mesh-proof="workspace-version-preview-ready"/);
  assert.match(script, /preview_managed_workspace_version/);
  assert.match(script, /Exact retained content verified/);
  assert.match(workspaceVersions, /Custom private location/);
  assert.match(workspaceVersions, /workspace you leave stays untouched/);
  assert.match(workspaceVersions, /Existing editors and agents keep their current directory handle/);
  assert.match(demo, /Give the \*\*Pinned agent folder\*\* shown by Mesh to an agent/);
  assert.match(demo, /Do not give a long-running\s+agent the stable link/);
  assert.doesNotMatch(demo, /Use that stable path with Finder, VS Code,\s*Terminal, or an agent/);
  assert.doesNotMatch(demo, /Give the stable native folder shown by Mesh to an agent/);
  assert.match(workspaceFilesChanges, /Give Terminal or a long-running agent the independent agent folder/);
  assert.match(desktopReadme, /long-running agents should receive the independent real folder/);
  assert.doesNotMatch(desktopReadme, /stable navigation path for Finder, editors,\s+terminals, and agents/);
  assert.match(script, /refuseEditorDraftDeparture/);
  assert.match(script, /async function confirmWorkspaceSwitch\(\{/);
  assert.match(script, /blockedAction = 'removing this workspace'/);
  assert.match(script, /Mesh will not discard text that exists only in this window/);
  assert.equal(
    script.match(/refuseEditorDraftDeparture\(\)/g)?.length,
    4,
    'departure must check drafts before and after inspection plus the protected import boundary',
  );
  assert.match(script, /They will remain in that workspace and are not part of the saved version/);
  assert.equal(
    script.match(/await confirmWorkspaceSwitch\(\)/g)?.length,
    2,
    'import and managed-workspace switches must protect local work',
  );
  assert.match(script, /confirmWorkspaceSwitch\(\{ allowNativeChanges, allowUnverified, blockedAction \}\)/);
  assert.equal(
    script.match(/blockedAction: 'creating the native working folder'/g)?.length,
    3,
    'every legacy native-folder handoff must refuse to strand unsaved work',
  );
  assert.match(script, /confirmWorkspaceSwitch\(\{ allowNativeChanges: false, allowUnverified: false \}\)/);
  assert.match(productionLayout, /Local only/);
  assert.match(workspaceRestore, /Working copy only/);
});

test('the desktop guide describes one reviewed parent-first native tree save', () => {
  assert.match(desktopReadme, /One scan presents the complete supported new directory tree/);
  assert.match(desktopReadme, /Save all privately/);
  assert.match(desktopReadme, /parent-first order/);
  assert.doesNotMatch(
    desktopReadme,
    /reveals the next nested folder or file as a separate review item/,
  );
});

test('saved files export to ordinary folders only through exact preview and atomic confirmation', () => {
  assert.match(script, /preview_managed_export/);
  assert.match(script, /preview_managed_exports/);
  assert.match(script, /preview_managed_directory_exports/);
  assert.match(script, /export_managed_file/);
  assert.match(script, /export_managed_directories/);
  assert.match(script, /expectedTargetInstallation: preview\.target_installation/);
  assert.match(script, /expectedTargetParentInstallation: preview\.target_parent_installation/);
  assert.match(script, /expectedTargetFileInstallation: preview\.target_file_installation/);
  assert.match(script, /expectedSourceVersion: preview\.source_version/);
  assert.match(script, /expectedSourceDigest: preview\.source_content_digest/);
  assert.match(script, /expectedTargetDigest: preview\.target_content_digest/);
  assert.match(script, /expectedTargetInstallation: batch\.targetInstallation/);
  assert.match(script, /Mesh history and the managed working file will not change/);
  assert.match(nativeHost, /preview_managed_file_export/);
  assert.match(nativeHost, /preview_all_managed_file_exports/);
  assert.match(nativeHost, /preview_managed_directory_export/);
  assert.match(nativeHost, /preview_managed_directory_exports/);
  assert.match(nativeHost, /export_managed_file/);
  assert.match(nativeHost, /export_managed_directory/);
  assert.match(nativeHost, /export_managed_directories/);
  assert.match(nativeHost, /discover_retired_exports/);
  assert.match(nativeHost, /preview_retired_export/);
  assert.match(nativeHost, /remove_retired_export/);
  assert.match(workspaceDestination, /automatically re-previews the files/);
  assert.match(workspaceDestination, /After current paths are installed, Mesh derives former saved paths/);
  assert.match(workspaceDestination, /never removes a destination entry silently/);
  assert.doesNotMatch(workspaceDestination, /non-pruning|remove separately after review/i);
  assert.match(workspaceDestination, /separate reviewed removal step/);
  assert.match(workspaceDestination, /Only paths proven to come from the original import or an exact durable update receipt/);
  assert.match(workspaceDestination, /Private-only, changed, replaced, or unrelated destination entries are preserved/);
  assert.match(workspaceDestination, /recursive deletion is unavailable/);
  assert.match(script, /renderSingleExportPlan\(preview\)/);
  assert.match(script, /renderDirectoryExportPlan\(targetRoot, missingDirectories\)/);
  assert.match(script, /renderCurrentFileExportPlan\(targetRoot, changed, previews\.length - changed\.length - preserved\.length, preserved\)/);
  assert.match(script, /preview\.replace_allowed/);
  assert.match(script, /changed outside this workspace/);
  assert.match(nativeHost, /"target_relation"/);
  assert.match(nativeHost, /"replace_allowed"/);
  assert.match(script, /renderRetiredExportPlan\(targetRoot, kind, previews, preserved, alreadyAbsent\)/);
  assert.match(script, /renderExportComplete\(targetRoot, preserved, alreadyAbsent\)/);
  assert.match(script, /kind === 'retired-files'/);
  assert.match(script, /kind === 'retired-directories'/);
  assert.match(script, /changed and unrelated files are preserved/i);
  assert.match(script, /no durable import or update receipt proves this path — keep it/);
  assert.match(script, /Recursive deletion is unavailable/);
  assert.match(script, /nextPhase = 'retired-files'/);
});

test('the alpha walkthrough distinguishes original and redirected destination vocabulary', () => {
  assert.match(workspaceDestination, /Update saved work in an ordinary folder/);
  assert.match(workspaceDestination, /Mesh remembers and prefills the original folder used for an import, including after you switch to an older saved version/);
  assert.match(workspaceDestination, /For a workspace opened manually, choose a destination once/);
  assert.doesNotMatch(workspaceDestination, /manually opened or older workspace/);
  assert.match(workspaceDestination, /Selected destination/);
  assert.match(productionRoute, /label: "Update destination"/);
  assert.match(workspaceDestination, /Preview the selected file/);
  assert.match(workspaceDestination, /Preview folders and files together/);
  assert.match(workspaceDestination, /button\("confirm-batch", "primary"/);
  assert.doesNotMatch(workspaceDestination, /pull(?:ed|ing)?[- ]back/i);
  assert.doesNotMatch(script, /['"`]([^'"`]*pull(?:ed|ing)?[- ]back[^'"`]*)['"`]/i);
});
