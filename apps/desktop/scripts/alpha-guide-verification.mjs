import assert from 'node:assert/strict';
import { lstat, readFile } from 'node:fs/promises';

export function assertUniqueArchiveEntries(entries, label) {
  assert.equal(
    new Set(entries).size,
    entries.length,
    `${label} must not contain duplicate paths`,
  );
}

export async function assertRegularExactFile(actual, expected, label) {
  const stat = await lstat(actual);
  assert.equal(
    stat.isFile() && !stat.isSymbolicLink(),
    true,
    `${label} must be a regular non-link file`,
  );
  const bytes = await readFile(actual);
  assert.equal(
    bytes.equals(await readFile(expected)),
    true,
    `${label} must match the reviewed source`,
  );
  return bytes;
}
