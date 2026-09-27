import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { checkRepository, repositoryIdentity } from './repository-target.mjs';

function fixture(t) {
  const cwd = mkdtempSync(join(tmpdir(), 'mesh-target-'));
  t.after(() => rmSync(cwd, { recursive: true, force: true }));
  const git = (...args) => execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  git('init', '-b', 'main');
  git('config', 'user.name', 'Target check fixture');
  git('config', 'user.email', 'fixture@example.invalid');
  writeFileSync(join(cwd, 'content'), 'base');
  git('add', 'content'); git('commit', '-m', 'base');
  const baseOid = git('rev-parse', 'HEAD');
  git('remote', 'add', 'origin', 'https://github.com/idosams/Mesh.git');
  git('update-ref', 'refs/remotes/origin/main', baseOid);
  git('switch', '-c', 'idosams/change');
  writeFileSync(join(cwd, 'content'), 'change');
  git('commit', '-am', 'change');
  const head = git('rev-parse', 'HEAD');
  const live = () => ({ name: 'idosams/Mesh', defaultBranch: 'main', baseOid, headOid: head });
  return { cwd, git, baseOid, head, live };
}

test('identity accepts canonical GitHub transports and rejects lookalikes and credential URLs', () => {
  for (const url of ['https://github.com/idosams/Mesh.git', 'git@github.com:idosams/Mesh.git', 'ssh://git@github.com/idosams/Mesh.git']) assert.equal(repositoryIdentity(url), 'idosams/mesh');
  for (const url of ['https://github.com/idosams/Mesh-internal.git', 'https://github.com.evil.invalid/idosams/Mesh.git', 'https://token@github.com/idosams/Mesh.git', '/Users/name/Mesh', 'https://github.com/idosams/Mesh/extra']) assert.notEqual(repositoryIdentity(url), 'idosams/mesh');
});
test('local edit preflight binds canonical identity and exact intended ancestry', t => {
  const f = fixture(t);
  assert.deepEqual(checkRepository({ cwd: f.cwd, action: 'edit', base: 'main' }), { repository: 'idosams/Mesh', action: 'edit', branch: 'idosams/change', head: f.head, base: 'main', baseOid: f.baseOid, live: false });
  f.git('remote', 'set-url', 'origin', 'https://github.com/idosams/Mesh-internal.git');
  assert.throws(() => checkRepository({ cwd: f.cwd, action: 'edit', base: 'main' }), /every origin/);
});
test('a separate wrong push URL is refused even with a canonical fetch URL', t => {
  const f = fixture(t);
  f.git('remote', 'set-url', '--push', 'origin', 'https://github.com/idosams/Mesh-internal.git');
  assert.throws(() => checkRepository({ cwd: f.cwd, action: 'push', base: 'main', live: f.live }), /every origin/);
});
test('missing and unrelated bases refuse instead of accepting a folder name', t => {
  const f = fixture(t);
  assert.throws(() => checkRepository({ cwd: f.cwd, action: 'edit', base: 'missing' }));
  f.git('switch', '--orphan', 'unrelated');
  writeFileSync(join(f.cwd, 'other'), 'other'); f.git('add', 'other'); f.git('commit', '-m', 'other history');
  f.git('update-ref', 'refs/remotes/origin/foreign', 'HEAD');
  f.git('switch', 'idosams/change');
  assert.throws(() => checkRepository({ cwd: f.cwd, action: 'edit', base: 'foreign' }), /intended base/);
});
test('stacked delivery uses a published canonical ancestor as the explicit base', t => {
  const f = fixture(t);
  f.git('update-ref', 'refs/remotes/origin/idosams/parent', f.head);
  f.git('switch', '-c', 'idosams/child');
  assert.equal(checkRepository({ cwd: f.cwd, action: 'edit', base: 'idosams/parent' }).baseOid, f.head);
  assert.throws(() => checkRepository({ cwd: f.cwd, action: 'edit' }), /explicit/);
});
test('live delivery rejects renamed repositories, moved bases, and unpublished heads', t => {
  const f = fixture(t);
  for (const patch of [{ name: 'idosams/Mesh-internal' }, { defaultBranch: 'other' }, { baseOid: f.head }, { headOid: f.baseOid }]) {
    assert.throws(() => checkRepository({ cwd: f.cwd, action: 'pr', base: 'main', live: () => ({ ...f.live(), ...patch }) }), /refused/);
  }
  assert.equal(checkRepository({ cwd: f.cwd, action: 'pr', base: 'main', live: f.live }).head, f.head);
  writeFileSync(join(f.cwd, 'uncommitted'), 'keep');
  assert.throws(() => checkRepository({ cwd: f.cwd, action: 'push', base: 'main', live: f.live }), /preserve local changes/);
});
test('merge preflight binds the actual PR but never performs or authorizes a merge', t => {
  const f = fixture(t);
  const pull = { state: 'open', draft: false, base: { repo: { full_name: 'idosams/Mesh' }, ref: 'main' }, head: { repo: { full_name: 'idosams/Mesh' }, ref: 'idosams/change', sha: f.head } };
  const live = () => ({ ...f.live(), pull });
  assert.throws(() => checkRepository({ cwd: f.cwd, action: 'merge', base: 'main', live }), /--pr/);
  assert.equal(checkRepository({ cwd: f.cwd, action: 'merge', base: 'main', pr: 12, live }).head, f.head);
  pull.base.repo.full_name = 'idosams/Mesh-internal';
  assert.throws(() => checkRepository({ cwd: f.cwd, action: 'merge', base: 'main', pr: 12, live }), /PR repository/);
  assert.equal(f.git('rev-parse', 'HEAD'), f.head);
});
