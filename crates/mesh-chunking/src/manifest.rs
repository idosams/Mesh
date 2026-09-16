//! The file manifest: byte length, content hash, and the chunks that reconstruct the bytes.
//!
//! [`ChunkRef`] and [`FileManifest`] mirror `mesh-types`' types of the same names field for field
//! and accessor for accessor, for the reason [`crate::blake3`] records — a dependency edge would
//! rewrite `Cargo.lock`, which this lane may not write. `tests/manifest_mirrors_mesh_types.rs`
//! holds the two shapes together by reading `mesh-types`' source, so a field added there without
//! being added here fails rather than silently producing a manifest the rest of the system cannot
//! read.
//!
//! The canonical-encoding half of `mesh-types`' module — `Absorb`, `CanonicalEncode`, the schema —
//! is deliberately absent. Serializing a manifest is `mesh-types`' job; producing one is this
//! crate's.

use crate::digest::Digest32;

/// A manifest entry naming a chunk by content digest together with its position and length.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkRef {
    content_hash: Digest32,
    offset: u64,
    length: u64,
}

impl ChunkRef {
    /// A chunk reference.
    #[must_use]
    pub const fn new(content_hash: Digest32, offset: u64, length: u64) -> Self {
        Self {
            content_hash,
            offset,
            length,
        }
    }

    /// The chunk's content digest — the name `mesh-cas` stores it under.
    #[must_use]
    pub const fn content_hash(&self) -> &Digest32 {
        &self.content_hash
    }

    /// Where this chunk starts in the reconstructed file.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        self.offset
    }

    /// How many bytes this chunk contributes.
    #[must_use]
    pub const fn length(&self) -> u64 {
        self.length
    }
}

/// The ordered list of chunk references that reconstructs a file version's bytes exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileManifest {
    byte_length: u64,
    content_hash: Digest32,
    chunks: Vec<ChunkRef>,
}

impl FileManifest {
    /// A file manifest over `chunks`.
    ///
    /// The declared `byte_length` and `content_hash` are carried, not recomputed — this
    /// constructor never sees the bytes. Everything this crate produces goes through
    /// [`crate::chunk_bytes`] or [`crate::ChunkStream`], which do hold the bytes and do compute
    /// both.
    #[must_use]
    pub const fn new(byte_length: u64, content_hash: Digest32, chunks: Vec<ChunkRef>) -> Self {
        Self {
            byte_length,
            content_hash,
            chunks,
        }
    }

    /// The reconstructed file's length in bytes.
    #[must_use]
    pub const fn byte_length(&self) -> u64 {
        self.byte_length
    }

    /// The digest of the reconstructed bytes — the whole file, not any chunk.
    #[must_use]
    pub const fn content_hash(&self) -> &Digest32 {
        &self.content_hash
    }

    /// The chunks, in reconstruction order.
    #[must_use]
    pub fn chunks(&self) -> &[ChunkRef] {
        &self.chunks
    }

    /// Whether the chunks tile the file exactly: each starting where the previous one ended, and
    /// the last one ending at `byte_length`.
    ///
    /// An empty chunk list is contiguous only for an empty file, which is what makes a manifest
    /// that lost its chunks detectable rather than merely odd.
    #[must_use]
    pub fn chunks_are_contiguous(&self) -> bool {
        let mut cursor = 0u64;
        for chunk in &self.chunks {
            if chunk.offset() != cursor {
                return false;
            }
            match cursor.checked_add(chunk.length()) {
                Some(next) => cursor = next,
                None => return false,
            }
        }
        cursor == self.byte_length
    }

    /// The distinct chunk digests this manifest names, in first-appearance order.
    ///
    /// A file that repeats a block names the same chunk twice; the store holds one copy, so the
    /// distinct set is what a transfer or a footprint calculation is over, not the chunk list.
    #[must_use]
    pub fn distinct_chunks(&self) -> Vec<Digest32> {
        let mut seen = Vec::with_capacity(self.chunks.len());
        for chunk in &self.chunks {
            if !seen.contains(chunk.content_hash()) {
                seen.push(*chunk.content_hash());
            }
        }
        seen
    }

    /// The bytes a peer holding `already_have` would still need in order to reconstruct this file.
    ///
    /// This is the transfer measure the plan's §12.4 budget row is stated in — "1 KiB edit in 1 GiB
    /// file: <4 MiB transfer" — and it is deliberately *content* bytes only. Manifest bytes,
    /// framing and per-chunk store overhead are the caller's to add; this crate does not know what
    /// they cost. [`crate::ChunkStream`] users measuring a real footprint should add
    /// `mesh-cas`' arrival-journal cost per distinct new chunk.
    #[must_use]
    pub fn transfer_bytes_given(&self, already_have: &[Digest32]) -> u64 {
        let mut total = 0u64;
        let mut counted: Vec<Digest32> = Vec::new();
        for chunk in &self.chunks {
            let digest = chunk.content_hash();
            if already_have.contains(digest) || counted.contains(digest) {
                continue;
            }
            counted.push(*digest);
            total = total.saturating_add(chunk.length());
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(seed: u8) -> Digest32 {
        Digest32::from_bytes([seed; 32])
    }

    #[test]
    fn a_tiling_manifest_is_contiguous() {
        let manifest = FileManifest::new(
            30,
            digest(0),
            vec![
                ChunkRef::new(digest(1), 0, 10),
                ChunkRef::new(digest(2), 10, 20),
            ],
        );
        assert!(manifest.chunks_are_contiguous());
    }

    #[test]
    fn a_gap_is_not_contiguous() {
        let manifest = FileManifest::new(
            30,
            digest(0),
            vec![
                ChunkRef::new(digest(1), 0, 10),
                ChunkRef::new(digest(2), 11, 19),
            ],
        );
        assert!(!manifest.chunks_are_contiguous());
    }

    #[test]
    fn an_empty_file_has_no_chunks_and_is_contiguous() {
        let manifest = FileManifest::new(0, digest(0), Vec::new());
        assert!(manifest.chunks_are_contiguous());
    }

    #[test]
    fn a_manifest_that_lost_its_chunks_is_detected() {
        let manifest = FileManifest::new(30, digest(0), Vec::new());
        assert!(!manifest.chunks_are_contiguous());
    }

    #[test]
    fn a_repeated_chunk_is_transferred_once() {
        let manifest = FileManifest::new(
            30,
            digest(0),
            vec![
                ChunkRef::new(digest(1), 0, 10),
                ChunkRef::new(digest(1), 10, 10),
                ChunkRef::new(digest(2), 20, 10),
            ],
        );
        assert_eq!(manifest.distinct_chunks(), vec![digest(1), digest(2)]);
        assert_eq!(manifest.transfer_bytes_given(&[]), 20);
    }

    #[test]
    fn a_chunk_the_peer_already_has_is_not_transferred() {
        let manifest = FileManifest::new(
            30,
            digest(0),
            vec![
                ChunkRef::new(digest(1), 0, 10),
                ChunkRef::new(digest(2), 10, 20),
            ],
        );
        assert_eq!(manifest.transfer_bytes_given(&[digest(1)]), 20);
        assert_eq!(
            manifest.transfer_bytes_given(&[digest(1), digest(2)]),
            0,
            "a peer holding every chunk transfers nothing"
        );
    }
}
