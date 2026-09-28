import { createPinPersistence } from './attachment-pin-persistence.js';
const hex = (value, length) => typeof value === 'string' && new RegExp(`^[a-f0-9]{${length}}$`).test(value);
const decimal = value => typeof value === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(value) && BigInt(value) <= 18446744073709551615n;
const id = value => typeof value === 'string' && /^[A-Za-z0-9_.:-]{1,128}$/.test(value);
const fields = ['key', 'objective', 'lane', 'checkpoint', 'version', 'bundle', 'source_version', 'input_after', 'input_object', 'input_open', 'input_layout', 'review_object', 'review_mode', 'review_layout'];
const schema = 'mesh.desktop-fleet-pin-selectors/v1';
export function fleetPinSnapshot(raw) {
  if (typeof raw === 'string' && raw.length > 131072) throw new Error('Saved fleet pins exceed the limit');
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  if (!value || Object.keys(value).sort().join(',') !== 'pins,revision,schema' || value.schema !== schema || !decimal(value.revision) || !Array.isArray(value.pins) || value.pins.length > 8) throw new Error('Invalid saved fleet pins');
  const keys = new Set(), selections = new Set();
  const pins = value.pins.map(pin => {
    if (!pin || Object.keys(pin).sort().join(',') !== [...fields].sort().join(',')
      || !decimal(pin.key) || pin.key === '0' || keys.has(pin.key)
      || typeof pin.objective !== 'string' || !/^fleet-[a-f0-9]{64}$/.test(pin.objective)
      || !id(pin.lane) || !id(pin.checkpoint) || ![pin.version, pin.bundle, pin.source_version].every(value => hex(value, 64))
      || ![pin.input_after, pin.input_object, pin.review_object].every(value => value === null || hex(value, 32))
      || typeof pin.input_open !== 'boolean' || (!pin.input_open && (pin.input_after !== null || pin.input_object !== null))
      || !['inline', 'split'].includes(pin.input_layout) || !['inline', 'split'].includes(pin.review_layout)
      || !['visual', 'content'].includes(pin.review_mode)) throw new Error('Invalid saved fleet selection');
    const identity = JSON.stringify([pin.objective, pin.lane, pin.checkpoint, pin.version, pin.bundle]);
    if (selections.has(identity)) throw new Error('Duplicate saved fleet selection');
    keys.add(pin.key); selections.add(identity);
    return Object.fromEntries(fields.map(field => [field, pin[field]]));
  });
  return { schema, revision: value.revision, pins };
}
export const defaultFleetView = () => ({ input_after: null, input_object: null, input_open: false, input_layout: 'inline', review_object: null, review_mode: 'content', review_layout: 'split' });
export const createFleetPinPersistence = options => createPinPersistence({ ...options, parseSnapshot: fleetPinSnapshot, schema, loadCommand: 'load_fleet_pins', saveCommand: 'save_fleet_pins' });
