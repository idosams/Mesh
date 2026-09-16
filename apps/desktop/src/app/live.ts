// The live window: everything impure that stands between a socket and `renderWindow`.
//
// # What this is for
//
// `src/ipc/client.ts` knows how to hold a connection open. `src/app/operations.ts` knows which
// versioned call each interface operation is. `src/app/window.ts` knows how to draw. Nothing put
// the three together, so nothing in this application could be STARTED — every caller of it was a
// test. This file is the missing caller, and `src/main.ts` is nine lines of argument handling on
// top of it.
//
// # Every reading comes over IPC, and there is no second path
//
// The three calls below go through `run()`, which goes through `DaemonConnection.call`, which is
// the only door. That is the acceptance criterion *"every UI operation maps to a versioned IPC
// call"* holding at run time and not only in a table: this window has no other way to learn
// anything, because `src/app/architecture.test.ts` fails the build if a source under `src/` names
// a database, a store or another process.
//
// # A refusal to invent a reading
//
// When the service goes away, the calls this window has already issued are HELD rather than
// rejected — see the note at the top of `client.ts` — so a refresh in progress simply does not
// come back until the service does. The window keeps showing the LAST reading it understood, and
// the connection line above it says the link is down. It never ages a reading into a guess, and
// `#refreshing` stops a stalled refresh from queueing another behind it.

import { DaemonConnection, type ConnectionState } from '../ipc/client.ts';
import { factsFrom, type ServiceFacts } from './facts.ts';
import { run } from './operations.ts';
import { connectionSentence, renderWindow, type WindowView } from './window.ts';

/** Move the cursor home and clear what was there, so a redraw looks like one window. */
export const CLEAR_SCREEN = '\u001B[2J\u001B[H';

/** How to build a live window. */
export type LiveOptions = {
  /** The connection to draw from. Started by the caller, not here. */
  readonly connection: DaemonConnection;
  /** The endpoint being watched, for the frame. */
  readonly endpoint: string;
  /** Where a frame goes. The seam a test replaces with an array. */
  readonly write: (text: string) => void;
  /** Whether this window stays open. Changes the footer only. */
  readonly staysOpen?: boolean;
  /** Whether to clear the screen before each frame. False when the output is not a terminal. */
  readonly clearScreen?: boolean;
  /**
   * Whether every change is written out as it happens.
   *
   * True for a window somebody is watching. False for one reading, where four frames of a
   * connection opening are noise around the one frame that was asked for; {@link LiveWindow.flush}
   * writes the frame that matters.
   */
  readonly drawsEveryChange?: boolean;
};

/** A window bound to one connection, redrawn whenever something it shows changes. */
export class LiveWindow {
  readonly #connection: DaemonConnection;
  readonly #endpoint: string;
  readonly #write: (text: string) => void;
  readonly #staysOpen: boolean;
  readonly #clearScreen: boolean;
  readonly #drawsEveryChange: boolean;

  #state: ConnectionState = 'idle';
  #everConnected = false;
  #facts: ServiceFacts | null = null;
  #fault: string | null = null;
  #refreshing = false;
  #frozen = false;
  #frames = 0;
  #lastFrame = '';

  constructor(options: LiveOptions) {
    this.#connection = options.connection;
    this.#endpoint = options.endpoint;
    this.#write = options.write;
    this.#staysOpen = options.staysOpen ?? true;
    this.#clearScreen = options.clearScreen ?? false;
    this.#drawsEveryChange = options.drawsEveryChange ?? true;
  }

  /** What the window would draw right now. */
  get view(): WindowView {
    const view: WindowView = {
      endpoint: this.#endpoint,
      connection: this.#state,
      everConnected: this.#everConnected,
      facts: this.#facts,
      fault: this.#fault,
      staysOpen: this.#staysOpen,
    };
    // A fault that says the same thing as the connection line is not a second piece of news.
    return this.#fault !== null && this.#fault === connectionSentence(view) ? { ...view, fault: null } : view;
  }

  /** How many frames have been drawn. */
  get frames(): number {
    return this.#frames;
  }

  /** The last frame drawn, so a test can read what a person would see. */
  get lastFrame(): string {
    return this.#lastFrame;
  }

  /** Subscribe to the connection and draw the first frame. */
  start(): void {
    this.#connection.onState((state) => this.#onState(state));
    this.#connection.onFault((message) => {
      this.#fault = message;
      this.render();
    });
    this.render();
  }

  /** Stop drawing. Used before a deliberate close, so the last frame stays on the screen. */
  freeze(): void {
    this.#frozen = true;
  }

  /**
   * Ask the service for everything this window shows, and draw the answer.
   *
   * Every failure becomes a sentence in the frame rather than an exception at the caller: this is
   * a window, and a person watching it has nowhere to catch anything.
   */
  async refresh(): Promise<void> {
    if (this.#refreshing || this.#state !== 'connected') return;
    this.#refreshing = true;
    try {
      const [status, startup, described] = await Promise.all([
        run(this.#connection, 'service.check'),
        run(this.#connection, 'service.startup'),
        run(this.#connection, 'service.describe'),
      ]);
      this.#facts = factsFrom(status, startup, described);
      this.#fault = null;
    } catch (error) {
      this.#fault = (error as Error).message;
    } finally {
      this.#refreshing = false;
    }
    this.render();
  }

  /** Compose one frame, and write it out unless this window was asked for one reading only. */
  render(): void {
    if (this.#frozen) return;
    this.#lastFrame = renderWindow(this.view);
    this.#frames += 1;
    if (this.#drawsEveryChange) this.#writeFrame();
  }

  /** Write the frame as it stands, whatever this window's drawing policy is. */
  flush(): void {
    if (this.#lastFrame.length === 0) this.#lastFrame = renderWindow(this.view);
    this.#writeFrame();
  }

  #writeFrame(): void {
    this.#write(this.#clearScreen ? `${CLEAR_SCREEN}${this.#lastFrame}` : this.#lastFrame);
  }

  #onState(state: ConnectionState): void {
    this.#state = state;
    if (state === 'connected') this.#everConnected = true;
    this.render();
    if (state === 'connected') void this.refresh();
  }
}
