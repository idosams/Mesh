import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import {
  CHUNK_DATA_BYTES,
  DaemonChunkAssembler,
  MAX_LINE_BYTES,
  MAX_JSON_DEPTH,
  PROTOCOL,
  WireError,
  decodeClient,
  decodeDaemon,
  encodeClient,
  encodeDaemon,
  splitLines,
} from './protocol.ts';

describe('the wire protocol', () => {
  it('encodes a hello with the key order the contract declares', () => {
    assert.equal(
      encodeClient({ t: 'hello', id: 1, versions: [1], session: 'desktop-01' }),
      '{"t":"hello","id":1,"protocol":"mesh-ipc","versions":[1],"session":"desktop-01"}',
    );
  });

  it('encodes a call with the key order the contract declares', () => {
    assert.equal(
      encodeClient({ t: 'call', id: 2, method: 'daemon.status', version: 1, params: {} }),
      '{"t":"call","id":2,"method":"daemon.status","version":1,"params":{}}',
    );
  });

  it('round-trips every daemon reply', () => {
    const replies = [
      { t: 'welcome', id: 1, version: 1, session: 'a', resumed: false, surfaceVersion: 1 },
      { t: 'refused', id: 1, code: 'unsupported-version', message: 'no', supported: [1] },
      { t: 'result', id: 2, value: { serving: true } },
      { t: 'failed', id: 3, code: 'unknown-method', message: 'no' },
    ] as const;
    for (const reply of replies) {
      assert.deepEqual(decodeDaemon(encodeDaemon(reply)), reply);
    }
  });

  it('reassembles a large daemon response and refuses reordered chunks', () => {
    const complete = encodeDaemon({ t: 'result', id: 9, value: { payload: 'x'.repeat(MAX_LINE_BYTES * 2) } });
    const bytes = Buffer.from(complete, 'utf8');
    const parts = Math.ceil(bytes.length / CHUNK_DATA_BYTES);
    const frames = Array.from({ length: parts }, (_, index) => ({
      t: 'chunk' as const,
      id: 9,
      index,
      parts,
      totalBytes: bytes.length,
      hex: bytes.subarray(index * CHUNK_DATA_BYTES, (index + 1) * CHUNK_DATA_BYTES).toString('hex'),
    }));
    const assembler = new DaemonChunkAssembler();
    let result = null;
    for (const frame of frames) result = assembler.push(decodeDaemon(encodeDaemon(frame)));
    assert.equal(result?.t, 'result');
    assert.equal(result === null || result.t !== 'result' ? null : result.value['payload'], 'x'.repeat(MAX_LINE_BYTES * 2));

    const reordered = new DaemonChunkAssembler();
    assert.throws(() => reordered.push(frames[1]!), WireError);
  });

  it('round-trips every client message', () => {
    const messages = [
      { t: 'hello', id: 1, versions: [1], session: 'a' },
      { t: 'call', id: 2, method: 'daemon.status', version: 1, params: { a: 1 } },
    ] as const;
    for (const message of messages) {
      assert.deepEqual(decodeClient(encodeClient(message)), message);
    }
  });

  it('refuses a protocol name that is not ours', () => {
    const line = JSON.stringify({ t: 'hello', id: 1, protocol: 'other', versions: [1], session: 'a' });
    assert.throws(() => decodeClient(line), (error: WireError) => error.code === 'malformed-message');
  });

  it('refuses everything outside the message set', () => {
    const cases: [string, string][] = [
      ['not json', 'not-json'],
      [JSON.stringify({ t: 'shutdown', id: 1 }), 'unknown-message'],
      [JSON.stringify({ t: 'call', id: 1, method: 'a', version: 1 }), 'malformed-message'],
      [JSON.stringify({ t: 'call', id: 1, method: 'a', version: 1, params: [] }), 'malformed-message'],
      [JSON.stringify({ t: 'hello', id: 1, protocol: PROTOCOL, versions: [], session: 'a' }), 'malformed-message'],
      [JSON.stringify({ t: 'call', id: -1, method: 'a', version: 1, params: {} }), 'malformed-message'],
      [JSON.stringify({ t: 'call', id: 1.5, method: 'a', version: 1, params: {} }), 'malformed-message'],
      [JSON.stringify([1, 2, 3]), 'malformed-message'],
    ];
    for (const [line, code] of cases) {
      assert.throws(
        () => decodeClient(line),
        (error: WireError) => error.code === code,
        `accepted, or gave the wrong code: ${line}`,
      );
    }
  });

  it('refuses a daemon reply that is missing a field', () => {
    const cases: [string, string][] = [
      [JSON.stringify({ t: 'welcome', id: 1, version: 1, session: 'a', surface_version: 1 }), 'malformed-message'],
      [JSON.stringify({ t: 'result', id: 1 }), 'malformed-message'],
      [JSON.stringify({ t: 'refused', id: 1, code: 'x', message: 'y', supported: [] }), 'malformed-message'],
      [JSON.stringify({ t: 'nope', id: 1 }), 'unknown-message'],
    ];
    for (const [line, code] of cases) {
      assert.throws(() => decodeDaemon(line), (error: WireError) => error.code === code, `accepted: ${line}`);
    }
  });

  it('refuses text outside the daemon published field bounds', () => {
    const cases = [
      JSON.stringify({ t: 'hello', id: 1, protocol: PROTOCOL, versions: [1], session: '' }),
      JSON.stringify({ t: 'hello', id: 1, protocol: PROTOCOL, versions: [1], session: '🙂'.repeat(33) }),
      JSON.stringify({ t: 'call', id: 1, method: '', version: 1, params: {} }),
      JSON.stringify({ t: 'call', id: 1, method: 'm'.repeat(65), version: 1, params: {} }),
      JSON.stringify({ t: 'event', id: 1, sequence: 1, kind: '', value: {} }),
      JSON.stringify({ t: 'event', id: 1, sequence: 1, kind: 'k'.repeat(65), value: {} }),
    ];
    for (const line of cases) {
      const decode = line.includes('"t":"event"') ? decodeDaemon : decodeClient;
      assert.throws(
        () => decode(line),
        (error: WireError) => error.code === 'malformed-message',
        `accepted text outside the shared byte bounds: ${line}`,
      );
    }
  });

  it('refuses duplicate keys before JavaScript can silently replace an earlier value', () => {
    const cases = [
      '{"t":"failed","t":"result","id":1,"value":{}}',
      '{"t":"result","id":1,"value":{"serving":true,"serving":false}}',
      '{"t":"result","id":1,"value":{"id":1,"\\u0069d":2}}',
    ];
    for (const line of cases) {
      assert.throws(
        () => decodeDaemon(line),
        (error: WireError) => error.code === 'malformed-message' && /duplicate key/u.test(error.message),
        `accepted duplicate key: ${line}`,
      );
    }

    assert.deepEqual(
      decodeDaemon('{"t":"result","id":1,"value":{"left":{"same":1},"right":{"same":2}}}'),
      { t: 'result', id: 1, value: { left: { same: 1 }, right: { same: 2 } } },
    );
    const escaped = {
      t: 'result' as const,
      id: 1,
      value: { 'quoted"key': '{[,]}', 'slash\\key': [{ same: 1 }, { same: 2 }] },
    };
    assert.deepEqual(decodeDaemon(JSON.stringify(escaped)), escaped);
  });

  it('refuses a second outer-message encoding instead of silently rewriting it', () => {
    const clientCases = [
      '{ "t":"call","id":1,"method":"daemon.status","version":1,"params":{} }',
      '{"id":1,"t":"call","method":"daemon.status","version":1,"params":{}}',
      '{"t":"call","id":1,"method":"daemon.status","version":1,"params":{},"shadow":true}',
      '{"t":"call","id":1,"method":"daemon.\\u0073tatus","version":1,"params":{}}',
    ];
    const daemonCases = [
      '{ "t":"failed","id":1,"code":"x","message":"m" }',
      '{"id":1,"t":"failed","code":"x","message":"m"}',
      '{"t":"failed","id":1,"code":"x","message":"m","shadow":true}',
      '{"t":"failed","id":1,"code":"\\u0078","message":"m"}',
    ];
    for (const line of clientCases) {
      assert.throws(
        () => decodeClient(line),
        (error: WireError) => error.code === 'malformed-message' && /one published key order/u.test(error.message),
        `accepted a second client-message encoding: ${line}`,
      );
    }
    for (const line of daemonCases) {
      assert.throws(
        () => decodeDaemon(line),
        (error: WireError) => error.code === 'malformed-message' && /one published key order/u.test(error.message),
        `accepted a second daemon-message encoding: ${line}`,
      );
    }
  });

  it('refuses values that JavaScript would widen or silently change outside the daemon subset', () => {
    const cases = [
      '{"t":"result","id":1,"value":{"count":9007199254740993}}',
      '{"t":"result","id":1,"value":{"fraction":1.5}}',
      '{"t":"result","id":1,"value":{"negative":-1}}',
      '{"t":"result","id":1,"value":{"exponent":1e3}}',
      '{"t":"result","id":1,"value":{"bad":"\\ud800"}}',
      '{"t":"result","id":1,"value":{"\\udfff":"bad"}}',
    ];
    for (const line of cases) {
      assert.throws(
        () => decodeDaemon(line),
        (error: WireError) => error.code === 'malformed-message',
        `accepted a value outside the shared subset: ${line}`,
      );
    }

    assert.deepEqual(
      decodeDaemon('{"t":"result","id":1,"value":{"count":9007199254740991,"face":"😀"}}'),
      { t: 'result', id: 1, value: { count: 9007199254740991, face: '😀' } },
    );
  });

  it('keeps duplicate-key inspection inside the daemon nesting bound', () => {
    const nested = `${'['.repeat(MAX_JSON_DEPTH)}null${']'.repeat(MAX_JSON_DEPTH)}`;
    assert.throws(
      () => decodeDaemon(`{"t":"result","id":1,"value":{"nested":${nested}}}`),
      (error: WireError) => error.code === 'malformed-message' && /nesting deeper/u.test(error.message),
    );
  });

  it('refuses a line at or over the framing limit', () => {
    const line = JSON.stringify({ t: 'result', id: 1, value: { a: 'x'.repeat(MAX_LINE_BYTES) } });
    assert.throws(() => decodeDaemon(line), (error: WireError) => error.code === 'line-too-long');
  });

  it('reassembles a message split across two reads', () => {
    const first = splitLines('{"a":1}\n{"b":');
    assert.deepEqual(first.lines, ['{"a":1}']);
    assert.equal(first.rest, '{"b":');
    const second = splitLines(`${first.rest}2}\n`);
    assert.deepEqual(second.lines, ['{"b":2}']);
    assert.equal(second.rest, '');
  });

  it('refuses an unfinished line once it reaches the framing limit', () => {
    assert.throws(
      () => splitLines('x'.repeat(MAX_LINE_BYTES)),
      (error: WireError) =>
        error.code === 'line-too-long' && /unfinished line/u.test(error.message),
    );

    const belowLimit = splitLines(`${'\u{1f642}'.repeat(Math.floor((MAX_LINE_BYTES - 1) / 4))}abc`);
    assert.equal(Buffer.byteLength(belowLimit.rest, 'utf8'), MAX_LINE_BYTES - 1);
  });

  it('drops blank lines rather than treating them as messages', () => {
    assert.deepEqual(splitLines('\n\n{"a":1}\n').lines, ['{"a":1}']);
  });
});
