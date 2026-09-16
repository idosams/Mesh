//! File manifests and chunk references.

use crate::canonical::{CanonicalEncode, CanonicalType, CanonicalValue, FieldSchema, RecordSchema};
use crate::digest::{Absorb, CanonicalRecord, Digest32, DigestHasher, DigestWriter, DomainTag};
use crate::record_id::ManifestId;

/// One chunk reference, as it appears inside a file manifest's `chunks` sequence.
const CHUNK_FIELDS: &[FieldSchema] = &[
    FieldSchema::new("content_hash", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("offset", CanonicalType::Unsigned),
    FieldSchema::new("length", CanonicalType::Unsigned),
];

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

    /// The chunk's content digest.
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
    /// The declared `byte_length` and `content_hash` are carried, not recomputed: this crate never
    /// reads bytes. [`FileManifest::chunks_are_contiguous`] is the consistency check a caller that
    /// *does* hold the bytes should run before trusting the manifest.
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

    /// The digest of the reconstructed bytes.
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
}

impl CanonicalRecord for FileManifest {
    type Id = ManifestId;

    const DOMAIN: DomainTag = DomainTag::new("mesh.v0.file-manifest");
}

impl Absorb for FileManifest {
    fn absorb<H: DigestHasher>(&self, writer: &mut DigestWriter<H>) {
        writer.u64(self.byte_length);
        writer.digest(&self.content_hash);
        writer.sequence(&self.chunks, |writer, chunk| {
            writer.digest(chunk.content_hash());
            writer.u64(chunk.offset());
            writer.u64(chunk.length());
        });
    }
}

impl CanonicalEncode for FileManifest {
    const SCHEMA: RecordSchema = RecordSchema::new(
        <Self as CanonicalRecord>::DOMAIN,
        &[
            FieldSchema::new("byte_length", CanonicalType::Unsigned),
            FieldSchema::new("content_hash", CanonicalType::Bytes(Some(32))),
            FieldSchema::new(
                "chunks",
                CanonicalType::Sequence(&CanonicalType::Group(CHUNK_FIELDS)),
            ),
        ],
    );

    fn canonical_fields(&self) -> Vec<CanonicalValue> {
        vec![
            CanonicalValue::Unsigned(self.byte_length),
            CanonicalValue::from_digest(&self.content_hash),
            CanonicalValue::Sequence(
                self.chunks
                    .iter()
                    .map(|chunk| {
                        CanonicalValue::Group(vec![
                            CanonicalValue::from_digest(chunk.content_hash()),
                            CanonicalValue::Unsigned(chunk.offset()),
                            CanonicalValue::Unsigned(chunk.length()),
                        ])
                    })
                    .collect(),
            ),
        ]
    }
}
