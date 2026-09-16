// The connection lifecycle: negotiate, call, survive a restart of the background service.
//
// # The one acceptance criterion this file exists for
//
// *"A daemon restart reconnects without losing user context."* The design answer is that the
// CLIENT owns the context and the daemon owns nothing about it. Four things survive a restart
// here, and each is asserted in `client.test.ts`:
//
//  1. `context` — the object the application handed in. It is never replaced and never cleared,
//     so a React tree holding a reference to it keeps holding the same object across the outage.
//  2. The session name, re-announced on every reconnection so the service can recognise a
//     returning client when it is able to.
//  3. Every queued call, plus every in-flight call that the method catalogue marks safe to retry.
//     A one-time action that may already have taken effect is rejected with an unknown outcome
//     instead of being sent twice.
//  4. Every state subscriber, so the interface can render "reconnecting" and then recover without
//     re-subscribing.
//
// # Why re-issuing an in-flight call is explicit
//
// `methods.ts` classifies every method. Reads and exact reuse operations are re-issued after a
// socket dies. `review.approve` is deliberately not: the daemon may have advanced the shared
// version before its reply was lost. Retrying would turn an uncertain answer into a duplicated
// mutation. The caller must refresh shared state before deciding what to do next.
//
// A re-issued `events.subscribe` is likewise safe and is in fact required: the subscription lives
// on the connection. A reconnect to the same daemon resumes after the last received sequence; a
// restarted daemon has a new sequence space and starts at zero. The welcome's `resumed` bit is the
// process-scoped distinction, so old events are neither duplicated nor confused with new ones.
//
// # What this client does NOT do
//
// It opens no listener, it has no direct database access of any kind, and it holds no state the
// daemon is authoritative for. Everything it knows about a workspace came back over the socket in
// answer to a method in `methods.ts`.

import { CONNECTION_COPY } from '../strings/connection.ts';
import { isCallableAt, methodEntry } from './methods.ts';
import {
  CLIENT_VERSIONS,
  DaemonChunkAssembler,
  NO_CORRELATION,
  WireError,
  decodeDaemon,
  encodeClient,
  type LogicalDaemonMessage,
  type WireObject,
} from './protocol.ts';

/** One thing the service pushed after a subscription, as the application sees it. */
export type DaemonEvent = {
  /** Where it sits in the service's feed. Not an ordering of anybody's work. */
  readonly sequence: number;
  /** The stable word for what happened. */
  readonly kind: string;
  /** Its own fields. */
  readonly value: WireObject;
};
import { connectToLocalSocket, type Connector, type Duplex } from './transport.ts';

/** What the interface renders. Six words are the product's state vocabulary; these five are the
 * connection's, and they describe the link to the background service rather than anybody's work. */
export type ConnectionState = 'idle' | 'opening' | 'connected' | 'reconnecting' | 'unusable' | 'stopped';

/** The application's own state, held across a restart of the background service. */
export type UserContext = Record<string, unknown>;

/** How to build a connection. Every field has a default except the endpoint. */
export type ClientOptions = {
  /** The filesystem path the background service listens on. */
  readonly endpoint: string;
  /** This client's session name, re-announced on every reconnection. */
  readonly session: string;
  /** The surface versions this client can speak. */
  readonly versions?: readonly number[];
  /** How to open a connection. The seam a test replaces. */
  readonly connect?: Connector;
  /** Delays between reconnection attempts, in milliseconds. The last one repeats. */
  readonly retryDelaysMs?: readonly number[];
  /** The application state to hold across restarts. Never replaced by this client. */
  readonly context?: UserContext;
};

/** Counters a test — or a person looking at a support panel — can read. */
export type ConnectionStats = {
  /** How many times the link dropped and a reconnection was started. */
  readonly reconnects: number;
  /** How many calls have been written to a socket, re-issues included. */
  readonly callsSent: number;
  /** How many calls were in flight when a link dropped and were sent again. */
  readonly callsReissued: number;
  /** The version in force, or `null` while no connection is open. */
  readonly negotiatedVersion: number | null;
  /** How many calls are waiting for a connection to send them on. */
  readonly queueDepth: number;
};

/** The default reconnection schedule: quick twice, then every two seconds. */
export const DEFAULT_RETRY_DELAYS: readonly number[] = [100, 400, 2000];

type Pending = {
  readonly method: string;
  readonly params: WireObject;
  readonly resolve: (value: WireObject) => void;
  readonly reject: (error: Error) => void;
  id: number | null;
};

/** A live connection to the local Mesh background service. */
export class DaemonConnection {
  readonly #endpoint: string;
  readonly #session: string;
  readonly #versions: readonly number[];
  readonly #connect: Connector;
  readonly #retryDelays: readonly number[];
  readonly #context: UserContext;

  readonly #stateHandlers: ((state: ConnectionState) => void)[] = [];
  readonly #faultHandlers: ((message: string) => void)[] = [];
  readonly #eventHandlers: ((event: DaemonEvent) => void)[] = [];
  readonly #queue: Pending[] = [];
  readonly #inFlight = new Map<number, Pending>();
  readonly #chunks = new DaemonChunkAssembler();

  #state: ConnectionState = 'idle';
  #link: Duplex | null = null;
  #negotiated: number | null = null;
  #nextId = 1;
  #helloId: number | null = null;
  #attempt = 0;
  #timer: ReturnType<typeof setTimeout> | null = null;
  #reconnects = 0;
  #callsSent = 0;
  #callsReissued = 0;
  #eventsReceived = 0;
  #latestSequence = 0;
  #subscriptionId: number | null = null;
  #subscriptionSequence = 0;
  #subscriptionWanted = false;
  #unusableCode = 'unsupported-version';
  #unusableSentence: string = CONNECTION_COPY.unusable;

  constructor(options: ClientOptions) {
    if (options.endpoint.length === 0) throw new Error('a connection needs an endpoint');
    if (options.session.length === 0) throw new Error('a connection needs a session name');
    this.#endpoint = options.endpoint;
    this.#session = options.session;
    this.#versions = options.versions ?? CLIENT_VERSIONS;
    this.#connect = options.connect ?? connectToLocalSocket;
    this.#retryDelays = options.retryDelaysMs ?? DEFAULT_RETRY_DELAYS;
    this.#context = options.context ?? {};
  }

  /** The application state this connection holds across a restart. Always the same object. */
  get context(): UserContext {
    return this.#context;
  }

  /** The session name announced on every connection. */
  get session(): string {
    return this.#session;
  }

  /** What the interface should be rendering right now. */
  get state(): ConnectionState {
    return this.#state;
  }

  /** The user-facing sentence for the current state. */
  get sentence(): string {
    return this.#state === 'unusable' ? this.#unusableSentence : CONNECTION_COPY[this.#state];
  }

  /** Counters, for a test or a support panel. */
  stats(): ConnectionStats {
    return {
      reconnects: this.#reconnects,
      callsSent: this.#callsSent,
      callsReissued: this.#callsReissued,
      negotiatedVersion: this.#negotiated,
      queueDepth: this.#queue.length,
    };
  }

  /** Subscribe to state changes. Subscribers survive every reconnection. */
  onState(handler: (state: ConnectionState) => void): void {
    this.#stateHandlers.push(handler);
  }

  /** Subscribe to connection-level faults — a reply the service could not correlate with a call. */
  onFault(handler: (message: string) => void): void {
    this.#faultHandlers.push(handler);
  }

  /**
   * Hear what the service pushes after an `events.subscribe` call.
   *
   * Handlers survive every reconnection, exactly as state subscribers do: an interface that had to
   * re-register after an outage would go quiet at the moment it most needs to be showing something.
   * Registering a handler does NOT subscribe — call `events.subscribe` for that — because a client
   * that wants no push should send no subscription rather than silently receive one.
   */
  onEvent(handler: (event: DaemonEvent) => void): void {
    this.#eventHandlers.push(handler);
  }

  /** How many pushed entries have arrived, and the newest sequence seen. */
  get feed(): { readonly received: number; readonly latestSequence: number } {
    return { received: this.#eventsReceived, latestSequence: this.#latestSequence };
  }

  /** Begin opening the connection and keep retrying until {@link stop}. */
  async start(): Promise<void> {
    if (this.#state !== 'idle' && this.#state !== 'stopped') return;
    await this.#open('opening');
  }

  /**
   * Call one catalogue method.
   *
   * A call made while the service is unreachable is HELD, not rejected: the interface's job during
   * an outage is to look like it is waiting, not to throw an error at somebody who did nothing
   * wrong. It is rejected only when the surface itself cannot serve it.
   */
  call(method: string, params: WireObject = {}): Promise<WireObject> {
    return new Promise<WireObject>((resolve, reject) => {
      if (methodEntry(method) === undefined) {
        reject(new WireError('unknown-method', `\`${method}\` is not on the Mesh service surface`));
        return;
      }
      if (this.#state === 'unusable') {
        reject(new WireError(this.#unusableCode, this.#unusableSentence));
        return;
      }
      if (this.#state === 'stopped') {
        reject(new WireError('stopped', CONNECTION_COPY.stopped));
        return;
      }
      const pending: Pending = { method, params, resolve, reject, id: null };
      this.#queue.push(pending);
      this.#drain();
    });
  }

  /** Close the connection, stop retrying, and reject everything still waiting. */
  stop(): void {
    this.#clearTimer();
    this.#link?.close();
    this.#link = null;
    this.#helloId = null;
    this.#subscriptionId = null;
    this.#subscriptionSequence = 0;
    this.#subscriptionWanted = false;
    this.#setState('stopped');
    const waiting = [...this.#inFlight.values(), ...this.#queue];
    this.#inFlight.clear();
    this.#queue.length = 0;
    for (const pending of waiting) {
      pending.reject(new WireError('stopped', CONNECTION_COPY.stopped));
    }
  }

  // ------------------------------------------------------------------ internals

  #setState(next: ConnectionState): void {
    if (this.#state === next) return;
    this.#state = next;
    for (const handler of this.#stateHandlers) handler(next);
  }

  #clearTimer(): void {
    if (this.#timer !== null) {
      clearTimeout(this.#timer);
      this.#timer = null;
    }
  }

  async #open(state: ConnectionState): Promise<void> {
    this.#setState(state);
    let link: Duplex;
    try {
      link = await this.#connect(this.#endpoint);
    } catch {
      this.#scheduleRetry();
      return;
    }
    if (this.#state === 'stopped') {
      link.close();
      return;
    }
    this.#link = link;
    // Both handlers check that this is still the CURRENT link. A socket that has already been
    // replaced can still deliver a buffered line or a close event, and acting on either would
    // corrupt the state of the connection that replaced it.
    link.onLine((line) => {
      if (this.#link === link) this.#receive(line);
    });
    link.onClosed(() => {
      if (this.#link === link) this.#lost();
    });
    const helloId = this.#take();
    this.#helloId = helloId;
    link.send(encodeClient({ t: 'hello', id: helloId, versions: this.#versions, session: this.#session }));
  }

  #take(): number {
    const id = this.#nextId;
    this.#nextId += 1;
    return id;
  }

  #scheduleRetry(): void {
    if (this.#state === 'stopped' || this.#state === 'unusable') return;
    const delay = this.#retryDelays[Math.min(this.#attempt, this.#retryDelays.length - 1)] ?? 1000;
    this.#attempt += 1;
    this.#setState('reconnecting');
    this.#clearTimer();
    this.#timer = setTimeout(() => {
      this.#timer = null;
      void this.#open('reconnecting');
    }, delay);
  }

  /** The socket ended. Nothing the application owns is discarded. */
  #lost(): void {
    if (this.#state === 'stopped' || this.#state === 'unusable') return;
    this.#link = null;
    this.#negotiated = null;
    this.#helloId = null;
    this.#subscriptionId = null;
    this.#chunks.reset();
    // The identifier belongs to the dead socket, but the cursor belongs to the established
    // subscription intent. Keep it until the welcome tells us whether the next socket reaches the
    // same daemon process; otherwise an explicit nonzero cursor with no later pushed event would
    // be forgotten and the reconnect would replay an already-consumed prefix.
    // Retry only calls whose catalogue entry says a second send cannot duplicate an effect.
    // Preserve the original order among the calls that do go back to the queue.
    const interrupted = [...this.#inFlight.values()];
    this.#inFlight.clear();
    for (const pending of interrupted.reverse()) {
      const method = methodEntry(pending.method);
      if (method?.retryAfterDisconnect !== true) {
        pending.reject(new WireError('outcome-unknown', CONNECTION_COPY.outcomeUnknown));
        continue;
      }
      pending.id = null;
      this.#queue.unshift(pending);
      this.#callsReissued += 1;
    }
    this.#reconnects += 1;
    this.#scheduleRetry();
  }

  #receive(line: string): void {
    let message: LogicalDaemonMessage;
    try {
      const complete = this.#chunks.push(decodeDaemon(line));
      if (complete === null) return;
      message = complete;
    } catch (error) {
      // A line that does not satisfy the negotiated wire contract cannot be assigned to any
      // request safely. Permanently distrust this conversation, while preserving the distinct
      // `outcome-unknown` result for a non-retryable mutation that was already sent. Treating that
      // mutation as an ordinary protocol mismatch would invite an unsafe manual retry; merely
      // closing the socket would reconnect automatically to the peer that sent corrupt bytes.
      this.#rejectProtocolFault((error as Error).message);
      return;
    }
    switch (message.t) {
      case 'welcome':
        // The daemon and client both select the newest common surface. Merely checking that the
        // selected version appears in our offer would let a stale or non-Mesh socket peer force a
        // downgrade and make newer method semantics disappear while the UI still reports a valid
        // connection.
        const newestCommonVersion = Math.max(
          ...this.#versions.filter((version) => version <= message.surfaceVersion),
        );
        if (
          message.id !== this.#helloId ||
          message.session !== this.#session ||
          !this.#versions.includes(message.version) ||
          message.surfaceVersion < message.version ||
          message.version !== newestCommonVersion
        ) {
          this.#rejectConversation('protocol-mismatch', CONNECTION_COPY.protocolMismatch);
          return;
        }
        this.#helloId = null;
        this.#negotiated = message.version;
        this.#attempt = 0;
        if (!message.resumed && this.#subscriptionWanted) {
          // The event sequence belongs to one daemon process. A replacement process starts a new
          // feed at one, so retaining the previous process's cursor would suppress valid events.
          this.#latestSequence = 0;
          this.#subscriptionSequence = 0;
        }
        this.#setState('connected');
        // A subscription is connection-local. Once the caller established one, reconnecting must
        // restore it before queued work continues or a transparent daemon restart silently turns
        // a live-looking UI deaf. A state handler may already have queued the call while observing
        // `connected`; the helper detects that and never sends a duplicate.
        this.#restoreSubscription(message.resumed);
        this.#drain();
        return;
      case 'refused':
        this.#refuse(message.id, message.code, message.message);
        return;
      case 'result': {
        const pending = this.#inFlight.get(message.id);
        if (pending === undefined) {
          this.#rejectProtocolFault(CONNECTION_COPY.uncorrelated);
          return;
        }
        try {
          methodEntry(pending.method)?.validateResult?.(message.value);
        } catch (error) {
          this.#rejectProtocolFault((error as Error).message);
          return;
        }
        if (pending.method === 'events.subscribe') {
          const requestedCursor = pending.params['after_sequence'] ?? 0;
          if (
            message.value['subscribed'] !== true ||
            typeof message.value['latest_sequence'] !== 'number' ||
            !Number.isSafeInteger(message.value['latest_sequence']) ||
            message.value['latest_sequence'] < 0 ||
            typeof requestedCursor !== 'number' ||
            !Number.isSafeInteger(requestedCursor) ||
            requestedCursor < 0 ||
            requestedCursor > message.value['latest_sequence']
          ) {
            this.#rejectProtocolFault(CONNECTION_COPY.uncorrelated);
            return;
          }
        }
        this.#inFlight.delete(message.id);
        if (pending.method === 'events.subscribe') {
          this.#subscriptionId = message.id;
          this.#subscriptionSequence = (pending.params['after_sequence'] as number | undefined) ?? 0;
          this.#subscriptionWanted = true;
        }
        pending.resolve(message.value);
        return;
      }
      case 'event': {
        // A pushed line is correlated with the successful call that opened this stream. The call
        // itself has already resolved, so the subscription identifier is retained separately
        // from the in-flight table for the lifetime of this exact socket.
        if (
          message.id !== this.#subscriptionId ||
          message.sequence !== this.#subscriptionSequence + 1
        ) {
          this.#rejectProtocolFault(CONNECTION_COPY.uncorrelated);
          return;
        }
        this.#subscriptionSequence = message.sequence;
        this.#eventsReceived += 1;
        this.#latestSequence = message.sequence;
        for (const handler of this.#eventHandlers) {
          handler({ sequence: message.sequence, kind: message.kind, value: message.value });
        }
        return;
      }
      default: {
        if (message.id === NO_CORRELATION) {
          this.#rejectProtocolFault(message.message);
          return;
        }
        if (
          message.id === this.#subscriptionId &&
          message.code === 'event-backlog-lost'
        ) {
          // A subscription may be admitted while its cursor is still in the bounded feed, then
          // lose that prefix before the service drains the stream. The call has already resolved,
          // so this failure is correlated by the retained subscription identifier rather than the
          // in-flight table. Continuing would make the local view look current after lost events.
          this.#fault(message.message);
          this.#rejectConversation(message.code, message.message);
          return;
        }
        const pending = this.#inFlight.get(message.id);
        if (pending === undefined) {
          this.#rejectProtocolFault(CONNECTION_COPY.uncorrelated);
          return;
        }
        this.#inFlight.delete(message.id);
        pending.reject(new WireError(message.code, message.message));
      }
    }
  }

  #refuse(id: number, code: string, message: string): void {
    if (this.#negotiated === null) {
      if (id !== this.#helloId) {
        this.#rejectConversation('protocol-mismatch', CONNECTION_COPY.protocolMismatch);
        return;
      }
      // A handshake refusal cannot be repaired by replaying calls on the same conversation, so
      // this is where retrying stops. Everything waiting receives the service's own sentence.
      this.#rejectConversation(code, message);
      return;
    }
    const pending = this.#inFlight.get(id);
    if (pending === undefined) {
      this.#rejectProtocolFault(CONNECTION_COPY.uncorrelated);
      return;
    }
    this.#inFlight.delete(id);
    pending.reject(new WireError(code, message));
  }

  #rejectConversation(code: string, message: string): void {
    this.#clearTimer();
    const link = this.#link;
    this.#link = null;
    this.#helloId = null;
    this.#negotiated = null;
    this.#subscriptionId = null;
    this.#chunks.reset();
    this.#subscriptionSequence = 0;
    this.#subscriptionWanted = false;
    this.#unusableCode = code;
    this.#unusableSentence = message;
    link?.close();
    this.#setState('unusable');
    const waiting = [...this.#inFlight.values(), ...this.#queue];
    this.#inFlight.clear();
    this.#queue.length = 0;
    for (const pending of waiting) pending.reject(new WireError(code, message));
  }

  #rejectProtocolFault(message: string): void {
    this.#fault(message);
    this.#rejectCorruptConversation();
  }

  /**
   * Permanently stop trusting a conversation whose bytes or correlation are invalid.
   *
   * Calls still in `#queue` were never sent, so protocol mismatch is exact. A sent call whose
   * method is unsafe to replay may already have taken effect, and must retain the stronger
   * `outcome-unknown` result even though this client will not reconnect automatically.
   */
  #rejectCorruptConversation(): void {
    this.#clearTimer();
    const link = this.#link;
    this.#link = null;
    this.#helloId = null;
    this.#negotiated = null;
    this.#subscriptionId = null;
    this.#chunks.reset();
    this.#subscriptionSequence = 0;
    this.#subscriptionWanted = false;
    this.#unusableCode = 'protocol-mismatch';
    this.#unusableSentence = CONNECTION_COPY.protocolMismatch;
    link?.close();
    this.#setState('unusable');

    const sent = [...this.#inFlight.values()];
    const unsent = [...this.#queue];
    this.#inFlight.clear();
    this.#queue.length = 0;
    for (const pending of sent) {
      const retrySafe = methodEntry(pending.method)?.retryAfterDisconnect === true;
      pending.reject(
        retrySafe
          ? new WireError('protocol-mismatch', CONNECTION_COPY.protocolMismatch)
          : new WireError('outcome-unknown', CONNECTION_COPY.outcomeUnknown),
      );
    }
    for (const pending of unsent) {
      pending.reject(new WireError('protocol-mismatch', CONNECTION_COPY.protocolMismatch));
    }
  }

  #fault(message: string): void {
    for (const handler of this.#faultHandlers) handler(message);
  }

  /** Reopen the connection-local event stream that this client had already established. */
  #restoreSubscription(sameDaemon: boolean): void {
    if (!this.#subscriptionWanted) return;
    const alreadyPending =
      [...this.#inFlight.values(), ...this.#queue].some(
        (pending) => pending.method === 'events.subscribe',
      );
    if (alreadyPending) return;
    this.#queue.unshift({
      method: 'events.subscribe',
      params: { after_sequence: sameDaemon ? this.#subscriptionSequence : 0 },
      id: null,
      resolve: () => {},
      reject: (error) => {
        if (this.#state === 'stopped' || this.#state === 'unusable') return;
        this.#fault(error.message);
        this.#rejectConversation(
          error instanceof WireError ? error.code : 'protocol-mismatch',
          error.message,
        );
      },
    });
  }

  #drain(): void {
    const link = this.#link;
    const version = this.#negotiated;
    if (link === null || version === null || this.#state !== 'connected') return;
    while (this.#queue.length > 0) {
      const pending = this.#queue.shift();
      if (pending === undefined) break;
      if (!isCallableAt(pending.method, version)) {
        pending.reject(
          new WireError('method-newer-than-surface', CONNECTION_COPY.methodTooNew),
        );
        continue;
      }
      const id = this.#take();
      pending.id = id;
      this.#inFlight.set(id, pending);
      this.#callsSent += 1;
      link.send(encodeClient({ t: 'call', id, method: pending.method, version, params: pending.params }));
    }
  }
}
