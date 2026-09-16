// The connection lifecycle, over a REAL Unix-domain socket against the stand-in service in
// `test-support/fake-daemon.ts`. Nothing here is mocked at the socket layer: every assertion below
// survived actual bytes, actual buffering and actual timing.

import assert from 'node:assert/strict';
import { after, describe, it } from 'node:test';

import { FakeDaemon } from '../../test-support/fake-daemon.ts';
import { freshEndpoint, pause, waitUntil } from '../../test-support/endpoint.ts';
import { DaemonConnection, type ConnectionState, type DaemonEvent } from './client.ts';
import { CONNECTION_COPY } from '../strings/connection.ts';
import { WireError, type WireObject } from './protocol.ts';

/** Every fixture built in this file, torn down at the end whatever happens. */
const teardown: (() => Promise<void>)[] = [];
after(async () => {
  for (const step of teardown) await step();
});

type Fixture = {
  readonly daemon: FakeDaemon;
  readonly connection: DaemonConnection;
  readonly states: ConnectionState[];
  readonly context: Record<string, unknown>;
};

const fixture = async (
  name: string,
  options: {
    supported?: number[];
    answerDelayMs?: number;
    start?: boolean;
    welcomeOverride?: {
      id?: number;
      version?: number;
      session?: string;
      surfaceVersion?: number;
    };
    malformedReplyFor?: string;
    uncorrelatedReplyFor?: string;
    unsolicitedEventFor?: string;
    eventOverride?: { id?: number; sequence?: number };
    duplicateEventSequence?: boolean;
    subscriptionBacklogLost?: boolean;
    firstEventSequence?: number;
    subscriptionResult?: Record<string, null | boolean | number | string>;
    performanceCounters?: WireObject;
    workspace?: WireObject;
    chunkLargeReplies?: boolean;
  } = {},
): Promise<Fixture> => {
  const endpoint = await freshEndpoint(name);
  const daemon = new FakeDaemon(endpoint.path, {
    supported: options.supported ?? [1],
    answerDelayMs: options.answerDelayMs ?? 0,
    welcomeOverride: options.welcomeOverride,
    malformedReplyFor: options.malformedReplyFor,
    uncorrelatedReplyFor: options.uncorrelatedReplyFor,
    unsolicitedEventFor: options.unsolicitedEventFor,
    eventOverride: options.eventOverride,
    duplicateEventSequence: options.duplicateEventSequence,
    subscriptionBacklogLost: options.subscriptionBacklogLost,
    firstEventSequence: options.firstEventSequence,
    subscriptionResult: options.subscriptionResult,
    performanceCounters: options.performanceCounters,
    workspace: options.workspace,
    chunkLargeReplies: options.chunkLargeReplies,
  });
  if (options.start !== false) await daemon.start();
  const context: Record<string, unknown> = { openWorkspace: 'design-system', scrollTop: 42 };
  const connection = new DaemonConnection({
    endpoint: endpoint.path,
    session: 'desktop-01',
    retryDelaysMs: [5, 10],
    context,
  });
  const states: ConnectionState[] = [];
  connection.onState((state) => states.push(state));
  teardown.push(async () => {
    connection.stop();
    await daemon.stop();
    await endpoint.cleanup();
  });
  return { daemon, connection, states, context };
};

describe('the connection to the background service', () => {
  it('negotiates a version and then answers calls', async () => {
    const { connection } = await fixture('negotiate');
    await connection.start();
    await waitUntil('the connection is ready', () => connection.state === 'connected');
    assert.equal(connection.stats().negotiatedVersion, 1);

    const status = await connection.call('daemon.status');
    assert.equal(status['serving'], true);

    const described = await connection.call('surface.describe');
    assert.equal(described['protocol'], 'mesh-ipc');
  });

  it('returns a complete large workspace through bounded surface-v7 frames', async () => {
    const paths = Array.from({ length: 4_000 }, (_, index) => `src/generated-${index}.rs`);
    const { connection } = await fixture('large-state', {
      supported: [7],
      workspace: { root: '/native/large', entries: paths },
      chunkLargeReplies: true,
    });
    await connection.start();
    await waitUntil('the connection is ready', () => connection.state === 'connected');
    const state = await connection.call('workspace.state');
    assert.deepEqual(state['entries'], paths);
  });

  it('holds a call made before the connection is open, and sends it once it is', async () => {
    const { connection } = await fixture('queued');
    const pending = connection.call('daemon.status');
    assert.equal(connection.stats().queueDepth, 1);
    await connection.start();
    const status = await pending;
    assert.equal(status['serving'], true);
    assert.equal(connection.stats().queueDepth, 0);
  });

  it('waits for the service rather than failing when it is not running yet', async () => {
    const { daemon, connection } = await fixture('not-yet', { start: false });
    await connection.start();
    await waitUntil('the first attempt has failed', () => connection.state === 'reconnecting');
    const pending = connection.call('daemon.status');
    await daemon.start();
    assert.equal((await pending)['serving'], true);
    assert.equal(connection.state, 'connected');
  });

  describe('when the background service restarts', () => {
    it('reconnects, and the application context is the same object it was', async () => {
      const { daemon, connection, context } = await fixture('restart');
      await connection.start();
      await waitUntil('ready', () => connection.state === 'connected');
      assert.equal((await connection.call('daemon.status'))['serving'], true);

      const before = connection.context;
      await daemon.stop();
      await waitUntil('the outage is noticed', () => connection.state === 'reconnecting');

      await daemon.start();
      await waitUntil('the link is back', () => connection.state === 'connected');

      // The four things the acceptance criterion is about.
      assert.equal(connection.context, before, 'the context object was replaced');
      assert.equal(connection.context, context, 'the context is not the one the app handed in');
      assert.equal(connection.context['openWorkspace'], 'design-system');
      assert.equal(connection.session, 'desktop-01');
      assert.deepEqual(daemon.sessionsSeen, ['desktop-01'], 'the session was not re-announced');
      assert.equal(connection.stats().reconnects, 1);

      // And the connection is usable again.
      assert.equal((await connection.call('daemon.status'))['serving'], true);
    });

    it('re-issues a call that was in flight when the link died', async () => {
      const { daemon, connection } = await fixture('inflight', { answerDelayMs: 250 });
      await connection.start();
      await waitUntil('ready', () => connection.state === 'connected');

      const pending = connection.call('startup.report');
      await waitUntil('the call is on the wire', () => connection.stats().callsSent === 1);

      await daemon.stop();
      await waitUntil('the outage is noticed', () => connection.state === 'reconnecting');
      assert.equal(connection.stats().callsReissued, 1);

      const answered = new FakeDaemon(daemon.endpoint, { supported: [1], answerDelayMs: 0 });
      teardown.push(() => answered.stop());
      await answered.start();

      const report = await pending;
      assert.equal(report['serving'], true);
      assert.equal(connection.stats().callsSent, 2, 'the call was sent again rather than dropped');
    });

    it('does not replay an approval whose outcome became unknown', async () => {
      const { daemon, connection } = await fixture('approval-outcome', {
        supported: [1, 2],
        answerDelayMs: 250,
      });
      await connection.start();
      await waitUntil('ready', () => connection.state === 'connected');

      const pending = assert.rejects(
        connection.call('review.approve', {}),
        (error: WireError) =>
          error.code === 'outcome-unknown' && error.message === CONNECTION_COPY.outcomeUnknown,
      );
      await waitUntil('the approval is on the wire', () => connection.stats().callsSent === 1);

      await daemon.stop();
      await waitUntil('the outage is noticed', () => connection.state === 'reconnecting');
      await pending;
      assert.equal(connection.stats().callsReissued, 0);

      const answered = new FakeDaemon(daemon.endpoint, { supported: [1, 2] });
      teardown.push(() => answered.stop());
      await answered.start();
      await waitUntil('the link is back', () => connection.state === 'connected');
      await pause(20);

      assert.equal(connection.stats().callsSent, 1, 'the uncertain approval was sent again');
      assert.equal(answered.callsAnswered, 0, 'the replacement service received the approval');
    });

    it('does not replay a workspace selection over a newer client choice', async () => {
      const { daemon, connection } = await fixture('workspace-open-outcome', {
        supported: [1, 2],
        answerDelayMs: 250,
      });
      await connection.start();
      await waitUntil('ready', () => connection.state === 'connected');

      const pending = assert.rejects(
        connection.call('workspace.open', { path: '/managed/workspace-a' }),
        (error: WireError) =>
          error.code === 'outcome-unknown' && error.message === CONNECTION_COPY.outcomeUnknown,
      );
      await waitUntil('the workspace selection is on the wire', () => connection.stats().callsSent === 1);

      await daemon.stop();
      await waitUntil('the lost reply is noticed', () => connection.state === 'reconnecting');
      await pending;
      assert.equal(connection.stats().callsReissued, 0);

      // This replacement service represents another client having selected workspace B while
      // the first client was disconnected. Reconnection must not resend workspace A and replace
      // that newer daemon-global selection.
      const answered = new FakeDaemon(daemon.endpoint, { supported: [1, 2] });
      teardown.push(() => answered.stop());
      await answered.start();
      await waitUntil('the link is back', () => connection.state === 'connected');
      await pause(20);

      assert.equal(connection.stats().callsSent, 1, 'the uncertain workspace selection was sent again');
      assert.equal(answered.callsAnswered, 0, 'the replacement service received the stale workspace selection');
    });

    it('keeps every state subscriber across the outage', async () => {
      const { daemon, connection, states } = await fixture('subscribers');
      await connection.start();
      await waitUntil('ready', () => connection.state === 'connected');
      await daemon.stop();
      await waitUntil('the outage is noticed', () => connection.state === 'reconnecting');
      await daemon.start();
      await waitUntil('the link is back', () => connection.state === 'connected');
      assert.deepEqual(states, ['opening', 'connected', 'reconnecting', 'connected']);
    });
  });

  describe('when the two ends cannot agree', () => {
    it('stops retrying and says so in the service’s own words', async () => {
      const { connection } = await fixture('mismatch', { supported: [99] });
      // The assertion is attached in the same turn the call is made: the refusal arrives on a
      // socket event some milliseconds later, and a promise left bare until then is an unhandled
      // rejection rather than a test.
      const pending = assert.rejects(
        connection.call('daemon.status'),
        (error: WireError) => error.code === 'unsupported-version',
      );
      await connection.start();
      await waitUntil('the mismatch is settled', () => connection.state === 'unusable');
      await pending;
      assert.equal(connection.sentence, CONNECTION_COPY.unusable);

      // A later call fails immediately rather than queueing behind a retry that cannot help.
      await assert.rejects(connection.call('daemon.status'), (error: WireError) => error.code === 'unsupported-version');

      // And it stays settled: nothing rescheduled a reconnection behind it.
      await pause(30);
      assert.equal(connection.state, 'unusable');
    });

    for (const [fixtureName, name, welcomeOverride] of [
      ['id', 'another request', { id: 99 }],
      ['session', 'another session', { session: 'desktop-02' }],
      ['version', 'a version the client did not offer', { version: 99, surfaceVersion: 99 }],
      ['surface', 'an impossible older surface', { version: 1, surfaceVersion: 0 }],
    ] as const) {
      it(`refuses a welcome for ${name} before sending a queued operation`, async () => {
        const { daemon, connection } = await fixture(`bad-welcome-${fixtureName}`, {
          welcomeOverride,
        });
        const pending = assert.rejects(
          connection.call('daemon.status'),
          (error: WireError) =>
            error.code === 'protocol-mismatch' &&
            error.message === CONNECTION_COPY.protocolMismatch,
        );

        await connection.start();
        await waitUntil('the invalid welcome is refused', () => connection.state === 'unusable');
        await pending;

        assert.equal(connection.stats().callsSent, 0, 'a call followed an unauthenticated welcome');
        assert.equal(daemon.callsAnswered, 0, 'the daemon received a call after its invalid welcome');
        assert.equal(connection.sentence, CONNECTION_COPY.protocolMismatch);
      });
    }

    it('refuses a supported version below the newest common surface', async () => {
      const { daemon, connection } = await fixture('bad-welcome-downgrade', {
        supported: [1, 2, 3],
        welcomeOverride: { version: 1, surfaceVersion: 3 },
      });
      const pending = assert.rejects(
        connection.call('daemon.status'),
        (error: WireError) =>
          error.code === 'protocol-mismatch' &&
          error.message === CONNECTION_COPY.protocolMismatch,
      );

      await connection.start();
      await waitUntil('the downgraded welcome is refused', () => connection.state === 'unusable');
      await pending;

      assert.equal(connection.stats().callsSent, 0, 'a call followed a downgraded welcome');
      assert.equal(daemon.callsAnswered, 0, 'the daemon received a call after its downgraded welcome');
      assert.equal(connection.sentence, CONNECTION_COPY.protocolMismatch);
    });

    it('refuses a method that is not on the surface without sending anything', async () => {
      const { daemon, connection } = await fixture('offsurface');
      await connection.start();
      await waitUntil('ready', () => connection.state === 'connected');
      const before = daemon.callsAnswered;
      await assert.rejects(
        connection.call('store.write'),
        (error: WireError) => error.code === 'unknown-method',
      );
      assert.equal(daemon.callsAnswered, before, 'an off-surface call reached the socket');
    });
  });

  it('rejects everything still waiting when the window closes', async () => {
    const { connection } = await fixture('stop', { start: false });
    const pending = connection.call('daemon.status');
    await connection.start();
    connection.stop();
    await assert.rejects(pending, (error: WireError) => error.code === 'stopped');
    assert.equal(connection.state, 'stopped');
    await pause(30);
    assert.equal(connection.state, 'stopped', 'a retry was still scheduled after stop');
  });

  it('reports a reply it cannot correlate as a fault rather than as an answer', async () => {
    const { connection } = await fixture('fault');
    const faults: string[] = [];
    connection.onFault((message) => faults.push(message));
    await connection.start();
    await waitUntil('ready', () => connection.state === 'connected');
    await connection.call('daemon.status');
    assert.deepEqual(faults, [], 'a well-behaved exchange produced a fault');
  });

  it('terminates a corrupted conversation instead of leaving a mutation pending', async () => {
    const { daemon, connection } = await fixture('malformed-reply', {
      supported: [1, 2],
      malformedReplyFor: 'review.approve',
    });
    const faults: string[] = [];
    connection.onFault((message) => faults.push(message));
    await connection.start();
    await waitUntil('ready', () => connection.state === 'connected');

    await assert.rejects(
      connection.call('review.approve', {}),
      (error: WireError) => error.code === 'outcome-unknown',
    );

    assert.equal(connection.state, 'unusable');
    assert.equal(connection.sentence, CONNECTION_COPY.protocolMismatch);
    assert.equal(connection.stats().queueDepth, 0);
    assert.equal(connection.stats().callsReissued, 0, 'the uncertain mutation was replayed');
    assert.equal(daemon.callsAnswered, 0, 'the malformed answer was counted as a valid answer');
    assert.equal(faults.length, 1, 'the protocol decoder fault was hidden');
  });

  it('terminates a corrupted conversation with an exact mismatch for a retry-safe read', async () => {
    const { daemon, connection } = await fixture('malformed-read-reply', {
      supported: [1, 2],
      malformedReplyFor: 'daemon.status',
    });
    const faults: string[] = [];
    connection.onFault((message) => faults.push(message));
    await connection.start();
    await waitUntil('ready', () => connection.state === 'connected');

    await assert.rejects(
      connection.call('daemon.status'),
      (error: WireError) => error.code === 'protocol-mismatch',
    );

    assert.equal(connection.state, 'unusable');
    assert.equal(connection.sentence, CONNECTION_COPY.protocolMismatch);
    assert.equal(connection.stats().queueDepth, 0);
    assert.equal(connection.stats().callsReissued, 0, 'the corrupt peer was contacted again');
    assert.equal(daemon.callsAnswered, 0, 'the malformed answer was counted as a valid answer');
    assert.equal(faults.length, 1, 'the protocol decoder fault was hidden');
  });

  it('terminates a validly encoded but uncorrelated mutation reply', async () => {
    const { daemon, connection } = await fixture('uncorrelated', {
      supported: [1, 2],
      uncorrelatedReplyFor: 'review.approve',
    });
    const faults: string[] = [];
    connection.onFault((message) => faults.push(message));
    await connection.start();
    await waitUntil('ready', () => connection.state === 'connected');

    const rejected = assert.rejects(
      connection.call('review.approve', {}),
      (error: WireError) => error.code === 'outcome-unknown',
    );
    await waitUntil('the uncorrelated reply closes the conversation', () => connection.state === 'unusable');
    await rejected;

    assert.equal(connection.stats().queueDepth, 0);
    assert.equal(connection.stats().callsReissued, 0, 'the uncertain mutation was replayed');
    assert.equal(daemon.callsAnswered, 1, 'the valid but misidentified reply did not reach the client');
    assert.equal(faults.length, 1, 'the correlation fault was hidden');
  });

  it('terminates a counter result that does not prove its exact integer encoding', async () => {
    const { connection } = await fixture('counter-result-shape', {
      supported: [1, 2, 3, 4],
      performanceCounters: {
        conditions: 'measured live',
        collection: {},
        counters: [],
        not_yet: [],
      },
    });
    const faults: string[] = [];
    connection.onFault((message) => faults.push(message));
    await connection.start();
    await waitUntil('ready', () => connection.state === 'connected');

    await assert.rejects(
      connection.call('performance.counters'),
      (error: WireError) => error.code === 'protocol-mismatch',
    );
    assert.equal(connection.state, 'unusable');
    assert.equal(faults.length, 1, 'the malformed counter result was hidden');
  });

  it('terminates an event stream that no subscription opened', async () => {
    const { connection } = await fixture('unsolicited-push', {
      supported: [1, 2],
      unsolicitedEventFor: 'daemon.status',
    });
    const heard: DaemonEvent[] = [];
    const faults: string[] = [];
    connection.onEvent((event) => heard.push(event));
    connection.onFault((message) => faults.push(message));
    await connection.start();
    await waitUntil('ready', () => connection.state === 'connected');

    assert.equal((await connection.call('daemon.status'))['serving'], true);
    await waitUntil('the unsolicited stream is refused', () => connection.state === 'unusable');

    assert.deepEqual(heard, [], 'an event escaped a stream the client never opened');
    assert.equal(faults.length, 1, 'the unsolicited-stream fault was hidden');
  });

  describe('what the service pushes', () => {
    it('delivers a pushed line to subscribers instead of treating it as an answer', async () => {
      const { connection } = await fixture('push', { supported: [1, 2] });
      const heard: DaemonEvent[] = [];
      const faults: string[] = [];
      connection.onEvent((event) => heard.push(event));
      connection.onFault((message) => faults.push(message));
      await connection.start();
      await waitUntil('ready', () => connection.state === 'connected');

      const answer = await connection.call('events.subscribe');
      assert.equal(answer['subscribed'], true);
      await waitUntil('the push arrives', () => heard.length === 1);
      assert.equal(heard[0]?.kind, 'serving');
      assert.equal(heard[0]?.sequence, 1);
      assert.deepEqual(faults, [], 'a pushed line was reported as an uncorrelated reply');
      assert.equal(connection.feed.received, 1);
      assert.equal(connection.feed.latestSequence, 1);
    });

    it('pushes nothing to a connection that never subscribed', async () => {
      const { connection } = await fixture('quiet', { supported: [1, 2] });
      const heard: DaemonEvent[] = [];
      connection.onEvent((event) => heard.push(event));
      await connection.start();
      await waitUntil('ready', () => connection.state === 'connected');
      await connection.call('daemon.status');
      await pause(30);
      assert.deepEqual(heard, [], 'an unsolicited line reached a client that did not ask');
    });

    for (const [fixtureName, name, options] of [
      ['event-id', 'wrong subscription', { eventOverride: { id: 99 } }],
      ['event-zero', 'zero sequence', { eventOverride: { sequence: 0 } }],
    ] as const) {
      it(`terminates a stream with ${name}`, async () => {
        const { connection } = await fixture(fixtureName, {
          supported: [1, 2],
          ...options,
        });
        const heard: DaemonEvent[] = [];
        connection.onEvent((event) => heard.push(event));
        await connection.start();
        await waitUntil('ready', () => connection.state === 'connected');
        await connection.call('events.subscribe');
        await waitUntil('the invalid stream is refused', () => connection.state === 'unusable');
        assert.deepEqual(heard, [], 'an invalid event reached the application');
      });
    }

    it('terminates a stream that repeats an event sequence', async () => {
      const { connection } = await fixture('duplicate-push', {
        supported: [1, 2],
        duplicateEventSequence: true,
      });
      const heard: DaemonEvent[] = [];
      connection.onEvent((event) => heard.push(event));
      await connection.start();
      await waitUntil('ready', () => connection.state === 'connected');

      await connection.call('events.subscribe');
      await waitUntil('the repeated sequence is refused', () => connection.state === 'unusable');

      assert.equal(heard.length, 1, 'the repeated event escaped stream validation');
      assert.equal(connection.feed.latestSequence, 1);
    });

    it('terminates a stream whose first pushed event skips the next sequence', async () => {
      const { connection } = await fixture('missing-event-prefix', {
        supported: [1, 2],
        firstEventSequence: 2,
      });
      const heard: DaemonEvent[] = [];
      connection.onEvent((event) => heard.push(event));
      await connection.start();
      await waitUntil('ready', () => connection.state === 'connected');

      await connection.call('events.subscribe');
      await waitUntil('the missing prefix is refused', () => connection.state === 'unusable');

      assert.deepEqual(heard, [], 'an event after a missing prefix reached an application handler');
      assert.equal(connection.feed.received, 0);
      assert.equal(connection.feed.latestSequence, 0);
    });

    it('fails closed when an admitted subscription loses its bounded backlog', async () => {
      const { connection } = await fixture('feed-wrap', {
        supported: [1, 2],
        subscriptionBacklogLost: true,
        subscriptionResult: { subscribed: true, latest_sequence: 0 },
      });
      const faults: string[] = [];
      connection.onFault((message) => faults.push(message));
      await connection.start();
      await waitUntil('ready', () => connection.state === 'connected');

      await connection.call('events.subscribe');
      await waitUntil('the wrapped feed is refused', () => connection.state === 'unusable');

      await assert.rejects(
        connection.call('daemon.status'),
        (error: WireError) => error.code === 'event-backlog-lost',
      );
      assert.equal(faults.length, 1, 'the asynchronous feed loss was hidden');
      assert.equal(connection.feed.received, 0);
    });

    it('refuses a result that does not prove the event subscription opened', async () => {
      const { connection } = await fixture('false-subscription', {
        supported: [1, 2],
        subscriptionResult: { subscribed: false, latest_sequence: 1 },
      });
      const heard: DaemonEvent[] = [];
      const faults: string[] = [];
      connection.onEvent((event) => heard.push(event));
      connection.onFault((message) => faults.push(message));
      await connection.start();
      await waitUntil('ready', () => connection.state === 'connected');

      await assert.rejects(
        connection.call('events.subscribe'),
        (error: WireError) => error.code === 'protocol-mismatch',
      );

      assert.equal(connection.state, 'unusable');
      assert.deepEqual(heard, [], 'an event escaped a subscription the result did not prove');
      assert.equal(faults.length, 1, 'the malformed subscription result was hidden');
    });

    it('reopens an established event subscription after the background service restarts', async () => {
      const { connection, daemon } = await fixture('push-restart', { supported: [1, 2] });
      const heard: DaemonEvent[] = [];
      connection.onEvent((event) => heard.push(event));
      await connection.start();
      await waitUntil('ready', () => connection.state === 'connected');
      await connection.call('events.subscribe');
      await waitUntil('the first push', () => heard.length === 1);

      await daemon.stop();
      await daemon.start();
      await waitUntil('reconnected', () => connection.state === 'connected');

      // The subscription lives on the old connection, so reconnecting must open a new one itself.
      // Requiring the UI to remember to call again leaves a connected-looking client silently
      // missing every event after an otherwise transparent daemon restart.
      await waitUntil('the push after the restart', () => heard.length === 2);
      assert.equal(heard[1]?.kind, 'serving');
      assert.equal(connection.stats().callsSent, 2, 'the established subscription was not reopened');
    });

    it('resumes a same-process event feed without replaying handlers', async () => {
      const { connection, daemon } = await fixture('push-resume', { supported: [1, 2] });
      const heard: DaemonEvent[] = [];
      connection.onEvent((event) => heard.push(event));
      await connection.start();
      await waitUntil('ready', () => connection.state === 'connected');
      await connection.call('events.subscribe');
      await waitUntil('the first push', () => heard.length === 1);

      daemon.dropConnections();
      await waitUntil('the outage is noticed', () => connection.state === 'reconnecting');
      await waitUntil('the same daemon reconnects', () => connection.state === 'connected');
      await waitUntil(
        'the subscription is restored',
        () => daemon.callsSeen.filter((call) => call.method === 'events.subscribe').length === 2,
      );
      await pause(30);

      assert.equal(heard.length, 1, 'an already delivered feed entry ran its handler twice');
      assert.deepEqual(
        daemon.callsSeen.filter((call) => call.method === 'events.subscribe').map((call) => call.params),
        [{}, { after_sequence: 1 }],
      );
      assert.equal(connection.feed.received, 1);
      assert.equal(connection.feed.latestSequence, 1);
    });

    it('preserves an explicit subscription cursor across a same-process reconnect', async () => {
      const { connection, daemon } = await fixture('push-explicit-cursor', {
        supported: [1, 2],
      });
      const heard: DaemonEvent[] = [];
      connection.onEvent((event) => heard.push(event));
      await connection.start();
      await waitUntil('ready', () => connection.state === 'connected');

      await connection.call('events.subscribe', { after_sequence: 1 });
      await pause(30);
      assert.deepEqual(heard, [], 'an event at or before the explicit cursor was replayed');

      daemon.dropConnections();
      await waitUntil('the outage is noticed', () => connection.state === 'reconnecting');
      await waitUntil('the same daemon reconnects', () => connection.state === 'connected');
      await waitUntil(
        'the explicit subscription is restored',
        () => daemon.callsSeen.filter((call) => call.method === 'events.subscribe').length === 2,
      );
      await pause(30);

      assert.deepEqual(
        daemon.callsSeen.filter((call) => call.method === 'events.subscribe').map((call) => call.params),
        [{ after_sequence: 1 }, { after_sequence: 1 }],
      );
      assert.deepEqual(heard, [], 'the reconnect replayed an event the caller had already consumed');
    });
  });
});
