//! Stage one complete absent subtree, then install its root without replacing concurrent entries.
use super::*;
use std::collections::BTreeMap;
use std::ffi::OsStr;

pub(crate) enum TreeInput {
    Directory(String),
    File(String, Vec<u8>, bool),
}
impl TreeInput {
    pub(crate) fn path(&self) -> &str {
        match self {
            Self::Directory(path) | Self::File(path, ..) => path,
        }
    }
}

pub(crate) fn open_tree(
    root: &PinnedWorkspaceRoot,
    path: &Path,
) -> io::Result<PinnedWorkspaceRoot> {
    let mut current = root.clone();
    for component in path.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(io::Error::other("invalid directory tree path"));
        };
        current = current.open_child_directory(name)?;
    }
    Ok(current)
}

pub(crate) fn observe_tree(
    root: &PinnedWorkspaceRoot,
    entries: usize,
    bytes: u64,
    file_bytes: u64,
) -> io::Result<crate::ipc::Json> {
    use crate::ipc::Json;
    let mut pending = vec![(String::new(), root.clone())];
    let mut result = BTreeMap::new();
    let mut remaining = bytes;
    while let Some((path, directory)) = pending.pop() {
        if result.len() >= entries {
            return Err(io::Error::other("tree entry budget exceeded"));
        }
        directory.ensure_namespace_identity()?;
        let descriptor = directory.try_clone_directory()?;
        let before = descriptor.metadata()?;
        let identity = managed_file_identity(&descriptor, &before)?.token();
        let metadata = metadata_digest(&descriptor)?;
        result.insert(
            path.clone(),
            Json::object([
                ("path", Json::text(&path)),
                ("kind", Json::text("directory")),
                ("installation", Json::text(identity)),
                ("mode", Json::Number(before.mode() as u64)),
                ("metadata", Json::text(metadata)),
                ("digest", Json::Null),
                ("bytes", Json::Null),
            ]),
        );
        for name in directory
            .filesystem()
            .read_directory_names_bounded(Path::new(""), entries.saturating_sub(result.len()))?
        {
            let name_text = name
                .to_str()
                .ok_or_else(|| io::Error::other("tree name is not UTF-8"))?;
            let child_path = if path.is_empty() {
                name_text.to_owned()
            } else {
                format!("{path}/{name_text}")
            };
            let file = directory.filesystem().inspect_entry(Path::new(&name))?;
            let meta = file.metadata()?;
            if meta.is_dir() {
                pending.push((child_path, directory.open_child_directory(&name)?));
            } else if meta.is_file() {
                if result.len() >= entries {
                    return Err(io::Error::other("tree entry budget exceeded"));
                }
                let observed =
                    observe_file(&directory, Path::new(&name), remaining.min(file_bytes))?;
                remaining = remaining
                    .checked_sub(observed.bytes)
                    .ok_or_else(|| io::Error::other("tree byte budget exceeded"))?;
                result.insert(
                    child_path.clone(),
                    Json::object([
                        ("path", Json::text(child_path)),
                        ("kind", Json::text("file")),
                        ("installation", Json::text(observed.installation)),
                        ("mode", Json::Number(observed.mode as u64)),
                        ("metadata", Json::text(observed.metadata)),
                        ("digest", Json::text(observed.digest)),
                        ("bytes", Json::Number(observed.bytes)),
                    ]),
                );
            } else {
                return Err(io::Error::other("unsupported tree entry"));
            }
        }
        let after = descriptor.metadata()?;
        if (
            before.dev(),
            before.ino(),
            before.ctime(),
            before.ctime_nsec(),
        ) != (after.dev(), after.ino(), after.ctime(), after.ctime_nsec())
        {
            return Err(io::Error::other(
                "directory changed during tree observation",
            ));
        }
        directory.ensure_namespace_identity()?;
    }
    root.ensure_namespace_identity()?;
    Ok(Json::Array(result.into_values().collect()))
}

pub(crate) struct RetainedTreeAddition {
    source: PinnedWorkspaceRoot,
    relative: PathBuf,
    parent: String,
    parent_metadata: String,
    parent_mode: u32,
    recovery: PinnedWorkspaceRoot,
    evidence: crate::ipc::Json,
    entries: usize,
    bytes: u64,
    file_bytes: u64,
}
impl RetainedTreeAddition {
    pub(crate) fn prepare(
        source: PinnedWorkspaceRoot,
        relative: PathBuf,
        inputs: &[TreeInput],
        recovery: PinnedWorkspaceRoot,
        receipt: impl FnOnce(&crate::ipc::Json, &str, &str, u32) -> io::Result<()>,
    ) -> io::Result<Self> {
        Self::prepare_bounded(source, relative, inputs, recovery, legacy_limits(), receipt)
    }

    /// Native callers must independently bind the admitted limits and exact installation intent.
    pub(crate) fn prepare_bounded(
        source: PinnedWorkspaceRoot,
        relative: PathBuf,
        inputs: &[TreeInput],
        recovery: PinnedWorkspaceRoot,
        limits: EntryLimits,
        receipt: impl FnOnce(&crate::ipc::Json, &str, &str, u32) -> io::Result<()>,
    ) -> io::Result<Self> {
        validate_inputs(inputs, limits)?;
        let parent = absent_parent(&source, &relative)?
            .ok_or_else(|| io::Error::other("directory destination is occupied"))?;
        let (destination_parent, _) = source
            .filesystem()
            .inspect_optional_entry_with_parent(&relative)?;
        let StagedTree {
            evidence,
            bytes,
            parent_metadata,
            parent_mode,
        } = stage_tree_bounded(&destination_parent, inputs, &recovery, limits)?;
        let prepared = Self {
            source,
            relative,
            parent,
            parent_metadata,
            parent_mode,
            recovery,
            evidence,
            entries: inputs.len() + 1,
            bytes,
            file_bytes: inputs
                .iter()
                .filter_map(|input| match input {
                    TreeInput::File(_, bytes, _) => Some(bytes.len() as u64),
                    _ => None,
                })
                .max()
                .unwrap_or(0),
        };
        prepared.validate()?;
        receipt(
            &prepared.evidence,
            &prepared.parent,
            &prepared.parent_metadata,
            prepared.parent_mode,
        )?;
        prepared.validate()?;
        Ok(prepared)
    }
    /// Immutable evidence only. The enclosing native transaction must bind and durably retain
    /// these bytes before installation; possession of a receipt conveys no write authority.
    pub(crate) fn recovery_receipt(&self) -> io::Result<String> {
        use crate::ipc::Json;
        self.source.ensure_namespace_identity()?;
        self.recovery.ensure_namespace_identity()?;
        let identity = |root: &PinnedWorkspaceRoot| -> io::Result<String> {
            let (device, inode) = root.identity()?;
            Ok(ManagedDirectoryIdentity { device, inode }.token())
        };
        let relative = self
            .relative
            .to_str()
            .ok_or_else(|| io::Error::other("tree path is not UTF-8"))?;
        Ok(Json::object([
            ("schema", Json::text("mesh.retained-tree-addition/v1")),
            ("source", Json::text(identity(&self.source)?)),
            ("recovery", Json::text(identity(&self.recovery)?)),
            ("path", Json::text(relative)),
            ("parent", Json::text(&self.parent)),
            ("parent_metadata", Json::text(&self.parent_metadata)),
            ("parent_mode", Json::Number(u64::from(self.parent_mode))),
            ("entries", Json::Number(self.entries as u64)),
            ("bytes", Json::Number(self.bytes)),
            ("file_bytes", Json::Number(self.file_bytes)),
            ("evidence", self.evidence.clone()),
        ])
        .encode())
    }

    /// Reconstruct only an exact retained or already-installed tree. The caller must independently
    /// authenticate the receipt against its native transaction and hold the necessary custody.
    pub(crate) fn resume(
        source: PinnedWorkspaceRoot,
        relative: PathBuf,
        recovery: PinnedWorkspaceRoot,
        receipt: &str,
        limits: EntryLimits,
    ) -> io::Result<Self> {
        use crate::ipc::Json;
        validate_limits(limits)?;
        if receipt.len() > 64 * 1024 * 1024 {
            return Err(io::Error::other("tree receipt exceeds limit"));
        }
        let json = Json::parse(receipt).map_err(io::Error::other)?;
        let text = |key| -> io::Result<String> {
            json.get(key)
                .and_then(Json::as_text)
                .map(str::to_owned)
                .ok_or_else(|| io::Error::other("invalid tree receipt field"))
        };
        let number = |key| -> io::Result<u64> {
            json.get(key)
                .and_then(Json::as_u64)
                .ok_or_else(|| io::Error::other("invalid tree receipt number"))
        };
        let entries = usize::try_from(number("entries")?).map_err(io::Error::other)?;
        let bytes = number("bytes")?;
        let file_bytes = number("file_bytes")?;
        let evidence = json
            .get("evidence")
            .cloned()
            .ok_or_else(|| io::Error::other("missing tree evidence"))?;
        if entries == 0
            || entries > limits.entries
            || bytes > limits.bytes
            || file_bytes > limits.file_bytes
            || evidence.as_array().map(<[_]>::len) != Some(entries)
        {
            return Err(io::Error::other("tree receipt exceeds admitted bounds"));
        }
        let prepared = Self {
            source,
            relative,
            recovery,
            evidence,
            entries,
            bytes,
            file_bytes,
            parent: text("parent")?,
            parent_metadata: text("parent_metadata")?,
            parent_mode: u32::try_from(number("parent_mode")?).map_err(io::Error::other)?,
        };
        if prepared.recovery_receipt()? != receipt {
            return Err(io::Error::other(
                "tree receipt is noncanonical or belongs to other roots/path",
            ));
        }
        if !prepared.installed()? {
            prepared.validate()?;
        }
        Ok(prepared)
    }

    fn installed(&self) -> io::Result<bool> {
        self.source.ensure_namespace_identity()?;
        self.recovery.ensure_namespace_identity()?;
        let (_, entry) = self
            .source
            .filesystem()
            .inspect_optional_entry_with_parent(&self.relative)?;
        let Some(entry) = entry else { return Ok(false) };
        if !entry.metadata()?.is_dir()
            || absent_parent(&self.recovery, Path::new(EXCHANGE))?.is_none()
        {
            return Err(io::Error::other(
                "tree installation conflicts with retained stage",
            ));
        }
        let check_parent = || -> io::Result<()> {
            let (policy, mode, identity) = parent_policy(&self.source, &self.relative)?;
            if policy != self.parent_metadata || mode != self.parent_mode || identity != self.parent
            {
                return Err(io::Error::other("tree installation parent changed"));
            }
            Ok(())
        };
        check_parent()?;
        let tree = open_tree(&self.source, &self.relative)?;
        if observe_tree(&tree, self.entries, self.bytes, self.file_bytes)? != self.evidence {
            return Err(io::Error::other(
                "installed tree differs from retained identity or content",
            ));
        }
        check_parent()?;
        self.source.ensure_namespace_identity()?;
        self.recovery.ensure_namespace_identity()?;
        Ok(true)
    }

    pub(crate) fn validate(&self) -> io::Result<()> {
        self.target().map(|_| ())
    }
    fn target(&self) -> io::Result<File> {
        self.source.ensure_namespace_identity()?;
        self.recovery.ensure_namespace_identity()?;
        let (parent, entry) = self
            .source
            .filesystem()
            .inspect_optional_entry_with_parent(&self.relative)?;
        let meta = parent.metadata()?;
        if entry.is_some()
            || (ManagedDirectoryIdentity {
                device: meta.dev(),
                inode: meta.ino(),
            })
            .token()
                != self.parent
            || meta.mode() != self.parent_mode
            || metadata_digest(&parent)? != self.parent_metadata
        {
            return Err(io::Error::other(
                "directory destination or parent policy changed",
            ));
        }
        let stage = self.recovery.open_child_directory(OsStr::new(EXCHANGE))?;
        if observe_tree(&stage, self.entries, self.bytes, self.file_bytes)? != self.evidence {
            return Err(io::Error::other("directory stage changed"));
        }
        if !parent_policy(&self.source, &self.relative).is_ok_and(|(policy, mode, identity)| {
            policy == self.parent_metadata && mode == self.parent_mode && identity == self.parent
        }) {
            return Err(io::Error::other(
                "directory parent changed during validation",
            ));
        }
        self.source.ensure_namespace_identity()?;
        self.recovery.ensure_namespace_identity()?;
        Ok(parent)
    }
    pub(crate) fn apply(self) -> io::Result<bool> {
        let receipt = self.recovery_receipt()?;
        Self::resume(
            self.source.clone(),
            self.relative.clone(),
            self.recovery.clone(),
            &receipt,
            EntryLimits {
                entries: self.entries,
                bytes: self.bytes.max(1),
                file_bytes: self.file_bytes.max(1),
            },
        )?
        .apply_with_hooks(|| {}, || {}, File::sync_all)
    }
    fn apply_with_hooks(
        self,
        before: impl FnOnce(),
        after: impl FnOnce(),
        sync: impl Fn(&File) -> io::Result<()>,
    ) -> io::Result<bool> {
        if self.installed()? {
            let (parent, _) = self
                .source
                .filesystem()
                .inspect_entry_with_parent(&self.relative)?;
            let recovery = self.recovery.try_clone_directory()?;
            let durable = sync(&parent).is_ok() & sync(&recovery).is_ok();
            return Ok(durable && self.installed()?);
        }
        let parent = self.target()?;
        let recovery = self.recovery.try_clone_directory()?;
        let name = self
            .relative
            .file_name()
            .ok_or_else(|| io::Error::other("directory has no leaf"))?;
        before();
        atomic_rename_noreplace_at(&recovery, OsStr::new(EXCHANGE), &parent, name)?;
        after();
        let durable = sync(&parent).is_ok() & sync(&recovery).is_ok();
        let observed = open_tree(&self.source, &self.relative)
            .and_then(|tree| observe_tree(&tree, self.entries, self.bytes, self.file_bytes));
        let parent_current =
            parent_policy(&self.source, &self.relative).is_ok_and(|(policy, mode, identity)| {
                policy == self.parent_metadata
                    && mode == self.parent_mode
                    && identity == self.parent
            });
        Ok(durable
            && parent_current
            && observed.is_ok_and(|value| value == self.evidence)
            && absent_parent(&self.recovery, Path::new(EXCHANGE))
                .is_ok_and(|entry| entry.is_some()))
    }
}

pub(super) struct StagedTree {
    pub(super) evidence: crate::ipc::Json,
    pub(super) bytes: u64,
    pub(super) parent_metadata: String,
    pub(super) parent_mode: u32,
}
/// Shared private staging for absent-path creation and an occupied-path type exchange.
pub(super) fn stage_tree(
    destination_parent: &File,
    inputs: &[TreeInput],
    recovery: &PinnedWorkspaceRoot,
) -> io::Result<StagedTree> {
    stage_tree_bounded(destination_parent, inputs, recovery, legacy_limits())
}

fn legacy_limits() -> EntryLimits {
    EntryLimits {
        entries: 64,
        bytes: 64 * 1024 * 1024,
        file_bytes: 64 * 1024 * 1024,
    }
}
fn validate_limits(limits: EntryLimits) -> io::Result<()> {
    if limits.entries == 0
        || limits.entries > 100_001
        || limits.bytes == 0
        || limits.bytes > 1024 * 1024 * 1024
        || limits.file_bytes == 0
        || limits.file_bytes > 64 * 1024 * 1024
    {
        return Err(io::Error::other("invalid tree staging limits"));
    }
    Ok(())
}
fn validate_inputs(inputs: &[TreeInput], limits: EntryLimits) -> io::Result<()> {
    validate_limits(limits)?;
    if inputs.len() >= limits.entries {
        return Err(io::Error::other("directory tree exceeds entry limit"));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut directories = std::collections::BTreeSet::from([""]);
    let mut remaining = limits.bytes;
    for input in inputs {
        let path = input.path();
        let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
        if path.is_empty()
            || path.contains('\0')
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || !seen.insert(path)
            || !directories.contains(parent)
        {
            return Err(io::Error::other(
                "invalid, duplicate or unordered tree member",
            ));
        }
        match input {
            TreeInput::Directory(_) => {
                directories.insert(path);
            }
            TreeInput::File(_, bytes, _) => {
                if bytes.len() as u64 > limits.file_bytes {
                    return Err(io::Error::other("tree file exceeds limit"));
                }
                remaining = remaining
                    .checked_sub(bytes.len() as u64)
                    .ok_or_else(|| io::Error::other("tree bytes exceed limit"))?;
            }
        }
    }
    Ok(())
}
fn stage_tree_bounded(
    destination_parent: &File,
    inputs: &[TreeInput],
    recovery: &PinnedWorkspaceRoot,
    limits: EntryLimits,
) -> io::Result<StagedTree> {
    validate_inputs(inputs, limits)?;
    recovery.ensure_namespace_identity()?;
    if destination_parent.metadata()?.dev() != recovery.identity()?.0 {
        return Err(io::Error::other(
            "directory recovery must share the destination filesystem",
        ));
    }
    let stage = recovery.create_child_directory_with_mode(OsStr::new(EXCHANGE), 0o777)?;
    let descriptor = stage.try_clone_directory()?;
    let mode = descriptor.metadata()?.mode() & 0o777;
    let (parent_metadata, parent_mode) =
        metadata::inherit_new_entry(destination_parent, &descriptor, mode)?;
    let mut directories = BTreeMap::from([(String::new(), stage.clone())]);
    let mut expected = BTreeMap::from([(String::new(), prepared_entry("", &descriptor, None)?)]);
    let mut seen = std::collections::BTreeSet::new();
    let mut bytes = 0u64;
    for input in inputs {
        let path = input.path();
        if path.is_empty()
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || !seen.insert(path)
        {
            return Err(io::Error::other(
                "invalid or duplicate directory tree member",
            ));
        }
        let (parent_path, leaf) = path.rsplit_once('/').unwrap_or(("", path));
        let parent = directories
            .get(parent_path)
            .ok_or_else(|| io::Error::other("tree parent is unavailable or unordered"))?
            .clone();
        let parent_fd = parent.try_clone_directory()?;
        match input {
            TreeInput::Directory(_) => {
                let child = parent.create_child_directory_with_mode(OsStr::new(leaf), 0o777)?;
                let fd = child.try_clone_directory()?;
                let mode = fd.metadata()?.mode() & 0o777;
                metadata::inherit_new_entry(&parent_fd, &fd, mode)?;
                fd.sync_all()?;
                expected.insert(path.to_owned(), prepared_entry(path, &fd, None)?);
                directories.insert(path.to_owned(), child);
            }
            TreeInput::File(_, content, executable) => {
                bytes = bytes
                    .checked_add(content.len() as u64)
                    .filter(|bytes| *bytes <= limits.bytes)
                    .ok_or_else(|| io::Error::other("tree bytes exceed limit"))?;
                let mut fd =
                    create_new_at_mode(&parent_fd, leaf, if *executable { 0o755 } else { 0o644 })?;
                let mode = fd.metadata()?.mode() & 0o777;
                fd.write_all(content)?;
                metadata::inherit_new_entry(&parent_fd, &fd, mode)?;
                if (fd.metadata()?.mode() & 0o111 != 0) != *executable {
                    return Err(io::Error::other("tree executable state was not preserved"));
                }
                fd.sync_all()?;
                expected.insert(path.to_owned(), prepared_entry(path, &fd, Some(content))?);
            }
        }
        parent_fd.sync_all()?;
    }
    descriptor.sync_all()?;
    recovery.try_clone_directory()?.sync_all()?;
    let evidence = crate::ipc::Json::Array(expected.into_values().collect());
    if observe_tree(&stage, inputs.len() + 1, bytes, limits.file_bytes)? != evidence {
        return Err(io::Error::other(
            "directory tree changed during preparation",
        ));
    }
    Ok(StagedTree {
        evidence,
        bytes,
        parent_metadata,
        parent_mode,
    })
}

pub(super) fn prepared_entry(
    path: &str,
    file: &File,
    bytes: Option<&[u8]>,
) -> io::Result<crate::ipc::Json> {
    use crate::ipc::Json;
    use mesh_types::ContentDigest as _;
    let stat = file.metadata()?;
    Ok(Json::object([
        ("path", Json::text(path)),
        (
            "kind",
            Json::text(if bytes.is_some() { "file" } else { "directory" }),
        ),
        (
            "installation",
            Json::text(managed_file_identity(file, &stat)?.token()),
        ),
        ("mode", Json::Number(stat.mode() as u64)),
        ("metadata", Json::text(metadata_digest(file)?)),
        (
            "digest",
            bytes.map_or(Json::Null, |bytes| {
                Json::text(mesh_types::Blake3::digest_bytes(bytes).to_string())
            }),
        ),
        (
            "bytes",
            bytes.map_or(Json::Null, |bytes| Json::Number(bytes.len() as u64)),
        ),
    ]))
}

#[cfg(test)]
mod tests {
    use super::super::tests::Fixture;
    use super::*;

    #[test]
    fn bounded_large_tree_resumes_from_saved_receipt_before_and_after_installation() {
        let f = Fixture::new();
        let inputs = (0..100)
            .map(|n| TreeInput::File(format!("file-{n:03}"), vec![n as u8], n == 0))
            .collect::<Vec<_>>();
        let source = || PinnedWorkspaceRoot::open(f.source.clone()).unwrap();
        let recovery = || PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap();
        assert!(RetainedTreeAddition::prepare(
            source(),
            "new".into(),
            &inputs,
            recovery(),
            |_, _, _, _| Ok(())
        )
        .is_err());
        assert!(!f.recovery.join(EXCHANGE).exists());
        let limits = EntryLimits {
            entries: 101,
            bytes: 100,
            file_bytes: 1,
        };
        let prepared = RetainedTreeAddition::prepare_bounded(
            source(),
            "new".into(),
            &inputs,
            recovery(),
            limits,
            |_, _, _, _| Ok(()),
        )
        .unwrap();
        let receipt_path = f.recovery.join("saved-receipt.json");
        fs::write(&receipt_path, prepared.recovery_receipt().unwrap()).unwrap();
        File::open(&receipt_path).unwrap().sync_all().unwrap();
        let staged = fs::metadata(f.recovery.join(EXCHANGE)).unwrap().ino();
        drop(prepared);
        let resume = || {
            RetainedTreeAddition::resume(
                source(),
                "new".into(),
                recovery(),
                &fs::read_to_string(&receipt_path).unwrap(),
                limits,
            )
        };
        let syncs = std::cell::Cell::new(0);
        assert!(!resume()
            .unwrap()
            .apply_with_hooks(
                || {},
                || {},
                |_| {
                    syncs.set(syncs.get() + 1);
                    Err(io::Error::other("lost durability acknowledgement"))
                }
            )
            .unwrap());
        assert_eq!(syncs.get(), 2);
        assert_eq!(fs::metadata(f.source.join("new")).unwrap().ino(), staged);
        assert!(!f.recovery.join(EXCHANGE).exists());
        for _ in 0..2 {
            assert!(resume().unwrap().apply().unwrap());
            assert_eq!(fs::metadata(f.source.join("new")).unwrap().ino(), staged);
        }
        for n in 0..100 {
            assert_eq!(
                fs::read(f.source.join(format!("new/file-{n:03}"))).unwrap(),
                [n as u8]
            );
        }
        assert_ne!(
            fs::metadata(f.source.join("new/file-000")).unwrap().mode() & 0o111,
            0
        );
        assert_eq!(fs::read(f.source.join("file")).unwrap(), b"before");
    }

    #[test]
    fn staging_budgets_and_bad_paths_refuse_before_creating_any_private_tree() {
        let cases = vec![
            (
                vec![TreeInput::File("x".into(), b"xx".to_vec(), false)],
                EntryLimits {
                    entries: 2,
                    bytes: 2,
                    file_bytes: 1,
                },
            ),
            (
                vec![TreeInput::File("x".into(), b"xx".to_vec(), false)],
                EntryLimits {
                    entries: 2,
                    bytes: 1,
                    file_bytes: 2,
                },
            ),
            (
                vec![TreeInput::File("x".into(), vec![], false)],
                EntryLimits {
                    entries: 1,
                    bytes: 1,
                    file_bytes: 1,
                },
            ),
            (
                vec![TreeInput::File("missing/x".into(), vec![], false)],
                legacy_limits(),
            ),
            (
                vec![TreeInput::Directory("../escape".into())],
                legacy_limits(),
            ),
            (
                vec![TreeInput::Directory("nul\0name".into())],
                legacy_limits(),
            ),
            (
                vec![
                    TreeInput::Directory("same".into()),
                    TreeInput::File("same".into(), vec![], false),
                ],
                legacy_limits(),
            ),
            (
                vec![],
                EntryLimits {
                    entries: 100_002,
                    bytes: 1,
                    file_bytes: 1,
                },
            ),
            (
                vec![],
                EntryLimits {
                    entries: 1,
                    bytes: 1024 * 1024 * 1024 + 1,
                    file_bytes: 1,
                },
            ),
        ];
        for (inputs, limits) in cases {
            let f = Fixture::new();
            let called = std::cell::Cell::new(false);
            assert!(RetainedTreeAddition::prepare_bounded(
                PinnedWorkspaceRoot::open(f.source.clone()).unwrap(),
                "new".into(),
                &inputs,
                PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap(),
                limits,
                |_, _, _, _| {
                    called.set(true);
                    Ok(())
                },
            )
            .is_err());
            assert!(!called.get());
            assert!(!f.recovery.join(EXCHANGE).exists());
            assert!(!f.source.join("new").exists());
            assert_eq!(fs::read(f.source.join("file")).unwrap(), b"before");
        }
    }

    #[test]
    fn resumed_tree_refuses_changed_receipt_stage_destination_and_native_roots() {
        for change in [
            "receipt",
            "path",
            "limit",
            "stage-edit",
            "installed-edit",
            "installed-extra",
            "both",
            "neither",
            "source-root",
            "recovery-root",
            "substituted-tree",
        ] {
            let f = Fixture::new();
            let prepared = prepare(&f);
            let mut receipt = prepared.recovery_receipt().unwrap();
            drop(prepared);
            let mut path = PathBuf::from("new");
            let mut limits = legacy_limits();
            match change {
                "receipt" => receipt.push(' '),
                "path" => path = "other".into(),
                "limit" => limits.entries = 1,
                "stage-edit" => {
                    fs::write(f.recovery.join("exchange/nested/run"), b"editor").unwrap()
                }
                "source-root" => {
                    fs::rename(&f.source, f.source.with_extension("moved")).unwrap();
                    fs::create_dir(&f.source).unwrap();
                }
                "recovery-root" => {
                    fs::rename(&f.recovery, f.recovery.with_extension("moved")).unwrap();
                    fs::create_dir(&f.recovery).unwrap();
                }
                "neither" => {
                    fs::rename(f.recovery.join(EXCHANGE), f.recovery.join("retained")).unwrap()
                }
                _ => {
                    fs::rename(f.recovery.join(EXCHANGE), f.source.join("new")).unwrap();
                    match change {
                        "installed-edit" => {
                            fs::write(f.source.join("new/nested/run"), b"editor").unwrap()
                        }
                        "installed-extra" => fs::write(f.source.join("new/user"), b"keep").unwrap(),
                        "both" => {
                            fs::create_dir(f.recovery.join(EXCHANGE)).unwrap();
                            fs::write(f.recovery.join("exchange/user"), b"keep").unwrap();
                        }
                        "substituted-tree" => {
                            fs::rename(f.source.join("new"), f.source.join("retained")).unwrap();
                            fs::create_dir_all(f.source.join("new/nested/empty")).unwrap();
                            fs::write(f.source.join("new/nested/run"), b"approved").unwrap();
                        }
                        _ => unreachable!(),
                    }
                }
            }
            assert!(
                RetainedTreeAddition::resume(
                    PinnedWorkspaceRoot::open(f.source.clone()).unwrap(),
                    path,
                    PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap(),
                    &receipt,
                    limits,
                )
                .is_err(),
                "{change}"
            );
            match change {
                "stage-edit" => assert_eq!(
                    fs::read(f.recovery.join("exchange/nested/run")).unwrap(),
                    b"editor"
                ),
                "installed-edit" => assert_eq!(
                    fs::read(f.source.join("new/nested/run")).unwrap(),
                    b"editor"
                ),
                "installed-extra" => {
                    assert_eq!(fs::read(f.source.join("new/user")).unwrap(), b"keep")
                }
                "both" => assert_eq!(fs::read(f.recovery.join("exchange/user")).unwrap(), b"keep"),
                "neither" => assert_eq!(
                    fs::read(f.recovery.join("retained/nested/run")).unwrap(),
                    b"approved"
                ),
                "source-root" => assert_eq!(
                    fs::read(f.source.with_extension("moved").join("file")).unwrap(),
                    b"before"
                ),
                "recovery-root" => assert_eq!(
                    fs::read(
                        f.recovery
                            .with_extension("moved")
                            .join("exchange/nested/run")
                    )
                    .unwrap(),
                    b"approved"
                ),
                "substituted-tree" => assert_eq!(
                    fs::read(f.source.join("retained/nested/run")).unwrap(),
                    b"approved"
                ),
                _ => assert_eq!(
                    fs::read(f.recovery.join("exchange/nested/run")).unwrap(),
                    b"approved"
                ),
            }
        }
    }

    #[test]
    fn tree_installation_recovers_after_installer_process_exits_without_acknowledgement() {
        const MODE: &str = "MESH_TREE_RECOVERY_PROCESS_MODE";
        const SOURCE: &str = "MESH_TREE_RECOVERY_PROCESS_SOURCE";
        const RECOVERY: &str = "MESH_TREE_RECOVERY_PROCESS_STORE";
        if let Some(mode) = std::env::var_os(MODE) {
            let source = PathBuf::from(std::env::var_os(SOURCE).unwrap());
            let recovery = PathBuf::from(std::env::var_os(RECOVERY).unwrap());
            let receipt = fs::read_to_string(recovery.join("receipt.json")).unwrap();
            let resumed = RetainedTreeAddition::resume(
                PinnedWorkspaceRoot::open(source).unwrap(),
                "new".into(),
                PinnedWorkspaceRoot::open(recovery).unwrap(),
                &receipt,
                legacy_limits(),
            )
            .unwrap();
            if mode == "interrupt" {
                let _ = resumed.apply_with_hooks(|| {}, || std::process::exit(73), File::sync_all);
                panic!("installer did not reach the post-rename boundary");
            }
            assert_eq!(mode, "recover");
            assert!(resumed.apply().unwrap());
            return;
        }
        let f = Fixture::new();
        let prepared = prepare(&f);
        fs::write(
            f.recovery.join("receipt.json"),
            prepared.recovery_receipt().unwrap(),
        )
        .unwrap();
        File::open(f.recovery.join("receipt.json"))
            .unwrap()
            .sync_all()
            .unwrap();
        File::open(&f.recovery).unwrap().sync_all().unwrap();
        let original_inode = fs::metadata(f.recovery.join("exchange")).unwrap().ino();
        drop(prepared);
        let invoke = |mode: &str| {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "managed_file::retained_replacement::tree_addition::tests::tree_installation_recovers_after_installer_process_exits_without_acknowledgement", "--test-threads=1"])
                .env(MODE, mode).env(SOURCE, &f.source).env(RECOVERY, &f.recovery)
                .output().unwrap()
        };
        assert_eq!(invoke("interrupt").status.code(), Some(73));
        assert!(!f.recovery.join("exchange").exists());
        for _ in 0..2 {
            let outcome = invoke("recover");
            assert!(
                outcome.status.success(),
                "{}",
                String::from_utf8_lossy(&outcome.stdout)
            );
            assert_eq!(
                fs::metadata(f.source.join("new")).unwrap().ino(),
                original_inode
            );
            assert_eq!(
                fs::read(f.source.join("new/nested/run")).unwrap(),
                b"approved"
            );
        }
    }

    fn prepare(f: &Fixture) -> RetainedTreeAddition {
        RetainedTreeAddition::prepare(
            PinnedWorkspaceRoot::open(f.source.clone()).unwrap(),
            PathBuf::from("new"),
            &[
                TreeInput::Directory("nested".into()),
                TreeInput::Directory("nested/empty".into()),
                TreeInput::File("nested/run".into(), b"approved".to_vec(), true),
            ],
            PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap(),
            |_, _, _, _| Ok(()),
        )
        .unwrap()
    }

    #[test]
    fn last_instant_destination_collision_preserves_both_trees() {
        for kind in ["directory", "file", "symlink"] {
            let f = Fixture::new();
            let prepared = prepare(&f);
            assert!(prepared
                .apply_with_hooks(
                    || {
                        match kind {
                            "directory" => {
                                fs::create_dir(f.source.join("new")).unwrap();
                                fs::write(f.source.join("new/user"), b"keep").unwrap();
                            }
                            "file" => fs::write(f.source.join("new"), b"keep").unwrap(),
                            _ => std::os::unix::fs::symlink(
                                f.source.join("file"),
                                f.source.join("new"),
                            )
                            .unwrap(),
                        }
                    },
                    || {},
                    File::sync_all
                )
                .is_err());
            assert_eq!(
                fs::read(f.recovery.join("exchange/nested/run")).unwrap(),
                b"approved"
            );
            assert!(f.recovery.join("exchange/nested/empty").is_dir());
            if kind == "directory" {
                assert_eq!(fs::read(f.source.join("new/user")).unwrap(), b"keep");
            } else if kind == "file" {
                assert_eq!(fs::read(f.source.join("new")).unwrap(), b"keep");
            } else {
                assert!(fs::symlink_metadata(f.source.join("new"))
                    .unwrap()
                    .file_type()
                    .is_symlink());
            }
            assert_eq!(fs::read(f.source.join("file")).unwrap(), b"before");
        }
    }

    #[test]
    fn uncertain_tree_installation_never_undoes_or_cleans_up_late_work() {
        for kind in ["edit", "extra", "policy", "replace", "sync"] {
            let f = Fixture::new();
            let prepared = prepare(&f);
            let mut held = fs::OpenOptions::new()
                .write(true)
                .open(f.recovery.join("exchange/nested/run"))
                .unwrap();
            let syncs = std::cell::Cell::new(0);
            let observed = prepared
                .apply_with_hooks(
                    || {},
                    || match kind {
                        "edit" => fs::write(f.source.join("new/nested/run"), b"editor").unwrap(),
                        "extra" => fs::write(f.source.join("new/user"), b"keep").unwrap(),
                        "policy" => {
                            let mode = fs::metadata(&f.source).unwrap().mode() ^ 0o010;
                            fs::set_permissions(&f.source, fs::Permissions::from_mode(mode))
                                .unwrap();
                        }
                        "replace" => {
                            fs::rename(f.source.join("new"), f.source.join("moved")).unwrap();
                            fs::create_dir(f.source.join("new")).unwrap();
                            fs::write(f.source.join("new/user"), b"keep").unwrap();
                        }
                        _ => {}
                    },
                    |file| {
                        syncs.set(syncs.get() + 1);
                        if kind == "sync" {
                            Err(io::Error::other("injected durability failure"))
                        } else {
                            file.sync_all()
                        }
                    },
                )
                .unwrap();
            assert!(!observed, "{kind}");
            assert_eq!(syncs.get(), 2);
            assert!(!f.recovery.join("exchange").exists());
            held.write_all(b"late").unwrap();
            let installed = if kind == "replace" { "moved" } else { "new" };
            assert!(fs::read(f.source.join(installed).join("nested/run"))
                .unwrap()
                .starts_with(b"late"));
            assert!(f.source.join(installed).join("nested/empty").is_dir());
            if kind == "replace" || kind == "extra" {
                assert_eq!(fs::read(f.source.join("new/user")).unwrap(), b"keep");
            }
        }
    }

    #[test]
    fn preparation_receipt_failure_and_replaced_source_preserve_staging() {
        let f = Fixture::new();
        let result = RetainedTreeAddition::prepare(
            PinnedWorkspaceRoot::open(f.source.clone()).unwrap(),
            PathBuf::from("new"),
            &[],
            PinnedWorkspaceRoot::open(f.recovery.clone()).unwrap(),
            |_, _, _, _| Err(io::Error::other("receipt failure")),
        );
        assert!(result.is_err());
        assert!(!f.source.join("new").exists());
        assert!(f.recovery.join("exchange").is_dir());
        let f = Fixture::new();
        let prepared = prepare(&f);
        let moved = f.source.with_extension("moved");
        fs::rename(&f.source, &moved).unwrap();
        fs::create_dir(&f.source).unwrap();
        assert!(prepared.apply().is_err());
        assert!(!f.source.join("new").exists());
        assert!(!moved.join("new").exists());
        assert_eq!(
            fs::read(f.recovery.join("exchange/nested/run")).unwrap(),
            b"approved"
        );
    }

    #[test]
    #[allow(unsafe_code)]
    fn tree_umask_is_preserved_in_an_isolated_process() {
        const CHILD: &str = "MESH_TREE_UMASK_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "managed_file::retained_replacement::tree_addition::tests::tree_umask_is_preserved_in_an_isolated_process", "--test-threads=1"])
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
        // SAFETY: only this isolated single-test child changes the process-global mask.
        unsafe {
            umask(0o077);
        }
        let f = Fixture::new();
        assert!(prepare(&f).apply().unwrap());
        for path in ["new", "new/nested", "new/nested/empty", "new/nested/run"] {
            assert_eq!(
                fs::metadata(f.source.join(path)).unwrap().mode() & 0o777,
                0o700
            );
        }
    }
}
