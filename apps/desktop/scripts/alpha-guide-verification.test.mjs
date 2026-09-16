import assert from 'node:assert/strict';
import { mkdtemp, mkdir, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import {
  assertRegularExactFile,
  assertUniqueArchiveEntries,
} from './alpha-guide-verification.mjs';

test('alpha guide verification accepts only exact regular files and unique archive paths', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'mesh-alpha-guide-verification-'));
  try {
    const expected = join(directory, 'expected.txt');
    const exact = join(directory, 'exact.txt');
    const changed = join(directory, 'changed.txt');
    const linked = join(directory, 'linked.txt');
    const nested = join(directory, 'nested');
    await writeFile(expected, 'reviewed guide\n');
    await writeFile(exact, 'reviewed guide\n');
    await writeFile(changed, 'changed guide\n');
    await symlink(expected, linked);
    await mkdir(nested);

    assert.equal((await assertRegularExactFile(exact, expected, 'guide')).toString(), 'reviewed guide\n');
    await assert.rejects(
      assertRegularExactFile(linked, expected, 'guide'),
      /regular non-link file/,
    );
    await assert.rejects(
      assertRegularExactFile(changed, expected, 'guide'),
      /match the reviewed source/,
    );
    await assert.rejects(
      assertRegularExactFile(nested, expected, 'guide'),
      /regular non-link file/,
    );

    assert.doesNotThrow(() => assertUniqueArchiveEntries(['Mesh.app/', 'START-HERE.txt'], 'archive'));
    assert.throws(
      () => assertUniqueArchiveEntries(['START-HERE.txt', 'START-HERE.txt'], 'archive'),
      /must not contain duplicate paths/,
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
