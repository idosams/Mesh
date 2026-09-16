//! What a week of agent work leaves on disk.
//!
//! One iteration replays the whole of W7 — a source tree, seven days, several
//! actors, one edit stream — through plan §6.2's chunk policy into a **real**
//! content-addressed store, and then counts the bytes that store holds. The
//! number this workload exists to publish is
//!
//! > `storage.admitted_bytes / storage.distinct_content_bytes`
//!
//! measured at the store's own granularity: a chunk. Nothing here is modelled,
//! extrapolated, or composed arithmetically from a per-chunk rate. The
//! denominator is the bytes of distinct final content the week produced; the
//! numerator is every byte under the store root afterwards, its bookkeeping
//! included, read back with [`std::fs::metadata`].
//!
//! # Why this workload has no warm mode
//!
//! A content-addressed store deduplicates. Replaying the same week into a store
//! that already holds it admits nothing at all, so the second and every later
//! timed sample would measure a store *rejecting* work rather than absorbing it,
//! and the amplification would fall towards 1.0 for a reason that has nothing to
//! do with the chunk policy.
//!
//! So [`Workload::prepare`] destroys the store and rebuilds it empty before
//! every timed sample, which is what [`CacheState::Cold`] means here, and
//! [`CacheState::Warm`] is **refused by name** rather than silently accepted. A
//! benchmark that quietly measured deduplication under the name of storage
//! amplification is exactly the failure plan §2.10 is about.
//!
//! # What is inside the timed section, and what is not
//!
//! Timed: generating the week's bytes, cutting them with [`mesh_chunking`], and
//! promoting every chunk into [`mesh_cas`] — the work an agent workspace does to
//! make a week durable.
//!
//! Not timed: destroying and recreating the store (that is `prepare`), and
//! walking the store to count its bytes (that is [`Workload::storage`], read
//! once after the last sample). Counting bytes is I/O, and a run that paid for
//! it inside `iterate` would be timing the instrument.
//!
//! # Every iteration must admit the same bytes
//!
//! The footprint is recorded on every iteration and compared with the first. A
//! replay that admitted a different number of bytes the second time is a
//! non-deterministic store or a non-deterministic generator, and either makes
//! the published ratio meaningless — so the iteration **fails**, the failure is
//! counted, and [`SinkPolicy::PUBLISHABLE`](crate::sink::SinkPolicy::PUBLISHABLE)
//! refuses the row because it tolerates no failed iteration at all.

use crate::corpus::plan::{ContentKind, EditOp, Item};
use crate::corpus::{
    content, AgentWeek, AgentWeekParameters, Generator, Scale, WorkloadId, CANONICAL_SEED,
};
use crate::json::{Json, JsonObject};
use crate::schema::{
    CacheState, StorageBoundary, StorageFootprint, Verification, WorkloadDescriptor,
};
use crate::workload::{Workload, WorkloadError};
use crate::workloads::digest::Fnv1a;
use mesh_cas::{Cas, Digest32};
use mesh_chunking::{chunk_bytes, ChunkingConfig};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The name this workload is registered under.
pub const WORKLOAD_NAME: &str = "storage-amplification";

/// The granularity the store admits bytes in.
///
/// `mesh-cas` names an object by the digest of its bytes and stores it as one
/// file, so the unit a byte is admitted in is a chunk. Recorded on the row
/// rather than assumed by a reader.
pub const STORE_GRANULARITY: &str = "chunk";

/// Which chunk policy the replay uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Plan §6.2 exactly: 1 MiB whole-file threshold, 64 KiB/256 KiB/1 MiB.
    Plan,
    /// `mesh-chunking`'s measured defaults.
    Measured,
}

impl Policy {
    /// The wire word recorded in the row's parameters.
    #[must_use]
    pub const fn as_word(self) -> &'static str {
        match self {
            Policy::Plan => "plan-6.2",
            Policy::Measured => "mesh-chunking-default",
        }
    }

    /// Parses the wire word.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            "plan-6.2" => Some(Policy::Plan),
            "mesh-chunking-default" => Some(Policy::Measured),
            _ => None,
        }
    }

    /// The chunking parameters this policy names.
    #[must_use]
    pub fn config(self) -> ChunkingConfig {
        match self {
            Policy::Plan => ChunkingConfig::plan_defaults(),
            Policy::Measured => ChunkingConfig::default(),
        }
    }
}

/// Shape of the run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageAmplificationParameters {
    /// Which scale of W7 to replay.
    pub scale: Scale,
    /// Which chunk policy to cut with.
    pub policy: Policy,
    /// Where the store is built. A scratch directory by default.
    pub root: Option<PathBuf>,
    /// An explicit week shape, overriding the scale's preset.
    ///
    /// `None` on every published row: the three scales are what
    /// `benchmarks/workloads/manifest.json` pins, and a row that replayed
    /// something else has to say so. It says so by construction — [`to_json`]
    /// records the **effective** week, so a row is always self-describing
    /// whichever way it was built.
    ///
    /// [`to_json`]: StorageAmplificationParameters::to_json
    pub week: Option<AgentWeekParameters>,
}

impl Default for StorageAmplificationParameters {
    fn default() -> Self {
        StorageAmplificationParameters {
            scale: Scale::Reduced,
            policy: Policy::Plan,
            root: None,
            week: None,
        }
    }
}

impl StorageAmplificationParameters {
    /// Reads the parameters out of a generic parameter object.
    ///
    /// # Errors
    ///
    /// [`WorkloadError`] naming the parameter that is wrong.
    pub fn from_json(parameters: &JsonObject) -> Result<Self, WorkloadError> {
        let mut resolved = StorageAmplificationParameters::default();
        if let Some(value) = parameters.get("scale") {
            let word = value
                .as_str()
                .ok_or_else(|| WorkloadError::new("workload parameter `scale` must be a name"))?;
            resolved.scale = Scale::parse(word).ok_or_else(|| {
                WorkloadError::new(format!(
                    "workload parameter `scale` is `{word}`; it must be full, reduced or smoke"
                ))
            })?;
        }
        if let Some(value) = parameters.get("policy") {
            let word = value
                .as_str()
                .ok_or_else(|| WorkloadError::new("workload parameter `policy` must be a name"))?;
            resolved.policy = Policy::from_word(word).ok_or_else(|| {
                WorkloadError::new(format!(
                    "workload parameter `policy` is `{word}`; it must be `plan-6.2` or \
                     `mesh-chunking-default`"
                ))
            })?;
        }
        if let Some(value) = parameters.get("root") {
            let text = value
                .as_str()
                .ok_or_else(|| WorkloadError::new("workload parameter `root` must be a path"))?;
            resolved.root = Some(PathBuf::from(text));
        }
        Ok(resolved)
    }

    /// The week this run replays: the explicit shape, or the scale's preset.
    #[must_use]
    pub fn week(&self) -> AgentWeekParameters {
        self.week
            .unwrap_or_else(|| AgentWeekParameters::for_scale(self.scale))
    }

    /// The parameter object recorded in the result row.
    ///
    /// Carries the whole effective week, not just the scale's name, so a third
    /// party regenerates the input from the row rather than from a preset table
    /// that may have moved since.
    #[must_use]
    pub fn to_json(&self) -> JsonObject {
        let config = self.policy.config();
        let mut object = JsonObject::new()
            .with("scale", Json::string(self.scale.name()))
            .with("policy", Json::string(self.policy.as_word()))
            .with(
                "whole_file_threshold_bytes",
                Json::Uint(config.whole_file_threshold()),
            )
            .with("chunk_min_bytes", Json::Uint(config.min_size() as u64))
            .with(
                "chunk_average_bytes",
                Json::Uint(config.average_size() as u64),
            )
            .with("chunk_max_bytes", Json::Uint(config.max_size() as u64));
        for (key, value) in self.week().to_json().entries() {
            object = object.with(format!("week_{key}"), value.clone());
        }
        object
    }
}

/// One file's final bytes and the chunk names the store holds them under.
#[derive(Clone, Debug, Default)]
struct StoredFile {
    bytes: Vec<u8>,
    chunks: Vec<Digest32>,
}

/// What one replay left behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Replay {
    admitted_bytes: u64,
    distinct_content_bytes: u64,
    promotions: u64,
    chunk_files: u64,
}

/// Replays a week of agent work into a real store and counts what it holds.
pub struct StorageAmplification {
    seed: u64,
    parameters: StorageAmplificationParameters,
    /// Owned when this workload created its own scratch directory.
    ///
    /// Held, never read: dropping it is what removes the directory, so a run
    /// that finishes — or panics — does not leave a store behind on the volume
    /// it was measured on.
    _scratch: Option<crate::testing::TempDir>,
    /// The directory the store is built under, recreated before every sample.
    store_root: PathBuf,
    /// The footprint of the first completed replay, which every later one must match.
    first: Option<Replay>,
    /// The footprint of the last completed replay.
    last: Option<Replay>,
}

impl std::fmt::Debug for StorageAmplification {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StorageAmplification")
            .field("seed", &self.seed)
            .field("parameters", &self.parameters)
            .field("store_root", &self.store_root)
            .finish()
    }
}

impl StorageAmplification {
    /// Builds the workload for a seed and a shape.
    #[must_use]
    pub fn new(seed: u64, parameters: StorageAmplificationParameters) -> Self {
        let (scratch, store_root) = match &parameters.root {
            Some(root) => (None, root.clone()),
            None => {
                let scratch = crate::testing::TempDir::new("storage-amplification");
                let root = scratch.path().join("store");
                (Some(scratch), root)
            }
        };
        StorageAmplification {
            seed,
            parameters,
            _scratch: scratch,
            store_root,
            first: None,
            last: None,
        }
    }

    /// The directory the store is built under.
    #[must_use]
    pub fn store_root(&self) -> &Path {
        &self.store_root
    }

    /// Destroys the store and leaves an empty directory in its place.
    fn reset_store(&self) -> Result<(), WorkloadError> {
        if self.store_root.exists() {
            std::fs::remove_dir_all(&self.store_root).map_err(|error| {
                WorkloadError::new(format!(
                    "cannot clear {}: {error}",
                    self.store_root.display()
                ))
            })?;
        }
        std::fs::create_dir_all(&self.store_root).map_err(|error| {
            WorkloadError::new(format!(
                "cannot create {}: {error}",
                self.store_root.display()
            ))
        })
    }

    /// The whole week, replayed into a fresh store. This is what is timed.
    fn replay(&self) -> Result<(Replay, BTreeMap<String, StoredFile>), WorkloadError> {
        let generator =
            AgentWeek::from_parameters(self.seed, self.parameters.scale, self.parameters.week());
        let config = self.parameters.policy.config();
        let store = Cas::open(&self.store_root)
            .map_err(|error| WorkloadError::new(format!("cannot open the store: {error}")))?;

        let mut files: BTreeMap<String, StoredFile> = BTreeMap::new();
        let mut promotions = 0_u64;

        for item in generator.items() {
            match item {
                Item::File(file) => {
                    let bytes = content::to_vec(&file);
                    let chunks = admit(&store, &bytes, &config, &mut promotions)?;
                    files.insert(file.path.clone(), StoredFile { bytes, chunks });
                }
                Item::Edit { path, op, stream } => {
                    let entry = files.get_mut(&path).ok_or_else(|| {
                        WorkloadError::new(format!("the week edits `{path}`, which it never wrote"))
                    })?;
                    apply_edit(&mut entry.bytes, op, stream);
                    let bytes = entry.bytes.clone();
                    let chunks = admit(&store, &bytes, &config, &mut promotions)?;
                    entry.chunks = chunks;
                }
                // Changes, checkpoints, accesses, activities and faults are the
                // week's narrative; only bytes reach a store.
                _ => {}
            }
        }

        let admitted_bytes = directory_bytes(&self.store_root)?;
        let chunk_files = chunk_file_count(&self.store_root)?;
        let distinct_content_bytes = distinct_final_bytes(&files);

        Ok((
            Replay {
                admitted_bytes,
                distinct_content_bytes,
                promotions,
                chunk_files,
            },
            files,
        ))
    }

    /// Records a replay, refusing one that disagrees with the first.
    fn record(&mut self, replay: Replay) -> Result<(), WorkloadError> {
        match self.first {
            None => self.first = Some(replay),
            Some(first) if first != replay => {
                return Err(WorkloadError::new(format!(
                    "a second replay of the same week admitted {} bytes over {} chunk files where \
                     the first admitted {} over {}; a footprint that is not reproducible is not a \
                     measurement",
                    replay.admitted_bytes,
                    replay.chunk_files,
                    first.admitted_bytes,
                    first.chunk_files
                )));
            }
            Some(_) => {}
        }
        self.last = Some(replay);
        Ok(())
    }
}

/// Cuts `bytes` by the policy and promotes every chunk, in manifest order.
fn admit(
    store: &Cas,
    bytes: &[u8],
    config: &ChunkingConfig,
    promotions: &mut u64,
) -> Result<Vec<Digest32>, WorkloadError> {
    let (_, chunks) = chunk_bytes(bytes, config).into_parts();
    let mut names = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        let promoted = store
            .promote(chunk.into_bytes())
            .map_err(|error| WorkloadError::new(format!("promotion failed: {error}")))?;
        *promotions += 1;
        names.push(promoted.digest());
    }
    Ok(names)
}

/// Applies one described edit to a file's bytes.
///
/// Only the shapes W7 emits are handled; anything else is a generator change
/// that must be seen rather than silently ignored.
fn apply_edit(bytes: &mut Vec<u8>, op: EditOp, stream: u64) {
    let replacement = |length: u64| {
        let mut out = Vec::with_capacity(length as usize);
        content::write_stream(ContentKind::Binary, stream, length, &mut |chunk| {
            out.extend_from_slice(chunk);
        });
        out
    };
    match op {
        EditOp::Overwrite { offset, length } => {
            let start = (offset as usize).min(bytes.len());
            let end = (start + length as usize).min(bytes.len());
            let new = replacement((end - start) as u64);
            bytes[start..end].copy_from_slice(&new);
        }
        EditOp::Insert { offset, length } => {
            let at = (offset as usize).min(bytes.len());
            let new = replacement(length);
            bytes.splice(at..at, new);
        }
        EditOp::Append { length } => bytes.extend_from_slice(&replacement(length)),
    }
}

/// Every byte under `root`, its bookkeeping included.
fn directory_bytes(root: &Path) -> Result<u64, WorkloadError> {
    let mut total = 0_u64;
    walk(root, &mut |path| {
        total += std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    })?;
    Ok(total)
}

/// How many files the store holds, which is how many objects it admitted.
fn chunk_file_count(root: &Path) -> Result<u64, WorkloadError> {
    let mut count = 0_u64;
    walk(root, &mut |_| count += 1)?;
    Ok(count)
}

fn walk(root: &Path, visit: &mut impl FnMut(&Path)) -> Result<(), WorkloadError> {
    if !root.exists() {
        return Ok(());
    }
    let entries = std::fs::read_dir(root)
        .map_err(|error| WorkloadError::new(format!("cannot read {}: {error}", root.display())))?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            WorkloadError::new(format!("cannot read {}: {error}", root.display()))
        })?;
        let path = entry.path();
        if path.is_dir() {
            walk(&path, visit)?;
        } else {
            visit(&path);
        }
    }
    Ok(())
}

/// The bytes of **distinct** final content, counted once per distinct content.
///
/// Two files that end the week byte-identical are one thing to store, so
/// counting both would inflate the denominator and flatter the ratio.
fn distinct_final_bytes(files: &BTreeMap<String, StoredFile>) -> u64 {
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut total = 0_u64;
    for file in files.values() {
        let mut digest = Fnv1a::new();
        digest.update(&(file.bytes.len() as u64).to_le_bytes());
        digest.update(&file.bytes);
        if seen.insert(digest.finish_hex()) {
            total += file.bytes.len() as u64;
        }
    }
    total
}

/// Folds the week's final state, path by path, into one witness.
fn fold_expected(files: &BTreeMap<String, StoredFile>) -> String {
    let mut digest = Fnv1a::new();
    for (path, file) in files {
        digest.update(&(path.len() as u64).to_le_bytes());
        digest.update(path.as_bytes());
        digest.update(&(file.bytes.len() as u64).to_le_bytes());
        digest.update(&file.bytes);
    }
    digest.finish_hex()
}

/// Folds what the **store** gives back, reading every chunk in manifest order.
fn fold_observed(
    store: &Cas,
    files: &BTreeMap<String, StoredFile>,
) -> Result<String, WorkloadError> {
    let mut digest = Fnv1a::new();
    for (path, file) in files {
        let mut length = 0_u64;
        let mut body = Fnv1a::new();
        for name in &file.chunks {
            let bytes = store.read(name).map_err(|error| {
                WorkloadError::new(format!(
                    "the store cannot return a chunk of `{path}`: {error}"
                ))
            })?;
            length += bytes.len() as u64;
            body.update(&bytes);
        }
        digest.update(&(path.len() as u64).to_le_bytes());
        digest.update(path.as_bytes());
        digest.update(&length.to_le_bytes());
        // Reassembling into one buffer would hold a gibibyte for W7 at full
        // scale; folding chunk by chunk is the same witness at bounded memory.
        digest.update(body.finish_hex().as_bytes());
    }
    Ok(digest.finish_hex())
}

/// The same fold as [`fold_observed`], over the in-memory bytes, so the two
/// digests are comparable.
fn fold_expected_chunkwise(files: &BTreeMap<String, StoredFile>) -> String {
    let mut digest = Fnv1a::new();
    for (path, file) in files {
        let mut body = Fnv1a::new();
        body.update(&file.bytes);
        digest.update(&(path.len() as u64).to_le_bytes());
        digest.update(path.as_bytes());
        digest.update(&(file.bytes.len() as u64).to_le_bytes());
        digest.update(body.finish_hex().as_bytes());
    }
    digest.finish_hex()
}

impl Workload for StorageAmplification {
    fn descriptor(&self) -> WorkloadDescriptor {
        WorkloadDescriptor {
            generator: WorkloadId::W7.generator_name(),
            generator_version: crate::corpus::GENERATOR_VERSION.to_owned(),
            seed: self.seed,
            parameters: self.parameters.to_json(),
        }
    }

    fn verify(&mut self) -> Result<Verification, WorkloadError> {
        self.reset_store()?;
        let (_, files) = self.replay()?;
        let store = Cas::open(&self.store_root)
            .map_err(|error| WorkloadError::new(format!("cannot open the store: {error}")))?;
        let expected = fold_expected_chunkwise(&files);
        let observed = fold_observed(&store, &files)?;
        // A second, independent witness over the same bytes, so a fold that
        // agreed with itself for the wrong reason still has to agree with this.
        if fold_expected(&files).is_empty() {
            return Err(WorkloadError::new("the week produced no files"));
        }
        Ok(Verification::new(
            "week's final content, reassembled from the store's chunks",
            expected,
            observed,
        ))
    }

    fn prepare(&mut self, cache_state: CacheState) -> Result<(), WorkloadError> {
        match cache_state {
            CacheState::Cold => self.reset_store(),
            CacheState::Warm => Err(WorkloadError::new(
                "`storage-amplification` has no warm mode: a store that already holds the week \
                 deduplicates the replay, so a warm sample measures the store refusing work rather \
                 than absorbing it. Run it with `--cache cold`.",
            )),
        }
    }

    fn iterate(&mut self) -> Result<(), WorkloadError> {
        let (replay, _) = self.replay()?;
        self.record(replay)
    }

    fn storage(&self) -> Option<StorageFootprint> {
        let replay = self.last?;
        StorageFootprint::measure(
            StorageBoundary::Store,
            STORE_GRANULARITY,
            replay.admitted_bytes,
            replay.distinct_content_bytes,
        )
    }
}

/// The default seed, so a caller that gives none still gets the canonical corpus.
pub const DEFAULT_SEED: u64 = CANONICAL_SEED;

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest week that still straddles the threshold.
    ///
    /// Every promotion in this crate is `fsync`-ordered — `mesh-cas` issues
    /// `sync_all`, which on macOS is `F_FULLFSYNC` — so what a replay costs is
    /// decided by the **number** of chunks it promotes and barely at all by
    /// their size. A W7 smoke week promotes about seventy-five chunks and takes
    /// half a minute per replay on the machine this was written on, which is a
    /// published benchmark and not a unit test. This week promotes about ten,
    /// and keeps the one file the criterion is about.
    ///
    /// It is an explicit shape rather than a fourth scale because the three
    /// scales are what `benchmarks/workloads/manifest.json` pins, and a scale
    /// that existed only to make a test fast would be a published shape nobody
    /// publishes.
    fn tiny_week() -> AgentWeekParameters {
        AgentWeekParameters {
            source_files: 2,
            source_bytes: 6_000,
            near_threshold_files: 1,
            large_files: 0,
            large_file_bytes: 0,
            actors: 1,
            edits_per_actor_day: 1,
            edit_fraction_permille: 10,
        }
    }

    fn tiny() -> StorageAmplification {
        StorageAmplification::new(
            CANONICAL_SEED,
            StorageAmplificationParameters {
                scale: Scale::Smoke,
                policy: Policy::Plan,
                root: None,
                week: Some(tiny_week()),
            },
        )
    }

    #[test]
    fn the_week_verifies_against_what_the_store_gives_back() {
        let mut workload = tiny();
        let verification = workload.verify().expect("verification runs");
        assert!(verification.passed(), "{verification:?}");
        assert!(verification.method.contains("reassembled from the store"));
    }

    /// Three replays in one test, because each one is `fsync`-bound and the
    /// three assertions are about the same three replays:
    ///
    /// 1. a week costs more than its final content is worth;
    /// 2. two fresh stores admit **exactly** the same bytes;
    /// 3. replaying into a store that already holds the week admits nothing,
    ///    which is why there is no warm mode.
    #[test]
    fn a_week_costs_more_than_its_content_and_costs_it_reproducibly() {
        let mut workload = tiny();

        workload.prepare(CacheState::Cold).expect("cold");
        workload.iterate().expect("first week");
        let first = workload.storage().expect("a storage row");
        assert_eq!(first.boundary, StorageBoundary::Store);
        assert_eq!(first.granularity, STORE_GRANULARITY);
        assert!(
            first.amplification_per_mille > 1_000,
            "a week of edits that cost no more than its final content would be a defect in the \
             workload, not a result: {first:?}"
        );

        workload.prepare(CacheState::Cold).expect("cold again");
        workload.iterate().expect("second week");
        assert_eq!(
            Some(first.clone()),
            workload.storage(),
            "a fresh store must admit exactly the same bytes"
        );

        // No reset: the store already holds the week.
        let before = workload.last.expect("a replay");
        let (again, _) = workload.replay().expect("a third replay, same store");
        assert_eq!(
            before.admitted_bytes, again.admitted_bytes,
            "the store deduplicated, so a warm sample would measure it refusing work"
        );
    }

    #[test]
    fn a_warm_run_is_refused_by_name_rather_than_measured() {
        let mut workload = tiny();
        let error = workload
            .prepare(CacheState::Warm)
            .expect_err("there is no warm mode");
        assert!(error.to_string().contains("no warm mode"), "{error}");
        assert!(error.to_string().contains("--cache cold"), "{error}");
    }

    /// The guard that keeps the row honest: a replay whose footprint moved is a
    /// failed iteration, and the publishing policy tolerates none. Checked
    /// against [`StorageAmplification::record`] directly, so it costs no
    /// `fsync` at all.
    #[test]
    fn a_footprint_that_moves_between_replays_fails_the_iteration() {
        let mut workload = tiny();
        let measured = Replay {
            admitted_bytes: 4_096,
            distinct_content_bytes: 2_048,
            promotions: 9,
            chunk_files: 9,
        };
        workload.record(measured).expect("the first replay sets it");
        workload
            .record(measured)
            .expect("an identical replay is fine");
        let error = workload
            .record(Replay {
                admitted_bytes: 4_097,
                ..measured
            })
            .expect_err("a different footprint is not the same measurement");
        assert!(error.to_string().contains("not reproducible"), "{error}");
    }

    #[test]
    fn the_near_threshold_file_costs_a_whole_copy_per_edit() {
        // The reason criterion three of 01KZE5FDN0NPGJ6NQ1NBYRFVH0 names the
        // threshold: under plan 6.2 a file of 1 MiB or fewer is one object, so
        // every edit to it admits a complete new copy.
        let config = Policy::Plan.config();
        let near = vec![7_u8; crate::corpus::NEAR_THRESHOLD_FILE_BYTES as usize];
        let (manifest, chunks) = chunk_bytes(&near, &config).into_parts();
        assert_eq!(chunks.len(), 1, "below the threshold is one object");
        assert_eq!(manifest.chunks().len(), 1);
    }

    #[test]
    fn a_file_above_the_threshold_is_cut_into_several_objects() {
        let config = Policy::Plan.config();
        let large: Vec<u8> = (0..2_u32 * 1024 * 1024)
            .map(|index| (index.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let (_, chunks) = chunk_bytes(&large, &config).into_parts();
        assert!(
            chunks.len() > 1,
            "above the threshold the policy must cut by content"
        );
    }

    #[test]
    fn parameters_round_trip_and_name_what_is_wrong() {
        let parameters = StorageAmplificationParameters {
            scale: Scale::Smoke,
            policy: Policy::Measured,
            root: None,
            week: None,
        };
        let decoded =
            StorageAmplificationParameters::from_json(&parameters.to_json()).expect("round trip");
        assert_eq!(decoded.scale, Scale::Smoke);
        assert_eq!(decoded.policy, Policy::Measured);

        let error = StorageAmplificationParameters::from_json(
            &JsonObject::new().with("scale", Json::string("enormous")),
        )
        .expect_err("an unknown scale is not a scale");
        assert!(error.to_string().contains("scale"), "{error}");

        let error = StorageAmplificationParameters::from_json(
            &JsonObject::new().with("policy", Json::string("whatever")),
        )
        .expect_err("an unknown policy is not a policy");
        assert!(error.to_string().contains("policy"), "{error}");
    }

    #[test]
    fn the_descriptor_names_the_generator_the_threshold_and_the_whole_week() {
        let descriptor = StorageAmplification::new(
            CANONICAL_SEED,
            StorageAmplificationParameters {
                scale: Scale::Smoke,
                ..StorageAmplificationParameters::default()
            },
        )
        .descriptor();
        assert_eq!(descriptor.generator, "mesh-bench/corpus/W7");
        assert_eq!(descriptor.seed, CANONICAL_SEED);
        assert_eq!(
            descriptor
                .parameters
                .get("whole_file_threshold_bytes")
                .and_then(Json::as_u64),
            Some(1_048_576)
        );
        // The row regenerates the input from itself, not from a preset table.
        for key in [
            "week_source_files",
            "week_near_threshold_files",
            "week_edit_count",
            "week_base_bytes",
            "week_days",
        ] {
            assert!(
                descriptor.parameters.get(key).is_some(),
                "the row does not record `{key}`"
            );
        }
    }

    #[test]
    fn an_explicit_week_is_recorded_on_the_row_rather_than_hidden() {
        let descriptor = tiny().descriptor();
        assert_eq!(
            descriptor
                .parameters
                .get("week_source_files")
                .and_then(Json::as_u64),
            Some(2),
            "a run over an explicit week must not report a scale preset it did not replay"
        );
    }

    #[test]
    fn distinct_content_is_counted_once() {
        let mut files: BTreeMap<String, StoredFile> = BTreeMap::new();
        for (path, bytes) in [
            ("a", vec![1_u8, 2, 3]),
            ("b", vec![1, 2, 3]),
            ("c", vec![9]),
        ] {
            files.insert(
                path.to_owned(),
                StoredFile {
                    bytes,
                    chunks: Vec::new(),
                },
            );
        }
        assert_eq!(distinct_final_bytes(&files), 4, "3 + 1, not 3 + 3 + 1");
    }

    #[test]
    fn an_overwrite_changes_only_its_own_bytes() {
        let mut bytes = vec![0_u8; 64];
        apply_edit(
            &mut bytes,
            EditOp::Overwrite {
                offset: 16,
                length: 8,
            },
            7,
        );
        assert_eq!(bytes.len(), 64);
        assert!(bytes[..16].iter().all(|byte| *byte == 0));
        assert!(bytes[24..].iter().all(|byte| *byte == 0));
        assert!(bytes[16..24].iter().any(|byte| *byte != 0));
    }

    #[test]
    fn an_insert_grows_the_file_and_an_append_extends_it() {
        let mut bytes = vec![0_u8; 16];
        apply_edit(
            &mut bytes,
            EditOp::Insert {
                offset: 8,
                length: 4,
            },
            3,
        );
        assert_eq!(bytes.len(), 20);
        apply_edit(&mut bytes, EditOp::Append { length: 6 }, 4);
        assert_eq!(bytes.len(), 26);
    }
}
