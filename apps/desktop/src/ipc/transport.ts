// The one place in this application that opens a socket.
//
// Everything above this file talks to the `Duplex` interface, so the connection lifecycle in
// `client.ts` can be exercised without a socket and — more importantly — so the claim "this
// application opens no network listener" has ONE file to be true of rather than a codebase.
// `no-network.test.ts` checks that claim two ways: it scans every source under `src/` for the
// network modules, and it reads Node's own live resource table while a connection is open and
// asserts every handle is a pipe.
//
// The endpoint is a filesystem path. `net.createConnection(path)` opens a Unix-domain socket;
// the port-taking form of the same function is never called, and the scan in `no-network.test.ts`
// is what keeps that true.

import { createConnection, type Socket } from 'node:net';

import { MAX_LINE_BYTES, WireError, splitLines } from './protocol.ts';

/** A line-oriented, two-way connection. The seam the client is written against. */
export type Duplex = {
  /** Send one line. The newline is added here. */
  readonly send: (line: string) => void;
  /** Stop reading and close. Safe to call more than once. */
  readonly close: () => void;
  /** Every whole line that arrives. */
  readonly onLine: (handler: (line: string) => void) => void;
  /** Called once when the connection ends, for any reason, including a clean close. */
  readonly onClosed: (handler: (reason: Error | null) => void) => void;
};

/** Open a connection to `endpoint`. The seam a test or a later transport replaces. */
export type Connector = (endpoint: string) => Promise<Duplex>;

/**
 * Connect to a Unix-domain socket at `endpoint`.
 *
 * Rejects if the socket cannot be reached — which is the normal state while the background service
 * is starting, and is why `client.ts` treats a rejection as "retry", not as "fail".
 */
export const connectToLocalSocket: Connector = (endpoint: string): Promise<Duplex> =>
  new Promise<Duplex>((resolve, reject) => {
    const socket: Socket = createConnection(endpoint);

    let settled = false;
    let closed = false;
    let buffered = '';
    let pendingLineBytes = 0;
    const utf8 = new TextDecoder('utf-8', { fatal: true });
    const lineHandlers: ((line: string) => void)[] = [];
    const closeHandlers: ((reason: Error | null) => void)[] = [];
    let closedWith: Error | null = null;

    const announceClosed = (reason: Error | null): void => {
      if (closed) return;
      closed = true;
      closedWith = reason;
      for (const handler of closeHandlers) handler(reason);
    };

    const closeUnreadable = (error: unknown): void => {
      buffered = '';
      pendingLineBytes = 0;
      announceClosed(error instanceof Error ? error : new Error('the local IPC frame was unreadable'));
      socket.destroy();
    };

    socket.on('data', (chunk: Buffer) => {
      for (const byte of chunk) {
        if (byte === 0x0a) {
          pendingLineBytes = 0;
        } else {
          pendingLineBytes += 1;
          if (pendingLineBytes >= MAX_LINE_BYTES) {
            closeUnreadable(
              new WireError('line-too-long', `the unfinished line is over the ${MAX_LINE_BYTES} byte limit`),
            );
            return;
          }
        }
      }
      try {
        buffered += utf8.decode(chunk, { stream: true });
      } catch {
        closeUnreadable(new WireError('malformed-message', 'the local IPC frame is not valid UTF-8'));
        return;
      }
      let split: ReturnType<typeof splitLines>;
      try {
        split = splitLines(buffered);
      } catch (error) {
        closeUnreadable(error);
        return;
      }
      const { lines, rest } = split;
      buffered = rest;
      for (const line of lines) {
        for (const handler of lineHandlers) handler(line);
      }
    });

    socket.on('end', () => {
      try {
        buffered += utf8.decode();
      } catch {
        closeUnreadable(new WireError('malformed-message', 'the local IPC frame is not valid UTF-8'));
        return;
      }
      if (buffered.length > 0 || pendingLineBytes > 0) {
        closeUnreadable(
          new WireError('malformed-message', 'the local IPC frame ended before its newline'),
        );
      }
    });

    socket.on('error', (error: Error) => {
      if (!settled) {
        settled = true;
        reject(error);
        return;
      }
      announceClosed(error);
    });

    socket.on('close', () => {
      announceClosed(null);
    });

    socket.once('connect', () => {
      settled = true;
      resolve({
        send: (line: string) => {
          socket.write(`${line}\n`);
        },
        close: () => {
          socket.destroy();
        },
        onLine: (handler) => {
          lineHandlers.push(handler);
        },
        onClosed: (handler) => {
          // A handler registered after the connection already ended still hears about it, so a
          // race between `connect` resolving and the peer disappearing cannot lose the event.
          if (closed) handler(closedWith);
          else closeHandlers.push(handler);
        },
      });
    });
  });
