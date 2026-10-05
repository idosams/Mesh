// Separate-process native harness proof. Retain the fixture and never open the graphical app.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawn } from 'node:child_process';
import { readFile, writeFile, rename } from 'node:fs/promises';
import { isAbsolute, join } from 'node:path';
import { EventEmitter } from 'node:events';
import { verifyBuildIdentity } from './build-identity-proof.mjs';

const options = {};
for (let i = 2; i < process.argv.length; i += 2) {
  const key = process.argv[i]; const value = process.argv[i + 1];
  assert.ok(['--fixture', '--executable', '--revision'].includes(key) && value && !(key in options), 'supply each required argument once');
  options[key] = value;
}
assert.equal(Object.keys(options).length, 3);
const executable = options['--executable'];
assert.ok(isAbsolute(executable) && isAbsolute(options['--fixture']));
const fixture = JSON.parse(await readFile(options['--fixture'], 'utf8'));
assert.equal(fixture.schema, 'mesh.registered-capture-fixture/v1');
for (const field of ['root', 'storage', 'project', 'owner', 'journal']) assert.ok(isAbsolute(fixture[field]));
assert.match(fixture.registration, /^[0-9a-f]{64}$/);
const args = action => ['--mesh-registered-attachment', action, fixture.storage, fixture.registration];
const children = new Set();
const started = performance.now();
function launch(arguments_) {
  const child = spawn(executable, arguments_, { stdio: ['pipe', 'pipe', 'pipe'] });
  children.add(child);
  child.once('close', () => children.delete(child));
  return child;
}
async function run(arguments_, expected = 0) {
  const child = launch(arguments_); child.stdin.end();
  let stdout = ''; let stderr = ''; let oversized = false;
  const append = (current, chunk) => {
    const next = current + chunk;
    if (next.length > 1_048_576) { oversized = true; child.kill('SIGKILL'); return current; }
    return next;
  };
  child.stdout.on('data', chunk => { stdout = append(stdout, chunk); });
  child.stderr.on('data', chunk => { stderr = append(stderr, chunk); });
  const timer = setTimeout(() => child.kill('SIGKILL'), 60_000);
  try {
    const code = await new Promise((done, fail) => { child.once('error', fail); child.once('close', done); });
    assert.equal(oversized, false, 'oversized command output');
    assert.equal(code, expected, `unexpected command exit: ${stderr}`);
    if (expected !== 0) assert.equal(stdout, '', 'refusal must not emit a saved acknowledgement');
    return stdout;
  } finally { clearTimeout(timer); }
}
function watch() {
  const child = launch(args('watch')); const events = []; const updates = new EventEmitter();
  let pending = ''; let stderr = ''; let ended = false; let problem;
  const deadline = setTimeout(() => child.kill('SIGKILL'), 120_000);
  child.stdout.on('data', chunk => {
    pending += chunk;
    if (pending.length > 262144 || events.length > 1024) { problem = new Error('oversized watch output'); child.kill('SIGKILL'); }
    while (pending.includes('\n') && !problem) {
      const at = pending.indexOf('\n'); const line = pending.slice(0, at); pending = pending.slice(at + 1);
      try { events.push(JSON.parse(line)); } catch { problem = new Error('invalid watch JSON'); child.kill('SIGKILL'); }
    }
    updates.emit('update');
  });
  child.stderr.on('data', chunk => { stderr += chunk; if (stderr.length > 262144) child.kill('SIGKILL'); });
  const closed = new Promise((done, fail) => {
    child.once('error', fail);
    child.once('close', code => { clearTimeout(deadline); ended = true; updates.emit('update'); done(code); });
  });
  const next = predicate => new Promise((done, fail) => {
    const cleanup = () => { clearTimeout(timer); updates.off('update', inspect); };
    const reject = error => { cleanup(); fail(error); };
    const inspect = () => {
      if (problem) return reject(problem);
      const found = events.find(predicate);
      if (found) { cleanup(); done(found); }
      else if (ended) reject(new Error(`watch closed before expected state: ${stderr}`));
    };
    const timer = setTimeout(() => reject(new Error('watch status deadline exceeded')), 60_000);
    updates.on('update', inspect); inspect();
  });
  return { child, events, next, closed };
}
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const list = async () => JSON.parse(await run(args('versions'))).versions;
let restoreOwner = false;
const offline = `${fixture.owner}-executable-proof-offline`;
try {
  const beforeExecutable = digest(await readFile(executable));
  verifyBuildIdentity(await run(['--mesh-build-identity']), options['--revision']);
  const original = await readFile(join(fixture.owner, 'note.txt'));
  const before = await list(); assert.equal(before.length, 4);
  assert.equal(JSON.parse(await run(args('capture'))).saved_version, before.at(-1));
  assert.deepEqual(await list(), before, 'unchanged process capture appended history');
  await writeFile(join(fixture.project, 'note.txt'), 'executable capture progress\n');
  const saved = JSON.parse(await run(args('capture'))).saved_version;
  assert.deepEqual(await list(), [...before, saved]);
  const watcher = watch();
  await watcher.next(value => value.phase === 'waiting' && value.saved_version === saved);
  await writeFile(join(fixture.project, 'note.txt'), 'executable watched progress\n');
  const watched = await watcher.next(value => value.last_outcome === 'saved' && value.saved_version && value.saved_version !== saved);
  watcher.child.stdin.end('stop\n');
  assert.equal(await watcher.closed, 0);
  assert.equal(watcher.events.at(-1).phase, 'stopped');
  const after = [...before, saved, watched.saved_version];
  assert.deepEqual(await list(), after, 'fresh process must see exact watched progress');
  const journal = await readFile(fixture.journal);
  await rename(fixture.owner, offline); restoreOwner = true;
  await run(args('capture'), 1); await run(args('versions'), 1);
  await rename(offline, fixture.owner); restoreOwner = false;
  assert.deepEqual(await list(), after, 'refused capture changed acknowledged history');
  assert.deepEqual(await readFile(fixture.journal), journal, 'refusal changed journal bytes');
  await run(['--mesh-registered-attachment', 'versions', fixture.storage, '0'.repeat(64)], 1);
  assert.deepEqual(await readFile(join(fixture.owner, 'note.txt')), original);
  assert.equal(digest(await readFile(executable)), beforeExecutable);
  process.stdout.write(`${JSON.stringify({ proof: 'mesh-registered-capture-process/v1', passed: true, revision: options['--revision'], executable_sha256: beforeExecutable, versions: after.length, elapsed_ms: Math.round(performance.now() - started), graphical: false, provider: false, packaged: false })}\n`);
} finally {
  for (const child of children) child.kill('SIGKILL');
  if (restoreOwner) await rename(offline, fixture.owner);
}
