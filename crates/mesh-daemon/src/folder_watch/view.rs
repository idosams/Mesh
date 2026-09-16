//! One folder on the real filesystem, presented as a [`WorkspaceView`].
//!
//! # One sentence this file is built around
//!
//! **Every question is answered by reading the folder again.** The object index this view keeps is
//! a *cache* and never the truth: [`DirectoryView::resolve`] checks a cached path against the
//! device and inode the kernel reports before trusting it, and [`DirectoryView::rescan`] rebuilds
//! the whole index when it does not agree. That is what makes a file moved behind Mesh's back
//! reachable again rather than lost, which is acceptance criterion 3 of task
//! `01KZC2QR9VVJK6Y60PS8D360JT` on the path inside the adapter rather than beside it.
//!
//! # Why this is a separate file from the adapter
//!
//! [`super`] owns the *seam*: what the backend declares, which workspace and head it accepts, and
//! how a view is created and released. This owns the *operations* — the eighteen `WorkspaceView`
//! methods over `read_dir`, `pread`, `pwrite` and `rename`. Two subjects, split at the API
//! boundary rather than by line count (plan §14.3 rule 8).

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::{FileExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use mesh_materializer::{
    AdapterError, DestinationBefore, EventSequence, NormalizedName, ObjectId, ObjectKind,
    OpenHandle, OpenMode, PortableMetadata, RenameBinding, RenameBindingEvidence,
    RenameDisposition, ViewAccess, ViewEntry, ViewId, WorkspaceView,
};

use super::{backend, lock, missing_or_backend, mode_for, object_of, portable_of};

#[derive(Default)]
struct ViewState {
    /// Where each object was last seen. A cache: [`DirectoryView::resolve`] verifies an entry
    /// against the filesystem before trusting it, and re-reads the whole folder when it is wrong.
    index: BTreeMap<ObjectId, PathBuf>,
    handles: BTreeMap<u64, File>,
    next_handle: u64,
}

/// One folder, presented as a workspace.
pub struct DirectoryView {
    id: ViewId,
    access: ViewAccess,
    at: PathBuf,
    root_object: ObjectId,
    state: Mutex<ViewState>,
}

impl DirectoryView {
    /// A view of `at`, with `access` deciding whether it will accept a write.
    pub(super) fn new(
        id: ViewId,
        access: ViewAccess,
        at: PathBuf,
        metadata: &fs::Metadata,
    ) -> Self {
        let root_object = object_of(metadata);
        let mut state = ViewState {
            next_handle: 1,
            ..ViewState::default()
        };
        state.index.insert(root_object, at.clone());
        Self {
            id,
            access,
            at,
            root_object,
            state: Mutex::new(state),
        }
    }

    /// The folder this view presents.
    #[must_use]
    pub fn folder(&self) -> &Path {
        &self.at
    }

    /// Read the whole folder again and rebuild the object index from what is actually there.
    ///
    /// The reconciliation path, used in anger rather than kept for a test: [`Self::resolve`] calls
    /// it whenever the cache disagrees with the filesystem, so an object moved behind this
    /// backend's back is found again rather than reported missing.
    pub fn rescan(&self) {
        let mut found: Vec<(ObjectId, PathBuf)> = Vec::new();
        collect(&self.at, &mut found);
        let mut state = lock(&self.state);
        state.index.clear();
        state.index.insert(self.root_object, self.at.clone());
        for (object, path) in found {
            state.index.insert(object, path);
        }
    }

    fn cached(&self, object: ObjectId) -> Option<PathBuf> {
        let path = lock(&self.state).index.get(&object)?.clone();
        let metadata = fs::symlink_metadata(&path).ok()?;
        (object_of(&metadata) == object).then_some(path)
    }

    fn resolve(&self, object: ObjectId) -> Result<PathBuf, AdapterError> {
        if let Some(path) = self.cached(object) {
            return Ok(path);
        }
        self.rescan();
        self.cached(object).ok_or(AdapterError::NotFound)
    }

    fn note(&self, object: ObjectId, path: PathBuf) {
        lock(&self.state).index.insert(object, path);
    }

    fn directory(&self, object: ObjectId) -> Result<PathBuf, AdapterError> {
        let path = self.resolve(object)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| missing_or_backend(&error))?;
        if metadata.is_dir() {
            Ok(path)
        } else {
            Err(AdapterError::NotADirectory)
        }
    }

    fn writable(&self) -> Result<(), AdapterError> {
        if self.access.is_read_only() {
            Err(AdapterError::ReadOnly)
        } else {
            Ok(())
        }
    }

    fn entry_at(&self, name: &NormalizedName, path: &Path) -> Result<ViewEntry, AdapterError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| missing_or_backend(&error))?;
        let kind = if metadata.is_dir() {
            ObjectKind::Directory
        } else if metadata.is_file() {
            ObjectKind::File
        } else {
            // A link is neither, `Symlink` is reserved at contract 1 and this backend does not
            // declare it, so there is nothing here this view is able to describe. Published as the
            // `links-are-not-presented` restriction rather than left as a silent omission: a
            // folder that holds one presents fewer entries than it has, and a person who is not
            // told that will believe the listing is the folder.
            return Err(AdapterError::NotFound);
        };
        let object = object_of(&metadata);
        self.note(object, path.to_path_buf());
        Ok(ViewEntry::new(
            name.clone(),
            object,
            kind,
            // A folder names no versions. `None` is a real answer here, not a missing one.
            None,
            portable_of(&metadata),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn rename_or_move_with_evidence(
        &self,
        sequence: EventSequence,
        from_parent: ObjectId,
        from: &NormalizedName,
        to_parent: ObjectId,
        to: &NormalizedName,
        disposition: RenameDisposition,
    ) -> Result<RenameBindingEvidence, AdapterError> {
        self.writable()?;
        let source = self.directory(from_parent)?.join(from.as_str());
        let destination_directory = self.directory(to_parent)?;
        let destination = destination_directory.join(to.as_str());
        let source_metadata =
            fs::symlink_metadata(&source).map_err(|error| missing_or_backend(&error))?;
        if source_metadata.is_dir() && destination_directory.starts_with(&source) {
            return Err(AdapterError::WouldCycle);
        }
        let source_object = object_of(&source_metadata);
        let destination_before = match fs::symlink_metadata(&destination) {
            Ok(metadata) => {
                let destination_object = object_of(&metadata);
                if destination_object != source_object {
                    if disposition == RenameDisposition::Fail {
                        return Err(AdapterError::AlreadyExists);
                    }
                    if metadata.is_dir() {
                        return Err(AdapterError::IsADirectory);
                    }
                    if source_metadata.is_dir() {
                        return Err(AdapterError::NotADirectory);
                    }
                }
                DestinationBefore::Bound(destination_object)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => DestinationBefore::Unbound,
            Err(error) => return Err(backend(&error)),
        };

        if source != destination {
            fs::rename(&source, &destination).map_err(|error| backend(&error))?;
        }
        let after = fs::symlink_metadata(&destination).map_err(|error| backend(&error))?;
        let after_object = object_of(&after);
        if after_object != source_object {
            return Err(AdapterError::Backend(
                "rename-binding-evidence-unavailable: moved object identity changed".to_owned(),
            ));
        }
        self.note(source_object, destination);
        Ok(RenameBindingEvidence::new(
            self.id,
            sequence,
            RenameBinding::new(from_parent, from.clone(), source_object),
            destination_before,
            RenameBinding::new(to_parent, to.clone(), after_object),
        ))
    }
}

fn collect(root: &Path, into: &mut Vec<(ObjectId, PathBuf)>) {
    // An explicit stack avoids both the old artificial depth-64 cutoff and call-stack exhaustion.
    // `symlink_metadata` means a link is never placed on the stack, so removing the depth bound
    // cannot turn a link cycle into an unbounded traversal.
    let mut pending = vec![root.to_path_buf()];
    while let Some(at) = pending.pop() {
        let Ok(listing) = fs::read_dir(at) else {
            continue;
        };
        for entry in listing.flatten() {
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !(metadata.is_file() || metadata.is_dir()) {
                continue;
            }
            into.push((object_of(&metadata), path.clone()));
            if metadata.is_dir() {
                pending.push(path);
            }
        }
    }
}

impl WorkspaceView for DirectoryView {
    fn id(&self) -> ViewId {
        self.id
    }

    fn access(&self) -> ViewAccess {
        self.access
    }

    fn root(&self) -> ObjectId {
        self.root_object
    }

    fn lookup(&self, parent: ObjectId, name: &NormalizedName) -> Result<ViewEntry, AdapterError> {
        let at = self.directory(parent)?;
        self.entry_at(name, &at.join(name.as_str()))
    }

    fn enumerate(&self, directory: ObjectId) -> Result<Vec<ViewEntry>, AdapterError> {
        let at = self.directory(directory)?;
        let listing = fs::read_dir(&at).map_err(|error| missing_or_backend(&error))?;
        let mut entries: Vec<ViewEntry> = Vec::new();
        for entry in listing.flatten() {
            let Some(text) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(name) = NormalizedName::new(text) else {
                // A name the portable rules refuse cannot be presented as an entry. Skipped rather
                // than refused: one such name on disk would otherwise make the whole folder
                // unreadable, and the fallback exists to keep a person working.
                continue;
            };
            match self.entry_at(&name, &entry.path()) {
                Ok(entry) => entries.push(entry),
                Err(AdapterError::NotFound) => continue,
                Err(other) => return Err(other),
            }
        }
        entries.sort_by(|left, right| {
            left.name()
                .as_str()
                .as_bytes()
                .cmp(right.name().as_str().as_bytes())
        });
        Ok(entries)
    }

    fn open(&self, object: ObjectId, mode: OpenMode) -> Result<OpenHandle, AdapterError> {
        let path = self.resolve(object)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| missing_or_backend(&error))?;
        if metadata.is_dir() {
            return Err(AdapterError::IsADirectory);
        }
        if mode.writes() {
            self.writable()?;
        }
        let file = OpenOptions::new()
            .read(mode.reads())
            .write(mode.writes())
            .open(&path)
            .map_err(|error| missing_or_backend(&error))?;
        let mut state = lock(&self.state);
        let handle = state.next_handle;
        state.next_handle += 1;
        state.handles.insert(handle, file);
        Ok(OpenHandle::new(handle, self.id, object, mode))
    }

    fn close(&self, handle: OpenHandle) -> Result<(), AdapterError> {
        if handle.view() != self.id {
            return Err(AdapterError::NotFound);
        }
        lock(&self.state)
            .handles
            .remove(&handle.handle())
            .map(|_| ())
            .ok_or(AdapterError::NotFound)
    }

    fn read(
        &self,
        handle: &OpenHandle,
        offset: u64,
        into: &mut [u8],
    ) -> Result<usize, AdapterError> {
        if handle.view() != self.id {
            return Err(AdapterError::NotFound);
        }
        let state = lock(&self.state);
        let file = state
            .handles
            .get(&handle.handle())
            .ok_or(AdapterError::NotFound)?;
        // `read_at` answers what it actually filled. A short read is reported as a short read,
        // which is the whole reason the seam borrows a buffer and returns a count.
        file.read_at(into, offset).map_err(|error| backend(&error))
    }

    fn write(&self, handle: &OpenHandle, offset: u64, from: &[u8]) -> Result<usize, AdapterError> {
        // Before the handle lookup, deliberately: a read-only view owes `ReadOnly` for a write it
        // was never going to accept, and answering `NotFound` for an unregistered handle first
        // would tell the caller the wrong thing about the view.
        self.writable()?;
        if handle.view() != self.id {
            return Err(AdapterError::NotFound);
        }
        let state = lock(&self.state);
        let file = state
            .handles
            .get(&handle.handle())
            .ok_or(AdapterError::NotFound)?;
        file.write_at(from, offset).map_err(|error| backend(&error))
    }

    fn set_file_length(&self, object: ObjectId, length: u64) -> Result<(), AdapterError> {
        // Keep refusal precedence consistent with every other mutation: a read-only view refuses
        // the mutation before consulting an object it was never going to change.
        self.writable()?;
        let path = self.resolve(object)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| missing_or_backend(&error))?;
        if metadata.is_dir() {
            return Err(AdapterError::IsADirectory);
        }

        // Unix file offsets are signed. Refuse a value the platform cannot represent before an
        // implementation-defined cast or syscall error can turn it into a different length.
        i64::try_from(length).map_err(|_| {
            AdapterError::Backend("file length exceeds the platform offset range".to_owned())
        })?;

        let file = OpenOptions::new()
            .write(true)
            .open(&path)
            .map_err(|error| missing_or_backend(&error))?;
        let opened = file
            .metadata()
            .map_err(|error| missing_or_backend(&error))?;
        if opened.is_dir() {
            return Err(AdapterError::IsADirectory);
        }
        // `resolve` is confined to this view, but the directory can change between resolution and
        // open. Verify the opened descriptor still names the requested object before mutating it;
        // a symlink swap therefore cannot redirect truncation outside the owned folder.
        if object_of(&opened) != object {
            self.rescan();
            return Err(AdapterError::NotFound);
        }
        file.set_len(length).map_err(|error| backend(&error))
    }

    fn create_file(
        &self,
        parent: ObjectId,
        name: &NormalizedName,
        metadata: PortableMetadata,
    ) -> Result<ViewEntry, AdapterError> {
        self.writable()?;
        let at = self.directory(parent)?.join(name.as_str());
        match OpenOptions::new().write(true).create_new(true).open(&at) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(AdapterError::AlreadyExists)
            }
            Err(error) => return Err(backend(&error)),
        }
        fs::set_permissions(&at, fs::Permissions::from_mode(mode_for(metadata, false)))
            .map_err(|error| backend(&error))?;
        self.entry_at(name, &at)
    }

    fn create_directory(
        &self,
        parent: ObjectId,
        name: &NormalizedName,
    ) -> Result<ViewEntry, AdapterError> {
        self.writable()?;
        let at = self.directory(parent)?.join(name.as_str());
        match fs::create_dir(&at) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(AdapterError::AlreadyExists)
            }
            Err(error) => return Err(backend(&error)),
        }
        self.entry_at(name, &at)
    }

    fn rename(
        &self,
        parent: ObjectId,
        from: &NormalizedName,
        to: &NormalizedName,
    ) -> Result<(), AdapterError> {
        self.writable()?;
        let at = self.directory(parent)?;
        let (source, destination) = (at.join(from.as_str()), at.join(to.as_str()));
        let metadata = fs::symlink_metadata(&source).map_err(|error| missing_or_backend(&error))?;
        if fs::symlink_metadata(&destination).is_ok() {
            // `fs::rename` would replace it. The contract refuses instead, because replacing an
            // entry a caller did not name is the kind of loss nobody finds until later.
            return Err(AdapterError::AlreadyExists);
        }
        fs::rename(&source, &destination).map_err(|error| backend(&error))?;
        self.note(object_of(&metadata), destination);
        Ok(())
    }

    fn rename_with_evidence(
        &self,
        sequence: EventSequence,
        parent: ObjectId,
        from: &NormalizedName,
        to: &NormalizedName,
        disposition: RenameDisposition,
    ) -> Result<RenameBindingEvidence, AdapterError> {
        self.rename_or_move_with_evidence(sequence, parent, from, parent, to, disposition)
    }

    fn move_entry(
        &self,
        from_parent: ObjectId,
        from: &NormalizedName,
        to_parent: ObjectId,
        to: &NormalizedName,
    ) -> Result<(), AdapterError> {
        self.writable()?;
        let source = self.directory(from_parent)?.join(from.as_str());
        let into = self.directory(to_parent)?;
        let metadata = fs::symlink_metadata(&source).map_err(|error| missing_or_backend(&error))?;
        if metadata.is_dir() && into.starts_with(&source) {
            // Checked here rather than left to the kernel: `rename` answers `EINVAL` for this and
            // the contract names it `WouldCycle`, and a backend that let the platform decide would
            // report a different refusal on a platform whose kernel disagreed.
            return Err(AdapterError::WouldCycle);
        }
        let destination = into.join(to.as_str());
        if fs::symlink_metadata(&destination).is_ok() {
            return Err(AdapterError::AlreadyExists);
        }
        fs::rename(&source, &destination).map_err(|error| backend(&error))?;
        self.note(object_of(&metadata), destination);
        Ok(())
    }

    fn move_entry_with_evidence(
        &self,
        sequence: EventSequence,
        from_parent: ObjectId,
        from: &NormalizedName,
        to_parent: ObjectId,
        to: &NormalizedName,
        disposition: RenameDisposition,
    ) -> Result<RenameBindingEvidence, AdapterError> {
        self.rename_or_move_with_evidence(sequence, from_parent, from, to_parent, to, disposition)
    }

    fn unlink(&self, parent: ObjectId, name: &NormalizedName) -> Result<(), AdapterError> {
        self.writable()?;
        let at = self.directory(parent)?.join(name.as_str());
        let metadata = fs::symlink_metadata(&at).map_err(|error| missing_or_backend(&error))?;
        if metadata.is_dir() {
            return Err(AdapterError::IsADirectory);
        }
        fs::remove_file(&at).map_err(|error| missing_or_backend(&error))
    }

    fn remove_directory(
        &self,
        parent: ObjectId,
        name: &NormalizedName,
    ) -> Result<(), AdapterError> {
        self.writable()?;
        let at = self.directory(parent)?.join(name.as_str());
        let metadata = fs::symlink_metadata(&at).map_err(|error| missing_or_backend(&error))?;
        if !metadata.is_dir() {
            return Err(AdapterError::NotADirectory);
        }
        let mut listing = fs::read_dir(&at).map_err(|error| missing_or_backend(&error))?;
        if listing.next().is_some() {
            // Never a recursive delete, and the emptiness is established before anything is
            // removed rather than inferred from the kernel's refusal afterwards.
            return Err(AdapterError::DirectoryNotEmpty);
        }
        fs::remove_dir(&at).map_err(|error| missing_or_backend(&error))
    }

    fn metadata(&self, object: ObjectId) -> Result<PortableMetadata, AdapterError> {
        let path = self.resolve(object)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| missing_or_backend(&error))?;
        Ok(portable_of(&metadata))
    }

    fn set_metadata(
        &self,
        object: ObjectId,
        metadata: PortableMetadata,
    ) -> Result<(), AdapterError> {
        self.writable()?;
        let path = self.resolve(object)?;
        let existing = fs::symlink_metadata(&path).map_err(|error| missing_or_backend(&error))?;
        fs::set_permissions(
            &path,
            fs::Permissions::from_mode(mode_for(metadata, existing.is_dir())),
        )
        .map_err(|error| backend(&error))
    }
}
