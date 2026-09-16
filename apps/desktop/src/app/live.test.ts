// The window, over a REAL socket, against a service that really answers.
//
// Every assertion below is on the text a person would be looking at, produced by a window that
// opened a connection, negotiated a version, issued three versioned calls and read the replies.
// Nothing is stubbed below the socket.
//
// What this file cannot establish, and `apps/desktop/README.md` §2 says the same: that the RUST
// service behaves identically to the stand-in. The encodings are identical by construction — both
// sides round-trip `crates/mesh-daemon/ipc-contract.json` byte for byte — and the behaviours are
// checked by hand against `cargo run -p mesh-daemon --example serve`, with the frame that produced
// pasted into the pull request. A suite that shelled out to cargo would take longer than the whole
// gate budget and would not run on a machine with no toolchain.

import assert from 'node:assert/strict';
import { after, describe, it } from 'node:test';

import { FakeDaemon } from '../../test-support/fake-daemon.ts';
import { freshEndpoint, waitUntil } from '../../test-support/endpoint.ts';
import { frameText } from '../../test-support/frame.ts';
import { DaemonConnection } from '../ipc/client.ts';
import { CONNECTION_COPY } from '../strings/connection.ts';
import { WINDOW_COPY } from '../strings/window.ts';
import { LiveWindow } from './live.ts';

const teardown: (() => Promise<void>)[] = [];
after(async () => {
  for (const step of teardown) await step();
});

type Fixture = {
  readonly daemon: FakeDaemon;
  readonly connection: DaemonConnection;
  readonly window: LiveWindow;
  readonly frames: string[];
};

const fixture = async (name: string, options: { supported?: number[]; start?: boolean } = {}): Promise<Fixture> => {
  const endpoint = await freshEndpoint(name);
  const daemon = new FakeDaemon(endpoint.path, { supported: options.supported ?? [1] });
  if (options.start !== false) await daemon.start();
  const connection = new DaemonConnection({
    endpoint: endpoint.path,
    session: 'mesh-desktop',
    retryDelaysMs: [5, 10],
    context: { openWorkspace: 'design-system' },
  });
  const frames: string[] = [];
  const window = new LiveWindow({
    connection,
    endpoint: endpoint.path,
    write: (text) => frames.push(text),
  });
  teardown.push(async () => {
    connection.stop();
    await daemon.stop();
    await endpoint.cleanup();
  });
  return { daemon, connection, window, frames };
};

describe('the window against a running background service', () => {
  it('shows what the service answered, over the socket, in the frame', async () => {
    const { connection, window, frames } = await fixture('running');
    window.start();
    await connection.start();
    await waitUntil('the service has answered', () => window.view.facts !== null);

    const frame = window.lastFrame;
    const text = frameText(frame);
    assert.ok(text.includes(CONNECTION_COPY.connected), `the link is not reported:\n${frame}`);
    assert.ok(text.includes('0 saved changes were read back.'), `the service’s sentence is missing:\n${frame}`);
    assert.ok(text.includes('version 1'), 'the negotiated interface version is missing');
    assert.equal(text.match(new RegExp(WINDOW_COPY.operationAvailable, 'g'))?.length, 3, 'not every operation was matched');
    assert.ok(frames.length >= 2, 'the window drew nothing while the connection was opening');
    assert.equal(frames.at(-1), frame, 'the last thing written is not the last frame composed');
  });

  it('reads every value through the IPC surface and holds no state of its own', async () => {
    const { daemon, connection, window } = await fixture('through-ipc');
    window.start();
    await connection.start();
    await waitUntil('the service has answered', () => window.view.facts !== null);
    // Three operations, three versioned calls, and nothing else got the window its numbers.
    assert.equal(daemon.callsAnswered, 3);
    assert.equal(connection.stats().negotiatedVersion, 1);
  });

  it('says the service is not running when nothing is listening', async () => {
    const { window, connection } = await fixture('not-running', { start: false });
    window.start();
    await connection.start();
    await waitUntil('the first attempt has failed', () => connection.state === 'reconnecting');
    window.render();
    assert.ok(frameText(window.lastFrame).includes('is not running on this device'), `wrong wording:\n${window.lastFrame}`);
    assert.equal(window.view.facts, null, 'a reading was invented for a service that never answered');
  });

  it('says the two ends are too far apart in age when they cannot agree', async () => {
    const { window, connection } = await fixture('mismatch', { supported: [99] });
    window.start();
    await connection.start();
    await waitUntil('the mismatch is settled', () => connection.state === 'unusable');
    assert.ok(frameText(window.lastFrame).includes('too far apart in age'), `wrong wording:\n${window.lastFrame}`);
  });

  describe('when the service restarts under it', () => {
    it('keeps the last reading and the user context, then reconnects and reads again', async () => {
      const { daemon, connection, window } = await fixture('restart');
      window.start();
      await connection.start();
      await waitUntil('the service has answered', () => window.view.facts !== null);
      const contextBefore = connection.context;

      await daemon.stop();
      await waitUntil('the outage is noticed', () => connection.state === 'reconnecting');
      const duringOutage = frameText(window.lastFrame);
      assert.ok(duringOutage.includes('Reconnecting to Mesh.'), `the outage is not reported:\n${duringOutage}`);
      assert.ok(duringOutage.includes('0 saved changes were read back.'), 'the last reading was thrown away');

      await daemon.start();
      await waitUntil('the link is back', () => connection.state === 'connected');
      await window.refresh();
      assert.ok(frameText(window.lastFrame).includes(CONNECTION_COPY.connected), 'the window did not come back');
      assert.equal(connection.context, contextBefore, 'the context object was replaced across the outage');
      assert.equal(connection.context['openWorkspace'], 'design-system');
      assert.deepEqual(daemon.sessionsSeen, ['mesh-desktop'], 'the window did not re-announce its session');
    });
  });

  it('writes one frame on demand when it was asked for one reading', async () => {
    const endpoint = await freshEndpoint('one-reading');
    const daemon = new FakeDaemon(endpoint.path);
    await daemon.start();
    const connection = new DaemonConnection({ endpoint: endpoint.path, session: 'mesh-desktop', retryDelaysMs: [5] });
    const frames: string[] = [];
    const window = new LiveWindow({
      connection,
      endpoint: endpoint.path,
      write: (text) => frames.push(text),
      staysOpen: false,
      drawsEveryChange: false,
    });
    teardown.push(async () => {
      connection.stop();
      await daemon.stop();
      await endpoint.cleanup();
    });

    window.start();
    await connection.start();
    await waitUntil('the service has answered', () => window.view.facts !== null);
    assert.deepEqual(frames, [], 'a window asked for one reading drew while it was connecting');

    window.freeze();
    window.flush();
    assert.equal(frames.length, 1, 'one reading is not one frame');
    assert.ok(frameText(frames[0] ?? '').includes(WINDOW_COPY.singleFrameHint), 'the footer is for a window that stays open');
  });
});
