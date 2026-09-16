// A per-test endpoint under an owner-only directory, and the wait helper the timing tests need.

import { mkdtemp, chmod, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

/** A fresh, owner-only directory with a socket path inside it. */
export const freshEndpoint = async (name: string): Promise<{ path: string; cleanup: () => Promise<void> }> => {
  const directory = await mkdtemp(join(tmpdir(), `mesh-desktop-${name}-`));
  await chmod(directory, 0o700);
  return {
    path: join(directory, 'daemon.sock'),
    cleanup: () => rm(directory, { recursive: true, force: true }),
  };
};

/** Sleep. Used only to let a scheduled reconnection run, never to order anything. */
export const pause = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Wait until `predicate` holds, or fail loudly.
 *
 * Polling rather than a fixed sleep: a fixed sleep is either flaky or slow, and a timeout that
 * throws with the caller's own description is the difference between a useful failure and
 * "expected true to be false".
 */
export const waitUntil = async (
  what: string,
  predicate: () => boolean,
  timeoutMs = 4000,
): Promise<void> => {
  const deadline = Date.now() + timeoutMs;
  while (!predicate()) {
    if (Date.now() > deadline) throw new Error(`timed out after ${timeoutMs} ms waiting until ${what}`);
    await pause(2);
  }
};
