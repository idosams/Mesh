import { createPinPersistence } from './attachment-pin-persistence.js';
const schema = 'mesh.desktop-progress-pin-selectors/v1';
const fields = ['key', 'objective', 'lane', 'version', 'source', 'starting', 'after', 'object', 'layout'];
const hex = (v, n) => typeof v === 'string' && new RegExp(`^[a-f0-9]{${n}}$`).test(v);
const decimal = v => typeof v === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(v) && BigInt(v) <= 18446744073709551615n;
const check = v => { if (!v) throw new Error('Invalid saved progress selectors'); };
export function progressPinSnapshot(raw) {
  check(typeof raw !== 'string' || raw.length <= 65536);
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  check(value && Object.keys(value).sort().join(',') === 'pins,revision,schema' && value.schema === schema
    && decimal(value.revision) && Array.isArray(value.pins) && value.pins.length <= 4);
  const keys = new Set(), selections = new Set();
  const pins = value.pins.map(pin => {
    check(pin && Object.keys(pin).sort().join(',') === [...fields].sort().join(',')
      && decimal(pin.key) && pin.key !== '0' && !keys.has(pin.key)
      && typeof pin.objective === 'string' && /^fleet-[a-f0-9]{64}$/.test(pin.objective)
      && typeof pin.lane === 'string' && /^lane-[a-f0-9]{64}$/.test(pin.lane)
      && [pin.version, pin.source, pin.starting].every(v => hex(v, 64))
      && [pin.after, pin.object].every(v => v === null || hex(v, 32)) && ['inline', 'split'].includes(pin.layout));
    const id = JSON.stringify([pin.objective, pin.lane, pin.version]);
    check(!selections.has(id)); keys.add(pin.key); selections.add(id);
    return Object.fromEntries(fields.map(field => [field, pin[field]]));
  });
  return { schema, revision: value.revision, pins };
}
export const createProgressPinPersistence = options => createPinPersistence({ ...options, schema, parseSnapshot: progressPinSnapshot, loadCommand: 'load_progress_pins', saveCommand: 'save_progress_pins' });
