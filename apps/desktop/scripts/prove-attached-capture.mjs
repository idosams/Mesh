// Isolated executable proof. Packaged mode verifies the exact sealed bundle; it never opens a GUI.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { captureProofArguments } from './attached-capture-proof-args.mjs';
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, writeFile, readFile, stat, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { EventEmitter } from 'node:events';

const root = resolve(fileURLToPath(new URL('../../..', import.meta.url)));
const options = captureProofArguments(process.argv.slice(2));
const app = options.app === null ? null : resolve(options.app);
const desktop = app === null ? join(root, 'target/debug/mesh-desktop') : join(app, 'Contents/MacOS/mesh-desktop');
const meshctl = join(root, 'target/debug/meshctl');
const fixture = await mkdtemp(join(tmpdir(), 'mesh-attached-capture-proof-'));
const project = join(fixture, 'project');
const metadata = join(fixture, 'metadata');
const children = new Set();
const started = performance.now();

function launch(command, args) {
  const child = spawn(command, args, { cwd: root, stdio: ['pipe', 'pipe', 'pipe'] });
  children.add(child);
  child.once('close', () => children.delete(child));
  return child;
}
async function run(command, args, expected = 0) {
  const child = launch(command, args);
  child.stdin.end();
  let stdout = ''; let stderr = '';
  child.stdout.on('data', chunk => { stdout += chunk; });
  child.stderr.on('data', chunk => { stderr += chunk; });
  const timer = setTimeout(() => child.kill('SIGKILL'), 30_000);
  try {
    const code = await new Promise((done, fail) => { child.once('error', fail); child.once('close', done); });
    assert.equal(code, expected, `unexpected process exit: ${stderr}`);
    return stdout;
  } finally { clearTimeout(timer); }
}
function watch() {
  const child = launch(desktop, ['--mesh-attachment', 'watch', metadata]);
  const deadline = setTimeout(() => child.kill('SIGKILL'), 30_000);
  child.once('close', () => clearTimeout(deadline));
  const emitter = new EventEmitter(); const events = [];
  let pending = ''; let stderr = ''; let ended = false;
  const failure = problem => emitter.emit('failure', problem);
  child.stdout.on('data', chunk => {
    pending += chunk;
    if (pending.length > 262144) { child.kill('SIGKILL'); failure(new Error('oversized status output')); return; }
    while (pending.includes('\n')) {
      const boundary = pending.indexOf('\n'); const line = pending.slice(0, boundary); pending = pending.slice(boundary + 1);
      try { events.push(JSON.parse(line)); emitter.emit('update'); }
      catch { failure(new Error('invalid native status JSON')); }
    }
  });
  child.stderr.on('data', chunk => { stderr += chunk; });
  let exit;
  const closed = new Promise((done, fail) => {
    child.once('error', fail);
    child.once('close', code => { ended = true; exit = code; emitter.emit('update'); done(code); });
  });
  const next = (predicate, timeout = 15_000) => new Promise((done, fail) => {
    const cleanup = () => { clearTimeout(timer); emitter.off('update', inspect); emitter.off('failure', reject); };
    const reject = problem => { cleanup(); fail(problem); };
    const inspect = () => {
      const index = events.findIndex(predicate);
      if (index >= 0) { const event = events[index]; events.splice(0, index + 1); cleanup(); done(event); }
      else if (ended) reject(new Error(`watch exited before expected state: ${exit}; ${stderr}`));
    };
    const timer = setTimeout(() => reject(new Error('native capture status deadline exceeded')), timeout);
    emitter.on('update', inspect); emitter.on('failure', reject); inspect();
  });
  return { child, next, closed, stderr: () => stderr };
}
async function git(args) { return run('git', ['-C', project, ...args]); }
async function list() { return JSON.parse(await run(desktop, ['--mesh-attachment', 'versions', metadata])).versions; }

try {
  const verifyBundle = async () => {
    if (app !== null) await run(process.execPath, [join(root, 'apps/desktop/scripts/verify-local-app.mjs'), '--app', app, '--revision', options.revision]);
  };
  await verifyBundle();
  const executableDigest = createHash('sha256').update(await readFile(desktop)).digest('hex');
  await mkdir(project); await mkdir(metadata);
  await git(['init', '--quiet']);
  await writeFile(join(project, 'work.txt'), 'staged\n');
  await git(['add', 'work.txt']);
  await writeFile(join(project, 'work.txt'), 'editing before attachment\n');
  const index = await readFile(join(project, '.git/index'));
  const head = await readFile(join(project, '.git/HEAD'));
  const identity = (await stat(project)).ino;
  await run(meshctl, ['attach', project, metadata]);
  const first = JSON.parse(await run(desktop, ['--mesh-attachment', 'capture', metadata])).saved_version;
  const duplicate = JSON.parse(await run(desktop, ['--mesh-attachment', 'capture', metadata])).saved_version;
  assert.equal(first, duplicate);
  assert.deepEqual(await list(), [first]);

  const live = watch();
  await live.next(value => value.phase === 'waiting' && value.last_outcome === 'unchanged');
  await writeFile(join(project, 'work.txt'), 'ordinary edit, no event signal\n');
  const expectedGit = await git(['status', '--porcelain=v1']);
  const saved = await live.next(value => value.phase === 'waiting' && value.last_outcome === 'saved' && value.saved_version !== first);
  assert.equal(saved.attribution, 'unknown');
  live.child.stdin.write('capture\n');
  await live.next(value => value.phase === 'waiting' && value.last_outcome === 'unchanged');
  live.child.stdin.end('stop\n');
  await live.next(value => value.phase === 'stopped');
  assert.equal(await live.closed, 0);
  assert.equal(await git(['status', '--porcelain=v1']), expectedGit);

  await writeFile(join(project, 'work.txt'), 'edit while Mesh was stopped\n');
  const resumed = watch();
  const recovered = await resumed.next(value => value.phase === 'waiting' && value.last_outcome === 'saved');
  assert.notEqual(recovered.saved_version, saved.saved_version);
  assert.equal(recovered.versions_saved, 1);
  resumed.child.stdin.end(); // EOF also requests a joined stop.
  await resumed.next(value => value.phase === 'stopped');
  assert.equal(await resumed.closed, 0);
  assert.deepEqual(await list(), [first, saved.saved_version, recovered.saved_version]);

  const refused = watch();
  await refused.next(value => value.phase === 'waiting' && value.last_outcome === 'unchanged');
  refused.child.stdin.end('untrusted-control-secret\n');
  assert.equal(await refused.closed, 1);
  assert(!refused.stderr().includes('untrusted-control-secret'));
  assert.equal((await list()).length, 3);
  assert.deepEqual(await readFile(join(project, '.git/index')), index);
  assert.deepEqual(await readFile(join(project, '.git/HEAD')), head);
  assert.equal((await stat(project)).ino, identity);
  assert.equal(await readFile(join(project, 'work.txt'), 'utf8'), 'edit while Mesh was stopped\n');
  assert.equal(createHash('sha256').update(await readFile(desktop)).digest('hex'), executableDigest, 'capture executable changed during proof');
  await verifyBundle();
  process.stdout.write(`${JSON.stringify({ proof: 'mesh-attached-capture-cli/v1', passed: true, versions: 3, elapsed_ms: Math.round(performance.now() - started), packaged: app !== null, graphical: false, revision: options.revision, executable_sha256: executableDigest, registration: 'development-meshctl' })}\n`);
} finally {
  for (const child of children) child.kill('SIGKILL');
  await Promise.all([...children].map(child => new Promise(done => child.once('close', done))));
  await rm(fixture, { recursive: true, force: true });
}
