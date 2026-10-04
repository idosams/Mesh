import { savedProgressPage, savedProgressComparison } from './fleet-progress.js';
// Each pin owns its immutable selector and request generation. Live refresh never retargets it.
export function createFleetProgress({ invoke, changed, laneFor }) {
  let queues = {}, pins = [], next = 1, disposed = false, notice = '';
  const publish = () => { if (!disposed) changed(); };
  const key = (objective, lane) => `${objective}/${lane}`;
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
      if (selected === null) { pin.input.page = result; pin.input.file = null; }
      else pin.input.file = result.changes[0];
    } catch {
      if (!disposed && pins.includes(pin) && generation === pin.generation) pin.input.error = 'The exact saved progress is unavailable. Earlier verified content remains visible.';
    } finally { if (generation === pin.generation) pin.input.loading = false; publish(); }
  }
  function handle(value) {
    if (disposed || !value || typeof value !== 'object' || typeof value.type !== 'string' || !value.type.startsWith('progress-')) return false;
    const fields = Object.keys(value).sort().join(',');
    if (value.type === 'progress-list' && fields === 'lane,objective,type') void page(value.objective, value.lane);
    if (value.type === 'progress-next' && fields === 'after,lane,objective,type') {
      const queue = queues[key(value.objective, value.lane)];
      if (queue?.page?.nextAfter === value.after && value.after) void page(value.objective, value.lane, value.after);
    }
    if (value.type === 'progress-close-list' && fields === 'lane,objective,type') { delete queues[key(value.objective, value.lane)]; publish(); }
    if (value.type === 'progress-pin' && fields === 'lane,objective,type,version') {
      const queue = queues[key(value.objective, value.lane)], record = queue?.page;
      if (!record || queue.loading || queue.error || !record.starting || !record.versions.some(row => row.version === value.version)) return true;
      if (pins.some(pin => pin.selection.objective === value.objective && pin.selection.lane === value.lane && pin.selection.version === value.version)) return true;
      if (pins.length >= 4) { notice = 'Close a progress panel before pinning another version.'; publish(); return true; }
      const pin = { key: String(next++), selection: Object.freeze({ objective: value.objective, lane: value.lane,
        version: value.version, source: record.source, starting: record.starting }), generation: 0, layout: 'inline',
        input: { loading: false, error: '', page: null, file: null }, request: { after: null, selected: null } };
      pins.push(pin); notice = ''; void read(pin);
    }
    const pin = pins.find(pin => pin.key === value.pin);
    if (!pin) return true;
    if (value.type === 'progress-close' && fields === 'pin,type') { pins = pins.filter(p => p !== pin); notice = ''; publish(); }
    if (value.type === 'progress-first' && fields === 'pin,type') void read(pin);
    if (value.type === 'progress-retry' && fields === 'pin,type') void read(pin, pin.request.after, pin.request.selected);
    if (value.type === 'progress-page' && fields === 'after,pin,type' && value.after && pin.input.page?.nextAfter === value.after) void read(pin, value.after);
    if (value.type === 'progress-file' && fields === 'object,pin,type' && pin.input.page?.changes.some(row => row.object === value.object)) void read(pin, null, value.object);
    if (value.type === 'progress-layout' && fields === 'layout,pin,type' && ['inline', 'split'].includes(value.layout)) { pin.layout = value.layout; publish(); }
    return true;
  }
  return { handle, snapshot: () => ({ progressQueues: queues, progressPins: pins, progressNotice: notice }), dispose: () => { disposed = true; queues = {}; pins = []; } };
}
