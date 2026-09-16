// "IPC is local-only: no network listener is opened by the desktop app."
//
// Two instruments, because either one alone is easy to fool:
//
//  1. **A text scan of every shipped source.** Catches the network modules and the listener call
//     before they can run at all. Cheap, total, and blind to anything reached indirectly.
//  2. **A reading of Node's own live resource table while a connection is open.** This is the
//     stronger one. `process.getActiveResourcesInfo()` is a public, stable API that names every
//     handle THIS PROCESS holds: a Unix-domain socket appears as `PipeWrap`, a TCP listener as
//     `TCPServerWrap` and a TCP connection as `TCPWrap`. So the assertion is not "no source
//     mentions TCP" but "this process, right now, with a live connection to the background
//     service and a live stand-in service in the same process, holds no TCP or UDP handle at all".
//
// What neither instrument establishes: that the OPERATING SYSTEM sees no listening port for this
// process. That is a `lsof`/`ss` question, it is platform-specific, and it would put a shell-out
// into a gate that has to stay hermetic and under thirty seconds. The resource table is the
// process's own answer to the same question and it is the one used here.
//
// Note what the second instrument covers that the scan cannot: the stand-in service in
// `test-support/fake-daemon.ts` DOES open a listener, on purpose, and it still passes — because
// it listens on a filesystem path. A test that failed on any listener would be measuring the
// wrong thing; this one measures the thing the criterion is about.

import assert from 'node:assert/strict';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { after, describe, it } from 'node:test';

import { FakeDaemon } from '../../test-support/fake-daemon.ts';
import { freshEndpoint, waitUntil } from '../../test-support/endpoint.ts';
import { DaemonConnection } from './client.ts';

const HERE = dirname(fileURLToPath(import.meta.url));
const SRC = join(HERE, '..');
const APP_ROOT = join(SRC, '..');
const SELF = fileURLToPath(import.meta.url);

/** Every shipped `.ts` file under `src/`. */
const shipped = (): string[] => {
  const found: string[] = [];
  const walk = (directory: string): void => {
    for (const entry of readdirSync(directory).sort()) {
      const path = join(directory, entry);
      if (statSync(path).isDirectory()) walk(path);
      else if (entry.endsWith('.ts') && !entry.endsWith('.test.ts')) found.push(path);
    }
  };
  walk(SRC);
  return found;
};

/** Network modules and the calls that open a listener. */
const NETWORK_MARKERS = ['node:http', 'node:https', 'node:dgram', 'node:tls', 'node:cluster', 'createServer', '.listen('];

/** Resource-table prefixes that mean a network handle. */
const NETWORK_HANDLES = ['TCP', 'UDP'];

const teardown: (() => Promise<void>)[] = [];
after(async () => {
  for (const step of teardown) await step();
});

describe('the desktop application opens no network listener', () => {
  it('names no network module and opens no listener, in any shipped source', () => {
    const files = shipped();
    assert.ok(files.length >= 5, `only ${files.length} shipped sources found — the walk broke`);
    for (const file of files) {
      if (file === SELF) continue; // the scanner names what it scans for.
      const text = readFileSync(file, 'utf8');
      for (const marker of NETWORK_MARKERS) {
        assert.ok(
          !text.includes(marker),
          `${relative(APP_ROOT, file)} names \`${marker}\`: this application connects, it never listens`,
        );
      }
    }
  });

  it('holds no TCP or UDP handle while a connection is open', async () => {
    // Before: whatever the test runner itself holds. Recorded so the assertion below is about
    // this application and not about an inherited handle nobody here created.
    const before = process.getActiveResourcesInfo();
    assert.deepEqual(
      before.filter((entry) => NETWORK_HANDLES.some((prefix) => entry.startsWith(prefix))),
      [],
      'the test runner already held a network handle; this measurement would be meaningless',
    );

    const endpoint = await freshEndpoint('no-network');
    const daemon = new FakeDaemon(endpoint.path);
    await daemon.start();
    const connection = new DaemonConnection({
      endpoint: endpoint.path,
      session: 'desktop-01',
      retryDelaysMs: [5, 10],
    });
    teardown.push(async () => {
      connection.stop();
      await daemon.stop();
      await endpoint.cleanup();
    });

    await connection.start();
    await waitUntil('the connection is ready', () => connection.state === 'connected');
    await connection.call('daemon.status');

    const during = process.getActiveResourcesInfo();
    assert.deepEqual(
      during.filter((entry) => NETWORK_HANDLES.some((prefix) => entry.startsWith(prefix))),
      [],
      `a network handle appeared: ${JSON.stringify(during)}`,
    );
    // And the connection really was live, so the measurement above was not taken over nothing.
    assert.ok(
      during.some((entry) => entry.startsWith('Pipe')),
      `no local socket handle was open, so this test proved nothing: ${JSON.stringify(during)}`,
    );
  });
});
