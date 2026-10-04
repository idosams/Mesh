import { createProgressPinPersistence } from './progress-pin-persistence.js';
import { savedProgressPage, savedProgressComparison } from './fleet-progress.js';
// Each pin owns its immutable selector and request generation. Live refresh never retargets it.
export function createFleetProgress({ invoke, changed, laneFor }) {
  let queues = {}, latestBusy = {}, pins = [], next = 1n, disposed = false, notice = '';
  const publish = () => { if (!disposed) changed(); };
  const key = (objective, lane) => `${objective}/${lane}`;
  let restoreGeneration = 0;
  let enabled = false, loaded = false, controlBusy = false, restoring = false;
  let persistence = { phase: 'session', message: '' };
  const editable = () => !enabled || (loaded && !controlBusy && !restoring && persistence.phase !== 'loading');
  const persist = () => { if (enabled && !restoring) storage.changed(); };
  const storage = createProgressPinPersistence({ invoke,
    selectors: () => pins.map(pin => ({ key: pin.key, ...pin.selection, after: pin.view.after, object: pin.view.object, layout: pin.layout })),
    status: (phase, message) => { persistence = { phase, message }; publish(); },
    restore: async selectors => {
      restoring = true; restoreGeneration++;
      try {
        pins = selectors.map(row => ({ key: row.key, selection: Object.freeze({ objective: row.objective, lane: row.lane,
          version: row.version, source: row.source, starting: row.starting }), generation: 0, layout: row.layout,
          input: { loading: false, error: '', page: null, file: null }, request: { after: row.after, selected: null },
          view: { after: row.after, object: row.object } }));
        next = pins.reduce((n, pin) => BigInt(pin.key) >= n ? BigInt(pin.key) + 1n : n, 1n);
        loaded = true; publish();
        await Promise.all(pins.map(async pin => {
          const view = { ...pin.view };
          await read(pin, view.after);
          if (!disposed && pins.includes(pin) && pin.input.page && !pin.input.error && view.object !== null) await read(pin, null, view.object);
          pin.view = view;
        }));
      } finally { restoring = false; }
    },
  });
  async function control(action) {
    if (controlBusy || disposed) return;
    controlBusy = true; publish();
    try { await storage[action](); } finally { controlBusy = false; publish(); }
  }

  function addPin(record, version) {
    if (pins.some(pin => pin.selection.objective === record.objective && pin.selection.lane === record.lane && pin.selection.version === version)) return;
    if (pins.length >= 4 || next > 18446744073709551615n) { notice = 'Close a progress panel before pinning another version.'; publish(); return; }
    const pin = { key: String(next++), selection: Object.freeze({ objective: record.objective, lane: record.lane,
      version, source: record.source, starting: record.starting }), generation: 0, layout: 'inline',
      input: { loading: false, error: '', page: null, file: null }, request: { after: null, selected: null }, view: { after: null, object: null } };
    pins.push(pin); notice = ''; persist(); void read(pin);
  }
  async function pinLatest(objective, lane, version) {
    const id = key(objective, lane);
    if (latestBusy[id] || Object.keys(latestBusy).length >= 32 || laneFor(objective, lane)?.savedVersion !== version
      || typeof version !== 'string' || !/^[a-f0-9]{64}$/.test(version)) return;
    const generation = restoreGeneration;
    latestBusy[id] = true; publish();
    try {
      const record = savedProgressPage(await invoke('fleet_saved_progress', { objective, lane, after: null }), { objective, lane });
      // The user's clicked version is fixed. A later acknowledgment never retargets this request.
      if (!disposed && generation === restoreGeneration && editable() && record.starting) addPin(record, version);
    } catch { if (!disposed) notice = 'The selected saved version could not be opened.'; }
    finally { delete latestBusy[id]; publish(); }
  }
  async function page(objective, lane, after = null) {
    const id = key(objective, lane), previous = queues[id];
    if (disposed || previous?.loading || !laneFor(objective, lane)) return;
    if (!previous && Object.keys(queues).length >= 32) { notice = 'Close a progress list before opening another.'; publish(); return; }
    const queue = { objective, lane, loading: true, error: '', page: previous?.page ?? null };
    queues[id] = queue; publish();
    try {
      const result = savedProgressPage(await invoke('fleet_saved_progress', { objective, lane, after }), { objective, lane }, after);
      if (!disposed && queues[id] === queue) queue.page = result;
    } catch { if (!disposed && queues[id] === queue) queue.error = 'Saved progress is unavailable. The previous page remains visible.'; }
    finally { queue.loading = false; publish(); }
  }
  async function read(pin, after = null, selected = null) {
    const generation = ++pin.generation;
    pin.input.loading = true; pin.input.error = ''; pin.request = { after, selected }; publish();
    try {
      const result = savedProgressComparison(await invoke('inspect_fleet_saved_progress', { objective: pin.selection.objective,
        lane: pin.selection.lane, version: pin.selection.version, after, selected }), pin.selection, after, selected, pin.input.page);
      if (disposed || !pins.includes(pin) || generation !== pin.generation) return;
      if (selected === null) { pin.input.page = result; pin.input.file = null; if (!restoring) pin.view = { after, object: null }; }
      else { pin.input.file = result.changes[0]; if (!restoring) pin.view.object = selected; }
      persist();
    } catch {
      if (!disposed && pins.includes(pin) && generation === pin.generation) pin.input.error = 'The exact saved progress is unavailable. Earlier verified content remains visible.';
    } finally { if (generation === pin.generation) pin.input.loading = false; publish(); }
  }
  function handle(value) {
    if (disposed || !value || typeof value !== 'object' || typeof value.type !== 'string' || !value.type.startsWith('progress-')) return false;
    const fields = Object.keys(value).sort().join(',');
    if (enabled && fields === 'type' && value.type === 'progress-retry-save') { void control('retry'); return true; }
    if (enabled && fields === 'type' && value.type === 'progress-reload-saved') { void control('reload'); return true; }
    if (!editable()) return true;
    if (value.type === 'progress-pin-latest' && fields === 'lane,objective,type,version') void pinLatest(value.objective, value.lane, value.version);
    if (value.type === 'progress-list' && fields === 'lane,objective,type') void page(value.objective, value.lane);
    if (value.type === 'progress-next' && fields === 'after,lane,objective,type') {
      const queue = queues[key(value.objective, value.lane)];
      if (queue?.page?.nextAfter === value.after && value.after) void page(value.objective, value.lane, value.after);
    }
    if (value.type === 'progress-close-list' && fields === 'lane,objective,type') { delete queues[key(value.objective, value.lane)]; publish(); }
    if (value.type === 'progress-pin' && fields === 'lane,objective,type,version') {
      const queue = queues[key(value.objective, value.lane)], record = queue?.page;
      if (!record || queue.loading || queue.error || !record.starting || !record.versions.some(row => row.version === value.version)) return true;
      addPin(record, value.version);
    }
    const pin = pins.find(pin => pin.key === value.pin);
    if (!pin) return true;
    if (value.type === 'progress-close' && fields === 'pin,type') { pins = pins.filter(p => p !== pin); notice = ''; persist(); publish(); }
    if (value.type === 'progress-first' && fields === 'pin,type') void read(pin);
    if (value.type === 'progress-retry' && fields === 'pin,type') void read(pin, pin.request.after, pin.request.selected);
    if (value.type === 'progress-page' && fields === 'after,pin,type' && value.after && pin.input.page?.nextAfter === value.after) void read(pin, value.after);
    if (value.type === 'progress-file' && fields === 'object,pin,type' && pin.input.page?.changes.some(row => row.object === value.object)) void read(pin, null, value.object);
    if (value.type === 'progress-layout' && fields === 'layout,pin,type' && ['inline', 'split'].includes(value.layout)) { pin.layout = value.layout; persist(); publish(); }
    return true;
  }
  return { handle, loadSaved() { enabled = true; return storage.ensureLoaded(); }, snapshot: () => ({ progressLatestBusy: latestBusy, progressQueues: queues, progressPins: pins, progressNotice: notice, progressPersistence: { ...persistence, editable: editable(), busy: controlBusy } }), dispose: () => { disposed = true; storage.dispose(); queues = {}; latestBusy = {}; pins = []; } };
}
