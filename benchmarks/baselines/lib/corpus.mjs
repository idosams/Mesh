// The data every arm of a comparison runs over — identical bytes, by construction.
//
// Two families, both regenerable by a third party from the row alone:
//
//   W1..W6   the committed seeded generators in `crates/mesh-bench/src/corpus/**`,
//            materialised through the same `mesh-bench` binary the Mesh runs use.
//            "The baseline runners and the Mesh runs consume identical generated
//            corpora" is the handoff condition of `benchmarks/workloads/README.md`,
//            and using a second generator here would quietly break it.
//
//   source-tree
//            every `*.rs` file tracked at the measured commit. Not seeded and not
//            synthetic on purpose: `benchmarks/budgets/storage.md` §6 row 2 is a
//            *real source tree*, and the 3.42x figure this task exists to make
//            re-runnable is that row. It is reproducible because it is pinned by
//            `repository.commit`, which every row carries.

import { mkdirSync, rmSync, readdirSync, rmdirSync } from 'node:fs';
import { join } from 'node:path';
import { output, run, shell } from './exec.mjs';
import { contentDigest, walk } from './tree.mjs';

/** Corpus specifications this runner accepts. */
export const CORPUS_IDS = ['source-tree', 'W1', 'W2', 'W3', 'W4', 'W5', 'W6'];

/**
 * Materialises `spec` into `root` and returns its descriptor and digest.
 *
 * The digest is taken here, before any baseline touches the tree, and is what
 * every baseline's correctness verification is held against.
 */
export function prepareCorpus(spec, { repoRoot, root, meshBench, scale, seed }) {
  rmSync(root, { recursive: true, force: true });
  mkdirSync(root, { recursive: true });
  const prepared =
    spec === 'source-tree'
      ? prepareSourceTree({ repoRoot, root })
      : prepareGenerated({ spec, root, meshBench, scale, seed });
  const files = [...walk(root)];
  return {
    ...prepared,
    root,
    digest: contentDigest(root),
    file_count: files.length,
    logical_bytes: files.reduce((total, entry) => total + entry.size, 0),
  };
}

function prepareSourceTree({ repoRoot, root }) {
  // `git archive` of the commit, not a copy of the working tree: a dirty file
  // would otherwise put uncommitted bytes into a corpus the row claims is
  // defined by `repository.commit`.
  shell(`git -C '${repoRoot}' archive --format=tar HEAD crates | tar -x -C '${root}'`);
  pruneToExtension(root, '.rs');
  return {
    generator: 'benchmarks/baselines/lib/corpus.mjs::source-tree',
    generator_version: '1',
    seed: 0,
    parameters: {
      selection: 'every *.rs file tracked under crates/ at repository.commit',
      seeded: false,
      why_not_seeded:
        'storage.md §6 row 2 is a real source tree; the commit is what makes it reproducible',
    },
  };
}

function prepareGenerated({ spec, root, meshBench, scale, seed }) {
  run(meshBench, [
    'corpus',
    'materialize',
    '--workload',
    spec,
    '--scale',
    scale,
    '--seed',
    String(seed),
    '--root',
    root,
  ]);
  const described = JSON.parse(
    output(meshBench, ['corpus', 'digest', '--workload', spec, '--scale', scale, '--seed', String(seed), '--content']),
  );
  return {
    generator: `crates/mesh-bench/src/corpus/${spec.toLowerCase()}.rs`,
    generator_version: described.generator_version,
    seed,
    parameters: {
      workload: spec,
      scale,
      plan_digest: described.plan_digest,
      content_digest: described.content_digest,
      materialized_by: `${meshBench} corpus materialize`,
    },
  };
}

/** Deletes every file that is not `extension`, then every directory left empty. */
function pruneToExtension(root, extension) {
  for (const entry of walk(root)) {
    if (!entry.relativePath.endsWith(extension)) {
      rmSync(entry.absolutePath, { force: true });
    }
  }
  pruneEmptyDirectories(root);
}

function pruneEmptyDirectories(directory) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (entry.isDirectory()) pruneEmptyDirectories(join(directory, entry.name));
  }
  if (readdirSync(directory).length === 0) {
    try {
      rmdirSync(directory);
    } catch {
      /* the root itself, or a race with another walker; either is harmless */
    }
  }
}
