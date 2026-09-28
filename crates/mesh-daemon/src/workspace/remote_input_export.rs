//! Native saved-history authority for immutable fleet input export.
use super::OpenWorkspace;
use crate::fleet::{RemoteInputChunk, RemoteInputEntry, RemoteInputManifest};
use crate::root_authority::PinnedWorkspaceRoot;
use mesh_cas::{Blake3, ContentDigest as _, Digest32, StoreLayout};
use mesh_store::RecordDigest;
use std::collections::BTreeMap;
use std::io::{self, Read as _};
use std::path::PathBuf;

fn invalid() -> io::Error {
    io::Error::other("the exact saved fleet input is unavailable or changed")
}

/// Read-only export of a native-verified saved tree. Construction is confined to native history.
///
/// This owns no capture lock, provider, credentials or execution authority. The caller still must
/// bind its manifest to an authorized assignment and authenticated destination. It is not a network
/// endpoint. Holding it does not authorize retention or prevent collection: unavailable bytes refuse.
pub struct RemoteInputSource {
    manifest: RemoteInputManifest,
    chunks: BTreeMap<Digest32, u64>,
    roots: Vec<PinnedWorkspaceRoot>,
    store: PinnedWorkspaceRoot,
}
impl RemoteInputSource {
    /// Immutable verified description. Task-bearing names stay private; this is no dispatch grant.
    pub fn manifest(&self) -> &RemoteInputManifest {
        &self.manifest
    }

    fn verify_roots(&self) -> io::Result<()> {
        for root in &self.roots {
            root.ensure_namespace_identity().map_err(|_| invalid())?;
        }
        self.store
            .ensure_namespace_identity()
            .map_err(|_| invalid())
    }

    /// Read one declared complete chunk, at most 4 MiB, and hash it before returning any bytes.
    ///
    /// The read is bounded to the declared size plus one byte, even if the stored file grew.
    /// Links, non-files, replacement roots, missing bytes, wrong sizes and corruption refuse.
    /// Reads do not quarantine, repair, create directories or mutate source/history. A transport
    /// must split verified chunks into the receiver's 64 KiB parts and recheck session authority.
    /// Availability is a read-time fact; each later call revalidates the store and complete bytes.
    pub fn read_chunk(&self, digest: Digest32) -> io::Result<Vec<u8>> {
        let size = *self.chunks.get(&digest).ok_or_else(invalid)?;
        self.verify_roots()?;
        let relative = StoreLayout::new(PathBuf::new()).chunk_path(&digest);
        let file = self
            .store
            .filesystem()
            .read_only()
            .read_file(&relative)
            .map_err(|_| invalid())?;
        let mut bytes = Vec::with_capacity(size as usize + 1);
        file.take(size + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        if bytes.len() as u64 != size || Blake3::digest_bytes(&bytes) != digest {
            return Err(invalid());
        }
        self.verify_roots()?;
        Ok(bytes)
    }
}

impl OpenWorkspace {
    pub(crate) fn remote_input_source(
        &self,
        operation: RecordDigest,
    ) -> io::Result<RemoteInputSource> {
        self.ensure_physical_root().map_err(|_| invalid())?;
        let snapshot = self
            .historical_workspace_preview(operation)
            .map_err(|_| invalid())?;
        if snapshot
            .directories
            .len()
            .saturating_add(snapshot.files.len())
            > 4096
        {
            return Err(invalid());
        }
        let mut entries = Vec::with_capacity(snapshot.directories.len() + snapshot.files.len());
        for directory in snapshot.directories {
            entries.push(RemoteInputEntry::Directory {
                path: directory.path,
            });
        }
        for file in snapshot.files {
            let retained = self
                .record_index
                .manifest(&file.manifest_id)
                .ok_or_else(invalid)?;
            if retained.byte_length != file.byte_length
                || retained.content_digest != file.content_digest
            {
                return Err(invalid());
            }
            entries.push(RemoteInputEntry::File {
                path: file.path,
                executable: file.executable,
                digest: Digest32::from_bytes(*file.content_digest.as_bytes()),
                chunks: retained
                    .chunks
                    .iter()
                    .map(|chunk| RemoteInputChunk {
                        digest: Digest32::from_bytes(*chunk.digest.as_bytes()),
                        bytes: chunk.byte_length,
                    })
                    .collect(),
            });
        }
        let manifest = RemoteInputManifest::new(operation, entries).map_err(|_| invalid())?;
        let mut chunks = BTreeMap::new();
        for entry in manifest.entries() {
            if let RemoteInputEntry::File { chunks: parts, .. } = entry {
                for part in parts {
                    chunks.insert(part.digest, part.bytes);
                }
            }
        }
        let source = RemoteInputSource {
            manifest,
            chunks,
            roots: vec![self.pinned_root.clone(), self.storage_pinned_root.clone()],
            store: self.storage_pinned_root.clone(),
        };
        source.verify_roots()?;
        Ok(source)
    }
}
