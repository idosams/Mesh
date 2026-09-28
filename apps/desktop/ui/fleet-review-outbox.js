import { reviewChangeMessage } from './fleet-review-changes.js';
const check = value => { if (!value) throw new Error('Pending review inputs could not be verified'); };
const keys = (value, names) => value && typeof value === 'object' && !Array.isArray(value) && Object.keys(value).sort().join(',') === names;
const hex = (value, size) => typeof value === 'string' && new RegExp(`^[a-f0-9]{${size}}$`).test(value);
const id = value => typeof value === 'string' && /^[A-Za-z0-9_.:-]{1,128}$/.test(value);
const equal = (a, b) => JSON.stringify(a) === JSON.stringify(b);
export const operationToken = entry => entry.kind === 'change' ? entry.input.request : entry.input.operation;
export function reviewOutbox(raw) {
  check(typeof raw !== 'string' || new TextEncoder().encode(raw).length <= 131072);
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  check(keys(value, 'entries,revision,schema') && value.schema === 'mesh.fleet-review-outbox/v1'
    && typeof value.revision === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(value.revision) && BigInt(value.revision) <= 18446744073709551615n
    && Array.isArray(value.entries) && value.entries.length <= 8);
  const seen = new Set();
  const entries = value.entries.map(entry => {
    check(keys(entry, 'input,kind,objective,selection') && typeof entry.objective === 'string' && /^fleet-[a-f0-9]{64}$/.test(entry.objective)
      && keys(entry.selection, 'bundle,checkpoint,lane,version') && id(entry.selection.lane) && id(entry.selection.checkpoint)
      && hex(entry.selection.version, 64) && hex(entry.selection.bundle, 64));
    const input = entry.input;
    if (entry.kind === 'change') check(keys(input, 'message,request') && reviewChangeMessage(input.message) && hex(input.request, 32));
    else {
      check(entry.kind === 'decision' && keys(input, 'bundle,checkpoint,expected_revision,operation,request,version')
        && hex(input.operation, 32) && typeof input.request === 'string' && /^review-change-[a-f0-9]{64}$/.test(input.request)
        && typeof input.expected_revision === 'string' && /^(0|[1-9][0-9]?)$/.test(input.expected_revision) && Number(input.expected_revision) < 64
        && (input.checkpoint === null ? input.version === null && input.bundle === null : id(input.checkpoint) && hex(input.version, 64) && hex(input.bundle, 64)));
    }
    const token = operationToken(entry); check(!seen.has(token)); seen.add(token);
    return { kind: entry.kind, objective: entry.objective, selection: { lane: entry.selection.lane, checkpoint: entry.selection.checkpoint, version: entry.selection.version, bundle: entry.selection.bundle },
      input: entry.kind === 'change' ? { request: input.request, message: input.message } : { operation: input.operation, request: input.request, expected_revision: input.expected_revision, checkpoint: input.checkpoint, version: input.version, bundle: input.bundle } };
  });
  return { schema: value.schema, revision: value.revision, entries };
}
export function pendingReviewEntry(selection, kind, pending) {
  const { objective, ...selected } = selection;
  const entry = { kind, objective, selection: selected, input: kind === 'change' ? pending : {
    operation: pending.operation, request: pending.request, expected_revision: String(pending.expectedRevision), checkpoint: pending.proposedCheckpoint, version: pending.version, bundle: pending.bundle,
  } };
  return reviewOutbox({ schema: 'mesh.fleet-review-outbox/v1', revision: '0', entries: [entry] }).entries[0];
}
export const pendingDecision = input => ({ operation: input.operation, request: input.request, expectedRevision: Number(input.expected_revision), proposedCheckpoint: input.checkpoint, version: input.version, bundle: input.bundle });
export function createReviewOutbox({ invoke, changed }) {
  let state = { schema: 'mesh.fleet-review-outbox/v1', revision: '0', entries: [] }, busy = false, loaded = false, error = '', disposed = false, active = Promise.resolve();
  const publish = () => { if (!disposed) changed(); };
  const read = async () => { const next = reviewOutbox(await invoke('load_fleet_review_outbox')); if (disposed) throw new Error('Closed'); state = next; loaded = true; return next; };
  const serial = action => {
    const result = active.catch(() => {}).then(async () => {
      if (disposed) throw new Error('Closed'); busy = true; error = ''; publish();
      try { return await action(); } catch (failure) { error = 'Pending review inputs could not be confirmed. Refresh or retry the exact operation.'; throw failure; }
      finally { busy = false; publish(); }
    });
    active = result; return result;
  };
  async function replace(previous, entries) {
    const request = { ...previous, entries };
    const result = reviewOutbox(await invoke('save_fleet_review_outbox', { snapshot: JSON.stringify(request) }));
    check(equal(result.entries, entries) && ((result.revision === previous.revision && equal(entries, previous.entries)) || BigInt(result.revision) === BigInt(previous.revision) + 1n));
    if (disposed) throw new Error('Closed'); state = result;
  }
  return {
    snapshot: () => ({ entries: state.entries, busy, loaded, error }),
    load: () => serial(read),
    retain: entry => serial(async () => {
      const valid = reviewOutbox({ ...state, entries: [entry] }).entries[0];
      const current = await read(), previous = current.entries.find(item => operationToken(item) === operationToken(valid));
      if (previous) { check(equal(previous, valid)); return; }
      check(!current.entries.some(item => item.kind === valid.kind && item.objective === valid.objective && equal(item.selection, valid.selection)));
      check(current.entries.length < 8); await replace(current, [...current.entries, valid]);
    }),
    release: entry => serial(async () => {
      const current = await read(), previous = current.entries.find(item => operationToken(item) === operationToken(entry));
      if (!previous) return; check(equal(previous, entry));
      await replace(current, current.entries.filter(item => item !== previous));
    }),
    dispose() { disposed = true; },
  };
}
