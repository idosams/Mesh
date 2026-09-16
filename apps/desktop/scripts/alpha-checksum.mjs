import assert from 'node:assert/strict';

export function canonicalAlphaChecksum(record) {
  return `${record.sha256}  ${record.artifact}\n`;
}

export function assertCanonicalAlphaChecksum(bytes, record) {
  assert.equal(
    bytes,
    canonicalAlphaChecksum(record),
    'the alpha checksum file must canonically bind the exact manifest digest and artifact basename',
  );
}
