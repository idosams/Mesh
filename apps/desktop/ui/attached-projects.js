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
    if (!/^[a-f0-9]{64}$/.test(project?.id ?? '') || ids.has(project.id)
      || !/^[1-9][0-9]{0,19}$/.test(project.generation ?? '')
      || !safeText(project.root, 4096) || !project.root.startsWith('/')
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
      captureAgeMs: capture.last_complete_capture_age_ms });
  });
}

export function startAttachedProjects({ document, invoke, CustomEvent, schedule = setTimeout, cancel = clearTimeout }) {
  let projects = [];
  let busy = false;
  let error = '';
  let mounted = false;
  let disposed = false;
  let timer = null;
  const publish = () => {
    if (!disposed) document.dispatchEvent(new CustomEvent('mesh:attachments-projection', {
      detail: { projects, busy, error, available: typeof invoke === 'function' },
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
    if (!mounted || busy || !value || typeof value !== 'object') return;
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
