"""BLAKE3, written in pure Python from the BLAKE3 specification (task 01KZC2TBX5BGTQPXXX2DATM3TY).

`protocol/schemas/canonical-encoding-v0.json` names the digest as `BLAKE3`, 32 output bytes, and
does not restate the algorithm -- correctly, because BLAKE3 is somebody else's published
specification. So this file is the one part of the second client that is NOT written from
`protocol/**`: it is written from the BLAKE3 reference specification, which is what an external
implementer would reach for.

What checks it: every `canonical_encoding_digest_hex` in `protocol/test-vectors/v0/**`. All of them
are shorter than one 1024-byte chunk, so the SINGLE-CHUNK path is pinned by published bytes and the
tree path below is not pinned by anything in this repository. That is recorded as a coverage gap in
`protocol/conformance/reports/second-client-r14.md` rather than papered over here.
"""

from struct import unpack

MASK = 0xFFFFFFFF
BLOCK_LEN = 64
CHUNK_LEN = 1024

IV = (
    0x6A09E667,
    0xBB67AE85,
    0x3C6EF372,
    0xA54FF53A,
    0x510E527F,
    0x9B05688C,
    0x1F83D9AB,
    0x5BE0CD19,
)

MSG_PERMUTATION = (2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8)

CHUNK_START = 1
CHUNK_END = 2
PARENT = 4
ROOT = 8


def _rotr(word, bits):
    return ((word >> bits) | (word << (32 - bits))) & MASK


def _mix(state, a, b, c, d, mx, my):
    state[a] = (state[a] + state[b] + mx) & MASK
    state[d] = _rotr(state[d] ^ state[a], 16)
    state[c] = (state[c] + state[d]) & MASK
    state[b] = _rotr(state[b] ^ state[c], 12)
    state[a] = (state[a] + state[b] + my) & MASK
    state[d] = _rotr(state[d] ^ state[a], 8)
    state[c] = (state[c] + state[d]) & MASK
    state[b] = _rotr(state[b] ^ state[c], 7)


def _round(state, message):
    _mix(state, 0, 4, 8, 12, message[0], message[1])
    _mix(state, 1, 5, 9, 13, message[2], message[3])
    _mix(state, 2, 6, 10, 14, message[4], message[5])
    _mix(state, 3, 7, 11, 15, message[6], message[7])
    _mix(state, 0, 5, 10, 15, message[8], message[9])
    _mix(state, 1, 6, 11, 12, message[10], message[11])
    _mix(state, 2, 7, 8, 13, message[12], message[13])
    _mix(state, 3, 4, 9, 14, message[14], message[15])


def _compress(chaining_value, block_words, counter, block_len, flags):
    state = [
        chaining_value[0],
        chaining_value[1],
        chaining_value[2],
        chaining_value[3],
        chaining_value[4],
        chaining_value[5],
        chaining_value[6],
        chaining_value[7],
        IV[0],
        IV[1],
        IV[2],
        IV[3],
        counter & MASK,
        (counter >> 32) & MASK,
        block_len,
        flags,
    ]
    message = list(block_words)
    for index in range(7):
        _round(state, message)
        if index < 6:
            message = [message[position] for position in MSG_PERMUTATION]
    for index in range(8):
        state[index] ^= state[index + 8]
        state[index + 8] ^= chaining_value[index]
    return state


def _words(block):
    return unpack("<16I", block + b"\x00" * (BLOCK_LEN - len(block)))


def _chunk(data, counter, flags):
    """The chaining value of one chunk (at most CHUNK_LEN bytes). `flags` may carry ROOT."""
    blocks = [data[at : at + BLOCK_LEN] for at in range(0, len(data), BLOCK_LEN)] or [b""]
    chaining_value = IV
    for index, block in enumerate(blocks):
        block_flags = flags
        if index == 0:
            block_flags |= CHUNK_START
        if index == len(blocks) - 1:
            block_flags |= CHUNK_END
        else:
            block_flags &= ~ROOT
        chaining_value = _compress(chaining_value, _words(block), counter, len(block), block_flags)[:8]
    return chaining_value


def _left_length(total):
    """The byte length of the left subtree: the largest power-of-two chunk count below `total`."""
    chunks = (total + CHUNK_LEN - 1) // CHUNK_LEN
    power = 1
    while power * 2 < chunks:
        power *= 2
    return power * CHUNK_LEN


def _subtree(data, counter, flags):
    if len(data) <= CHUNK_LEN:
        return _chunk(data, counter, flags)
    split = _left_length(len(data))
    left = _subtree(data[:split], counter, flags)
    right = _subtree(data[split:], counter + split // CHUNK_LEN, flags)
    return _compress(IV, _words(bytes_of(left) + bytes_of(right)), 0, BLOCK_LEN, flags | PARENT)[:8]


def bytes_of(words):
    return b"".join(word.to_bytes(4, "little") for word in words)


def blake3(data):
    """The 32-byte BLAKE3 digest of `data`, unkeyed."""
    if len(data) <= CHUNK_LEN:
        return bytes_of(_chunk(data, 0, ROOT))
    split = _left_length(len(data))
    left = _subtree(data[:split], 0, 0)
    right = _subtree(data[split:], split // CHUNK_LEN, 0)
    root = _compress(IV, _words(bytes_of(left) + bytes_of(right)), 0, BLOCK_LEN, PARENT | ROOT)
    return bytes_of(root[:8])


def blake3_hex(data):
    return blake3(data).hex()
