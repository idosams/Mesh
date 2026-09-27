//! Non-exclusive replacement keeps the displaced inode named for late editor writes.
use super::*;
use crate::root_authority::PinnedWorkspaceRoot;

const EXCHANGE: &str = "exchange";

mod metadata;
use metadata::{copy_metadata, metadata_digest};

/// Single-use in-process authority. Dropping it deliberately preserves staged recovery material.
pub(crate) struct RetainedReplacement {
    target: ManagedReplacementTarget,
    expected: Vec<u8>,
    bytes: Vec<u8>,
    mode: u32,
    installed: ManagedFileIdentity,
    recovery: PinnedWorkspaceRoot,
    metadata_digest: String,
}

pub(crate) fn read_target(
    root: &Path,
    relative: &str,
    limit: usize,
) -> io::Result<(ManagedReplacementTarget, Vec<u8>)> {
    let (path, directory, _, metadata, file, bytes) =
        read_confined_regular_file_in_layout_bounded(root, relative, false, limit)
            .map_err(|error| io::Error::other(error.to_string()))?;
    let parent = directory.metadata()?;
    Ok((
        ManagedReplacementTarget {
            root: root.to_owned(),
            relative: relative.to_owned(),
            reserve_private_top_level: false,
            path,
            parent: ManagedDirectoryIdentity {
                device: parent.dev(),
                inode: parent.ino(),
            },
            file,
            mode: metadata.mode(),
        },
        bytes,
    ))
}

fn matches(
    directory: &File,
    name: &std::ffi::OsStr,
    identity: ManagedFileIdentity,
    mode: u32,
    expected: &[u8],
) -> bool {
    open_read_at(directory, name).ok().is_some_and(|file| {
        let Ok(metadata) = file.metadata() else {
            return false;
        };
        let mut bytes = Vec::new();
        metadata.is_file()
            && metadata.mode() == mode
            && metadata.len() == expected.len() as u64
            && managed_file_identity(&file, &metadata).ok() == Some(identity)
            && file
                .take(expected.len() as u64 + 1)
                .read_to_end(&mut bytes)
                .is_ok()
            && bytes == expected
    })
}

impl RetainedReplacement {
    /// The caller creates a fresh, private external directory on the source filesystem. The
    /// callback durably binds staged identity to approved content before this can be applied.
    pub(crate) fn prepare(
        target: ManagedReplacementTarget,
        expected: Vec<u8>,
        bytes: Vec<u8>,
        mode: u32,
        recovery: PinnedWorkspaceRoot,
        receipt: impl FnOnce(ManagedFileIdentity, &str) -> io::Result<()>,
    ) -> io::Result<Self> {
        if target.mode & 0o7000 != 0 {
            return Err(io::Error::other(
                "special permission bits require explicit metadata support",
            ));
        }
        recovery.ensure_namespace_identity()?;
        let directory = recovery.try_clone_directory()?;
        if directory.metadata()?.dev() != target.file.device {
            return Err(io::Error::other(
                "recovery must share the source filesystem",
            ));
        }
        let mut staged = create_new_at(&directory, EXCHANGE)?;
        staged.set_permissions(fs::Permissions::from_mode(mode))?;
        let (_, parent, name, _, _, _) = read_confined_regular_file_in_layout_bounded(
            &target.root,
            &target.relative,
            target.reserve_private_top_level,
            expected.len(),
        )
        .map_err(|error| io::Error::other(error.to_string()))?;
        let source = open_read_at(&parent, &name)?;
        let source_metadata_digest = metadata_digest(&source)?;
        copy_metadata(&source, &staged)?;
        staged.set_permissions(fs::Permissions::from_mode(mode))?;
        if metadata_digest(&staged)? != source_metadata_digest {
            return Err(io::Error::other("replacement metadata was not preserved"));
        }
        staged.write_all(&bytes)?;
        staged.sync_all()?;
        let installed = managed_file_identity(&staged, &staged.metadata()?)?;
        directory.sync_all()?;
        receipt(installed, &source_metadata_digest)?;
        recovery.ensure_namespace_identity()?;
        Ok(Self {
            target,
            expected,
            bytes,
            mode: staged.metadata()?.mode(),
            installed,
            recovery,
            metadata_digest: source_metadata_digest,
        })
    }

    /// No error after the exchange is reported as a refusal. False means that reconciliation is
    /// needed, and both directory entries are preserved. There is no automatic rollback or cleanup.
    pub(crate) fn apply(self) -> io::Result<bool> {
        self.apply_with_hooks(|| {}, || {}, File::sync_all)
    }

    fn apply_with_hooks(
        self,
        before: impl FnOnce(),
        after: impl FnOnce(),
        sync: impl Fn(&File) -> io::Result<()>,
    ) -> io::Result<bool> {
        let (path, directory, name, metadata, identity, bytes) =
            read_confined_regular_file_in_layout_bounded(
                &self.target.root,
                &self.target.relative,
                self.target.reserve_private_top_level,
                self.expected.len(),
            )
            .map_err(|error| io::Error::other(error.to_string()))?;
        let parent = directory.metadata()?;
        if path != self.target.path
            || parent.dev() != self.target.parent.device
            || parent.ino() != self.target.parent.inode
            || identity != self.target.file
            || metadata.mode() != self.target.mode
            || bytes != self.expected
        {
            return Err(io::Error::other("source changed since preparation"));
        }
        self.recovery.ensure_namespace_identity()?;
        let recovery = self.recovery.try_clone_directory()?;
        let exchange = std::ffi::OsStr::new(EXCHANGE);
        if !matches(&recovery, exchange, self.installed, self.mode, &self.bytes) {
            return Err(io::Error::other("prepared replacement changed"));
        }
        let source = open_read_at(&directory, &name)?;
        if metadata_digest(&source)? != self.metadata_digest
            || metadata_digest(&open_read_at(&recovery, exchange)?)? != self.metadata_digest
        {
            return Err(io::Error::other("file metadata changed since preparation"));
        }
        before();
        atomic_exchange_at(&recovery, exchange, &directory, &name)?;
        after();
        // Always attempt both barriers even if one fails. Crucially, never unlink the old inode:
        // another process can keep writing it after every check below has finished.
        let source_durable = sync(&directory).is_ok();
        let recovery_durable = sync(&recovery).is_ok();
        let displaced_matches = matches(
            &recovery,
            exchange,
            self.target.file,
            self.target.mode,
            &self.expected,
        );
        let installed_matches = matches(&directory, &name, self.installed, self.mode, &self.bytes);
        let source_current =
            read_target(&self.target.root, &self.target.relative, self.bytes.len()).is_ok_and(
                |(current, bytes)| {
                    current.parent == self.target.parent
                        && current.file == self.installed
                        && current.mode == self.mode
                        && bytes == self.bytes
                },
            );
        let metadata_preserved = [(&recovery, exchange), (&directory, name.as_os_str())]
            .into_iter()
            .all(|(directory, name)| {
                open_read_at(directory, name).is_ok_and(|file| {
                    metadata_digest(&file).is_ok_and(|digest| digest == self.metadata_digest)
                })
            });
        Ok(metadata_preserved
            && source_durable
            && recovery_durable
            && displaced_matches
            && installed_matches
            && source_current
            && self.recovery.ensure_namespace_identity().is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        root: PathBuf,
        source: PathBuf,
        recovery: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "mesh-retained-{}-{}",
                std::process::id(),
                TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let source = root.join("source");
            let recovery = root.join("recovery");
            fs::create_dir(&source).unwrap();
            fs::create_dir(&recovery).unwrap();
            fs::write(source.join("file"), b"before").unwrap();
            Self {
                root,
                source,
                recovery,
            }
        }
        fn prepare(&self) -> RetainedReplacement {
            let (target, bytes) = read_target(&self.source, "file", 100).unwrap();
            let mode = target.mode;
            let recovery = PinnedWorkspaceRoot::open(self.recovery.clone()).unwrap();
            RetainedReplacement::prepare(
                target,
                bytes,
                b"after".to_vec(),
                mode,
                recovery.clone(),
                |id, _| {
                    recovery.filesystem().write_new_file(
                        Path::new("prepared"),
                        id.token().as_bytes(),
                        fs::Permissions::from_mode(0o600),
                    )
                },
            )
            .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn late_open_editor_writes_remain_named_after_success() {
        let f = Fixture::new();
        let mut editor = OpenOptions::new()
            .append(true)
            .open(f.source.join("file"))
            .unwrap();
        let prepared = f.prepare();
        assert_eq!(fs::read(f.source.join("file")).unwrap(), b"before");
        assert!(prepared.apply().unwrap());
        editor.write_all(b" late editor work").unwrap();
        editor.sync_all().unwrap();
        assert_eq!(
            fs::read(f.recovery.join(EXCHANGE)).unwrap(),
            b"before late editor work"
        );
        assert_eq!(fs::read(f.source.join("file")).unwrap(), b"after");
        assert!(f.recovery.join("prepared").is_file());
    }

    #[test]
    fn race_at_exchange_keeps_unexpected_displaced_work() {
        let f = Fixture::new();
        let prepared = f.prepare();
        assert!(!prepared
            .apply_with_hooks(
                || {
                    fs::write(f.source.join("other"), b"racing replacement").unwrap();
                    fs::rename(f.source.join("other"), f.source.join("file")).unwrap();
                },
                || {},
                File::sync_all
            )
            .unwrap());
        assert_eq!(
            fs::read(f.recovery.join(EXCHANGE)).unwrap(),
            b"racing replacement"
        );
        assert_eq!(fs::read(f.source.join("file")).unwrap(), b"after");
    }

    #[test]
    fn post_exchange_edits_and_barrier_failure_are_preserved_without_rollback() {
        let f = Fixture::new();
        let mut editor = OpenOptions::new()
            .append(true)
            .open(f.source.join("file"))
            .unwrap();
        let prepared = f.prepare();
        assert!(!prepared
            .apply_with_hooks(
                || {},
                || {
                    editor.write_all(b" through old handle").unwrap();
                    fs::write(f.source.join("file"), b"new live work").unwrap();
                },
                |_| Err(io::Error::other("simulated sync failure"))
            )
            .unwrap());
        assert_eq!(
            fs::read(f.recovery.join(EXCHANGE)).unwrap(),
            b"before through old handle"
        );
        assert_eq!(fs::read(f.source.join("file")).unwrap(), b"new live work");
    }

    #[test]
    fn stale_source_and_modified_stage_refuse_without_consuming_recovery() {
        let f = Fixture::new();
        let prepared = f.prepare();
        fs::write(f.source.join("file"), b"later").unwrap();
        assert!(prepared.apply().is_err());
        assert_eq!(fs::read(f.source.join("file")).unwrap(), b"later");
        assert_eq!(fs::read(f.recovery.join(EXCHANGE)).unwrap(), b"after");
        let g = Fixture::new();
        let prepared = g.prepare();
        fs::write(g.recovery.join(EXCHANGE), b"tampered").unwrap();
        assert!(prepared.apply().is_err());
        assert_eq!(fs::read(g.source.join("file")).unwrap(), b"before");
    }

    #[test]
    fn receipt_failure_and_abandonment_never_change_source() {
        let f = Fixture::new();
        let (target, bytes) = read_target(&f.source, "file", 100).unwrap();
        let mode = target.mode;
        assert!(RetainedReplacement::prepare(
            target,
            bytes,
            b"after".to_vec(),
            mode,
            PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap(),
            |_, _| Err(io::Error::other("receipt durability failed"))
        )
        .is_err());
        assert_eq!(fs::read(f.source.join("file")).unwrap(), b"before");
        assert_eq!(fs::read(f.recovery.join(EXCHANGE)).unwrap(), b"after");
        let g = Fixture::new();
        drop(g.prepare());
        assert_eq!(fs::read(g.source.join("file")).unwrap(), b"before");
        assert_eq!(fs::read(g.recovery.join(EXCHANGE)).unwrap(), b"after");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_extended_metadata_and_acl_survive_replacement() {
        let f = Fixture::new();
        let source = f.source.join("file");
        assert!(std::process::Command::new("/usr/bin/xattr")
            .args(["-w", "user.mesh-test", "private metadata"])
            .arg(&source)
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("/bin/chmod")
            .args(["+a", "everyone allow read"])
            .arg(&source)
            .status()
            .unwrap()
            .success());
        let before = metadata_digest(&File::open(&source).unwrap()).unwrap();
        assert!(f.prepare().apply().unwrap());
        assert_eq!(
            metadata_digest(&File::open(&source).unwrap()).unwrap(),
            before
        );
        assert_eq!(
            metadata_digest(&File::open(f.recovery.join(EXCHANGE)).unwrap()).unwrap(),
            before
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn late_metadata_changes_refuse_or_require_reconciliation() {
        let f = Fixture::new();
        let prepared = f.prepare();
        let change = || {
            assert!(std::process::Command::new("/usr/bin/xattr")
                .args(["-w", "user.mesh-test", "new metadata"])
                .arg(f.source.join("file"))
                .status()
                .unwrap()
                .success());
        };
        change();
        assert!(prepared.apply().is_err());
        assert_eq!(fs::read(f.source.join("file")).unwrap(), b"before");
        let g = Fixture::new();
        let prepared = g.prepare();
        assert!(!prepared
            .apply_with_hooks(
                || {
                    assert!(std::process::Command::new("/usr/bin/xattr")
                        .args(["-w", "user.mesh-test", "racing metadata"])
                        .arg(g.source.join("file"))
                        .status()
                        .unwrap()
                        .success());
                },
                || {},
                File::sync_all
            )
            .unwrap());
        let old = std::process::Command::new("/usr/bin/xattr")
            .args(["-p", "user.mesh-test"])
            .arg(g.recovery.join(EXCHANGE))
            .output()
            .unwrap();
        assert!(old.status.success());
        assert_eq!(old.stdout, b"racing metadata\n");
    }

    #[test]
    fn replaced_source_parent_is_not_adopted() {
        let f = Fixture::new();
        let prepared = f.prepare();
        fs::rename(&f.source, f.root.join("moved-source")).unwrap();
        fs::create_dir(&f.source).unwrap();
        fs::write(f.source.join("file"), b"before").unwrap();
        assert!(prepared.apply().is_err());
        assert_eq!(fs::read(f.source.join("file")).unwrap(), b"before");
        assert_eq!(
            fs::read(f.root.join("moved-source/file")).unwrap(),
            b"before"
        );
    }
}
