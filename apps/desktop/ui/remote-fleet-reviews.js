import { createRemoteProjectWorkflow } from './remote-project-workflow.js';
import { createRemotePinPersistence } from './remote-fleet-pin-persistence.js';
import { savedReviewItem } from './fleet-reviews.js';
import { loadSavedArtifact } from './fleet-artifact-preview.js';
import { validatedReviewArtifactPreview } from './review-artifact-validation.js';
const digest = v => typeof v === 'string' && /^[a-f0-9]{64}$/.test(v);
const objective = v => typeof v === 'string' && /^fleet-[a-f0-9]{64}$/.test(v);
const id = v => typeof v === 'string' && /^[A-Za-z0-9_.:-]{1,128}$/.test(v);
const count = v => Number.isSafeInteger(v) && v >= 0 && v <= 4096;
const require = v => { if (!v) throw new Error('Remote review could not be verified'); };
const parse = raw => { require(typeof raw !== 'string' || raw.length <= 40 * 1024 * 1024); return typeof raw === 'string' ? JSON.parse(raw) : raw; };
const keys = (v, names) => v && typeof v === 'object' && !Array.isArray(v) && Object.keys(v).sort().join(',') === names;
const fields = ['offer', 'correlation', 'lane', 'run', 'version', 'bundle', 'remote_version'];
export function remoteReviewPage(raw, expected, after, snapshot) {
  const v = parse(raw);
  require(objective(expected) && count(after) && keys(v, 'after,entries,next,objective,schema,snapshot') && v.schema === 'mesh.desktop-remote-reviews/v1'
    && v.objective === expected && v.after === after && count(v.snapshot) && v.snapshot >= after && (snapshot === null || snapshot === v.snapshot)
    && Array.isArray(v.entries) && v.entries.length === Math.min(16, v.snapshot - after));
  const offers = new Set();
  const rows = v.entries.map((entry, i) => {
    require(keys(entry, 'offer,selection,sequence') && entry.sequence === after + i + 1 && digest(entry.offer) && !offers.has(entry.offer));
    offers.add(entry.offer);
    if (entry.selection === null) return { sequence: entry.sequence, offer: entry.offer, selection: null };
    const s = entry.selection;
    require(keys(s, [...fields].sort().join(',')) && s.offer === entry.offer && fields.filter(f => !['lane', 'run'].includes(f)).every(f => digest(s[f])) && id(s.lane) && id(s.run));
    return { sequence: entry.sequence, offer: entry.offer, selection: { objective: expected, ...s } };
  });
  const end = after + rows.length;
  require(v.next === (end < v.snapshot ? end : null));
  return { rows, snapshot: v.snapshot, next: v.next };
}
export function remoteReview(raw, selection) {
  const v = parse(raw);
  require(v?.schema === 'mesh.remote-saved-review/v1' && v.objective === selection.objective
    && fields.every(f => v[f] === selection[f]) && v.comparison_basis === 'received-result-tree' && v.approval_authority === false);
  return savedReviewItem(v.review, selection.version, selection.bundle);
}
export function remoteArtifactSide(raw, selection, change, side, kind, page) {
  const v = parse(raw);
  require(keys(v, 'correlation,object,objective,offer,preview,schema') && v.schema === 'mesh.remote-artifact-preview/v1'
    && ['objective','offer','correlation'].every(f => v[f] === selection[f]) && v.object === change.object_id);
  const answer = validatedReviewArtifactPreview(v.preview, change, side, kind);
  require(kind !== 'pdf' || answer.page_number === page);
  return { side, versionId: answer.version_id, contentDigest: answer.content_digest, imageDataUrl: answer.image_data_url,
    pageNumber: answer.page_number, pageCount: answer.page_count, textSource: answer.text_source, textLines: answer.text_lines,
    textSections: answer.text_sections?.map(s => ({ label: s.label, lineStart: s.line_start, lineCount: s.line_count })) ?? null, textTruncated: answer.text_truncated };
}
export function createRemoteFleetReviews({ invoke, changed, otherPinCount = () => 0, requestId }) {
  let queues = {}, pins = [], next = 1n, generation = 0, disposed = false, notice = '';
  const publish = () => { if (!disposed) changed(); };
  const project = createRemoteProjectWorkflow({ invoke, changed: publish, requestId });
  let persistenceEnabled = false, editable = true, controlBusy = false, persistence = { phase: 'session', message: '' };
  const persist = () => { if (persistenceEnabled) storage.changed(); };
  const storage = createRemotePinPersistence({ invoke,
    selectors: () => pins.map(p => ({ key: p.key.slice(7), ...p.selection, ...p.view })),
    restore(saved) {
      if (saved.length + otherPinCount() > 8) throw new Error('Close local panels before loading this saved set');
      pins = saved.map(p => ({ key: `remote-${p.key}`, selection: Object.fromEntries(['objective', ...fields].map(f => [f,p[f]])), view: { object: p.object, mode: p.mode, layout: p.layout }, review: null, loading: false, error: '' }));
      next = saved.reduce((max,p) => BigInt(p.key) >= max ? BigInt(p.key) + 1n : max, next);
      notice = ''; publish();
      for (const p of pins) void read(p);
    },
    status(phase, message) { if (phase === 'loading') editable = false; if (phase === 'saved') editable = true; persistence = { phase, message }; publish(); },
  });
  async function control(action) {
    if (controlBusy) return;
    controlBusy = true; publish();
    try { await action(); } finally { controlBusy = false; publish(); }
  }
  async function page(name, after = 0, snapshot = null) {
    if (queues[name]?.loading) return;
    const token = ++generation;
    queues = { ...queues, [name]: { ...queues[name], loading: true, error: '', token } }; publish();
    try {
      const result = remoteReviewPage(await invoke('remote_fleet_reviews', { objective: name, after, snapshot }), name, after, snapshot);
      if (!disposed && queues[name]?.token === token) queues = { ...queues, [name]: { page: result, loading: false, error: '', token } };
    } catch { if (!disposed && queues[name]?.token === token) queues = { ...queues, [name]: { ...queues[name], loading: false, error: 'Remote results could not be verified. Refresh to retry.' } }; }
    publish();
  }
  async function read(pin) {
    const token = ++generation;
    pins = pins.map(p => p.key === pin.key ? { ...p, token, loading: true, error: '' } : p); publish();
    try {
      const { objective, offer, correlation } = pin.selection;
      const review = remoteReview(await invoke('inspect_remote_fleet_review', { objective, offer, correlation }), pin.selection);
      if (!disposed) pins = pins.map(p => p.key === pin.key && p.token === token ? { ...p, review, loading: false } : p);
    } catch { if (!disposed) pins = pins.map(p => p.key === pin.key && p.token === token ? { ...p, loading: false, error: 'This exact remote review is unavailable. Any displayed content is the previously verified snapshot.' } : p); }
    publish();
  }
  async function artifact(pin, object, page) {
    const change = pin.review?.bundle_changes.find(c => c.object_id === object);
    if (!change || !pin.review.content_complete || pin.error) return;
    const token = ++generation;
    pins = pins.map(p => p.key === pin.key ? { ...p, artifact: { generation: token, object, page, loading: true, error: '', envelope: null } } : p); publish();
    try {
      const request = { ...pin, selection: Object.fromEntries(['objective','offer','correlation','bundle'].map(f => [f, pin.selection[f]])) };
      const envelope = await loadSavedArtifact(invoke, request, change, page, token, 'render_remote_fleet_artifact', remoteArtifactSide);
      if (!disposed) pins = pins.map(p => p.key === pin.key && p.artifact?.generation === token ? { ...p, artifact: { ...p.artifact, loading: false, envelope } } : p);
    } catch { if (!disposed) pins = pins.map(p => p.key === pin.key && p.artifact?.generation === token ? { ...p, artifact: { ...p.artifact, loading: false, error: 'This exact saved artifact preview is unavailable.' } } : p); }
    publish();
  }
  return {
    snapshot: () => ({ ...project.snapshot(), remoteReviewQueues: queues, remoteReviewPins: pins, remoteReviewNotice: notice, remoteReviewPersistence: { ...persistence, editable, busy: controlBusy } }),
    loadSaved() { if (!disposed && typeof invoke === 'function') { persistenceEnabled = true; return Promise.all([storage.ensureLoaded(), project.load()]); } },
    dispose() { disposed = true; storage.dispose(); project.dispose(); },
    handle(v) {
      if (typeof v?.type !== 'string' || !v.type.startsWith('remote-')) return false;
      if (disposed || typeof invoke !== 'function') return true;
      const shape = Object.keys(v).sort().join(',');
      if(v.type==='remote-project-load' && shape==='type') {void project.load();return true;}
      if(v.type==='remote-project-action' && shape==='mode,request,type' && /^[a-f0-9]{32}$/.test(v.request)) {void project.retry(v.request,v.mode);return true;}
      if(v.type==='remote-project-forget' && shape==='request,type' && /^[a-f0-9]{32}$/.test(v.request)) {void project.forget(v.request);return true;}

      if (v.type === 'remote-pins-retry' && shape === 'type') { void control(() => storage.retry()); return true; }
      if (v.type === 'remote-pins-reload' && shape === 'type') { void control(() => storage.reload()); return true; }
      if (!editable && !['remote-results','remote-results-next','remote-results-close','remote-retry'].includes(v.type)) return true;
      // Native retained history is the oracle; live fleet polling is not a prerequisite for review.
      if (v.type === 'remote-results' && shape === 'objective,type' && objective(v.objective)) {
        if (!queues[v.objective] && Object.keys(queues).length >= 16) return true;
        void page(v.objective); return true;
      }
      const queue = queues[v.objective];
      if (v.type === 'remote-results-close' && shape === 'objective,type') { queues = { ...queues }; delete queues[v.objective]; publish(); return true; }
      if (v.type === 'remote-results-next' && shape === 'objective,type' && queue?.page?.next !== null && queue?.page && !queue.loading && !queue.error) { void page(v.objective, queue.page.next, queue.page.snapshot); return true; }
      if (v.type === 'remote-pin' && shape === 'correlation,objective,offer,type' && queue?.page && !queue.loading && !queue.error) {
        const selection = queue.page.rows.find(r => r.selection?.offer === v.offer && r.selection?.correlation === v.correlation)?.selection;
        if (!selection || pins.some(p => p.selection.objective === v.objective && p.selection.correlation === v.correlation)) return true;
        if (pins.length + otherPinCount() >= 8 || next > 18446744073709551615n) { notice = 'Close a review panel before opening another. Eight can stay pinned together.'; publish(); return true; }
        const pin = { key: `remote-${next++}`, selection, review: null, loading: false, error: '', view: { object: null, mode: 'content', layout: 'split' } };
        pins = [...pins, pin]; notice = ''; persist(); void read(pin); return true;
      }
      const pin = pins.find(p => p.key === v.pin);
      if (!pin) return true;
      if(v.type==='remote-project-prepare' && shape==='pin,type' && editable && pin.review?.content_complete && !pin.loading && !pin.error) {void project.prepare(pin.selection);return true;}

      if (v.type === 'remote-close' && shape === 'pin,type') { pins = pins.filter(p => p !== pin); persist(); publish(); }
      if (v.type === 'remote-retry' && shape === 'pin,type' && !pin.loading) void read(pin);
      if (v.type === 'remote-view' && shape === 'layout,mode,object,pin,type' && /^[a-f0-9]{32}$/.test(v.object) && pin.review?.bundle_changes.some(c => c.object_id === v.object) && ['content','visual'].includes(v.mode) && ['inline','split'].includes(v.layout)) {
        pins = pins.map(p => p === pin ? { ...p, view: { object: v.object, mode: v.mode, layout: v.layout } } : p); persist(); publish();
      }
      if (v.type === 'remote-artifact' && shape === 'object,page,pin,type' && /^[1-9][0-9]?$/.test(v.page) && Number(v.page) <= 64) void artifact(pin, v.object, Number(v.page));
      return true;
    },
  };
}
