import assert from 'node:assert/strict';
import { lstat, mkdir, readdir, realpath } from 'node:fs/promises';
import { isAbsolute, join, relative, sep } from 'node:path';

function contains(parent, child) {
  const path = relative(parent, child);
  return path === '' || (path !== '..' && !path.startsWith(`..${sep}`) && !isAbsolute(path));
}

export async function mirrorOrdinaryDirectories(source, destination, {
  excludedRootNames = [],
} = {}) {
  const [sourceMetadata, destinationMetadata] = await Promise.all([
    lstat(source),
    lstat(destination),
  ]);
  assert.ok(
    sourceMetadata.isDirectory() && !sourceMetadata.isSymbolicLink(),
    'private-export proof source root must be one real directory',
  );
  assert.ok(
    destinationMetadata.isDirectory() && !destinationMetadata.isSymbolicLink(),
    'private-export proof destination root must be one real directory',
  );
  const [canonicalSource, canonicalDestination] = await Promise.all([
    realpath(source),
    realpath(destination),
  ]);
  assert.ok(
    !contains(canonicalSource, canonicalDestination)
      && !contains(canonicalDestination, canonicalSource),
    'private-export proof source and destination roots must be disjoint',
  );
  const excluded = new Set(excludedRootNames);
  const visit = async (relative) => {
    const entries = await readdir(join(canonicalSource, relative), { withFileTypes: true });
    for (const entry of entries) {
      if (!relative && excluded.has(entry.name)) continue;
      const child = relative ? join(relative, entry.name) : entry.name;
      assert.ok(
        entry.isFile() || entry.isDirectory(),
        `private-export proof source contained a non-ordinary entry at ${child}`,
      );
      if (!entry.isDirectory()) continue;
      await mkdir(join(canonicalDestination, child), { mode: 0o700 });
      await visit(child);
    }
  };
  await visit('');
}
