import assert from 'node:assert/strict';
import test from 'node:test';
import { attachedProjectList, attachedVersionPage, attachedEntries, attachedText, attachedComparison, startAttachedProjects } from './attached-projects.js';
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
function harness(invoke, pinInvoke) {
  const document = new EventTarget();
  const projections = [];
  const timers = new Map();
  let sequence = 0;
  document.addEventListener('mesh:attachments-projection', (event) => projections.push(event.detail));
  let stored = { schema: 'mesh.desktop-pin-selectors/v1', revision: '0', pins: [] };
  const native = async (command, args) => {
    if (['load_attachment_pins', 'save_attachment_pins'].includes(command)) {
      if (pinInvoke) return pinInvoke(command, args);
      if (command === 'save_attachment_pins') stored = { ...JSON.parse(args.snapshot), revision: String(BigInt(stored.revision) + 1n) };
      return stored;
    }
    return invoke(command, args);
  };
  const dispose = startAttachedProjects({ document, invoke: native, CustomEvent,
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
    (value) => { value.projects[0].generation = 1; },
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


test('version pages bind project and cursor and reject malformed identities', () => {
  const page = { schema: 'mesh.attachment-versions/v1', project: id, before: null,
    versions: ['b'.repeat(64)], next_before: null };
  assert.deepEqual(attachedVersionPage(page, id, null).versions, page.versions);
  assert.throws(() => attachedVersionPage(page, 'c'.repeat(64), null));
  assert.throws(() => attachedVersionPage(page, id, 'c'.repeat(64)));
  assert.throws(() => attachedVersionPage({ ...page, next_before: page.versions[0] }, id, null));
  assert.throws(() => attachedVersionPage({ ...page, versions: ['arbitrary'] }, id, null));
});
test('status refresh preserves the explicitly loaded immutable version page', async () => {
  let saved = 'b'.repeat(64);
  const calls = [];
  const h = harness(async (command, args) => {
    calls.push({ command, args });
    if (command === 'attached_project_versions') return { schema: 'mesh.attachment-versions/v1',
      project: id, before: null, versions: [saved], next_before: null };
    const value = reply(); value.projects[0].capture.saved_version = saved; return value;
  });
  await settle(); h.intent({ type: 'versions', id, before: null }); await settle();
  saved = 'c'.repeat(64); h.intent({ type: 'refresh' }); await settle();
  assert.equal(h.projections.at(-1).projects[0].savedVersion, saved);
  assert.deepEqual(h.projections.at(-1).histories[id].versions, ['b'.repeat(64)]);
  const count = calls.length;
  h.intent({ type: 'versions', id, before: 'd'.repeat(64) });
  assert.equal(calls.length, count);
  h.dispose();
});


const operation = 'b'.repeat(64);
const savedEntry = { path: 'notes.txt', kind: 'file', bytes: 5, digest: 'd'.repeat(64), executable: false };
const entryReply = () => ({ project: id, inspection: { schema: 'mesh.attachment-entries/v1', operation,
  after: null, entries: [{ ...savedEntry }], next_after: null } });
const textReply = () => ({ project: id, inspection: { schema: 'mesh.attachment-text/v1', operation,
  path: savedEntry.path, digest: savedEntry.digest, bytes: 5, executable: false, state: 'text', text: 'saved' } });
test('saved inspection binds version, path, content metadata and page cursor', () => {
  assert.equal(attachedEntries(entryReply(), id, operation, null).entries[0].path, 'notes.txt');
  assert.equal(attachedText(textReply(), id, operation, savedEntry).text, 'saved');
  assert.throws(() => attachedEntries(entryReply(), id, 'c'.repeat(64), null));
  assert.throws(() => attachedEntries(entryReply(), id, operation, 'another-path'));
  const traversal = entryReply(); traversal.inspection.entries[0].path = '../outside';
  assert.throws(() => attachedEntries(traversal, id, operation, null));
  const altered = textReply(); altered.inspection.digest = 'e'.repeat(64);
  assert.throws(() => attachedText(altered, id, operation, savedEntry));
  const wrong = textReply(); wrong.inspection.path = 'other.txt';
  assert.throws(() => attachedText(wrong, id, operation, savedEntry));
});
test('file selection remains pinned while native capture advances and stale intents are refused', async () => {
  let live = operation; const calls = [];
  const h = harness(async (command, args) => {
    calls.push({ command, args });
    if (command === 'attached_project_versions') return { schema: 'mesh.attachment-versions/v1', project: id,
      before: null, versions: [operation], next_before: null };
    if (command === 'inspect_attached_version') return args.path === null ? entryReply() : textReply();
    const value = reply(); value.projects[0].capture.saved_version = live; return value;
  });
  await settle(); h.intent({ type: 'versions', id, before: null }); await settle();
  h.intent({ type: 'inspect', id, operation }); await settle();
  h.intent({ type: 'file', id, operation, path: 'notes.txt' }); await settle();
  live = 'c'.repeat(64); h.intent({ type: 'refresh' }); await settle();
  assert.equal(h.projections.at(-1).projects[0].savedVersion, live);
  assert.equal(h.projections.at(-1).inspections[id].operation, operation);
  assert.equal(h.projections.at(-1).inspections[id].file.text, 'saved');
  const count = calls.length;
  h.intent({ type: 'file', id, operation: live, path: 'notes.txt' });
  h.intent({ type: 'file', id, operation, path: '../outside' });
  assert.equal(calls.length, count); h.dispose();
});

test('unselected saved-file intents cannot dereference or invoke an absent inspection', async () => {
  const calls = [];
  const h = harness(async (command) => { calls.push(command); return reply(); });
  await settle();
  const count = calls.length;
  h.intent({ type: 'file', id, operation: undefined, path: 'note.txt' });
  h.intent({ type: 'entries', id, operation: undefined, after: 'note.txt' });
  await settle();
  assert.equal(calls.length, count);
  h.dispose();
});

const comparedTarget = 'c'.repeat(64);
const compareReply = () => ({ project: id, comparison: { schema: 'mesh.attachment-comparison/v1',
  base: operation, target: comparedTarget, after: null, total: 1, next_after: null,
  changes: [{ path: 'notes.txt', change: 'modified',
    before: { kind: 'file', bytes: 5, digest: 'd'.repeat(64), executable: false },
    after: { kind: 'file', bytes: 5, digest: 'e'.repeat(64), executable: true } }],
} });
test('comparison projection binds both saved versions and requires valid side metadata', () => {
  assert.equal(attachedComparison(compareReply(), id, operation, comparedTarget, null).total, 1);
  assert.throws(() => attachedComparison(compareReply(), id, comparedTarget, operation, null));
  const bad = compareReply(); bad.comparison.changes[0].after.digest = 'wrong';
  assert.throws(() => attachedComparison(bad, id, operation, comparedTarget, null));
  const absent = compareReply(); absent.comparison.changes[0].before = null;
  assert.throws(() => attachedComparison(absent, id, operation, comparedTarget, null));
  const cursor = compareReply(); cursor.comparison.next_after = 'notes.txt';
  assert.throws(() => attachedComparison(cursor, id, operation, comparedTarget, null));
});
test('comparison previews retain both exact sides while capture and next-base selection change', async () => {
  let live = comparedTarget; const calls = [];
  const h = harness(async (command, args) => {
    calls.push({ command, args });
    if (command === 'attached_project_versions') return { schema: 'mesh.attachment-versions/v1', project: id,
      before: null, versions: [comparedTarget, operation], next_before: null };
    if (command === 'compare_attached_versions') return compareReply();
    if (command === 'inspect_attached_version') {
      const value = textReply();
      if (args.operation === comparedTarget) Object.assign(value.inspection, { operation: comparedTarget,
        digest: 'e'.repeat(64), executable: true, text: 'after' });
      return value;
    }
    const value = reply(); value.projects[0].capture.saved_version = live; return value;
  });
  await settle(); h.intent({ type: 'versions', id, before: null }); await settle();
  h.intent({ type: 'set-base', id, operation });
  h.intent({ type: 'compare', id, target: comparedTarget }); await settle();
  h.intent({ type: 'compare-file', id, base: operation, target: comparedTarget, path: 'notes.txt' }); await settle();
  live = 'f'.repeat(64); h.intent({ type: 'refresh' }); await settle();
  h.intent({ type: 'set-base', id, operation: comparedTarget });
  const projection = h.projections.at(-1);
  assert.equal(projection.bases[id], comparedTarget);
  assert.equal(projection.comparisons[id].base, operation);
  assert.equal(projection.comparisons[id].target, comparedTarget);
  assert.equal(projection.comparisons[id].file.before.text, 'saved');
  assert.equal(projection.comparisons[id].file.after.text, 'after');
  const count = calls.length;
  h.intent({ type: 'compare-file', id, base: operation, target: live, path: 'notes.txt' });
  h.intent({ type: 'inspect', id: '__proto__', operation });
  h.intent({ type: 'set-base', id: 'constructor', operation });
  assert.equal(calls.length, count); h.dispose();
});

async function comparisonHarness(overrides = {}) {
  const calls = [];
  const h = harness(async (command, args) => {
    calls.push({ command, args });
    if (overrides[command]) return overrides[command](args);
    if (command === 'attached_project_versions') return { schema: 'mesh.attachment-versions/v1', project: id,
      before: null, versions: ['f'.repeat(64), comparedTarget, operation], next_before: null };
    if (command === 'compare_attached_versions') {
      const value = compareReply();
      value.comparison.base = args.base; value.comparison.target = args.target;
      value.comparison.changes.push({ ...value.comparison.changes[0], path: 'other.txt' });
      value.comparison.total = 2; return value;
    }
    if (command === 'inspect_attached_version') {
      const value = textReply(); value.inspection.path = args.path;
      if (args.operation === comparedTarget) Object.assign(value.inspection, { operation: comparedTarget,
        digest: 'e'.repeat(64), executable: true, text: 'after' });
      return value;
    }
    return reply();
  });
  await settle(); h.intent({ type: 'versions', id, before: null }); await settle();
  h.intent({ type: 'set-base', id, operation });
  h.intent({ type: 'compare', id, target: comparedTarget }); await settle();
  return { ...h, calls, pin: () => h.intent({ type: 'pin-comparison', id, base: operation, target: comparedTarget }) };
}
test('two pins retain independent file selections when the active comparison and capture change', async () => {
  const h = await comparisonHarness(); h.pin(); h.pin();
  const [one, two] = h.projections.at(-1).pins;
  h.intent({ type: 'compare-file', id, base: operation, target: comparedTarget, path: 'notes.txt', pin: one.key }); await settle();
  h.intent({ type: 'compare-file', id, base: operation, target: comparedTarget, path: 'other.txt', pin: two.key }); await settle();
  h.intent({ type: 'set-base', id, operation: comparedTarget });
  h.intent({ type: 'compare', id, target: 'f'.repeat(64) }); await settle();
  h.intent({ type: 'refresh' }); await settle();
  const state = h.projections.at(-1);
  assert.equal(state.comparisons[id].target, 'f'.repeat(64));
  assert.equal(state.pins[0].comparison.target, comparedTarget);
  assert.equal(state.pins[0].comparison.file.path, 'notes.txt');
  assert.equal(state.pins[1].comparison.file.path, 'other.txt');
  assert.equal(state.pins[0].comparison.file.before.text, 'saved');
  assert.equal(state.pins[1].comparison.file.after.text, 'after');
  const count = h.calls.length;
  h.intent({ type: 'compare-file', id, base: comparedTarget, target: 'f'.repeat(64), path: 'notes.txt', pin: one.key });
  h.intent({ type: 'compare-file', id, base: operation, target: comparedTarget, path: 'notes.txt', pin: 'missing' });
  assert.equal(h.calls.length, count); h.dispose();
});
test('paging a pin leaves sibling and active comparison pages unchanged', async () => {
  const h = await comparisonHarness({ compare_attached_versions: (args) => {
    const value = compareReply(); value.comparison.after = args.after; value.comparison.total = 201;
    value.comparison.changes = args.after === null
      ? Array.from({ length: 200 }, (_, index) => ({ ...value.comparison.changes[0], path: `file-${String(index).padStart(3, '0')}` }))
      : [{ ...value.comparison.changes[0], path: 'file-200' }];
    value.comparison.next_after = args.after === null ? 'file-199' : null; return value;
  } });
  h.pin(); h.pin(); const [one, two] = h.projections.at(-1).pins;
  h.intent({ type: 'compare-page', id, base: operation, target: comparedTarget, after: 'file-199', pin: one.key }); await settle();
  const state = h.projections.at(-1);
  assert.equal(state.pins[0].comparison.changes[0].path, 'file-200');
  assert.equal(state.pins[1].key, two.key);
  assert.equal(state.pins[1].comparison.changes[0].path, 'file-000');
  assert.equal(state.comparisons[id].changes[0].path, 'file-000'); h.dispose();
});
test('closed pins cannot reappear after outstanding previews, and pin identities are never reused', async () => {
  const pending = [];
  const h = await comparisonHarness({ inspect_attached_version: (args) => new Promise((resolve) => pending.push({ args, resolve })) });
  for (let index = 0; index < 9; index += 1) h.pin();
  assert.equal(h.projections.at(-1).pins.length, 8);
  const closed = h.projections.at(-1).pins[0].key;
  h.intent({ type: 'compare-file', id, base: operation, target: comparedTarget, path: 'notes.txt', pin: closed });
  assert.equal(pending.length, 2);
  h.intent({ type: 'close-pin', pin: closed });
  for (const { args, resolve } of pending) {
    const value = textReply();
    if (args.operation === comparedTarget) Object.assign(value.inspection, { operation: comparedTarget,
      digest: 'e'.repeat(64), executable: true, text: 'after' });
    resolve(value);
  }
  await settle();
  assert.equal(h.projections.at(-1).pins.length, 7);
  assert.ok(h.projections.at(-1).pins.every((pin) => pin.key !== closed));
  h.pin(); assert.equal(h.projections.at(-1).pins.length, 8);
  assert.ok(h.projections.at(-1).pins.every((pin) => pin.key !== closed));
  const count = h.calls.length;
  h.intent({ type: 'compare-file', id, base: operation, target: comparedTarget, path: 'notes.txt', pin: closed });
  assert.equal(h.calls.length, count); h.dispose();
});

test('pins from different projects cannot be redirected through another project handle', async () => {
  const otherId = '9'.repeat(64); const calls = [];
  const h = harness(async (command, args) => {
    calls.push({ command, args });
    if (command === 'attached_project_versions') return { schema: 'mesh.attachment-versions/v1', project: args.id,
      before: null, versions: [comparedTarget, operation], next_before: null };
    if (command === 'compare_attached_versions') return { ...compareReply(), project: args.id };
    const value = reply(); value.projects.push({ ...value.projects[0], id: otherId, root: '/other/project' }); return value;
  });
  await settle();
  for (const project of [id, otherId]) {
    h.intent({ type: 'versions', id: project, before: null }); await settle();
    h.intent({ type: 'set-base', id: project, operation });
    h.intent({ type: 'compare', id: project, target: comparedTarget }); await settle();
    h.intent({ type: 'pin-comparison', id: project, base: operation, target: comparedTarget });
  }
  const pins = h.projections.at(-1).pins;
  assert.deepEqual(pins.map((pin) => pin.root), ['/original/project', '/other/project']);
  const count = calls.length;
  h.intent({ type: 'compare-file', id: otherId, base: operation, target: comparedTarget, path: 'notes.txt', pin: pins[0].key });
  assert.equal(calls.length, count); h.dispose();
});


test('restored stopped and unavailable projects preserve native recovery state without claiming current capture', () => {
  const value = reply();
  value.projects[0].recovery = 'restored-stopped';
  value.projects[0].capture.phase = 'stopped';
  value.projects[0].capture.last_outcome = 'pending';
  value.projects[0].capture.last_complete_capture_age_ms = null;
  const restored = attachedProjectList(value)[0];
  assert.equal(restored.recovery, 'restored-stopped');
  assert.equal(restored.savedVersion, 'b'.repeat(64));
  assert.equal(restored.captureAgeMs, null);
  value.projects[0].recovery = 'unavailable'; value.projects[0].capture.saved_version = null;
  assert.equal(attachedProjectList(value)[0].recovery, 'unavailable');
  value.projects[0].recovery = 'silently-running';
  assert.throws(() => attachedProjectList(value));
});

test('restart revalidates selected file outside restored page and retains unavailable pins', async () => {
  const selectors = [
    { key: '12', project: id, base: operation, target: comparedTarget, after: 'notes.txt', path: 'notes.txt' },
    { key: '13', project: 'f'.repeat(64), base: operation, target: comparedTarget, after: null, path: null },
  ];
  const calls = [];
  const h = harness(async (command, args) => {
    calls.push({ command, args });
    if (command === 'attached_projects') return reply();
    if (command === 'compare_attached_versions') {
      const value = compareReply(); value.comparison.after = 'notes.txt'; value.comparison.changes = []; return value;
    }
    if (command === 'compare_attached_path') {
      const value = compareReply(); value.comparison.changes = value.comparison.changes.filter((change) => change.path === 'notes.txt');
      value.comparison.total = 1; return value;
    }
    if (command === 'inspect_attached_version') {
      const value = textReply();
      if (args.operation === comparedTarget) Object.assign(value.inspection, { operation: comparedTarget, digest: 'e'.repeat(64), bytes: 5, executable: true, text: 'after' });
      return value;
    }
    throw new Error('unexpected command');
  }, async () => ({ schema: 'mesh.desktop-pin-selectors/v1', revision: '4', pins: selectors }));
  await settle();
  const pins = h.projections.at(-1).pins;
  assert.equal(pins.length, 2);
  assert.equal(pins[0].comparison.after, 'notes.txt');
  assert.equal(pins[0].comparison.file.path, 'notes.txt');
  assert.equal(pins[0].comparison.file.after.text, 'after');
  assert.equal(pins[1].comparison, null);
  assert.deepEqual(pins[1].selector, selectors[1]);
  assert.ok(calls.some((call) => call.command === 'compare_attached_path'));
  assert.equal(h.projections.at(-1).pinStatus, 'saved');
  h.dispose();
});

test('detached projects retain history identity and send only explicit generation-bound lifecycle controls', async () => {
  const calls = [];
  const value = reply(); value.projects[0].detached = true;
  assert.equal(attachedProjectList(value)[0].detached, true);
  const invalid = reply(); invalid.projects[0].detached = 'yes';
  assert.throws(() => attachedProjectList(invalid));
  const h = harness(async (command, args) => { calls.push({ command, args }); return value; });
  await settle();
  assert.equal(h.projections.at(-1).projects[0].savedVersion, 'b'.repeat(64));
  h.intent({ type: 'control', id, generation: '1', action: 'reattach' }); await settle();
  assert.deepEqual(calls[1], { command: 'control_attached_project', args: { id, generation: '1', action: 'reattach' } });
  const count = calls.length;
  h.intent({ type: 'control', id, generation: '0', action: 'detach' }); await settle();
  assert.equal(calls.length, count);
  h.dispose();
});

test('native event availability is explicit and older status falls back to periodic checks', () => {
  assert.equal(attachedProjectList(reply())[0].nativeEvents, false);
  const value = reply(); value.projects[0].capture.native_events = true;
  assert.equal(attachedProjectList(value)[0].nativeEvents, true);
  value.projects[0].capture.native_events = 'active';
  assert.throws(() => attachedProjectList(value));
});
