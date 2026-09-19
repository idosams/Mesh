import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { realpath } from 'node:fs/promises';

function git(repositoryPath, arguments_) {
  return execFileSync('git', ['-C', repositoryPath, ...arguments_], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim();
}

function unsafeTrackedEntries(repositoryPath, revision) {
  const entries = execFileSync('git', [
    '-C', repositoryPath, 'ls-tree', '-rz', revision,
  ], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] })
    .split('\0')
    .filter(Boolean);
  return entries.filter((entry) => !/^100(?:644|755) blob /u.test(entry));
}

export async function inspectSeedRepository(repositoryPath) {
  if (repositoryPath === null) return null;
  const canonical = await realpath(repositoryPath);
  const topLevel = await realpath(git(canonical, ['rev-parse', '--show-toplevel']));
  assert.equal(
    canonical,
    topLevel,
    '--seed-repository must name the root of one Git worktree',
  );
  const status = git(canonical, ['status', '--porcelain=v2', '--untracked-files=all']);
  assert.equal(
    status,
    '',
    '--seed-repository must be clean so every imported byte comes from its committed backup',
  );
  const revision = git(canonical, ['rev-parse', '--verify', 'HEAD']);
  const tree = git(canonical, ['rev-parse', '--verify', 'HEAD^{tree}']);
  assert.deepEqual(
    unsafeTrackedEntries(canonical, revision),
    [],
    '--seed-repository must contain only ordinary tracked files; symlinks and gitlinks are refused',
  );
  const trackedFiles = git(canonical, ['ls-files', '-z'])
    .split('\0')
    .filter(Boolean).length;
  assert.ok(trackedFiles > 0, '--seed-repository contained no tracked files');
  return { path: canonical, revision, tree, trackedFiles };
}

export function assertSeedRepositoryUnchanged(seed) {
  if (seed === null) return;
  assert.equal(
    git(seed.path, ['rev-parse', '--verify', 'HEAD']),
    seed.revision,
    'the packaged proof changed the seed repository revision',
  );
  assert.equal(
    git(seed.path, ['rev-parse', '--verify', 'HEAD^{tree}']),
    seed.tree,
    'the packaged proof changed the seed repository tree',
  );
  assert.equal(
    git(seed.path, ['status', '--porcelain=v2', '--untracked-files=all']),
    '',
    'the packaged proof changed the seed repository worktree',
  );
}

export function restoreSeedRepository(seed, destination, scratch) {
  if (seed === null) return;
  const archive = join(scratch, 'seed-repository.tar');
  execFileSync('git', [
    '-C', seed.path, 'archive', '--format=tar', `--output=${archive}`, seed.revision,
  ], { stdio: 'inherit' });
  execFileSync('/usr/bin/tar', ['-xf', archive, '-C', destination], { stdio: 'inherit' });
  assertSeedRepositoryUnchanged(seed);
}
