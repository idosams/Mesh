import { createPinPersistence } from './attachment-pin-persistence.js';
// Native attachment coordinator. React receives display data and emits bounded user intents.
const PHASES = new Set(['starting', 'scanning', 'saving', 'waiting', 'stopping', 'stopped', 'failed']);
const NATIVE_SIGNALS = new Set(['disabled', 'starting', 'active', 'unavailable', 'stopping', 'stopped']);
const OUTCOMES = new Set(['pending', 'saved', 'unchanged', 'incomplete', 'source-unavailable', 'store-unavailable', 'save-unavailable', 'cancelled']);
const safeText = (value, maximum) => typeof value === 'string' && value.length > 0
  && value.length <= maximum && !/[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(value);

function laneOrigin(value) {
  if (value === null || value === undefined) return null;
  if (value.schema === 'mesh.attachment-lane-unavailable/v1') return { unavailable: true };
  if (value.schema !== 'mesh.attachment-lane-origin/v1' || !/^[a-f0-9]{64}$/.test(value.source_project)
    || !/^[a-f0-9]{64}$/.test(value.source_version) || !/^[a-f0-9]{32}$/.test(value.request)
    || value.attribution !== 'unknown' || value.provider !== null) throw new Error('Invalid lane ancestry');
  return { unavailable: false, sourceProject: value.source_project, sourceVersion: value.source_version };
}
export function attachedLane(raw, id, version, request) {
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  if (value?.schema !== 'mesh.desktop-attachment-lane/v1' || value.source_project !== id
    || value.source_version !== version || value.request !== request
    || !/^[a-f0-9]{64}$/.test(value.project) || value.project === id) throw new Error('Lane allocation identity mismatch');
  return value.project;
}

export function attachedProjectList(raw) {
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  if (value?.schema !== 'mesh.desktop-attachments/v1' || !Array.isArray(value.projects)
    || value.projects.length > 32) throw new Error('Invalid attachment list');
  const ids = new Set();
  return value.projects.map((project) => {
    const capture = project?.capture;
    if (typeof project?.id !== 'string' || typeof project.generation !== 'string'
      || !/^[a-f0-9]{64}$/.test(project.id) || ids.has(project.id)
      || !/^[1-9][0-9]{0,19}$/.test(project.generation ?? '')
      || !safeText(project.root, 4096) || !project.root.startsWith('/')
      || (project.detached !== undefined && typeof project.detached !== 'boolean')
      || ![undefined, null, 'restored-stopped', 'unavailable'].includes(project.recovery)
      || capture?.schema !== 'mesh.attachment-capture/v1'
      || !PHASES.has(capture.phase) || !OUTCOMES.has(capture.last_outcome)
      || (capture.native_events !== undefined && typeof capture.native_events !== 'boolean')
      || (capture.native_signal_state !== undefined && (!NATIVE_SIGNALS.has(capture.native_signal_state)
        || capture.native_events !== (capture.native_signal_state === 'active')
        || (capture.native_signal_state === 'active' && ['stopping', 'stopped', 'failed'].includes(capture.phase))))
      || capture.attribution !== 'unknown' || capture.atomic_snapshot !== false
      || (capture.saved_version !== null && !/^[a-f0-9]{64}$/.test(capture.saved_version))
      || (capture.last_complete_capture_age_ms !== null
        && (!Number.isSafeInteger(capture.last_complete_capture_age_ms) || capture.last_complete_capture_age_ms < 0))) {
      throw new Error('Invalid attachment status');
    }
    ids.add(project.id);
    return Object.freeze({ lane: laneOrigin(project.lane), nativeSignalState: capture.native_signal_state ?? (capture.native_events ? 'active' : 'unavailable'), nativeEvents: capture.native_events ?? false, detached: project.detached ?? false, id: project.id, generation: project.generation, root: project.root,
      phase: capture.phase, outcome: capture.last_outcome, savedVersion: capture.saved_version,
      captureAgeMs: capture.last_complete_capture_age_ms, recovery: project.recovery ?? null });
  });
}

export function attachedVersionPage(raw, id, before) {
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  if (value?.schema !== 'mesh.attachment-versions/v1' || value.project !== id || value.before !== before
    || !Array.isArray(value.versions) || value.versions.length > 50
    || value.versions.some((version) => typeof version !== 'string' || !/^[a-f0-9]{64}$/.test(version))
    || new Set(value.versions).size !== value.versions.length
    || value.versions.includes(before)
    || (value.next_before !== null && (value.versions.length !== 50 || value.next_before !== value.versions.at(-1)))) {
    throw new Error('Invalid attachment version page');
  }
  return Object.freeze({ versions: Object.freeze([...value.versions]), nextBefore: value.next_before });
}

const parseInspection = (raw, id, operation, schema) => {
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  if (value?.project !== id || value.inspection?.operation !== operation || value.inspection.schema !== schema) {
    throw new Error('Saved inspection identity mismatch');
  }
  return value.inspection;
};
export function attachedEntries(raw, id, operation, after) {
  const value = parseInspection(raw, id, operation, 'mesh.attachment-entries/v1');
  if (value.after !== after || !Array.isArray(value.entries) || value.entries.length > 200) throw new Error('Invalid saved entries');
  const paths = new Set();
  for (const entry of value.entries) {
    if (!safeText(entry?.path, 4096) || entry.path.startsWith('/')
      || entry.path.split('/').some((part) => !part || part === '.' || part === '..')
      || paths.has(entry.path) || entry.path === after
      || !['file', 'folder'].includes(entry.kind)
      || (entry.kind === 'folder' && (entry.bytes !== null || entry.digest !== null || entry.executable !== null))
      || (entry.kind === 'file' && (!Number.isSafeInteger(entry.bytes) || entry.bytes < 0
        || typeof entry.digest !== 'string' || !/^[a-f0-9]{64}$/.test(entry.digest) || typeof entry.executable !== 'boolean'))) {
      throw new Error('Invalid saved entry');
    }
    paths.add(entry.path);
  }
  if (value.next_after !== null && (value.entries.length !== 200 || value.next_after !== value.entries.at(-1).path)) {
    throw new Error('Invalid entry cursor');
  }
  return { operation, entries: value.entries, nextAfter: value.next_after, file: null };
}
export function attachedText(raw, id, operation, entry) {
  const value = parseInspection(raw, id, operation, 'mesh.attachment-text/v1');
  if (value.path !== entry.path || value.digest !== entry.digest || value.bytes !== entry.bytes || value.executable !== entry.executable
    || !['text', 'binary', 'too-large'].includes(value.state)
    || (value.state === 'text' && (typeof value.text !== 'string' || value.text.length > 262144 || value.bytes > 262144))
    || (value.state !== 'text' && value.text !== null)) throw new Error('Saved text identity mismatch');
  return value;
}

export function attachedComparison(raw, id, base, target, after) {
  const envelope = typeof raw === 'string' ? JSON.parse(raw) : raw;
  const value = envelope?.comparison;
  if (envelope?.project !== id || value?.schema !== 'mesh.attachment-comparison/v1'
    || value.base !== base || value.target !== target || value.after !== after
    || !Array.isArray(value.changes) || value.changes.length > 200
    || !Number.isSafeInteger(value.total) || value.total < value.changes.length) throw new Error('Invalid saved comparison');
  const paths = new Set();
  for (const change of value.changes) {
    if (!safeText(change?.path, 4096) || change.path.startsWith('/')
      || change.path.split('/').some((part) => !part || part === '.' || part === '..')
      || paths.has(change.path) || change.path === after
      || !['added', 'removed', 'modified', 'mode-changed', 'type-changed'].includes(change.change)) throw new Error('Invalid comparison change');
    paths.add(change.path);
    for (const side of [change.before, change.after]) {
      if (side === null) continue;
      if (!side || !['file', 'folder'].includes(side.kind)
        || (side.kind === 'folder' && (side.bytes !== null || side.digest !== null || side.executable !== null))
        || (side.kind === 'file' && (!Number.isSafeInteger(side.bytes) || side.bytes < 0
          || typeof side.digest !== 'string' || !/^[a-f0-9]{64}$/.test(side.digest) || typeof side.executable !== 'boolean'))) throw new Error('Invalid comparison side');
    }
    if (change.change === 'added' ? change.before !== null || change.after === null
      : change.change === 'removed' ? change.before === null || change.after !== null
        : change.before === null || change.after === null) throw new Error('Invalid comparison presence');
  }
  if (value.next_after !== null && (value.changes.length !== 200 || value.next_after !== value.changes.at(-1).path)) throw new Error('Invalid comparison cursor');
  return { base, target, after, changes: value.changes, total: value.total, nextAfter: value.next_after, file: null };
}

const reviewIdentity = (value) => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
function reviewSummary(value) {
  const optionalIdentity = (value) => value === null || reviewIdentity(value);
  const path = (value) => value === null || (safeText(value, 4096) && !value.startsWith('/')
    && !value.split('/').some((part) => !part || part === '.' || part === '..'));
  if (!reviewIdentity(value?.bundle) || !reviewIdentity(value.target)
    || !optionalIdentity(value.reviewed_head) || !optionalIdentity(value.presentation)
    || typeof value.complete !== 'boolean' || value.approval_authority !== false || value.author_attribution !== 'unknown'
    || (value.unavailable !== null && !safeText(value.unavailable, 128))
    || !Array.isArray(value.changes) || value.changes.length > 128
    || ![value.changes_not_listed, value.operations_not_listed].every((count) => Number.isSafeInteger(count) && count >= 0)
    || (value.complete && (value.unavailable !== null || !value.reviewed_head || !value.presentation || value.changes_not_listed || value.operations_not_listed))
    || value.changes.some((change) => !path(change?.before) || !path(change.after)
      || (change.before === null && change.after === null) || !safeText(change.effect, 80))) throw new Error('Invalid saved review');
  return value;
}
export function attachedReview(raw, id, target, bundle = null) {
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  if (value?.schema !== 'mesh.desktop-attachment-review/v1' || value.project !== id
    || value.review?.target !== target || (bundle !== null && value.review.bundle !== bundle)) throw new Error('Review identity mismatch');
  return reviewSummary(value.review);
}
export function attachedReviews(raw, id) {
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  if (value?.schema !== 'mesh.desktop-attachment-reviews/v1' || value.project !== id || !Array.isArray(value.queue?.reviews)
    || value.queue.reviews.length > 32 || !Number.isSafeInteger(value.queue.not_listed) || value.queue.not_listed < 0) throw new Error('Invalid review queue');
  const reviews = value.queue.reviews.map(reviewSummary);
  if (new Set(reviews.map((review) => review.bundle)).size !== reviews.length) throw new Error('Duplicate saved review');
  return { reviews, notListed: value.queue.not_listed };
}

export function attachedMain(raw, id) {
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  const credential = value?.credential;
  const main = value?.main;
  if (value?.schema !== 'mesh.desktop-attachment-main/v1' || value.project !== id
    || typeof value.main_available !== 'boolean'
    || typeof credential?.available !== 'boolean' || typeof credential.enrolled !== 'boolean'
    || (credential.enrolled && !credential.available)
    || (credential.available ? credential.unavailable_reason !== null : !safeText(credential.unavailable_reason, 4096))
    || (!value.main_available && main !== null)
    || (main !== null && (!reviewIdentity(main?.head) || !reviewIdentity(main.bundle) || !reviewIdentity(main.target)))) {
    throw new Error('Invalid attachment main');
  }
  return { available: credential.available, enrolled: credential.enrolled, reason: credential.unavailable_reason,
    mainAvailable: value.main_available, main };
}
export function attachedApproval(raw, id, review) {
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  if (value?.schema !== 'mesh.desktop-attachment-approval/v1' || value.project !== id
    || value.bundle !== review.bundle || value.target !== review.target || value.head !== review.reviewed_head
    || !reviewIdentity(value.head)) throw new Error('Approval result identity mismatch');
  return value;
}

const integrationStatuses = { 'matches-base': 'matches_base', 'already-present': 'already_present', 'preserve-current': 'preserve_current', conflict: 'conflicts', blocked: 'blocked' };
export function attachedIntegration(raw, id, main) {
  const outer = typeof raw === 'string' ? JSON.parse(raw) : raw;
  const value = outer?.preview;
  const count = value => Number.isSafeInteger(value) && value >= 0 && value <= 300000;
  const entry = value => value === null || (value?.kind === 'folder'
    ? value.bytes === null && value.digest === null && value.executable === null
    : value?.kind === 'file' && Number.isSafeInteger(value.bytes) && value.bytes >= 0
      && reviewIdentity(value.digest) && typeof value.executable === 'boolean');
  if (outer?.schema !== 'mesh.desktop-attachment-integration/v1' || outer.project !== id
    || value?.schema !== 'mesh.attachment-integration-preview/v1' || value.head !== main.head
    || value.bundle !== main.bundle || value.target !== main.target
    || !reviewIdentity(value.base_head) || !reviewIdentity(value.observed_digest)
    || value.atomic_snapshot !== false || value.write_authority !== false
    || !Object.values(integrationStatuses).every(key => count(value[key])) || !count(value.not_listed)
    || !Array.isArray(value.entries) || value.entries.length > 200) throw new Error('Invalid working-folder comparison');
  const seen = new Set(); const shown = {};
  for (const item of value.entries) {
    if (!safeText(item?.path, 4096) || item.path.startsWith('/') || item.path.split('/').some(part => !part || part === '.' || part === '..')
      || seen.has(item.path) || !Object.hasOwn(integrationStatuses, item.status)
      || ![item.base, item.target, item.current].every(entry)
      || (item.base === null && item.target === null && item.current === null)
      || (item.status === 'conflict' ? !['current-content-diverged', 'contains-current-work', 'contains-unobserved-entry'].includes(item.reason)
        : item.status === 'blocked' ? item.reason !== 'parent-change-conflicts' : item.reason !== null)) throw new Error('Invalid divergence entry');
    seen.add(item.path); shown[item.status] = (shown[item.status] ?? 0) + 1;
  }
  if (Object.entries(integrationStatuses).some(([status, key]) => (shown[status] ?? 0) > value[key])
    || Object.values(integrationStatuses).reduce((sum, key) => sum + value[key], 0) !== value.entries.length + value.not_listed) throw new Error('Invalid divergence counts');
  return value;
}

const recoveryTransaction = value => typeof value === 'string' && /^(integration|restoration)-[0-9a-f]{32}$/.test(value);
const groupIdentity = value => typeof value === 'string' && /^integration-group-[0-9a-f]{32}$/.test(value);
const recoveryStatuses = new Set(['group-reference', 'prepared-arrangement', 'applied-arrangement', 'changed-files', 'identity-mismatch', 'incomplete-observation', 'invalid-outcome', 'contradictory-outcome', 'invalid-receipt', 'unverified-history', 'unavailable-directory', 'unrecognized-directory-entry']);
export function attachedRecovery(raw, id, transaction = null, group = null) {
  const outer = typeof raw === 'string' ? JSON.parse(raw) : raw;
  const value = outer?.recovery;
  if (outer?.schema !== 'mesh.desktop-attachment-recovery/v1' || outer.project !== id || (group === null ? outer.group !== undefined : outer.group !== group || !groupIdentity(group))) throw new Error('Recovery identity mismatch');
  if (value === null && transaction === null) return { entries: [], more: false };
  if (value?.schema !== 'mesh.attachment-integration-recovery/v1' || value.project !== id
    || value.automatic_replay !== false || value.write_authority !== false || typeof value.more !== 'boolean'
    || !Number.isSafeInteger(value.live_content_budget_remaining) || value.live_content_budget_remaining < 0
    || !Array.isArray(value.entries) || value.entries.length > 32
    || (transaction !== null && (!recoveryTransaction(transaction) || value.more || value.entries.length !== 1 || value.entries[0].transaction !== transaction))) throw new Error('Invalid recovery observation');
  const observation = item => item === null || (safeText(item?.installation, 128)
    && reviewIdentity(item.digest) && Number.isSafeInteger(item.mode) && item.mode >= 0o100000 && item.mode <= 0o107777
    && Number.isSafeInteger(item.bytes) && item.bytes >= 0 && reviewIdentity(item.native_metadata_digest));
  const seen = new Set();
  const entries = value.entries.map(item => {
    if (!recoveryStatuses.has(item?.status) || typeof item.attention_required !== 'boolean'
      || !['observation_final', 'atomic_snapshot', 'write_authority', 'automatic_replay', 'cleanup_authority'].every(key => item[key] === false)
      || (item.status === 'group-reference' ? !groupIdentity(item.transaction) || item.details !== null : item.status === 'unrecognized-directory-entry' ? item.transaction !== '' : !recoveryTransaction(item.transaction))
      || (item.transaction && seen.has(item.transaction))) throw new Error('Invalid recovery entry');
    if (item.transaction) seen.add(item.transaction);
    const details = item.details;
    if (details !== null && (!safeText(details?.path, 4096) || details.path.startsWith('/')
      || details.path.split('/').some(part => !part || part === '.' || part === '..')
      || !['apply-approved', 'restore-retained', 'add-approved', 'remove-approved'].includes(details.operation)
      || details.content_is_approved_main !== (details.operation !== 'restore-retained')
      || !reviewIdentity(details.approved_head) || typeof details.is_current_main !== 'boolean'
      || !observation(details.source) || !observation(details.retained)
      || (details.retained_file_is_displaced !== undefined && typeof details.retained_file_is_displaced !== 'boolean')
      || (details.parent_policy_matches !== undefined && details.parent_policy_matches !== null && typeof details.parent_policy_matches !== 'boolean')
      || details.current_exclusions_checked !== false
      || !['absent', 'invalid', 'applied-observed', 'reconciliation-required'].includes(details.recorded_outcome))) throw new Error('Invalid recovery details');
    return { group, transaction: item.transaction, status: item.status, attention: item.attention_required,
      path: details?.path ?? null, operation: details?.operation ?? null,
      retainedAvailable: details?.retained_file_is_displaced === true && details.retained !== null && item.status !== 'prepared-arrangement',
      recordedOutcome: details?.recorded_outcome ?? null };
  });
  return { entries, more: value.more };
}
export function attachedFileChange(raw, id, restoration, group = null) {
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  const outcome = value?.outcome;
  const creation = restoration && outcome?.schema === 'mesh.attachment-file-restoration-addition-result/v1';
  if ((group === null ? value?.group !== undefined : value?.group !== group || !groupIdentity(group))
    || value?.schema !== 'mesh.desktop-attachment-file-change/v1' || value.project !== id
    || !recoveryTransaction(value.transaction) || !value.transaction.startsWith(restoration ? 'restoration-' : 'integration-')
    || outcome?.schema !== (creation ? 'mesh.attachment-file-restoration-addition-result/v1' : restoration ? 'mesh.attachment-file-restoration-result/v1' : 'mesh.attachment-file-integration-result/v1')
    || !reviewIdentity(outcome.proposal_digest) || !['applied-observed', 'reconciliation-required'].includes(outcome.status)
    || outcome.displaced_file_retained !== !creation || outcome.observation_final !== false) throw new Error('Invalid file-change result');
  return { transaction: value.transaction, status: outcome.status, creation };
}

export function attachedGroupChange(raw, id) {
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  const outcome = value?.outcome;
  if (value?.schema !== 'mesh.desktop-attachment-group-change/v1' || value.project !== id || !groupIdentity(value.group)
    || outcome?.schema !== 'mesh.attachment-integration-group-result/v1' || !reviewIdentity(outcome.proposal_digest)
    || !['applied-observed', 'reconciliation-required'].includes(outcome.status) || outcome.observation_final !== false
    || outcome.automatic_replay !== false || outcome.displaced_files_retained !== true
    || !Array.isArray(outcome.members) || !outcome.members.length || outcome.members.length > 64
    || new Set(outcome.members.map(item => item.transaction)).size !== outcome.members.length
    || outcome.members.some(item => !recoveryTransaction(item.transaction) || !['applied-observed', 'reconciliation-required', 'not-attempted'].includes(item.status))
    || (outcome.status === 'applied-observed' && outcome.members.some(item => item.status !== 'applied-observed'))) throw new Error('Invalid group outcome');
  return { group: value.group, status: outcome.status, members: outcome.members.map(item => ({ transaction: item.transaction, status: item.status })) };
}
export function attachedGroupRecovery(raw, id, group) {
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  if (value?.schema !== 'mesh.attachment-integration-group-recovery/v1' || value.project !== id || value.group !== group || !groupIdentity(group)
    || !reviewIdentity(value.proposal_digest) || value.automatic_replay !== false || value.write_authority !== false
    || value.observations_are_atomic !== false || value.already_present_is_preparation_evidence !== true
    || !Array.isArray(value.members) || !value.members.length || !Array.isArray(value.already_present) || value.members.length + value.already_present.length > 64) throw new Error('Invalid group recovery');
  if (value.members.some(member => !recoveryTransaction(member?.transaction))) throw new Error('Invalid group member');
  const entries = value.members.map(member => attachedRecovery({ schema: 'mesh.desktop-attachment-recovery/v1', project: id, group, recovery: member.recovery }, id, member.transaction, group).entries[0]);
  const paths = value.already_present;
  if (new Set(entries.map(entry => entry.transaction)).size !== entries.length || new Set(paths).size !== paths.length
    || paths.some(path => !safeText(path, 4096) || path.startsWith('/') || path.split('/').some(part => !part || part === '.' || part === '..'))) throw new Error('Invalid group membership');
  const restorations = value.restoration_references ?? [];
  const moreRestorations = value.more_restoration_references_may_exist ?? true;
  if (!Array.isArray(restorations) || restorations.length > 32 || new Set(restorations).size !== restorations.length
    || restorations.some(tx => !recoveryTransaction(tx) || !tx.startsWith('restoration-')) || typeof moreRestorations !== 'boolean') throw new Error('Invalid group restoration references');
  let execution = null;
  if (value.execution !== undefined) {
    const record = value.execution;
    if (record?.schema !== 'mesh.attachment-integration-group-execution/v1' || !['recorded', 'no-outcome', 'invalid', 'changed'].includes(record.status)
      || record.historical !== true || record.observation_final !== false || record.automatic_replay !== false || record.write_authority !== false
      || !Array.isArray(record.attempts) || record.attempts.length !== entries.length
      || record.attempts.some((item, index) => item?.transaction !== entries[index].transaction || !['recorded', 'absent', 'invalid'].includes(item.status))) throw new Error('Invalid group execution evidence');
    if (['recorded', 'no-outcome'].includes(record.status)) {
      let gap = false;
      for (const attempt of record.attempts) {
        if (attempt.status === 'invalid' || (gap && attempt.status === 'recorded')) throw new Error('Inconsistent group attempt order');
        gap ||= attempt.status === 'absent';
      }
    }
    let outcome = null;
    if (record.status === 'recorded') {
      outcome = attachedGroupChange({ schema: 'mesh.desktop-attachment-group-change/v1', project: id, group, outcome: record.outcome }, id);
      if (record.outcome.proposal_digest !== value.proposal_digest || outcome.members.length !== entries.length
        || outcome.members.some((item, index) => item.transaction !== entries[index].transaction)) throw new Error('Group execution membership mismatch');
    } else if (record.outcome !== null) throw new Error('Unverified group execution outcome');
    execution = { status: record.status, attempts: record.attempts.map(item => ({transaction: item.transaction, status: item.status})), outcome };
  }
  return { group, entries, alreadyPresent: [...paths], restorations: [...restorations], moreRestorations, execution };
}

export function startAttachedProjects({ document, invoke, CustomEvent, schedule = setTimeout, cancel = clearTimeout, requestId = () => globalThis.crypto.randomUUID().replaceAll('-', '') }) {
  let projects = [];
  let histories = {};
  let inspections = {};
  let bases = {};
  let comparisons = {};
  let reviewQueues = {};
  let selectedReviews = {};
  let reviewNavigation = null;
  let pendingReviewNavigation = null;
  let approvalStates = {};
  let approvalFeedback = {};
  let integrationPreviews = {};
  let integrationErrors = {};
  let recoveries = {};
  let selectedRecovery = {};
  let groupRecoveries = {};
  let groupOutcomes = {};
  let groupRecoveryErrors = {};
  let recoveryErrors = {};
  let fileChangeFeedback = {};
  let laneRequests = {};
  let laneFeedback = {};
  let pins = [];
  let nextPin = 1n;
  let pinStatus = 'loading';
  let pinError = '';
  let busy = false;
  let error = '';
  let mounted = false;
  let disposed = false;
  let timer = null;
  const publish = () => {
    if (!disposed) document.dispatchEvent(new CustomEvent('mesh:attachments-projection', {
      detail: { projects, histories, inspections, bases, comparisons, reviewQueues, selectedReviews, reviewNavigation, approvalStates, approvalFeedback, integrationPreviews, integrationErrors, recoveries, selectedRecovery, groupRecoveries, groupOutcomes, groupRecoveryErrors, recoveryErrors, fileChangeFeedback, laneRequests, laneFeedback, pins, pinStatus, pinError, busy, error, available: typeof invoke === 'function' },
    }));
  };
  async function createLane(id, pending) {
    try {
      attachedLane(await invoke('open_attached_version_lane', { id, version: pending.version, request: pending.request }),
        id, pending.version, pending.request);
      laneFeedback = { ...laneFeedback, [id]: 'Independent line created. Its folder and history appear below.' };
      laneRequests = { ...laneRequests }; delete laneRequests[id];
    } catch {
      laneFeedback = { ...laneFeedback, [id]: 'The line could not be confirmed. Retry the same request to avoid duplicating it. Partial work is retained.' };
    }
  }
  async function readRecovery(id, transaction = null, group = null) {
    const raw = group === null ? await invoke('inspect_attached_recovery', { id, transaction }) : await invoke('inspect_attached_group_file', { id, transaction, group });
    const observation = attachedRecovery(raw, id, transaction, group);
    if (transaction === null) recoveries = { ...recoveries, [id]: observation };
    else selectedRecovery = { ...selectedRecovery, [id]: observation.entries[0] };
    recoveryErrors = { ...recoveryErrors, [id]: '' };
  }
  async function changeFile(id, command, args, restoration, group = null) {
    try {
      const result = attachedFileChange(await invoke(command, args), id, restoration, group);
      fileChangeFeedback = { ...fileChangeFeedback, [id]: result.status === 'applied-observed'
        ? result.creation ? 'The retained snapshot was restored as a new file. The original retained file remains available for later editor writes.' : 'The file change was observed. Previous working content is retained; later editor writes may still arrive.'
        : 'The file change needs reconciliation. Retained work remains available; inspect recovery before continuing.' };
      await readRecovery(id, result.transaction, group);
    } catch {
      fileChangeFeedback = { ...fileChangeFeedback, [id]: 'The file change was not confirmed or was cancelled. Inspect recovery before retrying; this does not prove the working file is unchanged.' };
    }
    try { await readRecovery(id); }
    catch { recoveryErrors = { ...recoveryErrors, [id]: 'Recovery could not be refreshed. Any previous observation is retained and may be out of date.' }; }
  }
  async function readGroup(id, group) {
    try {
      const observation = attachedGroupRecovery(await invoke('inspect_attached_group_recovery', { id, group }), id, group);
      groupRecoveries = { ...groupRecoveries, [id]: observation };
      groupRecoveryErrors = { ...groupRecoveryErrors, [id]: '' };
    } catch {
      groupRecoveryErrors = { ...groupRecoveryErrors, [id]: 'This recovery group could not be verified. Previous observations may be out of date.' };
      throw new Error('Group recovery unavailable');
    }
  }
  async function changeGroup(id, preview) {
    let result = null;
    try {
      result = attachedGroupChange(await invoke('apply_attached_main_group', { id, bundle: preview.bundle, target: preview.target }), id);
      groupOutcomes = { ...groupOutcomes, [id]: result };
      fileChangeFeedback = { ...fileChangeFeedback, [id]: result.status === 'applied-observed'
        ? 'The accepted group was observed in the working folder. Recovery records remain available.'
        : 'The group needs reconciliation. Some files may have changed; remaining changes were stopped. Inspect each member before continuing.' };
    } catch {
      fileChangeFeedback = { ...fileChangeFeedback, [id]: 'The group was not confirmed or was cancelled. Inspect recovery before retrying; some working files may have changed.' };
    }
    if (result) { try { await readGroup(id, result.group); } catch { /* Keep the verified outcome and expose the separate inspection failure. */ } }
    integrationPreviews = { ...integrationPreviews }; delete integrationPreviews[id];
    try { await readRecovery(id); }
    catch { recoveryErrors = { ...recoveryErrors, [id]: 'Recovery could not be refreshed; previous observations may be out of date.' }; }
  }
  async function readApproval(id) {
    const status = attachedMain(await invoke('attachment_approval_status', { id }), id);
    approvalStates = { ...approvalStates, [id]: status };
  }
  function forgetApproval(id) {
    approvalStates = { ...approvalStates };
    delete approvalStates[id];
  }
  async function readComparisonFile(id, comparison, change) {
    const read = async (side, operation) => side?.kind === 'file'
      ? attachedText(await invoke('inspect_attached_version', { id, operation, path: change.path, after: null }),
        id, operation, { ...side, path: change.path }) : null;
    const [before, after] = await Promise.all([read(change.before, comparison.base), read(change.after, comparison.target)]);
    return { path: change.path, before, after, beforeKind: change.before?.kind ?? 'absent', afterKind: change.after?.kind ?? 'absent' };
  }
  async function hydrate(selector) {
    const project = projects.find((project) => project.id === selector.project);
    const pin = { key: selector.key, project: selector.project, root: project?.root ?? selector.project, selector, comparison: null };
    try {
      if (!project) return pin;
      const args = { id: selector.project, base: selector.base, target: selector.target, after: selector.after };
      const comparison = attachedComparison(await invoke('compare_attached_versions', args), args.id, args.base, args.target, args.after);
      if (selector.path !== null) {
        let change = comparison.changes.find((change) => change.path === selector.path);
        if (!change) {
          const exact = attachedComparison(await invoke('compare_attached_path', {
            id: args.id, base: args.base, target: args.target, path: selector.path,
          }), args.id, args.base, args.target, null);
          if (exact.total !== 1 || exact.changes.length !== 1 || exact.changes[0].path !== selector.path) throw new Error('Selected change mismatch');
          change = exact.changes[0];
        }
        comparison.file = await readComparisonFile(args.id, comparison, change);
      }
      return { ...pin, comparison };
    } catch { return pin; }
  }
  const persistence = createPinPersistence({
    invoke,
    selectors: () => pins.map((pin) => pin.selector),
    restore: async (selectors) => {
      const restored = [];
      for (const selector of selectors) restored.push(await hydrate(selector));
      if (disposed) return;
      pins = restored;
      for (const pin of pins) if (BigInt(pin.key) >= nextPin) nextPin = BigInt(pin.key) + 1n;
    },
    status: (phase, message) => { pinStatus = phase; pinError = message; publish(); },
  });
  const planRefresh = () => {
    if (timer !== null) cancel(timer);
    timer = mounted && !disposed ? schedule(() => { timer = null; void run(); }, 2000) : null;
  };
  async function run(operation) {
    if (busy || disposed || typeof invoke !== 'function') return;
    busy = true;
    publish();
    try {
      if (operation) await operation();
      projects = attachedProjectList(await invoke('attached_projects'));
      await persistence.ensureLoaded();
      error = '';
    } catch {
      error = 'Attachment status is unavailable. Your existing tools can keep working. Retry to refresh.';
    } finally {
      busy = false;
      publish();
      planRefresh();
      if (pendingReviewNavigation && !disposed) {
        const detail = pendingReviewNavigation; pendingReviewNavigation = null;
        intent({ detail });
      }
    }
  }
  function visible(event) {
    mounted = event.detail === true;
    if (mounted) { publish(); void run(); }
    else if (timer !== null) { cancel(timer); timer = null; }
  }
  function intent(event) {
    const value = event.detail;
    if (!mounted || !value || typeof value !== 'object') return;
    if (pinStatus !== 'loading' && value.type === 'close-pin' && Object.keys(value).length === 2 && typeof value.pin === 'string') {
      pins = pins.filter((pin) => pin.key !== value.pin); persistence.changed(); publish(); return;
    }
    if (value.type === 'open-exact-review' && Object.keys(value).length === 4
      && projects.some(project => project.id === value.id)
      && reviewIdentity(value.bundle) && reviewIdentity(value.target)) {
      if (busy) { pendingReviewNavigation = { ...value }; return; }
      void run(async () => {
        const review = attachedReview(await invoke('inspect_attached_review', {
          id: value.id, bundle: value.bundle, target: value.target,
        }), value.id, value.target, value.bundle);
        if (disposed) return;
        selectedReviews = { ...selectedReviews, [value.id]: review };
        reviewNavigation = { id: value.id, bundle: review.bundle, target: review.target,
          sequence: (reviewNavigation?.sequence ?? 0) + 1 };
      }); return;
    }
    if (busy) return;
    if (['retry-pin-save', 'reload-pins'].includes(value.type) && Object.keys(value).length === 1) {
      void run(() => value.type === 'retry-pin-save' ? persistence.retry() : persistence.reload()); return;
    }
    if (value.type === 'retry-pin' && Object.keys(value).length === 2) {
      const pinned = pins.find((pin) => pin.key === value.pin);
      if (pinned) void run(async () => {
        const restored = await hydrate(pinned.selector);
        pins = pins.map((pin) => pin === pinned ? restored : pin);
      });
      return;
    }
    if ('id' in value && !projects.some((project) => project.id === value.id)) return;
    if (value.type === 'open-folder' && Object.keys(value).length === 2 && projects.some(project => project.id === value.id)) {
      void run(() => invoke('open_attached_folder', { id: value.id })); return;
    }
    if (value.type === 'create-lane' && Object.keys(value).length === 3
      && histories[value.id]?.versions.includes(value.version) && !laneRequests[value.id]) {
      const request = requestId();
      if (typeof request !== 'string' || !/^[a-f0-9]{32}$/.test(request)) return;
      const pending = { request, version: value.version };
      laneRequests = { ...laneRequests, [value.id]: pending };
      void run(() => createLane(value.id, pending)); return;
    }
    if (value.type === 'retry-lane' && Object.keys(value).length === 2 && laneRequests[value.id]) {
      void run(() => createLane(value.id, laneRequests[value.id])); return;
    }
    if (value.type === 'refresh' && Object.keys(value).length === 1) { void run(); return; }
    if (value.type === 'choose' && Object.keys(value).length === 1) {
      void run(async () => {
        const source = await invoke('pick_folder');
        if (source === null) return;
        if (!safeText(source, 4096) || !source.startsWith('/')) throw new Error('Invalid folder selection');
        await invoke('attach_existing_project', { source });
      });
      return;
    }
    if (value.type === 'attach' && Object.keys(value).length === 2
      && safeText(value.source, 4096) && value.source.startsWith('/')) {
      void run(() => invoke('attach_existing_project', { source: value.source }));
      return;
    }
    if (value.type === 'versions' && Object.keys(value).length === 3
      && projects.some((project) => project.id === value.id)
      && (value.before === null || (typeof value.before === 'string' && value.before === histories[value.id]?.nextBefore))) {
      void run(async () => {
        const page = attachedVersionPage(await invoke('attached_project_versions', {
          id: value.id, before: value.before,
        }), value.id, value.before);
        histories = { ...histories, [value.id]: page };
      });
      return;
    }
    if (['reviews', 'request-review', 'open-review', 'review-files', 'check-approval', 'enroll-approval', 'approve-review', 'open-main', 'compare-main', 'recovery', 'lookup-recovery', 'restore-retained', 'restore-group-file', 'apply-main-file', 'apply-main-group', 'lookup-group', 'lookup-group-file'].includes(value.type)
      && !projects.some((project) => project.id === value.id)) return;
    if ((value.type === 'recovery' && Object.keys(value).length === 2)
      || (value.type === 'lookup-recovery' && Object.keys(value).length === 3 && recoveryTransaction(value.transaction))) {
      void run(async () => {
        try { await readRecovery(value.id, value.type === 'recovery' ? null : value.transaction); }
        catch { recoveryErrors = { ...recoveryErrors, [value.id]: 'Recovery could not be refreshed. Any previous observation is retained and may be out of date.' }; }
      }); return;
    }
    if (value.type === 'lookup-group' && Object.keys(value).length === 3 && groupIdentity(value.group)) {
      void run(async () => { try { await readGroup(value.id, value.group); }
        catch { /* readGroup retains the prior view and its own error until a successful group refresh. */ } }); return;
    }
    if (value.type === 'lookup-group-file' && Object.keys(value).length === 4 && groupIdentity(value.group) && recoveryTransaction(value.transaction)) {
      void run(async () => { try { await readRecovery(value.id, value.transaction, value.group); }
        catch { recoveryErrors = { ...recoveryErrors, [value.id]: 'The group file could not be inspected. Previous observations may be out of date.' }; } }); return;
    }
    if (value.type === 'restore-group-file' && Object.keys(value).length === 4 && groupIdentity(value.group) && recoveryTransaction(value.transaction)) {
      const entry = [...(groupRecoveries[value.id]?.entries ?? []), selectedRecovery[value.id]]
        .find(item => item?.group === value.group && item.transaction === value.transaction && item.retainedAvailable);
      if (!entry || recoveryErrors[value.id] || groupRecoveryErrors[value.id] || projects.find(project => project.id === value.id)?.detached) return;
      void run(() => changeFile(value.id, 'restore_attached_group_file', { id: value.id, group: value.group, transaction: value.transaction }, true, value.group)); return;
    }
    if (value.type === 'apply-main-group' && Object.keys(value).length === 2) {
      const preview = integrationPreviews[value.id];
      const main = approvalStates[value.id]?.main;
      if (!preview || main?.head !== preview.head || integrationErrors[value.id] || projects.find(project => project.id === value.id)?.detached) return;
      void run(() => changeGroup(value.id, preview)); return;
    }
    if (value.type === 'restore-retained' && Object.keys(value).length === 3 && recoveryTransaction(value.transaction)) {
      const entry = [...(recoveries[value.id]?.entries ?? []), selectedRecovery[value.id]]
        .find(item => item?.group === null && item.transaction === value.transaction && item.retainedAvailable);
      if (!entry || recoveryErrors[value.id] || projects.find(project => project.id === value.id)?.detached) return;
      void run(() => changeFile(value.id, 'restore_attached_retained_file', { id: value.id, transaction: value.transaction }, true)); return;
    }
    if (value.type === 'apply-main-file' && Object.keys(value).length === 3) {
      const preview = integrationPreviews[value.id];
      const main = approvalStates[value.id]?.main;
      const entry = preview?.entries.find(item => item.path === value.path);
      if (!entry || entry.status !== 'matches-base' || entry.base?.kind !== 'file' || entry.target?.kind !== 'file'
        || main?.head !== preview.head || integrationErrors[value.id] || projects.find(project => project.id === value.id)?.detached) return;
      void run(() => changeFile(value.id, 'apply_attached_main_file', {
        id: value.id, bundle: preview.bundle, target: preview.target, path: entry.path,
      }, false)); return;
    }
    if (value.type === 'check-approval' && Object.keys(value).length === 2) {
      void run(async () => {
        forgetApproval(value.id);
        try { await readApproval(value.id); approvalFeedback = { ...approvalFeedback, [value.id]: '' }; }
        catch { approvalFeedback = { ...approvalFeedback, [value.id]: 'Mesh main and approval availability could not be checked. Retry when ready.' }; }
      }); return;
    }
    if (value.type === 'enroll-approval' && Object.keys(value).length === 2
      && approvalStates[value.id]?.available && !approvalStates[value.id]?.enrolled) {
      void run(async () => {
        forgetApproval(value.id);
        try {
          await invoke('enroll_approval_credential');
          await readApproval(value.id);
          approvalFeedback = { ...approvalFeedback, [value.id]: '' };
        } catch { approvalFeedback = { ...approvalFeedback, [value.id]: 'Approval setup was not confirmed. Check availability before retrying.' }; }
      }); return;
    }
    if (value.type === 'compare-main' && Object.keys(value).length === 2 && approvalStates[value.id]?.main) {
      const main = approvalStates[value.id].main;
      void run(async () => {
        try {
          const preview = attachedIntegration(await invoke('preview_attached_main_integration', {
            id: value.id, bundle: main.bundle, target: main.target,
          }), value.id, main);
          integrationPreviews = { ...integrationPreviews, [value.id]: preview };
          integrationErrors = { ...integrationErrors, [value.id]: '' };
        } catch {
          integrationErrors = { ...integrationErrors, [value.id]: 'A fresh working-folder comparison is unavailable. Any previous observation is retained. Refresh main and retry.' };
        }
      }); return;
    }
    if (value.type === 'open-main' && Object.keys(value).length === 2 && approvalStates[value.id]?.main) {
      const main = approvalStates[value.id].main;
      void run(async () => {
        const review = attachedReview(await invoke('inspect_attached_review', {
          id: value.id, bundle: main.bundle, target: main.target,
        }), value.id, main.target, main.bundle);
        if (review.reviewed_head !== main.head) throw new Error('Main review identity mismatch');
        selectedReviews = { ...selectedReviews, [value.id]: review };
      }); return;
    }
    if (value.type === 'approve-review' && Object.keys(value).length === 4) {
      const selected = selectedReviews[value.id];
      const status = approvalStates[value.id];
      if (!selected?.complete || selected.unavailable || selected.bundle !== value.bundle || selected.target !== value.target
        || !status?.available || !status.enrolled || !status.mainAvailable || status.main?.head === selected.reviewed_head) return;
      void run(async () => {
        forgetApproval(value.id);
        try {
          attachedApproval(await invoke('approve_attached_review', { id: value.id, bundle: value.bundle, target: value.target }), value.id, selected);
          await readApproval(value.id);
          approvalFeedback = { ...approvalFeedback, [value.id]: 'Approval confirmed. Mesh main was refreshed.' };
        } catch {
          // The native append may have succeeded before a reply was lost. Never claim rollback.
          approvalFeedback = { ...approvalFeedback, [value.id]: 'Approval was not confirmed or was cancelled. Refresh Mesh main before retrying.' };
          try { await readApproval(value.id); } catch { forgetApproval(value.id); }
        }
      }); return;
    }
    if (value.type === 'reviews' && Object.keys(value).length === 2) {
      void run(async () => {
        const queue = attachedReviews(await invoke('attached_project_reviews', { id: value.id }), value.id);
        reviewQueues = { ...reviewQueues, [value.id]: queue };
      }); return;
    }
    if (value.type === 'request-review' && Object.keys(value).length === 3 && histories[value.id]?.versions.includes(value.target)) {
      void run(async () => {
        const review = attachedReview(await invoke('request_attached_review', { id: value.id, target: value.target }), value.id, value.target);
        selectedReviews = { ...selectedReviews, [value.id]: review };
        const queue = attachedReviews(await invoke('attached_project_reviews', { id: value.id }), value.id);
        reviewQueues = { ...reviewQueues, [value.id]: queue };
      }); return;
    }
    if (value.type === 'open-review' && Object.keys(value).length === 4
      && reviewQueues[value.id]?.reviews.some((review) => review.bundle === value.bundle && review.target === value.target)) {
      void run(async () => {
        const review = attachedReview(await invoke('inspect_attached_review', {
          id: value.id, bundle: value.bundle, target: value.target,
        }), value.id, value.target, value.bundle);
        selectedReviews = { ...selectedReviews, [value.id]: review };
      }); return;
    }
    if (value.type === 'review-files' && Object.keys(value).length === 4
      && selectedReviews[value.id]?.bundle === value.bundle && selectedReviews[value.id]?.target === value.target) {
      void run(async () => {
        const inspected = attachedEntries(await invoke('inspect_attached_version', {
          id: value.id, operation: value.target, path: null, after: null,
        }), value.id, value.target, null);
        inspections = { ...inspections, [value.id]: inspected };
      }); return;
    }
    if (value.type === 'inspect' && Object.keys(value).length === 3
      && histories[value.id]?.versions.includes(value.operation)) {
      void run(async () => {
        const inspected = attachedEntries(await invoke('inspect_attached_version', {
          id: value.id, operation: value.operation, path: null, after: null,
        }), value.id, value.operation, null);
        inspections = { ...inspections, [value.id]: inspected };
      });
      return;
    }
    const selected = inspections[value.id];
    if (selected && value.type === 'entries' && Object.keys(value).length === 4 && selected.operation === value.operation
      && typeof value.after === 'string' && value.after === selected.nextAfter) {
      void run(async () => {
        const inspected = attachedEntries(await invoke('inspect_attached_version', {
          id: value.id, operation: value.operation, path: null, after: value.after,
        }), value.id, value.operation, value.after);
        inspections = { ...inspections, [value.id]: { ...inspected, file: selected.file } };
      });
      return;
    }
    if (selected && value.type === 'file' && Object.keys(value).length === 4 && selected.operation === value.operation) {
      const entry = selected.entries.find((entry) => entry.path === value.path && entry.kind === 'file');
      if (!entry) return;
      void run(async () => {
        const file = attachedText(await invoke('inspect_attached_version', {
          id: value.id, operation: value.operation, path: value.path, after: null,
        }), value.id, value.operation, entry);
        inspections = { ...inspections, [value.id]: { ...selected, file } };
      });
      return;
    }
    if (value.type === 'set-base' && Object.keys(value).length === 3 && histories[value.id]?.versions.includes(value.operation)) {
      bases = { ...bases, [value.id]: value.operation }; publish(); return;
    }
    if (value.type === 'compare' && Object.keys(value).length === 3 && bases[value.id]
      && histories[value.id]?.versions.includes(value.target)) {
      const base = bases[value.id];
      void run(async () => {
        const comparison = attachedComparison(await invoke('compare_attached_versions', {
          id: value.id, base, target: value.target, after: null,
        }), value.id, base, value.target, null);
        comparisons = { ...comparisons, [value.id]: comparison };
      });
      return;
    }
    if (value.type === 'pin-comparison' && Object.keys(value).length === 4 && pins.length < 8
      && pinStatus !== 'loading' && !pinError && nextPin <= 18446744073709551615n && comparisons[value.id] && comparisons[value.id].base === value.base
      && comparisons[value.id].target === value.target) {
      const project = projects.find((project) => project.id === value.id);
      const comparison = comparisons[value.id];
      const key = String(nextPin++);
      const selector = { key, project: value.id, base: comparison.base, target: comparison.target, after: comparison.after, path: comparison.file?.path ?? null };
      pins = [...pins, { key, project: value.id, root: project.root, selector, comparison }];
      persistence.changed();
      publish(); return;
    }
    const hasPin = Object.hasOwn(value, 'pin');
    const pinned = hasPin ? pins.find((pin) => pin.key === value.pin && pin.project === value.id) : null;
    if (hasPin && !pinned) return;
    const comparison = pinned ? pinned.comparison : comparisons[value.id];
    const updateComparison = (next) => {
      // A pin closed during an outstanding read must not be recreated by its response.
      if (pinned) {
        if (!pins.includes(pinned)) return;
        pins = pins.map((pin) => pin === pinned ? { ...pin, comparison: next,
          selector: { ...pin.selector, after: next.after, path: next.file?.path ?? null } } : pin);
        persistence.changed();
      }
      else comparisons = { ...comparisons, [value.id]: next };
    };
    if (value.type === 'compare-page' && Object.keys(value).length === (hasPin ? 6 : 5)
      && comparison && comparison.base === value.base && comparison.target === value.target
      && typeof value.after === 'string' && comparison.nextAfter === value.after) {
      void run(async () => {
        const page = attachedComparison(await invoke('compare_attached_versions', {
          id: value.id, base: value.base, target: value.target, after: value.after,
        }), value.id, value.base, value.target, value.after);
        updateComparison({ ...page, file: comparison.file });
      });
      return;
    }
    if (value.type === 'compare-file' && Object.keys(value).length === (hasPin ? 6 : 5)
      && comparison && comparison.base === value.base && comparison.target === value.target) {
      const change = comparison.changes.find((change) => change.path === value.path);
      if (!change) return;
      void run(async () => {
        const read = async (side, operation) => side?.kind === 'file'
          ? attachedText(await invoke('inspect_attached_version', { id: value.id, operation, path: value.path, after: null }),
            value.id, operation, { ...side, path: value.path }) : null;
        const [before, after] = await Promise.all([read(change.before, value.base), read(change.after, value.target)]);
        updateComparison({ ...comparison, file: {
          path: value.path, before, after, beforeKind: change.before?.kind ?? 'absent', afterKind: change.after?.kind ?? 'absent',
        } });
      });
      return;
    }
    if (error || value.type !== 'control' || Object.keys(value).length !== 4
      || !['capture', 'stop', 'resume', 'detach', 'reattach'].includes(value.action)
      || !projects.some((project) => project.id === value.id && project.generation === value.generation)) return;
    void run(() => invoke('control_attached_project', { id: value.id, generation: value.generation, action: value.action }));
  }
  document.addEventListener('mesh:attachments-visible', visible);
  document.addEventListener('mesh:attachments-intent', intent);
  return () => {
    disposed = true;
    persistence.dispose();
    if (timer !== null) cancel(timer);
    document.removeEventListener('mesh:attachments-visible', visible);
    document.removeEventListener('mesh:attachments-intent', intent);
  };
}
