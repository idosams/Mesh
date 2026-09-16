// Every word the Mesh window puts on the screen that is not already in `connection.ts`,
// `errors.ts`, `operations.ts` or `status.json`.
//
// Under `src/strings/` for the reason the rest of this directory is: `tools/program/vocab-lint/
// surfaces.json` lints every shipped `.ts` under `apps/desktop/src/` with an exemption budget of
// zero, and copy written inline in a renderer is copy nobody reviews. Run it:
//
//   node tools/program/vocab-lint/lint.mjs --user-facing
//
// The rule these words are written to: the internal model is a version history, the user model is
// six words, and nine internal terms may never reach a person. None of them is below, and the
// rendered frame is scanned for all nine in `src/app/window.test.ts` — including the sentences
// that arrive from the background service, which is where an unreviewed word would enter.
//
// Nothing here is one of the six status words. This window reports the LINK and the SERVICE, and
// neither is a state a piece of work can be in; borrowing one of the six would put a second
// meaning on a word that has exactly one.

/** Every label and sentence the window frame is built from. */
export const WINDOW_COPY = {
  /** The product name, at the top of the frame. */
  title: 'Mesh',

  /** The section about the link to the background service. */
  connectionHeading: 'Connection',
  /** The section about the service itself. */
  serviceHeading: 'Background service',
  /** The section listing what this window can ask for. */
  operationsHeading: 'What this window can ask for',

  /** Where the background service is listening. */
  endpointLabel: 'Local endpoint',
  /** Which version of the service interface is in force. */
  interfaceLabel: 'Service interface',
  /** Whether the service is answering. */
  servingLabel: 'Serving requests',
  /** What the service said about its own start-up. */
  startupLabel: 'Last start-up',
  /** How long that start-up took. */
  startupTookLabel: 'Start-up took',

  /** The service answers requests. */
  serving: 'Yes.',
  /** The service is up but cannot answer. */
  notServing: 'No. The workspace needs attention before Mesh can use it.',
  /** Shown in the service section while nothing has been answered yet. */
  nothingYet: 'Nothing has been answered yet.',
  /** Shown against an operation the running service offers. */
  operationAvailable: 'ready',
  /** Shown against an operation this version of the service does not have. */
  operationMissing: 'not in the running service',
  /** The footer of a window that keeps running. */
  quitHint: 'Press Ctrl-C to close this window.',
  /** The footer of a single frame. */
  singleFrameHint: 'One reading, taken just now.',
} as const;

/** How the window says a version number, so the digit is never on its own. */
export const versionWording = (version: number): string => `version ${version}`;

/** How the window says a duration, in the one unit the service reports. */
export const durationWording = (milliseconds: number): string => `${milliseconds} ms`;

/** What `--help` prints. Not a sentence a person reads inside the window, but one they read. */
export const USAGE = [
  'Open the Mesh window on a running Mesh background service.',
  '',
  '  npm --prefix apps/desktop start -- [OPTIONS]',
  '',
  '  --endpoint <path>   where the background service is listening; defaults to',
  '                      MESH_DAEMON_ENDPOINT, then to $HOME/.mesh/run/daemon.sock',
  '  --session <name>    the name this window announces; defaults to mesh-desktop',
  '  --once              draw one frame and exit, instead of staying open',
  '  --wait-ms <n>       how long --once waits for an answer; defaults to 2000',
  '  --help              this text',
  '',
  'Start the background service first with:',
  '  cargo run -p mesh-daemon --example serve',
].join('\n');

/** The keys of {@link WINDOW_COPY}, so a test can walk every word. */
export type WindowCopyKey = keyof typeof WINDOW_COPY;
