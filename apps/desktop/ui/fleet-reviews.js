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

export function createFleetReviews({ invoke, laneFor, changed }) {
  let queues = {}, pins = [], nextPin = 1, disposed = false, notice = '';
  const publish = () => { if (!disposed) changed(); };
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
    const loading = { ...pin, loading: true, error: '' };
    pins = pins.map(value => value === pin ? loading : value); publish();
    try {
      const review = savedFleetReview(await invoke('inspect_fleet_saved_review', loading.selection), loading.selection);
      if (!disposed) pins = pins.map(value => value === loading ? { ...loading, loading: false, review } : value);
    } catch {
      if (!disposed) pins = pins.map(value => value === loading ? { ...loading, loading: false, error: 'This exact saved result could not be verified. Its selection is retained.' } : value);
    }
    publish();
  }
  return {
    snapshot: () => ({ reviewQueues: queues, reviewPins: pins, reviewNotice: notice }),
    dispose: () => { disposed = true; },
    handle(value) {
      if (disposed || typeof invoke !== 'function') return false;
      if (!['reviews', 'reviews-page', 'close-reviews', 'pin-review', 'close-review', 'retry-review'].includes(value.type)) return false;
      const fields = Object.keys(value).sort().join(',');
      if (['close-review', 'retry-review'].includes(value.type) && fields === 'pin,type') {
        const pin = pins.find(pin => pin.key === value.pin);
        if (!pin) return true;
        if (value.type === 'close-review') { pins = pins.filter(value => value !== pin); notice = ''; publish(); }
        else if (!pin.loading) void loadPin(pin);
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
        if (pins.some(pin => same(pin.selection, row))) { notice = 'This exact result is already pinned.'; publish(); return true; }
        if (pins.length >= 8) { notice = 'Close a review panel before opening another. Eight can stay pinned together.'; publish(); return true; }
        const selection = Object.fromEntries(selectorFields.map(field => [field, row[field]]));
        const pin = { key: String(nextPin++), selection, goal: lane.goal, startingInput: lane.base, review: null, loading: false, error: '' };
        pins = [...pins, pin]; notice = ''; void loadPin(pin);
      }
      return true;
    },
  };
}
