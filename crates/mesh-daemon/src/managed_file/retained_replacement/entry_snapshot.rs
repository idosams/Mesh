//! Freeze complete retained entries without moving their original objects or open handles.
use super::*;
use crate::ipc::Json;
use std::collections::BTreeMap;
use std::ffi::OsStr;

struct FrozenEntry {
    path: String,
    descriptor: File,
    content: Option<Vec<u8>>,
}
pub(crate) struct EntrySnapshot {
    pub(crate) evidence: Json,
    entries: Vec<FrozenEntry>,
}
impl EntrySnapshot {
    pub(crate) fn capture(
        root: &PinnedWorkspaceRoot,
        path: &Path,
        limits: EntryLimits,
    ) -> io::Result<Self> {
        let evidence = observe_entry(root, path, limits)?;
        let mut entries = Vec::new();
        let mut remaining = limits.bytes.min(64 * 1024 * 1024);
        for entry in evidence.as_array().unwrap() {
            let name = entry.get("path").and_then(Json::as_text).unwrap();
            let relative = if name.is_empty() {
                path.to_owned()
            } else {
                path.join(name)
            };
            let (descriptor, content) = if entry.get("kind") == Some(&Json::text("file")) {
                let (_, content, fd) =
                    snapshot_file(root, &relative, remaining.min(limits.file_bytes))?;
                remaining = remaining
                    .checked_sub(content.len() as u64)
                    .ok_or_else(|| io::Error::other("snapshot byte budget exceeded"))?;
                (fd, Some(content))
            } else {
                (open_tree(root, &relative)?.try_clone_directory()?, None)
            };
            if tree_addition::prepared_entry(name, &descriptor, content.as_deref())? != *entry {
                return Err(io::Error::other("entry changed while freezing content"));
            }
            // Special permission semantics need explicit support, as for single-file restoration.
            if descriptor.metadata()?.mode() & 0o7000 != 0 {
                return Err(io::Error::other(
                    "special entry permissions are unsupported",
                ));
            }
            entries.push(FrozenEntry {
                path: name.to_owned(),
                descriptor,
                content,
            });
        }
        if observe_entry(root, path, limits)? != evidence {
            return Err(io::Error::other("tree changed while freezing content"));
        }
        Ok(Self { evidence, entries })
    }
    pub(crate) fn files(&self) -> impl Iterator<Item = (&str, &[u8], bool)> {
        self.entries
            .iter()
            .zip(self.evidence.as_array().unwrap())
            .filter_map(|(entry, fact)| {
                entry.content.as_deref().map(|bytes| {
                    (
                        entry.path.as_str(),
                        bytes,
                        fact.get("mode").and_then(Json::as_u64).unwrap() & 0o111 != 0,
                    )
                })
            })
    }
    pub(crate) fn bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter_map(|entry| entry.content.as_ref())
            .map(|bytes| bytes.len() as u64)
            .sum()
    }
    /// Build all children before copying directory permissions/ACLs; restrictive retained metadata
    /// must not be silently widened to make construction succeed. Every copied object is new.
    pub(crate) fn stage(
        &self,
        recovery: &PinnedWorkspaceRoot,
        limits: EntryLimits,
    ) -> io::Result<Json> {
        recovery.ensure_namespace_identity()?;
        let mut directories = BTreeMap::from([(String::new(), recovery.clone())]);
        let mut descriptors = Vec::new();
        for entry in &self.entries {
            let staged_path = if entry.path.is_empty() {
                EXCHANGE.to_owned()
            } else {
                format!("{EXCHANGE}/{}", entry.path)
            };
            let (parent, name) = staged_path
                .rsplit_once('/')
                .unwrap_or(("", staged_path.as_str()));
            let directory = directories
                .get(parent)
                .ok_or_else(|| io::Error::other("snapshot parent unavailable"))?;
            let fd = if let Some(bytes) = &entry.content {
                let mut fd = create_new_at_mode(&directory.try_clone_directory()?, name, 0o600)?;
                fd.write_all(bytes)?;
                fd
            } else {
                let child = directory.create_child_directory_with_mode(OsStr::new(name), 0o700)?;
                let fd = child.try_clone_directory()?;
                directories.insert(staged_path, child);
                fd
            };
            descriptors.push(fd);
        }
        let mut expected = Vec::new();
        for ((entry, original), fd) in self
            .entries
            .iter()
            .zip(self.evidence.as_array().unwrap())
            .zip(&descriptors)
            .rev()
        {
            let mode = original.get("mode").and_then(Json::as_u64).unwrap() as u32;
            fd.set_permissions(fs::Permissions::from_mode(mode))?;
            copy_metadata(&entry.descriptor, fd)?;
            fd.set_permissions(fs::Permissions::from_mode(mode))?;
            fd.sync_all()?;
            let observed =
                tree_addition::prepared_entry(&entry.path, fd, entry.content.as_deref())?;
            for field in ["path", "kind", "mode", "metadata", "digest", "bytes"] {
                if observed.get(field) != original.get(field) {
                    return Err(io::Error::other(
                        "snapshot metadata or content was not preserved",
                    ));
                }
            }
            expected.push(observed);
        }
        expected.reverse();
        let evidence = Json::Array(expected);
        recovery.try_clone_directory()?.sync_all()?;
        if observe_entry(recovery, Path::new(EXCHANGE), limits)? != evidence {
            return Err(io::Error::other("snapshot stage changed"));
        }
        Ok(evidence)
    }
}
