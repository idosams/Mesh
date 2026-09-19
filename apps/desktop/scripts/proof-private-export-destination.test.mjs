import assert from 'node:assert/strict';
import { lstat, mkdtemp, mkdir, rm, symlink, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import test from 'node:test';
import { mirrorOrdinaryDirectories } from './proof-private-export-destination.mjs';

test('private export proof mirrors only the ordinary directory topology', async (context) => {
  const scratch = await mkdtemp('/tmp/mesh-proof-export-directories-');
  context.after(() => rm(scratch, { recursive: true, force: true }));
  const source = join(scratch, 'source');
  const destination = join(scratch, 'destination');
  await mkdir(join(source, 'nested', 'deeper'), { recursive: true });
  await mkdir(join(source, '.git'));
  await mkdir(destination);
  await writeFile(join(source, 'nested', 'deeper', 'saved.txt'), 'saved\n');

  await mirrorOrdinaryDirectories(source, destination, { excludedRootNames: ['.git'] });

  assert.equal((await lstat(join(destination, 'nested'))).isDirectory(), true);
  assert.equal((await lstat(join(destination, 'nested', 'deeper'))).isDirectory(), true);
  await assert.rejects(lstat(join(destination, '.git')), { code: 'ENOENT' });
  await assert.rejects(lstat(join(destination, 'nested', 'deeper', 'saved.txt')), { code: 'ENOENT' });
});

test('private export proof refuses symlinked source topology', async (context) => {
  const scratch = await mkdtemp('/tmp/mesh-proof-export-symlink-');
  context.after(() => rm(scratch, { recursive: true, force: true }));
  const source = join(scratch, 'source');
  const destination = join(scratch, 'destination');
  const external = join(scratch, 'external');
  await mkdir(source);
  await mkdir(destination);
  await mkdir(external);
  await symlink(external, join(source, 'redirect'));

  await assert.rejects(
    mirrorOrdinaryDirectories(source, destination),
    /non-ordinary entry at redirect/u,
  );
});

test('private export proof refuses a symlinked destination root before writing', async (context) => {
  const scratch = await mkdtemp('/tmp/mesh-proof-export-destination-symlink-');
  context.after(() => rm(scratch, { recursive: true, force: true }));
  const source = join(scratch, 'source');
  const external = join(scratch, 'external');
  const destination = join(scratch, 'destination');
  await mkdir(join(source, 'nested'), { recursive: true });
  await mkdir(external);
  await symlink(external, destination);

  await assert.rejects(
    mirrorOrdinaryDirectories(source, destination),
    /destination root must be one real directory/u,
  );
  await assert.rejects(lstat(join(external, 'nested')), { code: 'ENOENT' });
});

test('private export proof refuses nested source and destination roots', async (context) => {
  const scratch = await mkdtemp('/tmp/mesh-proof-export-nested-roots-');
  context.after(() => rm(scratch, { recursive: true, force: true }));
  const source = join(scratch, 'source');
  const destination = join(source, 'destination');
  await mkdir(destination, { recursive: true });

  await assert.rejects(
    mirrorOrdinaryDirectories(source, destination),
    /source and destination roots must be disjoint/u,
  );
});
