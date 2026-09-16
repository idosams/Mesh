//! One fresh-process W5-reduced measurement of the production checkpoint delivery path.
//!
//! The surrounding capture script invokes this binary five times. Each invocation owns a fresh
//! workspace and measures the bytes the second, edited version newly links into CAS. The row is
//! emitted only after reconstructing the edited file byte-for-byte from its manifest and CAS.

use std::error::Error;
use std::fs;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Instant;

use mesh_bench::corpus::content;
use mesh_bench::corpus::plan::{ContentKind, EditOp, Item};
use mesh_bench::corpus::{Generator, LargeBinary, Scale, CANONICAL_SEED, GENERATOR_VERSION};
use mesh_chunking::ChunkingConfig;
use mesh_daemon::{
    save_file_version, FileVersionCheckpointRequest, ManifestPagingPolicy, OpenWorkspace,
    PreparedCheckpointFile,
};
use mesh_operations::{
    ActorId, ActorSequence, CausalParents, HeadDerivation, HeadId, Hlc, ObjectId, PolicyEpoch,
    PortableMetadata, SessionId, Signature, TransitionCommitment, VersionId, WorkspaceId,
};
use mesh_types::{Blake3, ContentDigest as _};

const DELIVERY_CEILING_BYTES: u64 = 4 * 1024 * 1024;

struct CanonicalHead;

impl HeadDerivation for CanonicalHead {
    fn resulting_head(&self, commitment: &TransitionCommitment) -> HeadId {
        HeadId::from_bytes(*Blake3::digest_bytes(&commitment.canonical_bytes()).as_bytes())
    }
}

struct Scratch {
    root: PathBuf,
    cleaned: bool,
}

impl Scratch {
    fn new(sample: u64) -> Result<Self, std::io::Error> {
        let root = std::env::temp_dir().join(format!(
            "mesh-checkpoint-delivery-{}-{sample}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            cleaned: false,
        })
    }

    fn cleanup(&mut self) -> Result<(), std::io::Error> {
        fs::remove_dir_all(&self.root)?;
        if self.root.exists() {
            return Err(std::io::Error::other(format!(
                "scratch root survived cleanup: {}",
                self.root.display()
            )));
        }
        self.cleaned = true;
        Ok(())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

fn request(sequence: u64, version: u8) -> FileVersionCheckpointRequest {
    FileVersionCheckpointRequest::new(
        WorkspaceId::from_bytes([1; 16]),
        ActorId::from_bytes([2; 32]),
        SessionId::from_bytes([3; 16]),
        ActorSequence::new(sequence),
        CausalParents::genesis(),
        HeadId::from_bytes([4; 32]),
        PolicyEpoch::new(5),
        Hlc::new(1_700_000_000_000 + sequence, 0),
        ObjectId::from_bytes([7; 16]),
        VersionId::from_bytes([version; 32]),
        Vec::new(),
        PortableMetadata::new(true),
        Signature::from_bytes([10 + version; 64]),
    )
}

fn apply_edit(base: &[u8], operation: EditOp, stream: u64) -> Vec<u8> {
    let mut replacement = Vec::with_capacity(operation.length() as usize);
    content::write_stream(
        ContentKind::Binary,
        stream,
        operation.length(),
        &mut |chunk| replacement.extend_from_slice(chunk),
    );
    match operation {
        EditOp::Overwrite { offset, length } => {
            let mut edited = base.to_vec();
            let start = usize::try_from(offset).expect("W5 offset fits this host");
            let end = start + usize::try_from(length).expect("W5 length fits this host");
            edited[start..end].copy_from_slice(&replacement);
            edited
        }
        EditOp::Insert { offset, .. } => {
            let at = usize::try_from(offset).expect("W5 offset fits this host");
            let mut edited = Vec::with_capacity(base.len() + replacement.len());
            edited.extend_from_slice(&base[..at]);
            edited.extend_from_slice(&replacement);
            edited.extend_from_slice(&base[at..]);
            edited
        }
        EditOp::Append { .. } => {
            let mut edited = base.to_vec();
            edited.extend_from_slice(&replacement);
            edited
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let total_started = Instant::now();
    let sample = std::env::args()
        .nth(1)
        .ok_or("usage: checkpoint-delivery <sample-number>")?
        .parse::<u64>()?;
    if sample == 0 {
        return Err("sample number must be nonzero".into());
    }

    let generation_started = Instant::now();
    let generator = LargeBinary::new(CANONICAL_SEED, Scale::Reduced);
    let mut base_file = None;
    let mut first_edit = None;
    for item in generator.items() {
        match item {
            Item::File(file) => base_file = Some(file),
            Item::Edit { op, stream, .. } if first_edit.is_none() => {
                first_edit = Some((op, stream));
            }
            _ => {}
        }
    }
    let base_file = base_file.ok_or("W5 emitted no base file")?;
    let (edit, edit_stream) = first_edit.ok_or("W5 emitted no edit")?;
    let base = content::to_vec(&base_file);
    let edited = apply_edit(&base, edit, edit_stream);
    let generation_ms = generation_started.elapsed().as_millis();
    let config = ChunkingConfig::default();
    let paging_policy = ManifestPagingPolicy::page_at_or_above(
        NonZeroUsize::new(256).expect("the fixed candidate-B threshold is nonzero"),
    );
    let mut scratch = Scratch::new(sample)?;
    let mut workspace = OpenWorkspace::open(&scratch.root).map_err(|error| {
        std::io::Error::other(format!("fresh workspace refused to open: {error:?}"))
    })?;
    let cas = mesh_cas::Cas::open(&scratch.root)?;

    let baseline_started = Instant::now();
    let first = save_file_version(
        &mut workspace,
        &cas,
        &base,
        &config,
        paging_policy,
        request(1, 1),
        &CanonicalHead,
    )?;
    let baseline_save_ms = baseline_started.elapsed().as_millis();
    if first.linked_bytes() < base.len() as u64 {
        return Err("the fresh baseline did not link the complete base file".into());
    }
    let edited_started = Instant::now();
    let second = save_file_version(
        &mut workspace,
        &cas,
        &edited,
        &config,
        paging_policy,
        request(2, 2),
        &CanonicalHead,
    )?;
    let edited_save_ms = edited_started.elapsed().as_millis();
    let novel_cas_bytes = second.linked_bytes();
    second
        .physical_manifest_index()
        .ok_or("candidate-B paging did not produce a physical manifest index")?;

    let reconstruction_started = Instant::now();
    let prepared = PreparedCheckpointFile::from_bytes(&edited, &config, paging_policy)?;
    let mut reconstructed = Vec::with_capacity(edited.len());
    for slice in &prepared.manifest().chunks {
        let bytes = cas.read(&mesh_cas::Digest32::from_bytes(*slice.digest.as_bytes()))?;
        if bytes.len() as u64 != slice.byte_length {
            return Err("a CAS chunk length disagreed with the edited manifest".into());
        }
        reconstructed.extend_from_slice(&bytes);
    }
    if reconstructed != edited {
        return Err("the edited W5 file did not reconstruct byte-for-byte".into());
    }
    let reconstruction_ms = reconstruction_started.elapsed().as_millis();
    if novel_cas_bytes == 0 || novel_cas_bytes >= DELIVERY_CEILING_BYTES {
        return Err(format!(
            "edited delivery linked {} bytes; required 0 < bytes < {DELIVERY_CEILING_BYTES}",
            novel_cas_bytes
        )
        .into());
    }

    let (edit_offset, edit_length) = match edit {
        EditOp::Overwrite { offset, length } => (offset, length),
        _ => return Err("the first W5 edit is no longer the 1 KiB overwrite".into()),
    };
    let baseline_staged_objects = first.staged_cas_objects();
    let edited_staged_objects = second.staged_cas_objects();
    let edited_reused_objects = second.reused_cas_objects();
    drop(workspace);
    drop(cas);
    let cleanup_started = Instant::now();
    scratch.cleanup()?;
    let cleanup_ms = cleanup_started.elapsed().as_millis();
    let total_ms = total_started.elapsed().as_millis();
    let row = format!(
        "{{\"workload\":\"W5\",\"scale\":\"reduced\",\"generator\":\"mesh-bench/corpus/W5\",\"generator_version\":\"{GENERATOR_VERSION}\",\"seed\":{CANONICAL_SEED},\"sample\":{sample},\"process_id\":{},\"base_bytes\":{},\"edit\":\"overwrite\",\"edit_offset\":{edit_offset},\"edit_bytes\":{edit_length},\"whole_file_threshold_bytes\":{},\"minimum_chunk_bytes\":{},\"average_chunk_bytes\":{},\"maximum_chunk_bytes\":{},\"physical_paging\":true,\"paging_threshold_references\":256,\"novel_cas_bytes\":{},\"ceiling_bytes\":{DELIVERY_CEILING_BYTES},\"baseline_staged_objects\":{baseline_staged_objects},\"edited_staged_objects\":{edited_staged_objects},\"edited_reused_objects\":{edited_reused_objects},\"generation_ms\":{generation_ms},\"baseline_save_ms\":{baseline_save_ms},\"edited_save_ms\":{edited_save_ms},\"reconstruction_ms\":{reconstruction_ms},\"cleanup_ms\":{cleanup_ms},\"total_ms\":{total_ms},\"roundtrip_checked\":true,\"roundtrip_failures\":0}}",
        std::process::id(),
        base.len(),
        config.whole_file_threshold(),
        config.min_size(),
        config.average_size(),
        config.max_size(),
        novel_cas_bytes,
    );
    println!("{row}");
    Ok(())
}
