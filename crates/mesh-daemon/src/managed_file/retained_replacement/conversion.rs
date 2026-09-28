//! Exchange a regular file and a complete directory tree, retaining both original objects.
use super::*;
use crate::ipc::Json;
use std::ffi::OsStr;

pub(crate) enum ConversionInput<'a> {
    File(&'a [u8], bool),
    Directory(&'a [TreeInput]),
}
#[derive(Clone, Copy)]
pub(crate) struct EntryLimits {
    pub(crate) entries: usize,
    pub(crate) bytes: u64,
    pub(crate) file_bytes: u64,
}
pub(crate) fn observe_entry(
    root: &PinnedWorkspaceRoot,
    path: &Path,
    limits: EntryLimits,
) -> io::Result<Json> {
    if limits.entries == 0 {
        return Err(io::Error::other("entry budget unavailable"));
    }
    let entry = root.filesystem().inspect_entry(path)?;
    if entry.metadata()?.is_dir() {
        return observe_tree(
            &open_tree(root, path)?,
            limits.entries.min(64),
            limits.bytes.min(64 * 1024 * 1024),
            limits.file_bytes,
        );
    }
    let observed = observe_file(
        root,
        path,
        limits.bytes.min(limits.file_bytes).min(64 * 1024 * 1024),
    )?;
    Ok(Json::Array(vec![Json::object([
        ("path", Json::text("")),
        ("kind", Json::text("file")),
        ("installation", Json::text(observed.installation)),
        ("mode", Json::Number(observed.mode as u64)),
        ("metadata", Json::text(observed.metadata)),
        ("digest", Json::text(observed.digest)),
        ("bytes", Json::Number(observed.bytes)),
    ])]))
}
fn kind(value: &Json) -> Option<&str> {
    value.as_array()?.first()?.get("kind")?.as_text()
}
pub(crate) struct RetainedConversion {
    source: PinnedWorkspaceRoot,
    relative: PathBuf,
    recovery: PinnedWorkspaceRoot,
    before: Json,
    after: Json,
    parent: String,
    policy: String,
    mode: u32,
    limits: EntryLimits,
}
impl RetainedConversion {
    pub(crate) fn prepare(
        source: PinnedWorkspaceRoot,
        relative: PathBuf,
        recovery: PinnedWorkspaceRoot,
        input: ConversionInput<'_>,
        limits: EntryLimits,
        receipt: impl FnOnce(&Json, &Json, &str, &str, u32) -> io::Result<()>,
    ) -> io::Result<Self> {
        let (policy, mode, parent) = parent_policy(&source, &relative)?;
        let (destination_parent, _) = source.filesystem().inspect_entry_with_parent(&relative)?;
        let stat = destination_parent.metadata()?;
        if (ManagedDirectoryIdentity {
            device: stat.dev(),
            inode: stat.ino(),
        })
        .token()
            != parent
            || recovery.identity()?.0 != stat.dev()
        {
            return Err(io::Error::other(
                "conversion parent or recovery filesystem changed",
            ));
        }
        let before = observe_entry(&source, &relative, limits)?;
        let after = match input {
            ConversionInput::Directory(inputs) => {
                if kind(&before) != Some("file") || inputs.len() + 1 > limits.entries.min(64) {
                    return Err(io::Error::other("expected file-to-directory conversion"));
                }
                let mut total = 0u64;
                for input in inputs {
                    if let TreeInput::File(_, bytes, _) = input {
                        total = total
                            .checked_add(bytes.len() as u64)
                            .ok_or_else(|| io::Error::other("conversion size overflow"))?;
                        if bytes.len() as u64 > limits.file_bytes
                            || total > limits.bytes.min(64 * 1024 * 1024)
                        {
                            return Err(io::Error::other("conversion tree exceeds content budget"));
                        }
                    }
                }
                let staged = tree_addition::stage_tree(&destination_parent, inputs, &recovery)?;
                if staged.parent_metadata != policy || staged.parent_mode != mode {
                    return Err(io::Error::other("conversion inheritance policy changed"));
                }
                staged.evidence
            }
            ConversionInput::File(bytes, executable) => {
                if kind(&before) != Some("directory")
                    || bytes.len() as u64
                        > limits.bytes.min(limits.file_bytes).min(64 * 1024 * 1024)
                {
                    return Err(io::Error::other(
                        "expected bounded directory-to-file conversion",
                    ));
                }
                recovery.ensure_namespace_identity()?;
                let parent_fd = recovery.try_clone_directory()?;
                let mut fd = create_new_at_mode(
                    &parent_fd,
                    EXCHANGE,
                    if executable { 0o755 } else { 0o644 },
                )?;
                let file_mode = fd.metadata()?.mode() & 0o777;
                fd.write_all(bytes)?;
                let inherited = metadata::inherit_new_entry(&destination_parent, &fd, file_mode)?;
                if inherited != (policy.clone(), mode)
                    || (fd.metadata()?.mode() & 0o111 != 0) != executable
                {
                    return Err(io::Error::other("conversion file inheritance changed"));
                }
                fd.sync_all()?;
                parent_fd.sync_all()?;
                Json::Array(vec![tree_addition::prepared_entry("", &fd, Some(bytes))?])
            }
        };
        let prepared = Self {
            source,
            relative,
            recovery,
            before,
            after,
            parent,
            policy,
            mode,
            limits,
        };
        prepared.validate()?;
        receipt(
            &prepared.before,
            &prepared.after,
            &prepared.parent,
            &prepared.policy,
            prepared.mode,
        )?;
        prepared.validate()?;
        Ok(prepared)
    }
    pub(crate) fn validate(&self) -> io::Result<()> {
        self.target().map(|_| ())
    }
    fn target(&self) -> io::Result<File> {
        self.source.ensure_namespace_identity()?;
        self.recovery.ensure_namespace_identity()?;
        let (parent, _) = self
            .source
            .filesystem()
            .inspect_entry_with_parent(&self.relative)?;
        let stat = parent.metadata()?;
        let current_policy = || {
            parent_policy(&self.source, &self.relative)
                .is_ok_and(|p| p == (self.policy.clone(), self.mode, self.parent.clone()))
        };
        if (ManagedDirectoryIdentity {
            device: stat.dev(),
            inode: stat.ino(),
        })
        .token()
            != self.parent
            || !current_policy()
            || observe_entry(&self.source, &self.relative, self.limits)? != self.before
            || observe_entry(&self.recovery, Path::new(EXCHANGE), self.limits)? != self.after
            || !current_policy()
        {
            return Err(io::Error::other(
                "conversion source, stage or parent changed",
            ));
        }
        self.source.ensure_namespace_identity()?;
        self.recovery.ensure_namespace_identity()?;
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
            .relative
            .file_name()
            .ok_or_else(|| io::Error::other("conversion has no leaf"))?;
        before();
        atomic_exchange_at(&parent, name, &recovery, OsStr::new(EXCHANGE))?;
        after();
        let durable = sync(&parent).is_ok() & sync(&recovery).is_ok();
        let installed = observe_entry(&self.source, &self.relative, self.limits);
        let retained = observe_entry(&self.recovery, Path::new(EXCHANGE), self.limits);
        let policy = parent_policy(&self.source, &self.relative)
            .is_ok_and(|p| p == (self.policy, self.mode, self.parent));
        Ok(durable
            && policy
            && installed.is_ok_and(|tree| tree == self.after)
            && retained.is_ok_and(|tree| tree == self.before)
            && self.recovery.ensure_namespace_identity().is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::Fixture;
    use super::*;
    fn prepare(f: &Fixture, directory_before: bool) -> RetainedConversion {
        if directory_before {
            fs::create_dir_all(f.source.join("entry/empty")).unwrap();
            fs::write(f.source.join("entry/file"), b"original").unwrap();
        } else {
            fs::write(f.source.join("entry"), b"original").unwrap();
        }
        let contents = [
            TreeInput::Directory("empty".into()),
            TreeInput::File("file".into(), b"replacement".to_vec(), false),
        ];
        RetainedConversion::prepare(
            PinnedWorkspaceRoot::open(f.source.clone()).unwrap(),
            PathBuf::from("entry"),
            PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap(),
            if directory_before {
                ConversionInput::File(b"replacement", false)
            } else {
                ConversionInput::Directory(&contents)
            },
            EntryLimits {
                entries: 64,
                bytes: 100,
                file_bytes: 100,
            },
            |before, after, _, _, _| {
                assert_ne!(kind(before), kind(after));
                Ok(())
            },
        )
        .unwrap()
    }
    #[test]
    fn both_type_exchanges_preserve_original_inodes_and_late_handles() {
        for directory_before in [false, true] {
            let f = Fixture::new();
            let prepared = prepare(&f, directory_before);
            let original = if directory_before {
                f.source.join("entry/file")
            } else {
                f.source.join("entry")
            };
            let mut editor = OpenOptions::new().append(true).open(&original).unwrap();
            let directory = directory_before.then(|| File::open(f.source.join("entry")).unwrap());
            assert!(prepared.apply().unwrap());
            editor.write_all(b" late").unwrap();
            if let Some(directory) = directory {
                create_new_at_mode(&directory, "user", 0o600)
                    .unwrap()
                    .write_all(b"new work")
                    .unwrap();
                assert_eq!(
                    fs::read(f.recovery.join("exchange/user")).unwrap(),
                    b"new work"
                );
                assert!(f.recovery.join("exchange/empty").is_dir());
            }
            let old = if directory_before {
                f.recovery.join("exchange/file")
            } else {
                f.recovery.join("exchange")
            };
            let new = if directory_before {
                f.source.join("entry")
            } else {
                f.source.join("entry/file")
            };
            assert_eq!(fs::read(old).unwrap(), b"original late");
            assert_eq!(fs::read(new).unwrap(), b"replacement");
            if !directory_before {
                assert!(f.source.join("entry/empty").is_dir());
            }
        }
    }
    #[test]
    fn boundary_edits_and_uncertain_durability_never_undo_a_conversion() {
        for directory_before in [false, true] {
            for change in ["source", "stage", "replace-after", "sync"] {
                let f = Fixture::new();
                let prepared = prepare(&f, directory_before);
                let calls = std::cell::Cell::new(0);
                assert!(!prepared
                    .apply_with_hooks(
                        || {
                            if change == "source" {
                                fs::write(
                                    if directory_before {
                                        f.source.join("entry/file")
                                    } else {
                                        f.source.join("entry")
                                    },
                                    b"late source",
                                )
                                .unwrap();
                            }
                            if change == "stage" {
                                fs::write(
                                    if directory_before {
                                        f.recovery.join("exchange")
                                    } else {
                                        f.recovery.join("exchange/file")
                                    },
                                    b"late stage",
                                )
                                .unwrap();
                            }
                        },
                        || {
                            if change == "replace-after" {
                                fs::rename(f.source.join("entry"), f.source.join("moved")).unwrap();
                                fs::write(f.source.join("entry"), b"new user work").unwrap();
                            }
                        },
                        |file| {
                            calls.set(calls.get() + 1);
                            if change == "sync" {
                                Err(io::Error::other("injected durability failure"))
                            } else {
                                file.sync_all()
                            }
                        }
                    )
                    .unwrap());
                assert_eq!(calls.get(), 2);
                let old = if directory_before {
                    f.recovery.join("exchange/file")
                } else {
                    f.recovery.join("exchange")
                };
                assert_eq!(
                    fs::read(old).unwrap(),
                    if change == "source" {
                        b"late source".as_slice()
                    } else {
                        b"original".as_slice()
                    }
                );
                if change == "replace-after" {
                    assert_eq!(fs::read(f.source.join("entry")).unwrap(), b"new user work");
                    assert!(f.source.join("moved").exists());
                }
            }
        }
    }
    #[test]
    fn changed_stage_or_source_refuses_before_exchange() {
        for directory_before in [false, true] {
            let f = Fixture::new();
            let prepared = prepare(&f, directory_before);
            if directory_before {
                fs::write(f.source.join("entry/user"), b"keep").unwrap();
            } else {
                fs::write(f.recovery.join("exchange/user"), b"keep").unwrap();
            }
            assert!(prepared.apply().is_err());
            assert_eq!(
                fs::read(if directory_before {
                    f.source.join("entry/file")
                } else {
                    f.source.join("entry")
                })
                .unwrap(),
                b"original"
            );
            assert!(f.recovery.join("exchange").exists());
        }
    }
}
