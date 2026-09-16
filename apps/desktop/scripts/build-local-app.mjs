#!/usr/bin/env node

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const desktop = resolve(here, '..');
const repository = resolve(desktop, '../..');
const targetDirectory = process.env.CARGO_TARGET_DIR
  ? resolve(desktop, process.env.CARGO_TARGET_DIR)
  : resolve(repository, 'target');
const app = resolve(targetDirectory, 'release/bundle/macos/Mesh.app');
const rustupCargo = resolve(homedir(), '.cargo/bin/cargo');
const cargo = process.env.CARGO ?? (existsSync(rustupCargo) ? rustupCargo : 'cargo');

function git(...args) {
  return execFileSync('git', ['-C', repository, ...args], { encoding: 'utf8' }).trim();
}

const revision = git('rev-parse', '--verify', 'HEAD');
assert.match(revision, /^[0-9a-f]{40}$/, 'the local bundle requires one canonical Git commit');
assert.equal(
  git('status', '--porcelain'),
  '',
  'the local bundle refuses tracked changes so its embedded Git revision remains exact',
);

// ui-next has its own exact lockfile rather than inheriting caller-machine dependencies. Install
// that closure even in a clean detached packaging materialization before Tauri invokes Vite.
execFileSync('npm', ['run', 'ui:next:install'], {
  cwd: desktop,
  stdio: 'inherit',
});

execFileSync(cargo, ['tauri', 'build', '--bundles', 'app'], {
  cwd: desktop,
  env: { ...process.env, MESH_BUILD_REVISION: revision },
  stdio: 'inherit',
});
execFileSync('/usr/bin/codesign', ['--force', '--sign', '-', '--timestamp=none', app], {
  stdio: 'inherit',
});
execFileSync(process.execPath, [
  resolve(here, 'verify-local-app.mjs'),
  '--app', app,
  '--revision', revision,
], {
  cwd: desktop,
  stdio: 'inherit',
});
