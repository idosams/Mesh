// What a baseline is asked to do, and the deterministic mutation each one applies.
//
// Three workloads, each a named row of plan §10.4 or of
// `benchmarks/budgets/storage.md` §6. Every mutation is chosen by a rule over
// the sorted file list rather than by a hardcoded path, so the same workload
// applies to any corpus and the row records exactly which file it landed on.

import { readFileSync, writeFileSync, renameSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { contentDigest, walk } from './tree.mjs';

/** The workloads this runner knows how to drive. */
export const WORKLOAD_IDS = ['store-tree', 'one-byte-edit', 'directory-move'];

/** Builds the workload named `id` against a prepared corpus. */
export function buildWorkload(id, corpus) {
  switch (id) {
    case 'store-tree':
      return storeTree();
    case 'one-byte-edit':
      return oneByteEdit(corpus);
    case 'directory-move':
      return directoryMove(corpus);
    default:
      throw new Error(`unknown workload \`${id}\`; known: ${WORKLOAD_IDS.join(', ')}`);
  }
}

function storeTree() {
  return {
    id: 'store-tree',
    title: 'Store one version of a tree',
    planReference:
      'plan §10.4 checkpoint latency; benchmarks/budgets/storage.md §6 rows 1-2 (footprint of one stored version)',
    versionsBeforeTimedStore: 0,
    mutation: null,
    parameters: {},
  };
}

function oneByteEdit(corpus) {
  const target = middleFileAtLeast(corpus.root, 1024);
  const offset = Math.floor(target.size / 2);
  const flip = () => {
    const buffer = readFileSync(target.absolutePath);
    buffer[offset] ^= 0x01;
    writeFileSync(target.absolutePath, buffer);
  };
  return {
    id: 'one-byte-edit',
    title: 'Store a second version after a one-byte edit',
    planReference:
      'benchmarks/budgets/storage.md §6 row 3 (+608 B for Git against +8,962 B for Mesh); plan §10.4 large binary delta at file scale',
    versionsBeforeTimedStore: 1,
    mutation: reversibleMutation(flip, flip),
    parameters: {
      edited_path: target.relativePath,
      edited_file_bytes: target.size,
      edited_offset: offset,
      edit: 'XOR 0x01 on one byte',
      selection_rule: 'the median file, by sorted relative path, among files of at least 1024 bytes',
    },
  };
}

function directoryMove(corpus) {
  const source = firstDirectoryWithSiblings(corpus.root);
  const destination = `${source.absolutePath}-moved`;
  return {
    id: 'directory-move',
    title: 'Store a second version after renaming a directory',
    planReference: 'plan §10.4 directory moves',
    versionsBeforeTimedStore: 1,
    mutation: reversibleMutation(
      () => renameSync(source.absolutePath, destination),
      () => renameSync(destination, source.absolutePath),
    ),
    parameters: {
      moved_path: source.relativePath,
      moved_to: `${source.relativePath}-moved`,
      moved_file_count: source.fileCount,
      selection_rule: 'the first directory, by sorted relative path, holding at least two files',
    },
  };
}

/**
 * A mutation that can be taken back.
 *
 * Every iteration must start from the pristine corpus, or the second iteration
 * measures a different edit from the first — and for `directory-move` it does
 * not measure anything at all, because the directory has already moved. The
 * undo runs *before* the store is reset, so version 1 is always the pristine
 * tree and version 2 always differs from it by exactly the stated change.
 */
function reversibleMutation(apply, undo) {
  let applied = false;
  return {
    apply() {
      if (applied) return;
      apply();
      applied = true;
    },
    revert() {
      if (!applied) return;
      undo();
      applied = false;
    },
  };
}

/**
 * Runs the untimed part of one iteration: the pristine corpus, a fresh store,
 * the versions that precede the timed one, and then the mutation.
 */
export function setupIteration(instance, workload) {
  if (workload.mutation) workload.mutation.revert();
  instance.reset();
  for (let index = 0; index < workload.versionsBeforeTimedStore; index += 1) {
    instance.storeVersion();
  }
  if (workload.mutation) workload.mutation.apply();
}

/** The digest the corpus tree has once the workload's mutation has been applied. */
export function expectedDigestAfter(corpus, workload) {
  return contentDigest(corpus.root, { exclude: ['.git', '.jj'] });
}

function middleFileAtLeast(root, minimumBytes) {
  const candidates = [...walk(root, ['.git', '.jj'])].filter((entry) => entry.size >= minimumBytes);
  if (candidates.length === 0) {
    throw new Error(`workload: no file of at least ${minimumBytes} bytes under ${root}`);
  }
  return candidates[Math.floor(candidates.length / 2)];
}

function firstDirectoryWithSiblings(root) {
  const counts = new Map();
  for (const entry of walk(root, ['.git', '.jj'])) {
    const directory = dirname(entry.relativePath);
    if (directory === '.') continue;
    counts.set(directory, (counts.get(directory) ?? 0) + 1);
  }
  const chosen = [...counts.entries()]
    .filter(([, count]) => count >= 2)
    .sort(([left], [right]) => (left < right ? -1 : 1))[0];
  if (!chosen) {
    throw new Error(`workload: no directory under ${root} holds two files`);
  }
  return {
    relativePath: chosen[0],
    absolutePath: join(root, chosen[0]),
    fileCount: chosen[1],
  };
}
