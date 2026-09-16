// A stand-in for the Mesh background service, over a REAL Unix-domain socket.
//
// # Why a stand-in and not the real daemon
//
// The real daemon is a Rust crate. Driving it from here would make `npm --prefix apps/desktop
// test` depend on a cargo build, which is slower than the whole gate budget and would make this
// suite unrunnable on a machine with no toolchain. So the client is exercised against a socket
// that speaks the same bytes, and the risk that creates — two implementations of one format —
// is closed the other way: `src/ipc/contract.test.ts` decodes and re-encodes every published
// vector in `crates/mesh-daemon/ipc-contract.json` and compares BYTE FOR BYTE with the same
// corpus the Rust side round-trips in `crates/mesh-daemon/tests/ipc.rs`.
//
// What this file therefore proves and does not prove:
//
//  - PROVED: the client's framing, negotiation, correlation, queueing and reconnection are
//    correct against a peer that follows the contract, over a real socket with real timing.
//  - NOT PROVED: that the Rust daemon behaves the same at runtime. The corpus makes the two
//    encodings identical; only an end-to-end task can make the two behaviours identical, and
//    nothing in this pass claims it has.
//
// This file lives OUTSIDE `src/` on purpose. It opens a listener, which no source under `src/` is
// allowed to do, and `src/ipc/no-network.test.ts` enforces exactly that boundary.

import { createServer, type Server, type Socket } from 'node:net';
import { rm } from 'node:fs/promises';

import { METHODS } from '../src/ipc/methods.ts';
import {
  COUNTER_ATOMIC_WRITES_PER_OBSERVATION,
  COUNTER_ATOMIC_WRITES_PER_GROUP,
  COUNTER_CATALOGUE_COUNT,
  COUNTER_CATALOGUE,
  COUNTER_CONDITIONS,
  COUNTER_NOT_YET_CATALOGUE,
  COUNTER_WIRED_KEYS,
} from '../src/ipc/counter-result.ts';
import {
  CHUNK_DATA_BYTES,
  MAX_LINE_BYTES,
  decodeClient,
  encodeDaemon,
  splitLines,
  type DaemonMessage,
  type WireObject,
} from '../src/ipc/protocol.ts';

/** How this stand-in should behave, for the cases a well-behaved service also has. */
export type FakeOptions = {
  /** The versions it supports. Set it to something the client does not have to force a refusal. */
  readonly supported?: readonly number[];
  /** The `workspace.open` and `workspace.state` answer. */
  readonly workspace?: WireObject;
  /** The `startup.report` answer. */
  readonly startup?: WireObject;
  /**
   * How long to hold a reply before writing it.
   *
   * The only way to get a call reliably INTO the in-flight set and then kill the socket under it,
   * which is the case the reconnection logic exists for.
   */
  readonly answerDelayMs?: number;
  /**
   * Deliberately corrupt one field of the welcome for client fail-closed tests.
   * Production code never uses this seam; it keeps the mutation on real socket bytes.
   */
  readonly welcomeOverride?: Readonly<{
    id?: number;
    version?: number;
    session?: string;
    surfaceVersion?: number;
  }>;
  /** Reply to this method with bytes outside the daemon protocol. Test-only corruption seam. */
  readonly malformedReplyFor?: string;
  /** Reply to this method with a valid message carrying another call's identifier. */
  readonly uncorrelatedReplyFor?: string;
  /** Push an event after this ordinary method even though no subscription exists. */
  readonly unsolicitedEventFor?: string;
  /** Deliberately replace event correlation fields for fail-closed client tests. */
  readonly eventOverride?: Readonly<{ id?: number; sequence?: number }>;
  /** Push the same event sequence twice on one subscribed socket. */
  readonly duplicateEventSequence?: boolean;
  /** Fail the live subscription after its success reply, as when the bounded feed wraps. */
  readonly subscriptionBacklogLost?: boolean;
  /** Replace the first pushed sequence to plant a missing-prefix protocol mutation. */
  readonly firstEventSequence?: number;
  /** Replace the successful subscription value with a malformed one. Test-only corruption seam. */
  readonly subscriptionResult?: WireObject;
  /** Replace the v4 counter snapshot with malformed method bytes. Test-only corruption seam. */
  readonly performanceCounters?: WireObject;
  /** Encode oversized replies as surface-v7 chunks. */
  readonly chunkLargeReplies?: boolean;
};

const DEFAULT_COUNTERS: WireObject = {
  conditions: COUNTER_CONDITIONS,
  integer_encoding: 'decimal-u64',
  collection: {
    observations_recorded: '0',
    observations_summed: '0',
    concurrent_observations: '0',
    writers_in_flight_before_readings: '0',
    writers_in_flight_after_readings: '0',
    snapshot_consistent: true,
    snapshots_taken: '1',
    state_bytes: '1',
    counters: String(COUNTER_CATALOGUE_COUNT),
    atomic_writes_per_observation: String(COUNTER_ATOMIC_WRITES_PER_OBSERVATION),
    atomic_writes_per_group: String(COUNTER_ATOMIC_WRITES_PER_GROUP),
    allocations_per_observation: '0',
  },
  counters: COUNTER_CATALOGUE.map(({ key, unit }) => {
    return {
      key,
      family: key.slice(0, key.indexOf('.')),
      unit,
      determinism: unit === 'nanoseconds' ? 'load-dependent' : 'deterministic',
      band: unit === 'nanoseconds' ? 'enclosed' : 'exact',
      observations: '0',
      total: '0',
      produced: COUNTER_WIRED_KEYS.includes(key as (typeof COUNTER_WIRED_KEYS)[number]),
    };
  }),
  not_yet: COUNTER_NOT_YET_CATALOGUE,
};

/** What this stand-in says a workspace holds. Shaped exactly as the contract's own vector. */
const DEFAULT_WORKSPACE: WireObject = {
  root: '/home/ada/work',
  records: 3,
  unfinished_bytes: 0,
  operations: 2,
  actors: 1,
  manifests: 0,
  peers: 1,
  reviews: 0,
  digest: '3f2b0c8d1e4a5967b8c0d1e2f3a4b5c6',
  not_yet: [
    {
      subject: 'file contents',
      reason:
        "materialising a workspace's files means applying its operations to a workspace state, and this surface has no method that returns file bytes; the folder-watching fallback in this crate presents a folder that is already on disk and materialises nothing",
    },
  ],
};

const DEFAULT_STARTUP: WireObject = {
  serving: true,
  severity: 'routine',
  elapsed_ms: 0,
  sentence: 'Mesh started and your workspace is up to date. 0 saved changes were read back.',
};

/** A stand-in background service bound to one endpoint. */
export class FakeDaemon {
  readonly endpoint: string;
  readonly #supported: readonly number[];
  readonly #startup: WireObject;
  readonly #workspace: WireObject;
  readonly #answerDelayMs: number;
  readonly #welcomeOverride: FakeOptions['welcomeOverride'];
  readonly #malformedReplyFor: string | undefined;
  readonly #uncorrelatedReplyFor: string | undefined;
  readonly #unsolicitedEventFor: string | undefined;
  readonly #eventOverride: FakeOptions['eventOverride'];
  readonly #duplicateEventSequence: boolean;
  readonly #subscriptionBacklogLost: boolean;
  readonly #firstEventSequence: number;
  readonly #subscriptionResult: WireObject | undefined;
  readonly #performanceCounters: WireObject;
  readonly #chunkLargeReplies: boolean;
  readonly #sockets = new Set<Socket>();
  readonly #sessionsSeen: string[] = [];
  readonly #timers = new Set<ReturnType<typeof setTimeout>>();
  readonly #callsSeen: { readonly method: string; readonly params: WireObject }[] = [];

  #server: Server | null = null;
  #connections = 0;
  #callsAnswered = 0;

  constructor(endpoint: string, options: FakeOptions = {}) {
    this.endpoint = endpoint;
    this.#supported = options.supported ?? [1, 2, 3, 4, 5, 6, 7];
    this.#startup = options.startup ?? DEFAULT_STARTUP;
    this.#workspace = options.workspace ?? DEFAULT_WORKSPACE;
    this.#answerDelayMs = options.answerDelayMs ?? 0;
    this.#welcomeOverride = options.welcomeOverride;
    this.#malformedReplyFor = options.malformedReplyFor;
    this.#uncorrelatedReplyFor = options.uncorrelatedReplyFor;
    this.#unsolicitedEventFor = options.unsolicitedEventFor;
    this.#eventOverride = options.eventOverride;
    this.#duplicateEventSequence = options.duplicateEventSequence ?? false;
    this.#subscriptionBacklogLost = options.subscriptionBacklogLost ?? false;
    this.#firstEventSequence = options.firstEventSequence ?? 1;
    this.#subscriptionResult = options.subscriptionResult;
    this.#performanceCounters = options.performanceCounters ?? DEFAULT_COUNTERS;
    this.#chunkLargeReplies = options.chunkLargeReplies ?? false;
  }

  /** How many connections have been accepted since the last {@link start}. */
  get connections(): number {
    return this.#connections;
  }

  /** How many calls have been answered since construction. */
  get callsAnswered(): number {
    return this.#callsAnswered;
  }

  /** Calls received, including their exact parameters, for reconnection assertions. */
  get callsSeen(): readonly { readonly method: string; readonly params: WireObject }[] {
    return this.#callsSeen;
  }

  /** Every session name announced to this process, in order. Reset by {@link start}. */
  get sessionsSeen(): readonly string[] {
    return this.#sessionsSeen;
  }

  /** Bind the endpoint, replacing a socket file a previous run left behind. */
  async start(): Promise<void> {
    await rm(this.endpoint, { force: true });
    this.#connections = 0;
    this.#sessionsSeen.length = 0;
    const server = createServer((socket) => this.#serve(socket));
    this.#server = server;
    await new Promise<void>((resolve, reject) => {
      server.once('error', reject);
      server.listen(this.endpoint, () => resolve());
    });
  }

  /** Drop every connection and unbind, the way a killed background service would. */
  async stop(): Promise<void> {
    for (const timer of this.#timers) clearTimeout(timer);
    this.#timers.clear();
    for (const socket of this.#sockets) socket.destroy();
    this.#sockets.clear();
    const server = this.#server;
    this.#server = null;
    if (server !== null) await new Promise<void>((resolve) => server.close(() => resolve()));
    await rm(this.endpoint, { force: true });
  }

  /** Drop current sockets without restarting this daemon process or forgetting its sessions. */
  dropConnections(): void {
    for (const socket of this.#sockets) socket.destroy();
    this.#sockets.clear();
  }

  #serve(socket: Socket): void {
    this.#connections += 1;
    this.#sockets.add(socket);
    socket.setEncoding('utf8');
    socket.on('close', () => this.#sockets.delete(socket));
    socket.on('error', () => this.#sockets.delete(socket));

    let negotiated: number | null = null;
    let buffered = '';

    socket.on('data', (chunk: string) => {
      buffered += chunk;
      const { lines, rest } = splitLines(buffered);
      buffered = rest;
      for (const line of lines) {
        let reply: DaemonMessage;
        try {
          const request = decodeClient(line);
          if (request.t === 'hello') {
            const shared = this.#supported.filter((version) => request.versions.includes(version));
            const chosen = shared.length === 0 ? null : Math.max(...shared);
            if (chosen === null) {
              reply = {
                t: 'refused',
                id: request.id,
                code: 'unsupported-version',
                message:
                  'This app and the Mesh background service are too far apart in age to talk to each other. Update both to the same release and try again.',
                supported: [...this.#supported],
              };
            } else {
              const resumed = this.#sessionsSeen.includes(request.session);
              this.#sessionsSeen.push(request.session);
              negotiated = chosen;
              reply = {
                t: 'welcome',
                id: this.#welcomeOverride?.id ?? request.id,
                version: this.#welcomeOverride?.version ?? chosen,
                session: this.#welcomeOverride?.session ?? request.session,
                resumed,
                surfaceVersion:
                  this.#welcomeOverride?.surfaceVersion ?? Math.max(...this.#supported),
              };
            }
          } else if (negotiated === null) {
            reply = {
              t: 'refused',
              id: request.id,
              code: 'not-open',
              message: 'This connection has not finished opening yet. Nothing was changed.',
              supported: [...this.#supported],
            };
          } else {
            if (request.t === 'call' && request.method === this.#malformedReplyFor) {
              socket.write('{"t":"result","id":');
              socket.write('\n');
              continue;
            }
            this.#callsSeen.push({ method: request.method, params: request.params });
            reply = this.#answer(request.id, request.method);
            if (request.t === 'call' && request.method === this.#uncorrelatedReplyFor) {
              reply = { ...reply, id: request.id + 1 };
            }
            if (
              request.t === 'call' &&
              (request.method === 'events.subscribe' || request.method === this.#unsolicitedEventFor) &&
              reply.t === 'result'
            ) {
              // The push a real daemon makes: the backlog, delivered right after the answer. Sent
              // on a later turn of the loop so the client sees a reply and THEN an unsolicited
              // line, which is the ordering it has to survive.
              const cursor = request.params['after_sequence'];
              this.#pushSoon(
                socket,
                request.id,
                typeof cursor === 'number' && Number.isSafeInteger(cursor) && cursor >= 0 ? cursor : 0,
              );
            }
          }
        } catch (error) {
          reply = {
            t: 'failed',
            id: 0,
            code: 'malformed-message',
            message: `The Mesh background service could not read that request and nothing was changed. (${(error as Error).message})`,
          };
        }
        this.#send(socket, reply);
      }
    });
  }

  /**
   * Write one reply, and never to a socket that has gone.
   *
   * The delay applies to method answers only. Delaying the handshake too would make every test
   * slower without making any of them stricter, and the case the delay exists for is a call that
   * is in flight when the socket dies — which needs the handshake to have already finished.
   */
  #send(socket: Socket, reply: DaemonMessage): void {
    const write = (): void => {
      if (socket.destroyed) return;
      const encoded = encodeDaemon(reply);
      if (this.#chunkLargeReplies && Buffer.byteLength(encoded, 'utf8') >= MAX_LINE_BYTES) {
        const bytes = Buffer.from(encoded, 'utf8');
        const parts = Math.ceil(bytes.length / CHUNK_DATA_BYTES);
        for (let index = 0; index < parts; index += 1) {
          socket.write(`${encodeDaemon({
            t: 'chunk',
            id: reply.id,
            index,
            parts,
            totalBytes: bytes.length,
            hex: bytes.subarray(index * CHUNK_DATA_BYTES, (index + 1) * CHUNK_DATA_BYTES).toString('hex'),
          })}\n`);
        }
      } else {
        socket.write(`${encoded}\n`);
      }
    };
    if (this.#answerDelayMs === 0 || (reply.t !== 'result' && reply.t !== 'failed')) {
      write();
      return;
    }
    const timer = setTimeout(() => {
      this.#timers.delete(timer);
      write();
    }, this.#answerDelayMs);
    this.#timers.add(timer);
  }

  /** Push one feed entry to a subscriber, the way the daemon does after `events.subscribe`. */
  #pushSoon(socket: Socket, subscription: number, afterSequence: number): void {
    const timer = setTimeout(() => {
      this.#timers.delete(timer);
      if (socket.destroyed || afterSequence >= 1) return;
      if (this.#subscriptionBacklogLost) {
        socket.write(
          `${encodeDaemon({
            t: 'failed',
            id: subscription,
            code: 'event-backlog-lost',
            message:
              'Some background-service events are no longer available. Refresh the workspace state before continuing.',
          })}\n`,
        );
        return;
      }
      socket.write(
        `${encodeDaemon({
          t: 'event',
          id: this.#eventOverride?.id ?? subscription,
          sequence: this.#eventOverride?.sequence ?? this.#firstEventSequence,
          kind: 'serving',
          value: {},
        })}\n`,
      );
      if (this.#duplicateEventSequence) {
        socket.write(
          `${encodeDaemon({
            t: 'event',
            id: this.#eventOverride?.id ?? subscription,
            sequence: this.#eventOverride?.sequence ?? this.#firstEventSequence,
            kind: 'serving',
            value: {},
          })}\n`,
        );
      }
    }, 1);
    this.#timers.add(timer);
  }

  #answer(id: number, method: string): DaemonMessage {
    this.#callsAnswered += 1;
    switch (method) {
      case 'daemon.status':
        return { t: 'result', id, value: { serving: true, surface_version: Math.max(...this.#supported) } };
      case 'startup.report':
        return { t: 'result', id, value: this.#startup };
      case 'workspace.open':
      case 'workspace.state':
        return { t: 'result', id, value: this.#workspace };
      case 'folder.import.preview':
        return {
          t: 'result',
          id,
          value: { action: 'preview', files: 2, folders: 1, bytes: 128, digest: '04'.repeat(32) },
        };
      case 'folder.import.confirm':
        return {
          t: 'result',
          id,
          value: { action: 'confirmed', destination: '/home/ada/Mesh/work', workspace: this.#workspace },
        };
      case 'folder.import.rollback':
        return { t: 'result', id, value: { action: 'rolled-back', removed: true } };
      case 'workspace.restore.preview':
        return {
          t: 'result',
          id,
          value: { canonical_state_read_only: true, execution_authorized: false, undo_possible: true },
        };
      case 'workspace.version.fork':
        return {
          t: 'result',
          id,
          value: { action: 'workspace-version-opened-as-copy', destination: '/home/ada/Mesh/earlier/mounts', workspace: this.#workspace },
        };
      case 'performance.counters':
        return {
          t: 'result',
          id,
          value: this.#performanceCounters,
        };
      case 'review.open':
        return { t: 'result', id, value: { opened: true, reused: false } };
      case 'review.open-current':
        return { t: 'result', id, value: { opened: true, reused: false, workspace: this.#workspace } };
      case 'review.approve':
        return { t: 'result', id, value: { approved: true, shared_version: '04'.repeat(32) } };
      case 'events.subscribe':
        return {
          t: 'result',
          id,
          value: this.#subscriptionResult ?? { subscribed: true, latest_sequence: 1 },
        };
      case 'surface.describe':
        return {
          t: 'result',
          id,
          value: {
            protocol: 'mesh-ipc',
            surface_version: Math.max(...this.#supported),
            supported_versions: [...this.#supported],
            methods: METHODS.map((entry) => ({ name: entry.name, since: entry.since, summary: entry.summary })),
          },
        };
      default:
        return {
          t: 'failed',
          id,
          code: 'unknown-method',
          message:
            'The Mesh background service does not offer this operation. Nothing was changed. Updating both the app and the service usually fixes this.',
        };
    }
  }
}
