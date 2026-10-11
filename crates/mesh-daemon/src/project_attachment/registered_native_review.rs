//! Trust-aware saved reads for an original retained registration; never writable admission.
use super::{AttachmentStorage, ProvisionedAttachment};
use crate::ipc::Json;
use std::io;

impl AttachmentStorage {
    // This bounded marker is only a format selector. Every chosen reader still verifies its
    // complete history. In particular Default native facts cannot select signed publication:
    // they intentionally refuse it before the configured-trust replay can begin.
    fn registered_saved_format(
        &self,
        selected: &ProvisionedAttachment,
    ) -> io::Result<(bool, String)> {
        let current = self.exact_registered_work(selected)?;
        let guard = crate::workspace_custody::lock_workspace_initialization(&current.store)
            .map_err(|e| io::Error::other(e.to_string()))?;
        let marker = super::dependency_enrollment::read_private_in_store(
            &current.store,
            super::history::HISTORY,
        )?;
        let parsed = Json::parse(&marker).map_err(|e| io::Error::other(e.to_string()))?;
        let native = match parsed.get("schema").and_then(Json::as_text) {
            Some("mesh.attachment-history/v3") => true,
            Some("mesh.attachment-history/v1" | "mesh.attachment-history/v2") => false,
            _ => return Err(super::invalid("registered saved-history format is unknown")),
        };
        guard
            .ensure_current()
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok((native, marker))
    }

    fn with_registered_saved_read<T>(
        &self,
        selected: &ProvisionedAttachment,
        native: impl FnOnce(&ProvisionedAttachment) -> io::Result<T>,
        ordinary: impl FnOnce() -> io::Result<T>,
    ) -> io::Result<T> {
        let current = self.exact_registered_work(selected)?;
        let format = self.registered_saved_format(selected)?;
        let result = if format.0 {
            native(&current)?
        } else {
            ordinary()?
        };
        if self.registered_saved_format(selected)? != format {
            return Err(super::invalid(
                "registered saved-history format changed during inspection",
            ));
        }
        self.exact_registered_work(selected)?;
        Ok(result)
    }

    /// List saved history through its original registration and configured native trust.
    /// Failed native replay never falls back to an ordinary reader or grants capture/run authority.
    pub fn registered_review_versions_with_trust(
        &self,
        selected: &ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<Vec<super::SavedAttachmentVersion>> {
        self.with_registered_saved_read(
            selected,
            |current| self.inspect_native_saved_versions(current.id(), trusted),
            || self.registered_review_versions(selected),
        )
    }

    /// Return the existing bounded saved-entry schema through complete native reconstruction.
    pub fn registered_review_entries_with_trust(
        &self,
        selected: &ProvisionedAttachment,
        operation: &str,
        after: Option<&str>,
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<Json> {
        self.with_registered_saved_read(
            selected,
            |current| {
                self.with_native_publication_history(current.id(), trusted, |history, _| {
                    history.saved_entries(operation, after)
                })
            },
            || self.registered_review_entries(selected, operation, after),
        )
    }

    /// Return bounded text or binary/large metadata, never mutable editor-file contents.
    pub fn registered_review_text_with_trust(
        &self,
        selected: &ProvisionedAttachment,
        operation: &str,
        path: &str,
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<Json> {
        self.with_registered_saved_read(
            selected,
            |current| {
                self.with_native_publication_history(current.id(), trusted, |history, _| {
                    history.saved_text(operation, path)
                })
            },
            || self.registered_review_text(selected, operation, path),
        )
    }

    /// Compare two authenticated saved points with the existing paging/selection bounds.
    pub fn registered_review_comparison_with_trust(
        &self,
        selected: &ProvisionedAttachment,
        base: &str,
        target: &str,
        selection: (Option<&str>, Option<&str>),
        trusted: &crate::TrustedReviewers,
    ) -> io::Result<Json> {
        self.with_registered_saved_read(
            selected,
            |current| {
                self.with_native_publication_history(current.id(), trusted, |history, _| {
                    history.saved_comparison(base, target, selection)
                })
            },
            || self.registered_review_comparison(selected, base, target, selection),
        )
    }
}
