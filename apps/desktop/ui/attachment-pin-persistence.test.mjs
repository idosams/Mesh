import assert from 'node:assert/strict';
import test from 'node:test';
import { createPinPersistence, pinSnapshot } from './attachment-pin-persistence.js';
const pin = { key: '1', project: 'a'.repeat(64), base: 'b'.repeat(64), target: 'c'.repeat(64), after: null, path: 'notes.txt' };
const snapshot = (pins = [], revision = '0') => ({ schema: 'mesh.desktop-pin-selectors/v1', revision, pins });
const settle = () => new Promise((resolve) => setImmediate(resolve));
function setup(invoke) {
  let pins = []; const states = [];
  const store = createPinPersistence({ invoke, selectors: () => pins, restore: async (value) => { pins = value; }, status: (...value) => states.push(value) });
  return { store, states, set: (value) => { pins = value; store.changed(); }, get: () => pins };
}
test('pin projection rejects extra content, invalid identities and unsafe selectors', () => {
  assert.deepEqual(pinSnapshot(snapshot([pin])), snapshot([pin]));
  for (const changed of [{ ...pin, text: 'content' }, { ...pin, path: '../outside' }, { ...pin, key: '18446744073709551616' }, { ...pin, target: 'live' }]) {
    assert.throws(() => pinSnapshot(snapshot([changed])));
  }
});
test('close during outstanding save persists empty set after earlier acknowledgement', async () => {
  let complete; const writes = [];
  const h = setup(async (command, args) => {
    if (command === 'load_attachment_pins') return snapshot();
    const value = JSON.parse(args.snapshot); writes.push(value);
    if (writes.length === 1) return new Promise((resolve) => { complete = () => resolve({ ...value, revision: '1' }); });
    return { ...value, revision: '2' };
  });
  await h.store.ensureLoaded(); h.set([pin]); h.set([]);
  complete(); await settle();
  assert.deepEqual(writes.map((value) => value.pins), [[pin], []]);
  assert.equal(writes[1].revision, '1');
  assert.equal(h.states.at(-1)[0], 'saved');
});
test('uncertain successful save is acknowledged by retry without rewriting', async () => {
  let stored = snapshot(); let writes = 0;
  const h = setup(async (command, args) => {
    if (command === 'load_attachment_pins') return stored;
    stored = { ...JSON.parse(args.snapshot), revision: '1' }; writes++; throw new Error('transport lost');
  });
  await h.store.ensureLoaded(); h.set([pin]); await settle();
  assert.equal(h.states.at(-1)[0], 'error');
  await h.store.retry();
  assert.equal(h.states.at(-1)[0], 'saved'); assert.equal(writes, 1);
});
test('external changes refuse overwrite and explicit reload restores stored selectors', async () => {
  let stored = snapshot(); let writes = 0;
  const h = setup(async (command) => {
    if (command === 'load_attachment_pins') return stored;
    writes++; throw new Error('revision conflict');
  });
  await h.store.ensureLoaded();
  stored = snapshot([{ ...pin, key: '2' }], '1');
  h.set([pin]); await settle(); await h.store.retry();
  assert.equal(writes, 1); assert.deepEqual(h.get(), [pin]);
  assert.equal(h.states.at(-1)[0], 'error');
  await h.store.reload(); assert.deepEqual(h.get(), stored.pins);
});
test('corrupt initial record is never overwritten with local defaults', async () => {
  let writes = 0;
  const h = setup(async (command) => { if (command === 'load_attachment_pins') return 'corrupt'; writes++; });
  await h.store.ensureLoaded(); h.set([pin]); await settle(); await h.store.retry();
  assert.equal(writes, 0); assert.equal(h.states.at(-1)[0], 'error');
});
