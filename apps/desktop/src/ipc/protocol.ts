// The wire protocol the desktop application speaks to the Mesh daemon.
//
// This is the TypeScript half of `crates/mesh-daemon/ipc-contract.json`. The daemon owns the
// surface; this file implements it. Neither side may drift, and `contract.test.ts` is what stops
// it: every published vector is decoded here, re-encoded here, and compared BYTE FOR BYTE against
// the same line the Rust implementation round-trips in `crates/mesh-daemon/tests/ipc.rs`.
//
// Two properties make that comparison possible and they are both deliberate:
//
//  1. **Fixed key order.** Every builder below writes its keys in the order the contract declares,
//     and `JSON.stringify` preserves insertion order for string keys. The Rust writer emits the
//     same order with no whitespace.
//  2. **A narrow value subset.** Objects, arrays, strings, booleans, null and NON-NEGATIVE
//     INTEGERS. No floats, no negative numbers, no `undefined`. `JSON.stringify` and the Rust
//     writer agree exactly on that subset, and disagree on almost anything outside it.
//
// This is NOT the canonical signed encoding of the protocol — that is CBOR and it lives in
// `mesh-types`. Nothing on this socket is signed. Saying so here is cheaper than somebody later
// assuming the two are the same thing.

/** The protocol name both ends announce, so a socket that is something else fails fast. */
export const PROTOCOL = 'mesh-ipc';

/** Every surface version this client can speak, newest last. */
export const CLIENT_VERSIONS: readonly number[] = [1, 2, 3, 4, 5, 6, 7];

/** The largest line either end will accept, newline included. */
export const MAX_LINE_BYTES = 65536;

/** Largest logical daemon reply reconstructed from bounded surface-v7 frames. */
export const MAX_MESSAGE_BYTES = 16 * 1024 * 1024;

/** Raw payload bytes in one hex-encoded chunk. */
export const CHUNK_DATA_BYTES = 30000;

/** The deepest value nesting accepted by the daemon's JSON reader. */
export const MAX_JSON_DEPTH = 16;

/** The largest method or event-kind word accepted by the daemon. */
export const MAX_METHOD_BYTES = 64;

/** The largest session identity accepted by the daemon. */
export const MAX_SESSION_BYTES = 128;

/** The correlation identifier the daemon uses for a reply that answers no particular request. */
export const NO_CORRELATION = 0;

/** A value in the JSON subset this surface uses. */
export type WireValue =
  | null
  | boolean
  | number
  | string
  | readonly WireValue[]
  | { readonly [key: string]: WireValue };

/** An object in that subset — every `params` and every `value` is one. */
export type WireObject = { readonly [key: string]: WireValue };

/** What the desktop application sends. */
export type ClientMessage =
  | { readonly t: 'hello'; readonly id: number; readonly versions: readonly number[]; readonly session: string }
  | { readonly t: 'call'; readonly id: number; readonly method: string; readonly version: number; readonly params: WireObject };

/** What the daemon sends back. One logical reply may occupy multiple surface-v7 chunk frames. */
export type DaemonMessage =
  | {
      readonly t: 'welcome';
      readonly id: number;
      readonly version: number;
      readonly session: string;
      readonly resumed: boolean;
      readonly surfaceVersion: number;
    }
  | {
      readonly t: 'refused';
      readonly id: number;
      readonly code: string;
      readonly message: string;
      readonly supported: readonly number[];
    }
  | { readonly t: 'result'; readonly id: number; readonly value: WireObject }
  | { readonly t: 'failed'; readonly id: number; readonly code: string; readonly message: string }
  | {
      // The one line the daemon sends that no request asked for, and only to a connection that
      // called `events.subscribe`. `id` is that call's identifier. `sequence` orders entries on
      // this socket and NOTHING else — it is not an ordering of anybody's work.
      readonly t: 'event';
      readonly id: number;
      readonly sequence: number;
      readonly kind: string;
      readonly value: WireObject;
    }
  | {
      readonly t: 'chunk';
      readonly id: number;
      readonly index: number;
      readonly parts: number;
      readonly totalBytes: number;
      readonly hex: string;
    };

export type LogicalDaemonMessage = Exclude<DaemonMessage, { readonly t: 'chunk' }>;

/**
 * Whether a decoded line answers a request, as opposed to being pushed.
 *
 * The client uses it to keep its own bookkeeping straight: a pushed line must not be matched
 * against the call table and must not clear an in-flight entry. It mirrors
 * `DaemonMessage::is_reply` in `crates/mesh-daemon/src/ipc/message.rs`.
 */
export const isReply = (message: DaemonMessage): boolean => message.t !== 'event' && message.t !== 'chunk';

/** Why a line could not be read as a message. */
export class WireError extends Error {
  /** A stable machine code, matching the daemon's own `WireError::code`. */
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = 'WireError';
    this.code = code;
  }
}

/**
 * Reject values outside the daemon's JSON subset before JavaScript can silently change them.
 *
 * The Rust half rejects duplicate keys, non-integer number spellings, values outside `u64`, lone
 * surrogates and excessive nesting. JavaScript's parser either does not expose those facts or can
 * round them before a reviver sees them, so this small structural pass runs over the original
 * bytes. The full JSON parse still owns general syntax; this pass owns the narrower wire subset
 * without allocating a second object graph.
 */
const rejectOutsideWireSubset = (line: string): void => {
  let at = 0;

  const whitespace = (): void => {
    while (at < line.length && /\s/u.test(line[at] ?? '')) at += 1;
  };

  const stringValue = (): string => {
    const start = at;
    at += 1;
    while (at < line.length) {
      const byte = line[at];
      if (byte === '"') {
        at += 1;
        const decoded = JSON.parse(line.slice(start, at)) as string;
        for (let index = 0; index < decoded.length; index += 1) {
          const unit = decoded.charCodeAt(index);
          if (unit >= 0xd800 && unit <= 0xdbff) {
            const next = decoded.charCodeAt(index + 1);
            if (!(next >= 0xdc00 && next <= 0xdfff)) {
              throw new WireError('malformed-message', 'a string contains a lone surrogate');
            }
            index += 1;
          } else if (unit >= 0xdc00 && unit <= 0xdfff) {
            throw new WireError('malformed-message', 'a string contains a lone surrogate');
          }
        }
        return decoded;
      }
      if (byte === '\\') at += 1;
      at += 1;
    }
    throw new WireError('malformed-message', 'a string is not terminated');
  };

  const value = (depth: number): void => {
    if (depth > MAX_JSON_DEPTH) {
      throw new WireError('malformed-message', `nesting deeper than ${MAX_JSON_DEPTH}`);
    }
    whitespace();
    if (line[at] === '"') {
      stringValue();
      return;
    }
    if (line[at] === '[') {
      at += 1;
      whitespace();
      while (at < line.length && line[at] !== ']') {
        value(depth + 1);
        whitespace();
        if (line[at] === ',') at += 1;
        whitespace();
      }
      at += 1;
      return;
    }
    if (line[at] === '{') {
      at += 1;
      whitespace();
      const keys = new Set<string>();
      while (at < line.length && line[at] !== '}') {
        const key = stringValue();
        if (keys.has(key)) {
          throw new WireError('malformed-message', `duplicate key \`${key}\``);
        }
        keys.add(key);
        whitespace();
        at += 1; // The full parser has already proved this byte is `:`.
        value(depth + 1);
        whitespace();
        if (line[at] === ',') at += 1;
        whitespace();
      }
      at += 1;
      return;
    }
    const start = at;
    while (at < line.length && !/[,\]}\s]/u.test(line[at] ?? '')) at += 1;
    const token = line.slice(start, at);
    if (token === 'null' || token === 'true' || token === 'false') return;
    if (!/^(?:0|[1-9][0-9]*)$/u.test(token) || !Number.isSafeInteger(Number(token))) {
      throw new WireError('malformed-message', 'numbers must be exactly represented non-negative integers');
    }
  };

  value(0);
};

const isPlainObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const isCount = (value: unknown): value is number =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;

const requireCount = (source: Record<string, unknown>, key: string): number => {
  const value = source[key];
  if (!isCount(value)) throw new WireError('malformed-message', `\`${key}\` must be a non-negative integer`);
  return value;
};

const requireText = (source: Record<string, unknown>, key: string): string => {
  const value = source[key];
  if (typeof value !== 'string') throw new WireError('malformed-message', `\`${key}\` must be a string`);
  return value;
};

const requireBoundedText = (
  source: Record<string, unknown>,
  key: string,
  maximumBytes: number,
): string => {
  const value = requireText(source, key);
  const bytes = Buffer.byteLength(value, 'utf8');
  if (bytes === 0 || bytes > maximumBytes) {
    throw new WireError(
      'malformed-message',
      `\`${key}\` must be between 1 and ${maximumBytes} UTF-8 bytes`,
    );
  }
  return value;
};

const requireFlag = (source: Record<string, unknown>, key: string): boolean => {
  const value = source[key];
  if (typeof value !== 'boolean') throw new WireError('malformed-message', `\`${key}\` must be a boolean`);
  return value;
};

const requireObject = (source: Record<string, unknown>, key: string): WireObject => {
  const value = source[key];
  if (!isPlainObject(value)) throw new WireError('malformed-message', `\`${key}\` must be an object`);
  return value as WireObject;
};

const requireVersions = (source: Record<string, unknown>, key: string): number[] => {
  const value = source[key];
  if (!Array.isArray(value) || value.length === 0) {
    throw new WireError('malformed-message', `\`${key}\` must name at least one version`);
  }
  if (value.length > 32) {
    throw new WireError('malformed-message', `\`${key}\` must name at most 32 versions`);
  }
  return value.map((entry) => {
    if (!isCount(entry)) throw new WireError('malformed-message', `every entry of \`${key}\` must be a version number`);
    return entry;
  });
};

/**
 * Encode one client message as a single line, with no trailing newline.
 *
 * The key order in each branch IS the contract. Reordering a property here is a wire change.
 */
export const encodeClient = (message: ClientMessage): string => {
  if (message.t === 'hello') {
    return JSON.stringify({
      t: 'hello',
      id: message.id,
      protocol: PROTOCOL,
      versions: message.versions,
      session: message.session,
    });
  }
  return JSON.stringify({
    t: 'call',
    id: message.id,
    method: message.method,
    version: message.version,
    params: message.params,
  });
};

/** Encode one daemon message. Used by the test daemon and by the round-trip check. */
export const encodeDaemon = (message: DaemonMessage): string => {
  switch (message.t) {
    case 'welcome':
      return JSON.stringify({
        t: 'welcome',
        id: message.id,
        version: message.version,
        session: message.session,
        resumed: message.resumed,
        surface_version: message.surfaceVersion,
      });
    case 'refused':
      return JSON.stringify({
        t: 'refused',
        id: message.id,
        code: message.code,
        message: message.message,
        supported: message.supported,
      });
    case 'result':
      return JSON.stringify({ t: 'result', id: message.id, value: message.value });
    case 'event':
      return JSON.stringify({
        t: 'event',
        id: message.id,
        sequence: message.sequence,
        kind: message.kind,
        value: message.value,
      });
    case 'chunk':
      return JSON.stringify({
        t: 'chunk',
        id: message.id,
        index: message.index,
        parts: message.parts,
        total_bytes: message.totalBytes,
        hex: message.hex,
      });
    default:
      return JSON.stringify({
        t: 'failed',
        id: message.id,
        code: message.code,
        message: message.message,
      });
  }
};

/** Parse one line into JSON, refusing anything the daemon would also refuse. */
const parseLine = (line: string, bound = MAX_LINE_BYTES): Record<string, unknown> => {
  if (Buffer.byteLength(line, 'utf8') >= bound) {
    throw new WireError('line-too-long', `the line is over the ${bound} byte limit`);
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(line);
  } catch (error) {
    throw new WireError('not-json', `the line is not JSON: ${(error as Error).message}`);
  }
  rejectOutsideWireSubset(line);
  if (!isPlainObject(parsed)) throw new WireError('malformed-message', 'a message is an object');
  return parsed;
};

/**
 * Decode one line the daemon sent.
 *
 * @throws {WireError} when the line is not one of the daemon shapes.
 */
const decodeDaemonWithBound = (line: string, bound: number): DaemonMessage => {
  const raw = parseLine(line, bound);
  const id = requireCount(raw, 'id');
  let decoded: DaemonMessage;
  switch (raw['t']) {
    case 'welcome':
      decoded = {
        t: 'welcome',
        id,
        version: requireCount(raw, 'version'),
        session: requireBoundedText(raw, 'session', MAX_SESSION_BYTES),
        resumed: requireFlag(raw, 'resumed'),
        surfaceVersion: requireCount(raw, 'surface_version'),
      };
      break;
    case 'refused':
      decoded = {
        t: 'refused',
        id,
        code: requireText(raw, 'code'),
        message: requireText(raw, 'message'),
        supported: requireVersions(raw, 'supported'),
      };
      break;
    case 'result':
      decoded = { t: 'result', id, value: requireObject(raw, 'value') };
      break;
    case 'failed':
      decoded = { t: 'failed', id, code: requireText(raw, 'code'), message: requireText(raw, 'message') };
      break;
    case 'event':
      decoded = {
        t: 'event',
        id,
        sequence: requireCount(raw, 'sequence'),
        kind: requireBoundedText(raw, 'kind', MAX_METHOD_BYTES),
        value: requireObject(raw, 'value'),
      };
      break;
    case 'chunk':
      decoded = {
        t: 'chunk',
        id,
        index: requireCount(raw, 'index'),
        parts: requireCount(raw, 'parts'),
        totalBytes: requireCount(raw, 'total_bytes'),
        hex: requireText(raw, 'hex'),
      };
      break;
    default:
      throw new WireError('unknown-message', `\`${String(raw['t'])}\` is not a message on this surface`);
  }
  if (encodeDaemon(decoded) !== line) {
    throw new WireError('malformed-message', 'the message must use the one published key order and JSON spelling');
  }
  return decoded;
};

export const decodeDaemon = (line: string): DaemonMessage =>
  decodeDaemonWithBound(line, MAX_LINE_BYTES);

type PartialDaemonMessage = {
  readonly id: number;
  readonly parts: number;
  readonly totalBytes: number;
  next: number;
  readonly chunks: Buffer[];
  bytes: number;
};

/** Reassemble ordered surface-v7 frames without relaxing the per-line bound. */
export class DaemonChunkAssembler {
  #active: PartialDaemonMessage | null = null;

  reset(): void {
    this.#active = null;
  }

  push(frame: DaemonMessage): LogicalDaemonMessage | null {
    if (frame.t !== 'chunk') {
      if (this.#active !== null) this.#fail('a non-chunk interrupted an incomplete response');
      return frame;
    }
    if (
      frame.parts <= 0 ||
      frame.totalBytes < MAX_LINE_BYTES ||
      frame.totalBytes > MAX_MESSAGE_BYTES ||
      frame.hex.length === 0 ||
      frame.hex.length % 2 !== 0 ||
      frame.hex.length > CHUNK_DATA_BYTES * 2 ||
      !/^[0-9a-f]+$/u.test(frame.hex)
    ) {
      this.#fail('the declared chunk bounds or encoding are invalid');
    }
    const bytes = Buffer.from(frame.hex, 'hex');
    if (this.#active === null) {
      if (frame.index !== 0) this.#fail('the first chunk index is not zero');
      this.#active = {
        id: frame.id,
        parts: frame.parts,
        totalBytes: frame.totalBytes,
        next: 0,
        chunks: [],
        bytes: 0,
      };
    }
    const active = this.#active;
    if (
      active.id !== frame.id ||
      active.parts !== frame.parts ||
      active.totalBytes !== frame.totalBytes ||
      active.next !== frame.index ||
      frame.index >= frame.parts ||
      active.bytes + bytes.length > active.totalBytes
    ) {
      this.#fail('chunk identity, order, or length changed');
    }
    active.chunks.push(bytes);
    active.bytes += bytes.length;
    active.next += 1;
    if (active.next !== active.parts) return null;
    this.#active = null;
    if (active.bytes !== active.totalBytes) this.#fail('the final byte length does not match total_bytes');
    let line: string;
    try {
      line = new TextDecoder('utf-8', { fatal: true }).decode(Buffer.concat(active.chunks));
    } catch {
      this.#fail('the reconstructed response is not valid UTF-8');
    }
    const complete = decodeDaemonWithBound(line, MAX_MESSAGE_BYTES + 1);
    if (complete.t === 'chunk' || complete.id !== active.id) {
      this.#fail('the reconstructed response identity is invalid');
    }
    return complete;
  }

  #fail(message: string): never {
    this.#active = null;
    throw new WireError('malformed-message', message);
  }
}

/**
 * Decode one line a client sent. The application never needs this; the test daemon and the
 * round-trip check do, and a decoder that only ever runs in one direction is a decoder half of
 * whose contract is untested.
 *
 * @throws {WireError} when the line is not one of the two client shapes.
 */
export const decodeClient = (line: string): ClientMessage => {
  const raw = parseLine(line);
  const id = requireCount(raw, 'id');
  let decoded: ClientMessage;
  switch (raw['t']) {
    case 'hello': {
      const protocol = requireText(raw, 'protocol');
      if (protocol !== PROTOCOL) {
        throw new WireError('malformed-message', `\`protocol\` expected \`${PROTOCOL}\`, got \`${protocol}\``);
      }
      decoded = {
        t: 'hello',
        id,
        versions: requireVersions(raw, 'versions'),
        session: requireBoundedText(raw, 'session', MAX_SESSION_BYTES),
      };
      break;
    }
    case 'call':
      decoded = {
        t: 'call',
        id,
        method: requireBoundedText(raw, 'method', MAX_METHOD_BYTES),
        version: requireCount(raw, 'version'),
        params: requireObject(raw, 'params'),
      };
      break;
    default:
      throw new WireError('unknown-message', `\`${String(raw['t'])}\` is not a message on this surface`);
  }
  if (encodeClient(decoded) !== line) {
    throw new WireError('malformed-message', 'the message must use the one published key order and JSON spelling');
  }
  return decoded;
};

/**
 * Split a stream of bytes into whole lines, keeping whatever is left over.
 *
 * Returned rather than mutated: the caller keeps the remainder and hands it back next time, so a
 * message split across two socket reads is reassembled instead of being lost.
 */
export const splitLines = (buffered: string): { readonly lines: readonly string[]; readonly rest: string } => {
  const parts = buffered.split('\n');
  const rest = parts.pop() ?? '';
  if (Buffer.byteLength(rest, 'utf8') >= MAX_LINE_BYTES) {
    throw new WireError(
      'line-too-long',
      `the unfinished line is over the ${MAX_LINE_BYTES} byte limit`,
    );
  }
  return { lines: parts.map((line) => line.replace(/\r$/, '')).filter((line) => line.length > 0), rest };
};
