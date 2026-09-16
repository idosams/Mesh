//! BLAKE3, in the hash mode, with no dependency on anything.
//!
//! This is the reference tree hasher: the 7-round compression function, 1024-byte chunks, the
//! chaining-value stack that combines chunks into parent nodes, and a 32-byte root output. Keyed
//! hashing and key derivation are deliberately absent — nothing in this crate needs them, and an
//! unused mode is an untested mode.
//!
//! # This is a second copy of `mesh-types/src/blake3.rs`, and that is deliberate
//!
//! A chunk's name has to be the digest `mesh-types` computes, or a manifest written by one crate
//! names nothing in the store written by the other. The obvious way to guarantee that is a path
//! dependency on `mesh-types`. This crate does not have one, for the reason recorded in
//! `docs/adr/0008-ship-the-sqlite-schema-without-a-sqlite-driver.md`: any dependency edge rewrites
//! `Cargo.lock`, which is governance surface this lane may not write. So the code below is copied
//! verbatim, exactly as `mesh-store` mirrors `mesh-types`' identifier families rather than
//! importing them.
//!
//! **A copy nothing checks is a copy that drifts**, so two tests hold it in place:
//!
//! * `tests/blake3_agrees_with_mesh_types.rs` strips both files of comments and blank lines and
//!   compares the remaining code byte for byte, so an edit to either side turns this red;
//! * the same file re-runs the published reference vectors against *this* copy, so the digest is
//!   measured here and not inherited from `mesh-types`' passing suite.
//!
//! Neither test can survive `mesh-types/src/blake3.rs` being moved or renamed — it is read by
//! path — so both fail loudly rather than silently passing when the path stops resolving.
//!
//! Keyed hashing and key derivation are absent from the original and stay absent here; an unused
//! mode is an untested mode.

/// The BLAKE3 initialization vector — the SHA-256 IV.
const IV: [u32; 8] = [
    0x6A09_E667,
    0xBB67_AE85,
    0x3C6E_F372,
    0xA54F_F53A,
    0x510E_527F,
    0x9B05_688C,
    0x1F83_D9AB,
    0x5BE0_CD19,
];

/// The message word permutation applied between rounds.
const MSG_PERMUTATION: [usize; 16] = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8];

const BLOCK_LEN: usize = 64;
const CHUNK_LEN: usize = 1024;

const CHUNK_START: u32 = 1 << 0;
const CHUNK_END: u32 = 1 << 1;
const PARENT: u32 = 1 << 2;
const ROOT: u32 = 1 << 3;

/// The chaining-value stack depth: one entry per bit of the chunk counter.
const MAX_DEPTH: usize = 54;

/// The quarter-round mixing function.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn g(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, mx: u32, my: u32) {
    state[a] = state[a].wrapping_add(state[b]).wrapping_add(mx);
    state[d] = (state[d] ^ state[a]).rotate_right(16);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] = (state[b] ^ state[c]).rotate_right(12);
    state[a] = state[a].wrapping_add(state[b]).wrapping_add(my);
    state[d] = (state[d] ^ state[a]).rotate_right(8);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] = (state[b] ^ state[c]).rotate_right(7);
}

/// One round: four column mixes, then four diagonal mixes.
fn round(state: &mut [u32; 16], m: &[u32; 16]) {
    g(state, 0, 4, 8, 12, m[0], m[1]);
    g(state, 1, 5, 9, 13, m[2], m[3]);
    g(state, 2, 6, 10, 14, m[4], m[5]);
    g(state, 3, 7, 11, 15, m[6], m[7]);
    g(state, 0, 5, 10, 15, m[8], m[9]);
    g(state, 1, 6, 11, 12, m[10], m[11]);
    g(state, 2, 7, 8, 13, m[12], m[13]);
    g(state, 3, 4, 9, 14, m[14], m[15]);
}

fn permute(m: &mut [u32; 16]) {
    let original = *m;
    for (slot, &source) in m.iter_mut().zip(MSG_PERMUTATION.iter()) {
        *slot = original[source];
    }
}

/// The compression function: seven rounds over the extended state, then the feed-forward.
fn compress(
    chaining_value: &[u32; 8],
    block_words: &[u32; 16],
    counter: u64,
    block_len: u32,
    flags: u32,
) -> [u32; 16] {
    let mut state: [u32; 16] = [
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
        counter as u32,
        (counter >> 32) as u32,
        block_len,
        flags,
    ];
    let mut block = *block_words;

    for _ in 0..6 {
        round(&mut state, &block);
        permute(&mut block);
    }
    round(&mut state, &block);

    for i in 0..8 {
        state[i] ^= state[i + 8];
        state[i + 8] ^= chaining_value[i];
    }
    state
}

fn first_eight(words: &[u32; 16]) -> [u32; 8] {
    let mut out = [0u32; 8];
    out.copy_from_slice(&words[..8]);
    out
}

fn words_from_block(block: &[u8; BLOCK_LEN]) -> [u32; 16] {
    let mut words = [0u32; 16];
    for (word, bytes) in words.iter_mut().zip(block.chunks_exact(4)) {
        *word = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    }
    words
}

/// A node whose compression has not yet been performed, so the caller can choose whether it is a
/// chaining value or the root.
#[derive(Clone, Copy)]
struct Output {
    input_chaining_value: [u32; 8],
    block_words: [u32; 16],
    counter: u64,
    block_len: u32,
    flags: u32,
}

impl Output {
    fn chaining_value(&self) -> [u32; 8] {
        first_eight(&compress(
            &self.input_chaining_value,
            &self.block_words,
            self.counter,
            self.block_len,
            self.flags,
        ))
    }

    /// The 32-byte root hash. The extendable output is not implemented; 32 bytes is the whole
    /// requirement and a longer output nothing produces cannot be wrong.
    fn root_hash(&self) -> [u8; 32] {
        let words = compress(
            &self.input_chaining_value,
            &self.block_words,
            0,
            self.block_len,
            self.flags | ROOT,
        );
        let mut out = [0u8; 32];
        for (slot, word) in out.chunks_exact_mut(4).zip(words.iter().take(8)) {
            slot.copy_from_slice(&word.to_le_bytes());
        }
        out
    }
}

/// One 1024-byte chunk being absorbed.
#[derive(Clone, Copy)]
struct ChunkState {
    chaining_value: [u32; 8],
    chunk_counter: u64,
    block: [u8; BLOCK_LEN],
    block_len: u8,
    blocks_compressed: u8,
}

impl ChunkState {
    fn new(key: [u32; 8], chunk_counter: u64) -> Self {
        Self {
            chaining_value: key,
            chunk_counter,
            block: [0; BLOCK_LEN],
            block_len: 0,
            blocks_compressed: 0,
        }
    }

    fn len(&self) -> usize {
        BLOCK_LEN * usize::from(self.blocks_compressed) + usize::from(self.block_len)
    }

    fn start_flag(&self) -> u32 {
        if self.blocks_compressed == 0 {
            CHUNK_START
        } else {
            0
        }
    }

    fn update(&mut self, mut input: &[u8]) {
        while !input.is_empty() {
            if usize::from(self.block_len) == BLOCK_LEN {
                let block_words = words_from_block(&self.block);
                self.chaining_value = first_eight(&compress(
                    &self.chaining_value,
                    &block_words,
                    self.chunk_counter,
                    BLOCK_LEN as u32,
                    self.start_flag(),
                ));
                self.blocks_compressed += 1;
                self.block = [0; BLOCK_LEN];
                self.block_len = 0;
            }

            let want = BLOCK_LEN - usize::from(self.block_len);
            let take = want.min(input.len());
            let at = usize::from(self.block_len);
            self.block[at..at + take].copy_from_slice(&input[..take]);
            self.block_len += take as u8;
            input = &input[take..];
        }
    }

    fn output(&self) -> Output {
        Output {
            input_chaining_value: self.chaining_value,
            block_words: words_from_block(&self.block),
            counter: self.chunk_counter,
            block_len: u32::from(self.block_len),
            flags: self.start_flag() | CHUNK_END,
        }
    }
}

fn parent_output(left: [u32; 8], right: [u32; 8], key: [u32; 8]) -> Output {
    let mut block_words = [0u32; 16];
    block_words[..8].copy_from_slice(&left);
    block_words[8..].copy_from_slice(&right);
    Output {
        input_chaining_value: key,
        block_words,
        counter: 0,
        block_len: BLOCK_LEN as u32,
        flags: PARENT,
    }
}

/// Incremental BLAKE3 state.
///
/// Drive it through [`crate::DigestHasher`], which is the only interface it exposes — a caller
/// that wants a digest never names the algorithm twice. The result is a function of the
/// concatenated input alone: chunk boundaries, call sizes and machine make no difference, which is
/// the property record identifiers rest on.
///
/// ```
/// use mesh_cas::{Blake3Hasher, DigestHasher};
///
/// let mut once = Blake3Hasher::new();
/// once.update(b"canonical bytes");
///
/// let mut split = Blake3Hasher::new();
/// split.update(b"canonical ");
/// split.update(b"bytes");
///
/// assert_eq!(once.finalize(), split.finalize());
/// ```
#[derive(Clone)]
pub struct Blake3Hasher {
    chunk_state: ChunkState,
    key: [u32; 8],
    cv_stack: [[u32; 8]; MAX_DEPTH],
    cv_stack_len: usize,
}

impl Blake3Hasher {
    /// A hasher over an empty input.
    #[must_use]
    pub fn new() -> Self {
        Self {
            chunk_state: ChunkState::new(IV, 0),
            key: IV,
            cv_stack: [[0; 8]; MAX_DEPTH],
            cv_stack_len: 0,
        }
    }

    fn push_stack(&mut self, cv: [u32; 8]) {
        self.cv_stack[self.cv_stack_len] = cv;
        self.cv_stack_len += 1;
    }

    fn pop_stack(&mut self) -> [u32; 8] {
        self.cv_stack_len -= 1;
        self.cv_stack[self.cv_stack_len]
    }

    /// Merge the new chunk chaining value with every completed subtree to its left. A subtree is
    /// complete exactly when the low bit of the chunk total is clear, which is why the loop is a
    /// shift rather than a search.
    fn add_chunk_chaining_value(&mut self, mut new_cv: [u32; 8], mut total_chunks: u64) {
        while total_chunks & 1 == 0 {
            let left = self.pop_stack();
            new_cv = parent_output(left, new_cv, self.key).chaining_value();
            total_chunks >>= 1;
        }
        self.push_stack(new_cv);
    }

    /// Absorb more input. Reached from outside the crate through [`crate::DigestHasher`].
    pub(crate) fn update(&mut self, mut input: &[u8]) {
        while !input.is_empty() {
            if self.chunk_state.len() == CHUNK_LEN {
                let chunk_cv = self.chunk_state.output().chaining_value();
                let total_chunks = self.chunk_state.chunk_counter + 1;
                self.add_chunk_chaining_value(chunk_cv, total_chunks);
                self.chunk_state = ChunkState::new(self.key, total_chunks);
            }

            let want = CHUNK_LEN - self.chunk_state.len();
            let take = want.min(input.len());
            self.chunk_state.update(&input[..take]);
            input = &input[take..];
        }
    }

    /// The 32-byte hash of everything absorbed so far. Reached from outside the crate through
    /// [`crate::DigestHasher`].
    #[must_use]
    pub(crate) fn finalize(&self) -> [u8; 32] {
        let mut output = self.chunk_state.output();
        let mut remaining = self.cv_stack_len;
        while remaining > 0 {
            remaining -= 1;
            output = parent_output(self.cv_stack[remaining], output.chaining_value(), self.key);
        }
        output.root_hash()
    }
}

impl Default for Blake3Hasher {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for Blake3Hasher {
    /// Deliberately opaque: printing intermediate hash state in a log is how a preimage leaks.
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("Blake3Hasher(..)")
    }
}

/// The 32-byte BLAKE3 hash of `input`, in one call.
///
/// Crate-private: the public entry point is [`crate::ContentDigest::digest_bytes`], so no caller
/// names the algorithm to hash something.
#[must_use]
pub(crate) fn hash(input: &[u8]) -> [u8; 32] {
    let mut hasher = Blake3Hasher::new();
    hasher.update(input);
    hasher.finalize()
}
