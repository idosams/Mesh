// Baseline: Jujutsu workspaces, one change per version, measured at `.jj`.
//
// **Never executed on the host that published this directory's rows.** `jj` was
// not installed there, so the runner recorded this baseline as unsupported and
// emitted no timing number for it — which is the contract, not a shortcut. The
// code below is therefore an unexercised path: the first host with `jj` on PATH
// is the first evidence it works, and a failure there is a defect in this file
// rather than in Jujutsu. Read `benchmarks/baselines/README.md` before quoting
// anything from this arm.

import { mkdirSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { run } from '../lib/exec.mjs';
import { contentDigest, treeFootprint } from '../lib/tree.mjs';

export const id = 'jujutsu';

const QUIET_IDENTITY = [
  '--config=user.name=mesh-baseline',
  '--config=user.email=baseline@mesh.invalid',
  '--config=ui.paginate=never',
];

export function supports() {
  return null;
}

export function create({ sourceRoot, storeRoot }) {
  const jjDir = join(sourceRoot, '.jj');
  const jj = (args) => run('jj', ['-R', sourceRoot, ...QUIET_IDENTITY, ...args]);
  let version = 0;

  return {
    storeDescription: `${jjDir} — the Jujutsu store; the working copy is not counted`,
    storeCommand: 'jj describe -m v<N> && jj new',
    compactionCommand: 'jj util gc',
    retainsHistory: true,
    historyNote: 'every described change is retained in the operation history',

    reset() {
      rmSync(jjDir, { recursive: true, force: true });
      rmSync(storeRoot, { recursive: true, force: true });
      mkdirSync(storeRoot, { recursive: true });
      jj(['git', 'init', '--quiet']);
      version = 0;
    },

    storeVersion() {
      version += 1;
      // `jj` snapshots the working copy on any command; `describe` then `new`
      // is the closest analogue of Git's add-and-commit, and both are inside
      // the timed section because both are needed to reach a stored version.
      jj(['describe', '-m', `v${version}`]);
      jj(['new']);
    },

    compact() {
      jj(['util', 'gc']);
    },

    footprint() {
      return treeFootprint(jjDir);
    },

    exportLatest() {
      const destination = join(storeRoot, 'export');
      rmSync(destination, { recursive: true, force: true });
      mkdirSync(destination, { recursive: true });
      jj(['workspace', 'add', '--revision', '@-', destination]);
      return contentDigest(destination, { exclude: ['.jj'] });
    },
  };
}
