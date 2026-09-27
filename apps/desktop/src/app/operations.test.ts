// "Every UI operation maps to a versioned IPC call."
//
// Checked in both directions, because one direction alone leaves half the sentence untested: an
// operation with no method is an interface offering something that cannot happen. Every desktop
// method needs an operation. Agent-only methods must instead be refused by this client; the real
// native MCP process integration tests exercise their dedicated transport.

import assert from 'node:assert/strict';
import { after, describe, it } from 'node:test';

import { FakeDaemon } from '../../test-support/fake-daemon.ts';
import { freshEndpoint, waitUntil } from '../../test-support/endpoint.ts';
import { DaemonConnection } from '../ipc/client.ts';
import { METHODS, methodEntry } from '../ipc/methods.ts';
import { CLIENT_VERSIONS } from '../ipc/protocol.ts';
import { OPERATION_COPY } from '../strings/operations.ts';
import { UI_OPERATIONS, copyFor, run, uiOperation } from './operations.ts';

const newest = Math.max(...CLIENT_VERSIONS);

const teardown: (() => Promise<void>)[] = [];
after(async () => {
  for (const step of teardown) await step();
});

describe('the operation catalogue', () => {
  it('offers at least one operation', () => {
    assert.ok(UI_OPERATIONS.length > 0);
  });

  it('maps every operation to a method that exists at the version it names', () => {
    for (const operation of UI_OPERATIONS) {
      const entry = methodEntry(operation.method);
      assert.ok(entry !== undefined, `\`${operation.id}\` names \`${operation.method}\`, which is not on the surface`);
      assert.ok(
        entry.since <= operation.version,
        `\`${operation.id}\` calls version ${operation.version}, but \`${operation.method}\` arrived at ${entry.since}`,
      );
      assert.ok(
        operation.version <= newest,
        `\`${operation.id}\` calls version ${operation.version}, which this client cannot negotiate`,
      );
    }
  });

  it('reaches every desktop method and exposes no agent credential transport', () => {
    const called = new Set(UI_OPERATIONS.map((operation) => operation.method));
    for (const entry of METHODS) {
      if (entry.agentOnly) {
        assert.equal(called.has(entry.name), false, 'agent transport must not become a UI operation');
        continue;
      }
      assert.ok(called.has(entry.name), `no operation calls \`${entry.name}\``);
    }
  });

  it('refuses the exact agent-only surface before queuing or opening a connection', async () => {
    const agentMethods = METHODS.filter((entry) => entry.agentOnly).map((entry) => entry.name);
    assert.deepEqual(agentMethods, ['fleet.agent.call']);
    const connection = new DaemonConnection({ endpoint: '/nowhere/daemon.sock', session: 'desktop-01' });
    await assert.rejects(connection.call('fleet.agent.call', { credential: 'must-not-be-queued' }),
      (error: unknown) => error instanceof Error && 'code' in error && error.code === 'agent-transport-required');
    assert.equal(connection.state, 'idle');
  });

  it('has unique identifiers', () => {
    const ids = UI_OPERATIONS.map((operation) => operation.id);
    assert.equal(new Set(ids).size, ids.length, 'two operations share an identifier');
  });

  it('has words for every operation, and no words for an operation that does not exist', () => {
    for (const operation of UI_OPERATIONS) {
      const copy = copyFor(operation.id);
      assert.ok(copy !== undefined, `\`${operation.id}\` has no copy`);
      assert.ok(copy.label.length > 0 && copy.description.length > 0);
    }
    for (const id of Object.keys(OPERATION_COPY)) {
      assert.ok(uiOperation(id) !== undefined, `copy exists for \`${id}\`, which is not an operation`);
    }
  });

  it('refuses an identifier it does not have, without touching the connection', async () => {
    const connection = new DaemonConnection({ endpoint: '/nowhere/daemon.sock', session: 'desktop-01' });
    await assert.rejects(run(connection, 'nothing.like.this'), /is not an operation/);
  });

  it('performs every operation end to end', async () => {
    const endpoint = await freshEndpoint('operations');
    const daemon = new FakeDaemon(endpoint.path);
    await daemon.start();
    const connection = new DaemonConnection({
      endpoint: endpoint.path,
      session: 'desktop-01',
      retryDelaysMs: [5, 10],
    });
    teardown.push(async () => {
      connection.stop();
      await daemon.stop();
      await endpoint.cleanup();
    });
    await connection.start();
    await waitUntil('the connection is ready', () => connection.state === 'connected');

    for (const operation of UI_OPERATIONS) {
      // An operation that declares inputs is given each one. Passing `undefined` to it would
      // measure the refusal path while looking like it measured the answer.
      const supplied = Object.fromEntries(
        (operation.inputs ?? []).map((input, index) => [input.name, `${index + 1}`.repeat(64)]),
      );
      const answer = await run(
        connection,
        operation.id,
        operation.inputs === undefined ? undefined : supplied,
      );
      assert.equal(typeof answer, 'object');
      assert.notDeepEqual(answer, {}, `\`${operation.id}\` answered nothing`);
    }
    assert.equal(daemon.callsAnswered, UI_OPERATIONS.length);
  });

  it('refuses an operation whose argument is missing, without sending anything', async () => {
    const connection = new DaemonConnection({ endpoint: '/nowhere/daemon.sock', session: 'desktop-01' });
    const needsOne = UI_OPERATIONS.find((operation) => operation.inputs !== undefined);
    assert.ok(needsOne !== undefined, 'no operation takes an argument, so this rule cannot bite');
    await assert.rejects(run(connection, needsOne.id), /did not get it/);
    assert.equal(connection.stats().callsSent, 0, 'a refused operation still reached the socket');
  });

  it('refuses each missing review input before anything reaches the socket', async () => {
    const connection = new DaemonConnection({ endpoint: '/nowhere/daemon.sock', session: 'desktop-01' });
    for (const id of ['review.open', 'review.approve']) {
      const operation = uiOperation(id);
      assert.ok(operation?.inputs !== undefined);
      const complete = Object.fromEntries(operation.inputs.map((input) => [input.name, '04'.repeat(32)]));
      for (const omitted of operation.inputs) {
        const incomplete = { ...complete };
        delete incomplete[omitted.name];
        await assert.rejects(run(connection, id, incomplete), /did not get it/);
      }
    }
    assert.equal(connection.stats().callsSent, 0, 'an incomplete review operation reached the socket');
  });

  it('names every input for the methods whose parameters a person supplies', () => {
    for (const operation of UI_OPERATIONS) {
      if (operation.inputs === undefined) continue;
      assert.ok(operation.inputs.length > 0, `\`${operation.id}\` has an empty input list`);
      for (const input of operation.inputs) {
        assert.ok(input.name.length > 0, `\`${operation.id}\` has a nameless input`);
        assert.ok(input.what.length > 0, `\`${operation.id}\` cannot say what it needs`);
      }
    }
  });
});
