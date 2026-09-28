//! Compile an immutable lane result into original-project operations, retaining proven identities.
use crate::checkpoint_storage::PreparedCheckpointFile;
use crate::ipc::Json;
use crate::workspace::{HistoricalOperationPlan, HistoricalWorkspacePreview, OpenWorkspace};
use crate::ManifestPagingPolicy;
use mesh_chunking::ChunkingConfig;
use mesh_operations::{NormalizedName, ObjectId, Operation, PortableMetadata, VersionId};
use mesh_store::RecordDigest;
use mesh_types::{Blake3, ContentDigest as _, PublicKey};
use std::collections::{BTreeMap, BTreeSet};
use std::io;

const ROOT: ObjectId = ObjectId::from_bytes([0; 16]);
fn error(value: impl std::fmt::Display) -> io::Error {
    io::Error::other(value.to_string())
}

/// Native signing capability for candidate import, distinct from approval authority.
pub trait CandidateImportSigner: crate::checkpoint_storage::CheckpointSigner {
    /// Sign the domain-separated receipt binding provenance to the exact authenticated operation.
    fn sign_import_provenance(
        &self,
        payload: &mesh_crypto::SigningPayload,
    ) -> Result<mesh_types::Signature, String>;
}

/// An opaque, read-only compilation. It grants no journal append, review or approval authority.
/// A writer must rederive it under retained project custody and bind an exact durable import receipt.
pub struct PreparedProjectCandidateImport {
    pub(crate) historical: HistoricalOperationPlan,
    pub(crate) files: Vec<PreparedCheckpointFile>,
    pub(crate) candidate: RecordDigest,
    digest: RecordDigest,
    pub(crate) snapshot: HistoricalWorkspacePreview,
}
impl PreparedProjectCandidateImport {
    /// Original-project operations validated against one exact historical predecessor.
    #[must_use]
    pub fn historical_plan(&self) -> &HistoricalOperationPlan {
        &self.historical
    }
    /// Bounded native evidence. This contains no file bytes or signing capability.
    #[must_use]
    pub fn context(&self) -> Json {
        Json::object([
            ("schema", Json::text("mesh.fleet-project-import-plan/v1")),
            ("candidate_receipt", Json::text(self.candidate.to_string())),
            ("digest", Json::text(self.digest.to_string())),
            ("historical", self.historical.context()),
            ("prepared_files", Json::Number(self.files.len() as u64)),
            ("approval_authority", Json::Bool(false)),
        ])
    }
}
struct Desired {
    object: ObjectId,
    parent: ObjectId,
    name: NormalizedName,
    directory: bool,
    retained: bool,
}

pub(crate) fn compile(
    project: &OpenWorkspace,
    predecessor: RecordDigest,
    target: &OpenWorkspace,
    snapshot: &HistoricalWorkspacePreview,
    origins: &BTreeMap<String, String>,
    candidate: &Json,
    actor: PublicKey,
) -> io::Result<PreparedProjectCandidateImport> {
    let limits = crate::project_attachment::ObservationLimits::default();
    let total = snapshot.files.iter().try_fold(0u64, |sum, file| {
        sum.checked_add(file.byte_length)
            .ok_or_else(|| error("candidate size overflow"))
    })?;
    if snapshot
        .files
        .len()
        .saturating_add(snapshot.directories.len())
        > limits.entries
        || total > limits.bytes
        || snapshot
            .files
            .iter()
            .any(|file| file.byte_length > limits.file_bytes)
    {
        return Err(error("candidate import exceeds content limits"));
    }
    let old = project
        .historical_capture_entries(predecessor)
        .map_err(error)?;
    let old_ids: BTreeMap<_, _> = old
        .values()
        .map(|entry| (entry.binding.object_id, entry))
        .collect();
    if old_ids.len() != old.len() {
        return Err(error("ambiguous original object identity"));
    }
    let receipt =
        RecordDigest::from_bytes(*Blake3::digest_bytes(candidate.encode().as_bytes()).as_bytes());
    let mut ids = BTreeMap::from([(String::new(), ROOT)]);
    let mut claimed = BTreeSet::from([ROOT]);
    let mut directories = BTreeSet::from([ROOT]);
    let mut locals = BTreeSet::new();
    let mut mapped = BTreeSet::new();
    let mut entries = snapshot
        .directories
        .iter()
        .map(|e| (&e.path, e.object, true))
        .chain(snapshot.files.iter().map(|e| (&e.path, e.object, false)))
        .collect::<Vec<_>>();
    entries.sort_by_key(|(path, _, _)| (path.matches('/').count(), path.as_str()));
    let mut desired = BTreeMap::new();
    for (path, local, directory) in entries {
        if !locals.insert(local.to_string()) {
            return Err(error("duplicate candidate object"));
        }
        let retained = origins.get(&local.to_string());
        let object = if let Some(original) = retained {
            mapped.insert(local.to_string());
            let object = ObjectId::parse(original).map_err(error)?;
            let entry = old_ids
                .get(&object)
                .ok_or_else(|| error("unknown original object"))?;
            if entry.binding.is_directory != directory {
                return Err(error("candidate changed the kind of a retained object"));
            }
            object
        } else {
            let bytes = format!(
                "mesh.fleet-import-object/v1:{}:{}:{}",
                receipt, local, directory
            );
            let hash = Blake3::digest_bytes(bytes.as_bytes());
            let mut id = [0; 16];
            id.copy_from_slice(&hash.as_bytes()[..16]);
            let object = ObjectId::from_bytes(id);
            if old_ids.contains_key(&object) {
                return Err(error("candidate object identity collides with history"));
            }
            object
        };
        if !claimed.insert(object) {
            return Err(error("candidate correspondence is not one to one"));
        }
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        let parent = *ids
            .get(parent)
            .ok_or_else(|| error("candidate parent is absent"))?;
        if !directories.contains(&parent) {
            return Err(error("candidate parent is not a directory"));
        }
        let name = NormalizedName::new(name).map_err(error)?;
        if ids.insert(path.clone(), object).is_some() {
            return Err(error("duplicate candidate path"));
        }
        if directory {
            directories.insert(object);
        }
        desired.insert(
            path.clone(),
            Desired {
                object,
                parent,
                name,
                directory,
                retained: retained.is_some(),
            },
        );
    }
    if mapped.len() != origins.len() {
        return Err(error("correspondence names an absent result"));
    }
    let by_object: BTreeMap<_, _> = desired
        .values()
        .map(|entry| (entry.object, entry))
        .collect();
    let mut removed = old
        .iter()
        .filter(|(_, entry)| {
            by_object.get(&entry.binding.object_id).is_none_or(|next| {
                next.parent != entry.binding.parent_id || next.name != entry.binding.name
            })
        })
        .collect::<Vec<_>>();
    removed.sort_by_key(|(path, _)| std::cmp::Reverse((path.matches('/').count(), path.as_str())));
    let mut operations = Vec::new();
    for (_, entry) in removed {
        operations.push(Operation::UnlinkDirectoryEntry {
            directory_id: entry.binding.parent_id,
            name: entry.binding.name.clone(),
            object_id: entry.binding.object_id,
        });
    }
    let needs_link = |entry: &Desired| {
        old_ids.get(&entry.object).is_none_or(|old| {
            old.binding.parent_id != entry.parent || old.binding.name != entry.name
        })
    };
    let mut folders = desired
        .iter()
        .filter(|(_, e)| e.directory)
        .collect::<Vec<_>>();
    folders.sort_by_key(|(path, _)| (path.matches('/').count(), path.as_str()));
    for (_, entry) in folders {
        if !entry.retained {
            operations.push(Operation::CreateDirectory {
                object_id: entry.object,
            });
        }
        if needs_link(entry) {
            operations.push(Operation::LinkDirectoryEntry {
                directory_id: entry.parent,
                name: entry.name.clone(),
                object_id: entry.object,
                version_id: VersionId::from_bytes([0; 32]),
            });
        }
    }
    let mut files = Vec::new();
    let mut sorted_files = snapshot.files.iter().collect::<Vec<_>>();
    sorted_files.sort_by_key(|file| &file.path);
    for file in sorted_files {
        let entry = &desired[&file.path];
        let previous = old_ids.get(&entry.object).and_then(|entry| entry.file);
        let changed = previous.is_none_or(|(_, digest, executable)| {
            digest != file.content_digest || executable != file.executable
        });
        let version = if changed {
            let bytes = target
                .historical_workspace_file(snapshot.operation, &file.path)
                .map_err(error)?
                .ok_or_else(|| error("candidate file unavailable"))?;
            if bytes.object != file.object
                || bytes.bytes.len() as u64 != file.byte_length
                || bytes.executable != file.executable
                || Blake3::digest_bytes(&bytes.bytes).as_bytes() != file.content_digest.as_bytes()
            {
                return Err(error("candidate content changed"));
            }
            let prepared = PreparedCheckpointFile::from_bytes(
                &bytes.bytes,
                &ChunkingConfig::default(),
                ManifestPagingPolicy::flat(),
            )
            .map_err(error)?;
            let version = VersionId::from_bytes(
                *Blake3::digest_bytes(
                    format!("mesh.fleet-import-version/v1:{}:{}", receipt, entry.object).as_bytes(),
                )
                .as_bytes(),
            );
            if !entry.retained {
                operations.push(Operation::CreateFile {
                    object_id: entry.object,
                });
            }
            operations.push(Operation::WriteFileVersion {
                object_id: entry.object,
                version_id: version,
                parent_versions: previous
                    .into_iter()
                    .map(|(version, _, _)| version)
                    .collect(),
                manifest_id: mesh_operations::ManifestId::from_bytes(
                    *prepared.manifest().id.as_bytes(),
                ),
                portable_metadata: PortableMetadata::new(file.executable),
            });
            files.push(prepared);
            version
        } else {
            previous.ok_or_else(|| error("missing retained version"))?.0
        };
        if needs_link(entry) {
            operations.push(Operation::LinkDirectoryEntry {
                directory_id: entry.parent,
                name: entry.name.clone(),
                object_id: entry.object,
                version_id: version,
            });
        }
    }
    let historical = project
        .prepare_historical_operations(predecessor, actor, &operations)
        .map_err(error)?;
    let mut commitment = b"mesh.fleet-import-plan/v1".to_vec();
    for bytes in std::iter::once(receipt.as_bytes().to_vec())
        .chain(std::iter::once(historical.context().encode().into_bytes()))
        .chain(mesh_operations::encode_operations(&operations))
    {
        commitment.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
        commitment.extend_from_slice(&bytes);
    }
    Ok(PreparedProjectCandidateImport {
        historical,
        files,
        snapshot: snapshot.clone(),
        candidate: receipt,
        digest: RecordDigest::from_bytes(*Blake3::digest_bytes(&commitment).as_bytes()),
    })
}

#[cfg(test)]
#[path = "project_import_tests.rs"]
mod tests;
