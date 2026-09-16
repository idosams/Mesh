// Baseline: Git, one commit per version, measured at `.git`.
//
// The store is the object database, not the working copy — the same boundary
// `benchmarks/budgets/storage.md` §6 drew when it counted `find .git -type f`,
// kept identical so a row from this runner can be held against that page rather
// than merely resembling it. Git is also the only baseline here with a
// collector, so `compact()` runs `git gc --aggressive` and both figures are
// reported: a Git number after `gc` and a Git number before it are different
// numbers, and quoting one as the other is the easiest way to be wrong by 19x.

import { mkdirSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { run, shell } from '../lib/exec.mjs';
import { contentDigest, treeFootprint } from '../lib/tree.mjs';

export const id = 'git-worktree';

const QUIET_IDENTITY = [
  '-c',
  'user.name=mesh-baseline',
  '-c',
  'user.email=baseline@mesh.invalid',
  '-c',
  'commit.gpgsign=false',
  '-c',
  'core.fsmonitor=false',
  '-c',
  'gc.auto=0',
];

export function supports() {
  return null;
}

export function create({ sourceRoot, storeRoot }) {
  const gitDir = join(sourceRoot, '.git');
  const git = (args) => run('git', ['-C', sourceRoot, ...QUIET_IDENTITY, ...args]);
  let version = 0;

  return {
    storeDescription: `${gitDir} — the object database; the working copy is not counted`,
    storeCommand: 'git add -A && git commit -m v<N>',
    compactionCommand: 'git gc --aggressive --prune=now',
    retainsHistory: true,
    historyNote: 'every committed version is reachable from the stored history',

    reset() {
      rmSync(gitDir, { recursive: true, force: true });
      rmSync(storeRoot, { recursive: true, force: true });
      mkdirSync(storeRoot, { recursive: true });
      git(['init', '--quiet', '--initial-branch=main']);
      version = 0;
    },

    storeVersion() {
      version += 1;
      git(['add', '-A']);
      git(['commit', '--quiet', '--allow-empty', '-m', `v${version}`]);
    },

    compact() {
      git(['gc', '--aggressive', '--prune=now', '--quiet']);
    },

    footprint() {
      return treeFootprint(gitDir);
    },

    exportLatest() {
      const destination = join(storeRoot, 'export');
      rmSync(destination, { recursive: true, force: true });
      mkdirSync(destination, { recursive: true });
      shell(
        `git -C '${sourceRoot}' archive --format=tar HEAD | tar -x -C '${destination}'`,
      );
      return contentDigest(destination);
    },
  };
}
