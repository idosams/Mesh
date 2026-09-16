//! Turning a byte stream into named chunks and a manifest, in bounded memory.
//!
//! # Why streaming is the primary interface
//!
//! The workload this crate exists for is plan §12.2's W5: a gibibyte-scale file with a kibibyte of
//! change. An API that takes `&[u8]` requires the caller to hold a gibibyte, which is exactly the
//! cost chunking is supposed to remove. [`ChunkStream`] holds
//! [`ChunkingConfig::peak_buffered_bytes`] plus whatever the caller pushes — for the measured
//! default that is 64 KiB, independent of file size — and hands each chunk to the caller as soon as
//! it is final.
//!
//! [`crate::chunk_bytes`] is the convenience wrapper over it, for callers that genuinely hold the
//! bytes already.
//!
//! # The whole-file policy, and why it is decided late
//!
//! Plan §6.2's first two bullets are a size test, and a stream does not know its size. So the
//! stream buffers until it has seen `whole_file_threshold + 1` bytes and only then commits to
//! content-defined chunking — at which point it chunks from byte zero, not from where it happened
//! to notice. A file that ends at or below the threshold is emitted as exactly one chunk whose
//! digest is the file's own content hash, which is what makes a small file cost one store entry.
//!
//! This is the reason the threshold is bounded by memory: a caller that sets it to a gibibyte has
//! asked to buffer a gibibyte, and [`ChunkingConfig::peak_buffered_bytes`] says so.

use crate::cdc::next_cut;
use crate::config::ChunkingConfig;
use crate::digest::{Blake3, ContentDigest, Digest32, DigestHasher};
use crate::manifest::{ChunkRef, FileManifest};

/// Which of plan §6.2's two policies produced a manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkPolicy {
    /// The file was at or below the threshold and is one object.
    WholeFile,
    /// The file was above the threshold and was cut by content.
    ContentDefined,
}

/// The chunker's own working buffer.
///
/// A `Vec` and a read cursor rather than a `Vec` alone, for a reason that shows up as a factor of
/// seven in wall-clock time. Emitting a chunk with `Vec::drain(..n)` moves every remaining byte
/// down, so a caller that pushes a large buffer pays one memory move per chunk over the whole
/// buffer — quadratic in the push size. Advancing a cursor costs nothing, and the prefix is
/// reclaimed once, when it has grown past a chunk's worth. `buffer_stays_within_the_declared_bound`
/// is what holds that reclamation honest.
#[derive(Debug, Default)]
struct Pending {
    bytes: Vec<u8>,
    read: usize,
}

impl Pending {
    /// The bytes not yet emitted.
    fn unread(&self) -> &[u8] {
        &self.bytes[self.read..]
    }

    /// How many bytes are waiting.
    fn len(&self) -> usize {
        self.bytes.len() - self.read
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn extend(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    /// Mark `length` bytes consumed, reclaiming the read prefix once it is worth reclaiming.
    fn consume(&mut self, length: usize, reclaim_at: usize) {
        self.read += length;
        if self.read >= reclaim_at.max(1) {
            self.bytes.drain(..self.read);
            self.read = 0;
        }
    }
}

/// An incremental chunker: push bytes, receive final chunks, finish with a manifest.
///
/// ```
/// use mesh_chunking::{ChunkStream, ChunkingConfig};
///
/// let config = ChunkingConfig::default();
/// let mut stream = ChunkStream::new(config);
/// let mut chunk_count = 0usize;
/// let mut stored = 0u64;
///
/// for _ in 0..64 {
///     stream.push(&[7u8; 4096], |chunk_ref, bytes| {
///         assert_eq!(bytes.len() as u64, chunk_ref.length());
///         chunk_count += 1;
///         stored += chunk_ref.length();
///     });
/// }
/// let manifest = stream.finish(|chunk_ref, _bytes| {
///     chunk_count += 1;
///     stored += chunk_ref.length();
/// });
///
/// assert_eq!(manifest.byte_length(), 64 * 4096);
/// assert_eq!(stored, manifest.byte_length());
/// assert!(manifest.chunks_are_contiguous());
/// assert_eq!(manifest.chunks().len(), chunk_count);
/// ```
#[derive(Debug)]
pub struct ChunkStream {
    config: ChunkingConfig,
    /// Bytes seen but not yet emitted. Its first unread byte sits at `pending_offset` in the file.
    pending: Pending,
    pending_offset: u64,
    total_length: u64,
    hasher: <Blake3 as ContentDigest>::Hasher,
    refs: Vec<ChunkRef>,
    /// `false` until the stream has proved the file is above the whole-file threshold.
    chunking: bool,
}

impl ChunkStream {
    /// A stream that will apply `config`.
    #[must_use]
    pub fn new(config: ChunkingConfig) -> Self {
        Self {
            config,
            pending: Pending::default(),
            pending_offset: 0,
            total_length: 0,
            hasher: Blake3::hasher(),
            refs: Vec::new(),
            chunking: false,
        }
    }

    /// Absorb `bytes`, handing `emit` every chunk that became final as a result.
    ///
    /// `emit` receives the reference and the chunk's bytes. The bytes are borrowed from the
    /// stream's own buffer and are not retained after the call, so a caller that needs them must
    /// copy or write them through — which is what [`crate::chunk_bytes`] and a `mesh-cas`
    /// promotion both do.
    pub fn push<F>(&mut self, bytes: &[u8], mut emit: F)
    where
        F: FnMut(ChunkRef, &[u8]),
    {
        if bytes.is_empty() {
            return;
        }
        self.hasher.update(bytes);
        self.total_length = self.total_length.saturating_add(bytes.len() as u64);
        self.pending.extend(bytes);

        if !self.chunking {
            if self.total_length <= self.config.whole_file_threshold() {
                return;
            }
            self.chunking = true;
        }

        // A cut is final only when every byte it could have examined is already in `pending`. That
        // is guaranteed exactly when `pending` is at least `max_size` long, because `next_cut`
        // never looks past the maximum.
        while self.pending.len() >= self.config.max_size() {
            let cut = next_cut(self.pending.unread(), &self.config);
            debug_assert!(cut > 0 && cut <= self.pending.len());
            self.emit_prefix(cut, &mut emit);
        }
    }

    /// Finish the file: flush what is left and return the manifest.
    #[must_use]
    pub fn finish<F>(mut self, mut emit: F) -> FileManifest
    where
        F: FnMut(ChunkRef, &[u8]),
    {
        if self.chunking {
            while !self.pending.is_empty() {
                let cut = next_cut(self.pending.unread(), &self.config);
                debug_assert!(cut > 0 && cut <= self.pending.len());
                self.emit_prefix(cut, &mut emit);
            }
        } else if !self.pending.is_empty() {
            // The whole-file policy: one chunk, the whole file, named by the file's own digest.
            let length = self.pending.len();
            self.emit_prefix(length, &mut emit);
        }

        let content_hash = self.hasher.finalize();
        FileManifest::new(self.total_length, content_hash, self.refs)
    }

    /// Which policy this stream has committed to.
    ///
    /// Before [`ChunkStream::finish`] this is provisional: a stream that has not yet passed the
    /// threshold reports [`ChunkPolicy::WholeFile`] and will change its mind if more bytes arrive.
    #[must_use]
    pub const fn policy(&self) -> ChunkPolicy {
        if self.chunking {
            ChunkPolicy::ContentDefined
        } else {
            ChunkPolicy::WholeFile
        }
    }

    /// How many bytes the stream is currently holding, *including* the already-emitted prefix it
    /// has not yet reclaimed.
    ///
    /// The number a caller budgeting memory needs, which is why it is the larger of the two
    /// available readings. It is bounded by
    /// `config.peak_buffered_bytes() + config.max_size() + the largest single push`: the first term
    /// is what the policy requires, the second is the reclamation slack the read cursor buys, and
    /// the third is the caller's own. `the_buffer_stays_within_the_declared_bound` asserts exactly
    /// that sum rather than trusting this sentence.
    #[must_use]
    pub fn buffered_bytes(&self) -> usize {
        self.pending.bytes.len()
    }

    /// How many bytes are still waiting to be cut.
    #[must_use]
    pub fn unemitted_bytes(&self) -> usize {
        self.pending.len()
    }

    /// Emit `length` bytes from the front of `pending` as one chunk.
    fn emit_prefix<F>(&mut self, length: usize, emit: &mut F)
    where
        F: FnMut(ChunkRef, &[u8]),
    {
        let digest = Blake3::digest_bytes(&self.pending.unread()[..length]);
        let chunk_ref = ChunkRef::new(digest, self.pending_offset, length as u64);
        self.refs.push(chunk_ref);
        emit(chunk_ref, &self.pending.unread()[..length]);
        self.pending.consume(length, self.config.max_size());
        self.pending_offset = self.pending_offset.saturating_add(length as u64);
    }
}

/// One chunk, owned: its name, its position and its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    reference: ChunkRef,
    bytes: Vec<u8>,
}

impl Chunk {
    /// The chunk's manifest entry.
    #[must_use]
    pub const fn reference(&self) -> &ChunkRef {
        &self.reference
    }

    /// The chunk's name — the digest `mesh-cas` promotes it under.
    #[must_use]
    pub const fn digest(&self) -> &Digest32 {
        self.reference.content_hash()
    }

    /// The chunk's bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consume the chunk for its bytes, which is what a promotion wants.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// A chunked file: which policy applied, the manifest, and every chunk in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkedFile {
    policy: ChunkPolicy,
    manifest: FileManifest,
    chunks: Vec<Chunk>,
}

impl ChunkedFile {
    /// Which of plan §6.2's two policies applied.
    #[must_use]
    pub const fn policy(&self) -> ChunkPolicy {
        self.policy
    }

    /// The manifest.
    #[must_use]
    pub const fn manifest(&self) -> &FileManifest {
        &self.manifest
    }

    /// The chunks, in reconstruction order. A chunk repeated in the file appears once per
    /// occurrence here; [`FileManifest::distinct_chunks`] is the deduplicated view.
    #[must_use]
    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    /// Consume the file for its manifest and chunks.
    #[must_use]
    pub fn into_parts(self) -> (FileManifest, Vec<Chunk>) {
        (self.manifest, self.chunks)
    }
}

/// Chunk a byte string that is already in memory.
///
/// A thin wrapper over [`ChunkStream`] that keeps every chunk. For anything gibibyte-scale, drive
/// the stream directly — this function's peak memory is the input plus the output, which is twice
/// the file.
///
/// ```
/// use mesh_chunking::{chunk_bytes, ChunkPolicy, ChunkingConfig};
///
/// let config = ChunkingConfig::default();
/// let small = chunk_bytes(b"a short file", &config);
/// assert_eq!(small.policy(), ChunkPolicy::WholeFile);
/// assert_eq!(small.manifest().chunks().len(), 1);
/// // The single chunk's name is the file's own content hash.
/// assert_eq!(
///     small.chunks()[0].digest(),
///     small.manifest().content_hash(),
/// );
/// ```
#[must_use]
pub fn chunk_bytes(data: &[u8], config: &ChunkingConfig) -> ChunkedFile {
    let mut stream = ChunkStream::new(*config);
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut collect = |reference: ChunkRef, bytes: &[u8]| {
        chunks.push(Chunk {
            reference,
            bytes: bytes.to_vec(),
        });
    };

    // Pushed in slices rather than in one call so that the streaming path — the one W5 uses — is
    // the path every test in this crate exercises, including the ones that look like one-shot
    // tests.
    let step = config.max_size().max(1);
    let mut offset = 0;
    while offset < data.len() {
        let end = (offset + step).min(data.len());
        stream.push(&data[offset..end], &mut collect);
        offset = end;
    }
    let policy = stream.policy();
    let manifest = stream.finish(&mut collect);
    let policy = if manifest.chunks().len() > 1 {
        ChunkPolicy::ContentDefined
    } else {
        policy
    };

    ChunkedFile {
        policy,
        manifest,
        chunks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::pseudorandom_bytes;

    fn config() -> ChunkingConfig {
        ChunkingConfig::always_chunked(64, 256, 2048).expect("valid")
    }

    #[test]
    fn an_empty_file_produces_no_chunks() {
        let chunked = chunk_bytes(&[], &ChunkingConfig::default());
        assert!(chunked.chunks().is_empty());
        assert_eq!(chunked.manifest().byte_length(), 0);
        assert!(chunked.manifest().chunks_are_contiguous());
    }

    #[test]
    fn the_chunks_reconstruct_the_file() {
        let data = pseudorandom_bytes(200_000, 11);
        let chunked = chunk_bytes(&data, &config());
        let rebuilt: Vec<u8> = chunked
            .chunks()
            .iter()
            .flat_map(|chunk| chunk.bytes().to_vec())
            .collect();
        assert_eq!(rebuilt, data);
        assert!(chunked.manifest().chunks_are_contiguous());
    }

    #[test]
    fn the_content_hash_is_the_hash_of_the_whole_file() {
        let data = pseudorandom_bytes(200_000, 12);
        let chunked = chunk_bytes(&data, &config());
        assert_eq!(
            chunked.manifest().content_hash(),
            &Blake3::digest_bytes(&data)
        );
    }

    #[test]
    fn each_chunk_is_named_by_its_own_bytes() {
        let data = pseudorandom_bytes(200_000, 13);
        let chunked = chunk_bytes(&data, &config());
        for chunk in chunked.chunks() {
            assert_eq!(chunk.digest(), &Blake3::digest_bytes(chunk.bytes()));
            assert_eq!(chunk.reference().length(), chunk.bytes().len() as u64);
        }
    }

    #[test]
    fn a_file_at_the_threshold_is_one_object() {
        let config = ChunkingConfig::new(4096, 64, 256, 2048, 2).expect("valid");
        let data = pseudorandom_bytes(4096, 14);
        let chunked = chunk_bytes(&data, &config);
        assert_eq!(chunked.policy(), ChunkPolicy::WholeFile);
        assert_eq!(chunked.chunks().len(), 1);
        assert_eq!(
            chunked.chunks()[0].digest(),
            chunked.manifest().content_hash()
        );
    }

    #[test]
    fn one_byte_over_the_threshold_switches_policy() {
        let config = ChunkingConfig::new(4096, 64, 256, 2048, 2).expect("valid");
        let data = pseudorandom_bytes(4097, 15);
        let chunked = chunk_bytes(&data, &config);
        assert_eq!(chunked.policy(), ChunkPolicy::ContentDefined);
        assert!(
            chunked.chunks().len() > 1,
            "above the threshold the file is cut by content, and this fixture is long enough to cut"
        );
        assert!(chunked.manifest().chunks_are_contiguous());
    }

    #[test]
    fn the_push_size_does_not_change_the_boundaries() {
        let data = pseudorandom_bytes(300_000, 16);
        let reference: Vec<ChunkRef> = chunk_bytes(&data, &config()).manifest().chunks().to_vec();

        for step in [1usize, 3, 97, 1024, 4096, 65536, data.len()] {
            let mut stream = ChunkStream::new(config());
            let mut refs = Vec::new();
            let mut offset = 0;
            while offset < data.len() {
                let end = (offset + step).min(data.len());
                stream.push(&data[offset..end], |chunk_ref, _| refs.push(chunk_ref));
                offset = end;
            }
            let _ = stream.finish(|chunk_ref, _| refs.push(chunk_ref));
            assert_eq!(
                refs, reference,
                "a push size of {step} produced different boundaries"
            );
        }
    }

    #[test]
    fn the_buffer_stays_within_the_declared_bound() {
        let config = config();
        let data = pseudorandom_bytes(1_000_000, 17);
        let mut stream = ChunkStream::new(config);
        let push = 8192usize;
        let bound = config.peak_buffered_bytes() as usize + config.max_size() + push;
        let mut offset = 0;
        while offset < data.len() {
            let end = (offset + push).min(data.len());
            stream.push(&data[offset..end], |_, _| {});
            assert!(
                stream.buffered_bytes() <= bound,
                "the stream buffered {} bytes against a declared bound of {bound}",
                stream.buffered_bytes()
            );
            offset = end;
        }
        let _ = stream.finish(|_, _| {});
    }

    #[test]
    fn a_repeated_block_is_stored_once() {
        // Two identical halves must produce two identical chunk sets, which is deduplication
        // within a single file and the cheapest thing content addressing buys.
        let half = pseudorandom_bytes(100_000, 18);
        let mut whole = half.clone();
        whole.extend_from_slice(&half);
        let chunked = chunk_bytes(&whole, &config());
        let distinct = chunked.manifest().distinct_chunks().len();
        assert!(
            distinct < chunked.manifest().chunks().len(),
            "an exactly repeated block produced no shared chunks at all"
        );
    }
}
