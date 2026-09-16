import assert from 'node:assert/strict';
import { createServer, type Server, type Socket } from 'node:net';
import { after, describe, it } from 'node:test';

import { freshEndpoint } from '../../test-support/endpoint.ts';
import { MAX_LINE_BYTES, WireError } from './protocol.ts';
import { connectToLocalSocket } from './transport.ts';

const servers: Server[] = [];
const cleanups: (() => Promise<void>)[] = [];

after(async () => {
  for (const server of servers) {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  for (const cleanup of cleanups) await cleanup();
});

describe('the local socket transport', () => {
  it('closes instead of retaining an oversized unfinished frame', async () => {
    const endpoint = await freshEndpoint('frame');
    cleanups.push(endpoint.cleanup);

    let resolvePeerClosed: (() => void) | undefined;
    const peerClosed = new Promise<void>((resolve) => {
      resolvePeerClosed = resolve;
    });
    const server = createServer((socket) => {
      socket.on('close', () => resolvePeerClosed?.());
      socket.write('x'.repeat(MAX_LINE_BYTES));
    });
    servers.push(server);
    await new Promise<void>((resolve, reject) => {
      server.once('error', reject);
      server.listen(endpoint.path, () => {
        server.off('error', reject);
        resolve();
      });
    });

    const link = await connectToLocalSocket(endpoint.path);
    const reason = await new Promise<Error | null>((resolve) => link.onClosed(resolve));

    assert(reason instanceof WireError);
    assert.equal(reason.code, 'line-too-long');
    await peerClosed;
  });

  it('closes on malformed UTF-8 before delivering a replacement-character message', async () => {
    const endpoint = await freshEndpoint('invalid-utf8');
    cleanups.push(endpoint.cleanup);

    let acceptPeer: ((socket: Socket) => void) | undefined;
    const peer = new Promise<Socket>((resolve) => {
      acceptPeer = resolve;
    });
    const server = createServer((socket) => acceptPeer?.(socket));
    servers.push(server);
    await new Promise<void>((resolve, reject) => {
      server.once('error', reject);
      server.listen(endpoint.path, () => {
        server.off('error', reject);
        resolve();
      });
    });

    const link = await connectToLocalSocket(endpoint.path);
    const delivered: string[] = [];
    link.onLine((line) => delivered.push(line));
    const closed = new Promise<Error | null>((resolve) => link.onClosed(resolve));
    const socket = await peer;
    socket.end(
      Buffer.concat([
        Buffer.from('{"t":"failed","id":1,"code":"bad-utf8","message":"', 'utf8'),
        Buffer.from([0xff]),
        Buffer.from('"}\n', 'utf8'),
      ]),
    );

    const reason = await closed;
    assert(reason instanceof WireError);
    assert.equal(reason.code, 'malformed-message');
    assert.deepEqual(delivered, []);
  });

  it('closes on a final frame that never reaches its required newline', async () => {
    const endpoint = await freshEndpoint('missing-newline');
    cleanups.push(endpoint.cleanup);

    const server = createServer((socket) => {
      socket.end('{"t":"result","id":1,"value":{"serving":true}}');
    });
    servers.push(server);
    await new Promise<void>((resolve, reject) => {
      server.once('error', reject);
      server.listen(endpoint.path, () => {
        server.off('error', reject);
        resolve();
      });
    });

    const link = await connectToLocalSocket(endpoint.path);
    const delivered: string[] = [];
    link.onLine((line) => delivered.push(line));
    const reason = await new Promise<Error | null>((resolve) => link.onClosed(resolve));

    assert(reason instanceof WireError);
    assert.equal(reason.code, 'malformed-message');
    assert.match(reason.message, /before its newline/u);
    assert.deepEqual(delivered, [], 'an unterminated frame reached the protocol decoder');
  });
});
