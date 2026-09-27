import assert from 'node:assert/strict';
import test from 'node:test';
import { attachedProjectList, startAttachedProjects } from './attached-projects.js';
class CustomEvent extends Event {
  constructor(type, init = {}) { super(type); this.detail = init.detail; }
}
const id = 'a'.repeat(64);
const reply = (generation = '1') => ({ schema: 'mesh.desktop-attachments/v1', projects: [{
  id, generation, root: '/original/project', capture: { schema: 'mesh.attachment-capture/v1',
    phase: 'waiting', last_outcome: 'saved', saved_version: 'b'.repeat(64),
    last_complete_capture_age_ms: 1000, attribution: 'unknown', atomic_snapshot: false },
}] });
const settle = () => new Promise((resolve) => setImmediate(resolve));
function harness(invoke) {
  const document = new EventTarget();
  const projections = [];
  const timers = new Map();
  let sequence = 0;
  document.addEventListener('mesh:attachments-projection', (event) => projections.push(event.detail));
  const dispose = startAttachedProjects({ document, invoke, CustomEvent,
    schedule: (callback) => { const id = ++sequence; timers.set(id, callback); return id; },
    cancel: (id) => timers.delete(id),
  });
  const emit = (name, detail) => document.dispatchEvent(new CustomEvent(name, { detail }));
  emit('mesh:attachments-visible', true);
  return { projections, timers, dispose, emit, intent: (detail) => emit('mesh:attachments-intent', detail) };
}
test('attachment projection rejects ambiguous identities and invented attribution', () => {
  assert.equal(attachedProjectList(reply())[0].root, '/original/project');
  for (const mutate of [
    (value) => value.projects.push(value.projects[0]),
    (value) => { value.projects[0].generation = '01'; },
    (value) => { value.projects[0].root = '/project\nother'; },
    (value) => { value.projects[0].capture.attribution = 'agent'; },
    (value) => { value.projects[0].capture.atomic_snapshot = true; },
    (value) => { value.projects[0].capture.saved_version = 'arbitrary'; },
    (value) => { value.projects[0].capture.last_complete_capture_age_ms = -1; },
  ]) {
    const value = reply(); mutate(value);
    assert.throws(() => attachedProjectList(value));
  }
});
test('controls retain exact native generation and stale views cannot control a resumed session', async () => {
  const calls = []; let generation = '1';
  const h = harness(async (command, args) => {
    calls.push({ command, args });
    if (command === 'control_attached_project') generation = '2';
    return JSON.stringify(reply(generation));
  });
  await settle();
  h.intent({ type: 'control', id, generation: '1', action: 'resume' });
  await settle();
  assert.deepEqual(calls[1], { command: 'control_attached_project', args: { id, generation: '1', action: 'resume' } });
  const count = calls.length;
  h.intent({ type: 'control', id, generation: '1', action: 'stop' });
  h.intent({ type: 'control', id, generation: '2', action: 'approve' });
  assert.equal(calls.length, count);
  assert.equal(h.projections.at(-1).projects[0].generation, '2');
  h.dispose();
});
test('failed refresh retains saved identity but disables controls until successful refresh', async () => {
  let fail = false; const calls = [];
  const h = harness(async (command) => { calls.push(command); if (fail) throw new Error('private path'); return reply(); });
  await settle(); fail = true;
  h.intent({ type: 'refresh' }); await settle();
  assert.equal(h.projections.at(-1).projects[0].savedVersion, 'b'.repeat(64));
  assert.ok(h.projections.at(-1).error);
  assert.doesNotMatch(h.projections.at(-1).error, /private path/);
  const count = calls.length;
  h.intent({ type: 'control', id, generation: '1', action: 'stop' });
  assert.equal(calls.length, count);
  fail = false; h.intent({ type: 'refresh' }); await settle();
  assert.equal(h.projections.at(-1).error, ''); h.dispose();
});
test('picker cancellation does not attach and requests serialize while picker is open', async () => {
  let choose; const calls = [];
  const h = harness(async (command, args) => {
    calls.push({ command, args });
    if (command === 'pick_folder') return new Promise((resolve) => { choose = resolve; });
    return reply();
  });
  await settle(); h.intent({ type: 'choose' });
  h.intent({ type: 'attach', source: '/other/project' });
  choose(null); await settle();
  assert.equal(calls.filter((call) => call.command === 'attach_existing_project').length, 0);
  assert.equal(h.projections.at(-1).busy, false);
  assert.equal(h.timers.size, 1);
  h.emit('mesh:attachments-visible', false);
  assert.equal(h.timers.size, 0); h.dispose();
});
