//! The corpus benchmark the chunking parameters were chosen from.
//!
//! ```text
//! cargo bench -p mesh-chunking --bench chunking
//! cargo bench -p mesh-chunking --bench chunking -- --gibibyte
//! ```
//!
//! # What it measures, and why these three columns
//!
//! Three policies compete over the same corpus — **whole-file**, **fixed-size** and
//! **content-defined** — on the two numbers that actually decide the policy:
//!
//! * **stored bytes**, which is what a workspace costs on disk. It is *not* the sum of chunk
//!   lengths. `mesh-cas` writes 67 bytes of arrival journal per chunk (its own
//!   `storage-footprint` test holds that against a budget of 80), so a policy that cuts smaller
//!   pays for it here. That per-chunk cost is what makes this a tradeoff rather than a slider.
//! * **incremental bytes**, which is what an edit costs to replicate. One byte is changed in the
//!   middle of every file and the new chunks are added up.
//!
//! CPU is reported as MiB/s over the same corpus, because a policy that wins on bytes and loses an
//! order of magnitude on throughput is not a win.
//!
//! # Why the benchmark is in this crate and not in `mesh-bench`
//!
//! Task `01KZC2BSFS40V3ZA6WR88KNZZ5` names `cargo bench -p mesh-bench --bench chunking`. Putting it
//! there needs `mesh-bench` to declare a dev-dependency on `mesh-chunking`, and that rewrites
//! `Cargo.lock` — governance surface this lane may not write. Measured, not assumed: adding the
//! edge produces `dependencies = ["mesh-chunking"]` under `[[package]] name = "mesh-bench"`. The
//! benchmark is therefore the same benchmark under a command one word different, and the
//! divergence is recorded in the pull request rather than papered over.
//!
//! `harness = false`: this target owns its `main`, so there is no benchmark framework between the
//! clock and the workload, which is the same choice `mesh-bench`'s own bench makes.

use std::time::Instant;

use mesh_chunking::testing::{overwrite_at, pseudorandom_bytes, source_like_bytes};
use mesh_chunking::{Blake3, ChunkStream, ChunkingConfig, ContentDigest, Digest32, FileManifest};

/// `mesh-cas`' measured arrival-journal cost per chunk, from
/// `crates/mesh-cas/tests/storage-footprint.rs`. Read as a constant here because this crate has no
/// dependency edge to that one; if the store's per-chunk cost changes, this number is what has to
/// change with it.
const JOURNAL_BYTES_PER_CHUNK: u64 = 67;

/// One manifest entry costs 32 bytes of digest plus two 8-byte integers.
const MANIFEST_BYTES_PER_CHUNK: u64 = 48;

fn main() {
    let arguments: Vec<String> = std::env::args()
        .skip(1)
        .filter(|argument| argument != "--bench")
        .collect();
    let gibibyte = arguments.iter().any(|argument| argument == "--gibibyte");

    println!("mesh-chunking — plan §6.2 chunk policy, measured\n");

    storage_budget_row_three();
    println!();
    source_tree_corpus();
    println!();
    large_binary_corpus();
    println!();
    throughput();

    if gibibyte {
        println!();
        gibibyte_workload();
    } else {
        println!(
            "\n(the one-gibibyte W5 row is skipped; add `-- --gibibyte` for it, or run \
             MESH_CHUNKING_W5_BYTES=1073741824 cargo nextest run -p mesh-chunking --test \
             transfer_budget)"
        );
    }
}

/// One policy under test.
struct Policy {
    name: &'static str,
    config: Option<ChunkingConfig>,
    fixed_size: Option<usize>,
}

fn policies() -> Vec<Policy> {
    vec![
        Policy {
            name: "whole-file",
            config: None,
            fixed_size: None,
        },
        Policy {
            name: "fixed 8 KiB",
            config: None,
            fixed_size: Some(8 * 1024),
        },
        Policy {
            name: "fixed 64 KiB",
            config: None,
            fixed_size: Some(64 * 1024),
        },
        Policy {
            name: "plan §6.2 (1 MiB / 64 K / 256 K / 1 M)",
            config: Some(ChunkingConfig::plan_defaults()),
            fixed_size: None,
        },
        Policy {
            name: "CDC 2 K / 8 K / 64 K",
            config: Some(ChunkingConfig::new(2048, 2048, 8192, 65536, 2).expect("valid")),
            fixed_size: None,
        },
        Policy {
            name: "CDC 1 K / 4 K / 32 K  [default]",
            config: Some(ChunkingConfig::default()),
            fixed_size: None,
        },
        Policy {
            name: "CDC 1 K / 2 K / 16 K",
            config: Some(ChunkingConfig::new(1024, 1024, 2048, 16384, 2).expect("valid")),
            fixed_size: None,
        },
        Policy {
            name: "CDC 512 / 4 K / 32 K",
            config: Some(ChunkingConfig::new(512, 512, 4096, 32768, 2).expect("valid")),
            fixed_size: None,
        },
        Policy {
            name: "CDC 4 K / 16 K / 128 K",
            config: Some(ChunkingConfig::new(4096, 4096, 16384, 131_072, 2).expect("valid")),
            fixed_size: None,
        },
        Policy {
            name: "CDC 8 K / 32 K / 256 K",
            config: Some(ChunkingConfig::new(8192, 8192, 32768, 262_144, 2).expect("valid")),
            fixed_size: None,
        },
    ]
}

/// Chunk `data` under `policy` into `(digest, length)` pairs.
fn apply(policy: &Policy, data: &[u8]) -> Vec<(Digest32, u64)> {
    if let Some(size) = policy.fixed_size {
        return data
            .chunks(size)
            .map(|block| (Blake3::digest_bytes(block), block.len() as u64))
            .collect();
    }
    let Some(config) = policy.config else {
        if data.is_empty() {
            return Vec::new();
        }
        return vec![(Blake3::digest_bytes(data), data.len() as u64)];
    };
    let mut stream = ChunkStream::new(config);
    let mut out = Vec::new();
    stream.push(data, |chunk_ref, _| {
        out.push((*chunk_ref.content_hash(), chunk_ref.length()));
    });
    let _manifest: FileManifest = stream.finish(|chunk_ref, _| {
        out.push((*chunk_ref.content_hash(), chunk_ref.length()));
    });
    out
}

/// Distinct chunk bytes, plus the per-chunk store and manifest overhead.
fn stored_bytes(files: &[Vec<(Digest32, u64)>]) -> (u64, u64) {
    let mut seen: Vec<(Digest32, u64)> = Vec::new();
    let mut manifest_entries = 0u64;
    for file in files {
        manifest_entries += file.len() as u64;
        for entry in file {
            if !seen.iter().any(|(digest, _)| digest == &entry.0) {
                seen.push(*entry);
            }
        }
    }
    let content: u64 = seen.iter().map(|(_, length)| *length).sum();
    let overhead =
        seen.len() as u64 * JOURNAL_BYTES_PER_CHUNK + manifest_entries * MANIFEST_BYTES_PER_CHUNK;
    (content, overhead)
}

/// The bytes of `after` a holder of `before` still needs.
fn incremental(before: &[Vec<(Digest32, u64)>], after: &[Vec<(Digest32, u64)>]) -> u64 {
    let mut held: Vec<Digest32> = Vec::new();
    for file in before {
        for (digest, _) in file {
            if !held.contains(digest) {
                held.push(*digest);
            }
        }
    }
    let mut total = 0u64;
    let mut counted: Vec<Digest32> = Vec::new();
    for file in after {
        for (digest, length) in file {
            if held.contains(digest) || counted.contains(digest) {
                continue;
            }
            counted.push(*digest);
            total += *length;
        }
    }
    total
}

/// A corpus of source-like files with a realistic size distribution.
fn source_tree() -> Vec<Vec<u8>> {
    // 120 files: mostly small, a long tail, one large generated file. The distribution is what
    // makes the whole-file threshold matter — plan §6.2's 1 MiB threshold catches all of these.
    let sizes: [usize; 12] = [
        512, 1_024, 2_048, 4_096, 8_192, 16_384, 32_768, 65_536, 131_072, 262_144, 524_288,
        1_048_576,
    ];
    let mut files = Vec::new();
    for (index, size) in sizes.iter().cycle().take(120).enumerate() {
        files.push(source_like_bytes(*size, 400 + index as u64));
    }
    files
}

/// Every file with one byte changed in the middle: the smallest possible edit.
fn one_byte_edit(files: &[Vec<u8>]) -> Vec<Vec<u8>> {
    files
        .iter()
        .map(|file| {
            let at = file.len() / 2;
            let replacement = [file[at] ^ 0xFF];
            overwrite_at(file, at, &replacement)
        })
        .collect()
}

fn report(title: &str, files: &[Vec<u8>], edited: &[Vec<u8>]) {
    let total: usize = files.iter().map(Vec::len).sum();
    println!("## {title} — {} files, {total} content bytes", files.len());
    println!(
        "{:<40} {:>12} {:>12} {:>10} {:>14} {:>10}",
        "policy", "stored B", "overhead B", "chunks", "1-byte edit B", "vs file"
    );

    for policy in policies() {
        let before: Vec<_> = files.iter().map(|file| apply(&policy, file)).collect();
        let after: Vec<_> = edited.iter().map(|file| apply(&policy, file)).collect();
        let (content, overhead) = stored_bytes(&before);
        let chunks: usize = before.iter().map(Vec::len).sum();
        let delta = incremental(&before, &after);
        println!(
            "{:<40} {:>12} {:>12} {:>10} {:>14} {:>9.2}%",
            policy.name,
            content,
            overhead,
            chunks,
            delta,
            delta as f64 * 100.0 / total as f64
        );
    }
}

/// The row this task was opened for.
///
/// `benchmarks/budgets/storage.md` §6 measures *"a one-byte edit to an 8,895-byte file"* at
/// **+8,962 bytes** for Mesh against **+608 bytes** for Git — 14.7x — and names this task as the
/// owner of both causes. The whole-file column below reproduces 8,962 from first principles
/// (8,895 content bytes plus one 67-byte arrival-journal record), which is what makes the CDC
/// column comparable to a number measured by somebody else on another day.
///
/// The file is synthetic source-shaped content of exactly that length, not the original file,
/// which nothing here has. The comparison is therefore *like for like on size and shape*, and is
/// not a re-measurement of that specific file. Stated because the difference matters: a chunker's
/// answer depends on the content, and a claim to have re-run a measurement that was in fact
/// approximated would be exactly the kind of overclaim §7 of that page exists to prevent.
fn storage_budget_row_three() {
    const LENGTH: usize = 8_895;
    const GIT_BYTES: u64 = 608;

    println!(
        "## benchmarks/budgets/storage.md §6 row 3 — one byte edited in an {LENGTH}-byte file"
    );
    println!(
        "{:<40} {:>12} {:>10} {:>12}",
        "policy", "added B", "chunks", "vs Git 608B"
    );

    let original = source_like_bytes(LENGTH, 701);
    let at = LENGTH / 2;
    let edited = overwrite_at(&original, at, &[original[at] ^ 0xFF]);

    for policy in policies() {
        let before = apply(&policy, &original);
        let after = apply(&policy, &edited);
        let new_content = incremental(std::slice::from_ref(&before), std::slice::from_ref(&after));
        let new_chunks = {
            let held: Vec<Digest32> = before.iter().map(|(digest, _)| *digest).collect();
            after
                .iter()
                .filter(|(digest, _)| !held.contains(digest))
                .count() as u64
        };
        let added = new_content + new_chunks * JOURNAL_BYTES_PER_CHUNK;
        println!(
            "{:<40} {:>12} {:>10} {:>11.2}x",
            policy.name,
            added,
            new_chunks,
            added as f64 / GIT_BYTES as f64
        );
    }
    println!(
        "(added B = new chunk content + {JOURNAL_BYTES_PER_CHUNK} B of mesh-cas arrival journal \
         per new chunk; the local index is measured next door in mesh-store and is not in this \
         column)"
    );
}

fn source_tree_corpus() {
    let files = source_tree();
    let edited = one_byte_edit(&files);
    report("Source tree", &files, &edited);
}

fn large_binary_corpus() {
    // W5's shape at a size a benchmark can run in a second: incompressible content, a middle
    // insertion rather than an overwrite, so the whole tail shifts.
    let file = pseudorandom_bytes(16 * 1024 * 1024, 501);
    let mut inserted = Vec::with_capacity(file.len() + 1024);
    inserted.extend_from_slice(&file[..file.len() / 2]);
    inserted.extend_from_slice(&pseudorandom_bytes(1024, 502));
    inserted.extend_from_slice(&file[file.len() / 2..]);
    report("Large binary, 1 KiB middle insertion", &[file], &[inserted]);
}

fn throughput() {
    println!("## Throughput");
    let data = pseudorandom_bytes(32 * 1024 * 1024, 601);
    for policy in policies() {
        let start = Instant::now();
        let chunks = apply(&policy, &data).len();
        let elapsed = start.elapsed().as_secs_f64();
        println!(
            "{:<40} {:>10.1} MiB/s  {:>8} chunks",
            policy.name,
            32.0 / elapsed,
            chunks
        );
    }
}

fn gibibyte_workload() {
    println!("## W5 at one gibibyte — streamed, never held");
    let config = ChunkingConfig::default();
    let total = 1024usize * 1024 * 1024;
    let block = 4 * 1024 * 1024;
    let edit_at = total / 2;

    let mut before: Vec<Digest32> = Vec::new();
    let mut after: Vec<(Digest32, u64)> = Vec::new();
    let mut peak = 0usize;

    for edited in [false, true] {
        let mut stream = ChunkStream::new(config);
        let mut offset = 0;
        while offset < total {
            let length = block.min(total - offset);
            let mut bytes = pseudorandom_bytes(length, 0x5715_0000 ^ (offset / block) as u64);
            if edited && edit_at >= offset && edit_at + 1024 <= offset + length {
                let from = edit_at - offset;
                for byte in &mut bytes[from..from + 1024] {
                    *byte = !*byte;
                }
            }
            stream.push(&bytes, |chunk_ref, _| {
                if edited {
                    after.push((*chunk_ref.content_hash(), chunk_ref.length()));
                } else {
                    before.push(*chunk_ref.content_hash());
                }
            });
            peak = peak.max(stream.buffered_bytes());
            offset += length;
        }
        let _ = stream.finish(|chunk_ref, _| {
            if edited {
                after.push((*chunk_ref.content_hash(), chunk_ref.length()));
            } else {
                before.push(*chunk_ref.content_hash());
            }
        });
    }

    let held: std::collections::HashSet<Digest32> = before.iter().copied().collect();
    let mut counted = std::collections::HashSet::new();
    let mut moved = 0u64;
    for (digest, length) in &after {
        if held.contains(digest) || !counted.insert(*digest) {
            continue;
        }
        moved += *length;
    }
    println!(
        "1 GiB file, {} chunks, 1 KiB edit at {edit_at} -> {moved} B transferred ({:.6}% of the \
         file); peak chunker buffer {peak} B; plan §12.4 ceiling 4194304, target 1048576",
        before.len(),
        moved as f64 * 100.0 / total as f64
    );
}
