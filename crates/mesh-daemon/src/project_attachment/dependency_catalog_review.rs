//! Bounded saved-review projections over complete native dependency history.
use super::{inspection, AttachmentStorage};
use crate::ipc::Json;
use std::io;

impl AttachmentStorage {
    /// Inspect one exact saved version using native owning-root and required-input discovery.
    pub fn registered_dependency_entries(
        &self,
        work_id: &str,
        operation: &str,
        after: Option<&str>,
    ) -> io::Result<Json> {
        self.with_registered_dependency_history(work_id, |_, history, _| {
            inspection::entries_history(history, operation, after)
        })
    }

    /// Preview bounded saved text; binary and large files retain the existing metadata-only form.
    pub fn registered_dependency_text(
        &self,
        work_id: &str,
        operation: &str,
        path: &str,
    ) -> io::Result<Json> {
        self.with_registered_dependency_history(work_id, |_, history, _| {
            inspection::text_history(history, operation, path)
        })
    }

    /// Compare exact saved versions with bounded paging or one selected changed path.
    /// Read visibility conveys no capture, execution or protected-main authority.
    pub fn registered_dependency_comparison(
        &self,
        work_id: &str,
        base: &str,
        target: &str,
        selection: (Option<&str>, Option<&str>),
    ) -> io::Result<Json> {
        self.with_registered_dependency_history(work_id, |_, history, _| {
            inspection::compare_history(history, base, target, selection)
        })
    }
}

impl AttachmentStorage {
    fn with_registered_review<T>(
        &self,
        selected: &super::ProvisionedAttachment,
        action: impl FnOnce(
            &super::ProvisionedAttachment,
            &crate::workspace::OpenWorkspace,
            &str,
        ) -> io::Result<T>,
    ) -> io::Result<T> {
        let current = self.exact_registered_work(selected)?;
        let native = {
            let _guard = crate::workspace_custody::lock_workspace_initialization(&current.store)
                .map_err(|error| io::Error::other(error.to_string()))?;
            current
                .project()
                .read_native_facts(current.metadata_path(), &current.store, None, None)?
                .1
                .is_some()
        };
        if native {
            return self.with_registered_dependency_history(
                current.id(),
                |work, history, configuration| {
                    self.exact_registered_work(selected)?;
                    let result = action(work, history, configuration)?;
                    self.exact_registered_work(selected)?;
                    Ok(result)
                },
            );
        }
        current.project().with_read_history(
            current.metadata_path(),
            current.store.clone(),
            &crate::TrustedReviewers::default(),
            |history, store, configuration| {
                self.exact_registered_work(selected)?;
                if current
                    .project()
                    .read_native_facts(current.metadata_path(), store, None, None)?
                    .1
                    .is_some()
                {
                    return Err(super::invalid(
                        "registered review format changed before inspection",
                    ));
                }
                let result = action(&current, history, configuration)?;
                self.exact_registered_work(selected)?;
                Ok(result)
            },
        )
    }

    /// Read versions for a retained native registration, including ordinary existing projects.
    /// Dependency failures never fall back to independent history.
    pub fn registered_review_versions(
        &self,
        selected: &super::ProvisionedAttachment,
    ) -> io::Result<Vec<super::SavedAttachmentVersion>> {
        self.with_registered_review(selected, |work, history, configuration| {
            let line = super::capture_line::CaptureLine::load(&work.store, history, configuration)?;
            history
                .linear_history(line.head)
                .map_err(|error| io::Error::other(error.to_string()))?
                .into_iter()
                .map(|operation| {
                    super::SavedAttachmentVersion::from_verified_history(history, operation)
                })
                .collect()
        })
    }

    /// Inspect exact saved entries through the retained registration and complete native history.
    pub fn registered_review_entries(
        &self,
        selected: &super::ProvisionedAttachment,
        operation: &str,
        after: Option<&str>,
    ) -> io::Result<Json> {
        self.with_registered_review(selected, |_, history, _| {
            inspection::entries_history(history, operation, after)
        })
    }

    /// Preview exact saved text through the retained registration, never current editor files.
    pub fn registered_review_text(
        &self,
        selected: &super::ProvisionedAttachment,
        operation: &str,
        path: &str,
    ) -> io::Result<Json> {
        self.with_registered_review(selected, |_, history, _| {
            inspection::text_history(history, operation, path)
        })
    }

    /// Compare exact saved versions through one complete guarded history.
    pub fn registered_review_comparison(
        &self,
        selected: &super::ProvisionedAttachment,
        base: &str,
        target: &str,
        selection: (Option<&str>, Option<&str>),
    ) -> io::Result<Json> {
        self.with_registered_review(selected, |_, history, _| {
            inspection::compare_history(history, base, target, selection)
        })
    }
}
