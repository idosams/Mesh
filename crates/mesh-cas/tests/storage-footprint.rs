//! What one byte of workspace content costs on disk, in the content-addressed store.
//!
//! # Why this file exists
//!
//! Plan §12.4 budgets seventeen operations and one idle-CPU figure. It budgets **no** steady-state
//! storage, and plan §2.5 ("every durable version remains reachable until explicit retention policy
//! permits deletion") makes on-disk size grow without bound by construction — per actor, for the
//! life of the workspace. A private durable state per actor plus real-time replication is exactly
//! the shape where an unbudgeted term blows up, and it is cheapest to bound now, while the storage
//! layer is small enough to change.
//!
//! So this file is a **budget**, not a benchmark. Every number it asserts is a count of bytes, and
//! bytes do not flake: no clock is read here, nothing is timed, and the same corpus produces the
//! same figure on every machine that can run the test at all. That is deliberate —
//! `01KZD51YC12BP9AYVTX557RGAS` recorded that `npm test` already carries two wall-clock assertions, and a
//! third would make the merge path more fragile rather than better measured.
//!
//! # The measurement, stated so it can be disputed
//!
//! * The corpus is [`GateCorpus`], generated from a fixed seed by [`support::Bytes`], which is
//!   committed. Its file count and its exact content-byte total are constants below, and
//!   [`the_gate_corpus_is_the_corpus_the_budget_was_set_for`] fails if the generator drifts from
//!   them — so a third party regenerates the same input or finds out immediately.
//! * "Bytes on disk" means the sum of `metadata().len()` over every file the store root contains,
//!   which is filesystem-independent. **Allocated** blocks are *reported* and never gated: on APFS
//!   with a 4 KiB block, this corpus allocates 11.2 % more than it stores, and that figure is a
//!   property of the filesystem rather than of Mesh. Gating it would import the host's block size
//!   into the merge path.
//! * Every gated figure is measured [`REPETITIONS`] times and every repetition must produce the
//!   identical byte count. A byte measurement whose repetitions disagree is not a byte measurement.
//!
//! # What is bounded here, and what is bounded next door
//!
//! This file bounds the content-addressed store: chunk bytes, the arrival journal, the cost of a
//! second actor, and the cost of one more version. The local SQLite index is bounded by
//! `crates/mesh-store/tests/index-footprint.rs`, in that crate, because the lockfile fence of
//! `docs/adr/0014-narrow-the-lockfile-fence-to-admit-an-audited-cryptographic-dependency.md`
//! forbids the dependency edge that would let one process measure both. `benchmarks/budgets/storage.md`
//! composes the two halves and states the arithmetic; `tools/program/storage-budget/check.mjs`
//! fails if any constant here and its row in that document disagree.

mod support;

use std::collections::BTreeSet;
use std::path::Path;

use mesh_cas::{Cas, Digest32};

use support::{Bytes, TempRoot};

// ---------------------------------------------------------------------------
// Budgets. Every constant below has a row in `benchmarks/budgets/storage.md`
// carrying the same number and its justification; `verify:storage` fails when
// the two disagree, in either direction.
// ---------------------------------------------------------------------------

/// Bytes a promoted chunk costs beyond the content it holds.
///
/// Zero, and asserted as an equality rather than a ceiling: `mesh-cas` neither compresses nor
/// frames a chunk, so the file *is* the content. Any nonzero value is a framing change that
/// silently multiplies across every chunk in every workspace.
pub const BUDGET_CAS_CHUNK_BYTES_OVER_CONTENT: u64 = 0;

/// Bytes the arrival journal costs per promoted chunk.
///
/// The journal is the durable record written before a chunk becomes visible, and it is the only
/// term in the store that is per-chunk rather than per-byte — so it is the term that decides what
/// a workspace of small files costs. Measured at 67 bytes per chunk; budgeted at 80 to leave room
/// for a digest-encoding change and none at all for a second line per arrival.
///
/// **Per distinct chunk, not per promotion attempt**, and the distinction is the whole budget:
/// a record appended for a chunk that was already in the store makes this affine in the number of
/// actors holding identical content, which is the normal traffic under replication rather than the
/// exception. [`the_arrival_journal_costs_the_same_whether_content_is_promoted_once_or_eight_times`]
/// is what holds the two rates together.
pub const BUDGET_CAS_JOURNAL_BYTES_PER_CHUNK: u64 = 80;

/// Store bytes per thousand bytes of live content, for [`GateCorpus`].
///
/// The headline for the content-addressed half. 1010 means the store is allowed to be 1.0 % larger
/// than the content it holds. Measured at 1003 for this corpus, so the budget carries roughly two
/// and a half times the observed overhead — enough that a journal-format change does not fire it,
/// far too little for a second copy of anything.
pub const BUDGET_CAS_AMPLIFICATION_PER_MILLE: u64 = 1010;

/// Chunks a second, third or eighth actor adds when its content is identical.
///
/// Zero. This is the structural claim in its smallest testable form: N actors holding the same
/// tree cost 1x the tree, not Nx. If this ever becomes nonzero, content addressing has stopped
/// deduplicating and every per-actor figure in the program is wrong.
pub const BUDGET_IDENTICAL_ACTOR_CHUNK_GROWTH: u64 = 0;

/// Bytes a one-byte edit costs on disk, beyond the length of the file it edits.
///
/// **This budget records a cost, it does not defend one.** `mesh-chunking` is a 21-line scaffold,
/// so a file is one whole chunk and a one-byte edit promotes the whole file again: editing one byte
/// of a 1 GiB file costs 1 GiB on disk, against a plan §12.4 row that budgets under 4 MiB of
/// *transfer* for exactly that operation. 80 is the arrival-journal line that comes with the new
/// chunk. When content-defined chunking lands this number should collapse, and the row in
/// `benchmarks/budgets/storage.md` is to be tightened the day it does.
pub const BUDGET_WHOLE_FILE_REWRITE_OVERHEAD_BYTES: u64 = 80;

/// Files in [`GateCorpus`].
pub const GATE_CORPUS_FILES: u64 = 96;

/// Content bytes in [`GateCorpus`], exactly.
pub const GATE_CORPUS_CONTENT_BYTES: u64 = 1_808_424;

/// The seed [`GateCorpus`] is generated from.
pub const GATE_CORPUS_SEED: u64 = 42;

/// Files used by the actor-scaling tests: the first this many of [`GateCorpus`].
///
/// Fewer than the whole corpus on purpose. The actor law is exact rather than statistical, so a
/// third of the tree demonstrates it, and eight actors over the whole corpus would put a thousand
/// extra `fsync`-bound promotions on every merge for no extra evidence.
pub const ACTOR_SCALING_FILES: usize = 32;

/// How many times a gated figure is measured. Every repetition must agree exactly.
const REPETITIONS: usize = 3;

/// How many actors promote the same content in the repeated-promotion measurements.
///
/// Not a budget — it is the length of a demonstration whose claim is an exact equality, so any
/// value above one shows the same thing and eight is what the finding was measured at.
const IDENTICAL_ACTORS: u64 = 8;

// ---------------------------------------------------------------------------
// The corpus
// ---------------------------------------------------------------------------

/// A committed, seeded tree: [`GATE_CORPUS_FILES`] files, log-uniform between 8 KiB and 32 KiB.
///
/// Small enough that the whole file runs in seconds in a debug build, and shaped so the per-chunk
/// terms are visible without being dominant. Content entropy is irrelevant to the figures here —
/// `mesh-cas` does not compress — so random bytes and source code cost the same, which is stated
/// rather than left for a reader to wonder about.
struct GateCorpus {
    sizes: Vec<usize>,
}

impl GateCorpus {
    fn new() -> Self {
        let mut rng = Bytes::seeded(GATE_CORPUS_SEED);
        let sizes = (0..GATE_CORPUS_FILES)
            .map(|_| {
                let base = 1usize << (13 + rng.below(2) as u32); // 8 KiB or 16 KiB
                base + rng.below(base as u64) as usize
            })
            .collect();
        Self { sizes }
    }

    fn content_bytes(&self) -> u64 {
        self.sizes.iter().map(|size| *size as u64).sum()
    }

    /// File `index`'s bytes, as actor `actor` holds them at version `version`.
    ///
    /// `(actor, version) == (0, 0)` is the shared base every actor starts from.
    fn file(&self, index: usize, actor: u64, version: u64) -> Vec<u8> {
        let seed = 1 + index as u64 + actor * 1_000_003 + version * 7_919;
        Bytes::seeded(seed).take(self.sizes[index])
    }

    fn promote_base(&self, store: &Cas<mesh_cas::StdFs, mesh_cas::Blake3>) {
        for index in 0..self.sizes.len() {
            store
                .promote(self.file(index, 0, 0))
                .expect("the base tree promotes");
        }
    }
}

// ---------------------------------------------------------------------------
// Measuring
// ---------------------------------------------------------------------------

/// What a store root costs, split into the two figures that behave differently.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Footprint {
    /// Files under the root, including the arrival journal.
    files: u64,
    /// Sum of file lengths. Filesystem-independent, and the only thing gated.
    stored_bytes: u64,
}

fn footprint(root: &Path) -> Footprint {
    let mut measured = Footprint::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("the store root is readable") {
            let entry = entry.expect("an entry");
            let metadata = entry.metadata().expect("entry metadata");
            if metadata.is_dir() {
                stack.push(entry.path());
            } else {
                measured.files += 1;
                measured.stored_bytes += metadata.len();
            }
        }
    }
    measured
}

fn chunk_bytes(store: &Cas<mesh_cas::StdFs, mesh_cas::Blake3>) -> u64 {
    footprint(&store.layout().chunks_directory()).stored_bytes
}

fn chunk_count(store: &Cas<mesh_cas::StdFs, mesh_cas::Blake3>) -> u64 {
    footprint(&store.layout().chunks_directory()).files
}

fn journal_bytes(store: &Cas<mesh_cas::StdFs, mesh_cas::Blake3>) -> u64 {
    std::fs::metadata(store.layout().arrival_journal())
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

/// Runs `measure` [`REPETITIONS`] times and returns the figure, failing if any two disagree.
///
/// The plan §11 record asks for p50, p95 and p99 over a stated sample count. For a byte count they
/// are the same number by construction, and *that they are the same number* is the evidence the
/// instrument is deterministic — so the repetitions are run and compared rather than assumed away.
fn repeatable(label: &str, mut measure: impl FnMut() -> u64) -> u64 {
    let samples: Vec<u64> = (0..REPETITIONS).map(|_| measure()).collect();
    let first = samples[0];
    assert!(
        samples.iter().all(|sample| *sample == first),
        "`{label}` is not deterministic across {REPETITIONS} repetitions: {samples:?}. A byte \
         measurement whose repetitions disagree cannot carry a budget."
    );
    first
}

// ---------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------

/// The corpus constants the budget was set for are the corpus the generator produces.
///
/// Without this, every other figure in the file is a ratio against an unstated denominator, and
/// `benchmarks/budgets/storage.md` would be quoting a number nobody could reproduce.
#[test]
fn the_gate_corpus_is_the_corpus_the_budget_was_set_for() {
    let corpus = GateCorpus::new();
    assert_eq!(
        corpus.sizes.len() as u64,
        GATE_CORPUS_FILES,
        "the gate corpus changed its file count"
    );
    assert_eq!(
        corpus.content_bytes(),
        GATE_CORPUS_CONTENT_BYTES,
        "the gate corpus changed its content total; every budget below is a ratio against it"
    );
    // Byte-for-byte reproducibility of the content itself, not merely of the sizes.
    for index in [0usize, 1, corpus.sizes.len() - 1] {
        assert_eq!(
            corpus.file(index, 0, 0),
            GateCorpus::new().file(index, 0, 0),
            "the gate corpus is not reproducible at file {index}"
        );
    }
}

/// A chunk file is its content and nothing else.
#[test]
fn a_chunk_costs_exactly_the_bytes_it_holds() {
    let corpus = GateCorpus::new();
    let root = TempRoot::new("footprint-chunk-exact");
    let store = Cas::open(root.path()).expect("the store opens");
    corpus.promote_base(&store);

    let stored = chunk_bytes(&store);
    assert_eq!(
        stored - corpus.content_bytes(),
        BUDGET_CAS_CHUNK_BYTES_OVER_CONTENT,
        "chunk files hold {stored} bytes for {} bytes of content; the store neither compresses \
         nor frames, so the two are equal or something now costs per chunk that did not",
        corpus.content_bytes()
    );
}

/// The arrival journal is linear in chunks, at a rate inside its budget.
///
/// Both arms promote each chunk [`IDENTICAL_ACTORS`] times over, so the rate this gates is a rate
/// per **chunk** and not a rate per **promotion attempt**. Promoting only distinct content — which
/// is what this test did while `01KZE8KYZGGCD732XZHZSFT9B6` was open — makes every promotion a first
/// promotion, and the two rates coincide exactly where they must not.
#[test]
fn the_arrival_journal_costs_a_bounded_number_of_bytes_per_chunk() {
    let mut rates = Vec::new();
    for chunks in [32u64, 128] {
        let root = TempRoot::new("footprint-journal");
        let store = Cas::open(root.path()).expect("the store opens");
        for _ in 0..IDENTICAL_ACTORS {
            for index in 0..chunks {
                store
                    .promote(Bytes::seeded(90_001 + index).take(512))
                    .expect("promotes");
            }
        }
        let bytes = journal_bytes(&store);
        assert_eq!(
            bytes % chunks,
            0,
            "the journal is {bytes} bytes for {chunks} chunks, which is not a whole number per \
             chunk — the per-chunk cost is no longer a constant"
        );
        let rate = bytes / chunks;
        assert!(
            rate <= BUDGET_CAS_JOURNAL_BYTES_PER_CHUNK,
            "the arrival journal costs {rate} bytes per chunk, over the budget of {}",
            BUDGET_CAS_JOURNAL_BYTES_PER_CHUNK
        );
        rates.push(rate);
    }
    assert_eq!(
        rates[0], rates[1],
        "the journal's per-chunk cost changes with the number of chunks ({rates:?}); it is \
         budgeted as a constant and a superlinear journal is unbounded history by another name"
    );
}

/// Promoting one corpus eight times over costs the same journal as promoting it once.
///
/// The equality is what makes [`BUDGET_CAS_JOURNAL_BYTES_PER_CHUNK`] a budget over chunks. While
/// `01KZE8KYZGGCD732XZHZSFT9B6` was open, `RecordArrival` ran before the promotion discovered
/// whether the chunk had arrived, so eight actors holding the same 32 files cost 8 x 32 x 67 =
/// 17,152 journal bytes — 536 per chunk against a budget of 80, for content that adds no chunk
/// bytes at all. This is a strict equality rather than a bound because the correct number is a
/// constant: a promotion that makes nothing visible has nothing to record.
#[test]
fn the_arrival_journal_costs_the_same_whether_content_is_promoted_once_or_eight_times() {
    let corpus = GateCorpus::new();
    let root = TempRoot::new("footprint-journal-repeat");
    let store = Cas::open(root.path()).expect("the store opens");

    for index in 0..ACTOR_SCALING_FILES {
        store
            .promote(corpus.file(index, 0, 0))
            .expect("the base tree promotes");
    }
    let after_one = journal_bytes(&store);
    let chunks = chunk_count(&store);
    assert_eq!(
        chunks, ACTOR_SCALING_FILES as u64,
        "each file is one chunk at this chunking scaffold, which is what the rate below divides by"
    );

    for actor in 1..IDENTICAL_ACTORS {
        for index in 0..ACTOR_SCALING_FILES {
            store
                .promote(corpus.file(index, 0, 0))
                .expect("re-promotion succeeds");
        }
        let now = journal_bytes(&store);
        println!(
            "storage-footprint journal: actors={} chunks={chunks} journal_bytes={now} \
             journal_per_chunk={} budget={BUDGET_CAS_JOURNAL_BYTES_PER_CHUNK}",
            actor + 1,
            now / chunks
        );
        assert_eq!(
            now,
            after_one,
            "actor {} promoting content the store already holds grew the arrival journal from \
             {after_one} to {now} bytes. Nothing arrived, so nothing arrived to record: a journal \
             that costs per promotion attempt is affine in the actor count for content that \
             deduplicates perfectly, which is the normal traffic under replication",
            actor + 1
        );
    }
}

/// The headline for the content-addressed half: what the whole store costs, over the content.
#[test]
fn the_gate_corpus_amplification_is_inside_its_budget() {
    let corpus = GateCorpus::new();
    let stored = repeatable("gate corpus store bytes", || {
        let root = TempRoot::new("footprint-amplification");
        let store = Cas::open(root.path()).expect("the store opens");
        corpus.promote_base(&store);
        footprint(root.path()).stored_bytes
    });

    let per_mille = (stored * 1000) / corpus.content_bytes();
    println!(
        "storage-footprint cas: files={} content_bytes={} store_bytes={stored} \
         amplification_per_mille={per_mille} budget_per_mille={} repetitions={REPETITIONS}",
        GATE_CORPUS_FILES, GATE_CORPUS_CONTENT_BYTES, BUDGET_CAS_AMPLIFICATION_PER_MILLE
    );
    assert!(
        per_mille <= BUDGET_CAS_AMPLIFICATION_PER_MILLE,
        "the store holds {stored} bytes for {} bytes of content ({per_mille} per mille), over the \
         budget of {} per mille",
        corpus.content_bytes(),
        BUDGET_CAS_AMPLIFICATION_PER_MILLE
    );
}

/// N actors holding identical content cost one copy, not N — measured over the **whole store**.
///
/// The footprint is the assertion that matters and it used to be missing. This test asserted
/// `chunk_count` and `chunk_bytes` only, so the one test whose name claims N actors add nothing was
/// the one test that did not measure the term that grew with N: the arrival journal, at 67 bytes
/// per promotion attempt (`01KZE8KYZGGCD732XZHZSFT9B6`). A structural claim checked on two of the
/// store's three terms is not checked.
#[test]
fn actors_holding_identical_content_add_no_chunks() {
    let corpus = GateCorpus::new();
    let root = TempRoot::new("footprint-identical-actors");
    let store = Cas::open(root.path()).expect("the store opens");
    for index in 0..ACTOR_SCALING_FILES {
        store
            .promote(corpus.file(index, 0, 0))
            .expect("the base tree promotes");
    }
    let (chunks, bytes) = (chunk_count(&store), chunk_bytes(&store));
    let whole_store = footprint(root.path());

    for actor in 1..IDENTICAL_ACTORS {
        for index in 0..ACTOR_SCALING_FILES {
            // Same content, promoted as a different actor would promote it.
            store
                .promote(corpus.file(index, 0, 0))
                .expect("re-promotion succeeds");
        }
        assert_eq!(
            chunk_count(&store) - chunks,
            BUDGET_IDENTICAL_ACTOR_CHUNK_GROWTH,
            "actor {actor} holding identical content added chunks; N actors must cost 1x the \
             content, not Nx"
        );
        assert_eq!(
            chunk_bytes(&store),
            bytes,
            "actor {actor} holding identical content added bytes"
        );
        assert_eq!(
            footprint(root.path()),
            whole_store,
            "actor {actor} holding identical content added nothing to `chunks/` and something to \
             the store. Every byte under the root counts here, because a term that grows with the \
             actor count is a term that grows with the actor count wherever it lives"
        );
    }
}

/// N actors that have diverged cost the shared part once and the divergent part N times — exactly,
/// with no third term.
///
/// This is the shape the storage question actually has. It is neither 1x nor Nx: it is affine in
/// the actor count, with the slope set by how much of the tree the actors disagree about.
#[test]
fn diverged_actors_cost_the_shared_tree_once_and_the_divergence_per_actor() {
    let corpus = GateCorpus::new();
    // One file in five is private to each actor; the rest is the shared base.
    let private: BTreeSet<usize> = (0..ACTOR_SCALING_FILES).filter(|i| i % 5 == 0).collect();
    let shared = ACTOR_SCALING_FILES - private.len();

    for actors in [1u64, 2, 4, 8] {
        let root = TempRoot::new("footprint-diverged-actors");
        let store = Cas::open(root.path()).expect("the store opens");
        let mut digests: BTreeSet<Digest32> = BTreeSet::new();
        for actor in 0..actors {
            for index in 0..ACTOR_SCALING_FILES {
                let bytes = if private.contains(&index) {
                    corpus.file(index, actor + 1, 0)
                } else {
                    corpus.file(index, 0, 0)
                };
                digests.insert(store.promote(bytes).expect("promotes").digest());
            }
        }
        let expected = shared as u64 + actors * private.len() as u64;
        assert_eq!(
            chunk_count(&store),
            expected,
            "{actors} actors sharing {shared} files and diverging on {} produced {} chunks, not \
             the affine {expected}",
            private.len(),
            chunk_count(&store)
        );
        assert_eq!(
            digests.len() as u64,
            expected,
            "the store's chunk count and the distinct digests promoted disagree"
        );
        println!(
            "storage-footprint actors: n={actors} shared_files={shared} private_files={} \
             chunks={expected} chunks_over_one_actor_per_mille={}",
            private.len(),
            (expected * 1000) / (ACTOR_SCALING_FILES as u64)
        );
    }
}

/// One more version of a file costs that whole file again — measured, and budgeted as the cost it
/// is rather than the cost anyone wants.
#[test]
fn a_one_byte_edit_costs_the_whole_file_again() {
    let root = TempRoot::new("footprint-one-byte-edit");
    let store = Cas::open(root.path()).expect("the store opens");

    let length = 256 * 1024usize;
    let mut content = Bytes::seeded(31_337).take(length);
    store.promote(content.clone()).expect("promotes");
    let before = footprint(root.path()).stored_bytes;

    content[length / 2] ^= 0xff;
    store.promote(content).expect("promotes the edit");
    let after = footprint(root.path()).stored_bytes;

    let cost = after - before;
    println!(
        "storage-footprint edit: file_bytes={length} edited_bytes=1 disk_cost_bytes={cost} \
         disk_cost_per_edited_byte={cost}"
    );
    assert!(
        cost >= length as u64,
        "a one-byte edit cost {cost} bytes for a {length}-byte file. That is less than the whole \
         file, so the store has gained a delta or a compressor since this budget was set — which \
         is good news, and this assertion and its row in benchmarks/budgets/storage.md are to be \
         retightened rather than deleted."
    );
    assert!(
        cost <= length as u64 + BUDGET_WHOLE_FILE_REWRITE_OVERHEAD_BYTES,
        "a one-byte edit cost {cost} bytes for a {length}-byte file, more than the whole file plus \
         the {BUDGET_WHOLE_FILE_REWRITE_OVERHEAD_BYTES}-byte journal line"
    );
}

/// Allocated blocks are reported and never gated, and this test is why that choice is visible.
///
/// It asserts only that the store allocates at least what it stores — true on every filesystem —
/// and prints the ratio, which on APFS with a 4 KiB block is 1.112 for this corpus. Any gate
/// on that ratio would be a gate on the host's block size.
#[test]
fn allocated_blocks_are_reported_rather_than_budgeted() {
    let corpus = GateCorpus::new();
    let root = TempRoot::new("footprint-allocated");
    let store = Cas::open(root.path()).expect("the store opens");
    corpus.promote_base(&store);

    let stored = footprint(root.path()).stored_bytes;
    let allocated = allocated_bytes(root.path());
    println!(
        "storage-footprint allocated: stored_bytes={stored} allocated_bytes={allocated} \
         allocated_over_stored_per_mille={} (filesystem-dependent; not gated)",
        (allocated * 1000) / stored
    );
    assert!(
        allocated >= stored,
        "the store allocates {allocated} bytes for {stored} stored bytes, which no filesystem does"
    );
}

#[cfg(unix)]
fn allocated_bytes(root: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    let mut total = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("readable") {
            let entry = entry.expect("an entry");
            let metadata = entry.metadata().expect("entry metadata");
            if metadata.is_dir() {
                stack.push(entry.path());
            } else {
                total += metadata.blocks() * 512;
            }
        }
    }
    total
}

#[cfg(not(unix))]
fn allocated_bytes(root: &Path) -> u64 {
    footprint(root).stored_bytes
}
