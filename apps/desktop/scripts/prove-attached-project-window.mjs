#!/usr/bin/env node
// Uses a separate home and dirty Git fixture. No provider, installed app or user project is changed.
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import { mkdir, mkdtemp, readFile, readdir, realpath, stat, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { captureProofArguments } from './attached-capture-proof-args.mjs';
import { rendererProofReportsFromText } from './renderer-proof-protocol.mjs';

const options = captureProofArguments(process.argv.slice(2));
assert.ok(options.app, 'Use --app <sealed bundle> --revision <exact commit>');
const here = dirname(fileURLToPath(import.meta.url));
const verify = () => execFileSync(process.execPath, [join(here, 'verify-local-app.mjs'),
  '--app', options.app, '--revision', options.revision], { stdio: 'pipe' });
verify();
const scratch = await realpath(await mkdtemp('/tmp/mesh-attached-window-'));
const home = join(scratch, 'home');
const source = join(scratch, 'project');
await mkdir(home, { mode: 0o700 }); await mkdir(source);
const file = join(source, 'notes.txt');
const git = (...args) => execFileSync('git', ['-C', source, ...args], { encoding: 'utf8' });
git('init', '--quiet');
await writeFile(file, 'staged baseline\n'); git('add', 'notes.txt');
await writeFile(file, 'first external version\n');
await writeFile(join(source, 'untracked.txt'), 'keep untracked content\n');
const sourceIdentity = (await stat(source)).ino;
async function tree(path) {
  const result = {};
  for (const entry of (await readdir(path, { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
    const child = join(path, entry.name);
    assert.ok(!entry.isSymbolicLink(), 'fixture acquired a link');
    result[entry.name] = entry.isDirectory() ? await tree(child)
      : createHash('sha256').update(await readFile(child)).digest('hex');
  }
  return result;
}
const originalGit = await tree(join(source, '.git'));
const originalStatus = git('status', '--porcelain=v1');
const executable = join(options.app, 'Contents/MacOS/mesh-desktop');
const executableDigest = createHash('sha256').update(await readFile(executable)).digest('hex');
const probe = join(scratch, 'window-proof');
execFileSync('/usr/bin/xcrun', ['clang', '-fobjc-arc', '-framework', 'Foundation', '-framework',
  'CoreGraphics', join(here, 'window-proof.m'), '-o', probe], { stdio: 'pipe' });
const pinFile = join(home, 'Library/Application Support/dev.mesh.desktop/attached-projects/desktop-comparison-pins.json');
const markers = {
  'attached-initial': 'second external version\n',
  'attached-second': 'third external version\n',
  'attached-parallel': 'fourth external version\n',
  'attached-resumed': 'fifth external version after restart\n',
};
let lastBytes = 'first external version\n';
async function launch(surface) {
  const nonce = randomBytes(32).toString('hex');
  const environment = { ...process.env, HOME: home, CFFIXED_USER_HOME: home, TMPDIR: scratch,
    MESH_RENDERER_PROOF_NONCE: nonce, MESH_RENDERER_PROOF_SURFACE: surface, MESH_RENDERER_PROOF_SOURCE: source };
  delete environment.MESH_RENDERER_PROOF_DESTINATION; delete environment.MESH_RENDERER_PROOF_SCREENSHOT;
  const child = spawn(executable, [], { cwd: dirname(executable), env: environment, stdio: ['ignore', 'ignore', 'pipe'] });
  let stderr = ''; let spawnError; let window;
  child.on('error', error => { spawnError = error; });
  child.stderr.setEncoding('utf8'); child.stderr.on('data', chunk => { stderr += chunk; });
  const closed = new Promise(resolve => child.once('close', resolve));
  const seen = new Set();
  try {
    const deadline = Date.now() + 180_000;
    while (Date.now() < deadline) {
      if (spawnError) throw spawnError;
      assert.ok(child.exitCode === null && child.signalCode === null, `app exited: ${stderr}`);
      assert.ok(!stderr.includes(`mesh-renderer-proof-failure:${surface}:`), stderr);
      if (!window) {
        try { window = JSON.parse(execFileSync(probe, [String(child.pid)], { encoding: 'utf8', stdio: 'pipe' })); }
        catch { /* A real visible window remains a required exit condition. */ }
      }
      for (const [marker, bytes] of Object.entries(markers)) {
        if (!seen.has(marker) && stderr.includes(`mesh-renderer-proof-checkpoint:${marker}\n`)) {
          assert.equal(await readFile(file, 'utf8'), lastBytes, 'Mesh changed original work');
          await writeFile(file, bytes); lastBytes = bytes; seen.add(marker);
        }
      }
      if (stderr.includes('mesh-renderer-proof:')) {
        assert.ok(window, 'no real application window was observed');
        const report = rendererProofReportsFromText(stderr, { nonce, surface })[0];
        assert.deepEqual([...seen], surface === 'attached-projects'
          ? ['attached-initial', 'attached-second', 'attached-parallel'] : ['attached-resumed']);
        return { report, window };
      }
      await delay(100);
    }
    throw new Error(`attached window proof deadline exceeded: ${stderr}`);
  } finally {
    if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
    const stopped = await Promise.race([closed.then(() => true), delay(3000).then(() => false)]);
    if (!stopped) { child.kill('SIGKILL'); await closed; }
    await writeFile(join(scratch, `${surface}.log`), stderr, { mode: 0o600 });
  }
}
try {
  const first = await launch('attached-projects');
  // Independently inspect native registrations after the UI creates the work line.
  const catalogue = dirname(pinFile);
  const registrations = await Promise.all((await readdir(catalogue)).filter(name => /^project-[0-9a-f]{64}$/.test(name))
    .map(async name => JSON.parse(await readFile(join(catalogue, name, 'attachment.json'), 'utf8'))));
  assert.equal(registrations.length, 2, 'one original project and one independent line');
  const line = registrations.find(item => item.root !== source);
  assert.ok(line?.root.startsWith(`${home}/`), 'independent line must stay in the isolated proof home');
  assert.equal(await readFile(join(line.root, 'notes.txt'), 'utf8'), 'first external version\n');
  assert.equal(await readFile(join(line.root, 'untracked.txt'), 'utf8'), 'keep untracked content\n');
  const pins = await readFile(pinFile);
  assert.equal(JSON.parse(pins).pins.length, 2);
  const restarted = await launch('attached-projects-restart');
  assert.deepEqual(await readFile(pinFile), pins, 'restart/detach rewrote saved selections');
  assert.deepEqual(await tree(join(source, '.git')), originalGit, 'attachment changed Git internals');
  assert.equal(git('status', '--porcelain=v1'), originalStatus);
  assert.equal((await stat(source)).ino, sourceIdentity);
  assert.equal(await readFile(file, 'utf8'), lastBytes);
  assert.equal(await readFile(join(source, 'untracked.txt'), 'utf8'), 'keep untracked content\n');
  assert.deepEqual((await readdir(source)).sort(), ['.git', 'notes.txt', 'untracked.txt']);
  assert.equal(createHash('sha256').update(await readFile(executable)).digest('hex'), executableDigest);
  verify();
  const result = { schema: 'mesh-attached-project-window-proof/v1', revision: options.revision,
    passed: true, graphical: true, original_git_preserved: true, pinned_comparisons: 2,
    provider_launched: false, protected_main_approval: false, first, restarted, evidence: scratch };
  await writeFile(join(scratch, 'result.json'), JSON.stringify(result, null, 2), { mode: 0o600 });
  process.stdout.write(`${JSON.stringify(result)}\n`);
} catch (error) {
  process.stderr.write(`Preserved attachment proof evidence: ${scratch}\n`);
  throw error;
}
