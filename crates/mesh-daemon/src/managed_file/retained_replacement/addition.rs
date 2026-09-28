//! A private staged file can populate an absent attached path without replacing concurrent work.
use super::*;

pub(crate) struct RetainedAddition {
    source: PinnedWorkspaceRoot,
    relative: PathBuf,
    parent: String,
    bytes: Vec<u8>,
    recovery: PinnedWorkspaceRoot,
    installed: ManagedFileIdentity,
    mode: u32,
    metadata: String,
}

impl RetainedAddition {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare(
        source: PinnedWorkspaceRoot,
        relative: PathBuf,
        parent: String,
        bytes: Vec<u8>,
        executable: bool,
        recovery: PinnedWorkspaceRoot,
        receipt: impl FnOnce(ManagedFileIdentity, u32, &str) -> io::Result<()>,
    ) -> io::Result<Self> {
        if absent_parent(&source, &relative)?.as_deref() != Some(&parent) {
            return Err(io::Error::other(
                "addition destination changed before preparation",
            ));
        }
        recovery.ensure_namespace_identity()?;
        if source.identity()?.0 != recovery.identity()?.0 {
            return Err(io::Error::other(
                "recovery must share the source filesystem",
            ));
        }
        let directory = recovery.try_clone_directory()?;
        let mut stage = create_new_at(&directory, EXCHANGE)?;
        // New files use the same portable creation modes as native export. There is no existing
        // source metadata to copy; the actual staged native metadata is bound into the receipt.
        stage.set_permissions(fs::Permissions::from_mode(if executable {
            0o755
        } else {
            0o644
        }))?;
        stage.write_all(&bytes)?;
        stage.sync_all()?;
        let mode = stage.metadata()?.mode();
        if mode != if executable { 0o100755 } else { 0o100644 } {
            return Err(io::Error::other("addition mode was not preserved"));
        }
        let installed = managed_file_identity(&stage, &stage.metadata()?)?;
        let metadata = metadata_digest(&stage)?;
        directory.sync_all()?;
        let prepared = Self {
            source,
            relative,
            parent,
            bytes,
            recovery,
            installed,
            mode,
            metadata,
        };
        prepared.validate()?;
        receipt(installed, mode, &prepared.metadata)?;
        prepared.validate()?;
        Ok(prepared)
    }

    pub(crate) fn replacement_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn validate(&self) -> io::Result<()> {
        self.validated_target().map(|_| ())
    }

    fn validated_target(&self) -> io::Result<(File, std::ffi::OsString)> {
        self.source.ensure_namespace_identity()?;
        let (directory, entry) = self
            .source
            .filesystem()
            .inspect_optional_entry_with_parent(&self.relative)?;
        let metadata = directory.metadata()?;
        if entry.is_some()
            || (ManagedDirectoryIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
            })
            .token()
                != self.parent
        {
            return Err(io::Error::other(
                "addition destination is occupied or replaced",
            ));
        }
        self.recovery.ensure_namespace_identity()?;
        if metadata.dev() != self.recovery.identity()?.0 {
            return Err(io::Error::other(
                "addition parent and recovery must share the filesystem",
            ));
        }
        let recovery = self.recovery.try_clone_directory()?;
        let name = std::ffi::OsStr::new(EXCHANGE);
        if !matches(&recovery, name, self.installed, self.mode, &self.bytes)
            || metadata_digest(&open_read_at(&recovery, name)?)? != self.metadata
        {
            return Err(io::Error::other("addition stage changed"));
        }
        let name = self
            .relative
            .file_name()
            .ok_or_else(|| io::Error::other("addition has no leaf"))?
            .to_owned();
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
        use mesh_types::ContentDigest as _;
        let (directory, name) = self.validated_target()?;
        let recovery = self.recovery.try_clone_directory()?;
        before();
        atomic_rename_noreplace_at(&recovery, std::ffi::OsStr::new(EXCHANGE), &directory, &name)?;
        after();
        let source_durable = sync(&directory).is_ok();
        let recovery_durable = sync(&recovery).is_ok();
        let installed_matches = observe_file(&self.source, &self.relative, self.bytes.len() as u64)
            .is_ok_and(|current| {
                current.parent == self.parent
                    && current.installation == self.installed.token()
                    && current.mode == self.mode
                    && current.metadata == self.metadata
                    && current.digest == mesh_types::Blake3::digest_bytes(&self.bytes).to_string()
            });
        // No undo or cleanup follows uncertainty. A user can edit or replace the newly created
        // path immediately, and any remaining stage may contain independent concurrent work.
        Ok(source_durable
            && recovery_durable
            && installed_matches
            && absent_parent(&self.recovery, Path::new(EXCHANGE))
                .is_ok_and(|parent| parent.is_some()))
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::Fixture;
    use super::*;

    fn prepare(f: &Fixture) -> RetainedAddition {
        let root = PinnedWorkspaceRoot::open(f.source.clone()).unwrap();
        let relative = PathBuf::from("new.txt");
        let parent = absent_parent(&root, &relative).unwrap().unwrap();
        let recovery = PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap();
        RetainedAddition::prepare(
            root,
            relative,
            parent,
            b"approved".to_vec(),
            true,
            recovery.clone(),
            |id, _, _| {
                recovery.filesystem().write_new_file(
                    Path::new("prepared"),
                    id.token().as_bytes(),
                    fs::Permissions::from_mode(0o600),
                )
            },
        )
        .unwrap()
    }

    #[test]
    fn addition_prepares_without_source_writes_and_installs_exact_executable_file() {
        let f = Fixture::new();
        let addition = prepare(&f);
        assert!(!f.source.join("new.txt").exists());
        assert_eq!(fs::read(f.recovery.join(EXCHANGE)).unwrap(), b"approved");
        assert!(addition.apply().unwrap());
        assert_eq!(fs::read(f.source.join("new.txt")).unwrap(), b"approved");
        assert_eq!(
            fs::metadata(f.source.join("new.txt")).unwrap().mode() & 0o777,
            0o755
        );
        assert!(!f.recovery.join(EXCHANGE).exists());
        assert_eq!(fs::read(f.source.join("file")).unwrap(), b"before");
    }

    #[test]
    fn concurrent_creation_before_or_at_rename_preserves_both_files() {
        for at_rename in [false, true] {
            let f = Fixture::new();
            let addition = prepare(&f);
            if !at_rename {
                fs::write(f.source.join("new.txt"), b"user work").unwrap();
            }
            assert!(addition
                .apply_with_hooks(
                    || {
                        if at_rename {
                            fs::write(f.source.join("new.txt"), b"user work").unwrap();
                        }
                    },
                    || {},
                    File::sync_all
                )
                .is_err());
            assert_eq!(fs::read(f.source.join("new.txt")).unwrap(), b"user work");
            assert_eq!(fs::read(f.recovery.join(EXCHANGE)).unwrap(), b"approved");
        }
    }

    #[test]
    fn changed_stage_or_replaced_root_refuses_without_installing() {
        for changed_stage in [false, true] {
            let f = Fixture::new();
            let addition = prepare(&f);
            if changed_stage {
                fs::write(f.recovery.join(EXCHANGE), b"changed").unwrap();
            } else {
                fs::rename(&f.source, f.root.join("moved")).unwrap();
                fs::create_dir(&f.source).unwrap();
            }
            assert!(addition.apply().is_err());
            assert!(!f.source.join("new.txt").exists());
            assert!(f.recovery.join(EXCHANGE).is_file());
        }
    }

    #[test]
    fn parent_replacement_refuses_and_failed_receipt_preserves_only_the_stage() {
        let f = Fixture::new();
        fs::create_dir(f.source.join("nested")).unwrap();
        let root = PinnedWorkspaceRoot::open(f.source.clone()).unwrap();
        let relative = PathBuf::from("nested/new.txt");
        let parent = absent_parent(&root, &relative).unwrap().unwrap();
        let recovery = PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap();
        let addition = RetainedAddition::prepare(
            root,
            relative,
            parent,
            b"approved".to_vec(),
            false,
            recovery,
            |_, _, _| Ok(()),
        )
        .unwrap();
        fs::rename(f.source.join("nested"), f.source.join("original")).unwrap();
        fs::create_dir(f.source.join("nested")).unwrap();
        assert!(addition.apply().is_err());
        assert!(!f.source.join("nested/new.txt").exists());
        assert!(!f.source.join("original/new.txt").exists());
        assert_eq!(fs::read(f.recovery.join(EXCHANGE)).unwrap(), b"approved");

        let f = Fixture::new();
        let root = PinnedWorkspaceRoot::open(f.source.clone()).unwrap();
        let parent = absent_parent(&root, Path::new("new.txt")).unwrap().unwrap();
        let recovery = PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap();
        assert!(RetainedAddition::prepare(
            root,
            PathBuf::from("new.txt"),
            parent,
            b"approved".to_vec(),
            false,
            recovery,
            |_, _, _| Err(io::Error::other("receipt failed"))
        )
        .is_err());
        assert!(!f.source.join("new.txt").exists());
        assert_eq!(fs::read(f.recovery.join(EXCHANGE)).unwrap(), b"approved");
    }

    #[test]
    fn post_install_edits_and_failed_durability_are_uncertain_and_never_undone() {
        let f = Fixture::new();
        let barriers = std::cell::Cell::new(0);
        assert!(!prepare(&f)
            .apply_with_hooks(
                || {},
                || {
                    fs::write(f.source.join("new.txt"), b"new user work").unwrap();
                },
                |_| {
                    barriers.set(barriers.get() + 1);
                    Err(io::Error::other("injected barrier failure"))
                }
            )
            .unwrap());
        assert_eq!(barriers.get(), 2);
        assert_eq!(
            fs::read(f.source.join("new.txt")).unwrap(),
            b"new user work"
        );
        assert!(!f.recovery.join(EXCHANGE).exists());
    }
}
