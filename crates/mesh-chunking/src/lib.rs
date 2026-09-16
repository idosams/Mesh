//! Whole-file and content-defined chunking, with BLAKE3 chunk names and a Zstandard container.
//!
//! Plan §6.2's chunk policy, implemented so that a small edit to a large file costs a small
//! transfer. The measurable claim this crate exists to make, and the one it is judged by:
//!
//! > **A one-kibibyte edit inside a one-gibibyte file transfers under the plan's §12.4 budget of
//! > 4 MiB, against the target of 1 MiB.**
//!
//! That is measured, not asserted: `tests/transfer_budget.rs` builds a gibibyte through the
//! streaming chunker, edits a kilobyte in the middle of it, and fails if the changed-chunk bytes
//! exceed the budget. `cargo bench -p mesh-chunking --bench chunking` publishes the same number for
//! several parameter sets, which is what makes [`ChunkingConfig::default`] a measurement rather
//! than a preference.
//!
//! # The three things this crate is
//!
//! * [`ChunkStream`] — the streaming chunker. Bounded memory, one manifest out, every chunk handed
//!   to the caller as soon as it is final. This is the interface a gibibyte goes through.
//! * [`FileManifest`] — plan §6.2's manifest: byte length, whole-file content hash, and the chunk
//!   references that reconstruct the bytes. It mirrors `mesh-types`' type of the same name.
//! * [`ChunkingConfig`] — the parameters, checked on construction, with the plan's own numbers
//!   available as [`ChunkingConfig::plan_defaults`] and the measured ones as
//!   [`ChunkingConfig::default`].
//!
//! ```
//! use mesh_chunking::{ChunkStream, ChunkingConfig};
//!
//! let config = ChunkingConfig::default();
//! let original: Vec<u8> =
//!     (0..400_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
//!
//! // Version one.
//! let mut stream = ChunkStream::new(config);
//! stream.push(&original, |_, _| {});
//! let before = stream.finish(|_, _| {});
//!
//! // A one-byte edit in the middle.
//! let mut edited = original.clone();
//! edited[200_000] ^= 0xFF;
//!
//! let mut stream = ChunkStream::new(config);
//! stream.push(&edited, |_, _| {});
//! let after = stream.finish(|_, _| {});
//!
//! // Only the chunks around the edit have to move.
//! let held = before.distinct_chunks();
//! let transfer = after.transfer_bytes_given(&held);
//! assert!(transfer < original.len() as u64 / 4, "transferred {transfer} bytes");
//! ```
//!
//! # What it deliberately is not
//!
//! It does not touch a filesystem, a database or a socket — putting a chunk somewhere is
//! `mesh-cas`' job, and this crate is `core-services` in plan §8.3's ladder precisely so that a
//! boundary decision can never come to depend on where the bytes were going. It does not serialize
//! a manifest; `mesh-types` owns the canonical encoding. And it does not implement a real
//! Zstandard encoder — [`Compression`]'s module documentation says exactly what it does instead,
//! and says it first.
//!
//! # Dependencies
//!
//! None. [`blake3`] carries a copy of `mesh-types`' hasher rather than importing it, for the reason
//! that module records: a dependency edge rewrites `Cargo.lock`, which is governance surface this
//! lane may not write. Two tests hold the copy in place.

mod blake3;
mod cdc;
mod compress;
mod config;
mod digest;
mod gear;
mod manifest;
mod stream;
pub mod testing;

pub use crate::blake3::Blake3Hasher;
pub use crate::compress::{Compression, CompressionError};
pub use crate::config::{ChunkingConfig, ConfigError};
pub use crate::digest::{Blake3, ContentDigest, Digest32, DigestHasher, DigestParseError};
pub use crate::gear::GEAR_SEED;
pub use crate::manifest::{ChunkRef, FileManifest};
pub use crate::stream::{chunk_bytes, Chunk, ChunkPolicy, ChunkStream, ChunkedFile};

/// The crate's name, so a caller can attribute a manifest to its producer without a version.
pub const CRATE_NAME: &str = "mesh-chunking";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "mesh-chunking");
    }
}
