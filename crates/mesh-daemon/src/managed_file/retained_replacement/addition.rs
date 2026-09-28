//! A private staged file can populate an absent attached path without replacing concurrent work.
use super::*;

pub(crate) fn parent_policy(
    root: &PinnedWorkspaceRoot,
    relative: &Path,
) -> io::Result<(String, u32, String)> {
    root.ensure_namespace_identity()?;
    let (directory, _) = root
        .filesystem()
        .inspect_optional_entry_with_parent(relative)?;
    let before = directory.metadata()?;
    let policy = metadata_digest(&directory)?;
    let (current, _) = root
        .filesystem()
        .inspect_optional_entry_with_parent(relative)?;
    let same = |a: &fs::Metadata, b: &fs::Metadata| {
        (a.dev(), a.ino(), a.mode(), a.ctime(), a.ctime_nsec())
            == (b.dev(), b.ino(), b.mode(), b.ctime(), b.ctime_nsec())
    };
    if !same(&before, &directory.metadata()?) || !same(&before, &current.metadata()?) {
        return Err(io::Error::other("parent policy changed during observation"));
    }
    root.ensure_namespace_identity()?;
    Ok((
        policy,
        before.mode(),
        ManagedDirectoryIdentity {
            device: before.dev(),
            inode: before.ino(),
        }
        .token(),
    ))
}

pub(crate) struct RetainedAddition {
    source: PinnedWorkspaceRoot,
    relative: PathBuf,
    parent: String,
    bytes: Vec<u8>,
    recovery: PinnedWorkspaceRoot,
    installed: ManagedFileIdentity,
    mode: u32,
    metadata: String,
    parent_metadata: String,
    parent_mode: u32,
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
        receipt: impl FnOnce(ManagedFileIdentity, u32, &str, &str, u32) -> io::Result<()>,
    ) -> io::Result<Self> {
        Self::prepare_inner(
            source, relative, parent, bytes, executable, recovery, None, receipt,
        )
    }

    /// Restore a private retained snapshot into an absent path, preserving its exact metadata.
    /// The original retained inode is never moved or consumed.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_restoration(
        source: PinnedWorkspaceRoot,
        relative: PathBuf,
        parent: String,
        bytes: Vec<u8>,
        mode: u32,
        recovery: PinnedWorkspaceRoot,
        retained: &File,
        receipt: impl FnOnce(ManagedFileIdentity, u32, &str, &str, u32) -> io::Result<()>,
    ) -> io::Result<Self> {
        if mode & !0o100777 != 0 || mode & 0o100000 == 0 {
            return Err(io::Error::other("unsupported restoration mode"));
        }
        Self::prepare_inner(
            source,
            relative,
            parent,
            bytes,
            mode & 0o111 != 0,
            recovery,
            Some((retained, mode)),
            receipt,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_inner(
        source: PinnedWorkspaceRoot,
        relative: PathBuf,
        parent: String,
        bytes: Vec<u8>,
        executable: bool,
        recovery: PinnedWorkspaceRoot,
        retained: Option<(&File, u32)>,
        receipt: impl FnOnce(ManagedFileIdentity, u32, &str, &str, u32) -> io::Result<()>,
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
        // Let the kernel apply the process umask without reading or mutating process-global state.
        let requested_mode = if retained.is_some() {
            0o600
        } else if executable {
            0o755
        } else {
            0o644
        };
        let mut stage = create_new_at_mode(&directory, EXCHANGE, requested_mode)?;
        let effective_mode =
            retained.map_or(stage.metadata()?.mode() & 0o777, |(_, mode)| mode & 0o777);
        let (parent_directory, entry) = source
            .filesystem()
            .inspect_optional_entry_with_parent(&relative)?;
        if entry.is_some() {
            return Err(io::Error::other(
                "addition destination changed during staging",
            ));
        }
        stage.write_all(&bytes)?;
        let (parent_metadata, parent_mode) = if let Some((retained, _)) = retained {
            let policy = metadata_digest(&parent_directory)?;
            let parent_mode = parent_directory.metadata()?.mode();
            copy_metadata(retained, &stage)?;
            stage.set_permissions(fs::Permissions::from_mode(effective_mode))?;
            (policy, parent_mode)
        } else {
            metadata::inherit_new_file(&parent_directory, &stage, effective_mode)?
        };
        stage.sync_all()?;
        let mode = stage.metadata()?.mode();
        if mode != (0o100000 | effective_mode) || (mode & 0o111 != 0) != executable {
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
            parent_metadata,
            parent_mode,
        };
        prepared.validate()?;
        receipt(
            installed,
            mode,
            &prepared.metadata,
            &prepared.parent_metadata,
            prepared.parent_mode,
        )?;
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
        if metadata.mode() != self.parent_mode
            || metadata_digest(&directory)? != self.parent_metadata
        {
            return Err(io::Error::other("addition parent permissions changed"));
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
        let parent_current = self
            .source
            .filesystem()
            .inspect_optional_entry_with_parent(&self.relative)
            .is_ok_and(|(parent, _)| {
                parent
                    .metadata()
                    .is_ok_and(|m| m.mode() == self.parent_mode)
                    && metadata_digest(&parent).is_ok_and(|digest| digest == self.parent_metadata)
            });
        Ok(source_durable
            && parent_current
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
            |id, _, _, _, _| {
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
        let staged_mode = fs::metadata(f.recovery.join(EXCHANGE)).unwrap().mode() & 0o777;
        assert_ne!(staged_mode & 0o111, 0);
        assert!(addition.apply().unwrap());
        assert_eq!(fs::read(f.source.join("new.txt")).unwrap(), b"approved");
        assert_eq!(
            fs::metadata(f.source.join("new.txt")).unwrap().mode() & 0o777,
            staged_mode
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
            |_, _, _, _, _| Ok(()),
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
            |_, _, _, _, _| Err(io::Error::other("receipt failed"))
        )
        .is_err());
        assert!(!f.source.join("new.txt").exists());
        assert_eq!(fs::read(f.recovery.join(EXCHANGE)).unwrap(), b"approved");
    }

    #[test]
    #[allow(unsafe_code)]
    fn restrictive_umask_is_preserved_in_an_isolated_process() {
        const CHILD: &str = "MESH_INHERITANCE_UMASK_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "managed_file::retained_replacement::addition::tests::restrictive_umask_is_preserved_in_an_isolated_process", "--test-threads=1"])
                .env(CHILD, "1").output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
            return;
        }
        #[cfg(target_os = "macos")]
        type NativeMode = u16;
        #[cfg(not(target_os = "macos"))]
        type NativeMode = u32;
        unsafe extern "C" {
            fn umask(mode: NativeMode) -> NativeMode;
        }
        // SAFETY: only this explicitly isolated single-test child changes process-global umask.
        unsafe {
            umask(0o077);
        }
        for executable in [false, true] {
            let f = Fixture::new();
            let root = PinnedWorkspaceRoot::open(f.source.clone()).unwrap();
            let relative = PathBuf::from("new.txt");
            let parent = absent_parent(&root, &relative).unwrap().unwrap();
            let recovery = PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap();
            let addition = RetainedAddition::prepare(
                root,
                relative,
                parent,
                b"approved".to_vec(),
                executable,
                recovery,
                |_, mode, _, _, _| {
                    assert_eq!(mode & 0o777, if executable { 0o700 } else { 0o600 });
                    Ok(())
                },
            )
            .unwrap();
            assert!(addition.apply().unwrap());
            assert_eq!(
                fs::metadata(f.source.join("new.txt")).unwrap().mode() & 0o777,
                if executable { 0o700 } else { 0o600 }
            );
        }
    }

    #[test]
    fn parent_permission_changes_refuse_before_apply_and_require_reconciliation_after_rename() {
        for at_rename in [false, true] {
            let f = Fixture::new();
            let addition = prepare(&f);
            let original = fs::metadata(&f.source).unwrap().mode();
            let changed = original ^ 0o010;
            if !at_rename {
                fs::set_permissions(&f.source, fs::Permissions::from_mode(changed)).unwrap();
            }
            let outcome = addition.apply_with_hooks(
                || {
                    if at_rename {
                        fs::set_permissions(&f.source, fs::Permissions::from_mode(changed))
                            .unwrap();
                    }
                },
                || {},
                File::sync_all,
            );
            if at_rename {
                assert!(!outcome.unwrap());
                assert_eq!(fs::read(f.source.join("new.txt")).unwrap(), b"approved");
            } else {
                assert!(outcome.is_err());
                assert!(!f.source.join("new.txt").exists());
                assert_eq!(fs::read(f.recovery.join(EXCHANGE)).unwrap(), b"approved");
            }
            fs::set_permissions(&f.source, fs::Permissions::from_mode(original)).unwrap();
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn changed_parent_acl_refuses_staged_addition() {
        let f = Fixture::new();
        let addition = prepare(&f);
        assert!(std::process::Command::new("/bin/chmod")
            .arg("+a")
            .arg("everyone allow read,file_inherit,only_inherit")
            .arg(&f.source)
            .status()
            .unwrap()
            .success());
        assert!(addition.apply().is_err());
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
