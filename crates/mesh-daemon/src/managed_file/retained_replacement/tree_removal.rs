//! Removing an approved tree retains the directory and every inode, including late descriptor writes.
use super::*;
use crate::ipc::Json;
use std::ffi::OsStr;

pub(crate) struct RetainedTreeRemoval {
    source: PinnedWorkspaceRoot,
    relative: PathBuf,
    recovery: PinnedWorkspaceRoot,
    evidence: Json,
    parent: String,
    policy: String,
    mode: u32,
    entries: usize,
    bytes: u64,
    file_bytes: u64,
}
impl RetainedTreeRemoval {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare(
        source: PinnedWorkspaceRoot,
        relative: PathBuf,
        recovery: PinnedWorkspaceRoot,
        entries: usize,
        bytes: u64,
        file_bytes: u64,
        receipt: impl FnOnce(&Json, &str, &str, u32) -> io::Result<()>,
    ) -> io::Result<Self> {
        let (policy, mode, parent) = parent_policy(&source, &relative)?;
        if recovery.identity()?.0 != source.identity()?.0 {
            return Err(io::Error::other(
                "tree recovery must share the source filesystem",
            ));
        }
        let tree = open_tree(&source, &relative)?;
        let evidence = observe_tree(
            &tree,
            entries.min(64),
            bytes.min(64 * 1024 * 1024),
            file_bytes,
        )?;
        let prepared = Self {
            source,
            relative,
            recovery,
            evidence,
            parent,
            policy,
            mode,
            entries: entries.min(64),
            bytes: bytes.min(64 * 1024 * 1024),
            file_bytes,
        };
        prepared.validate()?;
        // The caller rederives every raw entry from approved history before recording authority.
        receipt(
            &prepared.evidence,
            &prepared.parent,
            &prepared.policy,
            prepared.mode,
        )?;
        prepared.validate()?;
        Ok(prepared)
    }
    fn target(&self) -> io::Result<File> {
        self.source.ensure_namespace_identity()?;
        self.recovery.ensure_namespace_identity()?;
        let (parent, _) = self
            .source
            .filesystem()
            .inspect_optional_entry_with_parent(&self.relative)?;
        let matches_parent = || {
            parent_policy(&self.source, &self.relative).is_ok_and(|(policy, mode, parent)| {
                policy == self.policy && mode == self.mode && parent == self.parent
            })
        };
        if !matches_parent() || absent_parent(&self.recovery, Path::new(EXCHANGE))?.is_none() {
            return Err(io::Error::other(
                "tree parent changed or recovery destination is occupied",
            ));
        }
        let tree = open_tree(&self.source, &self.relative)?;
        if observe_tree(&tree, self.entries, self.bytes, self.file_bytes)? != self.evidence
            || !matches_parent()
        {
            return Err(io::Error::other("removal tree changed"));
        }
        // Bind the syscall's held parent, not only the separately observed pathname.
        let stat = parent.metadata()?;
        if (ManagedDirectoryIdentity {
            device: stat.dev(),
            inode: stat.ino(),
        })
        .token()
            != self.parent
        {
            return Err(io::Error::other("retained removal parent changed"));
        }
        self.source.ensure_namespace_identity()?;
        self.recovery.ensure_namespace_identity()?;
        Ok(parent)
    }
    pub(crate) fn validate(&self) -> io::Result<()> {
        self.target().map(|_| ())
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
        let parent = self.target()?;
        let recovery = self.recovery.try_clone_directory()?;
        let name = self
            .relative
            .file_name()
            .ok_or_else(|| io::Error::other("removal has no leaf"))?;
        before();
        atomic_rename_noreplace_at(&parent, name, &recovery, OsStr::new(EXCHANGE))?;
        after();
        let durable = sync(&parent).is_ok() & sync(&recovery).is_ok();
        let retained = open_tree(&self.recovery, Path::new(EXCHANGE))
            .and_then(|tree| observe_tree(&tree, self.entries, self.bytes, self.file_bytes));
        let absent = absent_parent(&self.source, &self.relative)
            .is_ok_and(|parent| parent.as_deref() == Some(self.parent.as_str()));
        let policy =
            parent_policy(&self.source, &self.relative).is_ok_and(|(policy, mode, parent)| {
                policy == self.policy && mode == self.mode && parent == self.parent
            });
        Ok(durable
            && absent
            && policy
            && retained.is_ok_and(|tree| tree == self.evidence)
            && self.recovery.ensure_namespace_identity().is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::Fixture;
    use super::*;
    fn prepare(f: &Fixture) -> RetainedTreeRemoval {
        fs::create_dir_all(f.source.join("tree/empty")).unwrap();
        fs::write(f.source.join("tree/file"), b"approved").unwrap();
        RetainedTreeRemoval::prepare(
            PinnedWorkspaceRoot::open(f.source.clone()).unwrap(),
            PathBuf::from("tree"),
            PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap(),
            64,
            100,
            100,
            |_, _, _, _| Ok(()),
        )
        .unwrap()
    }
    #[test]
    fn retained_tree_keeps_open_files_and_directory_relative_writes_alive() {
        let f = Fixture::new();
        let prepared = prepare(&f);
        let held = File::open(f.source.join("tree")).unwrap();
        let mut editor = OpenOptions::new()
            .append(true)
            .open(f.source.join("tree/file"))
            .unwrap();
        assert!(prepared.apply().unwrap());
        assert!(!f.source.join("tree").exists());
        editor.write_all(b" late").unwrap();
        create_new_at_mode(&held, "new-user-file", 0o600)
            .unwrap()
            .write_all(b"keep")
            .unwrap();
        assert_eq!(
            fs::read(f.recovery.join("exchange/file")).unwrap(),
            b"approved late"
        );
        assert_eq!(
            fs::read(f.recovery.join("exchange/new-user-file")).unwrap(),
            b"keep"
        );
        assert!(f.recovery.join("exchange/empty").is_dir());
    }
    #[test]
    fn unknown_entries_and_destination_collisions_are_retained_without_source_cleanup() {
        for at_boundary in [false, true] {
            let f = Fixture::new();
            let prepared = prepare(&f);
            if !at_boundary {
                fs::write(f.source.join("tree/user"), b"keep").unwrap();
            }
            assert!(prepared
                .apply_with_hooks(
                    || {
                        if at_boundary {
                            fs::create_dir(f.recovery.join("exchange")).unwrap();
                        }
                    },
                    || {},
                    File::sync_all
                )
                .is_err());
            assert_eq!(fs::read(f.source.join("tree/file")).unwrap(), b"approved");
            assert!(f.source.join("tree/empty").is_dir());
            if !at_boundary {
                assert_eq!(fs::read(f.source.join("tree/user")).unwrap(), b"keep");
            }
        }
    }
    #[test]
    fn boundary_edits_recreation_and_sync_failure_preserve_work_without_undo() {
        for change in ["edit", "new-child", "substitute", "recreate", "sync"] {
            let f = Fixture::new();
            let prepared = prepare(&f);
            let calls = std::cell::Cell::new(0);
            assert!(!prepared
                .apply_with_hooks(
                    || {
                        match change {
                            "edit" => fs::write(f.source.join("tree/file"), b"late edit").unwrap(),
                            "new-child" => fs::write(f.source.join("tree/user"), b"keep").unwrap(),
                            "substitute" => {
                                fs::rename(f.source.join("tree"), f.source.join("original"))
                                    .unwrap();
                                fs::create_dir(f.source.join("tree")).unwrap();
                                fs::write(f.source.join("tree/user"), b"keep").unwrap();
                            }
                            _ => {}
                        }
                    },
                    || {
                        if change == "recreate" {
                            fs::create_dir(f.source.join("tree")).unwrap();
                            fs::write(f.source.join("tree/user"), b"keep").unwrap();
                        }
                    },
                    |file| {
                        calls.set(calls.get() + 1);
                        if change == "sync" {
                            Err(io::Error::other("injected sync failure"))
                        } else {
                            file.sync_all()
                        }
                    }
                )
                .unwrap());
            assert_eq!(calls.get(), 2);
            if change == "edit" {
                assert_eq!(
                    fs::read(f.recovery.join("exchange/file")).unwrap(),
                    b"late edit"
                );
            }
            if change == "new-child" || change == "substitute" {
                assert_eq!(fs::read(f.recovery.join("exchange/user")).unwrap(), b"keep");
            }
            if change == "substitute" {
                assert_eq!(
                    fs::read(f.source.join("original/file")).unwrap(),
                    b"approved"
                );
            }
            if change == "recreate" {
                assert_eq!(fs::read(f.source.join("tree/user")).unwrap(), b"keep");
            }
            assert!(f.recovery.join("exchange").is_dir());
        }
    }
}
