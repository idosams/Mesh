//! R3 chunking-policy spike harness — throwaway code, kept only so the numbers can be re-run.
//!
//! Task `01KZC2E6N03KVPK93EESJ15Z4V`. A spike's deliverable is a decision with its evidence and
//! its limits; this file is the evidence-producing half and is deliberately not a workspace
//! member. It is compiled by `run.sh` with `rustc`, linking the *shipping* `mesh-chunking` and
//! `mesh-bench` rlibs, so the policies measured here are the policies that ship and the corpora
//! are the corpora `benchmarks/workloads/manifest.json` publishes digests for. No `Cargo.toml` and
//! no `Cargo.lock` line is written to get that (ADR-0013, ADR-0014).
//!
//! Correctness runs before timing, per the task contract: every policy on every file must
//! reconstruct the original bytes exactly out of a digest-keyed chunk store, or the row carries no
//! timing at all.
//!
//! Output is JSON Lines on stdout; a human summary goes to stderr.

use std::collections::HashMap;
use std::time::Instant;

use mesh_bench::corpus::content;
use mesh_bench::corpus::plan::{EditOp, FileSpec, Item};
use mesh_bench::corpus::{build, Scale, WorkloadId, CANONICAL_SEED};
use mesh_chunking::{Blake3, ChunkStream, ChunkingConfig, ContentDigest, Digest32};

/// `mesh-cas`' measured arrival-journal cost per distinct chunk
/// (`crates/mesh-cas/tests/storage-footprint.rs`, 67 B, budgeted at 80). Read as a literal because
/// this harness has no dependency edge to that crate.
const JOURNAL_BYTES_PER_CHUNK: u64 = 67;

/// One manifest entry: a 32-byte digest plus two 8-byte integers.
const MANIFEST_BYTES_PER_CHUNK: u64 = 48;

/// Git's measured cost for the one-byte edit of `benchmarks/budgets/storage.md` §6 row 3, re-run
/// by `git-baseline.sh` on the same bytes rather than quoted.
const GIT_ROW_THREE_BYTES: u64 = 608;

/// The length of the file in that row.
const ROW_THREE_LENGTH: usize = 8_895;

/// The block size every policy is fed in.
///
/// **Not cosmetic, and the first version of this harness got it wrong.** `ChunkStream::push` marks
/// bytes consumed with a cursor and reclaims the read prefix with `Vec::drain` once the prefix has
/// passed `max_size`, so a caller that hands it a whole 64 MiB file pays one memory move of the
/// remaining buffer per `max_size` bytes — quadratic in the push size, and entirely a property of
/// the call, not of the policy. Measured at 64 MiB under `cdc-1k-4k-32k`: **36.8 MiB/s** pushed in
/// one call against **246 MiB/s** pushed in 1 MiB blocks. Publishing the first number as "the CPU
/// cost of content-defined chunking" would have been a wrong answer to the question this spike
/// exists to settle. `--push-sweep` is the standing evidence; the finding is written up in the ADR.
const PUSH_BLOCK: usize = 1024 * 1024;

// ------------------------------------------------------------------ policies

#[derive(Clone, Copy)]
enum Cut {
    WholeFile,
    Fixed(usize),
    Cdc(ChunkingConfig),
}

struct Policy {
    id: &'static str,
    family: &'static str,
    cut: Cut,
}

fn cdc(threshold: u64, min: usize, avg: usize, max: usize) -> Cut {
    Cut::Cdc(ChunkingConfig::new(threshold, min, avg, max, 2).expect("a valid policy"))
}

fn policies() -> Vec<Policy> {
    vec![
        Policy {
            id: "whole-file",
            family: "whole-file",
            cut: Cut::WholeFile,
        },
        Policy {
            id: "fixed-4k",
            family: "fixed-size",
            cut: Cut::Fixed(4 * 1024),
        },
        Policy {
            id: "fixed-8k",
            family: "fixed-size",
            cut: Cut::Fixed(8 * 1024),
        },
        Policy {
            id: "fixed-64k",
            family: "fixed-size",
            cut: Cut::Fixed(64 * 1024),
        },
        Policy {
            id: "cdc-plan-6.2",
            family: "content-defined",
            cut: Cut::Cdc(ChunkingConfig::plan_defaults()),
        },
        Policy {
            id: "cdc-1k-2k-16k",
            family: "content-defined",
            cut: cdc(1024, 1024, 2048, 16 * 1024),
        },
        Policy {
            id: "cdc-1k-4k-32k",
            family: "content-defined",
            cut: Cut::Cdc(ChunkingConfig::default()),
        },
        Policy {
            id: "cdc-512-4k-32k",
            family: "content-defined",
            cut: cdc(512, 512, 4096, 32 * 1024),
        },
        Policy {
            id: "cdc-2k-8k-64k",
            family: "content-defined",
            cut: cdc(2048, 2048, 8192, 64 * 1024),
        },
        Policy {
            id: "cdc-4k-16k-128k",
            family: "content-defined",
            cut: cdc(4096, 4096, 16 * 1024, 128 * 1024),
        },
        Policy {
            id: "cdc-8k-32k-256k",
            family: "content-defined",
            cut: cdc(8192, 8192, 32 * 1024, 256 * 1024),
        },
        Policy {
            id: "cdc-64k-256k-1m-nothreshold",
            family: "content-defined",
            cut: cdc(0, 64 * 1024, 256 * 1024, 1024 * 1024),
        },
    ]
}

/// The parameters of a policy, as a JSON fragment, so a row is self-describing.
fn parameters_json(policy: &Policy) -> String {
    match policy.cut {
        Cut::WholeFile => "{\"whole_file_threshold\":null}".to_owned(),
        Cut::Fixed(size) => format!("{{\"block_bytes\":{size}}}"),
        Cut::Cdc(config) => format!(
            "{{\"whole_file_threshold\":{},\"min_size\":{},\"average_size\":{},\"max_size\":{},\"normalization\":{}}}",
            config.whole_file_threshold(),
            config.min_size(),
            config.average_size(),
            config.max_size(),
            config.normalization()
        ),
    }
}

/// One chunked file: the ordered `(digest, length)` list, and the bytes keyed by digest.
struct Chunked {
    refs: Vec<(Digest32, u64)>,
}

/// Chunks `data`, handing every chunk's bytes to `store`.
fn chunk_into(cut: Cut, data: &[u8], store: &mut impl FnMut(Digest32, &[u8])) -> Chunked {
    let mut refs = Vec::new();
    match cut {
        Cut::WholeFile => {
            if !data.is_empty() {
                let digest = Blake3::digest_bytes(data);
                store(digest, data);
                refs.push((digest, data.len() as u64));
            }
        }
        Cut::Fixed(size) => {
            for block in data.chunks(size) {
                let digest = Blake3::digest_bytes(block);
                store(digest, block);
                refs.push((digest, block.len() as u64));
            }
        }
        Cut::Cdc(config) => {
            let mut stream = ChunkStream::new(config);
            for block in data.chunks(PUSH_BLOCK) {
                stream.push(block, |reference, bytes| {
                    store(*reference.content_hash(), bytes);
                    refs.push((*reference.content_hash(), reference.length()));
                });
            }
            let _manifest = stream.finish(|reference, bytes| {
                store(*reference.content_hash(), bytes);
                refs.push((*reference.content_hash(), reference.length()));
            });
        }
    }
    Chunked { refs }
}

/// Chunks without retaining bytes — the timing path.
fn chunk_only(cut: Cut, data: &[u8]) -> usize {
    let mut count = 0usize;
    chunk_into(cut, data, &mut |_, _| count += 1);
    count.max(1)
}

// ------------------------------------------------------------- accumulators

/// Everything one (segment, policy) pair accumulates over its files.
#[derive(Default)]
struct Accumulator {
    files: u64,
    content_bytes: u64,
    chunk_refs: u64,
    distinct: HashMap<Digest32, u64>,
    roundtrip_failures: u64,
    roundtrip_checked: u64,
    /// Bytes a holder of the before-state still has to fetch after the edit.
    transfer_bytes: u64,
    transfer_chunks: u64,
    edited_files: u64,
}

impl Accumulator {
    fn distinct_bytes(&self) -> u64 {
        self.distinct.values().copied().sum()
    }

    fn store_total(&self) -> u64 {
        self.distinct_bytes()
            + self.distinct.len() as u64 * JOURNAL_BYTES_PER_CHUNK
            + self.chunk_refs * MANIFEST_BYTES_PER_CHUNK
    }
}

/// Chunks one version of a file, verifies it round-trips, and folds it into `accumulator`.
fn absorb(accumulator: &mut Accumulator, cut: Cut, data: &[u8]) -> Vec<(Digest32, u64)> {
    let mut store: HashMap<Digest32, Vec<u8>> = HashMap::new();
    let chunked = chunk_into(cut, data, &mut |digest, bytes| {
        store.entry(digest).or_insert_with(|| bytes.to_vec());
    });

    // Correctness first: rebuild the file out of the digest-keyed store and compare byte for byte.
    let mut rebuilt: Vec<u8> = Vec::with_capacity(data.len());
    let mut intact = true;
    for (digest, length) in &chunked.refs {
        match store.get(digest) {
            Some(bytes) if bytes.len() as u64 == *length => rebuilt.extend_from_slice(bytes),
            _ => intact = false,
        }
    }
    accumulator.roundtrip_checked += 1;
    if !intact || rebuilt != data {
        accumulator.roundtrip_failures += 1;
    }

    accumulator.files += 1;
    accumulator.content_bytes += data.len() as u64;
    accumulator.chunk_refs += chunked.refs.len() as u64;
    for (digest, length) in &chunked.refs {
        accumulator.distinct.entry(*digest).or_insert(*length);
    }
    chunked.refs
}

/// Folds the edited version in, counting only what the holder of `before` does not already have.
fn absorb_edit(
    accumulator: &mut Accumulator,
    cut: Cut,
    before: &[(Digest32, u64)],
    edited: &[u8],
) {
    let held: std::collections::HashSet<Digest32> = before.iter().map(|(d, _)| *d).collect();
    let mut counted: std::collections::HashSet<Digest32> = std::collections::HashSet::new();
    let after = chunk_into(cut, edited, &mut |_, _| {});
    for (digest, length) in &after.refs {
        if held.contains(digest) || !counted.insert(*digest) {
            continue;
        }
        accumulator.transfer_bytes += *length;
        accumulator.transfer_chunks += 1;
    }
    accumulator.edited_files += 1;
}

// ------------------------------------------------------------------- timing

/// Nanoseconds per sample, sorted, with the percentiles the plan §11 record demands.
struct Timing {
    samples: Vec<u128>,
}

impl Timing {
    fn percentile(&self, fraction: f64) -> u128 {
        if self.samples.is_empty() {
            return 0;
        }
        let rank = (fraction * (self.samples.len() as f64 - 1.0)).round() as usize;
        self.samples[rank.min(self.samples.len() - 1)]
    }
    fn json(&self, bytes: u64) -> String {
        let p50 = self.percentile(0.50);
        let mib_per_s = if p50 == 0 {
            0.0
        } else {
            (bytes as f64 / (1024.0 * 1024.0)) / (p50 as f64 / 1e9)
        };
        format!(
            "\"sample_count\":{},\"ns_p50\":{},\"ns_p95\":{},\"ns_p99\":{},\"ns_min\":{},\"ns_max\":{},\"mib_per_s_at_p50\":{:.1},\"raw_ns\":[{}]",
            self.samples.len(),
            p50,
            self.percentile(0.95),
            self.percentile(0.99),
            self.samples.first().copied().unwrap_or(0),
            self.samples.last().copied().unwrap_or(0),
            mib_per_s,
            self.samples
                .iter()
                .map(u128::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

fn time_segment(cut: Cut, files: &[Vec<u8>], samples: usize) -> Timing {
    let mut readings = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let mut sink = 0usize;
        for file in files {
            sink = sink.wrapping_add(chunk_only(cut, file));
        }
        let elapsed = start.elapsed().as_nanos();
        std::hint::black_box(sink);
        readings.push(elapsed);
    }
    readings.sort_unstable();
    Timing { samples: readings }
}

// ------------------------------------------------------------------ corpora

/// A named group of files inside one workload — plan §10.4's "corpus segment".
struct Segment {
    workload: &'static str,
    scale: &'static str,
    name: String,
    profile: &'static str,
    before: Vec<Vec<u8>>,
    after: Vec<Vec<u8>>,
    edit: &'static str,
}

fn extension_of(path: &str) -> String {
    path.rsplit('.').next().unwrap_or("none").to_owned()
}

/// One byte flipped at the midpoint: the smallest edit a workspace can make, and the one
/// `benchmarks/budgets/storage.md` §6 row 3 lost 14.7x to Git on.
fn one_byte_edit(file: &[u8]) -> Vec<u8> {
    let mut out = file.to_vec();
    if !out.is_empty() {
        let at = out.len() / 2;
        out[at] ^= 0xFF;
    }
    out
}

fn bytes_of(spec: &FileSpec) -> Vec<u8> {
    content::to_vec(spec)
}

/// W1 and W4: every file, grouped by extension, each with a one-byte edit.
fn file_segments(
    workload: WorkloadId,
    scale: Scale,
    scale_name: &'static str,
    workload_name: &'static str,
) -> Vec<Segment> {
    let generator = build(workload, scale, CANONICAL_SEED);
    let mut grouped: std::collections::BTreeMap<String, (Vec<Vec<u8>>, &'static str)> =
        std::collections::BTreeMap::new();
    for item in generator.items() {
        let Item::File(spec) = item else { continue };
        let key = extension_of(&spec.path);
        let profile: &'static str = spec.kind.name();
        let data = bytes_of(&spec);
        grouped
            .entry(key)
            .or_insert_with(|| (Vec::new(), profile))
            .0
            .push(data);
    }
    grouped
        .into_iter()
        .map(|(extension, (files, profile))| {
            let after = files.iter().map(|file| one_byte_edit(file)).collect();
            Segment {
                workload: workload_name,
                scale: scale_name,
                name: extension,
                profile,
                before: files,
                after,
                edit: "one-byte overwrite at midpoint, every file",
            }
        })
        .collect()
}

/// Applies one of W5's described edits to the base file.
fn apply_edit(base: &[u8], op: EditOp, stream: u64) -> Vec<u8> {
    let mut replacement = Vec::new();
    content::write_stream(
        mesh_bench::corpus::plan::ContentKind::Binary,
        stream,
        op.length(),
        &mut |chunk| replacement.extend_from_slice(chunk),
    );
    match op {
        EditOp::Overwrite { offset, length } => {
            let mut out = base.to_vec();
            let at = offset as usize;
            let end = (at + length as usize).min(out.len());
            let take = end - at;
            out[at..end].copy_from_slice(&replacement[..take]);
            out
        }
        EditOp::Insert { offset, .. } => {
            let at = (offset as usize).min(base.len());
            let mut out = Vec::with_capacity(base.len() + replacement.len());
            out.extend_from_slice(&base[..at]);
            out.extend_from_slice(&replacement);
            out.extend_from_slice(&base[at..]);
            out
        }
        EditOp::Append { .. } => {
            let mut out = base.to_vec();
            out.extend_from_slice(&replacement);
            out
        }
    }
}

/// W5: the base file, and one segment per edit shape the generator describes.
fn w5_segments(scale: Scale, scale_name: &'static str) -> Vec<Segment> {
    let generator = build(WorkloadId::W5, scale, CANONICAL_SEED);
    let mut base: Option<Vec<u8>> = None;
    let mut edits: Vec<(EditOp, u64)> = Vec::new();
    for item in generator.items() {
        match item {
            Item::File(spec) => base = Some(bytes_of(&spec)),
            Item::Edit { op, stream, .. } => edits.push((op, stream)),
            _ => {}
        }
    }
    let base = base.expect("W5 has a base file");

    let mut segments = Vec::new();
    let mut scattered: Option<Vec<u8>> = None;
    let mut scattered_count = 0usize;
    for (op, stream) in &edits {
        match op {
            EditOp::Overwrite { length, .. } if *length == 4096 => {
                // The scattered 4 KiB rewrites are one segment: they compose into one new version.
                let current = scattered.take().unwrap_or_else(|| base.clone());
                scattered = Some(apply_edit(&current, *op, *stream));
                scattered_count += 1;
            }
            _ => {
                let name = match op {
                    EditOp::Overwrite { length, .. } => format!("overwrite-{length}b"),
                    EditOp::Insert { length, .. } => format!("insert-{length}b-at-midpoint"),
                    EditOp::Append { length } => format!("append-{length}b"),
                };
                segments.push(Segment {
                    workload: "W5",
                    scale: scale_name,
                    name,
                    profile: "binary",
                    before: vec![base.clone()],
                    after: vec![apply_edit(&base, *op, *stream)],
                    edit: "the edit the W5 generator describes",
                });
            }
        }
    }
    if let Some(after) = scattered {
        segments.push(Segment {
            workload: "W5",
            scale: scale_name,
            name: format!("scattered-4k-rewrites-x{scattered_count}"),
            profile: "binary",
            before: vec![base.clone()],
            after: vec![after],
            edit: "every scattered 4 KiB rewrite applied, measured as one new version",
        });
    }
    segments
}

/// `benchmarks/budgets/storage.md` §6 row 3, as its own segment so the headline ratio is a row and
/// not a footnote.
fn row_three_segment() -> Segment {
    let original = mesh_chunking::testing::source_like_bytes(ROW_THREE_LENGTH, 701);
    let edited = one_byte_edit(&original);
    Segment {
        workload: "storage.md-6.3",
        scale: "exact",
        name: format!("source-like-{ROW_THREE_LENGTH}b"),
        profile: "text",
        before: vec![original],
        after: vec![edited],
        edit: "one byte flipped at the midpoint",
    }
}

// -------------------------------------------------------------------- report

fn measure(segment: &Segment, samples: usize) -> Vec<String> {
    let mut rows = Vec::new();
    for policy in policies() {
        let mut accumulator = Accumulator::default();
        // Determinism check: the byte figures must be identical across three independent passes,
        // or the instrument, not the policy, is what is being measured.
        let mut signatures = Vec::new();
        for pass in 0..3 {
            let mut probe = Accumulator::default();
            for (before, after) in segment.before.iter().zip(segment.after.iter()) {
                let refs = absorb(&mut probe, policy.cut, before);
                absorb_edit(&mut probe, policy.cut, &refs, after);
            }
            signatures.push((
                probe.distinct_bytes(),
                probe.distinct.len() as u64,
                probe.transfer_bytes,
                probe.roundtrip_failures,
            ));
            if pass == 0 {
                accumulator = probe;
            }
        }
        let stable = signatures.windows(2).all(|pair| pair[0] == pair[1]);

        // Correctness gate: a policy that did not reconstruct the bytes gets no timing number.
        let timing = if accumulator.roundtrip_failures == 0 {
            Some(time_segment(policy.cut, &segment.before, samples))
        } else {
            None
        };

        let distinct_bytes = accumulator.distinct_bytes();
        let dedup_permille = if accumulator.content_bytes == 0 {
            0
        } else {
            1000 - (distinct_bytes * 1000 / accumulator.content_bytes)
        };
        let transfer_plus_journal =
            accumulator.transfer_bytes + accumulator.transfer_chunks * JOURNAL_BYTES_PER_CHUNK;
        let transfer_permille = if accumulator.content_bytes == 0 {
            0
        } else {
            transfer_plus_journal * 1000 / accumulator.content_bytes
        };

        let timing_json = match &timing {
            Some(t) => t.json(accumulator.content_bytes),
            None => "\"sample_count\":0,\"ns_p50\":null,\"ns_p95\":null,\"ns_p99\":null,\"ns_min\":null,\"ns_max\":null,\"mib_per_s_at_p50\":null,\"raw_ns\":[]".to_owned(),
        };

        rows.push(format!(
            "{{\"workload\":\"{}\",\"scale\":\"{}\",\"segment\":\"{}\",\"byte_profile\":\"{}\",\"edit\":\"{}\",\"policy\":\"{}\",\"policy_family\":\"{}\",\"parameters\":{},\"files\":{},\"content_bytes\":{},\"chunk_refs\":{},\"distinct_chunks\":{},\"distinct_chunk_bytes\":{},\"store_total_bytes\":{},\"store_amplification_per_mille\":{},\"dedup_per_mille\":{},\"transfer_bytes\":{},\"transfer_chunks\":{},\"transfer_bytes_with_journal\":{},\"transfer_per_mille_of_content\":{},\"roundtrip_checked\":{},\"roundtrip_failures\":{},\"byte_figures_repeatable\":{},{}}}",
            segment.workload,
            segment.scale,
            segment.name,
            segment.profile,
            segment.edit,
            policy.id,
            policy.family,
            parameters_json(&policy),
            accumulator.files,
            accumulator.content_bytes,
            accumulator.chunk_refs,
            accumulator.distinct.len(),
            distinct_bytes,
            accumulator.store_total(),
            if accumulator.content_bytes == 0 {
                0
            } else {
                accumulator.store_total() * 1000 / accumulator.content_bytes
            },
            dedup_permille,
            accumulator.transfer_bytes,
            accumulator.transfer_chunks,
            transfer_plus_journal,
            transfer_permille,
            accumulator.roundtrip_checked,
            accumulator.roundtrip_failures,
            stable,
            timing_json
        ));

        eprintln!(
            "  {:<30} store {:>12} B  1-edit {:>10} B ({:>7.3}% )  chunks {:>7}  rt-fail {}  {}",
            policy.id,
            accumulator.store_total(),
            transfer_plus_journal,
            transfer_plus_journal as f64 * 100.0 / accumulator.content_bytes.max(1) as f64,
            accumulator.chunk_refs,
            accumulator.roundtrip_failures,
            match &timing {
                Some(t) => format!(
                    "{:.0} MiB/s",
                    (accumulator.content_bytes as f64 / 1048576.0)
                        / (t.percentile(0.50) as f64 / 1e9)
                ),
                None => "NO TIMING (correctness failed)".to_owned(),
            }
        );
    }
    rows
}

/// How `ChunkStream`'s throughput depends on the size of the buffer it is handed.
///
/// A property of the API, not of the policy, and the reason [`PUSH_BLOCK`] exists.
fn push_sweep(samples: usize) -> Vec<String> {
    const BYTES: usize = 64 * 1024 * 1024;
    let data = mesh_chunking::testing::pseudorandom_bytes(BYTES, 909);
    let mut rows = Vec::new();
    eprintln!("\n## push-size sweep — {BYTES} B, one file, cdc-1k-4k-32k and cdc-1k-2k-16k");
    for (id, config) in [
        ("cdc-1k-4k-32k", ChunkingConfig::default()),
        (
            "cdc-1k-2k-16k",
            ChunkingConfig::new(1024, 1024, 2048, 16 * 1024, 2).expect("valid"),
        ),
    ] {
        for block in [BYTES, 4 * 1024 * 1024, 1024 * 1024, 256 * 1024, 64 * 1024] {
            let mut readings = Vec::with_capacity(samples);
            for _ in 0..samples {
                let start = Instant::now();
                let mut stream = ChunkStream::new(config);
                let mut count = 0usize;
                for slice in data.chunks(block) {
                    stream.push(slice, |_, _| count += 1);
                }
                let _ = stream.finish(|_, _| count += 1);
                readings.push(start.elapsed().as_nanos());
                std::hint::black_box(count);
            }
            readings.sort_unstable();
            let timing = Timing { samples: readings };
            rows.push(format!(
                "{{\"workload\":\"push-sweep\",\"scale\":\"exact\",\"segment\":\"push-block-{block}\",\"byte_profile\":\"binary\",\"edit\":\"none\",\"policy\":\"{id}\",\"policy_family\":\"content-defined\",\"content_bytes\":{BYTES},\"push_block_bytes\":{block},{}}}",
                timing.json(BYTES as u64)
            ));
            eprintln!(
                "  {id:<16} push {:>9} B  {:>8.1} MiB/s",
                block,
                (BYTES as f64 / 1048576.0) / (timing.percentile(0.50) as f64 / 1e9)
            );
        }
    }
    rows
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| -> Option<String> {
        arguments
            .iter()
            .position(|a| a == flag)
            .and_then(|i| arguments.get(i + 1))
            .cloned()
    };

    if let Some(path) = value("--emit-row-three") {
        let original = mesh_chunking::testing::source_like_bytes(ROW_THREE_LENGTH, 701);
        std::fs::write(format!("{path}/before.txt"), &original).expect("write");
        std::fs::write(format!("{path}/after.txt"), one_byte_edit(&original)).expect("write");
        eprintln!("row-three fixture written to {path} ({ROW_THREE_LENGTH} B)");
        return;
    }

    let samples: usize = value("--samples")
        .and_then(|s| s.parse().ok())
        .unwrap_or(11);
    let w1_scale = value("--w1").unwrap_or_else(|| "reduced".to_owned());
    let w4_scale = value("--w4").unwrap_or_else(|| "smoke".to_owned());
    let w5_scale = value("--w5").unwrap_or_else(|| "reduced".to_owned());

    let parse = |name: &str| Scale::parse(name).expect("full | reduced | smoke");

    let mut segments = vec![row_three_segment()];
    segments.extend(file_segments(
        WorkloadId::W1,
        parse(&w1_scale),
        Box::leak(w1_scale.clone().into_boxed_str()),
        "W1",
    ));
    segments.extend(file_segments(
        WorkloadId::W4,
        parse(&w4_scale),
        Box::leak(w4_scale.clone().into_boxed_str()),
        "W4",
    ));
    segments.extend(w5_segments(
        parse(&w5_scale),
        Box::leak(w5_scale.clone().into_boxed_str()),
    ));

    eprintln!(
        "chunking-policy spike: {} segments x {} policies, {samples} timing samples each",
        segments.len(),
        policies().len()
    );

    let mut total_failures = 0u64;
    for segment in &segments {
        let bytes: usize = segment.before.iter().map(Vec::len).sum();
        eprintln!(
            "\n## {} [{}] {} — {} files, {bytes} B",
            segment.workload,
            segment.scale,
            segment.name,
            segment.before.len()
        );
        for row in measure(segment, samples) {
            if row.contains("\"roundtrip_failures\":0") {
            } else {
                total_failures += 1;
            }
            println!("{row}");
        }
    }
    eprintln!("\ncorrectness: {total_failures} (segment, policy) pairs failed to round-trip");

    for row in push_sweep(samples) {
        println!("{row}");
    }

    // The headline the task is judged by, recomputed here so it cannot drift from the rows above.
    let row = row_three_segment();
    eprintln!(
        "\n## the 14.7x row — one byte in an {ROW_THREE_LENGTH}-byte file, Git = {GIT_ROW_THREE_BYTES} B"
    );
    for policy in policies() {
        let mut accumulator = Accumulator::default();
        let refs = absorb(&mut accumulator, policy.cut, &row.before[0]);
        absorb_edit(&mut accumulator, policy.cut, &refs, &row.after[0]);
        let added =
            accumulator.transfer_bytes + accumulator.transfer_chunks * JOURNAL_BYTES_PER_CHUNK;
        eprintln!(
            "  {:<30} {:>8} B  {:>6.2}x Git",
            policy.id,
            added,
            added as f64 / GIT_ROW_THREE_BYTES as f64
        );
    }
}
