//! Restore a frozen copy; the old retained object and any newly displaced object stay recoverable.
use super::*;
use crate::ipc::Json;
use std::ffi::OsStr;

pub(crate) struct RetainedEntryRestoration {
    source: PinnedWorkspaceRoot,
    path: PathBuf,
    origin: PinnedWorkspaceRoot,
    recovery: PinnedWorkspaceRoot,
    pub(crate) original: EntrySnapshot,
    pub(crate) current: Option<EntrySnapshot>,
    pub(crate) staged: Json,
    pub(crate) parent: (String, u32, String),
    limits: EntryLimits,
}

impl RetainedEntryRestoration {
    pub(crate) fn prepare(
        source: PinnedWorkspaceRoot,
        path: PathBuf,
        origin: PinnedWorkspaceRoot,
        recovery: PinnedWorkspaceRoot,
        limits: EntryLimits,
        admit: impl FnOnce(&Json, Option<&Json>) -> io::Result<()>,
    ) -> io::Result<Self> {
        let parent = parent_policy(&source, &path)?;
        let (fd, entry) = source
            .filesystem()
            .inspect_optional_entry_with_parent(&path)?;
        if fd.metadata()?.dev() != recovery.identity()?.0
            || origin.identity()?.0 != recovery.identity()?.0
        {
            return Err(io::Error::other(
                "restoration entries must share a filesystem",
            ));
        }
        let original_evidence = observe_entry(&origin, Path::new(EXCHANGE), limits)?;
        let original_bytes: u64 = original_evidence
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| entry.get("bytes").and_then(Json::as_u64))
            .sum();
        let current_limits = EntryLimits {
            bytes: limits
                .bytes
                .min(64 * 1024 * 1024)
                .saturating_sub(original_bytes),
            ..limits
        };
        let current_evidence = if entry.is_some() {
            Some(observe_entry(&source, &path, current_limits)?)
        } else {
            None
        };
        // Admission sees bounded evidence before private content is frozen or copied into staging.
        admit(&original_evidence, current_evidence.as_ref())?;
        let original = EntrySnapshot::capture(&origin, Path::new(EXCHANGE), limits)?;
        let current = if entry.is_some() {
            Some(EntrySnapshot::capture(
                &source,
                &path,
                EntryLimits {
                    bytes: limits
                        .bytes
                        .min(64 * 1024 * 1024)
                        .saturating_sub(original.bytes()),
                    ..limits
                },
            )?)
        } else {
            None
        };
        if original.evidence != original_evidence
            || current.as_ref().map(|snapshot| &snapshot.evidence) != current_evidence.as_ref()
        {
            return Err(io::Error::other(
                "restoration entries changed after admission",
            ));
        }
        let staged = original.stage(&recovery, limits)?;
        let prepared = Self {
            source,
            path,
            origin,
            recovery,
            original,
            current,
            staged,
            parent,
            limits,
        };
        prepared.validate()?;
        Ok(prepared)
    }
    pub(crate) fn validate(&self) -> io::Result<()> {
        self.target().map(|_| ())
    }
    fn target(&self) -> io::Result<File> {
        self.source.ensure_namespace_identity()?;
        self.origin.ensure_namespace_identity()?;
        self.recovery.ensure_namespace_identity()?;
        let (parent, entry) = self
            .source
            .filesystem()
            .inspect_optional_entry_with_parent(&self.path)?;
        let stat = parent.metadata()?;
        if (ManagedDirectoryIdentity {
            device: stat.dev(),
            inode: stat.ino(),
        })
        .token()
            != self.parent.2
            || parent_policy(&self.source, &self.path)? != self.parent
            || observe_entry(&self.origin, Path::new(EXCHANGE), self.limits)?
                != self.original.evidence
            || observe_entry(&self.recovery, Path::new(EXCHANGE), self.limits)? != self.staged
        {
            return Err(io::Error::other("restoration inputs or parent changed"));
        }
        match (&self.current, entry) {
            (None, None) => {}
            (Some(snapshot), Some(_))
                if observe_entry(&self.source, &self.path, self.limits)? == snapshot.evidence => {}
            _ => return Err(io::Error::other("restoration destination changed")),
        }
        if parent_policy(&self.source, &self.path)? != self.parent {
            return Err(io::Error::other(
                "restoration parent changed during validation",
            ));
        }
        self.origin.ensure_namespace_identity()?;
        self.recovery.ensure_namespace_identity()?;
        self.source.ensure_namespace_identity()?;
        Ok(parent)
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
            .path
            .file_name()
            .ok_or_else(|| io::Error::other("restoration leaf missing"))?;
        before();
        if self.current.is_some() {
            atomic_exchange_at(&parent, name, &recovery, OsStr::new(EXCHANGE))?;
        } else {
            atomic_rename_noreplace_at(&recovery, OsStr::new(EXCHANGE), &parent, name)?;
        }
        after();
        let durable = sync(&parent).is_ok() & sync(&recovery).is_ok();
        let retained = match &self.current {
            Some(snapshot) => observe_entry(&self.recovery, Path::new(EXCHANGE), self.limits)
                .is_ok_and(|value| value == snapshot.evidence),
            None => absent_parent(&self.recovery, Path::new(EXCHANGE))
                .is_ok_and(|value| value.is_some()),
        };
        Ok(durable
            && retained
            && observe_entry(&self.source, &self.path, self.limits)
                .is_ok_and(|value| value == self.staged)
            && observe_entry(&self.origin, Path::new(EXCHANGE), self.limits)
                .is_ok_and(|value| value == self.original.evidence)
            && parent_policy(&self.source, &self.path).is_ok_and(|value| value == self.parent)
            && self.recovery.ensure_namespace_identity().is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::Fixture;
    use super::*;
    fn prepare(f: &Fixture, directory: bool, current: &str) -> RetainedEntryRestoration {
        fs::create_dir(f.source.join("origin")).unwrap();
        if directory {
            fs::create_dir_all(f.source.join("origin/exchange/empty")).unwrap();
            fs::write(f.source.join("origin/exchange/file"), b"retained work").unwrap();
            fs::set_permissions(
                f.source.join("origin/exchange/file"),
                fs::Permissions::from_mode(0o640),
            )
            .unwrap();
        } else {
            fs::write(f.source.join("origin/exchange"), b"retained work").unwrap();
        }
        match current {
            "file" => fs::write(f.source.join("entry"), b"current work").unwrap(),
            "directory" => {
                fs::create_dir(f.source.join("entry")).unwrap();
                fs::write(f.source.join("entry/new"), b"current work").unwrap();
            }
            _ => {}
        }
        RetainedEntryRestoration::prepare(
            PinnedWorkspaceRoot::open(f.source.clone()).unwrap(),
            PathBuf::from("entry"),
            PinnedWorkspaceRoot::open(f.source.join("origin")).unwrap(),
            PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap(),
            EntryLimits {
                entries: 64,
                bytes: 1000,
                file_bytes: 1000,
            },
            |_, _| Ok(()),
        )
        .unwrap()
    }
    #[test]
    fn restores_both_entry_kinds_over_absence_files_and_directories_without_consuming_origin() {
        for directory in [false, true] {
            for current in ["absent", "file", "directory"] {
                let f = Fixture::new();
                let prepared = prepare(&f, directory, current);
                let origin_file = f.source.join(if directory {
                    "origin/exchange/file"
                } else {
                    "origin/exchange"
                });
                let original_inode = fs::metadata(&origin_file).unwrap().ino();
                let mut editor = OpenOptions::new().append(true).open(&origin_file).unwrap();
                assert!(prepared.apply().unwrap());
                editor.write_all(b" late").unwrap();
                assert_eq!(fs::read(&origin_file).unwrap(), b"retained work late");
                let restored = f
                    .source
                    .join(if directory { "entry/file" } else { "entry" });
                assert_ne!(fs::metadata(&restored).unwrap().ino(), original_inode);
                assert_eq!(fs::read(&restored).unwrap(), b"retained work");
                if directory {
                    assert!(f.source.join("entry/empty").is_dir());
                    assert_eq!(fs::metadata(restored).unwrap().mode() & 0o777, 0o640);
                }
                if current != "absent" {
                    assert_eq!(
                        fs::read(f.recovery.join(if current == "file" {
                            "exchange"
                        } else {
                            "exchange/new"
                        }))
                        .unwrap(),
                        b"current work"
                    );
                } else {
                    assert!(!f.recovery.join("exchange").exists());
                }
            }
        }
    }
    #[test]
    fn changed_origin_destination_or_stage_refuses_without_consuming_any_entry() {
        for target in ["origin/exchange/file", "entry/new", "stage"] {
            let f = Fixture::new();
            let prepared = prepare(&f, true, "directory");
            let changed = if target == "stage" {
                f.recovery.join("exchange/file")
            } else {
                f.source.join(target)
            };
            fs::write(&changed, b"new work").unwrap();
            assert!(prepared.apply().is_err());
            assert_eq!(fs::read(changed).unwrap(), b"new work");
            assert!(f.source.join("entry/new").exists());
            assert!(f.source.join("origin/exchange/file").exists());
            assert!(f.recovery.join("exchange/file").exists());
        }
    }
    #[test]
    fn boundary_changes_and_failed_barriers_keep_all_copies_without_undo() {
        for change in ["origin", "source", "stage", "after", "sync", "collision"] {
            let f = Fixture::new();
            let prepared = prepare(
                &f,
                true,
                if change == "collision" {
                    "absent"
                } else {
                    "directory"
                },
            );
            let barriers = std::cell::Cell::new(0);
            let result = prepared.apply_with_hooks(
                || match change {
                    "origin" => {
                        fs::write(f.source.join("origin/exchange/file"), b"late work").unwrap()
                    }
                    "source" => fs::write(f.source.join("entry/new"), b"late work").unwrap(),
                    "stage" => fs::write(f.recovery.join("exchange/file"), b"late work").unwrap(),
                    "collision" => fs::write(f.source.join("entry"), b"late work").unwrap(),
                    _ => {}
                },
                || {
                    if change == "after" {
                        fs::rename(f.source.join("entry"), f.source.join("moved")).unwrap();
                        fs::write(f.source.join("entry"), b"late work").unwrap();
                    }
                },
                |fd| {
                    barriers.set(barriers.get() + 1);
                    if change == "sync" {
                        Err(io::Error::other("injected barrier failure"))
                    } else {
                        fd.sync_all()
                    }
                },
            );
            if change == "collision" {
                assert!(result.is_err());
                assert_eq!(fs::read(f.source.join("entry")).unwrap(), b"late work");
            } else {
                assert!(!result.unwrap());
                assert_eq!(barriers.get(), 2);
            }
            assert!(f.source.join("origin/exchange/file").exists());
            assert!(f.recovery.join("exchange").exists());
            if change == "source" {
                assert_eq!(
                    fs::read(f.recovery.join("exchange/new")).unwrap(),
                    b"late work"
                );
            }
        }
    }
    #[test]
    fn refusal_and_combined_content_budget_leave_no_private_staged_copy() {
        for reject_admission in [false, true] {
            let f = Fixture::new();
            fs::create_dir(f.source.join("origin")).unwrap();
            fs::write(f.source.join("origin/exchange"), b"12345").unwrap();
            fs::write(f.source.join("entry"), b"67890").unwrap();
            let admitted = std::cell::Cell::new(false);
            let result = RetainedEntryRestoration::prepare(
                PinnedWorkspaceRoot::open(f.source.clone()).unwrap(),
                PathBuf::from("entry"),
                PinnedWorkspaceRoot::open(f.source.join("origin")).unwrap(),
                PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap(),
                EntryLimits {
                    entries: 64,
                    bytes: if reject_admission { 10 } else { 9 },
                    file_bytes: 5,
                },
                |_, _| {
                    admitted.set(true);
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "excluded private work",
                    ))
                },
            );
            assert!(result.is_err());
            assert_eq!(admitted.get(), reject_admission);
            assert_eq!(
                fs::read(f.source.join("origin/exchange")).unwrap(),
                b"12345"
            );
            assert_eq!(fs::read(f.source.join("entry")).unwrap(), b"67890");
            assert!(!f.recovery.join("exchange").exists());
        }
    }
}
