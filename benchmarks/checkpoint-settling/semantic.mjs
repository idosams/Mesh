import { createHash } from "node:crypto";

import { FIXTURE, fixtureEntries } from "./fixture.mjs";

export function fixtureSemanticOutcome(tree) {
  const prefix = `node_modules/${FIXTURE.package_name}/`;
  const expected = new Map(
    fixtureEntries()
      .filter(([path]) => path.startsWith("files/"))
      .map(([path, bytes]) => [`${prefix}${path}`, bytes]),
  );
  const actual = new Map(
    [...tree]
      .filter(([path, row]) => path.startsWith(`${prefix}files/`) && !row.dir)
      .map(([path, row]) => [path, row.bytes]),
  );
  let missing = 0;
  let unexpected = 0;
  let mismatched = 0;
  for (const [path, bytes] of expected) {
    const installed = actual.get(path);
    if (!installed) missing += 1;
    else if (!Buffer.isBuffer(installed) || !bytes.equals(installed)) mismatched += 1;
  }
  for (const path of actual.keys()) if (!expected.has(path)) unexpected += 1;
  const expectedDigest = digestEntries(expected);
  const installedDigest = digestEntries(actual);
  return {
    installed_fixture_files: actual.size,
    expected_fixture_files: expected.size,
    missing_fixture_files: missing,
    unexpected_fixture_files: unexpected,
    mismatched_fixture_files: mismatched,
    expected_fixture_tree_digest: expectedDigest,
    installed_fixture_tree_digest: installedDigest,
    fixture_bytes_verified: missing === 0 && unexpected === 0 && mismatched === 0 && installedDigest === expectedDigest,
  };
}

function digestEntries(entries) {
  const hash = createHash("sha256");
  for (const [path, bytes] of [...entries].sort(([a], [b]) => a.localeCompare(b))) {
    hash.update(path).update("\0").update(bytes).update("\0");
  }
  return `sha256:${hash.digest("hex")}`;
}
