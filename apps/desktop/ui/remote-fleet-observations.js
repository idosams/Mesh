import { observationReply } from './remote-observation.js';
const unavailable = 'Remote observation unavailable. Last verified history is retained.';
export function remoteFleetReply(raw, scope, previous) {
  const reply = typeof raw === 'string' ? JSON.parse(raw) : raw;
  const require = value => { if (!value) throw new Error(unavailable); };
  require(reply?.schema === 'mesh.remote-fleet-observation/v1'
    && ['objective', 'lane', 'run', 'assignment', 'worker'].every(key => reply[key] === scope[key])
    && typeof reply.observation?.id === 'string' && /^[a-f0-9]{64}$/.test(reply.observation.id));
  const next = observationReply(reply.observation, reply.observation.id, 'execution');
  if (previous) {
    require(BigInt(next.observed) >= BigInt(previous.observed)
      && (!previous.admitted || next.admitted) && (!previous.launchRecorded || next.launchRecorded));
    if (previous.recorded) require(next.recorded
      && BigInt(next.recorded.revision) >= BigInt(previous.recorded.revision)
      && (next.recorded.revision !== previous.recorded.revision || next.recorded.state === previous.recorded.state));
  }
  return next;
}
// Reads never occupy the fleet command/review queue. Native code independently enforces capacity.
export function createRemoteFleetObservations({ invoke, publish, now = () => performance.now() }) {
  let rows = new Map(), active = 0, visible = false, disposed = false;
  function pump() {
    if (!visible || disposed || typeof invoke !== 'function') return;
    while (active < 4) {
      const candidates = [...rows.values()].filter(row => !row.busy && now() - row.started >= 5000);
      candidates.sort((a, b) => a.started - b.started);
      const row = candidates[0]; if (!row) return;
      row.busy = true; row.started = now(); active++; publish();
      const scope = row.scope;
      Promise.resolve().then(() => invoke('read_remote_fleet_observation', {
        objective: scope.objective, lane: scope.lane, run: scope.run, assignment: scope.assignment,
      })).then(raw => {
        if (disposed || rows.get(row.key) !== row) return;
        row.value = remoteFleetReply(raw, scope, row.value); row.error = '';
      }).catch(() => {
        if (!disposed && rows.get(row.key) === row) row.error = unavailable;
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
        if (!lane.run?.remote) continue;
        const scope = { objective: fleet.objective, lane: lane.id, run: lane.run.id,
          assignment: lane.run.remote.assignment, worker: lane.run.remote.worker };
        const key = `${scope.objective}/${scope.lane}`, old = rows.get(key);
        next.set(key, old && Object.keys(scope).every(name => old.scope[name] === scope[name]) ? old
          : { key, scope, value: null, error: '', busy: false, started: -Infinity });
      }
      rows = next; pump();
    },
    snapshot() { return Object.fromEntries([...rows].map(([key, row]) => [key, {
      run: row.scope.run, assignment: row.scope.assignment, worker: row.scope.worker,
      value: row.value, busy: row.busy, error: row.error,
    }])); },
    dispose() { disposed = true; visible = false; rows.clear(); },
  };
}
