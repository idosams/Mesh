// The Mesh window, started.
//
//   npm --prefix apps/desktop start
//   npm --prefix apps/desktop start -- --endpoint /tmp/mesh/daemon.sock --once
//
// This is the whole of the entry point: read the arguments, open a connection, draw. Everything it
// does is in `src/app/live.ts` and `src/app/window.ts`, so that the day a Tauri host replaces the
// terminal there is one file to delete rather than a program to disentangle.
//
// It imports nothing but relative modules — `src/app/architecture.test.ts` holds every shipped
// source to that — and uses the `process` global for the three things a command-line program
// needs: its arguments, its environment and its output. It opens no listener and reaches no
// database; both are checked, in `src/ipc/no-network.test.ts` and `src/app/architecture.test.ts`.
//
// Exit codes, because a person may put this in a script:
//   0  the window connected (or stayed open until it was closed)
//   1  `--once` and the service did not answer in time
//   2  the arguments could not be read

import { DaemonConnection } from './ipc/client.ts';
import { LiveWindow } from './app/live.ts';
import { namedArgument, resolveEndpoint } from './app/endpoint.ts';
import { USAGE } from './strings/window.ts';

/** The session name this window announces when nothing says otherwise. */
const DEFAULT_SESSION = 'mesh-desktop';

/** How long `--once` waits for a first answer, in milliseconds. */
const DEFAULT_WAIT_MS = 2000;

/** How often an open window asks the service for a fresh reading, in milliseconds. */
const REFRESH_MS = 2000;

const pause = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

/** A whole number of milliseconds from an argument, or the default. */
const waitFrom = (argv: readonly string[]): number => {
  const given = namedArgument(argv, '--wait-ms');
  if (given === null) return DEFAULT_WAIT_MS;
  const value = Number(given);
  if (!Number.isSafeInteger(value) || value < 0) throw new Error('`--wait-ms` needs a whole number of milliseconds.');
  return value;
};

const main = async (): Promise<void> => {
  const argv: string[] = process.argv.slice(2);
  if (argv.includes('--help') || argv.includes('-h')) {
    process.stdout.write(`${USAGE}\n`);
    return;
  }

  let endpoint: string;
  let session: string;
  let waitMs: number;
  try {
    endpoint = resolveEndpoint(argv, process.env);
    session = namedArgument(argv, '--session') ?? DEFAULT_SESSION;
    waitMs = waitFrom(argv);
  } catch (error) {
    process.stderr.write(`${(error as Error).message}\n\n${USAGE}\n`);
    process.exitCode = 2;
    return;
  }

  const once = argv.includes('--once');
  const connection = new DaemonConnection({ endpoint, session, context: {} });
  const window = new LiveWindow({
    connection,
    endpoint,
    write: (text) => {
      process.stdout.write(text);
    },
    staysOpen: !once,
    // Redrawing over the previous frame only makes sense on a terminal. Piped into a file or a
    // test, the frames are kept one after another, which is what a reader of that file wants.
    clearScreen: !once && process.stdout.isTTY === true,
    drawsEveryChange: !once,
  });
  window.start();
  await connection.start();

  if (once) {
    const deadline = Date.now() + waitMs;
    while (Date.now() < deadline && connection.state !== 'connected' && connection.state !== 'unusable') {
      await pause(20);
    }
    await window.refresh();
    // Freeze before closing: `stop()` moves the connection to `stopped`, and a person who asked
    // for one reading should be left looking at the reading, not at "Mesh is closed".
    window.freeze();
    window.flush();
    connection.stop();
    process.exitCode = connection.state === 'stopped' && window.view.facts !== null ? 0 : 1;
    return;
  }

  const timer = setInterval(() => {
    void window.refresh();
  }, REFRESH_MS);
  const close = (): void => {
    clearInterval(timer);
    window.freeze();
    connection.stop();
    process.stdout.write('\n');
    process.exit(0);
  };
  process.on('SIGINT', close);
  process.on('SIGTERM', close);
};

void main();
