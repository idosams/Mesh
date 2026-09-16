// protocol/conformance/clients/published/blake3.mjs — BLAKE3, written from the published
// specification, for the reference conformance client (task 01KZC2P6SQ7E00ENWQ6F4YVXJJ).
//
// The published vectors carry `canonical_encoding_digest_hex`, and
// `protocol/test-vectors/README.md` step 4 says to check it "with any BLAKE3 library". The
// conformance clients carry no dependencies, so this is that library: the reference tree hash,
// 32-byte output, no keying and no derive-key mode, because the protocol uses neither.
//
// It is deliberately NOT a port of `crates/mesh-types/src/blake3.rs`. A digest checked against a
// translation of the implementation it is checking proves nothing; this is written from the BLAKE3
// specification and is held against the published digests by case family `ENC-*-digest`, and
// against the one digest anybody can look up — BLAKE3 of the empty input, which
// `protocol/test-vectors/README.md` prints as `af1349b9f5f9…` — by `--self-test`.

const IV = Uint32Array.from([
  0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
]);

const MSG_PERMUTATION = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8];

const CHUNK_START = 1;
const CHUNK_END = 2;
const PARENT = 4;
const ROOT = 8;

const BLOCK_LEN = 64;
const CHUNK_LEN = 1024;

const rotr = (word, bits) => ((word >>> bits) | (word << (32 - bits))) >>> 0;

function mix(state, a, b, c, d, mx, my) {
  state[a] = (state[a] + state[b] + mx) >>> 0;
  state[d] = rotr(state[d] ^ state[a], 16);
  state[c] = (state[c] + state[d]) >>> 0;
  state[b] = rotr(state[b] ^ state[c], 12);
  state[a] = (state[a] + state[b] + my) >>> 0;
  state[d] = rotr(state[d] ^ state[a], 8);
  state[c] = (state[c] + state[d]) >>> 0;
  state[b] = rotr(state[b] ^ state[c], 7);
}

function round(state, message) {
  mix(state, 0, 4, 8, 12, message[0], message[1]);
  mix(state, 1, 5, 9, 13, message[2], message[3]);
  mix(state, 2, 6, 10, 14, message[4], message[5]);
  mix(state, 3, 7, 11, 15, message[6], message[7]);
  mix(state, 0, 5, 10, 15, message[8], message[9]);
  mix(state, 1, 6, 11, 12, message[10], message[11]);
  mix(state, 2, 7, 8, 13, message[12], message[13]);
  mix(state, 3, 4, 9, 14, message[14], message[15]);
}

/** The compression function. Returns all sixteen output words; the first eight are the CV. */
function compress(chaining, block, counter, blockLength, flags) {
  const state = new Uint32Array(16);
  state.set(chaining, 0);
  state.set(IV.subarray(0, 4), 8);
  state[12] = Number(BigInt(counter) & 0xffffffffn) >>> 0;
  state[13] = Number((BigInt(counter) >> 32n) & 0xffffffffn) >>> 0;
  state[14] = blockLength;
  state[15] = flags;

  let message = Uint32Array.from(block);
  for (let index = 0; index < 7; index += 1) {
    round(state, message);
    if (index === 6) break;
    const permuted = new Uint32Array(16);
    for (let word = 0; word < 16; word += 1) permuted[word] = message[MSG_PERMUTATION[word]];
    message = permuted;
  }

  const out = new Uint32Array(16);
  for (let word = 0; word < 8; word += 1) {
    out[word] = (state[word] ^ state[word + 8]) >>> 0;
    out[word + 8] = (state[word + 8] ^ chaining[word]) >>> 0;
  }
  return out;
}

/** Sixty-four bytes as sixteen little-endian words, zero-padded when the block is short. */
function blockWords(bytes, start, length) {
  const words = new Uint32Array(16);
  for (let index = 0; index < length; index += 1) {
    words[index >> 2] |= bytes[start + index] << ((index & 3) * 8);
    words[index >> 2] >>>= 0;
  }
  return words;
}

/** The chaining value of one chunk, or the root output words when `isRoot`. */
function hashChunk(bytes, start, length, counter, isRoot) {
  let chaining = Uint32Array.from(IV);
  const blocks = Math.max(1, Math.ceil(length / BLOCK_LEN));
  for (let index = 0; index < blocks; index += 1) {
    const offset = index * BLOCK_LEN;
    const blockLength = Math.min(BLOCK_LEN, length - offset);
    let flags = 0;
    if (index === 0) flags |= CHUNK_START;
    if (index === blocks - 1) {
      flags |= CHUNK_END;
      if (isRoot) flags |= ROOT;
    }
    const out = compress(
      chaining,
      blockWords(bytes, start + offset, Math.max(0, blockLength)),
      counter,
      Math.max(0, blockLength),
      flags,
    );
    chaining = out.slice(0, 8);
  }
  return chaining;
}

/** The chaining value of the subtree covering `[start, start + length)`. */
function hashSubtree(bytes, start, length, counter, isRoot) {
  if (length <= CHUNK_LEN) return hashChunk(bytes, start, length, counter, isRoot);

  let leftLength = CHUNK_LEN;
  while (leftLength * 2 < length) leftLength *= 2;

  const left = hashSubtree(bytes, start, leftLength, counter, false);
  const right = hashSubtree(
    bytes,
    start + leftLength,
    length - leftLength,
    counter + leftLength / CHUNK_LEN,
    false,
  );

  const block = new Uint32Array(16);
  block.set(left, 0);
  block.set(right, 8);
  return compress(
    Uint32Array.from(IV),
    block,
    0,
    BLOCK_LEN,
    PARENT | (isRoot ? ROOT : 0),
  ).slice(0, 8);
}

/** BLAKE3 of `bytes` (an array or Uint8Array of octets), as lowercase hex, thirty-two bytes. */
export function blake3Hex(bytes) {
  const input = bytes instanceof Uint8Array ? bytes : Uint8Array.from(bytes);
  const words = hashSubtree(input, 0, input.length, 0, true);
  let out = "";
  for (const word of words) {
    for (let shift = 0; shift < 32; shift += 8) {
      out += ((word >>> shift) & 0xff).toString(16).padStart(2, "0");
    }
  }
  return out;
}
