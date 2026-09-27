// Native attachment coordinator. React receives display data and emits bounded user intents.
const PHASES = new Set(['starting', 'scanning', 'saving', 'waiting', 'stopping', 'stopped', 'failed']);
const OUTCOMES = new Set(['pending', 'saved', 'unchanged', 'incomplete', 'source-unavailable', 'store-unavailable', 'save-unavailable', 'cancelled']);
const safeText = (value, maximum) => typeof value === 'string' && value.length > 0
  && value.length <= maximum && !/[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(value);

export function attachedProjectList(raw) {
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  if (value?.schema !== 'mesh.desktop-attachments/v1' || !Array.isArray(value.projects)
    || value.projects.length > 32) throw new Error('Invalid attachment list');
  const ids = new Set();
  return value.projects.map((project) => {
    const capture = project?.capture;
    if (typeof project?.id !== 'string' || typeof project.generation !== 'string'
      || !/^[a-f0-9]{64}$/.test(project.id) || ids.has(project.id)
      || !/^[1-9][0-9]{0,19}$/.test(project.generation ?? '')
      || !safeText(project.root, 4096) || !project.root.startsWith('/')
      || ![undefined, null, 'restored-stopped', 'unavailable'].includes(project.recovery)
      || capture?.schema !== 'mesh.attachment-capture/v1'
      || !PHASES.has(capture.phase) || !OUTCOMES.has(capture.last_outcome)
      || capture.attribution !== 'unknown' || capture.atomic_snapshot !== false
      || (capture.saved_version !== null && !/^[a-f0-9]{64}$/.test(capture.saved_version))
      || (capture.last_complete_capture_age_ms !== null
        && (!Number.isSafeInteger(capture.last_complete_capture_age_ms) || capture.last_complete_capture_age_ms < 0))) {
      throw new Error('Invalid attachment status');
    }
    ids.add(project.id);
    return Object.freeze({ id: project.id, generation: project.generation, root: project.root,
      phase: capture.phase, outcome: capture.last_outcome, savedVersion: capture.saved_version,
      captureAgeMs: capture.last_complete_capture_age_ms, recovery: project.recovery ?? null });
  });
}

export function attachedVersionPage(raw, id, before) {
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  if (value?.schema !== 'mesh.attachment-versions/v1' || value.project !== id || value.before !== before
    || !Array.isArray(value.versions) || value.versions.length > 50
    || value.versions.some((version) => typeof version !== 'string' || !/^[a-f0-9]{64}$/.test(version))
    || new Set(value.versions).size !== value.versions.length
    || value.versions.includes(before)
    || (value.next_before !== null && (value.versions.length !== 50 || value.next_before !== value.versions.at(-1)))) {
    throw new Error('Invalid attachment version page');
  }
  return Object.freeze({ versions: Object.freeze([...value.versions]), nextBefore: value.next_before });
}

const parseInspection = (raw, id, operation, schema) => {
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  if (value?.project !== id || value.inspection?.operation !== operation || value.inspection.schema !== schema) {
    throw new Error('Saved inspection identity mismatch');
  }
  return value.inspection;
};
export function attachedEntries(raw, id, operation, after) {
  const value = parseInspection(raw, id, operation, 'mesh.attachment-entries/v1');
  if (value.after !== after || !Array.isArray(value.entries) || value.entries.length > 200) throw new Error('Invalid saved entries');
  const paths = new Set();
  for (const entry of value.entries) {
    if (!safeText(entry?.path, 4096) || entry.path.startsWith('/')
      || entry.path.split('/').some((part) => !part || part === '.' || part === '..')
      || paths.has(entry.path) || entry.path === after
      || !['file', 'folder'].includes(entry.kind)
      || (entry.kind === 'folder' && (entry.bytes !== null || entry.digest !== null || entry.executable !== null))
      || (entry.kind === 'file' && (!Number.isSafeInteger(entry.bytes) || entry.bytes < 0
        || typeof entry.digest !== 'string' || !/^[a-f0-9]{64}$/.test(entry.digest) || typeof entry.executable !== 'boolean'))) {
      throw new Error('Invalid saved entry');
    }
    paths.add(entry.path);
  }
  if (value.next_after !== null && (value.entries.length !== 200 || value.next_after !== value.entries.at(-1).path)) {
    throw new Error('Invalid entry cursor');
  }
  return { operation, entries: value.entries, nextAfter: value.next_after, file: null };
}
export function attachedText(raw, id, operation, entry) {
  const value = parseInspection(raw, id, operation, 'mesh.attachment-text/v1');
  if (value.path !== entry.path || value.digest !== entry.digest || value.bytes !== entry.bytes || value.executable !== entry.executable
    || !['text', 'binary', 'too-large'].includes(value.state)
    || (value.state === 'text' && (typeof value.text !== 'string' || value.text.length > 262144 || value.bytes > 262144))
    || (value.state !== 'text' && value.text !== null)) throw new Error('Saved text identity mismatch');
  return value;
}

export function attachedComparison(raw, id, base, target, after) {
  const envelope = typeof raw === 'string' ? JSON.parse(raw) : raw;
  const value = envelope?.comparison;
  if (envelope?.project !== id || value?.schema !== 'mesh.attachment-comparison/v1'
    || value.base !== base || value.target !== target || value.after !== after
    || !Array.isArray(value.changes) || value.changes.length > 200
    || !Number.isSafeInteger(value.total) || value.total < value.changes.length) throw new Error('Invalid saved comparison');
  const paths = new Set();
  for (const change of value.changes) {
    if (!safeText(change?.path, 4096) || change.path.startsWith('/')
      || change.path.split('/').some((part) => !part || part === '.' || part === '..')
      || paths.has(change.path) || change.path === after
      || !['added', 'removed', 'modified', 'mode-changed', 'type-changed'].includes(change.change)) throw new Error('Invalid comparison change');
    paths.add(change.path);
    for (const side of [change.before, change.after]) {
      if (side === null) continue;
      if (!side || !['file', 'folder'].includes(side.kind)
        || (side.kind === 'folder' && (side.bytes !== null || side.digest !== null || side.executable !== null))
        || (side.kind === 'file' && (!Number.isSafeInteger(side.bytes) || side.bytes < 0
          || typeof side.digest !== 'string' || !/^[a-f0-9]{64}$/.test(side.digest) || typeof side.executable !== 'boolean'))) throw new Error('Invalid comparison side');
    }
    if (change.change === 'added' ? change.before !== null || change.after === null
      : change.change === 'removed' ? change.before === null || change.after !== null
        : change.before === null || change.after === null) throw new Error('Invalid comparison presence');
  }
  if (value.next_after !== null && (value.changes.length !== 200 || value.next_after !== value.changes.at(-1).path)) throw new Error('Invalid comparison cursor');
  return { base, target, changes: value.changes, total: value.total, nextAfter: value.next_after, file: null };
}

export function startAttachedProjects({ document, invoke, CustomEvent, schedule = setTimeout, cancel = clearTimeout }) {
  let projects = [];
  let histories = {};
  let inspections = {};
  let bases = {};
  let comparisons = {};
  let pins = [];
  let nextPin = 1;
  let busy = false;
  let error = '';
  let mounted = false;
  let disposed = false;
  let timer = null;
  const publish = () => {
    if (!disposed) document.dispatchEvent(new CustomEvent('mesh:attachments-projection', {
      detail: { projects, histories, inspections, bases, comparisons, pins, busy, error, available: typeof invoke === 'function' },
    }));
  };
  const planRefresh = () => {
    if (timer !== null) cancel(timer);
    timer = mounted && !disposed ? schedule(() => { timer = null; void run(); }, 2000) : null;
  };
  async function run(operation) {
    if (busy || disposed || typeof invoke !== 'function') return;
    busy = true;
    publish();
    try {
      if (operation) await operation();
      projects = attachedProjectList(await invoke('attached_projects'));
      error = '';
    } catch {
      error = 'Attachment status is unavailable. Your existing tools can keep working. Retry to refresh.';
    } finally {
      busy = false;
      publish();
      planRefresh();
    }
  }
  function visible(event) {
    mounted = event.detail === true;
    if (mounted) { publish(); void run(); }
    else if (timer !== null) { cancel(timer); timer = null; }
  }
  function intent(event) {
    const value = event.detail;
    if (!mounted || !value || typeof value !== 'object') return;
    if (value.type === 'close-pin' && Object.keys(value).length === 2 && typeof value.pin === 'string') {
      pins = pins.filter((pin) => pin.key !== value.pin); publish(); return;
    }
    if (busy) return;
    if ('id' in value && !projects.some((project) => project.id === value.id)) return;
    if (value.type === 'refresh' && Object.keys(value).length === 1) { void run(); return; }
    if (value.type === 'choose' && Object.keys(value).length === 1) {
      void run(async () => {
        const source = await invoke('pick_folder');
        if (source === null) return;
        if (!safeText(source, 4096) || !source.startsWith('/')) throw new Error('Invalid folder selection');
        await invoke('attach_existing_project', { source });
      });
      return;
    }
    if (value.type === 'attach' && Object.keys(value).length === 2
      && safeText(value.source, 4096) && value.source.startsWith('/')) {
      void run(() => invoke('attach_existing_project', { source: value.source }));
      return;
    }
    if (value.type === 'versions' && Object.keys(value).length === 3
      && projects.some((project) => project.id === value.id)
      && (value.before === null || (typeof value.before === 'string' && value.before === histories[value.id]?.nextBefore))) {
      void run(async () => {
        const page = attachedVersionPage(await invoke('attached_project_versions', {
          id: value.id, before: value.before,
        }), value.id, value.before);
        histories = { ...histories, [value.id]: page };
      });
      return;
    }
    if (value.type === 'inspect' && Object.keys(value).length === 3
      && histories[value.id]?.versions.includes(value.operation)) {
      void run(async () => {
        const inspected = attachedEntries(await invoke('inspect_attached_version', {
          id: value.id, operation: value.operation, path: null, after: null,
        }), value.id, value.operation, null);
        inspections = { ...inspections, [value.id]: inspected };
      });
      return;
    }
    const selected = inspections[value.id];
    if (selected && value.type === 'entries' && Object.keys(value).length === 4 && selected.operation === value.operation
      && typeof value.after === 'string' && value.after === selected.nextAfter) {
      void run(async () => {
        const inspected = attachedEntries(await invoke('inspect_attached_version', {
          id: value.id, operation: value.operation, path: null, after: value.after,
        }), value.id, value.operation, value.after);
        inspections = { ...inspections, [value.id]: { ...inspected, file: selected.file } };
      });
      return;
    }
    if (selected && value.type === 'file' && Object.keys(value).length === 4 && selected.operation === value.operation) {
      const entry = selected.entries.find((entry) => entry.path === value.path && entry.kind === 'file');
      if (!entry) return;
      void run(async () => {
        const file = attachedText(await invoke('inspect_attached_version', {
          id: value.id, operation: value.operation, path: value.path, after: null,
        }), value.id, value.operation, entry);
        inspections = { ...inspections, [value.id]: { ...selected, file } };
      });
      return;
    }
    if (value.type === 'set-base' && Object.keys(value).length === 3 && histories[value.id]?.versions.includes(value.operation)) {
      bases = { ...bases, [value.id]: value.operation }; publish(); return;
    }
    if (value.type === 'compare' && Object.keys(value).length === 3 && bases[value.id]
      && histories[value.id]?.versions.includes(value.target)) {
      const base = bases[value.id];
      void run(async () => {
        const comparison = attachedComparison(await invoke('compare_attached_versions', {
          id: value.id, base, target: value.target, after: null,
        }), value.id, base, value.target, null);
        comparisons = { ...comparisons, [value.id]: comparison };
      });
      return;
    }
    if (value.type === 'pin-comparison' && Object.keys(value).length === 4 && pins.length < 8
      && Number.isSafeInteger(nextPin) && comparisons[value.id] && comparisons[value.id].base === value.base
      && comparisons[value.id].target === value.target) {
      const project = projects.find((project) => project.id === value.id);
      pins = [...pins, { key: String(nextPin++), project: value.id, root: project.root, comparison: comparisons[value.id] }];
      publish(); return;
    }
    const hasPin = Object.hasOwn(value, 'pin');
    const pinned = hasPin ? pins.find((pin) => pin.key === value.pin && pin.project === value.id) : null;
    if (hasPin && !pinned) return;
    const comparison = pinned ? pinned.comparison : comparisons[value.id];
    const updateComparison = (next) => {
      // A pin closed during an outstanding read must not be recreated by its response.
      if (pinned) pins = pins.map((pin) => pin.key === pinned.key ? { ...pin, comparison: next } : pin);
      else comparisons = { ...comparisons, [value.id]: next };
    };
    if (value.type === 'compare-page' && Object.keys(value).length === (hasPin ? 6 : 5)
      && comparison && comparison.base === value.base && comparison.target === value.target
      && typeof value.after === 'string' && comparison.nextAfter === value.after) {
      void run(async () => {
        const page = attachedComparison(await invoke('compare_attached_versions', {
          id: value.id, base: value.base, target: value.target, after: value.after,
        }), value.id, value.base, value.target, value.after);
        updateComparison({ ...page, file: comparison.file });
      });
      return;
    }
    if (value.type === 'compare-file' && Object.keys(value).length === (hasPin ? 6 : 5)
      && comparison && comparison.base === value.base && comparison.target === value.target) {
      const change = comparison.changes.find((change) => change.path === value.path);
      if (!change) return;
      void run(async () => {
        const read = async (side, operation) => side?.kind === 'file'
          ? attachedText(await invoke('inspect_attached_version', { id: value.id, operation, path: value.path, after: null }),
            value.id, operation, { ...side, path: value.path }) : null;
        const [before, after] = await Promise.all([read(change.before, value.base), read(change.after, value.target)]);
        updateComparison({ ...comparison, file: {
          path: value.path, before, after, beforeKind: change.before?.kind ?? 'absent', afterKind: change.after?.kind ?? 'absent',
        } });
      });
      return;
    }
    if (error || value.type !== 'control' || Object.keys(value).length !== 4
      || !['capture', 'stop', 'resume'].includes(value.action)
      || !projects.some((project) => project.id === value.id && project.generation === value.generation)) return;
    void run(() => invoke('control_attached_project', { id: value.id, generation: value.generation, action: value.action }));
  }
  document.addEventListener('mesh:attachments-visible', visible);
  document.addEventListener('mesh:attachments-intent', intent);
  return () => {
    disposed = true;
    if (timer !== null) cancel(timer);
    document.removeEventListener('mesh:attachments-visible', visible);
    document.removeEventListener('mesh:attachments-intent', intent);
  };
}
