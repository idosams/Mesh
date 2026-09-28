import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtempSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { attachedProjectList, attachedVersionPage, attachedEntries, attachedText, attachedComparison, attachedReview, attachedReviews, attachedMain, attachedApproval, attachedIntegration, attachedRecovery, attachedFileChange, startAttachedProjects } from './attached-projects.js';
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

const reviewRecord = () => ({ bundle: '6'.repeat(64), target: operation, reviewed_head: '7'.repeat(64), presentation: '8'.repeat(64),
  complete: true, unavailable: null, changes: [{ before: null, after: 'notes.txt', effect: 'added' }],
  changes_not_listed: 0, operations_not_listed: 0, author_attribution: 'unknown', approval_authority: false });
const reviewReply = () => ({ schema: 'mesh.desktop-attachment-review/v1', project: id, review: reviewRecord() });
const reviewQueueReply = () => ({ schema: 'mesh.desktop-attachment-reviews/v1', project: id, queue: { reviews: [reviewRecord()], not_listed: 0 } });
test('review projections bind exact identities and reject invented approval or completeness', () => {
  assert.equal(attachedReview(reviewReply(), id, operation).bundle, '6'.repeat(64));
  assert.equal(attachedReviews(reviewQueueReply(), id).reviews.length, 1);
  assert.throws(() => attachedReview(reviewReply(), id, comparedTarget));
  assert.throws(() => attachedReview(reviewReply(), 'f'.repeat(64), operation));
  assert.throws(() => attachedReview(reviewReply(), id, operation, 'f'.repeat(64)));
  for (const mutate of [
    (review) => { review.approval_authority = true; },
    (review) => { review.author_attribution = 'agent'; },
    (review) => { review.changes_not_listed = 1; },
    (review) => { review.changes[0].after = '../outside'; },
    (review) => { review.presentation = null; },
  ]) {
    const value = reviewReply(); mutate(value.review);
    assert.throws(() => attachedReview(value, id, operation));
  }
  const duplicate = reviewQueueReply(); duplicate.queue.reviews.push(reviewRecord());
  assert.throws(() => attachedReviews(duplicate, id));
});

test('requested review stays on its saved target while capture advances and opens exact saved files', async () => {
  const calls = []; let latest = operation;
  const h = harness(async (command, args) => {
    calls.push({ command, args });
    if (command === 'attached_project_versions') return { schema: 'mesh.attachment-versions/v1', project: id, before: null, versions: [operation], next_before: null };
    if (command === 'request_attached_review' || command === 'inspect_attached_review') return reviewReply();
    if (command === 'attached_project_reviews') return reviewQueueReply();
    if (command === 'inspect_attached_version') return entryReply();
    const value = reply(); value.projects[0].capture.saved_version = latest; return value;
  });
  await settle(); h.intent({ type: 'versions', id, before: null }); await settle();
  h.intent({ type: 'request-review', id, target: operation }); await settle();
  assert.equal(h.projections.at(-1).selectedReviews[id].target, operation);
  latest = comparedTarget; h.intent({ type: 'refresh' }); await settle();
  assert.equal(h.projections.at(-1).selectedReviews[id].target, operation);
  h.intent({ type: 'review-files', id, bundle: '6'.repeat(64), target: operation }); await settle();
  assert.equal(h.projections.at(-1).inspections[id].operation, operation);
  const count = calls.length;
  h.intent({ type: 'review-files', id, bundle: '6'.repeat(64), target: comparedTarget });
  h.intent({ type: 'request-review', id, target: comparedTarget });
  h.intent({ type: 'reviews', unrelated: true });
  assert.equal(calls.length, count);
  h.dispose();
});

test('reopened review queue can select a durable request outside the loaded version page', async () => {
  const h = harness(async (command) => {
    if (command === 'attached_project_reviews') return reviewQueueReply();
    if (command === 'inspect_attached_review') return reviewReply();
    return reply();
  });
  await settle(); h.intent({ type: 'reviews', id }); await settle();
  h.intent({ type: 'open-review', id, bundle: '6'.repeat(64), target: operation }); await settle();
  assert.equal(h.projections.at(-1).selectedReviews[id].presentation, '8'.repeat(64));
  h.dispose();
});

const mainReply = (main = null) => ({ schema: 'mesh.desktop-attachment-main/v1', project: id,
  credential: { available: true, enrolled: true, unavailable_reason: null }, main_available: true, main });
const acceptedMain = () => ({ head: reviewRecord().reviewed_head, bundle: reviewRecord().bundle, target: operation });
const approvalReply = () => ({ schema: 'mesh.desktop-attachment-approval/v1', project: id, ...acceptedMain() });
test('main and approval results bind project, bundle, target, head and truthful availability', () => {
  assert.equal(attachedMain(mainReply(), id).main, null);
  assert.deepEqual(attachedMain(mainReply(acceptedMain()), id).main, acceptedMain());
  for (const mutate of [
    value => { value.project = 'f'.repeat(64); },
    value => { value.main_available = false; },
    value => { value.main.head = 'unknown'; },
    value => { value.main.target = null; },
    value => { value.credential.available = false; },
    value => { value.credential.unavailable_reason = 'invented'; },
  ]) { const value = mainReply(acceptedMain()); mutate(value); assert.throws(() => attachedMain(value, id)); }
  const unavailable = mainReply(); unavailable.main_available = false;
  unavailable.credential = { available: false, enrolled: false, unavailable_reason: 'Validated application identity required' };
  assert.equal(attachedMain(unavailable, id).mainAvailable, false);
  assert.equal(attachedMain(unavailable, id).available, false);
  assert.equal(attachedApproval(approvalReply(), id, reviewRecord()).head, acceptedMain().head);
  for (const field of ['project', 'bundle', 'target', 'head']) {
    const value = approvalReply(); value[field] = 'f'.repeat(64);
    assert.throws(() => attachedApproval(value, id, reviewRecord()));
  }
});
async function selectReview(h) {
  await settle(); h.intent({ type: 'reviews', id }); await settle();
  h.intent({ type: 'open-review', id, bundle: reviewRecord().bundle, target: operation }); await settle();
}
test('only a selected verified review invokes native approval and capture does not replace its identity', async () => {
  let main = null; let finish; const calls = [];
  const h = harness(async (command, args) => {
    calls.push({ command, args });
    if (command === 'attached_project_reviews') return reviewQueueReply();
    if (command === 'inspect_attached_review') return reviewReply();
    if (command === 'attachment_approval_status') return mainReply(main);
    if (command === 'approve_attached_review') return new Promise(resolve => { finish = () => { main = acceptedMain(); resolve(approvalReply()); }; });
    const value = reply(); value.projects[0].capture.saved_version = comparedTarget; return value;
  });
  await selectReview(h);
  h.intent({ type: 'approve-review', id, bundle: reviewRecord().bundle, target: operation }); await settle();
  assert.equal(calls.some(call => call.command === 'approve_attached_review'), false);
  h.intent({ type: 'check-approval', id }); await settle();
  h.intent({ type: 'approve-review', id, bundle: reviewRecord().bundle, target: comparedTarget });
  h.intent({ type: 'approve-review', id, bundle: reviewRecord().bundle, target: operation, receipt: 'renderer-supplied' });
  assert.equal(calls.some(call => call.command === 'approve_attached_review'), false);
  h.intent({ type: 'approve-review', id, bundle: reviewRecord().bundle, target: operation }); await settle();
  assert.deepEqual(calls.at(-1), { command: 'approve_attached_review', args: { id, bundle: reviewRecord().bundle, target: operation } });
  h.intent({ type: 'approve-review', id, bundle: reviewRecord().bundle, target: operation });
  assert.equal(calls.filter(call => call.command === 'approve_attached_review').length, 1);
  assert.equal(h.projections.at(-1).busy, true);
  finish(); await settle();
  assert.equal(h.projections.at(-1).selectedReviews[id].target, operation);
  assert.equal(h.projections.at(-1).projects[0].savedVersion, comparedTarget);
  assert.equal(h.projections.at(-1).approvalStates[id].main.target, operation);
  assert.match(h.projections.at(-1).approvalFeedback[id], /confirmed/);
  h.intent({ type: 'approve-review', id, bundle: reviewRecord().bundle, target: operation });
  assert.equal(calls.filter(call => call.command === 'approve_attached_review').length, 1);
  h.dispose();
});
test('lost approval reply refreshes main without claiming rollback and main opens outside the queue', async () => {
  let main = null;
  const h = harness(async command => {
    if (command === 'attached_project_reviews') return reviewQueueReply();
    if (command === 'inspect_attached_review') return reviewReply();
    if (command === 'attachment_approval_status') return mainReply(main);
    if (command === 'approve_attached_review') { main = acceptedMain(); throw new Error('lost reply'); }
    return reply();
  });
  await selectReview(h); h.intent({ type: 'check-approval', id }); await settle();
  h.intent({ type: 'approve-review', id, bundle: reviewRecord().bundle, target: operation }); await settle();
  assert.equal(h.projections.at(-1).approvalStates[id].main.head, acceptedMain().head);
  assert.match(h.projections.at(-1).approvalFeedback[id], /not confirmed/);
  assert.doesNotMatch(h.projections.at(-1).approvalFeedback[id], /unchanged|rolled back/);
  h.dispose();
  const fresh = harness(async command => {
    if (command === 'attachment_approval_status') return mainReply(acceptedMain());
    if (command === 'inspect_attached_review') return reviewReply();
    return reply();
  });
  await settle(); fresh.intent({ type: 'check-approval', id }); await settle();
  fresh.intent({ type: 'open-main', id }); await settle();
  assert.equal(fresh.projections.at(-1).selectedReviews[id].bundle, acceptedMain().bundle);
  assert.equal(fresh.projections.at(-1).reviewQueues[id], undefined);
  fresh.dispose();
});
test('unavailable approval blocks enrollment and signing while verified main remains readable', async () => {
  const calls = [];
  const unavailable = mainReply(acceptedMain());
  unavailable.credential = { available: false, enrolled: false, unavailable_reason: 'Validated application identity required' };
  const h = harness(async command => {
    calls.push(command);
    if (command === 'attachment_approval_status') return unavailable;
    if (command === 'attached_project_reviews') return reviewQueueReply();
    if (command === 'inspect_attached_review') return reviewReply();
    return reply();
  });
  await selectReview(h); h.intent({ type: 'check-approval', id }); await settle();
  h.intent({ type: 'enroll-approval', id });
  h.intent({ type: 'approve-review', id, bundle: reviewRecord().bundle, target: operation });
  assert.equal(calls.includes('enroll_approval_credential'), false);
  assert.equal(calls.includes('approve_attached_review'), false);
  h.intent({ type: 'open-main', id }); await settle();
  assert.equal(h.projections.at(-1).selectedReviews[id].target, operation);
  h.dispose();
});
test('enrollment requires checked availability and rereads native status after setup', async () => {
  let enrolled = false; const calls = [];
  const h = harness(async command => {
    calls.push(command);
    if (command === 'attachment_approval_status') { const value = mainReply(); value.credential.enrolled = enrolled; return value; }
    if (command === 'enroll_approval_credential') { enrolled = true; return {}; }
    return reply();
  });
  await settle(); h.intent({ type: 'enroll-approval', id }); await settle();
  assert.equal(enrolled, false);
  h.intent({ type: 'check-approval', id }); await settle();
  h.intent({ type: 'enroll-approval', id }); await settle();
  assert.equal(h.projections.at(-1).approvalStates[id].enrolled, true);
  assert.equal(calls.filter(command => command === 'enroll_approval_credential').length, 1);
  h.dispose();
});

test('uncertain approval plus failed refresh clears authority until a new status check succeeds', async () => {
  let statusReadable = true; let approvals = 0;
  const h = harness(async command => {
    if (command === 'attached_project_reviews') return reviewQueueReply();
    if (command === 'inspect_attached_review') return reviewReply();
    if (command === 'attachment_approval_status') {
      if (!statusReadable) throw new Error('status unavailable');
      return mainReply();
    }
    if (command === 'approve_attached_review') {
      approvals++; statusReadable = false; throw new Error('reply lost');
    }
    return reply();
  });
  await selectReview(h); h.intent({ type: 'check-approval', id }); await settle();
  const intent = { type: 'approve-review', id, bundle: reviewRecord().bundle, target: operation };
  h.intent(intent); await settle();
  assert.equal(approvals, 1);
  assert.equal(h.projections.at(-1).approvalStates[id], undefined);
  assert.match(h.projections.at(-1).approvalFeedback[id], /Refresh Mesh main/);
  h.intent(intent); await settle(); assert.equal(approvals, 1);
  statusReadable = true;
  h.intent(intent); await settle(); assert.equal(approvals, 1);
  h.intent({ type: 'check-approval', id }); await settle();
  assert.equal(h.projections.at(-1).approvalStates[id].available, true);
  h.intent(intent); await settle(); assert.equal(approvals, 2);
  h.dispose();
});

const integrationReply = () => {
  const file = { kind: 'file', bytes: 4, digest: '1'.repeat(64), executable: false };
  return { schema: 'mesh.desktop-attachment-integration/v1', project: id, preview: {
    schema: 'mesh.attachment-integration-preview/v1', ...acceptedMain(), base_head: '2'.repeat(64), observed_digest: '3'.repeat(64),
    atomic_snapshot: false, write_authority: false, matches_base: 1, already_present: 0, preserve_current: 0, conflicts: 0, blocked: 0, not_listed: 0,
    entries: [{ path: 'work.txt', status: 'matches-base', reason: null, base: file, current: file, target: { ...file, digest: '4'.repeat(64) } }],
  } };
};
test('working-folder previews bind exact main, bounded counts and explicit read-only authority', () => {
  assert.equal(attachedIntegration(integrationReply(), id, acceptedMain()).matches_base, 1);
  for (const mutate of [
    value => { value.project = 'f'.repeat(64); },
    value => { value.preview.head = 'f'.repeat(64); },
    value => { value.preview.bundle = 'f'.repeat(64); },
    value => { value.preview.target = comparedTarget; },
    value => { value.preview.base_head = null; },
    value => { value.preview.write_authority = true; },
    value => { value.preview.atomic_snapshot = true; },
    value => { value.preview.matches_base = 0; },
    value => { value.preview.not_listed = 1; },
    value => { value.preview.entries[0].path = '../outside'; },
    value => { value.preview.entries[0].status = 'safe-to-write'; },
    value => { value.preview.entries[0].current.digest = 'unverified'; },
    value => { value.preview.entries.push(value.preview.entries[0]); },
  ]) { const value = integrationReply(); mutate(value); assert.throws(() => attachedIntegration(value, id, acceptedMain())); }
});
test('working-folder comparison stays pinned during capture and failed refresh retains the prior observation', async () => {
  let fail = false; const calls = [];
  const h = harness(async (command, args) => {
    calls.push({ command, args });
    if (command === 'attachment_approval_status') return mainReply(acceptedMain());
    if (command === 'preview_attached_main_integration') {
      if (fail) throw new Error('private diagnostic');
      return integrationReply();
    }
    return reply();
  });
  await settle(); h.intent({ type: 'compare-main', id }); await settle();
  assert.equal(calls.some(call => call.command === 'preview_attached_main_integration'), false);
  h.intent({ type: 'check-approval', id }); await settle();
  h.intent({ type: 'compare-main', id }); await settle();
  const call = calls.find(call => call.command === 'preview_attached_main_integration');
  assert.deepEqual(call.args, { id, bundle: acceptedMain().bundle, target: operation });
  const pinned = h.projections.at(-1).integrationPreviews[id];
  h.intent({ type: 'refresh' }); await settle();
  assert.equal(h.projections.at(-1).integrationPreviews[id], pinned);
  fail = true; h.intent({ type: 'compare-main', id }); await settle();
  assert.equal(h.projections.at(-1).integrationPreviews[id], pinned);
  assert.match(h.projections.at(-1).integrationErrors[id], /previous observation is retained/);
  assert.doesNotMatch(h.projections.at(-1).integrationErrors[id], /private diagnostic/);
  const before = calls.length;
  h.intent({ type: 'apply-main', id });
  h.intent({ type: 'compare-main', id, path: '/outside' });
  assert.equal(calls.length, before);
  h.dispose();
});

test('native signal lifecycle remains distinct from capture and rejects contradictory activity', () => {
  for (const state of ['disabled', 'starting', 'active', 'unavailable', 'stopping', 'stopped']) {
    const value = reply();
    value.projects[0].capture.native_signal_state = state;
    value.projects[0].capture.native_events = state === 'active';
    assert.equal(attachedProjectList(value)[0].nativeSignalState, state);
    value.projects[0].capture.native_events = state !== 'active';
    assert.throws(() => attachedProjectList(value));
  }
  const value = reply(); value.projects[0].capture.native_signal_state = 'invented';
  assert.throws(() => attachedProjectList(value));
  assert.equal(attachedProjectList(reply())[0].nativeSignalState, 'unavailable');
});

test('stopped captures cannot claim an active native signal stream', () => {
  for (const phase of ['stopping', 'stopped', 'failed']) {
    const value = reply(); Object.assign(value.projects[0].capture, { phase, native_events: true, native_signal_state: 'active' });
    assert.throws(() => attachedProjectList(value));
  }
});

const transaction = `integration-${'a'.repeat(32)}`;
const restoredTransaction = `restoration-${'b'.repeat(32)}`;
function recoveryReply(selected = transaction) {
  const observation = { installation: 'native-file', digest: 'a'.repeat(64), mode: 0o100644, bytes: 12, native_metadata_digest: 'c'.repeat(64) };
  return { schema: 'mesh.desktop-attachment-recovery/v1', project: id, recovery: {
    schema: 'mesh.attachment-integration-recovery/v1', project: id, automatic_replay: false, write_authority: false,
    more: false, live_content_budget_remaining: 100, entries: [{ transaction: selected, status: 'applied-arrangement',
      attention_required: false, observation_final: false, atomic_snapshot: false, write_authority: false, automatic_replay: false, cleanup_authority: false,
      details: { path: 'work.txt', operation: selected.startsWith('restoration-') ? 'restore-retained' : 'apply-approved',
        content_is_approved_main: !selected.startsWith('restoration-'), approved_head: 'd'.repeat(64), is_current_main: false,
        current_exclusions_checked: false, recorded_outcome: 'applied-observed', source: observation, retained: observation },
    }],
  } };
}
function changeReply(restoration = false) {
  return { schema: 'mesh.desktop-attachment-file-change/v1', project: id, transaction: restoration ? restoredTransaction : transaction,
    outcome: { schema: restoration ? 'mesh.attachment-file-restoration-result/v1' : 'mesh.attachment-file-integration-result/v1',
      proposal_digest: 'e'.repeat(64), status: 'applied-observed', displaced_file_retained: true, observation_final: false } };
}
test('recovery observations bind exact project and transaction without granting write or cleanup authority', () => {
  assert.equal(attachedRecovery(recoveryReply(), id).entries[0].retainedAvailable, true);
  assert.equal(attachedRecovery({ schema: 'mesh.desktop-attachment-recovery/v1', project: id, recovery: null }, id).entries.length, 0);
  assert.throws(() => attachedRecovery(recoveryReply(), id, restoredTransaction));
  for (const mutate of [
    value => { value.project = 'f'.repeat(64); },
    value => { value.recovery.project = 'f'.repeat(64); },
    value => { value.recovery.write_authority = true; },
    value => { value.recovery.entries[0].cleanup_authority = true; },
    value => { value.recovery.entries[0].observation_final = true; },
    value => { value.recovery.entries[0].status = 'safe-to-overwrite'; },
    value => { value.recovery.entries[0].details.path = '../outside'; },
    value => { value.recovery.entries[0].details.retained = undefined; },
    value => { value.recovery.entries[0].details.operation = 'restore-retained'; },
    value => { value.recovery.entries.push(value.recovery.entries[0]); },
  ]) { const value = recoveryReply(); mutate(value); assert.throws(() => attachedRecovery(value, id)); }
  const prepared = recoveryReply(); prepared.recovery.entries[0].status = 'prepared-arrangement';
  assert.equal(attachedRecovery(prepared, id).entries[0].retainedAvailable, false);
  assert.equal(attachedFileChange(changeReply(), id, false).transaction, transaction);
  assert.throws(() => attachedFileChange(changeReply(), id, true));
  for (const mutate of [
    value => { value.outcome.displaced_file_retained = false; },
    value => { value.outcome.observation_final = true; },
    value => { value.outcome.status = 'rolled-back'; },
    value => { value.transaction = '../outside'; },
  ]) { const value = changeReply(); mutate(value); assert.throws(() => attachedFileChange(value, id, false)); }
});
test('retained-file restoration selects native identifiers, rejects injected paths and refreshes uncertain results without replay', async () => {
  let fail = false; const calls = [];
  const h = harness(async (command, args) => {
    calls.push({ command, args });
    if (command === 'inspect_attached_recovery') return recoveryReply(args.transaction ?? transaction);
    if (command === 'restore_attached_retained_file') {
      if (fail) throw new Error('private native diagnostic');
      return changeReply(true);
    }
    return reply();
  });
  await settle();
  h.intent({ type: 'restore-retained', id, transaction }); await settle();
  assert.equal(calls.some(call => call.command === 'restore_attached_retained_file'), false);
  h.intent({ type: 'recovery', id }); await settle();
  const count = calls.length;
  h.intent({ type: 'restore-retained', id, transaction, path: '/outside' });
  h.intent({ type: 'lookup-recovery', id, transaction: '../outside' });
  assert.equal(calls.length, count);
  h.intent({ type: 'restore-retained', id, transaction }); await settle();
  assert.deepEqual(calls.find(call => call.command === 'restore_attached_retained_file').args, { id, transaction });
  assert.equal(h.projections.at(-1).selectedRecovery[id].transaction, restoredTransaction);
  assert.match(h.projections.at(-1).fileChangeFeedback[id], /observed/);
  fail = true;
  h.intent({ type: 'restore-retained', id, transaction: restoredTransaction }); await settle();
  assert.match(h.projections.at(-1).fileChangeFeedback[id], /does not prove/);
  assert.doesNotMatch(h.projections.at(-1).fileChangeFeedback[id], /private native/);
  assert.equal(calls.filter(call => call.command === 'restore_attached_retained_file').length, 2);
  const pinned = h.projections.at(-1).selectedRecovery[id];
  h.intent({ type: 'refresh' }); await settle();
  assert.equal(h.projections.at(-1).selectedRecovery[id], pinned);
  h.dispose();
});
test('applying main requires an exact loaded file comparison and sends no renderer content or recovery location', async () => {
  const calls = []; let inspectFailure = false;
  const h = harness(async (command, args) => {
    calls.push({ command, args });
    if (command === 'attachment_approval_status') return mainReply(acceptedMain());
    if (command === 'preview_attached_main_integration') return integrationReply();
    if (command === 'apply_attached_main_file') return changeReply();
    if (command === 'inspect_attached_recovery') { if (inspectFailure) throw new Error('private'); return recoveryReply(); }
    return reply();
  });
  await settle(); h.intent({ type: 'check-approval', id }); await settle();
  h.intent({ type: 'apply-main-file', id, path: 'work.txt' }); await settle();
  assert.equal(calls.some(call => call.command === 'apply_attached_main_file'), false);
  h.intent({ type: 'compare-main', id }); await settle();
  const count = calls.length;
  h.intent({ type: 'apply-main-file', id, path: '../other' });
  h.intent({ type: 'apply-main-file', id, path: 'work.txt', bytes: 'injected' });
  assert.equal(calls.length, count);
  h.intent({ type: 'apply-main-file', id, path: 'work.txt' }); await settle();
  assert.deepEqual(calls.find(call => call.command === 'apply_attached_main_file').args,
    { id, bundle: acceptedMain().bundle, target: operation, path: 'work.txt' });
  const previous = h.projections.at(-1).recoveries[id];
  inspectFailure = true; h.intent({ type: 'recovery', id }); await settle();
  assert.equal(h.projections.at(-1).recoveries[id], previous);
  assert.match(h.projections.at(-1).recoveryErrors[id], /out of date/);
  const after = calls.length;
  h.intent({ type: 'restore-retained', id, transaction }); await settle();
  assert.equal(calls.length, after);
  h.dispose();
});


test('recovery accepts native regular-file modes and refuses permission-only or non-file modes', () => {
  const root = mkdtempSync(join(tmpdir(), 'mesh-recovery-mode-'));
  try {
    const path = join(root, 'work.txt');
    writeFileSync(path, 'fixture', { mode: 0o600 });
    const mode = statSync(path).mode;
    assert.equal(mode & 0o170000, 0o100000);
    const value = recoveryReply();
    value.recovery.entries[0].details.source.mode = mode;
    value.recovery.entries[0].details.retained.mode = mode;
    assert.equal(attachedRecovery(value, id).entries[0].retainedAvailable, true);
    for (const bad of [0o600, 0o040755, 0o120777, 0o110644, -1, 1.5, Number.MAX_SAFE_INTEGER]) {
      const invalid = recoveryReply(); invalid.recovery.entries[0].details.retained.mode = bad;
      assert.throws(() => attachedRecovery(invalid, id));
    }
  } finally { rmSync(root, { recursive: true, force: true }); }
});
