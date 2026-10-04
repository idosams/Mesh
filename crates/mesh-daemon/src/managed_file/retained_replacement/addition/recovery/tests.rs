use super::super::super::tests::Fixture;
use super::super::tests::prepare;
use super::*;

#[test]
fn file_recovers_in_new_processes_after_installer_exit_and_keeps_exact_identity() {
    const MODE: &str = "MESH_FILE_RECOVERY_CHILD_MODE";
    const SOURCE: &str = "MESH_FILE_RECOVERY_CHILD_SOURCE";
    const STORE: &str = "MESH_FILE_RECOVERY_CHILD_STORE";
    if let Some(mode) = std::env::var_os(MODE) {
        let source = PathBuf::from(std::env::var_os(SOURCE).unwrap());
        let recovery = PathBuf::from(std::env::var_os(STORE).unwrap());
        let receipt = fs::read_to_string(recovery.join("receipt.json")).unwrap();
        let bytes = fs::read(recovery.join("saved-bytes")).unwrap();
        let resumed = RetainedAddition::resume(
            PinnedWorkspaceRoot::open(source).unwrap(),
            "new.txt".into(),
            PinnedWorkspaceRoot::open(recovery).unwrap(),
            bytes,
            &receipt,
            64,
        )
        .unwrap();
        if mode == "interrupt" {
            let _ = resumed.apply_with_hooks(|| {}, || std::process::exit(75), File::sync_all);
            panic!("installer never reached rename");
        }
        assert_eq!(mode, "recover");
        assert!(resumed.apply().unwrap());
        return;
    }
    let f = Fixture::new();
    let prepared = prepare(&f);
    let identity = prepared.installed.token();
    fs::write(
        f.recovery.join("receipt.json"),
        prepared.recovery_receipt().unwrap(),
    )
    .unwrap();
    fs::write(f.recovery.join("saved-bytes"), prepared.replacement_bytes()).unwrap();
    for name in ["receipt.json", "saved-bytes"] {
        File::open(f.recovery.join(name))
            .unwrap()
            .sync_all()
            .unwrap();
    }
    File::open(&f.recovery).unwrap().sync_all().unwrap();
    drop(prepared);
    let invoke = |mode: &str| {
        std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "managed_file::retained_replacement::addition::recovery::tests::file_recovers_in_new_processes_after_installer_exit_and_keeps_exact_identity", "--test-threads=1"])
        .env(MODE, mode).env(SOURCE, &f.source).env(STORE, &f.recovery).output().unwrap()
    };
    assert_eq!(invoke("interrupt").status.code(), Some(75));
    for _ in 0..2 {
        let output = invoke("recover");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let file = File::open(f.source.join("new.txt")).unwrap();
        assert_eq!(
            managed_file_identity(&file, &file.metadata().unwrap())
                .unwrap()
                .token(),
            identity
        );
        assert_eq!(fs::read(f.source.join("new.txt")).unwrap(), b"approved");
        assert_ne!(file.metadata().unwrap().mode() & 0o111, 0);
    }
    assert!(!f.recovery.join(EXCHANGE).exists());
    assert_eq!(fs::read(f.source.join("file")).unwrap(), b"before");
}

#[test]
fn file_recovery_refuses_wrong_bytes_receipts_roots_and_substituted_files() {
    for change in [
        "bytes",
        "limit",
        "path",
        "receipt",
        "stage-edit",
        "stage-replace",
        "installed-edit",
        "installed-replace",
        "both",
        "neither",
        "source-root",
        "recovery-root",
        "symlink",
    ] {
        let f = Fixture::new();
        let prepared = prepare(&f);
        let mut receipt = prepared.recovery_receipt().unwrap();
        let mode = prepared.mode;
        drop(prepared);
        let mut bytes = b"approved".to_vec();
        let mut path = PathBuf::from("new.txt");
        let mut limit = 8;
        match change {
            "bytes" => bytes = b"rejected".to_vec(),
            "limit" => limit = 7,
            "path" => path = "other.txt".into(),
            "receipt" => receipt.push(' '),
            "stage-edit" => fs::write(f.recovery.join(EXCHANGE), b"editor").unwrap(),
            "stage-replace" => {
                fs::rename(f.recovery.join(EXCHANGE), f.recovery.join("retained")).unwrap();
                fs::write(f.recovery.join(EXCHANGE), b"approved").unwrap();
                fs::set_permissions(f.recovery.join(EXCHANGE), fs::Permissions::from_mode(mode))
                    .unwrap();
            }
            "installed-edit" | "installed-replace" => {
                fs::rename(f.recovery.join(EXCHANGE), f.source.join("new.txt")).unwrap();
                if change == "installed-replace" {
                    fs::rename(f.source.join("new.txt"), f.source.join("retained")).unwrap();
                    fs::write(f.source.join("new.txt"), b"approved").unwrap();
                    fs::set_permissions(f.source.join("new.txt"), fs::Permissions::from_mode(mode))
                        .unwrap();
                } else {
                    fs::write(f.source.join("new.txt"), b"editor").unwrap();
                }
            }
            "both" => fs::write(f.source.join("new.txt"), b"keep").unwrap(),
            "neither" => {
                fs::rename(f.recovery.join(EXCHANGE), f.recovery.join("retained")).unwrap()
            }
            "source-root" => {
                fs::rename(&f.source, f.source.with_extension("moved")).unwrap();
                fs::create_dir(&f.source).unwrap();
            }
            "recovery-root" => {
                fs::rename(&f.recovery, f.recovery.with_extension("moved")).unwrap();
                fs::create_dir(&f.recovery).unwrap();
            }
            "symlink" => {
                std::os::unix::fs::symlink(f.source.join("file"), f.source.join("new.txt")).unwrap()
            }
            _ => unreachable!(),
        }
        assert!(
            RetainedAddition::resume(
                PinnedWorkspaceRoot::open(f.source.clone()).unwrap(),
                path,
                PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap(),
                bytes,
                &receipt,
                limit,
            )
            .is_err(),
            "{change}"
        );
        let (retained, expected): (PathBuf, &[u8]) = match change {
            "stage-edit" => (f.recovery.join(EXCHANGE), b"editor"),
            "stage-replace" | "neither" => (f.recovery.join("retained"), b"approved"),
            "installed-edit" => (f.source.join("new.txt"), b"editor"),
            "installed-replace" => (f.source.join("retained"), b"approved"),
            "both" => (f.source.join("new.txt"), b"keep"),
            "source-root" => (f.source.with_extension("moved").join("file"), b"before"),
            "recovery-root" => (
                f.recovery.with_extension("moved").join(EXCHANGE),
                b"approved",
            ),
            _ => (f.recovery.join(EXCHANGE), b"approved"),
        };
        assert_eq!(fs::read(retained).unwrap(), expected);
        if change == "symlink" {
            assert!(fs::symlink_metadata(f.source.join("new.txt"))
                .unwrap()
                .file_type()
                .is_symlink());
        }
    }
}

#[test]
fn installed_retry_flushes_both_parents_and_refuses_editor_changes_during_sync() {
    let f = Fixture::new();
    let prepared = prepare(&f);
    let receipt = prepared.recovery_receipt().unwrap();
    assert!(!prepared
        .apply_with_hooks(
            || {},
            || {},
            |_| Err(io::Error::other("lost acknowledgement"))
        )
        .unwrap());
    let resume = || {
        RetainedAddition::resume(
            PinnedWorkspaceRoot::open(f.source.clone()).unwrap(),
            "new.txt".into(),
            PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap(),
            b"approved".to_vec(),
            &receipt,
            8,
        )
        .unwrap()
    };
    let syncs = std::cell::Cell::new(0);
    assert!(!resume()
        .apply_with_hooks(
            || panic!("duplicate install"),
            || {},
            |_| {
                syncs.set(syncs.get() + 1);
                Err(io::Error::other("retry durability failure"))
            }
        )
        .unwrap());
    assert_eq!(syncs.get(), 2);
    syncs.set(0);
    assert!(resume()
        .apply_with_hooks(
            || panic!("duplicate install"),
            || {},
            |file| {
                syncs.set(syncs.get() + 1);
                fs::write(f.source.join("new.txt"), b"editor").unwrap();
                file.sync_all()
            }
        )
        .is_err());
    assert_eq!(syncs.get(), 2);
    assert_eq!(fs::read(f.source.join("new.txt")).unwrap(), b"editor");
}

#[test]
fn empty_file_resumes_with_zero_byte_budget_without_invented_content() {
    let f = Fixture::new();
    let source = PinnedWorkspaceRoot::open(f.source.clone()).unwrap();
    let relative = PathBuf::from("empty");
    let parent = absent_parent(&source, &relative).unwrap().unwrap();
    let prepared = RetainedAddition::prepare(
        source,
        relative.clone(),
        parent,
        vec![],
        false,
        PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap(),
        |_, _, _, _, _| Ok(()),
    )
    .unwrap();
    let receipt = prepared.recovery_receipt().unwrap();
    drop(prepared);
    for _ in 0..2 {
        assert!(RetainedAddition::resume(
            PinnedWorkspaceRoot::open(f.source.clone()).unwrap(),
            relative.clone(),
            PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap(),
            vec![],
            &receipt,
            0
        )
        .unwrap()
        .apply()
        .unwrap());
        assert!(fs::read(f.source.join("empty")).unwrap().is_empty());
    }
}
