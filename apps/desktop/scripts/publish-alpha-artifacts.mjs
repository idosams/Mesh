import { mkdir, rename } from 'node:fs/promises';
import { dirname } from 'node:path';

/**
 * Verify a staged alpha archive before any public artifact path changes, then publish the
 * inner checksum after its archive and manifest, then publish the self-contained delivery last.
 * A failed verifier therefore leaves an earlier complete release untouched, while an interrupted
 * publication cannot present a new delivery package before all of its inner artifacts exist.
 */
export async function publishVerifiedAlphaArtifacts({
  stagedArchive,
  stagedManifest,
  stagedChecksum,
  stagedDelivery,
  archive,
  manifest,
  checksumFile,
  delivery,
  verify,
  move = rename,
  ensureDirectory = mkdir,
}) {
  assertOptionalPair(stagedDelivery, delivery);
  await verify(stagedManifest);
  for (const directory of new Set(
    [archive, manifest, checksumFile, delivery].filter(Boolean).map(dirname),
  )) {
    await ensureDirectory(directory, { recursive: true });
  }
  await move(stagedArchive, archive);
  await move(stagedManifest, manifest);
  await move(stagedChecksum, checksumFile);
  if (delivery) await move(stagedDelivery, delivery);
}

function assertOptionalPair(stagedDelivery, delivery) {
  if (Boolean(stagedDelivery) !== Boolean(delivery)) {
    throw new Error('the staged and published alpha delivery paths must be supplied together');
  }
}
