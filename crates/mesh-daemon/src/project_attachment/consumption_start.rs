//! Immutable starting-snapshot preparation. No destination files or history are written here.
//! The consuming transaction must independently bind this result to its exact destination and
//! current grant under complete custody; a prepared operation list conveys no authority.
use super::{invalid, NativeGrantedInput, ObservationLimits};
use crate::checkpoint_storage::PreparedCheckpointFile;
use crate::ManifestPagingPolicy;
use mesh_chunking::ChunkingConfig;
use mesh_operations::{
    ActorId, ManifestId, NormalizedName, ObjectId, Operation, PortableMetadata, VersionId,
    WorkspaceId,
};
use mesh_types::{Blake3, ContentDigest as _};
use std::{
    collections::BTreeMap,
    io,
    path::{Component, Path},
};

pub(super) struct InitialSnapshot {
    pub(super) operations: Vec<Operation>,
    pub(super) files: Vec<PreparedCheckpointFile>,
}
struct BoundedBytes {
    bytes: Vec<u8>,
    limit: u64,
}
impl io::Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .filter(|n| (*n as u64) <= self.limit)
            .is_none()
        {
            return Err(invalid("starting content exceeded its admitted size"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
fn components(path: &str) -> io::Result<(&str, NormalizedName)> {
    if path.is_empty()
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(invalid("invalid saved starting path"));
    }
    let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
    Ok((parent, NormalizedName::new(name.to_owned()).map_err(error)?))
}
fn object(seed: &[u8], path: &str, kind: u8) -> ObjectId {
    let mut bytes = b"mesh.native-consumed-start/object/v1\0".to_vec();
    bytes.extend_from_slice(seed);
    bytes.push(kind);
    bytes.extend_from_slice(path.as_bytes());
    ObjectId::from_bytes(
        Blake3::digest_bytes(&bytes).as_bytes()[..16]
            .try_into()
            .unwrap(),
    )
}

// Only exact granted immutable bytes enter this builder. Callers retain the grant's custody
// through the entire bounded read; the later signer must run after that custody is released.
pub(super) fn prepare_initial_snapshot(
    input: &NativeGrantedInput<'_>,
    workspace: WorkspaceId,
    actor: ActorId,
    limits: ObservationLimits,
) -> io::Result<InitialSnapshot> {
    limits.validate()?;
    let mut directories = input.directories().collect::<Vec<_>>();
    let files = input.files().collect::<Vec<_>>();
    if directories
        .len()
        .checked_add(files.len())
        .filter(|n| *n <= limits.entries)
        .is_none()
    {
        return Err(invalid("starting snapshot exceeds entry budget"));
    }
    let mut remaining = limits.bytes;
    for file in &files {
        if file.byte_length > limits.file_bytes {
            return Err(invalid("starting file exceeds byte budget"));
        }
        remaining = remaining
            .checked_sub(file.byte_length)
            .ok_or_else(|| invalid("starting snapshot exceeds byte budget"))?;
    }
    let root = ObjectId::from_bytes([0; 16]);
    let mut seed = workspace.as_bytes().to_vec();
    seed.extend_from_slice(actor.as_bytes());
    // Explicit root declaration also gives a genuinely empty tree a real saved representation.
    let mut operations = vec![Operation::InitializeWorkspace { root_id: root }];
    let mut parents = BTreeMap::from([("", root)]);
    directories.sort_by_key(|path| (path.matches('/').count(), *path));
    for path in directories {
        let (parent, name) = components(path)?;
        let directory_id = *parents
            .get(parent)
            .ok_or_else(|| invalid("starting parent missing"))?;
        let id = object(&seed, path, 0);
        if parents.insert(path, id).is_some() {
            return Err(invalid("duplicate starting directory"));
        }
        operations.push(Operation::CreateDirectory { object_id: id });
        operations.push(Operation::LinkDirectoryEntry {
            directory_id,
            name,
            object_id: id,
            version_id: VersionId::from_bytes([0; 32]),
        });
    }
    let mut prepared = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for file in files {
        let (parent, name) = components(file.path)?;
        if parents.contains_key(file.path) || !names.insert(file.path) {
            return Err(invalid("duplicate starting entry"));
        }
        let directory_id = *parents
            .get(parent)
            .ok_or_else(|| invalid("starting parent missing"))?;
        let mut output = BoundedBytes {
            bytes: Vec::new(),
            limit: file.byte_length,
        };
        input.write_file(file.path, &mut output)?;
        let bytes = output.bytes;
        if bytes.len() as u64 != file.byte_length {
            return Err(invalid("starting file length changed"));
        }
        let retained = PreparedCheckpointFile::from_bytes(
            &bytes,
            &ChunkingConfig::default(),
            ManifestPagingPolicy::flat(),
        )
        .map_err(error)?;
        let id = object(&seed, file.path, 1);
        let mut version = seed.clone();
        version.extend_from_slice(id.as_bytes());
        version.extend_from_slice(Blake3::digest_bytes(&bytes).as_bytes());
        version.push(u8::from(file.executable));
        let version_id = VersionId::from_bytes(*Blake3::digest_bytes(&version).as_bytes());
        operations.push(Operation::CreateFile { object_id: id });
        operations.push(Operation::WriteFileVersion {
            object_id: id,
            version_id,
            parent_versions: vec![],
            manifest_id: ManifestId::from_bytes(*retained.manifest().id.as_bytes()),
            portable_metadata: PortableMetadata::new(file.executable),
        });
        operations.push(Operation::LinkDirectoryEntry {
            directory_id,
            name,
            object_id: id,
            version_id,
        });
        prepared.push(retained);
    }
    Ok(InitialSnapshot {
        operations,
        files: prepared,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    #[test]
    fn excess_stream_output_is_refused_before_buffer_growth() {
        let mut output = BoundedBytes {
            bytes: Vec::new(),
            limit: 3,
        };
        output.write_all(b"ab").unwrap();
        assert!(output.write_all(b"cd").is_err());
        assert_eq!(output.bytes, b"ab");
        output.write_all(b"c").unwrap();
        assert_eq!(output.bytes, b"abc");
        assert!(output.write_all(b"d").is_err());
        assert_eq!(output.bytes, b"abc");
    }
}
