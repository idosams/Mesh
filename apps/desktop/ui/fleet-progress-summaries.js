import { savedProgressSummary } from './fleet-progress.js';
const unavailable = 'Saved change summary is unavailable. Earlier verified counts are retained.';
const skipped = Symbol('not-dispatched');
const limit = 16 * 1024; // Same maximum as the validated fleet catalogue, not a first-page subset.
const hash = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);

// Immutable summary reads never occupy the fleet command or pinned-review queues.
export function createFleetProgressSummaries({ invoke, publish, now = () => performance.now() }) {
  let rows = new Map(), active = 0, sequence = 0, visible = false, disposed = false;
  function pump() {
    if (!visible || disposed || typeof invoke !== 'function') return;
    while (active < 2) {
      let next = null;
      for (const row of rows.values()) {
        if (row.busy || row.value?.version === row.version
          || (row.attempt === row.version && now() - row.started < 5000)) continue;
        if (!next || row.turn < next.turn) next = row;
      }
      if (!next) return;
      const row = next, selection = { ...row.scope, version: row.version };
      row.busy = true; row.attempt = selection.version; row.started = now(); row.turn = ++sequence;
      row.error = ''; active++; publish();
      Promise.resolve().then(() => {
        if (disposed || !visible || rows.get(row.key) !== row || row.version !== selection.version) return skipped;
        return invoke('summarize_fleet_saved_progress', {
          objective: selection.objective, lane: selection.lane, version: selection.version,
        });
      }).then(raw => {
        if (raw === skipped) { row.attempt = null; return; }
        if (disposed || rows.get(row.key) !== row || row.version !== selection.version) return;
        row.value = Object.freeze(savedProgressSummary(raw, selection)); row.error = '';
      }).catch(() => {
        if (!disposed && rows.get(row.key) === row && row.version === selection.version) { row.error = unavailable; row.started = now(); }
      }).finally(() => {
        active--; row.busy = false;
        if (!disposed) { publish(); pump(); }
      });
    }
  }
  return {
    sync(fleets, shown) {
      if (disposed) return;
      visible = shown;
      const next = new Map();
      for (const fleet of fleets) for (const lane of fleet.lanes) {
        if (fleet.ownership === 'unavailable' || !lane.allocated || lane.run?.remote
          || !hash(lane.savedVersion) || !hash(lane.base) || next.size >= limit) continue;
        const key = `${fleet.objective}/${lane.id}`, old = rows.get(key);
        const scope = { objective: fleet.objective, lane: lane.id, source: lane.base };
        const row = old?.scope.source === scope.source ? old : {
          key, scope, version: lane.savedVersion, value: null, error: '', busy: false,
          attempt: null, started: -Infinity, turn: ++sequence,
        };
        if (row.version !== lane.savedVersion) { row.version = lane.savedVersion; row.error = ''; }
        next.set(key, row);
      }
      rows = next; pump();
    },
    snapshot() { return Object.fromEntries([...rows].map(([key, row]) => [key, {
      source: row.scope.source, version: row.version, value: row.value, busy: row.busy,
      pending: row.value?.version !== row.version && !row.error, error: row.error,
    }])); },
    dispose() { disposed = true; visible = false; rows.clear(); },
  };
}
