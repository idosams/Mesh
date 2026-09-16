//! What the deleter does, and the three things it refuses to do.
//!
//! `cargo nextest run -p mesh-cas --test collection`
//!
//! Until this file existed nothing in this repository removed a chunk. Every assertion below is
//! therefore about a capability that is new and dangerous, and the file is organised around what
//! stands between it and a lost byte rather than around the API surface.

mod support;

use std::collections::BTreeSet;

use mesh_cas::{Blake3, Cas, CollectionMode, Digest32, DurableFs};

use support::{RecordingFs, TempRoot};

/// An oracle that says referenced for exactly these digests. The signature of `Cas::collect` makes
/// one of these mandatory, so a test cannot accidentally exercise a path without a veto available.
fn oracle(referenced: &BTreeSet<Digest32>) -> impl Fn(&Digest32) -> bool + '_ {
    move |digest: &Digest32| referenced.contains(digest)
}

fn nothing_referenced() -> impl Fn(&Digest32) -> bool {
    |_: &Digest32| false
}

fn promote_all<F: DurableFs>(store: &Cas<F, Blake3>, bodies: &[&[u8]]) -> Vec<Digest32> {
    bodies
        .iter()
        .map(|body| {
            store
                .promote(body.to_vec())
                .expect("promotion succeeds on a fresh store")
                .digest()
        })
        .collect()
}

#[test]
fn a_delete_run_removes_the_chunk_and_reports_the_bytes_it_freed() {
    let root = TempRoot::new("collect-delete");
    let store = Cas::open(root.path()).expect("the store opens");
    let digests = promote_all(&store, &[b"alpha", b"beta bytes here", b"gamma"]);

    let doomed = [digests[0], digests[2]];
    let report = store
        .collect(&doomed, &nothing_referenced(), CollectionMode::Delete)
        .expect("collection succeeds");

    assert_eq!(report.collected(), doomed);
    assert!(report.absent().is_empty());
    assert!(report.refused().is_empty());
    assert_eq!(report.bytes(), (b"alpha".len() + b"gamma".len()) as u64);

    assert!(!store.contains(&digests[0]));
    assert!(!store.contains(&digests[2]));
    assert_eq!(
        store.read(&digests[1]).expect("the survivor still reads"),
        b"beta bytes here"
    );
}

/// The dry run is the acceptance criterion "reports exactly what would be deleted". It is checked
/// from both sides: the report is identical to the delete run's, and the store is untouched.
#[test]
fn a_dry_run_reports_the_same_set_the_delete_run_removes_and_touches_nothing() {
    let root = TempRoot::new("collect-dry");
    let store = Cas::open(root.path()).expect("the store opens");
    let digests = promote_all(&store, &[b"one", b"two", b"three"]);
    let doomed = [digests[0], digests[1]];

    let dry = store
        .collect(&doomed, &nothing_referenced(), CollectionMode::DryRun)
        .expect("a dry run succeeds");
    assert_eq!(dry.mode(), CollectionMode::DryRun);
    assert!(store.contains(&digests[0]), "a dry run deleted a chunk");
    assert!(store.contains(&digests[1]), "a dry run deleted a chunk");
    assert_eq!(
        store
            .journal()
            .candidates()
            .expect("the journal reads")
            .len(),
        3,
        "a dry run rewrote the arrival journal"
    );

    let wet = store
        .collect(&doomed, &nothing_referenced(), CollectionMode::Delete)
        .expect("the delete run succeeds");
    assert_eq!(dry.collected(), wet.collected());
    assert_eq!(dry.bytes(), wet.bytes());
    assert_eq!(dry.absent(), wet.absent());
}

/// The second of the two independent reachability checks. The plan may be stale; the oracle is not.
#[test]
fn the_oracle_vetoes_a_deletion_at_the_instant_it_would_happen() {
    let root = TempRoot::new("collect-veto");
    let store = Cas::open(root.path()).expect("the store opens");
    let digests = promote_all(&store, &[b"kept by a late reference", b"genuine garbage"]);

    let referenced: BTreeSet<Digest32> = [digests[0]].into_iter().collect();
    let report = store
        .collect(&digests, &oracle(&referenced), CollectionMode::Delete)
        .expect("collection succeeds");

    assert_eq!(report.refused(), [digests[0]]);
    assert_eq!(report.collected(), [digests[1]]);
    assert!(
        store.contains(&digests[0]),
        "a vetoed chunk was deleted anyway"
    );
    assert!(!store.contains(&digests[1]));
}

/// Nothing outside `chunks/` is reachable from the collector, whatever it is handed.
#[test]
fn the_collector_only_ever_unlinks_a_content_named_file() {
    let root = TempRoot::new("collect-paths");
    let filesystem = RecordingFs::new();
    let store = Cas::with_filesystem(root.path(), filesystem).expect("the store opens");
    let digests = promote_all(&store, &[b"one", b"two"]);
    store.filesystem().clear();

    store
        .collect(&digests, &nothing_referenced(), CollectionMode::Delete)
        .expect("collection succeeds");

    let chunks = store.layout().chunks_directory();
    let removed = store.filesystem().paths("remove_file");
    assert_eq!(
        removed.len(),
        2,
        "expected exactly two unlinks: {removed:?}"
    );
    for path in &removed {
        assert!(
            path.starts_with(&chunks),
            "the collector unlinked {} which is outside chunks/",
            path.display()
        );
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("the unlinked file has a name");
        assert!(
            Digest32::parse_hex(name).is_ok(),
            "the collector unlinked {name}, which is not a content name"
        );
    }
    for path in store.filesystem().paths("read") {
        assert!(
            !path.starts_with(&chunks),
            "the collector read {} to delete it; measuring a chunk is a stat",
            path.display()
        );
    }
    // Counted over `chunks/` rather than over the whole run, because the sentence being asserted
    // is about chunks. `forget` measures the arrival journal once at the end of a collection — it
    // is how a rewrite tells that nothing was appended underneath it — and a bare total would fail
    // on that while reporting that a chunk had been measured twice. The second loop keeps the
    // total closed, so a future measurement of anything else still fails here.
    let measured = store.filesystem().paths("file_len");
    let journal = store.layout().arrival_journal();
    assert_eq!(
        measured
            .iter()
            .filter(|path| path.starts_with(&chunks))
            .count(),
        2,
        "each doomed chunk is measured exactly once: {measured:?}"
    );
    for path in &measured {
        assert!(
            path.starts_with(&chunks) || *path == journal,
            "the collector measured {}, which is neither a doomed chunk nor the journal",
            path.display()
        );
    }
}

/// The directory entry that stops naming the chunk has to reach the platter too, for the same
/// reason the entry that created it did.
#[test]
fn a_delete_run_syncs_the_directory_it_removed_from() {
    let root = TempRoot::new("collect-sync");
    let filesystem = RecordingFs::new();
    let store = Cas::with_filesystem(root.path(), filesystem).expect("the store opens");
    let digests = promote_all(&store, &[b"one"]);
    store.filesystem().clear();

    store
        .collect(&digests, &nothing_referenced(), CollectionMode::Delete)
        .expect("collection succeeds");

    let directory = store.layout().chunk_directory(&digests[0]);
    let unlinked = store
        .filesystem()
        .position("remove_file", &store.layout().chunk_path(&digests[0]))
        .expect("the chunk was unlinked");
    let synced = store
        .filesystem()
        .position("sync_dir", &directory)
        .expect("the directory was synced");
    assert!(
        unlinked < synced,
        "the directory was synced before the unlink it has to commit"
    );
}

/// A collected chunk stops being offered as a candidate, or every future run stats a file that is
/// not there.
#[test]
fn a_collected_chunk_leaves_the_candidate_set_and_the_survivors_stay() {
    let root = TempRoot::new("collect-journal");
    let store = Cas::open(root.path()).expect("the store opens");
    let digests = promote_all(&store, &[b"one", b"two", b"three"]);
    assert_eq!(
        store
            .journal()
            .candidates()
            .expect("the journal reads")
            .len(),
        3
    );

    store
        .collect(&digests[..2], &nothing_referenced(), CollectionMode::Delete)
        .expect("collection succeeds");

    let remaining = store.journal().candidates().expect("the journal reads");
    assert_eq!(remaining, vec![digests[2]]);
}

/// A `-` record means "a durable reference points at this". Collection must not fake one.
#[test]
fn forgetting_a_collected_chunk_keeps_every_other_records_meaning() {
    let root = TempRoot::new("collect-journal-meaning");
    let store = Cas::open(root.path()).expect("the store opens");
    let digests = promote_all(&store, &[b"referenced", b"garbage"]);
    store
        .journal()
        .record_retained(&digests[0])
        .expect("the retention record appends");
    assert_eq!(
        store.journal().candidates().expect("the journal reads"),
        vec![digests[1]]
    );

    store
        .collect(&digests[1..], &nothing_referenced(), CollectionMode::Delete)
        .expect("collection succeeds");

    assert!(
        store
            .journal()
            .candidates()
            .expect("the journal reads")
            .is_empty(),
        "the referenced chunk came back as a candidate"
    );
    assert!(
        store.contains(&digests[0]),
        "the referenced chunk was deleted"
    );
}

/// Being handed the same digest twice must free it once, not report double the bytes.
#[test]
fn a_repeated_digest_is_collected_once_and_counted_once() {
    let root = TempRoot::new("collect-repeat");
    let store = Cas::open(root.path()).expect("the store opens");
    let digests = promote_all(&store, &[b"once"]);

    let report = store
        .collect(
            &[digests[0], digests[0], digests[0]],
            &nothing_referenced(),
            CollectionMode::Delete,
        )
        .expect("collection succeeds");
    assert_eq!(report.collected(), [digests[0]]);
    assert_eq!(report.bytes(), b"once".len() as u64);
}

/// A candidate list naming a chunk the store does not hold is normal, not an error.
#[test]
fn a_doomed_chunk_the_store_never_held_is_reported_absent() {
    let root = TempRoot::new("collect-absent");
    let store = Cas::open(root.path()).expect("the store opens");
    let missing = Digest32::from_bytes([9; 32]);

    let report = store
        .collect(&[missing], &nothing_referenced(), CollectionMode::Delete)
        .expect("collection succeeds");
    assert_eq!(report.absent(), [missing]);
    assert!(report.collected().is_empty());
    assert!(report.freed_nothing());
}

/// Collecting an empty list is a no-op that still returns a report, so a caller can log a run that
/// found nothing without special-casing it.
#[test]
fn collecting_nothing_rewrites_nothing() {
    let root = TempRoot::new("collect-empty");
    let filesystem = RecordingFs::new();
    let store = Cas::with_filesystem(root.path(), filesystem).expect("the store opens");
    promote_all(&store, &[b"one"]);
    store.filesystem().clear();

    let report = store
        .collect(&[], &nothing_referenced(), CollectionMode::Delete)
        .expect("collection succeeds");
    assert!(report.freed_nothing());
    assert_eq!(store.filesystem().count("remove_file"), 0);
    assert_eq!(
        store.filesystem().count("rename"),
        0,
        "an empty collection rewrote the arrival journal"
    );
}

/// The default is the harmless mode, so a caller who forgets to choose does not delete.
#[test]
fn the_default_mode_deletes_nothing() {
    assert_eq!(CollectionMode::default(), CollectionMode::DryRun);
    assert!(!CollectionMode::default().deletes());
    assert!(CollectionMode::Delete.deletes());
}
