//! Candidate B's disposable physical paging for flat v0 file manifests.
//!
//! Pages and their index are CAS objects, never logical manifests. The caller must explicitly
//! choose whether paging is disabled or the chunk-reference count at which it begins. Reassembly
//! always derives and checks the original `mesh.v0.file-manifest` identity.

use core::fmt;
use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use mesh_cas::{Blake3 as CasBlake3, Cas, CasError, ContentDigest as _, Digest32 as CasDigest};
use mesh_store::{ChunkSlice, ManifestRecord, RecordDigest};
use mesh_types::{
    canonical_digest, decode_canonical, encode_canonical, Blake3, CanonicalEncode, CanonicalType,
    CanonicalValue, ChunkRef, Digest32, DomainTag, FieldSchema, FileManifest, RecordSchema,
};

/// Candidate B's fixed maximum page width.
pub const MANIFEST_PAGE_REFERENCES: usize = 256;

/// Caller-owned physical storage policy. There is deliberately no `Default` implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestPagingPolicy {
    /// Keep only the authoritative flat v0 record and its content chunks.
    Flat,
    /// Additionally store physical pages when the manifest has at least this many references.
    PageAtOrAbove(NonZeroUsize),
}

impl ManifestPagingPolicy {
    /// Explicitly disable physical paging.
    #[must_use]
    pub const fn flat() -> Self {
        Self::Flat
    }

    /// Enable physical paging at an explicitly supplied chunk-reference count.
    #[must_use]
    pub const fn page_at_or_above(chunk_references: NonZeroUsize) -> Self {
        Self::PageAtOrAbove(chunk_references)
    }

    const fn applies(self, chunk_references: usize) -> bool {
        match self {
            Self::Flat => false,
            Self::PageAtOrAbove(threshold) => chunk_references >= threshold.get(),
        }
    }
}

const CHUNK_FIELDS: &[FieldSchema] = &[
    FieldSchema::new("content_hash", CanonicalType::Bytes(Some(32))),
    FieldSchema::new("offset", CanonicalType::Unsigned),
    FieldSchema::new("length", CanonicalType::Unsigned),
];

const ROUTE_FIELDS: &[FieldSchema] = &[
    FieldSchema::new("first_offset", CanonicalType::Unsigned),
    FieldSchema::new("end_offset_exclusive", CanonicalType::Unsigned),
    FieldSchema::new("reference_count", CanonicalType::Unsigned),
    FieldSchema::new("page_digest", CanonicalType::Bytes(Some(32))),
];

/// One immutable physical page of 1–256 flat chunk references.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhysicalManifestPage {
    references: Vec<ChunkSlice>,
}

impl PhysicalManifestPage {
    /// References in their original flat-manifest order.
    #[must_use]
    pub fn references(&self) -> &[ChunkSlice] {
        &self.references
    }
}

impl CanonicalEncode for PhysicalManifestPage {
    const SCHEMA: RecordSchema = RecordSchema::new(
        DomainTag::new("mesh.physical.file-manifest-page.v1"),
        &[FieldSchema::new(
            "references",
            CanonicalType::Sequence(&CanonicalType::Group(CHUNK_FIELDS)),
        )],
    );

    fn canonical_fields(&self) -> Vec<CanonicalValue> {
        vec![CanonicalValue::Sequence(
            self.references
                .iter()
                .map(|reference| {
                    CanonicalValue::Group(vec![
                        CanonicalValue::Bytes(reference.digest.as_bytes().to_vec()),
                        CanonicalValue::Unsigned(reference.byte_offset),
                        CanonicalValue::Unsigned(reference.byte_length),
                    ])
                })
                .collect(),
        )]
    }
}

/// The exact range and CAS identity of one page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageRoute {
    first_offset: u64,
    end_offset_exclusive: u64,
    reference_count: u64,
    page_digest: RecordDigest,
}

impl PageRoute {
    /// Digest naming the canonical page bytes in CAS.
    #[must_use]
    pub const fn page_digest(&self) -> RecordDigest {
        self.page_digest
    }
}

/// Candidate B's single, non-recursive physical index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhysicalManifestIndex {
    byte_length: u64,
    content_hash: RecordDigest,
    pages: Vec<PageRoute>,
}

impl PhysicalManifestIndex {
    /// Routes in file order.
    #[must_use]
    pub fn pages(&self) -> &[PageRoute] {
        &self.pages
    }
}

impl CanonicalEncode for PhysicalManifestIndex {
    const SCHEMA: RecordSchema = RecordSchema::new(
        DomainTag::new("mesh.physical.file-manifest-index.v1"),
        &[
            FieldSchema::new("byte_length", CanonicalType::Unsigned),
            FieldSchema::new("content_hash", CanonicalType::Bytes(Some(32))),
            FieldSchema::new(
                "pages",
                CanonicalType::Sequence(&CanonicalType::Group(ROUTE_FIELDS)),
            ),
        ],
    );

    fn canonical_fields(&self) -> Vec<CanonicalValue> {
        vec![
            CanonicalValue::Unsigned(self.byte_length),
            CanonicalValue::Bytes(self.content_hash.as_bytes().to_vec()),
            CanonicalValue::Sequence(
                self.pages
                    .iter()
                    .map(|route| {
                        CanonicalValue::Group(vec![
                            CanonicalValue::Unsigned(route.first_offset),
                            CanonicalValue::Unsigned(route.end_offset_exclusive),
                            CanonicalValue::Unsigned(route.reference_count),
                            CanonicalValue::Bytes(route.page_digest.as_bytes().to_vec()),
                        ])
                    })
                    .collect(),
            ),
        ]
    }
}

/// Prepared physical objects for one authoritative v0 manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PagedManifest {
    index: PhysicalManifestIndex,
    index_digest: RecordDigest,
    objects: Vec<Vec<u8>>,
}

impl PagedManifest {
    /// Build physical pages only when the explicit policy applies.
    pub fn prepare(
        manifest: &ManifestRecord,
        policy: ManifestPagingPolicy,
    ) -> Result<Option<Self>, ManifestPagingError> {
        if !policy.applies(manifest.chunks.len()) {
            return Ok(None);
        }
        validate_flat(manifest.byte_length, &manifest.chunks)?;
        let found = logical_manifest_id(
            manifest.byte_length,
            manifest.content_digest,
            &manifest.chunks,
        );
        if found != manifest.id {
            return Err(ManifestPagingError::LogicalIdentityMismatch {
                expected: manifest.id,
                found,
            });
        }
        let mut routes = Vec::new();
        let mut objects = Vec::new();
        let mut digests = BTreeSet::new();
        for references in manifest.chunks.chunks(MANIFEST_PAGE_REFERENCES) {
            let page = PhysicalManifestPage {
                references: references.to_vec(),
            };
            let bytes = encode_canonical(&page);
            let digest = RecordDigest::from_bytes(*CasBlake3::digest_bytes(&bytes).as_bytes());
            if !digests.insert(*digest.as_bytes()) {
                return Err(ManifestPagingError::DuplicatePage(digest));
            }
            let first = references[0].byte_offset;
            let last = references.last().expect("a chunks() page is non-empty");
            routes.push(PageRoute {
                first_offset: first,
                end_offset_exclusive: last
                    .byte_offset
                    .checked_add(last.byte_length)
                    .ok_or(ManifestPagingError::InvalidLayout("page end overflow"))?,
                reference_count: references.len() as u64,
                page_digest: digest,
            });
            objects.push(bytes);
        }
        let index = PhysicalManifestIndex {
            byte_length: manifest.byte_length,
            content_hash: manifest.content_digest,
            pages: routes,
        };
        let index_bytes = encode_canonical(&index);
        let index_digest =
            RecordDigest::from_bytes(*CasBlake3::digest_bytes(&index_bytes).as_bytes());
        objects.push(index_bytes);
        Ok(Some(Self {
            index,
            index_digest,
            objects,
        }))
    }

    /// Physical index record.
    #[must_use]
    pub const fn index(&self) -> &PhysicalManifestIndex {
        &self.index
    }

    /// CAS name of the physical index.
    #[must_use]
    pub const fn index_digest(&self) -> RecordDigest {
        self.index_digest
    }

    /// Canonical page encodings followed by the canonical index encoding.
    #[must_use]
    pub fn cas_objects(&self) -> &[Vec<u8>] {
        &self.objects
    }
}

/// A physical record was absent, malformed, inconsistent, or named the wrong v0 manifest.
#[derive(Debug)]
pub enum ManifestPagingError {
    /// Verified CAS read/promotion failure.
    Cas(CasError),
    /// Canonical physical bytes did not decode.
    Decode(mesh_types::DecodeError),
    /// Page/index topology violated candidate B.
    InvalidLayout(&'static str),
    /// One physical page was routed more than once.
    DuplicatePage(RecordDigest),
    /// Reconstructed canonical v0 identity differs from the requested identity.
    LogicalIdentityMismatch {
        /// Identity the caller requested.
        expected: RecordDigest,
        /// Identity derived from reconstructed v0 bytes.
        found: RecordDigest,
    },
}

impl fmt::Display for ManifestPagingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cas(error) => error.fmt(formatter),
            Self::Decode(error) => error.fmt(formatter),
            Self::InvalidLayout(reason) => write!(formatter, "invalid physical manifest: {reason}"),
            Self::DuplicatePage(digest) => write!(formatter, "duplicate physical page {digest}"),
            Self::LogicalIdentityMismatch { expected, found } => write!(
                formatter,
                "physical pages derive v0 manifest {found}, expected {expected}"
            ),
        }
    }
}

impl std::error::Error for ManifestPagingError {}

impl From<CasError> for ManifestPagingError {
    fn from(error: CasError) -> Self {
        Self::Cas(error)
    }
}

impl From<mesh_types::DecodeError> for ManifestPagingError {
    fn from(error: mesh_types::DecodeError) -> Self {
        Self::Decode(error)
    }
}

/// Read candidate B records from CAS and return the exact authoritative v0 manifest.
pub fn reconstruct_paged_manifest(
    cas: &Cas,
    index_digest: RecordDigest,
    expected_manifest_id: RecordDigest,
) -> Result<ManifestRecord, ManifestPagingError> {
    let index_bytes = cas.read(&CasDigest::from_bytes(*index_digest.as_bytes()))?;
    let index = decode_index(&index_bytes)?;
    if index.pages.is_empty() {
        return Err(ManifestPagingError::InvalidLayout(
            "paged index has no pages",
        ));
    }
    let mut chunks = Vec::new();
    let mut cursor = 0u64;
    let mut seen = BTreeSet::new();
    for route in &index.pages {
        if !seen.insert(*route.page_digest.as_bytes()) {
            return Err(ManifestPagingError::DuplicatePage(route.page_digest));
        }
        if route.first_offset != cursor {
            return Err(ManifestPagingError::InvalidLayout(
                "page routes are reordered or gapped",
            ));
        }
        if route.reference_count == 0 || route.reference_count > MANIFEST_PAGE_REFERENCES as u64 {
            return Err(ManifestPagingError::InvalidLayout(
                "page reference count is outside 1..=256",
            ));
        }
        let bytes = cas.read(&CasDigest::from_bytes(*route.page_digest.as_bytes()))?;
        let page = decode_page(&bytes)?;
        if page.references.len() as u64 != route.reference_count {
            return Err(ManifestPagingError::InvalidLayout(
                "page route count disagrees with page",
            ));
        }
        validate_flat(route.end_offset_exclusive, &page.references)?;
        if page.references[0].byte_offset != route.first_offset {
            return Err(ManifestPagingError::InvalidLayout(
                "page first offset disagrees with route",
            ));
        }
        let last = page.references.last().expect("validated non-empty page");
        let end = last
            .byte_offset
            .checked_add(last.byte_length)
            .ok_or(ManifestPagingError::InvalidLayout("page end overflow"))?;
        if end != route.end_offset_exclusive {
            return Err(ManifestPagingError::InvalidLayout(
                "page end disagrees with route",
            ));
        }
        cursor = end;
        chunks.extend(page.references);
    }
    if cursor != index.byte_length {
        return Err(ManifestPagingError::InvalidLayout(
            "index byte length disagrees with routes",
        ));
    }
    let found = logical_manifest_id(index.byte_length, index.content_hash, &chunks);
    if found != expected_manifest_id {
        return Err(ManifestPagingError::LogicalIdentityMismatch {
            expected: expected_manifest_id,
            found,
        });
    }
    Ok(ManifestRecord {
        id: found,
        byte_length: index.byte_length,
        content_digest: index.content_hash,
        chunks,
    })
}

fn logical_manifest_id(
    byte_length: u64,
    content_digest: RecordDigest,
    chunks: &[ChunkSlice],
) -> RecordDigest {
    let logical = FileManifest::new(
        byte_length,
        Digest32::from_bytes(*content_digest.as_bytes()),
        chunks
            .iter()
            .map(|chunk| {
                ChunkRef::new(
                    Digest32::from_bytes(*chunk.digest.as_bytes()),
                    chunk.byte_offset,
                    chunk.byte_length,
                )
            })
            .collect(),
    );
    RecordDigest::from_bytes(*canonical_digest::<Blake3, _>(&logical).as_bytes())
}

fn validate_flat(byte_length: u64, references: &[ChunkSlice]) -> Result<(), ManifestPagingError> {
    if references.is_empty() {
        return Err(ManifestPagingError::InvalidLayout(
            "paged manifest has no references",
        ));
    }
    let mut cursor = references[0].byte_offset;
    for reference in references {
        if reference.byte_offset != cursor || reference.byte_length == 0 {
            return Err(ManifestPagingError::InvalidLayout(
                "references are gapped, reordered, or empty",
            ));
        }
        cursor = cursor
            .checked_add(reference.byte_length)
            .ok_or(ManifestPagingError::InvalidLayout("reference end overflow"))?;
    }
    if cursor != byte_length {
        return Err(ManifestPagingError::InvalidLayout(
            "references do not end at declared length",
        ));
    }
    Ok(())
}

fn digest(bytes: Vec<u8>) -> Result<RecordDigest, ManifestPagingError> {
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| ManifestPagingError::InvalidLayout("digest width is not 32 bytes"))?;
    Ok(RecordDigest::from_bytes(array))
}

fn unsigned(value: &CanonicalValue) -> Result<u64, ManifestPagingError> {
    if let CanonicalValue::Unsigned(value) = value {
        Ok(*value)
    } else {
        Err(ManifestPagingError::InvalidLayout(
            "decoded unsigned value changed shape",
        ))
    }
}

fn bytes(value: &CanonicalValue) -> Result<Vec<u8>, ManifestPagingError> {
    if let CanonicalValue::Bytes(value) = value {
        Ok(value.clone())
    } else {
        Err(ManifestPagingError::InvalidLayout(
            "decoded byte value changed shape",
        ))
    }
}

fn decode_page(encoded: &[u8]) -> Result<PhysicalManifestPage, ManifestPagingError> {
    let fields = decode_canonical::<PhysicalManifestPage>(encoded)?;
    let CanonicalValue::Sequence(values) = &fields[0] else {
        return Err(ManifestPagingError::InvalidLayout(
            "page references changed shape",
        ));
    };
    let mut references = Vec::with_capacity(values.len());
    for value in values {
        let CanonicalValue::Group(fields) = value else {
            return Err(ManifestPagingError::InvalidLayout(
                "page reference changed shape",
            ));
        };
        references.push(ChunkSlice {
            digest: digest(bytes(&fields[0])?)?,
            byte_offset: unsigned(&fields[1])?,
            byte_length: unsigned(&fields[2])?,
        });
    }
    Ok(PhysicalManifestPage { references })
}

fn decode_index(encoded: &[u8]) -> Result<PhysicalManifestIndex, ManifestPagingError> {
    let fields = decode_canonical::<PhysicalManifestIndex>(encoded)?;
    let CanonicalValue::Sequence(values) = &fields[2] else {
        return Err(ManifestPagingError::InvalidLayout(
            "index routes changed shape",
        ));
    };
    let mut pages = Vec::with_capacity(values.len());
    for value in values {
        let CanonicalValue::Group(fields) = value else {
            return Err(ManifestPagingError::InvalidLayout(
                "page route changed shape",
            ));
        };
        pages.push(PageRoute {
            first_offset: unsigned(&fields[0])?,
            end_offset_exclusive: unsigned(&fields[1])?,
            reference_count: unsigned(&fields[2])?,
            page_digest: digest(bytes(&fields[3])?)?,
        });
    }
    Ok(PhysicalManifestIndex {
        byte_length: unsigned(&fields[0])?,
        content_hash: digest(bytes(&fields[1])?)?,
        pages,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::num::NonZeroUsize;
    use std::path::PathBuf;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mesh-manifest-paging-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    fn manifest(reference_count: usize) -> ManifestRecord {
        let content_digest = RecordDigest::from_bytes([0xa5; 32]);
        let chunks = (0..reference_count)
            .map(|offset| ChunkSlice {
                digest: RecordDigest::from_bytes([(offset % 251) as u8; 32]),
                byte_offset: offset as u64,
                byte_length: 1,
            })
            .collect::<Vec<_>>();
        let logical = FileManifest::new(
            reference_count as u64,
            Digest32::from_bytes(*content_digest.as_bytes()),
            chunks
                .iter()
                .map(|chunk| {
                    ChunkRef::new(
                        Digest32::from_bytes(*chunk.digest.as_bytes()),
                        chunk.byte_offset,
                        chunk.byte_length,
                    )
                })
                .collect(),
        );
        ManifestRecord {
            id: RecordDigest::from_bytes(*canonical_digest::<Blake3, _>(&logical).as_bytes()),
            byte_length: reference_count as u64,
            content_digest,
            chunks,
        }
    }

    fn promote_pages(cas: &Cas, paged: &PagedManifest) {
        for object in paged.cas_objects() {
            cas.promote(object.clone()).expect("physical CAS promotion");
        }
    }

    #[test]
    fn explicit_threshold_and_large_manifest_roundtrip_preserve_v0() {
        let logical = manifest(600);
        assert!(
            PagedManifest::prepare(&logical, ManifestPagingPolicy::flat())
                .expect("flat policy")
                .is_none()
        );
        assert!(PagedManifest::prepare(
            &logical,
            ManifestPagingPolicy::page_at_or_above(NonZeroUsize::new(601).unwrap())
        )
        .expect("high explicit threshold")
        .is_none());
        let paged = PagedManifest::prepare(
            &logical,
            ManifestPagingPolicy::page_at_or_above(NonZeroUsize::new(600).unwrap()),
        )
        .expect("valid paging")
        .expect("threshold applies");
        assert_eq!(paged.index().pages().len(), 3);
        assert_eq!(paged.index().pages()[0].reference_count, 256);
        assert_eq!(paged.index().pages()[1].reference_count, 256);
        assert_eq!(paged.index().pages()[2].reference_count, 88);

        let root = scratch("roundtrip");
        let _ = fs::remove_dir_all(&root);
        let cas = Cas::open(&root).expect("CAS");
        promote_pages(&cas, &paged);
        let reconstructed = reconstruct_paged_manifest(&cas, paged.index_digest(), logical.id)
            .expect("verified reconstruction");
        assert_eq!(reconstructed, logical);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_page_is_refused() {
        let logical = manifest(300);
        let paged = PagedManifest::prepare(
            &logical,
            ManifestPagingPolicy::page_at_or_above(NonZeroUsize::new(1).unwrap()),
        )
        .unwrap()
        .unwrap();
        let root = scratch("missing");
        let _ = fs::remove_dir_all(&root);
        let cas = Cas::open(&root).expect("CAS");
        cas.promote(paged.cas_objects().last().unwrap().clone())
            .expect("index only");
        assert!(matches!(
            reconstruct_paged_manifest(&cas, paged.index_digest(), logical.id),
            Err(ManifestPagingError::Cas(CasError::Absent { .. }))
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reordered_and_duplicate_page_routes_are_refused() {
        let logical = manifest(600);
        let paged = PagedManifest::prepare(
            &logical,
            ManifestPagingPolicy::page_at_or_above(NonZeroUsize::new(1).unwrap()),
        )
        .unwrap()
        .unwrap();
        let root = scratch("topology");
        let _ = fs::remove_dir_all(&root);
        let cas = Cas::open(&root).expect("CAS");
        for page in &paged.cas_objects()[..paged.cas_objects().len() - 1] {
            cas.promote(page.clone()).expect("page");
        }

        let mut reordered = paged.index().clone();
        reordered.pages.swap(0, 1);
        let promoted = cas
            .promote(encode_canonical(&reordered))
            .expect("reordered index");
        assert!(matches!(
            reconstruct_paged_manifest(
                &cas,
                RecordDigest::from_bytes(*promoted.digest().as_bytes()),
                logical.id
            ),
            Err(ManifestPagingError::InvalidLayout(_))
        ));

        let mut duplicate = paged.index().clone();
        duplicate.pages[1].page_digest = duplicate.pages[0].page_digest;
        let promoted = cas
            .promote(encode_canonical(&duplicate))
            .expect("duplicate index");
        assert!(matches!(
            reconstruct_paged_manifest(
                &cas,
                RecordDigest::from_bytes(*promoted.digest().as_bytes()),
                logical.id
            ),
            Err(ManifestPagingError::DuplicatePage(_))
        ));
        let _ = fs::remove_dir_all(&root);
    }
}
