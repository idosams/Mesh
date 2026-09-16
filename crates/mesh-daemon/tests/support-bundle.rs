//! Support-bundle redaction, exact-preview and unhealthy-workspace coverage.

use std::fs;
use std::path::{Path, PathBuf};

use mesh_daemon::ipc::Json;
use mesh_daemon::{CrashReport, OpenWorkspace, SupportBundle, RECORD_FILE_NAME};
use mesh_store::RECOVERY_DATABASE_FILE_NAME;
use mesh_store::{
    frame_record, no_session, OperationRecord, RecordDigest, RecoverySnapshot,
    RecoveryStatePersistence as _, SqliteRecoveryState, StoredRecord,
};

fn scratch(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "mesh-support-bundle-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("mkdir");
    path
}

fn correlation(preview: &str) -> &str {
    mesh_daemon::ipc::Json::parse(preview).expect("valid json");
    // The parsed value cannot outlive this function, so read it from the source line instead.
    let prefix = "\"workspace_correlation\":\"";
    let after = preview.split_once(prefix).expect("correlation").1;
    after.split_once('"').expect("closing quote").0
}

#[test]
fn correlation_is_stable_and_replaces_the_workspace_path() {
    let root = scratch("private-user-path-sk-live-secret");
    let secret = "BEGIN PRIVATE KEY planted file bytes";
    fs::write(root.join("private-notes.txt"), secret).expect("plant content");

    let first = SupportBundle::collect(&root).preview();
    let second = SupportBundle::collect(&root).preview();

    assert_eq!(correlation(&first), correlation(&second));
    assert!(!first.contains(root.to_string_lossy().as_ref()));
    assert!(!first.contains("private-user-path"));
    assert!(!first.contains(secret));
    assert!(!first.contains("private-notes.txt"));
    assert!(correlation(&first).starts_with("blake3:"));
    assert_eq!(correlation(&first).len(), "blake3:".len() + 64);

    let other = scratch("other");
    assert_ne!(
        correlation(&first),
        correlation(&SupportBundle::collect(&other).preview())
    );
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(other);
}

#[test]
fn correlation_identifies_the_workspace_not_the_callers_path_spelling() {
    let root = scratch("correlation-path-spelling");
    let dotted = root.join(".");

    let direct = SupportBundle::collect(&root).preview();
    let equivalent = SupportBundle::collect(&dotted).preview();

    assert_eq!(correlation(&direct), correlation(&equivalent));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let alias_parent = scratch("correlation-path-alias");
        let alias = alias_parent.join("workspace");
        symlink(&root, &alias).expect("workspace alias");
        let through_alias = SupportBundle::collect(&alias).preview();
        assert_eq!(correlation(&direct), correlation(&through_alias));
        let _ = fs::remove_dir_all(alias_parent);

        let renamed = root.with_extension("renamed");
        let _ = fs::remove_dir_all(&renamed);
        fs::rename(&root, &renamed).expect("rename the same workspace directory");
        assert_eq!(
            correlation(&direct),
            correlation(&SupportBundle::collect(&renamed).preview()),
            "renaming one directory must not create a second support identity"
        );
        let _ = fs::remove_dir_all(renamed);
    }

    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn correlation_is_bound_to_the_exact_journal_generation_that_was_read() {
    let root = scratch("correlation-journal-generation");
    let journal = root.join(RECORD_FILE_NAME);
    let bytes = frame_record(&operation());
    fs::write(&journal, &bytes).expect("first journal generation");

    let first = SupportBundle::collect(&root).preview();
    let first_document = mesh_daemon::ipc::Json::parse(&first).expect("first preview");

    let displaced = root.join("records.mesh.displaced");
    fs::rename(&journal, &displaced).expect("displace first journal generation");
    fs::write(&journal, &bytes).expect("replace with byte-identical journal generation");

    let second = SupportBundle::collect(&root).preview();
    let second_document = mesh_daemon::ipc::Json::parse(&second).expect("second preview");

    assert_eq!(
        first_document
            .get("crash-diagnostics")
            .and_then(|value| value.get("saved_records"))
            .and_then(mesh_daemon::ipc::Json::as_u64),
        Some(1)
    );
    assert_eq!(
        second_document
            .get("crash-diagnostics")
            .and_then(|value| value.get("saved_records"))
            .and_then(mesh_daemon::ipc::Json::as_u64),
        Some(1)
    );
    assert_ne!(
        correlation(&first),
        correlation(&second),
        "a replacement journal cannot inherit the correlation of the generation that was read"
    );

    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn correlation_cannot_be_recovered_by_hashing_likely_private_paths() {
    use mesh_types::{Blake3, ContentDigest as _};
    use std::os::unix::ffi::OsStrExt as _;

    let root = scratch("private-customer-roadmap");
    let preview = SupportBundle::collect(&root).preview();
    let published = correlation(&preview);
    let candidates = [
        root.clone(),
        root.parent()
            .expect("scratch parent")
            .join("customer-roadmap"),
        root.parent()
            .expect("scratch parent")
            .join("secret-project"),
    ];

    for candidate in candidates {
        let canonical = fs::canonicalize(&candidate).unwrap_or(candidate);
        let mut prior_path_derived = b"mesh-support-bundle-workspace-v1\0".to_vec();
        prior_path_derived.extend_from_slice(canonical.as_os_str().as_bytes());
        let guessed = format!(
            "blake3:{}",
            Blake3::digest_bytes(&prior_path_derived).to_hex()
        );
        assert_ne!(
            published, guessed,
            "a recipient recovered the private workspace path from its public correlation"
        );
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn an_unhealthy_workspace_still_produces_only_the_sanitized_crash_section() {
    let root = scratch("unhealthy-secret-path");
    fs::write(
        root.join(RECORD_FILE_NAME),
        b"unfinished secret record content",
    )
    .expect("plant torn journal");

    let preview = SupportBundle::collect(&root).preview();
    let document = mesh_daemon::ipc::Json::parse(&preview).expect("valid json");
    let crash = document.get("crash-diagnostics").expect("crash section");

    assert_eq!(
        document.get("schema").and_then(|v| v.as_text()),
        Some(SupportBundle::SCHEMA)
    );
    assert_eq!(crash.get("serving").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(crash.get("saved_records").and_then(|v| v.as_u64()), Some(0));
    assert!(!preview.contains(root.to_string_lossy().as_ref()));
    assert!(!preview.contains("unfinished secret record content"));
    for excluded in [
        "configuration",
        "event-ledger",
        "file-content",
        "key-material",
        "raw-paths",
    ] {
        assert!(preview.contains(excluded), "missing exclusion {excluded}");
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn preview_document_and_encoded_preview_are_the_same_artifact() {
    let root = scratch("exact");
    let bundle = SupportBundle::collect(Path::new(&root));
    assert_eq!(bundle.document().encode(), bundle.preview());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn live_preview_validator_refuses_every_unallowlisted_string_position() {
    fn replace(value: &mut Json, key: &str, replacement: Json) {
        let Json::Object(pairs) = value else {
            panic!("mutation target is not an object");
        };
        let (_, value) = pairs
            .iter_mut()
            .find(|(name, _)| name == key)
            .expect("mutation key exists");
        *value = replacement;
    }

    fn mutate_crash(value: &mut Json, key: &str, replacement: Json) {
        let Json::Object(pairs) = value else {
            panic!("bundle is not an object");
        };
        let (_, crash) = pairs
            .iter_mut()
            .find(|(name, _)| name == CrashReport::BUNDLE_SECTION)
            .expect("crash section");
        replace(crash, key, replacement);
    }

    let root = scratch("live-validator");
    fs::create_dir_all(&root).expect("workspace");
    drop(OpenWorkspace::open(&root).expect("initialize workspace"));
    let safe = SupportBundle::collect(&root).document().clone();
    SupportBundle::validate_untrusted_preview(&safe).expect("producer output is accepted");

    let mut extra_top_level = safe.clone();
    let Json::Object(pairs) = &mut extra_top_level else {
        panic!("bundle is not an object");
    };
    pairs.push(("raw-paths".to_owned(), Json::text("planted-value")));

    let mut unsafe_sentence = safe.clone();
    mutate_crash(
        &mut unsafe_sentence,
        "sentence",
        Json::text("planted-value"),
    );

    let mut unsafe_sequence = safe.clone();
    mutate_crash(
        &mut unsafe_sequence,
        "meaningful_checkpoint_through",
        Json::text("01"),
    );

    let mut wrong_exclusions = safe.clone();
    replace(
        &mut wrong_exclusions,
        "excluded",
        Json::Array(vec![Json::text("configuration")]),
    );

    let mut wrong_producer = safe.clone();
    let Json::Object(pairs) = &mut wrong_producer else {
        panic!("bundle is not an object");
    };
    let (_, producer) = pairs
        .iter_mut()
        .find(|(name, _)| name == "producer")
        .expect("producer");
    replace(producer, "version", Json::text("different"));

    for (name, mutation) in [
        ("extra-top-level", extra_top_level),
        ("unsafe-sentence", unsafe_sentence),
        ("unsafe-sequence", unsafe_sequence),
        ("wrong-exclusions", wrong_exclusions),
        ("wrong-producer", wrong_producer),
    ] {
        assert!(
            SupportBundle::validate_untrusted_preview(&mutation).is_err(),
            "mutation {name} crossed the live preview boundary"
        );
    }
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn an_unsafe_recovery_database_cannot_be_reported_as_serving() {
    let root = scratch("unsafe-recovery-database");
    drop(OpenWorkspace::open(&root).expect("create a valid workspace"));
    let recovery_database = root.join(".mesh").join(RECOVERY_DATABASE_FILE_NAME);
    fs::create_dir(&recovery_database)
        .expect("plant a recovery-database directory that the real runtime cannot open");

    for invalid_owner in ["directory", "corrupt regular file"] {
        let preview = SupportBundle::collect(&root).preview();
        let document = mesh_daemon::ipc::Json::parse(&preview).expect("valid json");
        let crash = document.get("crash-diagnostics").expect("crash section");
        assert_eq!(
            crash.get("serving").and_then(|value| value.as_bool()),
            Some(false),
            "support preview certified a workspace whose recovery owner is an invalid {invalid_owner}"
        );

        if invalid_owner == "directory" {
            fs::remove_dir(&recovery_database).expect("remove planted directory");
            fs::write(&recovery_database, b"not a SQLite database")
                .expect("plant a corrupt recovery database");
        }
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_read_only_preview_reports_the_validated_recovery_snapshot() {
    let root = scratch("validated-recovery-snapshot");
    drop(OpenWorkspace::open(&root).expect("create a valid workspace"));
    let recovery_database = root.join(".mesh").join(RECOVERY_DATABASE_FILE_NAME);
    let mut recovery = SqliteRecoveryState::open(&recovery_database, b"live-daemon/workspace")
        .expect("open the isolated recovery owner");
    recovery
        .persist(&RecoverySnapshot::default())
        .expect("persist a canonical empty checkpoint snapshot");
    drop(recovery);
    let mut before_entries = fs::read_dir(&root)
        .expect("read pre-preview workspace")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<Vec<_>>();
    before_entries.sort();

    let preview = SupportBundle::collect(&root).preview();
    let document = mesh_daemon::ipc::Json::parse(&preview).expect("valid json");
    let crash = document.get("crash-diagnostics").expect("crash section");

    assert_eq!(
        crash
            .get("serving")
            .and_then(mesh_daemon::ipc::Json::as_bool),
        Some(true),
        "a quiescent canonical recovery owner made its first support preview fail"
    );
    assert_eq!(
        crash
            .get("checkpoint_state_available")
            .and_then(mesh_daemon::ipc::Json::as_bool),
        Some(true),
        "the producer discarded the canonical snapshot returned by its read-only inspector"
    );
    assert_eq!(
        crash
            .get("meaningful_checkpoint_through")
            .and_then(mesh_daemon::ipc::Json::as_u64),
        None
    );
    let mut after_entries = fs::read_dir(&root)
        .expect("read post-preview workspace")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<Vec<_>>();
    after_entries.sort();
    assert_eq!(
        after_entries, before_entries,
        "support preview created recovery database sidecars"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn collecting_from_an_import_source_creates_no_workspace_files_or_directories() {
    let root = scratch("plain-import-source");
    fs::create_dir_all(root.join("src/empty")).expect("source directories");
    fs::write(root.join("README.md"), b"user bytes\n").expect("source file");
    let before = tree_bytes(&root);

    let preview = SupportBundle::collect(&root).preview();
    let document = mesh_daemon::ipc::Json::parse(&preview).expect("valid json");

    assert_eq!(
        tree_bytes(&root),
        before,
        "preview mutated the import source"
    );
    assert_eq!(
        document
            .get("crash-diagnostics")
            .and_then(|value| value.get("serving"))
            .and_then(mesh_daemon::ipc::Json::as_bool),
        Some(false),
        "a folder with no Mesh journal is not a serving workspace"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn collecting_from_a_valid_workspace_does_not_rebuild_or_open_mutable_storage() {
    let root = scratch("valid-read-only");
    fs::write(root.join(RECORD_FILE_NAME), frame_record(&operation())).expect("valid journal");
    let before = tree_bytes(&root);

    let preview = SupportBundle::collect(&root).preview();
    let document = mesh_daemon::ipc::Json::parse(&preview).expect("valid json");

    assert_eq!(
        document
            .get("crash-diagnostics")
            .and_then(|value| value.get("saved_records"))
            .and_then(mesh_daemon::ipc::Json::as_u64),
        Some(1)
    );
    assert_eq!(
        tree_bytes(&root),
        before,
        "preview created an index or content-store directory"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_journal_only_preview_cannot_claim_a_structurally_unopenable_workspace_is_serving() {
    let root = scratch("unopenable-content-store");
    fs::write(root.join(RECORD_FILE_NAME), frame_record(&operation())).expect("valid journal");
    fs::write(root.join("chunks"), b"not a directory").expect("block the content store");
    let before = tree_bytes(&root);

    let preview = SupportBundle::collect(&root).preview();
    let document = mesh_daemon::ipc::Json::parse(&preview).expect("valid json");
    let crash = document.get("crash-diagnostics").expect("crash section");
    assert_eq!(
        crash
            .get("serving")
            .and_then(mesh_daemon::ipc::Json::as_bool),
        Some(false),
        "a journal fold alone cannot prove the complete workspace is usable"
    );
    assert_eq!(
        tree_bytes(&root),
        before,
        "support preview must not repair or otherwise mutate the blocked store"
    );

    let open = mesh_daemon::OpenWorkspace::open(&root)
        .expect_err("the blocked content store makes the workspace unopenable");
    assert_eq!(open.code(), "workspace-payload-store-unreachable");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn an_unsafe_runtime_layout_does_not_erase_the_known_unfinished_tail() {
    let root = scratch("unsafe-layout-with-unfinished-tail");
    let mut journal = frame_record(&operation());
    journal.extend_from_slice(b"torn");
    fs::write(root.join(RECORD_FILE_NAME), journal).expect("journal with one record and torn tail");
    fs::write(root.join("chunks"), b"not a directory").expect("block the content store");

    let preview = SupportBundle::collect(&root).preview();
    let document = mesh_daemon::ipc::Json::parse(&preview).expect("valid json");
    let crash = document.get("crash-diagnostics").expect("crash section");

    assert_eq!(
        crash.get("serving").and_then(|value| value.as_bool()),
        Some(false)
    );
    assert_eq!(
        crash.get("saved_records").and_then(|value| value.as_u64()),
        Some(1),
        "the unsafe layout cannot erase the known durable boundary"
    );
    assert_eq!(
        crash
            .get("unfinished_bytes")
            .and_then(|value| value.as_u64()),
        Some(4),
        "the unsafe layout cannot turn a known interrupted append into a clean tail"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn an_oversized_sparse_journal_is_refused_without_reading_or_mutating_it() {
    let root = scratch("oversized-sparse-journal");
    let journal = root.join(RECORD_FILE_NAME);
    let file = fs::File::create(&journal).expect("create sparse journal");
    file.set_len(1024 * 1024 * 1024)
        .expect("make one-gibibyte sparse journal");
    drop(file);

    let preview = SupportBundle::collect(&root).preview();
    let document = mesh_daemon::ipc::Json::parse(&preview).expect("valid json");
    let crash = document
        .get("crash-diagnostics")
        .expect("crash diagnostics");

    assert_eq!(crash.get("serving").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(crash.get("saved_records").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(
        fs::metadata(&journal).expect("journal metadata").len(),
        1024 * 1024 * 1024,
        "support preview changed the oversized journal"
    );
    assert_eq!(
        fs::read_dir(&root).expect("workspace directory").count(),
        1,
        "support preview created workspace state"
    );
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn a_linked_record_file_is_refused_without_reading_its_external_target() {
    use std::os::unix::fs::symlink;

    let root = scratch("linked-journal");
    let external = scratch("linked-journal-target");
    let target = external.join("private-records.mesh");
    fs::write(&target, frame_record(&operation())).expect("external record bytes");
    symlink(&target, root.join(RECORD_FILE_NAME)).expect("linked journal");
    let root_before = tree_bytes(&root);
    let external_before = tree_bytes(&external);

    let preview = SupportBundle::collect(&root).preview();
    let document = mesh_daemon::ipc::Json::parse(&preview).expect("valid json");
    let crash = document
        .get("crash-diagnostics")
        .expect("crash diagnostics");

    assert_eq!(
        crash
            .get("serving")
            .and_then(mesh_daemon::ipc::Json::as_bool),
        Some(false)
    );
    assert_eq!(
        crash
            .get("saved_records")
            .and_then(mesh_daemon::ipc::Json::as_u64),
        Some(0),
        "following the link would have reported the external record"
    );
    assert_eq!(tree_bytes(&root), root_before);
    assert_eq!(tree_bytes(&external), external_before);
    assert!(!preview.contains(target.to_string_lossy().as_ref()));
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(external);
}

fn operation() -> StoredRecord {
    StoredRecord::Operation(OperationRecord {
        id: RecordDigest::from_bytes([1; 32]),
        actor: RecordDigest::from_bytes([2; 32]),
        actor_sequence: 1,
        hlc_millis: 1_700_000_000_000,
        hlc_counter: 0,
        policy_epoch: 1,
        session: no_session(),
        payload_digest: RecordDigest::from_bytes([3; 32]),
        parents: Vec::new(),
    })
}

fn tree_bytes(root: &Path) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    fn visit(root: &Path, relative: &Path, found: &mut Vec<(PathBuf, Option<Vec<u8>>)>) {
        let mut entries: Vec<_> = fs::read_dir(root.join(relative))
            .expect("read directory")
            .map(|entry| entry.expect("directory entry"))
            .collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let child = relative.join(entry.file_name());
            if entry.file_type().expect("entry type").is_dir() {
                found.push((child.clone(), None));
                visit(root, &child, found);
            } else {
                found.push((
                    child.clone(),
                    Some(fs::read(root.join(&child)).expect("file bytes")),
                ));
            }
        }
    }

    let mut found = Vec::new();
    visit(root, Path::new(""), &mut found);
    found
}
