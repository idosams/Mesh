import { reviewChangeMessage, reviewChangeReceipt, reviewDecisionReceipt, savedReviewChangeActivity } from './fleet-review-changes.js';
import { loadFleetArtifact } from './fleet-artifact-preview.js';
import { reviewArtifactKind } from './review-artifact-validation.js';
import { createFleetPinPersistence, defaultFleetView } from './fleet-pin-persistence.js';
import { fleetInputComparison } from './fleet-input-comparison.js';
// Immutable saved-result selection and independent presentation requests. No publication authority.
const identity = value => typeof value === 'string' && /^[A-Za-z0-9_.:-]{1,128}$/.test(value);
const digest = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const objectiveId = value => typeof value === 'string' && /^fleet-[a-f0-9]{64}$/.test(value);
const count = (value, max = Number.MAX_SAFE_INTEGER) => Number.isSafeInteger(value) && value >= 0 && value <= max;
const check = valid => { if (!valid) throw new Error('Saved fleet review could not be verified'); };
const parse = raw => { check(typeof raw !== 'string' || raw.length <= 40 * 1024 * 1024); return typeof raw === 'string' ? JSON.parse(raw) : raw; };
const key = (objective, lane) => `${objective}/${lane}`;
const selectorFields = ['objective', 'lane', 'checkpoint', 'version', 'bundle'];
const same = (left, right) => selectorFields.every(field => left[field] === right[field]);

export function savedFleetReviewPage(raw, objective, lane, after) {
  const value = parse(raw);
  check(objectiveId(objective) && identity(lane) && (after === null || identity(after))
    && value?.schema === 'mesh.fleet-saved-reviews/v1' && value.objective === objective && value.lane === lane
    && value.after === after && value.order === 'checkpoint-id' && count(value.revision) && count(value.total, 4096)
    && Array.isArray(value.reviews) && value.reviews.length <= 50 && value.total >= value.reviews.length);
  let previous = after;
  const rows = value.reviews.map(row => {
    check(identity(row?.checkpoint) && digest(row.version) && digest(row.bundle) && identity(row.run)
      && (previous === null || row.checkpoint > previous));
    previous = row.checkpoint;
    return { objective, lane, checkpoint: row.checkpoint, version: row.version, bundle: row.bundle, run: row.run };
  });
  check(value.next_after === null || (rows.length === 50 && value.next_after === rows.at(-1).checkpoint && value.total > 50));
  check(after !== null || value.next_after !== null || value.total === rows.length);
  return { objective, lane, after, rows, total: value.total, nextAfter: value.next_after, revision: value.revision };
}

export function savedFleetReview(raw, selection) {
  const value = parse(raw), item = value?.review;
  check(objectiveId(selection.objective) && identity(selection.lane) && identity(selection.checkpoint)
    && digest(selection.version) && digest(selection.bundle));
  check(value?.schema === 'mesh.fleet-saved-review/v1' && value.objective === selection.objective
    && value.selection && Object.keys(value.selection).sort().join(',') === 'bundle,checkpoint,lane,version'
    && ['lane', 'checkpoint', 'version', 'bundle'].every(field => value.selection[field] === selection[field])
    && item?.bundle === selection.bundle && item.subject_operation === selection.version && item.recorded === true
    && item.projection_authorizes_approval === false && typeof item.content_complete === 'boolean'
    && count(item.bundle_changes_not_listed) && count(item.subject_operations_not_listed)
    && (item.unavailable_code === null || identity(item.unavailable_code))
    && (item.reviewed_head === null || digest(item.reviewed_head))
    && (item.presentation_digest === null || digest(item.presentation_digest))
    && Array.isArray(item.bundle_changes) && item.bundle_changes.length <= 64);
  check(item.content_complete === (item.unavailable_code === null && item.bundle_changes_not_listed === 0 && item.subject_operations_not_listed === 0));
  if (item.content_complete) check(digest(item.reviewed_head) && digest(item.presentation_digest));
  // The existing React review adapter verifies the bounded object/content identities and text hunks.
  // Unused operation bodies and actor metadata are not retained by this presentation coordinator.
  return { bundle: item.bundle, subject_operation: item.subject_operation, recorded: true,
    reviewed_head: item.reviewed_head, presentation_digest: item.presentation_digest,
    content_complete: item.content_complete, bundle_changes_not_listed: item.bundle_changes_not_listed,
    subject_operations_not_listed: item.subject_operations_not_listed, unavailable_code: item.unavailable_code,
    projection_authorizes_approval: false, bundle_changes: item.bundle_changes };
}

export function createFleetReviews({ invoke, laneFor, changed, requestId = () => globalThis.crypto.randomUUID().replaceAll('-', '') }) {
  let queues = {}, pins = [], nextPin = 1n, nextRead = 1, disposed = false, notice = '';
  let persistenceEnabled = false, controlBusy = false, editable = true, persistenceState = { phase: 'session', message: '' };
  const publish = () => { if (!disposed) changed(); };
  const persist = () => { if (persistenceEnabled) storage.changed(); };
  const storage = createFleetPinPersistence({ invoke,
    selectors: () => pins.map(pin => ({ key: pin.key, ...pin.selection, source_version: pin.startingInput, ...pin.view })),
    restore(saved) {
      pins = saved.map(value => ({ key: value.key, selection: Object.fromEntries(selectorFields.map(field => [field, value[field]])),
        startingInput: value.source_version, goal: laneFor(value.objective, value.lane)?.goal ?? null,
        view: Object.fromEntries(Object.keys(defaultFleetView()).map(field => [field, value[field]])), review: null, loading: false, error: '' }));
      nextPin = pins.reduce((max, pin) => BigInt(pin.key) >= max ? BigInt(pin.key) + 1n : max, nextPin);
      notice = ''; publish();
      for (const pin of pins) {
        void loadPin(pin);
        if (pin.view.input_open) void restoreInput(pin);
      }
    },
    status(phase, message) {
      if (phase === 'loading') editable = false;
      if (phase === 'saved') editable = true;
      persistenceState = { phase, message }; publish();
    },
  });
  async function controlStorage(action) {
    if (controlBusy) return;
    controlBusy = true; publish();
    try { await action(); } finally { controlBusy = false; publish(); }
  }
  function remember(pin, patch) {
    const updated = { ...pin, view: { ...pin.view, ...patch } };
    pins = pins.map(value => value.key === pin.key ? updated : value); persist(); publish(); return updated;
  }
  async function restoreInput(pin) {
    await loadInput(pin, pin.view.input_after);
    const current = pins.find(value => value.key === pin.key && value.selection === pin.selection);
    if (!disposed && current && !current.input?.error && pin.view.input_object) await loadInput(current, null, pin.view.input_object);
  }
  function requestInput(pin, after = null, selected = null) {
    const updated = remember(pin, { input_open: true, ...(selected === null ? { input_after: after } : { input_object: selected }) });
    void loadInput(updated, after, selected);
  }
  async function loadPage(objective, lane, after) {
    const id = key(objective, lane), existing = queues[id];
    if (existing?.loading) return;
    if (!existing && Object.keys(queues).length >= 32) { notice = 'Close a saved-result list before opening another.'; publish(); return; }
    const loading = { ...(existing ?? { page: null }), objective, lane, loading: true, error: '' };
    queues = { ...queues, [id]: loading }; notice = ''; publish();
    try {
      const page = savedFleetReviewPage(await invoke('fleet_saved_reviews', { objective, lane, after }), objective, lane, after);
      if (!disposed && queues[id] === loading) queues = { ...queues, [id]: { objective, lane, page, loading: false, error: '' } };
    } catch {
      if (!disposed && queues[id] === loading) queues = { ...queues, [id]: { ...loading, loading: false, error: 'Saved results could not be loaded. The previous page is retained.' } };
    }
    publish();
  }
  async function loadPin(pin) {
    if (disposed) return;
    const loading = { ...pin, loading: true, error: '', reviewRequest: nextRead++ };
    pins = pins.map(value => value === pin ? loading : value); publish();
    try {
      const review = savedFleetReview(await invoke('inspect_fleet_saved_review', loading.selection), loading.selection);
      if (!disposed) pins = pins.map(value => value.key === loading.key && value.reviewRequest === loading.reviewRequest ? { ...value, loading: false, review } : value);
    } catch {
      if (!disposed) pins = pins.map(value => value.key === loading.key && value.reviewRequest === loading.reviewRequest ? { ...value, loading: false, error: 'This exact saved result could not be verified. Its selection is retained.' } : value);
    }
    publish();
  }
  function pinResult(row, lane) {
    if (pins.some(pin => same(pin.selection, row))) { notice = 'This exact result is already pinned.'; publish(); return; }
    if (pins.length >= 8) { notice = 'Close a review panel before opening another. Eight can stay pinned together.'; publish(); return; }
    const selection = Object.fromEntries(selectorFields.map(field => [field, row[field]]));
    if (nextPin > 18446744073709551615n) { notice = 'Review display keys are exhausted.'; publish(); return; }
    const pin = { view: defaultFleetView(), key: String(nextPin++), selection, goal: lane.goal, startingInput: lane.base, review: null, loading: false, error: '' };
    pins = [...pins, pin]; notice = ''; persist(); void loadPin(pin);
  }
  async function loadChanges(pin, reloadDecision = false) {
    if (pin.feedback?.loading || pin.feedback?.sending || pin.feedback?.deciding) return;
    const feedback = { ...pin.feedback, rows: pin.feedback?.rows ?? null, loading: true, error: '' };
    pins = pins.map(value => value === pin ? { ...value, feedback } : value); publish();
    try {
      const activity = savedReviewChangeActivity(await invoke('fleet_review_changes', pin.selection), pin.selection);
      if (!disposed) pins = pins.map(value => value.selection === pin.selection && value.feedback === feedback
        ? { ...value, feedback: { ...feedback, ...activity, loaded: true, loading: false, ...(reloadDecision ? { decisionPending: null, decisionError: '', decisionNotice: 'Latest request decisions loaded.' } : {}) } } : value);
    } catch {
      if (!disposed) pins = pins.map(value => value.selection === pin.selection && value.feedback === feedback
        ? { ...value, feedback: { ...feedback, loading: false, error: 'Saved change requests could not be verified. Retry reading the same review.' } } : value);
    }
    publish();
  }
  async function decideChanges(pin, request, checkpoint, retry = false) {
    if (pin.feedback?.loading || pin.feedback?.sending || pin.feedback?.deciding) return;
    let pending = pin.feedback?.decisionPending;
    if (retry ? !pending : pending || !pin.feedback?.loaded || pin.feedback.error) return;
    if (!retry) {
      const current = pin.feedback.decisions?.find(value => value.request === request);
      const proposed = checkpoint === null ? null : pin.feedback.responses?.find(value => value.request === request && value.checkpoint === checkpoint);
      if (!current || current.revision >= 64 || current.checkpoint === checkpoint || (checkpoint !== null && !proposed)) return;
      pending = { operation: requestId(), request, expectedRevision: current.revision, proposedCheckpoint: checkpoint,
        version: proposed?.version ?? null, bundle: proposed?.bundle ?? null };
    }
    if (typeof pending.operation !== 'string' || !/^[a-f0-9]{32}$/.test(pending.operation)) return;
    const feedback = { ...pin.feedback, decisionPending: pending, deciding: true, decisionError: '', decisionNotice: '' };
    pins = pins.map(value => value === pin ? { ...value, feedback } : value); publish();
    try {
      const { version: _version, bundle: _bundle, ...args } = pending;
      const outcome = reviewDecisionReceipt(await invoke('decide_fleet_review_change', { ...pin.selection, ...args }), pin.selection, pending);
      if (!disposed) pins = pins.map(value => value.selection === pin.selection && value.feedback === feedback
        ? { ...value, feedback: { ...feedback, deciding: false, decisionPending: null,
          decisions: [...(feedback.decisions ?? []).filter(value => value.request !== pending.request), outcome.current],
          decisionNotice: outcome.cancelled ? 'Confirmation cancelled. The current recorded decision is shown.' : 'Decision receipt verified. The current recorded decision is shown.' } } : value);
    } catch {
      if (!disposed) pins = pins.map(value => value.selection === pin.selection && value.feedback === feedback
        ? { ...value, feedback: { ...feedback, deciding: false, decisionError: 'Decision is unconfirmed. Retry the same operation or read the latest state before choosing again.' } } : value);
    }
    publish();
  }
  async function requestChanges(pin, message, retry) {
    if (pin.feedback?.loading || pin.feedback?.sending || pin.feedback?.deciding || !pin.review?.content_complete) return;
    if (retry ? !pin.feedback?.pending : pin.feedback?.pending || !reviewChangeMessage(message)) return;
    const pending = retry ? pin.feedback.pending : { request: requestId(), message };
    if (typeof pending.request !== 'string' || !/^[a-f0-9]{32}$/.test(pending.request)) return;
    const feedback = { ...pin.feedback, rows: pin.feedback?.rows ?? null, pending, sending: true, error: '' };
    pins = pins.map(value => value === pin ? { ...value, feedback } : value); publish();
    try {
      const row = reviewChangeReceipt(await invoke('request_fleet_review_changes', { ...pin.selection, ...pending }), pin.selection, pending);
      if (!disposed) pins = pins.map(value => value.selection === pin.selection && value.feedback === feedback
        ? { ...value, feedback: { ...feedback, pending: null, sending: false, rows: [...(feedback.rows ?? []).filter(previous => previous.id !== row.id), row] } } : value);
    } catch {
      if (!disposed) pins = pins.map(value => value.selection === pin.selection && value.feedback === feedback
        ? { ...value, feedback: { ...feedback, sending: false, error: 'Recording is unconfirmed. Retry the same request to recover its receipt. Restored fleets require recovery before recording new requests.' } } : value);
    }
    publish();
  }
  async function loadArtifact(pin, object, page) {
    if (disposed || pin.artifact?.loading || pin.loading || pin.error || !pin.review?.content_complete) return;
    const change = pin.review.bundle_changes.find(change => change.object_id === object);
    const kind = change && reviewArtifactKind(change);
    if (!kind || !Number.isSafeInteger(page) || page < 1 || page > 64 || (kind !== 'pdf' && page !== 1)) return;
    const loading = { generation: nextRead++, object, page, loading: true, error: '', envelope: null };
    pins = pins.map(value => value === pin ? { ...value, artifact: loading } : value); publish();
    try {
      const envelope = await loadFleetArtifact(invoke, pin, change, page, loading.generation);
      if (!disposed) pins = pins.map(value => value.selection === pin.selection && value.artifact === loading
        ? { ...value, artifact: { ...loading, loading: false, envelope } } : value);
    } catch {
      if (!disposed) pins = pins.map(value => value.selection === pin.selection && value.artifact === loading
        ? { ...value, artifact: { ...loading, loading: false, error: 'This exact saved artifact preview could not be verified. Retry to render it again.' } } : value);
    }
    publish();
  }
  async function loadInput(pin, after = null, selected = null) {
    if (disposed) return;
    if (pin.input?.loading) return;
    const loading = { ...pin.input, page: pin.input?.page ?? null, file: pin.input?.file ?? null, loading: true, error: '', requestedAfter: after, requestedObject: selected };
    pins = pins.map(value => value.key === pin.key ? { ...value, input: loading } : value); publish();
    try {
      const comparison = fleetInputComparison(await invoke('inspect_fleet_starting_comparison', { ...pin.selection, after, selected }), pin, after, selected);
      if (!disposed) pins = pins.map(value => value.key === pin.key && value.input === loading
        ? { ...value, input: { ...loading, loading: false, ...(selected === null ? { page: comparison } : { file: comparison.changes[0] }) } } : value);
    } catch {
      if (!disposed) pins = pins.map(value => value.key === pin.key && value.input === loading
        ? { ...value, input: { ...loading, loading: false, error: 'This starting-version comparison could not be verified. Any displayed content is the previously verified result.' } } : value);
    }
    publish();
  }
  return {
    snapshot: () => ({ reviewQueues: queues, reviewPins: pins, reviewNotice: notice, reviewPersistence: { ...persistenceState, editable, busy: controlBusy } }),
    loadSaved() { if (!disposed && typeof invoke === 'function') { persistenceEnabled = true; return storage.ensureLoaded(); } },
    dispose: () => { disposed = true; storage.dispose(); },
    handle(value) {
      if (disposed || typeof invoke !== 'function') return false;
      if (!['reviews', 'reviews-page', 'close-reviews', 'pin-review', 'close-review', 'retry-review', 'input-review', 'input-page', 'input-file', 'retry-input', 'review-view', 'input-layout', 'retry-saved-reviews', 'reload-saved-reviews', 'artifact-preview', 'review-changes', 'request-review-changes', 'retry-review-changes', 'pin-review-response', 'decide-review-change', 'retry-review-decision', 'reload-review-decision'].includes(value.type)) return false;
      const fields = Object.keys(value).sort().join(',');
      if (fields === 'type' && value.type === 'retry-saved-reviews') { if (persistenceState.phase === 'error') void controlStorage(() => storage.retry()); return true; }
      if (fields === 'type' && value.type === 'reload-saved-reviews') { if (!controlBusy && ['saved', 'error'].includes(persistenceState.phase)) { editable = false; persistenceState = { phase: 'loading', message: '' }; publish(); void controlStorage(() => storage.reload()); } return true; }
      if (!editable && !['reviews', 'reviews-page', 'close-reviews', 'retry-review', 'retry-input'].includes(value.type)) {
        notice = 'Load the saved review set before changing its selections.'; publish(); return true;
      }
      if (['retry-review-decision', 'reload-review-decision'].includes(value.type) && fields === 'pin,type') {
        const pin = pins.find(pin => pin.key === value.pin);
        if (pin) { if (value.type === 'reload-review-decision') void loadChanges(pin, true); else void decideChanges(pin, null, null, true); }
        return true;
      }
      if (value.type === 'decide-review-change' && fields === 'checkpoint,pin,request,type' && typeof value.checkpoint === 'string') {
        const pin = pins.find(pin => pin.key === value.pin); if (pin) void decideChanges(pin, value.request, value.checkpoint || null);
        return true;
      }
      if (value.type === 'pin-review-response' && fields === 'checkpoint,pin,request,type') {
        const source = pins.find(pin => pin.key === value.pin);
        const response = source?.feedback?.responses?.find(response => response.request === value.request && response.checkpoint === value.checkpoint);
        if (source && response && !source.feedback.loading && !source.feedback.error) {
          pinResult({ ...source.selection, checkpoint: response.checkpoint, version: response.version, bundle: response.bundle },
            { goal: source.goal, base: source.startingInput });
        }
        return true;
      }
      if (['review-changes', 'retry-review-changes'].includes(value.type) && fields === 'pin,type') {
        const pin = pins.find(pin => pin.key === value.pin);
        if (pin) { if (value.type === 'review-changes') void loadChanges(pin); else void requestChanges(pin, null, true); }
        return true;
      }
      if (value.type === 'request-review-changes' && fields === 'message,pin,type') {
        const pin = pins.find(pin => pin.key === value.pin); if (pin) void requestChanges(pin, value.message, false);
        return true;
      }
      if (value.type === 'artifact-preview' && fields === 'object,page,pin,type' && typeof value.page === 'string' && /^[1-9][0-9]?$/.test(value.page)) {
        const pin = pins.find(pin => pin.key === value.pin);
        if (pin) void loadArtifact(pin, value.object, Number(value.page));
        return true;
      }
      if (value.type === 'input-layout' && fields === 'layout,pin,type' && ['inline', 'split'].includes(value.layout)) {
        const pin = pins.find(pin => pin.key === value.pin); if (pin) remember(pin, { input_layout: value.layout }); return true;
      }
      if (value.type === 'review-view' && fields === 'layout,mode,object,pin,type' && ['inline', 'split'].includes(value.layout) && ['visual', 'content'].includes(value.mode)) {
        const pin = pins.find(pin => pin.key === value.pin);
        if (pin?.review?.bundle_changes.some(change => change.object_id === value.object)) remember(pin, { review_object: value.object, review_mode: value.mode, review_layout: value.layout });
        return true;
      }
      if (['close-review', 'retry-review'].includes(value.type) && fields === 'pin,type') {
        const pin = pins.find(pin => pin.key === value.pin);
        if (!pin) return true;
        if (value.type === 'close-review') { pins = pins.filter(value => value !== pin); notice = ''; persist(); publish(); }
        else if (!pin.loading) void loadPin(pin);
        return true;
      }
      if (['input-review', 'input-page', 'input-file', 'retry-input'].includes(value.type)) {
        const pin = pins.find(pin => pin.key === value.pin);
        if (!pin || pin.input?.loading) return true;
        if (value.type === 'input-review' && fields === 'pin,type') requestInput(pin);
        if (value.type === 'retry-input' && fields === 'pin,type' && pin.input?.error) { if (pin.input.requestedObject === null && pin.view.input_object && !pin.input.file) void restoreInput(pin); else void loadInput(pin, pin.input.requestedAfter, pin.input.requestedObject); }
        if (value.type === 'input-page' && fields === 'after,pin,type' && typeof value.after === 'string' && value.after === pin.input?.page?.nextAfter) requestInput(pin, value.after);
        if (value.type === 'input-file' && fields === 'object,pin,type' && pin.input?.page?.changes.some(change => change.object === value.object)) requestInput(pin, null, value.object);
        return true;
      }
      const id = key(value.objective, value.lane);
      if (value.type === 'close-reviews' && fields === 'lane,objective,type') { queues = { ...queues }; delete queues[id]; publish(); return true; }
      const lane = laneFor(value.objective, value.lane);
      if (!lane) return true;
      if (value.type === 'reviews' && fields === 'lane,objective,type') { void loadPage(value.objective, value.lane, null); return true; }
      const queue = queues[id];
      if (value.type === 'reviews-page' && fields === 'after,lane,objective,type' && identity(value.after) && queue?.page?.nextAfter !== null && queue?.page?.nextAfter === value.after && !queue.error) {
        void loadPage(value.objective, value.lane, value.after); return true;
      }
      if (value.type === 'pin-review' && fields === 'bundle,checkpoint,lane,objective,type,version' && queue?.page && !queue.loading && !queue.error) {
        const row = queue.page.rows.find(row => same(row, value));
        if (!row) return true;
        pinResult(row, lane);
      }
      return true;
    },
  };
}
