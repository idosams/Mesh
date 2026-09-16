//! Demo-only composition root for turning one captured file into a durable Mesh checkpoint.
//!
//! The raw FUSE helper owns the kernel request loop. This example owns the storage seam: content
//! is promoted first, then one manifest and one atomic ChangeSet are appended to `records.mesh`.
//! It is intentionally narrow (one fresh file in one fresh workspace), so it cannot be mistaken
//! for the product checkpoint policy whose meaningful-settling parameter is not yet ratified.

use std::env;
use std::error::Error;
use std::fs;
use std::path::Path;

use mesh_operations::{
    ActorId, ActorSequence, CausalParents, ChangeSetDraft, HeadDerivation, HeadId, Hlc, ManifestId,
    NormalizedName, ObjectId, Operation, PolicyEpoch, PortableMetadata, SessionId, Signature,
    TransitionCommitment, VersionId, WorkspaceId,
};
use mesh_store::{
    journal_records, Checkpoint, ChunkSlice, EntityUuid, ManifestRecord, OperationRecord,
    RecordDigest,
};
use mesh_types::{canonical_digest, Blake3, ChunkRef, ContentDigest, Digest32, FileManifest};

const ROOT_ID: [u8; 16] = [0x10; 16];
const FILE_ID: [u8; 16] = [0x20; 16];
const WORKSPACE_ID: [u8; 16] = [0x30; 16];
const SESSION_ID: [u8; 16] = [0x40; 16];

struct CanonicalHead;

impl HeadDerivation for CanonicalHead {
    fn resulting_head(&self, commitment: &TransitionCommitment) -> HeadId {
        let digest = Blake3::digest_bytes(&commitment.canonical_bytes());
        HeadId::from_bytes(*digest.as_bytes())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args().skip(1);
    let workspace = arguments.next().ok_or("missing workspace path")?;
    let captured = arguments.next().ok_or("missing captured file path")?;
    let actor_text = arguments.next().ok_or("missing actor id")?;
    let entry_name = arguments.next().unwrap_or_else(|| "notes.txt".to_owned());
    if arguments.next().is_some() {
        return Err("usage: capture-checkpoint WORKSPACE CAPTURE ACTOR_HEX [ENTRY_NAME]".into());
    }

    let before = mesh_daemon::OpenWorkspace::open(Path::new(&workspace))
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    if before.boundary().records != 0 {
        return Err("the demo checkpoint accepts only a fresh workspace".into());
    }
    let storage = before
        .database_file()
        .parent()
        .ok_or("the demo workspace has no private storage directory")?
        .to_path_buf();
    drop(before);

    let actor = ActorId::parse(&actor_text)?;
    let bytes = fs::read(captured)?;
    let cas = mesh_cas::Cas::open(&storage)?;
    let promoted_content = cas.promote(bytes.clone())?;
    let content = Digest32::from_bytes(*promoted_content.digest().as_bytes());

    let manifest = FileManifest::new(
        bytes.len() as u64,
        content,
        vec![ChunkRef::new(content, 0, bytes.len() as u64)],
    );
    let manifest_digest = canonical_digest::<Blake3, _>(&manifest);
    let manifest_record = ManifestRecord {
        id: RecordDigest::from_bytes(*manifest_digest.as_bytes()),
        byte_length: bytes.len() as u64,
        content_digest: RecordDigest::from_bytes(*content.as_bytes()),
        chunks: vec![ChunkSlice {
            digest: RecordDigest::from_bytes(*content.as_bytes()),
            byte_offset: 0,
            byte_length: bytes.len() as u64,
        }],
    };

    let mut version_preimage = b"mesh.local-demo.file-version\0".to_vec();
    version_preimage.extend_from_slice(manifest_digest.as_bytes());
    let version_digest = Blake3::digest_bytes(&version_preimage);
    let version = VersionId::from_bytes(*version_digest.as_bytes());
    let object = ObjectId::from_bytes(FILE_ID);
    let operations = vec![
        Operation::CreateFile { object_id: object },
        Operation::WriteFileVersion {
            object_id: object,
            version_id: version,
            parent_versions: Vec::new(),
            manifest_id: ManifestId::from_bytes(*manifest_digest.as_bytes()),
            portable_metadata: PortableMetadata::default(),
        },
        Operation::LinkDirectoryEntry {
            directory_id: ObjectId::from_bytes(ROOT_ID),
            name: NormalizedName::new(entry_name)?,
            object_id: object,
            version_id: version,
        },
    ];
    let changeset = ChangeSetDraft::new(
        WorkspaceId::from_bytes(WORKSPACE_ID),
        actor,
        SessionId::from_bytes(SESSION_ID),
        ActorSequence::new(1),
        Hlc::new(1, 0),
    )
    .causal_parents(CausalParents::genesis())
    .base_head(HeadId::from_bytes([0; 32]))
    .policy_epoch(PolicyEpoch::new(1))
    .seal(operations, &CanonicalHead, Signature::from_bytes([0; 64]));
    let payload = cas.promote(mesh_operations::encode_canonical(&changeset))?;
    let payload_digest = RecordDigest::from_bytes(*payload.digest().as_bytes());
    let operation_record = OperationRecord {
        id: payload_digest,
        actor: RecordDigest::from_bytes(*actor.as_bytes()),
        actor_sequence: 1,
        hlc_millis: 1,
        hlc_counter: 0,
        policy_epoch: 1,
        session: EntityUuid::from_bytes(SESSION_ID),
        payload_digest,
        parents: Vec::new(),
    };
    let target = operation_record.id;

    let checkpoint = Checkpoint {
        manifests: vec![manifest_record],
        operations: vec![operation_record],
        ..Checkpoint::default()
    };
    let mut journal =
        mesh_daemon::workspace::RecordFile::open(&storage.join(mesh_daemon::RECORD_FILE_NAME))?;
    journal_records(&mut journal, checkpoint.records().iter())?;

    let after = mesh_daemon::OpenWorkspace::open(Path::new(&workspace))
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let entry = after
        .entries()
        .iter()
        .map(|entry| entry.path())
        .collect::<Vec<_>>()
        .join(",");
    println!(
        "checkpoint: records={} operations={} manifests={} actor={} entry={} target={} digest={}",
        after.boundary().records,
        after.operations(),
        after.manifests(),
        actor,
        entry,
        target,
        after.digest()
    );
    if after.boundary().records != 2
        || after.operations() != 1
        || after.manifests() != 1
        || !after.names_answered()
        || after.entries().len() != 1
    {
        return Err("the durable checkpoint did not materialize completely".into());
    }
    Ok(())
}
