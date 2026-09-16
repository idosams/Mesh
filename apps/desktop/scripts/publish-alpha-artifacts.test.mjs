import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { publishVerifiedAlphaArtifacts } from './publish-alpha-artifacts.mjs';

async function fixture() {
  const root = await mkdtemp(join(tmpdir(), 'mesh-alpha-publish-'));
  const paths = {
    stagedArchive: join(root, 'staged.zip'),
    stagedManifest: join(root, 'staged.json'),
    stagedChecksum: join(root, 'staged.sha256'),
    stagedDelivery: join(root, 'staged-delivery.zip'),
    archive: join(root, 'release.zip'),
    manifest: join(root, 'release.json'),
    checksumFile: join(root, 'release.sha256'),
    delivery: join(root, 'release-delivery.zip'),
  };
  await Promise.all([
    writeFile(paths.stagedArchive, 'new archive'),
    writeFile(paths.stagedManifest, 'new manifest'),
    writeFile(paths.stagedChecksum, 'new checksum'),
    writeFile(paths.stagedDelivery, 'new delivery'),
    writeFile(paths.archive, 'old archive'),
    writeFile(paths.manifest, 'old manifest'),
    writeFile(paths.checksumFile, 'old checksum'),
    writeFile(paths.delivery, 'old delivery'),
  ]);
  return { root, paths };
}

async function contents(paths) {
  return Promise.all([
    readFile(paths.archive, 'utf8'),
    readFile(paths.manifest, 'utf8'),
    readFile(paths.checksumFile, 'utf8'),
    readFile(paths.delivery, 'utf8'),
  ]);
}

test('a failed extracted-app proof leaves the prior complete release untouched', async () => {
  const { root, paths } = await fixture();
  const moves = [];
  const directories = [];
  try {
    await assert.rejects(
      publishVerifiedAlphaArtifacts({
        ...paths,
        verify: async (manifest) => {
          assert.equal(manifest, paths.stagedManifest);
          throw new Error('planted extracted-app proof failure');
        },
        move: async (...args) => {
          moves.push(args);
          throw new Error('move must not run before verification succeeds');
        },
        ensureDirectory: async (...args) => {
          directories.push(args);
          throw new Error('directory creation must not run before verification succeeds');
        },
      }),
      /planted extracted-app proof failure/,
    );
    assert.deepEqual(
      await contents(paths),
      ['old archive', 'old manifest', 'old checksum', 'old delivery'],
      'verification failure published or overwrote an artifact',
    );
    assert.deepEqual(directories, [], 'verification failure attempted to create a release directory');
    assert.deepEqual(moves, [], 'verification failure attempted to publish an artifact');
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('a verified staged release publishes its inner checksum before the delivery', async () => {
  const { root, paths } = await fixture();
  const observations = [];
  try {
    await publishVerifiedAlphaArtifacts({
      ...paths,
      verify: async (manifest) => {
        observations.push(['verified', manifest]);
      },
      ensureDirectory: async (directory, options) => {
        observations.push(['directory', directory, options]);
      },
      move: async (source, destination) => {
        observations.push(['move', source, destination]);
        const { rename } = await import('node:fs/promises');
        await rename(source, destination);
      },
    });
    observations.push(['published', ...(await contents(paths))]);
    assert.deepEqual(observations, [
      ['verified', paths.stagedManifest],
      ['directory', root, { recursive: true }],
      ['move', paths.stagedArchive, paths.archive],
      ['move', paths.stagedManifest, paths.manifest],
      ['move', paths.stagedChecksum, paths.checksumFile],
      ['move', paths.stagedDelivery, paths.delivery],
      ['published', 'new archive', 'new manifest', 'new checksum', 'new delivery'],
    ]);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('a verified staged release creates a missing artifact directory', async () => {
  const { root, paths } = await fixture();
  const nested = join(root, 'clean-checkout', 'bundle', 'macos');
  const destinations = {
    archive: join(nested, 'release.zip'),
    manifest: join(nested, 'release.json'),
    checksumFile: join(nested, 'release.sha256'),
    delivery: join(nested, 'release-delivery.zip'),
  };
  try {
    await publishVerifiedAlphaArtifacts({
      ...paths,
      ...destinations,
      verify: async () => {},
    });
    assert.deepEqual(
      await contents(destinations),
      ['new archive', 'new manifest', 'new checksum', 'new delivery'],
      'a clean checkout could not receive the verified release',
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('delivery publication paths are an all-or-none pair', async () => {
  const { root, paths } = await fixture();
  try {
    await assert.rejects(
      publishVerifiedAlphaArtifacts({
        ...paths,
        delivery: undefined,
        verify: async () => {},
      }),
      /staged and published alpha delivery paths must be supplied together/,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
