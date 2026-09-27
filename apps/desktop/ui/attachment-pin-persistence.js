// Persist selectors, never preview content. Native history remains the only content authority.
const U64_MAX = 18446744073709551615n;
const decimal = (value) => typeof value === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(value) && BigInt(value) <= U64_MAX;
const identity = (value) => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const relative = (value) => value === null || (typeof value === 'string' && value.length > 0 && value.length <= 4096
  && !/[\u0000-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(value)
  && !value.startsWith('/') && !value.split('/').some((part) => !part || part === '.' || part === '..'));
export function pinSnapshot(raw) {
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  if (value?.schema !== 'mesh.desktop-pin-selectors/v1' || !decimal(value.revision)
    || !Array.isArray(value.pins) || value.pins.length > 8) throw new Error('Invalid saved pin snapshot');
  const keys = new Set();
  const pins = value.pins.map((pin) => {
    if (!pin || Object.keys(pin).length !== 6 || !decimal(pin.key) || pin.key === '0' || keys.has(pin.key)
      || !identity(pin.project) || !identity(pin.base) || !identity(pin.target)
      || !relative(pin.after) || !relative(pin.path)) throw new Error('Invalid saved pin selector');
    keys.add(pin.key);
    return { key: pin.key, project: pin.project, base: pin.base, target: pin.target, after: pin.after, path: pin.path };
  });
  return { schema: value.schema, revision: value.revision, pins };
}
const same = (left, right) => JSON.stringify(left) === JSON.stringify(right);
export function createPinPersistence({ invoke, selectors, restore, status }) {
  let revision = '0';
  let acknowledged = [];
  let attempted = false;
  let ready = false;
  let saving = false;
  let dirty = false;
  let failed = false;
  let disposed = false;
  let active = Promise.resolve();
  const report = (phase, message = '') => { if (!disposed) status(phase, message); };
  async function load() {
    attempted = true;
    report('loading');
    try {
      const snapshot = pinSnapshot(await invoke('load_attachment_pins'));
      if (disposed) return;
      await restore(snapshot.pins);
      if (disposed) return;
      revision = snapshot.revision; acknowledged = snapshot.pins; ready = true; dirty = false; failed = false;
      report('saved');
    } catch {
      ready = false; failed = true;
      report('error', 'Saved pins could not be loaded. Their stored record has not been replaced.');
    }
  }
  async function drain() {
    if (saving || !ready || failed || disposed) return;
    saving = true;
    try {
      while (dirty && !disposed) {
        dirty = false;
        const snapshot = pinSnapshot({ schema: 'mesh.desktop-pin-selectors/v1', revision, pins: selectors() });
        report('saving');
        const stored = pinSnapshot(await invoke('save_attachment_pins', { snapshot: JSON.stringify(snapshot) }));
        if (!same(stored.pins, snapshot.pins)
          || (stored.revision !== revision && BigInt(stored.revision) !== BigInt(revision) + 1n)) throw new Error('Pin acknowledgement mismatch');
        revision = stored.revision; acknowledged = stored.pins;
      }
      report('saved');
    } catch {
      dirty = true; failed = true;
      report('error', 'Pin changes are not confirmed saved. Current views remain open; retry saving or reload the saved set.');
    } finally { saving = false; }
  }
  return {
    ensureLoaded() { if (!attempted) active = load(); return active; },
    changed() { dirty = true; if (!saving) active = drain(); },
    async retry() {
      await active;
      if (!ready || disposed) { if (!disposed) active = load(); return active; }
      try {
        const stored = pinSnapshot(await invoke('load_attachment_pins'));
        if (same(stored.pins, selectors())) {
          revision = stored.revision; acknowledged = stored.pins; dirty = false; failed = false; report('saved'); return;
        }
        if (!same(stored.pins, acknowledged)) throw new Error('Concurrent pin change');
        revision = stored.revision; failed = false; dirty = true; active = drain(); await active;
      } catch { failed = true; report('error', 'Saved pins changed or remain unavailable. Reload the saved set to reconcile; local views have not been overwritten.'); }
    },
    async reload() { await active; if (!disposed) { active = load(); await active; } },
    dispose() { disposed = true; },
  };
}
