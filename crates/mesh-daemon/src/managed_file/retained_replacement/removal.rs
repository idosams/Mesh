//! Removing an attached path retains its inode for editors that still hold the file open.
use super::*;

/// A successful observation of absence includes the exact existing confined parent.
pub(crate) fn absent_parent(
    root: &PinnedWorkspaceRoot,
    relative: &Path,
) -> io::Result<Option<String>> {
    root.ensure_namespace_identity()?;
    let (parent, entry) = root
        .filesystem()
        .inspect_optional_entry_with_parent(relative)?;
    let metadata = parent.metadata()?;
    let (latest_parent, latest_entry) = root
        .filesystem()
        .inspect_optional_entry_with_parent(relative)?;
    let latest_metadata = latest_parent.metadata()?;
    if (metadata.dev(), metadata.ino()) != (latest_metadata.dev(), latest_metadata.ino()) {
        return Err(io::Error::other(
            "absence parent changed during observation",
        ));
    }
    root.ensure_namespace_identity()?;
    Ok((entry.is_none() && latest_entry.is_none()).then(|| {
        ManagedDirectoryIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
        .token()
    }))
}

pub(crate) struct RetainedRemoval {
    target: ManagedReplacementTarget,
    expected: Vec<u8>,
    source: PinnedWorkspaceRoot,
    recovery: PinnedWorkspaceRoot,
    metadata_digest: String,
}

impl RetainedRemoval {
    pub(crate) fn prepare(
        target: ManagedReplacementTarget,
        expected: Vec<u8>,
        recovery: PinnedWorkspaceRoot,
        receipt: impl FnOnce(&str) -> io::Result<()>,
    ) -> io::Result<Self> {
        if target.mode & 0o7000 != 0 {
            return Err(io::Error::other(
                "special permission bits require explicit metadata support",
            ));
        }
        let source = PinnedWorkspaceRoot::open(target.root.clone())?;
        if recovery.identity()?.0 != target.file.device {
            return Err(io::Error::other(
                "recovery must share the source filesystem",
            ));
        }
        let file = source
            .filesystem()
            .inspect_entry(Path::new(&target.relative))?;
        let metadata_digest = metadata_digest(&file)?;
        let prepared = Self {
            target,
            expected,
            source,
            recovery,
            metadata_digest,
        };
        prepared.validate()?;
        receipt(&prepared.metadata_digest)?;
        prepared.validate()?;
        Ok(prepared)
    }

    pub(crate) fn current_bytes(&self) -> &[u8] {
        &self.expected
    }

    pub(crate) fn validate(&self) -> io::Result<()> {
        self.validated_target().map(|_| ())
    }

    fn validated_target(&self) -> io::Result<(File, std::ffi::OsString)> {
        self.source.ensure_namespace_identity()?;
        let (path, directory, name, metadata, identity, bytes) =
            read_confined_regular_file_in_layout_bounded(
                &self.target.root,
                &self.target.relative,
                self.target.reserve_private_top_level,
                self.expected.len(),
            )
            .map_err(io::Error::other)?;
        let parent = directory.metadata()?;
        if path != self.target.path
            || parent.dev() != self.target.parent.device
            || parent.ino() != self.target.parent.inode
            || identity != self.target.file
            || metadata.mode() != self.target.mode
            || bytes != self.expected
            || metadata_digest(&open_read_at(&directory, &name)?)? != self.metadata_digest
        {
            return Err(io::Error::other("removal source changed since preparation"));
        }
        if absent_parent(&self.recovery, Path::new(EXCHANGE))?.is_none() {
            return Err(io::Error::other("removal recovery destination is occupied"));
        }
        self.source.ensure_namespace_identity()?;
        Ok((directory, name))
    }

    pub(crate) fn apply(self) -> io::Result<bool> {
        self.apply_with_hooks(|| {}, || {}, File::sync_all)
    }

    fn apply_with_hooks(
        self,
        before: impl FnOnce(),
        after: impl FnOnce(),
        sync: impl Fn(&File) -> io::Result<()>,
    ) -> io::Result<bool> {
        let (directory, name) = self.validated_target()?;
        let recovery = self.recovery.try_clone_directory()?;
        let exchange = std::ffi::OsStr::new(EXCHANGE);
        before();
        atomic_rename_noreplace_at(&directory, &name, &recovery, exchange)?;
        after();
        // Both directories must be flushed. Never unlink or automatically move back an inode:
        // another process can keep editing it or recreate the original name at any time.
        let source_durable = sync(&directory).is_ok();
        let recovery_durable = sync(&recovery).is_ok();
        let retained = matches(
            &recovery,
            exchange,
            self.target.file,
            self.target.mode,
            &self.expected,
        ) && open_read_at(&recovery, exchange).is_ok_and(|file| {
            metadata_digest(&file).is_ok_and(|digest| digest == self.metadata_digest)
        });
        let absent = absent_parent(&self.source, Path::new(&self.target.relative))
            .is_ok_and(|parent| parent.as_deref() == Some(&self.target.parent.token()));
        Ok(source_durable
            && recovery_durable
            && retained
            && absent
            && self.recovery.ensure_namespace_identity().is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::Fixture;
    use super::*;

    fn prepare(f: &Fixture) -> RetainedRemoval {
        let (target, bytes) = read_target(&f.source, "file", 100).unwrap();
        let recovery = PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap();
        RetainedRemoval::prepare(target, bytes, recovery.clone(), |digest| {
            recovery.filesystem().write_new_file(
                Path::new("prepared"),
                digest.as_bytes(),
                fs::Permissions::from_mode(0o600),
            )
        })
        .unwrap()
    }

    #[test]
    fn removal_retains_late_open_editor_writes() {
        let f = Fixture::new();
        let mut editor = OpenOptions::new()
            .append(true)
            .open(f.source.join("file"))
            .unwrap();
        let removal = prepare(&f);
        assert!(f.source.join("file").is_file());
        assert!(removal.apply().unwrap());
        assert!(!f.source.join("file").exists());
        editor.write_all(b" later work").unwrap();
        editor.sync_all().unwrap();
        assert_eq!(
            fs::read(f.recovery.join(EXCHANGE)).unwrap(),
            b"before later work"
        );
    }

    #[test]
    fn changed_source_or_occupied_recovery_refuses_without_mutation() {
        for source_changed in [false, true] {
            let f = Fixture::new();
            let removal = prepare(&f);
            if source_changed {
                fs::write(f.source.join("file"), b"edited").unwrap();
            } else {
                fs::write(f.recovery.join(EXCHANGE), b"unrelated").unwrap();
            }
            assert!(removal.apply().is_err());
            assert_eq!(
                fs::read(f.source.join("file")).unwrap(),
                if source_changed { b"edited" } else { b"before" }
            );
            if !source_changed {
                assert_eq!(fs::read(f.recovery.join(EXCHANGE)).unwrap(), b"unrelated");
            }
        }
    }

    #[test]
    fn raced_removal_preserves_displaced_and_recreated_work() {
        let f = Fixture::new();
        let removal = prepare(&f);
        assert!(!removal
            .apply_with_hooks(
                || fs::write(f.source.join("file"), b"changed at rename").unwrap(),
                || fs::write(f.source.join("file"), b"recreated").unwrap(),
                File::sync_all,
            )
            .unwrap());
        assert_eq!(
            fs::read(f.recovery.join(EXCHANGE)).unwrap(),
            b"changed at rename"
        );
        assert_eq!(fs::read(f.source.join("file")).unwrap(), b"recreated");
    }

    #[test]
    fn substitution_at_rename_retains_both_inodes_without_rollback() {
        let f = Fixture::new();
        let removal = prepare(&f);
        assert!(!removal
            .apply_with_hooks(
                || {
                    fs::rename(f.source.join("file"), f.source.join("saved-by-editor")).unwrap();
                    fs::write(f.source.join("file"), b"new inode").unwrap();
                },
                || {},
                File::sync_all
            )
            .unwrap());
        assert_eq!(
            fs::read(f.source.join("saved-by-editor")).unwrap(),
            b"before"
        );
        assert_eq!(fs::read(f.recovery.join(EXCHANGE)).unwrap(), b"new inode");
    }

    #[test]
    fn recovery_occupied_at_rename_never_overwrites_the_destination() {
        let f = Fixture::new();
        let removal = prepare(&f);
        assert!(removal
            .apply_with_hooks(
                || {
                    fs::write(f.recovery.join(EXCHANGE), b"unrelated").unwrap();
                },
                || {},
                File::sync_all
            )
            .is_err());
        assert_eq!(fs::read(f.source.join("file")).unwrap(), b"before");
        assert_eq!(fs::read(f.recovery.join(EXCHANGE)).unwrap(), b"unrelated");
    }

    #[test]
    fn failed_barrier_still_flushes_both_directories_and_retains_file() {
        let f = Fixture::new();
        let calls = std::cell::Cell::new(0);
        assert!(!prepare(&f)
            .apply_with_hooks(
                || {},
                || {},
                |_| {
                    calls.set(calls.get() + 1);
                    Err(io::Error::other("injected sync failure"))
                }
            )
            .unwrap());
        assert_eq!(calls.get(), 2);
        assert_eq!(fs::read(f.recovery.join(EXCHANGE)).unwrap(), b"before");
    }

    #[test]
    fn absence_does_not_mean_missing_parent_symlink_or_replaced_root() {
        let f = Fixture::new();
        let root = PinnedWorkspaceRoot::open(f.source.clone()).unwrap();
        assert!(absent_parent(&root, Path::new("missing/file")).is_err());
        std::os::unix::fs::symlink("missing", f.source.join("link")).unwrap();
        assert!(!matches!(
            absent_parent(&root, Path::new("link")),
            Ok(Some(_))
        ));
        let removal = prepare(&f);
        fs::rename(&f.source, f.root.join("moved")).unwrap();
        fs::create_dir(&f.source).unwrap();
        assert!(removal.apply().is_err());
        assert!(absent_parent(&root, Path::new("file")).is_err());
        assert_eq!(fs::read(f.root.join("moved/file")).unwrap(), b"before");
    }
}
