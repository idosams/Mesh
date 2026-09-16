// Baseline: folder replication (rsync), the sync-tool family.
//
// The arm that is easiest to misread. rsync keeps exactly one version, so its
// footprint after a second store is the size of the tree and its *delta* is
// close to zero — which looks like a win over every version store in the table
// and is not one. `retainsHistory: false` is carried into the row and into every
// report so the comparison cannot be quoted without it: this baseline wins the
// bytes by discarding the thing the other baselines are storing.
//
// Syncthing is the other member of this family. It is publishable (MPL-2.0) and
// deliberately not implemented here: it needs two live daemons and a network,
// which is a different runner with a different failure surface.

import { mkdirSync, rmSync } from 'node:fs';
import { run } from '../lib/exec.mjs';
import { contentDigest, treeFootprint } from '../lib/tree.mjs';
import { VCS_EXCLUDES } from './native-fs.mjs';

export const id = 'replication';

export function supports(workloadId) {
  if (workloadId === 'checkpoint-history') {
    return 'rsync mirrors one version and keeps no history, so it cannot perform a workload that reads back an earlier stored version';
  }
  return null;
}

/**
 * The sync arguments, and why `--checksum` is on.
 *
 * `rsync -a` alone decides what changed with the *quick check*: size plus mtime.
 * macOS ships rsync 2.6.9, whose mtime comparison is whole-second, so an edit
 * that does not change a file's size and lands in the same second the file was
 * written is **silently skipped** — the mirror keeps the old bytes and rsync
 * exits 0. The `one-byte-edit` workload does exactly that, and this runner's
 * correctness gate caught it: the digests disagreed and no timing number was
 * produced. `--checksum` is therefore part of the configured baseline, not a
 * tuning choice, and it is recorded in every row.
 *
 * It is not free. `--checksum` reads and digests every file on both sides, so
 * this arm is slower than a default `rsync -a` would be. That cost is the price
 * of a mirror that is actually a mirror, and reading this arm's latency as
 * "rsync is slow" without it would be wrong in the other direction.
 */
export const SYNC_ARGUMENTS = ['-a', '--checksum', '--delete', '--exclude=.git', '--exclude=.jj'];

export function create({ sourceRoot, storeRoot }) {
  return {
    storeDescription: `${storeRoot} — a one-version mirror, refreshed in place`,
    storeCommand: `rsync ${SYNC_ARGUMENTS.join(' ')} <source>/ <mirror>/`,
    compactionCommand: null,
    retainsHistory: false,
    historyNote:
      'only the latest version exists; a delta measured here is not comparable to a version store without saying so',

    reset() {
      rmSync(storeRoot, { recursive: true, force: true });
      mkdirSync(storeRoot, { recursive: true });
    },

    storeVersion() {
      run('rsync', [...SYNC_ARGUMENTS, `${sourceRoot}/`, `${storeRoot}/`]);
    },

    compact() {},

    footprint() {
      return treeFootprint(storeRoot, { exclude: VCS_EXCLUDES });
    },

    exportLatest() {
      return contentDigest(storeRoot, { exclude: VCS_EXCLUDES });
    },
  };
}
