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
export function draftReply(raw) {
  const value = parse(raw);
  require(value?.schema === 'mesh.remote-setup-draft/v1' && hex(value.id) && ['installation', 'identity', 'hosts'].every(key => typeof value[key] === 'boolean'));
  return Object.freeze({ id: value.id, installation: value.installation, identity: value.identity, hosts: value.hosts });
}
export function setupInput(value) {
  require(value && ['host', 'account', 'objective', 'lane', 'run'].every(key => name(value[key])) && value.account.length <= 64 && hex(value.worker)
    && /^(0|[1-9][0-9]{0,4})$/.test(value.port) && Number(value.port) >= 1 && Number(value.port) <= 65535);
  return { host: value.host, account: value.account, port: Number(value.port), worker: value.worker, objective: value.objective, lane: value.lane, run: value.run };
}
const labelText = value => typeof value === 'string' && value.trim().length > 0 && new TextEncoder().encode(value).length <= 128 && !/[\x00-\x1f\x7f-\x9f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(value);
export function profilesReply(raw) {
  const value = parse(raw);
  require(value?.schema === 'mesh.remote-connection-profiles/v1' && decimal(value.revision)
    && BigInt(value.revision) <= 18446744073709551615n && Array.isArray(value.entries) && value.entries.length <= 16);
  const entries = value.entries.map(entry => {
    require(hex(entry?.id) && labelText(entry.label) && hex(entry.worker) && ['host', 'objective', 'lane', 'run'].every(key => name(entry[key])));
    return Object.freeze(Object.fromEntries(['id', 'label', 'host', 'worker', 'objective', 'lane', 'run'].map(key => [key, entry[key]])));
  });
  require(new Set(entries.map(entry => entry.id)).size === entries.length);
  return Object.freeze({ revision: value.revision, entries: Object.freeze(entries) });
}
export function profileOpenReply(raw, entry) {
  const value = parse(raw);
  require(value?.schema === 'mesh.remote-profile-open/v1' && value.profile === entry.id && value.label === entry.label);
  const selection = selectionReply(value.selection), draft = draftReply(value.draft), input = setupInput(value.input);
  require(draft.installation && draft.identity && draft.hosts && ['host', 'worker', 'objective', 'lane', 'run'].every(key => selection[key] === entry[key] && input[key] === entry[key]));
  return { selection, draft, preset: { id: draft.id, input: { ...input, port: String(input.port) } } };
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
  let draft = null, profiles = null, preset = null;
  let selection = null, status = null, results = null, busy = false, error = '', disposed = false;
  const publish = () => { if (!disposed) document.dispatchEvent(new CustomEvent('mesh:remote-observation-projection', { detail: { profiles, preset, draft, selection, status, results, busy, error, available: typeof invoke === 'function' } })); };
  async function intent(event) {
    if (disposed || busy || typeof invoke !== 'function') return;
    const type = event.detail?.type;
    if (!['choose', 'pick-setup', 'clear-setup', 'configure', 'status', 'results', 'forget', 'profiles-list', 'profiles-recover', 'profile-save', 'profile-open', 'profile-remove'].includes(type) || (['status', 'results', 'forget'].includes(type) && !selection)) return;
    if (type === 'pick-setup' && !['installation', 'identity', 'hosts'].includes(event.detail.part)) return;
    if (type === 'configure' && !(draft?.installation && draft?.identity && draft?.hosts)) return;
    if (type === 'profile-save' && (!selection || !profiles || !labelText(event.detail.label))) return;
    const entry = profiles?.entries.find(item => item.id === event.detail.profile);
    if (['profile-open', 'profile-remove'].includes(type) && !entry) return;
    busy = true; error = ''; publish();
    try {
      if (type.startsWith('profile')) {
        const action = ({ 'profiles-list': 'list', 'profiles-recover': 'recover', 'profile-save': 'save', 'profile-open': 'open', 'profile-remove': 'remove' })[type];
        const args = { action, selection: action === 'save' ? selection.id : '', label: action === 'save' ? event.detail.label : '', profile: ['open', 'remove'].includes(action) ? entry.id : '', revision: ['list', 'recover'].includes(action) ? '' : profiles.revision };
        const raw = await invoke('remote_connection_profiles', args);
        if (action === 'open') {
          const opened = profileOpenReply(raw, entry);
          selection = opened.selection; draft = opened.draft; preset = opened.preset; status = null; results = null;
        } else profiles = profilesReply(raw);
      } else if (type === 'clear-setup') {
        draft = draftReply(await invoke('clear_remote_setup')); preset = null;
      } else if (type === 'pick-setup') {
        const raw = await invoke('pick_remote_setup_file', { draft: draft?.id ?? '', part: event.detail.part });
        if (raw !== null) draft = draftReply(raw);
      } else if (type === 'configure') {
        const input = setupInput(event.detail.input);
        selection = selectionReply(await invoke('configure_remote_observation', { draft: draft.id, input: JSON.stringify(input) }));
        status = null; results = null;
      } else if (type === 'choose') {
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
