// Baseline: the native filesystem, one full copy per version.
//
// The floor every other baseline is measured against. It keeps every version and
// shares nothing between them, so its footprint is the cost of *not* having a
// version store — which is exactly why it belongs in the table.

import { mkdirSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { run } from '../lib/exec.mjs';
import { contentDigest, treeFootprint } from '../lib/tree.mjs';

export const VCS_EXCLUDES = ['.git', '.jj'];

export const id = 'native-fs';

/** Every workload is performable by a copy; nothing here is unsupported. */
export function supports() {
  return null;
}

export function create({ sourceRoot, storeRoot }) {
  let version = 0;
  return {
    storeDescription: `${storeRoot} — one directory per stored version, full copy each time`,
    storeCommand: 'cp -Rp <source>/. <store>/v<N>',
    compactionCommand: null,
    retainsHistory: true,
    historyNote: 'every version is kept, in full, with no sharing between them',

    reset() {
      rmSync(storeRoot, { recursive: true, force: true });
      mkdirSync(storeRoot, { recursive: true });
      version = 0;
    },

    storeVersion() {
      version += 1;
      run('cp', ['-Rp', `${sourceRoot}/.`, join(storeRoot, `v${version}`)]);
    },

    compact() {},

    footprint() {
      return treeFootprint(storeRoot, { exclude: VCS_EXCLUDES });
    },

    exportLatest() {
      return contentDigest(join(storeRoot, `v${version}`), { exclude: VCS_EXCLUDES });
    },
  };
}
