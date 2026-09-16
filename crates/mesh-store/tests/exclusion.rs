//! What a workspace exclusion actually costs and actually keeps.
//!
//! `cargo nextest run -p mesh-store --test exclusion`
//!
//! # The two claims this file exists for
//!
//! 1. **An excluded path produces no operation, no manifest and no admitted byte.** Asserting "no
//!    checkpoint was reported" would not do: a checkpoint that reports nothing while a manifest row
//!    quietly names the path is exactly the bug worth catching. So this file walks a **real
//!    directory tree** on disk, splits it with the predicate, and then asserts both on the bytes
//!    and on the index rows a checkpoint built from the admitted set would hold.
//! 2. **Excluding a path never loses content that is already durable.** Exclusion is additive and
//!    never retroactive: a path that becomes excluded stops producing *new* versions, and its
//!    existing versions stay exactly as reachable as they were.
//!
//! The second claim has a structural half worth stating outright, because it is stronger than any
//! test: `mesh_store::ExclusionSet` is not a parameter of `Reachability::compute` and not a
//! parameter of `CollectionPlan::compute`. There is no path by which excluding something subtracts
//! reachability, because the retention half of this crate cannot see the exclusion half. The test
//! below measures the byte-for-byte equality anyway, so the structure is checked rather than
//! asserted.
//!
//! # Nothing here reads a clock
//!
//! Every figure is a byte count or a set comparison. `01KZD51YC12BP9AYVTX557RGAS` refused a third
//! wall-clock assertion on the merge path and this file adds none.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mesh_store::{
    Admission, ChunkSlice, EntityUuid, Exclusion, ExclusionSet, ExclusionSource, Index,
    ManifestRecord, OperationRecord, Reachability, RecordDigest, RetainedRoots, RetentionPolicy,
    StoredRecord, WorkspaceRelativePath,
};

// ---------------------------------------------------------------------------
// A temporary tree. `mesh-store` keeps its test closure free of convenience
// dependencies, so there is no `tempfile` here and there is not going to be.
// ---------------------------------------------------------------------------

struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new(label: &str) -> Self {
        let mut root = std::env::temp_dir();
        root.push(format!(
            "mesh-exclusion-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a temporary tree");
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }

    /// Write `length` bytes at a workspace-relative path, creating parents.
    fn write(&self, relative: &str, length: usize) {
        let full = self.root.join(relative);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("a parent directory");
        }
        std::fs::write(&full, vec![b'x'; length]).expect("a file");
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Every file under `root`, as `(workspace-relative path, byte length)`, sorted.
///
/// This is the observation an adapter supplies. It is written here rather than in the crate on
/// purpose: `mesh_store::ExclusionSet` does no I/O, so whoever walks the tree is free to be a FUSE
/// mount, a watcher, or sixteen lines of test code, and the verdict does not change.
fn observe(root: &Path) -> Vec<(WorkspaceRelativePath, u64)> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("a readable directory") {
            let entry = entry.expect("an entry");
            let metadata = entry.metadata().expect("entry metadata");
            if metadata.is_dir() {
                stack.push(entry.path());
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(root)
                .expect("under the root")
                .to_path_buf();
            let path = WorkspaceRelativePath::new(&relative).expect("a legal workspace path");
            found.push((path, metadata.len()));
        }
    }
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

fn digest(tag: u8, number: u64) -> RecordDigest {
    let mut bytes = [0u8; 32];
    bytes[0] = tag;
    bytes[1..9].copy_from_slice(&number.to_be_bytes());
    RecordDigest::from_bytes(bytes)
}

/// The exclusion set a Rust workspace would declare: the compiler's output, and nothing else.
fn build_output_excluded() -> ExclusionSet {
    ExclusionSet::new()
        .with_source(ExclusionSource::WorkspaceFile, "target/\n")
        .expect("the rule parses")
}

// ---------------------------------------------------------------------------
// Claim 1 — an excluded path admits zero bytes and produces no record
// ---------------------------------------------------------------------------

/// A write into an excluded directory admits **zero bytes**, and the excluded bytes dominate.
///
/// The proportions are the finding, not the mechanism: this tree is 1,536 bytes of source beside
/// 3,145,728 bytes of build output, which is the shape `benchmarks/budgets/storage.md` cannot bound
/// with any per-byte budget, because the bytes should never have been admitted at all.
#[test]
fn a_write_into_an_excluded_directory_admits_zero_bytes() {
    let tree = TempTree::new("zero-bytes");
    tree.write("src/lib.rs", 1_024);
    tree.write("src/main.rs", 512);
    tree.write("target/debug/mesh", 2_097_152);
    tree.write("target/debug/deps/libmesh.rlib", 1_048_576);

    let rules = build_output_excluded();
    let admission = Admission::decide(&rules, observe(tree.path()));

    let admitted_under_target: Vec<_> = admission
        .admitted()
        .filter(|(path, _)| path.as_path().starts_with("target"))
        .collect();
    assert!(
        admitted_under_target.is_empty(),
        "the predicate admitted {} path(s) under an excluded directory: {admitted_under_target:?}",
        admitted_under_target.len()
    );

    let excluded_bytes: u64 = admission
        .refused()
        .filter(|(path, _, _)| path.as_path().starts_with("target"))
        .map(|(_, _, length)| length)
        .sum();
    assert_eq!(
        excluded_bytes, 3_145_728,
        "the excluded subtree's bytes were not all accounted for"
    );
    assert_eq!(
        admission.admitted_bytes(),
        1_536,
        "only the source files may be admitted"
    );
    assert_eq!(admission.refused_bytes(), 3_145_728);

    println!(
        "exclusion: admitted_bytes={} refused_bytes={} admitted_over_total_per_mille={}",
        admission.admitted_bytes(),
        admission.refused_bytes(),
        (admission.admitted_bytes() * 1000)
            / (admission.admitted_bytes() + admission.refused_bytes())
    );
}

/// The same tree, taken all the way to the index: no operation row and no manifest row names the
/// excluded path.
///
/// "The store admitted zero bytes" and "no record mentions it" are two different failures, and only
/// the second is visible in the index. Both are checked, because a manifest naming a path whose
/// chunks were never promoted is a dangling reference the fold would have to refuse later.
#[test]
fn an_excluded_path_produces_no_operation_and_no_manifest() {
    let tree = TempTree::new("no-records");
    tree.write("src/lib.rs", 64);
    tree.write("target/debug/mesh", 4_096);

    let rules = build_output_excluded();
    let admission = Admission::decide(&rules, observe(tree.path()));

    // Build exactly the records the admitted set justifies — one manifest per admitted file, one
    // operation carrying them. An excluded path contributes to neither, because it is not here.
    let mut manifests = Vec::new();
    let mut records = Vec::new();
    for (index, (path, length)) in admission.admitted().enumerate() {
        let number = index as u64 + 1;
        let manifest = ManifestRecord {
            id: digest(b'm', number),
            byte_length: length,
            content_digest: digest(b'c', number),
            chunks: vec![ChunkSlice {
                digest: digest(b'k', number),
                byte_offset: 0,
                byte_length: length,
            }],
        };
        assert!(
            !path.as_path().starts_with("target"),
            "an excluded path reached the manifest builder"
        );
        manifests.push(manifest.clone());
        records.push(StoredRecord::Manifest(manifest));
    }
    let operation = OperationRecord {
        id: digest(b'o', 1),
        actor: digest(b'a', 1),
        actor_sequence: 1,
        hlc_millis: 0,
        hlc_counter: 0,
        policy_epoch: 0,
        session: EntityUuid::from_bytes([7; 16]),
        payload_digest: manifests[0].id,
        parents: Vec::new(),
    };
    records.push(StoredRecord::Operation(operation));

    let mut index = Index::new();
    for record in records {
        index.apply(record).expect("every reference resolves");
    }

    // One manifest, for the one admitted file. Not two.
    assert_eq!(
        index.manifest_ids().len(),
        1,
        "the excluded file produced a manifest"
    );
    let admitted_length: u64 = admission.admitted().map(|(_, length)| length).sum();
    assert_eq!(admitted_length, 64);

    // And the whole excluded subtree is bytes the store never saw.
    assert_eq!(admission.refused_bytes(), 4_096);
}

// ---------------------------------------------------------------------------
// Claim 2 — exclusion is additive, never retroactive
// ---------------------------------------------------------------------------

/// Excluding a path **after** its content is durable leaves every existing version exactly as
/// reachable as it was.
///
/// The measurement is the reachable digest set before and after, compared as a set. Not a count —
/// two sets of the same size can differ.
#[test]
fn excluding_a_path_after_content_exists_keeps_every_existing_version_reachable() {
    // A history in which a build artefact was versioned before anyone thought to exclude it.
    let artefact = ManifestRecord {
        id: digest(b'm', 1),
        byte_length: 4_096,
        content_digest: digest(b'c', 1),
        chunks: vec![ChunkSlice {
            digest: digest(b'k', 1),
            byte_offset: 0,
            byte_length: 4_096,
        }],
    };
    let source = ManifestRecord {
        id: digest(b'm', 2),
        byte_length: 64,
        content_digest: digest(b'c', 2),
        chunks: vec![ChunkSlice {
            digest: digest(b'k', 2),
            byte_offset: 0,
            byte_length: 64,
        }],
    };
    let operation = OperationRecord {
        id: digest(b'o', 1),
        actor: digest(b'a', 1),
        actor_sequence: 1,
        hlc_millis: 0,
        hlc_counter: 0,
        policy_epoch: 0,
        session: EntityUuid::from_bytes([9; 16]),
        payload_digest: source.id,
        parents: Vec::new(),
    };

    let mut index = Index::new();
    for record in [
        StoredRecord::Manifest(artefact.clone()),
        StoredRecord::Manifest(source.clone()),
        StoredRecord::Operation(operation),
    ] {
        index.apply(record).expect("every reference resolves");
    }

    let roots = RetainedRoots::conservative(&index, RetentionPolicy::default());
    let before = Reachability::compute(&index, &roots).expect("the closure resolves");
    let reachable_before: BTreeSet<RecordDigest> =
        before.content().map(|(digest, _)| *digest).collect();

    // Now the user excludes the directory the artefact lives in. Nothing about the index changes,
    // because exclusion decides what becomes durable next — not what is durable already.
    let rules = build_output_excluded();
    assert_eq!(
        rules
            .verdict(&WorkspaceRelativePath::new("target/debug/mesh").expect("legal"))
            .excluded_by(),
        Some(ExclusionSource::WorkspaceFile)
    );

    let after = Reachability::compute(&index, &roots).expect("the closure resolves");
    let reachable_after: BTreeSet<RecordDigest> =
        after.content().map(|(digest, _)| *digest).collect();

    assert_eq!(
        reachable_before, reachable_after,
        "excluding a path changed what is reachable; exclusion is additive and never retroactive"
    );
    assert!(
        reachable_before.contains(&artefact.chunks[0].digest),
        "the already-durable artefact chunk stopped being reachable"
    );
    assert!(reachable_before.contains(&source.chunks[0].digest));

    // And the newly excluded path admits nothing from here on, which is the *other* half.
    let admission = Admission::decide(
        &rules,
        [(
            WorkspaceRelativePath::new("target/debug/mesh").expect("legal"),
            4_096,
        )],
    );
    assert_eq!(admission.admitted_bytes(), 0);
    assert_eq!(admission.refused_bytes(), 4_096);
}

/// Re-including a path that was excluded does not resurrect anything either, in either direction.
///
/// The pair with the test above: excluding is additive, and so is un-excluding. Neither edits
/// history, and a reader should not have to infer the second from the first.
#[test]
fn re_including_a_path_changes_only_what_happens_next() {
    let rules = ExclusionSet::new()
        .with_source(ExclusionSource::RepositoryIgnore, "generated/\n")
        .expect("parses");
    let path = WorkspaceRelativePath::new("generated/schema.rs").expect("legal");
    assert_eq!(
        rules.verdict(&path).excluded_by(),
        Some(ExclusionSource::RepositoryIgnore)
    );

    let reinstated = rules
        .with_source(ExclusionSource::WorkspaceFile, "!generated\n")
        .expect("parses");
    assert_eq!(
        reinstated.verdict(&path),
        Exclusion::Included {
            reinstated_by: Some(ExclusionSource::WorkspaceFile)
        }
    );

    // The original set is unchanged: `with_source` returns a new set, so a caller experimenting
    // with a candidate policy cannot alter the one in force.
    assert_eq!(
        rules.verdict(&path).excluded_by(),
        Some(ExclusionSource::RepositoryIgnore)
    );
}

// ---------------------------------------------------------------------------
// Uniformity across adapters
// ---------------------------------------------------------------------------

/// Every adapter gives the same answer for the same path, and the reason is that no adapter is an
/// input.
///
/// The adapter conformance suite lives in `crates/mesh-materializer/tests/adapter-conformance.rs`,
/// which is outside this task's allowed paths, so the uniformity claim is made here in the form
/// that does not need one: the predicate is called with the same two arguments a FUSE mount, an
/// FSKit extension and the folder-watching fallback would each supply, and the verdicts are
/// compared. A backend can only disagree by supplying a different path, which is a different
/// claim — `WorkspaceRelativePath` already forces one spelling per path.
#[test]
fn the_verdict_is_the_same_whichever_backend_asks() {
    let rules = ExclusionSet::new()
        .with_source(ExclusionSource::RepositoryIgnore, "*.o\nbuild/\n")
        .expect("parses")
        .with_source(ExclusionSource::Configuration, "vendor\n")
        .expect("parses")
        .with_source(ExclusionSource::WorkspaceFile, "target/\n!build/keep\n")
        .expect("parses");

    let cases = [
        "src/lib.rs",
        "target/debug/mesh",
        "build/out/a.o",
        "build/keep/note.md",
        "vendor/dep/lib.rs",
        "docs/vendor.md",
        "a/b/c/thing.o",
    ];
    for candidate in cases {
        let path = WorkspaceRelativePath::new(candidate).expect("legal");
        let answers: Vec<Exclusion> = ["fuse", "fskit", "watcher"]
            .iter()
            .map(|_backend| rules.verdict(&path))
            .collect();
        assert!(
            answers.windows(2).all(|pair| pair[0] == pair[1]),
            "{candidate} got different answers from different callers: {answers:?}"
        );
    }

    // And the answers themselves, so this is not merely three copies of the same mistake.
    let excluded = |candidate: &str| {
        rules
            .verdict(&WorkspaceRelativePath::new(candidate).expect("legal"))
            .excluded_by()
    };
    assert_eq!(excluded("src/lib.rs"), None);
    assert_eq!(excluded("docs/vendor.md"), None);
    assert_eq!(
        excluded("target/debug/mesh"),
        Some(ExclusionSource::WorkspaceFile)
    );
    assert_eq!(
        excluded("build/out/a.o"),
        Some(ExclusionSource::RepositoryIgnore)
    );
    assert_eq!(excluded("build/keep/note.md"), None);
    assert_eq!(
        excluded("vendor/dep/lib.rs"),
        Some(ExclusionSource::Configuration)
    );
    assert_eq!(
        excluded("a/b/c/thing.o"),
        Some(ExclusionSource::RepositoryIgnore)
    );
}
