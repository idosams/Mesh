import { createPinPersistence } from './attachment-pin-persistence.js';
const schema = 'mesh.desktop-remote-fleet-pin-selectors/v1';
const fields = ['key','objective','offer','correlation','lane','run','version','bundle','remote_version','object','mode','layout'];
const hex = (v, n) => typeof v === 'string' && new RegExp(`^[a-f0-9]{${n}}$`).test(v);
const decimal = v => typeof v === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(v) && BigInt(v) <= 18446744073709551615n;
const id = v => typeof v === 'string' && /^[A-Za-z0-9_.:-]{1,128}$/.test(v);
export function remotePinSnapshot(raw) {
  if (typeof raw === 'string' && new TextEncoder().encode(raw).length > 65536) throw new Error('Remote pin snapshot too large');
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  if (!value || Object.keys(value).sort().join(',') !== 'pins,revision,schema' || value.schema !== schema || !decimal(value.revision) || !Array.isArray(value.pins) || value.pins.length > 8) throw new Error('Invalid remote pin snapshot');
  const keys = new Set(), selections = new Set();
  const pins = value.pins.map(pin => {
    if (!pin || Object.keys(pin).sort().join(',') !== [...fields].sort().join(',') || !decimal(pin.key) || pin.key === '0' || keys.has(pin.key)
      || typeof pin.objective !== 'string' || !/^fleet-[a-f0-9]{64}$/.test(pin.objective) || !id(pin.lane) || !id(pin.run)
      || !['offer','correlation','version','bundle','remote_version'].every(f => hex(pin[f],64)) || !(pin.object === null || hex(pin.object,32))
      || !['content','visual'].includes(pin.mode) || !['inline','split'].includes(pin.layout)) throw new Error('Invalid remote pin selector');
    const selection = `${pin.objective}/${pin.correlation}`;
    if (selections.has(selection)) throw new Error('Duplicate remote selection');
    keys.add(pin.key); selections.add(selection);
    return Object.fromEntries(fields.map(f => [f,pin[f]]));
  });
  return { schema, revision: value.revision, pins };
}
export const createRemotePinPersistence = options => createPinPersistence({ ...options, parseSnapshot: remotePinSnapshot, schema, loadCommand: 'load_remote_fleet_pins', saveCommand: 'save_remote_fleet_pins' });
