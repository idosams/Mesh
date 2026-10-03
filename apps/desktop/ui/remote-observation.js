import { creationInput, creationList, creationPrepared, creationOutcome } from "./remote-creation.js";
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
export function inputRecoveryReply(raw, selection) {
  const value = parse(raw);
  require(value?.schema === 'mesh.remote-panel-input-recovery/v1' && value.id === selection.id
    && ['objective', 'lane', 'run'].every(key => value[key] === selection[key])
    && ['input-materialized', 'input-retained'].includes(value.disposition));
  return { disposition: value.disposition };
}
export function originalRecoveryReply(raw, selection) {
  const value = parse(raw);
  require(value?.schema === 'mesh.remote-panel-original-recovery/v1' && value.id === selection.id
    && ['objective', 'lane', 'run'].every(key => value[key] === selection[key])
    && value.disposition === 'initialization-recovered');
  return Object.freeze({ disposition: value.disposition });
}
export function receiptAttemptsReply(raw, selection) {
  const value = parse(raw);
  require(value?.schema === 'mesh.remote-panel-receipt-attempts/v1' && value.id === selection.id && Array.isArray(value.entries) && value.entries.length <= 64);
  const entries = value.entries.map(entry => {
    require(hex(entry?.offer) && hex(entry.version) && typeof entry.checkpoint === 'string' && /^[A-Za-z0-9_.:-]{1,128}$/.test(entry.checkpoint));
    return Object.freeze({ offer: entry.offer, checkpoint: entry.checkpoint, version: entry.version });
  });
  require(new Set(entries.map(e => e.offer)).size === entries.length);
  return Object.freeze(entries);
}
export function receivedResultReply(raw, selection, offer) {
  const value = parse(raw);
  require(value?.schema === 'mesh.remote-panel-received-result/v1' && value.id === selection.id && value.offer === offer
    && ['objective', 'lane', 'run'].every(key => value[key] === selection[key])
    && ['offer', 'correlation', 'version', 'review'].every(key => hex(value[key])));
  return Object.freeze(Object.fromEntries(['offer', 'correlation', 'version', 'review', 'objective'].map(key => [key, value[key]])));
}
export function observationReply(raw, id, kind, before = 0) {
  const value = parse(raw);
  require(value?.schema === 'mesh.remote-panel-observation/v2' && value.id === id && value.kind === kind);
  if (kind === 'input-inspection') {
    require(decimal(value.observed_ms) && ['unrecorded', 'verified', 'unavailable'].includes(value.disposition));
    return { kind, observed: value.observed_ms, disposition: value.disposition };
  }
  if (kind === 'execution') {
    require(decimal(value.observed_ms) && BigInt(value.observed_ms) <= 18446744073709551615n
      && typeof value.admitted === 'boolean' && typeof value.launch_recorded === 'boolean'
      && (!value.launch_recorded || value.admitted));
    let recorded = null;
    if (value.launch_recorded) {
      const item = value.execution;
      require(item && decimal(item.revision) && BigInt(item.revision) <= 9223372036854775807n);
      const revision = BigInt(item.revision);
      require(revision === 0n ? item.state === 'unrecorded' : revision < 4n ? item.state === 'setup-incomplete'
        : revision === 4n ? item.state === 'launching'
        : ['launching', 'running', 'waiting', 'reconciling', 'stopping', 'succeeded', 'failed', 'cancelled'].includes(item.state));
      recorded = Object.freeze({ revision: item.revision, state: item.state });
    } else require(value.execution === null);
    return Object.freeze({ kind, observed: value.observed_ms, admitted: value.admitted, launchRecorded: value.launch_recorded, recorded });
  }
  if (kind === 'status') {
    require(decimal(value.observed_ms) && typeof value.admitted === 'boolean' && typeof value.launch_recorded === 'boolean'
      && (!value.launch_recorded || value.admitted) && (value.lease_until_ms === null || decimal(value.lease_until_ms)));
    return { kind, observed: value.observed_ms, admitted: value.admitted, launchRecorded: value.launch_recorded, leaseUntil: value.lease_until_ms };
  }
  require(kind === 'results' && typeof value.available === 'boolean' && typeof value.has_more === 'boolean'
    && Number.isSafeInteger(before) && before >= 0 && before <= 4096
    && Number.isSafeInteger(value.count) && value.count >= 0 && value.count <= 16
    && Array.isArray(value.entries) && value.entries.length === value.count);
  if (value.available) {
    require(decimal(value.revision) && Number(value.revision) <= 4096 && Number.isSafeInteger(value.after)
      && value.after === before + value.count && value.after <= Number(value.revision)
      && value.count === Math.min(16, Number(value.revision) - before) && value.has_more === (value.after < Number(value.revision)));
  } else require(value.revision === null && value.after === null && value.count === 0 && !value.has_more);
  const entries = value.entries.map(entry => {
    require(entry && ['offer', 'version', 'review', 'manifest'].every(key => hex(entry[key])) && typeof entry.checkpoint === 'string' && /^[A-Za-z0-9_.:-]{1,128}$/.test(entry.checkpoint));
    return Object.fromEntries(['offer', 'checkpoint', 'version', 'review', 'manifest'].map(key => [key, entry[key]]));
  });
  require(new Set(entries.map(entry => entry.offer)).size === entries.length && new Set(entries.map(entry => entry.checkpoint)).size === entries.length);
  return { kind, available: value.available, count: value.count, revision: value.revision, hasMore: value.has_more, before, after: value.after, entries };

}
export function startRemoteObservation({ document, invoke, CustomEvent }) {
  let creations = null, creationStatus = null, execution = null, inspection = null, originalRecovery = null;
  let draft = null, profiles = null, preset = null, receiptAttempts = null, received = null;
  let selection = null, status = null, results = null, recovery = null, busy = false, error = '', disposed = false;
  const publish = () => { if (!disposed) document.dispatchEvent(new CustomEvent('mesh:remote-observation-projection', { detail: { execution, originalRecovery, inspection, creations, creationStatus, receiptAttempts, received, recovery, profiles, preset, draft, selection, status, results, busy, error, available: typeof invoke === 'function' } })); };
  async function intent(event) {
    if (disposed || busy || typeof invoke !== 'function') return;
    const type = event.detail?.type;
    if (!['creation-prepare','creation-list','creation-inspect','creation-send','choose', 'pick-setup', 'clear-setup', 'configure', 'status', 'execution', 'input-inspection', 'results', 'results-next', 'results-previous', 'forget', 'recover-original', 'reconnect-input', 'profiles-list', 'profiles-recover', 'profile-save', 'profile-open', 'profile-remove', 'receipt-list', 'download-result', 'recover-result'].includes(type) || (['status', 'execution', 'input-inspection', 'results', 'results-next', 'results-previous', 'forget', 'recover-original', 'reconnect-input', 'receipt-list', 'download-result', 'recover-result'].includes(type) && !selection)) return;
    if (type === 'results-next' && !(results?.available && results.hasMore)) return;
    if (type === 'results-previous' && !(results?.available && results.before > 0)) return;
    if (type === 'pick-setup' && !['installation', 'identity', 'hosts'].includes(event.detail.part)) return;
    if (type === 'configure' && !(draft?.installation && draft?.identity && draft?.hosts)) return;
    if (type === 'profile-save' && (!selection || !profiles || !labelText(event.detail.label))) return;
    const entry = profiles?.entries.find(item => item.id === event.detail.profile);
    if (['profile-open', 'profile-remove'].includes(type) && !entry) return;
    const receiptOffer = event.detail?.offer;
    if (type === 'download-result' && !results?.entries.some(item => item.offer === receiptOffer)) return;
    if (type === 'recover-result' && !receiptAttempts?.some(item => item.offer === receiptOffer)) return;
    if (['download-result', 'recover-result'].includes(type)) received = null;
    const creation = creations?.find(item=>item.request===event.detail?.request);
    if (type==='creation-prepare' && !(draft?.installation && draft?.identity && draft?.hosts)) return;
    if (['creation-inspect','creation-send'].includes(type) && !creation) return;
    if (type==='creation-send' && !(creationStatus?.request===creation.request && ['prepared','ready'].includes(creationStatus.kind))) return;
    if (['creation-prepare','creation-inspect','creation-send'].includes(type)) creationStatus = null;
    if (type === 'recover-original') originalRecovery = null;
    busy = true; error = ''; publish();
    try {
      if (type.startsWith('creation-')) {
        const action=type.slice('creation-'.length);
        const input=action==='prepare'?creationInput(event.detail.input):null;
        const raw=await invoke('remote_creation',{action,id:action==='prepare'?draft.id:creation?.request??'',input:input?JSON.stringify(input):''});
        if(action==='list') {creations=creationList(raw);creationStatus=null;}
        else if(action==='prepare') {
          const entry=creationPrepared(raw,draft.id,input);
          creations=[...(creations??[]).filter(e=>e.request!==entry.request),entry];
          creationStatus={request:entry.request,kind:'prepared'};
        } else {
          const result=creationOutcome(raw,action,creation.request);
          if(action==='inspect') {
            const next=result.selection===null?null:selectionReply(result.selection);
            require(next===null || (next.run===`start-${creation.request}` && next.host===creation.host && next.worker===creation.worker));
            selection=next;status=null;execution=null;inspection=null;originalRecovery=null;results=null;recovery=null;receiptAttempts=null;received=null;
          }
          creationStatus={request:result.request,kind:result.kind,disposition:result.disposition};
        }
      } else if (type.startsWith('profile')) {
        const action = ({ 'profiles-list': 'list', 'profiles-recover': 'recover', 'profile-save': 'save', 'profile-open': 'open', 'profile-remove': 'remove' })[type];
        const args = { action, selection: action === 'save' ? selection.id : '', label: action === 'save' ? event.detail.label : '', profile: ['open', 'remove'].includes(action) ? entry.id : '', revision: ['list', 'recover'].includes(action) ? '' : profiles.revision };
        const raw = await invoke('remote_connection_profiles', args);
        if (action === 'open') {
          const opened = profileOpenReply(raw, entry);
          selection = opened.selection; draft = opened.draft; preset = opened.preset; status = null; execution = null; inspection = null; originalRecovery = null; results = null; recovery = null; receiptAttempts = null; received = null;
        } else profiles = profilesReply(raw);
      } else if (type === 'clear-setup') {
        draft = draftReply(await invoke('clear_remote_setup')); preset = null;
      } else if (type === 'pick-setup') {
        const raw = await invoke('pick_remote_setup_file', { draft: draft?.id ?? '', part: event.detail.part });
        if (raw !== null) draft = draftReply(raw);
      } else if (type === 'configure') {
        const input = setupInput(event.detail.input);
        selection = selectionReply(await invoke('configure_remote_observation', { draft: draft.id, input: JSON.stringify(input) }));
        status = null; execution = null; inspection = null; originalRecovery = null; results = null; recovery = null; receiptAttempts = null; received = null;
      } else if (type === 'choose') {
        const raw = await invoke('pick_remote_observation');
        if (raw !== null) { selection = selectionReply(raw); status = null; execution = null; inspection = null; originalRecovery = null; results = null; recovery = null; receiptAttempts = null; received = null; }
      } else if (['receipt-list', 'download-result', 'recover-result'].includes(type)) {
        const action = type === 'receipt-list' ? 'list' : type === 'download-result' ? 'receive' : 'recover';
        const raw = await invoke('remote_result_receipt', { id: selection.id, action, offer: action === 'list' ? '' : receiptOffer });
        if (action === 'list') receiptAttempts = receiptAttemptsReply(raw, selection);
        else received = receivedResultReply(raw, selection, receiptOffer);
      } else if (type === 'recover-original') {
        originalRecovery = originalRecoveryReply(await invoke('recover_original_remote_worker', { id: selection.id }), selection);
      } else if (type === 'reconnect-input') {
        recovery = inputRecoveryReply(await invoke('reconnect_remote_input', { id: selection.id }), selection);
      } else if (type === 'forget') {
        await invoke('forget_remote_observation', { id: selection.id });
        selection = null; status = null; execution = null; inspection = null; originalRecovery = null; results = null; recovery = null; receiptAttempts = null; received = null;
      } else {
        const action = type.startsWith('results') ? 'results' : type;
        const after = type === 'results-next' ? results.after : type === 'results-previous' ? Math.max(0, results.before - 16) : 0;
        const observation = observationReply(await invoke('read_remote_observation', { id: selection.id, action, after }), selection.id, action, after);
        if (type === 'status') status = observation; else if (type === 'execution') {
          require(!execution?.recorded || (observation.recorded
            && BigInt(observation.recorded.revision) >= BigInt(execution.recorded.revision)
            && (observation.recorded.revision !== execution.recorded.revision || observation.recorded.state === execution.recorded.state)));
          execution = observation;
        } else if (type === 'input-inspection') inspection = observation; else {
          require(!observation.available || !results?.available || Number(observation.revision) >= Number(results.revision));
          results = observation;
        }
      }
    } catch { error = type === 'recover-original' ? 'Recovery could not be confirmed. Keep this attempt and read worker status before another action.' : type.startsWith('creation-') ? 'Remote creation could not be confirmed. Load saved requests and inspect the original attempt before another action.' : ['download-result', 'recover-result'].includes(type) ? 'Download could not be confirmed. Use saved downloads to check the original attempt. No work was approved or applied.' : type === 'receipt-list' ? 'Saved downloads are unavailable for this connection.' : type === 'reconnect-input' ? 'Input transfer outcome could not be confirmed. Keep the original assignment, inspect worker status, and resume only explicitly. No new attempt was requested.' : 'Remote observation is unavailable. Previous observations may be out of date. Check the signed application, private configuration and worker connection.'; }
    finally { busy = false; publish(); }
  }
  document.addEventListener('mesh:remote-observation-intent', intent);
  document.addEventListener('mesh:remote-observation-visible', publish);
  return () => { disposed = true; document.removeEventListener('mesh:remote-observation-intent', intent); document.removeEventListener('mesh:remote-observation-visible', publish); };
}
