import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtemp, mkdir, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import test from 'node:test';
import {
  assertSeedRepositoryUnchanged,
  inspectSeedRepository,
  restoreSeedRepository,
} from './proof-seed-repository.mjs';

function git(repository, arguments_) {
  return execFileSync('git', ['-C', repository, ...arguments_], {
    encoding: 'utf8',
    env: {
      ...process.env,
      GIT_AUTHOR_NAME: 'Mesh proof test',
      GIT_AUTHOR_EMAIL: 'proof@example.invalid',
      GIT_COMMITTER_NAME: 'Mesh proof test',
      GIT_COMMITTER_EMAIL: 'proof@example.invalid',
    },
  }).trim();
}

async function fixture() {
  const scratch = await mkdtemp('/tmp/mesh-proof-seed-test-');
  const repository = join(scratch, 'repository');
  const restored = join(scratch, 'restored');
  await mkdir(repository);
  await mkdir(restored);
  git(repository, ['init', '--quiet']);
  await mkdir(join(repository, 'nested'));
  await writeFile(join(repository, 'nested', 'saved.txt'), 'committed bytes\n');
  git(repository, ['add', 'nested/saved.txt']);
  git(repository, ['commit', '--quiet', '-m', 'fixture']);
  return { scratch, repository, restored };
}

test('a clean committed repository restores without changing its source', async (context) => {
  const current = await fixture();
  context.after(() => rm(current.scratch, { recursive: true, force: true }));
  const seed = await inspectSeedRepository(current.repository);

  restoreSeedRepository(seed, current.restored, current.scratch);

  assert.equal(await readFile(join(current.restored, 'nested', 'saved.txt'), 'utf8'), 'committed bytes\n');
  assert.equal(seed.revision, git(current.repository, ['rev-parse', 'HEAD']));
  assert.equal(seed.tree, git(current.repository, ['rev-parse', 'HEAD^{tree}']));
  assert.equal(seed.trackedFiles, 1);
  assertSeedRepositoryUnchanged(seed);
});

test('a dirty or partial worktree cannot be presented as a committed backup', async (context) => {
  const current = await fixture();
  context.after(() => rm(current.scratch, { recursive: true, force: true }));
  await writeFile(join(current.repository, 'untracked.txt'), 'not backed up\n');

  await assert.rejects(
    inspectSeedRepository(current.repository),
    /must be clean so every imported byte comes from its committed backup/u,
  );
  await assert.rejects(
    inspectSeedRepository(join(current.repository, 'nested')),
    /must name the root of one Git worktree/u,
  );
});

test('a post-inspection source change is detected before it can be reported as preserved', async (context) => {
  const current = await fixture();
  context.after(() => rm(current.scratch, { recursive: true, force: true }));
  const seed = await inspectSeedRepository(current.repository);
  await writeFile(join(current.repository, 'nested', 'saved.txt'), 'changed later\n');

  assert.throws(
    () => assertSeedRepositoryUnchanged(seed),
    /changed the seed repository worktree/u,
  );
});

test('a tracked symlink cannot redirect proof fixture writes outside the disposable source', async (context) => {
  const current = await fixture();
  context.after(() => rm(current.scratch, { recursive: true, force: true }));
  const external = join(current.scratch, 'external');
  await mkdir(external);
  await symlink(external, join(current.repository, 'assets'));
  git(current.repository, ['add', 'assets']);
  git(current.repository, ['commit', '--quiet', '-m', 'tracked symlink']);

  await assert.rejects(
    inspectSeedRepository(current.repository),
    /must contain only ordinary tracked files/u,
  );
});
