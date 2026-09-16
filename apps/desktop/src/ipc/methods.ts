// The method catalogue as this client knows it.
//
// Written as a literal rather than read from `crates/mesh-daemon/ipc-contract.json` at runtime,
// deliberately: a shipped desktop application has no `crates/` directory next to it, and a client
// that reads its own contract off disk would work in the repository and fail everywhere else.
// `contract.test.ts` compares this table with the published one at TEST time, which is where the
// comparison belongs.
//
// Adding a row here without adding it to the daemon's `METHODS` — or the other way round — turns
// that test red on both sides.

import { validateCounterResult } from './counter-result.ts';
import type { WireObject } from './protocol.ts';

/** One versioned method on the daemon's IPC surface. */
export type MethodEntry = {
  /** The name a call carries. */
  readonly name: string;
  /** The first surface version that has this method. */
  readonly since: number;
  /** What it answers. */
  readonly summary: string;
  /** Whether an interrupted call may be sent again without duplicating an effect. */
  readonly retryAfterDisconnect: boolean;
  /** Fail-closed proof for a method whose result has security- or precision-critical structure. */
  readonly validateResult?: (value: WireObject) => void;
};

/** Every method this client knows how to call, in the order the daemon publishes them. */
export const METHODS: readonly MethodEntry[] = [
  {
    name: 'daemon.status',
    since: 1,
    summary: 'whether the background service is serving requests, and which surface it speaks',
    retryAfterDisconnect: true,
  },
  {
    name: 'startup.report',
    since: 1,
    summary: 'what the background service found when it last started',
    retryAfterDisconnect: true,
  },
  {
    name: 'surface.describe',
    since: 1,
    summary: 'this catalogue, so a client can tell whether it is talking to a newer service',
    retryAfterDisconnect: true,
  },
  {
    name: 'workspace.open',
    since: 2,
    summary: 'open the workspace in a folder and read back what is saved there',
    // Opening changes the daemon-global workspace. If the first call took effect but its reply
    // was lost, another client may have opened a different workspace before this client
    // reconnects. Replaying here would silently overwrite that newer choice.
    retryAfterDisconnect: false,
  },
  {
    name: 'workspace.state',
    since: 2,
    summary: 'what the open workspace holds, and what this build cannot answer yet',
    retryAfterDisconnect: true,
  },
  {
    name: 'review.open',
    since: 2,
    summary: 'open or reuse an exact review bundle over one saved change',
    retryAfterDisconnect: true,
  },
  {
    name: 'review.open-current',
    since: 6,
    summary: 'compute and open the exact first-publication review for the current private head',
    retryAfterDisconnect: true,
  },
  {
    name: 'review.approve',
    since: 2,
    summary: 'request shared publication; unavailable until HumanHeld authority is durable',
    retryAfterDisconnect: false,
  },
  {
    name: 'events.subscribe',
    since: 2,
    summary: 'hear about what the background service is doing, as it happens',
    retryAfterDisconnect: true,
  },
  {
    name: 'folder.import.preview',
    since: 3,
    summary: 'hash one selected local folder without changing it or creating a managed copy',
    retryAfterDisconnect: true,
  },
  {
    name: 'folder.import.confirm',
    since: 3,
    summary: 'verify the accepted summary, create durable private history, and open the managed copy',
    retryAfterDisconnect: false,
  },
  {
    name: 'folder.import.rollback',
    since: 3,
    summary: 'remove an unchanged receipt-owned managed copy while preserving the original',
    retryAfterDisconnect: false,
  },
  {
    name: 'workspace.restore.preview',
    since: 3,
    summary: 'preview an exact append-only earlier-version restore without authorizing it',
    retryAfterDisconnect: true,
  },
  {
    name: 'workspace.version.fork',
    since: 5,
    summary: 'open one durable version of the exact displayed workspace as a new independent native working folder',
    retryAfterDisconnect: false,
  },
  {
    name: 'performance.counters',
    since: 4,
    summary: 'read every live performance counter and its collection conditions',
    retryAfterDisconnect: true,
    validateResult: validateCounterResult,
  },
];

/** The catalogue entry for `name`, when there is one. */
export const methodEntry = (name: string): MethodEntry | undefined =>
  METHODS.find((entry) => entry.name === name);

/** Whether `name` is callable on a connection that negotiated `version`. */
export const isCallableAt = (name: string, version: number): boolean => {
  const entry = methodEntry(name);
  return entry !== undefined && entry.since <= version;
};
