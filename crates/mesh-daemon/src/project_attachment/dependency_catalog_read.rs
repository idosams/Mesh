//! Native catalog reads without reconstructing a consumption transaction request.
use super::{
    capture_line::CaptureLine, dependency_owner_context::OwnerHistoryContext, invalid,
    AttachmentStorage, ProvisionedAttachment, SavedAttachmentVersion,
};
use crate::{workspace::OpenWorkspace, workspace_custody::lock_workspace_initialization_set};
use std::{collections::BTreeMap, io};

fn error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

impl AttachmentStorage {
    /// List exact versions of enrolled native work, including completed consumed lanes.
    /// IDs select existing native registrations, not paths or authority. The supplied input IDs
    /// are candidates only: missing, conflicting or incomplete consumption closure refuses.
    /// This does not recover an interrupted transaction or authorize capture, execution or main.
    pub fn saved_dependency_versions(
        &self,
        owner_id: &str,
        work_id: &str,
        input_ids: &[&str],
    ) -> io::Result<Vec<SavedAttachmentVersion>> {
        self.with_catalog_dependency_history(
            owner_id,
            work_id,
            input_ids,
            |work, history, configuration| {
                let line = CaptureLine::load(&work.store, history, configuration)?;
                history
                    .linear_history(line.head)
                    .map_err(error)?
                    .into_iter()
                    .map(|operation| {
                        SavedAttachmentVersion::from_verified_history(history, operation)
                    })
                    .collect()
            },
        )
    }

    /// Read immutable saved bytes using native catalog context; never fall back to editor files.
    pub fn saved_dependency_file(
        &self,
        owner_id: &str,
        work_id: &str,
        input_ids: &[&str],
        version: SavedAttachmentVersion,
        relative: &str,
    ) -> io::Result<Option<Vec<u8>>> {
        self.with_catalog_dependency_history(owner_id, work_id, input_ids, |_, history, _| {
            Ok(history
                .historical_workspace_file(version.operation(), relative)
                .map_err(error)?
                .map(|file| file.bytes))
        })
    }

    fn with_catalog_dependency_history<T>(
        &self,
        owner_id: &str,
        work_id: &str,
        input_ids: &[&str],
        action: impl FnOnce(&ProvisionedAttachment, &OpenWorkspace, &str) -> io::Result<T>,
    ) -> io::Result<T> {
        self.with_checked_catalog_dependency_history(
            owner_id,
            work_id,
            input_ids,
            |_| Ok(()),
            action,
        )
    }

    pub(super) fn with_checked_catalog_dependency_history<T>(
        &self,
        owner_id: &str,
        work_id: &str,
        input_ids: &[&str],
        check_selection: impl Fn(
            &crate::workspace_custody::WorkspaceInitializationGuard,
        ) -> io::Result<()>,
        action: impl FnOnce(&ProvisionedAttachment, &OpenWorkspace, &str) -> io::Result<T>,
    ) -> io::Result<T> {
        if input_ids.len() > 256 {
            return Err(invalid("catalog input selection exceeds bound"));
        }
        let mut works = BTreeMap::new();
        for id in std::iter::once(owner_id)
            .chain(std::iter::once(work_id))
            .chain(input_ids.iter().copied())
        {
            if !works.contains_key(id) {
                works.insert(id.to_owned(), self.reopen(id)?);
            }
        }
        let owner = &works[owner_id];
        let work = &works[work_id];
        let mut selected = Vec::new();
        let mut roots = BTreeMap::new();
        for candidate in works.values() {
            let selection = self.prepare_dependency_work(owner, candidate)?;
            for root in &selection.roots {
                roots
                    .entry(root.identity()?)
                    .or_insert_with(|| root.clone());
            }
            if roots.len() > 32 {
                return Err(invalid("complete catalog read custody exceeds bound"));
            }
            selected.push((candidate, selection));
        }
        let guard = lock_workspace_initialization_set(&roots.into_values().collect::<Vec<_>>())
            .map_err(error)?;
        check_selection(&guard)?;
        let handles = works.values().collect::<Vec<_>>();
        let context = self.resolve_consumed_histories(
            owner,
            &handles,
            &guard,
            OwnerHistoryContext::current(owner),
        )?;
        let mut before = Vec::new();
        for (candidate, selection) in &selected {
            context.validate(self, selection, &guard)?;
            before.push(context.read(candidate)?);
            context.verified_history_roots(candidate)?;
        }
        let (configuration, proof, history) = context.history(work)?;
        let result = action(work, &history, &configuration)?;
        for ((candidate, selection), snapshot) in selected.iter().zip(before) {
            context.validate(self, selection, &guard)?;
            if context.read(candidate)? != snapshot {
                return Err(invalid("catalog history changed during read"));
            }
            context.verified_history_roots(candidate)?;
        }
        if context.read(work)? != (configuration, Some(proof)) {
            return Err(invalid("selected catalog history changed during read"));
        }
        check_selection(&guard)?;
        guard.ensure_current().map_err(error)?;
        Ok(result)
    }
}

#[cfg(test)]
impl AttachmentStorage {
    pub(in crate::project_attachment) fn assert_catalog_read_freshness(
        &self,
        owner_id: &str,
        work_id: &str,
        input_ids: &[&str],
    ) {
        use std::io::Write as _;
        let owner = self.reopen(owner_id).unwrap();
        let path = owner.metadata_path().join(crate::RECORD_FILE_NAME);
        struct Restore(std::path::PathBuf, Vec<u8>);
        impl Drop for Restore {
            fn drop(&mut self) {
                std::fs::write(&self.0, &self.1).expect("restore fixture owner journal");
            }
        }
        let restore = Restore(path.clone(), std::fs::read(&path).unwrap());
        let result =
            self.with_catalog_dependency_history(owner_id, work_id, input_ids, |_, _, _| {
                let mut journal = std::fs::OpenOptions::new().append(true).open(&path)?;
                journal.write_all(&[1])?;
                journal.sync_all()?;
                Ok(())
            });
        drop(restore);
        assert!(
            result.is_err(),
            "catalog read returned success after owner history changed"
        );
        self.saved_dependency_versions(owner_id, work_id, input_ids)
            .unwrap();
    }
}
