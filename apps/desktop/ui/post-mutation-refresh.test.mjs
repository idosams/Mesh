import assert from 'node:assert/strict';
import test from 'node:test';
import { workspaceVersionsRetainedInteraction } from './workspace-versions-interaction-policy.js';

const TEST_AGENT_HANDOFF_GENERATION = '11'.repeat(16);
const testAgentHandoffGeneration = (value) => (value === 0
  ? null
  : value.toString(16).padStart(32, '0'));
const REMOVED_CURRENT_CONTROLLER_IDS = new Set([
  'workspace-current-card', 'workspace-current-current', 'reveal-workspace', 'switch-version',
  'open-codex', 'start-isolated-agent', 'update-original', 'finish-agent', 'refresh',
  'workspace-more-actions', 'return-workspace', 'open-agent-terminal', 'copy-diagnostics',
  'empty-workspace', 'workspace-panel', 'state-word', 'record-count', 'working-folder-block',
  'active-folder', 'copy-working-path', 'agent-folder-details', 'agent-path-label',
  'workspace-root', 'copy-agent-path', 'native-folder-hint', 'private-version',
  'shared-version', 'entry-count', 'entries', 'conditions', 'rollback',
]);
const REMOVED_IMPORT_CONTROLLER_IDS = new Set([
  'import-eyebrow', 'import-title', 'import-next-toggle', 'step-badge', 'empty-import',
  'source-path-input', 'preview-source-path', 'preview-panel', 'source-path', 'file-count',
  'dir-count', 'byte-count', 'import-scope-hint', 'import-file-preview',
  'import-file-preview-summary', 'import-file-list', 'summary-digest', 'destination',
  'choose-destination', 'confirm-import',
]);
const REMOVED_DESTINATION_CONTROLLER_IDS = new Set([
  'export-card', 'workspace-destination-current', 'update-destination-eyebrow',
  'update-destination-title', 'export-target', 'export-hint', 'export-output',
  'export-file', 'choose-export-target', 'export-preview', 'export-confirm',
  'export-preview-all', 'export-confirm-all',
]);

class FakeClassList {
  #values = new Set();

  add(...values) {
    values.forEach((value) => this.#values.add(value));
  }

  remove(...values) {
    values.forEach((value) => this.#values.delete(value));
  }

  toggle(value, force) {
    const enabled = force === undefined ? !this.#values.has(value) : force;
    if (enabled) this.#values.add(value);
    else this.#values.delete(value);
    return enabled;
  }

  contains(value) {
    return this.#values.has(value);
  }
}

class FakeElement {
  constructor() {
    this.classList = new FakeClassList();
    this.attributes = new Map();
    this.children = [];
    this.disabled = false;
    this.innerHTML = '';
    this.lastChild = { textContent: '' };
    this.textContent = '';
    this.value = '';
    this.listeners = new Map();
    this.focused = false;
    this.scrolledIntoView = false;
  }

  addEventListener(name, listener) {
    const listeners = this.listeners.get(name) || [];
    listeners.push(listener);
    this.listeners.set(name, listeners);
  }

  setAttribute(name, value) {
    this.attributes.set(name, String(value));
  }

  getAttribute(name) {
    return this.attributes.get(name) ?? null;
  }

  replaceChildren(...children) {
    this.children = children;
  }

  async emit(name, event = {}) {
    for (const listener of this.listeners.get(name) || []) await listener(event);
  }

  click() {
    return this.emit('click');
  }

  focus() {
    this.focused = true;
  }

  scrollIntoView() {
    this.scrolledIntoView = true;
  }
}

class FakeCustomEvent {
  constructor(type, options = {}) {
    this.type = type;
    this.detail = options.detail;
  }
}

function fakeDocument() {
  const elements = new Map();
  const listeners = new Map();
  let workspaceOverview = null;
  let workspaceEntry = null;
  let workspaceCurrent = null;
  let workspaceRestore = null;
  let workspaceVersions = null;
  let workspaceWork = null;
  let workspaceDestination = null;
  let importWorkbench = null;
  const workspaceDestinationDraft = { destination: '', selectedFile: '' };
  let reviewPage = null;
  let serviceState = { state: 'starting', label: 'Local service starting' };
  let buildIdentity = {
    label: 'Build identity unavailable',
    title: 'Mesh refused a malformed or incomplete build identity.',
  };
  const document = {
    get serviceState() {
      return serviceState;
    },
    get buildIdentity() {
      return buildIdentity;
    },
    get workspaceOverview() {
      return workspaceOverview;
    },
    get workspaceEntry() {
      return workspaceEntry;
    },
    get workspaceCurrent() {
      return workspaceCurrent;
    },
    workspaceCurrentAction(id) {
      return workspaceCurrent?.current?.actions.find((action) => action.id === id) || null;
    },
    workspaceDestinationAction(id) {
      return workspaceDestination?.destination?.actions.find((action) => action.id === id) || null;
    },
    destinationField(field) {
      const modelKey = field === 'selectedFile' ? 'selectedFile' : 'destination';
      const control = {
        get value() {
          return workspaceDestinationDraft[modelKey];
        },
        set value(value) {
          workspaceDestinationDraft[modelKey] = value;
        },
        get disabled() {
          const model = workspaceDestination?.destination;
          return modelKey === 'selectedFile' ? !model?.canSelectFile : !model?.canEditDestination;
        },
        async emit(name, event = {}) {
          if (name === 'keydown') {
            if (event.key === 'Enter') {
              await document.emitWorkspaceDestinationAction('preview-all', undefined, {
                destination: workspaceDestinationDraft.destination,
              });
            }
            return;
          }
          if ((modelKey === 'selectedFile' && name !== 'change')
            || (modelKey === 'destination' && name !== 'input')) return;
          await document.emitWorkspaceDestinationIntent({
            type: 'set-field',
            field: modelKey,
            value: workspaceDestinationDraft[modelKey],
          });
        },
      };
      return control;
    },
    destinationActionControl(id) {
      return {
        get disabled() {
          return !document.workspaceDestinationAction(id)?.enabled;
        },
        get textContent() {
          return document.workspaceDestinationAction(id)?.label || '';
        },
        get focused() {
          return document.getElementById('workspace-destination-next').focused;
        },
        async emit(name) {
          if (name === 'click') await document.emitWorkspaceDestinationAction(id);
        },
      };
    },
    get destinationHint() {
      return { textContent: workspaceDestination?.destination?.hint || '' };
    },
    get destinationOutput() {
      return { textContent: workspaceDestination?.destination?.plan?.text || '' };
    },
    get workspaceRestore() {
      return workspaceRestore;
    },
    get workspaceVersions() {
      return workspaceVersions;
    },
    get workspaceWork() {
      return workspaceWork;
    },
    get workspaceDestination() {
      return workspaceDestination;
    },
    get importWorkbench() {
      return importWorkbench;
    },
    async emitImportWorkbenchIntent(intent, generation = importWorkbench?.generation) {
      if (importWorkbench && Number.isSafeInteger(generation) && generation === importWorkbench.generation) {
        this.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-mounted', {
          detail: { generation },
        }));
      }
      this.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-intent', {
        detail: { generation, intent },
      }));
      await new Promise((resolve) => setTimeout(resolve, 0));
    },
    get reviewPage() {
      return reviewPage;
    },
    workspaceVersionChoice(operation = null) {
      const shadow = this.getElementById('workspace-versions-next').shadowRoot;
      if (!shadow?.testVersionChoices) return null;
      return operation === null
        ? shadow.testVersionChoices.values().next().value || null
        : shadow.testVersionChoices.get(operation) || null;
    },
    createElement: () => new FakeElement(),
    getElementById(id) {
      if (REMOVED_CURRENT_CONTROLLER_IDS.has(id)
        || REMOVED_IMPORT_CONTROLLER_IDS.has(id)
        || REMOVED_DESTINATION_CONTROLLER_IDS.has(id)) {
        throw new Error(`test attempted to recreate removed source-owned controller: ${id}`);
      }
      if (!elements.has(id)) elements.set(id, new FakeElement());
      return elements.get(id);
    },
    addEventListener(name, listener) {
      const current = listeners.get(name) || [];
      current.push(listener);
      listeners.set(name, current);
      if (name === 'mesh:workspace-overview-available'
        || name === 'mesh:workspace-entry-available'
        || name === 'mesh:import-workbench-available'
        || name === 'mesh:workspace-current-available'
        || name === 'mesh:workspace-files-changes-available'
        || name === 'mesh:workspace-restore-available'
        || name === 'mesh:workspace-versions-available'
        || name === 'mesh:workspace-destination-available'
        || name === 'mesh:review-workbench-available') {
        if (globalThis.window && typeof globalThis.window.CustomEvent !== 'function') {
          globalThis.window.CustomEvent = FakeCustomEvent;
        }
        queueMicrotask(listener);
      }
    },
    removeEventListener(name, listener) {
      const current = listeners.get(name) || [];
      listeners.set(name, current.filter((candidate) => candidate !== listener));
    },
    dispatchEvent(event) {
      if (event.type === 'mesh:service-state-projection') serviceState = event.detail;
      if (event.type === 'mesh:build-identity-projection') buildIdentity = event.detail;
      if (event.type === 'mesh:notice-projection') {
        const notice = this.getElementById('notice');
        notice.textContent = event.detail?.message || '';
        notice.classList.toggle('error', event.detail?.error === true);
        if (event.detail?.proof) notice.setAttribute('data-mesh-agent-proof', event.detail.proof);
        else notice.attributes.delete('data-mesh-agent-proof');
      }
      if (event.type === 'mesh:workspace-overview-projection') workspaceOverview = event.detail;
      if (event.type === 'mesh:import-workbench-projection') importWorkbench = event.detail;
      if (event.type === 'mesh:workspace-current-projection') workspaceCurrent = event.detail;
      if (event.type === 'mesh:workspace-destination-projection') {
        workspaceDestination = event.detail;
        workspaceDestinationDraft.destination = event.detail?.destination?.destination || '';
        workspaceDestinationDraft.selectedFile = event.detail?.destination?.selectedFile || '';
      }
      if (event.type === 'mesh:workspace-files-changes-projection') {
        workspaceWork = event.detail;
        // Test-only compatibility mirror. Production Files state and actions are source-owned;
        // legacy journey regressions exercise the same exact typed intents through this mirror.
        const files = event.detail?.workbench?.files;
        const actions = new Map((event.detail?.workbench?.actions || []).map((action) => [action.id, action]));
        if (files) {
          const newPath = this.getElementById('manage-path');
          newPath.value = files.newPath;
          newPath.disabled = !files.canEditNewPath;
          const selected = this.getElementById('manage-entry');
          const placeholder = new FakeElement();
          placeholder.value = '';
          placeholder.textContent = files.entries.length ? 'Choose an entry' : 'No entries yet';
          selected.replaceChildren(placeholder, ...files.entries.map((entry) => {
            const option = new FakeElement();
            option.value = entry.value;
            option.textContent = entry.label;
            return option;
          }));
          selected.value = files.selectedEntry;
          selected.disabled = !files.canSelectEntry;
          const movePath = this.getElementById('move-path');
          movePath.value = files.movePath;
          movePath.disabled = !files.canEditMovePath;
          this.getElementById('management-status').textContent = files.status;
          for (const [id, action] of [
            ['create-text-entry', actions.get('create-text')],
            ['create-folder-entry', actions.get('create-folder')],
            ['move-entry', actions.get('move-entry')],
            ['delete-entry', actions.get('delete-entry')],
          ]) {
            const control = this.getElementById(id);
            control.disabled = !action?.enabled;
            control.textContent = action?.label || '';
          }
        }
        // Test-only compatibility mirror for the removed Changes text controller. Production
        // selection, draft, and save authority are source-owned and cross only typed intents.
        const changes = event.detail?.workbench?.changes;
        if (changes) {
          const host = this.getElementById('workspace-changes-next');
          host.shadowRoot ||= {
            querySelector: (selector) => selector === 'textarea'
              ? this.getElementById('file-editor')
              : null,
          };
          const selectedFile = this.getElementById('edit-file');
          const placeholder = new FakeElement();
          placeholder.value = '';
          placeholder.textContent = changes.files.length ? 'Choose a native file' : 'No files available';
          selectedFile.replaceChildren(placeholder, ...changes.files.map((entry) => {
            const option = new FakeElement();
            option.value = entry.value;
            option.textContent = entry.label;
            return option;
          }));
          selectedFile.value = changes.selectedFile;
          selectedFile.disabled = !changes.canSelectFile;
          const editor = this.getElementById('file-editor');
          editor.value = changes.editorText;
          editor.disabled = !changes.canEditText;
          this.getElementById('edit-state').textContent = changes.editState;
          this.getElementById('edit-version').textContent = changes.editVersion;
          for (const [id, action] of [
            ['load-file', actions.get('load-file')],
            ['save-file', actions.get('preserve-edit')],
            ['save-private', actions.get('save-private')],
          ]) {
            const control = this.getElementById(id);
            control.disabled = !action?.enabled;
            control.textContent = action?.label || '';
          }
          // Test-only compatibility mirror for the removed native queue controller. Product
          // queue state and authority cross only the typed React projection and intent boundary.
          const scan = this.getElementById('scan-files');
          scan.disabled = !actions.get('scan-files')?.enabled;
          scan.textContent = actions.get('scan-files')?.label || '';
          const queue = this.getElementById('folder-change-queue');
          queue.classList.toggle('hidden', changes.queue.length === 0);
          this.getElementById('folder-change-count').textContent = changes.queueSummary;
          this.getElementById('folder-change-items').replaceChildren(...changes.queue.map((change) => {
            const row = new FakeElement();
            row.textContent = [change.path, change.description, change.detail].filter(Boolean).join(' · ');
            return row;
          }));
          const saveAll = this.getElementById('save-all-private');
          saveAll.disabled = !actions.get('save-all-private')?.enabled;
          saveAll.textContent = actions.get('save-all-private')?.label || '';
          const structural = changes.structural;
          this.getElementById('native-structural-change').classList.toggle('hidden', !structural);
          const source = this.getElementById('native-missing-source');
          const target = this.getElementById('native-move-target');
          if (structural) {
            source.replaceChildren(...structural.missingSources.map((entry) => {
              const option = new FakeElement();
              option.value = entry.value;
              option.textContent = entry.label;
              return option;
            }));
            source.value = structural.missingSource;
            source.disabled = !structural.canChoose;
            const deleted = new FakeElement();
            deleted.value = '';
            deleted.textContent = 'It was deleted';
            target.replaceChildren(deleted, ...structural.moveTargets.map((entry) => {
              const option = new FakeElement();
              option.value = entry.value;
              option.textContent = entry.label;
              return option;
            }));
            target.value = structural.moveTarget;
            target.disabled = !structural.canChoose;
          } else {
            source.replaceChildren();
            source.value = '';
            source.disabled = true;
            target.replaceChildren();
            target.value = '';
            target.disabled = true;
          }
          const record = this.getElementById('record-native-structural-change');
          record.disabled = !actions.get('record-structural-change')?.enabled;
          record.textContent = actions.get('record-structural-change')?.label || '';
          const hint = this.getElementById('native-structural-hint');
          hint.textContent = structural?.hint || '';
          hint.classList.toggle('hidden', !structural);
          const automatic = this.getElementById('auto-save-native');
          automatic.checked = changes.autoSaveChecked;
          automatic.disabled = !changes.autoSaveEnabled;
          this.getElementById('auto-save-native-hint').textContent = changes.autoSaveHint;
        }
      }
      if (event.type === 'mesh:review-workbench-projection') {
        reviewPage = event.detail;
        // Compatibility mirror for older coordinator assertions. Production has no Review DOM
        // controller; these fake controls dispatch the same exact typed intent as React.
        const controls = event.detail?.controls;
        if (controls) {
          const setup = this.getElementById('setup-approval');
          setup.textContent = controls.setupApprovalLabel;
          setup.disabled = !controls.canSetupApproval;
          setup.title = controls.canSetupApproval ? '' : controls.setupApprovalReason;
          const record = this.getElementById('open-current-review');
          record.textContent = controls.recordReviewLabel;
          record.disabled = !controls.canRecordReview;
          record.title = controls.recordReviewReason;
          this.getElementById('review-count').textContent = controls.countLabel;
          this.getElementById('review-overflow').textContent = controls.overflowLabel || '';
        }
        const empty = this.getElementById('empty-reviews-message');
        empty.textContent = event.detail?.state === 'ready' ? '' : event.detail?.status?.description || '';
        const list = this.getElementById('review-items');
        if (event.detail?.state !== 'ready') {
          list.replaceChildren();
        } else {
          const item = event.detail.projection;
          const card = new FakeElement();
          const heading = new FakeElement();
          const status = new FakeElement();
          status.textContent = item.recorded === false ? 'Current version ready' : 'Current version reviewed';
          heading.replaceChildren(status, new FakeElement());
          const context = new FakeElement();
          const changeCount = item.bundle_changes.length + item.bundle_changes_not_listed;
          context.textContent = `Current saved version · ${changeCount} ${changeCount === 1 ? 'change' : 'changes'}`;
          const changes = new FakeElement();
          const proof = new FakeElement();
          const proofSummary = new FakeElement();
          proofSummary.textContent = 'Technical proof';
          const proofBody = new FakeElement();
          proofBody.textContent = `Bundle ${item.bundle} · subject ${item.subject_operation}`;
          proof.replaceChildren(proofSummary, proofBody);
          const authority = new FakeElement();
          authority.textContent = event.detail.authority.approvalReason;
          const actions = new FakeElement();
          const appendAction = (label, intent, enabled, visible = true) => {
            if (!visible) return;
            const button = new FakeElement();
            button.textContent = label;
            button.disabled = !enabled;
            button.addEventListener('click', () => {
              if (!button.disabled) return this.emitReviewIntent(intent);
              return undefined;
            });
            actions.children.push(button);
          };
          appendAction('Approve to shared version', { type: 'approve-version' }, event.detail.authority.canApprove);
          appendAction('Approve and create Git branch', { type: 'approve-and-export' }, event.detail.authority.canApproveAndExport);
          appendAction('Create Git branch', { type: 'export-git' }, event.detail.authority.canExportGit);
          card.replaceChildren(heading, context, changes, proof, authority, actions);
          card.classList.add('review-item-current');
          list.replaceChildren(card);
        }
      }
      if (event.type === 'mesh:workspace-entry-mounted') {
        this.getElementById('workspace-entry-current').classList.add('hidden');
      }
      if (event.type === 'mesh:workspace-entry-rejected') {
        this.getElementById('workspace-entry-current').classList.remove('hidden');
      }
      if (event.type === 'mesh:workspace-entry-projection') {
        workspaceEntry = event.detail;
        // Older journey assertions still inspect a tiny fake control model. Mirror the real React
        // projection here; product code no longer reads or writes these removed legacy elements.
        const entry = event.detail?.entry;
        if (entry) {
          // Older journey assertions inspect this test-only mirror. Production has no hidden
          // Workspaces entry markup; React receives the source-owned projection directly.
          this.getElementById('hero-eyebrow').textContent = entry.eyebrow;
          this.getElementById('hero-title').textContent = entry.title;
          this.getElementById('hero-lede').textContent = entry.description;
          this.getElementById('workspace-entry-summary').textContent = entry.disclosureLabel;
          this.getElementById('workspace-entry-controls').open = entry.disclosureOpen;
          this.getElementById('choose-source').textContent = entry.chooseLabel;
          this.getElementById('choose-source').disabled = !entry.canChoose;
          const recent = this.getElementById('recent-workspace');
          recent.replaceChildren(...entry.recents.map((candidate) => {
            const option = new FakeElement();
            option.value = candidate.path;
            option.textContent = candidate.state === 'current'
              ? `Current · ${candidate.label}`
              : candidate.state === 'unavailable'
                ? `Unavailable · ${candidate.label}`
              : candidate.state === 'agent-assigned'
                ? `${candidate.label} · Agent assigned`
                : candidate.label;
            option.title = candidate.path;
            return option;
          }));
          recent.value = entry.selectedRecentPath;
          recent.disabled = !entry.canSelectRecent;
          const open = this.getElementById('open-recent-workspace');
          open.disabled = !entry.canOpenRecent;
          open.textContent = entry.recentOpenLabel;
          const forget = this.getElementById('forget-recent-workspace');
          forget.disabled = !entry.canForgetRecent;
          forget.title = entry.forgetRecentTitle;
          this.getElementById('recent-workspace-hint').textContent = entry.recentHint;
        }
      }
      if (event.type === 'mesh:workspace-restore-projection') {
        workspaceRestore = event.detail;
        // Older journey assertions inspect a tiny fake control model. Mirror the real React
        // projection here; product code no longer reads or writes the removed Restore controls.
        const restore = event.detail?.restore;
        if (restore) {
          const file = this.getElementById('restore-file');
          file.replaceChildren(...restore.files.map((choice) => {
            const option = new FakeElement();
            option.value = choice.id;
            option.textContent = choice.label;
            option.title = choice.path;
            return option;
          }));
          file.value = restore.selectedFileId;
          file.disabled = !restore.canSelectFile;
          const target = this.getElementById('restore-target');
          target.replaceChildren(...restore.versions.map((choice) => {
            const option = new FakeElement();
            option.value = choice.id;
            option.textContent = choice.label;
            return option;
          }));
          target.value = restore.selectedVersionId;
          target.disabled = !restore.canSelectVersion;
          this.getElementById('restore-preview').disabled = !restore.canPreview;
          this.getElementById('restore-apply').disabled = !restore.canApply;
          const undo = this.getElementById('restore-undo');
          undo.disabled = !restore.canUndo;
          undo.textContent = restore.undoLabel;
          this.getElementById('restore-hint').textContent = restore.hint;
          const output = this.getElementById('restore-output');
          output.classList.toggle('hidden', !restore.preview);
          output.textContent = restore.preview
            ? `${restore.preview.filePath}. ${restore.preview.change} ${restore.preview.historyNote} Undo: ${restore.preview.undoNote}`
            : '';
        }
      }
      if (event.type === 'mesh:workspace-versions-projection') {
        workspaceVersions = event.detail;
        // Older native/recovery assertions use this test-only mirror. Production has no hidden
        // Versions controls; React receives this exact source-owned projection directly.
        const versions = event.detail?.versions;
        if (versions) {
          const host = this.getElementById('workspace-versions-next');
          if (!host.shadowRoot) {
            const testVersionChoices = new Map();
            const shadow = {
              activeElement: null,
              testVersionChoices,
              querySelector(selector) {
                const exact = /^\[data-mesh-version-operation="([0-9a-f]{64})"\]:not\(:disabled\)$/u.exec(selector);
                if (exact) return testVersionChoices.get(exact[1]) || null;
                if (selector === '[role="radio"][tabindex="0"]:not(:disabled)') {
                  return testVersionChoices.get(workspaceVersions?.versions.selectedOperation)
                    || testVersionChoices.values().next().value
                    || null;
                }
                return null;
              },
            };
            host.shadowRoot = shadow;
          }
          if (host.shadowRoot.testVersionChoices) {
            const retained = host.shadowRoot.testVersionChoices;
            for (const operation of [...retained.keys()]) {
              if (!versions.versions.some((candidate) => candidate.operation === operation)) retained.delete(operation);
            }
            for (const candidate of versions.versions) {
              if (retained.has(candidate.operation)) continue;
              const choice = new FakeElement();
              choice.focus = () => {
                for (const other of retained.values()) other.focused = false;
                choice.focused = true;
                host.shadowRoot.activeElement = choice;
              };
              retained.set(candidate.operation, choice);
            }
          }
          const select = this.getElementById('workspace-version');
          const placeholder = new FakeElement();
          placeholder.value = '';
          placeholder.textContent = versions.versions.length
            ? versions.historyMode === 'concurrent'
              ? 'Choose from concurrent saved history'
              : 'Choose a saved workspace'
            : 'No durable workspace versions';
          select.replaceChildren(placeholder, ...versions.versions.map((candidate) => {
            const option = new FakeElement();
            option.value = candidate.operation;
            option.textContent = candidate.label;
            return option;
          }));
          select.value = versions.selectedOperation || '';
          select.disabled = !versions.canSelect;
          const destination = this.getElementById('version-destination');
          destination.value = versions.customLocation;
          destination.disabled = !versions.canUseCustomLocation;
          destination.title = versions.canUseCustomLocation
            ? ''
            : 'Fresh agent folders always use a new Mesh-managed private location.';
          this.getElementById('version-location-summary').textContent = versions.canUseCustomLocation
            ? 'Use a custom private location'
            : 'Fresh agent location is managed by Mesh';
          const open = this.getElementById('fork-version');
          open.disabled = !versions.canOpen;
          const codex = this.getElementById('fork-version-codex');
          codex.disabled = !versions.canStartCodex;
          codex.textContent = versions.codexLabel;
          codex.title = versions.canUseCustomLocation
            ? ''
            : 'Create a fresh independent folder for this exact saved point and assign it to one agent.';
          this.getElementById('version-hint').textContent = versions.previewState === 'ready'
            ? `${versions.previewTitle} is verified and ready to open as an independent native folder. ${versions.canUseCustomLocation ? 'Mesh manages its private location unless you enter a custom one.' : 'Mesh will create a fresh app-managed physical folder for this agent.'}`
            : versions.previewSummary;
          const output = this.getElementById('workspace-version-preview');
          output.classList.toggle('hidden', versions.previewState !== 'ready');
          const changeHeading = versions.changeBasis === 'initial'
            ? 'Initial saved contents'
            : versions.changeBasis === 'previous-point'
              ? `What changed since point ${versions.basisOrdinal}`
              : versions.changeBasis === 'combined-history'
                ? 'Combined saved contents'
                : 'What changed';
          const changeLines = versions.changes.length
            ? versions.changes.join('\n')
            : versions.changeBasis === 'combined-history'
              ? 'This point combines concurrent saved work. Inspect the complete file list below.'
              : 'No visible file or folder changes.';
          output.textContent = versions.previewState === 'ready'
            ? `${versions.previewTitle}\n${versions.previewSummary}\n${changeHeading}\n${changeLines}\nFiles in this point\n${versions.entries.join('\n')}`
            : '';
        }
      }
      if (event.type === 'mesh:workspace-page-request' && event.detail?.page === 'versions') {
        const host = this.getElementById('workspace-versions-next');
        host.scrollIntoView();
        const target = host.shadowRoot?.querySelector?.(event.detail.selector)
          || this.getElementById('workspace-version');
        target.focus();
      }
      if (event.type === 'mesh:workspace-page-request' && event.detail?.page === 'review') {
        const host = this.getElementById('review-workbench-next');
        host.scrollIntoView();
        host.focus();
      }
      if (event.type === 'mesh:workspace-page-request' && event.detail?.page === 'changes') {
        const host = this.getElementById('workspace-changes-next');
        host.scrollIntoView();
        this.getElementById(event.detail?.selector === 'textarea' ? 'file-editor' : 'workspace-changes-next').focus();
      }
      if (event.type === 'mesh:workspace-page-request' && event.detail?.page === 'update') {
        const host = this.getElementById('workspace-destination-next');
        host.scrollIntoView();
        const target = host.shadowRoot?.querySelector?.(event.detail.selector) || host;
        target.focus();
      }
      for (const listener of listeners.get(event.type) || []) listener(event);
      return true;
    },
    async emitWorkspaceOverviewIntent(type, generation = workspaceOverview?.generation) {
      if (workspaceOverview && Number.isSafeInteger(generation) && generation === workspaceOverview.generation) {
        document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-mounted', {
          detail: { generation },
        }));
      }
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-intent', {
        detail: { generation, intent: { type } },
      }));
      await Promise.resolve();
      await Promise.resolve();
      await new Promise((resolve) => setTimeout(resolve, 0));
    },
    async emitWorkspaceCurrentIntent(action, generation = workspaceCurrent?.generation) {
      if (workspaceCurrent && Number.isSafeInteger(generation) && generation === workspaceCurrent.generation) {
        document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-mounted', {
          detail: { generation },
        }));
      }
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-intent', {
        detail: { generation, intent: typeof action === 'string' ? { type: 'activate', action } : action },
      }));
      await Promise.resolve();
      await Promise.resolve();
      await new Promise((resolve) => setTimeout(resolve, 0));
    },
    async emitWorkspaceEntryIntent(intent, generation = workspaceEntry?.generation) {
      if (workspaceEntry && Number.isSafeInteger(generation) && generation === workspaceEntry.generation) {
        document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-mounted', {
          detail: { generation },
        }));
      }
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
        detail: { generation, intent },
      }));
      await Promise.resolve();
      await Promise.resolve();
      await new Promise((resolve) => setTimeout(resolve, 0));
    },
    async emitWorkspaceRestoreIntent(intent, generation = workspaceRestore?.generation) {
      if (workspaceRestore && Number.isSafeInteger(generation) && generation === workspaceRestore.generation) {
        document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-mounted', {
          detail: { generation },
        }));
      }
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-intent', {
        detail: { generation, intent },
      }));
      await Promise.resolve();
      await Promise.resolve();
      await new Promise((resolve) => setTimeout(resolve, 0));
    },
    async emitWorkspaceVersionsIntent(intent, generation = workspaceVersions?.generation) {
      if (workspaceVersions && Number.isSafeInteger(generation) && generation === workspaceVersions.generation) {
        document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-mounted', {
          detail: { generation },
        }));
      }
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-intent', {
        detail: { generation, intent },
      }));
      await Promise.resolve();
      await Promise.resolve();
      await new Promise((resolve) => setTimeout(resolve, 0));
    },
    async emitWorkspaceWorkIntent(intent, generation = workspaceWork?.generation) {
      if (workspaceWork && Number.isSafeInteger(generation) && generation === workspaceWork.generation) {
        document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-mounted', {
          detail: { generation },
        }));
      }
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-intent', {
        detail: { generation, intent },
      }));
      await Promise.resolve();
      await Promise.resolve();
      await new Promise((resolve) => setTimeout(resolve, 0));
    },
    async emitWorkspaceDestinationIntent(intent, generation = workspaceDestination?.generation) {
      if (workspaceDestination
        && Number.isSafeInteger(generation)
        && generation === workspaceDestination.generation) {
        document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-mounted', {
          detail: { generation },
        }));
      }
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-intent', {
        detail: { generation, intent },
      }));
      await Promise.resolve();
      await Promise.resolve();
      await new Promise((resolve) => setTimeout(resolve, 0));
    },
    async emitWorkspaceDestinationAction(id, generation = workspaceDestination?.generation, values = {}) {
      const destination = workspaceDestination?.destination;
      let intent = { type: 'activate', action: id };
      if (id === 'preview-single') {
        intent = {
          ...intent,
          selectedFile: values.selectedFile ?? destination?.selectedFile,
          destination: values.destination ?? destination?.destination,
        };
      } else if (id === 'preview-all') {
        intent = { ...intent, destination: values.destination ?? destination?.destination };
      }
      await this.emitWorkspaceDestinationIntent(intent, generation);
    },
    async emitReviewIntent(intent, generation = reviewPage?.generation) {
      if (reviewPage && Number.isSafeInteger(generation) && generation === reviewPage.generation) {
        document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-mounted', {
          detail: { generation, bundle: reviewPage.state === 'ready' ? reviewPage.projection.bundle : null },
        }));
      }
      document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
        detail: {
          generation,
          bundle: reviewPage?.state === 'ready' ? reviewPage.projection.bundle : null,
          intent,
        },
      }));
      await Promise.resolve();
      await Promise.resolve();
      await new Promise((resolve) => setTimeout(resolve, 0));
    },
  };
  for (const id of ['manage-path', 'manage-entry', 'move-path', 'create-text-entry', 'create-folder-entry', 'move-entry', 'delete-entry', 'edit-file', 'load-file', 'file-editor', 'save-file', 'save-private', 'scan-files', 'save-all-private', 'native-missing-source', 'native-move-target', 'record-native-structural-change', 'auto-save-native']) {
    document.getElementById(id).disabled = true;
  }
  document.getElementById('manage-path').addEventListener('input', () => (
    document.emitWorkspaceWorkIntent({
      type: 'set-field',
      field: 'newPath',
      value: document.getElementById('manage-path').value,
    })
  ));
  document.getElementById('manage-entry').addEventListener('change', () => (
    document.emitWorkspaceWorkIntent({
      type: 'set-field',
      field: 'selectedEntry',
      value: document.getElementById('manage-entry').value,
    })
  ));
  document.getElementById('move-path').addEventListener('input', () => (
    document.emitWorkspaceWorkIntent({
      type: 'set-field',
      field: 'movePath',
      value: document.getElementById('move-path').value,
    })
  ));
  document.getElementById('create-text-entry').addEventListener('click', () => (
    document.emitWorkspaceWorkIntent({
      type: 'activate', action: 'create-text', field: 'newPath', value: document.getElementById('manage-path').value,
    })
  ));
  document.getElementById('create-folder-entry').addEventListener('click', () => (
    document.emitWorkspaceWorkIntent({
      type: 'activate', action: 'create-folder', field: 'newPath', value: document.getElementById('manage-path').value,
    })
  ));
  document.getElementById('move-entry').addEventListener('click', () => (
    document.emitWorkspaceWorkIntent({
      type: 'activate', action: 'move-entry', field: 'movePath', value: document.getElementById('move-path').value,
    })
  ));
  document.getElementById('delete-entry').addEventListener('click', () => (
    document.emitWorkspaceWorkIntent({ type: 'activate', action: 'delete-entry' })
  ));
  document.getElementById('edit-file').addEventListener('change', () => (
    document.emitWorkspaceWorkIntent({
      type: 'set-field',
      field: 'selectedFile',
      value: document.getElementById('edit-file').value,
    })
  ));
  document.getElementById('load-file').addEventListener('click', () => (
    document.emitWorkspaceWorkIntent({
      type: 'activate', action: 'load-file', field: 'selectedFile', value: document.getElementById('edit-file').value,
    })
  ));
  document.getElementById('file-editor').addEventListener('input', () => (
    document.emitWorkspaceWorkIntent({
      type: 'set-field',
      field: 'editorText',
      value: document.getElementById('file-editor').value,
    })
  ));
  document.getElementById('save-file').addEventListener('click', () => (
    document.emitWorkspaceWorkIntent({
      type: 'activate', action: 'preserve-edit', field: 'editorText', value: document.getElementById('file-editor').value,
    })
  ));
  document.getElementById('save-private').addEventListener('click', () => (
    document.emitWorkspaceWorkIntent({ type: 'activate', action: 'save-private' })
  ));
  document.getElementById('scan-files').addEventListener('click', () => (
    document.emitWorkspaceWorkIntent({ type: 'activate', action: 'scan-files' })
  ));
  document.getElementById('save-all-private').addEventListener('click', () => (
    document.emitWorkspaceWorkIntent({ type: 'activate', action: 'save-all-private' })
  ));
  document.getElementById('native-missing-source').addEventListener('change', () => (
    document.emitWorkspaceWorkIntent({
      type: 'set-field',
      field: 'missingSource',
      value: document.getElementById('native-missing-source').value,
    })
  ));
  document.getElementById('native-move-target').addEventListener('change', () => (
    document.emitWorkspaceWorkIntent({
      type: 'set-field',
      field: 'moveTarget',
      value: document.getElementById('native-move-target').value,
    })
  ));
  document.getElementById('record-native-structural-change').addEventListener('click', () => (
    document.emitWorkspaceWorkIntent({
      type: 'activate',
      action: 'record-structural-change',
      missingSource: document.getElementById('native-missing-source').value,
      moveTarget: document.getElementById('native-move-target').value,
    })
  ));
  document.getElementById('auto-save-native').addEventListener('change', () => (
    document.emitWorkspaceWorkIntent({
      type: 'set-auto-save',
      checked: document.getElementById('auto-save-native').checked,
    })
  ));
  document.getElementById('recent-workspace').addEventListener('change', () => (
    document.emitWorkspaceEntryIntent({
      type: 'select-recent',
      path: document.getElementById('recent-workspace').value,
    })
  ));
  document.getElementById('open-recent-workspace').addEventListener('click', () => (
    document.emitWorkspaceEntryIntent({
      type: 'open-recent',
      path: document.workspaceEntry?.entry.selectedRecentPath || '',
    })
  ));
  document.getElementById('forget-recent-workspace').addEventListener('click', () => (
    document.emitWorkspaceEntryIntent({
      type: 'forget-recent',
      path: document.workspaceEntry?.entry.selectedRecentPath || '',
    })
  ));
  document.getElementById('restore-file').addEventListener('change', () => (
    document.emitWorkspaceRestoreIntent({
      type: 'select-file',
      id: document.getElementById('restore-file').value,
    })
  ));
  document.getElementById('restore-target').addEventListener('change', () => (
    document.emitWorkspaceRestoreIntent({
      type: 'select-version',
      id: document.getElementById('restore-target').value,
    })
  ));
  document.getElementById('restore-preview').addEventListener('click', () => (
    document.emitWorkspaceRestoreIntent({ type: 'preview' })
  ));
  document.getElementById('restore-apply').addEventListener('click', () => (
    document.emitWorkspaceRestoreIntent({ type: 'apply' })
  ));
  document.getElementById('restore-undo').addEventListener('click', () => (
    document.emitWorkspaceRestoreIntent({ type: 'undo' })
  ));
  document.getElementById('workspace-version').addEventListener('change', () => (
    document.emitWorkspaceVersionsIntent({
      type: 'select-version',
      operation: document.getElementById('workspace-version').value,
    })
  ));
  document.getElementById('version-destination').addEventListener('input', () => (
    document.emitWorkspaceVersionsIntent({
      type: 'set-custom-location',
      path: document.getElementById('version-destination').value,
    })
  ));
  document.getElementById('fork-version').addEventListener('click', () => (
    document.emitWorkspaceVersionsIntent({
      type: 'open-version',
      operation: document.workspaceVersions?.versions.selectedOperation || '',
    })
  ));
  document.getElementById('fork-version-codex').addEventListener('click', () => (
    document.emitWorkspaceVersionsIntent({
      type: 'start-codex',
      operation: document.workspaceVersions?.versions.selectedOperation || '',
    })
  ));
  document.getElementById('setup-approval').addEventListener('click', () => (
    document.emitReviewIntent({ type: 'setup-approval' })
  ));
  document.getElementById('open-current-review').addEventListener('click', () => (
    document.emitReviewIntent({ type: 'record-review' })
  ));
  return document;
}

async function waitFor(predicate, timeoutMs = 5_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
  throw new Error('desktop UI did not finish its initial refresh');
}

function daemonRefusal(code, message) {
  return JSON.stringify({ kind: 'mesh-daemon-refusal', code, message });
}

function importPreview({ files, directories, bytes, summary, source_scope }) {
  const listed = Math.min(files, 24);
  return {
    files,
    directories,
    bytes,
    summary,
    ...(source_scope === undefined ? {} : { source_scope }),
    file_entries: Array.from({ length: listed }, (_, index) => ({
      path: `preview-${String(index).padStart(2, '0')}.txt`,
      bytes: index === 0 ? String(bytes) : '0',
      executable: false,
    })),
    files_not_listed: files - listed,
  };
}

function savedWorkspacePreview(operation, entries = [], options = {}) {
  const files = entries.filter((entry) => entry.type === 'file');
  const folders = entries.filter((entry) => entry.type === 'folder');
  const changes = options.changes || entries.map((entry) => ({
    path: entry.path,
    type: entry.type,
    effect: 'added',
  }));
  return JSON.stringify({
    action: 'workspace-version-preview',
    source_version: operation,
    ordinal: options.ordinal || 1,
    basis_ordinal: options.basisOrdinal ?? null,
    change_basis: options.changeBasis || 'initial',
    actor_sequence: options.actorSequence || '1',
    folders: folders.length,
    files: files.length,
    total_bytes: String(files.reduce((sum, entry) => sum + Number(entry.bytes || 0), 0)),
    entries,
    entries_not_listed: 0,
    changes,
    changes_not_listed: 0,
    content_verified: true,
    creates_folder: false,
  });
}

function exactRestorePreview({
  objectId,
  currentVersion,
  targetVersion,
  currentManifest = `manifest-${currentVersion}`,
  targetManifest = `manifest-${targetVersion}`,
  path = 'kept.txt',
  workspaceRoot = '/managed/project',
  workspaceDigest = 'state-project',
  workspaceInstallation = 'state-project',
  workingDigest = 'ab'.repeat(32),
  workingExecutable = false,
  workingByteCount = '7',
  workingModified = false,
  targetDigest = 'cd'.repeat(32),
  targetExecutable = false,
  targetByteCount = '7',
  undoVersion = currentVersion,
  undoManifest = currentManifest,
}) {
  return JSON.stringify({
    schema: 'mesh.managed-working-copy-restore-preview/v1',
    canonical_state_read_only: true,
    workspace: {
      root: workspaceRoot,
      digest: workspaceDigest,
      installation: workspaceInstallation,
    },
    object_id: objectId,
    path,
    current: { version_id: currentVersion, manifest_id: currentManifest },
    target: {
      version_id: targetVersion,
      manifest_id: targetManifest,
      content_digest: targetDigest,
      byte_count: targetByteCount,
      executable: targetExecutable,
    },
    working_copy: {
      content_digest: workingDigest,
      byte_count: workingByteCount,
      executable: workingExecutable,
      modified_from_current_version: workingModified,
    },
    history_unchanged: true,
    undo_target: undoVersion === null ? null : {
      version_id: undoVersion,
      manifest_id: undoManifest,
    },
    execution_authorized: false,
  });
}

function recordedCurrentReview(subjectOperation, reviewedHead) {
  return {
    bundle: 'a1'.repeat(32),
    subject_operation: subjectOperation,
    reviewed_head: reviewedHead,
    opened_by: 'a2'.repeat(32),
    author: 'a3'.repeat(32),
    recorded: true,
    actor_sequence: '1',
    subject_operations: [],
    subject_operations_not_listed: 0,
    presentation_digest: 'a4'.repeat(32),
    bundle_changes: [],
    bundle_changes_not_listed: 0,
    content_complete: true,
    unavailable_code: null,
    projection_authorizes_approval: false,
  };
}

function recordedReadyReview(subjectOperation, reviewedHead) {
  const review = recordedCurrentReview(subjectOperation, reviewedHead);
  review.bundle_changes = [{
    object_id: 'b1'.repeat(16),
    path_before: '/approval-proof.bin',
    path_after: '/approval-proof.bin',
    effect: 'content-written',
    before: {
      kind: 'binary',
      version_id: 'b2'.repeat(32),
      content_digest: 'b3'.repeat(32),
      byte_length: '8',
      line_count: null,
    },
    after: {
      kind: 'binary',
      version_id: 'b4'.repeat(32),
      content_digest: 'b5'.repeat(32),
      byte_length: '9',
      line_count: null,
    },
    body: 'binary',
    opaque_reason: 'binary',
    verified_text: null,
  }];
  return review;
}

test('source-owned confirmation accepts only an exact mounted generation and fails closed when stale', async () => {
  const document = fakeDocument();
  const projections = [];
  const dismissals = [];
  let browserConfirmations = 0;
  document.addEventListener('mesh:confirmation-projection', (event) => projections.push(event.detail));
  document.addEventListener('mesh:confirmation-dismissed', (event) => dismissals.push(event.detail));
  const invoke = async () => { throw new Error('no native host in confirmation coordinator test'); };
  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.CustomEvent = FakeCustomEvent;
  globalThis.confirm = () => { browserConfirmations += 1; return true; };
  const module = await import(`./app.js?accessible-confirmation=${Date.now()}`);
  // Reproduce a cold-start ordering where the island's one-shot availability event fired before
  // the coordinator listener existed. The durable host marker must still select React instead of
  // falling back to the blocking browser confirmation.
  document.getElementById('confirmation-dialog-next')
    .setAttribute('data-mesh-confirmation-ready', 'true');

  let firstResolution = null;
  const first = module.requestAccessibleConfirmation({
    title: 'Update one proven file?',
    description: 'Update the reviewed saved file in /ordinary/private-copy?',
    confirmLabel: 'Update 1 changed file',
    tone: 'destructive',
    isCurrent: () => true,
  }).then((accepted) => { firstResolution = accepted; return accepted; });
  assert.equal(projections.length, 1);
  assert.equal(projections[0].generation, 1);
  assert.equal(Object.isFrozen(projections[0].confirmation), true);
  assert.equal(browserConfirmations, 0);

  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-mounted', {
    detail: { generation: 1 },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-intent', {
    detail: { generation: 0, intent: { type: 'confirm' } },
  }));
  await Promise.resolve();
  assert.equal(firstResolution, null, 'a stale dialog generation resolved current authority');
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-intent', {
    detail: { generation: 1, intent: { type: 'confirm', destination: '/forged' } },
  }));
  await Promise.resolve();
  assert.equal(firstResolution, null, 'an intent with forged authority fields was accepted');
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-intent', {
    detail: { generation: 1, intent: { type: 'confirm' } },
  }));
  assert.equal(await first, true);
  assert.deepEqual(dismissals.at(-1), { generation: 1 });
  assert.equal(browserConfirmations, 0);

  let current = true;
  const stale = module.requestAccessibleConfirmation({
    title: 'Update one proven file?',
    description: 'Update the reviewed saved file in /ordinary/private-copy?',
    confirmLabel: 'Update 1 changed file',
    tone: 'destructive',
    isCurrent: () => current,
    staleMessage: 'The reviewed destination plan changed while the confirmation was open.',
  });
  assert.equal(projections.at(-1).generation, 2);
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-mounted', {
    detail: { generation: 2 },
  }));
  current = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-intent', {
    detail: { generation: 2, intent: { type: 'confirm' } },
  }));
  assert.equal(await stale, false);
  assert.match(document.getElementById('notice').textContent, /plan changed while the confirmation was open/);
  assert.equal(browserConfirmations, 0);

  const fallback = module.requestAccessibleConfirmation({
    title: 'Create one saved folder?',
    description: 'Create the reviewed folder in /ordinary/private-copy?',
    confirmLabel: 'Create 1 saved folder',
    isCurrent: () => true,
  });
  assert.equal(projections.at(-1).generation, 3);
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-rejected', {
    detail: { generation: 3, reason: 'island rejected projection' },
  }));
  assert.equal(await fallback, true);
  assert.equal(browserConfirmations, 1, 'a rejected source-owned surface did not retain the safe browser fallback');
});

test('new private work never inherits the Approved label from an earlier shared version', async () => {
  const document = fakeDocument();
  let reviewProjection = null;
  document.addEventListener('mesh:review-workbench-projection', (event) => {
    reviewProjection = event.detail;
    document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-mounted', {
      detail: { generation: event.detail.generation, bundle: null },
    }));
  });
  const earlierOperation = '10'.repeat(32);
  const currentOperation = '20'.repeat(32);
  const earlierSharedHead = '30'.repeat(32);
  const workspace = {
    root: '/application/workspace-versions/current.mesh/mounts',
    digest: 'workspace-newer-private-work',
    installation: 'installation-newer-private-work',
    records: 4,
    reviews: 1,
    review_items: [recordedCurrentReview(earlierOperation, earlierSharedHead)],
    review_items_not_listed: 0,
    private_version: { version: 'new-private-version', state: 'working', concurrent_changes: 1 },
    shared_version: earlierSharedHead,
    entries: [{ path: 'notes.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [],
    native_untracked_files: [],
    native_inventory_complete: true,
    workspace_versions: [
      { operation: earlierOperation, ordinal: 1, actor_sequence: '1' },
      { operation: currentOperation, ordinal: 2, actor_sequence: '2' },
    ],
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        auto_opened: true,
        remembered: workspace.root,
        workspaces: [workspace.root],
        workspace_entries: [{ path: workspace.root, export_root: null, project_root: null }],
        active_folder: '/application/native-workspace/current',
        export_root: null,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: true,
        native_folder_path: workspace.root,
        working: false,
      });
    }
    if (command === 'reconcile_managed_workspace_navigation') {
      return JSON.stringify({
        path: '/application/native-workspace/current',
        workspace_root: workspace.root,
        stable: true,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    throw new Error(`unexpected newer-private-work command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?newer-private-than-shared=${Date.now()}`);
  try {
    await waitFor(() => document.serviceState.state === 'ready');
  } catch (error) {
    assert.fail(`${error}: ${document.getElementById('notice').textContent}`);
  }

  assert.equal(
    (document.workspaceCurrent?.current?.state ?? ''),
    'Saved privately',
    'a prior shared version made newer unapproved private work look approved',
  );
  assert.equal(document.workspaceCurrent?.current.state, 'Saved privately');
  assert.match((document.workspaceCurrent?.current?.sharedVersion ?? ''), /^Approved point 1/);
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-available'));
  assert.equal(reviewProjection.state, 'empty');
  assert.equal(reviewProjection.status.title, 'No review is ready for this saved version');
  assert.equal(reviewProjection.authority, undefined);
  assert.equal(reviewProjection.controls.canRecordReview, true);
  assert.equal(
    document.getElementById('review-workbench-next').classList.contains('hidden'),
    false,
    'the React Review page fell back to its indefinite Preparing state',
  );
});

test('unsaved native work overrides approval of the last durable point', async () => {
  const document = fakeDocument();
  const operation = '40'.repeat(32);
  const sharedHead = '50'.repeat(32);
  const workspace = {
    root: '/application/workspace-versions/approved-with-native-work.mesh/mounts',
    digest: 'workspace-approved-with-native-work',
    installation: 'installation-approved-with-native-work',
    records: 4,
    reviews: 1,
    review_items: [recordedCurrentReview(operation, sharedHead)],
    review_items_not_listed: 0,
    private_version: { version: sharedHead, state: 'working', concurrent_changes: 1 },
    shared_version: sharedHead,
    entries: [{ path: 'approved.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [],
    native_untracked_files: ['agent-draft.txt'],
    native_inventory_complete: true,
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  let missingTrackedFile = false;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        auto_opened: false,
        remembered: workspace.root,
        workspaces: [workspace.root],
        workspace_entries: [{ path: workspace.root, export_root: null, project_root: null }],
        active_folder: '/application/native-workspace/current',
        export_root: null,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: true,
        native_folder_path: workspace.root,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'reconcile_managed_workspace_navigation') {
      return JSON.stringify({
        path: '/application/native-workspace/current',
        workspace_root: workspace.root,
        stable: true,
      });
    }
    if (command === 'discover_native_directories') return '[]';
    if (command === 'discover_native_missing_files') {
      return JSON.stringify(missingTrackedFile ? [{
        path: 'approved.txt',
        current_version: sharedHead,
        content_digest: '60'.repeat(32),
        executable: false,
      }] : []);
    }
    throw new Error(`unexpected approved-native-work command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?approved-with-native-work=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.equal(
    (document.workspaceCurrent?.current?.state ?? ''),
    'Working',
    'unsaved agent work inherited the prior durable point\'s Approved label',
  );
  assert.equal(document.workspaceCurrent?.current.state, 'Working');
  assert.match((document.workspaceCurrent?.current?.sharedVersion ?? ''), /^Approved point 1/);

  // Once the daemon's untracked-path hint is gone, a missing tracked file is discoverable only by
  // the native scan and cannot install an editable inspection. It must still invalidate Approved.
  workspace.native_untracked_files = [];
  workspace.entries = [{ path: 'approved.txt', type: 'file' }];
  workspace.file_histories = [{
    path: 'approved.txt',
    object_id: '01APPROVEDNATIVEMISSING0000',
    current: { version_id: sharedHead, manifest_id: '60'.repeat(32) },
    retained_versions: [{ version_id: sharedHead, manifest_id: '60'.repeat(32) }],
  }];
  missingTrackedFile = true;
  await document.emitWorkspaceCurrentIntent('refresh');
  assert.equal((document.workspaceCurrent?.current?.state ?? ''), 'Approved');
  await document.getElementById('scan-files').emit('click');
  assert.match(document.getElementById('folder-change-items').children[0].textContent, /missing tracked file/);
  assert.equal(
    (document.workspaceCurrent?.current?.state ?? ''),
    'Working',
    'a deletion-only native scan retained the prior durable point\'s Approved label',
  );
});

test('startup displays only a canonical exact build revision', async () => {
  const exact = '0123456789abcdef0123456789abcdef01234567';
  for (const [label, recent, expected, expectedTitle] of [
    ['exact', { auto_opened: false, build_revision: exact, build_exact: true }, 'Build 0123456789ab', exact],
    ['development', { auto_opened: false, build_revision: 'development', build_exact: false }, 'Development build', 'does not claim'],
    ['malformed', { auto_opened: false, build_revision: 'not-a-commit', build_exact: true }, 'Build identity unavailable', 'refused'],
  ]) {
    const document = fakeDocument();
    const invoke = async (command, parameters = {}) => {
      if (command === 'recent_workspace_status') return JSON.stringify(recent);
      if (command === 'daemon_call' && parameters.method === 'workspace.state') {
        throw daemonRefusal('no-workspace-open', 'No workspace is open.');
      }
      throw new Error(`unexpected native command: ${command}`);
    };
    globalThis.document = document;
    globalThis.window = { __TAURI__: { core: { invoke } } };
    globalThis.confirm = () => true;
    await import(`./app.js?build-identity-${label}=${Date.now()}`);
    await waitFor(() => document.buildIdentity.label === expected);
    assert.match(document.buildIdentity.title, new RegExp(expectedTitle));
    let replayedIdentity = null;
    document.addEventListener('mesh:build-identity-projection', (event) => {
      replayedIdentity = event.detail;
    });
    document.dispatchEvent(new FakeCustomEvent('mesh:build-identity-available'));
    assert.deepEqual(replayedIdentity, document.buildIdentity);
  }
});

test('Workspaces Refresh recovers an exact build identity after a transient first-launch failure', async () => {
  const document = fakeDocument();
  const exact = '89abcdef0123456789abcdef0123456789abcdef';
  let recentReads = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      recentReads += 1;
      if (recentReads === 1) throw new Error('temporary native status failure');
      return JSON.stringify({ auto_opened: false, build_revision: exact, build_exact: true });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      throw daemonRefusal('no-workspace-open', 'No workspace is open.');
    }
    if (command === 'approval_credential_status') {
      return JSON.stringify({ enrolled: false, available: false, unavailable_reason: 'not configured' });
    }
    throw new Error(`unexpected build recovery command: ${command}`);
  };
  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?build-identity-recovery=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  assert.equal(document.buildIdentity.label, 'Build identity unavailable');

  assert.equal(document.workspaceEntry.entry.canRetry, true);
  await document.emitWorkspaceEntryIntent({ type: 'retry' });
  assert.equal(recentReads, 2);
  assert.equal(document.buildIdentity.label, 'Build 89abcdef0123');
  assert.match(document.buildIdentity.title, new RegExp(exact));
  assert.equal(document.getElementById('notice').classList.contains('error'), false);
  assert.doesNotMatch(document.getElementById('notice').textContent, /temporary native status failure/);
  assert.match(document.getElementById('notice').textContent, /verified that no managed workspace is open/);
});

test('Workspaces Retry stays visible but disabled while its exact read-only refresh is pending', async () => {
  const document = fakeDocument();
  let recentReads = 0;
  let releaseRefresh;
  const pendingRefresh = new Promise((resolve) => { releaseRefresh = resolve; });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      recentReads += 1;
      return recentReads === 1
        ? JSON.stringify({ auto_opened: false })
        : pendingRefresh;
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      throw daemonRefusal('no-workspace-open', 'No workspace is open.');
    }
    if (command === 'approval_credential_status') {
      return JSON.stringify({ enrolled: false, available: false, unavailable_reason: 'not configured' });
    }
    throw new Error(`unexpected pending refresh command: ${command}`);
  };
  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?workspace-entry-refresh-pending=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.emitWorkspaceEntryIntent({ type: 'retry' });
  await waitFor(() => recentReads === 2);
  assert.equal(document.workspaceEntry.entry.canRetry, false);
  await document.emitWorkspaceEntryIntent({ type: 'retry' });
  assert.equal(recentReads, 2, 'a disabled Retry dispatched another overlapping refresh');

  releaseRefresh(JSON.stringify({ auto_opened: false }));
  await waitFor(() => document.workspaceEntry.entry.canRetry === true);
  assert.equal(document.serviceState.state, 'ready');
});

test('source-owned service state projects starting, ready, attention, and recovered ready through Workspaces Retry', async () => {
  const document = fakeDocument();
  const serviceStates = [];
  document.addEventListener('mesh:service-state-projection', (event) => serviceStates.push(event.detail));
  let workspaceReads = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      auto_opened: false,
      build_revision: '89abcdef0123456789abcdef0123456789abcdef',
      build_exact: true,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      workspaceReads += 1;
      if (workspaceReads === 2) throw new Error('workspace verification failed');
      throw daemonRefusal('no-workspace-open', 'No workspace is open.');
    }
    if (command === 'approval_credential_status') {
      return JSON.stringify({ enrolled: false, available: false, unavailable_reason: 'not configured' });
    }
    throw new Error(`unexpected service-state command: ${command}`);
  };
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  globalThis.document = document;
  await import(`./app.js?service-state-retry=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  assert.equal(serviceStates[0]?.state, 'starting');
  assert.equal(serviceStates.at(-1)?.state, 'ready');
  serviceStates.length = 0;
  await document.emitWorkspaceEntryIntent({ type: 'retry' });
  assert.equal(document.serviceState.state, 'attention');
  assert.equal(document.workspaceEntry.entry.retryLabel, 'Retry verification');
  await document.emitWorkspaceEntryIntent({ type: 'retry' });
  assert.equal(document.serviceState.state, 'ready');
  assert.equal(document.getElementById('notice').classList.contains('error'), false);
  assert.doesNotMatch(document.getElementById('notice').textContent, /workspace verification failed/);
  assert.match(document.getElementById('notice').textContent, /verified that no managed workspace is open/);
  const transitions = serviceStates
    .map(({ state }) => state)
    .filter((state, index, states) => index === 0 || state !== states[index - 1]);
  assert.deepEqual(transitions, ['attention', 'ready']);
  assert.equal(serviceStates.at(-1)?.label, 'Local service ready');
});

test('human error prose cannot impersonate the no-workspace refusal', async () => {
  const document = fakeDocument();
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      throw new Error('No workspace recovery database could be verified');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?no-workspace-prose=${Date.now()}`);
  for (let turn = 0; turn < 5; turn += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }

  assert.equal(
    document.serviceState.state === 'ready',
    false,
    'unrelated human prose impersonated the stable no-workspace refusal',
  );
  assert.match(document.getElementById('notice').textContent, /recovery database/);
});

test('a verified live import without an original-folder binding leads back to its exact original', async () => {
  const document = fakeDocument();
  const operation = '71'.repeat(32);
  const workspace = {
    root: '/private/tmp/cli-managed/mounts',
    digest: 'live-cli-import-digest',
    installation: 'live-cli-import-installation',
    records: 3,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: '72'.repeat(32), state: 'working', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'notes.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  let pickerCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'pick_folder') {
      pickerCalls += 1;
      return '/private/tmp/cli-original';
    }
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        auto_opened: false,
        remembered: workspace.root,
        workspaces: [workspace.root],
        workspace_entries: [{
          path: workspace.root,
          export_root: null,
          project_root: null,
        }],
        active_folder: null,
        export_root: null,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: true,
        native_folder_path: workspace.root,
        confirmed_import_receipt: true,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'folder.import.preview') {
      return JSON.stringify(importPreview({
        files: 1,
        directories: 0,
        bytes: 12,
        summary: 'ordinary-folder-confirmation',
        source_scope: 'ordinary-folder',
      }));
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    throw new Error(`unexpected live-import-navigation command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?live-import-navigation=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  await waitFor(() => document.workspaceOverview?.overview.nextActionLabel === 'Connect original folder');

  assert.equal(
    document.workspaceOverview?.overview.nextActionTitle,
    'Connect this workspace to its original folder',
  );
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Connect original folder');
  assert.match(document.workspaceOverview?.overview.nextActionDescription, /restart/);
  assert.match(document.workspaceOverview?.overview.nextActionDescription, /reuse this managed copy/);
  for (const action of [
    'open-folder',
    'copy-working-path',
    'start-codex',
    'open-terminal',
    'copy-agent-path',
    'start-agent-copy',
  ]) {
    assert.equal(document.workspaceCurrentAction(action)?.enabled, false, `${action} must wait for the original-folder binding`);
    const noticeBeforeDisabledAction = document.getElementById('notice').textContent;
    await document.emitWorkspaceCurrentIntent(action);
    assert.equal(document.getElementById('notice').textContent, noticeBeforeDisabledAction);
  }
  assert.equal(document.workspaceCurrent?.current.agentAssigned, false);
  for (const action of ['open-folder', 'start-codex', 'open-terminal', 'copy-working-path', 'copy-agent-path']) {
    assert.equal(
      document.workspaceCurrent?.current.actions.find((candidate) => candidate.id === action)?.enabled,
      false,
      `${action} ignored the source-owned navigation recovery boundary`,
    );
  }
  await document.emitWorkspaceEntryIntent({ type: 'choose-folder' });
  assert.equal(pickerCalls, 1);
  assert.equal(document.importWorkbench?.import.confirmLabel, 'Connect original folder');
  assert.equal(
    document.workspaceOverview?.overview.nextActionTitle,
    'Connect this workspace to its original folder',
  );
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Connect original folder');
  assert.match(document.workspaceOverview?.overview.nextActionDescription, /existing managed workspace/);
  assert.match(document.importWorkbench?.import.scopeNote, /no second managed copy/);
  assert.match(document.getElementById('notice').textContent, /reuse the existing managed workspace/);
});

test('a remembered empty workspace leads back to importing an ordinary folder', async () => {
  const document = fakeDocument();
  const source = '/Users/person/remembered-empty-source';
  const workspace = {
    root: '/private/tmp/mesh-alpha-empty',
    digest: 'empty-workspace-digest',
    installation: 'empty-workspace-installation',
    records: 0,
    private_version: { version: null, state: 'working' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
    native_untracked_files: [],
    workspace_versions: [],
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'pick_folder') return source;
    if (command === 'recent_workspace_status') {
      return JSON.stringify({ auto_opened: true, remembered: workspace.root, workspaces: [workspace.root] });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: false,
        native_folder_path: null,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'daemon_call' && parameters.method === 'folder.import.preview') {
      assert.deepEqual(JSON.parse(parameters.paramsJson), { source });
      return JSON.stringify(importPreview({ files: 0, directories: 0, bytes: 0, summary: 'remembered-empty-summary' }));
    }
    throw new Error(`unexpected empty-workspace command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?remembered-empty=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.equal(document.workspaceEntry?.entry.chooseLabel, 'Choose a folder');
  assert.equal(document.workspaceEntry?.entry.canChoose, true);
  assert.equal(
    document.getElementById('workspace-entry-controls').open,
    true,
    'reopening an empty remembered workspace hid the import journey behind a collapsed control',
  );
  assert.equal(document.getElementById('workspace-entry-summary').textContent, 'Bring in an ordinary project folder');
  assert.equal(document.getElementById('review-workbench-next').classList.contains('hidden'), true);
  assert.equal(document.getElementById('workspace-versions-next').classList.contains('hidden'), true);
  assert.match(document.getElementById('hero-lede').textContent, /no saved work/i);
  assert.match(document.getElementById('hero-lede').textContent, /ordinary project folder/i);
  await document.emitWorkspaceEntryIntent({ type: 'choose-folder' });
  assert.equal(document.importWorkbench?.import.sourcePath, source);
  assert.equal(document.importWorkbench?.import.fileCount, '0');
  assert.equal(document.importWorkbench?.import.confirmLabel, 'Create empty workspace');
  assert.match(document.importWorkbench?.import.scopeNote, /no ordinary project files or folders/i);
  assert.match(document.importWorkbench?.import.scopeNote, /empty workspace/i);
});

test('a zero-history folder with ordinary files offers same-folder import instead of version navigation', async () => {
  const document = fakeDocument();
  const source = '/private/tmp/existing-project';
  let pickerCalls = 0;
  const workspace = {
    root: '/private/tmp/existing-project',
    digest: 'unversioned-workspace-digest',
    installation: 'unversioned-workspace-installation',
    records: 0,
    private_version: { version: null, state: 'working' },
    shared_version: null,
    entries: [],
    conditions: [{
      code: 'unversioned-native-content',
      message: 'This folder contains ordinary files or folders that are not saved in Mesh history.',
      related: [],
      recoverable: true,
    }],
    not_yet: [],
    file_histories: [],
    native_untracked_files: [],
    workspace_versions: [],
  };
  const importedWorkspace = {
    ...workspace,
    root: '/application/workspace-versions/same-folder.mesh/mounts',
    digest: 'same-folder-imported-digest',
    installation: 'same-folder-imported-installation',
    records: 4,
    private_version: { version: 'same-folder-version', state: 'working', concurrent_changes: 1 },
    entries: [{ path: 'docs', type: 'folder' }, { path: 'notes.txt', type: 'file' }],
    conditions: [],
    workspace_versions: [{ operation: 'same-folder-operation', ordinal: 1, actor_sequence: '1' }],
  };
  let imported = false;
  let importParameters = null;
  let finishImport = null;
  const importReply = new Promise((resolve) => { finishImport = resolve; });
  const confirmationPrompts = [];
  const invoke = async (command, parameters = {}) => {
    if (command === 'pick_folder') {
      pickerCalls += 1;
      return source;
    }
    if (command === 'recent_workspace_status') {
      return JSON.stringify({ auto_opened: true, remembered: workspace.root, workspaces: [workspace.root] });
    }
    if (command === 'managed_checkpoint_state') {
      const current = imported ? importedWorkspace : workspace;
      return JSON.stringify({
        root: current.root,
        workspace_digest: current.digest,
        workspace_installation: current.installation,
        native_folder: imported,
        native_folder_path: imported ? current.root : null,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(imported ? importedWorkspace : workspace);
    }
    if (command === 'daemon_call' && parameters.method === 'folder.import.preview') {
      assert.deepEqual(JSON.parse(parameters.paramsJson), { source });
      return JSON.stringify({
        files: 3,
        directories: 1,
        bytes: 42,
        summary: 'same-folder-import',
        source_scope: 'open-zero-history-workspace',
        file_entries: [
          { path: 'README.md', bytes: '11', executable: false },
          { path: 'run.sh', bytes: '17', executable: true },
          { path: 'src/main.rs', bytes: '14', executable: false },
        ],
        files_not_listed: 0,
      });
    }
    if (command === 'discover_native_directories') {
      return JSON.stringify([{ path: 'docs', installation: workspace.installation }]);
    }
    if (command === 'discover_native_missing_files') return '[]';
    if (command === 'import_managed_workspace') {
      importParameters = parameters;
      await importReply;
      imported = true;
      return JSON.stringify({
        workspace: importedWorkspace,
        materialized_entries: 4,
        recovered_after_interruption: false,
        navigation: {
          remembered: importedWorkspace.root,
          workspaces: [importedWorkspace.root],
          active_folder: '/application/native-workspace/current',
          export_root: source,
          warning: null,
        },
      });
    }
    if (command === 'reveal_managed_workspace') {
      return JSON.stringify({
        path: '/application/native-workspace/current',
        workspace_root: importedWorkspace.root,
        stable: true,
        native_folder: true,
      });
    }
    throw new Error(`unexpected unversioned-workspace command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = (message) => {
    confirmationPrompts.push(message);
    return true;
  };
  await import(`./app.js?unversioned-workspace=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.equal(document.workspaceEntry?.entry.chooseLabel, 'Preview this folder');
  assert.equal(document.workspaceEntry?.entry.canChoose, true);
  assert.equal(document.getElementById('workspace-entry-controls').open, true);
  assert.equal(document.getElementById('workspace-entry-summary').textContent, 'Bring in an ordinary project folder');
  assert.equal(document.getElementById('review-workbench-next').classList.contains('hidden'), true);
  assert.match(document.getElementById('hero-lede').textContent, /ordinary files.*not saved in Mesh/i);
  assert.match(document.getElementById('hero-lede').textContent, /Preview this folder/i);
  assert.equal(document.getElementById('workspace-files-next').classList.contains('hidden'), true);
  assert.equal(document.getElementById('scan-files').disabled, true);
  assert.equal(!document.workspaceCurrentAction('update-destination')?.enabled, true);
  assert.equal(!document.workspaceCurrentAction('rollback')?.enabled, true);
  assert.equal(document.workspaceWork, null, 'an unsaved ordinary folder received a Files authority projection');
  document.getElementById('manage-path').value = 'must-not-write.txt';
  await document.getElementById('manage-path').emit('input');
  assert.equal(
    document.getElementById('create-text-entry').disabled,
    true,
    'typing a path enabled a managed write against the pre-import original folder',
  );
  const noticeBeforeDisabledCreate = document.getElementById('notice').textContent;
  await document.getElementById('create-text-entry').emit('click');
  assert.equal(
    document.getElementById('notice').textContent,
    noticeBeforeDisabledCreate,
    'a disabled React Files action reached coordinator authority',
  );
  await document.emitWorkspaceEntryIntent({ type: 'choose-folder' });
  assert.equal(pickerCalls, 0, 'Mesh asked the person to reselect the exact folder it already verified');
  assert.equal(document.importWorkbench?.import.sourcePath, source);
  assert.equal(document.importWorkbench?.import.fileCount, '3');
  assert.equal(document.importWorkbench?.import.confirmLabel, 'Create workspace and open folder');
  assert.match(document.importWorkbench?.import.scopeNote, /ordinary project files/i);
  assert.match(document.importWorkbench?.import.scopeNote, /private history, databases, and content-store folders stay in the original/i);
  assert.deepEqual(document.importWorkbench?.import.files, [
    'README.md · 11 bytes',
    'run.sh · 17 bytes · executable',
    'src/main.rs · 14 bytes',
  ]);

  const importAttempt = document.emitImportWorkbenchIntent({ type: 'confirm-import' });
  await waitFor(() => importParameters !== null);

  assert.equal(document.importWorkbench?.import.busy, true);
  assert.equal(document.importWorkbench?.import.canConfirm, false);
  assert.equal(document.importWorkbench?.import.confirmLabel, 'Creating private workspace…');
  assert.match(document.getElementById('notice').textContent, /large projects can take several minutes/i);
  assert.match(document.getElementById('notice').textContent, /original stays unchanged/i);
  finishImport();
  await importAttempt;

  assert.deepEqual(confirmationPrompts, [], 'protected import falsely warned that its exact preview would be left behind');
  assert.deepEqual(importParameters, {
    source,
    summary: 'same-folder-import',
    destination: null,
  });
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), importedWorkspace.root);
  assert.equal(document.destinationField('destination').value, source);
  assert.equal(document.importWorkbench?.import.phase, 'select');
  assert.equal(document.importWorkbench?.import.canConfirm, false);
  assert.equal(document.importWorkbench?.import.sourcePath, '');
  assert.equal(document.getElementById('choose-source').classList.contains('hidden'), false);
  assert.equal(document.getElementById('choose-source').textContent, 'Import another folder');
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Start Codex on this saved version');
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Start Codex on this version');
  assert.equal(document.getElementById('manage-path').disabled, false);
  assert.equal(document.getElementById('scan-files').disabled, false);
  assert.equal(!document.workspaceCurrentAction('update-destination')?.enabled, false);
  assert.equal(!document.workspaceCurrentAction('rollback')?.enabled, false);

  await document.emitWorkspaceEntryIntent({ type: 'choose-folder' });
  assert.equal(pickerCalls, 1, 'the compact returning-workspace control did not open the folder chooser');
  assert.equal(document.importWorkbench?.import.sourcePath, source);
  assert.equal(document.importWorkbench?.import.phase, 'review');
});

test('the direct same-folder preview refuses when the daemon no longer holds its protected scope', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/private/tmp/replaced-zero-history',
    digest: 'replaced-zero-history-digest',
    installation: 'replaced-zero-history-installation',
    records: 0,
    private_version: { version: null, state: 'working' },
    shared_version: null,
    entries: [],
    conditions: [{
      code: 'unversioned-native-content',
      message: 'This folder contains ordinary files or folders that are not saved in Mesh history.',
      related: [],
      recoverable: true,
    }],
    not_yet: [],
    file_histories: [],
    native_untracked_files: [],
    workspace_versions: [],
  };
  let pickerCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'pick_folder') {
      pickerCalls += 1;
      return workspace.root;
    }
    if (command === 'recent_workspace_status') {
      return JSON.stringify({ auto_opened: true, remembered: workspace.root, workspaces: [workspace.root] });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: false,
        native_folder_path: null,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'daemon_call' && parameters.method === 'folder.import.preview') {
      assert.deepEqual(JSON.parse(parameters.paramsJson), { source: workspace.root });
      return JSON.stringify(importPreview({
        files: 9,
        directories: 4,
        bytes: 2048,
        summary: 'wrong-import-scope',
        source_scope: 'ordinary-folder',
      }));
    }
    throw new Error(`unexpected replaced-scope command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?replaced-same-folder-scope=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.emitWorkspaceEntryIntent({ type: 'choose-folder' });

  assert.equal(pickerCalls, 0);
  assert.equal(document.importWorkbench?.import.phase, 'select');
  assert.equal(document.importWorkbench?.import.canConfirm, false);
  assert.match(document.getElementById('notice').textContent, /no longer holds this exact zero-history folder/i);
  assert.match(document.getElementById('notice').textContent, /nothing was copied/i);
});

test('an unknown daemon import scope cannot enable confirmation', async () => {
  const document = fakeDocument();
  const source = '/Users/person/project';
  const invoke = async (command, parameters = {}) => {
    if (command === 'pick_folder') return source;
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      throw daemonRefusal('no-workspace-open', 'No workspace is open.');
    }
    if (command === 'daemon_call' && parameters.method === 'folder.import.preview') {
      return JSON.stringify(importPreview({
        files: 1,
        directories: 0,
        bytes: 7,
        summary: 'unknown-scope-preview',
        source_scope: 'caller-selected-private-filter',
      }));
    }
    throw new Error(`unexpected import-scope command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?unknown-import-scope=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  await document.emitWorkspaceEntryIntent({ type: 'choose-folder' });
  await waitFor(() => /unknown source scope/i.test(document.getElementById('notice').textContent));

  assert.equal(document.importWorkbench?.import.phase, 'select');
  assert.equal(document.importWorkbench?.import.canConfirm, false);
  assert.match(document.getElementById('notice').textContent, /unknown source scope/i);
});

test('an older opaque import preview cannot enable confirmation', async () => {
  const document = fakeDocument();
  const source = '/Users/person/project';
  const invoke = async (command, parameters = {}) => {
    if (command === 'pick_folder') return source;
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      throw daemonRefusal('no-workspace-open', 'No workspace is open.');
    }
    if (command === 'daemon_call' && parameters.method === 'folder.import.preview') {
      return JSON.stringify({
        files: 1,
        directories: 0,
        bytes: 7,
        summary: 'opaque-preview',
        source_scope: 'ordinary-folder',
      });
    }
    throw new Error(`unexpected partial-preview command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?opaque-import-preview=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  await document.emitWorkspaceEntryIntent({ type: 'choose-folder' });
  await waitFor(() => /incomplete import file preview/i.test(document.getElementById('notice').textContent));

  assert.equal(document.importWorkbench?.import.phase, 'select');
  assert.equal(document.importWorkbench?.import.canConfirm, false);
  assert.match(document.getElementById('notice').textContent, /incomplete import file preview/i);
  assert.match(document.getElementById('notice').textContent, /nothing was copied/i);
});

test('opening the working folder uses the exact verified workspace binding', async () => {
  const document = fakeDocument();
  const nativeFolderPath = '/private/managed/native-work';
  const stableAgentReference = '/.vol/16777229/123456';
  const workspace = {
    root: '/managed/native-work',
    digest: 'workspace-native-work',
    installation: 'installation-native-work',
    records: 1,
    private_version: { version: 'version-native-work' },
    shared_version: null,
    entries: [{ path: 'agent-result.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'agent-result.txt',
      object_id: 'object-agent-result',
      current: { version_id: 'version-agent-result', manifest_id: 'manifest-agent-result' },
      retained_versions: [{ version_id: 'version-agent-result', manifest_id: 'manifest-agent-result' }],
    }],
    support_bundle: {
      schema: 'mesh-support-bundle/v1',
      producer: { component: 'mesh-daemon', version: '0.1.0' },
      workspace_correlation: `blake3:${'ab'.repeat(32)}`,
      included: ['crash-diagnostics'],
      excluded: ['configuration', 'event-ledger', 'file-content', 'key-material', 'raw-paths'],
      'crash-diagnostics': {
        section: 'crash-diagnostics',
        serving: true,
        severity: 'routine',
        saved_records: 1,
        boundary_bytes: 128,
        unfinished_bytes: 0,
        checkpoint_state_available: true,
        meaningful_checkpoint_through: '1',
        recovery_preserved_through: '1',
        open_activity_from: null,
        open_activity_through: null,
        elapsed_ms: 4,
        sentence: 'Mesh started and your workspace is up to date. 1 saved changes were read back.',
      },
    },
  };
  let revealParameters = null;
  const workspaceEntryOpenParameters = [];
  let workspaceEntryOpenFailure = false;
  let workspaceEntryOpenGate = null;
  let codexParameters = null;
  let codexCalls = 0;
  let terminalParameters = null;
  let terminalCalls = 0;
  let copiedAgentParameters = null;
  let copiedAgentCalls = 0;
  let finishParameters = null;
  let finishCalls = 0;
  let agentHandoffInstallation = null;
  let agentHandoffGeneration = 0;
  let agentFinished = false;
  let agentWorkingChange = false;
  let savedAgentRevision = false;
  let privateSaveCalls = 0;
  let structuralAgentChange = false;
  let loseFinishReply = false;
  let poisonSaveControlOnRelease = false;
  let poisonedLifecycleSaveControlReads = 0;
  const retainedSaveControl = document.getElementById('save-file');
  let retainedSaveControlDisabled = retainedSaveControl.disabled;
  Object.defineProperty(retainedSaveControl, 'disabled', {
    configurable: true,
    get() {
      if (poisonSaveControlOnRelease) {
        poisonedLifecycleSaveControlReads += 1;
        return false;
      }
      return retainedSaveControlDisabled;
    },
    set(value) {
      retainedSaveControlDisabled = value;
    },
  });
  let lateAgentWriteOnRelease = false;
  let mismatchedCodexLaunchReply = false;
  const copiedPaths = [];
  let reconciledWorkingPath = '/application/native-workspace/current';
  let clipboardFailure = false;
  let inspectionFailure = false;
  let ordinaryInspectionCalls = 0;
  let holdOrdinaryInspection = false;
  let releaseOrdinaryInspection;
  let signalOrdinaryInspection;
  let reacquireAfterPrivateSave = false;
  let reacquireOnNextRecentRead = false;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      if (reacquireOnNextRecentRead) {
        reacquireOnNextRecentRead = false;
        agentHandoffInstallation = workspace.installation;
        agentHandoffGeneration += 1;
      }
      return JSON.stringify({
        auto_opened: false,
        active_folder: '/application/native-workspace/current',
        remembered: workspace.root,
        workspaces: [workspace.root],
        workspace_entries: [{
          path: workspace.root,
          export_root: null,
          project_root: null,
          agent_handoff_installation: agentHandoffInstallation,
          agent_handoff_generation: testAgentHandoffGeneration(agentHandoffGeneration),
        }],
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: true,
        native_folder_path: nativeFolderPath,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'reveal_managed_workspace') {
      revealParameters = parameters;
      return JSON.stringify({
        path: '/application/native-workspace/current',
        workspace_root: workspace.root,
        stable: true,
        native_folder: true,
      });
    }
    if (command === 'open_managed_workspace_entry') {
      workspaceEntryOpenParameters.push(parameters);
      if (workspaceEntryOpenGate) await workspaceEntryOpenGate;
      if (workspaceEntryOpenFailure) throw new Error('Finder refused the exact workspace entry');
      return JSON.stringify({
        schema: 'mesh.workspace-entry-open/v1',
        action: parameters.action,
        entry_kind: parameters.entryKind,
        opened: true,
      });
    }
    if (command === 'open_managed_workspace_in_codex') {
      if (parameters.confirmedReopen) {
        if (parameters.expectedAgentHandoffGeneration !== testAgentHandoffGeneration(agentHandoffGeneration)) {
          throw new Error('agent folder assignment changed; refresh and confirm the current assignment');
        }
      } else if (parameters.expectedAgentHandoffGeneration !== null) {
        throw new Error('a first agent handoff cannot carry reopen authority');
      }
      codexCalls += 1;
      codexParameters = parameters;
      agentHandoffInstallation = workspace.installation;
      agentHandoffGeneration += 1;
      return JSON.stringify({
        path: mismatchedCodexLaunchReply ? '/different/workspace' : workspace.root,
        workspace_installation: workspace.installation,
        fixed_workspace_path: true,
        agent_handoff_recorded: true,
        agent_handoff_generation: testAgentHandoffGeneration(agentHandoffGeneration),
        agent: 'Codex',
      });
    }
    if (command === 'open_managed_workspace_in_terminal') {
      if (parameters.confirmedReopen) {
        if (parameters.expectedAgentHandoffGeneration !== testAgentHandoffGeneration(agentHandoffGeneration)) {
          throw new Error('agent folder assignment changed; refresh and confirm the current assignment');
        }
      } else if (parameters.expectedAgentHandoffGeneration !== null) {
        throw new Error('a first agent handoff cannot carry reopen authority');
      }
      terminalCalls += 1;
      terminalParameters = parameters;
      agentHandoffInstallation = workspace.installation;
      agentHandoffGeneration += 1;
      return JSON.stringify({
        path: workspace.root,
        workspace_installation: workspace.installation,
        fixed_workspace_path: true,
        agent_handoff_recorded: true,
        agent_handoff_generation: testAgentHandoffGeneration(agentHandoffGeneration),
        agent: 'Terminal',
      });
    }
    if (command === 'prepare_managed_workspace_agent_path') {
      if (parameters.confirmedReopen || parameters.expectedAgentHandoffGeneration !== null) {
        throw new Error('manual path preparation unexpectedly carried reopen authority');
      }
      copiedAgentCalls += 1;
      copiedAgentParameters = parameters;
      agentHandoffInstallation = workspace.installation;
      agentHandoffGeneration += 1;
      return JSON.stringify({
        path: stableAgentReference,
        display_path: nativeFolderPath,
        workspace_installation: workspace.installation,
        agent_handoff_recorded: true,
        agent_handoff_generation: testAgentHandoffGeneration(agentHandoffGeneration),
        git_context: 'ready',
        git_context_warning: null,
      });
    }
    if (command === 'finish_managed_workspace_agent_handoff') {
      finishCalls += 1;
      finishParameters = parameters;
      agentHandoffInstallation = null;
      agentFinished = true;
      if (lateAgentWriteOnRelease) {
        // Reproduce a process completing its final write after Mesh's custody scan but before the
        // durable release reply. The finish flow must perform a second complete inspection.
        lateAgentWriteOnRelease = false;
        savedAgentRevision = false;
        agentWorkingChange = true;
      }
      if (loseFinishReply) {
        loseFinishReply = false;
        throw new Error('agent handoff reply was lost after the durable release');
      }
      return JSON.stringify({
        path: workspace.root,
        workspace_installation: workspace.installation,
        cleared: true,
      });
    }
    if (command === 'inspect_agent_finish_preflight') {
      if (inspectionFailure) throw new Error('could not inspect agent-result.txt');
      if (parameters.expectedAgentHandoffGeneration !== testAgentHandoffGeneration(agentHandoffGeneration)) {
        throw new Error('agent assignment changed during inspection');
      }
      assert.deepEqual(parameters, {
        expectedWorkspaceRoot: workspace.root,
        expectedWorkspaceDigest: workspace.digest,
        expectedWorkspaceInstallation: workspace.installation,
        expectedAgentHandoffGeneration: testAgentHandoffGeneration(agentHandoffGeneration),
      });
      return JSON.stringify({
        schema: 'mesh.agent-finish-preflight/v1',
        workspace_root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        agent_handoff_generation: testAgentHandoffGeneration(agentHandoffGeneration),
        managed_files: structuralAgentChange ? [] : [{
          path: 'agent-result.txt',
          current_version: 'version-agent-result',
          byte_count: agentFinished ? 17 : agentWorkingChange ? 19 : 11,
          content_digest: 'ab'.repeat(32),
          executable: false,
          modified_from_current_version: agentWorkingChange || (agentFinished && !savedAgentRevision),
        }],
        native_files: [{
          path: 'agent-created-after-projection.txt',
          byte_count: 7,
          content_digest: 'bb'.repeat(32),
          executable: false,
        }],
        native_directories: [],
        missing_files: structuralAgentChange ? [{
          path: 'agent-result.txt',
          current_version: 'version-agent-result',
          content_digest: 'ac'.repeat(32),
          executable: false,
        }] : [],
        unsupported_entries: [],
      });
    }
    if (command === 'reconcile_managed_workspace_navigation') {
      return JSON.stringify({
        path: reconciledWorkingPath,
        workspace_root: workspace.root,
        stable: true,
        native_folder: true,
      });
    }
    if (command === 'discover_native_directories') {
      ordinaryInspectionCalls += 1;
      if (agentHandoffInstallation) throw new Error('workspace is assigned to an agent');
      return '[]';
    }
    if (command === 'discover_native_missing_files') {
      ordinaryInspectionCalls += 1;
      if (agentHandoffInstallation) throw new Error('workspace is assigned to an agent');
      return JSON.stringify(structuralAgentChange ? [{
      path: 'agent-result.txt',
      current_version: 'version-agent-result',
      content_digest: 'digest-saved-work',
      executable: false,
      }] : []);
    }
    if (command === 'inspect_managed_file') {
      ordinaryInspectionCalls += 1;
      if (agentHandoffInstallation) throw new Error('workspace is assigned to an agent');
      assert.equal(parameters.relativePath, 'agent-result.txt');
      if (inspectionFailure) throw new Error('agent result became unreadable');
      if (holdOrdinaryInspection) {
        holdOrdinaryInspection = false;
        const gate = new Promise((resolve) => { releaseOrdinaryInspection = resolve; });
        signalOrdinaryInspection();
        await gate;
      }
      return JSON.stringify({
        path: 'agent-result.txt',
        text: agentFinished
          ? 'final agent work\n'
          : agentWorkingChange
            ? 'partial agent work\n'
            : 'saved work\n',
        text_editable: true,
        modified_from_current_version: agentWorkingChange || (agentFinished && !savedAgentRevision),
        current_version: 'version-agent-result',
        byte_count: agentFinished ? 17 : agentWorkingChange ? 19 : 11,
        content_digest: agentFinished
          ? 'digest-final-agent-work'
          : agentWorkingChange
            ? 'digest-partial-agent-work'
            : 'digest-saved-work',
        executable: false,
      });
    }
    if (command === 'save_managed_private') {
      privateSaveCalls += 1;
      assert.equal(parameters.relativePath, 'agent-result.txt');
      assert.equal(parameters.expectedContentDigest, 'digest-final-agent-work');
      savedAgentRevision = true;
      agentWorkingChange = false;
      workspace.digest = `workspace-native-work-saved-${privateSaveCalls}`;
      workspace.records += 1;
      if (reacquireAfterPrivateSave) {
        reacquireAfterPrivateSave = false;
        reacquireOnNextRecentRead = true;
      }
      return JSON.stringify({
        path: 'agent-result.txt',
        version: `version-agent-result-saved-${privateSaveCalls}`,
        manifest: `manifest-agent-result-saved-${privateSaveCalls}`,
        changeset: `changeset-agent-result-saved-${privateSaveCalls}`,
        stable_after_idle: true,
        saved_privately: true,
        author_authenticated: true,
      });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = {
    __TAURI__: { core: { invoke } },
    navigator: {
      clipboard: {
        async writeText(path) {
          if (clipboardFailure) throw new Error('clipboard denied');
          copiedPaths.push(path);
        },
      },
    },
  };
  globalThis.CustomEvent = FakeCustomEvent;
  globalThis.confirm = () => true;
  await import(`./app.js?native-folder=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  await waitFor(() => document.workspaceOverview !== null);
  assert.equal(
    document.getElementById('workspace-overview-next').classList.contains('hidden'),
    true,
    'a first Overview projection marked its assigned host ready before React committed it',
  );

  assert.equal(document.workspaceCurrentAction('copy-diagnostics')?.enabled, true);
  await document.emitWorkspaceCurrentIntent('copy-diagnostics');
  const diagnostic = JSON.parse(copiedPaths.at(-1));
  assert.equal(diagnostic.schema, 'mesh-support-bundle/v1');
  assert.deepEqual(diagnostic.excluded, ['configuration', 'event-ledger', 'file-content', 'key-material', 'raw-paths']);
  assert.deepEqual(Object.keys(diagnostic), [
    'schema',
    'producer',
    'workspace_correlation',
    'included',
    'excluded',
    'crash-diagnostics',
  ]);
  assert.doesNotMatch(copiedPaths.at(-1), /native-work|agent-result\.txt/u);
  assert.match(document.getElementById('notice').textContent, /excludes file contents, paths, configuration, event history, and keys/);
  workspace.support_bundle = { ...workspace.support_bundle, raw_path: '/secret/project' };
  const clipboardWritesBeforeMalformedBundle = copiedPaths.length;
  await document.emitWorkspaceCurrentIntent('refresh');
  assert.equal(document.workspaceCurrentAction('copy-diagnostics')?.enabled, false, 'an extended diagnostic object stayed copyable');
  await document.emitWorkspaceCurrentIntent('copy-diagnostics');
  assert.equal(copiedPaths.length, clipboardWritesBeforeMalformedBundle, 'an extended diagnostic object reached the clipboard');

  assert.equal(document.getElementById('hero-eyebrow').textContent, 'NATIVE WORKSPACE');
  assert.equal(document.getElementById('hero-title').textContent, 'Work in your folder. Mesh remembers.');
  assert.match(document.getElementById('hero-lede').textContent, /stable folder for Finder and editors/);
  assert.match(document.getElementById('hero-lede').textContent, /independent real folder for Terminal and agents/);
  assert.match((document.workspaceCurrent?.current?.nativeFolderHint ?? ''), /Automatic private save is unavailable/);
  assert.match((document.workspaceCurrent?.current?.nativeFolderHint ?? ''), /Review-first mode remains active/);
  assert.doesNotMatch((document.workspaceCurrent?.current?.nativeFolderHint ?? ''), /saving remains explicit/);
  assert.equal(document.getElementById('workspace-entry-controls').open, false);
  assert.equal(document.getElementById('workspace-entry-summary').textContent, 'Import or switch to another workspace');
  assert.equal(!document.workspaceCurrentAction('open-folder')?.enabled, false);
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Continue in the native folder');
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Open working folder');
  await document.emitWorkspaceOverviewIntent('recommended');
  await waitFor(() => revealParameters !== null);
  assert.deepEqual(revealParameters, {
    expectedWorkspaceRoot: workspace.root,
    expectedWorkspaceDigest: workspace.digest,
    expectedWorkspaceInstallation: workspace.installation,
  });
  assert.match(document.getElementById('notice').textContent, /Work there normally/);
  revealParameters = null;
  await document.emitWorkspaceWorkIntent({ type: 'activate', action: 'open-workspace-folder' });
  await waitFor(() => revealParameters !== null);
  assert.match(document.getElementById('notice').textContent, /Opened the current workspace folder in Finder/);
  await document.emitWorkspaceWorkIntent({
    type: 'set-field', field: 'selectedEntry', value: 'agent-result.txt',
  });
  let releaseWorkspaceEntryOpen;
  workspaceEntryOpenGate = new Promise((resolve) => { releaseWorkspaceEntryOpen = resolve; });
  const openingWorkspaceEntry = document.emitWorkspaceWorkIntent({ type: 'activate', action: 'open-entry' });
  await waitFor(() => workspaceEntryOpenParameters.length === 1);
  assert.equal(
    document.workspaceWork.workbench.actions.find((action) => action.id === 'open-entry')?.enabled,
    false,
    'Files kept exact-entry launch authority enabled while the native handoff was unresolved',
  );
  releaseWorkspaceEntryOpen();
  workspaceEntryOpenGate = null;
  await openingWorkspaceEntry;
  await waitFor(() => /Opened agent-result\.txt with its default application/.test(document.getElementById('notice').textContent));
  assert.deepEqual(workspaceEntryOpenParameters.at(-1), {
    expectedWorkspaceRoot: workspace.root,
    expectedWorkspaceDigest: workspace.digest,
    expectedWorkspaceInstallation: workspace.installation,
    expectedAgentHandoffGeneration: null,
    relativePath: 'agent-result.txt',
    entryKind: 'file',
    action: 'open-entry',
  });
  assert.match(document.getElementById('notice').textContent, /Opened agent-result\.txt with its default application/);
  assert.equal(
    document.workspaceWork.workbench.actions.find((action) => action.id === 'open-entry')?.enabled,
    true,
    'Files did not restore exact-entry launch authority after native completion',
  );
  workspaceEntryOpenFailure = true;
  await document.emitWorkspaceWorkIntent({ type: 'activate', action: 'reveal-entry' });
  assert.match(document.getElementById('notice').textContent, /File action unavailable: Finder refused the exact workspace entry/);
  workspaceEntryOpenFailure = false;
  assert.equal(!document.workspaceCurrentAction('start-codex')?.enabled, false);
  assert.equal(!document.workspaceCurrentAction('open-terminal')?.enabled, false);
  assert.equal(!document.workspaceCurrentAction('copy-agent-path')?.enabled, false);
  document.getElementById('edit-file').value = 'agent-result.txt';
  await document.getElementById('edit-file').emit('change');
  await document.getElementById('load-file').emit('click');
  const draftEditor = document.getElementById('file-editor');
  draftEditor.value = 'window-only work that the agent cannot see\n';
  await draftEditor.emit('input');

  assert.equal(!document.workspaceCurrentAction('start-codex')?.enabled, true);
  assert.equal(!document.workspaceCurrentAction('open-terminal')?.enabled, true);
  assert.equal(!document.workspaceCurrentAction('copy-agent-path')?.enabled, true);
  assert.equal(document.workspaceCurrent?.current.state, 'Working');
  for (const action of ['start-codex', 'open-terminal', 'copy-agent-path']) {
    assert.equal(
      document.workspaceCurrent?.current.actions.find((candidate) => candidate.id === action)?.enabled,
      false,
      `${action} ignored the source-owned editor draft boundary`,
    );
  }

  const noticeBeforeDisabledDraftActions = document.getElementById('notice').textContent;
  await document.emitWorkspaceCurrentIntent('start-codex');
  await document.emitWorkspaceCurrentIntent('open-terminal');
  await document.emitWorkspaceCurrentIntent('copy-agent-path');
  assert.equal(codexCalls, 0);
  assert.equal(terminalCalls, 0);
  assert.equal(copiedAgentCalls, 0);
  assert.equal(document.getElementById('notice').textContent, noticeBeforeDisabledDraftActions);

  draftEditor.value = 'saved work\n';
  await draftEditor.emit('input');
  assert.equal(!document.workspaceCurrentAction('start-codex')?.enabled, false);
  assert.equal(!document.workspaceCurrentAction('open-terminal')?.enabled, false);
  assert.equal(!document.workspaceCurrentAction('copy-agent-path')?.enabled, false);

  await document.emitWorkspaceCurrentIntent('open-terminal');
  await waitFor(() => terminalParameters !== null);
  assert.deepEqual(terminalParameters, {
    expectedWorkspaceRoot: workspace.root,
    expectedWorkspaceDigest: workspace.digest,
    expectedWorkspaceInstallation: workspace.installation,
    confirmedReopen: false,
    expectedAgentHandoffGeneration: null,
  });
  assert.match(document.getElementById('notice').textContent, /Start your local agent there/);
  assert.match(document.getElementById('notice').textContent, /real folder stays fixed/);
  assert.equal((document.workspaceCurrentAction('open-terminal')?.label ?? ''), 'Reopen agent terminal');
  assert.equal((document.workspaceCurrentAction('start-codex')?.label ?? ''), 'Reopen assigned Codex folder');
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Agent folder is assigned');
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Finish agent handoff');
  assert.match(document.workspaceOverview?.overview.nextActionDescription, /every process using it stops/);
  document.getElementById('manage-path').value = 'stale-ui-write.txt';
  await document.getElementById('manage-path').emit('input');
  assert.equal(document.getElementById('create-text-entry').disabled, true);
  assert.match(document.getElementById('management-status').textContent, /Finish agent handoff performs the exact complete inspection/);
  const assignedFile = document.getElementById('edit-file');
  assignedFile.value = 'agent-result.txt';
  await assignedFile.emit('change');
  const inspectionsBeforeAssignedLoad = ordinaryInspectionCalls;
  assert.equal(assignedFile.disabled, true);
  assert.equal(document.getElementById('load-file').disabled, true);
  await document.getElementById('load-file').emit('click');
  assert.equal(
    ordinaryInspectionCalls,
    inspectionsBeforeAssignedLoad,
    'an assigned folder entered ordinary single-file inspection',
  );
  assert.equal(document.destinationField('destination').disabled, true);
  assert.equal(document.destinationActionControl('preview-single').disabled, true);
  assert.equal(document.destinationActionControl('confirm-single').disabled, true);
  assert.match(document.destinationHint.textContent, /Export is paused/);
  assert.match(document.destinationHint.textContent, /Finish agent handoff/);
  await document.getElementById('create-text-entry').emit('click');
  assert.equal(document.workspaceCurrentAction('update-destination')?.enabled, false);
  const noticeBeforeDisabledDestination = document.getElementById('notice').textContent;
  await document.emitWorkspaceCurrentIntent('update-destination');
  assert.equal(document.getElementById('notice').textContent, noticeBeforeDisabledDestination);
  workspace.private_version.concurrent_changes = 1;
  workspace.workspace_versions = [{
    operation: 'operation-agent-baseline',
    ordinal: 1,
    actor_sequence: '1',
  }];
  await document.emitWorkspaceCurrentIntent('refresh');
  assert.equal(document.reviewPage.controls.canRecordReview, false);
  assert.match(document.reviewPage.controls.recordReviewReason, /Finish agent handoff/);
  let finishPrompt = null;
  globalThis.confirm = (message) => {
    finishPrompt = message;
    return false;
  };
  await document.emitWorkspaceCurrentIntent('finish-agent');
  await waitFor(() => finishPrompt !== null);
  assert.match(finishPrompt, /only after every Codex session/);
  assert.equal(finishParameters, null, 'cancelling the React confirmation released agent custody');
  globalThis.confirm = () => true;
  agentWorkingChange = true;
  const inspectionsBeforeAssignedScan = ordinaryInspectionCalls;
  const assignedScan = document.workspaceWork.workbench.actions.find((action) => action.id === 'scan-files');
  const assignedSaveAll = document.workspaceWork.workbench.actions.find((action) => action.id === 'save-all-private');
  assert.equal(assignedScan.enabled, false);
  assert.equal(assignedSaveAll.enabled, false);
  const noticeBeforeDisabledWork = document.getElementById('notice').textContent;
  await document.emitWorkspaceWorkIntent({ type: 'activate', action: 'scan-files' });
  assert.equal(
    ordinaryInspectionCalls,
    inspectionsBeforeAssignedScan,
    'an assigned folder entered the native command that must refuse ordinary inspection',
  );
  assert.equal(document.workspaceWork.workbench.changes.queue.length, 0);
  assert.equal(document.getElementById('notice').textContent, noticeBeforeDisabledWork);
  await document.emitWorkspaceWorkIntent({ type: 'activate', action: 'save-all-private' });
  assert.equal(document.getElementById('notice').textContent, noticeBeforeDisabledWork);
  assert.equal(
    document.workspaceOverview?.overview.nextActionTitle,
    'Agent folder is assigned',
    'partial agent bytes displaced the custody-first next action',
  );
  assert.equal(!document.workspaceCurrentAction('rollback')?.enabled, true);
  await document.emitWorkspaceCurrentIntent('rollback');
  assert.equal(document.getElementById('notice').textContent, noticeBeforeDisabledWork);
  const reopenProjections = [];
  let browserReopenConfirmations = 0;
  let autoConfirmFinish = true;
  document.addEventListener('mesh:confirmation-projection', (event) => {
    if (event.detail?.confirmation?.confirmLabel?.startsWith('Reopen')) {
      reopenProjections.push(event.detail);
    } else if (autoConfirmFinish && event.detail?.confirmation?.confirmLabel === 'Finish agent handoff') {
      queueMicrotask(() => {
        document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-mounted', {
          detail: { generation: event.detail.generation },
        }));
        document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-intent', {
          detail: { generation: event.detail.generation, intent: { type: 'confirm' } },
        }));
      });
    }
  });
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-available'));
  globalThis.confirm = () => {
    browserReopenConfirmations += 1;
    return false;
  };
  const refusedReopen = document.emitWorkspaceCurrentIntent('start-codex');
  await waitFor(() => reopenProjections.length === 1);
  assert.equal(browserReopenConfirmations, 0, 'assigned Codex reopen used the blocking browser confirmation');
  assert.equal(reopenProjections[0].confirmation.title, 'Reopen this assigned Codex folder?');
  assert.match(reopenProjections[0].confirmation.description, /already handed to an agent/);
  assert.match(reopenProjections[0].confirmation.description, /Two agents in one folder can overwrite each other/);
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-mounted', {
    detail: { generation: reopenProjections[0].generation },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-intent', {
    detail: { generation: reopenProjections[0].generation, intent: { type: 'cancel' } },
  }));
  await refusedReopen;
  assert.equal(codexParameters, null, 'a refused reopen launched Codex in the terminal agent folder');
  assert.match(document.getElementById('notice').textContent, /Start another agent/);

  // A confirmation authorizes only the custody generation displayed when it opened. Another
  // Mesh process can release and reacquire the same physical workspace while the source-owned
  // confirmation is visible; the native boundary must reject that stale authority before
  // an external launcher runs.
  const codexCallsBeforeStaleReopen = codexCalls;
  const staleReopenGeneration = testAgentHandoffGeneration(agentHandoffGeneration);
  const staleReopen = document.emitWorkspaceCurrentIntent('start-codex');
  await waitFor(() => reopenProjections.length === 2);
  agentHandoffGeneration += 1;
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-mounted', {
    detail: { generation: reopenProjections[1].generation },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-intent', {
    detail: { generation: reopenProjections[1].generation, intent: { type: 'confirm' } },
  }));
  await staleReopen;
  assert.equal(codexCalls, codexCallsBeforeStaleReopen);
  assert.match(document.getElementById('notice').textContent, /assignment changed/);
  assert.notEqual(
    staleReopenGeneration,
    testAgentHandoffGeneration(agentHandoffGeneration),
    'the external process did not install a newer custody generation',
  );

  await document.emitWorkspaceCurrentIntent('refresh');
  const acceptedReopen = document.emitWorkspaceCurrentIntent('start-codex');
  await waitFor(() => reopenProjections.length === 3);
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-mounted', {
    detail: { generation: reopenProjections[2].generation },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-intent', {
    detail: { generation: reopenProjections[2].generation, intent: { type: 'confirm' } },
  }));
  await acceptedReopen;
  assert.equal(browserReopenConfirmations, 0);
  assert.match(document.getElementById('notice').textContent, /Create a new task there and give it your instruction/);
  assert.deepEqual(codexParameters, {
    expectedWorkspaceRoot: workspace.root,
    expectedWorkspaceDigest: workspace.digest,
    expectedWorkspaceInstallation: workspace.installation,
    confirmedReopen: true,
    expectedAgentHandoffGeneration: testAgentHandoffGeneration(2),
  });
  assert.match(document.getElementById('notice').textContent, /^Reopened /);
  assert.equal(Boolean(document.workspaceCurrentAction('finish-agent')), true);
  assert.equal(document.workspaceCurrent?.current.agentAssigned, true);
  assert.equal(
    document.workspaceCurrent?.current.actions.find((action) => action.id === 'finish-agent')?.enabled,
    true,
  );
  loseFinishReply = true;
  // The retained Files controller is not lifecycle authority. Its stale editable-state mirror
  // must not suppress Finish's mandatory post-release scan after exact native preflight succeeds.
  poisonSaveControlOnRelease = true;
  await document.emitWorkspaceCurrentIntent('finish-agent');
  await waitFor(() => privateSaveCalls === 1);
  poisonSaveControlOnRelease = false;
  assert.equal(
    poisonedLifecycleSaveControlReads,
    0,
    'Finish still read the removed hidden Save-file mirror instead of source-owned draft authority',
  );
  assert.deepEqual(finishParameters, {
    expectedWorkspaceRoot: workspace.root,
    expectedWorkspaceDigest: 'workspace-native-work',
    expectedWorkspaceInstallation: workspace.installation,
    expectedAgentHandoffGeneration: testAgentHandoffGeneration(3),
  });
  assert.equal((document.workspaceCurrentAction('start-codex')?.label ?? ''), 'Start Codex on this version');
  assert.equal((document.workspaceCurrentAction('open-terminal')?.label ?? ''), 'Open agent terminal');
  assert.equal(Boolean(document.workspaceCurrentAction('finish-agent')), false);
  assert.equal(privateSaveCalls, 1);
  assert.match(document.getElementById('notice').textContent, /agent folder released/i);
  assert.match(document.getElementById('notice').textContent, /lost the release reply/);
  assert.match(document.getElementById('notice').textContent, /authenticated and saved privately/);
  assert.equal(document.getElementById('folder-change-items').children.length, 0);
  assert.equal(document.getElementById('save-all-private').disabled, true);
  assert.equal(!document.workspaceCurrentAction('copy-working-path')?.enabled, false);
  await document.emitWorkspaceCurrentIntent('copy-working-path');
  assert.equal(copiedPaths.at(-1), '/application/native-workspace/current');
  assert.match(document.getElementById('notice').textContent, /follows the workspace you open in Mesh/);
  reconciledWorkingPath = '/application/native-workspace/replaced';
  const copiedBeforeReplacement = copiedPaths.length;
  await document.emitWorkspaceCurrentIntent('copy-working-path');
  assert.equal(copiedPaths.length, copiedBeforeReplacement, 'a changed stable path reached the clipboard');
  assert.match(document.getElementById('notice').textContent, /could not copy the stable working path/);
  assert.equal(document.serviceState.state === 'ready', true);
  assert.equal(!document.workspaceCurrentAction('copy-working-path')?.enabled, true);
  assert.equal(!document.workspaceCurrentAction('copy-agent-path')?.enabled, false);
  await document.emitWorkspaceCurrentIntent('copy-agent-path');
  await waitFor(() => copiedAgentCalls === 1);
  assert.deepEqual(copiedAgentParameters, {
    expectedAgentPath: nativeFolderPath,
    expectedWorkspaceRoot: workspace.root,
    expectedWorkspaceDigest: workspace.digest,
    expectedWorkspaceInstallation: workspace.installation,
    confirmedReopen: false,
    expectedAgentHandoffGeneration: null,
  });
  assert.equal(copiedPaths.at(-1), stableAgentReference);
  assert.match(document.getElementById('notice').textContent, /stable reference/);
  assert.match(document.getElementById('notice').textContent, /Git history and status are ready/);
  assert.match(document.getElementById('notice').textContent, /marked this writable folder as assigned/);
  assert.equal((document.workspaceCurrentAction('start-codex')?.label ?? ''), 'Reopen assigned Codex folder');
  assert.equal(Boolean(document.workspaceCurrentAction('finish-agent')), true);
  assert.equal((document.workspaceCurrentAction('copy-agent-path')?.label ?? ''), 'Agent folder assigned');
  assert.equal(!document.workspaceCurrentAction('copy-agent-path')?.enabled, true);
  const noticeBeforeAssignedCopy = document.getElementById('notice').textContent;
  await document.emitWorkspaceCurrentIntent('copy-agent-path');
  assert.equal(copiedAgentCalls, 1, 'an assigned folder was exposed to another manual handoff');
  assert.equal(document.getElementById('notice').textContent, noticeBeforeAssignedCopy);
  structuralAgentChange = true;
  await document.emitWorkspaceCurrentIntent('finish-agent');
  assert.equal(privateSaveCalls, 1, 'an ambiguous missing path was saved without structural review');
  assert.match(document.getElementById('notice').textContent, /will not guess a rename, deletion, or unsupported entry/);
  assert.equal(document.getElementById('folder-change-items').children.length, 1);
  assert.equal(document.getElementById('save-all-private').disabled, true);
  structuralAgentChange = false;
  assert.equal(!document.workspaceCurrentAction('copy-agent-path')?.enabled, false);
  clipboardFailure = true;
  await document.emitWorkspaceCurrentIntent('copy-agent-path');
  assert.match(document.getElementById('notice').textContent, /marked this folder as assigned/);
  assert.match(document.getElementById('notice').textContent, /collision warning remains active/i);
  assert.match(document.getElementById('notice').textContent, /Finish agent handoff/);
  assert.equal(document.getElementById('notice').classList.contains('error'), true);

  // Releasing custody is followed by an authoritative scan. If that scan cannot inspect one
  // tracked file, Refresh alone must not make the older saved point writable or hand it to a new
  // agent: the unreadable result may contain the just-finished agent's only complete work.
  clipboardFailure = false;
  inspectionFailure = true;
  agentWorkingChange = true;
  savedAgentRevision = false;
  const finishCallsBeforeFailedInspection = finishCalls;
  await document.emitWorkspaceCurrentIntent('finish-agent');
  await waitFor(() => /could not inspect agent-result\.txt/u.test(document.getElementById('notice').textContent));
  assert.equal(
    finishCalls,
    finishCallsBeforeFailedInspection,
    'agent custody was cleared before its native result could be inspected',
  );
  assert.equal(agentHandoffInstallation, workspace.installation);
  assert.equal(Boolean(document.workspaceCurrentAction('finish-agent')), true);
  assert.equal((document.workspaceCurrent?.current?.state ?? ''), 'Needs attention');
  assert.equal(document.workspaceCurrent?.current.state, 'Needs attention');
  assert.equal(document.getElementById('create-text-entry').disabled, true);
  assert.equal(!document.workspaceCurrentAction('start-agent-copy')?.enabled, true);
  assert.equal(!document.workspaceCurrentAction('update-destination')?.enabled, true);
  assert.equal(!document.workspaceCurrentAction('start-codex')?.enabled, true);
  assert.equal(!document.workspaceCurrentAction('open-terminal')?.enabled, true);
  assert.equal(!document.workspaceCurrentAction('copy-agent-path')?.enabled, true);
  assert.equal(document.getElementById('scan-files').disabled, true);
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Agent folder is assigned');
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Finish agent handoff');
  assert.match(document.getElementById('notice').textContent, /could not inspect agent-result\.txt/);
  const agentLaunchesBeforeRefusal = codexCalls + terminalCalls + copiedAgentCalls;
  await document.emitWorkspaceCurrentIntent('start-codex');
  await document.emitWorkspaceCurrentIntent('open-terminal');
  await document.emitWorkspaceCurrentIntent('copy-agent-path');
  assert.equal(codexCalls + terminalCalls + copiedAgentCalls, agentLaunchesBeforeRefusal);
  assert.match(document.getElementById('notice').textContent, /could not complete its native inspection/);

  await document.emitWorkspaceCurrentIntent('refresh');
  assert.equal((document.workspaceCurrent?.current?.state ?? ''), 'Needs attention');
  assert.equal(document.getElementById('create-text-entry').disabled, true);
  assert.equal(!document.workspaceCurrentAction('start-agent-copy')?.enabled, true);
  assert.equal(document.getElementById('scan-files').disabled, true);

  inspectionFailure = false;
  await document.emitWorkspaceCurrentIntent('finish-agent');
  assert.equal(finishCalls, finishCallsBeforeFailedInspection + 1);
  assert.equal(privateSaveCalls, 2);
  assert.equal(document.getElementById('save-all-private').disabled, true);
  assert.match(document.getElementById('notice').textContent, /authenticated and saved privately/);

  // The first custody scan is clean. A final external write lands only when the native host clears
  // the handoff. Mesh must not claim the folder matches history from that stale clean scan.
  savedAgentRevision = true;
  agentWorkingChange = false;
  const ordinaryInspectionStarted = new Promise((resolve) => { signalOrdinaryInspection = resolve; });
  holdOrdinaryInspection = true;
  const scanStarted = document.getElementById('scan-files').emit('click');
  await ordinaryInspectionStarted;
  await document.emitWorkspaceCurrentIntent('start-codex');
  assert.equal(agentHandoffInstallation, workspace.installation);
  lateAgentWriteOnRelease = true;
  const privateSavesBeforeLateWrite = privateSaveCalls;
  let finishSettled = false;
  const finishCallsBeforeOverlappingScan = finishCalls;
  const finishAfterOverlappingScan = document.emitWorkspaceCurrentIntent('finish-agent').finally(() => {
    finishSettled = true;
  });
  await waitFor(() => document.workspaceCurrentAction('start-codex')?.enabled === false);
  assert.equal(
    finishCalls,
    finishCallsBeforeOverlappingScan,
    'Finish released custody before its already-started native scan settled and preflight ran',
  );
  assert.equal(finishSettled, false, 'Finish skipped an already-started native scan before custody preflight');
  assert.equal(
    !document.workspaceCurrentAction('start-codex')?.enabled,
    true,
    'Finish enabled a new agent launch before its exact post-release scan and save settled',
  );
  assert.equal(
    document.getElementById('open-recent-workspace').disabled,
    true,
    'Finish enabled workspace navigation before its exact post-release scan and save settled',
  );
  const launchesBeforeBlockedFinishClick = codexCalls;
  await document.emitWorkspaceCurrentIntent('start-codex');
  assert.equal(codexCalls, launchesBeforeBlockedFinishClick, 'a scripted launch bypassed the active Finish transition');
  releaseOrdinaryInspection();
  await scanStarted;
  await finishAfterOverlappingScan;
  await waitFor(() => (
    finishCalls === finishCallsBeforeFailedInspection + 2
    && privateSaveCalls === privateSavesBeforeLateWrite + 1
    && agentHandoffInstallation === null
  ));
  assert.equal(
    finishCalls,
    finishCallsBeforeFailedInspection + 2,
    'waiting for the older scan replayed the non-idempotent custody release',
  );
  assert.equal(privateSaveCalls, privateSavesBeforeLateWrite + 1);
  assert.equal(agentHandoffInstallation, null);
  assert.equal(document.getElementById('folder-change-items').children.length, 0);
  assert.match(document.getElementById('notice').textContent, /authenticated and saved privately/);

  mismatchedCodexLaunchReply = true;
  const codexCallsBeforeReactLaunch = codexCalls;
  await document.emitWorkspaceCurrentIntent('start-codex');
  await waitFor(() => codexCalls === codexCallsBeforeReactLaunch + 1);
  assert.equal(agentHandoffInstallation, workspace.installation);
  assert.match(document.getElementById('notice').textContent, /did not confirm that Codex received this exact verified workspace/);
  assert.doesNotMatch(document.getElementById('notice').textContent, /Opened \/different\/workspace in Codex/);

  // A second Mesh process may release and then reacquire the same physical folder while this
  // window's accessible confirmation is open. The installation alone cannot distinguish those
  // custody epochs; the stale confirmation must not clear the newer assignment.
  await document.emitWorkspaceCurrentIntent('refresh');
  autoConfirmFinish = false;
  let finishConfirmation = null;
  document.addEventListener('mesh:confirmation-projection', (event) => {
    if (event.detail?.confirmation?.confirmLabel === 'Finish agent handoff') {
      finishConfirmation = event.detail;
    }
  });
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-available'));
  const finishCallsBeforeReacquisition = finishCalls;
  const staleFinish = document.emitWorkspaceCurrentIntent('finish-agent');
  await waitFor(() => finishConfirmation !== null);
  agentHandoffGeneration += 2;
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-mounted', {
    detail: { generation: finishConfirmation.generation },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-intent', {
    detail: { generation: finishConfirmation.generation, intent: { type: 'confirm' } },
  }));
  await staleFinish;
  await waitFor(() => /assignment changed during inspection/u.test(document.getElementById('notice').textContent));
  assert.equal(finishCalls, finishCallsBeforeReacquisition);
  assert.equal(agentHandoffInstallation, workspace.installation);
  assert.match(document.getElementById('notice').textContent, /assignment changed during inspection/);
  assert.equal(Boolean(document.workspaceCurrentAction('finish-agent')), true);

  // A different process can acquire custody after this Finish saves its result. The final status
  // read must install that newer handoff and refuse the stale success instead of briefly enabling a
  // launch or claiming the just-finished generation still owns the outcome.
  autoConfirmFinish = true;
  await document.emitWorkspaceCurrentIntent('refresh');
  agentWorkingChange = true;
  savedAgentRevision = false;
  reacquireAfterPrivateSave = true;
  const finishCallsBeforePostSaveReacquisition = finishCalls;
  await document.emitWorkspaceCurrentIntent('finish-agent');
  assert.equal(finishCalls, finishCallsBeforePostSaveReacquisition + 1);
  assert.equal(agentHandoffInstallation, workspace.installation);
  assert.equal(!document.workspaceCurrentAction('start-codex')?.enabled, false);
  assert.equal((document.workspaceCurrentAction('start-codex')?.label ?? ''), 'Reopen assigned Codex folder');
  assert.match(document.getElementById('notice').textContent, /same workspace remained unassigned.*newer agent handoff/);
});

test('an ambiguous terminal launch is treated as an active agent handoff', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/ambiguous-terminal.mesh/mounts',
    digest: 'workspace-ambiguous-terminal',
    installation: 'installation-ambiguous-terminal',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-ambiguous-terminal', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'README.md', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    workspace_versions: [{ operation: '74'.repeat(32), ordinal: 1, actor_sequence: '1' }],
    file_histories: [],
  };
  let terminalCalls = 0;
  let codexCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: workspace.root,
        workspaces: [workspace.root],
        workspace_entries: [{
          path: workspace.root,
          export_root: null,
          project_root: null,
          agent_handoff_installation: null,
          agent_handoff_generation: null,
        }],
        auto_opened: true,
        active_folder: '/application/native-workspace/current',
        export_root: null,
        warning: null,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: true,
        native_folder_path: workspace.root,
        working: false,
      });
    }
    if (command === 'reconcile_managed_workspace_navigation') {
      return JSON.stringify({
        path: '/application/native-workspace/current',
        workspace_root: workspace.root,
        stable: true,
      });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'open_managed_workspace_in_terminal') {
      terminalCalls += 1;
      throw new Error('the launcher reply was lost after dispatch');
    }
    if (command === 'open_managed_workspace_in_codex') {
      codexCalls += 1;
      throw new Error('a refused second handoff must not reach Codex');
    }
    throw new Error(`unexpected ambiguous-terminal command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?ambiguous-terminal-handoff=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.emitWorkspaceCurrentIntent('open-terminal');
  assert.equal(terminalCalls, 1);
  assert.equal((document.workspaceCurrentAction('open-terminal')?.label ?? ''), 'Reopen agent terminal');
  assert.equal((document.workspaceCurrentAction('start-codex')?.label ?? ''), 'Reopen assigned Codex folder');
  assert.match(document.getElementById('notice').textContent, /could not confirm whether Terminal opened/);
  assert.match(document.getElementById('notice').textContent, /marked as handed off/);

  let collisionWarning = null;
  globalThis.confirm = (message) => {
    collisionWarning = message;
    return false;
  };
  await document.emitWorkspaceCurrentIntent('start-codex');
  await document.emitWorkspaceCurrentIntent('open-terminal');
  assert.equal(codexCalls, 0);
  assert.equal(terminalCalls, 1);
  assert.equal(collisionWarning, null, 'an unverified custody generation reached reopen confirmation');
  assert.match(document.getElementById('notice').textContent, /Refresh before reopening this assigned folder/);
});

test('copying an agent path refuses a native folder replaced after the last refresh', async () => {
  const document = fakeDocument();
  const nativeFolderPath = '/private/managed/replaced-agent-work';
  const workspace = {
    root: '/managed/replaced-agent-work',
    digest: 'workspace-replaced-agent-work',
    installation: 'installation-replaced-agent-work',
    records: 1,
    private_version: { version: 'version-replaced-agent-work' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
  };
  let checkpointReads = 0;
  let prepareCalls = 0;
  let copiedAgentPath = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        auto_opened: false,
        remembered: workspace.root,
        workspaces: [workspace.root],
        workspace_entries: [{
          path: workspace.root,
          export_root: '/Users/person/replaced-agent-work',
          project_root: '/Users/person/replaced-agent-work',
        }],
        active_folder: '/application/native-workspace/current',
      });
    }
    if (command === 'managed_checkpoint_state') {
      checkpointReads += 1;
      if (checkpointReads > 1) {
        throw new Error('the verified native workspace directory was replaced');
      }
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: true,
        native_folder_path: nativeFolderPath,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'prepare_managed_workspace_agent_path') {
      prepareCalls += 1;
      throw new Error('the verified native workspace directory was replaced');
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = {
    __TAURI__: { core: { invoke } },
    navigator: {
      clipboard: {
        async writeText(path) {
          copiedAgentPath = path;
        },
      },
    },
  };
  globalThis.confirm = () => true;
  await import(`./app.js?replaced-agent-path=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.emitWorkspaceCurrentIntent('copy-agent-path');

  assert.equal(checkpointReads, 1, 'the browser performed an obsolete checkpoint-only re-read');
  assert.equal(prepareCalls, 1, 'copy did not re-enter the native handoff verifier');
  assert.equal(copiedAgentPath, null, 'a stale native path reached the clipboard');
  assert.match(document.getElementById('notice').textContent, /replaced/);
  assert.match(document.getElementById('notice').textContent, /Refresh/);
  assert.doesNotMatch(
    document.getElementById('notice').textContent,
    /copy it manually|Select the Independent agent folder shown above/,
    'a refused native folder must not be offered as a manual agent handoff',
  );
  assert.equal(document.getElementById('notice').classList.contains('error'), true);
  assert.equal(document.serviceState.state === 'ready', false);
  assert.equal(!document.workspaceCurrentAction('copy-agent-path')?.enabled, true);
});

test('version switching detects an unsaved native directory when no file candidates exist', async () => {
  const document = fakeDocument();
  const operation = '42'.repeat(32);
  const workspace = {
    root: '/managed/folder-only/mounts',
    digest: 'workspace-folder-only',
    installation: 'installation-folder-only',
    records: 2,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-folder-only', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'saved-folder', type: 'folder' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  let directoryScans = 0;
  let confirmations = 0;
  let openCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: workspace.root,
        workspaces: [workspace.root],
        auto_opened: false,
        active_folder: '/application/native-workspace/current',
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'preview_managed_workspace_version') {
      return savedWorkspacePreview(operation, [{ path: 'saved-folder', type: 'folder', bytes: null }]);
    }
    if (command === 'discover_native_directories') {
      directoryScans += 1;
      return JSON.stringify([{ path: 'agent-created-empty-folder' }]);
    }
    if (command === 'discover_native_missing_files') return '[]';
    if (command === 'open_managed_workspace_version') {
      openCalls += 1;
      throw new Error('the switch should have been cancelled before opening');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => {
    confirmations += 1;
    return false;
  };
  await import(`./app.js?folder-only-departure=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const version = document.getElementById('workspace-version');
  version.value = operation;
  await version.emit('change');
  await document.getElementById('fork-version').emit('click');

  assert.equal(directoryScans, 1, 'folder-only workspace skipped its unsaved-directory scan');
  assert.equal(confirmations, 1, 'unsaved directory did not require departure confirmation');
  assert.equal(openCalls, 0, 'workspace version opened after the person cancelled the warning');
});

test('refresh retargets the stable folder after another local client switches workspaces', async () => {
  const document = fakeDocument();
  const stableFolder = '/application/native-workspace/current';
  const first = {
    root: '/managed/first/mounts',
    digest: 'workspace-first',
    installation: 'installation-first',
    records: 1,
    private_version: { version: 'version-first' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
  };
  const second = {
    ...first,
    root: '/managed/second/mounts',
    digest: 'workspace-second',
    installation: 'installation-second',
    private_version: { version: 'version-second' },
  };
  let current = first;
  let reconciled = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: first.root,
        workspaces: [first.root, second.root],
        auto_opened: true,
        active_folder: stableFolder,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: current.root,
        workspace_digest: current.digest,
        workspace_installation: current.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(current);
    }
    if (command === 'reconcile_managed_workspace_navigation') {
      reconciled = parameters;
      return JSON.stringify({
        path: stableFolder,
        workspace_root: current.root,
        stable: true,
        native_folder: true,
      });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?external-workspace-switch=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  current = second;
  await document.emitWorkspaceCurrentIntent('refresh');

  assert.deepEqual(reconciled, {
    expectedWorkspaceRoot: second.root,
    expectedWorkspaceDigest: second.digest,
    expectedWorkspaceInstallation: second.installation,
  });
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), second.root);
  assert.equal((document.workspaceCurrent?.current?.workingFolder ?? ''), stableFolder);
});

test('startup reconciles a stable folder after a same-path workspace replacement before the first read', async () => {
  const document = fakeDocument();
  const stableFolder = '/application/native-workspace/current';
  const remembered = {
    root: '/managed/current/mounts',
    digest: 'workspace-remembered',
    installation: 'installation-remembered',
  };
  const current = {
    root: '/managed/current/mounts',
    digest: 'workspace-current',
    installation: 'installation-current',
    records: 1,
    private_version: { version: 'version-current' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
  };
  let reconciled = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: remembered.root,
        workspaces: [remembered.root],
        auto_opened: true,
        active_folder: stableFolder,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: current.root,
        workspace_digest: current.digest,
        workspace_installation: current.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(current);
    }
    if (command === 'reconcile_managed_workspace_navigation') {
      reconciled = parameters;
      return JSON.stringify({
        path: stableFolder,
        workspace_root: current.root,
        stable: true,
        native_folder: true,
      });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?startup-external-workspace-switch=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.deepEqual(reconciled, {
    expectedWorkspaceRoot: current.root,
    expectedWorkspaceDigest: current.digest,
    expectedWorkspaceInstallation: current.installation,
  });
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), current.root);
  assert.equal((document.workspaceCurrent?.current?.workingFolder ?? ''), stableFolder);
});

test('a legacy workspace upgrades to the stable native folder from the primary action', async () => {
  const document = fakeDocument();
  const operation = '44'.repeat(32);
  const legacy = {
    root: '/managed/legacy',
    digest: 'workspace-legacy',
    installation: 'installation-legacy',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-legacy', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'note.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  const native = {
    ...legacy,
    root: '/managed/legacy-version-1.mesh/mounts',
    digest: 'workspace-native',
    installation: 'installation-native',
  };
  const stableFolder = '/application/native-workspace/current';
  const exportRoot = '/ordinary/original';
  let open = legacy;
  let forkParameters = null;
  let revealed = false;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: legacy.root,
        workspaces: [legacy.root],
        auto_opened: true,
        active_folder: null,
        export_root: exportRoot,
        warning: null,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: open.root,
        workspace_digest: open.digest,
        workspace_installation: open.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(open);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'preview_managed_workspace_version') {
      assert.equal(parameters.operation, operation);
      return savedWorkspacePreview(operation, [{ path: 'original.txt', type: 'file', bytes: '9' }]);
    }
    if (command === 'open_managed_workspace_version') {
      forkParameters = parameters;
      open = native;
      return JSON.stringify({
        source_version: operation,
        source_ordinal: 1,
        destination: native.root,
        workspace: native,
        navigation: {
          remembered: native.root,
          workspaces: [native.root, legacy.root],
          auto_opened: false,
          active_folder: stableFolder,
          export_root: exportRoot,
          warning: null,
          build_revision: 'development',
          build_exact: false,
        },
      });
    }
    if (command === 'remember_managed_workspace') {
      throw new Error(`native version navigation must not be repeated: ${JSON.stringify(parameters)}`);
    }
    if (command === 'reveal_managed_workspace') {
      revealed = true;
      return JSON.stringify({
        path: stableFolder,
        workspace_root: native.root,
        stable: true,
        native_folder: true,
      });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?legacy-native-upgrade=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.match(
    document.getElementById('hero-lede').textContent,
    /Create a native working folder/,
    'the first-screen guidance must not advertise a stable folder that does not exist yet',
  );
  assert.equal(document.workspaceCurrentAction('open-folder')?.label, 'Create native working folder');
  assert.equal(document.workspaceCurrentAction('open-folder')?.enabled, true);
  assert.equal(!document.workspaceCurrentAction('copy-agent-path')?.enabled, true);
  assert.equal((document.workspaceCurrent?.current?.agentFolderLabel ?? ''), 'Private workspace storage');
  assert.match((document.workspaceCurrent?.current?.nativeFolderHint ?? ''), /Create a native working folder before opening/);
  await document.emitWorkspaceCurrentIntent('open-folder');
  assert.deepEqual(forkParameters, {
    operation,
    destination: null,
    exportRoot,
    expectedWorkspaceRoot: legacy.root,
    expectedWorkspaceDigest: legacy.digest,
    expectedWorkspaceInstallation: legacy.installation,
  });
  assert.equal(revealed, true);
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), native.root);
  assert.equal((document.workspaceCurrent?.current?.workingFolder ?? ''), stableFolder);
  assert.equal(!document.workspaceCurrentAction('copy-agent-path')?.enabled, false);
  assert.equal((document.workspaceCurrent?.current?.agentFolderLabel ?? ''), 'Pinned agent folder');
  assert.equal(document.destinationField('destination').value, exportRoot);
  assert.match(document.getElementById('hero-lede').textContent, /Use the stable folder/);
});

test('creating a native folder never strands unsaved agent work in the legacy workspace', async () => {
  const document = fakeDocument();
  const operation = '4a'.repeat(32);
  const legacy = {
    root: '/managed/legacy-with-agent-work',
    digest: 'workspace-legacy-with-agent-work',
    installation: 'installation-legacy-with-agent-work',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-legacy-with-agent-work', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'saved.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: ['agent-draft.txt'],
    file_histories: [],
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  let versionOpenCalls = 0;
  let launcherCalls = 0;
  let confirmations = 0;
  let inspections = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: legacy.root,
        workspaces: [legacy.root],
        auto_opened: false,
        active_folder: null,
        export_root: '/ordinary/original',
        warning: null,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: legacy.root,
        workspace_digest: legacy.digest,
        workspace_installation: legacy.installation,
        native_folder: false,
        native_folder_path: null,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(legacy);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'inspect_native_file') {
      inspections += 1;
      assert.equal(parameters.relativePath, 'agent-draft.txt');
      return JSON.stringify({
        path: 'agent-draft.txt',
        text: 'unsaved agent work\n',
        text_editable: true,
        native_untracked: true,
        modified_from_current_version: true,
        current_version: null,
        byte_count: 19,
        content_digest: '4b'.repeat(32),
        executable: false,
      });
    }
    if (command === 'open_managed_workspace_version') {
      versionOpenCalls += 1;
      throw new Error('an upgrade with unsaved work must not create a version workspace');
    }
    if (command === 'open_managed_workspace_in_codex' || command === 'open_managed_workspace_in_terminal') {
      launcherCalls += 1;
      throw new Error('an upgrade with unsaved work must not launch an agent');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => {
    confirmations += 1;
    return true;
  };
  await import(`./app.js?legacy-native-unsaved-guard=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  for (const action of ['open-folder', 'start-codex', 'open-terminal', 'start-agent-copy']) {
    assert.equal(
      document.workspaceCurrentAction(action)?.enabled,
      false,
      `${action} looked available even though Mesh already knew about unsaved native work`,
    );
  }
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Review 1 native change');
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Find changes');

  for (const action of ['open-folder', 'start-codex', 'open-terminal']) {
    const noticeBeforeDisabledAction = document.getElementById('notice').textContent;
    await document.emitWorkspaceCurrentIntent(action);
    assert.equal(versionOpenCalls, 0, `${action} stranded the legacy agent draft`);
    assert.equal(launcherCalls, 0, `${action} launched an agent without the draft`);
    assert.equal(document.getElementById('notice').textContent, noticeBeforeDisabledAction);
  }
  assert.equal(inspections, 0, 'a disabled React handoff repeated an inspection already represented by the exact projection');
  assert.equal(confirmations, 0, 'initial native-folder creation must not offer a leave-work-behind override');
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Review 1 native change');
});

test('Open in Codex creates the native folder directly for a legacy saved workspace', async () => {
  const document = fakeDocument();
  const operation = '45'.repeat(32);
  const legacy = {
    root: '/managed/legacy-agent',
    digest: 'workspace-legacy-agent',
    installation: 'installation-legacy-agent',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-legacy-agent', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'note.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  const native = {
    ...legacy,
    root: '/managed/legacy-agent-version.mesh/mounts',
    digest: 'workspace-native-agent',
    installation: 'installation-native-agent',
  };
  const stableFolder = '/application/native-workspace/current';
  let open = legacy;
  let forkParameters = null;
  let codexParameters = null;
  let revealCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: legacy.root,
        workspaces: [legacy.root],
        auto_opened: true,
        active_folder: null,
        export_root: '/ordinary/original',
        warning: null,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: open.root,
        workspace_digest: open.digest,
        workspace_installation: open.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(open);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'open_managed_workspace_version') {
      forkParameters = parameters;
      open = native;
      return JSON.stringify({
        source_version: operation,
        source_ordinal: 1,
        destination: native.root,
        workspace: native,
        navigation: {
          remembered: native.root,
          workspaces: [native.root, legacy.root],
          auto_opened: false,
          active_folder: stableFolder,
          export_root: '/ordinary/original',
          warning: null,
          build_revision: 'development',
          build_exact: false,
        },
      });
    }
    if (command === 'remember_managed_workspace') {
      throw new Error(`native version navigation must not be repeated: ${JSON.stringify(parameters)}`);
    }
    if (command === 'reveal_managed_workspace') {
      revealCalls += 1;
      throw new Error('Codex handoff must not open Finder first');
    }
    if (command === 'open_managed_workspace_in_codex') {
      codexParameters = parameters;
      return JSON.stringify({
        path: native.root,
        workspace_installation: native.installation,
        fixed_workspace_path: true,
        agent_handoff_recorded: true,
        agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
        agent: 'Codex',
      });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?legacy-agent-upgrade=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.equal(document.workspaceCurrentAction('start-agent-copy')?.enabled, true, 'a clean legacy workspace could not start its first isolated agent directly');
  assert.equal(document.workspaceCurrentAction('start-codex')?.label, 'Create folder + start Codex');
  assert.equal(document.workspaceCurrentAction('start-codex')?.enabled, true);
  await document.emitWorkspaceCurrentIntent('start-codex');

  assert.deepEqual(forkParameters, {
    operation,
    destination: null,
    exportRoot: '/ordinary/original',
    expectedWorkspaceRoot: legacy.root,
    expectedWorkspaceDigest: legacy.digest,
    expectedWorkspaceInstallation: legacy.installation,
  });
  assert.deepEqual(codexParameters, {
    expectedWorkspaceRoot: native.root,
    expectedWorkspaceDigest: native.digest,
    expectedWorkspaceInstallation: native.installation,
    confirmedReopen: false,
    expectedAgentHandoffGeneration: null,
  });
  assert.equal(revealCalls, 0);
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), native.root);
  assert.equal((document.workspaceCurrent?.current?.workingFolder ?? ''), stableFolder);
  assert.equal(document.workspaceCurrentAction('start-codex')?.label, 'Reopen assigned Codex folder');
  assert.match(document.getElementById('notice').textContent, /Created the native working folder and opened/);
  assert.match(document.getElementById('notice').textContent, /fixed real folder/);
});

test('a stable-link failure does not misclassify a verified native workspace as legacy', async () => {
  const document = fakeDocument();
  const operation = '46'.repeat(32);
  const native = {
    root: '/application/workspace-versions/point-464646464646.mesh/mounts',
    digest: 'workspace-native-without-link',
    installation: 'installation-native-without-link',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-native-without-link', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  let forkCalls = 0;
  let revealCalls = 0;
  let codexParameters = null;
  let finishParameters = null;
  let handoffInstallation = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: native.root,
        workspaces: [native.root],
        workspace_entries: [{
          path: native.root,
          export_root: '/ordinary/original',
          project_root: '/ordinary/original',
          agent_handoff_installation: handoffInstallation,
          agent_handoff_generation: handoffInstallation ? TEST_AGENT_HANDOFF_GENERATION : null,
        }],
        auto_opened: true,
        active_folder: null,
        export_root: '/ordinary/original',
        warning: 'The stable folder could not be created, but the native workspace is healthy.',
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: native.root,
        workspace_digest: native.digest,
        workspace_installation: native.installation,
        native_folder: true,
        native_folder_path: native.root,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(native);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'open_managed_workspace_version') {
      forkCalls += 1;
      throw new Error('a healthy native workspace must not be forked again');
    }
    if (command === 'open_managed_workspace_in_codex') {
      codexParameters = parameters;
      handoffInstallation = native.installation;
      return JSON.stringify({ path: native.root, workspace_installation: native.installation, fixed_workspace_path: true, agent_handoff_recorded: true, agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION, agent: 'Codex' });
    }
    if (command === 'finish_managed_workspace_agent_handoff') {
      finishParameters = parameters;
      handoffInstallation = null;
      return JSON.stringify({
        path: native.root,
        workspace_installation: native.installation,
        cleared: true,
      });
    }
    if (command === 'inspect_agent_finish_preflight') {
      assert.deepEqual(parameters, {
        expectedWorkspaceRoot: native.root,
        expectedWorkspaceDigest: native.digest,
        expectedWorkspaceInstallation: native.installation,
        expectedAgentHandoffGeneration: TEST_AGENT_HANDOFF_GENERATION,
      });
      return JSON.stringify({
        schema: 'mesh.agent-finish-preflight/v1',
        workspace_root: native.root,
        workspace_digest: native.digest,
        workspace_installation: native.installation,
        agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
        managed_files: [],
        native_files: [],
        native_directories: [],
        missing_files: [],
        unsupported_entries: [],
      });
    }
    if (command === 'reveal_managed_workspace') {
      revealCalls += 1;
      return JSON.stringify({
        path: '/application/native-workspace/current',
        workspace_root: native.root,
        stable: true,
        native_folder: true,
      });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?native-without-stable-link=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.equal((document.workspaceCurrent?.current?.workingFolder ?? ''), 'Unavailable — retry Open working folder');
  assert.match(
    document.getElementById('hero-lede').textContent,
    /independent real folder is ready/,
    'a broken optional shortcut must not make the healthy native folder sound unavailable',
  );
  assert.match(document.getElementById('hero-lede').textContent, /stable shortcut needs attention/);
  assert.equal((document.workspaceCurrent?.current?.agentFolderLabel ?? ''), 'Pinned agent folder');
  assert.equal(!document.workspaceCurrentAction('copy-agent-path')?.enabled, false);
  assert.equal(document.workspaceCurrentAction('open-folder')?.label, 'Open working folder');
  assert.equal(document.workspaceCurrentAction('open-folder')?.enabled, true);
  assert.equal(document.workspaceCurrentAction('start-codex')?.label, 'Start Codex on this version');
  assert.equal(document.workspaceCurrentAction('start-codex')?.enabled, true);
  await document.emitWorkspaceCurrentIntent('start-codex');

  assert.equal(forkCalls, 0);
  assert.deepEqual(codexParameters, {
    expectedWorkspaceRoot: native.root,
    expectedWorkspaceDigest: native.digest,
    expectedWorkspaceInstallation: native.installation,
    confirmedReopen: false,
    expectedAgentHandoffGeneration: null,
  });
  assert.equal(Boolean(document.workspaceCurrentAction('finish-agent')), true);
  assert.equal(
    document.workspaceCurrentAction('finish-agent')?.enabled,
    true,
    'a broken optional stable link stranded the exact pinned agent-folder handoff',
  );
  await document.emitWorkspaceCurrentIntent('finish-agent');
  assert.deepEqual(finishParameters, {
    expectedWorkspaceRoot: native.root,
    expectedWorkspaceDigest: native.digest,
    expectedWorkspaceInstallation: native.installation,
    expectedAgentHandoffGeneration: TEST_AGENT_HANDOFF_GENERATION,
  });
  assert.equal(Boolean(document.workspaceCurrentAction('finish-agent')), false);
  await document.emitWorkspaceCurrentIntent('open-folder');
  assert.equal(revealCalls, 1);
  assert.equal(forkCalls, 0);
});

test('concurrent history and ordinary-folder shortcuts move focus to the required choice', async () => {
  const document = fakeDocument();
  let previewCalls = 0;
  let freshAgentCalls = 0;
  let ordinaryVersionCalls = 0;
  let codexCalls = 0;
  const workspace = {
    root: '/managed/concurrent-legacy',
    digest: 'workspace-concurrent-legacy',
    installation: 'installation-concurrent-legacy',
    records: 2,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-concurrent-legacy', concurrent_changes: 2 },
    shared_version: null,
    entries: [{ path: 'note.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [
      { operation: '46'.repeat(32), ordinal: 1, actor_sequence: '1' },
      { operation: '47'.repeat(32), ordinal: 2, actor_sequence: '1' },
    ],
  };
  const agentWorkspace = {
    ...workspace,
    root: '/managed/concurrent-agent-copy',
    digest: 'workspace-concurrent-agent-copy',
    installation: 'installation-concurrent-agent-copy',
    private_version: { version: 'version-concurrent-agent-copy', concurrent_changes: 1 },
    workspace_versions: [{ operation: '47'.repeat(32), ordinal: 2, actor_sequence: '1' }],
  };
  let open = workspace;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: workspace.root,
        workspaces: [workspace.root],
        auto_opened: true,
        active_folder: null,
        export_root: null,
        warning: null,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: open.root,
        workspace_digest: open.digest,
        workspace_installation: open.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(open);
    }
    if (command === 'preview_managed_workspace_version') {
      previewCalls += 1;
      const selected = workspace.workspace_versions.find((version) => (
        version.operation === parameters.operation
      ));
      return savedWorkspacePreview(parameters.operation, [{
        path: 'note.txt',
        type: 'file',
        bytes: '4',
      }], {
        ordinal: selected.ordinal,
        actorSequence: selected.actor_sequence,
        changeBasis: 'combined-history',
        changes: [],
      });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return JSON.stringify([]);
    }
    if (command === 'open_fresh_agent_workspace') {
      freshAgentCalls += 1;
      assert.equal(parameters.operation, '47'.repeat(32));
      assert.equal(Object.hasOwn(parameters, 'destination'), false);
      open = agentWorkspace;
      return JSON.stringify({
        source_version: '47'.repeat(32),
        source_ordinal: 2,
        destination: agentWorkspace.root,
        reused: false,
        workspace: agentWorkspace,
        navigation: {
          remembered: agentWorkspace.root,
          workspaces: [agentWorkspace.root, workspace.root],
          workspace_entries: [
            {
              path: agentWorkspace.root,
              export_root: null,
              project_root: null,
              source_point_ordinal: 2,
            },
            { path: workspace.root, export_root: null, project_root: null },
          ],
          auto_opened: false,
          active_folder: '/application/native-workspace/current',
          export_root: null,
          warning: null,
          build_revision: 'development',
          build_exact: false,
        },
      });
    }
    if (command === 'open_managed_workspace_version') {
      ordinaryVersionCalls += 1;
      throw new Error('agent version choice reused the ordinary workspace path');
    }
    if (command === 'open_managed_workspace_in_codex') {
      codexCalls += 1;
      return JSON.stringify({
        path: agentWorkspace.root,
        workspace_installation: agentWorkspace.installation,
        fixed_workspace_path: true,
        agent_handoff_recorded: true,
        agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
        agent: 'Codex',
      });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?concurrent-journey-shortcuts=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.match(
    document.getElementById('hero-lede').textContent,
    /does not yet have an isolated native working folder/,
  );
  assert.match(document.getElementById('hero-lede').textContent, /concurrent saved history/);
  assert.doesNotMatch(document.getElementById('hero-lede').textContent, /no .*durable point.*safe/i);
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Choose a saved workspace point');
  assert.match(document.workspaceOverview?.overview.nextActionDescription, /more than one current line/);
  assert.equal(document.workspaceCurrentAction('open-folder')?.label, 'Choose workspace version');
  assert.equal(document.workspaceCurrentAction('open-folder')?.enabled, true);
  await document.emitWorkspaceCurrentIntent('open-folder');
  assert.equal(document.getElementById('workspace-versions-next').scrolledIntoView, true);
  assert.equal(document.workspaceVersionChoice('47'.repeat(32)).focused, true);
  assert.match(document.getElementById('notice').textContent, /now in focus/);
  assert.equal(
    document.getElementById('notice').classList.contains('error'),
    false,
    'ordinary concurrent-version navigation was styled as an error',
  );

  assert.equal(document.workspaceCurrentAction('start-agent-copy')?.enabled, true, 'concurrent durable heads disabled agent version choice');
  assert.equal(document.workspaceCurrentAction('start-agent-copy')?.label, 'Choose another agent version');
  assert.deepEqual(
    document.getElementById('workspace-version').children.map((option) => option.textContent),
    [
      'Choose from concurrent saved history',
      'Saved workspace · point 2',
      'Saved workspace · point 1',
    ],
    'points from concurrent history were mislabeled as earlier linear history or as tips',
  );
  document.getElementById('version-destination').value = '/custom/ordinary-version-copy';
  await document.getElementById('version-destination').emit('input');
  await document.emitWorkspaceCurrentIntent('start-agent-copy');
  assert.equal(document.getElementById('workspace-version').value, '');
  assert.equal(document.getElementById('version-destination').value, '');
  assert.equal(document.getElementById('version-destination').disabled, true);
  assert.match(document.getElementById('version-destination').title, /Mesh-managed private location/i);
  assert.equal(document.getElementById('version-location-summary').textContent, 'Fresh agent location is managed by Mesh');
  assert.equal(previewCalls, 0, 'concurrent agent selection guessed one history row');
  assert.equal(document.getElementById('workspace-versions-next').scrolledIntoView, true);
  assert.equal(document.workspaceVersionChoice('47'.repeat(32)).focused, true);
  assert.match(document.getElementById('notice').textContent, /Choose one exact saved workspace from the concurrent history/);
  document.getElementById('workspace-version').value = '47'.repeat(32);
  await document.getElementById('workspace-version').emit('change');
  assert.equal(previewCalls, 1);
  assert.equal(document.workspaceVersions.versions.changeBasis, 'combined-history');
  assert.equal(document.workspaceVersions.versions.basisOrdinal, null);
  assert.match(document.getElementById('workspace-version-preview').textContent, /Combined saved contents/);
  assert.match(document.getElementById('workspace-version-preview').textContent, /This point combines concurrent saved work\. Inspect the complete file list below\./);
  assert.doesNotMatch(document.getElementById('workspace-version-preview').textContent, /No visible file or folder changes/);
  assert.match(document.getElementById('version-hint').textContent, /verified and ready to open/);
  assert.equal(document.getElementById('fork-version-codex').textContent, 'Start another agent from this point');
  assert.match(document.getElementById('fork-version-codex').title, /fresh independent folder/i);
  assert.match(document.getElementById('version-hint').textContent, /fresh app-managed physical folder/i);
  assert.doesNotMatch(document.getElementById('version-hint').textContent, /unless you enter a custom one/i);

  assert.equal(document.workspaceCurrentAction('update-destination')?.enabled, true);
  assert.equal(document.workspaceCurrentAction('update-destination')?.label, 'Choose destination folder');
  await document.emitWorkspaceCurrentIntent('update-destination');
  assert.equal(document.getElementById('workspace-destination-next').scrolledIntoView, true);
  assert.equal(document.destinationActionControl('choose-destination').focused, true);
  assert.match(document.getElementById('notice').textContent, /original or destination folder you want to update/);

  await document.getElementById('fork-version-codex').emit('click');
  assert.equal(freshAgentCalls, 1, 'the selected concurrent point did not create a fresh agent copy');
  assert.equal(ordinaryVersionCalls, 0, 'the agent choice reused an ordinary saved-version checkout');
  assert.equal(codexCalls, 1);
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), agentWorkspace.root);
  assert.match(document.getElementById('notice').textContent, /Opened Saved point 2/);
  assert.match(document.getElementById('notice').textContent, /independent writable folder/);
});

test('update original refuses before preview when native work is not saved', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/native-draft/mounts',
    digest: 'workspace-native-draft',
    installation: 'installation-native-draft',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-native-draft', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'note.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [{
      path: 'note.txt',
      object_id: 'object-note',
      current: { version_id: 'version-note', manifest_id: 'manifest-note' },
      retained_versions: [{ version_id: 'version-note', manifest_id: 'manifest-note' }],
    }],
    workspace_versions: [{ operation: '48'.repeat(32), ordinal: 1, actor_sequence: '1' }],
  };
  let exportPreviewCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: workspace.root,
        workspaces: [workspace.root],
        auto_opened: true,
        active_folder: '/application/native-workspace/current',
        export_root: '/ordinary/original',
        warning: null,
      });
    }
    if (command === 'reconcile_managed_workspace_navigation') {
      return JSON.stringify({
        path: '/application/native-workspace/current',
        workspace_root: workspace.root,
        stable: true,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return JSON.stringify([]);
    }
    if (command === 'inspect_managed_file') {
      return JSON.stringify({
        path: 'note.txt',
        text: 'agent edit\n',
        text_editable: true,
        modified_from_current_version: true,
        native_untracked: false,
        native_missing: false,
        current_version: 'version-note',
        byte_count: 11,
        content_digest: 'digest-agent-edit',
        executable: false,
        installation: workspace.installation,
      });
    }
    if (command === 'preview_managed_export') {
      exportPreviewCalls += 1;
      throw new Error('pull back preview must not run while native work is unsaved');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?pull-back-native-draft=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.equal(document.workspaceCurrentAction('update-destination')?.enabled, true);
  await document.emitWorkspaceCurrentIntent('update-destination');

  assert.equal(exportPreviewCalls, 0);
  assert.match(document.getElementById('notice').textContent, /1 unsaved native change/);
  assert.match(document.getElementById('notice').textContent, /before updating the destination folder/);
  assert.equal(document.getElementById('workspace-destination-next').scrolledIntoView, false);

  const exportFile = document.destinationField('selectedFile');
  exportFile.value = 'note.txt';
  await exportFile.emit('change');
  await document.destinationActionControl('preview-single').emit('click');
  await document.destinationActionControl('preview-all').emit('click');
  assert.equal(
    exportPreviewCalls,
    0,
    'the destination update card bypassed the unsaved native work check used by its shortcut',
  );
  assert.match(document.getElementById('notice').textContent, /before previewing the destination update/);
});

test('a lost navigation-record reply recovers the native folder completed by the native side', async () => {
  const document = fakeDocument();
  const operation = '55'.repeat(32);
  const presented = {
    root: '/managed/current.mesh/mounts',
    digest: 'workspace-presented',
    installation: 'installation-presented',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-presented', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'note.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  const legacy = {
    ...presented,
    root: '/managed/legacy',
    digest: 'workspace-legacy-after-switch',
    installation: 'installation-legacy-after-switch',
    private_version: { version: 'version-legacy', concurrent_changes: 1 },
  };
  const raced = {
    ...presented,
    root: '/managed/raced',
    digest: 'workspace-raced-after-switch',
    installation: 'installation-raced-after-switch',
    private_version: { version: 'version-raced', concurrent_changes: 1 },
  };
  const external = {
    ...presented,
    root: '/managed/external-client',
    digest: 'workspace-external-client',
    installation: 'installation-external-client',
    private_version: { version: 'version-external', concurrent_changes: 1 },
  };
  const staleStableFolder = '/application/native-workspace/current';
  const originalFolder = '/ordinary/original';
  let open = presented;
  let stableTarget = presented.root;
  const exactNavigationBindings = [];
  let recentStatus = {
    remembered: presented.root,
    workspaces: [presented.root],
    auto_opened: true,
    active_folder: staleStableFolder,
    export_root: null,
    warning: null,
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify(recentStatus);
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: open.root,
        workspace_digest: open.digest,
        workspace_installation: open.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(open);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'reconcile_managed_workspace_navigation') {
      exactNavigationBindings.push(parameters);
      if (parameters.expectedWorkspaceRoot !== open.root) {
        throw new Error('another local client changed the daemon workspace');
      }
      return JSON.stringify({
        path: staleStableFolder,
        workspace_root: open.root,
        stable: true,
        native_folder: true,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      const requested = JSON.parse(parameters.paramsJson).path;
      open = requested === raced.root ? raced : legacy;
      return JSON.stringify(open);
    }
    if (command === 'remember_managed_workspace') {
      stableTarget = parameters.path;
      recentStatus = {
        remembered: parameters.path,
        workspaces: [parameters.path, legacy.root, presented.root],
        auto_opened: false,
        active_folder: staleStableFolder,
        export_root: originalFolder,
        warning: null,
      };
      if (parameters.path === raced.root) {
        open = external;
        stableTarget = external.root;
      }
      throw new Error('recent workspace record is unavailable');
    }
    if (command === 'reconcile_current_workspace_navigation') {
      throw new Error('unbound current-workspace reconciliation is unsafe after a lost reply');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?failed-navigation-switch=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  let entryGeneration = document.workspaceEntry.generation;
  await document.emitWorkspaceEntryIntent({ type: 'update-managed-path', path: legacy.root }, entryGeneration);
  await document.emitWorkspaceEntryIntent({ type: 'open-managed-path', path: legacy.root }, entryGeneration);
  await waitFor(() => (document.workspaceCurrent?.current?.agentFolder ?? '') === legacy.root);

  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), legacy.root);
  assert.deepEqual(exactNavigationBindings, [
    {
      expectedWorkspaceRoot: presented.root,
      expectedWorkspaceDigest: presented.digest,
      expectedWorkspaceInstallation: presented.installation,
    },
    {
      expectedWorkspaceRoot: legacy.root,
      expectedWorkspaceDigest: legacy.digest,
      expectedWorkspaceInstallation: legacy.installation,
    },
  ]);
  assert.equal(stableTarget, legacy.root, 'the native side did not complete stable-link activation');
  assert.equal((document.workspaceCurrent?.current?.workingFolder ?? ''), staleStableFolder);
  assert.equal((document.workspaceCurrentAction('open-folder')?.label ?? ''), 'Open working folder');
  assert.equal(!document.workspaceCurrentAction('open-folder')?.enabled, false);
  assert.equal(document.destinationField('destination').value, originalFolder);
  assert.match(document.getElementById('notice').textContent, /recent workspace record is unavailable/);

  entryGeneration = document.workspaceEntry.generation;
  await document.emitWorkspaceEntryIntent({ type: 'update-managed-path', path: raced.root }, entryGeneration);
  await document.emitWorkspaceEntryIntent({ type: 'open-managed-path', path: raced.root }, entryGeneration);
  await waitFor(() => document.serviceState.state !== 'ready');

  assert.deepEqual(exactNavigationBindings.at(-1), {
    expectedWorkspaceRoot: raced.root,
    expectedWorkspaceDigest: raced.digest,
    expectedWorkspaceInstallation: raced.installation,
  });
  assert.equal(stableTarget, external.root);
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), raced.root);
  assert.equal(document.serviceState.state === 'ready', false);
  assert.equal(!document.workspaceCurrentAction('open-folder')?.enabled, true);
  assert.match(document.getElementById('notice').textContent, /another local client changed the daemon workspace/);
});

test('export confirmation is bound to the exact saved and ordinary-folder preview', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/project/mounts',
    digest: 'workspace-export',
    installation: 'installation-export',
    records: 3,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-saved' },
    shared_version: null,
    entries: [{ path: 'docs/note.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    workspace_versions: [],
    file_histories: [{
      object_id: 'object-note',
      path: 'docs/note.txt',
      current: { version_id: 'version-saved', manifest_id: 'manifest-saved' },
      retained_versions: [{ version_id: 'version-saved', manifest_id: 'manifest-saved' }],
    }],
  };
  const preview = {
    path: 'docs/note.txt',
    source_version: 'version-saved',
    source_byte_count: 12,
    source_content_digest: 'source-digest',
    source_executable: false,
    source_text: 'saved bytes\n',
    target_root: '/ordinary/original ',
    target_installation: 'target-dev-inode',
    target_parent_installation: 'target-parent-dev-inode',
    target_file_installation: 'target-file-dev-inode',
    target_exists: true,
    target_byte_count: 10,
    target_content_digest: 'target-digest',
    target_executable: false,
    target_text: 'old bytes\n',
    identical: false,
    target_relation: 'imported-unchanged',
    replace_allowed: true,
  };
  let previewParameters = null;
  let exportParameters = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return JSON.stringify([]);
    }
    if (command === 'inspect_managed_file') {
      return JSON.stringify({
        path: preview.path,
        text: preview.source_text,
        text_editable: true,
        modified_from_current_version: false,
        native_untracked: false,
        native_missing: false,
        current_version: preview.source_version,
        byte_count: preview.source_byte_count,
        content_digest: preview.source_content_digest,
        executable: preview.source_executable,
        installation: workspace.installation,
      });
    }
    if (command === 'preview_managed_export') {
      previewParameters = parameters;
      return JSON.stringify(preview);
    }
    if (command === 'export_managed_file') {
      exportParameters = parameters;
      return JSON.stringify({
        path: preview.path,
        target_root: preview.target_root,
        byte_count: preview.source_byte_count,
        content_digest: preview.source_content_digest,
        executable: preview.source_executable,
        created: !preview.target_exists,
      });
    }
    if (command === 'remember_managed_workspace') {
      assert.deepEqual(parameters, { path: workspace.root, exportRoot: preview.target_root });
      return JSON.stringify({
        remembered: workspace.root,
        workspaces: [workspace.root],
        auto_opened: false,
        active_folder: '/application/native-workspace/current',
        export_root: preview.target_root,
        warning: null,
      });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.CustomEvent = FakeCustomEvent;
  let browserConfirmations = 0;
  let acceptProjectedConfirmation = true;
  const confirmationProjections = [];
  document.addEventListener('mesh:confirmation-projection', (event) => {
    confirmationProjections.push(event.detail);
    document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-mounted', {
      detail: { generation: event.detail.generation },
    }));
    if (acceptProjectedConfirmation) {
      document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-intent', {
        detail: { generation: event.detail.generation, intent: { type: 'confirm' } },
      }));
    }
  });
  globalThis.confirm = () => {
    browserConfirmations += 1;
    throw new Error('single-file export must use the source-owned confirmation');
  };
  await import(`./app.js?export-exact-preview=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-available'));

  const file = document.destinationField('selectedFile');
  const target = document.destinationField('destination');
  file.value = preview.path;
  await file.emit('change');
  target.value = preview.target_root;
  await target.emit('input');
  await document.destinationActionControl('preview-single').emit('click');
  assert.deepEqual(previewParameters, {
    relativePath: preview.path,
    targetRoot: preview.target_root,
    expectedWorkspaceRoot: workspace.root,
    expectedWorkspaceDigest: workspace.digest,
    expectedWorkspaceInstallation: workspace.installation,
  });
  assert.equal(document.destinationActionControl('confirm-single').disabled, false);
  assert.match(document.destinationOutput.textContent, /Replace saved file: docs\/note\.txt/);
  assert.match(document.destinationOutput.textContent, /Saved text\n[-]+\nsaved bytes/);
  assert.match(document.destinationOutput.textContent, /Current ordinary-folder text\n[-]+\nold bytes/);
  assert.doesNotMatch(document.destinationOutput.textContent, /source_text|target_installation|[{}]/);

  acceptProjectedConfirmation = false;
  const staleConfirmation = document.destinationActionControl('confirm-single').emit('click');
  await waitFor(() => confirmationProjections.length === 1);
  target.value = '/ordinary/different';
  await target.emit('input');
  assert.equal(document.destinationActionControl('confirm-single').disabled, true);
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-intent', {
    detail: {
      generation: confirmationProjections[0].generation,
      intent: { type: 'confirm' },
    },
  }));
  await staleConfirmation;
  assert.equal(exportParameters, null, 'a stale accessible confirmation reached native export');
  assert.match(document.getElementById('notice').textContent, /destination plan changed while confirmation was open/);

  acceptProjectedConfirmation = true;
  target.value = preview.target_root;
  await target.emit('input');
  await document.destinationActionControl('preview-single').emit('click');
  await document.destinationActionControl('confirm-single').emit('click');

  assert.equal(browserConfirmations, 0);
  assert.equal(confirmationProjections.length, 2);
  assert.equal(confirmationProjections[1].confirmation.title, 'Replace saved docs/note.txt?');
  assert.equal(confirmationProjections[1].confirmation.confirmLabel, 'Replace saved file');
  assert.deepEqual(exportParameters, {
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
    expectedWorkspaceRoot: workspace.root,
    expectedWorkspaceDigest: workspace.digest,
    expectedWorkspaceInstallation: workspace.installation,
  });
  assert.match(document.getElementById('notice').textContent, /atomically/);
  assert.match(document.destinationHint.textContent, /Remembered destination folder: \/ordinary\/original/);
  assert.doesNotMatch(document.destinationHint.textContent, /Original folder ready/);
  assert.equal(document.destinationActionControl('confirm-single').disabled, true);

  preview.target_exists = false;
  preview.target_byte_count = null;
  preview.target_content_digest = null;
  preview.target_executable = null;
  preview.target_text = null;
  preview.target_file_installation = null;
  file.value = preview.path;
  await file.emit('change');
  target.value = preview.target_root;
  await target.emit('input');
  await document.destinationActionControl('preview-single').emit('click');
  assert.equal(document.destinationActionControl('confirm-single').textContent, 'Create file');
  assert.match(document.destinationOutput.textContent, /Create saved file: docs\/note\.txt/);
  assert.match(document.destinationOutput.textContent, /Current destination: file is absent/);
  await document.destinationActionControl('confirm-single').emit('click');
  assert.equal(browserConfirmations, 0);
  assert.equal(confirmationProjections.length, 3);
  assert.equal(confirmationProjections[2].confirmation.title, 'Create saved docs/note.txt?');
  assert.equal(confirmationProjections[2].confirmation.confirmLabel, 'Create saved file');
  assert.equal(exportParameters.expectedTargetDigest, null);
  assert.equal(exportParameters.expectedTargetExecutable, null);
  assert.match(document.getElementById('notice').textContent, /Created saved/);

  const lastExport = exportParameters;
  preview.target_exists = true;
  preview.target_byte_count = 16;
  preview.target_content_digest = 'other-agent-digest';
  preview.target_executable = false;
  preview.target_text = 'other agent work\n';
  preview.target_file_installation = 'other-agent-file';
  preview.target_relation = 'external-or-other-workspace';
  preview.replace_allowed = false;
  file.value = preview.path;
  await file.emit('change');
  target.value = preview.target_root;
  await target.emit('input');
  await document.destinationActionControl('preview-single').emit('click');
  assert.equal(document.destinationActionControl('confirm-single').disabled, true);
  assert.match(document.destinationOutput.textContent, /Keep existing ordinary file/);
  assert.match(document.destinationOutput.textContent, /will not replace work from another agent/);
  await document.destinationActionControl('confirm-single').emit('click');
  assert.equal(exportParameters, lastExport, 'an unproven replacement reached the native mutation');
});

test('a lost single-file export reply verifies the exact destination without replaying the write', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/lost-export-reply/mounts',
    digest: 'workspace-lost-export-reply',
    installation: 'installation-lost-export-reply',
    records: 2,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-lost-export-reply' },
    shared_version: null,
    entries: [{ path: 'report.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    workspace_versions: [],
    file_histories: [{
      object_id: 'object-lost-export-reply',
      path: 'report.txt',
      current: { version_id: 'version-lost-export-reply', manifest_id: 'manifest-lost-export-reply' },
      retained_versions: [{ version_id: 'version-lost-export-reply', manifest_id: 'manifest-lost-export-reply' }],
    }],
  };
  const targetRoot = '/ordinary/private-copy';
  let exported = false;
  let commitExport = true;
  let exportCalls = 0;
  let previewCalls = 0;
  let rememberCalls = 0;
  let rememberedExportRoot = null;
  const commands = [];
  const exportPreview = () => ({
    path: 'report.txt',
    source_version: workspace.private_version.version,
    source_byte_count: 13,
    source_content_digest: 'saved-report-digest',
    source_executable: false,
    source_text: 'saved report\n',
    target_root: targetRoot,
    target_installation: 'target-root-installation',
    target_parent_installation: 'target-parent-installation',
    target_file_installation: exported ? 'target-file-after-export' : 'target-file-before-export',
    target_exists: true,
    target_byte_count: exported ? 13 : 11,
    target_content_digest: exported ? 'saved-report-digest' : 'older-report-digest',
    target_executable: false,
    target_text: exported ? 'saved report\n' : 'old report\n',
    identical: exported,
    target_relation: exported ? 'saved-version' : 'imported-unchanged',
    replace_allowed: !exported,
  });
  const invoke = async (command, parameters = {}) => {
    commands.push(`${command}${parameters.method ? `:${parameters.method}` : ''}`);
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: workspace.root,
        workspaces: [workspace.root],
        workspace_entries: [{
          path: workspace.root,
          export_root: rememberedExportRoot,
          project_root: null,
          agent_handoff_installation: null,
          agent_handoff_generation: null,
        }],
        auto_opened: false,
        active_folder: '/application/native-workspace/current',
        export_root: rememberedExportRoot,
        warning: null,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'inspect_managed_file') {
      return JSON.stringify({
        path: 'report.txt',
        current_version: workspace.private_version.version,
        byte_count: 13,
        content_digest: 'saved-report-digest',
        executable: false,
        text: 'saved report\n',
        text_editable: true,
        modified_from_current_version: false,
        native_untracked: false,
      });
    }
    if (command === 'preview_managed_export') {
      previewCalls += 1;
      assert.equal(parameters.relativePath, 'report.txt');
      assert.equal(parameters.targetRoot, targetRoot);
      return JSON.stringify(exportPreview());
    }
    if (command === 'export_managed_file') {
      exportCalls += 1;
      if (commitExport) exported = true;
      throw new Error('the renderer lost the completed private export reply');
    }
    if (command === 'remember_managed_workspace') {
      rememberCalls += 1;
      rememberedExportRoot = parameters.exportRoot;
      return invoke('recent_workspace_status');
    }
    throw new Error(`unexpected lost-export command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?lost-single-export-reply=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  document.destinationField('selectedFile').value = 'report.txt';
  await document.destinationField('selectedFile').emit('change');
  document.destinationField('destination').value = targetRoot;
  await document.destinationField('destination').emit('input');
  await document.destinationActionControl('preview-single').emit('click');
  assert.equal(document.destinationActionControl('confirm-single').disabled, false);

  await document.destinationActionControl('confirm-single').emit('click');

  assert.equal(exportCalls, 1, 'ambiguous recovery replayed the destination write');
  assert.equal(
    previewCalls,
    2,
    `recovery did not read the exact post-error destination: ${document.getElementById('notice').textContent}; ${commands.join(', ')}`,
  );
  assert.equal(rememberedExportRoot, targetRoot);
  assert.equal(document.serviceState.state === 'ready', true);
  assert.equal(document.destinationActionControl('confirm-single').disabled, true);
  assert.match(document.getElementById('notice').textContent, /lost the private export reply/i);
  assert.match(document.getElementById('notice').textContent, /verified.+exact saved bytes/i);

  commitExport = false;
  exported = false;
  await document.destinationActionControl('preview-single').emit('click');
  assert.equal(document.destinationActionControl('confirm-single').disabled, false);
  await document.destinationActionControl('confirm-single').emit('click');

  assert.equal(exportCalls, 2, 'refused export recovery replayed the destination write');
  assert.equal(previewCalls, 4, 'refused export recovery did not inspect the unchanged destination');
  assert.equal(rememberCalls, 1, 'an unverified destination was remembered as a completed export');
  assert.match(document.getElementById('notice').textContent, /could not confirm the private export/i);
  assert.match(document.getElementById('notice').textContent, /Nothing was replayed/i);
});

test('a superseded pull-back preview cannot erase the newer destination plan', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/pull-back-race/mounts',
    digest: 'workspace-pull-back-race',
    installation: 'installation-pull-back-race',
    records: 2,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'private-pull-back-race' },
    shared_version: null,
    entries: [{ path: 'agent-result.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    workspace_versions: [],
    file_histories: [{
      object_id: 'object-agent-result',
      path: 'agent-result.txt',
      current: { version_id: 'version-agent-result', manifest_id: 'manifest-agent-result' },
      retained_versions: [],
    }],
  };
  const firstTarget = '/ordinary/first';
  const secondTarget = '/ordinary/second';
  let releaseFirst;
  const firstPreview = new Promise((resolve) => { releaseFirst = resolve; });
  const preview = (targetRoot) => JSON.stringify({
    path: 'agent-result.txt',
    source_version: 'version-agent-result',
    source_byte_count: 12,
    source_content_digest: 'source-agent-result',
    source_executable: false,
    source_text: 'agent result\n',
    target_root: targetRoot,
    target_installation: `target-${targetRoot}`,
    target_parent_installation: `parent-${targetRoot}`,
    target_file_installation: null,
    target_exists: false,
    target_byte_count: null,
    target_content_digest: null,
    target_executable: null,
    target_text: null,
    identical: false,
    target_relation: 'absent',
    replace_allowed: true,
  });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({ remembered: workspace.root, workspaces: [workspace.root], auto_opened: false, export_root: null });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: false,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'preview_managed_export') {
      return parameters.targetRoot === firstTarget ? firstPreview : preview(parameters.targetRoot);
    }
    throw new Error(`unexpected pull-back preview command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?pull-back-preview-supersession=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const file = document.destinationField('selectedFile');
  const target = document.destinationField('destination');
  const previewButton = document.destinationActionControl('preview-single');
  file.value = 'agent-result.txt';
  await file.emit('change');
  target.value = firstTarget;
  await target.emit('input');
  const staleRequest = previewButton.emit('click');

  target.value = secondTarget;
  await target.emit('input');
  await previewButton.emit('click');
  assert.equal(document.destinationActionControl('confirm-single').disabled, false);
  assert.match(document.destinationOutput.textContent, /Destination folder: \/ordinary\/second/);

  releaseFirst(preview(firstTarget));
  await staleRequest;
  assert.equal(
    document.destinationActionControl('confirm-single').disabled,
    false,
    'the older preview erased the newer reviewed destination plan',
  );
  assert.match(document.destinationOutput.textContent, /Destination folder: \/ordinary\/second/);
  assert.doesNotMatch(document.getElementById('notice').textContent, /selection changed|Preview it again/);
});

test('a superseded whole-workspace preview cannot erase the newer destination plan', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/pull-back-tree-race/mounts',
    digest: 'workspace-pull-back-tree-race',
    installation: 'installation-pull-back-tree-race',
    records: 2,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'private-pull-back-tree-race' },
    shared_version: null,
    entries: [{ path: 'agent-result.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    workspace_versions: [],
    file_histories: [{
      object_id: 'object-agent-result',
      path: 'agent-result.txt',
      current: { version_id: 'version-agent-result', manifest_id: 'manifest-agent-result' },
      retained_versions: [],
    }],
  };
  const firstTarget = '/ordinary/tree-first';
  const secondTarget = '/ordinary/tree-second';
  let releaseFirst;
  const firstPreview = new Promise((resolve) => { releaseFirst = resolve; });
  const preview = (targetRoot) => JSON.stringify({
    path: 'agent-result.txt',
    source_version: 'version-agent-result',
    source_byte_count: 12,
    source_content_digest: 'source-agent-result',
    source_executable: false,
    source_text: null,
    target_root: targetRoot,
    target_installation: `target-${targetRoot}`,
    target_parent_installation: `parent-${targetRoot}`,
    target_file_installation: null,
    target_exists: false,
    target_byte_count: null,
    target_content_digest: null,
    target_executable: null,
    target_text: null,
    identical: false,
    target_relation: 'absent',
    replace_allowed: true,
  });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({ remembered: workspace.root, workspaces: [workspace.root], auto_opened: false, export_root: null });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: false,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'preview_managed_exports') {
      const encoded = parameters.targetRoot === firstTarget
        ? await firstPreview
        : preview(parameters.targetRoot);
      return JSON.stringify([JSON.parse(encoded)]);
    }
    throw new Error(`unexpected whole-workspace pull-back command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?pull-back-tree-preview-supersession=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const target = document.destinationField('destination');
  const previewButton = document.destinationActionControl('preview-all');
  target.value = firstTarget;
  await target.emit('input');
  const staleRequest = previewButton.emit('click');
  target.value = secondTarget;
  await target.emit('input');
  await previewButton.emit('click');
  assert.equal(document.destinationActionControl('confirm-batch').disabled, false);
  assert.match(document.destinationOutput.textContent, /Destination folder: \/ordinary\/tree-second/);

  releaseFirst(preview(firstTarget));
  await staleRequest;
  assert.equal(
    document.destinationActionControl('confirm-batch').disabled,
    false,
    'the older whole-workspace preview erased the newer reviewed destination plan',
  );
  assert.match(document.destinationOutput.textContent, /Destination folder: \/ordinary\/tree-second/);
  assert.doesNotMatch(document.getElementById('notice').textContent, /destination folder changed|Preview it again/);
});

test('one reviewed batch exports only changed files in stable path order', async () => {
  const document = fakeDocument();
  const targetRoot = '/ordinary/original';
  const workspace = {
    root: '/managed/project/mounts',
    digest: 'workspace-batch-export',
    installation: 'installation-batch-export',
    records: 5,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'private-batch-export' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    workspace_versions: [],
    file_histories: ['b.txt', 'a.txt', 'nested/c.txt'].map((path) => ({
      object_id: `object-${path}`,
      path,
      current: { version_id: `version-${path}`, manifest_id: `manifest-${path}` },
      retained_versions: [],
    })),
  };
  const previewFor = (path) => ({
    path,
    source_version: `version-${path}`,
    source_byte_count: 10,
    source_content_digest: `source-${path}`,
    source_executable: false,
    source_text: `saved ${path}`,
    target_root: targetRoot,
    target_installation: 'target-root',
    target_parent_installation: `target-parent-${path}`,
    target_file_installation: path === 'nested/c.txt' ? null : `target-file-${path}`,
    target_exists: path !== 'nested/c.txt',
    target_byte_count: path === 'nested/c.txt' ? null : 8,
    target_content_digest: path === 'nested/c.txt' ? null : `target-${path}`,
    target_executable: path === 'nested/c.txt' ? null : false,
    target_text: path === 'nested/c.txt' ? null : `old ${path}`,
    identical: path === 'b.txt',
    target_relation: path === 'b.txt' ? 'identical' : path === 'nested/c.txt' ? 'absent' : 'imported-unchanged',
    replace_allowed: path !== 'b.txt',
  });
  const previewed = [];
  let batchPreviews = 0;
  const exported = [];
  let confirmations = 0;
  let failRetiredDiscovery = false;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({ remembered: workspace.root, workspaces: [workspace.root], auto_opened: false, export_root: targetRoot });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'preview_managed_exports') {
      batchPreviews += 1;
      const paths = ['a.txt', 'b.txt', 'nested/c.txt'];
      previewed.push(...paths);
      return JSON.stringify(paths.map((path) => ({
        ...previewFor(path),
        source_text: null,
        target_text: null,
      })));
    }
    if (command === 'preview_managed_export') {
      previewed.push(parameters.relativePath);
      return JSON.stringify(previewFor(parameters.relativePath));
    }
    if (command === 'export_managed_file') {
      exported.push(parameters.relativePath);
      return JSON.stringify({ path: parameters.relativePath, target_root: targetRoot, created: parameters.relativePath === 'nested/c.txt' });
    }
    if (command === 'discover_retired_exports') {
      if (failRetiredDiscovery) throw new Error('cleanup discovery unavailable');
      return JSON.stringify([]);
    }
    if (command === 'remember_managed_workspace') {
      assert.deepEqual(parameters, { path: workspace.root, exportRoot: targetRoot });
      return JSON.stringify({ remembered: workspace.root, workspaces: [workspace.root], auto_opened: false, export_root: targetRoot });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => { confirmations += 1; return true; };
  await import(`./app.js?batch-export-success=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  const confirmationProjections = [];
  document.addEventListener('mesh:confirmation-projection', (event) => {
    confirmationProjections.push(event.detail);
    document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-mounted', {
      detail: { generation: event.detail.generation },
    }));
    document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-intent', {
      detail: { generation: event.detail.generation, intent: { type: 'confirm' } },
    }));
  });
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-available'));

  assert.equal(document.destinationActionControl('preview-all').disabled, false);
  assert.equal(document.destinationField('destination').value, targetRoot);
  await document.destinationField('destination').emit('keydown', { key: 'Enter' });
  await waitFor(() => document.destinationActionControl('confirm-batch').textContent === 'Update 2 changed files');
  assert.equal(batchPreviews, 1, 'whole-workspace update used per-file native round trips');
  assert.deepEqual(previewed, ['a.txt', 'b.txt', 'nested/c.txt']);
  assert.equal(document.destinationActionControl('confirm-batch').textContent, 'Update 2 changed files');
  assert.equal(document.destinationActionControl('confirm-batch').disabled, false);
  assert.doesNotMatch(document.destinationOutput.textContent, /source_text/);
  assert.match(document.destinationOutput.textContent, /Update 2 saved files/);
  assert.match(document.destinationOutput.textContent, /Replace  a\.txt/);
  assert.match(document.destinationOutput.textContent, /Create   nested\/c\.txt/);
  assert.match(document.destinationOutput.textContent, /Already identical: 1 file/);
  assert.doesNotMatch(document.destinationOutput.textContent, /source_content_digest|[{}]/);

  document.destinationField('selectedFile').value = 'a.txt';
  await document.destinationField('selectedFile').emit('change');
  await document.destinationActionControl('preview-single').emit('click');
  assert.equal(document.destinationActionControl('confirm-single').disabled, false);
  assert.equal(document.destinationActionControl('confirm-batch').disabled, true, 'single preview retained stale batch authority');
  previewed.length = 0;
  await document.destinationActionControl('preview-all').emit('click');
  assert.equal(batchPreviews, 2, 'the refreshed whole-workspace preview did not stay batched');
  assert.deepEqual(previewed, ['a.txt', 'b.txt', 'nested/c.txt']);

  await document.destinationActionControl('confirm-batch').emit('click');
  assert.equal(confirmations, 0);
  assert.equal(confirmationProjections.length, 1);
  assert.equal(confirmationProjections[0].confirmation.title, 'Update 2 proven files?');
  assert.equal(confirmationProjections[0].confirmation.confirmLabel, 'Update 2 changed files');
  assert.deepEqual(exported, ['a.txt', 'nested/c.txt']);
  assert.match(document.getElementById('notice').textContent, /Saved changes, moves, and deletions are applied/);
  assert.equal(document.destinationActionControl('confirm-batch').disabled, true);

  failRetiredDiscovery = true;
  await document.destinationActionControl('preview-all').emit('click');
  await document.destinationActionControl('confirm-batch').emit('click');
  assert.deepEqual(exported, ['a.txt', 'nested/c.txt', 'a.txt', 'nested/c.txt']);
  assert.match(document.getElementById('notice').textContent, /Updated 2 proven files/);
  assert.match(document.getElementById('notice').textContent, /could not prepare the next cleanup review/);
  assert.match(document.getElementById('notice').textContent, /completed destination changes remain in place/);
  assert.equal(document.getElementById('notice').classList.contains('error'), true);
});

test('a second agent pull-back preserves another agent result while applying its own file', async () => {
  const document = fakeDocument();
  const targetRoot = '/ordinary/shared';
  const workspace = {
    root: '/managed/agent-b/mounts',
    digest: 'workspace-agent-b',
    installation: 'installation-agent-b',
    records: 3,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'private-agent-b' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    workspace_versions: [],
    file_histories: ['a.txt', 'b.txt'].map((path) => ({
      object_id: `object-${path}`,
      path,
      current: { version_id: `version-${path}`, manifest_id: `manifest-${path}` },
      retained_versions: [],
    })),
  };
  const exported = [];
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ remembered: workspace.root, workspaces: [workspace.root], auto_opened: false, export_root: targetRoot });
    if (command === 'managed_checkpoint_state') return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'preview_managed_exports') {
      return JSON.stringify(['a.txt', 'b.txt'].map((path) => {
        const otherAgent = path === 'a.txt';
        return {
          path,
          source_version: `version-${path}`,
          source_byte_count: 10,
          source_content_digest: `source-${path}`,
          source_executable: false,
          source_text: null,
          target_root: targetRoot,
          target_installation: 'target-root',
          target_parent_installation: 'target-root',
          target_file_installation: `file-${path}`,
          target_exists: true,
          target_byte_count: 14,
          target_content_digest: `target-${path}`,
          target_executable: false,
          target_text: null,
          identical: false,
          target_relation: otherAgent ? 'external-or-other-workspace' : 'imported-unchanged',
          replace_allowed: !otherAgent,
        };
      }));
    }
    if (command === 'export_managed_file') {
      exported.push(parameters.relativePath);
      return JSON.stringify({ path: parameters.relativePath, target_root: targetRoot, created: false });
    }
    if (command === 'discover_retired_exports') return JSON.stringify([]);
    if (command === 'remember_managed_workspace') return JSON.stringify({ remembered: workspace.root, workspaces: [workspace.root], auto_opened: false, export_root: targetRoot });
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?multi-agent-pull-back=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.destinationActionControl('preview-all').emit('click');
  assert.equal(document.destinationActionControl('confirm-batch').textContent, 'Update 1 changed file');
  assert.match(document.destinationOutput.textContent, /Replace  b\.txt/);
  assert.match(document.destinationOutput.textContent, /Keep for manual review/);
  assert.match(document.destinationOutput.textContent, /Keep     a\.txt — changed outside this workspace/);
  await document.destinationActionControl('confirm-batch').emit('click');
  assert.deepEqual(exported, ['b.txt']);
  assert.match(document.getElementById('notice').textContent, /preserved 1 changed or conflicting ordinary-folder path/);
});

test('whole-tree export creates reviewed folders before re-previewing nested files', async () => {
  const document = fakeDocument();
  const targetRoot = '/ordinary/original';
  const workspace = {
    root: '/managed/project/mounts',
    digest: 'workspace-tree-export',
    installation: 'installation-tree-export',
    records: 7,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'private-tree-export' },
    shared_version: null,
    entries: [
      { path: 'generated', type: 'folder' },
      { path: 'generated/reports', type: 'folder' },
      { path: 'generated/reports/result.txt', type: 'file' },
    ],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    workspace_versions: [],
    file_histories: [{
      object_id: 'object-result',
      path: 'generated/reports/result.txt',
      current: { version_id: 'version-result', manifest_id: 'manifest-result' },
      retained_versions: [],
    }],
  };
  const createdDirectories = new Set();
  let directoryBatchPreviews = 0;
  let directoryBatchExports = 0;
  const exportedFiles = [];
  let confirmations = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ remembered: workspace.root, workspaces: [workspace.root], auto_opened: false, export_root: targetRoot });
    if (command === 'managed_checkpoint_state') return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'preview_managed_directory_exports') {
      directoryBatchPreviews += 1;
      return JSON.stringify({
        target_root: targetRoot,
        target_installation: 'target-root',
        missing_paths: ['generated', 'generated/reports'].filter((path) => !createdDirectories.has(path)),
      });
    }
    if (command === 'export_managed_directories') {
      directoryBatchExports += 1;
      assert.deepEqual(parameters.paths, ['generated', 'generated/reports']);
      const completed = parameters.paths.map((path) => {
        createdDirectories.add(path);
        return { path, target_root: targetRoot, installation: `directory-${path}`, created: true };
      });
      return JSON.stringify({ completed, failure: null });
    }
    if (command === 'preview_managed_exports') return JSON.stringify([{
      path: 'generated/reports/result.txt',
      source_version: 'version-result',
      source_byte_count: 13,
      source_content_digest: 'source-result',
      source_executable: false,
      source_text: 'agent result\n',
      target_root: targetRoot,
      target_installation: 'target-root',
      target_parent_installation: 'directory-generated/reports',
      target_file_installation: null,
      target_exists: false,
      target_byte_count: null,
      target_content_digest: null,
      target_executable: null,
      target_text: null,
      identical: false,
      target_relation: 'absent',
      replace_allowed: true,
    }]);
    if (command === 'export_managed_file') {
      exportedFiles.push(parameters.relativePath);
      return JSON.stringify({ path: parameters.relativePath, target_root: targetRoot, created: true });
    }
    if (command === 'discover_retired_exports') return JSON.stringify([]);
    if (command === 'remember_managed_workspace') return JSON.stringify({ remembered: workspace.root, workspaces: [workspace.root], auto_opened: false, export_root: targetRoot });
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => { confirmations += 1; return true; };
  await import(`./app.js?tree-export=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.destinationActionControl('preview-all').emit('click');
  assert.equal(directoryBatchPreviews, 1);
  assert.equal(document.destinationActionControl('confirm-batch').textContent, 'Create 2 saved folders');
  assert.match(document.destinationOutput.textContent, /generated\/reports/);

  await document.destinationActionControl('confirm-batch').emit('click');
  assert.equal(confirmations, 1);
  assert.equal(directoryBatchExports, 1);
  assert.equal(directoryBatchPreviews, 2, 'folder re-preview after creation was not one batch');
  assert.deepEqual([...createdDirectories], ['generated', 'generated/reports']);
  assert.equal(document.destinationActionControl('confirm-batch').textContent, 'Update 1 changed file');
  assert.match(document.destinationOutput.textContent, /generated\/reports\/result\.txt/);

  await document.destinationActionControl('confirm-batch').emit('click');
  assert.equal(confirmations, 2);
  assert.deepEqual(exportedFiles, ['generated/reports/result.txt']);
  assert.match(document.getElementById('notice').textContent, /Saved changes, moves, and deletions are applied/);
});

test('whole-tree export completes cleanly for saved empty folders without file history', async () => {
  const document = fakeDocument();
  const targetRoot = '/ordinary/original';
  const workspace = {
    root: '/managed/empty-tree/mounts',
    digest: 'workspace-empty-tree',
    installation: 'installation-empty-tree',
    records: 3,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'private-empty-tree' },
    shared_version: null,
    entries: [
      { path: 'empty', type: 'folder' },
      { path: 'empty/nested', type: 'folder' },
    ],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    workspace_versions: [],
    file_histories: [],
  };
  const createdDirectories = new Set();
  let directoryBatchPreviews = 0;
  let directoryBatchExports = 0;
  let confirmations = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ remembered: workspace.root, workspaces: [workspace.root], auto_opened: false, export_root: targetRoot });
    if (command === 'managed_checkpoint_state') return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'preview_managed_directory_exports') {
      directoryBatchPreviews += 1;
      return JSON.stringify({
        target_root: targetRoot,
        target_installation: 'target-root',
        missing_paths: ['empty', 'empty/nested'].filter((path) => !createdDirectories.has(path)),
      });
    }
    if (command === 'export_managed_directories') {
      directoryBatchExports += 1;
      const completed = parameters.paths.map((path) => {
        createdDirectories.add(path);
        return { path, target_root: targetRoot, installation: `directory-${path}`, created: true };
      });
      return JSON.stringify({ completed, failure: null });
    }
    if (command === 'discover_retired_exports') return JSON.stringify([]);
    if (command === 'remember_managed_workspace') return JSON.stringify({ remembered: workspace.root, workspaces: [workspace.root], auto_opened: false, export_root: targetRoot });
    throw new Error(`unexpected empty-tree command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => { confirmations += 1; return true; };
  await import(`./app.js?empty-tree-export=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.equal(document.destinationActionControl('preview-all').disabled, false);
  assert.equal(document.destinationField('selectedFile').disabled, true);
  assert.match(document.destinationHint.textContent, /reviewed empty folders/);

  await document.destinationActionControl('preview-all').emit('click');
  assert.equal(document.destinationActionControl('confirm-batch').textContent, 'Create 2 saved folders');
  await document.destinationActionControl('confirm-batch').emit('click');

  assert.equal(confirmations, 1);
  assert.equal(directoryBatchPreviews, 2);
  assert.equal(directoryBatchExports, 1);
  assert.deepEqual([...createdDirectories], ['empty', 'empty/nested']);
  assert.equal(document.getElementById('notice').classList.contains('error'), false);
  assert.match(document.getElementById('notice').textContent, /Saved changes, moves, and deletions are applied/);
  assert.match(document.destinationOutput.textContent, /Destination-folder update complete/);
  assert.equal(document.destinationActionControl('confirm-batch').disabled, true);
});

test('saved rename pull-back installs the new path then separately removes only unchanged old paths', async () => {
  const document = fakeDocument();
  const targetRoot = '/ordinary/original';
  const workspace = {
    root: '/managed/renamed/mounts',
    digest: 'workspace-renamed',
    installation: 'installation-renamed',
    records: 4,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'private-renamed' },
    shared_version: null,
    entries: [{ path: 'new.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    workspace_versions: [],
    file_histories: [{
      object_id: 'object-new',
      path: 'new.txt',
      current: { version_id: 'version-new', manifest_id: 'manifest-new' },
      retained_versions: [],
    }],
  };
  const removed = new Set();
  const exported = [];
  const removalOrder = [];
  let confirmations = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ remembered: workspace.root, workspaces: [workspace.root], auto_opened: false, export_root: targetRoot });
    if (command === 'managed_checkpoint_state') return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'preview_managed_exports') return JSON.stringify([{
      path: 'new.txt', source_version: 'version-new', source_byte_count: 4,
      source_content_digest: 'saved-new', source_executable: false, source_text: null,
      target_root: targetRoot, target_installation: 'root-id', target_parent_installation: 'root-id',
      target_file_installation: null, target_exists: false, target_byte_count: null,
      target_content_digest: null, target_executable: null, target_text: null, identical: false,
      target_relation: 'absent', replace_allowed: true,
    }]);
    if (command === 'export_managed_file') {
      exported.push(parameters.relativePath);
      return JSON.stringify({ path: parameters.relativePath, target_root: targetRoot, created: true });
    }
    if (command === 'discover_retired_exports') return JSON.stringify([
      { path: 'old.txt', type: 'file' },
      { path: 'changed.txt', type: 'file' },
      { path: 'nonempty-folder', type: 'folder' },
      { path: 'old-folder', type: 'folder' },
    ]);
    if (command === 'preview_retired_export') {
      const absent = removed.has(parameters.relativePath);
      const changed = parameters.relativePath === 'changed.txt';
      const folder = parameters.relativePath.endsWith('-folder');
      const nonempty = parameters.relativePath === 'nonempty-folder';
      return JSON.stringify({
        path: parameters.relativePath,
        type: folder ? 'folder' : 'file',
        source_version: folder ? null : `version-${parameters.relativePath}`,
        source_content_digest: folder ? null : `saved-${parameters.relativePath}`,
        source_executable: folder ? null : false,
        target_root: targetRoot,
        target_installation: 'root-id',
        target_parent_installation: 'root-id',
        target_entry_installation: absent ? null : `entry-${parameters.relativePath}`,
        target_content_digest: folder || absent ? null : `target-${parameters.relativePath}`,
        target_executable: folder || absent ? null : false,
        removable: !absent && !changed && !nonempty,
        status: absent ? 'already-absent' : changed ? 'changed-preserved' : nonempty ? 'nonempty-preserved' : folder ? 'empty-old-folder' : 'unchanged-old-file',
      });
    }
    if (command === 'remove_retired_export') {
      removalOrder.push(parameters.relativePath);
      removed.add(parameters.relativePath);
      return JSON.stringify({ path: parameters.relativePath, type: parameters.expectedEntryType, target_root: targetRoot, removed: true });
    }
    if (command === 'remember_managed_workspace') return JSON.stringify({ remembered: workspace.root, workspaces: [workspace.root], auto_opened: false, export_root: targetRoot });
    throw new Error(`unexpected rename pull-back command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => { confirmations += 1; return true; };
  await import(`./app.js?rename-pull-back=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.destinationActionControl('preview-all').emit('click');
  assert.equal(document.destinationActionControl('confirm-batch').textContent, 'Update 1 changed file');
  await document.destinationActionControl('confirm-batch').emit('click');
  assert.deepEqual(exported, ['new.txt']);
  assert.equal(document.destinationActionControl('confirm-batch').textContent, 'Remove 1 unchanged old file');
  assert.match(document.destinationOutput.textContent, /changed since the last update/);

  await document.destinationActionControl('confirm-batch').emit('click');
  assert.deepEqual(removalOrder, ['old.txt']);
  assert.equal(document.destinationActionControl('confirm-batch').textContent, 'Remove 1 old empty folder');
  assert.match(document.destinationOutput.textContent, /folder is not empty/);
  await document.destinationActionControl('confirm-batch').emit('click');

  assert.deepEqual(removalOrder, ['old.txt', 'old-folder']);
  assert.equal(confirmations, 3);
  assert.match(document.getElementById('notice').textContent, /preserved 2 changed or conflicting ordinary-folder paths/);
  assert.equal(document.destinationActionControl('confirm-batch').disabled, true);
});

test('batch export stops on a stale later file and reports the completed prefix', async () => {
  const document = fakeDocument();
  const targetRoot = '/ordinary/original';
  const paths = ['a.txt', 'b.txt'];
  const workspace = {
    root: '/managed/project/mounts',
    digest: 'workspace-batch-race',
    installation: 'installation-batch-race',
    records: 4,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'private-batch-race' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    workspace_versions: [],
    file_histories: paths.map((path) => ({ object_id: `object-${path}`, path, current: { version_id: `version-${path}`, manifest_id: `manifest-${path}` }, retained_versions: [] })),
  };
  const attempts = [];
  let remembered = false;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ remembered: workspace.root, workspaces: [workspace.root], auto_opened: false, export_root: targetRoot });
    if (command === 'managed_checkpoint_state') return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'preview_managed_exports') return JSON.stringify(paths.map((path) => ({
      path,
      source_version: `version-${path}`,
      source_byte_count: 10,
      source_content_digest: `source-${path}`,
      source_executable: false,
      source_text: null,
      target_root: targetRoot,
      target_installation: 'target-root',
      target_parent_installation: `parent-${path}`,
      target_file_installation: `file-${path}`,
      target_exists: true,
      target_byte_count: 8,
      target_content_digest: `target-${path}`,
      target_executable: false,
      target_text: null,
      identical: false,
      target_relation: 'imported-unchanged',
      replace_allowed: true,
    })));
    if (command === 'export_managed_file') {
      attempts.push(parameters.relativePath);
      if (parameters.relativePath === 'b.txt') throw new Error('the export destination changed after preview');
      return JSON.stringify({ path: parameters.relativePath, target_root: targetRoot, created: false });
    }
    if (command === 'remember_managed_workspace') {
      remembered = true;
      return JSON.stringify({ remembered: workspace.root, workspaces: [workspace.root], auto_opened: false, export_root: targetRoot });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?batch-export-race=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  await document.destinationActionControl('preview-all').emit('click');
  await document.destinationActionControl('confirm-batch').emit('click');

  assert.deepEqual(attempts, paths);
  assert.equal(remembered, true, 'the successful exported prefix did not retain its ordinary-folder hint');
  assert.match(document.getElementById('notice').textContent, /Mesh stopped at b\.txt after updating 1 file/);
  assert.match(document.getElementById('notice').textContent, /The completed prefix remains in place/);
  assert.match(document.getElementById('notice').textContent, /Inspect that destination/);
  assert.match(document.getElementById('notice').textContent, /preview the remaining current tree again/);
  assert.equal(document.serviceState.state === 'ready', true);
  assert.equal(document.destinationActionControl('confirm-batch').disabled, true);
});

test('starting another agent always requests a fresh verified native folder', async () => {
  const document = fakeDocument();
  const operation = '10'.repeat(32);
  const current = {
    root: '/application/workspace-versions/current.mesh/mounts',
    digest: 'workspace-current-agent',
    installation: 'installation-current-agent',
    records: 2,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-current-agent', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  const fork = {
    ...current,
    root: '/application/workspace-versions/point-101010101010-2.mesh/mounts',
    digest: 'workspace-second-agent',
    installation: 'installation-second-agent',
  };
  const stableFolder = '/application/native-workspace/current';
  let open = current;
  let forkParameters = null;
  let codexParameters = null;
  let workspaceStateReads = 0;
  let releaseSwitchCheck;
  let markSwitchCheckStarted;
  const switchCheckStarted = new Promise((resolve) => {
    markSwitchCheckStarted = resolve;
  });
  const switchCheckHeld = new Promise((resolve) => {
    releaseSwitchCheck = resolve;
  });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: current.root,
        workspaces: [current.root],
        workspace_entries: [{
          path: current.root,
          export_root: '/ordinary/original',
          project_root: '/ordinary/original',
        }],
        auto_opened: false,
        active_folder: stableFolder,
        export_root: '/ordinary/original',
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: open.root,
        workspace_digest: open.digest,
        workspace_installation: open.installation,
        native_folder: true,
        native_folder_path: open.root,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      workspaceStateReads += 1;
      if (workspaceStateReads === 2) {
        markSwitchCheckStarted();
        await switchCheckHeld;
      }
      return JSON.stringify(open);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'open_fresh_agent_workspace') {
      forkParameters = parameters;
      open = fork;
      return JSON.stringify({
        source_version: operation,
        source_ordinal: 1,
        destination: fork.root,
        reused: false,
        workspace: fork,
        navigation: {
          remembered: fork.root,
          workspaces: [fork.root, current.root],
          workspace_entries: [
            {
              path: fork.root,
              export_root: '/ordinary/original',
              project_root: '/ordinary/original',
              source_point_ordinal: 1,
            },
            {
              path: current.root,
              export_root: '/ordinary/original',
              project_root: '/ordinary/original',
            },
          ],
          auto_opened: false,
          active_folder: stableFolder,
          export_root: '/ordinary/original',
          warning: null,
          build_revision: 'development',
          build_exact: false,
        },
      });
    }
    if (command === 'open_managed_workspace_in_codex') {
      codexParameters = parameters;
      return JSON.stringify({ path: open.root, workspace_installation: open.installation, fixed_workspace_path: true, agent_handoff_recorded: true, agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION, agent: 'Codex' });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  let finishConfirmations = 0;
  globalThis.confirm = () => {
    finishConfirmations += 1;
    return false;
  };
  await import(`./app.js?start-isolated-agent=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  assert.equal(document.workspaceCurrentAction('start-agent-copy')?.enabled, true, JSON.stringify({
    root: (document.workspaceCurrent?.current?.agentFolder ?? ''),
    version: (document.workspaceCurrent?.current?.privateVersion ?? ''),
    notice: document.getElementById('notice').textContent,
  }));
  assert.equal((document.workspaceCurrentAction('update-destination')?.label ?? ''), 'Update original folder');
  assert.equal(!document.workspaceCurrentAction('update-destination')?.enabled, true);
  assert.equal(document.workspaceDestination.destination.destination, '/ordinary/original');
  const firstStart = document.emitWorkspaceCurrentIntent('start-agent-copy');
  await switchCheckStarted;
  await firstStart;
  assert.equal(document.workspaceCurrentAction('start-agent-copy')?.enabled, false, 'the agent action remained enabled during its safety refresh');
  const readsBeforeDisabledIntent = workspaceStateReads;
  const noticeBeforeDisabledIntent = document.getElementById('notice').textContent;
  await document.emitWorkspaceCurrentIntent('start-agent-copy');
  assert.equal(workspaceStateReads, readsBeforeDisabledIntent, 'a disabled React action started another safety refresh');
  assert.equal(forkParameters, null, 'a second agent folder was created while the first safety refresh was pending');
  assert.equal(document.getElementById('notice').textContent, noticeBeforeDisabledIntent);
  releaseSwitchCheck();
  await waitFor(() => forkParameters !== null && codexParameters !== null);

  assert.ok(forkParameters, document.getElementById('notice').textContent);
  assert.deepEqual(forkParameters, {
    operation,
    exportRoot: '/ordinary/original',
    expectedWorkspaceRoot: current.root,
    expectedWorkspaceDigest: current.digest,
    expectedWorkspaceInstallation: current.installation,
  });
  assert.deepEqual(codexParameters, {
    expectedWorkspaceRoot: fork.root,
    expectedWorkspaceDigest: fork.digest,
    expectedWorkspaceInstallation: fork.installation,
    confirmedReopen: false,
    expectedAgentHandoffGeneration: null,
  });
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), fork.root);
  assert.match(document.getElementById('notice').textContent, /independent writable folder/);
  assert.match(document.getElementById('notice').textContent, /stays on this version/);
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Agent folder is assigned');
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Finish agent handoff');
  await document.emitWorkspaceCurrentIntent('finish-agent');
  assert.equal(
    finishConfirmations,
    1,
    'the successful fresh-agent launch discarded its native assignment generation and dead-ended Finish agent handoff until Refresh',
  );
  assert.doesNotMatch(document.getElementById('notice').textContent, /cannot identify this exact agent assignment yet/);
  assert.deepEqual(
    document.getElementById('recent-workspace').children.map((option) => option.textContent),
    [
      'Current · original · Copy of saved point 1 · Working copy 2',
      'original · Managed workspace',
    ],
    'two workspaces for the same project were still presented as opaque private paths',
  );
  assert.deepEqual(
    document.getElementById('recent-workspace').children.map((option) => option.title),
    [fork.root, current.root],
    'the exact independent folders must remain inspectable without dominating the label',
  );
});

test('switching saved points leaves an assigned agent folder pinned without inspecting live bytes', async () => {
  const document = fakeDocument();
  const operation = '4f'.repeat(32);
  const stableFolder = '/application/native-workspace/current';
  const current = {
    root: '/application/workspace-versions/agent-owned.mesh/mounts',
    digest: 'workspace-agent-owned',
    installation: 'installation-agent-owned',
    records: 3,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-agent-owned', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'agent-live.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [{
      path: 'agent-live.txt',
      object_id: 'object-agent-live',
      current: { version_id: 'version-agent-live', manifest_id: 'manifest-agent-live' },
      retained_versions: [{ version_id: 'version-agent-live', manifest_id: 'manifest-agent-live' }],
    }],
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  const earlier = {
    ...current,
    root: '/application/workspace-versions/point-4f4f4f4f4f4f.mesh/mounts',
    digest: 'workspace-earlier-independent',
    installation: 'installation-earlier-independent',
    private_version: { version: 'version-earlier-independent', concurrent_changes: 1 },
    entries: [{ path: 'saved.txt', type: 'file' }],
    file_histories: [],
  };
  let open = current;
  let switching = false;
  let departureInspections = 0;
  let openParameters = null;
  const entries = () => [
    {
      path: open.root,
      export_root: '/ordinary/original',
      project_root: '/ordinary/original',
      source_point_ordinal: open.root === earlier.root ? 1 : null,
    },
    ...(open.root === current.root ? [] : [{
      path: current.root,
      export_root: '/ordinary/original',
      project_root: '/ordinary/original',
      agent_handoff_installation: current.installation,
      agent_handoff_directory: '41:73',
      agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
    }]),
  ];
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: open.root,
        workspaces: entries().map((entry) => entry.path),
        workspace_entries: open.root === current.root
          ? [{
            ...entries()[0],
            agent_handoff_installation: current.installation,
            agent_handoff_directory: '41:73',
            agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
          }]
          : entries(),
        auto_opened: false,
        active_folder: stableFolder,
        export_root: '/ordinary/original',
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: open.root,
        workspace_digest: open.digest,
        workspace_installation: open.installation,
        native_folder: true,
        native_folder_path: open.root,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      if (switching && open.root === current.root) {
        departureInspections += 1;
        throw new Error('the assigned agent is replacing this file right now');
      }
      return JSON.stringify(open);
    }
    if (
      command === 'discover_native_directories'
      || command === 'discover_native_missing_files'
      || command === 'inspect_managed_file'
    ) {
      if (switching && open.root === current.root) {
        departureInspections += 1;
        throw new Error('live agent bytes are not stable yet');
      }
      return command === 'inspect_managed_file'
        ? JSON.stringify({
          path: 'agent-live.txt',
          text: 'stable before handoff\n',
          text_editable: true,
          modified_from_current_version: false,
          current_version: 'version-agent-live',
          byte_count: 22,
          content_digest: 'digest-agent-live',
          executable: false,
        })
        : '[]';
    }
    if (command === 'preview_managed_workspace_version') {
      return savedWorkspacePreview(operation, [{ path: 'saved.txt', type: 'file', bytes: '12' }]);
    }
    if (command === 'open_managed_workspace_version') {
      openParameters = parameters;
      open = earlier;
      return JSON.stringify({
        source_version: operation,
        source_ordinal: 1,
        destination: earlier.root,
        reused: false,
        workspace: earlier,
        navigation: {
          remembered: earlier.root,
          workspaces: [earlier.root, current.root],
          workspace_entries: entries(),
          auto_opened: false,
          active_folder: stableFolder,
          export_root: '/ordinary/original',
          warning: null,
          build_revision: 'development',
          build_exact: false,
        },
      });
    }
    if (command === 'reveal_managed_workspace') {
      return JSON.stringify({
        path: stableFolder,
        workspace_root: earlier.root,
        stable: true,
        native_folder: true,
      });
    }
    throw new Error(`unexpected assigned-switch command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?agent-pinned-version-switch=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  document.getElementById('workspace-version').value = operation;
  await document.getElementById('workspace-version').emit('change');
  switching = true;
  await document.getElementById('fork-version').emit('click');

  assert.equal(departureInspections, 0, 'version navigation tried to stabilize the live agent folder');
  assert.deepEqual(openParameters, {
    operation,
    destination: null,
    exportRoot: '/ordinary/original',
    expectedWorkspaceRoot: current.root,
    expectedWorkspaceDigest: current.digest,
    expectedWorkspaceInstallation: current.installation,
  });
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), earlier.root);
  assert.equal((document.workspaceCurrent?.current?.workingFolder ?? ''), stableFolder);
  assert.equal(
    document.getElementById('recent-workspace').children
      .some((option) => /Agent assigned/.test(option.textContent) && option.title === current.root),
    true,
    'the pinned agent folder lost its custody marker after navigation moved elsewhere',
  );
  assert.match(document.getElementById('notice').textContent, /Long-running agents remain pinned/);
});

test('a fresh agent folder stays collision protected when its Codex reply is ambiguous', async () => {
  const document = fakeDocument();
  const operation = '11'.repeat(32);
  const current = {
    root: '/application/workspace-versions/current-ambiguous.mesh/mounts',
    digest: 'workspace-current-ambiguous-agent',
    installation: 'installation-current-ambiguous-agent',
    records: 2,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-current-ambiguous-agent', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  const fork = {
    ...current,
    root: '/application/workspace-versions/point-111111111111-2.mesh/mounts',
    digest: 'workspace-fresh-ambiguous-agent',
    installation: 'installation-fresh-ambiguous-agent',
  };
  let open = current;
  let codexCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: current.root,
        workspaces: [current.root],
        workspace_entries: [{ path: current.root, export_root: null, project_root: null }],
        auto_opened: false,
        active_folder: '/application/native-workspace/current',
        export_root: null,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: open.root,
        workspace_digest: open.digest,
        workspace_installation: open.installation,
        native_folder: true,
        native_folder_path: open.root,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(open);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'open_fresh_agent_workspace') {
      open = fork;
      return JSON.stringify({
        source_version: operation,
        source_ordinal: 1,
        destination: fork.root,
        reused: false,
        workspace: fork,
        navigation: {
          remembered: fork.root,
          workspaces: [fork.root, current.root],
          workspace_entries: [
            { path: fork.root, export_root: null, project_root: null },
            { path: current.root, export_root: null, project_root: null },
          ],
          auto_opened: false,
          active_folder: '/application/native-workspace/current',
          export_root: null,
          warning: null,
          build_revision: 'development',
          build_exact: false,
        },
      });
    }
    if (command === 'open_managed_workspace_in_codex') {
      codexCalls += 1;
      throw new Error('the fresh-folder Codex reply was lost after dispatch');
    }
    throw new Error(`unexpected fresh ambiguous agent command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?fresh-ambiguous-agent=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.emitWorkspaceCurrentIntent('start-agent-copy');
  assert.equal(codexCalls, 1);
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), fork.root);
  assert.equal((document.workspaceCurrentAction('start-codex')?.label ?? ''), 'Reopen assigned Codex folder');
  assert.equal((document.workspaceCurrentAction('open-terminal')?.label ?? ''), 'Reopen agent terminal');
  assert.match(document.getElementById('notice').textContent, /could not confirm whether Codex opened/);
  assert.match(document.getElementById('notice').textContent, /marked as handed off/);

  let collisionWarning = null;
  globalThis.confirm = (message) => {
    collisionWarning = message;
    return false;
  };
  await document.emitWorkspaceCurrentIntent('start-codex');
  assert.equal(codexCalls, 1, 'an ambiguous fresh-folder launch was repeated without confirmation');
  assert.equal(collisionWarning, null, 'an unverified custody generation reached reopen confirmation');
  assert.match(document.getElementById('notice').textContent, /Refresh before reopening this assigned folder/);
});

test('same-named projects remain distinguishable in recent workspace navigation', async () => {
  const document = fakeDocument();
  const current = {
    root: '/application/workspaces/acme.mesh/mounts',
    digest: 'workspace-acme',
    installation: 'installation-acme',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-acme', concurrent_changes: 0 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [],
  };
  const other = '/application/workspaces/personal.mesh/mounts';
  const legacyVersion = '/application/workspace-versions/point-abcdef123456.mesh/mounts';
  const meshNamedProject = '/application/workspaces/mesh-named.mesh/mounts';
  const plainNamedProject = '/application/workspaces/plain-named.mesh/mounts';
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: current.root,
        workspaces: [current.root, other, legacyVersion, meshNamedProject, plainNamedProject],
        workspace_entries: [
          {
            path: current.root,
            export_root: '/exports/acme-release',
            project_root: '/clients/acme/app',
          },
          {
            path: other,
            export_root: '/exports/personal-release',
            project_root: '/personal/lab/app',
          },
          {
            path: legacyVersion,
            export_root: '/exports/legacy-release',
            project_root: '/legacy/project\\quarter',
          },
          {
            path: meshNamedProject,
            export_root: '/exports/mesh-named-release',
            project_root: '/legacy/project.mesh',
          },
          {
            path: plainNamedProject,
            export_root: '/exports/plain-named-release',
            project_root: '/legacy/project',
          },
        ],
        auto_opened: false,
        active_folder: '/application/native-workspace/current',
        export_root: '/exports/acme-release',
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: current.root,
        workspace_digest: current.digest,
        workspace_installation: current.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(current);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?same-named-projects=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.deepEqual(
    document.getElementById('recent-workspace').children.map((option) => option.textContent),
    [
      'Current · acme/app · Managed workspace',
      'lab/app · Managed workspace',
      'project\\quarter · Copy of saved version abcdef12',
      'project.mesh · Managed workspace',
      'project · Managed workspace',
    ],
  );
  assert.equal(
    (document.workspaceCurrentAction('return-workspace')?.label ?? ''),
    'Return to lab/app · Managed workspace',
  );
  assert.equal(
    document.getElementById('hero-title').textContent,
    'Working in acme/app · Managed workspace',
    'the active project identity is hidden when recent-workspace navigation is collapsed',
  );
  assert.equal(document.title, 'acme/app · Managed workspace — Mesh');
});

test('a numbered ordinary checkout is not mislabeled as an agent copy', async () => {
  const document = fakeDocument();
  const current = {
    root: '/application/workspace-versions/point-abcdef123456-2.mesh/mounts',
    digest: 'workspace-ordinary-copy-two',
    installation: 'installation-ordinary-copy-two',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-ordinary-copy-two', concurrent_changes: 0 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [],
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: current.root,
        workspaces: [current.root],
        workspace_entries: [{
          path: current.root,
          export_root: '/ordinary/project',
          project_root: '/ordinary/project',
          agent_handoff_installation: null,
          agent_handoff_generation: null,
          source_point_ordinal: 3,
        }],
        auto_opened: false,
        active_folder: '/application/native-workspace/current',
        export_root: '/ordinary/project',
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: current.root,
        workspace_digest: current.digest,
        workspace_installation: current.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(current);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?ordinary-copy-label=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.equal(
    document.getElementById('recent-workspace').children[0].textContent,
    'Current · project · Copy of saved point 3 · Working copy 2',
  );
  assert.equal(
    (document.workspaceCurrentAction('start-codex')?.label ?? ''),
    'Start Codex on this version',
    'the ordinary checkout unexpectedly inherited an agent handoff',
  );
});

test('opening a saved workspace switches to the independent native folder', async () => {
  const document = fakeDocument();
  const version = '11'.repeat(32);
  const current = {
    root: '/managed/current.mesh/mounts',
    digest: 'workspace-current',
    installation: 'installation-current',
    records: 2,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-current', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'current.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [],
    workspace_versions: [{ operation: version, ordinal: 1, actor_sequence: '1' }],
  };
  const fork = {
    ...current,
    root: '/application/workspace-versions/point-111111111111.mesh/mounts',
    digest: 'workspace-earlier',
    installation: 'installation-earlier',
    entries: [{ path: 'earlier.txt', type: 'file' }],
    workspace_versions: [{ operation: '22'.repeat(32), ordinal: 1, actor_sequence: '1' }],
  };
  let forkParameters = null;
  let navigationWarning = null;
  const revealParameters = [];
  const codexParameters = [];
  let open = current;
  let recentPaths = [current.root];
  let rememberCalls = 0;
  let handedOffInstallation = null;
  const stableFolder = '/application/native-workspace/current';
  const projectRoot = '/ordinary/original-project';
  const exportRoot = '/ordinary/release-copy';
  const recentEntries = () => recentPaths.map((path) => ({
    path,
    export_root: exportRoot,
    project_root: projectRoot,
    agent_handoff_installation: path === fork.root ? handedOffInstallation : null,
    agent_handoff_generation: path === fork.root && handedOffInstallation ? TEST_AGENT_HANDOFF_GENERATION : null,
    source_point_ordinal: path === fork.root ? 1 : null,
  }));
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({ remembered: recentPaths[0], workspaces: recentPaths, workspace_entries: recentEntries(), auto_opened: false, active_folder: stableFolder, export_root: exportRoot });
    }
    if (command === 'remember_managed_workspace') {
      rememberCalls += 1;
      assert.equal(parameters.exportRoot, parameters.path === fork.root ? exportRoot : null);
      recentPaths = [parameters.path, ...recentPaths.filter((path) => path !== parameters.path)];
      return JSON.stringify({ remembered: recentPaths[0], workspaces: recentPaths, workspace_entries: recentEntries(), auto_opened: false, active_folder: stableFolder, export_root: exportRoot });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: open.root,
        workspace_digest: open.digest,
        workspace_installation: open.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(open);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'preview_managed_workspace_version') {
      assert.deepEqual(parameters, {
        operation: version,
        expectedWorkspaceRoot: current.root,
        expectedWorkspaceDigest: current.digest,
        expectedWorkspaceInstallation: current.installation,
      });
      return savedWorkspacePreview(version, [{ path: 'original.txt', type: 'file', bytes: '14' }]);
    }
    if (command === 'open_managed_workspace_version') {
      forkParameters = parameters;
      open = fork;
      recentPaths = [fork.root, ...recentPaths.filter((path) => path !== fork.root)];
      return JSON.stringify({
        source_version: version,
        source_ordinal: 1,
        destination: fork.root,
        reused: handedOffInstallation === fork.installation,
        workspace: fork,
        navigation: {
          remembered: fork.root,
          workspaces: recentPaths,
          workspace_entries: recentEntries(),
          auto_opened: false,
          active_folder: stableFolder,
          export_root: exportRoot,
          warning: navigationWarning,
          build_revision: 'development',
          build_exact: false,
        },
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      const requested = JSON.parse(parameters.paramsJson).path;
      if (requested !== current.root) throw new Error(`unexpected workspace: ${requested}`);
      open = current;
      return JSON.stringify(current);
    }
    if (command === 'reveal_managed_workspace') {
      revealParameters.push(parameters);
      return JSON.stringify({ path: stableFolder, workspace_root: open.root, stable: true, native_folder: true });
    }
    if (command === 'open_managed_workspace_in_codex') {
      codexParameters.push(parameters);
      handedOffInstallation = fork.installation;
      return JSON.stringify({
        path: open.root,
        workspace_installation: open.installation,
        fixed_workspace_path: true,
        agent_handoff_recorded: true,
        agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
        agent: 'Codex',
      });
    }
    if (command === 'discover_retired_exports') return '[]';
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?workspace-version-fork=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.equal(
    (document.workspaceCurrentAction('update-destination')?.label ?? ''),
    'Update original folder',
    'a remembered alternate destination displaced the verified original-project action',
  );
  assert.equal(document.workspaceDestination.destination.destination, exportRoot);
  assert.match((document.workspaceCurrent?.current?.privateVersion ?? ''), /^Saved point 1 · version-curr/);
  assert.equal(
    (document.workspaceCurrent?.current?.privateVersionTitle ?? ''),
    `Exact private version: ${current.private_version.version}`,
  );

  let versionsProjection = null;
  document.addEventListener('mesh:workspace-versions-projection', (event) => {
    versionsProjection = event.detail;
    document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-mounted', {
      detail: { generation: event.detail.generation },
    }));
  });
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-available'));
  assert.equal(document.getElementById('workspace-versions-next').classList.contains('hidden'), false);
  assert.equal(versionsProjection.versions.previewState, 'choose');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-intent', {
    detail: {
      generation: versionsProjection.generation,
      intent: { type: 'select-version', operation: version },
    },
  }));
  await waitFor(() => versionsProjection?.versions?.previewState === 'ready');
  assert.equal(versionsProjection.versions.selectedOperation, version);
  assert.deepEqual(versionsProjection.versions.changes, ['Added · original.txt']);
  assert.deepEqual(versionsProjection.versions.entries, ['original.txt · 14 bytes']);
  assert.equal(versionsProjection.versions.canOpen, true);
  assert.equal(versionsProjection.versions.canStartCodex, true);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-intent', {
    detail: {
      generation: versionsProjection.generation - 1,
      intent: { type: 'open-version', operation: version },
    },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-intent', {
    detail: {
      generation: versionsProjection.generation,
      intent: { type: 'open-version', operation: '99'.repeat(32) },
    },
  }));
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(forkParameters, null, 'a stale or forged React version intent reached native open');
  assert.equal(
    document.getElementById('workspace-version').children[1].textContent,
    'Current saved workspace · point 1',
  );
  assert.match(document.getElementById('workspace-version-preview').textContent, /original\.txt · 14 bytes/);
  assert.match(document.getElementById('workspace-version-preview').textContent, /Added · original\.txt/);
  assert.match(document.getElementById('workspace-version-preview').textContent, /Exact retained content verified/);
  assert.equal(document.getElementById('workspace-version-preview').textContent.includes('later.txt'), false);
  assert.equal(document.getElementById('version-destination').value, '');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-intent', {
    detail: {
      generation: versionsProjection.generation,
      intent: { type: 'set-custom-location', path: '/managed/earlier ' },
    },
  }));
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(document.getElementById('workspace-versions-next').classList.contains('hidden'), false);
  assert.equal(document.getElementById('version-destination').value, '/managed/earlier ');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-intent', {
    detail: {
      generation: versionsProjection.generation,
      intent: { type: 'open-version', operation: version },
    },
  }));
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.deepEqual(forkParameters, {
    operation: version,
    destination: '/managed/earlier ',
    exportRoot,
    expectedWorkspaceRoot: current.root,
    expectedWorkspaceDigest: current.digest,
    expectedWorkspaceInstallation: current.installation,
  });
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), fork.root);
  assert.equal((document.workspaceCurrent?.current?.workingFolder ?? ''), stableFolder);
  assert.equal(rememberCalls, 0, 'the browser repeated the native command\'s committed navigation mutation');
  assert.deepEqual(revealParameters[0], {
    expectedWorkspaceRoot: fork.root,
    expectedWorkspaceDigest: fork.digest,
    expectedWorkspaceInstallation: fork.installation,
  });
  assert.equal(document.workspaceCurrent.current.entries[0], 'earlier.txt · file');
  assert.match(document.getElementById('notice').textContent, /Opened Saved point 1 in your working folder/);
  assert.match(document.getElementById('notice').textContent, new RegExp(stableFolder));
  assert.equal(
    document.getElementById('notice').textContent.includes(version.slice(0, 12)),
    false,
    'the switch confirmation exposed an internal operation identity',
  );
  assert.match(document.getElementById('notice').textContent, /stable working path now opens this version/);
  assert.match(document.getElementById('notice').textContent, /Reopen any editor or terminal that was already using the prior folder/);
  assert.match(document.getElementById('notice').textContent, /existing directory handles stay on that prior version/);
  assert.match(document.getElementById('notice').textContent, /Long-running agents remain pinned to their real folder/);
  assert.equal(document.workspaceOverview?.overview.canOpenAnotherVersion, false);
  assert.deepEqual(recentPaths, [fork.root, current.root]);
  assert.equal(
    (document.workspaceCurrentAction('return-workspace')?.label ?? ''),
    'Return to original-project · Managed workspace',
  );
  assert.equal(!document.workspaceCurrentAction('return-workspace')?.enabled, false);
  let overviewProjection = null;
  document.addEventListener('mesh:workspace-overview-projection', (event) => {
    overviewProjection = event.detail;
    document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-mounted', {
      detail: { generation: event.detail.generation },
    }));
  });
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-available'));
  assert.equal(overviewProjection.overview.canReturnWorkspace, true);
  assert.equal(
    overviewProjection.overview.workspaceName,
    'original-project · Copy of saved point 1',
    'the stable native `current` switch handle replaced the saved workspace identity',
  );
  assert.equal(overviewProjection.overview.workingFolder, stableFolder);
  assert.equal(
    document.getElementById('recent-workspace').children[0].textContent,
    'Current · original-project · Copy of saved point 1',
  );
  assert.equal(
    document.getElementById('recent-workspace').children[1].textContent,
    'original-project · Managed workspace',
  );
  assert.equal(document.getElementById('recent-workspace').value, current.root);
  assert.equal(document.getElementById('open-recent-workspace').disabled, false);

  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-intent', {
    detail: {
      generation: overviewProjection.generation,
      intent: { type: 'return-workspace' },
    },
  }));
  await waitFor(() => (document.workspaceCurrent?.current?.agentFolder ?? '') === current.root);
  assert.equal(rememberCalls, 1, 'ordinary recent-workspace navigation still records its new choice');
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), current.root);
  assert.equal((document.workspaceCurrent?.current?.workingFolder ?? ''), stableFolder);
  assert.deepEqual(revealParameters[1], {
    expectedWorkspaceRoot: current.root,
    expectedWorkspaceDigest: current.digest,
    expectedWorkspaceInstallation: current.installation,
  });
  assert.match(document.getElementById('notice').textContent, /stable working folder was opened for Finder and newly opened editors/);
  assert.match(document.getElementById('notice').textContent, /Reopen any editor or terminal that was already using the prior folder/);
  assert.match(document.getElementById('notice').textContent, /Start Codex on this version or Open agent terminal for a pinned agent folder/);
  assert.doesNotMatch(document.getElementById('notice').textContent, /editor or agent there/);
  assert.deepEqual(recentPaths, [current.root, fork.root]);
  assert.equal(
    (document.workspaceCurrentAction('return-workspace')?.label ?? ''),
    'Return to original-project · Copy of saved point 1',
  );
  assert.equal(document.getElementById('recent-workspace').children.length, 2);
  assert.equal(
    document.getElementById('recent-workspace').children[0].textContent,
    'Current · original-project · Managed workspace',
  );
  assert.equal(
    document.getElementById('recent-workspace').children[1].textContent,
    'original-project · Copy of saved point 1',
  );
  assert.equal(
    overviewProjection.overview.workspaceName,
    'original-project · Managed workspace',
    'returning through the same stable native path did not update the project identity',
  );
  assert.equal(overviewProjection.overview.workingFolder, stableFolder);

  assert.equal(
    document.getElementById('workspace-versions-next').classList.contains('hidden'),
    false,
    'the visible React picker was not active for the keyboard-focus regression',
  );
  current.workspace_versions = [
    { operation: version, ordinal: 1, actor_sequence: '1' },
    { operation: '33'.repeat(32), ordinal: 2, actor_sequence: '2' },
  ];
  await document.emitWorkspaceCurrentIntent('refresh');
  const earlierVersionChoice = new FakeElement();
  const currentVersionChoice = new FakeElement();
  let focusedVersionChoice = null;
  for (const [choice, operation] of [
    [earlierVersionChoice, version],
    [currentVersionChoice, '33'.repeat(32)],
  ]) {
    choice.focus = () => {
      earlierVersionChoice.focused = choice === earlierVersionChoice;
      currentVersionChoice.focused = choice === currentVersionChoice;
      focusedVersionChoice = choice;
    };
    choice.addEventListener('click', () => {
      document.getElementById('workspace-version').value = operation;
    });
  }
  document.getElementById('workspace-versions-next').shadowRoot = {
    querySelector(selector) {
      const exact = selector.match(/^\[data-mesh-version-operation="([0-9a-f]{64})"\]:not\(:disabled\)$/u);
      if (exact?.[1] === version) return earlierVersionChoice;
      if (exact?.[1] === '33'.repeat(32)) return currentVersionChoice;
      if (selector === '[role="radio"][tabindex="0"]:not(:disabled)') return currentVersionChoice;
      return null;
    },
  };
  assert.equal(overviewProjection.overview.canOpenAnotherVersion, true);
  assert.deepEqual(
    document.getElementById('workspace-version').children.map((option) => option.textContent),
    [
      'Choose a saved workspace',
      'Current saved workspace · point 2',
      'Earlier saved workspace · point 1',
    ],
    'the picker did not put the current point before older history',
  );
  document.getElementById('workspace-version').value = '33'.repeat(32);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-intent', {
    detail: {
      generation: overviewProjection.generation - 1,
      intent: { type: 'open-another-version' },
    },
  }));
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(
    document.getElementById('workspace-version').value,
    '33'.repeat(32),
    'a stale React overview generation reached the legacy coordinator action',
  );
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-intent', {
    detail: {
      generation: overviewProjection.generation,
      intent: { type: 'open-another-version' },
    },
  }));
  await waitFor(() => document.getElementById('workspace-version').value === version);
  assert.equal(
    document.getElementById('workspace-version').value,
    version,
    'the top-level Open another version shortcut preserved the current point instead of choosing the nearest earlier point',
  );
  assert.equal(currentVersionChoice.focused, false, 'the delayed React commit left focus on the current point');
  assert.equal(earlierVersionChoice.focused, true, 'the nearest earlier point did not receive focus');
  await focusedVersionChoice.click();
  assert.equal(
    document.getElementById('workspace-version').value,
    version,
    'Space on the focused React row reselected the prior current point',
  );
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-rejected', {
    detail: { generation: overviewProjection.generation },
  }));
  assert.equal(
    document.getElementById('workspace-overview-next').classList.contains('hidden'),
    true,
    'a rejected React overview falsely marked its empty assigned host ready and suppressed the outer failure fallback',
  );
  document.getElementById('workspace-version').value = '';
  earlierVersionChoice.focused = false;
  currentVersionChoice.focused = false;
  await document.emitWorkspaceCurrentIntent('open-version');
  assert.equal(
    document.getElementById('workspace-version').value,
    version,
    'the primary switch action did not choose the nearest earlier saved workspace',
  );
  assert.equal(document.getElementById('workspace-versions-next').scrolledIntoView, true);
  assert.equal(currentVersionChoice.focused, false, 'the primary switch focused the current point');
  assert.equal(earlierVersionChoice.focused, true, 'the primary switch did not focus the nearest earlier point');
  assert.match(document.getElementById('workspace-version-preview').textContent, /original\.txt · 14 bytes/);
  assert.equal(document.getElementById('fork-version').disabled, false);
  assert.equal(document.getElementById('fork-version-codex').disabled, false);

  navigationWarning = 'Mesh could not update optional recent-workspace history.';
  await document.getElementById('fork-version-codex').emit('click');
  assert.deepEqual(codexParameters, [{
    expectedWorkspaceRoot: fork.root,
    expectedWorkspaceDigest: fork.digest,
    expectedWorkspaceInstallation: fork.installation,
    confirmedReopen: false,
    expectedAgentHandoffGeneration: null,
  }]);
  assert.equal(
    revealParameters.length,
    2,
    'the direct Codex handoff opened Finder instead of the independent agent folder',
  );
  assert.match(document.getElementById('notice').textContent, /Opened .* in Codex as an independent writable folder/);
  assert.match(document.getElementById('notice').textContent, /stays on this version/);
  assert.match(document.getElementById('notice').textContent, /could not update optional recent-workspace history/);
  assert.equal(document.getElementById('notice').classList.contains('error'), true);

  await document.emitWorkspaceCurrentIntent('return-workspace');
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), current.root);

  document.getElementById('workspace-version').value = version;
  await document.getElementById('workspace-version').emit('change');
  await document.getElementById('fork-version-codex').emit('click');
  assert.equal(codexParameters.length, 1, 'a reused agent folder was handed to a second Codex agent');
  assert.match(document.getElementById('notice').textContent, /already handed to an agent/);
  assert.match(document.getElementById('notice').textContent, /Start another agent/);
  await document.emitWorkspaceCurrentIntent('return-workspace');
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), current.root);

  current.private_version.concurrent_changes = 2;
  current.workspace_versions = [
    { operation: version, ordinal: 1, actor_sequence: '1' },
    { operation: '33'.repeat(32), ordinal: 2, actor_sequence: '2' },
  ];
  await document.emitWorkspaceCurrentIntent('refresh');
  assert.equal(
    document.getElementById('workspace-version').children
      .some((option) => option.textContent.startsWith('Current saved workspace')),
    false,
    'a concurrent history was presented as one complete current point',
  );
  await document.emitWorkspaceCurrentIntent('update-destination');
  assert.equal(
    document.destinationField('destination').value,
    exportRoot,
    'an unapproved saved point reached the original-folder export controls',
  );
});

test('a saved-version refresh failure never leaves the stable folder on the prior workspace', async () => {
  const document = fakeDocument();
  const version = '13'.repeat(32);
  const current = {
    root: '/managed/navigation-a.mesh/mounts',
    digest: 'workspace-navigation-a',
    installation: 'installation-navigation-a',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-navigation-a', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [{ operation: version, ordinal: 1, actor_sequence: '1' }],
  };
  const fork = {
    ...current,
    root: '/managed/navigation-b.mesh/mounts',
    digest: 'workspace-navigation-b',
    installation: 'installation-navigation-b',
  };
  const stableFolder = '/application/native-workspace/current';
  let open = current;
  let postSwitchCheckpointFailed = false;
  let stableTarget = current.root;
  let failSafeReconciliations = 0;
  let exactReconciliations = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: current.root,
        workspaces: [current.root],
        auto_opened: false,
        active_folder: stableFolder,
        export_root: null,
        warning: null,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(open);
    }
    if (command === 'managed_checkpoint_state') {
      if (open === fork && !postSwitchCheckpointFailed) {
        postSwitchCheckpointFailed = true;
        throw new Error('checkpoint refresh unavailable after daemon switch');
      }
      return JSON.stringify({
        root: open.root,
        workspace_digest: open.digest,
        workspace_installation: open.installation,
        working: false,
      });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'preview_managed_workspace_version') {
      return savedWorkspacePreview(version);
    }
    if (command === 'open_managed_workspace_version') {
      open = fork;
      return JSON.stringify({ source_version: version, source_ordinal: 1, destination: fork.root, workspace: fork });
    }
    if (command === 'reconcile_current_workspace_navigation') {
      failSafeReconciliations += 1;
      stableTarget = null;
      return JSON.stringify({
        path: null,
        stable: false,
        warning: 'The stable folder was deactivated until current workspace verification succeeds.',
      });
    }
    if (command === 'reconcile_managed_workspace_navigation') {
      exactReconciliations += 1;
      assert.deepEqual(parameters, {
        expectedWorkspaceRoot: fork.root,
        expectedWorkspaceDigest: fork.digest,
        expectedWorkspaceInstallation: fork.installation,
      });
      stableTarget = fork.root;
      return JSON.stringify({
        path: stableFolder,
        workspace_root: fork.root,
        stable: true,
        native_folder: true,
      });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?version-post-switch-failure=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  document.getElementById('workspace-version').value = version;
  await document.getElementById('workspace-version').emit('change');
  await document.getElementById('fork-version').emit('click');

  assert.equal(open, fork, 'the daemon did not complete the workspace switch');
  assert.equal(failSafeReconciliations, 1);
  assert.equal(stableTarget, null, 'the stale stable folder remained on workspace A');
  assert.equal(document.serviceState.state === 'ready', false);
  assert.equal(document.getElementById('fork-version').disabled, true);
  assert.match(document.getElementById('notice').textContent, /stable folder was deactivated/);

  await document.emitWorkspaceCurrentIntent('refresh');

  assert.equal(exactReconciliations, 1, 'refresh did not repair the missing stable folder');
  assert.equal(stableTarget, fork.root);
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), fork.root);
  assert.equal((document.workspaceCurrent?.current?.workingFolder ?? ''), stableFolder);
  assert.equal(document.serviceState.state === 'ready', true);
});

test('a lost saved-version reply recovers the workspace the daemon actually opened', async () => {
  const document = fakeDocument();
  const version = '14'.repeat(32);
  const current = {
    root: '/managed/lost-version-reply-a.mesh/mounts',
    digest: 'workspace-lost-version-reply-a',
    installation: 'installation-lost-version-reply-a',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-lost-reply-a', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [{ operation: version, ordinal: 1, actor_sequence: '1' }],
  };
  const opened = {
    ...current,
    root: '/managed/lost-version-reply-b.mesh/mounts',
    digest: 'workspace-lost-version-reply-b',
    installation: 'installation-lost-version-reply-b',
    private_version: { version: 'version-lost-reply-b', concurrent_changes: 1 },
  };
  const stableFolder = '/application/native-workspace/current';
  let live = current;
  let rememberParameters = null;
  const navigation = () => ({
    remembered: live.root,
    workspaces: [live.root, current.root],
    workspace_entries: [{ path: live.root, export_root: '/ordinary/original' }],
    auto_opened: false,
    active_folder: stableFolder,
    export_root: '/ordinary/original',
    warning: null,
    build_revision: 'development',
    build_exact: false,
  });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify(navigation());
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: live.root,
        workspace_digest: live.digest,
        workspace_installation: live.installation,
        native_folder: true,
        native_folder_path: live.root,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(live);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'preview_managed_workspace_version') {
      return savedWorkspacePreview(version);
    }
    if (command === 'open_managed_workspace_version') {
      live = opened;
      throw new Error('saved-version reply lost after the native commit');
    }
    if (command === 'remember_managed_workspace') {
      rememberParameters = parameters;
      return JSON.stringify(navigation());
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?lost-version-reply=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  document.getElementById('workspace-version').value = version;
  await document.getElementById('workspace-version').emit('change');
  await document.getElementById('fork-version').emit('click');

  assert.deepEqual(rememberParameters, {
    path: opened.root,
    exportRoot: '/ordinary/original',
  });
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), opened.root);
  assert.equal((document.workspaceCurrent?.current?.workingFolder ?? ''), stableFolder);
  assert.equal(document.serviceState.state === 'ready', true);
  assert.match(document.getElementById('notice').textContent, /recovered and verified the workspace that is actually open/i);
  assert.match(document.getElementById('notice').textContent, /saved-version reply lost after the native commit/);
});

test('saved-version switching never opens a superseded visible choice', async () => {
  const document = fakeDocument();
  const firstOperation = '71'.repeat(32);
  const secondOperation = '72'.repeat(32);
  const workspace = {
    root: '/managed/version-choice/mounts',
    digest: 'workspace-version-choice',
    installation: 'installation-version-choice',
    records: 2,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-choice', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [
      { operation: firstOperation, ordinal: 1, actor_sequence: '1' },
      { operation: secondOperation, ordinal: 2, actor_sequence: '2' },
    ],
  };
  let stateReads = 0;
  let releasePreflight;
  let preflightReached;
  const blockedPreflight = new Promise((resolve) => { releasePreflight = resolve; });
  const reachedPreflight = new Promise((resolve) => { preflightReached = resolve; });
  const openedOperations = [];
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      stateReads += 1;
      if (stateReads === 2) {
        preflightReached();
        await blockedPreflight;
      }
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories') return JSON.stringify([]);
    if (command === 'discover_native_missing_files') return JSON.stringify([]);
    if (command === 'preview_managed_workspace_version') {
      return savedWorkspacePreview(parameters.operation);
    }
    if (command === 'open_managed_workspace_version') {
      openedOperations.push(parameters.operation);
      throw new Error('a superseded visible choice must not create a version workspace');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?version-choice-race=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const select = document.getElementById('workspace-version');
  select.value = firstOperation;
  await select.emit('change');
  const opening = document.getElementById('fork-version').emit('click');
  await reachedPreflight;
  select.value = secondOperation;
  await select.emit('change');
  releasePreflight();
  await opening;

  assert.deepEqual(openedOperations, [], 'the operation no longer shown by the selector was opened');
  assert.match(document.getElementById('notice').textContent, /saved-version action is no longer available/i);
});

test('a slow saved-version preview cannot replace the newly selected point', async () => {
  const document = fakeDocument();
  const firstOperation = '73'.repeat(32);
  const secondOperation = '74'.repeat(32);
  const workspace = {
    root: '/managed/version-preview-race/mounts',
    digest: 'workspace-version-preview-race',
    installation: 'installation-version-preview-race',
    records: 2,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-preview-race', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [
      { operation: firstOperation, ordinal: 1, actor_sequence: '1' },
      { operation: secondOperation, ordinal: 2, actor_sequence: '2' },
    ],
  };
  let releaseFirst;
  let firstReached;
  const blockedFirst = new Promise((resolve) => { releaseFirst = resolve; });
  const reachedFirst = new Promise((resolve) => { firstReached = resolve; });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'preview_managed_workspace_version') {
      if (parameters.operation === firstOperation) {
        firstReached();
        await blockedFirst;
        return savedWorkspacePreview(firstOperation, [{ path: 'old.txt', type: 'file', bytes: '3' }]);
      }
      return savedWorkspacePreview(
        secondOperation,
        [{ path: 'new.txt', type: 'file', bytes: '4' }],
        {
          ordinal: 2,
          actorSequence: '2',
          basisOrdinal: 1,
          changeBasis: 'previous-point',
          changes: [{ path: 'new.txt', type: 'file', effect: 'added' }],
        },
      );
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?version-preview-race=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const select = document.getElementById('workspace-version');
  select.value = firstOperation;
  const firstPreview = select.emit('change');
  await reachedFirst;
  select.value = secondOperation;
  await select.emit('change');
  assert.match(document.getElementById('workspace-version-preview').textContent, /new\.txt/);
  assert.match(document.getElementById('workspace-version-preview').textContent, /What changed/);
  assert.match(document.getElementById('workspace-version-preview').textContent, /Added · new\.txt/);
  assert.equal(document.getElementById('fork-version').disabled, false);
  releaseFirst();
  await firstPreview;

  assert.match(document.getElementById('workspace-version-preview').textContent, /new\.txt/);
  assert.equal(document.getElementById('workspace-version-preview').textContent.includes('old.txt'), false);
  assert.equal(document.getElementById('fork-version').disabled, false);
});

test('an invalid saved-version preview stays closed and the primary action retries it', async () => {
  const document = fakeDocument();
  const operation = '75'.repeat(32);
  const workspace = {
    root: '/managed/version-preview-invalid/mounts',
    digest: 'workspace-version-preview-invalid',
    installation: 'installation-version-preview-invalid',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-preview-invalid', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  let previewAttempts = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'preview_managed_workspace_version') {
      previewAttempts += 1;
      return previewAttempts === 1
        ? savedWorkspacePreview('76'.repeat(32), [{ path: 'wrong.txt', type: 'file', bytes: '3' }])
        : savedWorkspacePreview(operation, [{ path: 'right.txt', type: 'file', bytes: '3' }]);
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?version-preview-invalid=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  document.getElementById('workspace-version').value = operation;
  await document.getElementById('workspace-version').emit('change');

  assert.equal(document.getElementById('fork-version').disabled, true);
  assert.equal(document.getElementById('workspace-version-preview').classList.contains('hidden'), true);
  assert.match(document.getElementById('notice').textContent, /did not match the selected durable point/);
  assert.match(document.getElementById('version-hint').textContent, /Select it again to retry/);

  await document.emitWorkspaceCurrentIntent('open-version');

  assert.equal(previewAttempts, 2);
  assert.equal(document.getElementById('workspace-version').value, operation);
  assert.equal(document.getElementById('fork-version').disabled, false);
  assert.match(document.getElementById('workspace-version-preview').textContent, /right\.txt/);
  assert.match(document.getElementById('notice').textContent, /Review the verified saved point/);
});

test('dishonest saved-version preview summaries never enable the folder switch', async () => {
  const document = fakeDocument();
  const operation = '77'.repeat(32);
  const workspace = {
    root: '/managed/version-preview-dishonest-totals/mounts',
    digest: 'workspace-version-preview-dishonest-totals',
    installation: 'installation-version-preview-dishonest-totals',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-preview-dishonest-totals', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  const dishonestPreviews = [
    () => JSON.parse(savedWorkspacePreview(operation, [
      { path: 'only-proven.txt', type: 'file', bytes: '3' },
    ], { ordinal: 2 })),
    () => {
      const preview = JSON.parse(savedWorkspacePreview(
        operation,
        [{ path: 'only-proven.txt', type: 'file', bytes: '3' }],
      ));
      preview.actor_sequence = '2';
      return preview;
    },
    () => {
      const preview = JSON.parse(savedWorkspacePreview(
        operation,
        [{ path: 'only-proven.txt', type: 'file', bytes: '3' }],
      ));
      preview.files = 2;
      return preview;
    },
    () => JSON.parse(savedWorkspacePreview(operation, [
      { path: 'same.txt', type: 'file', bytes: '3' },
      { path: 'same.txt', type: 'file', bytes: '3' },
    ])),
    () => JSON.parse(savedWorkspacePreview(operation, [
      { path: 'z-last.txt', type: 'file', bytes: '3' },
      { path: 'a-first.txt', type: 'file', bytes: '3' },
    ])),
    () => {
      const preview = JSON.parse(savedWorkspacePreview(operation, [
        { path: 'one.txt', type: 'file', bytes: '3' },
        { path: 'two.txt', type: 'file', bytes: '3' },
      ]));
      preview.files = 0;
      preview.folders = 2;
      return preview;
    },
    () => {
      const preview = JSON.parse(savedWorkspacePreview(
        operation,
        [{ path: 'one.txt', type: 'file', bytes: '3' }],
      ));
      preview.changes.push({ ...preview.changes[0] });
      return preview;
    },
    () => {
      const preview = JSON.parse(savedWorkspacePreview(operation, [
        { path: 'a-first.txt', type: 'file', bytes: '3' },
        { path: 'z-last.txt', type: 'file', bytes: '3' },
      ]));
      preview.changes.reverse();
      return preview;
    },
  ];
  let previewAttempt = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'preview_managed_workspace_version') {
      return JSON.stringify(dishonestPreviews[previewAttempt++]());
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?version-preview-dishonest-totals=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  for (const _ of dishonestPreviews) {
    document.getElementById('workspace-version').value = operation;
    await document.getElementById('workspace-version').emit('change');
    assert.equal(document.getElementById('fork-version').disabled, true);
    assert.equal(document.getElementById('workspace-version-preview').classList.contains('hidden'), true);
    assert.match(document.getElementById('notice').textContent, /saved workspace preview/);
  }
  assert.equal(previewAttempt, dishonestPreviews.length);
});

test('a saved-version open response cannot substitute another durable point', async () => {
  const document = fakeDocument();
  const selectedOperation = '78'.repeat(32);
  const substitutedOperation = '79'.repeat(32);
  const current = {
    root: '/managed/version-open-identity-current/mounts',
    digest: 'workspace-version-open-identity-current',
    installation: 'installation-version-open-identity-current',
    records: 2,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-open-identity-current', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [
      { operation: selectedOperation, ordinal: 1, actor_sequence: '1' },
      { operation: substitutedOperation, ordinal: 2, actor_sequence: '2' },
    ],
  };
  const opened = {
    ...current,
    root: '/managed/version-open-identity-substituted/mounts',
    digest: 'workspace-version-open-identity-substituted',
    installation: 'installation-version-open-identity-substituted',
    private_version: { version: 'version-open-identity-substituted', concurrent_changes: 1 },
  };
  let live = current;
  let reveals = 0;
  const stableFolder = '/application/native-workspace/current';
  const navigation = () => ({
    remembered: live.root,
    workspaces: [live.root, current.root],
    workspace_entries: [{ path: live.root, source_point_ordinal: 2 }],
    auto_opened: false,
    active_folder: stableFolder,
    export_root: null,
    warning: null,
  });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify(navigation());
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: live.root,
        workspace_digest: live.digest,
        workspace_installation: live.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(live);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'preview_managed_workspace_version') {
      return savedWorkspacePreview(
        selectedOperation,
        [{ path: 'point-one.txt', type: 'file', bytes: '3' }],
      );
    }
    if (command === 'open_managed_workspace_version') {
      live = opened;
      return JSON.stringify({
        source_version: substitutedOperation,
        source_ordinal: 2,
        destination: opened.root,
        reused: false,
        workspace: opened,
        navigation: {
          remembered: opened.root,
          workspaces: [opened.root, current.root],
          workspace_entries: [
            { path: opened.root, source_point_ordinal: 2 },
            { path: current.root, source_point_ordinal: null },
          ],
          auto_opened: false,
          active_folder: stableFolder,
          export_root: null,
          warning: null,
        },
      });
    }
    if (command === 'remember_managed_workspace') {
      return JSON.stringify(navigation());
    }
    if (command === 'reveal_managed_workspace') {
      reveals += 1;
      return JSON.stringify({ path: stableFolder, workspace_root: opened.root, stable: true });
    }
    throw new Error(`unexpected saved-version identity command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?version-open-identity=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  document.getElementById('workspace-version').value = selectedOperation;
  await document.getElementById('workspace-version').emit('change');
  await document.getElementById('fork-version').emit('click');

  assert.equal(
    (document.workspaceCurrent?.current?.agentFolder ?? ''),
    opened.root,
    'the renderer left the old workspace visible after native had already switched',
  );
  assert.equal(document.serviceState.state === 'ready', true);
  assert.equal(reveals, 0);
  assert.match(document.getElementById('notice').textContent, /did not match the selected durable point/);
  assert.match(document.getElementById('notice').textContent, /recovered and verified the workspace that is actually open/i);
});

test('recent-workspace switching never opens a superseded visible choice', async () => {
  const document = fakeDocument();
  const current = {
    root: '/managed/recent-current/mounts',
    digest: 'workspace-recent-current',
    installation: 'installation-recent-current',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-recent-current', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [],
  };
  const second = '/managed/recent-second/mounts';
  const third = '/managed/recent-third/mounts';
  let stateReads = 0;
  let releasePreflight;
  let preflightReached;
  const blockedPreflight = new Promise((resolve) => { releasePreflight = resolve; });
  const reachedPreflight = new Promise((resolve) => { preflightReached = resolve; });
  const openedPaths = [];
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: current.root,
        workspaces: [current.root, second, third],
        auto_opened: false,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: current.root,
        workspace_digest: current.digest,
        workspace_installation: current.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      stateReads += 1;
      if (stateReads === 2) {
        preflightReached();
        await blockedPreflight;
      }
      return JSON.stringify(current);
    }
    if (command === 'discover_native_directories') return JSON.stringify([]);
    if (command === 'discover_native_missing_files') return JSON.stringify([]);
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      openedPaths.push(JSON.parse(parameters.paramsJson).path);
      throw new Error('a superseded visible recent choice must not open');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?recent-choice-race=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const mountedGeneration = document.workspaceEntry.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-mounted', {
    detail: { generation: mountedGeneration },
  }));
  await document.emitWorkspaceEntryIntent({ type: 'select-recent', path: second }, mountedGeneration);
  assert.ok(document.workspaceEntry.generation > mountedGeneration);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
    detail: { generation: mountedGeneration, intent: { type: 'open-recent', path: second } },
  }));
  await reachedPreflight;
  const replacementGeneration = document.workspaceEntry.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-mounted', {
    detail: { generation: replacementGeneration },
  }));
  await document.emitWorkspaceEntryIntent({ type: 'select-recent', path: third }, replacementGeneration);
  releasePreflight();
  await waitFor(() => /recent workspace choice changed/i.test(document.getElementById('notice').textContent));

  assert.deepEqual(openedPaths, [], 'the recent workspace no longer shown by the selector was opened');
  assert.match(document.getElementById('notice').textContent, /recent workspace choice changed/i);
});

test('a delayed Recent selection forgets once, recovers a lost reply, and selects a surviving workspace', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/recent-current/mounts',
    digest: 'workspace-recent-current',
    installation: 'installation-recent-current',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-recent-current', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_inventory_complete: true,
    file_histories: [],
    workspace_versions: [],
  };
  const first = '/managed/recent-first/mounts';
  const forgotten = '/managed/recent-forgotten/mounts';
  let recentPaths = [workspace.root, first, forgotten];
  let forgetCalls = 0;
  let statusReads = 0;
  const navigation = () => ({
    remembered: workspace.root,
    workspaces: recentPaths,
    workspace_entries: recentPaths.map((path) => ({
      path,
      export_root: null,
      project_root: null,
      agent_handoff_installation: null,
      agent_handoff_generation: null,
    })),
    auto_opened: false,
    active_folder: null,
    export_root: null,
  });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      statusReads += 1;
      return JSON.stringify(navigation());
    }
    if (command === 'forget_managed_workspace') {
      forgetCalls += 1;
      assert.equal(parameters.path, forgotten);
      recentPaths = recentPaths.filter((path) => path !== forgotten);
      throw new Error('forget reply was lost after the navigation entry was removed');
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    throw new Error(`unexpected Recent recovery command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?recent-forget-recovery=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready' && document.workspaceEntry);

  const mountedGeneration = document.workspaceEntry.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-mounted', {
    detail: { generation: mountedGeneration },
  }));
  await document.emitWorkspaceEntryIntent({ type: 'select-recent', path: forgotten }, mountedGeneration);
  assert.ok(document.workspaceEntry.generation > mountedGeneration);
  assert.equal(document.workspaceEntry.entry.selectedRecentPath, forgotten);

  for (const [generation, path] of [
    [mountedGeneration - 1, forgotten],
    [mountedGeneration, '/managed/forged/mounts'],
  ]) {
    document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
      detail: { generation, intent: { type: 'forget-recent', path } },
    }));
  }
  assert.equal(forgetCalls, 0);

  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
    detail: { generation: mountedGeneration, intent: { type: 'forget-recent', path: forgotten } },
  }));
  await waitFor(() => forgetCalls === 1 && !document.workspaceEntry.entry.recents.some((entry) => entry.path === forgotten));

  assert.equal(forgetCalls, 1, 'a lost non-idempotent forget reply replayed the native removal');
  assert.ok(statusReads >= 2, 'the lost reply was not classified from authoritative Recent state');
  assert.equal(document.workspaceEntry.entry.selectedRecentPath, first);
  assert.equal(document.workspaceEntry.entry.canOpenRecent, true);
  assert.equal(document.workspaceEntry.entry.canForgetRecent, true);
  assert.match(document.getElementById('notice').textContent, /lost the first recent-workspace reply but confirmed the entry was removed/i);
});

test('saved-version switching refuses to discard an editor-only draft', async () => {
  const document = fakeDocument();
  const operation = '81'.repeat(32);
  const workspace = {
    root: '/managed/draft/mounts',
    digest: 'workspace-draft',
    installation: 'installation-draft',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-draft', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'draft.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [{
      path: 'draft.txt',
      object_id: 'object-draft',
      current: { version_id: 'version-draft', manifest_id: 'manifest-draft' },
      retained_versions: [{ version_id: 'version-draft', manifest_id: 'manifest-draft' }],
    }],
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  let versionOpenCalls = 0;
  let confirmCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: workspace.root,
        workspaces: [workspace.root],
        auto_opened: false,
        active_folder: '/application/native-workspace/current',
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'inspect_managed_file') {
      return JSON.stringify({
        path: 'draft.txt',
        text: 'saved\n',
        text_editable: true,
        modified_from_current_version: false,
        current_version: 'version-draft',
        byte_count: 6,
        content_digest: 'digest-saved',
        executable: false,
      });
    }
    if (command === 'preview_managed_workspace_version') {
      return savedWorkspacePreview(parameters.operation, [{ path: 'draft.txt', type: 'file', bytes: '6' }]);
    }
    if (command === 'open_managed_workspace_version') {
      versionOpenCalls += 1;
      throw new Error('the draft guard should run before version creation');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => {
    confirmCalls += 1;
    return true;
  };
  await import(`./app.js?version-draft-guard=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const file = document.getElementById('edit-file');
  file.value = 'draft.txt';
  await file.emit('change');
  await document.getElementById('load-file').emit('click');
  const editor = document.getElementById('file-editor');
  editor.value = 'not preserved anywhere else\n';
  await editor.emit('input');
  assert.equal(document.getElementById('save-file').disabled, false);

  document.getElementById('workspace-version').value = operation;
  await document.getElementById('workspace-version').emit('change');
  await document.getElementById('fork-version').emit('click');

  assert.equal(versionOpenCalls, 0);
  assert.equal(confirmCalls, 0, 'editor-only text was offered a destructive switch-anyway path');
  assert.equal(editor.value, 'not preserved anywhere else\n');
  assert.match(document.getElementById('notice').textContent, /Preserve the open editor draft/);
});

test('saved-version switching refuses a draft typed while native inspection is pending', async () => {
  const document = fakeDocument();
  const operation = '8a'.repeat(32);
  const workspace = {
    root: '/managed/draft-race/mounts',
    digest: 'workspace-draft-race',
    installation: 'installation-draft-race',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-draft-race', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'draft.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [{
      path: 'draft.txt',
      object_id: 'object-draft-race',
      current: { version_id: 'version-draft-race', manifest_id: 'manifest-draft-race' },
      retained_versions: [{ version_id: 'version-draft-race', manifest_id: 'manifest-draft-race' }],
    }],
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  const replacement = {
    ...workspace,
    root: '/managed/draft-race-copy/mounts',
    digest: 'workspace-draft-race-copy',
    installation: 'installation-draft-race-copy',
  };
  let open = workspace;
  let stateReads = 0;
  let versionOpenCalls = 0;
  let releasePreflight;
  let preflightReached;
  const blockedPreflight = new Promise((resolve) => { releasePreflight = resolve; });
  const reachedPreflight = new Promise((resolve) => { preflightReached = resolve; });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: workspace.root,
        workspaces: [workspace.root],
        auto_opened: false,
        active_folder: '/application/native-workspace/current',
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: open.root, workspace_digest: open.digest, workspace_installation: open.installation, working: false });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      stateReads += 1;
      if (stateReads === 2) {
        preflightReached();
        await blockedPreflight;
      }
      return JSON.stringify(open);
    }
    if (command === 'discover_native_directories') return JSON.stringify([]);
    if (command === 'discover_native_missing_files') return JSON.stringify([]);
    if (command === 'inspect_managed_file') {
      return JSON.stringify({
        path: 'draft.txt',
        text: 'saved\n',
        text_editable: true,
        modified_from_current_version: false,
        current_version: 'version-draft-race',
        byte_count: 6,
        content_digest: 'digest-saved',
        executable: false,
      });
    }
    if (command === 'preview_managed_workspace_version') {
      return savedWorkspacePreview(parameters.operation, [{ path: 'draft.txt', type: 'file', bytes: '6' }]);
    }
    if (command === 'open_managed_workspace_version') {
      versionOpenCalls += 1;
      open = replacement;
      return JSON.stringify({ source_version: operation, source_ordinal: 1, destination: replacement.root, workspace: replacement });
    }
    if (command === 'remember_managed_workspace') {
      return JSON.stringify({
        remembered: replacement.root,
        workspaces: [replacement.root, workspace.root],
        auto_opened: false,
        active_folder: '/application/native-workspace/current',
      });
    }
    if (command === 'reveal_managed_workspace') {
      return JSON.stringify({ path: '/application/native-workspace/current', workspace_root: replacement.root, stable: true });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?version-draft-during-preflight=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const file = document.getElementById('edit-file');
  file.value = 'draft.txt';
  await file.emit('change');
  await document.getElementById('load-file').emit('click');
  const editor = document.getElementById('file-editor');
  document.getElementById('workspace-version').value = operation;
  await document.getElementById('workspace-version').emit('change');
  const switching = document.getElementById('fork-version').emit('click');
  await reachedPreflight;
  editor.value = 'typed while Mesh was checking the native folder\n';
  await editor.emit('input');
  assert.equal(
    document.workspaceWork?.workbench.changes.editorText,
    'typed while Mesh was checking the native folder\n',
    'the mounted Changes editor dropped a draft typed while native preflight was pending',
  );
  releasePreflight();
  await switching;
  await waitFor(() => /Preserve the open editor draft/.test(document.getElementById('notice').textContent));

  assert.equal(versionOpenCalls, 0, 'a draft typed during the asynchronous preflight was discarded');
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), workspace.root);
  assert.equal(editor.value, 'typed while Mesh was checking the native folder\n');
  assert.match(document.getElementById('notice').textContent, /Preserve the open editor draft/);
});

test('saved-version switching surfaces native edits before leaving their folder', async () => {
  const document = fakeDocument();
  const operation = '82'.repeat(32);
  const workspace = {
    root: '/managed/native-change/mounts',
    digest: 'workspace-native-change',
    installation: 'installation-native-change',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-native-change', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'changed.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [{
      path: 'changed.txt',
      object_id: 'object-native-change',
      current: { version_id: 'version-native-change', manifest_id: 'manifest-native-change' },
      retained_versions: [{ version_id: 'version-native-change', manifest_id: 'manifest-native-change' }],
    }],
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  let versionOpenCalls = 0;
  let confirmation = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories') return JSON.stringify([]);
    if (command === 'discover_native_missing_files') return JSON.stringify([]);
    if (command === 'inspect_managed_file') {
      assert.deepEqual(parameters, {
        relativePath: 'changed.txt',
        expectedWorkspaceRoot: workspace.root,
        expectedWorkspaceDigest: workspace.digest,
        expectedWorkspaceInstallation: workspace.installation,
      });
      return JSON.stringify({
        path: 'changed.txt',
        text: 'changed by agent\n',
        text_editable: true,
        modified_from_current_version: true,
        current_version: 'version-native-change',
        byte_count: 17,
        content_digest: 'digest-native-change',
        executable: false,
      });
    }
    if (command === 'preview_managed_workspace_version') {
      return savedWorkspacePreview(parameters.operation, [{ path: 'changed.txt', type: 'file', bytes: '17' }]);
    }
    if (command === 'open_managed_workspace_version') {
      versionOpenCalls += 1;
      throw new Error('a cancelled switch must not create a version workspace');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = (message) => {
    confirmation = message;
    return false;
  };
  await import(`./app.js?version-native-change-guard=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  document.getElementById('workspace-version').value = operation;
  await document.getElementById('workspace-version').emit('change');
  await document.getElementById('fork-version').emit('click');

  assert.equal(versionOpenCalls, 0);
  assert.match(confirmation, /1 unsaved native change/);
  assert.match(confirmation, /not part of the saved version/);
  assert.equal(document.getElementById('folder-change-items').children.length, 1);
  assert.match(document.getElementById('folder-change-items').children[0].textContent, /changed tracked file/);
});

test('saved-version switching refreshes newly created native files before leaving', async () => {
  const document = fakeDocument();
  const operation = '83'.repeat(32);
  const workspace = {
    root: '/managed/new-native/mounts',
    digest: 'workspace-new-native',
    installation: 'installation-new-native',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-new-native', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'existing.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [],
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
  };
  let stateReads = 0;
  let versionOpenCalls = 0;
  let confirmation = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      stateReads += 1;
      return JSON.stringify(stateReads === 1
        ? workspace
        : { ...workspace, native_untracked_files: ['created-after-refresh.txt'] });
    }
    if (command === 'discover_native_directories') return JSON.stringify([]);
    if (command === 'discover_native_missing_files') return JSON.stringify([]);
    if (command === 'inspect_native_file') {
      assert.deepEqual(parameters, {
        relativePath: 'created-after-refresh.txt',
        expectedWorkspaceRoot: workspace.root,
        expectedWorkspaceDigest: workspace.digest,
        expectedWorkspaceInstallation: workspace.installation,
      });
      return JSON.stringify({
        path: 'created-after-refresh.txt',
        text: 'new work from agent\n',
        text_editable: false,
        native_untracked: true,
        byte_count: 20,
        content_digest: 'digest-created-after-refresh',
        executable: false,
      });
    }
    if (command === 'preview_managed_workspace_version') {
      return savedWorkspacePreview(parameters.operation, [{ path: 'existing.txt', type: 'file', bytes: '1' }]);
    }
    if (command === 'open_managed_workspace_version') {
      versionOpenCalls += 1;
      throw new Error('a cancelled switch must not create a version workspace');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = (message) => {
    confirmation = message;
    return false;
  };
  await import(`./app.js?version-new-native-refresh=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  document.getElementById('workspace-version').value = operation;
  await document.getElementById('workspace-version').emit('change');
  await document.getElementById('fork-version').emit('click');

  assert.equal(stateReads, 2, 'switch preflight did not refresh the live native-file projection');
  assert.equal(versionOpenCalls, 0);
  assert.match(confirmation, /1 unsaved native change/);
  assert.equal(document.getElementById('folder-change-items').children.length, 1);
  assert.match(document.getElementById('folder-change-items').children[0].textContent, /created-after-refresh\.txt/);
  assert.match(document.getElementById('folder-change-items').children[0].textContent, /new native file/);
});

test('switching back to an agent folder preserves custody for the exact Finish inspection', async () => {
  const document = fakeDocument();
  const stableFolder = '/application/native-workspace/current';
  const current = {
    root: '/managed/agent-two/mounts',
    digest: 'workspace-agent-two',
    installation: 'installation-agent-two',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-agent-two' },
    shared_version: null,
    entries: [{ path: 'agent-two.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [{
      path: 'agent-two.txt',
      object_id: 'object-agent-two',
      current: { version_id: 'version-agent-two', manifest_id: 'manifest-agent-two' },
      retained_versions: [{ version_id: 'version-agent-two', manifest_id: 'manifest-agent-two' }],
    }],
    workspace_versions: [],
  };
  const prior = {
    ...current,
    root: '/managed/agent-one/mounts',
    digest: 'workspace-agent-one',
    installation: 'installation-agent-one',
    private_version: { version: 'version-agent-one' },
    entries: [{ path: 'agent-result.txt', type: 'file' }],
    file_histories: [{
      path: 'agent-result.txt',
      object_id: 'object-agent-result',
      current: { version_id: 'version-agent-one', manifest_id: 'manifest-agent-one' },
      retained_versions: [{ version_id: 'version-agent-one', manifest_id: 'manifest-agent-one' }],
    }],
  };
  let open = current;
  let inspectedPrior = 0;
  let inspectedCurrent = 0;
  let navigationIncludesPriorHandoff = false;
  let releaseCurrentScan;
  const currentScanHeld = new Promise((resolve) => {
    releaseCurrentScan = resolve;
  });
  const status = () => ({
    remembered: open.root,
    workspaces: [open.root, open.root === current.root ? prior.root : current.root],
    workspace_entries: [
      { path: current.root, export_root: '/ordinary/project', project_root: '/ordinary/project' },
      {
        path: prior.root,
        export_root: '/ordinary/project',
        project_root: '/ordinary/project',
        ...(navigationIncludesPriorHandoff
          ? { agent_handoff_installation: prior.installation, agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION }
          : {}),
      },
    ],
    auto_opened: false,
    active_folder: stableFolder,
    export_root: '/ordinary/project',
  });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify(status());
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: open.root,
        workspace_digest: open.digest,
        workspace_installation: open.installation,
        native_folder: true,
        native_folder_path: open.root,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      assert.equal(JSON.parse(parameters.paramsJson).path, prior.root);
      open = prior;
      return JSON.stringify(prior);
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(open);
    if (command === 'remember_managed_workspace') {
      navigationIncludesPriorHandoff = true;
      return JSON.stringify(status());
    }
    if (command === 'reveal_managed_workspace') {
      return JSON.stringify({ path: stableFolder, workspace_root: open.root, stable: true, native_folder: true });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'inspect_managed_file') {
      if (open.root === current.root) {
        inspectedCurrent += 1;
        if (inspectedCurrent === 1) await currentScanHeld;
        return JSON.stringify({
          path: 'agent-two.txt',
          text: 'unchanged agent two file\n',
          text_editable: true,
          modified_from_current_version: false,
          current_version: 'version-agent-two',
          byte_count: 25,
          content_digest: 'digest-agent-two-unchanged',
          executable: false,
        });
      }
      inspectedPrior += 1;
      assert.equal(open.root, prior.root);
      assert.equal(parameters.relativePath, 'agent-result.txt');
      return JSON.stringify({
        path: 'agent-result.txt',
        text: 'changed while agent two was selected\n',
        text_editable: true,
        modified_from_current_version: true,
        current_version: 'version-agent-one',
        byte_count: 37,
        content_digest: 'digest-agent-result-changed',
        executable: false,
      });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?agent-return-inspection=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const priorScan = document.getElementById('scan-files').emit('click');
  await waitFor(() => inspectedCurrent === 1);
  document.getElementById('recent-workspace').value = prior.root;
  await document.getElementById('recent-workspace').emit('change');
  const switchWorkspace = document.getElementById('open-recent-workspace').emit('click');
  await waitFor(() => open.root === prior.root);
  assert.equal(
    !document.workspaceCurrentAction('refresh')?.enabled,
    true,
    'another workspace action became available before the returned agent folder was inspected',
  );
  releaseCurrentScan();
  await Promise.all([priorScan, switchWorkspace]);

  assert.equal(inspectedPrior, 0, 'the returned assigned folder entered ordinary native inspection');
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), prior.root);
  assert.equal((document.workspaceCurrent?.current?.state ?? ''), 'Saved privately');
  assert.equal(document.getElementById('folder-change-items').children.length, 0);
  assert.match(document.getElementById('notice').textContent, /Managed workspace reopened from durable local history/);
  assert.match(document.getElementById('notice').textContent, /Reopen any editor or terminal that was already using the prior folder/);
  assert.match(document.getElementById('notice').textContent, /existing directory handles stay on that prior version/);
  assert.equal(document.getElementById('scan-files').disabled, true);
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Agent folder is assigned');
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Finish agent handoff');
  assert.equal(
    (document.workspaceCurrentAction('start-codex')?.label ?? ''),
    'Reopen assigned Codex folder',
    'committed navigation did not restore the prior exact agent handoff',
  );
});

test('a recent-workspace opener failure keeps the verified switch and offers one retry', async () => {
  const document = fakeDocument();
  const stableFolder = '/application/native-workspace/current';
  const first = {
    root: '/managed/first/mounts',
    digest: 'workspace-first',
    installation: 'installation-first',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-first' },
    shared_version: null,
    entries: [{ path: 'first.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [],
    workspace_versions: [],
  };
  const second = {
    ...first,
    root: '/managed/second/mounts',
    digest: 'workspace-second',
    installation: 'installation-second',
    private_version: { version: 'version-second' },
    entries: [{ path: 'second.txt', type: 'file' }],
  };
  let open = first;
  let openerFails = true;
  let revealAttempts = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({ remembered: first.root, workspaces: [first.root, second.root], auto_opened: false, active_folder: stableFolder });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: open.root, workspace_digest: open.digest, workspace_installation: open.installation, working: false });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(open);
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      assert.equal(JSON.parse(parameters.paramsJson).path, second.root);
      open = second;
      return JSON.stringify(second);
    }
    if (command === 'remember_managed_workspace') {
      return JSON.stringify({ remembered: second.root, workspaces: [second.root, first.root], auto_opened: false, active_folder: stableFolder });
    }
    if (command === 'reveal_managed_workspace') {
      revealAttempts += 1;
      assert.deepEqual(parameters, {
        expectedWorkspaceRoot: second.root,
        expectedWorkspaceDigest: second.digest,
        expectedWorkspaceInstallation: second.installation,
      });
      if (openerFails) throw new Error('Finder is temporarily unavailable');
      return JSON.stringify({ path: stableFolder, workspace_root: second.root, stable: true, native_folder: true });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?recent-opener-retry=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  document.getElementById('recent-workspace').value = second.root;
  await document.getElementById('recent-workspace').emit('change');
  await document.getElementById('open-recent-workspace').emit('click');
  await waitFor(() => (document.workspaceCurrent?.current?.agentFolder ?? '') === second.root);
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), second.root);
  assert.equal((document.workspaceCurrent?.current?.workingFolder ?? ''), stableFolder);
  assert.equal(document.getElementById('manage-path').disabled, false);
  assert.equal((document.workspaceCurrentAction('open-folder')?.label ?? ''), 'Open working folder');
  assert.match(document.getElementById('notice').textContent, /Mesh switched successfully/);
  assert.match(document.getElementById('notice').textContent, /Use Open working folder to try again/);
  assert.equal(document.getElementById('notice').classList.contains('error'), true);

  openerFails = false;
  await document.emitWorkspaceCurrentIntent('open-folder');
  assert.equal(revealAttempts, 2);
  assert.match(document.getElementById('notice').textContent, /Opened the stable native folder/);
  assert.equal(document.getElementById('notice').classList.contains('error'), false);
});

test('an unavailable recent folder leaves the current verified workspace usable', async () => {
  const document = fakeDocument();
  let pageRequest = null;
  document.addEventListener('mesh:workspace-page-request', (event) => {
    pageRequest = event.detail;
  });
  const workspace = {
    root: '/managed/current/mounts',
    digest: 'workspace-current-stable',
    installation: 'installation-current-stable',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-current-stable' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
    workspace_versions: [],
  };
  const unavailable = '/managed/no-longer-here/mounts';
  let recentPaths = [workspace.root, unavailable];
  let stateReads = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: workspace.root,
        workspaces: recentPaths,
        auto_opened: false,
      });
    }
    if (command === 'forget_managed_workspace') {
      recentPaths = recentPaths.filter((path) => path !== parameters.path);
      return JSON.stringify({
        remembered: recentPaths[0] || null,
        workspaces: recentPaths,
        auto_opened: false,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      stateReads += 1;
      return JSON.stringify(workspace);
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      assert.equal(JSON.parse(parameters.paramsJson).path, unavailable);
      throw daemonRefusal('workspace-unreachable', 'The selected recent workspace is unavailable');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?unavailable-recent=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  document.getElementById('recent-workspace').value = unavailable;
  await document.getElementById('recent-workspace').emit('change');
  assert.equal(document.getElementById('open-recent-workspace').disabled, false);
  assert.equal(document.getElementById('forget-recent-workspace').disabled, false);
  assert.match(document.getElementById('recent-workspace-hint').textContent, /navigation shortcut/);
  assert.match(document.getElementById('recent-workspace-hint').textContent, /saved history are not changed/);
  await document.getElementById('open-recent-workspace').emit('click');
  assert.match(document.getElementById('notice').textContent, /Saved workspace unavailable/);
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), workspace.root);
  assert.equal(document.getElementById('manage-path').disabled, false);
  assert.ok(stateReads >= 2, 'the current workspace was not reverified after the refusal');
  assert.match(document.getElementById('notice').textContent, /current workspace is still open and unchanged/);
  assert.match(document.getElementById('notice').textContent, /Forget from list/);
  assert.doesNotMatch(document.getElementById('notice').textContent, /Error:|Application Support|could not confirm the requested open/);
  assert.equal(
    document.getElementById('recent-workspace').children
      .find((option) => option.value === unavailable).textContent.startsWith('Unavailable ·'),
    true,
  );
  assert.equal(document.getElementById('open-recent-workspace').textContent, 'Try again');
  assert.match(document.getElementById('recent-workspace-hint').textContent, /Restore its saved folder/);
  assert.deepEqual(pageRequest, { page: 'workspaces', selector: '#workspace-entry-recent' });

  await document.getElementById('forget-recent-workspace').emit('click');
  await waitFor(() => recentPaths.length === 1);
  assert.deepEqual(recentPaths, [workspace.root]);
  assert.equal(document.getElementById('recent-workspace').children.length, 1);
  assert.match(document.getElementById('notice').textContent, /folder and saved history were not changed/);
  assert.equal(document.getElementById('recent-workspace').value, workspace.root);
  assert.equal(document.getElementById('forget-recent-workspace').disabled, true);
  assert.match(document.getElementById('forget-recent-workspace').title, /must stay remembered/);
  assert.match(document.getElementById('recent-workspace-hint').textContent, /open workspace/);
  assert.match(document.getElementById('recent-workspace-hint').textContent, /reopen it after restart/);
  await document.getElementById('forget-recent-workspace').emit('click');
  assert.deepEqual(recentPaths, [workspace.root], 'the open workspace lost its restart route');
  assert.equal(document.workspaceEntry.entry.canForgetRecent, false);
});

test('a lost manual managed-workspace open reply recovers the workspace native actually opened', async () => {
  const document = fakeDocument();
  const first = {
    root: '/managed/manual-first/mounts',
    digest: 'workspace-manual-first',
    installation: 'installation-manual-first',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-manual-first' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
    workspace_versions: [],
  };
  const second = {
    ...first,
    root: '/managed/manual-second/mounts',
    digest: 'workspace-manual-second',
    installation: 'installation-manual-second',
    private_version: { version: 'version-manual-second' },
  };
  let open = first;
  let stateReads = 0;
  let rememberedPath = null;
  let openAttempts = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: open.root,
        workspaces: [open.root],
        auto_opened: false,
        active_folder: null,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: open.root,
        workspace_digest: open.digest,
        workspace_installation: open.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      stateReads += 1;
      return JSON.stringify(open);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      assert.equal(JSON.parse(parameters.paramsJson).path, second.root);
      openAttempts += 1;
      open = second;
      throw new Error('the renderer lost the completed open reply');
    }
    if (command === 'remember_managed_workspace') {
      rememberedPath = parameters.path;
      return JSON.stringify({
        remembered: parameters.path,
        workspaces: [parameters.path],
        auto_opened: false,
        active_folder: null,
      });
    }
    throw new Error(`unexpected manual-open command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?manual-open-lost-reply=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const entryGeneration = document.workspaceEntry.generation;
  await document.emitWorkspaceEntryIntent({ type: 'update-managed-path', path: second.root }, entryGeneration);
  assert.ok(
    document.workspaceEntry.generation > entryGeneration,
    'the source-owned draft did not publish a replacement generation before immediate Enter',
  );
  await document.emitWorkspaceEntryIntent({ type: 'open-managed-path', path: second.root }, entryGeneration);
  await waitFor(() => (document.workspaceCurrent?.current?.agentFolder ?? '') === second.root);

  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), second.root);
  assert.equal(document.serviceState.state === 'ready', true);
  assert.equal(openAttempts, 1, 'immediate Enter did not execute the exact latest source-owned path once');
  assert.equal(rememberedPath, second.root, 'the recovered workspace would not reopen after restart');
  assert.ok(stateReads >= 4, 'the native workspace selected after the lost reply was not reverified after navigation repair');
  assert.match(document.getElementById('notice').textContent, /could not confirm the requested open/i);
  assert.match(document.getElementById('notice').textContent, /recovered and verified/i);
  assert.doesNotMatch(document.getElementById('notice').textContent, new RegExp(second.root));

  await document.emitWorkspaceCurrentIntent('refresh');
  assert.equal(openAttempts, 1, 'Refresh replayed the source-owned managed-path open');
  assert.equal(
    document.workspaceEntry.entry.openPath,
    second.root,
    'Refresh discarded the exact source-owned managed-path draft',
  );
});

test('a refused manual managed-workspace path explains Import without exposing private storage', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/current/mounts',
    digest: 'workspace-current-manual-refusal',
    installation: 'installation-current-manual-refusal',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-current-manual-refusal' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
    workspace_versions: [],
  };
  const ordinaryFolder = '/Users/person/Documents/ordinary-project';
  let stateReads = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: workspace.root,
        workspaces: [workspace.root],
        auto_opened: false,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      stateReads += 1;
      return JSON.stringify(workspace);
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      assert.equal(JSON.parse(parameters.paramsJson).path, ordinaryFolder);
      throw daemonRefusal('workspace-unreachable', 'internal native path detail');
    }
    throw new Error(`unexpected manual refusal command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?manual-open-refused=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.emitWorkspaceEntryIntent(
    { type: 'update-managed-path', path: ordinaryFolder },
    document.workspaceEntry.generation,
  );
  await document.emitWorkspaceEntryIntent(
    { type: 'open-managed-path', path: ordinaryFolder },
    document.workspaceEntry.generation,
  );
  assert.match(document.getElementById('notice').textContent, /not an available saved Mesh workspace/);

  const notice = document.getElementById('notice').textContent;
  assert.match(notice, /choose Import for an ordinary project folder/);
  assert.match(notice, /current workspace is still open and unchanged/);
  assert.doesNotMatch(notice, /Error:|internal native path detail|\/Users\/person/);
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), workspace.root);
  assert.ok(stateReads >= 3, 'the current workspace was not reverified after the deterministic refusal');
});

test('an agent-assigned recent workspace switches directly to its verified finish control', async () => {
  const document = fakeDocument();
  const current = {
    root: '/managed/current-agent-custody/mounts',
    digest: 'workspace-current-agent-custody',
    installation: 'installation-current-agent-custody',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-current-agent-custody' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
    workspace_versions: [],
  };
  const assigned = '/managed/assigned-agent-custody/mounts';
  const assignedInstallation = 'installation-assigned-agent-custody';
  const assignedWorkspace = {
    ...current,
    root: assigned,
    digest: 'workspace-assigned-agent-custody',
    installation: assignedInstallation,
  };
  let openWorkspace = current;
  let forgetCalls = 0;
  const recentStatus = () => ({
    remembered: openWorkspace.root,
    workspaces: openWorkspace.root === assigned
      ? [assigned, current.root]
      : [current.root, assigned],
    workspace_entries: [
      {
        path: current.root,
        agent_handoff_installation: null,
        agent_handoff_generation: null,
      },
      {
        path: assigned,
        agent_handoff_installation: assignedInstallation,
        agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
      },
    ],
    active_folder: null,
    auto_opened: false,
  });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify(recentStatus());
    }
    if (command === 'inspect_agent_live_work') {
      assert.deepEqual(parameters, {
        expectedWorkspaceRoot: assigned,
        expectedWorkspaceDigest: assignedWorkspace.digest,
        expectedWorkspaceInstallation: assignedInstallation,
        expectedAgentHandoffGeneration: TEST_AGENT_HANDOFF_GENERATION,
      });
      return JSON.stringify({
        schema: 'mesh.agent-live-work/v1',
        workspace_root: assigned,
        workspace_digest: assignedWorkspace.digest,
        workspace_installation: assignedInstallation,
        agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
        changes: [{ path: 'notes/live.txt', kind: 'new-file' }],
      });
    }
    if (command === 'forget_managed_workspace') {
      forgetCalls += 1;
      throw new Error(`assigned workspace reached native forget: ${parameters.path}`);
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: openWorkspace.root,
        workspace_digest: openWorkspace.digest,
        workspace_installation: openWorkspace.installation,
        native_folder: openWorkspace.root === assigned,
        native_folder_path: openWorkspace.root === assigned ? assigned : null,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(openWorkspace);
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      assert.deepEqual(JSON.parse(parameters.paramsJson), { path: assigned });
      openWorkspace = assignedWorkspace;
      return JSON.stringify(openWorkspace);
    }
    if (command === 'remember_managed_workspace') {
      assert.equal(parameters.path, assigned);
      return JSON.stringify(recentStatus());
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    throw new Error(`unexpected custody command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?assigned-recent-custody=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const assignedChoice = document.workspaceCurrent.current.workspaces.find((candidate) => candidate.path === assigned);
  assert.deepEqual(assignedChoice, {
    path: assigned,
    label: 'assigned-agent-custody',
    state: 'agent-assigned',
    canOpen: true,
  });
  assert.equal(forgetCalls, 0);

  await document.emitWorkspaceCurrentIntent({ type: 'switch-workspace', path: assigned });
  await waitFor(() => (document.workspaceCurrent?.current?.agentFolder ?? '') === assigned);
  await waitFor(() => document.workspaceCurrent?.current?.agentActivity?.state === 'ready');
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), assigned);
  assert.equal(Boolean(document.workspaceCurrentAction('finish-agent')), true);
  assert.equal(!document.workspaceCurrentAction('finish-agent')?.enabled, false);
  assert.deepEqual(document.workspaceCurrent.current.agentActivity.changes, [
    { path: 'notes/live.txt', kind: 'new-file' },
  ]);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-mounted', {
    detail: { generation: document.workspaceCurrent.generation },
  }));
  assert.equal(document.getElementById('workspace-current-next').classList.contains('hidden'), false);
  assert.match(document.getElementById('notice').textContent, /Opened the exact workspace assigned to a running agent/);
  assert.match(document.getElementById('notice').textContent, /Live changes are read-only/);
});

test('Files previews unchanged JSON and text while exact agent custody stays active', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/assigned-preview/mounts',
    digest: 'workspace-assigned-preview',
    installation: 'installation-assigned-preview',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-assigned-preview' },
    shared_version: null,
    entries: [
      { path: 'mesh-test-one.txt', type: 'file' },
      { path: 'mesh-test-three.json', type: 'file' },
    ],
    conditions: [],
    not_yet: [],
    file_histories: [
      {
        path: 'mesh-test-one.txt',
        object_id: 'object-assigned-preview-text',
        current: { version_id: 'version-assigned-preview-text', manifest_id: 'manifest-assigned-preview-text' },
        retained_versions: [{ version_id: 'version-assigned-preview-text', manifest_id: 'manifest-assigned-preview-text' }],
      },
      {
        path: 'mesh-test-three.json',
        object_id: 'object-assigned-preview-json',
        current: { version_id: 'version-assigned-preview-json', manifest_id: 'manifest-assigned-preview-json' },
        retained_versions: [{ version_id: 'version-assigned-preview-json', manifest_id: 'manifest-assigned-preview-json' }],
      },
    ],
    workspace_versions: [],
  };
  const previewed = [];
  const recent = {
    remembered: workspace.root,
    workspaces: [workspace.root],
    workspace_entries: [{
      path: workspace.root,
      agent_handoff_installation: workspace.installation,
      agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
    }],
    active_folder: null,
    auto_opened: false,
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify(recent);
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: true,
        native_folder_path: workspace.root,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'inspect_agent_live_work') {
      return JSON.stringify({
        schema: 'mesh.agent-live-work/v1',
        workspace_root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
        changes: [],
      });
    }
    if (command === 'inspect_agent_live_file') {
      assert.equal(parameters.expectedWorkspaceRoot, workspace.root);
      assert.equal(parameters.expectedWorkspaceDigest, workspace.digest);
      assert.equal(parameters.expectedWorkspaceInstallation, workspace.installation);
      assert.equal(parameters.expectedAgentHandoffGeneration, TEST_AGENT_HANDOFF_GENERATION);
      previewed.push(parameters.relativePath);
      const text = parameters.relativePath.endsWith('.json')
        ? '{\n  "preview": true\n}\n'
        : 'plain text preview\n';
      return JSON.stringify({
        schema: 'mesh.agent-live-file/v1',
        workspace_root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
        path: parameters.relativePath,
        kind: 'current-file',
        byte_count: text.length,
        content_digest: 'ab'.repeat(32),
        executable: false,
        text,
        preview_kind: 'text',
        image_data_url: null,
        preview_error: null,
        mutable: true,
        recorded: false,
      });
    }
    throw new Error(`unexpected assigned preview command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?assigned-files-preview=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.emitWorkspaceWorkIntent({
    type: 'set-field',
    field: 'selectedEntry',
    value: 'mesh-test-three.json',
  });
  await waitFor(() => document.workspaceWork?.workbench.changes.editorText.includes('"preview": true'));
  assert.deepEqual(previewed, ['mesh-test-three.json']);
  assert.equal(document.workspaceWork.workbench.files.workspaceState, 'agent-assigned');
  assert.equal(document.workspaceWork.workbench.changes.editorKind, 'text');
  assert.equal(document.workspaceWork.workbench.changes.canEditText, false);
});

test('an unchanged Current refresh renews actions without publishing another painted tree', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/no-flicker-refresh',
    digest: 'workspace-no-flicker-refresh',
    installation: 'installation-no-flicker-refresh',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-no-flicker-refresh' },
    shared_version: null,
    entries: [{ path: 'stable.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'stable.txt',
      object_id: 'object-no-flicker-refresh',
      current: { version_id: 'version-no-flicker-refresh', manifest_id: 'manifest-no-flicker-refresh' },
      retained_versions: [{ version_id: 'version-no-flicker-refresh', manifest_id: 'manifest-no-flicker-refresh' }],
    }],
    workspace_versions: [],
  };
  const recent = {
    remembered: workspace.root,
    workspaces: [workspace.root],
    workspace_entries: [{ path: workspace.root, agent_handoff_installation: null, agent_handoff_generation: null }],
    active_folder: null,
    auto_opened: false,
  };
  let stateReads = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify(recent);
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: true,
        native_folder_path: workspace.root,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      stateReads += 1;
      return JSON.stringify(workspace);
    }
    if (command === 'approval_credential_status') {
      return JSON.stringify({ enrolled: false, available: false, unavailable_reason: 'Unavailable in test.' });
    }
    throw new Error(`unexpected no-flicker refresh command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?no-flicker-refresh=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  const mountedGeneration = document.workspaceCurrent.generation;
  let repaints = 0;
  document.addEventListener('mesh:workspace-current-projection', () => { repaints += 1; });

  await document.emitWorkspaceCurrentIntent('refresh', mountedGeneration);
  await waitFor(() => stateReads >= 2);
  assert.equal(document.workspaceCurrent.generation, mountedGeneration);
  assert.equal(repaints, 0, 'an unchanged native refresh republished the full Current React tree');
});

test('rollback removes the stable folder through the canonical deleted workspace identity', async () => {
  const document = fakeDocument();
  const serviceStates = [];
  document.addEventListener('mesh:service-state-projection', (event) => serviceStates.push(event.detail));
  const selectedRoot = '/var/folders/demo/managed.mesh/mounts';
  const canonicalRoot = '/private/var/folders/demo/managed.mesh/mounts';
  const stableFolder = '/application/native-workspace/current';
  const workspace = {
    root: selectedRoot,
    digest: 'workspace-before-rollback',
    installation: 'installation-before-rollback',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-before-rollback' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
    workspace_versions: [],
    native_untracked_files: [],
  };
  let forgottenPath = null;
  let rollbackCalls = 0;
  let recentStatus = {
    remembered: canonicalRoot,
    workspaces: [canonicalRoot],
    auto_opened: false,
    active_folder: stableFolder,
    warning: null,
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify(recentStatus);
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'rollback_managed_workspace') {
      rollbackCalls += 1;
      assert.deepEqual(parameters, {
        expectedWorkspaceRoot: workspace.root,
        expectedWorkspaceDigest: workspace.digest,
        expectedWorkspaceInstallation: workspace.installation,
      });
      return JSON.stringify({
        action: 'folder-import-rolled-back',
        destination: selectedRoot,
        workspace_root: canonicalRoot,
        original_preserved: true,
      });
    }
    if (command === 'forget_managed_workspace') {
      forgottenPath = parameters.path;
      recentStatus = {
        remembered: null,
        workspaces: [],
        auto_opened: false,
        active_folder: null,
        warning: null,
      };
      throw new Error('forget reply was lost after native removal');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  let browserConfirmations = 0;
  globalThis.confirm = () => { browserConfirmations += 1; return true; };
  await import(`./app.js?rollback-native-folder=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const chromeProjections = [];
  document.addEventListener('mesh:workspace-chrome-projection', (event) => chromeProjections.push(event.detail));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-chrome-available'));
  assert.equal(chromeProjections.at(-1).chrome.workspaceReady, true);
  const openWorkspaceIdentity = chromeProjections.at(-1).workspaceIdentity;
  const projections = [];
  document.addEventListener('mesh:confirmation-projection', (event) => projections.push(event.detail));
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-available'));

  const staleRollback = document.emitWorkspaceCurrentIntent('rollback');
  await waitFor(() => projections.length === 1);
  assert.equal(projections[0].confirmation.title, 'Remove this managed workspace?');
  assert.equal(projections[0].confirmation.confirmLabel, 'Roll back managed copy');
  assert.equal(projections[0].confirmation.cancelLabel, 'Keep workspace');
  assert.equal(projections[0].confirmation.tone, 'destructive');
  // A fresh native verification supersedes the coordinator generation even when it reads back
  // identical visible facts. The first dialog must not authorize the newer verified session.
  await document.emitWorkspaceCurrentIntent('refresh');
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-mounted', {
    detail: { generation: projections[0].generation },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-intent', {
    detail: { generation: projections[0].generation, intent: { type: 'confirm' } },
  }));
  await staleRollback;
  assert.equal(rollbackCalls, 0, 'stale rollback confirmation removed a newer workspace state');
  assert.match(document.getElementById('notice').textContent, /workspace changed while rollback confirmation was open/i);
  assert.equal(!document.workspaceCurrentAction('rollback')?.enabled, false);

  const confirmedRollback = document.emitWorkspaceCurrentIntent('rollback');
  await waitFor(() => projections.length === 2);
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-mounted', {
    detail: { generation: projections[1].generation },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-intent', {
    detail: { generation: projections[1].generation, intent: { type: 'confirm' } },
  }));
  await confirmedRollback;
  assert.equal(browserConfirmations, 0);
  assert.equal(rollbackCalls, 1);
  assert.equal(forgottenPath, canonicalRoot);
  assert.equal(document.getElementById('recent-workspace').children.length, 0);
  assert.equal(document.getElementById('recent-workspace').value, '');
  assert.equal(document.workspaceCurrentAction('rollback')?.enabled, false);
  assert.equal(chromeProjections.at(-1).chrome.workspaceReady, false);
  assert.ok(
    chromeProjections.at(-1).workspaceIdentity > openWorkspaceIdentity,
    'rollback did not close the React page shell identity',
  );
  assert.match(document.getElementById('notice').textContent, /confirmed the entry was removed/);
  assert.equal(serviceStates.at(-1)?.state, 'ready');
});

test('a lost rollback reply recovers the authoritative empty workspace without replaying deletion', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/lost-rollback-reply.mesh/mounts',
    digest: 'workspace-before-lost-rollback-reply',
    installation: 'installation-before-lost-rollback-reply',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-before-lost-rollback-reply' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
    workspace_versions: [],
    native_untracked_files: [],
  };
  let open = true;
  let rollbackCalls = 0;
  let forgetCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: workspace.root,
        workspaces: [workspace.root],
        workspace_entries: [{
          path: workspace.root,
          export_root: null,
          project_root: null,
          agent_handoff_installation: null,
          agent_handoff_generation: null,
        }],
        auto_opened: false,
        active_folder: null,
        export_root: null,
        warning: null,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      if (!open) throw daemonRefusal('no-workspace-open', 'No managed workspace is open');
      return JSON.stringify(workspace);
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'rollback_managed_workspace') {
      rollbackCalls += 1;
      assert.deepEqual(parameters, {
        expectedWorkspaceRoot: workspace.root,
        expectedWorkspaceDigest: workspace.digest,
        expectedWorkspaceInstallation: workspace.installation,
      });
      open = false;
      throw new Error('the renderer lost the completed rollback reply');
    }
    if (command === 'forget_managed_workspace') {
      forgetCalls += 1;
      throw new Error('ambiguous rollback recovery must not guess that a same-path recent entry is stale');
    }
    throw new Error(`unexpected lost-rollback command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?lost-rollback-reply=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.emitWorkspaceCurrentIntent('rollback');

  assert.equal(rollbackCalls, 1, 'the destructive rollback was replayed after its reply became ambiguous');
  assert.equal(forgetCalls, 0, 'recovery removed an unverified same-path navigation entry');
  assert.equal(document.workspaceCurrentAction('rollback')?.enabled, false);
  assert.equal(document.serviceState.state === 'ready', true);
  assert.equal(!document.workspaceCurrentAction('rollback')?.enabled, true);
  assert.match(document.getElementById('notice').textContent, /could not confirm the rollback reply/i);
  assert.match(document.getElementById('notice').textContent, /verified that no managed workspace is open/i);
  assert.match(document.getElementById('notice').textContent, /Recent workspaces/i);
});

test('a refused rollback reply reverifies the unchanged workspace without replaying deletion', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/refused-rollback-reply.mesh/mounts',
    digest: 'workspace-after-refused-rollback',
    installation: 'installation-after-refused-rollback',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-after-refused-rollback' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
    workspace_versions: [],
    native_untracked_files: [],
  };
  let rollbackCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: workspace.root,
        workspaces: [workspace.root],
        workspace_entries: [{
          path: workspace.root,
          export_root: null,
          project_root: null,
          agent_handoff_installation: null,
          agent_handoff_generation: null,
        }],
        auto_opened: false,
        active_folder: null,
        export_root: null,
        warning: null,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'rollback_managed_workspace') {
      rollbackCalls += 1;
      assert.deepEqual(parameters, {
        expectedWorkspaceRoot: workspace.root,
        expectedWorkspaceDigest: workspace.digest,
        expectedWorkspaceInstallation: workspace.installation,
      });
      throw new Error('the daemon refused rollback before removing the managed copy');
    }
    throw new Error(`unexpected refused-rollback command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?refused-rollback-reply=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.emitWorkspaceCurrentIntent('rollback');

  assert.equal(rollbackCalls, 1, 'the refused destructive rollback was replayed');
  assert.notEqual(document.workspaceCurrent, null);
  assert.equal(document.serviceState.state === 'ready', true);
  assert.match(document.getElementById('notice').textContent, /same managed workspace remains open/i);
  assert.match(document.getElementById('notice').textContent, /Nothing was replayed/i);
});

test('an unavailable sole recent folder does not claim a current workspace remains open', async () => {
  const document = fakeDocument();
  const unavailable = '/managed/missing/mounts';
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: unavailable,
        workspaces: [unavailable],
        auto_opened: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      throw daemonRefusal('no-workspace-open', 'No managed workspace is open');
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      throw daemonRefusal('workspace-unreachable', 'The selected recent workspace is unavailable');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?unavailable-sole-recent=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.equal(document.getElementById('workspace-entry-controls').open, true);
  assert.equal(document.getElementById('workspace-entry-summary').textContent, 'Return to a recent Mesh workspace');

  await document.getElementById('open-recent-workspace').emit('click');
  await waitFor(() => /Saved workspace unavailable/.test(document.getElementById('notice').textContent));
  assert.match(document.getElementById('notice').textContent, /No workspace was opened, and nothing changed/);
  assert.doesNotMatch(document.getElementById('notice').textContent, /current workspace remains open/);
  assert.doesNotMatch(document.getElementById('notice').textContent, /Error:|\/managed\/missing/);
});

test('a durable review approves only through the native user-presence command', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/review',
    digest: 'workspace-review',
    installation: 'installation-review',
    records: 3,
    reviews: 1,
    review_items_not_listed: 0,
    review_items: [{
      bundle: 'aa'.repeat(32),
      subject_operation: 'bb'.repeat(32),
      reviewed_head: 'ab'.repeat(32),
      opened_by: 'cc'.repeat(32),
      author: 'dd'.repeat(32),
      actor_sequence: '9007199254740993',
      subject_operations: [{ kind: 'WriteFileVersion', object_id: 'ee'.repeat(32), version_id: 'ff'.repeat(32) }],
      subject_operations_not_listed: 0,
      presentation_digest: '11'.repeat(32),
      bundle_changes: [{
        object_id: 'ee'.repeat(16),
        path_before: '/notes.txt',
        path_after: '/notes.txt',
        effect: 'content-written',
        before: { kind: 'text', version_id: '22'.repeat(32), content_digest: null, byte_length: null, line_count: '2' },
        after: { kind: 'text', version_id: '44'.repeat(32), content_digest: null, byte_length: null, line_count: '2' },
        body: 'text',
        opaque_reason: null,
        verified_text: {
          source: 'before-after',
          before: {
            version_id: '22'.repeat(32),
            content_digest: '33'.repeat(32),
          },
          after: {
            version_id: '44'.repeat(32),
            content_digest: '55'.repeat(32),
          },
          hunks: [{
            before_start: 1,
            before_len: 2,
            after_start: 1,
            after_len: 2,
            lines: [
              { kind: 'removed', before: 1, after: null, text: 'old alpha line' },
              { kind: 'added', before: null, after: 1, text: 'alpha line' },
              { kind: 'context', before: 2, after: 2, text: 'second line' },
            ],
          }],
        },
      }],
      bundle_changes_not_listed: 0,
      content_complete: true,
      unavailable_code: null,
      projection_authorizes_approval: false,
    }],
    private_version: { version: 'version-review', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'notes.txt',
      object_id: 'ee'.repeat(16),
      current: { version_id: '44'.repeat(32), manifest_id: '45'.repeat(32) },
      retained_versions: [{ version_id: '44'.repeat(32), manifest_id: '45'.repeat(32) }],
    }, {
      path: 'summary.txt',
      object_id: 'ef'.repeat(16),
      current: { version_id: '48'.repeat(32), manifest_id: '49'.repeat(32) },
      retained_versions: [{ version_id: '48'.repeat(32), manifest_id: '49'.repeat(32) }],
    }],
    workspace_versions: [{ operation: 'bb'.repeat(32), ordinal: 1, actor_sequence: '9007199254740993' }],
  };
  let approveParameters = null;
  let approvalAttempts = 0;
  let exportRetryParameters = null;
  let exportInspectionParameters = null;
  let exportedBranchExists = true;
  let reviewScopeInspections = 0;
  let exportedOriginal = false;
  let completedOriginalVersion = null;
  let privateExportPickerCalls = 0;
  let privateExportPickerResult = '/ordinary/review';
  const managedPathOpenAttempts = [];
  const recentOpenPaths = [];
  const recentForgetPaths = [];
  const singleExportPreviewParameters = [];
  const batchExportPreviewParameters = [];
  let savedSideOpenParameters = null;
  let releaseSavedSideOpen = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      auto_opened: false,
      remembered: workspace.root,
      workspaces: [workspace.root, '/managed/alternate-a', '/managed/alternate-b'],
      export_root: '/ordinary/review',
      workspace_entries: [
        { path: workspace.root, export_root: '/ordinary/review', project_root: '/ordinary/review' },
        { path: '/managed/alternate-a', export_root: '/ordinary/alternate-a', project_root: '/ordinary/alternate-a' },
        { path: '/managed/alternate-b', export_root: '/ordinary/alternate-b', project_root: '/ordinary/alternate-b' },
      ],
    });
    if (command === 'approval_credential_status') {
      return JSON.stringify({ enrolled: true, available: true, unavailable_reason: null, algorithm: 'es256', user_verification: 'user-presence' });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      const path = JSON.parse(parameters.paramsJson).path;
      if (path === '/managed/alternate-a') recentOpenPaths.push(path);
      else managedPathOpenAttempts.push(path);
      throw new Error('a rejected managed-path intent reached workspace.open');
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'pick_folder') {
      privateExportPickerCalls += 1;
      return privateExportPickerResult;
    }
    if (command === 'preview_managed_exports') {
      batchExportPreviewParameters.push(parameters);
      return JSON.stringify(workspace.file_histories.map((history) => {
        const identical = history.path === 'summary.txt';
        return {
          path: history.path,
          source_version: history.current.version_id,
          source_byte_count: 11,
          source_content_digest: '46'.repeat(32),
          source_executable: false,
          source_text: null,
          target_root: parameters.targetRoot,
          target_installation: 'ordinary-review-installation',
          target_parent_installation: 'ordinary-review-parent',
          target_file_installation: 'ordinary-review-file',
          target_exists: true,
          target_byte_count: identical ? 11 : 8,
          target_content_digest: identical ? '46'.repeat(32) : '47'.repeat(32),
          target_executable: false,
          target_text: null,
          identical,
          target_relation: 'imported-unchanged',
          replace_allowed: true,
        };
      }));
    }
    if (command === 'preview_managed_export') {
      singleExportPreviewParameters.push(parameters);
      const history = workspace.file_histories.find((candidate) => candidate.path === parameters.relativePath);
      return JSON.stringify({
        path: parameters.relativePath,
        source_version: history.current.version_id,
        source_byte_count: 11,
        source_content_digest: '46'.repeat(32),
        source_executable: false,
        source_text: 'saved text\n',
        target_root: parameters.targetRoot,
        target_installation: 'ordinary-review-installation',
        target_parent_installation: 'ordinary-review-parent',
        target_file_installation: 'ordinary-review-file',
        target_exists: false,
        target_byte_count: null,
        target_content_digest: null,
        target_executable: null,
        target_text: null,
        identical: false,
        target_relation: 'absent',
        replace_allowed: true,
      });
    }
    if (command === 'discover_retired_exports') return '[]';
    if (command === 'export_managed_file') {
      exportedOriginal = true;
      return JSON.stringify({ path: parameters.relativePath, target_root: parameters.targetRoot, created: false });
    }
    if (command === 'remember_managed_workspace') {
      if (parameters.originalUpdateVersion) {
        completedOriginalVersion = parameters.originalUpdateVersion;
      }
      return JSON.stringify({
        auto_opened: false,
        remembered: workspace.root,
        workspaces: [workspace.root],
        export_root: '/ordinary/review',
        workspace_entries: [{ path: workspace.root, export_root: '/ordinary/review', project_root: '/ordinary/review' }],
      });
    }
    if (command === 'forget_managed_workspace') {
      recentForgetPaths.push(parameters.path);
      return JSON.stringify({
        auto_opened: false,
        remembered: workspace.root,
        workspaces: [workspace.root, '/managed/alternate-a'],
        export_root: '/ordinary/review',
        workspace_entries: [
          { path: workspace.root, export_root: '/ordinary/review', project_root: '/ordinary/review' },
          { path: '/managed/alternate-a', export_root: null, project_root: null },
        ],
      });
    }
    if (command === 'inspect_managed_file') {
      reviewScopeInspections += 1;
      const history = workspace.file_histories.find((candidate) => candidate.path === parameters.relativePath);
      return JSON.stringify({
        path: parameters.relativePath,
        text: 'saved text\n',
        text_editable: true,
        native_untracked: false,
        modified_from_current_version: false,
        current_version: history.current.version_id,
        byte_count: 11,
        content_digest: '46'.repeat(32),
        executable: false,
        installation: workspace.installation,
      });
    }
    if (command === 'approve_current_review') {
      approveParameters = parameters;
      approvalAttempts += 1;
      if (approvalAttempts === 1) {
        return JSON.stringify({
          workspace: { ...workspace, shared_version: workspace.review_items[0].reviewed_head },
          receipt: {
            protocol: 'mesh.v1.approval-receipt',
            canonical_receipt_blake3: '61'.repeat(32),
            canonical_statement_blake3: '62'.repeat(32),
            credential_id: '63'.repeat(32),
            review_bundle: 'tampered-bundle',
            shared_version: workspace.review_items[0].reviewed_head,
            user_verification: 'user-presence',
          },
          git_export: { status: 'not-requested', target: null, branch: null, commit: null, approval_ref: null, already_present: false, message: null },
        });
      }
      if (approvalAttempts === 2) {
        return JSON.stringify({
          workspace: { ...workspace, shared_version: null },
          receipt: {
            protocol: 'mesh.v1.approval-receipt',
            canonical_receipt_blake3: '61'.repeat(32),
            canonical_statement_blake3: '62'.repeat(32),
            credential_id: '63'.repeat(32),
            review_bundle: workspace.review_items[0].bundle,
            shared_version: workspace.review_items[0].reviewed_head,
            user_verification: 'user-presence',
          },
          git_export: { status: 'not-requested', target: null, branch: null, commit: null, approval_ref: null, already_present: false, message: null },
        });
      }
      if (approvalAttempts === 3) {
        return JSON.stringify({
          workspace: {
            ...workspace,
            root: '/managed/substituted-review',
            shared_version: workspace.review_items[0].reviewed_head,
          },
          receipt: {
            protocol: 'mesh.v1.approval-receipt',
            canonical_receipt_blake3: '61'.repeat(32),
            canonical_statement_blake3: '62'.repeat(32),
            credential_id: '63'.repeat(32),
            review_bundle: workspace.review_items[0].bundle,
            shared_version: workspace.review_items[0].reviewed_head,
            user_verification: 'user-presence',
          },
          git_export: {
            status: 'exported',
            target: '/ordinary/review',
            branch: `mesh/approved/${workspace.review_items[0].reviewed_head}`,
            commit: '64'.repeat(20),
            approval_ref: `refs/mesh/approvals/${'61'.repeat(32)}`,
            already_present: false,
            message: null,
          },
        });
      }
      if (approvalAttempts === 4) {
        return JSON.stringify({
          workspace: {
            ...workspace,
            shared_version: workspace.review_items[0].reviewed_head,
          },
          receipt: {
            protocol: 'mesh.v1.approval-receipt',
            canonical_receipt_blake3: '61'.repeat(32),
            canonical_statement_blake3: '62'.repeat(32),
            credential_id: '63'.repeat(32),
            review_bundle: workspace.review_items[0].bundle,
            shared_version: workspace.review_items[0].reviewed_head,
            user_verification: 'user-presence',
          },
          git_export: { status: 'not-requested', target: null, branch: null, commit: null, approval_ref: null, already_present: false, message: null },
        });
      }
      workspace.shared_version = workspace.review_items[0].reviewed_head;
      workspace.digest = 'workspace-approved';
      return JSON.stringify({
        workspace,
        receipt: {
          protocol: 'mesh.v1.approval-receipt',
          canonical_receipt_blake3: '61'.repeat(32),
          canonical_statement_blake3: '62'.repeat(32),
          credential_id: '63'.repeat(32),
          review_bundle: workspace.review_items[0].bundle,
          shared_version: workspace.review_items[0].reviewed_head,
          user_verification: 'user-presence',
        },
        git_export: {
          status: 'exported',
          target: '/ordinary/review',
          branch: `mesh/approved/${workspace.review_items[0].reviewed_head}`,
          commit: '64'.repeat(20),
          approval_ref: `refs/mesh/approvals/${'61'.repeat(32)}`,
          already_present: false,
          message: null,
        },
      });
    }
    if (command === 'export_shared_review_to_git') {
      exportRetryParameters = parameters;
      throw new Error('Git export reply was lost after the branch was created');
    }
    if (command === 'inspect_shared_review_git_export') {
      exportInspectionParameters = parameters;
      return JSON.stringify({
        workspace,
        git_export: exportedBranchExists
          ? {
            status: 'exported',
            target: '/ordinary/review',
            branch: `mesh/approved/${workspace.review_items[0].reviewed_head}`,
            commit: '64'.repeat(20),
            approval_ref: `refs/mesh/approvals/${'61'.repeat(32)}`,
            already_present: true,
            message: null,
          }
          : {
            status: 'missing',
            target: '/ordinary/review',
            branch: `mesh/approved/${workspace.review_items[0].reviewed_head}`,
            commit: null,
            approval_ref: null,
            already_present: false,
            message: null,
          },
      });
    }
    if (command === 'open_review_artifact_inspection') {
      savedSideOpenParameters = parameters;
      await new Promise((resolve) => { releaseSavedSideOpen = resolve; });
      return JSON.stringify({
        schema: 'mesh.review-side-open/v1',
        side: parameters.side,
        action: parameters.action,
        version_id: parameters.expectedVersionId,
        content_digest: parameters.expectedContentDigest,
        opened: true,
        working_folder_unchanged: true,
      });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  const { validatedReviewTextDiff } = await import(`./app.js?review-card=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  let islandProjection = null;
  let overviewProjection = null;
  let chromeProjection = null;
  let entryProjection = null;
  let currentProjection = null;
  let workProjection = null;
  let destinationProjection = null;
  let mountIslandProjection = true;
  let mountOverviewProjection = true;
  let mountChromeProjection = true;
  let mountEntryProjection = true;
  let mountCurrentProjection = true;
  let mountWorkProjection = true;
  let mountDestinationProjection = true;
  document.addEventListener('mesh:review-workbench-projection', (event) => {
    islandProjection = event.detail;
    if (mountIslandProjection) {
      document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-mounted', {
        detail: {
          generation: event.detail.generation,
          bundle: event.detail.state === 'ready' ? event.detail.projection.bundle : null,
        },
      }));
    }
  });
  document.addEventListener('mesh:workspace-overview-projection', (event) => {
    overviewProjection = event.detail;
    if (mountOverviewProjection) {
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-mounted', {
        detail: { generation: event.detail.generation },
      }));
    }
  });
  document.addEventListener('mesh:workspace-chrome-projection', (event) => {
    chromeProjection = event.detail;
    if (mountChromeProjection) {
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-chrome-mounted', {
        detail: { generation: event.detail.generation },
      }));
    }
  });
  document.addEventListener('mesh:workspace-entry-projection', (event) => {
    entryProjection = event.detail;
    if (mountEntryProjection) {
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-mounted', {
        detail: { generation: event.detail.generation },
      }));
    }
  });
  document.addEventListener('mesh:workspace-current-projection', (event) => {
    currentProjection = event.detail;
    if (mountCurrentProjection) {
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-mounted', {
        detail: { generation: event.detail.generation },
      }));
    }
  });
  document.addEventListener('mesh:workspace-files-changes-projection', (event) => {
    workProjection = event.detail;
    if (mountWorkProjection) {
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-mounted', {
        detail: { generation: event.detail.generation },
      }));
    }
  });
  document.addEventListener('mesh:workspace-destination-projection', (event) => {
    destinationProjection = event.detail;
    if (mountDestinationProjection) {
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-mounted', {
        detail: { generation: event.detail.generation },
      }));
    }
  });
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-chrome-available'));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-available'));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-available'));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-available'));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-available'));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-available'));
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-available'));
  assert.equal(chromeProjection.chrome.serviceState, 'ready');
  assert.equal(chromeProjection.chrome.workspaceReady, true);
  assert.equal(document.getElementById('workspace-chrome-next').classList.contains('hidden'), false);
  assert.equal(entryProjection.entry.mode, 'ready');
  assert.equal(entryProjection.entry.chooseLabel, 'Import another folder');
  assert.equal(entryProjection.entry.recents[0].state, 'current');
  assert.equal(document.getElementById('workspace-entry-next').classList.contains('hidden'), false);
  assert.equal(document.getElementById('workspace-entry-current').classList.contains('hidden'), true);
  assert.equal(destinationProjection.destination.destination, '/ordinary/review');
  assert.equal(destinationProjection.destination.actions.length, 5);
  assert.equal(document.getElementById('workspace-destination-next').classList.contains('hidden'), false);
  const destinationGeneration = destinationProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-intent', {
    detail: {
      generation: destinationGeneration - 1,
      intent: { type: 'set-field', field: 'destination', value: '/ordinary/forged' },
    },
  }));
  assert.equal(destinationProjection.destination.destination, '/ordinary/review', 'a stale destination intent changed coordinator state');
  mountDestinationProjection = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-intent', {
    detail: {
      generation: destinationGeneration,
      intent: { type: 'set-field', field: 'destination', value: '/ordinary/private' },
    },
  }));
  const failedDestinationGeneration = destinationProjection.generation;
  assert.ok(failedDestinationGeneration > destinationGeneration);
  assert.equal(
    destinationProjection.destination.destination,
    '/ordinary/private',
    'a typed React destination did not become the coordinator-owned selected destination',
  );
  assert.equal(
    document.getElementById('workspace-destination-next').classList.contains('hidden'),
    false,
    'an exact destination field update flickered out the mounted React surface before commit',
  );
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-rejected', {
    detail: { generation: failedDestinationGeneration },
  }));
  assert.equal(document.getElementById('workspace-destination-next').classList.contains('hidden'), true);
  mountDestinationProjection = true;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-available'));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-intent', {
    detail: {
      generation: destinationProjection.generation,
      intent: { type: 'set-field', field: 'destination', value: '/ordinary/private-copy' },
    },
  }));
  assert.ok(destinationProjection.generation > failedDestinationGeneration);
  assert.equal(destinationProjection.destination.destination, '/ordinary/private-copy');
  assert.equal(document.getElementById('workspace-destination-next').classList.contains('hidden'), false);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-intent', {
    detail: {
      generation: destinationProjection.generation,
      intent: { type: 'set-field', field: 'selectedFile', value: 'notes.txt' },
    },
  }));
  const selectedFileGeneration = destinationProjection.generation;
  assert.equal(destinationProjection.destination.selectedFile, 'notes.txt');
  assert.equal(destinationProjection.destination.actions.find((action) => action.id === 'preview-single').enabled, true);
  mountDestinationProjection = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-intent', {
    detail: {
      generation: selectedFileGeneration,
      intent: { type: 'set-field', field: 'selectedFile', value: 'summary.txt' },
    },
  }));
  assert.equal(destinationProjection.destination.selectedFile, 'summary.txt');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-intent', {
    detail: {
      generation: selectedFileGeneration,
      intent: {
        type: 'activate', action: 'preview-single', selectedFile: 'notes.txt', destination: '/ordinary/private-copy',
      },
    },
  }));
  assert.equal(singleExportPreviewParameters.length, 0, 'a stale selected-file echo started a preview');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-intent', {
    detail: {
      generation: selectedFileGeneration,
      intent: {
        type: 'activate', action: 'preview-single', selectedFile: 'summary.txt', destination: '/ordinary/private-copy',
      },
    },
  }));
  await waitFor(() => singleExportPreviewParameters.length === 1);
  assert.equal(singleExportPreviewParameters[0].relativePath, 'summary.txt');
  assert.equal(singleExportPreviewParameters[0].targetRoot, '/ordinary/private-copy');
  const singleReadyGeneration = destinationProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-intent', {
    detail: {
      generation: singleReadyGeneration - 1,
      intent: { type: 'activate', action: 'confirm-single' },
    },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-intent', {
    detail: {
      generation: singleReadyGeneration,
      intent: { type: 'activate', action: 'confirm-single', destination: '/ordinary/forged' },
    },
  }));
  await Promise.resolve();
  assert.equal(exportedOriginal, false, 'a stale or forged single confirmation reached native export');
  mountDestinationProjection = true;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-mounted', {
    detail: { generation: destinationProjection.generation },
  }));
  const selectedPreviewGeneration = destinationProjection.generation;
  mountDestinationProjection = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-intent', {
    detail: {
      generation: selectedPreviewGeneration,
      intent: { type: 'set-field', field: 'destination', value: '/ordinary/private-copy-2' },
    },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-intent', {
    detail: {
      generation: selectedPreviewGeneration,
      intent: { type: 'activate', action: 'preview-all', destination: '/ordinary/private-copy' },
    },
  }));
  assert.equal(batchExportPreviewParameters.length, 0, 'a stale destination echo started a workspace preview');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-intent', {
    detail: {
      generation: selectedPreviewGeneration,
      intent: { type: 'activate', action: 'preview-all', destination: '/ordinary/private-copy-2' },
    },
  }));
  await waitFor(() => batchExportPreviewParameters.length === 1);
  assert.equal(batchExportPreviewParameters[0].targetRoot, '/ordinary/private-copy-2');
  mountDestinationProjection = true;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-mounted', {
    detail: { generation: destinationProjection.generation },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-intent', {
    detail: {
      generation: destinationProjection.generation,
      intent: { type: 'set-field', field: 'destination', value: '/ordinary/review' },
    },
  }));
  const initialEntryGeneration = entryProjection.generation;
  const selectedRecentPath = entryProjection.entry.recents.find(
    (entry) => entry.path !== entryProjection.entry.selectedRecentPath,
  ).path;
  const entryHost = document.getElementById('workspace-entry-next');
  const recentOwner = { id: 'workspace-entry-recent', isConnected: true };
  entryHost.shadowRoot = { activeElement: recentOwner };
  const entryClassToggle = entryHost.classList.toggle.bind(entryHost.classList);
  entryHost.classList.toggle = (value, force) => {
    const enabled = entryClassToggle(value, force);
    if (value === 'hidden' && enabled) entryHost.shadowRoot.activeElement = null;
    return enabled;
  };
  mountEntryProjection = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
    detail: {
      generation: initialEntryGeneration - 1,
      intent: { type: 'select-recent', path: selectedRecentPath },
    },
  }));
  assert.notEqual(document.getElementById('recent-workspace').value, selectedRecentPath);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
    detail: {
      generation: initialEntryGeneration,
      intent: { type: 'select-recent', path: selectedRecentPath },
    },
  }));
  assert.equal(document.getElementById('recent-workspace').value, selectedRecentPath);
  assert.ok(
    entryProjection.generation > initialEntryGeneration,
    'selecting a projected recent workspace did not publish its newly selected path',
  );
  assert.equal(entryProjection.entry.selectedRecentPath, selectedRecentPath);
  assert.equal(
    document.getElementById('workspace-entry-next').classList.contains('hidden'),
    false,
    'selecting a recent workspace hid the still-mounted entry surface before its replacement committed',
  );
  assert.equal(entryHost.shadowRoot.activeElement, recentOwner, 'selecting a recent workspace dropped keyboard focus');

  await document.emitReviewIntent({
    type: 'open-review-side',
    changeId: 'ee'.repeat(16),
    side: 'after',
    action: 'reveal-entry',
  });
  await waitFor(() => savedSideOpenParameters !== null);
  assert.equal(savedSideOpenParameters.expectedVersionId, '44'.repeat(32));
  assert.equal(savedSideOpenParameters.expectedContentDigest, '55'.repeat(32));
  const pendingOpenGeneration = entryProjection.generation;
  await document.emitWorkspaceEntryIntent(
    { type: 'open-recent', path: selectedRecentPath },
    pendingOpenGeneration,
  );
  assert.deepEqual(
    recentOpenPaths,
    [],
    'a workspace switch crossed the production coordinator while an exact native side launch was pending',
  );
  releaseSavedSideOpen();
  await waitFor(() => /Revealed the exact after saved copy/.test(document.getElementById('notice').textContent));

  for (const [generation, path] of [
    [initialEntryGeneration - 1, selectedRecentPath],
    [initialEntryGeneration, '/managed/forged'],
  ]) {
    document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
      detail: { generation, intent: { type: 'open-recent', path } },
    }));
    document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
      detail: { generation, intent: { type: 'forget-recent', path } },
    }));
  }
  assert.deepEqual(recentOpenPaths, [], 'a stale or mismatched recent Open crossed the coordinator boundary');
  assert.deepEqual(recentForgetPaths, [], 'a stale or mismatched recent Forget crossed the coordinator boundary');
  assert.deepEqual(recentOpenPaths, [], 'a stale or forged selection started a recent Open');
  assert.deepEqual(recentForgetPaths, [], 'Select then immediate Open replayed a different recent action');

  mountEntryProjection = true;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-mounted', {
    detail: { generation: entryProjection.generation },
  }));
  assert.equal(
    entryHost.shadowRoot.activeElement,
    recentOwner,
    'the committed controlled Recent selection replaced its focused select',
  );
  const closedEntryGeneration = entryProjection.generation;
  const disclosureOwner = { id: 'workspace-entry-summary', isConnected: true };
  entryHost.shadowRoot.activeElement = disclosureOwner;
  mountEntryProjection = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
    detail: {
      generation: closedEntryGeneration,
      intent: { type: 'set-disclosure', open: true },
    },
  }));
  assert.equal(
    document.getElementById('workspace-entry-controls').open,
    true,
    'opening the React entry disclosure was not retained by its coordinator',
  );
  assert.equal(entryProjection.entry.disclosureOpen, true);
  assert.equal(
    document.getElementById('workspace-entry-next').classList.contains('hidden'),
    false,
    'opening the entry disclosure hid the still-mounted surface before its replacement committed',
  );
  assert.equal(entryHost.shadowRoot.activeElement, disclosureOwner, 'opening the entry disclosure dropped keyboard focus');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
    detail: {
      generation: closedEntryGeneration,
      intent: { type: 'open-recent', path: selectedRecentPath },
    },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
    detail: {
      generation: closedEntryGeneration,
      intent: { type: 'forget-recent', path: selectedRecentPath },
    },
  }));
  assert.deepEqual(recentOpenPaths, [], 'disclosure echo widened grace to a recent Open action');
  assert.deepEqual(recentForgetPaths, [], 'disclosure echo widened grace to a recent Forget action');
  mountEntryProjection = true;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-mounted', {
    detail: { generation: entryProjection.generation },
  }));
  const entryGeneration = entryProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
    detail: {
      generation: entryGeneration - 1,
      intent: { type: 'update-managed-path', path: '/private/stale.mesh' },
    },
  }));
  assert.equal(entryProjection.entry.openPath, '');
  mountEntryProjection = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
    detail: {
      generation: entryGeneration,
      intent: { type: 'update-managed-path', path: '/p' },
    },
  }));
  assert.ok(entryProjection.generation > entryGeneration);
  assert.equal(
    document.getElementById('workspace-entry-next').classList.contains('hidden'),
    false,
    'an exact managed-path field echo hid the still-mounted entry surface before its replacement committed',
  );
  assert.equal(document.getElementById('workspace-entry-current').classList.contains('hidden'), true);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
    detail: {
      generation: entryGeneration,
      intent: { type: 'update-managed-path', path: '/private/next.mesh ' },
    },
  }));
  assert.equal(entryProjection.entry.openPath, '/private/next.mesh ');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
    detail: {
      generation: entryGeneration,
      intent: { type: 'open-managed-path', path: '/private/forged.mesh' },
    },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
    detail: {
      generation: entryGeneration - 1,
      intent: { type: 'open-managed-path', path: '/private/next.mesh ' },
    },
  }));
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(
    managedPathOpenAttempts,
    [],
    'a mismatched or stale path submission crossed the coordinator boundary',
  );
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
    detail: {
      generation: entryGeneration,
      intent: { type: 'set-disclosure', open: false },
    },
  }));
  assert.equal(
    document.getElementById('workspace-entry-controls').open,
    true,
    'field-echo grace admitted a non-field action from a superseded entry generation',
  );
  assert.equal(
    entryProjection.entry.disclosureOpen,
    true,
    'typing a managed path collapsed the React entry disclosure after its first character',
  );
  assert.ok(entryProjection.generation > entryGeneration);
  mountEntryProjection = true;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-mounted', {
    detail: { generation: entryProjection.generation },
  }));
  const refreshedEntryGeneration = entryProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-intent', {
    detail: {
      generation: refreshedEntryGeneration,
      intent: { type: 'select-recent', path: '/private/not-projected.mesh' },
    },
  }));
  assert.notEqual(document.getElementById('recent-workspace').value, '/private/not-projected.mesh');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-chrome-rejected', {
    detail: { generation: chromeProjection.generation },
  }));
  assert.equal(document.getElementById('workspace-chrome-next').classList.contains('hidden'), true);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-rejected', {
    detail: { generation: entryProjection.generation },
  }));
  assert.equal(document.getElementById('workspace-entry-next').classList.contains('hidden'), true);
  assert.equal(document.getElementById('workspace-entry-current').classList.contains('hidden'), false);
  assert.equal(currentProjection.current.state, 'Ready for review');
  assert.equal(currentProjection.current.agentAssigned, false);
  assert.equal(currentProjection.current.destination, '/ordinary/review');
  const sourceOwnedCurrent = JSON.parse(JSON.stringify(currentProjection.current));
  assert.equal(Object.isFrozen(currentProjection.current), true);
  assert.equal(Object.isFrozen(currentProjection.current.actions), true);
  assert.throws(() => {
    currentProjection.current.privateVersionTitle = 'Forged private title';
  }, TypeError);
  assert.throws(() => {
    currentProjection.current.actions[0].enabled = !currentProjection.current.actions[0].enabled;
  }, TypeError);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-available'));
  assert.deepEqual(
    currentProjection.current,
    sourceOwnedCurrent,
    'mutating the retained hidden Current controller changed the React projection',
  );
  assert.equal(
    overviewProjection.overview.state,
    sourceOwnedCurrent.state,
    'Overview did not consume the same coordinator-owned Current state',
  );
  assert.equal(
    currentProjection.current.actions.find((action) => action.id === 'copy-diagnostics').enabled,
    sourceOwnedCurrent.actions.find((action) => action.id === 'copy-diagnostics').enabled,
  );
  assert.equal(
    currentProjection.current.actions.find((action) => action.id === 'rollback').enabled,
    sourceOwnedCurrent.actions.find((action) => action.id === 'rollback').enabled,
  );
  // Earlier fallback assertions deliberately rejected Chrome and Entry. Remount the healthy shell
  // before exercising a normal same-workspace refresh so this regression observes continuity,
  // not recovery from an already failed island.
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-chrome-available'));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-entry-available'));
  assert.equal(document.getElementById('workspace-current-next').classList.contains('hidden'), false);
  const committedCurrentGeneration = currentProjection.generation;
  const committedOverviewGeneration = overviewProjection.generation;
  const committedChromeGeneration = chromeProjection.generation;
  const committedEntryGeneration = entryProjection.generation;
  const committedWorkGeneration = workProjection.generation;
  mountOverviewProjection = false;
  mountChromeProjection = false;
  mountEntryProjection = false;
  mountCurrentProjection = false;
  mountWorkProjection = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-available'));
  assert.ok(currentProjection.generation > committedCurrentGeneration);
  assert.ok(overviewProjection.generation > committedOverviewGeneration);
  assert.ok(chromeProjection.generation > committedChromeGeneration);
  assert.ok(entryProjection.generation > committedEntryGeneration);
  assert.ok(workProjection.generation > committedWorkGeneration);
  assert.equal(
    document.getElementById('workspace-current-next').classList.contains('hidden'),
    false,
    'a healthy same-workspace Current refresh visibly swapped out the committed React surface',
  );
  for (const [reactId, legacyId] of [
    ['workspace-entry-next', 'workspace-entry-current'],
  ]) {
    assert.equal(
      document.getElementById(reactId).classList.contains('hidden'),
      false,
      `${reactId} flickered out while its same-workspace replacement was pending`,
    );
    assert.equal(
      document.getElementById(legacyId).classList.contains('hidden'),
      true,
      `${legacyId} flashed while a healthy same-workspace React surface was already mounted`,
    );
  }
  assert.equal(
    document.getElementById('workspace-changes-next').classList.contains('hidden'),
    false,
    'workspace-changes-next flickered out while its same-workspace replacement was pending',
  );
  assert.equal(
    document.getElementById('workspace-overview-next').classList.contains('hidden'),
    false,
    'workspace-overview-next flickered out while its same-workspace replacement was pending',
  );
  document.getElementById('review-card').scrolledIntoView = false;
  document.getElementById('open-current-review').focused = false;
  const noticeBeforeStaleRecommendation = document.getElementById('notice').textContent;
  const batchPreviewsBeforeStaleRecommendation = batchExportPreviewParameters.length;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-intent', {
    detail: {
      generation: committedOverviewGeneration,
      intent: { type: 'open-review' },
    },
  }));
  assert.equal(
    document.getElementById('review-workbench-next').scrolledIntoView,
    false,
    'an older Overview generation navigated while its exact replacement awaited commit',
  );
  assert.equal(overviewProjection.overview.nextActionTitle, 'Create the native working folder');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-intent', {
    detail: {
      generation: committedOverviewGeneration,
      intent: { type: 'recommended' },
    },
  }));
  assert.equal(batchExportPreviewParameters.length, batchPreviewsBeforeStaleRecommendation);
  assert.equal(document.getElementById('notice').textContent, noticeBeforeStaleRecommendation);
  document.getElementById('workspace-versions-next').scrolledIntoView = false;
  document.workspaceVersionChoice('bb'.repeat(32)).focused = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-intent', {
    detail: {
      generation: committedCurrentGeneration,
      intent: { type: 'activate', action: 'open-version' },
    },
  }));
  await Promise.resolve();
  assert.equal(
    document.getElementById('workspace-versions-next').scrolledIntoView,
    true,
    'visible React Current navigation was dropped while its same-workspace replacement awaited commit',
  );
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-rejected', {
    detail: { generation: currentProjection.generation },
  }));
  assert.equal(
    document.getElementById('workspace-current-next').classList.contains('hidden'),
    true,
    'a failed same-workspace replacement left the prior React surface visible',
  );
  mountOverviewProjection = true;
  mountChromeProjection = true;
  mountEntryProjection = true;
  mountCurrentProjection = true;
  mountWorkProjection = true;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-available'));
  assert.equal(document.getElementById('workspace-current-next').classList.contains('hidden'), false);
  document.getElementById('workspace-versions-next').scrolledIntoView = false;
  document.workspaceVersionChoice('bb'.repeat(32)).focused = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-intent', {
    detail: {
      generation: currentProjection.generation - 1,
      intent: { type: 'activate', action: 'open-version' },
    },
  }));
  await Promise.resolve();
  assert.equal(document.getElementById('workspace-versions-next').scrolledIntoView, false, 'a stale detailed workspace intent crossed the coordinator boundary');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-intent', {
    detail: {
      generation: currentProjection.generation,
      intent: { type: 'activate', action: 'open-version' },
    },
  }));
  await waitFor(() => document.workspaceVersionChoice('bb'.repeat(32)).focused === true);
  assert.equal(document.getElementById('workspace-versions-next').scrolledIntoView, true);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-intent', {
    detail: {
      generation: currentProjection.generation,
      intent: { type: 'activate', action: 'rollback', force: true },
    },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-rejected', {
    detail: { generation: currentProjection.generation },
  }));
  assert.equal(document.getElementById('workspace-current-next').classList.contains('hidden'), true);
  assert.equal(workProjection.workbench.changes.files[0].value, 'notes.txt');
  assert.equal(workProjection.workbench.changes.editorKind, 'none');
  assert.equal(document.getElementById('workspace-files-next').classList.contains('hidden'), false);
  assert.equal(document.getElementById('workspace-changes-next').classList.contains('hidden'), false);
  const staleWorkGeneration = workProjection.generation;
  mountWorkProjection = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-intent', {
    detail: {
      generation: staleWorkGeneration,
      intent: { type: 'set-field', field: 'selectedFile', value: 'notes.txt' },
    },
  }));
  assert.ok(workProjection.generation > staleWorkGeneration);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-intent', {
    detail: {
      generation: staleWorkGeneration,
      intent: { type: 'activate', action: 'load-file', field: 'selectedFile', value: 'notes.txt' },
    },
  }));
  await waitFor(() => reviewScopeInspections === 1);
  assert.equal(workProjection.workbench.changes.editorKind, 'text', 'choosing a file then immediately opening it was dropped before React committed the selection');
  mountWorkProjection = true;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-mounted', {
    detail: { generation: workProjection.generation },
  }));
  const fieldWorkGeneration = workProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-intent', {
    detail: {
      generation: fieldWorkGeneration - 1,
      intent: { type: 'set-field', field: 'newPath', value: 'forged.txt' },
    },
  }));
  assert.equal(document.getElementById('manage-path').value, '', 'a stale files intent changed coordinator state');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-intent', {
    detail: {
      generation: fieldWorkGeneration,
      intent: { type: 'set-field', field: 'newPath', value: 'alpha.txt' },
    },
  }));
  assert.equal(document.getElementById('manage-path').value, 'alpha.txt');
  assert.equal(workProjection.workbench.files.newPath, 'alpha.txt');
  assert.equal(overviewProjection.overview.workspaceName, 'review · Managed workspace');
  assert.equal(overviewProjection.overview.state, 'Ready for review');
  assert.equal(overviewProjection.overview.nextActionTitle, 'Create the native working folder');
  assert.equal(overviewProjection.overview.canOpenReview, true);
  assert.equal(overviewProjection.overview.canOpenAnotherVersion, false);
  assert.equal(document.getElementById('workspace-overview-next').classList.contains('hidden'), false);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-intent', {
    detail: {
      generation: overviewProjection.generation,
      intent: { type: 'open-review' },
    },
  }));
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(document.getElementById('review-workbench-next').scrolledIntoView, true);
  assert.equal(document.getElementById('review-workbench-next').focused, true);
  document.getElementById('review-workbench-next').scrolledIntoView = false;
  document.getElementById('review-workbench-next').focused = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-intent', {
    detail: {
      generation: overviewProjection.generation,
      intent: { type: 'open-review', bypass: true },
    },
  }));
  assert.equal(document.getElementById('review-workbench-next').scrolledIntoView, false, 'an extended overview intent crossed the closed coordinator boundary');
  assert.equal(document.getElementById('review-workbench-next').focused, false);
  assert.deepEqual(islandProjection.projection, workspace.review_items[0]);
  assert.deepEqual(islandProjection.authority, {
    canRenderArtifactPreview: false,
    canInspectExactCopies: false,
    canRecordReview: false,
    canApprove: true,
    canApproveAndExport: true,
    canExportGit: false,
    canExportPrivateCopy: true,
    approvalReason: 'This exact reviewed version is ready for native approval with macOS user presence.',
  });
  assert.equal(document.getElementById('review-workbench-next').classList.contains('hidden'), false);
  const committedReviewGeneration = islandProjection.generation;
  mountIslandProjection = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-available'));
  assert.ok(islandProjection.generation > committedReviewGeneration);
  assert.equal(
    document.getElementById('review-workbench-next').classList.contains('hidden'),
    false,
    'a same-workspace review refresh flickered out the committed React review before replacement',
  );
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-rejected', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
    },
  }));
  assert.equal(document.getElementById('review-workbench-next').classList.contains('hidden'), true);
  mountIslandProjection = true;
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-available'));
  assert.equal(document.getElementById('review-workbench-next').classList.contains('hidden'), false);
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'approve-version', authority: true },
    },
  }));
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(approvalAttempts, 0, 'an extended approval intent crossed the closed coordinator boundary');
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation - 1,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'approve-version' },
    },
  }));
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(approvalAttempts, 0, 'a stale workbench generation dispatched approval');
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'approve-version' },
    },
  }));
  workspace.native_untracked_files = ['arrived-after-click.txt'];
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-available'));
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(approvalAttempts, 0, 'a superseded review action ran after its exact generation was replaced');
  workspace.native_untracked_files = [];
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-available'));

  assert.deepEqual(validatedReviewTextDiff(workspace.review_items[0].bundle_changes[0]), {
    error: null,
    kind: 'diff',
    hunks: workspace.review_items[0].bundle_changes[0].verified_text.hunks,
  });

  const substitutedDiff = structuredClone(workspace.review_items[0].bundle_changes[0]);
  substitutedDiff.verified_text.after.version_id = '66'.repeat(32);
  assert.deepEqual(validatedReviewTextDiff(substitutedDiff), {
    error: 'Content identity mismatch',
    kind: 'unavailable',
    hunks: [],
  });
  const deceptiveDiff = structuredClone(workspace.review_items[0].bundle_changes[0]);
  deceptiveDiff.verified_text.hunks[0].lines[1].text = '\u202ereversed';
  assert.deepEqual(validatedReviewTextDiff(deceptiveDiff), {
    error: 'Invalid bounded diff',
    kind: 'unavailable',
    hunks: [],
  });

  assert.equal(islandProjection.controls.countLabel, '1 recorded review');
  assert.equal(document.workspaceCurrentAction('update-destination')?.enabled, false);
  assert.equal(document.destinationActionControl('preview-all').disabled, true);
  assert.match(document.destinationHint.textContent, /Review and approve/);
  await document.emitWorkspaceCurrentIntent('update-destination');
  assert.match(document.getElementById('notice').textContent, /review action is no longer available/i);
  assert.equal(islandProjection.projection.subject_operation, workspace.review_items[0].subject_operation);
  assert.equal(islandProjection.projection.bundle_changes[0].path_after, '/notes.txt');
  assert.equal(islandProjection.authority.canApprove, true);
  assert.equal(islandProjection.authority.canApproveAndExport, true);
  assert.equal(
    (document.workspaceCurrent?.current?.state ?? ''),
    'Ready for review',
    'a durable current review was hidden behind the earlier private-save status',
  );

  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-intent', {
    detail: {
      generation: workProjection.generation,
      intent: { type: 'set-field', field: 'selectedFile', value: 'notes.txt' },
    },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-intent', {
    detail: {
      generation: workProjection.generation,
      intent: { type: 'activate', action: 'load-file' },
    },
  }));
  await waitFor(() => workProjection.workbench.changes.editorKind === 'text');
  assert.equal(workProjection.workbench.changes.baselineText, 'saved text\n');
  const loadedGeneration = workProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-intent', {
    detail: {
      generation: loadedGeneration,
      intent: { type: 'set-field', field: 'editorText', value: 'draft begins' },
    },
  }));
  const firstDraftGeneration = workProjection.generation;
  assert.ok(firstDraftGeneration > loadedGeneration);
  assert.equal(workProjection.workbench.actions.find((action) => action.id === 'preserve-edit').enabled, true);
  mountWorkProjection = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-intent', {
    detail: {
      generation: firstDraftGeneration,
      intent: { type: 'set-field', field: 'editorText', value: 'draft that exists only in this window\n' },
    },
  }));
  assert.equal(
    document.getElementById('workspace-changes-next').classList.contains('hidden'),
    false,
    'an exact editor field echo hid the still-mounted workbench before its replacement committed',
  );
  assert.equal(document.getElementById('file-editor').value, 'draft that exists only in this window\n');
  assert.equal(workProjection.workbench.changes.editorText, 'draft that exists only in this window\n');
  assert.equal(workProjection.workbench.actions.find((action) => action.id === 'preserve-edit').enabled, true);
  assert.equal(workProjection.workbench.files.canEditNewPath, false, 'the React draft did not retain the legacy draft-loss guard');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-mounted', {
    detail: { generation: workProjection.generation },
  }));
  const blockedFilesGeneration = workProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-intent', {
    detail: {
      generation: blockedFilesGeneration,
      intent: { type: 'set-field', field: 'newPath', value: 'must-not-survive.txt' },
    },
  }));
  assert.equal(
    workProjection.workbench.files.newPath,
    'alpha.txt',
    'a forged field intent changed the source-owned Files draft while file management was disabled',
  );
  mountWorkProjection = true;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-intent', {
    detail: {
      generation: loadedGeneration,
      intent: { type: 'set-field', field: 'editorText', value: 'stale overwrite\n' },
    },
  }));
  assert.equal(document.getElementById('file-editor').value, 'draft that exists only in this window\n', 'a stale editor intent replaced the current draft');
  assert.equal(reviewScopeInspections, 2);
  assert.equal(islandProjection.authority.canApprove, false);
  assert.match(islandProjection.authority.approvalReason, /edit exists only in this window/);
  assert.match(islandProjection.authority.approvalReason, /not included in this review/);
  await document.emitReviewIntent({ type: 'approve-version' });
  assert.equal(approvalAttempts, 0, 'an editor-only draft reached native approval');
  assert.equal(reviewScopeInspections, 2, 'a known editor draft started a redundant folder scan');

  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-intent', {
    detail: {
      generation: workProjection.generation,
      intent: { type: 'set-field', field: 'editorText', value: 'saved text\n' },
    },
  }));
  assert.equal(workProjection.workbench.actions.find((action) => action.id === 'preserve-edit').enabled, false);
  assert.equal(islandProjection.authority.canApprove, true);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-rejected', {
    detail: { generation: workProjection.generation },
  }));
  assert.equal(document.getElementById('workspace-files-next').classList.contains('hidden'), true);
  assert.equal(document.getElementById('workspace-changes-next').classList.contains('hidden'), true);

  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'approve-version' },
    },
  }));
  await waitFor(() => approvalAttempts === 1);
  assert.equal(workspace.shared_version, null);
  assert.match(document.getElementById('notice').textContent, /complete receipt for the exact version/);
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-rejected', {
    detail: { generation: islandProjection.generation, reason: 'planted malformed projection' },
  }));
  assert.equal(document.getElementById('review-workbench-next').classList.contains('hidden'), true);
  await document.emitWorkspaceCurrentIntent('refresh');
  await document.emitReviewIntent({ type: 'approve-and-export' });
  await waitFor(() => approvalAttempts === 2);
  assert.equal(workspace.shared_version, null);
  assert.match(document.getElementById('notice').textContent, /complete receipt for the exact version/);
  for (const mutation of ['substituted workspace', 'missing requested Git export']) {
    await document.emitWorkspaceCurrentIntent('refresh');
    await document.emitReviewIntent({ type: 'approve-and-export' });
    await waitFor(() => approvalAttempts >= 3);
    assert.equal(workspace.shared_version, null, `${mutation} result changed shared state`);
    assert.match(document.getElementById('notice').textContent, /complete receipt for the exact version/);
  }
  await document.emitWorkspaceCurrentIntent('refresh');
  await document.emitReviewIntent({ type: 'approve-and-export' });
  await waitFor(() => workspace.shared_version !== null);
  assert.deepEqual(approveParameters, {
    bundle: workspace.review_items[0].bundle,
    target: workspace.review_items[0].subject_operation,
    expectedWorkspaceRoot: '/managed/review',
    expectedWorkspaceDigest: 'workspace-review',
    expectedWorkspaceInstallation: 'installation-review',
    exportToGit: true,
  });
  assert.match((document.workspaceCurrent?.current?.sharedVersion ?? ''), /^Approved point 1 · abababababab…$/);
  assert.equal(
    (document.workspaceCurrent?.current?.sharedVersionTitle ?? ''),
    `Exact shared version: ${workspace.review_items[0].reviewed_head}`,
  );
  assert.match(document.getElementById('notice').textContent, /created Git review branch/);
  assert.match(document.getElementById('notice').textContent, /Original working files were not changed/);
  assert.equal(
    (document.workspaceCurrent?.current?.state ?? ''),
    'Approved',
    'a local approval was mislabeled as peer replication',
  );
  if (document.getElementById('review-workbench-next').classList.contains('hidden')) {
    await islandToggle.emit('click');
  }
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'choose-private-export' },
    },
  }));
  await waitFor(() => privateExportPickerCalls === 1);
  assert.equal(document.destinationField('destination').value, '');
  assert.equal(document.destinationActionControl('preview-all').disabled, true);
  assert.match(document.getElementById('notice').textContent, /original project folder.*different ordinary folder/i);
  privateExportPickerResult = '/ordinary/review-private-copy';
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'choose-private-export' },
    },
  }));
  await waitFor(() => privateExportPickerCalls === 2);
  assert.equal(document.destinationField('destination').value, '/ordinary/review-private-copy');
  assert.equal(document.destinationActionControl('preview-all').disabled, false);
  document.getElementById('edit-file').value = 'notes.txt';
  await document.getElementById('edit-file').emit('change');
  await document.getElementById('load-file').emit('click');
  document.getElementById('file-editor').value = 'draft typed after approval\n';
  await document.getElementById('file-editor').emit('input');
  assert.equal(
    (document.workspaceCurrent?.current?.state ?? ''),
    'Working',
    'an editor-only draft retained the current saved point\'s Approved label',
  );
  document.getElementById('file-editor').value = 'saved text\n';
  await document.getElementById('file-editor').emit('input');
  assert.equal(
    (document.workspaceCurrent?.current?.state ?? ''),
    'Approved',
    'discarding the editor-only draft did not restore the durable current state',
  );
  assert.equal(document.workspaceCurrentAction('update-destination')?.enabled, true);
  await document.emitWorkspaceCurrentIntent('update-destination');
  assert.equal(document.destinationField('destination').value, '/ordinary/review');
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Update the original folder');
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Preview original update');
  assert.match(document.workspaceOverview?.overview.nextActionDescription, /did not change your original folder/);
  assert.equal(document.destinationActionControl('preview-all').disabled, false);
  await document.emitWorkspaceOverviewIntent('recommended');
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Update the original folder');
  assert.equal(document.destinationActionControl('confirm-batch').textContent, 'Update 1 changed file');
  assert.equal(exportedOriginal, false);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-intent', {
    detail: {
      generation: destinationProjection.generation - 1,
      intent: { type: 'activate', action: 'confirm-batch' },
    },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-intent', {
    detail: {
      generation: destinationProjection.generation,
      intent: { type: 'activate', action: 'confirm-batch', destination: '/ordinary/forged' },
    },
  }));
  await Promise.resolve();
  assert.equal(exportedOriginal, false, 'a stale or forged batch confirmation reached native export');
  await document.destinationActionControl('confirm-batch').emit('click');
  assert.equal(exportedOriginal, true);
  assert.equal(completedOriginalVersion, workspace.shared_version);
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Create the native working folder');
  assert.match(document.getElementById('notice').textContent, /Saved changes, moves, and deletions are applied/);
  assert.equal(islandProjection.authority.canExportGit, true);
  await document.emitReviewIntent({ type: 'export-git' });
  await waitFor(() => exportRetryParameters !== null);
  assert.deepEqual(exportRetryParameters, {
    bundle: workspace.review_items[0].bundle,
    target: workspace.review_items[0].subject_operation,
    expectedWorkspaceRoot: '/managed/review',
    expectedWorkspaceDigest: 'workspace-approved',
    expectedWorkspaceInstallation: 'installation-review',
  });
  assert.deepEqual(exportInspectionParameters, exportRetryParameters);
  assert.match(document.getElementById('notice').textContent, /lost the Git export reply/);
  assert.match(document.getElementById('notice').textContent, /verified the exact existing branch/);
  assert.match(document.getElementById('notice').textContent, /Original working files were not changed/);
  exportedBranchExists = false;
  await document.emitReviewIntent({ type: 'export-git' });
  await waitFor(() => /confirmed that no Git review branch was created/.test(document.getElementById('notice').textContent));
  assert.match(document.getElementById('notice').textContent, /confirmed that no Git review branch was created/);
  assert.match(document.getElementById('notice').textContent, /Choose Create Git branch to try again/);
  assert.equal(reviewScopeInspections, 19, 'each approval, destination switch, export attempt, and post-approval editor load did not freshly inspect the tracked files');
});

test('empty and unavailable review projections cannot retain ready-only native actions', async () => {
  const document = fakeDocument();
  const operation = '91'.repeat(32);
  const objectId = '92'.repeat(16);
  const review = recordedCurrentReview(operation, '93'.repeat(32));
  const artifactChange = {
    object_id: objectId,
    path_before: 'finance/board-pack.pdf',
    path_after: 'finance/board-pack.pdf',
    effect: 'content-written',
    before: {
      kind: 'binary',
      version_id: '94'.repeat(32),
      content_digest: '95'.repeat(32),
      byte_length: '101',
      line_count: null,
    },
    after: {
      kind: 'binary',
      version_id: '96'.repeat(32),
      content_digest: '97'.repeat(32),
      byte_length: '111',
      line_count: null,
    },
    body: 'binary',
    opaque_reason: null,
    verified_text: null,
  };
  review.bundle_changes = [artifactChange];
  review.bundle_changes_not_listed = 0;
  review.content_complete = true;
  const workspace = {
    root: '/managed/status-review/mounts',
    digest: 'status-review-digest',
    installation: 'status-review-installation',
    records: 2,
    reviews: 1,
    review_items: [review],
    review_items_not_listed: 0,
    private_version: { version: 'status-review-private', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'finance/board-pack.pdf', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
    file_histories: [],
  };
  const forbiddenCalls = [];
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      auto_opened: false,
      remembered: workspace.root,
      workspaces: [workspace.root],
      workspace_entries: [{ path: workspace.root, project_root: '/ordinary/status-review' }],
    });
    if (command === 'approval_credential_status') {
      return JSON.stringify({ enrolled: true, available: true, unavailable_reason: null });
    }
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'native_capture_preference') return JSON.stringify({ enabled: false });
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    forbiddenCalls.push(command);
    throw new Error(`forged status action reached ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?review-status-authority=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  let projection = null;
  document.addEventListener('mesh:review-workbench-projection', (event) => {
    projection = event.detail;
    document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-mounted', {
      detail: {
        generation: event.detail.generation,
        bundle: event.detail.state === 'ready' ? event.detail.projection.bundle : null,
      },
    }));
  });
  const render = async (expectedState) => {
    await document.emitWorkspaceCurrentIntent('refresh');
    assert.equal(projection.state, expectedState);
    assert.equal(projection.state === 'ready' ? projection.projection.bundle : null, expectedState === 'ready' ? review.bundle : null);
  };
  const forge = async (intent) => {
    document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
      detail: { generation: projection.generation, bundle: null, intent },
    }));
    await Promise.resolve();
    await Promise.resolve();
    await new Promise((resolve) => setTimeout(resolve, 0));
  };
  const forgeReadyOnlyActions = async () => {
    await forge({ type: 'approve-version' });
    await forge({ type: 'approve-and-export' });
    await forge({ type: 'export-git' });
    await forge({ type: 'choose-private-export' });
    await forge({ type: 'load-artifact-preview', changeId: objectId, pageNumber: 1 });
    await forge({ type: 'inspect-exact-copies', changeId: objectId });
  };

  review.bundle_changes = [];
  await render('empty');
  await forgeReadyOnlyActions();

  review.bundle_changes = [artifactChange];
  review.content_complete = false;
  await render('unavailable');
  await forgeReadyOnlyActions();

  review.content_complete = true;
  review.bundle_changes_not_listed = 1;
  await render('unavailable');
  await forgeReadyOnlyActions();

  workspace.shared_version = review.reviewed_head;
  await render('unavailable');
  await forgeReadyOnlyActions();

  assert.deepEqual(forbiddenCalls, [], 'a status projection retained an approval, export, or artifact action');
});

test('office and PDF review cards load exact side-by-side visual previews on demand', async () => {
  const document = fakeDocument();
  const operation = '71'.repeat(32);
  const beforeVersion = '72'.repeat(32);
  const afterVersion = '73'.repeat(32);
  const beforeDigest = '74'.repeat(32);
  const afterDigest = '75'.repeat(32);
  const objectId = '76'.repeat(16);
  const review = recordedCurrentReview(operation, '77'.repeat(32));
  review.bundle_changes = [{
    object_id: objectId,
    path_before: 'finance/board-pack.pdf',
    path_after: 'finance/board-pack.pdf',
    effect: 'content-written',
    before: { kind: 'binary', version_id: beforeVersion, content_digest: beforeDigest, byte_length: '101', line_count: null },
    after: { kind: 'binary', version_id: afterVersion, content_digest: afterDigest, byte_length: '111', line_count: null },
    body: 'binary',
    opaque_reason: null,
    verified_text: null,
  }];
  const workspace = {
    root: '/managed/artifact-review/mounts',
    digest: 'artifact-workspace-digest',
    installation: 'artifact-workspace-installation',
    records: 2,
    reviews: 1,
    review_items: [review],
    review_items_not_listed: 0,
    private_version: { version: 'artifact-private-version', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'finance/board-pack.pdf', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    workspace_versions: [{ operation, ordinal: 1, actor_sequence: '1' }],
    file_histories: [],
  };
  const renderedSides = [];
  let failedSide = 'before';
  let heldPage = null;
  let pageStarted = null;
  let releasePage = null;
  let activeArtifactKind = 'pdf';
  let exportedInspection = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      auto_opened: false,
      remembered: workspace.root,
      workspaces: [workspace.root],
      workspace_entries: [{ path: workspace.root }],
    });
    if (command === 'approval_credential_status') return JSON.stringify({ enrolled: false, available: true, unavailable_reason: null });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'pick_folder') return '/review-output';
    if (command === 'export_review_artifact_inspection') {
      exportedInspection = parameters;
      const beforeExtension = activeArtifactKind === 'image' ? 'png' : 'pdf';
      const afterExtension = activeArtifactKind === 'image' ? 'jpg' : 'pdf';
      return JSON.stringify({
        schema: 'mesh-review-inspection-export/v1',
        directory: '/review-output/Mesh review 777777777777',
        files: [
          {
            side: 'before',
            path: `/review-output/Mesh review 777777777777/Before.${beforeExtension}`,
            version_id: beforeVersion,
            content_digest: beforeDigest,
          },
          {
            side: 'after',
            path: `/review-output/Mesh review 777777777777/After.${afterExtension}`,
            version_id: afterVersion,
            content_digest: afterDigest,
          },
        ],
        working_folder_unchanged: true,
        document_content_opened: false,
        finder_opened: true,
        warning: null,
      });
    }
    if (command === 'render_review_artifact') {
      renderedSides.push(parameters.side);
      if (activeArtifactKind === 'image') {
        assert.equal(Object.hasOwn(parameters, 'pageNumber'), false);
        const before = parameters.side === 'before';
        return JSON.stringify({
          renderer: 'macos-imageio-thumbnail-v1',
          scope: 'representative-preview',
          kind: 'image',
          side: parameters.side,
          version_id: before ? beforeVersion : afterVersion,
          content_digest: before ? beforeDigest : afterDigest,
          image_data_url: `data:image/png;base64,${before ? 'YmVmb3Jl' : 'YWZ0ZXI='}`,
          text_source: null,
          text_lines: null,
          text_sections: null,
          text_truncated: false,
          page_number: null,
          page_count: null,
          rendering_authorizes_approval: false,
        });
      }
      if (activeArtifactKind === 'presentation') {
        assert.equal(Object.hasOwn(parameters, 'pageNumber'), false);
        const before = parameters.side === 'before';
        return JSON.stringify({
          renderer: 'macos-quick-look-thumbnail',
          scope: 'representative-preview',
          kind: 'presentation',
          side: parameters.side,
          version_id: before ? beforeVersion : afterVersion,
          content_digest: before ? beforeDigest : afterDigest,
          image_data_url: `data:image/png;base64,${before ? 'YmVmb3Jl' : 'YWZ0ZXI='}`,
          text_source: 'mesh-pptx-slide-text-v1',
          text_lines: before
            ? ['Revenue $100,000', 'Hiring plan pending']
            : ['Revenue $120,000', 'Hiring plan approved', 'Risk owner Finance'],
          text_sections: before
            ? [
              { label: 'Slide 1 · Board summary', line_start: 0, line_count: 1 },
              { label: 'Slide 2 · People plan', line_start: 1, line_count: 1 },
            ]
            : [
              { label: 'Slide 1 · Board summary', line_start: 0, line_count: 1 },
              { label: 'Slide 2 · People plan', line_start: 1, line_count: 1 },
              { label: 'Slide 3 · Risk register', line_start: 2, line_count: 1 },
            ],
          text_truncated: false,
          page_number: null,
          page_count: null,
          rendering_authorizes_approval: false,
        });
      }
      assert.ok(Number.isSafeInteger(parameters.pageNumber) && parameters.pageNumber >= 1);
      assert.deepEqual(parameters, {
        expectedWorkspaceRoot: workspace.root,
        expectedWorkspaceDigest: workspace.digest,
        expectedWorkspaceInstallation: workspace.installation,
        bundle: review.bundle,
        target: review.subject_operation,
        objectId,
        side: parameters.side,
        pageNumber: parameters.pageNumber,
      });
      if (parameters.pageNumber === heldPage) {
        pageStarted?.();
        await new Promise((resolve) => { releasePage = resolve; });
      }
      if (parameters.side === failedSide) throw new Error('Quick Look refused this exact side');
      const before = parameters.side === 'before';
      return JSON.stringify({
        renderer: 'macos-pdfkit-page-v1',
        scope: 'exact-page-preview',
        kind: 'pdf',
        side: parameters.side,
        version_id: before ? beforeVersion : afterVersion,
        content_digest: before ? beforeDigest : afterDigest,
        image_data_url: `data:image/png;base64,${before ? 'YmVmb3Jl' : 'YWZ0ZXI='}`,
        text_source: 'macos-pdfkit-page-text-v1',
        text_lines: parameters.pageNumber === 1
          ? before
            ? ['Revenue $100,000', 'Open roles 4']
            : ['Revenue $120,000', 'Open roles 5']
          : parameters.pageNumber === 2
            ? before ? ['Hiring plan pending'] : ['Hiring plan approved']
            : ['Risk owner Finance'],
        text_sections: [{
          label: `Page ${parameters.pageNumber}`,
          line_start: 0,
          line_count: parameters.pageNumber === 1 ? 2 : 1,
        }],
        text_truncated: false,
        page_number: parameters.pageNumber,
        page_count: before ? 2 : 3,
        rendering_authorizes_approval: false,
      });
    }
    throw new Error(`unexpected artifact-review command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  const {
    artifactTextChangeSummary,
    artifactTextDiff,
    artifactTextExtractionNote,
    artifactTextSectionChoices,
    artifactTextSourcesMatch,
    reviewArtifactKind,
    validatedReviewArtifactPreview,
    validatedReviewInspectionExport,
  } = await import(`./app.js?artifact-review=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  let islandProjection = null;
  let islandArtifactPreview = null;
  let mountIslandProjection = true;
  document.addEventListener('mesh:review-workbench-projection', (event) => {
    islandProjection = event.detail;
    if (mountIslandProjection) {
      document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-mounted', {
        detail: {
          generation: event.detail.generation,
          bundle: event.detail.state === 'ready' ? event.detail.projection.bundle : null,
        },
      }));
    }
  });
  document.addEventListener('mesh:review-workbench-artifact-preview', (event) => {
    islandArtifactPreview = event.detail;
  });
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-available'));
  assert.equal(islandProjection.authority.canRenderArtifactPreview, true);
  assert.equal(islandProjection.authority.canInspectExactCopies, true);

  const committedReviewGeneration = islandProjection.generation;
  mountIslandProjection = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-available'));
  assert.ok(islandProjection.generation > committedReviewGeneration);
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: committedReviewGeneration,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'load-artifact-preview', changeId: objectId, pageNumber: 1 },
    },
  }));
  await waitFor(() => islandArtifactPreview !== null);
  assert.equal(
    islandArtifactPreview.generation,
    committedReviewGeneration,
    'a preview requested from the preserved review generation was not delivered to that visible surface',
  );
  mountIslandProjection = true;
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-mounted', {
    detail: { generation: islandProjection.generation, bundle: islandProjection.projection.bundle },
  }));
  islandArtifactPreview = null;
  renderedSides.length = 0;

  const generationBeforeReadOnlyRefresh = islandProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: generationBeforeReadOnlyRefresh,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'load-artifact-preview', changeId: objectId, pageNumber: 1 },
    },
  }));
  // Advance and mount an unchanged projection before the listener's action microtask runs. The
  // exact read-only request must rebind to this generation instead of leaving React permanently
  // stuck in its loading state.
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-available'));
  await waitFor(() => islandArtifactPreview !== null);
  assert.ok(islandProjection.generation > generationBeforeReadOnlyRefresh);
  assert.equal(islandArtifactPreview.generation, islandProjection.generation);
  assert.deepEqual(renderedSides.sort(), ['after', 'before']);
  islandArtifactPreview = null;
  renderedSides.length = 0;

  assert.equal(reviewArtifactKind(review.bundle_changes[0]), 'pdf');
  assert.equal(reviewArtifactKind({ path_after: 'people/plan.pptx' }), 'presentation');
  assert.equal(reviewArtifactKind({ path_after: 'people/policy.docx' }), 'document');
  assert.equal(reviewArtifactKind({ path_after: 'finance/budget.xlsx' }), 'spreadsheet');
  assert.equal(reviewArtifactKind({ path_after: 'assets/hero.PNG' }), 'image');
  assert.equal(reviewArtifactKind({ path_before: 'assets/photo.jpg', path_after: 'assets/photo.jpeg' }), 'image');
  assert.equal(reviewArtifactKind({ path_before: 'slides.pptx', path_after: 'slides.pdf' }), null);
  assert.equal(reviewArtifactKind({ path_before: 'report.pdf', path_after: 'report.txt' }), null);
  assert.equal(reviewArtifactKind({ path_before: 'notes.txt', path_after: 'notes.docx' }), null);

  assert.equal(islandProjection.state, 'ready');
  assert.equal(islandProjection.authority.canRenderArtifactPreview, true);
  assert.equal(islandProjection.authority.canInspectExactCopies, true);
  assert.deepEqual(renderedSides, [], 'artifact bytes were rendered before the person asked');
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'load-artifact-preview', changeId: objectId, pageNumber: 1 },
    },
  }));
  await waitFor(() => islandArtifactPreview !== null);
  assert.equal(islandArtifactPreview.before, null);
  assert.equal(islandArtifactPreview.beforeAbsentPage, null);
  assert.equal(islandArtifactPreview.afterAbsentPage, null);
  assert.match(islandArtifactPreview.beforeError, /Quick Look refused/);
  assert.match(islandArtifactPreview.after.imageDataUrl, /^data:image\/png;base64,/);
  assert.equal(islandArtifactPreview.after.textSource, 'macos-pdfkit-page-text-v1');
  assert.deepEqual(islandArtifactPreview.after.textLines, ['Revenue $120,000', 'Open roles 5']);
  assert.deepEqual(islandArtifactPreview.after.textSections, [{
    label: 'Page 1', lineStart: 0, lineCount: 2,
  }]);
  assert.deepEqual(renderedSides.sort(), ['after', 'before']);
  failedSide = null;
  renderedSides.length = 0;
  islandArtifactPreview = null;
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'load-artifact-preview', changeId: objectId, pageNumber: 1 },
    },
  }));
  await waitFor(() => islandArtifactPreview !== null);
  assert.deepEqual(renderedSides.sort(), ['after', 'before']);
  assert.match(islandArtifactPreview.before.imageDataUrl, /^data:image\/png;base64,/);
  assert.match(islandArtifactPreview.after.imageDataUrl, /^data:image\/png;base64,/);
  heldPage = 3;
  renderedSides.length = 0;
  islandArtifactPreview = null;
  const slowWorkbenchPageStarted = new Promise((resolve) => { pageStarted = resolve; });
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'load-artifact-preview', changeId: objectId, pageNumber: 3 },
    },
  }));
  await slowWorkbenchPageStarted;
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'load-artifact-preview', changeId: objectId, pageNumber: 2 },
    },
  }));
  await waitFor(() => islandArtifactPreview?.requestedPage === 2);
  releasePage();
  heldPage = null;
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(
    islandArtifactPreview.requestedPage,
    2,
    'a slower workbench render replaced the newer requested page',
  );
  renderedSides.length = 0;
  islandArtifactPreview = null;
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'load-artifact-preview', changeId: objectId, pageNumber: 3 },
    },
  }));
  await waitFor(() => islandArtifactPreview?.requestedPage === 3);
  assert.deepEqual(renderedSides, ['after'], 'the new workbench requested a known-absent PDF page');
  assert.deepEqual(islandArtifactPreview.beforeAbsentPage, {
    side: 'before',
    versionId: beforeVersion,
    contentDigest: beforeDigest,
    pageCount: 2,
  });
  assert.equal(islandArtifactPreview.before, null);
  assert.equal(islandArtifactPreview.beforeError, null);
  assert.equal(islandArtifactPreview.afterAbsentPage, null);
  assert.equal(islandArtifactPreview.after.pageNumber, 3);
  failedSide = 'after';
  renderedSides.length = 0;
  islandArtifactPreview = null;
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'load-artifact-preview', changeId: objectId, pageNumber: 3 },
    },
  }));
  await waitFor(() => islandArtifactPreview !== null);
  assert.deepEqual(renderedSides, ['after']);
  assert.match(islandArtifactPreview.afterError, /Quick Look refused/);
  failedSide = null;
  renderedSides.length = 0;
  islandArtifactPreview = null;
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'load-artifact-preview', changeId: objectId, pageNumber: 3 },
    },
  }));
  await waitFor(() => islandArtifactPreview !== null);
  assert.deepEqual(renderedSides, ['after'], 'retry restarted at page one instead of the failed page');
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'inspect-exact-copies', changeId: objectId },
    },
  }));
  await waitFor(() => exportedInspection !== null);
  assert.deepEqual(exportedInspection, {
    expectedWorkspaceRoot: workspace.root,
    expectedWorkspaceDigest: workspace.digest,
    expectedWorkspaceInstallation: workspace.installation,
    bundle: review.bundle,
    target: review.subject_operation,
    objectId,
    sides: ['before', 'after'],
    destination: '/review-output',
  });
  assert.match(document.getElementById('notice').textContent, /Exact read-only copies are ready/);
  assert.match(document.getElementById('notice').textContent, /document content was not opened automatically/);
  assert.throws(() => validatedReviewInspectionExport({
    schema: 'mesh-review-inspection-export/v1',
    directory: '/review-output/substituted',
    files: [{
      side: 'before',
      path: '/review-output/substituted/Before.pdf',
      version_id: afterVersion,
      content_digest: beforeDigest,
    }],
    working_folder_unchanged: true,
    document_content_opened: false,
    finder_opened: true,
    warning: null,
  }, review.bundle_changes[0], ['before']), /did not match/);

  activeArtifactKind = 'presentation';
  review.bundle_changes[0].path_before = 'finance/board-pack.pptx';
  review.bundle_changes[0].path_after = 'finance/board-pack.pptx';
  renderedSides.length = 0;
  await document.emitWorkspaceCurrentIntent('refresh');
  islandArtifactPreview = null;
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'load-artifact-preview', changeId: objectId, pageNumber: 1 },
    },
  }));
  await waitFor(() => islandArtifactPreview !== null);
  assert.deepEqual(renderedSides.sort(), ['after', 'before']);
  assert.equal(islandArtifactPreview.kind, 'presentation');
  assert.equal(islandArtifactPreview.after.textSource, 'mesh-pptx-slide-text-v1');
  activeArtifactKind = 'image';
  review.bundle_changes[0].path_before = 'assets/hero.png';
  review.bundle_changes[0].path_after = 'assets/hero.jpg';
  renderedSides.length = 0;
  await document.emitWorkspaceCurrentIntent('refresh');
  islandArtifactPreview = null;
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'load-artifact-preview', changeId: objectId, pageNumber: 1 },
    },
  }));
  await waitFor(() => islandArtifactPreview !== null);
  assert.equal(islandArtifactPreview.kind, 'image');
  assert.deepEqual(renderedSides.sort(), ['after', 'before']);
  exportedInspection = null;
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: islandProjection.generation,
      bundle: islandProjection.projection.bundle,
      intent: { type: 'inspect-exact-copies', changeId: objectId },
    },
  }));
  await waitFor(() => exportedInspection !== null);
  assert.deepEqual(exportedInspection.sides, ['before', 'after']);
  review.bundle_changes[0].path_before = 'finance/board-pack.pdf';
  review.bundle_changes[0].path_after = 'finance/board-pack.pdf';

  const textDiff = artifactTextDiff(
    {
      text_lines: ['Revenue\t$100,000', 'Open roles\t4'],
      text_sections: [{ label: 'Sheet 1', line_start: 0, line_count: 2 }],
    },
    {
      text_lines: ['Revenue\t$120,000', 'Open roles\t5'],
      text_sections: [{ label: 'Sheet 1', line_start: 0, line_count: 2 }],
    },
  );
  assert.deepEqual(
    textDiff.hunks[0].lines.filter((line) => line.kind !== 'context').map((line) => line.kind),
    ['removed', 'removed', 'added', 'added'],
  );
  const repeatedTextInDifferentSheets = artifactTextDiff(
    {
      text_lines: ['A1\tvalue\tApproved'],
      text_sections: [{ label: 'Finance', line_start: 0, line_count: 1 }],
    },
    {
      text_lines: ['A1\tvalue\tApproved'],
      text_sections: [{ label: 'People', line_start: 0, line_count: 1 }],
    },
  );
  assert.deepEqual(
    repeatedTextInDifferentSheets.hunks[0].lines.map((line) => line.kind),
    ['removed', 'removed', 'added', 'added'],
    'identical content in a different worksheet was falsely aligned as unchanged',
  );
  assert.deepEqual(
    artifactTextSectionChoices(
      {
        text_sections: [
          { label: 'Budget', line_start: 0, line_count: 1 },
          { label: 'Hiring', line_start: 1, line_count: 1 },
        ],
      },
      {
        text_sections: [
          { label: 'Budget', line_start: 0, line_count: 1 },
          { label: 'Risk', line_start: 1, line_count: 1 },
        ],
      },
      'spreadsheet',
    ).map(({ label, before_present, after_present }) => ({ label, before_present, after_present })),
    [
      { label: 'Budget', before_present: true, after_present: true },
      { label: 'Hiring', before_present: true, after_present: false },
      { label: 'Risk', before_present: false, after_present: true },
    ],
  );
  const hiddenChange = artifactTextChangeSummary(artifactTextDiff(
    {
      text_lines: ['Revenue\t$120,000'],
      text_sections: [{ label: 'Sheet 1', line_start: 0, line_count: 1 }],
    },
    {
      text_lines: ['Revenue\t$120,000'],
      text_sections: [{ label: 'Sheet 1', line_start: 0, line_count: 1 }],
    },
  ));
  assert.equal(hiddenChange.changed, 0);
  assert.equal(hiddenChange.title, 'Artifact changed; visible text unchanged');
  assert.match(hiddenChange.warning, /No visible text difference/);
  assert.match(hiddenChange.warning, /before approval/);
  const semanticChange = artifactTextChangeSummary(textDiff, 'mesh-xlsx-cell-formula-v1');
  assert.equal(semanticChange.title, 'Cell and formula changes');
  const slideChange = artifactTextChangeSummary(textDiff, 'mesh-pptx-slide-text-v1');
  assert.equal(slideChange.title, 'Slide content changes');
  const slideHiddenChange = artifactTextChangeSummary(
    artifactTextDiff(
      {
        text_lines: ['Approved hiring plan'],
        text_sections: [{ label: 'Slide 2 · Hiring plan', line_start: 0, line_count: 1 }],
      },
      {
        text_lines: ['Approved hiring plan'],
        text_sections: [{ label: 'Slide 2 · Hiring plan', line_start: 0, line_count: 1 }],
      },
    ),
    'mesh-pptx-slide-text-v1',
  );
  assert.equal(slideHiddenChange.title, 'Artifact changed; slide text unchanged');
  assert.match(slideHiddenChange.warning, /PowerPoint or Keynote/);
  assert.match(
    artifactTextExtractionNote('presentation', 'mesh-pptx-slide-text-v1', true),
    /exact slide order.*OOXML slide name.*never executes presentation content.*speaker notes.*embedded content/,
  );
  const documentVisibilityChange = artifactTextChangeSummary(
    artifactTextDiff(
      {
        text_lines: ['Compensation adjustment'],
        text_sections: [{ label: 'Section 1 · People plan', line_start: 0, line_count: 1 }],
      },
      {
        text_lines: ['<hidden text: Compensation adjustment>'],
        text_sections: [{ label: 'Section 1 · People plan', line_start: 0, line_count: 1 }],
      },
    ),
    'mesh-docx-block-text-v1',
  );
  assert.equal(documentVisibilityChange.title, 'Document content and visibility changes');
  assert.match(
    artifactTextExtractionNote('document', 'mesh-docx-block-text-v1', false),
    /hidden-run visibility.*style-inherited visibility.*exact saved document bytes/,
  );
  const formulaOnlyDiff = artifactTextDiff(
    {
      text_lines: ['B1\tformula\tSUM(B2:B4)\tresult\t380000'],
      text_sections: [{ label: 'Budget', line_start: 0, line_count: 1 }],
    },
    {
      text_lines: ['B1\tformula\tAVERAGE(B2:B4)\tresult\t380000'],
      text_sections: [{ label: 'Budget', line_start: 0, line_count: 1 }],
    },
  );
  assert.deepEqual(
    formulaOnlyDiff.hunks[0].lines.map((line) => line.kind),
    ['context', 'removed', 'added'],
    'a changed formula with the same cached result remained hidden',
  );
  const semanticHiddenChange = artifactTextChangeSummary(
    artifactTextDiff(
      {
        text_lines: ['B1\tformula\tSUM(B2:B4)\tresult\t380000'],
        text_sections: [{ label: 'Budget', line_start: 0, line_count: 1 }],
      },
      {
        text_lines: ['B1\tformula\tSUM(B2:B4)\tresult\t380000'],
        text_sections: [{ label: 'Budget', line_start: 0, line_count: 1 }],
      },
    ),
    'mesh-xlsx-cell-formula-v1',
  );
  assert.equal(semanticHiddenChange.title, 'Artifact changed; cells and formulas unchanged');
  assert.match(semanticHiddenChange.warning, /No cell or formula difference/);
  for (const [source, label] of [
    ['mesh-xlsx-cell-formula-v1', 'workbook'],
    ['mesh-pptx-slide-text-v1', 'presentation'],
    ['mesh-docx-block-text-v1', 'document'],
    ['macos-pdfkit-page-text-v1', 'page'],
    ['macos-quick-look-visible-text', 'visible-text'],
  ]) {
    const boundedPrefix = artifactTextChangeSummary(
      artifactTextDiff(
        {
          text_lines: ['same retained row'],
          text_sections: [{ label: 'Section', line_start: 0, line_count: 1 }],
        },
        {
          text_lines: ['same retained row'],
          text_sections: [{ label: 'Section', line_start: 0, line_count: 1 }],
        },
      ),
      source,
      true,
    );
    assert.equal(boundedPrefix.title, `No differences in the extracted ${label} prefix`);
    assert.doesNotMatch(boundedPrefix.title, /match|unchanged/i);
    assert.match(boundedPrefix.warning, /beyond the extracted prefix was not compared/);
  }
  assert.match(
    artifactTextExtractionNote('spreadsheet', 'mesh-xlsx-cell-formula-v1', true),
    /stored values.*first 512 cells.*never executes the workbook.*embedded content/,
  );
  assert.equal(artifactTextSourcesMatch([
    { text_source: 'mesh-xlsx-cell-formula-v1' },
    { text_source: 'mesh-xlsx-cell-formula-v1' },
  ]), true);
  assert.equal(artifactTextSourcesMatch([
    { text_source: 'mesh-xlsx-cell-formula-v1' },
    { text_source: 'macos-quick-look-visible-text' },
  ]), false);
  assert.equal(artifactTextDiff({
    text_lines: Array(513).fill('bounded'),
    text_sections: [{ label: 'Sheet 1', line_start: 0, line_count: 513 }],
  }, null), null);

  assert.throws(() => validatedReviewArtifactPreview({
    renderer: 'macos-pdfkit-page-v1',
    scope: 'exact-page-preview',
    kind: 'pdf',
    side: 'after',
    version_id: afterVersion,
    content_digest: 'ff'.repeat(32),
    image_data_url: 'data:image/png;base64,YQ==',
    text_source: null,
    text_lines: null,
    text_sections: null,
    text_truncated: false,
    page_number: 1,
    page_count: 3,
    rendering_authorizes_approval: false,
  }, review.bundle_changes[0], 'after', 'pdf'), /did not match the exact reviewed artifact/);

  assert.throws(() => validatedReviewArtifactPreview({
    renderer: 'macos-pdfkit-page-v1',
    scope: 'exact-page-preview',
    kind: 'pdf',
    side: 'after',
    version_id: afterVersion,
    content_digest: afterDigest,
    image_data_url: 'data:image/png;base64,YQ==',
    text_source: 'macos-pdfkit-page-text-v1',
    text_lines: ['Revenue\u202e000,021$'],
    text_sections: [{ label: 'Page 1', line_start: 0, line_count: 1 }],
    text_truncated: false,
    page_number: 1,
    page_count: 3,
    rendering_authorizes_approval: false,
  }, review.bundle_changes[0], 'after', 'pdf'), /extracted artifact text was malformed/);
  const exactPdfPage = {
    renderer: 'macos-pdfkit-page-v1',
    scope: 'exact-page-preview',
    kind: 'pdf',
    side: 'after',
    version_id: afterVersion,
    content_digest: afterDigest,
    image_data_url: 'data:image/png;base64,YQ==',
    text_source: 'macos-pdfkit-page-text-v1',
    text_lines: ['Finance and HR review required'],
    text_sections: [{ label: 'Page 2', line_start: 0, line_count: 1 }],
    text_truncated: false,
    page_number: 2,
    page_count: 3,
    rendering_authorizes_approval: false,
  };
  assert.equal(
    validatedReviewArtifactPreview(
      exactPdfPage,
      review.bundle_changes[0],
      'after',
      'pdf',
    ).page_number,
    2,
  );
  assert.throws(() => validatedReviewArtifactPreview({
    ...exactPdfPage,
    text_sections: [{ label: 'Page 1', line_start: 0, line_count: 1 }],
  }, review.bundle_changes[0], 'after', 'pdf'), /extracted artifact text was malformed/);
  assert.equal(validatedReviewArtifactPreview({
    ...exactPdfPage,
    page_count: 65,
  }, review.bundle_changes[0], 'after', 'pdf').page_count, 65);
  assert.throws(() => validatedReviewArtifactPreview({
    ...exactPdfPage,
    page_count: 1_000_001,
  }, review.bundle_changes[0], 'after', 'pdf'), /did not match the exact reviewed artifact/);

  const spreadsheetChange = {
    ...review.bundle_changes[0],
    path_before: 'finance/budget.xlsx',
    path_after: 'finance/budget.xlsx',
  };
  const semanticPreview = {
    renderer: 'macos-quick-look-thumbnail',
    scope: 'representative-preview',
    kind: 'spreadsheet',
    side: 'after',
    version_id: afterVersion,
    content_digest: afterDigest,
    image_data_url: 'data:image/png;base64,YQ==',
    text_source: 'mesh-xlsx-cell-formula-v1',
    text_lines: ['B1\tformula\tSUM(B2:B4)\tresult\t380000'],
    text_sections: [{ label: 'Budget', line_start: 0, line_count: 1 }],
    text_truncated: false,
    page_number: null,
    page_count: null,
    rendering_authorizes_approval: false,
  };
  assert.equal(
    validatedReviewArtifactPreview(
      semanticPreview,
      spreadsheetChange,
      'after',
      'spreadsheet',
    ).text_source,
    'mesh-xlsx-cell-formula-v1',
  );
  assert.throws(() => validatedReviewArtifactPreview({
    ...semanticPreview,
    text_lines: ['A1\tvalue\t1', 'A2\tvalue\t2'],
    text_sections: [
      { label: 'Budget', line_start: 0, line_count: 1 },
      { label: 'Budget', line_start: 1, line_count: 1 },
    ],
  }, spreadsheetChange, 'after', 'spreadsheet'), /extracted artifact text was malformed/);
  const imageChange = {
    ...review.bundle_changes[0],
    path_before: 'assets/hero.png',
    path_after: 'assets/hero.png',
  };
  const imagePreview = {
    ...semanticPreview,
    renderer: 'macos-imageio-thumbnail-v1',
    kind: 'image',
    text_source: null,
    text_lines: null,
    text_sections: null,
    text_truncated: false,
  };
  assert.equal(
    validatedReviewArtifactPreview(
      imagePreview,
      imageChange,
      'after',
      'image',
    ).content_digest,
    afterDigest,
  );
  assert.throws(() => validatedReviewArtifactPreview({
    ...imagePreview,
    renderer: 'macos-quick-look-thumbnail',
  }, imageChange, 'after', 'image'), /did not match the exact reviewed artifact/);
  assert.throws(() => validatedReviewArtifactPreview({
    ...semanticPreview,
    kind: 'pdf',
    renderer: 'macos-pdfkit-page-v1',
    scope: 'exact-page-preview',
    page_number: 1,
    page_count: 3,
  }, review.bundle_changes[0], 'after', 'pdf'), /extracted artifact text was malformed/);
  const presentationChange = {
    ...review.bundle_changes[0],
    path_before: 'finance/board-pack.pptx',
    path_after: 'finance/board-pack.pptx',
  };
  const presentationPreview = {
    ...semanticPreview,
    kind: 'presentation',
    text_source: 'mesh-pptx-slide-text-v1',
    text_lines: ['Approve three hires after the revised cash-flow review.'],
    text_sections: [{ label: 'Slide 1 · Hiring plan', line_start: 0, line_count: 1 }],
  };
  assert.equal(
    validatedReviewArtifactPreview(
      presentationPreview,
      presentationChange,
      'after',
      'presentation',
    ).text_source,
    'mesh-pptx-slide-text-v1',
  );
  assert.throws(() => validatedReviewArtifactPreview({
    ...presentationPreview,
    text_sections: [{ label: 'Slide 2 · Hiring plan', line_start: 0, line_count: 1 }],
  }, presentationChange, 'after', 'presentation'), /extracted artifact text was malformed/);
  const documentChange = {
    ...presentationChange,
    path_before: 'people/plan.docx',
    path_after: 'people/plan.docx',
  };
  const documentPreview = {
    ...semanticPreview,
    kind: 'document',
    text_source: 'mesh-docx-block-text-v1',
    text_lines: ['People plan', 'Approve three hires', 'Owner\tFinance'],
    text_sections: [{ label: 'Section 1 · People plan', line_start: 0, line_count: 3 }],
  };
  assert.equal(
    validatedReviewArtifactPreview(
      documentPreview,
      documentChange,
      'after',
      'document',
    ).text_source,
    'mesh-docx-block-text-v1',
  );
  assert.deepEqual(
    artifactTextSectionChoices(null, documentPreview, 'document').map((choice) => choice.label),
    ['Section 1'],
  );
  const documentOpeningPreview = {
    ...documentPreview,
    text_lines: ['Introductory context', 'People plan', 'Approve three hires'],
    text_sections: [
      { label: 'Document opening', line_start: 0, line_count: 1 },
      { label: 'Section 1 · People plan', line_start: 1, line_count: 2 },
    ],
  };
  assert.equal(
    validatedReviewArtifactPreview(
      documentOpeningPreview,
      documentChange,
      'after',
      'document',
    ).text_source,
    'mesh-docx-block-text-v1',
  );
  assert.deepEqual(
    artifactTextSectionChoices(null, documentOpeningPreview, 'document')
      .map((choice) => choice.label),
    ['Document opening', 'Section 1'],
  );
  const emojiHeading = '📊'.repeat(57);
  const emojiDocumentPreview = {
    ...documentPreview,
    text_lines: [emojiHeading],
    text_sections: [{
      label: `Section 1 · ${'📊'.repeat(56)}…`,
      line_start: 0,
      line_count: 1,
    }],
  };
  assert.equal(
    validatedReviewArtifactPreview(
      emojiDocumentPreview,
      documentChange,
      'after',
      'document',
    ).image_data_url,
    semanticPreview.image_data_url,
    'a valid native astral Word heading discarded the exact rendered PNG',
  );
  assert.deepEqual(
    artifactTextSectionChoices(
      documentOpeningPreview,
      {
        ...documentOpeningPreview,
        text_lines: ['Introductory context revised', 'Workforce plan', 'Approve four hires'],
        text_sections: [
          { label: 'Document opening', line_start: 0, line_count: 1 },
          { label: 'Section 1 · Workforce plan', line_start: 1, line_count: 2 },
        ],
      },
      'document',
    ).map(({ label, before_present, after_present }) => ({ label, before_present, after_present })),
    [
      { label: 'Document opening', before_present: true, after_present: true },
      { label: 'Section 1', before_present: true, after_present: true },
    ],
  );
  assert.throws(() => validatedReviewArtifactPreview({
    ...presentationPreview,
    kind: 'document',
  }, documentChange, 'after', 'document'), /extracted artifact text was malformed/);
});

test('a lost approval reply immediately recovers the committed shared version without replay', async () => {
  const document = fakeDocument();
  const reviewOperation = '71'.repeat(32);
  const reviewedHead = 'approved-after-lost-reply';
  let workspace = {
    root: '/managed/recovered-approval',
    digest: 'workspace-before-approval',
    installation: 'installation-recovered-approval',
    records: 1,
    reviews: 1,
    review_items: [recordedReadyReview(reviewOperation, reviewedHead)],
    review_items_not_listed: 0,
    private_version: { version: 'private-recovered-approval', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    workspace_versions: [{ operation: reviewOperation, ordinal: 1, actor_sequence: '1' }],
    file_histories: [],
  };
  let approvalAttempts = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      auto_opened: false,
      remembered: workspace.root,
      workspaces: [workspace.root],
      export_root: '/ordinary/recovered-approval',
      workspace_entries: [{
        path: workspace.root,
        export_root: '/ordinary/recovered-approval',
        project_root: '/ordinary/recovered-approval',
      }],
    });
    if (command === 'approval_credential_status') return JSON.stringify({ enrolled: true, available: true, unavailable_reason: null });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      native_folder: false,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'approve_current_review') {
      approvalAttempts += 1;
      workspace = {
        ...workspace,
        digest: 'workspace-after-approval',
        shared_version: reviewedHead,
      };
      throw new Error('Mesh lost the approval reply after the shared version committed');
    }
    throw new Error(`unexpected recovered-approval command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?recovered-approval=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.notEqual(document.workspaceOverview?.overview.nextActionTitle, 'Update the original folder');
  await document.emitReviewIntent({ type: 'approve-version' });

  assert.equal(approvalAttempts, 1, 'Mesh replayed an approval whose result was unknown');
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Update the original folder');
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Preview original update');
  assert.equal(!document.workspaceCurrentAction('update-destination')?.enabled, false);
  assert.match(document.workspaceOverview?.overview.nextActionDescription, /did not change your original folder/);
  assert.match(document.getElementById('notice').textContent, /lost the approval reply.*confirmed.*shared version/i);
});

test('lost approval recovery never adopts a matching shared version from another workspace', async () => {
  const document = fakeDocument();
  const reviewOperation = '74'.repeat(32);
  const reviewedHead = 'same-head-different-workspace';
  const attemptedRoot = '/managed/approval-workspace-a';
  const attemptedInstallation = 'installation-approval-a';
  let workspace = {
    root: attemptedRoot,
    digest: 'workspace-before-approval-a',
    installation: attemptedInstallation,
    records: 1,
    reviews: 1,
    review_items: [recordedReadyReview(reviewOperation, reviewedHead)],
    review_items_not_listed: 0,
    private_version: { version: 'private-approval-a', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    workspace_versions: [{ operation: reviewOperation, ordinal: 1, actor_sequence: '1' }],
    file_histories: [],
  };
  let approvalAttempts = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      auto_opened: false,
      remembered: workspace.root,
      workspaces: [workspace.root],
      export_root: '/ordinary/approval-project',
      workspace_entries: [{
        path: workspace.root,
        export_root: '/ordinary/approval-project',
        project_root: '/ordinary/approval-project',
      }],
    });
    if (command === 'approval_credential_status') return JSON.stringify({ enrolled: true, available: true, unavailable_reason: null });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      native_folder: false,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'approve_current_review') {
      approvalAttempts += 1;
      workspace = {
        ...workspace,
        root: '/managed/approval-workspace-b',
        digest: 'workspace-after-approval-b',
        installation: 'installation-approval-b',
        private_version: { version: 'private-approval-b', concurrent_changes: 1 },
        shared_version: reviewedHead,
      };
      throw new Error('Mesh lost the approval reply while another workspace opened');
    }
    throw new Error(`unexpected cross-workspace approval command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?cross-workspace-approval=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.emitReviewIntent({ type: 'approve-version' });

  assert.equal(approvalAttempts, 1, 'Mesh replayed an approval whose result was unknown');
  assert.equal(workspace.root, '/managed/approval-workspace-b');
  assert.doesNotMatch(document.getElementById('notice').textContent, /confirmed that this exact reviewed version/i);
  assert.match(document.getElementById('notice').textContent, /could not verify the exact reviewed workspace/i);
  assert.equal(document.getElementById('notice').classList.contains('error'), true);
});

test('restart recovers an approved original-folder update until its exact version is complete', async () => {
  const document = fakeDocument();
  const reviewOperation = '72'.repeat(32);
  const reviewedHead = 'approved-after-restart';
  const workspace = {
    root: '/managed/restarted-approval',
    digest: 'workspace-restarted-approval',
    installation: 'installation-restarted-approval',
    records: 2,
    reviews: 1,
    review_items: [recordedCurrentReview(reviewOperation, reviewedHead)],
    review_items_not_listed: 0,
    private_version: { version: 'private-restarted-approval', concurrent_changes: 1 },
    shared_version: reviewedHead,
    entries: [{ path: 'approved.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    workspace_versions: [{ operation: reviewOperation, ordinal: 1, actor_sequence: '1' }],
    file_histories: [{
      path: 'approved.txt',
      object_id: 'approved-object',
      current: { version_id: 'approved-file-version', manifest_id: 'approved-manifest' },
      retained_versions: [{ version_id: 'approved-file-version', manifest_id: 'approved-manifest' }],
    }],
  };
  let nativeInspections = 0;
  let previewParameters = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      auto_opened: true,
      remembered: workspace.root,
      workspaces: [workspace.root],
      active_folder: '/application/native-workspace/current',
      export_root: '/ordinary/restarted-approval',
      workspace_entries: [{
        path: workspace.root,
        export_root: '/ordinary/restarted-approval',
        project_root: '/ordinary/restarted-approval',
        original_update_version: 'older-approved-version',
      }],
    });
    if (command === 'approval_credential_status') return JSON.stringify({ enrolled: true, available: true, unavailable_reason: null });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      native_folder: true,
      native_folder_path: workspace.root,
      working: false,
    });
    if (command === 'reconcile_managed_workspace_navigation') return JSON.stringify({
      path: '/application/native-workspace/current',
      workspace_root: workspace.root,
      stable: true,
      native_folder: true,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'inspect_managed_file') {
      nativeInspections += 1;
      assert.deepEqual(parameters, {
        relativePath: 'approved.txt',
        expectedWorkspaceRoot: workspace.root,
        expectedWorkspaceDigest: workspace.digest,
        expectedWorkspaceInstallation: workspace.installation,
      });
      return JSON.stringify({
        path: 'approved.txt',
        text: 'approved bytes\n',
        text_editable: true,
        native_untracked: false,
        native_missing: false,
        modified_from_current_version: false,
        current_version: 'approved-file-version',
        byte_count: 15,
        content_digest: 'approved-content-digest',
        executable: false,
        installation: workspace.installation,
      });
    }
    if (command === 'preview_managed_exports') {
      previewParameters = parameters;
      return JSON.stringify([{
        path: 'approved.txt',
        source_version: 'approved-file-version',
        source_byte_count: 15,
        source_content_digest: 'approved-content-digest',
        source_executable: false,
        source_text: 'approved bytes\n',
        target_root: '/ordinary/restarted-approval',
        target_installation: 'ordinary-installation',
        target_parent_installation: 'ordinary-parent-installation',
        target_file_installation: 'ordinary-file-installation',
        target_exists: true,
        target_byte_count: 12,
        target_content_digest: 'older-content-digest',
        target_executable: false,
        target_text: 'older bytes\n',
        identical: false,
        target_relation: 'imported-unchanged',
        replace_allowed: true,
      }]);
    }
    if (command === 'discover_retired_exports') return '[]';
    throw new Error(`unexpected restarted-approval command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?restarted-approval=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Update the original folder');
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Preview original update');
  assert.equal(!document.workspaceCurrentAction('update-destination')?.enabled, false);
  assert.match(document.workspaceOverview?.overview.nextActionDescription, /did not change your original folder/);
  const inspectionsBeforePreview = nativeInspections;
  await document.emitWorkspaceOverviewIntent('recommended');
  assert.equal(nativeInspections, inspectionsBeforePreview + 1, 'the recovered original-update shortcut skipped its fresh native scan');
  assert.deepEqual(previewParameters, {
    targetRoot: '/ordinary/restarted-approval',
    expectedWorkspaceRoot: workspace.root,
    expectedWorkspaceDigest: workspace.digest,
    expectedWorkspaceInstallation: workspace.installation,
  });
  assert.equal(document.destinationActionControl('confirm-batch').disabled, false);
  await document.emitWorkspaceDestinationIntent({
    type: 'set-field',
    field: 'destination',
    value: '/ordinary/different-target',
  });
  const inspectionsBeforeStaleShortcut = nativeInspections;
  await document.emitWorkspaceOverviewIntent('recommended');
  assert.equal(nativeInspections, inspectionsBeforeStaleShortcut, 'a changed target passed the recovered original-update authority check');
  assert.match(document.getElementById('notice').textContent, /original-folder update is no longer current/i);
});

test('an incomplete review projection never recommends an original-folder write it cannot authorize', async () => {
  const document = fakeDocument();
  let reviewProjection = null;
  document.addEventListener('mesh:review-workbench-projection', (event) => {
    reviewProjection = event.detail;
    document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-mounted', {
      detail: { generation: event.detail.generation, bundle: null },
    }));
  });
  const workspace = {
    root: '/managed/incomplete-approval',
    digest: 'workspace-incomplete-approval',
    installation: 'installation-incomplete-approval',
    records: 2,
    reviews: 1,
    review_items: [],
    review_items_not_listed: 1,
    private_version: { version: 'private-incomplete-approval', concurrent_changes: 1 },
    shared_version: 'approved-but-unprojected',
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    workspace_versions: [{ operation: '73'.repeat(32), ordinal: 1, actor_sequence: '1' }],
    file_histories: [],
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      auto_opened: true,
      remembered: workspace.root,
      workspaces: [workspace.root],
      export_root: '/ordinary/incomplete-approval',
      workspace_entries: [{
        path: workspace.root,
        export_root: '/ordinary/incomplete-approval',
        project_root: '/ordinary/incomplete-approval',
        original_update_version: 'older-approved-version',
      }],
    });
    if (command === 'approval_credential_status') return JSON.stringify({ enrolled: true, available: true, unavailable_reason: null });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      native_folder: false,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    throw new Error(`unexpected incomplete-approval command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?incomplete-approval=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-available'));
  await waitFor(() => reviewProjection?.state === 'unavailable');

  assert.equal(!document.workspaceCurrentAction('update-destination')?.enabled, true);
  assert.notEqual(document.workspaceOverview?.overview.nextActionTitle, 'Update the original folder');
  assert.notEqual(document.workspaceOverview?.overview.nextActionLabel, 'Preview original update');
  assert.match(document.getElementById('empty-reviews-message').textContent, /could not verify a complete bounded review/i);
  assert.equal(reviewProjection.projection, undefined);
  assert.equal(reviewProjection.authority, undefined);
  assert.match(reviewProjection.status.description, /could not verify a complete bounded review/i);
  assert.equal(document.getElementById('review-workbench-next').classList.contains('hidden'), false);
});

test('restart does not repeat the original-folder prompt after the exact shared version was applied', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/completed-original-update',
    digest: 'workspace-completed-original-update',
    installation: 'installation-completed-original-update',
    records: 2,
    reviews: 1,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'private-completed-original-update', concurrent_changes: 1 },
    shared_version: 'approved-and-applied',
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    workspace_versions: [],
    file_histories: [],
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      auto_opened: true,
      remembered: workspace.root,
      workspaces: [workspace.root],
      export_root: '/ordinary/completed-original-update',
      workspace_entries: [{
        path: workspace.root,
        export_root: '/ordinary/completed-original-update',
        project_root: '/ordinary/completed-original-update',
        original_update_version: workspace.shared_version,
      }],
    });
    if (command === 'approval_credential_status') return JSON.stringify({ enrolled: true, available: true, unavailable_reason: null });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      native_folder: false,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    throw new Error(`unexpected completed-original-update command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?completed-original-update=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.notEqual(document.workspaceOverview?.overview.nextActionTitle, 'Update the original folder');
  assert.notEqual(document.workspaceOverview?.overview.nextActionLabel, 'Preview original update');
});

test('review cards put the current saved version first and require a fresh review after copying an earlier point', async () => {
  const document = fakeDocument();
  const earlierOperation = '10'.repeat(32);
  const currentOperation = '20'.repeat(32);
  const review = (operation, bundle, reviewedHead, sequence) => ({
    bundle,
    subject_operation: operation,
    reviewed_head: reviewedHead,
    opened_by: '30'.repeat(32),
    author: '40'.repeat(32),
    recorded: true,
    actor_sequence: sequence,
    subject_operations: [{ kind: 'WriteFileVersion', canonical_hex: 'a1' }],
    subject_operations_not_listed: 0,
    presentation_digest: '50'.repeat(32),
    bundle_changes: [{
      object_id: '60'.repeat(16),
      path_before: '/notes.txt',
      path_after: '/notes.txt',
      effect: 'content-written',
      before: null,
      after: null,
      body: 'opaque',
      opaque_reason: 'test fixture',
    }],
    bundle_changes_not_listed: 0,
    content_complete: true,
    unavailable_code: null,
    projection_authorizes_approval: false,
  });
  const workspace = {
    root: '/managed/versioned-review',
    digest: 'workspace-versioned-review',
    installation: 'installation-versioned-review',
    records: 4,
    reviews: 2,
    review_items_not_listed: 0,
    review_items: [
      review(earlierOperation, '70'.repeat(32), '80'.repeat(32), '1'),
      review(currentOperation, '90'.repeat(32), 'a0'.repeat(32), '2'),
    ],
    private_version: { version: 'private-current', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
    workspace_versions: [
      { operation: earlierOperation, ordinal: 1, actor_sequence: '1' },
      { operation: currentOperation, ordinal: 2, actor_sequence: '2' },
    ],
  };
  let earlierPreviewParameters = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'approval_credential_status') {
      return JSON.stringify({ enrolled: true, available: true, unavailable_reason: null, algorithm: 'es256', user_verification: 'user-presence' });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    }
    if (command === 'preview_managed_workspace_version') {
      earlierPreviewParameters = parameters;
      return savedWorkspacePreview(earlierOperation, [{ path: 'notes.txt', type: 'file', bytes: '12' }]);
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?versioned-review-cards=${Date.now()}`);
  await waitFor(() => document.getElementById('setup-approval').textContent === 'Approval ready');

  assert.equal(document.reviewPage.state, 'ready');
  assert.equal(document.reviewPage.projection.subject_operation, currentOperation);
  assert.equal(document.reviewPage.authority.canApprove, true);
  assert.deepEqual(document.reviewPage.controls.earlierReviews, [{
    operation: earlierOperation,
    label: `Review saved point ${earlierOperation.slice(0, 12)}… again`,
    canOpen: true,
  }]);

  await document.emitReviewIntent({ type: 'open-earlier-review', operation: earlierOperation });
  assert.deepEqual(earlierPreviewParameters, {
    operation: earlierOperation,
    expectedWorkspaceRoot: workspace.root,
    expectedWorkspaceDigest: workspace.digest,
    expectedWorkspaceInstallation: workspace.installation,
  });
  assert.equal(document.getElementById('workspace-version').value, earlierOperation);
  assert.equal(document.getElementById('workspace-versions-next').scrolledIntoView, true);
  assert.equal(document.workspaceVersionChoice(earlierOperation).focused, true);
  assert.match(document.getElementById('workspace-version-preview').textContent, /notes\.txt/);
  assert.match(document.getElementById('notice').textContent, /open it as an independent working folder/);
  assert.match(document.getElementById('notice').textContent, /record its fresh review there before approval/);
  assert.match(document.getElementById('notice').textContent, /Newer private work remains unchanged/);
});

test('new native agent work blocks recording or approving an incomplete review scope', async () => {
  const document = fakeDocument();
  const earlierOperation = '10'.repeat(32);
  const currentOperation = '20'.repeat(32);
  const review = (operation, recorded, suffix) => ({
    bundle: suffix.repeat(64),
    subject_operation: operation,
    reviewed_head: `${suffix}1`.repeat(32),
    opened_by: recorded ? `${suffix}2`.repeat(32) : null,
    author: `${suffix}3`.repeat(32),
    recorded,
    actor_sequence: recorded ? '1' : '2',
    subject_operations: [{ kind: 'WriteFileVersion', canonical_hex: 'a1' }],
    subject_operations_not_listed: 0,
    presentation_digest: `${suffix}4`.repeat(32),
    bundle_changes: [],
    bundle_changes_not_listed: 0,
    content_complete: true,
    unavailable_code: null,
    projection_authorizes_approval: false,
  });
  const workspace = {
    root: '/managed/agent-review-scope',
    digest: 'workspace-agent-review-scope',
    installation: 'installation-agent-review-scope',
    records: 3,
    reviews: 1,
    review_items_not_listed: 0,
    review_items: [
      review(earlierOperation, true, '7'),
      review(currentOperation, false, '8'),
    ],
    private_version: { version: 'private-current', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: ['agent-alpha-notes.md'],
    native_unsupported_entries: [{ path: 'agent-latest', kind: 'symbolic-link' }],
    native_inventory_complete: true,
    file_histories: [],
    workspace_versions: [
      { operation: earlierOperation, ordinal: 1, actor_sequence: '1' },
      { operation: currentOperation, ordinal: 2, actor_sequence: '2' },
    ],
  };
  let reviewMutations = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'approval_credential_status') {
      return JSON.stringify({ enrolled: true, available: true, unavailable_reason: null, algorithm: 'es256', user_verification: 'user-presence' });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'inspect_native_file') {
      return JSON.stringify({
        path: 'agent-alpha-notes.md',
        text: 'work from the agent\n',
        text_editable: true,
        native_untracked: true,
        native_missing: false,
        modified_from_current_version: true,
        current_version: null,
        byte_count: 20,
        content_digest: '99'.repeat(32),
        executable: false,
        installation: workspace.installation,
      });
    }
    if (command === 'open_current_review' || command === 'approve_current_review') {
      reviewMutations += 1;
      throw new Error('pending native work reached a review mutation');
    }
    throw new Error(`unexpected review-scope command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?agent-review-scope=${Date.now()}`);
  await waitFor(() => document.getElementById('setup-approval').textContent === 'Approval ready');

  assert.equal(document.getElementById('open-current-review').disabled, true);
  assert.equal(document.reviewPage.controls.earlierReviews[0].canOpen, false);
  assert.match(document.reviewPage.controls.recordReviewReason, /2 newer native changes are not included in this review/);

  await document.getElementById('scan-files').emit('click');

  assert.equal(document.getElementById('open-current-review').disabled, true);
  assert.equal(document.reviewPage.controls.earlierReviews[0].canOpen, false);
  assert.match(document.reviewPage.controls.recordReviewReason, /2 newer native changes are not included in this review/);
  assert.match(document.reviewPage.controls.recordReviewReason, /Save them privately or resolve them/);
  assert.match(document.getElementById('folder-change-items').children[0].textContent, /agent-latest.*symbolic link.*convert or remove/);
  assert.equal(document.getElementById('save-all-private').disabled, true);
  await document.getElementById('open-current-review').emit('click');
  await document.emitReviewIntent({ type: 'open-earlier-review', operation: earlierOperation });
  assert.equal(reviewMutations, 0);

  workspace.native_untracked_files = [];
  workspace.native_unsupported_entries = [{ path: 'build/keep', kind: 'excluded-ancestor' }];
  await document.getElementById('scan-files').emit('click');
  assert.match(
    document.getElementById('folder-change-items').children[0].textContent,
    /build\/keep.*re-included below an excluded parent.*include its parent too/,
  );
  assert.match(document.workspaceOverview?.overview.nextActionDescription, /Include the parent too/);
  assert.equal(document.getElementById('save-all-private').disabled, true);

  workspace.native_untracked_files = [];
  workspace.native_unsupported_entries = [];
  workspace.native_inventory_complete = false;
  await document.getElementById('scan-files').emit('click');
  assert.equal(document.getElementById('open-current-review').disabled, true);
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Finish checking the native folder');
  assert.match(document.getElementById('notice').textContent, /could not inspect every native folder entry/);
  await document.getElementById('open-current-review').emit('click');
  assert.equal(reviewMutations, 0);
});

for (const scenario of [
  { name: 'finds a tracked edit made after the last scan', inspectionFails: false },
  { name: 'cannot complete the native inspection', inspectionFails: true },
]) {
  test(`review recording refuses when its fresh scope check ${scenario.name}`, async () => {
    const document = fakeDocument();
    const operation = '21'.repeat(32);
    const workspace = {
      root: '/managed/review-scope-check',
      digest: 'workspace-review-scope-check',
      installation: 'installation-review-scope-check',
      records: 2,
      reviews: 0,
      review_items_not_listed: 0,
      review_items: [{
        bundle: '81'.repeat(32),
        subject_operation: operation,
        reviewed_head: '82'.repeat(32),
        opened_by: null,
        author: '83'.repeat(32),
        recorded: false,
        actor_sequence: '2',
        subject_operations: [{ kind: 'WriteFileVersion', canonical_hex: 'a1' }],
        subject_operations_not_listed: 0,
        presentation_digest: '84'.repeat(32),
        bundle_changes: [],
        bundle_changes_not_listed: 0,
        content_complete: true,
        unavailable_code: null,
        projection_authorizes_approval: false,
      }],
      private_version: { version: 'private-scope-check', concurrent_changes: 1 },
      shared_version: null,
      entries: [{ path: 'tracked.txt', type: 'file' }],
      conditions: [],
      not_yet: [],
      native_untracked_files: [],
      file_histories: [{
        path: 'tracked.txt',
        object_id: '85'.repeat(16),
        current: { version_id: '86'.repeat(32), manifest_id: '87'.repeat(32) },
        retained_versions: [{ version_id: '86'.repeat(32), manifest_id: '87'.repeat(32) }],
      }],
      workspace_versions: [{ operation, ordinal: 1, actor_sequence: '2' }],
    };
    let reviewMutations = 0;
    let inspections = 0;
    const invoke = async (command, parameters = {}) => {
      if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
      if (command === 'approval_credential_status') return JSON.stringify({ enrolled: false, available: true, unavailable_reason: null });
      if (command === 'managed_checkpoint_state') {
        return JSON.stringify({
          root: workspace.root,
          workspace_digest: workspace.digest,
          workspace_installation: workspace.installation,
          working: false,
        });
      }
      if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
      if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
      if (command === 'inspect_managed_file') {
        inspections += 1;
        if (scenario.inspectionFails) throw new Error('injected native read failure');
        return JSON.stringify({
          path: 'tracked.txt',
          text: 'changed after the periodic scan\n',
          text_editable: true,
          native_untracked: false,
          native_missing: false,
          modified_from_current_version: true,
          current_version: '86'.repeat(32),
          byte_count: 32,
          content_digest: '88'.repeat(32),
          executable: false,
          installation: workspace.installation,
        });
      }
      if (command === 'open_current_review') {
        reviewMutations += 1;
        throw new Error('unverified review scope reached native mutation');
      }
      throw new Error(`unexpected fresh-review-scope command: ${command}`);
    };

    globalThis.document = document;
    globalThis.window = { __TAURI__: { core: { invoke } } };
    globalThis.confirm = () => true;
    await import(`./app.js?fresh-review-scope-${scenario.inspectionFails}-${Date.now()}`);
    await waitFor(() => document.serviceState.state === 'ready');

    const record = document.getElementById('open-current-review');
    assert.equal(record.disabled, false);
    await record.emit('click');

    assert.equal(inspections, 1);
    assert.equal(reviewMutations, 0);
    if (scenario.inspectionFails) {
      assert.match(document.getElementById('notice').textContent, /could not finish checking the native folder/);
    } else {
      assert.equal(document.getElementById('open-current-review').disabled, true);
      assert.match(document.reviewPage.controls.recordReviewReason, /not included in this review/);
    }
  });
}

test('the Import source and destination drafts remain source-owned across delayed React commits', async () => {
  const document = fakeDocument();
  const previewSources = [];
  const destinationPicks = [];
  let releaseDestinationPick = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'pick_folder') {
      destinationPicks.push(command);
      return new Promise((resolve) => { releaseDestinationPick = resolve; });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      throw daemonRefusal('no-workspace-open', 'No workspace is open');
    }
    if (command === 'daemon_call' && parameters.method === 'folder.import.preview') {
      const request = JSON.parse(parameters.paramsJson);
      previewSources.push(request.source);
      return JSON.stringify(importPreview({ files: 1, directories: 0, bytes: 12, summary: 'draft-summary' }));
    }
    throw new Error(`unexpected import-draft command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?import-draft-continuity=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  let projection = null;
  let mountImportProjection = true;
  document.addEventListener('mesh:import-workbench-projection', (event) => {
    projection = event.detail;
    if (mountImportProjection) {
      document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-mounted', {
        detail: { generation: event.detail.generation },
      }));
    }
  });
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-available'));
  assert.equal(projection.import.phase, 'select');

  const reactDraft = '/Users/finance/React draft ';
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-intent', {
    detail: {
      generation: projection.generation,
      intent: { type: 'source-draft', path: reactDraft },
    },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-available'));
  assert.equal(projection.import.sourcePath, reactDraft);

  const updatedDraft = '/Users/finance/Updated React draft ';
  await document.emitImportWorkbenchIntent({ type: 'source-draft', path: updatedDraft }, projection.generation);
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-available'));
  assert.equal(projection.import.sourcePath, updatedDraft);

  const submittedDraft = '/Users/finance/Exact submitted draft ';
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-intent', {
    detail: {
      generation: projection.generation,
      intent: { type: 'source-draft', path: submittedDraft },
    },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-intent', {
    detail: {
      generation: projection.generation,
      intent: { type: 'preview-path', path: submittedDraft },
    },
  }));
  await waitFor(() => projection?.import.phase === 'review');
  assert.deepEqual(previewSources, [submittedDraft]);
  assert.equal(projection.import.sourcePath, submittedDraft);

  const retainedReviewGeneration = projection.generation;
  mountImportProjection = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-intent', {
    detail: {
      generation: retainedReviewGeneration,
      intent: { type: 'choose-destination' },
    },
  }));
  await waitFor(() => releaseDestinationPick !== null);
  assert.equal(projection.import.destinationPath, '');
  assert.equal(projection.import.canConfirm, false);
  const firstDelayedDraft = '/Users/finance/Private d';
  const newestDelayedDraft = '/Users/finance/Private draft ';
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-intent', {
    detail: {
      generation: retainedReviewGeneration,
      intent: { type: 'destination-draft', path: firstDelayedDraft },
    },
  }));
  await waitFor(() => projection?.import.destinationPath === firstDelayedDraft);
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-intent', {
    detail: {
      generation: retainedReviewGeneration,
      intent: { type: 'destination-draft', path: newestDelayedDraft },
    },
  }));
  await waitFor(() => projection?.import.destinationPath === newestDelayedDraft);
  releaseDestinationPick('/Users/finance/Obsolete parent');
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(projection.import.destinationPath, newestDelayedDraft);
  assert.equal(projection.import.canConfirm, true);

  mountImportProjection = true;
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-mounted', {
    detail: { generation: projection.generation },
  }));
  releaseDestinationPick = null;
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-intent', {
    detail: {
      generation: projection.generation,
      intent: { type: 'choose-destination' },
    },
  }));
  await waitFor(() => releaseDestinationPick !== null);
  assert.equal(projection.import.destinationPath, newestDelayedDraft);
  assert.equal(projection.import.canConfirm, false);
  releaseDestinationPick(null);
  await waitFor(() => projection?.import.canConfirm === true);
  assert.equal(projection.import.destinationPath, newestDelayedDraft);
  assert.equal(destinationPicks.length, 2);
});

test('a lost first import reply recovers one app-managed workspace with committed navigation', async () => {
  const document = fakeDocument();
  const source = '/ordinary/original-project';
  const destination = '/managed/original-project.mesh';
  const workspace = {
    root: `${destination}/mounts`,
    digest: 'workspace-imported',
    installation: 'installation-imported',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-imported', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'README.md', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    workspace_versions: [{ operation: '66'.repeat(32), ordinal: 1, actor_sequence: '1' }],
    file_histories: [{
      object_id: 'object-readme',
      path: 'README.md',
      current: { version_id: 'version-imported', manifest_id: 'manifest-imported' },
      retained_versions: [{ version_id: 'version-imported', manifest_id: 'manifest-imported' }],
    }],
  };
  let importParameters = null;
  let importCalls = 0;
  let createdWorkspaces = 0;
  let revealParameters = null;
  let workspaceOpened = false;
  const codexParameters = [];
  let releaseCodex = null;
  let releaseImportPreview = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false, export_root: null });
    if (command === 'pick_folder') return source;
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      if (workspaceOpened) return JSON.stringify(workspace);
      throw daemonRefusal('no-workspace-open', 'No workspace is open');
    }
    if (command === 'daemon_call' && parameters.method === 'folder.import.preview') {
      await new Promise((resolve) => { releaseImportPreview = resolve; });
      return JSON.stringify(importPreview({ files: 1, directories: 0, bytes: 12, summary: 'import-summary' }));
    }
    if (command === 'import_managed_workspace') {
      importCalls += 1;
      importParameters = parameters;
      if (!workspaceOpened) {
        workspaceOpened = true;
        createdWorkspaces += 1;
      }
      if (importCalls === 1) {
        throw new Error('the native import reply was lost after its workspace and navigation committed');
      }
      return JSON.stringify({
        workspace,
        materialized_entries: 1,
        recovered_after_interruption: true,
        navigation: {
          remembered: workspace.root,
          workspaces: [workspace.root],
          auto_opened: false,
          active_folder: '/application/native-workspace/current',
          export_root: source,
          warning: null,
        },
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'reveal_managed_workspace') {
      revealParameters = parameters;
      return JSON.stringify({
        path: '/application/native-workspace/current',
        workspace_root: workspace.root,
        stable: true,
        native_folder: true,
      });
    }
    if (command === 'reconcile_managed_workspace_navigation') {
      return JSON.stringify({
        path: '/application/native-workspace/current',
        workspace_root: workspace.root,
        stable: true,
        native_folder: true,
      });
    }
    if (command === 'open_managed_workspace_in_codex') {
      codexParameters.push(parameters);
      await new Promise((resolve) => {
        releaseCodex = resolve;
      });
      return JSON.stringify({
        path: workspace.root,
        workspace_installation: workspace.installation,
        fixed_workspace_path: true,
        agent_handoff_recorded: true,
        agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
        agent: 'Codex',
        mesh_context: 'ready',
      });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?import-export-root=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  let importProjection = null;
  const importHost = document.getElementById('import-workbench-next');
  let importPageActive = false;
  const previewOwner = {
    isConnected: true,
    focus() {
      if (!importPageActive) return;
      importHost.shadowRoot.activeElement = previewOwner;
    },
  };
  importHost.shadowRoot = {
    activeElement: null,
    querySelector(selector) {
      return selector === '[data-mesh-import-choose]' ? previewOwner : null;
    },
  };
  document.getElementById('mesh-app-next').setAttribute('data-mesh-react-shell-active', 'true');
  let crossIslandFocusGeneration = null;
  document.addEventListener('mesh:workspace-page-request', (event) => {
    if (event.detail?.page !== 'import' || event.detail.selector !== '[data-mesh-import-choose]') return;
    setTimeout(() => {
      importPageActive = true;
      previewOwner.focus();
      document.dispatchEvent(new FakeCustomEvent('mesh:react-shell-committed'));
    }, 10);
  });
  document.addEventListener('mesh:import-workbench-external-focus', (event) => {
    crossIslandFocusGeneration = event.detail?.generation ?? null;
  });
  const toggleImportVisibility = importHost.classList.toggle.bind(importHost.classList);
  importHost.classList.toggle = (value, force) => {
    const result = toggleImportVisibility(value, force);
    if (value === 'hidden' && result) importHost.shadowRoot.activeElement = null;
    return result;
  };
  let reviewProjectionRetainedFocus = false;
  let mountImportProjection = true;
  document.addEventListener('mesh:import-workbench-projection', (event) => {
    importProjection = event.detail;
    if (event.detail.import.phase === 'review') {
      reviewProjectionRetainedFocus = importHost.shadowRoot.activeElement === previewOwner;
    }
    if (mountImportProjection) {
      document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-mounted', {
        detail: { generation: event.detail.generation },
      }));
    }
  });
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-available'));
  assert.equal(importProjection.import.phase, 'select');
  assert.equal(document.getElementById('import-workbench-next').classList.contains('hidden'), false);
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-available'));
  assert.equal(
    importProjection.import.sourcePath,
    '',
    'a repeated availability request invented an Import source draft',
  );
  const committedImportGeneration = importProjection.generation;
  mountImportProjection = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-available'));
  assert.ok(importProjection.generation > committedImportGeneration);
  assert.equal(document.getElementById('import-workbench-next').classList.contains('hidden'), true);
  const rejectedImportGeneration = importProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-rejected', {
    detail: { generation: rejectedImportGeneration, reason: 'synthetic render failure' },
  }));
  assert.equal(
    document.getElementById('import-workbench-next').classList.contains('hidden'),
    true,
    'a rejected first Import commit exposed a blank React host instead of its page fallback',
  );
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-available'));
  assert.ok(importProjection.generation > rejectedImportGeneration);
  mountImportProjection = true;
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-mounted', {
    detail: { generation: importProjection.generation },
  }));
  assert.equal(document.getElementById('import-workbench-next').classList.contains('hidden'), false);

  assert.equal(document.getElementById('workspace-entry-controls').open, false);
  assert.equal(document.getElementById('workspace-entry-summary').textContent, 'Already use Mesh? Open a managed workspace');
  assert.match(document.getElementById('hero-lede').textContent, /Bring in an ordinary project folder/);
  assert.doesNotMatch(document.getElementById('hero-lede').textContent, /Choose an existing folder/);

  const seededImportGeneration = importProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-intent', {
    detail: {
      generation: importProjection.generation - 1,
      intent: { type: 'choose-folder' },
    },
  }));
  assert.equal(releaseImportPreview, null, 'a stale import generation started native preview work');
  importHost.shadowRoot.activeElement = null;
  await document.emitWorkspaceEntryIntent({ type: 'choose-folder' });
  await waitFor(() => releaseImportPreview !== null);
  assert.equal(
    crossIslandFocusGeneration,
    seededImportGeneration,
    'the Entry action did not seed the exact mounted Import generation before selecting a source',
  );
  assert.equal(
    importHost.shadowRoot.activeElement,
    previewOwner,
    'publishing the exact in-flight import interaction hid its owner before native preview completed',
  );
  releaseImportPreview();
  await waitFor(() => importProjection?.import.phase === 'review');
  assert.equal(
    reviewProjectionRetainedFocus,
    true,
    'the exact review projection hid the initiating control before React could authorize heading focus',
  );
  assert.equal(importProjection.import.sourcePath, source);
  assert.equal(importProjection.import.fileCount, '1');
  assert.deepEqual(importProjection.import.files, ['preview-00.txt · 12 bytes']);
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-available'));
  assert.equal(importProjection.import.fileCount, '1');
  assert.equal(importProjection.import.folderCount, '0');
  assert.equal(importProjection.import.byteCount, '12');
  assert.equal(importProjection.import.summary, 'import-summary');
  assert.match(importProjection.import.scopeNote, /files Mesh will bring into the native working folder/);
  assert.deepEqual(importProjection.import.files, ['preview-00.txt · 12 bytes']);
  assert.equal(importProjection.import.confirmLabel, 'Create workspace and open folder');
  assert.equal(importProjection.import.canConfirm, true);
  assert.equal(importProjection.import.destinationPath, '');
  assert.equal(importProjection.import.confirmLabel, 'Create workspace and open folder');
  assert.equal(importProjection.import.canConfirm, true);
  const importGeneration = importProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-intent', {
    detail: {
      generation: importGeneration,
      intent: { type: 'confirm-import', summary: 'forged' },
    },
  }));
  assert.equal(importParameters, null, 'an extended React import intent crossed the coordinator boundary');
  document.dispatchEvent(new FakeCustomEvent('mesh:import-workbench-intent', {
    detail: {
      generation: importProjection.generation,
      intent: { type: 'confirm-import' },
    },
  }));
  await waitFor(() => revealParameters !== null);

  assert.equal(importCalls, 2, 'the renderer did not make exactly one bounded recovery attempt');
  assert.equal(createdWorkspaces, 1, 'the lost reply created a duplicate managed workspace');
  assert.deepEqual(importParameters, {
    source,
    summary: 'import-summary',
    destination: null,
  });
  assert.deepEqual(revealParameters, {
    expectedWorkspaceRoot: workspace.root,
    expectedWorkspaceDigest: workspace.digest,
    expectedWorkspaceInstallation: workspace.installation,
  });
  assert.equal(document.destinationField('destination').value, source);
  assert.equal(importProjection.import.phase, 'select');
  assert.equal(importProjection.import.canConfirm, false);
  assert.equal(importProjection.import.confirmLabel, 'Create workspace and open folder');
  assert.equal(importProjection.import.sourcePath, '');
  assert.match(document.destinationHint.textContent, /Remembered destination folder/);
  assert.match(document.getElementById('notice').textContent, /original is unchanged/i);
  assert.match(document.getElementById('notice').textContent, /native working folder is ready/i);
  assert.match(document.getElementById('notice').textContent, /lost the first import reply/i);
  assert.match(document.getElementById('notice').textContent, /recovered the exact completed workspace/i);
  assert.match(document.getElementById('notice').textContent, /no duplicate was created/i);
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Start Codex on this saved version');
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Start Codex on this version');
  assert.equal(document.workspaceOverview?.overview.nextActionDisabled, false);
  const launch = document.emitWorkspaceOverviewIntent('recommended');
  await waitFor(() => codexParameters.length === 1);
  assert.equal(document.workspaceOverview?.overview.nextActionDisabled, true);
  await document.emitWorkspaceCurrentIntent('start-codex');
  assert.equal(codexParameters.length, 1, 'a second Codex launch raced the first handoff');
  releaseCodex();
  await launch;
  await waitFor(() => document.workspaceOverview?.overview.nextActionTitle === 'Agent folder is assigned');
  assert.deepEqual(codexParameters, [{
    expectedWorkspaceRoot: workspace.root,
    expectedWorkspaceDigest: workspace.digest,
    expectedWorkspaceInstallation: workspace.installation,
    confirmedReopen: false,
    expectedAgentHandoffGeneration: null,
  }]);
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Agent folder is assigned');
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Finish agent handoff');
  assert.match(document.getElementById('notice').textContent, /fixed real folder/i);
  assert.match(document.getElementById('notice').textContent, /metadata tool pauses/);
  assert.equal((document.workspaceCurrentAction('start-codex')?.label ?? ''), 'Reopen assigned Codex folder');
  let repeatWarning = null;
  globalThis.confirm = (message) => {
    repeatWarning = message;
    return false;
  };
  await document.emitWorkspaceCurrentIntent('start-codex');
  assert.equal(codexParameters.length, 1, 'a refused reopen launched a second agent in the same folder');
  assert.match(repeatWarning, /Two agents in one folder can overwrite each other/);
  assert.match(document.getElementById('notice').textContent, /Start another agent/);
  globalThis.confirm = () => true;
  const reopen = document.emitWorkspaceCurrentIntent('start-codex');
  await waitFor(() => codexParameters.length === 2);
  releaseCodex();
  await reopen;
  assert.match(document.getElementById('notice').textContent, /^Reopened /);

  workspace.digest = 'workspace-imported-replaced';
  workspace.installation = 'installation-imported-replaced';
  await document.emitWorkspaceCurrentIntent('refresh');
  assert.equal(
    document.workspaceOverview?.overview.nextActionTitle,
    'Start Codex on this saved version',
    'a successful handoff for an older physical installation was reused for its replacement',
  );
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Start Codex on this version');
});

test('restart restores the exact agent-folder collision warning from native history', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/restarted-agent.mesh/mounts',
    digest: 'workspace-restarted-agent',
    installation: 'installation-restarted-agent',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-restarted-agent', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'README.md', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    workspace_versions: [{ operation: '67'.repeat(32), ordinal: 1, actor_sequence: '1' }],
    file_histories: [],
  };
  let launcherCalls = 0;
  let warning = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: workspace.root,
        workspaces: [workspace.root],
        workspace_entries: [{
          path: workspace.root,
          export_root: null,
          project_root: null,
          agent_handoff_installation: workspace.installation,
          agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
        }],
        auto_opened: true,
        active_folder: '/application/native-workspace/current',
        export_root: null,
        warning: null,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: true,
        native_folder_path: workspace.root,
        working: false,
      });
    }
    if (command === 'reconcile_managed_workspace_navigation') {
      return JSON.stringify({
        path: '/application/native-workspace/current',
        workspace_root: workspace.root,
        stable: true,
      });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'open_managed_workspace_in_codex') {
      launcherCalls += 1;
      throw new Error('a restart must warn before launching this exact folder again');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = (message) => {
    warning = message;
    return false;
  };
  await import(`./app.js?durable-agent-handoff=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.equal((document.workspaceCurrentAction('start-codex')?.label ?? ''), 'Reopen assigned Codex folder');
  await document.emitWorkspaceCurrentIntent('start-codex');
  assert.equal(launcherCalls, 0);
  assert.match(warning, /Two agents in one folder can overwrite each other/);
  assert.match(document.getElementById('notice').textContent, /Start another agent/);
});

test('Refresh learns that another Mesh process assigned this exact agent folder', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/external-agent.mesh/mounts',
    digest: 'workspace-external-agent',
    installation: 'installation-external-agent',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-external-agent', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'README.md', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_inventory_complete: true,
    workspace_versions: [{ operation: '68'.repeat(32), ordinal: 1, actor_sequence: '1' }],
    file_histories: [],
  };
  let durableHandoff = null;
  const launches = [];
  let warning = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: workspace.root,
        workspaces: [workspace.root],
        workspace_entries: [{
          path: workspace.root,
          export_root: null,
          project_root: null,
          agent_handoff_installation: durableHandoff,
          agent_handoff_generation: durableHandoff ? TEST_AGENT_HANDOFF_GENERATION : null,
        }],
        auto_opened: true,
        active_folder: '/application/native-workspace/current',
        export_root: null,
        warning: null,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: true,
        native_folder_path: workspace.root,
        working: false,
      });
    }
    if (command === 'reconcile_managed_workspace_navigation') {
      return JSON.stringify({
        path: '/application/native-workspace/current',
        workspace_root: workspace.root,
        stable: true,
      });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'open_managed_workspace_in_codex') {
      launches.push(parameters);
      return JSON.stringify({
        path: workspace.root,
        workspace_installation: workspace.installation,
        fixed_workspace_path: true,
        agent_handoff_recorded: true,
        agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
        agent: 'Codex',
      });
    }
    throw new Error(`unexpected external-agent command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?external-agent-handoff=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.equal((document.workspaceCurrentAction('start-codex')?.label ?? ''), 'Start Codex on this version');
  durableHandoff = workspace.installation;
  await document.emitWorkspaceCurrentIntent('refresh');
  assert.equal((document.workspaceCurrentAction('start-codex')?.label ?? ''), 'Reopen assigned Codex folder');
  assert.equal(Boolean(document.workspaceCurrentAction('finish-agent')), true);
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Agent folder is assigned');

  globalThis.confirm = (message) => {
    warning = message;
    return false;
  };
  await document.emitWorkspaceCurrentIntent('start-codex');
  assert.equal(launches.length, 0, 'a stale process launched into an externally assigned folder');
  assert.match(warning, /Two agents in one folder can overwrite each other/);

  globalThis.confirm = () => true;
  await document.emitWorkspaceCurrentIntent('start-codex');
  assert.equal(launches.length, 1);
  assert.deepEqual(launches[0], {
    expectedWorkspaceRoot: workspace.root,
    expectedWorkspaceDigest: workspace.digest,
    expectedWorkspaceInstallation: workspace.installation,
    confirmedReopen: true,
    expectedAgentHandoffGeneration: TEST_AGENT_HANDOFF_GENERATION,
  });
});

test('Refresh replaces a stale recent-workspace list from another Mesh process', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/current-refresh-list.mesh/mounts',
    digest: 'workspace-current-refresh-list',
    installation: 'installation-current-refresh-list',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'version-current-refresh-list', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_inventory_complete: true,
    workspace_versions: [],
    file_histories: [],
  };
  const added = '/managed/added-by-other-process.mesh/mounts';
  let externalNavigationChanged = false;
  const navigation = () => {
    const paths = externalNavigationChanged ? [added, workspace.root] : [workspace.root];
    return {
      remembered: paths[0],
      workspaces: paths,
      workspace_entries: paths.map((path) => ({
        path,
        export_root: null,
        project_root: null,
        agent_handoff_installation: null,
        agent_handoff_generation: null,
      })),
      auto_opened: true,
      active_folder: null,
      export_root: null,
      warning: null,
    };
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify(navigation());
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: true,
        native_folder_path: workspace.root,
        working: false,
      });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    throw new Error(`unexpected recent-list-refresh command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?external-recent-list=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  assert.equal(document.getElementById('recent-workspace').children.length, 1);

  externalNavigationChanged = true;
  await document.emitWorkspaceCurrentIntent('refresh');

  const choices = document.getElementById('recent-workspace').children;
  assert.equal(choices.length, 2);
  assert.equal(choices.some((choice) => choice.value === added), true);
  document.getElementById('recent-workspace').value = added;
  await document.getElementById('recent-workspace').emit('change');
  assert.equal(
    document.getElementById('open-recent-workspace').disabled,
    false,
    'the externally remembered workspace stayed unavailable after explicit Refresh',
  );
});

test('macOS exposes the native chooser while retaining the typed-path fallback', async () => {
  const document = fakeDocument();
  const nativeCalls = [];
  let folderPickCount = 0;
  let workspaceOpened = false;
  const source = '/Users/person/Alpha\\source ';
  const destinationParent = '/Users/person/Chosen\\parent';
  const generatedDestination = `${destinationParent}/Alpha\\source -mesh-alpha-su`;
  const managed = '/Users/person/Alpha managed ';
  const workspace = {
    root: managed,
    digest: 'workspace-alpha',
    installation: 'installation-alpha',
    records: 1,
    private_version: null,
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
  };
  const invoke = async (command, parameters = {}) => {
    nativeCalls.push(command);
    if (command === 'pick_folder') {
      folderPickCount += 1;
      return folderPickCount === 1 ? source : destinationParent;
    }
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'remember_managed_workspace') return JSON.stringify({ root: parameters.path });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: managed,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      if (workspaceOpened) return JSON.stringify(workspace);
      throw daemonRefusal('no-workspace-open', 'No workspace is open');
    }
    if (command === 'daemon_call' && parameters.method === 'folder.import.preview') {
      assert.deepEqual(JSON.parse(parameters.paramsJson), { source });
      return JSON.stringify(importPreview({ files: 2, directories: 1, bytes: 20, summary: 'alpha-summary' }));
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      assert.deepEqual(JSON.parse(parameters.paramsJson), { path: managed });
      workspaceOpened = true;
      return JSON.stringify(workspace);
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  const priorNavigator = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
  Object.defineProperty(globalThis, 'navigator', {
    configurable: true,
    value: { userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' },
  });
  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  try {
    await import(`./app.js?mac-typed-path=${Date.now()}`);
    await waitFor(() => document.serviceState.state === 'ready');
    assert.equal(document.workspaceEntry.entry.openPath, '', 'a new renderer replayed an earlier managed path');

    await document.emitWorkspaceEntryIntent({ type: 'choose-folder' });
    assert.equal(
      nativeCalls.filter((command) => command === 'pick_folder').length,
      1,
      'the primary first-run action should open the native chooser',
    );
    assert.equal(document.importWorkbench?.import.sourcePath, source);
    assert.equal(document.importWorkbench?.import.fileCount, '2');
    assert.equal(document.importWorkbench?.import.canChooseDestination, true);
    await document.emitImportWorkbenchIntent({ type: 'choose-destination' });
    assert.equal(
      document.importWorkbench?.import.destinationPath,
      generatedDestination,
      'the chosen macOS folder must remain the exact parent when its name contains a backslash',
    );

    // The typed path remains a first-class fallback if the chooser is unavailable or the person
    // already has an exact path to paste.
    await document.emitImportWorkbenchIntent({ type: 'source-draft', path: source });
    await document.emitImportWorkbenchIntent({ type: 'preview-path', path: source });

    await document.emitImportWorkbenchIntent({ type: 'destination-draft', path: managed });
    assert.equal(document.importWorkbench?.import.destinationPath, managed);
    assert.equal(document.importWorkbench?.import.canConfirm, true);

    const entryGeneration = document.workspaceEntry.generation;
    await document.emitWorkspaceEntryIntent({ type: 'update-managed-path', path: managed }, entryGeneration);
    await document.emitWorkspaceEntryIntent({ type: 'open-managed-path', path: managed }, entryGeneration);
    await waitFor(() => (document.workspaceCurrent?.current?.agentFolder ?? '') === managed);
    assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), managed);
    assert.equal(document.serviceState.state === 'ready', true);
    assert.equal(nativeCalls.filter((command) => command === 'pick_folder').length, 2);
  } finally {
    if (priorNavigator) Object.defineProperty(globalThis, 'navigator', priorNavigator);
    else delete globalThis.navigator;
  }
});

test('a delayed startup status read cannot invalidate a workspace opened afterward', async () => {
  const document = fakeDocument();
  let releaseRecent;
  const recent = new Promise((resolve) => {
    releaseRecent = resolve;
  });
  let workspaceStateReads = 0;
  const workspace = {
    root: '/managed/latest',
    digest: 'state-latest',
    installation: 'state-latest',
    records: 1,
    private_version: { version: null },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return recent;
    if (command === 'pick_folder') return workspace.root;
    if (command === 'remember_managed_workspace') {
      return JSON.stringify({ auto_opened: true, root: workspace.root });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      return JSON.stringify(workspace);
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      workspaceStateReads += 1;
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?startup-open-race=${Date.now()}`);

  await waitFor(() => document.workspaceEntry?.entry.canChooseManaged === true);
  await document.emitWorkspaceEntryIntent({ type: 'choose-managed-folder' });
  await waitFor(() => (document.workspaceCurrent?.current?.agentFolder ?? '') === workspace.root);
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), workspace.root);
  assert.equal(document.getElementById('manage-path').disabled, false);

  releaseRecent(JSON.stringify({ auto_opened: false }));
  for (let turn = 0; turn < 5; turn += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }

  assert.equal(
    workspaceStateReads,
    1,
    'the older startup continuation performed another read after the explicit open inspection',
  );
  assert.equal(document.getElementById('manage-path').disabled, false);
  assert.equal(document.serviceState.state === 'ready', true);
});

test('an older folder preview cannot replace or discredit the latest selected source', async (context) => {
  for (const olderOutcome of ['success', 'failure']) {
    await context.test(olderOutcome, async () => {
      const document = fakeDocument();
      let pickCount = 0;
      let previewCount = 0;
      let releaseOlder;
      let rejectOlder;
      const older = new Promise((resolve, reject) => {
        releaseOlder = resolve;
        rejectOlder = reject;
      });
      const invoke = async (command, parameters = {}) => {
        if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
        if (command === 'pick_folder') {
          pickCount += 1;
          return pickCount === 1 ? '/source/older' : '/source/latest';
        }
        if (command === 'daemon_call' && parameters.method === 'workspace.state') {
          throw daemonRefusal('no-workspace-open', 'No workspace is open');
        }
        if (command === 'daemon_call' && parameters.method === 'folder.import.preview') {
          previewCount += 1;
          if (previewCount === 1) return older;
          return JSON.stringify(importPreview({ files: 2, directories: 1, bytes: 20, summary: 'latest-summary' }));
        }
        throw new Error(`unexpected native command: ${command}`);
      };

      globalThis.document = document;
      globalThis.window = { __TAURI__: { core: { invoke } } };
      globalThis.confirm = () => true;
      await import(`./app.js?source-preview-race=${olderOutcome}-${Date.now()}`);
      await waitFor(() => document.serviceState.state === 'ready');

      await document.emitWorkspaceEntryIntent({ type: 'choose-folder' });
      await waitFor(() => previewCount === 1);
      await document.emitWorkspaceEntryIntent({ type: 'choose-folder' });
      await waitFor(() => document.importWorkbench?.import.sourcePath === '/source/latest');
      assert.equal(document.importWorkbench?.import.sourcePath, '/source/latest');
      assert.equal(document.importWorkbench?.import.fileCount, '2');

      if (olderOutcome === 'success') {
        releaseOlder(JSON.stringify(importPreview({ files: 99, directories: 9, bytes: 999, summary: 'older-summary' })));
      } else {
        rejectOlder(new Error('older preview failed'));
      }
      await new Promise((resolve) => setTimeout(resolve, 0));

      assert.equal(
        document.importWorkbench?.import.sourcePath,
        '/source/latest',
        'a late preview paired the latest source path with an older source result',
      );
      assert.equal(document.importWorkbench?.import.fileCount, '2');
      assert.doesNotMatch(document.getElementById('notice').textContent, /older preview failed/);
    });
  }
});

test('starting another import selection immediately revokes the prior confirmation', async (context) => {
  for (const selecting of ['source', 'destination']) {
    await context.test(selecting, async () => {
      const document = fakeDocument();
      let pickCount = 0;
      let releaseSelection;
      const pendingSelection = new Promise((resolve) => {
        releaseSelection = resolve;
      });
      const invoke = async (command, parameters = {}) => {
        if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
        if (command === 'pick_folder') {
          pickCount += 1;
          if (pickCount === 1) return '/source/original';
          if (pickCount === 2) return '/destination/original';
          return pendingSelection;
        }
        if (command === 'daemon_call' && parameters.method === 'workspace.state') {
          throw daemonRefusal('no-workspace-open', 'No workspace is open');
        }
        if (command === 'daemon_call' && parameters.method === 'folder.import.preview') {
          return JSON.stringify(importPreview({ files: 2, directories: 1, bytes: 20, summary: 'source-summary' }));
        }
        throw new Error(`unexpected native command: ${command}`);
      };

      globalThis.document = document;
      globalThis.window = { __TAURI__: { core: { invoke } } };
      globalThis.confirm = () => true;
      await import(`./app.js?selection-start=${selecting}-${Date.now()}`);
      await waitFor(() => document.serviceState.state === 'ready');

      await document.emitWorkspaceEntryIntent({ type: 'choose-folder' });
      await document.emitImportWorkbenchIntent({ type: 'choose-destination' });
      assert.equal(document.importWorkbench?.import.canConfirm, true);

      const choice = selecting === 'source'
        ? document.emitWorkspaceEntryIntent({ type: 'choose-folder' })
        : document.emitImportWorkbenchIntent({ type: 'choose-destination' });
      await waitFor(() => pickCount === 3);
      assert.equal(
        document.importWorkbench?.import.canConfirm,
        false,
        `the previous import stayed actionable while a new ${selecting} picker was unresolved`,
      );

      releaseSelection(selecting === 'source' ? '/source/replacement' : '/destination/replacement');
      await choice;
    });
  }
});

test('an older managed-workspace picker cannot replace the latest selected workspace', async () => {
  const document = fakeDocument();
  let pickCount = 0;
  let currentRoot = null;
  let releaseOlder;
  let releaseLatest;
  let releaseSupersededByPath;
  const older = new Promise((resolve) => { releaseOlder = resolve; });
  const latest = new Promise((resolve) => { releaseLatest = resolve; });
  const supersededByPath = new Promise((resolve) => { releaseSupersededByPath = resolve; });
  const workspace = (root) => ({
    root,
    digest: `state-${root}`,
    installation: `state-${root}`,
    records: 1,
    private_version: null,
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
  });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'approval_credential_status') return JSON.stringify({ enrolled: false, available: true, unavailable_reason: null });
    if (command === 'pick_folder') {
      pickCount += 1;
      return pickCount === 1 ? older : pickCount === 2 ? latest : supersededByPath;
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: currentRoot, workspace_digest: `state-${currentRoot}`, workspace_installation: `state-${currentRoot}`, working: false });
    }
    if (command === 'remember_managed_workspace') return JSON.stringify({ path: parameters.path });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      throw daemonRefusal('no-workspace-open', 'No workspace is open');
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      currentRoot = parameters.paramsJson ? JSON.parse(parameters.paramsJson).path : parameters.params.path;
      return JSON.stringify(workspace(currentRoot));
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?open-picker-race=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  document.emitWorkspaceEntryIntent({ type: 'choose-managed-folder' });
  await waitFor(() => pickCount === 1);
  document.emitWorkspaceEntryIntent({ type: 'choose-managed-folder' });
  await waitFor(() => pickCount === 2);

  releaseLatest('/managed/latest');
  await waitFor(() => (document.workspaceCurrent?.current?.agentFolder ?? '') === '/managed/latest');
  await waitFor(() => document.workspaceEntry.entry.canChooseManaged === true);
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), '/managed/latest');

  releaseOlder('/managed/older');
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(
    (document.workspaceCurrent?.current?.agentFolder ?? ''),
    '/managed/latest',
    'an older picker result replaced the user\'s newer workspace selection',
  );

  document.emitWorkspaceEntryIntent({ type: 'choose-managed-folder' });
  await waitFor(() => pickCount === 3);
  const entryGeneration = document.workspaceEntry.generation;
  await document.emitWorkspaceEntryIntent(
    { type: 'update-managed-path', path: '/managed/typed-latest' },
    entryGeneration,
  );
  await document.emitWorkspaceEntryIntent(
    { type: 'open-managed-path', path: '/managed/typed-latest' },
    entryGeneration,
  );
  await waitFor(() => (document.workspaceCurrent?.current?.agentFolder ?? '') === '/managed/typed-latest');
  await waitFor(() => document.workspaceEntry.entry.canChooseManaged === true);

  releaseSupersededByPath('/managed/stale-picker');
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(
    (document.workspaceCurrent?.current?.agentFolder ?? ''),
    '/managed/typed-latest',
    'an older picker result replaced the newer source-owned managed-path selection',
  );

});

test('a managed-workspace chooser cannot survive Refresh into another physical workspace', async () => {
  const document = fakeDocument();
  const workspace = (root, generation) => ({
    root,
    digest: `digest-${generation}`,
    installation: `installation-${generation}`,
    records: 1,
    private_version: null,
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_inventory_complete: true,
    file_histories: [],
    workspace_versions: [],
  });
  let current = workspace('/managed/original', 'original');
  let releasePicker;
  const picker = new Promise((resolve) => { releasePicker = resolve; });
  let pickerCalls = 0;
  const openedPaths = [];
  const navigation = () => ({
    auto_opened: false,
    remembered: current.root,
    workspaces: [current.root],
    workspace_entries: [{ path: current.root, export_root: null, project_root: null }],
    active_folder: null,
    export_root: null,
  });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify(navigation());
    if (command === 'pick_folder') {
      pickerCalls += 1;
      return picker;
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: current.root,
        workspace_digest: current.digest,
        workspace_installation: current.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(current);
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      const path = JSON.parse(parameters.paramsJson).path;
      openedPaths.push(path);
      current = workspace(path, 'stale-picker');
      return JSON.stringify(current);
    }
    if (command === 'remember_managed_workspace') return JSON.stringify(navigation());
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    throw new Error(`unexpected chooser continuity command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?open-picker-refresh-race=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready' && document.workspaceEntry?.entry.canChooseManaged);

  document.emitWorkspaceEntryIntent({ type: 'choose-managed-folder' });
  await waitFor(() => pickerCalls === 1);
  current = workspace('/managed/refreshed-elsewhere', 'replacement');
  await document.emitWorkspaceCurrentIntent('refresh');
  await waitFor(() => (document.workspaceCurrent?.current?.agentFolder ?? '') === current.root);

  releasePicker('/managed/stale-after-refresh');
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(
    (document.workspaceCurrent?.current?.agentFolder ?? ''),
    '/managed/refreshed-elsewhere',
    'a chooser opened for an earlier physical workspace survived Refresh and replaced its successor',
  );
  assert.deepEqual(openedPaths, [], 'a stale chooser result reached workspace.open after continuity changed');
  assert.match(document.getElementById('notice').textContent, /workspace changed while Mesh was choosing/i);
});

test('a source-folder chooser cannot preview into a replacement workspace after Refresh', async () => {
  const document = fakeDocument();
  const workspace = (generation) => ({
    root: `/managed/source-${generation}`,
    digest: `digest-source-${generation}`,
    installation: `installation-source-${generation}`,
    records: 1,
    private_version: null,
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_inventory_complete: true,
    file_histories: [],
    workspace_versions: [],
  });
  let current = workspace('original');
  let releasePicker;
  const picker = new Promise((resolve) => { releasePicker = resolve; });
  let pickerCalls = 0;
  let previewCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({ auto_opened: true, remembered: current.root, workspaces: [current.root] });
    }
    if (command === 'pick_folder') {
      pickerCalls += 1;
      return picker;
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: current.root,
        workspace_digest: current.digest,
        workspace_installation: current.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(current);
    if (command === 'daemon_call' && parameters.method === 'folder.import.preview') {
      previewCalls += 1;
      return JSON.stringify(importPreview({ files: 1, directories: 0, bytes: 1, summary: 'stale-source' }));
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    throw new Error(`unexpected source chooser continuity command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?source-picker-refresh-race=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready' && document.workspaceEntry?.entry.canChoose);

  document.emitWorkspaceEntryIntent({ type: 'choose-folder' });
  await waitFor(() => pickerCalls === 1);
  current = workspace('replacement');
  await document.emitWorkspaceCurrentIntent('refresh');
  await waitFor(() => (document.workspaceCurrent?.current?.agentFolder ?? '') === current.root);
  releasePicker('/ordinary/stale-source');
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(previewCalls, 0, 'a source selected for the prior physical workspace reached preview');
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), '/managed/source-replacement');
  assert.equal(document.workspaceEntry.entry.mode, 'ready');
});

test('checkpoint state from a same-path replacement cannot verify an older workspace summary', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/replaced-in-place',
    digest: 'workspace-generation-a',
    installation: 'workspace-generation-a',
    records: 1,
    private_version: null,
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: 'workspace-generation-b',
        workspace_installation: 'workspace-generation-b',
        working: false,
      });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?same-path-replacement=${Date.now()}`);
  await waitFor(() => document.getElementById('notice').textContent.includes(
    'Mesh changed the open workspace while its checkpoint state was being verified.',
  ));

  assert.equal(
    document.serviceState.state === 'ready',
    false,
    'checkpoint state from replacement B certified workspace summary A at the same path',
  );
  assert.equal(document.getElementById('workspace-files-next').classList.contains('hidden'), true);
});

test('an unverified multi-version Overview cannot offer or run the saved-version shortcut', async () => {
  const document = fakeDocument();
  const currentOperation = '41'.repeat(32);
  const earlierOperation = '42'.repeat(32);
  const workspace = {
    root: '/managed/unverified-versions',
    digest: 'workspace-unverified-versions',
    installation: 'installation-unverified-versions',
    records: 2,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: currentOperation, state: 'working', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'notes.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    workspace_versions: [
      { operation: currentOperation, ordinal: 2, actor_sequence: '2' },
      { operation: earlierOperation, ordinal: 1, actor_sequence: '1' },
    ],
  };
  let checkpointMatches = true;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({ auto_opened: true, remembered: workspace.root, workspaces: [workspace.root] });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: checkpointMatches ? workspace.digest : 'replacement-digest',
        workspace_installation: checkpointMatches ? workspace.installation : 'replacement-installation',
        native_folder: false,
        native_folder_path: null,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'approval_credential_status') {
      return JSON.stringify({ enrolled: false, available: true, unavailable_reason: null });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    throw new Error(`unexpected unverified-version command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?unverified-overview-versions=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  await waitFor(() => document.workspaceOverview?.overview.canOpenAnotherVersion === true);

  checkpointMatches = false;
  await document.emitWorkspaceCurrentIntent('refresh');
  await waitFor(() => document.workspaceOverview?.overview.canOpenAnotherVersion === false);
  const selection = document.getElementById('workspace-version');
  selection.value = currentOperation;
  const notice = document.getElementById('notice').textContent;
  await document.emitWorkspaceOverviewIntent('open-another-version');

  assert.equal(selection.value, currentOperation, 'an unverified Overview changed the saved-version selection');
  assert.equal(document.getElementById('workspace-versions-next').scrolledIntoView, false);
  assert.equal(document.workspaceVersionChoice(currentOperation).focused, false);
  assert.equal(document.getElementById('notice').textContent, notice, 'a refused unverified shortcut emitted misleading action feedback');
});

test('a completed mutation followed by refresh failure pauses every later managed write', async (context) => {
  for (const failure of ['workspace state', 'checkpoint state', 'workspace switched']) {
    await context.test(failure, async () => {
      const document = fakeDocument();
      const workspace = {
        root: '/managed/project',
        digest: 'state-project',
        installation: 'state-project',
        records: 1,
        private_version: { version: 'version-1' },
        shared_version: null,
        entries: [{ path: 'kept.txt', type: 'file' }],
        conditions: [],
        not_yet: [],
        file_histories: [],
      };
      let mutationFinished = false;
      const invoke = async (command, parameters = {}) => {
        if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
        if (command === 'managed_checkpoint_state') {
          if (mutationFinished && failure === 'checkpoint state') throw new Error('checkpoint refresh unavailable');
          return JSON.stringify({
            root: mutationFinished && failure === 'workspace switched' ? '/managed/other' : workspace.root,
            workspace_digest: workspace.digest,
            workspace_installation: workspace.installation,
            working: false,
          });
        }
        if (command === 'create_managed_text') {
          mutationFinished = true;
          return JSON.stringify({ author_authenticated: true, saved_privately: true });
        }
        if (command === 'daemon_call' && parameters.method === 'workspace.state') {
          if (mutationFinished && failure === 'workspace state') throw new Error('state refresh unavailable');
          return JSON.stringify(mutationFinished ? { ...workspace, root: '/managed/newer-state' } : workspace);
        }
        throw new Error(`unexpected native command: ${command}`);
      };

      globalThis.document = document;
      globalThis.window = { __TAURI__: { core: { invoke } } };
      globalThis.confirm = () => true;
      await import(`./app.js?post-mutation-refresh=${failure}-${Date.now()}`);
      await waitFor(() => document.serviceState.state === 'ready');

      const path = document.getElementById('manage-path');
      path.value = 'new.txt';
      await path.emit('input');
      assert.equal(document.getElementById('create-text-entry').disabled, false);

      await document.getElementById('create-text-entry').emit('click');

      assert.equal(mutationFinished, true, 'the native mutation completed before refresh failed');
      assert.equal(
        document.getElementById('create-text-entry').disabled,
        true,
        'the older workspace view must become read only until Refresh succeeds',
      );
      assert.equal(
        !document.workspaceCurrentAction('rollback')?.enabled,
        true,
        'an unverified workspace root must not remain eligible for destructive rollback',
      );
      assert.equal(document.workspaceCurrent?.current.state, 'Needs attention');
      assert.equal(
        document.workspaceCurrent?.current.actions.find((action) => action.id === 'rollback')?.enabled,
        false,
        'the source-owned Current projection kept a destructive action enabled after verification failed',
      );
      assert.equal(
        (document.workspaceCurrent?.current?.agentFolder ?? ''),
        workspace.root,
        'workspace and checkpoint readings must replace the displayed snapshot atomically',
      );
      assert.match(document.getElementById('management-status').textContent, /Management is paused/);
      assert.match(document.getElementById('notice').textContent, /Refresh succeeds/);
    });
  }
});

test('managed entry changes recover their exact visible state after a lost native reply without replay', async (context) => {
  const cases = [
    {
      command: 'create_managed_text',
      path: 'new.txt ',
      expectedEntry: { path: 'new.txt ', type: 'file' },
      act: async (document) => {
        const path = document.getElementById('manage-path');
        path.value = 'new.txt ';
        await path.emit('input');
        await document.getElementById('create-text-entry').emit('click');
      },
    },
    {
      command: 'create_managed_folder',
      path: 'new-folder ',
      expectedEntry: { path: 'new-folder ', type: 'folder' },
      act: async (document) => {
        const path = document.getElementById('manage-path');
        path.value = 'new-folder ';
        await path.emit('input');
        await document.getElementById('create-folder-entry').emit('click');
      },
    },
    {
      command: 'move_managed_entry',
      path: 'moved.txt ',
      expectedEntry: { path: 'moved.txt ', type: 'file' },
      act: async (document) => {
        const source = document.getElementById('manage-entry');
        source.value = 'kept.txt';
        await source.emit('change');
        const target = document.getElementById('move-path');
        target.value = 'moved.txt ';
        await target.emit('input');
        await document.getElementById('move-entry').emit('click');
      },
    },
  ];

  for (const scenario of cases) {
    await context.test(scenario.command, async () => {
      const document = fakeDocument();
      let workspace = {
        root: `/managed/${scenario.command}`,
        digest: `state-${scenario.command}-before`,
        installation: `installation-${scenario.command}`,
        records: 1,
        private_version: { version: 'version-before' },
        shared_version: null,
        entries: [{ path: 'kept.txt', type: 'file' }],
        conditions: [],
        not_yet: [],
        file_histories: [],
      };
      let mutationCalls = 0;
      const invoke = async (command, parameters = {}) => {
        if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
        if (command === 'managed_checkpoint_state') {
          return JSON.stringify({
            root: workspace.root,
            workspace_digest: workspace.digest,
            workspace_installation: workspace.installation,
            working: false,
          });
        }
        if (command === 'daemon_call' && parameters.method === 'workspace.state') {
          return JSON.stringify(workspace);
        }
        if (command === scenario.command) {
          if (scenario.command === 'move_managed_entry') {
            assert.equal(parameters.toPath, scenario.path, 'the move target was normalized into another entry');
          } else {
            assert.equal(parameters.relativePath, scenario.path, 'the new entry path was normalized into another entry');
          }
          mutationCalls += 1;
          workspace = {
            ...workspace,
            digest: `state-${scenario.command}-after`,
            records: 2,
            private_version: { version: 'version-after' },
            entries: scenario.command === 'move_managed_entry'
              ? [scenario.expectedEntry]
              : [...workspace.entries, scenario.expectedEntry],
          };
          throw new Error(`${scenario.command} reply was lost after commit`);
        }
        throw new Error(`unexpected native command: ${command}`);
      };

      globalThis.document = document;
      globalThis.window = { __TAURI__: { core: { invoke } } };
      globalThis.confirm = () => true;
      await import(`./app.js?managed-entry-lost-reply=${scenario.command}-${Date.now()}`);
      await waitFor(() => document.serviceState.state === 'ready');

      await scenario.act(document);

      assert.equal(mutationCalls, 1, 'the ambiguous managed entry change was replayed');
      assert.equal(
        document.serviceState.state === 'ready',
        true,
        'the exact refreshed workspace did not restore action continuity',
      );
      assert.ok(
        document.getElementById('manage-entry').children.some((entry) => entry.value === scenario.path),
        'the committed native state was not installed after the lost reply',
      );
      assert.match(document.getElementById('notice').textContent, /Nothing was replayed/);
      assert.match(document.getElementById('notice').textContent, /now shows/);
    });
  }

  await context.test('unreadable recovery stays paused', async () => {
    const document = fakeDocument();
    const workspace = {
      root: '/managed/unreadable-entry-recovery',
      digest: 'state-unreadable-before',
      installation: 'installation-unreadable',
      records: 1,
      private_version: { version: 'version-before' },
      shared_version: null,
      entries: [],
      conditions: [],
      not_yet: [],
      file_histories: [],
    };
    let mutationCalls = 0;
    let mutationDispatched = false;
    const invoke = async (command, parameters = {}) => {
      if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
      if (command === 'managed_checkpoint_state') {
        return JSON.stringify({
          root: workspace.root,
          workspace_digest: workspace.digest,
          workspace_installation: workspace.installation,
          working: false,
        });
      }
      if (command === 'daemon_call' && parameters.method === 'workspace.state') {
        if (mutationDispatched) throw new Error('recovery workspace read unavailable');
        return JSON.stringify(workspace);
      }
      if (command === 'create_managed_folder') {
        mutationCalls += 1;
        mutationDispatched = true;
        throw new Error('create_managed_folder reply was lost after commit');
      }
      throw new Error(`unexpected native command: ${command}`);
    };

    globalThis.document = document;
    globalThis.window = { __TAURI__: { core: { invoke } } };
    globalThis.confirm = () => true;
    await import(`./app.js?managed-entry-unreadable-recovery=${Date.now()}`);
    await waitFor(() => document.serviceState.state === 'ready');

    const path = document.getElementById('manage-path');
    path.value = 'uncertain-folder';
    await path.emit('input');
    await document.getElementById('create-folder-entry').emit('click');

    assert.equal(mutationCalls, 1, 'the unreadable managed entry change was replayed');
    assert.equal(document.serviceState.state === 'ready', false);
    assert.match(document.getElementById('notice').textContent, /Nothing was replayed/);
    assert.match(document.getElementById('notice').textContent, /Managed actions remain paused until Refresh succeeds/);
  });

  await context.test('replacement workspace is never accepted as read-back', async () => {
    const document = fakeDocument();
    let workspace = {
      root: '/managed/entry-recovery-a',
      digest: 'state-entry-recovery-a',
      installation: 'installation-entry-recovery-a',
      records: 1,
      private_version: { version: 'version-a' },
      shared_version: null,
      entries: [],
      conditions: [],
      not_yet: [],
      file_histories: [],
    };
    let mutationCalls = 0;
    const invoke = async (command, parameters = {}) => {
      if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
      if (command === 'managed_checkpoint_state') {
        return JSON.stringify({
          root: workspace.root,
          workspace_digest: workspace.digest,
          workspace_installation: workspace.installation,
          working: false,
        });
      }
      if (command === 'daemon_call' && parameters.method === 'workspace.state') {
        return JSON.stringify(workspace);
      }
      if (command === 'create_managed_text') {
        mutationCalls += 1;
        workspace = {
          ...workspace,
          root: '/managed/entry-recovery-b',
          digest: 'state-entry-recovery-b',
          installation: 'installation-entry-recovery-b',
          private_version: { version: 'version-b' },
          entries: [{ path: 'new.txt', type: 'file' }],
        };
        throw new Error('create_managed_text reply was lost while another workspace opened');
      }
      throw new Error(`unexpected native command: ${command}`);
    };

    globalThis.document = document;
    globalThis.window = { __TAURI__: { core: { invoke } } };
    globalThis.confirm = () => true;
    await import(`./app.js?managed-entry-replacement-recovery=${Date.now()}`);
    await waitFor(() => document.serviceState.state === 'ready');

    const path = document.getElementById('manage-path');
    path.value = 'new.txt';
    await path.emit('input');
    await document.getElementById('create-text-entry').emit('click');

    assert.equal(mutationCalls, 1, 'the ambiguous mutation was replayed against the replacement');
    assert.match(document.getElementById('notice').textContent, /Refresh found a different workspace/);
    assert.doesNotMatch(document.getElementById('notice').textContent, /this exact workspace now shows/);
  });
});

test('ambiguous recovery disables destructive managed-copy rollback', async () => {
  const document = fakeDocument();
  let preference = false;
  let preferenceReads = 0;
  let preferenceWrites = 0;
  const workspace = {
    root: '/managed/recovery-needed',
    digest: 'state-recovery-needed',
    installation: 'installation-recovery-needed',
    records: 1,
    private_version: { version: 'version-1' },
    shared_version: null,
    entries: [{ path: 'recover-me.txt', type: 'file' }],
    conditions: [{
      code: 'checkpoint-recovery-needs-attention',
      message: 'Preserve the current files while recovery is resolved.',
    }],
    not_yet: [],
    file_histories: [],
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'native_capture_preference') {
      preferenceReads += 1;
      return JSON.stringify({ enabled: preference });
    }
    if (command === 'set_native_capture_preference') {
      preferenceWrites += 1;
      preference = parameters.enabled;
      throw new Error('the renderer lost the committed recovery-mode preference reply');
    }
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: true,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?recovery-rollback=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  assert.equal((document.workspaceCurrent?.current?.state ?? ''), 'Needs attention');
  assert.equal(
    !document.workspaceCurrentAction('rollback')?.enabled,
    true,
    'ambiguous recovery left destructive whole-workspace rollback enabled',
  );
  const toggle = document.getElementById('auto-save-native');
  assert.equal(toggle.disabled, false, 'a global preference was blocked by one workspace recovery state');
  toggle.checked = true;
  await toggle.emit('change');
  await waitFor(() => preferenceWrites === 1);
  await waitFor(() => toggle.checked === true);
  assert.equal(preferenceReads, 2, 'the lost global preference reply was not read back exactly once');
  assert.match(document.getElementById('notice').textContent, /lost the automatic-save reply/i);
  assert.match(document.getElementById('notice').textContent, /verified.+is on/i);
});

test('a completed restore cannot leave stale history and later writes verified', async () => {
  const document = fakeDocument();
  const currentVersion = '01CURRENTVERSION00000000000';
  const targetVersion = '01TARGETVERSION000000000000';
  const workspace = {
    root: '/managed/project',
    digest: 'state-project',
    installation: 'state-project',
    records: 2,
    private_version: { version: currentVersion },
    shared_version: null,
    entries: [{ path: 'kept.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'kept.txt',
      object_id: '01OBJECT0000000000000000000',
      current: { version_id: currentVersion, manifest_id: 'manifest-current' },
      retained_versions: [
        { version_id: currentVersion, manifest_id: 'manifest-current' },
        { version_id: targetVersion, manifest_id: 'manifest-target' },
      ],
    }],
  };
  let restoreFinished = false;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      if (restoreFinished) throw new Error('checkpoint refresh unavailable');
      return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    }
    if (command === 'inspect_managed_file') {
      return JSON.stringify({ path: 'kept.txt', content_digest: 'digest-before-restore' });
    }
    if (command === 'restore_managed_version') {
      restoreFinished = true;
      return JSON.stringify({ stable_after_idle: true, recovery: 'recovery-1' });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'preview_managed_restore') {
      assert.equal(parameters.expectedWorkspaceRoot, workspace.root);
      assert.equal(parameters.expectedWorkspaceDigest, workspace.digest);
      return exactRestorePreview({
        objectId: workspace.file_histories[0].object_id,
        currentVersion,
        targetVersion,
        currentManifest: 'manifest-current',
        targetManifest: 'manifest-target',
        workspaceRoot: workspace.root,
        workspaceDigest: workspace.digest,
        workspaceInstallation: workspace.installation,
      });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?restore-refresh=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const path = document.getElementById('manage-path');
  path.value = 'later.txt';
  await path.emit('input');
  assert.equal(document.getElementById('create-text-entry').disabled, false);

  const file = document.getElementById('restore-file');
  file.value = workspace.file_histories[0].object_id;
  await file.emit('change');
  const target = document.getElementById('restore-target');
  target.value = targetVersion;
  await target.emit('change');
  await document.getElementById('restore-preview').emit('click');
  assert.equal(document.getElementById('restore-apply').disabled, false);

  await document.getElementById('restore-apply').emit('click');

  assert.equal(restoreFinished, true, 'the native restore completed before verification failed');
  assert.equal(
    document.getElementById('create-text-entry').disabled,
    true,
    'a stale pre-restore workspace must not authorize a later mutation',
  );
  assert.equal(document.getElementById('restore-undo').disabled, true);
  assert.match(document.getElementById('notice').textContent, /Refresh succeeds/);
});

test('a lost restore reply is never replayed and Refresh exposes the durable current version as recovery', async () => {
  const document = fakeDocument();
  const objectId = '01OBJECTRESTORERECOVERY00000';
  const currentVersion = '01CURRENTRESTORERECOVERY000';
  const earlierVersion = '01EARLIERRESTORERECOVERY000';
  const currentDigest = 'c1'.repeat(32);
  const earlierDigest = 'e1'.repeat(32);
  const workspace = {
    root: '/managed/restore-recovery',
    digest: 'state-restore-recovery',
    installation: 'installation-restore-recovery',
    records: 2,
    private_version: { version: currentVersion },
    shared_version: null,
    entries: [{ path: 'notes.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    file_histories: [{
      path: 'notes.txt',
      object_id: objectId,
      current: { version_id: currentVersion, manifest_id: 'manifest-current' },
      retained_versions: [
        { version_id: currentVersion, manifest_id: 'manifest-current' },
        { version_id: earlierVersion, manifest_id: 'manifest-earlier' },
      ],
    }],
  };
  let restoreCommitted = false;
  const restoreCalls = [];
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      native_folder: false,
      working: restoreCommitted,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'inspect_managed_file') return JSON.stringify({
      path: 'notes.txt',
      current_version: currentVersion,
      byte_count: restoreCommitted ? 8 : 16,
      content_digest: restoreCommitted ? earlierDigest : currentDigest,
      executable: false,
      text: restoreCommitted ? 'earlier\n' : 'current version\n',
      text_editable: true,
      modified_from_current_version: restoreCommitted,
    });
    if (command === 'preview_managed_restore') return exactRestorePreview({
      objectId,
      currentVersion,
      targetVersion: parameters.targetVersion,
      currentManifest: 'manifest-current',
      targetManifest: parameters.targetVersion === currentVersion ? 'manifest-current' : 'manifest-earlier',
      path: 'notes.txt',
      workspaceRoot: workspace.root,
      workspaceDigest: workspace.digest,
      workspaceInstallation: workspace.installation,
      workingDigest: restoreCommitted ? earlierDigest : currentDigest,
      workingByteCount: restoreCommitted ? '8' : '16',
      workingModified: restoreCommitted,
      undoVersion: parameters.targetVersion === currentVersion ? earlierVersion : currentVersion,
      undoManifest: parameters.targetVersion === currentVersion ? 'manifest-earlier' : 'manifest-current',
    });
    if (command === 'restore_managed_version') {
      restoreCalls.push(parameters);
      if (restoreCalls.length === 1) {
        restoreCommitted = true;
        throw new Error('native reply was lost after commit');
      }
      return JSON.stringify({
        stable_after_idle: true,
        recovery: 'recovery-return-current',
        content_digest: currentDigest,
        executable: false,
      });
    }
    throw new Error(`unexpected restore-recovery command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?restore-lost-reply-recovery=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const file = document.getElementById('restore-file');
  file.value = objectId;
  await file.emit('change');
  const target = document.getElementById('restore-target');
  target.value = earlierVersion;
  await target.emit('change');
  await document.getElementById('restore-preview').emit('click');
  await document.getElementById('restore-apply').emit('click');

  await waitFor(() => /native reply was lost after commit/.test(document.getElementById('notice').textContent));
  assert.equal(restoreCalls.length, 1, 'Mesh replayed a non-idempotent restore after losing its reply');
  assert.equal(document.getElementById('restore-apply').disabled, true);

  await document.emitWorkspaceCurrentIntent('refresh');
  await waitFor(() => document.getElementById('restore-target').children.some((entry) => entry.value === currentVersion));
  assert.equal(restoreCalls.length, 1, 'Refresh replayed the ambiguous restore instead of inspecting durable bytes');
  const returnOption = document.getElementById('restore-target').children
    .find((entry) => entry.value === currentVersion);
  assert.equal(returnOption.textContent.startsWith('Return to current saved version'), true);

  target.value = currentVersion;
  await target.emit('change');
  await document.getElementById('restore-preview').emit('click');
  assert.match(document.getElementById('restore-output').textContent, /return.+current saved version/i);
  await document.getElementById('restore-apply').emit('click');
  assert.equal(restoreCalls.length, 2, 'the explicit recovery target did not perform its single requested restore');
  assert.equal(restoreCalls[1].targetVersion, currentVersion);
  assert.equal(restoreCalls[1].expectedContentDigest, earlierDigest);
  assert.equal(restoreCalls[1].expectedExecutable, false);
  assert.equal(restoreCalls[1].expectedWorkspaceRoot, workspace.root);
  assert.equal(restoreCalls[1].expectedWorkspaceDigest, workspace.digest);
  assert.equal(restoreCalls[1].expectedWorkspaceInstallation, workspace.installation);
  assert.equal(document.getElementById('restore-undo').disabled, false);
  await document.getElementById('restore-undo').emit('click');
  assert.equal(restoreCalls.length, 3);
  assert.equal(restoreCalls[2].targetVersion, earlierVersion, 'recovery undo must return to the exact retained preimage, not replay the current target');
  assert.equal(restoreCalls[2].expectedContentDigest, currentDigest);
});

test('restart exposes binary working bytes as an explicit return-to-current restore target', async () => {
  const document = fakeDocument();
  const objectId = '01OBJECTBINARYRECOVERY000000';
  const currentVersion = '01CURRENTBINARYRECOVERY0000';
  const earlierVersion = '01EARLIERBINARYRECOVERY0000';
  const restoredDigest = 'b1'.repeat(32);
  const workspace = {
    root: '/managed/binary-restore-recovery',
    digest: 'state-binary-restore-recovery',
    installation: 'installation-binary-restore-recovery',
    records: 2,
    private_version: { version: currentVersion },
    shared_version: null,
    entries: [{ path: 'finance/model.xlsx', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    file_histories: [{
      path: 'finance/model.xlsx',
      object_id: objectId,
      current: { version_id: currentVersion, manifest_id: 'manifest-current-xlsx' },
      retained_versions: [
        { version_id: currentVersion, manifest_id: 'manifest-current-xlsx' },
        { version_id: earlierVersion, manifest_id: 'manifest-earlier-xlsx' },
      ],
    }],
  };
  const restoreCalls = [];
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      auto_opened: true,
      remembered: workspace.root,
      workspaces: [workspace.root],
      active_folder: '/managed/native/binary-restore-recovery',
    });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      native_folder: false,
      working: true,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'inspect_managed_file') return JSON.stringify({
      path: 'finance/model.xlsx',
      current_version: currentVersion,
      byte_count: 4096,
      content_digest: restoredDigest,
      executable: false,
      text: null,
      text_editable: false,
      modified_from_current_version: true,
    });
    if (command === 'preview_managed_restore') return exactRestorePreview({
      objectId,
      currentVersion,
      targetVersion: currentVersion,
      currentManifest: 'manifest-current-xlsx',
      targetManifest: 'manifest-current-xlsx',
      path: 'finance/model.xlsx',
      workspaceRoot: workspace.root,
      workspaceDigest: workspace.digest,
      workspaceInstallation: workspace.installation,
      workingDigest: restoredDigest,
      workingByteCount: '4096',
      workingModified: true,
      undoVersion: earlierVersion,
      undoManifest: 'manifest-earlier-xlsx',
    });
    if (command === 'restore_managed_version') {
      restoreCalls.push(parameters);
      return JSON.stringify({ stable_after_idle: true, recovery: 'binary-recovery', content_digest: 'digest-original-xlsx', executable: false });
    }
    if (command === 'reconcile_managed_workspace_navigation') return JSON.stringify({
      path: '/managed/native/binary-restore-recovery',
      stable: true,
      workspace_root: workspace.root,
    });
    throw new Error(`unexpected binary restore recovery command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?restart-binary-restore-recovery=${Date.now()}`);
  await waitFor(() => document.getElementById('folder-change-items').children.length === 1);

  const file = document.getElementById('restore-file');
  file.value = objectId;
  await file.emit('change');
  const target = document.getElementById('restore-target');
  const returnOption = target.children.find((entry) => entry.value === currentVersion);
  assert.equal(returnOption?.textContent.startsWith('Return to current saved version'), true);
  target.value = currentVersion;
  await target.emit('change');
  await document.getElementById('restore-preview').emit('click');
  assert.equal(document.getElementById('restore-apply').disabled, false, document.getElementById('notice').textContent);
  await document.getElementById('restore-apply').emit('click');
  assert.equal(restoreCalls.length, 1);
  assert.equal(restoreCalls[0].targetVersion, currentVersion);
  assert.equal(restoreCalls[0].expectedContentDigest, restoredDigest);
  assert.equal(restoreCalls[0].expectedExecutable, false);
  assert.equal(restoreCalls[0].expectedWorkspaceRoot, workspace.root);
  assert.equal(restoreCalls[0].expectedWorkspaceDigest, workspace.digest);
  assert.equal(restoreCalls[0].expectedWorkspaceInstallation, workspace.installation);
});

test('restore preview explains the exact working-copy change without exposing daemon JSON', async () => {
  const document = fakeDocument();
  const currentVersion = '01CURRENTVERSION00000000000';
  const targetVersion = '01TARGETVERSION000000000000';
  const objectId = '01OBJECT0000000000000000000';
  const workspace = {
    root: '/managed/project',
    digest: 'state-project',
    installation: 'installation-project',
    records: 2,
    private_version: { version: currentVersion },
    shared_version: null,
    entries: [{ path: 'finance/forecast.xlsx', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'finance/forecast.xlsx',
      object_id: objectId,
      current: { version_id: currentVersion, manifest_id: 'manifest-current' },
      retained_versions: [
        { version_id: currentVersion, manifest_id: 'manifest-current' },
        { version_id: targetVersion, manifest_id: 'manifest-target' },
      ],
    }],
  };
  const preview = JSON.parse(exactRestorePreview({
    objectId,
    currentVersion,
    targetVersion,
    currentManifest: 'manifest-current',
    targetManifest: 'manifest-target',
    path: 'finance/forecast.xlsx',
    workspaceRoot: workspace.root,
    workspaceDigest: workspace.digest,
    workspaceInstallation: workspace.installation,
    workingDigest: 'a1'.repeat(32),
    workingByteCount: '4096',
    targetDigest: 'c2'.repeat(32),
    targetByteCount: '3072',
  }));
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'preview_managed_restore') return JSON.stringify(preview);
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?restore-readable-preview=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  let restoreProjection = null;
  document.addEventListener('mesh:workspace-restore-projection', (event) => {
    restoreProjection = event.detail;
    document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-mounted', {
      detail: { generation: event.detail.generation },
    }));
  });
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-available'));
  assert.equal(restoreProjection.restore.files[0].format, 'Excel');
  assert.equal(document.getElementById('workspace-restore-next').classList.contains('hidden'), false);
  const initialGeneration = restoreProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-intent', {
    detail: { generation: initialGeneration - 1, intent: { type: 'select-file', id: objectId } },
  }));
  assert.equal(document.getElementById('restore-file').value, '');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-intent', {
    detail: { generation: initialGeneration, intent: { type: 'select-file', id: objectId } },
  }));
  assert.equal(restoreProjection.restore.selectedFileId, objectId);
  const fileGeneration = restoreProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-intent', {
    detail: { generation: fileGeneration, intent: { type: 'select-version', id: targetVersion } },
  }));
  assert.equal(restoreProjection.restore.selectedVersionId, targetVersion);
  const targetGeneration = restoreProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-intent', {
    detail: { generation: targetGeneration, intent: { type: 'select-version', id: 'not-projected' } },
  }));
  assert.equal(document.getElementById('restore-target').value, targetVersion);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-intent', {
    detail: { generation: targetGeneration, intent: { type: 'preview' } },
  }));
  await waitFor(() => restoreProjection.restore.preview !== null);

  const output = document.getElementById('restore-output').textContent;
  assert.match(output, /finance\/forecast\.xlsx/);
  assert.match(output, /working copy/i);
  assert.match(output, /private history stays unchanged/i);
  assert.match(output, /undo.+available/i);
  assert.doesNotMatch(output, /"schema"|"execution_authorized"|^\s*\{/m);
  assert.equal(document.getElementById('restore-apply').disabled, false);
  assert.equal(restoreProjection.restore.preview.filePath, 'finance/forecast.xlsx');
  assert.match(restoreProjection.restore.preview.change, /working folder/);
  assert.match(restoreProjection.restore.preview.historyNote, /Private history stays unchanged/);
  assert.equal(restoreProjection.restore.canApply, true);

  preview.working_copy.byte_count = 4096;
  await document.getElementById('restore-preview').emit('click');
  assert.equal(document.getElementById('restore-apply').disabled, true);
  assert.match(document.getElementById('notice').textContent, /could not verify.+selected file and saved versions/i);
  preview.working_copy.byte_count = '4096';

  preview.target.content_digest = 'CD'.repeat(32);
  await document.getElementById('restore-preview').emit('click');
  assert.equal(document.getElementById('restore-apply').disabled, true);
  assert.match(document.getElementById('notice').textContent, /could not verify.+selected file and saved versions/i);
  preview.target.content_digest = 'c2'.repeat(32);

  preview.operations = [];
  await document.getElementById('restore-preview').emit('click');
  assert.equal(document.getElementById('restore-apply').disabled, true);
  assert.match(document.getElementById('notice').textContent, /could not verify.+selected file and saved versions/i);
  delete preview.operations;

  preview.target.version_id = currentVersion;
  await document.getElementById('restore-preview').emit('click');
  assert.equal(document.getElementById('restore-apply').disabled, true);
  assert.equal(document.getElementById('restore-output').classList.contains('hidden'), true);
  assert.match(document.getElementById('notice').textContent, /could not verify.+selected file and saved versions/i);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-rejected', {
    detail: { generation: restoreProjection.generation },
  }));
  assert.equal(document.getElementById('workspace-restore-next').classList.contains('hidden'), true);
});

test('controlled Restore and Workspace Versions selections stay live until their replacement commits', async () => {
  const document = fakeDocument();
  const firstOperation = '41'.repeat(32);
  const secondOperation = '42'.repeat(32);
  const objectId = '01FOCUSRESTOREOBJECT00000000';
  const currentVersion = '01FOCUSCURRENTVERSION000000';
  const targetVersion = '01FOCUSTARGETVERSION0000000';
  const workspace = {
    root: '/managed/interaction-focus/mounts',
    digest: 'interaction-focus-digest',
    installation: 'interaction-focus-installation',
    records: 4,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: currentVersion, state: 'working', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'forecast.xlsx', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'forecast.xlsx',
      object_id: objectId,
      current: { version_id: currentVersion, manifest_id: 'manifest-current' },
      retained_versions: [
        { version_id: currentVersion, manifest_id: 'manifest-current' },
        { version_id: targetVersion, manifest_id: 'manifest-target' },
      ],
    }],
    native_untracked_files: [],
    native_inventory_complete: true,
    workspace_versions: [
      { operation: firstOperation, ordinal: 1, actor_sequence: '1' },
      { operation: secondOperation, ordinal: 2, actor_sequence: '2' },
    ],
  };
  const versionPreviewResolvers = [];
  let restorePreviewCalls = 0;
  let releaseRestorePreview = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        auto_opened: false,
        remembered: workspace.root,
        workspaces: [workspace.root],
        workspace_entries: [{ path: workspace.root, export_root: null, project_root: null }],
        active_folder: '/managed/interaction-focus/current',
        export_root: null,
      });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: true,
        native_folder_path: workspace.root,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'reconcile_managed_workspace_navigation') {
      return JSON.stringify({
        path: '/managed/interaction-focus/current',
        workspace_root: workspace.root,
        stable: true,
      });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'preview_managed_workspace_version') {
      return new Promise((resolve) => versionPreviewResolvers.push(() => resolve(savedWorkspacePreview(
        parameters.operation,
        [{ path: 'forecast.xlsx', type: 'file', bytes: '12' }],
        { ordinal: parameters.operation === firstOperation ? 1 : 2 },
      ))));
    }
    if (command === 'preview_managed_restore') {
      restorePreviewCalls += 1;
      return new Promise((resolve) => {
        releaseRestorePreview = () => resolve(exactRestorePreview({
          objectId,
          currentVersion,
          targetVersion,
          currentManifest: 'manifest-current',
          targetManifest: 'manifest-target',
          path: 'forecast.xlsx',
          workspaceRoot: workspace.root,
          workspaceDigest: workspace.digest,
          workspaceInstallation: workspace.installation,
        }));
      });
    }
    throw new Error(`unexpected interaction-focus command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?restore-version-interaction-focus=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const installFocusAwareHost = (id) => {
    const host = document.getElementById(id);
    host.shadowRoot = { activeElement: null };
    const toggle = host.classList.toggle.bind(host.classList);
    host.classList.toggle = (value, force) => {
      const result = toggle(value, force);
      if (value === 'hidden' && result) host.shadowRoot.activeElement = null;
      return result;
    };
    return host;
  };

  const versionsHost = installFocusAwareHost('workspace-versions-next');
  let versionsProjection = null;
  let committedVersionsProjection = null;
  const versionsProjections = new Map();
  let mountVersions = true;
  document.addEventListener('mesh:workspace-versions-projection', (event) => {
    versionsProjection = event.detail;
    versionsProjections.set(event.detail.generation, event.detail);
    if (mountVersions) {
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-mounted', {
        detail: { generation: event.detail.generation },
      }));
    }
  });
  document.addEventListener('mesh:workspace-versions-mounted', (event) => {
    if (event.detail.generation === versionsProjection?.generation) {
      committedVersionsProjection = versionsProjections.get(event.detail.generation) ?? null;
    }
  });
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-available'));
  const versionOwner = { isConnected: true };
  versionsHost.shadowRoot.activeElement = versionOwner;
  mountVersions = false;
  const mountedVersionGeneration = versionsProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-intent', {
    detail: {
      generation: mountedVersionGeneration,
      intent: { type: 'select-version', operation: firstOperation },
    },
  }));
  await waitFor(() => versionsProjection.generation > mountedVersionGeneration);
  assert.equal(versionsHost.classList.contains('hidden'), false, 'selection hid the focused version picker before its loading projection committed');
  assert.equal(versionsHost.shadowRoot.activeElement, versionOwner, 'selection dropped the focused version row during the commit gap');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-intent', {
    detail: {
      generation: mountedVersionGeneration,
      intent: { type: 'select-version', operation: secondOperation },
    },
  }));
  await waitFor(() => document.getElementById('workspace-version').value === secondOperation);
  const latestVersionGeneration = versionsProjection.generation;
  versionsHost.shadowRoot.activeElement = null;
  const externalFocus = { focused: true };
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-mounted', {
    detail: { generation: latestVersionGeneration },
  }));
  assert.equal(versionsHost.shadowRoot.activeElement, null, 'the replacement version projection reclaimed focus after the user moved away');
  assert.equal(externalFocus.focused, true, 'the replacement version projection stole newer external focus');
  for (const release of versionPreviewResolvers.splice(0)) release();

  const restoreHost = installFocusAwareHost('workspace-restore-next');
  let restoreProjection = null;
  let mountRestore = true;
  document.addEventListener('mesh:workspace-restore-projection', (event) => {
    restoreProjection = event.detail;
    if (mountRestore) {
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-mounted', {
        detail: { generation: event.detail.generation },
      }));
    }
  });
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-available'));
  const versionPollOwner = { isConnected: true };
  const restorePollOwner = { isConnected: true };
  versionsHost.shadowRoot.activeElement = versionPollOwner;
  restoreHost.shadowRoot.activeElement = restorePollOwner;
  const retainedVersionGeneration = versionsProjection.generation;
  const customLocationBeforeRefresh = '/managed/interaction-focus/custom';
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-intent', {
    detail: {
      generation: retainedVersionGeneration,
      intent: { type: 'set-custom-location', path: customLocationBeforeRefresh },
    },
  }));
  await waitFor(() => document.getElementById('version-destination').value === customLocationBeforeRefresh);
  mountVersions = false;
  mountRestore = false;
  const versionGenerationBeforeRefresh = versionsProjection.generation;
  const restoreGenerationBeforeRefresh = restoreProjection.generation;
  await document.emitWorkspaceCurrentIntent('refresh');
  await waitFor(() => versionsProjection.generation > versionGenerationBeforeRefresh
    && restoreProjection.generation > restoreGenerationBeforeRefresh);
  assert.equal(
    versionsHost.classList.contains('hidden'),
    false,
    'a same-workspace refresh flashed the Versions page while its replacement commit was pending',
  );
  assert.equal(versionsHost.shadowRoot.activeElement, versionPollOwner, 'a same-workspace refresh dropped focus from Versions');
  assert.equal(
    restoreHost.classList.contains('hidden'),
    false,
    'a same-workspace refresh flashed the Restore page while its replacement commit was pending',
  );
  assert.equal(restoreHost.shadowRoot.activeElement, restorePollOwner, 'a same-workspace refresh dropped focus from Restore');
  const supersededLocationProjection = versionsProjection;
  const retainedCustomLocation = `${customLocationBeforeRefresh}/continued`;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-intent', {
    detail: {
      generation: retainedVersionGeneration,
      intent: { type: 'set-custom-location', path: retainedCustomLocation },
    },
  }));
  await waitFor(() => versionsProjection.generation > supersededLocationProjection.generation
    && versionsProjection.versions.customLocation === retainedCustomLocation);
  assert.equal(document.getElementById('version-destination').value, retainedCustomLocation);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-mounted', {
    detail: { generation: supersededLocationProjection.generation },
  }));
  assert.notEqual(
    committedVersionsProjection?.generation,
    supersededLocationProjection.generation,
    'the projection captured before the retained field echo committed its stale visible value',
  );
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-mounted', {
    detail: { generation: versionsProjection.generation },
  }));
  assert.equal(committedVersionsProjection?.versions.customLocation, retainedCustomLocation);
  assert.equal(
    committedVersionsProjection?.versions.customLocation,
    document.getElementById('version-destination').value,
    'the committed React field and coordinator destination disagreed before Open',
  );
  const retainedOperation = document.getElementById('workspace-version').value;
  assert.match(retainedOperation, /^[0-9a-f]{64}$/u, 'the retained semantic-action check did not target an exact version');
  assert.equal(workspaceVersionsRetainedInteraction('set-custom-location'), true);
  assert.equal(workspaceVersionsRetainedInteraction(`select-version:${retainedOperation}`), true);
  assert.equal(workspaceVersionsRetainedInteraction(`open-version:${retainedOperation}`), false);
  assert.equal(workspaceVersionsRetainedInteraction(`start-codex:${retainedOperation}`), false);
  let staleOpenCalls = 0;
  let staleStartCalls = 0;
  document.getElementById('fork-version').addEventListener('click', () => { staleOpenCalls += 1; });
  document.getElementById('fork-version-codex').addEventListener('click', () => { staleStartCalls += 1; });
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-intent', {
    detail: {
      generation: retainedVersionGeneration,
      intent: { type: 'open-version', operation: retainedOperation },
    },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-intent', {
    detail: {
      generation: retainedVersionGeneration,
      intent: { type: 'start-codex', operation: retainedOperation },
    },
  }));
  await Promise.resolve();
  assert.equal(staleOpenCalls, 0, 'the superseded retained surface opened a version');
  assert.equal(staleStartCalls, 0, 'the superseded retained surface started Codex');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-intent', {
    detail: {
      generation: restoreGenerationBeforeRefresh,
      intent: { type: 'select-file', id: objectId },
    },
  }));
  assert.equal(
    document.getElementById('restore-file').value,
    '',
    'the retained Restore surface regained action authority before its replacement committed',
  );

  const refreshedVersionGeneration = versionsProjection.generation;
  const refreshedRestoreGeneration = restoreProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-rejected', {
    detail: { generation: refreshedVersionGeneration },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-rejected', {
    detail: { generation: refreshedRestoreGeneration },
  }));
  assert.equal(versionsHost.classList.contains('hidden'), true, 'a rejected Versions projection retained stale visible content');
  assert.equal(restoreHost.classList.contains('hidden'), true, 'a rejected Restore projection retained stale visible content');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-mounted', {
    detail: { generation: refreshedVersionGeneration },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-mounted', {
    detail: { generation: refreshedRestoreGeneration },
  }));
  mountVersions = true;
  mountRestore = true;
  const fileOwner = { isConnected: true };
  restoreHost.shadowRoot.activeElement = fileOwner;
  mountRestore = false;
  const initialRestoreGeneration = restoreProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-intent', {
    detail: { generation: initialRestoreGeneration, intent: { type: 'select-file', id: objectId } },
  }));
  assert.equal(restoreHost.classList.contains('hidden'), false, 'file selection hid the focused Restore picker before commit');
  assert.equal(restoreHost.shadowRoot.activeElement, fileOwner);
  const fileRestoreGeneration = restoreProjection.generation;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-mounted', {
    detail: { generation: fileRestoreGeneration },
  }));
  const restoreVersionOwner = { isConnected: true };
  restoreHost.shadowRoot.activeElement = restoreVersionOwner;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-intent', {
    detail: { generation: fileRestoreGeneration, intent: { type: 'select-version', id: targetVersion } },
  }));
  const targetRestoreGeneration = restoreProjection.generation;
  assert.equal(restoreHost.classList.contains('hidden'), false, 'version selection hid Restore before Preview became current');
  assert.equal(restoreHost.shadowRoot.activeElement, restoreVersionOwner);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-intent', {
    detail: { generation: fileRestoreGeneration, intent: { type: 'preview' } },
  }));
  await waitFor(() => restorePreviewCalls === 1);
  assert.equal(restoreHost.classList.contains('hidden'), false, 'Preview hid its focused owner while native inspection was pending');
  restoreHost.shadowRoot.activeElement = null;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-mounted', {
    detail: { generation: restoreProjection.generation },
  }));
  assert.equal(restoreHost.shadowRoot.activeElement, null, 'the replacement Restore projection reclaimed focus after the user moved away');
  assert.equal(externalFocus.focused, true, 'the replacement Restore projection stole newer external focus');
  releaseRestorePreview();
  await waitFor(() => restoreProjection.restore.preview !== null);
});

test('restore and its one-shot undo are bound to the exact inspected working-copy bytes', async () => {
  const document = fakeDocument();
  const objectId = '01OBJECT0000000000000000000';
  const initialVersion = '01INITIALVERSION0000000000';
  const restoredVersion = '01RESTOREDVERSION000000000';
  const manifest = (version) => `manifest-${version}`;
  const workspaceAt = (version, retainedVersions) => ({
    root: '/managed/project',
    digest: `state-${version}`,
    installation: `state-${version}`,
    records: retainedVersions.length,
    private_version: { version },
    shared_version: null,
    entries: [{ path: 'kept.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'kept.txt',
      object_id: objectId,
      current: { version_id: version, manifest_id: manifest(version) },
      retained_versions: retainedVersions.map((versionId) => ({
        version_id: versionId,
        manifest_id: manifest(versionId),
      })),
    }],
  });
  const workspace = workspaceAt(initialVersion, [initialVersion, restoredVersion]);
  const restoreCalls = [];
  let createCalls = 0;
  let inspectedDigest = 'd1'.repeat(32);
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    }
    if (command === 'inspect_managed_file') {
      return JSON.stringify({
        path: 'kept.txt',
        current_version: initialVersion,
        byte_count: 7,
        content_digest: inspectedDigest,
        text: 'before\n',
        text_editable: true,
        modified_from_current_version: false,
      });
    }
    if (command === 'restore_managed_version') {
      restoreCalls.push(parameters);
      inspectedDigest = restoreCalls.length === 1 ? 'd2'.repeat(32) : 'd3'.repeat(32);
      return JSON.stringify({
        stable_after_idle: true,
        recovery: `recovery-${restoreCalls.length}`,
        content_digest: inspectedDigest,
      });
    }
    if (command === 'create_managed_text') {
      createCalls += 1;
      return JSON.stringify({ author_authenticated: true, saved_privately: true });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'preview_managed_restore') {
      assert.equal(parameters.expectedWorkspaceRoot, workspace.root);
      assert.equal(parameters.expectedWorkspaceDigest, workspace.digest);
      return exactRestorePreview({
        objectId,
        currentVersion: initialVersion,
        targetVersion: restoredVersion,
        workspaceRoot: workspace.root,
        workspaceDigest: workspace.digest,
        workspaceInstallation: workspace.installation,
        workingDigest: inspectedDigest,
      });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?restore-undo-snapshot=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  let restoreProjection = null;
  document.addEventListener('mesh:workspace-restore-projection', (event) => {
    restoreProjection = event.detail;
    document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-mounted', {
      detail: { generation: event.detail.generation },
    }));
  });
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-available'));

  const editorFile = document.getElementById('edit-file');
  editorFile.value = 'kept.txt';
  await editorFile.emit('change');
  await document.getElementById('load-file').emit('click');
  const editor = document.getElementById('file-editor');
  const draft = 'draft that exists only in this window\n';
  editor.value = draft;
  await editor.emit('input');
  assert.equal(document.getElementById('save-file').disabled, false);
  const managePath = document.getElementById('manage-path');
  managePath.value = 'other.txt';
  await managePath.emit('input');
  const create = document.getElementById('create-text-entry');
  assert.equal(create.disabled, true, 'file management remained actionable over an editor-only draft');
  await create.emit('click');
  assert.equal(createCalls, 0, 'a scripted or stale create click bypassed the draft guard');
  assert.equal(editor.value, draft, 'file management discarded the editor-only draft');

  const file = document.getElementById('restore-file');
  file.value = objectId;
  await file.emit('change');
  const target = document.getElementById('restore-target');
  target.value = restoredVersion;
  await target.emit('change');
  await document.getElementById('restore-preview').emit('click');
  const apply = document.getElementById('restore-apply');
  assert.equal(apply.disabled, true, 'restore remained actionable over an editor-only draft');
  assert.equal(restoreProjection.restore.canApply, false, 'React projected restore authority over an editor-only draft');
  await apply.emit('click');
  assert.equal(restoreCalls.length, 0, 'a scripted or stale restore click bypassed the draft guard');
  assert.equal(editor.value, draft, 'restore discarded the editor-only draft');

  editor.value = 'before\n';
  const staleApplyGeneration = restoreProjection.generation;
  await editor.emit('input');
  assert.equal(apply.disabled, false, 'restore did not recover after the draft was reverted');
  assert.equal(restoreProjection.restore.canApply, true);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-intent', {
    detail: { generation: staleApplyGeneration, intent: { type: 'apply' } },
  }));
  assert.equal(restoreCalls.length, 0, 'a stale React restore intent crossed the coordinator boundary');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-intent', {
    detail: { generation: restoreProjection.generation, intent: { type: 'apply' } },
  }));
  await waitFor(() => document.getElementById('restore-undo').disabled === false);

  assert.equal(restoreCalls.length, 1);
  assert.equal(
    restoreCalls[0].expectedContentDigest,
    'd1'.repeat(32),
    'restore must refuse if the file changed after preview inspection',
  );
  assert.equal(
    document.getElementById('restore-undo').disabled,
    false,
    'the immediately preceding restore remains undoable',
  );
  assert.equal(restoreProjection.restore.canUndo, true);

  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-restore-intent', {
    detail: { generation: restoreProjection.generation, intent: { type: 'undo' } },
  }));
  await waitFor(() => restoreCalls.length === 2);
  assert.equal(restoreCalls.length, 2);
  assert.equal(
    restoreCalls[1].expectedContentDigest,
    'd2'.repeat(32),
    'undo must refuse rather than overwrite bytes changed after the restore',
  );
});

test('an older overlapping refresh cannot replace or invalidate a newer verified snapshot', async (context) => {
  for (const olderOutcome of ['success', 'failure']) {
    await context.test(olderOutcome, async () => {
      const document = fakeDocument();
      const workspace = (records) => ({
        root: '/managed/project',
        digest: `state-${records}`,
        installation: `state-${records}`,
        records,
        private_version: { version: `version-${records}` },
        shared_version: null,
        entries: [{ path: 'kept.txt', type: 'file' }],
        conditions: [],
        not_yet: [],
        file_histories: [],
      });
      let stateReads = 0;
      let releaseOlder;
      let rejectOlder;
      const older = new Promise((resolve, reject) => {
        releaseOlder = resolve;
        rejectOlder = reject;
      });
      const invoke = async (command, parameters = {}) => {
        if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
        if (command === 'managed_checkpoint_state') {
          return JSON.stringify({
            root: '/managed/project',
            workspace_digest: `state-${stateReads}`,
            workspace_installation: `state-${stateReads}`,
            working: false,
          });
        }
        if (command === 'daemon_call' && parameters.method === 'workspace.state') {
          stateReads += 1;
          if (stateReads === 1) return JSON.stringify(workspace(1));
          if (stateReads === 2) return older;
          if (stateReads === 3) return JSON.stringify(workspace(3));
        }
        throw new Error(`unexpected native command: ${command}`);
      };

      globalThis.document = document;
      globalThis.window = { __TAURI__: { core: { invoke } } };
      globalThis.confirm = () => true;
      await import(`./app.js?overlapping-refresh=${olderOutcome}-${Date.now()}`);
      await waitFor(() => (document.workspaceCurrent?.current?.recordSummary ?? '').startsWith('1 '));

      const first = document.emitWorkspaceCurrentIntent('refresh');
      await waitFor(() => stateReads === 2);
      assert.equal(
        document.getElementById('create-text-entry').disabled,
        true,
        'writes remained enabled while verification was in flight',
      );
      const second = document.emitWorkspaceCurrentIntent('refresh');
      await second;
      const path = document.getElementById('manage-path');
      path.value = 'later.txt';
      await path.emit('input');
      assert.equal(document.getElementById('create-text-entry').disabled, false);

      if (olderOutcome === 'success') releaseOlder(JSON.stringify(workspace(2)));
      else rejectOlder(new Error('older refresh failed'));
      await first;

      assert.match(
        (document.workspaceCurrent?.current?.recordSummary ?? ''),
        /^3 durable records$/,
        'an older request completed last and regressed the displayed verified state',
      );
      assert.equal(document.serviceState.state === 'ready', true);
      assert.equal(
        document.getElementById('create-text-entry').disabled,
        false,
        'an older failed request invalidated a newer verified state',
      );
    });
  }
});

test('a superseded periodic scan cannot overwrite a newer verified refresh with an error', async () => {
  const document = fakeDocument();
  document.visibilityState = 'visible';
  const workspace = (records) => ({
    root: '/managed/periodic-race/mounts',
    digest: `state-${records}`,
    installation: 'installation-periodic-race',
    records,
    private_version: { version: `version-${records}` },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
  });
  let stateReads = 0;
  let releaseOlder;
  let olderReturned = false;
  let scheduledScan = null;
  const older = new Promise((resolve) => {
    releaseOlder = (value) => {
      olderReturned = true;
      resolve(value);
    };
  });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      remembered: workspace(1).root,
      workspaces: [workspace(1).root],
      auto_opened: false,
      active_folder: '/application/native-workspace/current',
    });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace(stateReads).root,
      workspace_digest: `state-${stateReads}`,
      workspace_installation: 'installation-periodic-race',
      native_folder: true,
      native_folder_path: workspace(stateReads).root,
      working: false,
    });
    if (command === 'reconcile_managed_workspace_navigation') return JSON.stringify({
      path: '/application/native-workspace/current',
      workspace_root: workspace(stateReads).root,
      stable: true,
      native_folder: true,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      stateReads += 1;
      if (stateReads === 1) return JSON.stringify(workspace(1));
      if (stateReads === 2) return older;
      if (stateReads === 3) return JSON.stringify(workspace(3));
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = {
    __TAURI__: { core: { invoke } },
    setInterval(listener) {
      scheduledScan = listener;
      return 1;
    },
  };
  globalThis.confirm = () => true;
  await import(`./app.js?periodic-refresh-race=${Date.now()}`);
  await waitFor(() => (document.workspaceCurrent?.current?.recordSummary ?? '').startsWith('1 '));

  scheduledScan();
  await waitFor(() => stateReads === 2);
  await document.emitWorkspaceCurrentIntent('refresh');
  assert.match(
    (document.workspaceCurrent?.current?.recordSummary ?? ''),
    /^3 durable records$/,
    document.getElementById('notice').textContent,
  );
  releaseOlder(JSON.stringify(workspace(2)));
  await waitFor(() => olderReturned);
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(document.serviceState.state === 'ready', true);
  assert.match((document.workspaceCurrent?.current?.recordSummary ?? ''), /^3 durable records$/);
  assert.doesNotMatch(
    document.getElementById('notice').textContent,
    /newer workspace verification replaced this folder scan/i,
    'a stale read-only scan made the newer successful refresh look like a failure',
  );
});

test('periodic background inspection is bounded and rotates across a large workspace', async () => {
  const document = fakeDocument();
  document.visibilityState = 'visible';
  const earlierOperation = 'a1'.repeat(32);
  const currentOperation = 'b2'.repeat(32);
  const histories = Array.from({ length: 200 }, (_, index) => ({
    path: `src/file-${String(index).padStart(3, '0')}.txt`,
    object_id: `01PERIODIC${String(index).padStart(16, '0')}`,
    current: { version_id: `version-${index}`, manifest_id: `manifest-${index}` },
    retained_versions: [{ version_id: `version-${index}`, manifest_id: `manifest-${index}` }],
  }));
  const workspace = {
    root: '/managed/large-periodic/mounts',
    digest: 'large-periodic-digest',
    installation: 'large-periodic-installation',
    records: 200,
    private_version: { version: 'large-periodic-version', concurrent_changes: 1 },
    shared_version: null,
    entries: histories.map((history) => ({ path: history.path, type: 'file' })),
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: histories,
    workspace_versions: [
      { operation: earlierOperation, ordinal: 1, actor_sequence: '1' },
      { operation: currentOperation, ordinal: 2, actor_sequence: '2' },
    ],
  };
  const inspected = [];
  let scheduledScan = null;
  let stateReads = 0;
  let directoryDiscoveryCalls = 0;
  let openedEarlierVersion = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      remembered: workspace.root,
      workspaces: [workspace.root],
      auto_opened: false,
      active_folder: '/application/native-workspace/current',
    });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      native_folder: true,
      native_folder_path: workspace.root,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      stateReads += 1;
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories') {
      directoryDiscoveryCalls += 1;
      return JSON.stringify([]);
    }
    if (command === 'discover_native_missing_files') {
      return JSON.stringify([]);
    }
    if (command === 'inspect_managed_file') {
      inspected.push(parameters.relativePath);
      const index = Number(parameters.relativePath.match(/(\d+)\.txt$/)[1]);
      return JSON.stringify({
        path: parameters.relativePath,
        text: null,
        text_editable: false,
        native_untracked: false,
        modified_from_current_version: false,
        current_version: `version-${index}`,
        byte_count: 1,
        content_digest: 'a1'.repeat(32),
        executable: false,
      });
    }
    if (command === 'preview_managed_workspace_version') {
      assert.deepEqual(parameters, {
        operation: earlierOperation,
        expectedWorkspaceRoot: workspace.root,
        expectedWorkspaceDigest: workspace.digest,
        expectedWorkspaceInstallation: workspace.installation,
      });
      return savedWorkspacePreview(earlierOperation, [{ path: 'earlier.txt', type: 'file', bytes: '7' }]);
    }
    if (command === 'open_managed_workspace_version') {
      openedEarlierVersion += 1;
      assert.equal(parameters.operation, earlierOperation);
      return JSON.stringify({
        source_version: earlierOperation,
        source_ordinal: 1,
        destination: workspace.root,
        reused: true,
        workspace,
        navigation: {
          remembered: workspace.root,
          workspaces: [workspace.root],
          auto_opened: false,
          active_folder: workspace.root,
          export_root: null,
          warning: null,
        },
      });
    }
    if (command === 'reveal_managed_workspace') {
      return JSON.stringify({
        path: workspace.root,
        workspace_root: workspace.root,
        stable: true,
        native_folder: true,
      });
    }
    throw new Error(`unexpected bounded-periodic command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = {
    __TAURI__: { core: { invoke } },
    setInterval(listener) {
      scheduledScan = listener;
      return 1;
    },
  };
  globalThis.confirm = () => true;
  await import(`./app.js?bounded-periodic=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  let versionsProjection = null;
  document.addEventListener('mesh:workspace-versions-projection', (event) => {
    versionsProjection = event.detail;
    document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-mounted', {
      detail: { generation: event.detail.generation },
    }));
  });

  // Install every affected React adapter, then discard their initial projections. A clean periodic
  // inspection is only a background hint: it must not withdraw the last verified presentation and
  // replace it with a disabled generation before restoring the same snapshot.
  const projected = new Map([
    ['mesh:workspace-chrome-projection', []],
    ['mesh:workspace-current-projection', []],
    ['mesh:workspace-files-changes-projection', []],
    ['mesh:workspace-versions-projection', []],
    ['mesh:workspace-restore-projection', []],
  ]);
  for (const [eventName, generations] of projected) {
    document.addEventListener(eventName, (event) => generations.push(event.detail.generation));
  }
  for (const eventName of [
    'mesh:workspace-chrome-available',
    'mesh:workspace-current-available',
    'mesh:workspace-files-changes-available',
    'mesh:workspace-versions-available',
    'mesh:workspace-restore-available',
    'mesh:workspace-overview-available',
  ]) {
    document.dispatchEvent(new FakeCustomEvent(eventName));
  }
  assert.ok(versionsProjection, 'Versions did not receive its initial verified generation');
  assert.ok(document.workspaceOverview, 'Overview did not receive its initial verified generation');
  const usableVersionsGeneration = versionsProjection.generation;
  const usableOverviewGeneration = document.workspaceOverview.generation;
  await new Promise((resolve) => setTimeout(resolve, 0));
  for (const generations of projected.values()) generations.length = 0;

  scheduledScan();
  await waitFor(() => stateReads >= 2 && inspected.length >= 128);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(
    inspected.length,
    128,
    'one background tick read every tracked file instead of a bounded slice',
  );
  assert.equal(
    directoryDiscoveryCalls,
    0,
    'one background tick repeated the full native-tree discovery already performed by workspace.state',
  );

  scheduledScan();
  await waitFor(() => stateReads >= 3 && new Set(inspected).size === histories.length);
  assert.equal(inspected.length, histories.length, 'the second tick repeated files before finishing the rotation');
  assert.equal(directoryDiscoveryCalls, 0, 'background rotation repeated a full native-tree discovery');
  assert.deepEqual(new Set(inspected), new Set(histories.map((history) => history.path)));
  assert.deepEqual(
    Object.fromEntries(projected),
    {
      'mesh:workspace-chrome-projection': [],
      'mesh:workspace-current-projection': [],
      'mesh:workspace-files-changes-projection': [],
      'mesh:workspace-versions-projection': [],
      'mesh:workspace-restore-projection': [],
    },
    'a clean periodic scan replaced verified React generations with a transient disabled projection',
  );
  assert.equal(
    versionsProjection.generation,
    usableVersionsGeneration,
    'a clean periodic scan invalidated the mounted Versions action without replacing its generation',
  );
  assert.equal(
    document.workspaceOverview.generation,
    usableOverviewGeneration,
    'a clean periodic scan invalidated the mounted Overview action without replacing its generation',
  );

  // The no-projection guarantee must preserve the authority captured by already mounted React
  // actions. A version selection/open and Overview's recommendation must still execute against
  // the verified snapshot after the clean periodic tick.
  await document.emitWorkspaceOverviewIntent('recommended', usableOverviewGeneration);
  assert.equal(
    document.getElementById('review-workbench-next').scrolledIntoView,
    true,
    'the mounted Overview recommendation was rejected after a clean periodic scan',
  );
  assert.doesNotMatch(
    document.getElementById('notice').textContent,
    /no longer available for this exact workspace/i,
    'a clean periodic scan left the visible Overview recommendation stale',
  );
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-intent', {
    detail: {
      generation: usableVersionsGeneration,
      intent: { type: 'select-version', operation: earlierOperation },
    },
  }));
  await waitFor(() => versionsProjection?.versions?.previewState === 'ready');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-versions-intent', {
    detail: {
      generation: versionsProjection.generation,
      intent: { type: 'open-version', operation: earlierOperation },
    },
  }));
  await waitFor(() => openedEarlierVersion === 1);
});

test('periodic discovery keeps a nested native file with its required unsaved parent', async () => {
  const document = fakeDocument();
  document.visibilityState = 'visible';
  const workspace = {
    root: '/managed/periodic-native-tree/mounts',
    digest: 'periodic-native-tree-digest',
    installation: 'periodic-native-tree-installation',
    records: 1,
    private_version: { version: 'periodic-native-tree-version', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: ['generated/result.txt'],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    file_histories: [],
    workspace_versions: [],
  };
  let scheduledScan = null;
  let directoryDiscoveryCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      remembered: workspace.root,
      workspaces: [workspace.root],
      auto_opened: false,
      active_folder: '/application/native-workspace/current',
    });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      native_folder: true,
      native_folder_path: workspace.root,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories') {
      directoryDiscoveryCalls += 1;
      return JSON.stringify([{ path: 'generated', installation: '7a'.repeat(32) }]);
    }
    if (command === 'discover_native_missing_files') return JSON.stringify([]);
    if (command === 'inspect_native_file') {
      assert.equal(parameters.relativePath, 'generated/result.txt');
      return JSON.stringify({
        path: parameters.relativePath,
        text: 'agent result\n',
        text_editable: true,
        native_untracked: true,
        byte_count: 13,
        content_digest: '7b'.repeat(32),
        executable: false,
      });
    }
    throw new Error(`unexpected periodic native-tree command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = {
    __TAURI__: { core: { invoke } },
    setInterval(listener) {
      scheduledScan = listener;
      return 1;
    },
  };
  globalThis.confirm = () => true;
  await import(`./app.js?periodic-native-tree=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  scheduledScan();
  await waitFor(() => document.getElementById('folder-change-items').children.length > 0);

  assert.equal(
    directoryDiscoveryCalls,
    1,
    'the background scan published a nested file without binding its required native parent',
  );
  assert.equal(document.getElementById('folder-change-items').children.length, 2);
  assert.match(document.getElementById('folder-change-items').children[0].textContent, /generated.*new native folder/);
  assert.match(document.getElementById('folder-change-items').children[1].textContent, /generated\/result\.txt/);
});

test('bounded background content inspection still surfaces every missing tracked file', async () => {
  const document = fakeDocument();
  document.visibilityState = 'visible';
  const histories = Array.from({ length: 129 }, (_, index) => ({
    path: `tracked/file-${String(index).padStart(3, '0')}.txt`,
    object_id: `01MISSING${String(index).padStart(17, '0')}`,
    current: { version_id: `version-${index}`, manifest_id: `manifest-${index}` },
    retained_versions: [{ version_id: `version-${index}`, manifest_id: `manifest-${index}` }],
  }));
  const missingPath = histories.at(-1).path;
  const workspace = {
    root: '/managed/missing-periodic/mounts',
    digest: 'missing-periodic-digest',
    installation: 'missing-periodic-installation',
    records: 129,
    private_version: { version: 'missing-periodic-version', concurrent_changes: 1 },
    shared_version: null,
    entries: histories.map((history) => ({ path: history.path, type: 'file' })),
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: histories,
    workspace_versions: [],
  };
  let scheduledScan = null;
  let inspections = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      remembered: workspace.root,
      workspaces: [workspace.root],
      auto_opened: false,
      active_folder: '/application/native-workspace/current',
    });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      native_folder: true,
      native_folder_path: workspace.root,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories') return JSON.stringify([]);
    if (command === 'discover_native_missing_files') return JSON.stringify([{
      path: missingPath,
      current_version: histories.at(-1).current.version_id,
      content_digest: 'b2'.repeat(32),
      executable: false,
    }]);
    if (command === 'inspect_managed_file') {
      inspections += 1;
      return JSON.stringify({
        path: parameters.relativePath,
        text: null,
        text_editable: false,
        native_untracked: false,
        modified_from_current_version: false,
        current_version: 'unchanged',
        byte_count: 1,
        content_digest: 'a1'.repeat(32),
        executable: false,
      });
    }
    throw new Error(`unexpected missing-periodic command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = {
    __TAURI__: { core: { invoke } },
    setInterval(listener) {
      scheduledScan = listener;
      return 1;
    },
  };
  globalThis.confirm = () => true;
  await import(`./app.js?missing-periodic=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  scheduledScan();
  await waitFor(() => document.getElementById('folder-change-items').children.length === 1);
  assert.equal(inspections, 128, 'the background content bound was not retained');
  assert.match(document.getElementById('folder-change-items').children[0].textContent, new RegExp(missingPath));
  assert.match(document.getElementById('folder-change-items').children[0].textContent, /missing tracked file/);
});

test('preserving working-copy bytes must verify the resulting workspace before enabling another write', async () => {
  const document = fakeDocument();
  const version = '01CURRENTVERSION00000000000';
  const workspace = {
    root: '/managed/project',
    digest: 'state-project',
    installation: 'state-project',
    records: 1,
    private_version: { version },
    shared_version: null,
    entries: [{ path: 'kept.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'kept.txt',
      object_id: '01OBJECT0000000000000000000',
      current: { version_id: version, manifest_id: 'manifest-current' },
      retained_versions: [{ version_id: version, manifest_id: 'manifest-current' }],
    }],
  };
  let preserved = false;
  let checkpointReads = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      checkpointReads += 1;
      if (preserved) throw new Error('checkpoint refresh unavailable');
      return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    }
    if (command === 'inspect_managed_file') {
      return JSON.stringify({
        path: 'kept.txt',
        text: 'before\n',
        text_editable: true,
        modified_from_current_version: false,
        current_version: version,
        byte_count: 7,
        content_digest: 'digest-before',
      });
    }
    if (command === 'preserve_managed_text') {
      assert.equal(
        parameters.expectedContentDigest,
        'digest-before',
        'the native write was not bound to the inspected bytes',
      );
      preserved = true;
      return JSON.stringify({ stable_after_idle: true, recovery: 'recovery-after' });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify({ ...workspace, records: preserved ? 2 : 1 });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?preserve-refresh=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const path = document.getElementById('manage-path');
  path.value = 'later.txt';
  await path.emit('input');
  assert.equal(document.getElementById('create-text-entry').disabled, false);

  const file = document.getElementById('edit-file');
  file.value = 'kept.txt';
  await file.emit('change');
  await document.getElementById('load-file').emit('click');
  await waitFor(() => document.workspaceWork?.workbench.changes.editorKind === 'text');
  const editor = document.getElementById('file-editor');
  editor.value = 'after\n';
  await editor.emit('input');
  await document.getElementById('save-file').emit('click');

  assert.equal(preserved, true, 'the native working-copy mutation completed');
  assert.equal(checkpointReads, 2, 'the completed mutation never verified checkpoint truth');
  assert.equal(
    document.getElementById('create-text-entry').disabled,
    true,
    'later writes were authorized after an unverified working-copy mutation',
  );
  assert.match(document.getElementById('notice').textContent, /Refresh succeeds/);
});

test('a verified working-copy save carries its resulting digest into the private save', async () => {
  const document = fakeDocument();
  const version = '01CURRENTVERSION00000000000';
  const workspace = {
    root: '/managed/project',
    digest: 'state-project',
    installation: 'state-project',
    records: 1,
    private_version: { version },
    shared_version: null,
    entries: [{ path: 'kept.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'kept.txt',
      object_id: '01OBJECT0000000000000000000',
      current: { version_id: version, manifest_id: 'manifest-current' },
      retained_versions: [{ version_id: version, manifest_id: 'manifest-current' }],
    }],
  };
  let privateSaveCalled = false;
  let privateSaveReinspected = false;
  let preserveCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    }
    if (command === 'inspect_managed_file') {
      if (privateSaveCalled) {
        assert.equal(parameters.expectedWorkspaceDigest, 'state-project-after-private-save');
        privateSaveReinspected = true;
        return JSON.stringify({
          path: 'kept.txt',
          text: 'after\n',
          text_editable: true,
          modified_from_current_version: false,
          current_version: '01NEWVERSION000000000000000',
          byte_count: 6,
          content_digest: 'digest-after',
        });
      }
      return JSON.stringify({
        path: 'kept.txt',
        text: 'before\n',
        text_editable: true,
        modified_from_current_version: false,
        current_version: version,
        byte_count: 7,
        content_digest: 'digest-before',
      });
    }
    if (command === 'preserve_managed_text') {
      preserveCalls += 1;
      assert.equal(parameters.expectedContentDigest, 'digest-before');
      return JSON.stringify({
        stable_after_idle: true,
        recovery: 'recovery-after',
        content_digest: 'digest-after',
      });
    }
    if (command === 'save_managed_private') {
      assert.equal(
        parameters.expectedContentDigest,
        'digest-after',
        'the private save was authorized with the stale pre-edit digest',
      );
      privateSaveCalled = true;
      // A real private save appends an authenticated record, so the workspace identity observed by
      // the mandatory post-mutation refresh cannot remain equal to the pre-save fold digest.
      workspace.digest = 'state-project-after-private-save';
      return JSON.stringify({
        path: 'kept.txt',
        version: '01NEWVERSION000000000000000',
        manifest: 'manifest-after',
        changeset: 'changeset-after',
        stable_after_idle: true,
        saved_privately: true,
        author_authenticated: true,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?working-copy-private-save=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const file = document.getElementById('edit-file');
  file.value = 'kept.txt';
  await file.emit('change');
  await document.getElementById('load-file').emit('click');
  await waitFor(() => document.workspaceWork?.workbench.changes.editorKind === 'text');
  const editor = document.getElementById('file-editor');
  const mountedEditorGeneration = document.workspaceWork.generation;
  editor.value = 'after\n';
  await editor.emit('input');
  assert.ok(document.workspaceWork.generation > mountedEditorGeneration);
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-intent', {
    detail: {
      generation: mountedEditorGeneration,
      intent: { type: 'activate', action: 'preserve-edit', field: 'editorText', value: 'forged draft\n' },
    },
  }));
  assert.equal(preserveCalls, 0, 'a mismatched delayed editor action reached the native write');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-intent', {
    detail: {
      generation: mountedEditorGeneration,
      intent: { type: 'activate', action: 'preserve-edit', field: 'editorText', value: 'after\n' },
    },
  }));
  await waitFor(() => preserveCalls === 1);
  await waitFor(() => document.getElementById('save-private').disabled === false);
  assert.equal(document.getElementById('save-private').disabled, false);
  await document.getElementById('save-private').emit('click');

  assert.equal(privateSaveCalled, true, 'the just-preserved bytes could not be saved privately');
  assert.equal(privateSaveReinspected, true, 'the saved file was not reopened under the new workspace identity');
  assert.doesNotMatch(
    document.getElementById('notice').textContent,
    /Cannot set properties of null/,
    'the successful private save dereferenced editor state cleared by its own digest transition',
  );
  assert.match(document.getElementById('edit-state').textContent, /Saved privately/);
});

test('a lost working-copy save reply verifies the exact edited bytes without replaying the write', async () => {
  const document = fakeDocument();
  const version = '01CURRENTVERSION00000000000';
  const workspace = {
    root: '/managed/lost-editor-save',
    digest: 'state-lost-editor-save',
    installation: 'installation-lost-editor-save',
    records: 1,
    private_version: { version },
    shared_version: null,
    entries: [{ path: 'kept.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'kept.txt',
      object_id: '01OBJECT0000000000000000000',
      current: { version_id: version, manifest_id: 'manifest-current' },
      retained_versions: [{ version_id: version, manifest_id: 'manifest-current' }],
    }],
  };
  const editedText = 'after the reply was lost\n';
  let written = false;
  let preserveCalls = 0;
  let inspections = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: written,
      });
    }
    if (command === 'inspect_managed_file') {
      inspections += 1;
      assert.equal(parameters.relativePath, 'kept.txt');
      return JSON.stringify({
        path: 'kept.txt',
        text: written ? editedText : 'before\n',
        text_editable: true,
        native_untracked: false,
        modified_from_current_version: written,
        current_version: version,
        byte_count: written ? editedText.length : 7,
        content_digest: written ? 'digest-after-lost-reply' : 'digest-before',
        executable: false,
      });
    }
    if (command === 'preserve_managed_text') {
      preserveCalls += 1;
      assert.equal(parameters.text, editedText);
      assert.equal(parameters.expectedContentDigest, 'digest-before');
      written = true;
      throw new Error('working-copy reply lost after atomic replacement');
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    throw new Error(`unexpected lost-editor-save command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?lost-editor-save=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const file = document.getElementById('edit-file');
  file.value = 'kept.txt';
  await file.emit('change');
  await document.getElementById('load-file').emit('click');
  const editor = document.getElementById('file-editor');
  editor.value = editedText;
  await editor.emit('input');
  await document.getElementById('save-file').emit('click');

  assert.equal(preserveCalls, 1, 'an ambiguous atomic write was replayed');
  assert.equal(inspections, 2, 'Mesh did not re-read the exact working-copy path once');
  assert.equal(editor.value, editedText);
  assert.equal(document.getElementById('save-file').disabled, true);
  assert.equal(document.getElementById('save-private').disabled, false);
  assert.match(document.getElementById('edit-state').textContent, /local folder change detected/);
  assert.match(document.getElementById('notice').textContent, /lost.*reply.*confirmed.*exact edited text/i);
});

test('an agent-created native file is inspected and adopted into private history', async () => {
  const document = fakeDocument();
  const nativeDigest = '12'.repeat(32);
  const version = '01AGENTVERSION0000000000000';
  const workspace = {
    root: '/managed/agent-project/mounts',
    digest: 'workspace-before-agent-adoption',
    installation: 'installation-agent-project',
    records: 1,
    private_version: { version: 'private-before-agent-adoption' },
    shared_version: null,
    entries: [{ path: 'existing.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'existing.txt',
      object_id: '01EXISTINGOBJECT000000000000',
      current: { version_id: 'existing-version', manifest_id: 'existing-manifest' },
      retained_versions: [{ version_id: 'existing-version', manifest_id: 'existing-manifest' }],
    }],
    native_untracked_files: ['agent-notes.md'],
  };
  let inspected = false;
  let adopted = false;
  let reinspected = false;
  let workspaceReads = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      workspaceReads += 1;
      if (workspaceReads === 2) workspace.native_untracked_files = ['agent-notes.md'];
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories') return JSON.stringify([]);
    if (command === 'discover_native_missing_files') return JSON.stringify([]);
    if (command === 'inspect_native_file') {
      assert.equal(parameters.relativePath, 'agent-notes.md');
      assert.equal(parameters.expectedWorkspaceDigest, 'workspace-before-agent-adoption');
      inspected = true;
      return JSON.stringify({
        path: 'agent-notes.md',
        text: '# agent result\n',
        text_editable: false,
        native_untracked: true,
        byte_count: 15,
        content_digest: nativeDigest,
        executable: false,
      });
    }
    if (command === 'adopt_native_file') {
      assert.equal(parameters.relativePath, 'agent-notes.md');
      assert.equal(parameters.expectedContentDigest, nativeDigest);
      assert.equal(parameters.expectedExecutable, false);
      adopted = true;
      workspace.digest = 'workspace-after-agent-adoption';
      workspace.records = 2;
      workspace.native_untracked_files = [];
      workspace.entries = [{ path: 'agent-notes.md', type: 'file' }];
      workspace.file_histories = [{
        path: 'agent-notes.md',
        object_id: '01AGENTOBJECT00000000000000',
        current: { version_id: version, manifest_id: 'manifest-agent' },
        retained_versions: [{ version_id: version, manifest_id: 'manifest-agent' }],
      }];
      return JSON.stringify({
        path: 'agent-notes.md',
        version,
        manifest: 'manifest-agent',
        changeset: 'changeset-agent',
        stable_after_idle: true,
        saved_privately: true,
        author_authenticated: true,
      });
    }
    if (command === 'inspect_managed_file') {
      if (parameters.relativePath === 'existing.txt') {
        assert.equal(parameters.expectedWorkspaceDigest, 'workspace-before-agent-adoption');
        return JSON.stringify({
          path: 'existing.txt',
          text: 'unchanged\n',
          text_editable: true,
          modified_from_current_version: false,
          current_version: 'existing-version',
          byte_count: 10,
          content_digest: '34'.repeat(32),
          executable: false,
        });
      }
      assert.equal(parameters.expectedWorkspaceDigest, 'workspace-after-agent-adoption');
      reinspected = true;
      return JSON.stringify({
        path: 'agent-notes.md',
        text: '# agent result\n',
        text_editable: true,
        modified_from_current_version: false,
        current_version: version,
        byte_count: 15,
        content_digest: nativeDigest,
        executable: false,
      });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?agent-file-adoption=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.getElementById('scan-files').emit('click');
  assert.equal(workspaceReads, 2, 'folder scan did not refresh the daemon inventory');
  assert.equal(inspected, true);
  assert.equal(document.getElementById('edit-file').value, 'agent-notes.md');
  assert.equal(document.getElementById('file-editor').disabled, true);
  assert.equal(document.workspaceWork.workbench.changes.editorKind, 'text');
  assert.equal(document.workspaceWork.workbench.changes.editorText, '# agent result\n');
  assert.equal(document.workspaceWork.workbench.changes.baselineText, '');
  assert.equal(document.workspaceWork.workbench.changes.canEditText, false);
  assert.equal(document.getElementById('save-private').disabled, false);
  assert.match(document.getElementById('notice').textContent, /1 native change was found/);

  await document.getElementById('save-private').emit('click');
  assert.equal(adopted, true);
  assert.equal(reinspected, true);
  assert.equal(workspace.native_untracked_files.length, 0);
  assert.match(document.getElementById('edit-state').textContent, /Saved privately/);
  assert.match(document.getElementById('notice').textContent, /durable and survive restart/);
});

test('returning from a native editor discovers changes without another scan command', async () => {
  const document = fakeDocument();
  document.visibilityState = 'visible';
  const listeners = new Map();
  const workspace = {
    root: '/managed/focus-scan/mounts',
    digest: 'workspace-focus-scan',
    installation: 'installation-focus-scan',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'focus-version-0' },
    shared_version: null,
    entries: [{ path: 'agent.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: [{
      path: 'agent.txt',
      object_id: '01FOCUSOBJECT000000000000000',
      current: { version_id: 'focus-version-0', manifest_id: 'focus-manifest-0' },
      retained_versions: [{ version_id: 'focus-version-0', manifest_id: 'focus-manifest-0' }],
    }],
    workspace_versions: [],
  };
  let inspections = 0;
  let saves = 0;
  let releaseSecondInspection;
  let scheduledScan = null;
  let scheduledDelay = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      remembered: workspace.root,
      workspaces: [workspace.root],
      auto_opened: false,
      active_folder: '/application/native-workspace/current',
      export_root: null,
      warning: null,
    });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories') return JSON.stringify([]);
    if (command === 'discover_native_missing_files') return JSON.stringify([]);
    if (command === 'inspect_managed_file') {
      inspections += 1;
      if (inspections === 3) {
        await new Promise((resolve) => {
          releaseSecondInspection = resolve;
        });
      }
      return JSON.stringify({
        path: 'agent.txt',
        text: 'changed by agent\n',
        text_editable: true,
        native_untracked: false,
        modified_from_current_version: true,
        current_version: 'focus-version-0',
        byte_count: 17,
        content_digest: '91'.repeat(32),
        executable: false,
      });
    }
    if (command === 'reveal_managed_workspace') {
      await window.emit('focus');
      return JSON.stringify({
        path: '/application/native-workspace/current',
        workspace_root: workspace.root,
        native_folder: true,
        stable: true,
      });
    }
    if (command === 'save_managed_private') {
      saves += 1;
      throw new Error('a return-to-app scan must never save automatically');
    }
    throw new Error(`unexpected focus-scan command: ${command}`);
  };
  const window = {
    __TAURI__: { core: { invoke } },
    addEventListener(name, listener) {
      const named = listeners.get(name) || [];
      named.push(listener);
      listeners.set(name, named);
    },
    setInterval(listener, delay) {
      scheduledScan = listener;
      scheduledDelay = delay;
      return 1;
    },
    async emit(name) {
      for (const listener of listeners.get(name) || []) await listener({});
    },
  };

  globalThis.document = document;
  globalThis.window = window;
  globalThis.confirm = () => true;
  await import(`./app.js?automatic-focus-scan=${Date.now()}`);
  for (let turn = 0; turn < 10; turn += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  assert.equal(
    document.serviceState.state === 'ready',
    true,
    document.getElementById('notice').textContent,
  );
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Continue in the native folder');

  assert.equal(inspections, 0, 'startup should not invent a native edit');
  assert.equal(scheduledDelay, 5_000, 'visible native work did not receive a bounded refresh timer');
  document.visibilityState = 'hidden';
  scheduledScan();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(inspections, 0, 'a hidden window performed a full native scan');
  document.visibilityState = 'visible';
  scheduledScan();
  await waitFor(() => inspections === 1);
  assert.match(document.getElementById('notice').textContent, /noticed new native work/);
  await Promise.all([window.emit('focus'), window.emit('focus')]);
  await waitFor(() => inspections === 2
    && document.workspaceOverview?.overview.nextActionTitle === 'Review 1 native change');
  assert.equal(inspections, 2, 'one return to the app started overlapping folder walks');
  assert.equal(saves, 0);
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Review 1 native change');
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Inspect changes');
  assert.equal(document.getElementById('folder-change-items').children.length, 1);
  assert.match(document.getElementById('folder-change-items').children[0].textContent, /agent\.txt/);
  assert.match(document.getElementById('notice').textContent, /returned to Mesh/);
  assert.equal(!document.workspaceCurrentAction('open-folder')?.enabled, false, 'verified scan left native navigation disabled');
  assert.equal(!document.workspaceCurrentAction('start-codex')?.enabled, false, 'verified scan left the agent handoff disabled');

  const editor = document.getElementById('file-editor');
  const rescanning = window.emit('focus');
  await waitFor(() => typeof releaseSecondInspection === 'function');
  editor.value = 'draft typed while the native read is pending\n';
  await editor.emit('input');
  assert.equal(document.getElementById('save-file').disabled, false);
  releaseSecondInspection();
  await rescanning;
  assert.equal(inspections, 3);
  assert.equal(editor.value, 'draft typed while the native read is pending\n');
  assert.equal(document.getElementById('save-file').disabled, false);
  assert.match(document.getElementById('notice').textContent, /kept the file already open/);

  await document.emitWorkspaceCurrentIntent('open-folder');
  assert.equal(inspections, 3, 'the folder opener focus transition started a competing scan');
  assert.match(document.getElementById('notice').textContent, /Opened the stable native folder/);
});

test('a restored workspace surfaces tracked and new native work without a leave-and-return cycle', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/restored-native-work',
    digest: 'workspace-restored-native-work',
    installation: 'installation-restored-native-work',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'restored-version-0' },
    shared_version: null,
    entries: [{ path: 'tracked.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: ['agent-alpha-notes.md'],
    file_histories: [{
      path: 'tracked.txt',
      object_id: '01RESTOREDTRACKED00000000000',
      current: { version_id: 'tracked-version-0', manifest_id: 'tracked-manifest-0' },
      retained_versions: [{ version_id: 'tracked-version-0', manifest_id: 'tracked-manifest-0' }],
    }],
    workspace_versions: [],
  };
  let inspections = 0;
  let trackedInspections = 0;
  let saves = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      remembered: workspace.root,
      workspaces: [workspace.root],
      auto_opened: true,
      active_folder: null,
      export_root: null,
      warning: null,
    });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories') return JSON.stringify([]);
    if (command === 'discover_native_missing_files') return JSON.stringify([]);
    if (command === 'inspect_native_file') {
      inspections += 1;
      return JSON.stringify({
        path: 'agent-alpha-notes.md',
        text: 'made before reopening Mesh\n',
        text_editable: true,
        native_untracked: true,
        modified_from_current_version: true,
        current_version: null,
        byte_count: 26,
        content_digest: '97'.repeat(32),
        executable: false,
      });
    }
    if (command === 'inspect_managed_file') {
      trackedInspections += 1;
      return JSON.stringify({
        path: 'tracked.txt',
        text: 'tracked file changed while Mesh was closed\n',
        text_editable: true,
        native_untracked: false,
        modified_from_current_version: true,
        current_version: 'tracked-version-0',
        byte_count: 40,
        content_digest: '96'.repeat(32),
        executable: false,
      });
    }
    if (command === 'save_managed_private') {
      saves += 1;
      throw new Error('startup discovery must never save automatically');
    }
    throw new Error(`unexpected restored-workspace command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, addEventListener() {} };
  globalThis.confirm = () => true;
  await import(`./app.js?restored-native-work=${Date.now()}`);
  await waitFor(() => document.getElementById('folder-change-items').children.length === 2);

  assert.equal(inspections, 1);
  assert.equal(trackedInspections, 1);
  assert.equal(saves, 0);
  assert.equal(document.getElementById('folder-change-items').children.length, 2);
  assert.match(document.getElementById('folder-change-items').children[0].textContent, /tracked\.txt/);
  assert.match(document.getElementById('folder-change-items').children[1].textContent, /agent-alpha-notes\.md/);
  assert.match(document.getElementById('notice').textContent, /native changes were found/);
});

test('a restored workspace inspects files concurrently within a fixed bound and preserves queue order', async () => {
  const document = fakeDocument();
  const paths = Array.from({ length: 24 }, (_, index) => `file-${String(index).padStart(2, '0')}.txt`);
  const workspace = {
    root: '/managed/restored-many-files',
    digest: 'workspace-restored-many-files',
    installation: 'installation-restored-many-files',
    records: paths.length,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'restored-many-version', concurrent_changes: 1 },
    shared_version: null,
    entries: paths.map((path) => ({ path, type: 'file' })),
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    file_histories: paths.map((path, index) => ({
      path,
      object_id: `01RESTOREDMANY${String(index).padStart(10, '0')}`,
      current: { version_id: `version-${index}`, manifest_id: `manifest-${index}` },
      retained_versions: [{ version_id: `version-${index}`, manifest_id: `manifest-${index}` }],
    })),
    workspace_versions: [],
  };
  let activeInspections = 0;
  let maximumInspections = 0;
  let saves = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      remembered: workspace.root,
      workspaces: [workspace.root],
      auto_opened: true,
      active_folder: '/application/native-workspace/current',
      export_root: null,
      warning: null,
    });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'reconcile_managed_workspace_navigation') return JSON.stringify({
      path: '/application/native-workspace/current',
      workspace_root: workspace.root,
      stable: true,
      native_folder: true,
    });
    if (command === 'discover_native_directories') return JSON.stringify([]);
    if (command === 'discover_native_missing_files') return JSON.stringify([]);
    if (command === 'inspect_managed_file') {
      activeInspections += 1;
      maximumInspections = Math.max(maximumInspections, activeInspections);
      await new Promise((resolve) => setTimeout(resolve, 2));
      activeInspections -= 1;
      return JSON.stringify({
        path: parameters.relativePath,
        text: `${parameters.relativePath} changed while Mesh was closed\n`,
        text_editable: true,
        native_untracked: false,
        modified_from_current_version: true,
        current_version: `current-${parameters.relativePath}`,
        byte_count: 48,
        content_digest: '95'.repeat(32),
        executable: false,
      });
    }
    if (command === 'save_managed_private') {
      saves += 1;
      throw new Error('read-only restored-tree discovery must never save');
    }
    throw new Error(`unexpected restored-many-files command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, addEventListener() {} };
  globalThis.confirm = () => true;
  await import(`./app.js?restored-many-files=${Date.now()}`);
  await waitFor(() => document.getElementById('folder-change-items').children.length === paths.length);

  assert.equal(saves, 0);
  assert.ok(maximumInspections > 1, `restored scan remained sequential (${maximumInspections})`);
  assert.ok(maximumInspections <= 8, `restored scan exceeded its concurrency bound (${maximumInspections})`);
  assert.deepEqual(
    document.getElementById('folder-change-items').children.map((item) => item.textContent.split(' · ')[0]),
    paths,
  );
});

test('one save drains a reviewed agent-created tree in parent-first order', async () => {
  const document = fakeDocument();
  const directoryInstallation = '77'.repeat(32);
  const fileDigest = '88'.repeat(32);
  const workspace = {
    root: '/managed/agent-directory/mounts',
    digest: 'workspace-agent-directory-0',
    installation: 'installation-agent-directory',
    records: 1,
    private_version: { version: 'private-agent-directory-0' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
    native_untracked_files: ['generated/result.txt'],
  };
  const directoryAdoptions = [];
  let fileAdoptions = 0;
  let fileInspections = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories') {
      return JSON.stringify([
        { path: 'generated', installation: directoryInstallation },
        { path: 'generated/reports', installation: '99'.repeat(32) },
      ].filter((entry) => !directoryAdoptions.includes(entry.path)));
    }
    if (command === 'discover_native_missing_files') return JSON.stringify([]);
    if (command === 'adopt_native_directory') {
      const expectedPath = directoryAdoptions.length === 0 ? 'generated' : 'generated/reports';
      assert.equal(parameters.relativePath, expectedPath);
      assert.equal(
        parameters.expectedDirectoryInstallation,
        expectedPath === 'generated' ? directoryInstallation : '99'.repeat(32),
      );
      assert.equal(parameters.expectedWorkspaceDigest, `workspace-agent-directory-${directoryAdoptions.length}`);
      directoryAdoptions.push(parameters.relativePath);
      workspace.digest = `workspace-agent-directory-${directoryAdoptions.length}`;
      workspace.records = 1 + directoryAdoptions.length;
      workspace.private_version = { version: `private-agent-directory-${directoryAdoptions.length}` };
      workspace.entries.push({ path: parameters.relativePath, type: 'directory' });
      return JSON.stringify({
        action: 'adopt_folder',
        to_path: parameters.relativePath,
        changeset: `directory-changeset-${directoryAdoptions.length}`,
        saved_privately: true,
        author_authenticated: true,
      });
    }
    if (command === 'inspect_native_file') {
      assert.equal(parameters.relativePath, 'generated/result.txt');
      assert.ok([
        'workspace-agent-directory-0',
        'workspace-agent-directory-2',
      ].includes(parameters.expectedWorkspaceDigest));
      fileInspections += 1;
      return JSON.stringify({
        path: 'generated/result.txt',
        text: 'agent output\n',
        text_editable: false,
        native_untracked: true,
        byte_count: 13,
        content_digest: fileDigest,
        executable: false,
      });
    }
    if (command === 'inspect_managed_file') {
      assert.equal(parameters.relativePath, 'generated/result.txt');
      assert.equal(parameters.expectedWorkspaceDigest, 'workspace-agent-directory-3');
      return JSON.stringify({
        path: 'generated/result.txt',
        text: 'agent output\n',
        text_editable: true,
        native_untracked: false,
        modified_from_current_version: false,
        current_version: 'directory-file-version',
        byte_count: 13,
        content_digest: fileDigest,
        executable: false,
      });
    }
    if (command === 'adopt_native_file') {
      assert.equal(parameters.relativePath, 'generated/result.txt');
      assert.equal(parameters.expectedContentDigest, fileDigest);
      assert.equal(parameters.expectedWorkspaceDigest, 'workspace-agent-directory-2');
      fileAdoptions += 1;
      workspace.digest = 'workspace-agent-directory-3';
      workspace.records = 4;
      workspace.private_version = { version: 'private-agent-directory-3' };
      workspace.entries.push({ path: 'generated/result.txt', type: 'file' });
      workspace.native_untracked_files = [];
      workspace.file_histories = [{
        path: 'generated/result.txt',
        object_id: '01AGENTDIRECTORYFILE00000000',
        current: { version_id: 'directory-file-version', manifest_id: 'directory-file-manifest' },
        retained_versions: [{ version_id: 'directory-file-version', manifest_id: 'directory-file-manifest' }],
      }];
      return JSON.stringify({
        path: 'generated/result.txt',
        version: 'directory-file-version',
        manifest: 'directory-file-manifest',
        changeset: 'directory-file-changeset',
        stable_after_idle: true,
        saved_privately: true,
        author_authenticated: true,
      });
    }
    throw new Error(`unexpected agent-directory command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?agent-directory-adoption=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.getElementById('scan-files').emit('click');
  assert.equal(document.getElementById('folder-change-items').children.length, 3);
  assert.match(document.getElementById('folder-change-items').children[0].textContent, /new native folder/);

  await document.getElementById('save-all-private').emit('click');
  assert.deepEqual(directoryAdoptions, ['generated', 'generated/reports']);
  assert.equal(fileAdoptions, 1);
  assert.equal(fileInspections, 2, 'the nested file was not reviewed and re-inspected immediately before saving');
  assert.equal(document.getElementById('folder-change-queue').classList.contains('hidden'), true);
  assert.match(document.getElementById('notice').textContent, /2 new native folders/);
  assert.match(document.getElementById('notice').textContent, /1 changed file/);
});

test('one explicit action saves its verified queue and keeps work arriving during the save visible', async () => {
  const document = fakeDocument();
  const trackedDigest = '56'.repeat(32);
  const nativeDigest = '78'.repeat(32);
  const workspace = {
    root: '/managed/agent-batch/mounts',
    digest: 'workspace-agent-batch-0',
    installation: 'installation-agent-batch',
    records: 1,
    private_version: { version: 'private-agent-batch-0' },
    shared_version: null,
    entries: [{ path: 'tracked.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'tracked.txt',
      object_id: '01TRACKEDBATCH0000000000000',
      current: { version_id: 'tracked-version-0', manifest_id: 'tracked-manifest-0' },
      retained_versions: [{ version_id: 'tracked-version-0', manifest_id: 'tracked-manifest-0' }],
    }],
    native_untracked_files: ['agent-new.txt'],
  };
  const inspectedAgainst = [];
  const savedAgainst = [];
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories') return JSON.stringify([]);
    if (command === 'discover_native_missing_files') return JSON.stringify([]);
    if (command === 'inspect_managed_file') {
      inspectedAgainst.push([parameters.relativePath, parameters.expectedWorkspaceDigest]);
      const alreadySaved = parameters.expectedWorkspaceDigest === 'workspace-agent-batch-2';
      return JSON.stringify({
        path: parameters.relativePath,
        text: parameters.relativePath === 'tracked.txt' ? 'agent changed tracked\n' : 'agent new\n',
        text_editable: true,
        native_untracked: false,
        modified_from_current_version: !alreadySaved,
        current_version: parameters.relativePath === 'tracked.txt'
          ? workspace.file_histories[0].current.version_id
          : 'native-version-1',
        byte_count: parameters.relativePath === 'tracked.txt' ? 22 : 10,
        content_digest: parameters.relativePath === 'tracked.txt' ? trackedDigest : nativeDigest,
        executable: false,
      });
    }
    if (command === 'inspect_native_file') {
      inspectedAgainst.push([parameters.relativePath, parameters.expectedWorkspaceDigest]);
      const arrivedDuringSave = parameters.relativePath === 'arrived-during-save.txt';
      return JSON.stringify({
        path: parameters.relativePath,
        text: arrivedDuringSave ? 'arrived during save\n' : 'agent new\n',
        text_editable: false,
        native_untracked: true,
        byte_count: arrivedDuringSave ? 20 : 10,
        content_digest: arrivedDuringSave ? '9a'.repeat(32) : nativeDigest,
        executable: false,
      });
    }
    if (command === 'save_managed_private') {
      savedAgainst.push(['tracked.txt', parameters.expectedWorkspaceDigest]);
      assert.equal(parameters.expectedContentDigest, trackedDigest);
      workspace.digest = 'workspace-agent-batch-1';
      workspace.records = 2;
      workspace.private_version = { version: 'private-agent-batch-1' };
      workspace.file_histories[0].current = { version_id: 'tracked-version-1', manifest_id: 'tracked-manifest-1' };
      return JSON.stringify({
        path: 'tracked.txt',
        version: 'tracked-version-1',
        manifest: 'tracked-manifest-1',
        changeset: 'tracked-changeset-1',
        stable_after_idle: true,
        saved_privately: true,
        author_authenticated: true,
      });
    }
    if (command === 'adopt_native_file') {
      savedAgainst.push(['agent-new.txt', parameters.expectedWorkspaceDigest]);
      assert.equal(parameters.expectedContentDigest, nativeDigest);
      workspace.digest = 'workspace-agent-batch-2';
      workspace.records = 3;
      workspace.private_version = { version: 'private-agent-batch-2' };
      workspace.native_untracked_files = ['arrived-during-save.txt'];
      workspace.entries.push({ path: 'agent-new.txt', type: 'file' });
      workspace.file_histories.push({
        path: 'agent-new.txt',
        object_id: '01NATIVEBATCH00000000000000',
        current: { version_id: 'native-version-1', manifest_id: 'native-manifest-1' },
        retained_versions: [{ version_id: 'native-version-1', manifest_id: 'native-manifest-1' }],
      });
      return JSON.stringify({
        path: 'agent-new.txt',
        version: 'native-version-1',
        manifest: 'native-manifest-1',
        changeset: 'native-changeset-1',
        stable_after_idle: true,
        saved_privately: true,
        author_authenticated: true,
      });
    }
    throw new Error(`unexpected batch command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?agent-batch=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.getElementById('scan-files').emit('click');
  assert.equal(document.getElementById('folder-change-queue').classList.contains('hidden'), false);
  assert.equal(document.getElementById('folder-change-items').children.length, 2);
  assert.equal(document.getElementById('save-all-private').disabled, false);

  await document.getElementById('save-all-private').emit('click');
  assert.deepEqual(savedAgainst, [
    ['tracked.txt', 'workspace-agent-batch-0'],
    ['agent-new.txt', 'workspace-agent-batch-1'],
  ]);
  assert.deepEqual(inspectedAgainst, [
    ['tracked.txt', 'workspace-agent-batch-0'],
    ['agent-new.txt', 'workspace-agent-batch-0'],
    ['tracked.txt', 'workspace-agent-batch-0'],
    ['agent-new.txt', 'workspace-agent-batch-1'],
    ['tracked.txt', 'workspace-agent-batch-2'],
    ['agent-new.txt', 'workspace-agent-batch-2'],
    ['arrived-during-save.txt', 'workspace-agent-batch-2'],
  ]);
  assert.equal(workspace.digest, 'workspace-agent-batch-2');
  assert.equal(document.getElementById('folder-change-queue').classList.contains('hidden'), false);
  assert.equal(document.getElementById('folder-change-items').children.length, 1);
  assert.match(document.getElementById('folder-change-items').children[0].textContent, /arrived-during-save\.txt/);
  assert.equal((document.workspaceCurrent?.current?.state ?? ''), 'Working');
  assert.match(document.getElementById('notice').textContent, /2 changed files were authenticated and saved privately/);
  assert.match(document.getElementById('notice').textContent, /1 newer native change remains ready/);
});

test('a multi-file save stops before a file that changed after the scan', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/agent-batch-race/mounts',
    digest: 'workspace-agent-race-0',
    installation: 'installation-agent-race',
    records: 1,
    private_version: { version: 'private-agent-race-0' },
    shared_version: null,
    entries: [{ path: 'first.txt', type: 'file' }, { path: 'second.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: ['first.txt', 'second.txt'].map((path, index) => ({
      path,
      object_id: `01RACEOBJECT00000000000000${index}`,
      current: { version_id: `${path}-version-0`, manifest_id: `${path}-manifest-0` },
      retained_versions: [{ version_id: `${path}-version-0`, manifest_id: `${path}-manifest-0` }],
    })),
    native_untracked_files: [],
  };
  const inspections = new Map();
  const saved = [];
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories') return JSON.stringify([]);
    if (command === 'discover_native_missing_files') return JSON.stringify([]);
    if (command === 'inspect_managed_file') {
      const count = (inspections.get(parameters.relativePath) || 0) + 1;
      inspections.set(parameters.relativePath, count);
      if (parameters.relativePath === 'second.txt' && count === 2) {
        throw new Error('content digest changed after scan');
      }
      const alreadySaved = parameters.relativePath === 'first.txt'
        && workspace.digest === 'workspace-agent-race-1';
      return JSON.stringify({
        path: parameters.relativePath,
        text: `${parameters.relativePath} changed\n`,
        text_editable: true,
        native_untracked: false,
        modified_from_current_version: !alreadySaved,
        current_version: `${parameters.relativePath}-version-0`,
        byte_count: 18,
        content_digest: parameters.relativePath === 'first.txt' ? '90'.repeat(32) : 'ab'.repeat(32),
        executable: false,
      });
    }
    if (command === 'save_managed_private') {
      saved.push(parameters.relativePath);
      if (parameters.relativePath !== 'first.txt') throw new Error('second file must not be saved');
      workspace.digest = 'workspace-agent-race-1';
      workspace.records = 2;
      workspace.private_version = { version: 'private-agent-race-1' };
      workspace.file_histories[0].current = {
        version_id: 'first-version-1',
        manifest_id: 'first-manifest-1',
      };
      return JSON.stringify({
        path: 'first.txt',
        version: 'first-version-1',
        manifest: 'first-manifest-1',
        changeset: 'first-changeset-1',
        stable_after_idle: true,
        saved_privately: true,
        author_authenticated: true,
      });
    }
    throw new Error(`unexpected batch-race command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?agent-batch-race=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.getElementById('scan-files').emit('click');
  await document.getElementById('save-all-private').emit('click');
  assert.deepEqual(saved, ['first.txt']);
  assert.equal(workspace.digest, 'workspace-agent-race-1');
  assert.match(document.getElementById('notice').textContent, /could not inspect second\.txt/i);
  assert.match(document.getElementById('notice').textContent, /remaining queue was refreshed/i);
  assert.equal(document.getElementById('notice').classList.contains('error'), true);
  assert.equal(document.getElementById('folder-change-items').children.length, 1);
  assert.match(document.getElementById('folder-change-items').children[0].textContent, /second\.txt/);
});

test('a reverted scan queue is re-verified without inventing a private save', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/reverted-agent-change/mounts',
    digest: 'workspace-reverted-agent-change',
    installation: 'installation-reverted-agent-change',
    records: 1,
    private_version: { version: 'private-reverted-agent-change' },
    shared_version: null,
    entries: [{ path: 'reverted.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'reverted.txt',
      object_id: '01REVERTEDOBJECT00000000000',
      current: { version_id: 'reverted-version', manifest_id: 'reverted-manifest' },
      retained_versions: [{ version_id: 'reverted-version', manifest_id: 'reverted-manifest' }],
    }],
    native_untracked_files: [],
  };
  let inspectionCount = 0;
  let saveCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories') return JSON.stringify([]);
    if (command === 'discover_native_missing_files') return JSON.stringify([]);
    if (command === 'inspect_managed_file') {
      inspectionCount += 1;
      return JSON.stringify({
        path: 'reverted.txt',
        text: inspectionCount === 1 ? 'temporary change\n' : 'saved bytes\n',
        text_editable: true,
        native_untracked: false,
        modified_from_current_version: inspectionCount === 1,
        current_version: 'reverted-version',
        byte_count: 12,
        content_digest: inspectionCount === 1 ? 'bc'.repeat(32) : 'cd'.repeat(32),
        executable: false,
      });
    }
    if (command === 'save_managed_private') {
      saveCalls += 1;
      throw new Error('a reverted file must not be saved again');
    }
    throw new Error(`unexpected reverted-queue command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?reverted-agent-queue=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.getElementById('scan-files').emit('click');
  await document.getElementById('save-all-private').emit('click');
  assert.equal(saveCalls, 0);
  assert.equal(document.serviceState.state === 'ready', true);
  assert.match(document.getElementById('notice').textContent, /No queued file needed saving; 1 file already matches private history/);
});

test('native rename and deletion recover lost replies without replay and preserve exact identity inputs', async () => {
  const document = fakeDocument();
  const movedDigest = 'de'.repeat(32);
  const movedAndEditedDigest = 'ef'.repeat(32);
  const deletedDigest = 'ad'.repeat(32);
  const workspace = {
    root: '/managed/native-structure/mounts',
    digest: 'workspace-native-structure-0',
    installation: 'installation-native-structure',
    records: 2,
    private_version: { version: 'private-native-structure-0' },
    shared_version: null,
    entries: [
      { path: 'old.txt', type: 'file' },
      { path: 'deleted.txt', type: 'file' },
    ],
    conditions: [],
    not_yet: [],
    file_histories: [
      {
        path: 'old.txt',
        object_id: '01NATIVEMOVEOBJECT000000000',
        current: { version_id: 'move-version', manifest_id: 'move-manifest' },
        retained_versions: [{ version_id: 'move-version', manifest_id: 'move-manifest' }],
      },
      {
        path: 'deleted.txt',
        object_id: '01NATIVEDELETEOBJECT0000000',
        current: { version_id: 'delete-version', manifest_id: 'delete-manifest' },
        retained_versions: [{ version_id: 'delete-version', manifest_id: 'delete-manifest' }],
      },
    ],
    native_untracked_files: ['new.txt'],
  };
  const adopted = [];
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories') return JSON.stringify([]);
    if (command === 'discover_native_missing_files') {
      return JSON.stringify(workspace.file_histories
        .filter((history) => ['old.txt', 'deleted.txt'].includes(history.path))
        .map((history) => ({
          path: history.path,
          current_version: history.current.version_id,
          content_digest: history.path === 'old.txt' ? movedDigest : deletedDigest,
          executable: false,
        })));
    }
    if (command === 'inspect_native_file') {
      assert.equal(parameters.relativePath, 'new.txt');
      return JSON.stringify({
        path: 'new.txt',
        text: 'same retained bytes\n',
        text_editable: false,
        native_untracked: true,
        byte_count: 20,
        content_digest: movedAndEditedDigest,
        executable: false,
      });
    }
    if (command === 'inspect_managed_file') {
      assert.equal(parameters.relativePath, 'new.txt');
      return JSON.stringify({
        path: 'new.txt',
        text: 'same retained bytes\n',
        text_editable: true,
        native_untracked: false,
        modified_from_current_version: true,
        current_version: 'move-version',
        byte_count: 20,
        content_digest: movedAndEditedDigest,
        executable: false,
      });
    }
    if (command === 'adopt_native_file_move') {
      assert.deepEqual(
        {
          fromPath: parameters.fromPath,
          toPath: parameters.toPath,
          version: parameters.expectedCurrentVersion,
          digest: parameters.expectedDestinationContentDigest,
          executable: parameters.expectedDestinationExecutable,
        },
        {
          fromPath: 'old.txt',
          toPath: 'new.txt',
          version: 'move-version',
          digest: movedAndEditedDigest,
          executable: false,
        },
      );
      adopted.push('move');
      workspace.digest = 'workspace-native-structure-1';
      workspace.records += 1;
      workspace.entries[0].path = 'new.txt';
      workspace.file_histories[0].path = 'new.txt';
      workspace.native_untracked_files = [];
      throw new Error('native move reply was lost after commit');
    }
    if (command === 'adopt_native_file_deletion') {
      assert.equal(parameters.relativePath, 'deleted.txt');
      assert.equal(parameters.expectedCurrentVersion, 'delete-version');
      adopted.push('delete');
      workspace.digest = 'workspace-native-structure-2';
      workspace.records += 1;
      workspace.entries = workspace.entries.filter((entry) => entry.path !== 'deleted.txt');
      workspace.file_histories = workspace.file_histories.filter((history) => history.path !== 'deleted.txt');
      throw new Error('native deletion reply was lost after commit');
    }
    throw new Error(`unexpected native structure command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?native-structure=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.emitWorkspaceCurrentIntent('update-destination');
  assert.equal(document.getElementById('workspace-changes-next').scrolledIntoView, true);
  assert.equal(document.getElementById('workspace-changes-next').focused, true);

  await document.getElementById('scan-files').emit('click');
  assert.equal(document.getElementById('save-all-private').disabled, true);
  assert.equal(document.getElementById('native-structural-change').classList.contains('hidden'), false);
  assert.equal(document.getElementById('native-missing-source').value, 'old.txt');
  assert.match(document.getElementById('native-move-target').children[1].textContent, /content also changed/);
  const structuralGeneration = document.workspaceWork.generation;
  await document.emitWorkspaceWorkIntent({
    type: 'set-field',
    field: 'missingSource',
    value: 'forged.txt',
  }, structuralGeneration);
  assert.equal(document.getElementById('native-missing-source').value, 'old.txt');
  await document.emitWorkspaceWorkIntent({
    type: 'activate',
    action: 'record-structural-change',
    missingSource: 'old.txt',
    moveTarget: '',
  }, structuralGeneration - 1);
  assert.deepEqual(adopted, [], 'a stale React queue generation recorded a structural change');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-mounted', {
    detail: { generation: structuralGeneration },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-intent', {
    detail: {
      generation: structuralGeneration,
      intent: { type: 'set-field', field: 'moveTarget', value: 'new.txt' },
    },
  }));
  assert.ok(
    document.workspaceWork.generation > structuralGeneration,
    'the source-owned structural choice did not publish its delayed replacement',
  );
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-files-changes-intent', {
    detail: {
      generation: structuralGeneration,
      intent: {
        type: 'activate',
        action: 'record-structural-change',
        missingSource: 'old.txt',
        moveTarget: 'new.txt',
      },
    },
  }));
  await waitFor(() => adopted.length === 1);
  await waitFor(() => /Nothing was replayed/.test(document.getElementById('notice').textContent));
  assert.deepEqual(adopted, ['move']);
  assert.equal(workspace.file_histories[0].object_id, '01NATIVEMOVEOBJECT000000000');
  assert.equal(document.serviceState.state === 'ready', true);
  assert.ok(document.getElementById('manage-entry').children.some((entry) => entry.value === 'new.txt'));
  assert.match(document.getElementById('notice').textContent, /Nothing was replayed/);
  assert.match(document.getElementById('notice').textContent, /old\.txt absent and new\.txt present as a file/);

  await document.getElementById('scan-files').emit('click');
  assert.equal(document.getElementById('native-missing-source').value, 'deleted.txt');
  assert.equal(document.getElementById('native-move-target').value, '');
  await document.getElementById('record-native-structural-change').emit('click');
  assert.deepEqual(adopted, ['move', 'delete']);
  assert.equal(workspace.file_histories.length, 1);
  assert.equal(document.serviceState.state === 'ready', true);
  assert.ok(!document.getElementById('manage-entry').children.some((entry) => entry.value === 'deleted.txt'));
  assert.match(document.getElementById('notice').textContent, /Nothing was replayed/);
  assert.match(document.getElementById('notice').textContent, /deleted\.txt absent/);
});

test('a managed mutation is bound to the exact workspace the person verified', async () => {
  const document = fakeDocument();
  const workspace = (root, digest) => ({
    root,
    digest,
    installation: `physical-${digest}`,
    records: 1,
    private_version: { version: 'version-1' },
    shared_version: null,
    entries: [{ path: 'kept.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'kept.txt',
      object_id: '01MANAGEDMUTATIONOBJECT000000',
      current: { version_id: 'version-1', manifest_id: 'manifest-1' },
      retained_versions: [{ version_id: 'version-1', manifest_id: 'manifest-1' }],
    }],
  });
  const verified = workspace('/managed/verified', 'digest-verified');
  const replacement = workspace('/managed/replacement', 'digest-replacement');
  const inspectedDigest = '11'.repeat(32);
  let current = verified;
  let deletionPerformed = false;
  let submitted = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: current.root, workspace_digest: current.digest, workspace_installation: current.installation, working: false });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(current);
    }
    if (command === 'inspect_managed_file') {
      assert.equal(parameters.expectedWorkspaceRoot, verified.root);
      assert.equal(parameters.expectedWorkspaceDigest, verified.digest);
      assert.equal(parameters.expectedWorkspaceInstallation, verified.installation);
      return JSON.stringify({
        path: 'kept.txt',
        text: 'kept content\n',
        text_editable: true,
        native_untracked: false,
        modified_from_current_version: false,
        current_version: 'version-1',
        byte_count: 13,
        content_digest: inspectedDigest,
        executable: false,
      });
    }
    if (command === 'delete_managed_entry') {
      submitted = parameters;
      if (
        parameters.expectedWorkspaceRoot !== current.root ||
        parameters.expectedWorkspaceDigest !== current.digest ||
        parameters.expectedWorkspaceInstallation !== current.installation
      ) {
        throw new Error('the verified managed workspace changed before the operation began');
      }
      deletionPerformed = true;
      return JSON.stringify({ author_authenticated: true, saved_privately: true });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?external-workspace-switch=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const filesGeneration = document.workspaceWork.generation;
  await document.emitWorkspaceWorkIntent(
    { type: 'set-field', field: 'selectedEntry', value: 'kept.txt' },
    filesGeneration,
  );
  await waitFor(() => document.workspaceWork.workbench.changes.editorKind === 'text');
  assert.equal(document.workspaceWork.workbench.files.selectedEntry, 'kept.txt');
  assert.equal(
    document.workspaceWork.workbench.changes.editorText,
    'kept content\n',
    'selecting a file in Explorer did not load its exact content into the Files preview',
  );

  const selected = document.getElementById('manage-entry');
  selected.value = 'kept.txt';
  await selected.emit('change');
  assert.equal(document.workspaceWork?.workbench.files.selectedEntry, 'kept.txt');
  assert.equal(document.getElementById('delete-entry').disabled, false);

  // Another authenticated local client replaces the daemon's open workspace after this webview
  // verified A but before the person acts on A's still-visible entry.
  current = replacement;
  await document.getElementById('delete-entry').emit('click');

  assert.equal(submitted?.expectedWorkspaceRoot, verified.root);
  assert.equal(submitted?.expectedWorkspaceDigest, verified.digest);
  assert.equal(submitted?.expectedContentDigest, inspectedDigest);
  assert.equal(
    deletionPerformed,
    false,
    'a control rendered from workspace A deleted the same path in replacement workspace B',
  );
  assert.equal(document.getElementById('create-text-entry').disabled, true);
  assert.match(document.getElementById('notice').textContent, /workspace changed/i);
});

test('delete confirmation cannot cross into a replacement workspace with the same path', async () => {
  const document = fakeDocument();
  const workspace = (root, digest, installation) => ({
    root,
    digest,
    installation,
    records: 1,
    private_version: { version: `version-${digest}` },
    shared_version: null,
    entries: [{ path: 'kept.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [],
  });
  const first = workspace('/managed/first', 'digest-first', 'installation-first');
  const replacement = workspace('/managed/replacement', 'digest-replacement', 'installation-replacement');
  let current = first;
  const deletedRoots = [];
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: current.root,
        workspace_digest: current.digest,
        workspace_installation: current.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(current);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'inspect_managed_file') {
      return JSON.stringify({
        path: 'kept.txt',
        content_digest: current === first ? '11'.repeat(32) : '22'.repeat(32),
        executable: false,
      });
    }
    if (command === 'delete_managed_entry') {
      deletedRoots.push(parameters.expectedWorkspaceRoot);
      return JSON.stringify({ author_authenticated: true, saved_privately: true });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => { throw new Error('the React confirmation should be available'); };
  await import(`./app.js?delete-confirmation-workspace=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const projections = [];
  document.addEventListener('mesh:confirmation-projection', (event) => projections.push(event.detail));
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-available'));
  const selected = document.getElementById('manage-entry');
  selected.value = 'kept.txt';
  await selected.emit('change');
  assert.equal(document.workspaceWork?.workbench.files.selectedEntry, 'kept.txt');
  assert.equal(document.getElementById('delete-entry').disabled, false);

  const deletion = document.getElementById('delete-entry').emit('click');
  await waitFor(() => projections.length === 1);
  assert.equal(projections[0].confirmation.title, 'Delete kept.txt?');
  assert.equal(projections[0].confirmation.tone, 'destructive');

  current = replacement;
  await document.emitWorkspaceCurrentIntent('refresh');
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-mounted', {
    detail: { generation: projections[0].generation },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-intent', {
    detail: { generation: projections[0].generation, intent: { type: 'confirm' } },
  }));
  await deletion;

  assert.deepEqual(deletedRoots, [], 'confirmation for workspace A deleted the same path in workspace B');
  assert.match(document.getElementById('notice').textContent, /workspace changed while delete confirmation was open/i);
  assert.equal(document.getElementById('delete-entry').disabled, true);
  assert.equal(document.workspaceWork?.workbench.files.selectedEntry, '', 'workspace replacement retained a prior physical workspace selection');
});

test('a lost delete reply verifies the exact absent entry without replaying deletion', async () => {
  const document = fakeDocument();
  const before = {
    root: '/managed/delete-recovery',
    digest: 'digest-before-delete',
    installation: 'installation-delete-recovery',
    records: 4,
    private_version: { version: 'version-before-delete' },
    shared_version: null,
    entries: [{ path: 'obsolete.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [],
  };
  const after = {
    ...before,
    digest: 'digest-after-delete',
    records: 5,
    private_version: { version: 'version-after-delete' },
    entries: [],
  };
  let current = before;
  let workspaceReads = 0;
  let deleteCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: current.root,
        workspace_digest: current.digest,
        workspace_installation: current.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      workspaceReads += 1;
      return JSON.stringify(current);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'inspect_managed_file') {
      return JSON.stringify({
        path: 'obsolete.txt',
        content_digest: '33'.repeat(32),
        executable: false,
      });
    }
    if (command === 'delete_managed_entry') {
      deleteCalls += 1;
      assert.equal(parameters.relativePath, 'obsolete.txt');
      assert.equal(parameters.expectedWorkspaceRoot, before.root);
      assert.equal(parameters.expectedWorkspaceDigest, before.digest);
      assert.equal(parameters.expectedWorkspaceInstallation, before.installation);
      current = after;
      throw new Error('delete reply was lost after durable commit');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?lost-delete-reply=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const selected = document.getElementById('manage-entry');
  selected.value = 'obsolete.txt';
  await selected.emit('change');
  assert.equal(document.workspaceWork?.workbench.files.selectedEntry, 'obsolete.txt');
  assert.equal(document.getElementById('delete-entry').disabled, false);
  const readsBeforeDelete = workspaceReads;
  await document.getElementById('delete-entry').emit('click');

  assert.equal(deleteCalls, 1, 'ambiguous deletion was replayed');
  assert.equal(workspaceReads, readsBeforeDelete + 1, 'Mesh did not read authoritative state after the lost reply');
  assert.equal(document.serviceState.state === 'ready', true);
  assert.equal(document.getElementById('manage-entry').children.length, 1);
  assert.match(document.getElementById('notice').textContent, /lost the delete reply but verified.*obsolete\.txt.*no longer present/i);
});

test('an external exact clone cannot receive a stale draft and refresh preserves it unverified', async () => {
  const document = fakeDocument();
  const root = '/managed/reused-path';
  const workspace = (installation) => ({
    root,
    digest: 'identical-record-fold',
    installation,
    records: 1,
    private_version: { version: 'version-1' },
    shared_version: null,
    entries: [{ path: 'kept.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'kept.txt',
      object_id: '01OBJECT0000000000000000000',
      current: { version_id: '01CURRENTVERSION00000000000', manifest_id: 'manifest-current' },
      retained_versions: [{ version_id: '01CURRENTVERSION00000000000', manifest_id: 'manifest-current' }],
    }],
  });
  let current = workspace('physical-installation-a');
  let staleWrite = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: current.root,
        workspace_digest: current.digest,
        workspace_installation: current.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(current);
    }
    if (command === 'inspect_managed_file') {
      if (parameters.expectedWorkspaceInstallation !== current.installation) {
        throw new Error('the verified managed workspace changed before the operation began');
      }
      return JSON.stringify({
        path: 'kept.txt',
        text: 'before\n',
        text_editable: true,
        modified_from_current_version: false,
        current_version: '01CURRENTVERSION00000000000',
        byte_count: 7,
        content_digest: 'digest-before',
      });
    }
    if (command === 'preserve_managed_text') {
      if (parameters.expectedWorkspaceInstallation !== current.installation) {
        throw new Error('the verified managed workspace changed before the operation began');
      }
      staleWrite = parameters;
      return JSON.stringify({
        recovery: 'recovery-b',
        content_digest: 'digest-after',
        stable_after_idle: true,
      });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?external-exact-clone=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const file = document.getElementById('edit-file');
  file.value = 'kept.txt';
  await file.emit('change');
  await document.getElementById('load-file').emit('click');
  await waitFor(() => document.workspaceWork?.workbench.changes.editorKind === 'text');
  const editor = document.getElementById('file-editor');
  editor.value = 'draft only for physical installation A\n';
  await editor.emit('input');
  assert.equal(document.workspaceWork?.workbench.changes.editorText, 'draft only for physical installation A\n');
  assert.equal(document.workspaceWork?.workbench.actions.find((action) => action.id === 'preserve-edit')?.enabled, true);

  current = workspace('physical-installation-b');
  await document.getElementById('save-file').emit('click');
  await waitFor(() => /workspace changed/i.test(document.getElementById('notice').textContent));
  assert.equal(staleWrite, null, 'installation A draft was written into exact clone B');
  assert.equal(document.getElementById('save-file').disabled, true);
  assert.match(document.getElementById('notice').textContent, /workspace changed/i);

  await document.emitWorkspaceCurrentIntent('refresh');
  assert.equal(
    editor.value,
    'draft only for physical installation A\n',
    'Refresh destroyed the only remaining copy of installation A draft',
  );
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), root);
  assert.equal(document.serviceState.state === 'ready', false);
  assert.equal(document.getElementById('save-file').disabled, true);
  assert.equal(editor.disabled, false, 'the preserved draft could not be copied or reverted');
  assert.match(document.getElementById('edit-state').textContent, /draft preserved/i);
  assert.match(document.getElementById('notice').textContent, /save, copy, or revert the open editor draft/i);

  editor.value = 'before\n';
  await editor.emit('input');
  await document.emitWorkspaceCurrentIntent('refresh');
  assert.equal(document.serviceState.state === 'ready', true);
  assert.equal(editor.value, '', 'installation A editor state crossed into exact clone B');
  assert.equal(staleWrite, null);
});

test('a refresh cannot certify the pre-mutation snapshot while a native write is unresolved', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/project',
    digest: 'state-project',
    installation: 'state-project',
    records: 1,
    private_version: { version: 'version-1' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    file_histories: [],
  };
  let releaseMutation;
  let reportMutationStarted;
  const mutationStarted = new Promise((resolve) => { reportMutationStarted = resolve; });
  const mutationRelease = new Promise((resolve) => { releaseMutation = resolve; });
  let mutationFinished = false;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    }
    if (command === 'create_managed_text') {
      reportMutationStarted();
      await mutationRelease;
      mutationFinished = true;
      return JSON.stringify({ author_authenticated: true, saved_privately: true });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify({ ...workspace, records: mutationFinished ? 2 : 1 });
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?inflight-refresh=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const path = document.getElementById('manage-path');
  path.value = 'new.txt';
  await path.emit('input');
  const changing = document.getElementById('create-text-entry').emit('click');
  await mutationStarted;
  const refreshWasDisabled = !document.workspaceCurrentAction('refresh')?.enabled;
  const managedChooserWasDisabled = !document.workspaceEntry.entry.canChooseManaged;
  releaseMutation();
  await changing;

  assert.equal(refreshWasDisabled, true, 'Refresh could read and certify the old state mid-mutation');
  assert.equal(managedChooserWasDisabled, true, 'a second workspace transition could overtake the native write');
});

test('a late restore preview from an older workspace cannot authorize the current workspace', async () => {
  const document = fakeDocument();
  const currentVersion = '01CURRENTVERSION00000000000';
  const targetVersion = '01TARGETVERSION000000000000';
  const workspace = (root) => ({
    root,
    digest: `state-${root}`,
    installation: `state-${root}`,
    records: 2,
    private_version: { version: currentVersion },
    shared_version: null,
    entries: [{ path: 'kept.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'kept.txt',
      object_id: '01OBJECT0000000000000000000',
      current: { version_id: currentVersion, manifest_id: 'manifest-current' },
      retained_versions: [
        { version_id: currentVersion, manifest_id: 'manifest-current' },
        { version_id: targetVersion, manifest_id: 'manifest-target' },
      ],
    }],
  });
  let root = '/managed/original';
  let releasePreview;
  let reportPreviewStarted;
  const previewStarted = new Promise((resolve) => { reportPreviewStarted = resolve; });
  const previewResult = new Promise((resolve) => { releasePreview = resolve; });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root, workspace_digest: `state-${root}`, workspace_installation: `state-${root}`, working: false });
    }
    if (command === 'inspect_managed_file') {
      return JSON.stringify({ path: 'kept.txt', content_digest: 'digest-before-restore' });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace(root));
    }
    if (command === 'preview_managed_restore') {
      assert.equal(parameters.expectedWorkspaceRoot, '/managed/original');
      assert.equal(parameters.expectedWorkspaceDigest, 'state-/managed/original');
      reportPreviewStarted();
      return previewResult;
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?stale-restore-preview=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const file = document.getElementById('restore-file');
  file.value = '01OBJECT0000000000000000000';
  await file.emit('change');
  const target = document.getElementById('restore-target');
  target.value = targetVersion;
  await target.emit('change');
  const previewing = document.getElementById('restore-preview').emit('click');
  await previewStarted;

  root = '/managed/replacement';
  await document.emitWorkspaceCurrentIntent('refresh');
  releasePreview(JSON.stringify({ object_id: '01OBJECT0000000000000000000', target_version: targetVersion }));
  await previewing;

  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), root);
  assert.equal(
    document.getElementById('restore-apply').disabled,
    true,
    'an old preview re-enabled Restore against a replacement workspace',
  );
  assert.equal(document.getElementById('restore-output').classList.contains('hidden'), true);
});

test('workspace replacement and removal never discard an editor-only draft', async () => {
  const document = fakeDocument();
  const filePath = 'shared.txt';
  const contentDigest = 'same-working-copy-digest';
  const workspace = (root) => ({
    root,
    digest: `state-${root}`,
    installation: `state-${root}`,
    records: 1,
    private_version: { version: '01CURRENTVERSION00000000000' },
    shared_version: null,
    entries: [{ path: filePath, type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: filePath,
      object_id: `object-${root}`,
      current: { version_id: '01CURRENTVERSION00000000000', manifest_id: 'manifest-current' },
      retained_versions: [{ version_id: '01CURRENTVERSION00000000000', manifest_id: 'manifest-current' }],
    }],
  });
  let current = workspace('/managed/a');
  let staleWrite = null;
  let openCalls = 0;
  let importCalls = 0;
  let rollbackCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'pick_folder') return '/managed/b';
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: current.root, workspace_digest: current.digest, workspace_installation: current.installation, working: false });
    }
    if (command === 'remember_managed_workspace') return JSON.stringify({ root: parameters.path });
    if (command === 'forget_managed_workspace') return JSON.stringify({ remembered: null, workspaces: [], auto_opened: false });
    if (command === 'discover_native_directories') return JSON.stringify([]);
    if (command === 'discover_native_missing_files') return JSON.stringify([]);
    if (command === 'inspect_managed_file') {
      return JSON.stringify({
        path: filePath,
        text: staleWrite ? 'draft intended only for workspace A\n' : 'shared bytes\n',
        text_editable: true,
        modified_from_current_version: Boolean(staleWrite),
        current_version: '01CURRENTVERSION00000000000',
        byte_count: staleWrite ? 36 : 13,
        content_digest: staleWrite ? 'draft-digest' : contentDigest,
      });
    }
    if (command === 'preserve_managed_text') {
      staleWrite = parameters;
      return JSON.stringify({
        stable_after_idle: true,
        recovery: 'recovery-b',
        content_digest: 'draft-digest',
      });
    }
    if (command === 'daemon_call' && parameters.method === 'folder.import.preview') {
      return JSON.stringify(importPreview({ files: 1, directories: 0, bytes: 7, summary: 'draft-import-summary' }));
    }
    if (command === 'daemon_call' && parameters.method === 'folder.import.confirm') {
      importCalls += 1;
      current = workspace('/managed/imported');
      return JSON.stringify({ workspace: current, materialized_entries: 1 });
    }
    if (command === 'rollback_managed_workspace') {
      rollbackCalls += 1;
      return JSON.stringify({ workspace_root: current.root, original_preserved: true });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(current);
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      openCalls += 1;
      current = workspace('/managed/b');
      return JSON.stringify(current);
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?cross-workspace-editor=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const file = document.getElementById('edit-file');
  file.value = filePath;
  await file.emit('change');
  await document.getElementById('load-file').emit('click');
  const editor = document.getElementById('file-editor');
  editor.value = 'draft intended only for workspace A\n';
  await editor.emit('input');
  assert.equal(document.getElementById('save-file').disabled, false);

  await document.emitWorkspaceEntryIntent({ type: 'choose-managed-folder' });
  await waitFor(() => /preserve the open editor draft/i.test(document.getElementById('notice').textContent));
  assert.equal(openCalls, 0, 'the new workspace opened before the editor-only draft was preserved');
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), '/managed/a');
  assert.equal(
    document.getElementById('save-file').disabled,
    false,
    'the actionable editor-only draft was discarded',
  );
  assert.equal(editor.value, 'draft intended only for workspace A\n');
  assert.match(document.getElementById('notice').textContent, /preserve the open editor draft/i);

  await document.emitImportWorkbenchIntent({ type: 'source-draft', path: '/unmanaged/source' });
  await document.emitImportWorkbenchIntent({ type: 'preview-path', path: '/unmanaged/source' });
  await waitFor(() => document.importWorkbench?.import.phase === 'review');
  await document.emitImportWorkbenchIntent({ type: 'destination-draft', path: '/managed/imported' });
  await document.emitImportWorkbenchIntent({ type: 'confirm-import' });
  assert.equal(importCalls, 0, 'import replaced the workspace before the editor-only draft was preserved');
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), '/managed/a');
  assert.equal(editor.value, 'draft intended only for workspace A\n');

  await document.emitWorkspaceCurrentIntent('rollback');
  assert.equal(rollbackCalls, 0, 'rollback removed the workspace before the editor-only draft was preserved');
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), '/managed/a');
  assert.equal(editor.value, 'draft intended only for workspace A\n');

  await document.getElementById('save-file').emit('click');
  assert.equal(staleWrite?.expectedWorkspaceRoot, '/managed/a');

  await document.emitWorkspaceCurrentIntent('rollback');
  assert.equal(rollbackCalls, 0, 'rollback removed a workspace with unsaved native changes');
  assert.match(document.getElementById('notice').textContent, /unsaved native change/i);
});

test('reopening an exact clone at the same path clears state from the prior physical workspace', async () => {
  const document = fakeDocument();
  const root = '/managed/reused-path';
  const filePath = 'shared.txt';
  const workspace = () => ({
    root,
    digest: 'state-identical-clone',
    installation: 'state-identical-clone',
    records: 1,
    private_version: { version: '01CURRENTVERSION00000000000' },
    shared_version: null,
    entries: [{ path: filePath, type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: filePath,
      object_id: 'object-identical-clone',
      current: { version_id: '01CURRENTVERSION00000000000', manifest_id: 'manifest-identical-clone' },
      retained_versions: [{
        version_id: '01CURRENTVERSION00000000000',
        manifest_id: 'manifest-identical-clone',
      }],
    }],
  });
  let current = workspace();
  let physicalGeneration = 'a';
  let staleWrite = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'pick_folder') return root;
    if (command === 'remember_managed_workspace') {
      return JSON.stringify({ root: parameters.path });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: current.root,
        workspace_digest: current.digest,
        workspace_installation: current.installation,
        working: false,
      });
    }
    if (command === 'discover_native_directories') return JSON.stringify([]);
    if (command === 'discover_native_missing_files') return JSON.stringify([]);
    if (command === 'inspect_managed_file') {
      return JSON.stringify({
        path: filePath,
        text: 'shared bytes\n',
        text_editable: true,
        modified_from_current_version: false,
        current_version: '01CURRENTVERSION00000000000',
        byte_count: 13,
        content_digest: 'same-working-copy-digest',
      });
    }
    if (command === 'preserve_managed_text') {
      staleWrite = parameters;
      return JSON.stringify({
        stable_after_idle: true,
        recovery: 'recovery-b',
        content_digest: 'draft-digest',
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(current);
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.open') {
      physicalGeneration = 'b';
      current = workspace();
      return JSON.stringify(current);
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?same-path-new-digest=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const file = document.getElementById('edit-file');
  file.value = filePath;
  await file.emit('change');
  await document.getElementById('load-file').emit('click');
  const editor = document.getElementById('file-editor');
  editor.value = 'draft intended only for workspace generation A\n';
  await editor.emit('input');
  assert.equal(document.getElementById('save-file').disabled, false);

  editor.value = 'shared bytes\n';
  await editor.emit('input');
  assert.equal(document.getElementById('save-file').disabled, true, 'the editor still held an unsaved draft');

  await document.emitWorkspaceEntryIntent({ type: 'choose-managed-folder' });
  await waitFor(() => physicalGeneration === 'b');
  assert.equal((document.workspaceCurrent?.current?.agentFolder ?? ''), root);
  assert.equal(physicalGeneration, 'b');
  assert.equal(
    document.getElementById('save-file').disabled,
    true,
    'workspace generation B inherited generation A\'s actionable draft',
  );
  assert.equal(editor.value, '', 'workspace generation B displayed generation A\'s private draft');

  await document.getElementById('save-file').emit('click');
  assert.equal(staleWrite, null, 'generation A draft was submitted against generation B');
});

test('refreshing the same workspace preserves its draft and restores the guarded save control', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/project',
    digest: 'state-project',
    installation: 'state-project',
    records: 1,
    private_version: { version: '01CURRENTVERSION00000000000' },
    shared_version: null,
    entries: [{ path: 'kept.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'kept.txt',
      object_id: 'object-kept',
      current: { version_id: '01CURRENTVERSION00000000000', manifest_id: 'manifest-current' },
      retained_versions: [{ version_id: '01CURRENTVERSION00000000000', manifest_id: 'manifest-current' }],
    }],
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({ root: workspace.root, workspace_digest: workspace.digest, workspace_installation: workspace.installation, working: false });
    }
    if (command === 'inspect_managed_file') {
      return JSON.stringify({
        path: 'kept.txt',
        text: 'before\n',
        text_editable: true,
        modified_from_current_version: false,
        current_version: '01CURRENTVERSION00000000000',
        byte_count: 7,
        content_digest: 'digest-before',
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?same-workspace-editor=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const file = document.getElementById('edit-file');
  file.value = 'kept.txt';
  await file.emit('change');
  await document.getElementById('load-file').emit('click');
  const editor = document.getElementById('file-editor');
  editor.value = 'unsaved same-workspace draft\n';
  await editor.emit('input');

  await document.emitWorkspaceCurrentIntent('refresh');
  assert.equal(editor.value, 'unsaved same-workspace draft\n');
  assert.equal(editor.disabled, false, 'Refresh left the verified draft read only');
  assert.equal(document.getElementById('save-file').disabled, false, 'Refresh stranded the draft without Save');
});

test('a destination chosen while the same workspace refreshes is not silently discarded', async () => {
  const document = fakeDocument();
  let digest = 'destination-picker-before-refresh';
  let pickerCalls = 0;
  let releasePicker;
  let picker = new Promise((resolve) => { releasePicker = resolve; });
  const workspace = () => ({
    root: '/managed/destination-picker-refresh/mounts',
    digest,
    installation: 'destination-picker-refresh-installation',
    records: digest === 'destination-picker-before-refresh' ? 1 : 2,
    private_version: { version: '01DESTINATIONPICKER000000000' },
    shared_version: null,
    entries: [{ path: 'kept.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'kept.txt',
      object_id: 'object-kept',
      current: { version_id: '01DESTINATIONPICKER000000000', manifest_id: 'manifest-current' },
      retained_versions: [{ version_id: '01DESTINATIONPICKER000000000', manifest_id: 'manifest-current' }],
    }],
  });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: '/managed/destination-picker-refresh/mounts',
        workspace_digest: digest,
        workspace_installation: 'destination-picker-refresh-installation',
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace());
    }
    if (command === 'pick_folder') {
      pickerCalls += 1;
      return picker;
    }
    throw new Error(`unexpected destination-picker command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?destination-picker-refresh=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const choosing = document.destinationActionControl('choose-destination').emit('click');
  await waitFor(() => pickerCalls === 1);
  digest = 'destination-picker-after-refresh';
  await document.emitWorkspaceCurrentIntent('refresh');
  releasePicker('/ordinary/reviewed-destination');
  await choosing;
  await waitFor(() => document.destinationField('destination').value === '/ordinary/reviewed-destination');

  assert.equal(
    document.destinationField('destination').value,
    '/ordinary/reviewed-destination',
    'the accepted native picker result vanished during a harmless same-workspace refresh',
  );
  assert.equal(document.destinationActionControl('preview-all').disabled, false);

  let releaseOlderPicker;
  picker = new Promise((resolve) => { releaseOlderPicker = resolve; });
  const olderChoice = document.destinationActionControl('choose-destination').emit('click');
  await waitFor(() => pickerCalls === 2);
  document.destinationField('destination').value = '/ordinary/newer-typed-destination';
  await document.destinationField('destination').emit('input');
  releaseOlderPicker('/ordinary/stale-picker-destination');
  await olderChoice;
  assert.equal(
    document.destinationField('destination').value,
    '/ordinary/newer-typed-destination',
    'an older native picker result replaced the newer typed destination',
  );
});

test('refresh refuses a newer workspace digest while an editor-only draft is open', async () => {
  const document = fakeDocument();
  let digest = 'state-before-external-save';
  let workspaceReads = 0;
  const workspace = () => ({
    root: '/managed/project',
    digest,
    installation: 'installation-project',
    records: digest === 'state-before-external-save' ? 1 : 2,
    private_version: { version: '01CURRENTVERSION00000000000' },
    shared_version: null,
    entries: [{ path: 'kept.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [{
      path: 'kept.txt',
      object_id: 'object-kept',
      current: { version_id: '01CURRENTVERSION00000000000', manifest_id: 'manifest-current' },
      retained_versions: [{ version_id: '01CURRENTVERSION00000000000', manifest_id: 'manifest-current' }],
    }],
  });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: '/managed/project',
        workspace_digest: digest,
        workspace_installation: 'installation-project',
        working: false,
      });
    }
    if (command === 'inspect_managed_file') {
      return JSON.stringify({
        path: 'kept.txt',
        text: 'before\n',
        text_editable: true,
        modified_from_current_version: false,
        current_version: '01CURRENTVERSION00000000000',
        byte_count: 7,
        content_digest: 'digest-before',
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      workspaceReads += 1;
      return JSON.stringify(workspace());
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?refresh-new-digest-editor=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  document.getElementById('edit-file').value = 'kept.txt';
  await document.getElementById('edit-file').emit('change');
  await document.getElementById('load-file').emit('click');
  const editor = document.getElementById('file-editor');
  editor.value = 'irreplaceable window-only draft\n';
  await editor.emit('input');
  const readsBeforeBlockedRefresh = workspaceReads;

  digest = 'state-after-external-save';
  await document.emitWorkspaceCurrentIntent('refresh');
  assert.equal(workspaceReads, readsBeforeBlockedRefresh, 'Refresh read and installed newer state despite the draft guard');
  assert.equal(editor.value, 'irreplaceable window-only draft\n');
  assert.equal(document.getElementById('save-file').disabled, false);
  assert.match(document.getElementById('notice').textContent, /save, copy, or revert the open editor draft/i);

  editor.value = 'before\n';
  await editor.emit('input');
  await document.emitWorkspaceCurrentIntent('refresh');
  assert.equal(workspaceReads, readsBeforeBlockedRefresh + 1, 'Refresh did not admit the newer state after the draft was cleared');
  assert.equal(document.serviceState.state === 'ready', true);
});

test('choosing another file cannot discard an editor-only draft', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/project',
    digest: 'state-project',
    installation: 'installation-project',
    records: 2,
    private_version: { version: '01CURRENTVERSION00000000000' },
    shared_version: null,
    entries: [
      { path: 'a.txt', type: 'file' },
      { path: 'b.txt', type: 'file' },
    ],
    conditions: [],
    not_yet: [],
    file_histories: [
      {
        path: 'a.txt',
        object_id: 'object-a',
        current: { version_id: '01CURRENTVERSION00000000000', manifest_id: 'manifest-a' },
        retained_versions: [{ version_id: '01CURRENTVERSION00000000000', manifest_id: 'manifest-a' }],
      },
      {
        path: 'b.txt',
        object_id: 'object-b',
        current: { version_id: '01CURRENTVERSION00000000001', manifest_id: 'manifest-b' },
        retained_versions: [{ version_id: '01CURRENTVERSION00000000001', manifest_id: 'manifest-b' }],
      },
    ],
  };
  const inspected = [];
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        working: false,
      });
    }
    if (command === 'inspect_managed_file') {
      inspected.push(parameters.relativePath);
      const text = parameters.relativePath === 'a.txt' ? 'saved A\n' : 'saved B\n';
      return JSON.stringify({
        path: parameters.relativePath,
        text,
        text_editable: true,
        modified_from_current_version: false,
        current_version: parameters.relativePath === 'a.txt'
          ? '01CURRENTVERSION00000000000'
          : '01CURRENTVERSION00000000001',
        byte_count: text.length,
        content_digest: parameters.relativePath === 'a.txt' ? 'digest-a' : 'digest-b',
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?file-switch-draft=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const file = document.getElementById('edit-file');
  const editor = document.getElementById('file-editor');
  file.value = 'a.txt';
  await file.emit('change');
  await document.getElementById('load-file').emit('click');
  editor.value = 'irreplaceable draft for A\n';
  await editor.emit('input');

  file.value = 'b.txt';
  await file.emit('change');
  assert.equal(file.value, 'a.txt');
  assert.equal(editor.value, 'irreplaceable draft for A\n');
  assert.equal(document.getElementById('save-file').disabled, false);
  assert.deepEqual(inspected, ['a.txt']);
  assert.match(document.getElementById('notice').textContent, /save, copy, or revert the open editor draft/i);

  file.value = 'b.txt';
  await document.getElementById('load-file').emit('click');
  assert.equal(file.value, 'a.txt', 'a scripted Load attempt bypassed the draft guard');
  assert.equal(editor.value, 'irreplaceable draft for A\n');
  assert.deepEqual(inspected, ['a.txt']);

  editor.value = 'saved A\n';
  await editor.emit('input');
  file.value = 'b.txt';
  await file.emit('change');
  await document.getElementById('load-file').emit('click');
  assert.equal(file.value, 'b.txt');
  assert.equal(editor.value, 'saved B\n');
  assert.deepEqual(inspected, ['a.txt', 'b.txt']);
});

test('an automatic review candidate recovers a committed review after its reply is lost', async () => {
  const document = fakeDocument();
  const automaticReview = {
    bundle: 'computed-review-bundle',
    subject_operation: 'saved-operation',
    author: 'saved-author',
    opened_by: null,
    recorded: false,
    actor_sequence: '1',
    subject_operations: [{ kind: 'WriteFileVersion', canonical_hex: 'a1' }],
    subject_operations_not_listed: 0,
    presentation_digest: 'presentation-computed-review',
    bundle_changes: [{
      object_id: 'object-notes',
      path_before: null,
      path_after: '/notes.txt',
      effect: 'created',
      before: null,
      after: { kind: 'binary', version_id: 'version-notes', content_digest: 'digest-notes', byte_length: '12', line_count: null },
      body: 'binary',
      opaque_reason: null,
    }],
    bundle_changes_not_listed: 0,
    content_complete: true,
    unavailable_code: null,
    projection_authorizes_approval: false,
  };
  const base = {
    root: '/managed/review-project',
    digest: 'review-state-before',
    installation: 'review-installation',
    records: 1,
    reviews: 0,
    review_items: [automaticReview],
    review_items_not_listed: 0,
    private_version: { version: 'private-head', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'notes.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    file_histories: [],
    workspace_versions: [{ operation: 'saved-operation', ordinal: 1, actor_sequence: 1 }],
  };
  const reviewed = {
    ...base,
    digest: 'review-state-after',
    records: 2,
    reviews: 1,
    review_items: [{
      ...automaticReview,
      opened_by: 'local-reviewer',
      recorded: true,
    }],
  };
  let current = base;
  let reviewParameters = null;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'approval_credential_status') {
      return JSON.stringify({ enrolled: true, available: true, unavailable_reason: null, algorithm: 'es256', user_verification: 'user-presence' });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(current);
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: current.root,
        workspace_digest: current.digest,
        workspace_installation: current.installation,
        working: false,
      });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'open_current_review') {
      reviewParameters = parameters;
      current = reviewed;
      throw new Error('Mesh lost the review reply after the durable record committed');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?generated-review=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  await waitFor(() => document.getElementById('setup-approval').textContent === 'Approval ready');

  const button = document.getElementById('open-current-review');
  assert.equal(button.disabled, false, 'the current saved version was not reviewable');
  assert.equal(button.textContent, 'Record reviewed version');
  assert.equal(document.getElementById('review-count').textContent, '0 recorded reviews · 1 ready');
  await button.emit('click');

  assert.deepEqual(reviewParameters, {
    expectedWorkspaceRoot: base.root,
    expectedWorkspaceDigest: base.digest,
    expectedWorkspaceInstallation: base.installation,
  });
  assert.equal('bundle' in reviewParameters, false, 'the UI supplied review identity');
  assert.equal('target' in reviewParameters, false, 'the UI supplied review subject authority');
  assert.equal(button.disabled, true);
  assert.equal(button.textContent, 'Review recorded');
  assert.equal(document.getElementById('review-count').textContent, '1 recorded review');
  assert.match(document.getElementById('notice').textContent, /lost the review reply.*confirmed.*recorded/i);
});

test('the first alpha session carries one agent change through the React review into a private export', async () => {
  const document = fakeDocument();
  const operation = 'd1'.repeat(32);
  const bundle = 'd2'.repeat(32);
  const workspace = {
    root: '/managed/first-alpha-session/mounts',
    digest: 'first-alpha-session-before',
    installation: 'first-alpha-session-installation',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'd3'.repeat(32), concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'agent-result.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    file_histories: [{
      path: 'agent-result.txt',
      object_id: 'object-agent-result',
      current: { version_id: 'version-before-agent', manifest_id: 'manifest-before-agent' },
      retained_versions: [{ version_id: 'version-before-agent', manifest_id: 'manifest-before-agent' }],
    }],
    workspace_versions: [{ operation: 'd4'.repeat(32), ordinal: 1, actor_sequence: '1' }],
  };
  const reviewCandidate = {
    bundle,
    subject_operation: operation,
    reviewed_head: 'd5'.repeat(32),
    opened_by: null,
    author: 'd6'.repeat(32),
    recorded: false,
    actor_sequence: '2',
    subject_operations: [{ kind: 'WriteFileVersion', canonical_hex: 'a1' }],
    subject_operations_not_listed: 0,
    presentation_digest: 'd7'.repeat(32),
    bundle_changes: [{
      object_id: 'd8'.repeat(16),
      path_before: '/agent-result.txt',
      path_after: '/agent-result.txt',
      effect: 'content-written',
      before: {
        kind: 'text',
        version_id: 'd9'.repeat(32),
        content_digest: null,
        byte_length: null,
        line_count: '1',
      },
      after: {
        kind: 'text',
        version_id: 'da'.repeat(32),
        content_digest: null,
        byte_length: null,
        line_count: '1',
      },
      body: 'text',
      opaque_reason: null,
      verified_text: {
        source: 'before-after',
        before: { version_id: 'd9'.repeat(32), content_digest: 'db'.repeat(32) },
        after: { version_id: 'da'.repeat(32), content_digest: 'dc'.repeat(32) },
        hunks: [{
          before_start: 1,
          before_len: 1,
          after_start: 1,
          after_len: 1,
          lines: [
            { kind: 'removed', before: 1, after: null, text: 'before agent' },
            { kind: 'added', before: null, after: 1, text: 'after agent' },
          ],
        }],
      },
    }],
    bundle_changes_not_listed: 0,
    content_complete: true,
    unavailable_code: null,
    projection_authorizes_approval: false,
  };
  let agentHandoffInstallation = null;
  let agentChangedFile = false;
  let savedAgentFile = false;
  let exported = false;
  let finishPreflights = 0;
  let finishCalls = 0;
  let assignedGenericInspections = 0;
  let malformedFinishPreflight = true;
  let islandProjection = null;
  const targetRoot = '/ordinary/first-alpha-session-export';
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      auto_opened: false,
      remembered: workspace.root,
      workspaces: [workspace.root],
      active_folder: '/application/native-workspace/current',
      export_root: '/ordinary/original-alpha-project',
      workspace_entries: [{
        path: workspace.root,
        export_root: '/ordinary/original-alpha-project',
        project_root: '/ordinary/original-alpha-project',
        agent_handoff_installation: agentHandoffInstallation,
        agent_handoff_generation: agentHandoffInstallation ? TEST_AGENT_HANDOFF_GENERATION : null,
      }],
    });
    if (command === 'approval_credential_status') return JSON.stringify({
      enrolled: false,
      available: false,
      unavailable_reason: 'Secure Enclave approval requires a validated Apple application identity',
    });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      native_folder: true,
      native_folder_path: workspace.root,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'open_managed_workspace_in_codex') {
      agentHandoffInstallation = workspace.installation;
      agentChangedFile = true;
      return JSON.stringify({
        path: workspace.root,
        workspace_installation: workspace.installation,
        fixed_workspace_path: true,
        agent_handoff_recorded: true,
        agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
        agent: 'Codex',
      });
    }
    if (command === 'finish_managed_workspace_agent_handoff') {
      finishCalls += 1;
      assert.equal(parameters.expectedWorkspaceInstallation, workspace.installation);
      agentHandoffInstallation = null;
      return JSON.stringify({
        path: workspace.root,
        workspace_installation: workspace.installation,
        cleared: true,
      });
    }
    if (command === 'inspect_agent_finish_preflight') {
      finishPreflights += 1;
      assert.equal(agentHandoffInstallation, workspace.installation, 'preflight did not retain custody');
      assert.deepEqual(parameters, {
        expectedWorkspaceRoot: workspace.root,
        expectedWorkspaceDigest: workspace.digest,
        expectedWorkspaceInstallation: workspace.installation,
        expectedAgentHandoffGeneration: TEST_AGENT_HANDOFF_GENERATION,
      });
      return JSON.stringify({
        schema: 'mesh.agent-finish-preflight/v1',
        workspace_root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
        managed_files: [{
          path: 'agent-result.txt',
          current_version: 'version-before-agent',
          byte_count: 12,
          content_digest: 'aa'.repeat(32),
          executable: false,
          modified_from_current_version: true,
        }],
        native_files: [],
        native_directories: [],
        missing_files: [],
        unsupported_entries: [],
        ...(malformedFinishPreflight ? { unexpected_authority: true } : {}),
      });
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      if (agentHandoffInstallation) {
        assignedGenericInspections += 1;
        throw new Error('ordinary native inspection refuses active agent custody');
      }
      return '[]';
    }
    if (command === 'inspect_managed_file') {
      if (agentHandoffInstallation) {
        assignedGenericInspections += 1;
        throw new Error('ordinary native inspection refuses active agent custody');
      }
      return JSON.stringify({
        path: 'agent-result.txt',
        text: agentChangedFile ? 'after agent\n' : 'before agent\n',
        text_editable: true,
        modified_from_current_version: agentChangedFile && !savedAgentFile,
        current_version: savedAgentFile ? 'version-after-agent' : 'version-before-agent',
        byte_count: 12,
        content_digest: agentChangedFile ? 'digest-after-agent' : 'digest-before-agent',
        executable: false,
      });
    }
    if (command === 'save_managed_private') {
      assert.equal(parameters.relativePath, 'agent-result.txt');
      assert.equal(parameters.expectedContentDigest, 'digest-after-agent');
      savedAgentFile = true;
      workspace.digest = 'first-alpha-session-saved';
      workspace.records = 2;
      workspace.private_version = { version: 'd3'.repeat(32), concurrent_changes: 1 };
      workspace.file_histories[0] = {
        path: 'agent-result.txt',
        object_id: 'object-agent-result',
        current: { version_id: 'version-after-agent', manifest_id: 'manifest-after-agent' },
        retained_versions: [
          { version_id: 'version-before-agent', manifest_id: 'manifest-before-agent' },
          { version_id: 'version-after-agent', manifest_id: 'manifest-after-agent' },
        ],
      };
      workspace.workspace_versions = [{ operation, ordinal: 2, actor_sequence: '2' }];
      workspace.review_items = [reviewCandidate];
      return JSON.stringify({
        path: 'agent-result.txt',
        version: 'version-after-agent',
        manifest: 'manifest-after-agent',
        changeset: operation,
        stable_after_idle: true,
        saved_privately: true,
        author_authenticated: true,
      });
    }
    if (command === 'open_current_review') {
      assert.equal(parameters.expectedWorkspaceDigest, workspace.digest);
      workspace.digest = 'first-alpha-session-reviewed';
      workspace.records = 3;
      workspace.reviews = 1;
      workspace.review_items = [{ ...reviewCandidate, opened_by: 'dd'.repeat(32), recorded: true }];
      return JSON.stringify(workspace);
    }
    if (command === 'pick_folder') return targetRoot;
    if (command === 'preview_managed_exports') return JSON.stringify([{
      path: 'agent-result.txt',
      source_version: 'version-after-agent',
      source_byte_count: 12,
      source_content_digest: 'digest-after-agent',
      source_executable: false,
      source_text: null,
      target_root: targetRoot,
      target_installation: 'target-root-installation',
      target_parent_installation: 'target-parent-installation',
      target_file_installation: null,
      target_exists: false,
      target_byte_count: null,
      target_content_digest: null,
      target_executable: null,
      target_text: null,
      identical: false,
      target_relation: 'absent',
      replace_allowed: true,
    }]);
    if (command === 'export_managed_file') {
      assert.equal(parameters.relativePath, 'agent-result.txt');
      assert.equal(parameters.targetRoot, targetRoot);
      exported = true;
      return JSON.stringify({ path: 'agent-result.txt', target_root: targetRoot, created: true });
    }
    if (command === 'discover_retired_exports') return '[]';
    if (command === 'remember_managed_workspace') return JSON.stringify({
      remembered: workspace.root,
      workspaces: [workspace.root],
      auto_opened: false,
      export_root: targetRoot,
    });
    throw new Error(`unexpected first-session command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?complete-first-alpha-session=${Date.now()}`);
  document.addEventListener('mesh:review-workbench-projection', (event) => {
    islandProjection = event.detail;
  });
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-available'));
  await waitFor(() => document.serviceState.state === 'ready');

  await document.emitWorkspaceCurrentIntent('start-codex');
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Finish agent handoff');
  await document.emitWorkspaceOverviewIntent('recommended');
  assert.equal(finishPreflights, 1, 'Finish did not inspect the assigned folder');
  assert.equal(finishCalls, 0, 'a malformed preflight response released agent custody');
  assert.equal(agentHandoffInstallation, workspace.installation, 'a malformed preflight response cleared the assignment');
  assert.match(document.getElementById('notice').textContent, /invalid agent-finish inspection/);
  malformedFinishPreflight = false;
  await document.emitWorkspaceCurrentIntent('finish-agent');
  await waitFor(() => islandProjection?.authority?.canRecordReview === true);
  assert.equal(finishPreflights, 2, 'Finish did not retry one assigned read-only preflight');
  assert.equal(finishCalls, 1, 'Finish released agent custody more than once');
  assert.equal(assignedGenericInspections, 0, 'Finish used an ordinary inspection while custody was active');
  assert.equal(savedAgentFile, true, 'finishing the handoff did not save the inspected agent bytes');
  assert.equal(islandProjection.projection.bundle, bundle);

  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-mounted', { detail: {
    generation: islandProjection.generation,
    bundle,
  } }));
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', { detail: {
    generation: islandProjection.generation,
    bundle,
    intent: { type: 'record-review' },
  } }));
  await waitFor(() => islandProjection?.authority?.canExportPrivateCopy === true);
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-mounted', { detail: {
    generation: islandProjection.generation,
    bundle,
  } }));
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', { detail: {
    generation: islandProjection.generation,
    bundle,
    intent: { type: 'choose-private-export' },
  } }));
  await waitFor(() => document.destinationField('destination').value === targetRoot);
  await waitFor(() => document.destinationActionControl('preview-all').disabled === false);
  await document.destinationActionControl('preview-all').emit('click');
  assert.equal(document.destinationActionControl('confirm-batch').disabled, false);
  assert.match(document.destinationOutput.textContent, /Create\s+agent-result\.txt/);
  await document.destinationActionControl('confirm-batch').emit('click');
  assert.equal(exported, true, 'the reviewed agent result did not reach the private destination');
  assert.match(document.getElementById('notice').textContent, /Saved changes, moves, and deletions are applied/);
});

test('an unentitled alpha offers an explicit private copy without weakening original-folder approval', async () => {
  const document = fakeDocument();
  const operation = 'b1'.repeat(32);
  const reason = 'Secure Enclave approval requires a validated Apple application identity';
  const workspace = {
    root: '/managed/unentitled-alpha/mounts',
    digest: 'unentitled-alpha-digest',
    installation: 'unentitled-alpha-installation',
    records: 2,
    reviews: 0,
    review_items_not_listed: 0,
    review_items: [{
      bundle: 'b2'.repeat(32),
      subject_operation: operation,
      reviewed_head: 'b3'.repeat(32),
      opened_by: null,
      author: 'b5'.repeat(32),
      recorded: false,
      actor_sequence: '1',
      subject_operations: [{ kind: 'WriteFileVersion', canonical_hex: 'a1' }],
      subject_operations_not_listed: 0,
      presentation_digest: 'b6'.repeat(32),
      bundle_changes: [{
        object_id: 'b8'.repeat(16),
        path_before: '/reviewed.txt',
        path_after: '/reviewed.txt',
        effect: 'content-written',
        before: { kind: 'text', version_id: 'b9'.repeat(32), content_digest: null, byte_length: null, line_count: '1' },
        after: { kind: 'text', version_id: 'ba'.repeat(32), content_digest: null, byte_length: null, line_count: '1' },
        body: 'text',
        opaque_reason: null,
        verified_text: {
          source: 'before-after',
          before: { version_id: 'b9'.repeat(32), content_digest: 'bb'.repeat(32) },
          after: { version_id: 'ba'.repeat(32), content_digest: 'bc'.repeat(32) },
          hunks: [{
            before_start: 1,
            before_len: 1,
            after_start: 1,
            after_len: 1,
            lines: [
              { kind: 'removed', before: 1, after: null, text: 'before review' },
              { kind: 'added', before: null, after: 1, text: 'after review' },
            ],
          }],
        },
      }],
      bundle_changes_not_listed: 0,
      content_complete: true,
      unavailable_code: null,
      projection_authorizes_approval: false,
    }],
    private_version: { version: 'b7'.repeat(32), concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    file_histories: [],
    workspace_versions: [{ operation, ordinal: 2, actor_sequence: '1' }],
  };
  const reviewed = {
    ...workspace,
    digest: 'unentitled-alpha-reviewed-digest',
    records: 3,
    reviews: 1,
    file_histories: [{
      path: 'reviewed.txt',
      object_id: 'object-reviewed',
      current: { version_id: 'version-reviewed', manifest_id: 'manifest-reviewed' },
      retained_versions: [{ version_id: 'version-reviewed', manifest_id: 'manifest-reviewed' }],
    }],
    review_items: [{
      ...workspace.review_items[0],
      opened_by: 'b4'.repeat(32),
      recorded: true,
    }],
  };
  let current = workspace;
  let enrollmentCalls = 0;
  let pickerCalls = 0;
  let pickerResult = '/ordinary/private-export';
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      auto_opened: false,
      remembered: workspace.root,
      workspaces: [workspace.root],
      active_folder: '/application/native-workspace/current',
      export_root: '/ordinary/unentitled-alpha',
      workspace_entries: [{
        path: workspace.root,
        export_root: '/ordinary/unentitled-alpha',
        project_root: '/ordinary/unentitled-alpha',
      }],
    });
    if (command === 'approval_credential_status') {
      return JSON.stringify({ enrolled: false, available: false, unavailable_reason: reason });
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: current.root,
        workspace_digest: current.digest,
        workspace_installation: current.installation,
        working: false,
      });
    }
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(current);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'open_current_review') {
      current = reviewed;
      return JSON.stringify(reviewed);
    }
    if (command === 'enroll_approval_credential') {
      enrollmentCalls += 1;
      throw new Error('an unavailable setup action reached native enrolment');
    }
    if (command === 'pick_folder') {
      pickerCalls += 1;
      return pickerResult;
    }
    throw new Error(`unexpected unavailable-approval command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?unavailable-approval=${Date.now()}`);
  let islandProjection = null;
  let destinationProjection = null;
  document.addEventListener('mesh:review-workbench-projection', (event) => {
    islandProjection = event.detail;
  });
  document.addEventListener('mesh:workspace-destination-projection', (event) => {
    destinationProjection = event.detail;
  });
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-available'));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-destination-available'));
  await waitFor(() => document.getElementById('setup-approval').textContent === 'Approval unavailable');
  assert.equal(destinationProjection.destination.chooserRevision, 0);

  assert.equal(islandProjection.authority.canRecordReview, true);
  assert.equal(islandProjection.authority.canApprove, false);
  assert.equal(islandProjection.authority.canExportPrivateCopy, false);
  assert.match(islandProjection.authority.approvalReason, /approval is unavailable in this build/i);
  assert.match(islandProjection.authority.approvalReason, new RegExp(reason));
  assert.match(islandProjection.authority.approvalReason, /Choose export folder/);

  const setup = document.getElementById('setup-approval');
  assert.equal(setup.disabled, true);
  assert.equal(setup.title, reason);
  assert.match(islandProjection.authority.approvalReason, /Automatic review candidate/);
  const review = document.getElementById('open-current-review');
  assert.equal(review.disabled, false);
  await review.emit('click');
  assert.match(islandProjection.authority.approvalReason, /approval is unavailable in this build/i);
  assert.match(islandProjection.authority.approvalReason, new RegExp(reason));
  assert.equal(islandProjection.controls.setupApprovalLabel, 'Approval unavailable');
  assert.equal(islandProjection.controls.canSetupApproval, false);
  assert.equal(islandProjection.controls.setupApprovalReason, reason);
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Export a private copy');
  assert.equal(document.workspaceOverview?.overview.nextActionLabel, 'Choose export folder');
  assert.equal(document.workspaceOverview?.overview.nextActionDisabled, false);
  assert.match(document.workspaceOverview?.overview.nextActionDescription, /original project unchanged/i);
  assert.match(document.workspaceOverview?.overview.nextActionDescription, /Approval is unavailable/);
  assert.match(document.getElementById('notice').textContent, /Exact saved version recorded for review/);
  assert.match(document.getElementById('notice').textContent, /Approval is unavailable in this build/);
  assert.match(document.getElementById('notice').textContent, new RegExp(reason));
  assert.doesNotMatch(document.getElementById('notice').textContent, /Set up approvals/);
  assert.equal(islandProjection.authority.canRecordReview, false);
  assert.equal(islandProjection.authority.canApprove, false);
  assert.equal(islandProjection.authority.canExportPrivateCopy, true);
  assert.match(islandProjection.authority.approvalReason, /approval is unavailable in this build/i);
  assert.match(islandProjection.authority.approvalReason, /different ordinary folder/);
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-mounted', { detail: {
    generation: islandProjection.generation,
    bundle: islandProjection.projection.bundle,
  } }));
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', { detail: {
    generation: islandProjection.generation,
    bundle: islandProjection.projection.bundle,
    intent: { type: 'choose-private-export', authority: true },
  } }));
  assert.equal(pickerCalls, 0, 'an extended React export intent reached the native picker');
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', { detail: {
    generation: islandProjection.generation,
    bundle: islandProjection.projection.bundle,
    intent: { type: 'choose-private-export' },
  } }));
  await waitFor(() => pickerCalls === 1);
  assert.equal(document.getElementById('workspace-destination-next').scrolledIntoView, true);
  assert.equal(document.destinationActionControl('preview-all').focused, true);
  assert.equal(document.destinationField('destination').value, '/ordinary/private-export');
  assert.equal(destinationProjection.destination.chooserRevision, 1);
  assert.match(document.getElementById('notice').textContent, /Choose Preview saved workspace/);
  await setup.emit('click');
  assert.equal(enrollmentCalls, 0);
  assert.equal(document.destinationActionControl('preview-all').disabled, false);
  pickerResult = null;
  await document.destinationActionControl('choose-destination').emit('click');
  assert.equal(pickerCalls, 2);
  assert.equal(destinationProjection.destination.chooserRevision, 1, 'canceling the chooser invented an accepted result');
  assert.equal(document.getElementById('workspace-destination-next').focused, true);
  assert.match(document.getElementById('notice').textContent, /No destination folder was selected/);
  pickerResult = '/ordinary/unentitled-alpha';
  await document.destinationActionControl('choose-destination').emit('click');
  assert.equal(pickerCalls, 3);
  assert.equal(destinationProjection.destination.chooserRevision, 2, 'an accepted chooser path was not projected before its separate approval refusal');
  assert.equal(document.getElementById('workspace-destination-next').focused, true);
  assert.equal(document.destinationActionControl('preview-all').disabled, true);
  assert.match(document.getElementById('notice').textContent, /original project folder/);
  assert.match(document.getElementById('notice').textContent, /different ordinary folder/);
});

test('an unreadable approval status fails closed until Refresh verifies it', async () => {
  const document = fakeDocument();
  const operation = 'c1'.repeat(32);
  const workspace = {
    root: '/managed/unverified-approval/mounts',
    digest: 'unverified-approval-digest',
    installation: 'unverified-approval-installation',
    records: 2,
    reviews: 1,
    review_items_not_listed: 0,
    review_items: [recordedCurrentReview(operation, 'c2'.repeat(32))],
    private_version: { version: 'c3'.repeat(32), concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    file_histories: [],
    workspace_versions: [{ operation, ordinal: 2, actor_sequence: '1' }],
  };
  let approvalStatusReadable = false;
  let approvalStatusCalls = 0;
  let enrollmentCalls = 0;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      auto_opened: false,
      remembered: workspace.root,
      workspaces: [workspace.root],
      active_folder: '/application/native-workspace/current',
      export_root: '/ordinary/unverified-approval',
      workspace_entries: [{
        path: workspace.root,
        export_root: '/ordinary/unverified-approval',
        project_root: '/ordinary/unverified-approval',
      }],
    });
    if (command === 'approval_credential_status') {
      approvalStatusCalls += 1;
      if (!approvalStatusReadable) throw new Error('credential service unavailable');
      return JSON.stringify({ enrolled: false, available: true, unavailable_reason: null });
    }
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    if (command === 'enroll_approval_credential') {
      enrollmentCalls += 1;
      throw new Error('unverified status reached native enrolment');
    }
    throw new Error(`unexpected unverified-approval command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?unverified-approval=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  await waitFor(() => document.getElementById('setup-approval').textContent === 'Approval unavailable');

  const setup = document.getElementById('setup-approval');
  assert.equal(setup.disabled, true);
  assert.match(setup.title, /could not verify approval availability/i);
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Approval unavailable in this build');
  await setup.emit('click');
  assert.equal(enrollmentCalls, 0, 'an unverified status exposed credential creation');

  approvalStatusReadable = true;
  await document.emitWorkspaceCurrentIntent('refresh');
  assert.equal(approvalStatusCalls, 2, 'Refresh did not retry the approval status read');
  assert.equal(setup.textContent, 'Set up approvals');
  assert.equal(setup.disabled, false);
  assert.equal(setup.title, '');
  assert.equal(document.workspaceOverview?.overview.nextActionTitle, 'Set up local approval');
});

test('a stale status read cannot overwrite a newer approval enrollment', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/approval-status-race/mounts',
    digest: 'approval-status-race-digest',
    installation: 'approval-status-race-installation',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'd1'.repeat(32), concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    file_histories: [],
    workspace_versions: [{ operation: 'd2'.repeat(32), ordinal: 1, actor_sequence: '1' }],
  };
  let approvalStatusCalls = 0;
  let enrollmentCalls = 0;
  let resolveStaleStatus;
  let resolveEnrollment;
  const staleStatus = new Promise((resolve) => { resolveStaleStatus = resolve; });
  const enrollment = new Promise((resolve) => { resolveEnrollment = resolve; });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      auto_opened: false,
      remembered: workspace.root,
      workspaces: [workspace.root],
      active_folder: '/application/native-workspace/current',
      workspace_entries: [{ path: workspace.root }],
    });
    if (command === 'approval_credential_status') {
      approvalStatusCalls += 1;
      if (approvalStatusCalls === 2) return staleStatus;
      return JSON.stringify({ enrolled: false, available: true, unavailable_reason: null });
    }
    if (command === 'enroll_approval_credential') {
      enrollmentCalls += 1;
      return enrollment;
    }
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    throw new Error(`unexpected approval-status-race command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?approval-status-race=${Date.now()}`);
  await waitFor(() => document.getElementById('setup-approval').textContent === 'Set up approvals');

  const refresh = document.emitWorkspaceCurrentIntent('refresh');
  await waitFor(() => approvalStatusCalls === 2);
  const setup = document.getElementById('setup-approval');
  const enrolling = setup.emit('click');
  await waitFor(() => enrollmentCalls === 1);
  assert.equal(setup.textContent, 'Setting up approvals…');
  assert.equal(setup.disabled, true);

  await setup.emit('click');
  assert.equal(enrollmentCalls, 1, 'a second setup ceremony reached the native host');
  await document.emitWorkspaceCurrentIntent('refresh');
  assert.equal(approvalStatusCalls, 2, 'Refresh read an unenrolled status during setup');

  resolveEnrollment(JSON.stringify({ enrolled: true, available: true, unavailable_reason: null }));
  await enrolling;
  await waitFor(() => document.getElementById('setup-approval').textContent === 'Approval ready');
  assert.equal(document.getElementById('setup-approval').textContent, 'Approval ready');

  resolveStaleStatus(JSON.stringify({ enrolled: false, available: true, unavailable_reason: null }));
  await refresh;
  assert.equal(
    document.getElementById('setup-approval').textContent,
    'Approval ready',
    'the older status read replaced the newer enrollment result',
  );
});

test('a lost enrollment reply recovers the durable approval credential without retrying setup', async () => {
  const document = fakeDocument();
  const workspace = {
    root: '/managed/approval-enrollment-recovery/mounts',
    digest: 'approval-enrollment-recovery-digest',
    installation: 'approval-enrollment-recovery-installation',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'e1'.repeat(32), concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    file_histories: [],
    workspace_versions: [{ operation: 'e2'.repeat(32), ordinal: 1, actor_sequence: '1' }],
  };
  let approvalStatusCalls = 0;
  let enrollmentCalls = 0;
  let enrolled = false;
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({
      auto_opened: false,
      remembered: workspace.root,
      workspaces: [workspace.root],
      active_folder: '/application/native-workspace/current',
      workspace_entries: [{ path: workspace.root }],
    });
    if (command === 'approval_credential_status') {
      approvalStatusCalls += 1;
      return JSON.stringify({ enrolled, available: true, unavailable_reason: null });
    }
    if (command === 'enroll_approval_credential') {
      enrollmentCalls += 1;
      enrolled = true;
      throw new Error('native enrollment reply was lost after the credential was stored');
    }
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') {
      return '[]';
    }
    throw new Error(`unexpected approval-enrollment-recovery command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?approval-enrollment-recovery=${Date.now()}`);
  await waitFor(() => document.getElementById('setup-approval').textContent === 'Set up approvals');

  await document.getElementById('setup-approval').emit('click');
  assert.equal(enrollmentCalls, 1, 'Mesh retried an enrollment whose outcome was unknown');
  assert.equal(approvalStatusCalls, 2, 'Mesh did not reconcile the durable credential after losing the enrollment reply');
  assert.equal(document.getElementById('setup-approval').textContent, 'Approval ready');
  assert.match(document.getElementById('notice').textContent, /confirmed.*approval credential.*ready/i);
});

test('a native folder handoff freezes every competing workspace action', async () => {
  const document = fakeDocument();
  const currentOperation = '71'.repeat(32);
  const earlierOperation = '70'.repeat(32);
  const currentReview = recordedCurrentReview(currentOperation, '72'.repeat(32));
  currentReview.bundle_changes = [{
    object_id: '73'.repeat(32),
    path_before: '/notes.bin',
    path_after: '/notes.bin',
    effect: 'content-written',
    before: {
      kind: 'binary',
      version_id: '74'.repeat(32),
      content_digest: '75'.repeat(32),
      byte_length: '8',
      line_count: null,
    },
    after: {
      kind: 'binary',
      version_id: '76'.repeat(32),
      content_digest: '77'.repeat(32),
      byte_length: '9',
      line_count: null,
    },
    body: 'binary',
    opaque_reason: 'binary',
  }];
  const workspace = {
    root: '/managed/native-launch-lock/mounts',
    digest: 'native-launch-lock-digest',
    installation: 'native-launch-lock-installation',
    records: 2,
    reviews: 1,
    review_items: [currentReview],
    review_items_not_listed: 0,
    private_version: { version: 'native-launch-private', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    file_histories: [],
    workspace_versions: [
      { operation: earlierOperation, ordinal: 1, actor_sequence: '1' },
      { operation: currentOperation, ordinal: 2, actor_sequence: '2' },
    ],
  };
  let releaseCodex;
  let codexCalls = 0;
  let finishPreflights = 0;
  let approvalAttempts = 0;
  let competingWorkspaceWrites = 0;
  let currentProjection = null;
  let overviewProjection = null;
  let reviewProjection = null;
  let mountCurrentProjection = true;
  let mountOverviewProjection = true;
  let mountReviewProjection = true;
  document.addEventListener('mesh:workspace-current-projection', (event) => {
    currentProjection = event.detail;
    if (mountCurrentProjection) {
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-mounted', {
        detail: { generation: event.detail.generation },
      }));
    }
  });
  document.addEventListener('mesh:workspace-overview-projection', (event) => {
    overviewProjection = event.detail;
    if (mountOverviewProjection) {
      document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-mounted', {
        detail: { generation: event.detail.generation },
      }));
    }
  });
  document.addEventListener('mesh:review-workbench-projection', (event) => {
    reviewProjection = event.detail;
    if (mountReviewProjection) {
      document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-mounted', {
        detail: {
          generation: event.detail.generation,
          bundle: event.detail.state === 'ready' ? event.detail.projection.bundle : null,
        },
      }));
    }
  });
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') {
      return JSON.stringify({
        remembered: workspace.root,
        workspaces: [workspace.root, '/managed/another-agent/mounts'],
        workspace_entries: [
          { path: workspace.root },
          { path: '/managed/another-agent/mounts' },
        ],
        auto_opened: false,
        active_folder: '/application/native-workspace/current',
        export_root: '/Users/person/project',
      });
    }
    if (command === 'approval_credential_status') return JSON.stringify({ enrolled: true, available: true, unavailable_reason: null });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      return JSON.stringify(workspace);
    }
    if (command === 'managed_checkpoint_state') {
      return JSON.stringify({
        root: workspace.root,
        workspace_digest: workspace.digest,
        workspace_installation: workspace.installation,
        native_folder: true,
        native_folder_path: workspace.root,
        working: false,
      });
    }
    if (command === 'preview_managed_workspace_version') {
      return savedWorkspacePreview(earlierOperation, [], { ordinal: 1 });
    }
    if (command === 'open_managed_workspace_in_codex') {
      codexCalls += 1;
      await new Promise((resolve) => { releaseCodex = resolve; });
      return JSON.stringify({
        path: workspace.root,
        workspace_installation: workspace.installation,
        fixed_workspace_path: true,
        agent_handoff_recorded: true,
        agent_handoff_generation: TEST_AGENT_HANDOFF_GENERATION,
        agent: 'Codex',
        mesh_context: 'ready',
      });
    }
    if (command === 'inspect_agent_finish_preflight') {
      finishPreflights += 1;
      throw new Error('a stale recommendation reached Finish agent handoff');
    }
    if (command === 'approve_current_review') {
      approvalAttempts += 1;
      throw new Error('a superseded review action reached the native host');
    }
    if (command === 'open_managed_workspace_version' || (command === 'daemon_call' && parameters.method === 'workspace.open')) {
      competingWorkspaceWrites += 1;
      throw new Error('a competing workspace transition reached the native host');
    }
    throw new Error(`unexpected native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, CustomEvent: FakeCustomEvent };
  globalThis.confirm = () => true;
  await import(`./app.js?native-launch-lock=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-available'));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-available'));
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-available'));
  await waitFor(() => currentProjection !== null && overviewProjection !== null && reviewProjection !== null);
  assert.equal(document.getElementById('workspace-current-next').classList.contains('hidden'), false);
  assert.equal(document.getElementById('workspace-overview-next').classList.contains('hidden'), false);
  assert.equal(document.getElementById('review-workbench-next').classList.contains('hidden'), false);
  assert.equal(reviewProjection.controls.canRecordReview, false);

  document.getElementById('workspace-version').value = earlierOperation;
  await document.getElementById('workspace-version').emit('change');
  document.getElementById('recent-workspace').value = '/managed/another-agent/mounts';
  await document.getElementById('recent-workspace').emit('change');
  assert.equal(document.getElementById('fork-version').disabled, false);
  assert.equal(document.getElementById('open-recent-workspace').disabled, false);
  assert.equal(!document.workspaceCurrentAction('start-codex')?.enabled, false);

  const committedCurrentGeneration = currentProjection.generation;
  const committedOverviewGeneration = overviewProjection.generation;
  const committedReviewGeneration = reviewProjection.generation;
  mountCurrentProjection = false;
  mountOverviewProjection = false;
  mountReviewProjection = false;
  const launch = document.emitWorkspaceCurrentIntent('start-codex');
  await waitFor(() => codexCalls === 1);

  assert.ok(currentProjection.generation > committedCurrentGeneration);
  assert.ok(overviewProjection.generation > committedOverviewGeneration);
  assert.ok(reviewProjection.generation > committedReviewGeneration);
  assert.equal(
    document.getElementById('workspace-current-next').classList.contains('hidden'),
    false,
    'starting a native action flickered out the committed React Current card before replacement',
  );
  assert.equal(
    document.getElementById('workspace-overview-next').classList.contains('hidden'),
    false,
    'starting a native action flickered out the committed React Overview before replacement',
  );
  assert.equal(document.getElementById('workspace-current-next').getAttribute('aria-busy'), 'true');
  assert.equal(document.getElementById('workspace-overview-next').getAttribute('aria-busy'), 'true');
  assert.equal(
    document.getElementById('review-workbench-next').classList.contains('hidden'),
    false,
    'starting a native action flickered out the committed React Review before replacement',
  );
  assert.equal(document.getElementById('review-workbench-next').getAttribute('aria-busy'), 'true');
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-intent', {
    detail: {
      generation: committedCurrentGeneration,
      intent: { type: 'activate', action: 'start-codex' },
    },
  }));
  await Promise.resolve();
  assert.equal(codexCalls, 1, 'the visually retained Current card replayed a now-disabled action');
  document.getElementById('review-workbench-next').scrolledIntoView = false;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-intent', {
    detail: {
      generation: committedOverviewGeneration,
      intent: { type: 'open-review' },
    },
  }));
  assert.equal(
    document.getElementById('review-workbench-next').scrolledIntoView,
    false,
    'the visually retained Overview replayed an action from its superseded action set',
  );
  document.dispatchEvent(new FakeCustomEvent('mesh:review-workbench-intent', {
    detail: {
      generation: committedReviewGeneration,
      bundle: currentReview.bundle,
      intent: { type: 'approve-version' },
    },
  }));
  await Promise.resolve();
  assert.equal(approvalAttempts, 0, 'the visually retained Review replayed a now-disabled approval');
  assert.equal(
    document.getElementById('review-workbench-next').getAttribute('aria-busy'),
    'true',
    'the retained Review stopped reporting its pending action transition',
  );

  assert.equal(document.workspaceCurrentAction('refresh')?.enabled, false);
  for (const control of [
    'open-recent-workspace',
    'forget-recent-workspace',
    'recent-workspace',
    'workspace-version',
    'version-destination',
    'fork-version',
    'fork-version-codex',
    'open-current-review',
    'choose-source',
  ]) {
    assert.equal(
      document.getElementById(control).disabled,
      true,
      `${control} remained actionable while a native folder was being handed off`,
    );
  }
  assert.equal(
    document.workspaceEntry.entry.canChooseManaged,
    false,
    'the React managed-workspace chooser remained actionable during native folder handoff',
  );

  await document.getElementById('fork-version').emit('click');
  assert.equal(competingWorkspaceWrites, 0, 'a disabled version switch bypassed the interaction lock');

  releaseCodex();
  await launch;
  await waitFor(() => overviewProjection.overview.nextActionTitle === 'Agent folder is assigned');
  assert.equal(overviewProjection.overview.nextActionTitle, 'Agent folder is assigned');
  document.addEventListener('mesh:confirmation-projection', (event) => {
    document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-mounted', {
      detail: { generation: event.detail.generation },
    }));
    document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-intent', {
      detail: { generation: event.detail.generation, intent: { type: 'confirm' } },
    }));
  });
  document.dispatchEvent(new FakeCustomEvent('mesh:confirmation-available'));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-intent', {
    detail: {
      generation: committedOverviewGeneration,
      intent: { type: 'recommended' },
    },
  }));
  await Promise.resolve();
  assert.equal(
    finishPreflights,
    0,
    'a stale visible recommendation activated the newer generation\'s Finish action',
  );
  mountCurrentProjection = true;
  mountOverviewProjection = true;
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-current-mounted', {
    detail: { generation: currentProjection.generation },
  }));
  document.dispatchEvent(new FakeCustomEvent('mesh:workspace-overview-mounted', {
    detail: { generation: overviewProjection.generation },
  }));
  assert.equal(document.getElementById('workspace-current-next').classList.contains('hidden'), false);
  assert.equal(document.getElementById('workspace-current-next').getAttribute('aria-busy'), 'false');
  assert.equal(document.getElementById('workspace-overview-next').getAttribute('aria-busy'), 'false');
  assert.equal(document.getElementById('open-recent-workspace').disabled, false);
  assert.equal(document.getElementById('workspace-version').disabled, false);
  assert.equal(!document.workspaceCurrentAction('refresh')?.enabled, false);
});

test('opt-in automatic capture recovers a lost preference reply and signs one stable native edit while the Mesh window is hidden', async () => {
  const document = fakeDocument();
  document.visibilityState = 'hidden';
  const contentDigest = 'ab'.repeat(32);
  let preference = false;
  let losePreferenceReply = true;
  let failPreferenceRead = false;
  let preferenceReads = 0;
  let preferenceWrites = 0;
  let modified = false;
  let saves = 0;
  let scheduledScan = null;
  let scheduledDelay = null;
  const workspace = {
    root: '/managed/automatic-native/mounts',
    digest: 'automatic-workspace-0',
    installation: 'automatic-installation',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'automatic-version-0', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'notes.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    file_histories: [{
      path: 'notes.txt',
      object_id: '01AUTOMATICNATIVE0000000000',
      current: { version_id: 'automatic-version-0', manifest_id: 'automatic-manifest-0' },
      retained_versions: [{ version_id: 'automatic-version-0', manifest_id: 'automatic-manifest-0' }],
    }],
    workspace_versions: [],
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'native_capture_preference') {
      preferenceReads += 1;
      if (failPreferenceRead) throw new Error('the durable automatic-capture preference is unreadable');
      return JSON.stringify({ enabled: preference });
    }
    if (command === 'set_native_capture_preference') {
      preferenceWrites += 1;
      preference = parameters.enabled;
      if (losePreferenceReply) {
        losePreferenceReply = false;
        throw new Error('the renderer lost the committed automatic-capture preference reply');
      }
      return JSON.stringify({ enabled: preference });
    }
    if (command === 'recent_workspace_status') return JSON.stringify({
      remembered: workspace.root,
      workspaces: [workspace.root],
      workspace_entries: [{ path: workspace.root, agent_handoff_installation: null, agent_handoff_generation: null }],
      auto_opened: false,
      active_folder: '/application/native-workspace/current',
      export_root: null,
      warning: null,
    });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'inspect_managed_file') return JSON.stringify({
      path: 'notes.txt',
      text: 'edited in the native folder\n',
      text_editable: true,
      native_untracked: false,
      modified_from_current_version: modified,
      current_version: workspace.file_histories[0].current.version_id,
      byte_count: 28,
      content_digest: contentDigest,
      executable: false,
    });
    if (command === 'save_managed_private') {
      assert.equal(parameters.relativePath, 'notes.txt');
      assert.equal(parameters.expectedContentDigest, contentDigest);
      assert.equal(parameters.expectedWorkspaceInstallation, workspace.installation);
      saves += 1;
      modified = false;
      workspace.digest = 'automatic-workspace-1';
      workspace.records = 2;
      workspace.private_version = { version: 'automatic-version-1', concurrent_changes: 1 };
      workspace.file_histories[0].current = {
        version_id: 'automatic-version-1',
        manifest_id: 'automatic-manifest-1',
      };
      return JSON.stringify({
        path: 'notes.txt',
        version: 'automatic-version-1',
        manifest: 'automatic-manifest-1',
        changeset: 'automatic-changeset-1',
        stable_after_idle: true,
        saved_privately: true,
        author_authenticated: true,
      });
    }
    throw new Error(`unexpected automatic-native command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = {
    __TAURI__: { core: { invoke } },
    addEventListener() {},
    setInterval(listener, delay) {
      scheduledScan = listener;
      scheduledDelay = delay;
      return 1;
    },
  };
  globalThis.confirm = () => true;
  await import(`./app.js?automatic-native=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  await waitFor(() => document.getElementById('auto-save-native').disabled === false);

  const toggle = document.getElementById('auto-save-native');
  assert.equal(toggle.checked, false);
  assert.match((document.workspaceCurrent?.current?.nativeFolderHint ?? ''), /Review-first mode is active/);
  assert.match((document.workspaceCurrent?.current?.nativeFolderHint ?? ''), /waits for Save privately/);
  toggle.checked = true;
  await toggle.emit('change');
  assert.equal(preference, true);
  assert.equal(preferenceWrites, 1, 'preference recovery replayed the persistent write');
  assert.equal(preferenceReads, 2, 'preference recovery did not read back durable native truth');
  assert.equal(toggle.checked, true);
  assert.match(document.getElementById('notice').textContent, /lost the automatic-save reply/i);
  assert.match(document.getElementById('notice').textContent, /verified.+is on/i);
  assert.match((document.workspaceCurrent?.current?.nativeFolderHint ?? ''), /Automatic private save is on while Mesh is running/);
  assert.doesNotMatch((document.workspaceCurrent?.current?.nativeFolderHint ?? ''), /Review-first mode is active/);
  assert.equal(scheduledDelay, 5_000);
  assert.equal(typeof scheduledScan, 'function');

  modified = true;
  scheduledScan();
  await waitFor(() => saves === 1);
  assert.equal(saves, 1);
  assert.equal(workspace.digest, 'automatic-workspace-1');
  assert.equal(document.getElementById('folder-change-queue').classList.contains('hidden'), true);
  assert.equal((document.workspaceCurrent?.current?.state ?? ''), 'Saved privately');
  assert.match(document.getElementById('notice').textContent, /Automatic private save completed/);

  losePreferenceReply = true;
  failPreferenceRead = true;
  toggle.checked = false;
  await toggle.emit('change');
  assert.equal(preference, false);
  assert.equal(preferenceWrites, 2, 'failed preference recovery replayed the persistent write');
  assert.equal(toggle.checked, false);
  assert.equal(toggle.disabled, true, 'an unreadable durable preference remained actionable');
  assert.match(document.getElementById('notice').textContent, /could not confirm the automatic-save choice/i);
  assert.match(document.getElementById('notice').textContent, /paused in this window/i);
});

test('a delayed global preference completion binds any eligible queue save to the workspace now open', async () => {
  const document = fakeDocument();
  let preference = false;
  let preferenceWrites = 0;
  let resolvePreferenceWrite = null;
  let saves = 0;
  const workspaceA = {
    root: '/managed/preference-workspace-a',
    digest: 'preference-workspace-a-0',
    installation: 'preference-installation-a',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'preference-version-a-0', concurrent_changes: 1 },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    file_histories: [],
    workspace_versions: [],
  };
  const workspaceB = {
    ...workspaceA,
    root: '/managed/preference-workspace-b',
    digest: 'preference-workspace-b-0',
    installation: 'preference-installation-b',
    private_version: { version: 'preference-version-b-0', concurrent_changes: 1 },
    entries: [],
    file_histories: [],
  };
  let workspace = workspaceA;
  const invoke = async (command, parameters = {}) => {
    if (command === 'native_capture_preference') return JSON.stringify({ enabled: preference });
    if (command === 'set_native_capture_preference') {
      preferenceWrites += 1;
      assert.equal(parameters.enabled, true);
      return new Promise((resolve) => {
        resolvePreferenceWrite = () => {
          preference = true;
          resolve(JSON.stringify({ enabled: true }));
        };
      });
    }
    if (command === 'recent_workspace_status') return JSON.stringify({
      remembered: workspace.root,
      workspaces: [workspaceA.root, workspaceB.root],
      workspace_entries: [workspaceA, workspaceB].map((entry) => ({
        path: entry.root,
        agent_handoff_installation: null,
        agent_handoff_generation: null,
      })),
      auto_opened: false,
      active_folder: '/application/native-workspace/current',
      export_root: null,
      warning: null,
    });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      working: false,
    });
    if (command === 'reconcile_managed_workspace_navigation') return JSON.stringify({
      path: '/application/native-workspace/current',
      workspace_root: workspace.root,
      stable: true,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories') return JSON.stringify(
      workspace === workspaceB && workspaceB.entries.length ? [] : [{
        path: workspace === workspaceA ? 'a-folder' : 'b-folder',
        installation: workspace === workspaceA ? 'ca'.repeat(32) : 'cb'.repeat(32),
      }],
    );
    if (command === 'discover_native_missing_files') return '[]';
    if (command === 'adopt_native_directory') {
      assert.equal(parameters.relativePath, 'b-folder', 'the delayed preference completion saved the departed workspace queue');
      assert.equal(parameters.expectedDirectoryInstallation, 'cb'.repeat(32));
      assert.equal(parameters.expectedWorkspaceRoot, workspaceB.root);
      assert.equal(parameters.expectedWorkspaceDigest, 'preference-workspace-b-0');
      assert.equal(parameters.expectedWorkspaceInstallation, workspaceB.installation);
      saves += 1;
      workspaceB.digest = 'preference-workspace-b-1';
      workspaceB.records = 2;
      workspaceB.private_version = { version: 'preference-version-b-1', concurrent_changes: 1 };
      workspaceB.entries = [{ path: 'b-folder', type: 'folder' }];
      return JSON.stringify({
        action: 'adopt_folder',
        to_path: 'b-folder',
        changeset: 'preference-directory-changeset-b-1',
        saved_privately: true,
        author_authenticated: true,
      });
    }
    throw new Error(`unexpected delayed-preference command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, addEventListener() {} };
  globalThis.confirm = () => true;
  await import(`./app.js?delayed-preference-workspace-switch=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  await document.getElementById('scan-files').emit('click');
  await waitFor(() => document.getElementById('folder-change-items').children[0]?.textContent.includes('a-folder'));
  const toggle = document.getElementById('auto-save-native');
  toggle.checked = true;
  await toggle.emit('change');
  await waitFor(() => preferenceWrites === 1 && typeof resolvePreferenceWrite === 'function');

  workspace = workspaceB;
  await document.emitWorkspaceCurrentIntent('refresh');
  assert.equal(
    document.workspaceCurrent?.current.agentFolder,
    workspaceB.root,
    document.getElementById('notice').textContent,
  );
  await document.getElementById('scan-files').emit('click');
  await waitFor(() => document.getElementById('folder-change-items').children[0]?.textContent.includes('b-folder'));
  assert.equal(document.getElementById('save-all-private').disabled, false);

  resolvePreferenceWrite();
  await waitFor(() => saves === 1);
  assert.equal(preference, true);
  assert.equal(preferenceWrites, 1);
  assert.equal(saves, 1);
  assert.equal(workspaceB.digest, 'preference-workspace-b-1');
  assert.match(document.getElementById('notice').textContent, /Automatic private save enabled/);
});

test('automatic capture leaves a missing tracked path visible and writes nothing', async () => {
  const document = fakeDocument();
  let saves = 0;
  const workspace = {
    root: '/managed/automatic-structural/mounts',
    digest: 'automatic-structural-workspace',
    installation: 'automatic-structural-installation',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'automatic-structural-version', concurrent_changes: 1 },
    shared_version: null,
    entries: [{ path: 'removed.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    file_histories: [{
      path: 'removed.txt',
      object_id: '01AUTOMATICMISSING000000000',
      current: { version_id: 'removed-version', manifest_id: 'removed-manifest' },
      retained_versions: [{ version_id: 'removed-version', manifest_id: 'removed-manifest' }],
    }],
    workspace_versions: [],
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'native_capture_preference') return JSON.stringify({ enabled: true });
    if (command === 'recent_workspace_status') return JSON.stringify({
      remembered: workspace.root,
      workspaces: [workspace.root],
      workspace_entries: [{ path: workspace.root, agent_handoff_installation: null, agent_handoff_generation: null }],
      auto_opened: false,
      active_folder: '/application/native-workspace/current',
      export_root: null,
      warning: null,
    });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories') return '[]';
    if (command === 'discover_native_missing_files') return JSON.stringify([{
      path: 'removed.txt',
      current_version: 'removed-version',
      content_digest: 'cd'.repeat(32),
      executable: false,
    }]);
    if (command === 'save_managed_private' || command === 'adopt_native_file') {
      saves += 1;
      throw new Error('structural ambiguity must not save');
    }
    throw new Error(`unexpected automatic-structural command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } }, addEventListener() {} };
  globalThis.confirm = () => true;
  await import(`./app.js?automatic-structural=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  await waitFor(() => document.getElementById('auto-save-native').checked === true);
  await document.getElementById('scan-files').emit('click');

  assert.equal(saves, 0);
  assert.equal(document.getElementById('folder-change-items').children.length, 1);
  assert.match(document.getElementById('folder-change-items').children[0].textContent, /missing tracked file/);
  assert.equal(document.getElementById('save-all-private').disabled, true);
  assert.match(document.getElementById('notice').textContent, /asks you to identify each missing tracked file/);
});

test('a lost tracked private-save reply is read back without replaying the append', async () => {
  const document = fakeDocument();
  const savedDigest = '41'.repeat(32);
  let modified = true;
  let saveCalls = 0;
  let stateReads = 0;
  const workspace = {
    root: '/managed/lost-tracked-private/mounts',
    digest: 'lost-tracked-private-0',
    installation: 'lost-tracked-installation',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'lost-tracked-private-version-0' },
    shared_version: null,
    entries: [{ path: 'notes.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    file_histories: [{
      path: 'notes.txt',
      object_id: '01LOSTTRACKEDPRIVATE00000000',
      current: { version_id: 'lost-tracked-version-0', manifest_id: 'lost-tracked-manifest-0' },
      retained_versions: [{ version_id: 'lost-tracked-version-0', manifest_id: 'lost-tracked-manifest-0' }],
    }],
    workspace_versions: [],
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') {
      stateReads += 1;
      return JSON.stringify(workspace);
    }
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'inspect_managed_file') return JSON.stringify({
      path: 'notes.txt',
      text: 'agent result\n',
      text_editable: true,
      native_untracked: false,
      modified_from_current_version: modified,
      current_version: workspace.file_histories[0].current.version_id,
      byte_count: 13,
      content_digest: savedDigest,
      executable: false,
    });
    if (command === 'save_managed_private') {
      saveCalls += 1;
      assert.equal(parameters.expectedWorkspaceDigest, 'lost-tracked-private-0');
      modified = false;
      workspace.digest = 'lost-tracked-private-1';
      workspace.records = 2;
      workspace.private_version = { version: 'lost-tracked-private-version-1' };
      workspace.file_histories[0].current = {
        version_id: 'lost-tracked-version-1',
        manifest_id: 'lost-tracked-manifest-1',
      };
      workspace.file_histories[0].retained_versions.push(workspace.file_histories[0].current);
      throw new Error('tracked private-save reply was lost after durable append');
    }
    throw new Error(`unexpected lost-tracked-private command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?lost-tracked-private=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const file = document.getElementById('edit-file');
  file.value = 'notes.txt';
  await file.emit('change');
  await document.getElementById('load-file').emit('click');
  await document.getElementById('save-private').emit('click');

  assert.equal(saveCalls, 1, 'the ambiguous private append was replayed');
  assert.ok(stateReads >= 2, 'Mesh did not read back durable workspace state after losing the reply');
  assert.equal((document.workspaceCurrent?.current?.state ?? ''), 'Saved privately');
  assert.match(document.getElementById('notice').textContent, /lost.+reply.+verified.+saved privately/i);
});

test('a lost new-file adoption reply is read back without replaying the adoption', async () => {
  const document = fakeDocument();
  const savedDigest = '52'.repeat(32);
  let adopted = false;
  let adoptCalls = 0;
  const workspace = {
    root: '/managed/lost-new-file/mounts',
    digest: 'lost-new-file-0',
    installation: 'lost-new-file-installation',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'lost-new-file-private-0' },
    shared_version: null,
    entries: [],
    conditions: [],
    not_yet: [],
    native_untracked_files: ['agent.md'],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    file_histories: [],
    workspace_versions: [],
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'inspect_native_file') return JSON.stringify({
      path: 'agent.md',
      text: '# agent result\n',
      text_editable: false,
      native_untracked: true,
      byte_count: 15,
      content_digest: savedDigest,
      executable: false,
    });
    if (command === 'inspect_managed_file') return JSON.stringify({
      path: 'agent.md',
      text: '# agent result\n',
      text_editable: true,
      native_untracked: false,
      modified_from_current_version: false,
      current_version: 'lost-new-file-version-1',
      byte_count: 15,
      content_digest: savedDigest,
      executable: false,
    });
    if (command === 'adopt_native_file') {
      adoptCalls += 1;
      adopted = true;
      workspace.digest = 'lost-new-file-1';
      workspace.records = 2;
      workspace.private_version = { version: 'lost-new-file-private-1' };
      workspace.native_untracked_files = [];
      workspace.entries = [{ path: 'agent.md', type: 'file' }];
      workspace.file_histories = [{
        path: 'agent.md',
        object_id: '01LOSTNEWFILE00000000000000',
        current: { version_id: 'lost-new-file-version-1', manifest_id: 'lost-new-file-manifest-1' },
        retained_versions: [{ version_id: 'lost-new-file-version-1', manifest_id: 'lost-new-file-manifest-1' }],
      }];
      throw new Error('new-file adoption reply was lost after durable append');
    }
    throw new Error(`unexpected lost-new-file command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?lost-new-file=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  await document.getElementById('scan-files').emit('click');
  await document.getElementById('save-private').emit('click');

  assert.equal(adopted, true);
  assert.equal(adoptCalls, 1, 'the ambiguous native-file adoption was replayed');
  assert.equal((document.workspaceCurrent?.current?.state ?? ''), 'Saved privately');
  assert.match(document.getElementById('notice').textContent, /lost.+reply.+verified.+saved privately/i);
});

test('a lost first-directory batch reply advances from refreshed private state without replay', async () => {
  const document = fakeDocument();
  const trackedDigest = '63'.repeat(32);
  const directoryInstallation = '74'.repeat(32);
  let directoryAdopted = false;
  let directoryCalls = 0;
  let trackedSaved = false;
  const workspace = {
    root: '/managed/lost-directory-batch/mounts',
    digest: 'lost-directory-batch-0',
    installation: 'lost-directory-batch-installation',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'lost-directory-batch-private-0' },
    shared_version: null,
    entries: [{ path: 'tracked.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    file_histories: [{
      path: 'tracked.txt',
      object_id: '01LOSTDIRECTORYTRACKED00000',
      current: { version_id: 'lost-directory-tracked-0', manifest_id: 'lost-directory-manifest-0' },
      retained_versions: [{ version_id: 'lost-directory-tracked-0', manifest_id: 'lost-directory-manifest-0' }],
    }],
    workspace_versions: [],
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories') return JSON.stringify(directoryAdopted ? [] : [{
      path: 'generated',
      installation: directoryInstallation,
    }]);
    if (command === 'discover_native_missing_files') return '[]';
    if (command === 'inspect_managed_directory_installation') return JSON.stringify({
      path: 'generated',
      installation: directoryInstallation,
    });
    if (command === 'inspect_managed_file') return JSON.stringify({
      path: 'tracked.txt',
      text: 'agent changed tracked\n',
      text_editable: true,
      native_untracked: false,
      modified_from_current_version: !trackedSaved,
      current_version: workspace.file_histories[0].current.version_id,
      byte_count: 22,
      content_digest: trackedDigest,
      executable: false,
    });
    if (command === 'adopt_native_directory') {
      directoryCalls += 1;
      assert.equal(parameters.expectedWorkspaceDigest, 'lost-directory-batch-0');
      directoryAdopted = true;
      workspace.digest = 'lost-directory-batch-1';
      workspace.records = 2;
      workspace.private_version = { version: 'lost-directory-batch-private-1' };
      workspace.entries.push({ path: 'generated', type: 'folder' });
      throw new Error('directory adoption reply was lost after durable append');
    }
    if (command === 'save_managed_private') {
      assert.equal(parameters.expectedWorkspaceDigest, 'lost-directory-batch-1');
      trackedSaved = true;
      workspace.digest = 'lost-directory-batch-2';
      workspace.records = 3;
      workspace.private_version = { version: 'lost-directory-batch-private-2' };
      workspace.file_histories[0].current = {
        version_id: 'lost-directory-tracked-1',
        manifest_id: 'lost-directory-manifest-1',
      };
      workspace.file_histories[0].retained_versions.push(workspace.file_histories[0].current);
      return JSON.stringify({
        path: 'tracked.txt',
        version: 'lost-directory-tracked-1',
        manifest: 'lost-directory-manifest-1',
        changeset: 'lost-directory-changeset-1',
        stable_after_idle: true,
        saved_privately: true,
        author_authenticated: true,
      });
    }
    throw new Error(`unexpected lost-directory-batch command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?lost-directory-batch=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  await document.getElementById('scan-files').emit('click');
  await document.getElementById('save-all-private').emit('click');

  assert.equal(directoryCalls, 1, 'the ambiguous directory adoption was replayed');
  assert.equal(trackedSaved, true, 'the batch did not continue from the refreshed workspace digest');
  assert.equal((document.workspaceCurrent?.current?.state ?? ''), 'Saved privately');
  assert.match(document.getElementById('notice').textContent, /lost.+reply.+verified/i);
});

test('a concurrent replacement-directory adoption cannot satisfy a lost capture reply', async () => {
  const document = fakeDocument();
  const submittedDirectoryInstallation = 'a5'.repeat(32);
  const replacementDirectoryInstallation = 'b6'.repeat(32);
  const trackedDigest = 'c7'.repeat(32);
  let concurrentDirectoryAdopted = false;
  let directoryCalls = 0;
  let installationReads = 0;
  let trackedSaveCalls = 0;
  const workspace = {
    root: '/managed/replaced-directory-recovery/mounts',
    digest: 'replaced-directory-recovery-0',
    installation: 'replaced-directory-recovery-installation',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'replaced-directory-private-0' },
    shared_version: null,
    entries: [{ path: 'tracked.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    file_histories: [{
      path: 'tracked.txt',
      object_id: '01REPLACEDDIRECTORYTRACKED',
      current: { version_id: 'replaced-directory-version-0', manifest_id: 'replaced-directory-manifest-0' },
      retained_versions: [{ version_id: 'replaced-directory-version-0', manifest_id: 'replaced-directory-manifest-0' }],
    }],
    workspace_versions: [],
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories') return JSON.stringify(concurrentDirectoryAdopted ? [] : [{
      path: 'generated',
      installation: submittedDirectoryInstallation,
    }]);
    if (command === 'discover_native_missing_files') return '[]';
    if (command === 'inspect_managed_directory_installation') {
      installationReads += 1;
      assert.equal(parameters.relativePath, 'generated');
      assert.equal(parameters.expectedWorkspaceDigest, 'replaced-directory-recovery-1');
      return JSON.stringify({
        path: 'generated',
        installation: replacementDirectoryInstallation,
      });
    }
    if (command === 'inspect_managed_file') return JSON.stringify({
      path: 'tracked.txt',
      text: 'tracked native edit\n',
      text_editable: true,
      native_untracked: false,
      modified_from_current_version: true,
      current_version: workspace.file_histories[0].current.version_id,
      byte_count: 20,
      content_digest: trackedDigest,
      executable: false,
    });
    if (command === 'adopt_native_directory') {
      directoryCalls += 1;
      assert.equal(parameters.expectedDirectoryInstallation, submittedDirectoryInstallation);
      concurrentDirectoryAdopted = true;
      workspace.digest = 'replaced-directory-recovery-1';
      workspace.records = 2;
      workspace.private_version = { version: 'replaced-directory-private-1' };
      workspace.entries.push({ path: 'generated', type: 'folder' });
      throw new Error('submitted directory was replaced before append while another process adopted the replacement');
    }
    if (command === 'save_managed_private') {
      trackedSaveCalls += 1;
      throw new Error('the batch must stop before saving a later file');
    }
    throw new Error(`unexpected replaced-directory-recovery command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?replaced-directory-recovery=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');
  await document.getElementById('scan-files').emit('click');
  await document.getElementById('save-all-private').emit('click');

  assert.equal(directoryCalls, 1, 'the ambiguous directory adoption was replayed');
  assert.equal(installationReads, 1, 'the durable path was trusted without reopening its exact directory object');
  assert.equal(trackedSaveCalls, 0, 'the batch continued after a replacement directory failed proof');
  assert.equal((document.workspaceCurrent?.current?.state ?? ''), 'Needs attention');
  assert.match(document.getElementById('notice').textContent, /read-back was inconclusive/i);
  assert.match(document.getElementById('notice').textContent, /paused until Find folder changes succeeds/i);
});

test('private-capture read-back distinguishes an unchanged retry from ambiguous newer state', async () => {
  const document = fakeDocument();
  const submittedDigest = '85'.repeat(32);
  const newerDigest = '96'.repeat(32);
  let saveCalls = 0;
  let ambiguousNewerState = false;
  const workspace = {
    root: '/managed/private-capture-classification/mounts',
    digest: 'private-capture-classification-0',
    installation: 'private-capture-classification-installation',
    records: 1,
    reviews: 0,
    review_items: [],
    review_items_not_listed: 0,
    private_version: { version: 'private-capture-classification-private-0' },
    shared_version: null,
    entries: [{ path: 'notes.txt', type: 'file' }],
    conditions: [],
    not_yet: [],
    native_untracked_files: [],
    native_unsupported_entries: [],
    native_inventory_complete: true,
    file_histories: [{
      path: 'notes.txt',
      object_id: '01PRIVATECAPTURECLASSIFY0000',
      current: { version_id: 'private-capture-classification-version-0', manifest_id: 'private-capture-classification-manifest-0' },
      retained_versions: [{ version_id: 'private-capture-classification-version-0', manifest_id: 'private-capture-classification-manifest-0' }],
    }],
    workspace_versions: [],
  };
  const invoke = async (command, parameters = {}) => {
    if (command === 'recent_workspace_status') return JSON.stringify({ auto_opened: false });
    if (command === 'managed_checkpoint_state') return JSON.stringify({
      root: workspace.root,
      workspace_digest: workspace.digest,
      workspace_installation: workspace.installation,
      working: false,
    });
    if (command === 'daemon_call' && parameters.method === 'workspace.state') return JSON.stringify(workspace);
    if (command === 'discover_native_directories' || command === 'discover_native_missing_files') return '[]';
    if (command === 'inspect_managed_file') return JSON.stringify({
      path: 'notes.txt',
      text: ambiguousNewerState ? 'newer native edit\n' : 'reviewed agent result\n',
      text_editable: true,
      native_untracked: false,
      modified_from_current_version: true,
      current_version: workspace.file_histories[0].current.version_id,
      byte_count: ambiguousNewerState ? 18 : 22,
      content_digest: ambiguousNewerState ? newerDigest : submittedDigest,
      executable: false,
    });
    if (command === 'save_managed_private') {
      saveCalls += 1;
      assert.equal(parameters.expectedContentDigest, submittedDigest);
      if (saveCalls === 1) throw new Error('private capture refused before append');
      ambiguousNewerState = true;
      workspace.digest = 'private-capture-classification-1';
      workspace.records = 2;
      workspace.private_version = { version: 'private-capture-classification-private-1' };
      workspace.file_histories[0].current = {
        version_id: 'private-capture-classification-version-1',
        manifest_id: 'private-capture-classification-manifest-1',
      };
      workspace.file_histories[0].retained_versions.push(workspace.file_histories[0].current);
      throw new Error('private capture reply was lost while newer state appeared');
    }
    throw new Error(`unexpected private-capture-classification command: ${command}`);
  };

  globalThis.document = document;
  globalThis.window = { __TAURI__: { core: { invoke } } };
  globalThis.confirm = () => true;
  await import(`./app.js?private-capture-classification=${Date.now()}`);
  await waitFor(() => document.serviceState.state === 'ready');

  const file = document.getElementById('edit-file');
  file.value = 'notes.txt';
  await file.emit('change');
  await document.getElementById('load-file').emit('click');
  await document.getElementById('save-private').emit('click');
  assert.equal(saveCalls, 1);
  assert.equal(document.getElementById('save-private').disabled, false);
  assert.match(document.getElementById('notice').textContent, /exact reviewed change.+not added.+safe to retry/i);

  await document.getElementById('save-private').emit('click');
  assert.equal(saveCalls, 2, 'the ambiguous call was replayed');
  assert.equal(document.getElementById('save-private').disabled, true);
  assert.equal((document.workspaceCurrent?.current?.state ?? ''), 'Needs attention');
  assert.match(document.getElementById('notice').textContent, /read-back was inconclusive/i);
  assert.match(document.getElementById('notice').textContent, /paused until Find folder changes succeeds/i);
});
