// What the running background service said about itself, read at the boundary.
//
// Three catalogue methods answer three objects, and everything below is the one place their fields
// are turned into something this application will render. It is a BOUNDARY: the values arrived
// over a socket from another process, so every field is checked rather than assumed, and a shape
// this application does not recognise raises a reviewed sentence instead of rendering `undefined`
// at somebody.
//
// The rule this follows is the one `src/app/status.ts` states for internal states: no silent
// fallback. A missing field is a fault in one of the two implementations of the surface, and the
// day it happens the window says so rather than showing a plausible-looking blank.

import type { WireObject, WireValue } from '../ipc/protocol.ts';
import { ERROR_COPY } from '../strings/errors.ts';

/** What the window knows about the service it is connected to. */
export type ServiceFacts = {
  /** Whether the service can answer requests at all. */
  readonly serving: boolean;
  /** The version of the service interface the running service implements. */
  readonly surfaceVersion: number;
  /** What the service said about its last start-up, in its own words. */
  readonly startupSentence: string;
  /** How long that start-up took, in milliseconds. */
  readonly startupElapsedMs: number;
  /** Every method the running service publishes, in the order it publishes them. */
  readonly methodNames: readonly string[];
};

/** Raised when the service answered something this application cannot read. */
export class UnreadableService extends Error {
  /** Which field could not be read. Carried on a field, never interpolated into the sentence. */
  readonly field: string;

  constructor(field: string) {
    super(ERROR_COPY.unreadableService());
    this.name = 'UnreadableService';
    this.field = field;
  }
}

const flag = (source: WireObject, key: string): boolean => {
  const value = source[key];
  if (typeof value !== 'boolean') throw new UnreadableService(key);
  return value;
};

const count = (source: WireObject, key: string): number => {
  const value = source[key];
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0) {
    throw new UnreadableService(key);
  }
  return value;
};

const text = (source: WireObject, key: string): string => {
  const value = source[key];
  if (typeof value !== 'string' || value.length === 0) throw new UnreadableService(key);
  return value;
};

const methodNames = (described: WireObject): readonly string[] => {
  const value: WireValue | undefined = described['methods'];
  if (!Array.isArray(value)) throw new UnreadableService('methods');
  return value.map((entry) => {
    if (typeof entry !== 'object' || entry === null || Array.isArray(entry)) {
      throw new UnreadableService('methods');
    }
    return text(entry as WireObject, 'name');
  });
};

/**
 * Read the three answers into one set of facts.
 *
 * The surface version is taken from `daemon.status` rather than from `surface.describe`, because
 * `daemon.status` is the cheapest call and the one a window would keep asking; the two are
 * required to agree and this checks that they do, since a service answering two different numbers
 * for one question is a fault worth surfacing rather than averaging.
 */
export const factsFrom = (status: WireObject, startup: WireObject, described: WireObject): ServiceFacts => {
  const surfaceVersion = count(status, 'surface_version');
  if (count(described, 'surface_version') !== surfaceVersion) throw new UnreadableService('surface_version');
  return {
    serving: flag(status, 'serving'),
    surfaceVersion,
    startupSentence: text(startup, 'sentence'),
    startupElapsedMs: count(startup, 'elapsed_ms'),
    methodNames: methodNames(described),
  };
};
