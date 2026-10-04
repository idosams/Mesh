import { createRemoteFleetObservations } from './remote-fleet-observations.js';
import { createRemoteFleetReviews } from './remote-fleet-reviews.js';
import { createFleetReviews } from './fleet-reviews.js';
// Presentation of native-owned fleet state. This module never chooses paths or launches processes.
const hex = (value, length) => typeof value === 'string' && new RegExp(`^[a-f0-9]{${length}}$`).test(value);
const identity = (value) => typeof value === 'string' && /^[A-Za-z0-9_.:-]{1,128}$/.test(value);
const objective = (value) => typeof value === 'string' && /^fleet-[a-f0-9]{64}$/.test(value);
const digest = (value) => hex(value, 64);
const nullable = (value, validate) => value === null || validate(value);
const integer = (value, maximum) => Number.isSafeInteger(value) && value >= 0 && value <= maximum;
const decimal = (value) => typeof value === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(value);
const text = (value, maximum) => typeof value === 'string' && value.length > 0 && new TextEncoder().encode(value).length <= maximum && !/[\u0000\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(value);
const states = new Set(['launching', 'running', 'waiting', 'reconciling', 'stopping', 'succeeded', 'failed', 'cancelled']);
const parse = (raw) => typeof raw === 'string' ? JSON.parse(raw) : raw;
const require = (valid) => { if (!valid) throw new Error('Fleet response could not be verified'); };

export function fleetProviderPolicy(value, canonical = true) {
  require(value && typeof value === 'object' && Object.keys(value).sort().join(',') === 'coordinator,providers'
    && Array.isArray(value.providers) && value.providers.length >= 1 && value.providers.length <= 2
    && value.providers.every(provider => ['claude', 'codex'].includes(provider))
    && new Set(value.providers).size === value.providers.length && value.providers.includes(value.coordinator));
  const providers = [...value.providers].sort();
  require(!canonical || providers.every((provider, index) => provider === value.providers[index]));
  return Object.freeze({ coordinator: value.coordinator, providers: Object.freeze(providers) });
}

function remoteAssignment(value) {
  // Older catalogue replies omit this additive projection; omission proves no remote observation.
  if (value === undefined || value === null) return null;
  const positiveU64 = item => decimal(item) && BigInt(item) > 0n && BigInt(item) <= 18446744073709551615n;
  require(typeof value === 'object' && Object.keys(value).sort().join(',') === 'assignment,lease_sequence,lease_until_ms,worker'
    && identity(value.assignment) && value.assignment.length <= 96 && digest(value.worker)
    && positiveU64(value.lease_sequence) && positiveU64(value.lease_until_ms));
  return Object.freeze({ assignment: value.assignment, worker: value.worker, leaseSequence: value.lease_sequence, leaseUntil: value.lease_until_ms });
}

export function fleetCatalogue(raw) {
  const value = parse(raw);
  require(value?.schema === 'mesh.native-fleets/v1' && Array.isArray(value.fleets) && value.fleets.length <= 16);
  const ids = new Set();
  return value.fleets.map((row) => {
    require(objective(row?.objective) && !ids.has(row.objective) && ['current-host', 'restored-unattached', 'unavailable'].includes(row.ownership));
    ids.add(row.objective);
    if (row.ownership === 'unavailable') { require(row.state === null && row.policy == null); return { policy: null, objective: row.objective, ownership: row.ownership, cancelled: false, lanes: [] }; }
    const policy = row.policy == null ? null : fleetProviderPolicy(row.policy);
    const state = row.state;
    require(state?.objective === row.objective && integer(state.revision, Number.MAX_SAFE_INTEGER) && typeof state.cancelled === 'boolean' && Array.isArray(state.lanes) && state.lanes.length <= 1024);
    const lanes = new Set();
    const runs = new Set();
    const result = state.lanes.map((lane) => {
      require(identity(lane?.id) && !lanes.has(lane.id) && nullable(lane.parent, identity) && nullable(lane.source_project, digest)
        && text(lane.goal, 8192) && identity(lane.provider) && digest(lane.base) && typeof lane.allocated === 'boolean');
      lanes.add(lane.id);
      require(lane.allocated ? text(lane.workspace?.root, 4096) && lane.workspace.root.startsWith('/') && text(lane.workspace.installation, 4096) : lane.workspace === null);
      require(lane.run === null || (identity(lane.run?.id) && !runs.has(lane.run.id) && states.has(lane.run.state)));
      if (lane.run) runs.add(lane.run.id);
      return { id: lane.id, parent: lane.parent, sourceProject: lane.source_project, goal: lane.goal, provider: lane.provider,
        base: lane.base, allocated: lane.allocated, run: lane.run ? { id: lane.run.id, state: lane.run.state, remote: remoteAssignment(lane.run.remote) } : null };
    });
    require(result.every(lane => lane.parent === null || (lane.parent !== lane.id && lanes.has(lane.parent))));
    require(!policy || result.every(lane => policy.providers.includes(lane.provider) && (lane.parent !== null || lane.provider === policy.coordinator)));
    return { policy, objective: row.objective, ownership: row.ownership, revision: state.revision, cancelled: state.cancelled, lanes: result };
  });
}
export function fleetActivity(raw) {
  const value = parse(raw);
  require(value?.schema === 'mesh.desktop-fleet-activity/v1' && Array.isArray(value.fleets) && value.fleets.length <= 16);
  const ids = new Set();
  return value.fleets.map(row => {
    require(objective(row?.objective) && !ids.has(row.objective) && ['starting', 'monitoring', 'stop-requested', 'needs-attention'].includes(row.status)
      && typeof row.stop_requested === 'boolean' && nullable(row.observed_at, decimal) && Array.isArray(row.workers) && row.workers.length <= 1024);
    ids.add(row.objective);
    const lanes = new Set();
    const workers = row.workers.map(worker => {
      require(identity(worker?.lane) && !lanes.has(worker.lane) && identity(worker.run) && decimal(worker.observed_at)
        && nullable(worker.thread, value => typeof value === 'string' && /^[a-fA-F0-9-]{36}$/.test(value))
        && nullable(worker.activity, value => text(value, 128)) && decimal(worker.events) && decimal(worker.stderr_lines)
        && ['turn_completed', 'failed', 'streams_closed'].every(key => typeof worker[key] === 'boolean') && nullable(worker.outcome, value => typeof value === 'boolean'));
      const save = worker.progress_save ?? null;
      if (save !== null) require(['waiting', 'saving', 'saved', 'unchanged', 'needs-attention'].includes(save.state)
        && nullable(save.observed_at, decimal) && nullable(save.version, digest)
        && nullable(save.issue, value => typeof value === 'string' && /^[a-z0-9-]{1,128}$/.test(value))
        && (!['saved', 'unchanged'].includes(save.state) || (save.observed_at !== null && save.version !== null)));
      lanes.add(worker.lane);
      return { progressSave: save === null ? null : { state: save.state, observedAt: save.observed_at, version: save.version, issue: save.issue }, lane: worker.lane, run: worker.run, observedAt: worker.observed_at, activity: worker.activity,
        events: worker.events, outcome: worker.outcome, failed: worker.failed, streamsClosed: worker.streams_closed };
    });
    return { objective: row.objective, status: row.status, stopRequested: row.stop_requested, observedAt: row.observed_at, workers };
  });
}
export function fleetProvisioned(raw, pending) {
  const value = parse(raw);
  require(value?.schema === (pending.policy ? 'mesh.desktop-attached-fleet/v2' : 'mesh.desktop-attached-fleet/v1') && value.project === pending.id && value.request === pending.request && objective(value.objective) && value.started === false);
  if (pending.policy) require(JSON.stringify(fleetProviderPolicy(value.policy)) === JSON.stringify(pending.policy));
  return value.objective;
}
export function startFleets({ document, invoke, CustomEvent, schedule = setTimeout, cancel = clearTimeout, requestId = () => globalThis.crypto.randomUUID().replaceAll('-', '') }) {
  let fleets = [], activity = [], pending = null, busy = false, error = '', feedback = '', visible = false, disposed = false, timer = null;
  let sources = { projects: [], histories: {}, error: '' };
  const remoteObservations = createRemoteFleetObservations({ invoke, publish });
  function publish() { if (!disposed) document.dispatchEvent(new CustomEvent('mesh:fleets-projection', { detail: { remoteObservations: remoteObservations.snapshot(), fleets, activity, pending, busy, error, feedback, available: typeof invoke === 'function', ...reviews.snapshot(), ...remoteReviews.snapshot() } })); }
  const reviews = createFleetReviews({ invoke, changed: publish, otherPinCount: () => remoteReviews.snapshot().remoteReviewPins.length, laneFor: (objective, lane) =>
    fleets.find(fleet => fleet.objective === objective && fleet.ownership !== 'unavailable')?.lanes.find(value => value.id === lane) });
  const remoteReviews = createRemoteFleetReviews({ invoke, changed: publish, requestId,
    otherPinCount: () => reviews.snapshot().reviewPins.length });
  const plan = () => {
    if (timer !== null) cancel(timer);
    timer = visible && !disposed ? schedule(() => { timer = null; void refresh(); }, 2000) : null;
  };
  async function refresh(action) {
    if (busy || disposed || typeof invoke !== 'function') return;
    busy = true; publish();
    try {
      if (action) await action();
      const [catalogue, observations] = await Promise.all([invoke('attached_fleets'), invoke('fleet_activity')]);
      const nextFleets = fleetCatalogue(catalogue), nextActivity = fleetActivity(observations);
      fleets = nextFleets; activity = nextActivity; error = ''; remoteObservations.sync(fleets, visible);
    } catch { error = 'Fleet status could not be confirmed. Existing work is retained. Refresh before starting more work.'; }
    finally { busy = false; publish(); plan(); }
  }
  async function provision() {
    fleetProvisioned(await invoke('provision_attached_fleet', { id: pending.id, request: pending.request, goal: pending.goal, version: pending.version, limitsJson: JSON.stringify(pending.limits), ...(pending.policy ? { policyJson: JSON.stringify(pending.policy) } : {}) }), pending);
    pending = null;
    feedback = 'Fleet provisioned. Review it below, then choose Start agents.';
  }
  function intent(event) {
    const value = event.detail;
    if (!visible || disposed || !value || typeof value !== 'object') return;
    if (remoteReviews.handle(value) || reviews.handle(value)) return;
    if (busy) return;
    const fields = Object.keys(value).sort().join(',');
    if (value.type === 'refresh' && fields === 'type') { void refresh(); return; }
    if (value.type === 'retry-provision' && fields === 'type' && pending) { void refresh(provision); return; }
    if (value.type === 'provision' && ['concurrency,depth,goal,id,lanes,type,version', 'concurrency,depth,goal,id,lanes,policy,type,version'].includes(fields) && !pending && !error && !sources.error) {
      let policy;
      if ('policy' in value) { try { policy = fleetProviderPolicy(value.policy, false); } catch { return; } }
      const source = sources.projects.find(project => project.id === value.id);
      if (!source || source.detached || !digest(value.version) || !text(value.goal, 8192) || !value.goal.trim()) return;
      if (![value.lanes, value.concurrency, value.depth].every(number => typeof number === 'string' && /^(0|[1-9][0-9]{0,3})$/.test(number))) return;
      const limits = { lanes: Number(value.lanes), concurrency: Number(value.concurrency), depth: Number(value.depth), retries: 0 };
      if (!integer(limits.lanes, 1024) || limits.lanes < 1 || !integer(limits.concurrency, 64) || limits.concurrency < 1 || limits.concurrency > limits.lanes || !integer(limits.depth, 32)) return;
      const request = requestId(); if (!hex(request, 32)) return;
      pending = Object.freeze({ id: value.id, version: value.version, goal: value.goal, request, limits: Object.freeze(limits), ...(policy ? { policy } : {}) }); feedback = '';
      void refresh(provision); return;
    }
    if (['start', 'stop'].includes(value.type) && fields === 'objective,type') {
      const fleet = fleets.find(fleet => fleet.objective === value.objective && fleet.ownership === 'current-host');
      if (!fleet || fleet.cancelled) return;
      if (value.type === 'start' && (error || !fleet.policy || fleet.lanes.length === 0 || fleet.lanes.some(lane => lane.run !== null || !lane.allocated) || activity.some(row => row.objective === fleet.objective))) return;
      void refresh(async () => {
        // Commands are never replayed by polling. Native objective/ownership checks remain decisive.
        feedback = value.type === 'start'
          ? 'Start could not yet be confirmed. Check the selected providers and accounts and fleet status before retrying.'
          : 'Stop could not yet be confirmed. Refresh and retry the stop request for this fleet.';
        fleetActivity(await invoke(value.type === 'start' ? 'start_attached_fleet' : 'stop_attached_fleet', { objective: value.objective }));
        feedback = value.type === 'start' ? 'Start requested. Activity appears below.' : 'Stop requested. Ownership remains reserved until worker recovery is verified.';
      });
    }
  }
  function mount(event) { visible = event.detail === true; remoteObservations.sync(fleets, visible); if (visible) { publish(); void refresh(); void Promise.resolve(reviews.loadSaved()).then(() => remoteReviews.loadSaved()); } else if (timer !== null) { cancel(timer); timer = null; } }
  function attachment(event) {
    const value = event.detail;
    sources = value && Array.isArray(value.projects) && value.histories && typeof value.error === 'string'
      ? value : { projects: [], histories: {}, error: 'Attachment view unavailable' };
  }
  document.addEventListener('mesh:fleets-visible', mount);
  document.addEventListener('mesh:fleets-intent', intent);
  document.addEventListener('mesh:attachments-projection', attachment);
  return () => { disposed = true; remoteObservations.dispose(); reviews.dispose(); remoteReviews.dispose(); if (timer !== null) cancel(timer); document.removeEventListener('mesh:fleets-visible', mount); document.removeEventListener('mesh:fleets-intent', intent); document.removeEventListener('mesh:attachments-projection', attachment); };
}
