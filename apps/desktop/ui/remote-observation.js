// Native chooser selections are opaque. This controller never accepts a configuration path.
const parse = value => typeof value === 'string' ? JSON.parse(value) : value;
const hex = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const name = value => typeof value === 'string' && /^[A-Za-z0-9._-]{1,253}$/.test(value);
const decimal = value => typeof value === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(value);
const require = condition => { if (!condition) throw new Error('Remote observation could not be verified'); };
export function selectionReply(raw) {
  const value = parse(raw);
  require(value?.schema === 'mesh.remote-panel-selection/v1' && hex(value.id) && hex(value.worker)
    && ['host', 'objective', 'lane', 'run'].every(key => name(value[key])));
  return Object.freeze({ id: value.id, host: value.host, worker: value.worker, objective: value.objective, lane: value.lane, run: value.run });
}
export function observationReply(raw, id, kind) {
  const value = parse(raw);
  require(value?.schema === 'mesh.remote-panel-observation/v1' && value.id === id && value.kind === kind);
  if (kind === 'status') {
    require(decimal(value.observed_ms) && typeof value.admitted === 'boolean' && typeof value.launch_recorded === 'boolean'
      && (!value.launch_recorded || value.admitted) && (value.lease_until_ms === null || decimal(value.lease_until_ms)));
    return { kind, observed: value.observed_ms, admitted: value.admitted, launchRecorded: value.launch_recorded, leaseUntil: value.lease_until_ms };
  }
  require(kind === 'results' && typeof value.available === 'boolean' && typeof value.has_more === 'boolean'
    && Number.isSafeInteger(value.count) && value.count >= 0 && value.count <= 4096
    && (value.available ? decimal(value.revision) && value.after === 0 : value.revision === null && value.after === null && value.count === 0 && !value.has_more));
  return { kind, available: value.available, count: value.count, revision: value.revision, hasMore: value.has_more };
}
export function startRemoteObservation({ document, invoke, CustomEvent }) {
  let selection = null, status = null, results = null, busy = false, error = '', disposed = false;
  const publish = () => { if (!disposed) document.dispatchEvent(new CustomEvent('mesh:remote-observation-projection', { detail: { selection, status, results, busy, error, available: typeof invoke === 'function' } })); };
  async function intent(event) {
    if (disposed || busy || typeof invoke !== 'function') return;
    const type = event.detail?.type;
    if (!['choose', 'status', 'results', 'forget'].includes(type) || (type !== 'choose' && !selection)) return;
    busy = true; error = ''; publish();
    try {
      if (type === 'choose') {
        const raw = await invoke('pick_remote_observation');
        if (raw !== null) { selection = selectionReply(raw); status = null; results = null; }
      } else if (type === 'forget') {
        await invoke('forget_remote_observation', { id: selection.id });
        selection = null; status = null; results = null;
      } else {
        const observation = observationReply(await invoke('read_remote_observation', { id: selection.id, action: type, after: 0 }), selection.id, type);
        if (type === 'status') status = observation; else results = observation;
      }
    } catch { error = 'Remote observation is unavailable. Previous observations may be out of date. Check the signed application, private configuration and worker connection.'; }
    finally { busy = false; publish(); }
  }
  document.addEventListener('mesh:remote-observation-intent', intent);
  document.addEventListener('mesh:remote-observation-visible', publish);
  return () => { disposed = true; document.removeEventListener('mesh:remote-observation-intent', intent); document.removeEventListener('mesh:remote-observation-visible', publish); };
}
