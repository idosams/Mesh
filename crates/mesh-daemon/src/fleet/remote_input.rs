//! Bounded immutable input description and receipt through the existing verified CAS.
//! The native caller owns/authenticates the session and private store. No path is materialized here.
use super::{refuse, Error, RemoteAssignment};
use crate::ipc::Json;
use mesh_cas::{Blake3, Cas, ContentDigest, Digest32, DigestHasher, DurableFs, StdFs};
use std::borrow::Cow;
#[cfg(unix)]
mod native;
use mesh_store::RecordDigest;
use mesh_types::NormalizedName;
#[cfg(unix)]
pub use native::NativeRemoteInputReceiver;
use std::collections::BTreeMap;

const MAX_MANIFEST_BYTES: usize = 1_048_576;
const MAX_ENTRIES: usize = 4096;
const MAX_CHUNKS: usize = 16_384;
const MAX_CHUNK_BYTES: u64 = 4_194_304;
const MAX_TOTAL_BYTES: u64 = 2_147_483_648;
const MAX_PART_BYTES: usize = 65_536;
const DOMAIN: &[u8] = b"mesh.v1.fleet-input-manifest\0";

/// One complete CAS chunk in reconstruction order; offsets follow from preceding lengths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteInputChunk {
    /// Hash of the chunk's complete bytes.
    pub digest: Digest32,
    /// Exact positive length, bounded independently of wire part size.
    pub bytes: u64,
}
/// Portable saved tree entry. Links and special files have no representable input form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteInputEntry {
    /// An explicit directory, including empty directories.
    Directory {
        /// Confined, nonempty project-relative path.
        path: String,
    },
    /// Regular file content and Mesh's portable executable metadata.
    File {
        /// Confined, nonempty project-relative path.
        path: String,
        /// Whether the saved file is executable.
        executable: bool,
        /// Digest of the complete concatenated file, including the empty-file digest.
        digest: Digest32,
        /// Ordered complete chunks; empty only for an empty file.
        chunks: Vec<RemoteInputChunk>,
    },
}
impl RemoteInputEntry {
    fn path(&self) -> &str {
        match self {
            Self::Directory { path } | Self::File { path, .. } => path,
        }
    }
}
/// Validated canonical inventory, bound to one exact saved input and transfer-bundle digest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteInputManifest {
    input: RecordDigest,
    entries: Vec<RemoteInputEntry>,
    chunks: BTreeMap<Digest32, u64>,
    encoded: String,
    bundle: RecordDigest,
}
impl RemoteInputManifest {
    /// Build from a native-verified saved inventory. This does not itself prove source provenance.
    pub fn new(input: RecordDigest, mut entries: Vec<RemoteInputEntry>) -> Result<Self, Error> {
        if entries.len() > MAX_ENTRIES {
            return refuse("remote-input-entry-limit");
        }
        entries.sort_by(|a, b| a.path().cmp(b.path()));
        let mut paths = BTreeMap::new();
        let mut chunks = BTreeMap::new();
        let mut total = 0u64;
        let mut chunk_count = 0usize;
        for entry in &entries {
            let path = entry.path();
            if path.len() > 4096
                || path
                    .split('/')
                    .any(|part| part.len() > 255 || NormalizedName::new(part).is_err())
            {
                return refuse("remote-input-path");
            }
            if paths
                .insert(path, matches!(entry, RemoteInputEntry::Directory { .. }))
                .is_some()
            {
                return refuse("remote-input-duplicate-path");
            }
            if let Some((parent, _)) = path.rsplit_once('/') {
                if paths.get(parent) != Some(&true) {
                    return refuse("remote-input-parent");
                }
            }
            if let RemoteInputEntry::File {
                chunks: file_chunks,
                digest,
                ..
            } = entry
            {
                if file_chunks.is_empty() && *digest != Blake3::digest_bytes(&[]) {
                    return refuse("remote-input-empty-digest");
                }
                for chunk in file_chunks {
                    chunk_count += 1;
                    if chunk_count > MAX_CHUNKS || chunk.bytes == 0 || chunk.bytes > MAX_CHUNK_BYTES
                    {
                        return refuse("remote-input-chunk-limit");
                    }
                    total = total
                        .checked_add(chunk.bytes)
                        .ok_or(Error::Refused("remote-input-byte-limit"))?;
                    if total > MAX_TOTAL_BYTES {
                        return refuse("remote-input-byte-limit");
                    }
                    if chunks
                        .insert(chunk.digest, chunk.bytes)
                        .is_some_and(|previous| previous != chunk.bytes)
                    {
                        return refuse("remote-input-chunk-length-conflict");
                    }
                }
            }
        }
        let encoded = encode(input, &entries);
        if encoded.len() > MAX_MANIFEST_BYTES {
            return refuse("remote-input-manifest-limit");
        }
        let mut hasher = Blake3::hasher();
        hasher.update(DOMAIN);
        hasher.update(encoded.as_bytes());
        let bundle = RecordDigest::from_bytes(*hasher.finalize().as_bytes());
        Ok(Self {
            input,
            entries,
            chunks,
            encoded,
            bundle,
        })
    }
    /// Decode only the canonical closed form expected by a natively authenticated assignment.
    pub fn decode(
        raw: &str,
        expected_input: RecordDigest,
        expected_bundle: RecordDigest,
    ) -> Result<Self, Error> {
        if raw.len() > MAX_MANIFEST_BYTES {
            return refuse("remote-input-manifest-limit");
        }
        let value = Json::parse(raw).map_err(|_| Error::Refused("remote-input-format"))?;
        if text(&value, "schema")? != "mesh.remote-input/v1" {
            return refuse("remote-input-format");
        }
        let input = RecordDigest::parse_hex(text(&value, "input")?)
            .map_err(|_| Error::Refused("remote-input-format"))?;
        if input != expected_input {
            return refuse("remote-input-version-mismatch");
        }
        let rows = value
            .get("entries")
            .and_then(Json::as_array)
            .ok_or(Error::Refused("remote-input-format"))?;
        if rows.len() > MAX_ENTRIES {
            return refuse("remote-input-entry-limit");
        }
        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            let path = text(row, "path")?.to_owned();
            entries.push(match text(row, "kind")? {
                "directory" => RemoteInputEntry::Directory { path },
                "file" => {
                    let parts = row
                        .get("chunks")
                        .and_then(Json::as_array)
                        .ok_or(Error::Refused("remote-input-format"))?;
                    if parts.len() > MAX_CHUNKS {
                        return refuse("remote-input-chunk-limit");
                    }
                    let chunks = parts
                        .iter()
                        .map(|part| {
                            Ok(RemoteInputChunk {
                                digest: digest(part, "digest")?,
                                bytes: number(part, "bytes")?,
                            })
                        })
                        .collect::<Result<Vec<_>, Error>>()?;
                    RemoteInputEntry::File {
                        path,
                        executable: row
                            .get("executable")
                            .and_then(Json::as_bool)
                            .ok_or(Error::Refused("remote-input-format"))?,
                        digest: digest(row, "digest")?,
                        chunks,
                    }
                }
                _ => return refuse("remote-input-entry-kind"),
            });
        }
        let manifest = Self::new(input, entries)?;
        if manifest.encoded != raw || manifest.bundle != expected_bundle {
            return refuse("remote-input-bundle-mismatch");
        }
        Ok(manifest)
    }
    /// Exact saved source operation.
    pub fn input(&self) -> RecordDigest {
        self.input
    }
    /// Domain-bound canonical manifest identity to include in the remote assignment.
    pub fn bundle(&self) -> RecordDigest {
        self.bundle
    }
    /// Canonical task-bearing bytes; keep private rather than placing them in public logs.
    pub fn encoded(&self) -> &str {
        &self.encoded
    }
    /// Verified tree description, not permission to write any destination.
    pub fn entries(&self) -> &[RemoteInputEntry] {
        &self.entries
    }
}
fn text<'a>(value: &'a Json, key: &str) -> Result<&'a str, Error> {
    value
        .get(key)
        .and_then(Json::as_text)
        .ok_or(Error::Refused("remote-input-format"))
}
fn number(value: &Json, key: &str) -> Result<u64, Error> {
    match value.get(key) {
        Some(Json::Number(number)) => Ok(*number),
        _ => refuse("remote-input-format"),
    }
}
fn digest(value: &Json, key: &str) -> Result<Digest32, Error> {
    Digest32::parse_hex(text(value, key)?).map_err(|_| Error::Refused("remote-input-format"))
}
fn encode(input: RecordDigest, entries: &[RemoteInputEntry]) -> String {
    Json::object([
        ("schema", Json::text("mesh.remote-input/v1")),
        ("input", Json::text(input.to_string())),
        (
            "entries",
            Json::Array(
                entries
                    .iter()
                    .map(|entry| match entry {
                        RemoteInputEntry::Directory { path } => Json::object([
                            ("kind", Json::text("directory")),
                            ("path", Json::text(path)),
                        ]),
                        RemoteInputEntry::File {
                            path,
                            executable,
                            digest,
                            chunks,
                        } => Json::object([
                            ("kind", Json::text("file")),
                            ("path", Json::text(path)),
                            ("executable", Json::Bool(*executable)),
                            ("digest", Json::text(digest.to_string())),
                            (
                                "chunks",
                                Json::Array(
                                    chunks
                                        .iter()
                                        .map(|chunk| {
                                            Json::object([
                                                ("digest", Json::text(chunk.digest.to_string())),
                                                ("bytes", Json::Number(chunk.bytes)),
                                            ])
                                        })
                                        .collect(),
                                ),
                            ),
                        ]),
                    })
                    .collect(),
            ),
        ),
    ])
    .encode()
}

/// Serial receipt into a native-admitted private CAS. The caller must retain store identity and
/// exclude competing receivers; a network peer cannot supply the store or authorize filesystem use.
pub struct RemoteInputReceiver<'a, F: DurableFs = StdFs> {
    manifest: Cow<'a, RemoteInputManifest>,
    cas: &'a Cas<F>,
}
impl<'a, F: DurableFs> RemoteInputReceiver<'a, F> {
    /// Bind an already-verified manifest to the retained native assignment and private store.
    /// Possession of this data is not peer authentication or store-ownership authority.
    pub fn new(
        manifest: RemoteInputManifest,
        assignment: &RemoteAssignment,
        cas: &'a Cas<F>,
    ) -> Result<Self, Error> {
        if manifest.input() != assignment.input || manifest.bundle() != assignment.bundle {
            return refuse("remote-input-assignment-mismatch");
        }
        Ok(Self {
            manifest: Cow::Owned(manifest),
            cas,
        })
    }
    /// Durable resume offset and verified-complete flag for a declared chunk only.
    pub fn status(&self, digest: Digest32) -> Result<(u64, bool), Error> {
        let size = self.declared(digest)?;
        let incoming = self
            .cas
            .begin_receive(digest)
            .map_err(|_| Error::Refused("remote-input-store"))?;
        if incoming.next_offset() > size
            || (incoming.is_complete() && incoming.next_offset() != size)
        {
            return refuse("remote-input-stored-length");
        }
        Ok((incoming.next_offset(), incoming.is_complete()))
    }
    /// Accept one exact contiguous bounded part. A final part must end at the declared length.
    /// The CAS verifies complete chunk bytes before promoting them; no partial is readiness evidence.
    pub fn accept(
        &mut self,
        digest: Digest32,
        offset: u64,
        bytes: &[u8],
        final_part: bool,
    ) -> Result<(), Error> {
        let size = self.declared(digest)?;
        let end = offset
            .checked_add(bytes.len() as u64)
            .ok_or(Error::Refused("remote-input-part"))?;
        if bytes.is_empty()
            || bytes.len() > MAX_PART_BYTES
            || end > size
            || final_part != (end == size)
        {
            return refuse("remote-input-part");
        }
        let mut incoming = self
            .cas
            .begin_receive(digest)
            .map_err(|_| Error::Refused("remote-input-store"))?;
        incoming
            .accept(offset, bytes, final_part)
            .map_err(|_| Error::Refused("remote-input-transfer"))?;
        Ok(())
    }
    /// Re-read verified CAS content and hash each complete file. Success describes current input
    /// availability only; native materialization must still retain identity and verify its output.
    pub fn verify_complete(&self) -> Result<(), Error> {
        for entry in &self.manifest.entries {
            if let RemoteInputEntry::File { digest, chunks, .. } = entry {
                let mut hasher = Blake3::hasher();
                for chunk in chunks {
                    let bytes = self
                        .cas
                        .read(&chunk.digest)
                        .map_err(|_| Error::Refused("remote-input-incomplete"))?;
                    if bytes.len() as u64 != chunk.bytes {
                        return refuse("remote-input-stored-length");
                    }
                    hasher.update(&bytes);
                }
                if hasher.finalize() != *digest {
                    return refuse("remote-input-file-integrity");
                }
            }
        }
        Ok(())
    }
    /// Create a fresh private working tree through native-admitted storage. Existing or partial
    /// allocations refuse; this neither launches a provider nor grants execution ownership.
    #[cfg(unix)]
    pub fn materialize(
        &self,
        destination: &super::RemoteInputDestination,
        allocation_id: &str,
    ) -> std::io::Result<super::RemoteInputAllocation> {
        destination.materialize(&self.manifest, self.cas, allocation_id)
    }

    fn declared(&self, digest: Digest32) -> Result<u64, Error> {
        self.manifest
            .chunks
            .get(&digest)
            .copied()
            .ok_or(Error::Refused("remote-input-undeclared-chunk"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "mesh-remote-input-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn input() -> RecordDigest {
        RecordDigest::from_bytes([1; 32])
    }
    fn file(path: &str, bytes: &[u8]) -> RemoteInputEntry {
        let digest = Blake3::digest_bytes(bytes);
        RemoteInputEntry::File {
            path: path.into(),
            executable: true,
            digest,
            chunks: if bytes.is_empty() {
                vec![]
            } else {
                vec![RemoteInputChunk {
                    digest,
                    bytes: bytes.len() as u64,
                }]
            },
        }
    }
    fn manifest(bytes: &[u8]) -> RemoteInputManifest {
        RemoteInputManifest::new(
            input(),
            vec![
                file("src/main", bytes),
                RemoteInputEntry::Directory { path: "src".into() },
                file("empty", b""),
                RemoteInputEntry::Directory {
                    path: "empty-dir".into(),
                },
            ],
        )
        .unwrap()
    }
    fn receiving(manifest: RemoteInputManifest, cas: &Cas) -> RemoteInputReceiver<'_> {
        let assignment = RemoteAssignment {
            id: "assignment".into(),
            worker_key: "ab".repeat(32),
            input: manifest.input(),
            bundle: manifest.bundle(),
            lease_sequence: 1,
            lease_until_ms: 1000,
        };
        RemoteInputReceiver::new(manifest, &assignment, cas).unwrap()
    }
    #[test]
    fn receiver_cannot_bind_a_manifest_to_another_assignment() {
        let fixture = Fixture::new();
        let cas = Cas::open(&fixture.0).unwrap();
        let manifest = manifest(b"hello");
        let mut assignment = RemoteAssignment {
            id: "assignment".into(),
            worker_key: "ab".repeat(32),
            input: input(),
            bundle: input(),
            lease_sequence: 1,
            lease_until_ms: 1000,
        };
        assert!(RemoteInputReceiver::new(manifest.clone(), &assignment, &cas).is_err());
        assignment.bundle = manifest.bundle();
        assignment.input = RecordDigest::from_bytes([3; 32]);
        assert!(RemoteInputReceiver::new(manifest, &assignment, &cas).is_err());
    }
    #[test]
    fn canonical_manifest_preserves_empty_entries_modes_and_binds_exact_input() {
        let value = manifest(b"hello");
        assert_eq!(
            RemoteInputManifest::decode(value.encoded(), input(), value.bundle()).unwrap(),
            value
        );
        let mut reversed = value.entries().to_vec();
        reversed.reverse();
        assert_eq!(RemoteInputManifest::new(input(), reversed).unwrap(), value);
        assert!(RemoteInputManifest::decode(
            value.encoded(),
            RecordDigest::from_bytes([2; 32]),
            value.bundle()
        )
        .is_err());
        assert!(RemoteInputManifest::decode(
            value.encoded(),
            input(),
            RecordDigest::from_bytes([3; 32])
        )
        .is_err());
        for invalid in [
            value
                .encoded()
                .replace("\"executable\":true", "\"executable\":false"),
            value
                .encoded()
                .replace("\"kind\":\"directory\"", "\"kind\":\"symlink\""),
            value
                .encoded()
                .replacen("\"input\":", "\"extra\":0,\"input\":", 1),
            format!(" {}", value.encoded()),
        ] {
            assert!(RemoteInputManifest::decode(&invalid, input(), value.bundle()).is_err());
        }
    }
    #[test]
    fn traversal_duplicate_names_missing_parents_and_file_parents_refuse() {
        for path in [
            "",
            "/absolute",
            "../escape",
            "a/../escape",
            "a//b",
            "a/",
            "a\\b",
            "nul\0name",
            ".",
        ] {
            assert!(
                RemoteInputManifest::new(input(), vec![file(path, b"x")]).is_err(),
                "{path:?}"
            );
        }
        for entries in [
            vec![file("same", b"x"), file("same", b"y")],
            vec![file("missing/file", b"x")],
            vec![file("parent", b"x"), file("parent/file", b"x")],
        ] {
            assert!(RemoteInputManifest::new(input(), entries).is_err());
        }
    }
    #[test]
    fn declared_lengths_entry_counts_and_total_content_are_bounded() {
        let mut entry = file("large", b"x");
        if let RemoteInputEntry::File { chunks, .. } = &mut entry {
            chunks[0].bytes = MAX_CHUNK_BYTES + 1;
        }
        assert!(RemoteInputManifest::new(input(), vec![entry]).is_err());
        let entries = (0..=MAX_ENTRIES)
            .map(|n| RemoteInputEntry::Directory {
                path: format!("dir-{n}"),
            })
            .collect();
        assert!(RemoteInputManifest::new(input(), entries).is_err());
        let chunk = RemoteInputChunk {
            digest: Blake3::digest_bytes(b"x"),
            bytes: MAX_CHUNK_BYTES,
        };
        let huge = RemoteInputEntry::File {
            path: "huge".into(),
            executable: false,
            digest: chunk.digest,
            chunks: vec![chunk; (MAX_TOTAL_BYTES / MAX_CHUNK_BYTES + 1) as usize],
        };
        assert!(RemoteInputManifest::new(input(), vec![huge]).is_err());
        let first = file("a", b"x");
        let mut other = file("b", b"x");
        if let RemoteInputEntry::File { chunks, .. } = &mut other {
            chunks[0].bytes = 2;
        }
        assert!(RemoteInputManifest::new(input(), vec![first, other]).is_err());
        assert!(
            RemoteInputManifest::decode(&" ".repeat(MAX_MANIFEST_BYTES + 1), input(), input())
                .is_err()
        );
    }
    #[test]
    fn durable_resume_and_lost_ack_do_not_repeat_bytes_or_mark_partial_ready() {
        let fixture = Fixture::new();
        let bytes = b"exact immutable input";
        let digest = Blake3::digest_bytes(bytes);
        let manifest = manifest(bytes);
        {
            let cas = Cas::open(&fixture.0).unwrap();
            let mut receiver = receiving(manifest.clone(), &cas);
            receiver.accept(digest, 0, &bytes[..5], false).unwrap();
            assert_eq!(receiver.status(digest).unwrap(), (5, false));
            assert!(receiver.verify_complete().is_err());
        }
        let cas = Cas::open(&fixture.0).unwrap();
        let mut receiver = receiving(manifest.clone(), &cas);
        assert_eq!(receiver.status(digest).unwrap(), (5, false));
        assert!(receiver.accept(digest, 0, &bytes[..5], false).is_err());
        assert_eq!(receiver.status(digest).unwrap(), (5, false));
        receiver.accept(digest, 5, &bytes[5..], true).unwrap();
        receiver.verify_complete().unwrap();
        drop(receiver);
        drop(cas);
        let cas = Cas::open(&fixture.0).unwrap();
        let mut receiver = receiving(manifest, &cas);
        assert_eq!(receiver.status(digest).unwrap(), (bytes.len() as u64, true));
        assert!(receiver.accept(digest, 5, &bytes[5..], true).is_err());
        receiver.verify_complete().unwrap();
        assert_eq!(cas.read(&digest).unwrap(), bytes);
    }
    #[test]
    fn unexpected_oversized_or_corrupt_parts_never_become_verified_input() {
        let fixture = Fixture::new();
        let cas = Cas::open(&fixture.0).unwrap();
        let bytes = b"hello";
        let digest = Blake3::digest_bytes(bytes);
        let mut receiver = receiving(manifest(bytes), &cas);
        for (target, offset, part, final_part) in [
            (Blake3::digest_bytes(b"other"), 0, b"hello".as_slice(), true),
            (digest, 1, b"hello".as_slice(), true),
            (digest, 0, b"hello".as_slice(), false),
            (digest, 0, b"he".as_slice(), true),
            (digest, u64::MAX, b"he".as_slice(), true),
        ] {
            assert!(receiver.accept(target, offset, part, final_part).is_err());
            assert_eq!(receiver.status(digest).unwrap(), (0, false));
        }
        assert!(receiver.accept(digest, 0, b"wrong", true).is_err());
        assert!(!cas.contains(&digest));
        assert_eq!(receiver.status(digest).unwrap(), (0, false));
        receiver.accept(digest, 0, bytes, true).unwrap();
        receiver.verify_complete().unwrap();
        let large = vec![1; MAX_PART_BYTES + 1];
        let d = Blake3::digest_bytes(&large);
        let mut receiver = receiving(manifest(&large), &cas);
        assert!(receiver.accept(d, 0, &large, true).is_err());
        assert_eq!(receiver.status(d).unwrap(), (0, false));
    }
    #[test]
    fn complete_chunk_hashes_do_not_substitute_for_complete_file_integrity() {
        let fixture = Fixture::new();
        let cas = Cas::open(&fixture.0).unwrap();
        let mut entry = file("file", b"hello");
        if let RemoteInputEntry::File { digest, .. } = &mut entry {
            *digest = Blake3::digest_bytes(b"different file");
        }
        let manifest = RemoteInputManifest::new(input(), vec![entry]).unwrap();
        let mut receiver = receiving(manifest, &cas);
        let digest = Blake3::digest_bytes(b"hello");
        receiver.accept(digest, 0, b"hello", true).unwrap();
        assert_eq!(receiver.status(digest).unwrap(), (5, true));
        assert!(matches!(
            receiver.verify_complete(),
            Err(Error::Refused("remote-input-file-integrity"))
        ));
    }
}
