import { startFleets } from './fleets.js';
import { startAttachedProjects } from './attached-projects.js';
import { workspaceVersionsRetainedInteraction } from './workspace-versions-interaction-policy.js';
import { createNoticeSource } from './notice-source.js';
import {
  workspaceEditorBaselineAvailable,
  workspaceEditorBaselineText,
} from './workspace-editor-baseline.js';

// Capture the webview objects once. This is equivalent in the packaged app and keeps each
// independently loaded UI instance bound to its own window during concurrent regression tests.
const appWindow = window;
const appDocument = document;
const invoke = appWindow.__TAURI__?.core?.invoke;
startAttachedProjects({ document: appDocument, invoke, CustomEvent });
startFleets({ document: appDocument, invoke, CustomEvent });
const $ = (id) => appDocument.getElementById(id);
const noticeSource = createNoticeSource(appDocument, CustomEvent);
const model = { source: null, destination: null, preview: null, workspace: null, workspaceVerified: false, checkpoint: null, agentFolder: null, agentHandoff: null, agentLive: null, editor: null, folderChanges: [], nativeInspectionFailed: false, nativeCaptureEnabled: false, nativeCaptureAvailable: false, nativeCaptureChanging: false, exportPreview: null, exportBatchPreview: null, exportRoot: null, pullBackPrompt: null, restorePreview: null, restoreUndo: null, workspaceVersionPreview: null, workspaceVersionPreviewError: null, recent: null, activeFolder: null, approval: null };
const LOCAL_SERVICE_COPY = Object.freeze({
  starting: 'Local service starting',
  ready: 'Local service ready',
  attention: 'Local service needs attention',
});
let localService = Object.freeze({ state: 'starting', label: LOCAL_SERVICE_COPY.starting });
let buildIdentity = Object.freeze({
  label: 'Build identity unavailable',
  title: 'Mesh refused a malformed or incomplete build identity.',
});

function setLocalService(state) {
  const label = LOCAL_SERVICE_COPY[state];
  if (!label) throw new Error('Mesh refused an invalid local service state.');
  localService = Object.freeze({ state, label });
  appDocument.dispatchEvent(new CustomEvent('mesh:service-state-projection', {
    detail: localService,
  }));
}

function publishBuildIdentity() {
  appDocument.dispatchEvent(new CustomEvent('mesh:build-identity-projection', {
    detail: buildIdentity,
  }));
}

function setBuildIdentity(label, title) {
  buildIdentity = Object.freeze({ label, title });
  publishBuildIdentity();
}

appDocument.addEventListener('mesh:build-identity-available', publishBuildIdentity);
setLocalService('starting');
let workspaceVerificationSequence = 0;
let workspaceSelectionSequence = 0;
let workspaceVersionPreviewSequence = 0;
let exportPreviewSequence = 0;
let exportDestinationSelectionSequence = 0;
let exportDestinationInputSequence = 0;
let exportDestinationChooserRevision = 0;
let workspaceMutationInFlight = false;
let workspaceTransitionInFlight = false;
let folderScanInFlight = false;
let folderScanSettled = Promise.resolve();
let nativeWorkspaceLaunchInFlight = false;
let agentLiveInspectionInFlight = false;
let agentLiveInspectionSequence = 0;
let nextActionSnapshot = null;
const NATIVE_SCAN_INTERVAL_MS = 5_000;
// Background discovery is a hint, not the review/approval authority. A full exact scan still runs
// whenever the person returns to Mesh, asks to find changes, records a review, or approves one.
// Bound only the repeating visible-window poll so a large repository cannot make the desktop read
// and serialize every tracked file every five seconds forever.
const PERIODIC_FILE_INSPECTION_LIMIT = 128;
const NATIVE_SWITCH_HANDLE_NOTICE = 'Reopen any editor or terminal that was already using the prior folder; existing directory handles stay on that prior version.';
const APPROVAL_STATUS_UNVERIFIED = 'Mesh could not verify approval availability. Refresh before setting up or approving a version.';
let periodicInspectionKey = null;
let periodicInspectionCursor = 0;
let stableNavigationRepairPending = false;
let approvalStatusSequence = 0;
let approvalEnrollmentInFlight = false;
let pendingAgentVersionChoice = false;
const FOLDER_INSPECTION_CONCURRENCY = 8;
let reviewWorkbenchNextAvailable = false;
// Review is React-owned. Exact pending/mounted generations keep its source-owned actions closed
// whenever the projection is stale, rejected, or absent.
let reviewWorkbenchNextGeneration = 0;
let reviewWorkbenchNextArtifactRequestSequence = 0;
let reviewWorkbenchNextPending = null;
let reviewWorkbenchNextMounted = null;
let reviewWorkbenchNextActions = new Map();
let workspaceOverviewNextAvailable = false;
let workspaceOverviewNextGeneration = 0;
let workspaceOverviewNextPending = null;
let workspaceOverviewNextMounted = null;
let workspaceOverviewNextActions = new Map();
let workspaceCurrentNextAvailable = false;
let workspaceCurrentNextGeneration = 0;
let workspaceCurrentNextPending = null;
let workspaceCurrentNextMounted = null;
let workspaceCurrentNextActions = new Map();
let currentRefreshInFlight = 0;
let workspaceWorkNextAvailable = false;
let workspaceWorkNextGeneration = 0;
let workspaceWorkNextPending = null;
let workspaceWorkNextMounted = null;
let workspaceWorkNextActions = new Map();
let workspaceWorkFieldEchoGeneration = null;
let workspaceFilePreviewSequence = 0;
let workspaceFilesState = {
  newPath: '',
  selectedEntry: '',
  movePath: '',
  canEditNewPath: false,
  canSelectEntry: false,
  canEditMovePath: false,
  status: 'Open a managed workspace to manage its entries.',
};
let workspaceChangesEditorState = {
  selectedFile: '',
  canSelectFile: false,
  editorText: '',
  canEditText: false,
  canLoadFile: false,
  canPreserveEdit: false,
  canSavePrivate: false,
  editState: 'No file open',
  editVersion: '',
};
const WORKSPACE_STRUCTURAL_CHANGE_HINT = 'Mesh never guesses file identity from a similar path. Choose deletion, or explicitly pair the missing tracked file with a new file. Exact-byte moves can finish immediately; a move whose content also changed stays Working until you scan and save those bytes.';
let workspaceChangesQueueState = {
  scanLabel: 'Find folder changes',
  scanState: 'idle',
  canScan: false,
  saveAllLabel: 'Save all privately',
  savingAll: false,
  canSaveAllPrivate: false,
  missingSource: '',
  moveTarget: '',
  canChooseStructural: false,
  canRecordStructural: false,
  structuralHint: WORKSPACE_STRUCTURAL_CHANGE_HINT,
};
let importWorkbenchNextAvailable = false;
let importWorkbenchNextGeneration = 0;
let importWorkbenchNextPending = null;
let importWorkbenchNextMounted = null;
let importWorkbenchNextActions = new Map();
let importWorkbenchNextInteraction = null;
let importSourceDraft = '';
let importDestinationChooserInFlight = null;
let importConfirmationInFlight = false;
const IMPORT_CHOOSER_FOCUS_SELECTOR = '[data-mesh-import-choose]';
let workspaceVersionsNextAvailable = false;
let workspaceVersionsNextGeneration = 0;
let workspaceVersionsNextPending = null;
let workspaceVersionsNextMounted = null;
let workspaceVersionsNextActions = new Map();
let workspaceVersionsNextInteraction = null;
let selectedWorkspaceVersionOperation = '';
let workspaceVersionDestinationDraft = '';
let workspaceChromeNextAvailable = false;
let workspaceChromeNextGeneration = 0;
let workspaceChromeNextPending = null;
let workspaceChromeNextMounted = null;
let workspaceChromeIdentity = 0;
let workspaceChromeIdentityKey = 'workspace:none';
let workspaceEntryNextAvailable = false;
let workspaceEntryNextGeneration = 0;
let workspaceEntryNextPending = null;
let workspaceEntryNextMounted = null;
let workspaceEntryNextActions = new Map();
let workspaceEntryManagedPathDraft = '';
let workspaceEntryDisclosureOpen = false;
let selectedRecentWorkspacePath = '';
const unavailableRecentWorkspacePaths = new Set();
let workspaceEntrySelectionInFlight = 0;
let workspaceEntryRefreshInFlight = false;
let workspaceRestoreNextAvailable = false;
let workspaceRestoreNextGeneration = 0;
let workspaceRestoreNextPending = null;
let workspaceRestoreNextMounted = null;
let workspaceRestoreNextActions = new Map();
let workspaceRestoreNextInteraction = null;
let selectedRestoreFileId = '';
let selectedRestoreVersionId = '';
let restoreApplyAttempted = false;
let restoreRecoveryScanPending = false;
let workspaceDestinationNextAvailable = false;
let workspaceDestinationNextGeneration = 0;
let workspaceDestinationNextPending = null;
let workspaceDestinationNextMounted = null;
let workspaceDestinationNextActions = new Map();
let workspaceDestinationFieldEchoGeneration = null;
let workspaceDestinationFieldEchoField = null;
let workspaceDestinationDraft = '';
let workspaceDestinationSelectedFile = '';
let workspaceDestinationCanEdit = false;
let workspaceDestinationHint = 'Save a file privately before updating another folder from it.';
let workspaceDestinationPlanText = '';
let confirmationNextAvailable = false;
let confirmationNextGeneration = 0;
let confirmationNextPending = null;
let confirmationNextMounted = null;
const CONFIRMATION_MOUNT_TIMEOUT_MS = 1_500;
const PDF_PREVIEW_PAGE_LIMIT = 64;
const PDF_DOCUMENT_PAGE_LIMIT = 1_000_000;

function reviewWorkbenchActionKey(intent) {
  if (intent === null || typeof intent !== 'object' || Array.isArray(intent)
    || typeof intent.type !== 'string') return null;
  const keys = Object.keys(intent).sort();
  if (['record-review', 'setup-approval', 'approve-version', 'approve-and-export', 'export-git', 'choose-private-export'].includes(intent.type)) {
    return keys.length === 1 && keys[0] === 'type' ? intent.type : null;
  }
  if (intent.type === 'inspect-exact-copies') {
    return keys.length === 2
      && keys[0] === 'changeId'
      && keys[1] === 'type'
      && typeof intent.changeId === 'string'
      ? `${intent.type}:${intent.changeId}`
      : null;
  }
  if (intent.type === 'open-review-side') {
    return keys.length === 4
      && keys[0] === 'action'
      && keys[1] === 'changeId'
      && keys[2] === 'side'
      && keys[3] === 'type'
      && typeof intent.changeId === 'string'
      && ['before', 'after'].includes(intent.side)
      && ['open-entry', 'reveal-entry', 'open-folder'].includes(intent.action)
      ? `${intent.type}:${intent.changeId}:${intent.side}:${intent.action}`
      : null;
  }
  if (intent.type === 'load-artifact-preview') {
    return keys.length === 3
      && keys[0] === 'changeId'
      && keys[1] === 'pageNumber'
      && keys[2] === 'type'
      && typeof intent.changeId === 'string'
      && Number.isSafeInteger(intent.pageNumber)
      && intent.pageNumber > 0
      && intent.pageNumber <= PDF_PREVIEW_PAGE_LIMIT
      ? `${intent.type}:${intent.changeId}`
      : null;
  }
  if (intent.type === 'load-live-file' || intent.type === 'switch-live-workspace') {
    return keys.length === 2
      && keys[0] === 'path'
      && keys[1] === 'type'
      && typeof intent.path === 'string'
      && intent.path.length > 0
      && intent.path.length <= 4_096
      && !/[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(intent.path)
      ? `${intent.type}:${intent.path}`
      : null;
  }
  if (intent.type === 'open-earlier-review') {
    return keys.length === 2
      && keys[0] === 'operation'
      && keys[1] === 'type'
      && typeof intent.operation === 'string'
      && /^[0-9a-f]{64}$/u.test(intent.operation)
      ? `${intent.type}:${intent.operation}`
      : null;
  }
  return null;
}

function workspaceVersionsActionKey(intent) {
  if (intent === null || typeof intent !== 'object' || Array.isArray(intent)
    || typeof intent.type !== 'string') return null;
  const keys = Object.keys(intent).sort();
  if (intent.type === 'set-custom-location') {
    return keys.length === 2
      && keys[0] === 'path'
      && keys[1] === 'type'
      && typeof intent.path === 'string'
      && intent.path.length <= 4_096
      && !/[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(intent.path)
      ? intent.type
      : null;
  }
  if (!['select-version', 'open-version', 'start-codex'].includes(intent.type)
    || keys.length !== 2
    || keys[0] !== 'operation'
    || keys[1] !== 'type'
    || typeof intent.operation !== 'string'
    || !/^[0-9a-f]{64}$/u.test(intent.operation)) return null;
  return `${intent.type}:${intent.operation}`;
}

function workspaceEntryActionKey(intent) {
  if (intent === null || typeof intent !== 'object' || Array.isArray(intent)
    || typeof intent.type !== 'string') return null;
  const keys = Object.keys(intent).sort();
  if (['choose-folder', 'choose-managed-folder', 'retry'].includes(intent.type)) {
    return keys.length === 1 && keys[0] === 'type' ? intent.type : null;
  }
  if (intent.type === 'set-disclosure') {
    return keys.length === 2
      && keys[0] === 'open'
      && keys[1] === 'type'
      && typeof intent.open === 'boolean'
      ? intent.type
      : null;
  }
  if (!['update-managed-path', 'open-managed-path', 'select-recent', 'open-recent', 'forget-recent'].includes(intent.type)
    || keys.length !== 2
    || keys[0] !== 'path'
    || keys[1] !== 'type'
    || typeof intent.path !== 'string'
    || intent.path.length > 4_096
    || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(intent.path)
    || (intent.type !== 'update-managed-path' && intent.path.length === 0)) return null;
  return ['update-managed-path', 'open-managed-path'].includes(intent.type)
    ? intent.type
    : `${intent.type}:${intent.path}`;
}

function workspaceRestoreActionKey(intent) {
  if (intent === null || typeof intent !== 'object' || Array.isArray(intent)
    || typeof intent.type !== 'string') return null;
  const keys = Object.keys(intent).sort();
  if (['preview', 'apply', 'undo'].includes(intent.type)) {
    return keys.length === 1 && keys[0] === 'type' ? intent.type : null;
  }
  if (!['select-file', 'select-version'].includes(intent.type)
    || keys.length !== 2
    || keys[0] !== 'id'
    || keys[1] !== 'type'
    || typeof intent.id !== 'string'
    || intent.id.length === 0
    || intent.id.length > 256
    || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(intent.id)) return null;
  return `${intent.type}:${intent.id}`;
}

const WORKSPACE_DESTINATION_ACTION_IDS = Object.freeze([
  'choose-destination', 'preview-single', 'confirm-single', 'preview-all', 'confirm-batch',
]);
const WORKSPACE_DESTINATION_FIELDS = Object.freeze(['selectedFile', 'destination']);

function workspaceDestinationActionKey(intent) {
  if (intent === null || typeof intent !== 'object' || Array.isArray(intent) || typeof intent.type !== 'string') return null;
  const keys = Object.keys(intent).sort();
  if (intent.type === 'activate') {
    if (!WORKSPACE_DESTINATION_ACTION_IDS.includes(intent.action)) return null;
    if (intent.action === 'preview-single') {
      return keys.length === 4
        && keys[0] === 'action'
        && keys[1] === 'destination'
        && keys[2] === 'selectedFile'
        && keys[3] === 'type'
        && typeof intent.selectedFile === 'string'
        && intent.selectedFile.length > 0
        && intent.selectedFile.length <= 4_096
        && typeof intent.destination === 'string'
        && intent.destination.length > 0
        && intent.destination.length <= 4_096
        && !/[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(intent.selectedFile)
        && !/[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(intent.destination)
        ? 'activate:preview-single'
        : null;
    }
    if (intent.action === 'preview-all') {
      return keys.length === 3
        && keys[0] === 'action'
        && keys[1] === 'destination'
        && keys[2] === 'type'
        && typeof intent.destination === 'string'
        && intent.destination.length > 0
        && intent.destination.length <= 4_096
        && !/[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(intent.destination)
        ? 'activate:preview-all'
        : null;
    }
    return keys.length === 2 && keys[0] === 'action' && keys[1] === 'type'
      ? `activate:${intent.action}`
      : null;
  }
  if (intent.type !== 'set-field'
    || keys.length !== 3
    || keys[0] !== 'field'
    || keys[1] !== 'type'
    || keys[2] !== 'value'
    || !WORKSPACE_DESTINATION_FIELDS.includes(intent.field)
    || typeof intent.value !== 'string'
    || intent.value.length > 4_096
    || /[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(intent.value)) return null;
  return `set-field:${intent.field}`;
}

function workspaceCurrentActionKey(intent) {
  if (intent === null || typeof intent !== 'object' || Array.isArray(intent)) return null;
  const keys = Object.keys(intent).sort();
  if (intent.type === 'switch-workspace') {
    return keys.length === 2
      && keys[0] === 'path'
      && keys[1] === 'type'
      && typeof intent.path === 'string'
      && intent.path.length > 0
      && intent.path.length <= 4_096
      && !/[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(intent.path)
      ? `switch-workspace:${intent.path}`
      : null;
  }
  if (keys.length !== 2 || keys[0] !== 'action' || keys[1] !== 'type' || intent.type !== 'activate') return null;
  return typeof intent.action === 'string' ? intent.action : null;
}

const WORKSPACE_WORK_FIELDS = Object.freeze([
  'newPath', 'selectedEntry', 'movePath', 'selectedFile', 'editorText', 'missingSource', 'moveTarget',
]);
const WORKSPACE_WORK_ACTION_IDS = Object.freeze([
  'create-text', 'create-folder', 'move-entry', 'delete-entry', 'open-entry', 'reveal-entry',
  'open-workspace-folder', 'scan-files',
  'load-file', 'preserve-edit', 'save-private', 'save-all-private', 'record-structural-change',
]);
const WORKSPACE_WORK_ACTION_ECHO_FIELDS = Object.freeze({
  'create-text': 'newPath',
  'create-folder': 'newPath',
  'move-entry': 'movePath',
  'load-file': 'selectedFile',
  'preserve-edit': 'editorText',
});
function workspaceWorkIntentKind(intent) {
  if (intent === null || typeof intent !== 'object' || Array.isArray(intent) || typeof intent.type !== 'string') return null;
  const keys = Object.keys(intent).sort();
  if (intent.type === 'activate') {
    if (!WORKSPACE_WORK_ACTION_IDS.includes(intent.action)) return null;
    if (intent.action === 'record-structural-change') {
      return keys.length === 4
        && keys[0] === 'action'
        && keys[1] === 'missingSource'
        && keys[2] === 'moveTarget'
        && keys[3] === 'type'
        && typeof intent.missingSource === 'string'
        && intent.missingSource.length > 0
        && intent.missingSource.length <= 4_096
        && typeof intent.moveTarget === 'string'
        && intent.moveTarget.length <= 4_096
        && !/[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(intent.missingSource)
        && !/[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(intent.moveTarget)
        ? 'activate:record-structural-change'
        : null;
    }
    if (keys.length === 2 && keys[0] === 'action' && keys[1] === 'type') return `activate:${intent.action}`;
    const field = WORKSPACE_WORK_ACTION_ECHO_FIELDS[intent.action];
    return keys.length === 4
      && keys[0] === 'action'
      && keys[1] === 'field'
      && keys[2] === 'type'
      && keys[3] === 'value'
      && intent.field === field
      && typeof intent.value === 'string'
      && intent.value.length <= (field === 'editorText' ? 1_048_576 : 4_096)
      && !/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(intent.value)
      ? `activate:${intent.action}`
      : null;
  }
  if (intent.type === 'set-auto-save') {
    return keys.length === 2 && keys[0] === 'checked' && keys[1] === 'type' && typeof intent.checked === 'boolean'
      ? 'set-auto-save'
      : null;
  }
  if (intent.type !== 'set-field'
    || keys.length !== 3
    || keys[0] !== 'field'
    || keys[1] !== 'type'
    || keys[2] !== 'value'
    || !WORKSPACE_WORK_FIELDS.includes(intent.field)
    || typeof intent.value !== 'string'
    || intent.value.length > (intent.field === 'editorText' ? 1_048_576 : 4_096)
    || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(intent.value)) return null;
  return 'set-field';
}

function dispatchWorkspaceWorkControl(control, eventName) {
  if (typeof control.dispatchEvent === 'function' && typeof appWindow.Event === 'function') {
    control.dispatchEvent(new appWindow.Event(eventName, { bubbles: true }));
    return;
  }
  if (typeof control.emit === 'function') {
    Promise.resolve(control.emit(eventName)).catch((error) => {
      showNotice(`The files and changes control could not be updated: ${error}`, true);
    });
  }
}

function applyWorkspaceWorkField(field, value, interactionGeneration) {
  if (field === 'selectedFile' || field === 'editorText') {
    const enabled = field === 'selectedFile'
      ? workspaceChangesEditorState.canSelectFile
      : workspaceChangesEditorState.canEditText;
    if (!enabled) return;
    const authority = workspaceWorkNextPending?.authority?.[field];
    if (authority instanceof Set && !authority.has(value)) return;
    if (field === 'selectedFile') {
      if (refuseEditorDraftFileDeparture(value)) return;
      workspaceChangesEditorState.selectedFile = value;
      renderEditorChoices();
    } else {
      workspaceChangesEditorState.editorText = value;
      updateEditorDraftPresentation();
    }
    workspaceWorkFieldEchoGeneration = interactionGeneration;
    try {
      renderWorkspaceFilesChangesNext(interactionGeneration, field);
    } finally {
      workspaceWorkFieldEchoGeneration = null;
    }
    return;
  }
  if (field === 'newPath' || field === 'selectedEntry' || field === 'movePath') {
    const enabled = field === 'newPath'
      ? workspaceFilesState.canEditNewPath
      : field === 'selectedEntry'
        ? workspaceFilesState.canSelectEntry
        : workspaceFilesState.canEditMovePath;
    if (!enabled) return;
    const authority = workspaceWorkNextPending?.authority?.[field];
    if (authority instanceof Set && !authority.has(value)) return;
    if (field === 'selectedEntry') {
      workspaceFilesState.selectedEntry = value;
      workspaceFilesState.movePath = '';
    } else {
      workspaceFilesState[field] = value;
    }
    const fieldEchoGeneration = field === 'newPath' || field === 'movePath'
      ? interactionGeneration
      : null;
    workspaceWorkFieldEchoGeneration = fieldEchoGeneration;
    try {
      renderManagementChoices();
    } finally {
      workspaceWorkFieldEchoGeneration = null;
    }
    renderWorkspaceFilesChangesNext(fieldEchoGeneration, field);
    if (field === 'selectedEntry') {
      void inspectManagedWorkspaceEntryFromFiles(value);
    }
    return;
  }
  if (field !== 'missingSource' && field !== 'moveTarget') return;
  if (!workspaceChangesQueueState.canChooseStructural) return;
  const authority = workspaceWorkNextPending?.authority?.[field];
  if (authority instanceof Set && !authority.has(value)) return;
  if (field === 'missingSource') {
    workspaceChangesQueueState.missingSource = value;
    workspaceChangesQueueState.moveTarget = '';
  } else {
    workspaceChangesQueueState.moveTarget = value;
  }
  reconcileWorkspaceChangesQueueState();
  renderWorkspaceFilesChangesNext(interactionGeneration, field);
}

function installReviewWorkbenchNextVisibility() {
  const host = $('review-workbench-next');
  const visibleMount = projectionHasVisibleContinuity(
    reviewWorkbenchNextPending,
    reviewWorkbenchNextMounted,
  );
  const transitioning = Boolean(
    visibleMount
    && reviewWorkbenchNextPending.generation !== reviewWorkbenchNextMounted.generation
    && !projectionHasInteractionContinuity(
      reviewWorkbenchNextPending,
      reviewWorkbenchNextMounted,
    )
  );
  host.setAttribute('aria-busy', String(transitioning));
  host.classList.toggle('hidden', !visibleMount);
}

function installWorkspaceOverviewNextVisibility() {
  const host = $('workspace-overview-next');
  const exactMount = projectionHasVisibleContinuity(
    workspaceOverviewNextPending,
    workspaceOverviewNextMounted,
  );
  const transitioning = Boolean(
    exactMount
    && workspaceOverviewNextPending.generation !== workspaceOverviewNextMounted.generation
    && !projectionHasInteractionContinuity(
      workspaceOverviewNextPending,
      workspaceOverviewNextMounted,
    )
  );
  host.setAttribute('aria-busy', String(transitioning));
  // The outer React slot owns Preparing and failure states. Only a committed Overview (or its
  // same-workspace mounted continuity) may mark the assigned host ready and hide those states.
  host.classList.toggle('hidden', !exactMount);
}

function workspaceProjectionContinuityKey() {
  const workspace = model.workspace;
  if (!workspace) return 'workspace:none';
  if (typeof workspace.root !== 'string' || typeof workspace.installation !== 'string') return null;
  return `workspace:${workspace.root}\u0000${workspace.installation}`;
}

function reviewProjectionContinuityKey(bundle) {
  const workspaceKey = workspaceProjectionContinuityKey();
  return workspaceKey === null ? null : `${workspaceKey}\u0000review:${bundle}`;
}

function projectionHasVisibleContinuity(pending, mounted) {
  if (!pending || !mounted) return false;
  if (pending.generation === mounted.generation) return true;
  return typeof pending.continuityKey === 'string'
    && pending.continuityKey === mounted.continuityKey;
}

function projectionHasInteractionContinuity(pending, mounted) {
  if (!pending || !mounted) return false;
  if (pending.generation === mounted.generation) return true;
  if (pending.interactionKey === undefined && mounted.interactionKey === undefined) return true;
  return typeof pending.interactionKey === 'string'
    && pending.interactionKey === mounted.interactionKey;
}

function projectionAcceptsVisibleGeneration(generation, pending, mounted) {
  if (!pending || !mounted || !Number.isSafeInteger(generation)) return false;
  if (generation === pending.generation && generation === mounted.generation) return true;
  // A same-workspace refresh deliberately leaves the committed mounted React surface while its
  // replacement commits. Visual continuity and interaction continuity are separate: a changed
  // action set may remain painted without flashing the legacy fallback, but no intent from that
  // older presentation crosses into the new action semantics. The latest action map still decides
  // whether an otherwise identical action is enabled.
  return generation === mounted.generation
    && pending.generation !== mounted.generation
    && projectionHasVisibleContinuity(pending, mounted)
    && projectionHasInteractionContinuity(pending, mounted);
}

function projectionSurfaceContinuityKey(workspaceKey, surface, state) {
  return typeof workspaceKey === 'string'
    ? `${workspaceKey}\u0000${surface}:${JSON.stringify(state)}`
    : null;
}

function installWorkspaceCurrentNextVisibility() {
  const host = $('workspace-current-next');
  const visibleMount = projectionHasVisibleContinuity(
    workspaceCurrentNextPending,
    workspaceCurrentNextMounted,
  );
  const transitioning = Boolean(
    visibleMount
    && workspaceCurrentNextPending.generation !== workspaceCurrentNextMounted.generation
    && !projectionHasInteractionContinuity(
      workspaceCurrentNextPending,
      workspaceCurrentNextMounted,
    )
  );
  host.setAttribute('aria-busy', String(transitioning || currentRefreshInFlight > 0));
  host.classList.toggle('hidden', !visibleMount);
  // Current has no hidden controller fallback. Rejection leaves this host hidden so the route's
  // IslandSlot owns the explicit failure and Reload recovery surface.
}

function installWorkspaceWorkNextVisibility() {
  const exactMount = projectionHasVisibleContinuity(
    workspaceWorkNextPending,
    workspaceWorkNextMounted,
  );
  $('workspace-files-next').classList.toggle('hidden', !exactMount);
  $('workspace-changes-next').classList.toggle('hidden', !exactMount);
}

function installImportWorkbenchNextVisibility() {
  // The select control owns a one-shot focus handoff to the verified-review heading. Hiding its
  // host while that exact native preview is in flight makes browsers clear shadow-root focus and
  // prevents the review commit from proving ownership. Preserve only that accepted interaction;
  // every unrelated generation fails closed through the route's explicit IslandSlot fallback.
  const exactMount = importWorkbenchNextPending
    && importWorkbenchNextMounted
    && (importWorkbenchNextPending.generation === importWorkbenchNextMounted.generation
      || importWorkbenchNextPending.interactionGeneration === importWorkbenchNextMounted.generation);
  $('import-workbench-next').classList.toggle('hidden', !exactMount);
}

function installWorkspaceVersionsNextVisibility() {
  const visibleMount = projectionHasVisibleContinuity(
    workspaceVersionsNextPending,
    workspaceVersionsNextMounted,
  );
  $('workspace-versions-next').classList.toggle('hidden', !visibleMount);
}

function installWorkspaceChromeNextVisibility() {
  const exactMount = projectionHasVisibleContinuity(
    workspaceChromeNextPending,
    workspaceChromeNextMounted,
  );
  $('workspace-chrome-next').classList.toggle('hidden', !exactMount);
}

function installWorkspaceEntryNextVisibility() {
  const exactMount = projectionHasVisibleContinuity(
    workspaceEntryNextPending,
    workspaceEntryNextMounted,
  );
  $('workspace-entry-next').classList.toggle('hidden', !exactMount);
}

function installWorkspaceRestoreNextVisibility() {
  const visibleMount = projectionHasVisibleContinuity(
    workspaceRestoreNextPending,
    workspaceRestoreNextMounted,
  );
  $('workspace-restore-next').classList.toggle('hidden', !visibleMount);
}

function installWorkspaceDestinationNextVisibility() {
  const exactMount = projectionHasVisibleContinuity(
    workspaceDestinationNextPending,
    workspaceDestinationNextMounted,
  );
  $('workspace-destination-next').classList.toggle('hidden', !exactMount);
}

function dismissConfirmationNext(generation) {
  appDocument.dispatchEvent(new CustomEvent('mesh:confirmation-dismissed', {
    detail: Object.freeze({ generation }),
  }));
}

function settleConfirmationNext(generation, accepted) {
  const pending = confirmationNextPending;
  if (!pending || pending.generation !== generation) return false;
  if (pending.mountTimer !== null) clearTimeout(pending.mountTimer);
  confirmationNextPending = null;
  confirmationNextMounted = null;
  dismissConfirmationNext(generation);
  pending.resolve(accepted);
  return true;
}

function fallbackConfirmationNext(generation) {
  const pending = confirmationNextPending;
  if (!pending || pending.generation !== generation) return;
  if (!pending.isCurrent()) {
    settleConfirmationNext(generation, false);
    showNotice(pending.staleMessage, true);
    return;
  }
  const description = pending.description;
  if (pending.mountTimer !== null) clearTimeout(pending.mountTimer);
  confirmationNextPending = null;
  confirmationNextMounted = null;
  dismissConfirmationNext(generation);
  let accepted = false;
  try {
    accepted = confirm(description);
  } catch {
    accepted = false;
  }
  pending.resolve(accepted);
}

export function requestAccessibleConfirmation({
  title,
  description,
  confirmLabel,
  cancelLabel = 'Keep reviewing',
  tone = 'standard',
  isCurrent = () => true,
  staleMessage = 'This confirmation is no longer current. Review the latest workspace state and try again.',
}) {
  if (!isCurrent()) return Promise.resolve(false);
  // The confirmation island is intentionally dormant until an exact action requests it. Its
  // one-shot availability event can race this coordinator during cold WebKit startup, so recover
  // the same readiness from the durable host marker installed only after the island registered
  // both projection and dismissal listeners.
  if (!confirmationNextAvailable
    && $('confirmation-dialog-next')?.getAttribute('data-mesh-confirmation-ready') === 'true') {
    confirmationNextAvailable = true;
  }
  if (!confirmationNextAvailable) {
    try {
      return Promise.resolve(confirm(description));
    } catch {
      return Promise.resolve(false);
    }
  }
  if (confirmationNextPending) {
    settleConfirmationNext(confirmationNextPending.generation, false);
  }
  const generation = ++confirmationNextGeneration;
  return new Promise((resolve) => {
    confirmationNextMounted = null;
    confirmationNextPending = {
      generation,
      description,
      isCurrent,
      staleMessage,
      resolve,
      mountTimer: null,
    };
    appDocument.dispatchEvent(new CustomEvent('mesh:confirmation-projection', {
      detail: Object.freeze({
        generation,
        confirmation: Object.freeze({ title, description, confirmLabel, cancelLabel, tone }),
      }),
    }));
    if (confirmationNextPending?.generation === generation
      && confirmationNextMounted?.generation !== generation) {
      confirmationNextPending.mountTimer = setTimeout(
        () => fallbackConfirmationNext(generation),
        CONFIRMATION_MOUNT_TIMEOUT_MS,
      );
    }
  });
}

if (typeof appDocument.addEventListener === 'function') {
  appDocument.addEventListener('mesh:workspace-chrome-available', () => {
    workspaceChromeNextAvailable = true;
    renderNextAction();
  });
  appDocument.addEventListener('mesh:workspace-chrome-mounted', (event) => {
    const detail = event?.detail;
    if (detail?.generation !== workspaceChromeNextPending?.generation) return;
    workspaceChromeNextMounted = Object.freeze({
      generation: detail.generation,
      continuityKey: workspaceChromeNextPending.continuityKey,
    });
    installWorkspaceChromeNextVisibility();
  });
  appDocument.addEventListener('mesh:workspace-chrome-rejected', (event) => {
    if (event?.detail?.generation !== workspaceChromeNextPending?.generation) return;
    workspaceChromeNextMounted = null;
    installWorkspaceChromeNextVisibility();
  });
  appDocument.addEventListener('mesh:workspace-entry-available', () => {
    workspaceEntryNextAvailable = true;
    renderNextAction();
  });
  appDocument.addEventListener('mesh:workspace-entry-mounted', (event) => {
    const detail = event?.detail;
    if (detail?.generation !== workspaceEntryNextPending?.generation) return;
    workspaceEntryNextMounted = Object.freeze({
      generation: detail.generation,
      continuityKey: workspaceEntryNextPending.continuityKey,
    });
    installWorkspaceEntryNextVisibility();
  });
  appDocument.addEventListener('mesh:workspace-entry-rejected', (event) => {
    if (event?.detail?.generation !== workspaceEntryNextPending?.generation) return;
    workspaceEntryNextMounted = null;
    installWorkspaceEntryNextVisibility();
  });
  appDocument.addEventListener('mesh:workspace-entry-intent', (event) => {
    const detail = event?.detail;
    const actionKey = workspaceEntryActionKey(detail?.intent);
    if (!workspaceEntryNextPending || !workspaceEntryNextMounted || !actionKey) return;
    const exactGeneration = detail?.generation === workspaceEntryNextPending.generation
      && detail.generation === workspaceEntryNextMounted.generation;
    // React commits are asynchronous. Keep accepting field echoes from the one still-mounted
    // generation. Its form submission may follow only when it names the coordinator's exact latest
    // path; all other actions remain bound to the exact committed generation.
    const currentPathInteraction = detail?.generation === workspaceEntryNextMounted.generation
      && workspaceEntryNextPending.interactionGeneration === workspaceEntryNextMounted.generation
      && workspaceEntryNextPending.interactionKind === 'managed-path';
    const currentPathFieldEcho = actionKey === 'update-managed-path' && currentPathInteraction;
    const currentPathSubmit = actionKey === 'open-managed-path'
      && currentPathInteraction
      && detail.intent.path === workspaceEntryNextPending.openPath;
    // A recent selection also waits for React's replacement commit. Admit only Open/Forget for
    // the coordinator's exact newly selected option while the initiating generation remains
    // mounted; forged paths and every other action stay generation-bound.
    const currentRecentAction = (actionKey === `open-recent:${workspaceEntryNextPending.selectedRecentPath}`
        || actionKey === `forget-recent:${workspaceEntryNextPending.selectedRecentPath}`)
      && detail?.generation === workspaceEntryNextMounted.generation
      && workspaceEntryNextPending.interactionGeneration === workspaceEntryNextMounted.generation
      && workspaceEntryNextPending.interactionKind === 'recent-selection'
      && detail.intent.path === workspaceEntryNextPending.selectedRecentPath
      && workspaceEntryNextActions.has(actionKey);
    if (!exactGeneration && !currentPathFieldEcho && !currentPathSubmit && !currentRecentAction) return;
    const action = workspaceEntryNextActions.get(actionKey);
    if (typeof action === 'function') action(detail.intent, detail.generation);
  });
  appDocument.addEventListener('mesh:workspace-restore-available', () => {
    workspaceRestoreNextAvailable = true;
    renderRestoreNext();
  });
  appDocument.addEventListener('mesh:workspace-restore-mounted', (event) => {
    const detail = event?.detail;
    if (detail?.generation !== workspaceRestoreNextPending?.generation) return;
    const priorGeneration = workspaceRestoreNextMounted?.generation ?? null;
    const advanceInteraction = workspaceRestoreNextPending.interactionGeneration === priorGeneration
      && workspaceRestoreNextInteraction?.generation === priorGeneration
      && workspaceRestoreNextInteraction.owner?.isConnected
      && $('workspace-restore-next').shadowRoot?.activeElement === workspaceRestoreNextInteraction.owner;
    workspaceRestoreNextMounted = Object.freeze({
      generation: detail.generation,
      continuityKey: workspaceRestoreNextPending.continuityKey,
    });
    workspaceRestoreNextInteraction = advanceInteraction
      ? Object.freeze({
          generation: detail.generation,
          owner: workspaceRestoreNextInteraction.owner,
        })
      : null;
    installWorkspaceRestoreNextVisibility();
  });
  appDocument.addEventListener('mesh:workspace-restore-rejected', (event) => {
    if (event?.detail?.generation !== workspaceRestoreNextPending?.generation) return;
    workspaceRestoreNextMounted = null;
    workspaceRestoreNextInteraction = null;
    installWorkspaceRestoreNextVisibility();
  });
  appDocument.addEventListener('mesh:workspace-restore-intent', (event) => {
    const detail = event?.detail;
    const actionKey = workspaceRestoreActionKey(detail?.intent);
    if (!workspaceRestoreNextPending
      || !workspaceRestoreNextMounted
      || !actionKey) return;
    const exactGeneration = detail?.generation === workspaceRestoreNextPending.generation
      && detail.generation === workspaceRestoreNextMounted.generation;
    const interactionAction = actionKey === 'preview'
      || actionKey.startsWith('select-file:')
      || actionKey.startsWith('select-version:');
    const interactionGeneration = detail?.generation === workspaceRestoreNextMounted.generation
      && workspaceRestoreNextPending.interactionGeneration === workspaceRestoreNextMounted.generation
      && interactionAction;
    if (!exactGeneration && !interactionGeneration) return;
    const action = workspaceRestoreNextActions.get(actionKey);
    if (typeof action !== 'function') return;
    const owner = $('workspace-restore-next').shadowRoot?.activeElement ?? null;
    workspaceRestoreNextInteraction = owner?.isConnected
      && interactionAction
      ? Object.freeze({ generation: detail.generation, owner })
      : null;
    action();
  });
  appDocument.addEventListener('mesh:workspace-destination-available', () => {
    workspaceDestinationNextAvailable = true;
    renderExportChoices();
  });
  appDocument.addEventListener('mesh:workspace-destination-mounted', (event) => {
    const detail = event?.detail;
    if (detail?.generation !== workspaceDestinationNextPending?.generation) return;
    workspaceDestinationNextMounted = Object.freeze({
      generation: detail.generation,
      continuityKey: workspaceDestinationNextPending.continuityKey,
    });
    installWorkspaceDestinationNextVisibility();
  });
  appDocument.addEventListener('mesh:workspace-destination-rejected', (event) => {
    if (event?.detail?.generation !== workspaceDestinationNextPending?.generation) return;
    workspaceDestinationNextMounted = null;
    installWorkspaceDestinationNextVisibility();
  });
  appDocument.addEventListener('mesh:workspace-destination-intent', (event) => {
    const detail = event?.detail;
    const actionKey = workspaceDestinationActionKey(detail?.intent);
    if (!workspaceDestinationNextPending || !workspaceDestinationNextMounted || !actionKey) return;
    const exactGeneration = detail?.generation === workspaceDestinationNextPending.generation
      && detail.generation === workspaceDestinationNextMounted.generation;
    const currentFieldEcho = actionKey.startsWith('set-field:')
      && detail?.generation === workspaceDestinationNextMounted.generation
      && workspaceDestinationNextPending.interactionGeneration === workspaceDestinationNextMounted.generation
      && workspaceDestinationNextPending.interactionField === detail.intent.field;
    const currentPreviewEcho = (actionKey === 'activate:preview-single' || actionKey === 'activate:preview-all')
      && detail?.generation === workspaceDestinationNextMounted.generation
      && workspaceDestinationNextPending.interactionGeneration === workspaceDestinationNextMounted.generation
      && (workspaceDestinationNextPending.interactionField === 'selectedFile'
        || workspaceDestinationNextPending.interactionField === 'destination')
      && detail.intent.destination === workspaceDestinationDraft
      && (actionKey !== 'activate:preview-single'
        || detail.intent.selectedFile === workspaceDestinationSelectedFile);
    if (!exactGeneration && !currentFieldEcho && !currentPreviewEcho) return;
    const action = workspaceDestinationNextActions.get(actionKey);
    if (typeof action === 'function') action(detail.intent, detail.generation);
  });
  appDocument.addEventListener('mesh:confirmation-available', () => {
    confirmationNextAvailable = true;
  });
  appDocument.addEventListener('mesh:confirmation-mounted', (event) => {
    const detail = event?.detail;
    if (detail?.generation !== confirmationNextPending?.generation) return;
    confirmationNextMounted = Object.freeze({ generation: detail.generation });
    if (confirmationNextPending.mountTimer !== null) {
      clearTimeout(confirmationNextPending.mountTimer);
      confirmationNextPending.mountTimer = null;
    }
  });
  appDocument.addEventListener('mesh:confirmation-rejected', (event) => {
    if (event?.detail?.generation !== confirmationNextPending?.generation) return;
    fallbackConfirmationNext(event.detail.generation);
  });
  appDocument.addEventListener('mesh:confirmation-intent', (event) => {
    const detail = event?.detail;
    const intent = detail?.intent;
    if (!confirmationNextPending
      || !confirmationNextMounted
      || detail?.generation !== confirmationNextPending.generation
      || detail.generation !== confirmationNextMounted.generation
      || intent === null
      || typeof intent !== 'object'
      || Array.isArray(intent)
      || Object.keys(intent).length !== 1
      || (intent.type !== 'confirm' && intent.type !== 'cancel')) return;
    if (intent.type === 'confirm' && !confirmationNextPending.isCurrent()) {
      const { staleMessage } = confirmationNextPending;
      settleConfirmationNext(detail.generation, false);
      showNotice(staleMessage, true);
      return;
    }
    settleConfirmationNext(detail.generation, intent.type === 'confirm');
  });
  appDocument.addEventListener('mesh:import-workbench-available', () => {
    importWorkbenchNextAvailable = true;
    renderPreview();
  });
  appDocument.addEventListener('mesh:import-workbench-mounted', (event) => {
    const detail = event?.detail;
    if (detail?.generation !== importWorkbenchNextPending?.generation) return;
    const priorGeneration = importWorkbenchNextMounted?.generation ?? null;
    const advanceInteraction = importWorkbenchNextPending.phase === 'select'
      && importWorkbenchNextPending.interactionGeneration === priorGeneration
      && importWorkbenchNextInteraction?.generation === priorGeneration;
    importWorkbenchNextMounted = Object.freeze({ generation: detail.generation });
    if (advanceInteraction) {
      importWorkbenchNextInteraction = Object.freeze({
        ...importWorkbenchNextInteraction,
        generation: detail.generation,
      });
    } else if (importWorkbenchNextPending.phase === 'review') {
      importWorkbenchNextInteraction = null;
    }
    installImportWorkbenchNextVisibility();
  });
  appDocument.addEventListener('mesh:import-workbench-rejected', (event) => {
    if (event?.detail?.generation !== importWorkbenchNextPending?.generation) return;
    importWorkbenchNextMounted = null;
    importWorkbenchNextInteraction = null;
    installImportWorkbenchNextVisibility();
  });
  appDocument.addEventListener('mesh:import-workbench-intent', (event) => {
    const detail = event?.detail;
    const intent = detail?.intent;
    if (!importWorkbenchNextPending
      || !importWorkbenchNextMounted
      || intent === null
      || typeof intent !== 'object'
      || Array.isArray(intent)
      || typeof intent.type !== 'string') return;
    const exactGeneration = detail?.generation === importWorkbenchNextPending.generation
      && detail.generation === importWorkbenchNextMounted.generation;
    // React commits are asynchronous. A controlled destination input can emit more keystrokes
    // from the one still-mounted review generation after its first echo has already published a
    // replacement projection. Admit only that exact field while the coordinator state still
    // equals the authority captured for the pending projection. This also lets typing revoke an
    // unresolved native chooser without admitting stale Preview or Confirm actions.
    const retainedDestinationFieldEcho = intent.type === 'destination-draft'
      && detail?.generation === importWorkbenchNextMounted.generation
      && importWorkbenchNextPending.phase === 'review'
      && importWorkbenchNextPending.interactionGeneration === importWorkbenchNextMounted.generation
      && importPreviewStateAuthorityIsCurrent(importWorkbenchNextPending.destinationFieldEchoAuthority);
    if (!exactGeneration && !retainedDestinationFieldEcho) return;
    if (intent.type === 'source-draft') {
      const path = typeof intent.path === 'string' ? intent.path : '';
      if (Object.keys(intent).length !== 2
        || path.length > 4_096
        || /[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(path)
        || importWorkbenchNextPending.phase !== 'select') return;
      importSourceDraft = path;
      return;
    }
    if (intent.type === 'preview-path') {
      const path = typeof intent.path === 'string' ? intent.path : '';
      if (Object.keys(intent).length !== 2
        || !path
        || path.length > 4_096
        || /[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(path)
        || !importWorkbenchPresentation().canPreviewPath) return;
      importWorkbenchNextInteraction = Object.freeze({
        generation: detail.generation,
        selectionSequence: null,
      });
      importSourceDraft = path;
      void previewImportSourceDraft();
      return;
    }
    if (intent.type === 'destination-draft') {
      const path = typeof intent.path === 'string' ? intent.path : '';
      if (Object.keys(intent).length !== 2
        || path.length > 4_096
        || /[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(path)
        || importWorkbenchNextPending.phase !== 'review') return;
      workspaceSelectionSequence += 1;
      importDestinationChooserInFlight = null;
      model.destination = path || null;
      renderPreview(detail.generation);
      return;
    }
    if (Object.keys(intent).length !== 1) return;
    const action = importWorkbenchNextActions.get(intent.type);
    if (typeof action === 'function') {
      if (intent.type === 'choose-folder') {
        importWorkbenchNextInteraction = Object.freeze({
          generation: detail.generation,
          selectionSequence: null,
        });
      }
      action();
    }
  });
  appDocument.addEventListener('mesh:workspace-versions-available', () => {
    workspaceVersionsNextAvailable = true;
    renderWorkspaceVersionChoices();
  });
  appDocument.addEventListener('mesh:workspace-versions-mounted', (event) => {
    const detail = event?.detail;
    if (detail?.generation !== workspaceVersionsNextPending?.generation) return;
    const priorGeneration = workspaceVersionsNextMounted?.generation ?? null;
    const advanceInteraction = workspaceVersionsNextPending.interactionGeneration === priorGeneration
      && workspaceVersionsNextInteraction?.generation === priorGeneration
      && workspaceVersionsNextInteraction.owner?.isConnected
      && $('workspace-versions-next').shadowRoot?.activeElement === workspaceVersionsNextInteraction.owner;
    workspaceVersionsNextMounted = Object.freeze({
      generation: detail.generation,
      continuityKey: workspaceVersionsNextPending.continuityKey,
    });
    workspaceVersionsNextInteraction = advanceInteraction
      ? Object.freeze({
          generation: detail.generation,
          owner: workspaceVersionsNextInteraction.owner,
        })
      : null;
    installWorkspaceVersionsNextVisibility();
  });
  appDocument.addEventListener('mesh:workspace-versions-rejected', (event) => {
    if (event?.detail?.generation !== workspaceVersionsNextPending?.generation) return;
    workspaceVersionsNextMounted = null;
    workspaceVersionsNextInteraction = null;
    installWorkspaceVersionsNextVisibility();
  });
  appDocument.addEventListener('mesh:workspace-versions-intent', (event) => {
    const detail = event?.detail;
    const intent = detail?.intent;
    if (!workspaceVersionsNextPending
      || !workspaceVersionsNextMounted
      || !model.workspaceVerified) return;
    const actionKey = workspaceVersionsActionKey(intent);
    const exactGeneration = detail?.generation === workspaceVersionsNextPending.generation
      && detail.generation === workspaceVersionsNextMounted.generation;
    const interactionGeneration = detail?.generation === workspaceVersionsNextMounted.generation
      && workspaceVersionsNextPending.interactionGeneration === workspaceVersionsNextMounted.generation
      && workspaceVersionsRetainedInteraction(actionKey);
    if (!exactGeneration && !interactionGeneration) return;
    const action = workspaceVersionsNextActions.get(actionKey);
    if (typeof action !== 'function') {
      showNotice('That saved-version action is no longer available for this exact workspace. Refresh and inspect the current selection before trying again.', true);
      return;
    }
    const owner = $('workspace-versions-next').shadowRoot?.activeElement ?? null;
    workspaceVersionsNextInteraction = owner?.isConnected
      && workspaceVersionsRetainedInteraction(actionKey)
      ? Object.freeze({ generation: detail.generation, owner })
      : null;
    Promise.resolve()
      .then(() => action(intent))
      .catch((error) => showNotice(`Saved-version action unavailable: ${decodeDaemonRefusal(error).message || error}`, true));
  });
  appDocument.addEventListener('mesh:workspace-overview-available', () => {
    workspaceOverviewNextAvailable = true;
    renderNextAction();
  });
  appDocument.addEventListener('mesh:workspace-overview-mounted', (event) => {
    const detail = event?.detail;
    if (detail?.generation !== workspaceOverviewNextPending?.generation) return;
    workspaceOverviewNextMounted = Object.freeze({
      generation: detail.generation,
      continuityKey: workspaceOverviewNextPending.continuityKey,
      interactionKey: workspaceOverviewNextPending.interactionKey,
    });
    installWorkspaceOverviewNextVisibility();
  });
  appDocument.addEventListener('mesh:workspace-overview-rejected', (event) => {
    if (event?.detail?.generation !== workspaceOverviewNextPending?.generation) return;
    workspaceOverviewNextMounted = null;
    installWorkspaceOverviewNextVisibility();
  });
  appDocument.addEventListener('mesh:workspace-overview-intent', (event) => {
    const detail = event?.detail;
    const intent = detail?.intent;
    const exactGeneration = detail?.generation === workspaceOverviewNextPending?.generation
      && detail.generation === workspaceOverviewNextMounted?.generation;
    if (!exactGeneration
      || intent === null
      || typeof intent !== 'object'
      || Array.isArray(intent)
      || Object.keys(intent).length !== 1
      || typeof intent.type !== 'string') return;
    const action = workspaceOverviewNextActions.get(intent.type);
    if (typeof action === 'function') {
      Promise.resolve()
        .then(action)
        .catch((error) => showNotice(`Workspace action unavailable: ${decodeDaemonRefusal(error).message || error}`, true));
    }
  });
  appDocument.addEventListener('mesh:workspace-current-available', () => {
    workspaceCurrentNextAvailable = true;
    renderNextAction();
  });
  appDocument.addEventListener('mesh:workspace-current-mounted', (event) => {
    const detail = event?.detail;
    if (detail?.generation !== workspaceCurrentNextPending?.generation) return;
    workspaceCurrentNextMounted = Object.freeze({
      generation: detail.generation,
      continuityKey: workspaceCurrentNextPending.continuityKey,
      interactionKey: workspaceCurrentNextPending.interactionKey,
      projectionKey: workspaceCurrentNextPending.projectionKey,
    });
    installWorkspaceCurrentNextVisibility();
  });
  appDocument.addEventListener('mesh:workspace-current-rejected', (event) => {
    if (event?.detail?.generation !== workspaceCurrentNextPending?.generation) return;
    workspaceCurrentNextMounted = null;
    installWorkspaceCurrentNextVisibility();
  });
  appDocument.addEventListener('mesh:workspace-current-intent', (event) => {
    const detail = event?.detail;
    if (!projectionAcceptsVisibleGeneration(
      detail?.generation,
      workspaceCurrentNextPending,
      workspaceCurrentNextMounted,
    )) return;
    const action = workspaceCurrentNextActions.get(workspaceCurrentActionKey(detail.intent));
    if (typeof action === 'function') {
      Promise.resolve()
        .then(action)
        .catch((error) => showNotice(`Workspace action unavailable: ${decodeDaemonRefusal(error).message || error}`, true));
    }
  });
  appDocument.addEventListener('mesh:workspace-files-changes-available', () => {
    workspaceWorkNextAvailable = true;
    renderWorkspaceFilesChangesNext();
  });
  appDocument.addEventListener('mesh:workspace-files-changes-mounted', (event) => {
    const detail = event?.detail;
    if (detail?.generation !== workspaceWorkNextPending?.generation) return;
    workspaceWorkNextMounted = Object.freeze({
      generation: detail.generation,
      continuityKey: workspaceWorkNextPending.continuityKey,
    });
    installWorkspaceWorkNextVisibility();
  });
  appDocument.addEventListener('mesh:workspace-files-changes-rejected', (event) => {
    if (event?.detail?.generation !== workspaceWorkNextPending?.generation) return;
    workspaceWorkNextMounted = null;
    installWorkspaceWorkNextVisibility();
  });
  appDocument.addEventListener('mesh:workspace-files-changes-intent', (event) => {
    const detail = event?.detail;
    const kind = workspaceWorkIntentKind(detail.intent);
    if (!workspaceWorkNextPending || !workspaceWorkNextMounted || !kind) return;
    const exactGeneration = detail?.generation === workspaceWorkNextPending.generation
      && detail.generation === workspaceWorkNextMounted.generation;
    // React commits are asynchronous. Keep consecutive field edits from the currently mounted
    // workbench usable while its coordinator-authored echo is pending, but never extend that grace
    // to an older mounted view or a selection after another authority refresh.
    const currentFieldEcho = kind === 'set-field'
      && ['newPath', 'movePath', 'selectedFile', 'editorText', 'missingSource', 'moveTarget'].includes(detail.intent.field)
      && detail?.generation === workspaceWorkNextMounted.generation
      && workspaceWorkNextPending.interactionGeneration === workspaceWorkNextMounted.generation
      && workspaceWorkNextPending.interactionField === detail.intent.field;
    const actionEchoField = kind.startsWith('activate:')
      ? WORKSPACE_WORK_ACTION_ECHO_FIELDS[detail.intent.action]
      : null;
    const sourceOwnedActionEcho = ['newPath', 'movePath', 'selectedFile', 'editorText'].includes(actionEchoField);
    const sourceOwnedActionValue = actionEchoField === 'newPath' || actionEchoField === 'movePath'
      ? workspaceFilesState[actionEchoField]
      : workspaceChangesEditorState[actionEchoField];
    const actionEchoMatches = Boolean(
      detail.intent.field === actionEchoField
      && sourceOwnedActionEcho
      && sourceOwnedActionValue === detail.intent.value,
    );
    const currentActionAfterFieldEcho = kind.startsWith('activate:')
      && actionEchoMatches
      && detail?.generation === workspaceWorkNextMounted.generation
      && workspaceWorkNextPending.interactionGeneration === workspaceWorkNextMounted.generation
      && workspaceWorkNextPending.interactionField === actionEchoField
      && workspaceWorkNextActions.has(kind);
    const structuralActionEchoMatches = kind === 'activate:record-structural-change'
      && detail.intent.missingSource === workspaceChangesQueueState.missingSource
      && detail.intent.moveTarget === workspaceChangesQueueState.moveTarget;
    const currentStructuralActionAfterFieldEcho = structuralActionEchoMatches
      && detail?.generation === workspaceWorkNextMounted.generation
      && workspaceWorkNextPending.interactionGeneration === workspaceWorkNextMounted.generation
      && ['missingSource', 'moveTarget'].includes(workspaceWorkNextPending.interactionField)
      && workspaceWorkNextActions.has(kind);
    if (!exactGeneration && !currentFieldEcho && !currentActionAfterFieldEcho && !currentStructuralActionAfterFieldEcho) return;
    if (kind?.startsWith('activate:')) {
      if (kind === 'activate:record-structural-change' && !structuralActionEchoMatches) {
        renderWorkspaceFilesChangesNext();
        return;
      }
      if (detail.intent.field !== undefined && !actionEchoMatches) {
        if (sourceOwnedActionEcho) renderWorkspaceFilesChangesNext();
        return;
      }
      const action = workspaceWorkNextActions.get(kind);
      if (typeof action === 'function') {
        Promise.resolve()
          .then(action)
          .catch((error) => showNotice(`File action unavailable: ${decodeDaemonRefusal(error).message || error}`, true));
      }
      return;
    }
    if (kind === 'set-auto-save') {
      void updateNativeCapturePreference(detail.intent.checked);
      return;
    }
    if (kind !== 'set-field') return;
    applyWorkspaceWorkField(detail.intent.field, detail.intent.value, detail.generation);
  });
  appDocument.addEventListener('mesh:review-workbench-available', () => {
    reviewWorkbenchNextAvailable = true;
    renderReviews();
  });
  appDocument.addEventListener('mesh:review-workbench-mounted', (event) => {
    const detail = event?.detail;
    if (!reviewWorkbenchNextPending
      || detail?.generation !== reviewWorkbenchNextPending.generation
      || detail?.bundle !== reviewWorkbenchNextPending.bundle) return;
    reviewWorkbenchNextMounted = Object.freeze({
      generation: detail.generation,
      bundle: detail.bundle,
      continuityKey: reviewWorkbenchNextPending.continuityKey,
      interactionKey: reviewWorkbenchNextPending.interactionKey,
    });
    installReviewWorkbenchNextVisibility();
  });
  appDocument.addEventListener('mesh:review-workbench-rejected', (event) => {
    if (event?.detail?.generation !== reviewWorkbenchNextPending?.generation) return;
    reviewWorkbenchNextMounted = null;
    installReviewWorkbenchNextVisibility();
  });
  appDocument.addEventListener('mesh:review-workbench-intent', (event) => {
    const detail = event?.detail;
    const intent = detail?.intent;
    if (!reviewWorkbenchNextPending
      || !projectionAcceptsVisibleGeneration(
      detail?.generation,
      reviewWorkbenchNextPending,
      reviewWorkbenchNextMounted,
    )
      || detail?.bundle !== reviewWorkbenchNextPending.bundle
      || detail.bundle !== reviewWorkbenchNextMounted.bundle
      || intent === null
      || typeof intent !== 'object') return;
    const actionKey = reviewWorkbenchActionKey(intent);
    const action = reviewWorkbenchNextActions.get(actionKey);
    if (typeof action !== 'function') {
      showNotice('That review action is no longer available for this exact saved version. Refresh and inspect the current workspace before trying again.', true);
      return;
    }
    const generation = detail.generation;
    const bundle = detail.bundle;
    Promise.resolve()
      .then(() => {
        const exactGeneration = projectionAcceptsVisibleGeneration(
          generation,
          reviewWorkbenchNextPending,
          reviewWorkbenchNextMounted,
        );
        // Artifact rendering is a read-only request bound to an exact bundle, change, and page.
        // A periodic refresh may commit an identical action set after the click event is accepted
        // but before this microtask executes. Rebase only that read-only request onto the current
        // committed projection; mutating, exporting, and native-launch actions remain exact-
        // generation only.
        const sameReviewPreview = intent.type === 'load-artifact-preview'
          && reviewWorkbenchNextPending?.bundle === bundle
          && reviewWorkbenchNextMounted?.bundle === bundle
          && projectionHasVisibleContinuity(
            reviewWorkbenchNextPending,
            reviewWorkbenchNextMounted,
          )
          && projectionHasInteractionContinuity(
            reviewWorkbenchNextPending,
            reviewWorkbenchNextMounted,
          );
        if ((!exactGeneration && !sameReviewPreview)
          || reviewWorkbenchNextPending?.bundle !== bundle
          || reviewWorkbenchNextMounted?.bundle !== bundle) return undefined;
        const currentAction = reviewWorkbenchNextActions.get(actionKey);
        if (typeof currentAction !== 'function') return undefined;
        return currentAction(intent);
      })
      .catch((error) => showNotice(`Review action unavailable: ${decodeDaemonRefusal(error).message || error}`, true));
  });
}

class WorkspaceVerificationSuperseded extends Error {
  constructor() {
    super('The open workspace changed before this read completed. Review the current workspace and try again.');
  }
}

class ExportPreviewSuperseded extends Error {
  constructor() {
    super('A newer folder-update preview replaced this result.');
    this.name = 'ExportPreviewSuperseded';
  }
}

class ExportDestinationSelectionSuperseded extends Error {
  constructor() {
    super('A newer destination choice replaced this folder picker.');
    this.name = 'ExportDestinationSelectionSuperseded';
  }
}

class DaemonRefusal extends Error {
  constructor(code, message) {
    super(message);
    this.code = code;
  }
}

class ManagedWorkspaceOpenFailure extends Error {
  constructor(code, message) {
    super(message);
    this.name = 'ManagedWorkspaceOpenFailure';
    this.code = code;
  }
}

const MANAGED_WORKSPACE_REFUSAL_CODES = new Set([
  'workspace-unreachable',
  'workspace-payload-store-unreachable',
  'workspace-index-unavailable',
  'workspace-damaged',
  'workspace-nothing-readable',
  'workspace-contradictory',
]);

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
}

function managedWorkspaceOpenMessage(code, source, currentWorkspaceVerified) {
  const safety = currentWorkspaceVerified
    ? 'Your current workspace is still open and unchanged.'
    : 'No workspace was opened, and nothing changed.';
  if (source === 'recent') {
    if (code === 'workspace-unreachable') {
      return `Saved workspace unavailable\nMesh can’t open this recent workspace because its saved folder is no longer reachable.\n${safety}\nNext: restore the folder and choose Try again, or choose Forget from list to remove only this shortcut.`;
    }
    return `Saved workspace needs attention\nMesh found this recent workspace, but could not verify its private history.\n${safety}\nNext: leave the folder unchanged and choose Try again. If it still fails, copy diagnostics before repairing or removing anything.`;
  }
  if (code === 'workspace-unreachable') {
    return `This is not an available saved Mesh workspace\nMesh couldn’t find usable private history in the selected folder.\n${safety}\nNext: choose Import for an ordinary project folder, or select a saved workspace from Recent workspaces.`;
  }
  return `Saved workspace needs attention\nMesh found the selected workspace, but could not verify its private history.\n${safety}\nNext: leave the folder unchanged and try again. If it still fails, copy diagnostics before repairing or removing anything.`;
}

function decodeDaemonRefusal(error) {
  const encoded = typeof error === 'string' ? error : error?.message;
  if (typeof encoded === 'string') {
    try {
      const refusal = JSON.parse(encoded);
      if (
        refusal?.kind === 'mesh-daemon-refusal' &&
        typeof refusal.code === 'string' &&
        typeof refusal.message === 'string'
      ) {
        return new DaemonRefusal(refusal.code, refusal.message);
      }
    } catch {
      // Native/transport failures are intentionally not daemon refusals.
    }
  }
  return error instanceof Error ? error : new Error(String(error));
}

async function call(method, params = {}) {
  if (!invoke) throw new Error('This interface must run inside the Mesh desktop app.');
  try {
    return JSON.parse(await invoke('daemon_call', { method, paramsJson: JSON.stringify(params) }));
  } catch (error) {
    throw decodeDaemonRefusal(error);
  }
}

function showNotice(message, error = false) {
  return noticeSource.show(message, error);
}

const SUPPORT_BUNDLE_TOP_KEYS = Object.freeze([
  'schema',
  'producer',
  'workspace_correlation',
  'included',
  'excluded',
  'crash-diagnostics',
]);
const SUPPORT_BUNDLE_CRASH_KEYS = Object.freeze([
  'section',
  'serving',
  'severity',
  'saved_records',
  'boundary_bytes',
  'unfinished_bytes',
  'checkpoint_state_available',
  'meaningful_checkpoint_through',
  'recovery_preserved_through',
  'open_activity_from',
  'open_activity_through',
  'elapsed_ms',
  'sentence',
]);
const SUPPORT_BUNDLE_EXCLUDED = Object.freeze([
  'configuration',
  'event-ledger',
  'file-content',
  'key-material',
  'raw-paths',
]);
const SUPPORT_BUNDLE_MAX_BYTES = 65_536;
const U64_MAX = 18_446_744_073_709_551_615n;

function supportRecord(value, label) {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`${label} was not one object.`);
  }
  return value;
}

function supportExactKeys(value, expected, label) {
  const actual = Object.keys(supportRecord(value, label)).sort();
  const canonical = [...expected].sort();
  if (actual.length !== canonical.length
    || actual.some((key, index) => key !== canonical[index])) {
    throw new Error(`${label} contained unrecognized or missing fields.`);
  }
}

function supportUnsigned(value, label) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(`${label} was not a safe unsigned integer.`);
  }
  return value;
}

function supportSequence(value, label) {
  if (value === null) return null;
  if (typeof value !== 'string'
    || !/^[1-9][0-9]{0,19}$/u.test(value)
    || BigInt(value) > U64_MAX) {
    throw new Error(`${label} was not a canonical positive sequence.`);
  }
  return value;
}

function supportExactArray(value, expected, label) {
  if (!Array.isArray(value)
    || value.length !== expected.length
    || value.some((item, index) => item !== expected[index])) {
    throw new Error(`${label} did not match the privacy allowlist.`);
  }
  return [...value];
}

function supportSentence(crash) {
  const records = crash.saved_records;
  const allowed = [
    `Mesh started and your workspace is up to date. ${records} saved changes were read back.`,
    `Mesh started after an unexpected shutdown. Your workspace is up to date and ${records} saved changes were read back. One save had not finished when the shutdown happened and was set aside — nothing you were told had been saved privately is affected.`,
    'Mesh found an unfinished save in that folder and nothing finished before it, so it has not opened the folder and has changed nothing. This folder is not empty and Mesh will not treat it as though it were. Nothing you were told had been saved privately is missing, because nothing there had finished being saved. This folder needs attention before it can be used.',
    'Mesh could not open your workspace and has changed nothing. Your saved work is still on this device. Quit Mesh and start it again; if this message comes back, the workspace needs attention before it can be used.',
  ];
  if (typeof crash.sentence !== 'string' || !allowed.includes(crash.sentence)) {
    throw new Error('The diagnostic sentence was outside the fixed safe vocabulary.');
  }
  return crash.sentence;
}

// Reconstruct rather than forward the daemon object. Even though the local IPC response is
// already bounded, an older or replaced service must not smuggle an extra path, content field,
// configuration value, event, or key into a clipboard action intended for alpha feedback.
export function supportBundleClipboardText(value) {
  const bundle = supportRecord(value, 'support bundle');
  supportExactKeys(bundle, SUPPORT_BUNDLE_TOP_KEYS, 'support bundle');
  if (bundle.schema !== 'mesh-support-bundle/v1'
    || typeof bundle.workspace_correlation !== 'string'
    || !/^blake3:[0-9a-f]{64}$/u.test(bundle.workspace_correlation)) {
    throw new Error('The support bundle identity was malformed.');
  }
  const producer = supportRecord(bundle.producer, 'support producer');
  supportExactKeys(producer, ['component', 'version'], 'support producer');
  if (producer.component !== 'mesh-daemon'
    || typeof producer.version !== 'string'
    || !/^[0-9A-Za-z][0-9A-Za-z.+-]{0,63}$/u.test(producer.version)) {
    throw new Error('The support bundle producer was malformed.');
  }
  const included = supportExactArray(bundle.included, ['crash-diagnostics'], 'included support fields');
  const excluded = supportExactArray(bundle.excluded, SUPPORT_BUNDLE_EXCLUDED, 'excluded support fields');
  const crash = supportRecord(bundle['crash-diagnostics'], 'crash diagnostics');
  supportExactKeys(crash, SUPPORT_BUNDLE_CRASH_KEYS, 'crash diagnostics');
  if (crash.section !== 'crash-diagnostics'
    || typeof crash.serving !== 'boolean'
    || !['routine', 'notable', 'blocking'].includes(crash.severity)
    || typeof crash.checkpoint_state_available !== 'boolean') {
    throw new Error('The crash diagnostic state was malformed.');
  }
  const normalized = {
    schema: bundle.schema,
    producer: { component: producer.component, version: producer.version },
    workspace_correlation: bundle.workspace_correlation,
    included,
    excluded,
    'crash-diagnostics': {
      section: crash.section,
      serving: crash.serving,
      severity: crash.severity,
      saved_records: supportUnsigned(crash.saved_records, 'saved record count'),
      boundary_bytes: supportUnsigned(crash.boundary_bytes, 'durable boundary byte count'),
      unfinished_bytes: supportUnsigned(crash.unfinished_bytes, 'unfinished byte count'),
      checkpoint_state_available: crash.checkpoint_state_available,
      meaningful_checkpoint_through: supportSequence(crash.meaningful_checkpoint_through, 'meaningful checkpoint'),
      recovery_preserved_through: supportSequence(crash.recovery_preserved_through, 'recovery checkpoint'),
      open_activity_from: supportSequence(crash.open_activity_from, 'open activity start'),
      open_activity_through: supportSequence(crash.open_activity_through, 'open activity end'),
      elapsed_ms: supportUnsigned(crash.elapsed_ms, 'recovery duration'),
      sentence: supportSentence(crash),
    },
  };
  const encoded = JSON.stringify(normalized);
  if (new TextEncoder().encode(encoded).length > SUPPORT_BUNDLE_MAX_BYTES) {
    throw new Error('The safe diagnostic summary exceeded its byte limit.');
  }
  return encoded;
}

function supportBundleAvailable(workspace = model.workspace) {
  try {
    supportBundleClipboardText(workspace?.support_bundle);
    return true;
  } catch {
    return false;
  }
}

function workspaceInteractionInFlight() {
  return workspaceMutationInFlight || workspaceTransitionInFlight || nativeWorkspaceLaunchInFlight;
}

function beginWorkspaceTransition({ withinNativeWorkspaceLaunch = false } = {}) {
  if (
    workspaceMutationInFlight
    || workspaceTransitionInFlight
    || (nativeWorkspaceLaunchInFlight && !withinNativeWorkspaceLaunch)
  ) return false;
  workspaceTransitionInFlight = true;
  renderWorkspace();
  return true;
}

function finishWorkspaceTransition() {
  workspaceTransitionInFlight = false;
  renderWorkspace();
}

function beginNativeWorkspaceLaunch() {
  if (workspaceInteractionInFlight()) return false;
  nativeWorkspaceLaunchInFlight = true;
  // A launcher hands an exact native folder to another process. Freeze every workspace selector,
  // mutation, review, import, and export control until that handoff is known. Disabling only the
  // launcher buttons allows a recent-workspace or saved-version switch to race the folder being
  // handed to Codex, Terminal, Finder, or the clipboard.
  renderWorkspace();
  return true;
}

function finishNativeWorkspaceLaunch() {
  nativeWorkspaceLaunchInFlight = false;
  renderWorkspace();
}

function installBuildIdentity(recent) {
  const revision = typeof recent?.build_revision === 'string' ? recent.build_revision : '';
  if (recent?.build_exact === true && /^[0-9a-f]{40}$/.test(revision)) {
    setBuildIdentity(`Build ${revision.slice(0, 12)}`, `Exact source revision ${revision}`);
    return;
  }
  if (recent?.build_exact === false && revision === 'development') {
    setBuildIdentity('Development build', 'This build does not claim an exact source revision.');
    return;
  }
  setBuildIdentity('Build identity unavailable', 'Mesh refused a malformed or incomplete build identity.');
}

function installNavigationStatus(recent, workspaceRoot = model.workspace?.root ?? null) {
  installBuildIdentity(recent);
  model.recent = recent;
  const rememberedPaths = new Set(Array.isArray(recent?.workspaces) ? recent.workspaces : []);
  for (const path of unavailableRecentWorkspacePaths) {
    if (!rememberedPaths.has(path)) unavailableRecentWorkspacePaths.delete(path);
  }
  model.activeFolder = recent.active_folder
    ? { path: recent.active_folder, workspace_root: workspaceRoot, stable: true }
    : null;
  if (model.activeFolder && workspaceRoot && !model.agentFolder) {
    model.agentFolder = { path: workspaceRoot };
  }
  model.exportRoot = recent.export_root || null;
  stableNavigationRepairPending = false;
  workspaceDestinationDraft = model.exportRoot || '';
  // Version/recent-workspace switches commit native navigation after the new workspace snapshot
  // is installed. Re-read the exact handoff hint from that committed navigation so returning to a
  // previously used agent folder cannot briefly look like a first launch.
  restoreAgentHandoffForWorkspace();
}

function installAgentHandoffStatus(recent) {
  // Older native hosts and narrow test doubles predate the additive custody projection. Their
  // launch commands still enforce the durable native acquisition, so retain the current browser
  // hint until a host supplies the process-shared entries.
  if (!Array.isArray(recent?.workspace_entries)) return;
  // The bounded recent-workspace order and custody entries are process-shared. Replace them
  // together when the native host supplies one internally consistent record snapshot, while this
  // window's selected workspace, stable navigation, and typed export destination remain local.
  // Older native hosts exposed only workspace_entries, so their additive custody projection stays
  // compatible without letting a partial workspaces shape erase known navigation.
  const sharedNavigation = Array.isArray(recent.workspaces)
    && recent.workspaces.every((path) => typeof path === 'string' && path)
    && (recent.remembered === null || typeof recent.remembered === 'string')
    && (
      (recent.workspaces.length === 0 && recent.remembered === null)
      || recent.remembered === recent.workspaces[0]
    )
    ? { remembered: recent.remembered, workspaces: recent.workspaces }
    : {};
  model.recent = {
    ...(model.recent || {}),
    ...sharedNavigation,
    workspace_entries: recent.workspace_entries,
  };
  restoreAgentHandoffForWorkspace();
}

function restoreAgentHandoffForWorkspace(
  workspace = model.workspace,
  { preserveCurrent = false } = {},
) {
  if (!workspace || !model.workspaceVerified) return;
  // Workspace state and recent-workspace navigation are separate native authorities. A launcher
  // persists custody before it starts the external process, then the webview records that exact
  // result immediately. Reinstalling an ordinary workspace snapshot must not clear that newer
  // handoff from an older navigation snapshot. A fresh installNavigationStatus call still enters
  // this function without preserveCurrent and can authoritatively clear or replace the handoff.
  if (
    preserveCurrent
    && model.agentHandoff?.root === workspace.root
    && model.agentHandoff?.installation === workspace.installation
  ) return;
  const entry = recentWorkspaceEntries().find((candidate) => candidate.path === workspace.root);
  const previousHandoff = model.agentHandoff;
  model.agentHandoff = entry?.agentHandoffInstallation === workspace.installation
    ? {
      root: workspace.root,
      installation: workspace.installation,
      generation: entry.agentHandoffGeneration,
    }
    : null;
  if (!model.agentHandoff
    || previousHandoff?.root !== model.agentHandoff.root
    || previousHandoff?.installation !== model.agentHandoff.installation
    || previousHandoff?.generation !== model.agentHandoff.generation) {
    model.agentLive = null;
    agentLiveInspectionSequence += 1;
  }
}

async function rememberWorkspace(path, exportRoot = null, originalUpdateVersion = null) {
  model.activeFolder = null;
  model.exportRoot = null;
  workspaceDestinationDraft = '';
  try {
    const parameters = { path, exportRoot };
    if (originalUpdateVersion !== null) parameters.originalUpdateVersion = originalUpdateVersion;
    installNavigationStatus(
      JSON.parse(await invoke('remember_managed_workspace', parameters)),
      path,
    );
    return model.recent.warning || null;
  } catch (error) {
    const navigationWarning = await recoverRememberedWorkspaceFromNative(path);
    return `The workspace is open, but Mesh could not finish its stable-folder navigation state: ${error}`
      + (navigationWarning ? ` ${navigationWarning}` : '');
  }
}

async function recoverRememberedWorkspaceFromNative(path) {
  const warnings = [];
  const navigationWarning = await reconcileNavigationForVerifiedWorkspace(path);
  if (navigationWarning) warnings.push(navigationWarning);
  try {
    const recent = JSON.parse(await invoke('recent_workspace_status'));
    if (
      recent.remembered !== path
      || !Array.isArray(recent.workspaces)
      || !recent.workspaces.includes(path)
    ) {
      warnings.push('Mesh could not confirm that recent-workspace history recorded the open workspace.');
    } else {
      model.recent = recent;
      model.exportRoot = recent.export_root || null;
      workspaceDestinationDraft = model.exportRoot || '';
    }
  } catch (error) {
    warnings.push(`Mesh could not read back recent-workspace history: ${error}`);
  }
  return warnings.join(' ') || null;
}

async function reconcileNavigationForVerifiedWorkspace(path) {
  const binding = captureVerifiedWorkspace();
  if (binding.root !== path) {
    model.activeFolder = null;
    stableNavigationRepairPending = true;
    markWorkspaceUnverified();
    return 'Mesh could not bind the stable folder because the verified workspace changed.';
  }
  try {
    const navigation = JSON.parse(await invoke('reconcile_managed_workspace_navigation', {
      expectedWorkspaceRoot: binding.root,
      expectedWorkspaceDigest: binding.digest,
      expectedWorkspaceInstallation: binding.installation,
    }));
    assertVerifiedWorkspace(binding);
    if (navigation.workspace_root !== binding.root) {
      throw new Error('Mesh reconciled the stable native folder to a different workspace.');
    }
    model.activeFolder = navigation.path ? navigation : null;
    stableNavigationRepairPending = false;
    return null;
  } catch (error) {
    model.activeFolder = null;
    stableNavigationRepairPending = true;
    markWorkspaceUnverified();
    return `Mesh could not confirm the stable folder for the verified workspace: ${error}`;
  }
}

async function forgetWorkspace(path) {
  try {
    installNavigationStatus(JSON.parse(await invoke('forget_managed_workspace', { path })));
    return model.recent.warning || null;
  } catch (error) {
    const recovered = await recoverForgottenWorkspaceFromNative(path);
    if (recovered.confirmed) {
      return `Mesh lost the first recent-workspace reply but confirmed the entry was removed.${recovered.warning ? ` ${recovered.warning}` : ''}`;
    }
    return `Mesh could not confirm removal of the recent-workspace entry: ${error}${recovered.warning ? ` ${recovered.warning}` : ''}`;
  }
}

async function recoverForgottenWorkspaceFromNative(path) {
  try {
    const recent = JSON.parse(await invoke('recent_workspace_status'));
    if (!Array.isArray(recent.workspaces)) {
      throw new Error('recent-workspace history did not return its bounded workspace list');
    }
    installNavigationStatus(recent);
    return {
      confirmed: !recent.workspaces.includes(path),
      warning: typeof recent.warning === 'string' ? recent.warning : null,
    };
  } catch (error) {
    model.activeFolder = null;
    model.exportRoot = null;
    stableNavigationRepairPending = true;
    workspaceDestinationDraft = '';
    return {
      confirmed: false,
      warning: `Mesh could not read back recent-workspace history: ${error}`,
    };
  }
}

function formatBytes(value) {
  return new Intl.NumberFormat(undefined, { notation: value > 999999 ? 'compact' : 'standard', maximumFractionDigits: 1 }).format(value);
}

function exactByteCount(value) {
  if (typeof value !== 'string' || !/^(0|[1-9][0-9]*)$/.test(value)) {
    throw new Error('The saved workspace preview returned an invalid byte count.');
  }
  return BigInt(value);
}

function validateImportFilePreview(preview) {
  const hasEntries = preview?.file_entries !== undefined;
  const hasOmitted = preview?.files_not_listed !== undefined;
  // This shape is part of the confirmation ceremony, not optional decoration. An older process
  // can advertise the same IPC surface after an application update; omitting both fields must not
  // silently downgrade a human-reviewable preview back to an opaque count and digest.
  if (
    !hasEntries
    || !hasOmitted
    || !Array.isArray(preview.file_entries)
    || preview.file_entries.length > 24
    || !Number.isSafeInteger(preview.files_not_listed)
    || preview.files_not_listed < 0
    || !Number.isSafeInteger(preview.files)
    || preview.files < 0
    || preview.file_entries.length + preview.files_not_listed !== preview.files
  ) {
    throw new Error('Mesh returned an incomplete import file preview. Nothing was copied.');
  }
  let previousPath = null;
  for (const entry of preview.file_entries) {
    if (
      !entry
      || typeof entry.path !== 'string'
      || !entry.path
      || typeof entry.executable !== 'boolean'
    ) {
      throw new Error('Mesh returned an invalid import file preview. Nothing was copied.');
    }
    exactByteCount(entry.bytes);
    if (previousPath !== null && entry.path <= previousPath) {
      throw new Error('Mesh returned an unordered import file preview. Nothing was copied.');
    }
    previousPath = entry.path;
  }
  return preview.file_entries;
}

function managedName(source, summary) {
  const leaf = source.split('/').filter(Boolean).at(-1) || 'workspace';
  return `${leaf}-mesh-${summary.slice(0, 8)}`;
}

function managedMutationRecoveryBlocked() {
  return Boolean(model.workspace?.conditions?.some((item) => [
    'managed-mutation-recovery-needed',
    'checkpoint-recovery-needs-attention',
  ].includes(item.code)));
}

function managedMutationBlocked() {
  return !model.workspaceVerified || managedMutationRecoveryBlocked();
}

function managedWorkspaceWriteBlocked() {
  // A remembered ordinary folder is opened only so Mesh can inspect and import it. Until import
  // creates an app-owned native workspace, this source must not become a managed mutation target:
  // enabling Files, Save, Update original, or Rollback here contradicts the first-run guarantee
  // that the original folder stays untouched.
  // A failed native inspection is equally important: one unreadable candidate may contain the
  // just-finished agent's only complete work. Ordinary Refresh verifies daemon state but does not
  // re-read every native byte, so only a later successful full scan may clear this boundary.
  return managedMutationBlocked()
    || rememberedWorkspaceNeedsImport(model.workspace)
    || model.workspace?.native_inventory_complete === false
    || model.nativeInspectionFailed;
}

function managedWorkspaceMutationBlocked() {
  return managedWorkspaceWriteBlocked() || workspaceInstallationMatchesHandoff();
}

function activeAgentMutationRefusal() {
  return 'This folder is still assigned to an agent. After every process using it stops, choose Finish agent handoff before changing, saving, restoring, reviewing, approving, or returning its complete result.';
}

function shortIdentity(value) {
  return value ? `${String(value).slice(0, 12)}…` : 'Unavailable';
}

function reviewContentSummary(summary) {
  if (!summary) return 'not present';
  const parts = [];
  if (summary.kind === 'binary' && summary.byte_length !== null) {
    parts.push(`${summary.byte_length} bytes`);
  } else if (summary.kind === 'text' && summary.line_count !== null) {
    parts.push(`${summary.line_count} lines`);
  } else {
    parts.push(summary.kind || 'unknown content');
  }
  if (summary.content_digest) parts.push(`content ${summary.content_digest}`);
  if (summary.version_id) parts.push(`version ${summary.version_id}`);
  return parts.join(' · ');
}

function reviewIdentityMatches(identity, summary) {
  return identity
    && summary
    && identity.version_id === summary.version_id
    // Text summaries deliberately carry the signed version identity and line count, not a second
    // content digest. The verified text projection repeats its independently reconstructed digest,
    // while binary summaries carry both identities. Bind every field the signed summary names
    // without making real text projections impossible to render.
    && (summary.content_digest === null || identity.content_digest === summary.content_digest);
}

function reviewDiffTextIsSafe(text) {
  return !/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(text);
}

export function validatedReviewTextDiff(change) {
  const preview = change.verified_text;
  if (!preview) return { error: null, kind: 'unavailable', hunks: [] };
  let kind = 'diff';
  if (preview.source === 'after') {
    if (!reviewIdentityMatches(preview, change.after)) {
      return { error: 'Content identity mismatch', kind: 'unavailable', hunks: [] };
    }
    kind = 'legacy-snapshot';
  } else if (preview.source === 'before-after') {
    const beforeMatches = change.before === null
      ? preview.before === null
      : reviewIdentityMatches(preview.before, change.before);
    const afterMatches = change.after === null
      ? preview.after === null
      : reviewIdentityMatches(preview.after, change.after);
    if (!beforeMatches || !afterMatches) {
      return { error: 'Content identity mismatch', kind: 'unavailable', hunks: [] };
    }
  } else {
    return { error: 'Unknown verified text format', kind: 'unavailable', hunks: [] };
  }
  if (!Array.isArray(preview.hunks) || preview.hunks.length === 0 || preview.hunks.length > 128) {
    return { error: 'Invalid bounded diff', kind: 'unavailable', hunks: [] };
  }
  let renderedLines = 0;
  let renderedCharacters = 0;
  for (const hunk of preview.hunks) {
    if (!hunk || !Array.isArray(hunk.lines) || hunk.lines.length > 4096 - renderedLines) {
      return { error: 'Invalid bounded diff', kind: 'unavailable', hunks: [] };
    }
    const lengths = [hunk.before_len, hunk.after_len];
    const starts = [hunk.before_start, hunk.after_start];
    if (!lengths.every((value) => Number.isSafeInteger(value) && value >= 0 && value <= 4096)
      || !starts.every((value) => Number.isSafeInteger(value) && value >= 1 && value <= 4096)) {
      return { error: 'Invalid bounded diff', kind: 'unavailable', hunks: [] };
    }
    let nextBefore = hunk.before_start;
    let nextAfter = hunk.after_start;
    for (const line of hunk.lines) {
      if (!line
        || !['context', 'removed', 'added'].includes(line.kind)
        || typeof line.text !== 'string'
        || !reviewDiffTextIsSafe(line.text)) {
        return { error: 'Invalid bounded diff', kind: 'unavailable', hunks: [] };
      }
      renderedCharacters += line.text.length;
      if (renderedCharacters > 512 * 1024) {
        return { error: 'Invalid bounded diff', kind: 'unavailable', hunks: [] };
      }
      const beforeValid = line.kind === 'added'
        ? line.before === null
        : line.before === nextBefore;
      const afterValid = line.kind === 'removed'
        ? line.after === null
        : line.after === nextAfter;
      if (!beforeValid || !afterValid) {
        return { error: 'Invalid bounded diff', kind: 'unavailable', hunks: [] };
      }
      if (line.kind !== 'added') nextBefore += 1;
      if (line.kind !== 'removed') nextAfter += 1;
      renderedLines += 1;
    }
    if (nextBefore !== hunk.before_start + hunk.before_len
      || nextAfter !== hunk.after_start + hunk.after_len) {
      return { error: 'Invalid bounded diff', kind: 'unavailable', hunks: [] };
    }
  }
  return { error: null, kind, hunks: preview.hunks };
}

const REVIEW_ARTIFACT_KINDS = Object.freeze({
  pdf: 'pdf',
  pptx: 'presentation',
  docx: 'document',
  xlsx: 'spreadsheet',
  png: 'image',
  jpg: 'image',
  jpeg: 'image',
  gif: 'image',
  webp: 'image',
});

export function reviewArtifactKind(change) {
  const paths = [change.path_after, change.path_before].filter((path) => typeof path === 'string');
  const kinds = paths.map((path) => {
    const name = path.split('/').at(-1) || '';
    const extension = name.includes('.') ? name.split('.').at(-1).toLowerCase() : '';
    return REVIEW_ARTIFACT_KINDS[extension] || null;
  });
  return kinds.length > 0 && kinds.every((kind) => kind !== null && kind === kinds[0])
    ? kinds[0]
    : null;
}

function documentSectionIdentity(label) {
  if (label === 'Document' || label === 'Document opening') return label;
  const heading = /^Section ([1-9][0-9]*) · /u.exec(label);
  return heading ? `Section ${heading[1]}` : null;
}

function documentSectionsValid(sections) {
  if (sections.length === 1 && sections[0].label === 'Document') return true;
  const opening = sections[0]?.label === 'Document opening';
  if (opening && sections.length === 1) return false;
  return sections.slice(opening ? 1 : 0).every(
    (section, index) => section.label.startsWith(`Section ${index + 1} · `),
  );
}

export function validatedReviewArtifactPreview(answer, change, side, kind) {
  const summary = side === 'before' ? change.before : side === 'after' ? change.after : null;
  const path = side === 'before' ? change.path_before : change.path_after;
  const pdfPage = kind === 'pdf'
    && answer?.renderer === 'macos-pdfkit-page-v1'
    && answer?.scope === 'exact-page-preview'
    && Number.isSafeInteger(answer?.page_number)
    && answer.page_number >= 1
    && Number.isSafeInteger(answer?.page_count)
    && answer.page_count >= answer.page_number
    && answer.page_count <= PDF_DOCUMENT_PAGE_LIMIT;
  const representative = kind !== 'pdf'
    && answer?.renderer === (kind === 'image'
      ? 'macos-imageio-thumbnail-v1'
      : 'macos-quick-look-thumbnail')
    && answer?.scope === 'representative-preview'
    && answer?.page_number === null
    && answer?.page_count === null;
  if (
    !summary
    || summary.kind !== 'binary'
    || !/^[0-9a-f]{64}$/u.test(summary.version_id || '')
    || !/^[0-9a-f]{64}$/u.test(summary.content_digest || '')
    || typeof path !== 'string'
    || reviewArtifactKind({ [`path_${side}`]: path }) !== kind
    || !answer
    || (!pdfPage && !representative)
    || answer.kind !== kind
    || answer.side !== side
    || answer.version_id !== summary.version_id
    || answer.content_digest !== summary.content_digest
    || answer.rendering_authorizes_approval !== false
    || typeof answer.image_data_url !== 'string'
    || answer.image_data_url.length > 16 * 1024 * 1024
    || !/^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/u.test(answer.image_data_url)
  ) {
    throw new Error('The visual preview did not match the exact reviewed artifact.');
  }
  const noText = answer.text_source === null
    && answer.text_lines === null
    && answer.text_sections === null
    && answer.text_truncated === false;
  const textLines = Array.isArray(answer.text_lines) ? answer.text_lines : [];
  const textSections = Array.isArray(answer.text_sections) ? answer.text_sections : [];
  let expectedLineStart = 0;
  const sectionsValid = textSections.length > 0
    && textSections.length <= 64
    && textSections.every((section) => {
      const valid = section !== null
        && typeof section === 'object'
        && !Array.isArray(section)
        && Object.keys(section).sort().join(',') === 'label,line_count,line_start'
        && typeof section.label === 'string'
        && section.label.length > 0
        // Word headings are truncated by Unicode scalar value in Rust. Astral characters occupy
        // two UTF-16 units here, so the longest valid native `Section N · …` label needs 128.
        && section.label.length <= 128
        && reviewDiffTextIsSafe(section.label)
        && Number.isSafeInteger(section.line_start)
        && section.line_start === expectedLineStart
        && Number.isSafeInteger(section.line_count)
        && section.line_count > 0
        && section.line_count <= 512;
      if (valid) expectedLineStart += section.line_count;
      return valid;
    })
    && expectedLineStart === textLines.length;
  const textSourceValid = answer.text_source === 'macos-quick-look-visible-text'
    || (kind === 'pdf' && answer.text_source === 'macos-pdfkit-page-text-v1')
    || (kind === 'presentation' && answer.text_source === 'mesh-pptx-slide-text-v1')
    || (kind === 'document' && answer.text_source === 'mesh-docx-block-text-v1')
    || (kind === 'spreadsheet' && answer.text_source === 'mesh-xlsx-cell-formula-v1');
  const textStructureValid = (answer.text_source !== 'macos-pdfkit-page-text-v1'
      || (textSections.length === 1 && textSections[0].label === `Page ${answer.page_number}`))
    && (answer.text_source !== 'mesh-pptx-slide-text-v1'
      || textSections.every((section, index) => section.label === `Slide ${index + 1}`
        || section.label.startsWith(`Slide ${index + 1} · `)))
    && (answer.text_source !== 'mesh-docx-block-text-v1'
      || documentSectionsValid(textSections))
    && (answer.text_source !== 'mesh-xlsx-cell-formula-v1'
      || new Set(textSections.map((section) => section.label)).size === textSections.length);
  const hasText = textSourceValid
    && textStructureValid
    && textLines.length > 0
    && textLines.length <= 512
    && sectionsValid
    && typeof answer.text_truncated === 'boolean'
    && textLines.every((line) => typeof line === 'string'
      && line.length > 0
      && reviewDiffTextIsSafe(line))
    && textLines.reduce((total, line) => total + line.length, 0) <= 128 * 1024;
  if (!noText && !hasText) {
    throw new Error('The extracted artifact text was malformed or unbounded.');
  }
  return answer;
}

export function validatedReviewInspectionExport(answer, change, requestedSides) {
  const expectedSides = [...requestedSides].sort();
  if (!answer
    || typeof answer !== 'object'
    || Array.isArray(answer)
    || Object.keys(answer).sort().join(',')
      !== 'directory,document_content_opened,files,finder_opened,schema,warning,working_folder_unchanged'
    || answer.schema !== 'mesh-review-inspection-export/v1'
    || typeof answer.directory !== 'string'
    || !answer.directory.startsWith('/')
    || answer.working_folder_unchanged !== true
    || answer.document_content_opened !== false
    || typeof answer.finder_opened !== 'boolean'
    || !(answer.warning === null || (typeof answer.warning === 'string' && answer.warning.length > 0))
    || !Array.isArray(answer.files)
    || answer.files.length !== expectedSides.length) {
    throw new Error('The exact-copy export result was malformed.');
  }
  const actualSides = [];
  const paths = new Set();
  for (const file of answer.files) {
    if (!file
      || typeof file !== 'object'
      || Array.isArray(file)
      || Object.keys(file).sort().join(',')
        !== 'content_digest,path,side,version_id'
      || !['before', 'after'].includes(file.side)
      || typeof file.path !== 'string'
      || !file.path.startsWith(`${answer.directory}/`)
      || paths.has(file.path)) {
      throw new Error('The exact-copy export did not describe distinct reviewed files.');
    }
    const summary = file.side === 'before' ? change.before : change.after;
    if (!summary
      || summary.kind !== 'binary'
      || file.version_id !== summary.version_id
      || file.content_digest !== summary.content_digest) {
      throw new Error('The exact-copy export did not match the reviewed versions.');
    }
    actualSides.push(file.side);
    paths.add(file.path);
  }
  if (actualSides.sort().join(',') !== expectedSides.join(',')) {
    throw new Error('The exact-copy export returned the wrong reviewed sides.');
  }
  return answer;
}

function artifactTextRows(preview) {
  if (!preview) return [];
  if (!Array.isArray(preview.text_lines)
    || preview.text_lines.length > 512
    || !Array.isArray(preview.text_sections)
    || preview.text_sections.length > 64) return null;
  const rows = [];
  let expectedLineStart = 0;
  for (const [index, section] of preview.text_sections.entries()) {
    if (!section
      || typeof section.label !== 'string'
      || !Number.isSafeInteger(section.line_start)
      || section.line_start !== expectedLineStart
      || !Number.isSafeInteger(section.line_count)
      || section.line_count <= 0
      || section.line_start + section.line_count > preview.text_lines.length) return null;
    const sectionIdentity = preview.text_source === 'mesh-pptx-slide-text-v1'
      ? `Slide ${index + 1}`
      : preview.text_source === 'mesh-docx-block-text-v1'
        ? documentSectionIdentity(section.label)
        : section.label;
    if (!sectionIdentity) return null;
    rows.push({ key: `section\u0000${sectionIdentity}`, text: sectionIdentity, section: true, number: null });
    if (section.label !== sectionIdentity) {
      rows.push({
        key: `text\u0000${sectionIdentity}\u0000title\u0000${section.label}`,
        text: section.label,
        section: false,
        number: null,
      });
    }
    for (let offset = 0; offset < section.line_count; offset += 1) {
      const number = section.line_start + offset + 1;
      const text = preview.text_lines[number - 1];
      // A repeated sentence or cell value in another slide/sheet is not the same review row.
      // Binding the LCS key to its structural parent prevents cross-section matches from hiding
      // a moved, inserted, renamed, or removed document section.
      rows.push({ key: `text\u0000${sectionIdentity}\u0000${text}`, text, section: false, number });
    }
    expectedLineStart += section.line_count;
  }
  return expectedLineStart === preview.text_lines.length ? rows : null;
}

function artifactTextSection(preview, choice, kind) {
  if (!preview || choice === null) return preview;
  const sections = Array.isArray(preview.text_sections) ? preview.text_sections : [];
  const section = kind === 'presentation'
    ? sections[choice.index]
    : sections.find((candidate) => (
        kind === 'document'
          ? documentSectionIdentity(candidate.label) === choice.label
          : candidate.label === choice.label
      ));
  if (!section) return null;
  const lines = preview.text_lines.slice(
    section.line_start,
    section.line_start + section.line_count,
  );
  return {
    ...preview,
    text_lines: lines,
    text_sections: [{ label: section.label, line_start: 0, line_count: lines.length }],
  };
}

export function artifactTextSectionChoices(beforePreview, afterPreview, kind) {
  if (!['presentation', 'document', 'spreadsheet'].includes(kind)) return [];
  const before = Array.isArray(beforePreview?.text_sections) ? beforePreview.text_sections : [];
  const after = Array.isArray(afterPreview?.text_sections) ? afterPreview.text_sections : [];
  if (kind === 'presentation') {
    return Array.from({ length: Math.max(before.length, after.length) }, (_, index) => ({
      key: `presentation:${index + 1}`,
      index,
      label: after[index]?.label || before[index]?.label || `Slide ${index + 1}`,
      before_present: Boolean(before[index]),
      after_present: Boolean(after[index]),
    }));
  }
  const sectionLabel = (section) => kind === 'document'
    ? documentSectionIdentity(section.label)
    : section.label;
  const labels = [];
  const seen = new Set();
  for (const section of [...before, ...after]) {
    const label = sectionLabel(section);
    if (!label || seen.has(label)) continue;
    seen.add(label);
    labels.push(label);
  }
  return labels.map((label, index) => ({
    key: `${kind}:${label}`,
    index,
    label,
    before_present: before.some((section) => sectionLabel(section) === label),
    after_present: after.some((section) => sectionLabel(section) === label),
  }));
}

function compactArtifactTextHunks(lines) {
  const changed = lines.flatMap((line, index) => line.kind === 'context' ? [] : [index]);
  if (changed.length === 0) {
    return [{
      before_start: 1,
      before_len: lines.filter((line) => line.before !== null).length,
      after_start: 1,
      after_len: lines.filter((line) => line.after !== null).length,
      lines,
    }];
  }
  const ranges = changed.map((index) => {
    let start = Math.max(0, index - 3);
    for (let prior = index; prior >= 0; prior -= 1) {
      if (lines[prior].section) {
        start = Math.min(start, prior);
        break;
      }
    }
    return { start, end: Math.min(lines.length, index + 4) };
  });
  const merged = [];
  for (const range of ranges) {
    const prior = merged.at(-1);
    if (prior && range.start <= prior.end) prior.end = Math.max(prior.end, range.end);
    else merged.push({ ...range });
  }
  return merged.map(({ start, end }) => {
    const selected = lines.slice(start, end);
    return {
      before_start: selected.find((line) => line.before !== null)?.before || 0,
      before_len: selected.filter((line) => line.before !== null).length,
      after_start: selected.find((line) => line.after !== null)?.after || 0,
      after_len: selected.filter((line) => line.after !== null).length,
      lines: selected,
    };
  });
}

export function artifactTextDiff(beforePreview, afterPreview) {
  const beforeRows = artifactTextRows(beforePreview);
  const afterRows = artifactTextRows(afterPreview);
  if (!beforeRows || !afterRows || beforeRows.length > 576 || afterRows.length > 576) return null;
  const width = afterRows.length + 1;
  const matrix = new Uint16Array((beforeRows.length + 1) * width);
  const at = (before, after) => before * width + after;
  for (let before = beforeRows.length - 1; before >= 0; before -= 1) {
    for (let after = afterRows.length - 1; after >= 0; after -= 1) {
      matrix[at(before, after)] = beforeRows[before].key === afterRows[after].key
        ? matrix[at(before + 1, after + 1)] + 1
        : Math.max(matrix[at(before + 1, after)], matrix[at(before, after + 1)]);
    }
  }
  const lines = [];
  let before = 0;
  let after = 0;
  while (before < beforeRows.length || after < afterRows.length) {
    if (before < beforeRows.length
      && after < afterRows.length
      && beforeRows[before].key === afterRows[after].key) {
      lines.push({
        kind: 'context',
        before: beforeRows[before].number,
        after: afterRows[after].number,
        text: beforeRows[before].text,
        section: beforeRows[before].section,
      });
      before += 1;
      after += 1;
    } else if (before < beforeRows.length
      && (after === afterRows.length
        || matrix[at(before + 1, after)] >= matrix[at(before, after + 1)])) {
      lines.push({
        kind: 'removed',
        before: beforeRows[before].number,
        after: null,
        text: beforeRows[before].text,
        section: beforeRows[before].section,
      });
      before += 1;
    } else {
      lines.push({
        kind: 'added',
        before: null,
        after: afterRows[after].number,
        text: afterRows[after].text,
        section: afterRows[after].section,
      });
      after += 1;
    }
  }
  return {
    error: null,
    kind: 'artifact-text',
    hunks: compactArtifactTextHunks(lines),
  };
}

export function artifactTextChangeSummary(
  diff,
  source = 'macos-quick-look-visible-text',
  truncated = false,
) {
  const changed = diff.hunks.reduce(
    (count, hunk) => count + hunk.lines.filter((line) => line.kind !== 'context').length,
    0,
  );
  if (changed > 0) {
    return {
      changed,
      title: source === 'mesh-xlsx-cell-formula-v1'
        ? 'Cell and formula changes'
        : source === 'mesh-pptx-slide-text-v1'
          ? 'Slide content changes'
          : source === 'mesh-docx-block-text-v1'
            ? 'Document content and visibility changes'
          : source === 'macos-pdfkit-page-text-v1'
            ? 'Page text changes'
            : 'Visible text changes',
      warning: '',
    };
  }
  if (truncated) {
    return {
      changed,
      title: source === 'mesh-xlsx-cell-formula-v1'
        ? 'No differences in the extracted workbook prefix'
        : source === 'mesh-pptx-slide-text-v1'
          ? 'No differences in the extracted presentation prefix'
          : source === 'mesh-docx-block-text-v1'
            ? 'No differences in the extracted document prefix'
          : source === 'macos-pdfkit-page-text-v1'
            ? 'No differences in the extracted page prefix'
            : 'No differences in the extracted visible-text prefix',
      warning: 'Content beyond the extracted prefix was not compared. Inspect both exact saved versions before approval.',
    };
  }
  return {
    changed,
      title: source === 'mesh-xlsx-cell-formula-v1'
        ? 'Artifact changed; cells and formulas unchanged'
        : source === 'mesh-pptx-slide-text-v1'
          ? 'Artifact changed; slide text unchanged'
          : source === 'mesh-docx-block-text-v1'
            ? 'Document changed; extracted content and visibility unchanged'
          : source === 'macos-pdfkit-page-text-v1'
          ? 'Artifact changed; selected page text unchanged'
          : 'Artifact changed; visible text unchanged',
    warning: source === 'mesh-xlsx-cell-formula-v1'
      ? 'No cell or formula difference was extracted. Open both saved versions in the native app before approval.'
      : source === 'mesh-pptx-slide-text-v1'
        ? 'No slide-text difference was extracted. Open both saved versions in PowerPoint or Keynote before approval.'
        : source === 'mesh-docx-block-text-v1'
          ? 'No paragraph, table-text, or directly formatted hidden-run difference was extracted. Inspect both exact copies in Word before approval.'
        : source === 'macos-pdfkit-page-text-v1'
          ? 'No text difference was extracted on this page. Compare its exact visual rendering before approval.'
          : 'No visible text difference was extracted. Open both saved versions in the native app before approval.',
  };
}

export function artifactTextExtractionNote(kind, source, truncated, warning = '') {
  if (source === 'macos-pdfkit-page-text-v1') {
    return `Extracted as inert text from the exact selected PDF page${truncated ? ' and bounded to the first 512 text rows' : ''}. Mesh never executes document actions or embedded content. This view does not explain layout, annotations, forms, signatures, images, or embedded files; exact saved PDF bytes remain the approval input.${warning ? ` ${warning}` : ''}`;
  }
  if (source === 'mesh-pptx-slide-text-v1') {
    return `Grouped by exact slide order and OOXML slide name, with controls to compare one slide or the full presentation and inert text runs joined into paragraphs${truncated ? ' and bounded to the first 512 text rows' : ''}. Mesh never executes presentation content. This view does not explain layout, animation, speaker notes, charts, media, or embedded content; exact saved presentation bytes remain the approval input.${warning ? ` ${warning}` : ''}`;
  }
  if (source === 'mesh-docx-block-text-v1') {
    return `Grouped by Word heading structure and extracted as inert paragraph and table text plus directly formatted hidden-run visibility from bounded OOXML${truncated ? ', bounded to the first 512 blocks' : ''}. Mesh never executes fields, macros, links, or embedded content. This view does not explain style-inherited visibility, layout, comments, tracked-change metadata, headers, footers, images, or formatting; exact saved document bytes remain the approval input.${warning ? ` ${warning}` : ''}`;
  }
  if (source === 'mesh-xlsx-cell-formula-v1') {
    return `Grouped by worksheet name, with controls to compare one sheet or the full workbook, and extracted as inert cell coordinates, stored values, formulas, formula attributes, and cached results from bounded OOXML${truncated ? ', bounded to the first 512 cells' : ''}. Mesh never executes the workbook. This view does not explain formatting, metadata, or embedded content; exact saved document bytes remain the approval input.${warning ? ` ${warning}` : ''}`;
  }
  const structure = kind === 'presentation'
    ? 'slide'
    : kind === 'spreadsheet'
      ? 'sheet'
      : kind === 'pdf' ? 'page' : 'document';
  return `Grouped by ${structure} and extracted as plain visible text by macOS Quick Look${truncated ? ' and bounded to the first 512 text rows' : ''}. This view does not explain formulas, formatting, metadata, or embedded content; exact saved document bytes remain the approval input.${warning ? ` ${warning}` : ''}`;
}

export function artifactTextSourcesMatch(previews) {
  const sources = previews
    .map((preview) => preview?.text_source)
    .filter((source) => source !== null && source !== undefined);
  return new Set(sources).size <= 1;
}

function validateApprovalResult(answer, item, binding, exportToGit) {
  const digest = /^[0-9a-f]{64}$/;
  const git = answer?.git_export;
  if (
    !answer
    || typeof answer !== 'object'
    || !answer.workspace
    || typeof answer.workspace !== 'object'
    || answer.workspace.root !== binding.root
    || answer.workspace.installation !== binding.installation
    || answer.workspace.shared_version !== item.reviewed_head
    || !answer.receipt
    || typeof answer.receipt !== 'object'
    || answer.receipt.protocol !== 'mesh.v1.approval-receipt'
    || !digest.test(answer.receipt.canonical_receipt_blake3 || '')
    || !digest.test(answer.receipt.canonical_statement_blake3 || '')
    || !digest.test(answer.receipt.credential_id || '')
    || answer.receipt.review_bundle !== item.bundle
    || answer.receipt.shared_version !== item.reviewed_head
    || answer.receipt.user_verification !== 'user-presence'
    || !git
    || typeof git !== 'object'
    || !['not-requested', 'exported', 'failed'].includes(git.status)
    || (exportToGit && git.status === 'not-requested')
    || (!exportToGit && git.status !== 'not-requested')
    || (git.status === 'exported' && (
      typeof git.target !== 'string'
      || git.branch !== `mesh/approved/${item.reviewed_head}`
      || !/^[0-9a-f]{40,64}$/.test(git.commit || '')
      || git.approval_ref !== `refs/mesh/approvals/${answer.receipt.canonical_receipt_blake3}`
    ))
    || (git.status === 'failed' && typeof git.message !== 'string')
  ) {
    throw new Error('Mesh did not return a complete receipt for the exact version you approved. Refresh before relying on shared state.');
  }
  return answer;
}

function validateGitExportRetry(answer, item, binding) {
  const git = answer?.git_export;
  if (
    !answer
    || typeof answer !== 'object'
    || !answer.workspace
    || typeof answer.workspace !== 'object'
    || answer.workspace.root !== binding.root
    || answer.workspace.installation !== binding.installation
    || answer.workspace.digest !== binding.digest
    || answer.workspace.shared_version !== item.reviewed_head
    || !git
    || git.status !== 'exported'
    || typeof git.target !== 'string'
    || git.branch !== `mesh/approved/${item.reviewed_head}`
    || !/^[0-9a-f]{40,64}$/.test(git.commit || '')
    || !/^refs\/mesh\/approvals\/[0-9a-f]{64}$/.test(git.approval_ref || '')
    || typeof git.already_present !== 'boolean'
  ) {
    throw new Error('Mesh did not return a complete Git export for the exact shared version. Refresh before relying on the branch.');
  }
  return answer;
}

function validateGitExportInspection(answer, item, binding) {
  const git = answer?.git_export;
  if (
    !answer
    || typeof answer !== 'object'
    || !answer.workspace
    || typeof answer.workspace !== 'object'
    || answer.workspace.root !== binding.root
    || answer.workspace.installation !== binding.installation
    || answer.workspace.digest !== binding.digest
    || answer.workspace.shared_version !== item.reviewed_head
    || !git
    || typeof git !== 'object'
    || !['missing', 'exported'].includes(git.status)
    || typeof git.target !== 'string'
    || git.branch !== `mesh/approved/${item.reviewed_head}`
    || typeof git.already_present !== 'boolean'
    || (git.status === 'missing' && (
      git.commit !== null
      || git.approval_ref !== null
      || git.already_present
    ))
    || (git.status === 'exported' && (
      !/^[0-9a-f]{40,64}$/.test(git.commit || '')
      || !/^refs\/mesh\/approvals\/[0-9a-f]{64}$/.test(git.approval_ref || '')
      || !git.already_present
    ))
  ) {
    throw new Error('Mesh did not return a complete Git export inspection for the exact shared version.');
  }
  return answer;
}

function pendingReviewWork() {
  const nativePaths = new Set(model.workspace?.native_untracked_files || []);
  for (const entry of model.workspace?.native_unsupported_entries || []) nativePaths.add(entry.path);
  for (const change of model.folderChanges) nativePaths.add(change.path);
  return {
    nativeCount: nativePaths.size,
    editorDraft: editorDraftPending(),
    inventoryIncomplete: model.workspace?.native_inventory_complete === false
      || model.nativeInspectionFailed,
  };
}

function pendingReviewWorkMessage({ nativeCount, editorDraft, inventoryIncomplete }) {
  if (inventoryIncomplete) {
    return `Mesh could not inspect every native folder entry.${nativeCount ? ` ${nativeCount} visible native ${nativeCount === 1 ? 'change also remains' : 'changes also remain'}.` : ''} Retry Find folder changes before recording or approving a version.`;
  }
  if (nativeCount && editorDraft) {
    return `${nativeCount} newer native ${nativeCount === 1 ? 'change' : 'changes'} and an edit that exists only in this window are not included in this review. Preserve or resolve them before recording or approving a version.`;
  }
  if (nativeCount) {
    return `${nativeCount} newer native ${nativeCount === 1 ? 'change is' : 'changes are'} not included in this review. Save ${nativeCount === 1 ? 'it' : 'them'} privately or resolve ${nativeCount === 1 ? 'it' : 'them'} before recording or approving a version.`;
  }
  if (editorDraft) {
    return 'An edit exists only in this window and is not included in this review. Preserve or discard it before recording or approving a version.';
  }
  return '';
}

function refusePendingReviewWork() {
  const message = pendingReviewWorkMessage(pendingReviewWork());
  if (!message) return false;
  showNotice(message, true);
  return true;
}

async function verifyReviewScope() {
  if (workspaceInstallationMatchesHandoff()) {
    showNotice('This folder is still assigned to an agent. After every process using it stops, choose Finish agent handoff, inspect and save the complete result, then review or approve that saved version.', true);
    return false;
  }
  if (refusePendingReviewWork()) return false;
  // Review and approval are rare, high-impact transitions. Re-read the native folder at the
  // boundary so a tracked file changed after the last periodic scan cannot be silently omitted
  // from the exact saved version the person is about to review or share.
  const inspected = await scanNativeFolder({ automatic: true, startup: true });
  if (!inspected) {
    showNotice('Mesh could not finish checking the native folder for newer work. Refresh or scan the folder, then try the review again.', true);
    return false;
  }
  return !refusePendingReviewWork();
}

async function approveReviewItem(item, exportToGit) {
  let attemptedApproval = null;
  try {
    if (!await verifyReviewScope()) return;
    const binding = captureVerifiedWorkspace();
    attemptedApproval = Object.freeze({
      root: binding.root,
      installation: binding.installation,
      subject: item.subject_operation,
      bundle: item.bundle,
      reviewedHead: item.reviewed_head,
    });
    await coordinateWorkspaceMutation(async (sequence) => {
      const answer = validateApprovalResult(JSON.parse(await invoke('approve_current_review', {
        bundle: item.bundle,
        target: item.subject_operation,
        expectedWorkspaceRoot: binding.root,
        expectedWorkspaceDigest: binding.digest,
        expectedWorkspaceInstallation: binding.installation,
        exportToGit,
      })), item, binding, exportToGit);
      await installVerifiedWorkspace(async () => answer.workspace, sequence);
      renderWorkspace();
      const git = answer.git_export;
      if (git?.status === 'exported') {
        showNotice(`Approved and created Git review branch ${git.branch} in ${git.target}. Original working files were not changed.`);
      } else if (git?.status === 'failed') {
        showNotice(`Approval succeeded, but Git export was refused: ${git.message}`, true);
      } else {
        showNotice(`Approved with macOS user presence. Shared version ${answer.receipt.shared_version}. Receipt ${answer.receipt.canonical_receipt_blake3}.`);
      }
    });
  } catch (error) {
    if (!attemptedApproval) {
      showNotice(String(error), true);
      return;
    }
    const recovered = await refresh();
    const recoveredReview = currentReviewItem();
    if (
      recovered
      && model.workspace?.root === attemptedApproval.root
      && model.workspace?.installation === attemptedApproval.installation
      && model.workspace?.shared_version === attemptedApproval.reviewedHead
      && recoveredReview?.subject_operation === attemptedApproval.subject
      && recoveredReview?.bundle === attemptedApproval.bundle
      && recoveredReview?.reviewed_head === attemptedApproval.reviewedHead
      && recoveredReview.recorded !== false
    ) {
      showNotice('Mesh lost the approval reply but confirmed that this exact reviewed version is now the shared version. The original folder was not changed.');
    } else if (recovered) {
      showNotice(`${error} Mesh could not verify the exact reviewed workspace after the approval attempt. Review the workspace now visible before trying again.`, true);
    } else {
      showNotice(`${error} Mesh could not confirm the approval outcome. Managed actions remain paused until Refresh verifies the shared version.`, true);
    }
  }
}

async function exportSharedReviewItemToGit(item) {
  let attemptedExport = null;
  try {
    if (!await verifyReviewScope()) return;
    const binding = captureVerifiedWorkspace();
    attemptedExport = {
      root: binding.root,
      installation: binding.installation,
      reviewedHead: item.reviewed_head,
      bundle: item.bundle,
      target: item.subject_operation,
    };
    await coordinateWorkspaceMutation(async (sequence) => {
      const answer = validateGitExportRetry(JSON.parse(await invoke('export_shared_review_to_git', {
        bundle: item.bundle,
        target: item.subject_operation,
        expectedWorkspaceRoot: binding.root,
        expectedWorkspaceDigest: binding.digest,
        expectedWorkspaceInstallation: binding.installation,
      })), item, binding);
      await installVerifiedWorkspace(async () => answer.workspace, sequence);
      renderWorkspace();
      const git = answer.git_export;
      showNotice(`${git.already_present ? 'Verified existing' : 'Created'} Git review branch ${git.branch} in ${git.target}. Original working files were not changed.`);
    });
  } catch (error) {
    if (!attemptedExport) {
      showNotice(String(error), true);
      return;
    }
    const recovered = await refresh();
    if (
      !recovered
      || model.workspace?.root !== attemptedExport.root
      || model.workspace?.installation !== attemptedExport.installation
      || model.workspace?.shared_version !== attemptedExport.reviewedHead
    ) {
      showNotice(`${error} Mesh could not verify the exact shared workspace after the export attempt. Managed actions remain paused until Refresh succeeds.`, true);
      return;
    }
    const recoveryBinding = captureVerifiedWorkspace();
    try {
      const inspection = validateGitExportInspection(JSON.parse(await invoke('inspect_shared_review_git_export', {
        bundle: attemptedExport.bundle,
        target: attemptedExport.target,
        expectedWorkspaceRoot: recoveryBinding.root,
        expectedWorkspaceDigest: recoveryBinding.digest,
        expectedWorkspaceInstallation: recoveryBinding.installation,
      })), item, recoveryBinding);
      assertVerifiedWorkspace(recoveryBinding);
      const git = inspection.git_export;
      if (git.status === 'exported') {
        showNotice(`Mesh lost the Git export reply but verified the exact existing branch ${git.branch} in ${git.target}. Original working files were not changed.`);
      } else {
        showNotice(`${error} Mesh confirmed that no Git review branch was created. Choose Create Git branch to try again.`, true);
      }
    } catch (inspectionError) {
      showNotice(`${error} Mesh could not confirm the Git export outcome: ${inspectionError} Refresh before relying on the branch.`, true);
    }
  }
}

function installCurrentReviewArtifactActions(item) {
  if (item.subject_operation !== currentWorkspaceVersion()?.operation) return;
  for (const change of item.bundle_changes || []) {
    const kind = reviewArtifactKind(change);
    for (const side of ['before', 'after']) {
      if (change[side] === null) continue;
      for (const action of ['open-entry', 'reveal-entry', 'open-folder']) {
        reviewWorkbenchNextActions.set(`open-review-side:${change.object_id}:${side}:${action}`, async () => {
          if (!beginNativeWorkspaceLaunch()) return;
          const expectedVersion = change[side]?.version_id;
          const expectedDigest = change[side]?.content_digest
            || change.verified_text?.[side]?.content_digest
            || null;
          let completion;
          try {
            const binding = captureVerifiedWorkspace();
            if (!/^[0-9a-f]{64}$/u.test(expectedVersion || '')
              || !/^[0-9a-f]{64}$/u.test(expectedDigest || '')) {
              throw new Error('The selected saved side has no complete immutable identity. Refresh Review before opening it.');
            }
            const answer = JSON.parse(await invoke('open_review_artifact_inspection', {
              expectedWorkspaceRoot: binding.root,
              expectedWorkspaceDigest: binding.digest,
              expectedWorkspaceInstallation: binding.installation,
              bundle: item.bundle,
              target: item.subject_operation,
              objectId: change.object_id,
              side,
              expectedVersionId: expectedVersion,
              expectedContentDigest: expectedDigest,
              action,
            }));
            assertVerifiedWorkspace(binding);
            const keys = answer && typeof answer === 'object' && !Array.isArray(answer)
              ? Object.keys(answer).sort()
              : [];
            if (keys.join(',') !== 'action,content_digest,opened,schema,side,version_id,working_folder_unchanged'
              || answer.schema !== 'mesh.review-side-open/v1'
              || answer.side !== side
              || answer.action !== action
              || answer.version_id !== expectedVersion
              || (expectedDigest !== null && answer.content_digest !== expectedDigest)
              || !/^[0-9a-f]{64}$/u.test(answer.content_digest)
              || answer.opened !== true
              || answer.working_folder_unchanged !== true) {
              throw new Error('The native saved-side launcher returned a stale or malformed result.');
            }
            const sideLabel = side === 'before' ? 'Before' : 'After';
            completion = {
              message: action === 'open-entry'
                ? `Opened the exact ${sideLabel.toLowerCase()} saved copy in its default application.`
                : action === 'reveal-entry'
                  ? `Revealed the exact ${sideLabel.toLowerCase()} saved copy in Finder.`
                  : `Opened the folder containing the exact ${sideLabel.toLowerCase()} saved copy.`,
              error: false,
            };
          } catch (error) {
            completion = {
              message: `Exact ${side} copy unavailable: ${decodeDaemonRefusal(error).message || error}`,
              error: true,
            };
          } finally {
            finishNativeWorkspaceLaunch();
          }
          // Publish completion only after the global native-launch freeze has been released and
          // the fresh controls are mounted. A person may act on this success notice immediately.
          showNotice(completion.message, completion.error);
        });
      }
    }
    if (!kind) continue;
    const formatName = {
      pdf: 'PDF',
      presentation: 'PowerPoint',
      document: 'Word',
      spreadsheet: 'Excel',
    }[kind] || 'Document';
    const pdfPageCounts = new Map();
    reviewWorkbenchNextActions.set(`inspect-exact-copies:${change.object_id}`, async () => {
      const binding = captureVerifiedWorkspace();
      try {
        const destination = await invoke('pick_folder');
        if (!destination) return;
        assertVerifiedWorkspace(binding);
        const sides = ['before', 'after'].filter((side) => change[side] !== null);
        const answer = validatedReviewInspectionExport(JSON.parse(await invoke(
          'export_review_artifact_inspection',
          {
            expectedWorkspaceRoot: binding.root,
            expectedWorkspaceDigest: binding.digest,
            expectedWorkspaceInstallation: binding.installation,
            bundle: item.bundle,
            target: item.subject_operation,
            objectId: change.object_id,
            sides,
            destination,
          },
        )), change, sides);
        assertVerifiedWorkspace(binding);
        showNotice(answer.finder_opened
          ? `Exact read-only copies are ready in ${answer.directory}. Mesh opened the folder; document content was not opened automatically.`
          : `Exact read-only copies are ready in ${answer.directory}. Open that folder to inspect them in ${formatName}.${answer.warning ? ` ${answer.warning}` : ''}`);
      } catch (error) {
        showNotice(`Exact copies unavailable: ${decodeDaemonRefusal(error).message || error}`, true);
      }
    });
    reviewWorkbenchNextActions.set(`load-artifact-preview:${change.object_id}`, async (intent) => {
      const requestedPage = kind === 'pdf' ? intent?.pageNumber : 1;
      if (!Number.isSafeInteger(requestedPage) || requestedPage < 1 || requestedPage > 64) {
        throw new Error('The requested artifact preview page was invalid.');
      }
      const requestSequence = ++reviewWorkbenchNextArtifactRequestSequence;
      const binding = captureVerifiedWorkspace();
      const expectedSides = ['before', 'after'].filter((side) => change[side] !== null);
      const rendered = await Promise.all(expectedSides.map(async (side) => {
        const knownPages = pdfPageCounts.get(side);
        if (kind === 'pdf' && knownPages !== undefined && requestedPage > knownPages) {
          const summary = change[side];
          return {
            side,
            preview: null,
            absentPage: {
              side,
              versionId: summary.version_id,
              contentDigest: summary.content_digest,
              pageCount: knownPages,
            },
            error: null,
          };
        }
        try {
          const answer = JSON.parse(await invoke('render_review_artifact', {
            expectedWorkspaceRoot: binding.root,
            expectedWorkspaceDigest: binding.digest,
            expectedWorkspaceInstallation: binding.installation,
            bundle: item.bundle,
            target: item.subject_operation,
            objectId: change.object_id,
            side,
            ...(kind === 'pdf' ? { pageNumber: requestedPage } : {}),
          }));
          const preview = validatedReviewArtifactPreview(answer, change, side, kind);
          if (kind === 'pdf') pdfPageCounts.set(side, preview.page_count);
          return {
            side,
            preview: {
              side,
              versionId: preview.version_id,
              contentDigest: preview.content_digest,
              imageDataUrl: preview.image_data_url,
              pageNumber: preview.page_number,
              pageCount: preview.page_count,
              textSource: preview.text_source,
              textLines: preview.text_lines,
              textSections: preview.text_sections?.map((section) => ({
                label: section.label,
                lineStart: section.line_start,
                lineCount: section.line_count,
              })) || null,
              textTruncated: preview.text_truncated,
            },
            absentPage: null,
            error: null,
          };
        } catch (error) {
          return {
            side,
            preview: null,
            absentPage: null,
            error: (decodeDaemonRefusal(error).message || String(error)).slice(0, 1_024),
          };
        }
      }));
      if (requestSequence !== reviewWorkbenchNextArtifactRequestSequence) return;
      const delivery = reviewWorkbenchNextMounted;
      if (!delivery
        || !projectionHasVisibleContinuity(reviewWorkbenchNextPending, delivery)
        || reviewWorkbenchNextPending.bundle !== delivery.bundle
        || item.bundle !== delivery.bundle) return;
      assertVerifiedWorkspace(binding);
      if (requestSequence !== reviewWorkbenchNextArtifactRequestSequence) return;
      const result = new Map(rendered.map((entry) => [entry.side, entry]));
      appDocument.dispatchEvent(new appWindow.CustomEvent('mesh:review-workbench-artifact-preview', {
        detail: Object.freeze({
          generation: delivery.generation,
          bundle: delivery.bundle,
          changeId: change.object_id,
          kind,
          requestedPage,
          before: result.get('before')?.preview || null,
          after: result.get('after')?.preview || null,
          beforeAbsentPage: result.get('before')?.absentPage || null,
          afterAbsentPage: result.get('after')?.absentPage || null,
          beforeError: result.get('before')?.error || null,
          afterError: result.get('after')?.error || null,
        }),
      }));
    });
  }
}

function liveReviewPresentation() {
  const current = currentWorkspacePresentation();
  const assigned = workspaceInstallationMatchesHandoff();
  const state = assigned ? model.agentLive?.state || 'scanning' : 'idle';
  const changes = assigned ? model.agentLive?.changes || [] : [];
  return Object.freeze({
    available: assigned,
    state,
    summary: current?.agentActivity.summary || 'No agent currently owns this workspace.',
    workspaceRoot: model.workspace?.root || '',
    changes: Object.freeze(changes.map((change) => Object.freeze({ path: change.path, kind: change.kind }))),
    workspaces: Object.freeze(current?.workspaces || []),
  });
}

function installLiveReviewActions(live) {
  for (const workspace of live.workspaces) {
    if (!workspace.canOpen) continue;
    reviewWorkbenchNextActions.set(`switch-live-workspace:${workspace.path}`, async () => {
      const continuity = workspaceProjectionContinuityKey();
      const selected = recentWorkspaceEntries().find((entry) => entry.path === workspace.path);
      if (!selected || workspaceInteractionInFlight()) {
        throw new Error('That assigned workspace is no longer available. Refresh and choose it again.');
      }
      const opened = await openRecentWorkspacePath(
        workspace.path,
        () => recentWorkspaceEntries().some((entry) => entry.path === workspace.path)
          && workspaceProjectionContinuityKey() === continuity,
        'The workspace list changed while Mesh was switching Live review. Review the current workspace and choose again.',
      );
      if (opened && workspaceInstallationMatchesHandoff()) void inspectLiveAgentWork();
      return opened;
    });
  }
  if (!live.available) return;
  for (const change of live.changes) {
    if (!['modified-file', 'new-file'].includes(change.kind)) continue;
    reviewWorkbenchNextActions.set(`load-live-file:${change.path}`, async () => {
      const delivery = reviewWorkbenchNextMounted;
      const generation = delivery?.generation;
      const binding = captureVerifiedWorkspace();
      const agentGeneration = canonicalAgentHandoffGeneration(model.agentHandoff?.generation);
      if (!delivery || !agentGeneration || !workspaceInstallationMatchesHandoff()) {
        throw new Error('This workspace is no longer assigned to the same agent.');
      }
      try {
        const answer = JSON.parse(await invoke('inspect_agent_live_file', {
          expectedWorkspaceRoot: binding.root,
          expectedWorkspaceDigest: binding.digest,
          expectedWorkspaceInstallation: binding.installation,
          expectedAgentHandoffGeneration: agentGeneration,
          relativePath: change.path,
        }));
        assertVerifiedWorkspace(binding);
        const keys = answer && typeof answer === 'object' && !Array.isArray(answer)
          ? Object.keys(answer).sort()
          : [];
        if (keys.join(',') !== 'agent_handoff_generation,byte_count,content_digest,executable,image_data_url,kind,mutable,path,preview_error,preview_kind,recorded,schema,text,workspace_digest,workspace_installation,workspace_root'
          || answer.schema !== 'mesh.agent-live-file/v1'
          || answer.workspace_root !== binding.root
          || answer.workspace_digest !== binding.digest
          || answer.workspace_installation !== binding.installation
          || answer.agent_handoff_generation !== agentGeneration
          || answer.path !== change.path
          || answer.kind !== change.kind
          || !Number.isSafeInteger(answer.byte_count)
          || answer.byte_count < 0
          || typeof answer.content_digest !== 'string'
          || !/^[0-9a-f]{64}$/u.test(answer.content_digest)
          || typeof answer.executable !== 'boolean'
          || !['text', 'image', 'artifact', 'metadata'].includes(answer.preview_kind)
          || (answer.image_data_url !== null && (typeof answer.image_data_url !== 'string'
            || !answer.image_data_url.startsWith('data:image/')
            || answer.image_data_url.length > 12 * 1024 * 1024))
          || (answer.preview_error !== null && (typeof answer.preview_error !== 'string' || answer.preview_error.length > 1_024))
          || (answer.text !== null && (typeof answer.text !== 'string' || answer.text.length > 1_048_576))
          || answer.mutable !== true
          || answer.recorded !== false) {
          throw new Error('The live file snapshot was stale or malformed.');
        }
        if (!projectionAcceptsVisibleGeneration(generation, reviewWorkbenchNextPending, reviewWorkbenchNextMounted)
          || reviewWorkbenchNextMounted !== delivery
          || !agentLiveInspectionStillCurrent({ ...binding, generation: agentGeneration })) return;
        appDocument.dispatchEvent(new appWindow.CustomEvent('mesh:review-workbench-live-preview', {
          detail: Object.freeze({
            generation,
            path: change.path,
            error: null,
            snapshot: Object.freeze({
              path: answer.path,
              kind: answer.kind,
              byteCount: answer.byte_count,
              contentDigest: answer.content_digest,
              executable: answer.executable,
              text: answer.text,
              previewKind: answer.preview_kind,
              imageDataUrl: answer.image_data_url,
              previewError: answer.preview_error,
            }),
          }),
        }));
      } catch (error) {
        if (!projectionAcceptsVisibleGeneration(generation, reviewWorkbenchNextPending, reviewWorkbenchNextMounted)
          || reviewWorkbenchNextMounted !== delivery) return;
        appDocument.dispatchEvent(new appWindow.CustomEvent('mesh:review-workbench-live-preview', {
          detail: Object.freeze({
            generation,
            path: change.path,
            snapshot: null,
            error: (decodeDaemonRefusal(error).message || String(error)).slice(0, 1_024),
          }),
        }));
      }
    });
  }
}

function renderReviews() {
  reviewWorkbenchNextActions = new Map();
  const live = liveReviewPresentation();
  installLiveReviewActions(live);
  const items = model.workspace?.review_items || [];
  const total = model.workspace?.reviews || 0;
  const current = currentWorkspaceVersion();
  const currentItem = current
    ? items.find((item) => item.subject_operation === current.operation) || null
    : null;
  const readyCount = items.filter((item) => item.recorded === false).length;
  const recorded = Boolean(currentItem && currentItem.recorded !== false);
  const approvalReady = model.approval?.enrolled === true;
  const approvalAvailable = model.approval?.available === true;
  const approvalUnavailableReason = typeof model.approval?.unavailable_reason === 'string'
    && model.approval.unavailable_reason
    ? model.approval.unavailable_reason
    : APPROVAL_STATUS_UNVERIFIED;
  const pending = pendingReviewWork();
  const pendingMessage = pendingReviewWorkMessage(pending);
  const agentAssigned = workspaceInstallationMatchesHandoff();
  const reviewScopeBlocked = Boolean(
    agentAssigned
    || pending.nativeCount
    || pending.editorDraft
    || pending.inventoryIncomplete
  );
  const interactionBlocked = workspaceInteractionInFlight();
  const canSetupApproval = !approvalEnrollmentInFlight
    && !interactionBlocked
    && !approvalReady
    && approvalAvailable;
  const canRecordReview = Boolean(
    !interactionBlocked
    && model.workspaceVerified
    && current
    && !recorded
    && !reviewScopeBlocked
  );
  const alreadyShared = Boolean(currentItem?.reviewed_head)
    && model.workspace?.shared_version === currentItem.reviewed_head;
  const projectRoot = model.workspace?.root
    ? recentWorkspaceEntry(model.workspace.root).projectRoot
    : null;
  const canApprove = Boolean(
    currentItem
    && recorded
    && currentItem.content_complete
    && !alreadyShared
    && approvalReady
    && !interactionBlocked
    && model.workspaceVerified
    && !reviewScopeBlocked
  );
  const canApproveAndExport = Boolean(canApprove && projectRoot);
  const canExportGit = Boolean(
    currentItem
    && recorded
    && currentItem.content_complete
    && alreadyShared
    && projectRoot
    && !interactionBlocked
    && model.workspaceVerified
    && !reviewScopeBlocked
  );
  const canExportPrivateCopy = Boolean(
    currentItem
    && recorded
    && explicitPrivateExportAvailable()
  );
  const earlierReviews = Object.freeze(items
    .filter((item) => item.subject_operation !== current?.operation)
    .map((item) => {
      const savedPointAvailable = (model.workspace?.workspace_versions || [])
        .some((version) => version.operation === item.subject_operation);
      const itemAlreadyShared = Boolean(item.reviewed_head)
        && model.workspace?.shared_version === item.reviewed_head;
      const canOpen = item.recorded !== false
        && item.content_complete === true
        && !itemAlreadyShared
        && savedPointAvailable
        && !interactionBlocked
        && model.workspaceVerified
        && !reviewScopeBlocked;
      if (canOpen) {
        reviewWorkbenchNextActions.set(`open-earlier-review:${item.subject_operation}`, async () => {
          try {
            pendingAgentVersionChoice = false;
            if (!(model.workspace?.workspace_versions || [])
              .some((version) => version.operation === item.subject_operation)) {
              throw new Error('This earlier saved point is no longer available. Refresh the workspace before trying again.');
            }
            selectedWorkspaceVersionOperation = item.subject_operation;
            workspaceVersionDestinationDraft = '';
            await previewSelectedWorkspaceVersion(item.subject_operation);
            if (selectedWorkspaceVersionOperation !== item.subject_operation) return;
            focusWorkspaceVersionJourney(item.subject_operation);
            showNotice('This exact earlier saved point is selected. Review its contents, then open it as an independent working folder. Because that folder has its own history, record its fresh review there before approval. Newer private work remains unchanged.');
          } catch (error) {
            showNotice(`Mesh could not prepare that earlier saved point: ${error}`, true);
          }
        });
      }
      return Object.freeze({
        operation: item.subject_operation,
        label: savedPointAvailable
          ? `Review saved point ${shortIdentity(item.subject_operation)} again`
          : `Earlier saved point ${shortIdentity(item.subject_operation)} unavailable`,
        canOpen,
      });
    }));
  const controls = Object.freeze({
    countLabel: `${total} recorded review${total === 1 ? '' : 's'}${readyCount ? ` · ${readyCount} ready` : ''}`,
    overflowLabel: model.workspace?.review_items_not_listed
      ? `${model.workspace.review_items_not_listed} additional review${model.workspace.review_items_not_listed === 1 ? '' : 's'} not shown in this bounded view.`
      : null,
    canSetupApproval,
    setupApprovalLabel: approvalEnrollmentInFlight
      ? 'Setting up approvals…'
      : approvalReady
        ? 'Approval ready'
        : approvalAvailable
          ? 'Set up approvals'
          : 'Approval unavailable',
    setupApprovalReason: approvalReady
      ? 'A device-only approval credential is ready. Every approval still requires explicit macOS user presence.'
      : approvalAvailable
        ? 'Set up a device-only approval credential. Every approval still requires explicit macOS user presence.'
        : approvalUnavailableReason,
    canRecordReview,
    recordReviewLabel: recorded ? 'Review recorded' : 'Record reviewed version',
    recordReviewReason: agentAssigned
      ? 'Choose Finish agent handoff after every process using this folder has stopped, then save its complete result before recording a review.'
      : pendingMessage || (current
        ? 'Mesh must verify this exact saved workspace before recording its review.'
        : 'Save work privately before recording an exact reviewed version.'),
    earlierReviews,
  });
  if (canSetupApproval) reviewWorkbenchNextActions.set('setup-approval', setupApprovalCredential);
  if (canRecordReview) reviewWorkbenchNextActions.set('record-review', recordCurrentReview);
  const readyReview = Boolean(
    model.workspaceVerified
    && currentItem?.content_complete === true
    && Array.isArray(currentItem.bundle_changes)
    && currentItem.bundle_changes.length > 0
    && currentItem.bundle_changes_not_listed === 0
  );
  if (readyReview) {
    if (canApprove) reviewWorkbenchNextActions.set('approve-version', () => approveReviewItem(currentItem, false));
    if (canApproveAndExport) reviewWorkbenchNextActions.set('approve-and-export', () => approveReviewItem(currentItem, true));
    if (canExportGit) reviewWorkbenchNextActions.set('export-git', () => exportSharedReviewItemToGit(currentItem));
    if (canExportPrivateCopy) {
      reviewWorkbenchNextActions.set('choose-private-export', choosePrivateExportTarget);
    }
    installCurrentReviewArtifactActions(currentItem);
  }
  if (reviewWorkbenchNextAvailable
    && readyReview
    && typeof appDocument.dispatchEvent === 'function'
    && typeof appWindow.CustomEvent === 'function') {
    const generation = ++reviewWorkbenchNextGeneration;
    const continuityKey = reviewProjectionContinuityKey(currentItem.bundle);
    const interactionKey = projectionSurfaceContinuityKey(
      continuityKey,
      'review-actions',
      [...reviewWorkbenchNextActions.keys()].sort(),
    );
    reviewWorkbenchNextPending = Object.freeze({
      generation,
      bundle: currentItem.bundle,
      pageState: 'ready',
      continuityKey,
      interactionKey,
    });
    if (reviewWorkbenchNextMounted?.continuityKey !== continuityKey) reviewWorkbenchNextMounted = null;
    installReviewWorkbenchNextVisibility();
    const canRenderArtifactPreview = currentItem.bundle_changes.some((change) => (
      reviewWorkbenchNextActions.has(`load-artifact-preview:${change.object_id}`)
    ));
    const canInspectExactCopies = currentItem.bundle_changes.some((change) => (
      reviewWorkbenchNextActions.has(`inspect-exact-copies:${change.object_id}`)
    ));
    appDocument.dispatchEvent(new appWindow.CustomEvent('mesh:review-workbench-projection', {
      detail: Object.freeze({
        generation,
        state: 'ready',
        workspaceName: 'Current Mesh workspace',
        versionLabel: `Saved ${shortIdentity(currentItem.reviewed_head || currentItem.subject_operation)}`,
        projection: currentItem,
        controls,
        live,
        authority: Object.freeze({
          canRenderArtifactPreview,
          canInspectExactCopies,
          canRecordReview,
          canApprove,
          canApproveAndExport,
          canExportGit,
          canExportPrivateCopy,
          approvalReason: canApprove
            ? 'This exact reviewed version is ready for native approval with macOS user presence.'
            : alreadyShared
              ? 'Approved with user presence. This exact version is now shared.'
              : reviewScopeBlocked
                ? pendingMessage || 'Review actions stay unavailable until Mesh verifies the complete saved workspace.'
              : canRecordReview
                ? `Automatic review candidate. Inspect these changes, then confirm that this exact saved version is ready for approval.${approvalAvailable ? '' : ` Approval is unavailable in this build. ${approvalUnavailableReason} After review, use Choose export folder to make a private copy in a different ordinary folder.`}`
              : !approvalAvailable
                ? `You can inspect and record this exact version, but approval is unavailable in this build. ${approvalUnavailableReason} After review, use Choose export folder to make a private copy in a different ordinary folder.`
                : 'Review actions stay unavailable until Mesh verifies the exact workspace and its approval state.',
        }),
      }),
    }));
  } else if (reviewWorkbenchNextAvailable && model.workspace) {
    const state = !model.workspaceVerified
      || hasConcurrentWorkspaceHistory()
      || model.workspace.review_items_not_listed > 0
      || Boolean(currentItem && (
        currentItem.content_complete !== true
        || !Array.isArray(currentItem.bundle_changes)
        || currentItem.bundle_changes_not_listed !== 0
      ))
      ? 'unavailable'
      : 'empty';
    const status = state === 'unavailable'
      ? Object.freeze({
          title: 'Review details are unavailable',
          description: !model.workspaceVerified
            ? 'Mesh has not verified the current workspace. Refresh successfully before relying on review details.'
            : hasConcurrentWorkspaceHistory()
              ? 'This workspace has concurrent saved heads, so Mesh cannot name one exact current version to review.'
              : 'Mesh could not verify a complete bounded review for the current saved version. No review or approval action is available.',
        })
      : current
        ? Object.freeze({
            title: currentItem
              ? 'No changed files need review'
              : 'No review is ready for this saved version',
            description: currentItem
              ? 'The complete current review bundle contains no changed files, so there is no approval decision here.'
              : 'Mesh has no complete review bundle for the current saved version. Record the exact saved version when that action is available.',
          })
        : Object.freeze({
            title: 'No review is ready yet',
            description: 'Save work privately before reviewing an exact saved version.',
          });
    const generation = ++reviewWorkbenchNextGeneration;
    const continuityKey = reviewProjectionContinuityKey(`status:${state}`);
    const interactionKey = projectionSurfaceContinuityKey(continuityKey, 'review-status', {
      status,
      controls,
      liveActions: [...reviewWorkbenchNextActions.keys()].sort(),
    });
    reviewWorkbenchNextPending = Object.freeze({
      generation,
      bundle: null,
      pageState: state,
      continuityKey,
      interactionKey,
    });
    if (reviewWorkbenchNextMounted?.continuityKey !== continuityKey) reviewWorkbenchNextMounted = null;
    installReviewWorkbenchNextVisibility();
    appDocument.dispatchEvent(new appWindow.CustomEvent('mesh:review-workbench-projection', {
      detail: Object.freeze({ generation, state, status, controls, live }),
    }));
  } else {
    reviewWorkbenchNextPending = null;
    reviewWorkbenchNextMounted = null;
  }
  installReviewWorkbenchNextVisibility();
  renderNextAction();
}

async function refreshApprovalStatus({ duringEnrollmentRecovery = false } = {}) {
  // Enrollment is an authority-creating native ceremony. A status read started while its dialog
  // is open can truthfully observe the old unenrolled state, but it must not later replace the
  // ceremony's result. Keep status refreshes out of that interval rather than relying on response
  // ordering alone.
  if (approvalEnrollmentInFlight && !duringEnrollmentRecovery) return false;
  const sequence = ++approvalStatusSequence;
  try {
    const status = validateApprovalStatus(JSON.parse(await invoke('approval_credential_status')));
    if (sequence !== approvalStatusSequence) return false;
    model.approval = status;
  } catch {
    if (sequence !== approvalStatusSequence) return false;
    model.approval = {
      enrolled: false,
      available: false,
      unavailable_reason: APPROVAL_STATUS_UNVERIFIED,
    };
  }
  renderReviews();
  return true;
}

function validateApprovalStatus(status) {
  if (
    !status
    || typeof status !== 'object'
    || Array.isArray(status)
    || typeof status.enrolled !== 'boolean'
    || typeof status.available !== 'boolean'
    || (status.enrolled && !status.available)
    || (status.available && status.unavailable_reason !== null)
    || (!status.available && (
      typeof status.unavailable_reason !== 'string'
      || status.unavailable_reason.trim() === ''
    ))
  ) throw new Error('invalid approval status');
  return status;
}

async function setupApprovalCredential() {
  if (approvalEnrollmentInFlight) return;
  if (model.approval?.available !== true) {
    showNotice(model.approval?.unavailable_reason || APPROVAL_STATUS_UNVERIFIED, true);
    return;
  }
  approvalEnrollmentInFlight = true;
  const sequence = ++approvalStatusSequence;
  renderReviews();
  try {
    const status = validateApprovalStatus(JSON.parse(await invoke('enroll_approval_credential')));
    if (sequence !== approvalStatusSequence) return;
    model.approval = status;
    showNotice('Approval credential ready. Every approval still requires explicit macOS user presence.');
  } catch (error) {
    if (sequence !== approvalStatusSequence) return;
    // The native side may have stored the Secure Enclave credential before its reply was lost or
    // rejected. Never offer another authority-creating ceremony on an unknown outcome. Re-read
    // the durable native status while the setup control remains locked, then present that truth.
    await refreshApprovalStatus({ duringEnrollmentRecovery: true });
    if (model.approval?.enrolled === true) {
      showNotice('Mesh lost the setup reply but confirmed that the approval credential is ready. Every approval still requires explicit macOS user presence.');
    } else if (model.approval?.available === true) {
      showNotice(`Approval setup did not complete. ${error}`, true);
    } else {
      showNotice(`Mesh could not confirm the approval setup outcome. ${model.approval?.unavailable_reason || APPROVAL_STATUS_UNVERIFIED}`, true);
    }
  } finally {
    approvalEnrollmentInFlight = false;
    renderReviews();
  }
}

async function recordCurrentReview() {
  let attemptedReview = null;
  try {
    if (!await verifyReviewScope()) return;
    const binding = captureVerifiedWorkspace();
    attemptedReview = {
      root: binding.root,
      installation: binding.installation,
      subject: currentWorkspaceVersion()?.operation || null,
    };
    await coordinateWorkspaceMutation(async (sequence) => {
      const workspace = JSON.parse(await invoke('open_current_review', {
        expectedWorkspaceRoot: binding.root,
        expectedWorkspaceDigest: binding.digest,
        expectedWorkspaceInstallation: binding.installation,
      }));
      await installVerifiedWorkspace(async () => workspace, sequence);
      renderWorkspace();
      showNotice(model.approval?.enrolled === true
        ? 'Exact saved version recorded for review. Approval is ready; inspect the recorded change summary, then choose Approve to shared version.'
        : model.approval?.available !== true
          ? `Exact saved version recorded for review. Approval is unavailable in this build. ${model.approval?.unavailable_reason || APPROVAL_STATUS_UNVERIFIED}`
        : 'Exact saved version recorded for review. Set up approvals before this version can be shared.');
    });
  } catch (error) {
    if (!attemptedReview?.subject) {
      showNotice(String(error), true);
      return;
    }
    // Recording a review is also non-retryable after dispatch. If its reply is lost, reload the
    // exact workspace and accept only a durable recorded review for the same physical workspace
    // and saved-version subject. Never invite the person to create a duplicate record blindly.
    const recovered = await refresh();
    const review = currentReviewItem();
    if (
      recovered
      && model.workspace?.root === attemptedReview.root
      && model.workspace?.installation === attemptedReview.installation
      && review?.subject_operation === attemptedReview.subject
      && review.recorded !== false
    ) {
      showNotice('Mesh lost the review reply but confirmed that this exact saved version is recorded for review.');
    } else if (recovered) {
      showNotice(`${error} Mesh confirmed that this saved version is not recorded for review. Inspect the visible state before trying again.`, true);
    } else {
      showNotice(`${error} Mesh could not confirm the review outcome. Managed actions remain paused until Refresh verifies the workspace.`, true);
    }
  }
}

function markWorkspaceUnverified() {
  model.workspaceVerified = false;
  setLocalService('attention');
  renderWorkspace();
}

function clearWorkspaceScopedState() {
  exportPreviewSequence += 1;
  workspaceFilePreviewSequence += 1;
  pendingAgentVersionChoice = false;
  model.agentFolder = null;
  model.agentLive = null;
  agentLiveInspectionSequence += 1;
  model.editor = null;
  model.folderChanges = [];
  model.nativeInspectionFailed = false;
  model.exportPreview = null;
  model.exportBatchPreview = null;
  model.exportRoot = null;
  model.restorePreview = null;
  model.restoreUndo = null;
  selectedRestoreFileId = '';
  selectedRestoreVersionId = '';
  restoreApplyAttempted = false;
  model.workspaceVersionPreview = null;
  model.workspaceVersionPreviewError = null;
  selectedWorkspaceVersionOperation = '';
  workspaceVersionDestinationDraft = '';
  workspaceFilesState = {
    newPath: '',
    selectedEntry: '',
    movePath: '',
    canEditNewPath: false,
    canSelectEntry: false,
    canEditMovePath: false,
    status: 'Open a managed workspace to manage its entries.',
  };
  workspaceChangesEditorState = {
    selectedFile: '',
    canSelectFile: false,
    editorText: '',
    canEditText: false,
    canLoadFile: false,
    canPreserveEdit: false,
    canSavePrivate: false,
    editState: 'No file open',
    editVersion: '',
  };
  workspaceChangesQueueState = {
    scanLabel: 'Find folder changes',
    scanState: 'idle',
    canScan: false,
    saveAllLabel: 'Save all privately',
    savingAll: false,
    canSaveAllPrivate: false,
    missingSource: '',
    moveTarget: '',
    canChooseStructural: false,
    canRecordStructural: false,
    structuralHint: WORKSPACE_STRUCTURAL_CHANGE_HINT,
  };
  workspaceDestinationPlanText = '';
  workspaceDestinationSelectedFile = '';
  workspaceDestinationCanEdit = false;
  workspaceDestinationHint = 'Save a file privately before updating another folder from it.';
  workspaceDestinationDraft = '';
}

function folderCandidates(workspace = model.workspace) {
  return [
    ...(workspace?.file_histories || []).map((history) => ({
      path: history.path,
      command: 'inspect_managed_file',
    })),
    ...(workspace?.native_untracked_files || []).map((path) => ({
      path,
      command: 'inspect_native_file',
    })),
  ];
}

function unsupportedNativeKind(kind) {
  if (kind === 'symbolic-link') return 'symbolic link';
  if (kind === 'excluded-ancestor') return 're-included below an excluded parent';
  return 'special native entry';
}

function folderChangePresentation(change) {
  const projection = change.native_unsupported
    ? { code: '?', status: 'Unsupported', description: unsupportedNativeKind(change.unsupported_kind), detail: change.unsupported_kind === 'excluded-ancestor' ? 'include its parent too or keep this path excluded' : 'convert or remove before saving or review' }
    : change.native_directory
    ? { code: 'A', status: 'Added', description: 'new native folder', detail: 'saved parent-first with this reviewed queue' }
    : change.native_missing
    ? { code: 'D', status: 'Deleted or moved', description: 'missing tracked file', detail: 'choose rename or deletion below' }
    : change.native_untracked
    ? { code: 'A', status: 'Added', description: 'new native file', detail: `${formatBytes(change.byte_count)} bytes` }
    : { code: 'M', status: 'Modified', description: 'changed tracked file', detail: `${formatBytes(change.byte_count)} bytes` };
  return Object.freeze({ path: change.path, ...projection });
}

function reconcileWorkspaceChangesQueueState() {
  const changes = model.folderChanges;
  const scanState = changes.length
    ? 'changes'
    : workspaceChangesQueueState.scanState === 'changes'
      ? 'idle'
      : workspaceChangesQueueState.scanState;
  const missing = changes.filter((change) => change.native_missing);
  const unsupported = changes.filter((change) => change.native_unsupported);
  workspaceChangesQueueState.canSaveAllPrivate = changes.length > 0
    && missing.length === 0
    && unsupported.length === 0
    && !managedWorkspaceMutationBlocked()
    && !workspaceInteractionInFlight()
    && !workspaceChangesQueueState.savingAll
    && !workspaceChangesEditorState.canPreserveEdit;
  workspaceChangesQueueState.missingSource = missing.some(
    (change) => change.path === workspaceChangesQueueState.missingSource,
  )
    ? workspaceChangesQueueState.missingSource
    : missing[0]?.path || '';
  const selected = missing.find(
    (change) => change.path === workspaceChangesQueueState.missingSource,
  ) || missing[0];
  const candidates = selected ? changes.filter((change) => change.native_untracked) : [];
  workspaceChangesQueueState.moveTarget = candidates.some(
    (change) => change.path === workspaceChangesQueueState.moveTarget,
  )
    ? workspaceChangesQueueState.moveTarget
    : '';
  const structural = missing.length > 0;
  workspaceChangesQueueState.canChooseStructural = structural
    && !managedWorkspaceMutationBlocked()
    && !workspaceInteractionInFlight();
  workspaceChangesQueueState.canRecordStructural = structural
    && !managedWorkspaceMutationBlocked()
    && !editorDraftPending()
    && !workspaceInteractionInFlight();
  return Object.freeze({
    changes,
    missing,
    selected,
    candidates,
    scanState,
    queueSummary: changes.length
      ? `${changes.length} native ${changes.length === 1 ? 'change' : 'changes'} ready`
      : scanState === 'scanning'
        ? 'Checking native folder…'
        : scanState === 'clean'
          ? 'Folder matches private history'
          : scanState === 'error'
            ? 'Folder check incomplete'
            : 'Folder not checked yet',
  });
}

function renderFolderChanges() {
  reconcileWorkspaceChangesQueueState();
  renderReviews();
}

function installWorkspaceSnapshot(workspace, checkpoint, explicitOpen = false) {
  const previousWorkspace = model.workspace;
  const priorExportTarget = workspaceDestinationDraft;
  const preserveTypedExportTarget = Boolean(
    !explicitOpen
    && priorExportTarget
    && previousWorkspace?.root === workspace.root
    && previousWorkspace?.installation === workspace.installation
  );
  const observedApprovalTransition = Boolean(
    !explicitOpen
    && previousWorkspace
    && previousWorkspace.root === workspace.root
    && previousWorkspace.installation === workspace.installation
    && previousWorkspace.shared_version !== workspace.shared_version
    && typeof workspace.shared_version === 'string'
    && workspace.shared_version.length > 0
  );
  // Editor and one-shot restore state belong to one physical workspace, not merely to a relative
  // path or object-shaped dropdown value. Two projects can legitimately contain byte-identical
  // `shared.txt` entries, so preserving these objects across a root change would display one
  // project's unsaved draft or undo affordance inside the other project.
  // A pathname can be removed and reused for a different physical workspace. The record-fold
  // digest is part of the verified workspace identity, so root equality alone cannot carry an
  // unsaved draft or one-shot restore authority into that replacement generation.
  // An explicit open installs a newly admitted directory object. An exact clone can reuse both
  // the pathname and every record byte, so root + fold digest cannot distinguish it from the
  // previously opened physical workspace. Refreshes may preserve drafts for the current admitted
  // object; explicit open/import must revoke them unconditionally.
  if (
    explicitOpen ||
    model.workspace?.root !== workspace.root ||
    model.workspace?.digest !== workspace.digest ||
    model.workspace?.installation !== workspace.installation
  ) {
    clearWorkspaceScopedState();
  }
  model.workspace = workspace;
  model.checkpoint = checkpoint;
  if (checkpoint.native_folder === true) {
    if (typeof checkpoint.native_folder_path !== 'string' || !checkpoint.native_folder_path) {
      throw new Error('Mesh did not return the exact native workspace folder it verified.');
    }
    model.agentFolder = { path: checkpoint.native_folder_path };
  } else if (checkpoint.native_folder === false) {
    model.agentFolder = null;
  } else {
    // Older test doubles and older native hosts do not expose this additive field. Retain the
    // previous conservative behavior for them: only an existing stable link proves that the
    // displayed root is already a presented native folder.
    model.agentFolder = model.activeFolder?.path ? { path: workspace.root } : null;
  }
  model.workspaceVerified = true;
  restoreAgentHandoffForWorkspace(workspace, { preserveCurrent: true });
  if (model.recent?.remembered === workspace.root && model.recent?.export_root) {
    model.exportRoot = model.recent.export_root;
    workspaceDestinationDraft = preserveTypedExportTarget ? priorExportTarget : model.exportRoot;
  } else if (preserveTypedExportTarget) {
    workspaceDestinationDraft = priorExportTarget;
  }
  // Approval and an optional Git export are separate durable effects. Recent-workspace state
  // records the exact shared version last fully reconciled to the original folder. That lets a
  // restart, a lost approval reply, or another local client recover the guided update without
  // guessing from timestamps or treating a partial export as complete.
  const recentEntry = recentWorkspaceEntries().find((entry) => entry.path === workspace.root);
  const originalUpdatePending = Boolean(
    typeof workspace.shared_version === 'string'
    && workspace.shared_version.length > 0
    && recentEntry?.originalUpdateKnown
    && recentEntry?.originalUpdateVersion !== workspace.shared_version
  );
  if (
    (observedApprovalTransition || originalUpdatePending)
    && isOriginalProjectDestination(model.exportRoot)
  ) {
    model.pullBackPrompt = {
      root: workspace.root,
      installation: workspace.installation,
      version: workspace.shared_version,
    };
  }
  setLocalService('ready');
}

function nativeCaptureModeDescription() {
  if (!model.nativeCaptureAvailable) {
    return 'Automatic private save is unavailable in this build. Review-first mode remains active.';
  }
  if (!model.nativeCaptureEnabled) {
    return 'Review-first mode is active. Mesh notices native edits but waits for Save privately.';
  }
  if (workspaceInstallationMatchesHandoff()) {
    return 'Automatic private save is on but paused for this assigned agent folder. Finish the handoff after every process stops.';
  }
  return 'Automatic private save is on while Mesh is running, including with its window hidden. A complete stable scan signs safe file and folder changes; ambiguous structure still waits for review.';
}

function nativeFolderHintPresentation() {
  const activeNativeFolder = model.activeFolder?.path || null;
  const agentFolder = model.agentFolder?.path || null;
  return agentFolder
    ? `${activeNativeFolder ? 'Use the stable folder in Finder or an editor when it should follow the workspace selected in Mesh. ' : 'The stable Finder shortcut needs attention, but the exact native workspace folder remains available. '}Start Codex on this version and Open agent terminal assign this independent real folder, so a running agent stays in that writable workspace if Mesh later retargets the stable link. Do not share one writable folder between agents; Start another agent copy creates a fresh independent copy of the current saved point. Reopen tools that keep an old directory handle after switching. ${nativeCaptureModeDescription()}`
    : 'Create a native working folder before opening this workspace in an editor, Terminal, or agent. Mesh will materialize the saved version as an independent ordinary folder and create the stable folder that follows later version switches.';
}

function currentWorkspaceState() {
  const data = model.workspace;
  if (!data) return 'Working';
  const pending = pendingReviewWork();
  return !model.workspaceVerified
    || managedMutationRecoveryBlocked()
    || pending.inventoryIncomplete
    ? 'Needs attention'
    : model.checkpoint?.working
      || pending.nativeCount > 0
      || pending.editorDraft
      || model.editor?.native_untracked
      || model.editor?.modified_from_current_version
      ? 'Working'
      : currentSavedPointApproved()
        ? 'Approved'
        : currentRecordedReviewReady(data)
          ? 'Ready for review'
          : data.private_version?.version
            ? 'Saved privately'
            : 'Working';
}

function previousWorkspaceActionPresentation(entries = recentWorkspaceEntries()) {
  const previous = previousWorkspacePath();
  const previousEntry = previous ? recentWorkspaceEntry(previous) : null;
  const previousProject = previousEntry
    ? projectDisplayName(previousEntry.projectRoot, entries)
    : null;
  return Object.freeze({
    label: previous
      ? `Return to ${workspaceDisplayName(
        previous,
        previousProject,
        previousEntry.sourcePointOrdinal,
      )}`
      : 'Return to previous workspace',
    title: previous || '',
    visible: Boolean(previous),
    enabled: !workspaceInteractionInFlight() && Boolean(previous),
  });
}

function currentWorkspaceActionPresentation() {
  const interactionBlocked = workspaceInteractionInFlight();
  const activeNativeFolder = model.activeFolder?.path || null;
  const agentFolder = model.agentFolder?.path || null;
  const handedToAgent = workspaceInstallationMatchesHandoff();
  const navigationRecoveryRequired = currentWorkspaceNavigationMissing();
  const pending = pendingReviewWork();
  const inspectionBlocked = pending.inventoryIncomplete;
  const draftPending = editorDraftPending();
  const diagnosticsAvailable = Boolean(model.workspaceVerified && supportBundleAvailable());
  const upgrade = nativeFolderUpgradeCandidate();
  const pendingWorkspaceWork = Boolean(pending.nativeCount || pending.editorDraft || pending.inventoryIncomplete);
  const upgradeBlocked = Boolean(upgrade && pendingWorkspaceWork);
  const revealLabel = agentFolder
    ? 'Open working folder'
    : upgrade
      ? 'Create native working folder'
      : 'Choose workspace version';
  const codexLabel = handedToAgent
    ? 'Reopen assigned Codex folder'
    : activeNativeFolder
      ? 'Start Codex on this version'
      : upgrade
        ? 'Create folder + start Codex'
        : 'Start Codex on this version';
  const codexTitle = navigationRecoveryRequired
    ? 'Connect this workspace to its original folder before opening it in Codex.'
    : draftPending
      ? 'Save, copy, or revert the open editor draft before handing this folder to Codex.'
    : handedToAgent
      ? 'This exact folder was already handed to an agent. Use Start another agent copy for concurrent work; reopen only after the prior agent stops.'
      : 'Assign this version’s fixed real folder to one Codex session before opening it.';
  const terminalLabel = activeNativeFolder
    ? handedToAgent ? 'Reopen agent terminal' : 'Open agent terminal'
    : upgrade
      ? 'Create folder + open terminal'
      : 'Open agent terminal';
  const terminalTitle = navigationRecoveryRequired
    ? 'Connect this workspace to its original folder before opening an agent terminal.'
    : draftPending
      ? 'Save, copy, or revert the open editor draft before handing this folder to an agent terminal.'
    : handedToAgent
      ? 'This exact folder was already handed to an agent. Use Start another agent copy for concurrent work; reopen only after the prior agent stops.'
      : '';
  const revealDisabled = interactionBlocked
    || nativeWorkspaceLaunchInFlight
    || !model.workspace
    || !model.workspaceVerified
    || navigationRecoveryRequired
    || upgradeBlocked
    || (!agentFolder && !upgrade && !(model.workspace.workspace_versions || []).length);
  const switchVersionDisabled = interactionBlocked
    || !model.workspace
    || !model.workspaceVerified
    || !(model.workspace.workspace_versions || []).length;
  const codexDisabled = interactionBlocked
    || nativeWorkspaceLaunchInFlight
    || !model.workspace
    || !model.workspaceVerified
    || navigationRecoveryRequired
    || inspectionBlocked
    || draftPending
    || upgradeBlocked
    || (!agentFolder && !upgrade);
  const completeVersion = currentWorkspaceVersion();
  const concurrentVersionChoice = !completeVersion && hasConcurrentWorkspaceHistory();
  const agentStartBlocked = managedWorkspaceWriteBlocked() || pendingWorkspaceWork;
  const startAgentDisabled = interactionBlocked
    || nativeWorkspaceLaunchInFlight
    || !model.workspace
    || !model.workspaceVerified
    || navigationRecoveryRequired
    || agentStartBlocked
    || (!completeVersion && !concurrentVersionChoice);
  const startAgentTitle = nativeWorkspaceLaunchInFlight
    ? 'Wait for the current folder or agent to open.'
    : interactionBlocked
      ? 'Wait for the current workspace change to finish.'
    : !model.workspace
      ? 'Open a workspace before starting an agent.'
      : !model.workspaceVerified
        ? 'Refresh and verify this workspace before starting an agent.'
        : navigationRecoveryRequired
          ? 'Connect this workspace to its original folder before starting an agent.'
        : agentStartBlocked
          ? 'Save or resolve the pending workspace work before starting another agent.'
          : concurrentVersionChoice
            ? 'Choose the exact saved workspace from concurrent history that this new agent should receive; Mesh will not guess or combine branches.'
          : !completeVersion
            ? 'Save one complete private workspace point before starting another agent.'
            : agentFolder
              ? 'Create a fresh independent folder for one additional long-running agent.'
              : 'Create the first isolated native folder and open it in Codex.';
  const terminalDisabled = interactionBlocked
    || nativeWorkspaceLaunchInFlight
    || !model.workspace
    || !model.workspaceVerified
    || navigationRecoveryRequired
    || inspectionBlocked
    || draftPending
    || upgradeBlocked
    || (!agentFolder && !upgrade);
  const finishDisabled = interactionBlocked
    || nativeWorkspaceLaunchInFlight
    || !agentFolder
    || !handedToAgent;
  const updateCopy = workspaceUpdateCopy();
  // The primary workspace loop always returns imported work to its verified original project.
  // A remembered alternate export remains available in the destination section, but must not
  // silently replace this one-click original-folder route.
  const updateLabel = workspaceProjectRoot()
    ? 'Update original folder'
    : updateCopy.button;
  const originalUpdateBlocked = originalProjectUpdateBlocked();
  const updateDisabled = interactionBlocked
    || !model.workspace
    || managedWorkspaceWriteBlocked()
    || handedToAgent
    || originalUpdateBlocked;
  const updateTitle = handedToAgent
    ? 'Choose Finish agent handoff after every process using this folder has stopped, then save and review its complete result before updating the original folder.'
    : originalUpdateBlocked
      ? 'Record a review and approve the current saved version before updating the original project folder.'
      : '';
  const recentEntries = recentWorkspaceEntries();
  const previous = previousWorkspaceActionPresentation(recentEntries);
  const rollbackDisabled = interactionBlocked
    || !model.workspace
    || managedWorkspaceWriteBlocked()
    || handedToAgent;
  const rollbackTitle = handedToAgent
    ? 'Choose Finish agent handoff after every process using this folder has stopped before rolling back the managed copy.'
    : '';
  return Object.freeze([
    Object.freeze({ id: 'refresh', label: '↻', enabled: !interactionBlocked, title: 'Refresh' }),
    Object.freeze({ id: 'open-folder', label: revealLabel, enabled: !revealDisabled, title: navigationRecoveryRequired ? 'Connect this workspace to its original folder before opening its stable working path.' : '' }),
    Object.freeze({ id: 'open-version', label: 'Open saved version', enabled: !switchVersionDisabled, title: '' }),
    Object.freeze({ id: 'start-codex', label: codexLabel, enabled: !codexDisabled, title: codexTitle }),
    Object.freeze({ id: 'start-agent-copy', label: concurrentVersionChoice ? 'Choose another agent version' : 'Start another agent copy', enabled: !startAgentDisabled, title: startAgentTitle }),
    Object.freeze({ id: 'update-destination', label: updateLabel, enabled: !updateDisabled, title: updateTitle }),
    Object.freeze({ id: 'finish-agent', label: 'Finish agent handoff', enabled: !finishDisabled, title: handedToAgent ? 'Clear this folder’s collision warning only after every agent and terminal using it has stopped.' : '', visible: handedToAgent }),
    Object.freeze({ id: 'return-workspace', label: previous.label, enabled: previous.enabled, title: previous.title, visible: previous.visible }),
    Object.freeze({ id: 'open-terminal', label: terminalLabel, enabled: !terminalDisabled, title: terminalTitle }),
    Object.freeze({ id: 'copy-diagnostics', label: 'Copy safe diagnostics', enabled: !interactionBlocked && diagnosticsAvailable, title: diagnosticsAvailable ? 'Copies only bounded recovery facts. File contents, paths, configuration, event history, and keys are excluded.' : 'Refresh the workspace before copying its safe diagnostic summary.' }),
    Object.freeze({ id: 'copy-working-path', label: 'Copy working path', enabled: !(interactionBlocked || !model.workspace || !model.workspaceVerified || navigationRecoveryRequired || !activeNativeFolder), title: navigationRecoveryRequired ? 'Connect this workspace to its original folder before copying its stable working path.' : '' }),
    Object.freeze({ id: 'copy-agent-path', label: handedToAgent ? 'Agent folder assigned' : 'Copy agent path', enabled: !(interactionBlocked || !model.workspace || !model.workspaceVerified || navigationRecoveryRequired || !agentFolder || inspectionBlocked || draftPending || handedToAgent), title: navigationRecoveryRequired ? 'Connect this workspace to its original folder before handing it to an agent.' : draftPending ? 'Save, copy, or revert the open editor draft before handing this folder to an agent.' : handedToAgent ? 'This exact folder is already assigned. Use Start another agent copy for concurrent work, or Finish agent handoff after the current agent stops.' : '' }),
    Object.freeze({ id: 'rollback', label: 'Roll back managed copy', enabled: !rollbackDisabled, title: rollbackTitle }),
  ]);
}

function currentWorkspacePresentation() {
  const data = model.workspace;
  if (!data) return null;
  const privateVersion = privateVersionPresentation(data);
  const sharedVersion = sharedVersionPresentation(data);
  const actions = currentWorkspaceActionPresentation()
    .filter((action) => action.visible !== false)
    .map(({ id, label, enabled }) => Object.freeze({ id, label, enabled }));
  const conditions = [
    ...data.conditions.map((item) => `${item.message} (${item.code})`),
    ...data.not_yet.map((item) => `${item.subject}: ${item.reason}`),
    ...(!model.workspaceVerified ? ['The current workspace state could not be verified. Refresh before making another managed change.'] : []),
    ...(model.nativeInspectionFailed ? [nativeInspectionFailureCondition()] : []),
  ];
  const entries = (data.entries.length ? data.entries : [{ path: 'No materialized paths', type: '' }])
    .map((entry) => `${entry.path}${entry.type ? ` · ${entry.type}` : ''}`);
  const recentEntries = recentWorkspaceEntries();
  const workspaces = recentEntries.map((entry) => {
    const current = entry.path === data.root;
    const agentAssigned = Boolean(entry.agentHandoffInstallation);
    return Object.freeze({
      path: entry.path,
      label: workspaceDisplayName(
        entry.path,
        projectDisplayName(entry.projectRoot, recentEntries),
        entry.sourcePointOrdinal,
      ),
      state: current ? 'current' : agentAssigned ? 'agent-assigned' : 'available',
      canOpen: !current && !workspaceInteractionInFlight(),
    });
  });
  const liveChanges = model.agentLive?.changes || [];
  const liveState = workspaceInstallationMatchesHandoff()
    ? model.agentLive?.state || 'scanning'
    : 'idle';
  const liveSummary = liveState === 'idle'
    ? 'No agent currently owns this workspace.'
    : liveState === 'scanning'
      ? 'Reading the assigned folder without saving it…'
      : liveState === 'error'
        ? 'Mesh could not read the assigned folder. The handoff remains active; retry by returning to Mesh.'
        : liveChanges.length
          ? `${liveChanges.length} live ${liveChanges.length === 1 ? 'change' : 'changes'} detected. These remain unsaved until Finish agent handoff.`
          : 'The assigned folder currently matches its saved starting point.';
  return Object.freeze({
    state: currentWorkspaceState(),
    recordSummary: `${data.records} durable record${data.records === 1 ? '' : 's'}`,
    workingFolder: model.activeFolder?.path || (model.agentFolder?.path ? 'Unavailable — retry Open working folder' : 'Open a saved version as a new native folder'),
    agentFolderLabel: model.agentFolder?.path ? 'Pinned agent folder' : 'Private workspace storage',
    agentFolder: data.root,
    agentAssigned: workspaceInstallationMatchesHandoff(),
    nativeFolderHint: nativeFolderHintPresentation(),
    privateVersion: privateVersion.text,
    privateVersionTitle: privateVersion.title || 'No exact private version is available.',
    sharedVersion: sharedVersion.text,
    sharedVersionTitle: sharedVersion.title || 'No exact shared version is available.',
    destination: workspaceProjectRoot() || model.exportRoot || 'Choose a destination in Update destination',
    entryCount: data.entries.length,
    entries: Object.freeze(entries),
    conditions: Object.freeze(conditions),
    agentActivity: Object.freeze({
      state: liveState,
      summary: liveSummary,
      changes: Object.freeze(liveChanges.map((change) => Object.freeze({
        path: change.path,
        kind: change.kind,
      }))),
    }),
    workspaces: Object.freeze(workspaces),
    actions: Object.freeze(actions),
  });
}

function renderWorkspaceActions() {
}

const WORKSPACE_VERSION_FOCUS_SELECTOR = '[role="radio"][tabindex="0"]:not(:disabled)';

function reactPageForLegacySection(section) {
  return ({
    'workspace-files-next': 'files',
    'workspace-changes-next': 'changes',
    'review-workbench-next': 'review',
  })[section] || null;
}

function focusReactWorkspacePage(page, selector = null) {
  document.dispatchEvent(new CustomEvent('mesh:workspace-page-request', {
    detail: Object.freeze({ page, selector }),
  }));
}

function focusWorkspaceJourney(section, control, visibleReplacement = null, visibleControl = null) {
  const page = reactPageForLegacySection(section);
  const reactShell = document.getElementById('mesh-app-next');
  if (page && reactShell?.getAttribute('data-mesh-react-shell-active') === 'true') {
    const selectors = Array.isArray(visibleControl) ? visibleControl : [visibleControl];
    const selector = selectors.find((candidate) => typeof candidate === 'string') || null;
    document.dispatchEvent(new CustomEvent('mesh:workspace-page-request', {
      detail: Object.freeze({ page, selector }),
    }));
    return;
  }
  $(section).scrollIntoView?.({ behavior: 'smooth', block: 'start' });
  const replacement = visibleReplacement ? $(visibleReplacement) : null;
  const visibleReplacementActive = replacement && !replacement.classList.contains('hidden');
  const visibleControls = Array.isArray(visibleControl) ? visibleControl : [visibleControl];
  const replacementControl = visibleReplacementActive
    ? visibleControls
        .filter((selector) => typeof selector === 'string')
        .map((selector) => replacement.shadowRoot?.querySelector?.(selector))
        .find(Boolean)
    : null;
  const target = replacementControl || (visibleReplacementActive ? replacement : $(control));
  // Once a React workbench commits, its established controls remain source-owned but are hidden.
  // Focusing one of those controls silently drops keyboard focus in WebKit. Make the visible,
  // labelled island host programmatically focusable so section navigation always lands in the
  // interface the person can actually see; the established control remains the fallback.
  if (target === replacement && target.getAttribute('tabindex') === null) {
    target.setAttribute('tabindex', '-1');
  }
  target.focus?.({ preventScroll: true });
}

function focusWorkspaceVersionJourney(operation = null) {
  const exactSelector = typeof operation === 'string' && /^[0-9a-f]{64}$/u.test(operation)
    ? `[data-mesh-version-operation="${operation}"]:not(:disabled)`
    : null;
  document.dispatchEvent(new CustomEvent('mesh:workspace-page-request', {
    detail: Object.freeze({
      page: 'versions',
      selector: exactSelector || WORKSPACE_VERSION_FOCUS_SELECTOR,
    }),
  }));
}

function currentReviewItem() {
  const current = currentWorkspaceVersion();
  return current
    ? (model.workspace?.review_items || []).find((item) => item.subject_operation === current.operation) || null
    : null;
}

function currentSavedPointApproved() {
  const review = currentReviewItem();
  return Boolean(
    review
    && review.recorded !== false
    && review.content_complete === true
    && typeof review.reviewed_head === 'string'
    && review.reviewed_head.length > 0
    && model.workspace?.shared_version === review.reviewed_head
  );
}

function originalProjectUpdateBlocked(targetRoot = workspaceProjectRoot()) {
  return Boolean(targetRoot && isOriginalProjectDestination(targetRoot) && !currentSavedPointApproved());
}

function explicitPrivateExportAvailable() {
  const hasSavedTree = Boolean(
    (model.workspace?.file_histories || []).length
    || (model.workspace?.entries || []).some((entry) => entry.type === 'folder'),
  );
  return Boolean(
    hasSavedTree
    && model.workspaceVerified
    && !managedWorkspaceMutationBlocked(),
  );
}

function rememberedWorkspaceNeedsImport(workspace) {
  const conditions = workspace?.conditions || [];
  return Boolean(
    workspace
    && workspace.records === 0
    && !workspace.private_version?.version
    && (workspace.entries || []).length === 0
    && (workspace.workspace_versions || []).length === 0
    && (workspace.native_untracked_files || []).length === 0
    && (workspace.native_unsupported_entries || []).length === 0
    && conditions.every((condition) => condition.code === 'unversioned-native-content')
    && !model.checkpoint?.working,
  );
}

function rememberedWorkspaceHasUnversionedContent(workspace) {
  return Boolean(workspace?.conditions?.some((condition) => condition.code === 'unversioned-native-content'));
}

function isProtectedSameFolderImport(source, summary) {
  return Boolean(
    source
    && summary
    && model.workspaceVerified
    && rememberedWorkspaceNeedsImport(model.workspace)
    && model.preview?.source_scope === 'open-zero-history-workspace'
    && model.preview?.summary === summary
    && source === model.workspace?.root
  );
}

function continueInNativeFolderAction() {
  return {
    title: 'Continue in the native folder',
    description: model.activeFolder?.path
      ? 'Use the stable working folder with Finder or an editor. It follows the version you open in Mesh; agent sessions use the independent real folder and stay pinned if you switch versions.'
      : 'The independent folder is verified. Repair or reopen the stable working-folder shortcut before handing it to Finder or an editor.',
    label: 'Open working folder',
    page: 'current',
    currentAction: 'open-folder',
    activate: true,
  };
}

function startWorkingWithAgentAction() {
  return {
    title: 'Start Codex on this saved version',
    description: 'Mesh assigns this fixed real folder to one Codex session before opening it. The agent stays pinned here if you switch the stable working folder to another saved version.',
    label: 'Start Codex on this version',
    page: 'current',
    currentAction: 'start-codex',
    activate: true,
  };
}

function activeAgentHandoffAction() {
  return {
    title: 'Agent folder is assigned',
    description: 'Create the task in the opened agent, then keep this exact writable folder assigned while the agent or any related terminal is running. After every process using it stops, choose Finish agent handoff; Mesh will inspect the complete result and privately save unambiguous file and folder changes.',
    label: 'Finish agent handoff',
    page: 'current',
    currentAction: 'finish-agent',
    // The large NEXT action opens the same explicit confirmation as the workspace control. It cannot
    // release custody by navigation alone, but the person no longer has to find a second button.
    activate: true,
  };
}

function workspaceInstallationMatchesHandoff(workspace = model.workspace) {
  return Boolean(
    model.workspaceVerified
    && workspace
    && model.agentHandoff
    && model.agentHandoff.root === workspace.root
    && model.agentHandoff.installation === workspace.installation
  );
}

function rememberAgentHandoff(binding, generation = null) {
  model.agentHandoff = {
    root: binding.root,
    installation: binding.installation,
    generation: canonicalAgentHandoffGeneration(generation),
  };
  model.agentLive = null;
  renderNextAction();
  void inspectLiveAgentWork();
}

function agentHandoffMatches(expected) {
  return Boolean(
    expected
    && workspaceInstallationMatchesHandoff()
    && model.agentHandoff.root === expected.root
    && model.agentHandoff.installation === expected.installation
    && model.agentHandoff.generation !== null
    && model.agentHandoff.generation === expected.generation
  );
}

function confirmAssignedAgentFolderReopen(agent, attempt) {
  const codex = agent === 'Codex';
  return requestAccessibleConfirmation({
    title: codex
      ? 'Reopen this assigned Codex folder?'
      : 'Reopen this assigned agent terminal?',
    description: `This exact writable folder was already handed to an agent. Two agents in one folder can overwrite each other. Use Start another agent copy for concurrent work. Reopen this folder only if the prior agent has stopped.${codex ? ' Reopen assigned Codex folder?' : ' Reopen an agent terminal?'}`,
    confirmLabel: codex ? 'Reopen Codex folder' : 'Reopen agent terminal',
    cancelLabel: 'Keep agent assigned',
    tone: 'destructive',
    isCurrent: () => agentHandoffMatches(attempt) && !editorDraftPending() && !workspaceInteractionInFlight(),
    staleMessage: 'The agent assignment changed while confirmation was open. Mesh kept the current assignment active. Refresh and review it again.',
  });
}

function createNativeWorkspaceAction() {
  const run = importWorkbenchNextActions.get('confirm-import') || null;
  return {
    title: 'Create your native workspace',
    description: 'The source summary is verified. Create the private working copy; the original folder stays untouched.',
    label: 'Create workspace',
    page: 'import',
    activate: false,
    run,
    disabled: run === null,
  };
}

function currentWorkspaceNavigationMissing() {
  const root = model.workspace?.root;
  if (!root) return false;
  const entry = recentWorkspaceEntries().find((candidate) => candidate.path === root);
  return !entry || (model.checkpoint?.confirmed_import_receipt === true && !entry.projectRoot);
}

function connectCurrentWorkspaceAction() {
  if (model.preview) {
    const run = importWorkbenchNextActions.get('confirm-import') || null;
    return {
      title: 'Connect this workspace to its original folder',
      description: 'The selected original folder is verified. Connect it to this existing managed workspace; exact private origin receipts must agree, and Mesh will not create another copy.',
      label: 'Connect original folder',
      page: 'import',
      activate: false,
      run,
      disabled: run === null,
    };
  }
  const run = importWorkbenchNextActions.get('choose-folder') || null;
  return {
    title: 'Connect this workspace to its original folder',
    description: 'Mesh verified a workspace opened by another local client, but its restart and original-folder navigation are not recorded yet. Choose the original folder; exact origin receipts reuse this managed copy instead of copying it again.',
    label: 'Connect original folder',
    page: 'import',
    activate: false,
    run,
    disabled: run === null,
  };
}

function recommendedNextAction() {
  if (!model.workspace) {
    return model.preview
      ? createNativeWorkspaceAction()
      : {
          title: 'Bring in a folder',
          description: 'Choose an ordinary folder. Mesh verifies it before creating a private working copy.',
          label: 'Choose a folder',
          page: 'import',
          activate: false,
          run: importWorkbenchNextActions.get('choose-folder') || null,
          disabled: !importWorkbenchNextActions.has('choose-folder'),
        };
  }
  if (!model.workspaceVerified) {
    return {
      title: 'Verify the workspace again',
      description: 'Mesh paused every managed write because the current state could not be verified.',
      label: 'Refresh safely',
      page: 'current',
      currentAction: 'refresh',
      activate: true,
    };
  }
  if (managedMutationRecoveryBlocked()) {
    return {
      title: 'Resolve the interrupted change',
      description: 'Preserve the paths listed under Current state, then inspect the recovery condition before continuing.',
      label: 'Review conditions',
      page: 'current',
      activate: false,
    };
  }
  // Active custody has one exact inspection path: Finish agent handoff. Ordinary folder scans
  // are intentionally refused by the native authority while the agent still owns the folder.
  // Keep this recovery action ahead of inventoryIncomplete so a failed finish preflight never
  // points the person at a command that must fail.
  if (workspaceInstallationMatchesHandoff()) return activeAgentHandoffAction();
  if (pendingReviewWork().inventoryIncomplete) {
    return {
      title: 'Finish checking the native folder',
      description: 'Mesh could not inspect every native folder entry and will not use an incomplete view for another change or agent. Retry the scan after fixing folder access.',
      label: 'Find folder changes',
      section: 'workspace-changes-next',
      control: 'workspace-changes-next',
      visibleReplacement: 'workspace-changes-next',
      visibleControl: '[data-mesh-work-action="scan-files"]',
      activate: false,
      run: requestNativeFolderScan,
    };
  }
  if (editorDraftPending()) {
    return {
      title: 'Preserve the edit in this window',
      description: 'This text exists only in Mesh right now. Preserve it before opening a version or another workspace.',
      label: 'Return to the edit',
      section: 'workspace-changes-next',
      control: 'workspace-changes-next',
      visibleReplacement: 'workspace-changes-next',
      visibleControl: 'textarea',
      activate: false,
    };
  }
  // Once an external process owns this physical folder, do not lead the person back through the
  // moving stable link or into a possibly partial scan/save. The exact handoff remains in custody
  // until the person confirms every process has stopped; that command performs the authoritative
  // post-agent inspection before the normal review journey resumes.
  const pendingNativeCount = pendingReviewWork().nativeCount;
  if (pendingNativeCount > 0) {
    const scanned = model.folderChanges.length > 0;
    const structural = model.folderChanges.some((change) => change.native_missing);
    const unsupported = model.folderChanges.some((change) => change.native_unsupported)
      || (model.workspace?.native_unsupported_entries || []).length > 0;
    const exclusionConflict = model.folderChanges.some((change) => change.unsupported_kind === 'excluded-ancestor')
      || (model.workspace?.native_unsupported_entries || []).some((entry) => entry.kind === 'excluded-ancestor');
    return {
      title: unsupported
        ? 'Resolve the unsupported native entry'
        : structural
        ? 'Identify the moved or deleted file'
        : `Review ${pendingNativeCount} native ${pendingNativeCount === 1 ? 'change' : 'changes'}`,
      description: !scanned
        ? 'Mesh knows newer native work exists. Read the exact folder bytes before creating another workspace or agent.'
        : unsupported
        ? exclusionConflict
          ? 'Mesh found a path re-included below an excluded parent. Include the parent too or keep the descendant excluded, then scan again.'
          : 'Mesh found a symbolic link or special filesystem entry it will not follow or save. Convert it to an ordinary file or folder, or remove it, then scan again.'
        : structural
        ? 'Mesh found a tracked path missing from the folder and will not guess whether it moved or was deleted.'
        : 'Mesh found exact native changes but has not saved them. Inspect the queue before adding them to private history.',
      label: !scanned ? 'Find changes' : unsupported ? 'Resolve entry' : structural ? 'Resolve file identity' : 'Inspect changes',
      section: 'workspace-changes-next',
      control: 'workspace-changes-next',
      visibleReplacement: 'workspace-changes-next',
      visibleControl: !scanned
        ? '[data-mesh-work-action="scan-files"]'
        : unsupported
          ? '[data-mesh-native-queue]'
          : structural
            ? '[data-mesh-work-field="missingSource"]'
            : '[data-mesh-work-action="save-all-private"]',
      activate: false,
      run: !scanned ? requestNativeFolderScan : null,
    };
  }
  if (currentWorkspaceNavigationMissing()) return connectCurrentWorkspaceAction();
  if (rememberedWorkspaceNeedsImport(model.workspace)) {
    if (model.preview) return createNativeWorkspaceAction();
    const hasOrdinaryContent = rememberedWorkspaceHasUnversionedContent(model.workspace);
    if (hasOrdinaryContent) {
      return {
        title: 'Preserve this folder',
        description: 'Mesh already verified this folder and found ordinary files outside private history. Preview that exact folder, then confirm the new private working copy; the current folder stays unchanged.',
        label: 'Preview this folder',
        page: 'import',
        activate: false,
        run: previewCurrentWorkspaceImport,
      };
    }
    const run = importWorkbenchNextActions.get('choose-folder') || null;
    return {
      title: 'Bring in a folder',
      description: 'This remembered workspace has no saved work. Choose an ordinary folder to create a useful private workspace.',
      label: 'Choose a folder',
      page: 'import',
      activate: false,
      run,
      disabled: run === null,
    };
  }
  if (pendingOriginalPullBack()) return originalPullBackAction();
  if (!model.agentFolder?.path) {
    const upgrade = nativeFolderUpgradeCandidate();
    const concurrentHistory = hasConcurrentWorkspaceHistory();
    return {
      title: upgrade ? 'Create the native working folder' : 'Choose a saved workspace point',
      description: upgrade
        ? 'Mesh will materialize the exact saved point as an ordinary writable folder and connect the stable working-folder shortcut.'
        : concurrentHistory
          ? 'Mesh found more than one current line of saved work. Choose one exact saved workspace to open independently; Mesh will not guess or combine them.'
          : 'A complete saved point is required before Mesh can create a safe native working folder.',
      label: upgrade ? 'Create working folder' : 'Choose saved version',
      page: upgrade ? 'current' : 'versions',
      ...(upgrade ? { currentAction: 'open-folder' } : {}),
      activate: Boolean(upgrade),
      ...(!upgrade ? { run: () => focusWorkspaceVersionChoice() } : {}),
    };
  }
  const current = currentWorkspaceVersion();
  const review = currentReviewItem();
  // The first imported point is the trustworthy starting line, not a demand to approve the
  // person's pre-existing project before they have used Mesh. Keep Review available, but lead the
  // first successful session into the native workspace. The first later saved point advances to
  // the review step through the ordinary path below.
  if (
    current?.ordinal === 1
    && (model.workspace.workspace_versions || []).length === 1
    && model.exportRoot
  ) {
    return startWorkingWithAgentAction();
  }
  if (current && !review) {
    return {
      title: 'Review the saved version',
      description: 'Inspect the exact saved changes, then record that review before deciding whether to share the version.',
      label: 'Inspect review',
      section: 'review-workbench-next',
      control: 'review-workbench-next',
      activate: false,
    };
  }
  if (review?.recorded === false) {
    return {
      title: 'Record the reviewed version',
      description: 'The automatic card is ready. Inspect its exact change summary before recording this version for approval.',
      label: 'Inspect review',
      section: 'review-workbench-next',
      control: 'review-workbench-next',
      activate: false,
    };
  }
  if (review && model.workspace.shared_version !== review.reviewed_head) {
    if (approvalEnrollmentInFlight) {
      return {
        title: 'Finish approval setup',
        description: 'Complete or cancel the native setup dialog. Mesh will not start a second approval ceremony or replace its result with an older status read.',
        label: 'Setting up approvals…',
        section: 'review-workbench-next',
        control: 'review-workbench-next',
        activate: false,
        disabled: true,
      };
    }
    return model.approval?.enrolled === true
      ? {
          title: 'Approve the reviewed version',
          description: 'The review is recorded. Approval still requires explicit confirmation and macOS user presence.',
          label: 'Review approval',
          section: 'review-workbench-next',
          control: 'review-workbench-next',
          activate: false,
        }
      : explicitPrivateExportAvailable()
        ? {
            title: 'Export a private copy',
            description: 'Approval is unavailable in this build. Choose a different ordinary folder for an explicit private export; Mesh will keep the original project unchanged.',
            label: 'Choose export folder',
            page: 'update',
            activate: true,
            run: choosePrivateExportTarget,
          }
      : model.approval?.available !== true
        ? {
            title: 'Approval unavailable in this build',
            description: model.approval?.unavailable_reason || APPROVAL_STATUS_UNVERIFIED,
            label: 'Approval unavailable',
            section: 'review-workbench-next',
            control: 'review-workbench-next',
            activate: false,
            disabled: true,
          }
        : {
          title: 'Set up local approval',
          description: 'Create a device-only credential before this reviewed version can become the shared version.',
          label: 'Set up approvals',
          section: 'review-workbench-next',
          control: 'review-workbench-next',
          activate: false,
        };
  }
  return continueInNativeFolderAction();
}

function nativeInspectionFailureCondition() {
  return workspaceInstallationMatchesHandoff()
    ? 'Mesh could not complete the exact agent-finish inspection. Keep the folder assigned and retry Finish agent handoff after fixing folder access.'
    : 'Mesh could not inspect every native candidate. Find folder changes must succeed before making another managed change or starting an agent.';
}

function refuseOrdinaryInspectionDuringAgentHandoff({ automatic = false } = {}) {
  if (!workspaceInstallationMatchesHandoff()) return false;
  if (!automatic) {
    showNotice('This folder is still assigned to an agent. Ordinary inspection is paused; Finish agent handoff performs the exact complete inspection while custody remains active.', true);
  }
  return true;
}

const AGENT_LIVE_CHANGE_KINDS = Object.freeze([
  'modified-file', 'new-file', 'new-folder', 'missing-file', 'unsupported',
]);

function validateAgentLiveInspection(value, binding) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('Mesh received an invalid live agent-work inspection.');
  }
  const keys = Object.keys(value).sort();
  const expected = [
    'agent_handoff_generation', 'changes', 'schema', 'workspace_digest',
    'workspace_installation', 'workspace_root',
  ].sort();
  if (keys.length !== expected.length || keys.some((key, index) => key !== expected[index])) {
    throw new Error('Mesh received an unrecognized live agent-work inspection.');
  }
  if (value.schema !== 'mesh.agent-live-work/v1'
    || value.workspace_root !== binding.root
    || value.workspace_digest !== binding.digest
    || value.workspace_installation !== binding.installation
    || value.agent_handoff_generation !== binding.generation
    || !Array.isArray(value.changes)
    || value.changes.length > 10_000) {
    throw new Error('The live agent-work inspection did not match this exact assigned workspace.');
  }
  const seen = new Set();
  const changes = value.changes.map((candidate) => {
    if (!candidate || typeof candidate !== 'object' || Array.isArray(candidate)
      || Object.keys(candidate).sort().join(',') !== 'kind,path'
      || typeof candidate.path !== 'string'
      || !candidate.path
      || candidate.path.length > 4_096
      || /[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(candidate.path)
      || !AGENT_LIVE_CHANGE_KINDS.includes(candidate.kind)) {
      throw new Error('Mesh received an invalid live agent-work entry.');
    }
    const identity = `${candidate.kind}\u0000${candidate.path}`;
    if (seen.has(identity)) throw new Error('The live agent-work inspection repeated an entry.');
    seen.add(identity);
    return Object.freeze({ path: candidate.path, kind: candidate.kind });
  });
  return Object.freeze(changes);
}

function agentLiveInspectionStillCurrent(binding) {
  return Boolean(
    model.workspaceVerified
    && model.workspace?.root === binding.root
    && model.workspace?.digest === binding.digest
    && model.workspace?.installation === binding.installation
    && model.agentHandoff?.generation === binding.generation
    && workspaceInstallationMatchesHandoff()
  );
}

async function inspectLiveAgentWork() {
  if (agentLiveInspectionInFlight || !workspaceInstallationMatchesHandoff()) return false;
  const generation = canonicalAgentHandoffGeneration(model.agentHandoff?.generation);
  if (!generation || !model.workspaceVerified || !model.workspace) return false;
  const binding = Object.freeze({
    root: model.workspace.root,
    digest: model.workspace.digest,
    installation: model.workspace.installation,
    generation,
    sequence: ++agentLiveInspectionSequence,
  });
  agentLiveInspectionInFlight = true;
  if (!model.agentLive) {
    model.agentLive = Object.freeze({ state: 'scanning', changes: Object.freeze([]) });
    renderWorkspaceCurrentNext();
    renderReviews();
  }
  try {
    const result = JSON.parse(await invoke('inspect_agent_live_work', {
      expectedWorkspaceRoot: binding.root,
      expectedWorkspaceDigest: binding.digest,
      expectedWorkspaceInstallation: binding.installation,
      expectedAgentHandoffGeneration: binding.generation,
    }));
    if (binding.sequence !== agentLiveInspectionSequence || !agentLiveInspectionStillCurrent(binding)) return false;
    const changes = validateAgentLiveInspection(result, binding);
    const next = Object.freeze({ state: 'ready', changes });
    if (JSON.stringify(model.agentLive) !== JSON.stringify(next)) {
      model.agentLive = next;
      renderWorkspaceCurrentNext();
      renderReviews();
    }
    return true;
  } catch (error) {
    if (binding.sequence !== agentLiveInspectionSequence || !agentLiveInspectionStillCurrent(binding)) return false;
    const next = Object.freeze({ state: 'error', changes: Object.freeze([]) });
    if (JSON.stringify(model.agentLive) !== JSON.stringify(next)) {
      model.agentLive = next;
      renderWorkspaceCurrentNext();
      renderReviews();
    }
    return false;
  } finally {
    agentLiveInspectionInFlight = false;
  }
}

function renderNextAction() {
  nextActionSnapshot = freezeRecommendedAction(recommendedNextAction());
  renderWorkspaceOverviewNext();
  renderWorkspaceChromeNext();
  renderWorkspaceEntryNext();
  renderWorkspaceCurrentNext();
  renderWorkspaceFilesChangesNext();
}

function freezeRecommendedAction(action) {
  return Object.freeze({
    title: action.title,
    description: action.description,
    label: action.label,
    section: action.section,
    control: action.control,
    page: action.page || null,
    currentAction: action.currentAction || null,
    visibleReplacement: action.visibleReplacement || null,
    visibleControl: action.visibleControl || null,
    activate: Boolean(action.activate),
    run: typeof action.run === 'function' ? action.run : null,
    disabled: Boolean(action.disabled),
  });
}

function recommendedActionKey(action) {
  return projectionSurfaceContinuityKey('recommended-action', 'target', {
    title: action.title,
    description: action.description,
    label: action.label,
    section: action.section,
    control: action.control,
    page: action.page,
    currentAction: action.currentAction,
    visibleReplacement: action.visibleReplacement,
    visibleControl: action.visibleControl,
    activate: action.activate,
    run: Boolean(action.run),
    disabled: action.disabled,
  });
}

function recommendedActionEnabled(action) {
  if (action.disabled || workspaceInteractionInFlight() || nativeWorkspaceLaunchInFlight) return false;
  if (!action.activate) return true;
  if (action.currentAction) return currentWorkspaceActionIsEnabled(action.currentAction);
  if (action.run) return true;
  const control = $(action.control);
  return Boolean(control && !control.disabled);
}

function overviewWorkspaceAuthority() {
  const workspace = model.workspace;
  return workspace
    ? Object.freeze({
        sequence: workspaceVerificationSequence,
        root: workspace.root,
        digest: workspace.digest,
        installation: workspace.installation,
        verified: model.workspaceVerified,
      })
    : null;
}

function overviewWorkspaceAuthorityIsCurrent(authority) {
  return Boolean(
    authority
    && workspaceVerificationSequence === authority.sequence
    && model.workspaceVerified === authority.verified
    && model.workspace?.root === authority.root
    && model.workspace?.digest === authority.digest
    && model.workspace?.installation === authority.installation
  );
}

function activateProjectedRecommendation(authority) {
  const current = freezeRecommendedAction(recommendedNextAction());
  if (
    workspaceOverviewNextPending?.authority !== authority
    || workspaceOverviewNextMounted?.generation !== authority.generation
    || workspaceOverviewNextPending.generation !== authority.generation
    || !overviewWorkspaceAuthorityIsCurrent(authority.workspace)
    || recommendedActionKey(current) !== authority.targetKey
    || current.run !== authority.target.run
    || !recommendedActionEnabled(current)
  ) {
    showNotice('That recommended action is no longer available for this exact workspace. Refresh and inspect the current recommendation before trying again.', true);
    return;
  }
  const {
    section,
    control,
    page,
    currentAction,
    visibleReplacement,
    visibleControl,
    activate,
    run,
  } = authority.target;
  if (page) focusReactWorkspacePage(page);
  else focusWorkspaceJourney(section, control, visibleReplacement, visibleControl);
  if (run) return run();
  if (activate && currentAction) {
    return activateProjectedCurrentAction(authority.currentActionAuthority, currentAction);
  }
  if (activate) return $(control).click?.();
}

const WORKSPACE_WORK_ACTION_CONTROLS = Object.freeze([
  Object.freeze({ id: 'create-text', label: 'Create text file', source: true }),
  Object.freeze({ id: 'create-folder', label: 'Create folder', source: true }),
  Object.freeze({ id: 'move-entry', label: 'Move or rename', source: true }),
  Object.freeze({ id: 'delete-entry', label: 'Delete entry', source: true }),
  Object.freeze({ id: 'open-entry', label: 'Open', source: true }),
  Object.freeze({ id: 'reveal-entry', label: 'Reveal', source: true }),
  Object.freeze({ id: 'open-workspace-folder', label: 'Open folder', source: true }),
  Object.freeze({ id: 'scan-files', label: 'Find folder changes', source: true }),
  Object.freeze({ id: 'load-file', label: 'Open file', source: true }),
  Object.freeze({ id: 'preserve-edit', label: 'Preserve edit', source: true }),
  Object.freeze({ id: 'save-private', label: 'Save privately', source: true }),
  Object.freeze({ id: 'save-all-private', label: 'Save all privately', source: true }),
  Object.freeze({ id: 'record-structural-change', label: 'Record move or deletion', source: true }),
]);

function renderWorkspaceFilesChangesNext(interactionGeneration = workspaceWorkFieldEchoGeneration, interactionField = null) {
  const data = model.workspace;
  if (!workspaceWorkNextAvailable || !data || rememberedWorkspaceNeedsImport(data)) {
    workspaceWorkNextPending = null;
    workspaceWorkNextMounted = null;
    workspaceWorkNextActions = new Map();
    installWorkspaceWorkNextVisibility();
    return;
  }
  const missingPaths = new Set(model.folderChanges
    .filter((change) => change.native_missing)
    .map((change) => change.path));
  const fileChoices = [
    ...(data.file_histories || [])
      .filter((history) => !missingPaths.has(history.path))
      .map((history) => Object.freeze({ value: history.path, label: history.path })),
    ...(data.native_untracked_files || [])
      .map((path) => Object.freeze({ value: path, label: `${path} · new native file` })),
  ];
  const queue = reconcileWorkspaceChangesQueueState();
  const { missing, selected: selectedMissing, candidates: moveCandidates } = queue;
  const actionClosures = new Map();
  const projectedActions = WORKSPACE_WORK_ACTION_CONTROLS.map((spec) => {
    const control = spec.source ? null : $(spec.control);
    const enabled = spec.source ? workspaceFileActionEnabled(spec.id) : !control.disabled;
    const sourceAction = {
      'create-text': createManagedTextEntry,
      'create-folder': createManagedFolderEntry,
      'move-entry': moveManagedEntry,
      'delete-entry': deleteManagedEntry,
      'open-entry': () => openManagedWorkspaceEntry('open-entry'),
      'reveal-entry': () => openManagedWorkspaceEntry('reveal-entry'),
      'open-workspace-folder': async () => {
        if (!beginNativeWorkspaceLaunch()) return false;
        try {
          await revealCurrentWorkspaceFolder();
          showNotice('Opened the current workspace folder in Finder.');
          return true;
        } finally {
          finishNativeWorkspaceLaunch();
        }
      },
      'load-file': inspectSelectedManagedFile,
      'preserve-edit': preserveManagedEditorText,
      'save-private': saveInspectedFilePrivately,
      'scan-files': requestNativeFolderScan,
      'save-all-private': saveAllPrivateChanges,
      'record-structural-change': recordSelectedStructuralChange,
    }[spec.id];
    if (enabled) actionClosures.set(`activate:${spec.id}`, sourceAction);
    return Object.freeze({
      id: spec.id,
      label: spec.id === 'open-entry'
        ? model.workspace?.entries.find((entry) => entry.path === workspaceFilesState.selectedEntry)?.type === 'folder'
          ? 'Open in Finder'
          : 'Open'
        : spec.id === 'scan-files'
        ? workspaceChangesQueueState.scanLabel
        : spec.id === 'save-all-private'
          ? workspaceChangesQueueState.saveAllLabel
          : control?.textContent?.trim() || spec.label,
      enabled,
    });
  });
  const generation = ++workspaceWorkNextGeneration;
  const continuityKey = workspaceProjectionContinuityKey();
  const authority = Object.freeze({
    selectedEntry: new Set(['', ...(data.entries || []).map((entry) => entry.path)]),
    selectedFile: new Set(['', ...fileChoices.map((entry) => entry.value)]),
    missingSource: new Set(missing.map((change) => change.path)),
    moveTarget: new Set(['', ...moveCandidates.map((change) => change.path)]),
  });
  workspaceWorkNextPending = Object.freeze({
    generation,
    authority,
    interactionGeneration,
    interactionField,
    continuityKey,
  });
  if (workspaceWorkNextMounted?.continuityKey !== continuityKey) {
    workspaceWorkNextMounted = null;
  }
  installWorkspaceWorkNextVisibility();
  workspaceWorkNextActions = actionClosures;
  const editorKind = !model.editor
    ? 'none'
    : typeof model.editor.text === 'string'
      ? 'text'
      : 'binary';
  const structural = missing.length
    ? Object.freeze({
        missingSources: Object.freeze(missing.map((change) => Object.freeze({ value: change.path, label: change.path }))),
        missingSource: workspaceChangesQueueState.missingSource,
        moveTargets: Object.freeze(moveCandidates.map((change) => {
          const exact = change.content_digest === selectedMissing.content_digest
            && change.executable === selectedMissing.executable;
          return Object.freeze({
            value: change.path,
            label: `It moved to ${change.path} · ${exact ? 'exact saved bytes' : 'content also changed'}`,
          });
        })),
        moveTarget: workspaceChangesQueueState.moveTarget,
        canChoose: workspaceChangesQueueState.canChooseStructural,
        hint: workspaceChangesQueueState.structuralHint,
      })
    : null;
  appDocument.dispatchEvent(new appWindow.CustomEvent('mesh:workspace-files-changes-projection', {
    detail: Object.freeze({
      generation,
      workbench: Object.freeze({
        files: Object.freeze({
          workspaceLabel: currentWorkspaceDisplayName() || workspaceDisplayName(data.root),
          workspaceRoot: data.root,
          workspaceState: workspaceInstallationMatchesHandoff() ? 'agent-assigned' : 'current',
          entries: Object.freeze((data.entries || []).map((entry) => Object.freeze({
            value: entry.path,
            label: entry.path,
            kind: entry.type,
          }))),
          newPath: workspaceFilesState.newPath,
          selectedEntry: workspaceFilesState.selectedEntry,
          movePath: workspaceFilesState.movePath,
          canEditNewPath: workspaceFilesState.canEditNewPath,
          canSelectEntry: workspaceFilesState.canSelectEntry,
          canEditMovePath: workspaceFilesState.canEditMovePath,
          status: workspaceFilesState.status,
        }),
        changes: Object.freeze({
          files: Object.freeze(fileChoices),
          selectedFile: workspaceChangesEditorState.selectedFile,
          canSelectFile: workspaceChangesEditorState.canSelectFile,
          editorKind,
          editorText: editorKind === 'text' ? workspaceChangesEditorState.editorText : '',
          baselineText: editorKind === 'text' ? workspaceEditorBaselineText(model.editor) : '',
          baselineAvailable: editorKind === 'text' && workspaceEditorBaselineAvailable(model.editor),
          canEditText: editorKind === 'text' && workspaceChangesEditorState.canEditText,
          editState: workspaceChangesEditorState.editState,
          editVersion: workspaceChangesEditorState.editVersion,
          scanState: queue.scanState,
          queueSummary: queue.queueSummary,
          queue: Object.freeze(model.folderChanges.map(folderChangePresentation)),
          autoSaveChecked: model.nativeCaptureEnabled,
          autoSaveEnabled: model.nativeCaptureAvailable
            && !model.nativeCaptureChanging
            && !workspaceInteractionInFlight(),
          autoSaveHint: nativeCaptureModeDescription(),
          structural,
        }),
        actions: Object.freeze(projectedActions),
      }),
    }),
  }));
}

function renderWorkspaceChromeNext() {
  if (!workspaceChromeNextAvailable) return;
  const generation = ++workspaceChromeNextGeneration;
  const workspaceReady = Boolean(model.workspace && !rememberedWorkspaceNeedsImport(model.workspace));
  const nextWorkspaceIdentityKey = workspaceProjectionContinuityKey();
  if (nextWorkspaceIdentityKey !== workspaceChromeIdentityKey) {
    workspaceChromeIdentityKey = nextWorkspaceIdentityKey;
    workspaceChromeIdentity += 1;
  }
  const continuityKey = projectionSurfaceContinuityKey(
    nextWorkspaceIdentityKey,
    'chrome',
    { workspaceReady },
  );
  workspaceChromeNextPending = Object.freeze({ generation, workspaceReady, continuityKey });
  if (workspaceChromeNextMounted?.continuityKey !== continuityKey) {
    workspaceChromeNextMounted = null;
  }
  installWorkspaceChromeNextVisibility();
  appDocument.dispatchEvent(new appWindow.CustomEvent('mesh:workspace-chrome-projection', {
    detail: Object.freeze({
      generation,
      workspaceIdentity: workspaceChromeIdentity,
      chrome: Object.freeze({
        serviceState: localService.state,
        serviceLabel: localService.label,
        workspaceReady,
        nativeChangeCount: model.folderChanges.length,
      }),
    }),
  }));
}

function renderWorkspaceEntryNext(interactionGeneration = null, interactionKind = null) {
  if (!workspaceEntryNextAvailable) return;
  const generation = ++workspaceEntryNextGeneration;
  const workspace = model.workspace;
  const needsImport = rememberedWorkspaceNeedsImport(workspace);
  const workspaceReady = Boolean(workspace) && !needsImport;
  const entries = recentWorkspaceEntries();
  const recent = recentWorkspacePresentation(entries);
  const selectedRecentPath = recent.selectedPath;
  const openPath = workspaceEntryManagedPathDraft;
  const continuityKey = workspaceProjectionContinuityKey();
  workspaceEntryNextPending = Object.freeze({
    generation,
    interactionGeneration,
    interactionKind,
    openPath,
    selectedRecentPath,
    continuityKey,
  });
  if (workspaceEntryNextMounted?.continuityKey !== continuityKey) {
    workspaceEntryNextMounted = null;
  }
  installWorkspaceEntryNextVisibility();
  const actions = new Map();
  actions.set('set-disclosure', (intent, mountedGeneration) => {
    if (workspaceEntryActionKey(intent) !== 'set-disclosure'
      || workspaceEntryDisclosureOpen === intent.open) return;
    workspaceEntryDisclosureOpen = intent.open;
    renderWorkspaceEntryNext(mountedGeneration, 'disclosure');
  });
  const previewsCurrentFolder = Boolean(
    needsImport
    && workspace
    && rememberedWorkspaceHasUnversionedContent(workspace)
  );
  const entryActionBlocked = workspaceInteractionInFlight();
  const retryBlocked = entryActionBlocked
    || workspaceEntrySelectionInFlight > 0
    || workspaceEntryRefreshInFlight;
  if (!entryActionBlocked) {
    actions.set('choose-folder', () => {
      return previewsCurrentFolder
        ? previewCurrentWorkspaceImport()
        : chooseSourceFolder(Object.freeze({
            verificationSequence: workspaceVerificationSequence,
            continuityKey,
          }));
    });
  }
  if (!entryActionBlocked) {
    actions.set('choose-managed-folder', () => chooseManagedWorkspaceFromEntry(continuityKey));
  }
  if (!retryBlocked) {
    actions.set('retry', retryWorkspaceEntryState);
  }
  const canEditManagedPath = !entryActionBlocked;
  if (canEditManagedPath) {
    actions.set('update-managed-path', (intent, mountedGeneration) => {
      if (workspaceEntryActionKey(intent) !== 'update-managed-path') return;
      workspaceEntryManagedPathDraft = intent.path;
      renderWorkspaceEntryNext(mountedGeneration, 'managed-path');
    });
  }
  if (canEditManagedPath && openPath.length > 0) {
    actions.set('open-managed-path', (intent) => {
      if (workspaceEntryActionKey(intent) !== 'open-managed-path'
        || intent.path !== openPath
        || workspaceEntryManagedPathDraft !== openPath) return;
      return openManagedWorkspaceFromEntry(openPath, continuityKey);
    });
  }
  for (const entry of entries) {
    if (recent.canSelect) {
      actions.set(`select-recent:${entry.path}`, (intent, mountedGeneration) => {
        if (workspaceEntryActionKey(intent) !== `select-recent:${entry.path}`) return;
        if (workspaceInteractionInFlight()
          || !recentWorkspaceEntries().some((candidate) => candidate.path === entry.path)) return;
        selectedRecentWorkspacePath = entry.path;
        renderWorkspaceEntryNext(mountedGeneration, 'recent-selection');
      });
    }
    if (entry.path === selectedRecentPath && recent.canOpen) {
      actions.set(`open-recent:${entry.path}`, () => openSelectedRecentWorkspace(entry.path));
    }
    if (entry.path === selectedRecentPath && recent.canForget) {
      actions.set(`forget-recent:${entry.path}`, () => forgetSelectedRecentWorkspace(entry.path));
    }
  }
  workspaceEntryNextActions = actions;
  const presentation = workspaceEntryPresentation(workspace);
  appDocument.dispatchEvent(new appWindow.CustomEvent('mesh:workspace-entry-projection', {
    detail: Object.freeze({
      generation,
      entry: Object.freeze({
        mode: workspaceReady ? 'ready' : needsImport ? 'needs-import' : 'empty',
        eyebrow: presentation.eyebrow,
        title: presentation.title,
        description: presentation.description,
        disclosureLabel: presentation.disclosureLabel,
        disclosureOpen: workspaceEntryDisclosureOpen,
        chooseLabel: previewsCurrentFolder ? 'Preview this folder' : presentation.chooseLabel,
        canChoose: actions.has('choose-folder'),
        canChooseManaged: actions.has('choose-managed-folder'),
        retryLabel: localService.state === 'attention' ? 'Retry verification' : 'Refresh',
        canRetry: actions.has('retry'),
        openPath,
        canEditPath: canEditManagedPath,
        canOpenPath: actions.has('open-managed-path'),
        recents: Object.freeze(entries.map((entry) => {
          const projectName = projectDisplayName(entry.projectRoot, entries);
          const name = workspaceDisplayName(entry.path, projectName, entry.sourcePointOrdinal);
          return Object.freeze({
            path: entry.path,
            label: entry.projectRoot ? name : `${name} — ${entry.path}`,
            state: entry.path === workspace?.root
              ? 'current'
              : unavailableRecentWorkspacePaths.has(entry.path)
                ? 'unavailable'
              : entry.agentHandoffInstallation
                ? 'agent-assigned'
                : 'available',
          });
        })),
        selectedRecentPath,
        canSelectRecent: recent.canSelect,
        recentHint: recent.hint,
        recentOpenLabel: recent.openLabel,
        canOpenRecent: actions.has(`open-recent:${selectedRecentPath}`),
        canForgetRecent: actions.has(`forget-recent:${selectedRecentPath}`),
        forgetRecentTitle: recent.forgetTitle,
      }),
    }),
  }));
}

function renderWorkspaceOverviewNext() {
  const data = model.workspace;
  if (!workspaceOverviewNextAvailable || !data || rememberedWorkspaceNeedsImport(data)) {
    workspaceOverviewNextPending = null;
    workspaceOverviewNextMounted = null;
    workspaceOverviewNextActions = new Map();
    installWorkspaceOverviewNextVisibility();
    return;
  }
  const generation = ++workspaceOverviewNextGeneration;
  const current = currentWorkspacePresentation();
  const currentActionAuthority = currentWorkspaceActionAuthority('overview', generation);
  const next = nextActionSnapshot || freezeRecommendedAction(recommendedNextAction());
  const workspace = overviewWorkspaceAuthority();
  const canSwitchVersion = Boolean(
    workspace?.verified === true
    && (model.workspace?.workspace_versions || []).length > 1
    && !workspaceInteractionInFlight()
    && !nativeWorkspaceLaunchInFlight
  );
  const actions = new Map();
  let authority = null;
  actions.set('recommended', () => activateProjectedRecommendation(authority));
  if (canSwitchVersion) {
    actions.set('open-another-version', async () => {
      try {
        await focusWorkspaceVersionChoice({ preferEarlier: true });
      } catch (error) {
        showNotice(`Mesh could not prepare the version switch: ${error}`, true);
      }
    });
  }
  const currentActionEnabled = (id) => current.actions.some((action) => action.id === id && action.enabled);
  if (currentActionEnabled('open-folder')) {
    actions.set('open-folder', () => activateProjectedCurrentAction(currentActionAuthority, 'open-folder'));
  }
  if (workspaceChangesQueueState.canScan) actions.set('find-changes', requestNativeFolderScan);
  actions.set('open-review', () => focusWorkspaceJourney('review-workbench-next', 'review-workbench-next'));
  if (currentActionEnabled('copy-diagnostics')) {
    actions.set('copy-diagnostics', () => activateProjectedCurrentAction(currentActionAuthority, 'copy-diagnostics'));
  }
  if (currentActionEnabled('return-workspace') && previousWorkspacePath()) {
    actions.set('return-workspace', () => activateProjectedCurrentAction(currentActionAuthority, 'return-workspace'));
  }
  const continuityKey = workspaceProjectionContinuityKey();
  const interactionKey = projectionSurfaceContinuityKey(
    workspaceProjectionContinuityKey(),
    'overview-actions',
    {
      actions: [...actions.keys()].sort(),
      previousWorkspace: previousWorkspacePath(),
      recommended: {
        title: next.title,
        description: next.description,
        label: next.label,
        section: next.section,
        control: next.control,
        visibleReplacement: next.visibleReplacement || null,
        visibleControl: next.visibleControl || null,
        activate: Boolean(next.activate),
        run: typeof next.run === 'function',
        disabled: Boolean(next.disabled),
      },
    },
  );
  authority = Object.freeze({
    generation,
    workspace,
    currentActionAuthority,
    target: next,
    targetKey: recommendedActionKey(next),
  });
  workspaceOverviewNextPending = Object.freeze({ generation, continuityKey, interactionKey, authority });
  if (workspaceOverviewNextMounted?.continuityKey !== continuityKey) {
    workspaceOverviewNextMounted = null;
  }
  installWorkspaceOverviewNextVisibility();
  workspaceOverviewNextActions = actions;
  // The stable native navigation path normally ends in `current`; it is a switch handle, not
  // project identity. Keep that path visible as the working-folder fact, while deriving the
  // prominent heading from the exact remembered project/version binding.
  const workspaceName = currentWorkspaceDisplayName() || workspaceDisplayName(data.root);
  appDocument.dispatchEvent(new appWindow.CustomEvent('mesh:workspace-overview-projection', {
    detail: Object.freeze({
      generation,
      overview: Object.freeze({
        workspaceName,
        state: current.state,
        workingFolder: current.workingFolder,
        recordSummary: current.recordSummary,
        privateVersion: current.privateVersion,
        sharedVersion: current.sharedVersion,
        nativeChangeCount: pendingReviewWork().nativeCount,
        savedVersionCount: (data.workspace_versions || []).length,
        nextActionTitle: next.title,
        nextActionDescription: next.description,
        nextActionLabel: next.label,
        nextActionDisabled: !recommendedActionEnabled(next),
        canOpenAnotherVersion: actions.has('open-another-version'),
        canOpenFolder: actions.has('open-folder'),
        canFindChanges: actions.has('find-changes'),
        canOpenReview: true,
        canReturnWorkspace: actions.has('return-workspace'),
        canCopyDiagnostics: actions.has('copy-diagnostics'),
      }),
    }),
  }));
}

const CURRENT_WORKSPACE_ACTIONS = Object.freeze({
  refresh: activateCurrentRefresh,
  'open-folder': activateCurrentOpenFolder,
  'open-version': activateCurrentOpenVersion,
  'start-codex': activateCurrentStartCodex,
  'start-agent-copy': activateCurrentStartAgentCopy,
  'update-destination': activateCurrentUpdateDestination,
  'finish-agent': activateCurrentFinishAgent,
  'return-workspace': activateCurrentReturnWorkspace,
  'open-terminal': activateCurrentOpenTerminal,
  'copy-diagnostics': activateCurrentCopyDiagnostics,
  'copy-working-path': activateCurrentCopyWorkingPath,
  'copy-agent-path': activateCurrentCopyAgentPath,
  rollback: activateCurrentRollback,
});

function currentWorkspaceActionAuthority(surface, generation) {
  const workspace = model.workspace;
  if (!workspace) return null;
  return Object.freeze({
    surface,
    generation,
    sequence: workspaceVerificationSequence,
    root: workspace.root,
    digest: workspace.digest,
    installation: workspace.installation,
    verified: model.workspaceVerified,
    custodyGeneration: workspaceInstallationMatchesHandoff()
      ? canonicalAgentHandoffGeneration(model.agentHandoff?.generation)
      : null,
  });
}

function currentWorkspaceActionAuthorityIsCurrent(authority) {
  const pending = authority?.surface === 'current'
    ? workspaceCurrentNextPending
    : authority?.surface === 'overview'
      ? workspaceOverviewNextPending
      : null;
  const currentCustodyGeneration = workspaceInstallationMatchesHandoff()
    ? canonicalAgentHandoffGeneration(model.agentHandoff?.generation)
    : null;
  return Boolean(
    authority
    && pending?.generation === authority.generation
    && workspaceVerificationSequence === authority.sequence
    && model.workspaceVerified === authority.verified
    && model.workspace?.root === authority.root
    && model.workspace?.digest === authority.digest
    && model.workspace?.installation === authority.installation
    && currentCustodyGeneration === authority.custodyGeneration
  );
}

function currentWorkspaceActionIsEnabled(actionId) {
  const action = currentWorkspaceActionPresentation().find((candidate) => candidate.id === actionId);
  return Boolean(action?.enabled && action.visible !== false);
}

async function activateProjectedCurrentAction(authority, actionId) {
  const action = CURRENT_WORKSPACE_ACTIONS[actionId];
  if (
    typeof action !== 'function'
    || !currentWorkspaceActionAuthorityIsCurrent(authority)
    || !currentWorkspaceActionIsEnabled(actionId)
  ) {
    showNotice('That workspace action is no longer available for this exact workspace. Refresh and inspect Current before trying again.', true);
    return false;
  }
  await action();
  return true;
}

async function activateProjectedWorkspaceSwitch(authority, path) {
  const entry = recentWorkspaceEntries().find((candidate) => candidate.path === path) || null;
  const projected = currentWorkspacePresentation()?.workspaces.find((candidate) => candidate.path === path) || null;
  if (
    !entry
    || !projected?.canOpen
    || !currentWorkspaceActionAuthorityIsCurrent(authority)
    || workspaceInteractionInFlight()
  ) {
    showNotice('That workspace is no longer available to switch. Refresh and choose it again.', true);
    return false;
  }
  const expectedContinuityKey = workspaceProjectionContinuityKey();
  const opened = await openRecentWorkspacePath(
    path,
    () => recentWorkspaceEntries().some((candidate) => candidate.path === path)
      && workspaceProjectionContinuityKey() === expectedContinuityKey,
    'The workspace list changed while Mesh was opening that folder. Review the current workspace and choose again.',
  );
  if (opened && entry.agentHandoffInstallation && workspaceInstallationMatchesHandoff()) {
    focusReactWorkspacePage('current');
    showNotice('Opened the exact workspace assigned to a running agent. Live changes are read-only until you finish that handoff.');
    void inspectLiveAgentWork();
  }
  return opened;
}

function renderWorkspaceCurrentNext() {
  const data = model.workspace;
  if (!workspaceCurrentNextAvailable || !data || rememberedWorkspaceNeedsImport(data)) {
    workspaceCurrentNextPending = null;
    workspaceCurrentNextMounted = null;
    workspaceCurrentNextActions = new Map();
    installWorkspaceCurrentNextVisibility();
    return;
  }
  const continuityKey = workspaceProjectionContinuityKey();
  // Explicit Refresh revokes every old action immediately, but it keeps the last verified tree
  // painted while native identity is being rechecked. Replacing the full WebKit layer with the
  // temporary unverified projection produced a visible one-frame tear on macOS screen refresh.
  if (currentRefreshInFlight
    && !model.workspaceVerified
    && workspaceCurrentNextMounted?.continuityKey === continuityKey
    && workspaceCurrentNextPending?.continuityKey === continuityKey) {
    // Keep only the read-only Refresh retry live. Every workspace, folder, agent, and recovery
    // action remains revoked until one exact verification wins.
    workspaceCurrentNextActions = new Map([['refresh', activateCurrentRefresh]]);
    installWorkspaceCurrentNextVisibility();
    return;
  }
  const current = currentWorkspacePresentation();
  const projectionKey = JSON.stringify(current);
  const mountedGeneration = workspaceCurrentNextMounted?.generation ?? null;
  const canUpdateAuthorityWithoutPainting = currentRefreshInFlight > 0
    && Number.isSafeInteger(mountedGeneration)
    && workspaceCurrentNextPending?.generation === mountedGeneration
    && workspaceCurrentNextMounted?.continuityKey === continuityKey
    && workspaceCurrentNextMounted?.projectionKey === projectionKey;
  const generation = canUpdateAuthorityWithoutPainting
    ? mountedGeneration
    : ++workspaceCurrentNextGeneration;
  const authority = currentWorkspaceActionAuthority('current', generation);
  const actionHandlers = new Map();
  for (const action of current.actions) {
    if (action.enabled) {
      actionHandlers.set(action.id, () => activateProjectedCurrentAction(authority, action.id));
    }
  }
  for (const workspace of current.workspaces) {
    if (workspace.canOpen) {
      actionHandlers.set(
        `switch-workspace:${workspace.path}`,
        () => activateProjectedWorkspaceSwitch(authority, workspace.path),
      );
    }
  }
  const interactionKey = projectionSurfaceContinuityKey(
    workspaceProjectionContinuityKey(),
    'current-actions',
    current.actions,
  );
  workspaceCurrentNextPending = Object.freeze({ generation, continuityKey, interactionKey, projectionKey });
  if (workspaceCurrentNextMounted?.continuityKey !== continuityKey) {
    workspaceCurrentNextMounted = null;
  }
  installWorkspaceCurrentNextVisibility();
  workspaceCurrentNextActions = actionHandlers;
  // A successful no-change refresh only renews the exact native action authority. The mounted
  // React tree already represents the same bounded model, so repainting it adds risk and no truth.
  if (canUpdateAuthorityWithoutPainting) return;
  appDocument.dispatchEvent(new appWindow.CustomEvent('mesh:workspace-current-projection', {
    detail: Object.freeze({
      generation,
      current,
    }),
  }));
}

async function revealCurrentWorkspaceFolder() {
  if (!model.agentFolder?.path) return null;
  const binding = captureVerifiedWorkspace();
  const opened = JSON.parse(await invoke('reveal_managed_workspace', {
    expectedWorkspaceRoot: binding.root,
    expectedWorkspaceDigest: binding.digest,
    expectedWorkspaceInstallation: binding.installation,
  }));
  assertVerifiedWorkspace(binding);
  model.activeFolder = opened;
  return opened;
}

async function openManagedWorkspaceEntry(action) {
  const selectedPath = workspaceFilesState.selectedEntry;
  const selected = model.workspace?.entries.find((entry) => entry.path === selectedPath) || null;
  if (!selected || !['file', 'folder'].includes(selected.type)) {
    showNotice('Choose a current file or folder before opening it outside Mesh.', true);
    return false;
  }
  if (selected.type === 'folder' && action !== 'open-entry') {
    showNotice('Folders open directly in Finder. Choose Open in Finder.', true);
    return false;
  }
  if (!beginNativeWorkspaceLaunch()) return false;
  try {
    const binding = captureVerifiedWorkspace();
    const generation = workspaceInstallationMatchesHandoff()
      ? canonicalAgentHandoffGeneration(model.agentHandoff?.generation)
      : null;
    const result = JSON.parse(await invoke('open_managed_workspace_entry', {
      expectedWorkspaceRoot: binding.root,
      expectedWorkspaceDigest: binding.digest,
      expectedWorkspaceInstallation: binding.installation,
      expectedAgentHandoffGeneration: generation,
      relativePath: selected.path,
      entryKind: selected.type,
      action,
    }));
    assertVerifiedWorkspace(binding);
    const keys = result && typeof result === 'object' && !Array.isArray(result)
      ? Object.keys(result).sort()
      : [];
    if (keys.join(',') !== 'action,entry_kind,opened,schema'
      || result.schema !== 'mesh.workspace-entry-open/v1'
      || result.action !== action
      || result.entry_kind !== selected.type
      || result.opened !== true
      || workspaceFilesState.selectedEntry !== selected.path) {
      throw new Error('The native workspace-entry launcher returned a stale or malformed result.');
    }
    showNotice(selected.type === 'folder'
      ? `Opened ${selected.path} in Finder.`
      : action === 'reveal-entry'
        ? `Revealed ${selected.path} in Finder.`
        : `Opened ${selected.path} with its default application.`);
    return true;
  } finally {
    finishNativeWorkspaceLaunch();
  }
}

function legacyRecentWorkspacePaths() {
  if (Array.isArray(model.recent?.workspaces)) {
    return model.recent.workspaces.filter((path) => typeof path === 'string' && path);
  }
  return typeof model.recent?.remembered === 'string' ? [model.recent.remembered] : [];
}

function recentWorkspaceEntries() {
  const paths = legacyRecentWorkspacePaths();
  const details = Array.isArray(model.recent?.workspace_entries)
    ? model.recent.workspace_entries
    : [];
  const detailsByPath = new Map();
  for (const entry of details) {
    if (!entry || typeof entry.path !== 'string' || !paths.includes(entry.path)) continue;
    if (detailsByPath.has(entry.path)) continue;
    detailsByPath.set(entry.path, {
      path: entry.path,
      exportRoot: typeof entry.export_root === 'string' && entry.export_root
        ? entry.export_root
        : null,
      projectRoot: typeof entry.project_root === 'string' && entry.project_root
        ? entry.project_root
        : null,
      agentHandoffInstallation: typeof entry.agent_handoff_installation === 'string'
        && entry.agent_handoff_installation
        ? entry.agent_handoff_installation
        : null,
      agentHandoffGeneration: canonicalAgentHandoffGeneration(entry.agent_handoff_generation),
      sourcePointOrdinal: Number.isSafeInteger(entry.source_point_ordinal)
        && entry.source_point_ordinal > 0
        ? entry.source_point_ordinal
        : null,
      originalUpdateVersion: typeof entry.original_update_version === 'string'
        && entry.original_update_version
        ? entry.original_update_version
        : null,
      originalUpdateKnown: Object.prototype.hasOwnProperty.call(entry, 'original_update_version'),
    });
  }
  return paths.map((path) => detailsByPath.get(path) || {
    path,
    exportRoot: null,
    projectRoot: null,
    agentHandoffInstallation: null,
    agentHandoffGeneration: null,
    sourcePointOrdinal: null,
    originalUpdateVersion: null,
    originalUpdateKnown: false,
  });
}

function canonicalAgentHandoffGeneration(value) {
  return typeof value === 'string'
    && (value === 'legacy-v8' || /^[0-9a-f]{32}$/u.test(value))
    ? value
    : null;
}

function recentWorkspacePaths() {
  return recentWorkspaceEntries().map((entry) => entry.path);
}

function pathParts(path) {
  // Mesh Desktop is macOS-only; a backslash is a valid filename character,
  // not a path separator. Preserve it in every user-visible path label.
  return String(path ?? '').split('/').filter(Boolean);
}

function pathLeaf(path) {
  const parts = pathParts(path);
  let leaf = parts.at(-1) || String(path ?? '') || 'Workspace';
  if (leaf === 'mounts' && parts.length > 1) leaf = parts.at(-2);
  return leaf;
}

function projectDisplayName(projectRoot, entries) {
  if (!projectRoot) return null;
  const roots = [...new Set(entries.map((entry) => entry.projectRoot).filter(Boolean))];
  const parts = pathParts(projectRoot);
  if (!parts.length) return 'Project';
  // This is the person's original project name, not Mesh's generated workspace directory.
  // Preserve a real ".mesh" suffix so distinct original folders never collapse to one label.
  const render = (candidate) => candidate.join('/');
  for (let depth = 1; depth <= parts.length; depth += 1) {
    const candidate = render(parts.slice(-depth));
    const unique = roots.every((root) => root === projectRoot
      || render(pathParts(root).slice(-depth)) !== candidate);
    if (unique) return candidate;
  }
  return render(parts);
}

function workspaceDisplayName(path, projectName = null, sourcePointOrdinal = null) {
  const leaf = pathLeaf(path);
  const managedPoint = /^point-([0-9a-f]{12})(?:-(\d+))?\.mesh$/.exec(leaf);
  if (managedPoint) {
    const point = Number.isSafeInteger(sourcePointOrdinal) && sourcePointOrdinal > 0
      ? `Copy of saved point ${sourcePointOrdinal}`
      : `Copy of saved version ${managedPoint[1].slice(0, 8)}`;
    // The suffix is an app-managed destination collision number, not proof that this checkout was
    // created for an agent. Ordinary version navigation also allocates the next number when an
    // earlier copy contains work that must be preserved.
    const copy = managedPoint[2] ? ` · Working copy ${managedPoint[2]}` : '';
    return `${projectName ? `${projectName} · ` : ''}${point}${copy}`;
  }
  if (projectName) return `${projectName} · Managed workspace`;
  return leaf.replace(/\.mesh$/i, '') || 'Workspace';
}

function currentWorkspaceDisplayName() {
  const current = model.workspace?.root;
  if (!current) return null;
  const entries = recentWorkspaceEntries();
  const entry = entries.find((candidate) => candidate.path === current);
  // Private checkout names are storage details, not project identity. Use a friendly heading only
  // when the native navigation record binds this checkout to its original project; older records
  // retain the honest generic heading until their project identity is learned by Import.
  if (!entry?.projectRoot) return null;
  const projectName = projectDisplayName(entry.projectRoot, entries);
  return workspaceDisplayName(entry.path, projectName, entry.sourcePointOrdinal);
}

function recentWorkspaceEntry(path) {
  return recentWorkspaceEntries().find((entry) => entry.path === path) || {
    path,
    exportRoot: null,
    projectRoot: null,
    sourcePointOrdinal: null,
  };
}

function workspaceProjectRoot() {
  return model.workspace?.root
    ? recentWorkspaceEntry(model.workspace.root).projectRoot
    : null;
}

function isOriginalProjectDestination(destination) {
  return Boolean(destination) && workspaceProjectRoot() === destination;
}

function workspaceUpdateCopy() {
  const original = isOriginalProjectDestination(model.exportRoot);
  return {
    button: !model.exportRoot
      ? 'Choose destination folder'
      : original
        ? 'Update original folder'
        : 'Update destination folder',
    navigation: original ? 'Update original' : 'Update destination',
  };
}

function pendingOriginalPullBack() {
  const prompt = model.pullBackPrompt;
  return Boolean(
    prompt
    && currentSavedPointApproved()
    && model.workspaceVerified
    && model.workspace
    && prompt.root === model.workspace.root
    && prompt.installation === model.workspace.installation
    && prompt.version === model.workspace.shared_version
    && isOriginalProjectDestination(model.exportRoot)
  );
}

async function finishOriginalPullBackIfComplete(targetRoot, preserved) {
  if (
    pendingOriginalPullBack()
    && targetRoot === model.exportRoot
    && preserved.length === 0
  ) {
    const prompt = model.pullBackPrompt;
    const warning = await rememberWorkspace(model.workspace.root, targetRoot, prompt.version);
    if (warning) return warning;
    model.pullBackPrompt = null;
  }
  return null;
}

function originalPullBackAction() {
  return {
    title: 'Update the original folder',
    description: 'Approval advanced the protected shared version but did not change your original folder. Preview the complete saved workspace, then choose exactly what to update.',
    label: 'Preview original update',
    page: 'update',
    activate: true,
    run: previewPendingOriginalPullBack,
  };
}

async function previewPendingOriginalPullBack() {
  const expectedTargetRoot = model.exportRoot;
  if (!pendingOriginalPullBack()
    || typeof expectedTargetRoot !== 'string'
    || expectedTargetRoot.length === 0
    || workspaceDestinationDraft !== expectedTargetRoot
    || !workspaceDestinationActionEnabled('preview-all')) {
    showNotice('The approved original-folder update is no longer current. Refresh and inspect the exact destination again before previewing it.', true);
    return false;
  }
  return previewAllManagedExports({ verifyNativeWork: true, expectedTargetRoot });
}

function previousWorkspacePath() {
  const paths = recentWorkspacePaths();
  const current = model.workspace?.root;
  // The native record is newest-first. A direct "return" is truthful only while its first entry
  // is the exact workspace currently verified in the window; otherwise the advanced selector
  // remains available without inventing navigation history from a stale or partial record.
  return current && paths[0] === current ? paths[1] || null : null;
}

function reconcileSelectedRecentWorkspace(entries = recentWorkspaceEntries()) {
  const paths = entries.map((entry) => entry.path);
  const current = model.workspace?.root;
  selectedRecentWorkspacePath = paths.includes(selectedRecentWorkspacePath)
    ? selectedRecentWorkspacePath
    : paths.find((path) => path !== current) || paths[0] || '';
  return selectedRecentWorkspacePath;
}

function recentWorkspacePresentation(entries = recentWorkspaceEntries()) {
  const selectedPath = reconcileSelectedRecentWorkspace(entries);
  const selected = entries.find((entry) => entry.path === selectedPath) || null;
  const selectedHasAgent = Boolean(selected?.agentHandoffInstallation);
  const selectedIsCurrent = Boolean(selectedPath && selectedPath === model.workspace?.root);
  const selectedIsUnavailable = Boolean(
    selectedPath
    && !selectedIsCurrent
    && unavailableRecentWorkspacePaths.has(selectedPath)
  );
  const interactionBlocked = workspaceInteractionInFlight();
  return Object.freeze({
    selectedPath,
    canSelect: !interactionBlocked && entries.length > 0,
    canOpen: !interactionBlocked && Boolean(selected) && !selectedIsCurrent,
    canForget: !interactionBlocked && Boolean(selected) && !selectedHasAgent && !selectedIsCurrent,
    openLabel: selectedIsUnavailable
      ? 'Try again'
      : selectedHasAgent
        ? 'Switch to finish agent'
        : 'Switch workspace',
    forgetTitle: selectedHasAgent
    ? 'Choose Finish agent handoff after every process using this folder has stopped before removing it from Recent workspaces.'
    : selectedIsCurrent
      ? 'The open workspace must stay remembered so Mesh can reopen it after restart. Switch to another workspace first, or use Roll back managed copy when removal is intended.'
      : '',
    hint: !selected
    ? 'Validated native workspaces appear here and remain available across restarts.'
    : selectedIsUnavailable
      ? 'Mesh could not reopen this shortcut. Restore its saved folder and try again, or choose Forget from list. Forgetting removes only the shortcut.'
      : selectedHasAgent
      ? 'This folder is assigned to an agent. Return to it and choose Finish agent handoff after every process using it has stopped.'
      : selectedIsCurrent
        ? 'This is the open workspace. It stays remembered so Mesh can reopen it after restart. Switch to another workspace first, or use Roll back managed copy when removal is intended.'
        : 'Forgetting removes only this navigation shortcut. The workspace folder and its saved history are not changed.',
  });
}

async function activateCurrentOpenFolder() {
  if (refuseNativeNavigationWithoutOriginalBinding()) return;
  if (!beginNativeWorkspaceLaunch()) return;
  try {
    const upgrade = nativeFolderUpgradeCandidate();
    if (upgrade) {
      await openWorkspaceVersionAsFolder(
        upgrade.operation,
        null,
        {
          allowNativeChanges: false,
          allowUnverified: false,
          blockedAction: 'creating the native working folder',
          withinNativeWorkspaceLaunch: true,
        },
      );
      return;
    }
    if (!model.agentFolder?.path) {
      pendingAgentVersionChoice = false;
      focusWorkspaceVersionJourney();
      showNotice('Choose the saved workspace point now in focus. Mesh will verify it before opening an independent native folder.');
      return;
    }
    const binding = captureVerifiedWorkspace();
    const opened = await revealCurrentWorkspaceFolder();
    renderWorkspace();
    showNotice(`Opened the stable native folder ${opened.path}. It currently names ${opened.workspace_root || binding.root}. Work there normally, then return to Mesh to inspect and save exact changes.`);
  } catch (error) {
    showNotice(String(error), true);
  } finally {
    finishNativeWorkspaceLaunch();
  }
}

async function focusWorkspaceVersionChoice({ preferEarlier = false, forAgent = false } = {}) {
  const versions = model.workspace?.workspace_versions || [];
  pendingAgentVersionChoice = forAgent && versions.length > 0;
  if (pendingAgentVersionChoice) workspaceVersionDestinationDraft = '';
  renderWorkspaceVersionChoices();
  if (!versions.length) {
    showNotice('No saved workspace version is available yet. Save a durable change first.', true);
    return;
  }
  const selectedStillExists = versions.some((version) => (
    version.operation === selectedWorkspaceVersionOperation
  ));
  const currentOperation = currentWorkspaceVersion()?.operation || null;
  // Multiple tips have no truthful implicit "current" or "earlier" choice. The deterministic
  // causal order is useful for rendering, but using its final row as a default would quietly bias
  // the person toward one branch while the surrounding copy promises that Mesh will not guess.
  // Preserve a choice the person already made; otherwise leave the picker empty until they choose.
  if (hasConcurrentWorkspaceHistory() && !selectedStillExists) {
    selectedWorkspaceVersionOperation = '';
    workspaceVersionDestinationDraft = '';
    renderWorkspaceVersionChoices();
    focusWorkspaceVersionJourney();
    showNotice('Choose one exact saved workspace from the concurrent history. Mesh will verify your selection before it can open a working folder or start an agent.');
    return;
  }
  if (!selectedStillExists || (preferEarlier && selectedWorkspaceVersionOperation === currentOperation)) {
    const nearestEarlier = [...versions]
      .reverse()
      .find((version) => version.operation !== currentOperation);
    selectedWorkspaceVersionOperation = (nearestEarlier || versions.at(-1)).operation;
    workspaceVersionDestinationDraft = '';
  }
  const selectedOperation = selectedWorkspaceVersionOperation;
  renderWorkspaceVersionChoices();
  focusWorkspaceVersionJourney(selectedOperation);
  if (!workspaceVersionPreviewStateMatches(model.workspaceVersionPreview, selectedOperation)) {
    await previewSelectedWorkspaceVersion(selectedOperation);
  }
  // Entering the picker may deliberately clear or replace its visible selection before the
  // requested point is chosen. Re-render even when its authenticated preview was already cached;
  // otherwise the buttons recover but the exact saved contents remain visually blank.
  renderWorkspaceVersionChoices();
  if (workspaceVersionPreviewStateMatches(model.workspaceVersionPreview, selectedWorkspaceVersionOperation)) {
    showNotice('Review the verified saved point below, then open it as a working folder or directly in Codex. Your current folder stays untouched.');
  }
}

async function activateCurrentOpenVersion() {
  try {
    await focusWorkspaceVersionChoice();
  } catch (error) {
    showNotice(`Mesh could not prepare the version switch: ${error}`, true);
  }
}

async function activateCurrentCopyDiagnostics() {
  try {
    const binding = captureVerifiedWorkspace();
    const diagnostics = supportBundleClipboardText(model.workspace?.support_bundle);
    if (!appWindow.navigator?.clipboard?.writeText) {
      throw new Error('Clipboard access is unavailable');
    }
    await appWindow.navigator.clipboard.writeText(diagnostics);
    assertVerifiedWorkspace(binding);
    showNotice('Copied safe diagnostics for this exact workspace. The summary excludes file contents, paths, configuration, event history, and keys; paste it into your alpha feedback.');
  } catch (error) {
    showNotice(`Mesh could not copy safe diagnostics. Refresh the workspace and try again. (${error})`, true);
  }
}

async function activateCurrentCopyWorkingPath() {
  if (refuseNativeNavigationWithoutOriginalBinding()) return;
  try {
    if (!model.activeFolder?.path) {
      throw new Error('Create a native working folder before copying the stable path');
    }
    const binding = captureVerifiedWorkspace();
    const displayedWorkingPath = model.activeFolder.path;
    let workingPath;
    try {
      workingPath = await verifiedWorkingFolderPath(binding, displayedWorkingPath);
    } catch (error) {
      // The exact version folder can remain healthy when only the optional navigation link is
      // unavailable. Stop advertising that link without discrediting private workspace state.
      model.activeFolder = null;
      renderWorkspace();
      throw error;
    }
    if (!appWindow.navigator?.clipboard?.writeText) {
      throw new Error('Clipboard access is unavailable');
    }
    await appWindow.navigator.clipboard.writeText(workingPath);
    assertVerifiedWorkspace(binding);
    if (model.activeFolder?.path !== displayedWorkingPath) throw new WorkspaceVerificationSuperseded();
    showNotice(`Copied the stable working path ${workingPath}. It follows the workspace you open in Mesh; reopen tools that keep an older directory handle after switching.`);
  } catch (error) {
    showNotice(`Mesh could not copy the stable working path. Use Open working folder or Refresh before trying again. (${error})`, true);
  }
}

function codexContextMessage(opened) {
  const scope = 'The Mesh tool exposes workspace metadata, never file content or Mesh mutation authority. If you select another workspace in Mesh, this folder remains writable but its optional metadata tool pauses until you return to this assigned workspace.';
  if (opened.mesh_context === 'installed') {
    return ` Mesh launched Codex with private, read-only context for this independent workspace. ${scope}`;
  }
  if (opened.mesh_context === 'refreshed') {
    return ` Mesh refreshed and supplied private, read-only context for this agent session. ${scope}`;
  }
  if (opened.mesh_context === 'ready') {
    return ` Private, launch-scoped read-only Mesh context is ready for this agent session. ${scope}`;
  }
  const warning = typeof opened.mesh_context_warning === 'string'
    ? ` (${opened.mesh_context_warning})`
    : '';
  return ` Mesh preserved your Codex settings, but the optional Mesh context tool was not supplied${warning}. Codex remains usable in the independent folder.`;
}

function gitContextMessage(opened) {
  if (opened.git_context === 'installed' || opened.git_context === 'reused' || opened.git_context === 'ready') {
    return ' Independent Git history and status are ready in this folder; commits and locks are not shared with the original or another agent.';
  }
  if (opened.git_context === 'none' || opened.git_context === 'not-a-git-repository') {
    return '';
  }
  const warning = typeof opened.git_context_warning === 'string'
    ? ` (${opened.git_context_warning})`
    : '';
  return ` Git context is unavailable${warning}; the native files remain usable.`;
}

function assertAgentLaunchResponse(opened, binding, agent) {
  const generation = canonicalAgentHandoffGeneration(opened?.agent_handoff_generation);
  if (
    !opened
    || opened.path !== binding.root
    || opened.workspace_installation !== binding.installation
    || opened.fixed_workspace_path !== true
    || opened.agent_handoff_recorded !== true
    || opened.agent !== agent
    || generation === null
  ) {
    throw new Error(`Mesh did not confirm that ${agent} received this exact verified workspace. The folder remains marked as handed off until you inspect and finish it.`);
  }
  return generation;
}

function refuseAgentLaunchWithIncompleteNativeInspection() {
  if (!pendingReviewWork().inventoryIncomplete) return false;
  focusWorkspaceJourney('workspace-changes-next', 'workspace-changes-next', 'workspace-changes-next', '[data-mesh-work-action="scan-files"]');
  showNotice('Mesh could not inspect every native folder entry. Find folder changes must succeed before this folder can be handed to an agent.', true);
  return true;
}

function refuseNativeNavigationWithoutOriginalBinding() {
  if (!currentWorkspaceNavigationMissing()) return false;
  focusReactWorkspacePage('import');
  showNotice('Connect this workspace to its original folder before opening its stable path or handing it to an agent. Mesh will reuse this exact managed copy only after its private origin receipts prove the relationship.', true);
  return true;
}

async function activateCurrentStartCodex() {
  if (refuseNativeNavigationWithoutOriginalBinding()) return;
  if (refuseAgentLaunchWithEditorDraft()) return;
  if (refuseAgentLaunchWithIncompleteNativeInspection()) return;
  const reopeningAgentFolder = workspaceInstallationMatchesHandoff();
  const expectedAgentHandoffGeneration = reopeningAgentFolder
    ? canonicalAgentHandoffGeneration(model.agentHandoff?.generation)
    : null;
  if (reopeningAgentFolder && expectedAgentHandoffGeneration === null) {
    showNotice('Refresh before reopening this assigned folder. Mesh cannot bind a new agent to an unverified custody generation.', true);
    return;
  }
  const reopenAttempt = reopeningAgentFolder
    ? Object.freeze({
        root: model.agentHandoff.root,
        installation: model.agentHandoff.installation,
        generation: expectedAgentHandoffGeneration,
      })
    : null;
  if (reopeningAgentFolder && !(await confirmAssignedAgentFolderReopen('Codex', reopenAttempt))) {
    showNotice('Kept the existing agent folder unchanged. Use Start another agent copy to create a fresh independent folder for concurrent work.');
    return;
  }
  if (!beginNativeWorkspaceLaunch()) return;
  let upgraded = false;
  let attemptedBinding = null;
  try {
    const upgrade = nativeFolderUpgradeCandidate();
    if (upgrade) {
      upgraded = await openWorkspaceVersionAsFolder(
        upgrade.operation,
        null,
        {
          reveal: false,
          allowNativeChanges: false,
          allowUnverified: false,
          blockedAction: 'creating the native working folder',
          withinNativeWorkspaceLaunch: true,
        },
      );
      if (!upgraded) return;
    }
    const binding = captureVerifiedWorkspace();
    attemptedBinding = binding;
    const opened = JSON.parse(await invoke('open_managed_workspace_in_codex', {
      expectedWorkspaceRoot: binding.root,
      expectedWorkspaceDigest: binding.digest,
      expectedWorkspaceInstallation: binding.installation,
      confirmedReopen: reopeningAgentFolder,
      expectedAgentHandoffGeneration,
    }));
    assertVerifiedWorkspace(binding);
    const handoffGeneration = assertAgentLaunchResponse(opened, binding, 'Codex');
    rememberAgentHandoff(binding, handoffGeneration);
    const context = codexContextMessage(opened);
    const git = gitContextMessage(opened);
    const action = reopeningAgentFolder
      ? 'Reopened'
      : upgraded
        ? 'Created the native working folder and opened'
        : 'Opened';
    showNotice(`${action} ${opened.path} in Codex. Create a new task there and give it your instruction. This agent has a fixed real folder; switching Mesh later will not redirect it. Keep this Mesh assignment active until every related Codex session, terminal, and editor has stopped.${git}${context}`);
  } catch (error) {
    if (attemptedBinding) rememberAgentHandoff(attemptedBinding);
    showNotice(
      upgraded
        ? `The native working folder is ready, but Mesh could not confirm whether Codex opened it: ${error} The folder is marked as handed off; reopen only if the prior launch did not start.`
        : attemptedBinding
          ? `Mesh could not confirm whether Codex opened this folder: ${error} The folder is marked as handed off; reopen only if the prior launch did not start.`
          : String(error),
      true,
    );
  } finally {
    finishNativeWorkspaceLaunch();
  }
}

async function activateCurrentOpenTerminal() {
  if (refuseNativeNavigationWithoutOriginalBinding()) return;
  if (refuseAgentLaunchWithEditorDraft()) return;
  if (refuseAgentLaunchWithIncompleteNativeInspection()) return;
  const reopeningAgentFolder = workspaceInstallationMatchesHandoff();
  const expectedAgentHandoffGeneration = reopeningAgentFolder
    ? canonicalAgentHandoffGeneration(model.agentHandoff?.generation)
    : null;
  if (reopeningAgentFolder && expectedAgentHandoffGeneration === null) {
    showNotice('Refresh before reopening this assigned folder. Mesh cannot bind a new agent to an unverified custody generation.', true);
    return;
  }
  const reopenAttempt = reopeningAgentFolder
    ? Object.freeze({
        root: model.agentHandoff.root,
        installation: model.agentHandoff.installation,
        generation: expectedAgentHandoffGeneration,
      })
    : null;
  if (reopeningAgentFolder && !(await confirmAssignedAgentFolderReopen('Terminal', reopenAttempt))) {
    showNotice('Kept the existing agent folder unchanged. Use Start another agent copy to create a fresh independent folder for concurrent work.');
    return;
  }
  if (!beginNativeWorkspaceLaunch()) return;
  let upgraded = false;
  let attemptedBinding = null;
  try {
    const upgrade = nativeFolderUpgradeCandidate();
    if (upgrade) {
      upgraded = await openWorkspaceVersionAsFolder(
        upgrade.operation,
        null,
        {
          reveal: false,
          allowNativeChanges: false,
          allowUnverified: false,
          blockedAction: 'creating the native working folder',
          withinNativeWorkspaceLaunch: true,
        },
      );
      if (!upgraded) return;
    }
    const binding = captureVerifiedWorkspace();
    attemptedBinding = binding;
    const opened = JSON.parse(await invoke('open_managed_workspace_in_terminal', {
      expectedWorkspaceRoot: binding.root,
      expectedWorkspaceDigest: binding.digest,
      expectedWorkspaceInstallation: binding.installation,
      confirmedReopen: reopeningAgentFolder,
      expectedAgentHandoffGeneration,
    }));
    assertVerifiedWorkspace(binding);
    const handoffGeneration = assertAgentLaunchResponse(opened, binding, 'Terminal');
    rememberAgentHandoff(binding, handoffGeneration);
    const git = gitContextMessage(opened);
    showNotice(`${reopeningAgentFolder ? 'Reopened' : upgraded ? 'Created the native working folder and opened' : 'Opened'} ${opened.path} in Terminal. Start your local agent there; its real folder stays fixed if Mesh switches later.${git}`);
  } catch (error) {
    if (attemptedBinding) rememberAgentHandoff(attemptedBinding);
    showNotice(
      upgraded
        ? `The native working folder is ready, but Mesh could not confirm whether Terminal opened it: ${error} The folder is marked as handed off; reopen only if the prior launch did not start.`
        : attemptedBinding
          ? `Mesh could not confirm whether Terminal opened this folder: ${error} The folder is marked as handed off; reopen only if the prior launch did not start.`
          : String(error),
      true,
    );
  } finally {
    finishNativeWorkspaceLaunch();
  }
}

function exactAgentFinishPreflightKeys(value, keys) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  return actual.length === expected.length
    && actual.every((key, index) => key === expected[index]);
}

function validAgentFinishRelativePath(path) {
  return typeof path === 'string'
    && path.length > 0
    && path.length <= 4_096
    && !path.startsWith('/')
    && !/[\u0000-\u001f\u007f\u202a-\u202e\u2066-\u2069]/u.test(path)
    && !path.split('/').some((part) => part === '' || part === '.' || part === '..');
}

function validateAgentFinishPreflight(answer, binding, releaseAttempt) {
  const topKeys = [
    'schema',
    'workspace_root',
    'workspace_digest',
    'workspace_installation',
    'agent_handoff_generation',
    'managed_files',
    'native_files',
    'native_directories',
    'missing_files',
    'unsupported_entries',
  ];
  if (!exactAgentFinishPreflightKeys(answer, topKeys)
    || answer.schema !== 'mesh.agent-finish-preflight/v1'
    || answer.workspace_root !== binding.root
    || answer.workspace_digest !== binding.digest
    || answer.workspace_installation !== binding.installation
    || answer.agent_handoff_generation !== releaseAttempt.generation
    || !Array.isArray(answer.managed_files)
    || !Array.isArray(answer.native_files)
    || !Array.isArray(answer.native_directories)
    || !Array.isArray(answer.missing_files)
    || !Array.isArray(answer.unsupported_entries)) {
    throw new Error('Mesh returned an invalid agent-finish inspection. The agent folder remains assigned.');
  }
  const digest = (value) => typeof value === 'string' && /^[0-9a-f]{64}$/u.test(value);
  const bytes = (value) => Number.isSafeInteger(value) && value >= 0;
  const seen = new Set();
  const acceptPath = (value) => {
    if (!validAgentFinishRelativePath(value) || seen.has(value)) return false;
    seen.add(value);
    return true;
  };
  const managedValid = answer.managed_files.every((file) => exactAgentFinishPreflightKeys(file, [
    'path', 'current_version', 'byte_count', 'content_digest', 'executable',
    'modified_from_current_version',
  ]) && acceptPath(file.path)
    && typeof file.current_version === 'string' && file.current_version.length > 0
    && bytes(file.byte_count) && digest(file.content_digest)
    && typeof file.executable === 'boolean'
    && typeof file.modified_from_current_version === 'boolean');
  const nativeValid = answer.native_files.every((file) => exactAgentFinishPreflightKeys(file, [
    'path', 'byte_count', 'content_digest', 'executable',
  ]) && acceptPath(file.path)
    && bytes(file.byte_count) && digest(file.content_digest)
    && typeof file.executable === 'boolean');
  const directoriesValid = answer.native_directories.every((directory) => exactAgentFinishPreflightKeys(directory, [
    'path', 'installation',
  ]) && acceptPath(directory.path)
    && typeof directory.installation === 'string' && directory.installation.length > 0);
  const missingValid = answer.missing_files.every((file) => exactAgentFinishPreflightKeys(file, [
    'path', 'current_version', 'content_digest', 'executable',
  ]) && acceptPath(file.path)
    && typeof file.current_version === 'string' && file.current_version.length > 0
    && digest(file.content_digest) && typeof file.executable === 'boolean');
  const unsupportedValid = answer.unsupported_entries.every((entry) => exactAgentFinishPreflightKeys(entry, [
    'path', 'kind',
  ]) && acceptPath(entry.path)
    && ['symbolic-link', 'special', 'excluded-ancestor'].includes(entry.kind));
  if (!managedValid || !nativeValid || !directoriesValid || !missingValid || !unsupportedValid) {
    throw new Error('Mesh returned malformed or overlapping agent-finish inspection entries. The agent folder remains assigned.');
  }
  // This exact-generation native snapshot is authoritative. The workspace projection was read
  // before the assigned-only preflight and may not yet name files the agent just created; making
  // that older cache an equality oracle intermittently refused the very agent result Finish must
  // discover. The native command itself proves complete discovery while holding custody, and the
  // post-release scan independently rebuilds the editable UI state before any private save.
  return answer;
}

async function revalidateFinishedAgentWorkspace(releaseAttempt) {
  const binding = captureVerifiedWorkspace();
  if (binding.root !== releaseAttempt.root || binding.installation !== releaseAttempt.installation) {
    throw new Error('the open workspace changed before Finish completed');
  }
  const recent = JSON.parse(await invoke('recent_workspace_status'));
  assertVerifiedWorkspace(binding);
  installNavigationStatus(recent, binding.root);
  if (model.workspace?.root !== releaseAttempt.root
    || model.workspace?.installation !== releaseAttempt.installation) {
    throw new Error('the open workspace changed while Finish was checking its result');
  }
  if (recentWorkspaceEntry(releaseAttempt.root).agentHandoffInstallation
    || workspaceInstallationMatchesHandoff()) {
    throw new Error('a newer agent handoff now owns this workspace');
  }
}

async function inspectAgentFinishPreflight(releaseAttempt) {
  const binding = await refreshVerifiedWorkspaceForFolderScan();
  if (binding.root !== releaseAttempt.root
    || binding.installation !== releaseAttempt.installation
    || !agentHandoffMatches(releaseAttempt)) {
    throw new Error('The agent assignment changed before finish inspection.');
  }
  const answer = JSON.parse(await invoke('inspect_agent_finish_preflight', {
    expectedWorkspaceRoot: binding.root,
    expectedWorkspaceDigest: binding.digest,
    expectedWorkspaceInstallation: binding.installation,
    expectedAgentHandoffGeneration: releaseAttempt.generation,
  }));
  assertVerifiedWorkspace(binding);
  if (!agentHandoffMatches(releaseAttempt)) {
    throw new Error('The agent assignment changed during finish inspection.');
  }
  validateAgentFinishPreflight(answer, binding, releaseAttempt);
  model.nativeInspectionFailed = false;
  renderWorkspace();
  return true;
}

async function activateCurrentFinishAgent() {
  noticeSource.clearProof();
  if (!workspaceInstallationMatchesHandoff()) return;
  const handoff = model.agentHandoff;
  if (!handoff?.generation) {
    showNotice('Mesh cannot identify this exact agent assignment yet. Choose Refresh before finishing it.', true);
    return;
  }
  const releaseAttempt = Object.freeze({
    root: handoff.root,
    installation: handoff.installation,
    generation: handoff.generation,
  });
  if (!(await requestAccessibleConfirmation({
    title: 'Finish this agent handoff?',
    description: 'Mark this agent folder finished only after every Codex session, terminal agent, and editor using its pinned path has stopped. Mesh will inspect the complete folder and privately save unambiguous file and folder changes. Renames, deletions, and unsupported entries will remain for your review.',
    confirmLabel: 'Finish agent handoff',
    cancelLabel: 'Keep agent assigned',
    tone: 'destructive',
    isCurrent: () => agentHandoffMatches(releaseAttempt),
    staleMessage: 'The agent assignment changed while confirmation was open. Mesh kept the current assignment active. Refresh and review it again.',
  }))) return;
  if (!beginNativeWorkspaceLaunch()) return;
  try {
    // An already-started focus, startup, or periodic scan can still be returning when the person
    // confirms Finish. Freeze every new workspace action first, then drain that exact scan before
    // refreshing the verified frame used by the custody-held preflight. Otherwise two verified
    // reads can supersede one another and intermittently reject a healthy agent result.
    while (folderScanInFlight) await folderScanSettled;
    // One closed native command inspects every tracked, native-only, missing, directory, and
    // unsupported entry while exact-generation custody remains held. Ordinary file reads stay
    // unavailable during assignment, and this preflight cannot mutate or save anything.
    try {
      await inspectAgentFinishPreflight(releaseAttempt);
    } catch (error) {
      await invoke('renderer_proof_failure', { code: 'agent-handoff-finish' }).catch(() => {});
      model.nativeInspectionFailed = true;
      renderWorkspace();
      showNotice(
        `Agent folder remains assigned because Mesh could not complete its native inspection. ${error} Stop every process using it, then choose Finish agent handoff again.`,
        true,
      );
      return;
    }
    try {
      const recent = JSON.parse(await invoke('recent_workspace_status'));
      installNavigationStatus(recent, model.workspace?.root || null);
    } catch (error) {
      showNotice(`Agent folder remains assigned because Mesh could not recheck the exact assignment after inspection: ${error} Choose Refresh before trying again.`, true);
      return;
    }
    if (!agentHandoffMatches(releaseAttempt)) {
      showNotice('The agent assignment changed during inspection. Mesh kept the current assignment active; Refresh and review it before trying again.', true);
      return;
    }
    let released = false;
    let releaseRecoveryPrefix = '';
    let attemptedBinding = null;
    try {
      const binding = captureVerifiedWorkspace();
      attemptedBinding = binding;
      const answer = JSON.parse(await invoke('finish_managed_workspace_agent_handoff', {
        expectedWorkspaceRoot: binding.root,
        expectedWorkspaceDigest: binding.digest,
        expectedWorkspaceInstallation: binding.installation,
        expectedAgentHandoffGeneration: releaseAttempt.generation,
      }));
      assertVerifiedWorkspace(binding);
      if (
        answer.path !== binding.root
        || answer.workspace_installation !== binding.installation
        || typeof answer.cleared !== 'boolean'
      ) {
        throw new Error('Mesh did not confirm which exact agent folder was released. The collision warning remains active.');
      }
      const recent = JSON.parse(await invoke('recent_workspace_status'));
      assertVerifiedWorkspace(binding);
      installNavigationStatus(recent, binding.root);
      if (workspaceInstallationMatchesHandoff()) {
        throw new Error('Mesh could not verify that the agent-folder warning was cleared. Refresh before starting another agent.');
      }
      renderWorkspace();
      released = true;
    } catch (error) {
      if (!attemptedBinding) {
        showNotice(String(error), true);
      } else {
        try {
          const recent = JSON.parse(await invoke('recent_workspace_status'));
          assertVerifiedWorkspace(attemptedBinding);
          installNavigationStatus(recent, attemptedBinding.root);
          if (agentHandoffMatches(releaseAttempt)) {
            showNotice(`${error} Mesh confirmed that this exact agent folder remains assigned. Inspect it before trying Finish agent handoff again.`, true);
          } else if (
            recentWorkspaceEntry(attemptedBinding.root).agentHandoffGeneration === releaseAttempt.generation
            && !recentWorkspaceEntry(attemptedBinding.root).agentHandoffInstallation
          ) {
            renderWorkspace();
            released = true;
            releaseRecoveryPrefix = 'Mesh lost the release reply but verified that this exact agent handoff ended. ';
          } else {
            showNotice(`${error} Mesh found a newer or different agent assignment and kept it active. Refresh and review the current assignment before trying again.`, true);
          }
        } catch (recoveryError) {
          showNotice(`${error} Mesh could not confirm whether the exact agent handoff ended: ${recoveryError} Refresh before starting another agent.`, true);
        }
      }
    }
    if (!released) return;
    // A startup, focus, or periodic inspection can already own the single scan slot when the user
    // starts and then finishes an agent handoff. Releasing custody is non-idempotent, so never replay
    // it when that older read is still returning. Keep the global launch/transition guard held, let
    // every existing scan settle, then claim the slot synchronously below and perform the required
    // complete post-release inspection against current native truth before saving or reporting success.
    while (folderScanInFlight) await folderScanSettled;
    // Custody is cleared only after the first complete scan, but an external process can still
    // finish one last filesystem write between that scan and the durable release. Re-scan the now
    // unassigned physical folder before claiming it matches private history or selecting the exact
    // queue to save. Suppress the persisted background-save preference here: this confirmed finish
    // flow owns the resulting save and its user-facing outcome.
    const inspectedAfterRelease = await scanNativeFolder({
      automatic: true,
      agentFinished: true,
      allowAutomaticSave: false,
      withinNativeWorkspaceLaunch: true,
    });
    if (!inspectedAfterRelease) {
      const scanFailure = noticeSource.snapshot()?.message.trim() || '';
      showNotice(
        `${releaseRecoveryPrefix}Agent folder was released, but Mesh could not verify its final native contents after release.${scanFailure ? ` ${scanFailure}` : ''} Use Find folder changes before starting another agent.`,
        true,
      );
      return;
    }
    let finishRescanCompleted = false;
    if (model.folderChanges.length === 0) {
      showNotice(`${releaseRecoveryPrefix}Agent folder released. Its exact native contents match private history. Start Codex on this version for one agent, or use Start another agent copy for concurrent work.`);
      finishRescanCompleted = true;
    } else if (!model.folderChanges.some((change) => change.native_missing || change.native_unsupported)) {
      finishRescanCompleted = await saveAllPrivateChanges({
        successPrefix: `${releaseRecoveryPrefix}Agent folder released after a complete native inspection. `,
        failurePrefix: `${releaseRecoveryPrefix}Agent folder was released after a complete native inspection, but its private save stopped. `,
        withinNativeWorkspaceLaunch: true,
      });
    } else {
      const count = model.folderChanges.length;
      showNotice(`${releaseRecoveryPrefix}Agent folder was released after a complete native inspection. ${count} native ${count === 1 ? 'change needs' : 'changes need'} explicit review because Mesh will not guess a rename, deletion, or unsupported entry. Resolve the complete queue before recording or approving this result.`);
    }
    try {
      await revalidateFinishedAgentWorkspace(releaseAttempt);
    } catch (error) {
      showNotice(`Mesh completed the release inspection but could not verify that the same workspace remained unassigned: ${error} Refresh before starting or switching agents.`, true);
      return;
    }
    if (finishRescanCompleted) {
      const completedNoticeGeneration = noticeSource.snapshot()?.generation ?? null;
      await invoke('renderer_proof_agent_handoff_rescanned', {
        expectedWorkspaceRoot: releaseAttempt.root,
        expectedWorkspaceInstallation: releaseAttempt.installation,
        expectedAgentHandoffGeneration: releaseAttempt.generation,
      }).then(() => noticeSource.markProof(
        completedNoticeGeneration,
        'agent-handoff-rescanned',
      )).catch(() => {});
    }
  } finally {
    finishNativeWorkspaceLaunch();
  }
}

async function activateCurrentStartAgentCopy() {
  if (refuseNativeNavigationWithoutOriginalBinding()) return;
  if (refuseAgentLaunchWithIncompleteNativeInspection()) return;
  const selected = currentWorkspaceVersion();
  if (!selected) {
    if (hasConcurrentWorkspaceHistory()) {
      await focusWorkspaceVersionChoice({ forAgent: true });
      return;
    }
    showNotice('Save one complete private workspace point before starting another isolated agent.', true);
    return;
  }
  const operation = selected.operation;
  try {
    await openWorkspaceVersionAsFolder(operation, null, {
      reveal: false,
      openInCodex: true,
      freshCopy: true,
      allowNativeChanges: false,
      allowUnverified: false,
      blockedAction: 'starting another isolated agent',
      choiceStillCurrent: () => currentWorkspaceVersion()?.operation === operation,
    });
  } catch (error) {
    showNotice(`Mesh could not start another isolated agent: ${error}`, true);
  }
}

async function activateCurrentCopyAgentPath() {
  if (refuseNativeNavigationWithoutOriginalBinding()) return;
  if (refuseAgentLaunchWithEditorDraft()) return;
  if (refuseAgentLaunchWithIncompleteNativeInspection()) return;
  if (workspaceInstallationMatchesHandoff()) {
    showNotice('This exact folder is already assigned to an agent. Use Start another agent copy for concurrent work, or choose Finish agent handoff after the current agent stops.');
    return;
  }
  if (!appWindow.navigator?.clipboard?.writeText) {
    showNotice('Mesh could not copy the agent path because clipboard access is unavailable. Use Start Codex on this version or Open agent terminal instead.', true);
    return;
  }
  if (!beginNativeWorkspaceLaunch()) return;
  let handoffRecorded = false;
  try {
    if (!model.agentFolder?.path) {
      throw new Error('Create a native working folder before handing this workspace to an agent');
    }
    const binding = captureVerifiedWorkspace();
    const displayedAgentPath = model.agentFolder.path;
    let prepared;
    try {
      prepared = JSON.parse(await invoke('prepare_managed_workspace_agent_path', {
        expectedAgentPath: displayedAgentPath,
        expectedWorkspaceRoot: binding.root,
        expectedWorkspaceDigest: binding.digest,
        expectedWorkspaceInstallation: binding.installation,
        confirmedReopen: false,
        expectedAgentHandoffGeneration: null,
      }));
    } catch (error) {
      markWorkspaceUnverified();
      throw error;
    }
    if (
      prepared.display_path !== displayedAgentPath
      || typeof prepared.path !== 'string'
      || !prepared.path.startsWith('/')
      || prepared.workspace_installation !== binding.installation
      || prepared.agent_handoff_recorded !== true
      || canonicalAgentHandoffGeneration(prepared.agent_handoff_generation) === null
    ) {
      markWorkspaceUnverified();
      throw new Error('Mesh did not confirm which exact folder was assigned to the agent. Refresh before using it.');
    }
    // Native custody is now durable. Reflect it before the clipboard write so even an ambiguous
    // clipboard outcome cannot make the same writable folder look available to another agent.
    handoffRecorded = true;
    assertVerifiedWorkspace(binding);
    rememberAgentHandoff(binding, prepared.agent_handoff_generation);
    await appWindow.navigator.clipboard.writeText(prepared.path);
    assertVerifiedWorkspace(binding);
    if (model.agentFolder?.path !== displayedAgentPath) throw new WorkspaceVerificationSuperseded();
    const git = gitContextMessage(prepared);
    showNotice(`Copied a stable reference for ${prepared.display_path}. Mesh marked this writable folder as assigned; use Start another agent copy for concurrent work, or Finish agent handoff after the agent stops.${git}`);
  } catch (error) {
    showNotice(
      handoffRecorded
        ? `Mesh marked this folder as assigned but could not confirm that its path reached the clipboard. The collision warning remains active. If no agent received the path, choose Finish agent handoff before retrying. (${error})`
        : `Mesh could not copy the agent path. Do not use the displayed path until Mesh verifies it again. Choose Refresh, then retry Copy agent path, Start Codex on this version, or Open agent terminal. (${error})`,
      true,
    );
  } finally {
    finishNativeWorkspaceLaunch();
  }
}

async function activateCurrentUpdateDestination() {
  if (!model.workspace || !model.workspaceVerified) return;
  // Pullback reads only durable Mesh history, so it cannot corrupt the assigned folder. It would
  // still be misleading to start copying an older saved point while the agent can produce a newer
  // result at any moment. Keep the user journey linear and enforce the custody boundary again in
  // the handler so a stale or scripted click cannot bypass the disabled control.
  if (workspaceInstallationMatchesHandoff()) {
    showNotice('This folder is still assigned to an agent. After every process using it stops, choose Finish agent handoff, save and review the complete result, then update the original folder.', true);
    return;
  }
  try {
    const projectRoot = workspaceProjectRoot();
    if (projectRoot && originalProjectUpdateBlocked(projectRoot)) {
      focusWorkspaceJourney('review-workbench-next', 'review-workbench-next');
      showNotice('Record a review and approve the current saved version before updating the original project folder. Mesh will not return unapproved private work through this shortcut.', true);
      return;
    }
    if (!(await confirmWorkspaceSwitch({
      allowNativeChanges: false,
      allowUnverified: false,
      blockedAction: projectRoot ? 'updating the original folder' : 'updating the destination folder',
    }))) return;
    if (projectRoot && (model.exportRoot !== projectRoot || workspaceDestinationDraft !== projectRoot)) {
      clearExportPreview();
      model.exportRoot = projectRoot;
      workspaceDestinationDraft = projectRoot;
      renderExportChoices();
    }
    focusReactWorkspacePage('update', '[data-mesh-proof="destination-draft"]');
    if (!workspaceDestinationDraft) {
      showNotice('Type or paste the original or destination folder you want to update, then press Return. Mesh will preview the exact saved changes before enabling any write.', true);
      return;
    }
    await previewAllManagedExports();
  } catch (error) {
    showNotice(String(error), true);
  }
}

function beginWorkspaceVerification({ preserveVerifiedPresentation = false } = {}) {
  workspaceVerificationSequence += 1;
  // Periodic folder inspection is a read-only hint. Keeping the last verified frame painted while
  // its replacement is checked prevents every five-second clean scan from briefly disabling the
  // entire React workspace. An explicit refresh or a mutation still fails closed immediately.
  if (!preserveVerifiedPresentation) markWorkspaceUnverified();
  return workspaceVerificationSequence;
}

function beginWorkspaceMutation({
  withinWorkspaceTransition = false,
  withinNativeWorkspaceLaunch = false,
  consumeEditorDraft = false,
} = {}) {
  // Text in the embedded editor is still browser-only until preserve_managed_text succeeds.
  // Every other managed mutation can replace the verified workspace digest and clear that buffer,
  // so enforce the boundary at the shared entrypoint as well as in visible button state.
  if (editorDraftPending() && !consumeEditorDraft) {
    throw new Error('Save, copy, or revert the open editor draft before changing the managed workspace. Mesh will not discard text that exists only in this window.');
  }
  if (
    workspaceMutationInFlight
    || (workspaceTransitionInFlight && !withinWorkspaceTransition)
    || (nativeWorkspaceLaunchInFlight && !withinNativeWorkspaceLaunch)
  ) {
    throw new Error('Another local workspace change is still finishing. Wait for it before continuing.');
  }
  // A picker started before this mutation must not publish a source or destination after the
  // workspace-changing operation has begun. Disabled controls prevent new clicks; this generation
  // also invalidates native picker promises that were already unresolved.
  workspaceSelectionSequence += 1;
  workspaceMutationInFlight = true;
  return beginWorkspaceVerification();
}

function finishWorkspaceMutation() {
  workspaceMutationInFlight = false;
  // Mutation handlers install and render the authoritative post-commit snapshot while the
  // in-flight guard is still true. Re-render every interactive choice after releasing that guard;
  // updating only the top actions leaves lower controls stuck disabled until an unrelated refresh.
  // Do not redraw the state summary here: a handler may have truthfully kept it Working because
  // native work arrived during the commit.
  renderManagementChoices();
  renderHistoryChoices();
  renderWorkspaceVersionChoices();
  renderEditorChoices();
  renderExportChoices();
  renderWorkspaceActions();
  renderNextAction();
}

async function coordinateWorkspaceMutation(operation, options = {}) {
  const sequence = beginWorkspaceMutation(options);
  try {
    return await operation(sequence);
  } finally {
    finishWorkspaceMutation();
  }
}

function assertCurrentWorkspaceVerification(sequence) {
  if (sequence !== workspaceVerificationSequence) throw new WorkspaceVerificationSuperseded();
}

function captureVerifiedWorkspace() {
  if (!model.workspace || managedMutationBlocked()) {
    throw new Error('Refresh and verify the current workspace before reading managed file state.');
  }
  return {
    sequence: workspaceVerificationSequence,
    root: model.workspace.root,
    digest: model.workspace.digest,
    installation: model.workspace.installation,
  };
}

function assertVerifiedWorkspace(binding) {
  assertCurrentWorkspaceVerification(binding.sequence);
  if (
    !model.workspaceVerified ||
    model.workspace?.root !== binding.root ||
    model.workspace?.digest !== binding.digest ||
    model.workspace?.installation !== binding.installation
  ) {
    throw new WorkspaceVerificationSuperseded();
  }
}

function managedMutationBinding() {
  if (rememberedWorkspaceNeedsImport(model.workspace)) {
    throw new Error('Preview this ordinary folder and create the private native workspace before making managed changes.');
  }
  if (workspaceInstallationMatchesHandoff()) {
    throw new Error(activeAgentMutationRefusal());
  }
  const binding = captureVerifiedWorkspace();
  return {
    expectedWorkspaceRoot: binding.root,
    expectedWorkspaceDigest: binding.digest,
    expectedWorkspaceInstallation: binding.installation,
  };
}

async function readVerifiedWorkspace(readWorkspace, sequence) {
  const workspace = await readWorkspace();
  assertCurrentWorkspaceVerification(sequence);
  const checkpoint = JSON.parse(await invoke('managed_checkpoint_state'));
  assertCurrentWorkspaceVerification(sequence);
  if (
    typeof workspace.digest !== 'string' ||
    typeof workspace.installation !== 'string' ||
    checkpoint.root !== workspace.root ||
    checkpoint.workspace_digest !== workspace.digest ||
    checkpoint.workspace_installation !== workspace.installation
  ) {
    throw new Error('Mesh changed the open workspace while its checkpoint state was being verified.');
  }
  return { workspace, checkpoint };
}

async function verifiedWorkingFolderPath(binding, expectedPath) {
  const navigation = JSON.parse(await invoke('reconcile_managed_workspace_navigation', {
    expectedWorkspaceRoot: binding.root,
    expectedWorkspaceDigest: binding.digest,
    expectedWorkspaceInstallation: binding.installation,
  }));
  assertVerifiedWorkspace(binding);
  if (
    navigation.stable !== true
    || navigation.native_folder !== true
    || typeof navigation.path !== 'string'
    || navigation.path !== expectedPath
    || navigation.workspace_root !== binding.root
  ) {
    throw new Error('The stable working folder changed after it was shown. Refresh before copying it.');
  }
  return navigation.path;
}

async function installVerifiedWorkspace(readWorkspace, suppliedSequence = null, explicitOpen = false) {
  const sequence = suppliedSequence ?? beginWorkspaceVerification();
  try {
    const { workspace, checkpoint } = await readVerifiedWorkspace(readWorkspace, sequence);
    installWorkspaceSnapshot(workspace, checkpoint, explicitOpen);
    return workspace;
  } catch (cause) {
    if (sequence !== workspaceVerificationSequence || cause instanceof WorkspaceVerificationSuperseded) {
      throw new Error(
        'The operation completed, but a newer workspace verification replaced its result. Review the current workspace before continuing.',
        { cause },
      );
    }
    const navigationWarning = await reconcileNavigationFromDaemon();
    markWorkspaceUnverified();
    throw new Error(
      'The operation completed, but Mesh could not verify the updated workspace. Managed changes are paused until Refresh succeeds.'
        + (navigationWarning ? ` ${navigationWarning}` : ''),
      { cause },
    );
  }
}

// Opening a saved point is a native mutation: the daemon can switch workspaces before Git setup,
// stable-link publication, recent-history persistence, or response delivery reports an error.
// Never leave the prior browser snapshot verified after that ambiguous boundary. Read the daemon
// and checkpoint together, repair navigation for only that observed workspace, and then report the
// original error so the person knows the selected point itself was not confirmed.
async function recoverAmbiguousWorkspaceVersionOpen(sequence, exportRoot, cause) {
  let recovered;
  try {
    recovered = await installVerifiedWorkspace(
      () => call('workspace.state'),
      sequence,
      true,
    );
  } catch (recoveryError) {
    throw new Error(
      `Mesh could not confirm whether the saved workspace opened: ${cause} Mesh also could not verify which workspace is now open. Managed actions remain paused; choose Refresh before using either folder.`,
      { cause: recoveryError },
    );
  }

  const navigationWarning = await rememberWorkspace(recovered.root, exportRoot);
  assertCurrentWorkspaceVerification(sequence);
  if (
    !model.workspaceVerified
    || model.workspace?.root !== recovered.root
    || model.workspace?.digest !== recovered.digest
    || model.workspace?.installation !== recovered.installation
  ) {
    markWorkspaceUnverified();
    throw new Error(
      `Mesh could not confirm whether the saved workspace opened: ${cause} Mesh found the daemon's current workspace, but could not keep its native navigation verified. Managed actions remain paused; choose Refresh before using either folder.`,
      { cause },
    );
  }
  try {
    const afterNavigation = await readVerifiedWorkspace(() => call('workspace.state'), sequence);
    if (
      afterNavigation.workspace.root !== recovered.root
      || afterNavigation.workspace.digest !== recovered.digest
      || afterNavigation.workspace.installation !== recovered.installation
    ) {
      throw new WorkspaceVerificationSuperseded();
    }
    installWorkspaceSnapshot(afterNavigation.workspace, afterNavigation.checkpoint);
  } catch (recoveryError) {
    await reconcileNavigationFromDaemon();
    markWorkspaceUnverified();
    throw new Error(
      `Mesh could not confirm whether the saved workspace opened: ${cause} The workspace changed again while Mesh was repairing its navigation. Managed actions remain paused; choose Refresh before using either folder.`,
      { cause: recoveryError },
    );
  }
  renderWorkspace();
  throw new Error(
    `Mesh could not confirm the saved-version reply: ${cause} Mesh recovered and verified the workspace that is actually open at ${recovered.root}. Review that workspace before retrying the version switch.${navigationWarning ? ` ${navigationWarning}` : ''}`,
    { cause },
  );
}

async function reconcileNavigationFromDaemon() {
  try {
    const navigation = JSON.parse(await invoke('reconcile_current_workspace_navigation'));
    model.activeFolder = navigation.path
      ? { path: navigation.path, stable: navigation.stable === true }
      : null;
    stableNavigationRepairPending = !navigation.path && typeof navigation.warning === 'string';
    return typeof navigation.warning === 'string' ? navigation.warning : null;
  } catch (error) {
    model.activeFolder = null;
    stableNavigationRepairPending = true;
    return `Mesh could not confirm the stable folder from the open workspace: ${error}`;
  }
}

function verifiedWorkspaceSnapshotMatches(workspace, checkpoint) {
  return model.workspaceVerified
    && JSON.stringify(model.workspace) === JSON.stringify(workspace)
    && JSON.stringify(model.checkpoint) === JSON.stringify(checkpoint);
}

async function refreshVerifiedWorkspaceForFolderScan({ preserveVerifiedPresentation = false } = {}) {
  // A periodic scan is only a read hint. Do not publish a new authority sequence until the
  // verified pair actually changes: the mounted React actions close over this sequence, so
  // advancing it without reprojecting would leave a visibly enabled action permanently stale.
  let sequence = preserveVerifiedPresentation
    ? workspaceVerificationSequence
    : beginWorkspaceVerification();
  try {
    const { workspace, checkpoint } = await readVerifiedWorkspace(
      () => call('workspace.state'),
      sequence,
    );
    const snapshotChanged = !verifiedWorkspaceSnapshotMatches(workspace, checkpoint);
    if (preserveVerifiedPresentation && snapshotChanged) {
      // The verified replacement is already complete. Advance authority and publish its frame as
      // one synchronous transition, rather than briefly painting an unverified intermediate.
      sequence = beginWorkspaceVerification({ preserveVerifiedPresentation: true });
    }
    installWorkspaceSnapshot(workspace, checkpoint);
    // Explicit verification deliberately disables every managed control while the two
    // authenticated reads are in flight. Re-render its complete verified snapshot here; the
    // scan's final editor-only render cannot otherwise restore review, navigation, version, and
    // export actions in the real disabled DOM. A clean periodic read retains that already-verified
    // presentation instead.
    if (!preserveVerifiedPresentation || snapshotChanged) renderWorkspace();
    return captureVerifiedWorkspace();
  } catch (cause) {
    if (sequence !== workspaceVerificationSequence || cause instanceof WorkspaceVerificationSuperseded) {
      throw new WorkspaceVerificationSuperseded();
    }
    // A failed periodic read is no longer a harmless hint: revoke its old authority and paint the
    // fail-closed recovery state. Explicit refreshes already advanced before their read began.
    if (preserveVerifiedPresentation) beginWorkspaceVerification();
    else markWorkspaceUnverified();
    throw new Error(
      'Mesh could not verify the current workspace before scanning its native folder. Managed changes are paused until Refresh succeeds.',
      { cause },
    );
  }
}

function importWorkbenchPresentation() {
  const data = model.preview;
  const interactionBlocked = workspaceInteractionInFlight()
    || workspaceEntrySelectionInFlight > 0
    || importConfirmationInFlight;
  if (!data) {
    return Object.freeze({
      phase: 'select',
      sourcePath: importSourceDraft,
      fileCount: '0',
      folderCount: '0',
      byteCount: '0',
      summary: '',
      scopeNote: 'Choose an ordinary folder. Mesh verifies every included file before it creates a private working copy.',
      files: Object.freeze([]),
      fileListLabel: 'Review included files',
      destinationPath: '',
      confirmLabel: 'Create workspace and open folder',
      busy: false,
      canChoose: !interactionBlocked,
      canPreviewPath: !interactionBlocked,
      canEditDestination: false,
      canChooseDestination: false,
      canConfirm: false,
      stepLabel: '1 / 2',
    });
  }
  const fileEntries = validateImportFilePreview(data);
  const files = fileEntries.map((entry) => (
    `${entry.path} · ${formatBytes(exactByteCount(entry.bytes))} bytes${entry.executable ? ' · executable' : ''}`
  ));
  if (data.files_not_listed) files.push(`…and ${data.files_not_listed} more included files`);
  const sameFolderMigration = data.source_scope === 'open-zero-history-workspace'
    || (data.source_scope === undefined
      && Boolean(
        model.workspace
        && model.workspace.records === 0
        && rememberedWorkspaceHasUnversionedContent(model.workspace)
        && model.source === model.workspace.root,
      ));
  const connectExistingImport = currentWorkspaceNavigationMissing()
    && model.checkpoint?.confirmed_import_receipt === true;
  const emptyImport = data.files === 0 && data.directories === 0;
  const destinationAvailable = !interactionBlocked && importDestinationChooserInFlight === null;
  return Object.freeze({
    phase: 'review',
    sourcePath: model.source,
    fileCount: String(data.files),
    folderCount: String(data.directories),
    byteCount: formatBytes(data.bytes),
    summary: data.summary,
    scopeNote: connectExistingImport
      ? 'Mesh will use the selected folder only as the verified original for this existing workspace. Exact private origin receipts must agree before Mesh records the relationship; no second managed copy will be created.'
      : emptyImport
      ? 'This folder has no importable files or folders. Choose another folder containing project content. No workspace can be created from this preview.'
      : sameFolderMigration
        ? 'This is the currently open zero-history folder. Mesh will bring only its ordinary project files. Existing Mesh private history, databases, and content-store folders stay in the original and are not copied.'
        : "Counts cover the files Mesh will bring into the native working folder. Mesh applies this project's .gitignore and .meshignore. The source .git directory is never copied as workspace content; for Git projects, Mesh separately recreates independently owned history, branch, and index state in the new working folder. Excluded paths stay only in the original folder.",
    files: Object.freeze(files),
    fileListLabel: data.files === 1
      ? 'Review 1 included file'
      : `Review ${data.files} included files`,
    destinationPath: model.destination || '',
    confirmLabel: importConfirmationInFlight
      ? connectExistingImport
        ? 'Connecting original folder…'
        : 'Creating private workspace…'
      : connectExistingImport
      ? 'Connect original folder'
      : emptyImport
      ? 'Choose a folder with content'
      : 'Create workspace and open folder',
    busy: importConfirmationInFlight,
    canChoose: !interactionBlocked,
    canPreviewPath: !interactionBlocked,
    canEditDestination: destinationAvailable,
    canChooseDestination: destinationAvailable,
    canConfirm: destinationAvailable && !emptyImport,
    stepLabel: '2 / 2',
  });
}

function renderPreview(
  interactionGeneration = importWorkbenchNextInteraction?.generation ?? null,
) {
  const presentation = importWorkbenchPresentation();
  renderImportWorkbenchNext(presentation, interactionGeneration);
  renderNextAction();
}

function renderImportWorkbenchNext(
  presentation,
  interactionGeneration = importWorkbenchNextInteraction?.generation ?? null,
) {
  if (!importWorkbenchNextAvailable) {
    return;
  }
  const generation = ++importWorkbenchNextGeneration;
  const phase = presentation.phase;
  const chooserAuthority = Object.freeze({
    verificationSequence: workspaceVerificationSequence,
    continuityKey: workspaceProjectionContinuityKey(),
  });
  const previewStateAuthority = Object.freeze({
    selection: workspaceSelectionSequence,
    source: model.source,
    destination: model.destination,
    summary: model.preview?.summary ?? null,
  });
  const destinationFieldEchoAuthority = phase === 'review'
    && interactionGeneration === importWorkbenchNextMounted?.generation
    ? previewStateAuthority
    : null;
  importWorkbenchNextPending = Object.freeze({
    generation,
    interactionGeneration,
    phase,
    destinationFieldEchoAuthority,
  });
  if (interactionGeneration !== importWorkbenchNextMounted?.generation) {
    importWorkbenchNextMounted = null;
  }
  installImportWorkbenchNextVisibility();
  const actions = new Map();
  if (presentation.canChoose) {
    actions.set('choose-folder', () => chooseSourceFolder(chooserAuthority));
  }
  const previewAuthority = Object.freeze({
    generation,
    ...previewStateAuthority,
  });
  if (presentation.canChooseDestination) {
    actions.set('choose-destination', () => chooseImportDestination(previewAuthority));
  }
  if (presentation.canConfirm) {
    actions.set('confirm-import', () => confirmImport(previewAuthority));
  }
  importWorkbenchNextActions = actions;
  appDocument.dispatchEvent(new appWindow.CustomEvent('mesh:import-workbench-projection', {
    detail: Object.freeze({
      generation,
      import: Object.freeze({
        phase,
        sourcePath: presentation.sourcePath,
        fileCount: presentation.fileCount,
        folderCount: presentation.folderCount,
        byteCount: presentation.byteCount,
        summary: presentation.summary,
        scopeNote: presentation.scopeNote,
        files: presentation.files,
        destinationPath: presentation.destinationPath,
        confirmLabel: presentation.confirmLabel,
        busy: presentation.busy,
        canChoose: actions.has('choose-folder'),
        canPreviewPath: presentation.canPreviewPath,
        canEditDestination: presentation.canEditDestination,
        canChooseDestination: actions.has('choose-destination'),
        canConfirm: actions.has('confirm-import'),
      }),
    }),
  }));
}

const WORKSPACE_REQUIRED_SECTIONS = [
  'workspace-overview-next',
  'review-workbench-next',
  'workspace-files-next',
];

function workspaceHeroLede(workspace) {
  if (!workspace) {
    return 'Bring in an ordinary project folder. Mesh verifies every file before it creates a private working copy, the original stays untouched, and your recent validated workspaces remain available across restarts.';
  }
  if (rememberedWorkspaceNeedsImport(workspace)) {
    return rememberedWorkspaceHasUnversionedContent(workspace)
      ? 'This remembered folder contains ordinary files that are not saved in Mesh yet. Choose Preview this folder to review the exact content, then create a private working copy; the current folder stays unchanged.'
      : 'This remembered workspace contains no saved work. Bring in an ordinary project folder to create a useful private working copy; the original stays untouched.';
  }
  if (!model.agentFolder?.path) {
    if (nativeFolderUpgradeCandidate()) {
      return 'This older workspace is safe in private history but does not yet have an isolated native working folder. Create a native working folder once, then use its stable shortcut with Finder and editors or its independent real path with Terminal and agents.';
    }
    return hasConcurrentWorkspaceHistory()
      ? 'This workspace has concurrent saved history and does not yet have an isolated native working folder. Choose one exact saved workspace to open independently; Mesh will not guess or combine the current lines.'
      : 'This workspace does not yet have an isolated native working folder or a single durable point that Mesh can open safely. Save or resolve its current work first, then create the native folder before handing it to an editor or agent.';
  }
  return model.activeFolder?.path
    ? 'Use the stable folder for Finder and editors, or the independent real folder for Terminal and agents. The independent folder is writable and does not move when Mesh switches elsewhere. Return here to inspect changes, save a private version, open another version, or update the original folder from saved work.'
    : 'Your independent real folder is ready for Terminal and agents, but the stable shortcut needs attention. Use Open working folder to repair it; saving, version history, and updating the original remain available from the verified native folder.';
}

function workspaceEntryPresentation(workspace) {
  const recentPaths = recentWorkspacePaths();
  const needsImport = rememberedWorkspaceNeedsImport(workspace);
  const workspaceReady = Boolean(workspace) && !needsImport;
  const currentName = workspaceReady ? currentWorkspaceDisplayName() : null;
  return Object.freeze({
    eyebrow: workspaceReady ? 'NATIVE WORKSPACE' : 'LOCAL WORKSPACE',
    title: workspaceReady
      ? currentName
        ? `Working in ${currentName}`
        : 'Work in your folder. Mesh remembers.'
      : 'Your folder, with a private history.',
    description: workspaceHeroLede(workspace),
    disclosureLabel: workspaceReady
      ? 'Import or switch to another workspace'
      : needsImport
        ? 'Bring in an ordinary project folder'
        : recentPaths.length
          ? 'Return to a recent Mesh workspace'
          : 'Already use Mesh? Open a managed workspace',
    defaultDisclosureOpen: needsImport || (!workspace && recentPaths.length > 0),
    chooseLabel: workspaceReady ? 'Import another folder' : 'Choose a folder',
    currentName,
    workspaceReady,
  });
}

function renderWorkspaceDisclosure(workspace) {
  const presentation = workspaceEntryPresentation(workspace);
  appDocument.title = presentation.workspaceReady && presentation.currentName
    ? `${presentation.currentName} — Mesh`
    : 'Mesh — Local workspace';
  // A remembered zero-history workspace is not yet a useful Mesh workspace. Keep the import
  // journey visible so reopening an old empty shell cannot hide the one action that makes it
  // usable. Established workspaces still land directly in the native work loop.
  workspaceEntryDisclosureOpen = presentation.defaultDisclosureOpen;
  for (const id of WORKSPACE_REQUIRED_SECTIONS) {
    $(id).classList.toggle('hidden', !presentation.workspaceReady);
  }
}

function shortVersionIdentity(identity) {
  if (typeof identity !== 'string' || !identity) return '';
  return identity.length > 12 ? `${identity.slice(0, 12)}…` : identity;
}

function privateVersionPresentation(workspace) {
  const identity = workspace?.private_version?.version || null;
  if (!identity) return { text: 'No saved version yet', title: '' };
  const versions = workspace.workspace_versions || [];
  const current = workspace.private_version?.concurrent_changes === 1
    ? versions.at(-1) || null
    : null;
  return {
    text: current
      ? `Saved point ${current.ordinal} · ${shortVersionIdentity(identity)}`
      : `Private version · ${shortVersionIdentity(identity)}`,
    title: `Exact private version: ${identity}`,
  };
}

function sharedVersionPresentation(workspace) {
  const identity = workspace?.shared_version || null;
  if (!identity) return { text: 'Not available', title: '' };
  const review = (workspace.review_items || []).find((item) => item.reviewed_head === identity);
  const savedPoint = review
    ? (workspace.workspace_versions || []).find((version) => version.operation === review.subject_operation)
    : null;
  return {
    text: savedPoint
      ? `Approved point ${savedPoint.ordinal} · ${shortVersionIdentity(identity)}`
      : `Approved version · ${shortVersionIdentity(identity)}`,
    title: `Exact shared version: ${identity}`,
  };
}

function currentRecordedReviewReady(workspace) {
  if (workspace?.private_version?.concurrent_changes !== 1) return false;
  const current = (workspace.workspace_versions || []).at(-1);
  if (!current) return false;
  return (workspace.review_items || []).some((item) => (
    item.recorded !== false
    && item.content_complete === true
    && item.subject_operation === current.operation
  ));
}

function renderWorkspace() {
  const data = model.workspace;
  renderWorkspaceDisclosure(data);
  renderReviews();
  if (!data) {
    renderManagementChoices();
    renderHistoryChoices();
    renderWorkspaceVersionChoices();
    renderEditorChoices();
    renderExportChoices();
    renderWorkspaceActions();
    renderNextAction();
    return;
  }
  renderManagementChoices();
  renderHistoryChoices();
  renderWorkspaceVersionChoices();
  renderEditorChoices();
  renderExportChoices();
  renderWorkspaceActions();
  renderNextAction();
}

function clearExportPreview() {
  exportPreviewSequence += 1;
  model.exportPreview = null;
  model.exportBatchPreview = null;
  workspaceDestinationPlanText = '';
  // The plan is React-owned. Revoke its visible text and action projection immediately when any
  // selection or asynchronous preview is superseded; waiting for a later full render would leave
  // a stale plan on screen even though the hidden irreversible controls are already disabled.
  renderWorkspaceDestinationNext();
  return exportPreviewSequence;
}

function assertCurrentExportPreview(sequence) {
  if (sequence !== exportPreviewSequence) throw new ExportPreviewSuperseded();
}

async function confirmPullBackPreview(targetRoot = workspaceDestinationDraft) {
  if (originalProjectUpdateBlocked(targetRoot)) {
    focusWorkspaceJourney('review-workbench-next', 'review-workbench-next');
    showNotice('Record a review and approve the current saved version before updating the original project folder. Mesh will not return unapproved private work through this shortcut.', true);
    return false;
  }
  if (model.folderChanges.length) {
    const count = model.folderChanges.length;
    showNotice(
      `Mesh found ${count} unsaved native ${count === 1 ? 'change' : 'changes'} in ${model.workspace.root}. Save or revert them before previewing the destination update.`,
      true,
    );
    return false;
  }
  // Older private-storage layouts are not supported native editing surfaces. They still retain
  // any change found by startup discovery above, while a real native folder receives a fresh
  // complete inspection so an agent edit made after the last five-second scan cannot be missed.
  if (!model.agentFolder?.path) return true;
  return confirmWorkspaceSwitch({
    allowNativeChanges: false,
    allowUnverified: false,
    blockedAction: 'previewing the destination update',
  });
}

function showExportPlan(lines) {
  workspaceDestinationPlanText = lines.join('\n');
}

async function confirmWholeWorkspaceExport({
  title,
  description,
  confirmLabel,
  targetRoot,
  tone,
  isCurrent,
}) {
  try {
    const proofAccepted = await invoke('renderer_proof_accept_private_export_confirmation', {
      destination: targetRoot,
    });
    if (proofAccepted === true) return isCurrent();
  } catch {
    // Normal launches have no renderer-proof authority and continue into the source-owned human
    // confirmation. If that island cannot mount, its coordinator falls back to browser confirm.
  }
  return requestAccessibleConfirmation({
    title,
    description,
    confirmLabel,
    tone,
    isCurrent,
    staleMessage: 'The reviewed destination plan changed while the confirmation was open. Preview the saved workspace again.',
  });
}

function exportBatchStillCurrent(batch, sequence) {
  if (model.exportBatchPreview !== batch
    || exportPreviewSequence !== sequence
    || workspaceDestinationDraft !== batch.targetRoot) return false;
  try {
    const binding = managedMutationBinding();
    return binding.expectedWorkspaceRoot === batch.binding.root
      && binding.expectedWorkspaceDigest === batch.binding.digest
      && binding.expectedWorkspaceInstallation === batch.binding.installation;
  } catch {
    return false;
  }
}

function singleExportStillCurrent(preview, sequence, binding) {
  if (model.exportPreview !== preview
    || exportPreviewSequence !== sequence
    || workspaceDestinationSelectedFile !== preview.path
    || workspaceDestinationDraft !== preview.target_root
    || !workspaceDestinationActionEnabled('confirm-single')) return false;
  try {
    const current = managedMutationBinding();
    return current.expectedWorkspaceRoot === binding.expectedWorkspaceRoot
      && current.expectedWorkspaceDigest === binding.expectedWorkspaceDigest
      && current.expectedWorkspaceInstallation === binding.expectedWorkspaceInstallation;
  } catch {
    return false;
  }
}

function exportContentLabel(prefix, byteCount, digest, executable) {
  return `${prefix}: ${formatBytes(byteCount)} bytes · ${executable ? 'executable' : 'not executable'} · digest ${digest}`;
}

function renderSingleExportPlan(preview) {
  const action = preview.identical
    ? 'No change needed for'
    : preview.target_exists && !preview.replace_allowed
      ? 'Keep existing ordinary file'
    : preview.target_exists
      ? 'Replace saved file'
      : 'Create saved file';
  const lines = [
    `${action}: ${preview.path}`,
    `Destination folder: ${preview.target_root}`,
    '',
    `Saved version: ${preview.source_version}`,
    exportContentLabel('Saved file', preview.source_byte_count, preview.source_content_digest, preview.source_executable),
  ];
  if (typeof preview.source_text === 'string') {
    lines.push('', 'Saved text', '----------', preview.source_text);
  } else {
    lines.push('Saved text preview: unavailable for binary, large, or non-UTF-8 content.');
  }
  if (!preview.target_exists) {
    lines.push('', 'Current destination: file is absent.');
  } else {
    lines.push(
      '',
      exportContentLabel('Current destination', preview.target_byte_count, preview.target_content_digest, preview.target_executable),
    );
    if (typeof preview.target_text === 'string') {
      lines.push('', 'Current ordinary-folder text', '----------------------------', preview.target_text);
    } else {
      lines.push('Current text preview: unavailable for binary, large, or non-UTF-8 content.');
    }
  }
  lines.push(
    '',
    preview.identical
      ? 'Nothing will be written.'
      : !preview.replace_allowed
        ? 'Mesh cannot prove this destination is still the imported baseline or an earlier update from this workspace. It will not replace work from another agent or an external editor.'
      : 'Mesh will recheck these exact saved and destination identities immediately before one atomic write.',
  );
  showExportPlan(lines);
}

function plural(count, singular, pluralForm = `${singular}s`) {
  return `${count} ${count === 1 ? singular : pluralForm}`;
}

function renderCurrentFileExportPlan(targetRoot, previews, identicalCount, preserved = []) {
  const creates = previews.filter((preview) => !preview.target_exists);
  const replacements = previews.filter((preview) => preview.target_exists);
  const lines = [
    `Update ${plural(previews.length, 'saved file')}`,
    `Destination folder: ${targetRoot}`,
    '',
    ...replacements.map((preview) => `Replace  ${preview.path}`),
    ...creates.map((preview) => `Create   ${preview.path}`),
  ];
  if (preserved.length) {
    lines.push(
      '',
      'Keep for manual review',
      ...preserved.map((preview) => `Keep     ${preview.path} — changed outside this workspace`),
    );
  }
  lines.push(
    '',
    `Already identical: ${plural(identicalCount, 'file')}`,
    'This step removes nothing. Mesh rechecks each exact file before its atomic write.',
  );
  showExportPlan(lines);
}

function renderDirectoryExportPlan(targetRoot, paths) {
  showExportPlan([
    `Create ${plural(paths.length, 'saved folder')}`,
    `Destination folder: ${targetRoot}`,
    '',
    ...paths.map((path) => `Create   ${path}`),
    '',
    'This step creates absent folders only. Mesh will preview saved files separately afterward.',
  ]);
}

function retiredReasonLabel(reason) {
  if (reason === 'external-preserved') return 'changed outside this workspace — keep it';
  if (reason === 'changed-preserved') return 'changed since the last update — keep it';
  if (reason === 'nonempty-preserved') return 'folder is not empty — keep it';
  if (reason === 'unproven-preserved') return 'no durable import or update receipt proves this path — keep it';
  if (reason === 'already-absent') return 'already absent';
  return 'could not be verified safely — keep it';
}

function renderRetiredExportPlan(targetRoot, kind, previews, preserved, alreadyAbsent) {
  const noun = kind === 'retired-files' ? 'unchanged old file' : 'old empty folder';
  const lines = [
    `Separate cleanup review: remove ${plural(previews.length, noun)}`,
    `Destination folder: ${targetRoot}`,
    '',
    ...previews.map((preview) => `Remove   ${preview.path}`),
  ];
  if (preserved.length) {
    lines.push('', 'Keep for manual review', ...preserved.map((entry) => `Keep     ${entry.path} — ${retiredReasonLabel(entry.reason)}`));
  }
  if (alreadyAbsent) lines.push('', `Already absent: ${plural(alreadyAbsent, 'path')}`);
  lines.push(
    '',
    kind === 'retired-files'
      ? 'Only files with original-import or exact update provenance that remain byte-for-byte and metadata-identical to their last saved value can be removed.'
      : 'Only exact empty folders with original-import or exact update provenance can be removed. Recursive deletion is unavailable.',
  );
  showExportPlan(lines);
}

function renderExportComplete(targetRoot, preserved, alreadyAbsent) {
  const lines = [
    isOriginalProjectDestination(targetRoot)
      ? 'Original-folder update complete'
      : 'Destination-folder update complete',
    `Destination folder: ${targetRoot}`,
    '',
    'Every safely applicable saved path now matches.',
  ];
  if (preserved.length) {
    lines.push('', 'Keep for manual review', ...preserved.map((entry) => `Keep     ${entry.path} — ${retiredReasonLabel(entry.reason)}`));
  }
  if (alreadyAbsent) lines.push('', `Already absent: ${plural(alreadyAbsent, 'path')}`);
  lines.push('Unrelated files and folders were untouched.');
  showExportPlan(lines);
}

function workspaceDestinationBatchConfirmLabel(batch = model.exportBatchPreview) {
  if (!batch?.paths?.length) return 'Update changed files';
  if (batch.kind === 'directories') {
    return `Create ${batch.paths.length} saved ${batch.paths.length === 1 ? 'folder' : 'folders'}`;
  }
  if (batch.kind === 'retired-files') {
    return `Remove ${batch.paths.length} unchanged old ${batch.paths.length === 1 ? 'file' : 'files'}`;
  }
  if (batch.kind === 'retired-directories') {
    return `Remove ${batch.paths.length} old empty ${batch.paths.length === 1 ? 'folder' : 'folders'}`;
  }
  return `Update ${batch.paths.length} changed ${batch.paths.length === 1 ? 'file' : 'files'}`;
}

function workspaceDestinationActionPresentation() {
  const histories = model.workspace?.file_histories || [];
  const directories = (model.workspace?.entries || []).filter((entry) => entry.type === 'folder');
  const filesAvailable = histories.length > 0;
  const treeAvailable = filesAvailable || directories.length > 0;
  const available = treeAvailable && !managedWorkspaceMutationBlocked() && !workspaceInteractionInFlight();
  const originalApprovalBlocked = originalProjectUpdateBlocked(workspaceDestinationDraft);
  const selectedFileAvailable = histories.some((history) => history.path === workspaceDestinationSelectedFile);
  return Object.freeze([
    Object.freeze({ id: 'choose-destination', label: 'Choose folder', enabled: available }),
    Object.freeze({
      id: 'preview-single',
      label: 'Preview one file',
      enabled: filesAvailable && available && !originalApprovalBlocked
        && selectedFileAvailable && Boolean(workspaceDestinationDraft),
    }),
    Object.freeze({
      id: 'confirm-single',
      label: model.exportPreview?.target_exists ? 'Replace existing file' : 'Create file',
      enabled: filesAvailable && available && !originalApprovalBlocked
        && selectedFileAvailable && Boolean(model.exportPreview)
        && model.exportPreview.path === workspaceDestinationSelectedFile
        && model.exportPreview.target_root === workspaceDestinationDraft
        && !model.exportPreview.identical && model.exportPreview.replace_allowed,
    }),
    Object.freeze({
      id: 'preview-all',
      label: 'Preview saved workspace',
      enabled: available && !originalApprovalBlocked && Boolean(workspaceDestinationDraft),
    }),
    Object.freeze({
      id: 'confirm-batch',
      label: workspaceDestinationBatchConfirmLabel(),
      enabled: available && !originalApprovalBlocked
        && Boolean(model.exportBatchPreview?.paths?.length)
        && model.exportBatchPreview.targetRoot === workspaceDestinationDraft,
    }),
  ]);
}

function workspaceDestinationActionEnabled(id) {
  return workspaceDestinationActionPresentation().some((action) => action.id === id && action.enabled);
}

function selectWorkspaceDestinationFile(value, interactionGeneration = null) {
  const histories = model.workspace?.file_histories || [];
  const authority = new Set(histories.map((history) => history.path));
  const treeAvailable = histories.length > 0
    || (model.workspace?.entries || []).some((entry) => entry.type === 'folder');
  if (!authority.has(value)
    || !treeAvailable
    || managedWorkspaceMutationBlocked()
    || workspaceInteractionInFlight()) return;
  workspaceDestinationSelectedFile = value;
  workspaceDestinationFieldEchoGeneration = interactionGeneration;
  workspaceDestinationFieldEchoField = 'selectedFile';
  try {
    clearExportPreview();
    renderExportChoices();
  } finally {
    workspaceDestinationFieldEchoGeneration = null;
    workspaceDestinationFieldEchoField = null;
  }
}

function renderWorkspaceDestinationNext() {
  if (!workspaceDestinationNextAvailable
    || !model.workspace
    || rememberedWorkspaceNeedsImport(model.workspace)) {
    workspaceDestinationNextPending = null;
    workspaceDestinationNextMounted = null;
    workspaceDestinationNextActions = new Map();
    installWorkspaceDestinationNextVisibility();
    return;
  }
  const generation = ++workspaceDestinationNextGeneration;
  const histories = model.workspace.file_histories || [];
  const actions = new Map();
  const projectedActions = workspaceDestinationActionPresentation();
  const actionAvailable = (id) => projectedActions.some((action) => action.id === id && action.enabled);
  if (actionAvailable('choose-destination')) {
    actions.set('activate:choose-destination', () => chooseExportTarget());
  }
  if (actionAvailable('preview-single')) {
    actions.set('activate:preview-single', (intent) => previewSingleManagedExport({
      relativePath: intent.selectedFile,
      targetRoot: intent.destination,
    }));
  }
  if (actionAvailable('confirm-single')) {
    const preview = model.exportPreview;
    const previewSequence = exportPreviewSequence;
    actions.set('activate:confirm-single', () => confirmSingleManagedExport(preview, previewSequence));
  }
  if (actionAvailable('preview-all')) {
    actions.set('activate:preview-all', (intent) => previewAllManagedExports({
      verifyNativeWork: true,
      expectedTargetRoot: intent.destination,
    }));
  }
  if (actionAvailable('confirm-batch')) {
    const batch = model.exportBatchPreview;
    const previewSequence = exportPreviewSequence;
    actions.set('activate:confirm-batch', () => confirmBatchManagedExport(batch, previewSequence));
  }
  if (histories.length && !managedWorkspaceMutationBlocked() && !workspaceInteractionInFlight()) {
    actions.set('set-field:selectedFile', (intent, interactionGeneration) => {
      selectWorkspaceDestinationFile(intent.value, interactionGeneration);
    });
  }
  if (workspaceDestinationCanEdit) {
    actions.set('set-field:destination', (intent, interactionGeneration) => {
      if (!workspaceDestinationCanEdit) return;
      applyWorkspaceDestinationDraft(intent.value, interactionGeneration);
    });
  }
  const planText = workspaceDestinationPlanText;
  const lowerPlan = planText.toLowerCase();
  const plan = planText
    ? Object.freeze({
      state: lowerPlan.includes('update complete')
        ? 'complete'
        : projectedActions.some((item) => item.enabled && (item.id === 'confirm-single' || item.id === 'confirm-batch'))
          ? 'ready'
          : lowerPlan.includes('keep for manual review') || lowerPlan.includes('keep existing ordinary file')
            ? 'blocked'
            : 'information',
      text: planText,
    })
    : null;
  const continuityKey = workspaceProjectionContinuityKey();
  workspaceDestinationNextPending = Object.freeze({
    generation,
    interactionGeneration: workspaceDestinationFieldEchoGeneration,
    interactionField: workspaceDestinationFieldEchoField,
    continuityKey,
  });
  if (workspaceDestinationNextMounted?.continuityKey !== continuityKey) {
    workspaceDestinationNextMounted = null;
  }
  workspaceDestinationNextActions = actions;
  installWorkspaceDestinationNextVisibility();
  appDocument.dispatchEvent(new appWindow.CustomEvent('mesh:workspace-destination-projection', {
    detail: Object.freeze({
      generation,
      destination: Object.freeze({
        files: Object.freeze(histories.map((history) => Object.freeze({ value: history.path, label: history.path }))),
        selectedFile: workspaceDestinationSelectedFile,
        destination: workspaceDestinationDraft,
        chooserRevision: exportDestinationChooserRevision,
        canSelectFile: histories.length > 0
          && !managedWorkspaceMutationBlocked()
          && !workspaceInteractionInFlight(),
        canEditDestination: workspaceDestinationCanEdit,
        hint: workspaceDestinationHint,
        plan,
        actions: Object.freeze(projectedActions),
      }),
    }),
  }));
}

function renderExportChoices() {
  const histories = model.workspace?.file_histories || [];
  const directories = (model.workspace?.entries || []).filter((entry) => entry.type === 'folder');
  const previous = workspaceDestinationSelectedFile;
  const filesAvailable = histories.length > 0;
  const treeAvailable = filesAvailable || directories.length > 0;
  const available = treeAvailable && !managedWorkspaceMutationBlocked() && !workspaceInteractionInFlight();
  const originalApprovalBlocked = originalProjectUpdateBlocked(workspaceDestinationDraft);
  workspaceDestinationSelectedFile = histories.some((history) => history.path === previous) ? previous : '';
  workspaceDestinationCanEdit = available;
  workspaceDestinationHint = workspaceInstallationMatchesHandoff()
    ? 'Export is paused while this folder is assigned to an agent. After every process using it stops, choose Finish agent handoff, inspect and save the complete result, then export.'
    : originalApprovalBlocked
    ? 'Review and approve the current saved version before updating the original project folder. You may choose another destination for an explicit private export.'
    : filesAvailable
    ? model.exportRoot
      ? `Remembered destination folder: ${model.exportRoot}. Preview one saved file, or preview the whole saved tree. Mesh creates reviewed folders first, then automatically re-previews changed files.`
      : 'Select a saved file and a destination folder, or preview the whole saved tree. Mesh verifies exact folders and files before updating anything.'
    : directories.length
      ? model.exportRoot
        ? `Remembered destination folder: ${model.exportRoot}. Preview the whole saved tree to create its reviewed empty folders.`
        : 'Select a destination folder, then preview the whole saved tree to create its reviewed empty folders.'
    : 'Save a file privately before updating an original or destination folder.';
  renderWorkspaceDestinationNext();
}

function exportCommandParameters(preview, binding) {
  return {
    relativePath: preview.path,
    targetRoot: preview.target_root,
    expectedTargetInstallation: preview.target_installation,
    expectedTargetParentInstallation: preview.target_parent_installation,
    expectedTargetFileInstallation: preview.target_file_installation,
    expectedSourceVersion: preview.source_version,
    expectedSourceDigest: preview.source_content_digest,
    expectedSourceExecutable: preview.source_executable,
    expectedTargetDigest: preview.target_content_digest,
    expectedTargetExecutable: preview.target_executable,
    ...binding,
  };
}

async function previewManagedExport(relativePath, targetRoot, binding) {
  return JSON.parse(await invoke('preview_managed_export', {
    relativePath,
    targetRoot,
    expectedWorkspaceRoot: binding.root,
    expectedWorkspaceDigest: binding.digest,
    expectedWorkspaceInstallation: binding.installation,
  }));
}

function recoveredExportContainsExactSavedFile(actual, expected) {
  return actual
    && actual.path === expected.path
    && actual.target_root === expected.target_root
    && actual.source_version === expected.source_version
    && actual.source_byte_count === expected.source_byte_count
    && actual.source_content_digest === expected.source_content_digest
    && actual.source_executable === expected.source_executable
    && actual.target_exists === true
    && actual.identical === true
    && actual.target_byte_count === expected.source_byte_count
    && actual.target_content_digest === expected.source_content_digest
    && actual.target_executable === expected.source_executable;
}

async function recoverAmbiguousManagedFileExport({ preview, binding }, cause) {
  const refreshed = await refresh();
  if (
    !refreshed
    || !model.workspaceVerified
    || model.workspace?.root !== binding.root
    || model.workspace?.digest !== binding.digest
    || model.workspace?.installation !== binding.installation
  ) {
    clearExportPreview();
    renderExportChoices();
    showNotice(
      `${cause} Mesh could not confirm the private export or reverify the exact saved workspace. Nothing was replayed. Inspect the destination, then choose Refresh before previewing it again.`,
      true,
    );
    return;
  }
  if (!beginWorkspaceTransition()) {
    clearExportPreview();
    renderExportChoices();
    showNotice(
      `${cause} Mesh could not confirm the private export because another workspace action began during recovery. Nothing was replayed. Inspect the destination, then choose Refresh before previewing it again.`,
      true,
    );
    return;
  }
  try {
    const recoveredBinding = captureVerifiedWorkspace();
    const recovered = await previewManagedExport(
      preview.path,
      preview.target_root,
      recoveredBinding,
    );
    assertVerifiedWorkspace(recoveredBinding);
    if (!recoveredExportContainsExactSavedFile(recovered, preview)) {
      throw new Error('the destination does not contain the exact saved bytes');
    }
    const rememberWarning = await rememberWorkspace(recoveredBinding.root, preview.target_root);
    clearExportPreview();
    renderWorkspace();
    showNotice(
      `Mesh lost the private export reply but verified that ${preview.path} now contains the exact saved bytes in ${preview.target_root}. The write was not replayed.${rememberWarning ? ` ${rememberWarning}` : ''}`,
      Boolean(rememberWarning),
    );
  } catch (recoveryError) {
    clearExportPreview();
    renderExportChoices();
    showNotice(
      `${cause} Mesh could not confirm the private export: ${recoveryError} Nothing was replayed. Inspect the destination, then preview the saved file again.`,
      true,
    );
  } finally {
    finishWorkspaceTransition();
  }
}

async function previewManagedExports(paths, targetRoot, binding) {
  const previews = JSON.parse(await invoke('preview_managed_exports', {
    targetRoot,
    expectedWorkspaceRoot: binding.root,
    expectedWorkspaceDigest: binding.digest,
    expectedWorkspaceInstallation: binding.installation,
  }));
  if (!Array.isArray(previews) || previews.length !== paths.length) {
    throw new Error('Mesh returned an incomplete whole-workspace update preview. Nothing was changed.');
  }
  for (let index = 0; index < paths.length; index += 1) {
    if (previews[index]?.path !== paths[index]) {
      throw new Error('Mesh returned the wrong whole-workspace update order. Nothing was changed.');
    }
  }
  return previews;
}

async function previewManagedDirectoryExports(paths, targetRoot, binding) {
  const preview = JSON.parse(await invoke('preview_managed_directory_exports', {
    targetRoot,
    expectedWorkspaceRoot: binding.root,
    expectedWorkspaceDigest: binding.digest,
    expectedWorkspaceInstallation: binding.installation,
  }));
  if (
    preview?.target_root !== targetRoot ||
    typeof preview.target_installation !== 'string' ||
    !preview.target_installation ||
    !Array.isArray(preview.missing_paths)
  ) {
    throw new Error('Mesh returned an invalid whole-workspace folder preview. Nothing was changed.');
  }
  const indexes = new Map(paths.map((path, index) => [path, index]));
  let prior = -1;
  for (const path of preview.missing_paths) {
    const index = indexes.get(path);
    if (typeof path !== 'string' || index === undefined || index <= prior) {
      throw new Error('Mesh returned the wrong whole-workspace folder order. Nothing was changed.');
    }
    prior = index;
  }
  return preview;
}

function retiredExportCommandParameters(preview, binding) {
  return {
    relativePath: preview.path,
    targetRoot: preview.target_root,
    expectedEntryType: preview.type,
    expectedSourceVersion: preview.source_version,
    expectedSourceDigest: preview.source_content_digest,
    expectedSourceExecutable: preview.source_executable,
    expectedTargetInstallation: preview.target_installation,
    expectedTargetParentInstallation: preview.target_parent_installation,
    expectedTargetEntryInstallation: preview.target_entry_installation,
    expectedTargetDigest: preview.target_content_digest,
    expectedTargetExecutable: preview.target_executable,
    ...binding,
  };
}

function rememberPreservedRetiredPath(preserved, path, reason) {
  const prior = preserved.findIndex((entry) => entry.path === path);
  if (prior >= 0) preserved[prior] = { path, reason };
  else preserved.push({ path, reason });
}

async function previewRetiredExports(targetRoot, binding, previewSequence) {
  const entries = JSON.parse(await invoke('discover_retired_exports', {
    expectedWorkspaceRoot: binding.root,
    expectedWorkspaceDigest: binding.digest,
    expectedWorkspaceInstallation: binding.installation,
  }));
  assertCurrentExportPreview(previewSequence);
  assertVerifiedWorkspace(binding);
  const files = binding.retiredFilesDone ? [] : entries.filter((entry) => entry.type === 'file');
  const directories = entries
    .filter((entry) => entry.type === 'folder')
    .sort((left, right) => {
      const depth = right.path.split('/').length - left.path.split('/').length;
      return depth || (left.path < right.path ? -1 : left.path > right.path ? 1 : 0);
    });
  const phase = files.length ? files : directories;
  const kind = files.length ? 'retired-files' : 'retired-directories';
  const previews = [];
  const preserved = [...(binding.retiredPreserved || [])];
  let alreadyAbsent = 0;
  for (const entry of phase) {
    try {
      const preview = JSON.parse(await invoke('preview_retired_export', {
        relativePath: entry.path,
        targetRoot,
        expectedWorkspaceRoot: binding.root,
        expectedWorkspaceDigest: binding.digest,
        expectedWorkspaceInstallation: binding.installation,
      }));
      assertCurrentExportPreview(previewSequence);
      assertVerifiedWorkspace(binding);
      if (preview.removable) previews.push(preview);
      else if (preview.status === 'already-absent') alreadyAbsent += 1;
      else rememberPreservedRetiredPath(preserved, entry.path, preview.status);
    } catch (error) {
      if (error instanceof ExportPreviewSuperseded) throw error;
      rememberPreservedRetiredPath(preserved, entry.path, String(error));
    }
  }
  if (previews.length) {
    model.exportBatchPreview = {
      kind,
      binding: { root: binding.root, digest: binding.digest, installation: binding.installation },
      targetRoot,
      previews,
      paths: previews.map((preview) => preview.path),
      preserved,
      alreadyAbsent,
    };
    renderRetiredExportPlan(targetRoot, kind, previews, preserved, alreadyAbsent);
    renderExportChoices();
    showNotice(kind === 'retired-files'
      ? `The saved tree no longer contains ${previews.length} unchanged old ${previews.length === 1 ? 'file' : 'files'}. Review this separate removal step; ${preserved.length} changed or conflicting ${preserved.length === 1 ? 'path is' : 'paths are'} preserved.`
      : `The saved tree no longer contains ${previews.length} old ${previews.length === 1 ? 'folder' : 'folders'}. Mesh will remove only exact empty folders, deepest first.`);
    return true;
  }
  if (files.length) return previewRetiredExports(targetRoot, {
    ...binding,
    retiredFilesDone: true,
    retiredPreserved: preserved,
  }, previewSequence);
  assertCurrentExportPreview(previewSequence);
  const completionWarning = await finishOriginalPullBackIfComplete(targetRoot, preserved);
  renderExportComplete(targetRoot, preserved, alreadyAbsent);
  renderExportChoices();
  renderNextAction();
  showNotice((preserved.length
    ? `Saved changes were applied. Mesh preserved ${preserved.length} changed or conflicting ordinary-folder ${preserved.length === 1 ? 'path' : 'paths'} for you to resolve manually.`
    : `Saved changes, moves, and deletions are applied to ${targetRoot}. Unrelated ordinary-folder entries were untouched.`)
    + (completionWarning ? ` ${completionWarning}` : ''), Boolean(completionWarning));
  return false;
}

function renderManagementChoices() {
  const entries = model.workspace?.entries || [];
  const recoveryBlocked = managedMutationRecoveryBlocked();
  const stateBlocked = !model.workspaceVerified;
  const awaitingImport = rememberedWorkspaceNeedsImport(model.workspace);
  const inspectionBlocked = model.workspace?.native_inventory_complete === false
    || model.nativeInspectionFailed;
  const agentAssigned = workspaceInstallationMatchesHandoff();
  const draftBlocked = editorDraftPending();
  const available = Boolean(model.workspace)
    && !stateBlocked
    && !recoveryBlocked
    && !awaitingImport
    && !inspectionBlocked
    && !agentAssigned
    && !draftBlocked;
  if (!entries.some((entry) => entry.path === workspaceFilesState.selectedEntry)) {
    workspaceFilesState.selectedEntry = '';
    workspaceFilesState.movePath = '';
  }
  workspaceFilesState.canEditNewPath = available;
  workspaceFilesState.canSelectEntry = Boolean(model.workspace)
    && model.workspaceVerified
    && !awaitingImport
    && entries.length > 0;
  workspaceFilesState.canEditMovePath = available && Boolean(workspaceFilesState.selectedEntry);
  workspaceFilesState.status = stateBlocked && model.workspace
    ? 'Management is paused because the current workspace state could not be verified. Refresh before continuing.'
    : recoveryBlocked
    ? 'Management is paused. Preserve the reported paths and resolve the interrupted local file change before continuing.'
    : inspectionBlocked
    ? 'Management is paused because Mesh could not complete the native folder inspection. Retry Find folder changes before continuing.'
    : agentAssigned
    ? 'This folder is assigned to an agent. Ordinary inspection and changes are paused; Finish agent handoff performs the exact complete inspection before release.'
    : draftBlocked
    ? 'Management is paused while the editor contains unsaved text. Save, copy, or revert that draft before changing another file.'
    : awaitingImport
    ? 'This ordinary folder is read-only in Mesh until you preview it and create the private native workspace.'
    : available
    ? `${entries.length} materialized ${entries.length === 1 ? 'entry' : 'entries'} available to manage.`
    : 'Open a managed workspace to manage its entries.';
}

function workspaceFileActionEnabled(id) {
  if (id === 'open-workspace-folder') {
    return Boolean(
      !workspaceInteractionInFlight()
      && model.workspace
      && model.workspaceVerified
      && !rememberedWorkspaceNeedsImport(model.workspace),
    );
  }
  if (id === 'open-entry' || id === 'reveal-entry') {
    const entry = model.workspace?.entries.find((candidate) => candidate.path === workspaceFilesState.selectedEntry);
    return Boolean(
      !workspaceInteractionInFlight()
      && model.workspaceVerified
      && entry
      && (id === 'open-entry' || entry.type === 'file'),
    );
  }
  if (id === 'create-text' || id === 'create-folder') {
    return workspaceFilesState.canEditNewPath && Boolean(workspaceFilesState.newPath);
  }
  if (id === 'move-entry') {
    return workspaceFilesState.canSelectEntry
      && workspaceFilesState.canEditMovePath
      && Boolean(workspaceFilesState.selectedEntry)
      && Boolean(workspaceFilesState.movePath)
      && workspaceFilesState.movePath !== workspaceFilesState.selectedEntry;
  }
  if (id === 'delete-entry') {
    return workspaceFilesState.canSelectEntry && Boolean(workspaceFilesState.selectedEntry);
  }
  if (id === 'load-file') return workspaceChangesEditorState.canLoadFile;
  if (id === 'preserve-edit') return workspaceChangesEditorState.canPreserveEdit;
  if (id === 'save-private') return workspaceChangesEditorState.canSavePrivate;
  if (id === 'scan-files') return workspaceChangesQueueState.canScan;
  if (id === 'save-all-private') return workspaceChangesQueueState.canSaveAllPrivate;
  if (id === 'record-structural-change') return workspaceChangesQueueState.canRecordStructural;
  return false;
}

function renderEditorChoices() {
  const missingPaths = new Set(model.folderChanges
    .filter((change) => change.native_missing)
    .map((change) => change.path));
  const histories = (model.workspace?.file_histories || [])
    .filter((history) => !missingPaths.has(history.path));
  const nativeFiles = model.workspace?.native_untracked_files || [];
  const paths = [
    ...histories.map((history) => ({ path: history.path, native: false })),
    ...nativeFiles.map((path) => ({ path, native: true })),
  ];
  const previous = workspaceChangesEditorState.selectedFile;
  workspaceChangesEditorState.canSelectFile = !(paths.length === 0
    || !model.workspaceVerified
    || workspaceInstallationMatchesHandoff());
  const nativeScanBlocked = !model.workspace
    || managedMutationBlocked()
    || rememberedWorkspaceNeedsImport(model.workspace)
    || workspaceInteractionInFlight()
    || workspaceInstallationMatchesHandoff();
  // Find folder changes is the recovery action for an incomplete ordinary inspection. Active
  // custody instead uses Finish agent handoff's exact-generation aggregate preflight.
  workspaceChangesQueueState.canScan = !nativeScanBlocked;
  workspaceChangesEditorState.selectedFile = paths.some((entry) => entry.path === previous)
    ? previous
    : '';
  workspaceChangesEditorState.canLoadFile = Boolean(workspaceChangesEditorState.selectedFile)
    && model.workspaceVerified
    && !workspaceInstallationMatchesHandoff();
  if (!workspaceChangesEditorState.selectedFile
    || model.editor?.path !== workspaceChangesEditorState.selectedFile) {
    model.editor = null;
    workspaceChangesEditorState.editorText = '';
    workspaceChangesEditorState.canEditText = false;
    workspaceChangesEditorState.canPreserveEdit = false;
    workspaceChangesEditorState.canSavePrivate = false;
    workspaceChangesEditorState.editState = workspaceChangesEditorState.selectedFile
      ? 'Open the file to edit'
      : 'No file open';
    workspaceChangesEditorState.editVersion = '';
  } else {
    const blocked = managedWorkspaceMutationBlocked();
    const draftPending = editorDraftPending();
    workspaceChangesEditorState.canEditText = !((blocked && !draftPending)
      || !model.editor.text_editable
      || Boolean(model.editor.native_untracked));
    workspaceChangesEditorState.canPreserveEdit = !(blocked
      || Boolean(model.editor.native_untracked)
      || workspaceChangesEditorState.editorText === model.editor.text);
    workspaceChangesEditorState.canSavePrivate = !(blocked || (model.editor.native_untracked
      ? false
      : workspaceChangesEditorState.canPreserveEdit || !model.editor.modified_from_current_version));
  }
  if (managedWorkspaceWriteBlocked()) {
    const draftRecoverable = editorDraftPending()
      && model.editor?.text_editable
      && !model.editor.native_untracked;
    workspaceChangesEditorState.canEditText = Boolean(draftRecoverable);
    workspaceChangesEditorState.canPreserveEdit = false;
    workspaceChangesEditorState.canSavePrivate = false;
    if (draftRecoverable) {
      workspaceChangesEditorState.editState = 'Draft preserved · copy or revert it before Refresh';
    }
  }
  renderFolderChanges();
  reconcileRestoreRecoveryTarget();
  renderNextAction();
}

function installEditorInspection(editor) {
  model.editor = editor;
  workspaceChangesEditorState.selectedFile = editor.path;
  workspaceChangesEditorState.editorText = editor.text ?? '';
  workspaceChangesEditorState.canEditText = !(managedWorkspaceMutationBlocked()
    || !editor.text_editable
    || Boolean(editor.native_untracked));
  workspaceChangesEditorState.canPreserveEdit = false;
  workspaceChangesEditorState.canSavePrivate = !(managedWorkspaceMutationBlocked()
    || (!editor.native_untracked && !editor.modified_from_current_version));
  workspaceChangesEditorState.editState = editor.native_untracked
    ? 'Working · new native file inspected'
    : editor.text_editable
    ? editor.modified_from_current_version
      ? 'Working · local folder change detected'
      : 'Working copy matches private history'
    : editor.modified_from_current_version
      ? 'Working · binary or large local change detected'
      : 'Binary or large file matches private history';
  workspaceChangesEditorState.editVersion = editor.native_untracked
    ? `not yet saved · ${formatBytes(editor.byte_count)} bytes · ${editor.content_digest.slice(0, 16)}…`
    : `current ${editor.current_version.slice(0, 16)}… · ${formatBytes(editor.byte_count)} bytes · ${editor.content_digest.slice(0, 16)}…`;
  renderNextAction();
}

async function recoverAmbiguousManagedTextSave({ sequence, binding, path, editedText, cause }) {
  try {
    const workspace = await call('workspace.state');
    assertCurrentWorkspaceVerification(sequence);
    const checkpoint = JSON.parse(await invoke('managed_checkpoint_state'));
    assertCurrentWorkspaceVerification(sequence);
    if (
      workspace.root !== binding.expectedWorkspaceRoot
      || workspace.digest !== binding.expectedWorkspaceDigest
      || workspace.installation !== binding.expectedWorkspaceInstallation
      || checkpoint.root !== workspace.root
      || checkpoint.workspace_digest !== workspace.digest
      || checkpoint.workspace_installation !== workspace.installation
    ) {
      throw new Error('the open workspace identity changed after the ambiguous write');
    }
    // A working-copy replacement does not append durable history, so the exact record-fold identity
    // must remain unchanged. Install that verified snapshot without clearing the in-window draft,
    // then re-read only the path that was submitted. Matching text proves the intended bytes are now
    // in the native folder without replaying a non-idempotent atomic replacement.
    installWorkspaceSnapshot(workspace, checkpoint);
    const recoveredBinding = captureVerifiedWorkspace();
    const recoveredEditor = JSON.parse(await invoke('inspect_managed_file', {
      relativePath: path,
      expectedWorkspaceRoot: recoveredBinding.root,
      expectedWorkspaceDigest: recoveredBinding.digest,
      expectedWorkspaceInstallation: recoveredBinding.installation,
    }));
    assertVerifiedWorkspace(recoveredBinding);
    if (
      recoveredEditor.path !== path
      || recoveredEditor.text !== editedText
      || recoveredEditor.text_editable !== true
      || recoveredEditor.native_untracked === true
      || recoveredEditor.modified_from_current_version !== true
    ) {
      throw new Error('the exact edited text was not present at the submitted path');
    }
    installEditorInspection(recoveredEditor);
    renderWorkspace();
    showNotice('Mesh lost the working-copy save reply but confirmed that the exact edited text is in the native folder. Save privately when you are ready to add it to durable history.');
    return true;
  } catch (recoveryError) {
    throw new Error(
      `Mesh could not confirm the working-copy save after its reply was lost: ${cause} The exact edited text could not be verified, so the in-window draft remains unsaved; copy it before Refresh if you need another recovery path.`,
      { cause: recoveryError },
    );
  }
}

async function inspectFolderCandidate(candidate, binding) {
  try {
    const inspection = JSON.parse(await invoke(candidate.command, {
      relativePath: candidate.path,
      expectedWorkspaceRoot: binding.root,
      expectedWorkspaceDigest: binding.digest,
      expectedWorkspaceInstallation: binding.installation,
    }));
    if (binding.sequence !== undefined) assertVerifiedWorkspace(binding);
    return inspection;
  } catch (cause) {
    throw new Error(`Mesh could not inspect ${candidate.path}. Refresh and preserve the folder before trying again.`, { cause });
  }
}

async function inspectFolderCandidates(candidates, binding, missingByPath) {
  const inspected = new Array(candidates.length);
  let nextIndex = 0;
  let firstFailure = null;
  const workerCount = Math.min(FOLDER_INSPECTION_CONCURRENCY, candidates.length);
  const workers = Array.from({ length: workerCount }, async () => {
    while (!firstFailure) {
      const index = nextIndex;
      nextIndex += 1;
      if (index >= candidates.length) return;
      const candidate = candidates[index];
      const absent = missingByPath.get(candidate.path);
      if (absent) {
        inspected[index] = { ...absent, native_missing: true, byte_count: 0 };
        continue;
      }
      try {
        inspected[index] = await inspectFolderCandidate(candidate, binding);
      } catch (error) {
        firstFailure ??= error;
      }
    }
  });
  // Do not clear folderScanInFlight while an already-started verified read is still returning.
  // On failure no new candidate starts, every active worker settles, and the whole scan refuses.
  await Promise.all(workers);
  if (firstFailure) throw firstFailure;
  return inspected;
}

async function findFolderChanges(
  binding,
  candidates = folderCandidates(),
  { discoverDirectories = true } = {},
) {
  if (model.workspace?.native_inventory_complete === false) {
    throw new Error('Mesh could not inspect every native folder entry. Fix folder access and retry Find folder changes before saving or review.');
  }
  const directories = discoverDirectories
    ? JSON.parse(await invoke('discover_native_directories', {
        expectedWorkspaceRoot: binding.root,
        expectedWorkspaceDigest: binding.digest,
        expectedWorkspaceInstallation: binding.installation,
      }))
    : [];
  if (discoverDirectories && binding.sequence !== undefined) assertVerifiedWorkspace(binding);
  const missing = JSON.parse(await invoke('discover_native_missing_files', {
    expectedWorkspaceRoot: binding.root,
    expectedWorkspaceDigest: binding.digest,
    expectedWorkspaceInstallation: binding.installation,
  }));
  if (binding.sequence !== undefined) assertVerifiedWorkspace(binding);
  const missingByPath = new Map(missing.map((file) => [file.path, file]));
  // Explicit and focus-triggered scans surface every new directory. The repeating background
  // poll has already traversed the complete native tree inside workspace.state to discover new
  // files and unsupported entries, so it repeats directory discovery only when a nested native
  // file needs an unsaved parent bound for parent-first adoption. Empty-directory discovery can
  // wait for focus or Find folder changes, while missing tracked files remain cheap metadata
  // checks on the known durable paths and are still surfaced on every poll.
  const changes = [
    ...(model.workspace?.native_unsupported_entries || []).map((entry) => ({
      path: entry.path,
      unsupported_kind: entry.kind,
      native_unsupported: true,
      byte_count: 0,
    })),
    ...directories.map((directory) => ({
      ...directory,
      native_directory: true,
      byte_count: 0,
    })),
    ...missing.map((file) => ({
      ...file,
      native_missing: true,
      byte_count: 0,
    })),
  ];
  const inspections = await inspectFolderCandidates(candidates, binding, missingByPath);
  for (const inspection of inspections) {
    // Missing paths were inserted from the complete metadata pass above. Do not duplicate one
    // merely because it also happened to fall inside this tick's content slice.
    if (!inspection.native_missing && (inspection.native_untracked || inspection.modified_from_current_version)) {
      changes.push(inspection);
    }
  }
  return changes;
}

function periodicFolderCandidates(candidates, binding) {
  const native = candidates.filter((candidate) => candidate.command === 'inspect_native_file');
  const tracked = candidates.filter((candidate) => candidate.command !== 'inspect_native_file');
  // New unsaved files are the most useful background signal. Put them at the front whenever the
  // candidate inventory changes, then rotate deterministically through saved files without
  // repeating a prefix before the rest of the workspace has been checked.
  const ordered = [...native, ...tracked];
  const key = `${binding.root}\0${binding.digest}\0${binding.installation}\0${ordered.length}`;
  if (periodicInspectionKey !== key || periodicInspectionCursor >= ordered.length) {
    periodicInspectionKey = key;
    periodicInspectionCursor = 0;
  }
  const selected = ordered.slice(
    periodicInspectionCursor,
    periodicInspectionCursor + PERIODIC_FILE_INSPECTION_LIMIT,
  );
  periodicInspectionCursor += selected.length;
  if (periodicInspectionCursor >= ordered.length) periodicInspectionCursor = 0;
  return selected;
}

function periodicScanNeedsDirectoryDiscovery(workspace = model.workspace) {
  const durablePaths = new Set((workspace?.entries || []).map((entry) => entry.path));
  return (workspace?.native_untracked_files || []).some((path) => {
    let separator = path.lastIndexOf('/');
    while (separator > 0) {
      const parent = path.slice(0, separator);
      if (!durablePaths.has(parent)) return true;
      separator = parent.lastIndexOf('/');
    }
    return false;
  });
}

function folderChangeSummary(change) {
  return {
    path: change.path,
    native_unsupported: Boolean(change.native_unsupported),
    unsupported_kind: change.unsupported_kind,
    native_directory: Boolean(change.native_directory),
    native_untracked: Boolean(change.native_untracked),
    native_missing: Boolean(change.native_missing),
    byte_count: change.byte_count,
    current_version: change.current_version,
    content_digest: change.content_digest,
    executable: change.executable,
    installation: change.installation,
  };
}

function editorDraftPending() {
  return Boolean(
    model.editor?.text_editable
    && workspaceChangesEditorState.editorText !== (model.editor.text ?? ''),
  );
}

function refuseEditorDraftDeparture() {
  if (!editorDraftPending()) return false;
  showNotice('Preserve the open editor draft before switching workspaces. Mesh will not discard text that exists only in this window.', true);
  return true;
}

function refuseEditorDraftRefresh() {
  if (!editorDraftPending()) return false;
  showNotice('Save, copy, or revert the open editor draft before refreshing. Mesh will not replace text that exists only in this window.', true);
  return true;
}

function refuseAgentLaunchWithEditorDraft() {
  if (!editorDraftPending()) return false;
  focusWorkspaceJourney('workspace-changes-next', 'workspace-changes-next', 'workspace-changes-next', 'textarea');
  showNotice('Save, copy, or revert the open editor draft before handing this folder to an agent. The draft exists only in this Mesh window and is not part of the native folder.', true);
  return true;
}

function refuseEditorDraftFileDeparture(requestedPath) {
  if (!model.editor || requestedPath === model.editor.path || !editorDraftPending()) return false;
  workspaceChangesEditorState.selectedFile = model.editor.path;
  workspaceChangesEditorState.canLoadFile = Boolean(workspaceChangesEditorState.selectedFile)
    && model.workspaceVerified
    && !workspaceInstallationMatchesHandoff();
  renderWorkspaceFilesChangesNext();
  showNotice('Save, copy, or revert the open editor draft before opening another file. Mesh will not discard text that exists only in this window.', true);
  return true;
}

async function confirmWorkspaceSwitch({
  allowNativeChanges = true,
  allowUnverified = true,
  blockedAction = 'removing this workspace',
} = {}) {
  // Text typed into the embedded editor has not reached the native folder yet. Switching would
  // clear that workspace-scoped buffer, so do not offer a destructive "switch anyway" path. The
  // same check runs again after every awaited inspection: the editor remains intentionally usable
  // while Mesh reads the native folder, and text typed during that wait is just as valuable as a
  // draft that existed before the switch began.
  if (refuseEditorDraftDeparture()) return false;

  if (!model.workspace) return true;
  if (!model.workspaceVerified) {
    if (!allowUnverified) {
      showNotice(`Refresh and verify the current workspace before ${blockedAction}. Mesh will not continue from state it cannot inspect for newer native work.`, true);
      return false;
    }
    return confirm(
      'Mesh cannot verify the current workspace or scan it for newer native work. Its folder will remain unchanged. '
        + 'Choose Cancel to repair or refresh it, or OK to switch to another workspace.',
    );
  }

  // This is ordinary source material, not a Mesh working copy. Switching away leaves it exactly
  // where it is and must not enter the managed native-change scanner that is disabled below.
  if (rememberedWorkspaceNeedsImport(model.workspace)) return true;

  // An assigned agent folder is intentionally a pinned physical workspace. The agent can be in
  // the middle of replacing or writing a file, so a navigation request must not wait for a stable
  // read of that live folder or mistake partial bytes for a departure-time save candidate. Mesh
  // already holds exact custody of this verified installation, keeps its recent-workspace entry,
  // and requires Finish agent handoff before saving or releasing it. Opening another saved point
  // therefore changes only the stable navigation link; the agent's real directory stays put.
  if (workspaceInstallationMatchesHandoff()) return true;

  // Editors and agents can create a file after the last rendered workspace snapshot. Re-read the
  // live daemon projection and its checkpoint binding before deciding that there is nothing to
  // preserve. Otherwise a newly created native file is absent from the stale candidate list and
  // the switch can appear clean even though work remains in the old independent folder.
  const binding = captureVerifiedWorkspace();
  const { workspace, checkpoint } = await readVerifiedWorkspace(
    () => call('workspace.state'),
    binding.sequence,
  );
  assertVerifiedWorkspace(binding);
  if (
    workspace.root !== binding.root ||
    workspace.digest !== binding.digest ||
    workspace.installation !== binding.installation
  ) {
    throw new WorkspaceVerificationSuperseded();
  }
  installWorkspaceSnapshot(workspace, checkpoint);
  const candidates = folderCandidates(workspace);
  // Directory discovery is independent of file candidates. A folder-only workspace can have no
  // tracked or untracked file rows while an editor or agent has created a new empty directory.
  // Always run the native discovery pass before leaving so that work receives the same explicit
  // preserve-or-leave-behind decision as file changes.
  const changes = await findFolderChanges(binding, candidates);
  assertVerifiedWorkspace(binding);
  model.folderChanges = changes.map(folderChangeSummary);
  renderEditorChoices();
  if (refuseEditorDraftDeparture()) return false;
  if (!changes.length) return true;

  const count = changes.length;
  if (!allowNativeChanges) {
    const hasStructuralChange = changes.some((change) => change.native_missing);
    focusWorkspaceJourney(
      'workspace-changes-next',
      'workspace-changes-next',
      'workspace-changes-next',
      hasStructuralChange
        ? '[data-mesh-work-field="missingSource"]'
        : '[data-mesh-work-action="save-all-private"]',
    );
    showNotice(
      `Mesh found ${count} unsaved native ${count === 1 ? 'change' : 'changes'} in ${binding.root}. Save or revert them before ${blockedAction}.`,
      true,
    );
    return false;
  }
  return confirm(
    `Mesh found ${count} unsaved native ${count === 1 ? 'change' : 'changes'} in ${binding.root}. `
      + 'They will remain in that workspace and are not part of the saved version you are opening. '
      + 'Choose Cancel to review and save them, or OK to switch and leave them there.',
  );
}

function choice(label, value = '') {
  const option = appDocument.createElement('option');
  option.textContent = label;
  option.value = value;
  return option;
}

function renderHistoryChoices() {
  const histories = model.workspace?.file_histories || [];
  const history = histories.find((entry) => entry.object_id === selectedRestoreFileId) || null;
  if (!history) {
    selectedRestoreFileId = '';
    selectedRestoreVersionId = '';
    model.restorePreview = null;
    restoreApplyAttempted = false;
  } else if (!restoreVersionChoices(history).some((entry) => entry.version_id === selectedRestoreVersionId)) {
    selectedRestoreVersionId = '';
    model.restorePreview = null;
    restoreApplyAttempted = false;
  }
  renderRestoreNext();
}

function currentRestoreRecoveryVersion(history) {
  if (!model.workspaceVerified || !history?.current) return null;
  const retainedCurrent = history.retained_versions.find((entry) => (
    restoreVersionIdentityMatches(entry, history.current)
  ));
  if (!retainedCurrent) return null;
  const changedWorkingFile = model.folderChanges.find((change) => (
    change.path === history.path
    && !change.native_untracked
    && !change.native_missing
    && !change.native_directory
    && !change.native_unsupported
    && change.current_version === history.current.version_id
    && typeof change.content_digest === 'string'
    && change.content_digest.length > 0
    && typeof change.executable === 'boolean'
  ));
  return changedWorkingFile ? retainedCurrent : null;
}

function restoreVersionChoices(history) {
  if (!history) return [];
  const earlier = history.retained_versions.filter((entry) => (
    entry.version_id !== history.current?.version_id
  ));
  const recovery = currentRestoreRecoveryVersion(history);
  return recovery ? [recovery, ...earlier] : earlier;
}

function restoreVersionChoiceLabel(history, version) {
  return version.version_id === history?.current?.version_id
    ? `Return to current saved version · ${shortIdentity(version.version_id)}`
    : `Saved version · ${shortIdentity(version.version_id)}`;
}

function reconcileRestoreRecoveryTarget() {
  const history = (model.workspace?.file_histories || [])
    .find((entry) => entry.object_id === selectedRestoreFileId);
  if (!history) return;
  if (selectedRestoreVersionId
    && !restoreVersionChoices(history).some((entry) => entry.version_id === selectedRestoreVersionId)) {
    // A verified native inspection either discovered or cleared recovery work. Revoke any preview
    // derived from the prior byte state before changing the available target set.
    selectedRestoreVersionId = '';
    model.restorePreview = null;
    restoreApplyAttempted = false;
  }
  // Native inspection can add or remove the current saved version as an explicit recovery target
  // while an earlier selected target remains valid. Reproject the source-owned choices even when
  // that selection does not change; there is no hidden select left to act as an availability cache.
  renderRestoreNext();
}

function selectWorkspaceVersion(operation) {
  if (!model.workspaceVerified
    || !(model.workspace?.workspace_versions || [])
      .some((version) => version.operation === operation)) return false;
  selectedWorkspaceVersionOperation = operation;
  workspaceVersionDestinationDraft = '';
  return true;
}

function renderWorkspaceVersionChoices() {
  const versions = model.workspace?.workspace_versions || [];
  // Durable history arrives in causal order, while a version picker is a navigation surface.
  // Reverse that deterministic order, but never call one branch "current" or another "earlier"
  // when multiple concurrent heads exist. Keep the source array untouched because its final entry
  // is also the exact current-point identity only when private history has one head.
  const newestFirst = [...versions].reverse();
  if (!versions.some((version) => version.operation === selectedWorkspaceVersionOperation)) {
    selectedWorkspaceVersionOperation = '';
    workspaceVersionDestinationDraft = '';
    model.workspaceVersionPreview = null;
    model.workspaceVersionPreviewError = null;
  }
  const currentOperation = currentWorkspaceVersion()?.operation || null;
  const concurrentHistory = hasConcurrentWorkspaceHistory();
  const selected = versions.find((version) => (
    version.operation === selectedWorkspaceVersionOperation
  ));
  const preview = model.workspaceVersionPreview;
  const previewMatches = workspaceVersionPreviewStateMatches(preview, selected?.operation);
  const previewErrorMatches = workspaceVersionPreviewStateMatches(
    model.workspaceVersionPreviewError,
    selected?.operation,
  );
  const canSelect = versions.length > 0
    && !managedMutationBlocked()
    && !workspaceInteractionInFlight();
  const canUseCustomLocation = !pendingAgentVersionChoice && canSelect;
  const canOpen = Boolean(
    selected
    && previewMatches
    && !managedWorkspaceWriteBlocked()
    && !workspaceInteractionInFlight(),
  );
  renderWorkspaceVersionsNext({
    newestFirst,
    selected,
    previewMatches,
    previewErrorMatches,
    canSelect,
    canUseCustomLocation,
    canOpen,
  });
}

function renderWorkspaceVersionsNext({
  newestFirst,
  selected,
  previewMatches,
  previewErrorMatches,
  canSelect,
  canUseCustomLocation,
  canOpen,
}) {
  if (!workspaceVersionsNextAvailable || !model.workspace || rememberedWorkspaceNeedsImport(model.workspace)) {
    workspaceVersionsNextPending = null;
    workspaceVersionsNextMounted = null;
    workspaceVersionsNextActions = new Map();
    workspaceVersionsNextInteraction = null;
    installWorkspaceVersionsNextVisibility();
    return;
  }
  const generation = ++workspaceVersionsNextGeneration;
  // Keep the last committed view painted while the same physical workspace reprojects, including
  // background verification. Interaction continuity is narrower: only the connected focused
  // owner may carry a controlled selection or custom-location field echo through the commit gap;
  // opening a version or starting an agent remains generation-bound.
  const interactionGeneration = workspaceVersionsNextInteraction
    && workspaceVersionsNextMounted
    && workspaceVersionsNextInteraction.generation === workspaceVersionsNextMounted.generation
    && workspaceVersionsNextInteraction.owner?.isConnected
    && $('workspace-versions-next').shadowRoot?.activeElement === workspaceVersionsNextInteraction.owner
    ? workspaceVersionsNextMounted.generation
    : null;
  const continuityKey = workspaceProjectionContinuityKey();
  workspaceVersionsNextPending = Object.freeze({ generation, interactionGeneration, continuityKey });
  if (workspaceVersionsNextMounted?.continuityKey !== continuityKey) {
    workspaceVersionsNextMounted = null;
  }
  if (interactionGeneration === null) {
    workspaceVersionsNextInteraction = null;
  }
  installWorkspaceVersionsNextVisibility();
  const actions = new Map();
  const projectedBinding = managedMutationBlocked() ? null : captureVerifiedWorkspace();
  const assertProjectionCurrent = () => {
    if (workspaceVersionsNextPending?.generation !== generation || !projectedBinding) {
      throw new WorkspaceVerificationSuperseded();
    }
    assertVerifiedWorkspace(projectedBinding);
  };
  for (const version of newestFirst) {
    if (canSelect) {
      actions.set(`select-version:${version.operation}`, async () => {
        assertProjectionCurrent();
        if (!(model.workspace?.workspace_versions || [])
          .some((candidate) => candidate.operation === version.operation)
        ) {
          throw new WorkspaceVerificationSuperseded();
        }
        if (!selectWorkspaceVersion(version.operation)) {
          throw new WorkspaceVerificationSuperseded();
        }
        await previewSelectedWorkspaceVersion(version.operation);
      });
    }
  }
  if (selected && previewMatches && canOpen) {
    actions.set(`open-version:${selected.operation}`, () => {
      assertProjectionCurrent();
      if (selectedWorkspaceVersionOperation !== selected.operation) {
        throw new WorkspaceVerificationSuperseded();
      }
      return openSelectedWorkspaceVersion(selected.operation);
    });
    actions.set(`start-codex:${selected.operation}`, () => {
      assertProjectionCurrent();
      if (selectedWorkspaceVersionOperation !== selected.operation) {
        throw new WorkspaceVerificationSuperseded();
      }
      return startCodexFromSelectedWorkspaceVersion(selected.operation);
    });
  }
  if (canUseCustomLocation) {
    actions.set('set-custom-location', (intent) => {
      assertProjectionCurrent();
      if (workspaceVersionsActionKey(intent) !== 'set-custom-location'
        || !canUseCustomLocation) return;
      workspaceVersionDestinationDraft = intent.path;
      renderWorkspaceVersionChoices();
    });
  }
  workspaceVersionsNextActions = actions;
  const preview = previewMatches ? model.workspaceVersionPreview.answer : null;
  const changeLabels = {
    added: 'Added',
    changed: 'Changed',
    removed: 'Removed',
    replaced: 'Replaced',
  };
  const changes = preview
    ? preview.changes.map((change) => `${changeLabels[change.effect]} · ${change.path}${change.type === 'folder' ? '/' : ''}`)
    : [];
  if (preview?.changes_not_listed) changes.push(`…and ${preview.changes_not_listed} more changes`);
  const entries = preview
    ? preview.entries.map((entry) => (
        entry.type === 'file'
          ? `${entry.path} · ${formatBytes(exactByteCount(entry.bytes))} bytes`
          : `${entry.path}/`
      ))
    : [];
  if (preview?.entries_not_listed) entries.push(`…and ${preview.entries_not_listed} more entries`);
  const selectedDescription = selected && selected.operation === currentWorkspaceVersion()?.operation
    ? `Current saved workspace · point ${selected.ordinal}`
    : selected
      ? hasConcurrentWorkspaceHistory()
        ? `Saved point ${selected.ordinal} from concurrent history`
        : `Earlier saved workspace · point ${selected.ordinal}`
      : 'Choose a saved workspace point';
  const previewState = !selected
    ? 'choose'
    : previewMatches
      ? 'ready'
      : previewErrorMatches
        ? 'error'
        : 'loading';
  const previewSummary = preview
    ? `${preview.files} file${preview.files === 1 ? '' : 's'} · ${preview.folders} folder${preview.folders === 1 ? '' : 's'} · ${formatBytes(exactByteCount(preview.total_bytes))} bytes. Exact retained content verified.`
    : previewState === 'error'
      ? 'Mesh could not verify this saved point. Select it again to retry; no folder was created.'
      : previewState === 'loading'
        ? 'Checking its exact retained contents before any native folder can be created…'
        : newestFirst.length
          ? 'Select a point to verify its complete contents and see what changed.'
          : 'Whole-workspace versions appear after durable changes are saved.';
  const currentOperation = currentWorkspaceVersion()?.operation || null;
  appDocument.dispatchEvent(new appWindow.CustomEvent('mesh:workspace-versions-projection', {
    detail: Object.freeze({
      generation,
      versions: Object.freeze({
        historyMode: hasConcurrentWorkspaceHistory() ? 'concurrent' : 'linear',
        versions: Object.freeze(newestFirst.map((version) => Object.freeze({
          operation: version.operation,
          ordinal: version.ordinal,
          relation: hasConcurrentWorkspaceHistory()
            ? 'concurrent'
            : version.operation === currentOperation ? 'current' : 'earlier',
          label: version.operation === currentOperation
            ? `Current saved workspace · point ${version.ordinal}`
            : hasConcurrentWorkspaceHistory()
              ? `Saved workspace · point ${version.ordinal}`
              : `Earlier saved workspace · point ${version.ordinal}`,
        }))),
        selectedOperation: selected?.operation || null,
        previewState,
        previewTitle: selectedDescription,
        previewSummary,
        changeBasis: preview?.change_basis || null,
        basisOrdinal: preview?.basis_ordinal ?? null,
        changes: Object.freeze(changes),
        entries: Object.freeze(entries),
        canSelect,
        canOpen: Boolean(selected && previewMatches && actions.has(`open-version:${selected.operation}`)),
        canStartCodex: Boolean(selected && previewMatches && actions.has(`start-codex:${selected.operation}`)),
        customLocation: workspaceVersionDestinationDraft,
        canUseCustomLocation: actions.has('set-custom-location'),
        openLabel: 'Open in working folder',
        codexLabel: pendingAgentVersionChoice ? 'Start another agent from this point' : 'Start Codex on this point',
      }),
    }),
  }));
}

function workspaceVersionPreviewStateMatches(state, operation) {
  return Boolean(
    operation
    && state
    && state.operation === operation
    && state.root === model.workspace?.root
    && state.digest === model.workspace?.digest
    && state.installation === model.workspace?.installation,
  );
}

function validateWorkspaceVersionPreview(preview, selectedVersion) {
  if (
    !preview
    || preview.action !== 'workspace-version-preview'
    || preview.source_version !== selectedVersion.operation
    || preview.ordinal !== selectedVersion.ordinal
    || preview.actor_sequence !== selectedVersion.actor_sequence
    || preview.content_verified !== true
    || preview.creates_folder !== false
    || !Number.isSafeInteger(preview.ordinal)
    || preview.ordinal < 1
    || !['initial', 'previous-point', 'combined-history'].includes(preview.change_basis)
    || !(preview.basis_ordinal === null
      || (Number.isSafeInteger(preview.basis_ordinal)
        && preview.basis_ordinal >= 1
        && preview.basis_ordinal < preview.ordinal))
    || !Number.isSafeInteger(preview.files)
    || preview.files < 0
    || !Number.isSafeInteger(preview.folders)
    || preview.folders < 0
    || !Number.isSafeInteger(preview.entries_not_listed)
    || preview.entries_not_listed < 0
    || !Array.isArray(preview.entries)
    || preview.entries.length > 24
    || !Number.isSafeInteger(preview.changes_not_listed)
    || preview.changes_not_listed < 0
    || !Array.isArray(preview.changes)
    || preview.changes.length > 24
    || (preview.change_basis === 'previous-point') !== (preview.basis_ordinal !== null)
    || (preview.change_basis === 'combined-history'
      && (preview.changes.length !== 0 || preview.changes_not_listed !== 0))
  ) {
    throw new Error('The saved workspace preview did not match the selected durable point.');
  }
  exactByteCount(preview.total_bytes);
  const totalEntries = preview.files + preview.folders;
  if (
    !Number.isSafeInteger(totalEntries)
    || preview.entries.length + preview.entries_not_listed !== totalEntries
  ) {
    throw new Error('The saved workspace preview did not match the selected durable point.');
  }
  let listedFiles = 0;
  let listedFolders = 0;
  let previousEntryPath = null;
  for (const entry of preview.entries) {
    if (!entry || typeof entry.path !== 'string' || !entry.path || !['file', 'folder'].includes(entry.type)) {
      throw new Error('The saved workspace preview contained an invalid path entry.');
    }
    if (previousEntryPath !== null && entry.path <= previousEntryPath) {
      throw new Error('The saved workspace preview contained an invalid path entry.');
    }
    previousEntryPath = entry.path;
    if (entry.type === 'file') {
      listedFiles += 1;
      exactByteCount(entry.bytes);
    } else {
      listedFolders += 1;
      if (entry.bytes !== null) throw new Error('The saved workspace preview contained an invalid folder size.');
    }
  }
  if (listedFiles > preview.files || listedFolders > preview.folders) {
    throw new Error('The saved workspace preview did not match the selected durable point.');
  }
  let previousChangePath = null;
  for (const change of preview.changes) {
    if (
      !change
      || typeof change.path !== 'string'
      || !change.path
      || !['file', 'folder'].includes(change.type)
      || !['added', 'changed', 'removed', 'replaced'].includes(change.effect)
    ) {
      throw new Error('The saved workspace preview contained an invalid change entry.');
    }
    if (previousChangePath !== null && change.path <= previousChangePath) {
      throw new Error('The saved workspace preview contained an invalid change entry.');
    }
    previousChangePath = change.path;
  }
  return preview;
}

async function previewSelectedWorkspaceVersion(operation = selectedWorkspaceVersionOperation) {
  if (operation !== selectedWorkspaceVersionOperation) return;
  const sequence = ++workspaceVersionPreviewSequence;
  model.workspaceVersionPreview = null;
  model.workspaceVersionPreviewError = null;
  renderWorkspaceVersionChoices();
  if (!operation) return;
  try {
    const binding = captureVerifiedWorkspace();
    const selectedVersion = (model.workspace?.workspace_versions || [])
      .find((version) => version.operation === operation);
    if (!selectedVersion) throw new WorkspaceVerificationSuperseded();
    const answer = validateWorkspaceVersionPreview(JSON.parse(await invoke('preview_managed_workspace_version', {
      operation,
      expectedWorkspaceRoot: binding.root,
      expectedWorkspaceDigest: binding.digest,
      expectedWorkspaceInstallation: binding.installation,
    })), selectedVersion);
    assertVerifiedWorkspace(binding);
    if (sequence !== workspaceVersionPreviewSequence
      || selectedWorkspaceVersionOperation !== operation) return;
    model.workspaceVersionPreview = { operation, ...binding, answer };
    renderWorkspaceVersionChoices();
  } catch (error) {
    if (sequence !== workspaceVersionPreviewSequence
      || selectedWorkspaceVersionOperation !== operation) return;
    model.workspaceVersionPreviewError = {
      operation,
      root: model.workspace?.root,
      digest: model.workspace?.digest,
      installation: model.workspace?.installation,
    };
    showNotice(`Mesh could not verify that saved workspace point: ${error}`, true);
    renderWorkspaceVersionChoices();
  }
}

function currentWorkspaceVersion() {
  const versions = model.workspace?.workspace_versions || [];
  return model.workspace?.private_version?.concurrent_changes === 1
    ? versions.at(-1) || null
    : null;
}

function hasConcurrentWorkspaceHistory() {
  return model.workspace?.private_version?.concurrent_changes > 1
    && (model.workspace.workspace_versions || []).length > 0;
}

function nativeFolderUpgradeCandidate() {
  if (!model.workspace || !model.workspaceVerified) return null;
  if (model.agentFolder?.path) return null;
  if (model.recent?.warning) return null;
  return currentWorkspaceVersion();
}

function revokeRestorePreview() {
  model.restorePreview = null;
  restoreApplyAttempted = false;
}

function selectRestoreFile(objectId) {
  const histories = model.workspace?.file_histories || [];
  if (!model.workspaceVerified || !histories.some((entry) => entry.object_id === objectId)) return;
  if (selectedRestoreFileId === objectId) return;
  selectedRestoreFileId = objectId;
  selectedRestoreVersionId = '';
  revokeRestorePreview();
  renderRestoreNext();
}

function selectRestoreVersion(versionId) {
  const history = (model.workspace?.file_histories || [])
    .find((entry) => entry.object_id === selectedRestoreFileId);
  if (!model.workspaceVerified
    || !restoreVersionChoices(history).some((entry) => entry.version_id === versionId)) return;
  if (selectedRestoreVersionId === versionId) return;
  selectedRestoreVersionId = versionId;
  revokeRestorePreview();
  renderRestoreNext();
}

function restoreVersionIdentityMatches(actual, expected) {
  return actual
    && expected
    && actual.version_id === expected.version_id
    && actual.manifest_id === expected.manifest_id;
}

function restorePreviewHasExactKeys(value, keys) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  return actual.length === expected.length
    && actual.every((key, index) => key === expected[index]);
}

function validRestorePreviewByteCount(value) {
  if (typeof value !== 'string' || !/^(0|[1-9][0-9]*)$/u.test(value)) return false;
  try {
    exactByteCount(value);
    return true;
  } catch {
    return false;
  }
}

function validatedRestorePreview(preview, history, targetVersion, binding) {
  const target = history.retained_versions.find((entry) => entry.version_id === targetVersion);
  const undoTarget = preview?.undo_target;
  const undoValid = undoTarget === null || (
    restorePreviewHasExactKeys(undoTarget, ['version_id', 'manifest_id'])
    && undoTarget.version_id !== targetVersion
    && history.retained_versions.some((entry) => restoreVersionIdentityMatches(undoTarget, entry))
  );
  const valid = preview
    && preview.schema === 'mesh.managed-working-copy-restore-preview/v1'
    && restorePreviewHasExactKeys(preview, [
      'schema',
      'canonical_state_read_only',
      'workspace',
      'object_id',
      'path',
      'current',
      'target',
      'working_copy',
      'history_unchanged',
      'undo_target',
      'execution_authorized',
    ])
    && preview.canonical_state_read_only === true
    && preview.execution_authorized === false
    && preview.history_unchanged === true
    && restorePreviewHasExactKeys(preview.workspace, ['root', 'digest', 'installation'])
    && preview.workspace.root === binding.root
    && preview.workspace.digest === binding.digest
    && preview.workspace.installation === binding.installation
    && preview.object_id === history.object_id
    && preview.path === history.path
    && restorePreviewHasExactKeys(preview.current, ['version_id', 'manifest_id'])
    && restoreVersionIdentityMatches(preview.current, history.current)
    && restorePreviewHasExactKeys(preview.target, [
      'version_id', 'manifest_id', 'content_digest', 'byte_count', 'executable',
    ])
    && restoreVersionIdentityMatches(preview.target, target)
    && typeof preview.target.content_digest === 'string'
    && /^[0-9a-f]{64}$/u.test(preview.target.content_digest)
    && validRestorePreviewByteCount(preview.target.byte_count)
    && typeof preview.target.executable === 'boolean'
    && restorePreviewHasExactKeys(preview.working_copy, [
      'content_digest', 'byte_count', 'executable', 'modified_from_current_version',
    ])
    && typeof preview.working_copy.content_digest === 'string'
    && /^[0-9a-f]{64}$/u.test(preview.working_copy.content_digest)
    && validRestorePreviewByteCount(preview.working_copy.byte_count)
    && typeof preview.working_copy.executable === 'boolean'
    && typeof preview.working_copy.modified_from_current_version === 'boolean'
    && (targetVersion !== history.current?.version_id
      || preview.working_copy.modified_from_current_version === true)
    && (preview.working_copy.content_digest !== preview.target.content_digest
      || preview.working_copy.executable !== preview.target.executable)
    && undoValid;
  if (!valid) {
    throw new Error('Mesh could not verify that the restore preview matches the selected file and saved versions. Choose Preview restore again.');
  }
  return preview;
}

function restoreFileFormat(path) {
  const extension = String(path ?? '').split('.').at(-1)?.toLowerCase();
  if (extension === 'pdf') return 'PDF';
  if (extension === 'doc' || extension === 'docx') return 'Word';
  if (extension === 'ppt' || extension === 'pptx') return 'PowerPoint';
  if (extension === 'xls' || extension === 'xlsx') return 'Excel';
  if (['txt', 'md', 'markdown', 'csv', 'json', 'yaml', 'yml', 'toml', 'xml', 'html', 'css', 'js', 'jsx', 'ts', 'tsx', 'py', 'rs'].includes(extension)) return 'Text';
  return 'File';
}

function renderRestoreNext() {
  if (!workspaceRestoreNextAvailable) return;
  const generation = ++workspaceRestoreNextGeneration;
  const histories = model.workspace?.file_histories || [];
  const history = histories.find((entry) => entry.object_id === selectedRestoreFileId) || null;
  const versions = restoreVersionChoices(history);
  const recovery = currentRestoreRecoveryVersion(history);
  const storedPreview = model.restorePreview?.preview || null;
  const previewMatchesSelection = Boolean(
    history
    && selectedRestoreVersionId
    && storedPreview
    && storedPreview.object_id === history.object_id
    && storedPreview.current?.version_id === history.current?.version_id
    && storedPreview.current?.manifest_id === history.current?.manifest_id
    && storedPreview.target?.version_id === selectedRestoreVersionId
    && versions.some((version) => (
      version.version_id === storedPreview.target.version_id
      && version.manifest_id === storedPreview.target.manifest_id
    )),
  );
  const canSelectFile = histories.length > 0 && model.workspaceVerified;
  const canSelectVersion = Boolean(history && versions.length > 0 && model.workspaceVerified);
  const canPreview = Boolean(
    canSelectVersion
    && selectedRestoreVersionId
    && versions.some((version) => version.version_id === selectedRestoreVersionId),
  );
  const canApply = previewMatchesSelection
    && !restoreApplyAttempted
    && !managedWorkspaceMutationBlocked()
    && !editorDraftPending();
  const canUndo = Boolean(
    model.restoreUndo
    && !managedWorkspaceMutationBlocked()
    && !editorDraftPending(),
  );
  // Keep the last committed view painted while the same physical workspace reprojects, including
  // background verification. Interaction continuity is narrower: only the connected focused
  // owner may carry a controlled selection through the commit gap, and Apply/Undo remain bound to
  // the exact replacement generation.
  const interactionGeneration = workspaceRestoreNextInteraction
    && workspaceRestoreNextMounted
    && workspaceRestoreNextInteraction.generation === workspaceRestoreNextMounted.generation
    && workspaceRestoreNextInteraction.owner?.isConnected
    && $('workspace-restore-next').shadowRoot?.activeElement === workspaceRestoreNextInteraction.owner
    ? workspaceRestoreNextMounted.generation
    : null;
  const continuityKey = workspaceProjectionContinuityKey();
  workspaceRestoreNextPending = Object.freeze({ generation, interactionGeneration, continuityKey });
  if (workspaceRestoreNextMounted?.continuityKey !== continuityKey) {
    workspaceRestoreNextMounted = null;
  }
  if (interactionGeneration === null) {
    workspaceRestoreNextInteraction = null;
  }
  installWorkspaceRestoreNextVisibility();
  const actions = new Map();
  if (canSelectFile) {
    for (const candidate of histories) {
      actions.set(`select-file:${candidate.object_id}`, () => {
        selectRestoreFile(candidate.object_id);
      });
    }
  }
  if (canSelectVersion) {
    for (const candidate of versions) {
      actions.set(`select-version:${candidate.version_id}`, () => {
        selectRestoreVersion(candidate.version_id);
      });
    }
  }
  if (canPreview) actions.set('preview', previewSelectedRestore);
  if (canApply) actions.set('apply', applySelectedRestore);
  if (canUndo) actions.set('undo', undoSelectedRestore);
  workspaceRestoreNextActions = actions;
  const selectedFormat = restoreFileFormat(history?.path);
  appDocument.dispatchEvent(new appWindow.CustomEvent('mesh:workspace-restore-projection', {
    detail: Object.freeze({
      generation,
      restore: Object.freeze({
        files: Object.freeze(histories.map((candidate) => Object.freeze({
          id: candidate.object_id,
          path: candidate.path,
          label: `${candidate.path} · ${candidate.retained_versions.length} saved ${candidate.retained_versions.length === 1 ? 'version' : 'versions'}`,
          format: restoreFileFormat(candidate.path),
        }))),
        selectedFileId: selectedRestoreFileId,
        versions: Object.freeze(versions.map((candidate) => Object.freeze({
          id: candidate.version_id,
          label: restoreVersionChoiceLabel(history, candidate),
        }))),
        selectedVersionId: selectedRestoreVersionId,
        canSelectFile,
        canSelectVersion,
        canPreview: actions.has('preview'),
        canApply: actions.has('apply'),
        canUndo: actions.has('undo'),
        hint: history
          ? recovery
            ? `This working file differs from private history. Return it to the current saved version, or choose an earlier version, after previewing the exact retained bytes.`
            : `This file has ${history.retained_versions.length} immutable saved ${history.retained_versions.length === 1 ? 'version' : 'versions'}. Choose an earlier version to preview its exact retained bytes.`
          : histories.length
            ? 'Choose a file to see its earlier saved versions.'
            : 'Retained history will appear after a managed workspace is open and a file has been saved.',
        preview: previewMatchesSelection
          ? Object.freeze({
              filePath: history.path,
              format: selectedFormat,
              currentVersion: storedPreview.current.version_id,
              targetVersion: storedPreview.target.version_id,
              change: storedPreview.target.version_id === history.current?.version_id
                ? 'Return this file in the managed working folder to the exact retained bytes from the current saved version.'
                : 'Replace this file in the managed working folder with the exact retained bytes from the selected earlier saved version.',
              historyNote: 'Private history stays unchanged until you choose Save privately.',
              undoNote: storedPreview.undo_target
                ? 'Available for the exact retained version currently in the working copy, while the restored bytes still match.'
                : 'Unavailable because the current working bytes are not a retained saved version.',
            })
          : null,
        undoLabel: 'Undo last restore',
      }),
    }),
  }));
}

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (character) => ({ '&':'&amp;', '<':'&lt;', '>':'&gt;', '"':'&quot;', "'":'&#39;' })[character]);
}

async function refresh(startup = false) {
  if (workspaceInteractionInFlight()) {
    if (!startup) showNotice('A local workspace change is still finishing. Refresh will be available when it completes.', true);
    return false;
  }
  if (!startup && refuseEditorDraftRefresh()) return false;
  // Read process-shared agent custody before verifying the live daemon. That native read can
  // outlive a user-initiated open or another Refresh, so bind the continuation to the verification
  // generation that existed when startup began. Merely checking `workspaceMutationInFlight`
  // before the await is insufficient: the newer operation may start and finish while this read is
  // pending, leaving the flag false again but the result obsolete.
  const requestedAgainst = workspaceVerificationSequence;
  const priorWorkspace = model.workspace
    ? {
        root: model.workspace.root,
        digest: model.workspace.digest,
        installation: model.workspace.installation,
      }
    : null;
  try {
    const recent = JSON.parse(await invoke('recent_workspace_status'));
    if (
      workspaceInteractionInFlight() ||
      requestedAgainst !== workspaceVerificationSequence
    ) return false;
    if (startup) installNavigationStatus(recent);
    else {
      installBuildIdentity(recent);
      installAgentHandoffStatus(recent);
    }
  } catch (error) {
    if (
      workspaceInteractionInFlight() ||
      requestedAgainst !== workspaceVerificationSequence
    ) return false;
    if (startup) installBuildIdentity(null);
    showNotice(
      startup
        ? `Mesh could not read its recent-workspace status: ${error}`
        : `Mesh could not verify current agent-folder custody: ${error} Refresh again before handing this workspace to an agent.`,
      true,
    );
    if (!startup) {
      markWorkspaceUnverified();
      return false;
    }
  }
  // The recent-workspace/build read above yields to the webview. A draft can appear while that
  // native request is pending, so repeat the guard before starting the verified workspace read.
  if (!startup && refuseEditorDraftRefresh()) return false;
  const sequence = beginWorkspaceVerification();
  let verified = false;
  try {
    const { workspace, checkpoint } = await readVerifiedWorkspace(() => call('workspace.state'), sequence);
    const firstRestoredBinding = startup
      && Boolean(model.recent?.auto_opened)
      && !priorWorkspace;
    if (
      (firstRestoredBinding && Boolean(model.activeFolder?.path))
      || stableNavigationRepairPending
      || (
        Boolean(model.activeFolder?.path)
        && priorWorkspace
        && (
          priorWorkspace.root !== workspace.root
          || priorWorkspace.digest !== workspace.digest
          || priorWorkspace.installation !== workspace.installation
        )
      )
    ) {
      const navigation = JSON.parse(await invoke('reconcile_managed_workspace_navigation', {
        expectedWorkspaceRoot: workspace.root,
        expectedWorkspaceDigest: workspace.digest,
        expectedWorkspaceInstallation: workspace.installation,
      }));
      assertCurrentWorkspaceVerification(sequence);
      if (navigation.workspace_root !== workspace.root) {
        throw new Error('Mesh reconciled the stable native folder to a different workspace.');
      }
      model.activeFolder = navigation.path ? navigation : null;
      stableNavigationRepairPending = false;
    }
    // The workspace read and optional navigation repair also yield. Never let their newer digest
    // clear text typed into the embedded editor while Refresh was in flight.
    if (!startup && refuseEditorDraftRefresh()) return false;
    installWorkspaceSnapshot(workspace, checkpoint);
    renderWorkspace();
    verified = true;
    const restoredWorkspace = startup && Boolean(model.recent?.auto_opened);
    if (restoredWorkspace) {
      showNotice('Your last managed workspace reopened automatically from durable local history.');
    }
    // Rendering happened before this awaited scan, so the first verified workspace frame remains
    // usable while Mesh compares native bytes in the background. Inspect the complete restored
    // tree: the daemon can enumerate new paths directly, but only an exact read can reveal edits
    // to tracked files made while the desktop was closed.
    if (restoredWorkspace) {
      await scanNativeFolder({ automatic: true, startup: true });
      // The recovery verifier runs after the configured idle interval. Re-read once after both
      // that bounded startup window and the native scan, so it cannot invalidate a long scan's
      // workspace binding halfway through.
      setTimeout(() => refresh(), 250);
    }
  } catch (error) {
    if (sequence !== workspaceVerificationSequence || error instanceof WorkspaceVerificationSuperseded) return false;
    if (error instanceof DaemonRefusal && error.code === 'no-workspace-open') {
      model.workspace = null;
      model.checkpoint = null;
      clearWorkspaceScopedState();
      model.workspaceVerified = true;
      setLocalService('ready');
      renderWorkspace();
      verified = true;
    } else {
      markWorkspaceUnverified();
      showNotice(String(error), true);
    }
  }
  if (startup && model.recent?.warning) showNotice(model.recent.warning, true);
  return verified;
}

async function retryWorkspaceEntryState() {
  if (workspaceEntryRefreshInFlight
    || workspaceEntrySelectionInFlight > 0
    || workspaceInteractionInFlight()) return false;
  workspaceEntryRefreshInFlight = true;
  renderNextAction();
  try {
    const refreshed = await refresh(false);
    if (refreshed) {
      showNotice(model.workspace
        ? 'Mesh refreshed and verified the current managed workspace.'
        : 'Mesh refreshed and verified that no managed workspace is open.');
    }
    return refreshed;
  } finally {
    workspaceEntryRefreshInFlight = false;
    renderNextAction();
  }
}

function beginSourceSelection(interactionGeneration = null) {
  if (importWorkbenchNextInteraction?.generation !== interactionGeneration
    || importWorkbenchNextInteraction.selectionSequence !== null) {
    importWorkbenchNextInteraction = null;
  }
  const sequence = ++workspaceSelectionSequence;
  // Starting a new choice revokes the prior confirmation immediately. Waiting for the native
  // picker would leave the old source/destination pair actionable under a decision the user is
  // actively replacing.
  model.source = null;
  model.destination = null;
  model.preview = null;
  importDestinationChooserInFlight = null;
  renderPreview();
  return sequence;
}

function clearCommittedImportSelection() {
  model.source = null;
  model.destination = null;
  model.preview = null;
  importSourceDraft = '';
  importDestinationChooserInFlight = null;
  renderPreview();
}

function sourceFolderChooserAuthorityIsCurrent(authority) {
  return Boolean(
    authority
    && authority.verificationSequence === workspaceVerificationSequence
    && authority.continuityKey === workspaceProjectionContinuityKey()
  );
}

async function previewSource(
  source,
  sequence,
  { requiredSourceScope = null, expectedWorkspace = null, chooserAuthority = null } = {},
) {
  if (sequence !== workspaceSelectionSequence) return;
  if (chooserAuthority && !sourceFolderChooserAuthorityIsCurrent(chooserAuthority)) {
    throw new WorkspaceVerificationSuperseded();
  }
  const path = String(source ?? '');
  if (!path) {
    showNotice('Enter an absolute folder path to preview.', true);
    return;
  }
  const preview = await call('folder.import.preview', { source: path });
  if (sequence !== workspaceSelectionSequence) return;
  if (chooserAuthority && !sourceFolderChooserAuthorityIsCurrent(chooserAuthority)) {
    throw new WorkspaceVerificationSuperseded();
  }
  validateImportFilePreview(preview);
  if (
    preview?.source_scope !== undefined
    && !['ordinary-folder', 'open-zero-history-workspace'].includes(preview.source_scope)
  ) {
    throw new Error('Mesh returned an import preview with an unknown source scope. Nothing was copied.');
  }
  if (requiredSourceScope && preview?.source_scope !== requiredSourceScope) {
    throw new Error('Mesh no longer holds this exact zero-history folder. Refresh before previewing it again; nothing was copied.');
  }
  if (expectedWorkspace) assertVerifiedWorkspace(expectedWorkspace);
  model.source = path;
  importSourceDraft = path;
  model.preview = preview;
  renderPreview();
  showNotice(
    currentWorkspaceNavigationMissing() && model.checkpoint?.confirmed_import_receipt === true
      ? 'Preview complete. Review the exact summary, then connect the original folder. Mesh will reuse the existing managed workspace only when its private origin receipts agree.'
      : 'Preview complete. Review the exact summary, then create the workspace. Mesh manages its private location unless you choose a custom one.',
  );
}

async function previewCurrentWorkspaceImport() {
  let expectedWorkspace;
  let sequence = null;
  try {
    expectedWorkspace = captureVerifiedWorkspace();
    if (
      !rememberedWorkspaceNeedsImport(model.workspace)
      || !rememberedWorkspaceHasUnversionedContent(model.workspace)
    ) {
      throw new WorkspaceVerificationSuperseded();
    }
  } catch (error) {
    showNotice(`Mesh could not verify the folder that needs preserving: ${error}`, true);
    return;
  }
  workspaceEntrySelectionInFlight += 1;
  renderNextAction();
  try {
    const interactionGeneration = await seedCrossIslandImportInteraction();
    assertVerifiedWorkspace(expectedWorkspace);
    sequence = beginSourceSelection(interactionGeneration);
    await previewSource(expectedWorkspace.root, sequence, {
      requiredSourceScope: 'open-zero-history-workspace',
      expectedWorkspace,
    });
  } catch (error) {
    if (sequence === null || sequence === workspaceSelectionSequence) showNotice(String(error), true);
  } finally {
    workspaceEntrySelectionInFlight = Math.max(0, workspaceEntrySelectionInFlight - 1);
    renderPreview(importWorkbenchNextMounted?.generation ?? null);
  }
}

async function seedCrossIslandImportInteraction() {
  if (importWorkbenchNextInteraction?.selectionSequence === null) {
    return importWorkbenchNextInteraction.generation;
  }
  const pending = importWorkbenchNextPending;
  const mounted = importWorkbenchNextMounted;
  if (!pending
    || !mounted
    || pending.generation !== mounted.generation
    || pending.phase !== 'select') {
    focusReactWorkspacePage('import', IMPORT_CHOOSER_FOCUS_SELECTOR);
    return null;
  }
  const shell = $('mesh-app-next');
  const host = $('import-workbench-next');
  const generation = mounted.generation;
  if (shell?.getAttribute('data-mesh-react-shell-active') !== 'true'
    || !host?.shadowRoot) {
    focusReactWorkspacePage('import', IMPORT_CHOOSER_FOCUS_SELECTOR);
    return null;
  }
  const focused = await new Promise((resolve) => {
    let settled = false;
    const finish = (accepted) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      appDocument.removeEventListener('mesh:react-shell-committed', inspect);
      resolve(accepted);
    };
    const inspect = () => {
      const owner = host.shadowRoot?.querySelector?.(IMPORT_CHOOSER_FOCUS_SELECTOR) ?? null;
      if (importWorkbenchNextPending?.generation !== generation
        || importWorkbenchNextMounted?.generation !== generation
        || importWorkbenchNextPending.phase !== 'select') {
        finish(false);
      } else if (owner?.isConnected && host.shadowRoot?.activeElement === owner) {
        finish(true);
      }
    };
    const timer = setTimeout(() => finish(false), 250);
    appDocument.addEventListener('mesh:react-shell-committed', inspect);
    focusReactWorkspacePage('import', IMPORT_CHOOSER_FOCUS_SELECTOR);
    host.shadowRoot?.querySelector?.(IMPORT_CHOOSER_FOCUS_SELECTOR)?.focus?.({ preventScroll: true });
    inspect();
  });
  if (!focused
    || importWorkbenchNextPending?.generation !== generation
    || importWorkbenchNextMounted?.generation !== generation
    || importWorkbenchNextPending.phase !== 'select') return null;
  const owner = host.shadowRoot?.querySelector?.(IMPORT_CHOOSER_FOCUS_SELECTOR) ?? null;
  if (!owner?.isConnected || host.shadowRoot?.activeElement !== owner) return null;
  appDocument.dispatchEvent(new appWindow.CustomEvent('mesh:import-workbench-external-focus', {
    detail: Object.freeze({ generation }),
  }));
  importWorkbenchNextInteraction = Object.freeze({ generation, selectionSequence: null });
  return generation;
}

async function chooseSourceFolder(chooserAuthority = Object.freeze({
  verificationSequence: workspaceVerificationSequence,
  continuityKey: workspaceProjectionContinuityKey(),
})) {
  if (!sourceFolderChooserAuthorityIsCurrent(chooserAuthority)
    || workspaceInteractionInFlight()
    || workspaceEntryRefreshInFlight) {
    showNotice('The workspace changed before Mesh could choose a source folder. Refresh and try again.', true);
    return;
  }
  let sequence = null;
  workspaceEntrySelectionInFlight += 1;
  renderNextAction();
  try {
    const interactionGeneration = await seedCrossIslandImportInteraction();
    if (!sourceFolderChooserAuthorityIsCurrent(chooserAuthority)) {
      showNotice('The workspace changed before Mesh could choose a source folder. Refresh and try again.', true);
      return;
    }
    sequence = beginSourceSelection(interactionGeneration);
    if (interactionGeneration !== null && importWorkbenchNextInteraction?.selectionSequence === null) {
      importWorkbenchNextInteraction = Object.freeze({
        generation: importWorkbenchNextInteraction.generation,
        selectionSequence: sequence,
      });
    }
    const source = await invoke('pick_folder');
    if (sequence !== workspaceSelectionSequence) return;
    if (!source) return;
    if (!sourceFolderChooserAuthorityIsCurrent(chooserAuthority)) {
      throw new WorkspaceVerificationSuperseded();
    }
    await previewSource(source, sequence, { chooserAuthority });
  } catch (error) {
    if (sequence === null || sequence === workspaceSelectionSequence) {
      showNotice(
        error instanceof WorkspaceVerificationSuperseded
          ? 'The workspace changed while Mesh was choosing the source folder. Nothing was previewed; try again from the current workspace.'
          : String(error),
        true,
      );
    }
  } finally {
    if (sequence !== null && importWorkbenchNextInteraction?.selectionSequence === sequence && !model.preview) {
      importWorkbenchNextInteraction = null;
    }
    workspaceEntrySelectionInFlight = Math.max(0, workspaceEntrySelectionInFlight - 1);
    renderPreview(importWorkbenchNextMounted?.generation ?? null);
  }
}

async function previewImportSourceDraft() {
  const interactionGeneration = importWorkbenchNextInteraction?.selectionSequence === null
    ? importWorkbenchNextInteraction.generation
    : null;
  const sequence = beginSourceSelection(interactionGeneration);
  if (interactionGeneration !== null && importWorkbenchNextInteraction?.selectionSequence === null) {
    importWorkbenchNextInteraction = Object.freeze({
      generation: importWorkbenchNextInteraction.generation,
      selectionSequence: sequence,
    });
  }
  try {
    await previewSource(importSourceDraft, sequence);
  } catch (error) {
    if (sequence === workspaceSelectionSequence) showNotice(String(error), true);
  } finally {
    if (importWorkbenchNextInteraction?.selectionSequence === sequence && !model.preview) {
      importWorkbenchNextInteraction = null;
    }
  }
}

function importPreviewAuthorityIsCurrent(authority) {
  return Boolean(
    authority
    && authority.generation === importWorkbenchNextPending?.generation
    && authority.generation === importWorkbenchNextMounted?.generation
    && importPreviewStateAuthorityIsCurrent(authority)
  );
}

function importPreviewStateAuthorityIsCurrent(authority) {
  return Boolean(
    authority
    && authority.selection === workspaceSelectionSequence
    && authority.source === model.source
    && authority.destination === model.destination
    && authority.summary === (model.preview?.summary ?? null)
  );
}

async function chooseImportDestination(authority = null) {
  if (authority && !importPreviewAuthorityIsCurrent(authority)) {
    showNotice('The import preview changed before Mesh could choose its private location. Review it and try again.', true);
    return false;
  }
  if (!model.preview || workspaceInteractionInFlight() || importDestinationChooserInFlight !== null) {
    showNotice('The import preview is not ready for a private-location choice.', true);
    return false;
  }
  const sequence = ++workspaceSelectionSequence;
  importDestinationChooserInFlight = sequence;
  renderPreview(importWorkbenchNextMounted?.generation ?? null);
  // Opening the optional custom-location chooser revokes the default confirmation until the
  // choice either resolves or is cancelled. Otherwise the person could click the still-enabled
  // default action while an older picker promise is about to publish a different destination.
  try {
    const parent = await invoke('pick_folder');
    if (sequence !== workspaceSelectionSequence) return false;
    if (!parent) return false;
    model.destination = `${parent.replace(/\/$/, '')}/${managedName(model.source, model.preview.summary)}`;
    return true;
  } catch (error) {
    if (sequence === workspaceSelectionSequence) showNotice(String(error), true);
    return false;
  } finally {
    if (importDestinationChooserInFlight === sequence) {
      importDestinationChooserInFlight = null;
      renderPreview(importWorkbenchNextMounted?.generation ?? null);
    }
  }
}

async function chooseExportTarget({ privateOnly = false } = {}) {
  const selectionSequence = ++exportDestinationSelectionSequence;
  const inputSequence = exportDestinationInputSequence;
  const destinationAtStart = workspaceDestinationDraft;
  const workspaceBinding = model.workspace && Object.freeze({
    root: model.workspace.root,
    installation: model.workspace.installation,
  });
  clearExportPreview();
  try {
    const path = await invoke('pick_folder');
    const destinationInputChanged = inputSequence !== exportDestinationInputSequence
      && workspaceDestinationDraft !== destinationAtStart;
    const workspaceChanged = !workspaceBinding
      || model.workspace?.root !== workspaceBinding.root
      || model.workspace?.installation !== workspaceBinding.installation;
    if (selectionSequence !== exportDestinationSelectionSequence) {
      await invoke('renderer_proof_checkpoint', { code: 'private-export-picker-superseded' }).catch(() => {});
      throw new ExportDestinationSelectionSuperseded();
    }
    if (destinationInputChanged) {
      await invoke('renderer_proof_checkpoint', { code: 'private-export-input-superseded' }).catch(() => {});
      throw new ExportDestinationSelectionSuperseded();
    }
    if (workspaceChanged) {
      await invoke('renderer_proof_checkpoint', { code: 'private-export-workspace-superseded' }).catch(() => {});
      throw new ExportDestinationSelectionSuperseded();
    }
    if (!path) {
      focusReactWorkspacePage('update', '[data-mesh-proof="destination-choose"]');
      showNotice('No destination folder was selected. Choose a different ordinary folder when you are ready to make the private copy.');
      return;
    }
    if (privateOnly && isOriginalProjectDestination(path)) {
      workspaceDestinationDraft = '';
      renderExportChoices();
      focusReactWorkspacePage('update', '[data-mesh-proof="destination-choose"]');
      showNotice('That is the original project folder. Choose a different ordinary folder for this private copy. Updating the original is a separate action that remains approval-gated.', true);
      return;
    }
    workspaceDestinationDraft = path;
    exportDestinationChooserRevision = exportDestinationChooserRevision === Number.MAX_SAFE_INTEGER
      ? 0
      : exportDestinationChooserRevision + 1;
    await invoke('renderer_proof_checkpoint', { code: 'private-export-coordinator-accepted' }).catch(() => {});
    renderExportChoices();
    if (originalProjectUpdateBlocked(path)) {
      focusReactWorkspacePage('update', '[data-mesh-proof="destination-choose"]');
      showNotice('That is the original project folder. This build cannot approve an original-folder update. Choose a different ordinary folder for the private copy.', true);
      return;
    }
    focusReactWorkspacePage('update', '[data-mesh-proof="destination-preview-all"]');
    showNotice('Destination selected. Choose Preview saved workspace to inspect the exact create, replace, and remove plan before anything is changed.');
  } catch (error) {
    if (error instanceof ExportDestinationSelectionSuperseded) return;
    showNotice(String(error), true);
  }
}

async function choosePrivateExportTarget() {
  if (!explicitPrivateExportAvailable() || !workspaceDestinationActionEnabled('choose-destination')) {
    showNotice('Private export is no longer available for this exact saved version. Refresh before choosing a destination.', true);
    return false;
  }
  await chooseExportTarget({ privateOnly: true });
  return true;
}

function applyWorkspaceDestinationDraft(value, interactionGeneration = null) {
  if (!workspaceDestinationCanEdit) return;
  // Direct typing is a newer destination choice and must supersede an unresolved native picker.
  // Bind the accepted field echo to the exact still-mounted React generation. A stale or forged
  // field value never reaches this function because the destination intent gate remains closed.
  workspaceDestinationDraft = value;
  exportDestinationInputSequence += 1;
  workspaceDestinationFieldEchoGeneration = interactionGeneration;
  workspaceDestinationFieldEchoField = 'destination';
  try {
    clearExportPreview();
    renderExportChoices();
  } finally {
    workspaceDestinationFieldEchoGeneration = null;
    workspaceDestinationFieldEchoField = null;
  }
}

async function previewSingleManagedExport({
  relativePath = workspaceDestinationSelectedFile,
  targetRoot = workspaceDestinationDraft,
} = {}) {
  if (relativePath !== workspaceDestinationSelectedFile
    || targetRoot !== workspaceDestinationDraft
    || !workspaceDestinationActionEnabled('preview-single')) return false;
  const previewSequence = clearExportPreview();
  try {
    if (!(await confirmPullBackPreview())) return;
    assertCurrentExportPreview(previewSequence);
    if (workspaceDestinationSelectedFile !== relativePath || workspaceDestinationDraft !== targetRoot) {
      throw new ExportPreviewSuperseded();
    }
    const binding = captureVerifiedWorkspace();
    const preview = await previewManagedExport(relativePath, targetRoot, binding);
    assertCurrentExportPreview(previewSequence);
    assertVerifiedWorkspace(binding);
    if (workspaceDestinationSelectedFile !== relativePath || workspaceDestinationDraft !== targetRoot) {
      throw new Error('The update selection changed while Mesh was reading it. Preview the current selection again.');
    }
    model.exportPreview = preview;
    renderSingleExportPlan(preview);
    renderExportChoices();
    showNotice(preview.identical
      ? 'The saved Mesh version and the ordinary-folder file are already identical. Nothing will be changed.'
      : !preview.replace_allowed
        ? 'Mesh kept the ordinary-folder file unchanged because it may contain work from another agent or an external editor. Compare both sides and resolve it manually.'
      : preview.target_exists
        ? 'Update preview verified. Review both sides, then confirm the atomic replacement.'
        : 'Update preview verified the file is absent and its parent is unchanged. Confirm to create it atomically.');
  } catch (error) {
    if (error instanceof ExportPreviewSuperseded) return;
    clearExportPreview();
    showNotice(String(error), true);
  }
  return true;
}

async function confirmSingleManagedExport(
  expectedPreview = model.exportPreview,
  expectedPreviewSequence = exportPreviewSequence,
) {
  let attemptedExport = null;
  let exportDispatched = false;
  try {
    const preview = model.exportPreview;
    if (preview !== expectedPreview || exportPreviewSequence !== expectedPreviewSequence) {
      throw new ExportPreviewSuperseded();
    }
    if (!preview || preview.identical || !preview.replace_allowed) throw new Error('Preview a destination that Mesh can safely create or replace before pulling it back.');
    if (originalProjectUpdateBlocked(preview.target_root)) {
      throw new Error('Record a review and approve the current saved version before updating the original project folder.');
    }
    if (
      workspaceDestinationSelectedFile !== preview.path ||
      workspaceDestinationDraft !== preview.target_root
    ) {
      throw new Error('The update selection changed after preview. Preview it again.');
    }
    const binding = managedMutationBinding();
    const previewSequence = expectedPreviewSequence;
    const isCurrent = () => singleExportStillCurrent(preview, previewSequence, binding);
    const action = preview.target_exists ? 'Replace saved' : 'Create saved';
    const staleMessage = 'The saved-file destination plan changed while confirmation was open. Mesh did not update anything. Preview the current selection again.';
    if (!(await requestAccessibleConfirmation({
      title: `${action} ${preview.path}?`,
      description: `${action} ${preview.path} in ${preview.target_root} from saved Mesh version ${preview.source_version}? Mesh history and the managed working file will not change.`,
      confirmLabel: preview.target_exists ? 'Replace saved file' : 'Create saved file',
      cancelLabel: 'Keep destination unchanged',
      tone: preview.target_exists ? 'destructive' : 'standard',
      isCurrent,
      staleMessage,
    }))) return;
    if (!isCurrent()) throw new Error(staleMessage);
    attemptedExport = {
      preview,
      binding: {
        root: binding.expectedWorkspaceRoot,
        digest: binding.expectedWorkspaceDigest,
        installation: binding.expectedWorkspaceInstallation,
      },
    };
    await coordinateWorkspaceMutation(async (sequence) => {
      exportDispatched = true;
      const result = JSON.parse(await invoke('export_managed_file', {
        ...exportCommandParameters(preview, binding),
      }));
      await installVerifiedWorkspace(() => call('workspace.state'), sequence);
      const rememberWarning = await rememberWorkspace(model.workspace.root, result.target_root);
      assertCurrentWorkspaceVerification(sequence);
      clearExportPreview();
      renderWorkspace();
      showNotice(
        `${result.created ? 'Created' : 'Replaced'} saved ${result.path} atomically in ${result.target_root}. The saved Mesh version and native working folder are unchanged.${rememberWarning ? ` ${rememberWarning}` : ''}`,
        Boolean(rememberWarning),
      );
    });
  } catch (error) {
    if (error instanceof ExportPreviewSuperseded) return false;
    if (exportDispatched && attemptedExport) {
      await recoverAmbiguousManagedFileExport(attemptedExport, error);
    } else {
      showNotice(String(error), true);
    }
  }
  return true;
}

async function previewAllManagedExports({ verifyNativeWork = false, expectedTargetRoot = null } = {}) {
  if (expectedTargetRoot !== null
    && (expectedTargetRoot !== workspaceDestinationDraft
      || !workspaceDestinationActionEnabled('preview-all'))) return false;
  clearExportPreview();
  try {
    const targetRoot = workspaceDestinationDraft;
    if (verifyNativeWork && !(await confirmPullBackPreview())) return false;
    // Native-folder verification may install a fresher exact workspace snapshot. That install
    // intentionally revokes every older export preview, including the generation created before
    // this verification began. Start the actionable preview generation only after the read-only
    // native inspection has completed; later destination or workspace changes still revoke it.
    const previewSequence = clearExportPreview();
    assertCurrentExportPreview(previewSequence);
    if (workspaceDestinationDraft !== targetRoot) throw new ExportPreviewSuperseded();
    const binding = captureVerifiedWorkspace();
    const directoryPaths = (model.workspace?.entries || [])
      .filter((entry) => entry.type === 'folder')
      .map((entry) => entry.path)
      .sort((left, right) => {
        const depth = left.split('/').length - right.split('/').length;
        return depth || (left < right ? -1 : left > right ? 1 : 0);
      });
    const directoryPreview = directoryPaths.length
      ? await previewManagedDirectoryExports(directoryPaths, targetRoot, binding)
      : { missing_paths: [] };
    assertCurrentExportPreview(previewSequence);
    assertVerifiedWorkspace(binding);
    if (workspaceDestinationDraft !== targetRoot) {
      throw new Error('The ordinary destination folder changed while Mesh was previewing the saved tree. Preview it again.');
    }
    const missingDirectories = directoryPreview.missing_paths;
    if (missingDirectories.length) {
      model.exportBatchPreview = {
        kind: 'directories',
        binding: { root: binding.root, digest: binding.digest, installation: binding.installation },
        targetRoot,
        targetInstallation: directoryPreview.target_installation,
        paths: missingDirectories,
      };
      renderDirectoryExportPlan(targetRoot, missingDirectories);
      renderExportChoices();
      showNotice(`Tree preview found ${missingDirectories.length} saved ${missingDirectories.length === 1 ? 'folder' : 'folders'} missing from ${targetRoot}. Review the create-only folder list, then confirm once; saved files are previewed separately afterward.`);
      return;
    }
    const paths = (model.workspace?.file_histories || [])
      .map((history) => history.path)
      .sort((left, right) => left < right ? -1 : left > right ? 1 : 0);
    if (!paths.length) {
      await previewRetiredExports(targetRoot, binding, previewSequence);
      return;
    }
    const previews = [];
    for (const preview of await previewManagedExports(paths, targetRoot, binding)) {
      assertCurrentExportPreview(previewSequence);
      assertVerifiedWorkspace(binding);
      if (workspaceDestinationDraft !== targetRoot) {
        throw new Error('The ordinary destination folder changed while Mesh was previewing the batch. Preview it again.');
      }
      // The single-file preview may include bounded text for side-by-side review. A batch needs
      // only immutable identities and digests, so do not retain every project's file contents in
      // the webview while the person reviews the compact changed-file list.
      const { source_text: _sourceText, target_text: _targetText, ...boundedPreview } = preview;
      previews.push(boundedPreview);
    }
    const changed = previews.filter((preview) => !preview.identical && preview.replace_allowed);
    const preserved = previews.filter((preview) => !preview.identical && !preview.replace_allowed);
    if (!changed.length && !preserved.length) {
      await previewRetiredExports(targetRoot, binding, previewSequence);
      return;
    }
    if (!changed.length) {
      model.exportBatchPreview = null;
      renderCurrentFileExportPlan(targetRoot, [], previews.length - preserved.length, preserved);
      renderExportChoices();
      showNotice(`Mesh kept ${preserved.length} destination ${preserved.length === 1 ? 'file' : 'files'} unchanged because they may contain work from another agent or an external editor. Resolve them manually before updating this workspace.`, true);
      return;
    }
    model.exportBatchPreview = {
      kind: 'files',
      binding: { root: binding.root, digest: binding.digest, installation: binding.installation },
      targetRoot,
      previews: changed,
      paths: changed.map((preview) => preview.path),
      identicalCount: previews.length - changed.length - preserved.length,
      totalCount: previews.length,
      preserved,
    };
    renderCurrentFileExportPlan(targetRoot, changed, previews.length - changed.length - preserved.length, preserved);
    renderExportChoices();
    showNotice(changed.length
      ? `Batch preview verified ${previews.length} saved files: ${changed.length} safe to update, ${previews.length - changed.length - preserved.length} already identical, and ${preserved.length} kept for manual review. Review the list, then confirm once.`
      : `All ${previews.length} saved files are already identical in ${targetRoot}. Nothing needs to be updated.`);
  } catch (error) {
    if (error instanceof ExportPreviewSuperseded) return false;
    clearExportPreview();
    renderExportChoices();
    showNotice(String(error), true);
  }
}

async function confirmBatchManagedExport(
  expectedBatch = model.exportBatchPreview,
  expectedPreviewSequence = exportPreviewSequence,
) {
  try {
    const batch = model.exportBatchPreview;
    const previewSequence = expectedPreviewSequence;
    if (batch !== expectedBatch || exportPreviewSequence !== expectedPreviewSequence) {
      throw new ExportPreviewSuperseded();
    }
    if (!batch?.paths?.length) throw new Error('Preview the saved workspace before updating the destination.');
    if (originalProjectUpdateBlocked(batch.targetRoot)) {
      throw new Error('Record a review and approve the current saved version before updating the original project folder.');
    }
    if (workspaceDestinationDraft !== batch.targetRoot) {
      throw new Error('The ordinary destination folder changed after preview. Preview the batch again.');
    }
    const binding = managedMutationBinding();
    if (
      binding.expectedWorkspaceRoot !== batch.binding.root ||
      binding.expectedWorkspaceDigest !== batch.binding.digest ||
      binding.expectedWorkspaceInstallation !== batch.binding.installation
    ) {
      throw new Error('The managed workspace changed after the batch preview. Preview it again.');
    }
    const confirmation = batch.kind === 'directories'
      ? `Create ${batch.paths.length} saved ${batch.paths.length === 1 ? 'folder' : 'folders'} in ${batch.targetRoot}? Mesh creates only absent folders in depth order, rechecking every exact parent, then previews files separately.`
      : batch.kind === 'retired-files'
        ? `Remove ${batch.paths.length} unchanged old ${batch.paths.length === 1 ? 'file' : 'files'} from ${batch.targetRoot}? Every file is moved aside, reverified against its last saved bytes and metadata, then removed. Changed and unrelated files are preserved.`
        : batch.kind === 'retired-directories'
          ? `Remove ${batch.paths.length} old ${batch.paths.length === 1 ? 'folder' : 'folders'} from ${batch.targetRoot}? Mesh attempts exact identities deepest first and can remove only empty folders. Recursive deletion is unavailable.`
          : `Update ${batch.previews.length} proven ${batch.previews.length === 1 ? 'file' : 'files'} in ${batch.targetRoot}? Mesh keeps ${batch.preserved?.length || 0} unproven ${(batch.preserved?.length || 0) === 1 ? 'file' : 'files'} unchanged. Each selected file is installed atomically; if a later file changes, Mesh stops and keeps the already-completed prefix.`;
    const confirmationTitle = batch.kind === 'directories'
      ? `Create ${plural(batch.paths.length, 'saved folder')}?`
      : batch.kind === 'retired-files'
        ? `Remove ${plural(batch.paths.length, 'unchanged old file')}?`
        : batch.kind === 'retired-directories'
          ? `Remove ${plural(batch.paths.length, 'old folder')}?`
          : `Update ${plural(batch.previews.length, 'proven file')}?`;
    const isCurrent = () => exportBatchStillCurrent(batch, previewSequence);
    if (!(await confirmWholeWorkspaceExport({
      title: confirmationTitle,
      description: confirmation,
      confirmLabel: workspaceDestinationBatchConfirmLabel(batch),
      targetRoot: batch.targetRoot,
      tone: batch.kind === 'directories' ? 'standard' : 'destructive',
      isCurrent,
    }))) return;
    if (!isCurrent()) {
      throw new Error('The reviewed destination plan changed while confirmation was open. Preview the saved workspace again.');
    }
    let nextPhase = null;
    let completedPhaseNotice = null;
    await coordinateWorkspaceMutation(async (sequence) => {
      const completed = [];
      let failure = null;
      if (batch.kind === 'directories') {
        try {
          const outcome = JSON.parse(await invoke('export_managed_directories', {
            paths: batch.paths,
            targetRoot: batch.targetRoot,
            expectedTargetInstallation: batch.targetInstallation,
            ...binding,
          }));
          if (!Array.isArray(outcome?.completed) || outcome.completed.length > batch.paths.length) {
            throw new Error('Mesh returned an invalid saved-folder result. Inspect the destination before retrying.');
          }
          for (let index = 0; index < outcome.completed.length; index += 1) {
            const result = outcome.completed[index];
            if (result?.path !== batch.paths[index] || result.target_root !== batch.targetRoot) {
              throw new Error('Mesh returned the wrong saved-folder completion order. Inspect the destination before retrying.');
            }
            completed.push(result);
          }
          if (outcome.failure) {
            const expectedFailurePath = batch.paths[completed.length];
            if (outcome.failure.path !== expectedFailurePath || typeof outcome.failure.error !== 'string') {
              throw new Error('Mesh returned an invalid saved-folder refusal. Inspect the destination before retrying.');
            }
            failure = { path: outcome.failure.path, error: new Error(outcome.failure.error) };
          } else if (completed.length !== batch.paths.length) {
            throw new Error('Mesh returned an incomplete saved-folder result. Inspect the destination before retrying.');
          }
        } catch (error) {
          failure ||= { path: batch.paths[completed.length] || batch.paths[0], error };
        }
      } else {
        for (const path of batch.paths) {
          try {
            if (batch.kind === 'files') {
              const preview = batch.previews.find((candidate) => candidate.path === path);
              if (!preview) throw new Error('The reviewed file update plan is incomplete. Preview it again.');
              completed.push(JSON.parse(await invoke('export_managed_file', {
                ...exportCommandParameters(preview, binding),
              })));
            } else {
              const preview = batch.previews.find((candidate) => candidate.path === path);
              if (!preview) throw new Error('The reviewed stale-path plan is incomplete. Preview it again.');
              completed.push(JSON.parse(await invoke('remove_retired_export', {
                ...retiredExportCommandParameters(preview, binding),
              })));
            }
          } catch (error) {
            failure = { path, error };
            break;
          }
        }
      }
      await installVerifiedWorkspace(() => call('workspace.state'), sequence);
      const rememberWarning = completed.length
        ? await rememberWorkspace(model.workspace.root, batch.targetRoot)
        : null;
      assertCurrentWorkspaceVerification(sequence);
      clearExportPreview();
      renderWorkspace();
      if (failure) {
        const noun = batch.kind === 'directories' || batch.kind === 'retired-directories' ? 'folder' : 'file';
        const action = batch.kind.startsWith('retired-')
          ? 'removing'
          : batch.kind === 'directories'
            ? 'creating'
            : 'updating';
        throw new Error(`Mesh stopped at ${failure.path} after ${action} ${completed.length} ${completed.length === 1 ? noun : `${noun}s`}: ${String(failure.error)} The completed prefix remains in place. Inspect that destination, then preview the remaining current tree again.${rememberWarning ? ` ${rememberWarning}` : ''}`);
      }
      if (batch.kind === 'directories') {
        nextPhase = 'files';
        completedPhaseNotice = `Created ${completed.filter((result) => result.created).length} saved folders in ${batch.targetRoot}.`;
        showNotice(`${completedPhaseNotice} Mesh is now re-previewing the files against those exact parents.${rememberWarning ? ` ${rememberWarning}` : ''}`, Boolean(rememberWarning));
        return;
      }
      if (batch.kind === 'retired-files') {
        nextPhase = 'retired-directories';
        completedPhaseNotice = `Removed ${completed.length} unchanged old ${completed.length === 1 ? 'file' : 'files'}.`;
        showNotice(`${completedPhaseNotice} Mesh is now checking whether former folders are empty and removable.${rememberWarning ? ` ${rememberWarning}` : ''}`, Boolean(rememberWarning));
        return;
      }
      if (batch.kind === 'retired-directories') {
        nextPhase = 'complete';
        completedPhaseNotice = `Removed ${completed.length} old empty ${completed.length === 1 ? 'folder' : 'folders'}.`;
        showNotice(`${completedPhaseNotice} No recursive deletion was used.${rememberWarning ? ` ${rememberWarning}` : ''}`, Boolean(rememberWarning));
        return;
      }
      completedPhaseNotice = `Updated ${completed.length} proven ${completed.length === 1 ? 'file' : 'files'} in ${batch.targetRoot}; ${batch.identicalCount} ${batch.identicalCount === 1 ? 'file was' : 'files were'} already identical and ${batch.preserved?.length || 0} ${(batch.preserved?.length || 0) === 1 ? 'file was' : 'files were'} kept for manual review.`;
      showNotice(
        `${completedPhaseNotice} Each completed file was rechecked and installed atomically. Mesh history and the native working folder are unchanged.${rememberWarning ? ` ${rememberWarning}` : ''}`,
        Boolean(rememberWarning),
      );
      nextPhase = 'retired-files';
    });
    try {
      if (nextPhase === 'files') await previewAllManagedExports();
      if (nextPhase === 'retired-files') {
        const previewSequence = clearExportPreview();
        await previewRetiredExports(
          batch.targetRoot,
          {
            ...captureVerifiedWorkspace(),
            retiredPreserved: (batch.preserved || []).map((entry) => ({
              path: entry.path,
              reason: 'external-preserved',
            })),
          },
          previewSequence,
        );
      }
      if (nextPhase === 'retired-directories' || nextPhase === 'complete') {
        const previewSequence = clearExportPreview();
        await previewRetiredExports(batch.targetRoot, {
          ...captureVerifiedWorkspace(),
          retiredFilesDone: true,
          retiredPreserved: batch.preserved,
        }, previewSequence);
      }
    } catch (error) {
      showNotice(
        `${completedPhaseNotice || 'The confirmed destination update completed.'} Mesh could not prepare the next cleanup review: ${error} The completed destination changes remain in place; inspect that folder before previewing again.`,
        true,
      );
    }
  } catch (error) {
    if (error instanceof ExportPreviewSuperseded) return;
    showNotice(String(error), true);
  }
}

async function importManagedWorkspaceWithRecovery(parameters) {
  try {
    return {
      answer: JSON.parse(await invoke('import_managed_workspace', parameters)),
      recoveredReply: false,
    };
  } catch (firstError) {
    // Import is an exact, receipt-bound native transaction. The daemon can finish copying,
    // opening, and publishing navigation before the webview receives its reply. Replay this one
    // already-authorized tuple once: the native side reopens only the exact completed import and
    // otherwise performs the original requested import. A second ambiguity is not retried.
    try {
      return {
        answer: JSON.parse(await invoke('import_managed_workspace', parameters)),
        recoveredReply: true,
      };
    } catch (recoveryError) {
      throw new Error(`Mesh could not confirm whether the workspace was created: ${firstError} The one-time recovery also failed: ${recoveryError} Choose Refresh before trying the import again.`);
    }
  }
}

async function confirmImport(authority = null) {
  try {
    if (authority && !importPreviewAuthorityIsCurrent(authority)) {
      showNotice('The import preview changed before Mesh could create its workspace. Review the current source and private location, then try again.', true);
      return false;
    }
    if (workspaceInteractionInFlight() || importDestinationChooserInFlight !== null) {
      showNotice('Another local workspace choice is still finishing. Wait for it before creating this workspace.', true);
      return false;
    }
    const selection = workspaceSelectionSequence;
    const source = model.source;
    const destination = model.destination;
    const summary = model.preview?.summary;
    const connectingExistingImport = currentWorkspaceNavigationMissing()
      && model.checkpoint?.confirmed_import_receipt === true;
    if (!source || !summary) {
      renderPreview();
      showNotice(
        connectingExistingImport
          ? 'Preview the original folder before connecting it to this workspace.'
          : 'Preview the source folder before creating the workspace.',
        true,
      );
      return false;
    }
    // This preview already binds the daemon-held zero-history source, its exact visible content,
    // and the protected namespace. The native confirmation rechecks all three immediately before
    // copying. Running the generic departure scan here would mislabel the very directories being
    // imported as unsaved work that will be left behind.
    if (isProtectedSameFolderImport(source, summary)) {
      if (refuseEditorDraftDeparture()) return false;
    } else if (!(await confirmWorkspaceSwitch())) return false;
    if (
      selection !== workspaceSelectionSequence
      || model.source !== source
      || model.destination !== destination
      || model.preview?.summary !== summary
    ) {
      showNotice(
        connectingExistingImport
          ? 'The original-folder choice changed while Mesh was checking the current workspace. Review the visible source, then connect it again.'
          : 'The import choice changed while Mesh was checking the current folder. Review the visible source and destination, then create it again.',
        true,
      );
      return false;
    }
    let completed = false;
    importConfirmationInFlight = true;
    try {
      renderPreview();
      showNotice(
        `Creating a private workspace from ${model.preview.files} verified files and ${model.preview.directories} folders. The original stays unchanged. Keep Mesh open; large projects can take several minutes.`,
      );
      await coordinateWorkspaceMutation(async (sequence) => {
      const { answer, recoveredReply } = await importManagedWorkspaceWithRecovery({
        source,
        summary,
        destination,
      });
      await installVerifiedWorkspace(async () => answer.workspace, sequence, true);
      if (!answer.navigation || typeof answer.navigation !== 'object') {
        throw new Error('The native import did not return its committed navigation state. Refresh before using either folder.');
      }
      installNavigationStatus(answer.navigation, model.workspace.root);
      const rememberWarning = answer.navigation.warning || null;
      assertCurrentWorkspaceVerification(sequence);
      // The accepted preview is a one-shot authorization. Leaving it actionable beside the new
      // workspace lets an accidental second click create another private copy from the original
      // after this workspace diverges. Clear it only after the native host has committed both the
      // workspace and its navigation; a failed import retains the exact preview for correction.
      clearCommittedImportSelection();
      renderWorkspace();
      let revealWarning = '';
      if (model.agentFolder?.path) {
        try {
          await revealCurrentWorkspaceFolder();
        } catch (error) {
          revealWarning = `The workspace is ready, but its working folder did not open automatically: ${error} Use Open working folder to try again.`;
        }
      }
      const warning = [rememberWarning, revealWarning].filter(Boolean).join(' ');
      const git = gitContextMessage(answer);
      const importOutcome = recoveredReply
        ? connectingExistingImport
          ? `Mesh lost the first connection reply, then recovered this exact workspace-to-original-folder relationship with ${answer.materialized_entries} verified entries; no duplicate was created.`
          : `Mesh lost the first import reply, then recovered the exact completed workspace with ${answer.materialized_entries} materialized entries; no duplicate was created.`
        : answer.recovered_after_interruption
          ? connectingExistingImport
            ? `Connected this existing workspace to its original folder with ${answer.materialized_entries} verified entries; no duplicate was created.`
            : `Mesh recovered the completed workspace with ${answer.materialized_entries} materialized entries instead of creating a duplicate.`
          : `Workspace created with ${answer.materialized_entries} materialized entries.`;
      showNotice(
        `${importOutcome} Its native working folder is ready and the original is unchanged.${git}${warning ? ` ${warning}` : ' Work there normally; Mesh will reopen this workspace automatically.'}`,
        Boolean(warning),
      );
      });
      completed = true;
    } finally {
      importConfirmationInFlight = false;
      if (!completed && model.preview) renderPreview();
    }
    return true;
  } catch (error) {
    showNotice(String(error), true);
    return false;
  }
}

async function openManagedWorkspace(
  path,
  selection,
  {
    choiceStillCurrent = null,
    choiceChangedMessage = 'The workspace choice changed while Mesh was checking the current folder. Review the visible choice and open it again.',
    openSource = 'manual',
  } = {},
) {
  if (selection !== workspaceSelectionSequence) return false;
  const root = String(path ?? '');
  if (!root) {
    renderPreview();
    showNotice('Enter an absolute managed workspace path to open.', true);
    return false;
  }
  if (!(await confirmWorkspaceSwitch())) return false;
  if (selection !== workspaceSelectionSequence) return false;
  if (choiceStillCurrent && !choiceStillCurrent()) {
    showNotice(choiceChangedMessage, true);
    return false;
  }
  await coordinateWorkspaceMutation(async (sequence) => {
    let workspace;
    try {
      workspace = await call('workspace.open', { path: root });
    } catch (error) {
      await recoverAmbiguousManagedWorkspaceOpen(sequence, error, openSource);
    }
    await installVerifiedWorkspace(async () => workspace, sequence, true);
    const rememberWarning = await rememberWorkspace(model.workspace.root);
    assertCurrentWorkspaceVerification(sequence);
    let revealWarning = '';
    if (model.activeFolder?.path) {
      try {
        await revealCurrentWorkspaceFolder();
      } catch (error) {
        revealWarning = ` Mesh switched successfully, but the native folder did not reopen: ${error} Use Open working folder to try again.`;
      }
    }
    renderWorkspace();
    const warning = [rememberWarning, revealWarning].filter(Boolean).join(' ');
    showNotice(
      `Managed workspace reopened from durable local history and remembered for the next launch.${model.activeFolder?.path ? ` Its stable working folder was opened for Finder and newly opened editors. ${NATIVE_SWITCH_HANDLE_NOTICE} Use Start Codex on this version or Open agent terminal for a pinned agent folder.` : ' Create a native working folder before handing this legacy workspace to an editor or agent.'}${warning ? ` ${warning}` : ''}`,
      Boolean(warning),
    );
    // A superseded scan can still have bounded native reads returning after the workspace switch.
    // Let those workers settle before claiming the single scan slot for the selected agent folder;
    // otherwise this explicit return would silently skip its inspection and wait for a later hint.
    while (folderScanInFlight) await folderScanSettled;
    // An unassigned recent workspace may have changed while another workspace was selected, so
    // inspect it at this explicit return boundary. An assigned workspace deliberately stops here:
    // Finish agent handoff owns its exact-generation aggregate inspection.
    await scanNativeFolder({ automatic: true, startup: true, workspaceReturn: true });
  });
  return true;
}

async function recoverAmbiguousManagedWorkspaceOpen(sequence, cause, openSource) {
  // workspace.open is a local process boundary. The daemon can durably select the requested
  // workspace even when the webview never receives its reply, so the snapshot that preceded the
  // attempt is no longer safe to present as current. Keep the shared mutation guard held while
  // reading native truth and repairing recency/navigation; otherwise the recovered snapshot can
  // briefly enable another workspace action before the restart route is durable.
  const deterministicRefusal = cause instanceof DaemonRefusal
    && MANAGED_WORKSPACE_REFUSAL_CODES.has(cause.code);
  let recovered;
  try {
    if (deterministicRefusal) {
      const verified = await readVerifiedWorkspace(() => call('workspace.state'), sequence);
      installWorkspaceSnapshot(verified.workspace, verified.checkpoint);
      recovered = verified.workspace;
    } else {
      recovered = await installVerifiedWorkspace(
        () => call('workspace.state'),
        sequence,
        true,
      );
    }
  } catch (recoveryError) {
    if (
      deterministicRefusal
      && recoveryError instanceof DaemonRefusal
      && recoveryError.code === 'no-workspace-open'
    ) {
      model.workspace = null;
      model.checkpoint = null;
      clearWorkspaceScopedState();
      model.workspaceVerified = true;
      setLocalService('ready');
      renderWorkspace();
      throw new ManagedWorkspaceOpenFailure(
        cause.code,
        managedWorkspaceOpenMessage(cause.code, openSource, false),
      );
    }
    throw new Error(
      `Mesh could not confirm the requested open: ${errorMessage(cause)} Mesh also could not verify which workspace is now open. Managed actions remain paused; choose Refresh before using either folder.`,
      { cause: recoveryError },
    );
  }

  // A typed workspace.open refusal is guaranteed not to select or initialize anything. Reverify
  // the current daemon workspace, but do not rewrite its already-correct native navigation as if
  // an ambiguous open might have committed.
  const navigationWarning = deterministicRefusal ? '' : await rememberWorkspace(recovered.root);
  assertCurrentWorkspaceVerification(sequence);
  if (
    !model.workspaceVerified
    || model.workspace?.root !== recovered.root
    || model.workspace?.digest !== recovered.digest
    || model.workspace?.installation !== recovered.installation
  ) {
    markWorkspaceUnverified();
    throw new Error(
      `Mesh could not confirm the requested open: ${errorMessage(cause)} Mesh found the local service's current workspace, but could not keep its native navigation verified. Managed actions remain paused; choose Refresh before using either folder.`,
      { cause },
    );
  }
  try {
    const afterNavigation = await readVerifiedWorkspace(() => call('workspace.state'), sequence);
    if (
      afterNavigation.workspace.root !== recovered.root
      || afterNavigation.workspace.digest !== recovered.digest
      || afterNavigation.workspace.installation !== recovered.installation
    ) {
      throw new WorkspaceVerificationSuperseded();
    }
    installWorkspaceSnapshot(afterNavigation.workspace, afterNavigation.checkpoint);
  } catch (recoveryError) {
    await reconcileNavigationFromDaemon();
    markWorkspaceUnverified();
    throw new Error(
      `Mesh could not confirm the requested open: ${errorMessage(cause)} The workspace changed again while Mesh was repairing its navigation. Managed actions remain paused; choose Refresh before using either folder.`,
      { cause: recoveryError },
    );
  }
  renderWorkspace();
  if (deterministicRefusal) {
    throw new ManagedWorkspaceOpenFailure(
      cause.code,
      managedWorkspaceOpenMessage(cause.code, openSource, true),
    );
  }
  throw new Error(
    `Mesh could not confirm the requested open: ${errorMessage(cause)} Mesh recovered and verified the workspace the local service actually has open. Review it before continuing.${navigationWarning ? ` ${navigationWarning}` : ''}`,
    { cause },
  );
}

async function chooseManagedWorkspaceFromEntry(expectedContinuityKey) {
  if (
    workspaceInteractionInFlight()
    || workspaceEntryRefreshInFlight
    || expectedContinuityKey !== workspaceProjectionContinuityKey()
  ) return false;
  workspaceEntrySelectionInFlight += 1;
  renderPreview(importWorkbenchNextMounted?.generation ?? null);
  const selection = ++workspaceSelectionSequence;
  let mutationStarted = false;
  // A prior import stays visible while the native picker is open, but cannot be confirmed against
  // a workspace choice the person is actively replacing. Cancellation restores that exact preview.
  try {
    const path = await invoke('pick_folder');
    if (selection !== workspaceSelectionSequence) return false;
    if (!path) {
      renderPreview();
      return false;
    }
    if (expectedContinuityKey !== workspaceProjectionContinuityKey()) {
      renderPreview();
      showNotice('The workspace changed while Mesh was choosing another managed folder. Review the current workspace and choose again.', true);
      return false;
    }
    mutationStarted = true;
    return await openManagedWorkspace(path, selection, {
      choiceStillCurrent: () => expectedContinuityKey === workspaceProjectionContinuityKey(),
      choiceChangedMessage: 'The workspace changed while Mesh was choosing another managed folder. Review the current workspace and choose again.',
    });
  } catch (error) {
    if (mutationStarted || selection === workspaceSelectionSequence) {
      if (!mutationStarted) renderPreview();
      showNotice(errorMessage(error), true);
    }
    return false;
  } finally {
    workspaceEntrySelectionInFlight = Math.max(0, workspaceEntrySelectionInFlight - 1);
    renderPreview(importWorkbenchNextMounted?.generation ?? null);
  }
}

async function openManagedWorkspaceFromEntry(path, expectedContinuityKey) {
  const expectedPath = String(path ?? '');
  if (
    workspaceInteractionInFlight()
    || expectedPath !== workspaceEntryManagedPathDraft
    || expectedContinuityKey !== workspaceProjectionContinuityKey()
  ) return false;
  workspaceEntrySelectionInFlight += 1;
  const selection = ++workspaceSelectionSequence;
  let mutationStarted = false;
  renderPreview(importWorkbenchNextMounted?.generation ?? null);
  try {
    mutationStarted = Boolean(expectedPath);
    return await openManagedWorkspace(expectedPath, selection, {
      choiceStillCurrent: () => (
        workspaceEntryManagedPathDraft === expectedPath
        && expectedContinuityKey === workspaceProjectionContinuityKey()
      ),
      choiceChangedMessage: 'The workspace path changed while Mesh was checking the current folder. Review the visible path and open it again.',
    });
  } catch (error) {
    if (mutationStarted || selection === workspaceSelectionSequence) {
      renderPreview();
      showNotice(errorMessage(error), true);
    }
    return false;
  } finally {
    workspaceEntrySelectionInFlight = Math.max(0, workspaceEntrySelectionInFlight - 1);
    renderPreview(importWorkbenchNextMounted?.generation ?? null);
  }
}

async function openRecentWorkspacePath(path, choiceStillCurrent, choiceChangedMessage) {
  const selection = ++workspaceSelectionSequence;
  try {
    await openManagedWorkspace(path, selection, {
      choiceStillCurrent,
      choiceChangedMessage,
      openSource: 'recent',
    });
    unavailableRecentWorkspacePaths.delete(path);
    renderWorkspace();
    return true;
  } catch (error) {
    if (error instanceof ManagedWorkspaceOpenFailure) {
      unavailableRecentWorkspacePaths.add(path);
      selectedRecentWorkspacePath = path;
      workspaceEntryDisclosureOpen = true;
      renderWorkspace();
      focusReactWorkspacePage('workspaces', '#workspace-entry-recent');
      showNotice(error.message, true);
      return false;
    }
    const refreshSucceeded = await refresh();
    const currentReverified = refreshSucceeded && Boolean(model.workspace) && model.workspaceVerified;
    showNotice(
      `${errorMessage(error)} ${currentReverified
        ? 'The current workspace remains open; remove or repair the unavailable folder before trying it again.'
        : refreshSucceeded
          ? 'No managed workspace is open; remove or repair the unavailable folder before trying it again.'
          : 'Mesh could not reverify the current workspace, so managed controls remain paused.'}`,
      true,
    );
    return false;
  }
}

async function openSelectedRecentWorkspace(path) {
  const entries = recentWorkspaceEntries();
  const recent = recentWorkspacePresentation(entries);
  const selected = entries.find((entry) => entry.path === path) || null;
  if (
    workspaceInteractionInFlight()
    || recent.selectedPath !== path
    || !recent.canOpen
    || !selected
  ) return false;
  const expectedContinuityKey = workspaceProjectionContinuityKey();
  const assignedInstallation = selected.agentHandoffInstallation;
  const opened = await openRecentWorkspacePath(
    path,
    () => (
      selectedRecentWorkspacePath === path
      && recentWorkspaceEntries().some((entry) => entry.path === path)
      && workspaceProjectionContinuityKey() === expectedContinuityKey
    ),
    'The recent workspace choice changed while Mesh was checking the current folder. Review the visible choice and switch again.',
  );
  if (
    opened
    && assignedInstallation
    && model.workspace?.root === path
    && model.workspace?.installation === assignedInstallation
    && workspaceInstallationMatchesHandoff()
  ) {
    focusReactWorkspacePage('current');
    showNotice('Returned to the exact folder assigned to this agent. After every Codex session, terminal, and editor using it has stopped, choose Finish agent handoff. Mesh will then inspect the folder for unsaved native work.');
  }
  return opened;
}

async function activateCurrentReturnWorkspace() {
  const path = previousWorkspacePath();
  if (!path) return;
  await openRecentWorkspacePath(
    path,
    () => previousWorkspacePath() === path,
    'The previous workspace changed while Mesh was checking the current folder. Review the visible workspace and return again.',
  );
}

async function forgetSelectedRecentWorkspace(path) {
  const recent = recentWorkspacePresentation();
  if (!path || recent.selectedPath !== path) return false;
  if (path === model.workspace?.root) {
    showNotice('The open workspace must stay remembered so Mesh can reopen it after restart. Switch to another workspace first, or use Roll back managed copy when removal is intended.', true);
    return false;
  }
  if (recentWorkspaceEntry(path)?.agentHandoffInstallation) {
    showNotice('This workspace is still assigned to an agent. Return to it and choose Finish agent handoff after every process using the folder has stopped before removing it from Recent workspaces.', true);
    return false;
  }
  if (!recent.canForget || !beginWorkspaceTransition()) return false;
  try {
    const warning = await forgetWorkspace(path);
    if (!recentWorkspaceEntries().some((entry) => entry.path === path)) {
      unavailableRecentWorkspacePaths.delete(path);
    }
    showNotice(
      warning || `Removed ${path} from the recent-workspace list. Its folder and saved history were not changed.`,
      Boolean(warning),
    );
    return !recentWorkspaceEntries().some((entry) => entry.path === path);
  } catch (error) {
    showNotice(`Mesh could not remove that recent-workspace entry: ${error}`, true);
    return false;
  } finally {
    finishWorkspaceTransition();
  }
}

async function activateCurrentRefresh() {
  currentRefreshInFlight += 1;
  installWorkspaceCurrentNextVisibility();
  try {
    const verified = await refresh();
    if (verified && restoreRecoveryScanPending) {
      // Refresh is the documented recovery action after an ambiguous managed mutation. Reconcile
      // the verified workspace with its physical folder without automatically saving anything, so
      // a restore that committed before its reply was lost becomes visible instead of being replayed.
      const inspected = await scanNativeFolder({ automatic: true, startup: true, allowAutomaticSave: false });
      if (inspected) restoreRecoveryScanPending = false;
    }
    await refreshApprovalStatus();
    return verified;
  } finally {
    currentRefreshInFlight = Math.max(0, currentRefreshInFlight - 1);
    // Failure leaves the verified bit false and therefore needs one explicit recovery projection.
    // Success normally takes the no-paint authority-renewal path above.
    if (currentRefreshInFlight === 0 && !model.workspaceVerified) renderWorkspaceCurrentNext();
    else installWorkspaceCurrentNextVisibility();
  }
}

function rollbackWorkspaceStillCurrent(binding) {
  try {
    assertVerifiedWorkspace(binding);
    return !workspaceInteractionInFlight()
      && !workspaceInstallationMatchesHandoff()
      && !managedWorkspaceWriteBlocked()
      && !editorDraftPending()
      && model.folderChanges.length === 0
      && currentWorkspaceActionPresentation().some((action) => (
        action.id === 'rollback' && action.enabled
      ));
  } catch {
    return false;
  }
}

async function recoverAmbiguousWorkspaceRollback(binding, error) {
  const failure = String(error);
  const refreshed = await refresh();
  if (!refreshed) {
    return `${failure} Mesh could not confirm the rollback reply or reverify the current workspace. Managed workspace actions remain paused. Choose Refresh before continuing; do not retry Roll back managed copy while its outcome is unknown.`;
  }
  if (!model.workspace && model.workspaceVerified) {
    return `${failure} Mesh could not confirm the rollback reply, but verified that no managed workspace is open. The managed copy may have been removed; the original folder was not changed. If its unavailable shortcut remains, remove it from Recent workspaces after checking the folder.`;
  }
  if (
    model.workspaceVerified
    && model.workspace?.root === binding.root
    && model.workspace?.digest === binding.digest
    && model.workspace?.installation === binding.installation
  ) {
    return `${failure} Mesh could not confirm the rollback reply, but reverified that the same managed workspace remains open. Nothing was replayed. Review it before trying rollback again.`;
  }
  return `${failure} Mesh could not confirm the rollback reply, and Refresh found a different managed workspace. Nothing was replayed. Review the visible workspace before continuing.`;
}

async function activateCurrentRollback() {
  if (!model.workspace) return;
  if (workspaceInstallationMatchesHandoff()) {
    showNotice('This workspace is still assigned to an agent. Choose Finish agent handoff after every process using the folder has stopped before rolling back the managed copy.', true);
    return;
  }
  let rollbackBinding = null;
  let rollbackDispatched = false;
  try {
    if (!(await confirmWorkspaceSwitch({ allowNativeChanges: false, allowUnverified: false }))) return;
    rollbackBinding = captureVerifiedWorkspace();
    const isCurrent = () => rollbackWorkspaceStillCurrent(rollbackBinding);
    if (!(await requestAccessibleConfirmation({
      title: 'Remove this managed workspace?',
      description: `Roll back the unchanged managed copy at ${rollbackBinding.root}? The original folder remains untouched, but this private working copy and its local Mesh history will be removed.`,
      confirmLabel: 'Roll back managed copy',
      cancelLabel: 'Keep workspace',
      tone: 'destructive',
      isCurrent,
      staleMessage: 'The workspace changed while rollback confirmation was open. Mesh did not remove anything. Refresh and review it again.',
    }))) return;
    if (!isCurrent()) {
      throw new Error('The workspace changed while rollback confirmation was open. Mesh did not remove anything. Refresh and review it again.');
    }
    await coordinateWorkspaceMutation(async (sequence) => {
      const destination = rollbackBinding.root;
      rollbackDispatched = true;
      const rolledBack = JSON.parse(await invoke('rollback_managed_workspace', {
        expectedWorkspaceRoot: rollbackBinding.root,
        expectedWorkspaceDigest: rollbackBinding.digest,
        expectedWorkspaceInstallation: rollbackBinding.installation,
      }));
      const forgetWarning = await forgetWorkspace(rolledBack.workspace_root || destination);
      assertCurrentWorkspaceVerification(sequence);
      model.workspace = null;
      model.workspaceVerified = true;
      model.checkpoint = null;
      model.editor = null;
      model.restorePreview = null;
      model.restoreUndo = null;
      setLocalService('ready');
      renderWorkspace();
      showNotice(forgetWarning || 'Managed copy rolled back. The original folder was preserved and the recent-workspace pointer was removed.', Boolean(forgetWarning));
    });
  } catch (error) {
    showNotice(
      rollbackDispatched && rollbackBinding
        ? await recoverAmbiguousWorkspaceRollback(rollbackBinding, error)
        : String(error),
      true,
    );
  }
}

async function completeManagedChange(result, message, sequence) {
  if (!result.author_authenticated) {
    throw new Error('The local change was not authenticated and cannot be shown as saved.');
  }
  await installVerifiedWorkspace(() => call('workspace.state'), sequence);
  model.editor = null;
  model.restorePreview = null;
  model.restoreUndo = null;
  workspaceFilesState.newPath = '';
  workspaceFilesState.movePath = '';
  renderWorkspace();
  showNotice(result.saved_privately
    ? `${message} The signed change is durable in private history.`
    : `${message} The change is durable, but newer activity keeps the workspace Working.`);
}

async function recoverAmbiguousManagedEntryChange(attempt, error) {
  const failure = String(error);
  const refreshed = await refresh();
  if (!refreshed) {
    showNotice(`${failure} Mesh could not confirm the ${attempt.label} reply or reverify the exact workspace. Nothing was replayed. Managed actions remain paused until Refresh succeeds.`, true);
    return;
  }
  const sameWorkspace = model.workspaceVerified
    && model.workspace?.root === attempt.root
    && model.workspace?.installation === attempt.installation;
  if (!sameWorkspace) {
    showNotice(`${failure} Mesh could not confirm the ${attempt.label} reply. Nothing was replayed. Refresh found a different workspace; review the workspace now visible before continuing.`, true);
    return;
  }
  const entries = Array.isArray(model.workspace?.entries) ? model.workspace.entries : [];
  const source = attempt.fromPath
    ? entries.find((entry) => entry.path === attempt.fromPath) || null
    : null;
  const target = attempt.toPath
    ? entries.find((entry) => entry.path === attempt.toPath) || null
    : null;
  const requestedShapeVisible = attempt.fromPath && !attempt.toPath
    ? source === null
    : target?.type === attempt.entryType && (!attempt.fromPath || source === null);
  const visibleState = attempt.fromPath && attempt.toPath
    ? requestedShapeVisible
      ? `${attempt.fromPath} absent and ${attempt.toPath} present as a ${attempt.entryType}`
      : `${attempt.fromPath} ${source ? `present as a ${source.type}` : 'absent'} and ${attempt.toPath} ${target ? `present as a ${target.type}` : 'absent'}`
    : attempt.fromPath
      ? `${attempt.fromPath} ${source ? `present as a ${source.type}` : 'absent'}`
      : `${attempt.toPath} ${target ? `present as a ${target.type}` : 'absent'}`;
  showNotice(
    `${failure} Mesh could not confirm the ${attempt.label} reply. Nothing was replayed. Native read-back verified this exact workspace now shows ${visibleState}. ${requestedShapeVisible ? 'That state may be the completed request or another local change; inspect it before continuing.' : 'The requested result was not proven; inspect the visible workspace before trying again.'}`,
    true,
  );
}

async function createManagedTextEntry() {
  const relativePath = workspaceFilesState.newPath;
  let attemptedChange = null;
  let changeDispatched = false;
  try {
    const binding = managedMutationBinding();
    attemptedChange = Object.freeze({
      root: binding.expectedWorkspaceRoot,
      installation: binding.expectedWorkspaceInstallation,
      fromPath: null,
      toPath: relativePath,
      entryType: 'file',
      label: 'create file',
    });
    await coordinateWorkspaceMutation(async (sequence) => {
      changeDispatched = true;
      const result = JSON.parse(await invoke('create_managed_text', {
        relativePath,
        text: '',
        ...binding,
      }));
      await completeManagedChange(result, `Created ${relativePath}.`, sequence);
      workspaceChangesEditorState.selectedFile = relativePath;
      renderEditorChoices();
    });
  } catch (error) {
    if (attemptedChange && changeDispatched) await recoverAmbiguousManagedEntryChange(attemptedChange, error);
    else showNotice(String(error), true);
  }
}

async function createManagedFolderEntry() {
  const relativePath = workspaceFilesState.newPath;
  let attemptedChange = null;
  let changeDispatched = false;
  try {
    const binding = managedMutationBinding();
    attemptedChange = Object.freeze({
      root: binding.expectedWorkspaceRoot,
      installation: binding.expectedWorkspaceInstallation,
      fromPath: null,
      toPath: relativePath,
      entryType: 'folder',
      label: 'create folder',
    });
    await coordinateWorkspaceMutation(async (sequence) => {
      changeDispatched = true;
      const result = JSON.parse(await invoke('create_managed_folder', { relativePath, ...binding }));
      await completeManagedChange(result, `Created folder ${relativePath}.`, sequence);
    });
  } catch (error) {
    if (attemptedChange && changeDispatched) await recoverAmbiguousManagedEntryChange(attemptedChange, error);
    else showNotice(String(error), true);
  }
}

async function moveManagedEntry() {
  const fromPath = workspaceFilesState.selectedEntry;
  const toPath = workspaceFilesState.movePath;
  let attemptedChange = null;
  let changeDispatched = false;
  try {
    const binding = managedMutationBinding();
    const entryType = model.workspace?.entries.find((entry) => entry.path === fromPath)?.type;
    if (!['file', 'folder'].includes(entryType)) throw new WorkspaceVerificationSuperseded();
    attemptedChange = Object.freeze({
      root: binding.expectedWorkspaceRoot,
      installation: binding.expectedWorkspaceInstallation,
      fromPath,
      toPath,
      entryType,
      label: 'move',
    });
    await coordinateWorkspaceMutation(async (sequence) => {
      changeDispatched = true;
      const result = JSON.parse(await invoke('move_managed_entry', { fromPath, toPath, ...binding }));
      await completeManagedChange(result, `Moved ${fromPath} to ${toPath}.`, sequence);
    });
  } catch (error) {
    if (attemptedChange && changeDispatched) await recoverAmbiguousManagedEntryChange(attemptedChange, error);
    else showNotice(String(error), true);
  }
}

function deleteEntryStillCurrent(binding, relativePath, entryType) {
  try {
    assertVerifiedWorkspace(binding);
    const entry = model.workspace?.entries.find((candidate) => candidate.path === relativePath);
    return entry?.type === entryType
      && workspaceFilesState.selectedEntry === relativePath
      && workspaceFileActionEnabled('delete-entry')
      && !workspaceInteractionInFlight()
      && !managedWorkspaceMutationBlocked()
      && !editorDraftPending();
  } catch {
    return false;
  }
}

async function recoverAmbiguousManagedEntryDelete(attempt, error) {
  const failure = String(error);
  const refreshed = await refresh();
  if (!refreshed) {
    showNotice(`${failure} Mesh could not confirm the delete reply or reverify the current workspace. Nothing was replayed. Managed actions remain paused until Refresh succeeds.`, true);
    return;
  }
  const sameWorkspace = model.workspaceVerified
    && model.workspace?.root === attempt.root
    && model.workspace?.installation === attempt.installation;
  const entry = sameWorkspace
    ? model.workspace.entries.find((candidate) => candidate.path === attempt.relativePath)
    : null;
  const advanced = sameWorkspace
    && model.workspace.digest !== attempt.digest
    && Number.isSafeInteger(model.workspace.records)
    && Number.isSafeInteger(attempt.records)
    && model.workspace.records > attempt.records;
  if (advanced && !entry) {
    showNotice(`Mesh lost the delete reply but verified that ${attempt.relativePath} is no longer present in this exact workspace. The deletion was not replayed; any retained file history remains available.`);
    return;
  }
  if (sameWorkspace && entry?.type === attempt.entryType) {
    showNotice(`${failure} Mesh reverified the exact workspace and ${attempt.relativePath} is still present. Nothing was replayed. Review the entry before trying again.`, true);
    return;
  }
  showNotice(`${failure} Mesh could not prove the delete outcome against the exact workspace and entry. Nothing was replayed. Review the workspace now visible before continuing.`, true);
}

async function deleteManagedEntry() {
  const relativePath = workspaceFilesState.selectedEntry;
  if (!relativePath) return;
  if (workspaceInstallationMatchesHandoff()) {
    showNotice(activeAgentMutationRefusal(), true);
    return;
  }
  let attemptedDelete = null;
  let deleteDispatched = false;
  try {
    const verified = captureVerifiedWorkspace();
    const binding = {
      expectedWorkspaceRoot: verified.root,
      expectedWorkspaceDigest: verified.digest,
      expectedWorkspaceInstallation: verified.installation,
    };
    const entry = model.workspace.entries.find((candidate) => candidate.path === relativePath);
    if (!entry) throw new WorkspaceVerificationSuperseded();
    const inspected = entry.type === 'file'
      ? JSON.parse(await invoke('inspect_managed_file', { relativePath, ...binding }))
      : null;
    assertVerifiedWorkspace(verified);
    attemptedDelete = Object.freeze({
      root: verified.root,
      digest: verified.digest,
      installation: verified.installation,
      records: model.workspace.records,
      relativePath,
      entryType: entry.type,
    });
    const isCurrent = () => deleteEntryStillCurrent(verified, relativePath, entry.type);
    const staleMessage = 'The workspace changed while delete confirmation was open. Mesh did not delete anything. Refresh and review the entry again.';
    if (!(await requestAccessibleConfirmation({
      title: `Delete ${relativePath}?`,
      description: `Delete ${relativePath} from ${verified.root}? Saved file content remains in immutable history, but the current working ${entry.type === 'file' ? 'file' : 'folder'} will be removed.`,
      confirmLabel: entry.type === 'file' ? 'Delete file' : 'Delete folder',
      cancelLabel: 'Keep entry',
      tone: 'destructive',
      isCurrent,
      staleMessage,
    }))) return;
    if (!isCurrent()) throw new Error(staleMessage);
    await coordinateWorkspaceMutation(async (sequence) => {
      deleteDispatched = true;
      const result = JSON.parse(await invoke('delete_managed_entry', {
        relativePath,
        expectedContentDigest: inspected?.content_digest ?? null,
        expectedExecutable: inspected?.executable ?? null,
        ...binding,
      }));
      await completeManagedChange(result, `Deleted ${relativePath} from the managed folder.`, sequence);
    });
  } catch (error) {
    if (attemptedDelete && deleteDispatched) {
      await recoverAmbiguousManagedEntryDelete(attemptedDelete, error);
    } else {
      showNotice(String(error), true);
    }
  }
}

async function openWorkspaceVersionAsFolder(
  operation,
  destination,
  {
    reveal = true,
    choiceStillCurrent = null,
    allowNativeChanges = true,
    allowUnverified = true,
    blockedAction = 'opening another saved workspace',
    openInCodex = false,
    freshCopy = false,
    withinNativeWorkspaceLaunch = false,
  } = {},
) {
  const selectedVersion = (model.workspace?.workspace_versions || [])
    .find((version) => version.operation === operation);
  const selectedPoint = selectedVersion && Number.isSafeInteger(selectedVersion.ordinal)
    ? `Saved point ${selectedVersion.ordinal}`
    : 'Selected saved version';
  if (!beginWorkspaceTransition({ withinNativeWorkspaceLaunch })) {
    showNotice('Another workspace or agent folder is already being prepared. Wait for it to finish before opening another one.', true);
    return false;
  }
  try {
    if (!(await confirmWorkspaceSwitch({ allowNativeChanges, allowUnverified, blockedAction }))) return false;
    if (choiceStillCurrent && !choiceStillCurrent()) {
      showNotice('The saved workspace choice changed while Mesh was checking the current folder. Review the visible choice and open it again.', true);
      return false;
    }
    const source = captureVerifiedWorkspace();
    const inheritedExportRoot = model.exportRoot;
    await coordinateWorkspaceMutation(async (sequence) => {
      const command = freshCopy ? 'open_fresh_agent_workspace' : 'open_managed_workspace_version';
      let answer;
      try {
        answer = JSON.parse(await invoke(command, {
          operation,
          ...(freshCopy ? {} : { destination: destination || null }),
          exportRoot: inheritedExportRoot,
          expectedWorkspaceRoot: source.root,
          expectedWorkspaceDigest: source.digest,
          expectedWorkspaceInstallation: source.installation,
        }));
        if (
          !answer
          || typeof answer !== 'object'
          || !selectedVersion
          || answer.source_version !== selectedVersion.operation
          || answer.source_ordinal !== selectedVersion.ordinal
        ) {
          throw new Error('The opened workspace did not match the selected durable point.');
        }
      } catch (cause) {
        await recoverAmbiguousWorkspaceVersionOpen(sequence, inheritedExportRoot, cause);
      }
      const unexpectedlyReused = freshCopy && answer.reused === true;
      await installVerifiedWorkspace(async () => answer.workspace, sequence, true);
      if (!answer.navigation || typeof answer.navigation !== 'object') {
        throw new Error('The native version switch did not return its committed navigation state. Refresh before opening either folder.');
      }
      installNavigationStatus(answer.navigation, model.workspace.root);
      const rememberWarning = answer.navigation.warning || null;
      assertCurrentWorkspaceVerification(sequence);
      workspaceVersionDestinationDraft = '';
      renderWorkspace();
      if (unexpectedlyReused) {
        throw new Error('Mesh reused an existing agent folder instead of creating a fresh isolated copy. The returned workspace is open for inspection, but no new agent was launched.');
      }
      let presentation = `The stable working path now opens this version in Finder or a newly opened editor. ${NATIVE_SWITCH_HANDLE_NOTICE} Long-running agents remain pinned to their real folder.`;
      let revealWarning = '';
      if (openInCodex) {
        if (workspaceInstallationMatchesHandoff()) {
          // Reopening an existing saved-point checkout must preserve the same collision boundary
          // as the ordinary Start Codex on this version action. The native navigation record is installed above,
          // so this exact physical folder may already belong to a still-running agent even though
          // the version choice began from a different workspace.
          presentation = 'The saved version is open in its independent folder and remains assigned to its existing agent.';
          revealWarning = ' This exact folder was already handed to an agent. Mesh did not open a second Codex session there; use Start another agent copy to create a fresh independent copy.';
        } else {
          let attemptedBinding = null;
          try {
            const binding = captureVerifiedWorkspace();
            attemptedBinding = binding;
            const opened = JSON.parse(await invoke('open_managed_workspace_in_codex', {
              expectedWorkspaceRoot: binding.root,
              expectedWorkspaceDigest: binding.digest,
              expectedWorkspaceInstallation: binding.installation,
              confirmedReopen: false,
              expectedAgentHandoffGeneration: null,
            }));
            assertVerifiedWorkspace(binding);
            const handoffGeneration = assertAgentLaunchResponse(opened, binding, 'Codex');
            rememberAgentHandoff(binding, handoffGeneration);
            presentation = `Opened ${opened.path} in Codex as an independent writable folder. Create a new task there and give it your instruction. That agent stays on this version if Mesh switches elsewhere; keep this Mesh assignment active until every related Codex session, terminal, and editor has stopped.${codexContextMessage(opened)}`;
          } catch (error) {
            if (attemptedBinding) rememberAgentHandoff(attemptedBinding);
            revealWarning = attemptedBinding
              ? ` The version is ready, but Mesh could not confirm whether Codex opened it: ${error} The folder is marked as handed off; reopen only if the prior launch did not start.`
              : ` The version is ready, but Codex did not open it: ${error} Use Start Codex on this version to try again.`;
          }
        }
      } else if (reveal) {
        try {
          await revealCurrentWorkspaceFolder();
        } catch (error) {
          revealWarning = ` The version is ready, but its working folder did not open automatically: ${error} Use Open working folder to try again.`;
        }
      }
      const workingDestination = model.activeFolder?.path || answer.destination;
      const openedCopy = answer.reused === true
        ? `Reopened ${selectedPoint} in your working folder at ${workingDestination}.`
        : `Opened ${selectedPoint} in your working folder at ${workingDestination}.`;
      const warnings = [rememberWarning, revealWarning.trim()].filter(Boolean);
      showNotice(
        `${openedCopy} ${presentation}${warnings.length ? ` Attention: ${warnings.join(' ')}` : ''}`,
        warnings.length > 0,
      );
    }, { withinWorkspaceTransition: true, withinNativeWorkspaceLaunch });
    return true;
  } finally {
    finishWorkspaceTransition();
  }
}

function selectedWorkspaceVersionStillCurrent(operation, destination, agentChoice) {
  return model.workspaceVerified
    && selectedWorkspaceVersionOperation === operation
    && (workspaceVersionDestinationDraft || null) === destination
    && pendingAgentVersionChoice === agentChoice
    && workspaceVersionPreviewStateMatches(model.workspaceVersionPreview, operation)
    && (model.workspace?.workspace_versions || [])
      .some((version) => version.operation === operation);
}

async function openSelectedWorkspaceVersion(operation) {
  const destination = workspaceVersionDestinationDraft || null;
  if (!selectedWorkspaceVersionStillCurrent(operation, destination, pendingAgentVersionChoice)) {
    throw new WorkspaceVerificationSuperseded();
  }
  pendingAgentVersionChoice = false;
  try {
    return await openWorkspaceVersionAsFolder(operation, destination, {
      choiceStillCurrent: () => selectedWorkspaceVersionStillCurrent(
        operation,
        destination,
        false,
      ),
    });
  } catch (error) { showNotice(String(error), true); }
  return false;
}

async function startCodexFromSelectedWorkspaceVersion(operation) {
  const destination = workspaceVersionDestinationDraft || null;
  const freshAgent = pendingAgentVersionChoice;
  if (!selectedWorkspaceVersionStillCurrent(operation, destination, freshAgent)) {
    throw new WorkspaceVerificationSuperseded();
  }
  try {
    return await openWorkspaceVersionAsFolder(operation, destination, {
      reveal: false,
      openInCodex: true,
      freshCopy: freshAgent,
      allowNativeChanges: !freshAgent,
      allowUnverified: !freshAgent,
      blockedAction: freshAgent ? 'starting another isolated agent' : 'opening another saved workspace',
      choiceStillCurrent: () => selectedWorkspaceVersionStillCurrent(
        operation,
        destination,
        freshAgent,
      ),
    });
  } catch (error) { showNotice(String(error), true); }
  return false;
}

async function previewSelectedRestore() {
  revokeRestorePreview();
  renderRestoreNext();
  try {
    const binding = captureVerifiedWorkspace();
    const object = selectedRestoreFileId;
    const target = selectedRestoreVersionId;
    const history = model.workspace.file_histories.find((entry) => entry.object_id === object);
    if (!history || !restoreVersionChoices(history).some((entry) => entry.version_id === target)) {
      throw new WorkspaceVerificationSuperseded();
    }
    const preview = validatedRestorePreview(JSON.parse(await invoke('preview_managed_restore', {
      objectId: object,
      targetVersion: target,
      expectedWorkspaceRoot: binding.root,
      expectedWorkspaceDigest: binding.digest,
      expectedWorkspaceInstallation: binding.installation,
    })), history, target, binding);
    assertVerifiedWorkspace(binding);
    if (selectedRestoreFileId !== object || selectedRestoreVersionId !== target) {
      throw new WorkspaceVerificationSuperseded();
    }
    model.restorePreview = {
      preview,
      expectedContentDigest: preview.working_copy.content_digest,
      expectedExecutable: preview.working_copy.executable,
    };
    renderRestoreNext();
  } catch (error) {
    showNotice(String(error), true);
    renderRestoreNext();
  }
}

async function restoreVersion(
  objectId,
  targetVersion,
  expectedContentDigest,
  expectedExecutable,
  undo = false,
  onDispatched = () => {},
) {
  const binding = managedMutationBinding();
  return coordinateWorkspaceMutation(async (sequence) => {
    onDispatched();
    const result = JSON.parse(await invoke('restore_managed_version', {
      objectId,
      targetVersion,
      expectedContentDigest,
      expectedExecutable,
      ...binding,
    }));
    await installVerifiedWorkspace(() => call('workspace.state'), sequence);
    model.editor = null;
    renderWorkspace();
    showNotice(result.stable_after_idle
      ? `${undo ? 'Undo' : 'Restore'} completed in the working copy; exact recovery is preserved.`
      : `${undo ? 'Undo' : 'Restore'} recovery is preserved, but a newer external edit was detected.`);
    return result;
  });
}

async function applySelectedRestore() {
  const history = model.workspace?.file_histories
    .find((entry) => entry.object_id === selectedRestoreFileId);
  const targetVersion = selectedRestoreVersionId;
  const previewAuthority = model.restorePreview;
  const undoTarget = previewAuthority?.preview?.undo_target;
  const previewStillExact = history
    && previewAuthority
    && !restoreApplyAttempted
    && previewAuthority.preview.object_id === history.object_id
    && previewAuthority.preview.target?.version_id === targetVersion
    && restoreVersionChoices(history).some((entry) => (
      entry.version_id === previewAuthority.preview.target.version_id
      && entry.manifest_id === previewAuthority.preview.target.manifest_id
    ));
  if (!previewStillExact || managedWorkspaceMutationBlocked() || editorDraftPending()) {
    showNotice('That restore preview is no longer current. Choose the file and saved version, then preview it again.', true);
    renderRestoreNext();
    return;
  }
  let dispatched = false;
  try {
    const result = await restoreVersion(
      history.object_id,
      targetVersion,
      previewAuthority.expectedContentDigest,
      previewAuthority.expectedExecutable,
      false,
      () => {
        dispatched = true;
        restoreApplyAttempted = true;
        model.restorePreview = null;
        renderRestoreNext();
      },
    );
    restoreRecoveryScanPending = false;
    model.restoreUndo = undoTarget ? {
      objectId: history.object_id,
      versionId: undoTarget.version_id,
      expectedContentDigest: result.content_digest,
      expectedExecutable: result.executable,
    } : null;
    renderRestoreNext();
  } catch (error) {
    // A rejected promise cannot distinguish a native refusal from a commit whose reply was lost.
    // Never replay it. The next explicit Refresh performs a read-only byte scan and offers the
    // durable current version as a separately previewed recovery target if the restore committed.
    if (dispatched) restoreRecoveryScanPending = true;
    else restoreApplyAttempted = false;
    showNotice(String(error), true);
    renderRestoreNext();
  }
}

async function undoSelectedRestore() {
  const undoAuthority = model.restoreUndo;
  if (!undoAuthority || managedWorkspaceMutationBlocked() || editorDraftPending()) {
    showNotice('Undo is no longer available for the exact restored working file. Refresh and inspect the current file history.', true);
    renderRestoreNext();
    return;
  }
  let dispatched = false;
  try {
    await restoreVersion(
      undoAuthority.objectId,
      undoAuthority.versionId,
      undoAuthority.expectedContentDigest,
      undoAuthority.expectedExecutable,
      true,
      () => {
        dispatched = true;
        model.restoreUndo = null;
        renderRestoreNext();
      },
    );
    restoreRecoveryScanPending = false;
    renderRestoreNext();
  } catch (error) {
    if (dispatched) restoreRecoveryScanPending = true;
    showNotice(String(error), true);
    renderRestoreNext();
  }
}

async function scanNativeFolder({
  automatic = false,
  startup = false,
  periodic = false,
  workspaceReturn = false,
  agentFinished = false,
  allowAutomaticSave = true,
  withinNativeWorkspaceLaunch = false,
} = {}) {
  // The generic inspection commands require an unassigned workspace. Finish agent handoff uses
  // the separate exact-generation aggregate preflight while custody is active; never enter the
  // generic native path here or let an automatic focus scan overwrite the truthful handoff state.
  if (!agentFinished && refuseOrdinaryInspectionDuringAgentHandoff({ automatic })) return false;
  // A focus event is only a read hint. Never let it replace an in-app draft, overlap a managed
  // mutation, or start a second O(files) walk during a focus storm. A restored legacy workspace
  // has no stable link yet, but it still needs one exact read-only startup inspection so tracked
  // edits and new agent work are not hidden until a later focus cycle.
  const daemonKnowsNativeWork = (model.workspace?.native_untracked_files || []).length > 0
    || (model.workspace?.native_unsupported_entries || []).length > 0;
  if (
    (periodic && (
      model.folderChanges.length > 0
      || (appDocument.visibilityState !== 'visible' && !model.nativeCaptureEnabled)
    )) ||
    folderScanInFlight ||
    (nativeWorkspaceLaunchInFlight && !withinNativeWorkspaceLaunch) ||
    ((workspaceMutationInFlight || workspaceTransitionInFlight) && !workspaceReturn) ||
    !model.workspace ||
    !model.workspaceVerified ||
    (automatic && (
      (!startup && !agentFinished && !model.activeFolder?.path && !daemonKnowsNativeWork)
      // Finish owns a closed exact-generation transition. Its preflight already ran under active
      // custody and the post-release scan deliberately preserves any open editor presentation.
      // A retained legacy Save-button mirror is not authority to skip this mandatory scan after
      // the non-idempotent release has completed.
      || (!agentFinished && workspaceChangesEditorState.canPreserveEdit)
    ))
  ) return false;

  // A zero-history folder has no durable parent against which a native change can be adopted.
  // Its ordinary tree belongs to the protected import preview instead. Classifying directories
  // as saveable native changes here hides onboarding behind an impossible review queue.
  if (rememberedWorkspaceNeedsImport(model.workspace)) {
    model.folderChanges = [];
    model.nativeInspectionFailed = false;
    workspaceChangesQueueState.scanState = 'idle';
    renderEditorChoices();
    if (!automatic) {
      showNotice('This folder is not saved in Mesh yet. Use Preview this folder to review its exact content before creating the private workspace.');
    }
    return true;
  }

  const priorLabel = workspaceChangesQueueState.scanLabel;
  let settleScan;
  let binding = null;
  let queuePresentationChanged = false;
  let editorPresentationChanged = false;
  folderScanInFlight = true;
  workspaceChangesQueueState.canScan = false;
  workspaceChangesQueueState.scanLabel = 'Finding changes…';
  if (!periodic) workspaceChangesQueueState.scanState = 'scanning';
  if (!periodic) renderWorkspaceFilesChangesNext();
  const inspectionFailedBefore = model.nativeInspectionFailed;
  folderScanSettled = new Promise((resolve) => {
    settleScan = resolve;
  });
  try {
    if (!automatic) {
      model.folderChanges = [];
      renderFolderChanges();
    }
    // An editor or agent can create a native file after the last rendered workspace.state. The
    // daemon inventory is what makes that new path discoverable, so refresh both workspace and
    // checkpoint truth inside this action instead of requiring a separate Refresh click first.
    binding = await refreshVerifiedWorkspaceForFolderScan({
      preserveVerifiedPresentation: periodic,
    });
    // The first verified frame was rendered before this scan. Compare the complete candidate set
    // even during automatic restoration: daemon state can name new files, but it cannot truthfully
    // classify tracked OS bytes without inspecting them.
    const allCandidates = folderCandidates();
    const candidates = periodic
      ? periodicFolderCandidates(allCandidates, binding)
      : allCandidates;
    // workspace.state has already completed this tick's native-tree discovery. Its file list is
    // enough for a root-level creation, but a file below a native-only parent cannot be adopted
    // until that directory is bound to its exact OS identity. Pay for the second directory pass
    // only in that actionable case; otherwise the repeating poll remains one tree traversal.
    const discoverDirectories = !periodic || periodicScanNeedsDirectoryDiscovery();
    let changes = await findFolderChanges(binding, candidates, {
      discoverDirectories,
    });
    // The rotating periodic read is deliberately incomplete for large workspaces. It may wake the
    // automatic path, but it may never authorize a save by itself. Once it notices any work, pay
    // for one complete scan before constructing the queue that can be signed. This keeps the idle
    // no-change cost bounded without turning a 128-file slice into a dishonest whole-tree claim.
    if (periodic && model.nativeCaptureEnabled && changes.length > 0) {
      changes = await findFolderChanges(binding);
    }
    // Keep only review summaries in the queue. A scan may cover many text files, and batch save
    // deliberately re-inspects each exact file instead of retaining every file body in webview
    // memory or treating scan-time bytes as fresh authority.
    const nextFolderChanges = changes.map(folderChangeSummary);
    queuePresentationChanged = JSON.stringify(model.folderChanges) !== JSON.stringify(nextFolderChanges);
    model.folderChanges = nextFolderChanges;
    model.nativeInspectionFailed = false;
    if (changes.length > 0) workspaceChangesQueueState.scanState = 'changes';
    else if (!periodic) workspaceChangesQueueState.scanState = 'clean';
    if (!changes.length) {
      if (!automatic) {
        showNotice('No supported native changes were found. The native folder matches private history.');
      }
      return true;
    }
    const editable = changes.find((change) => !change.native_missing
      && !change.native_directory
      && !change.native_unsupported);
    // Native inspection may take long enough for the person to open or edit another file after
    // this scan began. Keep the verified queue, but never let a late read replace editor state that
    // became active while the filesystem walk was in flight.
    const keptOpenEditor = Boolean(model.editor);
    if (editable && !keptOpenEditor) {
      installEditorInspection(editable);
      editorPresentationChanged = true;
    }
    const switchContext = workspaceReturn ? `${NATIVE_SWITCH_HANDLE_NOTICE} ` : '';
    const discoveryContext = automatic
      ? agentFinished
        ? 'Mesh checked the agent folder before release. '
        : startup
        ? `Mesh reopened this workspace. ${switchContext}`
        : periodic
          ? 'Mesh noticed new native work. '
          : 'You returned to Mesh. '
      : '';
    const editorContext = keptOpenEditor
      ? ' Mesh kept the file already open in its editor; preserve that draft before opening another change.'
      : '';
    const unsupportedContext = changes.some((change) => change.native_unsupported)
      ? changes.some((change) => change.unsupported_kind === 'excluded-ancestor')
        ? ' Resolve every listed exclusion conflict by including its parent too or keeping the descendant excluded before saving or review.'
        : ' Convert or remove every listed symbolic link or special entry before saving or review.'
      : '';
    showNotice(`${discoveryContext}${changes.length} native ${changes.length === 1 ? 'change was' : 'changes were'} found. Review the complete queue; Mesh saves new folders parent-first, rechecks every file, and asks you to identify each missing tracked file as a deletion or an exact rename.${unsupportedContext}${editorContext}`);
    if (
      allowAutomaticSave
      && model.nativeCaptureEnabled
      && !workspaceInstallationMatchesHandoff()
      && !keptOpenEditor
      && !changes.some((change) => change.native_missing || change.native_unsupported)
    ) {
      return saveAllPrivateChanges({
        successPrefix: `Automatic private save completed after a stable full scan. ${switchContext}`,
        failurePrefix: `Automatic private save paused. ${switchContext}`,
      });
    }
    return true;
  } catch (error) {
    // A newer Refresh, save, import, or version switch deliberately invalidates this read-only
    // hint. Its result now owns both the verified frame and the notice region; a stale scan must
    // not make that successful action look like a failure.
    if (error instanceof WorkspaceVerificationSuperseded) return false;
    // Native rejection can arrive as an ordinary IPC error after another workspace replaced the
    // scan's verified frame. Recheck the captured binding before recording an inspection failure;
    // otherwise a stale read can poison the newly opened workspace and overwrite its notice.
    if (binding) {
      try {
        assertVerifiedWorkspace(binding);
      } catch (verificationError) {
        if (verificationError instanceof WorkspaceVerificationSuperseded) return false;
        throw verificationError;
      }
    }
    // The daemon frame can remain valid while one tracked file becomes unreadable or changes
    // during inspection. That is still an incomplete view of native work. Preserve the failure
    // across Refresh and pause every write/new-agent action until a complete scan succeeds.
    model.nativeInspectionFailed = true;
    workspaceChangesQueueState.scanState = 'error';
    showNotice(String(error), true);
    return false;
  } finally {
    folderScanInFlight = false;
    settleScan();
    workspaceChangesQueueState.scanLabel = priorLabel;
    // Entering or leaving the fail-closed inspection boundary changes navigation, management,
    // restore, export, and agent controls—not only the editor queue.
    if (inspectionFailedBefore !== model.nativeInspectionFailed) renderWorkspace();
    else if (!periodic || queuePresentationChanged || editorPresentationChanged) renderEditorChoices();
  }
}

async function requestNativeFolderScan() {
  return scanNativeFolder();
}

function exactPrivateCaptureStillPending(change, candidate) {
  if (!candidate || candidate.path !== change.path) return false;
  if (change.native_directory) {
    return candidate.native_directory === true && candidate.installation === change.installation;
  }
  return candidate.native_directory !== true
    && candidate.native_missing !== true
    && candidate.native_unsupported !== true
    && candidate.content_digest === change.content_digest
    && candidate.executable === change.executable;
}

async function recoverAmbiguousPrivateCapture({ sequence, binding, change, cause }) {
  let installed = false;
  try {
    const workspace = await installVerifiedWorkspace(async () => {
      const current = await call('workspace.state');
      if (current.root !== binding.root || current.installation !== binding.installation) {
        throw new Error('the open workspace changed after the ambiguous private capture');
      }
      return current;
    }, sequence);
    installed = true;
    const recoveredBinding = captureVerifiedWorkspace();
    const changes = await findFolderChanges(recoveredBinding);
    model.folderChanges = changes.map(folderChangeSummary);
    model.nativeInspectionFailed = false;

    const pending = changes.find((candidate) => exactPrivateCaptureStillPending(change, candidate));
    const durableWorkspaceAdvanced = workspace.digest !== binding.digest;
    let committed = false;
    let committedVersion = null;
    if (change.native_directory) {
      const durableDirectory = durableWorkspaceAdvanced && (workspace.entries || []).some(
        (entry) => entry.path === change.path && entry.type === 'folder',
      );
      if (durableDirectory) {
        const inspected = JSON.parse(await invoke('inspect_managed_directory_installation', {
          relativePath: change.path,
          expectedWorkspaceRoot: recoveredBinding.root,
          expectedWorkspaceDigest: recoveredBinding.digest,
          expectedWorkspaceInstallation: recoveredBinding.installation,
        }));
        assertVerifiedWorkspace(recoveredBinding);
        committed = inspected.path === change.path
          && inspected.installation === change.installation;
      }
    } else {
      const history = (workspace.file_histories || []).find((entry) => entry.path === change.path);
      if (durableWorkspaceAdvanced && history) {
        const inspected = await inspectFolderCandidate({
          path: change.path,
          command: 'inspect_managed_file',
        }, recoveredBinding);
        const durableVersionAdvanced = change.native_untracked === true
          || (typeof change.current_version === 'string'
            && inspected.current_version !== change.current_version);
        committed = durableVersionAdvanced
          && inspected.native_untracked !== true
          && inspected.modified_from_current_version === false
          && inspected.content_digest === change.content_digest
          && inspected.executable === change.executable;
        if (committed) committedVersion = inspected.current_version;
      }
    }

    renderWorkspace();
    if (committed) {
      return {
        status: 'committed',
        result: {
          path: change.path,
          version: committedVersion,
          saved_privately: true,
          author_authenticated: true,
        },
      };
    }
    if (!durableWorkspaceAdvanced && pending) {
      return {
        status: 'pending',
        message: `Mesh lost the private-capture reply: ${cause} Mesh verified that ${change.path} is still the exact reviewed change and was not added to private history. It is safe to retry Save privately.`,
      };
    }

    // A changed record fold without the exact submitted bytes as its current authenticated version
    // can be another process's append or a newer native edit. Keep the refreshed workspace visible,
    // but fail closed instead of attributing that durable state to this lost response.
    model.nativeInspectionFailed = true;
    renderWorkspace();
    throw new Error('the refreshed workspace did not prove either the submitted private version or the unchanged reviewed candidate');
  } catch (recoveryError) {
    if (installed) {
      model.nativeInspectionFailed = true;
      renderWorkspace();
    }
    const recoveryBoundary = installed
      ? 'The refreshed workspace remains visible, but private writes and agent handoff are paused until Find folder changes succeeds.'
      : 'Mesh could not install a verified workspace after the ambiguous reply. Managed actions remain paused until Refresh succeeds.';
    throw new Error(
      `Mesh could not confirm the private capture after its reply was lost: ${cause} Read-back was inconclusive: ${recoveryError} ${recoveryBoundary}`,
      { cause: recoveryError },
    );
  }
}

async function saveAllPrivateChanges({
  successPrefix = '',
  failurePrefix = '',
  withinNativeWorkspaceLaunch = false,
} = {}) {
  const queued = [...model.folderChanges];
  if (!queued.length || queued.some((change) => change.native_missing || change.native_unsupported)) return false;
  if (workspaceInstallationMatchesHandoff()) {
    showNotice(activeAgentMutationRefusal(), true);
    return false;
  }
  const priorLabel = workspaceChangesQueueState.saveAllLabel;
  workspaceChangesQueueState.savingAll = true;
  workspaceChangesQueueState.canSaveAllPrivate = false;
  renderWorkspaceFilesChangesNext();
  const captured = captureVerifiedWorkspace();
  const initialBinding = {
    root: captured.root,
    digest: captured.digest,
    installation: captured.installation,
  };
  let completed = 0;
  let completedFolders = 0;
  let skipped = 0;
  let recoveredReplies = 0;
  try {
    await coordinateWorkspaceMutation(async (sequence) => {
      let binding = initialBinding;
      for (const [index, queuedChange] of queued.entries()) {
        workspaceChangesQueueState.saveAllLabel = `Saving ${index + 1} of ${queued.length}…`;
        renderWorkspaceFilesChangesNext();
        let result;
        let recovered = false;
        if (queuedChange.native_directory) {
          try {
            result = JSON.parse(await invoke('adopt_native_directory', {
              relativePath: queuedChange.path,
              expectedDirectoryInstallation: queuedChange.installation,
              expectedWorkspaceRoot: binding.root,
              expectedWorkspaceDigest: binding.digest,
              expectedWorkspaceInstallation: binding.installation,
            }));
          } catch (cause) {
            const recovery = await recoverAmbiguousPrivateCapture({
              sequence,
              binding,
              change: queuedChange,
              cause,
            });
            if (recovery.status !== 'committed') throw new Error(recovery.message);
            result = recovery.result;
            recovered = true;
            recoveredReplies += 1;
          }
        } else {
          const tracked = (model.workspace?.file_histories || []).some((history) => history.path === queuedChange.path);
          const native = (model.workspace?.native_untracked_files || []).includes(queuedChange.path);
          if (!tracked && !native) {
            throw new Error(`${queuedChange.path} was removed or renamed after the scan. ${completed} file${completed === 1 ? '' : 's'} were already saved; refresh before continuing.`);
          }
          const inspection = await inspectFolderCandidate({
            path: queuedChange.path,
            command: native ? 'inspect_native_file' : 'inspect_managed_file',
          }, binding);
          if (!inspection.native_untracked && !inspection.modified_from_current_version) {
            skipped += 1;
            continue;
          }
          try {
            result = JSON.parse(await invoke(inspection.native_untracked ? 'adopt_native_file' : 'save_managed_private', {
              relativePath: inspection.path,
              expectedContentDigest: inspection.content_digest,
              expectedExecutable: inspection.executable,
              expectedWorkspaceRoot: binding.root,
              expectedWorkspaceDigest: binding.digest,
              expectedWorkspaceInstallation: binding.installation,
            }));
          } catch (cause) {
            const recovery = await recoverAmbiguousPrivateCapture({
              sequence,
              binding,
              change: inspection,
              cause,
            });
            if (recovery.status !== 'committed') throw new Error(recovery.message);
            result = recovery.result;
            recovered = true;
            recoveredReplies += 1;
          }
        }
        if (!recovered) {
          await installVerifiedWorkspace(async () => {
            const workspace = await call('workspace.state');
            if (workspace.root !== binding.root || workspace.installation !== binding.installation) {
              throw new WorkspaceVerificationSuperseded();
            }
            return workspace;
          }, sequence);
        }
        binding = {
          root: model.workspace.root,
          digest: model.workspace.digest,
          installation: model.workspace.installation,
        };
        if (!result.author_authenticated) {
          throw new Error(`${queuedChange.path} did not produce an authenticated private change. ${completed} earlier change${completed === 1 ? '' : 's'} were saved; inspect it before continuing.`);
        }
        if (!result.saved_privately) {
          throw new Error(`${queuedChange.path} has an authenticated durable change, but newer native activity appeared during settling and remains Working. ${completed} earlier change${completed === 1 ? '' : 's'} were saved; inspect it before continuing.`);
        }
        completed += 1;
        if (queuedChange.native_directory) completedFolders += 1;
      }
      if (completed === 0) {
        await installVerifiedWorkspace(async () => {
          const workspace = await call('workspace.state');
          if (
            workspace.root !== binding.root
            || workspace.digest !== binding.digest
            || workspace.installation !== binding.installation
          ) {
            throw new WorkspaceVerificationSuperseded();
          }
          return workspace;
        }, sequence);
      }
      // Saving a queue is not a filesystem freeze. An agent can create or change another file
      // while the authenticated prefix is being appended. Re-scan after every batch, not only
      // after admitting a directory, so the visible "Saved privately" result cannot hide work
      // that arrived during the save and then reappear several seconds later.
      const remaining = await findFolderChanges({ sequence, ...binding });
      model.folderChanges = remaining.map(folderChangeSummary);
      model.editor = null;
      renderWorkspace();
      const completedFiles = completed - completedFolders;
      const completedParts = [];
      if (completedFolders) {
        completedParts.push(`${completedFolders} new native ${completedFolders === 1 ? 'folder was' : 'folders were'} authenticated and saved privately.`);
      }
      if (completedFiles) {
        completedParts.push(`${completedFiles} changed ${completedFiles === 1 ? 'file was' : 'files were'} authenticated and saved privately.`);
      }
      if (skipped && (completedFolders || completedFiles)) {
        completedParts.push(`${skipped} already-matching ${skipped === 1 ? 'file was' : 'files were'} skipped.`);
      }
      const completedSummary = completedParts.join(' ')
        || `No queued file needed saving; ${skipped} ${skipped === 1 ? 'file already matches' : 'files already match'} private history.`;
      const remainingSummary = remaining.length
        ? `${remaining.length} newer native ${remaining.length === 1 ? 'change remains' : 'changes remain'} ready for review; save again after inspecting the queue.`
        : '';
      const recoverySummary = recoveredReplies
        ? `Mesh lost ${recoveredReplies === 1 ? 'a private-capture reply' : `${recoveredReplies} private-capture replies`} but verified the exact ${recoveredReplies === 1 ? 'change was' : 'changes were'} saved privately without replaying ${recoveredReplies === 1 ? 'it' : 'them'}. `
        : '';
      showNotice(`${successPrefix}${recoverySummary}${[completedSummary, remainingSummary].filter(Boolean).join(' ')}`);
    }, { withinNativeWorkspaceLaunch });
    return true;
  } catch (error) {
    let queueRefreshed = false;
    if (completed > 0 || skipped > 0) {
      queueRefreshed = await scanNativeFolder({
        automatic: true,
        startup: true,
        withinNativeWorkspaceLaunch,
      });
    }
    showNotice(
      `${failurePrefix}${String(error)}${queueRefreshed ? ' The remaining queue was refreshed from the current native folder.' : ''}`,
      true,
    );
    return false;
  } finally {
    workspaceChangesQueueState.saveAllLabel = priorLabel;
    workspaceChangesQueueState.savingAll = false;
    renderEditorChoices();
  }
}
function validatedNativeCapturePreference(answer) {
  if (typeof answer.enabled !== 'boolean') throw new Error('invalid preference response');
  return answer.enabled;
}

async function readNativeCapturePreference() {
  return validatedNativeCapturePreference(JSON.parse(await invoke('native_capture_preference')));
}

async function updateNativeCapturePreference(requested) {
  if (!model.nativeCaptureAvailable || model.nativeCaptureChanging || workspaceInteractionInFlight()) return;
  model.nativeCaptureChanging = true;
  renderEditorChoices();
  try {
    const answer = JSON.parse(await invoke('set_native_capture_preference', { enabled: requested }));
    if (answer.enabled !== requested) {
      throw new Error('Mesh did not read back the selected automatic-save mode exactly.');
    }
    model.nativeCaptureEnabled = requested;
    showNotice(requested
      ? 'Automatic private save is on for complete, stable file and folder scans. Ambiguous changes still pause for review.'
      : 'Review-first mode is on. Mesh will keep noticing native edits and wait for an explicit private save.');
  } catch (error) {
    try {
      const recovered = await readNativeCapturePreference();
      model.nativeCaptureEnabled = recovered;
      showNotice(
        recovered === requested
          ? `Mesh lost the automatic-save reply but verified that Automatic private save is ${recovered ? 'on' : 'off'}. The persistent choice was not replayed.`
          : `${error} Mesh verified that Automatic private save is still ${recovered ? 'on' : 'off'}; the requested change did not take effect.`,
        recovered !== requested,
      );
    } catch (recoveryError) {
      // Automatic capture is driven by this window. If durable preference truth cannot be read,
      // pause it locally and disable the toggle rather than showing the prior value as persistent.
      // Startup will read the owner-only preference again before any restored-workspace scan.
      model.nativeCaptureEnabled = false;
      model.nativeCaptureAvailable = false;
      showNotice(
        `Mesh could not confirm the automatic-save choice: ${error} The read-back also failed: ${recoveryError} Automatic private capture is paused in this window. Restart Mesh before relying on this setting.`,
        true,
      );
    }
  } finally {
    model.nativeCaptureChanging = false;
    renderEditorChoices();
  }
  if (
    requested === true
    && model.nativeCaptureEnabled
    && !model.editor
    && workspaceFileActionEnabled('save-all-private')
  ) {
    let binding;
    try {
      // The preference is global, while the optional immediate queue save is workspace-scoped.
      // Bind only that follow-up to the exact workspace which is current after durable read-back;
      // a recovery-blocked workspace must still be able to change the global preference.
      binding = captureVerifiedWorkspace();
      assertVerifiedWorkspace(binding);
    } catch (error) {
      showNotice(
        `Automatic private save is ${model.nativeCaptureEnabled ? 'on' : 'off'}, but Mesh could not verify the current workspace for the waiting queue. No queued files were saved automatically. ${error}`,
        true,
      );
      return;
    }
    await saveAllPrivateChanges({
      successPrefix: 'Automatic private save enabled. ',
      failurePrefix: 'Automatic private save paused. ',
    });
  }
}
async function recordSelectedStructuralChange() {
  if (!workspaceChangesQueueState.canRecordStructural) return;
  const fromPath = workspaceChangesQueueState.missingSource;
  const toPath = workspaceChangesQueueState.moveTarget;
  const missing = model.folderChanges.find((change) => change.native_missing && change.path === fromPath);
  if (!missing) return;
  const destination = toPath
    ? model.folderChanges.find((change) => change.native_untracked && change.path === toPath)
    : null;
  if (toPath && !destination) {
    showNotice('That rename candidate is no longer in the verified scan. Scan the folder again.', true);
    return;
  }
  const description = toPath
    ? `Record that ${fromPath} moved to ${toPath}? Mesh will preserve the same file identity and will not move the native file again.`
    : `Record that ${fromPath} was deleted? Its immutable saved versions remain available.`;
  if (!confirm(description)) return;
  let attemptedChange = null;
  let changeDispatched = false;
  try {
    const binding = managedMutationBinding();
    attemptedChange = Object.freeze({
      root: binding.expectedWorkspaceRoot,
      installation: binding.expectedWorkspaceInstallation,
      fromPath,
      toPath: toPath || null,
      entryType: 'file',
      label: toPath ? 'native move' : 'native deletion',
    });
    await coordinateWorkspaceMutation(async (sequence) => {
      changeDispatched = true;
      const result = JSON.parse(await invoke(toPath ? 'adopt_native_file_move' : 'adopt_native_file_deletion', {
        ...(toPath ? {
          fromPath,
          toPath,
          expectedDestinationContentDigest: destination.content_digest,
          expectedDestinationExecutable: destination.executable,
        } : { relativePath: fromPath }),
        expectedCurrentVersion: missing.current_version,
        ...binding,
      }));
      await completeManagedChange(
        result,
        toPath ? `Recorded the native move from ${fromPath} to ${toPath}.` : `Recorded the native deletion of ${fromPath}.`,
        sequence,
      );
      model.folderChanges = [];
      renderFolderChanges();
      showNotice(result.saved_privately
        ? `${toPath ? 'Move' : 'Deletion'} authenticated and saved privately. Scan again to review any remaining native changes.`
        : `${toPath ? 'Move' : 'Deletion'} is durable, but newer native activity keeps the workspace Working. Scan again before continuing.`);
    });
  } catch (error) {
    if (attemptedChange && changeDispatched) {
      model.folderChanges = [];
      renderFolderChanges();
      await recoverAmbiguousManagedEntryChange(attemptedChange, error);
    } else {
      showNotice(String(error), true);
    }
  }
}
async function inspectSelectedManagedFile() {
  if (refuseOrdinaryInspectionDuringAgentHandoff()) return;
  try {
    const relativePath = workspaceChangesEditorState.selectedFile;
    if (refuseEditorDraftFileDeparture(relativePath)) return;
    const binding = captureVerifiedWorkspace();
    const nativeUntracked = (model.workspace?.native_untracked_files || []).includes(relativePath);
    const editor = JSON.parse(await invoke(nativeUntracked ? 'inspect_native_file' : 'inspect_managed_file', {
      relativePath,
      expectedWorkspaceRoot: binding.root,
      expectedWorkspaceDigest: binding.digest,
      expectedWorkspaceInstallation: binding.installation,
    }));
    assertVerifiedWorkspace(binding);
    if (workspaceChangesEditorState.selectedFile !== relativePath) throw new WorkspaceVerificationSuperseded();
    installEditorInspection(editor);
  } catch (error) { showNotice(String(error), true); }
}

async function inspectManagedWorkspaceEntryFromFiles(relativePath) {
  const entry = model.workspace?.entries.find((candidate) => candidate.path === relativePath) || null;
  if (!entry || entry.type !== 'file' || !model.workspaceVerified) return false;
  if (refuseEditorDraftFileDeparture(relativePath)) return false;
  const knownFile = (model.workspace?.file_histories || []).some((history) => history.path === relativePath)
    || (model.workspace?.native_untracked_files || []).includes(relativePath);
  if (!knownFile) return false;
  if (workspaceInstallationMatchesHandoff()) {
    const sequence = ++workspaceFilePreviewSequence;
    const binding = captureVerifiedWorkspace();
    const agentGeneration = canonicalAgentHandoffGeneration(model.agentHandoff?.generation);
    if (!agentGeneration) return false;
    workspaceChangesEditorState.selectedFile = relativePath;
    model.editor = null;
    workspaceChangesEditorState.editorText = '';
    renderWorkspaceFilesChangesNext();
    try {
      const answer = JSON.parse(await invoke('inspect_agent_live_file', {
        expectedWorkspaceRoot: binding.root,
        expectedWorkspaceDigest: binding.digest,
        expectedWorkspaceInstallation: binding.installation,
        expectedAgentHandoffGeneration: agentGeneration,
        relativePath,
      }));
      assertVerifiedWorkspace(binding);
      const keys = answer && typeof answer === 'object' && !Array.isArray(answer)
        ? Object.keys(answer).sort()
        : [];
      if (keys.join(',') !== 'agent_handoff_generation,byte_count,content_digest,executable,image_data_url,kind,mutable,path,preview_error,preview_kind,recorded,schema,text,workspace_digest,workspace_installation,workspace_root'
        || answer.schema !== 'mesh.agent-live-file/v1'
        || answer.workspace_root !== binding.root
        || answer.workspace_digest !== binding.digest
        || answer.workspace_installation !== binding.installation
        || answer.agent_handoff_generation !== agentGeneration
        || answer.path !== relativePath
        || !['current-file', 'modified-file', 'new-file'].includes(answer.kind)
        || !Number.isSafeInteger(answer.byte_count)
        || answer.byte_count < 0
        || typeof answer.content_digest !== 'string'
        || !/^[0-9a-f]{64}$/u.test(answer.content_digest)
        || typeof answer.executable !== 'boolean'
        || !['text', 'image', 'artifact', 'metadata'].includes(answer.preview_kind)
        || (answer.text !== null && (typeof answer.text !== 'string' || answer.text.length > 1_048_576))
        || (answer.image_data_url !== null && (typeof answer.image_data_url !== 'string'
          || !answer.image_data_url.startsWith('data:image/')
          || answer.image_data_url.length > 12 * 1024 * 1024))
        || (answer.preview_error !== null && (typeof answer.preview_error !== 'string'
          || answer.preview_error.length > 1_024))
        || answer.mutable !== true
        || answer.recorded !== false) {
        throw new Error('The assigned-folder file preview was stale or malformed.');
      }
      if (sequence !== workspaceFilePreviewSequence
        || workspaceFilesState.selectedEntry !== relativePath
        || !agentLiveInspectionStillCurrent({ ...binding, generation: agentGeneration })) return false;
      const history = (model.workspace?.file_histories || []).find((candidate) => candidate.path === relativePath);
      installEditorInspection({
        path: answer.path,
        current_version: history?.current?.version_id || 'agent-live-snapshot',
        byte_count: answer.byte_count,
        content_digest: answer.content_digest,
        executable: answer.executable,
        text: answer.text,
        baseline_text: null,
        text_editable: false,
        modified_from_current_version: answer.kind !== 'current-file',
        max_text_bytes: 1_048_576,
        native_untracked: answer.kind === 'new-file',
        agent_live: true,
      });
      renderWorkspaceFilesChangesNext();
      return true;
    } catch (error) {
      if (sequence !== workspaceFilePreviewSequence
        || workspaceFilesState.selectedEntry !== relativePath) return false;
      workspaceChangesEditorState.selectedFile = '';
      model.editor = null;
      renderWorkspaceFilesChangesNext();
      showNotice(`Mesh could not preview ${relativePath}: ${decodeDaemonRefusal(error).message || error}`, true);
      return false;
    }
  }
  workspaceFilePreviewSequence += 1;
  workspaceChangesEditorState.selectedFile = relativePath;
  renderEditorChoices();
  if (!workspaceChangesEditorState.canLoadFile) return false;
  return inspectSelectedManagedFile();
}

function updateEditorDraftPresentation() {
  workspaceChangesEditorState.canPreserveEdit = !(managedWorkspaceMutationBlocked()
    || !model.editor
    || workspaceChangesEditorState.editorText === model.editor.text);
  workspaceChangesEditorState.canSavePrivate = !(managedWorkspaceMutationBlocked()
    || workspaceChangesEditorState.canPreserveEdit
    || !model.editor?.modified_from_current_version);
  workspaceChangesEditorState.editState = !workspaceChangesEditorState.canPreserveEdit
    ? model.editor.modified_from_current_version
      ? 'Working · local folder change detected'
      : 'Working copy matches private history'
    : 'Working · unsaved edits';
  // Keep every unrelated mutation surface truthful as the draft appears or is reverted. The
  // shared beginWorkspaceMutation guard remains the final fail-closed boundary for stale or
  // scripted clicks.
  renderManagementChoices();
  renderRestoreNext();
  renderFolderChanges();
  renderWorkspaceActions();
  renderNextAction();
}

async function preserveManagedEditorText() {
  try {
    const binding = managedMutationBinding();
    await coordinateWorkspaceMutation(async (sequence) => {
      const editedText = workspaceChangesEditorState.editorText;
      const path = model.editor.path;
      let result;
      try {
        result = JSON.parse(await invoke('preserve_managed_text', {
          relativePath: path,
          text: editedText,
          expectedContentDigest: model.editor.content_digest,
          expectedExecutable: model.editor.executable,
          ...binding,
        }));
      } catch (cause) {
        await recoverAmbiguousManagedTextSave({ sequence, binding, path, editedText, cause });
        return;
      }
      await installVerifiedWorkspace(() => call('workspace.state'), sequence);
      model.editor.text = editedText;
      model.editor.content_digest = result.content_digest;
      model.editor.modified_from_current_version = true;
      renderWorkspace();
      workspaceChangesEditorState.canPreserveEdit = false;
      workspaceChangesEditorState.canSavePrivate = result.stable_after_idle;
      workspaceChangesEditorState.editState = result.stable_after_idle
        ? 'Working · exact recovery preserved'
        : 'Working · newer external edit detected';
      workspaceChangesEditorState.editVersion = `recovery ${result.recovery.slice(0, 16)}…`;
      renderWorkspaceFilesChangesNext();
      showNotice(result.stable_after_idle
        ? 'The managed file was replaced atomically and exact recovery bytes survived the settling interval.'
        : 'Mesh preserved this edit, but the file changed again during settling and remains Working.');
    }, { consumeEditorDraft: true });
  } catch (error) { showNotice(String(error), true); }
}

async function saveInspectedFilePrivately() {
  try {
    const binding = managedMutationBinding();
    await coordinateWorkspaceMutation(async (sequence) => {
      workspaceChangesEditorState.canSavePrivate = false;
      renderWorkspaceFilesChangesNext();
      const submitted = {
        path: model.editor.path,
        native_untracked: model.editor.native_untracked === true,
        current_version: model.editor.current_version,
        content_digest: model.editor.content_digest,
        executable: model.editor.executable,
      };
      let result;
      let recoveredReply = false;
      try {
        result = JSON.parse(await invoke(submitted.native_untracked ? 'adopt_native_file' : 'save_managed_private', {
          relativePath: submitted.path,
          expectedContentDigest: model.editor.content_digest,
          expectedExecutable: model.editor.executable,
          ...binding,
        }));
      } catch (cause) {
        const recovery = await recoverAmbiguousPrivateCapture({
          sequence,
          binding: {
            root: binding.expectedWorkspaceRoot,
            digest: binding.expectedWorkspaceDigest,
            installation: binding.expectedWorkspaceInstallation,
          },
          change: submitted,
          cause,
        });
        if (recovery.status !== 'committed') {
          showNotice(recovery.message, true);
          return;
        }
        result = recovery.result;
        recoveredReply = true;
      }
      if (!result.author_authenticated) throw new Error('The local version was not authenticated and cannot be shown as saved.');
      if (!recoveredReply) await installVerifiedWorkspace(() => call('workspace.state'), sequence);
      // The authenticated append changes the record-fold digest, so installing the verified
      // post-save snapshot intentionally clears every object scoped to the old workspace identity.
      // Re-open the file under the new exact root/digest instead of mutating that cleared editor or
      // carrying pre-save bytes into the new generation.
      const refreshedBinding = captureVerifiedWorkspace();
      const refreshedEditor = JSON.parse(await invoke('inspect_managed_file', {
        relativePath: result.path,
        expectedWorkspaceRoot: refreshedBinding.root,
        expectedWorkspaceDigest: refreshedBinding.digest,
        expectedWorkspaceInstallation: refreshedBinding.installation,
      }));
      assertVerifiedWorkspace(refreshedBinding);
      model.editor = refreshedEditor;
      workspaceChangesEditorState.selectedFile = result.path;
      workspaceChangesEditorState.editorText = refreshedEditor.text ?? '';
      renderWorkspace();
      const currentBytesAreSaved = result.saved_privately
        && refreshedEditor.current_version === result.version
        && !refreshedEditor.modified_from_current_version;
      workspaceChangesEditorState.editState = currentBytesAreSaved
        ? 'Saved privately · signed local version'
        : 'Working · durable version appended; newer edit detected';
      workspaceChangesEditorState.editVersion = `current ${refreshedEditor.current_version.slice(0, 16)}… · ${formatBytes(refreshedEditor.byte_count)} bytes · ${refreshedEditor.content_digest.slice(0, 16)}…`;
      renderWorkspaceFilesChangesNext();
      showNotice(currentBytesAreSaved
        ? recoveredReply
          ? 'Mesh lost the private-capture reply but verified the exact change was saved privately without replaying it. The authenticated version, manifest, and file chunks are durable and survive restart.'
          : 'Saved privately. The authenticated version, manifest, and file chunks are durable and survive restart.'
        : 'The authenticated version is durable, but the file changed during settling and remains Working.');
    });
  } catch (error) {
    workspaceChangesEditorState.canSavePrivate = !(managedWorkspaceMutationBlocked()
      || (!model.editor?.native_untracked && !model.editor?.modified_from_current_version));
    renderWorkspaceFilesChangesNext();
    showNotice(String(error), true);
  }
}

async function loadNativeCapturePreference() {
  try {
    model.nativeCaptureEnabled = validatedNativeCapturePreference(
      JSON.parse(await invoke('native_capture_preference')),
    );
    model.nativeCaptureAvailable = true;
  } catch {
    model.nativeCaptureEnabled = false;
    model.nativeCaptureAvailable = false;
  }
  renderEditorChoices();
}
appWindow.addEventListener?.('focus', () => workspaceInstallationMatchesHandoff()
  ? inspectLiveAgentWork()
  : scanNativeFolder({ automatic: true }));
appWindow.setInterval?.(() => {
  if (workspaceInstallationMatchesHandoff()) void inspectLiveAgentWork();
  else void scanNativeFolder({ automatic: true, periodic: true });
}, NATIVE_SCAN_INTERVAL_MS);
void refreshApprovalStatus();
void (async () => {
  // Load the native-host preference before startup restoration scans the workspace. Otherwise a
  // fast workspace read could expose unsaved work while a slower preference read arrives too late
  // to apply the mode the person selected before the prior app exit.
  await loadNativeCapturePreference();
  await refresh(true);
})();
