//! Candidate discovery never admits history: exact catalog reads revalidate under complete custody.
#[cfg(test)]
mod tests;
use super::{
    capture_line::CaptureLine, invalid, AttachmentStorage, ProvisionedAttachment,
    SavedAttachmentVersion, VerifiedDependencyRead,
};
use mesh_store::RecordDigest;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};

type Key = (RecordDigest, RecordDigest);
type Hint = (Key, Vec<String>);
pub(super) type CatalogHints = (BTreeMap<String, Hint>, BTreeMap<Key, Vec<String>>);
#[derive(PartialEq, Eq)]
struct Selection {
    owner: (String, Option<VerifiedDependencyRead>),
    works: BTreeMap<String, Key>,
}
fn error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

impl AttachmentStorage {
    /// Resolve recorded owning-root ancestry and required inputs for one enrolled native work.
    /// Catalog IDs are selectors only; the final read validates exact ancestry and history under
    /// complete custody. Unenrolled work and incomplete ancestry refuse without a local fallback.
    pub fn registered_dependency_versions(
        &self,
        work_id: &str,
    ) -> io::Result<Vec<SavedAttachmentVersion>> {
        let owner_id = self.candidate_owning_root(work_id)?;
        self.discovered_dependency_versions(&owner_id, work_id)
    }

    /// Read exact saved bytes after resolving owning-root ancestry from native registrations.
    /// This grants no capture, execution, publication or pending-transaction recovery authority.
    pub fn registered_dependency_file(
        &self,
        work_id: &str,
        version: SavedAttachmentVersion,
        relative: &str,
    ) -> io::Result<Option<Vec<u8>>> {
        let owner_id = self.candidate_owning_root(work_id)?;
        self.discovered_dependency_file(&owner_id, work_id, version, relative)
    }

    /// Discover required native inputs and list exact saved versions without a caller input list.
    /// The owner must be the native owning root; this grants no capture or publication permission.
    pub fn discovered_dependency_versions(
        &self,
        owner_id: &str,
        work_id: &str,
    ) -> io::Result<Vec<SavedAttachmentVersion>> {
        let selection = self.discover_dependency_read(owner_id, work_id)?;
        let inputs = selection
            .works
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        self.with_checked_catalog_dependency_history(
            owner_id,
            work_id,
            &inputs,
            |guard| self.verify_discovery(owner_id, work_id, &selection, guard),
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

    /// Discover native inputs and read saved bytes, never current editor bytes.
    pub fn discovered_dependency_file(
        &self,
        owner_id: &str,
        work_id: &str,
        version: SavedAttachmentVersion,
        relative: &str,
    ) -> io::Result<Option<Vec<u8>>> {
        let selection = self.discover_dependency_read(owner_id, work_id)?;
        let inputs = selection
            .works
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        self.with_checked_catalog_dependency_history(
            owner_id,
            work_id,
            &inputs,
            |guard| self.verify_discovery(owner_id, work_id, &selection, guard),
            |_, history, _| {
                Ok(history
                    .historical_workspace_file(version.operation(), relative)
                    .map_err(error)?
                    .map(|file| file.bytes))
            },
        )
    }

    pub(super) fn with_registered_dependency_history<T>(
        &self,
        work_id: &str,
        action: impl FnOnce(
            &ProvisionedAttachment,
            &crate::workspace::OpenWorkspace,
            &str,
        ) -> io::Result<T>,
    ) -> io::Result<T> {
        self.with_registered_dependency_context(work_id, |context| {
            action(context.work, context.history, context.configuration)
        })
    }

    pub(super) fn with_registered_dependency_context<T>(
        &self,
        work_id: &str,
        action: impl FnOnce(
            &super::dependency_catalog_read::VerifiedCatalogDependencyRead<'_>,
        ) -> io::Result<T>,
    ) -> io::Result<T> {
        let owner_id = self.candidate_owning_root(work_id)?;
        let selection = self.discover_dependency_read(&owner_id, work_id)?;
        let inputs = selection
            .works
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        self.with_checked_catalog_dependency_context(
            &owner_id,
            work_id,
            &inputs,
            |guard| self.verify_discovery(&owner_id, work_id, &selection, guard),
            action,
        )
    }

    pub(super) fn registered_capture_inputs(
        &self,
        work_id: &str,
    ) -> io::Result<(ProvisionedAttachment, BTreeMap<String, Key>)> {
        let owner_id = self.candidate_owning_root(work_id)?;
        let selection = self.discover_dependency_read(&owner_id, work_id)?;
        Ok((self.reopen(&owner_id)?, selection.works))
    }

    fn verify_discovery(
        &self,
        owner_id: &str,
        work_id: &str,
        selection: &Selection,
        guard: &crate::workspace_custody::WorkspaceInitializationGuard,
    ) -> io::Result<()> {
        if self.discover_dependency_read_held(owner_id, work_id, guard)? != *selection {
            return Err(invalid(
                "native input discovery changed before or during read",
            ));
        }
        Ok(())
    }

    fn discover_dependency_read(&self, owner_id: &str, work_id: &str) -> io::Result<Selection> {
        let owner = self.reopen(owner_id)?;
        let selected = self.prepare_dependency_work(&owner, &owner)?;
        // Origin inspection may take a candidate's own short guard. Finish it before holding
        // the owner-only policy guard; hints are never admitted history.
        let hints = self.catalog_discovery_hints(&owner)?;
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&selected.roots)
            .map_err(error)?;
        self.select_dependency_read(&owner, work_id, hints, &guard)
    }

    pub(super) fn catalog_discovery_hints(
        &self,
        owner: &ProvisionedAttachment,
    ) -> io::Result<CatalogHints> {
        // An unavailable unrelated registration is not an input. Required keys must nevertheless
        // resolve uniquely below; omission never becomes a partial-history success.
        let mut hints: BTreeMap<String, Hint> = BTreeMap::new();
        let mut keys: BTreeMap<Key, Vec<String>> = BTreeMap::new();
        for registration in self.registrations()? {
            let hint = (|| {
                let candidate = self.reopen(registration.id())?;
                self.prepare_dependency_work(owner, &candidate)?
                    .catalog_selection_hint()
            })();
            if let Ok(hint) = hint {
                keys.entry(hint.0)
                    .or_default()
                    .push(registration.id().to_owned());
                hints.insert(registration.id().to_owned(), hint);
            }
        }
        Ok((hints, keys))
    }

    fn discover_dependency_read_held(
        &self,
        owner_id: &str,
        work_id: &str,
        guard: &crate::workspace_custody::WorkspaceInitializationGuard,
    ) -> io::Result<Selection> {
        let owner = self.reopen(owner_id)?;
        let hints = self.catalog_discovery_hints(&owner)?;
        self.select_dependency_read(&owner, work_id, hints, guard)
    }

    fn select_dependency_read(
        &self,
        owner: &ProvisionedAttachment,
        work_id: &str,
        hints: CatalogHints,
        guard: &crate::workspace_custody::WorkspaceInitializationGuard,
    ) -> io::Result<Selection> {
        let owner_id = owner.id();
        let selected_owner = self.prepare_dependency_work(owner, owner)?;
        guard.require_roots(&selected_owner.roots).map_err(error)?;
        let owner_read = owner
            .project()
            .read_configuration(owner.metadata_path(), &owner.store)?;
        let facts = owner_read
            .1
            .as_ref()
            .ok_or_else(|| invalid("catalog discovery requires native owner enrollment"))?
            .policy()
            .consumption_facts();
        let (hints, keys) = hints;
        let mut pending = BTreeSet::from([owner_id.to_owned(), work_id.to_owned()]);
        let mut selected = BTreeMap::new();
        while let Some(id) = pending.pop_first() {
            if selected.contains_key(&id) {
                continue;
            }
            if selected.len() >= 256 {
                return Err(invalid("discovered native input count exceeds bound"));
            }
            let (key, parents) = hints
                .get(&id)
                .ok_or_else(|| invalid("required native input is unavailable"))?;
            if keys.get(key).is_none_or(|matches| matches.len() != 1) {
                return Err(invalid("native input selection is ambiguous"));
            }
            selected.insert(id, *key);
            pending.extend(parents.iter().cloned());
            for fact in facts
                .iter()
                .filter(|fact| (fact.start.0, fact.start.1) == *key)
            {
                if fact.bindings.is_none() {
                    return Err(invalid("discovered consumption lacks native correlation"));
                }
                for input in std::iter::once(&fact.source).chain(&fact.inputs) {
                    let matches = keys
                        .get(&(input.0, input.1))
                        .ok_or_else(|| invalid("required consumed input is unavailable"))?;
                    let [input_id] = matches.as_slice() else {
                        return Err(invalid("consumed input selection is ambiguous"));
                    };
                    pending.insert(input_id.clone());
                }
            }
        }
        Ok(Selection {
            owner: owner_read,
            works: selected,
        })
    }
}
